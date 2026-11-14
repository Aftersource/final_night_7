//! Game queries: `game.<name>` over the WebSocket (answered for the asker's seat), and the `public` ones also at
//! `GET /api/rooms/{code}/query/{name}?arg=value` (answered with no seat: public information only). The data contract
//! is in docs/CLIENT_API.md ("Game queries").
//!
//! **Cheap by construction.** The core runs `query` synchronously on the room actor, so a slow query stalls that room's
//! timers, submits and broadcasts. A query never loads the whole `World` (at beat 50 the ledger and memory are most of
//! a ~4 MB state and take ~5 ms to deserialize): it deserializes only the tables it reads (each well under 0.3 ms),
//! reaches ledger rows by index (`lg:N` is row N) and scans at most [`SCAN_CAP`] of them per call, tests a row for
//! publicity on a borrowed head before deserializing it, and pages every list that can grow (`limit` + a cursor the
//! reply hands back).
//!
//! **Hidden information.** Public queries build only what the PublicView already shows (positions, crowds, armies
//! without their orders, public events). Seat-scoped answers (`me`, `person`'s allegiance and memories, `army`'s
//! orders) use the asking seat's demigod; with no seat (HTTP) they return the public subset.

use fn7_model::*;
use serde::Deserialize;
use serde_json::{json, Value};
use theatre_plugin_api::{QuerySpec, Seat};

use crate::payload::{self, region_name};
use crate::society::{alleg_json, cell_json, chain_json, crowd_json, mob_json, node_json, who_json};
use crate::world::{knows, seat_state, selves};

/// Ledger rows one query call looks at, at most (events are paged past it with the returned cursor).
pub const SCAN_CAP: usize = 2000;

/// Every query, its args schema and whether it is public (the manifest in `server.hello.api.queries`).
pub fn specs() -> Vec<QuerySpec> {
    let since = json!({ "type": ["integer", "string"], "minimum": 0,
        "description": "Event cursor: a ledger row index (`next` from the previous page). Events from this row on. Omit for the latest events." });
    let limit = |def: usize, max: usize| {
        json!({ "type": ["integer", "string"], "minimum": 1, "maximum": max,
                "description": format!("Page size (default {def}, at most {max}).") })
    };
    let cursor = json!({ "type": ["integer", "string"], "minimum": 0,
        "description": "Page cursor: `next_cursor` from the previous page. Omit for the first page." });
    let r#ref = |pattern: &str, desc: &str| json!({ "type": "string", "pattern": pattern, "description": desc });
    let obj = |props: Value, required: &[&str]| {
        json!({ "type": "object", "properties": props, "required": required, "additionalProperties": true,
                "description": "Over HTTP every arg is a query-string parameter (numbers as numeric strings). Unknown args are ignored." })
    };
    let spec = |name: &str, public: bool, description: &str, args_schema: Value| QuerySpec {
        name: name.into(),
        description: description.into(),
        args_schema,
        public,
    };
    vec![
        spec("globe", true,
             "The globe poll: arena circle, demigods' public positions, influence per region, live world mods, armies, Entropy, beat, and a page of recent public geotagged events.",
             obj(json!({ "since": since, "limit": limit(30, 200) }), &[])),
        spec("region", true,
             "One region: its rolled-up node (population, mood), cities, a page of its biggest crowds, mobs, armies and demigods present, mods, influence, recent public events.",
             obj(json!({ "ref": r#ref(r"^pl:\d+$", "The region (a `pl:` ref of a region; in a city arena, a district)."),
                         "cursor": cursor, "limit": limit(10, 50) }), &["ref"])),
        spec("node", true,
             "One hierarchy node by ref: a place (planet, region, city, district: its node, child nodes and a page of its crowds), a crowd (`cl:`, not an army), a cell (`ce:`) or a mob (`mb:`), with the people known there.",
             obj(json!({ "ref": r#ref(r"^(pl|cl|ce|mb):\d+$", "The node."), "cursor": cursor, "limit": limit(20, 100) }), &["ref"])),
        spec("person", true,
             "One person (`ch:`): role, rank, place. For your seat also the allegiance you know and a page of the memories you can see.",
             obj(json!({ "ref": r#ref(r"^ch:\d+$", "The person."), "cursor": cursor, "limit": limit(20, 100) }), &["ref"])),
        spec("army", true,
             "One army (`cl:`): general, captains, size, position, target, morale. Its demigod's seat also gets its orders and full chain of command.",
             obj(json!({ "ref": r#ref(r"^cl:\d+$", "The army.") }), &["ref"])),
        spec("events", true,
             "Public geotagged events, paged by ledger row: since a cursor, or the latest. Optionally one region's.",
             obj(json!({ "since": since, "limit": limit(50, 200),
                         "region": r#ref(r"^pl:\d+$", "Only events in this region.") }), &[])),
        spec("me", false, "Your seat view (the SeatView, as in round.private_state), now.", obj(json!({}), &[])),
        spec("powers", true, "The power palette (casts), then the custom entry.", obj(json!({}), &[])),
        spec("gods", true, "The pantheon: each god's ref, name, slug, domain and health now.", obj(json!({}), &[])),
        spec("standings", true, "Every demigod ranked now: alive first, then who fell last, then score.", obj(json!({}), &[])),
        spec("piles_final", true,
             "Every demigod's end-of-match pile (items, summons, stats, statue prompt). An error until the match is over.",
             obj(json!({}), &[])),
    ]
}

/// Answer `name` for `seat` (`None` = a public asker).
pub fn query(pantheon: &Pantheon, state: &Value, seat: Option<Seat>, name: &str, args: &Value) -> Result<Value, String> {
    let args = Args(args);
    let mut v = match name {
        "globe" => globe(state, &args),
        "region" => region(state, &args),
        "node" => node(state, &args),
        "person" => person(state, seat, &args),
        "army" => army(state, seat, &args),
        "events" => events(state, &args),
        "me" => me(state, seat),
        "powers" => Ok(json!({ "powers": payload::powers_json() })),
        "gods" => gods(state, pantheon),
        "standings" => standings(state),
        "piles_final" => piles_final(state),
        other => Err(format!("unknown query {other}")),
    }?;
    payload::tidy(&mut v);
    Ok(v)
}

// ---------------------------------------------------------------------------------------------------------------------
// Partial state
// ---------------------------------------------------------------------------------------------------------------------

/// The state tables a query may load. Anything not asked for stays empty.
#[derive(Clone, Copy, PartialEq, Eq)]
enum T {
    Places,
    Groups,
    Clusters,
    Characters,
    Demigods,
    Items,
    Agents,
    Gods,
    Rulings,
    Seats,
    Mods,
    Nodes,
    Cells,
    Mobs,
    Battles,
    Slow,
}

fn table<D: serde::de::DeserializeOwned + Default>(state: &Value, key: &str, want: bool) -> Result<D, String> {
    match state.get(key) {
        Some(v) if want && !v.is_null() => D::deserialize(v).map_err(|e| format!("state.{key}: {e}")),
        _ => Ok(D::default()),
    }
}

/// A `World` with only `t` loaded (plus `meta`); the ledger, memory, intents, rewind ring and beat records stay empty.
fn world(state: &Value, t: &[T]) -> Result<World, String> {
    let has = |x: T| t.contains(&x);
    let meta = state.get("meta").ok_or("no match state")?;
    Ok(World {
        meta: Meta::deserialize(meta).map_err(|e| format!("state.meta: {e}"))?,
        places: table(state, "places", has(T::Places))?,
        groups: table(state, "groups", has(T::Groups))?,
        clusters: table(state, "clusters", has(T::Clusters))?,
        characters: table(state, "characters", has(T::Characters))?,
        demigods: table(state, "demigods", has(T::Demigods))?,
        items: table(state, "items", has(T::Items))?,
        relations: vec![],
        agents: table(state, "agents", has(T::Agents))?,
        gods: table(state, "gods", has(T::Gods))?,
        rulings: table(state, "rulings", has(T::Rulings))?,
        intents: vec![],
        ledger: Ledger::default(),
        memory: MemoryStore::default(),
        rewind: RewindRing::default(),
        beats: vec![],
        seats: table(state, "seats", has(T::Seats))?,
        pending: vec![],
        mods: table(state, "mods", has(T::Mods))?,
        mod_seq: table(state, "mod_seq", true)?,
        slow: table(state, "slow", has(T::Slow))?,
        slow_settled: vec![],
        out_reported: vec![],
        nodes: table(state, "nodes", has(T::Nodes))?,
        cells: table(state, "cells", has(T::Cells))?,
        mobs: table(state, "mobs", has(T::Mobs))?,
        battles: table(state, "battles", has(T::Battles))?,
        soc: Default::default(),
    })
}

/// The ledger rows, unparsed (`lg:N` is index N).
fn rows(state: &Value) -> &[Value] {
    state
        .pointer("/ledger/rows")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn row(v: &Value) -> Option<LedgerRow> {
    LedgerRow::deserialize(v).ok()
}

/// What [`payload::is_public_head`] reads, borrowed from the unparsed row.
#[derive(Deserialize)]
struct Head<'a> {
    verb: &'a str,
    magnitude: u8,
    source: Source,
    actor: Ref,
}

fn is_public(w: &World, v: &Value) -> bool {
    Head::deserialize(v).is_ok_and(|h| payload::is_public_head(w, h.verb, h.magnitude, &h.source, h.actor))
}

/// The asking seat's demigod (`None`: a public asker, or a seat with no demigod).
fn asker(w: &World, seat: Option<Seat>) -> Option<DemigodId> {
    seat.and_then(|s| seat_state(w, s)).map(|s| s.demigod)
}

// ---------------------------------------------------------------------------------------------------------------------
// Args (WebSocket: JSON; HTTP: every value a string)
// ---------------------------------------------------------------------------------------------------------------------

struct Args<'a>(&'a Value);

impl Args<'_> {
    fn get(&self, k: &str) -> Option<&Value> {
        self.0.get(k).filter(|v| !v.is_null() && v.as_str() != Some(""))
    }
    fn uint(&self, k: &str) -> Result<Option<usize>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::Number(n)) => n
                .as_u64()
                .map(|x| Some(x as usize))
                .ok_or_else(|| format!("`{k}` must be a non-negative integer")),
            Some(Value::String(s)) => s
                .trim()
                .parse::<usize>()
                .map(Some)
                .map_err(|_| format!("`{k}` must be a non-negative integer")),
            Some(_) => Err(format!("`{k}` must be a non-negative integer")),
        }
    }
    fn limit(&self, def: usize, max: usize) -> Result<usize, String> {
        match self.uint("limit")? {
            None => Ok(def),
            Some(n) if (1..=max).contains(&n) => Ok(n),
            Some(_) => Err(format!("`limit` must be 1..{max}")),
        }
    }
    fn r#ref(&self, k: &str) -> Result<Option<Ref>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::String(s)) => s.trim().parse::<Ref>().map(Some).map_err(|e| format!("`{k}`: {e}")),
            Some(_) => Err(format!("`{k}` must be a ref string such as \"pl:3\"")),
        }
    }
    fn need_ref(&self, k: &str) -> Result<Ref, String> {
        self.r#ref(k)?.ok_or_else(|| format!("`{k}` is required"))
    }
}

/// One page of a list: `{items, next_cursor, total}` (`next_cursor` null on the last page). Only the page's entries
/// are built.
fn page<X>(all: Vec<X>, cursor: usize, limit: usize, build: impl Fn(X) -> Value) -> Value {
    let total = all.len();
    let end = (cursor + limit).min(total);
    let items: Vec<Value> = all.into_iter().skip(cursor).take(limit).map(build).collect();
    json!({ "items": items, "next_cursor": (end < total).then_some(end), "total": total })
}

// ---------------------------------------------------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------------------------------------------------

/// A public event as a query returns it: the outcome's event plus `beat` and `region_ref`.
fn event_json(w: &World, r: &LedgerRow) -> Value {
    let mut v = payload::event_json(w, r);
    v["beat"] = r.beat.into();
    v["region_ref"] = json!(payload::geotag_place(w, r).1.map(Ref::Place));
    v
}

/// A page of public events: `{items, since, next, more, head}`. With `since`, rows `since..` in order (at most
/// [`SCAN_CAP`] scanned, `next` = the row to ask from next time); without, the latest `limit` (scanning back at most
/// [`SCAN_CAP`] rows), oldest first, `next` = `head`. `head` = rows in the ledger now.
fn event_page(w: &World, state: &Value, since: Option<usize>, limit: usize, region: Option<PlaceId>) -> Value {
    let rows = rows(state);
    let head = rows.len();
    let mut items = vec![];
    let take = |v: &Value| -> Option<Value> {
        if !is_public(w, v) {
            return None;
        }
        let r = row(v)?;
        if region.is_some() && payload::geotag_place(w, &r).1 != region {
            return None;
        }
        Some(event_json(w, &r))
    };
    let next = match since {
        Some(s) => {
            let start = s.min(head);
            let end = head.min(start + SCAN_CAP);
            let mut next = end;
            for (i, v) in rows.iter().enumerate().take(end).skip(start) {
                if let Some(e) = take(v) {
                    items.push(e);
                    if items.len() >= limit {
                        next = i + 1;
                        break;
                    }
                }
            }
            next
        }
        None => {
            for v in rows.iter().rev().take(SCAN_CAP) {
                if let Some(e) = take(v) {
                    items.push(e);
                    if items.len() >= limit {
                        break;
                    }
                }
            }
            items.reverse();
            head
        }
    };
    json!({ "items": items, "since": since, "next": next, "more": next < head, "head": head })
}

fn events(state: &Value, a: &Args) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Clusters, T::Gods])?;
    let region = match a.r#ref("region")? {
        None => None,
        Some(r) => Some(region_ref(&w, r)?),
    };
    let mut v = event_page(&w, state, a.uint("since")?, a.limit(50, 200)?, region);
    v["beat"] = w.meta.beat.into();
    Ok(v)
}

// ---------------------------------------------------------------------------------------------------------------------
// Globe, region, node
// ---------------------------------------------------------------------------------------------------------------------

/// An army as the public sees it on the globe (no orders, no chain of command).
fn army_marker(w: &World, c: &Cluster, a: &Army) -> Value {
    json!({ "ref": Ref::Cluster(c.id), "name": c.label, "demigod": w.demigods.get(a.demigod.ix()).map(|d| d.name.clone()),
            "demigod_ref": Ref::Demigod(a.demigod), "rogue": a.rogue, "size": c.size, "pos": a.pos,
            "region": region_name(w, a.pos), "dest": fn7_society::army_dest(w, a), "target": a.target,
            "stance": a.stance, "morale": a.morale })
}

fn globe(state: &Value, a: &Args) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Clusters, T::Characters, T::Demigods, T::Gods, T::Mods])?;
    let events = event_page(&w, state, a.uint("since")?, a.limit(30, 200)?, None);
    Ok(json!({
        "beat": w.meta.beat,
        "beats_total": w.meta.beats_total,
        "final_night": w.meta.beat >= w.meta.beats_total,
        "arena": crate::phases::arena_json(&w),
        "alive": w.alive_demigods().count(),
        "entropy": payload::entropy_json(&w, None),
        "demigods": w.demigods.iter().map(|d| crate::phases::demigod_public(&w, d)).collect::<Vec<_>>(),
        "influence": payload::influence_json(&w),
        "mods": payload::mods_json(&w),
        "armies": fn7_society::armies(&w).map(|(c, x)| army_marker(&w, c, x)).collect::<Vec<_>>(),
        "events": events,
    }))
}

/// A `pl:` ref that is a region (a district in a city arena).
fn region_ref(w: &World, r: Ref) -> Result<PlaceId, String> {
    match r {
        Ref::Place(p) if w.places.get(p.ix()).is_some_and(|x| x.kind == PlaceKind::District) => Ok(p),
        Ref::Place(_) => Err(format!("{r} is not a region")),
        _ => Err(format!("{r} is not a place (`pl:` ref)")),
    }
}

/// Crowds (not armies, not empty) living under place `p`, biggest first.
fn crowds_under(w: &World, p: PlaceId) -> Vec<&Cluster> {
    let mut v: Vec<&Cluster> = w
        .clusters
        .iter()
        .filter(|c| c.army.is_none() && c.size > 0 && fn7_society::sim::hier::under(w, c.place, p))
        .collect();
    v.sort_by(|a, b| b.size.cmp(&a.size).then(a.id.cmp(&b.id)));
    v
}

fn region(state: &Value, a: &Args) -> Result<Value, String> {
    let w = world(
        state,
        &[T::Places, T::Nodes, T::Clusters, T::Characters, T::Cells, T::Mobs, T::Demigods, T::Gods, T::Mods],
    )?;
    let p = region_ref(&w, a.need_ref("ref")?)?;
    let pl = &w.places[p.ix()];
    let cities: Vec<Value> = w
        .places
        .iter()
        .filter(|c| c.parent == Some(p))
        .map(|c| node_json(&w, c.id))
        .collect();
    let here = |at: LonLat| fn7_society::geo::region_at(&w, at) == p;
    let reg = Ref::Place(p).to_string();
    Ok(json!({
        "ref": Ref::Place(p),
        "name": pl.name,
        "center": pl.center,
        "radius_km": pl.radius_km,
        "node": node_json(&w, p),
        "cities": cities,
        "crowds": page(crowds_under(&w, p), a.uint("cursor")?.unwrap_or(0), a.limit(10, 50)?, |c| crowd_json(&w, c.id)),
        "mobs": w.mobs.iter().filter(|m| m.ended.is_none() && fn7_society::sim::hier::under(&w, m.place, p))
            .map(|m| mob_json(&w, m)).collect::<Vec<_>>(),
        "armies": fn7_society::armies(&w).filter(|(_, x)| here(x.pos)).map(|(c, x)| army_marker(&w, c, x)).collect::<Vec<_>>(),
        "demigods": w.alive_demigods().filter(|d| here(d.pos)).map(|d| crate::phases::demigod_public(&w, d)).collect::<Vec<_>>(),
        "mods": payload::mods_json(&w).into_iter().filter(|m| m["region_ref"] == reg.as_str()).collect::<Vec<_>>(),
        "influence": payload::influence_json(&w).into_iter().filter(|m| m["region_ref"] == reg.as_str()).collect::<Vec<_>>(),
        "events": event_page(&w, state, None, 10, Some(p)),
    }))
}

fn node(state: &Value, a: &Args) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Nodes, T::Clusters, T::Characters, T::Cells, T::Mobs, T::Demigods, T::Gods])?;
    let r = a.need_ref("ref")?;
    let cursor = a.uint("cursor")?.unwrap_or(0);
    let limit = a.limit(20, 100)?;
    let people = |f: &dyn Fn(&Character) -> bool| -> Value {
        let ids: Vec<CharId> = w.characters.iter().filter(|c| f(c)).map(|c| c.id).collect();
        page(ids, cursor, limit, |c| who_json(&w, Some(c)))
    };
    match r {
        Ref::Place(p) if p.ix() < w.places.len() => {
            let children: Vec<Value> = w
                .places
                .iter()
                .filter(|c| c.parent == Some(p))
                .map(|c| node_json(&w, c.id))
                .collect();
            let crowds = page(crowds_under(&w, p), cursor, limit, |c| crowd_json(&w, c.id));
            Ok(json!({ "kind": "place", "ref": r, "node": node_json(&w, p), "children": children, "crowds": crowds }))
        }
        Ref::Cluster(k) if k.ix() < w.clusters.len() => {
            if w.clusters[k.ix()].army.is_some() {
                return Err(format!("{r} is an army: ask game.army"));
            }
            let ppl = people(&|c| matches!(c.origin, Origin::Materialized { cluster, .. } if cluster == k));
            Ok(json!({ "kind": "crowd", "ref": r, "node": crowd_json(&w, k), "people": ppl }))
        }
        Ref::Cell(id) if id.ix() < w.cells.len() => {
            let ppl = people(&|c| c.life.as_ref().is_some_and(|l| l.cell == Some(id)));
            Ok(json!({ "kind": "cell", "ref": r, "node": cell_json(&w, id), "people": ppl }))
        }
        Ref::Mob(m) if m.ix() < w.mobs.len() => {
            let ppl = people(&|c| c.life.as_ref().is_some_and(|l| l.mob == Some(m)));
            Ok(json!({ "kind": "mob", "ref": r, "node": mob_json(&w, &w.mobs[m.ix()]), "people": ppl }))
        }
        Ref::Place(_) | Ref::Cluster(_) | Ref::Cell(_) | Ref::Mob(_) => Err(format!("no such node {r}")),
        _ => Err(format!("{r} is not a node (pl:, cl:, ce: or mb:)")),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Person, army
// ---------------------------------------------------------------------------------------------------------------------

fn person(state: &Value, seat: Option<Seat>, a: &Args) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Characters, T::Demigods, T::Agents, T::Seats])?;
    let c = match a.need_ref("ref")? {
        Ref::Char(c) if c.ix() < w.characters.len() => c,
        Ref::Char(c) => return Err(format!("no such person {c}")),
        r => return Err(format!("{r} is not a person (`ch:` ref)")),
    };
    let x = &w.characters[c.ix()];
    let life = x.life.as_deref();
    let mut v = json!({
        "ref": Ref::Char(c), "name": x.name, "alive": x.alive,
        "role": life.map(|l| l.role), "rank": life.map(|l| &l.rank),
        "place": Ref::Place(x.place), "place_name": w.name_of(Ref::Place(x.place)),
        "region": region_name(&w, x.pos), "at": x.pos, "died_beat": life.and_then(|l| l.died),
    });
    let Some(d) = asker(&w, seat) else {
        return Ok(v);
    };
    let me = selves(&w, d);
    let following = w.demigods[d.ix()].following;
    // Yours: a body your agents steer, or someone your side brought into the story.
    let yours = me.contains(&Ref::Char(c)) || x.memorized_by.is_some_and(|m| me.contains(&m));
    let alleg: Vec<(DemigodId, f32)> =
        x.allegiance.iter().copied().filter(|(of, _)| yours || *of == d).collect();
    v["yours"] = yours.into();
    v["follower"] = x.groups.contains(following).into();
    v["allegiance"] = json!(alleg_json(&w, &alleg));
    v["memories"] = memories(state, c, &me, following, yours, a)?;
    Ok(v)
}

/// The memories of `c` a seat can see, newest first: all of them if the person is `yours`, else those about an
/// event the seat knows or told by one of its own. `{items: [{memory, beat, kind, text, about, row, confidence}],
/// next_cursor}`; `cursor` = the memory index to continue below.
fn memories(state: &Value, c: CharId, me: &[Ref], following: GroupId, yours: bool, a: &Args) -> Result<Value, String> {
    let limit = a.limit(20, 100)?;
    let items = state.pointer("/memory/items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let rows = rows(state);
    let owner = Ref::Char(c).to_string();
    let start = a.uint("cursor")?.unwrap_or(items.len()).min(items.len());
    let mut out = vec![];
    let mut next = None;
    for i in (0..start).rev() {
        if items[i].get("owner").and_then(Value::as_str) != Some(owner.as_str()) {
            continue;
        }
        let Ok(m) = MemoryItem::deserialize(&items[i]) else { continue };
        if m.forgotten || m.folded_into.is_some() || m.text.is_empty() {
            continue;
        }
        let seen = yours
            || m.from.is_some_and(|f| me.contains(&f))
            || m.ledger.and_then(|l| rows.get(l.ix())).and_then(row).is_some_and(|r| knows(me, following, &r));
        if !seen {
            continue;
        }
        if out.len() == limit {
            next = Some(i + 1);
            break;
        }
        out.push(json!({ "memory": m.id.0, "beat": m.beat, "kind": m.kind, "text": m.text, "about": m.about,
                         "row": m.ledger.map(Ref::Ledger), "confidence": m.confidence }));
    }
    Ok(json!({ "items": out, "next_cursor": next }))
}

fn army(state: &Value, seat: Option<Seat>, a: &Args) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Clusters, T::Characters, T::Cells, T::Demigods, T::Seats])?;
    let r = a.need_ref("ref")?;
    let (c, x) = match r {
        Ref::Cluster(k) => match w.clusters.get(k.ix()) {
            Some(c) => match &c.army {
                Some(x) => (c, x),
                None => return Err(format!("{r} is a crowd, not an army: ask game.node")),
            },
            None => return Err(format!("no such army {r}")),
        },
        _ => return Err(format!("{r} is not an army (`cl:` ref)")),
    };
    let owner = !x.rogue && asker(&w, seat) == Some(x.demigod);
    let live = fn7_society::armies(&w).any(|(k, _)| k.id == c.id);
    let mut v = army_marker(&w, c, x);
    let o = v.as_object_mut().expect("object");
    o.insert("live".into(), live.into());
    o.insert("yours".into(), owner.into());
    o.insert("supply".into(), json!(x.supply));
    o.insert("raised_beat".into(), json!(x.raised_beat));
    o.insert("battles".into(), json!(x.battles));
    o.insert("losses".into(), json!(x.losses));
    o.insert("general".into(), who_json(&w, x.general));
    o.insert("captains".into(), json!(x.captains.iter().map(|k| who_json(&w, Some(*k))).collect::<Vec<_>>()));
    o.insert("units".into(), json!(x.units.iter().filter(|u| w.cells.get(u.ix()).is_some_and(|c| c.ended.is_none())).count()));
    o.insert("order".into(), chain_json(&w, x, owner)["order"].clone());
    if owner {
        o.insert("orders".into(), json!(x.orders));
        o.insert("chain".into(), chain_json(&w, x, true));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------------------------------------------------
// Me, gods, standings, piles
// ---------------------------------------------------------------------------------------------------------------------

/// The seat view, built on a world whose ledger holds only the rows it reads: the last three beats' and the open
/// debts (it iterates both in order and never indexes the ledger).
fn me(state: &Value, seat: Option<Seat>) -> Result<Value, String> {
    let seat = seat.ok_or("`me` needs a seat (ask over the WebSocket)")?;
    let mut w = world(
        state,
        &[
            T::Places, T::Groups, T::Clusters, T::Characters, T::Demigods, T::Items, T::Agents, T::Gods, T::Seats,
            T::Mods, T::Nodes, T::Cells, T::Mobs, T::Battles, T::Slow,
        ],
    )?;
    if seat_state(&w, seat).is_none() {
        return Err(format!("seat {seat} has no demigod"));
    }
    #[derive(Deserialize)]
    struct Beat {
        beat: u32,
    }
    let since = w.meta.beat.saturating_sub(2);
    let all = rows(state);
    let tail = all.iter().rposition(|v| Beat::deserialize(v).is_ok_and(|b| b.beat < since)).map_or(0, |i| i + 1);
    let mut kept: Vec<LedgerRow> = all[..tail]
        .iter()
        .filter(|v| {
            v.get("debt").and_then(|d| d.get("status")).and_then(Value::as_str).is_some_and(|s| s == "open" || s == "delayed")
        })
        .filter_map(row)
        .collect();
    kept.extend(all[tail..].iter().filter_map(row));
    w.ledger.rows = kept;
    Ok(crate::phases::seat_view(&w, seat))
}

fn gods(state: &Value, pantheon: &Pantheon) -> Result<Value, String> {
    let w = world(state, &[T::Gods])?;
    let gods: Vec<Value> = payload::gods(pantheon)
        .into_iter()
        .enumerate()
        .map(|(i, mut g)| {
            let health = w.gods.get(i).map(|x| x.health);
            g["health"] = json!(health);
            g["unmade"] = health.is_some_and(|h| h <= 0.0).into();
            g
        })
        .collect();
    Ok(json!({ "gods": gods }))
}

fn standings(state: &Value) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Groups, T::Clusters, T::Characters, T::Demigods, T::Gods])?;
    let mut ds: Vec<(&Demigod, f64)> = w.demigods.iter().map(|d| (d, crate::phases::score(&w, d))).collect();
    // Never by health (hidden): alive first, then who fell last, then score, then id.
    ds.sort_by(|(a, sa), (b, sb)| {
        b.alive
            .cmp(&a.alive)
            .then(b.eliminated_beat.unwrap_or(u32::MAX).cmp(&a.eliminated_beat.unwrap_or(u32::MAX)))
            .then(sb.total_cmp(sa))
            .then(a.id.cmp(&b.id))
    });
    let rows: Vec<Value> = ds
        .iter()
        .enumerate()
        .map(|(i, (d, s))| {
            json!({ "rank": i + 1, "demigod": Ref::Demigod(d.id), "name": d.name,
                    "seat": (d.seat < crate::SYNTHETIC_SEAT).then_some(d.seat), "alive": d.alive,
                    "eliminated_beat": d.eliminated_beat, "cause": d.cause, "score": s, "bolts": d.bolts })
        })
        .collect();
    Ok(json!({ "beat": w.meta.beat, "beats_total": w.meta.beats_total, "alive": w.alive_demigods().count(),
               "over": crate::phases::match_over(&w, w.meta.beat), "standings": rows }))
}

fn piles_final(state: &Value) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Characters, T::Demigods, T::Items, T::Agents, T::Rulings])?;
    if !crate::phases::match_over(&w, w.meta.beat) {
        return Err(format!(
            "piles_final: the match is not over (beat {} of {})",
            w.meta.beat, w.meta.beats_total
        ));
    }
    #[derive(Deserialize)]
    struct Lite {
        actor: Ref,
        #[serde(default)]
        debt: Option<Debt>,
    }
    let lite: Vec<Lite> = rows(state).iter().filter_map(|v| Lite::deserialize(v).ok()).collect();
    let piles: Vec<Value> = w
        .demigods
        .iter()
        .map(|d| {
            let me = selves(&w, d.id);
            let acts = lite.iter().filter(|r| me.contains(&r.actor)).count();
            let paid = crate::phases::debt_paid(d.id, lite.iter().filter_map(|r| r.debt.as_ref()));
            crate::phases::pile_with(&w, d, &me, acts, paid)
        })
        .collect();
    Ok(json!({ "beat": w.meta.beat, "piles": piles }))
}
