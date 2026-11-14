//! Images through the server (mock generator only; nothing here ever calls a paid API): a recorded match (core JSONL
//! format) through `MediaPlugin` gets `statue_image` objects and `media_*` events in its run; the engine's `router_with` (what `serve_with` serves) serves the Theatre's
//! routes and the media routes side by side.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use theatre_server::axum::body::Body;
use theatre_server::axum::http::Request;
use clap::Parser;
use fn7_media::{ImageGenerator, Media, MediaConfig, MockGenerator, Mode};
use fn7_plugin::driver::run_match;
use fn7_plugin::testkit::Failing;
use fn7_plugin::FinalNight7Plugin;
use fn7_server::media::MediaPlugin;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use theatre_plugin_api::*;
use fn7_server::record::{recorder, Recorded};
use tower::ServiceExt;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fn7-media-{name}-{}", uuid::Uuid::new_v4()));
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

fn seats() -> Vec<SeatInfo> {
    ["Ash", "Brix"]
        .iter()
        .enumerate()
        .map(|(i, n)| SeatInfo {
            seat: i as u8,
            display_name: n.to_string(),
            is_bot: true,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_recorded_match_gets_statue_images_and_media_events() {
    let dir = temp_dir("match");
    let media = mock_media(&dir);
    let rec = recorder(dir.join("rec"), theatre_trace::Level::Events).unwrap();
    let p = Recorded::new(
        MediaPlugin::new(FinalNight7Plugin::new(), media.clone()),
        rec.run("m1"),
    );
    let none = |_: Seat, _: u32| None;
    let settings = json!({ "seed": 3, "beats_total": 3, "clusters": 24, "llm": false });
    let (state, summary) = run_match(&p, &seats(), &settings, &Failing, None, &none, |_, _| true)
        .await
        .unwrap();
    assert!(media.wait_idle(Duration::from_secs(10)).await);
    rec.flush();

    let piles = summary.summary["piles"].as_array().unwrap();
    assert!(!piles.is_empty());
    for pile in piles {
        let img = &pile["statue_image"];
        assert_eq!(img["kind"], "statue", "{pile}");
        assert!(matches!(img["status"].as_str(), Some("pending" | "ready")));
        let hash = img["hash"].as_str().unwrap();
        let e = media.entry(hash).unwrap();
        assert_eq!(
            e.url.as_deref(),
            Some(format!("/media/{hash}.svg").as_str())
        );
        assert!(dir.join("media").join(format!("{hash}.svg")).is_file());
        assert!(!img["prompt"].as_str().unwrap().contains("dg:"));
    }
    let events: Vec<Value> = fn7_server::replay::read_records(&dir.join("rec").join("m1.jsonl"))
        .unwrap()
        .into_iter()
        .filter(|l| l["type"] == "event" && l["ctx"]["run"] == "m1")
        .collect();
    let n = |kind: &str| events.iter().filter(|e| e["kind"] == kind).count();
    assert!(n("media_request") >= piles.len());
    assert_eq!(
        n("media_request"),
        n("media_ready"),
        "every request finished and was recorded in the run"
    );
    let path = events.iter().find(|e| e["kind"] == "media_ready").unwrap()["data"]["path"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(path.ends_with(".svg"));

    // The state is untouched by images: the same match without them ends in the same state.
    let plain = Recorded::new(FinalNight7Plugin::new(), rec.run("m2"));
    let (state2, _) = run_match(
        &plain,
        &seats(),
        &settings,
        &Failing,
        None,
        &none,
        |_, _| true,
    )
    .await
    .unwrap();
    assert_eq!(state, state2);
}

#[tokio::test(flavor = "multi_thread")]
async fn images_off_leaves_payloads_alone() {
    let p = MediaPlugin::new(FinalNight7Plugin::new(), Media::off());
    let none = |_: Seat, _: u32| None;
    let settings = json!({ "seed": 3, "beats_total": 2, "clusters": 24, "llm": false });
    let (_, s) = run_match(&p, &seats(), &settings, &Failing, None, &none, |_, _| true)
        .await
        .unwrap();
    let (_, plain) = run_match(
        &FinalNight7Plugin::new(),
        &seats(),
        &settings,
        &Failing,
        None,
        &none,
        |_, _| true,
    )
    .await
    .unwrap();
    assert!(s.summary["piles"][0].get("statue_image").is_none());
    assert_eq!(s.summary["piles"], plain.summary["piles"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_router_serves_media_beside_the_theatre_routes() {
    let dir = temp_dir("route");
    let media = mock_media(&dir);
    let r = media.request(
        &fn7_media::ImagePrompt::from_subject(fn7_media::ImageKind::Portrait, "the demigod Rook")
            .unwrap(),
        None,
    );
    assert!(media.wait_idle(Duration::from_secs(5)).await);

    let cfg = theatre_server::Config::parse_from(["fn7-server"]);
    let llm = theatre_server::build_llm(&cfg).unwrap();
    let app = theatre_server::AppState::new(cfg, Arc::new(FinalNight7Plugin::new()), llm);
    // The engine's route extension: the core's router with the /media routes merged in (what serve_with serves).
    let router = theatre_server::router_with(app.clone(), fn7_media::http::router(media.clone())).unwrap();
    // Overlapping the core's reserved paths is an error, not a panic.
    let clash = theatre_server::axum::Router::new()
        .route("/healthz", theatre_server::axum::routing::get(|| async { "x" }));
    assert!(theatre_server::router_with(app, clash).is_err());
    let get = |uri: String| {
        let router = router.clone();
        async move {
            let res = router
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = res.status().as_u16();
            let ct = res
                .headers()
                .get("content-type")
                .map(|v| v.to_str().unwrap().to_string());
            (
                status,
                ct,
                res.into_body().collect().await.unwrap().to_bytes(),
            )
        }
    };
    assert_eq!(get("/healthz".into()).await.0, 200);
    let (s, ct, body) = get(format!("/media/{}.svg", r.hash)).await;
    assert_eq!((s, ct.as_deref()), (200, Some("image/svg+xml")));
    assert!(body.starts_with(b"<svg"));
    let (s, _, body) = get("/media/index?since=0".into()).await;
    assert_eq!(s, 200);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"][0]["hash"], r.hash.as_str());
    assert_eq!(v["items"][0]["status"], "ready");
}

fn limited_media(dir: &std::path::Path, rate: u32, price: f64, match_budget: f64) -> Media {
    let cfg = MediaConfig {
        mode: Mode::Mock,
        dir: dir.join("media"),
        rate_per_min: rate,
        budget_usd: 10.0,
        match_budget_usd: match_budget,
        ..Default::default()
    };
    let gen: Arc<dyn ImageGenerator> = Arc::new(MockGenerator { price_usd: price, ..Default::default() });
    Media::start(cfg, Some(gen))
}

/// A mock-mode match through the server wrapper: the style in the ruleset, moment cards, a full storyboard with image
/// objects that reuse the moment cards' and portraits' images, and `game.media` (images by reference) answering with
/// image objects, idempotently, within visibility.
#[tokio::test(flavor = "multi_thread")]
async fn storyboard_and_media_queries_through_the_server_wrapper() {
    let dir = temp_dir("story");
    let media = mock_media(&dir);
    let rec = recorder(dir.join("rec"), theatre_trace::Level::Events).unwrap();
    let p = Recorded::new(
        MediaPlugin::new(FinalNight7Plugin::new().with_media_style(media.style().ruleset_json()), media.clone()),
        rec.run("s1"),
    );
    let none = |_: Seat, _: u32| None;
    let settings = json!({ "seed": 5, "beats_total": 6, "clusters": 40 });
    let mut cards: std::collections::BTreeMap<String, String> = Default::default();
    let mut unselected = 0;
    let (state, summary) = run_match(&p, &seats(), &settings, &fn7_plugin::testkit::Scripted::default(), None, &none, |rep, _| {
        for e in rep.outcome.public["events"].as_array().into_iter().flatten() {
            let sel = media.style().event_selection.selects(e["kind"].as_str().unwrap(), e["magnitude"].as_u64().unwrap() as u8);
            assert_eq!(e.get("image").is_some(), sel, "moment cards follow the selection rules: {e}");
            match e.get("image") {
                Some(img) => {
                    cards.insert(e["row"].as_str().unwrap().into(), img["hash"].as_str().unwrap().into());
                }
                None => unselected += 1,
            }
        }
        true
    })
    .await
    .unwrap();
    assert!(media.wait_idle(Duration::from_secs(10)).await);
    rec.flush();

    // The style is recorded in the match's ruleset.
    let records = fn7_server::replay::read_records(&dir.join("rec").join("s1.jsonl")).unwrap();
    let rs = records.iter().find(|l| l["type"] == "ruleset" && l["body"]["media_style"].is_object()).expect("ruleset with media_style");
    assert_eq!(rs["body"]["media_style"]["version"], "2");
    assert_eq!(rs["body"]["media_style"]["hash"], media.style().hash.as_str());

    // The storyboard: every shot has an image object; moments reuse their moment card's image; the champion's is its
    // portrait.
    let sb = summary.summary["storyboard"].as_array().unwrap();
    assert!(sb.len() >= 3);
    let pv = p.public_view(&state);
    for s in sb {
        let img = &s["image"];
        assert!(matches!(img["status"].as_str(), Some("pending" | "ready")), "{s}");
        assert_eq!(img["kind"], s["image_kind"]);
        if let Some(h) = s["row"].as_str().and_then(|r| cards.get(r)) {
            assert_eq!(img["hash"].as_str().unwrap(), h, "a moment card's picture is reused");
        }
    }
    let champ = summary.summary["champions"][0]["demigod"].as_str().unwrap();
    let d = pv["demigods"].as_array().unwrap().iter().find(|d| d["ref"] == champ).unwrap();
    assert_eq!(sb.last().unwrap()["image"]["hash"], d["portrait_image"]["hash"]);
    assert!(d["visual"]["sheet_hash"].is_string());

    // game.media: an image object at once, the same image as the payloads', idempotent.
    let m = p.query(&state, Some(0), "media", &json!({ "kind": "portrait", "ref": champ })).unwrap();
    assert_eq!(m["image"]["hash"], d["portrait_image"]["hash"]);
    assert!(m.get("image_prompt").is_none() && m["visual"] == d["visual"]);
    assert_eq!(p.query(&state, None, "media", &json!({ "kind": "portrait", "ref": champ })).unwrap()["image"]["hash"], m["image"]["hash"]);
    let w = fn7_plugin::world::load(&state).unwrap();
    let region = w.places.iter().find(|x| x.kind == fn7_model::PlaceKind::District).unwrap();
    let place = p.query(&state, Some(1), "media", &json!({ "kind": "place", "ref": format!("pl:{}", region.id.0) })).unwrap();
    assert_eq!(place["image"]["kind"], "place");
    assert!(matches!(place["image"]["status"].as_str(), Some("pending" | "ready")));
    let private = w.ledger.rows.iter().rev().find(|r| !fn7_plugin::payload::is_public_event(&w, r)).unwrap();
    assert!(p.query(&state, None, "media", &json!({ "kind": "event", "ref": format!("lg:{}", private.id.0) })).is_err(),
            "HTTP gets public refs only");
    assert!(media.wait_idle(Duration::from_secs(10)).await);
    let again = p.query(&state, Some(1), "media", &json!({ "kind": "place", "ref": format!("pl:{}", region.id.0) })).unwrap();
    assert_eq!((again["image"]["hash"].clone(), again["image"]["status"].as_str()), (place["image"]["hash"].clone(), Some("ready")));

    // Replay compares summaries without the server's image objects: the bare plugin's summary matches.
    let (_, plain) = run_match(&FinalNight7Plugin::new(), &seats(), &settings, &fn7_plugin::testkit::Scripted::default(), None, &none, |_, _| true)
        .await
        .unwrap();
    assert_eq!(fn7_server::replay::strip_media(&summary.summary), fn7_server::replay::strip_media(&plain.summary));
    assert!(plain.summary["storyboard"][0]["image"].is_null());
}

/// `game.media` through the wrapper: the per-seat rate limit (an error), budget refusal (an `off` image with the
/// reason), and repeats free.
#[tokio::test(flavor = "multi_thread")]
async fn media_queries_are_rate_limited_and_budgeted() {
    let none = |_: Seat, _: u32| None;
    let settings = json!({ "seed": 5, "beats_total": 2, "clusters": 24, "llm": false });
    let (state, _) = run_match(&FinalNight7Plugin::new(), &seats(), &settings, &Failing, None, &none, |_, _| true).await.unwrap();
    let w = fn7_plugin::world::load(&state).unwrap();
    let places: Vec<String> = w.places.iter().take(6).map(|x| format!("pl:{}", x.id.0)).collect();
    let ask = |p: &MediaPlugin<FinalNight7Plugin>, seat: Option<Seat>, r: &str| {
        p.query(&state, seat, "media", &json!({ "kind": "place", "ref": r }))
    };
    // Rate: 2 new images per minute per seat.
    let p = MediaPlugin::new(FinalNight7Plugin::new(), limited_media(&temp_dir("rate"), 2, 0.0, 10.0));
    assert!(ask(&p, Some(0), &places[0]).is_ok());
    assert!(ask(&p, Some(0), &places[1]).is_ok());
    assert!(ask(&p, Some(0), &places[0]).is_ok(), "a repeat is free");
    assert!(ask(&p, Some(0), &places[2]).unwrap_err().contains("rate limited"));
    assert!(ask(&p, Some(1), &places[2]).is_ok(), "per seat");
    assert!(ask(&p, None, &places[3]).is_ok(), "the public asker has its own bucket");
    // Budget: $0.05 an image, $0.05 per match.
    let p = MediaPlugin::new(FinalNight7Plugin::new(), limited_media(&temp_dir("budget"), 0, 0.05, 0.05));
    assert_eq!(ask(&p, Some(0), &places[0]).unwrap()["image"]["status"], "pending");
    let refused = ask(&p, Some(0), &places[1]).unwrap();
    assert_eq!(refused["image"]["status"], "off");
    assert!(refused["reason"].as_str().unwrap().contains("budget"), "{refused}");
    // Images off: an off image, never an error.
    let off = MediaPlugin::new(FinalNight7Plugin::new(), Media::off());
    let v = ask(&off, None, &places[0]).unwrap();
    assert_eq!((v["image"]["status"].as_str(), v["reason"].as_str()), (Some("off"), Some("images are off")));
}

/// Prewarm queues every built-in AI demigod's portrait and diorama (plus extra names) once; a later process finds them
/// cached; matches with different seeds then get those exact images, ready at once.
#[tokio::test(flavor = "multi_thread")]
async fn prewarm_caches_the_roster_and_every_match_reuses_it() {
    use fn7_server::media::{prewarm, prewarm_names, Prewarm};
    let roster = fn7_plugin::phases::AI_NAMES;
    let names = prewarm_names(|k| (k == "FN7_MEDIA_PREWARM_NAMES").then(|| "Chris, benito ,VEX,".to_string()));
    assert_eq!(names.len(), roster.len() + 2, "the roster plus Chris and Benito (Vex is already in it): {names:?}");
    assert!(prewarm_names(|k| (k == "FN7_MEDIA_PREWARM").then(|| "off".to_string())).is_empty());
    let dir = temp_dir("prewarm");
    let media = mock_media(&dir);
    // Per name: its portrait and diorama, and both of its hands' (the player's two demigods).
    assert_eq!(prewarm(&media, &names), Prewarm { queued: 6 * names.len(), cached: 0, refused: 0 });
    assert!(media.wait_idle(Duration::from_secs(10)).await);
    // Images off: nothing.
    assert_eq!(prewarm(&Media::off(), &names), Prewarm::default());
    // The next server process on the same cache: all cached, nothing queued.
    let media2 = mock_media(&dir);
    assert_eq!(prewarm(&media2, &names), Prewarm { queued: 0, cached: 6 * names.len(), refused: 0 });
    let mut hashes: Vec<std::collections::BTreeMap<String, (String, String)>> = vec![];
    for seed in [1u64, 777] {
        let p = MediaPlugin::new(FinalNight7Plugin::new(), media2.clone());
        let settings = p.validate_settings(&json!({ "seed": seed, "players": 4, "beats_total": 6, "clusters": 40 })).unwrap();
        let s = p.on_match_start(&seats(), &settings, &Failing).await.unwrap();
        let v = p.public_view(&s);
        // The seat view's hands (the player's two demigods) carry their own images too.
        let me = p.seat_view(&s, 0);
        let hands = me["hands"].as_array().unwrap();
        assert_eq!(hands.len(), 2);
        for h in hands {
            assert_eq!(h["portrait_image"]["kind"], "portrait", "{h}");
            assert_eq!(h["diorama_image"]["kind"], "diorama", "{h}");
            assert!(h["visual"]["sheet_hash"].is_string());
        }
        let mut m = std::collections::BTreeMap::new();
        for d in v["demigods"].as_array().unwrap() {
            let name = d["name"].as_str().unwrap().to_string();
            if roster.contains(&name.as_str()) {
                assert_eq!(d["portrait_image"]["status"], "ready", "{name}'s portrait is prewarmed");
                assert_eq!(d["diorama_image"]["status"], "ready", "{name}'s diorama is prewarmed");
            }
            m.insert(name, (d["portrait_image"]["hash"].as_str().unwrap().into(), d["diorama_image"]["hash"].as_str().unwrap().into()));
        }
        hashes.push(m);
    }
    assert_eq!(hashes[0].len(), 4);
    assert_eq!(hashes[0], hashes[1], "the same demigods are the same images in matches with different seeds");
}
