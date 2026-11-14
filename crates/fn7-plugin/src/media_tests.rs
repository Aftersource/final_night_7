//! Media on the plugin side: visual sheets (stable across beats, deterministic across replays), `game.media`
//! (visibility, shapes, idempotence) and the storyboard (shape, order, determinism, a full one in a played match).

use std::sync::OnceLock;

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::testkit::Scripted;
use crate::*;

fn seats() -> Vec<SeatInfo> {
    ["Ash", "Brix", "Cyra", "Dune"]
        .iter()
        .enumerate()
        .map(|(i, n)| SeatInfo { seat: i as u8, display_name: n.to_string(), is_bot: true })
        .collect()
}

struct Played {
    /// The public view's demigods after every commit.
    views: Vec<Value>,
    last: Value,
    summary: Summary,
    /// Every public event of every outcome.
    events: Vec<Value>,
}

async fn play(seed: u64) -> Played {
    let p = FinalNight7Plugin::new();
    let mut views = vec![];
    let mut events = vec![];
    let none = |_: Seat, _: u32| None;
    let settings = json!({ "seed": seed, "beats_total": 8, "clusters": 60 });
    let (last, summary) = driver::run_match(&p, &seats(), &settings, &Scripted::default(), None, &none, |rep, st| {
        views.push(p.public_view(st)["demigods"].clone());
        events.extend(rep.outcome.public["events"].as_array().cloned().unwrap_or_default());
        true
    })
    .await
    .unwrap();
    Played { views, last, summary, events }
}

fn played() -> &'static (Played, Played) {
    static S: OnceLock<(Played, Played)> = OnceLock::new();
    S.get_or_init(|| {
        std::thread::spawn(|| {
            tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap().block_on(async {
                (play(11).await, play(11).await)
            })
        })
        .join()
        .unwrap()
    })
}

fn visuals(demigods: &Value) -> Vec<Value> {
    demigods.as_array().unwrap().iter().map(|d| d["visual"].clone()).collect()
}

#[test]
fn visual_sheets_are_stable_across_beats_and_deterministic_across_replays() {
    let (a, b) = played();
    assert!(a.views.len() >= 3);
    let first = visuals(&a.views[0]);
    for v in &first {
        assert_eq!(v["sheet_hash"].as_str().unwrap().len(), 64, "{v}");
        let t = &v["traits"];
        for k in ["build", "face", "outfit", "prop", "emblem"] {
            assert!(t[k].as_str().is_some_and(|s| !s.is_empty()), "{k} in {t}");
        }
        assert_eq!(t["colors"].as_array().unwrap().len(), 2);
    }
    let hashes: std::collections::BTreeSet<&str> = first.iter().map(|v| v["sheet_hash"].as_str().unwrap()).collect();
    assert_eq!(hashes.len(), first.len(), "every demigod has its own sheet");
    for view in &a.views {
        assert_eq!(visuals(view), first, "the same sheet at every beat");
    }
    // The same match again (a replay): the same sheets; the globe query and the summary agree.
    assert_eq!(visuals(&b.views[0]), first);
    let p = FinalNight7Plugin::new();
    let globe = p.query(&a.last, None, "globe", &json!({})).unwrap();
    assert_eq!(visuals(&globe["demigods"]), first);
    // Another seed: the same demigod sheets (a demigod's sheet is its name alone), but other person sheets.
    let w = world::load(&a.last).unwrap();
    let mut w2 = w.clone();
    w2.meta.seed ^= 0xdead_beef;
    assert_eq!(visual::demigod(&w, &w.demigods[0]).sheet_hash, visual::demigod(&w2, &w2.demigods[0]).sheet_hash);
    if let Some(c) = w.characters.first() {
        assert_ne!(visual::person(&w, c).sheet_hash, visual::person(&w2, &w2.characters[0]).sheet_hash);
    }
    // Every image of a character carries its sheet: portrait, statue, and events that name it.
    for (i, d) in w.demigods.iter().enumerate() {
        let brief = visual::demigod(&w, d).brief();
        assert!(a.views[0][i]["portrait_prompt"].as_str().unwrap().contains(&brief));
        assert!(a.summary.summary["piles"][i]["statue_prompt"].as_str().unwrap().contains(&brief));
    }
    let starring: Vec<&Value> = a
        .events
        .iter()
        .filter(|e| e["refs"].as_array().unwrap().iter().any(|r| r.as_str().is_some_and(|r| r.starts_with("dg:"))))
        .collect();
    assert!(!starring.is_empty());
    for e in starring.iter().take(20) {
        let r: Ref = e["refs"].as_array().unwrap().iter().find_map(|r| r.as_str().filter(|r| r.starts_with("dg:"))).unwrap().parse().unwrap();
        let prompt = e["image_prompt"].as_str().unwrap();
        if prompt.contains("starring") {
            let x = visual::of(&w, r).unwrap();
            assert!(prompt.contains(&x.traits.emblem) || !prompt.contains(&w.name_of(r)), "{prompt}");
        }
    }
    // A person's sheet in the person query.
    let c = w.characters.first().expect("people");
    let person = p.query(&a.last, None, "person", &json!({ "ref": Ref::Char(c.id).to_string() })).unwrap();
    assert_eq!(person["visual"], visual::person(&w, c).to_json());
}

#[test]
fn every_public_event_is_a_candidate_moment_card() {
    let (a, _) = played();
    assert!(!a.events.is_empty());
    for e in &a.events {
        assert_eq!(e["image_kind"], "event", "{e}");
        let p = e["image_prompt"].as_str().unwrap();
        assert!(!p.is_empty());
        assert!(!p.contains("dg:") && !p.contains("ch:"), "{p}");
    }
}

#[test]
fn the_storyboard_has_its_shape_order_and_is_deterministic() {
    let (a, b) = played();
    let sb = a.summary.summary["storyboard"].as_array().expect("storyboard");
    assert_eq!(&a.summary.summary["storyboard"], &b.summary.summary["storyboard"], "deterministic");
    assert!(sb.len() >= 3, "title, at least one moment, champion: {}", sb.len());
    assert_eq!(sb.first().unwrap()["kind"], "title");
    assert_eq!(sb.last().unwrap()["kind"], "champion");
    let w = world::load(&a.last).unwrap();
    let mut last_row: i64 = -1;
    let mut last_beat = 0;
    for (i, s) in sb.iter().enumerate() {
        assert_eq!(s["shot"], i, "{s}");
        for k in ["beat", "at", "region", "image", "caption", "duration_ms", "camera", "image_prompt", "image_kind"] {
            assert!(s.get(k).is_some(), "shot {i} lacks {k}: {s}");
        }
        assert!(s["image"].is_null(), "the plugin leaves image to the media server");
        assert_eq!(s["at"].as_array().unwrap().len(), 2, "{s}");
        assert!(s["region"].as_str().is_some_and(|r| !r.is_empty()), "{s}");
        assert!(s["caption"].as_str().is_some_and(|c| !c.is_empty()), "{s}");
        assert!(s["duration_ms"].as_u64().unwrap() >= 1000);
        assert!(matches!(s["camera"].as_str(), Some("globe" | "close")));
        assert!(matches!(s["image_kind"].as_str(), Some("event" | "portrait")));
        let beat = s["beat"].as_u64().unwrap();
        assert!(beat >= last_beat, "shots are in story order");
        last_beat = beat;
        if s["kind"] == "moment" {
            let row: Ref = s["row"].as_str().unwrap().parse().unwrap();
            let Ref::Ledger(l) = row else { panic!("{row}") };
            assert!(l.0 as i64 > last_row);
            last_row = l.0 as i64;
            // The same prompt as the row's moment card: the same picture.
            let r = &w.ledger.rows[l.ix()];
            assert_eq!(s["image_prompt"], payload::event_json(&w, r)["image_prompt"]);
            assert_eq!(s["camera"], if r.verb == "eliminate" { "close" } else { "globe" });
        }
    }
    let moments = sb.iter().filter(|s| s["kind"] == "moment").count();
    assert!((1..=phases::STORYBOARD_MOMENTS).contains(&moments));
    // Every elimination is in it (when there are no more than the cap).
    let elims = w.ledger.rows.iter().filter(|r| r.verb == "eliminate" && payload::is_public_event(&w, r)).count();
    if elims <= phases::STORYBOARD_MOMENTS {
        assert_eq!(sb.iter().filter(|s| s["camera"] == "close" && s["kind"] == "moment").count(), elims);
    }
    // The title is the final cut's; the champion shot is the champion's portrait (the same prompt as the globe's).
    assert_eq!(sb[0]["caption"], a.summary.summary["final_cut"]["title"]);
    let champ = &a.summary.summary["champions"][0];
    let last = sb.last().unwrap();
    assert_eq!(last["refs"][0], champ["demigod"]);
    let i = w.demigods.iter().position(|d| Ref::Demigod(d.id).to_string() == champ["demigod"].as_str().unwrap()).unwrap();
    assert_eq!(last["image_prompt"], a.views.last().unwrap()[i]["portrait_prompt"]);
    assert_eq!(last["camera"], "close");
}

#[test]
fn closing_lines_and_storyboard_without_a_final_cut() {
    let (a, _) = played();
    let w = world::load(&a.last).unwrap();
    let sb = phases::storyboard(&w, &Value::Null);
    assert_eq!(sb[0]["caption"], "Final Night 7");
    assert!(sb.last().unwrap()["caption"].as_str().unwrap().contains("stands alone"));
    let sb = phases::storyboard(&w, &json!({ "title": "T", "story": "One. Two! Three? Four and the end." }));
    assert_eq!(sb.last().unwrap()["caption"], "Three? Four and the end.");
    let sb = phases::storyboard(&w, &json!({ "title": "T", "story": "Beat 3: Rook trips.\nBeat 4: Moth wins it all." }));
    assert_eq!(sb.last().unwrap()["caption"], "Rook trips. Moth wins it all.");
}

fn q(state: &Value, seat: Option<Seat>, args: Value) -> Result<Value, String> {
    FinalNight7Plugin::new().query(state, seat, "media", &args)
}

#[test]
fn media_queries_check_visibility_and_write_prompts_from_state() {
    let (a, _) = played();
    let st = &a.last;
    let w = world::load(st).unwrap();
    // Portraits of demigods: public; the subject is exactly the globe's portrait prompt (so the same image).
    let v = q(st, None, json!({ "kind": "portrait", "ref": "dg:1" })).unwrap();
    assert_eq!((v["kind"].as_str(), v["ref"].as_str(), v["image_kind"].as_str()), (Some("portrait"), Some("dg:1"), Some("portrait")));
    assert_eq!(v["image_prompt"], a.views.last().unwrap()[1]["portrait_prompt"]);
    assert_eq!(v["visual"], a.views.last().unwrap()[1]["visual"]);
    assert_eq!(q(st, Some(0), json!({ "kind": "portrait", "ref": "dg:1" })).unwrap(), v, "idempotent, any asker");
    // Places and armies.
    let region = w.places.iter().find(|p| p.kind == PlaceKind::District).unwrap();
    let v = q(st, None, json!({ "kind": "place", "ref": Ref::Place(region.id).to_string() })).unwrap();
    assert!(v["image_prompt"].as_str().unwrap().contains(&region.name));
    if let Some((c, _)) = fn7_society::armies(&w).next() {
        let v = q(st, None, json!({ "kind": "army", "ref": Ref::Cluster(c.id).to_string() })).unwrap();
        assert!(v["image_prompt"].as_str().unwrap().contains("banners in"), "{v}");
    }
    let crowd = w.clusters.iter().find(|c| c.army.is_none()).unwrap();
    assert!(q(st, None, json!({ "kind": "army", "ref": Ref::Cluster(crowd.id).to_string() })).unwrap_err().contains("not an army"));
    // Bad requests.
    assert!(q(st, None, json!({ "kind": "selfie", "ref": "dg:0" })).is_err());
    assert!(q(st, None, json!({ "kind": "portrait", "ref": "pl:0" })).unwrap_err().contains("demigod or person"));
    assert!(q(st, None, json!({ "kind": "event", "ref": "lg:999999" })).unwrap_err().contains("no such event"));
    assert!(q(st, None, json!({ "kind": "portrait" })).is_err());
    // Events: a public one for anyone, with the moment card's prompt; a private one only for a seat that knows it.
    let public = w.ledger.rows.iter().rev().find(|r| payload::is_public_event(&w, r)).unwrap();
    let v = q(st, None, json!({ "kind": "event", "ref": Ref::Ledger(public.id).to_string() })).unwrap();
    assert_eq!(v["image_prompt"], payload::event_json(&w, public)["image_prompt"]);
    let mut checked = 0;
    for r in w.ledger.rows.iter().rev().filter(|r| !payload::is_public_event(&w, r)).take(400) {
        let arg = json!({ "kind": "event", "ref": Ref::Ledger(r.id).to_string() });
        assert!(q(st, None, arg.clone()).unwrap_err().contains("not something you can see"), "HTTP never gets {r:?}");
        for s in &w.seats {
            let d = s.demigod;
            let knows = world::knows(&world::selves(&w, d), w.demigods[d.ix()].following, r);
            assert_eq!(q(st, Some(s.seat), arg.clone()).is_ok(), knows, "seat {} on {}", s.seat, r.id.0);
            checked += knows as u32;
        }
    }
    assert!(checked > 0, "some seat knew some private row");
    // People: public ones for anyone; a private one only for a seat that knows them.
    let mut hidden = 0;
    let mut known = 0;
    for c in w.characters.iter().take(300) {
        let arg = json!({ "kind": "portrait", "ref": Ref::Char(c.id).to_string() });
        let public = queries::person_public(&w, st, c.id);
        assert_eq!(q(st, None, arg.clone()).is_ok(), public, "{} public={public}", c.name);
        if !public {
            hidden += 1;
            for s in &w.seats {
                let k = queries::person_known(&w, st, s.demigod, c.id);
                assert_eq!(q(st, Some(s.seat), arg.clone()).is_ok(), k);
                known += k as u32;
            }
        }
    }
    assert!(hidden > 0, "some people are private");
    let _ = known;
}
