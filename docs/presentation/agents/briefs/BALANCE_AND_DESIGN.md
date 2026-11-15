# Brief: balance and design (Benito's design sessions)

*For Benito's gamedesign sessions and anyone tuning the game. Designers: Chris and Benito. Read
`../FULL_EXPLAINER.md` first; this is your subset.*

## 1. How tuning works (PLAYBOOK Play 3)

**Every number is a room setting. Never change code to balance; change settings.** Workflow: play with settings on
STABLE (`llm: false` for speed) → run batches with `POST /sim/run` / `POST /sim/compare` (up to 64 seeds) → send the
backend the settings you like → the backend makes them the defaults in the next release. `GET /sim/settings` lists
every key with its default, range and description (generated from the validator). With `llm: false` a seed's result
is deterministic. The simulator needs the host's admin token (never in a URL, never in a repo).

## 2. The core settings (API "Room settings")

| Setting | Default | Range | What it tunes |
|---|---|---|---|
| `beats_total` | 60 | 1-1000 | the Final Night's beat |
| `flow_ms` | 3000 | 500-120000 | beat length (the house lobby uses 12 s) |
| `slow_ms` | 30000 | 1000-600000 | the longest slow window |
| `bolt_start` / `spark_start` | 1 / 5 | 0-99 | the starting purse |
| `bolt_gift` / `spark_gift` | 1 / 0 | 0-10 | what every other player gets per mega |
| `entropy_per_mega` / `entropy_per_mini` | 10 / 1 | 0-100 / 0-20 | Entropy per cast |
| `entropy_scale` | 1.0 | 0-10 | multiplier on every Entropy rise ("4 = heat death four times as fast") |
| `debt_scale` | 0.04 | 0-10 | Consequence's bill per demigod per beat |
| `debt_due_beats` | 50 | 1-100 | beats until a bill falls due |
| `instant_order_debt` | 0.5 | 0-10 | Time's flat debt for orders without a window |
| `arena_close_beats` | 160 | 1-1000 | when Time's circle reaches 1,500 km |
| `accident_scale`, `army_strike_scale`, `army_first_host_scale` | 0.82 each | 0-10 | accidents, army damage, first volunteer hosts |
| `final_wish_magnitude` | 2 | 0-5 | the beat-1 boost |
| `manifest_scale` | 1.0 | 0-10 | the item engine's bolt price per tier |
| `demigods_per_player` | 2 | 1-2 | two bodies, or the old one-body game |

## 3. The DM's settings (DM_RULEBOOK §22)

**World (0.8.0):** `beats_per_day` 8; `dm_reach_km` 30; encounters `dm_encounters_per_beat` 1, `dm_encounters_max` 10,
`dm_encounter_fade_beats` 20, `dm_encounter_damage_scale` 1.0; `dm_settlement_step_beats` 6; roads `dm_road_wear` 4.0,
`dm_road_decay` 0.05; shrines `dm_shrines_per_demigod` 3, `dm_shrine_spark_every` 6, `dm_shrine_heal` 0.15,
`dm_shrine_ward` 0.1; `dm_memory_lines` 6.

**Fault lines:** `fault_near_km` 150 / `fault_far_km` 1500; `fault_near_magnitude` 3 / `fault_far_magnitude` -2;
`fault_near_radius` 1.5 / `fault_far_radius` 1.0; `fault_near_damage` 1.6 / `fault_far_damage` 0.6; junctions
`fault_junctions_enabled` true, `fault_junction_km` 300, `fault_junction_magnitude` 3, `fault_junction_radius` 1.5,
`fault_junction_damage` 1.8; `fault_tiny_mod_km` 150.

**Summons and god powers:** `summon_enabled` true, `summon_cooldown_beats` 1, `summon_loss_scale` 1.0;
`god_power_cost` 3, `god_power_gift` 1.

**Duels (0.8.1):** `challenge_enabled` true; `challenge_min_km` 120; `challenge_contact_km_regions` / `_villages` /
`_houses` / `_people` 600 / 15 / 0.15 / 0.015; `duel_cooldown_beats` 1; `duel_deadline_ms` 30000; `duel_damage_scale`
1.0; `duel_min_gap_ms` 500 (spam); `duel_followup_ms` 1500; `duel_counter_ms` 3000; `duel_ai_chance` 0.02;
`duel_ai_reach_km` 2000; `duel_auto_rounds` 2.

**Wills (0.9.0):** `wills_enabled` true; `will_spark_per_bolt` 5; `will_own_dc` 6; `will_debt_cap` 3.0;
`will_debt_per_spark` 0.2; `will_entropy_per_spark` 0.25; heals `heal_price_sparks` 1, `heal_token` 0.5, `heal_scale`
1.0; boons `boon_beats` 6, `boon_bonus` 1; arming `arm_price_artifact` 2, `arm_price_relic` 5, `arm_bless_sparks` 5,
`arms_scale` 1.0; raising `raise_price_sparks` 5, `raise_stale_beats` 8, `raise_cap` 3, `raise_years` 1.0,
`raise_health_fraction` 0.4; curses `curse_price_sparks` 5, `curse_god_price_sparks` 15, `curse_beats` 12,
`curse_dc` 10, `curse_max_per_player` 2, `curse_entropy` 3.0; quests `quest_steps_max` 3, `quest_expire_beats` 24,
`quest_spark_step` 2; `wills_ai_chance` 0.25.

**Looks:** `looks_enabled` true; `look_extra_items` 2; `look_items_shown` 3; `look_images_per_demigod` 4;
`look_debounce_beats` 3; `item_art_per_match` 12.

Also: `earth_*` (EARTH_SIM §8), `token_play` (`all` / `humans` / `off`), `token_play_llm` (off by default). Every
other setting: `docs/PROJECT_GUIDE.md` §3.13.

## 4. What is measured

`GET /sim/jobs/{id}` returns per seed: the champion, beats played, eliminations by beat and cause, casts and bolts
spent, the Entropy curve, slow bills, debts paid, society totals, timing; plus mean / median / min / max.

Measured at today's defaults (BALANCE_REPORT_2, 256 seeds, `llm: false`, labels [M] measured / [E] estimate):

- [M] Median match **57 beats** (P10-P90 41-60), about 7.1 min with the LLM at 7.5 s a beat. 44% of matches reach the
  Final Night (4% by heat death).
- [M] Eliminations: debt 32%, armies 25%, accidents 19%, the clock 9%, other 15%; 2.38 distinct causes per match.
  Champion seats balanced (chi-squared 3.59, under the 5% critical value).
- [M] Bolts: a median of 82 on the table at the end; casts per match median 12; fizzles 0.01 per match.
- [M] "Frantic casting ends the world in about 30 s": four seats casting at the client's maximum rate reach heat death
  at the 2nd-3rd resolve.
- [E] On the default Final Night the circle is about 57% closed: a design point to confirm.
- Duels (RB 21.8, 400 simulated duels): a human typing good lines beats the AI about 9 in 10 at 84 wpm, 2 in 3 at
  60, 1 in 3 at 42; typing nothing loses every time.
- Fault lines (RB 13.1): a mega volcano at the Afar triple junction lands at damage 11.8, radius 1,789 km; in central
  Australia damage 0.8, radius 240 km.

## 5. Open design questions in Benito's court (FOR_BENITO §4)

The duel's look (box, stage, mannequins, what a hit feels like); which extra skins (each about 10 Meshy credits) and
whether a skin is earned, chosen or from the story; missing move classes and what beats what; what each kind of place
always has on a duel stage; ten mystic quests in his own voice; anything wrong in his eight features (settlements,
the piece's story, the table d20, encounters, loot, day and night, roads, shrines).

## 6. Never

- Never change code to balance; never ask an agent to "just tweak" a rule in code.
- Never present an estimate as a measurement (label [M] / [E]).
- Never paste the admin token or access key anywhere but the backend.

## 7. Read

`../FULL_EXPLAINER.md`; server repo `docs/DM_RULEBOOK.md` §22 and the chapters each setting names,
`docs/BALANCE_REPORT_2.md`, `docs/AGENT_GUIDE.md` §12 (the simulator), `docs/CLIENT_API.md` "Room settings";
Unreal repo `docs/FOR_BENITO.md`, `docs/design/EVENTS_IN_THE_WORLD.md`.
