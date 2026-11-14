//! Accounts in the match: a signed-in player's two demigods carry their custom names and the character the player
//! wrote for them (docs/ACCOUNTS.md "In the match").
//!
//! The plugin knows nothing about accounts, sessions or the store. The server resolves a seat to an account (see
//! `fn7-server::accounts`) and hands the plugin a [`SeatPersona`]: already moderated text, the account's public id and
//! the two optional custom names. [`apply`] writes it into the world:
//! - `seats[seat].account` = the account's public id (history accrues to the account; never shown to other seats);
//! - each of the seat's two bodies gets `persona` ([`fn7_model::PlayerPersona`]: personality, style, creed and the
//!   player's pronouns), which the hand and step prompts show fenced as untrusted player text;
//! - a body with a custom name is renamed (its hand agent follows; refs never change). A custom name another demigod
//!   in the match already carries is not applied: the body keeps `"<player>'s Left Hand"` / `"… Right Hand"`.
//!   The image identity is the name's ([`crate::visual::identity`]), as for every demigod.
//!
//! **State.** Only optional fields (`Demigod::persona`, `SeatState::account`), absent unless a signed-in player sits,
//! so every other state serializes exactly as before and needs no schema number. **Replay.** The server applies a
//! persona right after `on_match_start` (inside the recorded snapshot) or `on_seat_takeover`, and records the
//! `seat_persona` event with exactly the [`SeatPersona`] it applied; `fn7-server::replay` feeds that event back through
//! [`apply_to_state`], so a recording with personas replays byte for byte and one without never reaches this module.

use fn7_model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::world::event;

/// The trace event that records an applied persona (data: `{round, persona: SeatPersona, renamed: [...]}`).
pub const EVENT: &str = "seat_persona";
/// The trace event for a takeover the server refused (an unbound player on an account's name): `{round, seat}`.
pub const REFUSED_EVENT: &str = "seat_refused";

/// One demigod's part of a [`SeatPersona`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HandPersona {
    /// The custom name; `None` keeps `"<player>'s Left Hand"` / `"<player>'s Right Hand"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub personality: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub creed: String,
}

/// What a signed-in player brings to a seat.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SeatPersona {
    pub seat: u8,
    /// The account's public id (`acc_…`): not a secret, never an email.
    pub account: String,
    /// One of the settings document's pronoun values (`""` = unset).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pronouns: String,
    #[serde(default)]
    pub left: HandPersona,
    #[serde(default)]
    pub right: HandPersona,
}

/// Longest demigod name kept (as the plugin keeps of a display name).
pub const MAX_NAME: usize = 40;

/// Write `p` into the world (see the module doc). Returns the renames made, as `{demigod, from, to}`; the world always
/// changes when the seat exists (its `account` is set). A seat the world doesn't have, or a state from before two
/// demigods per player (schema < 5), is left alone (`None`).
pub fn apply(w: &mut World, p: &SeatPersona, round: u32) -> Option<Vec<Value>> {
    if !w.two_bodies() || p.account.is_empty() {
        return None;
    }
    let i = w.seats.iter().position(|s| s.seat == p.seat)?;
    w.seats[i].account = Some(p.account.clone());
    let ids: Vec<DemigodId> = w.bodies_of_seat(p.seat).filter(|d| d.side.is_some()).map(|d| d.id).collect();
    let mut renamed = vec![];
    for d in ids {
        let hand = if w.demigods[d.ix()].side == Some(Hand::Right) { &p.right } else { &p.left };
        let persona = PlayerPersona {
            personality: hand.personality.clone(),
            style: hand.style.clone(),
            creed: hand.creed.clone(),
            pronouns: p.pronouns.clone(),
        };
        w.demigods[d.ix()].persona = (!persona.is_empty()).then_some(persona);
        let Some(new) = hand.name.as_deref().map(clean_name).filter(|n| !n.is_empty()) else {
            continue;
        };
        let old = w.demigods[d.ix()].name.clone();
        if old == new {
            continue;
        }
        // The same identity on another body (anyone's) would share its portrait and confuse every prompt: keep the default.
        let id = crate::visual::identity(&new);
        if w.demigods.iter().any(|x| x.id != d && crate::visual::identity(&x.name) == id) {
            event(
                "demigod_rename_skipped",
                Some(Ref::Demigod(d).to_string()),
                json!({ "round": round, "wanted": new, "kept": old, "reason": "another demigod in this match has that name" }),
            );
            continue;
        }
        w.demigods[d.ix()].name = new.clone();
        for h in w.demigods[d.ix()].hands {
            if let Some(a) = w.agents.get_mut(h.ix()) {
                a.name = new.clone();
            }
        }
        event("demigod_renamed", Some(Ref::Demigod(d).to_string()), json!({ "round": round, "from": old, "to": new }));
        renamed.push(json!({ "demigod": Ref::Demigod(d), "from": old, "to": new }));
    }
    Some(renamed)
}

/// [`apply`] on a serialized state, recording the [`EVENT`]. `None` when nothing applied (the state is unchanged).
pub fn apply_to_state(state: &Value, p: &SeatPersona, round: u32) -> Option<Value> {
    let mut w = crate::world::load(state).ok()?;
    let renamed = apply(&mut w, p, round)?;
    event(EVENT, Some(format!("seat:{}", p.seat)), json!({ "round": round, "persona": p, "renamed": renamed }));
    Some(crate::world::save(&w))
}

/// A custom demigod name as the match keeps it: trimmed, whitespace collapsed, at most [`MAX_NAME`] characters.
pub fn clean_name(raw: &str) -> String {
    let s = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(MAX_NAME).collect::<String>().trim_end().to_string()
}

/// The persona fields a public demigod object carries (`persona: {personality?, style?, creed?, pronouns?}`), merged in
/// only when the player wrote something: every other demigod object is unchanged.
pub fn add_fields(d: &Demigod, v: &mut Value) {
    if let (Some(o), Some(p)) = (v.as_object_mut(), d.persona.as_ref().filter(|p| !p.is_empty())) {
        o.insert("persona".into(), json!(p));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theatre_plugin_api::SeatInfo;

    fn world() -> World {
        let seats = [
            SeatInfo { seat: 0, display_name: "Chris".into(), is_bot: false },
            SeatInfo { seat: 1, display_name: "Benito".into(), is_bot: false },
        ];
        let pantheon = fn7_pantheon::load();
        crate::phases::new_match(&seats, &json!({ "seed": 7, "players": 4 }), &pantheon, 7)
    }

    fn persona() -> SeatPersona {
        SeatPersona {
            seat: 0,
            account: "acc_test".into(),
            pronouns: "she".into(),
            left: HandPersona {
                name: Some("  Ashen   Vow ".into()),
                personality: "Dry, patient, counts every debt.".into(),
                style: "Short sentences.".into(),
                creed: "Nothing is owed twice.".into(),
            },
            right: HandPersona::default(),
        }
    }

    #[test]
    fn a_state_without_personas_serializes_as_before() {
        let w = world();
        let v = crate::world::save(&w);
        for d in v["demigods"].as_array().unwrap() {
            assert!(d.get("persona").is_none(), "no persona key unless a player wrote one");
        }
        for s in v["seats"].as_array().unwrap() {
            assert!(s.get("account").is_none(), "no account key unless a seat is bound");
        }
    }

    #[test]
    fn applies_names_persona_and_account_to_the_seat_only() {
        let mut w = world();
        let before_other: Vec<String> = w.bodies_of_seat(1).map(|d| d.name.clone()).collect();
        let renamed = apply(&mut w, &persona(), 0).expect("applied");
        assert_eq!(renamed.len(), 1);
        let left = w.bodies_of_seat(0).find(|d| d.side == Some(Hand::Left)).unwrap().clone();
        let right = w.bodies_of_seat(0).find(|d| d.side == Some(Hand::Right)).unwrap().clone();
        assert_eq!(left.name, "Ashen Vow");
        assert_eq!(w.agents[left.hands[0].ix()].name, "Ashen Vow", "the hand agent follows the name");
        assert_eq!(right.name, "Chris's Right Hand", "no custom name: the default stays");
        let p = left.persona.expect("persona");
        assert_eq!(p.personality, "Dry, patient, counts every debt.");
        assert_eq!(p.pronouns, "she");
        // The right hand has no text of its own but carries the player's pronouns.
        assert_eq!(right.persona.map(|p| p.pronouns), Some("she".into()));
        assert_eq!(w.seats.iter().find(|s| s.seat == 0).unwrap().account.as_deref(), Some("acc_test"));
        assert_eq!(w.seats.iter().find(|s| s.seat == 1).unwrap().account, None);
        assert_eq!(w.bodies_of_seat(1).map(|d| d.name.clone()).collect::<Vec<_>>(), before_other);
        assert!(w.bodies_of_seat(1).all(|d| d.persona.is_none()));
    }

    #[test]
    fn a_name_another_demigod_carries_is_not_applied() {
        let mut w = world();
        let taken = w.bodies_of_seat(1).next().unwrap().name.clone();
        let mut p = persona();
        p.left.name = Some(taken.to_uppercase());
        let renamed = apply(&mut w, &p, 0).expect("applied");
        assert!(renamed.is_empty());
        assert_eq!(w.bodies_of_seat(0).find(|d| d.side == Some(Hand::Left)).unwrap().name, "Chris's Left Hand");
    }

    #[test]
    fn round_trips_through_the_state_and_shows_in_public_objects() {
        let w = world();
        let st = crate::world::save(&w);
        let st2 = apply_to_state(&st, &persona(), 0).expect("changed");
        let w2 = crate::world::load(&st2).unwrap();
        let left = w2.bodies_of_seat(0).find(|d| d.side == Some(Hand::Left)).unwrap();
        let mut v = json!({ "name": left.name });
        add_fields(left, &mut v);
        assert_eq!(v["persona"]["creed"], "Nothing is owed twice.");
        let other = w2.bodies_of_seat(1).next().unwrap();
        let mut v = json!({});
        add_fields(other, &mut v);
        assert!(v.get("persona").is_none());
        // An unknown seat or an empty account changes nothing.
        let mut p = persona();
        p.seat = 9;
        assert!(apply_to_state(&st, &p, 0).is_none());
    }
}
