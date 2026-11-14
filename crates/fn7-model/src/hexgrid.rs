//! The table's hex grid, exactly as the client indexes it (rules 0.7.0, Benito's people-level occupancy rule).
//!
//! A position on the globe goes to a cube face by gnomonic projection, then to metres on that face's plane, then to a
//! pointy-top axial hex at one of four levels. The client (Unreal) uses the same formulas, so a cell key here is the
//! client's cell key: `"<face>:<level>:<q>:<r>"`.
//!
//! - `LatLonToVec(lat, lon) = (cosLa·cosLo, sinLa, −cosLa·sinLo)`, Y up.
//! - Cube face: `Ax = |x|, Ay = |y|, Az = |z|`. If `Ax ≥ Ay && Ax ≥ Az`: `x > 0` → face 0 (`U = −z/Ax`, `W = y/Ax`),
//!   else face 1 (`U = z/Ax`, `W = y/Ax`). Else if `Ay ≥ Az`: `y > 0` → face 2 (`U = x/Ay`, `W = −z/Ay`), else face 3
//!   (`U = x/Ay`, `W = z/Ay`). Else `z > 0` → face 4 (`U = x/Az`, `W = y/Az`), else face 5 (`U = −x/Az`, `W = y/Az`).
//! - Plane metres: `X = U·R`, `Y = W·R`, `R` = 6,371,000 (real Earth).
//! - Pointy-top axial: `qf = (√3/3·X − Y/3)/S`, `rf = (2/3·Y)/S`, `sf = −qf − rf`; round each; fix the component with
//!   the largest rounding error. `HexToPlane(q, r, S) = (S·√3·(q + r/2), S·1.5·r)`.
//! - Levels (hex size `S` in metres): people 3, houses 30, villages 3,000, regions 120,000.
//!
//! Pure functions, no state. Positions in the game are `[lon, lat]` ([`crate::LonLat`]).

use crate::society::LonLat;

/// Earth's radius on the plane, in metres (both client and server use the real value).
pub const EARTH_R_M: f64 = 6_371_000.0;

/// A hex level of the table and its hex size `S` in metres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HexLevel {
    People,
    Houses,
    Villages,
    Regions,
}

impl HexLevel {
    pub const ALL: [HexLevel; 4] = [HexLevel::People, HexLevel::Houses, HexLevel::Villages, HexLevel::Regions];
    /// Hex size in metres.
    pub fn size_m(self) -> f64 {
        match self {
            HexLevel::People => 3.0,
            HexLevel::Houses => 30.0,
            HexLevel::Villages => 3000.0,
            HexLevel::Regions => 120_000.0,
        }
    }
    /// The level's key word in a cell key.
    pub fn as_str(self) -> &'static str {
        match self {
            HexLevel::People => "people",
            HexLevel::Houses => "houses",
            HexLevel::Villages => "villages",
            HexLevel::Regions => "regions",
        }
    }
    pub fn parse(s: &str) -> Option<HexLevel> {
        HexLevel::ALL.into_iter().find(|l| l.as_str() == s)
    }
}

/// One hex cell: a cube face, a level and axial coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HexCell {
    pub face: u8,
    pub level: HexLevel,
    pub q: i64,
    pub r: i64,
}

impl HexCell {
    /// `"<face>:<level>:<q>:<r>"`.
    pub fn key(&self) -> String {
        format!("{}:{}:{}:{}", self.face, self.level.as_str(), self.q, self.r)
    }
    pub fn parse(key: &str) -> Option<HexCell> {
        let mut it = key.split(':');
        let face: u8 = it.next()?.parse().ok().filter(|f| *f < 6)?;
        let level = HexLevel::parse(it.next()?)?;
        let q = it.next()?.parse().ok()?;
        let r = it.next()?.parse().ok()?;
        it.next().is_none().then_some(HexCell { face, level, q, r })
    }
    /// The six neighbours on the same face and level, in a fixed order (E, NE, NW, W, SW, SE in axial terms).
    pub fn neighbors(&self) -> [HexCell; 6] {
        const D: [(i64, i64); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
        D.map(|(dq, dr)| HexCell { q: self.q + dq, r: self.r + dr, ..*self })
    }
    /// The ring of cells at hex distance `k` (k ≥ 1), in a fixed order starting from the cell `k` steps "SW-W".
    pub fn ring(&self, k: i64) -> Vec<HexCell> {
        if k <= 0 {
            return vec![*self];
        }
        const D: [(i64, i64); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
        let mut out = Vec::with_capacity(6 * k as usize);
        let (mut q, mut r) = (self.q + D[4].0 * k, self.r + D[4].1 * k);
        for d in D {
            for _ in 0..k {
                out.push(HexCell { q, r, ..*self });
                q += d.0;
                r += d.1;
            }
        }
        out
    }
    /// The cell's centre on the globe, `[lon, lat]`.
    pub fn center(&self) -> LonLat {
        let (x, y) = hex_to_plane(self.q, self.r, self.level.size_m());
        plane_to_lonlat(self.face, x, y)
    }
}

/// `LatLonToVec`: a unit vector, Y up.
pub fn lat_lon_to_vec(lat_deg: f64, lon_deg: f64) -> [f64; 3] {
    let (la, lo) = (lat_deg.to_radians(), lon_deg.to_radians());
    [la.cos() * lo.cos(), la.sin(), -la.cos() * lo.sin()]
}

/// The cube face and gnomonic (U, W) of a vector.
pub fn cube_face(v: [f64; 3]) -> (u8, f64, f64) {
    let [x, y, z] = v;
    let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
    if ax >= ay && ax >= az {
        if x > 0.0 {
            (0, -z / ax, y / ax)
        } else {
            (1, z / ax, y / ax)
        }
    } else if ay >= az {
        if y > 0.0 {
            (2, x / ay, -z / ay)
        } else {
            (3, x / ay, z / ay)
        }
    } else if z > 0.0 {
        (4, x / az, y / az)
    } else {
        (5, -x / az, y / az)
    }
}

/// The inverse of [`cube_face`]: a unit vector from a face and its (U, W).
pub fn face_to_vec(face: u8, u: f64, w: f64) -> [f64; 3] {
    let v = match face {
        0 => [1.0, w, -u],
        1 => [-1.0, w, u],
        2 => [u, 1.0, -w],
        3 => [u, -1.0, w],
        4 => [u, w, 1.0],
        _ => [-u, w, -1.0],
    };
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

/// `PlaneToHex(X, Y, S)`: pointy-top axial with cube rounding.
pub fn plane_to_hex(x: f64, y: f64, s: f64) -> (i64, i64) {
    let sqrt3 = 3f64.sqrt();
    let qf = (sqrt3 / 3.0 * x - y / 3.0) / s;
    let rf = (2.0 / 3.0 * y) / s;
    let sf = -qf - rf;
    let (mut q, mut r, s_) = (qf.round(), rf.round(), sf.round());
    let (dq, dr, ds) = ((q - qf).abs(), (r - rf).abs(), (s_ - sf).abs());
    if dq > dr && dq > ds {
        q = -r - s_;
    } else if dr > ds {
        r = -q - s_;
    }
    (q as i64, r as i64)
}

/// `HexToPlane(q, r, S)`: the hex centre in plane metres.
pub fn hex_to_plane(q: i64, r: i64, s: f64) -> (f64, f64) {
    (s * 3f64.sqrt() * (q as f64 + r as f64 / 2.0), s * 1.5 * r as f64)
}

/// A position's face and plane metres.
pub fn lonlat_to_plane(p: LonLat) -> (u8, f64, f64) {
    let (face, u, w) = cube_face(lat_lon_to_vec(p[1], p[0]));
    (face, u * EARTH_R_M, w * EARTH_R_M)
}

/// Plane metres on a face back to `[lon, lat]`.
pub fn plane_to_lonlat(face: u8, x: f64, y: f64) -> LonLat {
    let [vx, vy, vz] = face_to_vec(face, x / EARTH_R_M, y / EARTH_R_M);
    let lat = vy.clamp(-1.0, 1.0).asin().to_degrees();
    let lon = (-vz).atan2(vx).to_degrees();
    [lon, lat]
}

/// The cell holding `p` at `level`.
pub fn cell_at(p: LonLat, level: HexLevel) -> HexCell {
    let (face, x, y) = lonlat_to_plane(p);
    let (q, r) = plane_to_hex(x, y, level.size_m());
    HexCell { face, level, q, r }
}

/// The people-level cell key of a position (what payloads call `cell`).
pub fn people_cell(p: LonLat) -> String {
    cell_at(p, HexLevel::People).key()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_of_the_axes() {
        // lon 0, lat 0 → +x → face 0; lon 180 → −x → face 1; north pole → face 2; south → face 3;
        // lon −90 → z = −cos(0)·sin(−90°) = +1 → face 4; lon 90 → face 5.
        assert_eq!(cell_at([0.0, 0.0], HexLevel::People).key(), "0:people:0:0");
        assert_eq!(cell_at([180.0, 0.0], HexLevel::People).face, 1);
        assert_eq!(cell_at([0.0, 90.0], HexLevel::People).face, 2);
        assert_eq!(cell_at([0.0, -90.0], HexLevel::People).face, 3);
        assert_eq!(cell_at([-90.0, 0.0], HexLevel::People).face, 4);
        assert_eq!(cell_at([90.0, 0.0], HexLevel::People).face, 5);
    }

    /// Known points: the client's cross-check table (computed independently from the same formulas).
    #[test]
    fn known_cells() {
        let table: [(&str, LonLat, [&str; 4]); 4] = [
            ("Paris", [2.3522, 48.8566],
             ["2:people:1044945:50767", "2:houses:104494:5077", "2:villages:1045:51", "2:regions:26:1"]),
            ("New York", [-74.006, 40.7128],
             ["4:people:-282246:1267371", "4:houses:-28225:126737", "4:villages:-282:1267", "4:regions:-7:32"]),
            ("Sydney", [151.2093, -33.8688],
             ["1:people:-131656:-1084278", "1:houses:-13165:-108428", "1:villages:-132:-1084", "1:regions:-3:-27"]),
            ("Tokyo", [139.6917, 35.6895],
             ["1:people:-1706898:1333569", "1:houses:-170690:133357", "1:villages:-1707:1334", "1:regions:-43:34"]),
        ];
        for (name, p, keys) in table {
            for (level, key) in HexLevel::ALL.into_iter().zip(keys) {
                assert_eq!(cell_at(p, level).key(), key, "{name} at {level:?}");
            }
        }
        let (face, x, y) = lonlat_to_plane([2.3522, 48.8566]);
        assert_eq!(face, 2);
        assert!((x - 5_561_591.46).abs() < 0.1 && (y - 228_451.90).abs() < 0.1, "{x} {y}");
        assert_eq!(people_cell([0.0, 0.0]), "0:people:0:0");
        // 10 m along +X on face 0 is two 3 m hexes over (centres are √3·3 = 5.196 m apart).
        let c = cell_at(plane_to_lonlat(0, 10.0, 0.0), HexLevel::People);
        assert_eq!((c.face, c.q, c.r), (0, 2, 0));
    }

    #[test]
    fn plane_round_trip_and_centres() {
        for p in [[2.3522, 48.8566], [-74.006, 40.7128], [151.2093, -33.8688], [139.69, 35.69], [0.0, -89.0]] {
            let (face, x, y) = lonlat_to_plane(p);
            let back = plane_to_lonlat(face, x, y);
            assert!((back[0] - p[0]).abs() < 1e-9 && (back[1] - p[1]).abs() < 1e-9, "{p:?} → {back:?}");
            for level in HexLevel::ALL {
                let c = cell_at(p, level);
                assert_eq!(cell_at(c.center(), level), c, "a cell's centre lies in the cell");
                assert_eq!(HexCell::parse(&c.key()), Some(c));
            }
        }
    }

    #[test]
    fn rounding_fixes_the_largest_error() {
        // Every plane point maps to the hex whose centre is nearest (pointy-top, size 3).
        let s = 3.0;
        for i in 0..400 {
            let x = (i as f64 * 0.731).sin() * 40.0;
            let y = (i as f64 * 1.37).cos() * 40.0;
            let (q, r) = plane_to_hex(x, y, s);
            let (cx, cy) = hex_to_plane(q, r, s);
            let d = (cx - x).hypot(cy - y);
            for (dq, dr) in [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)] {
                let (nx, ny) = hex_to_plane(q + dq, r + dr, s);
                assert!(d <= (nx - x).hypot(ny - y) + 1e-9);
            }
        }
    }

    #[test]
    fn rings() {
        let c = HexCell { face: 0, level: HexLevel::People, q: 0, r: 0 };
        assert_eq!(c.ring(1).len(), 6);
        assert_eq!(c.ring(2).len(), 12);
        let mut n = c.neighbors().to_vec();
        let mut r = c.ring(1);
        n.sort();
        r.sort();
        assert_eq!(n, r);
    }
}
