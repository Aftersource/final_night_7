//! The style config (`data/media_style.json`): the art director's lever (see docs/SHARED_MEDIA_SPEC.md §4).
//!
//! ```json
//! { "version": "1",
//!   "house_style": "collectible-figure game illustration, …",
//!   "kinds": { "portrait": {"framing": "waist-up, three-quarter view, dramatic rim light", "size": "1024x1024"}, … },
//!   "avoid": ["text", "logos", …],
//!   "event_selection": { "min_magnitude": 8, "always": ["cast", "elimination", …], "max_per_match": 12 } }
//! ```
//!
//! - Every prompt is `<framing for its kind>: <subject>. Style: <house_style>. Avoid: <avoid, …>.` and is generated at
//!   its kind's `size`.
//! - `event_selection` decides which public events (moment cards) get an image: magnitude ≥ `min_magnitude`, or a
//!   `kind` listed in `always` (`elimination` is accepted for the ledger verb `eliminate`); at most `max_per_match`
//!   event images per match (`FN7_IMAGE_MAX_EVENTS` overrides it).
//! - Loaded from `FN7_MEDIA_STYLE` (a path), else `data/media_style.json` in the working directory, else the copy
//!   compiled into the binary. Validated at startup: a bad file stops the server with a message naming the field.
//! - Its `version` and a content hash (sha256 of the parsed JSON, so whitespace edits don't change it) are recorded in
//!   every match's ruleset. The prompt text (framing, house style, avoid list) and the size are part of every image
//!   key, so a new look never reuses an old picture, while retuning `event_selection` alone keeps every image.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::prompt::ImageKind;

/// The compiled-in style (the repo's `data/media_style.json` at build time).
pub const BUILTIN_STYLE_JSON: &str = include_str!("../../../data/media_style.json");

/// The diorama's framing (also in `data/media_style.json`).
const DIORAMA_FRAMING: &str = "the demigod as a collectible minifig standing on a small square themed diorama base that \
reflects its home region and shows off its signature prop, three-quarter view from slightly above, centered, square";

/// Fallback framing and size for a kind the style file doesn't list.
fn builtin_kind(kind: ImageKind) -> KindStyle {
    let (framing, size) = match kind {
        ImageKind::Portrait => ("waist-up, three-quarter view, dramatic rim light", "1024x1024"),
        ImageKind::Item => (
            "a single item shown alone like a loot icon, centered, three-quarter view, plain background",
            "1024x1024",
        ),
        ImageKind::Event => ("wide cinematic, seen from high above", "1536x1024"),
        ImageKind::Statue => ("amiibo-style figure on a round base atop a pile of its items", "1024x1024"),
        ImageKind::Place => ("establishing shot", "1536x1024"),
        ImageKind::Army => ("wide cinematic, a marching host seen from a low angle, banners raised", "1536x1024"),
        ImageKind::Diorama => (DIORAMA_FRAMING, "1024x1024"),
    };
    KindStyle {
        framing: framing.into(),
        size: size.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindStyle {
    pub framing: String,
    pub size: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSelection {
    /// Events of at least this magnitude (0..10) get an image.
    pub min_magnitude: u8,
    /// Event kinds that always get an image (the `kind` of `events[]`; `elimination` = `eliminate`).
    #[serde(default)]
    pub always: Vec<String>,
    /// Event images per match.
    pub max_per_match: u32,
}

impl EventSelection {
    /// Does an event of this kind and magnitude get an image?
    pub fn selects(&self, kind: &str, magnitude: u8) -> bool {
        let k = canonical_kind(kind);
        magnitude >= self.min_magnitude || self.always.iter().any(|a| canonical_kind(a) == k)
    }
}

/// Event kinds as the ledger spells them (`elimination` → `eliminate`).
fn canonical_kind(k: &str) -> String {
    let k = k.trim().to_ascii_lowercase();
    match k.as_str() {
        "elimination" | "eliminated" | "eliminations" => "eliminate".into(),
        "casts" => "cast".into(),
        _ => k,
    }
}

/// A validated style config.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaStyle {
    pub version: String,
    pub house_style: String,
    pub kinds: BTreeMap<ImageKind, KindStyle>,
    pub avoid: Vec<String>,
    pub event_selection: EventSelection,
    /// sha256 (hex) of the parsed JSON.
    pub hash: String,
    /// Where it came from: a path, or `builtin`.
    pub source: String,
    /// The JSON as loaded (recorded in the ruleset).
    pub body: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    version: Value,
    house_style: String,
    kinds: BTreeMap<String, KindStyle>,
    #[serde(default)]
    avoid: Vec<String>,
    event_selection: EventSelection,
}

impl Default for MediaStyle {
    fn default() -> Self {
        Self::builtin()
    }
}

impl MediaStyle {
    /// The compiled-in style. Panics only if the repo's own file is invalid (a test guards it).
    pub fn builtin() -> Self {
        static S: std::sync::OnceLock<MediaStyle> = std::sync::OnceLock::new();
        S.get_or_init(|| {
            Self::parse(BUILTIN_STYLE_JSON, "builtin").expect("data/media_style.json (compiled in) is invalid")
        })
        .clone()
    }

    /// Parse and validate a style file's text. Errors name the source and the field.
    pub fn parse(text: &str, source: &str) -> Result<Self, String> {
        let body: Value = serde_json::from_str(text).map_err(|e| format!("{source}: not valid JSON: {e}"))?;
        Self::from_value(body, source)
    }

    pub fn from_value(mut body: Value, source: &str) -> Result<Self, String> {
        let err = |m: String| format!("{source}: {m}");
        // Keys starting with "_" are comments.
        if let Some(o) = body.as_object_mut() {
            o.retain(|k, _| !k.starts_with('_'));
        } else {
            return Err(err("the style must be a JSON object".into()));
        }
        let raw: Raw = serde_json::from_value(body.clone()).map_err(|e| err(e.to_string()))?;
        let version = match &raw.version {
            Value::String(s) if !s.trim().is_empty() => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            _ => return Err(err("`version` must be a non-empty string".into())),
        };
        if raw.house_style.trim().is_empty() {
            return Err(err("`house_style` must not be empty".into()));
        }
        let mut kinds = BTreeMap::new();
        for (name, ks) in raw.kinds {
            let kind = ImageKind::parse(&name).ok_or_else(|| {
                err(format!(
                    "kinds.{name}: unknown kind (use {})",
                    ImageKind::ALL.map(|k| k.as_str()).join(", ")
                ))
            })?;
            if ks.framing.trim().is_empty() {
                return Err(err(format!("kinds.{name}.framing must not be empty")));
            }
            if !valid_size(&ks.size) {
                return Err(err(format!(
                    "kinds.{name}.size {:?} is not WIDTHxHEIGHT (e.g. \"1024x1024\", each side 64..4096)",
                    ks.size
                )));
            }
            kinds.insert(
                kind,
                KindStyle {
                    framing: ks.framing.trim().to_string(),
                    size: ks.size.trim().to_string(),
                },
            );
        }
        if let Some(i) = raw.avoid.iter().position(|a| a.trim().is_empty()) {
            return Err(err(format!("avoid[{i}] is empty")));
        }
        let sel = raw.event_selection;
        if sel.min_magnitude > 10 {
            return Err(err(format!(
                "event_selection.min_magnitude {} is past the 0..10 scale",
                sel.min_magnitude
            )));
        }
        if let Some(i) = sel.always.iter().position(|a| a.trim().is_empty()) {
            return Err(err(format!("event_selection.always[{i}] is empty")));
        }
        if sel.max_per_match > 1000 {
            return Err(err("event_selection.max_per_match is more than 1000".into()));
        }
        let hash = hex::encode(Sha256::digest(
            serde_json::to_string(&canonical(&body)).unwrap_or_default().as_bytes(),
        ));
        Ok(Self {
            version,
            house_style: raw.house_style.trim().to_string(),
            kinds,
            avoid: raw.avoid.iter().map(|a| a.trim().to_string()).collect(),
            event_selection: sel,
            hash,
            source: source.into(),
            body,
        })
    }

    /// Read and validate a style file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text, &path.display().to_string())
    }

    /// The style for this process: `FN7_MEDIA_STYLE` (must exist and validate), else `data/media_style.json` if it
    /// exists (must validate), else the compiled-in copy.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        if let Some(p) = get("FN7_MEDIA_STYLE").map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            return Self::load(&PathBuf::from(p));
        }
        let default = PathBuf::from("data/media_style.json");
        if default.is_file() {
            return Self::load(&default);
        }
        Ok(Self::builtin())
    }

    /// Framing and size for a kind (the file's, else the built-in fallback).
    pub fn kind(&self, kind: ImageKind) -> KindStyle {
        self.kinds.get(&kind).cloned().unwrap_or_else(|| builtin_kind(kind))
    }

    /// The text after the subject: `Style: … Avoid: ….`
    pub fn suffix(&self) -> String {
        let mut s = format!("Style: {}.", self.house_style.trim_end_matches('.'));
        if !self.avoid.is_empty() {
            s.push_str(&format!(" Avoid: {}.", self.avoid.join(", ")));
        }
        s
    }

    /// What the ruleset records: `{version, hash, source, body}`.
    pub fn ruleset_json(&self) -> Value {
        json!({ "version": self.version, "hash": self.hash, "source": self.source, "body": self.body })
    }
}

/// `WIDTHxHEIGHT`, each side 64..=4096.
fn valid_size(s: &str) -> bool {
    let Some((w, h)) = s.trim().split_once('x') else {
        return false;
    };
    let side = |x: &str| x.parse::<u32>().is_ok_and(|n| (64..=4096).contains(&n));
    side(w) && side(h)
}

/// The value with object keys sorted (whatever map order serde_json was built with).
fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let sorted: BTreeMap<&String, Value> = o.iter().map(|(k, x)| (k, canonical(x))).collect();
            let mut m = serde_json::Map::new();
            for (k, x) in sorted {
                m.insert(k.clone(), x);
            }
            Value::Object(m)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        x => x.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repo_style_is_valid_and_matches_the_spec() {
        let s = MediaStyle::builtin();
        assert_eq!(s.version, "1");
        assert_eq!(s.kind(ImageKind::Event).size, "1536x1024");
        assert_eq!(s.event_selection.min_magnitude, 8);
        assert_eq!(s.event_selection.max_per_match, 12);
        assert!(s.event_selection.selects("eliminate", 1), "elimination = the ledger's eliminate");
        assert!(s.event_selection.selects("cast", 0));
        assert!(s.event_selection.selects("riot", 8));
        assert!(!s.event_selection.selects("riot", 7));
        assert_eq!(s.hash.len(), 64);
    }

    #[test]
    fn whitespace_does_not_change_the_hash_but_content_does() {
        let a = MediaStyle::parse(BUILTIN_STYLE_JSON, "a").unwrap();
        let v: Value = serde_json::from_str(BUILTIN_STYLE_JSON).unwrap();
        let b = MediaStyle::parse(&serde_json::to_string(&v).unwrap(), "b").unwrap();
        assert_eq!(a.hash, b.hash);
        let mut v2 = v.clone();
        v2["house_style"] = json!("watercolor storybook");
        let c = MediaStyle::from_value(v2, "c").unwrap();
        assert_ne!(a.hash, c.hash);
    }

    #[test]
    fn bad_files_are_refused_with_the_field_named() {
        let base: Value = serde_json::from_str(BUILTIN_STYLE_JSON).unwrap();
        let bad = |f: &dyn Fn(&mut Value)| {
            let mut v = base.clone();
            f(&mut v);
            MediaStyle::from_value(v, "media_style.json").unwrap_err()
        };
        assert!(bad(&|v| v["kinds"]["portrait"]["size"] = json!("big")).contains("kinds.portrait.size"));
        assert!(bad(&|v| v["kinds"]["selfie"] = json!({"framing": "x", "size": "512x512"})).contains("kinds.selfie"));
        assert!(bad(&|v| v["event_selection"]["min_magnitude"] = json!(11)).contains("min_magnitude"));
        assert!(bad(&|v| v["house_style"] = json!(" ")).contains("house_style"));
        assert!(bad(&|v| v["colour"] = json!("red")).contains("colour"), "typos are caught");
        assert!(bad(&|v| v["version"] = json!("")).contains("version"));
        assert!(MediaStyle::parse("{", "x.json").unwrap_err().starts_with("x.json: not valid JSON"));
        // Comments are allowed.
        let mut v = base.clone();
        v["_note"] = json!("Benito: try a darker palette next week");
        assert!(MediaStyle::from_value(v, "ok").is_ok());
    }
}
