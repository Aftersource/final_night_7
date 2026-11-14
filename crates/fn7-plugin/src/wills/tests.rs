//! Rules 0.9.0: wills that matter. Every rule and failure path, the recorded live case as a regression, the gate.

use async_trait::async_trait;
use serde_json::{json, Value};
use theatre_plugin_api::*;

use super::curses::CurseKind;
use super::quests::{Goal, PlaceReq, StepKind};
use super::*;
use crate::testkit::Failing;
use crate::{actions, world, FinalNight7Plugin};

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(f)
}

/// Seat 0 (Chris) and seat 1 (Benito) are humans; seats 2 and 3 are Theatre bots.
fn seats() -> Vec<SeatInfo> {
    (0..4).map(|i| SeatInfo { seat: i, display_name: ["Chris", "Benito", "Vex", "Mara"][i as usize].into(), is_bot: i >= 2 }).collect()
}

struct Table {
    p: FinalNight7Plugin,
    state: Value,
    round: u32,
}

/// A quiet match at the start of beat 2: no encounters, no AI wills or duels (the tests drive everything).
fn table(seed: u64, extra: Value) -> Table {
    let p = FinalNight7Plugin::new();
    let mut s = json!({ "seed": seed, "beats_total": 60, "llm": false, "dm_encounters_per_beat": 0, "duel_ai_chance": 0.0,
                        "wills_ai_chance": 0.0, "arena_close_beats": 1000 });
    if let (Some(o), Some(x)) = (s.as_object_mut(), extra.as_object()) {
        o.extend(x.clone());
    }
    let settings = p.validate_settings(&s).unwrap();
    let state = block(p.on_match_start(&seats(), &settings, &Failing)).unwrap();
    let state = block(p.on_round_start(&state, 1, &Failing)).unwrap().state;
    let o = block(p.resolve_round(&state, 1, &quiet(&p, &state), &RoundContext::default(), &Failing, &NoProgress)).unwrap();
    let state = block(p.commit_round(&o.state, 1, &[], &Failing)).unwrap().state;
    let mut w = world::load(&state).unwrap();
    for d in w.demigods.iter_mut() {
        d.health = 10.0;
        d.max_health = 10.0;
        d.alive = true;
        d.eliminated_beat = None;
        d.cause = None;
    }
    let state = block(p.on_round_start(&world::save(&w), 2, &Failing)).unwrap().state;
    Table { p, state, round: 2 }
}

fn quiet(p: &FinalNight7Plugin, state: &Value) -> Vec<Action> {
    (0..4u8).map(|s| p.default_action(state, s)).collect()
}

impl Table {
    fn w(&self) -> World {
        world::load(&self.state).unwrap()
    }
    fn edit(&mut self, f: impl FnOnce(&mut World)) {
        let mut w = self.w();
        f(&mut w);
        self.state = world::save(&w);
    }
    /// Resolve this beat with `text` as seat `seat`'s will (the left box), then commit and start the next beat.
    fn will(&mut self, seat: u8, text: &str) -> RoundOutcome {
        self.will_with(seat, text, &Failing)
    }
    fn will_with(&mut self, seat: u8, text: &str, llm: &dyn LlmProvider) -> RoundOutcome {
        let mut acts = quiet(&self.p, &self.state);
        if !text.is_empty() {
            acts[seat as usize] = actions::action(seat, text, "", None);
        }
        let o = block(self.p.resolve_round(&self.state, self.round, &acts, &RoundContext::default(), llm, &NoProgress)).unwrap();
        let s = block(self.p.commit_round(&o.state, self.round, &[], &Failing)).unwrap().state;
        self.round += 1;
        self.state = block(self.p.on_round_start(&s, self.round, &Failing)).unwrap().state;
        o
    }
    fn pass(&mut self) -> RoundOutcome {
        self.will(0, "")
    }
}

fn private(o: &RoundOutcome, seat: u8) -> &Value {
    &o.per_seat.iter().find(|(s, _)| *s == seat).unwrap().1
}

fn mine(w: &World, seat: u8) -> Vec<DemigodId> {
    w.bodies_of_seat(seat).map(|d| d.id).collect()
}

fn purse(w: &mut World, seat: u8, bolts: u32, sparks: u32) {
    let d = mine(w, seat)[0];
    w.set_bolts(d, bolts);
    w.set_sparks(d, sparks);
}

/// The hand LLM answering with a fixed reply (every other mind offline: the rules fallback).
struct HandSays(&'static str);

#[async_trait]
impl LlmProvider for HandSays {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        if req.system.contains("HAND of a demigod") {
            return Ok(LlmResponse { choices: vec![self.0.to_string()], cached: false });
        }
        anyhow::bail!("offline")
    }
}

// ---- reading a will ---------------------------------------------------------------------------------------------------

#[test]
fn a_will_splits_into_its_parts() {
    let t = table(1, json!({}));
    let mut w = t.w();
    let b = Book::default();
    let k = |w: &World, text: &str| kinds_in(w, &b, 0, text);
    assert_eq!(k(&w, "heal and arm my demigods"), vec![Kind::Heal, Kind::Arm]);
    assert_eq!(k(&w, "every one of mydemigods is fully healed and gets rocket launchers"), vec![Kind::Heal, Kind::Arm], "the recorded will");
    assert_eq!(k(&w, "Healing and arming my demigods now"), vec![Kind::Heal, Kind::Arm]);
    assert_eq!(k(&w, "bless my hands with god powers"), vec![Kind::Bless]);
    assert_eq!(k(&w, "arm my demigods with holy swords"), vec![Kind::Arm], "a blessing with an arming is the gear's");
    assert_eq!(k(&w, "go to Paris and heal me"), vec![Kind::Heal]);
    // Not wills: the old routes keep them.
    for text in ["raise a shrine to me here", "raise an army from the dockworkers and march on Vex", "forge a sword of stars",
                 "go to Lagos", "silence the crowd", "seal the gates", "arm the dockworkers", "raise the dead", "recruit the locals",
                 "raise banners of 10,000 soldiers at Rome, no debt", "attack Vex's Left Hand"] {
        assert_eq!(k(&w, text), vec![], "{text}");
    }
    // Curses: strong words, or a weak one aimed at a named rival.
    assert_eq!(k(&w, "curse Benito's Left Hand with wither"), vec![Kind::Curse]);
    assert_eq!(k(&w, "bind Benito's Right Hand"), vec![Kind::Curse]);
    assert_eq!(k(&w, "no resurrection for Vex"), vec![Kind::Curse]);
    assert_eq!(k(&w, "how do I break the curse on me?"), vec![Kind::Ask]);
    assert_eq!(k(&w, "what does my quest need"), vec![Kind::Ask]);
    assert_eq!(k(&w, "offer the sparks to the quest"), vec![Kind::Offer]);
    // A plea needs someone fallen.
    assert_eq!(k(&w, "my guys come back to life!"), vec![]);
    let fallen = mine(&w, 0)[1];
    w.demigods[fallen.ix()].alive = false;
    assert_eq!(k(&w, "my guys come back to life!"), vec![Kind::Raise]);
    assert_eq!(k(&w, "revive the other demigods"), vec![Kind::Raise]);
    assert_eq!(k(&w, "raise the dead"), vec![Kind::Raise]);
    assert_eq!(k(&w, "raise a shrine to me here"), vec![]);
    // The gear a will names.
    assert_eq!(gear_noun("every one of mydemigods gets rocket launchers"), Some(("weapon", "Rocket Launcher".to_string())));
    assert_eq!(gear_noun("give us shields"), Some(("armor", "Shield".to_string())));
    assert_eq!(gear_noun("arm my demigods"), None);
    assert_eq!(CurseKind::from_text("no resurrection for him"), Some(CurseKind::Seal));
    assert_eq!(CurseKind::from_text("wither her"), Some(CurseKind::Wither));
    assert_eq!(CurseKind::from_text("hello"), None);
}

// ---- the recorded case (match d3a3779e, beat 42) ---------------------------------------------------------------------

/// What the live server did with "every one of mydemigods is fully healed and gets rocket launchers" (the hand said
/// "Healing and arming both demigods now."): Nature refused the heal outright ("you cannot pay"), the arming went to
/// the resolver as "a deed" (d20 2 vs DC 11: a bent bottle cap), nothing reached the Right Hand, no `wills[]` told him
/// why. Now: both demigods get a heal attempt and an arm attempt, each with a visible result.
fn recorded_setup(t: &mut Table) -> (DemigodId, DemigodId) {
    let w = t.w();
    let (l, r) = (mine(&w, 0)[0], mine(&w, 0)[1]);
    t.edit(|w| {
        w.demigods[l.ix()].health = 3.8;
        w.demigods[r.ix()].health = 7.1;
        // 0 bolts, 4 sparks, and 2.26 of open debt.
        purse(w, 0, 0, 4);
        let p = Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Debt { owed_by: Ref::Demigod(l), god: GodId(0), amount: 2.26, due_in: 50 },
            act: "Time's old bills".into(),
            actor: Ref::God(GodId(0)),
            magnitude: 2,
            valence: -1,
            witnesses: vec![],
            cause: None,
        };
        crate::merge::run(w, vec![p], &fn7_pantheon::load(), 2, 4);
    });
    (l, r)
}

fn check_recorded(o: &RoundOutcome, w0: &World, w2: &World, l: DemigodId, r: DemigodId) {
    let me = private(o, 0);
    let wills = me["wills"].as_array().expect("wills[] tells the player");
    assert_eq!(wills.len(), 1, "{me}");
    let will = &wills[0];
    let parts = will["parts"].as_array().unwrap();
    // Two parts x two demigods, each with its own ruling.
    for (part, d) in [("heal", l), ("heal", r), ("arm", l), ("arm", r)] {
        let p = parts
            .iter()
            .find(|p| p["part"] == part && p["target"] == json!(Ref::Demigod(d)))
            .unwrap_or_else(|| panic!("no {part} part for {d}: {will}"));
        let outcome = p["outcome"].as_str().unwrap();
        assert!(!p["result"].as_str().unwrap().is_empty(), "plain words: {p}");
        assert!(p["roll"]["target_dc"].as_i64().unwrap() < 11, "an own-target DC is below the old 11: {p}");
        assert!(p["roll"]["mods"].is_array() && p["paid"]["price_sparks"].is_u64(), "the roll's modifiers and the price are named: {p}");
        match outcome {
            "success" | "crit" => {}
            // Short of the will: it says why and what would work.
            "partial" | "fizzle" | "quest" => {
                assert!(p["why"].is_string(), "{p}");
                assert!(p["next"].is_string() || p["quest"]["steps"].as_array().is_some_and(|s| !s.is_empty()), "what would work: {p}");
            }
            other => panic!("unexpected outcome {other}: {p}"),
        }
        if part == "arm" && outcome != "fizzle" {
            let item = &p["item"];
            assert!(item["mods"].is_object() && item["does"].is_string(), "the gear does something: {p}");
            assert!(item["name"].as_str().unwrap().contains("Rocket Launcher"), "{item}");
        }
    }
    // The world changed: nobody is worse off, and unless both heals fizzled someone is better; the gear exists.
    for d in [l, r] {
        assert!(w2.demigods[d.ix()].health >= w0.demigods[d.ix()].health - 1e-4);
    }
    let healed: f32 = [l, r].iter().map(|d| w2.demigods[d.ix()].health - w0.demigods[d.ix()].health).sum();
    let fizzled = parts.iter().filter(|p| p["part"] == "heal" && p["outcome"] == "fizzle").count();
    assert!(healed > 0.0 || fizzled == 2, "healed {healed}: {will}");
    let b = book(w2);
    for p in parts.iter().filter(|p| matches!(p["outcome"].as_str(), Some("partial" | "fizzle" | "quest")) && p["quest"].is_object()) {
        let id = p["quest"]["ref"].as_str().unwrap();
        assert!(b.quests.iter().any(|q| quests::qref(q.id) == id && q.open()), "the quest is stored: {id}");
    }
    // No bare "a deed" for the arming, and it is not a standing order repeated every other beat.
    assert!(!o.public["dice"].as_array().unwrap().iter().any(|x| x["reason"] == "a deed" && x["who"] == json!(Ref::Demigod(l))), "{}", o.public["dice"]);
    assert!(w2.demigods[l.ix()].orders.is_empty(), "a will is resolved once: {:?}", w2.demigods[l.ix()].orders);
    assert!(me["omens"].as_array().unwrap().iter().any(|x| x["god"] == "Nature"), "Nature's ruling is shown: {}", me["omens"]);
    // The private demigods carry their gear.
    let gear: usize = me["demigods"].as_array().unwrap().iter().map(|d| d["items"].as_array().map(Vec::len).unwrap_or(0)).sum();
    let armed = parts.iter().filter(|p| p["part"] == "arm" && p["outcome"] != "fizzle").count();
    assert_eq!(gear, armed, "{}", me["demigods"]);
}

#[test]
fn the_recorded_will_heals_and_arms_both_demigods_by_the_rules_fallback() {
    for seed in [42, 43, 44] {
        let mut t = table(seed, json!({}));
        let (l, r) = recorded_setup(&mut t);
        let w0 = t.w();
        let o = t.will(0, "every one of mydemigods is fully healed and gets rocket launchers");
        check_recorded(&o, &w0, &world::load(&o.state).unwrap(), l, r);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_recorded_will_heals_and_arms_both_demigods_with_the_recorded_hand_reply() {
    // The hand's recorded answer: two intents (heal 8; arm 6), both for P1 and P6, both routed "world".
    const RECORDED: &str = r#"{"intents":[{"arg":"","how":"","magnitude":8,"route":"world","targets":[],"verb":"heal"},{"arg":"","how":"","magnitude":6,"route":"world","targets":[],"verb":"arm"}],"said":"Healing and arming both demigods now."}"#;
    let mut t = table(42, json!({ "llm": true }));
    let (l, r) = recorded_setup(&mut t);
    let w0 = t.w();
    let mut acts = quiet(&t.p, &t.state);
    acts[0] = actions::action(0, "every one of mydemigods is fully healed and gets rocket launchers", "", None);
    let o = t.p.resolve_round(&t.state, 2, &acts, &RoundContext::default(), &HandSays(RECORDED), &NoProgress).await.unwrap();
    let w2 = world::load(&o.state).unwrap();
    check_recorded(&o, &w0, &w2, l, r);
    // The heal went by its magnitude 8 (DC 6 + 2), the arming by 6 (an artifact: DC 6 + 2).
    let parts = private(&o, 0)["wills"][0]["parts"].clone();
    let heal = parts.as_array().unwrap().iter().find(|p| p["part"] == "heal").unwrap().clone();
    assert_eq!(heal["roll"]["target_dc"], 8);
    let arm = parts.as_array().unwrap().iter().find(|p| p["part"] == "arm").unwrap().clone();
    // The 4 sparks went to the heals, so the artifact asked for came down to what was paid: a trinket (DC 6).
    assert_eq!((arm["asked_tier"].as_str(), arm["roll"]["target_dc"].as_i64(), arm["item"]["tier"].as_str()), (Some("artifact"), Some(6), Some("trinket")));
    assert!(arm["why"].as_str().unwrap().contains("artifact tier costs 2 sparks; 0 paid: trinket tier"), "{arm}");
    assert_eq!(private(&o, 0)["hands"][0]["said"], "Healing and arming both demigods now.");
}

// ---- healing and blessing ---------------------------------------------------------------------------------------------

/// Every verdict of the heal's d20, over seeds: crit = more and a boon; success = the heal; fail = a token and a quest;
/// a natural 1 = a fizzle and a quest. Never a bare fail.
#[test]
fn a_heal_is_never_a_bare_fail() {
    let mut seen = std::collections::BTreeSet::new();
    for seed in 100..140 {
        let mut t = table(seed, json!({}));
        let d = mine(&t.w(), 0)[0];
        t.edit(|w| {
            w.demigods[d.ix()].health = 4.0;
            purse(w, 0, 0, 5);
        });
        let o = t.will(0, &format!("heal {}", t.w().demigods[d.ix()].name));
        let w2 = world::load(&o.state).unwrap();
        let p = &private(&o, 0)["wills"][0]["parts"][0];
        // What the will healed (the world may also have hurt it this beat: armies, accidents).
        let got = p["healed"].as_f64().unwrap_or(0.0) as f32;
        let outcome = p["outcome"].as_str().unwrap().to_string();
        let verdict = p["roll"]["verdict"].as_str().unwrap();
        // The rules hand reads a plain "heal" as magnitude 4: 1 + 4/4 = 2 health, 2 sparks, DC 6 + 1.
        assert_eq!((p["paid"]["price_sparks"].as_u64(), p["paid"]["sparks"].as_u64(), p["roll"]["target_dc"].as_i64()), (Some(2), Some(2), Some(7)));
        assert_eq!(w2.demigods[d.ix()].sparks, 3);
        let b = book(&w2);
        match verdict {
            "crit_success" => {
                assert_eq!(outcome, "crit");
                assert!((got - 3.0).abs() < 0.06, "{got}");
                assert!(b.boons.iter().any(|x| x.demigod == d && x.bonus == 1 && x.until_beat == 8), "{:?}", b.boons);
            }
            "success" => {
                assert_eq!(outcome, "success");
                assert!((got - 2.0).abs() < 0.06, "{got}");
                assert!(b.quests.is_empty());
            }
            "fail" => {
                assert_eq!(outcome, "partial");
                assert!((got - 0.5).abs() < 0.06, "a token heal: {got}");
                let q = b.quests.iter().find(|q| q.demigod == d).expect("the quest that completes it");
                assert!(matches!(q.goal, Goal::Heal { amount } if (amount - 1.5).abs() < 0.06), "{:?}", q.goal);
                assert!(p["why"].as_str().unwrap().contains("against DC 7") && p["quest"]["steps"][0]["text"].is_string(), "{p}");
            }
            _ => {
                assert_eq!(outcome, "fizzle");
                assert!(got.abs() < 1e-4);
                assert!(b.quests.iter().any(|q| q.demigod == d && matches!(q.goal, Goal::Heal { .. })));
            }
        }
        seen.insert(outcome);
    }
    assert!(seen.contains("success") && seen.contains("partial"), "{seen:?}");
}

#[test]
fn an_empty_purse_borrows_and_past_the_cap_becomes_a_quest() {
    // Nothing to pay with: Nature lends (a debt), the roll is "on credit".
    let mut t = table(7, json!({}));
    let d = mine(&t.w(), 0)[0];
    t.edit(|w| {
        w.demigods[d.ix()].health = 4.0;
        purse(w, 0, 0, 0);
    });
    let before = world::open_debt(&t.w(), Ref::Demigod(d), None);
    let o = t.will(0, "heal me");
    let w2 = world::load(&o.state).unwrap();
    let p = &private(&o, 0)["wills"][0]["parts"][0];
    assert_eq!((p["paid"]["sparks"].as_u64(), p["paid"]["short_sparks"].as_u64()), (Some(0), Some(2)));
    assert!((p["paid"]["debt"].as_f64().unwrap() - 0.4).abs() < 1e-6, "{p}");
    assert!(p["roll"]["mods"].as_array().unwrap().iter().any(|m| m["name"] == "on credit" && m["value"] == -1), "{p}");
    assert!(world::open_debt(&w2, Ref::Demigod(d), None) >= before + 0.39, "the debt is owed");
    // Past the cap the gods lend no more: no roll, a quest instead (never silence).
    let mut t = table(7, json!({ "will_debt_cap": 0.0 }));
    t.edit(|w| {
        w.demigods[d.ix()].health = 4.0;
        purse(w, 0, 0, 0);
    });
    let o = t.will(0, "heal me");
    let p = &private(&o, 0)["wills"][0]["parts"][0];
    assert_eq!(p["outcome"], "quest");
    assert!(p.get("roll").is_none() && p["quest"]["steps"].as_array().is_some_and(|s| !s.is_empty()), "{p}");
    assert_eq!(world::load(&o.state).unwrap().demigods[d.ix()].health, 4.0);
    // A bolt breaks into five sparks, and the change stays.
    let mut t = table(7, json!({}));
    t.edit(|w| {
        w.demigods[d.ix()].health = 4.0;
        // Two bolts: Time takes one as the order's toll (text entry is Time's), the other breaks for the heal.
        purse(w, 0, 2, 0);
    });
    let o = t.will(0, "heal me");
    let w2 = world::load(&o.state).unwrap();
    assert_eq!((w2.demigods[d.ix()].bolts, w2.demigods[d.ix()].sparks), (0, 3));
    assert_eq!(private(&o, 0)["wills"][0]["parts"][0]["paid"]["bolts_broken"], 1);
}

#[test]
fn a_blessing_is_a_boon_on_the_dice() {
    for seed in 200..220 {
        let mut t = table(seed, json!({}));
        let d = mine(&t.w(), 0)[0];
        let o = t.will(0, "bless me");
        let p = private(&o, 0)["wills"][0]["parts"][0].clone();
        if p["outcome"] != "success" {
            continue;
        }
        let w2 = world::load(&o.state).unwrap();
        let st = world::settings(&w2);
        assert_eq!(standing_mods(&w2, d, 3), vec![("boon".to_string(), 1)]);
        assert_eq!(arms::act_mod(&w2, d, "punch", 3, &st), 1);
        assert_eq!(standing_mods(&w2, d, 9), vec![], "six beats");
        assert_eq!(private(&o, 0)["boons"][0]["bonus"], 1);
        return;
    }
    panic!("no blessing succeeded in 20 seeds");
}

// ---- arms ---------------------------------------------------------------------------------------------------------------

#[test]
fn arms_have_teeth() {
    use fn7_model::manifest::Tier;
    // The pure table.
    let obsidian = &arms::MATERIALS[0];
    let (m, p) = arms::mods_for("weapon", Tier::Relic, obsidian, None);
    assert_eq!((m.strike, m.damage, m.duel, p), (2, 0.5, 1, Some("smite")));
    let (m, p) = arms::mods_for("armor", Tier::Artifact, obsidian, None);
    assert_eq!((m.guard, m.resist.as_deref(), p), (0.5, Some("fire"), None));
    let (m, p) = arms::mods_for("charm", Tier::Relic, obsidian, Some("nature"));
    assert_eq!((m.deed, (m.regen * 100.0).round(), p), (1, 35.0, Some("second_wind")));
    assert_eq!(arms::mods_for("weapon", Tier::Trinket, obsidian, Some("death")).1, Some("last_stand"));
    assert_eq!(arms::material_of(1, "Butterfly-Wing Cloak").0, "butterfly-wing");
    // In a world: a relic weapon and an artifact armour.
    let t = table(11, json!({}));
    let mut w = t.w();
    let pantheon = fn7_pantheon::load();
    let st = world::settings(&w);
    let (a, foe) = (mine(&w, 0)[0], mine(&w, 1)[0]);
    forge_now(&mut w, 2, a, ItemKind::Weapon, Tier::Relic, "Obsidian Spear", None, "a test", &pantheon, &st).unwrap();
    forge_now(&mut w, 2, foe, ItemKind::Armor, Tier::Artifact, "Gold Shield", None, "a test", &pantheon, &st).unwrap();
    let k = arms::kit(&w, a, &st);
    assert_eq!((k.mods.strike, k.mods.damage, k.mods.duel), (2, 0.5, 1));
    assert_eq!(arms::act_mod(&w, a, "attack", 2, &st), 2, "strikes use the weapon");
    assert_eq!(arms::act_mod(&w, a, "walk", 2, &st), 0);
    assert_eq!(arms::duel_bonus(&w, a), 1);
    // A strike of 2 by the spear on the shield: +0.5 (weapon) +2 (Smite, once), -0.5 (guard), halved (gold resists
    // blades... a "spear" strike): (2 + 0.5 + 2 - 0.5) / 2 = 2.
    let strike = |w: &World| Proposal {
        source: Source::Resolver,
        intent: None,
        effect: Effect::Damage { who: Ref::Demigod(foe), amount: 2.0, cause: "a spear thrust".into() },
        act: format!("{} strikes", w.demigods[a.ix()].name),
        actor: Ref::Demigod(a),
        magnitude: 5,
        valence: -2,
        witnesses: vec![],
        cause: None,
    };
    let s1 = strike(&w);
    let p = arms::shape(&mut w, vec![s1], 2);
    assert!(matches!(p[0].effect, Effect::Damage { amount, .. } if (amount - 2.0).abs() < 1e-4), "{:?}", p[0].effect);
    assert_eq!(book(&w).used.len(), 1, "Smite is spent");
    // The second strike: no Smite: (2 + 0.5 - 0.5) / 2 = 1.
    let s2 = strike(&w);
    let p = arms::shape(&mut w, vec![s2], 3);
    assert!(matches!(p[0].effect, Effect::Damage { amount, .. } if (amount - 1.0).abs() < 1e-4), "{:?}", p[0].effect);
    // The payload says what it does, and the piece's story pegs carry it.
    let item = crate::items::item_json(&w, &w.items[w.items.len() - 2]);
    assert_eq!((item["slot"].as_str(), item["mods"]["strike"].as_i64(), item["power"]["id"].as_str(), item["power"]["used"].as_bool()),
               (Some("weapon"), Some(2), Some("smite"), Some(true)));
    assert!(item["does"].as_str().unwrap().contains("+2 to strikes"), "{item}");
    let peg = &crate::dm::story_json(&w, a)["items"][0];
    assert_eq!((peg["slot"].as_str(), peg["mods"]["strike"].as_i64()), (Some("weapon"), Some(2)));
    // Bounded: a second, lesser weapon adds nothing; arms_scale 0 switches gear off.
    forge_now(&mut w, 2, a, ItemKind::Weapon, Tier::Trinket, "Oak Club", None, "a test", &pantheon, &st).unwrap();
    assert_eq!(arms::kit(&w, a, &st).mods.strike, 2);
    let off = crate::Settings { arms_scale: 0.0, ..st.clone() };
    assert_eq!(arms::kit(&w, a, &off), arms::Kit::default());
    // Gated: with the rules off nothing applies.
    let none = crate::Settings { wills_enabled: false, ..st.clone() };
    assert_eq!(arms::act_mod(&w, a, "attack", 2, &none), 0);
}

#[test]
fn last_stand_and_second_wind_fire_once() {
    use fn7_model::manifest::Tier;
    let t = table(12, json!({}));
    let mut w = t.w();
    let pantheon = fn7_pantheon::load();
    let st = world::settings(&w);
    let a = mine(&w, 0)[0];
    forge_now(&mut w, 2, a, ItemKind::Armor, Tier::Relic, "Moonsteel Plate", None, "a test", &pantheon, &st).unwrap();
    forge_now(&mut w, 2, a, ItemKind::Relic, Tier::Relic, "Gold Talisman", None, "a test", &pantheon, &st).unwrap();
    let kill = |w: &mut World, beat: u32| {
        let p = Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Eliminate { who: Ref::Demigod(a), cause: "a test".into() },
            act: "it falls".into(),
            actor: Ref::Demigod(a),
            magnitude: 9,
            valence: -5,
            witnesses: vec![],
            cause: None,
        };
        crate::merge::run(w, vec![p], &fn7_pantheon::load(), beat, 4);
    };
    kill(&mut w, 2);
    assert!(!w.demigods[a.ix()].alive);
    assert_eq!(arms::last_stand(&mut w, 2, &[a], &pantheon, &st), vec![a]);
    assert!(w.demigods[a.ix()].alive && w.demigods[a.ix()].health == 1.0, "{}", w.demigods[a.ix()].health);
    assert!(w.dm.memory.iter().any(|e| e.kind == "arms.power" && e.public));
    // Second Wind: at 1 health the charm heals 2 at the beat's end (and mends 0.15 a beat).
    arms::upkeep(&mut w, 2, &pantheon, &st);
    assert!((w.demigods[a.ix()].health - 3.15).abs() < 0.01, "{}", w.demigods[a.ix()].health);
    // Once a match each.
    kill(&mut w, 3);
    assert!(arms::last_stand(&mut w, 3, &[a], &pantheon, &st).is_empty());
    assert!(!w.demigods[a.ix()].alive);
}

#[test]
fn arming_makes_gear_by_tier_and_blessing() {
    let mut tiers = std::collections::BTreeSet::new();
    for seed in 300..330 {
        // Relic asked ("legendary": magnitude 8), blessed by Nature, with the purse for it (5 + 5 sparks a demigod).
        let mut t = table(seed, json!({}));
        t.edit(|w| purse(w, 0, 4, 0));
        let o = t.will(0, "Nature, arm my demigods with legendary holy spears");
        let w2 = world::load(&o.state).unwrap();
        let parts = private(&o, 0)["wills"][0]["parts"].as_array().unwrap().clone();
        assert_eq!(parts.len(), 2, "one arming per demigod");
        assert_eq!(w2.demigods[mine(&w2, 0)[0].ix()].bolts, 0, "2 x (5 + 5) sparks = 4 bolts");
        for p in &parts {
            assert_eq!((p["god"].as_str(), p["asked_tier"].as_str(), p["blessed"].as_bool(), p["roll"]["target_dc"].as_i64()),
                       (Some("Nature"), Some("relic"), Some(true), Some(10)));
            let outcome = p["outcome"].as_str().unwrap();
            if outcome == "fizzle" {
                assert!(p["quest"]["goal"]["kind"] == "arm");
                continue;
            }
            let item = &p["item"];
            let want = match outcome {
                "crit" => "legend",
                "success" => "wonder",
                _ => "relic",
            };
            // Blessed: a tier up, and Nature's signature (regeneration) on a weapon.
            assert_eq!((item["tier"].as_str(), item["blessed_by"].as_str(), item["slot"].as_str()), (Some(want), Some("Nature"), Some("weapon")), "{p}");
            assert!((item["mods"]["regen"].as_f64().unwrap() - 0.2).abs() < 1e-6 && item["name"].as_str().unwrap().ends_with("Spear"), "{item}");
            if outcome == "partial" {
                assert_eq!(p["quest"]["goal"]["kind"], "temper");
            }
            tiers.insert(want);
        }
    }
    assert!(tiers.contains("wonder") && tiers.contains("relic"), "{tiers:?}");
    // What can't be paid is lent while the debt allows; past the cap the tier comes down to what was paid, and it says so.
    let mut t = table(301, json!({ "will_debt_cap": 0.0 }));
    t.edit(|w| purse(w, 0, 0, 2));
    let o = t.will(0, "arm my demigods with legendary spears");
    let parts = private(&o, 0)["wills"][0]["parts"].as_array().unwrap().clone();
    let (a, b) = (&parts[0], &parts[1]);
    assert_eq!((a["paid"]["sparks"].as_u64(), b["paid"]["sparks"].as_u64()), (Some(2), Some(0)));
    assert!(a["why"].as_str().unwrap().contains("relic tier costs 5 sparks; 2 paid: artifact tier"), "{a}");
    assert!(b["why"].as_str().unwrap().contains("trinket tier"), "{b}");
    assert_eq!((a["roll"]["target_dc"].as_i64(), b["roll"]["target_dc"].as_i64()), (Some(8), Some(6)));
}

/// Balance guard: gear's totals are capped whatever is held.
#[test]
fn a_kit_is_capped() {
    use fn7_model::manifest::Tier;
    let t = table(13, json!({}));
    let mut w = t.w();
    let pantheon = fn7_pantheon::load();
    let st = world::settings(&w);
    let a = mine(&w, 0)[0];
    let chance = pantheon.id_of("chance");
    forge_now(&mut w, 2, a, ItemKind::Weapon, Tier::Apocalypse, "Gold Sword", chance, "t", &pantheon, &st).unwrap();
    forge_now(&mut w, 2, a, ItemKind::Armor, Tier::Apocalypse, "Gold Plate", chance, "t", &pantheon, &st).unwrap();
    forge_now(&mut w, 2, a, ItemKind::Relic, Tier::Apocalypse, "Gold Idol", pantheon.id_of("nature"), "t", &pantheon, &st).unwrap();
    let k = arms::kit(&w, a, &st);
    assert!(k.mods.strike <= arms::CAP_D20 && k.mods.deed <= arms::CAP_D20 && k.mods.duel <= arms::CAP_DUEL);
    assert!(k.mods.damage <= arms::CAP_DAMAGE && k.mods.guard <= arms::CAP_GUARD && k.mods.regen <= arms::CAP_REGEN);
    assert_eq!(k.arms.len(), 3);
}

// ---- resurrection -------------------------------------------------------------------------------------------------------

fn fell(w: &mut World, d: DemigodId, beat: u32) {
    let x = &mut w.demigods[d.ix()];
    x.alive = false;
    x.health = 0.0;
    x.eliminated_beat = Some(beat);
    x.cause = Some("a test".into());
}

#[test]
fn death_raises_for_its_price_and_the_price_rises() {
    let mut t = table(21, json!({}));
    let (l, r) = (mine(&t.w(), 0)[0], mine(&t.w(), 0)[1]);
    t.edit(|w| {
        fell(w, r, 1);
        purse(w, 0, 3, 5);
    });
    let o = t.will(0, "bring my Right Hand back to life");
    let w2 = world::load(&o.state).unwrap();
    let p = &private(&o, 0)["wills"][0]["parts"][0];
    assert_eq!((p["part"].as_str(), p["outcome"].as_str(), p["god"].as_str()), (Some("raise"), Some("raised"), Some("Death")), "{p}");
    assert_eq!((p["price"]["sparks"].as_u64(), p["paid"]["sparks"].as_u64()), (Some(5), Some(5)));
    let x = &w2.demigods[r.ix()];
    assert!(x.alive && (x.max_health - 9.0).abs() < 1e-4 && (x.health - 3.6).abs() < 1e-3, "partial health, a year of its life: {} / {}", x.health, x.max_health);
    assert_eq!(book(&w2).raises.len(), 1);
    assert!(o.public["dm_events"].as_array().unwrap().iter().any(|e| e["event"] == "demigod.raised" && e["demigod"] == json!(Ref::Demigod(r))), "{}", o.public["dm_events"]);
    assert!(o.public["wills"].as_array().unwrap().iter().any(|e| e["event"] == "will.raised"));
    // It falls again: the second raising costs double, and two years.
    t.edit(|w| fell(w, r, 3));
    let o = t.will(0, "revive my right hand");
    let w3 = world::load(&o.state).unwrap();
    let p = &private(&o, 0)["wills"][0]["parts"][0];
    assert_eq!((p["price"]["sparks"].as_u64(), p["price"]["years"].as_f64(), p["price"]["raised_before"].as_u64()), (Some(10), Some(2.0), Some(1)), "{p}");
    assert!(w3.demigods[r.ix()].alive && (w3.demigods[r.ix()].max_health - 7.0).abs() < 1e-4);
    // The cap: a third and fourth plea in a match capped at 2 are refused, with the reason.
    let mut t = table(21, json!({ "raise_cap": 1 }));
    t.edit(|w| {
        fell(w, r, 1);
        purse(w, 0, 9, 5);
    });
    assert_eq!(private(&t.will(0, "revive my right hand"), 0)["wills"][0]["parts"][0]["outcome"], "raised");
    t.edit(|w| fell(w, r, 3));
    let o = t.will(0, "revive my right hand");
    let p = &private(&o, 0)["wills"][0]["parts"][0];
    assert_eq!(p["outcome"], "refused");
    assert!(p["result"].as_str().unwrap().contains("no more"), "{p}");
    let _ = l;
}

#[test]
fn anyone_may_be_raised_and_a_wiped_player_may_plead() {
    // "revive the other demigods": a rival's fallen demigod, raised by seat 0's purse.
    let mut t = table(22, json!({}));
    let theirs = mine(&t.w(), 1)[0];
    t.edit(|w| {
        fell(w, theirs, 1);
        purse(w, 0, 2, 5);
    });
    let o = t.will(0, "revive the other demigods");
    let p = &private(&o, 0)["wills"][0]["parts"][0];
    assert_eq!((p["outcome"].as_str(), p["target"].clone()), (Some("raised"), json!(Ref::Demigod(theirs))), "{p}");
    assert!(world::load(&o.state).unwrap().demigods[theirs.ix()].alive);
    // A wiped player with the price: both come back, by the fallen-will path.
    let mut t = table(22, json!({}));
    let (l, r) = (mine(&t.w(), 0)[0], mine(&t.w(), 0)[1]);
    t.edit(|w| {
        fell(w, l, 1);
        fell(w, r, 1);
        purse(w, 0, 2, 0);
    });
    let o = t.will(0, "my guys come back to life!");
    let w2 = world::load(&o.state).unwrap();
    let will = &private(&o, 0)["wills"][0];
    assert_eq!((will["routed_to"].as_str(), will["verdict"].as_str(), will["parts"].as_array().unwrap().len()), (Some("Death"), Some("granted"), 2), "{will}");
    assert!(w2.demigods[l.ix()].alive && w2.demigods[r.ix()].alive && w2.seat_alive(0));
    assert!(private(&o, 0)["omens"].as_array().unwrap().iter().any(|x| x["god"] == "Death"));
    // A wiped player with nothing: a public quest of vigils (all the fallen can do), then it rises owing Death.
    let mut t = table(22, json!({}));
    t.edit(|w| {
        fell(w, l, 1);
        fell(w, r, 1);
        purse(w, 0, 0, 0);
    });
    let o = t.will(0, "my guys come back to life!");
    let will = private(&o, 0)["wills"][0].clone();
    let parts = will["parts"].as_array().unwrap();
    assert_eq!(will["verdict"], "partial");
    assert!(parts.iter().all(|p| p["outcome"] == "quest" && p["why"].as_str().unwrap().contains("you hold 0 sparks and 0 bolts")), "{will}");
    let b = book(&t.w());
    assert_eq!(b.quests.len(), 2);
    for q in &b.quests {
        assert!(q.public && matches!(q.goal, Goal::Raise { .. }) && q.steps.iter().all(|s| matches!(s.kind, StepKind::Vigil { times: 2, .. })), "{q:?}");
    }
    assert_eq!(o.public["quests"].as_array().unwrap().len(), 2, "a raising's quest is public");
    assert!(o.public["dm_events"].as_array().unwrap().iter().any(|e| e["event"] == "quest.given"));
    // Any other order from the fallen is refused, with what they can do.
    let o = t.will(0, "go to Paris and punch the mayor");
    let will = &private(&o, 0)["wills"][0];
    assert_eq!(will["verdict"], "refused");
    assert!(will["parts"][0]["next"].as_str().unwrap().contains("plead with Death"), "{will}");
    // They can ask what the quest needs.
    let o = t.will(0, "what does my quest need?");
    let told = &private(&o, 0)["wills"][0]["parts"][0];
    assert_eq!((told["part"].as_str(), told["quests"].as_array().unwrap().len()), (Some("ask"), 2), "{told}");
    // The vigil: the hour comes twice (within three days), and they rise, each owing Death.
    let mut rose = false;
    for _ in 0..26 {
        let o = t.pass();
        if o.public["dm_events"].as_array().unwrap().iter().any(|e| e["event"] == "demigod.raised") {
            rose = true;
            assert!(o.public["dm_events"].as_array().unwrap().iter().any(|e| e["event"] == "quest.done"));
        }
    }
    let w3 = t.w();
    assert!(rose && w3.demigods[l.ix()].alive | w3.demigods[r.ix()].alive, "{:?}", book(&w3).quests);
    assert!(book(&w3).raises.iter().all(|x| x.how.starts_with("quest")));
    let death = fn7_pantheon::load().id_of("death").unwrap();
    assert!(world::open_debt(&w3, Ref::Demigod(l), Some(death)) + world::open_debt(&w3, Ref::Demigod(r), Some(death)) >= 0.99, "a debt to Death");
}

// ---- seals and curses ---------------------------------------------------------------------------------------------------

/// Cast `text` as seat 1 until the curse lands on its target (the d20 may weaken or turn it): returns the table and
/// the curse.
fn cursed(kind: CurseKind, text: &str, extra: Value, prep: impl Fn(&mut World)) -> (Table, curses::Curse, RoundOutcome) {
    for seed in 400..440 {
        let mut t = table(seed, extra.clone());
        t.edit(|w| {
            purse(w, 1, 5, 5);
            prep(w);
        });
        let target = mine(&t.w(), 0)[0];
        let o = t.will(1, text);
        let b = book(&t.w());
        if let Some(c) = b.curses.iter().find(|c| c.kind == kind && c.target == target && c.strength >= 2) {
            return (t, c.clone(), o);
        }
    }
    panic!("no curse landed in 40 seeds");
}

/// Do what the quest's current step asks, by the book: stand at the place (at the right hour), hold the ingredient,
/// offer the sparks...
fn satisfy(t: &mut Table, seat: u8, quest: u32) {
    for _ in 0..40 {
        let b = book(&t.w());
        let q = b.quests.iter().find(|q| q.id == quest).unwrap().clone();
        let Some(c) = q.current() else { return };
        if !q.open() {
            return;
        }
        let d = mine(&t.w(), seat).into_iter().find(|d| t.w().demigods[d.ix()].alive);
        let st = world::settings(&t.w());
        let mut say = String::new();
        match &q.steps[c].kind {
            StepKind::Hold { word } => say = format!("gather the {} for the quest", quests::ingredient_name(word).to_lowercase()),
            StepKind::Sparks { .. } => {
                t.edit(|w| purse(w, seat, 3, 5));
                say = "offer the sparks to the quest".into();
            }
            StepKind::Shrine => say = "raise a shrine to me here".into(),
            StepKind::At { req, phase, .. } => {
                let w = t.w();
                let to: LonLat = match req {
                    PlaceReq::Shrine { of } => w.dm.shrines.iter().find(|s| s.demigod == *of).map(|s| s.at).unwrap(),
                    PlaceReq::Settlement { level } => w.dm.settlements.iter().find(|s| s.level >= *level).map(|s| w.places[s.place.ix()].center).unwrap(),
                    PlaceReq::Fault => [-122.4, 37.7],
                    PlaceReq::Coast => [-9.3, 38.7],
                    PlaceReq::Point { at, .. } => *at,
                };
                assert!(quests::place_ok(&w, req, to), "{req:?} at {to:?}");
                let d = d.unwrap();
                // Wait for the hour, standing there.
                let now = crate::dm::phase_of(t.round, st.beats_per_day).1;
                if phase.as_deref().is_some_and(|p| p != now) {
                    t.edit(|w| w.demigods[d.ix()].pos = to);
                    t.pass();
                    continue;
                }
                t.edit(|w| w.demigods[d.ix()].pos = to);
            }
            StepKind::Vigil { .. } => {}
            StepKind::Army { at, .. } => {
                let at = *at;
                t.edit(|w| {
                    let own: Vec<DemigodId> = w.bodies_of_seat(seat).map(|d| d.id).collect();
                    for c in w.clusters.iter_mut() {
                        if let Some(a) = c.army.as_mut().filter(|a| own.contains(&a.demigod)) {
                            a.pos = at;
                        }
                    }
                });
            }
            StepKind::Encounter | StepKind::Duel => panic!("the test tables place no encounters and fight no duels: {:?}", q.steps[c]),
        }
        t.will(seat, &say);
    }
    panic!("quest {quest} not done: {:?}", book(&t.w()).quests);
}

#[test]
fn a_curse_has_a_public_face_a_price_and_always_a_counter() {
    let (t, c, o) = cursed(CurseKind::Wither, "curse Chris's Left Hand with withering", json!({}), |_| {});
    let w = t.w();
    let caster = mine(&w, 1)[0];
    assert_eq!((c.owner, c.owner_seat, c.paid_sparks), (caster, 1, 5), "5 sparks");
    assert!(matches!((c.strength, c.until_beat), (2, Some(14)) | (3, Some(20))), "12 beats (18 on a crit): {c:?}");
    assert_eq!((w.demigods[caster.ix()].bolts, w.demigods[caster.ix()].sparks), (4, 0), "Time's toll took a bolt; the curse 5 sparks");
    // Always a counter: a quest for the bearer, as long as the curse is strong.
    let b = book(&w);
    let q = b.quests.iter().find(|q| q.id == c.quest).expect("the counter-quest");
    assert_eq!((q.seat, q.demigod, q.steps.len(), q.public), (0, c.target, c.strength as usize, false));
    assert!(matches!(q.goal, Goal::BreakCurse { curse, .. } if curse == c.id));
    // The public face: the mark, never the owner or the steps.
    let face = &o.public["curses"][0];
    assert_eq!((face["kind"].as_str(), face["target"].clone(), face["counter"].as_str()), (Some("wither"), json!(Ref::Demigod(c.target)), Some("a mystic quest")));
    assert!(face.get("owner").is_none() && face.get("quest").is_none(), "{face}");
    let ev = o.public["dm_events"].as_array().unwrap().iter().find(|e| e["event"] == "curse.cast").expect("curse.cast");
    assert!(ev.get("owner").is_none() && !ev.to_string().contains("Benito"), "{ev}");
    assert!(!o.public["wills"][0].to_string().contains("Benito"));
    let pv = crate::phases::public_view(&w);
    let piece = pv["demigods"].as_array().unwrap().iter().find(|d| d["ref"] == json!(Ref::Demigod(c.target))).unwrap();
    assert_eq!(piece["curses"][0]["kind"], "wither");
    assert!(o.public["quests"].as_array().unwrap().is_empty(), "a wither's counter is private");
    // The bearer's seat: who did it and the counter's steps; the owner's seat too; a bystander only the face.
    let on_me = &private(&o, 0)["curses"][0];
    assert_eq!((on_me["owner"].clone(), on_me["on_you"].as_bool(), on_me["quest"]["steps"].as_array().map(Vec::len)), (json!(Ref::Demigod(caster)), Some(true), Some(c.strength as usize)));
    assert!(private(&o, 0)["will_events"].as_array().unwrap().iter().any(|e| e["event"] == "quest.given"), "{}", private(&o, 0)["will_events"]);
    assert_eq!(private(&o, 1)["curses"][0]["yours"], true);
    let cast = &private(&o, 1)["wills"][0]["parts"][0];
    assert_eq!((cast["part"].as_str(), cast["curse"].as_str(), cast["god"].as_str()), (Some("curse"), Some("wither"), Some("Death")));
    assert!(cast["roll"]["target_dc"].as_i64().unwrap() >= 10 && cast["counter"].is_string());
    let sv = crate::phases::seat_view(&w, 0);
    assert_eq!(sv["quests"][0]["ref"], json!(quests::qref(c.quest)));
    assert!(sv["quests"][0]["steps"][0]["done"] == false);
    // It can't be paid: no curse, and why.
    let mut t2 = table(400, json!({ "will_debt_cap": 0.0 }));
    t2.edit(|w| purse(w, 1, 0, 2));
    let o = t2.will(1, "curse Chris's Left Hand with withering");
    let p = &private(&o, 1)["wills"][0]["parts"][0];
    assert_eq!(p["outcome"], "unpaid");
    assert!(p["result"].as_str().unwrap().contains("5 sparks") && book(&t2.w()).curses.is_empty(), "{p}");
    assert_eq!(t2.w().demigods[mine(&t2.w(), 1)[0].ix()].sparks, 2, "nothing was taken");
}

#[test]
fn wither_blocks_healing_until_its_quest_is_done() {
    let (mut t, c, _) = cursed(CurseKind::Wither, "curse Chris's Left Hand with withering", json!({ "curse_beats": 200 }), |w| {
        let d = w.bodies_of_seat(0).next().unwrap().id;
        w.demigods[d.ix()].health = 4.0;
    });
    let d = c.target;
    t.edit(|w| purse(w, 0, 3, 5));
    // The will says why, and what would work.
    let name = t.w().demigods[d.ix()].name.clone();
    let o = t.will(0, &format!("heal {name}"));
    let p = &private(&o, 0)["wills"][0]["parts"][0];
    assert_eq!(p["outcome"], "blocked");
    assert!(p["why"].as_str().unwrap().contains("withered") && p["quest"]["steps"].as_array().is_some_and(|s| !s.is_empty()), "{p}");
    assert_eq!(t.w().demigods[d.ix()].health, 4.0);
    // Every heal withers: a shrine's, a god's, gear's (anything entering a merge).
    t.edit(|w| {
        let p = Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Heal { who: Ref::Demigod(d), amount: 2.0 },
            act: "a shrine mends it".into(),
            actor: Ref::Demigod(d),
            magnitude: 2,
            valence: 1,
            witnesses: vec![],
            cause: None,
        };
        crate::merge::run(w, vec![p], &fn7_pantheon::load(), 3, 4);
    });
    assert_eq!(t.w().demigods[d.ix()].health, 4.0);
    // The counter: do the steps; the curse breaks (publicly), and healing works again.
    satisfy(&mut t, 0, c.quest);
    let b = book(&t.w());
    let q = b.quests.iter().find(|q| q.id == c.quest).unwrap();
    assert!(q.done_beat.is_some(), "{q:?}");
    let broken = b.curses.iter().find(|x| x.id == c.id).unwrap();
    assert_eq!(broken.broken_how, "countered");
    assert!(t.w().dm.memory.iter().any(|e| e.kind == "curse.broken" && e.public));
    assert!(t.w().dm.memory.iter().any(|e| e.kind == "quest.done" && !e.public), "a private quest's facts stay private");
    t.edit(|w| purse(w, 0, 3, 5));
    let mut healed = false;
    for _ in 0..6 {
        let o = t.will(0, &format!("heal {name}"));
        let p = &private(&o, 0)["wills"][0]["parts"][0];
        assert_ne!(p["outcome"], "blocked");
        healed |= t.w().demigods[d.ix()].health > 4.0;
    }
    assert!(healed);
}

#[test]
fn a_sealed_corpse_stays_down_until_the_seal_is_broken() {
    let (mut t, c, o) = cursed(CurseKind::Seal, "no resurrection for Chris's Left Hand", json!({}), |w| {
        let d = w.bodies_of_seat(0).next().unwrap().id;
        fell(w, d, 1);
    });
    let d = c.target;
    assert_eq!(c.until_beat, None, "a seal holds until countered");
    assert!(o.public["quests"].as_array().unwrap().iter().any(|q| q["ref"] == json!(quests::qref(c.quest))), "a seal's counter is public");
    // The plea is refused, with the seal and its counter.
    t.edit(|w| purse(w, 0, 5, 5));
    let o = t.will(0, "bring my Left Hand back to life");
    let p = &private(&o, 0)["wills"][0]["parts"][0];
    assert_eq!(p["outcome"], "sealed");
    assert!(p["quest"]["steps"].as_array().is_some_and(|s| !s.is_empty()) && p["curse"]["kind"] == "seal", "{p}");
    assert!(!t.w().demigods[d.ix()].alive);
    assert_eq!(t.w().demigods[mine(&t.w(), 0)[1].ix()].sparks, 5, "Death took nothing");
    // No other road around it: Death's own raising withers on the seal too.
    t.edit(|w| {
        let p = Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Revive { who: Ref::Demigod(d) },
            act: "Death raises it".into(),
            actor: Ref::Demigod(d),
            magnitude: 8,
            valence: 3,
            witnesses: vec![],
            cause: None,
        };
        crate::merge::run(w, vec![p], &fn7_pantheon::load(), 3, 4);
    });
    assert!(!t.w().demigods[d.ix()].alive);
    // The sibling does the seal's quest; then the plea works.
    satisfy(&mut t, 0, c.quest);
    assert!(book(&t.w()).curses[0].broken_beat.is_some());
    t.edit(|w| purse(w, 0, 5, 5));
    let o = t.will(0, "bring my Left Hand back to life");
    assert_eq!(private(&o, 0)["wills"][0]["parts"][0]["outcome"], "raised", "{}", private(&o, 0)["wills"][0]);
    assert!(t.w().demigods[d.ix()].alive);
}

#[test]
fn silence_bind_and_hex_do_what_they_say() {
    // Silence: the bearer's player pays double.
    let (mut t, c, _) = cursed(CurseKind::Silence, "silence Chris's Left Hand", json!({}), |w| {
        for d in w.demigods.iter_mut().filter(|d| d.seat == 0) {
            d.health = 4.0;
        }
    });
    t.edit(|w| purse(w, 0, 0, 10));
    let o = t.will(0, "heal me");
    assert_eq!(private(&o, 0)["wills"][0]["parts"][0]["paid"]["price_sparks"], 4, "2 doubled");
    // Bind: it can't be summoned, and the refusal names the counter.
    let (t, c2, _) = cursed(CurseKind::Bind, "bind Chris's Left Hand", json!({}), |_| {});
    let w = t.w();
    let st = world::settings(&w);
    let order = actions::SummonOrder { demigod: c2.target, target: [10.0, 10.0], cell: None, seq: 0 };
    assert_eq!(crate::summon::check(&w, 0, &order, 3, &st), Err(format!("bound:{}", quests::qref(c2.quest))));
    let free = actions::SummonOrder { demigod: mine(&w, 0)[1], ..order.clone() };
    assert_eq!(crate::summon::check(&w, 0, &free, 3, &st), Ok(()));
    // Hex: minus its strength on the bearer's d20s.
    let (t, c3, _) = cursed(CurseKind::Hex, "hex Chris's Left Hand", json!({}), |_| {});
    let w = t.w();
    assert_eq!(arms::act_mod(&w, c3.target, "attack", 3, &world::settings(&w)), -(c3.strength as i32));
    // A curse fades when its time is up (curse.broken, faded), and its counter-quest lapses as moot.
    let mut t = t;
    for _ in 0..22 {
        t.pass();
    }
    let b = book(&t.w());
    assert_eq!(b.curses[0].broken_how, "faded");
    assert_eq!(b.quests.iter().find(|q| q.id == c3.quest).unwrap().failed.as_deref(), Some("moot"));
    let _ = c;
}

#[test]
fn a_god_tier_curse_and_the_caps() {
    // Through Death: 15 sparks, strength 3, until countered, three steps.
    for seed in 500..520 {
        let mut t = table(seed, json!({}));
        // Four bolts: one is Time's toll for the order, three break into the 15 sparks.
        t.edit(|w| purse(w, 1, 4, 0));
        let o = t.will(1, "Death, wither Chris's Left Hand forever");
        let b = book(&t.w());
        let p = private(&o, 1)["wills"][0]["parts"][0].clone();
        assert_eq!(p["paid"]["price_sparks"], 15);
        let Some(c) = b.curses.iter().find(|c| c.strength == 3) else { continue };
        assert_eq!((c.until_beat, c.god.is_some(), b.quests[c.quest as usize].steps.len()), (None, true, 3));
        assert_eq!(o.public["curses"][0]["god_tier"], true);
        // One of a kind on a target; two standing per player.
        t.edit(|w| purse(w, 1, 9, 9));
        let o = t.will(1, "curse Chris's Left Hand with withering");
        assert_eq!(private(&o, 1)["wills"][0]["parts"][0]["outcome"], "refused");
        t.will(1, "hex Chris's Right Hand");
        let o = t.will(1, "bind Chris's Right Hand");
        let p = &private(&o, 1)["wills"][0]["parts"][0];
        assert!(p["outcome"] == "refused" && p["result"].as_str().unwrap().contains("2 curses standing"), "{p}");
        return;
    }
    panic!("no god-tier curse landed");
}

// ---- quests -------------------------------------------------------------------------------------------------------------

#[test]
fn quests_are_generated_by_rules_and_checked_from_the_state() {
    let t = table(31, json!({}));
    let mut w = t.w();
    let st = world::settings(&w);
    let d = mine(&w, 0)[0];
    let curser = mine(&w, 1)[0];
    let mut b = Book::default();
    let mut kinds = std::collections::BTreeSet::new();
    for k in 0..60u32 {
        let id = quests::give(&mut w, &mut b, 2 + k % 5, quests::Ask { seat: 0, demigod: d, goal: Goal::Heal { amount: 1.0 }, god: GodId(1), difficulty: 1 + (k % 3) as u8, curser: Some(curser), public: false, expires_beat: Some(30) }, &st);
        let q = &b.quests[id as usize];
        assert_eq!(q.steps.len(), 1 + (k % 3) as usize);
        assert!(q.text.starts_with("Nature has ruled: ") && q.text.contains("only then"), "{}", q.text);
        for s in &q.steps {
            assert!(!s.text.is_empty());
            kinds.insert(serde_json::to_value(&s.kind).unwrap()["kind"].as_str().unwrap().to_string());
        }
        // Seeded: the same quest again is the same quest.
        let mut b2 = Book { quests: b.quests[..id as usize].to_vec(), ..Default::default() };
        let mut w2 = w.clone();
        w2.dm.memory.truncate(w2.dm.memory.len() - 1);
        let again = quests::give(&mut w2, &mut b2, 2 + k % 5, quests::Ask { seat: 0, demigod: d, goal: Goal::Heal { amount: 1.0 }, god: GodId(1), difficulty: 1 + (k % 3) as u8, curser: Some(curser), public: false, expires_beat: Some(30) }, &st);
        assert_eq!(b2.quests[again as usize], *q);
    }
    for k in ["at", "hold", "sparks", "vigil", "shrine"] {
        assert!(kinds.contains(k), "{k} never generated: {kinds:?}");
    }
    assert!(w.dm.memory.iter().filter(|e| e.kind == "quest.given" && !e.public).count() == 60, "remembered, privately");
    // Places: the engine can verify each kind.
    assert!(quests::place_ok(&w, &PlaceReq::Fault, [-122.4, 37.7]) && !quests::place_ok(&w, &PlaceReq::Fault, [20.0, 5.0]));
    assert!(quests::is_coast(&w, [-9.3, 38.7]) && !quests::is_coast(&w, [60.0, 60.0]) && !quests::is_coast(&w, [-150.0, 20.0]));
    let s = w.dm.settlements.iter().max_by_key(|s| s.level).unwrap().clone();
    let c = w.places[s.place.ix()].center;
    assert!(quests::place_ok(&w, &PlaceReq::Settlement { level: s.level }, c) && !quests::place_ok(&w, &PlaceReq::Settlement { level: s.level + 1 }, c));
}

/// The designer's end-to-end example: wings of a butterfly, burned at dawn on the shrine of the one who
/// cursed you".
#[test]
fn butterfly_wings_burned_at_dawn_on_the_cursers_shrine() {
    let mut t = table(33, json!({ "curse_beats": 200 }));
    let (d, curser) = (mine(&t.w(), 0)[0], mine(&t.w(), 1)[0]);
    // The curser raises a shrine (a will through the ordinary route).
    t.will(1, "raise a shrine to me here");
    let shrine = t.w().dm.shrines.iter().find(|s| s.demigod == curser).expect("the curser's shrine").at;
    // A hex with exactly this counter (written by hand, as the generator would).
    t.edit(|w| {
        let mut b = book(w);
        b.curses.push(curses::Curse { id: 0, kind: CurseKind::Hex, owner: curser, owner_seat: 1, target: d, strength: 2, cast_beat: 3, until_beat: None, paid_sparks: 5, god: None, quest: 0, broken_beat: None, broken_how: String::new() });
        b.quests.push(quests::Quest {
            id: 0, seat: 0, helpers: vec![], demigod: d, goal: Goal::BreakCurse { curse: 0, curse_kind: CurseKind::Hex }, god: GodId(4),
            steps: vec![
                quests::Step { kind: StepKind::Hold { word: "butterfly".into() }, text: "Hold the wings of a butterfly.".into(), done_beat: None, progress: String::new(), near: None },
                quests::Step { kind: StepKind::At { req: PlaceReq::Shrine { of: curser }, phase: Some("dawn".into()), offer: Some("butterfly".into()) }, text: "Burn them at dawn on the shrine of the one who cursed you.".into(), done_beat: None, progress: String::new(), near: None },
            ],
            text: "the wings of a butterfly that has crossed the Sunda Strait, burned at dawn on the shrine of the one who cursed you".into(),
            given_beat: 3, since: 3, expires_beat: None, done_beat: None, failed: None, failed_beat: None, public: false, waiting: String::new(),
        });
        save(w, &b);
    });
    // The will asks; the answer has the steps and their progress.
    let o = t.will(0, "how do I break the curse on me?");
    let told = &private(&o, 0)["wills"][0]["parts"][0];
    assert!(told["result"].as_str().unwrap().contains("[to do] Hold the wings of a butterfly."), "{told}");
    assert_eq!(told["curses"][0]["kind"], "hex");
    // Gather the wings (the item engine's odd ingredient), until the d20 allows.
    for _ in 0..4 {
        let o = t.will(0, "catch a butterfly and take its wings for the quest");
        if private(&o, 0)["wills"][0]["parts"][0]["outcome"] == "gathered" {
            break;
        }
    }
    let w = t.w();
    let wings = quests::held_with(&w, d, "butterfly").expect("the wings");
    let q = &book(&w).quests[0];
    assert!(q.steps[0].done_beat.is_some() && q.steps[1].done_beat.is_none());
    assert!(q.steps[1].progress.contains("none of your demigods stands there"), "{}", q.steps[1].progress);
    // Stand on the shrine: not at dawn, nothing; at dawn the wings burn and the hex breaks.
    t.edit(|w| w.demigods[d.ix()].pos = shrine);
    let st = world::settings(&t.w());
    let mut done = None;
    for _ in 0..10 {
        let beat = t.round;
        let o = t.pass();
        let b = book(&t.w());
        if b.quests[0].done_beat.is_some() {
            assert_eq!(crate::dm::phase_of(beat, st.beats_per_day).1, "dawn", "only at dawn");
            done = Some(o);
            break;
        }
        assert!(b.quests[0].steps[1].progress.contains("not dawn"), "{}", b.quests[0].steps[1].progress);
    }
    let o = done.expect("dawn came");
    let w = t.w();
    assert_eq!(w.items[wings.ix()].ext.state, ItemState::Destroyed, "burned");
    assert_eq!(book(&w).curses[0].broken_how, "countered");
    let mine_events = private(&o, 0)["will_events"].as_array().unwrap().clone();
    assert!(mine_events.iter().any(|e| e["event"] == "quest.step") && mine_events.iter().any(|e| e["event"] == "quest.done"), "{mine_events:?}");
    assert!(private(&o, 1)["will_events"].as_array().unwrap().is_empty(), "not the curser's business");
    assert!(o.public["dm_events"].as_array().unwrap().iter().any(|e| e["event"] == "curse.broken"), "the breaking is public");
    assert!(o.public["curses"].as_array().unwrap().is_empty());
}

#[test]
fn a_failed_heals_quest_completes_the_heal_and_lapses_if_ignored() {
    // Find a seed whose heal fails (a token and a quest).
    for seed in 600..640 {
        let mut t = table(seed, json!({ "quest_expire_beats": 6 }));
        let d = mine(&t.w(), 0)[0];
        t.edit(|w| {
            w.demigods[d.ix()].health = 3.0;
            purse(w, 0, 0, 5);
        });
        let name = t.w().demigods[d.ix()].name.clone();
        let o = t.will(0, &format!("heal {name}"));
        let p = private(&o, 0)["wills"][0]["parts"][0].clone();
        if p["outcome"] != "partial" {
            continue;
        }
        let q = book(&t.w()).quests[0].clone();
        let Goal::Heal { amount } = q.goal else { panic!() };
        assert_eq!(q.expires_beat, Some(8));
        // Ignored: it lapses, and says so.
        let mut t2 = Table { p: FinalNight7Plugin::new(), state: t.state.clone(), round: t.round };
        for _ in 0..8 {
            t2.pass();
        }
        assert_eq!(book(&t2.w()).quests[0].failed.as_deref(), Some("expired"));
        assert!(t2.w().dm.memory.iter().any(|e| e.kind == "quest.failed"));
        // Done: the heal completes.
        let before = t.w().demigods[d.ix()].health;
        satisfy(&mut t, 0, q.id);
        let b = book(&t.w());
        assert!(b.quests[0].done_beat.is_some(), "{:?}", b.quests[0]);
        assert!((t.w().demigods[d.ix()].health - before - amount).abs() < 0.3, "{} -> {} (+{amount})", before, t.w().demigods[d.ix()].health);
        return;
    }
    panic!("no heal failed in 40 seeds");
}

// ---- the laws -----------------------------------------------------------------------------------------------------------

#[test]
fn wills_replay_exactly_and_the_gate_keeps_old_rules() {
    let mut t = table(51, json!({}));
    t.edit(|w| {
        for d in w.demigods.iter_mut() {
            d.health = 5.0;
        }
        purse(w, 0, 2, 3);
        purse(w, 1, 2, 5);
    });
    let mut acts = quiet(&t.p, &t.state);
    acts[0] = actions::action(0, "heal and arm my demigods", "", None);
    acts[1] = actions::action(1, "curse Chris's Left Hand with a hex", "bless me", None);
    let run = |p: &FinalNight7Plugin, state: &Value| block(p.resolve_round(state, 2, &acts, &RoundContext::default(), &Failing, &NoProgress)).unwrap();
    let a = run(&t.p, &t.state);
    let b = run(&FinalNight7Plugin::new(), &t.state);
    assert_eq!(a.state, b.state, "seeded and in the state: a replay is exact");
    assert_eq!(a.public["dice"], b.public["dice"]);
    assert_eq!(a.per_seat, b.per_seat);
    let w = world::load(&a.state).unwrap();
    assert!(!w.dm.wills.is_null() && a.state["dm"]["wills"].is_object());
    // Every result is remembered: dice at least, quests and curses when they were given.
    assert!(w.dm.memory.iter().any(|e| e.kind == "dice.rolled" && e.query.as_deref().is_some_and(|q| q.starts_with("will:"))));
    assert!(w.dm.memory.iter().any(|e| e.kind == "curse.cast"));
    // The gate: without the room setting (a 0.8.1 recording) nothing of this runs and the state carries no book.
    let mut old = t.w();
    old.meta.settings.as_object_mut().unwrap().remove("wills_enabled");
    assert!(!on(&old) && !world::settings(&old).wills_enabled);
    let o = run(&FinalNight7Plugin::new(), &world::save(&old));
    assert!(o.state["dm"].get("wills").is_none(), "no book");
    let me = private(&o, 0);
    assert!(me.get("wills").is_none() && me.get("quests").is_none() && o.public.get("curses").is_none());
    // And on an older schema whatever the setting says.
    let mut s6 = t.w();
    s6.meta.schema = 6;
    assert!(!on(&s6) && !world::settings(&s6).wills_enabled);
}

/// The built-in AI uses wills and pursues quests sometimes (its own seeded stream), and a whole AI match with them
/// replays exactly.
#[test]
fn the_built_in_ai_wills_and_quests() {
    let p = FinalNight7Plugin::new();
    let settings = p.validate_settings(&json!({ "seed": 77, "beats_total": 40, "llm": false, "wills_ai_chance": 1.0, "players": 4 })).unwrap();
    let play = |p: &FinalNight7Plugin| {
        let mut state = block(p.on_match_start(&[], &settings, &Failing)).unwrap();
        let mut kinds = std::collections::BTreeMap::<String, u32>::new();
        for round in 1..=40u32 {
            state = block(p.on_round_start(&state, round, &Failing)).unwrap().state;
            let o = block(p.resolve_round(&state, round, &[], &RoundContext::default(), &Failing, &NoProgress)).unwrap();
            for e in o.public["dm_events"].as_array().into_iter().flatten() {
                *kinds.entry(e["event"].as_str().unwrap_or("").to_string()).or_default() += 1;
            }
            let c = block(p.commit_round(&o.state, round, &[], &Failing)).unwrap();
            state = c.state;
            if p.is_match_over(&state, round) {
                break;
            }
        }
        (state, kinds)
    };
    let (state, kinds) = play(&p);
    let w = world::load(&state).unwrap();
    let b = book(&w);
    let made = w.manifests.iter().filter(|m| m.verdict.reasons.iter().any(|r| r.starts_with("willed:"))).count();
    assert!(made > 0, "the AI armed itself");
    assert!(w.dm.memory.iter().any(|e| e.query.as_deref().is_some_and(|q| q.contains(":heal:"))), "the AI healed");
    assert!(!b.quests.is_empty() || !b.curses.is_empty(), "quests or curses came of it: {kinds:?}");
    assert_eq!(play(&FinalNight7Plugin::new()).0, state, "replay");
}

/// A god-tier power aimed at your own demigod arms it: god-blessed wonder gear with that god's signature (once per
/// god); Death arms its own instead of striking it.
#[test]
fn a_god_tier_power_arms_your_own_demigod() {
    let mut t = table(61, json!({}));
    let d = mine(&t.w(), 0)[0];
    t.edit(|w| purse(w, 0, 8, 5));
    let cast = |t: &Table, power: &str| {
        let mut acts = quiet(&t.p, &t.state);
        acts[0] = Action { fields: json!({ "cast": { "power": power, "tier": "god", "target": Ref::Demigod(d) } }), instant: true, seq: 1, ..Action::new(0, "") };
        block(t.p.resolve_round(&t.state, t.round, &acts, &RoundContext::default(), &Failing, &NoProgress)).unwrap()
    };
    let o = cast(&t, "god_death");
    let w2 = world::load(&o.state).unwrap();
    let death = fn7_pantheon::load().id_of("death").unwrap();
    let ruling = w2.rulings.iter().rev().find(|r| r.god == death).expect("Death ruled");
    assert_ne!(ruling.verdict, Verdict::Refuse, "the rules god grants a paid power");
    {
        assert_eq!(w2.demigods[d.ix()].health, 10.0, "Death arms its own instead of striking it");
        let gear = crate::items::items_of(&w2, d);
        assert_eq!(gear.len(), 1, "{gear:?}");
        assert_eq!((gear[0]["tier"].as_str(), gear[0]["blessed_by"].as_str(), gear[0]["power"]["id"].as_str()), (Some("wonder"), Some("Death"), Some("last_stand")), "{}", gear[0]);
        assert!(gear[0]["name"].as_str().unwrap().ends_with("of Death"));
        // Once per god: a second Pale Verdict on it adds nothing.
        t.state = block(t.p.on_round_start(&block(t.p.commit_round(&o.state, 2, &[], &Failing)).unwrap().state, 3, &Failing)).unwrap().state;
        t.round = 3;
        let o = cast(&t, "god_death");
        assert_eq!(crate::items::items_of(&world::load(&o.state).unwrap(), d).len(), 1);
    }
}
