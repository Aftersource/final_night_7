# Final Night 7: Balance Report 1

For Benito (design, balance, UX). Written by the simulation side on 2026-11-14. This is measurement and
recommendation only. No game code or settings defaults were changed.

Every number here is **[M] measured** in the simulator unless it's marked **[E] estimate** (arithmetic from measured
rates, not simulated) or **[J] judgment** (our design opinion).

---

## 0. Summary

1. **[M] Time kills the demigods, not Consequence.** At defaults, debt causes 65–70% of all eliminations, and about
   **97% of the debt billed is Time's bill for orders**, not Consequence's per-act bill. Per match, Time bills
   **49.0** debt points and Consequence bills **1.5**. So `debt_scale` does almost nothing: at `debt_scale` 0, matches
   are as long and as debt-heavy as at 0.04.
2. **[M] Why:** the built-in AI gives orders on **0.35 of its beats**. In a simulation every order is billed as a
   zero-second slow: `0.5 × (1 + 0.5 × slows in the last 20 beats)`, doubled when unpaid. The repeat factor averages
   about ×3, so each demigod accrues about **0.55 debt per beat**, roughly 10 health in about 18 beats. That debt
   falls due 6 beats later. Default matches end at a median of **beat 28** (P10–P90: 17–41).
3. **[M] `debt_due_beats` is the only setting that moves match length much.** Going from 6 to 40 or more takes the
   median from 28 to 46–49 beats and pushes the first elimination from beat 13 to 25. Debt drops to 0.1–0.5
   eliminations per match, armies, accidents and the arena clock share the rest, and society activity doubles to
   triples. Beyond about 50 the curve is flat: armies, accidents and Time's arena clock then end the match by
   **beat ~49**, whatever `beats_total` says.
4. **[M] With settings alone, no package reaches the Final Night.** In every arm with players acting, 100% of matches
   end "last standing" before `beats_total`. With `beats_total` 60 the arena shrinks faster, so matches get *shorter*
   (median 39 at best, P90 53–54). **The target "beats_total ≈ 60, last elimination near 55–60" can't be reached
   with today's settings. It needs a rules change** (§9).
5. **[M] The best settings-only fit for the ~7-minute feel** is `debt_due_beats` 50 with `beats_total` 130. Over
   192 seeds it gives a median of 53 beats (P10–P90: 36–64), which is about **6.6 min with the LLM** (7.5 s per
   beat; P10–P90 4.5–8.0) and 2.6 min rules-only. The first elimination comes at about beat 28. Its causes spread
   out (2.4 distinct causes per match), there are about 85 battles and 18 generals per match, and seats are fair. Its weakness: the Final Night is never reached, so the "one survivor on the Final Night" moment
   never happens (§8).
6. **[M] Bolts flood.** Paying Time for an order is a bolt spend, so every order gives each rival +2. The table ends
   a match holding **~210 bolts** at defaults and ~330 in long matches, against 3 at the start. Casting is limited by
   how often the AI chooses to cast (0.25 casts per beat, never a fizzle), not by bolts. `start_bolts` ≥ 1 changes
   nothing measurable. **`start_bolts` 0 is a trap:** nobody ever gets a bolt, nobody casts, and every order bill is
   doubled.
7. **[M] Entropy at scale 1 reaches 20 at about beat 14 and 40 at about beat 28.** Levels 60 and 80 and heat death are
   only reached in long matches or at `entropy_scale` ≥ 2. With scale 2 to 3, heat death becomes a real ending
   (2–64% of matches).
8. **[M] Seats are fair** (256 seeds: 77 / 54 / 68 / 57, χ² 5.2, under the 5% critical value of 7.81). The "seat 0 is weak" signal in
   16- and 64-seed samples is sampling noise; it reverses in other seed blocks. The health leader at mid-match wins
   about half the time (chance is 25%); at 75% of the match, about 58%.
9. **[E] "Everyone casting everything ends the world in ~30 s" already holds at `entropy_scale` 1.0** (a model of the
   cast rules, §8b). Four players casting at the client's maximum rate (2/s) hit heat death at the 2nd resolve with a
   12 s beat (about 24–32 s). Spamming only the cheapest powers takes 3 resolves (about 36 s). Normal play peaks
   around Entropy 36. **No rules change is needed; keep `entropy_scale` ≤ 1.25.** This needs a live check with
   scripted casters.

---

## 1. Method

- **Build B (primary):** the current working tree, with the society polish in `fn7-society` and
  `data/society_template.json` and the media changes. HEAD is `51f0d48` plus uncommitted changes. Built with
  `cargo build --release -p fn7-server --bins`, then copied to a private folder so later rebuilds couldn't change
  the binary mid-study.
- **Build A (superseded):** the release binary from 05:08 (game logic = `95c1511`), from before the society polish.
  It was used for the first sweeps; the wider sweeps that weren't repeated on B are in Appendix B. The headline
  numbers moved little between A and B (default median length 29 → 28, debt eliminations 2.1 → 1.9), but society
  activity rose a lot (battles 3.9 → 30.5 per match).
- **Runner:** a private `fn7-server` on `127.0.0.1:8095` with my own admin token, `THEATRE_RECORD=off`, no API key
  (`llm: false`, every mind on its rules fallback), and `FN7_SIM_WORKERS=12`. Jobs went through `POST /sim/run`. The
  stable server (`:8080`), `:8081` and ngrok weren't touched.
- **Per-beat health and debt by god:** these aren't in the sim API, so I used `fn7-sim` from the same build with an
  events-level recording to a scratch folder. Each recording was parsed and deleted at once (about 17 MB each, none
  kept). For all 64 seeds, `fn7-sim`'s champion matched the server's.
- **Seeds:** 64 per arm (seeds 1–64; the same seeds in every arm, so arm differences are the settings' doing). The
  defaults and the three packages also ran on seeds 65–192 or 65–256 (192–256 matches).
- **Statistics:** "mean / median [P10–P90] (min–max)" across matches. The cause classes are the sim API's
  `cause_kind`. "Other" in the tables is `god_power` + `mod` + `bolt` + `entropy` + `final_night` + `other`.
- **Minutes [E]:** beats × seconds per beat. **LLM on: 7.5 s per beat** (the coordinator's live measurement: 3 s
  compose plus 4.3–5.3 s resolve). **Rules only: 3.0 s per beat** (the compose window; the sim's resolve is about
  5 ms).
- **Caveats:**
  - All four demigods are the built-in AI (`bot_lines`). Humans will order at a different rate and hold slow windows
    open, which costs *more* (`1 + secs/10`).
  - With the LLM on, the resolver and generals decide differently. The rules fallback has a flat 1-in-20 accident
    chance per non-attack act.
  - Real slow windows don't exist in the sim: every order is an instant order.

## 2. The debt mechanism (dig into the known finding)

**Time's bill** (`fn7-plugin/src/flow.rs::bill`), charged for every order:

```
debt    = 0.5 × (1 + secs_open/10) × (1 + 0.5 × recent) × (2 if no bolt to pay Time else 1)     (cap 10)
recent  = this demigod's other slows in the last 20 beats (SLOW_REPEAT_BEATS)
due_in  = debt_due_beats      (the same setting as Consequence's bill; debt_scale does NOT apply)
also:   1 bolt paid to Time (counts as a spend: every rival +2 bolts, Entropy +0.4), Entropy +0.5
```

**Consequence's bill** (`beat.rs`): `debt_scale × Σ debt_for(magnitude, words)` over the beat's landed acts. At
0.04 that's a few hundredths per beat.

| [M] build B, 64 seeds, defaults | per match |
|---|---|
| Orders given (all instant, `slow.instant_orders`) | 31.5 (P10–P90 23–40) |
| AI order rate | **0.354 orders per alive-demigod-beat** (one every ~2.8 beats) |
| Debt billed to **Time** | **49.0** (median 47.2) |
| Debt billed to **Consequence** | **1.5** (median 1.4) |
| Time debt per alive-demigod-beat | 0.548 (implied mean repeat factor ×3.1) |
| Eliminations by debt | 1.94 of 3 |

**[M] The `debt_scale` sweep shows it.** Build B: 0 / 0.04 / 0.1. Build A: 0 / 0.02 / 0.03 / 0.04 / 0.06 / 0.1 /
0.2 / 0.4. Debt eliminations stay between 1.9 and 2.1 up to `debt_scale` 0.2 (2.4 at 0.4), and the median length goes
30 → 28 → 27 (only at 10× the default does it fall to 22). Halving `debt_scale` does nothing useful.

**[J] Instant orders shouldn't escalate like slow windows.** The repeat factor and its 20-beat memory were designed
against people who stop the world for 30 s at a time. An order that doesn't slow anything still pays the full
escalation, and in a flow game that runs 20 beats in a minute, the factor grows quadratically with how engaged a
player is. That punishes exactly the fast, constant play the BUILDLOG asks for. See §9 for options.

## 3. Study 1: the elimination mix vs `debt_scale` and `debt_due_beats`

[M] Build B, 64 seeds per arm, `beats_total` 100 unless noted. The cause columns are eliminations per match (3 per
match in total).

| arm | beats mean/med [P10–P90] | 1st elim med [P10–P90] | debt | army | accident | arena_clock | other* | kinds/match | ent max med | ent40% | heat death% | casts med | battles mean | %w/ battle | generals mean | mobs mean | champ seats 0/1/2/3 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| base | 28.4/28 [17–41] | 13 [11–20] | 1.94 | 0.31 | 0.41 | 0.05 | 0.30 | 1.94 | 36 | 31 | 0 | 7 | 30.5 | 100 | 10.3 | 21.7 | 16/13/18/17 |
| ds0 | 29.0/30 [18–42] | 13 [11–20] | 1.98 | 0.28 | 0.39 | 0.08 | 0.27 | 1.94 | 38 | 36 | 0 | 7 | 31.5 | 100 | 10.5 | 21.8 | 15/14/19/16 |
| ds010 | 27.1/27 [17–38] | 13 [10–20] | 2.03 | 0.27 | 0.33 | 0.11 | 0.27 | 1.84 | 35 | 20 | 0 | 7 | 27.6 | 100 | 9.9 | 21.2 | 16/14/16/18 |
| dd3 | 27.1/27 [14–39] | 11 [8–19] | 2.11 | 0.34 | 0.27 | 0.05 | 0.23 | 1.80 | 32 | 3 | 0 | 7 | 27.2 | 100 | 9.6 | 18.8 | 14/12/20/18 |
| dd10 | 30.9/30 [19–42] | 16 [12–23] | 1.77 | 0.44 | 0.38 | 0.19 | 0.23 | 2.03 | 41 | 55 | 0 | 8 | 36.2 | 100 | 11.5 | 26.2 | 13/15/17/19 |
| dd25 | 40.0/39 [30–49] | 25 [13–33] | 1.38 | 0.67 | 0.44 | 0.30 | 0.22 | 2.25 | 56 | 91 | 0 | 10 | 54.7 | 100 | 13.9 | 42.0 | 10/17/21/16 |
| dd40 | 46.3/46 [35–59] | 25 [13–41] | 0.53 | 0.92 | 0.66 | 0.56 | 0.33 | 2.34 | 63 | 97 | 0 | 12 | 71.5 | 100 | 15.7 | 50.3 | 12/20/15/17 |
| dd50 | 48.2/49 [35–61] | 25 [13–41] | 0.38 | 1.02 | 0.67 | 0.59 | 0.34 | 2.36 | 65 | 97 | 0 | 12 | 76.9 | 100 | 16.4 | 52.0 | 14/20/13/17 |
| dd60 | 49.4/49 [35–64] | 25 [13–41] | 0.12 | 1.02 | 0.81 | 0.69 | 0.36 | 2.33 | 66 | 97 | 0 | 12 | 80.6 | 100 | 16.7 | 53.4 | 15/20/15/14 |
| dd100 | 49.7/49 [35–66] | 25 [13–41] | 0.08 | 1.02 | 0.81 | 0.70 | 0.39 | 2.31 | 66 | 97 | 0 | 12 | 81.7 | 100 | 16.8 | 53.9 | 14/19/16/15 |

[M] Median match length vs `debt_due_beats` (build B, `beats_total` 100):

```
due  3 | ###########################                       27   first elim 11
due  6 | ############################                      28   first elim 13   <- default
due 10 | ##############################                    30   first elim 16
due 25 | #######################################           39   first elim 25
due 40 | ##############################################    46   first elim 25
due 60 | #################################################  49   first elim 25
due100 | #################################################  49   first elim 25
         0        10        20        30        40        50 beats
```

- **[M] Due 25–40 gives the richest mix that still keeps debt meaningful.** At due 25, debt causes 1.4 of the 3
  eliminations and there are 2.25 distinct causes per match. At due 40, debt 0.5, armies 0.9, accidents 0.7 and the
  arena clock 0.6 (2.34 distinct causes).
- **[M] Due ≥ 60 makes debt irrelevant** (0.1 per match): all the debt is still owed when the match ends.
- **[M] The first elimination stalls at about beat 25 for every due ≥ 25.** That's when the non-debt killers (armies,
  accidents, the clock) start to land. P10 stays at 11–13 in every arm: about 1 match in 10 loses a demigod early to
  an army, an accident or a god power.

## 4. Study 2: match length and society time vs `beats_total` and the arena

| arm | beats mean/med [P10–P90] | 1st elim med [P10–P90] | debt | army | accident | arena_clock | other* | kinds/match | ent max med | ent40% | heat death% | casts med | battles mean | %w/ battle | generals mean | mobs mean | champ seats 0/1/2/3 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| base | 28.4/28 [17–41] | 13 [11–20] | 1.94 | 0.31 | 0.41 | 0.05 | 0.30 | 1.94 | 36 | 31 | 0 | 7 | 30.5 | 100 | 10.3 | 21.7 | 16/13/18/17 |
| bt60 | 24.5/24 [16–32] | 14 [10–18] | 1.88 | 0.28 | 0.30 | 0.36 | 0.19 | 1.97 | 34 | 19 | 0 | 6 | 22.4 | 100 | 9.6 | 20.9 | 15/14/20/15 |
| bt150 | 31.2/30 [18–44] | 12 [10–20] | 1.92 | 0.34 | 0.38 | 0.05 | 0.31 | 1.94 | 37 | 25 | 0 | 7 | 36.1 | 100 | 10.9 | 21.9 | 15/15/15/19 |
| bt60_dd20 | 31.6/31 [25–39] | 22 [11–29] | 1.30 | 0.50 | 0.45 | 0.48 | 0.27 | 2.22 | 46 | 77 | 0 | 8 | 36.0 | 100 | 12.0 | 33.4 | 13/11/19/21 |
| bt60_dd30 | 35.8/36 [28–43] | 22 [11–34] | 0.67 | 0.70 | 0.58 | 0.67 | 0.38 | 2.27 | 51 | 88 | 0 | 10 | 45.3 | 100 | 13.2 | 40.0 | 12/14/15/23 |
| bt60_dd40 | 38.4/39 [28–47] | 22 [11–35] | 0.23 | 0.70 | 0.66 | 0.97 | 0.44 | 2.28 | 54 | 88 | 0 | 10 | 52.2 | 100 | 13.6 | 43.2 | 10/14/20/20 |
| bt60_dd60 | 39.5/39 [28–54] | 22 [11–35] | 0.09 | 0.70 | 0.69 | 1.03 | 0.48 | 2.23 | 55 | 88 | 0 | 10 | 55.0 | 100 | 13.9 | 44.9 | 9/16/16/23 |
| bt60_dd60_es15 | 38.7/39 [29–51] | 23 [11–35] | 0.27 | 0.78 | 0.53 | 0.98 | 0.44 | 2.22 | 82 | 98 | 16 | 10 | 53.0 | 100 | 13.9 | 47.9 | 12/15/17/20 |
| bt60_dd60_cl100 | 40.4/40 [31–53] | 22 [11–34] | 0.08 | 0.92 | 0.58 | 1.03 | 0.39 | 2.30 | 53 | 84 | 0 | 10 | 39.4 | 100 | 11.0 | 38.9 | 12/16/20/16 |
| bt80_dd60 | 45.1/45 [31–61] | 24 [12–40] | 0.08 | 0.86 | 0.69 | 0.84 | 0.53 | 2.34 | 61 | 92 | 0 | 12 | 67.5 | 100 | 16.2 | 50.3 | 15/13/20/16 |
| bt90_dd60 | 46.6/46 [33–63] | 24 [12–38] | 0.11 | 1.17 | 0.59 | 0.64 | 0.48 | 2.44 | 63 | 94 | 0 | 12 | 70.5 | 100 | 16.2 | 51.5 | 13/19/18/14 |
| dd60 | 49.4/49 [35–64] | 25 [13–41] | 0.12 | 1.02 | 0.81 | 0.69 | 0.36 | 2.33 | 66 | 97 | 0 | 12 | 80.6 | 100 | 16.7 | 53.4 | 15/20/15/14 |
| dd50_bt130 | 52.1/54 [38–68] | 26 [12–43] | 0.47 | 0.97 | 0.77 | 0.42 | 0.38 | 2.38 | 68 | 92 | 0 | 14 | 84.8 | 100 | 17.5 | 55.2 | 17/14/18/15 |
| slow_b | 59.2/56 [37–83] | 25 [12–49] | 0.14 | 1.02 | 0.94 | 0.27 | 0.64 | 2.31 | 41 | 56 | 0 | 14 | 98.3 | 100 | 19.2 | 54.2 | 16/16/19/13 |
| city | 14.5/15 [12–17] | 11 [9–14] | 0.53 | 1.56 | 0.22 | 0.00 | 0.69 | 2.20 | 15 | 0 | 0 | 5 | 42.8 | 100 | 14.8 | 6.3 | 14/14/18/18 |
| cl100 | 28.5/28 [17–39] | 13 [11–20] | 2.03 | 0.39 | 0.30 | 0.06 | 0.22 | 1.91 | 37 | 28 | 0 | 7 | 20.2 | 100 | 7.5 | 18.3 | 14/20/18/12 |
| cl400 | 30.1/30 [19–42] | 13 [11–23] | 2.41 | 0.20 | 0.19 | 0.03 | 0.17 | 1.59 | 39 | 39 | 0 | 8 | 40.4 | 100 | 12.6 | 28.7 | 9/22/17/16 |
| dd60_cl100 | 50.2/51 [36–64] | 24 [13–35] | 0.16 | 1.00 | 0.75 | 0.64 | 0.45 | 2.38 | 65 | 95 | 0 | 13 | 58.6 | 100 | 12.8 | 47.4 | 19/20/14/11 |
| so | 98.1/100 [100–100] | 100 [13–100] | 0.00 | 0.62 | 0.00 | 0.14 | 2.23 | 1.45 | 0 | 0 | 0 | 0 | 227.1 | 100 | 38.3 | 40.8 | 13/17/21/13 |

- **[M] `beats_total` barely matters while debt rules.** 60 / 100 / 150 gives a median of 24 / 28 / 30.
- **[M] With debt deferred, `beats_total` mostly sets how hard the arena clock bites.** At `beats_total` 60 the arena
  clock takes about 1.0 demigods per match and matches end at a median of 39. At 100, 0.7 and 49. At 130, 0.4 and
  54. Time's circle (`arena_radius_at`) is a function of beat ÷ `beats_total` with an ease-out, and its damage
  (`0.5 + 1.5 × progress + 4 × overshoot`) also scales with progress. A short Final Night therefore means an
  aggressive circle.
- **[M] The `city` arena is over in a median of 15 beats:** armies are in reach at once, with 1.56 army eliminations
  per match. Not a candidate for a 7-minute match.
- **[M] `clusters`** (100 / 200 / 400) changes society volume (battles 20 / 31 / 40 per match) but not match length.
- **[M] Does the society get time to matter?** At defaults (build B) there are already 30 battles, 10 generals and 22
  mobs per match, in 100% of matches. With due ≥ 40 that grows to 70–85 battles, 16–17 generals and 50–55 mobs.
  Armies then become the top single cause (about 1.0 per match). A society-only run (no demigod orders or casts)
  runs to the Final Night at beat 100 in 98% of matches, with 227 battles. **The demigods' own actions, plus Time's
  bill for them, are what end matches early.**
- **[M] Army strikes and early army formation (build B's polish).** Build A to B at the same seeds and defaults:
  battles 3.9 → 30.5 per match, generals 3.7 → 10.3, army eliminations 0.31 → 0.31, median length 29 → 28. In
  deferred-debt arms (due 60): army eliminations 1.11 → 1.02, length 48 → 49. So the polish made the society far
  busier without making armies more lethal to demigods on average. An army strike is
  `(0.5 + size/40 000) × (0.6 + 0.6 × competence)`, at most 2.0 per beat, and only one army storms a demigod per beat.
  Early overruns do happen (P10 first elimination ≈ beat 11–13, about 15% of first eliminations are army), and the
  seed-7 match ending at beat 13 is one of those tails.

## 5. Study 3: Entropy pacing

[M] The beat at which Entropy first reaches each level (median [P10–P90]) and the share of matches that get there.
Build B.

| arm | Entropy max med [P10–P90] | ≥20: % / beat | ≥40 | ≥60 | ≥80 | heat death % / beat |
|---|---|---|---|---|---|---|
| es05 | 18 [13–23] | 31% / 28 [19–35] | 0% / — | 0% / — | 0% / — | 0% / — |
| base | 36 [27–46] | 100% / 14 [8–21] | 31% / 28 [19–35] | 0% / — | 0% / — | 0% / — |
| es2 | 72 [51–89] | 100% / 6 [4–12] | 100% / 14 [8–21] | 80% / 21 [13–33] | 30% / 27 [21–41] | 2% / 34 [34–34] |
| es3 | 100 [81–100] | 100% / 4 [3–8] | 100% / 9 [5–15] | 100% / 14 [8–21] | 92% / 19 [11–30] | 64% / 24 [16–35] |
| sb0 | 13 [10–17] | 2% / 15 [15–15] | 0% / — | 0% / — | 0% / — | 0% / — |
| sb6 | 36 [27–46] | 100% / 14 [8–21] | 31% / 27 [19–39] | 0% / — | 0% / — | 0% / — |
| dd60 | 66 [45–82] | 100% / 14 [9–21] | 97% / 27 [19–41] | 59% / 39 [29–49] | 16% / 51 [37–68] | 0% / — |
| dd50_bt130 | 68 [43–92] | 100% / 14 [9–21] | 92% / 28 [19–41] | 62% / 39 [29–53] | 23% / 49 [37–61] | 0% / — |
| bt60_dd60 | 55 [32–72] | 100% / 14 [9–19] | 88% / 27 [19–35] | 31% / 39 [33–45] | 0% / — | 0% / — |
| bt60_dd60_es15 | 82 [50–100] | 100% / 9 [5–15] | 98% / 18 [13–25] | 86% / 27 [19–35] | 55% / 37 [25–43] | 16% / 46 [37–52] |
| chaos_c | 100 [73–100] | 100% / 6 [4–11] | 100% / 14 [9–21] | 94% / 20 [14–27] | 88% / 27 [19–37] | 58% / 33 [23–40] |
| slow_b | 41 [27–66] | 98% / 23 [13–35] | 56% / 46 [31–61] | 19% / 60 [47–79] | 3% / 84 [79–88] | 0% / — |

- **[M] At scale 1, Entropy climbs about 1.4 per beat:** about 0.5 per order, 0.4 per bolt spent, 0.2 per bolt
  handed out. Level 40 lands around beat 27–28 in every arm; level 60 around beat 39 and level 80 around beat 50,
  if the match lasts that long.
- **[M] Heat death at scale 1 never happened in any build-B arm.** It needs scale ≥ 2 (2% of matches at defaults,
  58% in the chaos package) or scale 3 (64%, median beat 24).
- **[M] `start_bolts` 3 → 6 changes nothing** (the bolt flood swamps it). `start_bolts` 0 caps Entropy at about 13.
- **[J] For a ~55-beat match, scale 1.0–1.25 puts level 80 ("godfall") near the end** (beat 39–50), the escalation
  arc the BUILDLOG describes.

## 6. Study 4: the bolt economy

| arm | casts mean / med [P10–P90] | casts/beat | fizzled | bolts spent on casts | orders paid with a bolt | bolts held at end (all 4) |
|---|---|---|---|---|---|---|
| sb0 | 0.0 / 0 [0–0] | 0.00 | 0.0 | 0.0 / 0 [0–0] | 0.0 / 0 [0–0] | 0.0 / 0 [0–0] |
| base | 7.5 / 7 [3–13] | 0.26 | 0.0 | 15.4 / 16 [4–26] | 31.5 / 32 [23–40] | 212.9 / 209 [160–278] |
| sb6 | 7.5 / 8 [3–13] | 0.26 | 0.0 | 15.7 / 15 [4–27] | 31.6 / 33 [23–40] | 227.1 / 222 [172–290] |
| dd60 | 12.1 / 12 [6–17] | 0.25 | 0.0 | 25.3 / 26 [13–38] | 56.5 / 57 [37–76] | 324.9 / 343 [214–393] |
| es2 | 7.5 / 8 [3–13] | 0.26 | 0.0 | 15.7 / 15 [4–28] | 30.8 / 30 [22–40] | 212.7 / 206 [160–278] |
| chaos_c | 9.2 / 9 [4–14] | 0.28 | 0.0 | 19.3 / 20 [8–30] | 41.1 / 43 [32–48] | 298.0 / 322 [221–361] |
| dd50_bt130 | 12.8 / 14 [5–20] | 0.24 | 0.0 | 26.5 / 26 [10–42] | 59.1 / 60 [36–86] | 326.2 / 339 [215–392] |
| slow_b | 14.5 / 14 [8–20] | 0.25 | 0.0 | 29.9 / 30 [17–43] | 66.6 / 60 [32–103] | 330.4 / 337 [227–393] |

- **[M] Casting is rare and constant.** The built-in AI casts about 0.25 times per beat across the table (about one
  cast per demigod every 16 beats), with 0 fizzles in any arm, while holding 40+ bolts each by mid-match. Casts by
  power are spread evenly across the eleven powers.
- **[M] "+2 to every rival per bolt" makes a flood.** It's mostly fed by **paying Time for orders** (31 orders → about
  190 bolts handed out per match) rather than by casts (about 15 bolts spent → about 90). Bolts stop being a
  decision: nobody runs out, so nobody ever pays double debt.
- **[J] With humans, bolts will be a flood too unless paying Time is excluded from the gift.** That would also make
  the "unpaid order costs double" rule actually bite.

## 7. Study 5: seat fairness, variance, the early leader

- **[M] Champion seats at defaults over 256 seeds (build B): 77 / 54 / 68 / 57 (χ² = 5.22, df 3; the 5% critical value is 7.81).** For reference, build A over 256 seeds was
  62 / 64 / 71 / 59 (χ² 1.22). The 64-seed blocks swing a lot (build A: seat 0 won 8, 18, 17 and 19 in four blocks),
  so a 16-seed sample (the frontend's 1 / 6 / 5 / 4) can't show a bias. **No seat bias was detected.**
- **[M] The first demigod out, by seat (build A, 256 seeds):** 74 / 47 / 68 / 67. Seat 1 goes out first a little less
  often (χ² 6.5, p ≈ 0.09, not significant). Worth re-checking with 512 seeds if it matters. Seat 1 is "Brix", and
  "Volunteers of Vienna for Brix" turns up repeatedly as an early killer, which suggests a start-region effect.
- **[M] The early leader (highest health; ties excluded), build B, 64 seeds:**

Share of matches the health leader at that point went on to win (chance = 25%):

| package | beat 5 | beat 10 | 25% of match | 50% of match | 75% of match |
|---|---|---|---|---|---|
| defaults | 29% (n=59) | 52% (n=64) | 39% (n=62) | 47% (n=64) | 58% (n=64) |
| default+ | 31% (n=59) | 40% (n=64) | 42% (n=64) | 52% (n=64) | 56% (n=64) |
| chaos | 39% (n=30) | 61% (n=32) | 53% (n=32) | 75% (n=32) | 64% (n=32) |
| slow burn | 30% (n=29) | 40% (n=32) | 46% (n=32) | 50% (n=32) | 47% (n=32) |

- **[J]** Leads are soft early (29% at beat 5 is near chance) and firm late (about 58% at 75% of the match). That's a
  healthy shape for a battle royale. The chaos package is the most "snowbally" (§8).

## 8. The 7-minute target and the three packages

The target, as revised: a match that feels like about 7 minutes. **With the LLM on (7.5 s per beat) that's 55–60
beats.** First elimination around beat 15–25, the last near the Final Night, causes spread out, the society busy
throughout.

**[M] The settings-only ceiling.** With the AI demigods acting, the median match length tops out at 49–56 beats,
whatever `debt_due_beats` and `beats_total` are. Armies, accidents and the arena clock take over once debt is
deferred. Setting `beats_total` near 60 makes the arena clock harsher, so matches end *earlier* (median 39), and no
match reaches the Final Night. Either accept a long `beats_total` that's never reached, or change rules (§9).

[M] Each package was run on seeds 1–192 (192 matches; the defaults on 256), build B, `llm: false`. The minutes are
[E]: median beats × 7.5 s (LLM on) or × 3.0 s (rules only), with the P10–P90 range of match length.

| | **defaults** | **default+** (recommended) | **chaos** | **slow burn** |
|---|---|---|---|---|
| Settings (changes from default) | none | `debt_due_beats: 50, beats_total: 130` | `debt_due_beats: 25, entropy_scale: 2, beats_total: 80` | `debt_due_beats: 100, beats_total: 200, entropy_scale: 0.6` |
| Matches | 256 | 192 | 192 | 192 |
| Match length, median [P10–P90] (mean) | 27 [18–39] (27.8) | **53 [36–64] (51.4)** | 34 [26–42] (33.7) | 58 [38–83] (60.2) |
| First elimination, median [P10–P90] | 13 [11–20] | **28 [14–43]** | 26 [15–31] | 29 [13–48] |
| Beats between eliminations (median) | 5 | 10 | 3 | 12 |
| **Minutes, LLM on** (7.5 s/beat), median [P10–P90] | 3.4 [2.2–4.9] | **6.6 [4.5–8.0]** | 4.2 [3.2–5.2] | 7.2 [4.8–10.4] |
| Minutes, rules only (3.0 s/beat) | 1.4 [0.9–1.9] | 2.6 [1.8–3.2] | 1.7 [1.3–2.1] | 2.9 [1.9–4.2] |
| Eliminations per match: debt / army / accident / arena clock / other | 2.06 / 0.26 / 0.37 / 0.07 / 0.25 | 0.51 / 0.93 / 0.73 / 0.42 / 0.41 | 1.81 / 0.44 / 0.30 / 0.26 / 0.19 | 0.10 / 0.94 / 1.04 / 0.29 / 0.64 |
| Distinct causes per match | 1.85 | **2.39** | 1.87 | 2.33 |
| Entropy max (median); reaches 80; heat death | 36; 0%; 0% | 67; 19%; 2% | 100; 90%; **58%** | 45; 2%; 0% |
| Casts per match (median); bolts held at the end | 6; 209 | 13; 342 | 9; 310 | 14; 344 |
| Battles / generals / mobs / mutinies (mean) | 29 / 10 / 22 / 8 | 85 / 18 / 55 / 11 | 40 / 13 / 44 / 7 | 106 / 20 / 57 / 11 |
| Champion seats 0/1/2/3 (χ²) | 77/54/68/57 (5.2) | 55/49/46/42 (1.9) | 52/51/48/41 (1.5) | 50/50/51/41 (1.4) |
| Debt billed to Time per match | 49 | 127 (most still owed at the end) | 72 (32 seeds) | 170 (32 seeds; almost all still owed) |

**default+.** [M] It doubles the match: 53 beats, about 6.6 min with the LLM. The first elimination moves to about
beat 28 (P10 14). Eliminations spread across armies 31%, accidents 24%, debt 17% and the clock 14%. The society
gets about 85 battles and 18 generals, and Entropy climbs to about 67 (level 60 around beat 39). Seats are fair.
[J] It's the best settings-only answer to the ~7-minute target, at the price of a `beats_total` that's never
reached: the Final Night is only a backstop. Debt stays "owed" (127 billed against 0.5 debt eliminations), so it
would decide a Final Night if one came.

**chaos.** [M] Short and loud: 34 beats (about 4.2 min with the LLM). Heat death ends 58% of matches, 41% of
matches end with two or more demigods falling on the same last beat, and debt dominates again (60% of
eliminations). [J] It's the pandemonium mode: Entropy's arc is the story, and the leader snowballs (see §7).

**slow burn.** [M] The longest: median 58 beats (about 7.2 min with the LLM) but the widest spread (P90 83, about
10 min), and the most society (about 106 battles, 20 generals). Debt is nearly gone (0.1 per match); accidents
(35%) and armies (31%) decide it, and Entropy stays below 60 in most matches. [J] It's a "war of armies" mode.
Without debt as a threat, the Consequence fantasy is missing.

**Closest to "beats_total ≈ 60" with settings only.** [M] `beats_total: 60, debt_due_beats: 60,
entropy_scale: 1.5`: median 39 [29–51], first elimination 23, the arena clock 0.98 per match, heat death 16%.
**It misses the target** (last elimination near 55–60) because Time's arena circle tightens on a 60-beat schedule.
That needs R2 below.

## 8b. "All players using all superpowers destroys the world in ~30 seconds": calibration

Chris's target (BUILDLOG, "the core loop, restated"): with 4 players all casting at the client's maximum rate, heat
death should come in about 30 s of real time, which is 2–3 beats at the human-room pace of `flow_ms` 12000. Normal
play (a few casts per player per match) should still last the full ~30-beat match.

**[E] This is a model, not a simulation.** The built-in AI casts about 0.25 times per beat and can't be made to spam,
so the sim API can't measure frantic casting. The model follows the code exactly:
- The rate limit is 2 instant actions per second per seat, with a burst of 4 (the coordinator's numbers), so there
  are 28 attempts per seat in the first 12 s beat and 24 after that.
- Casts settle once per beat, at the next resolve, in arrival order (round-robin across seats in the model,
  `casts.rs`). A cast lands if its caster holds at least its cost at that moment, otherwise it fizzles.
- Spending c bolts gives every other live demigod +2c, capped at 99 (`spend_bolts`), and raises Entropy by
  0.4 × c × `entropy_scale`. Heat death at 100 makes that beat the Final Night.
- Start with 3 bolts each. The palette's mean cost is 2.1 (`data/powers.json`); "cheapest only" means spamming the
  1-bolt powers.
- The Entropy from orders and slow windows (about 0.5 per order) is left out, which makes the model slightly
  conservative.

**[E] Bolts are not a brake.** With 4 seats casting in interleaved order, each landed 1-bolt cast puts 6 bolts in
rivals' hands. The table is self-funding from the second or third cast on: about 96–112 casts land per 12 s beat,
and nobody runs dry.

| entropy_scale | casting | powers | Entropy after resolve 1 / 2 / 3 | heat death at resolve | ≈ seconds (12 s beats) |
|---|---|---|---|---|---|
| 0.5 | 4 seats at 2/s (max) | mixed palette (mean cost 2.1) | 46 / 87 / 127 | 3 | 36 |
| 0.5 | 4 seats at 2/s (max) | cheapest only (cost 1) | 22 / 42 / 61 | 6 | 72 |
| 0.5 | 4 seats at 1/s | mixed palette (mean cost 2.1) | 27 / 46 / 67 | 5 | 60 |
| 0.5 | 4 seats at 1/s | cheapest only (cost 1) | 13 / 22 / 32 | >6 | >72 |
| 0.75 | 4 seats at 2/s (max) | mixed palette (mean cost 2.1) | 70 / 130 | 2 | 24 |
| 0.75 | 4 seats at 2/s (max) | cheapest only (cost 1) | 34 / 62 / 91 | 4 | 48 |
| 0.75 | 4 seats at 1/s | mixed palette (mean cost 2.1) | 40 / 70 / 100 | 4 | 48 |
| 0.75 | 4 seats at 1/s | cheapest only (cost 1) | 19 / 34 / 48 | >6 | >72 |
| 1.0 | 4 seats at 2/s (max) | mixed palette (mean cost 2.1) | 93 / 174 | 2 | 24 |
| 1.0 | 4 seats at 2/s (max) | cheapest only (cost 1) | 45 / 83 / 122 | 3 | 36 |
| 1.0 | 4 seats at 1/s | mixed palette (mean cost 2.1) | 54 / 93 / 133 | 3 | 36 |
| 1.0 | 4 seats at 1/s | cheapest only (cost 1) | 26 / 45 / 64 | 5 | 60 |
| 1.5 | 4 seats at 2/s (max) | mixed palette (mean cost 2.1) | 139 | 1 | 12 |
| 1.5 | 4 seats at 2/s (max) | cheapest only (cost 1) | 67 / 125 | 2 | 24 |
| 1.5 | 4 seats at 1/s | mixed palette (mean cost 2.1) | 81 / 139 | 2 | 24 |
| 1.5 | 4 seats at 1/s | cheapest only (cost 1) | 38 / 67 / 96 | 4 | 48 |
| 2.0 | 4 seats at 2/s (max) | mixed palette (mean cost 2.1) | 186 | 1 | 12 |
| 2.0 | 4 seats at 2/s (max) | cheapest only (cost 1) | 90 / 166 | 2 | 24 |
| 2.0 | 4 seats at 1/s | mixed palette (mean cost 2.1) | 108 | 1 | 12 |
| 2.0 | 4 seats at 1/s | cheapest only (cost 1) | 51 / 90 / 128 | 3 | 36 |

**[E] Normal play at the same settings** ([M] rates from the sims). At `entropy_scale` 1, Entropy climbs about
1.4 per beat and peaks at a median of 36 by the end of a ~28-beat match. Heat death occurred in 0 of 256 default
matches. At 1.5 it hit 16–42% of the long-match arms, and only after beat ~40. At 2 it hit 2% of default-length matches and
58–73% of longer ones.

**Recommendation.**
- [E] **Keep `entropy_scale` at 1.0: no rules change is needed.** At today's rules (0.4 Entropy per bolt, +2 per bolt
  to each rival), four players casting the mixed palette at the maximum rate hit heat death at the **2nd resolve
  (about 24 s, ≈ 28–32 s with the LLM's resolve time)**. Spamming only the cheapest powers takes until the **3rd
  resolve (about 36 s)**. A normal ~30-beat match peaks around Entropy 36–45 and never reaches heat death.
- [E] **Don't go above 1.25.** At 1.5 and above, a frantic table ends the world at the *first* resolve (12 s), and
  normal long matches start to hit heat death.
- [E] At 0.75, frantic mixed casting still takes 24 s, but cheapest-only spam takes about 48 s.

**[J] Rules notes (optional, not required for the target):**
1. Heat death is quantized to resolves, so with 12 s beats it can only land at 12 / 24 / 36 s. If you want exactly
   "30 seconds of clicking", heat death would have to be checked on instant casts as they arrive. That's a rules and
   engine change; today casts only settle at the next resolve.
2. The self-funding bolt chain depends on gifts landing within the same resolve, in arrival order. If gifts were
   applied at the end of the resolve instead, a frantic table would be capped by bolts held (about 3 to 12 per
   seat early on) and heat death would take noticeably longer. Keep today's order if the 30 s feel matters.
3. **Verify live before the playtest.** On a private server, run a 4-seat room at `flow_ms` 12000 with scripted
   clients sending one instant cast every 500 ms. The estimate is 2–3 resolves.

## 9. Rules changes we recommend (for the designers to decide; no code changed)

**R1. Charge instant orders less than slow windows (the main lever).**
- Options:
  - Bill instant orders at a flat base with no repeat factor.
  - Shorten `SLOW_REPEAT_BEATS` from 20 to about 5 for all slows.
  - Make the repeat factor count real windows only.
- [E] What each would do to the debt rate (from the measured AI order rate of 0.354 per alive-demigod-beat):

  | Option | Time debt per alive-demigod-beat | Beats for debt alone to reach 10 health | Debt death, due 6 |
  |---|---|---|---|
  | Today (measured) | 0.55 | 18 | ~beat 24–30 (measured median 28) |
  | Instant: 0.5 flat, no repeat factor | 0.18 | 56 | ~beat 62 |
  | Instant: 0.25 flat | 0.09 | 113 | past the Final Night |
  | Repeat window 20 → 5 beats | ~0.25 | ~40 | ~beat 46 |

- [M] The closest settings proxy is "debt never falls due mid-match" (due ≥ 60). It measured median length 28 → 49,
  first elimination 13 → 25, debt eliminations 1.94 → 0.1, battles 30 → 81 per match and generals 10 → 17.
- [J] The best fit is the flat-0.5 option with `debt_due_beats` around 10–15. Debt then lands mid-to-late as the
  "Final Destination accident" it was meant to be, instead of being the clock.

**R2. Decouple Time's arena circle from `beats_total`, or soften it for short matches.** [M] At `beats_total` 60 the
clock causes about 1.0 eliminations per match and ends matches at a median of 39. A gentler ease, or a lower damage
ramp (`0.5 + 1.5 × progress`), would let `beats_total` 60 actually be reached. [J] Without this, "beats_total ≈ 60
and the last elimination near the Final Night" is unreachable.

**R3. Don't give the +2 per bolt when the bolt pays Time.** [M] About two thirds of the bolt flood comes from order
payments. [J] Keeping the gift for casts keeps the BUILDLOG's "Entropy says yes, everyone gets power" and makes bolt
scarcity real again.

**R4. Turn down damage from the non-debt killers for a ~60-beat Final Night.**
- [M] With debt deferred, the remaining killers end matches by about beat 49. The levers:
  - the accident rate (`ACCIDENT_ODDS` 20, rules fallback)
  - army strike damage (`(0.5 + size/40k) × (0.6 + 0.6 × competence)`, cap 2.0)
  - early army formation (`FIRST_HOST_P` 0.2, `HOST_NEAR_KM` 1200)
- [E] To push the *last* elimination from about 49 to about 58, the combined non-debt damage rate needs to fall by
  roughly 15–20%. For example, accident odds 1-in-30 and army strikes ×0.8.
- [J] Not simulated. These need a build with knobs. **Exposing them as room settings** (`time_order_debt`,
  `slow_repeat_beats`, `accident_odds`, `army_strike_scale`, `arena_ease`) would let the next study measure them
  directly through the sim API.

**R5. The sim's AI order rate.** [M] 0.35 orders per alive-demigod-beat, every one instant. [J] Before tuning to the
AI, measure human order rates in a playtest (`slow.instant_orders`, windows and seconds open per seat). If humans
order less often but hold windows open 10–30 s, Time's bill per order is ×2–4 higher than the sim's, and the pacing
picture changes.

## 10. Open design questions for Benito

1. **Is Time's order bill meant to be the match clock?** Today it is: it decides 2 of 3 eliminations and most of the
   length. If debt should be "Final Destination accidents" (spiky and narrative), we recommend R1.
2. **Should instant orders (no slow window) cost the same as a held window?** The buildlog says early Enter should be
   cheap. Today a 0-second order costs `0.5 × repeat`, and the repeat dominates.
3. **The Final Night as an ending:** in today's tuning no match with acting demigods reaches it. Should the target
   be "most matches end on the Final Night" (needs R1, R2 and R4), or "the Final Night is a backstop" (the default+
   package as is)?
4. **The bolt economy:** is the flood intended ("infinite magic power ahahaha"), or should bolts stay scarce enough
   that "unpaid = double debt" matters? (R3)
5. **Entropy's arc:** should heat death be a common ending (scale ≥ 2, chaos) or a rare event (scale ≤ 1.25)? At
   scale 1, level 80 needs a match of 50+ beats.
6. **The `city` arena** ends in about 15 beats with armies decisive. Is it a separate quick mode, or does it need its
   own tuning?
7. **Early knockouts:** about 10% of matches lose a demigod by beat ~12 to an army or an accident. For a human
   player, that's a short evening. Is a grace period wanted (no army strikes or accidents on demigods before beat N)?

---

## Appendix A: reproducing

```sh
# private server (no LLM, no recording), then POST /sim/run with {"settings": {...}, "seeds": [1..64]}
THEATRE_BIND=127.0.0.1:8095 THEATRE_ADMIN_TOKEN=<random> THEATRE_RECORD=off FN7_SIM_WORKERS=12 ./target/release/fn7-server
```

Each arm is one `/sim/run` of seeds 1–64 with the settings named in its label: `ddN` = `debt_due_beats` N, `btN` =
`beats_total` N, `dsX` = `debt_scale` 0.X, `esX` = `entropy_scale`, `sbN` = `start_bolts`, `clN` = `clusters`, `so` =
`society_only`. Determinism (`llm: false`) means a re-run gives identical results on the same build.

## Appendix B: build A sweeps (before the society polish; superseded but directionally useful)

[M] Build A, 64 seeds per arm.

| arm | beats mean/med [P10–P90] | 1st elim med [P10–P90] | debt | army | accident | arena_clock | other* | kinds/match | ent max med | ent40% | heat death% | casts med | battles mean | %w/ battle | generals mean | mobs mean | champ seats 0/1/2/3 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| base | 27.9/29 [20–36] | 14 [11–20] | 2.12 | 0.31 | 0.28 | 0.08 | 0.20 | 1.80 | 37 | 34 | 0 | 6 | 3.9 | 59 | 3.7 | 17.4 | 8/19/23/14 |
| ds0 | 27.8/27 [20–35] | 14 [11–20] | 2.09 | 0.39 | 0.28 | 0.06 | 0.17 | 1.83 | 37 | 36 | 0 | 6 | 3.7 | 56 | 3.7 | 16.2 | 10/18/21/15 |
| ds002 | 27.8/27 [20–35] | 14 [11–20] | 2.11 | 0.36 | 0.28 | 0.06 | 0.19 | 1.81 | 36 | 36 | 0 | 6 | 3.6 | 58 | 3.6 | 15.9 | 10/18/21/15 |
| ds003 | 27.7/27 [20–36] | 14 [11–20] | 2.11 | 0.38 | 0.28 | 0.06 | 0.17 | 1.83 | 36 | 34 | 0 | 6 | 3.7 | 55 | 3.7 | 16.3 | 9/18/21/16 |
| ds006 | 27.0/26 [19–35] | 14 [11–20] | 2.08 | 0.34 | 0.31 | 0.09 | 0.17 | 1.80 | 36 | 30 | 0 | 6 | 3.5 | 55 | 3.4 | 16.7 | 9/19/22/14 |
| ds010 | 26.0/26 [18–33] | 13 [11–20] | 2.03 | 0.31 | 0.36 | 0.12 | 0.17 | 1.83 | 34 | 25 | 0 | 6 | 3.2 | 53 | 3.3 | 15.4 | 12/18/23/11 |
| ds020 | 24.9/25 [17–32] | 13 [11–19] | 1.97 | 0.38 | 0.30 | 0.09 | 0.27 | 1.83 | 33 | 17 | 0 | 6 | 2.7 | 48 | 3.2 | 13.5 | 14/16/24/10 |
| ds040 | 22.6/22 [15–30] | 12 [10–18] | 2.38 | 0.20 | 0.23 | 0.03 | 0.16 | 1.58 | 30 | 6 | 0 | 5 | 1.8 | 41 | 2.6 | 9.2 | 14/13/22/15 |
| dd3 | 24.7/24 [17–32] | 11 [9–18] | 1.97 | 0.36 | 0.34 | 0.08 | 0.25 | 1.97 | 31 | 5 | 0 | 6 | 2.4 | 45 | 2.9 | 12.8 | 15/16/19/14 |
| dd10 | 30.2/29 [21–40] | 17 [11–24] | 1.88 | 0.44 | 0.28 | 0.17 | 0.23 | 2.09 | 40 | 50 | 0 | 7 | 4.6 | 64 | 4.3 | 22.9 | 7/20/22/15 |
| dd15 | 32.9/32 [25–42] | 21 [11–26] | 1.83 | 0.44 | 0.34 | 0.16 | 0.23 | 2.03 | 45 | 73 | 0 | 8 | 6.0 | 70 | 5.3 | 29.8 | 12/18/19/15 |
| dd25 | 38.2/38 [30–47] | 26 [11–32] | 1.30 | 0.59 | 0.58 | 0.27 | 0.27 | 2.42 | 54 | 94 | 0 | 10 | 12.1 | 89 | 7.4 | 44.9 | 8/22/22/12 |
| dd40 | 44.8/45 [33–52] | 27 [11–39] | 0.73 | 0.95 | 0.61 | 0.47 | 0.23 | 2.34 | 62 | 97 | 0 | 10 | 19.0 | 95 | 8.7 | 63.6 | 14/18/18/14 |
| dd60 | 48.6/48 [33–64] | 27 [11–39] | 0.12 | 1.11 | 0.78 | 0.64 | 0.34 | 2.31 | 65 | 97 | 2 | 11 | 25.3 | 95 | 9.2 | 73.0 | 12/22/17/13 |
| dd100 | 48.8/48 [33–64] | 27 [11–39] | 0.08 | 1.11 | 0.80 | 0.67 | 0.34 | 2.28 | 65 | 97 | 3 | 11 | 25.5 | 95 | 9.2 | 73.6 | 12/22/16/14 |
| bt50 | 23.5/23 [17–30] | 14 [11–19] | 1.81 | 0.36 | 0.20 | 0.44 | 0.19 | 1.95 | 33 | 8 | 0 | 6 | 2.5 | 38 | 3.2 | 15.1 | 7/21/22/14 |
| bt150 | 28.8/28 [20–38] | 14 [11–20] | 2.12 | 0.28 | 0.36 | 0.05 | 0.19 | 1.77 | 37 | 28 | 0 | 7 | 4.3 | 53 | 3.8 | 17.3 | 10/17/20/17 |
| bt200 | 29.3/29 [21–40] | 14 [11–19] | 2.09 | 0.34 | 0.33 | 0.02 | 0.22 | 1.75 | 36 | 23 | 0 | 7 | 4.7 | 61 | 4.1 | 17.8 | 15/17/18/14 |
| bt300 | 30.5/31 [21–40] | 14 [11–17] | 2.03 | 0.39 | 0.41 | 0.00 | 0.17 | 1.84 | 36 | 25 | 0 | 7 | 5.2 | 64 | 4.3 | 18.7 | 13/17/17/17 |
| es05 | 27.8/28 [20–36] | 14 [11–20] | 2.17 | 0.28 | 0.31 | 0.06 | 0.17 | 1.77 | 18 | 0 | 0 | 6 | 3.5 | 56 | 3.6 | 15.2 | 8/20/22/14 |
| es2 | 27.7/28 [20–38] | 14 [11–20] | 2.08 | 0.39 | 0.30 | 0.11 | 0.12 | 1.81 | 72 | 100 | 8 | 6 | 4.3 | 64 | 4.5 | 29.6 | 10/12/26/16 |
| es3 | 24.5/24 [16–33] | 14 [11–20] | 1.97 | 0.30 | 0.31 | 0.02 | 0.41 | 1.91 | 100 | 100 | 64 | 6 | 3.4 | 55 | 3.9 | 32.4 | 11/16/22/15 |
| es5 | 13.9/14 [9–19] | 12 [9–18] | 1.05 | 0.11 | 0.06 | 0.00 | 1.78 | 2.00 | 100 | 100 | 100 | 4 | 0.6 | 11 | 2.0 | 10.1 | 17/16/19/12 |
| sb0 | 23.7/22 [15–33] | 11 [10–18] | 2.31 | 0.36 | 0.28 | 0.05 | 0.00 | 1.59 | 13 | 0 | 0 | 0 | 2.1 | 36 | 2.5 | 8.7 | 14/13/25/12 |
| sb1 | 27.5/28 [19–35] | 14 [11–20] | 2.17 | 0.30 | 0.30 | 0.03 | 0.20 | 1.75 | 35 | 28 | 0 | 6 | 3.5 | 58 | 3.5 | 17.0 | 12/18/21/13 |
| sb6 | 27.8/28 [20–36] | 14 [11–20] | 2.08 | 0.33 | 0.31 | 0.08 | 0.20 | 1.83 | 37 | 34 | 0 | 6 | 3.8 | 58 | 3.7 | 17.0 | 9/20/21/14 |
| sb12 | 27.8/28 [20–36] | 14 [11–20] | 2.08 | 0.33 | 0.31 | 0.08 | 0.20 | 1.83 | 37 | 34 | 0 | 6 | 3.8 | 58 | 3.7 | 17.0 | 9/20/21/14 |
| city | 16.8/16 [14–21] | 14 [11–16] | 0.72 | 1.44 | 0.22 | 0.00 | 0.62 | 2.19 | 17 | 0 | 0 | 5 | 24.7 | 100 | 9.2 | 7.1 | 22/11/19/12 |
| cl100 | 27.4/28 [18–36] | 14 [11–20] | 2.08 | 0.38 | 0.31 | 0.06 | 0.17 | 1.78 | 36 | 31 | 0 | 6 | 1.6 | 33 | 2.2 | 8.8 | 15/18/20/11 |
| cl400 | 28.1/27 [20–39] | 14 [11–22] | 2.11 | 0.34 | 0.38 | 0.08 | 0.09 | 1.73 | 36 | 27 | 0 | 6 | 6.7 | 77 | 5.6 | 27.2 | 14/17/19/14 |

Other build-A findings, not repeated on B:
- `max_intents_per_hand` 1–6 and `max_subagents_per_hand` 0 have **no effect** on the built-in AI (identical results):
  `bot_lines` doesn't go through those limits.
- `entropy_scale` 5: heat death in 100% of matches at a median of beat 14 (the match length is then the heat-death
  beat).
- `beats_total` 300: median 31 (debt-bound).
