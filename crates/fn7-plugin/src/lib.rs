//! # fn7-plugin
//!
//! Final Night 7 as a Theatre `GamePlugin`. One Theatre round = one beat (the round number is the beat):
//! Start (Time's clock, due debts) → Brief (per-seat view) → Compose (beat window; `fields: {left, right, zoom}`) →
//! Resolve (intake → route → act → merge → consequence → narrate) → Reveal (`SeatDelta`) → Commit.
//! Every stage is timed into the `beat` event; every effect, ruling, memorization, spawn and ledger row is a trace event.
//!
//! The plugin state is the `fn7_model::World` itself (settings in `meta.settings`, per-seat views in `seats`).
//! Demigods no seat plays (when `players` > seats) are driven by the built-in AI with synthetic seat numbers
//! ≥ [`SYNTHETIC_SEAT`], which never reach the Theatre, until a mid-match joiner's new seat takes one over
//! ([`table`], `on_seat_takeover`).

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
pub mod hotspots;
pub mod items;
pub mod occupancy;
pub mod merge;
pub mod payload;
pub mod phases;
pub mod queries;
pub mod settings;
pub mod society;
pub mod table;
pub mod testkit;
pub mod visual;
pub mod world;

pub use settings::Settings;

pub const PLUGIN_ID: &str = "final_night_7";
/// Bump on any change to rules, prompts or how outcomes are computed.
pub const RULES_VERSION: &str = "0.7.0";
pub const RULES_LABEL: &str = "planet edition on Theatre v2.2: flow beats that never wait; text entry = Time's slow \
windows (core action windows, Enter cut-off, penalty from real seconds open); casts = instant bolts with a public streak, \
Entropy's yes and ramifications; Death's revive returns a seat to play; armies, world mods, Entropy to heat death; \
Final Night leaves exactly one player; two demigods per player (Left and Right hands, each its own body and health); \
the item engine judges manifestations; no two demigods share a people-level hex unless engaged";
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
    "Arena: the planet (default; regions = UN subregions from Natural Earth, each with its largest cities; demigods start in different regions; Time's clock shrinks a circle around the convergence city from the whole globe to 1500 km by beat arena_close_beats, its own schedule, independent of beats_total) or one city (districts and sites).",
    "Start: Time's clock shrinks the arena on an ease-out curve over arena_close_beats; demigods and possessed bodies outside take 0.5-3 damage (0.5 + 1.5 x the circle's progress + 4 x the overshoot) and are dragged back inside. Debts due (everything on the Final Night) are queued as Consequence's collections. Agents past their budget expire; a rewind frame is captured.",
    "Text entry is the God of Time interaction, on the Theatre's action windows: a seat's action.begin opens a slow-time window (deadline slow_ms; refused, with no window, if its demigod is dead or has no bolt) and everyone is told who; flow beats keep resolving. Other seats' orders are held by the core, hidden (a begin from another seat only registers it as engaged, unseen), and arrive at the next resolve when it closes. The initiator's submit (Enter) closes it at once; later submits get a private cut reply and never arrive. On close Time takes a bolt as a toll (no rival gains from it; Entropy still rises 0.4) and writes a debt of 0.5 x (1 + seconds open/10) x (1 + 0.5 x slows in the last 20 beats) (doubled if the bolt can't be paid), and Entropy rises by 0.5 + seconds/30; seconds open run from the window's real open time to the Enter's received_at_ms (or the deadline). Orders sent without a window (Compose: bots, the plugin's AI, clients that skip action.begin) are an instant order: the same toll and Entropy as a zero-second slow, but a flat debt of instant_order_debt (no repeat or seconds factor; doubled if the bolt can't be paid); they still count as slows for a later window's repeat factor.",
    "Casts (UX god powers from data/powers.json, or a custom power priced by magnitude) are Theatre instant actions (any phase): validated on arrival (known power, enough bolts for the action's total at the last commit; casts never carry orders) and shown to everyone at once as a public streak (on_instant). They are paid at the start of the next Resolve in arrival order (the plugin's AI first, then the core's seq); a cast its caster can no longer pay fizzles. It costs its price in bolts, every other live demigod gains 2 per bolt, Entropy rises 0.4 per bolt (0.2 per bolt Entropy hands out), and it strikes its point at once. Entropy's check always says yes: one ramifications call per region with casts (N = 1 + cost + magnitude/3 per cast, at most 10) in the act batch becomes geotagged rows and lasting world mods (at most 24 live; the same name in the same region stacks, up to 5).",
    "Mega and mini powers (rules 0.7.0, the 1:5 economy): every player holds lightning bolts (the mega currency, bolt_start 1) and sparks (the mini currency, spark_start 5), a purse both demigods share. A cast says tier mega or mini (absent = mini). A MEGA acts at map scale (each power's mega variant: radius x 4 within 600-2500 km, magnitude + 2, damage x 1.5 on its target and 40% on every rival demigod within the radius) and costs a bolt; every OTHER player with a demigod standing gains bolt_gift (1) bolts and spark_gift (0) sparks, once; Entropy rises entropy_per_mega (10). A MINI acts on the board (radius / 20 within 3-30 km, magnitude - 3, damage x 0.6, its target only), costs a spark, gifts nothing, and raises Entropy entropy_per_mini (1). This replaces the old cast price and its 2-per-bolt gift; bolts still pay Time's toll and gods' prices. A player spends at most the charges it held at the beat's start (gifts are spendable from the next beat). Charges are public.",
    "Intake: each hand with words is one hand-mind call (max_intents_per_hand acts, or max_intents_per_window for orders from a window open at least one beat). Hands that spoke keep their ordinary acts as standing orders (2 per hand); silent demigods' hands carry them out by rules every other beat, with no LLM call.",
    "Mentions: places and groups match by name words; people resolve to an existing character (fuzzy name/alias) or are memorized from the best cluster (max_memorize_per_beat), inheriting what their cluster knew; a materialize mind writes their flavor.",
    "Route: world acts group by district/region (one resolver per active one; it uses the LLM only when someone who spoke this beat acts there); army acts (raise an army from a crowd, send it at a place or a rival) are rules; divine acts go to the owning god (one god mind per invocation; it costs 1 + magnitude/5 bolts, paid unless refused; a god whose price can't be paid twists or refuses; Entropy charges nothing, always says yes and gives 2 bolts; an unmade god's invocations go to Entropy; Time can cool Entropy by 4 + 1.2 x magnitude for its price). Subagents and possessions use the LLM only when their demigod spoke, paladins when their god ruled. Resolvers, gods, steps, flavor and ramifications run in ONE concurrent batch.",
    "World rules each beat: armies march up to 900 km toward their target, rival armies within 300 km clash (each loses 8-16% of the other's size), armies within 200 km of a rival demigod strike it for (0.5 + size/40000) x (0.6 + 0.6 x the general's competence), at most 2, times army_strike_scale (one army per demigod per beat); a crowd near a demigod with no army yet raises a first volunteer host with extra chance 0.2 x army_first_host_scale a beat; the resolver's rules fallback gives the actor of a non-attack act a Final Destination accident (1-3 damage) with chance accident_scale/20; Entropy at 20+ brings freak weather, 40+ unreliable physics (demigods flung, a third of casts off target), 60+ riots and worship, 80+ wounds to the gods; world mods harm, heal and sway what is inside them.",
    "Merge: Time's Delay beats Consequence's Collect on the same debt (the debt grows 50% and moves); a Time Rewind overrules Nature and Chance on its target that beat (never debts or prices); gods pulling one target opposite ways: higher pantheon precedence wins, Chance's seeded coin breaks ties; several non-rules damage proposals on one target become one hit at the median. Each surviving proposal is applied (invalid ones are recorded with the reason) and appends one ledger row, geotagged.",
    "Consequence's bill: per demigod per beat, debt_scale x sum of debt_for(magnitude, words this beat) over its acts that landed, due in debt_due_beats.",
    "Consequence stage: memories from the beat's rows; news spreads one hop along group overlaps; moods and allegiances drift; owners over memory_per_owner fold by rules, except the first llm_folds_per_beat of them, whose folds the fold mind writes during the next beat.",
    "Narrate (off the critical path): beat N's narrator runs during beat N+1 (with the LLM only if beat N was loud: someone spoke, a power was cast or someone fell), on beat N's applied rows, public rulings, eliminations, standings and the Entropy line; delivered as previous_caption. The last beat's caption is written in the summary.",
    "Hidden information: a seat sees the rows its demigod, hands, agents or possessed bodies did, saw, were told or were hit by, plus what its Following heard. Public: the arena, live demigods' positions, hex cells and bolts (never health, per demigod: a seat sees only its own two demigods' health), casts and their ramifications, armies, world mods, freak weather, eliminations, big rulings, Entropy, the slow window's initiator (never who else typed), the caption, each demigod's influence per region.",
    "Heat death: Entropy reaching 100 makes that beat the Final Night at once: every outstanding debt is collected at its commit, then the survivor rule.",
    "Revive: a demigod brought back from the dead (Death's raise-the-dead, or the survivor rule) whose seat was reported eliminated is reported revived (seat.revived) and plays again.",
    "Final Night (beat = beats_total, whatever the circle's schedule), and any beat that would end with nobody standing: exactly one PLAYER survives (rules 0.7.0): the most total standing over its demigods (health now minus outstanding debt), then the most total health at the beat's start, then its lowest demigod id; its standing demigods stand, every other player's fall; with none standing, its demigod with the most start-of-beat health is revived at that health (immune that beat). Schema-4 matches keep the per-demigod rule: lowest outstanding debt, then most health, then most health at the beat's start.",
    "Two demigods per player (rules 0.7.0, state schema 5; room setting demigods_per_player, default 2): each player's Left and Right hands are two demigods, each a body on the globe with its own health (10), position, items, deeds, look, portrait and diorama, wounded, eliminated and revived on its own. Default names \"<player>'s Left Hand\" / \"<player>'s Right Hand\". The Right lands 150 km from the Left (a city: 8% of the radius). The left text (l: / fields.left) drives the Left demigod and the right text the Right one; a fallen demigod's text goes to its standing sibling. Bolts are a per-player purse (both demigods show it; every rival PLAYER gains 2 per bolt spent, once). Debts are per demigod: the acting body owes (Time's debt for orders is split evenly between the demigods that got orders; casts and Time's bolt come from the purse; the lead demigod, the first standing, answers for the seat). Every rule that touches a demigod applies per body: Time's circle, armies striking (never a sibling's army), accidents, casts' area, gods' prices (Life), revive, Entropy's flinging, possession. Rivals are other players' demigods. A player is out (seat.eliminated) only when both demigods have fallen, and back (seat.revived) when either stands again.",
    "Item engine (rules 0.7.0): an order to manifest something (a making verb: manifest, conjure, forge, summon, raise, build, create... and a thing: army, fleet, creature, structure, weapon, armor, relic, artifact, phenomenon; an army only with a count >= 1,000, banners or a strong verb; acts routed to a god by keyword count too, except phenomena) is judged by rules: tier from kind, count and magnitude (trinket <= 12, artifact <= 1,000, relic <= 100,000, wonder <= 1M, legend <= 10M, myth <= 100M, titan <= 1B, apocalypse beyond); cost = tier price in bolts (0,0,1,1,2,3,4,6 x manifest_scale) from the purse, the shortfall as debt to Consequence 0.6 x missing x (1 + tier/4) (refused instead if the order says no debt); odds = clamp(sigmoid(logit(base[tier]) + 0.35 x extra bolts - 0.15 x missing + 1.2 x favor/10 + 0.8 x Entropy/100 (wonder+) - 0.3 x recent attempts), 0.02, 0.99) with bases 0.97,0.90,0.80,0.66,0.52,0.38,0.25,0.14; one seeded roll (seed, MANIFEST_SALT, beat, mf id): success, twisted (0.04 + 0.30 x e of successes), partial (0.25 + 0.05 x tier of failures: a tenth of a host, one tier down for an object), backfired ((0.05 + 0.25 x e) x tier/7: 0.5 + 0.25 x tier damage to the maker) or failed. Bolts spent gift rivals and raise Entropy as any bolt, plus 0.15 per tier above relic. Every attempt is a row (mf:N); a made thing is an item held by its maker with kind, icon, tier, count (f64), state, banner and custody; a host of 100+ takes the field as an army of muster_size(count) x manifest_muster_scale under its maker, its losses scaling the item's count, destroyed when gone. The LLM (manifest mind, wonders and up and twists, at most manifest_calls_per_beat) only names and describes; the rules name the rest (never a bare verb). Public from relic tier up.",
    "People-level occupancy (rules 0.7.0, Benito's rule): cells are the client's hex cells (cube-face gnomonic projection, R = 6,371 km, pointy-top axial, people level 3 m). After each beat's moves no two standing demigods share a people-level cell unless engaged (one acted directly on the other this beat: a fight, a seduction, a parley); otherwise the later mover is nudged to the nearest free neighbouring cell (rings 1-6, fixed order; non-movers keep their cells first, then movers by their last move row).",
    "The Final Wish (rules 0.7.0): beat 1 is the final wish: each player's orders are their final wish; both hands get max_intents_per_window acts, every act gets final_wish_magnitude more magnitude, the hand and resolver prompts frame it, the narrator calls them wishes, and every built-in AI demigod makes a wish (within its purse). Every wish costs as normal.",
    "Hotspots (rules 0.7.0): per region, the densest weighted spot of the last 5 beats (public events 1 + magnitude/3, battles 4, live mobs 2 + size/2000 up to 6, crowds in unrest 1 + 2 x anger, demigods 3) within a quarter of the region's radius (25-400 km; a city 0.3-3 km): centre = weighted mean, radius = weighted spread (at least half that), score = weight. Built-in AI demigods drift toward the nearest hotspot within reach about one beat in four when the circle isn't pressing.",
    "Mid-match join (room setting allow_midmatch_join): at the round boundary a human taking over a bot's seat keeps its demigod, now human-controlled; a new seat takes over the built-in AI demigod still alive with the most health (then the lowest id), whose hands follow that seat's orders from that beat; with none alive the joiner spectates (no demigod). Every demigod's public controller is human or ai.",
    "Match ends at beats_total or when at most one player stands. Standings and the champion are per player (each demigod's state shown); every demigod gets a pile, a portrait prompt, a diorama prompt and an amiibo statue prompt.",
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
        final_wish: false,
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
        items_by_engine: false,
    })
}

/// The Final Night 7 plugin. Holds the pantheon (data, compiled in) and nothing else: everything Time's bill needs
/// comes from the core (`RoundContext`, also passed to `fallback_outcome`), and the core runs every hook, sync ones
/// included, inside the match's trace context (theatre 1741d8f), so the plugin keeps no memory between hooks.
pub struct FinalNight7Plugin {
    pantheon: Arc<Pantheon>,
    /// The server's media style config (`{version, hash, source, body}`), recorded in every match's ruleset as
    /// `media_style` (the plugin never reads it otherwise; images are the server's).
    media_style: Option<Value>,
}

impl Default for FinalNight7Plugin {
    fn default() -> Self {
        Self {
            pantheon: Arc::new(fn7_pantheon::load()),
            media_style: None,
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

    /// Record the server's media style (`fn7_media::MediaStyle::ruleset_json`) in every match's ruleset.
    pub fn with_media_style(mut self, style: Value) -> Self {
        self.media_style = Some(style);
        self
    }

    /// The provider minds use: `None` when the room turned the LLM off (every mind falls back, no requests).
    fn llm<'a>(w: &World, llm: &'a dyn LlmProvider) -> Option<&'a dyn LlmProvider> {
        world::settings(w).llm.then_some(llm)
    }

    /// `on_action_begin` on a loaded world.
    fn action_begin(
        &self,
        w: &World,
        round: u32,
        seat: Seat,
        window: Option<&WindowInfo>,
        next_window_id: u64,
    ) -> PhaseControl {
        let Some(s) = world::seat_state(w, seat) else {
            return PhaseControl::default();
        };
        // The seat's lead demigod (its first standing one): a player is in play while either demigod stands.
        let lead = w.lead_of_seat(s.seat).unwrap_or(s.demigod);
        let d = &w.demigods[lead.ix()];
        if let Some(info) = window {
            // Someone else's window: this seat engages, unseen. Recorded for the data, never shown.
            world::event(
                "slow_engaged",
                Some(Ref::Demigod(d.id).to_string()),
                json!({ "round": round, "seat": seat, "window_id": info.window_id, "by_seat": info.by_seat,
                        "opened_ms": info.opened_ms, "at_ms": theatre_trace::now_ms() }),
            );
            return PhaseControl::default();
        }
        let refuse = if !d.alive {
            Some("fallen")
        } else if d.bolts < 1 {
            Some("no bolt to pay Time")
        } else {
            None
        };
        if let Some(reason) = refuse {
            world::event(
                "slow_refused",
                Some(Ref::Demigod(d.id).to_string()),
                json!({ "round": round, "seat": seat, "reason": reason, "bolts": d.bolts }),
            );
            return PhaseControl::default();
        }
        let st = world::settings(w);
        let now = theatre_trace::now_ms();
        let deadline = now + st.slow_ms as i64;
        let bill = crate::payload::time_bill(w, d.id);
        let id = next_window_id; // exact: the id the core gives the window this call opens
        world::event(
            "slow_begin",
            Some(Ref::Demigod(d.id).to_string()),
            json!({ "round": round, "seat": seat, "demigod": d.id, "window_id": id, "begun_ms": now,
                    "deadline_ms": deadline, "slow_ms": st.slow_ms, "bolts": d.bolts }),
        );
        PhaseControl {
            window: Some(WindowControl {
                deadline_in_ms: st.slow_ms as u64,
                close_on_submit_of: Some(seat),
                missing: MissingActions::Drop,
            }),
            public: Some(json!({
                "event": "slowed",
                "by": d.player_name(),
                "by_ref": Ref::Demigod(d.id),
                "by_seat": seat,
                "slow_ms": st.slow_ms,
                "text": format!("{} slowed time", d.player_name()),
                // The live Time bill (rules 0.7.0): debt(t) = min(debt_cap, debt_base + debt_per_second × t), t = seconds
                // since opened_at_ms, plus one bolt (doubled debt if it can't be paid; see `time_bill`).
                "opened_at_ms": now,
                "deadline_ms": deadline,
                "bill_now": { "bolts": 1, "debt": bill["debt_base"].clone() },
                "debt_base": bill["debt_base"].clone(),
                "debt_per_second": bill["debt_per_second"].clone(),
                "debt_per_10s": bill["debt_per_10s"].clone(),
                "debt_cap": bill["debt_cap"].clone(),
                "repeat_factor": bill["repeat_factor"].clone(),
                "recent_slows": bill["recent_slows"].clone(),
                "bolt_paid": bill["can_pay_bolt"].clone(),
            })),
            ..Default::default()
        }
    }

    /// The turns of a beat and every submission's arrival facts. A seat may have several actions (this round's
    /// Compose action, then windowed and instant ones in arrival order): their orders join in that order, their casts
    /// keep their `seq`. A bad action counts as silence; an instant action's orders are ignored (casts only).
    fn turns(w: &World, round: u32, actions: &[Action]) -> (Vec<beat::Turn>, Vec<flow::Sub>) {
        let mut out = vec![];
        let mut subs = vec![];
        for s in &w.seats {
            // The seat's lead demigod (its first standing one; schema 4: its only one) answers for the seat's orders
            // and casts; a player is out only when every demigod of theirs has fallen.
            let Some(lead) = w.lead_of_seat(s.seat).filter(|_| w.seat_alive(s.seat)) else {
                continue;
            };
            let d = &w.demigods[lead.ix()];
            let input = if s.seat >= SYNTHETIC_SEAT {
                let i = actions::bot_input(w, s.seat, round);
                if i.has_orders() {
                    subs.push(flow::Sub {
                        demigod: d.id,
                        seat: s.seat,
                        window_id: None,
                        seq: 0,
                        at_ms: 0,
                        orders: true,
                        ai: true,
                    });
                }
                i
            } else {
                let mut input = actions::Input::default();
                for a in actions.iter().filter(|a| a.seat == s.seat) {
                    let Ok(mut i) = actions::parse(&a.text, &a.fields) else {
                        continue;
                    };
                    if a.instant && i.has_orders() {
                        world::event(
                            "orders_ignored",
                            Some(Ref::Demigod(d.id).to_string()),
                            json!({ "beat": round, "seat": a.seat, "seq": a.seq,
                                    "reason": "instant actions carry casts, not orders" }),
                        );
                        i.left.clear();
                        i.right.clear();
                    }
                    let orders = i.has_orders() && !a.defaulted;
                    if orders || a.windowed {
                        subs.push(flow::Sub {
                            demigod: d.id,
                            seat: a.seat,
                            window_id: if a.windowed { a.window_id } else { None },
                            seq: a.seq,
                            at_ms: a.received_at_ms,
                            orders,
                            ai: false,
                        });
                    }
                    input.left = join(&input.left, &i.left);
                    input.right = join(&input.right, &i.right);
                    if i.zoom.is_some() {
                        input.zoom = i.zoom;
                    }
                    for mut c in i.casts {
                        c.seq = a.seq;
                        c.instant = a.instant;
                        input.casts.push(c);
                    }
                }
                input
            };
            out.extend(beat::split_turn(w, s.seat, d.id, input));
        }
        (out, subs)
    }

    async fn resolve(
        &self,
        w: &mut World,
        round: u32,
        actions: &[Action],
        ctx: &RoundContext,
        llm: Option<&dyn LlmProvider>,
    ) -> RoundOutcome {
        let (turns, subs) = Self::turns(w, round, actions);
        let out = beat::resolve(w, round, turns, &subs, &ctx.windows_closed, llm, &self.pantheon).await;
        RoundOutcome {
            public: out.public,
            per_seat: out.per_seat,
            choices: vec![],
            state: world::save(w),
        }
    }
}

pub(crate) fn join(a: &str, b: &str) -> String {
    match (a.trim().is_empty(), b.trim().is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a}\n{b}"),
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
            version: env!("CARGO_PKG_VERSION"),
            rules_version: RULES_VERSION,
            min_players: 1,
            max_players: 8,
            min_active: 0,
            default_settings: Settings::validate(&Value::Null).expect("defaults validate"),
        }
    }

    fn validate_settings(&self, settings: &Value) -> Result<Value, String> {
        Settings::validate(settings)
    }

    /// The `game.*` queries (docs/CLIENT_API.md "Game queries"); cheap partial reads of the state ([`queries`]).
    fn queries(&self) -> Vec<QuerySpec> {
        queries::specs()
    }

    fn query(&self, state: &Value, seat: Option<Seat>, name: &str, args: &Value) -> Result<Value, String> {
        queries::query(&self.pantheon, state, seat, name, args)
    }

    async fn on_match_start(
        &self,
        seats: &[SeatInfo],
        settings: &Value,
        _llm: &dyn LlmProvider,
    ) -> anyhow::Result<Value> {
        let settings = Settings::validate(settings).map_err(anyhow::Error::msg)?;
        let w = phases::new_match(seats, &settings, &self.pantheon, now_seed());
        let mut body = ruleset_body(&w, &self.pantheon);
        if let Some(m) = &self.media_style {
            body["media_style"] = m.clone();
        }
        theatre_trace::ruleset(PLUGIN_ID, RULES_VERSION, RULES_LABEL, body);
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
        kind: SubmitKind,
    ) -> Result<(), String> {
        let w = world::load(state).map_err(|e| e.to_string())?;
        let i = actions::parse(text, fields)?;
        if kind == SubmitKind::Instant && i.has_orders() {
            // Enforced on arrival (the core replies accepted:false); resolve keeps a safety net (`orders_ignored`).
            return Err("instant actions carry casts, not orders: send orders as text entry".into());
        }
        let Some(s) = world::seat_state(&w, seat) else {
            return Ok(());
        };
        let d = &w.demigods[w.lead_of_seat(s.seat).unwrap_or(s.demigod).ix()];
        if i.casts.is_empty() {
            return Ok(()); // a fallen demigod's player may type; it is ignored
        }
        if i.has_orders() {
            return Err("casts never carry orders: send casts as an instant action and orders as text entry".into());
        }
        if !d.alive {
            return Err("your demigods have fallen: they cast nothing".into());
        }
        // Read-only: checked against the last committed state; spending is settled at the next resolve.
        casts::affordable(&w, d.id, &i.casts)
    }

    fn on_action_begin(
        &self,
        state: &Value,
        round: u32,
        seat: Seat,
        window: Option<&WindowInfo>,
        next_window_id: u64,
    ) -> PhaseControl {
        let Ok(w) = world::load(state) else {
            return PhaseControl::default();
        };
        self.action_begin(&w, round, seat, window, next_window_id)
    }

    fn on_instant(&self, state: &Value, seat: Seat, action: &Action) -> Option<Value> {
        let w = world::load(state).ok()?;
        let s = world::seat_state(&w, seat)?;
        let i = actions::parse(&action.text, &action.fields).ok()?;
        if i.casts.is_empty() {
            return None;
        }
        let mut v = casts::streak(&w, w.lead_of_seat(s.seat).unwrap_or(s.demigod), seat, &i.casts)?;
        v["seq"] = action.seq.into();
        v["received_at_ms"] = action.received_at_ms.into();
        world::event("cast_streak", Some(Ref::Demigod(s.demigod).to_string()), v.clone());
        Some(v)
    }

    /// A human starts playing `takeover.seat` mid-match ([`table::seat_takeover`]): a bot's seat keeps its demigod; a
    /// new seat takes over the healthiest live built-in AI demigod; with none alive, `None` (the joiner spectates).
    fn on_seat_takeover(&self, state: &Value, takeover: &SeatTakeover) -> Option<Value> {
        let mut w = world::load(state).ok()?;
        table::seat_takeover(&mut w, takeover).then(|| world::save(&w))
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
        if !w.seat_alive(s.seat) {
            return Ok(self.default_action(state, seat));
        }
        // The brief comes before Compose, so the coming beat is meta.beat (set at the beat's Start).
        let i = actions::bot_input(&w, s.seat, w.meta.beat.max(1));
        Ok(actions::action_of(seat, &i))
    }

    async fn resolve_round(
        &self,
        state: &Value,
        round: u32,
        actions: &[Action],
        ctx: &RoundContext,
        llm: &dyn LlmProvider,
        _progress: &dyn Progress,
    ) -> anyhow::Result<RoundOutcome> {
        let mut w = world::load(state)?;
        let llm = Self::llm(&w, llm);
        Ok(self.resolve(&mut w, round, actions, ctx, llm).await)
    }

    fn fallback_outcome(&self, state: &Value, round: u32, actions: &[Action], ctx: &RoundContext) -> RoundOutcome {
        match world::load(state) {
            // No LLM: every mind takes its rules fallback immediately, so this never waits. `ctx` is the context the
            // failed resolve attempts got, so the windows they were handed are still billed.
            Ok(mut w) => futures::executor::block_on(self.resolve(&mut w, round, actions, ctx, None)),
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
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod media_tests;
#[cfg(test)]
mod table_tests;
#[cfg(test)]
mod two_bodies_tests;
