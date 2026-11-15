//! Room settings: validated once in the lobby, stored in `World.meta.settings`, read back every hook.

use fn7_model::ArenaKind;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use theatre_plugin_api::util::{copy_timer_settings, int_setting, settings_object};

/// Validated settings. Every field has a default; see the SETTINGS table in the README.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Match seed (all randomness derives from it). Absent in the room settings → picked at match start and recorded.
    pub seed: Option<u64>,
    /// Arena city by name (fuzzy); `None` = a seeded pick among cities ≥ 1M on land.
    pub city: Option<String>,
    /// Demigods in the match. Seats take the first places; the plugin drives the rest as built-in AI demigods.
    pub players: u32,
    /// `planet` (default: the whole Earth, regions → cities; `city` is where the Final Night converges) or `city`
    /// (one real city, districts → sites: the schema-2 game).
    pub arena: ArenaKind,
    /// The Final Night falls on this beat (one Theatre round = one beat). `rounds` is accepted as an alias.
    pub beats_total: u32,
    /// Flow beat length (= `flow_ms`; `beat_ms` is accepted as an alias). The Compose window (`compose_seconds`)
    /// defaults to it.
    pub beat_ms: u32,
    /// Flow beat length: the world's action runs in beats this long whether or not anyone types.
    pub flow_ms: u32,
    /// How long a slow-time window may stay open (it spans `ceil(slow_ms / flow_ms)` flow beats).
    pub slow_ms: u32,
    /// Lightning bolts each demigod starts with.
    pub start_bolts: u32,
    /// Multiplier on every Entropy rise (a playtest dial: 2 = twice as fast to heat death).
    pub entropy_scale: f32,
    pub clusters: u32,
    pub max_subagents_per_hand: u32,
    /// Per-call timeout for every mind; a late call is replaced by its rules fallback.
    pub mind_timeout_ms: u32,
    /// false = never call the LLM (every mind uses its rules fallback, no requests made).
    pub llm: bool,
    /// Consequence's bill multiplier (`fn7_society::debt_for` × this), per demigod per beat.
    pub debt_scale: f32,
    /// Beats until a bill falls due (Consequence's per-act bill and Time's bill for orders).
    pub debt_due_beats: u32,
    /// Time's flat debt for orders sent without a slow window (an instant order: bots, the built-in AI, Compose
    /// submits that skip `action.begin`): no repeat factor, no seconds factor; doubled when the bolt can't be paid.
    /// Real slow windows keep the full formula.
    pub instant_order_debt: f32,
    /// Time's circle reaches its end radius (1,500 km on the planet) on this beat, and its damage ramps on the same
    /// schedule: its own clock, independent of `beats_total` (a circle scheduled past the Final Night is still closing
    /// when it falls). The Final Night itself still falls on `beats_total`, at heat death, or with nobody standing.
    pub arena_close_beats: u32,
    /// Multiplier on the rules fallback's Final Destination accident chance (1 in 20 per non-attack act at 1.0).
    pub accident_scale: f32,
    /// Multiplier on the damage an army does when it storms a demigod.
    pub army_strike_scale: f32,
    /// Multiplier on the extra chance that a crowd near an unarmed demigod raises their first volunteer host.
    pub army_first_host_scale: f32,
    /// Live memory items per owner before old ones fold into a summary.
    pub memory_per_owner: u32,
    /// LLM folds per beat (the rest fold by rules).
    pub llm_folds_per_beat: u32,
    /// Most intents one hand turns into acts per beat (orders sent in the same beat they slow time).
    pub max_intents_per_hand: u32,
    /// Most intents one hand turns into acts from orders that closed a slow window that stayed open at least one beat.
    pub max_intents_per_window: u32,
    /// Most people memorized from player mentions per beat.
    pub max_memorize_per_beat: u32,
    /// Most witnesses recorded per act (actors and targets first).
    pub max_witnesses: u32,
    /// World-only simulation: the built-in AI demigods send no orders and cast nothing, so only the society acts.
    pub society_only: bool,
    /// Most general-mind (LLM) calls per beat at decision points (engagements, strain); orders a hand gives a general
    /// always ask the general.
    pub general_calls_per_beat: u32,
    /// Rules 0.7.0: demigods per player: 2 (the Left and Right hands are two bodies, each with its own health; the
    /// player is out when both fall) or 1 (the hands share one body, as before 0.7.0).
    pub demigods_per_player: u32,
    /// Rules 0.7.0: multiplier on the item engine's bolt price per tier (0 = manifesting is free; 2 = twice the price).
    pub manifest_scale: f32,
    /// Rules 0.7.0: multiplier on a manifested host's army footprint in the simulation (`muster_size` × this; 0 = hosts
    /// never take the field, they are only items).
    pub manifest_muster_scale: f32,
    /// Rules 0.7.0: most manifest-mind (LLM) calls per beat (names for wonders and up, and twists); the rest are
    /// named by the rules.
    pub manifest_calls_per_beat: u32,
    /// Rules 0.7.0: magnitude added to every act of the Final Wish (beat 1).
    pub final_wish_magnitude: u32,
    /// Rules 0.7.0, the 1:5 economy: lightning bolts (the mega currency; they also pay Time's toll and invocations)
    /// each player starts with. `start_bolts` is accepted as an alias.
    pub bolt_start: u32,
    /// Sparks (the mini currency) each player starts with.
    pub spark_start: u32,
    /// Bolts every OTHER player gains when someone casts a mega power.
    pub bolt_gift: u32,
    /// Sparks every OTHER player gains when someone casts a mega power.
    pub spark_gift: u32,
    /// Entropy a mega cast adds (× entropy_scale).
    pub entropy_per_mega: f32,
    /// Entropy a mini cast adds (× entropy_scale).
    pub entropy_per_mini: f32,
    /// Rules 0.8.0 (the DM's rulebook): beats in one game day (dawn, day, dusk, night).
    pub beats_per_day: u32,
    /// Rules 0.8.0: the step radius in km (a demigod steps on a hex when its move this beat passes this close; the
    /// planet's value, a city arena uses a hundredth).
    pub dm_reach_km: f32,
    /// Rules 0.8.0: encounter hexes the DM may mark per beat.
    pub dm_encounters_per_beat: u32,
    /// Rules 0.8.0: most hidden encounter hexes on the board at once.
    pub dm_encounters_max: u32,
    /// Rules 0.8.0: beats an unfound encounter hex stays marked before it fades.
    pub dm_encounter_fade_beats: u32,
    /// Rules 0.8.0: multiplier on the damage encounters do.
    pub dm_encounter_damage_scale: f32,
    /// Rules 0.8.0: beats a settlement must hold its level before it grows or shrinks another step.
    pub dm_settlement_step_beats: u32,
    /// Rules 0.8.0: traffic wear at which a path between two settlements becomes a road.
    pub dm_road_wear: f32,
    /// Rules 0.8.0: wear an unused path loses per beat (a road fades below half `dm_road_wear`).
    pub dm_road_decay: f32,
    /// Rules 0.8.0: most shrines one demigod may have.
    pub dm_shrines_per_demigod: u32,
    /// Rules 0.8.0: a spark shrine grants its player a spark every this many beats (one fewer per level above 1, at
    /// least 2).
    pub dm_shrine_spark_every: u32,
    /// Rules 0.8.0: health a heal shrine restores per level per beat to its demigods within reach.
    pub dm_shrine_heal: f32,
    /// Rules 0.8.0: damage a ward shrine does per level per beat to rival demigods within reach.
    pub dm_shrine_ward: f32,
    /// Rules 0.8.0: most memory lines fed to one LLM DM prompt.
    pub dm_memory_lines: u32,
    /// Rules 0.8.0, fault lines: a mega cast this close (km, great circle) to a plate boundary gets the full near
    /// bonus.
    pub fault_near_km: f32,
    /// Rules 0.8.0: a mega cast this far (km) from any plate boundary gets the full far penalty (interpolated in
    /// between).
    pub fault_far_km: f32,
    /// Rules 0.8.0: magnitude added to a mega cast near a fault line.
    pub fault_near_magnitude: f32,
    /// Rules 0.8.0: magnitude added to a mega cast far from any fault line (negative: milder).
    pub fault_far_magnitude: f32,
    /// Rules 0.8.0: radius multiplier of a mega cast near a fault line.
    pub fault_near_radius: f32,
    /// Rules 0.8.0: radius multiplier of a mega cast far from any fault line.
    pub fault_far_radius: f32,
    /// Rules 0.8.0: damage multiplier of a mega cast near a fault line.
    pub fault_near_damage: f32,
    /// Rules 0.8.0: damage multiplier of a mega cast far from any fault line.
    pub fault_far_damage: f32,
    /// State schema 16, fault junctions and tiers (the feature gate): a mega near a plate-boundary junction gets the
    /// junction multipliers on top of the distance curve, every mega lands in a tier (`junction` .. `tiny`) that scales
    /// the world's answer, and a tiny one's lasting mods stay local. Absent in settings recorded before it: off.
    pub fault_junctions_enabled: bool,
    /// Schema 16: a mega this close (km) to a junction gets its multipliers (scaled by the junction's strength,
    /// falling off linearly to nothing at this distance).
    pub fault_junction_km: f32,
    /// Schema 16: magnitude added at a junction of strength 1 (× strength × falloff).
    pub fault_junction_magnitude: f32,
    /// Schema 16: radius multiplier at a junction of strength 1 (1 + (this − 1) × strength × falloff).
    pub fault_junction_radius: f32,
    /// Schema 16: damage multiplier at a junction of strength 1 (1 + (this − 1) × strength × falloff).
    pub fault_junction_damage: f32,
    /// Schema 16: a tiny mega's (≥ `fault_far_km` from any fault) world mods reach at most this far (km): local.
    pub fault_tiny_mod_km: f32,
    /// Rules 0.8.0: players may summon their demigods across the board (an instant `fields.summon`).
    pub summon_enabled: bool,
    /// Rules 0.8.0: beats between two summonings of the same demigod (1 = once a beat).
    pub summon_cooldown_beats: u32,
    /// Rules 0.8.0: multiplier on the health a reckless summoning costs (0 = summoning never hurts).
    pub summon_loss_scale: f32,
    /// Rules 0.8.0: bolts a god-tier power costs (each god's signature power, resolved as its ruling).
    pub god_power_cost: u32,
    /// Rules 0.8.0: bolts every OTHER player gains when someone casts a god-tier power (like a mega).
    pub god_power_gift: u32,
    /// Rules 0.9.0, wills that matter (the feature gate; also needs state schema 7+): a will splits into its parts, healing and blessing your own never just fail, arming makes gear with real modifiers, anyone may plead for a resurrection, seals and curses exist and every one has a mystic counter-quest. Settings recorded before 0.9.0 lack it and read back `false` (old recordings replay unchanged).
    pub wills_enabled: bool,
    /// Rules 0.9.0: sparks one lightning bolt breaks into when a will's price is paid from bolts (the 1:5 economy; the change stays in the purse as sparks).
    pub will_spark_per_bolt: u32,
    /// Rules 0.9.0: the base DC of a will on your own demigods (heal, bless, arm): DC = this + magnitude/4 (an arming adds 2 per tier). A hostile act's is 8 + magnitude/2.
    pub will_own_dc: u32,
    /// Rules 0.9.0: the open debt (health points, per demigod) beyond which the gods finance no more unpaid wills: the will becomes a mystic quest instead.
    pub will_debt_cap: f32,
    /// Rules 0.9.0: debt (to the ruling god) per spark of a will's price the purse could not pay.
    pub will_debt_per_spark: f32,
    /// Rules 0.9.0: Entropy raised per spark paid for a will.
    pub will_entropy_per_spark: f32,
    /// Rules 0.9.0: a heal or blessing costs this + magnitude/4 sparks per target.
    pub heal_price_sparks: u32,
    /// Rules 0.9.0: the token heal of a failed heal roll (the rest waits on its mystic quest).
    pub heal_token: f32,
    /// Rules 0.9.0: multiplier on every will heal (1 + magnitude/4 health per target before it).
    pub heal_scale: f32,
    /// Rules 0.9.0: beats a boon (a blessing, or a critical heal's gift) lasts.
    pub boon_beats: u32,
    /// Rules 0.9.0: the d20 bonus a boon gives its demigod's acts.
    pub boon_bonus: u32,
    /// Rules 0.9.0: sparks an artifact-tier arm costs per demigod (a trinket is free).
    pub arm_price_artifact: u32,
    /// Rules 0.9.0: sparks a relic-tier arm costs per demigod.
    pub arm_price_relic: u32,
    /// Rules 0.9.0: sparks a god's blessing on an arming costs (per will part): the gear is a tier up and carries the god's signature.
    pub arm_bless_sparks: u32,
    /// Rules 0.9.0: multiplier on every number an item's modifiers add (strike, damage, guard, regeneration, duel); 0 = gear is decoration.
    pub arms_scale: f32,
    /// Rules 0.9.0: Death's base price in sparks to raise a fallen demigod, times 1 + the times it was raised before, plus 1 per raise_stale_beats it has lain dead.
    pub raise_price_sparks: u32,
    /// Rules 0.9.0: every this many beats a demigod has been dead adds a spark to Death's price.
    pub raise_stale_beats: u32,
    /// Rules 0.9.0: raisings one player may be granted per match (0 = none).
    pub raise_cap: u32,
    /// Rules 0.9.0: max-health years a raised demigod loses, times 1 + the times it was raised before.
    pub raise_years: f32,
    /// Rules 0.9.0: the share of its max health a raised demigod returns with.
    pub raise_health_fraction: f32,
    /// Rules 0.9.0: sparks a curse or a seal costs (a bolt breaks into will_spark_per_bolt sparks).
    pub curse_price_sparks: u32,
    /// Rules 0.9.0: sparks the god-tier variant of a curse costs (strength 3, until countered).
    pub curse_god_price_sparks: u32,
    /// Rules 0.9.0: beats an ordinary curse lasts (a seal and a god-tier curse last until countered).
    pub curse_beats: u32,
    /// Rules 0.9.0: the DC of casting a curse (plus half the target's wardrobe stage).
    pub curse_dc: u32,
    /// Rules 0.9.0: curses and seals one player may have standing at once.
    pub curse_max_per_player: u32,
    /// Rules 0.9.0: Entropy raised by a curse (a god-tier one raises entropy_per_mega).
    pub curse_entropy: f32,
    /// Rules 0.9.0: the most steps a mystic quest has.
    pub quest_steps_max: u32,
    /// Rules 0.9.0: beats a heal's or an arming's quest stays open (0 = until done); a curse's counter-quest lives as long as its curse, a raising's until the Final Night.
    pub quest_expire_beats: u32,
    /// Rules 0.9.0: sparks a quest's offering step asks for (times the quest's difficulty).
    pub quest_spark_step: u32,
    /// Rules 0.9.0: how readily the built-in AI uses wills (heals when hurt, arms itself, pleads for its fallen sibling, curses a rival, pursues its quests); 0 = never.
    pub wills_ai_chance: f32,
    /// Gear looks (state schema 11; the feature gate): each demigod's loadout (its best item per slot plus its other major items) is tracked, a human player's demigod gets a new portrait and diorama WITH its gear at each major acquisition, and notable items get item art. Settings recorded before it lack it and read back `false` (old recordings replay unchanged).
    pub looks_enabled: bool,
    /// Gear looks: major items a loadout lists beyond the best item per slot (relic tier and up, god-blessed, or the first item of a new kind).
    pub look_extra_items: u32,
    /// Gear looks: items a gear portrait or diorama shows at most (the loadout's first ones).
    pub look_items_shown: u32,
    /// Gear looks: new gear images (a portrait and a diorama each) one human player's demigod may get per match; beyond it the loadout still updates and the picture stays.
    pub look_images_per_demigod: u32,
    /// Gear looks: beats between two new gear images of one demigod (the newest loadout wins when the wait ends).
    pub look_debounce_beats: u32,
    /// Gear looks: item pictures granted per match (notable items a human player's demigod holds, or lying in a loot pile); lesser and later items keep their icon.
    pub item_art_per_match: u32,
    /// Rules 0.8.1, challenge mode: a demigod may challenge a rival in reach to a typed duel (`fields.challenge`), and
    /// the built-in AI now and then does. Settings recorded before 0.8.1 lack it and read back `false` (old recordings
    /// replay unchanged).
    pub challenge_enabled: bool,
    /// Rules 0.8.1 marker: always `true` in settings validated by a 0.8.1+ server, absent (`false`) in recordings from
    /// before, so 0.8.1 behaviour changes (the takeover rename fix) stay off when an older match replays.
    pub rules_0_8_1: bool,
    /// Rules 0.8.1: a challenge always reaches this far (km), at any zoom.
    pub challenge_min_km: f32,
    /// Rules 0.8.1: at the client's board `level` a challenge also reaches as far as two board cards touching there
    /// (km): direct combat must be very straightforward to engage. The reach is max(`challenge_min_km`, this). This is
    /// the regions level's (also for a missing or unknown level).
    pub challenge_contact_km_regions: f32,
    /// Rules 0.8.1: the touching-cards reach (km) of a challenge issued at the villages level.
    pub challenge_contact_km_villages: f32,
    /// Rules 0.8.1: the touching-cards reach (km) of a challenge issued at the houses level.
    pub challenge_contact_km_houses: f32,
    /// Rules 0.8.1: the touching-cards reach (km) of a challenge issued at the people level.
    pub challenge_contact_km_people: f32,
    /// Rules 0.8.1: beats between two challenges by the same demigod (1 = one a beat).
    pub duel_cooldown_beats: u32,
    /// Rules 0.8.1: multiplier on every duel blow's damage (0-3, a crit 4).
    pub duel_damage_scale: f32,
    /// Rules 0.8.1: the chance a built-in AI demigod with a rival in reach challenges it, per beat.
    pub duel_ai_chance: f32,
    /// Rules 0.8.1: a built-in AI demigod whose nearest rival is beyond regions reach but within this (km) summons
    /// itself next to it first, then challenges (the summoning's mishap applies).
    pub duel_ai_reach_km: f32,
    /// Rules 0.8.1: blows each side throws automatically in a duel nobody typed in (the AI's phrase bank).
    pub duel_auto_rounds: u32,
    /// Rules 0.8.1: a human duellist who hasn't blown for this long (ms) is countered automatically, like an AI.
    pub duel_counter_ms: u32,
    /// Rules 0.8.1: a blow sooner than this (ms) after the same side's previous one is spam: a penalty verdict.
    pub duel_min_gap_ms: u32,
    /// Rules 0.8.1: a blow within this (ms) of the same side's previous one is a quick follow-up (+1).
    pub duel_followup_ms: u32,
    /// Rules 0.8.1: the longest a duel may run (ms from the lock): the engine's hold deadline ("the world holds its
    /// breath"); without engine holds the world keeps beating, lines after it are refused, and the duel ends at the
    /// first resolve past it (at most `ceil(duel_deadline_ms / flow_ms)` resolves from its lock).
    pub duel_deadline_ms: u32,
    /// The Earth simulator (state schema 13, docs/EARTH_SIM.md): god powers act on a simulated voxel field of the planet whose fires, floods, quakes and lava reach demigods, crowds and settlements through the rules. Planet arenas only. Absent in settings recorded before it: off.
    pub earth_sim_enabled: bool,
    /// Most cells that may differ from the base terrain; past it the calmest settle back to base (remembered as `earth.settled`).
    pub earth_max_cells: u32,
    /// Most live phenomena; past it a newcomer replaces the weakest, or is refused.
    pub earth_max_phenomena: u32,
    /// Sub-steps of the field per beat at rest (Entropy 60, 80 and 100 each add one).
    pub earth_substeps: u32,
    /// A keyframe of the field is kept every this many beats (any beat is rebuilt by re-simulating at most this many).
    pub earth_keyframe_beats: u32,
    /// A drifting patch of plate moves one cell every this many beats.
    pub earth_plate_beats: u32,
    /// A mega's reach in cells is its radius in km over this (1 to 4 cells); a mini is one cell.
    pub earth_km_per_cell: f32,
    /// Multiplier on how fast fire, water and swarms spread.
    pub earth_spread_scale: f32,
    /// Multiplier on the odds of the world's own phenomena under Entropy (quakes, volcanoes and drifting plates on stressed faults, freak weather).
    pub earth_spontaneous_scale: f32,
    /// Multiplier on every phenomenon's starting power.
    pub earth_energy_scale: f32,
    /// A mini's starting power relative to a mega of the same magnitude.
    pub earth_mini_scale: f32,
    /// Multiplier on the damage the field deals demigods standing in it.
    pub earth_damage_scale: f32,
    /// Most damage the field deals one demigod in one beat (before `earth_damage_scale`).
    pub earth_damage_cap: f32,
    /// Damage per beat in a cell burning at full intensity.
    pub earth_fire_damage: f32,
    /// Damage per beat in a cell deep in lava.
    pub earth_lava_damage: f32,
    /// Damage per beat in a cell flooded to the brim, drowned, or hit by a tidal wave.
    pub earth_flood_damage: f32,
    /// Damage in a cell shaking at full strength.
    pub earth_quake_damage: f32,
    /// Damage from a lightning strike on the cell.
    pub earth_storm_damage: f32,
    /// Damage per beat in a blizzard.
    pub earth_cold_damage: f32,
    /// Damage per beat in a cell thick with a swarm.
    pub earth_swarm_damage: f32,
    /// Multiplier on the fear, awe and deaths the field brings crowds.
    pub earth_crowd_scale: f32,
    /// Most crowds the field moves in one beat (the worst hit first).
    pub earth_crowd_effects: u32,
    /// Multiplier on how much a cell's ruin lowers its settlements' level target.
    pub earth_ruin_scale: f32,
    /// Most Earth entries written to the DM's memory in one beat.
    pub earth_events_per_beat: u32,
    /// Schema 12: a player fights one duel at a time: while one of its demigods is in an open duel, a challenge by
    /// or to any of its demigods is refused (`busy:player`) and the AI's chooser skips it.
    pub duel_one_per_player: bool,
    /// Schema 12 (AI duellists type): the chance an AI fighter types a line of its own in an input window of a duel
    /// with a clock (1 = every window; 0 = off: it only counters, as in 0.8.2).
    pub duel_ai_lines_per_window: f32,
    /// Schema 12: how often the AI attacks rather than sets a defence, repositions or taunts (0 = cautious, 1 = it
    /// only ever swings).
    pub duel_ai_aggression: f32,
    /// Schema 12: the AI's handicap: subtracted from the d20 modifier of the lines it types against a human (it
    /// strikes in every window, which no human does; its wit is capped at 2 like an automatic line's). Waived
    /// against a human silent for 12 s and between two AIs. 0 = none.
    pub duel_ai_handicap: u32,
    /// Schema 14 (docs/DUEL_MOVES.md §4.1): `on`: one structured LLM call per typed human duel line chooses its move
    /// class and presentation from the list (needs the room's `llm` and a server LLM); `off`: the keyword mapper only.
    pub duel_picker: String,
    /// How long a pick may take before the mapper's stands (ms).
    pub duel_picker_timeout_ms: u32,
    /// Also pick for the lines AI duellists type (off: they keep their rules lines' classes and clips).
    pub duel_picker_for_ai: bool,
    /// Schema 14: hammer-space draws per fighter per duel (pulling out something it does not hold).
    pub duel_draws: u32,
    /// Sparks a hammer-space draw costs when the purse has them (paid: an artifact-tier prop; unpaid: a trinket).
    pub duel_draw_sparks: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            seed: None,
            city: None,
            players: 4,
            arena: ArenaKind::Planet,
            beats_total: DEFAULT_BEATS_TOTAL,
            beat_ms: 3000,
            flow_ms: 3000,
            slow_ms: 30_000,
            start_bolts: DEFAULT_BOLT_START,
            entropy_scale: 1.0,
            clusters: 200,
            max_subagents_per_hand: 3,
            mind_timeout_ms: 6000,
            llm: true,
            debt_scale: 0.04,
            debt_due_beats: DEFAULT_DEBT_DUE_BEATS,
            instant_order_debt: 0.5,
            arena_close_beats: DEFAULT_ARENA_CLOSE_BEATS,
            accident_scale: DEFAULT_KILLER_SCALE,
            army_strike_scale: DEFAULT_KILLER_SCALE,
            army_first_host_scale: DEFAULT_KILLER_SCALE,
            memory_per_owner: 40,
            llm_folds_per_beat: 2,
            max_intents_per_hand: 6,
            max_intents_per_window: 12,
            max_memorize_per_beat: 6,
            max_witnesses: 4,
            society_only: false,
            general_calls_per_beat: 2,
            demigods_per_player: 2,
            manifest_scale: 1.0,
            manifest_muster_scale: DEFAULT_MUSTER_SCALE,
            manifest_calls_per_beat: 2,
            final_wish_magnitude: 2,
            bolt_start: DEFAULT_BOLT_START,
            spark_start: 5,
            bolt_gift: 1,
            spark_gift: 0,
            entropy_per_mega: 10.0,
            entropy_per_mini: 1.0,
            beats_per_day: 8,
            dm_reach_km: 30.0,
            dm_encounters_per_beat: 1,
            dm_encounters_max: 10,
            dm_encounter_fade_beats: 20,
            dm_encounter_damage_scale: 1.0,
            dm_settlement_step_beats: 6,
            dm_road_wear: 4.0,
            dm_road_decay: 0.05,
            dm_shrines_per_demigod: 3,
            dm_shrine_spark_every: 6,
            dm_shrine_heal: 0.15,
            dm_shrine_ward: 0.1,
            dm_memory_lines: 6,
            fault_near_km: 150.0,
            fault_far_km: 1500.0,
            fault_near_magnitude: 3.0,
            fault_far_magnitude: -2.0,
            fault_near_radius: 1.5,
            fault_far_radius: 1.0,
            fault_near_damage: 1.6,
            fault_far_damage: 0.6,
            fault_junctions_enabled: true,
            fault_junction_km: 300.0,
            fault_junction_magnitude: 3.0,
            fault_junction_radius: 1.5,
            fault_junction_damage: 1.8,
            fault_tiny_mod_km: 150.0,
            summon_enabled: true,
            summon_cooldown_beats: 1,
            summon_loss_scale: 1.0,
            god_power_cost: 3,
            god_power_gift: 1,
            wills_enabled: true,
            will_spark_per_bolt: 5,
            will_own_dc: 6,
            will_debt_cap: 3.0,
            will_debt_per_spark: 0.2,
            will_entropy_per_spark: 0.25,
            heal_price_sparks: 1,
            heal_token: 0.5,
            heal_scale: 1.0,
            boon_beats: 6,
            boon_bonus: 1,
            arm_price_artifact: 2,
            arm_price_relic: 5,
            arm_bless_sparks: 5,
            arms_scale: 1.0,
            raise_price_sparks: 5,
            raise_stale_beats: 8,
            raise_cap: 3,
            raise_years: 1.0,
            raise_health_fraction: 0.4,
            curse_price_sparks: 5,
            curse_god_price_sparks: 15,
            curse_beats: 12,
            curse_dc: 10,
            curse_max_per_player: 2,
            curse_entropy: 3.0,
            quest_steps_max: 3,
            quest_expire_beats: 24,
            quest_spark_step: 2,
            wills_ai_chance: 0.25,
            looks_enabled: true,
            look_extra_items: 2,
            look_items_shown: 3,
            look_images_per_demigod: 4,
            look_debounce_beats: 3,
            item_art_per_match: 12,
            challenge_enabled: true,
            rules_0_8_1: true,
            challenge_min_km: 120.0,
            challenge_contact_km_regions: 600.0,
            challenge_contact_km_villages: 15.0,
            challenge_contact_km_houses: 0.15,
            challenge_contact_km_people: 0.015,
            duel_cooldown_beats: 1,
            duel_damage_scale: 1.0,
            duel_ai_chance: 0.02,
            duel_ai_reach_km: 2000.0,
            duel_auto_rounds: 2,
            duel_counter_ms: 3000,
            duel_min_gap_ms: 500,
            duel_followup_ms: 1500,
            duel_deadline_ms: 30_000,
            earth_sim_enabled: true,
            earth_max_cells: 1500,
            earth_max_phenomena: 48,
            earth_substeps: 1,
            earth_keyframe_beats: 16,
            earth_plate_beats: 3,
            earth_km_per_cell: 400.0,
            earth_spread_scale: 1.0,
            earth_spontaneous_scale: 1.0,
            earth_energy_scale: 1.0,
            earth_mini_scale: 0.5,
            earth_damage_scale: 1.0,
            earth_damage_cap: 1.0,
            earth_fire_damage: 0.6,
            earth_lava_damage: 1.2,
            earth_flood_damage: 0.5,
            earth_quake_damage: 0.5,
            earth_storm_damage: 0.4,
            earth_cold_damage: 0.2,
            earth_swarm_damage: 0.2,
            earth_crowd_scale: 1.0,
            earth_crowd_effects: 6,
            earth_ruin_scale: 1.0,
            earth_events_per_beat: 6,
            duel_one_per_player: true,
            duel_ai_lines_per_window: 1.0,
            duel_ai_aggression: 0.6,
            duel_ai_handicap: 5,
            duel_picker: "on".into(),
            duel_picker_timeout_ms: 700,
            duel_picker_for_ai: false,
            duel_draws: 2,
            duel_draw_sparks: 1,
        }
    }
}

/// Balance defaults (docs/BALANCE_REPORT_2.md): the Final Night's beat, Time's circle, bills falling due, and the
/// non-debt killers' multiplier (accidents, army strikes, first hosts).
pub const DEFAULT_BEATS_TOTAL: u32 = 60;
pub const DEFAULT_ARENA_CLOSE_BEATS: u32 = 160;
pub const DEFAULT_DEBT_DUE_BEATS: u32 = 50;
pub const DEFAULT_KILLER_SCALE: f32 = 0.82;
/// Rules 0.7.0: lightning bolts a player starts with (the designer's 1 bolt : 5 sparks).
pub const DEFAULT_BOLT_START: u32 = 1;
/// Rules 0.7.0: a manifested host's footprint in the simulation, as a share of `muster_size`.
pub const DEFAULT_MUSTER_SCALE: f32 = 0.25;

/// The f32 settings, echoed back at f32 precision.
const F32_KEYS: &[&str] =
    &["debt_scale", "entropy_scale", "instant_order_debt", "accident_scale", "army_strike_scale", "army_first_host_scale",
      "manifest_scale", "manifest_muster_scale", "entropy_per_mega", "entropy_per_mini", "dm_reach_km",
      "dm_encounter_damage_scale", "dm_road_wear", "dm_road_decay", "dm_shrine_heal", "dm_shrine_ward", "fault_near_km", "fault_far_km",
      "fault_near_magnitude", "fault_far_magnitude", "fault_near_radius", "fault_far_radius", "fault_near_damage",
      "fault_far_damage", "fault_junction_km", "fault_junction_magnitude", "fault_junction_radius", "fault_junction_damage",
      "fault_tiny_mod_km", "summon_loss_scale", "challenge_min_km", "challenge_contact_km_regions", "challenge_contact_km_villages",
      "challenge_contact_km_houses", "challenge_contact_km_people", "duel_damage_scale", "duel_ai_chance", "duel_ai_reach_km",
      "duel_ai_lines_per_window", "duel_ai_aggression",
      "will_debt_cap", "will_debt_per_spark", "will_entropy_per_spark", "heal_token", "heal_scale", "arms_scale", "raise_years", "raise_health_fraction", "curse_entropy", "wills_ai_chance",
      "earth_km_per_cell", "earth_spread_scale", "earth_spontaneous_scale", "earth_energy_scale", "earth_mini_scale", "earth_damage_scale", "earth_damage_cap", "earth_fire_damage", "earth_lava_damage", "earth_flood_damage", "earth_quake_damage", "earth_storm_damage", "earth_cold_damage", "earth_swarm_damage", "earth_crowd_scale", "earth_ruin_scale"];


/// The shortest decimal that reads back as `x` (so `0.04f32` is `0.04`, not `0.03999999910593033`).
fn f32_decimal(x: f32) -> f64 {
    format!("{x}").parse().unwrap_or(x as f64)
}

/// `resolve_timeout_s` when the room doesn't set one: a beat must never wait on the core's 180 s default.
pub const DEFAULT_RESOLVE_TIMEOUT_S: f64 = 60.0;

impl Settings {
    /// Validate and normalize room settings (the object the Theatre stores and passes to `on_match_start`).
    pub fn validate(v: &Value) -> Result<Value, String> {
        let m = settings_object(v)?;
        let d = Settings::default();
        let int = |k: &str, def: u32, min: i64, max: i64| -> Result<u32, String> {
            int_setting(&m, k, def as i64, min, max).map(|x| x as u32)
        };
        let boolean = |k: &str, def: bool| -> Result<bool, String> {
            match m.get(k) {
                None | Some(Value::Null) => Ok(def),
                Some(x) => x.as_bool().ok_or(format!("`{k}` must be true/false")),
            }
        };
        let num = |k: &str, def: f32, min: f32, max: f32| -> Result<f32, String> {
            match m.get(k) {
                None | Some(Value::Null) => Ok(def),
                Some(x) => {
                    let f = x.as_f64().ok_or(format!("`{k}` must be a number"))? as f32;
                    if !(min..=max).contains(&f) {
                        return Err(format!("`{k}` must be between {min} and {max}"));
                    }
                    Ok(f)
                }
            }
        };
        let seed = match m.get("seed") {
            None | Some(Value::Null) => None,
            Some(s) => Some(s.as_u64().ok_or("`seed` must be a non-negative integer")?),
        };
        let city = match m.get("city") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.trim().is_empty() => None,
            Some(Value::String(s)) if s.chars().count() <= 80 => Some(s.trim().to_string()),
            _ => return Err("`city` must be a city name (up to 80 characters)".into()),
        };
        let beats_total = match (m.get("beats_total"), m.get("rounds")) {
            (Some(b), _) if !b.is_null() => int("beats_total", d.beats_total, 1, 1000)?,
            (_, Some(r)) if !r.is_null() => int("rounds", d.beats_total, 1, 1000)?,
            _ => d.beats_total,
        };
        let arena = match m.get("arena") {
            None | Some(Value::Null) => d.arena,
            Some(Value::String(s)) if s.trim().eq_ignore_ascii_case("planet") => ArenaKind::Planet,
            Some(Value::String(s)) if s.trim().eq_ignore_ascii_case("city") => ArenaKind::City,
            _ => return Err("`arena` must be \"planet\" or \"city\"".into()),
        };
        let flow_ms = match (m.get("flow_ms"), m.get("beat_ms")) {
            (Some(f), _) if !f.is_null() => int("flow_ms", d.flow_ms, 500, 120_000)?,
            (_, Some(b)) if !b.is_null() => int("beat_ms", d.flow_ms, 500, 120_000)?,
            _ => d.flow_ms,
        };
        let s = Settings {
            seed,
            city,
            players: int("players", d.players, 1, 8)?,
            arena,
            beats_total,
            beat_ms: flow_ms,
            flow_ms,
            slow_ms: int("slow_ms", d.slow_ms, 1000, 600_000)?,
            start_bolts: 0, // set from bolt_start below
            entropy_scale: num("entropy_scale", d.entropy_scale, 0.0, 10.0)?,
            clusters: int("clusters", d.clusters, 8, 600)?,
            max_subagents_per_hand: int("max_subagents_per_hand", d.max_subagents_per_hand, 0, 10)?,
            mind_timeout_ms: int("mind_timeout_ms", d.mind_timeout_ms, 50, 120_000)?,
            llm: boolean("llm", d.llm)?,
            debt_scale: num("debt_scale", d.debt_scale, 0.0, 10.0)?,
            debt_due_beats: int("debt_due_beats", d.debt_due_beats, 1, 100)?,
            instant_order_debt: num("instant_order_debt", d.instant_order_debt, 0.0, 10.0)?,
            arena_close_beats: int("arena_close_beats", d.arena_close_beats, 1, 1000)?,
            accident_scale: num("accident_scale", d.accident_scale, 0.0, 10.0)?,
            army_strike_scale: num("army_strike_scale", d.army_strike_scale, 0.0, 10.0)?,
            army_first_host_scale: num("army_first_host_scale", d.army_first_host_scale, 0.0, 10.0)?,
            memory_per_owner: int("memory_per_owner", d.memory_per_owner, 4, 1000)?,
            llm_folds_per_beat: int("llm_folds_per_beat", d.llm_folds_per_beat, 0, 32)?,
            max_intents_per_hand: int("max_intents_per_hand", d.max_intents_per_hand, 1, 12)?,
            max_intents_per_window: int("max_intents_per_window", d.max_intents_per_window, 1, 24)?,
            max_memorize_per_beat: int("max_memorize_per_beat", d.max_memorize_per_beat, 0, 64)?,
            max_witnesses: int("max_witnesses", d.max_witnesses, 1, 64)?,
            society_only: boolean("society_only", d.society_only)?,
            general_calls_per_beat: int("general_calls_per_beat", d.general_calls_per_beat, 0, 16)?,
            demigods_per_player: int("demigods_per_player", d.demigods_per_player, 1, 2)?,
            manifest_scale: num("manifest_scale", d.manifest_scale, 0.0, 10.0)?,
            manifest_muster_scale: num("manifest_muster_scale", d.manifest_muster_scale, 0.0, 1.0)?,
            manifest_calls_per_beat: int("manifest_calls_per_beat", d.manifest_calls_per_beat, 0, 16)?,
            final_wish_magnitude: int("final_wish_magnitude", d.final_wish_magnitude, 0, 5)?,
            bolt_start: match m.get("bolt_start") {
                Some(b) if !b.is_null() => int("bolt_start", d.bolt_start, 0, 99)?,
                _ => int("start_bolts", d.bolt_start, 0, 99)?,
            },
            spark_start: int("spark_start", d.spark_start, 0, 99)?,
            bolt_gift: int("bolt_gift", d.bolt_gift, 0, 10)?,
            spark_gift: int("spark_gift", d.spark_gift, 0, 10)?,
            entropy_per_mega: num("entropy_per_mega", d.entropy_per_mega, 0.0, 100.0)?,
            entropy_per_mini: num("entropy_per_mini", d.entropy_per_mini, 0.0, 20.0)?,
            beats_per_day: int("beats_per_day", d.beats_per_day, 2, 200)?,
            dm_reach_km: num("dm_reach_km", d.dm_reach_km, 0.1, 2000.0)?,
            dm_encounters_per_beat: int("dm_encounters_per_beat", d.dm_encounters_per_beat, 0, 8)?,
            dm_encounters_max: int("dm_encounters_max", d.dm_encounters_max, 0, 64)?,
            dm_encounter_fade_beats: int("dm_encounter_fade_beats", d.dm_encounter_fade_beats, 1, 1000)?,
            dm_encounter_damage_scale: num("dm_encounter_damage_scale", d.dm_encounter_damage_scale, 0.0, 10.0)?,
            dm_settlement_step_beats: int("dm_settlement_step_beats", d.dm_settlement_step_beats, 1, 100)?,
            dm_road_wear: num("dm_road_wear", d.dm_road_wear, 0.5, 1000.0)?,
            dm_road_decay: num("dm_road_decay", d.dm_road_decay, 0.0, 10.0)?,
            dm_shrines_per_demigod: int("dm_shrines_per_demigod", d.dm_shrines_per_demigod, 0, 20)?,
            dm_shrine_spark_every: int("dm_shrine_spark_every", d.dm_shrine_spark_every, 1, 100)?,
            dm_shrine_heal: num("dm_shrine_heal", d.dm_shrine_heal, 0.0, 5.0)?,
            dm_shrine_ward: num("dm_shrine_ward", d.dm_shrine_ward, 0.0, 5.0)?,
            dm_memory_lines: int("dm_memory_lines", d.dm_memory_lines, 0, 30)?,
            fault_near_km: num("fault_near_km", d.fault_near_km, 0.0, 5000.0)?,
            fault_far_km: num("fault_far_km", d.fault_far_km, 1.0, 20000.0)?,
            fault_near_magnitude: num("fault_near_magnitude", d.fault_near_magnitude, -10.0, 10.0)?,
            fault_far_magnitude: num("fault_far_magnitude", d.fault_far_magnitude, -10.0, 10.0)?,
            fault_near_radius: num("fault_near_radius", d.fault_near_radius, 0.1, 10.0)?,
            fault_far_radius: num("fault_far_radius", d.fault_far_radius, 0.1, 10.0)?,
            fault_near_damage: num("fault_near_damage", d.fault_near_damage, 0.0, 10.0)?,
            fault_far_damage: num("fault_far_damage", d.fault_far_damage, 0.0, 10.0)?,
            fault_junctions_enabled: boolean("fault_junctions_enabled", d.fault_junctions_enabled)?,
            fault_junction_km: num("fault_junction_km", d.fault_junction_km, 0.0, 3000.0)?,
            fault_junction_magnitude: num("fault_junction_magnitude", d.fault_junction_magnitude, -10.0, 10.0)?,
            fault_junction_radius: num("fault_junction_radius", d.fault_junction_radius, 0.1, 10.0)?,
            fault_junction_damage: num("fault_junction_damage", d.fault_junction_damage, 0.0, 10.0)?,
            fault_tiny_mod_km: num("fault_tiny_mod_km", d.fault_tiny_mod_km, 20.0, 1500.0)?,
            summon_enabled: boolean("summon_enabled", d.summon_enabled)?,
            summon_cooldown_beats: int("summon_cooldown_beats", d.summon_cooldown_beats, 1, 100)?,
            summon_loss_scale: num("summon_loss_scale", d.summon_loss_scale, 0.0, 10.0)?,
            god_power_cost: int("god_power_cost", d.god_power_cost, 0, 20)?,
            god_power_gift: int("god_power_gift", d.god_power_gift, 0, 10)?,
            wills_enabled: boolean("wills_enabled", d.wills_enabled)?,
            will_spark_per_bolt: int("will_spark_per_bolt", d.will_spark_per_bolt, 1, 20)?,
            will_own_dc: int("will_own_dc", d.will_own_dc, 2, 20)?,
            will_debt_cap: num("will_debt_cap", d.will_debt_cap, 0.0, 20.0)?,
            will_debt_per_spark: num("will_debt_per_spark", d.will_debt_per_spark, 0.0, 5.0)?,
            will_entropy_per_spark: num("will_entropy_per_spark", d.will_entropy_per_spark, 0.0, 10.0)?,
            heal_price_sparks: int("heal_price_sparks", d.heal_price_sparks, 0, 20)?,
            heal_token: num("heal_token", d.heal_token, 0.0, 5.0)?,
            heal_scale: num("heal_scale", d.heal_scale, 0.0, 10.0)?,
            boon_beats: int("boon_beats", d.boon_beats, 1, 100)?,
            boon_bonus: int("boon_bonus", d.boon_bonus, 0, 5)?,
            arm_price_artifact: int("arm_price_artifact", d.arm_price_artifact, 0, 50)?,
            arm_price_relic: int("arm_price_relic", d.arm_price_relic, 0, 50)?,
            arm_bless_sparks: int("arm_bless_sparks", d.arm_bless_sparks, 0, 50)?,
            arms_scale: num("arms_scale", d.arms_scale, 0.0, 5.0)?,
            raise_price_sparks: int("raise_price_sparks", d.raise_price_sparks, 0, 100)?,
            raise_stale_beats: int("raise_stale_beats", d.raise_stale_beats, 1, 1000)?,
            raise_cap: int("raise_cap", d.raise_cap, 0, 50)?,
            raise_years: num("raise_years", d.raise_years, 0.0, 10.0)?,
            raise_health_fraction: num("raise_health_fraction", d.raise_health_fraction, 0.05, 1.0)?,
            curse_price_sparks: int("curse_price_sparks", d.curse_price_sparks, 0, 100)?,
            curse_god_price_sparks: int("curse_god_price_sparks", d.curse_god_price_sparks, 0, 200)?,
            curse_beats: int("curse_beats", d.curse_beats, 1, 1000)?,
            curse_dc: int("curse_dc", d.curse_dc, 2, 30)?,
            curse_max_per_player: int("curse_max_per_player", d.curse_max_per_player, 0, 20)?,
            curse_entropy: num("curse_entropy", d.curse_entropy, 0.0, 100.0)?,
            quest_steps_max: int("quest_steps_max", d.quest_steps_max, 1, 3)?,
            quest_expire_beats: int("quest_expire_beats", d.quest_expire_beats, 0, 1000)?,
            quest_spark_step: int("quest_spark_step", d.quest_spark_step, 0, 50)?,
            wills_ai_chance: num("wills_ai_chance", d.wills_ai_chance, 0.0, 1.0)?,
            looks_enabled: boolean("looks_enabled", d.looks_enabled)?,
            look_extra_items: int("look_extra_items", d.look_extra_items, 0, 6)?,
            look_items_shown: int("look_items_shown", d.look_items_shown, 1, 5)?,
            look_images_per_demigod: int("look_images_per_demigod", d.look_images_per_demigod, 0, 20)?,
            look_debounce_beats: int("look_debounce_beats", d.look_debounce_beats, 0, 100)?,
            item_art_per_match: int("item_art_per_match", d.item_art_per_match, 0, 200)?,
            challenge_enabled: boolean("challenge_enabled", d.challenge_enabled)?,
            rules_0_8_1: true,
            challenge_min_km: num("challenge_min_km", d.challenge_min_km, 0.0, 20000.0)?,
            challenge_contact_km_regions: num("challenge_contact_km_regions", d.challenge_contact_km_regions, 0.0, 20000.0)?,
            challenge_contact_km_villages: num("challenge_contact_km_villages", d.challenge_contact_km_villages, 0.0, 20000.0)?,
            challenge_contact_km_houses: num("challenge_contact_km_houses", d.challenge_contact_km_houses, 0.0, 20000.0)?,
            challenge_contact_km_people: num("challenge_contact_km_people", d.challenge_contact_km_people, 0.0, 20000.0)?,
            duel_cooldown_beats: int("duel_cooldown_beats", d.duel_cooldown_beats, 1, 100)?,
            duel_damage_scale: num("duel_damage_scale", d.duel_damage_scale, 0.0, 10.0)?,
            duel_ai_chance: num("duel_ai_chance", d.duel_ai_chance, 0.0, 1.0)?,
            duel_ai_reach_km: num("duel_ai_reach_km", d.duel_ai_reach_km, 0.0, 20000.0)?,
            duel_auto_rounds: int("duel_auto_rounds", d.duel_auto_rounds, 0, 10)?,
            duel_counter_ms: int("duel_counter_ms", d.duel_counter_ms, 0, 60_000)?,
            duel_min_gap_ms: int("duel_min_gap_ms", d.duel_min_gap_ms, 0, 5000)?,
            duel_followup_ms: int("duel_followup_ms", d.duel_followup_ms, 0, 10_000)?,
            duel_deadline_ms: int("duel_deadline_ms", d.duel_deadline_ms, 1000, 600_000)?,
            earth_sim_enabled: boolean("earth_sim_enabled", d.earth_sim_enabled)?,
            earth_max_cells: int("earth_max_cells", d.earth_max_cells, 64, 8000)?,
            earth_max_phenomena: int("earth_max_phenomena", d.earth_max_phenomena, 4, 200)?,
            earth_substeps: int("earth_substeps", d.earth_substeps, 1, 4)?,
            earth_keyframe_beats: int("earth_keyframe_beats", d.earth_keyframe_beats, 1, 1000)?,
            earth_plate_beats: int("earth_plate_beats", d.earth_plate_beats, 1, 50)?,
            earth_km_per_cell: num("earth_km_per_cell", d.earth_km_per_cell, 50.0, 5000.0)?,
            earth_spread_scale: num("earth_spread_scale", d.earth_spread_scale, 0.0, 4.0)?,
            earth_spontaneous_scale: num("earth_spontaneous_scale", d.earth_spontaneous_scale, 0.0, 8.0)?,
            earth_energy_scale: num("earth_energy_scale", d.earth_energy_scale, 0.1, 4.0)?,
            earth_mini_scale: num("earth_mini_scale", d.earth_mini_scale, 0.0, 2.0)?,
            earth_damage_scale: num("earth_damage_scale", d.earth_damage_scale, 0.0, 10.0)?,
            earth_damage_cap: num("earth_damage_cap", d.earth_damage_cap, 0.0, 10.0)?,
            earth_fire_damage: num("earth_fire_damage", d.earth_fire_damage, 0.0, 10.0)?,
            earth_lava_damage: num("earth_lava_damage", d.earth_lava_damage, 0.0, 10.0)?,
            earth_flood_damage: num("earth_flood_damage", d.earth_flood_damage, 0.0, 10.0)?,
            earth_quake_damage: num("earth_quake_damage", d.earth_quake_damage, 0.0, 10.0)?,
            earth_storm_damage: num("earth_storm_damage", d.earth_storm_damage, 0.0, 10.0)?,
            earth_cold_damage: num("earth_cold_damage", d.earth_cold_damage, 0.0, 10.0)?,
            earth_swarm_damage: num("earth_swarm_damage", d.earth_swarm_damage, 0.0, 10.0)?,
            earth_crowd_scale: num("earth_crowd_scale", d.earth_crowd_scale, 0.0, 10.0)?,
            earth_crowd_effects: int("earth_crowd_effects", d.earth_crowd_effects, 0, 100)?,
            earth_ruin_scale: num("earth_ruin_scale", d.earth_ruin_scale, 0.0, 10.0)?,
            earth_events_per_beat: int("earth_events_per_beat", d.earth_events_per_beat, 0, 50)?,
            duel_one_per_player: boolean("duel_one_per_player", d.duel_one_per_player)?,
            duel_ai_lines_per_window: num("duel_ai_lines_per_window", d.duel_ai_lines_per_window, 0.0, 1.0)?,
            duel_ai_aggression: num("duel_ai_aggression", d.duel_ai_aggression, 0.0, 1.0)?,
            duel_ai_handicap: int("duel_ai_handicap", d.duel_ai_handicap, 0, 10)?,
            duel_picker: match m.get("duel_picker") {
                None | Some(Value::Null) => d.duel_picker.clone(),
                Some(Value::Bool(b)) => if *b { "on" } else { "off" }.into(),
                Some(Value::String(x)) if matches!(x.trim(), "on" | "off") => x.trim().to_string(),
                _ => return Err("`duel_picker` must be \"on\" or \"off\"".into()),
            },
            duel_picker_timeout_ms: int("duel_picker_timeout_ms", d.duel_picker_timeout_ms, 100, 5000)?,
            duel_picker_for_ai: boolean("duel_picker_for_ai", d.duel_picker_for_ai)?,
            duel_draws: int("duel_draws", d.duel_draws, 0, 10)?,
            duel_draw_sparks: int("duel_draw_sparks", d.duel_draw_sparks, 0, 10)?,
        };
        let s = Settings { start_bolts: s.bolt_start, ..s };
        let mut out = match serde_json::to_value(&s).map_err(|e| e.to_string())? {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        // f32 settings echo back at f32 precision (0.04, not 0.03999999910593033); reading it back gives the same f32.
        for k in F32_KEYS {
            if let Some(x) = out.get(*k).and_then(Value::as_f64) {
                out.insert((*k).into(), f32_decimal(x as f32).into());
            }
        }
        if s.seed.is_none() {
            out.remove("seed");
        }
        if s.city.is_none() {
            out.remove("city");
        }
        copy_timer_settings(&m, &mut out)?;
        if !out.contains_key("compose_seconds") {
            out.insert("compose_seconds".into(), (s.flow_ms as f64 / 1000.0).into());
        }
        if !out.contains_key("resolve_timeout_s") {
            out.insert("resolve_timeout_s".into(), DEFAULT_RESOLVE_TIMEOUT_S.into());
        }
        Ok(Value::Object(out))
    }

    /// Read validated settings (unknown keys such as the timers are ignored).
    pub fn from_value(v: &Value) -> Settings {
        let d = Settings::default();
        let mut s: Settings = serde_json::from_value(v.clone()).unwrap_or_else(|_| d.clone());
        if s.beats_total == 0 {
            s.beats_total = d.beats_total;
        }
        // Settings recorded before rules 0.6.0 have no circle schedule or killer multipliers: the old rules (the circle
        // closes on `beats_total`, multipliers 1), which is also what `fn7-society` reads when a key is absent.
        if v.get("arena_close_beats").is_none() {
            s.arena_close_beats = s.beats_total;
        }
        for (k, x) in [
            ("accident_scale", &mut s.accident_scale),
            ("army_strike_scale", &mut s.army_strike_scale),
            ("army_first_host_scale", &mut s.army_first_host_scale),
        ] {
            if v.get(k).is_none() {
                *x = 1.0;
            }
        }
        // Rules 0.8.1: settings recorded before challenge mode have no `challenge_enabled`: off, so a 0.8.0 recording
        // (schema 6 too) replays byte for byte (the built-in AI never challenges in it).
        if v.get("challenge_enabled").is_none() {
            s.challenge_enabled = false;
        }
        if v.get("rules_0_8_1").is_none() {
            s.rules_0_8_1 = false;
        }
        // Rules 0.9.0: settings recorded before "wills that matter" have no `wills_enabled`: off, so a 0.8.1 recording
        // replays byte for byte.
        if v.get("wills_enabled").is_none() {
            s.wills_enabled = false;
        }
        // Gear looks (schema 11): settings recorded before them have no `looks_enabled`: off.
        if v.get("looks_enabled").is_none() {
            s.looks_enabled = false;
        }
        // The Earth simulator (schema 13): settings recorded before it have no `earth_sim_enabled`: off.
        if v.get("earth_sim_enabled").is_none() {
            s.earth_sim_enabled = false;
        }
        // AI duellists type (schema 12): settings recorded before it have no `duel_ai_lines_per_window`: off.
        if v.get("duel_ai_lines_per_window").is_none() {
            s.duel_ai_lines_per_window = 0.0;
        }
        if v.get("duel_one_per_player").is_none() {
            s.duel_one_per_player = false;
        }
        // Duel weapons (schema 14): settings recorded before it have no `duel_picker`: off.
        if v.get("duel_picker").is_none() {
            s.duel_picker = "off".into();
        }
        // Fault junctions and tiers (schema 16): settings recorded before them have no `fault_junctions_enabled`: off.
        if v.get("fault_junctions_enabled").is_none() {
            s.fault_junctions_enabled = false;
        }
        // Settings recorded before flow/slow (schema 2) had only `beat_ms`.
        if v.get("flow_ms").is_none() {
            s.flow_ms = s.beat_ms.max(500);
            if v.get("arena").is_none() {
                s.arena = ArenaKind::City;
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_aliases_and_errors() {
        let v = Settings::validate(&Value::Null).unwrap();
        assert_eq!(v["beats_total"], DEFAULT_BEATS_TOTAL);
        // Rules 0.6.0's balance settings, echoed at f32 precision.
        assert_eq!(v["instant_order_debt"].to_string(), "0.5");
        assert_eq!(v["accident_scale"].to_string(), "0.82");
        assert_eq!((v["arena_close_beats"].as_u64(), v["debt_due_beats"].as_u64()), (Some(160), Some(50)));
        assert!(Settings::validate(&json!({"army_strike_scale": -1})).is_err());
        assert!(Settings::validate(&json!({"arena_close_beats": 0})).is_err());
        // Settings recorded before 0.6.0 read back as the old rules for the circle and the killers.
        let old = Settings::from_value(&json!({"beats_total": 30, "flow_ms": 3000}));
        assert_eq!((old.arena_close_beats, old.army_strike_scale, old.accident_scale), (30, 1.0, 1.0));
        assert_eq!(v["compose_seconds"], 3.0);
        assert_eq!(v["arena"], "planet");
        assert_eq!((v["flow_ms"].as_u64(), v["slow_ms"].as_u64()), (Some(3000), Some(30000)));
        assert!(Settings::validate(&json!({"arena": "moon"})).is_err());
        let c = Settings::validate(&json!({"arena": "city", "beat_ms": 6000})).unwrap();
        assert_eq!((c["flow_ms"].as_u64(), c["compose_seconds"].as_f64()), (Some(6000), Some(6.0)));
        assert_eq!(v["resolve_timeout_s"], DEFAULT_RESOLVE_TIMEOUT_S);
        assert!(v.get("seed").is_none());
        let v = Settings::validate(
            &json!({"rounds": 12, "beat_ms": 3000, "seed": 9, "compose_seconds": 4}),
        )
        .unwrap();
        assert_eq!(
            (v["beats_total"].as_u64(), v["seed"].as_u64()),
            (Some(12), Some(9))
        );
        assert_eq!(v["compose_seconds"], 4);
        let s = Settings::from_value(&v);
        assert_eq!((s.beats_total, s.beat_ms, s.flow_ms, s.seed), (12, 3000, 3000, Some(9)));
        let v = Settings::validate(&json!({"debt_scale": 0.04})).unwrap();
        assert_eq!(v["debt_scale"].to_string(), "0.04");
        assert_eq!(Settings::from_value(&v).debt_scale, 0.04f32);
        assert!(Settings::validate(&json!({"players": 0})).is_err());
        assert!(Settings::validate(&json!({"llm": "yes"})).is_err());
        assert!(Settings::validate(&json!({"seed": -1})).is_err());
        assert!(Settings::validate(&json!([1])).is_err());
        assert!(Settings::validate(&json!({"compose_seconds": 0})).is_err());
        // Rules 0.8.1: challenge mode is on for new matches, off for settings recorded before it.
        assert_eq!(v["challenge_enabled"], true);
        assert!(Settings::from_value(&v).challenge_enabled);
        assert!(!Settings::from_value(&json!({"beats_total": 30, "flow_ms": 3000})).challenge_enabled);
        assert_eq!(Settings::validate(&Value::Null).unwrap()["challenge_contact_km_houses"].to_string(), "0.15");
        assert_eq!(Settings::validate(&Value::Null).unwrap()["challenge_min_km"].to_string(), "120.0");
        assert!(Settings::validate(&json!({"duel_ai_chance": 2})).is_err());
    }

    #[test]
    fn allow_midmatch_join_passes_through() {
        // The core's room flag (mid-match join) survives validation (copy_timer_settings), as a boolean only.
        let v = Settings::validate(&json!({"allow_midmatch_join": true})).unwrap();
        assert_eq!(v["allow_midmatch_join"], true);
        assert_eq!(Settings::validate(&json!({"allow_midmatch_join": false})).unwrap()["allow_midmatch_join"], false);
        assert!(Settings::validate(&Value::Null).unwrap().get("allow_midmatch_join").is_none(), "absent = off");
        assert!(Settings::validate(&json!({"allow_midmatch_join": "yes"})).is_err());
        // Through the plugin, too (what the room stores and returns as match.started.rules).
        use theatre_plugin_api::GamePlugin;
        let p = crate::FinalNight7Plugin::new();
        assert_eq!(p.validate_settings(&json!({"allow_midmatch_join": true, "players": 4})).unwrap()["allow_midmatch_join"], true);
    }
}
