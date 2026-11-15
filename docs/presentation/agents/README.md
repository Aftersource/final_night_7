# Final Night 7: the production agents' explainers

*Designers: Chris and Benito.* One full explainer of the game end to end, and one brief per kind of agent: the subset
that agent must know, its interaction points, what it must never do, and what to read.

**Read `FULL_EXPLAINER.md` first, whatever your job is.** Its section 18, "INTERACTION POINTS", is the master table of
every moment a player or the world triggers something: the trigger, the server message (from `docs/CLIENT_API.md`),
what the client shows, and the sound and voice cue (or "none yet").

| Agent | Brief | Covers |
|---|---|---|
| Voiceover engine (TTS, Chris's pipeline) | [`briefs/VOICEOVER.md`](briefs/VOICEOVER.md) | the 7 existing lines and their hooks, how voice plays in the client, every candidate moment with its trigger event, who hears it, max length and the documented voices |
| Unreal client: Chris's Mac session, Benito's Windows builder and gamedesign session | [`briefs/UNREAL_CLIENT.md`](briefs/UNREAL_CLIENT.md) | the client contract, the locked controls (HAND_OF_GOD §6) and today's demigod double-click rules, every server message to draw |
| Server agents (backend, rules, media) | [`briefs/SERVER_AGENTS.md`](briefs/SERVER_AGENTS.md) | rules first, no invented rules, schema gates, replay equality, hidden information, the deploy routine |
| Art and Meshy | [`briefs/ART_AND_MESHY.md`](briefs/ART_AND_MESHY.md) | the two styles (Benito's rule), the house style, image kinds, Meshy only through the backend and the asset registry, budgets |
| Balance and design (Benito's design sessions) | [`briefs/BALANCE_AND_DESIGN.md`](briefs/BALANCE_AND_DESIGN.md) | every setting that tunes the game (DM_RULEBOOK §22 and the room settings) and what is measured |
| Video and pitch | [`briefs/VIDEO_AND_PITCH.md`](briefs/VIDEO_AND_PITCH.md) | the 30-60 s video shot by shot, the pitch, what reads best on screen, honest limits |

Rules for everyone: never invent a rule, number, event name or feature (unclear: leave it out or mark it
unconfirmed, and ask the backend); never put keys, tokens, addresses or personal data in a repo, log or screenshot (the
repo will be public); follow `docs/PLAYBOOK.md` for git and ownership.

Sources: `docs/presentation/design/GAME_DESIGN.md`, `docs/DM_RULEBOOK.md`, `docs/CLIENT_API.md`,
`docs/TIME_TRAVEL.md`, `docs/EARTH_SIM.md`, `docs/MEDIA.md`, `docs/ACCOUNTS.md`, `docs/DUEL_MOVES.md`,
`docs/PLAYBOOK.md`, `BUILDLOG.md`, `data/pantheon.json`; and the Unreal repo's `HAND_OF_GOD.md`,
`EVENTS_IN_THE_WORLD.md`, `FOR_BENITO.md`, `production_assets/audio/vo/README.md` and the voice and sound code.
