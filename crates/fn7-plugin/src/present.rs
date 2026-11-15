//! Exact resume (state schema 8): the present moment of a match, for a client that reconnects, a viewer that joins
//! mid-duel and a server that restarts. See docs/TIME_TRAVEL.md for the whole data structure.
//!
//! **The gap this closes.** The Theatre's sync hooks are read-only and the plugin state changes only at Start, Resolve
//! and Commit, so everything between two resolves (casts, summonings, `challenge.lock`, duel lines) exists in the core
//! only as queued instant actions: they are not in the state a `match.snapshot` is built from, and they are not in the
//! room's disk snapshot (written at each Commit). The plugin therefore keeps a **journal**: every instant action it
//! accepted, with the core's arrival stamps (`seq`, `received_at_ms`) and the public event it made, per match.
//!
//! - **In memory** it is the live view: the open duels as they stand NOW are the state's bouts (which, in schema 8,
//!   keep every blow so far) plus a replay of the journal entries stamped after the state's delivery mark
//!   ([`fn7_model::dm::Mark`]) with the same pure functions the resolve uses ([`crate::duel::apply_act`]).
//!   [`Journal::present`] is that view; it goes out in `public_state.present` (so in every `match.snapshot`) and in the
//!   `game.present` query.
//! - **On disk** (`with_journal`, the server only) each entry is appended to `<dir>/<match>.jsonl` as it is accepted.
//!   After a kill, the restarted server reads it back: the live view shows the same duel with the same blows, and the
//!   first resolve is handed the entries the dead process never delivered ([`Journal::recovered`]), merged into its
//!   actions in arrival order.
//!
//! **Determinism.** A replay re-executes the hooks from the recording and has no journal and no clock. The two things
//! the live plugin reads from outside its inputs are therefore recorded as trace events inside the hook that used them
//! and fed back by the verifier ([`Feed`]): `journal_recovered {beat, actions}` (resolve) and `duel_clock {beat,
//! now_ms, pause}` (Start). With neither (a simulation, a test, an older recording) the hooks are pure functions of
//! their inputs, as before.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use fn7_model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theatre_plugin_api::Action;

use crate::duel::{self, Bout, Moved};
use crate::settings::Settings;

/// A gap this long (ms) between a Commit and the next Start (or between the last sign of life before a restart and
/// the Start after it) is a pause: open duels' clocks stand still for it.
pub const PAUSE_MIN_MS: i64 = 2000;

/// One accepted instant action, as the core stamped it, and the public event it made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The server process that accepted it (entries of another boot were never delivered by this one's core).
    pub boot: u64,
    /// The state's beat when it arrived.
    pub beat: u32,
    pub seat: u8,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub fields: Value,
    pub seq: u64,
    pub at_ms: i64,
    /// The match's seed (how a view, which runs outside the recording context, finds its journal after a restart).
    #[serde(default)]
    pub seed: u64,
    /// The public `phase.event` payload it made (before the media wrapper's image objects), if any.
    #[serde(default)]
    pub event: Option<Value>,
}

impl Entry {
    fn action(&self) -> Action {
        Action { fields: self.fields.clone(), instant: true, seq: self.seq, received_at_ms: self.at_ms,
                 ..Action::new(self.seat, self.text.clone()) }
    }
}

#[derive(Debug, Default)]
struct Run {
    entries: Vec<Entry>,
    /// The disk journal was read (or there is none).
    loaded: bool,
    /// The server's clock at this boot's last Commit of the match, until its next Start.
    idle_since: Option<i64>,
    /// A Start ran in this boot.
    started: bool,
    seed: u64,
    /// The match's live state as the hooks last left it: its beat and delivery mark. A view of any other state of the
    /// seed (the admin's state at a round, a fork not started yet) is history: it gets no journal on top.
    at: Option<(u32, Option<fn7_model::dm::Mark>)>,
}

#[derive(Debug, Default)]
struct Inner {
    runs: HashMap<String, Run>,
    /// Journal files found on disk at the first use, by seed, oldest first (a restart: the views run outside the
    /// recording context and only know the state).
    on_disk: HashMap<u64, Vec<String>>,
    scanned: bool,
}

/// The side inputs of one beat's Start: the server's clock and the pause it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockIn {
    pub now_ms: i64,
    /// `[from_ms, to_ms]`: the room stood still (paused with nobody connected, or the server was down).
    #[serde(default)]
    pub pause: Option<[i64; 2]>,
}

/// The side inputs of a recorded match, by round (for replay verification).
#[derive(Debug, Clone, Default)]
pub struct Feed {
    pub clocks: BTreeMap<u32, ClockIn>,
    pub recovered: BTreeMap<u32, Vec<Action>>,
    /// Schema 14: the duel picks each resolve was handed (`duel_picks`, [`crate::picker`]).
    pub picks: BTreeMap<u32, Vec<crate::picker::PickRec>>,
}

impl Feed {
    /// Read the side inputs out of a recording's records (`duel_clock` and `journal_recovered` events; the last of a
    /// round wins: a retried hook records its inputs again).
    pub fn from_records(records: &[Value]) -> Feed {
        let mut f = Feed::default();
        for r in records.iter().filter(|r| r["type"] == "event") {
            let Some(round) = r["round"].as_u64().or_else(|| r["ctx"]["round"].as_u64()).map(|n| n as u32) else { continue };
            match r["kind"].as_str() {
                Some("duel_clock") => {
                    if let Ok(c) = serde_json::from_value::<ClockIn>(r["data"].clone()) {
                        f.clocks.insert(round, c);
                    }
                }
                Some("journal_recovered") => {
                    let a: Vec<Action> = serde_json::from_value(r["data"]["actions"].clone()).unwrap_or_default();
                    f.recovered.insert(round, a);
                }
                Some("duel_picks") => {
                    let p: Vec<crate::picker::PickRec> = serde_json::from_value(r["data"]["picks"].clone()).unwrap_or_default();
                    f.picks.insert(round, p);
                }
                _ => {}
            }
        }
        f
    }

    pub fn is_empty(&self) -> bool {
        self.clocks.is_empty() && self.recovered.is_empty() && self.picks.is_empty()
    }
}

/// The plugin's record of the instant actions it accepted (see the module docs).
#[derive(Debug)]
pub struct Journal {
    boot: u64,
    /// Where entries are appended (the server); `None`: memory only.
    dir: Option<PathBuf>,
    inner: Mutex<Inner>,
}

impl Default for Journal {
    fn default() -> Self {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        Journal { boot: fn7_model::seed::Rng::new(t ^ std::process::id() as u64).next_u64() | 1, dir: None, inner: Default::default() }
    }
}

fn file_name(key: &str) -> String {
    let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    format!("{safe}.jsonl")
}

impl Journal {
    /// A journal that also appends to `dir` and reads it back after a restart (the durable mode: recovery of
    /// undelivered actions, and the server's clock at each Start).
    pub fn durable(dir: PathBuf) -> Journal {
        Journal { dir: Some(dir), ..Default::default() }
    }

    pub fn is_durable(&self) -> bool {
        self.dir.is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn scan(&self, inner: &mut Inner) {
        let (Some(dir), false) = (&self.dir, inner.scanned) else { return };
        inner.scanned = true;
        let mut found: Vec<(std::time::SystemTime, u64, String)> = vec![];
        for f in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = f.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let first: Option<Entry> =
                std::fs::read_to_string(&path).ok().and_then(|t| t.lines().next().and_then(|l| serde_json::from_str(l).ok()));
            if let Some(e) = first {
                let at = f.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                found.push((at, e.seed, stem));
            }
        }
        found.sort();
        for (_, seed, stem) in found {
            inner.on_disk.entry(seed).or_default().push(stem);
        }
    }

    /// In a hook: the match is the recording run we are in (the match id); with recording off, the seed.
    fn hook_key(w: &World) -> String {
        theatre_trace::current().map(|c| c.run).filter(|r| !r.is_empty()).unwrap_or_else(|| format!("seed-{}", w.meta.seed))
    }

    /// For a view (`public_view` runs outside the recording context and is also asked about old states): the match
    /// `w` is the LIVE state of, if any: the run we are in (a query), else the run of this seed whose hooks last left
    /// exactly this beat and mark, else (after a restart, before any hook ran) the newest journal on disk of this seed.
    fn view_key(&self, inner: &mut Inner, w: &World) -> Option<String> {
        if let Some(run) = theatre_trace::current().map(|c| c.run).filter(|r| !r.is_empty()) {
            return Some(run);
        }
        self.scan(inner);
        let seed = w.meta.seed;
        let here = Some((w.meta.beat, w.dm.mark));
        if let Some(k) = inner.runs.iter().find(|(_, r)| r.seed == seed && r.at == here).map(|(k, _)| k.clone()) {
            return Some(k);
        }
        let fresh = |inner: &Inner, k: &str| inner.runs.get(k).is_none_or(|r| r.at.is_none());
        if let Some(k) = inner.on_disk.get(&seed).and_then(|v| v.iter().rev().find(|k| fresh(inner, k))).cloned() {
            return Some(k);
        }
        let k = format!("seed-{seed}");
        (inner.runs.contains_key(&k) && fresh(inner, &k)).then_some(k)
    }

    fn run<'a>(&self, inner: &'a mut Inner, key: &str, w: &World) -> &'a mut Run {
        let run = inner.runs.entry(key.to_string()).or_default();
        run.seed = w.meta.seed;
        if !run.loaded {
            run.loaded = true;
            if let Some(dir) = &self.dir {
                if let Ok(text) = std::fs::read_to_string(dir.join(file_name(key))) {
                    let mut old: Vec<Entry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
                    old.retain(|e| e.boot != self.boot);
                    old.append(&mut run.entries);
                    run.entries = old;
                }
            }
        }
        run
    }

    /// An entry a state at `w`'s point has not been handed: stamped after its mark, and arrived within a beat of it.
    fn pending(w: &World, e: &Entry) -> bool {
        let beat = w.meta.beat;
        e.beat + 1 >= beat && e.beat <= beat + 1 && !w.dm.mark.is_some_and(|m| m.delivered(e.at_ms, e.seq))
    }

    fn pending_of(run: &Run, w: &World) -> Vec<Entry> {
        let mut v: Vec<Entry> = run.entries.iter().filter(|e| Self::pending(w, e)).cloned().collect();
        v.sort_by_key(|e| (e.at_ms, e.seq));
        v
    }

    /// In a hook: the accepted instant actions `w` has not been handed yet, in arrival order.
    pub fn entries(&self, w: &World) -> Vec<Entry> {
        let mut inner = self.lock();
        Self::pending_of(self.run(&mut inner, &Self::hook_key(w), w), w)
    }

    /// In a hook that returns a state (or leaves the match at one): `w` is the match's live state now.
    pub fn seen(&self, w: &World) {
        let mut inner = self.lock();
        self.run(&mut inner, &Self::hook_key(w), w).at = Some((w.meta.beat, w.dm.mark));
    }

    /// Note an accepted instant action and the public event it made.
    pub fn note(&self, w: &World, seat: u8, a: &Action, event: Option<&Value>) {
        let e = Entry { boot: self.boot, beat: w.meta.beat, seat, text: a.text.clone(), fields: a.fields.clone(), seq: a.seq,
                        at_ms: a.received_at_ms, seed: w.meta.seed, event: event.cloned() };
        let mut inner = self.lock();
        let key = Self::hook_key(w);
        if let Some(dir) = &self.dir {
            let line = serde_json::to_string(&e).unwrap_or_default();
            let res = std::fs::create_dir_all(dir).and_then(|_| {
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(file_name(&key)))?;
                writeln!(f, "{line}")
            });
            if let Err(err) = res {
                eprintln!("fn7 journal: append to {} failed: {err}", dir.display());
            }
        }
        self.run(&mut inner, &key, w).entries.push(e);
    }

    /// Schema 14: journal a duel pick ([`crate::picker`]) for the match `key` (the hook key captured when the line
    /// arrived: a pick comes back outside any hook), stamped with its own `at_ms` and its line's `seq`.
    pub fn note_pick(&self, key: &str, seed: u64, beat: u32, seat: u8, rec: &crate::picker::PickRec) {
        let e = Entry { boot: self.boot, beat, seat, text: String::new(), fields: json!({ "duel_pick": rec }), seq: rec.line_seq,
                        at_ms: rec.at_ms, seed, event: None };
        let mut inner = self.lock();
        if let Some(dir) = &self.dir {
            let line = serde_json::to_string(&e).unwrap_or_default();
            let res = std::fs::create_dir_all(dir).and_then(|_| {
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(file_name(key)))?;
                writeln!(f, "{line}")
            });
            if let Err(err) = res {
                eprintln!("fn7 journal: append to {} failed: {err}", dir.display());
            }
        }
        let run = inner.runs.entry(key.to_string()).or_default();
        run.seed = seed;
        run.entries.push(e);
    }

    /// The hook key of the match `w` is in (captured by a picker before it leaves the hook).
    pub fn key_of(w: &World) -> String {
        Self::hook_key(w)
    }

    /// Schema 14, at a resolve: the journaled picks the state has not been handed and that are stamped at or before
    /// `upto` (the batch's newest instant action: the mark it will set), in their order.
    pub fn picks(&self, w: &World, upto: Option<(i64, u64)>) -> Vec<crate::picker::PickRec> {
        let Some(upto) = upto else { return vec![] };
        self.entries(w)
            .iter()
            .filter(|e| (e.at_ms, e.seq) <= upto)
            .filter_map(|e| serde_json::from_value(e.fields.get("duel_pick")?.clone()).ok())
            .collect()
    }

    /// The last sign of life a restored state carries: its newest stamp, or a later journal entry of another boot.
    fn last_stamp(&self, run: &Run, w: &World) -> Option<i64> {
        let last = run.entries.iter().filter(|e| e.boot != self.boot).map(|e| e.at_ms).max().unwrap_or(0);
        let m = w.dm.mark.unwrap_or_default();
        Some(m.resolved_ms.max(m.at_ms).max(m.round_ms).max(last)).filter(|t| *t > 0)
    }

    /// A beat's Start: forget what every later state has been handed, and read the side inputs (durable mode): the
    /// server's clock and the pause since the last Commit (this boot) or since the last sign of life before the
    /// restart or the fork.
    pub fn round_start(&self, w: &World) -> Option<ClockIn> {
        let mut inner = self.lock();
        let durable = self.is_durable();
        let run = self.run(&mut inner, &Self::hook_key(w), w);
        let beat = w.meta.beat;
        run.entries.retain(|e| e.beat + 3 >= beat);
        let idle = run.idle_since.take();
        let first = !run.started;
        run.started = true;
        if !durable {
            return None;
        }
        let now_ms = theatre_trace::now_ms();
        let from = match idle {
            Some(t) => Some(t),
            // The first Start of this boot for the match: a restored or forked one stood still since its last stamp.
            None if first => self.last_stamp(run, w),
            None => None,
        };
        let pause = from.filter(|f| now_ms - f >= PAUSE_MIN_MS).map(|f| [f, now_ms]);
        Some(ClockIn { now_ms, pause })
    }

    /// A Commit: the room may pause here (nobody connected) until its next Start.
    pub fn committed(&self, w: &World) {
        let mut inner = self.lock();
        let durable = self.is_durable();
        let run = self.run(&mut inner, &Self::hook_key(w), w);
        run.at = Some((w.meta.beat, w.dm.mark));
        if durable {
            run.idle_since = Some(theatre_trace::now_ms());
        }
    }

    /// The match is over: its journal is no longer needed.
    pub fn finished(&self, w: &World) {
        let mut inner = self.lock();
        let key = Self::hook_key(w);
        inner.runs.remove(&key);
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_file(dir.join(file_name(&key)));
        }
    }

    /// Durable mode, at a resolve: the accepted instant actions another boot of the server took in and never
    /// delivered (the core's queue died with it), minus any the core did hand over.
    pub fn recovered(&self, w: &World, actions: &[Action]) -> Vec<Action> {
        if !self.is_durable() {
            return vec![];
        }
        let boot = self.boot;
        self.entries(w)
            .iter()
            .filter(|e| e.boot != boot && !actions.iter().any(|a| a.seat == e.seat && a.seq == e.seq && a.received_at_ms == e.at_ms))
            // Picks are handed over on their own ([`Journal::picks`]).
            .filter(|e| e.fields.get("duel_pick").is_none())
            .map(Entry::action)
            .collect()
    }

    /// The duels as they stand now ([`live`]) from the state and the journal (in a hook).
    pub fn live(&self, w: &World, st: &Settings) -> Live {
        live(w, st, &self.entries(w))
    }

    /// `public_state.present` / `game.present`: everything since the last resolve, and every open duel in full.
    ///
    /// For the match's live state: the journal's pending entries on top of the state, clocks read now (or, while the
    /// room stands still, at the moment it stopped). For any other state (history, a fork before its first Start):
    /// the state alone, its clocks frozen at its own last stamp.
    pub fn present(&self, w: &World) -> Value {
        let st = crate::world::settings(w);
        let now_ms = theatre_trace::now_ms();
        let (entries, idle, live_state) = {
            let mut inner = self.lock();
            match self.view_key(&mut inner, w) {
                Some(key) => {
                    let durable = self.is_durable();
                    let run = self.run(&mut inner, &key, w);
                    // The plugin is not told about pauses: the room stands still when this boot's last Commit was
                    // [`PAUSE_MIN_MS`] ago with no Start since, or the match was restored and has not started.
                    let idle = match run.idle_since {
                        _ if !durable => None,
                        Some(t) => Some(t).filter(|t| now_ms - t >= PAUSE_MIN_MS),
                        None if !run.started => self.last_stamp(run, w).filter(|t| now_ms - t >= PAUSE_MIN_MS),
                        None => None,
                    };
                    (Self::pending_of(run, w), idle, true)
                }
                None => {
                    let m = w.dm.mark.unwrap_or_default();
                    (vec![], Some(m.resolved_ms.max(m.at_ms).max(m.round_ms)).filter(|t| *t > 0), false)
                }
            }
        };
        let lv = live(w, &st, &entries);
        let at = idle.unwrap_or(now_ms);
        let events: Vec<Value> = entries
            .iter()
            .filter_map(|e| {
                let mut v = e.event.clone()?;
                v["seq"] = e.seq.into();
                v["received_at_ms"] = e.at_ms.into();
                v["beat"] = e.beat.into();
                Some(v)
            })
            .collect();
        let earth = crate::earth::view::summary_of(w, &st);
        json!({
            "server_time_ms": now_ms,
            "beat": w.meta.beat,
            // This is the match's live state (else: a state of the past, shown as it was).
            "live": live_state,
            // The newest instant action the state itself contains; `events` are the ones after it.
            "mark": w.dm.mark.map(|m| json!({ "at_ms": m.at_ms, "seq": m.seq, "resolved_ms": m.resolved_ms, "round_ms": m.round_ms })),
            "paused_since_ms": idle,
            "events": events,
            "duels": lv.bouts.iter().map(|b| duel_json(w, b, at, idle.is_some())).collect::<Vec<_>>(),
            // The Earth simulator (schema 13; `null` when off): the field's beat, hash, dirty count and phenomena. A
            // reconnecting client compares the hash with its own and fetches the cells with `game.earth` if they differ.
            "earth": earth,
        })
    }
}

/// The duels under way at a moment: the bouts and the summonings that moved their ground.
#[derive(Debug, Default)]
pub struct Live {
    pub bouts: Vec<Bout>,
    pub moved: Vec<Moved>,
}

/// The duels as they stand after `entries` (the accepted instant actions the state has not been handed), from the
/// state: its open bouts, the built-in AI's challenges of its beat, then each entry through the resolve's own
/// [`duel::apply_act`]. Pure.
pub fn live(w: &World, st: &Settings, entries: &[Entry]) -> Live {
    let mut out = Live::default();
    if !st.challenge_enabled || !w.dm_rules() {
        return out;
    }
    let beat = w.meta.beat;
    out.bouts = duel::open_bouts(w);
    out.bouts.extend(duel::ai_challenges(w, beat, st).iter().map(|c| duel::ai_bout(w, beat, c)));
    out.moved = out.bouts.iter().filter(|b| !b.opened).filter_map(|b| b.duel.leap).collect();
    for e in entries {
        if let Some(act) = duel::act_of(e.seat, &e.text, &e.fields, e.seq, e.at_ms) {
            duel::apply_act(w, st, beat, &mut out.bouts, &mut out.moved, &act, false);
        }
    }
    out
}

/// One open duel in full: who, where, the formal state, every blow so far with its choreography and stamps, and its
/// clock at `at_ms`.
pub fn duel_json(w: &World, b: &Bout, at_ms: i64, paused: bool) -> Value {
    let d = &b.duel;
    let name = |s: usize| if s == 0 { "a" } else { "b" };
    let clock = b.clock(at_ms);
    let over = b.ko.is_some() || (d.deadline_ms > 0 && at_ms > d.deadline_ms);
    let lock = duel::lock_json(w, d);
    let mut v = json!({
        "duel_id": d.id,
        "a": lock["a"], "b": lock["b"],
        "lock": lock,
        "hp": { "a": n1(b.hp[0]), "b": n1(b.hp[1]) },
        "range": b.flow.range, "initiative": name(b.flow.initiative),
        "stances": { "a": b.flow.stance[0], "b": b.flow.stance[1] },
        "set": { "a": b.flow.set[0], "b": b.flow.set[1] },
        "blows": b.blows, "n": b.n, "typed": b.typed,
        "ko": b.ko.map(name),
        "opened": b.opened,
        "started_ms": (d.started_ms > 0).then_some(d.started_ms),
        "deadline_ms": (d.deadline_ms > 0).then_some(d.deadline_ms),
        "ends_beat": d.ends_beat,
        "pauses": b.pauses,
        // Rules 0.8.2: the scene and every panel so far (`duel.scene`, `duel.panel`).
        "scene": b.scene, "panels": b.panels,
        "clock": clock.map(|(elapsed, left)| json!({ "at_ms": at_ms, "elapsed_ms": elapsed, "remaining_ms": left.max(0),
                                                     "running": !paused && !over })),
        "over": over,
    });
    // Schema 12: the lines its AI fighters are typing right now (`duel.typing_script`: seek to `server_time_ms`).
    if b.ai.is_some() {
        v["typing_scripts"] = crate::duel_ai::pending_json(b).into();
    }
    // Schema 14: the duel sim's checkpoint and the newest panel's sim (docs/DUEL_SIM.md); `panels[]` stay bare.
    if let Some(sim) = crate::panel::present_sim(b) {
        v["sim"] = sim;
        v["scene"] = crate::panel::live_scene(b).unwrap_or(Value::Null);
    }
    v
}

fn n1(x: f32) -> f64 {
    (x as f64 * 10.0).round() / 10.0
}

/// The actions a resolve works on: the core's, with the recovered ones (arrival order) placed where the core puts
/// instant actions: after the Compose actions, before this batch's own windowed and instant ones (they are older).
pub fn merge(actions: &[Action], mut recovered: Vec<Action>) -> Vec<Action> {
    if recovered.is_empty() {
        return actions.to_vec();
    }
    recovered.sort_by_key(|a| (a.received_at_ms, a.seq));
    let at = actions.iter().position(|a| a.instant || a.windowed).unwrap_or(actions.len());
    let mut out = actions[..at].to_vec();
    out.extend(recovered);
    out.extend_from_slice(&actions[at..]);
    out
}

/// Start, schema 8: stamp the beat's clock and stop every open duel's clock for the pause.
pub fn apply_clock(w: &mut World, c: &ClockIn) -> Value {
    let mut m = w.dm.mark.unwrap_or_default();
    m.round_ms = c.now_ms;
    w.dm.mark = Some(m);
    let mut moved = vec![];
    if let Some([from, to]) = c.pause {
        let mut bouts = duel::open_bouts(w);
        for b in bouts.iter_mut() {
            b.pause(from, to);
            moved.push(json!({ "duel_id": b.duel.id, "deadline_ms": (b.duel.deadline_ms > 0).then_some(b.duel.deadline_ms),
                               "pauses": b.pauses }));
        }
        if !bouts.is_empty() {
            w.dm.duels = bouts.iter().map(|b| serde_json::to_value(b).unwrap_or(Value::Null)).collect();
        }
    }
    json!({ "event": "duel.clock", "beat": w.meta.beat, "now_ms": c.now_ms,
            "pause": c.pause.map(|p| json!({ "from_ms": p[0], "to_ms": p[1], "ms": p[1] - p[0] })), "duels": moved })
}
