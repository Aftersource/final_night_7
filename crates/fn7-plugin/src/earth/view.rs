//! What clients get of the Earth: the per-beat `earth` object (`delta`, `phenomena`), the queries `game.earth` and
//! `game.earth_at`, the summary in `present`, finer zoom levels derived from the coarse truth, and an ASCII map.
//!
//! Cell columns travel as base64 arrays (docs/EARTH_SIM.md "The client contract"): `cells` is little-endian u16
//! indices into the regions grid, every other column one byte per cell in the same order. JSON number arrays are
//! available with `format: "json"` for debugging; they are 3–4 times the size.

use fn7_model::earth::{EarthState, Phenomenon};
use fn7_model::hexgrid::{cell_at, HexCell, HexLevel};
use fn7_model::LonLat;
use serde_json::{json, Map, Value};

use super::codec::b64;
use super::grid::{grid, mix, M_ICE, M_ROCK, M_SAND, M_SOIL, NONE, SEA_LEVEL};
use super::rules::{cfg, EarthBeat};
use super::sim::*;
use crate::settings::Settings;

/// The layers a client is sent, in this order (`fuel` is what can burn now, not the stored change).
pub const CLIENT_LAYERS: [&str; 14] =
    ["water", "lava", "ash", "fire", "snow", "cloud", "charge", "heat", "crack", "ruin", "fuel", "swarm", "shake", "plume"];

fn client_layer(sim: &Sim, i: u16, k: usize) -> u8 {
    let c = sim.c(i);
    match k {
        0 => c.water,
        1 => c.lava,
        2 => c.ash,
        3 => c.fire,
        4 => c.snow,
        5 => c.cloud,
        6 => c.charge,
        7 => c.heat as u8,
        8 => c.crack,
        9 => c.ruin,
        10 => sim.fuel(i),
        11 => c.swarm,
        12 => c.shake,
        _ => c.plume,
    }
}

fn round5(x: f64) -> f64 {
    (x * 1e5).round() / 1e5
}

fn at_json(p: LonLat) -> Value {
    json!([round5(p[0]), round5(p[1])])
}

/// How cell columns are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enc {
    /// One base64 block, cell after cell (the default: the smallest). See [`pack_cells`].
    Pack,
    /// A base64 array per column: `cells` u16 little-endian, every other column one byte per cell.
    Columns,
    /// JSON number arrays (for reading by eye).
    Json,
}

impl Enc {
    pub fn parse(a: &Value) -> Result<Enc, String> {
        match a.get("format").and_then(Value::as_str) {
            None | Some("pack") => Ok(Enc::Pack),
            Some("b64") | Some("columns") => Ok(Enc::Columns),
            Some("json") => Ok(Enc::Json),
            Some(x) => Err(format!("`format` must be pack, columns or json, not `{x}`")),
        }
    }
}

/// The packed block: for each cell, in index order,
///
/// ```text
/// varint  index − the previous cell's index (the first: the index itself)
/// byte    height, 0..=15
/// byte    top material (an index into `tops`)
/// varint  mask: bit k set = layer k of `layers` is not zero
/// bytes   that layer's value, for each bit set, lowest first (`heat` is a signed byte)
/// ```
///
/// (varints are LEB128: 7 bits a byte, low bits first, the high bit set on every byte but the last).
pub fn pack_cells(sim: &Sim, cells: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(cells.len() * 8);
    let put = |out: &mut Vec<u8>, mut x: u32| loop {
        let b = (x & 0x7F) as u8;
        x >>= 7;
        if x == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    };
    let mut last = 0u32;
    for &i in cells {
        put(&mut out, i as u32 - last);
        last = i as u32;
        out.push(sim.height(i));
        out.push(sim.top(i));
        let vals: Vec<u8> = (0..CLIENT_LAYERS.len()).map(|k| client_layer(sim, i, k)).collect();
        let mask = vals.iter().enumerate().fold(0u32, |m, (k, v)| if *v != 0 { m | 1 << k } else { m });
        put(&mut out, mask);
        out.extend(vals.iter().filter(|v| **v != 0));
    }
    out
}

/// Read a packed block back: `(index, height, top, layers)` per cell (what a client does).
pub fn unpack_cells(b: &[u8]) -> Vec<(u16, u8, u8, [u8; CLIENT_LAYERS.len()])> {
    let get = |at: &mut usize| -> Option<u32> {
        let (mut x, mut shift) = (0u32, 0);
        loop {
            let v = *b.get(*at)?;
            *at += 1;
            x |= ((v & 0x7F) as u32) << shift;
            if v & 0x80 == 0 {
                return Some(x);
            }
            shift += 7;
        }
    };
    let (mut at, mut i, mut out) = (0usize, 0u32, vec![]);
    while at < b.len() {
        let Some(d) = get(&mut at) else { break };
        i += d;
        let (Some(&h), Some(&top)) = (b.get(at), b.get(at + 1)) else { break };
        at += 2;
        let Some(mask) = get(&mut at) else { break };
        let mut layers = [0u8; CLIENT_LAYERS.len()];
        for (k, v) in layers.iter_mut().enumerate() {
            if mask & (1 << k) != 0 {
                *v = b.get(at).copied().unwrap_or(0);
                at += 1;
            }
        }
        out.push((i as u16, h, top, layers));
    }
    out
}

/// `cells` (sorted by index) for a client: `{n, enc, ...}`.
///
/// - `enc: "pack"`: `data`, one base64 block ([`pack_cells`]).
/// - `enc: "columns"`: `cells` (u16 LE), `h`, `top` and `layers{name}` (one byte per cell; a layer that is zero on
///   every cell is left out), each base64.
/// - `enc: "json"`: the same columns as number arrays (`heat` signed).
pub fn columns(sim: &Sim, cells: &[u16], enc: Enc) -> Value {
    let n = cells.len();
    if enc == Enc::Pack {
        return json!({ "n": n, "enc": "pack", "data": b64(&pack_cells(sim, cells)) });
    }
    let json_arrays = enc == Enc::Json;
    let h: Vec<u8> = cells.iter().map(|&i| sim.height(i)).collect();
    let top: Vec<u8> = cells.iter().map(|&i| sim.top(i)).collect();
    let mut layers = Map::new();
    for (k, name) in CLIENT_LAYERS.iter().enumerate() {
        let col: Vec<u8> = cells.iter().map(|&i| client_layer(sim, i, k)).collect();
        if col.iter().all(|&x| x == 0) {
            continue;
        }
        layers.insert((*name).into(), if json_arrays {
            if *name == "heat" { json!(col.iter().map(|&x| x as i8).collect::<Vec<_>>()) } else { json!(col) }
        } else {
            b64(&col).into()
        });
    }
    if json_arrays {
        json!({ "n": n, "enc": "json", "cells": cells, "h": h, "top": top, "layers": layers })
    } else {
        let idx: Vec<u8> = cells.iter().flat_map(|i| i.to_le_bytes()).collect();
        json!({ "n": n, "enc": "columns", "cells": b64(&idx), "h": b64(&h), "top": b64(&top), "layers": layers })
    }
}

/// What a client sees of one cell, coarsely: its height, its top material and the high four bits of every layer.
fn seen(sim: &Sim, i: u16) -> [u8; 2 + CLIENT_LAYERS.len()] {
    let mut v = [0u8; 2 + CLIENT_LAYERS.len()];
    v[0] = sim.height(i);
    v[1] = sim.top(i);
    for k in 0..CLIENT_LAYERS.len() {
        v[2 + k] = client_layer(sim, i, k) >> 4;
    }
    v
}

/// The cells a client must redraw: those whose height, top material, or any layer by a sixteenth of its range (its
/// high four bits) differ between the field `before` and the field now. Sorted. A slow fade (ruin healing a point a
/// beat) is sent once every 16 beats, not every beat; a client's copy of a layer is never more than 15/255 stale, and
/// its heights and top materials are exact.
pub fn changed(before: &Sim, sim: &Sim) -> Vec<u16> {
    let (a, b) = (&before.list, &sim.list);
    let (mut x, mut y, mut out) = (0, 0, vec![]);
    while x < a.len() || y < b.len() {
        let ia = a.get(x).map(|v| *v as u32).unwrap_or(u32::MAX);
        let ib = b.get(y).map(|v| *v as u32).unwrap_or(u32::MAX);
        let i = ia.min(ib) as u16;
        if ia == i as u32 {
            x += 1;
        }
        if ib == i as u32 {
            y += 1;
        }
        if seen(before, i) != seen(sim, i) {
            out.push(i);
        }
    }
    out
}

/// One phenomenon for a client: where it is, how big, how strong, how old, where it is going, whose it is and why.
pub fn phenomenon_json(sim: &Sim, p: &Phenomenon) -> Value {
    let g = sim.grid();
    let center = g.center[p.cell as usize];
    let origin = [p.lon_e3 as f64 / 1000.0, p.lat_e3 as f64 / 1000.0];
    let mini = p.tier == TIER_MINI;
    // A mini is far smaller than a cell: it is drawn where it was cast.
    let at = if mini { origin } else { center };
    let mut v = json!({
        "id": p.id,
        "kind": kind_name(p.kind),
        "at": at_json(at),
        "cell": g.key(p.cell),
        "cell_index": p.cell,
        "origin": at_json(origin),
        "radius_cells": p.radius,
        "radius_km": if mini { (4 + p.power as i64 / 4).max(3) } else { 104 + 208 * p.radius as i64 },
        "power": p.power,
        "intensity": ((p.power as f64 / 16.0) * 10.0).round() / 10.0,
        "age": sim.beat.saturating_sub(p.born),
        "born": p.born,
        "beats_left": p.dur,
        "owner_seat": if p.owner == NOBODY { Value::Null } else { p.owner.into() },
        "cause": CAUSES[p.cause.min(4) as usize],
        "tier": TIERS[p.tier.min(3) as usize],
    });
    if p.parent != 0 {
        v["parent"] = p.parent.into();
    }
    // Where it will be a beat from now, so the client can glide it there.
    let toward = |dir: u8, cells_per_beat: f64| {
        let next = g.nb[p.cell as usize][dir.min(5) as usize];
        json!({ "dir": dir, "to": at_json(g.center[next as usize]), "to_cell": g.key(next), "cells_per_beat": cells_per_beat })
    };
    match p.kind {
        STORM | BLIZZARD => {
            let (dir, _) = wind(g, sim.seed, sim.beat + 1, p.cell, sim.c(p.cell).cloud);
            v["velocity"] = toward(dir, 1.0);
        }
        SWARM if p.dir != NO_DIR => v["velocity"] = toward(p.dir, 1.0),
        TIDAL_WAVE => {
            v["front_cells"] = p.a.into();
            v["front_km"] = (208 * p.a as i64).into();
            v["front_speed_cells_per_beat"] = (cfg_substeps(sim) as i64).into();
        }
        PLATE_DRIFT => {
            v["velocity"] = toward(p.dir, 1.0 / sim.cfg.plate_beats.max(1) as f64);
            v["moves_left"] = p.a.into();
            v["next_move_in"] = p.b.into();
        }
        VOLCANO => v["state"] = if p.dur > 0 { "erupting" } else { "dormant" }.into(),
        EARTHQUAKE => v["ruptured_cells"] = p.a.into(),
        _ => {}
    }
    v
}

fn cfg_substeps(sim: &Sim) -> u32 {
    sim.cfg.substeps.max(1)
}

pub fn phenomena_json(sim: &Sim) -> Vec<Value> {
    sim.phen.iter().map(|p| phenomenon_json(sim, p)).collect()
}

fn hash_hex(sim: &Sim) -> String {
    format!("{:016x}", sim.hash())
}

/// The beat's public `earth` object: what changed since the last beat and every live phenomenon.
pub fn beat_json(eb: &EarthBeat) -> Value {
    let sim = &eb.sim;
    let cells = changed(&eb.before_sim(), sim);
    let r = &eb.report;
    let g = sim.grid();
    let keys = |c: &[u16]| c.iter().map(|&i| g.key(i)).collect::<Vec<_>>();
    let mut v = json!({
        "beat": sim.beat,
        "hash": hash_hex(sim),
        "dirty": sim.list.len(),
        "substeps": r.substeps,
        "delta": columns(sim, &cells, Enc::Pack),
        "phenomena": ranked(sim, None, PHENOMENA_SENT),
        "phenomena_total": sim.phen.len(),
        "ended": r.ended.iter().map(|p| json!({ "id": p.id, "kind": kind_name(p.kind) })).collect::<Vec<_>>(),
        "strikes": keys(&r.strikes),
        "did": {
            "spawned": r.spawned, "spontaneous": r.spontaneous, "ignitions": r.ignitions, "quenched": r.quenched,
            "steamed": r.steamed, "rock_made": r.rock_made, "melted": r.melted, "land_rose": keys(&r.land_rose),
            "drowned": keys(&r.drowned), "woke": r.woke, "evicted": r.evicted.len(), "refused": r.refused,
        },
    });
    // Schema 20: what the earth did, thing by thing.
    if let (Some(fx), Some(o)) = (&eb.effects, v.as_object_mut()) {
        o.extend(super::effects::beat_fields(&fx.rows, fx.total, &fx.counts));
    }
    v
}

/// The field in short (for `present` and the PublicView): enough to know whether a client's copy is current.
pub fn summary(sim: &Sim, s: &EarthState) -> Value {
    let mut tops = [0u32; TOPS.len()];
    for &i in &sim.list {
        tops[sim.top(i) as usize] += 1;
    }
    let tops: Map<String, Value> = TOPS.iter().zip(tops).filter(|x| x.1 > 0).map(|(n, c)| ((*n).to_string(), c.into())).collect();
    json!({
        "beat": sim.beat,
        "hash": hash_hex(sim),
        "dirty": sim.list.len(),
        "tops": tops,
        "phenomena": ranked(sim, None, PHENOMENA_SENT),
        "phenomena_total": sim.phen.len(),
        "keyframes": s.keyframes.iter().map(|k| k.beat).collect::<Vec<_>>(),
        "first_beat": s.first_beat,
        "grid": { "level": "regions", "cells": sim.grid().len() },
    })
}

/// The summary straight from a match state (`None` when the simulator is off or has not begun).
pub fn summary_of(w: &fn7_model::World, st: &Settings) -> Option<Value> {
    let s = w.dm.earth.as_deref()?;
    let mut v = summary(&super::rules::sim_of(w, st)?, s);
    // Schema 20: the last stepped beat's effects (a reconnecting client replays them once).
    if w.earth_effects_rules() {
        if let Some(o) = v.as_object_mut() {
            o.extend(super::effects::stored_fields(s, s.beat));
        }
    }
    Some(v)
}

// ---------------------------------------------------------------------------------------------------------------------
// Finer levels: a pure function of the coarse cell, the seed and the hex
// ---------------------------------------------------------------------------------------------------------------------

fn hex_hash(seed: u64, c: &HexCell) -> u64 {
    mix(seed ^ mix((c.face as u64) | (c.level as u64) << 8 | ((c.q as u64) & 0xFFFF_FFFF) << 16) ^ mix((c.r as u64) ^ 0xF1E))
}

/// A hex at any level as the field has it. At the regions level this is the simulated cell. Below it, the hex is
/// derived: its height is its coarse cell's (in 16ths of a level) with a hashed relief; a patchy layer (fire, flood
/// water, lava, snow, ash, swarm, cracks) is present on the share of fine hexes equal to its coarse intensity, picked
/// by the hex's own hash (so the same hexes burn every time you look, and a growing fire only adds hexes); a smooth
/// layer (cloud, heat, plume, shaking) is the coarse value. A mini's own layer is certain within its small radius of
/// where it was cast.
pub fn hex_json(sim: &Sim, c: &HexCell) -> Value {
    let g = sim.grid();
    let i = if c.level == HexLevel::Regions { g.index(c) } else { g.index_of(c.center()) };
    if i == NONE {
        return Value::Null;
    }
    let cell = *sim.c(i);
    let h = sim.height(i);
    if c.level == HexLevel::Regions {
        let mut v = json!({ "cell": c.key(), "index": i, "level": "regions", "derived": false, "h": h, "h16": h as u32 * 16 + 8,
                            "top": TOPS[sim.top(i) as usize], "material": TOPS[sim.material(i) as usize], "fuel": sim.fuel(i),
                            "base_h": g.h[i as usize], "fault_hops": g.fault[i as usize], "coast_hops": g.coast[i as usize],
                            "weather": weather_word(&cell, sim.cold(i)) });
        for (k, name) in LAYERS.iter().enumerate() {
            let b = cell.bytes()[k];
            if b != 0 && !matches!(*name, "mat") {
                v[*name] = if matches!(*name, "dh" | "heat" | "stress" | "dfuel") { (b as i8).into() } else { b.into() };
            }
        }
        return v;
    }
    let f = fine(sim, c, i);
    json!({
        "cell": c.key(), "level": c.level.as_str(), "derived": true, "coarse": g.key(i), "coarse_index": i,
        "h": (f.h16 / 16) as u8, "h16": f.h16, "top": TOPS[f.top as usize], "material": TOPS[f.mat as usize],
        "fire": f.fire, "water": f.water, "lava": f.lava, "snow": f.snow, "ash": f.ash, "swarm": f.swarm, "crack": f.crack,
        "cloud": cell.cloud, "heat": cell.heat, "plume": cell.plume, "shake": cell.shake, "ruin": cell.ruin,
        "forest": f.forest, "weather": weather_word(&cell, sim.cold(i)),
    })
}

/// A hex below the regions level, derived from its coarse cell `i` (see [`hex_json`]).
pub struct Fine {
    pub h16: u8,
    pub top: u8,
    pub mat: u8,
    pub fire: u8,
    pub water: u8,
    pub lava: u8,
    pub snow: u8,
    pub ash: u8,
    pub swarm: u8,
    pub crack: u8,
    pub forest: bool,
}

pub fn fine(sim: &Sim, c: &HexCell, i: u16) -> Fine {
    let cell = *sim.c(i);
    let h = sim.height(i);
    let hh = hex_hash(sim.seed, c);
    let sea = h < SEA_LEVEL;
    // Relief: ±7 sixteenths, never across the shoreline.
    let h16 = (h as i32 * 16 + 8 + (hh & 15) as i32 - 8).clamp(if sea { 0 } else { 128 }, if sea { 127 } else { 255 }) as u8;
    // Minis cast in this coarse cell: certain within their radius (in hexes of this level).
    let near = |kind: &[u8]| {
        sim.phen.iter().any(|p| {
            if p.tier != TIER_MINI || !kind.contains(&p.kind) || p.cell != i {
                return false;
            }
            let o = cell_at([p.lon_e3 as f64 / 1000.0, p.lat_e3 as f64 / 1000.0], c.level);
            if o.face != c.face {
                return false;
            }
            let (dq, dr) = (o.q - c.q, o.r - c.r);
            let dist = (dq.abs() + dr.abs() + (dq + dr).abs()) / 2;
            let km = (4 + p.power as i64 / 4).max(3) as f64;
            dist as f64 * c.level.size_m() * 1.732 / 1000.0 <= km
        })
    };
    let patch = |k: u32, value: u8, kinds: &[u8]| -> u8 {
        if near(kinds) {
            (value as u32 * 2).clamp(128, 255) as u8
        } else if ((hh >> (8 * (k % 7) + 4)) & 0xFF) < value as u64 {
            (value as u32 * 2).min(255) as u8
        } else {
            0
        }
    };
    let fire = patch(0, cell.fire, &[WILDFIRE, LIGHTNING, METEOR]);
    let water = if sea { 0 } else { patch(1, cell.water, &[FLOOD, SINK_LAND]) };
    let lava = patch(2, cell.lava, &[VOLCANO]);
    let snow = patch(3, cell.snow, &[BLIZZARD]);
    let ash = patch(4, cell.ash, &[VOLCANO]);
    let swarm = patch(5, cell.swarm, &[SWARM]);
    let crack = patch(6, cell.crack, &[EARTHQUAKE, METEOR]);
    let fuel = sim.fuel(i);
    let forest = !sea && ((hh >> 44) & 0xFF) < (fuel as u64 * 2).min(255);
    let mat = sim.material(i);
    let top = if lava > 0 {
        T_LAVA
    } else if fire > 0 {
        T_FIRE
    } else if sea {
        sim.top(i).min(M_ICE)
    } else if crack >= 128 {
        T_RIFT
    } else if water > 0 {
        if cell.heat <= -48 { M_ICE } else { T_FLOOD }
    } else if snow > 0 {
        T_SNOW
    } else if ash >= 64 {
        T_ASH
    } else if swarm > 0 {
        T_SWARM
    } else if forest && mat == M_SOIL {
        T_FOREST
    } else {
        mat
    };
    let _ = (M_ROCK, M_SAND);
    Fine { h16, top, mat, fire, water, lava, snow, ash, swarm, crack, forest }
}

/// The weather the field has over a cell, in one word.
pub fn weather_word(c: &Cell, cold: bool) -> &'static str {
    if c.plume >= 96 {
        "ashfall"
    } else if c.cloud >= 112 && c.charge >= 96 {
        "thunderstorm"
    } else if c.cloud >= 112 {
        if cold { "snow" } else { "rain" }
    } else if c.fire >= 32 || c.plume >= 32 {
        "smoke"
    } else if c.cloud >= 48 {
        "overcast"
    } else if c.heat >= 48 {
        "scorching"
    } else if c.heat <= -32 {
        "bitter cold"
    } else {
        "clear"
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------------------------------------------------

/// Most cells one answer carries.
pub const QUERY_CELLS_MAX: usize = 4000;

struct Loaded {
    /// Schema 20 (the effects and the scars are answered).
    fx: bool,
    sim: Sim,
    state: EarthState,
    resimulated: u32,
    from_keyframe: Option<u32>,
}

fn load(state: &Value, beat: Option<u32>) -> Result<Loaded, String> {
    let schema = state.pointer("/meta/schema").and_then(Value::as_u64).unwrap_or(0) as u32;
    let st = Settings::from_value(state.pointer("/meta/settings").unwrap_or(&Value::Null));
    let s: Option<EarthState> = state.pointer("/dm/earth").filter(|v| !v.is_null())
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string())).transpose()?;
    let Some(s) = s.filter(|_| schema >= fn7_model::earth::EARTH_SCHEMA && st.earth_sim_enabled) else {
        return Err("the Earth simulator is off in this match (state schema 13 with `earth_sim_enabled`)".into());
    };
    let seed = state.pointer("/meta/seed").and_then(Value::as_u64).unwrap_or(0);
    let c = cfg(&st);
    let fx = schema >= fn7_model::earth::EARTH_EFFECTS_SCHEMA;
    match beat.filter(|b| *b != s.beat) {
        None => Ok(Loaded { fx, sim: Sim::from_state(seed, c, &s), state: s, resimulated: 0, from_keyframe: None }),
        Some(b) => {
            let kf = s.keyframes.iter().rev().find(|k| k.beat <= b).map(|k| k.beat);
            let sim = Sim::at_beat(seed, c, &s, b)
                .ok_or_else(|| format!("no field at beat {b}: the simulator ran from beat {} to {}", s.first_beat, s.beat))?;
            let from = kf.unwrap_or(s.first_beat.saturating_sub(1));
            Ok(Loaded { fx, sim, resimulated: b - from, from_keyframe: kf, state: s })
        }
    }
}

/// The field of a match state (a `World` as JSON) at `beat` (None: now), rebuilt from a keyframe when it is a past
/// beat, with the simulator's stored state. For tools (the `fn7-earth-view` CLI).
pub fn field_at(state: &Value, beat: Option<u32>) -> Result<(Sim, EarthState), String> {
    let l = load(state, beat)?;
    Ok((l.sim, l.state))
}

fn uint(a: &Value, k: &str) -> Result<Option<u64>, String> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_u64().map(Some).ok_or_else(|| format!("`{k}` must be a whole number, 0 or more")),
        Some(Value::String(s)) => s.trim().parse().map(Some).map_err(|_| format!("`{k}` must be a whole number, 0 or more")),
        Some(_) => Err(format!("`{k}` must be a whole number")),
    }
}

fn truthy(a: &Value, k: &str) -> bool {
    matches!(a.get(k), Some(Value::Bool(true))) || a.get(k).and_then(Value::as_str).is_some_and(|s| s == "true" || s == "1")
}

/// A board or tiles answer carries its own `fields`: the column lists of `head` are left out.
fn strip_lists(v: &mut Value) {
    if let Some(o) = v.as_object_mut() {
        o.remove("layers");
    }
}

fn head(l: &Loaded) -> Value {
    json!({ "beat": l.sim.beat, "now": l.state.beat, "hash": hash_hex(&l.sim), "dirty": l.sim.list.len(),
            "phenomena": phenomena_json(&l.sim), "rebuilt": l.sim.beat != l.state.beat,
            "from_keyframe": l.from_keyframe, "resimulated_beats": l.resimulated,
            "tops": TOPS, "layers": CLIENT_LAYERS })
}

/// `game.earth {cell?, region?, bbox?, level?, rings?, beat?, base?, grid?, format?}`.
pub fn query_earth(state: &Value, a: &Value) -> Result<Value, String> {
    let beat = uint(a, "beat")?.map(|b| b as u32);
    let l = load(state, beat)?;
    let (sim, g) = (&l.sim, grid());
    let enc = Enc::parse(a)?;
    let key_level = a.get("cell").and_then(Value::as_str).and_then(HexCell::parse).map(|c| c.level);
    let level = match a.get("level").and_then(Value::as_str) {
        None => key_level.unwrap_or(HexLevel::Regions),
        Some(s) => HexLevel::parse(s).ok_or_else(|| format!("`level` must be regions, villages, houses or people, not `{s}`"))?,
    };
    let rings = uint(a, "rings")?;
    let mut out = head(&l);
    out["level"] = level.as_str().into();
    // The whole planet as a lat/long raster (the client's globe resamples it).
    if let Some(deg) = uint(a, "tiles")? {
        let deg = deg.clamp(2, 10) as u32;
        let since = uint(a, "since")?.map(|b| b as u32);
        let then = since.map(|b| Sim::at_beat(sim.seed, sim.cfg, &l.state, b).ok_or_else(|| format!("no field at beat {b}"))).transpose()?;
        out["tiles"] = tiles_json(sim, then.as_ref(), deg, a.get("layout").and_then(Value::as_str) != Some("grid"));
        strip_lists(&mut out);
        out["phenomena"] = ranked(sim, None, uint(a, "max_phenomena")?.unwrap_or(PHENOMENA_SENT as u64) as usize).into();
        return Ok(out);
    }
    // Where: a hex, a region's centre, a box, or (regions level only) everything that differs from base.
    let center: Option<LonLat> = match (a.get("cell").and_then(Value::as_str), a.get("region").and_then(Value::as_str)) {
        (Some(k), _) => Some(HexCell::parse(k).ok_or_else(|| format!("`{k}` is not a hex cell key"))?.center()),
        (None, Some(r)) => {
            let ix: usize = r.strip_prefix("pl:").and_then(|x| x.parse().ok()).ok_or_else(|| format!("`{r}` is not a place ref"))?;
            let c = state.pointer(&format!("/places/{ix}/center")).and_then(|c| Some([c.get(0)?.as_f64()?, c.get(1)?.as_f64()?]));
            Some(c.ok_or_else(|| format!("no place {r}"))?)
        }
        _ => None,
    };
    // Schema 20: `scars: true` answers what lasts, over the whole planet, at the beat.
    if l.fx && truthy(a, "scars") && center.is_none() {
        out["scars"] = super::effects::scars_json(sim, &l.state, None);
        if let Some(o) = out.as_object_mut() {
            o.extend(super::effects::stored_fields(&l.state, sim.beat));
        }
        strip_lists(&mut out);
        return Ok(out);
    }
    let verbose = truthy(a, "hexes") || truthy(a, "field");
    if let Some(center) = center.filter(|_| !verbose) {
        let c0 = match a.get("cell").and_then(Value::as_str).and_then(HexCell::parse) {
            Some(c) if c.level == level => c,
            _ => cell_at(center, level),
        };
        let rings = rings.unwrap_or(BOARD_RINGS as u64).min(BOARD_RINGS as u64) as i64;
        let since = uint(a, "since")?.map(|b| b as u32);
        let then = since.map(|b| Sim::at_beat(sim.seed, sim.cfg, &l.state, b).ok_or_else(|| format!("no field at beat {b}"))).transpose()?;
        out["center_cell"] = c0.key().into();
        out["rings"] = rings.into();
        out["board"] = board_json(sim, then.as_ref(), &c0, rings, truthy(a, "coarse"));
        strip_lists(&mut out);
        let reach_km = level.size_m() * 1.732 * rings as f64 / 1000.0;
        // Schema 20, `scars: true`: what lasts on the board (burnt, mud, ruins, wrecks, people sheltering, the fallen).
        if l.fx && truthy(a, "scars") {
            out["scars"] = super::effects::scars_json(sim, &l.state, Some((c0.center(), reach_km + 120.0)));
        }
        out["phenomena"] = ranked(sim, Some((c0.center(), reach_km)), uint(a, "max_phenomena")?.unwrap_or(PHENOMENA_SENT as u64) as usize).into();
        return Ok(out);
    }
    if level != HexLevel::Regions {
        let center = center.ok_or("a level below regions needs `cell` (a hex of any level) or `region`: the board's centre")?;
        let rings = rings.unwrap_or(6).min(16) as i64;
        let c0 = cell_at(center, level);
        let mut hexes = vec![];
        for k in 0..=rings {
            for c in c0.ring(k) {
                // A board near a face's seam: hexes past the edge are left out (the next face's board continues there).
                let x = fn7_model::hexgrid::hex_to_plane(c.q, c.r, level.size_m());
                if x.0.abs() <= fn7_model::hexgrid::EARTH_R_M && x.1.abs() <= fn7_model::hexgrid::EARTH_R_M {
                    hexes.push(hex_json(sim, &c));
                }
            }
        }
        out["center_cell"] = c0.key().into();
        out["rings"] = rings.into();
        out["derived"] = true.into();
        out["hexes"] = hexes.into();
        return Ok(out);
    }
    let cells: Vec<u16> = if let Some(center) = center {
        let rings = rings.unwrap_or(3).min(12) as u8;
        let i = g.index_of(center);
        if i == NONE {
            return Err("no cell there".into());
        }
        out["center_cell"] = g.key(i).into();
        out["rings"] = rings.into();
        g.disc(i, rings).into_iter().map(|x| x.0).collect()
    } else if let Some(b) = a.get("bbox") {
        let nums: Vec<f64> = match b {
            Value::Array(v) => v.iter().filter_map(Value::as_f64).collect(),
            Value::String(s) => s.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
            _ => vec![],
        };
        let [lon0, lat0, lon1, lat1] = nums[..] else { return Err("`bbox` is [west, south, east, north] in degrees".into()) };
        let inside = |p: LonLat| p[1] >= lat0 && p[1] <= lat1 && if lon0 <= lon1 { p[0] >= lon0 && p[0] <= lon1 } else { p[0] >= lon0 || p[0] <= lon1 };
        (0..g.len() as u16).filter(|&i| inside(g.center[i as usize])).collect()
    } else {
        sim.list.clone()
    };
    let mut cells = cells;
    cells.sort_unstable();
    cells.dedup();
    if cells.len() > QUERY_CELLS_MAX {
        return Err(format!("{} cells: ask for a smaller box (at most {QUERY_CELLS_MAX})", cells.len()));
    }
    out["field"] = columns(sim, &cells, enc);
    // The whole globe, once: every cell's height and top material now, in index order (what is not in `field` is at
    // base, and this is the base).
    if truthy(a, "base") {
        let all: Vec<u16> = (0..g.len() as u16).collect();
        let h: Vec<u8> = all.iter().map(|&i| sim.height(i)).collect();
        let top: Vec<u8> = all.iter().map(|&i| sim.top(i)).collect();
        let fuel: Vec<u8> = all.iter().map(|&i| sim.fuel(i)).collect();
        out["globe"] = if enc == Enc::Json {
            json!({ "n": g.len(), "enc": "json", "h": h, "top": top, "fuel": fuel })
        } else {
            json!({ "n": g.len(), "enc": "b64", "h": b64(&h), "top": b64(&top), "fuel": b64(&fuel) })
        };
    }
    // The grid itself, once: every cell's face, q and r (signed bytes) in index order.
    if truthy(a, "grid") {
        let col = |f: &dyn Fn(&HexCell) -> u8| b64(&g.cells.iter().map(f).collect::<Vec<u8>>());
        out["grid"] = json!({ "level": "regions", "hex_size_m": 120000, "cells": g.len(),
                              "face": col(&|c| c.face), "q": col(&|c| c.q as i8 as u8), "r": col(&|c| c.r as i8 as u8) });
    }
    Ok(out)
}

/// `game.earth_at {beat}`: the whole field as it stood at the end of a past beat, rebuilt from the newest keyframe
/// before it and the input log.
pub fn query_earth_at(state: &Value, a: &Value) -> Result<Value, String> {
    let beat = uint(a, "beat")?.ok_or("`beat` is required")? as u32;
    let l = load(state, Some(beat))?;
    let enc = Enc::parse(a)?;
    let mut out = head(&l);
    out["field"] = columns(&l.sim, &l.sim.list, enc);
    out["keyframes"] = l.state.keyframes.iter().map(|k| k.beat).collect::<Vec<_>>().into();
    out["inputs"] = l.state.inputs.iter().filter(|i| i.beat <= beat).count().into();
    // Schema 20: that beat's effects, thing by thing, and what lasted at its end.
    if l.fx {
        if let Some(o) = out.as_object_mut() {
            o.extend(super::effects::stored_fields(&l.state, beat));
        }
        out["scars"] = super::effects::scars_json(&l.sim, &l.state, None);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------------------
// The board (a disc of hexes at any level) and the globe's raster: what an Unreal client draws
// ---------------------------------------------------------------------------------------------------------------------

/// The client's board: 22 rings around the centre hex, 1,519 hexes.
pub const BOARD_RINGS: i64 = 22;
/// Phenomena sent to a client at most (the biggest first, or the nearest to a board's centre).
pub const PHENOMENA_SENT: usize = 20;
/// One board (or globe tile) record, a byte each: height in 256ths (128 = sea level), top material, then the layers.
/// `heat` is a signed byte.
pub const BOARD_FIELDS: [&str; 9] = ["h16", "top", "water", "lava", "ash", "fire", "snow", "heat", "crack"];

/// The record of one hex of any level. At the regions level it is the cell's own values; below, it is derived from
/// the coarse cell under it ([`fine`]), a pure function of the field, the seed and the hex. A hex past its face's edge
/// takes the cell that holds its centre on the next face, so every position of the client's ring enumeration has one.
pub fn board_record(sim: &Sim, c: &HexCell) -> ([u8; 9], u16) {
    let g = sim.grid();
    let mut i = if c.level == HexLevel::Regions { g.index(c) } else { NONE };
    if i == NONE {
        i = g.index_of(c.center());
    }
    if i == NONE {
        return ([0; 9], NONE);
    }
    let cell = sim.c(i);
    if c.level == HexLevel::Regions {
        let h16 = (sim.height(i) as u32 * 16 + 8).min(255) as u8;
        return ([h16, sim.top(i), cell.water, cell.lava, cell.ash, cell.fire, cell.snow, cell.heat as u8, cell.crack], i);
    }
    let f = fine(sim, c, i);
    ([f.h16, f.top, f.water, f.lava, f.ash, f.fire, f.snow, cell.heat as u8, f.crack], i)
}

/// The board's hexes in the client's order: ring 0, then ring 1.. `rings`, each ring as [`HexCell::ring`] walks it.
pub fn board_cells(c0: &HexCell, rings: i64) -> Vec<HexCell> {
    (0..=rings).flat_map(|k| c0.ring(k)).collect()
}

/// `board`: `{level, center, rings, n, fields, enc: "b64", data}`, `data` = 9 bytes per hex in ring order. With
/// `then` (the field at `since`): only the positions whose record changed, as `changed` (u16 LE ring positions).
/// `coarse`: the regions cell index under each hex (u16 LE), so a client can intersect it with a beat's delta.
pub fn board_json(sim: &Sim, then: Option<&Sim>, c0: &HexCell, rings: i64, coarse: bool) -> Value {
    let cells = board_cells(c0, rings);
    let mut data = Vec::with_capacity(cells.len() * 9);
    let mut under = Vec::with_capacity(cells.len() * 2);
    let mut changed = Vec::new();
    for (k, c) in cells.iter().enumerate() {
        let (rec, i) = board_record(sim, c);
        if let Some(t) = then {
            if board_record(t, c).0 == rec {
                continue;
            }
            changed.extend((k as u16).to_le_bytes());
        }
        data.extend(rec);
        under.extend(i.to_le_bytes());
    }
    let mut v = json!({ "level": c0.level.as_str(), "center": c0.key(), "rings": rings, "n": data.len() / 9, "fields": BOARD_FIELDS,
                        "enc": "b64", "data": b64(&data) });
    if then.is_some() {
        v["changed"] = b64(&changed).into();
        v["since"] = then.map(|t| t.beat).into();
    }
    if coarse {
        v["coarse"] = b64(&under).into();
    }
    v
}

/// Dramatic tops win a tile (lava over fire over a rift ...); otherwise the most common top under it.
const TOP_RANK: [u8; 13] = [0, 0, 0, 0, 0, 0, 9, 8, 4, 2, 6, 7, 3];

/// The globe's tiles: rings of latitude `deg`° tall, rows from the north pole down; row `y` (centre latitude `lat`)
/// has `max(1, round(360 / deg × cos lat))` tiles from 180° W eastward, so tiles are about equally large (4°: 2,578
/// tiles, about 445 km). `layout: "grid"` instead gives every row 360 / deg tiles (4°: 4,050).
pub fn row_cols(deg: u32, equal_area: bool) -> Vec<usize> {
    let rows = (180 / deg) as usize;
    let full = (360 / deg) as usize;
    (0..rows)
        .map(|y| {
            if !equal_area {
                return full;
            }
            let lat = 90.0 - deg as f64 * (y as f64 + 0.5);
            ((full as f64 * lat.to_radians().cos()).round() as usize).max(1)
        })
        .collect()
}

/// `tiles`: `{deg, layout, rows, row_cols, n, fields, enc, data}` with 9 bytes per tile ([`BOARD_FIELDS`]), rows
/// from the north down and west to east in each: the mean height of the regions cells whose centres fall in the
/// tile, the most dramatic top (else the most common), each layer's maximum (heat: the largest in size, signed). A
/// tile no centre falls in takes the cell under its centre. With `then`: only the tiles that changed (`changed`,
/// u16 LE tile indices).
pub fn tiles_json(sim: &Sim, then: Option<&Sim>, deg: u32, equal_area: bool) -> Value {
    let rc = row_cols(deg, equal_area);
    let now = tiles(sim, deg, &rc);
    let mut data = Vec::with_capacity(now.len() * 9);
    let mut changed = vec![];
    let old = then.map(|t| tiles(t, deg, &rc));
    for (k, rec) in now.iter().enumerate() {
        if let Some(o) = &old {
            if o[k] == *rec {
                continue;
            }
            changed.extend((k as u16).to_le_bytes());
        }
        data.extend(rec);
    }
    let mut v = json!({ "deg": deg, "layout": if equal_area { "rings" } else { "grid" }, "rows": rc.len(), "row_cols": rc,
                        "tiles": now.len(), "n": data.len() / 9, "fields": BOARD_FIELDS, "enc": "b64", "data": b64(&data) });
    if let Some(t) = then {
        v["changed"] = b64(&changed).into();
        v["since"] = t.beat.into();
    }
    v
}

fn tiles(sim: &Sim, deg: u32, rc: &[usize]) -> Vec<[u8; 9]> {
    let g = sim.grid();
    let d = deg as f64;
    let start: Vec<usize> = rc.iter().scan(0, |a, &n| { let s = *a; *a += n; Some(s) }).collect();
    let total: usize = rc.iter().sum();
    let tile_of = |p: LonLat| {
        let y = (((90.0 - p[1]) / d).floor() as i64).clamp(0, rc.len() as i64 - 1) as usize;
        let x = ((((p[0] + 180.0) / 360.0) * rc[y] as f64).floor() as i64).rem_euclid(rc[y] as i64) as usize;
        start[y] + x
    };
    type Acc = (u32, u32, [u16; 13], [u8; 6], i8);
    let mut acc: Vec<Acc> = vec![(0, 0, [0; 13], [0; 6], 0); total];
    let add = |acc: &mut Vec<Acc>, t: usize, i: u16| {
        let c = sim.c(i);
        let a = &mut acc[t];
        a.0 += sim.height(i) as u32 * 16 + 8;
        a.1 += 1;
        a.2[sim.top(i) as usize] += 1;
        for (k, v) in [c.water, c.lava, c.ash, c.fire, c.snow, c.crack].into_iter().enumerate() {
            a.3[k] = a.3[k].max(v);
        }
        if (c.heat as i32).abs() > (a.4 as i32).abs() {
            a.4 = c.heat;
        }
    };
    for i in 0..g.len() as u16 {
        add(&mut acc, tile_of(g.center[i as usize]), i);
    }
    for (y, &n) in rc.iter().enumerate() {
        for x in 0..n {
            let t = start[y] + x;
            if acc[t].1 == 0 {
                let p = [-180.0 + 360.0 * (x as f64 + 0.5) / n as f64, 90.0 - d * (y as f64 + 0.5)];
                let i = g.index_of(p);
                if i != NONE {
                    add(&mut acc, t, i);
                }
            }
        }
    }
    acc.iter()
        .map(|a| {
            let h = if a.1 == 0 { 0 } else { (a.0 / a.1).min(255) as u8 };
            let dramatic = (0..13).filter(|&k| a.2[k] > 0 && TOP_RANK[k] > 0).max_by_key(|&k| TOP_RANK[k]);
            let top = dramatic.unwrap_or_else(|| (0..13).max_by_key(|&k| (a.2[k], std::cmp::Reverse(k))).unwrap_or(0)) as u8;
            [h, top, a.3[0], a.3[1], a.3[2], a.3[3], a.3[4], a.4 as u8, a.3[5]]
        })
        .collect()
}

/// The phenomena a client is sent: at most `max`, ranked by size (power × reach) or, around a board's centre, nearest
/// first (only those whose reach touches the board). Each keeps its stable `id`.
pub fn ranked(sim: &Sim, around: Option<(LonLat, f64)>, max: usize) -> Vec<Value> {
    let g = sim.grid();
    let size = |p: &Phenomenon| p.power.max(0) as i64 * (p.radius as i64 + 1);
    let mut list: Vec<(i64, i64, u32, &Phenomenon)> = sim.phen.iter()
        .filter(|p| p.kind != VOLCANO || p.dur > 0 || around.is_some())
        .filter_map(|p| {
            let at = if p.tier == TIER_MINI { [p.lon_e3 as f64 / 1000.0, p.lat_e3 as f64 / 1000.0] } else { g.center[p.cell as usize] };
            match around {
                None => Some((0, -size(p), p.id, p)),
                Some((c, reach)) => {
                    let km = fn7_society::geo::distance_km(c, at);
                    let r = if p.tier == TIER_MINI { 10.0 } else { 104.0 + 208.0 * p.radius as f64 };
                    (km <= reach + r).then_some((km as i64, -size(p), p.id, p))
                }
            }
        })
        .collect();
    list.sort_by_key(|x| (x.0, x.1, x.2));
    list.into_iter().take(max).map(|x| phenomenon_json(sim, x.3)).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// The ASCII map
// ---------------------------------------------------------------------------------------------------------------------

/// One character per top material, and the ANSI colour it is drawn in.
pub const GLYPHS: [(char, &str); 13] = [
    ('~', "34"), ('^', "37"), ('.', "32"), (':', "33"), ('*', "97"), ('"', "92"), ('%', "91;1"), ('&', "31;1"), ('a', "90"),
    ('o', "97;1"), ('=', "36;1"), ('/', "35;1"), ('z', "93"),
];

fn phen_glyph(kind: u8) -> char {
    match kind {
        STORM => 'S',
        LIGHTNING => 'L',
        FLOOD => 'F',
        WILDFIRE => 'W',
        VOLCANO => 'V',
        EARTHQUAKE => 'Q',
        METEOR => 'M',
        RAISE_LAND => 'R',
        SINK_LAND => 'K',
        FOREST => 'G',
        SWARM => 'Z',
        BLIZZARD => 'B',
        TIDAL_WAVE => 'T',
        _ => 'P',
    }
}

/// A map of `bbox` ([west, south, east, north]) `cols` × `rows` characters: each the top material of the cell under
/// it; a capital letter marks a phenomenon's centre. Sea deeper than the shelf is drawn as a space.
pub fn ascii(sim: &Sim, bbox: [f64; 4], cols: usize, rows: usize, ansi: bool) -> String {
    let g = sim.grid();
    let [w, s, e, n] = bbox;
    let mut out = String::new();
    for y in 0..rows {
        let lat = n - (n - s) * (y as f64 + 0.5) / rows as f64;
        for x in 0..cols {
            let lon = w + (e - w) * (x as f64 + 0.5) / cols as f64;
            let i = g.index_of([lon, lat.clamp(-89.9, 89.9)]);
            if i == NONE {
                out.push('?');
                continue;
            }
            let top = sim.top(i);
            let mut ch = GLYPHS[top as usize].0;
            let mut col = GLYPHS[top as usize].1;
            if top == T_WATER && sim.height(i) < 6 && sim.c(i).water < 16 {
                ch = ' ';
            }
            if let Some(p) = sim.phen.iter().find(|p| p.cell == i && (p.kind != VOLCANO || p.dur > 0)) {
                ch = phen_glyph(p.kind);
                col = "1;7";
            }
            if ansi {
                out.push_str(&format!("\x1b[{col}m{ch}\x1b[0m"));
            } else {
                out.push(ch);
            }
        }
        out.push('\n');
    }
    out
}

pub const LEGEND: &str = "~ sea (deep: blank)  ^ rock  . soil  : sand  * ice  \" forest  % lava  & fire  a ash  o snow  = flood  / rift  z swarm   \
                          capitals = phenomena: S storm L lightning F flood W wildfire V volcano Q quake M meteor R raise K sink G forest Z swarm B blizzard T tidal wave P plate";
