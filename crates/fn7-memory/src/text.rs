//! Prompt-ready prose helpers: names without engine ids, first-person voice, and an id scrubber.

use fn7_model::*;

/// Display name for any ref, never an engine id. A thin alias of [`World::name_of`], kept for callers of this crate.
#[inline]
pub fn name(w: &World, r: Ref) -> String {
    w.name_of(r)
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Voice {
    /// "I" / "We"
    Subject,
    /// "me" / "us"
    Object,
    /// "I was" / "We were"
    Passive,
}

/// Collectives (clusters, groups) remember as "we"; everyone else as "I".
pub(crate) fn owner_voice(owner: Ref, v: Voice) -> &'static str {
    let we = matches!(owner, Ref::Cluster(_) | Ref::Group(_));
    match (v, we) {
        (Voice::Subject, false) => "I",
        (Voice::Subject, true) => "We",
        (Voice::Object, false) => "me",
        (Voice::Object, true) => "us",
        (Voice::Passive, false) => "I was",
        (Voice::Passive, true) => "We were",
    }
}

/// Scrubbed, trimmed, ending in punctuation.
pub(crate) fn sentence(w: &World, s: &str) -> String {
    let mut t = scrub(w, s.trim());
    if t.is_empty() {
        t.push_str("something happened");
    }
    if !t.ends_with(['.', '!', '?', '"', '\'']) {
        t.push('.');
    }
    t
}

/// Replace any engine ref (`"ch:12"`) in prose with its display name. Defensive: ledger prose is supposed to be clean.
pub fn scrub(w: &World, s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        let starts_word = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if starts_word
            && i + 3 < b.len()
            && b[i].is_ascii_lowercase()
            && b[i + 1].is_ascii_lowercase()
            && b[i + 2] == b':'
            && b[i + 3].is_ascii_digit()
        {
            let mut j = i + 3;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if let Ok(r) = s[i..j].parse::<Ref>() {
                out.push_str(&s[last..i]);
                out.push_str(&name(w, r));
                last = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&s[last..]);
    out
}

/// Whether `s` contains something shaped like an engine id (`[a-z][a-z]:\d`).
pub fn has_engine_id(s: &str) -> bool {
    s.as_bytes().windows(4).any(|w| {
        w[0].is_ascii_lowercase()
            && w[1].is_ascii_lowercase()
            && w[2] == b':'
            && w[3].is_ascii_digit()
    })
}

/// At most `n` chars, cut at a word boundary with an ellipsis.
pub(crate) fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n).collect();
    let cut = match cut.rfind(' ') {
        Some(p) if p > n / 2 => &cut[..p],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', '.', ' ']))
}
