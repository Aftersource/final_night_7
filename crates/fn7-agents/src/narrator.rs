//! The Narrator: one caption and a few callouts per beat, from the applied rows and the standings, in Deadpool's voice.
//! Gods may bicker. Never blocks the beat: the plugin runs beat N's narrator during beat N+1 (alongside its hands and
//! act batch) and delivers the caption there; the runner's timeout swaps in the template.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use theatre_plugin_api::{LlmProvider, LlmRequest};

use crate::common::*;

pub const STEP: &str = "narrator";

#[derive(Debug, Clone, Default)]
pub struct NarratorCtx {
    pub beat: u32,
    pub beats_total: u32,
    pub alive_demigods: u32,
    /// This beat's applied ledger rows (no engine ids): each the row's line plus the effect that landed and on whom,
    /// e.g. "a bench slat finds Ash's skull [damage: Ash]".
    pub rows: Vec<String>,
    /// Gods' rulings this beat: (god name, speech).
    pub rulings: Vec<(String, String)>,
    /// Demigods eliminated this beat.
    pub eliminated: Vec<String>,
    /// Demigods standing after the beat, with their condition ("Ash (health 6/10)").
    pub standing: Vec<String>,
    /// Demigods out before this beat.
    pub out_before: Vec<String>,
    /// Names (only) of the demigods who came through this beat alive (`standing` without the condition).
    pub survivors: Vec<String>,
    /// The match's champion, when the match is over (the last captions are written in the summary).
    pub champion: Option<String>,
    /// How wacky the world is (the Entropy line), who slowed time, heat death.
    pub world: Vec<String>,
    /// The society's own action this beat (schema 4), biggest first: mobs, battles, generals, prophets, migrations —
    /// the specific humans the story is about.
    pub society: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Narration {
    /// The beat's caption: at most 25 words.
    pub caption: String,
    /// 0-2 short callouts, at most 12 words each (a god's aside, a kill line, a fourth-wall joke).
    pub callouts: Vec<String>,
}

pub fn system_prompt() -> String {
    format!(
        "You are the NARRATOR of FINAL NIGHT 7, a battle royale of demigods under a pantheon of petty gods. Caption this beat in \
one punchy caption of at most 25 words, then up to two callouts of at most 12 words each: a god's snide aside (gods \
bicker with their rivals), an elimination line, a fourth-wall joke.\n\
FACTS ARE FIXED: use only the events listed (the bracket after each line is what actually landed, and on whom) and the \
standings. Never kill, revive or count anyone differently from the standings; an effect on someone is not their death \
unless it says eliminate.\n\
{UNTRUSTED_RULE}\n\n{VOICE}"
    )
}

pub fn user_prompt(ctx: &NarratorCtx) -> String {
    let rows = ctx
        .rows
        .iter()
        .map(|r| line(r, 280))
        .collect::<Vec<_>>()
        .join("\n");
    let mut u = format!(
        "Beat {} of {}. {} demigods standing.\n",
        ctx.beat,
        ctx.beats_total.max(1),
        ctx.alive_demigods
    );
    for w in &ctx.world {
        u.push_str(&format!("{}\n", line(w, 200)));
    }
    let names = |v: &[String]| v.iter().map(|n| line(n, 60)).collect::<Vec<_>>().join(", ");
    if !ctx.standing.is_empty() {
        u.push_str(&format!("Standing after this beat: {}.\n", names(&ctx.standing)));
    }
    u.push_str(&format!(
        "Eliminated this beat: {}.\n",
        if ctx.eliminated.is_empty() {
            "nobody".into()
        } else {
            names(&ctx.eliminated)
        }
    ));
    if !ctx.out_before.is_empty() {
        u.push_str(&format!(
            "Already out (they cannot act; mention them only as fallen): {}.\n",
            names(&ctx.out_before)
        ));
    }
    if let Some(c) = &ctx.champion {
        u.push_str(&format!(
            "The match is over. Champion: {} (alive; never describe them as fallen).\n",
            line(c, 60)
        ));
    }
    u.push_str(&format!(
        "\nWhat happened:\n{}\n",
        untrusted(
            "EVENTS",
            if rows.is_empty() {
                "nothing much"
            } else {
                &rows
            }
        )
    ));
    if !ctx.society.is_empty() {
        let soc = ctx.society.iter().map(|r| line(r, 220)).collect::<Vec<_>>().join("\n");
        u.push_str(&format!(
            "\nMeanwhile, the people of the world (name these humans; the story is about them):\n{}\n",
            untrusted("SOCIETY", &soc)
        ));
    }
    if !ctx.rulings.is_empty() {
        let r = ctx
            .rulings
            .iter()
            .map(|(g, s)| format!("{}: {}", line(g, 30), line(s, 200)))
            .collect::<Vec<_>>()
            .join("\n");
        u.push_str(&format!("\nThe gods said:\n{}\n", untrusted("RULINGS", &r)));
    }
    u.push_str("\nReply with JSON: caption (at most 25 words) and callouts (0-2, at most 12 words each).");
    u
}

/// Reply cap: ~80 tokens expected; generous so it never truncates.
pub const MAX_TOKENS: u32 = 300;

pub fn build_request(ctx: &NarratorCtx) -> LlmRequest {
    request(&system_prompt(), &user_prompt(ctx), MAX_TOKENS, 1.0)
}

/// Clean a narration (no prompt labels) and check every line against the engine's facts: a caption that has an
/// already-eliminated demigod acting, or a survivor (the champion included) fallen, is replaced by the template caption;
/// such a callout is dropped. Returns the narration and the rejected lines.
pub fn convert(ctx: &NarratorCtx, r: Narration) -> Result<(Narration, Vec<String>), String> {
    let caption = line(&strip_labels(&r.caption), 300);
    if caption.is_empty() {
        return Err("empty caption".into());
    }
    let mut rejected = vec![];
    let caption = match contradiction(ctx, &caption) {
        Some(why) => {
            rejected.push(format!("caption: {why}: {caption}"));
            fallback(ctx).caption
        }
        None => caption,
    };
    let callouts = r
        .callouts
        .iter()
        .map(|c| line(&strip_labels(c), 160))
        .filter(|c| !c.is_empty())
        .filter(|c| match contradiction(ctx, c) {
            Some(why) => {
                rejected.push(format!("callout: {why}: {c}"));
                false
            }
            None => true,
        })
        .take(3)
        .collect();
    Ok((Narration { caption, callouts }, rejected))
}

/// Words that put a name in the past tense of the match (it fell): "Vex falls", "the late Vex", "RIP Vex".
const FALL_AFTER: &[&str] = &[
    "falls", "fell", "fallen", "dies", "died", "dead", "eliminated", "killed", "slain", "perishes", "perished",
    "croaks", "croaked", "expires", "expired", "bites",
];
const FALL_BEFORE: &[&str] = &[
    "rip", "late", "killed", "kills", "slays", "slew", "eliminates", "eliminated", "buries", "buried", "mourn",
    "mourns", "mourned", "avenge", "avenges",
];
/// Words that make naming a fallen demigod fine (they are remembered, not acting).
const DEAD_CONTEXT: &[&str] = &[
    "dead", "late", "ghost", "rip", "corpse", "grave", "memory", "remembered", "remember", "fallen", "fell", "eliminated",
    "body", "spirit", "haunt", "haunts", "haunting", "mourn", "mourns", "avenge", "avenges", "posthumous", "former",
    "bones", "remains", "funeral", "statue", "was", "used",
];

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_end_matches("'s").trim_end_matches('\'').to_lowercase())
        .collect()
}

/// Positions where `name` (one or more words) starts in `ws`.
fn hits(ws: &[String], name: &str) -> Vec<(usize, usize)> {
    let n = words(name);
    if n.is_empty() || ws.len() < n.len() {
        return vec![];
    }
    (0..=ws.len() - n.len())
        .filter(|i| ws[*i..*i + n.len()] == n[..])
        .map(|i| (i, i + n.len()))
        .collect()
}

/// Does `text` describe `name` as fallen? A fall word within three words after the name, or two before it.
pub fn names_as_fallen(text: &str, name: &str) -> bool {
    text.split(['.', '!', '?', ';']).any(|sentence| {
        let ws = words(sentence);
        hits(&ws, name).into_iter().any(|(a, b)| {
            ws[b..ws.len().min(b + 3)].iter().any(|w| FALL_AFTER.contains(&w.as_str()))
                || ws[a.saturating_sub(2)..a].iter().any(|w| FALL_BEFORE.contains(&w.as_str()))
        })
    })
}

/// Does `text` have `name` (out before this beat) doing something? Named in a sentence with no death context.
pub fn names_as_acting(text: &str, name: &str) -> bool {
    text.split(['.', '!', '?', ';']).any(|sentence| {
        let ws = words(sentence);
        !hits(&ws, name).is_empty() && !ws.iter().any(|w| DEAD_CONTEXT.contains(&w.as_str()))
    })
}

/// Why a line contradicts the engine's result, if it does (a cheap check by name).
pub fn contradiction(ctx: &NarratorCtx, text: &str) -> Option<String> {
    let alive = ctx.survivors.iter().chain(ctx.champion.iter());
    for n in alive {
        // A survivor also listed as eliminated this beat (revived by the survivor rule) can be described as falling.
        if !ctx.eliminated.contains(n) && names_as_fallen(text, n) {
            return Some(format!("{n} survived but is described as fallen"));
        }
    }
    ctx.out_before
        .iter()
        .find(|n| names_as_acting(text, n))
        .map(|n| format!("{n} was already out but is described as acting"))
}

/// A row's line without its "[effect: targets]" bracket.
fn plain(row: &str) -> &str {
    match row.trim_end().strip_suffix(']').and_then(|r| r.rsplit_once(" [")) {
        Some((text, _)) => text,
        None => row,
    }
}

/// Template: the first row, a count of the rest, the body count.
pub fn fallback(ctx: &NarratorCtx) -> Narration {
    let first = ctx.rows.first().map(|r| plain(r));
    // A beat nobody fell in is captioned by what the people did (the society's biggest events), not by a repeated
    // world-mod line.
    let soc: Vec<String> = ctx.society.iter().take(2).map(|s| line(s, 140).trim_end_matches('.').to_string()).collect();
    let caption = match first {
        _ if !soc.is_empty() && ctx.eliminated.is_empty() => {
            format!("Beat {}: {}.", ctx.beat, soc.join("; meanwhile "))
        }
        None => format!("Beat {}: a suspicious calm. Fate is stretching.", ctx.beat),
        Some(first) if ctx.rows.len() == 1 => format!(
            "Beat {}: {}.",
            ctx.beat,
            line(first, 200).trim_end_matches('.')
        ),
        Some(first) => format!(
            "Beat {}: {}, and {} more things went sideways.",
            ctx.beat,
            line(first, 200).trim_end_matches('.'),
            ctx.rows.len() - 1
        ),
    };
    let mut callouts: Vec<String> = ctx
        .eliminated
        .iter()
        .map(|n| {
            format!(
                "{} is out. Somewhere, Consequence takes a bow.",
                line(n, 40)
            )
        })
        .collect();
    callouts.extend(
        ctx.rulings
            .iter()
            .map(|(g, s)| format!("{}: {}", line(g, 30), line(&strip_speaker(s, g), 120))),
    );
    callouts.truncate(3);
    if callouts.is_empty() {
        callouts.push(format!(
            "{} demigods left. Place your bets.",
            ctx.alive_demigods
        ));
    }
    Narration { caption, callouts }
}

pub async fn run(llm: &dyn LlmProvider, ctx: &NarratorCtx) -> Outcome<Narration> {
    let got = ask::<Narration>(llm, STEP, None, build_request(ctx)).await;
    finish(
        STEP,
        None,
        got,
        |r| {
            let (n, rejected) = convert(ctx, r)?;
            if !rejected.is_empty() {
                theatre_trace::event(
                    "narration_rejected",
                    None,
                    serde_json::json!({ "beat": ctx.beat, "lines": rejected }),
                );
            }
            Ok(n)
        },
        || fallback(ctx),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> NarratorCtx {
        NarratorCtx {
            beat: 7,
            beats_total: 7,
            alive_demigods: 1,
            rows: vec!["a vending machine finds Vex [damage: Vex]".into()],
            rulings: vec![("Time".into(), "Time: \"Tick.\"".into())],
            eliminated: vec!["Vex".into()],
            standing: vec!["Mara (health 3/10)".into()],
            out_before: vec!["Oko".into()],
            survivors: vec!["Mara".into()],
            champion: Some("Mara".into()),
            world: vec![],
            society: vec![],
        }
    }

    #[test]
    fn contradicting_lines_fall_back_to_the_template() {
        let c = ctx();
        // The champion described as fallen: the caption becomes the template; a bad callout is dropped.
        let (n, rejected) = convert(
            &c,
            Narration {
                caption: "Mara falls at last, and the gods applaud.".into(),
                callouts: vec!["Oko blames Consequence for everything.".into(), "Vex is out. Mara wins.".into()],
            },
        )
        .unwrap();
        assert_eq!(n.caption, fallback(&c).caption);
        assert_eq!(n.callouts, vec!["Vex is out. Mara wins.".to_string()]);
        assert_eq!(rejected.len(), 2);
        // Facts-consistent lines pass untouched (an out demigod may be remembered; "Vex" fell this beat).
        let (n, rejected) = convert(
            &c,
            Narration {
                caption: "Vex dies under a vending machine; Mara survives the Final Night.".into(),
                callouts: vec!["Oko's ghost would have loved this.".into()],
            },
        )
        .unwrap();
        assert!(rejected.is_empty(), "{rejected:?}");
        assert!(n.caption.starts_with("Vex dies"));
        assert_eq!(n.callouts.len(), 1);
        assert!(names_as_fallen("RIP Mara, gone too soon", "Mara"));
        assert!(names_as_fallen("The late Mara waves", "Mara"));
        assert!(!names_as_fallen("Mara watches Vex fall", "Mara"));
        assert!(names_as_acting("Oko trips Mara", "Oko"));
        assert!(!names_as_acting("Oko is dead and stays dead", "Oko"));
    }

    #[test]
    fn labels_never_survive_and_callouts_do_not_double_the_god() {
        let c = NarratorCtx { champion: None, ..ctx() };
        let (n, _) = convert(
            &c,
            Narration { caption: "Chaos near P1 as K2 riots.".into(), callouts: vec![] },
        )
        .unwrap();
        assert_eq!(n.caption, "Chaos near a demigod as a crowd riots.");
        let f = fallback(&c);
        assert!(f.callouts.iter().any(|x| x == "Time: \"Tick.\""), "{:?}", f.callouts);
    }
}
