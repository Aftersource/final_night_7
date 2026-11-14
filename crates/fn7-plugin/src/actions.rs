//! Player input: `fields: {left, right, zoom}` (or `text` as `l: …` / `r: …` lines), validation, and the built-in AI's
//! cheap seeded lines (bots and the demigods no seat plays).

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::Action;

use crate::world::{district_of, visible};

/// Longest text accepted per hand per beat.
pub const MAX_HAND_CHARS: usize = 2000;

/// Most casts in one action.
pub const MAX_CASTS: usize = 5;
/// Longest custom-power description.
pub const MAX_CUSTOM_CHARS: usize = 200;

/// Where a cast is aimed: a ref string (`"dg:2"`), a point, or a name (a demigod, region, city, crowd...).
#[derive(Debug, Clone, PartialEq)]
pub enum CastTarget {
    Ref(Ref),
    At(LonLat),
    Name(String),
}

/// One god power cast from the UX (`fields.cast` / `casts`, or `bolt` / `bolts` = lightning).
#[derive(Debug, Clone, PartialEq)]
pub struct CastOrder {
    /// A palette slug (`data/powers.json`) or `"custom"`.
    pub power: String,
    pub target: CastTarget,
    /// A custom power's description (player words, untrusted).
    pub text: Option<String>,
    /// A custom power's magnitude (0..10; default 5).
    pub magnitude: Option<u8>,
    /// The carrying action's room-wide arrival order (`Action::seq`, set by the core; 0 for the plugin's own AI).
    pub seq: u64,
    /// Sent as an instant action (`action.submit { instant: true }`), the path clients use for casts.
    pub instant: bool,
    /// Rules 0.7.0: `"mega"` (a bolt, map scale) or `"mini"` (a spark, the board). Absent = mini in a schema-5 match.
    pub scale: Option<CastScale>,
}

/// Rules 0.8.0: a summoning (`fields.summon {demigod, target}`): move one of your demigods there this beat.
#[derive(Debug, Clone, PartialEq)]
pub struct SummonOrder {
    pub demigod: DemigodId,
    pub target: LonLat,
    /// The cell key asked for, if the target was a cell.
    pub cell: Option<String>,
    pub seq: u64,
}

/// Rules 0.8.0: a god-tier power (`fields.cast {power: "god_<slug>", tier: "god", target}`): that god's signature
/// power, resolved as its ruling.
#[derive(Debug, Clone, PartialEq)]
pub struct GodPowerOrder {
    /// The god's slug ("time").
    pub god: String,
    pub target: CastTarget,
    pub seq: u64,
}

/// Rules 0.8.1, challenge mode: `fields.challenge {demigod, target, level}`: one of your demigods locks a rival's in a
/// duel (an instant action, forced: no consent).
#[derive(Debug, Clone, PartialEq)]
pub struct ChallengeOrder {
    pub demigod: DemigodId,
    pub target: DemigodId,
    /// The board level it was issued at: its hex size is the reach (`crate::duel::LEVELS`).
    pub level: &'static str,
}

/// Rules 0.8.1: `fields.duel {duel_id, text}`: one typed line in a duel (resolved at once as a blow).
#[derive(Debug, Clone, PartialEq)]
pub struct BlowOrder {
    pub duel_id: String,
    pub text: String,
}

/// Longest duel line.
pub const MAX_DUEL_CHARS: usize = 120;

/// One seat's input for one beat. Two separate paths: text entry (orders to the hands: the God of Time interaction,
/// typed in a core action window opened by `action.begin`) and casts (UX god powers, sent as instant actions). Casts
/// never carry orders and never slow time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Input {
    pub left: String,
    pub right: String,
    /// A view change: level and optional focus (a ref string or a name).
    pub zoom: Option<(Zoom, Option<String>)>,
    /// God powers to cast this beat (in order).
    pub casts: Vec<CastOrder>,
    /// Rules 0.8.0: summonings (instant actions).
    pub summons: Vec<SummonOrder>,
    /// Rules 0.8.0: god-tier powers (instant actions).
    pub god_powers: Vec<GodPowerOrder>,
    /// Rules 0.8.1: a challenge (an instant action that travels alone).
    pub challenge: Option<ChallengeOrder>,
    /// Rules 0.8.1: a duel line (an instant action that travels alone).
    pub blow: Option<BlowOrder>,
    /// Rules 0.8.2: `fields.duel_tick {duel_id}`: a duellist's client says the panel's input window closed (an instant
    /// action that travels alone; it only moves the panel cycle on, see [`crate::panel`]).
    pub tick: Option<String>,
    /// `fields.duel_keys {duel_id, len, tail}`: a duellist's live typing (an instant action that travels alone; relayed
    /// as `duel.typing` and nothing else, see [`crate::typing`]).
    pub keys: Option<crate::typing::KeysOrder>,
}

impl Input {
    /// Does it carry orders (text entry)?
    pub fn has_orders(&self) -> bool {
        !self.left.trim().is_empty() || !self.right.trim().is_empty()
    }

    /// Does it carry a challenge or a duel line (rules 0.8.1)?
    pub fn is_duel(&self) -> bool {
        self.challenge.is_some() || self.blow.is_some() || self.tick.is_some() || self.keys.is_some()
    }
}

/// `fields.challenge {demigod: "dg:mine", target: "dg:N", level}` (rules 0.8.1). Refusal words are the client's.
fn challenge_of(fields: &Value) -> Result<Option<ChallengeOrder>, String> {
    let Some(v) = fields.get("challenge").filter(|v| !v.is_null()) else { return Ok(None) };
    let o = v.as_object().ok_or("bad_target")?;
    let dg = |k: &str| match o.get(k).and_then(Value::as_str).map(|s| s.trim().parse::<Ref>()) {
        Some(Ok(Ref::Demigod(d))) => Some(d),
        _ => None,
    };
    let demigod = dg("demigod").ok_or("not_yours")?;
    let target = dg("target").ok_or("bad_target")?;
    // A missing or unknown level counts as regions (the widest contact reach).
    let level = o.get("level").and_then(Value::as_str).and_then(crate::duel::level_of).unwrap_or("regions");
    Ok(Some(ChallengeOrder { demigod, target, level }))
}

/// `fields.duel {duel_id, text}` (rules 0.8.1).
fn blow_of(fields: &Value) -> Result<Option<BlowOrder>, String> {
    let Some(v) = fields.get("duel").filter(|v| !v.is_null()) else { return Ok(None) };
    let o = v.as_object().ok_or("no_duel")?;
    let duel_id = o.get("duel_id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or("no_duel")?;
    let text = o.get("text").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    if text.is_empty() {
        return Err("empty".into());
    }
    if text.chars().count() > MAX_DUEL_CHARS {
        return Err("too_long".into());
    }
    Ok(Some(BlowOrder { duel_id: duel_id.chars().take(64).collect(), text: text.to_string() }))
}

fn lonlat(v: &Value) -> Option<LonLat> {
    let a = v.as_array()?;
    if a.len() != 2 {
        return None;
    }
    let (lon, lat) = (a[0].as_f64()?, a[1].as_f64()?);
    ((-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat)).then_some([lon, lat])
}

fn cast_target(v: &Value) -> Result<CastTarget, String> {
    if let Some(p) = lonlat(v) {
        return Ok(CastTarget::At(p));
    }
    match v {
        Value::String(s) if !s.trim().is_empty() && s.chars().count() <= 200 => {
            let t = s.trim();
            if let Ok(r) = t.parse::<Ref>() {
                return Ok(CastTarget::Ref(r));
            }
            if let Some((a, b)) = t.split_once(',') {
                if let (Ok(lon), Ok(lat)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
                    if (-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat) {
                        return Ok(CastTarget::At([lon, lat]));
                    }
                }
            }
            Ok(CastTarget::Name(t.to_string()))
        }
        _ => Err("a cast's `target` must be a ref (\"dg:2\"), a name, or [lon, lat]".into()),
    }
}

/// One cast: `{power, target, text?, magnitude?}`, or (for `bolt`/`bolts`) `{target}` or the bare target.
fn cast_of(v: &Value, default_power: Option<&str>) -> Result<CastOrder, String> {
    let mut scale = None;
    let (power, target, text, magnitude) = match v {
        Value::Object(o) => {
            scale = match o.get("tier").or(o.get("scale")) {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if s.eq_ignore_ascii_case("mega") => Some(CastScale::Mega),
                Some(Value::String(s)) if s.eq_ignore_ascii_case("mini") => Some(CastScale::Mini),
                _ => return Err("a cast's `tier` is \"mega\" (a bolt) or \"mini\" (a spark)".into()),
            };
            let power = match (o.get("power"), default_power) {
                (Some(Value::String(p)), _) => p.trim().to_lowercase(),
                (None | Some(Value::Null), Some(d)) => d.to_string(),
                _ => return Err("a cast needs `power`: a palette slug or \"custom\"".into()),
            };
            let target = o.get("target").ok_or("a cast needs `target`")?;
            let text = match o.get("text") {
                None | Some(Value::Null) => None,
                Some(Value::String(t)) if t.chars().count() <= MAX_CUSTOM_CHARS => Some(t.trim().to_string()),
                _ => return Err(format!("`text` must be a string of at most {MAX_CUSTOM_CHARS} characters")),
            };
            let magnitude = match o.get("magnitude") {
                None | Some(Value::Null) => None,
                Some(x) => Some(
                    x.as_f64()
                        .filter(|m| (0.0..=10.0).contains(m))
                        .ok_or("`magnitude` must be a number from 0 to 10")? as u8,
                ),
            };
            (power, cast_target(target)?, text, magnitude)
        }
        other => match default_power {
            Some(d) => (d.to_string(), cast_target(other)?, None, None),
            None => return Err("a cast must be {power, target}".into()),
        },
    };
    let powers = crate::powers();
    if power != "custom" && powers.get(&power).is_none() {
        let known: Vec<&str> = powers.powers.iter().map(|p| p.slug.as_str()).collect();
        return Err(format!("unknown power `{power}` (one of {}, or custom)", known.join(", ")));
    }
    if power == "custom" && text.as_deref().is_none_or(str::is_empty) {
        return Err("a custom power needs `text` (what it is)".into());
    }
    let power = powers.get(&power).map(|p| p.slug.clone()).unwrap_or(power);
    Ok(CastOrder {
        power,
        target,
        text,
        magnitude,
        seq: 0,
        instant: false,
        scale,
    })
}

/// The gods with a god-tier power (rules 0.8.0), by slug.
pub const GOD_POWERS: &[&str] = &["time", "nature", "consequence", "death", "chance", "memory", "entropy"];

/// A god-tier power (`tier: "god"` or a `god_<slug>` power), if `v` is one.
fn god_power_of(v: &Value) -> Result<Option<GodPowerOrder>, String> {
    let Value::Object(o) = v else { return Ok(None) };
    let power = o.get("power").and_then(Value::as_str).map(|p| p.trim().to_lowercase()).unwrap_or_default();
    let god_tier = o.get("tier").or(o.get("scale")).and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case("god"));
    if !god_tier && !power.starts_with("god_") {
        return Ok(None);
    }
    let slug = power.strip_prefix("god_").unwrap_or(&power).to_string();
    if !GOD_POWERS.contains(&slug.as_str()) {
        return Err(format!("unknown god-tier power `{power}` (god_time, god_nature, god_consequence, god_death, god_chance, god_memory, god_entropy)"));
    }
    let target = cast_target(o.get("target").ok_or("a god-tier power needs `target`")?)?;
    Ok(Some(GodPowerOrder { god: slug, target, seq: 0 }))
}

fn casts_of(fields: &Value) -> Result<(Vec<CastOrder>, Vec<GodPowerOrder>), String> {
    let mut out = vec![];
    let mut gods = vec![];
    let mut one = |v: &Value, default: Option<&str>| -> Result<(), String> {
        match god_power_of(v)? {
            Some(g) => gods.push(g),
            None => out.push(cast_of(v, default)?),
        }
        Ok(())
    };
    for (k, default) in [("cast", None), ("bolt", Some("lightning"))] {
        match fields.get(k) {
            None | Some(Value::Null) => {}
            Some(v) => one(v, default)?,
        }
    }
    for (k, default) in [("casts", None), ("bolts", Some("lightning"))] {
        match fields.get(k) {
            None | Some(Value::Null) => {}
            Some(Value::Array(a)) => {
                for v in a {
                    one(v, default)?;
                }
            }
            Some(_) => return Err(format!("`{k}` must be an array")),
        }
    }
    if out.len() + gods.len() > MAX_CASTS {
        return Err(format!("at most {MAX_CASTS} casts per action"));
    }
    Ok((out, gods))
}

/// `fields.summon: {demigod: "dg:N", target: [lon, lat] | "<cell key>"}` (rules 0.8.0).
fn summon_of(fields: &Value) -> Result<Vec<SummonOrder>, String> {
    let Some(v) = fields.get("summon").filter(|v| !v.is_null()) else { return Ok(vec![]) };
    // Refusals use the client's reason words: not_yours (no such demigod of yours), bad_target.
    let o = v.as_object().ok_or("bad_target")?;
    let demigod = match o.get("demigod").and_then(Value::as_str).map(|s| s.trim().parse::<Ref>()) {
        Some(Ok(Ref::Demigod(d))) => d,
        _ => return Err("not_yours".into()),
    };
    let t = o.get("target").ok_or("bad_target")?;
    let (target, cell) = match (lonlat(t), t.as_str().and_then(|s| fn7_model::hexgrid::HexCell::parse(s.trim()))) {
        (Some(p), _) => (p, None),
        (None, Some(c)) => (c.center(), Some(c.key())),
        _ => return Err("bad_target".into()),
    };
    Ok(vec![SummonOrder { demigod, target, cell, seq: 0 }])
}

pub fn parse_zoom_level(s: &str) -> Option<Zoom> {
    match s.trim().to_lowercase().as_str() {
        "world" | "map" | "planet" | "globe" => Some(Zoom::World),
        "district" | "region" => Some(Zoom::District),
        "city" => Some(Zoom::City),
        "character" | "person" => Some(Zoom::Character),
        _ => None,
    }
}

fn zoom_of(v: &Value) -> Result<Option<(Zoom, Option<String>)>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) => parse_zoom_level(s)
            .map(|z| Some((z, None)))
            .ok_or_else(|| format!("unknown zoom level `{s}` (world, region, city, character)")),
        Value::Object(o) => {
            let level = o
                .get("level")
                .and_then(Value::as_str)
                .ok_or("`zoom.level` must be world, region, city or character")?;
            let z =
                parse_zoom_level(level).ok_or_else(|| format!("unknown zoom level `{level}`"))?;
            let focus = match o.get("focus") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if s.trim().is_empty() => None,
                Some(Value::String(s)) if s.chars().count() <= 200 => Some(s.trim().to_string()),
                _ => {
                    return Err(
                        "`zoom.focus` must be a short string (a ref like \"ch:12\" or a name)"
                            .into(),
                    )
                }
            };
            Ok(Some((z, focus)))
        }
        _ => Err("`zoom` must be a level string or {level, focus}".into()),
    }
}

/// Parse an action: `fields.left` / `fields.right` / `fields.zoom`; when neither hand field is present, `text` is read
/// as lines prefixed `l:` / `r:` (unprefixed lines go to the left hand).
pub fn parse(text: &str, fields: &Value) -> Result<Input, String> {
    let mut i = Input::default();
    let field = |k: &str| -> Result<Option<String>, String> {
        match fields.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(format!("`{k}` must be a string")),
        }
    };
    let (l, r) = (field("left")?, field("right")?);
    let fielded = l.is_some() || r.is_some();
    if fielded {
        i.left = l.unwrap_or_default();
        i.right = r.unwrap_or_default();
    } else {
        let (mut ls, mut rs) = (vec![], vec![]);
        for line in text.lines() {
            let t = line.trim();
            let lower = t.to_lowercase();
            if let Some(x) = lower
                .strip_prefix("l:")
                .or_else(|| lower.strip_prefix("left:"))
            {
                let n = t.len() - x.len();
                ls.push(t[n..].trim().to_string());
            } else if let Some(x) = lower
                .strip_prefix("r:")
                .or_else(|| lower.strip_prefix("right:"))
            {
                let n = t.len() - x.len();
                rs.push(t[n..].trim().to_string());
            } else if !t.is_empty() {
                ls.push(t.to_string());
            }
        }
        i.left = ls.join("\n");
        i.right = rs.join("\n");
    }
    i.zoom = zoom_of(fields.get("zoom").unwrap_or(&Value::Null))?;
    // Slow time v0 is gone (rules 0.5): time slows through the Theatre's action windows (`action.begin`), and the core
    // stamps arrival order (`seq`, `received_at_ms`). Old clients get a clear error instead of silently losing orders.
    if !fielded && text.lines().any(|x| x.trim().eq_ignore_ascii_case("/slow")) {
        return Err("`/slow` was removed: send `action.begin` when you start typing (it opens the slow window)".into());
    }
    for k in ["slow", "sent_at", "window"] {
        if fields.get(k).is_some_and(|v| !v.is_null()) {
            return Err(format!(
                "`fields.{k}` was removed: send `action.begin` to slow time and submit with the `window_id` from \
                 `window.opened`; the server stamps arrival order"
            ));
        }
    }
    let (casts, gods) = casts_of(fields)?;
    i.casts = casts;
    i.god_powers = gods;
    i.summons = summon_of(fields)?;
    i.challenge = challenge_of(fields)?;
    i.blow = blow_of(fields)?;
    i.tick = match fields.get("duel_tick").filter(|v| !v.is_null()) {
        Some(v) => Some(v.get("duel_id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or("no_duel")?
                        .chars().take(64).collect()),
        None => None,
    };
    i.keys = crate::typing::keys_of(fields)?;
    if i.is_duel()
        && (i.challenge.is_some() as u8 + i.blow.is_some() as u8 + i.tick.is_some() as u8 + i.keys.is_some() as u8 > 1
            || !i.casts.is_empty()
            || !i.summons.is_empty()
            || !i.god_powers.is_empty()
            || i.has_orders())
    {
        return Err("a challenge or a duel line travels alone in its instant action".into());
    }
    for (k, s) in [("left", &i.left), ("right", &i.right)] {
        if s.chars().count() > MAX_HAND_CHARS {
            return Err(format!("`{k}` is longer than {MAX_HAND_CHARS} characters"));
        }
    }
    Ok(i)
}

/// A focus string → a ref: a ref string that exists, else an exact (case-insensitive) name among `candidates`.
pub fn resolve_focus(w: &World, s: &str, candidates: &[Ref]) -> Option<Ref> {
    if let Ok(r) = s.parse::<Ref>() {
        return fn7_society::exists(w, r).then_some(r);
    }
    candidates
        .iter()
        .copied()
        .find(|r| w.name_of(*r).eq_ignore_ascii_case(s.trim()))
}

pub fn action(seat: u8, left: &str, right: &str, zoom: Option<&str>) -> Action {
    let mut fields = json!({ "left": left, "right": right });
    if let Some(z) = zoom {
        fields["zoom"] = z.into();
    }
    let mut text = String::new();
    if !left.is_empty() {
        text.push_str(&format!("l: {left}"));
    }
    if !right.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("r: {right}"));
    }
    Action {
        fields,
        ..Action::new(seat, text)
    }
}

fn short(label: &str) -> String {
    label.split(" of ").next().unwrap_or(label).to_lowercase()
}

/// The built-in AI's input for a seat this beat. One demigod (schema 4): [`bot_lines`] for it. Two (schema 5): each
/// standing demigod gets its own lines (its ordinary act, then any reach for a god, joined), the Left's in `left` and
/// the Right's in `right`; zoom and casts come from the lead demigod's lines (casts are the player's, paid from the
/// shared purse).
pub fn bot_input(w: &World, seat: u8, beat: u32) -> Input {
    let bodies: Vec<&Demigod> = w.bodies_of_seat(seat).collect();
    let Some(lead) = w.lead_of_seat(seat) else {
        return Input::default();
    };
    if bodies.len() < 2 {
        return bot_lines(w, lead, beat);
    }
    let mut out = Input::default();
    for d in bodies.iter().filter(|d| d.alive) {
        let i = bot_lines(w, d.id, beat);
        let text = [i.left.as_str(), i.right.as_str()]
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        match d.side {
            Some(Hand::Right) => out.right = text,
            _ => out.left = text,
        }
        if d.id == lead {
            out.zoom = i.zoom;
            out.casts = i.casts;
        }
    }
    out
}

/// The built-in AI's lines for demigod `d` this beat: seeded, cheap, no LLM. The left hand does ordinary acts; the
/// right hand sometimes reaches for a god, spawns a scout, possesses someone, zooms or recalls.
pub fn bot_lines(w: &World, d: DemigodId, beat: u32) -> Input {
    if crate::world::settings(w).society_only {
        return Input::default();
    }
    let me = &w.demigods[d.ix()];
    let mut rng = Rng::derive(w.meta.seed, &[hash_str("bot"), beat as u64, d.0 as u64]);
    let rivals: Vec<&Demigod> = w.demigods.iter().filter(|x| x.alive && !w.same_player(x.id, d)).collect();
    let rival = rng
        .pick(&rivals)
        .map(|x| x.name.clone())
        .unwrap_or_else(|| "the nearest stranger".into());
    let home = district_of(w, me.place);
    let seen = visible(w, d, Zoom::District, None, 64);
    let crowds: Vec<String> = seen
        .iter()
        .filter_map(|r| match r {
            Ref::Cluster(c) => Some(short(&w.clusters[c.ix()].label)),
            _ => None,
        })
        .collect();
    let people: Vec<String> = seen
        .iter()
        .filter_map(|r| match r {
            Ref::Char(c) if w.characters[c.ix()].serves.is_none() => {
                Some(w.characters[c.ix()].name.clone())
            }
            _ => None,
        })
        .collect();
    let crowd = rng
        .pick(&crowds)
        .cloned()
        .unwrap_or_else(|| "locals".into());
    // The command chain: its biggest army's general, a ringleader in sight, a rival general in reach.
    let surname = |c: CharId| w.characters[c.ix()].name.rsplit(' ').next().unwrap_or("").to_string();
    let general = crate::beat::own_army(w, d)
        .and_then(|a| w.clusters[a.ix()].army.as_ref().and_then(|x| x.general))
        .filter(|g| w.characters[g.ix()].alive)
        .map(surname);
    let ringleader = seen.iter().find_map(|r| match r {
        Ref::Char(c) if w.characters[c.ix()].alive && w.characters[c.ix()].life.as_ref().is_some_and(|l| l.role == Role::Ringleader) => {
            Some(w.characters[c.ix()].name.clone())
        }
        _ => None,
    });
    let rival_general = fn7_society::armies(w)
        .filter(|(_, a)| !w.same_player(a.demigod, d) && !a.rogue)
        .find_map(|(_, a)| a.general.filter(|g| w.characters[g.ix()].alive))
        .map(surname);
    let person = rng.pick(&people).cloned();
    let districts: Vec<&Place> = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::District)
        .collect();
    let center = districts
        .iter()
        .min_by(|a, b| {
            fn7_society::geo::distance_km(a.center, w.places[0].center)
                .total_cmp(&fn7_society::geo::distance_km(b.center, w.places[0].center))
        })
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let elsewhere = rng
        .pick(&districts)
        .filter(|p| p.id != home)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| center.clone());
    let arena_center = w.places[w.meta.arena.ix()].center;
    let edge = fn7_society::geo::distance_km(me.pos, arena_center) > w.meta.arena_radius_km * 0.8;
    let late = beat * 10 >= w.meta.beats_total * 6 || edge;
    // Rules 0.7.0: the built-in AI drifts toward the action (the nearest hotspot within reach), now and then, when the
    // circle isn't pressing. Drawn only in schema-5 worlds.
    let drift = if w.two_bodies() && !edge && !late && rng.below(4) == 0 {
        let spots = crate::hotspots::hotspots(w, crate::hotspots::recent_rows(w));
        let reach = if w.is_planet() { 4000.0 } else { w.meta.arena_radius_km };
        crate::hotspots::nearest(&spots, me.pos, reach)
            .filter(|h| fn7_society::geo::distance_km(h.pos, me.pos) > h.radius_km)
            .map(|h| w.name_of(Ref::Place(fn7_society::geo::place_at(w, h.pos))))
    } else {
        None
    };
    let left = match rng.below(10) {
        _ if edge => format!("go to {center}"),
        _ if drift.is_some() => format!("go to {}", drift.clone().expect("checked")),
        0 | 1 if late => format!("go to {center}"),
        0 => format!("go to {elsewhere}"),
        1 | 2 => format!("recruit the {crowd}"),
        3 => format!("go to {rival}"),
        4 => format!("preach to the {crowd} about my greatness"),
        5 => match &person {
            Some(p) => format!("talk to {p}"),
            None => "talk to the baker who saw the fire".into(),
        },
        6 => format!("threaten the {crowd}"),
        7 => "search for a weapon".into(),
        8 => "ask the old fisherman with the limp what he saw".into(),
        _ => match rivals.iter().find(|x| district_of(w, x.place) == home) {
            Some(r) => format!("attack {}", r.name),
            None => format!("go to {rival}"),
        },
    };
    let subs = crate::world::hands_of(me)
        .iter()
        .map(|h| crate::world::live_children(w, *h).len())
        .sum::<usize>();
    // The right hand reaches for a god about one beat in twelve (prices add up), spawns or possesses now and then.
    let right = match rng.below(100) {
        0 => "rewind that last bit".into(),
        1 => "slow time around me".into(),
        2 => "slow time and delay my debt".into(),
        3 => format!("summon a storm over {rival}"),
        4 => "make them forget what I did".into(),
        5 => "I feel lucky, roll the dice".into(),
        6 => "raise the dead".into(),
        7 => "absolve my debt".into(),
        8..=11 if subs < 2 => format!("send a scout to spy on {rival}"),
        12 | 13 => match &person {
            Some(p) => format!("possess {p}"),
            None => format!("rally the {crowd}"),
        },
        14 => "recall the scout".into(),
        15..=27 if general.is_some() => {
            let g = general.clone().expect("checked");
            match rng.below(6) {
                0 => format!("tell General {g} to march on {rival}"),
                1 => format!("General {g}, take {elsewhere}"),
                2 => format!("General {g}: hold where you are and dig in"),
                3 => format!("General {g}, deal with it, use your judgment"),
                4 => match &ringleader {
                    Some(r) => format!("promote {r} to captain"),
                    None => format!("General {g}, raid the cities that love {rival}"),
                },
                _ => match &rival_general {
                    Some(r) => format!("buy off General {r} and his captains"),
                    None => format!("tell General {g} to hunt down {rival}"),
                },
            }
        }
        15..=19 => format!("raise an army from the {crowd} and march on {rival}"),
        20..=34 => format!("rally the {crowd}"),
        _ => String::new(),
    };
    // Rules 0.7.0: a demigod with nothing else for its right hand manifests something now and then (the item engine
    // judges it). Drawn only in schema-5 worlds, so schema-4 recordings replay.
    let right = if w.two_bodies() && right.is_empty() && rng.below(8) == 0 {
        manifest_line(&mut rng, &elsewhere, me.bolts)
    } else {
        right
    };
    // Rules 0.8.0: now and then it raises a shrine to itself where it stands. Drawn only in schema-6 worlds.
    let right = if w.dm_rules() && right.is_empty() && rng.below(24) == 0 {
        ["raise a shrine to me here", "build a temple of light in my name", "erect a healing shrine to me",
         "carve a monument of war to my glory"][rng.below(4) as usize]
            .to_string()
    } else {
        right
    };

    let zoom = match rng.below(12) {
        0 => Some((Zoom::World, None)),
        1 => Some((Zoom::Character, person.clone())),
        2 => Some((Zoom::District, None)),
        3 => Some((Zoom::City, None)),
        _ => None,
    };
    // Text entry is the God of Time interaction (every message is billed by Time: a bolt and a debt), so the AI speaks
    // about one beat in six (always when the arena's edge is near). It can't open the core's action windows (only a
    // connected client's `action.begin` can), so its orders are instant Enters: a zero-second slow. It casts a power
    // about one beat in nine when it has the bolts, on beats it doesn't speak (casts never carry orders).
    // Rules 0.7.0: a player's two demigods each speak about one beat in twelve, so the player speaks (and pays Time)
    // about as often as one demigod did.
    let speak = edge || rng.below(if w.two_bodies() { 12 } else { 6 }) == 0;
    // The Final Wish (beat 1, rules 0.7.0): every built-in AI demigod makes a wish.
    let wish = crate::beat::is_final_wish(w, beat);
    let (left, right) = if wish {
        (left, manifest_line(&mut rng, &elsewhere, me.bolts))
    } else if speak {
        (left, right)
    } else {
        (String::new(), String::new())
    };
    // Rules 0.9.0: now and then the built-in AI makes a will of its own (heals when hurt, arms itself, pleads for its
    // fallen sibling, curses a rival, pursues its quest): its own seeded stream, drawn only when the rules are on.
    let will = if crate::wills::on(w) && !wish { crate::wills::ai_line(w, d, beat, &crate::world::settings(w)) } else { None };
    let speak = speak || will.is_some();
    let (left, right) = match will {
        Some(l) => (left, l),
        None => (left, right),
    };
    let mut casts = vec![];
    if w.two_bodies() {
        // Rules 0.7.0: minis often (a spark), megas sometimes (a bolt, and only with a bolt to spare for Time: the AI
        // hoards); never on a beat it speaks.
        let mega = me.bolts >= 2 && rng.below(14) == 0;
        let mini = !mega && me.sparks >= 1 && rng.below(5) == 0;
        if (mega || mini) && !speak {
            let pw = &crate::powers().powers;
            if let Some(p) = rng.pick(pw) {
                let target = match rng.pick(&rivals) {
                    Some(r) if mini || p.target == PowerTarget::Unit || rng.below(2) == 0 => CastTarget::Ref(Ref::Demigod(r.id)),
                    Some(r) => CastTarget::At(r.pos),
                    None => CastTarget::At(arena_center),
                };
                casts.push(CastOrder {
                    power: p.slug.clone(),
                    target,
                    text: None,
                    magnitude: None,
                    seq: 0,
                    instant: false,
                    scale: Some(if mega { CastScale::Mega } else { CastScale::Mini }),
                });
            }
        }
    } else if me.bolts >= 1 && rng.below(9) == 0 && !speak {
        let pw: Vec<&PowerDef> = crate::powers().powers.iter().filter(|p| p.cost <= me.bolts).collect();
        if let Some(p) = rng.pick(&pw) {
            let target = match rng.pick(&rivals) {
                Some(r) if p.target == PowerTarget::Unit || rng.below(2) == 0 => CastTarget::Ref(Ref::Demigod(r.id)),
                _ => {
                    let cities: Vec<&Place> = w.places.iter().filter(|p| p.kind == PlaceKind::Site).collect();
                    match rng.pick(&cities) {
                        Some(c) => CastTarget::At(c.center),
                        None => CastTarget::At(arena_center),
                    }
                }
            };
            casts.push(CastOrder {
                power: p.slug.clone(),
                target,
                text: None,
                magnitude: None,
                seq: 0,
                instant: false,
                scale: None,
            });
        }
    }
    Input {
        left,
        right,
        zoom,
        casts,
        ..Default::default()
    }
}

/// A built-in AI's manifestation order (rules 0.7.0): an army under banners at a seeded scale it can pay for (the
/// item engine's tier price, so the AI never wishes itself into a lethal debt), or a seeded object.
fn manifest_line(rng: &mut Rng, place: &str, bolts: u32) -> String {
    // (words, bolts the tier costs): relic, wonder, wonder, legend, myth, titan.
    const ALL: [(&str, u32); 6] =
        [("10,000", 1), ("250,000", 1), ("a million", 1), ("10,000,000", 2), ("100,000,000", 3), ("a billion", 4)];
    let scales: Vec<&str> = ALL.iter().filter(|(_, c)| *c <= bolts.max(1)).map(|x| x.0).collect();
    const SCALES_FALLBACK: [&str; 1] = ["10,000"];
    let scales: &[&str] = if scales.is_empty() { &SCALES_FALLBACK } else { &scales };
    const THINGS: [&str; 8] = [
        "forge a sword of stars",
        "conjure a crown that hears lies",
        "summon a dragon over the hills",
        "raise a fortress overnight",
        "manifest an armada of airships",
        "conjure a storm that follows me",
        "forge armor from a dead moon",
        "summon a relic of the first war",
    ];
    // The AI never borrows for it: "no debt" makes the item engine refuse what the purse can't pay.
    if rng.below(2) == 0 {
        let s = rng.pick(scales).copied().unwrap_or("10,000");
        format!("raise banners of {s} soldiers at {place}, no debt")
    } else {
        format!("{}, no debt", rng.pick(&THINGS).copied().unwrap_or("forge a sword of stars"))
    }
}

/// An `Action` carrying an [`Input`] in `fields` (what a well-behaved client sends).
pub fn action_of(seat: u8, i: &Input) -> Action {
    let zoom = i.zoom.as_ref().map(|(z, _)| match z {
        Zoom::World => "world",
        Zoom::District => "district",
        Zoom::City => "city",
        Zoom::Character => "character",
    });
    let mut a = action(seat, &i.left, &i.right, zoom);
    if !i.casts.is_empty() {
        a.fields["casts"] = i
            .casts
            .iter()
            .map(|c| {
                let target = match &c.target {
                    CastTarget::Ref(r) => json!(r),
                    CastTarget::At(p) => json!(p),
                    CastTarget::Name(n) => json!(n),
                };
                let mut v = json!({ "power": c.power, "target": target });
                if let Some(s) = c.scale {
                    v["tier"] = json!(s);
                }
                if let Some(t) = &c.text {
                    v["text"] = t.clone().into();
                }
                v
            })
            .collect();
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fields_and_text() {
        let i = parse(
            "",
            &json!({"left": "go", "right": "rewind", "zoom": "world"}),
        )
        .unwrap();
        assert_eq!(
            (i.left.as_str(), i.right.as_str(), i.zoom),
            ("go", "rewind", Some((Zoom::World, None)))
        );
        let i = parse("l: punch Vex\nr: slow time\nand run", &Value::Null).unwrap();
        assert_eq!(
            (i.left.as_str(), i.right.as_str()),
            ("punch Vex\nand run", "slow time")
        );
        let i = parse(
            "",
            &json!({"left": "x", "zoom": {"level": "character", "focus": "ch:3"}}),
        )
        .unwrap();
        assert_eq!(i.zoom, Some((Zoom::Character, Some("ch:3".into()))));
        assert!(parse("", &json!({"left": 3})).is_err());
        assert!(parse("", &json!({"zoom": "galaxy"})).is_err());
        assert!(parse("", &json!({"left": "a".repeat(MAX_HAND_CHARS + 1)})).is_err());
        // Slow time v0 inputs are refused with a pointer to action.begin.
        for f in [json!({"slow": true}), json!({"left": "x", "sent_at": 5}), json!({"window": 1})] {
            assert!(parse("", &f).unwrap_err().contains("action.begin"), "{f}");
        }
        assert!(parse("/slow", &Value::Null).is_err());
    }
}
