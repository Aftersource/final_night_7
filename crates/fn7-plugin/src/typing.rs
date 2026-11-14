//! Live typing in a duel: `fields.duel_keys {duel_id, len, tail}` → the public phase event `duel.typing`.
//!
//! The designer wants both the player's text and the opponent's text visible as each is locked in.
//! password type obfuscation - if you can watch what your opponent said character for character and read it... cool.
//! You understand what they said, otherwise, you're winging it."
//!
//! A duellist's client sends, while its player types, how long the line is now (`len`) and the newest characters
//! (`tail`, at most [`MAX_TAIL`]). The server checks it and relays it at once to everyone. **That is all it does:**
//!
//! - it decides nothing (the line that counts is the `fields.duel` sent on Enter);
//! - it is not in the state: [`strip`] takes these actions out of a resolve's batch before anything reads it, so a
//!   beat resolved with them is byte for byte the beat resolved without them (and a recording that contains them
//!   replays as before: the resolve is still a pure function of its recorded inputs);
//! - it is not journaled ([`crate::present::Journal`]): a reconnecting client is not replayed old keystrokes and a
//!   restarted server recovers none. Only the current length per side is kept, in memory ([`Typing`]), and shown as
//!   `present.duels[].typing`;
//! - it never touches the panel cycle (it is not a tick and its stamp closes no window).
//!
//! No state schema and no rules version: an optional action field and an ephemeral event.

use std::collections::HashMap;
use std::sync::Mutex;

use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::Action;

use crate::actions::MAX_DUEL_CHARS;

/// The action field.
pub const FIELD: &str = "duel_keys";
/// Longest `tail` (characters).
pub const MAX_TAIL: usize = 8;
/// Lengths not refreshed for this long are dropped from the cache when it grows (a duel lasts seconds to minutes).
const STALE_MS: i64 = 10 * 60 * 1000;

/// `fields.duel_keys {duel_id, len, tail}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeysOrder {
    pub duel_id: String,
    /// Characters in the line being typed, 0..=[`MAX_DUEL_CHARS`].
    pub len: u32,
    /// The newest characters typed since the sender's last message: 0..=[`MAX_TAIL`] (empty after a deletion).
    pub tail: String,
}

/// Characters a relayed tail never carries: controls, and the invisible format characters that reorder or hide text.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Parse `fields.duel_keys`. Refusals: `no_duel` (no id), `bad_len` (not an integer 0..=120), `bad_tail` (not a
/// string, longer than 8 characters, longer than `len`, or with a control or invisible format character).
pub fn keys_of(fields: &Value) -> Result<Option<KeysOrder>, String> {
    let Some(v) = fields.get(FIELD).filter(|v| !v.is_null()) else { return Ok(None) };
    let o = v.as_object().ok_or("no_duel")?;
    let duel_id: String =
        o.get("duel_id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or("no_duel")?.chars().take(64).collect();
    let len = o.get("len").and_then(Value::as_u64).filter(|n| *n <= MAX_DUEL_CHARS as u64).ok_or("bad_len")? as u32;
    let tail = match o.get("tail") {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err("bad_tail".into()),
    };
    let n = tail.chars().count();
    if n > MAX_TAIL || n as u32 > len || tail.chars().any(hidden) {
        return Err("bad_tail".into());
    }
    Ok(Some(KeysOrder { duel_id, len, tail: tail.to_string() }))
}

/// Is this delivered action a typing relay (nothing a resolve may read)?
pub fn is_keys(a: &Action) -> bool {
    a.instant && a.fields.get(FIELD).is_some_and(|v| !v.is_null())
}

/// A resolve's batch without the typing relays; `None` when it has none (the usual case: no copy).
pub fn strip(actions: &[Action]) -> Option<Vec<Action>> {
    actions.iter().any(is_keys).then(|| actions.iter().filter(|a| !is_keys(a)).cloned().collect())
}

/// The sender's side (0 = a, 1 = b) and demigod in the duel it names. Refusals as a `duel_tick`'s: `challenge_disabled`,
/// `panels_off` (a state before rules 0.8.2), `no_duel` (no such duel under way, or its outro is out), `not_yours`
/// (only its two duellists type).
fn side_of(j: &crate::present::Journal, w: &World, seat: u8, k: &KeysOrder) -> Result<(usize, DemigodId), String> {
    let st = crate::world::settings(w);
    if !st.challenge_enabled || !w.dm_rules() {
        return Err("challenge_disabled".into());
    }
    if !w.panel_rules() {
        return Err("panels_off".into());
    }
    let lv = j.live(w, &st);
    let bout = lv.bouts.iter().find(|b| b.duel.id == k.duel_id).ok_or("no_duel")?;
    let side = bout.duel.side_of(seat).ok_or("not_yours")?;
    if !crate::panel::running(bout) {
        return Err("no_duel".into());
    }
    Ok((side, bout.duel.f[side].id))
}

/// `validate_action` for a `duel_keys`.
pub fn validate(j: &crate::present::Journal, w: &World, seat: u8, k: &KeysOrder) -> Result<(), String> {
    side_of(j, w, seat, k).map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    seat: u8,
    len: u32,
    at_ms: i64,
}

/// How long each duellist's unsent line is right now, per match (seed) and duel. Memory only, display only: never
/// read by a resolve, lost at a restart (the clients' next message fills it again).
#[derive(Debug, Default)]
pub struct Typing {
    inner: Mutex<HashMap<(u64, String), [Option<Slot>; 2]>>,
}

impl Typing {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(u64, String), [Option<Slot>; 2]>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn note(&self, seed: u64, duel_id: &str, side: usize, slot: Slot) {
        let mut m = self.lock();
        if m.len() >= 256 {
            let old = slot.at_ms - STALE_MS;
            m.retain(|_, v| v.iter().flatten().any(|s| s.at_ms >= old));
        }
        m.entry((seed, duel_id.to_string())).or_default()[side.min(1)] = Some(slot);
    }

    /// The seat sent its line (`fields.duel`): what it was typing is gone.
    pub fn sent(&self, seed: u64, duel_id: &str, seat: u8, at_ms: i64) {
        if let Some(v) = self.lock().get_mut(&(seed, duel_id.to_string())) {
            for s in v.iter_mut().flatten().filter(|s| s.seat == seat) {
                (s.len, s.at_ms) = (0, at_ms);
            }
        }
    }

    /// `on_instant` for a `duel_keys`: the public `duel.typing` event, and the length noted for `present`.
    pub fn relay(&self, j: &crate::present::Journal, w: &World, seat: u8, a: &Action) -> Option<Value> {
        let k = keys_of(&a.fields).ok()??;
        let (side, demigod) = side_of(j, w, seat, &k).ok()?;
        self.note(w.meta.seed, &k.duel_id, side, Slot { seat, len: k.len, at_ms: a.received_at_ms });
        Some(json!({ "event": "duel.typing", "duel_id": k.duel_id, "by": if side == 0 { "a" } else { "b" },
                     "demigod": Ref::Demigod(demigod), "len": k.len, "tail": k.tail,
                     "at_ms": a.received_at_ms, "seq": a.seq }))
    }

    /// Add `typing: {a: {len, at_ms}, b: {len, at_ms}}` to every open duel of a `present` (a view of the past, or of
    /// a duel nobody typed in since this process started: 0 and null).
    pub fn fill(&self, seed: u64, present: &mut Value) {
        let live = present["live"] == true;
        let m = self.lock();
        let Some(duels) = present["duels"].as_array_mut() else { return };
        for d in duels {
            let slots = d["duel_id"].as_str().filter(|_| live).and_then(|id| m.get(&(seed, id.to_string()))).copied().unwrap_or_default();
            let side = |s: Option<Slot>| json!({ "len": s.map(|s| s.len).unwrap_or(0), "at_ms": s.map(|s| s.at_ms) });
            d["typing"] = json!({ "a": side(slots[0]), "b": side(slots[1]) });
        }
    }
}
