//! Who or what an asset shows: `(subject_kind, subject_key, display_name)`.
//!
//! Keys: a demigod, god or person by its **normalized name** (`fn7_profile::normalize_name` of the name without a
//! trailing ` (2)` counter: the same identity its visual sheet keys on); an item by its normalized descriptor; a
//! match-scoped picture (a moment card, an army) by its match id; anything shared by a fixed key.

use fn7_profile::normalize_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub kind: &'static str,
    pub key: String,
    pub name: String,
}

impl Subject {
    pub fn new(kind: &'static str, key: impl Into<String>, name: impl Into<String>) -> Self {
        Self { kind, key: key.into(), name: name.into() }
    }
    pub fn shared(key: &str, name: &str) -> Self {
        Self::new("shared", key, name)
    }
    pub fn unknown() -> Self {
        Self::new("shared", "unknown", "unknown")
    }
    pub fn named(kind: &'static str, name: &str) -> Self {
        match name_key(name) {
            Some(k) => Self::new(kind, k, name.trim()),
            None => Self::unknown(),
        }
    }
}

/// A name as a subject key: the trailing ` (N)` counter dropped (the plugin's `visual::image_name`), then
/// normalized (NFKC, invisibles dropped, whitespace collapsed, lowercased, 40 chars).
pub fn name_key(name: &str) -> Option<String> {
    let t = name.trim();
    let base = match t.strip_suffix(')').and_then(|x| x.rfind(" (").map(|i| (x, i))) {
        Some((x, i)) if !x[i + 2..].is_empty() && x[i + 2..].chars().all(|c| c.is_ascii_digit()) => x[..i].trim_end(),
        _ => t,
    };
    normalize_name(base)
}

/// The subject of a media-layer image, read from its kind and exact prompt (`<Framing>: <subject>. Style: …`).
/// Portraits and dioramas name `<Name>, a demigod:`; statues `the demigod <Name> (` or `,`; items lead with their
/// descriptor; moment cards, armies and anything unreadable belong to the match that asked (`match_id`).
pub fn from_prompt(kind: &str, prompt: &str, match_id: Option<&str>) -> Subject {
    let body = prompt.split(" Style: ").next().unwrap_or(prompt);
    let of_match = |label: &str| match match_id.filter(|m| !m.is_empty() && *m != "-" && *m != "prewarm") {
        Some(m) => Subject::new("match", m, clip(label, 80)),
        None => Subject::new("shared", "unknown", clip(label, 80)),
    };
    let after_framing = body.split_once(": ").map(|x| x.1).unwrap_or(body);
    match kind {
        "portrait" | "diorama" => {
            for (mark, k) in [(", a demigod:", "demigod"), (", a mortal caught up", "person"), (", a god:", "god")] {
                if let Some(i) = body.find(mark) {
                    let head = &body[..i];
                    let name = head.rsplit_once(": ").map(|x| x.1).unwrap_or(head);
                    let s = Subject::named(k, name);
                    if s.key != "unknown" {
                        return s;
                    }
                }
            }
            of_match(after_framing)
        }
        "statue" => {
            if let Some(i) = after_framing.find("the demigod ") {
                let rest = &after_framing[i + "the demigod ".len()..];
                let end = [rest.find(" ("), rest.find(", "), rest.find(". ")].into_iter().flatten().min().unwrap_or(rest.len());
                let s = Subject::named("demigod", &rest[..end]);
                if s.key != "unknown" {
                    return s;
                }
            }
            of_match(after_framing)
        }
        "item" => {
            let end = [after_framing.find(": "), after_framing.find(". "), after_framing.find(", ")]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(after_framing.len());
            let s = Subject::named("item", after_framing[..end].trim_end_matches('.'));
            if s.key != "unknown" { s } else { of_match(after_framing) }
        }
        "place" => {
            let label = after_framing.split([',', ';']).next().unwrap_or(after_framing);
            let s = Subject::named("place", label.trim_end_matches('.'));
            if s.key != "unknown" { s } else { of_match(after_framing) }
        }
        _ => of_match(after_framing),
    }
}

/// The avoid / negative tail of a prompt (`No words or letters …` or `Avoid: …`), when it has one. `known` is the
/// style's avoid text when the caller has it.
pub fn avoid_tail(prompt: &str, known: Option<&str>) -> Option<String> {
    if let Some(k) = known.map(str::trim).filter(|k| !k.is_empty()) {
        if prompt.trim_end().ends_with(k) {
            return Some(k.to_string());
        }
    }
    let style = prompt.find(" Style: ")?;
    let tail = &prompt[style..];
    let i = [tail.rfind(". Avoid: "), tail.find(". No ")].into_iter().flatten().min()?;
    Some(tail[i + 2..].trim().to_string())
}

pub fn clip(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    t.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_from_prompts() {
        let p = "Waist-up, three-quarter view, dramatic rim light: Lune's Left Hand, a demigod: wiry and long-limbed; a larger-than-life figure. Style: a dark-fantasy world. No words or letters in the picture, no gore, no real people.";
        let s = from_prompt("portrait", p, None);
        assert_eq!((s.kind, s.key.as_str(), s.name.as_str()), ("demigod", "lune's left hand", "Lune's Left Hand"));
        assert_eq!(avoid_tail(p, None).as_deref(), Some("No words or letters in the picture, no gore, no real people."));
        let st = "A slightly larger faceted low-poly figure of the demigod on a raised hexagonal plinth, lit by candles: a faceted low-poly figure of the demigod Benito's Left Hand (round and sturdy), mid-stride. Style: x.";
        assert_eq!(from_prompt("statue", st, Some("m1")).key, "benito's left hand");
        let old = "An amiibo-style collectible statue on a round display base: An amiibo-style collectible figure of the demigod Mara, frozen mid-fall, on a round base. Style: x.";
        assert_eq!(from_prompt("statue", old, None).key, "mara");
        let it = "A single item shown alone as a tiny game piece, on the dark candlelit table: Healing shrine to Left Hand: a gleaming shrine rises. Style: x.";
        assert_eq!(from_prompt("item", it, None).key, "healing shrine to left hand");
        let ev = "Seen from high above the hex-tile table: Vex casts volcano on Mara in Oslo; starring Vex (round), rendered with blocky geometry. Style: x.";
        let e = from_prompt("event", ev, Some("m9"));
        assert_eq!((e.kind, e.key.as_str()), ("match", "m9"));
        assert_eq!(from_prompt("event", ev, Some("prewarm")).key, "unknown");
        assert_eq!(name_key(" Chris (2) ").as_deref(), Some("chris"));
        assert_eq!(name_key("Ｃｈｒｉｓ").as_deref(), Some("chris"));
    }
}
