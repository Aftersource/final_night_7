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
        MediaConfig::from_lookup(move |k| m.get(k).cloned()).unwrap()
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

fn styled_cfg(dir: &std::path::Path, style: MediaStyle) -> MediaConfig {
    MediaConfig {
        max_events_per_match: style.event_selection.max_per_match,
        style,
        ..cfg(dir, 1.0, 1.0)
    }
}

fn alt_style() -> MediaStyle {
    let mut v: Value = serde_json::from_str(style::BUILTIN_STYLE_JSON).unwrap();
    v["version"] = json!("2-noir");
    v["house_style"] = json!("hand-inked noir comic panels, heavy shadows");
    v["kinds"]["portrait"] = json!({ "framing": "tight head shot, low key light", "size": "1024x1536" });
    v["avoid"] = json!(["text", "blood"]);
    v["event_selection"] = json!({ "min_magnitude": 5, "always": ["riot"], "max_per_match": 3 });
    MediaStyle::from_value(v, "alt").unwrap()
}

#[tokio::test]
async fn a_different_style_gives_different_prompts_and_hashes() {
    let a = Media::start(cfg(&temp_dir(), 0.0, 0.0), mock(0.0));
    let b = Media::start(styled_cfg(&temp_dir(), alt_style()), mock(0.0));
    let pa = a.prompt(ImageKind::Portrait, "the demigod Rook, wiry, crimson and gold").unwrap();
    let pb = b.prompt(ImageKind::Portrait, "the demigod Rook, wiry, crimson and gold").unwrap();
    assert_eq!(pa.subject, pb.subject);
    assert_ne!(pa.text, pb.text);
    assert!(pa.text.starts_with("Waist-up, three-quarter view, dramatic rim light: the demigod Rook"), "{}", pa.text);
    assert!(pb.text.starts_with("Tight head shot, low key light: the demigod Rook"), "{}", pb.text);
    assert!(pb.text.contains("hand-inked noir") && pb.text.ends_with("Avoid: text, blood."));
    assert_eq!((pa.size.as_str(), pb.size.as_str()), ("1024x1024", "1024x1536"));
    assert_ne!(a.key_for(&pa), b.key_for(&pb));
    assert_ne!(a.style().hash, b.style().hash);
    assert_eq!(b.style().version, "2-noir");
    assert_eq!(b.config().max_events_per_match, 3, "the cap comes from the style");
    // The same subject under the same style is the same image.
    assert_eq!(a.key_for(&pa), a.key_for(&a.prompt(ImageKind::Portrait, "the demigod Rook, wiry, crimson and gold").unwrap()));
    // The env can point at a style file; a bad one is refused at startup with the field named.
    let dir = temp_dir();
    let good = dir.join("style.json");
    // (Its references resolve next to the file, so this copy drops them.)
    let mut body = alt_style().body;
    body.as_object_mut().unwrap().remove("references");
    for k in body["kinds"].as_object_mut().unwrap().values_mut() {
        k.as_object_mut().unwrap().remove("references");
    }
    std::fs::write(&good, serde_json::to_string(&body).unwrap()).unwrap();
    let c = MediaConfig::from_lookup(|k| (k == "FN7_MEDIA_STYLE").then(|| good.display().to_string())).unwrap();
    assert_eq!((c.style.version.as_str(), c.max_events_per_match), ("2-noir", 3));
    let bad = dir.join("bad.json");
    std::fs::write(&bad, r#"{"version":"1","house_style":"x","kinds":{"portrait":{"framing":"x","size":"huge"}},"event_selection":{"min_magnitude":8,"max_per_match":1}}"#).unwrap();
    let e = MediaConfig::from_lookup(|k| (k == "FN7_MEDIA_STYLE").then(|| bad.display().to_string())).unwrap_err();
    assert!(e.to_string().contains("kinds.portrait.size"), "{e}");
}

#[tokio::test]
async fn moment_cards_follow_the_selection_rules_and_never_block() {
    let ev = |row: u32, kind: &str, mag: u8| {
        json!({ "row": format!("lg:{row}"), "kind": kind, "magnitude": mag, "text": format!("{kind} number {row}"),
                "image_prompt": format!("{kind} number {row} over Lagos"), "image_kind": "event" })
    };
    let payload = || json!({ "events": [ ev(1, "cast", 2), ev(2, "eliminate", 3), ev(3, "riot", 6), ev(4, "march", 9),
                                         ev(5, "general_appointed", 4), ev(6, "battle", 7) ] });
    let has = |p: &Value| -> Vec<bool> { p["events"].as_array().unwrap().iter().map(|e| e.get("image").is_some()).collect() };
    // The repo style: magnitude ≥ 8, or cast / elimination / general_appointed / mutiny.
    let a = Media::start(cfg(&temp_dir(), 0.0, 0.0), mock(0.0));
    let mut p = payload();
    a.annotate(&mut p, Some("m"));
    assert_eq!(has(&p), [true, true, false, true, true, false]);
    // Another style: magnitude ≥ 5 or riot, at most 3 per match.
    let b = Media::start(styled_cfg(&temp_dir(), alt_style()), mock(0.0));
    let mut p = payload();
    b.annotate(&mut p, Some("m"));
    assert_eq!(has(&p), [false, false, true, true, false, true]);
    let mut more = json!({ "events": [ ev(7, "riot", 1) ] });
    b.annotate(&mut more, Some("m"));
    assert_eq!(more["events"][0]["image"]["status"], "off", "past max_per_match");
    // Re-annotating the same events (a later globe poll) reuses the images and costs no cap.
    let mut again = payload();
    b.annotate(&mut again, Some("m"));
    assert_eq!(again["events"][2]["image"]["hash"], p["events"][2]["image"]["hash"]);
    assert_eq!(again["events"][2]["image"]["status"], p["events"][2]["image"]["status"]);

    // A slow generator never holds up the payload: annotation only enqueues.
    let slow = Media::start(
        cfg(&temp_dir(), 0.0, 0.0),
        Some(Arc::new(MockGenerator { delay: Duration::from_secs(3), ..Default::default() })),
    );
    let t0 = Instant::now();
    let mut p = payload();
    assert_eq!(slow.annotate(&mut p, Some("m")), 4);
    assert!(t0.elapsed() < Duration::from_millis(500), "{:?}", t0.elapsed());
    assert!(p["events"].as_array().unwrap().iter().filter_map(|e| e.get("image")).all(|i| i["status"] == "pending"));
}

#[tokio::test]
async fn requests_by_reference_are_rate_limited_budgeted_and_idempotent() {
    let dir = temp_dir();
    let mut c = cfg(&dir, 1.0, 0.10);
    c.rate_per_min = 3;
    let media = Media::start(c, mock(0.04));
    let p = |i: u32| media.prompt(ImageKind::Place, &format!("the city number {i}")).unwrap();
    // Idempotent: the same prompt is the same hash, and repeats take no token.
    let (a, why) = media.request_by_ref(&p(1), Some("m"), "seat:0").unwrap();
    assert_eq!((a.status, why), ("pending", None));
    for _ in 0..5 {
        let (again, _) = media.request_by_ref(&p(1), Some("m"), "seat:0").unwrap();
        assert_eq!(again.hash, a.hash);
    }
    // Budget: $0.04 each, $0.10 per match: the third new image is refused (off, with the reason), not an error.
    assert_eq!(media.request_by_ref(&p(2), Some("m"), "seat:0").unwrap().0.status, "pending");
    let (r, why) = media.request_by_ref(&p(3), Some("m"), "seat:0").unwrap();
    assert_eq!(r.status, "off");
    assert!(why.unwrap().contains("match image budget"));
    // Rate: 3 new images per minute per seat; the fourth is an error, and nothing is queued for it.
    let e = media.request_by_ref(&p(4), Some("m"), "seat:0").unwrap_err();
    assert!(e.starts_with("rate limited"), "{e}");
    assert!(media.entry(&media.key_for(&p(4))).is_none());
    // Another seat, or another match, has its own bucket; a known image is still free for the limited seat.
    assert!(media.request_by_ref(&p(4), Some("m"), "seat:1").is_ok());
    assert!(media.request_by_ref(&p(5), Some("other"), "seat:0").is_ok());
    assert!(media.request_by_ref(&p(1), Some("m"), "seat:0").is_ok());
    // Images off: an off image with the reason, never an error.
    let off = Media::off();
    let (r, why) = off.request_by_ref(&p(9), Some("m"), "public").unwrap();
    assert_eq!((r.status, why.as_deref()), ("off", Some("images are off")));
}

#[tokio::test]
async fn annotates_demigod_dioramas() {
    let media = Media::start(cfg(&temp_dir(), 0.0, 0.0), mock(0.0));
    let mut public = json!({ "demigods": [ {
        "ref": "dg:0", "name": "Rook", "image_kind": "portrait", "diorama_image_kind": "diorama",
        "portrait_prompt": "Rook, a demigod: wiry, crimson and gold",
        "diorama_prompt": "Rook, a demigod: wiry; as a collectible minifig on a base themed on Western Africa (dg:0)",
    } ] });
    assert_eq!(media.annotate(&mut public, Some("m")), 2, "a portrait and a diorama");
    let d = &public["demigods"][0];
    assert_eq!(d["portrait_image"]["kind"], "portrait");
    let img = &d["diorama_image"];
    assert_eq!(img["kind"], "diorama");
    let prompt = img["prompt"].as_str().unwrap();
    assert!(!prompt.contains("minifig"), "the style rewrites the plugin's collectible wording: {prompt}");
    assert!(prompt.contains("geometric miniature figure") && prompt.contains("hexagonal diorama tile") && !prompt.contains("dg:0"), "{prompt}");
    // The style file's framing and a square size.
    let style = MediaStyle::builtin();
    let k = style.kind(ImageKind::Diorama);
    assert_eq!(k.size, "1024x1024");
    assert!(prompt.to_lowercase().starts_with(&k.framing[..40]), "{prompt}");
    assert!(style.kinds.contains_key(&ImageKind::Diorama), "data/media_style.json lists diorama");
    assert_eq!(ImageKind::parse("diorama"), Some(ImageKind::Diorama));
    // Annotating again adds nothing new (the image objects are skipped) and the same prompt is the same image.
    let mut again = public.clone();
    media.annotate(&mut again, Some("m"));
    assert_eq!(again["demigods"][0]["diorama_image"]["hash"], public["demigods"][0]["diorama_image"]["hash"]);
}

#[tokio::test]
async fn the_reference_set_is_part_of_the_image_key() {
    let media = Media::start(cfg(&temp_dir(), 0.0, 0.0), mock(0.0));
    let p1 = media.prompt(ImageKind::Portrait, "the demigod Rook").unwrap();
    assert_eq!(p1.refs.len(), 6, "portraits carry Benito's six refs");
    let mut p2 = p1.clone();
    p2.refs.pop();
    assert_ne!(media.key_for(&p1), media.key_for(&p2), "a different ref set is a different image");
    let mut p3 = p1.clone();
    p3.refs[0].sha256 = "0".repeat(64);
    assert_ne!(media.key_for(&p1), media.key_for(&p3), "a changed ref file is a different image");
    assert_eq!(media.key_for(&p1), media.key_for(&media.prompt(ImageKind::Portrait, "the demigod Rook").unwrap()));
    // The request record lists the refs by file name.
    let r = media.request(&p1, Some("m"));
    assert_eq!(r.status, "pending");
}

/// Records the order generations start in.
struct OrderGen {
    started: Mutex<Vec<String>>,
    delay: Duration,
}

#[async_trait::async_trait]
impl ImageGenerator for OrderGen {
    fn provider(&self) -> &'static str {
        "mock"
    }
    fn label(&self) -> String {
        "order".into()
    }
    fn price_usd(&self, _: ImageKind, _: &str) -> Option<f64> {
        Some(0.0)
    }
    async fn generate(&self, req: &GenRequest) -> anyhow::Result<Generated> {
        self.started.lock().unwrap().push(req.prompt.split(':').nth(1).unwrap_or("").trim().split('.').next().unwrap_or("").to_string());
        tokio::time::sleep(self.delay).await;
        Ok(Generated { bytes: b"<svg/>".to_vec(), ext: "svg", cost_usd: 0.0, model: "order".into() })
    }
}

#[tokio::test]
async fn the_queue_starts_portraits_before_queued_event_images() {
    assert_eq!(MediaConfig::default().concurrency, 6, "the default concurrency");
    let gen = Arc::new(OrderGen { started: Mutex::new(vec![]), delay: Duration::from_millis(60) });
    let mut c = cfg(&temp_dir(), 1.0, 1.0);
    c.concurrency = 1;
    c.max_events_per_match = 100;
    let media = Media::start(c, Some(gen.clone() as Arc<dyn ImageGenerator>));
    // Plain-framed kinds so the subject is easy to read back out of the prompt.
    let mut v: Value = serde_json::from_str(style::BUILTIN_STYLE_JSON).unwrap();
    for k in ["event", "statue", "item", "portrait", "diorama"] {
        v["kinds"][k]["framing"] = json!(k);
    }
    let st = MediaStyle::from_value(v, "order").unwrap();
    let q = |kind, s: &str| ImagePrompt::styled(&st, kind, s).unwrap();
    // e0 takes the only slot; the rest queue up in this order.
    for (kind, name) in [
        (ImageKind::Event, "e0"), (ImageKind::Event, "e1"), (ImageKind::Event, "e2"), (ImageKind::Item, "i1"),
        (ImageKind::Statue, "s1"), (ImageKind::Portrait, "p1"), (ImageKind::Event, "e3"), (ImageKind::Diorama, "d1"),
        (ImageKind::Portrait, "p2"),
    ] {
        media.request(&q(kind, name), Some("m"));
        if name == "e0" {
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }
    assert!(media.wait_idle(Duration::from_secs(10)).await);
    let order = gen.started.lock().unwrap().clone();
    assert_eq!(order, ["e0", "p1", "d1", "p2", "s1", "i1", "e1", "e2", "e3"], "by priority, FIFO within one");
}

// ---------------------------------------------------------------------------------------------------------------------
// Gear looks: a picture drawn on a base image, the pending swap, the cache across matches
// ---------------------------------------------------------------------------------------------------------------------

/// Records every request (its key and the reference images sent with it).
struct Spy {
    made: Mutex<Vec<(String, Vec<String>)>>,
    delay: Duration,
}

#[async_trait::async_trait]
impl ImageGenerator for Spy {
    fn provider(&self) -> &'static str {
        "mock"
    }
    fn label(&self) -> String {
        "mock".into()
    }
    fn price_usd(&self, _: ImageKind, _: &str) -> Option<f64> {
        Some(0.0)
    }
    async fn generate(&self, req: &GenRequest) -> anyhow::Result<Generated> {
        tokio::time::sleep(self.delay).await;
        self.made.lock().unwrap().push((req.key.clone(), req.refs.iter().map(|r| r.name.clone()).collect()));
        Ok(Generated { bytes: format!("<svg>{}</svg>", req.key).into_bytes(), ext: "svg", cost_usd: 0.0, model: "spy".into() })
    }
}

fn spy(delay_ms: u64) -> Arc<Spy> {
    Arc::new(Spy { made: Mutex::new(vec![]), delay: Duration::from_millis(delay_ms) })
}

const BASE_P: &str = "Chris's Left Hand, a demigod: wiry and long-limbed, in violet and jet black";
const BASE_D: &str = "Chris's Left Hand, a demigod: wiry and long-limbed; as a tiny geometric miniature figure";

/// A public demigod with a `look` as the plugin sends it.
fn demigod_with_look(loadout: &str, gear: Option<&str>, fallback: &[&str]) -> Value {
    let subj = |g: &str, base: &str| format!("{base}; newly geared: {g}; the same face as before");
    json!({ "ref": "dg:0", "name": "Chris's Left Hand", "player": "Chris", "controller": "human",
            "portrait_prompt": BASE_P, "image_kind": "portrait", "diorama_prompt": BASE_D,
            "look": {
                "loadout": [ { "slot": "weapon", "item": "it:3", "name": "Butterfly Wing Blade", "icon": "sword", "tier": "relic" } ],
                "loadout_hash": loadout, "image_loadout_hash": loadout, "since_beat": 4, "gear": gear.is_some(),
                "base_portrait": BASE_P, "base_diorama": BASE_D,
                "portrait_subject": gear.map(|g| subj(g, BASE_P)), "diorama_subject": gear.map(|g| subj(g, BASE_D)),
                "fallback": fallback.iter().map(|g| json!({ "loadout_hash": "old", "portrait_subject": subj(g, BASE_P), "diorama_subject": subj(g, BASE_D) })).collect::<Vec<_>>(),
            } })
}

#[tokio::test]
async fn a_gear_look_is_drawn_on_its_base_and_swaps_in_when_ready() {
    let dir = temp_dir();
    let gen = spy(40);
    let media = Media::start(cfg(&dir, 0.0, 0.0), Some(gen.clone() as Arc<dyn ImageGenerator>));
    // No gear: the look's images are the base identity images.
    let mut v = demigod_with_look("", None, &[]);
    let (_, seen) = media.annotate_seen(&mut v, Some("m1"));
    assert_eq!(v["look"]["portrait_image"]["hash"], v["portrait_image"]["hash"]);
    assert_eq!(v["look"]["diorama_image"]["hash"], v["diorama_image"]["hash"]);
    assert!(v["look"].get("pending").is_none() && v["look"].get("base_portrait").is_none() && v["look"].get("fallback").is_none(), "{}", v["look"]);
    assert_eq!((seen.len(), seen[0].demigod.as_deref(), seen[0].player.as_deref(), seen[0].portrait.is_none()), (1, Some("dg:0"), Some("Chris"), true));
    assert!(media.wait_idle(Duration::from_secs(5)).await);
    let (base_p, base_d) = (v["portrait_image"]["hash"].as_str().unwrap().to_string(), v["diorama_image"]["hash"].as_str().unwrap().to_string());
    assert_eq!(gen.made.lock().unwrap().len(), 2);

    // A major acquisition: the gear images are asked for; until they exist the payload keeps the base images and
    // names the new ones as pending.
    let mut v = demigod_with_look("h1", Some("gripped in the right hand, a blade"), &[]);
    let (_, seen) = media.annotate_seen(&mut v, Some("m1"));
    let l = &v["look"];
    assert_eq!((l["portrait_image"]["hash"].as_str(), l["portrait_image"]["status"].as_str()), (Some(base_p.as_str()), Some("ready")));
    assert_eq!(l["diorama_image"]["hash"], base_d.as_str());
    let (gear_p, gear_d) = (l["pending"]["portrait_image"]["hash"].as_str().unwrap().to_string(), l["pending"]["diorama_image"]["hash"].as_str().unwrap().to_string());
    assert_eq!((l["pending"]["portrait_image"]["status"].as_str(), l["pending"]["loadout_hash"].as_str()), (Some("pending"), Some("h1")));
    assert!(gear_p != base_p && gear_d != base_d && gear_p != gear_d);
    let pp = l["pending"]["portrait_image"]["prompt"].as_str().unwrap();
    assert!(pp.find(style::DEFAULT_PORTRAIT_BASE_NOTE).unwrap() < pp.find("Style:").unwrap(), "a gear portrait's note comes before the style: {pp}");
    assert!(l["pending"]["diorama_image"]["prompt"].as_str().unwrap().ends_with(style::DEFAULT_BASE_NOTE), "the base note closes a gear diorama's prompt");
    assert_eq!(v["portrait_image"]["hash"], base_p.as_str(), "the identity image stays for older clients");
    assert_eq!((seen[0].loadout_hash.as_str(), seen[0].since_beat, seen[0].loadout.clone()), ("h1", Some(4), vec!["Butterfly Wing Blade".to_string()]));
    assert_eq!(seen[0].portrait.as_ref().map(|p| p.hash.clone()), Some(gear_p.clone()));
    // Asking again while it renders changes nothing and queues nothing more.
    let mut again = demigod_with_look("h1", Some("gripped in the right hand, a blade"), &[]);
    media.annotate_seen(&mut again, Some("m1"));
    assert_eq!(again, v);
    assert!(media.wait_idle(Duration::from_secs(5)).await);
    // Ready: the same payload now shows the gear images and nothing is pending.
    let mut v2 = demigod_with_look("h1", Some("gripped in the right hand, a blade"), &[]);
    media.annotate_seen(&mut v2, Some("m1"));
    assert_eq!((v2["look"]["portrait_image"]["hash"].as_str(), v2["look"]["portrait_image"]["status"].as_str()), (Some(gear_p.as_str()), Some("ready")));
    assert_eq!(v2["look"]["diorama_image"]["hash"], gear_d.as_str());
    assert!(v2["look"].get("pending").is_none());
    // Each gear picture was generated once, with its own base as the FIRST reference image.
    let made = gen.made.lock().unwrap().clone();
    assert_eq!(made.len(), 4);
    let refs_of = |k: &str| made.iter().find(|m| m.0 == k).unwrap().1.clone();
    assert_eq!(refs_of(&gear_p)[0], format!("base/{base_p}.svg"));
    assert_eq!(refs_of(&gear_d)[0], format!("base/{base_d}.svg"));
    assert_eq!(refs_of(&gear_p).len(), 1 + refs_of(&base_p).len(), "the style's references follow the base");

    // The next acquisition: while it renders, the PREVIOUS gear look stands in (not the bare base).
    let mut v3 = demigod_with_look("h2", Some("gripped in the right hand, a blade; strapped over the chest, plate armour"), &["gripped in the right hand, a blade"]);
    media.annotate_seen(&mut v3, Some("m1"));
    assert_eq!(v3["look"]["portrait_image"]["hash"], gear_p.as_str());
    assert_eq!(v3["look"]["pending"]["loadout_hash"], "h2");
    assert!(media.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(gen.made.lock().unwrap().len(), 6);

    // Another process, another match, the same cache directory (a later match, or a replay through the server):
    // every look resolves to the same files and nothing is generated.
    let gen2 = spy(0);
    let later = Media::start(cfg(&dir, 0.0, 0.0), Some(gen2.clone() as Arc<dyn ImageGenerator>));
    for (h, g, fb) in [("h1", Some("gripped in the right hand, a blade"), vec![]),
                       ("h2", Some("gripped in the right hand, a blade; strapped over the chest, plate armour"), vec!["gripped in the right hand, a blade"]),
                       ("", None, vec![])] {
        let mut x = demigod_with_look(h, g, &fb);
        later.annotate_seen(&mut x, Some("m2"));
        assert_eq!(x["look"]["portrait_image"]["status"], "ready", "{h}");
        assert_eq!(x["look"]["diorama_image"]["status"], "ready");
        assert!(x["look"].get("pending").is_none());
        if h == "h1" {
            assert_eq!(x["look"]["portrait_image"]["hash"], gear_p.as_str(), "the same key in any match");
        }
    }
    assert!(later.wait_idle(Duration::from_secs(5)).await);
    assert!(gen2.made.lock().unwrap().is_empty(), "nothing is generated twice");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_look_changed_event_is_annotated_like_a_look_and_the_key_names_its_base() {
    let dir = temp_dir();
    let media = Media::start(cfg(&dir, 0.0, 0.0), Some(spy(30) as Arc<dyn ImageGenerator>));
    let look = demigod_with_look("h1", Some("gripped in the right hand, a blade"), &[])["look"].clone();
    let mut ev = look.clone();
    ev["event"] = "look.changed".into();
    ev["demigod"] = "dg:0".into();
    ev["name"] = "Chris's Left Hand".into();
    ev["player"] = "Chris".into();
    ev["gained"] = json!({ "item": "it:3", "name": "Butterfly Wing Blade", "icon": "sword", "tier": "relic", "image_prompt": "a blade of butterfly-wing", "image_kind": "item" });
    let raw = ev.clone();
    let mut payload = json!({ "dm_events": [ev] });
    let (_, seen) = media.annotate_seen(&mut payload, Some("m1"));
    let e = &payload["dm_events"][0];
    assert_eq!(e["pending"]["portrait_image"]["status"], "pending");
    assert_eq!(e["gained"]["image"]["kind"], "item", "the gained item's picture rides along: {e}");
    assert_eq!((seen.len(), seen[0].demigod.as_deref(), seen[0].event.as_ref()), (1, Some("dg:0"), Some(&raw)));
    // The key: the same subject on another base is another image; the same on the same base is the same image.
    let base = media.prompt(ImageKind::Portrait, BASE_P).unwrap();
    let other = media.prompt(ImageKind::Portrait, "Vex, a demigod").unwrap();
    let a = media.prompt_on(ImageKind::Portrait, "a subject", &base).unwrap();
    assert_eq!(media.key_for(&a), media.key_for(&media.prompt_on(ImageKind::Portrait, "a subject", &base).unwrap()));
    assert_ne!(media.key_for(&a), media.key_for(&media.prompt_on(ImageKind::Portrait, "a subject", &other).unwrap()));
    assert_ne!(media.key_for(&a), media.key_for(&media.prompt(ImageKind::Portrait, "a subject").unwrap()));
    assert_eq!(a.base.as_deref(), Some(media.key_for(&base).as_str()));
    assert_eq!(a.ref_count(), a.refs.len() + 1);
    // An item picture's key is its subject's: the same descriptor is one image.
    let i = |s: &str| media.key_for(&media.prompt(ImageKind::Item, s).unwrap());
    assert_eq!(i("a blade of beaten gold: a demigod's weapon, ornate, glowing steadily"), i("a blade of beaten gold: a demigod's weapon, ornate, glowing steadily"));
    assert_ne!(i("a blade of beaten gold: a demigod's weapon, ornate, glowing steadily"), i("a blade of beaten gold: a demigod's weapon, plain and battered"));
    assert!(media.wait_idle(Duration::from_secs(5)).await);
    // A gear picture whose base was refused is not generated (and is retried by a later match).
    let poor = Media::start(cfg(&temp_dir(), 0.0, 0.0), Some(Arc::new(MockGenerator { price_usd: 1.0, ..Default::default() }) as Arc<dyn ImageGenerator>));
    let mut x = demigod_with_look("h1", Some("gripped in the right hand, a blade"), &[]);
    poor.annotate_seen(&mut x, Some("m1"));
    assert_eq!(x["look"]["portrait_image"]["status"], "off");
    assert!(x["look"].get("pending").is_none(), "nothing will come: nothing is pending");
    let _ = std::fs::remove_dir_all(dir);
}

/// A gear portrait (match ae54b0af kept the base portrait's hands and never showed the blade): the portrait base note
/// lets the hands change and comes right after the subject, BEFORE the style text; its key carries the gear-portrait
/// version, so a picture from the old template is never reused. A gear diorama is unchanged (its note at the end, its
/// key as before); identity portraits never carry a gear version.
#[tokio::test]
async fn a_gear_portrait_says_the_hands_change_before_the_style() {
    let dir = temp_dir();
    let media = Media::start(cfg(&dir, 0.0, 0.0), Some(spy(30) as Arc<dyn ImageGenerator>));
    let st = media.style().clone();
    let subject = "Chris's Left Hand, a demigod, newly geared: waist-up, the new gear is the subject of this shot: raised in the right hand and held up across the chest, a blade of black volcanic obsidian glass with razor facets, plain and battered";
    let base = media.prompt(ImageKind::Portrait, BASE_P).unwrap();
    let p = media.prompt_on(ImageKind::Portrait, subject, &base).unwrap();
    let t = &p.text;
    let (g, note, style) = (t.find("raised in the right hand").unwrap(), t.find(&st.portrait_base_note).unwrap(), t.find("Style:").unwrap());
    assert!(g < note && note < style, "the gear, the note, then the style: {t}");
    assert!(t.ends_with(&st.suffix_for(ImageKind::Portrait)), "{t}");
    assert!(st.portrait_base_note.contains("but not what its hands hold") && !st.portrait_base_note.contains("pose family exactly"));
    // The key carries the gear-portrait version: the same request as another kind (no gear version) keys differently.
    assert_eq!(gear_version(ImageKind::Portrait), Some("fn7-gear-portrait/2"));
    assert_eq!(gear_version(ImageKind::Diorama), None);
    let mut same = p.clone();
    same.kind = ImageKind::Diorama;
    assert_ne!(media.key_for(&p), media.key_for(&same));
    // Identity portraits (no base) key exactly as before: no gear version.
    assert_eq!(media.key_for(&base), image_key(&media.inner.label, &base));
    let mut id = base.clone();
    id.kind = ImageKind::Diorama;
    assert_eq!(image_key("x", &base), image_key("x", &id), "without a base the kind adds nothing to the key");
    // A gear diorama: the shared base note at the end, as before.
    let dbase = media.prompt(ImageKind::Diorama, BASE_P).unwrap();
    let d = media.prompt_on(ImageKind::Diorama, "a subject", &dbase).unwrap();
    assert!(d.text.ends_with(&st.base_note), "{}", d.text);
    assert!(!d.text.contains(&st.portrait_base_note));
    let _ = std::fs::remove_dir_all(dir);
}
