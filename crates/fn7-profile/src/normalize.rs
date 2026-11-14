//! Names as keys. The Theatre has no accounts: a player is whoever types a display name, and the resume token and
//! `player_id` change every session. So a profile is keyed by the **normalized display name**:
//!
//! 1. Unicode NFKC (fullwidth "Ｃｈｒｉｓ" → "Chris", ligatures split, compatibility forms folded);
//! 2. invisible characters dropped (controls, zero-width spaces and joiners, bidi overrides, BOM, soft hyphen);
//! 3. whitespace trimmed and collapsed to single spaces;
//! 4. lowercased;
//! 5. at most [`MAX_NAME_CHARS`] characters (the plugin keeps 40 of a display name too).
//!
//! The same rule is the demigod identity key (a demigod's visual sheet keys on its trimmed, lowercased name; NFKC and
//! the invisible-character drop only make that stricter).
//!
//! **Collisions and impersonation.** Anyone who types "Chris" lands in Chris's profile. [`skeleton`] folds common
//! lookalikes (Cyrillic and Greek letters, 0/1/3/5 for o/l/e/s, separators) so the store can flag a new profile that
//! *looks* like an existing one ("Chr1s", "Сhris" with a Cyrillic С); a claimed profile ([`crate::ProfileStore::claim`])
//! protects its description from edits. Neither stops someone playing under the name: see the design doc.

use unicode_normalization::UnicodeNormalization;

/// Longest key kept, in characters.
pub const MAX_NAME_CHARS: usize = 40;

fn invisible(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{115F}'
                | '\u{1160}'
                | '\u{17B4}'
                | '\u{17B5}'
                | '\u{180B}'..='\u{180F}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{E0000}'..='\u{E007F}'
        )
}

/// A display name as a profile key, or `None` if nothing printable is left.
pub fn normalize_name(raw: &str) -> Option<String> {
    let folded: String = raw.nfkc().filter(|c| !invisible(*c)).collect();
    let collapsed = folded.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = collapsed.to_lowercase();
    let key: String = lower.chars().take(MAX_NAME_CHARS).collect();
    let key = key.trim_end().to_string();
    (!key.is_empty()).then_some(key)
}

/// A display name cleaned for showing (NFKC, invisibles dropped, whitespace collapsed, case kept, capped).
pub fn display_name(raw: &str) -> String {
    let folded: String = raw.nfkc().filter(|c| !invisible(*c)).collect();
    let s = folded.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(MAX_NAME_CHARS).collect::<String>().trim_end().to_string()
}

/// A lookalike skeleton of a key: confusable letters folded to ASCII, digits read as letters, separators dropped.
/// Two different keys with one skeleton are flagged as lookalikes; nothing is merged.
pub fn skeleton(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for c in key.chars() {
        let m = match c {
            // Cyrillic
            'а' => 'a',
            'в' => 'b',
            'е' | 'ё' => 'e',
            'һ' => 'h',
            'і' | 'ї' | 'ӏ' => 'i',
            'ј' => 'j',
            'к' => 'k',
            'м' => 'm',
            'н' => 'h',
            'о' => 'o',
            'р' => 'p',
            'с' => 'c',
            'т' => 't',
            'у' => 'y',
            'х' => 'x',
            'ѕ' => 's',
            'ԁ' => 'd',
            'ԛ' => 'q',
            'ԝ' => 'w',
            // Greek
            'α' => 'a',
            'β' => 'b',
            'ε' => 'e',
            'η' => 'n',
            'ι' => 'i',
            'κ' => 'k',
            'ν' => 'v',
            'ο' => 'o',
            'ρ' => 'p',
            'τ' => 't',
            'υ' => 'u',
            'χ' => 'x',
            // Digits and symbols read as letters
            '0' => 'o',
            '1' | '|' | '!' => 'l',
            '3' => 'e',
            '4' | '@' => 'a',
            '5' | '$' => 's',
            '7' => 't',
            '8' => 'b',
            'ı' => 'i',
            // Separators vanish
            ' ' | '_' | '-' | '.' | '\'' | '`' | '·' | '•' => continue,
            c => c,
        };
        out.push(m);
    }
    // i and l are interchangeable in most fonts' small sizes; "rn" reads as "m"; "vv" as "w".
    out.replace('i', "l").replace("rn", "m").replace("vv", "w")
}
