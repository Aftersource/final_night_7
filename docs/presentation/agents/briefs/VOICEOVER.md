# Brief: the voiceover engine (TTS)

*For the agent running Chris's TTS pipeline (Fish Audio, designed voices) and anyone wiring voice lines into the
Unreal client. Designers: Chris and Benito. Read `../FULL_EXPLAINER.md` first; this is your subset.*

## 1. What you must know about the game (the subset)

- Each player is a god with two demigods (Left Hand, Right Hand). The world beats every **3 s** (`flow_ms`; 12 s in
  the shared lobby) and **never waits**. A match is **60 beats** to the Final Night.
- Typing a will **slows time for everyone** (Time's interaction); the opener's Enter **cuts off** slower players;
  **Time** sends a bill (a bolt and a debt).
- Powers: **megas** (bolts) shake the map and push **Entropy** toward **heat death** (100 = the Final Night at once);
  **minis** (sparks) hit one spot.
- Seven god minds rule on anything divine: **grant, refuse, bargain or twist**, with a price.
- Double-click a rival: a forced **30-second typed duel**; the whole table watches.
- Fallen demigods can be **raised by Death** for a price; rivals can **seal** a grave or curse a demigod; every curse
  has a **mystic counter-quest**.
- The last god with a demigod standing wins.
- **Hidden information matters for voice:** nobody learns who else typed, who was cut, or who cast a curse; rivals'
  health is never told (except the duel's health bars). A voice line must never reveal what the screen may not show.

## 2. How voice plays in the client today (`Source/FinalNight/UI/FNVoiceOver.cpp`)

- Files: `production_assets/audio/vo/<name>_master.wav` (plain) and `<name>_god.wav` (pitched-down double); the game
  plays the `_god` versions, imported as `/Game/Audio/VO/VO_<Name>`. Spec: 44.1 kHz stereo, -14 LUFS, peaks under
  -1 dBFS. Designed voices only, never a real person's voice (Fish Audio Community License: free commercial use under
  $500k/yr). Each render comes with a report: the words heard (Whisper) and the voice match per take.
- **One line at a time.** A line already speaking or waiting is not queued twice; **at most three lines wait**; extra
  requests are dropped.
- **Any key skips** the line being spoken (the key still reaches the game). Players type a lot: a line that starts
  while someone types can be cut off at the first keystroke.
- `SayOnce(line, key)`: each hook plays once per key (once a run, once per match id, once per duel id).
- Volume = master x voice (per account). Lines keep speaking through a pause (UI sound).
- The voice plays locally on each client. "Who hears" below means which clients call it.

## 3. The seven lines that exist (vo/README.md, hooks in the Unreal source)

| Line | File | Hook (trigger) | Who hears | Text |
|---|---|---|---|---|
| Welcome | `fn7_welcome` (alt: `fn7_welcome_godnarrator`, "ancient godlike narrator"; the game uses the god-narrator take) | the signed-in lobby first opens, once a run (`SFNStartPanel`) | the player | "Welcome, god. Your two hands wait on the table below. Speak, and the world will listen." |
| Intro | `fn7_intro` | a match starts, once per match id (`SFNHudLive`, on `match.started`) | everyone in the match | "Seven nights. Seven wars. Gods at one table, and the whole world for a board. This is... FINAL NIGHT SEVEN." |
| Clash | `fn7_clash` | **your** duel locks (`phase.event` `challenge.lock`; not on a reconnect restore) | the duellist (each human duellist's client) | "CLASH! Will against will." |
| Victory | `fn7_victory` | the end screen, you won (`match.ended`) | the winner | "The night is yours. Every other god has fallen silent. You are the last one standing." |
| Defeat | `fn7_defeat` | the end screen, you fell (`match.ended`) | each player who lost | "Your hands have fallen. The table goes dark. But the world remembers what you did." |
| Duel won | `fn7_duel_won` | your duel ends with you the winner (`challenge.end`) | the winning duellist | "Your words cut deeper. The duel is yours." |
| Duel lost | `fn7_duel_lost` | your duel ends with you the loser (`challenge.end`; a draw plays nothing) | the losing duellist | "Outspoken. Outfought. Rise, and speak faster next time." |

## 4. The voices, as the docs describe them

Use these words when designing voices; do not add traits the docs don't give. Quotes are from `data/pantheon.json`
unless noted.

| Voice | As documented |
|---|---|
| **The narrator** | The existing lines' voice; the welcome has an "ancient godlike narrator" take (vo/README). The text narrator that writes captions is described in the API as "Deadpool's voice": treat that as a writing tone only; a TTS voice must be designed, never an imitation of a real actor |
| **Time** | "A punctual, bone-dry bureaucrat of eternity who has never once been late and resents everyone who tries to be. Speaks in schedules, appointments and overdue notices." |
| **Death** | "Tired, fair, darkly funny. Has seen every trick. Respects style." The rulebook adds: "Death keeps a price list" (RB 25), e.g. "I keep a price list and a list of errands; choose." |
| **Consequence** | "A cheerful, meticulous collector with an infinite ledger and a love of Rube Goldberg machines. Never angry. Always exact." |
| **Entropy** | "A giggling, sparking, delighted maniac of heat death: the only god who WANTS you to use power. Talks fast, laughs mid-sentence (ahahhaahhaaa), says yes yes YES, begs everyone to spend more bolts, and treats every catastrophe as a gift." (The brief's "gleeful mad scientist" is not in the docs; use the words above) |
| **Nature** | "Patient, enormous, amused and occasionally furious. Treats demigods as weather treats picnics." |
| **Memory** | "Elegant, gossipy, keeps everyone's secrets and sells them." |
| **Chance** | "A grinning trickster who loves an underdog and hates a sure thing." |

The writing tone of every DM answer is "history is a lens": the result is already decided by rules and dice, and the
words make it make mystical, Rube Goldberg sense ("the bridge held only because the ferryman had washed the keystone in
platinum dust at dawn"). Gods' spoken lines never start with the god's own name (the API's omen rule).

## 5. Every interaction point that could carry a voice line

**None of these exist yet.** Each needs Chris's approval, a render, an import and a hook in the Unreal client. "Max
length" is guidance derived from the pacing (a 3 s beat, a 30 s duel, a queue of three, any key skips), not a rule.
Placeholders like `<name>` are live values: a pre-rendered line cannot speak them, so either keep the line generic or
render per-name variants (unconfirmed which; ask Chris).

### 5.1 Time and the will

| Candidate | Trigger (message / event) | Who hears | Max length | Voice and tone |
|---|---|---|---|---|
| Time slowed (by `<name>`) | `window.opened` + `phase.event` `slowed` | everyone (it is public who slowed time) | ~2 s | Time: bone-dry, an appointment being opened |
| Your orders were cut | `action.received {accepted: false, reason: "cut"}` | **the cut player only** (never tell anyone else) | ~2 s | Time: an overdue notice |
| Time snaps back / your bill | `round.outcome.public.slow[]` `event: "closed"` (or `instant`) | the opener (the bill is public, but say it to the payer) | ~3 s | Time: the invoice |
| The Final Wish | `round.brief.public.final_wish: true` (beat 1) | everyone | ~4 s | narrator |
| Time's circle closes on you | your seat view `hint.kind: "danger"` about Time's circle | the player only | ~3 s | Time: you are late |
| Time Stands Still | `god_powers[]` with `god: "time"` / `phase.event` `god_power` | everyone | ~3 s | Time |

### 5.2 Powers, Entropy and the end of the world

| Candidate | Trigger | Who hears | Max length | Voice and tone |
|---|---|---|---|---|
| A mega is cast | `phase.event` `streak` with `tier: "mega"` | everyone (or the caster only: decide; megas can come several a beat) | ~2 s | Entropy: delighted, yes yes YES |
| Entropy's check (always yes) | `omens[]` with `god: "Entropy"` | everyone (public at magnitude >= 6) | ~3 s | Entropy |
| Entropy level rises (20 freak weather, 40 unreliable physics, 60 riots and worship, 80 wounds to the gods) | `round.outcome.public.entropy.level` / `key` changes | everyone | ~3 s | Entropy, more giddy each level |
| Heat death near (80) | `entropy.level` reaching the 80 band | everyone | ~3 s | Entropy, begging for one more bolt |
| Heat death: the Final Night comes at once | `entropy.heat_death` set | everyone | ~4 s | Entropy, then narrator |
| Own-hit warning | `streak.own_hit` (at once) or next beat `hint.kind: "own_hit"` | **the caster only** | ~2 s | narrator, dry |
| A mega at a junction (the hint) | `hint.kind: "mega"` | the player only | ~3 s | narrator |
| A god-tier power lands | `phase.event` `god_power`; `round.outcome.public.god_powers[]` | everyone | ~3 s | the power's god |

### 5.3 The gods' verdicts (per god)

A will's verdict arrives in `round.private.wills[].verdict` (the god's `grant` / `bargain` / `twist` / `refuse`, or
`refused` when it never reached a god; the 0.9.0 summary reads `granted` / `partial` / `refused`); god-tier powers
and big invocations also put the god's speech in `omens[]`. That makes **7 gods x 4 verdicts = 28 candidate lines**:

| God | grant | bargain | twist | refuse |
|---|---|---|---|---|
| Time | the appointment is kept | at interest | the schedule slips | overdue, denied |
| Death | the price is paid | a price list and a list of errands | darkly funny | "No." (a real refusal in the API) |
| Consequence | entered in the ledger | the bill, itemised | a Rube Goldberg chain | never angry, exact |
| Entropy | (Entropy always says yes: grant only) | | | |
| Nature | amused | patient | furious | weather at a picnic |
| Memory | a secret sold | gossip | a false memory | forgotten |
| Chance | the underdog wins | a wager | the coin lands wrong | hates a sure thing |

The cell words are tone cues taken from each persona, not scripts. Who hears: the will's author only for private
wills; everyone when it is a public omen (`round.outcome.public.omens[]`) or a god-tier power. Max ~3 s.

### 5.4 Duels

| Candidate | Trigger | Who hears | Max length | Voice and tone |
|---|---|---|---|---|
| The challenge card | the client opens the card (double-click a rival in reach; odds from `game.duel_odds`) | the challenger only | ~1.5 s (must not delay Enter = DUEL) | narrator |
| You are challenged | `phase.event` `challenge.lock` where you are `b` (or an AI lock in `round.started.public.challenges[]`) | the target only | ~2 s, after or instead of "Clash" | narrator |
| Watch: a duel on the table | `challenge.lock` for a duel you are not in | the other players (watchers) | ~2 s | narrator |
| Type now (in a duel) | `hint.kind: "duel"` (`action: "duel_type"`, urgency 3) | the duellist only | ~1 s; never while they type (any key skips) | narrator |
| A whiff | `duel.blow` with `whiff` ("you have no pistol") | the duellists | ~1.5 s | narrator, dry |
| A natural 20 / natural 1 blow | `duel.blow.verdict` `crit_success` / `crit_fail` | the duellists (or everyone) | ~1 s | narrator |
| Knockout | `challenge.end {reason: "ko"}` | everyone | ~2 s | narrator |
| A draw | `challenge.end` with `winner: null` | the duellists | ~2 s (today a draw plays nothing) | narrator |

### 5.5 Summons, falls, raisings, curses, quests

| Candidate | Trigger | Who hears | Max length | Voice and tone |
|---|---|---|---|---|
| A summon lands | `summoned` in `round.outcome.public.events[]` (accept-time: `phase.event` `summoning`) | the summoner | ~2 s | narrator |
| One of your demigods falls | your demigod in `round.outcome.public.eliminated` | the player only | ~3 s | narrator, or Death |
| A demigod falls (any) | `round.outcome.public.eliminated` | everyone | ~2 s | narrator |
| Both your demigods are down (the fallen WILL) | `players[].alive` false for you | the player only | ~4 s ("you may still plead") | Death |
| Raised by Death | `demigod.raised` (`how`: paid / on credit / quest) | everyone (public); the owner as the main listener | ~3 s | Death, calm, the price named |
| A seal or curse lands | `curse.cast` (`kind`: seal, wither, silence, bind, hex) | everyone sees the mark; **never say who cast it** | ~3 s | the ruling god, or narrator |
| A curse breaks | `curse.broken` | everyone | ~2 s | narrator |
| Quest given / step / done / failed | `quest.given`, `quest.step`, `quest.done`, `quest.failed` (public); private ones in `round.private.will_events[]` | everyone for public quests; the player only for private ones | ~3 s | the quest's god (`god` field) |
| Last Stand holds | `arms.power` | everyone | ~2 s | narrator |

### 5.6 The world

| Candidate | Trigger | Who hears | Max length | Voice and tone |
|---|---|---|---|---|
| An encounter revealed | `encounter.revealed` (`type`: monster, treasure, trap, stranger, undead, ghost, thief) | everyone | ~2 s | narrator |
| Night falls / dawn breaks | `time.phase` (`phase`: dusk / dawn) | everyone | ~2 s | narrator |
| A settlement grows to a great city | `settlement.grew` (`to`) | everyone | ~2 s | narrator |
| A shrine is raised | `shrine.built` | everyone | ~2 s | narrator |
| The world's own quake or eruption | `dm_events` `earth.quake`, `earth.eruption`, `earth.omen` | everyone | ~2 s | Nature |
| New gear | `look.changed` | the owner | ~2 s | narrator |
| The Final Night (beat 60) | the last beat (`beats_total`) and `match.ended` | everyone | ~4 s, before victory / defeat | narrator |

## 6. What you must never do

- Never use or imitate a real person's voice. Designed voices only.
- Never write a line that reveals hidden information: who typed, who was cut (except to the cut player), who cast a
  curse, a rival's health, an encounter's hidden type before it is revealed, another seat's private wills or quests.
- Never invent a rule, a number, a god's trait or an event name. If a candidate needs something the docs don't say,
  mark it unconfirmed and ask.
- Never ship a line that would fire every beat (a 3 s beat and a queue of three: it would drown the game), or a long
  line in a duel (players type, and any key skips).
- No profanity in lines. No keys, emails or personal data in reports, file names or metadata.
- Do not spend Fish Audio or other paid credits beyond what Chris approved; do not commit or push (the Mac session
  imports and hooks lines on its own branch; the backend merges).

## 7. Read

1. `../FULL_EXPLAINER.md` (section 16 and the master table, section 18).
2. Unreal repo: `production_assets/audio/vo/README.md`, `Source/FinalNight/UI/FNVoiceOver.cpp` (the queue, skip and
   once rules), the hooks in `UI/SFNStartPanel.cpp` and `UI/SFNHudLive.cpp`, and `UI/FNSoundPass.h` (sounds under
   your lines).
3. Server repo: `data/pantheon.json` (personas), `docs/CLIENT_API.md` ("Hidden information", "Next move hint",
   "Challenge mode", "Wills that matter"), `docs/DM_RULEBOOK.md` chapters 0, 12, 21, 25-27.
