//! Fault lines (rules 0.8.0, Chris's rule): a MEGA cast's effect scales with its distance to the nearest plate
//! boundary. Near a fault (≤ `fault_near_km`) it is much stronger; far from any (≥ `fault_far_km`) it is mild ("sort of
//! a mild volcano"); in between the multipliers are interpolated linearly. Minis are unaffected.
//!
//! The lines are `data/geo/PB2002_boundaries.json` (GeoJSON LineStrings, `[lon, lat]`), compiled in and indexed once
//! per process into a 5° grid of great-circle segments (deterministic: the file's order). Distance is great-circle
//! (R = 6,371 km) to the nearest segment.
//!
//! Credit (required, ODC-BY 1.0): "Plate boundaries: Bird, P. (2002), PB2002; GeoJSON by Hugo Ahlenius, Nordpil
//! (ODC-BY 1.0)."

use std::collections::BTreeMap;
use std::sync::OnceLock;

use fn7_model::{LonLat, PowerDef};
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

/// The compiled index: every segment, and per 5° grid cell the segments whose bounding box touches it.
pub struct FaultIndex {
    segs: Vec<Seg>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
}

fn cell_of(lon: f64, lat: f64) -> (i32, i32) {
    (((lon + 180.0) / CELL_DEG).floor() as i32, ((lat + 90.0) / CELL_DEG).floor() as i32)
}

impl FaultIndex {
    pub fn parse(json: &str) -> FaultIndex {
        let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
        let mut segs = vec![];
        let mut grid: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        for f in v["features"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let pts: Vec<LonLat> = f["geometry"]["coordinates"]
                .as_array()
                .map(|a| a.iter().filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?])).collect())
                .unwrap_or_default();
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
        FaultIndex { segs, grid }
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
}
