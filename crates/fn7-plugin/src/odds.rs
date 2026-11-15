//! `game.duel_odds` (state schema 18; docs/CLIENT_API.md "Duel odds"): how a duel between two demigods would likely
//! go, from the duel rules (DM_RULEBOOK §21.2), before anyone challenges.
//!
//! The designer lost 7 of 7 duels; in 5 the AI challenged him and he typed nothing. This answers "should I fight, and
//! what if I don't type?".
//!
//! **The model.** Each blow is a d20 + modifier against DC 10 (+1 at night, + half the opponent's wardrobe stage): a
//! natural 1 misses, a natural 20 hits for 4, a success for 1 (+1 beating the DC by 5; the assumed average wit of +1
//! never reaches the +1 for wit 3), × `duel_damage_scale`. The modifier is the wit (assumed +1 for both: a short line
//! with a verb, the AI's counters are capped at +2), +1 hale (duel health 7+) / −1 hurt (3 or less), half the wardrobe
//! stage, +1 with any item, the gear's duel bonus, +1 on home turf (the duel is fought on the target's ground). The
//! duel runs `duel_deadline_ms / duel_counter_ms` exchanges (you type a line, they answer: the AI's counter comes every
//! `duel_counter_ms` whatever you do); a knockout ends it, else the one who took less damage wins (a tie is half).
//! [`TRIALS`] seeded trials (the match seed, both ids, the beat): the same state always gives the same odds.
//!
//! **Visibility.** Health is exact only where the asker already sees it (its own demigods, and both sides of an open
//! duel it can see, whose hp is public); otherwise a band (`hale` 7+, `hurt`, `near death` 3 or less), and the odds use
//! the band's middle, so they reveal no more than the band. A rival's private kit (non-public items, its gear's bonus)
//! is not counted; its public items and wardrobe are.

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use fn7_society::geo::{distance_km, region_at};
use serde_json::{json, Value};

use crate::settings::Settings;

/// Trials per answer.
pub const TRIALS: u32 = 600;
const ODDS_SALT: u64 = 0x0DD5;
/// The wit both sides are assumed to bring.
pub const WIT: i32 = 1;

/// A health band: what anyone may know of a demigod's health outside a duel.
pub fn band(h: f32) -> &'static str {
    if h >= 7.0 {
        "hale"
    } else if h <= 3.0 {
        "near death"
    } else {
        "hurt"
    }
}

/// The health the odds use for a band (its middle).
fn band_mid(b: &str, max: f32) -> f32 {
    match b {
        "hale" => (7.0 + max.max(7.0)) / 2.0,
        "near death" => 2.0,
        _ => 5.0,
    }
}

fn hp_term(h: f32) -> i32 {
    if h >= 7.0 {
        1
    } else if h <= 3.0 {
        -1
    } else {
        0
    }
}

/// One side as the odds see it.
struct Side {
    health: f32,
    exact: bool,
    /// The modifier before the health term.
    base: i32,
    wardrobe: u8,
    parts: Vec<String>,
}

fn side(w: &World, d: &Demigod, seen: bool, duel_hp: Option<f32>, region: PlaceId) -> Side {
    let me = Ref::Demigod(d.id);
    let story = w.dm.story(d.id);
    let wardrobe = story.map(|s| s.wardrobe).unwrap_or(0);
    let held: Vec<&Item> = w.items.iter().filter(|i| i.holders.last() == Some(&me) && i.ext.state == fn7_model::manifest::ItemState::Held).collect();
    let public = |i: &&Item| i.ext.manifestation.is_some() && i.ext.tier >= fn7_model::manifest::PUBLIC_TIER;
    let has_items = if seen { !held.is_empty() } else { held.iter().any(public) };
    let arms = if seen { crate::wills::arms::duel_bonus(w, d.id) } else { 0 };
    let home = d.home == Some(region);
    let base = WIT + wardrobe as i32 / 2 + has_items as i32 + arms + home as i32;
    let mut parts = vec![];
    if wardrobe >= 2 {
        parts.push(format!("wardrobe +{}", wardrobe / 2));
    }
    if has_items {
        parts.push("an item +1".into());
    }
    if arms != 0 {
        parts.push(format!("gear {arms:+}"));
    }
    if home {
        parts.push("home turf +1".into());
    }
    let (health, exact) = match duel_hp {
        Some(h) => (h, true),
        None if seen => (d.health, true),
        None => (band_mid(band(d.health), d.max_health), false),
    };
    Side { health, exact, base, wardrobe, parts }
}

/// One blow's damage: a seeded d20 + `m` against `dc`.
fn blow(r: &mut Rng, m: i32, dc: i32, scale: f32) -> f32 {
    let roll = 1 + r.below(20) as i32;
    let dmg = match roll {
        1 => 0.0,
        20 => 4.0,
        _ if roll + m >= dc => 1.0 + ((roll + m - dc) >= 5) as i32 as f32 + (WIT >= 3) as i32 as f32,
        _ => 0.0,
    };
    dmg * scale
}

/// The share of `TRIALS` duels `a` wins against `b` (`type_lines`: `a` types a line every exchange; else only `b` hits).
#[allow(clippy::too_many_arguments)]
fn simulate(seed: u64, key: &[u64], a: &Side, b: &Side, dc_a: i32, dc_b: i32, exchanges: u32, scale: f32, type_lines: bool) -> f64 {
    let mut r = Rng::derive(seed, key);
    let mut won = 0.0;
    for _ in 0..TRIALS {
        let (mut ha, mut hb) = (a.health, b.health);
        let (mut ta, mut tb) = (0.0f32, 0.0f32);
        let mut done = false;
        for _ in 0..exchanges {
            if type_lines {
                let x = blow(&mut r, a.base + hp_term(ha), dc_a, scale);
                hb -= x;
                tb += x;
                if hb <= 0.0 {
                    won += 1.0;
                    done = true;
                    break;
                }
            }
            let y = blow(&mut r, b.base + hp_term(hb), dc_b, scale);
            ha -= y;
            ta += y;
            if ha <= 0.0 {
                done = true;
                break;
            }
        }
        if !done {
            won += if ta < tb { 1.0 } else if ta == tb { 0.5 } else { 0.0 };
        }
    }
    won / TRIALS as f64
}

/// `game.duel_odds {demigod, target, level?}` on a world with the demigods, items, seats, places and the DM's light
/// state loaded. `asker`: the asking seat (`None`: public).
pub fn duel_odds(w: &World, asker: Option<u8>, a: DemigodId, b: DemigodId, level: Option<&str>) -> Result<Value, String> {
    let da = w.demigods.get(a.ix()).ok_or("`demigod`: no such demigod")?;
    let db = w.demigods.get(b.ix()).ok_or("`target`: no such demigod")?;
    if a == b {
        return Err("a demigod can't duel itself".into());
    }
    let st: Settings = crate::world::settings(w);
    let seen = |d: &Demigod| asker.is_some_and(|s| s == d.seat && s < crate::SYNTHETIC_SEAT);
    // An open duel between them: its hp is public.
    let bout = (!w.dm.duels.is_empty())
        .then(|| crate::duel::open_bouts(w).into_iter().find(|x| x.duel.f.iter().any(|f| f.id == a) && x.duel.f.iter().any(|f| f.id == b)))
        .flatten();
    let hp_in = |d: DemigodId| bout.as_ref().and_then(|x| x.duel.f.iter().position(|f| f.id == d).map(|i| x.hp[i]));
    let at = db.pos;
    let region = region_at(w, at);
    let sa = side(w, da, seen(da), hp_in(a), region);
    let sb = side(w, db, seen(db), hp_in(b), region);
    let night = crate::dm::is_night(w.meta.beat.max(1), &st) as i32;
    let dc_a = 10 + night + sb.wardrobe as i32 / 2;
    let dc_b = 10 + night + sa.wardrobe as i32 / 2;
    let exchanges = (st.duel_deadline_ms / st.duel_counter_ms.max(500)).clamp(1, 60);
    let key = [ODDS_SALT, a.0 as u64, b.0 as u64, w.meta.beat as u64, hash_str("duel_odds")];
    let win = simulate(w.meta.seed, &key, &sa, &sb, dc_a, dc_b, exchanges, st.duel_damage_scale, true);
    let idle = simulate(w.meta.seed, &key, &sa, &sb, dc_a, dc_b, exchanges, st.duel_damage_scale, false);
    let km = distance_km(da.pos, db.pos);
    let lvl = level.and_then(crate::duel::level_of).unwrap_or("regions");
    let reach = crate::duel::reach_km(lvl, &st);
    let reach_ok = da.alive && db.alive && km <= reach;
    let mut why: Vec<String> = vec![];
    let (ma, mb) = (sa.base + hp_term(sa.health), sb.base + hp_term(sb.health));
    why.push(format!("your blows: d20 {ma:+} vs DC {dc_a}; theirs: d20 {mb:+} vs DC {dc_b}"));
    if !sa.parts.is_empty() {
        why.push(format!("you: {}", sa.parts.join(", ")));
    }
    if !sb.parts.is_empty() {
        why.push(format!("them: {}", sb.parts.join(", ")));
    }
    if night == 1 {
        why.push("night: DC +1 for both".into());
    }
    if hp_term(sa.health) != 0 {
        why.push(if hp_term(sa.health) > 0 { "you are hale: +1".into() } else { "you are hurt: -1".into() });
    }
    if crate::table::controller(w, db) == "ai" {
        why.push(format!("the AI counters every {} s whether you type or not", st.duel_counter_ms as f32 / 1000.0));
    }
    why.push(format!("type nothing and you win {}%", (idle * 100.0).round()));
    if !reach_ok {
        why.push(if !(da.alive && db.alive) { "a fallen demigod can't duel".into() } else { format!("out of reach: {} km of {} allowed", km.round(), reach.round()) });
    }
    let health = |s: &Side, d: &Demigod| if s.exact { json!((s.health * 10.0).round() / 10.0) } else { json!(band(d.health)) };
    let mut v = json!({
        "demigod": Ref::Demigod(a), "target": Ref::Demigod(b),
        "win_pct": (win * 1000.0).round() / 10.0,
        "idle_win_pct": (idle * 1000.0).round() / 10.0,
        "my_health": health(&sa, da), "their_health": health(&sb, db),
        "my_mod": ma, "their_mod": mb, "my_dc": dc_a, "their_dc": dc_b,
        "exchanges": exchanges, "trials": TRIALS,
        "reach_ok": reach_ok, "reach_km": reach.round(), "distance_km": km.round(),
        "why": why,
    });
    if !reach_ok && da.alive && db.alive {
        v["too_far_km"] = json!((km - reach).max(0.0).round());
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_and_their_middles() {
        assert_eq!((band(10.0), band(7.0), band(5.0), band(3.0), band(0.5)), ("hale", "hale", "hurt", "near death", "near death"));
        assert_eq!(band_mid("hale", 10.0), 8.5);
    }

    #[test]
    fn typing_beats_silence_and_better_odds_with_better_modifiers() {
        let s = |h: f32, base: i32| Side { health: h, exact: true, base, wardrobe: 0, parts: vec![] };
        let k = [1, 2, 3];
        let even = simulate(7, &k, &s(10.0, 1), &s(10.0, 1), 10, 10, 10, 1.0, true);
        assert!((0.35..0.65).contains(&even), "{even}");
        let idle = simulate(7, &k, &s(10.0, 1), &s(10.0, 1), 10, 10, 10, 1.0, false);
        assert!(idle < 0.05, "{idle}");
        let strong = simulate(7, &k, &s(10.0, 4), &s(10.0, 1), 10, 10, 10, 1.0, true);
        assert!(strong > even + 0.15, "{strong} vs {even}");
        let weak = simulate(7, &k, &s(2.0, 1), &s(10.0, 1), 10, 10, 10, 1.0, true);
        assert!(weak < even, "{weak}");
        // Seeded: the same answer every time.
        assert_eq!(even, simulate(7, &k, &s(10.0, 1), &s(10.0, 1), 10, 10, 10, 1.0, true));
    }
}
