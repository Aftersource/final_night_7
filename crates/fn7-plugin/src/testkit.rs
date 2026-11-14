//! Offline providers for tests and the simulator: never touch the network.
//! - [`Scripted`]: a valid reply for every mind, picked by the system prompt (a counter varies spawns and dispatches).
//! - [`Failing`]: every call errors (every mind falls back).
//! - [`Slow`]: answers like `Scripted`, after a delay (to exercise the per-call timeout).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use theatre_plugin_api::{LlmProvider, LlmRequest, LlmResponse};

/// The reply a scripted provider gives to `req` (call number `n`), or `None` for an unknown prompt.
pub fn scripted_reply(req: &LlmRequest, n: u64) -> Option<Value> {
    let s = &req.system;
    Some(if s.contains("HAND of a demigod") {
        let mut intents = vec![
            json!({ "verb": "recruit", "targets": ["K1"], "how": "with a speech", "magnitude": 4, "route": "world", "arg": "" }),
            json!({ "verb": "punch", "targets": ["P2"], "how": "hard", "magnitude": 5, "route": "world", "arg": "" }),
        ];
        match n % 7 {
            0 => intents.push(json!({ "verb": "slow", "targets": [], "how": "delay my debt", "magnitude": 5, "route": "god", "arg": "time" })),
            1 => intents.push(json!({ "verb": "scout", "targets": ["P2"], "how": "", "magnitude": 3, "route": "spawn", "arg": "spy on the rival for 3 beats" })),
            2 => intents.push(json!({ "verb": "storm", "targets": ["P2"], "how": "", "magnitude": 6, "route": "god", "arg": "nature" })),
            3 => intents.push(json!({ "verb": "possess", "targets": [], "how": "", "magnitude": 3, "route": "possess", "arg": "C1" })),
            4 => intents.push(json!({ "verb": "look", "targets": [], "how": "", "magnitude": 0, "route": "zoom", "arg": "world" })),
            5 => intents.push(json!({ "verb": "forget", "targets": ["P2"], "how": "", "magnitude": 4, "route": "god", "arg": "memory" })),
            _ => intents.push(json!({ "verb": "ask", "targets": ["the old fisherman with the limp"], "how": "", "magnitude": 2, "route": "world", "arg": "" })),
        }
        json!({ "said": "On it, boss.", "intents": intents })
    } else if s.contains("You are a SUBAGENT") || s.contains("POSSESSION") || s.contains("PALADIN")
    {
        json!({ "actions": [{ "kind": "persuade", "target": "K1", "magnitude": 3, "valence": 1, "act": "works the crowd with a rumor" }],
                "done": n.is_multiple_of(4), "report": "Did what I could.", "said": "Psst." })
    } else if s.contains("ramifications") && s.contains("You are ENTROPY") {
        json!({ "speech": "YES! More power, ahahhaahhaaa!", "ramifications": [
            { "kind": "mood", "text": "the crowd screams and scatters", "target": "K1", "amount": -0.8 },
            { "kind": "happen", "text": "every car alarm in town sings at once" },
            { "kind": "lasting", "text": "a storm settles in and refuses to leave",
              "lasting": { "name": "Permanent thunderstorm", "beats": 0, "harm": 0.2, "fear": 0.1 } },
            { "kind": "harm", "text": "a flying billboard clips the target", "target": "P1", "amount": 1.0 }
        ]})
    } else if s.contains("a god of FINAL NIGHT 7") {
        let effects = if s.contains("You are Time") {
            json!([{ "kind": "rewind", "target": null, "beats": 1, "text": "Time rewinds the invoker a beat" }])
        } else if s.contains("You are Memory") {
            json!([{ "kind": "forget", "target": "P2", "about": ["P1"], "text": "a memory slips away" }])
        } else {
            json!([{ "kind": "harm", "target": "P2", "amount": 2.0, "text": "a bolt from the sky" }])
        };
        json!({ "verdict": "bargain", "price_act": null, "price_scale": 1.0, "speech": "Granted, for a price.",
                "effects": effects, "dispatch": if n.is_multiple_of(5) { json!("my paladin") } else { Value::Null },
                "mission": "remind them who they owe", "favor_delta": 0.5 })
    } else if s.contains("GAME MASTER") {
        json!({ "effects": [
            { "kind": "mood", "actor": "P1", "target": "K1", "amount": 0.1, "feel": "hope", "text": "the crowd perks up", "act": 1, "magnitude": 3, "valence": 1, "witnesses": [] },
            { "kind": "damage", "actor": "P1", "target": "P2", "amount": 1.0, "text": "a slap lands", "act": 2, "magnitude": 3, "valence": -2, "witnesses": [] }
        ]})
    } else if s.contains("NARRATOR of FINAL NIGHT 7, a battle royale") {
        json!({ "caption": format!("Beat chaos number {n}."), "callouts": ["Consequence is taking notes."] })
    } else if s.contains("You are a GENERAL in FINAL NIGHT 7") {
        json!({ "reading": "obey", "target": null, "stance": "fight", "speech": "Form up. We hold the line.", "defect_to": null })
    } else if s.contains("give one ordinary person") {
        json!({ "flavor": "Paint-flecked overalls, hums sea shanties, wants to be left alone." })
    } else if s.contains("compress memories") {
        json!({ "summary": "A lot happened; I remember the important bits." })
    } else if s.contains("You name the things demigods conjure") {
        json!({ "name": "The Copper Tide", "description": "A sea of spears under copper and teal banners.",
                "motto": "Ten thousand for every star", "twist": "" })
    } else if s.contains("final cut") {
        json!({ "title": "The Last Bill", "story": "Everyone paid. One walked away." })
    } else {
        return None;
    })
}

#[derive(Default)]
pub struct Scripted {
    pub calls: AtomicU64,
}

#[async_trait]
impl LlmProvider for Scripted {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        // Varied by the request itself, not arrival order: concurrent calls must not race for counter values.
        let k = fn7_model::seed::hash_str(
            &req.messages
                .first()
                .map(|m| m.1.clone())
                .unwrap_or_default(),
        );
        match scripted_reply(&req, k) {
            Some(v) => Ok(LlmResponse {
                choices: vec![v.to_string()],
                cached: false,
            }),
            None => anyhow::bail!("scripted: unknown prompt"),
        }
    }
}

pub struct Failing;

#[async_trait]
impl LlmProvider for Failing {
    async fn complete(&self, _req: LlmRequest) -> anyhow::Result<LlmResponse> {
        anyhow::bail!("offline by design")
    }
}

pub struct Slow(pub Duration);

#[async_trait]
impl LlmProvider for Slow {
    async fn complete(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        tokio::time::sleep(self.0).await;
        Scripted::default().complete(req).await
    }
}

/// A planet match played offline through the plugin's hooks (4 bot seats, [`Scripted`] minds) for `beats` beats, with
/// every demigod kept alive and healthy after each commit so the state keeps growing like a long match (armies, mobs,
/// people, ledger and memory). For query tests and timing.
pub async fn played_state(seed: u64, beats: u32, beats_total: u32) -> Value {
    use fn7_model::AgentStatus;
    use theatre_plugin_api::{GamePlugin, NoProgress, RoundContext, SeatInfo};
    let llm = Scripted::default();
    let p = crate::FinalNight7Plugin::new();
    let seats: Vec<SeatInfo> = (0..4)
        .map(|i| SeatInfo { seat: i, display_name: ["Ash", "Brix", "Cyra", "Dune"][i as usize].into(), is_bot: true })
        .collect();
    let settings = p.validate_settings(&json!({ "seed": seed, "beats_total": beats_total })).expect("settings");
    let mut state = p.on_match_start(&seats, &settings, &llm).await.expect("start");
    for round in 1..=beats {
        state = p.on_round_start(&state, round, &llm).await.expect("round start").state;
        let mut actions = vec![];
        for s in 0..4u8 {
            actions.push(p.bot_action(&state, s, &llm).await.expect("bot"));
        }
        let o = p
            .resolve_round(&state, round, &actions, &RoundContext::default(), &llm, &NoProgress)
            .await
            .expect("resolve");
        state = p.commit_round(&o.state, round, &[], &llm).await.expect("commit").state;
        let mut w = crate::world::load(&state).expect("load");
        for d in w.demigods.iter_mut() {
            d.max_health = d.max_health.max(5.0);
            d.health = d.max_health;
            d.alive = true;
            d.eliminated_beat = None;
            d.cause = None;
            for h in d.hands {
                w.agents[h.ix()].status = AgentStatus::Live;
            }
        }
        state = crate::world::save(&w);
    }
    state
}

/// A new match's world with `players` built-in AI demigods (two bodies each at rules 0.7.0) and `seed`, no seats: for
/// unit tests of rules that read a whole world.
pub fn world(players: u32, seed: u64) -> fn7_model::World {
    let settings = crate::Settings::validate(&serde_json::json!({ "players": players, "seed": seed, "llm": false }))
        .expect("valid settings");
    let pantheon = fn7_pantheon::load();
    crate::phases::new_match(&[], &settings, &pantheon, seed)
}
