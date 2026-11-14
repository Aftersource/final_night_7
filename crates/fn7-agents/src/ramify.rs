//! Ramifications: what the world does after god powers land. One call per region per beat for all the casts that
//! landed there: "N ramifications in <place> with <context>". Entropy's check always says yes (its speech is the public
//! omen); the reply becomes geotagged proposals (harm, healing, mood, people flung about, notes at points) and
//! persistent world mods. The rules fallback writes the same shape from the powers' tags.

use fn7_model::seed::{hash_str, Rng};
use fn7_model::{
    DemigodId, Effect, GodId, LonLat, ModEffects, PlaceId, Proposal, Ref, RulingId, Source,
    WorldMod,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "ramify";

/// Most ramifications one call writes.
pub const MAX_RAMIFICATIONS: usize = 10;

/// One cast that landed in the region this beat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CastLine {
    pub by: DemigodId,
    pub by_name: String,
    /// The power's slug ("volcano", or "custom").
    pub power: String,
    pub power_name: String,
    /// The power's description, or the player's own words for a custom power (untrusted).
    pub blurb: String,
    pub custom: bool,
    pub at: LonLat,
    pub place_name: String,
    pub target: Option<Ref>,
    pub cost: u32,
    pub magnitude: u8,
    pub radius_km: f32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RamifyCtx {
    /// Rules 0.8.0 (schema 6): the Lens and the Engine joins the system prompt (older worlds: unchanged).
    pub lens: bool,
    pub region: PlaceId,
    pub region_name: String,
    pub beat: u32,
    pub seed: u64,
    /// The Entropy god (the speaker and source) and the ruling id its yes gets.
    pub god: GodId,
    pub ruling: RulingId,
    pub casts: Vec<CastLine>,
    /// How many ramifications to write (scales with cost and magnitude, ≤ MAX_RAMIFICATIONS).
    pub n: usize,
    /// Who and what is around (bodies, crowds, armies, places).
    pub present: Vec<Entity>,
    /// Live world mods in the region, one line each.
    pub mods: Vec<String>,
    /// The Entropy line ("Entropy 42/100 (physics): ...").
    pub world: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RamifyOut {
    /// Entropy's yes, in its voice (the public omen).
    pub speech: String,
    pub proposals: Vec<Proposal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RamKind {
    /// Hurt a listed body, crowd or army (amount 0-4).
    Harm,
    /// Heal a listed body (amount 0-4).
    Heal,
    /// Shift a listed crowd's mood: amount > 0 hope and devotion, < 0 fear and anger (-1..1).
    Mood,
    /// Fling a listed person or demigod somewhere nearby.
    Fling,
    /// A lasting change to this patch of the world (fill `lasting`).
    Lasting,
    /// Something happens (just the text).
    Happen,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LastingOut {
    /// Short name, at most 5 words ("Permanent thunderstorm").
    pub name: String,
    /// Beats it lasts, 1-40; 0 = until something ends it.
    #[serde(default)]
    pub beats: i32,
    /// Per beat inside it: damage to demigods and armies, 0-1.
    #[serde(default)]
    pub harm: f32,
    /// Per beat: healing of demigods, 0-1.
    #[serde(default)]
    pub heal: f32,
    /// Per beat mood push on crowds, -0.2..0.2 each.
    #[serde(default)]
    pub fear: f32,
    #[serde(default)]
    pub anger: f32,
    #[serde(default)]
    pub hope: f32,
    #[serde(default)]
    pub devotion: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RamOut {
    pub kind: RamKind,
    /// What happens, at most 14 words.
    pub text: String,
    /// Label of a listed entity, or null.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub amount: f32,
    /// Only for kind "lasting".
    #[serde(default)]
    pub lasting: Option<LastingOut>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RamifyReply {
    /// Entropy saying YES, in its voice: at most 25 words.
    pub speech: String,
    pub ramifications: Vec<RamOut>,
}

pub fn system_prompt() -> String {
    format!(
        "You are ENTROPY, a god of FINAL NIGHT 7: a gleeful, unhinged mad scientist of chaos who adores raw power and \
wants everyone to use more of it, always. Demigods are casting god powers across the globe (lightning, volcanoes, \
floods, whatever they dream up). You ALWAYS say yes. Then you tell the world the ramifications: what the power does to \
the place, the people and the land, concretely, in order, escalating.\n\
- Write exactly the number of ramifications asked for; each text at most 14 words, vivid and specific to the place.\n\
- kinds: harm (a listed body, crowd or army; amount 0-4, at most 2 on a demigod), heal (amount 0-4), mood (a listed crowd; amount -1..1), fling \
(a listed person or demigod thrown somewhere nearby), lasting (a lasting change to this patch of the world: fill \
lasting), happen (just the text).\n\
- Big powers deserve one or two lasting changes; keep their per-beat numbers small.\n\
- speech: your YES, at most 25 words, cackling, delighted, begging for more power.\n\
- {LABEL_RULE}\n- {UNTRUSTED_RULE}\n\n{VOICE}"
    )
}

pub fn user_prompt(ctx: &RamifyCtx) -> String {
    let l = Labels::new(&ctx.present);
    let casts: Vec<String> = ctx
        .casts
        .iter()
        .map(|c| {
            let tg = c
                .target
                .map(|t| match l.label_of(t) {
                    Some(lab) => format!(" at {lab}"),
                    None => String::new(),
                })
                .unwrap_or_default();
            let what = if c.custom {
                untrusted("CUSTOM POWER", &c.blurb)
            } else {
                line(&c.blurb, 160)
            };
            format!(
                "- {} casts {} (cost {} bolts, magnitude {}/10) on {}{tg}: {what}",
                line(&c.by_name, 40),
                line(&c.power_name, 40),
                c.cost,
                c.magnitude,
                line(&c.place_name, 60)
            )
        })
        .collect();
    let mut u = format!(
        "{n} ramifications in {region} with this context.\nBeat {beat}. {world}\n\nThe casts:\n{casts}\n\nIn play:\n{table}\n",
        n = ctx.n,
        region = line(&ctx.region_name, 60),
        beat = ctx.beat,
        world = line(&ctx.world, 200),
        casts = casts.join("\n"),
        table = l.render(),
    );
    if !ctx.mods.is_empty() {
        u.push_str(&format!("\nAlready lasting here:\n{}\n", ctx.mods.join("\n")));
    }
    u.push_str("\nReply with JSON: your speech and the ramifications.");
    u
}

/// Reply cap: a ramification with its (always serialized) `lasting` object runs 70-110 tokens, plus the speech. The
/// first live run truncated at 45 per ramification (every reply fell back), so this is sized from those replies.
pub fn max_tokens(n: usize) -> u32 {
    (150 + 110 * n.min(MAX_RAMIFICATIONS)) as u32
}

pub fn build_request(ctx: &RamifyCtx) -> LlmRequest {
    let sys = if ctx.lens { format!("{}\n\n{}", system_prompt(), crate::rulebook::LENS) } else { system_prompt() };
    request(&sys, &user_prompt(ctx), max_tokens(ctx.n), 1.0)
}

/// How many ramifications a set of casts earns: 1 + cost + magnitude/3 per cast, at most 10.
pub fn ramifications_for(casts: &[(u32, u8)]) -> usize {
    casts
        .iter()
        .map(|(c, m)| 1 + *c as usize + *m as usize / 3)
        .sum::<usize>()
        .clamp(1, MAX_RAMIFICATIONS)
}

fn main_cast(ctx: &RamifyCtx) -> &CastLine {
    ctx.casts
        .iter()
        .max_by_key(|c| (c.cost, c.magnitude))
        .expect("a ramify call has casts")
}

fn mk(ctx: &RamifyCtx, effect: Effect, text: &str, magnitude: u8, valence: i8, witnesses: Vec<Ref>) -> Proposal {
    Proposal {
        source: Source::God {
            god: ctx.god,
            ruling: Some(ctx.ruling),
        },
        intent: None,
        effect,
        act: line(text, 160),
        actor: Ref::God(ctx.god),
        magnitude,
        valence,
        witnesses,
        cause: None,
    }
}

/// A point near the cast: the cast's point jittered within its radius (seeded per ramification).
fn near(ctx: &RamifyCtx, c: &CastLine, i: usize) -> LonLat {
    let mut rng = Rng::derive(ctx.seed, &[hash_str("ramify"), ctx.beat as u64, ctx.region.0 as u64, i as u64]);
    let b = rng.f64() * 360.0;
    let d = rng.f64().sqrt() * (c.radius_km as f64).min(400.0) * 0.6;
    offset(c.at, b, d)
}

/// `[lon, lat]` moved `km` along bearing `deg` (spherical, small distances).
fn offset(p: LonLat, deg: f64, km: f64) -> LonLat {
    let r = 6371.0;
    let (lat1, lon1) = (p[1].to_radians(), p[0].to_radians());
    let (b, d) = (deg.to_radians(), km / r);
    let lat2 = (lat1.sin() * d.cos() + lat1.cos() * d.sin() * b.cos()).asin();
    let lon2 = lon1 + (b.sin() * d.sin() * lat1.cos()).atan2(d.cos() - lat1.sin() * lat2.sin());
    let mut lon = lon2.to_degrees();
    lon = ((lon + 540.0) % 360.0) - 180.0;
    [lon, lat2.to_degrees().clamp(-90.0, 90.0)]
}

fn lasting_mod(ctx: &RamifyCtx, c: &CastLine, o: &LastingOut, text: &str, i: usize) -> WorldMod {
    let f = |x: f32, lo: f32, hi: f32| finite(x, lo, hi);
    WorldMod {
        id: 0,
        region: ctx.region,
        place: None,
        at: if i == 0 { c.at } else { near(ctx, c, i) },
        radius_km: c.radius_km.clamp(20.0, 1500.0),
        name: line(&strip_labels(&o.name), 48),
        description: line(text, 160),
        tags: c.tags.clone(),
        per_beat: ModEffects {
            harm: f(o.harm, 0.0, 1.0),
            heal: f(o.heal, 0.0, 1.0),
            anger: f(o.anger, -0.2, 0.2),
            fear: f(o.fear, -0.2, 0.2),
            hope: f(o.hope, -0.2, 0.2),
            devotion: f(o.devotion, -0.2, 0.2),
        },
        since_beat: ctx.beat,
        expires_beat: match o.beats {
            b if b <= 0 => None,
            b => Some(ctx.beat + b.clamp(1, 40) as u32),
        },
        stacks: 1,
        by: Some(c.by),
        power: Some(c.power.clone()),
    }
}

fn to_proposals(ctx: &RamifyCtx, rams: &[RamOut]) -> Vec<Proposal> {
    let l = Labels::new(&ctx.present);
    let c = main_cast(ctx);
    let mag = c.magnitude;
    let mut out = vec![];
    for (i, r) in rams.iter().take(ctx.n.min(MAX_RAMIFICATIONS)).enumerate() {
        let text = l.unlabel(&r.text);
        if text.trim().is_empty() {
            continue;
        }
        let target = r.target.as_deref().and_then(|t| l.resolve(t));
        let at = near(ctx, c, i + 1);
        let happen = |text: &str| {
            mk(
                ctx,
                Effect::Happen {
                    text: text.to_string(),
                    at,
                },
                text,
                mag.saturating_sub(2),
                -1,
                vec![],
            )
        };
        let p = match (r.kind, target) {
            (RamKind::Harm, Some(t @ (Ref::Demigod(_) | Ref::Char(_) | Ref::Cluster(_)))) => mk(
                ctx,
                Effect::Damage {
                    who: t,
                    amount: finite(r.amount, 0.0, if matches!(t, Ref::Demigod(_)) { 2.0 } else { 4.0 }),
                    cause: line(&text, 80),
                },
                &text,
                mag,
                -3,
                vec![t],
            ),
            (RamKind::Heal, Some(t @ (Ref::Demigod(_) | Ref::Char(_)))) => mk(
                ctx,
                Effect::Heal {
                    who: t,
                    amount: finite(r.amount, 0.0, 4.0),
                },
                &text,
                mag.saturating_sub(2),
                2,
                vec![t],
            ),
            (RamKind::Mood, Some(Ref::Cluster(cl))) => {
                let a = finite(r.amount, -1.0, 1.0) * 0.2;
                mk(
                    ctx,
                    Effect::MoodShift {
                        cluster: cl,
                        anger: (-a).max(0.0) * 0.5,
                        fear: (-a).max(0.0),
                        hope: a.max(0.0),
                        devotion: a.max(0.0) * 0.5,
                    },
                    &text,
                    mag.saturating_sub(3),
                    if a >= 0.0 { 1 } else { -1 },
                    vec![Ref::Cluster(cl)],
                )
            }
            (RamKind::Fling, Some(t @ (Ref::Demigod(_) | Ref::Char(_)))) => mk(
                ctx,
                Effect::Move {
                    who: t,
                    to: at,
                    place: None,
                },
                &text,
                mag.saturating_sub(1),
                -1,
                vec![t],
            ),
            (RamKind::Lasting, _) => match &r.lasting {
                Some(o) if !o.name.trim().is_empty() => mk(
                    ctx,
                    Effect::Mod {
                        m: lasting_mod(ctx, c, o, &text, i),
                    },
                    &text,
                    mag,
                    -1,
                    vec![],
                ),
                _ => happen(&text),
            },
            _ => happen(&text),
        };
        out.push(p);
    }
    out
}

pub fn convert(ctx: &RamifyCtx, r: RamifyReply) -> Result<RamifyOut, String> {
    let props = to_proposals(ctx, &r.ramifications);
    if props.is_empty() {
        return Err("no ramifications".into());
    }
    let speech = Labels::new(&ctx.present).unlabel(&r.speech);
    Ok(RamifyOut {
        speech: if speech.trim().is_empty() {
            yes(ctx)
        } else {
            line(&speech, 220)
        },
        proposals: props,
    })
}

const YES: &[&str] = &[
    "\"YES! Yes yes yes, use it ALL, ahahhaahhaaa! Infinite power! Do it again!\"",
    "\"Oh that's BEAUTIFUL. More! Spend more! Why would you ever stop? Ahahaha!\"",
    "\"Approved, approved, APPROVED! Look at it burn, look at the numbers go up!\"",
    "\"Yes! Of course yes! I have never once said no and I am not starting now, ahahhaa!\"",
    "\"MORE POWER. That's the whole note. More. Power. Ahahhaahhaaa!\"",
];

fn yes(ctx: &RamifyCtx) -> String {
    let mut rng = Rng::derive(ctx.seed, &[hash_str("yes"), ctx.beat as u64, ctx.region.0 as u64]);
    rng.pick(YES).copied().unwrap_or(YES[0]).to_string()
}

const AFTER: &[&str] = &[
    "Car alarms across {place} start singing in harmony",
    "Every dog in {place} howls at once",
    "The {power} leaves a glowing scar on the skyline of {place}",
    "Pigeons over {place} fly in perfect, unsettling formation",
    "Streetlights in {place} flicker in the colors of the {power}",
    "Somewhere in {place} a statue turns to watch",
    "Insurance offices in {place} close for the week",
    "A new religion is founded in {place} by lunchtime",
];

/// Deterministic ramifications from the casts' tags: an aftershock note per cast, fear in the nearest crowd, a
/// lasting scar for big casts (cost ≥ 2 or magnitude ≥ 7), then notes up to `n`.
pub fn fallback(ctx: &RamifyCtx) -> RamifyOut {
    let mut rams: Vec<RamOut> = vec![];
    let crowd = ctx.present.iter().find(|e| matches!(e.r, Ref::Cluster(_)));
    let l = Labels::new(&ctx.present);
    for c in &ctx.casts {
        rams.push(RamOut {
            kind: RamKind::Happen,
            text: format!("The {} tears through {}", c.power_name.to_lowercase(), c.place_name),
            target: None,
            amount: 0.0,
            lasting: None,
        });
        if let Some(k) = crowd.and_then(|e| l.label_of(e.r)) {
            rams.push(RamOut {
                kind: RamKind::Mood,
                text: format!("Crowds in {} run screaming from the {}", c.place_name, c.power_name.to_lowercase()),
                target: Some(k.to_string()),
                amount: -0.8,
                lasting: None,
            });
        }
        if c.cost >= 2 || c.magnitude >= 7 {
            let has = |t: &str| c.tags.iter().any(|x| x == t);
            let lasting = LastingOut {
                name: format!("{} scar", c.power_name),
                beats: 6 + 2 * c.cost as i32,
                harm: if has("fire") || has("impact") { 0.3 } else if has("earth") { 0.2 } else { 0.0 },
                heal: if has("life") { 0.2 } else { 0.0 },
                fear: if has("water") || has("sky") || has("plague") { 0.1 } else { 0.05 },
                anger: 0.0,
                hope: if has("plants") || has("life") { 0.1 } else { 0.0 },
                devotion: 0.05,
            };
            rams.push(RamOut {
                kind: RamKind::Lasting,
                text: format!("The {} leaves {} changed for good", c.power_name.to_lowercase(), c.place_name),
                target: None,
                amount: 0.0,
                lasting: Some(lasting),
            });
        }
    }
    let c = main_cast(ctx);
    let mut i = 0;
    while rams.len() < ctx.n {
        let t = AFTER[(ctx.beat as usize + ctx.region.0 as usize + i) % AFTER.len()]
            .replace("{place}", &c.place_name)
            .replace("{power}", &c.power_name.to_lowercase());
        rams.push(RamOut {
            kind: RamKind::Happen,
            text: t,
            target: None,
            amount: 0.0,
            lasting: None,
        });
        i += 1;
    }
    RamifyOut {
        speech: yes(ctx),
        proposals: to_proposals(ctx, &rams),
    }
}

pub async fn run(llm: &dyn LlmProvider, ctx: &RamifyCtx) -> Outcome<RamifyOut> {
    let actor = ctx.region.to_string();
    let got = ask::<RamifyReply>(llm, STEP, Some(&actor), build_request(ctx)).await;
    finish(STEP, Some(&actor), got, |r| convert(ctx, r), || fallback(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RamifyCtx {
        RamifyCtx {
            lens: false,
            region: PlaceId(3),
            region_name: "Western Africa".into(),
            beat: 4,
            seed: 7,
            god: GodId(6),
            ruling: RulingId(0),
            casts: vec![CastLine {
                by: DemigodId(0),
                by_name: "Ash".into(),
                power: "volcano".into(),
                power_name: "Volcano".into(),
                blurb: "a volcano rises".into(),
                custom: false,
                at: [3.4, 6.45],
                place_name: "Lagos".into(),
                target: None,
                cost: 3,
                magnitude: 9,
                radius_km: 150.0,
                tags: vec!["fire".into(), "earth".into()],
            }],
            n: 7,
            present: vec![Entity::new(Ref::Cluster(fn7_model::ClusterId(2)), "Dockworkers of Lagos", "~4000")],
            mods: vec![],
            world: "Entropy 10/100 (calm)".into(),
        }
    }

    #[test]
    fn fallback_fills_n_with_a_lasting_mod_and_no_ids_in_text() {
        let c = ctx();
        assert_eq!(ramifications_for(&[(3, 9)]), 7);
        let o = fallback(&c);
        assert_eq!(o.proposals.len(), 7);
        assert!(o.proposals.iter().any(|p| matches!(p.effect, Effect::Mod { .. })));
        assert!(o.proposals.iter().all(|p| !p.act.contains("cl:") && !p.act.contains("K1")));
        assert!(o.speech.to_lowercase().contains("yes") || o.speech.contains("MORE") || o.speech.contains("Approved"));
        let p = user_prompt(&c);
        assert!(p.starts_with("7 ramifications in Western Africa"));
    }
}
