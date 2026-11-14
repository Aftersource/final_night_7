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
