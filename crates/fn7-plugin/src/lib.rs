//! # fn7-plugin
//!
//! Final Night 7 as a Theatre `GamePlugin`. One Theatre round = one beat (the round number is the beat):
//! Start (Time's clock, due debts) → Brief (per-seat view) → Compose (beat window; `fields: {left, right, zoom}`) →
//! Resolve (intake → route → act → merge → consequence → narrate) → Reveal (`SeatDelta`) → Commit.
//! Every stage is timed into the `beat` event; every effect, ruling, memorization, spawn and ledger row is a trace event.
//!
//! The plugin state is the `fn7_model::World` itself (settings in `meta.settings`, per-seat views in `seats`).
//! Demigods no seat plays (when `players` > seats) are driven by the built-in AI with synthetic seat numbers
//! ≥ [`SYNTHETIC_SEAT`], which never reach the Theatre.

use std::sync::Arc;

use async_trait::async_trait;
use fn7_model::*;
use serde_json::{json, Value};
use theatre_plugin_api::*;

pub mod actions;
pub mod beat;
pub mod casts;
pub mod driver;
pub mod flow;
pub mod merge;
pub mod payload;
pub mod phases;
pub mod settings;
pub mod society;
pub mod testkit;
pub mod world;

pub use settings::Settings;

pub const PLUGIN_ID: &str = "final_night_7";
/// Bump on any change to rules, prompts or how outcomes are computed.
pub const RULES_VERSION: &str = "0.4.0";
pub const RULES_LABEL: &str = "planet edition: flow beats that never wait; text entry = Time's slow windows; casts = bolts, \
Entropy's yes and ramifications; armies, world mods, Entropy to heat death; Final Night leaves exactly one";
/// The compiled-in power palette (`data/powers.json`).
pub fn powers() -> &'static fn7_pantheon::Powers {
    static P: std::sync::OnceLock<fn7_pantheon::Powers> = std::sync::OnceLock::new();
    P.get_or_init(fn7_pantheon::powers)
}

/// Seat numbers at or above this are the built-in AI's demigods (never real Theatre seats).
pub const SYNTHETIC_SEAT: u8 = 200;

/// Mechanics enforced in code (the minds' rules are in their prompts, recorded alongside).
pub const MECHANICS: &[&str] = &[
    "One Theatre round = one beat (a flow beat, flow_ms; the room's compose_seconds). The world's action never waits for players: every beat resolves whether or not anyone typed. Health 10 per body on Final Night's 0-10 damage scale; a demigod at 0 is eliminated.",
    "Arena: the planet (default; regions = UN subregions from Natural Earth, each with its largest cities; demigods start in different regions; Time's clock shrinks a circle around the convergence city from the whole globe to 1500 km on the Final Night) or one city (districts and sites).",
    "Start: Time's clock shrinks the arena linearly; demigods and possessed bodies outside take 1-5 damage. Debts due (everything on the Final Night) are queued as Consequence's collections. Agents past their budget expire; a rewind frame is captured.",
    "Text entry is the God of Time interaction: a seat's slow request, or orders sent while no window is open, opens a slow-time window (Time takes one bolt; the window lasts up to ceil(slow_ms/flow_ms) flow beats, which keep resolving). Other seats' orders during it are held, hidden, and go through when it closes. The initiator's orders (Enter) close it; held orders and orders sent before the Enter that beat (by fields.sent_at; the plugin's AI counts as first) go through, later ones and orders tagged for a closed window are cut off. On close Time writes a debt of 0.5 x (1 + seconds open/10) x (1 + 0.5 x slows in the last 20 beats) (doubled if the bolt couldn't be paid), and Entropy rises by 0.5 + seconds/30.",
    "Casts (UX god powers from data/powers.json, or a custom power priced by magnitude): settled in arrival order at the start of Resolve; a cast its caster can't pay fizzles. It costs its price in bolts, every other live demigod gains 2 per bolt, Entropy rises 0.4 per bolt (0.2 per bolt Entropy hands out), and it strikes its point at once. Entropy's check always says yes: one ramifications call per region with casts (N = 1 + cost + magnitude/3 per cast, at most 10) in the act batch becomes geotagged rows and lasting world mods (at most 24 live; the same name in the same region stacks, up to 5).",
    "Intake: each hand with words is one hand-mind call (max_intents_per_hand acts, or max_intents_per_window for orders from a window open at least one beat). Hands that spoke keep their ordinary acts as standing orders (2 per hand); silent demigods' hands carry them out by rules every other beat, with no LLM call.",
    "Mentions: places and groups match by name words; people resolve to an existing character (fuzzy name/alias) or are memorized from the best cluster (max_memorize_per_beat), inheriting what their cluster knew; a materialize mind writes their flavor.",
    "Route: world acts group by district/region (one resolver per active one; it uses the LLM only when someone who spoke this beat acts there); army acts (raise an army from a crowd, send it at a place or a rival) are rules; divine acts go to the owning god (one god mind per invocation; it costs 1 + magnitude/5 bolts, paid unless refused; a god whose price can't be paid twists or refuses; Entropy charges nothing, always says yes and gives 2 bolts; an unmade god's invocations go to Entropy; Time can cool Entropy by 4 + 1.2 x magnitude for its price). Subagents and possessions use the LLM only when their demigod spoke, paladins when their god ruled. Resolvers, gods, steps, flavor and ramifications run in ONE concurrent batch.",
    "World rules each beat: armies march up to 900 km toward their target, rival armies within 300 km clash (each loses 8-16% of the other's size), armies within 200 km of a rival demigod strike it; Entropy at 20+ brings freak weather, 40+ unreliable physics (demigods flung, a third of casts off target), 60+ riots and worship, 80+ wounds to the gods; world mods harm, heal and sway what is inside them.",
    "Merge: Time's Delay beats Consequence's Collect on the same debt (the debt grows 50% and moves); a Time Rewind overrules Nature and Chance on its target that beat (never debts or prices); gods pulling one target opposite ways: higher pantheon precedence wins, Chance's seeded coin breaks ties; several non-rules damage proposals on one target become one hit at the median. Each surviving proposal is applied (invalid ones are recorded with the reason) and appends one ledger row, geotagged.",
    "Consequence's bill: per demigod per beat, debt_scale x sum of debt_for(magnitude, words this beat) over its acts that landed, due in debt_due_beats.",
    "Consequence stage: memories from the beat's rows; news spreads one hop along group overlaps; moods and allegiances drift; owners over memory_per_owner fold by rules, except the first llm_folds_per_beat of them, whose folds the fold mind writes during the next beat.",
    "Narrate (off the critical path): beat N's narrator runs during beat N+1 (with the LLM only if beat N was loud: someone spoke, a power was cast or someone fell), on beat N's applied rows, public rulings, eliminations, standings and the Entropy line; delivered as previous_caption. The last beat's caption is written in the summary.",
    "Hidden information: a seat sees the rows its demigod, hands, agents or possessed bodies did, saw, were told or were hit by, plus what its Following heard. Public: the arena, live demigods' positions and bolts, casts and their ramifications, armies, world mods, freak weather, eliminations, big rulings, Entropy, the slow window's initiator (never who else typed), the caption, each demigod's influence per region.",
    "Heat death: Entropy reaching 100 makes that beat the Final Night at once: every outstanding debt is collected at its commit, then the survivor rule.",
    "Final Night (beat = beats_total), and any beat that would end with nobody standing: exactly one demigod survives: lowest outstanding debt, then most health, then most health at the beat's start; a fallen survivor is revived at its start-of-beat health (immune that beat).",
    "Match ends at beats_total or when at most one demigod stands. Every demigod gets a pile, a portrait prompt and an amiibo statue prompt.",
];

/// The rules body recorded with every run (`theatre_trace::ruleset`).
pub fn ruleset_body(w: &World, pantheon: &Pantheon) -> Value {
    use fn7_agents::step::StepKind;
    let gods: std::collections::BTreeMap<String, String> = pantheon
        .gods
        .iter()
        .map(|g| (g.slug.clone(), sample_god_prompt(g)))
        .collect();
    let steps: std::collections::BTreeMap<String, String> =
        [StepKind::Subagent, StepKind::Possession, StepKind::Paladin]
            .into_iter()
            .map(|k| (k.step().to_string(), sample_step_prompt(k)))
            .collect();
    json!({
        "plugin": PLUGIN_ID,
        "rules_version": RULES_VERSION,
        "label": RULES_LABEL,
        "state_schema": STATE_SCHEMA,
        "pantheon_version": pantheon.version,
        "pantheon": pantheon,
        "powers": powers(),
        "society_template_version": fn7_society::template::template().version,
        "settings": w.meta.settings,
        "mechanics": MECHANICS,
        "prompts": {
            "resolver_system": fn7_agents::resolver::system_prompt(),
            "narrator_system": fn7_agents::narrator::system_prompt(),
            "materialize_system": fn7_agents::materialize::system_prompt(),
            "fold_system": fn7_agents::fold::system_prompt(),
            "ramify_system": fn7_agents::ramify::system_prompt(),
            "final_cut_system": phases::FINAL_CUT_SYSTEM,
            "hand_system (rendered, first demigod's left hand)": sample_hand_prompt(w, pantheon),
            "god_system (rendered per god)": gods,
            "step_system (rendered per kind)": steps,
            "voice": fn7_agents::common::VOICE,
        },
    })
}

fn sample_hand_prompt(w: &World, pantheon: &Pantheon) -> String {
    let Some(d) = w.demigods.first() else {
        return String::new();
    };
    let a = &w.agents[d.hands[0].ix()];
    fn7_agents::hand::system_prompt(&fn7_agents::hand::HandCtx {
        agent: a.id,
        demigod: d.id,
        demigod_name: d.name.clone(),
        hand: Hand::Left,
        persona: a.persona.clone(),
        beat: 1,
        first_intent: IntentId(0),
        place: Some(d.place),
        utterances: vec![],
        zoom: Zoom::District,
        focus: None,
        visible: vec![],
        subagents: vec![],
        memory: String::new(),
        pantheon: Arc::new(pantheon.clone()),
        max_subagents: world::settings(w).max_subagents_per_hand as usize,
    })
}

fn sample_god_prompt(g: &GodDef) -> String {
    fn7_agents::god::system_prompt(&fn7_agents::god::GodCtx {
        god: GodId(0),
        def: g.clone(),
        beat: 1,
        ruling: RulingId(0),
        intent: Intent {
            id: IntentId(0),
            beat: 1,
            agent: AgentId(0),
            demigod: DemigodId(0),
            verb: "<VERB>".into(),
            targets: vec![],
            how: String::new(),
            place: None,
            magnitude: 5,
            span: "<PLAYER WORDS>".into(),
            route: Route::Pending,
        },
        invoker: Ref::Demigod(DemigodId(0)),
        invoker_name: "<DEMIGOD>".into(),
        favor: 0.0,
        open_debt: 0.0,
        entities: vec![],
        paladins: vec![],
        rivals: g.rivals.clone(),
        context: String::new(),
        bolts: 3,
        cost: 1,
        world: "<ENTROPY LINE>".into(),
    })
}

fn sample_step_prompt(kind: fn7_agents::step::StepKind) -> String {
    fn7_agents::step::system_prompt(&fn7_agents::step::StepCtx {
        kind,
        agent: AgentId(0),
        name: "<NAME>".into(),
        persona: "<PERSONA>".into(),
        goal: "<GOAL>".into(),
        beat: 1,
        steps_taken: 0,
        beats_left: None,
        demigod: None,
        master_name: "<MASTER>".into(),
        body: None,
        intent: None,
        place: None,
        scope: vec![],
        visible: vec![],
        orders: vec![],
        memory: String::new(),
    })
}

/// The Final Night 7 plugin. Holds the pantheon (data, compiled in).
pub struct FinalNight7Plugin {
    pantheon: Arc<Pantheon>,
}

impl Default for FinalNight7Plugin {
    fn default() -> Self {
        Self {
            pantheon: Arc::new(fn7_pantheon::load()),
        }
    }
}

impl FinalNight7Plugin {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pantheon(&self) -> &Arc<Pantheon> {
        &self.pantheon
    }

    /// The provider minds use: `None` when the room turned the LLM off (every mind falls back, no requests).
    fn llm<'a>(w: &World, llm: &'a dyn LlmProvider) -> Option<&'a dyn LlmProvider> {
        world::settings(w).llm.then_some(llm)
    }

    /// The turns of a beat: seats' actions (parsed; a bad one counts as silence) and the built-in AI's lines.
    fn turns(w: &World, round: u32, actions: &[Action]) -> Vec<beat::Turn> {
        let mut out = vec![];
        for s in &w.seats {
            let d = &w.demigods[s.demigod.ix()];
            if !d.alive {
                continue;
            }
            let input = if s.seat >= SYNTHETIC_SEAT {
                actions::bot_lines(w, d.id, round)
            } else {
                actions
                    .iter()
                    .find(|a| a.seat == s.seat)
                    .and_then(|a| actions::parse(&a.text, &a.fields).ok())
                    .unwrap_or_default()
            };
            out.push(beat::Turn {
                demigod: d.id,
                seat: s.seat,
                input,
            });
        }
        out
    }

    async fn resolve(
        &self,
        w: &mut World,
        round: u32,
        actions: &[Action],
        llm: Option<&dyn LlmProvider>,
    ) -> RoundOutcome {
        let turns = Self::turns(w, round, actions);
        let out = beat::resolve(w, round, turns, llm, &self.pantheon).await;
        RoundOutcome {
            public: out.public,
            per_seat: out.per_seat,
            choices: vec![],
            state: world::save(w),
        }
    }
}

fn now_seed() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(7);
    fn7_model::seed::Rng::new(t).next_u64()
}

#[async_trait]
impl GamePlugin for FinalNight7Plugin {
    fn info(&self) -> PluginInfo {
        // min_active 0: the match ends by its own rule (the Final Night or one demigod left), not because every seat
        // is out while AI demigods still fight.
        PluginInfo {
            id: PLUGIN_ID,
            min_players: 1,
            max_players: 8,
            min_active: 0,
            default_settings: Settings::validate(&Value::Null).expect("defaults validate"),
        }
    }

    fn validate_settings(&self, settings: &Value) -> Result<Value, String> {
        Settings::validate(settings)
    }

    async fn on_match_start(
        &self,
        seats: &[SeatInfo],
        settings: &Value,
        _llm: &dyn LlmProvider,
    ) -> anyhow::Result<Value> {
        let settings = Settings::validate(settings).map_err(anyhow::Error::msg)?;
        let w = phases::new_match(seats, &settings, &self.pantheon, now_seed());
        theatre_trace::ruleset(
            PLUGIN_ID,
            RULES_VERSION,
            RULES_LABEL,
            ruleset_body(&w, &self.pantheon),
        );
        Ok(world::save(&w))
    }

    async fn on_round_start(
        &self,
        state: &Value,
        round: u32,
        _llm: &dyn LlmProvider,
    ) -> anyhow::Result<RoundStart> {
        let mut w = world::load(state)?;
        Ok(phases::round_start(&mut w, round, &self.pantheon))
    }

    async fn brief(
        &self,
        state: &Value,
        round: u32,
        _llm: &dyn LlmProvider,
    ) -> anyhow::Result<Brief> {
        let w = world::load(state)?;
        let per_seat = w
            .seats
            .iter()
            .filter(|s| s.seat < SYNTHETIC_SEAT)
            .map(|s| {
                let mut v = phases::seat_view(&w, s.seat);
                v["beat"] = round.into();
                (s.seat, v)
            })
            .collect();
        Ok(Brief {
            public: phases::brief_public(&w, round, &self.pantheon),
            per_seat,
        })
    }

    fn validate_action(
        &self,
        state: &Value,
        seat: Seat,
        text: &str,
        fields: &Value,
    ) -> Result<(), String> {
        let w = world::load(state).map_err(|e| e.to_string())?;
        if world::seat_state(&w, seat).is_some_and(|s| !w.demigods[s.demigod.ix()].alive) {
            return Ok(()); // spectators may type; it is ignored
        }
        actions::parse(text, fields).map(|_| ())
    }

    fn default_action(&self, _state: &Value, seat: Seat) -> Action {
        let mut a = actions::action(seat, "", "", None);
        a.defaulted = true;
        a
    }

    async fn bot_action(
        &self,
        state: &Value,
        seat: Seat,
        _llm: &dyn LlmProvider,
    ) -> anyhow::Result<Action> {
        let w = world::load(state)?;
        let Some(s) = world::seat_state(&w, seat) else {
            return Ok(self.default_action(state, seat));
        };
        if !w.demigods[s.demigod.ix()].alive {
            return Ok(self.default_action(state, seat));
        }
        // The brief comes before Compose, so the coming beat is meta.beat (set at the beat's Start).
        let i = actions::bot_lines(&w, s.demigod, w.meta.beat.max(1));
        Ok(actions::action_of(seat, &i))
    }

    async fn resolve_round(
        &self,
        state: &Value,
        round: u32,
        actions: &[Action],
        llm: &dyn LlmProvider,
        _progress: &dyn Progress,
    ) -> anyhow::Result<RoundOutcome> {
        let mut w = world::load(state)?;
        let llm = Self::llm(&w, llm);
        Ok(self.resolve(&mut w, round, actions, llm).await)
    }

    fn fallback_outcome(&self, state: &Value, round: u32, actions: &[Action]) -> RoundOutcome {
        match world::load(state) {
            // No LLM: every mind takes its rules fallback immediately, so this never waits.
            Ok(mut w) => futures::executor::block_on(self.resolve(&mut w, round, actions, None)),
            Err(_) => RoundOutcome {
                state: state.clone(),
                ..Default::default()
            },
        }
    }

    fn default_choice(&self, _state: &Value, _seat: Seat, offered: &[Choice]) -> String {
        offered.first().map(|c| c.id.clone()).unwrap_or_default()
    }

    async fn commit_round(
        &self,
        state: &Value,
        round: u32,
        _picked: &[Picked],
        _llm: &dyn LlmProvider,
    ) -> anyhow::Result<Committed> {
        let mut w = world::load(state)?;
        Ok(phases::commit(&mut w, round, &self.pantheon))
    }

    fn is_match_over(&self, state: &Value, round: u32) -> bool {
        world::load(state)
            .map(|w| phases::match_over(&w, round))
            .unwrap_or(true)
    }

    fn public_view(&self, state: &Value) -> Value {
        world::load(state)
            .map(|w| phases::public_view(&w))
            .unwrap_or(Value::Null)
    }

    fn seat_view(&self, state: &Value, seat: Seat) -> Value {
        world::load(state)
            .map(|w| phases::seat_view(&w, seat))
            .unwrap_or(Value::Null)
    }

    async fn summarize(&self, state: &Value, llm: &dyn LlmProvider) -> anyhow::Result<Summary> {
        let w = world::load(state)?;
        let llm = Self::llm(&w, llm);
        Ok(phases::summarize(&w, llm).await)
    }
}

#[cfg(test)]
mod tests;
