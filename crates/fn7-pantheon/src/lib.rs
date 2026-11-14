//! # fn7-pantheon
//!
//! The gods as data. `data/pantheon.json` is compiled in; a match records the pantheon version it ran with.
//! This crate owns keyword routing (no LLM), favor bookkeeping, price computation, precedence and rival rules.
//! The gods' LLM minds live in `fn7-agents`.

use fn7_model::{GodId, Pantheon, PowerDef};
use serde::{Deserialize, Serialize};

pub const PANTHEON_JSON: &str = include_str!("../../../data/pantheon.json");
/// The power palette: what demigods cast from the UX (each paid in lightning bolts). Data, not code.
pub const POWERS_JSON: &str = include_str!("../../../data/powers.json");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Powers {
    pub version: String,
    pub powers: Vec<PowerDef>,
}

impl Powers {
    pub fn get(&self, slug: &str) -> Option<&PowerDef> {
        let s = slug.trim().to_lowercase().replace([' ', '-'], "_");
        self.powers.iter().find(|p| p.slug == s)
    }
}

/// The compiled-in power palette.
pub fn powers() -> Powers {
    serde_json::from_str(POWERS_JSON).expect("data/powers.json is valid (checked by tests)")
}

/// The bolt cost of a custom power of `magnitude` (0..10): 1 for small, up to 4.
pub fn custom_cost(magnitude: u8) -> u32 {
    (1 + magnitude.min(10) as u32 / 3).min(4)
}

/// The compiled-in pantheon.
pub fn load() -> Pantheon {
    serde_json::from_str(PANTHEON_JSON).expect("data/pantheon.json is valid (checked by tests)")
}

/// Route by keyword on the raw text (never stemmed: "duplicator" stems to "duplic", Final Night's lesson).
/// Returns the god whose longest keyword matches; ties go to higher precedence. `None` = ordinary act or ask the LLM.
pub fn route_by_keyword(p: &Pantheon, text: &str) -> Option<GodId> {
    let t = text.to_lowercase();
    p.gods
        .iter()
        .enumerate()
        .filter_map(|(i, g)| {
            let best = g.keywords.iter().filter(|k| contains_phrase(&t, k)).map(|k| k.len()).max()?;
            Some((best, g.precedence, i))
        })
        .max()
        .map(|(_, _, i)| GodId::from_ix(i))
}

/// Whole-word phrase match ("age" must not match "damage").
fn contains_phrase(text: &str, phrase: &str) -> bool {
    let p = phrase.to_lowercase();
    let bytes = text.as_bytes();
    let mut start = 0;
    while let Some(off) = text[start..].find(&p) {
        let i = start + off;
        let j = i + p.len();
        let left_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let right_ok = j == text.len() || !bytes[j].is_ascii_alphanumeric();
        if left_ok && right_ok {
            return true;
        }
        start = i + 1;
    }
    false
}

/// Are these two gods rivals (either direction)?
pub fn rivals(p: &Pantheon, a: GodId, b: GodId) -> bool {
    match (p.get(a), p.get(b)) {
        (Some(x), Some(y)) => x.rivals.contains(&y.slug) || y.rivals.contains(&x.slug),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_routes() {
        let p = load();
        assert_eq!(p.gods.len(), 7);
        assert_eq!(p.id_of("entropy"), Some(GodId(6)));
        assert_eq!(route_by_keyword(&p, "unleash infinite power on them"), p.id_of("entropy"));
        assert_eq!(route_by_keyword(&p, "calm the chaos please"), Some(p.id_of("time").unwrap()));
        let pw = powers();
        assert!(pw.powers.len() >= 11);
        assert!(pw.get("Lightning").is_some_and(|x| x.cost == 1));
        for x in &pw.powers {
            assert!(x.cost >= 1 && x.magnitude <= 10 && x.radius_km > 0.0, "{}", x.slug);
        }
        assert_eq!(custom_cost(10), 4);
        for g in &p.gods {
            for r in &g.rivals {
                assert!(p.id_of(r).is_some(), "{} names unknown rival {r}", g.slug);
            }
        }
        let time = p.id_of("time").unwrap();
        assert_eq!(route_by_keyword(&p, "Left hand, REWIND that last bit"), Some(time));
        assert_eq!(route_by_keyword(&p, "slow time and walk through the bullets"), Some(time));
        assert_eq!(route_by_keyword(&p, "raise the baker from the dead"), p.id_of("death"));
        assert_eq!(route_by_keyword(&p, "summon a storm over the docks"), p.id_of("nature"));
        assert_eq!(route_by_keyword(&p, "punch Vex in the damage"), None, "'age' must not match inside 'damage'");
        assert_eq!(route_by_keyword(&p, "walk to the market"), None);
        assert!(rivals(&p, time, p.id_of("consequence").unwrap()));
    }
}
