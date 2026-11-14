//! `[lon, lat]` ↔ toolkit `Point` adapters and small spatial helpers over the world's places.

use std::sync::OnceLock;

use fn7_model::seed::Rng;
use fn7_model::{LonLat, PlaceId, PlaceKind, Ref, World};
use theatre_toolkit::spatial::{self as tk, embedded, Point};

/// Half the Earth's circumference: a planet arena's starting safe radius (everywhere is inside).
pub const PLANET_RADIUS_KM: f64 = 20_037.5;

/// One Natural Earth country, as the planet arena uses it (from the toolkit's embedded 1:110m countries).
#[derive(Debug, Clone)]
pub struct CountryInfo {
    pub name: String,
    pub admin: String,
    pub iso: String,
    /// UN subregion ("Western Africa"): a planet region.
    pub subregion: String,
    pub continent: String,
    pub label: LonLat,
    pub pop: u64,
}

/// Subregions that are never arena regions.
const NOT_REGIONS: &[&str] = &["Seven seas (open ocean)", "Antarctica"];

/// Every country of the embedded Natural Earth data (parsed once).
pub fn countries() -> &'static [CountryInfo] {
    static V: OnceLock<Vec<CountryInfo>> = OnceLock::new();
    V.get_or_init(|| {
        let v: serde_json::Value =
            serde_json::from_str(embedded::COUNTRIES_110M).expect("embedded countries parse");
        let mut out = vec![];
        for f in v["features"].as_array().into_iter().flatten() {
            let p = &f["properties"];
            let s = |k: &str| p[k].as_str().unwrap_or("").trim().to_string();
            let sub = s("SUBREGION");
            if sub.is_empty() || NOT_REGIONS.contains(&sub.as_str()) {
                continue;
            }
            let iso = [s("ISO_A3"), s("ADM0_A3")]
                .into_iter()
                .find(|x| !x.is_empty() && x != "-99")
                .unwrap_or_default();
            out.push(CountryInfo {
                name: s("NAME"),
                admin: s("ADMIN"),
                iso,
                subregion: sub,
                continent: s("CONTINENT"),
                label: [
                    p["LABEL_X"].as_f64().unwrap_or(0.0),
                    p["LABEL_Y"].as_f64().unwrap_or(0.0),
                ],
                pop: p["POP_EST"].as_f64().unwrap_or(0.0).max(0.0) as u64,
            });
        }
        out
    })
}

/// The country (if any) whose polygon contains `p`.
pub fn country_at(p: LonLat) -> Option<&'static CountryInfo> {
    let (name, iso) = embedded::countries().country_at(pt(p))?;
    countries()
        .iter()
        .find(|c| (!iso.is_empty() && c.iso == iso) || c.name == name || c.admin == name)
}

/// A country by name (Natural Earth NAME / ADMIN, case-insensitive).
pub fn country_named(name: &str) -> Option<&'static CountryInfo> {
    let n = name.trim().to_lowercase();
    countries()
        .iter()
        .find(|c| c.name.to_lowercase() == n || c.admin.to_lowercase() == n)
}

/// The planet region (UN subregion) at a point: the country under it, else `None` (open sea).
pub fn subregion_at(p: LonLat) -> Option<&'static str> {
    country_at(p).map(|c| c.subregion.as_str())
}

/// The region (planet) or district (city) a point lies in: on the planet, the region of the country under it, else the
/// region with the nearest center; in a city, the district of the most specific place there.
pub fn region_at(w: &World, p: LonLat) -> PlaceId {
    if w.is_planet() {
        if let Some(sr) = subregion_at(p) {
            if let Some(pl) = w
                .places
                .iter()
                .find(|x| x.kind == PlaceKind::District && x.name == sr)
            {
                return pl.id;
            }
        }
        return nearest_district(w, p).unwrap_or(w.meta.arena);
    }
    district_of(w, place_at(w, p))
}

fn nearest_district(w: &World, p: LonLat) -> Option<PlaceId> {
    w.places
        .iter()
        .filter(|x| x.kind == PlaceKind::District)
        .map(|x| (distance_km(x.center, p), x.id))
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|x| x.1)
}

/// The district a place lies in (itself for a district or the arena).
pub fn district_of(w: &World, p: PlaceId) -> PlaceId {
    let mut cur = p;
    for _ in 0..8 {
        let Some(pl) = w.places.get(cur.ix()) else {
            return w.meta.arena;
        };
        if !matches!(pl.kind, PlaceKind::Site | PlaceKind::Quarter) {
            return cur;
        }
        match pl.parent {
            Some(par) => cur = par,
            None => return cur,
        }
    }
    cur
}

/// Where a ref is on the globe (bodies, crowds and armies, places).
pub fn at_of(w: &World, r: Ref) -> Option<LonLat> {
    pos_of(w, r)
}

pub fn pt(p: LonLat) -> Point {
    Point::new(p[0], p[1])
}

pub fn ll(p: Point) -> LonLat {
    [p.x(), p.y()]
}

pub fn distance_km(a: LonLat, b: LonLat) -> f64 {
    tk::distance_km(pt(a), pt(b))
}

pub fn bearing_deg(from: LonLat, to: LonLat) -> f64 {
    tk::bearing_deg(pt(from), pt(to))
}

pub fn destination(from: LonLat, bearing_deg: f64, km: f64) -> LonLat {
    let p = tk::destination(pt(from), bearing_deg, km.max(0.0));
    [tk::normalize_lon(p.x()), p.y().clamp(-90.0, 90.0)]
}

pub fn on_land(p: LonLat) -> bool {
    embedded::land().contains(pt(p))
}

/// Uniform point in a disc of `radius_km` around `center`.
pub fn random_in(rng: &mut Rng, center: LonLat, radius_km: f64) -> LonLat {
    let b = rng.f64() * 360.0;
    let d = rng.f64().sqrt() * radius_km;
    destination(center, b, d)
}

/// Eight-way compass word for names ("North", "Southeast").
pub fn dir_word(bearing: f64) -> &'static str {
    const W: [&str; 8] = [
        "North",
        "Northeast",
        "East",
        "Southeast",
        "South",
        "Southwest",
        "West",
        "Northwest",
    ];
    W[(((bearing.rem_euclid(360.0) + 22.5) / 45.0) as usize) % 8]
}

/// Terrain prose for a point: the toolkit's terrain plus where it lies relative to `center`.
pub fn prose(p: LonLat, center: Option<LonLat>) -> String {
    let base = tk::terrain(
        embedded::land(),
        embedded::places(),
        pt(p),
        tk::CITY_RADIUS_KM,
    )
    .map(|t| t.to_string())
    .unwrap_or_else(|| tk::describe(embedded::countries(), embedded::land(), pt(p)));
    let ground = if on_land(p) {
        "on land"
    } else {
        "by the water"
    };
    match center {
        Some(c) if distance_km(c, p) > 0.05 => format!(
            "{base}; {:.1} km {} of the center, {ground}",
            distance_km(c, p),
            tk::compass(bearing_deg(c, p))
        ),
        _ => format!("{base}; at the center, {ground}"),
    }
}

/// The most specific place containing `pos` (site, then district), else the nearest district, else the arena. On the
/// planet: a city within its radius, else the region of the country under the point, else the nearest region.
pub fn place_at(w: &World, pos: LonLat) -> PlaceId {
    if w.is_planet() {
        let city = w
            .places
            .iter()
            .filter(|p| p.kind == PlaceKind::Site)
            .map(|p| (distance_km(p.center, pos), p))
            .filter(|(d, p)| *d <= p.radius_km)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        if let Some((_, p)) = city {
            return p.id;
        }
        return region_at(w, pos);
    }
    let mut best: Option<(u8, f64, PlaceId)> = None;
    for p in &w.places {
        let d = distance_km(p.center, pos);
        let rank = match p.kind {
            PlaceKind::Site | PlaceKind::Quarter => 0,
            PlaceKind::District => 1,
            PlaceKind::Arena => 2,
        };
        if d <= p.radius_km {
            let cand = (rank, d, p.id);
            if best.is_none_or(|b| (cand.0, cand.1) < (b.0, b.1)) {
                best = Some(cand);
            }
        }
    }
    if let Some((r, _, id)) = best {
        if r < 2 {
            return id;
        }
    }
    w.places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .map(|p| (distance_km(p.center, pos), p.id))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|x| x.1)
        .unwrap_or(w.meta.arena)
}

/// Current position of something with a body (or a place's center for clusters).
pub fn pos_of(w: &World, r: Ref) -> Option<LonLat> {
    match r {
        Ref::Demigod(d) => w.demigods.get(d.ix()).map(|x| x.pos),
        Ref::Char(c) => w.characters.get(c.ix()).map(|x| x.pos),
        Ref::Cluster(c) => w.clusters.get(c.ix()).and_then(|x| match &x.army {
            Some(a) => Some(a.pos),
            None => w.places.get(x.place.ix()).map(|p| p.center),
        }),
        Ref::Place(p) => w.places.get(p.ix()).map(|p| p.center),
        Ref::Cell(c) => w.cells.get(c.ix()).and_then(|x| pos_of(w, Ref::Cluster(x.crowd))),
        Ref::Mob(m) => w.mobs.get(m.ix()).map(|x| x.pos),
        _ => None,
    }
}

/// The city a place lies in: itself for a city (a planet Site), its parent for a planet city district (Quarter); in a
/// city arena the district (the unit that holds crowds). `None` above that level.
pub fn city_of(w: &World, p: PlaceId) -> Option<PlaceId> {
    let pl = w.places.get(p.ix())?;
    match (w.is_planet(), pl.kind) {
        (true, PlaceKind::Site) => Some(p),
        (true, PlaceKind::Quarter) => pl.parent,
        (false, PlaceKind::District) => Some(p),
        (false, PlaceKind::Site | PlaceKind::Quarter) => pl.parent,
        _ => None,
    }
}
