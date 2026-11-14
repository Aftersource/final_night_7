//! Image prompts in one house style. Every prompt is `<Framing for its kind>: <subject>. Style: <house style>. Avoid:
//! <avoid list>.`, with the framing, size, house style and avoid list from the style config ([`crate::style`],
//! `data/media_style.json`), built deterministically (same inputs, same bytes) from plain names and text, with engine ids
//! (`dg:3`, `ch:12`, `lg:49`, …) scrubbed out: an image model must never see them, and they would make identical
//! pictures hash differently.

use serde::{Deserialize, Serialize};

use crate::style::MediaStyle;

/// Bumped when the prompt format changes (it is part of every image key, so old images are not reused for a new
/// format). The style config's hash is part of the key too.
pub const PROMPT_VERSION: &str = "fn7-media/2";

/// Longest subject kept (characters); the rest is cut at a word boundary.
pub const MAX_SUBJECT_CHARS: usize = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    /// A demigod or a memorized character.
    Portrait,
    /// One item.
    Item,
    /// A big ledger row / god ruling / world event.
    Event,
    /// A demigod's end-of-match statue.
    Statue,
    /// A region, city or the arena (requested by reference).
    Place,
    /// An army (requested by reference).
    Army,
    /// A demigod as a collectible minifig on a small diorama base themed on its home region (the table piece).
    Diorama,
}

impl ImageKind {
    pub const ALL: [ImageKind; 7] = [
        Self::Portrait,
        Self::Item,
        Self::Event,
        Self::Statue,
        Self::Place,
        Self::Army,
        Self::Diorama,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Item => "item",
            Self::Event => "event",
            Self::Statue => "statue",
            Self::Place => "place",
            Self::Army => "army",
            Self::Diorama => "diorama",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == s.trim().to_ascii_lowercase())
    }

    /// Images that want a transparent background where the model supports it.
    pub fn wants_transparency(self) -> bool {
        matches!(self, Self::Item | Self::Statue | Self::Diorama)
    }
}

/// A finished prompt: its kind, the scrubbed subject, and the full text sent to the image model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagePrompt {
    pub kind: ImageKind,
    pub subject: String,
    pub text: String,
    /// `WIDTHxHEIGHT`, from the style's kind.
    pub size: String,
}

impl ImagePrompt {
    /// A prompt from free text (a plugin's `image_prompt`, a statue prompt, a news line). `None` when nothing is left
    /// after scrubbing.
    /// In the compiled-in style ([`MediaStyle::builtin`]); the server uses [`ImagePrompt::styled`] with its loaded one.
    pub fn from_subject(kind: ImageKind, subject: &str) -> Option<Self> {
        Self::styled(&MediaStyle::builtin(), kind, subject)
    }

    /// A prompt from free text in `style`. `None` when nothing is left after scrubbing.
    pub fn styled(style: &MediaStyle, kind: ImageKind, subject: &str) -> Option<Self> {
        let subject = clip(&scrub(subject), MAX_SUBJECT_CHARS);
        let subject = subject.trim_end_matches(['.', ' ']).to_string();
        if subject.is_empty() {
            return None;
        }
        Some(Self::build(style, kind, subject))
    }

    fn build(style: &MediaStyle, kind: ImageKind, subject: String) -> Self {
        let k = style.kind(kind);
        let framing = capitalize(k.framing.trim().trim_end_matches([':', '.', ' ']));
        let text = format!("{framing}: {subject}. {}", style.suffix());
        Self {
            kind,
            subject,
            text,
            size: k.size,
        }
    }

    /// The same subject and kind in another style.
    pub fn restyle(&self, style: &MediaStyle) -> Self {
        Self::build(style, self.kind, self.subject.clone())
    }

    pub fn portrait(p: &Portrait) -> Option<Self> {
        let mut s = format!("{} {}", p.role.trim(), p.name.trim());
        if let Some(e) = nonempty(&p.epithet) {
            s.push_str(&format!(", {e}"));
        }
        if let Some(d) = nonempty(&p.description) {
            s.push_str(&format!(", {d}"));
        }
        if let Some(g) = nonempty(&p.patron) {
            s.push_str(&format!(", marked by the god {g}"));
        }
        Self::from_subject(ImageKind::Portrait, &s)
    }

    pub fn item(i: &Item) -> Option<Self> {
        let mut s = format!("the item \"{}\"", i.name.trim());
        if let Some(d) = nonempty(&i.description) {
            s.push_str(&format!(", {d}"));
        }
        if let Some(o) = nonempty(&i.origin) {
            s.push_str(&format!(" ({o})"));
        }
        Self::from_subject(ImageKind::Item, &s)
    }

    pub fn event(e: &Event) -> Option<Self> {
        let mut s = e.text.trim().trim_end_matches('.').to_string();
        if let Some(p) = nonempty(&e.place) {
            s.push_str(&format!(", over {p}"));
        }
        let actors: Vec<&str> = e
            .actors
            .iter()
            .map(|a| a.trim())
            .filter(|a| !a.is_empty())
            .collect();
        if !actors.is_empty() {
            s.push_str(&format!(", starring {}", actors.join(", ")));
        }
        Self::from_subject(ImageKind::Event, &s)
    }

    pub fn statue(st: &Statue) -> Option<Self> {
        let pose = if st.alive {
            "standing on the summit, smug, barely scratched".to_string()
        } else {
            match nonempty(&st.cause) {
                Some(c) => format!("frozen mid-fall ({c})"),
                None => "frozen mid-fall".to_string(),
            }
        };
        let mut s = format!("the demigod {}, {pose}", st.name.trim());
        if let Some(b) = nonempty(&st.base) {
            s.push_str(&format!(", on a base shaped like {b}"));
        }
        let items: Vec<&str> = st
            .items
            .iter()
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .take(24)
            .collect();
        s.push_str(&format!(
            ", atop a towering pile of everything they held and summoned: {}",
            if items.is_empty() {
                "nothing but debts".into()
            } else {
                items.join(", ")
            }
        ));
        let people: Vec<&str> = st
            .people
            .iter()
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .take(6)
            .collect();
        if !people.is_empty() {
            s.push_str(&format!(
                "; tiny figures of the people they touched cling to the pile: {}",
                people.join(", ")
            ));
        }
        Self::from_subject(ImageKind::Statue, &s)
    }
}

/// A portrait's inputs. `role` is "the demigod", "the character", …
#[derive(Debug, Clone, Default)]
pub struct Portrait {
    pub role: String,
    pub name: String,
    pub epithet: Option<String>,
    pub description: Option<String>,
    pub patron: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Item {
    pub name: String,
    pub description: Option<String>,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Event {
    pub text: String,
    pub place: Option<String>,
    pub actors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Statue {
    pub name: String,
    pub alive: bool,
    pub cause: Option<String>,
    pub base: Option<String>,
    pub items: Vec<String>,
    pub people: Vec<String>,
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn nonempty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Remove engine ids (`xx:123`, two lowercase letters, a colon and digits, standing alone), then tidy what is left:
/// empty brackets go, whitespace collapses, stray separators at the ends are trimmed.
pub fn scrub(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < c.len() {
        let starts_id = i + 3 < c.len()
            && (i == 0 || !c[i - 1].is_alphanumeric())
            && c[i].is_ascii_lowercase()
            && c[i + 1].is_ascii_lowercase()
            && c[i + 2] == ':'
            && c[i + 3].is_ascii_digit();
        if starts_id {
            let mut j = i + 3;
            while j < c.len() && c[j].is_ascii_digit() {
                j += 1;
            }
            if j == c.len() || !c[j].is_alphanumeric() {
                i = j;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    let mut prev = String::new();
    while prev != out {
        prev = out.clone();
        for empty in ["()", "[]", "( )", "[ ]", "(, )", "(,)"] {
            out = out.replace(empty, "");
        }
        out = out.replace(" ,", ",").replace(",,", ",").replace(" .", ".");
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    out.trim_matches(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | ':' | '-'))
        .to_string()
}

/// `s` cut to at most `max` chars, at a word boundary when one is near.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    match cut.rfind(' ') {
        Some(ix) if ix > max / 2 => cut[..ix].to_string(),
        _ => cut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_removes_engine_ids_only() {
        assert_eq!(
            scrub("Rook (dg:0) hits ch:12, near pl:3."),
            "Rook hits, near."
        );
        assert_eq!(
            scrub("lg:49 Juniper's Send #1 hits ag:17"),
            "Juniper's Send #1 hits"
        );
        // Not ids: times, words with colons, ids glued to words.
        assert_eq!(
            scrub("at 12:30, ratio 3:1, abc:12, dg:12x"),
            "at 12:30, ratio 3:1, abc:12, dg:12x"
        );
    }

    #[test]
    fn prompts_are_deterministic_styled_and_id_free() {
        let st = Statue {
            name: "Rook".into(),
            alive: false,
            cause: Some("caught by dg:2 outside the arena".into()),
            base: Some("Northeast Works".into()),
            items: vec!["laser sword".into(), "it:4".into(), "bag of riches".into()],
            people: vec!["Ada Okafor".into()],
        };
        let a = ImagePrompt::statue(&st).unwrap();
        let b = ImagePrompt::statue(&st).unwrap();
        assert_eq!(a, b);
        let style = MediaStyle::builtin();
        assert!(a.text.ends_with(&style.suffix()));
        assert!(a.text.starts_with("Amiibo-style figure on a round base"), "{}", a.text);
        assert_eq!(a.size, "1024x1024");
        assert!(
            a.text.contains("Rook")
                && a.text.contains("laser sword")
                && a.text.contains("Ada Okafor")
        );
        for id in ["dg:2", "it:4", "dg:", "it:"] {
            assert!(!a.text.contains(id), "{id} leaked into {}", a.text);
        }
        for p in [
            ImagePrompt::portrait(&Portrait {
                role: "the demigod".into(),
                name: "Moth".into(),
                ..Default::default()
            }),
            ImagePrompt::item(&Item {
                name: "Wendy's sand shield".into(),
                ..Default::default()
            }),
            ImagePrompt::event(&Event {
                text: "Time freezes the harbor (gd:0)".into(),
                place: Some("Jakarta".into()),
                actors: vec![],
            }),
            ImagePrompt::from_subject(
                ImageKind::Statue,
                "An amiibo-style figure of the demigod Rook",
            ),
        ] {
            let p = p.unwrap();
            assert!(p.text.ends_with(&style.suffix()), "{}", p.text);
            assert!(p.text.contains(&style.house_style) && p.text.contains("Avoid: text, logos"));
            assert!(!p.text.contains("gd:0"));
        }
        assert!(ImagePrompt::from_subject(ImageKind::Event, " dg:1 (ch:2) ").is_none());
    }

    #[test]
    fn long_subjects_are_clipped() {
        let long = "pandemonium ".repeat(400);
        let p = ImagePrompt::from_subject(ImageKind::Event, &long).unwrap();
        assert!(p.subject.chars().count() <= MAX_SUBJECT_CHARS);
    }
}
