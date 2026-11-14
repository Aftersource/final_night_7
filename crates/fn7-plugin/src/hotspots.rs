//! Hotspots (rules 0.7.0): per region, the most dynamic area as a circle, by rules, from the state alone.
//!
//! Weighted points in the region: public events of the last [`WINDOW`] beats (weight 1 + magnitude/3), battles of the
//! last [`WINDOW`] beats (4), live mobs (2 + size/2,000, at most 6), crowds in unrest (anger ≥ 0.6: 1 + 2 × anger) and
//! demigods present (3). The hotspot is the densest spot: the point whose neighbourhood of radius `r` (a quarter of the
//! region's radius, 25–400 km; a city arena: 0.3–3 km) holds the most weight; its centre is the weighted mean of the
//! points there, its radius their weighted spread (at least `r / 2`), its `score` that weight, and `why` the kinds that
//! made it, heaviest first. Deterministic (ties: the lower point index). No hotspot in a region with no weight.
//!
//! The client opens zoom level 2 centred on it when the globe is clicked; built-in AI demigods drift toward the nearest
//! one ([`crate::actions::bot_lines`]).

use std::collections::BTreeMap;

use fn7_model::*;
use fn7_society::geo::{distance_km, region_at};
use serde::Serialize;
use serde_json::{json, Value};

/// Beats of history a hotspot reads.
pub const WINDOW: u32 = 5;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hotspot {
    pub region: PlaceId,
    pub pos: LonLat,
    pub radius_km: f64,
    pub score: f64,
    pub why: Vec<String>,
}

impl Hotspot {
    pub fn to_json(&self) -> Value {
        json!({ "pos": self.pos, "radius_km": (self.radius_km * 10.0).round() / 10.0,
                "score": (self.score * 100.0).round() / 100.0, "why": self.why })
    }
}

struct Pt {
    at: LonLat,
    weight: f64,
    kind: &'static str,
}

/// Every region's hotspot (regions with nothing going on have none), keyed by region. `rows` are recent ledger rows
/// (any superset of the last [`WINDOW`] beats; the rest is ignored).
pub fn hotspots(w: &World, rows: &[LedgerRow]) -> BTreeMap<PlaceId, Hotspot> {
    let now = w.meta.beat;
    let since = now.saturating_sub(WINDOW - 1);
    let mut by_region: BTreeMap<PlaceId, Vec<Pt>> = BTreeMap::new();
    let mut add = |at: LonLat, weight: f64, kind: &'static str| {
        if weight > 0.0 && at[0].is_finite() && at[1].is_finite() {
            by_region.entry(region_at(w, at)).or_default().push(Pt { at, weight, kind });
        }
    };
    for r in rows.iter().filter(|r| r.beat >= since && crate::payload::is_public_event(w, r)) {
        if let (Some(at), _) = crate::payload::geotag(w, r) {
            add(at, 1.0 + r.magnitude as f64 / 3.0, "events");
        }
    }
    for b in w.battles.iter().filter(|b| b.beat >= since) {
        add(b.at, 4.0, "battles");
    }
    for m in w.mobs.iter().filter(|m| m.ended.is_none() && m.size > 0) {
        add(m.pos, (2.0 + m.size as f64 / 2000.0).min(6.0), "mobs");
    }
    for c in w.clusters.iter().filter(|c| c.army.is_none() && c.size > 0 && c.mood.anger >= 0.6) {
        if let Some(at) = fn7_society::geo::pos_of(w, Ref::Cluster(c.id)) {
            add(at, 1.0 + 2.0 * c.mood.anger as f64, "unrest");
        }
    }
    for d in w.demigods.iter().filter(|d| d.alive) {
        add(d.pos, 3.0, "demigods");
    }
    let mut out = BTreeMap::new();
    for (region, pts) in by_region {
        let rr = w.places.get(region.ix()).map(|p| p.radius_km).unwrap_or(500.0);
        let r = if w.is_planet() { (rr / 4.0).clamp(25.0, 400.0) } else { (rr / 4.0).clamp(0.3, 3.0) };
        let density = |p: &Pt| pts.iter().filter(|q| distance_km(p.at, q.at) <= r).map(|q| q.weight).sum::<f64>();
        let Some((best, _)) = pts
            .iter()
            .enumerate()
            .map(|(i, p)| (i, density(p)))
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        else {
            continue;
        };
        let near: Vec<&Pt> = pts.iter().filter(|q| distance_km(pts[best].at, q.at) <= r).collect();
        let total: f64 = near.iter().map(|q| q.weight).sum();
        // Weighted mean (lon unwrapped around the best point, so the antimeridian doesn't split a hotspot).
        let base = pts[best].at;
        let (mut lon, mut lat) = (0.0, 0.0);
        for q in &near {
            let mut dl = q.at[0] - base[0];
            if dl > 180.0 {
                dl -= 360.0;
            } else if dl < -180.0 {
                dl += 360.0;
            }
            lon += (base[0] + dl) * q.weight;
            lat += q.at[1] * q.weight;
        }
        let mut pos = [lon / total, lat / total];
        if pos[0] > 180.0 {
            pos[0] -= 360.0;
        } else if pos[0] < -180.0 {
            pos[0] += 360.0;
        }
        let spread = (near.iter().map(|q| q.weight * distance_km(pos, q.at).powi(2)).sum::<f64>() / total).sqrt();
        let mut kinds: BTreeMap<&str, f64> = BTreeMap::new();
        for q in &near {
            *kinds.entry(q.kind).or_default() += q.weight;
        }
        let mut why: Vec<(&str, f64)> = kinds.into_iter().collect();
        why.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
        out.insert(
            region,
            Hotspot {
                region,
                pos,
                radius_km: spread.max(r / 2.0),
                score: total,
                why: why.into_iter().map(|x| x.0.to_string()).collect(),
            },
        );
    }
    out
}

/// The recent rows of a world's ledger a hotspot reads (the tail back to `WINDOW` beats ago).
pub fn recent_rows(w: &World) -> &[LedgerRow] {
    let since = w.meta.beat.saturating_sub(WINDOW - 1);
    let n = w.ledger.rows.len();
    let start = w.ledger.rows.iter().rposition(|r| r.beat < since).map_or(0, |i| i + 1).min(n);
    &w.ledger.rows[start..]
}

/// The hotspot nearest to `at` within `max_km`, if any.
pub fn nearest(spots: &BTreeMap<PlaceId, Hotspot>, at: LonLat, max_km: f64) -> Option<&Hotspot> {
    spots
        .values()
        .map(|h| (distance_km(h.pos, at), h))
        .filter(|(d, _)| *d <= max_km)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.region.cmp(&b.1.region)))
        .map(|x| x.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotspots_sit_on_the_action_and_are_deterministic() {
        let w = crate::testkit::world(4, 21);
        let a = hotspots(&w, recent_rows(&w));
        assert!(!a.is_empty(), "the demigods' landing makes action somewhere");
        assert_eq!(a, hotspots(&w, recent_rows(&w)));
        // Every demigod's region has a hotspot, within reach of it.
        for d in w.demigods.iter() {
            let h = a.get(&region_at(&w, d.pos)).expect("a hotspot where a demigod stands");
            assert!(h.score >= 3.0 && h.radius_km > 0.0 && !h.why.is_empty());
        }
        let v = a.values().next().unwrap().to_json();
        assert!(v["pos"].is_array() && v["radius_km"].is_number() && v["why"].is_array());
    }
}
