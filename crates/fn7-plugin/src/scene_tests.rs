//! Token play (state schema 19): a typed will played out by the tokens on the people-level board.

use std::sync::Mutex;

use fn7_model::dm::{DiceRoll, DiceVerdict, Scope};
use fn7_model::hexgrid::{cell_at, HexCell, HexLevel};
use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::duel_tests::{bot_actions, table, Table};
use crate::scene::{self, Scene, MAX_STEPS, SCENE_MAX_MS, VERBS, MOODS, CAPTION_MAX};
use crate::*;

const ORDER: &str = "I go into the house to my left and seduce the maiden";
/// The same scene through the world resolver and its d20 ("seduce" is a society word: a sweet-talk, by rules).
const FLIRT: &str = "I go into the house to my left and flirt with the maiden";

/// The HAND reads the order as one world act; the GAME MASTER (when `gm` is set) answers with the ruling the test
/// wants: a relation shift from the demigod to the maiden, by their labels in its prompt.
struct Scripted {
    verb: &'static str,
    gm: Option<&'static str>,
    prompts: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl LlmProvider for Scripted {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        if req.system.contains("HAND of a demigod") {
            let r = json!({ "said": "Going in.", "intents": [{ "verb": self.verb, "targets": ["the maiden"], "how": "in the house to my left",
                                                                "magnitude": 3, "route": "world", "arg": "" }] });
            return Ok(LlmResponse { choices: vec![r.to_string()], cached: false });
        }
        if req.system.contains("GAME MASTER") {
            let user = req.messages.first().map(|m| m.1.clone()).unwrap_or_default();
            self.prompts.lock().unwrap().push(user.clone());
            if let Some(text) = self.gm {
                // Labels: "<label>: <name> ..." lines; the actor is the demigod, the target the person it addressed.
                let label_of = |needle: &str| -> Option<String> {
                    user.lines().find(|l| l.contains(needle)).and_then(|l| l.trim().trim_start_matches("- ").split(':').next()).map(|s| s.trim().to_string())
                };
                if let (Some(a), Some(t)) = (label_of("Hand"), label_of("maiden").or_else(|| label_of("the one"))) {
                    let r = json!({ "effects": [{ "kind": "relation", "actor": a, "target": t, "amount": 0.3, "feel": "trust",
                                                  "text": text, "act": 1, "magnitude": 3, "valence": 2, "witnesses": [] }] });
                    return Ok(LlmResponse { choices: vec![r.to_string()], cached: false });
                }
            }
        }
        anyhow::bail!("offline")
    }
}

/// A house on `cell` in the place × time memory (an observation of the hex with one building on it).
fn house_on(w: &mut World, cell: &HexCell, beat: u32) {
    let key = cell.key();
    let wh = crate::dm::locate(w, cell.center(), HexLevel::People);
    let (mut state, _, _) = crate::dm::cell_state(w, &key, beat).unwrap();
    state["buildings"] = 1.into();
    crate::dm::remember(w, beat, &wh, crate::dm::Decision { scope: Scope::Cell, kind: "place.observed", query: Some(format!("observed:{key}:{beat}")),
                                                          public: true, text: String::new(), data: state });
}

fn left_of(c: &HexCell) -> HexCell {
    HexCell { q: c.q - 1, ..*c }
}

/// The table with a house on the left of seat 0's Left demigod (and nowhere else near it).
fn maiden_table(seed: u64) -> (Table, HexCell, HexCell) {
    let t = table(seed, json!({ "llm": true, "wills_enabled": false }));
    let mut w = world::load(&t.state).unwrap();
    let at = cell_at(w.demigods[t.me.ix()].pos, HexLevel::People);
    let house = left_of(&at);
    house_on(&mut w, &house, 1);
    (Table { state: world::save(&w), ..t }, at, house)
}

async fn resolve(p: &FinalNight7Plugin, state: &Value, llm: &dyn LlmProvider, order: &str) -> RoundOutcome {
    let mut actions = bot_actions(p, state);
    actions[0] = actions::action(0, order, "", None);
    p.resolve_round(state, 2, &actions, &RoundContext::default(), llm, &NoProgress).await.unwrap()
}

fn mine(o: &RoundOutcome, me: DemigodId) -> Scene {
    let v = o.public["scenes"].as_array().expect("scenes").iter().find(|s| s["demigod"] == Ref::Demigod(me).to_string()).expect("my scene").clone();
    assert_eq!(v["event"], "scene.play");
    serde_json::from_value(v).unwrap()
}

fn check_shape(s: &Scene, beat_ms: u32) {
    assert!(!s.steps.is_empty() && s.steps.len() <= MAX_STEPS, "{} steps", s.steps.len());
    assert!(s.total_ms <= SCENE_MAX_MS && s.total_ms > 0, "total {}", s.total_ms);
    let _ = beat_ms;
    let mut t = 0;
    for st in &s.steps {
        assert_eq!(st.t_ms, t, "steps are laid end to end");
        t += st.dur_ms;
        assert!(VERBS.contains(&st.verb.as_str()), "verb {}", st.verb);
        assert!(MOODS.contains(&st.mood.as_str()), "mood {}", st.mood);
        assert!(st.caption.chars().count() <= CAPTION_MAX && !st.caption.is_empty());
        assert!(s.actors.iter().any(|a| a.id == st.actor));
        if let Some(c) = &st.to_cell {
            assert!(HexCell::parse(c).is_some_and(|c| c.level == HexLevel::People), "people-level cells");
        }
    }
    assert_eq!(t, s.total_ms);
    assert_eq!(s.actors[0].kind, "demigod");
    assert_eq!(s.place.level, "people");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn i_go_into_the_house_to_my_left_and_seduce_the_maiden() {
    let (t, at, house) = maiden_table(41);
    let gm = Scripted { verb: "seduce", gm: Some("the maiden laughs and lets the stranger in"), prompts: Mutex::default() };
    let o = resolve(&FinalNight7Plugin::new(), &t.state, &gm, ORDER).await;
    let w = world::load(&o.state).unwrap();
    let s = mine(&o, t.me);
    println!("SCENE {}", serde_json::to_string_pretty(&s.json()).unwrap());
    check_shape(&s, 6000);
    // The stage: the demigod's cell; the house on its left from the place × time memory.
    assert_eq!(s.place.cell, at.key());
    let b = s.place.building.as_ref().expect("a house");
    assert_eq!((b.cell.as_str(), b.side.as_str(), b.source.as_str()), (house.key().as_str(), "left", "observed"));
    // Walk to the adjacent house, enter, talk with a named, remembered person.
    assert_eq!((s.steps[0].verb.as_str(), s.steps[0].to_cell.as_deref()), ("walk", Some(house.key().as_str())));
    assert_eq!(s.steps[0].path, vec![house.key()], "one cell: it is adjacent");
    assert_eq!((s.steps[1].verb.as_str(), s.steps[1].to_cell.as_deref()), ("enter", Some(house.key().as_str())));
    assert_eq!((s.steps[2].verb.as_str(), s.steps[2].actor.as_str(), s.steps[2].target_actor.as_deref(), s.steps[2].mood.as_str()),
               ("talk", "a0", Some("a1"), "love"));
    let maiden = &s.actors[1];
    assert_eq!((maiden.kind.as_str(), maiden.start_cell.as_str()), ("person", house.key().as_str()), "she is inside");
    let Some(Ref::Char(c)) = maiden.who else { panic!("a person ref: {maiden:?}") };
    assert_eq!(w.characters[c.ix()].name, maiden.name, "named");
    assert!(!maiden.name.is_empty());
    // The outcome is the ruling's. "Seduce" is one of the society's words: the rules sweet-talk her (an allegiance
    // row, no die), and the scene shows exactly that.
    let intent = w.intents.iter().find(|i| i.beat == 2 && i.demigod == t.me && i.verb == "seduce").expect("the intent");
    assert!(w.dm.decided(&format!("act:{}", intent.id.0)).is_none() && s.dice.is_none(), "no die was rolled, none is shown");
    let rows: Vec<&LedgerRow> = w.ledger.rows.iter().filter(|r| r.intent == Some(intent.id)).collect();
    let row = rows.iter().find(|r| r.verb == "allegiance").expect("the sweet-talk");
    assert_eq!(s.outcome.text, "Otavio Pires falls into Ash's Left Hand's arms".replace("Otavio Pires", &maiden.name), "the shown step (the row itself is private)");
    assert!(!crate::payload::is_public_event(&w, row));
    assert_eq!((s.outcome.result.as_str(), s.outcome.verb.as_str()), ("success", "embrace"));
    assert!(row.targets.contains(&Ref::Char(c)), "she is the one the ruling names");
    // Hidden stays hidden: the order's words are nowhere in the public scene.
    let text = s.json().to_string();
    for w in ["my left", "seduce the maiden", "in the house to my left"] {
        assert!(!text.contains(w), "{w:?} leaked");
    }
    // The state keeps it; present shows it.
    assert!(scene::scenes_of(&w, 2).contains(&s));
    // Deterministic: the same inputs give the same scene and the same state.
    let o2 = resolve(&FinalNight7Plugin::new(), &t.state, &gm, ORDER).await;
    assert_eq!(o2.public["scenes"], o.public["scenes"]);
    assert_eq!(o2.state, o.state);
}

/// Every d20 face the maiden can answer: success embraces, a fail half-lands, a natural 1 slaps and runs out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_maiden_answers_by_the_dice() {
    let mut seen = std::collections::BTreeSet::new();
    for seed in 40..64 {
        let (t, _, house) = maiden_table(seed);
        let o = resolve(&FinalNight7Plugin::new(), &t.state, &Scripted { verb: "flirt", gm: None, prompts: Mutex::default() }, FLIRT).await;
        let s = mine(&o, t.me);
        check_shape(&s, 6000);
        assert_eq!(s.place.building.as_ref().map(|b| b.cell.clone()), Some(house.key()));
        let w = world::load(&o.state).unwrap();
        let intent = w.intents.iter().find(|i| i.beat == 2 && i.demigod == t.me && i.verb == "flirt").expect("the intent");
        let roll: DiceRoll = serde_json::from_value(w.dm.decided(&format!("act:{}", intent.id.0)).expect("rolled").data.clone()).unwrap();
        let d = s.dice.as_ref().expect("the d20 rides along");
        assert_eq!((d.roll, d.total, d.target_dc, d.verdict.as_str()), (roll.roll, roll.total, roll.target_dc, roll.verdict.as_str()));
        let talk = s.steps.iter().position(|x| x.verb == "talk").expect("the charm");
        assert_eq!(d.at_ms, s.steps[talk].t_ms + 200, "it tumbles as she is charmed");
        if s.outcome.result != "twisted" {
            // A fail lands half (partial) unless the ruling made nothing of it.
            let expect: &[&str] = match roll.verdict {
                DiceVerdict::Success | DiceVerdict::CritSuccess => &["success"],
                DiceVerdict::Fail => &["partial", "failed"],
                DiceVerdict::CritFail => &["failed"],
            };
            assert!(expect.contains(&s.outcome.result.as_str()), "{:?} {:?}", roll.verdict, s.outcome);
        }
        if s.outcome.result == "failed" {
            let v: Vec<&str> = s.steps.iter().map(|x| x.verb.as_str()).collect();
            assert!(v.contains(&"slap") && v.contains(&"flee"), "a slap and she runs out: {v:?}");
        }
        if s.outcome.result == "success" {
            assert_eq!(s.outcome.verb, "embrace");
            assert_eq!(s.steps.last().unwrap().verb, "exit", "and back out");
        }
        seen.insert(s.outcome.result.clone());
    }
    assert!(seen.len() >= 2, "{seen:?}");
}

// ---------------------------------------------------------------------------------------------------------------------
// Unit scenes: a hand intent, its rows and its die, built by hand
// ---------------------------------------------------------------------------------------------------------------------

struct Unit {
    w: World,
    d: DemigodId,
    row_start: usize,
    pos: Vec<LonLat>,
}

fn unit(seed: u64) -> Unit {
    let mut w = crate::testkit::world(4, seed);
    w.meta.beat = 3;
    let d = w.demigods[0].id;
    let row_start = w.ledger.rows.len();
    let pos = w.demigods.iter().map(|d| d.pos).collect();
    let _ = &mut w;
    Unit { w, d, row_start, pos }
}

impl Unit {
    fn intent(&mut self, verb: &str, how: &str, targets: Vec<Ref>, route: Route) -> IntentId {
        let id = IntentId::from_ix(self.w.intents.len());
        let agent = self.w.demigods[self.d.ix()].hands[0];
        self.w.intents.push(Intent { id, beat: 3, agent, demigod: self.d, verb: verb.into(), targets: targets.into_iter().map(Target::Known).collect(),
                                     how: how.into(), place: None, magnitude: 4, span: format!("secret plan: {verb} {how}"), route });
        id
    }
    fn row(&mut self, intent: IntentId, actor: Ref, verb: &str, act: &str, targets: Vec<Ref>) {
        let id = self.w.ledger.next_id();
        self.w.ledger.rows.push(LedgerRow { id, beat: 3, source: Source::Resolver, actor, verb: verb.into(), act: act.into(), targets, place: None,
                                            magnitude: 4, valence: 0, witnessed_by: vec![], known_by: Default::default(), known_at: vec![],
                                            told_to: vec![], erased_from: vec![], debt: None, cause: None, intent: Some(intent), at: None });
    }
    fn die(&mut self, intent: IntentId, roll: u8, verdict: DiceVerdict) {
        let r = DiceRoll { roll, modifier: 0, total: roll as i32, target_dc: 10, verdict, confirm: 0.5 };
        let pos = self.w.demigods[self.d.ix()].pos;
        crate::dm::record_roll(&mut self.w, 3, &format!("act:{}", intent.0), Ref::Demigod(self.d), &r, "a deed", pos);
    }
    fn build(&mut self) -> Vec<Scene> {
        let st = world::settings(&self.w);
        let pantheon = fn7_pantheon::load();
        scene::build(&mut self.w, 3, self.row_start, &self.pos, &pantheon, &st)
    }
    /// A person standing `k` cells to the right of the demigod.
    fn person(&mut self, k: i64) -> Ref {
        let at = cell_at(self.w.demigods[self.d.ix()].pos, HexLevel::People);
        let c = self.w.characters.iter().position(|c| c.alive).expect("a person");
        self.w.characters[c].pos = HexCell { q: at.q + k, ..at }.center();
        self.w.characters[c].place = self.w.demigods[self.d.ix()].place;
        Ref::Char(self.w.characters[c].id)
    }
}

#[test]
fn a_fight_walks_up_strikes_and_drops_the_foe() {
    let mut u = unit(5);
    let foe = u.person(3);
    let i = u.intent("punch", "", vec![foe], Route::World);
    u.row(i, Ref::Demigod(u.d), "damage", "Ash's Left Hand lays the dock boss out cold", vec![foe]);
    u.die(i, 17, DiceVerdict::Success);
    let s = u.build().pop().expect("a scene");
    println!("FIGHT {}", s.json());
    check_shape(&s, 6000);
    let v: Vec<&str> = s.steps.iter().map(|x| x.verb.as_str()).collect();
    assert_eq!(v, ["walk", "fight", "fall"]);
    assert_eq!(s.steps[0].path.len(), 2, "up to the cell next to the foe");
    assert_eq!(s.actors[1].start_cell, {
        let at = cell_at(u.pos[u.d.ix()], HexLevel::People);
        HexCell { q: at.q + 3, ..at }.key()
    }, "the foe stands where it really is");
    assert_eq!((s.outcome.result.as_str(), s.outcome.verb.as_str()), ("success", "fall"));
    assert_eq!(s.outcome.text, s.steps[2].caption, "a private row's words stay private: the shown step speaks");
    assert!(s.place.building.is_none(), "a street fight stays outdoors");
}

#[test]
fn a_failed_search_finds_nothing() {
    let mut u = unit(6);
    let i = u.intent("search", "the ruins", vec![], Route::World);
    u.row(i, Ref::Demigod(u.d), "note", "Ash tries to search the ruins, and it comes to nothing", vec![]);
    u.die(i, 4, DiceVerdict::Fail);
    let s = u.build().pop().expect("a scene");
    check_shape(&s, 6000);
    let last = s.steps.iter().rev().find(|x| x.verb != "exit").unwrap();
    assert_eq!((last.verb.as_str(), last.caption.ends_with("finds nothing")), ("search", true), "{s:?}");
    assert_eq!((s.outcome.result.as_str(), s.outcome.verb.as_str()), ("failed", "search"));
    assert_eq!(s.dice.as_ref().map(|d| d.verdict.as_str()), Some("fail"));
    assert!(!s.json().to_string().contains("secret plan"), "the order's words stay private");
}

#[test]
fn an_accident_twists_it() {
    let mut u = unit(7);
    let p = u.person(2);
    let i = u.intent("ask", "", vec![p], Route::World);
    u.row(i, Ref::Demigod(u.d), "relation", "Ash chats", vec![p]);
    let place = Ref::Place(u.w.demigods[u.d.ix()].place);
    u.row(i, place, "damage", "a Final Destination moment: a loose sign", vec![Ref::Demigod(u.d)]);
    u.die(i, 12, DiceVerdict::Success);
    let s = u.build().pop().unwrap();
    assert_eq!(s.outcome.result, "twisted");
    assert_eq!((s.steps.last().unwrap().verb.as_str(), s.steps.last().unwrap().actor.as_str()), ("fall", "a0"));
}

#[test]
fn hidden_acts_make_no_scene_and_old_schemas_none_at_all() {
    // A subagent's act (a spy) shows nothing.
    let mut u = unit(8);
    let i = u.intent("spy", "", vec![], Route::World);
    let parent = u.w.demigods[u.d.ix()].hands[0];
    let sub = crate::beat::spawn_agent(&mut u.w, AgentKind::Subagent { parent }, "spy".into(), "watch".into(), vec![], Some(3), 3);
    u.w.intents[i.ix()].agent = sub;
    u.row(i, Ref::Demigod(u.d), "note", "someone watches", vec![]);
    u.die(i, 15, DiceVerdict::Success);
    assert!(u.build().is_empty());
    // Before schema 19: nothing built, nothing kept.
    let mut u = unit(9);
    u.w.meta.schema = 17;
    let i = u.intent("punch", "", vec![], Route::World);
    u.row(i, Ref::Demigod(u.d), "note", "a swing", vec![]);
    let before = world::save(&u.w);
    assert!(u.build().is_empty());
    assert_eq!(world::save(&u.w), before, "byte for byte");
}

#[test]
fn the_caps_hold() {
    let mut steps: Vec<scene::Step> = (0..MAX_STEPS)
        .map(|k| scene::Step { t_ms: 0, dur_ms: 4000, actor: "a0".into(), verb: "walk".into(), to_cell: None, path: vec![], target_actor: None,
                               caption: format!("step {k}"), mood: "neutral".into() })
        .collect();
    let total = scene::fit(&mut steps);
    assert!(total <= SCENE_MAX_MS, "{total}");
    assert_eq!(steps.last().map(|s| s.t_ms + s.dur_ms), Some(total));
    assert_eq!(scene::clip(&"x".repeat(200), CAPTION_MAX).chars().count(), CAPTION_MAX);
    // Every act, every result: within the caps.
    for (k, verb) in ["seduce", "talk", "threaten", "give", "steal", "punch", "curse", "search", "hide", "flee", "pray", "dance", "go"].iter().enumerate() {
        for (roll, v) in [(20, DiceVerdict::CritSuccess), (11, DiceVerdict::Success), (5, DiceVerdict::Fail), (1, DiceVerdict::CritFail)] {
            let mut u = unit(20 + k as u64);
            let p = u.person(2);
            let i = u.intent(verb, "into the house", vec![p], Route::World);
            u.row(i, Ref::Demigod(u.d), "relation", "something happens", vec![p]);
            u.die(i, roll, v);
            let s = u.build().pop().unwrap_or_else(|| panic!("{verb}"));
            check_shape(&s, 6000);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The caption polish: journaled, applied at the next resolve, fed back on replay
// ---------------------------------------------------------------------------------------------------------------------

/// Answers the polish prompt with one upper-cased caption per step (and counts its calls).
#[derive(Default)]
struct Polish {
    calls: std::sync::atomic::AtomicU64,
}

#[async_trait::async_trait]
impl LlmProvider for Polish {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(req.model.as_deref(), Some(fn7_agents::scene_polish::MODEL));
        let user = &req.messages[0].1;
        let n: usize = user.split("exactly ").nth(1).and_then(|x| x.split_whitespace().next()).and_then(|x| x.parse().ok()).unwrap();
        let caps: Vec<String> = (0..n).map(|k| format!("POLISHED STEP {}", k + 1)).collect();
        Ok(LlmResponse { choices: vec![json!({ "captions": caps }).to_string()], cached: false })
    }
}

async fn next_round(p: &FinalNight7Plugin, o: &RoundOutcome, round: u32) -> Value {
    let s = p.commit_round(&o.state, round, &[], &crate::testkit::Failing).await.unwrap().state;
    p.on_round_start(&s, round + 1, &crate::testkit::Failing).await.unwrap().state
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn polished_captions_are_a_recorded_side_input() {
    let llm = Scripted { verb: "seduce", gm: None, prompts: Mutex::default() };
    // Off by default: nothing is asked.
    let (t, _, _) = maiden_table(41);
    let polish = std::sync::Arc::new(Polish::default());
    let p = FinalNight7Plugin::new().with_scene_polish(polish.clone());
    let o = resolve(&p, &t.state, &llm, ORDER).await;
    assert!(!o.public["scenes"].as_array().unwrap().is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(polish.calls.load(std::sync::atomic::Ordering::Relaxed), 0, "token_play_llm is off by default");

    // On: the human seat's scene is polished outside the hooks and journaled.
    let mut w = world::load(&t.state).unwrap();
    w.meta.settings["token_play_llm"] = true.into();
    let state = world::save(&w);
    let o = resolve(&p, &state, &llm, ORDER).await;
    let mine = |v: &Value| v.as_array().unwrap().iter().filter(|s| s["seat"] == 0).count();
    assert!(mine(&o.public["scenes"]) >= 1);
    let s3 = next_round(&p, &o, 2).await;
    let w3 = world::load(&s3).unwrap();
    let mut recs = vec![];
    for _ in 0..100 {
        recs = p.journal().captions(&w3);
        if !recs.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(!recs.is_empty(), "the polish came back");
    assert!(recs.iter().all(|r| r.scene_id.starts_with("sc:2:")));
    // Only human seats are polished (seat 0's two bodies at most).
    assert!(polish.calls.load(std::sync::atomic::Ordering::Relaxed) <= 2);
    // `present` shows them at once.
    let present = p.present_for_tests(&w3);
    assert!(present["scenes"].to_string().contains("POLISHED STEP 1"), "{}", present["scenes"]);
    // The next resolve applies them and says so.
    let acts = bot_actions(&p, &s3);
    let o3 = p.resolve_round(&s3, 3, &acts, &RoundContext::default(), &crate::testkit::Failing, &NoProgress).await.unwrap();
    let ev = &o3.public["scene_captions"][0];
    assert_eq!((ev["event"].as_str(), ev["captions"][0].as_str()), (Some("scene.captions"), Some("POLISHED STEP 1")));
    let after = world::load(&o3.state).unwrap();
    let kept = scene::scenes_of(&after, 2);
    let polished = kept.iter().find(|s| s.scene_id == ev["scene_id"].as_str().unwrap()).unwrap();
    assert!(polished.polished && polished.steps[0].caption == "POLISHED STEP 1");
    // A replay is fed the recorded records and never asks a model: the same state.
    let mut feed = present::Feed::default();
    feed.captions.insert(3, recs.clone());
    let replay = FinalNight7Plugin::new().with_feed(feed);
    let r3 = replay.resolve_round(&s3, 3, &acts, &RoundContext::default(), &crate::testkit::Failing, &NoProgress).await.unwrap();
    assert_eq!(r3.state, o3.state, "replayed exactly");
    // Applied once: a polished scene stays as it is.
    let mut again = after.clone();
    assert!(scene::apply_captions(&mut again, &polished.scene_id, &vec!["x".to_string(); polished.steps.len()]).is_none());
    // A client can never send one.
    assert!(p.validate_action(&s3, 0, "", &json!({ "scene_captions": recs[0] }), SubmitKind::Instant).is_err());
}
