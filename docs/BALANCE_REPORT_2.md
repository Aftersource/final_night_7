# Final Night 7: Balance Report 2

For Chris and Benito. Written by the simulation side on 2026-11-14. It follows `docs/BALANCE_REPORT_1.md`: the
designers approved the **default+** package plus rule changes **R1–R4**. This report covers what was built (rules
**0.6.0**), the re-measurement, and the new defaults.

Labels: **[M] measured** in the simulator, **[E] estimate** (arithmetic from measured rates, not simulated),
**[J] judgment**.

---

## 0. Summary

1. **[M] The target is met at the new defaults** (256 seeds, `llm: false`). The median match is **57 beats**
   (P10–P90 41–60), about **7.1 min with the LLM** at 7.5 s per beat. The median last elimination comes at beat 57.
   **44% of matches reach the Final Night** (including 4% by heat death; Report 1's default+ reached it 5% of the time,
   all heat death). Eliminations spread across debt 32%, armies 25%, accidents 19%, the clock 9% and other causes 15%,
   with 2.38 distinct causes per match. Champion seats are 67 / 58 / 75 / 56 (χ² 3.59, under the 5% critical value
   of 7.81).
2. **The new defaults:** `beats_total` **60**, `arena_close_beats` **160**, `debt_due_beats` **50**,
   `instant_order_debt` **0.5**, and `accident_scale` = `army_strike_scale` = `army_first_host_scale` = **0.82**.
3. **[M] Report 1 paired R1 with `debt_due_beats` around 10–15. That doesn't hold after R1–R4.** At due 10–25, debt
   is again the top killer (1.2–1.5 of 3 eliminations) and matches end at beat 31–43. Due 50 is what makes debt land
   late: 0.42 debt eliminations per match before the Final Night and 0.52 on it, when every outstanding debt is
   collected.
4. **[M] Time's circle has to run on a long schedule.** With `arena_close_beats` 60 the clock ends matches by beat
   32–40 at every `debt_due_beats` tried, and it takes up to 1.16 eliminations per match. With 160 it takes 0.28. On
   the default Final Night (beat 60) the circle is therefore about 57% closed [E]. That's a design point to confirm
   (§6).
5. **[M] R3 ended the bolt flood.** The table holds a median of 82 bolts at the end (Report 1's default+: 347).
   Casts per match are unchanged (median 12) and almost never fizzle (0.01 per match). The built-in AI still pays
   Time with a bolt for 84% of its orders.
6. **[M] "Frantic casting ends the world in about 30 s" still holds after R3** at `entropy_scale` 1, now with a
   regression test (`frantic_casting_ends_the_world_in_about_30_seconds`). Four seats casting at the client's maximum
   rate reach heat death at the **2nd resolve** with the mixed palette (Entropy 77.6 → 100) and at the **3rd** with
   only the 1-bolt powers (44.8 → 83.2 → 100). At 12 s beats that is 24 s and 36 s.

## 1. What changed in the rules (0.6.0)

| | Rule | Where | Setting |
|---|---|---|---|
| **R1** | An order without a slow window (bots, the built-in AI, Compose submits that skip `action.begin`) costs Time a **flat debt**, with no repeat factor and no seconds factor. It is still doubled when the bolt can't be paid. Real windows keep `0.5 × (1 + secs/10) × (1 + 0.5 × recent)`. Instant orders still count as slows in a later window's repeat count. | `fn7-plugin/src/flow.rs::bill` | `instant_order_debt` (0.5, range 0–10) |
| **R2** | **Time's circle runs on its own schedule.** It reaches its end radius (1,500 km on the planet) at `arena_close_beats`, and its damage ramp (`0.5 + 1.5 × progress + 4 × overshoot`) uses the same progress. The Final Night still falls on `beats_total`, at heat death, or when nobody is standing. A circle scheduled past the Final Night is still closing when the Final Night falls. | `fn7-society/src/rules.rs` (`arena_close_beats`, `arena_progress`, `arena_radius_at`), `fn7-plugin/src/phases.rs` | `arena_close_beats` (160, range 1–1000) |
| **R3** | **Time's toll gives no gift.** The bolt paid to Time for an order or a window doesn't give rivals +2. It still counts as a spend for Entropy (+0.4), so Entropy's handling is unchanged. Cast bolts and invocation bolts still give the gift, and so do bribes. | `Effect::Bolts { toll }` (`fn7-model`), `spend_bolts` (`fn7-society/src/effects.rs`) | — |
| **R4** | The non-debt killers become room multipliers: the rules fallback's accident chance (1 in 20 per non-attack act × scale), army strike damage on a demigod (× scale, applied after the cap of 2), and the extra chance (0.2) that a crowd near an unarmed demigod raises a first host (× scale). | `fn7-agents/src/resolver.rs`, `fn7-society/src/sim/armies.rs` | `accident_scale`, `army_strike_scale`, `army_first_host_scale` (0.82 each, range 0–10) |

- `RULES_VERSION` is now **0.6.0**. Mid-match join had also landed since 0.5.0.
- **Old states and recordings.** A settings object without the new keys reads back as the old rules for the circle
  and the killers: the circle closes on `beats_total` and the multipliers are 1. Both the plugin
  (`Settings::from_value`) and the society (`setting_f64` fallbacks) do this. R1 and R3 apply to everything. The
  `toll` flag is omitted from JSON when false, so recorded effects parse unchanged. Determinism and replay are
  intact: all workspace tests pass, including the replay tests.
- **Accidents only apply to the rules fallback.** With the LLM on, the resolver decides accidents itself, and
  `accident_scale` doesn't touch that path.

## 2. Method

- **Before:** a release build of `a413609` (main, clean), copied aside. Its defaults reproduce Report 1's build B
  exactly (27 [18–39], seats 77/54/68/57).
- **After:** release builds of the working tree, served by a private `fn7-server` on `127.0.0.1:8096–8098` with
  `THEATRE_RECORD=off`, no API key and `llm: false`. Jobs went through `POST /sim/run`. Ports 8080 and 8081 and ngrok
  were not touched, and nothing was recorded. The same seeds were used in every arm: 1–64 per arm, and 1–256 for the
  before-defaults, the before-default+ and the final defaults.
- **Statistics:** "mean/median [P10–P90]". The cause columns count eliminations per match. "Final Night %" counts
  matches that ran to `beats_total` or hit heat death. Minutes [E] = beats × 7.5 s (LLM on).
- **The "nat" arms** set `beats_total` 200 so that the Final Night is never reached. They show where a match ends on
  its own. The `bt60` / `bt62` arms then confirm the real cut-off. Before the Final Night everything is identical
  except the AI's "late game" heuristic (beat ≥ 0.6 × `beats_total`, §6).

## 3. Before and after

[M] 256 seeds each.

| | **Before: defaults** (0.5.0) | **Before: default+** (0.5.0, `dd` 50, `bt` 130) | **After: new defaults** (0.6.0) |
|---|---|---|---|
| Match length, median [P10–P90] (mean) | 27 [18–39] (27.8) | 52 [38–63.5] (51.1) | **57 [41.5–60] (54.2)** |
| Minutes with the LLM, median [P10–P90] [E] | 3.4 [2.2–4.9] | 6.5 [4.8–8.0] | **7.1 [5.1–7.5]** |
| First elimination, median [P10–P90] | 13 [11–20] | 28 [14–43.5] | 30 [15–48.5] |
| Last elimination, median | 27 | 52 | **57** |
| Final Night reached (incl. heat death) | 0% | 5% (all heat death) | **44%** (4% heat death) |
| Eliminations per match: debt / army / accident / arena clock / other | 2.06 / 0.26 / 0.37 / 0.07 / 0.25 | 0.52 / 0.91 / 0.74 / 0.41 / 0.42 | **0.95 / 0.75 / 0.58 / 0.28 / 0.43** |
| Distinct causes per match | 1.85 | 2.40 | **2.38** |
| Champion seats 0/1/2/3 (χ²; 7.81 = 5% critical) | 77/54/68/57 (5.22) | 74/66/59/57 (2.78) | **67/58/75/56 (3.59)** |
| First out by seat (χ²) | 58/56/75/67 (3.59) | 64/64/72/56 (2.0) | 62/77/55/62 (4.03) |
| Entropy max, median; reaches 40 / 80 (median beat) | 36.5; 27% / 0% | 67; 96% (27) / 21% (47) | 67; 95% (31) / 24% (51) |
| Casts per match, median; fizzles | 6; 0 | 12; 0 | 12; 0.01 |
| Bolts held at the end (all four), median | 209 | 347 | **82** |
| Time's debt billed per match (mean) | 49.6 | 130.6 | **38.0** |
| Orders per match; paid with a bolt | 31.5; 100% | 60.6; 100% | 65.6; 84% |
| Battles per match (mean) | 28.7 | 83.6 | 91.1 |

[M] Where the after-defaults eliminations happen (256 seeds):
- **Before the Final Night:** army 0.75, accident 0.57, debt 0.42, arena clock 0.28, other 0.40 per match.
- **On the Final Night beat:** 0.56 per match, nearly all of them debt (134 of 144 eliminations). Outstanding debt
  is collected there and the survivor rule keeps the lowest debt, so Consequence's bill decides the Final Night, as
  the BUILDLOG intended.

## 4. The sweeps (64 seeds per arm)

Every 0.6.0 arm uses `instant_order_debt` 0.5 and multipliers 0.82 unless it says `ks1` (multipliers 1).

| arm | beats mean/med [P10–P90] | 1st elim med [P10–P90] | last elim med | debt | army | accident | arena_clock | other* | kinds/match | Final Night % | heat death % | ent max med | casts med | bolts end med | Time debt/match | battles | champ seats (χ²) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **R1+R3 only** (old `dd` 6, `bt` 100, circle 100, ks1) | 33.9/34 [20–49] | 16 [10–26] | 34 | 1.44 | 0.44 | 0.58 | 0.33 | 0.21 | 2.30 | 0% | 0% | 42 | 8 | 50 | 21.6 | 41 | 12/18/16/18 (1.5) |
| **R1+R3 only** (default+: `dd` 50, `bt` 130, circle 130, ks1) | 53.4/54 [35–68] | 26 [11–44] | 54 | 0.48 | 0.94 | 0.81 | 0.36 | 0.40 | 2.27 | 0% | 0% | 63 | 13 | 87 | 34.5 | 88 | 14/15/15/20 (1.4) |
| bt65 ac60 dd10 | 31.3/31 [24–39] | 21 [12–27] | 31 | 1.38 | 0.39 | 0.38 | 0.61 | 0.25 | 2.25 | 2% | 0% | 42 | 7 | 70 | 20.9 | 37 | 14/17/17/16 (0.4) |
| bt65 ac60 dd15 | 32.8/32 [25–41] | 22 [12–29] | 32 | 1.17 | 0.52 | 0.30 | 0.77 | 0.25 | 2.27 | 0% | 0% | 43 | 7.5 | 74 | 22.3 | 40 | 9/16/18/21 (4.9) |
| bt65 ac60 dd15 ks1 | 32.2/31.5 [25–41] | 21 [10–29] | 32 | 1.22 | 0.47 | 0.47 | 0.56 | 0.28 | 2.34 | 0% | 0% | 43 | 7.5 | 66 | 21.5 | 38 | 7/19/18/20 (6.9) |
| bt65 ac60 dd25 | 36.0/35.5 [28–44] | 24.5 [12–34] | 36 | 0.77 | 0.56 | 0.47 | 0.88 | 0.33 | 2.33 | 2% | 0% | 49 | 9 | 82 | 24.0 | 46 | 13/15/19/17 (1.3) |
| bt65 ac60 dd50 | 40.3/39.5 [28–54] | 24.5 [12–39] | 40 | 0.12 | 0.75 | 0.44 | 1.16 | 0.53 | 2.25 | 0% | 0% | 54 | 10.5 | 86 | 26.5 | 57 | 15/15/15/19 (0.8) |
| bt130 ac60 dd50 | *identical to bt65 ac60 dd50* | | | | | | | | | | | | | | | | |
| bt60 ac100 dd15 | 39.6/39 [29–51] | 22.5 [12–32] | 39 | 1.55 | 0.47 | 0.41 | 0.25 | 0.34 | 2.22 | 3% | 0% | 50 | 10 | 68 | 25.7 | 54 | 15/17/19/13 (1.3) |
| bt60 ac100 dd25 | 42.3/43 [31–56] | 27 [12–36] | 43 | 1.19 | 0.53 | 0.52 | 0.42 | 0.34 | 2.39 | 5% | 0% | 53 | 11 | 79 | 28.2 | 59 | 16/15/16/17 (0.1) |
| bt60 ac100 dd40 | 46.7/47 [37–60] | 29 [12–43] | 47 | 0.78 | 0.80 | 0.59 | 0.44 | 0.39 | 2.36 | 11% | 0% | 61 | 12.5 | 88 | 30.8 | 69 | 16/14/17/17 (0.4) |
| nat ac100 dd50 | 49.9/51.5 [37–63] | 29 [12–45] | 52 | 0.41 | 0.84 | 0.75 | 0.55 | 0.45 | 2.47 | 0% | 0% | 62 | 13 | 87 | 32.1 | 78 | 18/13/15/18 (1.1) |
| nat ac130 dd40 | 50.5/49 [42–62] | 29 [13–43] | 49 | 0.72 | 0.86 | 0.56 | 0.45 | 0.41 | 2.47 | 0% | 0% | 63 | 12 | 88 | 33.6 | 79 | 11/20/15/18 (2.9) |
| nat ac130 dd50 | 54.5/54 [43–68] | 29 [13–47] | 54 | 0.50 | 0.94 | 0.45 | 0.55 | 0.56 | 2.41 | 0% | 0% | 67 | 12.5 | 86 | 35.8 | 89 | 9/21/15/19 (5.3) |
| nat ac130 dd60 | 56.8/56 [43–71] | 29 [13–47] | 56 | 0.33 | 0.97 | 0.59 | 0.56 | 0.55 | 2.33 | 3% | 3% | 68 | 13.5 | 83 | 36.9 | 97 | 10/19/16/19 (3.4) |
| nat ac160 dd50 | 58.2/59.5 [40–72] | 29 [12–50] | 59.5 | 0.77 | 0.84 | 0.62 | 0.33 | 0.44 | 2.55 | 6% | 6% | 71 | 14 | 90 | 38.3 | 101 | 19/15/13/17 (1.3) |
| nat ac160 dd60 | 61.4/64 [40–76] | 29 [12–50] | 64 | 0.58 | 0.98 | 0.64 | 0.34 | 0.45 | 2.45 | 12% | 12% | 76 | 14.5 | 93 | 40.0 | 110 | 18/13/18/15 (1.1) |
| nat ac200 dd50 | 56.3/56.5 [41–72] | 33 [12–51] | 56.5 | 0.69 | 0.83 | 0.81 | 0.22 | 0.45 | 2.30 | 6% | 6% | 67 | 15 | 101 | 36.9 | 95 | 20/14/20/10 (4.5) |
| bt62 ac160 dd50 | 55.3/59 [40–62] | 29 [12–50] | 59 | 0.98 | 0.78 | 0.56 | 0.25 | 0.43 | 2.39 | 45% | 3% | 70 | 12.5 | 95 | 37.0 | 93 | 18/17/14/15 (0.6) |
| **bt60 ac160 dd50** (chosen) | 54.4/59 [40–60] | 29 [12–50] | 59 | 1.03 | 0.78 | 0.55 | 0.25 | 0.39 | 2.38 | 52% | 3% | 69 | 12.5 | 95 | 36.6 | 90 | 17/17/14/16 (0.4) |

\* other = `final_night` + `god_power` + `mod` + `other`.

[M] Final Night share if `beats_total` were cut at N, read from the "nat" arms (an approximation; see §2):

| nat arm | N = 55 | 60 | 62 | 65 | 70 |
|---|---|---|---|---|---|
| ac100 dd50 | 31% | 14% | 11% | 8% | 2% |
| ac130 dd50 | 36% | 27% | 20% | 14% | 6% |
| ac130 dd60 | 56% | 42% | 36% | 25% | 12% |
| ac160 dd50 | 62% | 42% | 36% | 28% | 16% |
| ac160 dd60 | 70% | 64% | 56% | 45% | 23% |
| ac200 dd50 | 52% | 30% | 27% | 19% | 12% |

What the sweeps show:
- **[M] R1+R3 on their own move the old defaults from 27 to 34 beats.** Time's debt drops from 49.6 to 21.6 per
  match, but at due 6 debt still takes 1.44 eliminations: a demigod already worn down by other damage dies to a
  small bill. R1 changes how big the bills are. `debt_due_beats` still decides when they land.
- **[M] R2 works as built.** `bt65 ac60 dd50` and `bt130 ac60 dd50` are identical, seed for seed: `beats_total` no
  longer moves the circle. The circle's own schedule is now the strongest length lever: at due 50, 60 → 100 → 130 →
  160 takes the natural median from 39.5 to 51.5, 54 and 59.5.
- **[M] Circle 200 is no better than 160.** The clock almost stops killing (0.22), and accidents take its place
  (0.81).
- **[M] R4 at 0.82 vs 1** (`dd15` arms, the only same-seed pair): accidents 0.47 → 0.30, while army eliminations are
  flat within noise (0.47 → 0.52). It's a small arm. The before/after default+ comparison (accidents 0.74 → 0.58,
  armies 0.91 → 0.75 per match) also mixes in R1–R3.
- **[M] Due 60 instead of 50** gives a longer match (natural median 64) and a 64% Final Night share at `beats_total`
  60. But debt drops to 0.58 per match, heat death rises to 12%, and the median then sits at the cut-off itself.

## 5. Why these defaults

- **`beats_total` 60.** [M] The median match is 57 beats (7.1 min [E]) and 44% of matches (256 seeds) reach the Final
  Night. At 62 the median is the same (59 in the 64-seed arm) and the share falls to 45% in that arm (from 52% at 60).
  Going higher only trades Final Nights for last-standing endings.
- **`arena_close_beats` 160.** [M] It is the shortest schedule that lets a match's natural end land past beat 60 at
  due 50 (natural median 59.5). Shorter schedules make the clock the match clock again: 0.55 clock eliminations at
  100–130, and up to 1.16 at 60. It still takes 0.28 eliminations per match and keeps pushing demigods together.
- **`debt_due_beats` 50** (the default+ value, kept). [M] It is the only value tried that keeps debt a real but not
  dominant killer before the Final Night (0.42 per match) while making it decide the Final Night (0.52). At 10–25
  debt dominates again and matches end at 31–43. At 60 debt fades (0.33–0.58) and heat death grows.
- **`instant_order_debt` 0.5.** As approved. [M] Time's bill falls from 130.6 to 38.0 per match for the same order
  volume (default+ vs. now). The bill no longer escalates with engagement.
- **Multipliers 0.82.** As approved (an 18% softening). [J] Left at the approved value. The circle schedule did the
  heavy lifting, so there was no need to soften the killers further. Further tuning is a settings change (Play 3).

## 6. Flags for the designers

1. **The circle is only partly closed on the default Final Night.** [E] At beat 60 of a 160-beat schedule (ease-out
   1.8) the circle has covered about 57% of its shrink: about 9,450 km from a 20,037 km start. If "the circle closes
   on the Final Night" matters visually, the options are a stronger ease (a code change), or a shorter
   `arena_close_beats` traded against a higher `debt_due_beats` and lower multipliers. That combination wasn't
   measured.
2. **Short rooms must set `arena_close_beats` too.** The shared lobby is 30 beats (`MatchRounds=30`). With the default
   circle of 160 its arena would barely shrink (about 31% of the way by beat 30 [E]). The Unreal room settings should send
   `arena_close_beats` (for example 30, the old behavior, or about 80). The client team needs to know before the next
   lobby build.
3. **Unpaid instant orders are still doubled** (1.0 instead of 0.5). R3 makes bolts scarcer, so this now bites: the
   AI paid a bolt for 84% of its orders, down from 100%. That was the report's intent for R3; confirm it's wanted.
4. **Instant orders still count toward a real window's repeat factor.** A player who sends many window-less orders and
   then opens a window pays the escalation. The approved R1 kept real windows on the current formula, so this was
   left alone. Changing it is one line.
5. **The AI's "late game" heuristic** (`actions.rs`: walk toward the center from beat 0.6 × `beats_total`) and the
   resolver's pacing line in the LLM prompt still key on `beats_total`, not on the circle. With `beats_total` 60
   that's beat 36. The new defaults were measured with that behavior. Moving either onto the circle's schedule would
   change results.
6. **R5 is still open.** All of this is tuned to the built-in AI's order rate (65.6 instant orders per match at
   the new defaults, all of them window-less). Humans who hold windows open pay the full formula (`1 + secs/10`, repeats).
   Measure human order rates in a playtest before trusting the debt numbers for human seats.
7. **Accidents with the LLM on** aren't covered by `accident_scale`: the LLM resolver decides accidents itself. The
   multiplier only softens the rules fallback (sims, timeouts).
8. **Heat death is a 4% ending at scale 1** in a 60-beat match. Level 80 ("godfall") comes at a median of beat 51 in
   24% of matches. [J] That's the escalation arc Report 1 described.
9. **Docs correction:** MECHANICS and PROJECT_GUIDE said the clock does 1–5 damage. The code clamps it to 0.5–3. Both
   now state the code's numbers.

## 7. The ~30-second calibration (re-checked after R3)

[M] This is now a test, `fn7-plugin/src/tests.rs::frantic_casting_ends_the_world_in_about_30_seconds`. It runs four
seats at `entropy_scale` 1 with `flow_ms` 12000, each casting at the client's maximum (2 per second, burst 4: 28 casts
in beat 1, 24 per beat after), interleaved by `seq`, at open ocean so nobody dies. No orders are sent, so R1 and R3
don't enter.

| Casting | Entropy after resolve 1 / 2 / 3 | Heat death | ≈ seconds |
|---|---|---|---|
| Mixed palette (cycling all 11 powers) | 77.6 / 100 | 2nd resolve | 24 |
| Only the 1-bolt powers | 44.8 / 83.2 / 100 | 3rd resolve | 36 |

These match Report 1's model [E] (93 / 174 and 45 / 83 / 122). The mixed run is a little slower because the
interleaving makes some expensive casts fizzle early. The test asserts heat death at the 2nd or 3rd resolve, never
the 1st. R3 doesn't touch this path: casts keep their +2 gift, and that gift is what makes a frantic table
self-funding.

## Appendix: reproducing

```sh
# private server, no LLM, no recording; then POST /sim/run {"seeds": [1..64], "llm": false, "settings": {...}}
THEATRE_BIND=127.0.0.1:8096 THEATRE_ADMIN_TOKEN=<random> THEATRE_RECORD=off FN7_SIM_WORKERS=16 \
  FN7_SIM_LOG_DIR=<scratch> ./target/release/fn7-server
```

Arm labels: `btN` = `beats_total`, `acN` = `arena_close_beats`, `ddN` = `debt_due_beats`, `ks1` = all three
multipliers 1, `nat` = `beats_total` 200. The final defaults are `{}` (no overrides) on seeds 1–256.
