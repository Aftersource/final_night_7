//! Fault lines (rules 0.8.0, Chris's rule): a MEGA cast's effect scales with its distance to the nearest plate
//! boundary. Near a fault (≤ `fault_near_km`) it is much stronger; far from any (≥ `fault_far_km`) it is mild ("sort of
//! a mild volcano"); in between the multipliers are interpolated linearly. Minis are unaffected.
//!
//! The lines are `data/geo/PB2002_boundaries.json` (GeoJSON LineStrings, `[lon, lat]`), compiled in and indexed once
//! per process into a 5° grid of great-circle segments (deterministic: the file's order). Distance is great-circle
//! (R = 6,371 km) to the nearest segment.
//!
//! State schema 16 (junctions and tiers, DM_RULEBOOK §13): the index also finds the plate-boundary **junctions** once,
//! deterministically: triple junctions (polyline ends within 30 km whose boundaries touch 3+ different plates, merged
//! within 150 km) and "knots" (dense, twisting stretches of boundary far from any triple junction), each with a
//! strength 0..1; the big ones are named from a small table by location. A mega within `fault_junction_km` of one gets
//! an extra multiplier on top of the distance curve, and every mega lands in a **tier** (`junction`, `fault`, `near`,
//! `mild`, `tiny`) that also sets how much the world answers (ramifications, lasting mods: a tiny one stays local).
//! `game.fault` and `game.faults_overview` let the client preview it under the cursor while a mega is armed.
//!
//! Credit (required, ODC-BY 1.0): "Plate boundaries: Bird, P. (2002), PB2002; GeoJSON by Hugo Ahlenius, Nordpil
//! (ODC-BY 1.0)."

use std::collections::BTreeMap;
use std::sync::OnceLock;

use fn7_model::{LonLat, PowerDef, World};
use serde_json::{json, Value};

use crate::settings::Settings;

pub const FAULTS_JSON: &str = include_str!("../../../data/geo/PB2002_boundaries.json");
pub const CREDIT: &str =
    "Plate boundaries: Bird, P. (2002), PB2002; GeoJSON by Hugo Ahlenius, Nordpil (ODC-BY 1.0).";

const R_KM: f64 = 6371.0;
/// Grid cell size of the index, degrees.
const CELL_DEG: f64 = 5.0;

type V3 = [f64; 3];

fn v3(p: LonLat) -> V3 {
    let (lo, la) = (p[0].to_radians(), p[1].to_radians());
    [la.cos() * lo.cos(), la.cos() * lo.sin(), la.sin()]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn angle(a: V3, b: V3) -> f64 {
    norm(cross(a, b)).atan2(dot(a, b))
}

/// One great-circle segment.
struct Seg {
    a: V3,
    b: V3,
}

impl Seg {
    /// Great-circle distance (km) from `p` to this segment.
    fn distance_km(&self, p: V3) -> f64 {
        let n = cross(self.a, self.b);
        let nn = norm(n);
        let ends = angle(p, self.a).min(angle(p, self.b));
        if nn < 1e-12 {
            return ends * R_KM;
        }
        let n = [n[0] / nn, n[1] / nn, n[2] / nn];
        let s = dot(p, n);
        let c = [p[0] - s * n[0], p[1] - s * n[1], p[2] - s * n[2]];
        let cn = norm(c);
        if cn < 1e-12 {
            return ends * R_KM;
        }
        let c = [c[0] / cn, c[1] / cn, c[2] / cn];
        let within = (angle(self.a, c) + angle(c, self.b) - angle(self.a, self.b)).abs() < 1e-9;
        if within {
            s.clamp(-1.0, 1.0).asin().abs() * R_KM
        } else {
            ends * R_KM
        }
    }
}

/// The compiled index: every segment, per 5° grid cell the segments whose bounding box touches it, and (schema 16) the
/// junctions, strongest first.
pub struct FaultIndex {
    segs: Vec<Seg>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
    junctions: Vec<Junction>,
}

fn cell_of(lon: f64, lat: f64) -> (i32, i32) {
    (((lon + 180.0) / CELL_DEG).floor() as i32, ((lat + 90.0) / CELL_DEG).floor() as i32)
}

impl FaultIndex {
    pub fn parse(json: &str) -> FaultIndex {
        let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
        let mut segs = vec![];
        let mut grid: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        let mut lines: Vec<Line> = vec![];
        for f in v["features"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let pts: Vec<LonLat> = f["geometry"]["coordinates"]
                .as_array()
                .map(|a| a.iter().filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?])).collect())
                .unwrap_or_default();
            let plate = |k: &str| f["properties"][k].as_str().unwrap_or("").to_string();
            lines.push(Line { plates: [plate("PlateA"), plate("PlateB")], pts: pts.clone() });
            for w in pts.windows(2) {
                let (p, q) = (w[0], w[1]);
                let id = segs.len() as u32;
                segs.push(Seg { a: v3(p), b: v3(q) });
                // Segments crossing the antimeridian are short: index both ends' cells.
                let (lo0, lo1) = if (p[0] - q[0]).abs() > 180.0 { (p[0], p[0]) } else { (p[0].min(q[0]), p[0].max(q[0])) };
                let (c0, c1) = (cell_of(lo0, p[1].min(q[1])), cell_of(lo1, p[1].max(q[1])));
                for x in c0.0..=c1.0 {
                    for y in c0.1..=c1.1 {
                        grid.entry((x, y)).or_default().push(id);
                    }
                }
                if (p[0] - q[0]).abs() > 180.0 {
                    grid.entry(cell_of(q[0], q[1])).or_default().push(id);
                }
            }
        }
        let junctions = find_junctions(&lines);
        FaultIndex { segs, grid, junctions }
    }

    /// Schema 16: the junctions, strongest first.
    pub fn junctions(&self) -> &[Junction] {
        &self.junctions
    }

    /// Schema 16: the junction whose pull on `p` is strongest within `radius_km` (pull = strength × (1 − km / radius),
    /// ties to the nearer, then the stronger): `(junction, km, pull)`.
    pub fn junction_near(&self, p: LonLat, radius_km: f64) -> Option<(&Junction, f64, f64)> {
        if radius_km <= 0.0 {
            return None;
        }
        let pv = v3(p);
        let mut best: Option<(&Junction, f64, f64)> = None;
        for j in &self.junctions {
            let km = angle(pv, j.v) * R_KM;
            if km >= radius_km {
                continue;
            }
            let pull = j.strength * (1.0 - km / radius_km);
            if best.is_none_or(|(_, bk, bp)| pull > bp || (pull == bp && km < bk)) {
                best = Some((j, km, pull));
            }
        }
        best
    }

    pub fn len(&self) -> usize {
        self.segs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    /// Great-circle distance (km) from `p` to the nearest fault segment. Looks in the grid cells within `cap_km` first;
    /// with none there, checks every segment.
    pub fn distance_km(&self, p: LonLat, cap_km: f64) -> f64 {
        let pv = v3(p);
        let dlat = cap_km / 111.0;
        let coslat = p[1].to_radians().cos().max(0.05);
        let dlon = (cap_km / (111.0 * coslat)).min(180.0);
        let (c0, c1) = (cell_of(p[0] - dlon, (p[1] - dlat).max(-90.0)), cell_of(p[0] + dlon, (p[1] + dlat).min(89.999)));
        let ncols = (360.0 / CELL_DEG) as i32;
        let mut best = f64::INFINITY;
        for x in c0.0..=c1.0 {
            for y in c0.1..=c1.1 {
                let key = (x.rem_euclid(ncols), y);
                for &i in self.grid.get(&key).map(Vec::as_slice).unwrap_or(&[]) {
                    best = best.min(self.segs[i as usize].distance_km(pv));
                }
            }
        }
        if best > cap_km {
            for s in &self.segs {
                best = best.min(s.distance_km(pv));
            }
        }
        best
    }
}

/// The index, compiled once per process.
pub fn index() -> &'static FaultIndex {
    static I: OnceLock<FaultIndex> = OnceLock::new();
    I.get_or_init(|| FaultIndex::parse(FAULTS_JSON))
}

/// How a mega cast at a distance `km` from the nearest fault scales: `(t, magnitude delta, radius ×, damage ×)`,
/// `t` = 0 at or inside `fault_near_km`, 1 at or beyond `fault_far_km`.
pub fn scaling(km: f64, st: &Settings) -> (f64, f32, f32, f32) {
    let (near, far) = (st.fault_near_km as f64, (st.fault_far_km as f64).max(st.fault_near_km as f64 + 1.0));
    let t = ((km - near) / (far - near)).clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32| a + (b - a) * t as f32;
    (
        t,
        lerp(st.fault_near_magnitude, st.fault_far_magnitude),
        lerp(st.fault_near_radius, st.fault_far_radius),
        lerp(st.fault_near_damage, st.fault_far_damage),
    )
}

/// Apply the fault rule to a mega power landing at `at`: returns the power as it lands and the public fields
/// `{fault_km, fault_factor}` (`fault_factor` = its damage multiplier).
pub fn apply(p: PowerDef, at: LonLat, st: &Settings) -> (PowerDef, Value) {
    let km = index().distance_km(at, st.fault_far_km as f64);
    let (_, dm, rm, xm) = scaling(km, st);
    let magnitude = (p.magnitude as f32 + dm).round().clamp(1.0, 10.0) as u8;
    let radius_km = (p.radius_km * rm * 10.0).round() / 10.0;
    let damage = (p.damage * xm * 10.0).round() / 10.0;
    let fields = json!({ "fault_km": km.round(), "fault_factor": (xm as f64 * 100.0).round() / 100.0 });
    (PowerDef { magnitude, radius_km, damage, ..p }, fields)
}

// ---------------------------------------------------------------------------------------------------------------------
// Schema 16: junctions, tiers and the hover preview
// ---------------------------------------------------------------------------------------------------------------------

/// The state schema junctions and tiers switch on at (with the room setting `fault_junctions_enabled`). Kept here, not
/// in `fn7_model::dm`, so the branches that take 13..15 merge without touching it.
pub const JUNCTION_SCHEMA: u32 = 16;

/// Polyline ends this close (km) meet: a boundary touching 3+ plates there is a triple junction.
const MEET_KM: f64 = 30.0;
/// Triple junctions this close (km) are one (e.g. the Galápagos microplate's three, around one Pacific-Cocos-Nazca).
const MERGE_KM: f64 = 150.0;
/// Boundary length is summed within this radius (km) for the density part of a strength.
const DENSITY_KM: f64 = 300.0;
/// Boundary is cut into pieces at most this long (km) to measure density.
const PIECE_KM: f64 = 25.0;
/// A knot: at least this much boundary (km) within `DENSITY_KM`, ...
const KNOT_MIN_KM: f64 = 1500.0;
/// ... at least this far (km) from every triple junction and from a stronger knot.
const KNOT_APART_KM: f64 = 400.0;
const KNOT_GAP_KM: f64 = 500.0;
/// A knot's strength tops out here (the densest knot): below the great triple junctions.
const KNOT_MAX_STRENGTH: f64 = 0.6;
/// A table name goes to the nearest triple junction within this many km.
const NAME_KM: f64 = 250.0;

/// The named junctions, by location (`[lon, lat]`): the nearest detected triple junction within 250 km takes the name.
pub const NAMES: &[(&str, LonLat)] = &[
    ("Afar triple junction", [42.0, 11.5]),
    ("Boso triple junction", [142.0, 34.0]),
    ("Azores triple junction", [-29.5, 39.2]),
    ("Galápagos triple junction", [-101.5, 2.2]),
    ("Mendocino triple junction", [-124.4, 40.4]),
    ("Rodrigues triple junction", [70.0, -25.5]),
    ("Bouvet triple junction", [-0.4, -54.8]),
    ("Macquarie triple junction", [161.0, -61.5]),
    ("Chile triple junction", [-75.7, -46.4]),
    ("Juan Fernández triple junction", [-111.0, -35.0]),
    ("Easter triple junction", [-113.0, -26.8]),
    ("Rivera triple junction", [-105.6, 18.0]),
    ("Queen Charlotte triple junction", [-130.9, 51.6]),
    ("Kamchatka–Aleutian junction", [164.0, 55.5]),
    ("Karlıova triple junction", [41.0, 39.3]),
    ("Solomon Islands junction", [156.3, -8.2]),
    ("Maraş triple junction", [36.5, 37.0]),
];

/// One boundary polyline as read (its two plates and its points).
struct Line {
    plates: [String; 2],
    pts: Vec<LonLat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JunctionKind {
    /// Boundaries of 3+ plates meet.
    Triple,
    /// A dense, twisting stretch of boundary (high density and curvature) far from any triple junction.
    Knot,
}

/// A plate-boundary junction.
#[derive(Debug, Clone)]
pub struct Junction {
    /// `[lon, lat]`, a vertex of the data.
    pub at: LonLat,
    v: V3,
    pub kind: JunctionKind,
    /// The plates meeting (PB2002 codes, sorted); empty for a knot.
    pub plates: Vec<String>,
    /// 0..1 (2 decimals): the strongest triple junction is 1.
    pub strength: f64,
    pub name: Option<&'static str>,
}

impl Junction {
    /// The public face: `{name?, kind, at, strength, plates}`.
    pub fn json(&self) -> Value {
        let mut v = json!({ "kind": self.kind, "at": self.at, "strength": self.strength, "plates": self.plates });
        if let Some(n) = self.name {
            v["name"] = n.into();
        }
        v
    }

    /// How narration names it: "at the Afar triple junction", "at a triple junction of four plates", "in a knot of
    /// fault lines".
    pub fn phrase(&self) -> String {
        match (self.name, self.kind) {
            (Some(n), _) => format!("at the {n}"),
            (None, JunctionKind::Triple) if self.plates.len() >= 4 => {
                format!("at a junction of {} plates", ["", "", "", "three", "four", "five"].get(self.plates.len()).copied().unwrap_or("many"))
            }
            (None, JunctionKind::Triple) => "at a triple junction".into(),
            (None, JunctionKind::Knot) => "in a knot of fault lines".into(),
        }
    }
}

fn km_between(a: V3, b: V3) -> f64 {
    angle(a, b) * R_KM
}

fn lonlat_of(v: V3) -> LonLat {
    [v[1].atan2(v[0]).to_degrees(), v[2].clamp(-1.0, 1.0).asin().to_degrees()]
}

/// Boundary cut into short pieces (centre, length km), bucketed by 5° cell, to sum boundary length near a point.
struct Pieces {
    all: Vec<(V3, f64)>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
}

impl Pieces {
    fn new(lines: &[Line]) -> Pieces {
        let (mut all, mut grid) = (vec![], BTreeMap::<(i32, i32), Vec<u32>>::new());
        for l in lines {
            for w in l.pts.windows(2) {
                let (a, b) = (v3(w[0]), v3(w[1]));
                let len = km_between(a, b);
                let n = (len / PIECE_KM).ceil().max(1.0) as usize;
                for i in 0..n {
                    let t = (i as f64 + 0.5) / n as f64;
                    let m = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
                    let mn = norm(m).max(1e-12);
                    let m = [m[0] / mn, m[1] / mn, m[2] / mn];
                    let ll = lonlat_of(m);
                    grid.entry(cell_of(ll[0], ll[1].min(89.999))).or_default().push(all.len() as u32);
                    all.push((m, len / n as f64));
                }
            }
        }
        Pieces { all, grid }
    }

    /// Boundary length (km) within `DENSITY_KM` of `p`.
    fn density(&self, p: V3) -> f64 {
        let ll = lonlat_of(p);
        let dlat = DENSITY_KM / 111.0;
        let dlon = (DENSITY_KM / (111.0 * ll[1].to_radians().cos().max(0.05))).min(180.0);
        let (c0, c1) = (cell_of(ll[0] - dlon, (ll[1] - dlat).max(-90.0)), cell_of(ll[0] + dlon, (ll[1] + dlat).min(89.999)));
        let ncols = (360.0 / CELL_DEG) as i32;
        let cos_r = (DENSITY_KM / R_KM).cos();
        let mut seen = std::collections::BTreeSet::new();
        let mut sum = 0.0;
        for x in c0.0..=c1.0 {
            for y in c0.1..=c1.1 {
                let key = (x.rem_euclid(ncols), y);
                if !seen.insert(key) {
                    continue;
                }
                for &i in self.grid.get(&key).map(Vec::as_slice).unwrap_or(&[]) {
                    let (q, len) = self.all[i as usize];
                    if dot(p, q) >= cos_r {
                        sum += len;
                    }
                }
            }
        }
        sum
    }
}

/// Find the junctions (once per process, deterministic: the file's order, then sorted strongest first).
fn find_junctions(lines: &[Line]) -> Vec<Junction> {
    // A plate's size, by proxy: the length of its boundary (km). Weight = sqrt(share of the longest).
    let mut perimeter: BTreeMap<&str, f64> = BTreeMap::new();
    for l in lines {
        let len: f64 = l.pts.windows(2).map(|w| km_between(v3(w[0]), v3(w[1]))).sum();
        for p in &l.plates {
            *perimeter.entry(p.as_str()).or_default() += len;
        }
    }
    let longest = perimeter.values().copied().fold(1.0, f64::max);
    let weight = |p: &str| (perimeter.get(p).copied().unwrap_or(0.0) / longest).sqrt();
    // How big the meeting is: the mean weight of its three biggest plates, + 0.1 per plate past three.
    let size = |plates: &[String]| {
        let mut ws: Vec<f64> = plates.iter().map(|p| weight(p)).collect();
        ws.sort_by(|a, b| b.total_cmp(a));
        ws.iter().take(3).sum::<f64>() / 3.0 + 0.1 * ws.len().saturating_sub(3) as f64
    };

    // 1. Polyline ends that meet (greedy, file order): 3+ plates = a triple junction candidate.
    let ends: Vec<(LonLat, usize)> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.pts.len() >= 2)
        .flat_map(|(i, l)| [(l.pts[0], i), (l.pts[l.pts.len() - 1], i)])
        .collect();
    let mut used = vec![false; ends.len()];
    let mut cands: Vec<(LonLat, Vec<String>)> = vec![];
    for a in 0..ends.len() {
        if used[a] {
            continue;
        }
        used[a] = true;
        let va = v3(ends[a].0);
        let mut plates: Vec<String> = lines[ends[a].1].plates.to_vec();
        for b in a + 1..ends.len() {
            if !used[b] && km_between(va, v3(ends[b].0)) < MEET_KM {
                used[b] = true;
                plates.extend(lines[ends[b].1].plates.iter().cloned());
            }
        }
        plates.retain(|p| !p.is_empty());
        plates.sort();
        plates.dedup();
        if plates.len() >= 3 {
            cands.push((ends[a].0, plates));
        }
    }
    // 2. Merge candidates chained within MERGE_KM of each other (union-find); the biggest member's point (ties: file
    //    order) stands for the group, with every member's plates.
    let mut parent: Vec<usize> = (0..cands.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..cands.len() {
        for j in i + 1..cands.len() {
            if km_between(v3(cands[i].0), v3(cands[j].0)) < MERGE_KM {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by(|&i, &j| size(&cands[j].1).total_cmp(&size(&cands[i].1)).then(i.cmp(&j)));
    let mut groups: Vec<(usize, LonLat, Vec<String>)> = vec![];
    for i in order {
        let r = root(&mut parent, i);
        match groups.iter_mut().find(|g| g.0 == r) {
            Some(g) => {
                g.2.extend(cands[i].1.iter().cloned());
                g.2.sort();
                g.2.dedup();
            }
            None => groups.push((r, cands[i].0, cands[i].1.clone())),
        }
    }
    let merged: Vec<(LonLat, Vec<String>)> = groups.into_iter().map(|g| (g.1, g.2)).collect();
    // 3. Strength: 3/4 the size of the meeting, 1/4 the boundary density around it; the strongest is 1.
    let pieces = Pieces::new(lines);
    let dens: Vec<f64> = merged.iter().map(|m| pieces.density(v3(m.0))).collect();
    let dmax = dens.iter().copied().fold(1.0, f64::max);
    let raw: Vec<f64> = merged.iter().zip(&dens).map(|(m, d)| 0.75 * size(&m.1) + 0.25 * d / dmax).collect();
    let rmax = raw.iter().copied().fold(1e-9, f64::max);
    let round2 = |x: f64| (x * 100.0).round() / 100.0;
    let mut out: Vec<Junction> = merged
        .into_iter()
        .zip(raw)
        .map(|((at, plates), r)| Junction { at, v: v3(at), kind: JunctionKind::Triple, plates, strength: round2(r / rmax), name: None })
        .collect();
    // 4. Knots: the densest boundary (every 4th piece sampled), apart from every triple junction and stronger knot.
    let samples: Vec<(f64, usize)> = (0..pieces.all.len()).step_by(4).map(|i| (pieces.density(pieces.all[i].0), i)).collect();
    let smax = samples.iter().map(|s| s.0).fold(1.0, f64::max);
    let mut by_density = samples.clone();
    by_density.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut knots: Vec<Junction> = vec![];
    for (d, i) in by_density {
        if d < KNOT_MIN_KM {
            break;
        }
        let v = pieces.all[i].0;
        if out.iter().any(|j| km_between(j.v, v) < KNOT_APART_KM) || knots.iter().any(|k| km_between(k.v, v) < KNOT_GAP_KM) {
            continue;
        }
        let ll = lonlat_of(v);
        let at = [(ll[0] * 1e4).round() / 1e4, (ll[1] * 1e4).round() / 1e4];
        knots.push(Junction { at, v: v3(at), kind: JunctionKind::Knot, plates: vec![], strength: round2(KNOT_MAX_STRENGTH * d / smax), name: None });
    }
    out.extend(knots);
    out.sort_by(|a, b| b.strength.total_cmp(&a.strength).then(a.at[0].total_cmp(&b.at[0])).then(a.at[1].total_cmp(&b.at[1])));
    // 5. Names: each table entry to the nearest triple junction within NAME_KM not yet named.
    for (name, at) in NAMES {
        let p = v3(*at);
        let best = out
            .iter()
            .enumerate()
            .filter(|(_, j)| j.kind == JunctionKind::Triple && j.name.is_none())
            .map(|(i, j)| (km_between(p, j.v), i))
            .filter(|(km, _)| *km < NAME_KM)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        if let Some((_, i)) = best {
            out[i].name = Some(name);
        }
    }
    out
}

/// A mega's tier: `junction` (within `fault_junction_km` of a junction), `fault` (≤ `fault_near_km`), `near` (the
/// first half of the curve), `mild` (the second half), `tiny` (≥ `fault_far_km`).
pub fn tier(km: f64, at_junction: bool, st: &Settings) -> &'static str {
    let (t, ..) = scaling(km, st);
    if at_junction {
        "junction"
    } else if km <= st.fault_near_km as f64 {
        "fault"
    } else if t < 0.5 {
        "near"
    } else if km < st.fault_far_km as f64 {
        "mild"
    } else {
        "tiny"
    }
}

/// The tiers in order, with the distance each ends at (km; `tiny` has none).
pub fn tier_thresholds(st: &Settings) -> Value {
    let (near, far) = (st.fault_near_km as f64, (st.fault_far_km as f64).max(st.fault_near_km as f64 + 1.0));
    let mid = (near + far) / 2.0;
    json!([
        { "tier": "junction", "within_km_of_junction": st.fault_junction_km },
        { "tier": "fault", "max_km": near },
        { "tier": "near", "max_km": mid },
        { "tier": "mild", "max_km": far },
        { "tier": "tiny", "min_km": far },
    ])
}

/// The ramifications' weight of a mega in a tier (the old rule counts every mega as 3): a junction 4, a fault or near
/// one 3, a mild one 2, a tiny one 1.
pub fn ramify_weight(tier: &str) -> u32 {
    match tier {
        "junction" => 4,
        "fault" | "near" => 3,
        "mild" => 2,
        _ => 1,
    }
}

/// Where a mega lands in the schema-16 rules: distance, tier, the junction pulling on it, and the multipliers.
#[derive(Debug, Clone)]
pub struct Assessment {
    pub km: f64,
    pub tier: &'static str,
    /// `(junction, km, pull)`; `None` without the junction rule or outside every junction's reach.
    pub junction: Option<(Junction, f64, f64)>,
    pub magnitude: f32,
    pub radius: f32,
    pub damage: f32,
}

impl Assessment {
    /// `{name?, kind, at, strength, plates, km}` or null.
    pub fn junction_json(&self) -> Value {
        match &self.junction {
            Some((j, km, _)) => {
                let mut v = j.json();
                v["km"] = json!(km.round());
                v
            }
            None => Value::Null,
        }
    }
}

/// Assess a mega at `at`. `junctions`: the junction rule is on (schema 16 and the room setting).
pub fn assess(at: LonLat, st: &Settings, junctions: bool) -> Assessment {
    let i = index();
    let km = i.distance_km(at, st.fault_far_km as f64);
    let (_, mut dm, mut rm, mut xm) = scaling(km, st);
    let junction = if junctions {
        i.junction_near(at, st.fault_junction_km as f64).map(|(j, k, p)| (j.clone(), k, p))
    } else {
        None
    };
    if let Some((_, _, pull)) = &junction {
        let k = *pull as f32;
        dm += st.fault_junction_magnitude * k;
        rm *= 1.0 + (st.fault_junction_radius - 1.0) * k;
        xm *= 1.0 + (st.fault_junction_damage - 1.0) * k;
    }
    let tier = tier(km, junction.is_some(), st);
    Assessment { km, tier, junction, magnitude: dm, radius: rm, damage: xm }
}

/// Is the schema-16 fault rule (junctions and tiers) on in this world?
pub fn junction_rules(w: &World, st: &Settings) -> bool {
    w.meta.schema >= JUNCTION_SCHEMA && st.fault_junctions_enabled
}

/// Schema 16's [`apply`]: the power as it lands and the public fields `{fault_km, fault_factor, fault_tier, junction}`
/// (`fault_factor` = the whole damage multiplier, the junction's included).
pub fn apply_tiers(p: PowerDef, at: LonLat, st: &Settings) -> (PowerDef, Value, Assessment) {
    let a = assess(at, st, true);
    let magnitude = (p.magnitude as f32 + a.magnitude).round().clamp(1.0, 10.0) as u8;
    let radius_km = (p.radius_km * a.radius * 10.0).round() / 10.0;
    let damage = (p.damage * a.damage * 10.0).round() / 10.0;
    let fields = json!({ "fault_km": a.km.round(), "fault_factor": (a.damage as f64 * 100.0).round() / 100.0,
                         "fault_tier": a.tier, "junction": a.junction_json() });
    (PowerDef { magnitude, radius_km, damage, ..p }, fields, a)
}

/// `[lon, lat]` from the query args: `at` as `[lon, lat]` or `"lon,lat"` (HTTP), or `lon` and `lat`.
fn at_arg(args: &Value) -> Result<LonLat, String> {
    let num = |v: &Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
    let p = match args.get("at") {
        Some(Value::Array(a)) if a.len() == 2 => num(&a[0]).zip(num(&a[1])).map(|(x, y)| [x, y]),
        Some(Value::String(s)) => {
            let mut it = s.trim().trim_start_matches('[').trim_end_matches(']').split(',').map(|x| x.trim().parse::<f64>().ok());
            match (it.next().flatten(), it.next().flatten(), it.next()) {
                (Some(x), Some(y), None) => Some([x, y]),
                _ => None,
            }
        }
        None | Some(Value::Null) => args.get("lon").and_then(num).zip(args.get("lat").and_then(num)).map(|(x, y)| [x, y]),
        _ => None,
    };
    match p {
        Some([lon, lat]) if lon.is_finite() && lat.is_finite() && (-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat) => Ok([lon, lat]),
        Some(_) => Err("`at` must be [lon, lat] with lon in -180..180 and lat in -90..90".into()),
        None => Err("`at` is required: [lon, lat] (over HTTP `at=lon,lat`, or `lon` and `lat`)".into()),
    }
}

/// The match's settings and whether its fault rules include junctions (read from `meta` only: cheap).
fn rules_of(state: &Value) -> (Settings, bool, bool) {
    let st = Settings::from_value(&state["meta"]["settings"]);
    let schema = state["meta"]["schema"].as_u64().unwrap_or(0) as u32;
    let faults = schema >= fn7_model::dm::DM_SCHEMA;
    let junctions = schema >= JUNCTION_SCHEMA && st.fault_junctions_enabled;
    (st, faults, junctions)
}

/// `game.fault {at}`: what a mega aimed at `at` would get (read-only, deterministic): `{at, km, tier, junction,
/// factor: {magnitude, radius, damage}, junctions}`.
pub fn query_fault(state: &Value, args: &Value) -> Result<Value, String> {
    let at = at_arg(args)?;
    let (st, faults, junctions) = rules_of(state);
    let a = assess(at, &st, junctions);
    let factor = if faults {
        json!({ "magnitude": (a.magnitude as f64 * 100.0).round() / 100.0, "radius": (a.radius as f64 * 100.0).round() / 100.0,
                "damage": (a.damage as f64 * 100.0).round() / 100.0 })
    } else {
        json!({ "magnitude": 0, "radius": 1, "damage": 1 })
    };
    Ok(json!({ "at": at, "km": a.km.round(), "tier": a.tier, "junction": a.junction_json(), "factor": factor,
               "junctions": junctions }))
}

/// `game.faults_overview`: every junction (`{lon, lat, name?, kind, strength, plates}`, strongest first), the tier
/// thresholds and the curve's settings, for the globe overlay. Static per match.
pub fn query_overview(state: &Value) -> Result<Value, String> {
    let (st, _, junctions) = rules_of(state);
    let list: Vec<Value> = index()
        .junctions()
        .iter()
        .map(|j| {
            let mut v = json!({ "lon": j.at[0], "lat": j.at[1], "kind": j.kind, "strength": j.strength, "plates": j.plates });
            if let Some(n) = j.name {
                v["name"] = n.into();
            }
            v
        })
        .collect();
    Ok(json!({
        "junctions_rule": junctions,
        "junction_km": st.fault_junction_km,
        "tiers": tier_thresholds(&st),
        "factors": {
            "near": { "km": st.fault_near_km, "magnitude": st.fault_near_magnitude, "radius": st.fault_near_radius, "damage": st.fault_near_damage },
            "far": { "km": st.fault_far_km, "magnitude": st.fault_far_magnitude, "radius": st.fault_far_radius, "damage": st.fault_far_damage },
            "junction": { "km": st.fault_junction_km, "magnitude": st.fault_junction_magnitude, "radius": st.fault_junction_radius, "damage": st.fault_junction_damage },
        },
        "junctions": list,
        "credit": CREDIT,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_compiles_and_measures_great_circle_distance() {
        let i = index();
        assert!(i.len() > 1000, "{} segments", i.len());
        // On the San Andreas and the Japan trench: near. Central Australia: far.
        let sa = i.distance_km([-120.5, 35.9], 1500.0);
        let jp = i.distance_km([143.5, 38.0], 1500.0);
        let au = i.distance_km([134.0, -24.0], 1500.0);
        assert!(sa < 150.0 && jp < 150.0, "San Andreas {sa}, Japan {jp}");
        assert!(au > 1500.0, "central Australia {au}");
        // The grid agrees with a full scan.
        for p in [[-120.5, 35.9], [10.0, 45.0], [-30.0, 0.0], [179.9, -20.0], [0.0, 89.0]] {
            let full = i.segs.iter().map(|s| s.distance_km(v3(p))).fold(f64::INFINITY, f64::min);
            assert!((i.distance_km(p, 1500.0) - full).abs() < 1e-6, "{p:?}");
        }
    }

    #[test]
    fn near_is_stronger_than_far() {
        let st = Settings::default();
        let p = PowerDef {
            slug: "volcano".into(), name: "Volcano".into(), icon: "volcano".into(), cost: 1, radius_km: 1000.0,
            target: fn7_model::PowerTarget::Point, magnitude: 7, damage: 3.0, tags: vec![], blurb: String::new(),
            mega: None, mini: None,
        };
        let (sa, fa) = apply(p.clone(), [-120.5, 35.9], &st);
        let (jp, fj) = apply(p.clone(), [143.5, 38.0], &st);
        let (au, fu) = apply(p.clone(), [134.0, -24.0], &st);
        for x in [&sa, &jp] {
            assert!(x.damage > au.damage && x.magnitude > au.magnitude && x.radius_km > au.radius_km);
        }
        assert_eq!((sa.magnitude, sa.damage, sa.radius_km), (10, 4.8, 1500.0));
        assert_eq!((au.magnitude, au.damage), (5, 1.8));
        assert_eq!(fa["fault_factor"], 1.6);
        assert_eq!(fu["fault_factor"], 0.6);
        assert!(fj["fault_km"].as_f64().unwrap() < 150.0);
        // In between: interpolated.
        let (t, _, _, x) = scaling(825.0, &st);
        assert!((t - 0.5).abs() < 1e-9 && (x - 1.1).abs() < 1e-6);
    }

    /// The live house curve (near 200 km: +5, ×2.2, ×2.5; far 1,000 km: −4, ×0.4, ×0.25) and the junction defaults.
    fn house() -> Settings {
        Settings::from_value(&Settings::validate(&json!({
            "fault_near_km": 200, "fault_far_km": 1000, "fault_near_magnitude": 5, "fault_far_magnitude": -4,
            "fault_near_radius": 2.2, "fault_far_radius": 0.4, "fault_near_damage": 2.5, "fault_far_damage": 0.25,
        })).unwrap())
    }

    fn named(name: &str) -> Junction {
        index().junctions().iter().find(|j| j.name == Some(name)).unwrap_or_else(|| panic!("no {name}")).clone()
    }

    #[test]
    fn known_triple_junctions_are_found_and_named() {
        let afar = named("Afar triple junction");
        assert!(km_between(afar.v, v3([42.0, 11.5])) < 50.0, "{:?}", afar.at);
        assert_eq!(afar.plates, ["AF", "AR", "SO"]);
        let boso = named("Boso triple junction");
        assert!(km_between(boso.v, v3([142.0, 34.0])) < 50.0, "{:?}", boso.at);
        assert_eq!(boso.plates, ["OK", "PA", "PS"]);
        // The Galápagos microplate's three junctions are one, of four plates.
        assert_eq!(named("Galápagos triple junction").plates, ["CO", "GP", "NZ", "PA"]);
        for n in ["Azores triple junction", "Mendocino triple junction", "Macquarie triple junction", "Rodrigues triple junction"] {
            assert_eq!(named(n).kind, JunctionKind::Triple);
        }
        let js = index().junctions();
        assert!(js.len() > 60 && js.len() < 150, "{}", js.len());
        assert_eq!(js[0].strength, 1.0);
        assert!(js.windows(2).all(|w| w[0].strength >= w[1].strength));
        assert!(js.iter().all(|j| (0.0..=1.0).contains(&j.strength)));
        assert!(js.iter().any(|j| j.kind == JunctionKind::Knot));
        assert!(js.iter().filter(|j| j.kind == JunctionKind::Knot).all(|j| j.strength <= KNOT_MAX_STRENGTH && j.name.is_none()));
        // Nothing near central Australia.
        assert!(index().junction_near([134.0, -24.0], 300.0).is_none());
    }

    #[test]
    fn junctions_are_deterministic() {
        let a = FaultIndex::parse(FAULTS_JSON);
        let b = FaultIndex::parse(FAULTS_JSON);
        let key = |i: &FaultIndex| i.junctions().iter().map(|j| format!("{:?}", (j.at, j.kind, &j.plates, j.strength, j.name))).collect::<Vec<_>>();
        assert_eq!(key(&a), key(&b));
        assert_eq!(key(&a), key(index()));
    }

    #[test]
    fn tiers_follow_the_distance_and_the_junctions() {
        let st = house();
        assert_eq!(tier(50.0, true, &st), "junction");
        assert_eq!(tier(150.0, false, &st), "fault");
        assert_eq!(tier(200.0, false, &st), "fault");
        assert_eq!(tier(500.0, false, &st), "near");
        assert_eq!(tier(700.0, false, &st), "mild");
        assert_eq!(tier(1000.0, false, &st), "tiny");
        assert_eq!(tier(3000.0, false, &st), "tiny");
        // At real places.
        assert_eq!(assess([42.0, 11.5], &st, true).tier, "junction");
        assert_eq!(assess([42.0, 11.5], &st, false).tier, "fault");
        assert_eq!(assess([134.0, -24.0], &st, true).tier, "tiny");
        assert_eq!(assess([-120.5, 35.9], &st, true).tier, "fault");
        assert_eq!([ramify_weight("junction"), ramify_weight("fault"), ramify_weight("near"), ramify_weight("mild"), ramify_weight("tiny")], [4, 3, 3, 2, 1]);
        let t = tier_thresholds(&st);
        assert_eq!(t[2], json!({ "tier": "near", "max_km": 600.0 }));
    }

    #[test]
    fn a_junction_multiplies_on_top_of_the_curve() {
        let st = house();
        let afar = named("Afar triple junction");
        // At its very point: the near curve times the full junction pull (= its strength).
        let a = assess(afar.at, &st, true);
        let (j, km, pull) = a.junction.clone().unwrap();
        assert_eq!(j.name, Some("Afar triple junction"));
        assert!(km < 1.0 && (pull - afar.strength).abs() < 1e-3, "{km} {pull}");
        let s = afar.strength as f32;
        assert!((a.magnitude - (5.0 + 3.0 * s)).abs() < 0.01, "{}", a.magnitude);
        assert!((a.radius - 2.2 * (1.0 + 0.5 * s)).abs() < 0.01, "{}", a.radius);
        assert!((a.damage - 2.5 * (1.0 + 0.8 * s)).abs() < 0.01, "{}", a.damage);
        // Halfway to the edge: half the pull; the edge: none.
        let half = assess(fn7_society::geo::destination(afar.at, 200.0, 150.0), &st, true);
        if let Some((j2, _, p2)) = &half.junction {
            assert!(j2.name != afar.name || (p2 - afar.strength * 0.5).abs() < 0.02, "{p2}");
        }
        // Without the rule: the plain curve.
        let off = assess(afar.at, &st, false);
        assert!(off.junction.is_none() && off.damage == 2.5 && off.magnitude == 5.0);
        // A volcano at Afar vs the same in central Australia (tiny).
        let p = PowerDef {
            slug: "volcano".into(), name: "Volcano".into(), icon: "volcano".into(), cost: 1, radius_km: 1000.0,
            target: fn7_model::PowerTarget::Point, magnitude: 7, damage: 3.0, tags: vec![], blurb: String::new(),
            mega: None, mini: None,
        };
        let (big, f, _) = apply_tiers(p.clone(), afar.at, &st);
        let (tiny, g, _) = apply_tiers(p.clone(), [134.0, -24.0], &st);
        assert_eq!(big.magnitude, 10);
        assert!(big.damage > 11.0 && big.radius_km > 2800.0, "{big:?}");
        assert_eq!((tiny.magnitude, tiny.damage, tiny.radius_km), (3, 0.8, 400.0));
        assert_eq!(f["fault_tier"], "junction");
        assert_eq!(f["junction"]["name"], "Afar triple junction");
        assert_eq!(g["fault_tier"], "tiny");
        assert!(g["junction"].is_null());
        // The old rule is untouched.
        let (old, o) = apply(p, afar.at, &st);
        assert_eq!((old.magnitude, old.damage), (10, 7.5));
        assert!(o.get("fault_tier").is_none());
    }

    #[test]
    fn the_queries_preview_without_touching_the_state() {
        let st = Settings::validate(&json!({ "fault_near_km": 200, "fault_far_km": 1000, "fault_near_damage": 2.5 })).unwrap();
        let state = json!({ "meta": { "schema": JUNCTION_SCHEMA, "settings": st } });
        let before = state.clone();
        let q = query_fault(&state, &json!({ "at": [42.0, 11.5] })).unwrap();
        assert_eq!(q["tier"], "junction");
        assert_eq!(q["junction"]["name"], "Afar triple junction");
        assert_eq!(q["junctions"], true);
        assert!(q["factor"]["damage"].as_f64().unwrap() > 2.5);
        // HTTP-style args agree.
        assert_eq!(query_fault(&state, &json!({ "at": "42,11.5" })).unwrap(), q);
        assert_eq!(query_fault(&state, &json!({ "lon": "42", "lat": "11.5" })).unwrap(), q);
        let far = query_fault(&state, &json!({ "at": [134.0, -24.0] })).unwrap();
        assert_eq!((far["tier"].as_str(), far["junction"].is_null()), (Some("tiny"), true));
        assert!(query_fault(&state, &json!({})).is_err());
        assert!(query_fault(&state, &json!({ "at": [200.0, 0.0] })).is_err());
        // An older match: the plain curve, no junction.
        let old = json!({ "meta": { "schema": 12, "settings": st } });
        let o = query_fault(&old, &json!({ "at": [42.0, 11.5] })).unwrap();
        assert_eq!((o["junctions"].as_bool(), o["junction"].is_null(), o["factor"]["damage"].as_f64()), (Some(false), true, Some(2.5)));
        let ov = query_overview(&state).unwrap();
        assert_eq!(ov["junctions"].as_array().unwrap().len(), index().junctions().len());
        assert_eq!(ov["junctions"][0]["strength"], 1.0);
        assert!(ov["junctions"].as_array().unwrap().iter().any(|j| j["name"] == "Boso triple junction"));
        assert_eq!(ov["tiers"].as_array().unwrap().len(), 5);
        assert_eq!(query_overview(&state).unwrap(), ov);
        assert_eq!(state, before);
    }
}
