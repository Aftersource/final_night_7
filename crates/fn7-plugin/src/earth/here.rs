//! THE EARTH UNDERFOOT (state schema 17; docs/EARTH_SIM.md §12): what the field holds at one place now, as one short,
//! factual line for the LLM prompts that act there, and as the ground of a duel's stage.
//!
//! Read straight from the stored field (`dm.earth`: the cells as they stood at the end of the last stepped beat, the
//! live phenomena and the input log), with integer thresholds only: no draw, no wall time, so the same state always
//! gives the same words. A cell at base, or with nothing above the thresholds, gives nothing (the prompt is unchanged).
//! At most [`MAX_CLAUSES`] clauses, worst first, each cut to the bone; the callers cap the lines per prompt
//! ([`MAX_LINES`]).

use fn7_model::earth::EarthInput;
use fn7_model::*;

use super::grid::{grid, NONE, SEA_LEVEL};
use super::sim::*;
use crate::settings::Settings;

/// Most clauses in one place's description.
pub const MAX_CLAUSES: usize = 3;
/// Most earth lines one prompt carries.
pub const MAX_LINES: usize = 2;
/// Longest a line gets (characters).
pub const LINE_MAX: usize = 200;

/// The field as a match holds it, for reading places.
pub struct Ground {
    pub sim: Sim,
    pub(crate) inputs: Vec<EarthInput>,
}

/// What a duel's stage stands on: the line, the floor word, the hazards and the props they leave.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub line: String,
    /// One word for the floor: `lava`, `burning`, `flooded`, `frozen`, `rift`, `cracked`, `snow`, `ash`, `ruins`.
    pub floor: &'static str,
    pub hazards: Vec<&'static str>,
    pub slippery: bool,
    /// The hazard props to put on the stage (object kinds of `panel::object`), worst first.
    pub props: Vec<&'static str>,
}

/// "last beat", "2 beats ago".
fn ago(n: u32) -> String {
    if n <= 1 { "last beat".into() } else { format!("{n} beats ago") }
}

fn levels(n: i32) -> String {
    if n.abs() == 1 { "1 level".into() } else { format!("{} levels", n.abs()) }
}

impl Ground {
    /// The field of `w` when the earth underfoot is on (schema 17, the simulator running, the field begun).
    pub fn of(w: &World, st: &Settings) -> Option<Ground> {
        if !w.earth_prompt_rules() {
            return None;
        }
        let s = w.dm.earth.as_deref()?;
        let sim = super::rules::sim_of(w, st)?;
        Some(Ground { sim, inputs: s.inputs.clone() })
    }

    /// The cell under a point (`None` off the grid).
    pub fn cell(&self, at: LonLat) -> Option<u16> {
        let i = grid().index_of(at);
        (i != NONE).then_some(i)
    }

    /// The beat the field's "now" is: the one after its last stepped beat.
    fn now(&self) -> u32 {
        self.sim.beat + 1
    }

    /// The newest (or, `oldest`, the first) thing of one of `kinds` that reached cell `i`: a live phenomenon whose reach
    /// covers it, or a logged input whose reach did. Its kind and beat.
    fn source(&self, i: u16, kinds: &[u8], oldest: bool) -> Option<(u8, u32)> {
        let covers = |c: u16, r: u8| within(c, i, r);
        let mut best: Option<(u8, u32)> = None;
        let mut take = |k: u8, b: u32| {
            let better = match best {
                None => true,
                Some((_, x)) => if oldest { b < x } else { b > x },
            };
            if better {
                best = Some((k, b));
            }
        };
        for p in self.sim.phen.iter().filter(|p| kinds.contains(&p.kind) && p.born <= self.sim.beat) {
            // A quake's rupture runs along the fault, a few cells past its reach.
            let r = p.radius.saturating_add(if p.kind == EARTHQUAKE { 3 } else { 1 });
            if covers(p.cell, r) {
                take(p.kind, p.born);
            }
        }
        for x in self.inputs.iter().filter(|x| kinds.contains(&x.kind) && x.beat <= self.sim.beat) {
            let r = x.radius.saturating_add(if x.kind == EARTHQUAKE { 3 } else { 1 });
            if covers(x.cell, r) {
                take(x.kind, x.beat);
            }
        }
        best
    }

    fn when(&self, i: u16, kinds: &[u8]) -> Option<(u8, String)> {
        self.source(i, kinds, false).map(|(k, b)| (k, ago(self.now().saturating_sub(b))))
    }

    /// A live phenomenon of `kind` within `r` cells of `i`.
    fn live_near(&self, i: u16, kind: u8, r: u8) -> bool {
        self.sim.phen.iter().filter(|p| p.kind == kind && p.dur > 0).any(|p| within(p.cell, i, r))
    }

    /// Was this ground land before the field took it?
    fn drowned(&self, i: u16) -> bool {
        grid().h[i as usize] >= SEA_LEVEL && self.sim.is_sea(i)
    }

    /// The clauses for cell `i`, worst first (all of them; callers keep [`MAX_CLAUSES`]).
    fn clauses(&self, i: u16) -> Vec<String> {
        let c = *self.sim.c(i);
        if c.is_base() {
            return vec![];
        }
        let sea = self.sim.is_sea(i);
        let mut out: Vec<String> = vec![];
        // Molten rock first: nothing else matters while it runs.
        if c.lava >= 8 {
            let mut s = if c.lava >= 64 { "molten lava running".to_string() } else { "cooling black lava, still glowing".to_string() };
            if c.water >= 8 || c.snow >= 32 || sea {
                s.push_str(", steam hissing off it");
            } else if c.plume >= 32 || c.fire >= 16 {
                s.push_str(", smoke");
            }
            if let Some((k, a)) = self.when(i, &[VOLCANO, PLATE_DRIFT]) {
                s.push_str(&format!(", {} {a}", if k == VOLCANO { "the eruption began" } else { "the rift opened" }));
            }
            out.push(s);
        }
        if self.drowned(i) {
            let mut s = "the sea has swallowed this land".to_string();
            if let Some((k, a)) = self.when(i, &[TIDAL_WAVE, SINK_LAND, METEOR]) {
                s.push_str(&format!(", {} {a}", match k { TIDAL_WAVE => "the wave came", METEOR => "the impact", _ => "it sank" }));
            }
            out.push(s);
        }
        if c.fire >= 32 {
            let mut s = if c.fire >= 128 { "a wildfire roaring through".to_string() } else { "fires burning".to_string() };
            if let Some((k, a)) = self.when(i, &[WILDFIRE, LIGHTNING, METEOR]) {
                s.push_str(&format!(", {} {a}", match k { LIGHTNING => "lit by lightning", METEOR => "lit by the impact", _ => "started" }));
            }
            out.push(s);
        }
        if !sea && c.water >= 48 {
            let mut s = if c.heat <= -48 {
                "floodwater frozen solid".to_string()
            } else if c.water >= 160 {
                "floodwater over a man's head".to_string()
            } else if c.water >= 96 {
                "waist-deep floodwater".to_string()
            } else {
                "knee-deep floodwater".to_string()
            };
            if let Some((k, a)) = self.when(i, &[FLOOD, TIDAL_WAVE, STORM, SINK_LAND]) {
                s.push_str(&format!(", {} {a}", match k {
                    TIDAL_WAVE => "the wave struck",
                    STORM => "the storm's rain began",
                    SINK_LAND => "the land sank",
                    _ => "the flood came",
                }));
            }
            out.push(s);
        }
        if c.crack >= 48 || c.shake >= 32 {
            let shaking = c.shake >= 16 || self.live_near(i, EARTHQUAKE, 4);
            let mut s = match (c.crack >= 128, c.crack >= 48, shaking) {
                (true, _, true) => "an open rift splits the ground, still shaking".to_string(),
                (true, _, false) => "an open rift splits the ground".to_string(),
                (false, true, true) => "the ground is cracked and still shaking".to_string(),
                (false, true, false) => "the ground is cracked".to_string(),
                _ => "the ground is shaking".to_string(),
            };
            if let Some((k, a)) = self.when(i, &[EARTHQUAKE, PLATE_DRIFT, METEOR]) {
                s.push_str(&format!(" after the {} {a}", match k { PLATE_DRIFT => "plates moved", METEOR => "impact", _ => "quake" }));
            }
            out.push(s);
        }
        if !sea && c.snow >= 48 {
            let mut s = if c.snow >= 160 { "snowed in, drifts deep".to_string() } else if c.snow >= 96 { "deep snow".to_string() } else { "snow on the ground".to_string() };
            if c.heat <= -32 {
                s.push_str(", bitter cold");
            }
            if let Some((_, a)) = self.when(i, &[BLIZZARD]) {
                s.push_str(&format!(", the blizzard came {a}"));
            }
            out.push(s);
        }
        if c.swarm >= 64 {
            out.push("a swarm thick in the air, eating everything green".into());
        }
        // The land itself moved (a rift says so already).
        if c.dh != 0 && c.crack < 128 && !self.drowned(i) {
            let up = c.dh > 0;
            let kinds: &[u8] = if up { &[RAISE_LAND, VOLCANO, PLATE_DRIFT] } else { &[SINK_LAND, METEOR, PLATE_DRIFT] };
            let since = self.source(i, kinds, true);
            let mut s = match (up, since.map(|x| x.0)) {
                (false, Some(METEOR)) => format!("a crater {} deep", levels(c.dh as i32)),
                (true, _) => format!("the land here rose {}", levels(c.dh as i32)),
                (false, _) => format!("the land here sank {}", levels(c.dh as i32)),
            };
            if let Some((_, b)) = since {
                s.push_str(&format!(" since beat {b}"));
            }
            out.push(s);
        }
        if c.ash >= 48 || c.plume >= 96 {
            out.push(if c.plume >= 96 { "ash falling from a dark sky".into() } else { "grey ash lying thick".into() });
        }
        if c.ruin >= 96 {
            out.push("the buildings around here in ruins".into());
        }
        if c.fire < 32 && c.dfuel <= -48 && !sea {
            out.push("burnt black, nothing green left".into());
        } else if c.dfuel >= 48 && !sea {
            out.push("a forest grown up out of nowhere".into());
        }
        // The sky last.
        if c.cloud >= 112 {
            let cold = self.sim.cold(i);
            out.push(match (c.charge >= 96, cold, c.cloud >= 176) {
                (true, _, _) => "a thunderstorm overhead".into(),
                (false, true, true) => "a blizzard blowing".into(),
                (false, true, false) => "snow falling".into(),
                (false, false, true) => "heavy rain".into(),
                _ => "rain".into(),
            });
        } else if c.heat >= 48 && c.lava < 8 && c.fire < 32 {
            out.push("scorching heat".into());
        }
        if self.live_near(i, VOLCANO, 2) && c.lava < 8 {
            out.push("a volcano erupting nearby".into());
        }
        out
    }

    /// The earth at cell `i` now, the worst [`MAX_CLAUSES`] clauses (`None` when calm).
    pub fn describe(&self, i: u16) -> Option<String> {
        let mut c = self.clauses(i);
        c.truncate(MAX_CLAUSES);
        (!c.is_empty()).then(|| c.join("; "))
    }

    /// `"<head>: <the earth there>."`, cut to [`LINE_MAX`]; `None` when calm.
    pub fn line(&self, head: &str, at: LonLat) -> Option<String> {
        let d = self.describe(self.cell(at)?)?;
        Some(cut(&format!("{head}: {d}."), LINE_MAX))
    }

    /// One line per distinct cell of `who` (`(name, where)`), at most [`MAX_LINES`], first come first: `"The earth
    /// here, now: ..."` when all of them share one cell, else `"The earth where <name> stands, now: ..."`.
    pub fn lines_for(&self, who: &[(String, LonLat)]) -> Vec<String> {
        self.lines_with(who, true)
    }

    /// As [`Ground::lines_for`], always naming who stands there (`"The earth under <name>, now: ..."`): for prompts
    /// that look at the whole world (the narrator).
    pub fn lines_under(&self, who: &[(String, LonLat)]) -> Vec<String> {
        self.lines_with(who, false)
    }

    fn lines_with(&self, who: &[(String, LonLat)], here: bool) -> Vec<String> {
        let mut cells: Vec<(u16, Vec<&str>)> = vec![];
        for (name, at) in who {
            let Some(i) = self.cell(*at) else { continue };
            match cells.iter_mut().find(|c| c.0 == i) {
                Some(c) => c.1.push(name),
                None => cells.push((i, vec![name])),
            }
        }
        let one = here && cells.len() == 1;
        cells
            .iter()
            .filter_map(|(i, names)| {
                let d = self.describe(*i)?;
                let n: Vec<&str> = names.iter().take(2).copied().collect();
                let head = if one {
                    "The earth here, now".to_string()
                } else if here {
                    format!("The earth where {} {}, now", n.join(" and "), if n.len() > 1 { "stand" } else { "stands" })
                } else {
                    format!("The earth under {}, now", n.join(" and "))
                };
                Some(cut(&format!("{head}: {d}."), LINE_MAX))
            })
            .take(MAX_LINES)
            .collect()
    }

    /// What is already on the ground where each cast lands (`(what, its phenomenon kind, where)`; one line per distinct
    /// cell, at most [`MAX_LINES`]): `"Where Ash's Volcano lands, the earth now: ...; it meets: ...."`.
    pub fn cast_lines(&self, casts: &[(String, Option<u8>, LonLat)]) -> Vec<String> {
        let mut seen: Vec<u16> = vec![];
        let mut out = vec![];
        for (what, kind, at) in casts {
            let Some(i) = self.cell(*at) else { continue };
            if seen.contains(&i) {
                continue;
            }
            seen.push(i);
            let Some(d) = self.describe(i) else { continue };
            let meets = kind.and_then(|k| self.meets(k, i)).map(|m| format!("; it meets: {m}")).unwrap_or_default();
            out.push(cut(&format!("Where {what} lands, the earth now: {d}{meets}."), LINE_MAX + 60));
            if out.len() >= MAX_LINES {
                break;
            }
        }
        out
    }

    /// What a phenomenon of `kind` cast on cell `i` meets there, by the field's own interaction rules (§3), one clause.
    pub fn meets(&self, kind: u8, i: u16) -> Option<&'static str> {
        let c = self.sim.c(i);
        let sea = self.sim.is_sea(i);
        Some(match kind {
            FLOOD | STORM | TIDAL_WAVE if c.lava >= 8 => "water on this lava will flash to steam and harden it to rock",
            FLOOD | STORM | TIDAL_WAVE if c.fire >= 32 => "the water will drown the fires in clouds of steam",
            BLIZZARD if !sea && c.water >= 48 => "the floodwater will freeze solid",
            BLIZZARD if c.lava >= 8 => "snow on this lava will boil off as steam",
            WILDFIRE | LIGHTNING if !sea && c.water >= 48 => "fire will drown in the standing floodwater",
            WILDFIRE | LIGHTNING if c.cloud >= 160 => "the heavy rain will keep fire from spreading",
            WILDFIRE | LIGHTNING if c.dfuel <= -48 => "this ground already burnt; little left to catch",
            WILDFIRE | LIGHTNING if c.dfuel >= 48 => "the new forest here is fuel; it will burn fast",
            VOLCANO | METEOR if sea || c.water >= 48 || c.snow >= 96 => "lava into this water will boil it to steam and make new rock",
            VOLCANO if c.crack >= 128 => "magma will pour up through the open rift",
            // (a rupture within 2 cells of a volcano feeds it, and wakes a sleeping one)
            EARTHQUAKE if self.sim.phen.iter().any(|p| p.kind == VOLCANO && within(p.cell, i, 2)) => {
                "a quake this near the volcano can wake it"
            }
            EARTHQUAKE if c.crack >= 48 => "the cracked ground will split wider",
            SWARM if c.fire >= 32 => "the swarm will burn in the fires",
            SWARM if c.dfuel <= -48 => "nothing green left here for a swarm to eat",
            RAISE_LAND if !sea && c.water >= 48 => "rising ground will shed the floodwater",
            SINK_LAND if !sea && grid().coast[i as usize] <= 1 => "sinking land this near the coast will let the sea in",
            FOREST if c.ash >= 48 => "the ash will feed the new growth",
            _ => return None,
        })
    }

    /// The ground of a duel's stage at `at` (`None` when calm).
    pub fn stage(&self, at: LonLat) -> Option<Stage> {
        let i = self.cell(at)?;
        let line = self.describe(i)?;
        let c = *self.sim.c(i);
        let sea = self.sim.is_sea(i);
        let mut hazards: Vec<&'static str> = vec![];
        let mut props: Vec<&'static str> = vec![];
        if c.lava >= 8 {
            hazards.push("lava");
            props.push("lava");
            if c.lava >= 64 {
                props.push("lava");
            }
        }
        if c.fire >= 32 {
            hazards.push("fire");
            props.push("fire");
            if c.fire >= 128 {
                props.push("fire");
            }
        }
        let frozen = !sea && c.water >= 48 && c.heat <= -48;
        if !sea && c.water >= 48 {
            hazards.push(if frozen { "ice" } else { "flood" });
            props.push(if frozen { "ice" } else { "floodwater" });
            if c.water >= 96 {
                props.push(if frozen { "ice" } else { "floodwater" });
            }
        }
        if self.drowned(i) {
            hazards.push("sea");
            props.push("floodwater");
        }
        if c.crack >= 48 {
            hazards.push(if c.crack >= 128 { "rift" } else { "cracks" });
            props.push("fissure");
        }
        if c.shake >= 16 || (c.crack >= 48 && self.live_near(i, EARTHQUAKE, 4)) {
            hazards.push("quake");
        }
        if !sea && c.snow >= 48 {
            hazards.push("snow");
            props.push("snowdrift");
        }
        if c.swarm >= 64 {
            hazards.push("swarm");
        }
        if c.ash >= 48 || c.plume >= 96 {
            hazards.push("ash");
        }
        if c.ruin >= 96 {
            hazards.push("ruins");
            props.push("rubble");
        }
        let floor = if c.lava >= 8 {
            "lava"
        } else if c.fire >= 32 {
            "burning"
        } else if frozen {
            "frozen"
        } else if (!sea && c.water >= 48) || self.drowned(i) {
            "flooded"
        } else if c.crack >= 128 {
            "rift"
        } else if c.crack >= 48 {
            "cracked"
        } else if !sea && c.snow >= 48 {
            "snow"
        } else if c.ash >= 48 {
            "ash"
        } else if c.ruin >= 96 {
            "ruins"
        } else {
            "shaken"
        };
        let slippery = frozen || (!sea && c.water >= 48) || self.drowned(i) || (!sea && c.snow >= 96);
        props.truncate(4);
        Some(Stage { line, floor, hazards, slippery, props })
    }
}

/// Is cell `i` within `r` hops of cell `c`? (Cells are ~150 km apart: a pair farther than 250 km a hop is not, without
/// walking the disc.)
fn within(c: u16, i: u16, r: u8) -> bool {
    let g = grid();
    if c == i {
        return true;
    }
    if fn7_society::geo::distance_km(g.center[c as usize], g.center[i as usize]) > 250.0 * (r as f64 + 1.0) {
        return false;
    }
    g.disc(c, r).iter().any(|x| x.0 == i)
}

/// `s` cut to `n` characters (on a char boundary, with an ellipsis when cut).
fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('…');
        t
    }
}

/// [`Ground::lines_for`] in one call (empty when the earth underfoot is off or the places are calm).
pub fn lines(w: &World, st: &Settings, who: &[(String, LonLat)]) -> Vec<String> {
    Ground::of(w, st).map(|g| g.lines_for(who)).unwrap_or_default()
}
