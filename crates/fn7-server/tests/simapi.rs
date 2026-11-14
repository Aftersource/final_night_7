//! The simulator API (`/sim/*`, `fn7_server::simapi`): auth (404 unset / 401 bad / 200 good, `?key=` tolerated),
//! run + poll, determinism, compare, bounds (400s), the settings listing against `Settings::validate`, `llm: true`
//! through a fake provider with a budget, recordings, and that simulations leave the server's runtime responsive.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fn7_plugin::settings::Settings;
use fn7_server::simapi::{self, stats, LlmBackend, LlmFactory, SimConfig};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use theatre_plugin_api::{LlmProvider, LlmRequest, LlmResponse};
use theatre_server::axum::body::Body;
use theatre_server::axum::http::{Request, StatusCode};
use theatre_server::axum::Router;
use tower::ServiceExt;

const TOKEN: &str = "test-admin-token";

fn dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("fn7-simapi-{}-{name}", std::process::id()))
}

fn config(name: &str, token: Option<&str>, llm: Option<LlmFactory>) -> SimConfig {
    SimConfig {
        admin_token: token.map(str::to_string),
        workers: 2,
        record_dir: dir(name).join("recordings"),
        log_dir: Some(dir(name)),
        llm,
        limits: Default::default(),
    }
}

async fn call(app: &Router, method: &str, uri: &str, auth: Option<&str>, body: Option<Value>) -> (StatusCode, Value, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = auth {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, v, bytes)
}

async fn submit(app: &Router, path: &str, body: Value) -> String {
    let (s, v, _) = call(app, "POST", &format!("{path}?key=tunnel-key"), Some(TOKEN), Some(body)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["job_id"].as_str().unwrap().to_string()
}

async fn wait(app: &Router, id: &str) -> Value {
    let t = Instant::now();
    loop {
        let (s, v, _) = call(app, "GET", &format!("/sim/jobs/{id}?key=k"), Some(TOKEN), None).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["status"] == "done" || v["status"] == "failed" {
            return v;
        }
        assert!(t.elapsed() < Duration::from_secs(600), "job {id} never finished: {v}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

const SMALL: fn() -> Value = || json!({ "clusters": 16 });

#[tokio::test]
async fn auth_404_unset_401_bad_200_good() {
    let off = simapi::router(config("auth-off", None, None));
    for (m, uri) in [("GET", "/sim/settings"), ("GET", "/sim/jobs"), ("GET", "/sim/jobs/x"), ("POST", "/sim/run")] {
        let (s, _, _) = call(&off, m, uri, Some(TOKEN), Some(json!({}))).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {uri} with the token unset");
    }
    let on = simapi::router(config("auth-on", Some(TOKEN), None));
    for (m, uri) in [("GET", "/sim/settings"), ("GET", "/sim/jobs"), ("POST", "/sim/run"), ("POST", "/sim/compare")] {
        assert_eq!(call(&on, m, uri, None, None).await.0, StatusCode::UNAUTHORIZED, "{m} {uri} no header");
        assert_eq!(call(&on, m, uri, Some("wrong"), None).await.0, StatusCode::UNAUTHORIZED, "{m} {uri} bad token");
    }
    let (s, v, _) = call(&on, "GET", "/sim/settings?key=tunnel-key", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["settings"].as_array().unwrap().len() > 20);
    let (s, v, _) = call(&on, "GET", "/sim/jobs?key=tunnel-key", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["workers"], 2);
    // Auth comes before body parsing: a bad body with a bad token is a 401, not a 400.
    let mut req = Request::post("/sim/run").header("authorization", "Bearer nope").body(Body::from("{not json")).unwrap();
    req.headers_mut().insert("content-type", "application/json".parse().unwrap());
    assert_eq!(on.clone().oneshot(req).await.unwrap().status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn run_poll_and_determinism() {
    let app = simapi::router(config("run", Some(TOKEN), None));
    let body = json!({ "settings": SMALL(), "seeds": [7, 8], "beats": 12, "client": "test" });
    let id = submit(&app, "/sim/run", body.clone()).await;
    let v = wait(&app, &id).await;
    assert_eq!(v["status"], "done", "{v}");
    assert_eq!(v["kind"], "run");
    assert_eq!(v["progress"], json!({ "done": 2, "total": 2, "failed": 0 }));
    assert_eq!(v["who"]["client"], "test");
    assert!(v["duration_ms"].as_f64().is_some());
    let r = &v["results"][0];
    assert_eq!(r["seed"], 7);
    for k in ["champion", "beats_played", "eliminations", "eliminations_by_cause", "eliminations_by_cause_kind", "casts", "entropy", "slow", "debts", "society", "llm", "timing"] {
        assert!(r.get(k).is_some(), "result lacks `{k}`: {r}");
    }
    assert!(r["beats_played"].as_u64().unwrap() >= 1 && r["beats_played"].as_u64().unwrap() <= 12);
    assert!(r["entropy"]["curve"].as_array().unwrap().len() <= stats::CURVE_POINTS);
    assert!(r["society"]["people_emerged_by_role"].is_object());
    assert!(r["timing"]["beat_ms"]["mean"].as_f64().is_some());
    assert_eq!(r["llm"]["provider_calls"], 0, "llm: false makes no LLM call");
    assert_eq!(v["aggregate"]["seeds_ok"], 2);
    assert!(v["aggregate"]["stats"]["beats_played"]["mean"].as_f64().is_some());
    assert_eq!(v["settings"]["llm"], false);
    assert_eq!(v["settings"]["beats_total"], 12);

    // Same settings + seeds again: identical results (timing aside); and the listing shows both jobs.
    let id2 = submit(&app, "/sim/run", body).await;
    let v2 = wait(&app, &id2).await;
    assert_eq!(v["settings_hash"], v2["settings_hash"]);
    for i in 0..2 {
        assert_eq!(
            stats::deterministic_part(&v["results"][i]),
            stats::deterministic_part(&v2["results"][i]),
            "seed {} differs between runs",
            v["results"][i]["seed"]
        );
    }
    assert_ne!(stats::deterministic_part(&v["results"][0]), stats::deterministic_part(&v["results"][1]));
    let (_, list, _) = call(&app, "GET", "/sim/jobs", Some(TOKEN), None).await;
    let ids: Vec<&str> = list["jobs"].as_array().unwrap().iter().map(|j| j["job_id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec![id2.as_str(), id.as_str()], "newest first");
    // The job log got a submitted and a finished event per job.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let log = std::fs::read_to_string(dir("run").join("fn7-sim-jobs.jsonl")).unwrap_or_default();
    assert_eq!(log.matches("sim_job_finished").count(), 2, "{log}");
    assert!(log.contains(&id) && log.contains("settings_hash"));
}

#[tokio::test]
async fn society_only_and_recording_download() {
    let app = simapi::router(config("rec", Some(TOKEN), None));
    let id = submit(&app, "/sim/run", json!({ "settings": SMALL(), "seeds": [3], "beats": 6, "society_only": true, "record": true })).await;
    let v = wait(&app, &id).await;
    assert_eq!(v["status"], "done", "{v}");
    assert_eq!(v["settings"]["society_only"], true);
    let r = &v["results"][0];
    assert_eq!(r["casts"]["count"], 0, "no casts in a society-only world: {r}");
    let run = r["recording"]["run_id"].as_str().unwrap();
    assert_eq!(r["recording"]["download"], format!("/sim/jobs/{id}/recording/3"));
    let (s, _, bytes) = call(&app, "GET", &format!("/sim/jobs/{id}/recording/3?key=k"), Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    let text = String::from_utf8(bytes).unwrap();
    let first: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["run"], run);
    assert!(text.contains("\"run_end\""), "complete recording");
    assert!(dir("rec").join("recordings").join(format!("{run}.jsonl")).exists());
    assert_eq!(call(&app, "GET", &format!("/sim/jobs/{id}/recording/99"), Some(TOKEN), None).await.0, StatusCode::NOT_FOUND);
    // The recording replays with 0 differences through the same verifier as fn7-sim --replay.
    let path = dir("rec").join("recordings").join(format!("{run}.jsonl"));
    let rep = fn7_server::replay::verify(path.to_str().unwrap(), |_| {}).await.unwrap();
    assert!(rep.differences.is_empty(), "{:?}", rep.differences);
    assert!(rep.snapshots > 0);
}

#[tokio::test]
async fn compare_side_by_side() {
    let app = simapi::router(config("cmp", Some(TOKEN), None));
    let id = submit(
        &app,
        "/sim/compare",
        json!({ "settings": SMALL(), "a": { "entropy_scale": 1 }, "b": { "entropy_scale": 6 }, "seeds": [1, 2], "beats": 10 }),
    )
    .await;
    let v = wait(&app, &id).await;
    assert_eq!(v["status"], "done", "{v}");
    assert_eq!(v["kind"], "compare");
    assert_eq!(v["a"]["settings"]["entropy_scale"], 1.0);
    assert_eq!(v["b"]["settings"]["entropy_scale"], 6.0);
    assert_eq!(v["a"]["results"].as_array().unwrap().len(), 2);
    assert_ne!(v["a"]["settings_hash"], v["b"]["settings_hash"]);
    let ea = v["a"]["aggregate"]["stats"]["entropy.max"]["mean"].as_f64().unwrap();
    let eb = v["b"]["aggregate"]["stats"]["entropy.max"]["mean"].as_f64().unwrap();
    assert!(eb > ea, "6× entropy should run hotter: a {ea}, b {eb}");
    assert!(v["delta"]["entropy.max"]["mean"].as_f64().unwrap() > 0.0, "{}", v["delta"]);
    // A compare whose arms are identical is refused.
    let (s, e, _) = call(&app, "POST", "/sim/compare", Some(TOKEN), Some(json!({ "a": {}, "b": {}, "seeds": [1] }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
}

#[tokio::test]
async fn bounds_are_400s() {
    let app = simapi::router(config("bounds", Some(TOKEN), None));
    let seeds: Vec<u64> = (0..65).collect();
    let cases = [
        json!({ "seeds": seeds }),
        json!({ "seeds": [] }),
        json!({}),
        json!({ "seeds": [1, 1] }),
        json!({ "seeds": [-1] }),
        json!({ "seeds": [1], "beats": 501 }),
        json!({ "seeds": [1], "beats": 0 }),
        json!({ "seeds": [1], "settings": { "beats_total": 900 } }),
        json!({ "seeds": [1], "players": 9 }),
        json!({ "seeds": [1], "settings": { "entropy_scal": 2 } }),
        json!({ "seeds": [1], "settings": { "entropy_scale": 11 } }),
        json!({ "seeds": [1], "settings": { "seed": 4 } }),
        json!({ "seeds": [1], "settings": { "llm": true } }),
        json!({ "seeds": [1], "settings": [1] }),
        json!({ "seeds": [1], "llm": true }),
        json!({ "seeds": [1], "llm": true, "budget_usd": 2.0 }),
        json!({ "seeds": [1], "llm": true, "budget_usd": 0.5 }),
        json!({ "seeds": [1], "budget_usd": 0.5 }),
        json!({ "seeds": [1], "record": "yes" }),
        json!({ "seeds": [1, 2, 3, 4, 5, 6, 7, 8, 9], "record": true }),
        json!({ "seeds": [1], "nope": 1 }),
        json!([1, 2]),
    ];
    for c in cases {
        let (s, v, _) = call(&app, "POST", "/sim/run", Some(TOKEN), Some(c.clone())).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{c} → {v}");
        assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{c} → {v}");
    }
    let mut req = Request::post("/sim/run").header("authorization", format!("Bearer {TOKEN}")).body(Body::from("{bad")).unwrap();
    req.headers_mut().insert("content-type", "application/json".parse().unwrap());
    assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::BAD_REQUEST);
    assert_eq!(call(&app, "GET", "/sim/jobs/nope", Some(TOKEN), None).await.0, StatusCode::NOT_FOUND);
    let (s, v, _) = call(&app, "POST", "/sim/compare", Some(TOKEN), Some(json!({ "seeds": [1], "a": { "entropy_scale": 2 } }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    // The queue bound: more runs than it holds is a 429.
    let mut small = config("bounds-q", Some(TOKEN), None);
    small.limits.max_queued_runs = 3;
    let q = simapi::router(small);
    let (s, v, _) = call(&q, "POST", "/sim/run", Some(TOKEN), Some(json!({ "seeds": [1, 2, 3, 4] }))).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{v}");
}

#[tokio::test]
async fn settings_listing_matches_settings_rs() {
    let app = simapi::router(config("list", Some(TOKEN), None));
    let (_, v, _) = call(&app, "GET", "/sim/settings", Some(TOKEN), None).await;
    let list = v["settings"].as_array().unwrap();
    let by_key = |k: &str| list.iter().find(|e| e["key"] == k);
    let validated = Settings::validate(&Value::Null).unwrap();
    // Every key the validator emits is listed, with the validator's default.
    for (k, d) in validated.as_object().unwrap() {
        let e = by_key(k).unwrap_or_else(|| panic!("`{k}` is not listed"));
        assert_eq!(&e["default"], d, "default of `{k}`");
    }
    // Every Settings field is listed and described.
    let fields = serde_json::to_value(Settings::default()).unwrap();
    for k in fields.as_object().unwrap().keys() {
        let e = by_key(k).unwrap_or_else(|| panic!("field `{k}` is not listed"));
        assert!(!e["description"].as_str().unwrap_or("").is_empty(), "`{k}` has no description");
    }
    // Every listed range is the validator's: the bounds pass, one step outside fails.
    let mut ranged = 0;
    for e in list {
        let k = e["key"].as_str().unwrap();
        let (Some(lo), Some(hi)) = (e["min"].as_f64(), e["max"].as_f64()) else { continue };
        let ok = |x: f64| -> Value {
            if e["type"] == "integer" { json!(x as i64) } else { json!(x) }
        };
        let step = if e["type"] == "integer" { 1.0 } else { 0.01 };
        for x in [lo, hi] {
            assert!(Settings::validate(&json!({ k: ok(x) })).is_ok(), "`{k}` = {x} should be valid");
        }
        if lo - step >= 0.0 || e["type"] == "number" {
            assert!(Settings::validate(&json!({ k: ok(lo - step) })).is_err(), "`{k}` below {lo} should fail");
        }
        assert!(Settings::validate(&json!({ k: ok(hi + step) })).is_err(), "`{k}` above {hi} should fail");
        ranged += 1;
    }
    assert!(ranged >= 18, "only {ranged} ranged settings parsed");
    // Timer settings: (0, max].
    for e in list.iter().filter(|e| e["source"] == "theatre") {
        let k = e["key"].as_str().unwrap();
        let hi = e["max"].as_f64().unwrap();
        assert!(Settings::validate(&json!({ k: hi })).is_ok());
        assert!(Settings::validate(&json!({ k: hi + 1.0 })).is_err());
        assert!(Settings::validate(&json!({ k: 0 })).is_err());
    }
    assert_eq!(by_key("beat_ms").unwrap()["alias_of"], "flow_ms");
    assert_eq!(by_key("beats_total").unwrap()["aliases"], json!(["rounds"]));
    assert_eq!(by_key("arena").unwrap()["values"], json!(["planet", "city"]));
    assert!(by_key("seed").unwrap()["sim"].as_str().unwrap().contains("seeds"));
}

/// A fake paid provider: scripted replies, each call "costs" $0.002.
struct Fake {
    inner: fn7_plugin::testkit::Scripted,
    calls: Arc<AtomicU64>,
}

#[async_trait]
impl LlmProvider for Fake {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.complete(req).await
    }
}

#[tokio::test]
async fn llm_true_uses_the_backend_within_budget() {
    let made = Arc::new(AtomicU64::new(0));
    let m = made.clone();
    let factory: LlmFactory = Arc::new(move || {
        m.fetch_add(1, Ordering::Relaxed);
        let calls = Arc::new(AtomicU64::new(0));
        let c = calls.clone();
        Ok(LlmBackend {
            llm: Arc::new(Fake { inner: Default::default(), calls }),
            label: "fake".into(),
            cost_usd: Arc::new(move || c.load(Ordering::Relaxed) as f64 * 0.002),
        })
    });
    let app = simapi::router(config("llm", Some(TOKEN), Some(factory)));
    let id = submit(&app, "/sim/run", json!({ "settings": SMALL(), "seeds": [5], "beats": 4, "llm": true, "budget_usd": 0.02 })).await;
    let v = wait(&app, &id).await;
    assert_eq!(v["status"], "done", "{v}");
    assert_eq!(made.load(Ordering::Relaxed), 1, "one backend per job");
    let llm = &v["llm"];
    assert_eq!(llm["backend"], "fake");
    assert_eq!(llm["budget_usd"], 0.02);
    assert!(llm["calls"].as_u64().unwrap() > 0, "{llm}");
    assert!(llm["refused_over_budget"].as_u64().unwrap() > 0, "a tiny budget runs out: {llm}");
    // Calls in flight when the budget ran out still finish; the overshoot is bounded by them.
    assert!(llm["cost_usd"].as_f64().unwrap() < 0.02 + 0.002 * 64.0, "{llm}");
    assert_eq!(v["settings"]["llm"], true);
    assert!(v["results"][0]["llm"]["provider_calls"].as_u64().unwrap() > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sims_do_not_stall_the_server_runtime() {
    let app = simapi::router(config("stall", Some(TOKEN), None));
    let id = submit(&app, "/sim/run", json!({ "settings": { "clusters": 60 }, "seeds": [11, 12, 13, 14], "beats": 30 })).await;
    // A 10 ms ticker on the server's runtime (as a room's timers are) while 4 seeds run on the sim threads.
    let mut worst = Duration::ZERO;
    let mut ticks = 0;
    let t = Instant::now();
    loop {
        let before = Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
        worst = worst.max(before.elapsed().saturating_sub(Duration::from_millis(10)));
        ticks += 1;
        if ticks % 20 == 0 {
            let (_, v, _) = call(&app, "GET", &format!("/sim/jobs/{id}"), Some(TOKEN), None).await;
            if v["status"] == "done" {
                break;
            }
        }
        assert!(t.elapsed() < Duration::from_secs(600));
    }
    println!("{ticks} ticks over {:?} while simulating; worst tick lateness {worst:?}", t.elapsed());
    assert!(worst < Duration::from_millis(100), "the server runtime stalled for {worst:?}");
}
