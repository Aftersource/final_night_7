//! Replay verification of a core recording (`<match_id>.jsonl`, from the Theatre server with `THEATRE_RECORD` set, or
//! from `fn7-sim`): re-execute the match from its `on_match_start` snapshot (a fork's run: its `fork` snapshot) with every LLM request answered by the
//! engine's [`ReplayLlm`] (keyed by round, actor, step and request content; a request the recording lacks fails, so its
//! mind takes the same rules fallback it took live), feeding each round the actions and `RoundContext` (closed action
//! windows) recorded in its `resolve_round` hook input, and check that every recorded snapshot
//! (`on_round_start` and `resolve_round` at `full`, `commit_round` and `fallback_outcome` always) serializes byte for
//! byte the same, plus the summary in `run_end`. A round whose resolve failed twice (or timed out) carries the core's
//! `fallback_outcome` hook record (theatre 1741d8f: input round, actions, ctx, resolve_error): replay calls
//! `fallback_outcome` with exactly that input. A mid-match join (room setting `allow_midmatch_join`) carries the core's
//! `on_seat_takeover` hook record (input `{takeover}`, output `{state_changed}`) at the round boundary, before that
//! round's `on_round_start`, and an `on_seat_takeover` snapshot when the state changed: replay calls
//! `on_seat_takeover` with exactly that takeover, in record order, and compares each snapshot. A schema-8 match run by
//! the server carries the plugin's side inputs as events (`duel_clock` at each Start, `journal_recovered` at a resolve
//! after a restart; `fn7_plugin::present`): replay feeds them back to the hooks that read them.
//! Accounts (docs/ACCOUNTS.md): the server's wrapper records what it did beside the plugin as events, and replay
//! feeds them back: a `seat_persona` event (the exact `SeatPersona` applied after a takeover) is applied again through
//! `fn7_plugin::persona::apply_to_state`, and a `seat_refused` event (a takeover the server refused: the plugin's
//! takeover never ran) skips the call. Personas applied at match start are already inside the `on_match_start`
//! snapshot. A recording without these events (every recording from before accounts) replays exactly as before.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use fn7_plugin::FinalNight7Plugin;
use serde_json::Value;
use theatre_plugin_api::*;
use theatre_server::replay::ReplayLlm;
use theatre_trace::{Ctx, Record, Sink};

#[derive(Debug, Default)]
pub struct Report {
    pub run: String,
    pub snapshots: usize,
    pub identical: usize,
    pub differences: Vec<String>,
    pub llm_hits: u64,
    pub llm_misses: u64,
    pub beats: u32,
    /// Beats replayed through `fallback_outcome` (from its hook record).
    pub fallbacks: u32,
    /// Mid-match joins replayed through `on_seat_takeover` (from its hook records).
    pub takeovers: u32,
    /// Whether the replayed summary equals the recorded one (`None`: the recording has no `run_end`).
    pub summary_identical: Option<bool>,
}

/// `v` without the image objects the media wrapper adds (`image`, `statue_image`, `portrait_image`, `diorama_image`): a summary recorded
/// by a server with images on compares equal to its replay through the bare plugin. A look (an object with
/// `image_loadout_hash`) also loses the inputs the wrapper consumes (`base_portrait`, `base_diorama`,
/// `portrait_subject`, `diorama_subject`, `fallback`) and its `pending`.
pub fn strip_media(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let look = o.contains_key("image_loadout_hash");
            Value::Object(
                o.iter()
                    .filter(|(k, _)| !matches!(k.as_str(), "image" | "statue_image" | "portrait_image" | "diorama_image"))
                    .filter(|(k, _)| {
                        !(look && matches!(k.as_str(), "base_portrait" | "base_diorama" | "portrait_subject" | "diorama_subject" | "fallback" | "pending"))
                    })
                    .map(|(k, x)| (k.clone(), strip_media(x)))
                    .collect(),
            )
        }
        Value::Array(a) => Value::Array(a.iter().map(strip_media).collect()),
        x => x.clone(),
    }
}

/// The first place two JSON values differ (path: a vs b), if any.
pub fn first_diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                let (va, vb) = (
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                );
                if let Some(d) = first_diff(va, vb, &format!("{path}.{k}")) {
                    return Some(d);
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{path}: length {} vs {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (va, vb))| first_diff(va, vb, &format!("{path}[{i}]")))
        }
        _ if a == b => None,
        _ => Some(format!("{path}: {} vs {}", trunc(a), trunc(b))),
    }
}

fn trunc(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 120 {
        format!("{}…", s.chars().take(120).collect::<String>())
    } else {
        s
    }
}

/// Discards everything: the replay runs hooks inside a trace context (so the LLM replay sees round and step) but
/// records nothing.
struct NullSink(AtomicU64);

impl Sink for NullSink {
    fn next_id(&self) -> i64 {
        self.0.fetch_add(1, Ordering::Relaxed) as i64
    }
    fn write(&self, _: Record) {}
}

/// Counts answered calls (the engine's `ReplayLlm` counts only misses).
struct Counting {
    inner: ReplayLlm,
    calls: AtomicU64,
}

#[async_trait]
impl LlmProvider for Counting {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.complete(req).await
    }
}

/// Read a JSONL recording (unparseable lines are skipped, like the engine's readers do).
pub fn read_records(path: &Path) -> anyhow::Result<Vec<Value>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

fn round_of(r: &Value) -> Option<u32> {
    r["round"]
        .as_u64()
        .or_else(|| r["ctx"]["round"].as_u64())
        .map(|n| n as u32)
}

struct ResolveIn {
    hook_id: Option<i64>,
    actions: Vec<Action>,
    ctx: RoundContext,
    /// From the `fallback_outcome` hook record (else the successful `resolve_round`).
    fallback: bool,
}

impl ResolveIn {
    fn of(h: &Value, fallback: bool) -> Self {
        Self {
            hook_id: h["id"].as_i64(),
            actions: serde_json::from_value(h["input"]["actions"].clone()).unwrap_or_default(),
            ctx: serde_json::from_value(h["input"]["ctx"].clone()).unwrap_or_default(),
            fallback,
        }
    }
}

/// The recording's `llm_call` records in the order their calls STARTED (their `id`). The engine's recorder numbers a
/// call when it starts but writes it when it finishes, so two identical concurrent requests (same round, actor, step
/// and content: e.g. one player's same god invocation twice in a beat) can sit in the file in finish order, and
/// `ReplayLlm` hands out identical keys' answers in file order: their answers swapped (recording 79b32d4b, beat 4).
/// The plugin asks in start order, so sorting by id gives each call its own answer.
pub fn llm_calls_in_start_order(records: &[Value]) -> Vec<Value> {
    let mut calls: Vec<Value> = records.iter().filter(|r| r["type"] == "llm_call").cloned().collect();
    calls.sort_by_key(|r| r["id"].as_i64().unwrap_or(i64::MAX));
    calls
}

pub async fn verify(path: impl AsRef<Path>, mut log: impl FnMut(&str)) -> anyhow::Result<Report> {
    let records = read_records(path.as_ref())?;
    let run = records
        .iter()
        .find(|r| r["type"] == "run_start")
        .and_then(|r| r["run"].as_str())
        .or_else(|| records.iter().find_map(|r| r["run"].as_str().or(r["ctx"]["run"].as_str())))
        .ok_or_else(|| anyhow!("empty recording"))?
        .to_string();
    let snaps: Vec<(Option<u32>, &str, &Value)> = records
        .iter()
        .filter(|r| r["type"] == "snapshot")
        .map(|r| (round_of(r), r["label"].as_str().unwrap_or(""), &r["state"]))
        .collect();
    let snap = |round: u32, label: &str| {
        snaps
            .iter()
            .rev()
            .find(|s| s.0 == Some(round) && s.1 == label)
            .map(|s| s.2)
    };
    // `on_seat_takeover` snapshots per round, in order (several joins can complete at one boundary).
    let takeover_snaps = |round: u32| -> Vec<&Value> {
        snaps
            .iter()
            .filter(|s| s.0 == Some(round) && s.1 == "on_seat_takeover")
            .map(|s| s.2)
            .collect()
    };
    // Each round's resolve: the `resolve_round` attempt that succeeded, or the core's `fallback_outcome` record (the
    // resolve failed twice or timed out). A round with only failed attempts and no fallback record is an error.
    let mut resolves: BTreeMap<u32, ResolveIn> = BTreeMap::new();
    let mut failed: Vec<u32> = vec![];
    // Each round's mid-match joins, in record order: (hook id, takeover, recorded state_changed).
    let mut takeovers: BTreeMap<u32, Vec<(Option<i64>, SeatTakeover, bool)>> = BTreeMap::new();
    for h in records.iter().filter(|r| r["type"] == "hook") {
        let Some(round) = round_of(h) else { continue };
        match h["hook"].as_str() {
            Some("on_seat_takeover") => {
                let t: SeatTakeover = serde_json::from_value(h["input"]["takeover"].clone())
                    .with_context(|| format!("run {run}: an on_seat_takeover record without its takeover"))?;
                let changed = h["output"]["state_changed"].as_bool().unwrap_or(false);
                takeovers.entry(round).or_default().push((h["id"].as_i64(), t, changed));
            }
            Some("resolve_round") if h["error"].is_null() => {
                resolves.insert(round, ResolveIn::of(h, false));
            }
            Some("resolve_round") => failed.push(round),
            Some("fallback_outcome") if h["error"].is_null() => {
                resolves.insert(round, ResolveIn::of(h, true));
            }
            _ => {}
        }
    }
    // Accounts: per (round, seat), the personas applied after a takeover and the takeovers refused, in record order.
    let mut personas: BTreeMap<(u32, u8), Vec<fn7_plugin::persona::SeatPersona>> = BTreeMap::new();
    let mut refused: Vec<(u32, u8)> = vec![];
    for e in records.iter().filter(|r| r["type"] == "event") {
        let d = &e["data"];
        match e["kind"].as_str() {
            Some(fn7_plugin::persona::EVENT) => {
                if let (Some(round), Ok(p)) = (d["round"].as_u64(), serde_json::from_value::<fn7_plugin::persona::SeatPersona>(d["persona"].clone())) {
                    // Round 0 is match start: inside the on_match_start snapshot already.
                    if round > 0 {
                        personas.entry((round as u32, p.seat)).or_default().push(p);
                    }
                }
            }
            Some(fn7_plugin::persona::REFUSED_EVENT) => {
                if let (Some(round), Some(seat)) = (d["round"].as_u64(), d["seat"].as_u64()) {
                    refused.push((round as u32, seat as u8));
                }
            }
            _ => {}
        }
    }
    if let Some(r) = failed.iter().find(|r| !resolves.contains_key(r)) {
        return Err(anyhow!(
            "run {run}: beat {r}'s resolve_round failed and the recording has no fallback_outcome record \
             (recorded before theatre 1741d8f?)"
        ));
    }
    // A match starts from its on_match_start snapshot; a fork (admin API) from its `fork` snapshot after round N.
    let (first, start) = snaps
        .iter()
        .find(|s| s.1 == "on_match_start" || s.1 == "fork")
        .map(|s| (s.0.unwrap_or(0) + 1, s.2.clone()))
        .ok_or_else(|| anyhow!("no on_match_start or fork snapshot in run {run}"))?;
    let last_commit = snaps
        .iter()
        .filter(|s| s.1 == "commit_round")
        .filter_map(|s| s.0)
        .max()
        .unwrap_or(0);
    let last = resolves.keys().max().copied().unwrap_or(0).max(last_commit);

    let replay = Counting {
        inner: ReplayLlm::from_records(&llm_calls_in_start_order(&records)),
        calls: AtomicU64::new(0),
    };
    let sink: Arc<dyn Sink> = Arc::new(NullSink(AtomicU64::new(1)));
    let ctx = |round: u32| {
        let mut c = Ctx::new(sink.clone(), run.clone());
        c.round = Some(round);
        c
    };
    // The side inputs the live plugin read outside its hook inputs (state schema 8: the Start's clock and pause, the
    // actions recovered from its journal after a restart) are recorded as events and fed back here.
    let p = FinalNight7Plugin::new().with_feed(fn7_plugin::present::Feed::from_records(&records));
    // Schema 14: the duel move list the match was played with (its ruleset records it when it was not the compiled
    // one): the mapper's reading decides classes.
    let ruleset = records.iter().find(|r| r["type"] == "ruleset").map(|r| &r["body"]);
    fn7_plugin::moves::install_recorded(ruleset).map_err(|e| anyhow!("run {run}: {e}"))?;
    let mut rep = Report {
        run: run.clone(),
        ..Default::default()
    };
    let mut compare = |round: u32, label: &str, theirs: Option<&Value>, got: &Value, rep: &mut Report| {
        if let Some(theirs) = theirs {
            rep.snapshots += 1;
            if serde_json::to_string(theirs).ok() == serde_json::to_string(got).ok() {
                rep.identical += 1;
            } else {
                let d = first_diff(theirs, got, "state").unwrap_or_else(|| "byte-level difference".into());
                log(&format!("  beat {round:>3} {label:<16} DIFFERS at {d}"));
                rep.differences.push(format!("beat {round} {label}: {d}"));
            }
        }
    };
    type Compare<'a> = dyn FnMut(u32, &str, Option<&Value>, &Value, &mut Report) + 'a;
    // The joins the core completed at `round`'s boundary, before its on_round_start: exactly the recorded takeovers,
    // in record order, each changed state compared with the next `on_seat_takeover` snapshot of the round.
    let joins = |round: u32, state: &mut Value, rep: &mut Report, compare: &mut Compare| {
        let Some(ts) = takeovers.get(&round) else { return };
        let mut theirs = takeover_snaps(round).into_iter();
        for (hook_id, t, changed) in ts {
            let mut c = ctx(round);
            c.hook_id = *hook_id;
            let out = theatre_trace::within_sync(c, || {
                // What the server's accounts wrapper did around the plugin's takeover, from its recorded events.
                if refused.contains(&(round, t.seat)) {
                    return None;
                }
                let base = p.on_seat_takeover(state, t);
                match personas.get(&(round, t.seat)).and_then(|v| v.first()) {
                    Some(persona) => {
                        fn7_plugin::persona::apply_to_state(base.as_ref().unwrap_or(state), persona, t.round).or(base)
                    }
                    None => base,
                }
            });
            rep.takeovers += 1;
            if out.is_some() != *changed {
                rep.differences.push(format!(
                    "beat {round} on_seat_takeover (seat {}): state_changed {changed} recorded, {} replayed",
                    t.seat,
                    out.is_some()
                ));
            }
            if let Some(st) = out {
                compare(round, "on_seat_takeover", theirs.next(), &st, rep);
                *state = st;
            }
        }
    };
    let mut state = start;
    for round in first..=last {
        joins(round, &mut state, &mut rep, &mut compare);
        let rs = theatre_trace::within(ctx(round), p.on_round_start(&state, round, &replay)).await?;
        compare(round, "on_round_start", snap(round, "on_round_start"), &rs.state, &mut rep);
        state = rs.state;
        let Some(input) = resolves.get(&round) else {
            break;
        };
        let mut c = ctx(round);
        c.hook_id = input.hook_id;
        let out = if input.fallback {
            // Exactly the recorded call: the context the failed resolve attempts got, as the core passed it.
            let o = theatre_trace::within_sync(c, || p.fallback_outcome(&state, round, &input.actions, &input.ctx));
            compare(round, "fallback_outcome", snap(round, "fallback_outcome"), &o.state, &mut rep);
            rep.fallbacks += 1;
            o
        } else {
            theatre_trace::within(c, p.resolve_round(&state, round, &input.actions, &input.ctx, &replay, &NoProgress))
                .await?
        };
        compare(round, "resolve_round", snap(round, "resolve_round"), &out.state, &mut rep);
        state = out.state;
        let committed = theatre_trace::within(ctx(round), p.commit_round(&state, round, &[], &replay)).await?;
        compare(round, "commit_round", snap(round, "commit_round"), &committed.state, &mut rep);
        state = committed.state;
        rep.beats = round;
    }
    // A next beat's Start the driver ran before learning the match was over.
    if snap(last + 1, "on_round_start").is_some() {
        joins(last + 1, &mut state, &mut rep, &mut compare);
        let rs = theatre_trace::within(ctx(last + 1), p.on_round_start(&state, last + 1, &replay)).await?;
        compare(last + 1, "on_round_start", snap(last + 1, "on_round_start"), &rs.state, &mut rep);
        state = rs.state;
    }
    if let Some(end) = records.iter().rev().find(|r| r["type"] == "run_end") {
        let summarize = records
            .iter()
            .rev()
            .find(|r| r["type"] == "hook" && r["hook"] == "summarize" && r["error"].is_null());
        if let Some(h) = summarize {
            let round = round_of(h).unwrap_or(rep.beats);
            let s = theatre_trace::within(ctx(round), p.summarize(&state, &replay)).await?;
            // The server's media wrapper adds image objects on the way out (never to the state); compare without them.
            let (want, got) = (strip_media(&end["summary"]["summary"]), strip_media(&s.summary));
            let same = want == got;
            if !same {
                let d = first_diff(&want, &got, "summary").unwrap_or_default();
                log(&format!("  summary DIFFERS at {d}"));
                rep.differences.push(format!("summary: {d}"));
            }
            rep.summary_identical = Some(same);
        }
    }
    let misses = *replay.inner.misses.lock();
    rep.llm_misses = misses;
    rep.llm_hits = replay.calls.load(Ordering::Relaxed).saturating_sub(misses);
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Two identical concurrent requests written in finish order are answered in start order (recording 79b32d4b).
    #[tokio::test]
    async fn identical_concurrent_calls_get_their_own_answers() {
        let ask = || LlmRequest { model: None, system: "s".into(), messages: vec![(Role::User, "same".into())], temperature: 0.8,
                                  max_tokens: 800, json_schema: None, n: 1 };
        let req = serde_json::to_value(ask()).unwrap();
        let call = |id: i64, answer: &str| json!({ "type": "llm_call", "id": id, "round": 4, "actor": "gd:3", "step": "god",
                                                   "request": req, "choices": [answer] });
        // The later-started call finished first, so it was written first.
        let records = vec![json!({ "type": "event" }), call(6023, "second"), call(6022, "first")];
        let r = ReplayLlm::from_records(&llm_calls_in_start_order(&records));
        let ctx = |n| {
            let mut c = Ctx::new(Arc::new(NullSink(AtomicU64::new(1))), "run");
            c.round = Some(4);
            c.actor = Some("gd:3".into());
            c.step = Some("god".into());
            let _ = n;
            c
        };
        let a = theatre_trace::within(ctx(0), r.complete(ask())).await;
        let b = theatre_trace::within(ctx(1), r.complete(ask())).await;
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!((a.first(), b.first()), (Some("first"), Some("second")));
        assert_eq!(llm_calls_in_start_order(&records).iter().map(|r| r["id"].as_i64().unwrap()).collect::<Vec<_>>(), vec![6022, 6023]);
    }
}
