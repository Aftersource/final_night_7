//! The style config (`data/media_style.json`): the art director's lever (see docs/SHARED_MEDIA_SPEC.md §4 and
//! docs/MEDIA.md).
//!
//! ```json
//! { "version": "2",
//!   "house_style": "Final Night 7's tabletop world: …",
//!   "house_style_original": "Benito's verbatim DEFAULT_STYLE (documentation; never sent)",
//!   "kinds": { "portrait": {"framing": "waist-up portrait, …", "size": "1024x1024"},
//!              "event": {"framing": "seen from high above the hex-tile table: {subject}, …", "size": "1536x1024"}, … },
//!   "avoid": "No words or letters in the picture, no gore, no real people.",
//!   "subject_rewrites": [ {"from": "a collectible minifig", "to": "a tiny geometric miniature figure"} ],
//!   "references": { "geometry": [], "mood": ["style_refs/IMG_3751.jpg", …], "mood_per_request": 4, "note": "…" },
//!   (and per kind, e.g. "kinds": {"portrait": {…, "style": "…", "references": {…}}})
//!   "event_selection": { "min_magnitude": 8, "always": ["cast", "elimination", …], "max_per_match": 12 } }
//! ```
//!
//! - Every prompt is `<Framing>: <subject>. Style: <house_style>. <references.note> <avoid>` (a framing containing
//!   `{subject}` puts the subject there instead of after a colon), generated at its kind's `size`. `avoid` is either a
//!   sentence (sent as written) or a list (rendered `Avoid: a, b.`).
//! - Per kind, `kinds.<k>.style` and `kinds.<k>.references` replace the top-level `house_style` and `references`
//!   (style v2: portraits in Benito's dark-fantasy look, everything else in the max-zoom hex look).
//! - `references` (optional): reference images sent with every request (`/images/edits` on gpt-image models). Each
//!   request gets every `geometry` image first, in order (a kind may list its own `geometry` instead), then
//!   `mood_per_request` images from `mood`, picked deterministically from the prompt (the same prompt always gets the
//!   same refs; different prompts spread over the set). Paths are relative to the style file's directory. Every file
//!   must exist when the style loads (a missing one stops the server, naming the field and the path); their sha256s
//!   are part of every image key, so a new reference set never reuses an old picture.
//! - `event_selection` decides which public events (moment cards) get an image: magnitude ≥ `min_magnitude`, or a
//!   `kind` listed in `always` (`elimination` is accepted for the ledger verb `eliminate`); at most `max_per_match`
//!   event images per match (`FN7_IMAGE_MAX_EVENTS` overrides it).
//! - Loaded from `FN7_MEDIA_STYLE` (a path), else `data/media_style.json` in the working directory, else the copy
//!   compiled into the binary (its references resolve against the repo's `data/` at build time). Validated at
//!   startup: a bad file stops the server with a message naming the field.
//! - Its `version` and a content hash (sha256 of the parsed JSON, so whitespace edits don't change it) are recorded in
//!   every match's ruleset. The prompt text and the size are part of every image key, so a new look never reuses an
//!   old picture, while retuning `event_selection` alone keeps every image.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::prompt::ImageKind;

/// The compiled-in style (the repo's `data/media_style.json` at build time).
pub const BUILTIN_STYLE_JSON: &str = include_str!("../../../data/media_style.json");

/// Where the compiled-in style's relative reference paths resolve (the repo's `data/` at build time).
pub const BUILTIN_STYLE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data");

/// The diorama's framing (also in `data/media_style.json`).
const DIORAMA_FRAMING: &str = "the demigod as a tiny, extremely simple geometric miniature figure (a few faceted \
low-poly blocks, a near-faceless head, glowing hand shapes) standing on a single raised hexagonal diorama tile \
with its signature prop, three-quarter view from slightly above, centered on the dark \
candlelit table";

/// Fallback framing and size for a kind the style file doesn't list.
fn builtin_kind(kind: ImageKind) -> KindStyle {
    let (framing, size) = match kind {
        ImageKind::Portrait => ("waist-up, three-quarter view, dramatic rim light", "1024x1024"),
        ImageKind::Item => (
            "a single item shown alone as a tiny faceted low-poly game piece standing on one raised hexagonal tile, \
             centered, three-quarter view, on the dark candlelit table",
            "1024x1024",
        ),
        ImageKind::Event => (
            "seen from high above the hex-tile table: {subject}, rendered with blocky geometry, tiny white pillar \
             figures for crowds, voxel props, candlelight and starry violet haze",
            "1536x1024",
        ),
        ImageKind::Statue => (
            "a slightly larger faceted low-poly figure of the demigod on a raised hexagonal plinth atop a heap of \
             blocky voxel item pieces, lit by candles and torches on the dark table",
            "1024x1024",
        ),
        ImageKind::Place => (
            "an establishing shot across the hex-tile table: {subject}, built as a cluster of raised hexagonal tiles \
             with chunky voxel buildings and terrain, candlelight and starry violet haze above",
            "1536x1024",
        ),
        ImageKind::Army => (
            "a low angle across the hex-tile table: {subject}, as ranks of small white pillar figures with flat \
             banner blocks in their colours marching over raised hex tiles, candlelight and violet haze",
            "1536x1024",
        ),
        ImageKind::Diorama => (DIORAMA_FRAMING, "1024x1024"),
        ImageKind::Sprite => (
            "one full-body game sprite of a single character standing in a relaxed three-quarter view, the whole \
             figure from head to feet in frame and centered, a clear silhouette readable at a small size, on a plain \
             flat background with no scenery and no ground: {subject}",
            "1024x1024",
        ),
        ImageKind::Building => (
            "a single building shown alone as a chunky low-poly miniature game piece on one raised hexagonal tile, \
             three-quarter view from slightly above, centered, on a plain flat background with no scenery: {subject}",
            "1024x1024",
        ),
    };
    KindStyle {
        framing: framing.into(),
        size: size.into(),
        style: None,
        references: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindStyle {
    pub framing: String,
    pub size: String,
    /// This kind's own house style (replacing the top-level `house_style` for it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// This kind's own reference set and note (replacing the top-level `references` for it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<RefsSpec>,
}

/// A `references` section as written: `{geometry: [paths], mood: [paths], mood_per_request, note}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefsSpec {
    /// Sent first, in order, with every request (the Unreal geometry screenshots).
    #[serde(default)]
    pub geometry: Vec<String>,
    /// The pool the per-request mood pick draws from (Benito's images).
    #[serde(default)]
    pub mood: Vec<String>,
    /// Mood images per request (default: all of them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood_per_request: Option<usize>,
    /// Appended to every prompt that uses this set.
    pub note: String,
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

/// One reference image: as named in the style file, where it is, and its content hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RefImage {
    /// The path as written in the style file (`style_refs/IMG_3751.jpg`).
    pub name: String,
    /// The resolved path.
    pub path: PathBuf,
    /// sha256 (hex) of the file's bytes.
    pub sha256: String,
}

impl RefImage {
    /// The file name alone (what the request record lists).
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.name.clone())
    }
}

/// A validated `references` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct References {
    /// Sent first, in order, with every request.
    pub geometry: Vec<RefImage>,
    /// The pool the per-request mood pick draws from.
    pub mood: Vec<RefImage>,
    pub mood_per_request: usize,
    /// Appended to every prompt that has references.
    pub note: String,
}

impl References {
    /// The references for one prompt: the geometry refs in order, then `mood_per_request` mood refs chosen
    /// deterministically by `seed` (each mood ref is ranked by sha256(seed, its sha256); the lowest win; they keep the
    /// file's order).
    pub fn pick(&self, seed: &[u8]) -> Vec<RefImage> {
        let mut out = self.geometry.clone();
        let mut ranked: Vec<(Vec<u8>, usize)> = self
            .mood
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut h = Sha256::new();
                h.update(seed);
                h.update(b"\n");
                h.update(r.sha256.as_bytes());
                (h.finalize().to_vec(), i)
            })
            .collect();
        ranked.sort();
        let mut chosen: Vec<usize> = ranked
            .into_iter()
            .take(self.mood_per_request)
            .map(|(_, i)| i)
            .collect();
        chosen.sort_unstable();
        out.extend(chosen.into_iter().map(|i| self.mood[i].clone()));
        out
    }
}

/// A subject rewrite (`from` → `to`, case-insensitive).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rewrite {
    pub from: String,
    pub to: String,
}

/// A validated style config.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaStyle {
    pub version: String,
    pub house_style: String,
    /// Benito's verbatim original style text, kept in the file for him to choose (never sent to the model).
    pub house_style_original: Option<String>,
    pub kinds: BTreeMap<ImageKind, KindStyle>,
    /// The rendered avoid section (`No words or letters …` as written, or `Avoid: a, b.` from a list); may be empty.
    pub avoid: String,
    /// Applied to every subject, in order, before it is framed.
    pub subject_rewrites: Vec<Rewrite>,
    /// The default reference set (kinds may have their own: [`MediaStyle::references_for`]).
    pub references: Option<References>,
    /// Per-kind house styles (`kinds.<k>.style`).
    pub kind_styles: BTreeMap<ImageKind, String>,
    /// Per-kind reference sets (`kinds.<k>.references`).
    pub kind_references: BTreeMap<ImageKind, References>,
    pub event_selection: EventSelection,
    /// Appended to a prompt drawn on a base image (gear portraits and dioramas; `base_note`, default
    /// [`DEFAULT_BASE_NOTE`]).
    pub base_note: String,
    /// Said of the base image in a gear PORTRAIT, before the style text (`portrait_base_note`, default
    /// [`DEFAULT_PORTRAIT_BASE_NOTE`]): unlike [`MediaStyle::base_note`] it lets the hands change, since the new gear
    /// replaces what the base portrait's hands held.
    pub portrait_base_note: String,
    /// sha256 (hex) of the parsed JSON.
    pub hash: String,
    /// Where it came from: a path, or `builtin`.
    pub source: String,
    /// The JSON as loaded (recorded in the ruleset).
    pub body: Value,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawAvoid {
    Sentence(String),
    List(Vec<String>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    version: Value,
    house_style: String,
    #[serde(default)]
    house_style_original: Option<String>,
    kinds: BTreeMap<String, KindStyle>,
    #[serde(default)]
    avoid: Option<RawAvoid>,
    #[serde(default)]
    subject_rewrites: Vec<Rewrite>,
    #[serde(default)]
    references: Option<RefsSpec>,
    event_selection: EventSelection,
    #[serde(default)]
    base_note: Option<String>,
    #[serde(default)]
    portrait_base_note: Option<String>,
}

/// What a prompt drawn on a base image says about it when the style file has no `base_note`.
pub const DEFAULT_BASE_NOTE: &str = "The first reference image is this same character: keep its face, build, clothes, colours and pose family exactly as they are there and add only the gear described; the other reference images set the art style.";

/// What a gear portrait says about its base image when the style file has no `portrait_base_note`.
pub const DEFAULT_PORTRAIT_BASE_NOTE: &str = "The first reference image is this same character before the new gear: keep its face, build, clothes and colours exactly as they are there, but not what its hands hold or its pose: the hands now hold the new gear described, which must be clearly visible and in focus; the other reference images set the art style.";

/// The most reference images one request may carry (OpenAI's edits endpoint takes up to 16).
pub const MAX_REFS_PER_REQUEST: usize = 16;

impl Default for MediaStyle {
    fn default() -> Self {
        Self::builtin()
    }
}

impl MediaStyle {
    /// The compiled-in style, its references resolved against the repo's `data/` at build time. If those files are
    /// gone (a binary moved away from its checkout), this is the same style without references; the startup lookup
    /// ([`MediaStyle::from_lookup`]) reports the missing files instead of falling back silently. Panics only if the
    /// repo's own file is invalid (a test guards it).
    pub fn builtin() -> Self {
        match Self::builtin_checked() {
            Ok(s) => s,
            Err(_) => {
                static S: std::sync::OnceLock<MediaStyle> = std::sync::OnceLock::new();
                S.get_or_init(|| {
                    let mut v: Value = serde_json::from_str(BUILTIN_STYLE_JSON).expect("data/media_style.json");
                    if let Some(o) = v.as_object_mut() {
                        o.remove("references");
                    }
                    if let Some(kinds) = v.get_mut("kinds").and_then(Value::as_object_mut) {
                        for k in kinds.values_mut().filter_map(Value::as_object_mut) {
                            k.remove("references");
                        }
                    }
                    Self::from_value_in(v, "builtin", Path::new(BUILTIN_STYLE_DIR))
                        .expect("data/media_style.json (compiled in) is invalid")
                })
                .clone()
            }
        }
    }

    /// The compiled-in style with its references checked.
    pub fn builtin_checked() -> Result<Self, String> {
        static S: std::sync::OnceLock<Result<MediaStyle, String>> = std::sync::OnceLock::new();
        S.get_or_init(|| Self::parse_in(BUILTIN_STYLE_JSON, "builtin", Path::new(BUILTIN_STYLE_DIR)))
            .clone()
    }

    /// Parse and validate a style file's text, references relative to the compiled-in style's directory.
    pub fn parse(text: &str, source: &str) -> Result<Self, String> {
        Self::parse_in(text, source, Path::new(BUILTIN_STYLE_DIR))
    }

    /// Parse and validate a style file's text, references relative to `base`. Errors name the source and the field.
    pub fn parse_in(text: &str, source: &str, base: &Path) -> Result<Self, String> {
        let body: Value = serde_json::from_str(text).map_err(|e| format!("{source}: not valid JSON: {e}"))?;
        Self::from_value_in(body, source, base)
    }

    /// [`MediaStyle::from_value_in`] with references relative to the compiled-in style's directory.
    pub fn from_value(body: Value, source: &str) -> Result<Self, String> {
        Self::from_value_in(body, source, Path::new(BUILTIN_STYLE_DIR))
    }

    pub fn from_value_in(mut body: Value, source: &str, base: &Path) -> Result<Self, String> {
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
        let resolve = |field: String, name: &str| -> Result<RefImage, String> {
            if name.trim().is_empty() {
                return Err(err(format!("{field} is empty")));
            }
            let p = Path::new(name.trim());
            let path = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
            let bytes = std::fs::read(&path).map_err(|e| {
                err(format!("{field} {name:?}: cannot read reference image {}: {e}", path.display()))
            })?;
            if bytes.is_empty() {
                return Err(err(format!("{field} {name:?}: {} is empty", path.display())));
            }
            Ok(RefImage {
                name: name.trim().to_string(),
                path,
                sha256: hex::encode(Sha256::digest(&bytes)),
            })
        };
        let refs_of = |field: &str, r: &RefsSpec| -> Result<References, String> {
            if r.note.trim().is_empty() {
                return Err(err(format!("{field}.note must not be empty")));
            }
            let list = |which: &str, v: &[String]| {
                v.iter()
                    .enumerate()
                    .map(|(i, n)| resolve(format!("{field}.{which}[{i}]"), n))
                    .collect::<Result<Vec<_>, _>>()
            };
            let geometry = list("geometry", &r.geometry)?;
            let mood = list("mood", &r.mood)?;
            let per = r.mood_per_request.unwrap_or(mood.len()).min(mood.len());
            if geometry.is_empty() && mood.is_empty() {
                return Err(err(format!("{field} lists no images (geometry and mood are both empty)")));
            }
            if geometry.len() + per > MAX_REFS_PER_REQUEST {
                return Err(err(format!(
                    "{field}: {} images per request; the most is {MAX_REFS_PER_REQUEST}",
                    geometry.len() + per
                )));
            }
            Ok(References {
                geometry,
                mood,
                mood_per_request: per,
                note: r.note.trim().to_string(),
            })
        };
        let mut kinds = BTreeMap::new();
        let mut kind_styles = BTreeMap::new();
        let mut kind_references = BTreeMap::new();
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
            if let Some(st) = &ks.style {
                if st.trim().is_empty() {
                    return Err(err(format!("kinds.{name}.style must not be empty (leave it out for the default)")));
                }
                kind_styles.insert(kind, st.trim().to_string());
            }
            if let Some(r) = &ks.references {
                kind_references.insert(kind, refs_of(&format!("kinds.{name}.references"), r)?);
            }
            kinds.insert(
                kind,
                KindStyle {
                    framing: ks.framing.trim().to_string(),
                    size: ks.size.trim().to_string(),
                    style: ks.style.clone(),
                    references: ks.references.clone(),
                },
            );
        }
        let avoid = match raw.avoid {
            None => String::new(),
            Some(RawAvoid::Sentence(s)) => s.trim().to_string(),
            Some(RawAvoid::List(l)) => {
                if let Some(i) = l.iter().position(|a| a.trim().is_empty()) {
                    return Err(err(format!("avoid[{i}] is empty")));
                }
                if l.is_empty() {
                    String::new()
                } else {
                    format!("Avoid: {}.", l.iter().map(|a| a.trim()).collect::<Vec<_>>().join(", "))
                }
            }
        };
        for (i, r) in raw.subject_rewrites.iter().enumerate() {
            if r.from.trim().is_empty() {
                return Err(err(format!("subject_rewrites[{i}].from is empty")));
            }
        }
        let references = match &raw.references {
            None => None,
            Some(r) => Some(refs_of("references", r)?),
        };
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
            house_style_original: raw.house_style_original.map(|s| s.trim().to_string()),
            kinds,
            avoid,
            subject_rewrites: raw.subject_rewrites,
            references,
            kind_styles,
            kind_references,
            event_selection: sel,
            base_note: match raw.base_note.map(|s| s.trim().to_string()) {
                Some(s) if s.is_empty() => return Err(err("base_note is empty".into())),
                Some(s) => s,
                None => DEFAULT_BASE_NOTE.to_string(),
            },
            portrait_base_note: match raw.portrait_base_note.map(|s| s.trim().to_string()) {
                Some(s) if s.is_empty() => return Err(err("portrait_base_note is empty".into())),
                Some(s) => s,
                None => DEFAULT_PORTRAIT_BASE_NOTE.to_string(),
            },
            hash,
            source: source.into(),
            body,
        })
    }

    /// Read and validate a style file (references relative to its directory).
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        Self::parse_in(&text, &path.display().to_string(), base)
    }

    /// The style for this process: `FN7_MEDIA_STYLE` (must exist and validate), else `data/media_style.json` if it
    /// exists (must validate), else the compiled-in copy (whose reference files must exist).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        if let Some(p) = get("FN7_MEDIA_STYLE").map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            return Self::load(&PathBuf::from(p));
        }
        let default = PathBuf::from("data/media_style.json");
        if default.is_file() {
            return Self::load(&default);
        }
        Self::builtin_checked()
    }

    /// Framing and size for a kind (the file's, else the built-in fallback).
    pub fn kind(&self, kind: ImageKind) -> KindStyle {
        self.kinds.get(&kind).cloned().unwrap_or_else(|| builtin_kind(kind))
    }

    /// The subject after `subject_rewrites` (case-insensitive, in order).
    pub fn rewrite_subject(&self, subject: &str) -> String {
        let mut s = subject.to_string();
        for r in &self.subject_rewrites {
            s = replace_ci(&s, r.from.trim(), r.to.trim());
        }
        s
    }

    /// The house style for a kind (`kinds.<k>.style`, else `house_style`).
    pub fn house_style_for(&self, kind: ImageKind) -> &str {
        self.kind_styles.get(&kind).map(String::as_str).unwrap_or(&self.house_style)
    }

    /// The reference set for a kind (`kinds.<k>.references`, else `references`), if any.
    pub fn references_for(&self, kind: ImageKind) -> Option<&References> {
        self.kind_references.get(&kind).or(self.references.as_ref())
    }

    /// The text after the subject for a kind: `Style: …. [<references note>] [<avoid>]`. The note is there exactly
    /// when the kind has references (then every prompt of that kind carries them).
    pub fn suffix_for(&self, kind: ImageKind) -> String {
        let mut s = format!("Style: {}.", self.house_style_for(kind).trim_end_matches('.'));
        if let Some(r) = self.references_for(kind) {
            s.push(' ');
            s.push_str(&r.note);
            if !r.note.ends_with('.') {
                s.push('.');
            }
        }
        if !self.avoid.is_empty() {
            s.push(' ');
            s.push_str(&self.avoid);
        }
        s
    }

    /// What the ruleset records: `{version, hash, source, body}`.
    pub fn ruleset_json(&self) -> Value {
        json!({ "version": self.version, "hash": self.hash, "source": self.source, "body": self.body })
    }
}

/// `s` with every case-insensitive (ASCII) occurrence of `from` replaced by `to`.
fn replace_ci(s: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return s.to_string();
    }
    let lower = s.to_ascii_lowercase();
    let needle = from.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while let Some(i) = lower[at..].find(&needle) {
        out.push_str(&s[at..at + i]);
        out.push_str(to);
        at += i + needle.len();
    }
    out.push_str(&s[at..]);
    out
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
        let s = MediaStyle::builtin_checked().expect("the repo's reference images exist");
        assert_eq!(s.version, "2");
        assert_eq!(s.references_for(ImageKind::Portrait).unwrap().mood.len(), 6);
        assert_eq!(s.references_for(ImageKind::Diorama).unwrap().mood_per_request, 4);
        assert!(s.house_style_for(ImageKind::Portrait).starts_with("a dark-fantasy world at night under"));
        assert!(s.house_style_for(ImageKind::Event).starts_with("Final Night 7's tabletop world"));
        assert!(s.house_style_original.as_deref().unwrap().contains("burning stone keep"), "Benito's original is kept");
        assert!(s.references.as_ref().unwrap().geometry.is_empty(), "no Unreal screenshots yet");
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
        // Reference images must exist; the message names the field and the path.
        let e = bad(&|v| v["references"]["mood"][1] = json!("style_refs/IMG_0000.jpg"));
        assert!(e.contains("references.mood[1]") && e.contains("IMG_0000.jpg"), "{e}");
        let e = bad(&|v| v["kinds"]["portrait"]["references"]["geometry"] = json!(["style_refs/unreal/missing.png"]));
        assert!(e.contains("kinds.portrait.references.geometry[0]") && e.contains("missing.png"), "{e}");
        assert!(bad(&|v| v["references"]["note"] = json!(" ")).contains("references.note"));
        assert!(bad(&|v| v["references"]["mood"] = json!([])).contains("lists no images"));
        assert!(bad(&|v| v["kinds"]["portrait"]["style"] = json!("")).contains("kinds.portrait.style"));
        // A style file elsewhere resolves its references next to itself.
        let dir = std::env::temp_dir().join(format!("fn7-style-refs-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("refs")).unwrap();
        let mut v = base.clone();
        v["references"] = json!({ "geometry": ["refs/table.png"], "mood": [], "note": "match them" });
        for k in v["kinds"].as_object_mut().unwrap().values_mut() {
            k.as_object_mut().unwrap().remove("references");
        }
        std::fs::write(dir.join("style.json"), serde_json::to_string(&v).unwrap()).unwrap();
        let e = MediaStyle::load(&dir.join("style.json")).unwrap_err();
        assert!(e.contains("references.geometry[0]") && e.contains("table.png"), "{e}");
        std::fs::write(dir.join("refs/table.png"), b"png").unwrap();
        let ok = MediaStyle::load(&dir.join("style.json")).unwrap();
        assert_eq!(ok.references_for(ImageKind::Portrait).unwrap().geometry[0].path, dir.join("refs/table.png"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(MediaStyle::parse("{", "x.json").unwrap_err().starts_with("x.json: not valid JSON"));
        // Comments are allowed.
        let mut v = base.clone();
        v["_note"] = json!("Benito: try a darker palette next week");
        assert!(MediaStyle::from_value(v, "ok").is_ok());
    }
}
