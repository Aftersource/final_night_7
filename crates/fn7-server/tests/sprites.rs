//! The people-and-houses library through the server (mock generator only, FN7_IMAGES=mock; nothing here calls a paid
//! API): `game.people` / `game.buildings` sprites get their pictures, one per library key, cached by key; the prewarm
//! asks for all 72 once; the budget refuses past its limit (the payloads keep `off`); every picture is an asset row
//! (subkind sprite / building, the exact prompt, the cost).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fn7_media::{ImageGenerator, Media, MediaConfig, MockGenerator, Mode};
use fn7_plugin::FinalNight7Plugin;
use fn7_server::media::{library_prewarm_on, prewarm_library, MediaPlugin, Prewarm};
use serde_json::{json, Value};
use theatre_plugin_api::GamePlugin;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fn7-sprites-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn media(dir: &std::path::Path, price: f64, budget: f64) -> Media {
    let cfg = MediaConfig {
        mode: Mode::Mock,
        dir: dir.join("media"),
        budget_usd: budget,
        match_budget_usd: budget,
        ..Default::default()
    };
    let gen: Arc<dyn ImageGenerator> = Arc::new(MockGenerator { price_usd: price, ..Default::default() });
    Media::start(cfg, Some(gen))
}

async fn played() -> Value {
    fn7_plugin::testkit::played_state(9, 8, 60).await
}

/// A board centred on the first city: its people and its houses.
fn boards(state: &Value) -> (Value, Value) {
    let w = fn7_plugin::world::load(state).unwrap();
    let city = w.places.iter().find(|p| p.kind == fn7_model::PlaceKind::Site).unwrap();
    let key = fn7_model::hexgrid::cell_at(city.center, fn7_model::hexgrid::HexLevel::Villages).key();
    let houses = fn7_model::hexgrid::cell_at(city.center, fn7_model::hexgrid::HexLevel::Houses).key();
    (json!({ "cell": key, "limit": 300 }), json!({ "cell": houses }))
}

#[tokio::test(flavor = "multi_thread")]
async fn sprites_are_one_picture_per_key_and_cached() {
    let dir = temp_dir("lazy");
    let m = media(&dir, 0.0, 0.0);
    let p = MediaPlugin::new(FinalNight7Plugin::new(), m.clone());
    let state = played().await;
    let (pa, ba) = boards(&state);
    let v = p.query(&state, None, "people", &pa).unwrap();
    let people = v["people"].as_array().unwrap();
    assert!(!people.is_empty());
    let mut by_key = std::collections::BTreeMap::new();
    for x in people {
        let s = &x["sprite"];
        assert_eq!(s["kind"], "sprite");
        assert!(matches!(s["status"].as_str(), Some("pending" | "ready")), "{s}");
        let h = s["hash"].as_str().unwrap().to_string();
        assert_eq!(by_key.entry(s["key"].as_str().unwrap().to_string()).or_insert_with(|| h.clone()), &h, "one picture per key");
    }
    let b = p.query(&state, None, "buildings", &ba).unwrap();
    assert!(!b["buildings"].as_array().unwrap().is_empty());
    assert!(m.wait_idle(Duration::from_secs(20)).await);
    // Asked again: ready, with the same hashes and a URL.
    let v2 = p.query(&state, None, "people", &pa).unwrap();
    for x in v2["people"].as_array().unwrap() {
        assert_eq!(x["sprite"]["status"], "ready", "{}", x["sprite"]);
        assert_eq!(x["sprite"]["hash"].as_str().unwrap(), by_key[x["sprite"]["key"].as_str().unwrap()]);
        assert!(x["sprite"]["url"].as_str().unwrap().starts_with("/media/"));
    }
    // The manifest carries the prompt in the library's framing.
    let man = m.manifest(0);
    let item = man["items"].as_array().unwrap().iter().find(|i| i["kind"] == "sprite").unwrap().clone();
    assert!(item["prompt"].as_str().unwrap().contains("full-body game sprite of a single character"), "{item}");
    // Images off: the bare stub goes out untouched.
    let off = MediaPlugin::new(FinalNight7Plugin::new(), Media::off());
    let v3 = off.query(&state, None, "people", &pa).unwrap();
    assert_eq!(v3["people"][0]["sprite"]["status"], "off");
    assert_eq!(v3["people"][0]["sprite"]["hash"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_prewarm_respects_the_budget_and_refusals_stay_off() {
    assert!(library_prewarm_on(|k| (k == "FN7_SPRITES_PREWARM").then(|| "on".to_string())));
    assert!(!library_prewarm_on(|_| None), "off by default");
    let dir = temp_dir("budget");
    // $0.025 a picture, $0.06 left: two pictures, the other 70 refused.
    let m = media(&dir, 0.025, 0.06);
    assert_eq!(prewarm_library(&m), Prewarm { queued: 2, cached: 0, refused: 70 });
    assert!(m.wait_idle(Duration::from_secs(20)).await);
    let man = m.manifest(0);
    let items = man["items"].as_array().unwrap();
    let refused: Vec<&Value> = items.iter().filter(|i| i["status"] == "refused").collect();
    assert_eq!(refused.len(), 70);
    assert!(refused[0]["reason"].as_str().unwrap().contains("image budget reached"), "{}", refused[0]);
    // A refused sprite in a payload stays `off` (the client keeps its placeholder); asking again spends nothing.
    let p = MediaPlugin::new(FinalNight7Plugin::new(), m.clone());
    let state = played().await;
    let (pa, _) = boards(&state);
    let v = p.query(&state, None, "people", &pa).unwrap();
    for x in v["people"].as_array().unwrap() {
        let st = x["sprite"]["status"].as_str().unwrap();
        assert!(matches!(st, "off" | "ready"), "{}", x["sprite"]);
    }
    assert!(v["people"].as_array().unwrap().iter().any(|x| x["sprite"]["status"] == "off"));
    let (spent, _) = m.spent(Some("library"));
    assert!((spent - 0.05).abs() < 1e-9, "spent {spent}");
    // Images off: nothing is asked for.
    assert_eq!(prewarm_library(&Media::off()), Prewarm::default());
}

#[tokio::test(flavor = "multi_thread")]
async fn every_library_picture_is_an_asset_row() {
    let dir = temp_dir("registry");
    let m = media(&dir, 0.0, 0.0);
    let (reg, _) = fn7_server::assets::open(&dir, |k| (k == "FN7_ASSETS_DIR").then(|| dir.join("assets").display().to_string())).unwrap().unwrap();
    let hook = fn7_server::assets::attach(reg.clone(), &m);
    assert_eq!(prewarm_library(&m), Prewarm { queued: 72, cached: 0, refused: 0 });
    assert!(m.wait_idle(Duration::from_secs(30)).await);
    hook.flush();
    let count = |sql: &str| -> i64 { reg.with(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap() };
    assert_eq!(count("SELECT COUNT(*) FROM assets WHERE subkind = 'sprite' AND status = 'ready'"), 48);
    assert_eq!(count("SELECT COUNT(*) FROM assets WHERE subkind = 'building' AND status = 'ready'"), 24);
    assert_eq!(count("SELECT COUNT(DISTINCT subject_key) FROM assets WHERE subkind IN ('sprite', 'building')"), 72);
    assert_eq!(count("SELECT COUNT(*) FROM assets WHERE subkind = 'sprite' AND subject_key = 'sprite/witch/1'"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM generations g JOIN assets a ON a.id = g.asset_id WHERE a.subkind IN ('sprite', 'building') AND g.prompt LIKE '%(look %' AND g.cost_usd IS NOT NULL AND g.requested_by = 'library'"), 72);
    // A second process on the same cache: all 72 cached, nothing spent.
    let m2 = media(&dir, 0.025, 0.0);
    assert_eq!(prewarm_library(&m2), Prewarm { queued: 0, cached: 72, refused: 0 });
}
