//! The simulator's grid and its base terrain: every regions-level hex (120 km, `fn7_model::hexgrid`) of the six cube
//! faces, numbered once, with its neighbours (across the faces' seams too) and what the planet is like there before
//! anyone casts anything. Built once per process; a pure function of the geodata (the 1° land mask and the PB2002
//! plate boundaries) and a fixed hash: the same in every match, on every machine, forever.
//!
//! There is no elevation data in the repositories, so the heightmap is generated: land or sea from the mask, height
//! from the distance to the coast, the distance to the nearest plate boundary (mountains and mid-ocean ridges) and
//! two octaves of hashed noise. Sixteen levels: 0..=7 are sea floor (7 = a shelf), 8..=15 land (15 = the high peaks).

use std::sync::OnceLock;

use fn7_model::hexgrid::{cell_at, hex_to_plane, plane_to_lonlat, HexCell, HexLevel, EARTH_R_M};
use fn7_model::LonLat;
use serde_json::Value;

/// The PB2002 plate boundaries (the same file `fn7-plugin::faults` reads).
pub const FAULTS_JSON: &str = include_str!("../../../data/geo/PB2002_boundaries.json");

pub const LAND_MASK: &[u8] = include_bytes!("../../../data/geo/land_mask_1deg.bin");

/// "No cell".
pub const NONE: u16 = u16::MAX;
/// The first land level.
pub const SEA_LEVEL: u8 = 8;
pub const H_MAX: u8 = 15;

/// Base surface materials.
pub const M_WATER: u8 = 0;
pub const M_ROCK: u8 = 1;
pub const M_SOIL: u8 = 2;
pub const M_SAND: u8 = 3;
pub const M_ICE: u8 = 4;

const S: f64 = 120_000.0;
/// The lookup table per face: `q` in −64..64, `r` in −40..40.
const TQ: i64 = 64;
const TR: i64 = 40;
const TW: usize = 128;
const TH: usize = 80;

pub struct Grid {
    pub cells: Vec<HexCell>,
    table: Vec<u16>,
    /// The six neighbours of each cell (a cell's own index where the grid ends: never at a seam, only as a guard).
    pub nb: Vec<[u16; 6]>,
    pub center: Vec<LonLat>,
    pub h: Vec<u8>,
    pub mat: Vec<u8>,
    /// Vegetation that can burn, 0..=120.
    pub fuel: Vec<u8>,
    /// Hops to the nearest cell a plate boundary crosses (0 = on it), at most 15.
    pub fault: Vec<u8>,
    /// Hops to the nearest cell of the other kind (land to sea, sea to land), at most 15.
    pub coast: Vec<u8>,
    /// The neighbour index that lies most to the east, west, north and south.
    pub dirs: Vec<[u8; 4]>,
    /// Every cell a plate boundary crosses, in index order.
    pub fault_cells: Vec<u16>,
}

pub fn is_land(lon: f64, lat: f64) -> bool {
    let x = ((lon + 180.0).floor() as i64).rem_euclid(360);
    let y = ((90.0 - lat).floor() as i64).clamp(0, 179);
    let i = (y * 360 + x) as usize;
    (LAND_MASK[i >> 3] >> (i & 7)) & 1 == 1
}

/// A fixed integer hash (SplitMix64's finalizer).
pub fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn slot(face: u8, q: i64, r: i64) -> Option<usize> {
    ((-TQ..TQ).contains(&q) && (-TR..TR).contains(&r))
        .then(|| face as usize * TW * TH + (r + TR) as usize * TW + (q + TQ) as usize)
}

/// Is this hex part of its face: its centre or one of its corners lies on the face's square.
fn on_face(q: i64, r: i64) -> bool {
    let (x, y) = hex_to_plane(q, r, S);
    let inside = |x: f64, y: f64| x.abs() <= EARTH_R_M && y.abs() <= EARTH_R_M;
    if inside(x, y) {
        return true;
    }
    (0..6).any(|k| {
        let a = (30.0 + 60.0 * k as f64).to_radians();
        inside(x + S * a.cos(), y + S * a.sin())
    })
}

impl Grid {
    fn build() -> Grid {
        let mut cells = vec![];
        let mut table = vec![NONE; 6 * TW * TH];
        for face in 0..6u8 {
            for r in -TR..TR {
                for q in -TQ..TQ {
                    if on_face(q, r) {
                        table[slot(face, q, r).unwrap()] = cells.len() as u16;
                        cells.push(HexCell { face, level: HexLevel::Regions, q, r });
                    }
                }
            }
        }
        let n = cells.len();
        assert!(n < NONE as usize);
        let center: Vec<LonLat> = cells.iter().map(|c| c.center()).collect();
        let mut g = Grid { cells, table, nb: vec![], center, h: vec![0; n], mat: vec![0; n], fuel: vec![0; n],
                           fault: vec![15; n], coast: vec![15; n], dirs: vec![[0; 4]; n], fault_cells: vec![] };
        // Neighbours: on the face where there is one, else the cell that holds the neighbour's centre on the next face.
        let mut nb = vec![[0u16; 6]; n];
        for i in 0..n {
            let c = g.cells[i];
            for (k, x) in c.neighbors().iter().enumerate() {
                let same = slot(x.face, x.q, x.r).map(|s| g.table[s]).unwrap_or(NONE);
                let j = if same != NONE {
                    same
                } else {
                    let (px, py) = hex_to_plane(x.q, x.r, S);
                    g.index_of(plane_to_lonlat(x.face, px, py))
                };
                nb[i][k] = if j == NONE { i as u16 } else { j };
            }
        }
        g.nb = nb;
        // Compass directions.
        for i in 0..n {
            let [lon, lat] = g.center[i];
            let (mut e, mut w, mut no, mut so) = ((f64::MIN, 0u8), (f64::MAX, 0u8), (f64::MIN, 0u8), (f64::MAX, 0u8));
            for k in 0..6u8 {
                let j = g.nb[i][k as usize] as usize;
                if j == i {
                    continue;
                }
                let [lo, la] = g.center[j];
                let mut de = lo - lon;
                if de > 180.0 {
                    de -= 360.0;
                } else if de < -180.0 {
                    de += 360.0;
                }
                let de = de * lat.to_radians().cos().max(0.05);
                let dn = la - lat;
                if de > e.0 {
                    e = (de, k);
                }
                if de < w.0 {
                    w = (de, k);
                }
                if dn > no.0 {
                    no = (dn, k);
                }
                if dn < so.0 {
                    so = (dn, k);
                }
            }
            g.dirs[i] = [e.1, w.1, no.1, so.1];
        }
        // Land or sea: seven samples of the mask.
        let land: Vec<bool> = (0..n)
            .map(|i| {
                let c = g.cells[i];
                let (x, y) = hex_to_plane(c.q, c.r, S);
                let mut hits = is_land(g.center[i][0], g.center[i][1]) as u32;
                for k in 0..6 {
                    let a = (30.0 + 60.0 * k as f64).to_radians();
                    let [lo, la] = plane_to_lonlat(c.face, x + 0.5 * S * a.cos(), y + 0.5 * S * a.sin());
                    hits += is_land(lo, la) as u32;
                }
                hits >= 4
            })
            .collect();
        // Hops to the coast and to the plate boundaries.
        let seeds: Vec<usize> = (0..n).filter(|&i| g.nb[i].iter().any(|&j| land[j as usize] != land[i])).collect();
        g.coast = g.hops(&seeds, 1);
        let mut on_fault = vec![false; n];
        let v: Value = serde_json::from_str(FAULTS_JSON).unwrap_or(Value::Null);
        for f in v["features"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let pts: Vec<LonLat> = f["geometry"]["coordinates"]
                .as_array()
                .map(|a| a.iter().filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?])).collect())
                .unwrap_or_default();
            for w in pts.windows(2) {
                let (p, q) = (w[0], w[1]);
                let mut dlon = q[0] - p[0];
                if dlon > 180.0 {
                    dlon -= 360.0;
                } else if dlon < -180.0 {
                    dlon += 360.0;
                }
                let steps = ((dlon.abs().max((q[1] - p[1]).abs()) / 0.5).ceil() as usize).max(1);
                for s in 0..=steps {
                    let t = s as f64 / steps as f64;
                    let mut lo = p[0] + dlon * t;
                    if lo > 180.0 {
                        lo -= 360.0;
                    } else if lo < -180.0 {
                        lo += 360.0;
                    }
                    let i = g.index_of([lo, (p[1] + (q[1] - p[1]) * t).clamp(-89.99, 89.99)]);
                    if i != NONE {
                        on_fault[i as usize] = true;
                    }
                }
            }
        }
        let seeds: Vec<usize> = (0..n).filter(|&i| on_fault[i]).collect();
        g.fault_cells = seeds.iter().map(|&i| i as u16).collect();
        g.fault = g.hops(&seeds, 0);
        // Two octaves of hashed noise: the cell's own hash, and the mean of its neighbourhood's.
        let own: Vec<u32> = (0..n).map(|i| {
            let c = g.cells[i];
            (mix(c.face as u64 ^ ((c.q as u64) << 8) ^ ((c.r as u64) << 32) ^ 0xEA27) & 0xFF) as u32
        }).collect();
        let smooth: Vec<u32> = (0..n).map(|i| (own[i] * 2 + g.nb[i].iter().map(|&j| own[j as usize]).sum::<u32>()) / 8).collect();
        let smooth: Vec<u32> = (0..n).map(|i| (smooth[i] * 2 + g.nb[i].iter().map(|&j| smooth[j as usize]).sum::<u32>()) / 8).collect();
        for i in 0..n {
            let [lon, lat] = g.center[i];
            let (coast, fault, sm, own) = (g.coast[i] as i32, g.fault[i] as i32, smooth[i] as i32, own[i] as i32);
            if land[i] {
                let polar = lat <= -62.0 || lat >= 72.0 || (lat >= 60.0 && (-60.0..=-20.0).contains(&lon));
                let mut h = 8 + (coast - 1).clamp(0, 2);
                h += match fault {
                    0 | 1 => 3,
                    2 => 1,
                    _ => 0,
                };
                h += (sm > 135) as i32 + (sm > 150) as i32 + polar as i32;
                let h = h.clamp(8, 15) as u8;
                let desert = (15.0..=32.0).contains(&lat.abs()) && coast >= 2 && sm < 140;
                let mat = if polar {
                    M_ICE
                } else if h >= 13 {
                    M_ROCK
                } else if desert {
                    M_SAND
                } else {
                    M_SOIL
                };
                let fuel = match mat {
                    M_ICE => 0,
                    M_ROCK => 16,
                    M_SAND => 6,
                    _ => {
                        let a = lat.abs();
                        let band = if a < 12.0 {
                            104
                        } else if a < 25.0 {
                            72
                        } else if a < 40.0 {
                            60
                        } else if a < 62.0 {
                            96
                        } else {
                            30
                        };
                        (band + (own % 32) - 16).clamp(0, 120)
                    }
                };
                g.h[i] = h;
                g.mat[i] = mat;
                g.fuel[i] = fuel as u8;
            } else {
                let mut h = match coast {
                    0 | 1 => 7,
                    2 => 5,
                    3 => 3,
                    _ => 2,
                };
                h += if fault == 0 { 2 } else { 0 };
                h += (sm > 150) as i32 - (sm < 105) as i32;
                g.h[i] = h.clamp(0, 7) as u8;
                g.mat[i] = M_WATER;
            }
        }
        g
    }

    /// Breadth-first hops from `seeds` (which start at `first`), at most 15.
    fn hops(&self, seeds: &[usize], first: u8) -> Vec<u8> {
        let mut d = vec![15u8; self.cells.len()];
        let mut q: std::collections::VecDeque<usize> = seeds.iter().copied().collect();
        for &s in seeds {
            d[s] = first;
        }
        while let Some(i) = q.pop_front() {
            if d[i] >= 15 {
                continue;
            }
            for &j in &self.nb[i] {
                let j = j as usize;
                if d[j] > d[i] + 1 {
                    d[j] = d[i] + 1;
                    q.push_back(j);
                }
            }
        }
        d
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The index of a regions-level hex ([`NONE`] if it is not on its face).
    pub fn index(&self, c: &HexCell) -> u16 {
        if c.level != HexLevel::Regions {
            return NONE;
        }
        slot(c.face, c.q, c.r).map(|s| self.table[s]).unwrap_or(NONE)
    }

    /// The cell a point of the globe lies in.
    pub fn index_of(&self, p: LonLat) -> u16 {
        let c = cell_at(p, HexLevel::Regions);
        let i = self.index(&c);
        if i != NONE {
            return i;
        }
        c.neighbors().iter().map(|x| self.index(x)).find(|&j| j != NONE).unwrap_or(NONE)
    }

    pub fn key(&self, i: u16) -> String {
        self.cells[i as usize].key()
    }

    pub fn land(&self, i: u16) -> bool {
        self.h[i as usize] >= SEA_LEVEL
    }

    /// The cells within `r` hops of `c` with their distance, nearest first (ties in discovery order): `c` itself first.
    pub fn disc(&self, c: u16, r: u8) -> Vec<(u16, u8)> {
        let mut out = vec![(c, 0u8)];
        let mut k = 0;
        while k < out.len() {
            let (i, d) = out[k];
            k += 1;
            if d >= r {
                continue;
            }
            for &j in &self.nb[i as usize] {
                if !out.iter().any(|x| x.0 == j) {
                    out.push((j, d + 1));
                }
            }
        }
        out
    }
}

pub fn grid() -> &'static Grid {
    static G: OnceLock<Grid> = OnceLock::new();
    G.get_or_init(Grid::build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_covers_the_globe_and_knows_the_earth() {
        let g = grid();
        assert!(g.len() > 24_000 && g.len() < 30_000, "{} cells", g.len());
        // Every point of the globe has a cell, and it is the client's cell.
        for lat in (-89..=89).step_by(2) {
            for lon in (-180..180).step_by(3) {
                let p = [lon as f64 + 0.37, lat as f64 + 0.21];
                let i = g.index_of(p);
                assert_ne!(i, NONE, "{p:?}");
                let c = cell_at(p, HexLevel::Regions);
                if g.index(&c) != NONE {
                    assert_eq!(g.key(i), c.key());
                }
            }
        }
        // Neighbours exist everywhere (seams included) and are mostly mutual.
        let mut one_way = 0;
        for i in 0..g.len() {
            for &j in &g.nb[i] {
                assert!((j as usize) < g.len());
                if j as usize != i && !g.nb[j as usize].contains(&(i as u16)) {
                    one_way += 1;
                }
            }
        }
        // (At the faces' seams the two faces' hexes do not line up: a few links there are one-way.)
        assert!(one_way < g.len() / 5, "{one_way} one-way links");
        // Land where there is land, sea where there is sea, mountains on the plate boundaries.
        let at = |lon: f64, lat: f64| g.index_of([lon, lat]) as usize;
        for (name, lon, lat, land) in [("Sahara", 10.0, 23.0, true), ("Amazon", -60.0, -5.0, true), ("Siberia", 100.0, 62.0, true),
                                        ("mid Pacific", -150.0, 0.0, false), ("mid Atlantic", -35.0, 30.0, false),
                                        ("Antarctica", 60.0, -80.0, true), ("Indian Ocean", 80.0, -30.0, false)] {
            assert_eq!(g.h[at(lon, lat)] >= SEA_LEVEL, land, "{name}");
        }
        assert_eq!(g.mat[at(10.0, 23.0)], M_SAND);
        assert_eq!(g.mat[at(60.0, -80.0)], M_ICE);
        assert!(g.fuel[at(-60.0, -5.0)] > 80, "the Amazon burns");
        let andes = g.disc(at(-68.5, -20.0) as u16, 2).iter().map(|x| g.h[x.0 as usize]).max().unwrap();
        assert!(andes >= 11, "the Andes: {andes}");
        assert!(g.fault[at(143.5, 38.0)] <= 1 && g.fault[at(134.0, -24.0)] >= 5, "Japan trench near, central Australia far");
        let land = (0..g.len()).filter(|&i| g.h[i] >= SEA_LEVEL).count() as f64 / g.len() as f64;
        assert!((0.22..0.42).contains(&land), "land share {land}");
        assert!(g.fault_cells.len() > 800 && g.fault_cells.len() < 6000, "{} fault cells", g.fault_cells.len());
        // Stable forever: the base terrain's fingerprint.
        let mut hsh = 0u64;
        for i in 0..g.len() {
            hsh = mix(hsh ^ ((g.h[i] as u64) << 24 | (g.mat[i] as u64) << 16 | (g.fuel[i] as u64) << 8 | g.fault[i] as u64));
        }
        eprintln!("grid: {} cells, land {:.3}, fault cells {}, fingerprint {hsh:016x}", g.len(), land, g.fault_cells.len());
    }
}
