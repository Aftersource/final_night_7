use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;

fn temp_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "fn7-media-test-{}-{}-{}",
        std::process::id(),
        theatre_trace::now_ms(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cfg(dir: &std::path::Path, budget: f64, match_budget: f64) -> MediaConfig {
    MediaConfig {
        mode: Mode::Mock,
        dir: dir.to_path_buf(),
        budget_usd: budget,
        match_budget_usd: match_budget,
        ..Default::default()
    }
}

fn mock(price: f64) -> Option<Arc<dyn ImageGenerator>> {
    Some(Arc::new(MockGenerator {
        price_usd: price,
        ..Default::default()
    }))
}

fn p(kind: ImageKind, s: &str) -> ImagePrompt {
    ImagePrompt::from_subject(kind, s).unwrap()
}

#[tokio::test]
async fn disabled_by_default() {
    let env = |vars: &[(&str, &str)]| {
        let m: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        MediaConfig::from_lookup(move |k| m.get(k).cloned())
    };
    let c = env(&[("OPENAI_API_KEY", "sk-test")]);
    assert_eq!(c.mode, Mode::Off);
    assert_eq!(c.budget_usd, 0.0);
    assert!(c.generator().unwrap().is_none());
    assert_eq!(
        env(&[("FN7_IMAGES", "openai")]).mode,
        Mode::Off,
        "no key, no images"
    );
    assert_eq!(env(&[("FN7_IMAGES", "dalle")]).mode, Mode::Off);

    let media = Media::start(c.clone(), c.generator().unwrap());
    let r = media.request(&p(ImageKind::Portrait, "the demigod Rook"), Some("run-1"));
    assert_eq!((r.status, r.url.clone()), ("off", None));
    assert_eq!(media.manifest(0)["enabled"], false);
    let mut payload =
        json!({ "piles": [ { "name": "Rook", "statue_prompt": "An amiibo figure of Rook" } ] });
    let before = payload.clone();
    assert_eq!(media.annotate(&mut payload, None), 0);
    assert_eq!(payload, before, "off never touches payloads");

    // openai with a key but the default budget of 0: built, but refuses everything before any request is made.
    let dir = temp_dir();
    let mut c = env(&[
        ("FN7_IMAGES", "openai"),
        ("OPENAI_API_KEY", "sk-test"),
        ("FN7_IMAGE_BASE_URL", "http://127.0.0.1:9"),
    ]);
    c.dir = dir.clone();
    assert_eq!(c.mode, Mode::OpenAi);
    let media = Media::start(c.clone(), c.generator().unwrap());
    let r = media.request(&p(ImageKind::Statue, "Rook on a pile"), Some("run-1"));
    assert_eq!(r.status, "off");
    let e = media.entry(&r.hash).unwrap();
    assert_eq!(e.status, Status::Refused);
    assert!(e.reason.unwrap().contains("budget"));
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}

#[tokio::test]
async fn generates_in_background_then_hits_the_cache() {
    let dir = temp_dir();
    let media = Media::start(cfg(&dir, 0.0, 0.0), mock(0.0));
    let prompt = p(
        ImageKind::Item,
        "the Deduplicator, a box full of other boxes",
    );
    let r = media.request(&prompt, Some("m1"));
    assert_eq!(r.status, "pending");
    assert_eq!(r.hash, media.key_for(&prompt));
    assert!(cache::valid_key(&r.hash));
    assert!(media.wait_idle(Duration::from_secs(5)).await);
    let r2 = media.request(&prompt, Some("m1"));
    assert_eq!(r2.status, "ready");
    assert_eq!(
        r2.url.as_deref(),
        Some(format!("/media/{}.svg", r.hash).as_str())
    );
    let side = media.cache().lookup(&r.hash).unwrap();
    assert_eq!(
        (side.kind, side.provider.as_str(), side.ext.as_str()),
        (ImageKind::Item, "mock", "svg")
    );
    assert!(side.prompt.contains("Deduplicator"));
    let svg = std::fs::read_to_string(dir.join(format!("{}.svg", r.hash))).unwrap();
    assert!(svg.starts_with("<svg") && svg.contains(&r.hash[..12]));

    // A new process on the same directory: ready at once, nothing generated.
    let again = Media::start(
        cfg(&dir, 0.0, 0.0),
        Some(Arc::new(MockGenerator {
            fail: true,
            ..Default::default()
        })),
    );
    let r3 = again.request(&prompt, Some("m2"));
    assert_eq!(r3.status, "ready");
    assert_eq!(r3.hash, r.hash);
}

#[tokio::test]
async fn budgets_refuse_beyond_the_limit() {
    let dir = temp_dir();
    // $0.04 an image, $0.10 per match, $0.15 in all.
    let media = Media::start(cfg(&dir, 0.15, 0.10), mock(0.04));
    let s = |i: u32| p(ImageKind::Portrait, &format!("the demigod number {i}"));
    assert_eq!(media.request(&s(1), Some("a")).status, "pending");
    assert_eq!(media.request(&s(2), Some("a")).status, "pending");
    let r = media.request(&s(3), Some("a"));
    assert_eq!(r.status, "off");
    assert!(media
        .entry(&r.hash)
        .unwrap()
        .reason
        .unwrap()
        .contains("match image budget"));
    // Another match: its own match budget, but the global one stops it after one more.
    assert_eq!(media.request(&s(4), Some("b")).status, "pending");
    let r = media.request(&s(5), Some("b"));
    assert_eq!(r.status, "off");
    assert!(media
        .entry(&r.hash)
        .unwrap()
        .reason
        .unwrap()
        .starts_with("image budget"));
    assert!(media.wait_idle(Duration::from_secs(5)).await);
    let (total, a) = media.spent(Some("a"));
    assert!(
        (total - 0.12).abs() < 1e-9 && (a - 0.08).abs() < 1e-9,
        "{total} {a}"
    );
    // What was spent persists in the sidecars: a restart with the same budget has $0.03 left, not $0.15.
    let restarted = Media::start(cfg(&dir, 0.15, 0.10), mock(0.04));
    assert!((restarted.spent(None).0 - 0.12).abs() < 1e-9);
    assert_eq!(restarted.request(&s(6), Some("c")).status, "off");
}

#[tokio::test]
async fn event_images_are_capped_per_match_and_failures_refund() {
    let dir = temp_dir();
    let mut c = cfg(&dir, 1.0, 1.0);
    c.max_events_per_match = 2;
    let media = Media::start(c, mock(0.0));
    let e = |i: u32| p(ImageKind::Event, &format!("pandemonium number {i}"));
    assert_eq!(media.request(&e(1), Some("a")).status, "pending");
    assert_eq!(media.request(&e(2), Some("a")).status, "pending");
    assert_eq!(media.request(&e(3), Some("a")).status, "off");
    assert_eq!(
        media.request(&e(3), Some("b")).status,
        "pending",
        "a refusal is per match"
    );

    let failing = Media::start(
        cfg(&temp_dir(), 1.0, 1.0),
        Some(Arc::new(MockGenerator {
            fail: true,
            price_usd: 0.5,
            ..Default::default()
        })),
    );
    let r = failing.request(&e(1), Some("a"));
    assert_eq!(r.status, "pending");
    assert!(failing.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(failing.entry(&r.hash).unwrap().status, Status::Failed);
    assert_eq!(failing.request(&e(1), Some("a")).status, "off");
    assert_eq!(failing.spent(Some("a")), (0.0, 0.0));
}

#[tokio::test]
async fn manifest_polls_by_cursor() {
    let media = Media::start(cfg(&temp_dir(), 0.0, 0.0), mock(0.0));
    let a = media.request(&p(ImageKind::Portrait, "the demigod Moth"), None);
    let m0 = media.manifest(0);
    assert_eq!(m0["enabled"], true);
    assert_eq!(m0["mode"], "mock");
    assert_eq!(m0["items"].as_array().unwrap().len(), 1);
    assert_eq!(m0["items"][0]["hash"], a.hash.as_str());
    assert!(media.wait_idle(Duration::from_secs(5)).await);
    let cursor = m0["cursor"].as_i64().unwrap();
    let m1 = media.manifest(cursor);
    let items = m1["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "the change to ready shows after the cursor");
    assert_eq!(items[0]["status"], "ready");
    assert_eq!(items[0]["url"], format!("/media/{}.svg", a.hash));
    let m2 = media.manifest(m1["cursor"].as_i64().unwrap());
    assert!(m2["items"].as_array().unwrap().is_empty());
    assert_eq!(m2["cursor"], m1["cursor"]);
}

#[tokio::test]
async fn annotates_payloads() {
    let media = Media::start(cfg(&temp_dir(), 0.0, 0.0), mock(0.0));
    // Before any image_prompt: interim omens and demigod portraits.
    let mut public = json!({
        "omens": [ { "god": "Time", "god_ref": "gd:0", "speech": "\"Granted, for the usual price.\"" } ],
        "demigods": [ { "ref": "dg:0", "name": "Rook" }, { "ref": "dg:1", "name": "Quill" } ],
    });
    assert_eq!(media.annotate(&mut public, Some("m")), 3);
    assert_eq!(public["omens"][0]["image"]["kind"], "event");
    assert_eq!(public["demigods"][1]["image"]["kind"], "portrait");
    assert!(!public["omens"][0]["image"]["prompt"]
        .as_str()
        .unwrap()
        .contains("gd:0"));
    let mut summary = json!({ "piles": [ { "demigod": "dg:0", "name": "Rook", "statue_prompt": "An amiibo-style figure of Rook (dg:0)" } ] });
    assert_eq!(media.annotate(&mut summary, Some("m")), 1);
    let img = &summary["piles"][0]["statue_image"];
    assert_eq!(img["kind"], "statue");
    assert_eq!(img["status"], "pending");
    assert!(img["url"].is_null());
    assert!(!img["prompt"].as_str().unwrap().contains("dg:0"));

    // The plugin's own prompts: kinds from image_kind or inferred; interim rules stop.
    let mut p2 = json!({
        "events": [ { "row": "lg:4", "image_prompt": "a tidal wave of bees over Lagos" } ],
        "items": [ { "ref": "it:3", "name": "sand shield", "image_prompt": "a shield of living sand" } ],
        "x": { "image_prompt": "Moth, grinning", "image_kind": "portrait" },
        "omens": [ { "god": "Chance", "speech": "Heads." } ],
    });
    assert_eq!(media.annotate(&mut p2, Some("m")), 3);
    assert_eq!(p2["events"][0]["image"]["kind"], "event");
    assert_eq!(p2["items"][0]["image"]["kind"], "item");
    assert_eq!(p2["x"]["image"]["kind"], "portrait");
    assert!(p2["omens"][0].get("image").is_none());
    let mut again = json!({ "omens": [ { "god": "Time", "speech": "Later." } ] });
    assert_eq!(media.annotate(&mut again, None), 0);

    // The same prompt twice is one image.
    let before = media.manifest(0)["items"].as_array().unwrap().len();
    let mut dup = json!({ "a": { "image_prompt": "a tidal wave of bees over Lagos" } });
    media.annotate(&mut dup, Some("m"));
    assert_eq!(dup["a"]["image"]["hash"], p2["events"][0]["image"]["hash"]);
    assert_eq!(media.manifest(0)["items"].as_array().unwrap().len(), before);
}

#[tokio::test]
async fn routes_serve_the_manifest_and_cached_files() {
    let dir = temp_dir();
    let media = Media::start(cfg(&dir, 0.0, 0.0), mock(0.0));
    let r = media.request(
        &p(ImageKind::Statue, "Rook on a pile of everything"),
        Some("m"),
    );
    assert!(media.wait_idle(Duration::from_secs(5)).await);
    let app = http::router(media.clone());
    let get = |uri: String| {
        let app = app.clone();
        async move {
            let res = app
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = res.status().as_u16();
            let ct = res
                .headers()
                .get("content-type")
                .map(|v| v.to_str().unwrap().to_string());
            let cors = res.headers().get("access-control-allow-origin").is_some();
            let body = res.into_body().collect().await.unwrap().to_bytes();
            (status, ct, cors, body)
        }
    };
    let (s, ct, cors, body) = get(format!("/media/{}.svg", r.hash)).await;
    assert_eq!((s, ct.as_deref(), cors), (200, Some("image/svg+xml"), true));
    assert_eq!(
        &body[..],
        &std::fs::read(dir.join(format!("{}.svg", r.hash))).unwrap()[..]
    );
    let (s, _, _, body) = get("/media/index?since=0".into()).await;
    assert_eq!(s, 200);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["items"][0]["status"], "ready");
    for bad in [
        format!("/media/{}.json", r.hash),
        "/media/..%2Fsecret.png".into(),
        format!("/media/{}.png", "0".repeat(64)),
    ] {
        assert_eq!(get(bad).await.0, 404);
    }
}
