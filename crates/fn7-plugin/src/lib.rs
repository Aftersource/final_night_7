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
pub mod choreo;
pub mod dm;
pub mod duel;
pub mod driver;
pub mod earth;
pub mod faults;
pub mod godtier;
pub mod flow;
pub mod hotspots;
pub mod items;
pub mod look;
pub mod occupancy;
pub mod panel;
pub mod picker;
pub mod merge;
pub mod payload;
pub mod persona;
pub mod phases;
pub mod present;
pub mod queries;
pub mod settings;
pub mod skin;
pub mod society;
pub mod summon;
pub mod table;
pub mod testkit;
pub mod typing;
pub mod duel_ai;
pub mod moves;
pub mod visual;
pub mod weapons;
pub mod wills;
pub mod world;

pub use settings::Settings;

pub const PLUGIN_ID: &str = "final_night_7";
/// Bump on any change to rules, prompts or how outcomes are computed.
pub const RULES_VERSION: &str = "0.9.0";
pub const RULES_LABEL: &str = "planet edition on Theatre v2.2: flow beats that never wait; text entry = Time's slow \
windows (core action windows, Enter cut-off, penalty from real seconds open); casts = instant bolts with a public streak, \
Entropy's yes and ramifications; Death's revive returns a seat to play; armies, world mods, Entropy to heat death; \
Final Night leaves exactly one player; two demigods per player (Left and Right hands, each its own body and health); \
the item engine judges manifestations; no two demigods share a people-level hex unless engaged; the Dungeon Master's \
rulebook: a place x time memory, visible d20s, growing settlements, story pieces, map encounters, loot, day and night, \
roads, shrines, fault lines for megas, summoning and god-tier powers; challenge mode: typed duels decided by the dice; exact resume and duel panels";
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
    "Mid-match join (room setting allow_midmatch_join): at the round boundary a human taking over a bot's seat keeps its demigods, now human-controlled (rules 0.8.0: renamed \"<joiner>'s Left Hand\" / \"<joiner>'s Right Hand\", refs unchanged, hand agents too, remembered as seat.takeover); a new seat takes over the built-in AI demigod still alive with the most health (then the lowest id), whose hands follow that seat's orders from that beat; with none alive the joiner spectates (no demigod). Every demigod's public controller is human or ai. Rules 0.8.1: a takeover name that collides only with the SAME player's older seat (same display name) keeps the plain name (no \" (n)\" suffix; only a different player's collision is suffixed), and image identities ignore any \" (n)\" suffix so art matches the prewarmed images.",
    "The Dungeon Master's rulebook (rules 0.8.0, state schema 6; docs/DM_RULEBOOK.md, the single rules reference): the DM's core rules (the dice decide, the DM remembers, places nest, describe the world as the rules left it) join the resolver's and the gods' system prompts, with up to dm_memory_lines settled facts about the place, each act's d20 and the time of day; older schemas keep the old prompts. Every 0.8.0 rule is gated on schema 6; every 0.8.1 rule (challenge mode, the summon's damage check at the accept, heals to Nature, fallen wills, the takeover rename fix) on schema 7.",
    "Place x time memory (rules 0.8.0): every DM decision about a place at a beat is an entry (World.dm.memory, append-only): scope cell (a people- or house-level hex) -> settlement (the nearest city/site within 1.5 x its radius, at least 15 km / 0.5 km) -> region -> world, with its ancestors, a kind, an optional query key and the result. Lookups: what was decided about P (or an ancestor or descendant) at or before beat B (game.memory). INTEGRATE: each beat's facts roll up as one memory.rollup per ancestor touched (counts by kind, up to 3 notable events, the ancestor's state). COMPARE: a query already answered keeps its answer (a different one is a memory_conflict trace event, dropped); resolver proposals are checked against their act's recorded d20 (a natural 1 lands nothing on others, a fail at most 3 damage). EXTRAPOLATE: a hex's state (people, buildings, encounters, loot, shrines, road, weather, light, its parent's changes) is carried from its last place.observed record by its parent's changes since, seeded by cell and beats; the DM records observations of hexes where demigods arrive (and daily where they stand), each seat's zoom focus and every hex it decided about.",
    "The d20 (rules 0.8.0): seeded by the match seed and the query (act:<intent>, mf:<id>, encounter:<id>, fight:<beat>:<a>:<b>), recorded once and shown as a public dice.rolled outcome event {who, die, roll, modifier, total, target_dc, verdict, reason, at, cell}. A natural 1 is crit_fail, a natural 20 crit_success, else total vs DC. Resolver acts: DC 8 + magnitude/2, modifier +1 hale (health >= 7) / -1 hurt (<= 3) + wardrobe/2 (+2 on the Final Wish); rules-fallback strikes land 0 / 0.5x / 1x / 1.5x (max 7) by verdict, talk and threats the same factors; a harmless act's accident is its confirmed natural 1 ((roll - 1) + confirm < accident_scale). Manifestations: target_dc = 21 - round(20 x tier base odds), the modifier carries the other odds terms (so success <=> face >= 21 - round(20 x odds)); the confirm draw places the outcome inside the success or failure share; a natural 20 is an untwisted success, a natural 1 fails or backfires.",
    "Day and night (rules 0.8.0): beats_per_day (8) beats a day: the first eighth dawn, day to half, dusk to five eighths, then night; light 0.5 + 0.5 sin(2 pi (beat_in_day - 1) / beats_per_day). Each phase change is remembered. round.brief.public.time_of_day {day, phase, light} and time_of_day_phase.",
    "Growing settlements (rules 0.8.0): cities (planet) / sites (city) have a level, founded from people (<200k 1, <1M 2, <3M 3, <8M 4, else 5); each beat a target = people's level +1 prosperous (0.4 supply/1.5 + 0.3 hope + 0.3 (1 - danger) >= 0.62) -1 wretched (< 0.3) +1 building orders there in the last 8 beats +1 a shrine there or a demigod's allegiance >= 0.5, clamp 1-7; the level steps one toward a target held two beats running after dm_settlement_step_beats (6). settlement.grew / settlement.shrank events; settlement {level, size, center_cell, settlement_cells} on game.region / game.node nodes.",
    "Story pieces (rules 0.8.0): per demigod followers (its Following's headcount), items held ({ref, icon}), wardrobe stage from deeds never going down (0 plain, 3 marked, 8 adorned, 16 regalia, 30 mythic; deeds: up to 3 landed public acts of magnitude 5+ a beat, 1 per thing made, 1-3 per encounter won, 3 per shrine, 1 per loot taken) with a condition (scorched: hit by fire this beat; ragged: health <= 3), and scars {beat, cause, place, cell} for a hit of 1.5+ in one beat (12 kept). public_state.demigods[].story.",
    "Map encounters (rules 0.8.0): up to dm_encounters_per_beat (1) marked hexes a beat while fewer than dm_encounters_max (10) hide, 0.2-0.8 x dm_reach_km from a region's centre (where travel ends; one hidden per region; weight 3 where Time's circle converges, 2 a region with a hotspot, else 1), never a centre within reach of a standing demigod; monster, treasure, trap, stranger (night adds undead, ghost, thief); hidden as {id, cell, marked} until a standing demigod's move passes within dm_reach_km (30 km; city /100), then revealed forever and resolved by a d20 (DC monster 12, treasure 10, trap 13, stranger 11, undead 13, ghost 12, thief 12; modifier the demigod's, -1 monsters and traps at night, +1 near its own shrine): damage x dm_encounter_damage_scale, items, a stranger who becomes a named follower, sparks (tables in the rulebook). Unfound ones fade after dm_encounter_fade_beats (20).",
    "Loot left behind (rules 0.8.0): a death drops every held item on the fallen's hex; of two engaged demigods the one hurt worse (>= 1) rolls DC 11 to keep its newest item. From the next beat the standing demigod whose move passes closest within reach takes the pile (its dropper only after 3 beats): held, custody stolen (a rival) or transferred (a sibling). game.region / game.node loot [{ref, icon, cell}].",
    "Roads (rules 0.8.0): moves between settlements wear their path (a demigod 1, an army 1 + size/20,000 up to 3, a migration 0.5; ends within 400 km / 4 km of a settlement); at dm_road_wear (4) it becomes a road, remembered; unused paths lose dm_road_decay (0.05) a beat and a road below half the threshold fades. roads [{from_cell, to_cell, wear}] on game.region / game.node / game.globe.",
    "Shrines (rules 0.8.0): an order with a making verb and a shrine word, or a made structure, raises a shrine on the demigod's house-level hex (at most dm_shrines_per_demigod (3), one a beat); power by its words or seeded: spark (+1 spark every dm_shrine_spark_every (6) beats, one sooner per level above 1, min 2), heal (its player's demigods within 3 x reach heal dm_shrine_heal (0.15) x level a beat), ward (rivals within 3 x reach take dm_shrine_ward (0.1) x level); worshippers from crowds within 300 km (size x (0.001 + 0.004 devotion) x (1 + faith need), max 250k a beat) plus pilgrims (Following/1000, 5-500); level 1 below 1,000 then +1 per tenfold (max 5); abandoned shrines lose a tenth a beat. shrines [{cell, owner_seat, side, worshippers, power}] on game.region / game.node and public_state.",
    "Fault lines (rules 0.8.0, data/geo/PB2002_boundaries.json, compiled once into a 5-degree segment index): a MEGA cast's magnitude, radius and damage scale with its great-circle distance to the nearest plate boundary: within fault_near_km (150) + fault_near_magnitude (3), x fault_near_radius (1.5), x fault_near_damage (1.6); beyond fault_far_km (1500) + fault_far_magnitude (-2), x fault_far_radius (1.0), x fault_far_damage (0.6); linear in between. Minis are unaffected. The streak and the cast line carry fault_km and fault_factor.",
    "The Lens and the Engine (rules 0.8.0, the rulebook's governing principle): a place's history is a lens the DM looks through; the rules and dice are an engine that already decided and is never overruled; the DM makes the answer make mystical sense through Rube Goldberg causality. Its concise form joins the resolver's, the gods', the narrator's, the ramifications' and the summon mind's prompts (schema 6 only); the rules' encounter lines carry seeded flourishes rhyming with the region's memory.",
    "Summoning (rules 0.8.0): an instant fields.summon {demigod (one of yours), target ([lon, lat] or a hex key)}, any phase, when summon_enabled and not within summon_cooldown_beats (1) of the last; it lands at the next resolve, this beat: a rules Move row (the occupancy rule nudges it), its hands' travel this beat void and its standing orders ended. A visible d20 (summon:<beat>:<demigod>, no modifier, DC round(20p) + 1 with p = clamp(0.25 + 0.6 health/max, 0, 0.95)) decides a mishap costing round(0.2 x health) clamped 1..2 (x summon_loss_scale; none below 2.5 health). The summon mind (LLM, else seeded rules parts) tells the transit deed: a tale in its story and memory entries at both places (summon.departed, summoned: the public event {demigod, from {pos, cell}, to {pos, cell}, deed, health_lost}). Rules 0.8.1: the damage check comes before the teleport: the mishap d20 (query summon:<beat>:<demigod>:<lon>,<lat>) and health_lost are decided at the accept from the committed state and shown in the instant summoning push; the resolve recomputes them from the beat's starting health (the same values).",
    "God-tier powers (rules 0.8.0): one signature power per god (god_time, god_nature, god_consequence, god_death, god_chance, god_memory, god_entropy; tier god in the palette), cast as fields.cast {power, tier: god, target}: god_power_cost (3) bolts at the next resolve, every other player with a demigod standing gains god_power_gift (1) bolt, Entropy + entropy_per_mega. It becomes an invocation (magnitude 8) routed to that god's mind (verdict, price, precedence; its bolt price already paid); unless refused, its signature lands as the god's proposals: Time rewinds a unit 2 beats (or slows rival demigods within 1,500 km a beat), Nature 1.2 damage to rivals within 400 km, Consequence collects the target rival's biggest open debt, Death raises a fallen target or strikes a standing one for 3, Chance a seeded coin per demigod within 600 km (heal 1.5 or 1.5 damage), Memory resets every crowd's allegiance in the region, Entropy +15.",
    "Challenge mode (rules 0.8.1, state schema 7 and room setting challenge_enabled; older states never duel): an instant fields.challenge {demigod (yours, standing), target (a rival's, standing), level} locks both demigods in a duel when they stand within max(challenge_min_km (120), the touching-cards distance at the client's level: challenge_contact_km_regions 600 (also a missing or unknown level), _villages 15, _houses 0.15, _people 0.015) km (the level also frames the fight) and neither is already in a duel (and the challenger's last challenge is duel_cooldown_beats (1) back); refusals not_yours, fallen, bad_target, too_far:<distance km>:<allowed km>, busy, cooldown; no consent. on_instant shows challenge.lock {duel_id = duel:s<seq>:<a>:<b> (the challenge action's seq; the AI's duel:<beat>:<a>:<b>), a, b (demigod, name, health, portrait/diorama), at, focus {at, lat, lon, cell, level}, ends ko_or_deadline, ends_beat, deadline_ms, world beating}. Each typed line (fields.duel {duel_id, text <= 120}) is a blow resolved at once by rules (no LLM): a d20 seeded by the match seed and <duel_id>:<n> + modifier (wit of the line -3..+4: action verbs, body parts, items held, terrain and place words, the opponent's story and scars, minus repetition, echoes, short lines and gibberish; +1 hale (duel hp >= 7) / -1 hurt (<= 3); half the wardrobe stage + 1 with any item; +1 a quick follow-up within duel_followup_ms (1500); +1 on home turf; the formal model's situation) vs DC 10 (+1 at night, + half the opponent's wardrobe stage); damage 0 on a fail, 1 (+1 beating the DC by 5, +1 wit >= 3) on a success, 4 on a natural 20, shaped by the interaction matrix, x duel_damage_scale; a blow within duel_min_gap_ms (500) of the same side's last is a penalty (no die, 0.5 to its author). Against an AI, or a human who hasn't blown for duel_counter_ms (3000), each blow carries the opponent's automatic counter (wit capped +2). A duel ends at a knockout or its deadline (duel_deadline_ms, 30 s). The engine has no room-wide hold yet, so the world keeps beating: the duel is kept in the state (World.dm.duels) across resolves, each resolve applying the damage so far, the dice (dice[], reason a duel) and the invokes' bolts (round.outcome.public.duels_open[] challenge.open), the duellists locked in place; it ends at the first resolve after its deadline stamp or at beat ends_beat (lock beat + ceil(duel_deadline_ms / flow_ms) - 1), or at once when a duellist falls elsewhere or both are built-in AIs (fought automatically for duel_auto_rounds (2) exchanges if nobody typed). Every resolve recomputes the duels deterministically from the state and the ordered actions (a per-room live cache only numbers and displays blows). At the end: a knockout eliminates by the usual rules, the loser's loot drops on the duel's hex (everything at a knockout; else a DC 11 keep-hold roll when beaten by 1+), duel.lock and duel.end are remembered at the hex, both get a tale, and round.outcome.public.duels[] carries challenge.end {duel_id, reason ko|deadline|fallen|auto, winner, loser, ko, damage {a, b}, blows, typed, deed, log}, the deed one line from the duel mind (LLM when a human fought, else seeded rules parts). The built-in AI challenges its nearest rival with chance duel_ai_chance (0.02) per beat, decided at the round's start (round.started.public.challenges): within 120 km directly, else within duel_ai_reach_km (2000) after summoning itself next to it (a leap: the summoning's mishap d20 and health loss apply). The formal model (DM_RULEBOOK 21, \"the flips\"): range (far/mid/close/grapple), initiative and stances (neutral, guarding, committed, off_balance, airborne, downed) carry between blows; each line has a move class by rules (strike, heavy, guard, parry, feint, evade, close, retreat, grapple, throw, invoke (a bolt, any range), command (needs an army), taunt), gated by range, bolts and armies; the d20 adds initiative +2, the stances and openings; the interaction matrix decides what a success means (a parried strike leaves the attacker off balance and flips the initiative; a heavy breaks guards and parries but commits its user; a feint wastes a set defence and flips; an evade beats a heavy; grapple at close, throw from a grapple); every strike carries choreo {range and initiative before/after, flip, result, each figure's move class, timed poses from a fixed 30-pose vocabulary, contact, stance after, contacts[], camera, 600-1400 ms}. Rules decide; an LLM only phrases the deed.",
    "AI duellists type (state schema 12; nothing of it in an older state): in a duel with a clock an AI fighter types a line of its own in every input window (room setting duel_ai_lines_per_window, 1; 0 = off): at the window's opening (the lock, then each close) the rules pick its move class (range, initiative, stances, the opponent's set defence and last move; duel_ai_aggression 0.6) and a seeded sentence of 20 to 70 characters for it, published as duel.typing_script {duel_id, by, demigod, window, text, len, starts_at_ms, cps 6..9 seeded per demigod, key_ms, pauses[], ends_at_ms (>= 400 ms before input_closes_ms), input_closes_ms} in also[], on the panel that opens the window (a|b.typing_script) and in present.duels[].typing_scripts; that panel's play_ms grows to fit the line. At ends_at_ms the line is a duel.blow (ai true, window, at_ms = ends_at_ms) by the same d20 and matrix, wit capped at 2, minus duel_ai_handicap (5) against a human (waived when the human entered no line for 12 s, and between two AIs); it is entered when the first duel action stamped at or after it arrives or when its window closes. An AI that typed in a window does not also counter automatically in it and its line draws no automatic counter. Two AIs with a clock type their duel out to a knockout or the deadline instead of ending at once. Words to moves: a line's words pick its move class and its clip (duel.blow.move {clip, word, setup[], path}), within one or two edits in commands of at most four words; the last move word is the move, earlier ones are setups, an invoke or command wins wherever it stands; a throw works from close range; 15 new clips with fixed fallbacks; idle time in panels is steps, shuffles and guard, hit reactions go by damage. One duel per player (duel_one_per_player): a challenge by or to any demigod of a player with a demigod in an open duel is refused busy:player and the AI's chooser skips it. A duel closes at the first duel action or resolve stamped past its deadline: the panels left, the outro and a provisional challenge.end are that action's event; the resolve applies the result as before.",
    "Exact resume (state schema 8; docs/TIME_TRAVEL.md): an open duel keeps every blow so far in the state (dm.duels[].blows, with each blow's receive stamp and seq); every resolve stamps the delivery mark (dm.mark: the newest instant action it was handed, its newest receive stamp); challenge and duel input is ordered by (received_at_ms, seq); a player's duel id that already names a duel of the match steps by 1000000. The plugin journals every accepted instant action (memory; on the server also a file) and shows the present (public_state.present, game.present: the server's clock, the events since the last resolve, every open duel in full) by applying the pending ones to the state with the resolve's own function. On the server each Start reads the clock (dm.mark.round_ms; an AI's challenge takes it as its lock stamp, so it has a deadline) and the pause since the last Commit, the last stamp before a restart, or the fork: a pause of 2 s or more moves every open duel's deadline and is listed in its pauses (round.started.public.duel_clock); the first resolve after a restart is handed the journaled instant actions the dead process never delivered, in arrival order, before the batch's own. Both side inputs are recorded (events duel_clock, journal_recovered) and fed back on replay.",
    "Duel panels (rules 0.8.2, state schema 9; crate panel, client design duel_lab/): panels present the blows, they decide nothing (every typed line is still a 0.8.1 blow resolved at once). At the lock the bout gets its scene (duel.scene: a rules fallback seeded by the match seed and the duel's hex: 6-10 objects of known kinds (pillar, wall, crate, table, boulder, rock, tree, log, jar, brazier, altar, chest, bones, cairn) picked by the place's words, the settlement's level, night, a shrine or loot on the hex and the DM's newest memories of it; each with hex position, primitive parts, material and the paths it affords) and panel 0 (starts at the lock's stamp, 2600 ms). A panel plays play_ms (2400 idle, 600 + 1000 per staged strike up to 3: 2400-3600); its input closes 700 ms before it ends; the blows stamped up to that close make the next panel, which starts when this one ends: per strike a run-in to the range the matrix left, the class's clip, the defender's reaction by the matrix's result (stagger, knock, block, parry, an evasion over, under or off a scene object that affords it), a contact on the foe (or the scenery on a miss); a fighter with no strike in the window circles (holding the initiative, when nobody struck) or guards; caption = the decisive blow's flavor line; hp, range, initiative after. A knockout's panel is followed at once by the outro; so is the first slot at or past the deadline. The cycle advances on stamps only: a duel line or a fields.duel_tick {duel_id} from a duellist (instant; up to 150 ms early closes the window at the tick; refusals panels_off, no_duel, not_yours) closes every window its stamp has passed, each resolve does the same with its batch's newest stamp, and a duel a resolve ends plays out to its outro (challenge.end.panels). A pause moves the schedule with the deadline.",
    "Healing, routing and no vanishing intents (rules 0.8.1, the match 79fd73bf and f18d76a3 fixes; gated on state schema 7): a heal (heal, restore, mend, cure...) is routed to Nature (Death when a target has fallen), ruled on with its normal price, and unless refused heals each target 1 + magnitude/4 (1-3, half when twisted; Death raises the fallen); rival demigods are dropped from a heal unless the order names them, and \"me\" / \"my demigods\" / \"all my demigods\" (or no demigod named) mean the player's own; the hand's prompt marks the player's own demigods (yours). A god-routed intent naming no god in the match goes to Entropy (a visible ruling), never silently to the world. A fallen player's orders (all their demigods down) are never dropped: a plea to come back (revive, come back, back to life, raise, resurrect...) becomes an invocation of Death (verb revive, magnitude 6, by their first demigod, for all their fallen demigods) ruled like any other (Death's price; without the bolts Death twists or refuses), any other order is refused; either way the player's round.private carries wills[] {text, routed_to, verdict, speech, reason}.",
    "Wills that matter (rules 0.9.0; room setting wills_enabled on a schema-7+ state with the 0.8.1 rules; older settings read it false and replay unchanged; DM_RULEBOOK 23-27; the state is World.dm.wills: quests, curses, raises, boons, spent item powers). A WILL SPLITS INTO ITS PARTS: heal, bless, arm, raise, curse (or seal), ask, offer, gather; the hand mind is asked for one intent per part and the rules read the player's words again and add any part it dropped (so the LLM path and the rules fallback agree); each part x each target is resolved by rules and dice before the route (the resolver and the god minds never see it), with its own ruling (rules-written, fallback true), and is never a standing order. PAYING: prices are in sparks; sparks first, then a bolt breaks into will_spark_per_bolt (5) sparks (the change stays in the purse); what the purse can't pay is lent by the ruling god (will_debt_per_spark 0.2 debt per spark) while the actor's open debt stays under will_debt_cap (3), else the effect shrinks or becomes a quest; a silence curse doubles prices; Entropy rises will_entropy_per_spark (0.25) per spark paid. HEAL / BLESS (Nature): price heal_price_sparks (1) + magnitude/4 sparks per target; d20 (query will:<intent>:<part>:<demigod>) vs DC will_own_dc (6) + magnitude/4 for your own (8 + magnitude/2 for a named rival), modifiers named (wardrobe, charm, boon, hex, the god's favor +-1 at +-2, paid in full +1 / on credit -1 / part paid -1, the Final Wish +2); heal = (1 + magnitude/4, 1-3.5) x heal_scale x (0.5 + 0.5 x share paid); crit: x1.5 and a boon (boon_bonus +1 on its d20s for boon_beats 6); success: the heal; fail: heal_token (0.5) now and a mystic quest for the rest; natural 1: a fizzle and the quest; nothing paid and nothing lent: the quest alone, never silence. A blessing is a boon (crit +1 more and half again as long; fail: 2 beats and a quest). ARM: gear for each of your own demigods, kind and name from the words (\"rocket launchers\" -> a weapon, the Rocket Launcher) else the slot it lacks (weapon, armour, charm), a seeded material; tier asked from the magnitude (0-3 trinket, 4-6 artifact, 7+ relic), prices 0 / arm_price_artifact (2) / arm_price_relic (5) sparks, a god's blessing (a god named, or god power / holy / divine / blessed) arm_bless_sparks (5) more: a tier up and that god's signature; d20 vs will_own_dc + 2 x tier; crit: a tier up; success: as paid; fail: a tier low plus a quest that tempers it; natural 1: nothing plus a quest that forges it. The gear is an item (an mf: row, Effect::Manifest). ARMS DO THINGS (wills::arms; nothing stored but spent powers): by slot and tier (trinket/artifact/relic/wonder) a weapon gives +1/+1/+2/+2 on strike d20s, +0/0.5/0.5/1 damage per landed strike, +0/1/1/2 in duels, Smite from relic; armour turns aside 0.25/0.5/0.75/1 of each hit (never more than half), halves its material's damage type from artifact, +0/0/1/1 in duels, Last Stand from relic; a charm (relic or artifact) gives +0/1/1/2 on other deeds and mends 0/0.1/0.15/0.2 a beat, Second Wind from relic; materials: obsidian and dragonscale (fire), butterfly-wing (storm, +1 journeys), platinum (curses), struck-oak (lightning), moonsteel (cold), whalebone (water), salt-glass (poison), gold (blades), heron-feather (falls, +1 journeys); signatures: Time +2 journeys, Nature +0.2 mending, Consequence thorns 0.5, Death Last Stand, Chance +1 on every d20, Memory +1 in duels, Entropy +0.5 damage; best item per slot, capped at +3 d20, +1.5 damage, 1.5 guard, 0.4 mending, +3 duel, all x arms_scale; once-per-match powers: Smite (+2 on the first strike landed on a rival demigod), Last Stand (the first killing blow leaves 1 health; checked after the merge and the society, before the DM's step), Second Wind (heals 2 the first beat ended at 3 or less); applied to act d20s (dm::resolver_dice), every damage / heal / revive proposal entering a merge (merge::run -> arms::shape), the duel's gear term, and the beat's upkeep; shown in item payloads (slot, material, mods, power, blessed_by, does) and on the piece's story pegs. RESURRECTION (Death): anyone may plead for any fallen demigod, a wiped player included (the fallen may only plead, ask and offer); price raise_price_sparks (5) x (1 + times raised before) + 1 per raise_stale_beats (8) dead, and raise_years (1) x (1 + times raised) of max health; paid in full (or lent): it rises at once at raise_health_fraction (0.4) of its max, where it fell or at its shrine (demigod.raised); it can't be paid: a public quest (difficulty 2 + raised before + another player's, max 3; the fallen keep vigils), and a quest-raised demigod owes Death the sparks as a debt; raise_cap (3) raisings per player per match; a seal refuses it. SEALS AND CURSES (wills::curses): seal (no resurrection; on the fallen or the living), wither (no healing reaches it), silence (its player's wills cost double), bind (can't be summoned: bound:<quest>), hex (minus its strength on its d20s); curse_price_sparks (5), the god-tier variant (a god named, eternal, forever) curse_god_price_sparks (15) through Death (seal, wither), Memory (silence), Time (bind), Chance (hex): strength 3, until countered; paid whole or not cast; d20 vs curse_dc (10) + half the target's wardrobe (+2 against platinum gear): crit strength +1 and half again as long, success strength 2 for curse_beats (12; a seal until countered), fail strength 1 for half, natural 1 it lies on its caster; curse_max_per_player (2) standing; Entropy + curse_entropy (3) (god-tier: entropy_per_mega); public face = the mark on the target (kind, strength, until), never the owner; the bearer's and the owner's seats see the owner and the counter; ALWAYS a counter: a mystic quest given to the bearer the moment it is cast (as many steps as its strength; a seal's is public). MYSTIC QUESTS (wills::quests): 1-3 steps by rules from seeded tables and the place x time memory, told in Rube Goldberg style; step kinds the engine verifies each beat after the DM's step: at (a shrine of the curser or your own, a settlement of level >= N, within 150 km of a fault line, a coast, a remembered spot; optionally at dawn / day / dusk / night; optionally burning an ingredient held there), hold (an item whose name carries the ingredient's word: gather it by will, or win an encounter), encounter (win one), shrine (raise one), duel (survive one), sparks (offer quest_spark_step x difficulty by will), army (a general's army within 300 km of a place), vigil (the hour comes N times); in order, several a beat; quest.given / quest.step / quest.done / quest.failed are remembered (public for raisings and seals' counters, else for the seats in data.seats: round.private.will_events); heal and arming quests lapse after quest_expire_beats (24), a counter lives as long as its curse, a raising until it is done; on completion the effect applies (the heal completes, the gear is tempered or forged, the curse breaks, the demigod rises). TELLING: round.private.wills[] {text, by, routed_to, gods, verdict, speech, reason, parts[] {part, target, god, roll {roll, modifier, total, target_dc, verdict, mods[]}, paid {price_sparks, sparks, bolts_broken, short_sparks, debt}, outcome, result, why, next, quest, item}}, round.private quests / curses / boons / raises / will_events and items on its demigods; public wills[] one-liners, curses[] and quests[]; curses on every public demigod. The built-in AI (wills_ai_chance 0.25, its own seeded stream) heals when hurt, arms itself, pleads for its fallen sibling, curses a near rival and pursues its quests.",
    "Gear looks (state schema 11 and room setting looks_enabled; older states and settings read it off and replay unchanged; docs/MEDIA.md Looks; the state is World.dm.looks: a look record per demigod and the item art granted). LOADOUT: a demigod's notable gear now, in a stable order: its best item per slot (weapon, armour, charm: the higher tier, the newer of equals), then up to look_extra_items (2) other major items (relic tier and up or god-blessed, highest tier first; then the first item of each kind not listed); items lying in a loot pile and destroyed things never count. Its hash is sha256 over fn7-loadout/1 and one line role|kind|material|tier|name stem|blessing per item (never an item id), so the same gear is the same hash in every match. MAJOR ACQUISITION: at the end of each beat (after the DM's step and the gear's upkeep) every demigod's loadout hash is noted; a human-controlled, standing demigod whose loadout is not the one its images show takes a new look: portrait and diorama subjects = its identity subjects unchanged plus a gear clause for the first look_items_shown (3) items (place on the body, a concrete noun, the material's look, a glow by tier, the blessing god's signature; no numbers); at most look_images_per_demigod (4) new gear pictures per demigod per match and one per look_debounce_beats (3) beats (a change inside the wait or past the cap is announced with kept debounced / capped; when the wait ends one picture is made of the newest loadout); a look worn before and the base look (no gear) are free; a fallen demigod keeps its look; the built-in AI's and bot seats' demigods never get one, and a human who takes a seat over counts from the end of that beat. Each change is remembered (look.changed, public, query look:<demigod>:<beat>) and sent in round.outcome.public.looks and dm_events. ITEM ART: at the end of each beat, in item order, up to item_art_per_match (12): an item a human player's demigod holds that is relic tier or up, god-blessed or the first of its kind its player holds, and a relic-tier or blessed item in a loot pile, is granted a picture keyed by its descriptor (sha256 of fn7-item-art/1 and kind|material|tier|name stem|blessing); only granted items carry image_prompt (written from the descriptor alone), the rest keep their icon. ARMING WORDS (schema 11): kit words (equipment, gear, armaments, weaponry, arsenal) and needing or wanting a piece of gear are an arming of your own demigods too (the recorded \"my demigods need crazy powerful equipment\" armed nobody under 0.9.0). The plugin never calls the image service: the server draws a gear picture on the demigod's base image (first reference) and shows the previous look while it renders.",
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
            "resolver_system": if w.dm_rules() {
                fn7_agents::resolver::system_prompt_dm()
            } else {
                fn7_agents::resolver::system_prompt()
            },
            "dm_core (rules 0.8.0, schema 6: in the resolver's and the gods' system prompts)": fn7_agents::rulebook::DM_CORE,
            "lens (rules 0.8.0, schema 6: in the resolver's, gods', narrator's, ramify and summon prompts)": fn7_agents::rulebook::LENS,
            "summon_system (rules 0.8.0)": fn7_agents::summon::system_prompt(),
            "duel_system (rules 0.8.1)": fn7_agents::duel::system_prompt(),
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
        player: None,
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
        own: vec![],
        rules_081: false,
        wills: false,
        earth: vec![],
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
        dm: false,
        dm_memory: vec![],
        earth: vec![],
    })
}

fn sample_step_prompt(kind: fn7_agents::step::StepKind) -> String {
    fn7_agents::step::system_prompt(&fn7_agents::step::StepCtx {
        kind,
        agent: AgentId(0),
        name: "<NAME>".into(),
        persona: "<PERSONA>".into(),
        player: None,
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
        earth: vec![],
    })
}

/// The Final Night 7 plugin. Holds the pantheon (data, compiled in) and nothing else: everything Time's bill needs
/// comes from the core (`RoundContext`, also passed to `fallback_outcome`), and the core runs every hook, sync ones
/// included, inside the match's trace context (theatre 1741d8f), so the plugin keeps no memory between hooks.
pub struct FinalNight7Plugin {
    pantheon: Arc<Pantheon>,
    /// Rules 0.8.1: the duels shown live this beat (display only; the resolve recomputes them; see [`duel`]).
    live: duel::LiveCache,
    /// Schema 8 (exact resume): the accepted instant actions of each match since its last resolve ([`present`]).
    journal: Arc<present::Journal>,
    /// Live typing in duels: each duellist's current line length, display only ([`typing`]).
    typing: typing::Typing,
    /// The side inputs of a recorded match (replay verification), instead of the journal and the clock.
    feed: Option<present::Feed>,
    /// The server's media style config (`{version, hash, source, body}`), recorded in every match's ruleset as
    /// `media_style` (the plugin never reads it otherwise; images are the server's).
    media_style: Option<Value>,
    /// Schema 14: the duel picker ([`picker`]; the server's LLM), and where the duel data rows go.
    picker: Option<Arc<picker::Picker>>,
    duel_data: Option<Arc<dyn picker::DuelData>>,
}

impl Default for FinalNight7Plugin {
    fn default() -> Self {
        Self {
            pantheon: Arc::new(fn7_pantheon::load()),
            live: Default::default(),
            journal: Default::default(),
            typing: Default::default(),
            feed: None,
            media_style: None,
            picker: None,
            duel_data: None,
        }
    }
}

impl FinalNight7Plugin {
    pub fn new() -> Self {
        Self::default()
    }

    /// The server's mode: every accepted instant action is appended to `<dir>/<match>.jsonl` and read back after a
    /// restart (the undelivered ones are handed to the first resolve), and each beat's Start reads the server's clock
    /// (pauses stop open duels' clocks). Both are recorded as events so a replay reproduces them ([`present`]).
    pub fn with_journal(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.journal = Arc::new(present::Journal::durable(dir.into()));
        self
    }

    /// Schema 14: pick duel lines with `llm` ([`picker`]; rooms with `duel_picker: on` and the LLM on), caching
    /// picks in (and writing duel data rows to) `data`.
    pub fn with_picker(mut self, llm: Arc<dyn LlmProvider>, data: Option<Arc<dyn picker::DuelData>>) -> Self {
        self.picker = Some(Arc::new(picker::Picker::new(llm, data.clone())));
        if data.is_some() {
            self.duel_data = data;
        }
        self
    }

    /// Schema 14: where each ended duel's `duel_line` rows go (the server's SQLite store).
    pub fn with_duel_data(mut self, data: Arc<dyn picker::DuelData>) -> Self {
        self.duel_data = Some(data);
        self
    }

    /// The duel picker, when the server gave one.
    pub fn picker(&self) -> Option<&Arc<picker::Picker>> {
        self.picker.as_ref()
    }

    /// The journal (tests: wait for a pick to come back).
    pub fn journal(&self) -> &Arc<present::Journal> {
        &self.journal
    }

    /// Schema 14, a duel line arriving: a cached pick is journaled now, before the line (it may set the class);
    /// returns the request for a pick to make once the line is accepted, when there is none.
    fn pick_before(&self, w: &World, seat: Seat, action: &Action) -> Option<(fn7_agents::duel_pick::PickCtx, picker::PickRec)> {
        let p = self.picker.as_ref()?;
        let st = world::settings(w);
        if !w.duel_weapon_rules() || st.duel_picker != "on" || !st.llm {
            return None;
        }
        let i = actions::parse(&action.text, &action.fields).ok()?;
        let b = i.blow?;
        let lv = self.journal.live(w, &st);
        let bout = lv.bouts.iter().find(|x| x.duel.id == b.duel_id && x.weapons)?;
        let side = bout.duel.side_of(seat)?;
        let range = serde_json::to_value(bout.flow.range).ok()?.as_str()?.to_string();
        let key = picker::key(&b.text, &range, weapons::class_of_side(bout, side), &moves::list_version());
        let mut rec = picker::PickRec { duel_id: b.duel_id.clone(), line_seq: action.seq, line_at_ms: action.received_at_ms, pre: false,
                                        at_ms: action.received_at_ms, pick: None, cached: false, latency_ms: 0, tokens_in: 0, tokens_out: 0,
                                        cost_usd: 0.0, model: fn7_agents::duel_pick::MODEL.into(), error: None, key: key.clone() };
        if let Some(pick) = p.cached(&key) {
            rec.pre = true;
            rec.cached = true;
            rec.pick = Some(pick);
            self.journal.note_pick(&present::Journal::key_of(w), w.meta.seed, w.meta.beat, seat, &rec);
            return None;
        }
        Some((duel::pick_ctx(bout, side, &b.text), rec))
    }

    /// Replay verification: take the side inputs (`duel_clock`, `journal_recovered`) from a recording.
    pub fn with_feed(mut self, feed: present::Feed) -> Self {
        self.feed = Some(feed);
        self
    }

    /// `present` (the journal's view) with each open duel's live typing lengths.
    fn present(&self, w: &World) -> Value {
        let mut v = self.journal.present(w);
        self.typing.fill(w.meta.seed, &mut v);
        v
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
    fn turns(w: &World, round: u32, actions: &[Action]) -> (Vec<beat::Turn>, Vec<flow::Sub>, Vec<beat::FallenWill>) {
        let mut out = vec![];
        let mut subs = vec![];
        let mut wills = vec![];
        let fixed = world::settings(w).rules_0_8_1;
        for s in &w.seats {
            // The seat's lead demigod (its first standing one; schema 4: its only one) answers for the seat's orders
            // and casts; a player is out only when every demigod of theirs has fallen.
            let Some(lead) = w.lead_of_seat(s.seat).filter(|_| w.seat_alive(s.seat)) else {
                // Rules 0.8.1: a fallen player's WILL never vanishes: their orders get a visible ruling (a plea to
                // Death to come back, or a refusal with the reason; `beat::FallenWill`).
                if fixed && s.seat < SYNTHETIC_SEAT && w.bodies_of_seat(s.seat).next().is_some() {
                    for a in actions.iter().filter(|a| a.seat == s.seat && !a.defaulted) {
                        let Ok(i) = actions::parse(&a.text, &a.fields) else { continue };
                        if i.has_orders() && !i.is_duel() {
                            wills.push(beat::FallenWill { seat: s.seat, text: join(&i.left, &i.right), seq: a.seq });
                        }
                    }
                }
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
                    // Rules 0.8.1: challenges and duel lines are fought apart (`duel::acts_of`).
                    if i.is_duel() {
                        continue;
                    }
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
                    // Rules 0.8.0: summonings and god-tier powers ride on the lead's turn, in arrival order.
                    for mut x in i.summons {
                        x.seq = a.seq;
                        input.summons.push(x);
                    }
                    for mut g in i.god_powers {
                        g.seq = a.seq;
                        input.god_powers.push(g);
                    }
                }
                input
            };
            out.extend(beat::split_turn(w, s.seat, d.id, input));
        }
        (out, subs, wills)
    }

    async fn resolve(
        &self,
        w: &mut World,
        round: u32,
        actions: &[Action],
        ctx: &RoundContext,
        llm: Option<&dyn LlmProvider>,
    ) -> RoundOutcome {
        // Typing relays (`fields.duel_keys`) were shown when they arrived and are nothing to a resolve: without them
        // the batch is exactly the batch of a match nobody typed live in.
        let kept = typing::strip(actions);
        let actions = kept.as_deref().unwrap_or(actions);
        // Schema 8: the instant actions an earlier boot of the server accepted and never delivered come first (a
        // recorded side input: `journal_recovered`).
        let merged;
        let actions = if w.resume_rules() {
            let recovered = match &self.feed {
                Some(f) => f.recovered.get(&round).cloned().unwrap_or_default(),
                None => self.journal.recovered(w, actions),
            };
            if !recovered.is_empty() {
                world::event("journal_recovered", None, json!({ "beat": round, "actions": recovered }));
            }
            merged = present::merge(actions, recovered);
            &merged[..]
        } else {
            actions
        };
        let (turns, subs, wills) = Self::turns(w, round, actions);
        let mut duels = duel::acts_of(actions, w.resume_rules());
        // Schema 14: the duel picks journaled by the batch's newest stamp (a recorded side input: `duel_picks`).
        if w.duel_weapon_rules() {
            let picks = match &self.feed {
                Some(f) => f.picks.get(&round).cloned().unwrap_or_default(),
                None => self.journal.picks(w, duels.newest),
            };
            if !picks.is_empty() {
                world::event("duel_picks", None, json!({ "beat": round, "picks": picks }));
            }
            duels.add_picks(&picks);
        }
        let out = beat::resolve(w, round, turns, &subs, &duels, &wills, &ctx.windows_closed, llm, &self.pantheon).await;
        // Schema 14: the ended duels' line rows to the server's store (never on a replay: it has no store).
        if let (Some(d), false) = (&self.duel_data, out.lines.is_empty()) {
            let run = theatre_trace::current().map(|c| c.run).unwrap_or_else(|| format!("seed-{}", w.meta.seed));
            for row in &out.lines {
                d.line(&run, row);
            }
        }
        if w.resume_rules() {
            // The delivery mark: the newest instant action this state contains, and the resolve's clock.
            let mut m = w.dm.mark.unwrap_or_default();
            if let Some((at_ms, seq)) = duels.newest.filter(|n| *n > (m.at_ms, m.seq)) {
                (m.at_ms, m.seq) = (at_ms, seq);
            }
            m.resolved_ms = duels.now_ms.max(m.resolved_ms);
            w.dm.mark = Some(m);
            self.journal.seen(w);
        }
        RoundOutcome {
            public: out.public,
            per_seat: out.per_seat,
            choices: vec![],
            state: world::save(w),
        }
    }

    /// The public event an accepted instant action makes at once (`on_instant`).
    fn instant_event(&self, w: &World, seat: Seat, action: &Action) -> Option<Value> {
        let s = world::seat_state(w, seat)?;
        let i = actions::parse(&action.text, &action.fields).ok()?;
        // Rules 0.8.1: a challenge locks two demigods in (`challenge.lock`); a duel line is a blow (`duel.blow`).
        if i.is_duel() {
            return if w.resume_rules() { duel::on_instant_now(&self.journal, w, seat, action, &i) } else { duel::on_instant(&self.live, w, seat, action, &i) };
        }
        // Rules 0.8.0: a summoning or a god-tier power is shown at once too (the client flashes; it lands at the
        // next resolve).
        if i.casts.is_empty() && w.dm_rules() && (!i.summons.is_empty() || !i.god_powers.is_empty()) {
            let lead = w.lead_of_seat(s.seat).unwrap_or(s.demigod);
            let first = i.summons.first();
            let v = json!({
                "event": if i.summons.is_empty() { "god_power" } else { "summoning" },
                // The client's contract: {demigod, from, to, by_seat} of the (first) summoning.
                "demigod": first.map(|x| Ref::Demigod(x.demigod)),
                "from": first.and_then(|x| w.demigods.get(x.demigod.ix())).map(|d| d.pos),
                "to": first.map(|x| x.target),
                "by": w.demigods[lead.ix()].player_name(), "by_ref": Ref::Demigod(lead), "by_seat": seat,
                "summons": i.summons.iter().map(|x| json!({ "demigod": Ref::Demigod(x.demigod), "to": x.target,
                    "cell": x.cell.clone().unwrap_or_else(|| people_cell(x.target)) })).collect::<Vec<_>>(),
                "god_powers": i.god_powers.iter().map(|g| json!({ "power": format!("god_{}", g.god), "tier": "god" })).collect::<Vec<_>>(),
                "seq": action.seq, "received_at_ms": action.received_at_ms,
            });
            let mut v = v;
            // Rules 0.8.1: the damage check comes before the teleport: the mishap d20 and the health it costs are
            // decided now (the resolve applies exactly these), and the summoning is noted for challenge mode (a
            // challenge right after it fights from the destination, at the reduced health).
            let st = world::settings(w);
            if st.rules_0_8_1 {
                let mut noted = vec![];
                for (k, x) in i.summons.iter().enumerate() {
                    let health = w.demigods.get(x.demigod.ix()).map(|d| d.health).unwrap_or(0.0);
                    let (_, roll, lost) = summon::mishap(w, w.meta.beat, x, health, &st);
                    let lost = (lost as f64 * 10.0).round() / 10.0;
                    let cue = json!({ "roll": roll.roll, "modifier": roll.modifier, "total": roll.total,
                                      "target_dc": roll.target_dc, "verdict": roll.verdict, "health_lost": lost });
                    if k == 0 {
                        if let (Some(o), Value::Object(c)) = (v.as_object_mut(), cue.clone()) {
                            o.extend(c);
                        }
                    }
                    if let Some(o) = v["summons"][k].as_object_mut() {
                        if let Value::Object(c) = cue {
                            o.extend(c);
                        }
                    }
                    if summon::check(w, s.seat, x, w.meta.beat + 1, &st).is_ok() {
                        noted.push(duel::Moved { demigod: x.demigod, to: x.target, lost: lost as f32 });
                    }
                }
                if !w.resume_rules() {
                    duel::note_summons(&self.live, w, noted);
                }
            }
            world::event("summon_streak", Some(Ref::Demigod(lead).to_string()), v.clone());
            return Some(v);
        }
        if i.casts.is_empty() {
            return None;
        }
        let mut v = casts::streak(w, w.lead_of_seat(s.seat).unwrap_or(s.demigod), seat, &i.casts)?;
        v["seq"] = action.seq.into();
        v["received_at_ms"] = action.received_at_ms.into();
        world::event("cast_streak", Some(Ref::Demigod(s.demigod).to_string()), v.clone());
        Some(v)
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
        if name == "present" {
            // The one query that needs the plugin's journal (and the whole state: ask it on (re)connect, don't poll it).
            let w = world::load(state).map_err(|e| e.to_string())?;
            return Ok(if w.resume_rules() { self.present(&w) } else { Value::Null });
        }
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
        // Schema 14: the duel move list this server reads lines with (in full when it is not the compiled one: a
        // replay reads the match with it) and the picker's prompt.
        if w.duel_weapon_rules() {
            body["duel_moves"] = moves::current().ruleset_json();
            body["prompts"]["duel_pick_system (schema 14)"] = fn7_agents::duel_pick::system_prompt().into();
        }
        theatre_trace::ruleset(PLUGIN_ID, RULES_VERSION, RULES_LABEL, body);
        self.journal.seen(&w);
        Ok(world::save(&w))
    }

    async fn on_round_start(
        &self,
        state: &Value,
        round: u32,
        _llm: &dyn LlmProvider,
    ) -> anyhow::Result<RoundStart> {
        let mut w = world::load(state)?;
        let mut rs = phases::round_start(&mut w, round, &self.pantheon);
        // Rules 0.8.1: the built-in AI's challenges this beat, announced so a human can fight back.
        // Schema 8: the beat's clock and the pause before it (side inputs, recorded as `duel_clock`).
        if w.resume_rules() {
            let clock = match &self.feed {
                Some(f) => f.clocks.get(&round).copied(),
                None => self.journal.round_start(&w),
            };
            if let Some(c) = clock {
                world::event("duel_clock", None, serde_json::to_value(c).unwrap_or(Value::Null));
                let v = present::apply_clock(&mut w, &c);
                if c.pause.is_some() {
                    rs.public["duel_clock"] = v;
                }
                rs.state = world::save(&w);
            }
        }
        if w.resume_rules() {
            self.journal.seen(&w);
        }
        let locks = if w.resume_rules() { duel::round_start_now(&self.journal, &w) } else { duel::round_start(&self.live, &w) };
        if !locks.is_empty() {
            rs.public["challenges"] = locks.into();
        }
        Ok(rs)
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
        // Schema 14: picks are the server's own journal entries, never a client's.
        if i.pick.is_some() {
            return Err("duel_pick is not an action".into());
        }
        // Rules 0.8.1: a challenge or a duel line (instant only; checked against the live duels).
        if i.is_duel() {
            if kind != SubmitKind::Instant {
                return Err("a challenge or a duel line is an instant action".into());
            }
            if let Some(k) = &i.keys {
                return typing::validate(&self.journal, &w, seat, k);
            }
            return if w.resume_rules() { duel::validate_now(&self.journal, &w, seat, &i) } else { duel::validate(&self.live, &w, seat, &i) };
        }
        if kind == SubmitKind::Instant && i.has_orders() {
            // Enforced on arrival (the core replies accepted:false); resolve keeps a safety net (`orders_ignored`).
            return Err("instant actions carry casts, not orders: send orders as text entry".into());
        }
        let Some(s) = world::seat_state(&w, seat) else {
            return Ok(());
        };
        let d = &w.demigods[w.lead_of_seat(s.seat).unwrap_or(s.demigod).ix()];
        // Rules 0.8.0: summonings and god-tier powers.
        if !i.summons.is_empty() || !i.god_powers.is_empty() {
            if !w.dm_rules() {
                return Err("summoning and god-tier powers need rules 0.8.0".into());
            }
            let st = world::settings(&w);
            for x in &i.summons {
                crate::summon::check(&w, s.seat, x, w.meta.beat + 1, &st)?;
            }
            if !i.god_powers.is_empty() {
                let need = st.god_power_cost * i.god_powers.len() as u32
                    + i.casts.iter().filter(|c| casts::scale_of(&w, c) == Some(CastScale::Mega)).count() as u32;
                if !d.alive {
                    return Err("your demigods have fallen: they cast nothing".into());
                }
                if need > d.bolts {
                    return Err(format!("not enough bolts: a god-tier power costs {}, you have {}", st.god_power_cost, d.bolts));
                }
            }
        }
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
        // Live typing: relayed and forgotten (not journaled: `present` never replays keystrokes).
        if typing::is_keys(action) {
            return self.typing.relay(&self.journal, &w, seat, action);
        }
        // Schema 14: a cached pick goes into the journal before the line; otherwise one is asked for once it is in.
        let ask = self.pick_before(&w, seat, action);
        let v = self.instant_event(&w, seat, action);
        if let (Some((ctx, rec)), Some(p)) = (ask, &self.picker) {
            let accepted = v.as_ref().is_some_and(|v| v["event"] == "duel.blow" && v["seq"] == action.seq && v["duel_id"] == json!(rec.duel_id));
            if accepted {
                let st = world::settings(&w);
                let (journal, key, seed, beat) = (self.journal.clone(), present::Journal::key_of(&w), w.meta.seed, w.meta.beat);
                p.spawn(moves::current().body.clone(), ctx, rec, std::time::Duration::from_millis(st.duel_picker_timeout_ms as u64), move |r| {
                    journal.note_pick(&key, seed, beat, seat, &r);
                });
            }
        }
        // Schema 14 (`duel_picker_for_ai`): the AI lines whose typing this action started are picked too; a pick
        // that comes back before a line's Enter chooses its class (only windows opened by an instant action: one
        // opened by a resolve keeps its rules line).
        if let (Some(p), Some(v)) = (&self.picker, v.as_ref()) {
            let st = world::settings(&w);
            if w.duel_weapon_rules() && st.duel_picker == "on" && st.duel_picker_for_ai && st.llm {
                let scripts: Vec<Value> = std::iter::once(v).chain(v["also"].as_array().into_iter().flatten())
                    .filter(|e| e["event"] == "duel.typing_script").cloned().collect();
                if !scripts.is_empty() {
                    let lv = self.journal.live(&w, &st);
                    for sc in scripts {
                        let (Some(id), Some(ends), Some(text)) = (sc["duel_id"].as_str(), sc["ends_at_ms"].as_i64(), sc["text"].as_str()) else { continue };
                        let Some(bout) = lv.bouts.iter().find(|b| b.duel.id == id && b.weapons) else { continue };
                        let side = if sc["by"] == "a" { 0 } else { 1 };
                        let range = serde_json::to_value(bout.flow.range).ok().and_then(|r| r.as_str().map(str::to_string)).unwrap_or_default();
                        let rec = picker::PickRec { duel_id: id.into(), line_seq: 0, line_at_ms: ends, pre: true, at_ms: ends, pick: None, cached: false,
                                                    latency_ms: 0, tokens_in: 0, tokens_out: 0, cost_usd: 0.0, model: fn7_agents::duel_pick::MODEL.into(),
                                                    error: None, key: picker::key(text, &range, weapons::class_of_side(bout, side), &moves::list_version()) };
                        let (journal, key, seed, beat) = (self.journal.clone(), present::Journal::key_of(&w), w.meta.seed, w.meta.beat);
                        p.spawn(moves::current().body.clone(), duel::pick_ctx(bout, side, text), rec,
                                std::time::Duration::from_millis(st.duel_picker_timeout_ms as u64), move |r| journal.note_pick(&key, seed, beat, seat, &r));
                    }
                }
            }
        }
        // The line is sent: what its author was typing is gone.
        if let Some(id) = v.as_ref().filter(|v| v["event"] == "duel.blow").and_then(|v| v["duel_id"].as_str()) {
            self.typing.sent(w.meta.seed, id, seat, action.received_at_ms);
        }
        // Schema 8: every accepted instant action is journaled with its stamps and its public event, so the present
        // can be redrawn (a reconnect) and survives a restart.
        if w.resume_rules() {
            self.journal.note(&w, seat, action, v.as_ref());
        }
        v
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
        let c = phases::commit(&mut w, round, &self.pantheon);
        if w.resume_rules() {
            // The room may pause after this Commit (nobody connected): the next Start measures it.
            self.journal.committed(&w);
        }
        Ok(c)
    }

    fn is_match_over(&self, state: &Value, round: u32) -> bool {
        world::load(state)
            .map(|w| phases::match_over(&w, round))
            .unwrap_or(true)
    }

    fn public_view(&self, state: &Value) -> Value {
        world::load(state)
            .map(|w| {
                let mut v = phases::public_view(&w);
                // Which rules this match runs on (a client reads it here or in `server.hello.api.rules_version`).
                v["rules"] = json!({ "version": w.meta.rules_version, "state_schema": w.meta.schema, "server": RULES_VERSION });
                // Schema 8 (exact resume): the present moment (the server's clock, the public events since the last
                // resolve, every open duel in full), so a `match.snapshot` redraws it.
                if w.resume_rules() {
                    v["present"] = self.present(&w);
                }
                v
            })
            .unwrap_or(Value::Null)
    }

    fn seat_view(&self, state: &Value, seat: Seat) -> Value {
        world::load(state)
            .map(|w| phases::seat_view(&w, seat))
            .unwrap_or(Value::Null)
    }

    async fn summarize(&self, state: &Value, llm: &dyn LlmProvider) -> anyhow::Result<Summary> {
        let w = world::load(state)?;
        if w.resume_rules() {
            self.journal.finished(&w);
        }
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
#[cfg(test)]
mod dm_tests;
#[cfg(test)]
mod duel_tests;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod panel_tests;
#[cfg(test)]
mod typing_tests;
#[cfg(test)]
mod look_tests;
#[cfg(test)]
mod duel_ai_tests;
#[cfg(test)]
mod moves_tests;
#[cfg(test)]
mod picker_tests;
#[cfg(test)]
mod weapons_tests;
