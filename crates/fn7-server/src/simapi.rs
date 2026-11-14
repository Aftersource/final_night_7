//! The simulator API (`/sim/*`): balance simulations with any room settings, run server-side, for agents that don't
//! build this code (the bridge agent; see BUILDLOG "the bridge agent's role" and docs/AGENT_GUIDE.md §12).
//!
//! | Route | What |
//! |---|---|
//! | `POST /sim/run` | queue matches (`settings`, `seeds` ≤ 64, `players`, `beats`, `society_only`, `llm`, `budget_usd`, `record`) → `{job_id}` |
//! | `POST /sim/compare` | the same seeds under settings A and B (`settings` shared, `a` / `b` overrides) → `{job_id}` |
//! | `GET /sim/jobs` | recent jobs, newest first, and the pool's load |
//! | `GET /sim/jobs/{id}` | status, per-seed results, aggregates (and A/B deltas) |
//! | `GET /sim/jobs/{id}/recording/{seed}[?arm=a\|b]` | a recorded seed's JSONL (the core's format) |
//! | `GET /sim/settings` | every room setting: default, range, description (generated from settings.rs) |
//!
//! Auth is the engine's admin token (`THEATRE_ADMIN_TOKEN`, `Authorization: Bearer <token>`): every route answers 404
//! when it is unset and 401 when the header is missing or wrong. Query parameters (the tunnel's `?key=`) are ignored.
//!
//! Matches run in-process through the `GamePlugin` trait exactly as `fn7-sim` runs them ([`stats::run_seed`]), on a
//! fixed pool of OS threads (`FN7_SIM_WORKERS`, default 2), each with its own single-threaded Tokio runtime, so they
//! never run on the server's runtime and never block a room; extra runs wait in a bounded queue. Rules-only results
//! are deterministic (same settings + seed + players → the same result, `timing` aside). Every job is logged
//! (`tracing` target `fn7_sim`) and written as `sim_job_submitted` / `sim_job_finished` trace events to
//! `<FN7_SIM_LOG_DIR>/fn7-sim-jobs.jsonl` (who, settings hash, seeds, duration).

pub mod settings_doc;
pub mod stats;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fn7_plugin::settings::Settings;
use serde_json::{json, Map, Value};
use theatre_plugin_api::{LlmProvider, LlmRequest, LlmResponse};
use theatre_server::axum::body::{Body, Bytes};
use theatre_server::axum::extract::{Path, Query, State};
use theatre_server::axum::http::{header, HeaderMap, StatusCode};
use theatre_server::axum::response::{IntoResponse, Response};
use theatre_server::axum::routing::{get, post};
use theatre_server::axum::{Json, Router};
use theatre_server::recording::Recorder;
use theatre_trace::{JsonlSink, Level, Sink};

/// The LLM a job with `llm: true` uses, and how much it has cost so far (dollars).
pub struct LlmBackend {
    pub llm: Arc<dyn LlmProvider>,
    pub label: String,
    pub cost_usd: Arc<dyn Fn() -> f64 + Send + Sync>,
}

/// Builds a fresh [`LlmBackend`] per job (its own cost counter).
pub type LlmFactory = Arc<dyn Fn() -> anyhow::Result<LlmBackend> + Send + Sync>;

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_seeds: usize,
    pub max_beats: u32,
    /// Runs (seed × arm) waiting or running across all jobs.
    pub max_queued_runs: usize,
    pub max_budget_usd: f64,
    /// Jobs kept in memory (oldest finished ones are dropped first).
    pub keep_jobs: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_seeds: 64,
            max_beats: 500,
            max_queued_runs: 512,
            max_budget_usd: 1.0,
            keep_jobs: 200,
        }
    }
}

pub struct SimConfig {
    /// `THEATRE_ADMIN_TOKEN`; `None` = the API doesn't exist (404).
    pub admin_token: Option<String>,
    /// Concurrent simulations (worker threads).
    pub workers: usize,
    /// Where `record: true` writes `<run>.jsonl`.
    pub record_dir: PathBuf,
    /// Where the job log (`fn7-sim-jobs.jsonl`) goes; `None` = tracing only.
    pub log_dir: Option<PathBuf>,
    /// `llm: true` backend; `None` = refused.
    pub llm: Option<LlmFactory>,
    pub limits: Limits,
}

impl SimConfig {
    /// From the server's config and env: `FN7_SIM_WORKERS` (default 2, max 16), `FN7_SIM_RECORD_DIR` (default: the
    /// core's recording dir, so `/admin/matches/<run>/…` reads sim recordings too; else `runs/sim/recordings`),
    /// `FN7_SIM_LOG_DIR` (default `runs/sim`).
    pub fn from_env(cfg: &theatre_server::Config, core_record_dir: Option<PathBuf>, llm: Option<LlmFactory>) -> Self {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        Self {
            admin_token: cfg.admin_token.clone().filter(|t| !t.is_empty()),
            workers: env("FN7_SIM_WORKERS").and_then(|v| v.parse().ok()).unwrap_or(2).clamp(1, 16),
            record_dir: env("FN7_SIM_RECORD_DIR")
                .map(PathBuf::from)
                .or(core_record_dir)
                .unwrap_or_else(|| PathBuf::from("runs/sim/recordings")),
            log_dir: Some(env("FN7_SIM_LOG_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("runs/sim"))),
            llm,
            limits: Limits::default(),
        }
    }
}

/// `llm: true` through Claude like the server's own backend (same key, model, timeouts), with a fresh usage counter
/// per job; `None` when the server isn't on Claude (`label` from [`crate::llm::Built`]).
pub fn claude_factory(cfg: &theatre_server::Config, label: &str) -> Option<LlmFactory> {
    let model = label.strip_prefix("claude:")?.to_string();
    let (pi, po) = crate::anthropic::price_per_mtok(&model)?;
    let timeout = Duration::from_secs_f64(cfg.llm_timeout_secs.max(30.0));
    let (conc, retries) = (cfg.llm_max_concurrency, cfg.llm_retries);
    Some(Arc::new(move || {
        let b = crate::llm::claude(&model, timeout, conc, retries)?
            .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY is not set"))?;
        let usage = b.usage.ok_or_else(|| anyhow::anyhow!("no usage counter"))?;
        Ok(LlmBackend {
            llm: b.llm,
            label: b.label,
            cost_usd: Arc::new(move || {
                let (_, i, o, _) = usage.snapshot();
                i as f64 / 1e6 * pi + o as f64 / 1e6 * po
            }),
        })
    }))
}

/// Refuses calls once the job's cost reaches its budget (the mind then uses its rules fallback). Calls already in
/// flight when the budget is reached still finish, so the final cost can pass the budget by those.
struct Budgeted {
    inner: Arc<dyn LlmProvider>,
    cost: Arc<dyn Fn() -> f64 + Send + Sync>,
    budget: f64,
    calls: AtomicU64,
    refused: AtomicU64,
    label: String,
}

#[async_trait]
impl LlmProvider for Budgeted {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        if (self.cost)() >= self.budget {
            self.refused.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!("simulation budget of ${} reached", self.budget);
        }
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.complete(req).await
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Jobs

enum Slot {
    Queued,
    Running,
    Done(Value),
}

struct Arm {
    name: &'static str,
    settings: Value,
    hash: String,
    slots: Vec<Slot>,
}

struct Job {
    id: String,
    kind: &'static str,
    who: Value,
    seeds: Vec<u64>,
    players: u8,
    society_only: bool,
    record: bool,
    arms: Vec<Arm>,
    llm: Option<Arc<Budgeted>>,
    created_ms: i64,
    started_ms: Option<i64>,
    finished_ms: Option<i64>,
    started: Option<Instant>,
    duration_ms: Option<f64>,
}

impl Job {
    fn total(&self) -> usize {
        self.arms.iter().map(|a| a.slots.len()).sum()
    }
    fn count(&self, f: impl Fn(&Slot) -> bool) -> usize {
        self.arms.iter().flat_map(|a| a.slots.iter()).filter(|s| f(s)).count()
    }
    fn done(&self) -> usize {
        self.count(|s| matches!(s, Slot::Done(_)))
    }
    fn failed(&self) -> usize {
        self.count(|s| matches!(s, Slot::Done(v) if v.get("error").is_some()))
    }
    fn status(&self) -> &'static str {
        if self.finished_ms.is_some() {
            if self.failed() == self.total() {
                "failed"
            } else {
                "done"
            }
        } else if self.started_ms.is_some() {
            "running"
        } else {
            "queued"
        }
    }
    fn hash(&self) -> String {
        self.arms.iter().map(|a| a.hash.as_str()).collect::<Vec<_>>().join("/")
    }
    fn llm_view(&self) -> Value {
        match &self.llm {
            None => Value::Null,
            Some(b) => json!({
                "backend": b.label, "budget_usd": b.budget, "cost_usd": ((b.cost)() * 10000.0).round() / 10000.0,
                "calls": b.calls.load(Ordering::Relaxed), "refused_over_budget": b.refused.load(Ordering::Relaxed),
            }),
        }
    }
    fn summary(&self) -> Value {
        json!({
            "job_id": self.id, "kind": self.kind, "status": self.status(),
            "created_at_ms": self.created_ms, "finished_at_ms": self.finished_ms, "duration_ms": self.duration_ms,
            "progress": { "done": self.done(), "total": self.total(), "failed": self.failed() },
            "seeds": self.seeds.len(), "settings_hash": self.hash(), "llm": self.llm.is_some(),
            "client": self.who.get("client").cloned().unwrap_or(Value::Null),
        })
    }
    fn arm_view(&self, a: &Arm) -> Value {
        let results: Vec<Value> = a
            .slots
            .iter()
            .zip(&self.seeds)
            .map(|(s, seed)| match s {
                Slot::Queued => json!({ "seed": seed, "status": "queued" }),
                Slot::Running => json!({ "seed": seed, "status": "running" }),
                Slot::Done(v) => v.clone(),
            })
            .collect();
        let done: Vec<Value> = a
            .slots
            .iter()
            .filter_map(|s| match s {
                Slot::Done(v) => Some(v.clone()),
                _ => None,
            })
            .collect();
        json!({
            "settings": a.settings, "settings_hash": a.hash,
            "aggregate": if done.is_empty() { Value::Null } else { stats::aggregate(&done) },
            "results": results,
        })
    }
    fn view(&self) -> Value {
        let mut v = self.summary();
        let o = v.as_object_mut().expect("object");
        o.insert("started_at_ms".into(), json!(self.started_ms));
        o.insert(
            "request".into(),
            json!({ "seeds": self.seeds, "players": self.players, "society_only": self.society_only,
                    "record": self.record, "llm": self.llm.is_some() }),
        );
        o.insert("who".into(), self.who.clone());
        if self.llm.is_some() {
            o.insert("llm".into(), self.llm_view());
        }
        if self.kind == "run" {
            if let Value::Object(m) = self.arm_view(&self.arms[0]) {
                o.extend(m);
            }
        } else {
            let a = self.arm_view(&self.arms[0]);
            let b = self.arm_view(&self.arms[1]);
            if !a["aggregate"].is_null() && !b["aggregate"].is_null() {
                o.insert("delta".into(), stats::delta(&a["aggregate"], &b["aggregate"]));
            }
            o.insert("a".into(), a);
            o.insert("b".into(), b);
        }
        v
    }
}

struct Task {
    job: Arc<Mutex<Job>>,
    arm: usize,
    ix: usize,
}

struct Inner {
    cfg: SimConfig,
    jobs: Mutex<VecDeque<Arc<Mutex<Job>>>>,
    tx: Mutex<mpsc::Sender<Task>>,
    /// Runs queued or running.
    pending: AtomicUsize,
    running: AtomicUsize,
    recorder: OnceLock<Option<Recorder>>,
    log: OnceLock<Option<Arc<JsonlSink>>>,
}

impl Inner {
    fn recorder(&self) -> Option<&Recorder> {
        self.recorder
            .get_or_init(|| match crate::record::recorder(&self.cfg.record_dir, Level::Events) {
                Ok(r) => Some(r),
                Err(e) => {
                    tracing::warn!(target: "fn7_sim", dir = %self.cfg.record_dir.display(), error = %e, "sim recording unavailable");
                    None
                }
            })
            .as_ref()
    }

    /// A job log line: `tracing` (target `fn7_sim`) and a trace event in `fn7-sim-jobs.jsonl`.
    fn log(&self, kind: &str, job: &Job) {
        let data = json!({
            "job_id": job.id, "kind": job.kind, "who": job.who, "settings_hash": job.hash(), "seeds": job.seeds,
            "players": job.players, "society_only": job.society_only, "record": job.record,
            "llm": job.llm_view(), "settings": job.arms.iter().map(|a| json!({ "arm": a.name, "settings": a.settings })).collect::<Vec<_>>(),
            "duration_ms": job.duration_ms, "status": job.status(), "failed": job.failed(),
        });
        tracing::info!(target: "fn7_sim", event = kind, job = %job.id, kind = job.kind, settings_hash = %job.hash(),
                       seeds = ?job.seeds, who = %job.who, duration_ms = ?job.duration_ms, status = job.status(), "sim job");
        let sink = self.log.get_or_init(|| {
            let dir = self.cfg.log_dir.as_ref()?;
            JsonlSink::new(dir).ok()
        });
        if let Some(sink) = sink {
            let ctx = theatre_trace::Ctx::new(sink.clone() as Arc<dyn Sink>, "fn7-sim-jobs");
            let actor = job.who["client"].as_str().unwrap_or("bridge").to_string();
            theatre_trace::within_sync(ctx, || theatre_trace::event(kind, Some(&actor), data));
        }
    }

    fn find(&self, id: &str) -> Option<Arc<Mutex<Job>>> {
        self.jobs
            .lock()
            .expect("jobs")
            .iter()
            .find(|j| j.lock().expect("job").id == id)
            .cloned()
    }
}

/// The `/sim/*` router (state applied), workers started. Merge it into the server's extra routes.
pub fn router(cfg: SimConfig) -> Router {
    let (tx, rx) = mpsc::channel::<Task>();
    let rx = Arc::new(Mutex::new(rx));
    let workers = cfg.workers.max(1);
    let inner = Arc::new(Inner {
        cfg,
        jobs: Mutex::new(VecDeque::new()),
        tx: Mutex::new(tx),
        pending: AtomicUsize::new(0),
        running: AtomicUsize::new(0),
        recorder: OnceLock::new(),
        log: OnceLock::new(),
    });
    for n in 0..workers {
        let (rx, inner) = (rx.clone(), Arc::downgrade(&inner));
        std::thread::Builder::new()
            .name(format!("fn7-sim-{n}"))
            .spawn(move || worker(rx, inner))
            .expect("spawn sim worker");
    }
    Router::new()
        .route("/sim/run", post(run))
        .route("/sim/compare", post(compare))
        .route("/sim/jobs", get(list))
        .route("/sim/jobs/{id}", get(job))
        .route("/sim/jobs/{id}/recording/{seed}", get(recording))
        .route("/sim/settings", get(settings))
        .with_state(inner)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("sim runtime")
}

fn worker(rx: Arc<Mutex<mpsc::Receiver<Task>>>, inner: std::sync::Weak<Inner>) {
    let mut rt = runtime();
    loop {
        let task = match rx.lock() {
            Ok(r) => match r.recv() {
                Ok(t) => t,
                Err(_) => return,
            },
            Err(_) => return,
        };
        let Some(inner) = inner.upgrade() else { return };
        inner.running.fetch_add(1, Ordering::Relaxed);
        let (settings, players, llm, record, seed, job_id, arm_name) = {
            let mut j = task.job.lock().expect("job");
            if j.started_ms.is_none() {
                j.started_ms = Some(theatre_trace::now_ms());
                j.started = Some(Instant::now());
            }
            let seed = j.seeds[task.ix];
            let mut s = j.arms[task.arm].settings.clone();
            s["seed"] = seed.into();
            j.arms[task.arm].slots[task.ix] = Slot::Running;
            let llm: Arc<dyn LlmProvider> = match &j.llm {
                Some(b) => b.clone(),
                None => Arc::new(fn7_plugin::testkit::Failing),
            };
            (s, j.players, llm, j.record, seed, j.id.clone(), j.arms[task.arm].name)
        };
        let rec = if record { inner.recorder() } else { None };
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rt.block_on(stats::run_seed(&settings, players, llm, rec))
        }));
        let mut out = match res {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => json!({ "seed": seed, "error": format!("{e:#}") }),
            Err(_) => {
                rt = runtime();
                json!({ "seed": seed, "error": "the simulation panicked" })
            }
        };
        if out.get("recording").is_some() {
            let q = if arm_name == "run" { String::new() } else { format!("?arm={arm_name}") };
            out["recording"]["download"] = format!("/sim/jobs/{job_id}/recording/{seed}{q}").into();
        }
        inner.running.fetch_sub(1, Ordering::Relaxed);
        inner.pending.fetch_sub(1, Ordering::Relaxed);
        let mut j = task.job.lock().expect("job");
        j.arms[task.arm].slots[task.ix] = Slot::Done(out);
        if j.done() == j.total() && j.finished_ms.is_none() {
            j.finished_ms = Some(theatre_trace::now_ms());
            j.duration_ms = j.started.map(|t| (t.elapsed().as_secs_f64() * 1000.0).round());
            inner.log("sim_job_finished", &j);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// HTTP

/// An error answer: `{"error": "..."}` with its status.
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

type ApiResult = Result<Response, ApiError>;

fn err(code: StatusCode, msg: impl Into<String>) -> ApiError {
    ApiError(code, msg.into())
}

fn bad(msg: impl Into<String>) -> ApiError {
    err(StatusCode::BAD_REQUEST, msg)
}

/// The engine's admin-token check (same token, same answers: 404 unset, 401 wrong).
fn authorize(inner: &Inner, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(token) = inner.cfg.admin_token.as_deref() else {
        return Err(err(StatusCode::NOT_FOUND, "not found"));
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let ok = given.len() == token.len()
        && given.bytes().zip(token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
    if ok {
        Ok(())
    } else {
        Err(err(StatusCode::UNAUTHORIZED, "bad admin token"))
    }
}

/// FNV-1a 64 of the canonical (key-sorted) JSON: a stable settings fingerprint.
pub fn settings_hash(v: &Value) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in v.to_string().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn who(headers: &HeaderMap, body: &Map<String, Value>) -> Value {
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).map(|s| s.chars().take(200).collect::<String>());
    let client = body
        .get("client")
        .and_then(|c| c.as_str())
        .map(|s| s.chars().take(64).collect::<String>())
        .or_else(|| h("x-sim-client"));
    json!({ "client": client, "user_agent": h("user-agent"), "forwarded_for": h("x-forwarded-for") })
}

const RUN_FIELDS: &[&str] = &["settings", "seeds", "players", "beats", "society_only", "llm", "budget_usd", "record", "client"];

/// Merge, check and validate one arm's settings (the request's `players` / `beats` / `society_only` / `llm` win).
fn arm_settings(
    base: Option<&Value>,
    over: Option<&Value>,
    body: &Map<String, Value>,
    llm: bool,
    limits: &Limits,
) -> Result<(Value, u8), String> {
    let known = settings_doc::known_keys();
    let mut m = Map::new();
    for (name, src) in [("settings", base), ("a/b", over)] {
        match src {
            None | Some(Value::Null) => {}
            Some(Value::Object(o)) => {
                for (k, v) in o {
                    if let Some((_, note)) = settings_doc::SET_BY_REQUEST.iter().find(|(x, _)| x == k) {
                        return Err(format!("`{k}` can't be set in settings: {note}"));
                    }
                    if !known.contains(k) {
                        return Err(format!("unknown setting `{k}` (see GET /sim/settings)"));
                    }
                    m.insert(k.clone(), v.clone());
                }
            }
            Some(_) => return Err(format!("`{name}` must be an object of room settings")),
        }
    }
    if let Some(p) = body.get("players").filter(|v| !v.is_null()) {
        let p = p.as_u64().filter(|p| (1..=8).contains(p)).ok_or("`players` must be an integer 1–8")?;
        m.insert("players".into(), p.into());
    }
    if let Some(b) = body.get("beats").filter(|v| !v.is_null()) {
        let b = b
            .as_u64()
            .filter(|b| (1..=limits.max_beats as u64).contains(b))
            .ok_or(format!("`beats` must be an integer 1–{}", limits.max_beats))?;
        m.remove("rounds");
        m.insert("beats_total".into(), b.into());
    }
    if let Some(s) = body.get("society_only").filter(|v| !v.is_null()) {
        m.insert("society_only".into(), s.as_bool().ok_or("`society_only` must be true/false")?.into());
    }
    m.insert("llm".into(), llm.into());
    let v = Settings::validate(&Value::Object(m)).map_err(|e| format!("settings: {e}"))?;
    let s = Settings::from_value(&v);
    if s.beats_total > limits.max_beats {
        return Err(format!("beats_total {} is over the simulator's cap of {}", s.beats_total, limits.max_beats));
    }
    Ok((v, s.players as u8))
}

struct Parsed {
    seeds: Vec<u64>,
    llm: bool,
    budget: Option<f64>,
    record: bool,
}

fn parse_common(body: &Map<String, Value>, limits: &Limits) -> Result<Parsed, String> {
    let seeds = body
        .get("seeds")
        .and_then(|s| s.as_array())
        .ok_or("`seeds` must be a non-empty array of integers")?;
    if seeds.is_empty() {
        return Err("`seeds` must be a non-empty array of integers".into());
    }
    if seeds.len() > limits.max_seeds {
        return Err(format!("at most {} seeds per job ({} given)", limits.max_seeds, seeds.len()));
    }
    let mut out: Vec<u64> = vec![];
    for s in seeds {
        let s = s.as_u64().ok_or("every seed must be a non-negative integer")?;
        if out.contains(&s) {
            return Err(format!("seed {s} is listed twice"));
        }
        out.push(s);
    }
    let flag = |k: &str| -> Result<bool, String> {
        match body.get(k) {
            None | Some(Value::Null) => Ok(false),
            Some(v) => v.as_bool().ok_or(format!("`{k}` must be true/false")),
        }
    };
    let llm = flag("llm")?;
    let record = flag("record")?;
    let budget = match body.get("budget_usd") {
        None | Some(Value::Null) => None,
        Some(b) => Some(b.as_f64().ok_or("`budget_usd` must be a number")?),
    };
    match (llm, budget) {
        (true, None) => return Err("`llm: true` needs an explicit `budget_usd` (at most 1.0)".into()),
        (true, Some(b)) if !(b > 0.0 && b <= limits.max_budget_usd) => {
            return Err(format!("`budget_usd` must be > 0 and at most {}", limits.max_budget_usd))
        }
        (false, Some(_)) => return Err("`budget_usd` only applies with `llm: true`".into()),
        _ => {}
    }
    if let Some(c) = body.get("client").filter(|c| !c.is_null() && !c.is_string()) {
        return Err(format!("`client` must be a string, not {c}"));
    }
    Ok(Parsed { seeds: out, llm, budget, record })
}

fn parse_body(bytes: &Bytes) -> Result<Map<String, Value>, ApiError> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(bad("the body must be a JSON object")),
        Err(e) => Err(bad(format!("invalid JSON: {e}"))),
    }
}

fn submit(inner: &Arc<Inner>, kind: &'static str, headers: &HeaderMap, body: Map<String, Value>) -> ApiResult {
    let allowed: Vec<&str> = match kind {
        "run" => RUN_FIELDS.to_vec(),
        _ => RUN_FIELDS.iter().copied().chain(["a", "b"]).collect(),
    };
    if let Some(k) = body.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(bad(format!("unknown field `{k}` (allowed: {})", allowed.join(", "))));
    }
    let limits = &inner.cfg.limits;
    let p = parse_common(&body, limits).map_err(bad)?;
    let arms: Vec<(&'static str, Option<&Value>)> = match kind {
        "run" => vec![("run", None)],
        _ => {
            for k in ["a", "b"] {
                if !body.get(k).is_some_and(|v| v.is_object()) {
                    return Err(bad(format!("`{k}` must be an object of settings overrides (may be {{}})")));
                }
            }
            vec![("a", body.get("a")), ("b", body.get("b"))]
        }
    };
    let mut built = vec![];
    let mut players = 0u8;
    for (name, over) in &arms {
        let (s, pl) = arm_settings(body.get("settings"), *over, &body, p.llm, limits)
            .map_err(|e| bad(if kind == "run" { e } else { format!("{name}: {e}") }))?;
        players = pl;
        built.push((*name, s));
    }
    if kind == "compare" && built[0].1 == built[1].1 {
        return Err(bad("`a` and `b` give the same settings"));
    }
    let runs = built.len() * p.seeds.len();
    let pending = inner.pending.load(Ordering::Relaxed);
    if pending + runs > limits.max_queued_runs {
        return Err(err(
            StatusCode::TOO_MANY_REQUESTS,
            format!("the simulator queue is full ({pending} runs waiting, at most {}); retry later", limits.max_queued_runs),
        ));
    }
    let llm = match (p.llm, p.budget) {
        (true, Some(budget)) => {
            let Some(f) = inner.cfg.llm.as_ref() else {
                return Err(bad("`llm: true` needs the server's Claude backend, which this server doesn't have"));
            };
            let b = f().map_err(|e| bad(format!("llm backend: {e:#}")))?;
            Some(Arc::new(Budgeted {
                inner: b.llm,
                cost: b.cost_usd,
                budget,
                calls: AtomicU64::new(0),
                refused: AtomicU64::new(0),
                label: b.label,
            }))
        }
        _ => None,
    };
    let society_only = built[0].1["society_only"].as_bool().unwrap_or(false);
    let job = Job {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        who: who(headers, &body),
        players,
        society_only,
        record: p.record,
        arms: built
            .into_iter()
            .map(|(name, s)| Arm {
                name,
                hash: settings_hash(&s),
                settings: s,
                slots: p.seeds.iter().map(|_| Slot::Queued).collect(),
            })
            .collect(),
        seeds: p.seeds,
        llm,
        created_ms: theatre_trace::now_ms(),
        started_ms: None,
        finished_ms: None,
        started: None,
        duration_ms: None,
    };
    inner.log("sim_job_submitted", &job);
    let reply = json!({ "job_id": job.id, "status": "queued", "runs": runs, "settings_hash": job.hash(),
                        "poll": format!("/sim/jobs/{}", job.id) });
    let n_arms = job.arms.len();
    let n_seeds = job.seeds.len();
    let job = Arc::new(Mutex::new(job));
    {
        let mut jobs = inner.jobs.lock().expect("jobs");
        jobs.push_back(job.clone());
        while jobs.len() > limits.keep_jobs {
            match jobs.iter().position(|j| j.lock().expect("job").finished_ms.is_some()) {
                Some(i) => {
                    jobs.remove(i);
                }
                None => break,
            }
        }
    }
    inner.pending.fetch_add(runs, Ordering::Relaxed);
    let tx = inner.tx.lock().expect("tx");
    // Seed-major order, so a compare's A and B of one seed run side by side.
    for ix in 0..n_seeds {
        for arm in 0..n_arms {
            let _ = tx.send(Task { job: job.clone(), arm, ix });
        }
    }
    Ok(Json(reply).into_response())
}

async fn run(State(inner): State<Arc<Inner>>, headers: HeaderMap, body: Bytes) -> ApiResult {
    authorize(&inner, &headers)?;
    let body = parse_body(&body)?;
    submit(&inner, "run", &headers, body)
}

async fn compare(State(inner): State<Arc<Inner>>, headers: HeaderMap, body: Bytes) -> ApiResult {
    authorize(&inner, &headers)?;
    let body = parse_body(&body)?;
    submit(&inner, "compare", &headers, body)
}

async fn list(State(inner): State<Arc<Inner>>, headers: HeaderMap) -> ApiResult {
    authorize(&inner, &headers)?;
    let jobs: Vec<Value> = inner
        .jobs
        .lock()
        .expect("jobs")
        .iter()
        .rev()
        .map(|j| j.lock().expect("job").summary())
        .collect();
    Ok(Json(json!({
        "jobs": jobs,
        "workers": inner.cfg.workers,
        "running_runs": inner.running.load(Ordering::Relaxed),
        "queued_runs": inner.pending.load(Ordering::Relaxed).saturating_sub(inner.running.load(Ordering::Relaxed)),
        "limits": limits_json(&inner.cfg.limits),
    }))
    .into_response())
}

fn limits_json(l: &Limits) -> Value {
    json!({ "max_seeds": l.max_seeds, "max_beats": l.max_beats, "max_queued_runs": l.max_queued_runs,
            "max_budget_usd": l.max_budget_usd, "players": [1, 8] })
}

async fn job(State(inner): State<Arc<Inner>>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult {
    authorize(&inner, &headers)?;
    let j = inner.find(&id).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such job"))?;
    let v = j.lock().expect("job").view();
    Ok(Json(v).into_response())
}

async fn recording(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    Path((id, seed)): Path<(String, String)>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> ApiResult {
    authorize(&inner, &headers)?;
    let seed: u64 = seed.parse().map_err(|_| bad("bad seed"))?;
    let j = inner.find(&id).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such job"))?;
    let file = {
        let j = j.lock().expect("job");
        let arm = match q.get("arm").map(String::as_str) {
            None => &j.arms[0],
            Some(a) => j.arms.iter().find(|x| x.name == a).ok_or_else(|| bad("`arm` must be a or b"))?,
        };
        let ix = j.seeds.iter().position(|s| *s == seed).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such seed in this job"))?;
        match &arm.slots[ix] {
            Slot::Done(v) => v["recording"]["file"].as_str().map(str::to_string),
            _ => return Err(err(StatusCode::CONFLICT, "that seed hasn't finished")),
        }
    };
    let file = file.ok_or_else(|| err(StatusCode::NOT_FOUND, "that seed wasn't recorded (send `record: true`)"))?;
    // The file name is a uuid the server made; no path from the request reaches the filesystem.
    let bytes = tokio::fs::read(inner.cfg.record_dir.join(&file))
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "the recording file is gone"))?;
    Ok((
        [(header::CONTENT_TYPE, "application/x-ndjson"), (header::CONTENT_DISPOSITION, "attachment")],
        Body::from(bytes),
    )
        .into_response())
}

async fn settings(State(inner): State<Arc<Inner>>, headers: HeaderMap) -> ApiResult {
    authorize(&inner, &headers)?;
    let mut v = settings_doc::listing();
    v["limits"] = limits_json(&inner.cfg.limits);
    Ok(Json(v).into_response())
}
