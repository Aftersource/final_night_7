//! Payload annotation: walk an outgoing payload and put an image object next to every image prompt.
//!
//! | Found in an object | Added beside it | Kind |
//! |---|---|---|
//! | `image_prompt: string` | `image` | `image_kind` if given, else inferred (below) |
//! | `statue_prompt: string` (when no `image_prompt`) | `statue_image` | statue |
//! | `portrait_prompt: string` | `portrait_image` | portrait |
//! | `diorama_prompt: string` | `diorama_image` | diorama |
//!
//! Inference when `image_kind` is absent: `statue` inside `piles`/`statues` or next to a `statue_prompt`; `portrait`
//! under a key containing "portrait" or on an object whose `ref`/`demigod`/`character` is a `dg:`/`ch:` ref; `item` on
//! an `it:` ref; otherwise `event`.
//!
//! **Event selection (moment cards).** An event object (one with a numeric `magnitude` and a `kind`, as in
//! `events[]`) whose prompt is an event gets an image only if the style's `event_selection` picks it (magnitude ≥
//! `min_magnitude`, or its `kind` is in `always`); the per-match cap (`max_per_match`) is enforced by
//! [`Media::request`]. Other objects (storyboard shots, statues, portraits) are not filtered, and storyboard shots
//! (objects with a `camera`) don't count against the cap (they mostly reuse the moment cards' images; budgets apply).
//!
//! Every prompt is written in the service's style ([`Media::style`]). Annotation only enqueues: it never waits on a
//! generator, so it never holds up a beat.
//!
//! **Interim prompts.** Until the plugin sends any `image_prompt` (checked per process), two more rules give the
//! client something to show: public omens (`omens[]`, the big god rulings) become event images, and demigod lists
//! (`demigods[]` with a `dg:` ref and a name) get portraits. They switch off for good the first time an `image_prompt`
//! is seen, so nothing is generated twice under two prompts.

use serde_json::{Map, Value};

use crate::prompt::{ImageKind, ImagePrompt};
use crate::{ImageRef, Media};

/// A look the annotation met ([`Media::annotate_seen`]): a demigod's `look` object or a `look.changed` event.
#[derive(Debug, Clone, PartialEq)]
pub struct LookSeen {
    /// The demigod (`dg:N`), its name and its player, when the payload says.
    pub demigod: Option<String>,
    pub name: Option<String>,
    pub player: Option<String>,
    /// The loadout its images show (`""`: the base look) and since which beat.
    pub loadout_hash: String,
    pub since_beat: Option<u64>,
    /// The names of the loadout's items.
    pub loadout: Vec<String>,
    /// The gear portrait and diorama asked for (`None`: the base look).
    pub portrait: Option<ImageRef>,
    pub diorama: Option<ImageRef>,
    /// A `look.changed` event as the plugin sent it (before annotation), when this is one.
    pub event: Option<Value>,
}

impl Media {
    /// [`Media::annotate`], also returning every look it met (for the profile library and the ready events).
    pub fn annotate_seen(&self, payload: &mut Value, match_id: Option<&str>) -> (usize, Vec<LookSeen>) {
        if !self.enabled() {
            return (0, vec![]);
        }
        let mut seen = vec![];
        self.looks(payload, match_id, &mut seen);
        (self.annotate(payload, match_id), seen)
    }

    /// The look pass (before the generic one): every object carrying the look inputs (`base_portrait`,
    /// `base_diorama`, `portrait_subject`, `diorama_subject`, `fallback`) gets `portrait_image` and `diorama_image`
    /// (the image to show NOW: the gear image when it is ready, else the newest earlier look that is, else the base
    /// image) and, while a gear image renders, `pending: {loadout_hash, portrait_image?, diorama_image?}`. The inputs
    /// are removed.
    fn looks(&self, v: &mut Value, match_id: Option<&str>, seen: &mut Vec<LookSeen>) {
        match v {
            Value::Array(a) => {
                for x in a {
                    self.looks(x, match_id, seen);
                }
            }
            Value::Object(o) => {
                if o.contains_key("base_portrait") && o.contains_key("base_diorama") {
                    let mut s = self.look(o, match_id);
                    let get = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
                    s.demigod = get("demigod");
                    s.name = get("name");
                    s.player = get("player");
                    seen.push(s);
                }
                let (r, name, player) = {
                    let get = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
                    (get("ref"), get("name"), get("player"))
                };
                for (k, x) in o.iter_mut() {
                    let at = seen.len();
                    self.looks(x, match_id, seen);
                    // A demigod's own `look`: the demigod is the object around it.
                    if k == "look" && seen.len() == at + 1 && seen[at].demigod.is_none() {
                        seen[at].demigod = r.clone();
                        seen[at].name = name.clone();
                        seen[at].player = player.clone();
                    }
                }
            }
            _ => {}
        }
    }

    fn look(&self, o: &mut Map<String, Value>, match_id: Option<&str>) -> LookSeen {
        let text = |o: &Map<String, Value>, k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
        let event = o.contains_key("event").then(|| Value::Object(o.clone()));
        let fallback: Vec<Value> = o.get("fallback").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut seen = LookSeen {
            demigod: None,
            name: None,
            player: None,
            loadout_hash: text(o, "image_loadout_hash").unwrap_or_default(),
            since_beat: o.get("since_beat").and_then(Value::as_u64),
            loadout: o
                .get("loadout")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x["name"].as_str().map(str::to_string)).collect())
                .or_else(|| o.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()))
                .unwrap_or_default(),
            portrait: None,
            diorama: None,
            event,
        };
        let mut pending = Map::new();
        for (kind, base_key, subject_key, out) in [
            (ImageKind::Portrait, "base_portrait", "portrait_subject", "portrait_image"),
            (ImageKind::Diorama, "base_diorama", "diorama_subject", "diorama_image"),
        ] {
            let Some(base) = text(o, base_key).and_then(|b| self.prompt(kind, &b)) else { continue };
            // The base first: it is the gear picture's first reference, so it must be ahead of it in the queue.
            let base_img = self.request_with(&base, match_id, true);
            let shown = match text(o, subject_key).and_then(|s| self.prompt_on(kind, &s, &base)) {
                None => base_img,
                Some(want) => {
                    let img = self.request_with(&want, match_id, true);
                    let shown = if img.status == "ready" {
                        img.clone()
                    } else {
                        if img.status == "pending" {
                            pending.insert(out.to_string(), img.to_json());
                        }
                        // The newest earlier look whose picture exists, else the base.
                        fallback
                            .iter()
                            .filter_map(|f| f[subject_key].as_str())
                            .filter_map(|s| self.prompt_on(kind, s, &base))
                            .filter_map(|p| self.peek(&p))
                            .find(|r| r.status == "ready")
                            .unwrap_or(base_img)
                    };
                    match kind {
                        ImageKind::Portrait => seen.portrait = Some(img),
                        _ => seen.diorama = Some(img),
                    }
                    shown
                }
            };
            o.insert(out.to_string(), shown.to_json());
        }
        for k in ["base_portrait", "base_diorama", "portrait_subject", "diorama_subject", "fallback"] {
            o.remove(k);
        }
        if pending.is_empty() {
            o.remove("pending");
        } else {
            pending.insert("loadout_hash".into(), seen.loadout_hash.clone().into());
            o.insert("pending".into(), Value::Object(pending));
        }
        seen
    }

    /// Annotate `payload` in place (see the module doc). Returns how many image objects were added. Never blocks; with
    /// images off it changes nothing and returns 0.
    pub fn annotate(&self, payload: &mut Value, match_id: Option<&str>) -> usize {
        if !self.enabled() {
            return 0;
        }
        if has_key(payload, "image_prompt") {
            self.note_image_prompt();
        }
        let interim = self.interim_prompts();
        let mut n = 0;
        self.walk(payload, "", interim, match_id, &mut n);
        n
    }

    fn walk(
        &self,
        v: &mut Value,
        parent: &str,
        interim: bool,
        match_id: Option<&str>,
        n: &mut usize,
    ) {
        match v {
            Value::Array(a) => {
                for x in a {
                    self.walk(x, parent, interim, match_id, n);
                }
            }
            Value::Object(o) => {
                let capped = !o.contains_key("camera");
                for (out, p) in self.prompts_in(o, parent, interim) {
                    if let Some(p) = p {
                        o.insert(out.to_string(), self.request_with(&p, match_id, capped).to_json());
                        *n += 1;
                    }
                }
                for (k, x) in o.iter_mut() {
                    if matches!(k.as_str(), "image" | "statue_image" | "portrait_image" | "diorama_image") {
                        continue;
                    }
                    self.walk(x, k, interim, match_id, n);
                }
            }
            _ => {}
        }
    }

    fn prompts_in(
        &self,
        o: &Map<String, Value>,
        parent: &str,
        interim: bool,
    ) -> Vec<(&'static str, Option<ImagePrompt>)> {
        let s = |k: &str| o.get(k).and_then(Value::as_str);
        let style = self.style();
        let mut out = vec![];
        if let Some(text) = s("image_prompt") {
            let kind = s("image_kind")
                .and_then(ImageKind::parse)
                .unwrap_or_else(|| infer_kind(o, parent));
            let selected = match (kind, o.get("magnitude").and_then(Value::as_u64), s("kind")) {
                (ImageKind::Event, Some(m), Some(k)) => {
                    style.event_selection.selects(k, m.min(255) as u8)
                }
                _ => true,
            };
            if selected {
                out.push(("image", self.prompt(kind, text)));
            }
        } else if let Some(text) = s("statue_prompt") {
            out.push(("statue_image", self.prompt(ImageKind::Statue, text)));
        }
        if let Some(text) = s("portrait_prompt") {
            out.push(("portrait_image", self.prompt(ImageKind::Portrait, text)));
        }
        if let Some(text) = s("diorama_prompt") {
            out.push(("diorama_image", self.prompt(ImageKind::Diorama, text)));
        }
        if interim && out.is_empty() && !o.contains_key("image") && !o.contains_key("image_prompt") {
            if parent == "omens" {
                if let (Some(god), Some(speech)) = (s("god"), s("speech")) {
                    let speech = speech.trim().trim_matches('"');
                    let subject = format!("the god {god} reaches down into the battle royale of demigods, decreeing \"{speech}\"");
                    out.push(("image", self.prompt(ImageKind::Event, &subject)));
                }
            } else if parent == "demigods" {
                if let (Some(r), Some(name)) = (s("ref"), s("name")) {
                    if r.starts_with("dg:") {
                        out.push((
                            "image",
                            ImagePrompt::portrait(&crate::prompt::Portrait {
                                role: "the demigod".into(),
                                name: name.into(),
                                ..Default::default()
                            })
                            .map(|p| p.restyle(style)),
                        ));
                    }
                }
            }
        }
        out
    }
}

fn infer_kind(o: &Map<String, Value>, parent: &str) -> ImageKind {
    let r = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("");
    let refs = [r("ref"), r("demigod"), r("character"), r("item")];
    if matches!(parent, "piles" | "statues") || o.contains_key("statue_prompt") {
        ImageKind::Statue
    } else if parent.contains("portrait")
        || refs
            .iter()
            .any(|x| x.starts_with("dg:") || x.starts_with("ch:"))
    {
        ImageKind::Portrait
    } else if refs.iter().any(|x| x.starts_with("it:")) {
        ImageKind::Item
    } else {
        ImageKind::Event
    }
}

fn has_key(v: &Value, key: &str) -> bool {
    match v {
        Value::Object(o) => o.contains_key(key) || o.values().any(|x| has_key(x, key)),
        Value::Array(a) => a.iter().any(|x| has_key(x, key)),
        _ => false,
    }
}
