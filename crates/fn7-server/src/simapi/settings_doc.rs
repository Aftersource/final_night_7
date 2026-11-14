//! `GET /sim/settings`: every room setting with its default, range and description, generated from the source of
//! `fn7-plugin/src/settings.rs` (compiled in with `include_str!`), so the listing cannot drift from the validator:
//! descriptions are the `///` docs of the `Settings` fields, ranges are the literal bounds of the `int(..)` /
//! `num(..)` / `boolean(..)` calls in `Settings::validate`, defaults are `Settings::default()` and
//! `Settings::validate(null)`. The Theatre's timer settings (`copy_timer_settings`) are listed after them.
//! The tests (`tests/simapi.rs`) validate every listed bound and default through `Settings::validate`.

use fn7_plugin::settings::Settings;
use serde_json::{json, Map, Value};

/// The validator's source, as compiled.
pub const SETTINGS_RS: &str = include_str!("../../../fn7-plugin/src/settings.rs");

/// Settings the simulator request sets itself (top-level request fields), so they may not appear in `settings`.
pub const SET_BY_REQUEST: &[(&str, &str)] = &[
    ("seed", "use the request's `seeds` list (one match per seed)"),
    ("llm", "use the request's `llm` field (default false)"),
];

/// The Theatre core's timer settings (`theatre_plugin_api::util::copy_timer_settings`): (key, exclusive min, max).
pub const TIMER_SETTINGS: &[(&str, f64, f64, &str)] = &[
    ("compose_seconds", 0.0, 600.0, "Compose window per beat in seconds (defaults to flow_ms / 1000). Wall-clock only: no effect in a simulation."),
    ("choose_seconds", 0.0, 600.0, "Choose window in seconds (unused by Final Night 7). Wall-clock only: no effect in a simulation."),
    ("resolve_timeout_s", 0.0, 1800.0, "How long the core waits for a beat's resolve before the rules fallback. Wall-clock only: no effect in a simulation."),
];

/// Descriptions for `Settings` fields that have no `///` doc in settings.rs (used only while the doc is missing; the
/// test fails for a field with neither).
pub const FALLBACK_DOCS: &[(&str, &str)] = &[
    ("clusters", "Crowds (population clusters) the world is built with; fewer make the society step cheaper."),
    ("max_subagents_per_hand", "Most subagents one hand may run at once."),
];

struct Field {
    name: String,
    ty: String,
    doc: String,
}

/// `pub name: Type,` fields of `pub struct Settings { .. }` with their `///` docs.
fn struct_fields(src: &str) -> Vec<Field> {
    let Some(start) = src.find("pub struct Settings {") else {
        return vec![];
    };
    let body = &src[start..];
    let end = body.find("\n}").unwrap_or(body.len());
    let mut out = vec![];
    let mut doc: Vec<String> = vec![];
    for line in body[..end].lines().skip(1) {
        let t = line.trim();
        if let Some(d) = t.strip_prefix("///") {
            doc.push(d.trim().to_string());
        } else if let Some(rest) = t.strip_prefix("pub ") {
            if let Some((name, ty)) = rest.split_once(':') {
                out.push(Field {
                    name: name.trim().to_string(),
                    ty: ty.trim().trim_end_matches(',').trim().to_string(),
                    doc: doc.join(" "),
                });
            }
            doc.clear();
        } else if !t.starts_with("#[") {
            doc.clear();
        }
    }
    out
}

/// One validator call: `int("key", d.field, MIN, MAX)` / `num(..)` / `boolean("key", d.field)`.
#[derive(Debug, Clone)]
pub struct Bound {
    pub kind: &'static str,
    pub key: String,
    pub field: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

fn number(s: &str) -> Option<f64> {
    s.trim().replace('_', "").parse().ok()
}

/// Every validator call in `Settings::validate`, in source order.
pub fn validator_bounds(src: &str) -> Vec<Bound> {
    let body = src
        .find("pub fn validate(")
        .map(|i| &src[i..])
        .unwrap_or("");
    let body = &body[..body.find("pub fn from_value(").unwrap_or(body.len())];
    let mut out = vec![];
    for (kind, pat) in [("integer", "int(\""), ("number", "num(\""), ("boolean", "boolean(\"")] {
        let mut rest = body;
        while let Some(i) = rest.find(pat) {
            // Skip identifiers that merely end in the pattern (e.g. `print(`).
            let prev = rest[..i].chars().last();
            rest = &rest[i + pat.len()..];
            if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let Some(close) = rest.find(')') else { break };
            let args = format!("\"{}", &rest[..close]);
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            let key = parts[0].trim_matches('"').to_string();
            let field = parts
                .get(1)
                .and_then(|d| d.strip_prefix("d."))
                .unwrap_or(&key)
                .to_string();
            out.push(Bound {
                kind,
                key,
                field,
                min: parts.get(2).and_then(|x| number(x)),
                max: parts.get(3).and_then(|x| number(x)),
            });
        }
    }
    let pos = |b: &Bound| body.find(&format!("\"{}\"", b.key)).unwrap_or(usize::MAX);
    out.sort_by_key(pos);
    out
}

fn json_type(ty: &str) -> &'static str {
    match ty {
        "u32" | "u64" | "Option<u64>" | "Option<u32>" => "integer",
        "f32" | "f64" => "number",
        "bool" => "boolean",
        _ => "string",
    }
}

/// The listing (`{settings: [...], set_by_request: [...]}`); see the module doc.
pub fn listing() -> Value {
    let fields = struct_fields(SETTINGS_RS);
    let bounds = validator_bounds(SETTINGS_RS);
    let defaults = serde_json::to_value(Settings::default()).unwrap_or(Value::Null);
    let validated = Settings::validate(&Value::Null).unwrap_or(Value::Null);
    let mut out = vec![];
    for f in &fields {
        let own = bounds.iter().find(|b| b.key == f.name && b.field == f.name);
        // A key validated as another field's alias (`beat_ms` → flow_ms) is listed as that alias.
        let alias_of = bounds
            .iter()
            .find(|b| b.key == f.name && b.field != f.name)
            .map(|b| b.field.clone());
        let aliases: Vec<String> = bounds
            .iter()
            .filter(|b| b.field == f.name && b.key != f.name && !fields.iter().any(|g| g.name == b.key))
            .map(|b| b.key.clone())
            .collect();
        let mut e = Map::new();
        e.insert("key".into(), f.name.clone().into());
        e.insert(
            "type".into(),
            own.map(|b| b.kind).unwrap_or_else(|| json_type(&f.ty)).into(),
        );
        e.insert("rust_type".into(), f.ty.clone().into());
        let default = validated
            .get(&f.name)
            .cloned()
            .or_else(|| defaults.get(&f.name).cloned())
            .unwrap_or(Value::Null);
        e.insert("default".into(), default);
        let range = own.or_else(|| {
            alias_of
                .as_ref()
                .and_then(|a| bounds.iter().find(|b| b.key == f.name && &b.field == a))
        });
        if let Some(b) = range {
            if let (Some(lo), Some(hi)) = (b.min, b.max) {
                e.insert("min".into(), json!(lo));
                e.insert("max".into(), json!(hi));
            }
        }
        if f.ty == "ArenaKind" {
            e.insert("values".into(), json!(["planet", "city"]));
        }
        if !aliases.is_empty() {
            e.insert("aliases".into(), json!(aliases));
        }
        if let Some(a) = &alias_of {
            e.insert("alias_of".into(), a.clone().into());
        }
        let doc = if f.doc.is_empty() {
            FALLBACK_DOCS.iter().find(|(k, _)| *k == f.name).map(|x| x.1.to_string()).unwrap_or_default()
        } else {
            f.doc.clone()
        };
        e.insert("description".into(), doc.into());
        if let Some((_, note)) = SET_BY_REQUEST.iter().find(|(k, _)| *k == f.name) {
            e.insert("sim".into(), format!("not allowed in `settings`: {note}").into());
        }
        if matches!(f.name.as_str(), "flow_ms" | "beat_ms") {
            e.insert(
                "sim".into(),
                "wall-clock beat length; a simulation runs beats back to back".into(),
            );
        }
        e.insert("source".into(), "fn7".into());
        out.push(Value::Object(e));
    }
    for (key, lo, hi, doc) in TIMER_SETTINGS {
        out.push(json!({
            "key": key, "type": "number", "default": validated.get(*key).cloned().unwrap_or(Value::Null),
            "min_exclusive": lo, "max": hi, "description": doc, "source": "theatre",
        }));
    }
    json!({
        "settings": out,
        "set_by_request": SET_BY_REQUEST.iter().map(|(k, n)| json!({ "key": k, "note": n })).collect::<Vec<_>>(),
        "generated_from": "crates/fn7-plugin/src/settings.rs (Settings fields + Settings::validate bounds)",
    })
}

/// Every key `settings` may carry (field names, validator aliases, timer keys).
pub fn known_keys() -> Vec<String> {
    let mut k: Vec<String> = struct_fields(SETTINGS_RS).into_iter().map(|f| f.name).collect();
    for b in validator_bounds(SETTINGS_RS) {
        if !k.contains(&b.key) {
            k.push(b.key);
        }
    }
    k.extend(TIMER_SETTINGS.iter().map(|t| t.0.to_string()));
    k
}
