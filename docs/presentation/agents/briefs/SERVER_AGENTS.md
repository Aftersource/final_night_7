# Brief: the server agents (backend, rules, media, engine requests)

*For every session that writes `final_night_7` (the Rust plugin on The Theatre). Designers: Chris and Benito. Read
`../FULL_EXPLAINER.md` first; this is your subset.*

## 1. The principles you work under

1. **Rules first, the LLM phrases.** "The rules and dice are an engine that has already decided" (RB 0). Every number
   (dice, damage, prices, move classes, verdict odds, hints, token scenes) comes from deterministic, seeded rules; an
   LLM only names, phrases and makes it make mystical sense. The DM never overrules a die. Every LLM call has a rules
   fallback, and a late call (`mind_timeout_ms`) takes it: **the beat never waits** (RB 1, 5).
2. **No invented rules.** Every rule traces to Chris's or Benito's words in `BUILDLOG.md` and to the rulebook
   (`docs/DM_RULEBOOK.md`, rules 0.9.0). When a request is unclear, ask; do not fill the gap. (Today's lesson: a
   made-up "mercy" rule for a lone dead player was cancelled; Chris wants time travel, which needs `room.rewind` from
   the engine, requested and not built.)
3. **One question, one roll, forever.** Every roll is seeded by the match seed and its query and recorded; asked
   again, the same face (RB 2.5).
4. **Schema gates.** Every rule change is gated on the state schema (`meta.schema`) and, where it has one, a setting:
   older states and recordings replay unchanged. Examples: wills need schema 10 and `wills_enabled`; looks 11; the
   Earth 13 and `earth_sim_enabled`; the hint 18; token play 19; earth effects 20; duel words 21; clear words 22
   (`World::clear_words_rules`). Settings recorded before a feature read back its old value.
5. **Replay equality.** A recording re-computes every snapshot and compares them byte for byte: **0 differences**,
   LLM answers included. A killed and resumed, paused, joined, or forked match replays exactly. The live view and the
   resolve run the same code on the same ordered actions (e.g. `duel::apply_act`). No accepted instant action is lost
   to a server kill: it is journalled before the next one is taken (TIME_TRAVEL §7).
6. **Hidden information** (API "Hidden information"). Never send: a rival's health (except the duel's health bars),
   who else typed or submitted or was cut, a rival's standing orders or armies' orders, a hidden encounter's type,
   anything private in another seat's book, who cast a curse. The hint reads only what the seat already sees. Private
   facts in `game.memory_tree` are counted, never shown.
7. **The client contract is yours.** `docs/CLIENT_API.md` and `docs/schemas/` are the single source for every payload
   (PLAYBOOK Play 10). Schemas are generated from the code (`cargo run -p fn7-tools --bin fn7-schemas`); a test fails
   if they go stale, and every example in `docs/examples/` is validated against its schema (unknown keys rejected).
   New fields are optional and additive; renames are asked first.
8. **Balance through settings, never code** (Play 3). Every number Benito tunes is a room setting listed by
   `GET /sim/settings`.

## 2. The game you serve (the server's view)

- One beat = one Theatre round (`flow_ms` 3 s; 12 s in the house lobby); `beats_total` 60. Order: Start → Brief →
  Compose → Resolve → Reveal → Commit (RB 17).
- Two input paths: slow-time windows for text (`action.begin` → `window.opened`; the opener's Enter closes it, later
  submits get `cut`), instant actions for casts, summons, challenges and duel lines.
- What you emit every beat: `round.outcome.public` (`dice[]`, `dm_events[]`, `casts`, `events`, `omens`, `slow`,
  `entropy`, `earth`, `scenes`, `looks`, `duels_open`, `duels`, `god_powers`, `summons`, `quests`, `curses`, `wills`),
  `round.private` (`wills[]`, `will_events[]`, `hands`, `news`), the seat view (with `hint` and `goal`), and the
  public view at commit. The master table of every event is in `../FULL_EXPLAINER.md` §18.
- The DM's memory by place and time (RB 3-4) feeds the minds the newest 6 settled public facts per region labelled
  "never contradict them"; the Earth field's one factual line goes into every prompt that acts at a place.
- The media layer writes every image prompt from the state; a client never sends one. Every image, Meshy call and
  generated file goes into the asset registry (`docs/ASSETS.md`).

## 3. The deploy routine (PLAYBOOK §2-§4, BUILDLOG session log)

1. Both test suites green (`cargo test --workspace`; the bridge's suite).
2. Verify on a private DEV server. **Announce before deploying**: the tag, the changes, any payload changes (before →
   after), "client action needed".
3. Wait for the bridge's "ok" (or "hold": a playtest is running). STABLE is never touched mid-playtest without warning.
4. Deploy STABLE: the `final_night_7` worktree is detached at `origin/main` so its `data/` files match the binary
   (`git checkout --detach origin/main` before each deploy); start **only** through the stable start script in
   `runs/lan/` (it loads the local env files and the Meshy and sprite switches).
5. Verify through the public URL: a full match, the queries, admin.
6. Play 8: both builders (Windows and Mac) build the same client commit and connect; the release is tagged
   `playtest-N` only when the server is verified and both builds connect. A failure anywhere is a BUG (Play 1).

## 4. Never

- Never invent a rule, number, event name or feature. Never let an LLM decide an outcome.
- Never break replay equality or change an old schema's behaviour.
- Never leak hidden information through a payload, a hint, a caption, an image prompt or a log.
- Never commit secrets: keys and tokens live in env vars and git-ignored files (Play 6). No keys, addresses or tunnel
  URLs in docs; the repo will be public.
- Never deploy unannounced, force-push, rewrite `main`, push tags you don't own, or write Unreal source (the backend
  merges and verifies only).
- Never spend image, Meshy or LLM budget beyond what Chris set (the image budget was nearly spent at the 17:00 EST log;
  raising it is her call).
- No attribution lines in commits; bracketed commit names (Play 9).

## 5. Read

`../FULL_EXPLAINER.md`; `docs/DM_RULEBOOK.md` (all), `docs/CLIENT_API.md`, `docs/TIME_TRAVEL.md`,
`docs/EARTH_SIM.md`, `docs/MEDIA.md`, `docs/ASSETS.md`, `docs/ACCOUNTS.md`, `docs/DUEL_MOVES.md`, `docs/PLAYBOOK.md`,
`docs/AGENT_GUIDE.md`, the last ~400 lines of `BUILDLOG.md`.
