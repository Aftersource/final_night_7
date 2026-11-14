//! Lazy individuality: resolve a mention to a memorized character, or materialize one from the best cluster.

use std::sync::OnceLock;

use fn7_model::seed::Rng;
use fn7_model::*;
use serde::{Deserialize, Serialize};
use theatre_toolkit::consensus::normalize;

use crate::geo::{place_at, random_in};
use crate::template::template;

/// Score at or above which a mention is taken to mean an existing character.
pub const EXISTING_MIN: f32 = 0.88;
/// Score given to a materialization picked only by place (no word overlap).
pub const PLACE_ONLY_SCORE: f32 = 0.1;

/// What a mention refers to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "to")]
pub enum Resolution {
    /// An already memorized character.
    Existing { char: CharId, score: f32 },
    /// Nobody yet: materialize from this cluster.
    Materialize { cluster: ClusterId, score: f32 },
    /// Not a person, or nothing fits.
    None,
}

fn stopwords() -> &'static Vec<String> {
    static S: OnceLock<Vec<String>> = OnceLock::new();
    S.get_or_init(|| {
        [
            "a", "an", "the", "of", "and", "or", "with", "who", "whom", "that", "which", "at",
            "in", "on", "by", "from", "to", "near", "one", "some", "someone", "somebody", "person",
            "people", "man", "woman", "guy", "girl", "boy", "saw", "seen", "was", "is", "his",
            "her", "their", "my", "this", "those", "these", "for", "about", "old", "young",
        ]
        .iter()
        .map(|w| normalize(w))
        .collect()
    })
}

/// Content tokens of a text (normalized, stemmed, stopwords dropped).
pub fn tokens(text: &str) -> Vec<String> {
    let sw = stopwords();
    let mut v: Vec<String> = normalize(text)
        .split_whitespace()
        .filter(|t| !sw.iter().any(|s| s == t))
        .map(String::from)
        .collect();
    v.sort();
    v.dedup();
    v
}

fn token_match(a: &str, b: &str) -> bool {
    a == b || (a.len() >= 4 && b.len() >= 4 && strsim::jaro_winkler(a, b) >= 0.92)
}

/// Name similarity between a normalized mention and a normalized name or alias.
fn name_score(m: &str, cand: &str) -> f32 {
    if m.is_empty() || cand.is_empty() {
        return 0.0;
    }
    let mut s = strsim::jaro_winkler(m, cand) as f32;
    // "Ada" or "Moreau" alone for "Ada Moreau".
    let mt: Vec<&str> = m.split_whitespace().collect();
    let ct: Vec<&str> = cand.split_whitespace().collect();
    if mt.len() <= 2 && ct.len() >= 2 && mt.iter().all(|t| t.len() >= 3 && ct.contains(t)) {
        s = s.max(0.9);
    }
    s
}

fn same_area(w: &World, a: PlaceId, b: PlaceId) -> bool {
    let up = |p: PlaceId| {
        w.places
            .get(p.ix())
            .and_then(|x| x.parent)
            .filter(|q| *q != w.meta.arena)
            .unwrap_or(p)
    };
    a == b || up(a) == up(b)
}

/// Resolve a mention: an existing character by fuzzy name/alias (preferring the same place), else the best cluster by
/// word overlap with its label, traits, needs and group names (plus place), else `None`.
pub fn resolve_mention(w: &World, m: &Mention, near: Option<PlaceId>) -> Resolution {
    if matches!(m.kind.as_str(), "group" | "place" | "item" | "god") {
        return Resolution::None;
    }
    let near = near.or(m.near);
    let mn = normalize(&m.text);
    if mn.is_empty() {
        return Resolution::None;
    }
    let mut best: Option<(f32, CharId)> = None;
    for c in &w.characters {
        let mut s = std::iter::once(&c.name)
            .chain(&c.aliases)
            .map(|n| name_score(&mn, &normalize(n)))
            .fold(0.0, f32::max);
        if near.is_some_and(|p| same_area(w, p, c.place)) {
            s += 0.04;
        }
        if !c.alive {
            s -= 0.02;
        }
        if best.is_none_or(|b| s > b.0) {
            best = Some((s, c.id));
        }
    }
    if let Some((s, id)) = best {
        if s >= EXISTING_MIN {
            return Resolution::Existing {
                char: id,
                score: s.min(1.0),
            };
        }
    }
    best_cluster(w, &tokens(&m.text), near)
        .map(|(cluster, score)| Resolution::Materialize { cluster, score })
        .unwrap_or(Resolution::None)
}

fn best_cluster(w: &World, mt: &[String], near: Option<PlaceId>) -> Option<(ClusterId, f32)> {
    let mut best: Option<(f32, u32, ClusterId)> = None;
    for c in w.clusters.iter().filter(|c| c.size > 0) {
        let mut words = vec![c.label.as_str()];
        words.extend(c.traits.iter().map(String::as_str));
        words.extend(c.needs.iter().map(String::as_str));
        let gnames: Vec<&str> = c
            .groups
            .as_slice()
            .iter()
            .filter_map(|g| w.groups.get(g.ix()))
            .map(|g| g.name.as_str())
            .collect();
        words.extend(gnames);
        let ct = tokens(&words.join(" "));
        let hits = mt
            .iter()
            .filter(|t| ct.iter().any(|c| token_match(t, c)))
            .count();
        let overlap = if mt.is_empty() {
            0.0
        } else {
            hits as f32 / mt.len() as f32
        };
        let here = near.is_some_and(|p| same_area(w, p, c.place));
        if hits == 0 && !here {
            continue;
        }
        let s = if hits == 0 {
            PLACE_ONLY_SCORE
        } else {
            overlap * 0.8 + if here { 0.2 } else { 0.0 }
        };
        let cand = (s, c.size, c.id);
        if best.is_none_or(|b| cand.0 > b.0 || (cand.0 == b.0 && cand.1 > b.1)) {
            best = Some(cand);
        }
    }
    best.map(|b| (b.2, b.0))
}

/// Materialize the cluster's next person. Name and traits come from `Rng::derive(cluster.seed, &[index])`, so the n-th
/// person of a cluster is always the same person. Inherits groups and allegiance, stands inside the cluster's place,
/// keeps the mention text as an alias; the cluster shrinks by one.
pub fn materialize(
    w: &mut World,
    cluster: ClusterId,
    m: &Mention,
    beat: u32,
    by: Option<Ref>,
) -> CharId {
    let t = template();
    let c = w.clusters[cluster.ix()].clone();
    let index = c.materialized;
    let mut rng = Rng::derive(c.seed, &[index as u64]);
    // A family cluster gives its surname (on purpose: kin share it); otherwise the culture of the crowd's home names
    // them, with no full name repeated in the match.
    let fam = c
        .groups
        .as_slice()
        .iter()
        .filter_map(|g| w.groups.get(g.ix()))
        .find(|g| g.kind == GroupKind::Family)
        .and_then(|g| t.names.surnames.iter().find(|s| g.name.contains(s.as_str())));
    // Soldiers are named for where their army was raised, not where it marches.
    let home = c.army.as_ref().and_then(|a| w.clusters.get(a.from.ix())).map(|f| f.place).unwrap_or(c.place);
    let name = crate::names::pick_name(w, home, &mut rng, fam.map(String::as_str));
    let mut traits = crate::util::sample(&mut rng, &c.traits, 2);
    if let Some(q) = rng.pick(&t.names.quirks) {
        traits.push(q.clone());
    }
    let center = w
        .places
        .get(c.place.ix())
        .map(|p| (p.center, p.radius_km))
        .unwrap_or(([0.0, 0.0], 0.0));
    let pos = random_in(&mut rng, center.0, center.1 * 0.8);
    let mut aliases = vec![];
    let txt = m.text.trim();
    if !txt.is_empty() && normalize(txt) != normalize(&name) {
        aliases.push(txt.to_string());
    }
    let id = CharId::from_ix(w.characters.len());
    let mut place = c.place;
    let at = place_at(w, pos);
    if w.places
        .get(at.ix())
        .is_some_and(|p| p.parent == Some(c.place))
    {
        place = at;
    }
    w.characters.push(Character {
        id,
        name,
        aliases,
        origin: Origin::Materialized { cluster, index },
        place,
        pos,
        traits,
        groups: c.groups.clone(),
        health: DEFAULT_MAX_HEALTH,
        max_health: DEFAULT_MAX_HEALTH,
        alive: true,
        allegiance: c.allegiance.clone(),
        serves: None,
        possessed_by: None,
        memorized_beat: beat,
        memorized_by: by,
        tempo: vec![],
        life: None,
    });
    let cl = &mut w.clusters[cluster.ix()];
    cl.size = cl.size.saturating_sub(1);
    cl.materialized += 1;
    id
}

/// Ledger rows a cluster could know (its groups know them, it witnessed them, or it was a target), oldest first.
/// The memory crate seeds a new character's inherited memories from these.
pub fn inheritable_rows(w: &World, cluster: ClusterId) -> Vec<LedgerId> {
    let Some(c) = w.clusters.get(cluster.ix()) else {
        return vec![];
    };
    let me = Ref::Cluster(cluster);
    w.ledger
        .rows
        .iter()
        .filter(|r| {
            r.known_by.intersects(&c.groups)
                || r.witnessed_by.contains(&me)
                || r.targets.contains(&me)
        })
        .map(|r| r.id)
        .collect()
}
