//! The next move (state schema 18): every priority case gives its hint, spectators watch, AI seats get none, the caps
//! hold, it is deterministic, and nothing hidden leaks.

use fn7_model::dm::{Encounter, EncounterKind};
use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::*;

use super::*;
use crate::earth::grid::grid;
use crate::earth::rules;
use crate::earth::sim::Sim;
use crate::testkit::Failing;
use crate::wills::curses::Curse;
use crate::wills::quests::{Goal, Step, StepKind};
use crate::{world, FinalNight7Plugin};

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(f)
}

/// Seats 0 (Chris) and 1 (Benito) are humans; 2 and 3 are Theatre bots.
fn seats() -> Vec<SeatInfo> {
    (0..4).map(|i| SeatInfo { seat: i, display_name: ["Chris", "Benito", "Vex", "Mara"][i as usize].into(), is_bot: i >= 2 }).collect()
}

const MADRID: LonLat = [-3.7, 40.4];
const LISBON: LonLat = [-9.14, 38.72];
/// Far from every fault and from each other.
const FAR: [LonLat; 6] = [[134.0, -24.0], [128.0, -28.0], [-60.0, -10.0], [-55.0, -15.0], [20.0, 20.0], [25.0, 25.0]];

/// The state at the start of beat 2 of a quiet match (no encounters, no AI wills or duels).
fn start(extra: Value) -> (FinalNight7Plugin, Value) {
    let p = FinalNight7Plugin::new();
    let mut s = json!({ "seed": 41, "beats_total": 60, "llm": false, "dm_encounters_per_beat": 0, "duel_ai_chance": 0.0,
                        "wills_ai_chance": 0.0, "arena_close_beats": 1000 });
    if let (Some(o), Some(x)) = (s.as_object_mut(), extra.as_object()) {
        o.extend(x.clone());
    }
    let settings = p.validate_settings(&s).unwrap();
    let state = block(p.on_match_start(&seats(), &settings, &Failing)).unwrap();
    let state = block(p.on_round_start(&state, 1, &Failing)).unwrap().state;
    let acts: Vec<Action> = (0..4u8).map(|s| p.default_action(&state, s)).collect();
    let o = block(p.resolve_round(&state, 1, &acts, &RoundContext::default(), &Failing, &NoProgress)).unwrap();
    let state = block(p.commit_round(&o.state, 1, &[], &Failing)).unwrap().state;
    let state = block(p.on_round_start(&state, 2, &Failing)).unwrap().state;
    (p, state)
}

/// A calm board: everyone healthy, carrying a trinket, Time's circle wide open, no armies, no duels, no fresh events;
/// seat 0's two demigods in Madrid and Lisbon, every rival far away and far from the faults.
fn calm(w: &mut World) {
    let a = w.meta.arena;
    w.places[a.ix()].radius_km = 30000.0;
    w.meta.arena_end_radius_km = 30000.0;
    w.meta.arena_radius_km = 30000.0;
    for c in w.clusters.iter_mut() {
        c.army = None;
    }
    w.dm.duels.clear();
    w.dm.encounters.clear();
    w.dm.loot.clear();
    w.dm.earth = None;
    w.dm.wills = Value::Null;
    let beat = w.meta.beat;
    w.ledger.rows.retain(|r| r.beat + 1 < beat);
    let mut far = FAR.iter();
    for i in 0..w.demigods.len() {
        let d = &mut w.demigods[i];
        d.health = 10.0;
        d.max_health = 10.0;
        d.alive = true;
        d.eliminated_beat = None;
        d.cause = None;
        d.bolts = 0;
        d.pos = match (d.seat, d.side) {
            (0, Some(Hand::Right)) => LISBON,
            (0, _) => MADRID,
            _ => *far.next().unwrap_or(&[0.0, -80.0]),
        };
    }
    for i in 0..w.demigods.len() {
        let p = w.demigods[i].pos;
        w.demigods[i].place = fn7_society::geo::place_at(w, p);
    }
    for i in 0..w.demigods.len() {
        give(w, w.demigods[i].id, "a plain walking stick");
    }
}

fn give(w: &mut World, d: DemigodId, name: &str) {
    let id = ItemId(w.items.len() as u32);
    w.items.push(Item {
        id, name: name.into(), description: name.into(), origin: ItemOrigin::Starting, created_beat: 1, created_by: None,
        holders: vec![Ref::Demigod(d)], uses: 0, ext: Default::default(),
    });
    w.demigods[d.ix()].items.push(id);
}

fn strip(w: &mut World, d: DemigodId) {
    let me = Ref::Demigod(d);
    for it in w.items.iter_mut() {
        it.holders.retain(|h| *h != me);
    }
    w.demigods[d.ix()].items.clear();
}

/// Seat `seat`'s `side` demigod.
fn body(w: &World, seat: u8, side: Hand) -> DemigodId {
    w.bodies_of_seat(seat).find(|d| d.side == Some(side)).unwrap().id
}

fn army(w: &mut World, owner: DemigodId, pos: LonLat, orders: &str) -> ClusterId {
    let c = w.clusters.iter().position(|c| c.army.is_none() && c.size > 0).unwrap();
    let id = w.clusters[c].id;
    w.clusters[c].army = Some(Army {
        demigod: owner, orders: orders.into(), pos, dest: None, target: None, raised_beat: 1, from: id, general: None,
        captains: vec![], units: vec![], morale: 1.0, supply: 1.0, stance: Default::default(), rogue: false, order: None,
        strain: 0.0, battles: 0, losses: 0, volunteer: false,
    });
    id
}

fn row(w: &mut World, actor: DemigodId, verb: &str, act: &str, at: LonLat, magnitude: u8) -> LedgerId {
    let id = w.ledger.next_id();
    w.ledger.rows.push(LedgerRow {
        id, beat: w.meta.beat.saturating_sub(1), source: Source::Resolver, actor: Ref::Demigod(actor), verb: verb.into(),
        act: act.into(), targets: vec![], place: None, magnitude, valence: -3, witnessed_by: vec![], known_by: Default::default(),
        known_at: vec![], told_to: vec![], erased_from: vec![], debt: None, cause: None, intent: None, at: Some(at),
    });
    id
}

fn paint(w: &mut World, at: LonLat, f: impl Fn(&mut crate::earth::sim::Cell)) {
    let st = world::settings(w);
    let mut sim = rules::sim_of(w, &st).unwrap_or_else(|| Sim::new(w.meta.seed, rules::cfg(&st)));
    f(sim.m(grid().index_of(at)));
    sim.normalize();
    let s = w.dm.earth.get_or_insert_with(Default::default);
    sim.to_state(s);
}

fn quest_step(text: &str, kind: StepKind, near: Option<LonLat>) -> Step {
    Step { kind, text: text.into(), done_beat: None, progress: String::new(),
           near: near.map(|at| crate::wills::quests::Near { name: "Toledo".into(), at }) }
}

fn quest(id: u32, seat: u8, d: DemigodId, goal: Goal, steps: Vec<Step>) -> Quest {
    Quest { id, seat, helpers: vec![], demigod: d, goal, god: GodId(3), steps, text: "a mystic quest".into(), given_beat: 1, since: 1,
            expires_beat: None, done_beat: None, failed: None, failed_beat: None, public: false, waiting: String::new() }
}

/// A calm world at beat 2, edited by `f`, and seat 0's hint.
fn case(extra: Value, f: impl FnOnce(&mut World)) -> (World, Hint) {
    let (_, state) = start(extra);
    let mut w = world::load(&state).unwrap();
    calm(&mut w);
    f(&mut w);
    let w = world::load(&world::save(&w)).unwrap();
    let h = for_seat(&w, 0).expect("a human seat gets a hint");
    check(&h);
    println!("{:<12} {}", h.kind, serde_json::to_string(&h.json()).unwrap());
    (w, h)
}

fn check(h: &Hint) {
    assert!(h.text.chars().count() <= TEXT_MAX, "text too long: {}", h.text);
    assert!(h.why.chars().count() <= WHY_MAX, "why too long: {}", h.why);
    assert!(!h.text.is_empty() && !h.why.is_empty());
    assert!(h.urgency <= 3);
    assert!(KINDS.contains(&h.kind), "{}", h.kind);
    let v = h.json();
    assert!(HintAction::ALL.contains(&v["action"].as_str().unwrap()));
}

#[test]
fn danger_an_army_at_a_weak_demigod_and_bolts_to_spend() {
    let (w, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        w.demigods[l.ix()].health = 3.0;
        w.demigods[l.ix()].bolts = 2;
        let vex = body(w, 1, Hand::Left);
        army(w, vex, fn7_society::geo::destination(MADRID, 90.0, 180.0), "march");
    });
    assert_eq!((h.kind, h.action, h.urgency), ("danger", HintAction::CastPower, 3));
    assert!(h.text.starts_with("Your Left Hand is under attack in ") && h.text.contains("drag a power onto"), "{}", h.text);
    assert!(h.why.contains("army is 180 km away") && h.why.contains("3 health"), "{}", h.why);
    assert!(matches!(h.target.as_ref().unwrap().r#ref, Some(Ref::Cluster(_))));
    let _ = w;
}

#[test]
fn danger_without_bolts_heals_and_a_withering_flees() {
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        w.demigods[l.ix()].health = 2.5;
        let vex = body(w, 1, Hand::Right);
        w.demigods[vex.ix()].pos = fn7_society::geo::destination(MADRID, 0.0, 200.0);
    });
    assert_eq!((h.kind, h.action), ("danger", HintAction::Heal));
    assert!(h.why.contains("no bolts") && h.why.contains("2.5 health"), "{}", h.why);
}

#[test]
fn danger_the_ground_burning_under_a_weak_demigod() {
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        w.demigods[l.ix()].health = 3.0;
        paint(w, MADRID, |c| c.fire = 160);
    });
    assert_eq!(h.kind, "danger");
    assert!(matches!(h.action, HintAction::Summon | HintAction::Flee));
    assert!(h.text.contains("fire") && h.why.contains("fire"), "{} / {}", h.text, h.why);
    let t = h.target.unwrap();
    assert_ne!(t.at, Some(MADRID), "points away from the fire");
}

#[test]
fn danger_time_s_circle_closing_past_a_weak_demigod() {
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        w.demigods[l.ix()].health = 1.0;
        let a = w.meta.arena;
        let c = w.places[a.ix()].center;
        w.places[a.ix()].radius_km = 100.0;
        w.meta.arena_end_radius_km = 100.0;
        w.demigods[l.ix()].pos = fn7_society::geo::destination(c, 45.0, 2000.0);
    });
    assert_eq!(h.kind, "danger");
    assert!(h.text.contains("Time's circle"), "{}", h.text);
}

#[test]
fn a_healthy_demigod_near_an_army_is_not_in_danger() {
    let (_, h) = case(json!({}), |w| {
        let vex = body(w, 1, Hand::Left);
        army(w, vex, fn7_society::geo::destination(MADRID, 90.0, 180.0), "march");
    });
    assert_ne!(h.kind, "danger");
}

#[test]
fn resurrect_a_fallen_demigod_death_can_raise() {
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        w.demigods[l.ix()].alive = false;
        w.demigods[l.ix()].health = 0.0;
        w.demigods[l.ix()].eliminated_beat = Some(1);
    });
    assert_eq!((h.kind, h.action, h.urgency), ("resurrect", HintAction::Resurrect, 2));
    assert!(h.text.starts_with("Raise your Left Hand") && h.why.starts_with("Death asks"), "{} / {}", h.text, h.why);
}

#[test]
fn a_sealed_demigod_names_the_counter_quest() {
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        let vex = body(w, 1, Hand::Left);
        w.demigods[l.ix()].alive = false;
        w.demigods[l.ix()].health = 0.0;
        w.demigods[l.ix()].eliminated_beat = Some(1);
        let mut b = crate::wills::Book::default();
        b.quests.push(quest(1, 0, l, Goal::BreakCurse { curse: 1, curse_kind: CurseKind::Seal }, vec![
            quest_step("Hold a butterfly's wing", StepKind::Hold { word: "wing".into() }, None),
            quest_step("Stand at a shrine at dawn", StepKind::At { req: PlaceReq::Settlement { level: 1 }, phase: Some("dawn".into()), offer: None }, Some(LISBON)),
        ]));
        b.curses.push(Curse { id: 1, kind: CurseKind::Seal, owner: vex, owner_seat: 1, target: l, strength: 2, cast_beat: 1, until_beat: None,
                              paid_sparks: 5, god: None, quest: 1, broken_beat: None, broken_how: String::new() });
        crate::wills::save(w, &b);
    });
    assert_eq!((h.kind, h.action), ("break_curse", HintAction::BreakCurse));
    assert_eq!(h.text, "Break the seal on your Left Hand: Hold a butterfly's wing");
    assert!(h.why.contains("step 1 of 2"), "{}", h.why);
    // The owner of the seal is the bearer's to know (curses::private_json), but the hint never needs it.
    assert!(!h.text.contains("Benito") && !h.why.contains("Benito"));
}

#[test]
fn spectators_watch_the_biggest_thing() {
    let (_, h) = case(json!({ "raise_cap": 0 }), |w| {
        for side in [Hand::Left, Hand::Right] {
            let d = body(w, 0, side);
            w.demigods[d.ix()].alive = false;
            w.demigods[d.ix()].health = 0.0;
            w.demigods[d.ix()].eliminated_beat = Some(1);
        }
        let vex = body(w, 1, Hand::Left);
        row(w, vex, "cast", "Benito's Left Hand drops a volcano on Cairo", [31.2, 30.0], 8);
        row(w, vex, "cast", "Benito's Left Hand lights a candle", [31.2, 30.0], 2);
    });
    assert_eq!((h.kind, h.action, h.urgency), ("watch", HintAction::Watch, 0));
    assert!(h.text.contains("drops a volcano on Cairo"), "{}", h.text);
    assert!(h.why.contains("Death will raise no more"), "{}", h.why);
}

#[test]
fn a_rival_in_your_hex_is_a_challenge() {
    let (_, h) = case(json!({}), |w| {
        let vex = body(w, 1, Hand::Right);
        w.demigods[vex.ix()].pos = fn7_society::geo::destination(MADRID, 10.0, 40.0);
    });
    assert_eq!((h.kind, h.action, h.urgency), ("challenge", HintAction::Challenge, 2));
    assert!(h.text.contains("Benito's Right Hand") && h.text.contains("challenge"), "{}", h.text);
}

#[test]
fn a_world_event_near_your_demigod_asks_for_a_reaction() {
    let (_, h) = case(json!({}), |w| {
        let vex = body(w, 1, Hand::Left);
        row(w, vex, "cast", "Benito's Left Hand hurls a meteor at Zaragoza", [-0.88, 41.65], 7);
    });
    assert_eq!((h.kind, h.action, h.urgency), ("event", HintAction::TypeWill, 2));
    assert!(h.text.starts_with("A cast struck near your Left Hand") && h.text.ends_with("type a will to react"), "{}", h.text);
    assert!(h.why.contains("hurls a meteor at Zaragoza"), "{}", h.why);
    assert!(matches!(h.target.unwrap().r#ref, Some(Ref::Ledger(_))));
}

#[test]
fn bolts_and_a_rival_on_a_junction_ask_for_a_mega() {
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        w.demigods[l.ix()].bolts = 3;
        let vex = body(w, 1, Hand::Left);
        w.demigods[vex.ix()].pos = [42.0, 11.5];
    });
    assert_eq!((h.kind, h.action, h.urgency), ("mega", HintAction::CastPower, 1));
    assert!(h.text.starts_with("Cast a mega on Benito's Left Hand at the Afar triple junction"), "{}", h.text);
    assert!(h.why.starts_with("Plate boundaries meet there, so it hits ") && h.why.contains("3 bolts"), "{}", h.why);
    assert!(!h.why.contains("tier") && !h.text.contains("tier"), "no jargon: {} / {}", h.text, h.why);
}

/// Schema 22 (clear words) and its gate: the schema-21 wording stays as it was.
fn at_schema(schema: u32, f: impl FnOnce(&mut World)) -> Hint {
    let (_, state) = start(json!({}));
    let mut w = world::load(&state).unwrap();
    calm(&mut w);
    w.meta.schema = schema;
    f(&mut w);
    let w = world::load(&world::save(&w)).unwrap();
    let h = for_seat(&w, 0).expect("a hint");
    check(&h);
    println!("schema {schema}: {:<8} {} | {}", h.kind, h.text, h.why);
    h
}

fn junction_rival(w: &mut World) {
    let l = body(w, 0, Hand::Left);
    w.demigods[l.ix()].bolts = 3;
    let vex = body(w, 1, Hand::Left);
    w.demigods[vex.ix()].pos = [42.0, 11.5];
}

#[test]
fn clear_words_mega_has_no_tier_jargon_and_schema_21_is_unchanged() {
    let old = at_schema(21, junction_rival);
    assert!(old.why.contains("junction tier"), "{}", old.why);
    let new = at_schema(22, junction_rival);
    assert_eq!(new.text, old.text);
    assert!(new.why.starts_with("Plate boundaries meet there, so it hits ") && !new.why.contains("tier"), "{}", new.why);
}

#[test]
fn clear_words_an_event_right_here_says_so() {
    let ev = |w: &mut World| {
        let vex = body(w, 1, Hand::Left);
        row(w, vex, "cast", "The sea walks inland", MADRID, 7);
    };
    let old = at_schema(21, ev);
    assert!(old.why.starts_with("0 km away: The sea walks inland"), "{}", old.why);
    let new = at_schema(22, ev);
    assert_eq!(new.kind, "event");
    assert_eq!(new.why, "Right where your Left Hand stands: The sea walks inland");
    assert!(!new.why.contains("0 km"));
}

#[test]
fn clear_words_a_stale_low_urgency_hint_rotates_every_3_beats() {
    // A mega (rule 6) and a will (rule 9) both apply: they take turns, 3 beats each, from the state's beat.
    let kinds: Vec<&str> = (2..=9u32)
        .map(|beat| at_schema(22, |w| {
            junction_rival(w);
            w.meta.beat = beat;
        }).kind)
        .collect();
    assert_eq!(kinds, ["mega", "will", "will", "will", "mega", "mega", "mega", "will"]);
    // Schema 21: the top rule every beat.
    for beat in 2..=9u32 {
        assert_eq!(at_schema(21, |w| { junction_rival(w); w.meta.beat = beat; }).kind, "mega");
    }
    // Urgent hints never rotate: a rival in your hex stays.
    for beat in 2..=9u32 {
        let h = at_schema(22, |w| {
            junction_rival(w);
            w.meta.beat = beat;
            let vex = body(w, 1, Hand::Right);
            w.demigods[vex.ix()].pos = fn7_society::geo::destination(MADRID, 10.0, 0.5);
        });
        assert_eq!(h.kind, "challenge");
    }
}

#[test]
fn clear_words_a_cast_on_your_own_demigod_warns_next_beat() {
    let own = |w: &mut World| {
        let l = body(w, 0, Hand::Left);
        let r = body(w, 0, Hand::Right);
        w.demigods[r.ix()].health = 7.0;
        let id = row(w, l, "cast", "Chris's Left Hand casts lightning on Lisbon", LISBON, 5);
        w.ledger.rows.last_mut().unwrap().witnessed_by = vec![Ref::Demigod(r)];
        let _ = id;
    };
    let new = at_schema(22, own);
    assert_eq!((new.kind, new.action, new.urgency), ("own_hit", HintAction::Heal, 2));
    assert!(new.text.starts_with("Your lightning hit your own Right Hand in ") && !new.text.contains("38."), "{}", new.text);
    let old = at_schema(21, own);
    assert_ne!(old.kind, "own_hit");
}

#[test]
fn clear_words_cast_lines_name_the_place_and_an_own_hit() {
    use crate::actions::{CastOrder, CastTarget};
    let moscow: LonLat = [37.63, 55.96];
    let lines = |schema: u32| {
        let (_, state) = start(json!({}));
        let mut w = world::load(&state).unwrap();
        calm(&mut w);
        w.meta.schema = schema;
        let l = body(&w, 1, Hand::Left);
        w.demigods[l.ix()].pos = moscow;
        let o = |at: LonLat| CastOrder { power: "lightning".into(), target: CastTarget::At(at), text: None, magnitude: None, seq: 1,
                                         instant: true, scale: Some(CastScale::Mini) };
        let mine = crate::casts::streak(&w, l, 1, &[o(moscow)]).unwrap();
        let far = crate::casts::streak(&w, l, 1, &[o([20.0, 20.0])]).unwrap();
        (mine, far)
    };
    let (mine, far) = lines(22);
    println!("{}\n{}", mine["text"], far["text"]);
    let t = mine["text"].as_str().unwrap();
    assert!(t.contains("right on their own Left Hand") && t.contains("Moscow"), "{t}");
    assert!(!t.contains("55.") && mine["at"] == json!(moscow) && mine["own_hit"].is_string(), "{mine}");
    let t = far["text"].as_str().unwrap();
    assert!(t.starts_with("Benito casts ") && !t.contains("own") && !t.contains("20."), "{t}");
    assert!(far.get("own_hit").is_none());
    let (mine, _) = lines(21);
    assert!(mine.get("text").is_none() && mine.get("own_hit").is_none(), "schema 21 untouched: {mine}");
}

#[test]
fn no_gear_seeks_the_nearest_marked_hex_without_saying_what_hides_there() {
    let (w, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        strip(w, l);
    });
    assert_eq!((h.kind, h.action), ("gear", HintAction::TypeWill));
    assert!(h.text.starts_with("Arm your Left Hand"), "{}", h.text);
    let _ = w;
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        strip(w, l);
        let at = [-3.0, 40.0];
        let cell = fn7_model::hexgrid::cell_at(at, fn7_model::hexgrid::HexLevel::People).key();
        let region = fn7_society::geo::region_at(w, at);
        w.dm.encounters.push(Encounter { id: 7, kind: EncounterKind::Monster, cell: cell.clone(), at, region, settlement: None,
                                         placed_beat: 1, night: false, faded_beat: None, outcome: None });
    });
    assert_eq!((h.kind, h.action), ("gear", HintAction::SeekGear));
    assert!(h.text.contains("marked hex"), "{}", h.text);
    let s = serde_json::to_string(&h.json()).unwrap().to_lowercase();
    assert!(!s.contains("monster"), "the hidden kind never shows: {s}");
    assert!(h.target.unwrap().cell.is_some());
}

#[test]
fn an_open_quest_names_its_current_step() {
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        let mut b = crate::wills::Book::default();
        let mut s1 = quest_step("Win an encounter", StepKind::Encounter, None);
        s1.done_beat = Some(1);
        b.quests.push(quest(4, 0, l, Goal::Heal { amount: 3.0 }, vec![
            s1,
            quest_step("Stand in a city at dusk", StepKind::At { req: PlaceReq::Settlement { level: 2 }, phase: Some("dusk".into()), offer: None }, Some(LISBON)),
        ]));
        crate::wills::save(w, &b);
    });
    assert_eq!((h.kind, h.action, h.urgency), ("quest", HintAction::Summon, 1));
    assert_eq!(h.text, "Quest for your Left Hand: Stand in a city at dusk");
    assert!(h.why.contains("Step 2 of 2") && h.why.contains("heals your Left Hand 3 more"), "{}", h.why);
    assert_eq!(h.target.unwrap().at, Some(LISBON));
}

#[test]
fn otherwise_a_concrete_will() {
    let (_, h) = case(json!({}), |_| {});
    assert_eq!((h.kind, h.action, h.urgency), ("will", HintAction::TypeWill, 0));
    assert!(h.text.starts_with("Type a will: raise an army in "), "{}", h.text);
    let (_, h) = case(json!({}), |w| {
        let r = body(w, 0, Hand::Right);
        w.demigods[r.ix()].health = 5.0;
    });
    assert_eq!((h.kind, h.action), ("will", HintAction::Heal));
    assert!(h.text.starts_with("Heal your Right Hand"), "{}", h.text);
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        army(w, l, MADRID, "hold Madrid");
    });
    assert!(h.text.starts_with("Type a will: march your Left Hand on "), "{}", h.text);
}

#[test]
fn ai_seats_get_no_hint_and_humans_get_it_in_every_seat_view() {
    let (p, state) = start(json!({}));
    let w = world::load(&state).unwrap();
    assert!(w.hint_rules());
    for seat in [2u8, 3] {
        assert!(for_seat(&w, seat).is_none());
        assert!(crate::phases::seat_view(&w, seat).get("hint").is_none());
    }
    for seat in [0u8, 1] {
        let mut h = for_seat(&w, seat).unwrap().json();
        crate::payload::tidy(&mut h);
        let sv = crate::phases::seat_view(&w, seat);
        assert_eq!(sv["hint"], h);
        // game.me answers the same object.
        let me = crate::queries::query(&p.pantheon, &state, Some(seat), "me", &json!({})).unwrap();
        assert_eq!(me["hint"], sv["hint"], "game.me agrees with the seat view");
    }
    let brief = block(p.brief(&state, 2, &Failing)).unwrap();
    for (seat, v) in &brief.per_seat {
        assert_eq!(v.get("hint").is_some(), *seat < 2, "seat {seat}");
    }
    // A takeover by the AI: the seat turns bot, the hint goes.
    let mut w2 = w.clone();
    w2.seats.iter_mut().find(|s| s.seat == 0).unwrap().is_bot = true;
    assert!(for_seat(&w2, 0).is_none());
}

#[test]
fn schema_17_states_carry_no_hint() {
    let (_, state) = start(json!({}));
    let mut w = world::load(&state).unwrap();
    w.meta.schema = 17;
    assert!(for_seat(&w, 0).is_none());
    assert!(crate::phases::seat_view(&w, 0).get("hint").is_none());
}

#[test]
fn deterministic_and_within_the_caps_over_a_played_match() {
    let p = FinalNight7Plugin::new();
    let settings = p.validate_settings(&json!({ "seed": 9, "beats_total": 30, "llm": false })).unwrap();
    let mut state = block(p.on_match_start(&seats(), &settings, &Failing)).unwrap();
    let mut kinds = std::collections::BTreeSet::new();
    for round in 1..=12 {
        state = block(p.on_round_start(&state, round, &Failing)).unwrap().state;
        let w = world::load(&state).unwrap();
        for seat in [0u8, 1] {
            let a = for_seat(&w, seat).unwrap();
            check(&a);
            let again = for_seat(&world::load(&world::save(&w)).unwrap(), seat).unwrap();
            assert_eq!(a, again, "beat {round} seat {seat}");
            kinds.insert(a.kind);
        }
        let acts: Vec<Action> = (0..4u8).map(|s| block(p.bot_action(&state, s, &Failing)).unwrap()).collect();
        let o = block(p.resolve_round(&state, round, &acts, &RoundContext::default(), &Failing, &NoProgress)).unwrap();
        state = block(p.commit_round(&o.state, round, &[], &Failing)).unwrap().state;
    }
    println!("kinds over 12 beats: {kinds:?}");
}

#[test]
fn fit_cuts_at_a_word() {
    assert_eq!(fit("short", 80), "short");
    let s = fit("Watch the great and terrible meteor that fell on the city of Constantinople last night", 40);
    assert!(s.chars().count() <= 40 && s.ends_with('…'), "{s}");
    assert!(!s.contains("Constantinople"));
}

/// Hidden information never reaches the hint, whatever the case: a rival's standing orders, its armies' orders, its
/// health, a hidden encounter's kind.
#[test]
fn hidden_information_never_leaks() {
    let secret = |w: &mut World| {
        for s in [1u8, 2, 3] {
            for d in w.bodies_of_seat(s).map(|d| d.id).collect::<Vec<_>>() {
                w.demigods[d.ix()].orders.push(StandingOrder { hand: Hand::Left, verb: "burn".into(), targets: vec![], place: None,
                    magnitude: 3, span: "SECRET-ORDERS-QWERTY".into(), since: 1 });
                w.demigods[d.ix()].health = 7.25;
            }
        }
        let vex = body(w, 1, Hand::Left);
        let c = army(w, vex, fn7_society::geo::destination(MADRID, 90.0, 150.0), "SECRET-ARMY-XYZZY");
        let me = body(w, 0, Hand::Left);
        if let Some(a) = w.clusters[c.ix()].army.as_mut() {
            a.target = Some(Ref::Demigod(me));
        }
        let at = [-3.0, 40.0];
        let cell = fn7_model::hexgrid::cell_at(at, fn7_model::hexgrid::HexLevel::People).key();
        let region = fn7_society::geo::region_at(w, at);
        w.dm.encounters.push(Encounter { id: 9, kind: EncounterKind::Undead, cell, at, region, settlement: None,
                                         placed_beat: 1, night: true, faded_beat: None, outcome: None });
    };
    let edits: Vec<Box<dyn Fn(&mut World)>> = vec![
        Box::new(|w| { let l = body(w, 0, Hand::Left); w.demigods[l.ix()].health = 2.0; w.demigods[l.ix()].bolts = 1; }),
        Box::new(|w| { let l = body(w, 0, Hand::Left); w.demigods[l.ix()].health = 2.0; }),
        Box::new(|w| { let l = body(w, 0, Hand::Left); strip(w, l); }),
        Box::new(|w| { let vex = body(w, 1, Hand::Right); w.demigods[vex.ix()].pos = fn7_society::geo::destination(MADRID, 0.0, 20.0); }),
        Box::new(|w| { let l = body(w, 0, Hand::Left); w.demigods[l.ix()].bolts = 4; let v = body(w, 1, Hand::Left); w.demigods[v.ix()].pos = [42.0, 11.5]; }),
        Box::new(|_| {}),
    ];
    for e in edits {
        let (_, h) = case(json!({}), |w| {
            secret(w);
            e(w);
        });
        let s = serde_json::to_string(&h.json()).unwrap();
        for bad in ["SECRET-ORDERS-QWERTY", "SECRET-ARMY-XYZZY", "7.25", "undead", "Undead"] {
            assert!(!s.contains(bad), "{bad} leaked in {s}");
        }
    }
}

fn open_duel(w: &mut World, a: DemigodId, b: DemigodId) {
    let beat = w.meta.beat;
    let d = crate::duel::Duel::new(w, beat, a, b, "regions", Some(1), 0, &[]);
    let bout = crate::duel::Bout::new(d);
    w.dm.duels.push(serde_json::to_value(&bout).unwrap());
}

#[test]
fn in_a_duel_the_hint_says_type_now_above_everything() {
    let (_, h) = case(json!({}), |w| {
        let l = body(w, 0, Hand::Left);
        w.demigods[l.ix()].health = 2.0; // in danger too: the duel still comes first
        let vex = body(w, 2, Hand::Right);
        w.demigods[vex.ix()].pos = fn7_society::geo::destination(MADRID, 0.0, 30.0);
        open_duel(w, vex, l);
    });
    assert_eq!((h.kind, h.action, h.urgency), ("duel", HintAction::DuelType, 3));
    assert_eq!(h.text, "You're in a duel with Vex's Right Hand: TYPE NOW");
    assert_eq!(h.why, "They hit you every 3 s; each line is a blow");
    assert_eq!(h.json()["action"], "duel_type");
}

#[test]
fn every_human_seat_gets_a_goal_line() {
    let (p, state) = start(json!({}));
    let mut w = world::load(&state).unwrap();
    let total = w.meta.beats_total;
    let beat = w.meta.beat;
    let sv = crate::phases::seat_view(&w, 0);
    assert_eq!(sv["goal"], format!("Last standing wins. You: 2 alive, 3 rivals. Beat {beat} of {total}."));
    assert!(crate::phases::seat_view(&w, 2).get("goal").is_none(), "bots get none");
    let brief = block(p.brief(&state, beat + 1, &Failing)).unwrap();
    let b0 = brief.per_seat.iter().find(|x| x.0 == 0).unwrap();
    assert!(b0.1["goal"].as_str().unwrap().ends_with(&format!("Beat {} of {total}.", beat + 1)));
    let l = body(&w, 0, Hand::Left);
    w.demigods[l.ix()].alive = false;
    for side in [Hand::Left, Hand::Right] {
        let d = body(&w, 1, side);
        w.demigods[d.ix()].alive = false;
    }
    w.meta.beat = total;
    assert_eq!(goal(&w, 0, total).unwrap(), format!("Last standing wins. You: 1 alive, 2 rivals. Beat {total} of {total}: the Final Night."));
}

#[test]
fn duel_odds_are_seeded_banded_and_honest_about_silence() {
    let (p, state) = start(json!({}));
    let mut w = world::load(&state).unwrap();
    calm(&mut w);
    let l = body(&w, 0, Hand::Left);
    let vex = body(&w, 2, Hand::Right);
    w.demigods[vex.ix()].pos = fn7_society::geo::destination(MADRID, 0.0, 50.0);
    w.demigods[vex.ix()].health = 6.37;
    let state = world::save(&w);
    let q = |seat: Option<u8>, a: DemigodId, b: DemigodId| {
        crate::queries::query(&p.pantheon, &state, seat, "duel_odds", &json!({ "demigod": Ref::Demigod(a).to_string(), "target": Ref::Demigod(b).to_string() })).unwrap()
    };
    let v = q(Some(0), l, vex);
    println!("duel_odds {v}");
    assert_eq!(v["my_health"], 10.0);
    assert_eq!(v["their_health"], "hurt", "a rival's health is a band");
    assert!(!v.to_string().contains("6.37") && !v.to_string().contains("6.4"));
    assert_eq!(v["reach_ok"], true);
    let win = v["win_pct"].as_f64().unwrap();
    assert!(win > 50.0 && win <= 100.0, "{win}");
    assert!(v["idle_win_pct"].as_f64().unwrap() < 5.0);
    assert!(v["why"].as_array().unwrap().iter().any(|x| x.as_str().unwrap().contains("AI counters every 3 s")));
    assert_eq!(q(Some(0), l, vex), v, "deterministic");
    // Over HTTP (no seat): both sides banded.
    let pubv = q(None, l, vex);
    assert_eq!(pubv["my_health"], "hale");
    // The rival's own seat sees its health exactly (seat 2 is a bot here, but a seat is a seat).
    assert_eq!(q(Some(2), vex, l)["my_health"], 6.4);
    // Too far.
    let far = body(&w, 3, Hand::Left);
    let f = q(Some(0), l, far);
    assert_eq!(f["reach_ok"], false);
    assert!(f["too_far_km"].as_f64().unwrap() > 0.0);
    // In an open duel the hp is public, so it is exact.
    let mut w2 = world::load(&state).unwrap();
    open_duel(&mut w2, vex, l);
    let s2 = world::save(&w2);
    let d = crate::queries::query(&p.pantheon, &s2, Some(0), "duel_odds", &json!({ "demigod": Ref::Demigod(l).to_string(), "target": Ref::Demigod(vex).to_string() })).unwrap();
    assert_eq!(d["their_health"], 6.4);
}
