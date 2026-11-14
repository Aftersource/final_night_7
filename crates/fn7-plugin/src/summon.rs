//! Summoning (rules 0.8.0): a player drags one of its demigods onto the board or the globe (`fields.summon {demigod,
//! target}`, an instant action, any phase) and it appears there at the next resolve, this beat.
//!
//! - **The move** is a rules `Move` row, so the people-level occupancy rule nudges it off an occupied hex like any
//!   mover.
//! - **Recklessness:** a visible d20 (`summon:<beat>:<demigod>`) decides a mishap with chance
//!   `p = clamp(0.25 + 0.6 × health/max, 0, 0.95)` (DC `round(20p) + 1`, no modifier); a mishap costs
//!   `round(0.2 × health)` clamped to 1..2, × `summon_loss_scale` (none below 2.5 health: no loss is possible then).
//! - **The deed:** the summon mind (LLM, or its rules fallback) tells the transit in one hilarious line, which is
//!   recorded in the demigod's story (a tale) and in the memory at both places (`summon.departed`, `summoned`).
//! - Once per `summon_cooldown_beats` per demigod; `summon_enabled` turns it off.

use fn7_agents::summon::SummonCtx;
use fn7_model::dm::{Scope, Tale, TALES_KEPT};
use fn7_model::hexgrid::HexLevel;
use fn7_model::seed::hash_str;
use fn7_model::*;
use fn7_society::geo::{distance_km, place_at};
use serde_json::json;

use crate::actions::SummonOrder;
use crate::beat::Turn;
use crate::dm::{self, Decision};
use crate::settings::Settings;

/// Is this summoning allowed for `seat` at `beat` (against the last commit)?
pub fn check(w: &World, seat: u8, x: &SummonOrder, beat: u32, st: &Settings) -> Result<(), String> {
    // The refusal reasons are the client's contract: not_yours, fallen, cooldown, bad_target (and summon_disabled).
    if !st.summon_enabled {
        return Err("summon_disabled".into());
    }
    let Some(d) = w.demigods.get(x.demigod.ix()).filter(|d| d.seat == seat) else {
        return Err("not_yours".into());
    };
    if !d.alive {
        return Err("fallen".into());
    }
    if let Some(last) = w.dm.story(d.id).and_then(|s| s.last_summon) {
        if beat < last + st.summon_cooldown_beats.max(1) {
            return Err("cooldown".into());
        }
    }
    if !(x.target[0].is_finite() && x.target[1].is_finite()) {
        return Err("bad_target".into());
    }
    Ok(())
}

/// The chance of a mishap and the health it would cost, at `health` of `max`.
pub fn recklessness(health: f32, max: f32, scale: f32) -> (f64, f32) {
    let frac = (health / max.max(0.1)).clamp(0.0, 1.0) as f64;
    let p = (0.25 + 0.6 * frac).clamp(0.0, 0.95);
    let raw = (max as f64 * 0.2 * frac).round();
    let loss = if raw < 1.0 { 0.0 } else { raw.clamp(1.0, 2.0) as f32 * scale };
    (p, (loss * 10.0).round() / 10.0)
}

/// One summoning settled this beat, waiting for its deed.
#[derive(Debug, Clone)]
pub struct Pending {
    pub demigod: DemigodId,
    pub from: LonLat,
    pub from_cell: String,
    pub to: LonLat,
    pub lost: f32,
    /// The mishap check's face.
    pub roll: u8,
    pub ctx: SummonCtx,
}

/// Settle this beat's summonings (arrival order): move, roll the mishap, take the health. Returns the deeds to tell.
pub fn process(w: &mut World, beat: u32, turns: &[Turn], pantheon: &Pantheon, st: &Settings) -> Vec<Pending> {
    let mut order: Vec<(u8, &SummonOrder)> = turns.iter().flat_map(|t| t.input.summons.iter().map(move |x| (t.seat, x))).collect();
    order.sort_by_key(|(seat, x)| (x.seq, *seat, x.demigod));
    let mut out = vec![];
    dm::ensure_stories(w);
    for (seat, x) in order {
        if let Err(why) = check(w, seat, x, beat, st) {
            crate::world::event("summon_refused", Some(Ref::Demigod(x.demigod).to_string()),
                                json!({ "beat": beat, "reason": why }));
            continue;
        }
        let d = x.demigod;
        let (from, health, max, name, from_place) = {
            let me = &w.demigods[d.ix()];
            (me.pos, me.health, me.max_health, me.name.clone(), me.place)
        };
        let (p, loss) = recklessness(health, max, st.summon_loss_scale);
        let dc = ((20.0 * p).round() as i32 + 1).clamp(2, 20);
        let q = format!("summon:{beat}:{}", d.0);
        let roll = dm::d20(w, beat, &q, Ref::Demigod(d), 0, dc, "a summoning", x.target);
        let lost = if roll.verdict.ok() { 0.0 } else { loss };
        let to_place = place_at(w, x.target);
        let mut props = vec![Proposal {
            source: Source::Rules,
            intent: None,
            effect: Effect::Move { who: Ref::Demigod(d), to: x.target, place: Some(to_place) },
            act: format!("{name} is summoned to {}", w.name_of(Ref::Place(to_place))),
            actor: Ref::Demigod(d),
            magnitude: 4,
            valence: 0,
            witnesses: vec![Ref::Demigod(d)],
            cause: None,
        }];
        if lost > 0.0 {
            props.push(Proposal {
                source: Source::Rules,
                intent: None,
                effect: Effect::Damage { who: Ref::Demigod(d), amount: lost, cause: "a reckless summoning".into() },
                act: format!("{name} lands badly"),
                actor: Ref::Place(to_place),
                magnitude: 3,
                valence: -1,
                witnesses: vec![Ref::Demigod(d)],
                cause: None,
            });
        }
        crate::merge::run(w, props, pantheon, beat, st.max_witnesses as usize);
        if let Some(s) = dm::story_mut(w, d) {
            s.last_summon = Some(beat);
        }
        // The summoning interrupts what it was doing: its standing orders end (they'd walk it straight back).
        let doing_order = w.demigods[d.ix()].orders.first().map(|o| o.span.clone());
        w.demigods[d.ix()].orders.clear();
        // What it was doing: its latest row, else its first standing order.
        let me = Ref::Demigod(d);
        let doing = w
            .ledger
            .rows
            .iter()
            .rev()
            .take(400)
            .find(|r| r.actor == me && r.beat + 3 >= beat && r.verb != "move")
            .map(|r| r.act.clone())
            .or(doing_order)
            .unwrap_or_default();
        let (_, phase, _, _) = dm::phase_of(beat, st.beats_per_day);
        let to = w.demigods[d.ix()].pos;
        let ctx = SummonCtx {
            demigod: d,
            name: name.clone(),
            look: crate::visual::demigod(w, &w.demigods[d.ix()]).brief(),
            from_place: w.name_of(Ref::Place(from_place)),
            to_place: w.name_of(Ref::Place(to_place)),
            doing,
            km: distance_km(from, to),
            health_lost: lost,
            time: phase.into(),
            seed: fn7_model::seed::Rng::derive(w.meta.seed, &[hash_str(&q)]).next_u64(),
        };
        out.push(Pending { demigod: d, from, from_cell: people_cell(from), to, lost, roll: roll.roll, ctx });
    }
    out
}

/// Record the told deeds: a tale in the story, and the memory at both places (`summon.departed` where it was,
/// `summoned` where it landed: the public event the client flashes on).
pub fn record(w: &mut World, beat: u32, pending: &[Pending], deeds: &[String]) {
    for (k, p) in pending.iter().enumerate() {
        let deed = deeds.get(k).filter(|x| !x.is_empty()).cloned().unwrap_or_else(|| fn7_agents::summon::fallback(&p.ctx).deed);
        let d = p.demigod;
        let to = w.demigods[d.ix()].pos;
        let to_cell = people_cell(to);
        let from_wh = dm::locate(w, p.from, HexLevel::People);
        let to_wh = dm::locate(w, to, HexLevel::People);
        let data = json!({ "demigod": Ref::Demigod(d), "from": { "pos": p.from, "cell": p.from_cell },
                           "to": { "pos": to, "cell": to_cell }, "deed": deed, "health_lost": p.lost, "roll": p.roll });
        let name = w.demigods[d.ix()].name.clone();
        dm::remember(w, beat, &from_wh, Decision { scope: Scope::Cell, kind: "summon.departed", query: None, public: true,
                                                   text: format!("{name} vanished from here, summoned away"), data: data.clone() });
        dm::remember(w, beat, &to_wh, Decision { scope: Scope::Cell, kind: "summoned", query: Some(format!("summon:{beat}:{}:told", d.0)),
                                                 public: true, text: deed.clone(), data });
        if let Some(s) = dm::story_mut(w, d) {
            s.tales.push(Tale { beat, kind: "summoned".into(), text: deed });
            if s.tales.len() > TALES_KEPT {
                s.tales.remove(0);
            }
            s.deeds += 0.5;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recklessness_grows_with_health() {
        assert_eq!(recklessness(10.0, 10.0, 1.0), (0.85, 2.0));
        let (p, l) = recklessness(5.0, 10.0, 1.0);
        assert!((p - 0.55).abs() < 1e-9 && l == 1.0);
        assert_eq!(recklessness(2.0, 10.0, 1.0).1, 0.0, "no loss at low health");
        assert_eq!(recklessness(10.0, 10.0, 0.0).1, 0.0);
    }
}
