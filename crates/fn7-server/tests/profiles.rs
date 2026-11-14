//! Profiles through the server (mock images only; no LLM, no paid API): a short recorded match through
//! `MediaPlugin::with_profiles` writes the history rows and library rows at match end, keyed by the core's match id
//! with its time-travel paths; `game.profile`/`game.history`/`game.library` answer from the store; a `game.media`
//! portrait of a player's demigod lands in their library once ready; the server's router serves the profile routes
//! beside the core's, with nothing private in them; a backfill of the same recording reproduces the row.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use fn7_media::{ImageGenerator, Media, MediaConfig, MockGenerator, Mode};
use fn7_plugin::driver::run_match;
use fn7_plugin::testkit::Failing;
use fn7_plugin::FinalNight7Plugin;
use fn7_profile::ProfileStore;
use fn7_server::media::MediaPlugin;
use fn7_server::profile::Profiles;
use fn7_server::record::{recorder, Recorded};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use theatre_plugin_api::*;
use theatre_server::axum::body::Body;
use theatre_server::axum::http::Request;
use tower::ServiceExt;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fn7-profiles-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn mock_media(dir: &std::path::Path) -> Media {
    let cfg = MediaConfig {
        mode: Mode::Mock,
        dir: dir.join("media"),
        ..Default::default()
    };
    let gen: Arc<dyn ImageGenerator> = Arc::new(MockGenerator::new());
    Media::start(cfg, Some(gen))
}

/// Two human seats (bots get no profile).
fn seats() -> Vec<SeatInfo> {
    ["Chris", "Benito"]
        .iter()
        .enumerate()
        .map(|(i, n)| SeatInfo { seat: i as u8, display_name: n.to_string(), is_bot: false })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_match_writes_history_and_library_and_the_routes_serve_them() {
    let dir = temp_dir("match");
    let media = mock_media(&dir);
    let rec = recorder(dir.join("rec"), theatre_trace::Level::Events).unwrap();
    let store = Arc::new(ProfileStore::open(&dir.join("fn7").join("profiles.sqlite")).unwrap());
    let profiles = Profiles::new(store.clone());
    profiles.attach_recordings(rec.dir.clone(), Some(rec.sink.clone()));
    let p = Recorded::new(
        MediaPlugin::new(FinalNight7Plugin::new().with_media_style(media.style().ruleset_json()), media.clone())
            .with_profiles(profiles.clone()),
        rec.run("pm1"),
    );
    // The profile queries are in the manifest the core publishes.
    let names: Vec<String> = p.queries().into_iter().map(|q| q.name).collect();
    for q in ["profile", "history", "library", "media"] {
        assert!(names.iter().any(|n| n == q), "{q} in {names:?}");
    }
    let none = |_: Seat, _: u32| None;
    let settings = json!({ "seed": 7, "beats_total": 3, "clusters": 24, "llm": false });
    let (state, summary) = run_match(&p, &seats(), &settings, &Failing, None, &none, |_, _| true).await.unwrap();
    assert!(media.wait_idle(Duration::from_secs(10)).await);
    rec.flush();

    // The history row: written in summarize, before match.ended goes out.
    let h = store.history("chris", 0, None).unwrap();
    assert_eq!(h.total, 1);
    let row = &h.items[0];
    assert_eq!(row.match_id, "pm1", "the core's match id");
    assert_eq!((row.outcome.as_str(), row.seat, row.display_name.as_str()), ("ended", 0, "Chris"));
    let field = summary.summary["standings"].as_array().unwrap().len() as u32;
    assert_eq!(row.field, Some(field));
    assert!(row.placement.is_some_and(|p| (1..=field).contains(&p)));
    assert_eq!(row.rules_version.as_deref(), Some(fn7_plugin::RULES_VERSION));
    assert!(!row.champions.is_empty());
    assert_eq!(row.demigods[0].name, "Chris's Left Hand");
    assert_eq!(row.display_name, "Chris");
    assert!(row.demigods[0].sheet_hash.is_some(), "visual sheet from the final public view");
    // Time travel: the admin routes take this match id; the recording is the core's file.
    assert_eq!(row.time_travel.match_id, "pm1");
    assert_eq!(row.time_travel.recording.as_deref(), Some("pm1.jsonl"));
    assert_eq!(row.time_travel.state, "/admin/matches/pm1/state?after_round={n}&view=public");
    assert_eq!(row.time_travel.fork, "/admin/fork");
    assert_eq!(store.history("benito", 0, None).unwrap().items[0].seat, 1);
    assert!(store.profile("Vex").unwrap().is_none(), "AI demigods get no profile");

    // The library: the payload images of Chris's demigod (portrait, diorama) and its statue, tagged with the match.
    let lib = store.library("chris", 0, Some(100), None).unwrap();
    let kinds: Vec<&str> = lib.items.iter().map(|i| i.kind.as_str()).collect();
    for k in ["portrait", "diorama", "statue"] {
        assert!(kinds.contains(&k), "{k} in {kinds:?}");
    }
    assert!(lib.items.iter().all(|i| i.matches == vec!["pm1".to_string()]));
    let pile = summary.summary["piles"].as_array().unwrap().iter().find(|x| x["seat"] == 0).unwrap();
    let statue = pile["statue_image"]["hash"].as_str().unwrap();
    assert!(lib.items.iter().any(|i| i.hash == statue));

    // The queries through the wrapper: the asking seat's own name by default, any name with `name`.
    let mine = p.query(&state, Some(0), "history", &json!({})).unwrap();
    assert_eq!(mine["items"][0]["match_id"], "pm1");
    assert_eq!(mine["items"][0]["display_name"], "Chris");
    let other = p.query(&state, None, "profile", &json!({ "name": "BENITO" })).unwrap();
    assert_eq!((other["key"].as_str(), other["totals"]["matches"].as_u64()), (Some("benito"), Some(1)));
    let portraits = p.query(&state, Some(1), "library", &json!({ "name": "chris", "kind": "portrait" })).unwrap();
    assert!(portraits["items"].as_array().unwrap().iter().all(|i| i["kind"] == "portrait"));
    assert!(p.query(&state, None, "history", &json!({})).is_err(), "no seat and no name");
    // Over HTTP (GET /api/rooms/{code}/query/history?name=chris&page=1) every arg is a string.
    let p1 = p.query(&state, None, "history", &json!({ "name": "chris", "page": "1", "per_page": "1" })).unwrap();
    assert_eq!((p1["page"].as_u64(), p1["per_page"].as_u64(), p1["items"].as_array().unwrap().len()), (Some(1), Some(1), 0));
    assert!(p.query(&state, None, "profile", &json!({ "name": "nobody" })).unwrap_err().starts_with("not_found"));

    // game.media for a player's demigod: into their library once ready (a fresh store, so only this path writes).
    let store2 = Arc::new(ProfileStore::open_in_memory().unwrap());
    let p2 = MediaPlugin::new(FinalNight7Plugin::new().with_media_style(media.style().ruleset_json()), media.clone())
        .with_profiles(Profiles::new(store2.clone()));
    let dg0 = state["seats"][0]["demigod"].as_u64().unwrap();
    let m = p2.query(&state, Some(0), "media", &json!({ "kind": "diorama", "ref": format!("dg:{dg0}") })).unwrap();
    let hash = m["image"]["hash"].as_str().unwrap().to_string();
    let mut found = None;
    for _ in 0..200 {
        found = store2.library("chris", 0, None, None).unwrap().items.into_iter().find(|i| i.hash == hash);
        if found.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let found = found.expect("the diorama was registered once ready");
    assert_eq!((found.kind.as_str(), found.caption.as_str()), ("diorama", "Chris's Left Hand, diorama"));
    assert_eq!(found.subject.as_deref(), Some(format!("dg:{dg0}").as_str()));
    // An AI demigod's portrait belongs to nobody.
    let ai = state["demigods"].as_array().unwrap().iter().find(|d| d["seat"].as_u64().unwrap() >= 2).unwrap();
    p2.query(&state, None, "media", &json!({ "kind": "portrait", "ref": format!("dg:{}", ai["id"]) })).unwrap();
    assert!(media.wait_idle(Duration::from_secs(5)).await);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(store2.list().unwrap().len(), 1, "only Chris has a profile");

    // The server's router: the profile routes beside the core's, JSON, nothing private.
    store.claim("Chris", "hunter22", None).unwrap();
    let cfg = theatre_server::Config::parse_from(["fn7-server"]);
    let llm = theatre_server::build_llm(&cfg).unwrap();
    let app = theatre_server::AppState::new(cfg, Arc::new(FinalNight7Plugin::new()), llm);
    let extra = fn7_media::http::router(media.clone())
        .merge(fn7_profile::http::router(store.clone(), fn7_profile::http::HttpConfig::default()));
    let router = theatre_server::router_with(app, extra).unwrap();
    let call = |method: &str, uri: &str, body: Option<Value>| {
        let router = router.clone();
        let mut req = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(b) => {
                req = req.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let req = req.body(body).unwrap();
        async move {
            let res = router.oneshot(req).await.unwrap();
            let status = res.status().as_u16();
            let ct = res.headers().get("content-type").map(|v| v.to_str().unwrap().to_string());
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            (status, ct, String::from_utf8(bytes.to_vec()).unwrap())
        }
    };
    assert_eq!(call("GET", "/healthz", None).await.0, 200);
    let mut all = String::new();
    for uri in ["/api/profiles/Chris", "/api/profiles/chris/history", "/api/profiles/CHRIS/library?kind=statue"] {
        let (s, ct, body) = call("GET", uri, None).await;
        assert_eq!((s, ct.as_deref()), (200, Some("application/json")), "{uri}: {body}");
        serde_json::from_str::<Value>(&body).unwrap();
        all += &body;
    }
    let h: Value = serde_json::from_str(&call("GET", "/api/profiles/chris/history", None).await.2).unwrap();
    assert_eq!(h["items"][0]["match_id"], "pm1");
    assert_eq!(h["items"][0]["time_travel"]["match_id"], "pm1");
    for bad in ["player_id", "claim_hash", "claim_salt", "resume", "token", "hunter22", "secret"] {
        assert!(!all.contains(bad), "{bad} leaked");
    }
    let (s, _, body) = call("PUT", "/api/profiles/chris/description", Some(json!({ "text": "x" }))).await;
    assert_eq!(s, 403, "{body}");
    let (s, _, body) = call("PUT", "/api/profiles/chris/description", Some(json!({ "text": "hi", "secret": "hunter22" }))).await;
    assert_eq!(s, 200, "{body}");
    assert!(!body.contains("hunter22"));
    assert_eq!(call("POST", "/api/profiles/chris/claim", Some(json!({ "secret": "another1" }))).await.0, 403, "claimed");
    assert_eq!(call("GET", "/api/profiles/nobody", None).await.0, 404);

    // A backfill of the same recording (now with run_end) reproduces the row.
    let fresh = ProfileStore::open_in_memory().unwrap();
    assert!(fn7_server::profile::should_backfill(&fresh, |_| None));
    assert!(!fn7_server::profile::should_backfill(&store, |_| None));
    assert!(fn7_server::profile::should_backfill(&store, |k| (k == "FN7_PROFILE_BACKFILL").then(|| "1".into())));
    let r = fn7_server::profile::backfill(&fresh, &rec.dir).unwrap();
    assert_eq!((r.files, r.ended, r.errors), (1, 1, 0));
    let b = &fresh.history("chris", 0, None).unwrap().items[0];
    assert_eq!((b.match_id.as_str(), b.placement, b.field, b.champion), ("pm1", row.placement, row.field, row.champion));
    assert_eq!(b.eliminations, row.eliminations);
    assert_eq!(fn7_server::profile::backfill(&fresh, &rec.dir).unwrap().known, 1, "skipped unopened");
}

/// Recording off: the row still lands (a local id, no recording), built from the hook and the state's seats.
#[tokio::test(flavor = "multi_thread")]
async fn without_a_recording_the_row_comes_from_the_hook() {
    let store = Arc::new(ProfileStore::open_in_memory().unwrap());
    let p = MediaPlugin::new(FinalNight7Plugin::new(), Media::off()).with_profiles(Profiles::new(store.clone()));
    let none = |_: Seat, _: u32| None;
    let settings = json!({ "seed": 4, "beats_total": 2, "clusters": 24, "llm": false });
    run_match(&p, &seats(), &settings, &Failing, None, &none, |_, _| true).await.unwrap();
    let h = store.history("Benito", 0, None).unwrap();
    assert_eq!(h.total, 1);
    assert!(h.items[0].match_id.starts_with("local-"));
    assert_eq!(h.items[0].time_travel.recording, None);
    assert_eq!(h.items[0].outcome, "ended");
}
