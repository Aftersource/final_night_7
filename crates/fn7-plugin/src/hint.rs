//! THE NEXT MOVE (state schema 18; docs/CLIENT_API.md "Next move hint", DM_RULEBOOK chapter 28).
//!
//! The designer, after playing, noted that the screen never says what the player should do next or why it matters.
//!
//! Every beat each human seat's SeatView (`round.brief.private`, `round.private_state.state`, `game.me`) carries one
//! private `hint`: an imperative line naming names and places (`text`, ≤ [`TEXT_MAX`] chars), what it changes (`why`,
//! ≤ [`WHY_MAX`]), the gesture the client maps it to (`action`), where to point (`target`), how pressing it is
//! (`urgency` 0..3) and which rule picked it (`kind`). The client draws it as one big line at top centre with an arrow
//! at the target.
//!
//! **Rules, not an LLM**, first match wins:
//!
//! 0. `duel`: one of yours is IN an open duel: "TYPE NOW" (`duel_type`, urgency 3; the AI's counters land whether
//!    you type or not).
//! 1. `danger`: a demigod of yours about to fall (health ≤ 40% of its cap) with a threat at hand: the earth field's
//!    hazard on its cell (lava, fire, flood, the sea, a rift, a quake, a swarm), Time's circle closing past it, a rival
//!    army within [`ARMY_KM`] (or hunting it within [`HUNT_KM`]), a rival demigod within [`RIVAL_KM`].
//!    Schema 22, after it: `own_hit`: a cast of yours last beat covered one of your own demigods (urgency 2).
//! 2. `resurrect` / `break_curse`: a fallen demigod of yours Death can raise (or whose raising quest is open), or one
//!    under a seal of no resurrection (name the counter-quest's step).
//! 3. `watch` (spectators: no demigod standing and none to raise): the biggest thing happening.
//! 4. `challenge`: a rival stands in the hex of one of yours (or within the challenge's always-reach).
//! 5. `event`: a public world event of the last beat near one of yours.
//! 6. `mega`: unspent bolts while a fault junction (or, without the junction rule, a fault) lies under a rival.
//! 7. `gear`: one of yours carries nothing: the nearest marked encounter hex or loot pile, or a will to arm it.
//! 8. `quest`: the current step of an open quest of yours (a curse's counter, a heal, a tempering, a raising).
//! 9. `will`: a concrete will from the demigod's situation (heal it, raise an army, march on the nearest rival).
//!
//! **Schema 22 (clear words).** Plain words (no tier jargon; a distance under 1 km is "right here"), and a low-urgency
//! hint (5..9, urgency ≤ 1) takes turns with the other low-urgency rules that apply, [`ROTATE_BEATS`] beats each,
//! from the state's beat, so no hint sits unchanged for 9 beats while there is something else to do.
//!
//! **Hidden information.** The hint reads only what the seat already sees: its own demigods in full; rivals' public
//! fields (position, name, alive; never health); armies' public fields (never orders); the earth field (public);
//! public ledger events; encounters as the public sees them (a hidden one is only its marked cell); loot (public);
//! the seat's own book of quests and curses. Built-in AI and bot seats get none.
//!
//! **Cheap and exact.** Read-only over the world the caller already has (`game.me` passes its partial one), bounded
//! scans (the last two beats of the ledger, at most [`SCAN_ROWS`] rows; the demigods; the live armies; the board's
//! encounters and loot), no wall time, no randomness: the same state gives the same hint. It is never stored, and it
//! appears only from state schema 18 on, so older recordings replay with their payloads unchanged.

use fn7_model::hexgrid::HexCell;
use fn7_model::*;
use fn7_society::geo::distance_km;
use serde::Serialize;
use serde_json::Value;

use crate::settings::Settings;
use crate::SYNTHETIC_SEAT;
use crate::wills::curses::{self, CurseKind};
use crate::wills::quests::{Goal, PlaceReq, Quest, StepKind};
use crate::world::{seat_state, settings};

/// Longest `text` (characters).
pub const TEXT_MAX: usize = 80;
/// Longest `why` (characters).
pub const WHY_MAX: usize = 100;
/// "About to fall": health at or below this share of the cap.
pub const LOW_SHARE: f32 = 0.4;
/// A rival army this near (km, planet; a city arena uses a hundredth) threatens.
pub const ARMY_KM: f64 = 600.0;
/// An army hunting the demigod (its target) threatens from this far.
pub const HUNT_KM: f64 = 1500.0;
/// A rival demigod this near threatens.
pub const RIVAL_KM: f64 = 300.0;
/// A public event this near one of yours is worth reacting to.
pub const EVENT_KM: f64 = 800.0;
/// Ledger rows looked at, at most (newest first, the last two beats).
pub const SCAN_ROWS: usize = 400;

/// The gesture the client maps a hint to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HintAction {
    DuelType,
    CastPower,
    TypeWill,
    Summon,
    Challenge,
    Heal,
    Flee,
    SeekGear,
    Explore,
    Resurrect,
    BreakCurse,
    Watch,
}

impl HintAction {
    pub const ALL: [&'static str; 12] = [
        "duel_type", "cast_power", "type_will", "summon", "challenge", "heal", "flee", "seek_gear", "explore", "resurrect", "break_curse", "watch",
    ];
}

/// Which rule picked a hint.
pub const KINDS: [&str; 12] = ["duel", "danger", "own_hit", "resurrect", "break_curse", "watch", "challenge", "event", "mega", "gear", "quest", "will"];

/// Schema 22: a low-urgency hint (urgency ≤ 1) stays at most this many beats while another low-urgency rule applies:
/// the applicable ones take turns, this many beats each, from the state's beat (no stored history).
pub const ROTATE_BEATS: u32 = 3;

/// Where the arrow points: a thing (`ref`), a point (`at`, `[lon, lat]`), a hex (`cell`); any of them.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Target {
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<Ref>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<LonLat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
}

impl Target {
    fn thing(r: Ref, at: LonLat) -> Target {
        Target { r#ref: Some(r), at: Some(at), cell: Some(people_cell(at)) }
    }
    fn point(at: LonLat) -> Target {
        Target { r#ref: None, at: Some(at), cell: Some(people_cell(at)) }
    }
}

/// The next move.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hint {
    pub text: String,
    pub why: String,
    pub action: HintAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    pub urgency: u8,
    pub kind: &'static str,
}

impl Hint {
    fn new(kind: &'static str, action: HintAction, urgency: u8, text: &[String], why: &[String], target: Option<Target>) -> Hint {
        Hint { text: pick(text, TEXT_MAX), why: pick(why, WHY_MAX), action, target, urgency: urgency.min(3), kind }
    }
    pub fn json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// The first candidate that fits `n` characters, else the last one cut to fit.
fn pick(cands: &[String], n: usize) -> String {
    cands.iter().find(|c| c.chars().count() <= n).cloned().unwrap_or_else(|| fit(cands.last().map(String::as_str).unwrap_or(""), n))
}

/// `s` cut to `n` characters at a word boundary when it can, with an ellipsis when cut.
pub fn fit(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n {
        return s.to_string();
    }
    let head: String = s.chars().take(n.saturating_sub(1)).collect();
    let cut = match head.rfind(' ') {
        Some(i) if i >= n / 2 => head[..i].trim_end_matches([',', ';', ':', '.', ' ']).to_string(),
        _ => head,
    };
    format!("{cut}…")
}

/// The next move for `seat`, or `None` (an older state, an AI or bot seat, a seat with no demigod).
pub fn for_seat(w: &World, seat: u8) -> Option<Hint> {
    if !w.hint_rules() || seat >= SYNTHETIC_SEAT {
        return None;
    }
    let s = seat_state(w, seat)?;
    if s.is_bot {
        return None;
    }
    let st = settings(w);
    let cx = Cx::new(w, seat, &st);
    if cx.mine.is_empty() {
        return None;
    }
    if !cx.clear {
        return in_duel(&cx)
            .or_else(|| danger(&cx))
            .or_else(|| fallen(&cx))
            .or_else(|| cx.live().next().is_none().then(|| watch(&cx)))
            .or_else(|| duel(&cx))
            .or_else(|| event(&cx))
            .or_else(|| mega(&cx))
            .or_else(|| gear(&cx))
            .or_else(|| quest(&cx))
            .or_else(|| will(&cx));
    }
    // Schema 22: the own-hit warning after danger; a low-urgency hint takes turns with the other applicable ones.
    if let Some(h) = in_duel(&cx)
        .or_else(|| danger(&cx))
        .or_else(|| own_hit(&cx))
        .or_else(|| fallen(&cx))
        .or_else(|| cx.live().next().is_none().then(|| watch(&cx)))
        .or_else(|| duel(&cx))
    {
        return Some(h);
    }
    let rules: [fn(&Cx) -> Option<Hint>; 5] = [event, mega, gear, quest, will];
    let mut found = rules.iter().filter_map(|r| r(&cx));
    let top = found.next()?;
    if top.urgency > 1 {
        return Some(top);
    }
    let mut pool = vec![top];
    pool.extend(found.filter(|h| h.urgency <= 1));
    let i = (cx.w.meta.beat / ROTATE_BEATS) as usize % pool.len();
    Some(pool.swap_remove(i))
}

/// The seat's goal line, every beat (human seats from schema 18): "Last standing wins. You: 2 alive, 3 rivals. Beat 41
/// of 60." Public facts only (who stands is public). `beat`: the beat to name (the brief names the coming one).
pub fn goal(w: &World, seat: u8, beat: u32) -> Option<String> {
    if !w.hint_rules() || seat >= SYNTHETIC_SEAT || seat_state(w, seat).is_none_or(|s| s.is_bot) {
        return None;
    }
    let mine = w.demigods.iter().filter(|d| d.seat == seat && d.alive).count();
    let mut rivals: Vec<u8> = w.demigods.iter().filter(|d| d.seat != seat && d.alive).map(|d| d.seat).collect();
    rivals.dedup();
    rivals.sort_unstable();
    rivals.dedup();
    let total = w.meta.beats_total;
    let when = if beat >= total { format!("Beat {beat} of {total}: the Final Night.") } else { format!("Beat {beat} of {total}.") };
    let r = rivals.len();
    Some(format!("Last standing wins. You: {mine} alive, {r} {}. {when}", if r == 1 { "rival" } else { "rivals" }))
}

/// What every rule reads.
struct Cx<'a> {
    w: &'a World,
    st: &'a Settings,
    seat: u8,
    /// The seat's demigods, in id order.
    mine: Vec<&'a Demigod>,
    /// Rivals standing (other players' demigods, the built-in AI's included), in id order.
    rivals: Vec<&'a Demigod>,
    /// km scale: 1 on the planet, a hundredth in a city.
    k: f64,
    beat: u32,
    /// The wills' book when the wills are on.
    book: Option<crate::wills::Book>,
    /// Schema 22: clear words.
    clear: bool,
}

impl<'a> Cx<'a> {
    fn new(w: &'a World, seat: u8, st: &'a Settings) -> Cx<'a> {
        let mine: Vec<&Demigod> = w.demigods.iter().filter(|d| d.seat == seat).collect();
        let rivals = w.demigods.iter().filter(|d| d.alive && d.seat != seat).collect();
        Cx {
            w,
            st,
            seat,
            mine,
            rivals,
            k: if w.is_planet() { 1.0 } else { 0.01 },
            beat: w.meta.beat.max(1),
            book: crate::wills::on(w).then(|| crate::wills::book(w)),
            clear: w.clear_words_rules(),
        }
    }
    fn live(&self) -> impl Iterator<Item = &&'a Demigod> {
        self.mine.iter().filter(|d| d.alive)
    }
    fn lead(&self) -> &'a Demigod {
        self.mine.iter().find(|d| d.alive).copied().unwrap_or(self.mine[0])
    }
    fn bolts(&self) -> u32 {
        self.lead().bolts
    }
    fn place(&self, d: &Demigod) -> String {
        self.w.name_of(Ref::Place(d.place))
    }
    fn region(&self, at: LonLat) -> String {
        crate::payload::region_name(self.w, at)
    }
    fn km(&self, km: f64) -> String {
        if self.k < 1.0 {
            format!("{:.1} km", km)
        } else {
            format!("{} km", km.round() as i64)
        }
    }
    /// Schema 22: is `km` "right here" (under 1 km on the planet; what would print as 0.0 km in a city)?
    fn here(&self, km: f64) -> bool {
        self.clear && km < if self.k < 1.0 { 0.05 } else { 1.0 }
    }
    /// "620 km away", or (schema 22) "right here" for a distance that rounds to nothing.
    fn away(&self, km: f64) -> String {
        if self.here(km) { "right here".into() } else { format!("{} away", self.km(km)) }
    }
    fn withered(&self, d: &Demigod) -> bool {
        self.book.as_ref().is_some_and(|b| curses::active(b, d.id, CurseKind::Wither, self.beat).is_some())
    }
    /// Can `d` be summoned to `to` now (the summoning's own check)?
    fn can_summon(&self, d: &Demigod, to: LonLat) -> bool {
        let x = crate::actions::SummonOrder { demigod: d.id, target: to, cell: None, seq: 0 };
        crate::summon::check(self.w, self.seat, &x, self.beat, self.st).is_ok()
    }
    fn in_duel(&self, d: DemigodId) -> bool {
        !self.w.dm.duels.is_empty() && crate::duel::open_bouts(self.w).iter().any(|b| b.duel.f.iter().any(|f| f.id == d))
    }
}

/// "your Left Hand" for a default-named body of yours, else its name.
fn me(d: &Demigod) -> String {
    match d.side {
        Some(Hand::Left) if d.name.ends_with("'s Left Hand") => "your Left Hand".into(),
        Some(Hand::Right) if d.name.ends_with("'s Right Hand") => "your Right Hand".into(),
        _ => d.name.clone(),
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn hp(x: f32) -> String {
    let r = (x * 10.0).round() / 10.0;
    if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{r:.1}") }
}

fn plural(n: u32, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

// ---------------------------------------------------------------------------------------------------------------------
// 1. danger
// ---------------------------------------------------------------------------------------------------------------------

enum Threat {
    /// The ground under it (a hazard word) and a calm point to leave for.
    Ground(&'static str, Option<LonLat>),
    /// Time's circle closes past it; the arena's centre.
    Circle(LonLat),
    Army { r: Ref, at: LonLat, owner: String, km: f64 },
    Rival { r: Ref, at: LonLat, name: String, km: f64 },
}

const HURTS: &[&str] = &["lava", "fire", "flood", "sea", "rift", "quake", "swarm"];

fn danger(cx: &Cx) -> Option<Hint> {
    let low: Vec<&Demigod> = cx.live().copied().filter(|d| d.health <= LOW_SHARE * d.max_health.max(0.1)).collect();
    if low.is_empty() {
        return None;
    }
    let ground = crate::earth::here::Ground::of(cx.w, cx.st);
    let centre = cx.w.places.get(cx.w.meta.arena.ix()).map(|p| p.center);
    let radius = fn7_society::arena_radius_at(cx.w, cx.w.meta.beat + 1);
    let mut found: Vec<(&Demigod, Threat)> = vec![];
    for d in low {
        let t = ground
            .as_ref()
            .and_then(|g| {
                let s = g.stage(d.pos)?;
                let h = s.hazards.iter().copied().find(|h| HURTS.contains(h))?;
                Some(Threat::Ground(h, calm_near(g, d.pos, centre)))
            })
            .or_else(|| centre.filter(|c| cx.w.is_planet() && distance_km(*c, d.pos) > radius).map(Threat::Circle))
            .or_else(|| {
                fn7_society::armies(cx.w)
                    .filter(|(_, a)| a.rogue || cx.w.demigods.get(a.demigod.ix()).is_some_and(|o| o.seat != cx.seat))
                    .map(|(c, a)| (c, a, distance_km(a.pos, d.pos)))
                    .filter(|(_, a, km)| *km <= ARMY_KM * cx.k || (a.target == Some(Ref::Demigod(d.id)) && *km <= HUNT_KM * cx.k))
                    .min_by(|x, y| x.2.total_cmp(&y.2).then(x.0.id.cmp(&y.0.id)))
                    .map(|(c, a, km)| Threat::Army {
                        r: Ref::Cluster(c.id),
                        at: a.pos,
                        owner: if a.rogue { "a rogue".into() } else { format!("{}'s", cx.w.demigods[a.demigod.ix()].player_name()) },
                        km,
                    })
            })
            .or_else(|| {
                cx.rivals
                    .iter()
                    .map(|r| (r, distance_km(r.pos, d.pos)))
                    .filter(|(_, km)| *km <= RIVAL_KM * cx.k)
                    .min_by(|x, y| x.1.total_cmp(&y.1).then(x.0.id.cmp(&y.0.id)))
                    .map(|(r, km)| Threat::Rival { r: Ref::Demigod(r.id), at: r.pos, name: r.name.clone(), km })
            });
        if let Some(t) = t {
            found.push((d, t));
        }
    }
    // The weakest first (then the lower id).
    let (d, t) = found.into_iter().min_by(|a, b| a.0.health.total_cmp(&b.0.health).then(a.0.id.cmp(&b.0.id)))?;
    let (who, place, h) = (me(d), cx.place(d), hp(d.health));
    let who_c = cap(&who);
    Some(match t {
        Threat::Ground(hz, calm) => {
            let what = match hz {
                "lava" => "lava",
                "fire" => "fire",
                "flood" => "floodwater",
                "sea" => "the sea",
                "rift" => "an open rift",
                "quake" => "a quake",
                _ => "a swarm",
            };
            let to = calm.or(centre).unwrap_or(d.pos);
            let summon = cx.can_summon(d, to);
            let why = [format!("{who_c} has {h} health and the {hz} hurts it every beat it stays"), format!("{who_c} has {h} health; the {hz} hurts it every beat")];
            if summon {
                Hint::new("danger", HintAction::Summon, 3,
                          &[format!("{who_c} is caught in {what} at {place}: drag it to safe ground"),
                            format!("{who_c} is caught in {what}: drag it to safe ground"), format!("Drag {who} out of the {hz}")],
                          &why, Some(Target::thing(Ref::Demigod(d.id), to)))
            } else {
                Hint::new("danger", HintAction::Flee, 3,
                          &[format!("{who_c} is caught in {what} at {place}: order it out"),
                            format!("{who_c} is caught in {what}: order it out"), format!("Get {who} out of the {hz}")],
                          &why, Some(Target::thing(Ref::Demigod(d.id), to)))
            }
        }
        Threat::Circle(c) => {
            let why = [format!("Time's circle closes past {who} next beat and it has {h} health"), format!("Time's circle closes past it; {h} health left")];
            if cx.can_summon(d, c) {
                Hint::new("danger", HintAction::Summon, 3,
                          &[format!("{who_c} is outside Time's circle at {place}: drag it inside"), format!("Drag {who} inside Time's circle")],
                          &why, Some(Target::thing(Ref::Demigod(d.id), c)))
            } else {
                Hint::new("danger", HintAction::Flee, 3,
                          &[format!("{who_c} is outside Time's circle at {place}: order it inward"), format!("Move {who} inside Time's circle")],
                          &why, Some(Target::thing(Ref::Demigod(d.id), c)))
            }
        }
        Threat::Army { r, at, owner, km } => {
            let foe = format!("{owner} army");
            attacked(cx, d, &who, &place, &h, r, at, &foe, km)
        }
        Threat::Rival { r, at, name, km } => attacked(cx, d, &who, &place, &h, r, at, &name, km),
    })
}

#[allow(clippy::too_many_arguments)]
fn attacked(cx: &Cx, d: &Demigod, who: &str, place: &str, h: &str, r: Ref, at: LonLat, foe: &str, km: f64) -> Hint {
    let dist = cx.away(km);
    let foe_c = cap(foe);
    let who_c = cap(who);
    let lower = who;
    if cx.bolts() >= 1 {
        Hint::new("danger", HintAction::CastPower, 3,
                  &[format!("{who_c} is under attack in {place}: drag a power onto {foe}"),
                    format!("{who_c} is under attack in {place}: drag a power onto it"),
                    format!("{who_c} is under attack: drag a power onto {foe}"),
                    format!("{who_c} is under attack: drag a power onto it")],
                  &[format!("{foe_c} is {dist} and {lower} has {h} health"), format!("{foe_c} is {dist}; {h} health left")],
                  Some(Target::thing(r, at)))
    } else if !cx.withered(d) {
        Hint::new("danger", HintAction::Heal, 3,
                  &[format!("{who_c} is under attack in {place}: type a will to heal it"), format!("Heal {lower}: type a will to mend it")],
                  &[format!("{foe_c} is {dist}, {lower} has {h} health and you have no bolts"), format!("{foe_c} is {dist}; {h} health, no bolts")],
                  Some(Target::thing(Ref::Demigod(d.id), d.pos)))
    } else {
        Hint::new("danger", HintAction::Flee, 3,
                  &[format!("{who_c} is under attack in {place}: order it away from {foe}"), format!("Get {lower} away from {foe}")],
                  &[format!("{foe_c} is {dist}, {lower} has {h} health and a withering blocks heals"), format!("{foe_c} is {dist}; {h} health, no heals")],
                  Some(Target::thing(Ref::Demigod(d.id), d.pos)))
    }
}

/// Schema 22, 1b. `own_hit`: a cast of yours in the last beat covered one of your own demigods (its row names it as a
/// witness or target). No rule change: it was hurt as anyone there; the hint warns, once, the beat after.
fn own_hit(cx: &Cx) -> Option<Hint> {
    let w = cx.w;
    let mine: Vec<Ref> = cx.mine.iter().map(|d| Ref::Demigod(d.id)).collect();
    // The beat just resolved (rows of the last two beats, as the event rule reads them).
    let since = w.meta.beat.saturating_sub(1);
    let (r, d) = w.ledger.rows.iter().rev().take(SCAN_ROWS).take_while(|r| r.beat >= since).find_map(|r| {
        if r.verb != "cast" || !mine.contains(&r.actor) {
            return None;
        }
        let hit = r.targets.iter().chain(r.witnessed_by.iter()).find(|x| mine.contains(x))?;
        let Ref::Demigod(id) = *hit else { return None };
        Some((r, w.demigods.get(id.ix())?))
    })?;
    let who = me(d);
    let at = r.at.unwrap_or(d.pos);
    let (wh, _) = crate::payload::where_words(w, at);
    let power = r.act.split_once(" casts ").map(|(_, x)| x.split(" on ").next().unwrap_or(x).trim().to_string()).filter(|x| !x.is_empty());
    let what = power.map(|p| format!("Your {p}")).unwrap_or_else(|| "Your cast".into());
    let why = [format!("{} stood inside your cast's reach and took the blow like anyone there", cap(&who)), "Your casts hurt your own demigods too".into()];
    let text = [format!("{what} hit your own {} {wh}: aim clear of it", who.trim_start_matches("your ")),
                format!("{what} hit {who}: aim clear of it"), format!("Your cast hit {who}")];
    if d.alive && d.health < d.max_health && !cx.withered(d) {
        return Some(Hint::new("own_hit", HintAction::Heal, 2, &text, &why, Some(Target::thing(Ref::Demigod(d.id), d.pos))));
    }
    Some(Hint::new("own_hit", HintAction::CastPower, 2, &text, &why, Some(Target::thing(Ref::Demigod(d.id), d.pos))))
}

/// A calm cell next to `at` (rings 1 and 2), the one nearest the arena's centre (then the lowest index).
fn calm_near(g: &crate::earth::here::Ground, at: LonLat, centre: Option<LonLat>) -> Option<LonLat> {
    let grid = crate::earth::grid::grid();
    let i = g.cell(at)?;
    let c = centre.unwrap_or(at);
    grid.disc(i, 2)
        .into_iter()
        .filter(|(j, _)| *j != i && grid.land(*j) && g.describe(*j).is_none())
        .map(|(j, _)| (j, grid.center[j as usize]))
        .min_by(|a, b| distance_km(a.1, c).total_cmp(&distance_km(b.1, c)).then(a.0.cmp(&b.0)))
        .map(|(_, p)| p)
}

// ---------------------------------------------------------------------------------------------------------------------
// 2. the fallen
// ---------------------------------------------------------------------------------------------------------------------

fn step_of(q: &Quest) -> Option<(usize, &crate::wills::quests::Step)> {
    let i = q.current()?;
    Some((i, &q.steps[i]))
}

fn step_target(q: &Quest) -> Option<Target> {
    let (_, s) = step_of(q)?;
    match &s.kind {
        StepKind::Army { at, .. } => Some(Target::point(*at)),
        StepKind::At { req: PlaceReq::Point { at, .. }, .. } => Some(Target::point(*at)),
        _ => s.near.as_ref().map(|n| Target::point(n.at)),
    }
}

fn step_text(s: &crate::wills::quests::Step) -> String {
    s.text.trim().trim_end_matches('.').to_string()
}

fn fallen(cx: &Cx) -> Option<Hint> {
    let b = cx.book.as_ref()?;
    for d in cx.mine.iter().filter(|d| !d.alive) {
        let who = me(d);
        if let Some(cu) = curses::active(b, d.id, CurseKind::Seal, cx.beat) {
            let q = b.quests.iter().find(|q| q.id == cu.quest && q.open());
            let (text, why, target) = match q.and_then(|q| step_of(q).map(|s| (q, s))) {
                Some((q, (i, s))) => (
                    vec![format!("Break the seal on {who}: {}", step_text(s)), format!("Break the seal: {}", step_text(s))],
                    vec![format!("A seal of no resurrection holds {who}; this is step {} of {} of its counter-quest", i + 1, q.steps.len()),
                         format!("Sealed: step {} of {} of the counter-quest frees it", i + 1, q.steps.len())],
                    step_target(q).or_else(|| Some(Target::thing(Ref::Demigod(d.id), d.pos))),
                ),
                None => (
                    vec![format!("Break the seal on {who}: finish its counter-quest")],
                    vec![format!("A seal of no resurrection holds {who}; nothing raises it until it breaks")],
                    Some(Target::thing(Ref::Demigod(d.id), d.pos)),
                ),
            };
            return Some(Hint::new("break_curse", HintAction::BreakCurse, 2, &text, &why, target));
        }
        if let Some(q) = b.quests.iter().find(|q| q.open() && q.demigod == d.id && q.takes(cx.seat) && matches!(q.goal, Goal::Raise { .. })) {
            if let Some((i, s)) = step_of(q) {
                return Some(Hint::new("resurrect", HintAction::Resurrect, 2,
                    &[format!("Raise {who}: {}", step_text(s))],
                    &[format!("Death's quest, step {} of {}: finish it and {who} rises", i + 1, q.steps.len()),
                      format!("Step {} of {} of Death's quest", i + 1, q.steps.len())],
                    step_target(q).or_else(|| Some(Target::thing(Ref::Demigod(d.id), d.pos)))));
            }
        }
        let granted = b.raises.iter().filter(|r| r.by_seat == cx.seat).count() as u32;
        if granted < cx.st.raise_cap {
            let (price, _) = crate::wills::raise_price(cx.w, b, d.id, cx.beat, cx.st);
            let lead = cx.lead();
            let (sparks, bolts) = (lead.sparks, lead.bolts);
            return Some(Hint::new("resurrect", HintAction::Resurrect, 2,
                &[format!("Raise {who}: type a will to bring it back from the dead"), format!("Raise {who}: type a will to bring it back")],
                &[format!("Death asks {} for it; you have {} and {}", plural(price, "spark"), plural(sparks, "spark"), plural(bolts, "bolt")),
                  format!("Death asks {} sparks; you have {sparks} and {bolts} bolts", price)],
                Some(Target::thing(Ref::Demigod(d.id), d.pos))));
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// 3. spectators
// ---------------------------------------------------------------------------------------------------------------------

fn watch(cx: &Cx) -> Hint {
    let w = cx.w;
    let why = match &cx.book {
        Some(b) if b.raises.iter().filter(|r| r.by_seat == cx.seat).count() as u32 >= cx.st.raise_cap => {
            "Both your demigods have fallen and Death will raise no more for you".to_string()
        }
        _ => "Both your demigods have fallen; you watch the Final Night play out".to_string(),
    };
    // An open duel first (public: who and where).
    if !w.dm.duels.is_empty() {
        if let Some(b) = crate::duel::open_bouts(w).into_iter().next() {
            let (a, z) = (&b.duel.f[0].name, &b.duel.f[1].name);
            return Hint::new("watch", HintAction::Watch, 0,
                &[format!("Watch {a} duel {z} in {}", b.duel.place), format!("Watch {a} duel {z}"), "Watch the duel".into()],
                &[why], Some(Target::point(b.duel.at)));
        }
    }
    if let Some((r, at)) = recent_public(w).into_iter().filter_map(|r| crate::payload::geotag(w, r).0.map(|a| (r, a))).max_by(|a, b| {
        a.0.magnitude.cmp(&b.0.magnitude).then(a.0.id.cmp(&b.0.id))
    }) {
        let region = cx.region(at);
        return Hint::new("watch", HintAction::Watch, 0,
            &[format!("Watch {region}: {}", r.act.trim().trim_end_matches('.'))],
            &[why], Some(Target::point(at)));
    }
    let arena = w.places.get(w.meta.arena.ix());
    Hint::new("watch", HintAction::Watch, 0,
        &[format!("Watch Time's circle close on {}", arena.map(|p| p.name.as_str()).unwrap_or("the arena"))],
        &[why], arena.map(|p| Target::point(p.center)))
}

/// The public rows of the last two beats, newest first (at most [`SCAN_ROWS`] looked at).
fn recent_public(w: &World) -> Vec<&LedgerRow> {
    let since = w.meta.beat.saturating_sub(1);
    w.ledger
        .rows
        .iter()
        .rev()
        .take(SCAN_ROWS)
        .take_while(|r| r.beat >= since)
        .filter(|r| crate::payload::is_public_event(w, r))
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// 4. duels and challenges
// ---------------------------------------------------------------------------------------------------------------------

/// 0. One of yours is IN a duel: type now (every line is a blow; the AI's counters land whether you type or not).
fn in_duel(cx: &Cx) -> Option<Hint> {
    if cx.w.dm.duels.is_empty() {
        return None;
    }
    for b in crate::duel::open_bouts(cx.w) {
        if b.ko.is_some() {
            continue;
        }
        let Some(side) = b.duel.f.iter().position(|f| cx.mine.iter().any(|d| d.id == f.id)) else { continue };
        let foe = &b.duel.f[1 - side];
        let secs = hp(cx.st.duel_counter_ms as f32 / 1000.0);
        let why = if foe.ai {
            vec![format!("They hit you every {secs} s; each line is a blow"), "Each line you type is a blow".into()]
        } else {
            vec![format!("Each line is a blow; idle {secs} s and their blows carry counters"), "Each line you type is a blow".into()]
        };
        return Some(Hint::new("duel", HintAction::DuelType, 3,
            &[format!("You're in a duel with {}: TYPE NOW", foe.name), "You're in a duel: TYPE NOW".into()],
            &why, Some(Target::thing(Ref::Demigod(foe.id), b.duel.at))));
    }
    None
}

fn duel(cx: &Cx) -> Option<Hint> {
    if !cx.st.challenge_enabled {
        return None;
    }
    let reach = cx.st.challenge_min_km.max(0.0) as f64 * cx.k;
    let mut best: Option<(&Demigod, &Demigod, f64)> = None;
    for d in cx.live() {
        if cx.in_duel(d.id) {
            continue;
        }
        let cell = people_cell(d.pos);
        for r in &cx.rivals {
            let km = distance_km(d.pos, r.pos);
            if (people_cell(r.pos) == cell || km <= reach) && !cx.in_duel(r.id) && best.is_none_or(|b| km < b.2) {
                best = Some((d, r, km));
            }
        }
    }
    let (d, r, km) = best?;
    let who = me(d);
    let place = cx.place(d);
    Some(Hint::new("challenge", HintAction::Challenge, 2,
        &[format!("{} stands by {who} in {place}: challenge it", r.name), format!("Challenge {} with {who}", r.name), format!("Challenge {}", r.name)],
        &[format!("It is {}, in reach: a duel won by knockout eliminates it", cx.away(km)), "In reach: a knockout eliminates it".into()],
        Some(Target::thing(Ref::Demigod(r.id), r.pos))))
}

// ---------------------------------------------------------------------------------------------------------------------
// 5. world events
// ---------------------------------------------------------------------------------------------------------------------

/// A public event's opening words, by its verb.
fn event_word(w: &World, r: &LedgerRow) -> Option<&'static str> {
    let entropy = matches!(r.source, Source::God { god, .. } if w.gods.get(god.ix()).is_some_and(|g| g.name == "Entropy"));
    Some(match r.verb.as_str() {
        _ if entropy => "Entropy struck",
        "cast" => "A cast struck",
        "happen" => "Something struck",
        "raise_army" => "An army rose",
        "eliminate" => "A demigod fell",
        "revive" => "A demigod rose",
        "wound" => "A god was wounded",
        _ => return None,
    })
}

fn event(cx: &Cx) -> Option<Hint> {
    let w = cx.w;
    let mine: Vec<Ref> = cx.mine.iter().flat_map(|d| crate::world::body_selves(w, d.id)).collect();
    let mut best: Option<(&LedgerRow, LonLat, &Demigod, f64)> = None;
    for r in recent_public(w) {
        if r.magnitude < 4 || mine.contains(&r.actor) {
            continue;
        }
        if event_word(w, r).is_none() {
            continue;
        }
        let Some(at) = crate::payload::geotag(w, r).0 else { continue };
        let Some((d, km)) = cx.live().map(|d| (*d, distance_km(d.pos, at))).min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.id.cmp(&b.0.id))) else {
            continue;
        };
        if km > EVENT_KM * cx.k {
            continue;
        }
        // The biggest, then the newest (rows come newest first).
        if best.is_none_or(|b| r.magnitude > b.0.magnitude) {
            best = Some((r, at, d, km));
        }
    }
    let (r, at, d, km) = best?;
    let word = event_word(w, r)?;
    let who = me(d);
    let region = cx.region(at);
    let act = r.act.trim().trim_end_matches('.');
    Some(Hint::new("event", HintAction::TypeWill, if r.magnitude >= 7 { 2 } else { 1 },
        &[format!("{word} near {who} in {region}: type a will to react"), format!("{word} near {who}: type a will to react")],
        &[if cx.here(km) { format!("Right where {who} stands: {act}") } else { format!("{} away: {act}", cap(&cx.km(km))) }, act.to_string()],
        Some(Target { r#ref: Some(Ref::Ledger(r.id)), at: Some(at), cell: Some(people_cell(at)) })))
}

// ---------------------------------------------------------------------------------------------------------------------
// 6. a mega at a fault junction
// ---------------------------------------------------------------------------------------------------------------------

fn mega(cx: &Cx) -> Option<Hint> {
    let bolts = cx.bolts();
    if bolts == 0 || !cx.w.dm_rules() {
        return None;
    }
    let junctions = crate::faults::junction_rules(cx.w, cx.st);
    let want = if junctions { "junction" } else { "fault" };
    let (r, a) = cx
        .rivals
        .iter()
        .map(|r| (*r, crate::faults::assess(r.pos, cx.st, junctions)))
        .filter(|(_, a)| a.tier == want)
        .max_by(|x, y| x.1.damage.total_cmp(&y.1.damage).then(y.0.id.cmp(&x.0.id)))?;
    let wh = match &a.junction {
        Some((j, _, _)) => j.phrase(),
        None => "on a fault line".into(),
    };
    let x = (a.damage as f64 * 10.0).round() / 10.0;
    if cx.clear {
        // Schema 22: plain words, no tier jargon.
        let because = match &a.junction {
            Some((j, _, _)) if j.kind == crate::faults::JunctionKind::Knot => "fault lines tangle there",
            Some(_) => "plate boundaries meet there",
            None => "it sits on a fault line",
        };
        return Some(Hint::new("mega", HintAction::CastPower, 1,
            &[format!("Cast a mega on {} {wh}: drag a power there", r.name), format!("Cast a mega on {} {wh}", r.name), format!("Cast a mega on {}", r.name)],
            &[format!("{}, so it hits {x}x as hard; you have {}", cap(because), plural(bolts, "bolt")), format!("{}: {x}x as hard; {} left", cap(because), plural(bolts, "bolt")),
              format!("It hits {x}x as hard there; {} left", plural(bolts, "bolt"))],
            Some(Target::thing(Ref::Demigod(r.id), r.pos))));
    }
    Some(Hint::new("mega", HintAction::CastPower, 1,
        &[format!("Cast a mega on {} {wh}: drag a power there", r.name), format!("Cast a mega on {} {wh}", r.name), format!("Cast a mega on {}", r.name)],
        &[format!("A mega there hits {x}x as hard ({} tier); you have {}", a.tier, plural(bolts, "bolt")), format!("{x}x damage there; {} left", plural(bolts, "bolt"))],
        Some(Target::thing(Ref::Demigod(r.id), r.pos))))
}

// ---------------------------------------------------------------------------------------------------------------------
// 7. gear
// ---------------------------------------------------------------------------------------------------------------------

fn holds_any(w: &World, d: DemigodId) -> bool {
    let me = Ref::Demigod(d);
    w.items.iter().any(|i| i.holders.last() == Some(&me))
}

fn gear(cx: &Cx) -> Option<Hint> {
    let w = cx.w;
    let d = cx.live().copied().find(|d| !holds_any(w, d.id))?;
    let who = me(d);
    // Hidden encounters only as their marked cell; loot piles as the public sees them.
    let mut best: Option<(f64, Target, String, bool)> = None;
    for e in w.dm.encounters.iter().filter(|e| e.faded_beat.is_none() && e.outcome.is_none()) {
        let Some(at) = HexCell::parse(&e.cell).map(|c| c.center()) else { continue };
        let km = distance_km(d.pos, at);
        if best.as_ref().is_none_or(|b| km < b.0) {
            best = Some((km, Target { r#ref: None, at: Some(at), cell: Some(e.cell.clone()) }, "the marked hex".into(), true));
        }
    }
    let named = crate::look::on(w);
    for l in w.dm.loot.iter().filter(|l| l.taken_beat.is_none() && !l.items.is_empty()) {
        let km = distance_km(d.pos, l.at);
        if best.as_ref().is_none_or(|b| km < b.0) {
            let what = match l.items.first().and_then(|i| w.items.get(i.ix())) {
                Some(it) if named => it.name.clone(),
                _ => "the loot".into(),
            };
            let r = l.items.first().map(|i| Ref::Item(*i));
            best = Some((km, Target { r#ref: r, at: Some(l.at), cell: Some(l.cell.clone()) }, what, false));
        }
    }
    if let Some((km, t, what, enc)) = best {
        let region = cx.region(t.at.unwrap_or(d.pos));
        let dist = cx.away(km);
        let short = if cx.here(km) { dist.clone() } else { cx.km(km) };
        let (text, why) = if enc {
            (vec![format!("Send {who} to the marked hex in {region}: gear may wait there"), format!("Send {who} to the marked hex in {region}")],
             vec![format!("{} carries nothing; an encounter won there drops gear ({dist})", cap(&who)), format!("No gear; a won encounter drops some ({short})")])
        } else {
            (vec![format!("Send {who} to {what} in {region}: pick it up"), format!("Send {who} to the loot in {region}")],
             vec![format!("{} carries nothing; whoever steps on the pile takes it ({dist})", cap(&who)), format!("No gear; the pile is {dist}")])
        };
        return Some(Hint::new("gear", HintAction::SeekGear, 1, &text, &why, Some(t)));
    }
    if cx.book.is_some() {
        let side = match d.side { Some(Hand::Right) => "r", _ => "l" };
        return Some(Hint::new("gear", HintAction::TypeWill, 1,
            &[format!("Arm {who}: type a will like \"{side}: forge me a blessed spear\""), format!("Arm {who}: type a will to forge it a weapon")],
            &[format!("{} carries nothing; a will to arm it rolls a d20 against a low DC", cap(&who))],
            Some(Target::thing(Ref::Demigod(d.id), d.pos))));
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// 8. quests
// ---------------------------------------------------------------------------------------------------------------------

fn quest(cx: &Cx) -> Option<Hint> {
    let b = cx.book.as_ref()?;
    let w = cx.w;
    let mine = |d: DemigodId| cx.mine.iter().any(|x| x.id == d);
    // The seat's own quests for its own demigods first, then the ones it took up; by id.
    let q = b
        .quests
        .iter()
        .filter(|q| q.open() && q.takes(cx.seat) && q.current().is_some())
        .min_by_key(|q| (!mine(q.demigod), q.id))?;
    let (i, s) = step_of(q)?;
    let n = q.steps.len();
    let for_whom = w.demigods.get(q.demigod.ix()).map(|d| if d.seat == cx.seat { me(d) } else { d.name.clone() }).unwrap_or_default();
    let (action, effect) = match &q.goal {
        Goal::BreakCurse { curse_kind, .. } => (HintAction::BreakCurse, format!("breaks {} on {for_whom}", curse_kind.title())),
        Goal::Raise { .. } => (HintAction::Resurrect, format!("raises {for_whom}")),
        Goal::Heal { amount } => (step_action(&s.kind), format!("heals {for_whom} {} more", hp(*amount))),
        Goal::Boon { bonus, beats } => (step_action(&s.kind), format!("blesses {for_whom} +{bonus} for {beats} beats")),
        Goal::Temper { tier, .. } => (step_action(&s.kind), format!("tempers {for_whom}'s gear to {}", tier_word(*tier))),
        Goal::Arm { slot, .. } => (step_action(&s.kind), format!("forges {for_whom}'s {slot}")),
    };
    let st = step_text(s);
    let kind = if action == HintAction::BreakCurse { "break_curse" } else { "quest" };
    let mut why = vec![format!("Step {} of {n}: finishing the quest {effect}", i + 1)];
    if !s.progress.is_empty() {
        why.insert(0, format!("Step {} of {n}, {}: the quest {effect}", i + 1, s.progress.trim().trim_end_matches('.')));
    }
    why.push(format!("Step {} of {n} of the quest", i + 1));
    Some(Hint::new(kind, action, 1, &[format!("Quest for {for_whom}: {st}"), st.clone()], &why,
                   step_target(q).or_else(|| w.demigods.get(q.demigod.ix()).filter(|d| d.alive).map(|d| Target::thing(Ref::Demigod(d.id), d.pos)))))
}

fn step_action(k: &StepKind) -> HintAction {
    match k {
        StepKind::At { .. } => HintAction::Summon,
        StepKind::Hold { .. } | StepKind::Encounter => HintAction::SeekGear,
        StepKind::Duel => HintAction::Challenge,
        StepKind::Vigil { .. } => HintAction::Watch,
        StepKind::Shrine | StepKind::Sparks { .. } | StepKind::Army { .. } => HintAction::TypeWill,
    }
}

fn tier_word(t: fn7_model::manifest::Tier) -> String {
    serde_json::to_value(t).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "a higher tier".into())
}

// ---------------------------------------------------------------------------------------------------------------------
// 9. a will to type
// ---------------------------------------------------------------------------------------------------------------------

fn will(cx: &Cx) -> Option<Hint> {
    let w = cx.w;
    // The most hurt of yours standing (then the lower id).
    let d = cx.live().copied().min_by(|a, b| (a.health / a.max_health.max(0.1)).total_cmp(&(b.health / b.max_health.max(0.1))).then(a.id.cmp(&b.id)))?;
    let who = me(d);
    let place = cx.place(d);
    if d.health < 0.7 * d.max_health && !cx.withered(d) {
        return Some(Hint::new("will", HintAction::Heal, 0,
            &[format!("Heal {who} in {place}: type a will to mend its wounds"), format!("Heal {who}: type a will to mend it")],
            &[format!("{} has {} of {} health; the next blow could be its last", cap(&who), hp(d.health), hp(d.max_health))],
            Some(Target::thing(Ref::Demigod(d.id), d.pos))));
    }
    let near = cx.rivals.iter().map(|r| (*r, distance_km(r.pos, d.pos))).min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.id.cmp(&b.0.id)));
    let Some((r, km)) = near else {
        return Some(Hint::new("will", HintAction::Explore, 0,
            &[format!("Explore beyond {place} with {who}: type where it goes")], &["No rival stands: claim the world before the Final Night".into()], None));
    };
    let armies = cx.mine.iter().map(|x| fn7_society::armies(w).filter(|(_, a)| a.demigod == x.id && !a.rogue).count()).sum::<usize>();
    let region = cx.region(r.pos);
    let dist = cx.away(km);
    if armies == 0 {
        return Some(Hint::new("will", HintAction::TypeWill, 0,
            &[format!("Type a will: raise an army in {place} for {who}"), format!("Type a will: raise an army for {who}")],
            &[format!("You have no army and {} is {dist}", r.name), format!("No army; nearest rival {dist}")],
            Some(Target::thing(Ref::Demigod(d.id), d.pos))));
    }
    Some(Hint::new("will", HintAction::TypeWill, 0,
        &[format!("Type a will: march {who} on {} in {region}", r.name), format!("Type a will: march {who} on {}", r.name)],
        &[format!("{} is {dist} and you have {}", r.name, plural(armies as u32, "army").replace("armys", "armies")), format!("Nearest rival {dist}")],
        Some(Target::thing(Ref::Demigod(r.id), r.pos))))
}

#[cfg(test)]
#[path = "hint_tests.rs"]
mod tests;
