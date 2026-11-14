//! What `fn7-agents` players say and how they say it: protocol envelopes, the seeded phrase bank for scripted fast
//! commands (with the words-per-minute dial), and a raw-HTTP Claude client (same request shape as Final Night's
//! `anthropic.rs`) whose calls are recorded through `theatre-trace`.

use std::time::Duration;

use serde_json::{json, Value};
use theatre_protocol::{t, Envelope};
use theatre_trace as trace;

// ------------------------------------------------------------------------------------------------------------------
// Protocol envelopes
// ------------------------------------------------------------------------------------------------------------------

/// Server limit on `action.submit` text (PROTOCOL.md: actions ≤ 500 chars).
pub const MAX_ACTION_CHARS: usize = 500;

/// One client request frame, `{ "t": ..., "d": {...} }` (no `id`: we don't need acks).
pub fn msg(ty: &str, d: Value) -> String {
    Envelope::request(ty, None, d).encode()
}

pub fn login(name: &str) -> String {
    msg(
        t::AUTH_LOGIN,
        json!({ "display_name": name, "client": { "kind": "fn7-agents", "version": env!("CARGO_PKG_VERSION") } }),
    )
}

pub fn resume(token: &str) -> String {
    msg(t::AUTH_RESUME, json!({ "resume_token": token }))
}

pub fn ping() -> String {
    msg(t::SYS_PING, json!({}))
}

pub fn room_create(settings: Value) -> String {
    msg(
        t::ROOM_CREATE,
        json!({ "visibility": "private", "settings": settings }),
    )
}

pub fn room_join(code: &str) -> String {
    msg(t::ROOM_JOIN, json!({ "code": code }))
}

pub fn room_ready() -> String {
    msg(t::ROOM_READY, json!({ "ready": true }))
}

pub fn room_start() -> String {
    msg(t::ROOM_START, json!({}))
}

/// Both hands in one submission: `{ round, text, fields: { left, right, zoom, cast?, sent_at? } }`. `text` is the
/// `l:` / `r:` form a human would type, so a server that ignores `fields` still gets both hands. Text entry and casts
/// are separate paths: `cast` (a god power) rides along but never carries orders.
pub fn action_submit(round: u32, s: &Submission) -> String {
    let mut fields = json!({ "left": s.left, "right": s.right, "zoom": s.zoom });
    if let Some(c) = &s.cast {
        fields["cast"] = c.clone();
    }
    if let Some(t) = s.sent_at {
        fields["sent_at"] = t.into();
    }
    msg(
        t::ACTION_SUBMIT,
        json!({ "round": round, "text": s.text(), "fields": fields }),
    )
}

/// Room settings for a FN7 match: one round per beat, Compose lasts the beat.
pub fn match_settings(rounds: u32, beat_ms: u64, seed: Option<u64>) -> Value {
    let mut s = json!({ "rounds": rounds, "beats_total": rounds, "beat_ms": beat_ms, "compose_seconds": beat_ms as f64 / 1000.0 });
    if let Some(seed) = seed {
        s["seed"] = seed.into();
    }
    s
}

// ------------------------------------------------------------------------------------------------------------------
// Submissions and the speed dial
// ------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    pub left: String,
    pub right: String,
    /// "world" | "district" | "city" | "character".
    pub zoom: String,
    /// A god power to cast (`{power, target}`), or none.
    pub cast: Option<Value>,
    /// The client's clock (epoch ms) at sending (the cut-off rule's order).
    pub sent_at: Option<i64>,
}

impl Submission {
    pub fn text(&self) -> String {
        let t = format!("l: {}\nr: {}", self.left, self.right);
        t.chars().take(MAX_ACTION_CHARS).collect()
    }
    pub fn words(&self) -> usize {
        word_count(&self.left) + word_count(&self.right)
    }
}

pub fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Words a player gets out in one beat at `wpm` words per minute (at least one).
pub fn words_per_beat(wpm: f64, beat_ms: u64) -> usize {
    ((wpm.max(0.0) * beat_ms as f64 / 60_000.0).round() as usize).max(1)
}

/// Split a beat's words over the two hands (left gets the odd one).
pub fn split_hands(words: usize) -> (usize, usize) {
    (words - words / 2, words / 2)
}

/// Longest a single hand's line may be so both fit in one action (`"l: " + left + "\nr: " + right`).
pub const MAX_HAND_CHARS: usize = (MAX_ACTION_CHARS - 7) / 2;

/// Keep the first `words` words of `s`, then trim whole words until it fits in `MAX_HAND_CHARS`.
pub fn clip_words(s: &str, words: usize) -> String {
    let mut w: Vec<&str> = s.split_whitespace().take(words).collect();
    while !w.is_empty() && w.join(" ").chars().count() > MAX_HAND_CHARS {
        w.pop();
    }
    w.join(" ")
}

// ------------------------------------------------------------------------------------------------------------------
// The phrase bank
// ------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// An ordinary act (the world resolver).
    Ordinary,
    /// A god invocation; the slug of the god whose keyword it carries.
    God(&'static str),
    /// "send someone to…" (spawn a subagent).
    Spawn,
    /// "take over …" (possession).
    Possess,
    /// "zoom into …", with the zoom level it implies.
    Zoom(&'static str),
    /// Recall a subagent or end a possession.
    Recall,
}

/// Every command a scripted player can say. Each god's lines use that god's keywords from `data/pantheon.json`; the
/// ordinary lines carry none (tests check both against `fn7_pantheon::route_by_keyword`).
pub const BANK: &[(Cue, &str)] = &[
    (Cue::Ordinary, "walk to the market and buy bread"),
    (Cue::Ordinary, "punch the rival in the jaw"),
    (Cue::Ordinary, "shout at the dockworkers to strike tonight"),
    (Cue::Ordinary, "build a barricade across the bridge"),
    (Cue::Ordinary, "steal the bishop's lantern"),
    (Cue::Ordinary, "hide in the cathedral crypt"),
    (Cue::Ordinary, "bribe the night watch with silver"),
    (Cue::Ordinary, "set fire to the warehouse on the quay"),
    (
        Cue::Ordinary,
        "tell the fishmongers the rival poisoned the well",
    ),
    (Cue::Ordinary, "teleport to the lighthouse"),
    (Cue::God("time"), "rewind the last minute on the bridge"),
    (Cue::God("time"), "slow time and walk through the crossfire"),
    (Cue::God("time"), "freeze time while I grab the relic"),
    (Cue::God("nature"), "summon a storm over the docks"),
    (Cue::God("nature"), "call lightning down on the rival"),
    (Cue::God("nature"), "make vines swallow the barricade"),
    (Cue::God("consequence"), "make the rival pay his debt"),
    (
        Cue::God("consequence"),
        "absolve the fishmongers of what they owe",
    ),
    (Cue::God("death"), "raise the drowned sailors from the dead"),
    (Cue::God("death"), "resurrect the baker"),
    (Cue::God("death"), "curse to death the captain of the watch"),
    (Cue::God("chance"), "roll the dice on the cathedral heist"),
    (
        Cue::God("chance"),
        "gamble everything on a coin toss with the rival",
    ),
    (Cue::God("memory"), "make them forget my face"),
    (Cue::God("memory"), "implant a false memory in the bishop"),
    (Cue::God("entropy"), "unleash infinite power on the whole region"),
    (Cue::God("entropy"), "let it all burn, maximum power"),
    (Cue::God("time"), "calm the chaos and cool the world"),
    (Cue::Spawn, "send someone to scout the cathedral"),
    (
        Cue::Spawn,
        "send someone to turn the dockworkers against the rival",
    ),
    (Cue::Spawn, "send someone to follow the bishop home"),
    (Cue::Possess, "take over the baker"),
    (
        Cue::Possess,
        "take over the captain of the watch and march him to the docks",
    ),
    (Cue::Zoom("district"), "zoom into the docks"),
    (Cue::Zoom("character"), "zoom into the baker"),
    (Cue::Zoom("world"), "zoom out to the whole city"),
    (Cue::Recall, "recall the scout"),
    (Cue::Recall, "release the baker"),
];

/// SplitMix64, the same generator `fn7_model::seed` uses (local copy: this crate doesn't need the model).
#[derive(Debug, Clone)]
pub struct SplitMix(u64);

impl SplitMix {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

/// A scripted player's mouth: walks shuffled passes over the whole bank (so every cue comes up within one pass) and
/// cuts each beat's speech to the words-per-beat budget.
#[derive(Debug, Clone)]
pub struct Script {
    rng: SplitMix,
    order: Vec<usize>,
    pos: usize,
    zoom: &'static str,
}

impl Script {
    pub fn new(seed: u64) -> Self {
        let mut s = Self {
            rng: SplitMix::new(seed),
            order: Vec::new(),
            pos: 0,
            zoom: "world",
        };
        s.reshuffle();
        s
    }

    fn reshuffle(&mut self) {
        self.order = (0..BANK.len()).collect();
        for i in (1..self.order.len()).rev() {
            let j = self.rng.below(i + 1);
            self.order.swap(i, j);
        }
        self.pos = 0;
    }

    pub fn next_phrase(&mut self) -> (Cue, &'static str) {
        if self.pos >= self.order.len() {
            self.reshuffle();
        }
        let (cue, text) = BANK[self.order[self.pos]];
        self.pos += 1;
        if let Cue::Zoom(z) = cue {
            self.zoom = z;
        }
        (cue, text)
    }

    /// Exactly `words` words of commands (fewer only if the line would overflow the action limit).
    pub fn hand_line(&mut self, words: usize) -> String {
        let mut out: Vec<&str> = Vec::new();
        while out.len() < words {
            let (_, p) = self.next_phrase();
            out.extend(p.split_whitespace());
        }
        clip_words(&out.join(" "), words)
    }

    /// One beat for both hands.
    pub fn beat(&mut self, words_per_beat: usize) -> Submission {
        let (l, r) = split_hands(words_per_beat);
        let left = self.hand_line(l);
        let right = self.hand_line(r);
        // About one beat in three, a god power at one of the world's big cities.
        const SPOTS: [(&str, [f64; 2]); 6] = [
            ("lightning", [3.38, 6.52]),
            ("storm_clouds", [139.69, 35.69]),
            ("flood", [-74.0, 40.71]),
            ("earthquake", [77.21, 28.61]),
            ("wildfire", [-46.63, -23.55]),
            ("grow_forest", [2.35, 48.86]),
        ];
        let cast = (self.rng.below(3) == 0).then(|| {
            let (power, at) = SPOTS[self.rng.below(SPOTS.len())];
            json!({ "power": power, "target": at })
        });
        Submission {
            left,
            right,
            zoom: self.zoom.to_string(),
            cast,
            sent_at: None,
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Claude (Anthropic Messages API over raw HTTP), recorded through theatre-trace
// ------------------------------------------------------------------------------------------------------------------

pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";

pub const PLAYER_SYSTEM: &str = "You are a human player in FINAL NIGHT 7, a battle royale of four demigods across the whole planet. \
You never act directly: you bark fast commands at your two hands (LEFT and RIGHT), orchestrator agents that act, spawn \
subagents ('send someone to ...'), possess people ('take over the baker'), zoom ('zoom into the docks') and recall helpers. \
Reaching into a god's domain (Time: rewind/slow time; Nature: storms, beasts; Consequence: debts, karma; Death: raise the \
dead; Chance: dice, luck; Memory: forget, false memories; Entropy: unleash chaos, infinite power) wakes that god, who names \
a price in lightning bolts. Your hands can raise armies from crowds and march them across the globe. Everyone you address is \
remembered forever, and every act lands in a ledger of consequence. Play to be the last demigod standing. Write short, \
punchy spoken commands, no engine ids, no narration. Reply with JSON only.";

pub struct Claude {
    client: reqwest::Client,
    api_key: String,
    url: String,
    pub model: String,
    effort: String,
    no_thinking: bool,
    max_attempts: u32,
}

impl Claude {
    pub fn new(api_key: String, timeout: Duration) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder().timeout(timeout).build()?,
            api_key,
            url: std::env::var("ANTHROPIC_MESSAGES_URL").unwrap_or_else(|_| API_URL.to_string()),
            model: std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string()),
            effort: std::env::var("ANTHROPIC_EFFORT").unwrap_or_else(|_| "low".into()),
            no_thinking: std::env::var("ANTHROPIC_THINKING")
                .map(|v| v != "on")
                .unwrap_or(true),
            max_attempts: 4,
        })
    }

    /// `ANTHROPIC_API_KEY` from the environment (call `dotenvy::dotenv()` first to pick up `.env`).
    pub fn from_env(timeout: Duration) -> Option<anyhow::Result<Self>> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())?;
        Some(Self::new(key, timeout))
    }

    pub fn hands_schema() -> Value {
        json!({
            "type": "object",
            "properties": { "left": { "type": "string" }, "right": { "type": "string" } },
            "required": ["left", "right"],
            "additionalProperties": false
        })
    }

    /// The Messages API body: structured output for `{left, right}`, effort, thinking off, no temperature.
    pub fn body(&self, system: &str, user: &str, max_tokens: u32) -> Value {
        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens.max(256),
            "system": system,
            "messages": [{ "role": "user", "content": user }],
            "output_config": { "effort": self.effort, "format": { "type": "json_schema", "schema": Self::hands_schema() } },
        });
        if self.no_thinking {
            body["thinking"] = json!({ "type": "disabled" });
        }
        body
    }

    /// Ask for one beat of both hands. Recorded as one `llm_calls` row (step `agent_hands`) with an `http_exchanges` row
    /// per attempt.
    pub async fn hands(
        &self,
        actor: &str,
        round: u32,
        user: String,
        words: (usize, usize),
    ) -> anyhow::Result<(String, String)> {
        let body = self.body(PLAYER_SYSTEM, &user, 400);
        let started_at = trace::now_ms();
        let t0 = std::time::Instant::now();
        let ctx = trace::current().map(|mut c| {
            c.llm_call_id = Some(c.sink.next_id());
            c.round = Some(round);
            c.step = Some("agent_hands".into());
            c.actor = Some(actor.to_string());
            c
        });
        let out = match &ctx {
            Some(c) => trace::within(c.clone(), self.call(&body)).await,
            None => self.call(&body).await,
        };
        if let Some(c) = &ctx {
            let request = json!({ "model": self.model, "system": PLAYER_SYSTEM, "messages": [["user", user]], "json_schema": Self::hands_schema(), "max_tokens": 400, "n": 1 });
            c.sink.write(trace::Record::LlmCall {
                id: c.llm_call_id.unwrap_or_default(),
                run: c.run.clone(),
                hook_id: None,
                round: Some(round),
                step: c.step.clone(),
                actor: c.actor.clone(),
                started_at,
                duration_ms: t0.elapsed().as_secs_f64() * 1000.0,
                request,
                choices: json!(out.as_ref().map(|s| vec![s.clone()]).unwrap_or_default()),
                cached: Some(false),
                error: out.as_ref().err().map(|e| format!("{e:#}")),
            });
        }
        let text = out?;
        let v: Value = serde_json::from_str(text.trim())
            .map_err(|e| anyhow::anyhow!("hands reply is not JSON ({e}): {text}"))?;
        let left = clip_words(v["left"].as_str().unwrap_or(""), words.0);
        let right = clip_words(v["right"].as_str().unwrap_or(""), words.1);
        Ok((left, right))
    }

    async fn call(&self, body: &Value) -> anyhow::Result<String> {
        let mut attempt = 0;
        loop {
            let started_at = trace::now_ms();
            let t0 = std::time::Instant::now();
            let mut rec = trace::HttpRecord {
                attempt,
                started_at,
                url: self.url.clone(),
                request: body.clone(),
                ..Default::default()
            };
            let mut retry_after = None;
            let out = self.exchange(body, &mut rec, &mut retry_after).await;
            rec.duration_ms = t0.elapsed().as_secs_f64() * 1000.0;
            if let Err((e, _)) = &out {
                rec.error = Some(format!("{e:#}"));
            }
            trace::http(rec);
            match out {
                Ok(t) => return Ok(t),
                Err((_, true)) if attempt + 1 < self.max_attempts => {
                    let wait = retry_after
                        .unwrap_or(0.5 * 2f64.powi(attempt as i32))
                        .clamp(0.2, 30.0);
                    tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                    attempt += 1;
                }
                Err((e, _)) => return Err(e),
            }
        }
    }

    /// One HTTP exchange. Errors carry whether they are worth retrying.
    async fn exchange(
        &self,
        body: &Value,
        rec: &mut trace::HttpRecord,
        retry_after: &mut Option<f64>,
    ) -> Result<String, (anyhow::Error, bool)> {
        let resp = self
            .client
            .post(&self.url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| (e.into(), true))?;
        let status = resp.status();
        rec.status = Some(status.as_u16());
        rec.request_headers = json!({ "anthropic-version": API_VERSION, "content-type": "application/json", "x-api-key": "<redacted>" });
        rec.response_headers = Value::Object(
            resp.headers()
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_string(),
                        Value::String(v.to_str().unwrap_or("<binary>").to_string()),
                    )
                })
                .collect(),
        );
        *retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok());
        let raw = resp.text().await.map_err(|e| (e.into(), true))?;
        let v: Value = serde_json::from_str(&raw).unwrap_or_else(|_| Value::String(raw.clone()));
        rec.response = v.clone();
        rec.input_tokens = v["usage"]["input_tokens"].as_u64();
        rec.output_tokens = v["usage"]["output_tokens"].as_u64();
        rec.stop_reason = v["stop_reason"].as_str().map(str::to_string);
        if !status.is_success() {
            let retry =
                status.as_u16() == 429 || status.as_u16() == 529 || status.is_server_error();
            return Err((
                anyhow::anyhow!(
                    "anthropic {status}: {}",
                    v["error"]["message"].as_str().unwrap_or("")
                ),
                retry,
            ));
        }
        if v["stop_reason"] == "refusal" {
            return Err((anyhow::anyhow!("model declined (refusal)"), false));
        }
        let text: String = v["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
        if text.trim().is_empty() {
            return Err((
                anyhow::anyhow!("empty response (stop_reason {})", v["stop_reason"]),
                true,
            ));
        }
        Ok(text)
    }
}

/// Cap a JSON payload's size for a prompt.
fn compact(v: &Value, max_chars: usize) -> String {
    let s = v.to_string();
    if s.chars().count() <= max_chars {
        s
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

/// The user prompt for one beat, built from what the server told this seat.
pub fn hands_prompt(
    name: &str,
    persona: &str,
    round: u32,
    words: (usize, usize),
    brief: &Value,
    private: &Value,
    private_state: &Value,
) -> String {
    let gods: Vec<String> = fn7_pantheon::load()
        .gods
        .iter()
        .map(|g| format!("{} ({})", g.name, g.domain))
        .collect();
    format!(
        "You are {name}, {persona}.\nBeat {round}.\nGods: {}.\n\nThis beat's brief (JSON): {}\n\nWhat only you saw last beat (JSON): {}\n\nYour seat's view of the game (JSON): {}\n\n\
         Write what you shout at each hand this beat: about {} words for LEFT and {} words for RIGHT. Reply as JSON {{\"left\": \"...\", \"right\": \"...\"}}.",
        gods.join("; "),
        compact(brief, 4000),
        compact(private, 3000),
        compact(private_state, 3000),
        words.0,
        words.1
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(raw: &str) -> Envelope {
        Envelope::decode(raw).expect("valid envelope")
    }

    #[test]
    fn envelopes_match_the_protocol() {
        let e = decode(&login("Rook"));
        assert_eq!(e.t, "auth.login");
        assert_eq!(e.d["display_name"], "Rook");
        assert_eq!(e.d["client"]["kind"], "fn7-agents");
        assert!(e.id.is_none() && e.re.is_none());

        assert_eq!(decode(&resume("tok")).d, json!({ "resume_token": "tok" }));
        let p = decode(&ping());
        assert_eq!((p.t.as_str(), p.d.clone()), ("sys.ping", json!({})));
        assert_eq!(decode(&room_join("XK4P2Q")).d, json!({ "code": "XK4P2Q" }));
        assert_eq!(decode(&room_ready()).d, json!({ "ready": true }));
        assert_eq!(decode(&room_start()).t, "room.start");
        let c = decode(&room_create(match_settings(12, 6000, Some(7))));
        assert_eq!(c.t, "room.create");
        assert_eq!(c.d["visibility"], "private");
        assert_eq!(c.d["settings"]["compose_seconds"], 6.0);
        assert_eq!(c.d["settings"]["beats_total"], 12);
        assert_eq!(c.d["settings"]["seed"], 7);

        let s = Submission {
            left: "summon a storm over the docks".into(),
            right: "send someone to scout the cathedral".into(),
            zoom: "district".into(),
            cast: None,
            sent_at: None,
        };
        let a = decode(&action_submit(3, &s));
        assert_eq!(a.t, "action.submit");
        assert_eq!(a.d["round"], 3);
        assert_eq!(
            a.d["text"],
            "l: summon a storm over the docks\nr: send someone to scout the cathedral"
        );
        assert_eq!(
            a.d["fields"],
            json!({ "left": s.left, "right": s.right, "zoom": "district" })
        );
        // The raw frame is exactly the envelope: t + d, nothing else.
        let raw: Value = serde_json::from_str(&action_submit(3, &s)).unwrap();
        assert_eq!(
            raw.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["d", "t"]
        );
    }

    #[test]
    fn bank_covers_every_god_and_every_cue() {
        let p = fn7_pantheon::load();
        for g in &p.gods {
            let lines: Vec<&str> = BANK
                .iter()
                .filter(|(c, _)| matches!(c, Cue::God(s) if *s == g.slug))
                .map(|(_, t)| *t)
                .collect();
            assert!(!lines.is_empty(), "no lines for god {}", g.slug);
            for l in lines {
                assert_eq!(
                    fn7_pantheon::route_by_keyword(&p, l),
                    p.id_of(&g.slug),
                    "{l:?} should route to {}",
                    g.slug
                );
            }
        }
        // Every god line names a god that exists.
        for (c, l) in BANK {
            if let Cue::God(slug) = c {
                assert!(p.id_of(slug).is_some(), "{l:?} names unknown god {slug}");
            }
        }
        // Non-divine lines must not wake a god by accident.
        for (c, l) in BANK.iter().filter(|(c, _)| !matches!(c, Cue::God(_))) {
            assert_eq!(
                fn7_pantheon::route_by_keyword(&p, l),
                None,
                "{c:?} line {l:?} trips a god keyword"
            );
        }
        let has = |pred: &dyn Fn(&Cue, &str) -> bool| BANK.iter().any(|(c, t)| pred(c, t));
        assert!(has(
            &|c, t| *c == Cue::Spawn && t.starts_with("send someone to")
        ));
        assert!(has(
            &|c, t| *c == Cue::Possess && t.contains("take over the baker")
        ));
        assert!(has(
            &|c, t| *c == Cue::Zoom("district") && t.contains("zoom into the docks")
        ));
        assert!(has(&|c, _| *c == Cue::Zoom("character")));
        assert!(has(&|c, _| *c == Cue::Zoom("world")));
        assert!(has(&|c, t| *c == Cue::Recall && t.contains("recall")));
        assert!(has(&|c, _| *c == Cue::Ordinary));
    }

    #[test]
    fn script_reaches_every_cue_in_one_pass_and_is_seeded() {
        let mut s = Script::new(42);
        let pass: Vec<&str> = (0..BANK.len()).map(|_| s.next_phrase().1).collect();
        let mut sorted = pass.clone();
        sorted.sort_unstable();
        let mut all: Vec<&str> = BANK.iter().map(|(_, t)| *t).collect();
        all.sort_unstable();
        assert_eq!(sorted, all, "one pass says every phrase once");
        let again: Vec<&str> = {
            let mut s = Script::new(42);
            (0..BANK.len()).map(|_| s.next_phrase().1).collect()
        };
        assert_eq!(pass, again, "same seed, same speech");
        let other: Vec<&str> = {
            let mut s = Script::new(43);
            (0..BANK.len()).map(|_| s.next_phrase().1).collect()
        };
        assert_ne!(pass, other);
    }

    #[test]
    fn wpm_scales_words_per_beat() {
        assert_eq!(words_per_beat(60.0, 6000), 6);
        assert_eq!(words_per_beat(120.0, 6000), 12);
        assert_eq!(words_per_beat(240.0, 6000), 24);
        assert_eq!(words_per_beat(120.0, 3000), 6);
        assert_eq!(words_per_beat(0.0, 6000), 1, "always at least one word");
        assert_eq!(split_hands(13), (7, 6));
        let mut s = Script::new(1);
        let mut last = 0;
        for wpm in [30.0, 60.0, 120.0, 240.0, 400.0] {
            let n = words_per_beat(wpm, 6000);
            let b = s.beat(n);
            assert_eq!(b.words(), n, "at {wpm} wpm a beat carries {n} words: {b:?}");
            assert!(b.words() > last);
            last = b.words();
            assert!(b.text().chars().count() <= MAX_ACTION_CHARS);
        }
        // Absurd speeds are clipped to the action limit, never past it.
        let b = s.beat(words_per_beat(5000.0, 6000));
        assert!(b.text().chars().count() <= MAX_ACTION_CHARS);
        assert!(
            b.left.chars().count() <= MAX_HAND_CHARS && b.right.chars().count() <= MAX_HAND_CHARS
        );
    }

    #[test]
    fn zoom_field_follows_the_last_zoom_cue() {
        let mut s = Script::new(9);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..40 {
            let b = s.beat(30);
            assert!(["world", "district", "character"].contains(&b.zoom.as_str()));
            seen.insert(b.zoom);
        }
        assert!(seen.len() >= 2);
    }

    #[test]
    fn claude_body_shape() {
        let c = Claude::new("k".into(), Duration::from_secs(5)).unwrap();
        let v = c.body("sys", "hi", 100);
        assert_eq!(v["model"], c.model);
        assert_eq!(v["system"], "sys");
        assert_eq!(v["max_tokens"], 256);
        assert_eq!(v["messages"][0]["role"], "user");
        assert!(v.get("temperature").is_none());
        assert_eq!(v["output_config"]["format"]["type"], "json_schema");
        assert_eq!(
            v["output_config"]["format"]["schema"]["additionalProperties"],
            false
        );
        let p = hands_prompt(
            "Rook",
            "a hot-dropper",
            2,
            (6, 6),
            &json!({ "a": 1 }),
            &Value::Null,
            &json!({}),
        );
        assert!(p.contains("Beat 2") && p.contains("6 words for LEFT"));
    }
}
