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
}

/// One seat's input for one beat. Two separate paths: text entry (orders to the hands: the God of Time interaction,
/// with `slow`, `sent_at` and `window`) and casts (UX god powers). Casts never carry orders and never slow time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Input {
    pub left: String,
    pub right: String,
    /// A view change: level and optional focus (a ref string or a name).
    pub zoom: Option<(Zoom, Option<String>)>,
    /// God powers to cast this beat (in order).
    pub casts: Vec<CastOrder>,
    /// The player started typing: slow time (open a window).
    pub slow: bool,
    /// Client timestamp of the submission (epoch ms), for the cut-off rule and time-to-Enter.
    pub sent_at: Option<i64>,
    /// The slow window (its opening beat) these orders were typed in.
    pub window: Option<u32>,
}

impl Input {
    /// Does it carry orders (text entry)?
    pub fn has_orders(&self) -> bool {
        !self.left.trim().is_empty() || !self.right.trim().is_empty()
    }
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
    let (power, target, text, magnitude) = match v {
        Value::Object(o) => {
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
    })
}

fn casts_of(fields: &Value) -> Result<Vec<CastOrder>, String> {
    let mut out = vec![];
    for (k, default) in [("cast", None), ("bolt", Some("lightning"))] {
        match fields.get(k) {
            None | Some(Value::Null) => {}
            Some(v) => out.push(cast_of(v, default)?),
        }
    }
    for (k, default) in [("casts", None), ("bolts", Some("lightning"))] {
        match fields.get(k) {
            None | Some(Value::Null) => {}
            Some(Value::Array(a)) => {
                for v in a {
                    out.push(cast_of(v, default)?);
                }
            }
            Some(_) => return Err(format!("`{k}` must be an array")),
        }
    }
    if out.len() > MAX_CASTS {
        return Err(format!("at most {MAX_CASTS} casts per action"));
    }
    Ok(out)
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
    // `/slow` on its own line in `text` asks to slow time (text-only clients); it is not an order.
    if !fielded {
        let mut slow = false;
        for part in [&mut i.left, &mut i.right] {
            let kept: Vec<&str> = part
                .lines()
                .filter(|x| {
                    let s = x.trim().eq_ignore_ascii_case("/slow");
                    slow |= s;
                    !s
                })
                .collect();
            *part = kept.join("\n");
        }
        i.slow = slow;
    }
    match fields.get("slow") {
        None | Some(Value::Null) => {}
        Some(Value::Bool(b)) => i.slow |= *b,
        Some(_) => return Err("`slow` must be true/false".into()),
    }
    i.sent_at = match fields.get("sent_at") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_f64()
                .filter(|x| x.is_finite() && *x >= 0.0)
                .ok_or("`sent_at` must be epoch milliseconds")? as i64,
        ),
    };
    i.window = match fields.get("window") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or("`window` must be the window's beat (an integer)")? as u32),
    };
    i.casts = casts_of(fields)?;
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
        seat,
        text,
        fields,
        defaulted: false,
    }
}

fn short(label: &str) -> String {
    label.split(" of ").next().unwrap_or(label).to_lowercase()
}

/// The built-in AI's lines for demigod `d` this beat: seeded, cheap, no LLM. The left hand does ordinary acts; the
/// right hand sometimes reaches for a god, spawns a scout, possesses someone, zooms or recalls.
pub fn bot_lines(w: &World, d: DemigodId, beat: u32) -> Input {
    if crate::world::settings(w).society_only {
        return Input::default();
    }
    let me = &w.demigods[d.ix()];
    let mut rng = Rng::derive(w.meta.seed, &[hash_str("bot"), beat as u64, d.0 as u64]);
    let rivals: Vec<&Demigod> = w.demigods.iter().filter(|x| x.alive && x.id != d).collect();
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
        .filter(|(_, a)| a.demigod != d && !a.rogue)
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
    let left = match rng.below(10) {
        _ if edge => format!("go to {center}"),
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
    let subs = me
        .hands
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

    let zoom = match rng.below(12) {
        0 => Some((Zoom::World, None)),
        1 => Some((Zoom::Character, person.clone())),
        2 => Some((Zoom::District, None)),
        3 => Some((Zoom::City, None)),
        _ => None,
    };
    // Text entry is the God of Time interaction (every message slows time and costs a bolt), so the AI speaks about
    // one beat in six (always when the arena's edge is near). Now and then it slows time first and presses Enter one
    // to three beats later. It casts a power about one beat in nine when it has the bolts.
    let mine = w.slow.as_ref().filter(|s| s.initiator == d);
    let (speak, slow) = match mine {
        Some(s) => (beat >= s.opened_beat + 1 + (s.opened_beat + d.0) % 3, false),
        None if w.slow.is_none() && rng.below(30) == 0 => (false, true),
        None => (edge || rng.below(6) == 0, false),
    };
    let (left, right) = if speak { (left, right) } else { (String::new(), String::new()) };
    let mut casts = vec![];
    if me.bolts >= 1 && rng.below(9) == 0 {
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
            });
        }
    }
    Input {
        left,
        right,
        zoom,
        casts,
        slow,
        sent_at: None,
        window: None,
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
    if i.slow {
        a.fields["slow"] = true.into();
    }
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
                if let Some(t) = &c.text {
                    v["text"] = t.clone().into();
                }
                v
            })
            .collect();
    }
    if let Some(t) = i.sent_at {
        a.fields["sent_at"] = t.into();
    }
    if let Some(wi) = i.window {
        a.fields["window"] = wi.into();
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
    }
}
