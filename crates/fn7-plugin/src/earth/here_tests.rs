//! The earth underfoot (state schema 17): every prompt that acts in a place carries what the field holds there now;
//! calm ground adds nothing; a schema-16 state's prompts are byte for byte what they were.

use std::sync::Mutex;

use async_trait::async_trait;
use fn7_model::earth::EarthInput;
use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::*;

use super::grid::grid;
use super::here::{Ground, MAX_CLAUSES};
use super::rules;
use super::sim::*;
use super::sim::Cell;
use crate::world::{load, save, settings};
use crate::FinalNight7Plugin;

/// Paint the field at `at` (each distinct cell once) with `f`, logging an input of `kind` at `beat` on each.
pub(crate) fn paint(w: &mut World, at: &[LonLat], kind: Option<(u8, u32)>, f: impl Fn(&mut Cell)) -> Vec<u16> {
    let st = settings(w);
    let mut sim = rules::sim_of(w, &st).unwrap_or_else(|| Sim::new(w.meta.seed, rules::cfg(&st)));
    let g = grid();
    let mut cells = vec![];
    for p in at {
        let i = g.index_of(*p);
        if !cells.contains(&i) {
            f(sim.m(i));
            cells.push(i);
        }
    }
    sim.normalize();
    let s = w.dm.earth.get_or_insert_with(Default::default);
    sim.to_state(s);
    if let Some((k, b)) = kind {
        for &i in &cells {
            s.inputs.push(EarthInput { beat: b, kind: k, cell: i, radius: 1, energy: 5, ..Default::default() });
        }
    }
    cells
}

fn ground_at(beat: u32, inputs: Vec<EarthInput>, i: u16, f: impl Fn(&mut Cell)) -> Ground {
    let st = crate::Settings::from_value(&json!({ "seed": 3 }));
    let mut sim = Sim::new(3, rules::cfg(&st));
    sim.beat = beat;
    f(sim.m(i));
    sim.normalize();
    Ground { sim, inputs }
}

/// A land cell well inland (Madrid).
fn land() -> u16 {
    let g = grid();
    let i = g.index_of([-3.7, 40.4]);
    assert!(g.land(i));
    i
}

fn input(beat: u32, kind: u8, cell: u16) -> EarthInput {
    EarthInput { beat, kind, cell, radius: 1, energy: 5, ..Default::default() }
}

#[test]
fn the_ground_reads_as_one_short_factual_line() {
    let i = land();
    // Floodwater, and the flood that brought it (the field is at the end of beat 4: "now" is beat 5).
    let g = ground_at(4, vec![input(3, FLOOD, i)], i, |c| c.water = 120);
    assert_eq!(g.describe(i).as_deref(), Some("waist-deep floodwater, the flood came 2 beats ago"));
    // Cracked and shaking after a quake.
    let g = ground_at(4, vec![input(4, EARTHQUAKE, i)], i, |c| {
        c.crack = 60;
        c.shake = 40;
    });
    assert_eq!(g.describe(i).as_deref(), Some("the ground is cracked and still shaking after the quake last beat"));
    // Cooling lava under smoke.
    let g = ground_at(4, vec![], i, |c| {
        c.lava = 20;
        c.plume = 40;
    });
    assert_eq!(g.describe(i).as_deref(), Some("cooling black lava, still glowing, smoke"));
    // Risen land, since the first lift.
    let g = ground_at(14, vec![input(12, RAISE_LAND, i), input(13, RAISE_LAND, i)], i, |c| c.dh = 3);
    assert_eq!(g.describe(i).as_deref(), Some("the land here rose 3 levels since beat 12"));
    // Snowed in.
    let g = ground_at(4, vec![], i, |c| {
        c.snow = 200;
        c.heat = -40;
    });
    assert_eq!(g.describe(i).as_deref(), Some("snowed in, drifts deep, bitter cold"));
    // Worst first, and never more than three clauses.
    let g = ground_at(4, vec![], i, |c| {
        c.lava = 90;
        c.fire = 200;
        c.water = 100;
        c.crack = 200;
        c.snow = 100;
        c.ruin = 200;
        c.cloud = 200;
        c.charge = 120;
    });
    let d = g.describe(i).unwrap();
    assert_eq!(d.split("; ").count(), MAX_CLAUSES, "{d}");
    assert!(d.starts_with("molten lava running, steam hissing off it; a wildfire roaring through"), "{d}");
    // A meeting by the field's own rules: water on lava.
    assert_eq!(g.meets(FLOOD, i), Some("water on this lava will flash to steam and harden it to rock"));
    // Calm ground says nothing: base, or below every threshold.
    let g = ground_at(4, vec![input(3, FLOOD, i)], i, |c| {
        c.water = 10;
        c.cloud = 30;
        c.heat = 5;
        c.stress = 40;
    });
    assert_eq!(g.describe(i), None);
    assert_eq!(g.describe(grid().index_of([2.35, 48.85])), None, "an untouched cell is base");
    assert!(g.lines_for(&[("Ash".into(), [-3.7, 40.4])]).is_empty());
    assert!(g.stage([-3.7, 40.4]).is_none());
}

#[test]
fn lines_name_the_place_or_who_stands_there() {
    let i = land();
    let g = ground_at(4, vec![], i, |c| c.water = 120);
    let madrid = [-3.7, 40.4];
    let paris = [2.35, 48.85];
    assert_eq!(g.lines_for(&[("Ash".into(), madrid)]), vec!["The earth here, now: waist-deep floodwater.".to_string()]);
    assert_eq!(g.lines_for(&[("Ash".into(), madrid), ("Brix".into(), paris)]),
               vec!["The earth where Ash stands, now: waist-deep floodwater.".to_string()], "a calm cell adds no line");
    assert_eq!(g.lines_under(&[("Ash".into(), madrid)]), vec!["The earth under Ash, now: waist-deep floodwater.".to_string()]);
    let s = g.stage(madrid).unwrap();
    assert_eq!((s.floor, s.slippery, s.hazards.clone(), s.props.clone()), ("flooded", true, vec!["flood"], vec!["floodwater", "floodwater"]));
}

#[test]
fn the_gate_is_schema_17_and_the_simulator() {
    let mut w = crate::testkit::world(4, 5);
    assert_eq!(w.meta.schema, 17);
    let at: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
    paint(&mut w, &at, None, |c| c.fire = 150);
    let st = settings(&w);
    assert!(Ground::of(&w, &st).is_some());
    let who: Vec<(String, LonLat)> = w.demigods.iter().map(|d| (d.name.clone(), d.pos)).collect();
    assert!(!super::here::lines(&w, &st, &who).is_empty());
    w.meta.schema = 16;
    assert!(Ground::of(&w, &st).is_none(), "a schema-16 state reads no ground");
    assert!(super::here::lines(&w, &st, &who).is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------
// In a match: every prompt
// ---------------------------------------------------------------------------------------------------------------------

/// Records every request. The hands always act in the world, invoke Nature and send a scout; the rest answer as
/// [`crate::testkit::Scripted`] does.
#[derive(Default)]
struct Rec {
    seen: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl LlmProvider for Rec {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        let user = req.messages.first().map(|m| m.1.clone()).unwrap_or_default();
        self.seen.lock().unwrap().push((req.system.clone(), user.clone()));
        let v = if req.system.contains("HAND of a demigod") {
            json!({ "said": "On it.", "intents": [
                { "verb": "punch", "targets": [], "how": "hard", "magnitude": 5, "route": "world", "arg": "" },
                { "verb": "storm", "targets": [], "how": "", "magnitude": 6, "route": "god", "arg": "nature" },
                { "verb": "scout", "targets": [], "how": "", "magnitude": 3, "route": "spawn", "arg": "watch the ground for 3 beats" } ] })
        } else {
            match crate::testkit::scripted_reply(&req, fn7_model::seed::hash_str(&user)) {
                Some(v) => v,
                None => anyhow::bail!("unscripted"),
            }
        };
        Ok(LlmResponse { choices: vec![v.to_string()], cached: false })
    }
}

fn humans() -> Vec<SeatInfo> {
    (0..4).map(|i| SeatInfo { seat: i, display_name: ["Ash", "Brix", "Cyra", "Dune"][i as usize].into(), is_bot: false }).collect()
}

/// Beat 1 with orders, then (`flood`) floodwater under every demigod, then beat 2 with orders and a volcano cast on
/// seat 0's demigod: beat 2's requests, sorted.
async fn beat_two(flood: bool, schema: Option<u32>) -> Vec<(String, String)> {
    let p = FinalNight7Plugin::new();
    let settings = p
        .validate_settings(&json!({ "seed": 23, "beats_total": 30, "clusters": 40, "bolt_start": 40, "accident_scale": 0.0,
                                     "dm_encounters_per_beat": 0, "duel_ai_chance": 0.0 }))
        .unwrap();
    let r1 = Rec::default();
    let mut state = p.on_match_start(&humans(), &settings, &r1).await.unwrap();
    if let Some(s) = schema {
        let mut w = load(&state).unwrap();
        w.meta.schema = s;
        state = save(&w);
    }
    let orders = |round: u32| -> Vec<Action> {
        (0..4u8).map(|s| crate::actions::action(s, &format!("punch the nearest rival, beat {round}"), "", None)).collect()
    };
    state = p.on_round_start(&state, 1, &r1).await.unwrap().state;
    let o = p.resolve_round(&state, 1, &orders(1), &RoundContext::default(), &r1, &NoProgress).await.unwrap();
    state = p.commit_round(&o.state, 1, &[], &r1).await.unwrap().state;
    let mut w = load(&state).unwrap();
    for d in w.demigods.iter_mut() {
        d.health = d.max_health.max(5.0);
        d.alive = true;
    }
    if flood {
        let at: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
        paint(&mut w, &at, Some((FLOOD, 1)), |c| c.water = 120);
    }
    let target = w.bodies_of_seat(0).next().unwrap().pos;
    state = save(&w);
    let r2 = Rec::default();
    state = p.on_round_start(&state, 2, &r2).await.unwrap().state;
    let mut acts = orders(2);
    acts.push(Action { instant: true, seq: 1, received_at_ms: 1_000,
                       fields: json!({ "cast": { "power": "volcano", "target": target, "tier": "mega" } }), ..Action::new(1, "") });
    let o = p.resolve_round(&state, 2, &acts, &RoundContext::default(), &r2, &NoProgress).await.unwrap();
    assert!(o.public["casts"].as_array().is_some_and(|c| !c.is_empty()), "the cast landed: {}", o.public["casts"]);
    let mut seen = r2.seen.into_inner().unwrap();
    seen.sort();
    seen
}

fn of<'a>(seen: &'a [(String, String)], sys: &str) -> Vec<&'a String> {
    seen.iter().filter(|(s, _)| s.contains(sys)).map(|x| &x.1).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn every_prompt_that_acts_in_a_flooded_place_says_so() {
    let wet = beat_two(true, None).await;
    let calm = beat_two(false, None).await;
    let kinds = [
        ("HAND of a demigod", "The earth here, now: waist-deep floodwater, the flood came last beat."),
        ("GAME MASTER", "waist-deep floodwater, the flood came last beat."),
        ("a god of FINAL NIGHT 7", "waist-deep floodwater"),
        ("You are a SUBAGENT", "The earth here, now: waist-deep floodwater"),
        ("You are ENTROPY", "'s Volcano lands, the earth now: waist-deep floodwater, the flood came last beat; it meets: lava into this water will boil it to steam and make new rock."),
        ("NARRATOR of FINAL NIGHT 7", "The earth under "),
    ];
    for (sys, want) in kinds {
        let got = of(&wet, sys);
        assert!(!got.is_empty(), "a {sys} prompt was asked");
        assert!(got.iter().any(|u| u.contains(want)), "{sys} carries {want:?}:\n{}", got[0]);
        let quiet = of(&calm, sys);
        assert!(!quiet.is_empty() && quiet.iter().all(|u| !u.contains("The earth") && !u.contains("the earth now")), "calm: no line in {sys}");
    }
    // A god's ruling: from the invoker's own ground.
    let god: Vec<&String> = of(&wet, "a god of FINAL NIGHT 7").into_iter().filter(|u| u.contains("The invocation")).collect();
    // (Seed 23: these four stand on land; the other four on coasts whose regions cell is sea, where standing water is
    // just the sea: no line.)
    let on_land = ["Ash's Left Hand", "Brix's Left Hand", "Dune's Left Hand", "Dune's Right Hand"];
    for u in &god {
        let land = on_land.iter().any(|n| u.contains(&format!("Invoker: P1 {n}.")));
        assert_eq!(u.contains("now: waist-deep floodwater"), land, "{u}");
    }
    assert!(god.iter().any(|u| u.contains("now: waist-deep floodwater")));
    // Small: at most two lines per prompt.
    for (_, u) in &wet {
        assert!(u.matches("The earth").count() + u.matches("the earth now").count() <= 2, "{u}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schema_16_state_asks_byte_identical_prompts() {
    let wet16 = beat_two(true, Some(16)).await;
    let calm16 = beat_two(false, Some(16)).await;
    let calm17 = beat_two(false, None).await;
    assert!(!wet16.is_empty());
    assert_eq!(wet16, calm16, "schema 16: the field changes no prompt");
    assert_eq!(calm16, calm17, "calm ground: schema 17 asks what schema 16 asked");
}

#[test]
fn a_general_reads_the_ground_under_its_army() {
    let p = FinalNight7Plugin::new();
    let settings = p.validate_settings(&json!({ "seed": 11, "llm": false })).unwrap();
    let state = futures::executor::block_on(p.on_match_start(&humans(), &settings, &crate::testkit::Failing)).unwrap();
    let mut w = load(&state).unwrap();
    let pantheon = p.pantheon().clone();
    let d = DemigodId(0);
    let src = w.clusters.iter().filter(|c| c.army.is_none()).max_by_key(|c| c.size).unwrap().id;
    let raise = Proposal {
        source: Source::Rules, intent: None,
        effect: Effect::RaiseArmy { demigod: d, from: src, size: 5000, name: "Ash's Host".into(), orders: String::new() },
        act: "raise".into(), actor: Ref::Demigod(d), magnitude: 5, valence: 0, witnesses: vec![], cause: None,
    };
    w.meta.beat = 1;
    crate::merge::run(&mut w, vec![raise], &pantheon, 1, 4);
    let army = ClusterId::from_ix(w.clusters.len() - 1);
    let st = crate::Settings::from_value(&w.meta.settings);
    crate::society::run_step(&mut w, 1, &[], vec![], &pantheon, &st);
    let pos = w.clusters[army.ix()].army.as_ref().unwrap().pos;
    let m = crate::society::GeneralMeta { army, by: d, why: "order", text: "hold the ford".into(), agent: None, intent: None };
    let calm = crate::society::general_call(&w, &m, None, 2).expect("a general").situation;
    assert!(calm.iter().all(|s| !s.contains("The earth")));
    paint(&mut w, &[pos], None, |c| {
        c.crack = 140;
        c.lava = 30;
    });
    let wet = crate::society::general_call(&w, &m, None, 2).unwrap().situation;
    assert_eq!(wet.last().map(String::as_str),
               Some("The earth where your army stands, now: cooling black lava, still glowing; an open rift splits the ground."));
    w.meta.schema = 16;
    assert_eq!(crate::society::general_call(&w, &m, None, 2).unwrap().situation, calm);
}

// ---------------------------------------------------------------------------------------------------------------------
// The duel: the stage and the deed
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn a_duel_on_lava_stands_on_lava() {
    use crate::duel_tests::*;
    let t = table(41, json!({ "duel_deadline_ms": 30000 }));
    let w = load(&t.state).unwrap();
    let id = format!("duel:s{}:0:1", 7);
    let d = crate::duel::Duel::with_id(&w, id.clone(), 2, t.me, t.foe, "regions", Some(0), t0() + 1_000, &[]);
    let calm = crate::panel::scene(&w, &d);
    assert!(calm.get("ground").is_none());
    let mut hot = w.clone();
    paint(&mut hot, &[d.at], Some((VOLCANO, 1)), |c| {
        c.lava = 70;
        c.plume = 50;
    });
    let s = crate::panel::scene(&hot, &d);
    assert_eq!(s["ground"], json!({ "line": "molten lava running, smoke, the eruption began last beat", "floor": "lava", "hazards": ["lava"], "slippery": false }));
    assert!(s["backstory"].as_str().unwrap().ends_with("The ground: molten lava running, smoke, the eruption began last beat."));
    let objs = s["objects"].as_array().unwrap();
    assert_eq!(objs.iter().filter(|o| o["kind"] == "lava").count(), 2, "{objs:?}");
    // Everything else on the stage is where it was.
    let n = calm["objects"].as_array().unwrap().len();
    assert_eq!(&objs[..n], calm["objects"].as_array().unwrap().as_slice());
    for o in &objs[n..] {
        assert!(o["parts"].as_array().unwrap().iter().all(|p| p["shape"].is_string() && p["material"].is_string()));
        assert!(o["at"] != s["a_start"] && o["at"] != s["b_start"]);
    }
    // A flooded hex: slippery; snow: a drift to vault.
    let mut wet = w.clone();
    paint(&mut wet, &[d.at], None, |c| c.water = 100);
    let s = crate::panel::scene(&wet, &d);
    assert_eq!((s["ground"]["floor"].as_str(), s["ground"]["slippery"].as_bool()), (Some("flooded"), Some(true)));
    // Schema 16: the same scene as calm ground.
    hot.meta.schema = 16;
    let mut calm16 = w.clone();
    calm16.meta.schema = 16;
    assert_eq!(crate::panel::scene(&hot, &d), crate::panel::scene(&calm16, &d));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_deed_of_a_duel_on_cracked_ground_knows_it() {
    use crate::duel_tests::*;
    async fn deed(paint_it: bool, schema: Option<u32>) -> (Value, Vec<String>) {
        let mut t = table(34, json!({ "duel_deadline_ms": 9000, "duel_damage_scale": 0.1, "llm": true }));
        let mut w = load(&t.state).unwrap();
        if paint_it {
            let at = w.demigods[t.me.ix()].pos;
            paint(&mut w, &[at], Some((EARTHQUAKE, 1)), |c| {
                c.crack = 90;
                c.shake = 30;
            });
        }
        if let Some(s) = schema {
            w.meta.schema = s;
        }
        t.state = save(&w);
        let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
        let id = lock["duel_id"].as_str().unwrap().to_string();
        let scene = lock["also"][0].clone();
        let rec = Rec::default();
        let mut actions = bot_actions(&t.p, &t.state);
        actions.extend([instant(0, challenge(&t, "regions"), 1, 1_000), instant(0, line(&id, "I kick your knee into the mud"), 2, 1_500), past_deadline()]);
        let _ = t.p.resolve_round(&t.state, 2, &actions, &RoundContext::default(), &rec, &NoProgress).await.unwrap();
        let asked: Vec<String> = rec.seen.into_inner().unwrap().into_iter().filter(|(s, _)| s.contains("Two demigods just fought")).map(|x| x.1).collect();
        (scene, asked)
    }
    let (scene, asked) = deed(true, None).await;
    assert_eq!(scene["ground"]["floor"], "cracked");
    assert!(scene["objects"].as_array().unwrap().iter().any(|o| o["kind"] == "fissure"));
    assert_eq!(asked.len(), 1, "the deed was asked once");
    assert!(asked[0].contains("The ground they fought on: the ground is cracked and still shaking after the quake last beat."), "{}", asked[0]);
    let (calm_scene, calm) = deed(false, None).await;
    assert!(calm_scene.get("ground").is_none() && !calm[0].contains("The ground"));
    let (_, old) = deed(true, Some(16)).await;
    assert_eq!(old, calm, "schema 16: the deed is asked as before");
}

/// The prompts of one flooded beat and the same beat on calm ground, for the docs (`-- --ignored --nocapture`).
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn print_earth_prompt_examples() {
    let wet = beat_two(true, None).await;
    let calm = beat_two(false, None).await;
    for sys in ["HAND of a demigod", "GAME MASTER", "a god of FINAL NIGHT 7", "You are a SUBAGENT", "You are ENTROPY", "NARRATOR of FINAL NIGHT 7"] {
        let god = |u: &&String| sys != "a god of FINAL NIGHT 7" || u.contains("The invocation");
        let w = of(&wet, sys).into_iter().filter(god).find(|u| u.contains("The earth") || u.contains("the earth now")).cloned().unwrap_or_default();
        let c = of(&calm, sys).into_iter().find(god).map(|x| x.to_string()).unwrap_or_default();
        println!("===== {sys} (calm)\n{c}\n===== {sys} (flooded)\n{w}\n");
    }
}
