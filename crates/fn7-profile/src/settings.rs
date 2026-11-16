//! The account settings document (`GET/PUT /api/me/settings`, docs/ACCOUNTS.md "Settings"): one JSON object per
//! account, validated against [`schema`] (also published as `docs/schemas/account_settings.schema.json`).
//!
//! - **Versioned**: `version` is [`VERSION`]. A stored document of an older version goes through [`upgrade`] when it
//!   is read (there is only version 1 so far); a `PUT` with a version this server doesn't know is refused.
//! - **Defaults**: every key has one ([`defaults`]). A `PUT` replaces the whole document: keys it leaves out go back
//!   to their defaults (read, change, write back).
//! - **Unknown keys are refused**, with the path of the first one (`hud.colour`), at every level.
//! - **Size-capped**: the request body is at most [`MAX_BYTES`] bytes.
//! - `display_name` is the account's display name: reading it gives the current one, writing a different one renames
//!   the account (the name rules of [`crate::accounts`] apply).
//!
//! The validator understands exactly the JSON Schema keywords the schema uses (`type`, `properties`,
//! `additionalProperties`, `maxProperties`, `enum`, `minimum`, `maximum`, `minLength`, `maxLength`, `default`), plus
//! `x-format` for the three strings whose `pattern` it checks in code (there is no regex engine in the workspace).

use serde_json::{json, Map, Value};

/// The document version this server writes.
pub const VERSION: u32 = 1;
/// Largest `PUT` body accepted, in bytes.
pub const MAX_BYTES: usize = 16 * 1024;
/// Most keybind overrides kept.
pub const MAX_KEYBINDS: usize = 64;

/// The pronoun values (narration and the demigods' prompts use them; `unspecified` uses the demigod's name).
pub const PRONOUNS: [&str; 5] = ["unspecified", "she", "he", "they", "it"];

/// The heroes a player can play as (`""` = none): their demigods are named "Chris's Left Hand" / "... Right Hand"
/// (or Benito's) in a schema-23 match, and wear those portraits and figures.
pub const HEROES: [&str; 3] = ["", "chris", "benito"];

fn num(min: f64, max: f64, default: f64, d: &str) -> Value {
    json!({ "type": "number", "minimum": min, "maximum": max, "default": default, "description": d })
}

fn flag(default: bool, d: &str) -> Value {
    json!({ "type": "boolean", "default": default, "description": d })
}

fn choice(values: &[&str], default: &str, d: &str) -> Value {
    json!({ "type": "string", "enum": values, "default": default, "description": d })
}

fn object(props: Value, d: &str) -> Value {
    json!({ "type": "object", "additionalProperties": false, "properties": props, "description": d })
}

/// The settings document's JSON Schema (draft 2020-12).
pub fn schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "account_settings.schema.json",
        "title": "Account settings",
        "description": "GET/PUT /api/me/settings. Every key is optional on PUT and takes its default when left out; unknown keys are refused; the body is at most 16384 bytes.",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "version": { "type": "integer", "minimum": 1, "maximum": VERSION, "default": VERSION,
                         "description": "The document version. Send the one you read." },
            "display_name": { "type": "string", "minLength": 1, "maxLength": 24, "x-format": "display_name",
                              "description": "The account's display name (the name on the websocket's auth.login). Writing a different one renames the account: 409 name_taken / name_lookalike / profile_claimed if it cannot. Default: the current name." },
            "pronouns": choice(&PRONOUNS, "unspecified", "How narration and your demigods' minds refer to you. unspecified: by name."),
            "hero": choice(&HEROES, "", "Play as a hero: chris or benito names your two demigods \"Chris's Left Hand\" / \"Chris's Right Hand\" (or Benito's) from your next match or takeover on, with that hero's portrait, statue and animated figure (over any custom demigod names). Empty: your own demigods."),
            "audio": object(json!({
                "master": num(0.0, 1.0, 0.8, "Master volume."),
                "music": num(0.0, 1.0, 0.6, "Music volume."),
                "sfx": num(0.0, 1.0, 0.8, "Effects volume."),
                "voice": num(0.0, 1.0, 1.0, "Narration and voice volume."),
                "mute_when_unfocused": flag(true, "Mute while the window is in the background."),
            }), "Volumes, each 0 to 1."),
            "camera": object(json!({
                "pan_speed": num(0.1, 5.0, 1.0, "Camera pan speed multiplier."),
                "zoom_speed": num(0.1, 5.0, 1.0, "Zoom speed multiplier."),
                "edge_pan": flag(true, "Pan when the pointer touches a screen edge."),
                "invert_zoom": flag(false, "Invert the zoom direction."),
            }), "Camera feel."),
            "accessibility": object(json!({
                "aura_palette": choice(&["default", "deuteranopia", "protanopia", "tritanopia", "high_contrast"], "default",
                                       "The colour-blind palette for demigod auras."),
                "reduce_flashing": flag(false, "Tone down lightning flashes, the challenge lock's flashes and screen shakes."),
                "text_scale": num(0.75, 2.0, 1.0, "UI text scale."),
                "subtitles": flag(true, "Show narration as text."),
            }), "Accessibility."),
            "content": object(json!({
                "gore": choice(&["full", "reduced", "off"], "full", "How much of the cartoon gore the client shows."),
                "profanity": choice(&["on", "masked"], "on", "Mask profanity in text the client shows."),
                "show_player_personas": flag(true, "Show other players' demigod personality text."),
            }), "Content preferences (applied by the client)."),
            "language": { "type": "string", "minLength": 2, "maxLength": 35, "default": "en", "x-format": "language",
                          "pattern": "^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$",
                          "description": "UI language as a BCP 47 tag (en, pt-BR, zh-Hant)." },
            "keybinds": { "type": "object", "default": {}, "maxProperties": MAX_KEYBINDS, "x-format": "keybinds",
                          "propertyNames": { "pattern": "^[a-z][a-z0-9_.]{0,39}$" },
                          "additionalProperties": { "type": "string", "minLength": 1, "maxLength": 40, "pattern": "^[A-Za-z0-9+_-]{1,40}$" },
                          "description": "Keybind overrides: action name (lower_snake, dots allowed: camera.pan_left) to a key chord as the client names it (Ctrl+Shift+A, SpaceBar, LeftMouseButton). Only overrides are stored; an action left out uses the client's default." },
            "hud": object(json!({
                "layout": choice(&["default", "compact", "minimal"], "default", "The HUD layout preset."),
                "scale": num(0.5, 2.0, 1.0, "HUD scale."),
                "demigod_cards": choice(&["lower_left", "lower_right"], "lower_left", "Where the two demigod cards sit."),
                "show_minimap": flag(true, "Show the minimap."),
                "show_chat": flag(true, "Show room chat."),
                "show_hints": flag(true, "Show control hints."),
            }), "HUD layout."),
        }
    })
}

/// The whole document at its defaults, for an account named `display_name`.
pub fn defaults(display_name: &str) -> Value {
    let mut v = fill(&schema(), None).unwrap_or_else(|| json!({}));
    v["display_name"] = display_name.into();
    v
}

/// The default of a schema node (objects: their properties' defaults), `None` when it has none.
fn fill(schema: &Value, have: Option<&Value>) -> Option<Value> {
    if let Some(v) = have {
        return Some(v.clone());
    }
    if let Some(d) = schema.get("default") {
        return Some(d.clone());
    }
    let props = schema.get("properties")?.as_object()?;
    let mut out = Map::new();
    for (k, s) in props {
        if let Some(v) = fill(s, None) {
            out.insert(k.clone(), v);
        }
    }
    Some(Value::Object(out))
}

fn is_language(s: &str) -> bool {
    let mut parts = s.split('-');
    let first = parts.next().unwrap_or("");
    (2..=3).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_lowercase())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn is_action(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 40
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.'))
}

fn is_key(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 40 && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'_' | b'-'))
}

fn path_of(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// Validate `value` against `schema`, returning it with every missing key at its default.
fn check(schema: &Value, value: &Value, path: &str) -> Result<Value, String> {
    let here = if path.is_empty() { "the document" } else { path };
    match schema["type"].as_str().unwrap_or("object") {
        "object" => {
            let obj = value.as_object().ok_or_else(|| format!("{here} must be an object"))?;
            if schema["x-format"] == "keybinds" {
                if obj.len() > MAX_KEYBINDS {
                    return Err(format!("{here} holds at most {MAX_KEYBINDS} overrides"));
                }
                for (k, v) in obj {
                    if !is_action(k) {
                        return Err(format!("{}: an action name is lower_snake (dots allowed), at most 40 characters", path_of(path, k)));
                    }
                    if !v.as_str().is_some_and(is_key) {
                        return Err(format!("{}: a key chord is 1 to 40 of A-Z a-z 0-9 + _ -", path_of(path, k)));
                    }
                }
                return Ok(value.clone());
            }
            let empty = Map::new();
            let props = schema["properties"].as_object().unwrap_or(&empty);
            if let Some(k) = obj.keys().find(|k| !props.contains_key(*k)) {
                return Err(format!("unknown key {}", path_of(path, k)));
            }
            let mut out = Map::new();
            for (k, s) in props {
                match obj.get(k) {
                    Some(v) => {
                        out.insert(k.clone(), check(s, v, &path_of(path, k))?);
                    }
                    None => {
                        if let Some(d) = fill(s, None) {
                            out.insert(k.clone(), d);
                        }
                    }
                }
            }
            Ok(Value::Object(out))
        }
        "string" => {
            let s = value.as_str().ok_or_else(|| format!("{here} must be a string"))?;
            if let Some(allowed) = schema["enum"].as_array() {
                if !allowed.iter().any(|a| a == value) {
                    let list: Vec<&str> = allowed.iter().filter_map(Value::as_str).collect();
                    return Err(format!("{here} must be one of {}", list.join(", ")));
                }
            }
            let n = s.chars().count() as u64;
            if schema["minLength"].as_u64().is_some_and(|m| n < m) || schema["maxLength"].as_u64().is_some_and(|m| n > m) {
                return Err(format!(
                    "{here} must be {} to {} characters",
                    schema["minLength"].as_u64().unwrap_or(0),
                    schema["maxLength"].as_u64().unwrap_or(u64::MAX)
                ));
            }
            if schema["x-format"] == "language" && !is_language(s) {
                return Err(format!("{here} must be a language tag like en or pt-BR"));
            }
            if schema["x-format"] == "display_name" && (s.trim().is_empty() || s.chars().any(char::is_control)) {
                return Err(format!("{here} must be a printable name"));
            }
            Ok(value.clone())
        }
        "boolean" => value.is_boolean().then(|| value.clone()).ok_or_else(|| format!("{here} must be true or false")),
        t @ ("number" | "integer") => {
            let n = value.as_f64().filter(|n| n.is_finite()).ok_or_else(|| format!("{here} must be a number"))?;
            if t == "integer" && n.fract() != 0.0 {
                return Err(format!("{here} must be a whole number"));
            }
            let (min, max) = (schema["minimum"].as_f64(), schema["maximum"].as_f64());
            if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                return Err(format!("{here} must be between {} and {}", min.unwrap_or(f64::MIN), max.unwrap_or(f64::MAX)));
            }
            Ok(value.clone())
        }
        other => Err(format!("{here}: unsupported schema type {other}")),
    }
}

/// Validate a `PUT` body: the full document, every key present (defaults filled, `display_name` defaulting to
/// `current_name`), or the reason it is refused.
pub fn validate(doc: &Value, current_name: &str) -> Result<Value, String> {
    if let Some(v) = doc.get("version") {
        match v.as_u64() {
            Some(n) if n as u32 == VERSION => {}
            Some(n) if n as u32 > VERSION => {
                return Err(format!("version {n} is newer than this server's ({VERSION})"));
            }
            _ => return Err(format!("version must be {VERSION}")),
        }
    }
    let mut out = check(&schema(), doc, "")?;
    out["version"] = VERSION.into();
    if out.get("display_name").is_none() {
        out["display_name"] = current_name.into();
    }
    Ok(out)
}

/// A stored document brought to [`VERSION`]: unknown keys dropped, missing ones defaulted, invalid values reset.
/// (Version 1 is the first: this is where a later version's field moves go.)
pub fn upgrade(_from: u32, stored: &Value, current_name: &str) -> Value {
    let mut clean = Map::new();
    let schema = schema();
    let empty = Map::new();
    let props = schema["properties"].as_object().unwrap_or(&empty);
    for (k, s) in props {
        if let Some(v) = stored.get(k) {
            let v = match (v.as_object(), s["properties"].as_object()) {
                // One level down: keep the valid keys of a section.
                (Some(o), Some(sp)) => Value::Object(
                    o.iter()
                        .filter(|(kk, vv)| sp.get(*kk).is_some_and(|ss| check(ss, vv, kk).is_ok()))
                        .map(|(kk, vv)| (kk.clone(), vv.clone()))
                        .collect(),
                ),
                _ => v.clone(),
            };
            if check(s, &v, k).is_ok() {
                clean.insert(k.clone(), v);
            }
        }
    }
    clean.remove("version");
    clean.insert("display_name".into(), current_name.into());
    validate(&Value::Object(clean), current_name).unwrap_or_else(|_| defaults(current_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_complete() {
        let d = defaults("Chris");
        assert_eq!(d["display_name"], "Chris");
        assert_eq!(d["version"], 1);
        assert_eq!(d["audio"]["music"], 0.6);
        assert_eq!(d["accessibility"]["aura_palette"], "default");
        assert_eq!(d["keybinds"], json!({}));
        assert_eq!(validate(&d, "Chris").unwrap(), d, "the defaults round-trip");
        // Every top-level key of the schema is present.
        for k in schema()["properties"].as_object().unwrap().keys() {
            assert!(d.get(k).is_some(), "default for {k}");
        }
    }

    #[test]
    fn a_partial_put_takes_defaults_for_the_rest() {
        let v = validate(&json!({ "audio": { "music": 0.1 }, "pronouns": "they" }), "Chris").unwrap();
        assert_eq!(v["audio"]["music"], 0.1);
        assert_eq!(v["audio"]["master"], 0.8);
        assert_eq!(v["pronouns"], "they");
        assert_eq!(v["display_name"], "Chris");
        assert_eq!(v["hud"]["layout"], "default");
        assert_eq!(v["hero"], "", "no hero by default");
        assert_eq!(validate(&json!({ "hero": "benito" }), "C").unwrap()["hero"], "benito");
    }

    #[test]
    fn unknown_keys_are_refused_at_every_level() {
        assert_eq!(validate(&json!({ "theme": "dark" }), "C").unwrap_err(), "unknown key theme");
        assert_eq!(validate(&json!({ "hud": { "colour": "red" } }), "C").unwrap_err(), "unknown key hud.colour");
    }

    #[test]
    fn values_are_checked() {
        for (doc, want) in [
            (json!({ "audio": { "music": 1.5 } }), "audio.music must be between 0 and 1"),
            (json!({ "audio": { "music": "loud" } }), "audio.music must be a number"),
            (json!({ "audio": 3 }), "audio must be an object"),
            (json!({ "pronouns": "xe" }), "pronouns must be one of unspecified, she, he, they, it"),
            (json!({ "hero": "vex" }), "hero must be one of , chris, benito"),
            (json!({ "hero": "Chris" }), "hero must be one of , chris, benito"),
            (json!({ "hero": 1 }), "hero must be a string"),
            (json!({ "accessibility": { "reduce_flashing": "yes" } }), "accessibility.reduce_flashing must be true or false"),
            (json!({ "language": "English" }), "language must be a language tag like en or pt-BR"),
            (json!({ "keybinds": { "Fire!": "A" } }), "keybinds.Fire!: an action name is lower_snake (dots allowed), at most 40 characters"),
            (json!({ "keybinds": { "fire": "a b" } }), "keybinds.fire: a key chord is 1 to 40 of A-Z a-z 0-9 + _ -"),
            (json!({ "version": 2 }), "version 2 is newer than this server's (1)"),
            (json!({ "version": "1" }), "version must be 1"),
            (json!({ "display_name": "" }), "display_name must be 1 to 24 characters"),
            (json!([1]), "the document must be an object"),
        ] {
            assert_eq!(validate(&doc, "C").unwrap_err(), want, "{doc}");
        }
        let many: Map<String, Value> = (0..65).map(|i| (format!("a{i}"), json!("K"))).collect();
        assert_eq!(validate(&json!({ "keybinds": many }), "C").unwrap_err(), "keybinds holds at most 64 overrides");
        let ok = validate(&json!({ "language": "pt-BR", "keybinds": { "camera.pan_left": "Ctrl+Shift+A" } }), "C").unwrap();
        assert_eq!(ok["keybinds"]["camera.pan_left"], "Ctrl+Shift+A");
    }

    #[test]
    fn upgrade_keeps_what_is_valid() {
        let stored = json!({ "version": 1, "audio": { "music": 0.2, "gone": 1 }, "legacy": true, "pronouns": "xe", "display_name": "Old" });
        let v = upgrade(1, &stored, "New");
        assert_eq!(v["audio"]["music"], 0.2);
        assert_eq!(v["pronouns"], "unspecified");
        assert_eq!(v["display_name"], "New");
        assert!(v.get("legacy").is_none());
    }
}
