//! The materials table, what each skin is made of, the figures and a fighter's mass: `data/materials.json`
//! (a port of `duel_lab/lib/materials.js`), parsed once.

use crate::math::{root4, V3};
use serde_json::Value;
use std::sync::OnceLock;

pub const DATA: &str = include_str!("../data/materials.json");

#[derive(Debug, Clone, PartialEq)]
pub struct Mat {
    pub name: String,
    pub density: f64,
    pub friction: f64,
    pub restitution: f64,
    pub strength: f64,
    pub brittle: bool,
    pub shards: usize,
    pub word: String,
    pub sound: String,
}
#[derive(Debug, Clone)]
pub struct Skin { pub name: String, pub material: String, pub fill: f64, pub future: bool }
#[derive(Debug, Clone)]
pub struct Figure {
    pub name: String,
    pub volume: f64,
    pub height: f64,
    pub pivot: f64,
    /// (bone, share of the mass, centre in the standing pose), shares normalised to 1.
    pub segments: Vec<(String, f64, V3)>,
    pub anchors: Vec<(String, V3)>,
    pub spheres: [[f64; 2]; 2],
    pub half_stance: f64,
}
#[derive(Debug, Clone, Copy)]
pub struct Limb { pub m: f64, pub v: f64, pub y: f64 }
#[derive(Debug, Clone)]
pub struct Projectile { pub kind: String, pub mass: f64, pub speed: f64, pub blast: f64, pub radius: f64 }

pub struct Table {
    pub materials: Vec<Mat>,
    pub skins: Vec<Skin>,
    pub figures: Vec<Figure>,
    pub limbs: Vec<(String, Limb)>,
    pub projectiles: Vec<Projectile>,
    pub crack_at: f64,
    pub ref_mass: f64,
}
fn f(v: &Value, k: &str) -> f64 { v[k].as_f64().unwrap_or(0.0) }
fn v3(v: &Value) -> V3 { [v[0].as_f64().unwrap_or(0.0), v[1].as_f64().unwrap_or(0.0), v[2].as_f64().unwrap_or(0.0)] }

pub fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| {
        let d: Value = serde_json::from_str(DATA).expect("data/materials.json");
        let obj = |k: &str| d[k].as_object().expect(k).iter().filter(|(n, _)| n.as_str() != "_").map(|(n, v)| (n.clone(), v.clone())).collect::<Vec<_>>();
        // serde_json's map order depends on a cargo feature another crate may switch on: sort, so sums never depend on it
        let obj = |k: &str| { let mut o = obj(k); o.sort_by(|a, b| a.0.cmp(&b.0)); o };
        let materials = obj("materials").into_iter().map(|(name, v)| Mat { name, density: f(&v, "density"), friction: f(&v, "friction"), restitution: f(&v, "restitution"),
            strength: f(&v, "strength"), brittle: v["brittle"].as_bool().unwrap_or(false), shards: v["shards"].as_u64().unwrap_or(1) as usize,
            word: v["word"].as_str().unwrap_or("").into(), sound: v["sound"].as_str().unwrap_or("").into() }).collect();
        let skins = obj("skins").into_iter().map(|(name, v)| Skin { name, material: v["material"].as_str().unwrap_or("wood").into(), fill: f(&v, "fill"), future: v["future"].as_bool().unwrap_or(false) }).collect();
        let figures = obj("figures").into_iter().map(|(name, v)| {
            let mut segments: Vec<(String, f64, V3)> = v["segments"].as_object().unwrap().iter().map(|(b, s)| (b.clone(), s.as_f64().unwrap(), v3(&v["rest"][b]))).collect();
            segments.sort_by(|a, b| a.0.cmp(&b.0));
            let mut tot = 0.0;
            for s in &segments { tot += s.1; }
            for s in &mut segments { s.1 /= tot; }
            Figure { name, volume: f(&v, "volume"), height: f(&v, "height"), pivot: f(&v, "pivot"), segments,
                anchors: ["head", "chest", "belly", "legs"].iter().map(|a| (a.to_string(), v3(&v["anchors"][*a]))).collect(),
                spheres: [[v["spheres"][0][0].as_f64().unwrap(), v["spheres"][0][1].as_f64().unwrap()], [v["spheres"][1][0].as_f64().unwrap(), v["spheres"][1][1].as_f64().unwrap()]],
                half_stance: f(&v, "foot_half_stance") }
        }).collect();
        let limbs = obj("limbs").into_iter().map(|(n, v)| (n, Limb { m: f(&v, "m"), v: f(&v, "v"), y: f(&v, "y") })).collect();
        let projectiles = obj("projectiles").into_iter().map(|(kind, v)| Projectile { kind, mass: f(&v, "mass"), speed: f(&v, "speed"), blast: f(&v, "blast"), radius: f(&v, "radius") }).collect();
        Table { materials, skins, figures, limbs, projectiles, crack_at: f(&d, "crack_at"), ref_mass: f(&d, "ref_mass") }
    })
}
/// A material by name (an unknown name is wood, as in the lab).
pub fn mat(name: &str) -> &'static Mat {
    let t = table();
    t.materials.iter().find(|m| m.name == name).unwrap_or_else(|| t.materials.iter().find(|m| m.name == "wood").unwrap())
}
pub fn is_material(name: &str) -> bool { table().materials.iter().any(|m| m.name == name) }
pub fn figure(name: &str) -> &'static Figure { let t = table(); t.figures.iter().find(|f| f.name == name).unwrap_or(&t.figures[0]) }
pub fn limb(kind: &str) -> Limb { let t = table(); t.limbs.iter().find(|l| l.0 == kind).or_else(|| t.limbs.iter().find(|l| l.0 == "hand")).unwrap().1 }
pub fn projectile(kind: &str) -> &'static Projectile { let t = table(); t.projectiles.iter().find(|p| p.kind == kind).unwrap_or_else(|| t.projectiles.iter().find(|p| p.kind == "stone").unwrap()) }
pub fn crack_at() -> f64 { table().crack_at }
pub fn ref_mass() -> f64 { table().ref_mass }

/// A fighter as the simulation sees it: its mass from the skin's material and the figure's volume, and what that mass
/// does to it. Muscle does not scale with weight: a heavier figure launches slower.
#[derive(Debug, Clone, PartialEq)]
pub struct FighterBody {
    pub skin: String,
    pub material: String,
    pub figure: String,
    pub mass: f64,
    /// Jump apex scale: min(1.25, sqrt(43 kg / mass)).
    pub jump: f64,
    /// Strike and take-off speed scale: min(1.15, (43 kg / mass)^(1/4)).
    pub speed: f64,
    pub friction: f64,
    pub restitution: f64,
}
pub fn fighter_body(skin: &str, figure_name: &str) -> FighterBody {
    let t = table();
    let s = t.skins.iter().find(|s| s.name == skin).unwrap_or_else(|| t.skins.iter().find(|s| s.name == "wood").unwrap());
    let (m, fig) = (mat(&s.material), figure(figure_name));
    let mass = m.density * s.fill * fig.volume;
    let k = t.ref_mass / mass;
    FighterBody { skin: s.name.clone(), material: s.material.clone(), figure: fig.name.clone(), mass, jump: k.sqrt().min(1.25), speed: root4(k).min(1.15), friction: m.friction, restitution: m.restitution }
}
impl FighterBody {
    /// Each segment's mass (kg), in the figure's bone order.
    pub fn segment_masses(&self) -> Vec<(String, f64)> { figure(&self.figure).segments.iter().map(|s| (s.0.clone(), s.1 * self.mass)).collect() }
    /// The whole-body centre of mass in the standing pose, root-local [x left, y up, z forward].
    pub fn com_rest(&self) -> V3 {
        let mut c = [0.0; 3];
        for s in &figure(&self.figure).segments { for i in 0..3 { c[i] += s.1 * s.2[i]; } }
        c
    }
}
