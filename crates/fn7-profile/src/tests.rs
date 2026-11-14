use std::io::BufReader;

use serde_json::{json, Value};

use crate::ingest::{read_recording_from, MatchInput, SeatIn, Source};
use crate::normalize::{display_name, normalize_name, skeleton};
use crate::*;

/// A `match.ended` summary in the real shape (trimmed from runs/lan/recordings/0f8ba721…, the shared match).
fn summary(champion_seat: Option<u8>) -> Value {
    json!({
        "champion_seat": champion_seat,
        "champions": [{ "demigod": "dg:2", "health": 3.42, "name": "Vex", "seat": null }],
        "final_cut": { "title": "FINAL NIGHT 7: Vex Stands", "story": "…", "fallback": false },
        "standings": [
            { "alive": true, "cause": null, "demigod": "dg:2", "eliminated_beat": null, "name": "Vex", "rank": 1, "score": 9.0, "seat": null },
            { "alive": false, "cause": "a debt collected by Consequence", "demigod": "dg:1", "eliminated_beat": 21, "name": "Benito", "rank": 2, "score": 0.0, "seat": 1 },
            { "alive": false, "cause": "a debt collected by Consequence", "demigod": "dg:3", "eliminated_beat": 20, "name": "Mara", "rank": 3, "score": -98.0, "seat": null },
            { "alive": false, "cause": "a debt collected by Consequence", "demigod": "dg:0", "eliminated_beat": 17, "name": "Chris", "rank": 4, "score": 0.0, "seat": 0 }
        ],
        "piles": [
            { "alive": false, "bolts": 62, "demigod": "dg:0", "name": "Chris", "seat": 0, "slows": 4,
              "items": [{ "item": "it:0", "name": "Rusty Pipe", "origin": "summoned", "origin_ref": null }],
              "summons": [{ "item": "it:0", "name": "Rusty Pipe" }, { "item": "it:5", "name": "Banner of the Horde of 100,000,000" }],
              "stats": { "acts": 8, "debt_paid": 5.44, "words": 9 },
              "statue_image": { "hash": "aaaa01", "kind": "statue", "status": "pending", "url": null } },
            { "alive": false, "bolts": 55, "demigod": "dg:1", "name": "Benito", "seat": 1, "slows": 3, "items": [], "summons": [],
              "stats": { "acts": 5 }, "statue_image": { "hash": "bbbb01", "kind": "statue" } },
            { "alive": true, "demigod": "dg:2", "name": "Vex", "seat": null, "items": [], "summons": [],
              "statue_image": { "hash": "cccc01", "kind": "statue" } }
        ],
        "storyboard": [
            { "beat": 0, "kind": "title", "caption": "FINAL NIGHT 7", "refs": ["dg:0", "dg:1", "dg:2", "dg:3"], "image_kind": "event",
              "image": { "hash": "title01", "kind": "event" } },
            { "beat": 2, "kind": "moment", "caption": "Benito casts flood on Northern America", "refs": ["dg:1"], "row": "lg:38",
              "image_kind": "event", "image": { "hash": "flood01", "kind": "event" } },
            { "beat": 2, "kind": "moment", "caption": "Benito casts flood on Northern America", "refs": ["dg:1"], "row": "lg:39",
              "image_kind": "event", "image": { "hash": "flood01", "kind": "event" } },
            { "beat": 12, "kind": "moment", "caption": "Vex casts lightning bolt on Chris in Kyiv", "refs": ["dg:2", "dg:0"], "row": "lg:351",
              "image_kind": "event", "image": { "hash": "kyiv01", "kind": "event" } },
            { "beat": 17, "kind": "champion", "caption": "Vex walks on", "refs": ["dg:2"], "image_kind": "portrait",
              "image": { "hash": "vexp01", "kind": "portrait" } }
        ],
        "stats": { "beats": 21, "beats_total": 30 }
    })
}

fn public_state() -> Value {
    json!({
        "beat": 21,
        "demigods": [
            { "ref": "dg:0", "name": "Chris", "seat": 0, "alive": false,
              "visual": { "sheet_hash": "c5a7e1b8", "traits": { "build": "willowy and tall", "colors": ["copper", "teal"] } },
              "portrait_image": { "hash": "chrisp01", "kind": "portrait" }, "diorama_image": { "hash": "chrisd01", "kind": "diorama" } },
            { "ref": "dg:1", "name": "Benito", "seat": 1, "alive": false,
              "visual": { "sheet_hash": "beni0001", "traits": {} }, "portrait_image": { "hash": "benip01", "kind": "portrait" } },
            { "ref": "dg:2", "name": "Vex", "seat": null, "alive": true, "portrait_image": { "hash": "vexp01", "kind": "portrait" } }
        ]
    })
}

fn seats() -> Vec<SeatIn> {
    vec![
        SeatIn { seat: 0, display_name: "Chris".into(), is_bot: false },
        SeatIn { seat: 1, display_name: "Benito".into(), is_bot: false },
    ]
}

fn ended(id: &str, at: i64) -> MatchInput {
    MatchInput {
        match_id: id.into(),
        room_code: Some("DYL5GN".into()),
        started_at: Some(at - 300_000),
        ended_at: Some(at),
        rules_version: Some("0.5.0".into()),
        beats_total: Some(30),
        forked_from: None,
        seats: seats(),
        summary: Some(summary(None)),
        rounds: Some(21),
        public_state: Some(public_state()),
        eliminations: vec![
            json!({ "beat": 17, "cause": "a debt collected by Consequence", "chain": [195, 89], "demigod": 0, "name": "Chris" }),
            json!({ "beat": 20, "cause": "a debt collected by Consequence", "chain": [242], "demigod": 3, "name": "Mara" }),
            json!({ "beat": 21, "cause": "a debt collected by Consequence", "chain": [255], "demigod": 1, "name": "Benito" }),
        ],
        recording: Some(format!("{id}.jsonl")),
        source: Source::Backfill,
    }
}

#[test]
fn names_normalize() {
    assert_eq!(normalize_name("  Chris  ").as_deref(), Some("chris"));
    assert_eq!(normalize_name("CHRIS").as_deref(), Some("chris"));
    assert_eq!(normalize_name("Ｃｈｒｉｓ").as_deref(), Some("chris"), "fullwidth folds under NFKC");
    assert_eq!(normalize_name("Ch\u{200B}ris").as_deref(), Some("chris"), "zero-width space dropped");
    assert_eq!(normalize_name("Chris\u{202E}").as_deref(), Some("chris"), "bidi override dropped");
    assert_eq!(normalize_name("Big   \t Ben").as_deref(), Some("big ben"));
    assert_eq!(normalize_name("Ⅻ").as_deref(), Some("xii"));
    assert_eq!(normalize_name("   "), None);
    assert_eq!(normalize_name("\u{200B}\u{FEFF}"), None);
    assert_eq!(normalize_name(&"x".repeat(80)).unwrap().chars().count(), 40);
    assert_eq!(normalize_name("Benito").as_deref(), Some("benito"));
    assert_eq!(display_name("  Big \u{200B}Ben "), "Big Ben");
}

#[test]
fn lookalikes_share_a_skeleton() {
    let k = |s: &str| skeleton(&normalize_name(s).unwrap());
    assert_eq!(k("Chris"), k("Chr1s"));
    assert_eq!(k("Chris"), k("\u{0421}hris"), "Cyrillic Es");
    assert_eq!(k("Chris"), k("chris_"));
    assert_eq!(k("Chris"), k("Chrls"));
    assert_ne!(k("Chris"), k("Benito"));
    assert_eq!(k("Benito"), k("Ben1t0"));
}

#[test]
fn migrations_run_once() {
    let dir = std::env::temp_dir().join(format!("fn7-profile-test-{}-{}", std::process::id(), now_ms()));
    let path = dir.join("p.sqlite");
    let a = ProfileStore::open(&path).unwrap();
    assert_eq!(a.version().unwrap(), schema::latest());
    a.upsert_profile("Chris").unwrap();
    drop(a);
    let b = ProfileStore::open(&path).unwrap();
    assert_eq!(b.version().unwrap(), schema::latest());
    assert!(b.profile("chris").unwrap().is_some(), "data survives reopening");
    drop(b);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn record_match_writes_history_identities_and_library() {
    let s = ProfileStore::open_in_memory().unwrap();
    let rep = s.record_match(&ended("0f8ba721-aaaa", 1_794_656_935_083)).unwrap();
    assert_eq!(rep.players, vec!["chris", "benito"]);
    assert!(!rep.skipped);

    let chris = s.profile("CHRIS").unwrap().unwrap();
    assert_eq!(chris.display_name, "Chris");
    assert_eq!((chris.totals.matches, chris.totals.finished, chris.totals.champion), (1, 1, 0));
    assert_eq!(chris.totals.best_placement, Some(4));
    assert_eq!(chris.totals.falls, 1);
    assert_eq!(chris.totals.nemesis, None, "one fall is not a nemesis");
    assert_eq!(chris.demigods.len(), 1);
    let d = &chris.demigods[0];
    assert_eq!((d.name.as_str(), d.sheet_hash.as_deref(), d.portrait_hash.as_deref()), ("Chris", Some("c5a7e1b8"), Some("chrisp01")));
    assert_eq!(d.diorama_hash.as_deref(), Some("chrisd01"));
    assert_eq!(d.matches, 1);
    assert!(chris.description.auto.contains("1 match as Chris"), "{}", chris.description.auto);
    assert!(chris.description.auto.contains("Best finish: 4th"), "{}", chris.description.auto);

    let h = s.history("chris", 0, None).unwrap();
    assert_eq!(h.total, 1);
    let m = &h.items[0];
    assert_eq!((m.seat, m.placement, m.field, m.champion), (0, Some(4), Some(4), false));
    assert_eq!(m.outcome, "ended");
    assert_eq!(m.champions, vec!["Vex"]);
    assert_eq!(m.eliminated_beat, Some(17));
    assert_eq!(m.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Rusty Pipe", "Banner of the Horde of 100,000,000"]);
    assert_eq!(m.deeds.len(), 1, "only the moment that shows Chris");
    assert_eq!(m.deeds[0].beat, 12);
    assert_eq!(m.eliminations.len(), 3);
    assert_eq!(m.eliminations.iter().filter(|e| e.own).count(), 1);
    assert_eq!(m.stats["bolts"], 62);
    assert_eq!(m.stats["acts"], 8);
    assert_eq!(m.time_travel.timeline, "/admin/matches/0f8ba721-aaaa/timeline");
    assert!(m.time_travel.state.contains("after_round={n}"));
    assert_eq!(m.time_travel.recording.as_deref(), Some("0f8ba721-aaaa.jsonl"));

    let lib = s.library("chris", 0, None, None).unwrap();
    let mut kinds: Vec<&str> = lib.items.iter().map(|i| i.kind.as_str()).collect();
    kinds.sort();
    assert_eq!(kinds, ["diorama", "event", "event", "portrait", "statue"], "{:?}", lib.items);
    assert!(lib.items.iter().all(|i| i.matches == vec!["0f8ba721-aaaa".to_string()]));
    // Never someone else's statue or portrait.
    assert!(!lib.items.iter().any(|i| ["bbbb01", "cccc01", "vexp01", "benip01", "flood01"].contains(&i.hash.as_str())));

    // Benito: second, his flood moment once (two shots, one image), the title card.
    let b = s.history("benito", 0, None).unwrap();
    assert_eq!(b.items[0].placement, Some(2));
    assert_eq!(b.items[0].deeds.len(), 1);
    let bl = s.library("benito", 0, None, Some("event")).unwrap();
    assert_eq!(bl.total, 2);
    // AI demigods never get a profile.
    assert!(s.profile("Vex").unwrap().is_none());
    assert!(s.profile("Mara").unwrap().is_none());
}

#[test]
fn record_match_is_idempotent_and_never_downgrades() {
    let s = ProfileStore::open_in_memory().unwrap();
    let m = ended("m1", 1_000_000);
    s.record_match(&m).unwrap();
    s.record_match(&m).unwrap();
    assert_eq!(s.history("chris", 0, None).unwrap().total, 1);
    let lib = s.library("chris", 0, None, None).unwrap();
    assert!(lib.items.iter().all(|i| i.matches.len() == 1));
    // The same recording read while it was still running: unfinished, never replaces the ended record.
    let mut early = m.clone();
    early.summary = None;
    early.ended_at = None;
    assert!(s.record_match(&early).unwrap().skipped);
    assert_eq!(s.history("chris", 0, None).unwrap().items[0].outcome, "ended");
    // An unfinished match is upgraded once it ends.
    let mut u = ended("m2", 2_000_000);
    u.summary = None;
    s.record_match(&u).unwrap();
    let h = s.history("chris", 0, None).unwrap();
    assert_eq!((h.total, h.items[0].outcome.as_str(), h.items[0].placement), (2, "unfinished", None));
    assert_eq!(h.items[0].demigods.len(), 1, "demigods come from the public state");
    s.record_match(&ended("m2", 2_000_000)).unwrap();
    let h = s.history("chris", 0, None).unwrap();
    assert_eq!((h.total, h.items[0].outcome.as_str()), (2, "ended"));
    let p = s.profile("chris").unwrap().unwrap();
    assert_eq!((p.totals.matches, p.totals.unfinished), (2, 0));
    assert_eq!(p.totals.nemesis.as_deref(), Some("a debt collected by Consequence"), "two falls to it");
    // The portrait is one image across both matches.
    let lib = s.library("chris", 0, None, Some("portrait")).unwrap();
    assert_eq!(lib.total, 1);
    assert_eq!(lib.items[0].matches, vec!["m1", "m2"]);
    assert_eq!(p.demigods[0].matches, 2);
}

#[test]
fn champions_and_bots() {
    let s = ProfileStore::open_in_memory().unwrap();
    let mut m = ended("m3", 5);
    let mut sum = summary(Some(1));
    sum["champions"] = json!([{ "demigod": "dg:1", "name": "Benito", "seat": 1 }]);
    m.summary = Some(sum);
    m.seats.push(SeatIn { seat: 2, display_name: "Bot Brix".into(), is_bot: true });
    s.record_match(&m).unwrap();
    let b = s.profile("benito").unwrap().unwrap();
    assert_eq!(b.totals.champion, 1);
    assert!(b.description.auto.contains("Champion 1 time."), "{}", b.description.auto);
    assert!(s.history("benito", 0, None).unwrap().items[0].champion);
    assert!(s.profile("Bot Brix").unwrap().is_none(), "Theatre bots are skipped by default");
    s.record_match_with(&m, true).unwrap();
    assert!(s.profile("Bot Brix").unwrap().is_some());
}

#[test]
fn paging() {
    let s = ProfileStore::open_in_memory().unwrap();
    for i in 0..5 {
        s.record_match(&ended(&format!("m{i}"), 1_000 + i)).unwrap();
    }
    let p0 = s.history("chris", 0, Some(2)).unwrap();
    assert_eq!((p0.total, p0.items.len(), p0.next), (5, 2, Some(1)));
    assert_eq!(p0.items[0].match_id, "m4", "newest first");
    let p2 = s.history("chris", 2, Some(2)).unwrap();
    assert_eq!((p2.items.len(), p2.next), (1, None));
    assert_eq!(s.history("chris", 0, Some(0)).unwrap().per_page, 1);
    assert_eq!(s.history("chris", 0, Some(10_000)).unwrap().per_page, MAX_PER_PAGE);
    let lib = s.library("chris", 0, Some(2), None).unwrap();
    assert_eq!((lib.total, lib.next), (5, Some(1)));
    assert!(s.history("nobody", 0, None).unwrap().items.is_empty());
    assert!(matches!(s.history("  ", 0, None), Err(ProfileError::BadName)));
}

#[test]
fn descriptions_claims_and_images() {
    let s = ProfileStore::open_in_memory().unwrap();
    assert!(matches!(s.set_description("Chris", "hi", None), Err(ProfileError::NotFound)));
    s.upsert_profile("Chris").unwrap();
    let p = s.set_description("chris", "Builder of \u{0007}absurd hordes.", None).unwrap();
    assert_eq!(p.description.user, "Builder of absurd hordes.");
    let long = "y".repeat(5000);
    assert_eq!(s.set_description("chris", &long, None).unwrap().description.user.len(), MAX_DESCRIPTION);
    // Claim it: from now on only the secret edits it.
    assert!(matches!(s.claim("chris", "short", None), Err(ProfileError::Invalid(_))));
    s.claim("chris", "hunter22", None).unwrap();
    assert!(s.profile("chris").unwrap().unwrap().claimed);
    assert!(matches!(s.set_description("chris", "pwned", None), Err(ProfileError::Locked)));
    assert!(matches!(s.set_description("chris", "pwned", Some("nope")), Err(ProfileError::Locked)));
    assert_eq!(s.set_description("Chris", "mine", Some("hunter22")).unwrap().description.user, "mine");
    assert!(matches!(s.claim("chris", "another1", None), Err(ProfileError::Locked)));
    s.claim("chris", "another1", Some("hunter22")).unwrap();
    assert!(s.set_description("chris", "ok", Some("another1")).is_ok());
    // A lookalike profile is flagged on both.
    s.upsert_profile("Chr1s").unwrap();
    assert_eq!(s.profile("chris").unwrap().unwrap().lookalikes, vec!["Chr1s"]);
    // Images by hand.
    let img = NewImage {
        hash: "abc123".into(),
        kind: "item".into(),
        caption: "Banner of the Horde".into(),
        subject: Some("it:5".into()),
        subject_name: None,
        match_id: Some("m9".into()),
        at: Some(10),
    };
    s.add_image("chris", &img).unwrap();
    s.add_image("chris", &img).unwrap();
    let lib = s.library("chris", 0, None, Some("item")).unwrap();
    assert_eq!((lib.total, lib.items[0].matches.len()), (1, 1));
    assert!(matches!(s.add_image("chris", &NewImage { hash: "../etc".into(), ..img }), Err(ProfileError::Invalid(_))));
}

#[test]
fn profile_json_has_nothing_private() {
    let s = ProfileStore::open_in_memory().unwrap();
    s.record_match(&ended("m1", 1)).unwrap();
    s.claim("chris", "hunter22", None).unwrap();
    let j = serde_json::to_string(&s.profile("chris").unwrap().unwrap()).unwrap()
        + &serde_json::to_string(&s.history("chris", 0, None).unwrap()).unwrap();
    for bad in ["player_id", "claim_hash", "claim_salt", "resume", "hunter22", "token"] {
        assert!(!j.contains(bad), "{bad} leaked: {j}");
    }
}

/// One record line as the core writes it: compact, `type` first (serde's internally tagged enum). `json!` sorts keys,
/// so the tag is put in front by hand.
fn rec(mut v: Value) -> String {
    let ty = v.as_object_mut().unwrap().remove("type").unwrap();
    let rest = v.to_string();
    format!("{{\"type\":{ty},{}", &rest[1..])
}

/// An event line in the core's field order (`type, id, ctx, kind, actor, at, data`): `kind` comes before `data`.
fn ev(kind: &str, round: u32, data: Value) -> String {
    format!(
        "{{\"type\":\"event\",\"id\":1,\"ctx\":{{\"round\":{round},\"step\":null}},\"kind\":\"{kind}\",\"actor\":null,\"at\":1,\"data\":{data}}}"
    )
}

/// A miniature core recording: the lines the scan keeps, plus ones it must skip without parsing.
fn recording(with_end: bool) -> String {
    let mut lines = vec![
        rec(json!({ "type": "run_start", "run": "rec1", "plugin": "final_night_7", "at": 1000,
                "settings": { "beats_total": 30 },
                "meta": { "code": "DYL5GN", "match_id": "rec1",
                          "seats": [{ "display_name": "Chris", "is_bot": false, "player_id": "secret-uuid", "seat": 0 },
                                    { "display_name": "Benito", "is_bot": false, "player_id": "secret-uuid-2", "seat": 1 }] } })),
        rec(json!({ "type": "ruleset", "run": "rec1", "label": "0.5.0", "version": "0.1.0", "body": {} })),
        rec(json!({ "type": "snapshot", "run": "rec1", "state": { "huge": "x".repeat(10_000) } })),
        "{\"type\":\"frame\",\"not json at all".to_string(),
        // An event whose 400th byte falls inside a multi-byte character (a real recording had one).
        format!("{{\"type\":\"event\",\"id\":9,\"ctx\":{{\"x\":\"{}ãããã\"}},\"kind\":\"caption\"}}", "a".repeat(364)),
        ev("round.committed", 1, json!({ "public_state": { "beat": 1, "demigods": [] } })),
    ];
    for e in ended("rec1", 0).eliminations {
        lines.push(ev("elimination", 17, e));
    }
    lines.push(
        ev("round.committed", 21, json!({ "public_state": public_state() })),
    );
    if with_end {
        lines.push(
            rec(json!({ "type": "run_end", "run": "rec1", "at": 9000,
                    "summary": { "rounds": 21, "winners": [], "summary": summary(None) } })),
        );
    }
    lines.join("\n") + "\n"
}

#[test]
fn recordings_stream_into_inputs() {
    let text = recording(true);
    let (m, st) = read_recording_from(BufReader::new(text.as_bytes()), Some("rec1.jsonl".into())).unwrap();
    let m = m.unwrap();
    assert_eq!((m.match_id.as_str(), m.room_code.as_deref(), m.rules_version.as_deref()), ("rec1", Some("DYL5GN"), Some("0.5.0")));
    assert_eq!((m.started_at, m.ended_at, m.rounds, m.beats_total), (Some(1000), Some(9000), Some(21), Some(30)));
    assert_eq!(m.seats.len(), 2);
    assert_eq!(m.eliminations.len(), 3);
    assert_eq!(m.public_state.as_ref().unwrap()["beat"], 21, "the last committed state");
    assert!(m.ended());
    assert!(st.parsed < st.lines, "snapshots and frames are skipped");

    let s = ProfileStore::open_in_memory().unwrap();
    let rep = s.record_match(&m).unwrap();
    assert_eq!(rep.players, vec!["chris", "benito"]);
    assert_eq!(s.history("chris", 0, None).unwrap().items[0].room_code.as_deref(), Some("DYL5GN"));

    // Without run_end: unfinished, rounds from the last committed beat.
    let (u, _) = read_recording_from(BufReader::new(recording(false).as_bytes()), None).unwrap();
    let u = u.unwrap();
    assert!(!u.ended());
    assert_eq!(u.rounds, Some(21));
    // Not a recording at all.
    let (none, _) = read_recording_from(BufReader::new("{\"type\":\"frame\"}\n".as_bytes()), None).unwrap();
    assert!(none.is_none());
}

/// The checked-in example frame (docs/examples/match.ended.json) goes through the live path.
#[test]
fn live_path_from_the_example_frame() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/match.ended.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let frame: Value = serde_json::from_str(&text).unwrap();
    let champ_seat = frame["d"]["summary"]["champion_seat"].as_u64().unwrap() as u8;
    let seat_names: Vec<SeatIn> = frame["d"]["summary"]["standings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| Some(SeatIn { seat: s["seat"].as_u64()? as u8, display_name: s["name"].as_str()?.into(), is_bot: false }))
        .collect();
    let m = MatchInput::from_match_ended(seat_names, &frame, Some(1)).unwrap();
    let s = ProfileStore::open_in_memory().unwrap();
    let rep = s.record_match(&m).unwrap();
    assert!(!rep.players.is_empty());
    let champ = frame["d"]["summary"]["champions"][0]["name"].as_str().unwrap();
    let p = s.profile(champ).unwrap().unwrap();
    assert_eq!(p.totals.champion, 1, "seat {champ_seat} was champion");
}

#[cfg(feature = "http")]
mod http_routes {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::http::{router, HttpConfig};

    async fn call(app: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(b) => {
                req = req.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let code = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn routes_answer_and_gate() {
        let store = Arc::new(ProfileStore::open_in_memory().unwrap());
        store.record_match(&ended("m1", 1)).unwrap();
        let open = router(store.clone(), HttpConfig::default());
        let (c, p) = call(&open, "GET", "/api/profiles/CHRIS", None).await;
        assert_eq!((c, p["key"].as_str()), (StatusCode::OK, Some("chris")));
        let (c, h) = call(&open, "GET", "/api/profiles/chris/history?per_page=1", None).await;
        assert_eq!((c, h["total"].as_u64()), (StatusCode::OK, Some(1)));
        let (c, l) = call(&open, "GET", "/api/profiles/Chris/library?kind=portrait", None).await;
        assert_eq!((c, l["items"][0]["hash"].as_str()), (StatusCode::OK, Some("chrisp01")));
        let (c, _) = call(&open, "GET", "/api/profiles/nobody/history", None).await;
        assert_eq!(c, StatusCode::NOT_FOUND);
        let (c, _) = call(&open, "GET", "/api/profiles/%20/history", None).await;
        assert_eq!(c, StatusCode::BAD_REQUEST);
        let (c, p) = call(&open, "PUT", "/api/profiles/chris/description", Some(json!({ "text": "hello" }))).await;
        assert_eq!((c, p["description"]["user"].as_str()), (StatusCode::OK, Some("hello")));
        let (c, _) = call(&open, "POST", "/api/profiles/chris/claim", Some(json!({ "secret": "hunter22" }))).await;
        assert_eq!(c, StatusCode::NO_CONTENT);
        let (c, e) = call(&open, "PUT", "/api/profiles/chris/description", Some(json!({ "text": "x" }))).await;
        assert_eq!((c, e["error"].as_str()), (StatusCode::FORBIDDEN, Some("claimed")));

        let gated = router(store, HttpConfig { key: Some("k3y".into()) });
        let (c, _) = call(&gated, "GET", "/api/profiles/chris", None).await;
        assert_eq!(c, StatusCode::FORBIDDEN);
        let (c, _) = call(&gated, "GET", "/api/profiles/chris?key=nope", None).await;
        assert_eq!(c, StatusCode::FORBIDDEN);
        let (c, _) = call(&gated, "GET", "/api/profiles/chris?key=k3y", None).await;
        assert_eq!(c, StatusCode::OK);
    }
}
