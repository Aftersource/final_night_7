//! Room settings: validated once in the lobby, stored in `World.meta.settings`, read back every hook.

use fn7_model::ArenaKind;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use theatre_plugin_api::util::{copy_timer_settings, int_setting, settings_object};

/// Validated settings. Every field has a default; see the SETTINGS table in the README.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Match seed (all randomness derives from it). Absent in the room settings → picked at match start and recorded.
    pub seed: Option<u64>,
    /// Arena city by name (fuzzy); `None` = a seeded pick among cities ≥ 1M on land.
    pub city: Option<String>,
    /// Demigods in the match. Seats take the first places; the plugin drives the rest as built-in AI demigods.
    pub players: u32,
    /// `planet` (default: the whole Earth, regions → cities; `city` is where the Final Night converges) or `city`
    /// (one real city, districts → sites: the schema-2 game).
    pub arena: ArenaKind,
    /// The Final Night falls on this beat (one Theatre round = one beat). `rounds` is accepted as an alias.
    pub beats_total: u32,
    /// Flow beat length (= `flow_ms`; `beat_ms` is accepted as an alias). The Compose window (`compose_seconds`)
    /// defaults to it.
    pub beat_ms: u32,
    /// Flow beat length: the world's action runs in beats this long whether or not anyone types.
    pub flow_ms: u32,
    /// How long a slow-time window may stay open (it spans `ceil(slow_ms / flow_ms)` flow beats).
    pub slow_ms: u32,
    /// Lightning bolts each demigod starts with.
    pub start_bolts: u32,
    /// Multiplier on every Entropy rise (a playtest dial: 2 = twice as fast to heat death).
    pub entropy_scale: f32,
    pub clusters: u32,
    pub max_subagents_per_hand: u32,
    /// Per-call timeout for every mind; a late call is replaced by its rules fallback.
    pub mind_timeout_ms: u32,
    /// false = never call the LLM (every mind uses its rules fallback, no requests made).
    pub llm: bool,
    /// Consequence's bill multiplier (`fn7_society::debt_for` × this), per demigod per beat.
    pub debt_scale: f32,
    /// Beats until Consequence's bill for a beat falls due.
    pub debt_due_beats: u32,
    /// Live memory items per owner before old ones fold into a summary.
    pub memory_per_owner: u32,
    /// LLM folds per beat (the rest fold by rules).
    pub llm_folds_per_beat: u32,
    /// Most intents one hand turns into acts per beat (orders sent in the same beat they slow time).
    pub max_intents_per_hand: u32,
    /// Most intents one hand turns into acts from orders that closed a slow window that stayed open at least one beat.
    pub max_intents_per_window: u32,
    /// Most people memorized from player mentions per beat.
    pub max_memorize_per_beat: u32,
    /// Most witnesses recorded per act (actors and targets first).
    pub max_witnesses: u32,
    /// World-only simulation: the built-in AI demigods send no orders and cast nothing, so only the society acts.
    pub society_only: bool,
    /// Most general-mind (LLM) calls per beat at decision points (engagements, strain); orders a hand gives a general
    /// always ask the general.
    pub general_calls_per_beat: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            seed: None,
            city: None,
            players: 4,
            arena: ArenaKind::Planet,
            beats_total: 100,
            beat_ms: 3000,
            flow_ms: 3000,
            slow_ms: 30_000,
            start_bolts: 3,
            entropy_scale: 1.0,
            clusters: 200,
            max_subagents_per_hand: 3,
            mind_timeout_ms: 6000,
            llm: true,
            debt_scale: 0.04,
            debt_due_beats: 6,
            memory_per_owner: 40,
            llm_folds_per_beat: 2,
            max_intents_per_hand: 6,
            max_intents_per_window: 12,
            max_memorize_per_beat: 6,
            max_witnesses: 4,
            society_only: false,
            general_calls_per_beat: 2,
        }
    }
}

/// The shortest decimal that reads back as `x` (so `0.04f32` is `0.04`, not `0.03999999910593033`).
fn f32_decimal(x: f32) -> f64 {
    format!("{x}").parse().unwrap_or(x as f64)
}

/// `resolve_timeout_s` when the room doesn't set one: a beat must never wait on the core's 180 s default.
pub const DEFAULT_RESOLVE_TIMEOUT_S: f64 = 60.0;

impl Settings {
    /// Validate and normalize room settings (the object the Theatre stores and passes to `on_match_start`).
    pub fn validate(v: &Value) -> Result<Value, String> {
        let m = settings_object(v)?;
        let d = Settings::default();
        let int = |k: &str, def: u32, min: i64, max: i64| -> Result<u32, String> {
            int_setting(&m, k, def as i64, min, max).map(|x| x as u32)
        };
        let boolean = |k: &str, def: bool| -> Result<bool, String> {
            match m.get(k) {
                None | Some(Value::Null) => Ok(def),
                Some(x) => x.as_bool().ok_or(format!("`{k}` must be true/false")),
            }
        };
        let num = |k: &str, def: f32, min: f32, max: f32| -> Result<f32, String> {
            match m.get(k) {
                None | Some(Value::Null) => Ok(def),
                Some(x) => {
                    let f = x.as_f64().ok_or(format!("`{k}` must be a number"))? as f32;
                    if !(min..=max).contains(&f) {
                        return Err(format!("`{k}` must be between {min} and {max}"));
                    }
                    Ok(f)
                }
            }
        };
        let seed = match m.get("seed") {
            None | Some(Value::Null) => None,
            Some(s) => Some(s.as_u64().ok_or("`seed` must be a non-negative integer")?),
        };
        let city = match m.get("city") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.trim().is_empty() => None,
            Some(Value::String(s)) if s.chars().count() <= 80 => Some(s.trim().to_string()),
            _ => return Err("`city` must be a city name (up to 80 characters)".into()),
        };
        let beats_total = match (m.get("beats_total"), m.get("rounds")) {
            (Some(b), _) if !b.is_null() => int("beats_total", d.beats_total, 1, 1000)?,
            (_, Some(r)) if !r.is_null() => int("rounds", d.beats_total, 1, 1000)?,
            _ => d.beats_total,
        };
        let arena = match m.get("arena") {
            None | Some(Value::Null) => d.arena,
            Some(Value::String(s)) if s.trim().eq_ignore_ascii_case("planet") => ArenaKind::Planet,
            Some(Value::String(s)) if s.trim().eq_ignore_ascii_case("city") => ArenaKind::City,
            _ => return Err("`arena` must be \"planet\" or \"city\"".into()),
        };
        let flow_ms = match (m.get("flow_ms"), m.get("beat_ms")) {
            (Some(f), _) if !f.is_null() => int("flow_ms", d.flow_ms, 500, 120_000)?,
            (_, Some(b)) if !b.is_null() => int("beat_ms", d.flow_ms, 500, 120_000)?,
            _ => d.flow_ms,
        };
        let s = Settings {
            seed,
            city,
            players: int("players", d.players, 1, 8)?,
            arena,
            beats_total,
            beat_ms: flow_ms,
            flow_ms,
            slow_ms: int("slow_ms", d.slow_ms, 1000, 600_000)?,
            start_bolts: int("start_bolts", d.start_bolts, 0, 99)?,
            entropy_scale: num("entropy_scale", d.entropy_scale, 0.0, 10.0)?,
            clusters: int("clusters", d.clusters, 8, 600)?,
            max_subagents_per_hand: int("max_subagents_per_hand", d.max_subagents_per_hand, 0, 10)?,
            mind_timeout_ms: int("mind_timeout_ms", d.mind_timeout_ms, 50, 120_000)?,
            llm: boolean("llm", d.llm)?,
            debt_scale: num("debt_scale", d.debt_scale, 0.0, 10.0)?,
            debt_due_beats: int("debt_due_beats", d.debt_due_beats, 1, 100)?,
            memory_per_owner: int("memory_per_owner", d.memory_per_owner, 4, 1000)?,
            llm_folds_per_beat: int("llm_folds_per_beat", d.llm_folds_per_beat, 0, 32)?,
            max_intents_per_hand: int("max_intents_per_hand", d.max_intents_per_hand, 1, 12)?,
            max_intents_per_window: int("max_intents_per_window", d.max_intents_per_window, 1, 24)?,
            max_memorize_per_beat: int("max_memorize_per_beat", d.max_memorize_per_beat, 0, 64)?,
            max_witnesses: int("max_witnesses", d.max_witnesses, 1, 64)?,
            society_only: boolean("society_only", d.society_only)?,
            general_calls_per_beat: int("general_calls_per_beat", d.general_calls_per_beat, 0, 16)?,
        };
        let mut out = match serde_json::to_value(&s).map_err(|e| e.to_string())? {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        // f32 settings echo back at f32 precision (0.04, not 0.03999999910593033); reading it back gives the same f32.
        out.insert("debt_scale".into(), f32_decimal(s.debt_scale).into());
        out.insert("entropy_scale".into(), f32_decimal(s.entropy_scale).into());
        if s.seed.is_none() {
            out.remove("seed");
        }
        if s.city.is_none() {
            out.remove("city");
        }
        copy_timer_settings(&m, &mut out)?;
        if !out.contains_key("compose_seconds") {
            out.insert("compose_seconds".into(), (s.flow_ms as f64 / 1000.0).into());
        }
        if !out.contains_key("resolve_timeout_s") {
            out.insert("resolve_timeout_s".into(), DEFAULT_RESOLVE_TIMEOUT_S.into());
        }
        Ok(Value::Object(out))
    }

    /// Read validated settings (unknown keys such as the timers are ignored).
    pub fn from_value(v: &Value) -> Settings {
        let d = Settings::default();
        let mut s: Settings = serde_json::from_value(v.clone()).unwrap_or_else(|_| d.clone());
        if s.beats_total == 0 {
            s.beats_total = d.beats_total;
        }
        // Settings recorded before flow/slow (schema 2) had only `beat_ms`.
        if v.get("flow_ms").is_none() {
            s.flow_ms = s.beat_ms.max(500);
            if v.get("arena").is_none() {
                s.arena = ArenaKind::City;
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_aliases_and_errors() {
        let v = Settings::validate(&Value::Null).unwrap();
        assert_eq!(v["beats_total"], 100);
        assert_eq!(v["compose_seconds"], 3.0);
        assert_eq!(v["arena"], "planet");
        assert_eq!((v["flow_ms"].as_u64(), v["slow_ms"].as_u64()), (Some(3000), Some(30000)));
        assert!(Settings::validate(&json!({"arena": "moon"})).is_err());
        let c = Settings::validate(&json!({"arena": "city", "beat_ms": 6000})).unwrap();
        assert_eq!((c["flow_ms"].as_u64(), c["compose_seconds"].as_f64()), (Some(6000), Some(6.0)));
        assert_eq!(v["resolve_timeout_s"], DEFAULT_RESOLVE_TIMEOUT_S);
        assert!(v.get("seed").is_none());
        let v = Settings::validate(
            &json!({"rounds": 12, "beat_ms": 3000, "seed": 9, "compose_seconds": 4}),
        )
        .unwrap();
        assert_eq!(
            (v["beats_total"].as_u64(), v["seed"].as_u64()),
            (Some(12), Some(9))
        );
        assert_eq!(v["compose_seconds"], 4);
        let s = Settings::from_value(&v);
        assert_eq!((s.beats_total, s.beat_ms, s.flow_ms, s.seed), (12, 3000, 3000, Some(9)));
        let v = Settings::validate(&json!({"debt_scale": 0.04})).unwrap();
        assert_eq!(v["debt_scale"].to_string(), "0.04");
        assert_eq!(Settings::from_value(&v).debt_scale, 0.04f32);
        assert!(Settings::validate(&json!({"players": 0})).is_err());
        assert!(Settings::validate(&json!({"llm": "yes"})).is_err());
        assert!(Settings::validate(&json!({"seed": -1})).is_err());
        assert!(Settings::validate(&json!([1])).is_err());
        assert!(Settings::validate(&json!({"compose_seconds": 0})).is_err());
    }

    #[test]
    fn allow_midmatch_join_passes_through() {
        // The core's room flag (mid-match join) survives validation (copy_timer_settings), as a boolean only.
        let v = Settings::validate(&json!({"allow_midmatch_join": true})).unwrap();
        assert_eq!(v["allow_midmatch_join"], true);
        assert_eq!(Settings::validate(&json!({"allow_midmatch_join": false})).unwrap()["allow_midmatch_join"], false);
        assert!(Settings::validate(&Value::Null).unwrap().get("allow_midmatch_join").is_none(), "absent = off");
        assert!(Settings::validate(&json!({"allow_midmatch_join": "yes"})).is_err());
        // Through the plugin, too (what the room stores and returns as match.started.rules).
        use theatre_plugin_api::GamePlugin;
        let p = crate::FinalNight7Plugin::new();
        assert_eq!(p.validate_settings(&json!({"allow_midmatch_join": true, "players": 4})).unwrap()["allow_midmatch_join"], true);
    }
}
