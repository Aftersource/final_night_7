//! Gear looks through the server (mock images only; no LLM, no paid API): a recorded match in which a human arms
//! their demigods. The look changes with `pending`, the mock images arrive and the next public payload says so, a
//! reconnect mid-way (`public_view`, `seat_view`) shows the right look, snapshots at earlier beats show the earlier
//! look, the built-in AI's demigods keep their base images, the profile library records each gear picture with its
//! match, beat and loadout, the recording replays with zero differences, and a later process over the same cache
//! resolves every look without generating anything.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fn7_media::{ImageGenerator, Media, MediaConfig, MockGenerator, Mode};
use fn7_plugin::testkit::Failing;
use fn7_plugin::FinalNight7Plugin;
use fn7_profile::ProfileStore;
use fn7_server::media::MediaPlugin;
use fn7_server::profile::Profiles;
use fn7_server::record::{recorder, Recorded};
use serde_json::{json, Value};
use theatre_plugin_api::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fn7-looks-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn media(dir: &std::path::Path, gen: MockGenerator) -> Media {
    let cfg = MediaConfig { mode: Mode::Mock, dir: dir.join("media"), ..Default::default() };
    Media::start(cfg, Some(Arc::new(gen) as Arc<dyn ImageGenerator>))
}

fn demigod<'a>(view: &'a Value, r: &str) -> &'a Value {
    view["demigods"].as_array().unwrap().iter().find(|d| d["ref"] == r).unwrap_or_else(|| panic!("{r} in the view"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_human_arms_their_demigods_the_look_swaps_in_and_the_recording_replays_clean() {
    let dir = temp_dir("match");
    // Slow enough that an image asked for in a resolve is still rendering when its payload goes out.
    let media_ = media(&dir, MockGenerator { delay: Duration::from_millis(120), ..Default::default() });
    let rec = recorder(dir.join("rec"), theatre_trace::Level::Full).unwrap();
    let store = Arc::new(ProfileStore::open(&dir.join("fn7").join("profiles.sqlite")).unwrap());
    let profiles = Profiles::new(store.clone());
    profiles.attach_recordings(rec.dir.clone(), Some(rec.sink.clone()));
    let p = Recorded::new(
        MediaPlugin::new(FinalNight7Plugin::new().with_media_style(media_.style().ruleset_json()), media_.clone()).with_profiles(profiles.clone()),
        rec.run("looks1"),
    );
    let llm = Failing;
    let seats: Vec<SeatInfo> = [("Chris", false), ("Benito", false), ("Cyra", true), ("Dune", true)]
        .iter().enumerate().map(|(i, (n, bot))| SeatInfo { seat: i as u8, display_name: n.to_string(), is_bot: *bot }).collect();
    let settings = p.validate_settings(&json!({ "seed": 31, "beats_total": 40, "llm": false, "duel_ai_chance": 0.0, "dm_encounters_per_beat": 0,
        "bolt_start": 6, "arena_close_beats": 1000, "look_debounce_beats": 2 })).unwrap();
    let mut state = p.on_match_start(&seats, &settings, &llm).await.unwrap();
    // The clients join (their `match.snapshot` carries the public view): the base portraits and dioramas are made.
    p.public_view(&state);
    assert!(media_.wait_idle(Duration::from_secs(20)).await);
    let mut before: Option<Value> = None; // a state from before any gear
    let mut first: Option<(u32, Value, Value)> = None; // (beat, the pending event, the state after it)
    let mut ready_event: Option<Value> = None;
    let mut all_events: Vec<Value> = vec![];
    for round in 1..=12u32 {
        let start = p.on_round_start(&state, round, &llm).await.unwrap();
        state = start.state.clone();
        // A look announced while its images rendered is announced again, ready, in the next public payload.
        if let Some(l) = start.public["looks"].as_array() {
            for e in l {
                assert_eq!((e["event"].as_str(), e["ready"].as_bool()), (Some("look.changed"), Some(true)), "{e}");
                assert!(e.get("pending").is_none() && e["portrait_image"]["status"] == "ready" && e["diorama_image"]["status"] == "ready", "{e}");
                ready_event.get_or_insert(e.clone());
            }
        }
        let mut actions: Vec<Action> = vec![];
        for s in &seats {
            actions.push(if s.is_bot { p.bot_action(&state, s.seat, &llm).await.unwrap() } else { p.default_action(&state, s.seat) });
        }
        if round == 2 {
            before = Some(state.clone());
        }
        if (3..=5).contains(&round) || round == 8 {
            let text = if round == 8 { "arm my demigods with moonsteel plate armor" } else { "arm my demigods with butterfly wing blades" };
            actions[0] = fn7_plugin::actions::action(0, text, "", None);
        }
        let outcome = p.resolve_round(&state, round, &actions, &RoundContext::default(), &llm, &NoProgress).await.unwrap();
        let committed = p.commit_round(&outcome.state, round, &[], &llm).await.unwrap();
        state = committed.state.clone();
        let evs: Vec<Value> = outcome.public["looks"].as_array().cloned().unwrap_or_default();
        for e in &evs {
            assert_eq!(e["player"], "Chris", "only the human who armed changes look: {e}");
            assert!(e.get("base_portrait").is_none() && e.get("portrait_subject").is_none(), "the inputs are consumed: {e}");
            assert!(outcome.public["dm_events"].as_array().unwrap().iter().any(|d| d == e), "the same event rides in dm_events");
        }
        if first.is_none() {
            if let Some(e) = evs.iter().find(|e| e["image_change"] == true) {
                first = Some((round, e.clone(), state.clone()));
            }
        }
        all_events.extend(evs);
        // The images of this beat arrive before the next one starts.
        assert!(media_.wait_idle(Duration::from_secs(20)).await);
    }
    let (beat, pending, after) = first.expect("an arming changed a look within the first beats");
    let who = pending["demigod"].as_str().unwrap().to_string();

    // At acquisition: the gear images are pending, the payload keeps the previous (base) images.
    let gear_p = pending["pending"]["portrait_image"]["hash"].as_str().expect("pending portrait").to_string();
    let gear_d = pending["pending"]["diorama_image"]["hash"].as_str().expect("pending diorama").to_string();
    assert_eq!(pending["pending"]["portrait_image"]["status"], "pending");
    let base = p.public_view(before.as_ref().unwrap());
    let base_d = demigod(&base, &who);
    assert_eq!(pending["portrait_image"]["hash"], base_d["portrait_image"]["hash"], "the base portrait stands in");
    assert_eq!(pending["portrait_image"]["status"], "ready");
    assert!(pending["gained"]["name"].as_str().unwrap().contains("Blade"), "{pending}");
    assert_eq!(pending["loadout"][0]["slot"], "weapon");
    assert!(pending["pending"]["portrait_image"]["prompt"].as_str().unwrap().contains("gripped in the right hand"));

    // Then ready: announced once more with the same hashes.
    let ready = ready_event.expect("the ready announcement");
    assert_eq!((ready["demigod"].as_str(), ready["portrait_image"]["hash"].as_str(), ready["diorama_image"]["hash"].as_str()),
               (Some(who.as_str()), Some(gear_p.as_str()), Some(gear_d.as_str())));
    assert_eq!(ready["memory"], pending["memory"]);

    // A reconnect mid-way (a snapshot's public and seat views of the state after that beat): the right look, ready.
    let pv = p.public_view(&after);
    let l = &demigod(&pv, &who)["look"];
    assert_eq!((l["portrait_image"]["hash"].as_str(), l["portrait_image"]["status"].as_str(), l["since_beat"].as_u64()),
               (Some(gear_p.as_str()), Some("ready"), Some(beat as u64)), "{l}");
    assert_eq!(l["diorama_image"]["hash"], gear_d.as_str());
    assert!(l.get("pending").is_none() && l["gear"] == true);
    assert_eq!(demigod(&pv, &who)["portrait_image"]["hash"], base_d["portrait_image"]["hash"], "the identity image is still there");
    let sv = p.seat_view(&after, 0);
    let mine = sv["demigods"].as_array().unwrap().iter().find(|d| d["ref"] == who.as_str()).unwrap();
    assert_eq!(mine["look"]["portrait_image"]["hash"], gear_p.as_str());
    // Time travel: the state from before the arming still shows the base look; the final state shows the final one.
    assert_eq!(base_d["look"]["portrait_image"]["hash"], base_d["portrait_image"]["hash"]);
    assert_eq!(base_d["look"]["gear"], false);
    let fin = p.public_view(&state);
    let lf = &demigod(&fin, &who)["look"];
    assert_eq!(lf["portrait_image"]["status"], "ready");
    assert!(all_events.len() >= 2, "blades for both hands at least: {}", all_events.len());
    // The built-in bots' demigods never get a gear picture, whatever they carry.
    for d in fin["demigods"].as_array().unwrap().iter().filter(|d| d["controller"] == "ai") {
        assert_eq!(d["look"]["gear"], false, "{}", d["name"]);
        assert_eq!(d["look"]["portrait_image"]["hash"], d["portrait_image"]["hash"]);
    }
    // Items with a picture carry it everywhere they are listed.
    let it = &l["loadout"][0];
    assert_eq!((it["image"]["kind"].as_str(), it["image"]["status"].as_str()), (Some("item"), Some("ready")), "{it}");
    let peg = demigod(&pv, &who)["story"]["items"].as_array().unwrap().iter().find(|x| x["ref"] == it["item"]).unwrap();
    assert_eq!(peg["image"]["hash"], it["image"]["hash"]);
    let my_item = sv["my_items"].as_array().unwrap().iter().find(|x| x["ref"] == it["item"]).unwrap();
    assert_eq!(my_item["image"]["hash"], it["image"]["hash"]);

    // The library: the gear portrait and diorama, with the match, the beat and the loadout.
    let (_, summary) = (0, p.summarize(&state, &llm).await.unwrap());
    assert!(media_.wait_idle(Duration::from_secs(20)).await);
    tokio::time::sleep(Duration::from_millis(600)).await; // the library watchers
    rec.flush();
    let lib = store.library("chris", 0, Some(100), Some("portrait")).unwrap();
    let g = lib.items.iter().find(|i| i.hash == gear_p).unwrap_or_else(|| panic!("the gear portrait is in the library: {:?}", lib.items.iter().map(|i| &i.caption).collect::<Vec<_>>()));
    assert_eq!(g.subject.as_deref(), Some(who.as_str()));
    assert_eq!((g.looks[0]["match_id"].as_str(), g.looks[0]["beat"].as_u64()), (Some("looks1"), Some(beat as u64)), "{:?}", g.looks);
    assert!(g.looks[0]["loadout"][0].as_str().unwrap().contains("Blade") && g.looks[0]["loadout_hash"].as_str().unwrap().len() == 64);
    assert!(g.caption.contains("portrait with"), "{}", g.caption);
    assert!(store.library("chris", 0, Some(100), Some("diorama")).unwrap().items.iter().any(|i| i.hash == gear_d));
    assert!(store.library("chris", 0, Some(100), Some("item")).unwrap().total >= 1, "the item pictures too (the piles)");
    assert!(summary.summary["players"][0]["demigods"][0]["looks"].is_array());

    // The recording replays with zero differences (the look step is rules only; the summary too).
    let path = dir.join("rec").join("looks1.jsonl");
    let rep = fn7_server::replay::verify(&path, |_| {}).await.unwrap();
    assert!(rep.snapshots > 20 && rep.differences.is_empty(), "{:?}", rep.differences);
    assert_eq!((rep.identical, rep.summary_identical), (rep.snapshots, Some(true)));
    // Every snapshot of the recording, shown again by another process over the same cache (a scrub, a fork, a
    // replay through the server): each look resolves to a file that exists, and nothing is generated (this
    // generator fails whatever it is asked).
    let later = media(&dir, MockGenerator { fail: true, ..Default::default() });
    let viewer = MediaPlugin::new(FinalNight7Plugin::new().with_media_style(later.style().ruleset_json()), later.clone());
    let text = std::fs::read_to_string(&path).unwrap();
    let mut seen_gear = std::collections::BTreeSet::new();
    let mut n = 0;
    for line in text.lines().filter(|l| l.starts_with("{\"type\":\"snapshot\"") && l.contains("\"label\":\"commit_round\"")) {
        let snap: Value = serde_json::from_str(line).unwrap();
        let v = viewer.public_view(&snap["state"]);
        for d in v["demigods"].as_array().unwrap() {
            let l = &d["look"];
            assert_eq!((l["portrait_image"]["status"].as_str(), l["diorama_image"]["status"].as_str()), (Some("ready"), Some("ready")), "{l}");
            assert!(l.get("pending").is_none());
            if l["gear"] == true {
                seen_gear.insert(l["portrait_image"]["hash"].as_str().unwrap().to_string());
            }
        }
        n += 1;
    }
    assert!(n >= 10 && seen_gear.contains(&gear_p), "{n} snapshots, {} gear looks", seen_gear.len());
    assert!(later.wait_idle(Duration::from_secs(5)).await);
    let not_ready: Vec<String> = later.manifest(0)["items"].as_array().unwrap().iter().filter(|e| e["status"] != "ready")
        // (An omen's moment card is not a look: a fresh process asks for those under its interim rule.)
        .filter(|e| matches!(e["kind"].as_str(), Some("portrait" | "diorama" | "item")))
        .map(|e| format!("{} {}: {}", e["kind"], e["status"], e["prompt"].as_str().unwrap_or("").chars().take(160).collect::<String>())).collect();
    assert!(not_ready.is_empty(), "nothing was generated, refused or failed: {not_ready:#?}");
    let _ = std::fs::remove_dir_all(dir);
}
