//! Gear looks (state schema 11): loadout selection and its hash, human-only looks, the debounce and the caps, what
//! every snapshot shows, takeovers, item art keys, and the gate.

use fn7_model::manifest::{ItemKind, Tier};
use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::look::{self, loadout, loadout_hash};
use crate::testkit::Failing;
use crate::wills::forge_now;
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

/// A quiet match at the start of beat 2 (no encounters, no AI wills or duels: the tests drive everything).
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
    let acts: Vec<Action> = (0..4u8).map(|s| p.default_action(&state, s)).collect();
    let o = block(p.resolve_round(&state, 1, &acts, &RoundContext::default(), &Failing, &NoProgress)).unwrap();
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

impl Table {
    fn w(&self) -> World {
        world::load(&self.state).unwrap()
    }
    fn edit(&mut self, f: impl FnOnce(&mut World)) {
        let mut w = self.w();
        f(&mut w);
        self.state = world::save(&w);
    }
    /// Give `d` a piece of gear now (as a will would, without the dice).
    fn give(&mut self, d: DemigodId, kind: ItemKind, tier: Tier, name: &str, god: Option<&str>) {
        let beat = self.round;
        self.edit(|w| {
            let pantheon = fn7_pantheon::load();
            let st = world::settings(w);
            forge_now(w, beat, d, kind, tier, name, god.and_then(|g| pantheon.id_of(g)), "a test", &pantheon, &st).unwrap();
        });
    }
    /// Resolve this beat with seat 0 saying `text`, commit, start the next.
    fn beat(&mut self, text: &str) -> RoundOutcome {
        let mut acts: Vec<Action> = (0..4u8).map(|s| self.p.default_action(&self.state, s)).collect();
        if !text.is_empty() {
            acts[0] = actions::action(0, text, "", None);
        }
        let o = block(self.p.resolve_round(&self.state, self.round, &acts, &RoundContext::default(), &Failing, &NoProgress)).unwrap();
        let s = block(self.p.commit_round(&o.state, self.round, &[], &Failing)).unwrap().state;
        self.round += 1;
        self.state = block(self.p.on_round_start(&s, self.round, &Failing)).unwrap().state;
        o
    }
    fn public(&self) -> Value {
        self.p.public_view(&self.state)
    }
    fn look(&self, d: DemigodId) -> Value {
        self.public()["demigods"].as_array().unwrap().iter().find(|x| x["ref"] == json!(Ref::Demigod(d))).unwrap()["look"].clone()
    }
}

fn mine(w: &World, seat: u8) -> Vec<DemigodId> {
    w.bodies_of_seat(seat).map(|d| d.id).collect()
}

fn changes(o: &RoundOutcome) -> Vec<Value> {
    o.public["looks"].as_array().cloned().unwrap_or_default()
}

#[test]
fn a_new_match_is_schema_11_and_looks_are_on() {
    let t = table(3, json!({}));
    let w = t.w();
    assert_eq!((w.meta.schema, w.look_rules(), look::on(&w)), (STATE_SCHEMA, true, true));
    assert!(w.dm.looks.is_null(), "nothing is stored until someone holds something");
    // Everyone has a look: an empty loadout and the base images.
    for d in t.public()["demigods"].as_array().unwrap() {
        let l = &d["look"];
        assert_eq!((l["loadout"].as_array().unwrap().len(), l["loadout_hash"].as_str(), l["gear"].as_bool()), (0, Some(""), Some(false)), "{l}");
        assert!(l["portrait_subject"].is_null() && l["base_portrait"] == d["portrait_prompt"] && l["base_diorama"] == d["diorama_prompt"]);
    }
}

#[test]
fn the_loadout_is_the_best_per_slot_plus_major_extras_in_a_stable_order() {
    let mut t = table(5, json!({}));
    let a = mine(&t.w(), 0)[0];
    t.give(a, ItemKind::Weapon, Tier::Trinket, "Oak Club", None);
    t.give(a, ItemKind::Relic, Tier::Artifact, "Gold Talisman", None);
    t.give(a, ItemKind::Weapon, Tier::Relic, "Butterfly Wing Blade", Some("nature"));
    t.give(a, ItemKind::Armor, Tier::Artifact, "Moonsteel Plate", None);
    t.give(a, ItemKind::Weapon, Tier::Artifact, "Obsidian Dagger", None);
    t.give(a, ItemKind::Structure, Tier::Trinket, "Shrine of Small Hours", None);
    t.give(a, ItemKind::Structure, Tier::Wonder, "Lantern Temple", None);
    t.give(a, ItemKind::Other, Tier::Trinket, "Lucky Coin", None);
    let w = t.w();
    let lo = loadout(&w, a, 2);
    let names: Vec<(&str, &str)> = lo.iter().map(|e| (e.role, e.gear.name.as_str())).collect();
    // Weapon, armour, charm (the higher tier wins the slot), then the extras: the wonder first; the blessed blade
    // already has the weapon slot, so the next is the first of a kind not listed yet.
    assert_eq!(names, vec![("weapon", "Butterfly Wing Blade"), ("armor", "Moonsteel Plate"), ("charm", "Gold Talisman"),
                           ("extra", "Lantern Temple"), ("extra", "Lucky Coin")]);
    assert_eq!(loadout(&w, a, 0).len(), 3, "no extras when the setting is 0");
    let g = &lo[0].gear;
    assert_eq!((g.material.as_str(), g.tier, g.stem.as_str(), g.blessing.as_str()), ("butterfly-wing", Tier::Relic, "butterfly wing blade", "nature"),
               "the descriptor is normalized");
    assert_eq!(g.key(), "weapon|butterfly-wing|relic|butterfly wing blade|nature");
    // An item lying in a loot pile is not carried.
    let mut w2 = w.clone();
    let blade = lo[0].item;
    crate::dm::drop_loot_at(&mut w2, 2, a, vec![blade], "a test", w.demigods[a.ix()].pos);
    assert_eq!(loadout(&w2, a, 2)[0].gear.name, "Obsidian Dagger", "the next best weapon takes the slot");
}

#[test]
fn the_same_gear_is_the_same_hash_in_any_match_whatever_the_item_ids() {
    let mut t1 = table(5, json!({}));
    let mut t2 = table(99, json!({}));
    let a1 = mine(&t1.w(), 0)[0];
    let a2 = mine(&t2.w(), 1)[1];
    // Another match, another seat and body, other item ids (junk made first and held by someone else), another order.
    let other = mine(&t2.w(), 3)[0];
    t2.give(other, ItemKind::Other, Tier::Trinket, "Bent Spoon", None);
    t2.give(other, ItemKind::Weapon, Tier::Trinket, "Oak Club", None);
    t1.give(a1, ItemKind::Weapon, Tier::Relic, "Butterfly Wing Blade", None);
    t1.give(a1, ItemKind::Armor, Tier::Artifact, "Moonsteel Plate", None);
    t2.give(a2, ItemKind::Armor, Tier::Artifact, "Moonsteel Plate", None);
    t2.give(a2, ItemKind::Weapon, Tier::Relic, "Butterfly Wing Blade", None);
    let (w1, w2) = (t1.w(), t2.w());
    let (l1, l2) = (loadout(&w1, a1, 2), loadout(&w2, a2, 2));
    assert_ne!(l1[0].item, l2[0].item, "different item ids");
    assert_ne!(w1.meta.seed, w2.meta.seed);
    assert_eq!(loadout_hash(&l1), loadout_hash(&l2));
    assert_eq!(loadout_hash(&l1).len(), 64);
    assert_eq!(l1[0].gear.art_key(), l2[0].gear.art_key(), "and the same item picture");
    assert_eq!(look::item_subject(&l1[0].gear), look::item_subject(&l2[0].gear));
    // Any change of a descriptor is another hash: the tier, the blessing, a third item.
    t2.give(a2, ItemKind::Relic, Tier::Trinket, "Gold Ring", None);
    assert_ne!(loadout_hash(&loadout(&t2.w(), a2, 2)), loadout_hash(&l1));
    let mut t3 = table(5, json!({}));
    let a3 = mine(&t3.w(), 0)[0];
    t3.give(a3, ItemKind::Weapon, Tier::Relic, "Butterfly Wing Blade", Some("time"));
    t3.give(a3, ItemKind::Armor, Tier::Artifact, "Moonsteel Plate", None);
    assert_ne!(loadout_hash(&loadout(&t3.w(), a3, 2)), loadout_hash(&l1));
    assert_eq!(loadout_hash(&[]), "", "an empty loadout is the base look");
}

#[test]
fn a_humans_major_acquisition_is_a_new_look_and_the_ais_is_not() {
    let mut t = table(7, json!({}));
    let (a, bot) = (mine(&t.w(), 0)[0], mine(&t.w(), 2)[0]);
    t.give(a, ItemKind::Weapon, Tier::Relic, "Butterfly Wing Blade", Some("nature"));
    t.give(bot, ItemKind::Weapon, Tier::Relic, "Obsidian Spear", None);
    let o = t.beat("");
    let ev = changes(&o);
    assert_eq!(ev.len(), 1, "only the human's demigod changes its look: {ev:?}");
    let e = &ev[0];
    assert_eq!((e["event"].as_str(), e["demigod"].as_str(), e["image_change"].as_bool()), (Some("look.changed"), Some("dg:0"), Some(true)));
    assert_eq!(e["gained"]["name"], "Butterfly Wing Blade");
    assert_eq!(e["text"], "Chris's Left Hand now carries the Butterfly Wing Blade");
    assert!(e["gained"]["image_prompt"].as_str().unwrap().contains("butterfly-wing membrane"), "a relic has its picture: {e}");
    assert_eq!(e["loadout"][0]["slot"], "weapon");
    // The same object is in the DM's events (not the short memory form).
    let dm: Vec<&Value> = o.public["dm_events"].as_array().unwrap().iter().filter(|x| x["event"] == "look.changed").collect();
    assert_eq!(dm.len(), 1);
    assert_eq!(dm[0], e);
    // The human's look: the gear subjects, on top of the unchanged identity subjects.
    let l = t.look(a);
    let (ps, ds) = (l["portrait_subject"].as_str().unwrap(), l["diorama_subject"].as_str().unwrap());
    let base = l["base_portrait"].as_str().unwrap();
    assert!(ps.starts_with(base), "the identity sheet is unchanged: {ps}");
    assert!(ps.contains("newly geared: gripped in the right hand, a blade of iridescent butterfly-wing membrane veined like stained glass, ornate, glowing steadily, wound with living vines and small green leaves; the same face"), "{ps}");
    assert!(ds.starts_with(l["base_diorama"].as_str().unwrap()) && ds.contains("raised in its right hand, a blade of"), "{ds}");
    assert!(!ps.contains("it:") && !ps.contains("dg:") && !ps.contains('+'), "no ids and no stats");
    assert_eq!((l["gear"].as_bool(), l["since_beat"].as_u64(), l["looks_made"].as_u64(), l["looks_cap"].as_u64()), (Some(true), Some(2), Some(1), Some(4)));
    assert_eq!(l["loadout_hash"], l["image_loadout_hash"]);
    // The AI's: its loadout is listed, its images stay the base ones.
    let lb = t.look(bot);
    assert_eq!(lb["loadout"][0]["name"], "Obsidian Spear");
    assert!(lb["portrait_subject"].is_null() && lb["gear"] == false && lb["image_loadout_hash"] == "", "{lb}");
    assert_eq!(lb["loadout_hash"].as_str().unwrap().len(), 64);
    // The state: a record each; only the human's has an image identity.
    let b = look::book(&t.w());
    let (ra, rb) = (b.looks.iter().find(|x| x.demigod == a).unwrap(), b.looks.iter().find(|x| x.demigod == bot).unwrap());
    assert_eq!((ra.loadout.len(), ra.portrait.len(), ra.diorama.len(), ra.since_beat, ra.made.len()), (64, 64, 64, 2, 1));
    assert_eq!((rb.held.len(), rb.loadout.as_str(), rb.portrait.as_str(), rb.made.len()), (64, "", "", 0));
    // It is remembered (the DM's place x time memory), once.
    assert_eq!(t.w().dm.memory.iter().filter(|m| m.kind == "look.changed").count(), 1);
    // A quiet beat changes nothing.
    let before = t.w().dm.looks.clone();
    assert!(changes(&t.beat("")).is_empty());
    assert_eq!(t.w().dm.looks, before);
}

#[test]
fn a_human_who_takes_a_seat_over_counts_from_then() {
    let mut t = table(8, json!({}));
    let bot = mine(&t.w(), 2)[0];
    t.give(bot, ItemKind::Armor, Tier::Relic, "Dragonscale Plate", None);
    assert!(changes(&t.beat("")).is_empty(), "the AI's demigod gets no look");
    let take = SeatTakeover { seat: 2, display_name: "Dana".into(), round: t.round, new_seat: false, previous_name: Some("Vex".into()) };
    t.state = t.p.on_seat_takeover(&t.state, &take).expect("the seat changes hands");
    assert!(t.look(bot)["portrait_subject"].is_null(), "not before the beat ends");
    let ev = changes(&t.beat(""));
    assert_eq!(ev.len(), 1, "{ev:?}");
    assert_eq!((ev[0]["demigod"].as_str(), ev[0]["name"].as_str(), ev[0]["gained"]["name"].as_str()), (Some("dg:2"), Some("Dana's Left Hand"), Some("Dragonscale Plate")));
    let l = t.look(bot);
    assert!(l["portrait_subject"].as_str().unwrap().starts_with("Dana's Left Hand, a demigod"), "its new identity, with the gear: {l}");
    assert!(l["portrait_subject"].as_str().unwrap().contains("overlapping ember-red dragon scales"));
}

#[test]
fn new_pictures_are_debounced_and_capped_and_the_newest_loadout_wins() {
    let mut t = table(9, json!({ "look_debounce_beats": 3, "look_images_per_demigod": 2 }));
    let a = mine(&t.w(), 0)[0];
    t.give(a, ItemKind::Weapon, Tier::Trinket, "Oak Club", None);
    let e = changes(&t.beat(""));
    assert_eq!((e.len(), e[0]["image_change"].as_bool(), e[0]["kept"].is_null()), (1, Some(true), true), "beat 2: the first look");
    let first = t.look(a);
    // Beat 3: armour. Too soon: announced, the picture stays.
    t.give(a, ItemKind::Armor, Tier::Artifact, "Moonsteel Plate", None);
    let e = changes(&t.beat(""));
    assert_eq!((e.len(), e[0]["image_change"].as_bool(), e[0]["kept"].as_str(), e[0]["gained"]["name"].as_str()), (1, Some(false), Some("debounced"), Some("Moonsteel Plate")));
    let l = t.look(a);
    assert_eq!(l["portrait_subject"], first["portrait_subject"], "the picture is still the club's");
    assert_eq!(l["loadout"].as_array().unwrap().len(), 2, "the loadout already lists both");
    assert_ne!(l["loadout_hash"], l["image_loadout_hash"]);
    // Beat 4: a charm too. Still waiting; announced once.
    t.give(a, ItemKind::Relic, Tier::Relic, "Gold Talisman", None);
    let e = changes(&t.beat(""));
    assert_eq!((e.len(), e[0]["kept"].as_str()), (1, Some("debounced")));
    // Beat 5 = 2 + 3: the wait is over; ONE new picture, of the newest loadout (all three pieces).
    let e = changes(&t.beat(""));
    assert_eq!((e.len(), e[0]["image_change"].as_bool()), (1, Some(true)), "{e:?}");
    let l = t.look(a);
    let ps = l["portrait_subject"].as_str().unwrap();
    assert!(ps.contains("oak") && ps.contains("moonsteel") && ps.contains("beaten gold"), "{ps}");
    assert_eq!((l["looks_made"].as_u64(), l["since_beat"].as_u64()), (Some(2), Some(5)));
    assert_eq!(l["loadout_hash"], l["image_loadout_hash"]);
    assert_eq!(l["fallback"].as_array().unwrap().len(), 1, "the club look stands in while the new one renders");
    assert_eq!(l["fallback"][0]["portrait_subject"], first["portrait_subject"]);
    assert!(changes(&t.beat("")).is_empty());
    // The cap (2): a better weapon is announced, the picture stays for the rest of the match.
    t.beat("");
    t.beat("");
    t.give(a, ItemKind::Weapon, Tier::Wonder, "Obsidian Greatsword", None);
    let e = changes(&t.beat(""));
    assert_eq!((e.len(), e[0]["kept"].as_str(), e[0]["gained"]["name"].as_str()), (1, Some("capped"), Some("Obsidian Greatsword")));
    for _ in 0..5 {
        assert!(changes(&t.beat("")).is_empty());
    }
    let l2 = t.look(a);
    assert_eq!(l2["portrait_subject"], l["portrait_subject"]);
    assert_eq!(l2["loadout"][0]["name"], "Obsidian Greatsword");
    assert_eq!(look::book(&t.w()).looks[0].made.len(), 2);
}

#[test]
fn losing_the_gear_goes_back_to_the_base_look_for_free_and_an_old_look_costs_nothing_again() {
    let mut t = table(10, json!({ "look_debounce_beats": 50, "look_images_per_demigod": 1 }));
    let a = mine(&t.w(), 0)[0];
    t.give(a, ItemKind::Weapon, Tier::Relic, "Gold Spear", None);
    t.beat("");
    let with = t.look(a);
    let spear = loadout(&t.w(), a, 2)[0].item;
    // It drops the spear: the base look again (no new picture, so neither the debounce nor the cap applies).
    t.edit(|w| {
        let pos = w.demigods[a.ix()].pos;
        crate::dm::drop_loot_at(w, 3, a, vec![spear], "a test", pos);
        // The test keeps the pile where nobody walks.
        w.dm.loot[0].at = [0.0, -89.0];
    });
    let e = changes(&t.beat(""));
    assert_eq!((e.len(), e[0]["image_change"].as_bool(), e[0]["gained"].is_null(), e[0]["text"].as_str()), (1, Some(true), true, Some("Chris's Left Hand stands bare-handed again")), "{e:?}");
    let l = t.look(a);
    assert!(l["portrait_subject"].is_null() && l["gear"] == false && l["image_loadout_hash"] == "");
    // It picks it up again: the same look as before (same subjects, so the same cached images), although the cap is 1.
    t.edit(|w| w.dm.loot[0].taken_beat = Some(4));
    let e = changes(&t.beat(""));
    assert_eq!((e.len(), e[0]["image_change"].as_bool()), (1, Some(true)), "{e:?}");
    assert_eq!(t.look(a)["portrait_subject"], with["portrait_subject"]);
    assert_eq!(look::book(&t.w()).looks[0].made.len(), 1);
    assert_eq!(crate::look::history_json(&t.w(), a).len(), 3);
}

#[test]
fn every_snapshot_shows_the_look_that_was_current_at_its_beat() {
    let mut t = table(12, json!({ "look_debounce_beats": 1 }));
    let a = mine(&t.w(), 0)[0];
    let mut states = vec![t.state.clone()]; // the start of beat 2: nothing yet
    t.give(a, ItemKind::Weapon, Tier::Artifact, "Obsidian Blade", None);
    t.beat("");
    states.push(t.state.clone()); // beat 3: the blade
    t.beat("");
    t.give(a, ItemKind::Armor, Tier::Relic, "Platinum Plate", Some("death"));
    t.beat("");
    states.push(t.state.clone()); // beat 5: blade and plate
    let look_at = |s: &Value| t.p.public_view(s)["demigods"][0]["look"].clone();
    let (l0, l1, l2) = (look_at(&states[0]), look_at(&states[1]), look_at(&states[2]));
    assert!(l0["portrait_subject"].is_null());
    assert_eq!((l1["since_beat"].as_u64(), l2["since_beat"].as_u64()), (Some(2), Some(4)));
    let (p1, p2) = (l1["portrait_subject"].as_str().unwrap(), l2["portrait_subject"].as_str().unwrap());
    assert!(p1.contains("obsidian") && !p1.contains("platinum"));
    assert!(p2.contains("obsidian") && p2.contains("pale mirror-bright platinum") && p2.contains("inlaid with pale bone"));
    assert_ne!(l1["image_loadout_hash"], l2["image_loadout_hash"]);
    // Asking the old snapshots again (a scrub, a fork, a replay) gives the same answers, byte for byte, and the
    // seat view agrees with the public one.
    assert_eq!(look_at(&states[1]), l1);
    assert_eq!(t.p.seat_view(&states[1], 0)["demigods"][0]["look"], l1);
    assert_eq!(t.p.seat_view(&states[2], 0)["demigods"][0]["look"]["portrait_subject"], l2["portrait_subject"]);
    // The record's subject hashes are the subjects' (what a media key is made from).
    let b = look::book(&world::load(&states[2]).unwrap());
    let r = &b.looks[0];
    assert_eq!((r.past.len(), r.past[0].since_beat, r.past[0].until_beat), (1, 2, 4));
    // The same match played again is the same state (the look step is rules only).
    let mut u = table(12, json!({ "look_debounce_beats": 1 }));
    u.give(a, ItemKind::Weapon, Tier::Artifact, "Obsidian Blade", None);
    u.beat("");
    assert_eq!(u.state, states[1]);
}

#[test]
fn a_will_arms_a_humans_demigods_and_their_looks_change() {
    let mut t = table(21, json!({}));
    t.edit(|w| {
        let d = mine(w, 0)[0];
        w.set_bolts(d, 6);
        w.set_sparks(d, 20);
    });
    let o = t.beat("arm my demigods with legendary butterfly wing blades");
    let ev = changes(&o);
    assert!(!ev.is_empty(), "an arming is a major acquisition: {:?}", o.public["dm_events"]);
    for e in &ev {
        assert_eq!(e["player"], "Chris");
        assert!(e["gained"]["name"].as_str().unwrap().contains("Blade"), "{e}");
        assert!(e["portrait_subject"].as_str().unwrap().contains("gripped in the right hand, a "));
    }
    // The private will result's item and the seat's items carry the same picture prompt as the look's loadout.
    let mine_ = o.per_seat.iter().find(|(s, _)| *s == 0).unwrap().1.clone();
    let part = mine_["wills"][0]["parts"].as_array().unwrap().iter().find(|p| p["item"].is_object()).expect("a forged item");
    let l = t.look(mine(&t.w(), 0)[0]);
    let in_look = l["loadout"].as_array().unwrap().iter().find(|x| x["item"] == part["item"]["ref"]).expect("the forged blade is in the loadout");
    assert!(in_look["image_prompt"].is_string(), "{in_look}");
    assert_eq!(in_look["image_prompt"], part["item"]["image_prompt"]);
    assert_eq!(in_look["art"], part["item"]["art"]);
}

#[test]
fn item_art_is_keyed_by_the_descriptor_capped_and_only_for_what_a_human_sees() {
    let mut t = table(13, json!({ "item_art_per_match": 3 }));
    let (a, a2, bot) = (mine(&t.w(), 0)[0], mine(&t.w(), 0)[1], mine(&t.w(), 2)[0]);
    t.give(bot, ItemKind::Weapon, Tier::Wonder, "Obsidian Spear", None); // the AI's: no picture
    t.give(a, ItemKind::Weapon, Tier::Trinket, "Struck Oak Club", None); // the human's first weapon: a picture
    t.give(a2, ItemKind::Weapon, Tier::Trinket, "Gold Knife", None); // the player's second weapon, a trinket: its icon
    t.give(a, ItemKind::Armor, Tier::Relic, "Moonsteel Plate", Some("memory")); // relic, blessed: a picture
    t.beat("");
    let w = t.w();
    let b = look::book(&w);
    assert_eq!(b.art.len(), 2, "{:?}", b.art);
    let item = |name: &str| crate::items::item_json(&w, w.items.iter().find(|i| i.name == name).unwrap());
    let club = item("Struck Oak Club");
    assert_eq!(club["image_kind"], "item");
    assert_eq!(club["image_prompt"], "a club of lightning-charred oak with glowing orange cracks in the grain: a demigod's weapon, plain and battered");
    assert_eq!(club["art"].as_str().unwrap().len(), 64);
    assert_eq!(club["icon"], "sword", "the icon stays");
    for lesser in ["Gold Knife", "Obsidian Spear"] {
        let v = item(lesser);
        assert!(v.get("image_prompt").is_none() && v.get("image_kind").is_none() && v["icon"].is_string(), "{v}");
    }
    // Everywhere an item is listed: the look's loadout, the story pegs, the seat's items.
    let pv = t.public();
    let d0 = &pv["demigods"][0];
    assert!(d0["look"]["loadout"][0]["image_prompt"].is_string());
    let pegs = d0["story"]["items"].as_array().unwrap();
    assert!(pegs.iter().all(|p| p["image_prompt"].is_string() && p["name"].is_string()), "{pegs:?}");
    let sv = t.p.seat_view(&t.state, 0);
    assert_eq!(sv["my_items"].as_array().unwrap().iter().filter(|i| i["image_prompt"].is_string()).count(), 2);
    // The queries (partial worlds) agree with the views: the same look and the same pictures.
    let globe = t.p.query(&t.state, None, "globe", &json!({})).unwrap();
    assert_eq!(globe["demigods"][0]["look"], pv["demigods"][0]["look"]);
    let region = t.p.query(&t.state, Some(0), "items", &json!({})).unwrap();
    assert!(region.to_string().contains("\"art\""), "game.items shows the pictures too");
        // The same descriptor is the same picture for whoever holds it later (the AI too: it costs nothing).
    t.give(bot, ItemKind::Armor, Tier::Relic, "Moonsteel Plate", Some("memory"));
    let w = t.w();
    let plates: Vec<Value> = w.items.iter().filter(|i| i.name == "Moonsteel Plate").map(|i| crate::items::item_json(&w, i)).collect();
    assert_eq!(plates.len(), 2);
    assert_eq!((plates[0]["art"].clone(), plates[0]["image_prompt"].clone()), (plates[1]["art"].clone(), plates[1]["image_prompt"].clone()));
    // The cap: a third picture, then no more this match.
    t.give(a, ItemKind::Relic, Tier::Relic, "Gold Talisman", None);
    t.give(a2, ItemKind::Armor, Tier::Wonder, "Dragonscale Mail", None);
    t.beat("");
    assert_eq!(look::book(&t.w()).art.len(), 3);
    // A relic lying in a loot pile is on the board for everyone: it would get a picture (were the cap not reached).
    let mut u = table(13, json!({}));
    let bot = mine(&u.w(), 2)[0];
    u.give(bot, ItemKind::Weapon, Tier::Relic, "Obsidian Spear", None);
    u.beat("");
    assert!(look::book(&u.w()).art.is_empty());
    u.edit(|w| {
        let it = w.items[0].id;
        crate::dm::drop_loot_at(w, 3, bot, vec![it], "a test", [10.0, -80.0]);
    });
    u.beat("");
    assert_eq!(look::book(&u.w()).art.len(), 1);
    let w = u.w();
    let loot = crate::dm::loot_under(&w, None);
    assert!(loot[0]["image_prompt"].as_str().unwrap().starts_with("a spear of black volcanic obsidian glass"), "{loot:?}");
}

#[test]
fn older_states_and_the_setting_off_keep_everything_as_it_was() {
    // The setting off: no look anywhere, no state, items as before.
    let mut t = table(14, json!({ "looks_enabled": false }));
    let a = mine(&t.w(), 0)[0];
    t.give(a, ItemKind::Weapon, Tier::Relic, "Gold Spear", None);
    let o = t.beat("");
    assert!(o.public.get("looks").is_none() && t.w().dm.looks.is_null());
    assert!(t.public()["demigods"][0].get("look").is_none());
    let w = t.w();
    assert!(crate::items::item_json(&w, &w.items[0])["image_prompt"].is_string(), "the 0.9.0 item prompt");
    // A schema-10 state with today's settings: off too (the state's schema gates it).
    let mut t = table(14, json!({}));
    t.edit(|w| w.meta.schema = 10);
    t.give(a, ItemKind::Weapon, Tier::Relic, "Gold Spear", None);
    let o = t.beat("");
    assert!(o.public.get("looks").is_none() && t.w().dm.looks.is_null() && t.public()["demigods"][0].get("look").is_none());
    // Settings recorded before looks read back off.
    let mut old = t.w().meta.settings.clone();
    old.as_object_mut().unwrap().remove("looks_enabled");
    assert!(!crate::settings::Settings::from_value(&old).looks_enabled);
}

/// The three example prompts of the report (run with `--nocapture` to print them).
#[test]
fn example_subjects() {
    let mut t = table(5, json!({}));
    let a = mine(&t.w(), 0)[0];
    t.give(a, ItemKind::Weapon, Tier::Relic, "Butterfly Wing Blade", Some("nature"));
    t.give(a, ItemKind::Armor, Tier::Artifact, "Moonsteel Plate", None);
    t.give(a, ItemKind::Relic, Tier::Relic, "Gold Talisman", Some("death"));
    t.beat("");
    let l = t.look(a);
    let w = t.w();
    let blade = look::gear_of(&w, &w.items[0]);
    println!("PORTRAIT SUBJECT: {}\n\nDIORAMA SUBJECT: {}\n\nITEM SUBJECT: {}", l["portrait_subject"].as_str().unwrap(), l["diorama_subject"].as_str().unwrap(), look::item_subject(&blade));
    assert!(l["portrait_subject"].as_str().unwrap().len() < 1200, "within the media layer's subject limit");
    assert!(l["diorama_subject"].as_str().unwrap().len() < 1200);
}

/// The recorded live case (match 1ccfab63, rules 0.8.2): "my demigods need crazy powerful equipment! do that! and heal
/// more!" gave one trinket by a god's whim; under rules 0.9.0 it armed nobody (no arming word). From schema 11 it is an
/// arming: both demigods get gear and a look. A schema-10 state reads it as before.
#[test]
fn the_designers_own_words_arm_their_demigods() {
    let text = "my demigods need crazy powerful equipment! do that! and heal more!";
    let mut t = table(77, json!({}));
    t.edit(|w| {
        let d = mine(w, 0)[0];
        w.set_bolts(d, 6);
    });
    let w = t.w();
    assert!(crate::wills::kinds_in(&w, &crate::wills::book(&w), 0, text).contains(&crate::wills::Kind::Arm));
    assert!(crate::wills::kinds_in(&w, &crate::wills::book(&w), 0, "my guys need swords").contains(&crate::wills::Kind::Arm));
    assert!(!crate::wills::kinds_in(&w, &crate::wills::book(&w), 0, "the crowd needs equipment").contains(&crate::wills::Kind::Arm));
    let mut old = w.clone();
    old.meta.schema = 10;
    assert!(!crate::wills::kinds_in(&old, &crate::wills::book(&old), 0, text).contains(&crate::wills::Kind::Arm), "0.9.0 states replay as recorded");
    let o = t.beat(text);
    let w = t.w();
    let held: Vec<&Item> = w.items.iter().filter(|i| matches!(i.holders.last(), Some(Ref::Demigod(d)) if w.demigods[d.ix()].seat == 0)).collect();
    assert_eq!(held.len(), 2, "gear for both demigods: {:?}", w.items.iter().map(|i| &i.name).collect::<Vec<_>>());
    assert_eq!(changes(&o).len(), 2, "and both looks change");
}

/// A resolver's find, as the live resolver wrote it (match ae54b0af, beat 17): "<actor>: <what happened>", so the
/// actor's name became the item's name.
fn resolver_find(w: &mut World, holder: DemigodId, name: &str, description: &str) -> ItemId {
    let p = Proposal {
        source: Source::Rules,
        intent: None,
        effect: Effect::CreateItem { name: name.into(), description: description.into(), holder: Ref::Demigod(holder) },
        act: format!("{name}: {description}"),
        actor: Ref::Demigod(holder),
        magnitude: 2,
        valence: 1,
        witnesses: vec![Ref::Demigod(holder)],
        cause: None,
    };
    let before = w.items.len();
    crate::merge::run(w, vec![p], &fn7_pantheon::load(), 3, 8);
    assert_eq!(w.items.len(), before + 1, "the find was made");
    ItemId::from_ix(before)
}

/// The live case (match ae54b0af): Bot Kestrel's Right Hand "finds a rusted fish-gutting knife among the trout"; the
/// resolver named the find after the finder ("Bot Kestrel's Right Hand"), it passed to Chris's Left Hand and showed in
/// its loadout and gear portrait as "tucked into the belt, a bot kestrel right hand". From schema 15 the find is named
/// after the object ("Rusted Fish-Gutting Knife") and a thing named after an entity never enters a loadout or item
/// art; a schema-12 state keeps both as recorded.
#[test]
fn a_thing_is_never_named_after_an_entity_and_never_pictured_as_one() {
    let mut t = table(21, json!({}));
    let (a, bot) = (mine(&t.w(), 0)[0], mine(&t.w(), 2)[1]);
    t.edit(|w| {
        w.demigods[bot.ix()].name = "Bot Kestrel's Right Hand".into();
        w.demigods[bot.ix()].player = "Bot Kestrel".into();
        w.seats[2].display_name = "Bot Kestrel".into();
    });
    assert_eq!(t.w().meta.schema, 15);
    // The source: the find takes the object's name.
    let mut w = t.w();
    let it = resolver_find(&mut w, bot, "Bot Kestrel's Right Hand", "finds a rusted fish-gutting knife among the trout.");
    assert_eq!(w.items[it.ix()].name, "Rusted Fish-Gutting Knife");
    // A schema-12 state replays the find as it was recorded.
    let mut old = t.w();
    old.meta.schema = 12;
    let it_old = resolver_find(&mut old, bot, "Bot Kestrel's Right Hand", "finds a rusted fish-gutting knife among the trout.");
    assert_eq!(old.items[it_old.ix()].name, "Bot Kestrel's Right Hand");

    // The loadout: Chris's Left Hand holds the obsidian blade and the junk item (made under the old rules, carried on).
    t.give(a, ItemKind::Weapon, Tier::Trinket, "Obsidian Blade", None);
    t.edit(|w| {
        let mut old = w.clone();
        old.meta.schema = 12;
        let j = resolver_find(&mut old, a, "Bot Kestrel's Right Hand", "finds a rusted fish-gutting knife among the trout.");
        *w = World { meta: w.meta.clone(), ..old };
        assert_eq!(w.items[j.ix()].name, "Bot Kestrel's Right Hand");
        assert_eq!(w.items[j.ix()].ext.kind, ItemKind::Other);
    });
    let w = t.w();
    let junk = w.items.iter().find(|i| i.name == "Bot Kestrel's Right Hand").unwrap();
    assert!(!look::real_object(&w, junk) && look::real_object(&w, &w.items[0]));
    let names: Vec<String> = loadout(&w, a, 2).iter().map(|e| e.gear.name.clone()).collect();
    assert_eq!(names, vec!["Obsidian Blade".to_string()], "the junk item is not in the loadout");
    let mut w12 = w.clone();
    w12.meta.schema = 12;
    assert_eq!(loadout(&w12, a, 2).len(), 2, "a schema-12 state keeps its recorded loadout");
    // Empty names and every kind of entity name are not objects; real things are.
    for n in ["", "  ", "Bot Kestrel's Right Hand", "bot kestrel right hand", "Bot Kestrel", "Chris's Left Hand", "Death"] {
        assert!(w.names_entity(n), "{n:?} names an entity");
    }
    for n in ["Obsidian Blade", "Rusted Fish-Gutting Knife", "Right Hand of Glory", "Kestrel Feather"] {
        assert!(!w.names_entity(n), "{n:?} is a thing");
    }

    // The picture: no belt item, no item art for it.
    t.beat("");
    let l = t.look(a);
    let ps = l["portrait_subject"].as_str().unwrap();
    assert!(!ps.contains("kestrel"), "{ps}");
    assert!(l["loadout"].as_array().unwrap().iter().all(|e| e["name"] != "Bot Kestrel's Right Hand"), "{l}");
    let w = t.w();
    let junk = w.items.iter().find(|i| i.name == "Bot Kestrel's Right Hand").unwrap();
    assert!(!look::book(&w).art.contains(&look::gear_of(&w, junk).art_key()), "no item art for it");
}
