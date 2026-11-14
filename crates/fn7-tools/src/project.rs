//! `fn7-project`: projects a recording's generic `events` and `snapshots` into the typed `fn7_*` tables of
//! docs/DATA_MODEL.md §4, plus a few analysis views.
//!
//! Sources, in order (later wins for the same row): the `match_start` snapshot, the events (in recording order), then the
//! last `final` (or `committed`) snapshot. Rows folded out of the live state survive through their events. Event `data`
//! is read as loose JSON, so extra or missing fields never break a projection; ids may be bare numbers (`5`) or refs
//! (`"lg:5"`).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::Value;

/// The projection tables and indexes, exactly as docs/DATA_MODEL.md §4 (made idempotent).
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS fn7_entities   (run_id, ref, kind, name, origin, cluster_ref, created_beat, ended_beat, PRIMARY KEY (run_id, ref));
CREATE TABLE IF NOT EXISTS fn7_membership (run_id, member_ref, group_ref, from_beat, to_beat);
CREATE TABLE IF NOT EXISTS fn7_ledger     (run_id, row_ref, beat, source, actor_ref, verb, act, place_ref, magnitude, valence,
                                           cause_ref, intent_ref, debt_god_ref, debt_amount, debt_due, debt_status, PRIMARY KEY (run_id, row_ref));
CREATE TABLE IF NOT EXISTS fn7_ledger_ref (run_id, row_ref, role, ref);
CREATE TABLE IF NOT EXISTS fn7_known      (run_id, row_ref, group_ref, beat);
CREATE TABLE IF NOT EXISTS fn7_memory     (run_id, mem_ref, owner_ref, beat, kind, text, salience, confidence, ledger_ref, forgotten, folded_into);
CREATE TABLE IF NOT EXISTS fn7_agents     (run_id, agent_ref, kind, parent_ref, demigod_ref, god_ref, body_ref, goal, spawned_beat, ended_beat, status, steps, report);
CREATE TABLE IF NOT EXISTS fn7_rulings    (run_id, ruling_ref, beat, god_ref, invoker_ref, intent_ref, verdict, price_kind, price_amount, speech, fallback);
CREATE TABLE IF NOT EXISTS fn7_intents    (run_id, intent_ref, beat, agent_ref, demigod_ref, verb, magnitude, route, routed_by, span);
CREATE TABLE IF NOT EXISTS fn7_beats      (run_id, beat, started_at, intake_ms, route_ms, act_ms, merge_ms, consequence_ms, narrate_ms,
                                           total_ms, llm_calls, llm_fallbacks, words, intents, applied, caption);
CREATE TABLE IF NOT EXISTS fn7_geo        (run_id, row_ref, beat, lon, lat, place_ref, PRIMARY KEY (run_id, row_ref));
CREATE TABLE IF NOT EXISTS fn7_casts      (run_id, beat, by_ref, power, cost, lon, lat, region_ref, target_ref, row_ref, off_target, fizzled);
CREATE TABLE IF NOT EXISTS fn7_slows      (run_id, window, initiator_ref, opened_beat, closed_beat, how, beats_open, ms_open, measured,
                                           released, cut_off, debt, entropy, paid_bolt);
CREATE TABLE IF NOT EXISTS fn7_cutoffs    (run_id, beat, demigod_ref, window, reason, sent_at, enter_at);
CREATE TABLE IF NOT EXISTS fn7_entropy    (run_id, beat, before, after, level);
CREATE TABLE IF NOT EXISTS fn7_mods       (run_id, mod_id, beat, region_ref, name, lon, lat, radius_km, stacked, expired_beat);
CREATE INDEX IF NOT EXISTS fn7_ledger_actor  ON fn7_ledger (run_id, actor_ref, beat);
CREATE INDEX IF NOT EXISTS fn7_ledger_cause  ON fn7_ledger (run_id, cause_ref);
CREATE INDEX IF NOT EXISTS fn7_ledger_ref_ix ON fn7_ledger_ref (run_id, ref, role);
CREATE INDEX IF NOT EXISTS fn7_known_group   ON fn7_known (run_id, group_ref, beat);
CREATE INDEX IF NOT EXISTS fn7_memory_owner  ON fn7_memory (run_id, owner_ref, beat);
CREATE INDEX IF NOT EXISTS fn7_member_group  ON fn7_membership (run_id, group_ref);
CREATE INDEX IF NOT EXISTS fn7_agents_parent ON fn7_agents (run_id, parent_ref);
"#;

/// Analysis views (dropped and recreated each run so their definitions stay current).
pub const VIEWS: &str = r#"
DROP VIEW IF EXISTS v_fn7_chain;
-- Final Destination chains: every ledger row with the root of its cause chain, its depth below the root and the path.
CREATE VIEW v_fn7_chain AS
WITH RECURSIVE chain(run_id, row_ref, root_ref, depth, path) AS (
    SELECT l.run_id, l.row_ref, l.row_ref, 0, l.row_ref
    FROM fn7_ledger l
    WHERE l.cause_ref IS NULL
       OR NOT EXISTS (SELECT 1 FROM fn7_ledger p WHERE p.run_id = l.run_id AND p.row_ref = l.cause_ref)
    UNION ALL
    SELECT l.run_id, l.row_ref, c.root_ref, c.depth + 1, c.path || ' > ' || l.row_ref
    FROM fn7_ledger l JOIN chain c ON l.run_id = c.run_id AND l.cause_ref = c.row_ref
    WHERE c.depth < 10000
)
SELECT c.run_id, c.row_ref, c.root_ref, c.depth, c.path, l.beat, l.actor_ref, l.verb, l.act
FROM chain c JOIN fn7_ledger l ON l.run_id = c.run_id AND l.row_ref = c.row_ref;

DROP VIEW IF EXISTS v_fn7_agent_tree;
-- The orchestration tree: every agent with its root (a hand or a god's mind), depth and path.
CREATE VIEW v_fn7_agent_tree AS
WITH RECURSIVE tree(run_id, agent_ref, root_ref, depth, path) AS (
    SELECT a.run_id, a.agent_ref, a.agent_ref, 0, a.agent_ref
    FROM fn7_agents a
    WHERE a.parent_ref IS NULL
       OR NOT EXISTS (SELECT 1 FROM fn7_agents p WHERE p.run_id = a.run_id AND p.agent_ref = a.parent_ref)
    UNION ALL
    SELECT a.run_id, a.agent_ref, t.root_ref, t.depth + 1, t.path || ' > ' || a.agent_ref
    FROM fn7_agents a JOIN tree t ON a.run_id = t.run_id AND a.parent_ref = t.agent_ref
    WHERE t.depth < 1000
)
SELECT t.run_id, t.agent_ref, t.root_ref, t.depth, t.path, a.kind, a.parent_ref, a.demigod_ref, a.god_ref, a.body_ref,
       a.goal, a.status, a.spawned_beat, a.ended_beat, a.steps
FROM tree t JOIN fn7_agents a ON a.run_id = t.run_id AND a.agent_ref = t.agent_ref;

DROP VIEW IF EXISTS v_fn7_rumor_path;
-- How news of a ledger row travelled over the Venn: one row per hop (group reached, in beat order), with the demigods
-- who were members of that group at that beat (NULL when none).
CREATE VIEW v_fn7_rumor_path AS
SELECT h.run_id, h.row_ref, h.hop, h.group_ref, h.beat, m.member_ref AS demigod_ref
FROM (SELECT k.run_id, k.row_ref, k.group_ref, k.beat,
             ROW_NUMBER() OVER (PARTITION BY k.run_id, k.row_ref ORDER BY k.beat, k.group_ref) AS hop
      FROM fn7_known k) h
LEFT JOIN fn7_membership m
       ON m.run_id = h.run_id AND m.group_ref = h.group_ref AND m.member_ref LIKE 'dg:%'
      AND (m.from_beat IS NULL OR m.from_beat <= h.beat) AND (m.to_beat IS NULL OR m.to_beat > h.beat);
"#;

/// Needs the recorder's `v_http_by_step` (assumes Theatre round = FN7 beat).
pub const BEAT_COST_VIEW: &str = r#"
DROP VIEW IF EXISTS v_fn7_beat_cost;
-- Where each beat's time and tokens went: fn7_beats joined to the recorder's per-round HTTP spend.
CREATE VIEW v_fn7_beat_cost AS
SELECT b.run_id, b.beat, b.total_ms, b.llm_calls, b.llm_fallbacks, b.words, b.intents, b.applied,
       COALESCE(SUM(h.requests), 0) AS http_requests,
       COALESCE(SUM(h.input_tokens), 0) AS input_tokens,
       COALESCE(SUM(h.output_tokens), 0) AS output_tokens,
       MAX(h.max_ms) AS http_max_ms,
       COALESCE(SUM(h.errors), 0) AS http_errors
FROM fn7_beats b LEFT JOIN v_http_by_step h ON h.run_id = b.run_id AND h.round = b.beat
GROUP BY b.run_id, b.beat;
"#;

pub const TABLES: &[&str] = &[
    "fn7_entities",
    "fn7_membership",
    "fn7_ledger",
    "fn7_ledger_ref",
    "fn7_known",
    "fn7_memory",
    "fn7_agents",
    "fn7_rulings",
    "fn7_intents",
    "fn7_beats",
    "fn7_geo",
    "fn7_casts",
    "fn7_slows",
    "fn7_cutoffs",
    "fn7_entropy",
    "fn7_mods",
];

/// Event kinds this projection reads (others are ignored).
pub const EVENT_KINDS: &[&str] = &[
    "cast",
    "cast_fizzled",
    "slow_started",
    "slow_closed",
    "orders_cut_off",
    "entropy",
    "mod_created",
    "mod_expired",
    "utterance",
    "intent",
    "mention_resolved",
    "memorized",
    "agent_spawned",
    "agent_step",
    "agent_ended",
    "ruling",
    "proposal",
    "resolution",
    "ledger_row",
    "spread",
    "debt_due",
    "debt_delayed",
    "debt_collected",
    "rewind",
    "memory_write",
    "memory_forget",
    "memory_fold",
    "zoom",
    "beat",
    "caption",
    "elimination",
    "hand_said",
];

/// Row counts per table for one projected run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunSummary {
    pub run: String,
    pub events: usize,
    pub snapshots: Vec<String>,
    pub counts: Vec<(String, i64)>,
}

impl RunSummary {
    pub fn count(&self, table: &str) -> i64 {
        self.counts
            .iter()
            .find(|(t, _)| t == table)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }
}

fn has_object(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
        params![name],
        |r| r.get::<_, i64>(0),
    )? > 0)
}

/// Create the `fn7_*` tables, indexes and views (idempotent).
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    conn.execute_batch(VIEWS)?;
    if has_object(conn, "v_http_by_step")? {
        conn.execute_batch(BEAT_COST_VIEW)?;
    }
    Ok(())
}

/// Every run in the recording (from `runs`, plus any run that only has events or snapshots), oldest first.
pub fn all_runs(conn: &Connection) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    if has_object(conn, "runs")? {
        let mut st = conn.prepare("SELECT id FROM runs ORDER BY started_at, id")?;
        out.extend(
            st.query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    for t in ["events", "snapshots"] {
        if has_object(conn, t)? {
            let mut st = conn.prepare(&format!(
                "SELECT run_id FROM {t} GROUP BY run_id ORDER BY MIN(id)"
            ))?;
            for r in st.query_map([], |r| r.get::<_, String>(0))? {
                let r = r?;
                if !out.contains(&r) {
                    out.push(r);
                }
            }
        }
    }
    Ok(out)
}

/// Project the given runs inside one transaction (each run's old rows are deleted first).
pub fn project(conn: &mut Connection, runs: &[String]) -> Result<Vec<RunSummary>> {
    ensure_schema(conn)?;
    let tx = conn.transaction()?;
    let mut out = Vec::new();
    for run in runs {
        out.push(project_run(&tx, run).with_context(|| format!("projecting run {run}"))?);
    }
    tx.commit()?;
    Ok(out)
}

// ------------------------------------------------------------------------------------------------------------------
// Loose JSON helpers
// ------------------------------------------------------------------------------------------------------------------

/// An id as a ref: `5` → `"lg:5"`, `"lg:5"` stays, `{ "id": 5 }` → `"lg:5"`.
fn rf(v: Option<&Value>, prefix: &str) -> Option<String> {
    match v? {
        Value::Number(n) => n.as_u64().map(|n| format!("{prefix}:{n}")),
        Value::String(s) if s.contains(':') => Some(s.clone()),
        Value::String(s) => s
            .trim()
            .parse::<u64>()
            .ok()
            .map(|n| format!("{prefix}:{n}")),
        Value::Object(o) => rf(o.get("id"), prefix),
        _ => None,
    }
}

/// A `Ref` field (always a `"xx:N"` string); mentions and other shapes are skipped.
fn any_ref(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .filter(|s| s.contains(':'))
        .map(str::to_string)
}

fn refs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| any_ref(Some(x)))
        .collect()
}

fn st(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn int(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.parse().ok(),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn boolean(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => Some(n.as_f64().unwrap_or(0.0) != 0.0),
        _ => None,
    }
}

/// The variant name of a serde enum: `"created"`, `{ "materialized": {...} }` (external) or `{ key: "god", ... }` (internal).
fn tag(v: Option<&Value>, key: &str) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => match o.get(key).and_then(Value::as_str) {
            Some(t) => Some(t.to_string()),
            None if o.len() == 1 => o.keys().next().cloned(),
            None => None,
        },
        _ => None,
    }
}

/// The payload of an externally tagged variant (`{ "materialized": { "cluster": 3 } }` → `{ "cluster": 3 }`), or the
/// object itself for internally tagged ones.
fn variant_body(v: Option<&Value>) -> Option<&Value> {
    match v? {
        Value::Object(o) if o.len() == 1 && o.values().next().is_some_and(Value::is_object) => {
            o.values().next()
        }
        o @ Value::Object(_) => Some(o),
        _ => None,
    }
}

/// The object an event is about: `data[key]` if it is an object, otherwise `data` itself.
fn inner<'a>(data: &'a Value, keys: &[&str]) -> &'a Value {
    keys.iter()
        .find_map(|k| data.get(*k).filter(|v| v.is_object()))
        .unwrap_or(data)
}

/// Objects carried by an event: an array, `data[key]` (array or object), or `data` itself if it has an `id`.
fn objects<'a>(data: &'a Value, keys: &[&str]) -> Vec<&'a Value> {
    if let Some(a) = data.as_array() {
        return a.iter().filter(|x| x.is_object()).collect();
    }
    for k in keys {
        match data.get(*k) {
            Some(Value::Array(a)) => return a.iter().filter(|x| x.is_object()).collect(),
            Some(o @ Value::Object(_)) => return vec![o],
            _ => {}
        }
    }
    if data.get("id").is_some() {
        vec![data]
    } else {
        vec![]
    }
}

/// Bare ids carried by an event (`data[key]` as an array of numbers/refs, or a single one).
fn ids(data: &Value, keys: &[&str], prefix: &str) -> Vec<String> {
    for k in keys {
        match data.get(*k) {
            Some(Value::Array(a)) => return a.iter().filter_map(|x| rf(Some(x), prefix)).collect(),
            Some(v @ (Value::Number(_) | Value::String(_))) => {
                return rf(Some(v), prefix).into_iter().collect()
            }
            _ => {}
        }
    }
    vec![]
}

/// Find the `World` inside a snapshot state (unwrapping the recorder's run tag and an optional `world` key).
fn world_of(state: &Value) -> Value {
    let (_, s) = theatre_recorder::unwrap_state(state);
    if s.get("ledger").is_some() || s.get("meta").is_some() {
        return s;
    }
    for k in ["world", "state", "game"] {
        if let Some(w) = s
            .get(k)
            .filter(|w| w.get("ledger").is_some() || w.get("meta").is_some())
        {
            return w.clone();
        }
    }
    s
}

fn arr<'a>(v: &'a Value, k: &str) -> impl Iterator<Item = (usize, &'a Value)> {
    v.get(k)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
}

/// Recursive-merge `new` over `old` (objects merge key by key; anything else is replaced).
fn overlay(old: &mut Value, new: &Value) {
    match (old, new) {
        (Value::Object(o), Value::Object(n)) => {
            for (k, v) in n {
                match o.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => overlay(slot, v),
                    _ => {
                        o.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (o, n) => *o = n.clone(),
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Accumulated projection
// ------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct Entity {
    kind: String,
    name: Option<String>,
    origin: Option<String>,
    cluster_ref: Option<String>,
    created_beat: Option<i64>,
    ended_beat: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Op {
    Join,
    Leave,
}

/// From an `agent_ended` event: (ended beat, status, report).
type AgentEnd = (Option<i64>, Option<String>, Option<String>);

#[derive(Default)]
struct Proj {
    entities: BTreeMap<String, Entity>,
    /// (beat, seq, member, group, op).
    member_ops: Vec<(i64, usize, String, String, Op)>,
    seeded_members: BTreeSet<String>,
    group_dissolved: BTreeMap<String, i64>,
    ledger: BTreeMap<String, Value>,
    known: BTreeMap<(String, String), i64>,
    memory: BTreeMap<String, Value>,
    forgotten: BTreeSet<String>,
    folded: BTreeMap<String, String>,
    agents: BTreeMap<String, Value>,
    agent_steps: BTreeMap<String, i64>,
    agent_end: BTreeMap<String, AgentEnd>,
    god_agent: BTreeMap<String, String>,
    rulings: BTreeMap<String, Value>,
    intents: BTreeMap<String, (Value, Option<String>)>,
    beats: BTreeMap<i64, Value>,
    /// Beat → caption from `caption` events (the narrator runs one beat late, so the `beat` event has none).
    captions: BTreeMap<i64, String>,
    /// Planet-edition events, kept as they came: (kind, beat, data).
    planet: Vec<(String, Option<i64>, Value)>,
}

impl Proj {
    fn entity(&mut self, r: String, e: Entity) {
        let slot = self.entities.entry(r).or_insert_with(|| Entity {
            kind: e.kind.clone(),
            ..Default::default()
        });
        if !e.kind.is_empty() {
            slot.kind = e.kind;
        }
        if e.name.is_some() {
            slot.name = e.name;
        }
        if e.origin.is_some() {
            slot.origin = e.origin;
        }
        if e.cluster_ref.is_some() {
            slot.cluster_ref = e.cluster_ref;
        }
        slot.created_beat = match (slot.created_beat, e.created_beat) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if e.ended_beat.is_some() {
            slot.ended_beat = e.ended_beat;
        }
    }

    fn join(&mut self, beat: i64, member: &str, groups: impl IntoIterator<Item = String>) {
        for g in groups {
            let seq = self.member_ops.len();
            self.member_ops
                .push((beat, seq, member.to_string(), g, Op::Join));
        }
    }

    fn character(&mut self, r: String, c: &Value, beat_hint: Option<i64>) {
        let origin = c.get("origin");
        let cluster = variant_body(origin).and_then(|b| rf(b.get("cluster"), "cl"));
        self.entity(
            r,
            Entity {
                kind: "character".into(),
                name: st(c, "name"),
                origin: tag(origin, "kind"),
                cluster_ref: cluster,
                created_beat: int(c.get("memorized_beat")).or(beat_hint),
                ended_beat: None,
            },
        );
    }

    fn agent(&mut self, r: String, a: &Value) {
        match self.agents.get_mut(&r) {
            Some(old) => overlay(old, a),
            None => {
                self.agents.insert(r, a.clone());
            }
        }
    }

    /// Everything in one `World` value. `start` = the match_start snapshot (seeds memberships).
    fn world(&mut self, w: &Value, start: bool) {
        let beat0 = int(w.pointer("/meta/beat")).unwrap_or(0);
        let pantheon = fn7_pantheon::load();
        for (i, p) in arr(w, "places") {
            let r = rf(p.get("id"), "pl").unwrap_or(format!("pl:{i}"));
            self.entity(
                r,
                Entity {
                    kind: "place".into(),
                    name: st(p, "name"),
                    origin: tag(p.get("kind"), "kind"),
                    created_beat: Some(beat0),
                    ..Default::default()
                },
            );
        }
        for (i, g) in arr(w, "groups") {
            let r = rf(g.get("id"), "gr").unwrap_or(format!("gr:{i}"));
            let ended = int(g.get("dissolved_beat"));
            if let Some(e) = ended {
                self.group_dissolved.insert(r.clone(), e);
            }
            self.entity(
                r,
                Entity {
                    kind: "group".into(),
                    name: st(g, "name"),
                    origin: tag(g.get("kind"), "kind"),
                    created_beat: int(g.get("founded_beat")).or(Some(beat0)),
                    ended_beat: ended,
                    ..Default::default()
                },
            );
        }
        for (i, c) in arr(w, "clusters") {
            let r = rf(c.get("id"), "cl").unwrap_or(format!("cl:{i}"));
            if start && self.seeded_members.insert(r.clone()) {
                let gs: Vec<String> = c
                    .get("groups")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|g| rf(Some(g), "gr"))
                    .collect();
                self.join(beat0, &r, gs);
            }
            self.entity(
                r,
                Entity {
                    kind: "cluster".into(),
                    name: st(c, "label"),
                    origin: Some("cluster".into()),
                    created_beat: Some(beat0),
                    ..Default::default()
                },
            );
        }
        for (i, c) in arr(w, "characters") {
            let r = rf(c.get("id"), "ch").unwrap_or(format!("ch:{i}"));
            self.character(r.clone(), c, Some(beat0));
            // Characters first seen here (no match_start membership, no memorized event) start in their current groups.
            if self.seeded_members.insert(r.clone()) {
                let at = if start {
                    beat0
                } else {
                    int(c.get("memorized_beat")).unwrap_or(beat0)
                };
                let gs: Vec<String> = c
                    .get("groups")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|g| rf(Some(g), "gr"))
                    .collect();
                self.join(at, &r, gs);
            }
        }
        for (i, d) in arr(w, "demigods") {
            let r = rf(d.get("id"), "dg").unwrap_or(format!("dg:{i}"));
            if self.seeded_members.insert(r.clone()) {
                if let Some(g) = rf(d.get("following"), "gr") {
                    self.join(beat0, &r, [g]);
                }
            }
            let origin = int(d.get("seat")).map(|s| format!("seat {s}"));
            self.entity(
                r,
                Entity {
                    kind: "demigod".into(),
                    name: st(d, "name"),
                    origin,
                    created_beat: Some(beat0),
                    ended_beat: int(d.get("eliminated_beat")),
                    ..Default::default()
                },
            );
        }
        for (i, it) in arr(w, "items") {
            let r = rf(it.get("id"), "it").unwrap_or(format!("it:{i}"));
            self.entity(
                r,
                Entity {
                    kind: "item".into(),
                    name: st(it, "name"),
                    origin: tag(it.get("origin"), "kind"),
                    created_beat: int(it.get("created_beat")),
                    ..Default::default()
                },
            );
        }
        for (i, g) in arr(w, "gods") {
            let r = rf(g.get("id"), "gd").unwrap_or(format!("gd:{i}"));
            let ix = r
                .split_once(':')
                .and_then(|(_, n)| n.parse::<usize>().ok())
                .unwrap_or(i);
            let def = pantheon.gods.get(ix);
            if let Some(a) = rf(g.get("agent"), "ag") {
                self.god_agent.insert(r.clone(), a);
            }
            self.entity(
                r,
                Entity {
                    kind: "god".into(),
                    name: def.map(|d| d.name.clone()),
                    origin: def.map(|d| d.slug.clone()),
                    created_beat: Some(beat0),
                    ..Default::default()
                },
            );
        }
        for (i, a) in arr(w, "agents") {
            let r = rf(a.get("id"), "ag").unwrap_or(format!("ag:{i}"));
            self.agent(r, a);
        }
        for (i, x) in arr(w, "rulings") {
            let r = rf(x.get("id"), "ru").unwrap_or(format!("ru:{i}"));
            self.rulings.insert(r, x.clone());
        }
        for (i, x) in arr(w, "intents") {
            let r = rf(x.get("id"), "in").unwrap_or(format!("in:{i}"));
            let by = self.intents.get(&r).and_then(|(_, b)| b.clone());
            self.intents.insert(r, (x.clone(), by));
        }
        let rows = w
            .pointer("/ledger/rows")
            .or_else(|| w.get("ledger").filter(|l| l.is_array()));
        for x in rows.and_then(Value::as_array).into_iter().flatten() {
            if let Some(r) = rf(x.get("id"), "lg") {
                self.ledger.insert(r, x.clone());
            }
        }
        let items = w
            .pointer("/memory/items")
            .or_else(|| w.get("memory").filter(|l| l.is_array()));
        for x in items.and_then(Value::as_array).into_iter().flatten() {
            if let Some(r) = rf(x.get("id"), "mm") {
                self.memory.insert(r, x.clone());
            }
        }
        for x in w
            .get("beats")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(b) = int(x.get("beat")) {
                // The `beat` event's record (with wall-clock timings) wins over the state's copy.
                self.beats.entry(b).or_insert_with(|| x.clone());
            }
        }
    }

    fn event(&mut self, round: Option<i64>, kind: &str, actor: Option<&str>, data: &Value) {
        let beat = int(data.get("beat")).or(round);
        match kind {
            "intent" => {
                let obj = inner(data, &["intent"]);
                let by = st(data, "routed_by")
                    .or_else(|| data.get("by").and_then(Value::as_str).map(str::to_string));
                if let Some(r) = rf(obj.get("id"), "in") {
                    let by = by.or_else(|| self.intents.get(&r).and_then(|(_, b)| b.clone()));
                    self.intents.insert(r, (obj.clone(), by));
                }
            }
            "memorized" => {
                let c = inner(data, &["character", "char"]);
                if let Some(r) = rf(c.get("id"), "ch") {
                    self.character(r.clone(), c, round);
                    if let Some(cl) = rf(data.get("cluster"), "cl") {
                        if let Some(e) = self.entities.get_mut(&r) {
                            e.cluster_ref.get_or_insert(cl);
                        }
                    }
                    if self.seeded_members.insert(r.clone()) {
                        let at = int(c.get("memorized_beat")).or(round).unwrap_or(0);
                        let gs: Vec<String> = c
                            .get("groups")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|g| rf(Some(g), "gr"))
                            .collect();
                        self.join(at, &r, gs);
                    }
                }
            }
            "agent_spawned" => {
                let a = inner(data, &["agent"]);
                if let Some(r) = rf(a.get("id"), "ag")
                    .or_else(|| any_ref(actor.map(|s| Value::String(s.into())).as_ref()))
                {
                    self.agent(r, a);
                }
            }
            "agent_step" => {
                let r = rf(data.get("agent"), "ag")
                    .or_else(|| actor.filter(|a| a.starts_with("ag:")).map(str::to_string));
                if let Some(r) = r {
                    *self.agent_steps.entry(r).or_default() += 1;
                }
            }
            "agent_ended" => {
                let a = inner(data, &["agent"]);
                let r = rf(a.get("id"), "ag")
                    .or_else(|| rf(data.get("agent"), "ag"))
                    .or_else(|| actor.filter(|a| a.starts_with("ag:")).map(str::to_string));
                if let Some(r) = r {
                    if a.get("kind").is_some() {
                        self.agent(r.clone(), a);
                    }
                    let report = st(data, "report").or_else(|| st(a, "report"));
                    let status = st(data, "status").or_else(|| st(a, "status"));
                    self.agent_end
                        .insert(r, (int(a.get("ended_beat")).or(beat), status, report));
                }
            }
            "ruling" => {
                let x = inner(data, &["ruling"]);
                if let Some(r) = rf(x.get("id"), "ru") {
                    self.rulings.insert(r, x.clone());
                }
            }
            "ledger_row" => {
                let x = inner(data, &["row", "ledger_row"]);
                if let Some(r) = rf(x.get("id"), "lg") {
                    self.ledger.insert(r, x.clone());
                }
            }
            "spread" => {
                let row = rf(data.get("row"), "lg").or_else(|| rf(data.get("id"), "lg"));
                let groups = ids(
                    data,
                    &["groups", "reached", "new_groups", "newly_reached", "group"],
                    "gr",
                );
                if let (Some(row), Some(b)) = (row, beat) {
                    for g in groups {
                        self.known_at(&row, g, b);
                    }
                }
            }
            "debt_due" | "debt_delayed" | "debt_collected" => {
                let row = rf(data.get("row"), "lg").or_else(|| rf(data.get("id"), "lg"));
                let Some(row) = row else { return };
                // A full row in `data.row` replaces ours; a `debt` object replaces its debt.
                if let Some(full) = data
                    .get("row")
                    .filter(|v| v.is_object() && v.get("verb").is_some())
                {
                    self.ledger.insert(row.clone(), full.clone());
                }
                if let Some(slot) = self.ledger.get_mut(&row) {
                    match data.get("debt").filter(|d| d.is_object()) {
                        Some(d) => {
                            slot["debt"] = d.clone();
                        }
                        None if slot.get("debt").is_some_and(Value::is_object) => {
                            let status = match kind {
                                "debt_delayed" => "delayed",
                                "debt_collected" => "collected",
                                _ => "open",
                            };
                            slot["debt"]["status"] = Value::String(status.into());
                        }
                        None => {}
                    }
                }
            }
            "memory_write" | "memory_fold" => {
                for x in objects(data, &["items", "item", "memory", "originals", "folded"]) {
                    if let Some(r) = rf(x.get("id"), "mm") {
                        self.memory.insert(r, x.clone());
                    }
                }
                if kind == "memory_fold" {
                    let summary = data.get("summary").filter(|s| s.is_object());
                    if let Some(s) = summary {
                        if let Some(r) = rf(s.get("id"), "mm") {
                            self.memory.insert(r, s.clone());
                        }
                    }
                    let into = summary
                        .and_then(|s| rf(s.get("id"), "mm"))
                        .or_else(|| rf(data.get("into"), "mm"))
                        .or_else(|| rf(data.get("summary"), "mm"));
                    if let Some(into) = into {
                        for m in ids(data, &["from", "folded_ids", "ids"], "mm") {
                            self.folded.insert(m, into.clone());
                        }
                    }
                }
            }
            "memory_forget" => {
                for x in objects(data, &["items", "item", "memory"]) {
                    if let Some(r) = rf(x.get("id"), "mm") {
                        self.memory.insert(r.clone(), x.clone());
                        self.forgotten.insert(r);
                    }
                }
                for r in ids(data, &["ids", "items", "item", "id"], "mm") {
                    self.forgotten.insert(r);
                }
            }
            "beat" => {
                let x = inner(data, &["record", "beat_record"]);
                if let Some(b) = int(x.get("beat")).or(round) {
                    self.beats.insert(b, x.clone());
                }
            }
            "caption" => {
                if let (Some(b), Some(c)) = (beat, data.get("caption").and_then(Value::as_str)) {
                    self.captions.insert(b, c.to_string());
                }
            }
            "elimination" => {
                let d = rf(data.get("demigod"), "dg")
                    .or_else(|| actor.filter(|a| a.starts_with("dg:")).map(str::to_string));
                if let Some(d) = d {
                    self.entity(
                        d,
                        Entity {
                            kind: "demigod".into(),
                            ended_beat: beat,
                            ..Default::default()
                        },
                    );
                }
            }
            "cast" | "cast_fizzled" | "slow_started" | "slow_closed" | "orders_cut_off" | "entropy"
            | "mod_created" | "mod_expired" => {
                let mut d = data.clone();
                if let (Some(a), Value::Object(o)) = (actor, &mut d) {
                    o.entry("actor").or_insert(a.into());
                }
                self.planet.push((kind.to_string(), beat, d));
            }
            // utterance, mention_resolved, proposal, resolution, rewind, zoom, hand_said: no table of their own.
            _ => {}
        }
    }

    fn known_at(&mut self, row: &str, group: String, beat: i64) {
        let slot = self.known.entry((row.to_string(), group)).or_insert(beat);
        *slot = (*slot).min(beat);
    }

    /// Derived rows that need everything collected first: known-by from ledger rows, membership changes.
    fn finish(&mut self) {
        let rows: Vec<(String, Value)> = self
            .ledger
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (r, row) in &rows {
            let row_beat = int(row.get("beat")).unwrap_or(0);
            // known_at: [[group, beat], ...] | [{group, beat}, ...] | {"gr:1": beat}.
            match row.get("known_at") {
                Some(Value::Array(a)) => {
                    for x in a {
                        let (g, b) = match x {
                            Value::Array(p) => (rf(p.first(), "gr"), int(p.get(1))),
                            Value::Object(_) => (rf(x.get("group"), "gr"), int(x.get("beat"))),
                            _ => (None, None),
                        };
                        if let Some(g) = g {
                            self.known_at(r, g, b.unwrap_or(row_beat));
                        }
                    }
                }
                Some(Value::Object(o)) => {
                    for (g, b) in o {
                        if let Some(g) = rf(Some(&Value::String(g.clone())), "gr") {
                            self.known_at(r, g, int(Some(b)).unwrap_or(row_beat));
                        }
                    }
                }
                _ => {}
            }
            for g in row
                .get("known_by")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(g) = rf(Some(g), "gr") {
                    self.known.entry((r.clone(), g)).or_insert(row_beat);
                }
            }
            // Membership changes: an explicit effect if the row carries one, else the verb plus targets.
            let fx = row.get("effect");
            let (op, member, group) = match fx.and_then(|e| tag(Some(e), "fx")).as_deref() {
                Some("join") | Some("leave") => {
                    let e = variant_body(fx).unwrap_or(&Value::Null);
                    let op = if tag(fx, "fx").as_deref() == Some("join") {
                        Op::Join
                    } else {
                        Op::Leave
                    };
                    (Some(op), any_ref(e.get("member")), rf(e.get("group"), "gr"))
                }
                _ => {
                    let verb = row
                        .get("verb")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_lowercase();
                    let op = match verb.as_str() {
                        "join" | "joins" | "joined" | "recruit" | "convert" => Some(Op::Join),
                        "leave" | "leaves" | "left" | "desert" | "defect" => Some(Op::Leave),
                        _ => None,
                    };
                    let targets = refs(row.get("targets"));
                    let group = targets.iter().find(|t| t.starts_with("gr:")).cloned();
                    let member = targets
                        .iter()
                        .find(|t| !t.starts_with("gr:"))
                        .cloned()
                        .or_else(|| any_ref(row.get("actor")));
                    (op, member, group)
                }
            };
            if let (Some(op), Some(m), Some(g)) = (op, member, group) {
                let seq = self.member_ops.len();
                self.member_ops.push((row_beat, seq, m, g, op));
            }
        }
    }

    fn membership(&self) -> Vec<(String, String, i64, Option<i64>)> {
        let mut ops = self.member_ops.clone();
        ops.sort();
        let mut open: BTreeMap<(String, String), i64> = BTreeMap::new();
        let mut out = Vec::new();
        for (beat, _, m, g, op) in ops {
            let key = (m, g);
            match op {
                Op::Join => {
                    open.entry(key).or_insert(beat);
                }
                Op::Leave => {
                    if let Some(from) = open.remove(&key) {
                        out.push((key.0, key.1, from, Some(beat)));
                    }
                }
            }
        }
        for ((m, g), from) in open {
            let to = self.group_dissolved.get(&g).copied().filter(|d| *d >= from);
            out.push((m, g, from, to));
        }
        out.sort();
        out
    }
}

/// Walk parents to find the demigod (hands) or god (god minds, paladins) an agent ultimately works for.
fn agent_owner(r: &str, field: &str, cache: &BTreeMap<String, AgentRow>) -> Option<String> {
    let mut cur = r.to_string();
    for _ in 0..64 {
        let a = cache.get(&cur)?;
        let v = if field == "demigod" {
            &a.demigod_ref
        } else {
            &a.god_ref
        };
        if v.is_some() {
            return v.clone();
        }
        cur = a.parent_ref.clone()?;
    }
    None
}

#[derive(Debug, Clone, Default)]
struct AgentRow {
    kind: Option<String>,
    parent_ref: Option<String>,
    demigod_ref: Option<String>,
    god_ref: Option<String>,
    body_ref: Option<String>,
    goal: Option<String>,
    spawned_beat: Option<i64>,
    ended_beat: Option<i64>,
    status: Option<String>,
    steps: i64,
    report: Option<String>,
}

fn source_str(v: Option<&Value>) -> Option<String> {
    let v = v?;
    let by = tag(Some(v), "by")?;
    let body = variant_body(Some(v)).unwrap_or(v);
    let mut parts = vec![by];
    if let Some(g) = rf(body.get("god"), "gd") {
        parts.push(g);
    }
    if let Some(r) = rf(body.get("ruling"), "ru") {
        parts.push(r);
    }
    if let Some(a) = rf(body.get("agent"), "ag") {
        parts.push(a);
    }
    Some(parts.join(" "))
}

fn stage_ms(timing: &Value, stage: &str) -> Option<f64> {
    match timing.get("ms")? {
        Value::Array(a) => {
            let hits: Vec<f64> = a
                .iter()
                .filter_map(|p| match p {
                    Value::Array(x) if x.first().and_then(Value::as_str) == Some(stage) => {
                        num(x.get(1))
                    }
                    Value::Object(_) if p.get("stage").and_then(Value::as_str) == Some(stage) => {
                        num(p.get("ms"))
                    }
                    _ => None,
                })
                .collect();
            (!hits.is_empty()).then(|| hits.iter().sum())
        }
        Value::Object(o) => num(o.get(stage)),
        _ => None,
    }
}

fn total_ms(b: &Value) -> Option<f64> {
    if let Some(t) = num(b.get("total_ms")) {
        return Some(t);
    }
    let timing = b.get("timing")?;
    match timing.get("ms")? {
        Value::Array(a) => Some(
            a.iter()
                .filter_map(|p| match p {
                    Value::Array(x) => num(x.get(1)),
                    Value::Object(_) => num(p.get("ms")),
                    _ => None,
                })
                .sum(),
        ),
        Value::Object(o) => Some(o.values().filter_map(|v| num(Some(v))).sum()),
        _ => None,
    }
}

fn load_snapshot(
    tx: &Transaction,
    run: &str,
    labels: &[&str],
    latest: bool,
) -> Result<Option<(String, Value)>> {
    let order = if latest { "DESC" } else { "ASC" };
    for l in labels {
        let row: Option<String> = tx
            .query_row(&format!("SELECT state FROM snapshots WHERE run_id = ?1 AND label = ?2 ORDER BY id {order} LIMIT 1"), params![run, l], |r| r.get(0))
            .optional()?;
        if let Some(s) = row {
            return Ok(Some((
                l.to_string(),
                serde_json::from_str(&s).unwrap_or(Value::Null),
            )));
        }
    }
    Ok(None)
}

fn project_run(tx: &Transaction, run: &str) -> Result<RunSummary> {
    let mut p = Proj::default();
    let mut summary = RunSummary {
        run: run.to_string(),
        ..Default::default()
    };
    let have_snaps = tx.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = 'snapshots'",
        [],
        |r| r.get::<_, i64>(0),
    )? > 0;
    let have_events = tx.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = 'events'",
        [],
        |r| r.get::<_, i64>(0),
    )? > 0;

    if have_snaps {
        if let Some((label, s)) = load_snapshot(tx, run, &["match_start"], false)? {
            p.world(&world_of(&s), true);
            summary.snapshots.push(label);
        }
    }
    if have_events {
        let kinds = EVENT_KINDS
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(",");
        let mut stmt = tx.prepare(&format!("SELECT round, kind, actor, data FROM events WHERE run_id = ?1 AND kind IN ({kinds}) ORDER BY id"))?;
        let rows = stmt.query_map(params![run], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        for row in rows {
            let (round, kind, actor, data) = row?;
            let data: Value = data
                .and_then(|d| serde_json::from_str(&d).ok())
                .unwrap_or(Value::Null);
            p.event(round, &kind, actor.as_deref(), &data);
            summary.events += 1;
        }
    }
    if have_snaps {
        if let Some((label, s)) = load_snapshot(
            tx,
            run,
            &["final", "committed", "resolved", "round_start"],
            true,
        )? {
            p.world(&world_of(&s), false);
            summary.snapshots.push(label);
        }
    }
    p.finish();

    for t in TABLES {
        tx.execute(&format!("DELETE FROM {t} WHERE run_id = ?1"), params![run])?;
    }

    // Agents: rows first (entities and owner walks need them).
    let mut agents: BTreeMap<String, AgentRow> = BTreeMap::new();
    let agent_json = std::mem::take(&mut p.agents);
    for (r, a) in &agent_json {
        let kind = a.get("kind");
        let k = tag(kind, "kind");
        let body = variant_body(kind).unwrap_or(&Value::Null);
        let god_ref = rf(body.get("god"), "gd");
        let mut parent_ref = rf(body.get("parent"), "ag");
        if parent_ref.is_none() && k.as_deref() == Some("paladin") {
            parent_ref = god_ref.as_ref().and_then(|g| p.god_agent.get(g)).cloned();
        }
        let (end_beat, end_status, end_report) = p.agent_end.get(r).cloned().unwrap_or_default();
        let row = AgentRow {
            kind: k.clone(),
            parent_ref,
            demigod_ref: rf(body.get("demigod"), "dg"),
            god_ref,
            body_ref: rf(body.get("body"), "ch"),
            goal: st(a, "goal"),
            spawned_beat: int(a.get("spawned_beat")),
            ended_beat: int(a.get("ended_beat")).or(end_beat),
            status: st(a, "status")
                .filter(|s| s != "live" || end_status.is_none())
                .or(end_status),
            steps: int(a.get("steps"))
                .unwrap_or(0)
                .max(p.agent_steps.get(r).copied().unwrap_or(0)),
            report: st(a, "report").or(end_report),
        };
        let e = Entity {
            kind: "agent".into(),
            name: st(a, "name"),
            origin: k,
            created_beat: row.spawned_beat,
            ended_beat: row.ended_beat,
            ..Default::default()
        };
        p.entity(r.clone(), e);
        agents.insert(r.clone(), row);
    }
    let owners: Vec<(String, Option<String>, Option<String>)> = agents
        .keys()
        .map(|r| {
            (
                r.clone(),
                agent_owner(r, "demigod", &agents),
                agent_owner(r, "god", &agents),
            )
        })
        .collect();
    for (r, d, g) in owners {
        if let Some(a) = agents.get_mut(&r) {
            a.demigod_ref = a.demigod_ref.take().or(d);
            a.god_ref = a.god_ref.take().or(g);
        }
    }
    {
        let mut ins = tx.prepare(
            "INSERT INTO fn7_agents (run_id, agent_ref, kind, parent_ref, demigod_ref, god_ref, body_ref, goal, spawned_beat, ended_beat, status, steps, report) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        )?;
        for (r, a) in &agents {
            ins.execute(params![
                run,
                r,
                a.kind,
                a.parent_ref,
                a.demigod_ref,
                a.god_ref,
                a.body_ref,
                a.goal,
                a.spawned_beat,
                a.ended_beat,
                a.status,
                a.steps,
                a.report
            ])?;
        }
    }
    {
        let mut ins = tx.prepare("INSERT INTO fn7_entities (run_id, ref, kind, name, origin, cluster_ref, created_beat, ended_beat) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)")?;
        for (r, e) in &p.entities {
            let kind = if e.kind.is_empty() {
                r.split(':').next().unwrap_or("").to_string()
            } else {
                e.kind.clone()
            };
            ins.execute(params![
                run,
                r,
                kind,
                e.name,
                e.origin,
                e.cluster_ref,
                e.created_beat,
                e.ended_beat
            ])?;
        }
    }
    {
        let mut ins = tx.prepare("INSERT INTO fn7_membership (run_id, member_ref, group_ref, from_beat, to_beat) VALUES (?1,?2,?3,?4,?5)")?;
        for (m, g, from, to) in p.membership() {
            ins.execute(params![run, m, g, from, to])?;
        }
    }
    {
        let mut ins = tx.prepare(
            "INSERT INTO fn7_ledger (run_id, row_ref, beat, source, actor_ref, verb, act, place_ref, magnitude, valence, cause_ref, intent_ref, debt_god_ref, debt_amount, debt_due, debt_status) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        )?;
        let mut ins_ref = tx.prepare(
            "INSERT INTO fn7_ledger_ref (run_id, row_ref, role, ref) VALUES (?1,?2,?3,?4)",
        )?;
        for (r, row) in &p.ledger {
            let debt = row.get("debt").filter(|d| d.is_object());
            ins.execute(params![
                run,
                r,
                int(row.get("beat")),
                source_str(row.get("source")),
                any_ref(row.get("actor")),
                st(row, "verb"),
                st(row, "act"),
                rf(row.get("place"), "pl"),
                int(row.get("magnitude")),
                int(row.get("valence")),
                rf(row.get("cause"), "lg"),
                rf(row.get("intent"), "in"),
                debt.and_then(|d| rf(d.get("owed_to"), "gd")),
                debt.and_then(|d| num(d.get("amount"))),
                debt.and_then(|d| int(d.get("due_beat"))),
                debt.and_then(|d| st(d, "status")),
            ])?;
            for (role, key) in [
                ("target", "targets"),
                ("witness", "witnessed_by"),
                ("told", "told_to"),
                ("erased", "erased_from"),
            ] {
                let mut seen = BTreeSet::new();
                for x in refs(row.get(key)) {
                    if seen.insert(x.clone()) {
                        ins_ref.execute(params![run, r, role, x])?;
                    }
                }
            }
        }
    }
    {
        let mut ins = tx.prepare(
            "INSERT INTO fn7_known (run_id, row_ref, group_ref, beat) VALUES (?1,?2,?3,?4)",
        )?;
        let mut rows: Vec<(&(String, String), &i64)> = p.known.iter().collect();
        rows.sort_by(|a, b| (&a.0 .0, a.1, &a.0 .1).cmp(&(&b.0 .0, b.1, &b.0 .1)));
        for ((row, g), b) in rows {
            ins.execute(params![run, row, g, b])?;
        }
    }
    {
        let mut ins = tx.prepare(
            "INSERT INTO fn7_memory (run_id, mem_ref, owner_ref, beat, kind, text, salience, confidence, ledger_ref, forgotten, folded_into) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )?;
        for (r, m) in &p.memory {
            let forgotten = boolean(m.get("forgotten")).unwrap_or(false) || p.forgotten.contains(r);
            let folded = rf(m.get("folded_into"), "mm").or_else(|| p.folded.get(r).cloned());
            ins.execute(params![
                run,
                r,
                any_ref(m.get("owner")),
                int(m.get("beat")),
                tag(m.get("kind"), "kind"),
                st(m, "text"),
                int(m.get("salience")),
                num(m.get("confidence")),
                rf(m.get("ledger"), "lg"),
                forgotten as i64,
                folded
            ])?;
        }
    }
    {
        let mut ins = tx.prepare(
            "INSERT INTO fn7_rulings (run_id, ruling_ref, beat, god_ref, invoker_ref, intent_ref, verdict, price_kind, price_amount, speech, fallback) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )?;
        for (r, x) in &p.rulings {
            // price: [ {kind: "life"}, 2.0 ] (a tuple) | { kind, amount } | null.
            let (pk, pa) = match x.get("price") {
                Some(Value::Array(a)) => (tag(a.first(), "kind"), num(a.get(1))),
                Some(o @ Value::Object(_)) => (
                    tag(o.get("kind"), "kind").or_else(|| tag(Some(o), "kind")),
                    num(o.get("amount")),
                ),
                _ => (None, None),
            };
            ins.execute(params![
                run,
                r,
                int(x.get("beat")),
                rf(x.get("god"), "gd"),
                any_ref(x.get("invoker")),
                rf(x.get("intent"), "in"),
                tag(x.get("verdict"), "verdict"),
                pk,
                pa,
                st(x, "speech"),
                boolean(x.get("fallback")).map(|b| b as i64)
            ])?;
        }
    }
    {
        let mut ins = tx.prepare(
            "INSERT INTO fn7_intents (run_id, intent_ref, beat, agent_ref, demigod_ref, verb, magnitude, route, routed_by, span) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        )?;
        for (r, (x, by)) in &p.intents {
            let route = x.get("route");
            let routed_by = by.clone().or_else(|| {
                route
                    .and_then(|r| variant_body(Some(r)))
                    .and_then(|b| st(b, "by"))
            });
            ins.execute(params![
                run,
                r,
                int(x.get("beat")),
                rf(x.get("agent"), "ag"),
                rf(x.get("demigod"), "dg"),
                st(x, "verb"),
                int(x.get("magnitude")),
                tag(route, "to"),
                routed_by,
                st(x, "span")
            ])?;
        }
    }
    {
        let mut ins = tx.prepare(
            "INSERT INTO fn7_beats (run_id, beat, started_at, intake_ms, route_ms, act_ms, merge_ms, consequence_ms, narrate_ms, total_ms, llm_calls, llm_fallbacks, words, intents, applied, caption) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        )?;
        for (b, x) in &p.beats {
            let timing = x.get("timing").cloned().unwrap_or(Value::Null);
            ins.execute(params![
                run,
                b,
                int(x.get("started_at_ms")).or_else(|| int(x.get("started_at"))),
                stage_ms(&timing, "intake"),
                stage_ms(&timing, "route"),
                stage_ms(&timing, "act"),
                stage_ms(&timing, "merge"),
                stage_ms(&timing, "consequence"),
                stage_ms(&timing, "narrate"),
                total_ms(x),
                int(timing.get("llm_calls")),
                int(timing.get("llm_fallbacks")),
                int(x.get("words")),
                int(x.get("intents")),
                int(x.get("applied")),
                p.captions
                    .get(b)
                    .cloned()
                    .or_else(|| st(x, "caption").filter(|c| !c.is_empty()))
            ])?;
        }
    }

    write_planet(tx, run, &p)?;

    for t in TABLES {
        let n: i64 = tx.query_row(
            &format!("SELECT COUNT(*) FROM {t} WHERE run_id = ?1"),
            params![run],
            |r| r.get(0),
        )?;
        summary.counts.push((t.to_string(), n));
    }
    Ok(summary)
}

/// The planet edition's tables: geotags of every ledger row, casts, slow windows, cut-offs, the Entropy curve, mods.
fn write_planet(tx: &Transaction, run: &str, p: &Proj) -> Result<()> {
    let lonlat = |v: Option<&Value>| -> (Option<f64>, Option<f64>) {
        match v.and_then(Value::as_array) {
            Some(a) if a.len() == 2 => (a[0].as_f64(), a[1].as_f64()),
            _ => (None, None),
        }
    };
    let mut geo = tx.prepare("INSERT OR REPLACE INTO fn7_geo (run_id, row_ref, beat, lon, lat, place_ref) VALUES (?1,?2,?3,?4,?5,?6)")?;
    for (r, row) in &p.ledger {
        let (lon, lat) = lonlat(row.get("at"));
        if lon.is_some() || row.get("place").is_some() {
            geo.execute(params![run, r, int(row.get("beat")), lon, lat, rf(row.get("place"), "pl")])?;
        }
    }
    let mut cast = tx.prepare("INSERT INTO fn7_casts (run_id, beat, by_ref, power, cost, lon, lat, region_ref, target_ref, row_ref, off_target, fizzled) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)")?;
    let mut slow = tx.prepare("INSERT INTO fn7_slows (run_id, window, initiator_ref, opened_beat, closed_beat, how, beats_open, ms_open, measured, released, cut_off, debt, entropy, paid_bolt) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)")?;
    let mut cut = tx.prepare("INSERT INTO fn7_cutoffs (run_id, beat, demigod_ref, window, reason, sent_at, enter_at) VALUES (?1,?2,?3,?4,?5,?6,?7)")?;
    let mut ent = tx.prepare("INSERT INTO fn7_entropy (run_id, beat, before, after, level) VALUES (?1,?2,?3,?4,?5)")?;
    let mut md = tx.prepare("INSERT INTO fn7_mods (run_id, mod_id, beat, region_ref, name, lon, lat, radius_km, stacked, expired_beat) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)")?;
    let mut opened: BTreeMap<i64, (Option<String>, Option<i64>)> = BTreeMap::new();
    for (kind, beat, d) in &p.planet {
        match kind.as_str() {
            "cast" | "cast_fizzled" => {
                let (lon, lat) = lonlat(d.get("at"));
                cast.execute(params![
                    run, beat, rf(d.get("by"), "dg"), st(d, "power"), int(d.get("cost")), lon, lat,
                    rf(d.get("region"), "pl"), d.get("target").and_then(|t| any_ref(Some(t))),
                    rf(d.get("row"), "lg"), d.get("off_target").and_then(Value::as_bool),
                    (kind == "cast_fizzled").then(|| st(d, "reason")).flatten()
                ])?;
            }
            "slow_started" => {
                if let Some(wi) = int(d.get("window")) {
                    opened.insert(wi, (rf(d.get("initiator"), "dg"), *beat));
                }
            }
            "slow_closed" => {
                let wi = int(d.get("window"));
                slow.execute(params![
                    run, wi, rf(d.get("initiator"), "dg"), wi, beat, st(d, "how"), int(d.get("beats_open")),
                    int(d.get("ms_open")), st(d, "measured"), int(d.get("released")), int(d.get("cut_off")),
                    num(d.get("debt")), num(d.get("entropy")), d.get("paid_bolt").and_then(Value::as_bool)
                ])?;
                if let Some(w) = wi {
                    opened.remove(&w);
                }
            }
            "orders_cut_off" => {
                cut.execute(params![
                    run, beat, rf(d.get("demigod"), "dg"), int(d.get("window")), st(d, "reason"),
                    int(d.get("sent_at")), int(d.get("enter_at"))
                ])?;
            }
            "entropy" => {
                ent.execute(params![run, beat, num(d.get("before")), num(d.get("after")), int(d.get("level"))])?;
            }
            "mod_created" => {
                let m = d.get("mod").cloned().unwrap_or(Value::Null);
                let (lon, lat) = lonlat(m.get("at"));
                md.execute(params![
                    run, int(m.get("id")), beat, rf(m.get("region"), "pl"), st(&m, "name"), lon, lat,
                    num(m.get("radius_km")), d.get("stacked").and_then(Value::as_bool), Option::<i64>::None
                ])?;
            }
            "mod_expired" => {
                tx.execute(
                    "UPDATE fn7_mods SET expired_beat = ?3 WHERE run_id = ?1 AND mod_id = ?2 AND expired_beat IS NULL",
                    params![run, int(d.get("id")), beat],
                )?;
            }
            _ => {}
        }
    }
    // Windows still open at the end of the recording.
    for (w, (who, b)) in opened {
        slow.execute(params![run, w, who, b, Option::<i64>::None, "open", Option::<i64>::None, Option::<i64>::None,
            Option::<String>::None, Option::<i64>::None, Option::<i64>::None, Option::<f64>::None, Option::<f64>::None,
            Option::<bool>::None])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
