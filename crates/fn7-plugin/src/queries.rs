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
             "One hierarchy node by ref: a place (planet, region, city, district: its node, child nodes and a page of its crowds), a crowd (`cl:`, not an army), a cell (`ce:`) or a mob (`mb:`), with the people known there. Rules 0.8.0: or a hex (`cell`: a hex cell key) zoomed into: its state now, observed or extrapolated from the DM's memory.",
             obj(json!({ "ref": r#ref(r"^(pl|cl|ce|mb):\d+$", "The node (or give `cell`)."),
                         "cell": { "type": "string", "description": "Rules 0.8.0: a hex cell key (\"<face>:<level>:<q>:<r>\") instead of `ref`." },
                         "cursor": cursor, "limit": limit(20, 100) }), &[])),
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
        spec("present", true,
             "Exact resume (state schema 8): the present moment: the server's clock, the public events of the instant actions since the last resolve (casts, summonings, challenge.lock, duel.blow) and every open duel in full (fighters, hp, range, initiative, stances, every blow so far with its choreography and stamps, deadline and clock). The same object as match.snapshot.public_state.present. Loads the whole state: ask it when you (re)connect, don't poll it. null in older states.",
             obj(json!({}), &[])),
        spec("media", true,
             "An image of something in the game, by reference (never a free-text prompt): a portrait of a demigod or person, a demigod's diorama (its miniature on a hex tile, the same in every match), a place, a public event, or an army. Answers at once with `image` (pending, then ready via GET /media/index); the server writes the prompt from the state. Visibility: a seat may ask for what it can see (public things, its own people, events it knows); over HTTP only public refs. Rate-limited per seat and budgeted.",
             obj(json!({ "kind": { "type": "string", "enum": ["portrait", "diorama", "place", "event", "army"], "description": "What to draw." },
                         "ref": r#ref(r"^(dg|ch|pl|lg|cl):\d+$", "portrait: `dg:`/`ch:`; diorama: `dg:`; place: `pl:`; event: `lg:`; army: `cl:` (an army).") }),
                 &["kind", "ref"])),
        spec("items", true,
             "Rules 0.7.0: a page of items, most recent first. Public items (manifested things of tier relic or up, or deployed) for everyone; the asking seat's own items in full. Optional filters: holder (a `dg:` ref), state, min_tier.",
             obj(json!({ "holder": r#ref(r"^dg:\d+$", "Only items this demigod holds."),
                         "state": { "type": "string", "enum": ["held", "deployed", "destroyed", "transferred", "stolen"] },
                         "min_tier": { "type": "string", "enum": ["trinket", "artifact", "relic", "wonder", "legend", "myth", "titan", "apocalypse"] },
                         "cursor": cursor, "limit": limit(20, 100) }), &[])),
        spec("item", true,
             "Rules 0.7.0: one item (`it:`) with its manifestation (verdict, provenance) and custody history. A private item only for its holder's seat.",
             obj(json!({ "ref": r#ref(r"^it:\d+$", "The item.") }), &["ref"])),
        spec("memory", true,
             "Rules 0.8.0: the Dungeon Master's memory of a place: every public decision about it, or anything above or below it (cell → settlement → region → world), at or before a beat, newest first.",
             obj(json!({ "place": { "type": "string", "description": "A hex cell key (\"0:people:12:-3\"), a `pl:` ref (settlement or region) or \"world\"." },
                         "until_beat": { "type": ["integer", "string"], "minimum": 0, "description": "Only decisions made at or before this beat (default: now)." },
                         "limit": limit(50, 200) }), &["place"])),
        spec("encounters", true,
             "Rules 0.8.0: the encounter hexes on the board: hidden ones only as {id, cell, marked}; revealed ones with their type and outcome. Optionally one region's.",
             obj(json!({ "region": r#ref(r"^pl:\d+$", "Only encounters in this region.") }), &[])),
        spec("earth", true,
             "The Earth simulator (state schema 13): the voxel field for a view. `cell` (a hex key of any level: the board's centre and, unless `level` says otherwise, its level) or `region` (a `pl:` ref): the client's BOARD, `board` = 22 rings (`rings` fewer) = 1,519 records of 9 bytes (h16, top, water, lava, ash, fire, snow, heat, crack) in ring order, base64; `since` (a beat): only the records that changed since then. `tiles` (degrees, 4 = the client's globe): the whole planet as rings of latitude, 9 bytes a tile (2,578 at 4°; `layout: \"grid\"` for a full raster); `since` works here too. No args: every regions cell that differs from base (`field`, packed); `bbox`: a box of regions cells; `base: true` adds `globe` (every cell's h, top, fuel); `grid: true` adds the grid. `hexes: true` / `field: true` with a centre: the verbose JSON hexes / regions columns. `beat`: the field as it stood then. `phenomena`: at most 20 (`max_phenomena`), the nearest to a board's centre first, else the biggest. See docs/EARTH_SIM.md.",
             obj(json!({ "cell": { "type": "string", "description": "A hex cell key (\"<face>:<level>:<q>:<r>\"): the board's centre." },
                         "region": r#ref(r"^pl:\d+$", "A region (or any place): the board's centre."),
                         "bbox": { "type": ["array", "string"], "description": "[west, south, east, north] in degrees (regions level)." },
                         "level": { "type": "string", "enum": ["regions", "villages", "houses", "people"], "description": "The zoom level (default: the cell key's, else regions)." },
                         "rings": { "type": ["integer", "string"], "minimum": 0, "maximum": 22, "description": "Board rings (default and most 22)." },
                         "since": { "type": ["integer", "string"], "minimum": 0, "description": "With a board or tiles: only what changed since this beat." },
                         "tiles": { "type": ["integer", "string"], "minimum": 2, "maximum": 10, "description": "The globe in tiles of this many degrees." },
                         "layout": { "type": "string", "enum": ["rings", "grid"], "description": "Tiles: rings of latitude of about equal tiles (default) or a full raster." },
                         "coarse": { "type": ["boolean", "string"], "description": "Board: add the regions cell index under each hex (u16 LE, base64)." },
                         "hexes": { "type": ["boolean", "string"], "description": "With a centre below regions: verbose JSON hexes instead of the board." },
                         "field": { "type": ["boolean", "string"], "description": "With a centre at regions: the cells as columns instead of the board." },
                         "max_phenomena": { "type": ["integer", "string"], "minimum": 0, "description": "Phenomena sent at most (default 20)." },
                         "beat": { "type": ["integer", "string"], "minimum": 0, "description": "A past beat (default: now)." },
                         "base": { "type": ["boolean", "string"], "description": "true: add `globe` (h, top and fuel of every regions cell of the globe, about 27,000, in index order)." },
                         "grid": { "type": ["boolean", "string"], "description": "true: add the grid (face, q, r of every cell in index order)." },
                         "format": { "type": "string", "enum": ["pack", "columns", "json"], "description": "Cell encoding of `field` (default pack)." } }), &[])),
        spec("earth_at", true,
             "The Earth simulator, time travel: the whole field (every cell that differs from base, and the phenomena) as it stood at the end of a past beat, rebuilt from the newest keyframe at or before it by re-simulating at most `earth_keyframe_beats` beats of the recorded inputs.",
             obj(json!({ "beat": { "type": ["integer", "string"], "minimum": 0, "description": "The beat." },
                         "format": { "type": "string", "enum": ["pack", "columns", "json"], "description": "Cell encoding (default pack)." } }), &["beat"])),
        spec("fault", true,
             "State schema 16 (the hover preview while a mega is armed): what a mega aimed at `at` would get: great-circle distance to the nearest fault line (`km`), its `tier` (junction, fault, near, mild, tiny), the junction pulling on it (`{name?, kind, at, strength, plates, km}` or null) and the multipliers `factor: {magnitude (added), radius, damage}` under this match's rules. Read-only, deterministic, cheap (reads only the settings).",
             obj(json!({ "at": { "type": ["array", "string"], "description": "[lon, lat] in degrees (over HTTP `at=lon,lat`; or give `lon` and `lat`)." },
                         "lon": { "type": ["number", "string"], "description": "Instead of `at`." },
                         "lat": { "type": ["number", "string"], "description": "Instead of `at`." } }), &[])),
        spec("faults_overview", true,
             "State schema 16, for the globe overlay while a mega is armed: every plate-boundary junction (`{lon, lat, name?, kind: triple|knot, strength 0..1, plates}`, strongest first), the tier thresholds and the curve's settings. The same for the whole match: ask once.",
             obj(json!({}), &[])),
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
        "powers" => powers(pantheon, state),
        "gods" => gods(state, pantheon),
        "standings" => standings(state),
        "piles_final" => piles_final(state),
        "items" => items(state, seat, &args),
        "item" => item(state, seat, &args),
        "media" => media(state, seat, &args),
        "memory" => memory(state, &args),
        "encounters" => encounters(state, &args),
        "earth" => crate::earth::view::query_earth(state, args.0),
        "earth_at" => crate::earth::view::query_earth_at(state, args.0),
        "fault" => crate::faults::query_fault(state, args.0),
        "faults_overview" => crate::faults::query_overview(state),
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
    Manifests,
    Dm,
    DmLite,
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
        manifests: table(state, "manifests", has(T::Manifests))?,
        // Rules 0.8.0: the DM's state. Its memory (the big part) only for the queries that read it; demigods carry
        // their story pieces, so the rest of it comes with them.
        dm: if has(T::Dm) { table(state, "dm", true)? } else { dm_lite(state, has(T::Demigods) || has(T::DmLite))? },
    })
}

/// The DM's state without its memory (the queries that only show the board).
fn dm_lite(state: &Value, want: bool) -> Result<fn7_model::dm::Dm, String> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Lite {
        settlements: Vec<fn7_model::dm::Settlement>,
        stories: Vec<fn7_model::dm::Story>,
        encounters: Vec<fn7_model::dm::Encounter>,
        loot: Vec<fn7_model::dm::LootPile>,
        trails: Vec<fn7_model::dm::Trail>,
        shrines: Vec<fn7_model::dm::Shrine>,
        /// Rules 0.9.0: the book of quests and curses (small).
        wills: Value,
        /// Schema 11: the look records and the item art (small).
        looks: Value,
        /// Schema 13: the Earth simulator's state (tens of KB at most).
        earth: Option<Box<fn7_model::earth::EarthState>>,
    }
    let l: Lite = table(state, "dm", want)?;
    Ok(fn7_model::dm::Dm { memory: vec![], settlements: l.settlements, stories: l.stories, encounters: l.encounters,
                           loot: l.loot, trails: l.trails, shrines: l.shrines, duels: vec![], wills: l.wills, looks: l.looks, mark: None,
                           earth: l.earth, scenes: vec![] })
}

/// Memory entries passing `keep` (tested on the unparsed entry), oldest first: only those are deserialized.
fn memory_where(state: &Value, keep: impl Fn(&Value) -> bool) -> Vec<fn7_model::dm::DmEntry> {
    state
        .pointer("/dm/memory")
        .and_then(Value::as_array)
        .map(|m| m.iter().filter(|e| keep(e)).filter_map(|e| fn7_model::dm::DmEntry::deserialize(e).ok()).collect())
        .unwrap_or_default()
}

fn str_of<'a>(e: &'a Value, k: &str) -> &'a str {
    e.get(k).and_then(Value::as_str).unwrap_or("")
}

/// The memory a hex's state reads: its observations and its parent's roll-ups.
fn hex_memory(w: &mut World, state: &Value, key: &str) {
    let Some(c) = fn7_model::hexgrid::HexCell::parse(key) else { return };
    let wh = crate::dm::locate(w, c.center(), c.level);
    let parent = Ref::Place(wh.settlement.unwrap_or(wh.region)).to_string();
    w.dm.memory = memory_where(state, |e| {
        (str_of(e, "kind") == "place.observed" && str_of(e, "place") == key)
            || (str_of(e, "kind") == "memory.rollup" && str_of(e, "place") == parent)
    });
}

/// The latest `memory.rollup` entry about `key`, read from the end of the unparsed memory (only it is deserialized).
fn latest_rollup(state: &Value, key: &str) -> Option<fn7_model::dm::DmEntry> {
    let mem = state.pointer("/dm/memory")?.as_array()?;
    mem.iter()
        .rev()
        .find(|e| e.get("kind").and_then(Value::as_str) == Some("memory.rollup") && e.get("place").and_then(Value::as_str) == Some(key))
        .and_then(|e| fn7_model::dm::DmEntry::deserialize(e).ok())
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
    seat.and_then(|s| seat_state(w, s)).map(|s| w.lead_of_seat(s.seat).unwrap_or(s.demigod))
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
    // Demigods and characters: event image prompts carry the visual sheets of who is in them.
    let w = world(state, &[T::Places, T::Clusters, T::Gods, T::Demigods, T::Characters])?;
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

/// The ledger rows a hotspot reads (the last `hotspots::WINDOW` beats; at most [`SCAN_CAP`] rows back).
fn recent_rows(w: &World, state: &Value) -> Vec<LedgerRow> {
    #[derive(Deserialize)]
    struct Beat {
        beat: u32,
    }
    let since = w.meta.beat.saturating_sub(crate::hotspots::WINDOW - 1);
    let all = rows(state);
    let mut out: Vec<LedgerRow> = all
        .iter()
        .rev()
        .take(SCAN_CAP)
        .take_while(|v| Beat::deserialize(*v).is_ok_and(|b| b.beat >= since))
        .filter(|v| is_public(w, v))
        .filter_map(row)
        .collect();
    out.reverse();
    out
}

/// Every region with its hotspot (rules 0.7.0): `[{ref, name, center, hotspot: {pos, radius_km, score, why} | null}]`.
fn regions_with_hotspots(w: &World, state: &Value) -> Vec<Value> {
    let spots = crate::hotspots::hotspots(w, &recent_rows(w, state));
    w.places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .map(|p| json!({ "ref": Ref::Place(p.id), "name": p.name, "center": p.center,
                         "hotspot": spots.get(&p.id).map(|h| h.to_json()) }))
        .collect()
}

fn globe(state: &Value, a: &Args) -> Result<Value, String> {
    let w = world(
        state,
        &[T::Places, T::Clusters, T::Characters, T::Demigods, T::Gods, T::Mods, T::Seats, T::Items, T::Manifests, T::Mobs, T::Battles, T::DmLite],
    )?;
    let events = event_page(&w, state, a.uint("since")?, a.limit(30, 200)?, None);
    let mut v = json!({
        "beat": w.meta.beat,
        "beats_total": w.meta.beats_total,
        "final_night": w.meta.beat >= w.meta.beats_total,
        "arena": crate::phases::arena_json(&w),
        "alive": w.alive_demigods().count(),
        "entropy": payload::entropy_json(&w, None),
        "demigods": w.demigods.iter().map(|d| crate::phases::demigod_public(&w, d)).collect::<Vec<_>>(),
        "players": payload::players_json(&w),
        "regions": regions_with_hotspots(&w, state),
        "influence": payload::influence_json(&w),
        "mods": payload::mods_json(&w),
        "armies": fn7_society::armies(&w).map(|(c, x)| army_marker(&w, c, x)).collect::<Vec<_>>(),
        "events": events,
    });
    if w.dm_rules() {
        v["roads"] = crate::dm::roads_under(&w, None).into();
        v["shrines"] = crate::dm::shrines_under(&w, None).into();
        crate::dm::add_time_of_day(&mut v, w.meta.beat, &crate::world::settings(&w));
    }
    Ok(v)
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
        &[T::Places, T::Nodes, T::Clusters, T::Characters, T::Cells, T::Mobs, T::Demigods, T::Gods, T::Mods, T::Seats, T::Items, T::Manifests, T::Battles, T::DmLite],
    )?;
    let p = region_ref(&w, a.need_ref("ref")?)?;
    let hotspot = crate::hotspots::hotspots(&w, &recent_rows(&w, state)).get(&p).map(|h| h.to_json());
    let pl = &w.places[p.ix()];
    let cities: Vec<Value> = w
        .places
        .iter()
        .filter(|c| c.parent == Some(p))
        .map(|c| node_json(&w, c.id))
        .collect();
    let here = |at: LonLat| fn7_society::geo::region_at(&w, at) == p;
    let reg = Ref::Place(p).to_string();
    let mut v = json!({
        "ref": Ref::Place(p),
        "name": pl.name,
        "center": pl.center,
        "radius_km": pl.radius_km,
        "node": node_json(&w, p),
        "hotspot": hotspot,
        "cities": cities,
        "crowds": page(crowds_under(&w, p), a.uint("cursor")?.unwrap_or(0), a.limit(10, 50)?, |c| crowd_json(&w, c.id)),
        "mobs": w.mobs.iter().filter(|m| m.ended.is_none() && fn7_society::sim::hier::under(&w, m.place, p))
            .map(|m| mob_json(&w, m)).collect::<Vec<_>>(),
        "armies": fn7_society::armies(&w).filter(|(_, x)| here(x.pos)).map(|(c, x)| army_marker(&w, c, x)).collect::<Vec<_>>(),
        "demigods": w.alive_demigods().filter(|d| here(d.pos)).map(|d| crate::phases::demigod_public(&w, d)).collect::<Vec<_>>(),
        "mods": payload::mods_json(&w).into_iter().filter(|m| m["region_ref"] == reg.as_str()).collect::<Vec<_>>(),
        "influence": payload::influence_json(&w).into_iter().filter(|m| m["region_ref"] == reg.as_str()).collect::<Vec<_>>(),
        "events": event_page(&w, state, None, 10, Some(p)),
    });
    add_dm(&w, state, &mut v, Some(p), true);
    Ok(v)
}

/// Rules 0.8.0: the DM's pieces under a place (`None`: the whole board): encounters, loot, shrines and (with
/// `roads`) roads.
fn add_dm(w: &World, state: &Value, v: &mut Value, p: Option<PlaceId>, roads: bool) {
    if !w.dm_rules() {
        return;
    }
    v["encounters"] = crate::dm::encounters_under(w, p).into();
    v["loot"] = crate::dm::loot_under(w, p).into();
    v["shrines"] = crate::dm::shrines_under(w, p).into();
    if roads {
        v["roads"] = crate::dm::roads_under(w, p).into();
    }
    // The integrated truth beneath this place: its latest roll-up (counts, notable events, state now).
    let key = p.filter(|p| *p != w.meta.arena).map(|p| Ref::Place(p).to_string()).unwrap_or_else(|| "world".into());
    v["memory_summary"] = latest_rollup(state, &key).as_ref().map(crate::dm::entry_json).unwrap_or(Value::Null);
    v["observed_beat"] = w.meta.beat.into();
    v["extrapolated"] = false.into();
    crate::dm::add_time_of_day(v, w.meta.beat, &crate::world::settings(w));
}

/// Rules 0.8.0: a hex zoomed into: its state now (observed this beat, or extrapolated from its last observation).
fn cell_node(w: &World, key: &str) -> Result<Value, String> {
    if !w.dm_rules() {
        return Err("hex cells are known from rules 0.8.0 (state schema 6) on".into());
    }
    let (node, observed, extrapolated) = crate::dm::cell_state(w, key, w.meta.beat)?;
    Ok(json!({ "kind": "hex", "cell": key, "node": node, "observed_beat": observed, "extrapolated": extrapolated,
               "beat": w.meta.beat }))
}

/// The palette; rules 0.8.0 adds the god-tier powers.
fn powers(pantheon: &Pantheon, state: &Value) -> Result<Value, String> {
    let mut v = payload::powers_json();
    if let Some(w) = world(state, &[]).ok().filter(|w| w.dm_rules()) {
        let at = v.len().saturating_sub(1);
        v.splice(at..at, crate::godtier::powers_json(pantheon, &crate::world::settings(&w)));
    }
    Ok(json!({ "powers": v }))
}

fn memory(state: &Value, a: &Args) -> Result<Value, String> {
    let mut w = world(state, &[T::Places, T::DmLite, T::Nodes, T::Items, T::Manifests, T::Demigods])?;
    let place = match a.get("place") {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(_) => return Err("`place` must be a string: a hex cell key, a `pl:` ref or \"world\"".into()),
        None => return Err("`place` is required".into()),
    };
    let until = a.uint("until_beat")?.map(|b| b as u32).unwrap_or(w.meta.beat);
    let limit = a.limit(50, 200)?;
    let ancestors = crate::dm::ancestors_of_key(&w, &place)?;
    // Only the entries about this place (or above or below it) are deserialized, newest first, up to the page.
    let mut items: Vec<Value> = vec![];
    if let Some(mem) = state.pointer("/dm/memory").and_then(Value::as_array) {
        for e in mem.iter().rev() {
            if items.len() >= limit {
                break;
            }
            if e.get("public").and_then(Value::as_bool) != Some(true) || e.get("beat").and_then(Value::as_u64).unwrap_or(0) > until as u64 {
                continue;
            }
            let p = str_of(e, "place");
            let up = e.get("up").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
            if p == place || up.iter().any(|u| u.as_str() == Some(place.as_str())) || ancestors.iter().any(|a| a == p) {
                if let Ok(x) = fn7_model::dm::DmEntry::deserialize(e) {
                    items.push(crate::dm::entry_json(&x));
                }
            }
        }
    }
    if fn7_model::hexgrid::HexCell::parse(&place).is_some() {
        hex_memory(&mut w, state, &place);
    }
    let mut v = json!({ "place": place, "ancestors": ancestors, "until_beat": until, "beat": w.meta.beat, "items": items });
    // A hex: its state at that beat too (observed or extrapolated).
    if fn7_model::hexgrid::HexCell::parse(&place).is_some() {
        let (node, observed, extrapolated) = crate::dm::cell_state(&w, &place, until)?;
        v["state"] = node;
        v["observed_beat"] = json!(observed);
        v["extrapolated"] = extrapolated.into();
    } else {
        v["observed_beat"] = until.into();
        v["extrapolated"] = false.into();
    }
    Ok(v)
}

fn encounters(state: &Value, a: &Args) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::DmLite, T::Items, T::Manifests, T::Demigods])?;
    let region = match a.r#ref("region")? {
        None => None,
        Some(r) => Some(region_ref(&w, r)?),
    };
    let list = crate::dm::encounters_under(&w, region);
    let revealed = list.iter().filter(|e| e.get("revealed").is_some()).count();
    Ok(json!({ "beat": w.meta.beat, "region": region.map(Ref::Place), "hidden": list.len() - revealed,
               "revealed": revealed, "encounters": list }))
}

fn node(state: &Value, a: &Args) -> Result<Value, String> {
    if let Some(Value::String(key)) = a.get("cell") {
        let mut w = world(state, &[T::Places, T::Nodes, T::Demigods, T::Items, T::Manifests, T::DmLite])?;
        hex_memory(&mut w, state, key.trim());
        return cell_node(&w, key.trim());
    }
    let w = world(state, &[T::Places, T::Nodes, T::Clusters, T::Characters, T::Cells, T::Mobs, T::Demigods, T::Gods, T::DmLite, T::Items, T::Manifests])?;
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
            let mut v = json!({ "kind": "place", "ref": r, "node": node_json(&w, p), "children": children, "crowds": crowds });
            add_dm(&w, state, &mut v, Some(p), true);
            Ok(v)
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
        "visual": crate::visual::person(&w, x).to_json(),
    });
    let Some(d) = asker(&w, seat) else {
        return Ok(v);
    };
    let me = selves(&w, d);
    let following = w.demigods[d.ix()].following;
    // Yours: a body your agents steer, or someone your side brought into the story.
    let yours = me.contains(&Ref::Char(c)) || x.memorized_by.is_some_and(|m| me.contains(&m));
    let alleg: Vec<(DemigodId, f32)> =
        x.allegiance.iter().copied().filter(|(of, _)| yours || w.same_player(*of, d)).collect();
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
    let owner = !x.rogue && asker(&w, seat).is_some_and(|d| w.same_player(d, x.demigod));
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
            T::Mods, T::Nodes, T::Cells, T::Mobs, T::Battles, T::Slow, T::Manifests,
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
    let w = world(state, &[T::Places, T::Groups, T::Clusters, T::Characters, T::Demigods, T::Gods, T::Seats])?;
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
            let mut v = json!({ "rank": i + 1, "demigod": Ref::Demigod(d.id), "name": d.name,
                    "seat": (d.seat < crate::SYNTHETIC_SEAT).then_some(d.seat), "alive": d.alive,
                    "eliminated_beat": d.eliminated_beat, "cause": d.cause, "score": s, "bolts": d.bolts });
            crate::table::add_table_fields(&w, d, &mut v);
            v
        })
        .collect();
    let mut v = json!({ "beat": w.meta.beat, "beats_total": w.meta.beats_total, "alive": w.alive_demigods().count(),
               "over": crate::phases::match_over(&w, w.meta.beat), "standings": rows });
    if w.demigods.iter().any(|d| d.side.is_some()) {
        // Rules 0.7.0: the players ranked (each with both demigods' state; never health).
        v["players_alive"] = w.alive_seats().len().into();
        v["players"] = crate::phases::ranked_players(&w)
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let first = w.demigod_of_seat(s).expect("a player has a demigod");
                json!({ "rank": i + 1, "seat": (s < crate::SYNTHETIC_SEAT).then_some(s), "player": first.player_name(),
                        "aura": payload::aura(&w, s), "alive": w.seat_alive(s), "bolts": first.bolts,
                        "demigods": w.bodies_of_seat(s).map(|d| json!({ "ref": Ref::Demigod(d.id), "side": d.side,
                            "name": d.name, "alive": d.alive, "eliminated_beat": d.eliminated_beat, "cause": d.cause }))
                            .collect::<Vec<_>>() })
            })
            .collect();
    }
    Ok(v)
}

/// Can the asking seat see this item? Public (a manifested thing of tier relic or up, or deployed), or held by one of
/// the seat's demigods.
fn item_visible(w: &World, it: &Item, me: Option<DemigodId>) -> bool {
    let public = it.ext.manifestation.is_some()
        && (it.ext.tier >= fn7_model::manifest::PUBLIC_TIER || it.ext.state == fn7_model::manifest::ItemState::Deployed);
    public
        || me.is_some_and(|d| {
            it.holders.last().is_some_and(|h| matches!(h, Ref::Demigod(x) if w.same_player(*x, d)))
        })
}

fn items(state: &Value, seat: Option<Seat>, a: &Args) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Demigods, T::Items, T::Manifests, T::Seats, T::Characters, T::Clusters])?;
    let me = asker(&w, seat);
    let holder = a.r#ref("holder")?;
    let state_f = a.get("state").and_then(Value::as_str).map(str::to_string);
    let min_tier = match a.get("min_tier").and_then(Value::as_str) {
        None => None,
        Some(t) => Some(
            serde_json::from_value::<fn7_model::manifest::Tier>(json!(t)).map_err(|_| format!("`min_tier` {t:?} is not a tier"))?,
        ),
    };
    let list: Vec<&Item> = w
        .items
        .iter()
        .rev()
        .filter(|i| item_visible(&w, i, me))
        .filter(|i| holder.is_none_or(|h| i.holders.last() == Some(&h)))
        .filter(|i| state_f.as_deref().is_none_or(|s| json!(i.ext.state) == json!(s)))
        .filter(|i| min_tier.is_none_or(|t| i.ext.tier >= t))
        .collect();
    let mut v = page(list, a.uint("cursor")?.unwrap_or(0), a.limit(20, 100)?, |i| crate::items::item_json(&w, i));
    v["beat"] = w.meta.beat.into();
    Ok(v)
}

fn item(state: &Value, seat: Option<Seat>, a: &Args) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Demigods, T::Items, T::Manifests, T::Seats, T::Characters, T::Clusters])?;
    let id = match a.need_ref("ref")? {
        Ref::Item(i) if i.ix() < w.items.len() => i,
        Ref::Item(i) => return Err(format!("no such item {i}")),
        r => return Err(format!("{r} is not an item (`it:` ref)")),
    };
    let it = &w.items[id.ix()];
    let me = asker(&w, seat);
    if !item_visible(&w, it, me) {
        return Err(format!("{} is not something you can see", Ref::Item(id)));
    }
    let mut v = crate::items::item_json(&w, it);
    let manifests: Vec<fn7_model::manifest::Manifestation> = table(state, "manifests", true)?;
    if let Some(m) = it.ext.manifestation.and_then(|m| manifests.get(m.ix())) {
        let mine = me.is_some_and(|d| w.same_player(d, m.demigod));
        v["manifestation"] = crate::items::manifest_json(&w, m, mine);
        v["rows"] = json!(m.rows.iter().map(|r| Ref::Ledger(*r)).collect::<Vec<_>>());
    }
    v["origin"] = crate::payload::item_origin(&it.origin).0.into();
    v["created_by"] = json!(it.created_by);
    v["holders"] = json!(it.holders);
    v["history"] = json!(it.ext.history);
    Ok(v)
}

fn piles_final(state: &Value) -> Result<Value, String> {
    let w = world(state, &[T::Places, T::Characters, T::Demigods, T::Items, T::Manifests, T::Agents, T::Rulings])?;
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
            let me = crate::world::body_selves(&w, d.id);
            let acts = lite.iter().filter(|r| me.contains(&r.actor)).count();
            let paid = crate::phases::debt_paid(d.id, lite.iter().filter_map(|r| r.debt.as_ref()));
            crate::phases::pile_with(&w, d, &me, acts, paid)
        })
        .collect();
    Ok(json!({ "beat": w.meta.beat, "piles": piles }))
}

// ---------------------------------------------------------------------------------------------------------------------
// Media (images by reference)
// ---------------------------------------------------------------------------------------------------------------------

/// What [`media`] reads from a ledger row: who is in it.
#[derive(Deserialize)]
struct Cast<'a> {
    verb: &'a str,
    magnitude: u8,
    source: Source,
    actor: Ref,
    #[serde(default)]
    targets: Vec<Ref>,
}

/// Is person `c` public? Seeded notables and paladins, the generals and captains of armies (the globe shows them), and
/// anyone in a public event among the last [`SCAN_CAP`] ledger rows.
pub(crate) fn person_public(w: &World, state: &Value, c: CharId) -> bool {
    let x = &w.characters[c.ix()];
    if matches!(x.origin, Origin::Seeded | Origin::Paladin { .. }) {
        return true;
    }
    if w.clusters.iter().filter_map(|k| k.army.as_ref()).any(|a| a.general == Some(c) || a.captains.contains(&c)) {
        return true;
    }
    let me = Ref::Char(c);
    rows(state).iter().rev().take(SCAN_CAP).any(|v| {
        Cast::deserialize(v).is_ok_and(|h| {
            (h.actor == me || h.targets.contains(&me)) && payload::is_public_head(w, h.verb, h.magnitude, &h.source, h.actor)
        })
    })
}

/// Does demigod `d` know person `c`? Public, or one of its own (a body its agents steer, someone its side brought into
/// the story, a follower), or in its current view, or in a ledger row it knows (the last [`SCAN_CAP`] rows).
pub(crate) fn person_known(w: &World, state: &Value, d: DemigodId, c: CharId) -> bool {
    if person_public(w, state, c) {
        return true;
    }
    let me = selves(w, d);
    let x = &w.characters[c.ix()];
    let following = w.demigods[d.ix()].following;
    if me.contains(&Ref::Char(c))
        || x.memorized_by.is_some_and(|m| me.contains(&m) || fn7_society::demigod_of(w, m) == Some(d))
        || x.groups.contains(following)
    {
        return true;
    }
    if let Some(s) = crate::world::seat_of(w, d) {
        if crate::world::visible(w, d, s.zoom, s.focus, 60).contains(&Ref::Char(c)) {
            return true;
        }
    }
    let r = Ref::Char(c);
    rows(state).iter().rev().take(SCAN_CAP).any(|v| {
        Cast::deserialize(v).is_ok_and(|h| h.actor == r || h.targets.contains(&r))
            && row(v).is_some_and(|row| knows(&me, following, &row))
    })
}

/// `game.media {kind, ref}`: the subject of an image of something in the game, written from the state, after the
/// visibility check. `{kind, ref, name, image_prompt, image_kind, visual?}`; the server's media wrapper turns it into
/// `{kind, ref, name, image, visual?}` (and enforces the rate limit and budgets). Cheap: partial tables, bounded scans.
fn media(state: &Value, seat: Option<Seat>, a: &Args) -> Result<Value, String> {
    let kind = match a.get("kind").and_then(Value::as_str).map(str::trim) {
        Some(k @ ("portrait" | "diorama" | "place" | "event" | "army")) => k.to_string(),
        Some(k) => return Err(format!("`kind` {k:?} is not portrait, diorama, place, event or army")),
        None => return Err("`kind` is required (portrait, diorama, place, event or army)".into()),
    };
    let r = a.need_ref("ref")?;
    let hidden = || format!("{r} is not something you can see");
    let out = |name: String, subject: String, visual: Option<Value>| {
        let mut v = json!({ "kind": kind, "ref": r, "name": name, "image_prompt": subject, "image_kind": kind });
        if let Some(x) = visual {
            v["visual"] = x;
        }
        v
    };
    match (kind.as_str(), r) {
        ("portrait", Ref::Demigod(d)) => {
            let w = world(state, &[T::Demigods])?;
            let x = w.demigods.get(d.ix()).ok_or_else(|| format!("no such demigod {r}"))?;
            Ok(out(x.name.clone(), payload::portrait_prompt(&w, x), Some(crate::visual::demigod(&w, x).to_json())))
        }
        ("diorama", Ref::Demigod(d)) => {
            let w = world(state, &[T::Demigods, T::Places])?;
            let x = w.demigods.get(d.ix()).ok_or_else(|| format!("no such demigod {r}"))?;
            Ok(out(x.name.clone(), payload::diorama_prompt(&w, x), Some(crate::visual::demigod(&w, x).to_json())))
        }
        ("portrait", Ref::Char(c)) => {
            let w = world(
                state,
                &[T::Places, T::Groups, T::Clusters, T::Characters, T::Demigods, T::Agents, T::Seats, T::Nodes, T::Cells, T::Mobs],
            )?;
            if c.ix() >= w.characters.len() {
                return Err(format!("no such person {r}"));
            }
            let ok = match asker(&w, seat) {
                Some(d) => person_known(&w, state, d, c),
                None => person_public(&w, state, c),
            };
            if !ok {
                return Err(hidden());
            }
            let x = &w.characters[c.ix()];
            Ok(out(x.name.clone(), payload::person_portrait_prompt(&w, x), Some(crate::visual::person(&w, x).to_json())))
        }
        ("place", Ref::Place(p)) => {
            let w = world(state, &[T::Places])?;
            let x = w.places.get(p.ix()).ok_or_else(|| format!("no such place {r}"))?;
            Ok(out(x.name.clone(), payload::place_prompt(&w, x), None))
        }
        ("event", Ref::Ledger(l)) => {
            let w = world(state, &[T::Places, T::Clusters, T::Gods, T::Demigods, T::Characters, T::Agents, T::Seats])?;
            let v = rows(state).get(l.ix()).ok_or_else(|| format!("no such event {r}"))?;
            let row = row(v).ok_or_else(|| format!("{r} could not be read"))?;
            let ok = payload::is_public_event(&w, &row)
                || asker(&w, seat).is_some_and(|d| knows(&selves(&w, d), w.demigods[d.ix()].following, &row));
            if !ok {
                return Err(hidden());
            }
            let (_, region) = payload::geotag(&w, &row);
            Ok(out(row.act.clone(), payload::event_subject(&w, &row, region.as_deref()), None))
        }
        ("army", Ref::Cluster(k)) => {
            let w = world(state, &[T::Clusters, T::Demigods])?;
            let c = w.clusters.get(k.ix()).ok_or_else(|| format!("no such army {r}"))?;
            let x = c.army.as_ref().ok_or_else(|| format!("{r} is a crowd, not an army"))?;
            Ok(out(c.label.clone(), payload::army_prompt(&w, c, x), None))
        }
        ("portrait", _) => Err(format!("a portrait is of a demigod or person (`dg:`/`ch:`), not {r}")),
        ("diorama", _) => Err(format!("a diorama is of a demigod (`dg:`), not {r}")),
        ("place", _) => Err(format!("a place image needs a `pl:` ref, not {r}")),
        ("event", _) => Err(format!("an event image needs a ledger row (`lg:`), not {r}")),
        _ => Err(format!("an army image needs an army (`cl:`), not {r}")),
    }
}
