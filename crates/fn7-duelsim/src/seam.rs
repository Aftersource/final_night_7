//! The seam between the duel rules and the simulation, and the JSON a client gets.
//!
//! **What the lead wires** (after `backend/ai-duel-typing` merges; nothing in `fn7-plugin` calls this crate yet):
//!
//! ```text
//! // Cargo.toml (workspace): members += "crates/fn7-duelsim"; [workspace.dependencies] fn7-duelsim = { path = "crates/fn7-duelsim" }
//! // crates/fn7-plugin/Cargo.toml: fn7-duelsim.workspace = true
//! // panel.rs, `since()` (the one place panels leave the bout for `also[]` and the outcomes), 3 lines:
//! let f = fn7_duelsim::fighters_of(&b.scene[0], skin_a, skin_b);          // the skins as of the lock
//! let sims = fn7_duelsim::with_sims(&b.scene[0], &f, fn7_duelsim::seed32(b.duel.seed), &b.panels, from);
//! out.extend(sims);                                                        // instead of b.panels.iter().skip(from).cloned()
//! // present.rs, next to `"scene": b.scene, "panels": b.panels`:
//! "sim": fn7_duelsim::present_sim(&b.scene[0], &f, fn7_duelsim::seed32(b.duel.seed), &b.panels),
//! // duel.scene (once, at the lock): scene["sim"] = fn7_duelsim::scene_sim(&scene, &f);
//! ```
//!
//! The `sim` objects are **computed when a payload is built and never stored**: the bout keeps its panels exactly as
//! today, so the state schema, the snapshots and every recording are untouched. They are a pure function of
//! (scene, the two skins, the panels so far, the seed).

use crate::bake::{FighterSpec, On, PanelBake, Sim};
use crate::materials::{figure, table};
use crate::math::*;
use crate::physics::{Ev, With};
use crate::props::Scene;
use crate::wire::*;
use serde_json::{json, Map, Value};

pub const VERSION: u32 = 1;

/// The two fighters of a duel from its scene (`a_start`, `b_start`) and their skins as of the lock.
pub fn fighters_of(scene: &Value, skin_a: &str, skin_b: &str) -> [FighterSpec; 2] {
    let hex = |k: &str, d: [f64; 2]| [scene[k][0].as_f64().unwrap_or(d[0]), scene[k][1].as_f64().unwrap_or(d[1])];
    [FighterSpec::new(skin_a, hex("a_start", [-3.0, 0.0])), FighterSpec::new(skin_b, hex("b_start", [3.0, 0.0]))]
}
/// The duel's 64-bit seed folded to the engine's 32.
pub fn seed32(seed: u64) -> u32 { (seed ^ (seed >> 32)) as u32 }

fn mm(v: V3) -> Value { json!([q_mm(v[0]), q_mm(v[1]), q_mm(v[2])]) }
fn r1(x: f64) -> f64 { (x * 10.0 + 0.5).floor() / 10.0 }
fn r3(x: f64) -> f64 { (x * 1000.0 + 0.5).floor() / 1000.0 }
fn ms(x: f64) -> i64 { (x + 0.5).floor() as i64 }
fn side(s: usize) -> &'static str { if s == 0 { "a" } else { "b" } }
fn fx(sound: &str) -> &'static str { match sound { "stone" | "clay" => "dust", "wood" => "splinter", "metal" => "spark", "glass" => "glitter", "bone" => "chip", _ => "puff" } }

impl PanelBake {
    /// The `sim` object of a `duel.panel` (docs/CLIENT_API.md, "Duel sim").
    pub fn to_json(&self, scene: &Scene) -> Value {
        let id = |o: usize| scene.objects[o].id.clone();
        let nscene = scene.objects.len();
        let bodies: Vec<Value> = self.bodies.iter().map(|(b, o, k)| {
            let mut m = Map::new();
            m.insert("b".into(), (*b).into());
            if *b < nscene { m.insert("obj".into(), id(*o).into()); }
            m.insert("k".into(), b64(&pack_body(k)).into());
            Value::Object(m)
        }).collect();
        // the events worth a client's attention: every strike, crack, break, shatter and whiff; the 8 hardest collisions; the 6 fastest touches
        let mut hard: Vec<(f64, usize)> = self.events.iter().enumerate().filter(|(_, e)| e.kind == Ev::Hit && e.sim && e.j > 60.0).map(|(i, e)| (e.j, i)).collect();
        hard.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1))); hard.truncate(8);
        let mut touch: Vec<(f64, usize)> = self.events.iter().enumerate().filter(|(_, e)| e.kind == Ev::Touch && e.speed > 1.0).map(|(i, e)| (e.speed, i)).collect();
        touch.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1))); touch.truncate(6);
        let mut events = vec![];
        for (i, e) in self.events.iter().enumerate() {
            let keep = match e.kind { Ev::Push | Ev::Blast => false, Ev::Hit => !e.sim || hard.iter().any(|h| h.1 == i), Ev::Touch => touch.iter().any(|h| h.1 == i), _ => true };
            if !keep { continue; }
            let mut m = Map::new();
            m.insert("t".into(), ms(e.step as f64 * 1000.0 / 120.0 - self.t0_ms).into());
            m.insert("e".into(), e.kind.name().into());
            m.insert("obj".into(), id(e.obj).into());
            m.insert("b".into(), e.body.into());
            m.insert("part".into(), e.src.into());
            m.insert("at".into(), mm(e.at));
            if let Some(by) = e.by { m.insert("by".into(), side(by).into()); }
            match e.kind {
                Ev::Hit => {
                    m.insert("j".into(), ms(e.j).into());
                    m.insert("with".into(), match e.with { With::Floor => "floor".into(), With::Body(b) => json!(b), With::Fighter(s) => side(s).into(), With::None => e.why.into() });
                    m.insert("fx".into(), fx(e.sound).into());
                }
                Ev::Touch => { m.insert("speed".into(), r1(e.speed).into()); }
                Ev::Crack => { m.insert("damage".into(), r3(e.damage).into()); m.insert("fx".into(), fx(e.sound).into()); }
                Ev::Break => {
                    m.insert("pieces".into(), e.made.iter().map(|b| { let p = self.pieces.iter().find(|p| p.0 == *b).unwrap(); json!([p.0, p.2]) }).collect::<Vec<_>>().into());
                    m.insert("word".into(), e.word.into()); m.insert("fx".into(), fx(e.sound).into());
                }
                Ev::Shatter => {
                    m.insert("shards".into(), e.made.iter().map(|b| { let p = self.pieces.iter().find(|p| p.0 == *b).unwrap(); json!([p.0, q_mm(p.4[0]), q_mm(p.4[1]), q_mm(p.4[2])]) }).collect::<Vec<_>>().into());
                    m.insert("word".into(), e.word.into()); m.insert("fx".into(), fx(e.sound).into());
                }
                _ => {}
            }
            events.push(Value::Object(m));
        }
        let on = |o: &On| -> (Value, Value) { match o { On::Floor => ("".into(), Value::Null), On::Obj(o, p) => (id(*o).into(), (*p).into()), On::Fighter(s, at) => (side(*s).into(), at.clone().into()) } };
        let fighter = |s: usize| -> Value {
            let f = &self.fighters[s];
            json!({
                "root": b64(&pack_root(&f.root)),
                "limbs": f.limbs.iter().map(|l| { let (o, p) = on(&l.on); json!([ms(l.t0_ms), ms(l.t1_ms), ms(l.in_ms), ms(l.out_ms), l.limb, l.kind, q_mm(l.p[0]), q_mm(l.p[1]), q_mm(l.p[2]), q_mm(l.n[0]), q_mm(l.n[1]), q_mm(l.n[2]), o, p]) }).collect::<Vec<_>>(),
                "look": f.look.iter().map(|l| json!([ms(l.0), l.1])).collect::<Vec<_>>(),
                "arcs": f.arcs.iter().map(|a| json!([ms(a.0), ms(a.1), q_mm(a.2), ms(a.3)])).collect::<Vec<_>>(),
            })
        };
        let mut o = json!({
            "v": VERSION, "n": self.n, "t0_ms": ms(self.t0_ms), "step0": self.step0, "len": self.len,
            "bodies": bodies, "events": events,
            "fighters": { "a": fighter(0), "b": fighter(1) },
            "hash": format!("{:08x}", self.hash), "state": format!("{:016x}", self.state),
        });
        if !self.shots.is_empty() {
            o["shots"] = self.shots.iter().map(|s| { let (h, p) = on(&s.hit); json!({ "t0": ms(s.t0_ms), "t1": ms(s.t1_ms), "kind": s.kind, "by": side(s.by), "from": mm(s.from), "to": mm(s.to), "hit": h, "part": p, "blast": ms(s.blast), "radius": q_mm(s.radius) }) }).collect::<Vec<_>>().into();
        }
        o
    }
}

impl Sim {
    /// Everything a client that saw none of the earlier panels needs to stand the stage up as it is now: where every
    /// displaced body is, which pieces exist, which parts are gone or cracked, where the fighters stand.
    pub fn checkpoint_json(&self) -> Value {
        let sc = &self.scene; let nscene = sc.objects.len();
        let (mut bodies, mut pieces, mut dead, mut cracked) = (vec![], vec![], vec![], vec![]);
        for b in &self.world.bodies {
            if b.hidden { continue; }
            let o = &sc.objects[b.obj];
            if b.id < nscene {
                for p in &b.parts { if p.dead { dead.push(json!([o.id, p.src])); } else if p.cracked { cracked.push(json!([o.id, p.src])); } }
                if b.dead { continue; }
                let at = b.origin();
                if at != o.p || b.q != o.q { bodies.push(json!([b.id, o.id, q_mm(at[0]), q_mm(at[1]), q_mm(at[2]), q_quat(b.q)])); }
            } else if !b.dead {
                let (at, p) = (b.origin(), &b.parts[0]); let s = mul(p.prim.half, 2.0);
                if b.frag { pieces.push(json!([b.id, o.id, p.src, "shard", q_mm(at[0]), q_mm(at[1]), q_mm(at[2]), q_quat(b.q), q_mm(s[0]), q_mm(s[1]), q_mm(s[2])])); }
                else { pieces.push(json!([b.id, o.id, p.src, "piece", q_mm(at[0]), q_mm(at[1]), q_mm(at[2]), q_quat(b.q)])); if p.cracked { cracked.push(json!([o.id, p.src])); } }
            }
        }
        json!({ "v": VERSION, "n": self.n, "t_ms": ms(self.t_ms), "bodies": bodies, "pieces": pieces, "dead": dead, "cracked": cracked,
                "fighters": { "a": mm(self.cur[0].p), "b": mm(self.cur[1].p) },
                "hash": format!("{:08x}", self.hash), "state": format!("{:016x}", self.world.state_hash(FNV64_0)) })
    }
}

/// What `panel_sim` returns: the `sim` to attach to the panel, what a reconnect needs, the state to keep (or drop).
pub struct PanelSim {
    /// Attach as `duel.panel.sim`.
    pub sim: Value,
    /// For `present.duels[].sim`: `{ at: <the stage as the panel starts>, sim: <the panel's sim> }`.
    pub present: Value,
    /// The simulation after this panel. Keep it in memory to make the next call cheap, or drop it: it is rebuilt from
    /// the panels whenever it is missing or stale. Never store it in the match state.
    pub state: Sim,
    pub bake: PanelBake,
}

/// THE SEAM. Bake the panel just built.
///
/// - `scene`: the duel's `duel.scene`. `fighters`: [`fighters_of`]. `seed`: [`seed32`] of the duel's seed.
/// - `earlier`: every panel before this one, in order. `panel`: the panel just built.
/// - `state`: the [`PanelSim::state`] of the previous call for this duel, if the caller kept it. It is used only when
///   it is exactly the state after `earlier` (its panel count matches); otherwise the earlier panels are re-baked
///   from scratch, which gives the same result bit for bit (tested), in well under a millisecond per panel.
pub fn panel_sim(scene: &Value, fighters: &[FighterSpec; 2], seed: u32, earlier: &[Value], panel: &Value, state: Option<Sim>) -> PanelSim {
    let mut sim = match state { Some(s) if s.n == earlier.len() && s.seed == seed => s, _ => { let mut s = Sim::new(scene, fighters, seed); for p in earlier { s.panel(p); } s } };
    let at = sim.checkpoint_json();
    let bake = sim.panel(panel);
    let js = bake.to_json(&sim.scene);
    PanelSim { present: json!({ "at": at, "sim": js }), sim: js, state: sim, bake }
}
/// `panels[from..]`, each with its `sim` attached (for `also[]` and the outcomes).
pub fn with_sims(scene: &Value, fighters: &[FighterSpec; 2], seed: u32, panels: &[Value], from: usize) -> Vec<Value> {
    let mut sim = Sim::new(scene, fighters, seed);
    let mut out = vec![];
    for (i, p) in panels.iter().enumerate() {
        let b = sim.panel(p);
        if i >= from { let mut p = p.clone(); p["sim"] = b.to_json(&sim.scene); out.push(p); }
    }
    out
}
/// For `present.duels[]`: the stage as the newest panel starts, and that panel's sim. Null before the first panel.
pub fn present_sim(scene: &Value, fighters: &[FighterSpec; 2], seed: u32, panels: &[Value]) -> Value {
    match panels.split_last() { Some((last, earlier)) => panel_sim(scene, fighters, seed, earlier, last, None).present, None => Value::Null }
}

/// The whole duel at once (tools, tests, replays).
pub struct DuelBake { pub panels: Vec<PanelBake>, pub sim: Sim, pub hash: u32, pub state: u64 }
pub fn bake(scene: &Value, fighters: &[FighterSpec; 2], panels: &[Value], seed: u32) -> DuelBake {
    let mut sim = Sim::new(scene, fighters, seed);
    let out: Vec<PanelBake> = panels.iter().map(|p| sim.panel(p)).collect();
    let (hash, state) = (sim.hash, sim.world.state_hash(FNV64_0));
    DuelBake { panels: out, sim, hash, state }
}

/// What the simulation derives from a scene and the two skins before any panel: sent once with `duel.scene`.
pub fn scene_sim(scene_spec: &Value, fighters: &[FighterSpec; 2]) -> Value {
    let sim = Sim::new(scene_spec, fighters, 0);
    let sc = &sim.scene;
    let objects: Vec<Value> = sc.objects.iter().enumerate().filter(|(_, o)| !o.hidden).map(|(i, o)| json!({
        "id": o.id, "b": i, "mass": r1(o.mass), "volume": r3(o.volume), "com": mm(o.com), "at": mm(o.p), "tip_deg": r1(o.tip_angle), "tip_j": r1(o.tip_energy),
        "support": o.support.iter().map(|p| json!([q_mm(p[0]), q_mm(p[1])])).collect::<Vec<_>>(),
        "bounds": [mm(o.aabb[0]), mm(o.aabb[1])],
    })).collect();
    let aff: Vec<Value> = sc.affordances.iter().map(|a| { let mut v = json!({ "kind": a.kind, "obj": sc.objects[a.obj].id, "part": a.part, "at": mm(a.pos), "hex": a.hex });
        if !a.mode.is_empty() { v["mode"] = a.mode.into(); } if a.height > 0.0 { v["height"] = q_mm(a.height).into(); } v }).collect();
    let fighter = |s: usize| { let f = &sim.fb[s]; let fig = figure(&f.figure); let c = f.com_rest();
        json!({ "skin": f.skin, "material": f.material, "figure": f.figure, "mass": r1(f.mass), "jump": r3(f.jump), "speed": r3(f.speed), "com": mm(c), "start": mm(sim.cur[s].p),
                "segments": f.segment_masses().into_iter().map(|(n, m)| (n, json!(r3(m)))).collect::<Map<String, Value>>(),
                "anchors": fig.anchors.iter().map(|(n, p)| (n.clone(), mm(*p))).collect::<Map<String, Value>>(),
                "spheres": fig.spheres.iter().map(|s| json!([q_mm(s[0]), q_mm(s[1])])).collect::<Vec<_>>() }) };
    json!({ "v": VERSION, "hz": 60, "step_hz": 120, "unit": "mm", "frame": "x right of the audience, y up, z toward the audience; hex [q, r] -> x = 1.7320508 (q + r/2) m, z = 1.5 r m",
            "objects": objects, "affordances": aff, "fighters": { "a": fighter(0), "b": fighter(1) },
            "materials": table().materials.iter().map(|m| (m.name.clone(), json!({ "density": m.density, "friction": m.friction, "restitution": m.restitution, "strength": m.strength, "brittle": m.brittle, "shards": m.shards, "word": m.word, "sound": m.sound }))).collect::<Map<String, Value>>() })
}

/// A lab duel file (`duel_lab/duel.json`) as the server would stream it: an intro panel (the lab's `intro_ms` of
/// standing), its panels, an outro (the lab's `outro_ms`; the settle follows it).
pub fn lab_panels(d: &Value) -> Vec<Value> {
    let mut v = vec![json!({ "kind": "intro", "play_ms": d["intro_ms"], "a": { "segs": [] }, "b": { "segs": [] } })];
    v.extend(d["panels"].as_array().cloned().unwrap_or_default());
    v.push(json!({ "kind": "outro", "play_ms": d["outro_ms"], "a": { "segs": [] }, "b": { "segs": [] } }));
    v
}

/// An in-process memo of the newest simulation of each duel, so a duel streaming panel by panel bakes each panel
/// once. It is **never part of the match state**: a miss (a restart, a replay, a fork, another duel) re-bakes from
/// the panels and gives the same bytes.
pub struct SimCache { cap: usize, entries: Vec<CacheEntry> }
struct CacheEntry { key: String, sig: Vec<u32>, sim: Sim, sims: Vec<Value>, at: Value }
fn sig(v: &Value) -> u32 { fnv(FNV0, serde_json::to_string(v).unwrap_or_default().as_bytes()) }
impl SimCache {
    /// `cap`: how many duels to remember (the oldest is dropped).
    pub fn new(cap: usize) -> SimCache { SimCache { cap: cap.max(1), entries: vec![] } }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    /// Every panel's `sim` so far, and the stage as the newest panel starts. `key`: the duel's id.
    pub fn sims(&mut self, key: &str, scene: &Value, fighters: &[FighterSpec; 2], seed: u32, panels: &[Value]) -> (&[Value], &Value) {
        let mut want = vec![sig(scene) ^ seed, fnv(FNV0, format!("{fighters:?}").as_bytes())];
        want.extend(panels.iter().map(sig));
        let at = self.entries.iter().position(|e| e.key == key);
        let mut e = match at { Some(i) => self.entries.remove(i), None => CacheEntry { key: key.to_string(), sig: vec![], sim: Sim::new(scene, fighters, seed), sims: vec![], at: Value::Null } };
        if e.sig.len() < 2 || e.sig.len() > want.len() || e.sig[..] != want[..e.sig.len()] { e = CacheEntry { key: key.to_string(), sig: want[..2].to_vec(), sim: Sim::new(scene, fighters, seed), sims: vec![], at: Value::Null }; }
        for (i, p) in panels.iter().enumerate().skip(e.sims.len()) {
            e.at = e.sim.checkpoint_json();
            let b = e.sim.panel(p);
            e.sims.push(b.to_json(&e.sim.scene)); e.sig.push(want[i + 2]);
        }
        if self.entries.len() >= self.cap { self.entries.remove(0); }
        self.entries.push(e);
        let e = self.entries.last().unwrap();
        (&e.sims, &e.at)
    }
    /// `panels[from..]`, each with its `sim` attached.
    pub fn with_sims(&mut self, key: &str, scene: &Value, fighters: &[FighterSpec; 2], seed: u32, panels: &[Value], from: usize) -> Vec<Value> {
        let (sims, _) = self.sims(key, scene, fighters, seed, panels);
        panels.iter().zip(sims).skip(from).map(|(p, s)| { let mut p = p.clone(); p["sim"] = s.clone(); p }).collect()
    }
    /// For `present.duels[].sim`.
    pub fn present(&mut self, key: &str, scene: &Value, fighters: &[FighterSpec; 2], seed: u32, panels: &[Value]) -> Value {
        if panels.is_empty() { return Value::Null; }
        let (sims, at) = self.sims(key, scene, fighters, seed, panels);
        json!({ "at": at, "sim": sims[sims.len() - 1] })
    }
}
