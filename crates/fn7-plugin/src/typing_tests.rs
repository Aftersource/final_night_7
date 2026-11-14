//! Live typing (`fields.duel_keys` → `duel.typing`) and duellists' skins: validation, refusals, the relay, what a
//! reconnect sees, replay-neutrality, and the skin function against known vectors.

use serde_json::{json, Value};
use theatre_plugin_api::*;

use crate::duel_tests::*;
use crate::*;

fn keys(id: &str, len: u64, tail: &str) -> Value {
    json!({ "duel_keys": { "duel_id": id, "len": len, "tail": tail } })
}

fn open(seed: u64) -> (Table, String, Value) {
    let t = table(seed, json!({ "duel_deadline_ms": 30000, "duel_damage_scale": 0.1 }));
    let lock = submit(&t, &instant(0, challenge(&t, "regions"), 1, 1_000)).unwrap();
    let id = lock["duel_id"].as_str().unwrap().to_string();
    (t, id, lock)
}

#[test]
fn duel_keys_are_validated() {
    let p = |f: Value| actions::parse("", &f).map(|i| i.keys);
    let k = p(keys("duel:s1:0:1", 5, "knee")).unwrap().unwrap();
    assert_eq!((k.duel_id.as_str(), k.len, k.tail.as_str()), ("duel:s1:0:1", 5, "knee"));
    // The tail may be empty (a deletion, or a cleared line) or missing; non-ASCII counts in characters, not bytes.
    assert_eq!(p(keys("d", 0, "")).unwrap().unwrap().len, 0);
    assert_eq!(p(json!({ "duel_keys": { "duel_id": "d", "len": 3 } })).unwrap().unwrap().tail, "");
    assert_eq!(p(keys("d", 4, "né魔🔥")).unwrap().unwrap().tail, "né魔🔥");
    assert_eq!(p(keys("d", 120, "x")).unwrap().unwrap().len, 120, "the duel line's cap");
    assert_eq!(p(json!({ "duel_keys": null })), Ok(None));
    for (f, why) in [
        (json!({ "duel_keys": { "len": 1, "tail": "a" } }), "no_duel"),
        (json!({ "duel_keys": { "duel_id": "  ", "len": 1, "tail": "a" } }), "no_duel"),
        (json!({ "duel_keys": "duel:s1:0:1" }), "no_duel"),
        (keys("d", 121, "a"), "bad_len"),
        (json!({ "duel_keys": { "duel_id": "d", "tail": "a" } }), "bad_len"),
        (json!({ "duel_keys": { "duel_id": "d", "len": -1, "tail": "" } }), "bad_len"),
        (json!({ "duel_keys": { "duel_id": "d", "len": 2.5, "tail": "" } }), "bad_len"),
        (json!({ "duel_keys": { "duel_id": "d", "len": "3", "tail": "" } }), "bad_len"),
        (keys("d", 12, "kneesknee"), "bad_tail"),
        (keys("d", 2, "kne"), "bad_tail"),
        (keys("d", 0, "k"), "bad_tail"),
        (keys("d", 9, "a\nb"), "bad_tail"),
        (keys("d", 9, "a\u{7}"), "bad_tail"),
        (keys("d", 9, "a\u{202E}b"), "bad_tail"),
        (keys("d", 9, "\u{200B}"), "bad_tail"),
        (json!({ "duel_keys": { "duel_id": "d", "len": 3, "tail": 7 } }), "bad_tail"),
    ] {
        assert_eq!(p(f.clone()), Err(why.to_string()), "{f}");
    }
    // It travels alone.
    let mut both = keys("d", 1, "a");
    both["duel_tick"] = json!({ "duel_id": "d" });
    assert!(p(both).is_err());
    let mut with_orders = keys("d", 1, "a");
    with_orders["left"] = "march north".into();
    assert!(p(with_orders).is_err());
}

#[test]
fn only_the_two_duellists_type_and_everyone_sees_it() {
    let (t, id, _) = open(51);
    let v = |seat: u8, f: Value, kind: SubmitKind| t.p.validate_action(&t.state, seat, "", &f, kind);
    // Refusals: a duel_tick's.
    assert_eq!(v(2, keys(&id, 3, "abc"), SubmitKind::Instant), Err("not_yours".into()), "a third seat");
    assert_eq!(v(0, keys("duel:s99:0:1", 3, "abc"), SubmitKind::Instant), Err("no_duel".into()));
    assert!(v(0, keys(&id, 3, "abc"), SubmitKind::Compose).is_err(), "an instant action only");
    assert_eq!(v(0, keys(&id, 3, "abcde"), SubmitKind::Instant), Err("bad_tail".into()));
    assert!(t.p.on_instant(&t.state, 2, &instant(2, keys(&id, 3, "abc"), 9, 1_100)).is_none(), "and nothing is relayed for it");
    // The relay: exactly the documented shape, for each side.
    let a = submit(&t, &instant(0, keys(&id, 4, "I ki"), 2, 1_200)).unwrap();
    assert_eq!(a, json!({ "event": "duel.typing", "duel_id": id, "by": "a", "demigod": Ref::Demigod(t.me), "len": 4, "tail": "I ki",
                          "at_ms": t0() + 1_200, "seq": 2 }));
    let b = submit(&t, &instant(1, keys(&id, 2, "no"), 3, 1_300)).unwrap();
    assert_eq!((b["by"].as_str(), b["demigod"].clone(), b["len"].as_u64(), b["tail"].as_str(), b["seq"].as_u64()),
               (Some("b"), json!(Ref::Demigod(t.foe)), Some(2), Some("no"), Some(3)));
    let a2 = submit(&t, &instant(0, keys(&id, 8, "ck y"), 4, 1_650)).unwrap();
    assert_eq!((a2["len"].as_u64(), a2["tail"].as_str()), (Some(8), Some("ck y")));
    // A reconnect: no typing event is replayed; each side's current length is in the duel.
    let pr = t.p.public_view(&t.state)["present"].clone();
    let events: Vec<&str> = pr["events"].as_array().unwrap().iter().map(|e| e["event"].as_str().unwrap()).collect();
    assert_eq!(events, ["challenge.lock"], "typing is never journaled");
    assert_eq!(pr["duels"][0]["typing"], json!({ "a": { "len": 8, "at_ms": t0() + 1_650 }, "b": { "len": 2, "at_ms": t0() + 1_300 } }));
    assert_eq!(t.p.query(&t.state, None, "present", &json!({})).unwrap()["duels"][0]["typing"]["a"]["len"], 8);
    // Sending the line clears its author's (the other side keeps typing).
    let blow = submit(&t, &instant(0, line(&id, "I kick your knee into the mud"), 5, 1_900)).unwrap();
    assert_eq!(blow["event"], "duel.blow");
    let ty = t.p.public_view(&t.state)["present"]["duels"][0]["typing"].clone();
    assert_eq!(ty, json!({ "a": { "len": 0, "at_ms": t0() + 1_900 }, "b": { "len": 2, "at_ms": t0() + 1_300 } }));
    // An explicit clear (the player erased the line).
    let z = submit(&t, &instant(1, keys(&id, 0, ""), 6, 2_000)).unwrap();
    assert_eq!((z["len"].as_u64(), z["tail"].as_str()), (Some(0), Some("")));
}

#[test]
fn states_before_0_8_2_refuse_typing() {
    let t = table(52, json!({ "duel_deadline_ms": 30000 }));
    let mut w = world::load(&t.state).unwrap();
    w.meta.schema = fn7_model::dm::PANEL_SCHEMA - 1;
    let old = world::save(&w);
    assert_eq!(t.p.validate_action(&old, 0, "", &keys("duel:s1:0:1", 1, "a"), SubmitKind::Instant), Err("panels_off".into()));
    assert!(t.p.on_instant(&old, 0, &instant(0, keys("duel:s1:0:1", 1, "a"), 2, 1_100)).is_none());
}

/// Typing decides nothing and is in no state: a beat resolved with the relays in its batch is byte for byte the beat
/// resolved without them (state and public outcome), panels included, whatever their stamps; and it stays so on the
/// following beat.
#[test]
fn typing_never_changes_a_resolve() {
    let (t, id, _) = open(53);
    let acts = vec![
        instant(0, challenge(&t, "regions"), 1, 1_000),
        instant(0, keys(&id, 3, "I k"), 2, 1_200),
        instant(1, keys(&id, 2, "no"), 3, 1_300),
        instant(0, line(&id, "I kick your knee into the mud"), 4, 1_500),
        instant(0, keys(&id, 4, "then"), 5, 2_000),
        instant(0, json!({ "duel_tick": { "duel_id": id } }), 6, 2_800),
        instant(1, keys(&id, 9, "duck"), 7, 2_900),
        instant(0, line(&id, "then I smash your jaw with my elbow"), 8, 5_000),
        // The newest stamps of the batch are typing: they are not the resolve's clock and not its delivery mark.
        instant(0, keys(&id, 5, "gain"), 9, 20_000),
        instant(1, keys(&id, 0, ""), 10, 29_500),
    ];
    for a in &acts {
        let _ = submit(&t, a);
    }
    let without: Vec<Action> = acts.iter().filter(|a| a.fields.get("duel_keys").is_none()).cloned().collect();
    assert_eq!(without.len(), 4);
    let with = resolve_at(&FinalNight7Plugin::new(), &t, 2, &acts);
    let plain = resolve_at(&FinalNight7Plugin::new(), &t, 2, &without);
    assert_eq!(with.state, plain.state, "the state");
    // (`timing` is the resolve's own wall-clock stopwatch: it differs between any two runs.)
    let untimed = |o: &RoundOutcome| {
        let mut v = o.public.clone();
        v.as_object_mut().unwrap().remove("timing");
        v
    };
    assert_eq!(untimed(&with), untimed(&plain), "the public outcome");
    assert_eq!(with.per_seat.len(), plain.per_seat.len());
    for (x, y) in with.per_seat.iter().zip(&plain.per_seat) {
        assert_eq!(serde_json::to_value(x).unwrap(), serde_json::to_value(y).unwrap(), "each seat's private outcome");
    }
    // The live plugin (which relayed them) resolves to the same state, and nothing of them is in it.
    let live = resolve_at(&t.p, &t, 2, &acts);
    assert_eq!(live.state, plain.state);
    let text = serde_json::to_string(&live.state).unwrap();
    assert!(!text.contains("duel_keys") && !text.contains("duel.typing"));
    let w = world::load(&live.state).unwrap();
    assert_eq!(w.dm.duels.len(), 1, "the duel is still open");
    let m = w.dm.mark.unwrap();
    assert_eq!((m.at_ms, m.seq), (t0() + 5_000, 8), "the delivery mark is the newest action that counts");
    // The fallback path (a failed resolve) strips them too.
    let mut all = bot_actions(&t.p, &t.state);
    all.extend(acts.iter().cloned());
    let mut some = bot_actions(&t.p, &t.state);
    some.extend(without.iter().cloned());
    let ctx = RoundContext::default();
    assert_eq!(FinalNight7Plugin::new().fallback_outcome(&t.state, 2, &all, &ctx).state,
               FinalNight7Plugin::new().fallback_outcome(&t.state, 2, &some, &ctx).state);
    // The next beat: typing after the resolve, then the present shows the state's duel with its panels untouched.
    let t3 = next_beat(&t, &live, 2);
    let before = t3.p.public_view(&t3.state)["present"]["duels"][0]["panels"].clone();
    submit(&t3, &instant(1, keys(&id, 6, "roll"), 11, 30_000)).unwrap();
    let pr = t3.p.public_view(&t3.state)["present"].clone();
    assert_eq!(pr["duels"][0]["panels"], before, "typing never moves the panel cycle");
    assert!(pr["events"].as_array().unwrap().is_empty());
    assert_eq!(pr["duels"][0]["typing"]["b"]["len"], 6);
}

#[test]
fn skins_by_name_match_the_documented_vectors() {
    assert_eq!(skin::fnv1a32(b""), 0x811c9dc5);
    assert_eq!(skin::fnv1a32(b"a"), 0xe40c292c);
    assert_eq!(skin::fnv1a32(b"foobar"), 0xbf9cf968);
    for (name, hash, want) in VECTORS {
        assert_eq!(skin::fnv1a32(name.to_ascii_lowercase().as_bytes()), *hash, "{name}");
        assert_eq!(skin::by_name(name), *want, "{name}");
    }
    assert_eq!(skin::by_name("ASH'S LEFT HAND"), skin::by_name("ash's left hand"), "case never matters");
    for s in skin::SKINS {
        assert_eq!(skin::parse(&s.to_uppercase()), Some(s));
    }
    assert_eq!(skin::parse("chrome"), None);
}

/// The vectors of docs/CLIENT_API.md ("Skins"): name, FNV-1a 32 of its lower-cased UTF-8 bytes, skin.
const VECTORS: &[(&str, u32, &str)] = &[
    ("Ash's Left Hand", 0xb1aeb883, "obsidian"),
    ("Brix's Right Hand", 0x5b655229, "wood"),
    ("Morrigan", 0x6c70a582, "wood"),
    ("Dune's Right Hand", 0xb090f624, "gold"),
    ("Nyx", 0x197b6960, "butterfly"),
    ("KALI", 0x2eced3aa, "dragonscale"),
    ("kali", 0x2eced3aa, "dragonscale"),
    // Only ASCII letters are lower-cased: the É stays as it is (bytes C3 89).
    ("Émile the Unready", 0x3eadb7d1, "gold"),
];

#[test]
fn the_lock_carries_each_fighters_skin_and_gear_overrides_the_name() {
    let (t, id, lock) = open(54);
    let w = world::load(&t.state).unwrap();
    let (a, b) = (&w.demigods[t.me.ix()], &w.demigods[t.foe.ix()]);
    assert_eq!(lock["a"]["skin"], skin::by_name(&a.name));
    assert_eq!(lock["b"]["skin"], skin::by_name(&b.name));
    let pd = t.p.public_view(&t.state)["present"]["duels"][0].clone();
    assert_eq!((pd["a"]["skin"].clone(), pd["lock"]["b"]["skin"].clone()), (lock["a"]["skin"].clone(), lock["b"]["skin"].clone()));
    // Never in the state (the lock the DM remembers at the hex has none): old recordings replay unchanged.
    let o = resolve_at(&t.p, &t, 2, &[instant(0, challenge(&t, "regions"), 1, 1_000), instant(0, line(&id, "I kick your knee"), 2, 1_500)]);
    assert!(!serde_json::to_string(&o.state).unwrap().contains("\"skin\""));
    // Gear: a held thing of gold, then a god's grant.
    let mut w = w;
    let item = |w: &World, name: &str, origin: ItemOrigin, holder: DemigodId| Item {
        id: ItemId(w.items.len() as u32), name: name.into(), description: String::new(), origin, created_beat: 1,
        created_by: None, holders: vec![Ref::Demigod(holder)], uses: 0, ext: Default::default(),
    };
    assert_eq!(skin::from_gear(&w, &w.demigods[t.me.ix()]), None);
    let it = item(&w, "The Golden Abacus", ItemOrigin::Found, t.me);
    w.items.push(it);
    let it = item(&w, "a marigold wreath", ItemOrigin::Found, t.foe);
    w.items.push(it);
    assert_eq!(skin::of(&w, &w.demigods[t.me.ix()]), "gold");
    assert_eq!(skin::of(&w, &w.demigods[t.foe.ix()]), skin::by_name(&w.demigods[t.foe.ix()].name), "whole words only");
    let it = item(&w, "Death's own scythe", ItemOrigin::Granted { god: GodId(3) }, t.me);
    w.items.push(it);
    assert_eq!(skin::of(&w, &w.demigods[t.me.ix()]), "dragonscale", "a god's grant outranks gold");
    // A thing it no longer holds does not count.
    for i in w.items.iter_mut().filter(|i| i.holders.last() == Some(&Ref::Demigod(t.me))) {
        i.holders.push(Ref::Demigod(t.foe));
    }
    assert_eq!(skin::from_gear(&w, &w.demigods[t.me.ix()]), None);
    assert_eq!(skin::of(&w, &w.demigods[t.foe.ix()]), "dragonscale");
}
