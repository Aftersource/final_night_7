//! THE TREE OF CONSEQUENCE (`game.memory_tree`): the DM's place × time memory drawn as the exact tree it is, for the
//! overlay a judge opens to ask "what's the data structure like".
//!
//! Read-only, deterministic and cheap, like every query (docs/CLIENT_API.md "Tree of consequence"):
//!
//! - **The hierarchy** from the world down to a focus: world → region → settlement (or the region's open country) →
//!   villages hex (3 km) → houses hex (30 m) → people hex (3 m), and beside the hexes the society branch of a planet
//!   city: district → crowd → cell → person (and a focused demigod's story hangs under the hex it stands on). Each node
//!   counts the DM memory entries at it or rolled up into it (`facts_total`), their density, beats and kinds, a few
//!   samples, which children rolled up into it, and its latest `memory.rollup` with the exact entries it cites.
//!   The society branch counts the minds' memories (`World::memory`, fn7-memory) instead: a person's own, rolled up
//!   into its cell, crowd and district.
//! - **Recall paths**: for a few granular facts at the focus, how each is recalled: the query key → the stored roll →
//!   the memory entry → the roll-ups that cite it → the prompts that carry it now (the real `dm::prompt_lines`, the
//!   earth line, a person's rendered memory).
//! - **Totals**: entries by scope and kind, ledger rows, persons, observed hexes, and the state's size by table.
//!
//! **Bounded.** The DM memory is read newest first, at most [`SCAN_CAP`] entries (all of them up to beat ~60), each by
//! its head on the unparsed state; the minds' store the same (its newest [`SCAN_CAP`] items); ledger rows only by index.
//! Each reply says what it read (`bounds`) and whether that was everything (`complete`).
//!
//! **Hidden information.** Private entries and memories are counted (`private_facts`), never shown, except to the
//! seat they belong to (a private DM entry's `data.seats`; a memory the seat may see by the `game.person` rule).
//!
//! **Past beats.** The memory is append-only and beat-stamped, so `beat: N` gives the memory exactly as it stood at the
//! end of beat N; the society's structure and the earth are the live ones (`as_of` says so). A whole past state comes
//! from the admin state at a round.

use std::cell::RefCell;
use std::collections::BTreeMap;

use fn7_model::dm::WORLD_KEY;
use fn7_model::hexgrid::{cell_at, people_cell, HexCell, HexLevel};
use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use theatre_plugin_api::Seat;

use crate::earth::here::Ground;
use crate::queries::{asker, is_public, rows, world, SCAN_CAP, T};
use crate::settings::Settings;
use crate::world::{knows, selves};

/// Tiers below the world a tree may show.
pub const MAX_DEPTH: u32 = 6;
/// Samples and collapsed siblings per node, at most (and by default).
pub const MAX_PER_NODE: usize = 12;
pub const DEFAULT_PER_NODE: usize = 6;
/// "The busiest region now": the most facts in the last this many beats.
pub const BUSY_BEATS: u32 = 8;
/// The planet's surface (km²).
const WORLD_KM2: f64 = 510_072_000.0;
/// Kinds the DM's prompt leaves out (`dm::prompt_lines`).
const NOT_IN_PROMPT: [&str; 3] = ["dice.rolled", "place.observed", "memory.rollup"];

/// The tiers, for the client's legend.
fn levels_json() -> Value {
    json!([
        { "level": 0, "kinds": ["world"], "name": "World", "scale": "the planet", "store": "dm" },
        { "level": 1, "kinds": ["region"], "name": "Region", "scale": "a world region (a city arena: a district)", "store": "dm" },
        { "level": 2, "kinds": ["settlement", "open"], "name": "Settlement", "scale": "a city or village, or the region's open country", "store": "dm" },
        { "level": 3, "kinds": ["hex", "district"], "name": "Villages hex / city district", "scale": "3 km hex; a city's district (society)", "store": "dm | minds" },
        { "level": 4, "kinds": ["hex", "crowd"], "name": "Houses hex / crowd", "scale": "30 m hex; a crowd of thousands", "store": "dm | minds" },
        { "level": 5, "kinds": ["hex", "cell"], "name": "People hex / cell", "scale": "3 m hex; a cell of 10-500 people", "store": "dm | minds" },
        { "level": 6, "kinds": ["person", "demigod"], "name": "Person", "scale": "one person, or a demigod's story", "store": "minds | dm" },
    ])
}

// ---------------------------------------------------------------------------------------------------------------------
// Reading the stores (heads only)
// ---------------------------------------------------------------------------------------------------------------------

fn st<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn beat_of(v: &Value) -> u32 {
    v.get("beat").and_then(Value::as_u64).unwrap_or(0) as u32
}

/// A private DM entry is its seat's own: `data.seats` holds the seat (or `data.seat` is it).
fn seat_private(v: &Value, seat: Seat) -> bool {
    let d = &v["data"];
    d["seats"].as_array().is_some_and(|a| a.iter().any(|s| s.as_u64() == Some(seat as u64))) || d["seat"].as_u64() == Some(seat as u64)
}

/// One DM memory entry, read by its head from the unparsed state.
struct Fact<'a> {
    id: u64,
    beat: u32,
    scope: &'a str,
    place: &'a str,
    up: Vec<&'a str>,
    kind: &'a str,
    query: Option<&'a str>,
    public: bool,
    /// Public, or private to the asking seat.
    shown: bool,
    raw: &'a Value,
}

/// What [`Fact`] reads of an entry, borrowed from the unparsed state (its text and data stay where they are).
#[derive(Deserialize)]
struct FactHead<'a> {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    beat: u32,
    #[serde(default, borrow)]
    scope: &'a str,
    #[serde(default, borrow)]
    place: &'a str,
    #[serde(default, borrow)]
    up: Vec<&'a str>,
    #[serde(default, borrow)]
    kind: &'a str,
    #[serde(default, borrow)]
    query: Option<&'a str>,
    #[serde(default)]
    public: bool,
}

impl<'a> Fact<'a> {
    fn read(v: &'a Value, seat: Option<Seat>) -> Option<Fact<'a>> {
        let h = FactHead::deserialize(v).ok()?;
        Some(Fact {
            id: h.id,
            beat: h.beat,
            scope: h.scope,
            place: h.place,
            up: h.up,
            kind: h.kind,
            query: h.query,
            public: h.public,
            shown: h.public || seat.is_some_and(|s| seat_private(v, s)),
            raw: v,
        })
    }
    fn text(&self) -> &'a str {
        st(self.raw, "text")
    }
    /// The region key it lies under (`None`: a world fact).
    fn region(&self) -> Option<&'a str> {
        if self.place == WORLD_KEY {
            None
        } else if self.up.len() <= 1 {
            Some(self.place)
        } else {
            Some(self.up[self.up.len() - 2])
        }
    }
    /// Its slot under its region: `None` at the region itself, else a settlement key or the open-country key.
    fn settlement(&self) -> Option<String> {
        let r = self.region()?;
        if self.place == r {
            return None;
        }
        match self.scope {
            "cell" if self.up.len() >= 3 => Some(self.up[0].to_string()),
            "cell" => Some(open_key(r)),
            _ => Some(self.place.to_string()),
        }
    }
    /// Does `memory.rollup` at `key` count it (dm::integrate: its ancestors, and its own place unless it is a hex)?
    fn rolls_into(&self, key: &str) -> bool {
        self.public
            && self.kind != "memory.rollup"
            && self.kind != "place.observed"
            && (self.up.contains(&key) || (self.place == key && self.scope != "cell"))
    }
    fn json(&self) -> Value {
        json!({ "id": self.id, "beat": self.beat, "scope": self.scope, "place": self.place, "up": self.up, "kind": self.kind,
                "query_key": self.query, "public": self.public, "text": self.text() })
    }
    /// A sample line: its text (an observation, which has none, as what was seen).
    fn sample(&self) -> Value {
        let text = if self.kind == "place.observed" {
            let d = &self.raw["data"];
            format!("observed: {} people, {} buildings, {}", d["people"].as_u64().unwrap_or(0), d["buildings"].as_u64().unwrap_or(0),
                    d["weather"].as_str().unwrap_or("calm"))
        } else {
            self.text().to_string()
        };
        let mut v = json!({ "id": self.id, "beat": self.beat, "kind": self.kind, "text": text, "public": self.public, "place": self.place });
        if let Some(q) = self.query {
            v["query_key"] = q.into();
        }
        v
    }
}

fn open_key(region: &str) -> String {
    format!("open:{region}")
}

/// One mind's memory (`World::memory`), read by its head.
struct Mind<'a> {
    id: u64,
    owner: &'a str,
    beat: u32,
    kind: &'static str,
    ledger: Option<usize>,
    /// Neither forgotten nor folded by the asked beat, and with its text kept.
    live: bool,
    raw: &'a Value,
}

/// What [`Mind`] reads of a memory item, borrowed.
#[derive(Deserialize)]
struct MindHead<'a> {
    #[serde(default)]
    id: u64,
    #[serde(default, borrow)]
    owner: &'a str,
    #[serde(default)]
    beat: u32,
    #[serde(default, borrow)]
    kind: &'a str,
    #[serde(default, borrow)]
    text: &'a str,
    #[serde(default)]
    ledger: Option<u64>,
    #[serde(default)]
    forgotten: bool,
    #[serde(default)]
    folded_into: Option<u64>,
    #[serde(default)]
    folded_beat: Option<u32>,
}

impl<'a> Mind<'a> {
    fn read(v: &'a Value, b: u32) -> Option<Mind<'a>> {
        let h = MindHead::deserialize(v).ok()?;
        let folded = match (h.folded_into, h.folded_beat) {
            (_, Some(f)) => f <= b,
            (Some(_), None) => true,
            _ => false,
        };
        Some(Mind {
            id: h.id,
            owner: h.owner,
            beat: h.beat,
            kind: mind_kind(h.kind),
            ledger: h.ledger.map(|l| l as usize),
            live: !folded && !h.forgotten && !h.text.is_empty(),
            raw: v,
        })
    }
}

/// A memory kind as the tree's kind (`mind.<kind>`).
fn mind_kind(k: &str) -> &'static str {
    match k {
        "witnessed" => "mind.witnessed",
        "did" => "mind.did",
        "told" => "mind.told",
        "rumor" => "mind.rumor",
        "inherited" => "mind.inherited",
        "planted" => "mind.planted",
        "summary" => "mind.summary",
        "report" => "mind.report",
        _ => "mind.other",
    }
}

/// The hex at `l` above (or at) a cell key: itself at its own level, the hex containing its centre at a coarser one,
/// `None` at a finer one (or for a key that is no hex).
fn hex_above(place: &str, l: HexLevel) -> Option<String> {
    let c = HexCell::parse(place)?;
    if c.level == l {
        Some(place.to_string())
    } else if c.level < l {
        Some(cell_at(c.center(), l).key())
    } else {
        None
    }
}

fn hex_slot(l: HexLevel) -> usize {
    match l {
        HexLevel::Villages => 0,
        HexLevel::Houses => 1,
        _ => 2,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Aggregates and nodes
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Agg {
    total: u32,
    here: u32,
    private: u32,
    first: Option<u32>,
    last: Option<u32>,
    kinds: BTreeMap<String, u32>,
}

impl Agg {
    fn add(&mut self, beat: u32, kind: &str, public: bool, here: bool) {
        self.total += 1;
        self.here += here as u32;
        self.private += !public as u32;
        self.first = Some(self.first.map_or(beat, |f| f.min(beat)));
        self.last = Some(self.last.map_or(beat, |l| l.max(beat)));
        match self.kinds.get_mut(kind) {
            Some(n) => *n += 1,
            None => {
                self.kinds.insert(kind.to_string(), 1);
            }
        }
    }
    fn families(&self) -> BTreeMap<String, u32> {
        let mut f: BTreeMap<String, u32> = BTreeMap::new();
        for (k, n) in &self.kinds {
            *f.entry(k.split('.').next().unwrap_or(k).to_string()).or_default() += n;
        }
        f
    }
}

struct Node {
    id: String,
    kind: &'static str,
    level: u32,
    name: String,
    store: &'static str,
    agg: Agg,
    children_total: usize,
    area_km2: Option<f64>,
    on_path: bool,
    collapsed: bool,
    extra: Map<String, Value>,
    samples: Vec<Value>,
    rolled: Vec<Value>,
    rollup: Value,
    children: Vec<Node>,
}

impl Node {
    fn new(id: String, kind: &'static str, level: u32, name: String, store: &'static str) -> Node {
        Node { id, kind, level, name, store, agg: Agg::default(), children_total: 0, area_km2: None, on_path: false, collapsed: true,
               extra: Map::new(), samples: vec![], rolled: vec![], rollup: Value::Null, children: vec![] }
    }
    /// The node as the reply carries it (built by moving its parts in: `json!` would copy every subtree again).
    fn json(self, span: u32) -> Value {
        let a = &self.agg;
        // Densities span a dozen orders of magnitude (the planet to a 3 m hex): per million km² and as log10 per km²,
        // so two decimals keep them all.
        let per_km2 = self.area_km2.filter(|x| *x > 0.0).map(|x| a.total as f64 / x);
        let log10 = per_km2.filter(|d| *d > 0.0).map(f64::log10);
        let mut v = json!({
            "id": self.id, "kind": self.kind, "level": self.level, "name": self.name, "store": self.store,
            "children_total": self.children_total, "facts_total": a.total, "facts_here": a.here, "private_facts": a.private,
            "density": { "per_beat": a.total as f64 / span.max(1) as f64, "per_million_km2": per_km2.map(|d| d * 1e6),
                         "log10_per_km2": log10, "area_m2": self.area_km2.map(|x| x * 1e6) },
            "first_beat": a.first, "last_beat": a.last, "kinds": a.kinds, "families": a.families(),
            "on_path": self.on_path, "collapsed": self.collapsed,
        });
        let o = v.as_object_mut().expect("object");
        o.insert("sample_facts".into(), Value::Array(self.samples));
        o.insert("rolled_up_from".into(), Value::Array(self.rolled));
        o.insert("rollup".into(), self.rollup);
        o.insert("children".into(), Value::Array(self.children.into_iter().map(|c| c.json(span)).collect()));
        for (k, x) in self.extra {
            o.insert(k, x);
        }
        v
    }
}

/// The tiers of the DM's place branch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lv {
    World,
    Region,
    Settlement,
    Villages,
    Houses,
    People,
}

impl Lv {
    fn level(self) -> u32 {
        match self {
            Lv::World => 0,
            Lv::Region => 1,
            Lv::Settlement => 2,
            Lv::Villages => 3,
            Lv::Houses => 4,
            Lv::People => 5,
        }
    }
    fn child(self) -> Option<Lv> {
        match self {
            Lv::World => Some(Lv::Region),
            Lv::Region => Some(Lv::Settlement),
            Lv::Settlement => Some(Lv::Villages),
            Lv::Villages => Some(Lv::Houses),
            Lv::Houses => Some(Lv::People),
            Lv::People => None,
        }
    }
    fn hex(self) -> Option<HexLevel> {
        match self {
            Lv::Villages => Some(HexLevel::Villages),
            Lv::Houses => Some(HexLevel::Houses),
            Lv::People => Some(HexLevel::People),
            _ => None,
        }
    }
}

/// Where the tree is drilled to: each tier's key (a missing one is the busiest child).
#[derive(Default, Debug, Clone)]
struct Path {
    region: Option<String>,
    settlement: Option<String>,
    villages: Option<String>,
    houses: Option<String>,
    people: Option<String>,
    quarter: Option<PlaceId>,
    crowd: Option<ClusterId>,
    cell: Option<CellId>,
    person: Option<CharId>,
    demigod: Option<DemigodId>,
}

impl Path {
    fn at(&self, lv: Lv) -> Option<&String> {
        match lv {
            Lv::World => None,
            Lv::Region => self.region.as_ref(),
            Lv::Settlement => self.settlement.as_ref(),
            Lv::Villages => self.villages.as_ref(),
            Lv::Houses => self.houses.as_ref(),
            Lv::People => self.people.as_ref(),
        }
    }
    fn set(&mut self, lv: Lv, k: String) {
        match lv {
            Lv::World => {}
            Lv::Region => self.region = Some(k),
            Lv::Settlement => self.settlement = Some(k),
            Lv::Villages => self.villages = Some(k),
            Lv::Houses => self.houses = Some(k),
            Lv::People => self.people = Some(k),
        }
    }
}

struct Ctx<'a> {
    state: &'a Value,
    w: &'a World,
    st: Settings,
    b: u32,
    now: u32,
    depth: u32,
    per_node: usize,
    span: u32,
    facts: Vec<Fact<'a>>,
    /// Each fact's villages, houses and people hex, worked out when first asked.
    hexes: RefCell<Vec<[Option<Option<String>>; 3]>>,
    minds: Vec<Mind<'a>>,
    by_owner: BTreeMap<Ref, Vec<usize>>,
    /// The minds' window holds the whole store.
    minds_complete: bool,
    /// The DM prompt's lines per region key, once worked out.
    prompt_cache: RefCell<BTreeMap<String, Vec<String>>>,
    rows: &'a [Value],
    public_rows: RefCell<BTreeMap<usize, bool>>,
    rows_read: RefCell<usize>,
    /// The asking seat's selves and following (for the memories it may see).
    me: Option<(Vec<Ref>, GroupId)>,
    ground: Option<Ground>,
    prompt_world: RefCell<Option<World>>,
}

impl<'a> Ctx<'a> {
    fn hex(&self, i: usize, l: HexLevel) -> Option<String> {
        let j = hex_slot(l);
        if let Some(h) = self.hexes.borrow().get(i).and_then(|x| x[j].clone()) {
            return h;
        }
        let h = hex_above(self.facts[i].place, l);
        self.hexes.borrow_mut()[i][j] = Some(h.clone());
        h
    }
    /// The society's cells, unparsed (`ce:N` is index N): the few the tree shows are read from here.
    fn cells(&self) -> &'a [Value] {
        self.state.get("cells").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
    }
    fn name(&self, key: &str) -> String {
        if key == WORLD_KEY {
            return "the world".into();
        }
        if let Some(r) = key.strip_prefix("open:") {
            return format!("the open country of {}", self.name(r));
        }
        match key.parse::<Ref>() {
            Ok(r) => self.ref_name(r),
            Err(_) => key.to_string(),
        }
    }
    /// A ref's name (a cell's from the unparsed cells: the table is not loaded).
    fn ref_name(&self, r: Ref) -> String {
        match r {
            Ref::Cell(c) => self.cells().get(c.ix()).map(|v| st(v, "name").to_string()).unwrap_or_else(|| "a cell".into()),
            _ => self.w.name_of(r),
        }
    }
    fn place(&self, key: &str) -> Option<&'a Place> {
        match key.parse::<Ref>() {
            Ok(Ref::Place(p)) => self.w.places.get(p.ix()),
            _ => None,
        }
    }
    fn row_public(&self, ix: usize) -> bool {
        if let Some(b) = self.public_rows.borrow().get(&ix) {
            return *b;
        }
        *self.rows_read.borrow_mut() += 1;
        let b = self.rows.get(ix).is_some_and(|v| is_public(self.w, v));
        self.public_rows.borrow_mut().insert(ix, b);
        b
    }
    /// A mind's memory the asker may see: a live one about a public event; for a seat also the ones `game.person`
    /// shows it (its own people's, told by its own, about an event it knows).
    fn mind_shown(&self, m: &Mind) -> bool {
        if !m.live {
            return false;
        }
        if m.ledger.is_some_and(|l| self.row_public(l)) {
            return true;
        }
        let Some((me, following)) = &self.me else { return false };
        let owner = m.owner.parse::<Ref>().ok();
        let yours = owner.is_some_and(|o| {
            me.contains(&o)
                || matches!(o, Ref::Char(c) if self.w.characters.get(c.ix()).and_then(|x| x.memorized_by).is_some_and(|b| me.contains(&b)))
        });
        let from = m.raw.get("from").and_then(Value::as_str).and_then(|s| s.parse::<Ref>().ok()).is_some_and(|f| me.contains(&f));
        yours
            || from
            || m.ledger.and_then(|l| self.rows.get(l)).and_then(|v| LedgerRow::deserialize(v).ok()).is_some_and(|r| knows(me, *following, &r))
    }
    fn mind_public(&self, m: &Mind) -> bool {
        m.live && m.ledger.is_some_and(|l| self.row_public(l))
    }
    fn mind_sample(&self, m: &Mind) -> Value {
        let mut v = json!({ "id": m.id, "beat": m.beat, "kind": m.kind, "text": st(m.raw, "text"),
                            "public": self.mind_public(m), "owner": m.owner });
        if let Some(l) = m.ledger {
            v["row"] = json!(Ref::Ledger(LedgerId::from_ix(l)));
        }
        if let Some(c) = m.raw.get("confidence").and_then(Value::as_f64) {
            v["confidence"] = c.into();
        }
        v
    }
    fn earth_line(&self, at: LonLat) -> Option<String> {
        self.ground.as_ref().and_then(|g| g.line("The earth here, now", at))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The query
// ---------------------------------------------------------------------------------------------------------------------

fn uint(args: &Value, k: &str) -> Result<Option<u64>, String> {
    match args.get(k).filter(|v| !v.is_null() && v.as_str() != Some("")) {
        None => Ok(None),
        Some(Value::Number(n)) => n.as_u64().map(Some).ok_or_else(|| format!("`{k}` must be a non-negative integer")),
        Some(Value::String(s)) => s.trim().parse::<u64>().map(Some).map_err(|_| format!("`{k}` must be a non-negative integer")),
        Some(_) => Err(format!("`{k}` must be a non-negative integer")),
    }
}

/// `game.memory_tree {focus?, beat?, depth?, per_node?}`.
pub fn query(state: &Value, seat: Option<Seat>, args: &Value) -> Result<Value, String> {
    // The agents only for a seat (its selves); the rest for names, publicity and the society branch.
    let mut t = vec![T::Places, T::Clusters, T::Characters, T::Demigods, T::Gods, T::Seats, T::DmLite];
    if seat.is_some() {
        t.push(T::Agents);
    }
    let w = world(state, &t)?;
    if !w.dm_rules() {
        return Err("the memory tree needs the DM's memory (rules 0.8.0, state schema 6)".into());
    }
    let now = w.meta.beat;
    let b = match uint(args, "beat")? {
        None => now,
        Some(x) if x as u32 <= now => x as u32,
        Some(x) => return Err(format!("`beat` {x} has not happened yet (now: beat {now})")),
    };
    let depth = match uint(args, "depth")? {
        None => MAX_DEPTH,
        Some(d) if (1..=MAX_DEPTH as u64).contains(&d) => d as u32,
        Some(_) => return Err(format!("`depth` must be 1..{MAX_DEPTH}")),
    };
    let per_node = match uint(args, "per_node")? {
        None => DEFAULT_PER_NODE,
        Some(n) if n as usize <= MAX_PER_NODE => n as usize,
        Some(_) => return Err(format!("`per_node` must be 0..{MAX_PER_NODE}")),
    };
    let focus = match args.get("focus").filter(|v| !v.is_null() && v.as_str() != Some("")) {
        None => None,
        Some(Value::String(s)) => Some(s.trim().to_string()),
        Some(_) => return Err("`focus` must be a string: a hex cell key, a `pl:`, `dg:` or `ch:` ref".into()),
    };
    let st = crate::world::settings(&w);

    // The DM memory, newest first back to SCAN_CAP entries (the memory is beat-ordered: entries past `b` are skipped
    // by a binary search, never read).
    let mem: &[Value] = state.pointer("/dm/memory").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let end = mem.partition_point(|e| beat_of(e) <= b);
    let start = end.saturating_sub(SCAN_CAP);
    let facts: Vec<Fact> = mem[start..end].iter().filter_map(|e| Fact::read(e, seat)).filter(|f| f.beat <= b).collect();
    let dm_complete = start == 0;
    let from_beat = if dm_complete { 0 } else { facts.first().map_or(b, |f| f.beat) };
    // The minds' store, the same way (newest SCAN_CAP items written by `b`).
    let items: &[Value] = state.pointer("/memory/items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let mstart = items.len().saturating_sub(SCAN_CAP);
    let minds: Vec<Mind> = items[mstart..].iter().filter_map(|v| Mind::read(v, b)).filter(|m| m.beat <= b).collect();
    let mut by_owner: BTreeMap<Ref, Vec<usize>> = BTreeMap::new();
    for (i, m) in minds.iter().enumerate() {
        if let Ok(r) = m.owner.parse::<Ref>() {
            by_owner.entry(r).or_default().push(i);
        }
    }
    let me = asker(&w, seat).map(|d| (selves(&w, d), w.demigods[d.ix()].following));
    let ground = if b == now { Ground::of(&w, &st) } else { None };
    let n = facts.len();
    let ctx = Ctx {
        state, w: &w, st, b, now, depth, per_node, span: b + 1 - from_beat.min(b), facts,
        hexes: RefCell::new(vec![[None, None, None]; n]), minds, by_owner, minds_complete: mstart == 0, rows: rows(state), public_rows: RefCell::new(BTreeMap::new()),
        rows_read: RefCell::new(0), me, ground, prompt_world: RefCell::new(None), prompt_cache: RefCell::new(BTreeMap::new()),
    };

    let (path, focus_json) = resolve_focus(&ctx, focus.as_deref())?;
    let all: Vec<usize> = (0..ctx.facts.len()).collect();
    let mut path = path;
    let tree = dm_node(&ctx, &mut path, Lv::World, WORLD_KEY.to_string(), &all, true, "");
    let recall = recall_paths(&ctx, &path);
    let totals = totals(&ctx, &tree.agg, state, mem.len(), items.len());
    let society_now = b == now;
    let span = ctx.span;
    let mut v = json!({
        "beat": now,
        "as_of": { "beat": b, "memory": "exact", "society": if society_now { "exact" } else { "now" },
                   "earth": if b == now { "exact" } else { "omitted" } },
        "depth": depth,
        "per_node": per_node,
        "visibility": { "seat": seat, "rule": "public facts shown; private ones counted (private_facts), shown only to their own seat" },
        "bounds": {
            "scan_cap": SCAN_CAP,
            "dm_window": { "from_beat": from_beat, "to_beat": b, "entries": ctx.facts.len(), "complete": dm_complete },
            "minds_window": { "from_beat": ctx.minds.first().map(|m| m.beat), "to_beat": b, "items": ctx.minds.len(), "complete": ctx.minds_complete },
            "ledger_rows_read": *ctx.rows_read.borrow(),
        },
    });
    let o = v.as_object_mut().expect("object");
    o.insert("focus".into(), focus_json);
    o.insert("levels".into(), levels_json());
    o.insert("tree".into(), tree.json(span));
    o.insert("recall_paths".into(), Value::Array(recall));
    o.insert("totals".into(), totals);
    Ok(v)
}

/// The focus as a path through the tiers, and its description.
fn resolve_focus(ctx: &Ctx, focus: Option<&str>) -> Result<(Path, Value), String> {
    let w = ctx.w;
    let mut p = Path::default();
    let pl = |id: PlaceId| Ref::Place(id).to_string();
    let hex_path = |p: &mut Path, at: LonLat, level: HexLevel, key: String| {
        let wh = crate::dm::locate(w, at, level);
        p.region = Some(pl(wh.region));
        p.settlement = Some(wh.settlement.map(pl).unwrap_or_else(|| open_key(&pl(wh.region))));
        if level <= HexLevel::Villages {
            p.villages = Some(if level == HexLevel::Villages { key.clone() } else { cell_at(at, HexLevel::Villages).key() });
        }
        if level <= HexLevel::Houses {
            p.houses = Some(if level == HexLevel::Houses { key.clone() } else { cell_at(at, HexLevel::Houses).key() });
        }
        if level == HexLevel::People {
            p.people = Some(key);
        }
    };
    let Some(f) = focus else {
        let r = busiest_region(ctx);
        let name = r.as_deref().map(|k| ctx.name(k));
        p.region = r.clone();
        return Ok((p, json!({ "arg": null, "default": true, "kind": "region", "id": r, "name": name,
                              "why": format!("the busiest region now: the most facts in the last {BUSY_BEATS} beats") })));
    };
    if let Some(c) = HexCell::parse(f) {
        hex_path(&mut p, c.center(), c.level, f.to_string());
        return Ok((p, json!({ "arg": f, "default": false, "kind": "hex", "id": f, "name": format!("{} hex", c.level.as_str()) })));
    }
    let r: Ref = f.parse().map_err(|_| format!("`focus` `{f}`: a hex cell key, a `pl:`, `dg:` or `ch:` ref"))?;
    let (kind, name) = match r {
        Ref::Place(id) => {
            let x = w.places.get(id.ix()).ok_or_else(|| format!("no such place {r}"))?;
            match x.kind {
                PlaceKind::Arena => {
                    p.region = busiest_region(ctx);
                }
                PlaceKind::District => p.region = Some(pl(id)),
                PlaceKind::Site => {
                    p.region = Some(pl(fn7_society::geo::district_of(w, id)));
                    p.settlement = Some(pl(id));
                }
                PlaceKind::Quarter => {
                    p.region = Some(pl(fn7_society::geo::district_of(w, id)));
                    p.settlement = x.parent.map(pl);
                    p.quarter = Some(id);
                }
            }
            ("place", x.name.clone())
        }
        Ref::Demigod(d) => {
            let x = w.demigods.get(d.ix()).ok_or_else(|| format!("no such demigod {r}"))?;
            hex_path(&mut p, x.pos, HexLevel::People, people_cell(x.pos));
            p.demigod = Some(d);
            ("demigod", x.name.clone())
        }
        Ref::Char(c) => {
            let x = w.characters.get(c.ix()).ok_or_else(|| format!("no such person {r}"))?;
            hex_path(&mut p, x.pos, HexLevel::People, people_cell(x.pos));
            p.person = Some(c);
            if let Origin::Materialized { cluster, .. } = x.origin {
                p.crowd = Some(cluster);
                if let Some(k) = w.clusters.get(cluster.ix()).filter(|k| k.army.is_none()) {
                    if w.places.get(k.place.ix()).is_some_and(|q| q.kind == PlaceKind::Quarter) {
                        p.quarter = Some(k.place);
                        // The society branch hangs under the quarter's city: the tree follows it there.
                        let city = w.places[k.place.ix()].parent;
                        if city.map(pl) != p.settlement {
                            p.settlement = city.map(pl);
                            p.region = Some(pl(fn7_society::geo::district_of(w, k.place)));
                            p.villages = None;
                            p.houses = None;
                            p.people = None;
                        }
                    }
                }
            }
            p.cell = x.life.as_ref().and_then(|l| l.cell);
            ("person", x.name.clone())
        }
        _ => return Err(format!("`focus` {r}: a hex cell key, a `pl:`, `dg:` or `ch:` ref")),
    };
    Ok((p, json!({ "arg": f, "default": false, "kind": kind, "id": f, "name": name })))
}

/// The region with the most facts in the last [`BUSY_BEATS`] beats (then in the whole window; ties: the lower key).
fn busiest_region(ctx: &Ctx) -> Option<String> {
    let pick = |since: u32| {
        let mut n: BTreeMap<&str, u32> = BTreeMap::new();
        for f in ctx.facts.iter().filter(|f| f.beat >= since) {
            if let Some(r) = f.region().filter(|r| r.starts_with("pl:")) {
                *n.entry(r).or_default() += 1;
            }
        }
        n.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0))).map(|x| x.0.to_string())
    };
    pick(ctx.b.saturating_sub(BUSY_BEATS - 1)).or_else(|| pick(0)).or_else(|| {
        ctx.w.places.iter().find(|p| p.kind == PlaceKind::District).map(|p| Ref::Place(p.id).to_string())
    })
}

/// Groups `idx` by `slot` (`None`: the fact is at the node itself): the node's aggregate and its children's facts.
fn split<'a>(ctx: &Ctx<'a>, idx: &[usize], slot: impl Fn(usize) -> Option<String>) -> (Agg, BTreeMap<String, Vec<usize>>) {
    let mut agg = Agg::default();
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &i in idx {
        let f = &ctx.facts[i];
        match slot(i) {
            None => agg.add(f.beat, f.kind, f.public, true),
            Some(k) => {
                agg.add(f.beat, f.kind, f.public, false);
                groups.entry(k).or_default().push(i);
            }
        }
    }
    (agg, groups)
}

/// Children by facts, busiest first (ties: the key).
fn ranked(groups: &BTreeMap<String, Vec<usize>>) -> Vec<(&String, usize)> {
    let mut v: Vec<(&String, usize)> = groups.iter().map(|(k, x)| (k, x.len())).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v
}

fn hex_area_km2(l: HexLevel) -> f64 {
    let s = l.size_m() / 1000.0;
    1.5 * 3f64.sqrt() * s * s
}

/// One node of the DM's place branch (`full`: on the path, with samples, its roll-up and, above `depth`, children).
fn dm_node(ctx: &Ctx, path: &mut Path, lv: Lv, key: String, idx: &[usize], full: bool, parent_name: &str) -> Node {
    let w = ctx.w;
    let slot = |i: usize| -> Option<String> {
        let f = &ctx.facts[i];
        match lv {
            Lv::World => f.region().map(str::to_string),
            Lv::Region => f.settlement(),
            Lv::Settlement => (f.scope == "cell").then(|| ctx.hex(i, HexLevel::Villages)).flatten(),
            Lv::Villages => ctx.hex(i, HexLevel::Houses),
            Lv::Houses => ctx.hex(i, HexLevel::People),
            Lv::People => None,
        }
    };
    let (agg, groups) = split(ctx, idx, slot);
    let (kind, name) = match lv {
        Lv::World => ("world", w.places.get(w.meta.arena.ix()).map(|p| p.name.clone()).unwrap_or_else(|| "the world".into())),
        Lv::Region => ("region", ctx.name(&key)),
        Lv::Settlement if key.starts_with("open:") => ("open", ctx.name(&key)),
        Lv::Settlement => ("settlement", ctx.name(&key)),
        _ => ("hex", format!("{} hex in {parent_name}", lv.hex().map(|l| l.as_str()).unwrap_or(""))),
    };
    let mut node = Node::new(key.clone(), kind, lv.level(), name.clone(), "dm");
    node.on_path = full;
    node.collapsed = !full;
    node.agg = agg;
    // Area, centre and what the field and the memory say about the place itself.
    match lv {
        Lv::World => node.area_km2 = Some(WORLD_KM2),
        Lv::Region | Lv::Settlement => {
            if let Some(p) = ctx.place(&key) {
                node.area_km2 = Some(std::f64::consts::PI * p.radius_km * p.radius_km);
                node.extra.insert("center".into(), json!(p.center));
                if full && lv == Lv::Settlement {
                    node.extra.insert("earth_line".into(), json!(ctx.earth_line(p.center)));
                }
            }
            if lv == Lv::Settlement {
                if let Some(s) = ctx.place(&key).and_then(|p| w.dm.settlement(p.id)) {
                    node.extra.insert("settlement_level".into(), s.level.into());
                }
            }
        }
        _ => {
            if let (Some(l), Some(c)) = (lv.hex(), HexCell::parse(&key)) {
                node.area_km2 = Some(hex_area_km2(l));
                node.extra.insert("hex_level".into(), l.as_str().into());
                node.extra.insert("center".into(), json!(c.center()));
                let observed = idx.iter().rev().map(|&i| &ctx.facts[i]).find(|f| f.kind == "place.observed" && f.place == key).map(|f| f.beat);
                node.extra.insert("observed_beat".into(), json!(observed));
                if full {
                    node.extra.insert("earth_line".into(), json!(ctx.earth_line(c.center())));
                }
            }
        }
    }
    if lv != Lv::World && lv.hex().is_none() {
        node.extra.insert("key".into(), key.clone().into());
    }
    // How many children it has: regions of the world, settlements of a region (+ its open country), the hexes the
    // memory knows below a settlement or hex (+ a city's districts), and who stands on a people hex.
    let quarters: Vec<PlaceId> = match (lv, ctx.place(&key)) {
        (Lv::Settlement, Some(p)) => w.places.iter().filter(|q| q.parent == Some(p.id) && q.kind == PlaceKind::Quarter).map(|q| q.id).collect(),
        _ => vec![],
    };
    let standing: Vec<DemigodId> = match lv {
        Lv::People => w.demigods.iter().filter(|d| people_cell(d.pos) == key).map(|d| d.id).collect(),
        _ => vec![],
    };
    node.children_total = match lv {
        Lv::World => w.places.iter().filter(|p| p.kind == PlaceKind::District).count(),
        Lv::Region => {
            let r = ctx.place(&key).map(|p| p.id);
            w.places.iter().filter(|p| p.kind == PlaceKind::Site && Some(fn7_society::geo::district_of(w, p.id)) == r).count()
                + groups.contains_key(&open_key(&key)) as usize
        }
        Lv::Settlement => groups.len() + quarters.len(),
        Lv::Villages | Lv::Houses => groups.len(),
        Lv::People => standing.len(),
    };
    if !full {
        return node;
    }
    // Samples: the newest facts shown here or below (a child's roll-ups and, above the hexes, observations left out).
    node.samples = idx
        .iter()
        .rev()
        .map(|&i| &ctx.facts[i])
        .filter(|f| f.shown && (f.kind != "memory.rollup" || f.place == key) && (f.kind != "place.observed" || lv.hex().is_some()))
        .take(ctx.per_node)
        .map(Fact::sample)
        .collect();
    let ranks = ranked(&groups);
    node.rolled = ranks
        .iter()
        .take(ctx.per_node)
        .map(|(k, n)| json!({ "from": k, "name": child_name(ctx, lv, k, &name, parent_name), "facts": n }))
        .collect();
    node.rollup = rollup_json(ctx, idx, &key);
    if lv.level() >= ctx.depth {
        return node;
    }
    if let Some(cl) = lv.child() {
        // Unasked, the busiest child; under a region the busiest real settlement (its society branch shows too),
        // the open country only when no settlement has a fact.
        let busiest = match lv {
            Lv::Region => ranks.iter().find(|(k, _)| !k.starts_with("open:")).or(ranks.first()),
            _ => ranks.first(),
        };
        let want = path.at(cl).cloned().or_else(|| busiest.map(|(k, _)| (*k).clone()));
        if let Some(k) = &want {
            path.set(cl, k.clone());
        }
        let empty = vec![];
        let mut kids: Vec<(String, bool)> = vec![];
        if let Some(k) = &want {
            kids.push((k.clone(), true));
        }
        for (k, _) in ranks.iter().filter(|(k, _)| Some(*k) != want.as_ref()).take(ctx.per_node) {
            kids.push(((*k).clone(), false));
        }
        for (k, on) in kids {
            let sub = groups.get(&k).unwrap_or(&empty);
            // A hex is named for the settlement (or open country) it lies in.
            let anchor = if lv.hex().is_some() { parent_name } else { name.as_str() };
            node.children.push(dm_node(ctx, path, cl, k, sub, on, anchor));
        }
        node.children.sort_by(|a, b| b.agg.total.cmp(&a.agg.total).then(a.id.cmp(&b.id)));
    }
    // The society branch of a planet city, beside its hexes.
    if lv == Lv::Settlement && !quarters.is_empty() {
        let soc = Society::new(ctx);
        let mut qs: Vec<(PlaceId, Vec<usize>)> = quarters.iter().map(|q| (*q, soc.quarter_minds(*q))).collect();
        qs.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
        let want = path.quarter.or_else(|| qs.first().map(|q| q.0));
        path.quarter = want;
        let mut picked: Vec<(PlaceId, bool)> = want.map(|q| vec![(q, true)]).unwrap_or_default();
        picked.extend(qs.iter().filter(|q| Some(q.0) != want).take(ctx.per_node).map(|q| (q.0, false)));
        for (q, on) in picked {
            node.children.push(soc.quarter(path, q, on));
        }
    }
    // Who stands here: a focused demigod's story, a focused person outside the society branch.
    if lv == Lv::People && ctx.depth >= 6 {
        if let Some(d) = path.demigod {
            node.children.push(demigod_node(ctx, d));
        }
        if let Some(c) = path.person.filter(|_| path.crowd.is_none() || path.quarter.is_none()) {
            let soc = Society::new(ctx);
            node.children.push(soc.person(c, true));
        }
    }
    node
}

fn child_name(ctx: &Ctx, lv: Lv, key: &str, name: &str, parent: &str) -> String {
    match lv {
        Lv::World | Lv::Region => ctx.name(key),
        _ => match HexCell::parse(key) {
            Some(c) => format!("{} hex in {}", c.level.as_str(), if lv == Lv::Settlement { name } else { parent }),
            None => ctx.name(key),
        },
    }
}

/// The node's latest `memory.rollup` (its place's), with the entries it cites: walking back from it through its own
/// beat, the public facts that roll into it, until as many as it counted (`exact`: all found in the window).
fn rollup_json(ctx: &Ctx, idx: &[usize], key: &str) -> Value {
    let all = idx.iter().filter(|&&i| ctx.facts[i].kind == "memory.rollup" && ctx.facts[i].place == key).count();
    let Some(&ri) = idx.iter().rev().find(|&&i| ctx.facts[i].kind == "memory.rollup" && ctx.facts[i].place == key) else {
        return Value::Null;
    };
    let r = &ctx.facts[ri];
    let (cited, exact) = cited(ctx, ri);
    let data = &r.raw["data"];
    json!({ "rollups": all, "id": r.id, "beat": r.beat, "query_key": r.query, "text": r.text(), "counts": data["counts"],
            "facts": data["facts"], "cites": cited.iter().take(ctx.per_node.max(1) * 2).map(|&i| ctx.facts[i].id).collect::<Vec<_>>(),
            "cites_total": cited.len(), "exact": exact })
}

/// The facts roll-up `ri` cites, newest first, and whether all of them were found.
fn cited(ctx: &Ctx, ri: usize) -> (Vec<usize>, bool) {
    let r = &ctx.facts[ri];
    let want = r.raw["data"]["facts"].as_u64().unwrap_or(0) as usize;
    let mut out = vec![];
    let mut j = ri;
    while j > 0 && out.len() < want {
        j -= 1;
        let f = &ctx.facts[j];
        if f.beat != r.beat {
            break;
        }
        if f.rolls_into(r.place) {
            out.push(j);
        }
    }
    let exact = out.len() == want;
    (out, exact)
}

// ---------------------------------------------------------------------------------------------------------------------
// The society branch (the minds' memories)
// ---------------------------------------------------------------------------------------------------------------------

struct Society<'c, 'a> {
    ctx: &'c Ctx<'a>,
    crowd_people: BTreeMap<ClusterId, Vec<CharId>>,
    cell_people: BTreeMap<CellId, Vec<CharId>>,
}

impl<'c, 'a> Society<'c, 'a> {
    fn new(ctx: &'c Ctx<'a>) -> Self {
        let mut crowd_people: BTreeMap<ClusterId, Vec<CharId>> = BTreeMap::new();
        let mut cell_people: BTreeMap<CellId, Vec<CharId>> = BTreeMap::new();
        for c in &ctx.w.characters {
            if let Origin::Materialized { cluster, .. } = c.origin {
                crowd_people.entry(cluster).or_default().push(c.id);
            }
            if let Some(cell) = c.life.as_ref().and_then(|l| l.cell) {
                cell_people.entry(cell).or_default().push(c.id);
            }
        }
        Society { ctx, crowd_people, cell_people }
    }
    fn own(&self, r: Ref) -> &[usize] {
        self.ctx.by_owner.get(&r).map(Vec::as_slice).unwrap_or(&[])
    }
    fn people_minds(&self, ppl: &[CharId]) -> Vec<usize> {
        let mut v: Vec<usize> = ppl.iter().flat_map(|c| self.own(Ref::Char(*c)).iter().copied()).collect();
        v.sort_unstable();
        v
    }
    fn crowds(&self, q: PlaceId) -> Vec<ClusterId> {
        self.ctx.w.clusters.iter().filter(|k| k.place == q && k.army.is_none()).map(|k| k.id).collect()
    }
    fn crowd_minds(&self, k: ClusterId) -> Vec<usize> {
        let mut v: Vec<usize> = self.own(Ref::Cluster(k)).to_vec();
        v.extend(self.people_minds(self.crowd_people.get(&k).map(Vec::as_slice).unwrap_or(&[])));
        v.sort_unstable();
        v
    }
    fn quarter_minds(&self, q: PlaceId) -> Vec<usize> {
        let mut v: Vec<usize> = self.crowds(q).into_iter().flat_map(|k| self.crowd_minds(k)).collect();
        v.sort_unstable();
        v
    }
    fn cell_minds(&self, c: CellId) -> Vec<usize> {
        self.people_minds(self.cell_people.get(&c).map(Vec::as_slice).unwrap_or(&[]))
    }
    /// A node over minds' memories `idx` (`here`: the ones its own owner holds).
    fn node(&self, id: Ref, kind: &'static str, level: u32, idx: &[usize], here: &[usize], full: bool) -> Node {
        let ctx = self.ctx;
        let mut node = Node::new(id.to_string(), kind, level, ctx.ref_name(id), "minds");
        node.on_path = full;
        node.collapsed = !full;
        for &i in idx {
            let m = &ctx.minds[i];
            node.agg.add(m.beat, m.kind, ctx.mind_public(m), here.binary_search(&i).is_ok());
        }
        if full {
            node.samples = idx.iter().rev().map(|&i| &ctx.minds[i]).filter(|m| ctx.mind_shown(m)).take(ctx.per_node).map(|m| ctx.mind_sample(m)).collect();
        }
        node
    }
    /// Children ranked by their memories (busiest first; ties: the id), the wanted one first.
    fn pick<X: Copy + Ord>(&self, all: Vec<(X, Vec<usize>)>, want: Option<X>) -> (Vec<(X, bool)>, Option<X>) {
        let mut all = all;
        all.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
        let want = want.or_else(|| all.first().map(|x| x.0));
        let mut out: Vec<(X, bool)> = want.map(|x| vec![(x, true)]).unwrap_or_default();
        out.extend(all.iter().filter(|x| Some(x.0) != want).take(self.ctx.per_node).map(|x| (x.0, false)));
        (out, want)
    }
    fn quarter(&self, path: &mut Path, q: PlaceId, full: bool) -> Node {
        let ctx = self.ctx;
        let crowds: Vec<(ClusterId, Vec<usize>)> = self.crowds(q).into_iter().map(|k| (k, self.crowd_minds(k))).collect();
        let idx: Vec<usize> = {
            let mut v: Vec<usize> = crowds.iter().flat_map(|c| c.1.iter().copied()).collect();
            v.sort_unstable();
            v
        };
        let mut node = self.node(Ref::Place(q), "district", 3, &idx, &[], full);
        node.children_total = crowds.len();
        if let Some(p) = ctx.w.places.get(q.ix()) {
            node.area_km2 = Some(std::f64::consts::PI * p.radius_km * p.radius_km);
            node.extra.insert("center".into(), json!(p.center));
        }
        if !full {
            return node;
        }
        node.rolled = ranked_minds(ctx, crowds.iter().map(|c| (Ref::Cluster(c.0), c.1.len())));
        if ctx.depth <= 3 {
            return node;
        }
        let (kids, want) = self.pick(crowds, path.crowd);
        path.crowd = want;
        for (k, on) in kids {
            node.children.push(self.crowd(path, k, on));
        }
        node
    }
    fn crowd(&self, path: &mut Path, k: ClusterId, full: bool) -> Node {
        let ctx = self.ctx;
        let idx = self.crowd_minds(k);
        let own = self.own(Ref::Cluster(k)).to_vec();
        let mut node = self.node(Ref::Cluster(k), "crowd", 4, &idx, &own, full);
        let cells: Vec<(CellId, Vec<usize>)> =
            ctx.cells()
                .iter()
                .enumerate()
                .filter(|(_, c)| c["crowd"].as_u64() == Some(k.0 as u64))
                .map(|(i, _)| (CellId::from_ix(i), self.cell_minds(CellId::from_ix(i))))
                .collect();
        node.children_total = cells.len();
        if let Some(c) = ctx.w.clusters.get(k.ix()) {
            node.extra.insert("size".into(), c.size.into());
        }
        if !full {
            return node;
        }
        let ppl = self.crowd_people.get(&k).map(Vec::as_slice).unwrap_or(&[]);
        let mut rolled = vec![json!({ "from": Ref::Cluster(k), "name": "its own memories", "facts": own.len() })];
        rolled.extend(ranked_minds(ctx, ppl.iter().map(|c| (Ref::Char(*c), self.own(Ref::Char(*c)).len()))));
        rolled.retain(|r| r["facts"].as_u64() != Some(0));
        rolled.truncate(ctx.per_node.max(1));
        node.rolled = rolled;
        if ctx.depth <= 4 {
            return node;
        }
        // A focused person outside any cell hangs straight under its crowd.
        if let Some(c) = path.person.filter(|_| path.crowd == Some(k) && path.cell.is_none()) {
            if ctx.depth >= 6 {
                node.children.push(self.person(c, true));
            }
            return node;
        }
        let (kids, want) = self.pick(cells, path.cell);
        if path.crowd == Some(k) || path.cell.is_none() {
            path.cell = want;
        }
        for (c, on) in kids {
            node.children.push(self.cell(path, c, on));
        }
        node
    }
    fn cell(&self, path: &mut Path, c: CellId, full: bool) -> Node {
        let ctx = self.ctx;
        let ppl: Vec<CharId> = self.cell_people.get(&c).cloned().unwrap_or_default();
        let idx = self.cell_minds(c);
        let mut node = self.node(Ref::Cell(c), "cell", 5, &idx, &[], full);
        node.children_total = ppl.len();
        if let Some(x) = ctx.cells().get(c.ix()) {
            node.extra.insert("size".into(), x["size"].clone());
            node.extra.insert("cell_kind".into(), x["kind"].clone());
        }
        if !full {
            return node;
        }
        node.rolled = ranked_minds(ctx, ppl.iter().map(|p| (Ref::Char(*p), self.own(Ref::Char(*p)).len())));
        if ctx.depth <= 5 {
            return node;
        }
        let all: Vec<(CharId, Vec<usize>)> = ppl.iter().map(|p| (*p, self.own(Ref::Char(*p)).to_vec())).collect();
        let (kids, want) = self.pick(all, path.person.filter(|p| ppl.contains(p)));
        if path.person.is_none() {
            path.person = want;
        }
        for (p, on) in kids {
            node.children.push(self.person(p, on));
        }
        node
    }
    fn person(&self, c: CharId, full: bool) -> Node {
        let ctx = self.ctx;
        let own = self.own(Ref::Char(c)).to_vec();
        let mut node = self.node(Ref::Char(c), "person", 6, &own, &own, full);
        if let Some(x) = ctx.w.characters.get(c.ix()) {
            let life = x.life.as_deref();
            node.extra.insert("role".into(), json!(life.map(|l| l.role)));
            node.extra.insert("title".into(), json!(life.map(|l| l.rank.title.clone())));
            node.extra.insert("alive".into(), x.alive.into());
        }
        node
    }
}

fn ranked_minds(ctx: &Ctx, it: impl Iterator<Item = (Ref, usize)>) -> Vec<Value> {
    let mut v: Vec<(Ref, usize)> = it.filter(|x| x.1 > 0).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.into_iter().take(ctx.per_node).map(|(r, n)| json!({ "from": r, "name": ctx.ref_name(r), "facts": n })).collect()
}

/// A focused demigod's story: the DM facts that name it (its rolls, scars, wardrobe, followers, duels, loot), wherever
/// they happened.
fn demigod_node(ctx: &Ctx, d: DemigodId) -> Node {
    let r = Ref::Demigod(d).to_string();
    let names = |f: &Fact| {
        let data = &f.raw["data"];
        ["who", "demigod", "a", "b", "by", "owner", "winner", "loser", "from"].iter().any(|k| data[*k].as_str() == Some(r.as_str()))
    };
    let mut node = Node::new(r.clone(), "demigod", 6, ctx.w.name_of(Ref::Demigod(d)), "dm");
    node.on_path = true;
    node.collapsed = false;
    let idx: Vec<usize> = (0..ctx.facts.len()).filter(|&i| names(&ctx.facts[i])).collect();
    let mut places: BTreeMap<String, usize> = BTreeMap::new();
    for &i in &idx {
        let f = &ctx.facts[i];
        node.agg.add(f.beat, f.kind, f.public, true);
        if let Some(rk) = f.region() {
            *places.entry(rk.to_string()).or_default() += 1;
        }
    }
    node.samples = idx.iter().rev().map(|&i| &ctx.facts[i]).filter(|f| f.shown).take(ctx.per_node).map(Fact::sample).collect();
    let mut by: Vec<(String, usize)> = places.into_iter().collect();
    by.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    node.rolled = by.into_iter().take(ctx.per_node).map(|(k, n)| json!({ "from": k, "name": ctx.name(&k), "facts": n })).collect();
    if let Some(s) = ctx.w.dm.story(d) {
        node.extra.insert("story".into(), json!({ "followers": s.followers, "wardrobe": s.wardrobe, "scars": s.scars.len() }));
    }
    node
}

// ---------------------------------------------------------------------------------------------------------------------
// Recall paths
// ---------------------------------------------------------------------------------------------------------------------

/// The facts of the focus's deepest DM node first, then each tier above it (for finding examples near the focus).
fn focus_scopes(ctx: &Ctx, path: &Path) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = vec![];
    let all: Vec<usize> = (0..ctx.facts.len()).collect();
    let mut cur = all.clone();
    out.push(all);
    let steps = [Lv::Region, Lv::Settlement, Lv::Villages, Lv::Houses, Lv::People];
    for lv in steps {
        let Some(k) = path.at(lv) else { break };
        cur = cur
            .into_iter()
            .filter(|&i| {
                let f = &ctx.facts[i];
                match lv {
                    Lv::Region => f.region() == Some(k.as_str()),
                    Lv::Settlement => f.settlement().as_deref() == Some(k.as_str()),
                    Lv::Villages => f.scope == "cell" && ctx.hex(i, HexLevel::Villages).as_deref() == Some(k.as_str()),
                    Lv::Houses => ctx.hex(i, HexLevel::Houses).as_deref() == Some(k.as_str()),
                    _ => ctx.hex(i, HexLevel::People).as_deref() == Some(k.as_str()),
                }
            })
            .collect();
        if cur.is_empty() {
            break;
        }
        out.push(cur.clone());
    }
    out.reverse();
    out
}

/// The newest shown fact passing `want`, nearest the focus first.
fn find(ctx: &Ctx, scopes: &[Vec<usize>], want: impl Fn(&Fact) -> bool) -> Option<usize> {
    scopes.iter().find_map(|s| s.iter().rev().copied().find(|&i| ctx.facts[i].shown && want(&ctx.facts[i])))
}

/// The tree node ids a fact sits under (for highlighting).
fn highlight(ctx: &Ctx, i: usize) -> Vec<String> {
    let f = &ctx.facts[i];
    let mut v = vec![WORLD_KEY.to_string()];
    if let Some(r) = f.region() {
        v.push(r.to_string());
    }
    if let Some(s) = f.settlement() {
        v.push(s);
    }
    if f.scope == "cell" {
        v.extend([HexLevel::Villages, HexLevel::Houses, HexLevel::People].into_iter().filter_map(|l| ctx.hex(i, l)));
    }
    v.dedup();
    v
}

/// The roll-ups that cite fact `i` (one per ancestor, at its own beat).
fn citing(ctx: &Ctx, i: usize) -> Vec<Value> {
    let f = &ctx.facts[i];
    let mut keys: Vec<&str> = f.up.clone();
    if f.scope != "cell" && f.place != WORLD_KEY {
        keys.insert(0, f.place);
    }
    let mut out = vec![];
    for k in keys {
        if !f.rolls_into(k) {
            continue;
        }
        let q = format!("rollup:{k}:{}", f.beat);
        let Some(ri) = (i + 1..ctx.facts.len()).take_while(|&j| ctx.facts[j].beat == f.beat).find(|&j| ctx.facts[j].query == Some(q.as_str())) else {
            continue;
        };
        let (c, _) = cited(ctx, ri);
        let r = &ctx.facts[ri];
        out.push(json!({ "id": r.id, "place": k, "name": ctx.name(k), "beat": r.beat, "query_key": q, "text": r.text(),
                         "counted_as": f.kind, "kind_count": r.raw["data"]["counts"][f.kind], "facts": r.raw["data"]["facts"],
                         "cites_this": c.contains(&i) }));
    }
    out
}

/// The DM lines the resolver and the gods read about region `rk` at the asked beat: the real `dm::prompt_lines`, run
/// on the newest entries that could make the cut.
fn prompt_lines(ctx: &Ctx, rk: &str) -> Vec<String> {
    if let Some(v) = ctx.prompt_cache.borrow().get(rk) {
        return v.clone();
    }
    let v = prompt_lines_now(ctx, rk);
    ctx.prompt_cache.borrow_mut().insert(rk.to_string(), v.clone());
    v
}

fn prompt_lines_now(ctx: &Ctx, rk: &str) -> Vec<String> {
    let Some(p) = ctx.place(rk).map(|p| p.id) else { return vec![] };
    let n = ctx.st.dm_memory_lines as usize;
    let mut pick: Vec<fn7_model::dm::DmEntry> = ctx
        .facts
        .iter()
        .rev()
        .filter(|f| f.public && !NOT_IN_PROMPT.contains(&f.kind) && !f.text().is_empty() && (f.concerns_region(rk)))
        .take(n)
        .filter_map(|f| fn7_model::dm::DmEntry::deserialize(f.raw).ok())
        .collect();
    pick.reverse();
    let mut pw = ctx.prompt_world.borrow_mut();
    if pw.is_none() {
        *pw = world(ctx.state, &[]).ok().map(|mut w| {
            w.places = ctx.w.places.clone();
            w
        });
    }
    let Some(w) = pw.as_mut() else { return vec![] };
    w.dm.memory = pick;
    crate::dm::prompt_lines(w, p, ctx.b, n)
}

impl Fact<'_> {
    /// About region `rk` for the DM's prompt: at it, below it, or a world fact (`Dm::recall` with its ancestors).
    fn concerns_region(&self, rk: &str) -> bool {
        self.place == rk || self.up.contains(&rk) || self.place == WORLD_KEY
    }
}

fn line_of(f: &Fact) -> String {
    format!("beat {}: {}", f.beat, f.text())
}

/// Where the prompts stand on fact `i` now.
fn prompts_step(ctx: &Ctx, i: usize) -> Value {
    let f = &ctx.facts[i];
    let mut items = vec![];
    if let Some(rk) = f.region() {
        let lines = prompt_lines(ctx, rk);
        let line = line_of(f);
        let mut v = json!({ "prompt": "dm_memory", "reader": format!("the resolver and the gods acting in {}", ctx.name(rk)),
                            "place": rk, "lines": lines, "included": lines.contains(&line) });
        if NOT_IN_PROMPT.contains(&f.kind) {
            v["why"] = "left out by rule: a roll is shown with its own act, roll-ups and observations are bookkeeping".into();
        } else if !f.public {
            v["why"] = "private: the DM's prompt carries public facts only".into();
        } else if !lines.contains(&line) {
            v["why"] = format!("newer facts fill the {} lines (dm_memory_lines)", ctx.st.dm_memory_lines).into();
        } else {
            v["line"] = line.into();
        }
        items.push(v);
        // The coarse zoom's integrated truth: game.region's memory_summary is its latest roll-up.
        let latest = ctx.facts.iter().enumerate().rev().find(|(_, x)| x.kind == "memory.rollup" && x.place == rk).map(|(j, _)| j);
        let carried = latest.is_some_and(|j| cited(ctx, j).0.contains(&i));
        items.push(json!({ "prompt": "memory_summary", "reader": format!("game.region / game.node of {} (the latest roll-up)", ctx.name(rk)),
                           "place": rk, "included": carried, "rollup_id": latest.map(|j| ctx.facts[j].id) }));
    }
    if ctx.b == ctx.now {
        let at = f.raw["data"]["at"].as_array().and_then(|a| Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?]))
            .or_else(|| HexCell::parse(f.place).map(|c| c.center()));
        if let Some(at) = at {
            let line = ctx.earth_line(at);
            items.push(json!({ "prompt": "earth", "reader": "every prompt that acts at this place (the earth underfoot, schema 17)",
                               "at": at, "line": line, "included": line.is_some() }));
        }
    }
    json!({ "step": "prompts", "items": items })
}

fn entry_step(f: &Fact) -> Value {
    json!({ "step": "memory_entry", "entry": f.json() })
}

fn rollups_step(ctx: &Ctx, i: usize) -> Value {
    let items = citing(ctx, i);
    let mut v = json!({ "step": "rollups", "items": items });
    if !ctx.facts[i].rolls_into(WORLD_KEY) && ctx.facts[i].place != WORLD_KEY {
        v["why"] = "not rolled up: private facts, roll-ups and observations stay where they are".into();
    }
    v
}

fn recall_paths(ctx: &Ctx, path: &Path) -> Vec<Value> {
    let scopes = focus_scopes(ctx, path);
    let mut out = vec![];
    // 1. A d20: the key, the seeded roll it always gives, the entry, its roll-ups, the prompts.
    if let Some(i) = find(ctx, &scopes, |f| f.kind == "dice.rolled" && f.query.is_some()) {
        let f = &ctx.facts[i];
        let q = f.query.unwrap_or("");
        let d = &f.raw["data"];
        let mut r = Rng::derive(ctx.w.meta.seed, &[crate::dm::DICE_SALT, hash_str(q)]);
        let fresh = 1 + r.below(20) as u64;
        let who = d["who"].as_str().and_then(|s| s.parse::<Ref>().ok());
        out.push(json!({
            "fact": "d20", "title": f.text(), "highlight": highlight(ctx, i),
            "steps": [
                { "step": "query_key", "key": q, "key_hash": hash_str(q).to_string(),
                  "derivation": "Rng::derive(match seed, [0xD20, hash_str(key)]): the first draw is the face",
                  "reproduces": fresh == d["roll"].as_u64().unwrap_or(0),
                  "asked_again": "the recorded roll is returned; another modifier or DC is logged as memory_conflict and dropped" },
                { "step": "stored_roll", "roll": d["roll"], "modifier": d["modifier"], "total": d["total"], "target_dc": d["target_dc"],
                  "verdict": d["verdict"], "reason": d["reason"], "who": d["who"], "who_name": who.map(|r| ctx.ref_name(r)) },
                entry_step(f),
                rollups_step(ctx, i),
                prompts_step(ctx, i),
            ],
        }));
    }
    // 2. An encounter: its chain of entries (marked, rolled, revealed or faded), all under one id.
    if let Some(i) = find(ctx, &scopes, |f| f.kind == "encounter.revealed").or_else(|| find(ctx, &scopes, |f| f.kind.starts_with("encounter."))) {
        let f = &ctx.facts[i];
        let id = f.raw["data"]["id"].as_u64().or_else(|| f.query.and_then(|q| q.split(':').nth(1)?.parse().ok()));
        let pre = id.map(|x| format!("encounter:{x}"));
        let chain: Vec<Value> = ctx
            .facts
            .iter()
            .filter(|x| pre.as_deref().is_some_and(|p| x.query.is_some_and(|q| q == p || q.starts_with(&format!("{p}:")))))
            .map(|x| if x.shown { x.json() } else { json!({ "id": x.id, "beat": x.beat, "kind": x.kind, "public": false, "hidden": true }) })
            .collect();
        out.push(json!({
            "fact": "encounter", "title": f.text(), "highlight": highlight(ctx, i),
            "steps": [
                { "step": "query_key", "key": pre, "keys": chain.iter().filter_map(|c| c.get("query_key").cloned()).collect::<Vec<_>>(),
                  "asked_again": "a placed, rolled or revealed encounter is decided once: asked again, the DM answers from the record" },
                { "step": "chain", "entries": chain },
                entry_step(f),
                rollups_step(ctx, i),
                prompts_step(ctx, i),
            ],
        }));
    }
    // 3. An earth event: the field's event, remembered by place and beat, and the line the field gives there now.
    if let Some(i) = find(ctx, &scopes, |f| f.kind.starts_with("earth.")) {
        let f = &ctx.facts[i];
        let d = &f.raw["data"];
        out.push(json!({
            "fact": "earth", "title": f.text(), "highlight": highlight(ctx, i),
            "steps": [
                { "step": "field", "cell": d["cell"], "at": d["at"], "phenomenon": d["phenomenon"], "cells": d["cells"],
                  "source": "dm.earth: the voxel field (one column per 120 km regions hex); the DM reads it, never invents it" },
                entry_step(f),
                rollups_step(ctx, i),
                prompts_step(ctx, i),
            ],
        }));
    }
    // 4. A settled fact the DM's prompt carries now (the newest line about the focus's region that is a real fact).
    let rk = path.region.clone();
    if let Some(rk) = rk {
        let lines = prompt_lines(ctx, &rk);
        let pick = ctx.facts.iter().enumerate().rev().find(|(_, f)| {
            f.shown && f.public && !NOT_IN_PROMPT.contains(&f.kind) && f.kind != "time.phase" && lines.contains(&line_of(f))
        });
        if let Some((i, f)) = pick {
            out.push(json!({
                "fact": "settled", "title": f.text(), "highlight": highlight(ctx, i),
                "steps": [
                    { "step": "query_key", "key": f.query,
                      "asked_again": if f.query.is_some() { "decided once under this key: the record stands" } else { "a record, never re-decided" } },
                    entry_step(f),
                    rollups_step(ctx, i),
                    prompts_step(ctx, i),
                ],
            }));
        }
    }
    // 5. A person's memory: the event it is about, the memory, how it is recalled.
    if let Some(p) = person_path(ctx, path) {
        out.push(p);
    }
    out
}

/// A person's memory on the focus's society path (or the newest one the asker may see).
fn person_path(ctx: &Ctx, path: &Path) -> Option<Value> {
    let owner = path.person.map(Ref::Char);
    let shown = |i: &usize| {
        let m = &ctx.minds[*i];
        m.owner.starts_with("ch:") && m.ledger.is_some() && ctx.mind_shown(m)
    };
    // The focused person's, else one of the people of the society branch's district, else the newest anywhere.
    let in_branch = |i: &usize| {
        let Ok(Ref::Char(c)) = ctx.minds[*i].owner.parse::<Ref>() else { return false };
        let crowd = match ctx.w.characters.get(c.ix()).map(|x| x.origin) {
            Some(Origin::Materialized { cluster, .. }) => ctx.w.clusters.get(cluster.ix()),
            _ => None,
        };
        path.quarter.is_some_and(|q| crowd.is_some_and(|k| k.place == q))
    };
    let i = owner
        .and_then(|o| ctx.by_owner.get(&o))
        .and_then(|v| v.iter().rev().copied().find(|i| shown(i)))
        .or_else(|| (0..ctx.minds.len()).rev().find(|i| shown(i) && in_branch(i)))
        .or_else(|| (0..ctx.minds.len()).rev().find(shown))?;
    let m = &ctx.minds[i];
    let owner: Ref = m.owner.parse().ok()?;
    let item = MemoryItem::deserialize(m.raw).ok()?;
    let row = m.ledger.and_then(|l| ctx.rows.get(l)).and_then(|v| LedgerRow::deserialize(v).ok());
    let row_json = row.as_ref().map(|r| {
        let public = ctx.row_public(r.id.ix());
        if public {
            json!({ "step": "ledger_row", "row": Ref::Ledger(r.id), "beat": r.beat, "verb": r.verb, "text": r.act, "public": true })
        } else {
            json!({ "step": "ledger_row", "row": Ref::Ledger(r.id), "beat": r.beat, "public": false })
        }
    });
    // The rank recall would give it among its owner's memories in the window (overlap 0: salience, beat, id).
    let mut mine: Vec<&Mind> = ctx.by_owner.get(&owner).map(|v| v.iter().map(|&j| &ctx.minds[j]).filter(|x| x.live).collect()).unwrap_or_default();
    let key = |x: &Mind| (x.raw["salience"].as_u64().unwrap_or(0), x.beat, x.id);
    mine.sort_by(|a, b| key(b).cmp(&key(a)));
    let rank = mine.iter().position(|x| x.id == m.id).map(|p| p + 1);
    let line = (ctx.b == ctx.now).then(|| fn7_memory::render(ctx.w, &[&item]).trim_end().to_string());
    let x = ctx.w.characters.get(match owner { Ref::Char(c) => c.ix(), _ => usize::MAX });
    let crowd = x.and_then(|x| match x.origin { Origin::Materialized { cluster, .. } => Some(Ref::Cluster(cluster)), _ => None });
    let cell = x.and_then(|x| x.life.as_ref()?.cell).map(Ref::Cell);
    let mut steps = vec![];
    steps.extend(row_json);
    steps.push(json!({ "step": "mind_memory", "memory": m.id, "owner": owner, "owner_name": ctx.ref_name(owner), "beat": item.beat,
                       "kind": item.kind, "text": item.text, "salience": item.salience, "confidence": item.confidence,
                       "hedge": fn7_memory::hedge(&item), "about": item.about, "public": ctx.mind_public(m) }));
    let ups: Vec<Value> = [crowd, cell]
        .into_iter()
        .flatten()
        .map(|r| json!({ "id": r, "name": ctx.ref_name(r), "how": "counted in its society node's facts (the tree's roll-up of minds)" }))
        .collect();
    steps.push(json!({ "step": "rollups", "items": ups }));
    steps.push(json!({ "step": "prompts", "items": [
        { "prompt": "recall", "reader": "fn7_memory::recall for this person (rank by overlap, salience, recency)",
          "rank": rank, "of": mine.len(), "window_complete": ctx.minds_complete, "line": line, "included": rank.is_some() },
        { "prompt": "game.person", "reader": "the seats that may see it (their own people, told by their own, an event they know)", "included": true },
        { "prompt": "fold", "reader": "the memory fold (LLM step `fold`) once its owner's store passes its budget", "included": item.folded_into.is_none() },
    ] }));
    let mut hl = vec![owner.to_string()];
    hl.extend([crowd, cell].into_iter().flatten().map(|r| r.to_string()));
    Some(json!({ "fact": "person_memory", "title": item.text, "highlight": hl, "steps": steps }))
}

// ---------------------------------------------------------------------------------------------------------------------
// Totals
// ---------------------------------------------------------------------------------------------------------------------

fn totals(ctx: &Ctx, agg: &Agg, state: &Value, dm_len: usize, minds_len: usize) -> Value {
    let mut by_scope: BTreeMap<&str, u32> = BTreeMap::new();
    let mut observed: BTreeMap<&str, u32> = BTreeMap::new();
    for f in &ctx.facts {
        match by_scope.get_mut(f.scope) {
            Some(n) => *n += 1,
            None => {
                by_scope.insert(f.scope, 1);
            }
        }
        if f.kind == "place.observed" {
            *observed.entry(f.place).or_default() += 1;
        }
    }
    let mut by_owner: BTreeMap<String, u32> = BTreeMap::new();
    let mut live = 0u32;
    for m in &ctx.minds {
        let k = m.owner.split(':').next().unwrap_or("");
        match by_owner.get_mut(k) {
            Some(n) => *n += 1,
            None => {
                by_owner.insert(k.to_string(), 1);
            }
        }
        live += m.live as u32;
    }
    let w = ctx.w;
    let kinds = |k: PlaceKind| w.places.iter().filter(|p| p.kind == k).count();
    json!({
        "dm_memory": { "entries": dm_len, "read": ctx.facts.len(), "public": agg.total - agg.private, "private": agg.private,
                       "by_scope": by_scope, "by_kind": agg.kinds, "families": agg.families(),
                       "first_beat": agg.first, "last_beat": agg.last },
        "minds": { "items": minds_len, "read": ctx.minds.len(), "live": live, "by_owner": by_owner },
        "ledger_rows": rows(state).len(),
        "persons_known": w.characters.len(),
        "society_cells": ctx.cells().len(),
        "crowds": w.clusters.iter().filter(|c| c.army.is_none()).count(),
        "hexes_observed": observed.len(),
        "observations": observed.values().sum::<u32>(),
        "places": { "regions": kinds(PlaceKind::District), "settlements": kinds(PlaceKind::Site), "districts": kinds(PlaceKind::Quarter) },
        "state_bytes": state_bytes(state),
    })
}

/// The JSON size of `v`, estimated by walking it with every list sampled (at most [`SIZE_SAMPLE`] evenly spaced
/// rows, scaled up): strings by their length, integers by their digits, fractions as 18 characters (a stored f32 or
/// a coordinate prints about so long). No serialization.
fn json_size(v: &Value) -> usize {
    match v {
        Value::Null => 4,
        Value::Bool(b) => 4 + !*b as usize,
        Value::Number(n) => match (n.as_u64(), n.as_i64()) {
            (Some(u), _) => digits(u),
            (None, Some(i)) => 1 + digits(i.unsigned_abs()),
            _ => match n.as_f64() {
                Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => digits(f.abs() as u64) + 2 + (f < 0.0) as usize,
                _ => 18,
            },
        },
        Value::String(s) => s.len() + 2,
        Value::Array(a) if a.is_empty() => 2,
        Value::Array(a) => {
            let k = a.len().min(SIZE_SAMPLE);
            let sample: usize = (0..k).map(|j| json_size(&a[(2 * j + 1) * a.len() / (2 * k)])).sum();
            1 + a.len() + sample * a.len() / k
        }
        Value::Object(o) => 1 + o.len() + o.iter().map(|(k, x)| k.len() + 3 + json_size(x)).sum::<usize>(),
    }
}

/// Rows sampled per list by [`json_size`].
const SIZE_SAMPLE: usize = 24;

fn digits(mut u: u64) -> usize {
    let mut n = 1;
    while u >= 10 {
        u /= 10;
        n += 1;
    }
    n
}

/// The state's size by table, estimated cheaply ([`json_size`]): the big holders by their parts (the ledger's rows,
/// the minds' items, the DM's memory and earth...).
pub fn state_bytes(state: &Value) -> Value {
    let Some(o) = state.as_object() else { return json!({ "total": 0, "tables": [] }) };
    let rows = |v: &Value| v.as_array().map(Vec::len);
    let mut tables: Vec<(String, usize, Option<usize>)> = vec![];
    for (k, v) in o {
        match (k.as_str(), v) {
            ("ledger", Value::Object(x)) | ("memory", Value::Object(x)) | ("dm", Value::Object(x)) => {
                for (k2, v2) in x {
                    tables.push((format!("{k}.{k2}"), json_size(v2), rows(v2)));
                }
            }
            _ => tables.push((k.clone(), json_size(v), rows(v))),
        }
    }
    tables.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let total: usize = tables.iter().map(|t| t.1 + t.0.len() + 4).sum();
    json!({ "total": total, "estimated": true,
            "tables": tables.into_iter().map(|(t, b, n)| json!({ "table": t, "bytes": b, "rows": n })).collect::<Vec<_>>() })
}
