//! Synthetic recording → projection. The recording is written through the real recorder (`SqliteSink`), so the source
//! schema is exactly the recorder's.

use std::sync::Arc;

use rusqlite::Connection;
use serde_json::{json, Value};
use theatre_recorder::{wrap_state, SqliteSink};
use theatre_trace::{HttpRecord, Ids, Record, Sink};

use super::*;

const RUN: &str = "fn7-test-run";

fn tmp_db(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fn7-project-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("rec.sqlite")
}

fn world(final_: bool) -> Value {
    let mut w = json!({
        "meta": { "schema": 1, "beat": 0, "beats_total": 10, "beat_ms": 6000 },
        "places": [ { "id": 0, "name": "Docks", "kind": "district" } ],
        "groups": [
            { "id": 0, "name": "Harbor", "kind": "neighborhood", "founded_beat": 0 },
            { "id": 1, "name": "Laborers", "kind": "trade", "founded_beat": 0 },
            { "id": 2, "name": "Faithful of Nature", "kind": "faith", "founded_beat": 0 },
            { "id": 3, "name": "Followers of Vex", "kind": "following", "founded_beat": 0, "follows": 0 },
            { "id": 4, "name": "Followers of Ash", "kind": "following", "founded_beat": 0, "follows": 1 }
        ],
        "clusters": [
            { "id": 0, "label": "night-shift dockworkers", "size": 4000, "place": 0, "groups": [0, 1] },
            { "id": 1, "label": "storm-chapel faithful", "size": 900, "place": 0, "groups": [1, 2] }
        ],
        "characters": [],
        "demigods": [
            { "id": 0, "seat": 0, "name": "Vex", "alive": true, "hands": [0, 1], "following": 3 },
            { "id": 1, "seat": 1, "name": "Ash", "alive": true, "hands": [2, 3], "following": 4 }
        ],
        "items": [],
        "agents": [
            { "id": 0, "kind": { "kind": "hand", "demigod": 0, "hand": "left" }, "name": "Vex/L", "goal": "win", "spawned_beat": 0, "status": "live", "steps": 0 },
            { "id": 1, "kind": { "kind": "hand", "demigod": 0, "hand": "right" }, "name": "Vex/R", "goal": "win", "spawned_beat": 0, "status": "live", "steps": 0 },
            { "id": 2, "kind": { "kind": "hand", "demigod": 1, "hand": "left" }, "name": "Ash/L", "goal": "win", "spawned_beat": 0, "status": "live", "steps": 0 },
            { "id": 3, "kind": { "kind": "hand", "demigod": 1, "hand": "right" }, "name": "Ash/R", "goal": "win", "spawned_beat": 0, "status": "live", "steps": 0 },
            { "id": 4, "kind": { "kind": "god", "god": 0 }, "name": "Time", "goal": "keep the clock", "spawned_beat": 0, "status": "live", "steps": 0 }
        ],
        "gods": [ { "id": 0, "favor": [0.0, 0.0], "attention": 0.0, "paladins": [], "agent": 4, "invocations": 0 } ],
        "rulings": [], "intents": [], "ledger": { "rows": [], "folds": [] }, "memory": { "items": [] }, "beats": []
    });
    if final_ {
        w["meta"]["beat"] = json!(5);
        w["characters"] = json!([
            { "id": 0, "name": "Ada", "origin": { "materialized": { "cluster": 0, "index": 0 } }, "groups": [0, 3],
              "alive": true, "memorized_beat": 1, "extra_field_from_a_newer_schema": true },
            { "id": 1, "name": "Sel", "origin": { "paladin": { "god": 0 } }, "groups": [2], "alive": true, "memorized_beat": 3 }
        ]);
        w["agents"][0]["steps"] = json!(4);
        w["agents"].as_array_mut().unwrap().extend([
            json!({ "id": 5, "kind": { "kind": "subagent", "parent": 0 }, "name": "scout", "goal": "scout the cathedral", "spawned_beat": 1, "status": "done", "ended_beat": 3, "steps": 2, "report": "the cathedral is empty" }),
            json!({ "id": 6, "kind": { "kind": "subagent", "parent": 5 }, "name": "runner", "goal": "carry word", "spawned_beat": 2, "status": "expired", "ended_beat": 4, "steps": 1 }),
            json!({ "id": 7, "kind": { "kind": "possession", "parent": 1, "body": 0 }, "name": "Ada (possessed)", "goal": "steer Ada", "spawned_beat": 2, "status": "live", "steps": 3 }),
            json!({ "id": 8, "kind": { "kind": "paladin", "god": 0, "body": 1 }, "name": "Warden Sel", "goal": "enforce the price", "spawned_beat": 3, "status": "live", "steps": 1 }),
        ]);
        // The final state still holds lg2 (with a known_at the event stream never mentioned) and lg4; lg0, lg1, lg3 were
        // folded away and survive only through their events.
        w["ledger"]["rows"] = json!([
            ledger_row(
                2,
                3,
                Some(1),
                "collapse",
                json!(["dg:1"]),
                Some(
                    json!({ "owed_by": "dg:0", "owed_to": 0, "amount": 4.5, "due_beat": 7, "delays": 1, "status": "delayed", "collected_by": null })
                )
            ),
            ledger_row(4, 3, None, "leave", json!(["gr:1"]), None),
        ]);
        w["ledger"]["rows"][0]["known_at"] = json!([[0, 3], [1, 4]]);
        w["ledger"]["rows"][0]["known_by"] = json!([0, 1]);
        w["ledger"]["rows"][1]["actor"] = json!("ch:0");
    }
    w
}

fn ledger_row(
    id: u32,
    beat: u32,
    cause: Option<u32>,
    verb: &str,
    targets: Value,
    debt: Option<Value>,
) -> Value {
    json!({
        "id": id, "beat": beat, "source": { "by": "agent", "agent": 0 }, "actor": "dg:0", "verb": verb,
        "act": format!("Vex does {verb}"), "targets": targets, "place": 0, "magnitude": 5, "valence": -2,
        "witnessed_by": ["ch:0", "cl:0"], "known_by": [0], "told_to": [], "erased_from": [], "debt": debt,
        "cause": cause, "intent": 0
    })
}

struct Rec {
    sink: Arc<SqliteSink>,
}

impl Rec {
    fn ids(&self, round: u32) -> Ids {
        Ids {
            id: self.sink.next_id(),
            run: RUN.into(),
            round: Some(round),
            ..Default::default()
        }
    }
    fn event(&self, round: u32, kind: &str, actor: Option<&str>, data: Value) {
        let ctx = self.ids(round);
        self.sink.write(Record::Event {
            id: ctx.id,
            ctx,
            kind: kind.into(),
            actor: actor.map(str::to_string),
            at: 1_000 + round as i64,
            data,
        });
    }
    fn snapshot(&self, round: u32, label: &str, state: Value) {
        let ctx = self.ids(round);
        self.sink.write(Record::Snapshot {
            id: ctx.id,
            ctx,
            label: label.into(),
            at: 1_000 + round as i64,
            state,
        });
    }
}

fn synthetic(path: &std::path::Path) {
    let sink = Arc::new(SqliteSink::open(path).unwrap());
    sink.write(Record::RunStart {
        run: RUN.into(),
        plugin: "final_night_7".into(),
        at: 1_000,
        settings: json!({}),
        meta: json!({}),
    });
    let r = Rec { sink: sink.clone() };
    r.snapshot(0, "match_start", wrap_state(RUN, world(false)));

    r.event(1, "utterance", Some("dg:0"), json!({ "seat": 0, "hand": "left", "beat": 1, "text": "send someone to scout the cathedral", "mode": "type", "words": 6 }));
    r.event(1, "intent", Some("ag:0"), json!({ "intent": { "id": 0, "beat": 1, "agent": 0, "demigod": 0, "verb": "spawn", "targets": [{ "text": "the cathedral", "kind": "place", "near": null }],
        "magnitude": 2, "span": "send someone to scout the cathedral", "route": { "to": "spawn", "goal": "scout", "budget_beats": 3 } }, "routed_by": "keyword" }));
    r.event(1, "intent", Some("ag:1"), json!({ "id": 1, "beat": 1, "agent": 1, "demigod": 0, "verb": "rewind", "targets": ["dg:1"], "magnitude": 6, "span": "rewind Ash", "route": { "to": "god", "god": 0, "by": "keyword" } }));
    r.event(1, "agent_spawned", Some("ag:5"), json!({ "agent": { "id": 5, "kind": { "kind": "subagent", "parent": 0 }, "name": "scout", "goal": "scout the cathedral", "spawned_beat": 1, "status": "live", "steps": 0 } }));
    r.event(1, "memorized", Some("ch:0"), json!({ "character": { "id": 0, "name": "Ada", "origin": { "materialized": { "cluster": 0, "index": 0 } }, "groups": [0, 1], "memorized_beat": 1 }, "cluster": 0, "index": 0 }));
    r.event(
        1,
        "ledger_row",
        Some("dg:0"),
        ledger_row(0, 1, None, "burn", json!(["cl:0", "pl:0"]), None),
    );
    r.event(1, "ruling", Some("gd:0"), json!({ "id": 0, "beat": 1, "god": 0, "intent": 1, "invoker": "dg:0", "verdict": "bargain", "price": [{ "kind": "life" }, 2.0], "speech": "Two years.", "dispatched": null, "fallback": false }));
    r.event(1, "memory_write", Some("ch:0"), json!({ "items": [
        { "id": 0, "owner": "ch:0", "beat": 1, "kind": "witnessed", "text": "I saw the docks burn.", "about": ["dg:0"], "ledger": 0, "from": null, "salience": 8, "confidence": 1.0, "forgotten": false, "folded_into": null },
        { "id": 1, "owner": "ag:0", "beat": 1, "kind": "did", "text": "We burned the docks.", "about": ["cl:0"], "ledger": 0, "from": null, "salience": 6, "confidence": 1.0, "forgotten": false, "folded_into": null }
    ] }));
    r.event(1, "beat", None, json!({ "beat": 1, "started_at_ms": 1_000, "timing": { "ms": [["intake", 120.0], ["route", 5.0], ["act", 900.0], ["merge", 3.0], ["consequence", 7.0], ["narrate", 400.0]], "llm_calls": 9, "llm_fallbacks": 1, "llm_ms_max": 880.0 },
        "utterances": 2, "words": 9, "intents": 2, "proposals": 3, "applied": 2, "rows": [0, 0], "memorized": [0], "rulings": [0], "spawned": [5], "eliminated": [], "caption": "" }));

    // Beat 1's caption is narrated one beat late (during beat 2), in its own event.
    r.event(2, "caption", None, json!({ "beat": 1, "caption": "Smoke on the water.", "callouts": [], "fallback": false }));
    r.event(
        2,
        "agent_step",
        Some("ag:5"),
        json!({ "agent": 5, "output": "walking" }),
    );
    r.event(2, "agent_spawned", None, json!({ "id": 6, "kind": { "kind": "subagent", "parent": 5 }, "name": "runner", "goal": "carry word", "spawned_beat": 2, "status": "live", "steps": 0 }));
    r.event(2, "ledger_row", Some("dg:0"), ledger_row(1, 2, Some(0), "spread_fire", json!(["cl:1"]), Some(json!({ "owed_by": "dg:0", "owed_to": 0, "amount": 2.0, "due_beat": 4, "delays": 0, "status": "open", "collected_by": null }))));
    r.event(2, "ledger_row", Some("ch:0"), json!({ "id": 3, "beat": 2, "source": { "by": "rules" }, "actor": "ch:0", "verb": "join", "act": "Ada joins Vex", "targets": ["ch:0", "gr:3"], "magnitude": 1, "valence": 1,
        "witnessed_by": [], "known_by": [], "told_to": ["dg:0", "dg:0"], "erased_from": [], "debt": null, "cause": null, "intent": null }));
    r.event(
        2,
        "spread",
        None,
        json!({ "row": 0, "groups": [1], "path": [0, 1] }),
    );
    r.event(2, "agent_step", Some("ag:5"), json!({ "agent": 5 }));

    r.event(3, "ledger_row", Some("dg:0"), ledger_row(2, 3, Some(1), "collapse", json!(["dg:1"]), Some(json!({ "owed_by": "dg:0", "owed_to": 0, "amount": 3.0, "due_beat": 5, "delays": 0, "status": "open", "collected_by": null }))));
    r.event(
        3,
        "spread",
        None,
        json!({ "row": "lg:0", "groups": ["gr:2"], "path": [1, 2] }),
    );
    r.event(3, "memory_forget", Some("ch:0"), json!({ "ids": [0] }));
    r.event(
        3,
        "agent_ended",
        Some("ag:5"),
        json!({ "agent": 5, "status": "done", "report": "the cathedral is empty" }),
    );
    r.event(
        4,
        "spread",
        None,
        json!({ "row": 0, "groups": [4], "path": [2, 4] }),
    );
    r.event(4, "debt_collected", Some("gd:0"), json!({ "row": 1, "debt": { "owed_by": "dg:0", "owed_to": 0, "amount": 2.0, "due_beat": 4, "delays": 0, "status": "collected", "collected_by": 5 } }));
    r.event(4, "debt_delayed", Some("gd:0"), json!({ "row": 2 }));
    r.event(4, "memory_fold", None, json!({ "summary": { "id": 2, "owner": "ag:0", "beat": 4, "kind": "summary", "text": "Fire, then more fire.", "about": [], "ledger": null, "from": null, "salience": 5, "confidence": 1.0, "forgotten": false, "folded_into": null }, "from": [1] }));
    r.event(
        4,
        "elimination",
        Some("dg:1"),
        json!({ "demigod": 1, "cause": "collapse", "chain": [0, 1, 2] }),
    );
    r.event(
        4,
        "zoom",
        Some("dg:0"),
        json!({ "seat": 0, "zoom": "district", "focus": "pl:0" }),
    );
    r.event(
        4,
        "hand_said",
        Some("ag:0"),
        json!({ "text": "done, boss" }),
    );
    r.event(4, "some_future_kind", None, json!({ "ignored": true }));

    // One provider exchange in beat 1, for the beat-cost view.
    let ids = r.ids(1);
    sink.write(Record::Http(
        HttpRecord {
            started_at: 1_001,
            duration_ms: 800.0,
            url: "x".into(),
            status: Some(200),
            input_tokens: Some(100),
            output_tokens: Some(20),
            ..Default::default()
        },
        ids,
    ));

    r.snapshot(5, "committed", wrap_state(RUN, world(false)));
    r.snapshot(5, "final", wrap_state(RUN, world(true)));
    sink.close();
}

fn q<T: rusqlite::types::FromSql>(c: &Connection, sql: &str) -> T {
    c.query_row(sql, [], |r| r.get(0))
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

#[test]
fn projects_a_synthetic_recording() {
    let path = tmp_db("main");
    synthetic(&path);
    let mut c = Connection::open(&path).unwrap();
    let runs = all_runs(&c).unwrap();
    assert_eq!(runs, vec![RUN.to_string()]);
    let s = project(&mut c, &runs).unwrap().remove(0);
    assert_eq!(
        s.snapshots,
        vec!["match_start".to_string(), "final".to_string()]
    );
    assert_eq!(s.events, 27, "unknown kinds are skipped");

    assert_eq!(s.count("fn7_ledger"), 5);
    assert_eq!(s.count("fn7_agents"), 9);
    assert_eq!(s.count("fn7_rulings"), 1);
    assert_eq!(s.count("fn7_intents"), 2);
    assert_eq!(s.count("fn7_beats"), 1);
    assert_eq!(s.count("fn7_memory"), 3);
    // 1 place + 5 groups + 2 clusters + 2 characters + 2 demigods + 1 god + 9 agents.
    assert_eq!(s.count("fn7_entities"), 22);

    // Ledger: sources, debts (event-collected lg1, snapshot-delayed lg2), refs deduplicated.
    assert_eq!(
        q::<String>(&c, "SELECT source FROM fn7_ledger WHERE row_ref = 'lg:0'"),
        "agent ag:0"
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT debt_status FROM fn7_ledger WHERE row_ref = 'lg:1'"
        ),
        "collected"
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT debt_status FROM fn7_ledger WHERE row_ref = 'lg:2'"
        ),
        "delayed"
    );
    assert_eq!(
        q::<f64>(
            &c,
            "SELECT debt_amount FROM fn7_ledger WHERE row_ref = 'lg:2'"
        ),
        4.5
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT debt_god_ref FROM fn7_ledger WHERE row_ref = 'lg:2'"
        ),
        "gd:0"
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT place_ref FROM fn7_ledger WHERE row_ref = 'lg:0'"
        ),
        "pl:0"
    );
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT COUNT(*) FROM fn7_ledger_ref WHERE row_ref = 'lg:3' AND role = 'told'"
        ),
        1
    );
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT COUNT(*) FROM fn7_ledger_ref WHERE role = 'witness'"
        ),
        8
    );

    // Chains: lg2 ← lg1 ← lg0.
    assert_eq!(
        q::<String>(
            &c,
            "SELECT root_ref FROM v_fn7_chain WHERE row_ref = 'lg:2'"
        ),
        "lg:0"
    );
    assert_eq!(
        q::<i64>(&c, "SELECT depth FROM v_fn7_chain WHERE row_ref = 'lg:2'"),
        2
    );
    assert_eq!(
        q::<String>(&c, "SELECT path FROM v_fn7_chain WHERE row_ref = 'lg:2'"),
        "lg:0 > lg:1 > lg:2"
    );
    assert_eq!(q::<i64>(&c, "SELECT COUNT(*) FROM v_fn7_chain"), 5);

    // Agent tree: ag6 under ag5 under the left hand; the paladin under Time's mind; owners inherited.
    assert_eq!(
        q::<String>(
            &c,
            "SELECT root_ref FROM v_fn7_agent_tree WHERE agent_ref = 'ag:6'"
        ),
        "ag:0"
    );
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT depth FROM v_fn7_agent_tree WHERE agent_ref = 'ag:6'"
        ),
        2
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT demigod_ref FROM fn7_agents WHERE agent_ref = 'ag:6'"
        ),
        "dg:0"
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT parent_ref FROM fn7_agents WHERE agent_ref = 'ag:8'"
        ),
        "ag:4"
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT god_ref FROM fn7_agents WHERE agent_ref = 'ag:8'"
        ),
        "gd:0"
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT body_ref FROM fn7_agents WHERE agent_ref = 'ag:7'"
        ),
        "ch:0"
    );
    assert_eq!(
        q::<i64>(&c, "SELECT steps FROM fn7_agents WHERE agent_ref = 'ag:5'"),
        2
    );
    assert_eq!(
        q::<String>(&c, "SELECT status FROM fn7_agents WHERE agent_ref = 'ag:5'"),
        "done"
    );
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT COUNT(*) FROM fn7_agents WHERE parent_ref = 'ag:0'"
        ),
        1
    );

    // Rumor path for lg0: Harbor (witnessed, beat 1) → Laborers (2) → Faithful (3) → Ash's followers (4), reaching Ash.
    let mut st = c.prepare("SELECT hop, group_ref, beat, demigod_ref FROM v_fn7_rumor_path WHERE row_ref = 'lg:0' ORDER BY hop").unwrap();
    let path: Vec<(i64, String, i64, Option<String>)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop(st);
    assert_eq!(
        path,
        vec![
            (1, "gr:0".into(), 1, None),
            (2, "gr:1".into(), 2, None),
            (3, "gr:2".into(), 3, None),
            (4, "gr:4".into(), 4, Some("dg:1".into()))
        ]
    );
    // known_at from the final snapshot.
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT beat FROM fn7_known WHERE row_ref = 'lg:2' AND group_ref = 'gr:1'"
        ),
        4
    );

    // Membership: Ada inherits Harbor + Laborers at beat 1, joins Vex's following at 2, leaves Laborers at 3.
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT to_beat FROM fn7_membership WHERE member_ref = 'ch:0' AND group_ref = 'gr:1'"
        ),
        3
    );
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT from_beat FROM fn7_membership WHERE member_ref = 'ch:0' AND group_ref = 'gr:3'"
        ),
        2
    );
    assert_eq!(
        q::<Option<i64>>(
            &c,
            "SELECT to_beat FROM fn7_membership WHERE member_ref = 'ch:0' AND group_ref = 'gr:3'"
        ),
        None
    );
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT from_beat FROM fn7_membership WHERE member_ref = 'ch:1' AND group_ref = 'gr:2'"
        ),
        3
    );
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT COUNT(*) FROM fn7_membership WHERE member_ref = 'cl:1'"
        ),
        2
    );

    // Entities.
    assert_eq!(
        q::<String>(
            &c,
            "SELECT cluster_ref FROM fn7_entities WHERE ref = 'ch:0'"
        ),
        "cl:0"
    );
    assert_eq!(
        q::<String>(&c, "SELECT origin FROM fn7_entities WHERE ref = 'ch:0'"),
        "materialized"
    );
    assert_eq!(
        q::<i64>(&c, "SELECT ended_beat FROM fn7_entities WHERE ref = 'dg:1'"),
        4
    );
    assert_eq!(
        q::<String>(&c, "SELECT name FROM fn7_entities WHERE ref = 'gd:0'"),
        fn7_pantheon::load().gods[0].name.clone()
    );

    // Memory: forgotten by event, folded by event.
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT forgotten FROM fn7_memory WHERE mem_ref = 'mm:0'"
        ),
        1
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT folded_into FROM fn7_memory WHERE mem_ref = 'mm:1'"
        ),
        "mm:2"
    );

    // Rulings, intents, beats.
    assert_eq!(
        q::<String>(
            &c,
            "SELECT price_kind || ':' || price_amount FROM fn7_rulings"
        ),
        "life:2.0"
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT route || '/' || routed_by FROM fn7_intents WHERE intent_ref = 'in:0'"
        ),
        "spawn/keyword"
    );
    assert_eq!(
        q::<String>(
            &c,
            "SELECT route || '/' || routed_by FROM fn7_intents WHERE intent_ref = 'in:1'"
        ),
        "god/keyword"
    );
    assert_eq!(
        q::<f64>(&c, "SELECT total_ms FROM fn7_beats WHERE beat = 1"),
        1435.0
    );
    assert_eq!(
        q::<f64>(&c, "SELECT act_ms FROM fn7_beats WHERE beat = 1"),
        900.0
    );
    assert_eq!(
        q::<String>(&c, "SELECT caption FROM fn7_beats WHERE beat = 1"),
        "Smoke on the water."
    );
    assert_eq!(
        q::<i64>(
            &c,
            "SELECT input_tokens FROM v_fn7_beat_cost WHERE beat = 1"
        ),
        100
    );

    // Idempotent: a second projection gives identical counts and rows.
    let dump = |c: &Connection| -> Vec<String> {
        TABLES
            .iter()
            .flat_map(|t| {
                let mut st = c
                    .prepare(&format!("SELECT * FROM {t} ORDER BY 1, 2, 3, 4"))
                    .unwrap();
                let n = st.column_count();
                st.query_map([], |r| {
                    Ok((0..n)
                        .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                        .collect::<Vec<_>>()
                        .join("|"))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect::<Vec<_>>()
            })
            .collect()
    };
    let before = dump(&c);
    let s2 = project(&mut c, &runs).unwrap().remove(0);
    assert_eq!(s.counts, s2.counts);
    assert_eq!(before, dump(&c));
}

#[test]
fn empty_and_unknown_runs_project_to_nothing() {
    let path = tmp_db("empty");
    let sink = SqliteSink::open(&path).unwrap();
    sink.close();
    let mut c = Connection::open(&path).unwrap();
    let s = project(&mut c, &["nope".to_string()]).unwrap().remove(0);
    assert!(s.counts.iter().all(|(_, n)| *n == 0));
    // A database without the recorder's tables still gets the schema (and no beat-cost view).
    let mut bare = Connection::open_in_memory().unwrap();
    project(&mut bare, &[]).unwrap();
    assert_eq!(
        q::<i64>(
            &bare,
            "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'fn7_%' AND type = 'table'"
        ),
        TABLES.len() as i64
    );
    assert_eq!(
        q::<i64>(
            &bare,
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'v_fn7_beat_cost'"
        ),
        0
    );
}

#[test]
fn loose_json_helpers() {
    assert_eq!(rf(Some(&json!(5)), "lg").as_deref(), Some("lg:5"));
    assert_eq!(rf(Some(&json!("lg:5")), "lg").as_deref(), Some("lg:5"));
    assert_eq!(rf(Some(&json!({ "id": 2 })), "mm").as_deref(), Some("mm:2"));
    assert_eq!(rf(Some(&Value::Null), "lg"), None);
    assert_eq!(
        tag(Some(&json!("created")), "kind").as_deref(),
        Some("created")
    );
    assert_eq!(
        tag(Some(&json!({ "materialized": { "cluster": 1 } })), "kind").as_deref(),
        Some("materialized")
    );
    assert_eq!(
        tag(Some(&json!({ "to": "god", "god": 1 })), "to").as_deref(),
        Some("god")
    );
    assert_eq!(
        source_str(Some(&json!({ "by": "god", "god": 2, "ruling": 7 }))).as_deref(),
        Some("god gd:2 ru:7")
    );
    assert_eq!(
        source_str(Some(&json!({ "by": "resolver" }))).as_deref(),
        Some("resolver")
    );
}
