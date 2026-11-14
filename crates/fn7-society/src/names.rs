//! Names for the people of the society: drawn from the culture of the place a person comes from (the society
//! template's `names.cultures`, by country, then by UN subregion; the template's old pool where no culture claims the
//! place), never reusing a full name within a match and reusing a surname only when the pool runs dry or a family
//! gives it on purpose. Deterministic: the rolls come from the caller's seeded `Rng` and the world as it stands.

use std::collections::{BTreeMap, BTreeSet};

use fn7_model::seed::Rng;
use fn7_model::*;

use crate::template::{template, NameCulture};

/// Rolls before a surname someone already carries is accepted (once), and before any surname is.
const FRESH_SURNAME_TRIES: usize = 14;
const ANY_SURNAME_TRIES: usize = 28;

/// The country and UN subregion of a place (walking up the hierarchy): a planet city carries its country in its tags
/// and sits in a region named for its subregion; a city arena carries its country in the arena's tags.
pub fn country_and_subregion(w: &World, place: PlaceId) -> (Option<&str>, Option<&str>) {
    let mut country: Option<&str> = None;
    let mut subregion: Option<&str> = None;
    let mut p = Some(place);
    for _ in 0..8 {
        let Some(pl) = p.and_then(|x| w.places.get(x.ix())) else { break };
        let holds_country = match pl.kind {
            PlaceKind::Site => w.is_planet(),
            PlaceKind::Arena => !w.is_planet(),
            _ => false,
        };
        match pl.kind {
            _ if holds_country && country.is_none() => country = pl.tags.get(1).map(String::as_str),
            PlaceKind::District if w.is_planet() && subregion.is_none() => subregion = Some(pl.name.as_str()),
            _ => {}
        }
        p = pl.parent;
    }
    if subregion.is_none() {
        subregion = country.and_then(crate::geo::country_named).map(|c| c.subregion.as_str());
    }
    (country, subregion)
}

/// The naming culture of a place, if any culture claims its country or subregion.
pub fn culture_for(w: &World, place: PlaceId) -> Option<&'static NameCulture> {
    let t = template();
    let (country, subregion) = country_and_subregion(w, place);
    country
        .and_then(|c| t.names.cultures.iter().find(|x| x.countries.iter().any(|k| k.eq_ignore_ascii_case(c))))
        .or_else(|| subregion.and_then(|s| t.names.cultures.iter().find(|x| x.subregions.iter().any(|k| k == s))))
}

/// A full name nobody in the match has, for someone from `place`. `family` = the surname a family gives on purpose
/// (only the given name is then rolled). Surnames already carried are avoided while the pool allows.
pub fn pick_name(w: &World, place: PlaceId, rng: &mut Rng, family: Option<&str>) -> String {
    let t = template();
    let (given, surnames) = match culture_for(w, place) {
        Some(c) => (&c.given, &c.surnames),
        None => (&t.names.given, &t.names.surnames),
    };
    let taken: BTreeSet<&str> = w.characters.iter().map(|c| c.name.as_str()).collect();
    let mut carried: BTreeMap<&str, u32> = BTreeMap::new();
    for c in &w.characters {
        *carried.entry(crate::sim::people::surname(&c.name)).or_default() += 1;
    }
    let mut last = None;
    for attempt in 0..ANY_SURNAME_TRIES {
        let g = rng.pick(given).map(String::as_str).unwrap_or("Nameless");
        let s = match family {
            Some(f) => f,
            None => rng.pick(surnames).map(String::as_str).unwrap_or("Nobody"),
        };
        let full = format!("{g} {s}");
        if taken.contains(full.as_str()) {
            continue;
        }
        let n = carried.get(s).copied().unwrap_or(0);
        if family.is_some() || n == 0 || (attempt >= FRESH_SURNAME_TRIES && n <= 1) {
            return full;
        }
        last = Some(full);
    }
    if let Some(full) = last {
        return full;
    }
    // Every roll collided: a double-barrelled surname nobody carries.
    for _ in 0..64 {
        let g = rng.pick(given).map(String::as_str).unwrap_or("Nameless");
        let a = family.or_else(|| rng.pick(surnames).map(String::as_str)).unwrap_or("Nobody");
        let b = rng.pick(surnames).map(String::as_str).unwrap_or("Else");
        let full = format!("{g} {a}-{b}");
        if a != b && !taken.contains(full.as_str()) {
            return full;
        }
    }
    format!("{} {}", rng.pick(given).map(String::as_str).unwrap_or("Nameless"), w.characters.len())
}
