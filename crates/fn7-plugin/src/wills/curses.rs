//! Seals and curses (rules 0.9.0, DM_RULEBOOK chapter 26): first-class objects in the state (`Book::curses`).
//!
//! A curse has an owner, a target, a strength, a duration (or "until countered"), what was paid for it, a public face
//! (the mark on the target's piece: its kind, never its owner) and a private one (the owner, and the counter-quest's
//! steps, for the bearer's and the owner's seats), and ALWAYS a counter: a mystic quest the DM generates the moment the
//! curse is made (`quests::give`).
//!
//! | Kind | On | What it does |
//! |---|---|---|
//! | `seal` | a demigod (fallen, or living: it bites when it falls) | NO RESURRECTION: nothing raises it |
//! | `wither` | a living demigod | no healing reaches it (wills, shrines, gear, gods) |
//! | `silence` | a living demigod | its player's wills cost double |
//! | `bind` | a living demigod | it can't be summoned |
//! | `hex` | a living demigod | minus its strength on that demigod's d20s |

use fn7_model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::Book;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurseKind {
    Seal,
    Wither,
    Silence,
    Bind,
    Hex,
}

impl CurseKind {
    pub const ALL: [CurseKind; 5] = [CurseKind::Seal, CurseKind::Wither, CurseKind::Silence, CurseKind::Bind, CurseKind::Hex];
    pub fn as_str(self) -> &'static str {
        match self {
            CurseKind::Seal => "seal",
            CurseKind::Wither => "wither",
            CurseKind::Silence => "silence",
            CurseKind::Bind => "bind",
            CurseKind::Hex => "hex",
        }
    }
    /// What the table calls it.
    pub fn title(self) -> &'static str {
        match self {
            CurseKind::Seal => "a seal of no resurrection",
            CurseKind::Wither => "a curse of withering",
            CurseKind::Silence => "a curse of silence",
            CurseKind::Bind => "a binding",
            CurseKind::Hex => "a hex",
        }
    }
    /// What it does, in plain words.
    pub fn does(self) -> &'static str {
        match self {
            CurseKind::Seal => "nothing can raise it from the dead",
            CurseKind::Wither => "no healing reaches it",
            CurseKind::Silence => "its player's wills cost double",
            CurseKind::Bind => "it can't be summoned",
            CurseKind::Hex => "its d20s are lowered by the hex's strength",
        }
    }
    /// The god whose god-tier variant this is (by slug).
    pub fn god(self) -> &'static str {
        match self {
            CurseKind::Seal | CurseKind::Wither => "death",
            CurseKind::Silence => "memory",
            CurseKind::Bind => "time",
            CurseKind::Hex => "chance",
        }
    }
    /// The kind a will's words ask for (`None`: no curse words at all).
    pub fn from_text(text: &str) -> Option<CurseKind> {
        let t = text.to_lowercase();
        let has = |ws: &[&str]| ws.iter().any(|w| fn7_agents::common::has_phrase(&t, w));
        if has(&["no resurrection", "no resurrect", "no ressurect", "no rez", "seal", "seals", "sealed", "stay dead", "stays dead", "never rise",
                 "never rises", "cannot be raised", "can't be raised", "no raising", "no revive", "no reviving"]) {
            Some(CurseKind::Seal)
        } else if has(&["wither", "withers", "withering", "no healing", "cannot heal", "can't heal", "blight", "rot"]) {
            Some(CurseKind::Wither)
        } else if has(&["silence", "silences", "mute", "gag", "muzzle"]) {
            Some(CurseKind::Silence)
        } else if has(&["bind", "binds", "binding", "root", "chain", "chains", "anchor", "shackle", "cannot be summoned", "can't be summoned"]) {
            Some(CurseKind::Bind)
        } else if has(&["hex", "hexes", "jinx", "curse", "curses", "cursed", "malediction", "evil eye"]) {
            Some(CurseKind::Hex)
        } else {
            None
        }
    }
}

/// One seal or curse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curse {
    pub id: u32,
    pub kind: CurseKind,
    /// Who cast it (private: the public face never names the owner).
    pub owner: DemigodId,
    pub owner_seat: u8,
    pub target: DemigodId,
    /// 1 (weak) .. 3 (god-tier): the counter-quest's length, a hex's bite.
    pub strength: u8,
    pub cast_beat: u32,
    /// The last beat it holds (`None`: until countered).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_beat: Option<u32>,
    /// What was paid for it, in sparks (a bolt counts as `will_spark_per_bolt`).
    pub paid_sparks: u32,
    /// The god whose god-tier variant it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub god: Option<GodId>,
    /// Its counter: the mystic quest that breaks it (`Book::quests` id).
    pub quest: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken_beat: Option<u32>,
    /// `countered` (its quest was done) | `faded` (its time ran out).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub broken_how: String,
}

impl Curse {
    pub fn holds(&self, beat: u32) -> bool {
        self.broken_beat.is_none() && self.until_beat.is_none_or(|u| beat <= u)
    }
}

/// The curse of `kind` holding on `d` at `beat`, if any.
pub fn active(b: &Book, d: DemigodId, kind: CurseKind, beat: u32) -> Option<&Curse> {
    b.curses.iter().find(|c| c.target == d && c.kind == kind && c.holds(beat))
}

/// Every curse holding on `d` at `beat`.
pub fn on(b: &Book, d: DemigodId, beat: u32) -> impl Iterator<Item = &Curse> {
    b.curses.iter().filter(move |c| c.target == d && c.holds(beat))
}

/// Why `d` can't be summoned now (rules 0.9.0: a binding), if it can't.
pub fn bound(w: &World, d: DemigodId) -> Option<String> {
    if !super::on(w) {
        return None;
    }
    let b = super::book(w);
    active(&b, d, CurseKind::Bind, w.meta.beat.max(1)).map(|c| format!("bound:{}", super::quests::qref(c.quest)))
}

/// The public face: the mark on the target's piece. Never the owner, never the counter's steps.
pub fn public_json(w: &World, c: &Curse) -> Value {
    json!({
        "ref": cref(c.id), "kind": c.kind, "title": c.kind.title(), "does": c.kind.does(),
        "target": Ref::Demigod(c.target), "target_name": w.name_of(Ref::Demigod(c.target)),
        "strength": c.strength, "cast_beat": c.cast_beat, "until_beat": c.until_beat,
        "god_tier": c.god.is_some(), "god": c.god.map(|g| w.name_of(Ref::God(g))),
        "counter": "a mystic quest", "broken_beat": c.broken_beat, "broken_how": c.broken_how,
    })
}

/// The private face (the bearer's and the owner's seats): the owner and the counter-quest.
pub fn private_json(w: &World, b: &Book, c: &Curse, seat: u8) -> Value {
    let mut v = public_json(w, c);
    v["owner"] = json!(Ref::Demigod(c.owner));
    v["owner_name"] = w.name_of(Ref::Demigod(c.owner)).into();
    v["yours"] = (c.owner_seat == seat).into();
    v["on_you"] = (w.demigods.get(c.target.ix()).map(|d| d.seat) == Some(seat)).into();
    v["paid_sparks"] = c.paid_sparks.into();
    v["quest"] = b.quests.iter().find(|q| q.id == c.quest).map(|q| super::quests::quest_json(w, q)).unwrap_or(Value::Null);
    v
}

pub fn cref(id: u32) -> String {
    format!("cu:{id}")
}
