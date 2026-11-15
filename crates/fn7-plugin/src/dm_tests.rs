//! Rules 0.8.0: the Dungeon Master's rulebook, its place × time memory and the eight features it governs.

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::testkit::Failing;
use crate::*;
use fn7_model::dm::*;

fn seats() -> Vec<SeatInfo> {
    (0..4)
        .map(|i| SeatInfo { seat: i, display_name: ["Ash", "Brix", "Cyra", "Dune"][i as usize].into(), is_bot: true })
        .collect()
}

/// A bot match through the plugin's hooks (no LLM), `beats` beats long; returns every state after each commit and
/// every resolve's public outcome.
async fn play(seed: u64, beats: u32, extra: Value) -> (Vec<Value>, Vec<Value>) {
    let llm = Failing;
    let p = FinalNight7Plugin::new();
    let mut s = json!({ "seed": seed, "beats_total": 60, "llm": false });
    if let (Some(o), Some(x)) = (s.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            o.insert(k.clone(), v.clone());
        }
    }
    let settings = p.validate_settings(&s).expect("settings");
    let mut state = p.on_match_start(&seats(), &settings, &llm).await.expect("start");
    let (mut states, mut outs) = (vec![], vec![]);
    for round in 1..=beats {
        if p.is_match_over(&state, round - 1) && round > 1 {
            break;
        }
        state = p.on_round_start(&state, round, &llm).await.expect("round start").state;
        let mut actions = vec![];
        for seat in 0..4u8 {
            actions.push(p.bot_action(&state, seat, &llm).await.expect("bot"));
        }
        let o = p.resolve_round(&state, round, &actions, &RoundContext::default(), &llm, &NoProgress).await.expect("resolve");
        outs.push(o.public.clone());
        state = p.commit_round(&o.state, round, &[], &llm).await.expect("commit").state;
        states.push(state.clone());
    }
    (states, outs)
}

fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(f)
}

#[test]
fn a_new_match_is_schema_9_and_rules_0_8_2() {
    let w = testkit::world(4, 3);
    assert_eq!((w.meta.schema, w.dm_rules(), w.challenge_rules(), w.resume_rules(), w.panel_rules(), w.wills_rules(), RULES_VERSION), (fn7_model::STATE_SCHEMA, true, true, true, true, true, "0.9.0"));
    assert!(w.dm.is_empty(), "the DM's memory starts empty");
}

#[test]
fn the_dm_remembers_and_every_feature_shows_up() {
    let (states, outs) = block(play(5, 40, json!({ "dm_encounters_per_beat": 2 })));
    let w = world::load(states.last().unwrap()).unwrap();
    let kinds = |k: &str| w.dm.memory.iter().filter(|e| e.kind == k).count();
    // Memory: dice, settlements, time of day.
    assert!(kinds("dice.rolled") > 20, "dice: {}", kinds("dice.rolled"));
    assert!(kinds("time.phase") >= 4);
    assert!(!w.dm.settlements.is_empty() && w.dm.settlements.iter().all(|s| (1..=7).contains(&s.level)));
    assert!(kinds("encounter.marked") > 0);
    assert_eq!(w.dm.stories.len(), w.demigods.len());
    // Every entry's id is its index (append-only), and every decided query is unique.
    let mut qs = std::collections::BTreeSet::new();
    for (i, e) in w.dm.memory.iter().enumerate() {
        assert_eq!(e.id as usize, i);
        if let Some(q) = &e.query {
            assert!(qs.insert(q.clone()), "query {q} decided twice");
        }
    }
    // The outcome carries the visible dice and the time of day.
    let o = &outs[3];
    let d = &o["dice"][0];
    assert_eq!(d["event"], "dice.rolled");
    assert_eq!(d["die"], "d20");
    for k in ["who", "roll", "modifier", "total", "target_dc", "verdict", "reason", "at", "cell"] {
        assert!(!d[k].is_null(), "dice.rolled.{k}");
    }
    assert!(d.get("confirm").is_none());
    assert!(["crit_fail", "fail", "success", "crit_success"].contains(&d["verdict"].as_str().unwrap()));
    assert!(o["time_of_day"]["phase"].is_string() && o["time_of_day_phase"].is_string());
    // Public state: stories and shrines.
    let p = FinalNight7Plugin::new();
    let pv = p.public_view(states.last().unwrap());
    let story = &pv["demigods"][0]["story"];
    for k in ["followers", "items", "wardrobe", "scars"] {
        assert!(!story[k].is_null(), "story.{k}");
    }
    assert!(pv["shrines"].is_array() && pv["time_of_day"]["light"].is_number());
    // The brief.
    let b = block(p.brief(states.last().unwrap(), 41, &Failing)).unwrap();
    assert_eq!(b.public["time_of_day"]["day"], 6);
    assert_eq!(b.public["time_of_day_phase"], "dawn");
}

#[test]
fn the_dm_is_deterministic() {
    let a = block(play(9, 25, json!({}))).0;
    let b = block(play(9, 25, json!({}))).0;
    assert_eq!(a.last().unwrap()["dm"], b.last().unwrap()["dm"]);
    assert!(a.last().unwrap()["dm"]["memory"].as_array().is_some_and(|m| !m.is_empty()));
}

#[test]
fn the_same_query_gives_the_same_roll() {
    let mut w = testkit::world(4, 3);
    let r1 = dm::roll_d20(&w, "check:42", 1, 12);
    let at = w.demigods[0].pos;
    dm::record_roll(&mut w, 1, "check:42", Ref::Demigod(DemigodId(0)), &r1, "a test", at);
    let n = w.dm.memory.len();
    // Asked again (even with another modifier or DC, at another beat): the recorded result.
    let r2 = dm::roll_d20(&w, "check:42", 5, 3);
    assert_eq!(r1, r2);
    dm::record_roll(&mut w, 7, "check:42", Ref::Demigod(DemigodId(0)), &r2, "a test", at);
    assert_eq!(w.dm.memory.len(), n, "recorded once");
    // Faces are 1..=20 and roughly uniform; crits follow the faces.
    let mut seen = [0u32; 21];
    for k in 0..4000 {
        let r = dm::roll_d20(&w, &format!("q:{k}"), 0, 11);
        seen[r.roll as usize] += 1;
        assert_eq!(r.verdict == DiceVerdict::CritSuccess, r.roll == 20);
        assert_eq!(r.verdict == DiceVerdict::CritFail, r.roll == 1);
    }
    assert!(seen[1..].iter().all(|n| (130..270).contains(n)), "{seen:?}");
}

#[test]
fn memory_answers_by_place_and_time_up_and_down_the_hierarchy() {
    let (states, _) = block(play(4, 20, json!({})));
    let state = states.last().unwrap();
    let w = world::load(state).unwrap();
    let p = FinalNight7Plugin::new();
    // A settlement that changed (or was built in): its entries, and the world's, but not other regions'.
    let e = w.dm.memory.iter().find(|e| e.scope == Scope::Settlement && e.public).expect("a settlement entry");
    let v = p.query(state, None, "memory", &json!({ "place": e.place, "until_beat": e.beat, "limit": 200 })).unwrap();
    let items = v["items"].as_array().unwrap();
    assert!(items.iter().any(|x| x["id"] == e.id));
    assert!(items.iter().all(|x| x["place"] == e.place || x["place"] == "world"
        || x["up"].as_array().unwrap().iter().any(|u| *u == e.place)
        || v["ancestors"].as_array().unwrap().contains(&x["place"])));
    // At or before a beat.
    let v = p.query(state, None, "memory", &json!({ "place": "world", "until_beat": 3 })).unwrap();
    assert!(v["items"].as_array().unwrap().iter().all(|x| x["beat"].as_u64().unwrap() <= 3));
    // A hex cell resolves to its settlement and region.
    let cell = &w.dm.memory.iter().find(|e| e.scope == Scope::Cell).unwrap().place;
    let v = p.query(state, None, "memory", &json!({ "place": cell })).unwrap();
    assert!(v["ancestors"].as_array().unwrap().iter().any(|a| a == "world"));
    assert!(p.query(state, None, "memory", &json!({ "place": "nowhere" })).is_err());
    // Only public entries.
    let all = p.query(state, None, "memory", &json!({ "place": "world", "limit": 200 })).unwrap();
    for x in all["items"].as_array().unwrap() {
        assert!(w.dm.memory[x["id"].as_u64().unwrap() as usize].public);
    }
}

/// A world a few beats in, with an encounter of `kind` on demigod 0's way to a point 100 km off.
fn stepping_world(kind: EncounterKind) -> (World, LonLat, LonLat) {
    let (states, _) = block(play(6, 3, json!({ "dm_encounters_per_beat": 0 })));
    let mut w = world::load(states.last().unwrap()).unwrap();
    let from = w.demigods[0].pos;
    let to = fn7_society::geo::destination(from, 90.0, 100.0);
    let mid = fn7_society::geo::destination(from, 90.0, 50.0);
    let wh = dm::locate(&w, mid, fn7_model::hexgrid::HexLevel::Houses);
    w.dm.encounters.push(Encounter { id: w.dm.encounters.len() as u32, kind, cell: wh.cell, at: mid, region: wh.region,
                                     settlement: wh.settlement, placed_beat: 1, night: false, faded_beat: None, outcome: None });
    (w, from, to)
}

fn dm_step(w: &mut World, beat: u32, from_pos: Vec<LonLat>) {
    let alive: Vec<DemigodId> = w.alive_demigods().map(|d| d.id).collect();
    let health: Vec<f32> = w.demigods.iter().map(|d| d.health).collect();
    let start = dm::Start { alive: &alive, pos: &from_pos, health: &health, armies: &[], row_start: w.ledger.rows.len(),
                            intent_start: w.intents.len(), manifest_start: w.manifests.len(), memory_start: w.dm.memory.len() };
    let st = world::settings(w);
    dm::step(w, beat, &start, &fn7_pantheon::load(), &st);
}

#[test]
fn an_encounter_is_hidden_until_stepped_on_then_revealed_for_good() {
    let (mut w, from, to) = stepping_world(EncounterKind::Treasure);
    let id = w.dm.encounters.len() - 1;
    // Hidden: only {id, cell, marked}.
    let v = dm::encounter_json(&w, &w.dm.encounters[id]);
    assert_eq!(v.as_object().unwrap().len(), 3, "{v}");
    assert!(v.get("type").is_none());
    // Nobody moves: it stays hidden.
    let still: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
    dm_step(&mut w, 4, still);
    assert!(w.dm.encounters[id].outcome.is_none());
    // Demigod 0 walks over it.
    let mut start: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
    start[0] = from;
    w.demigods[0].pos = to;
    dm_step(&mut w, 5, start);
    let o = w.dm.encounters[id].outcome.clone().expect("revealed");
    assert_eq!(o.by, DemigodId(0));
    assert_eq!(o.roll.target_dc, 10);
    let v = dm::encounter_json(&w, &w.dm.encounters[id]);
    assert_eq!((v["revealed"].as_bool(), v["type"].as_str()), (Some(true), Some("treasure")));
    assert!(w.dm.memory.iter().any(|e| e.kind == "encounter.revealed" && e.query.as_deref() == Some(&format!("encounter:{id}:revealed"))));
    // The roll is remembered: the same query gives the same roll; stepping on it again changes nothing.
    assert_eq!(dm::roll_d20(&w, &format!("encounter:{id}"), 0, 0).roll, o.roll.roll);
    let mut start: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
    start[0] = from;
    dm_step(&mut w, 6, start);
    assert_eq!(w.dm.encounters[id].outcome.as_ref(), Some(&o));
    // A success leaves an item with an icon.
    if o.roll.verdict.ok() {
        let it = &w.items[o.item.expect("treasure").ix()];
        assert_eq!(it.holders.last(), Some(&Ref::Demigod(DemigodId(0))));
    }
}

#[test]
fn every_encounter_kind_resolves_by_its_table() {
    for kind in [EncounterKind::Monster, EncounterKind::Trap, EncounterKind::Stranger, EncounterKind::Undead,
                 EncounterKind::Ghost, EncounterKind::Thief] {
        let (mut w, from, to) = stepping_world(kind);
        let id = w.dm.encounters.len() - 1;
        let h0 = w.demigods[0].health;
        let mut start: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
        start[0] = from;
        w.demigods[0].pos = to;
        dm_step(&mut w, 5, start);
        let o = w.dm.encounters[id].outcome.clone().unwrap_or_else(|| panic!("{kind:?} revealed"));
        let hurt = h0 - w.demigods[0].health;
        if o.damage > 0.0 {
            assert!(!o.roll.verdict.ok(), "{kind:?}: damage only on a failure");
            assert!(hurt > 0.0 || !w.demigods[0].alive);
        }
        if kind == EncounterKind::Stranger && o.roll.verdict.ok() {
            let c = o.character.expect("the stranger becomes a named character");
            assert!(w.characters[c.ix()].groups.contains(w.demigods[0].following), "who follows the demigod");
        }
    }
}

#[test]
fn loot_drops_on_death_and_is_taken_by_who_steps_there() {
    let (states, _) = block(play(6, 3, json!({ "dm_encounters_per_beat": 0 })));
    let mut w = world::load(states.last().unwrap()).unwrap();
    // Demigod 0 holds a thing and falls.
    let pantheon = fn7_pantheon::load();
    crate::merge::run(&mut w, vec![Proposal { source: Source::Rules, intent: None,
        effect: Effect::CreateItem { name: "Sunsteel Blade".into(), description: "a blade".into(), holder: Ref::Demigod(DemigodId(0)) },
        act: "a blade".into(), actor: Ref::Demigod(DemigodId(0)), magnitude: 2, valence: 1, witnesses: vec![], cause: None }],
        &pantheon, 4, 4);
    let item = ItemId::from_ix(w.items.len() - 1);
    let alive: Vec<DemigodId> = w.alive_demigods().map(|d| d.id).collect();
    let health: Vec<f32> = w.demigods.iter().map(|d| d.health).collect();
    let pos: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
    w.demigods[0].alive = false;
    w.demigods[0].health = 0.0;
    let start = dm::Start { alive: &alive, pos: &pos, health: &health, armies: &[], row_start: w.ledger.rows.len(),
                            intent_start: w.intents.len(), manifest_start: w.manifests.len(), memory_start: w.dm.memory.len() };
    let st = world::settings(&w);
    dm::step(&mut w, 4, &start, &pantheon, &st);
    let pile = w.dm.loot.last().cloned().expect("a pile");
    assert_eq!((pile.items.clone(), pile.from, pile.since_beat), (vec![item], DemigodId(0), 4));
    let v = dm::loot_under(&w, None);
    assert_eq!(v[0]["ref"], json!(Ref::Item(item)));
    assert_eq!(v[0]["cell"], json!(pile.cell));
    assert!(v[0]["icon"].is_string());
    // A rival walks over the spot next beat and takes it.
    let rival = w.demigods.iter().find(|d| d.alive && !w.same_player(d.id, DemigodId(0))).unwrap().id;
    let mut start_pos: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
    start_pos[rival.ix()] = fn7_society::geo::destination(pile.at, 0.0, 60.0);
    w.demigods[rival.ix()].pos = fn7_society::geo::destination(pile.at, 180.0, 60.0);
    dm_step(&mut w, 5, start_pos);
    let pile = w.dm.loot.last().unwrap();
    assert_eq!((pile.taken_by, pile.taken_beat), (Some(rival), Some(5)));
    assert_eq!(w.items[item.ix()].holders.last(), Some(&Ref::Demigod(rival)));
    assert!(dm::loot_under(&w, None).is_empty());
}

#[test]
fn an_order_raises_a_shrine_that_grows_and_grants() {
    let (states, _) = block(play(6, 3, json!({ "dm_encounters_per_beat": 0 })));
    let mut w = world::load(states.last().unwrap()).unwrap();
    let d = DemigodId(0);
    let mut i = w.intents.last().cloned().expect("an intent");
    i.id = IntentId::from_ix(w.intents.len());
    i.demigod = d;
    i.verb = "build".into();
    i.span = "build a shrine of light to me".into();
    let from = w.intents.len();
    w.intents.push(i);
    let alive: Vec<DemigodId> = w.alive_demigods().map(|d| d.id).collect();
    let health: Vec<f32> = w.demigods.iter().map(|d| d.health).collect();
    let pos: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
    let start = dm::Start { alive: &alive, pos: &pos, health: &health, armies: &[], row_start: w.ledger.rows.len(),
                            intent_start: from, manifest_start: w.manifests.len(), memory_start: w.dm.memory.len() };
    let st = world::settings(&w);
    dm::step(&mut w, 4, &start, &fn7_pantheon::load(), &st);
    let s = w.dm.shrines.last().cloned().expect("a shrine");
    assert_eq!((s.demigod, s.power, s.built_beat), (d, ShrinePower::Spark, 4));
    let v = dm::shrine_json(&w, &s);
    for k in ["cell", "owner_seat", "side", "worshippers", "power"] {
        assert!(v.get(k).is_some(), "shrine.{k}");
    }
    // It draws worshippers from the crowds around it and grants a spark every few beats.
    let sparks = w.demigods[0].sparks;
    for b in 5..=12 {
        let still: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
        dm_step(&mut w, b, still);
    }
    assert!(w.dm.shrines[s.id as usize].worshippers > 0);
    assert!(w.demigods[0].sparks > sparks, "the spark shrine grants");
}

#[test]
fn traffic_wears_a_road_between_settlements() {
    let mut w = testkit::world(4, 3);
    let sites: Vec<PlaceId> = w.places.iter().filter(|p| p.kind == PlaceKind::Site).map(|p| p.id).take(2).collect();
    let (a, b) = (w.places[sites[0].ix()].center, w.places[sites[1].ix()].center);
    for beat in 1..=6 {
        let mut start: Vec<LonLat> = w.demigods.iter().map(|d| d.pos).collect();
        let (f, t) = if beat % 2 == 1 { (a, b) } else { (b, a) };
        start[0] = f;
        w.demigods[0].pos = t;
        dm_step(&mut w, beat, start);
    }
    let roads = dm::roads_under(&w, None);
    assert_eq!(roads.len(), 1, "{roads:?}");
    for k in ["from_cell", "to_cell", "wear"] {
        assert!(!roads[0][k].is_null());
    }
    assert!(w.dm.memory.iter().any(|e| e.kind == "road.formed"));
}

#[test]
fn settlements_grow_and_are_exposed_by_cell() {
    let (states, _) = block(play(5, 30, json!({ "dm_settlement_step_beats": 1 })));
    let state = states.last().unwrap();
    let w = world::load(state).unwrap();
    assert!(w.dm.memory.iter().any(|e| e.kind == "settlement.grew" || e.kind == "settlement.shrank"));
    let region = w.places.iter().find(|p| p.kind == PlaceKind::District && w.places.iter().any(|c| c.parent == Some(p.id) && c.kind == PlaceKind::Site)).unwrap().id;
    let p = FinalNight7Plugin::new();
    let v = p.query(state, None, "region", &json!({ "ref": Ref::Place(region) })).unwrap();
    let s = &v["cities"][0]["settlement"];
    assert!(s["level"].as_u64().is_some() && s["size"].as_u64().is_some(), "{s}");
    assert!(s["center_cell"].is_string() && s["settlement_cells"].as_array().is_some_and(|c| !c.is_empty()));
    for k in ["encounters", "loot", "roads", "shrines"] {
        assert!(v[k].is_array(), "region.{k}");
    }
    let n = p.query(state, None, "node", &json!({ "ref": v["cities"][0]["ref"] })).unwrap();
    assert!(n["node"]["settlement"]["level"].is_number());
    let e = p.query(state, None, "encounters", &json!({})).unwrap();
    assert!(e["encounters"].is_array() && e["hidden"].is_number());
    let g = p.query(state, None, "globe", &json!({})).unwrap();
    assert!(g["roads"].is_array() && g["shrines"].is_array());
}

#[test]
fn schema_5_worlds_see_none_of_it() {
    let (states, outs) = block(play(5, 1, json!({})));
    let mut w = world::load(&states[0]).unwrap();
    w.meta.schema = 5;
    w.dm = Default::default();
    let state = world::save(&w);
    let p = FinalNight7Plugin::new();
    let llm = Failing;
    let s2 = block(p.on_round_start(&state, 2, &llm)).unwrap().state;
    let mut actions = vec![];
    for seat in 0..4u8 {
        actions.push(block(p.bot_action(&s2, seat, &llm)).unwrap());
    }
    let o = block(p.resolve_round(&s2, 2, &actions, &RoundContext::default(), &llm, &NoProgress)).unwrap();
    assert!(o.state.get("dm").is_none(), "no DM state");
    assert!(o.public.get("dice").is_none() && o.public.get("time_of_day").is_none());
    assert!(p.public_view(&o.state)["demigods"][0].get("story").is_none());
    assert!(outs[0].get("dice").is_some());
    // The resolver's prompt is the old one without the DM's view.
    let ctx_prompt = fn7_agents::resolver::system_prompt();
    assert!(!ctx_prompt.contains("DUNGEON MASTER"));
    assert!(fn7_agents::resolver::system_prompt_dm().contains("DUNGEON MASTER"));
}

#[test]
fn zooming_into_an_unobserved_hex_extrapolates_the_same_answer_and_observing_records_it() {
    let (states, _) = block(play(5, 12, json!({})));
    let state = states.last().unwrap();
    let mut w = world::load(state).unwrap();
    let p = FinalNight7Plugin::new();
    // A hex nobody looked at: near a settlement, never observed.
    let site = w.places.iter().find(|x| x.kind == PlaceKind::Site).unwrap();
    let key = fn7_model::hexgrid::cell_at(fn7_society::geo::destination(site.center, 45.0, 2.0), fn7_model::hexgrid::HexLevel::Houses).key();
    assert!(!w.dm.memory.iter().any(|e| e.place == key));
    let a = p.query(state, None, "node", &json!({ "cell": key })).unwrap();
    let b = p.query(state, None, "node", &json!({ "cell": key })).unwrap();
    assert_eq!(a, b, "the same hex, the same beat: the same extrapolation");
    assert_eq!((a["extrapolated"].as_bool(), a["observed_beat"].is_null()), (Some(true), true));
    for k in ["people", "buildings", "weather", "time_of_day", "settlement_level", "encounters", "on_road"] {
        assert!(a["node"].get(k).is_some(), "hex.{k}");
    }
    // game.memory on a hex carries the same state.
    let m = p.query(state, None, "memory", &json!({ "place": key })).unwrap();
    assert_eq!(m["state"], a["node"]);
    // Observed: recorded as the new fact; the next look at this beat is the record, not an extrapolation.
    let beat = w.meta.beat;
    dm::observe(&mut w, beat, &key);
    let (rec, obs, ext) = dm::cell_state(&w, &key, beat).unwrap();
    assert_eq!((obs, ext), (Some(beat), false));
    assert_eq!(rec, a["node"]);
    // Later: carried forward from the record (deterministic), remembering when it was last seen.
    let (later, obs, ext) = dm::cell_state(&w, &key, beat + 5).unwrap();
    assert_eq!((obs, ext), (Some(beat), true));
    assert_eq!(dm::cell_state(&w, &key, beat + 5).unwrap().0, later);
    assert_eq!(later["terrain"], rec["terrain"]);
}

#[test]
fn a_coarse_query_reads_the_facts_beneath_it() {
    let (states, _) = block(play(5, 16, json!({ "dm_encounters_per_beat": 2 })));
    let state = states.last().unwrap();
    let w = world::load(state).unwrap();
    // A fact recorded on a hex rolls up to its region (and the world) the same beat.
    let e = w.dm.memory.iter().find(|e| e.scope == Scope::Cell && e.kind == "encounter.marked").expect("a marked hex");
    let region = e.up.iter().find(|u| u.starts_with("pl:") && {
        let r: Ref = u.parse().unwrap();
        matches!(r, Ref::Place(p) if w.places[p.ix()].kind == PlaceKind::District)
    }).unwrap().clone();
    let roll = w.dm.memory.iter().find(|x| x.kind == "memory.rollup" && x.place == region && x.beat == e.beat).expect("a region roll-up");
    assert!(roll.data["counts"]["encounter.marked"].as_u64().unwrap() >= 1);
    assert!(w.dm.memory.iter().any(|x| x.kind == "memory.rollup" && x.place == "world" && x.beat == e.beat));
    // game.memory on the region includes it; game.region carries the latest roll-up.
    let p = FinalNight7Plugin::new();
    let m = p.query(state, None, "memory", &json!({ "place": region, "until_beat": e.beat, "limit": 200 })).unwrap();
    assert!(m["items"].as_array().unwrap().iter().any(|x| x["id"] == e.id));
    let v = p.query(state, None, "region", &json!({ "ref": region })).unwrap();
    assert_eq!(v["memory_summary"]["kind"], "memory.rollup");
    assert_eq!((v["observed_beat"].as_u64(), v["extrapolated"].as_bool()), (Some(w.meta.beat as u64), Some(false)));
}

#[test]
fn a_result_that_contradicts_the_record_is_reconciled_to_it() {
    let mut w = testkit::world(4, 3);
    // The record: act 7 rolled a natural 1.
    let at = w.demigods[0].pos;
    let q = "act:7";
    let mut r = dm::roll_d20(&w, q, 0, 10);
    r.roll = 1;
    r.total = 1;
    r.verdict = DiceVerdict::CritFail;
    dm::record_roll(&mut w, 1, q, Ref::Demigod(DemigodId(0)), &r, "a strike", at);
    // The LLM's reply says the strike landed for 6: reconciled to the record (narration only).
    let p = Proposal { source: Source::Resolver, intent: Some(IntentId(7)),
                       effect: Effect::Damage { who: Ref::Demigod(DemigodId(2)), amount: 6.0, cause: "a stab".into() },
                       act: "Ash stabs Cyra".into(), actor: Ref::Demigod(DemigodId(0)), magnitude: 5, valence: -3,
                       witnesses: vec![], cause: None };
    let out = dm::reconcile_resolver(&w, 1, vec![p.clone()]);
    assert!(matches!(out[0].effect, Effect::Note { .. }), "{:?}", out[0].effect);
    // A failure caps the damage at 3.
    let q2 = "act:8";
    let mut r2 = dm::roll_d20(&w, q2, 0, 10);
    r2.roll = 5;
    r2.total = 5;
    r2.verdict = DiceVerdict::Fail;
    dm::record_roll(&mut w, 1, q2, Ref::Demigod(DemigodId(0)), &r2, "a strike", at);
    let out = dm::reconcile_resolver(&w, 1, vec![Proposal { intent: Some(IntentId(8)), ..p.clone() }]);
    assert!(matches!(out[0].effect, Effect::Damage { amount, .. } if amount == 3.0));
    // Acts with no record pass unchanged.
    let out = dm::reconcile_resolver(&w, 1, vec![Proposal { intent: Some(IntentId(99)), ..p.clone() }]);
    assert_eq!(out[0], Proposal { intent: Some(IntentId(99)), ..p });
    // A decision with a recorded answer keeps the answer.
    let wh = dm::locate(&w, at, fn7_model::hexgrid::HexLevel::People);
    let a = dm::remember(&mut w, 2, &wh, dm::Decision { scope: Scope::Cell, kind: "encounter.revealed", query: Some("encounter:0:revealed".into()),
                                                       public: true, text: "found gold".into(), data: json!({ "verdict": "success" }) });
    let b = dm::remember(&mut w, 3, &wh, dm::Decision { scope: Scope::Cell, kind: "encounter.revealed", query: Some("encounter:0:revealed".into()),
                                                       public: true, text: "found nothing".into(), data: json!({ "verdict": "fail" }) });
    assert_eq!(a, b);
    assert_eq!(w.dm.memory[a as usize].data["verdict"], "success");
}

/// Prints real payload examples for docs/CLIENT_API.md (run with `--ignored --nocapture`).
#[test]
#[ignore]
fn print_examples() {
    let (states, outs) = block(play(5, 40, json!({ "dm_encounters_per_beat": 3, "dm_settlement_step_beats": 2 })));
    let state = states.last().unwrap();
    let w = world::load(state).unwrap();
    let p = FinalNight7Plugin::new();
    let show = |k: &str, v: &Value| eprintln!("=== {k}\n{}", serde_json::to_string(v).unwrap());
    for o in &outs {
        if let Some(d) = o["dice"].as_array().and_then(|d| d.first()) {
            show("dice", d);
            break;
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for o in &outs {
        for e in o["dm_events"].as_array().cloned().unwrap_or_default() {
            let k = e["event"].as_str().unwrap().to_string();
            if seen.insert(k.clone()) {
                show(&format!("dm_event {k}"), &e);
            }
        }
    }
    show("time_of_day", &outs[5]["time_of_day"]);
    let pv = p.public_view(state);
    show("story", &pv["demigods"][0]["story"]);
    show("shrines", &pv["shrines"]);
    let region = w.dm.encounters.iter().find(|e| e.outcome.is_some()).map(|e| e.region).unwrap_or(w.dm.encounters[0].region);
    let r = p.query(state, None, "region", &json!({ "ref": Ref::Place(region) })).unwrap();
    for k in ["encounters", "loot", "roads", "shrines", "memory_summary", "observed_beat", "extrapolated"] {
        show(&format!("region.{k}"), &r[k]);
    }
    show("region.cities[0]", &r["cities"][0]);
    show("encounters", &p.query(state, None, "encounters", &json!({})).unwrap());
    let cell = w.demigods[0].pos;
    let key = fn7_model::hexgrid::cell_at(cell, fn7_model::hexgrid::HexLevel::Houses).key();
    show("node cell", &p.query(state, None, "node", &json!({ "cell": key })).unwrap());
    show("memory", &p.query(state, None, "memory", &json!({ "place": Ref::Place(region), "limit": 3 })).unwrap());
    eprintln!("loot piles {} roads {} shrines {}", w.dm.loot.len(), w.dm.roads().count(), w.dm.shrines.len());
}


#[test]
fn a_mega_on_a_fault_line_is_stronger_than_one_in_the_outback() {
    use crate::actions::{CastOrder, CastTarget};
    let w = testkit::world(4, 3);
    let cast = |at: LonLat, scale: CastScale| CastOrder {
        power: "volcano".into(), target: CastTarget::At(at), text: None, magnitude: None, seq: 1, instant: true,
        scale: Some(scale),
    };
    let streak = |at: LonLat, scale: CastScale| casts::streak(&w, DemigodId(0), 0, &[cast(at, scale)]).expect("a streak");
    let sa = streak([-120.5, 35.9], CastScale::Mega);
    let jp = streak([143.5, 38.0], CastScale::Mega);
    let au = streak([134.0, -24.0], CastScale::Mega);
    for s in [&sa, &jp] {
        assert!(s["fault_km"].as_f64().unwrap() < 150.0, "{s}");
        assert!(s["fault_factor"].as_f64().unwrap() > au["fault_factor"].as_f64().unwrap());
        assert!(s["casts"][0]["radius_km"].as_f64().unwrap() > au["casts"][0]["radius_km"].as_f64().unwrap());
    }
    assert!(au["fault_km"].as_f64().unwrap() > 1500.0);
    assert_eq!(au["fault_factor"], 0.6);
    // Minis are unaffected.
    assert!(streak([-120.5, 35.9], CastScale::Mini).get("fault_factor").is_none());
}

/// One resolve of `state` at `round` with the bots' actions plus `extra` (seat 0's instant action).
fn resolve_with(state: &Value, round: u32, extra: Value) -> theatre_plugin_api::RoundOutcome {
    let p = FinalNight7Plugin::new();
    let llm = Failing;
    let s = block(p.on_round_start(state, round, &llm)).unwrap().state;
    let mut actions = vec![];
    for seat in 0..4u8 {
        actions.push(block(p.bot_action(&s, seat, &llm)).unwrap());
    }
    let mut a = Action { fields: extra, ..Action::new(0, "") };
    a.instant = true;
    a.seq = 99;
    actions.push(a);
    block(p.resolve_round(&s, round, &actions, &RoundContext::default(), &llm, &NoProgress)).unwrap()
}

#[test]
fn a_summoned_demigod_lands_this_beat_with_its_tale() {
    let (states, _) = block(play(6, 4, json!({ "dm_encounters_per_beat": 0 })));
    let state = states.last().unwrap();
    let w0 = world::load(state).unwrap();
    let p = FinalNight7Plugin::new();
    let target = [-21.94, 64.15]; // Reykjavik
    let summon = json!({ "summon": { "demigod": "dg:0", "target": target } });
    p.validate_action(state, 0, "", &summon, SubmitKind::Instant).expect("valid");
    assert_eq!(p.validate_action(state, 0, "", &json!({ "summon": { "demigod": "dg:1", "target": target } }), SubmitKind::Instant),
               Err("not_yours".to_string()));
    assert_eq!(p.validate_action(state, 0, "", &json!({ "summon": { "demigod": "dg:0", "target": "nowhere" } }), SubmitKind::Instant),
               Err("bad_target".to_string()));
    let v = p.on_instant(state, 0, &Action { fields: summon.clone(), instant: true, ..Action::new(0, "") }).expect("a push");
    assert_eq!((v["event"].as_str(), v["demigod"].as_str(), v["by_seat"].as_u64()), (Some("summoning"), Some("dg:0"), Some(0)));
    assert_eq!(v["to"], json!(target));
    assert_eq!(v["from"], json!(w0.demigods[0].pos));
    let o = resolve_with(state, 5, summon);
    let w = world::load(&o.state).unwrap();
    let d = &w.demigods[0];
    assert!(fn7_society::geo::distance_km(d.pos, target) < 1.0, "it landed there this beat: {:?} {} {}", d.pos, o.public["summons"], w.dm.memory.iter().filter(|e| e.kind.starts_with("summon")).count());
    let ev = o.public["summons"].as_array().unwrap().first().cloned().expect("a summoned event");
    assert_eq!(ev["event"], "summoned");
    assert_eq!(ev["demigod"], "dg:0");
    assert!(ev["deed"].as_str().unwrap().len() > 20);
    assert_ne!(ev["from"]["cell"], ev["to"]["cell"]);
    let lost = ev["health_lost"].as_f64().unwrap();
    assert!((0.0..=2.0).contains(&lost));
    assert!((w0.demigods[0].health - d.health) as f64 >= lost - 1e-6 || !d.alive);
    let ev2 = o.public["events"].as_array().unwrap().iter().find(|e| e["kind"] == "summoned").cloned().expect("in events[]");
    assert_eq!(ev2["deed"], ev["deed"]);
    assert!(ev2["roll"].as_u64().is_some_and(|r| (1..=20).contains(&r)));
    let dice = o.public["dice"].as_array().unwrap();
    assert!(dice.iter().any(|x| x["reason"] == "a summoning" && x["who"] == "dg:0"));
    let story = w.dm.story(DemigodId(0)).unwrap();
    assert_eq!((story.last_summon, story.tales.last().map(|t| t.kind.as_str())), (Some(5), Some("summoned")));
    assert!(w.dm.memory.iter().any(|e| e.kind == "summon.departed" && json!(e.place) == ev["from"]["cell"]));
    assert!(w.dm.memory.iter().any(|e| e.kind == "summoned"));
    // Once a beat.
    assert!(p.validate_action(&o.state, 0, "", &json!({ "summon": { "demigod": "dg:0", "target": [0.0, 0.0] } }), SubmitKind::Instant).is_ok(),
            "the next beat is allowed (cooldown 1)");
    let mut w2 = w.clone();
    w2.meta.beat = 4;
    assert_eq!(p.validate_action(&world::save(&w2), 0, "", &json!({ "summon": { "demigod": "dg:0", "target": [0.0, 0.0] } }), SubmitKind::Instant),
               Err("cooldown".to_string()));
}

#[test]
fn a_god_tier_power_is_that_gods_ruling() {
    let (states, _) = block(play(6, 3, json!({ "dm_encounters_per_beat": 0 })));
    let mut w = world::load(states.last().unwrap()).unwrap();
    w.set_bolts(DemigodId(0), 5);
    let state = world::save(&w);
    let p = FinalNight7Plugin::new();
    // The palette lists the seven, tier god.
    let b = block(p.brief(&state, 4, &Failing)).unwrap();
    let gods: Vec<&Value> = b.public["powers"].as_array().unwrap().iter().filter(|x| x["tier"] == "god").collect();
    assert_eq!(gods.len(), 7);
    assert_eq!(gods[0]["slug"], "god_time");
    assert_eq!(gods[0]["cost"], 3);
    let cast = json!({ "cast": { "power": "god_entropy", "tier": "god", "target": w.demigods[0].pos } });
    p.validate_action(&state, 0, "", &cast, SubmitKind::Instant).expect("valid");
    let rival_bolts = w.demigods[1].bolts;
    let o = resolve_with(&state, 4, cast);
    let w2 = world::load(&o.state).unwrap();
    let line = &o.public["god_powers"][0];
    assert_eq!((line["power"].as_str(), line["tier"].as_str(), line["god"].as_str()), (Some("god_entropy"), Some("god"), Some("entropy")));
    assert!(line.get("fizzled").is_none(), "{line}");
    assert!(w2.demigods[1].bolts > rival_bolts, "rivals gain a bolt");
    // Entropy ruled on it (Entropy always says yes) and its chaos burst landed: +10 for the power, +15 for the burst.
    let entropy = pantheon_id(&p, "entropy");
    assert!(w2.rulings.iter().any(|r| r.beat == 4 && r.god == entropy));
    assert!(w2.meta.entropy >= w.meta.entropy + 24.0, "{} -> {}", w.meta.entropy, w2.meta.entropy);
    // Not enough bolts: refused on arrival.
    let mut poor = w.clone();
    poor.set_bolts(DemigodId(0), 1);
    assert!(p.validate_action(&world::save(&poor), 0, "", &json!({ "cast": { "power": "god_death", "tier": "god", "target": "dg:1" } }), SubmitKind::Instant).is_err());
}

fn pantheon_id(p: &FinalNight7Plugin, slug: &str) -> GodId {
    p.pantheon().id_of(slug).unwrap()
}
