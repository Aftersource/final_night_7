# Final Night 7: the playbook (Chris's backend ↔ Benito's frontend)

Read this whole file before acting, and again after every compaction. It is how the two teams work together.

## 1. The team
| Who | Owns | Address |
|---|---|---|
| **Chris** (designer) | The game's direction. Final say on everything. | through his session |
| **Benito** (designer) | Design, balance, the exact UX, playtesting, rule design | through his sessions |
| **Backend session** | The Theatre engine integration, Final Night 7's simulation, LLM minds, rules, storage, recording, hosting, server releases | `#chris/final-night-7` |
| **Bridge** (Benito's agent) | The client (browser, then Unreal), and the intermediary between Benito and the backend. Runs the server, simulator and admin tools on Benito's behalf. | `#benito-linux/finalnight-linux` (to be renamed `final-night-7-benito-bridge`) |
| **Shared ground** | Image, video and character generation: designed together | both |

The platform is **The Theatre**. The game is **Final Night 7**. The designers' words are kept verbatim in
`BUILDLOG.md`, which is the source of intent.

## 2. Two servers: STABLE and DEV
- **STABLE**: the URL Chris gave you (behind the access key). This is the calm version Benito playtests and designs
  rules against. It changes **only** through an announced release (§4). It is never touched mid-playtest without
  warning.
- **DEV**: the backend's private servers and the `main` branch. These churn constantly; don't build against `main` mid-day.
  Build against the **release tag** that STABLE runs.
- **Releases are git tags** on `Aftersource/final_night_7`: `playtest-N`. STABLE always runs exactly one tag.
  `GET /stats` and `server.hello` report the rules version; each release note gives the tag, rules version and engine
  commit.

## 3. The plays (how each kind of work flows)
**Play 1: a bug.** Bridge → backend, with this template:
```
BUG <short title>
server: STABLE (playtest-N) | room code: XXXXXX | match id: <from /admin/matches> | beat: N
expected: ...   got: ...   frames/queries: <the exact request and response, trimmed>
severity: blocks-playtest | wrong-but-playable | cosmetic
```
The backend answers within the hour with an ETA. A fix ships in the next release unless it blocks a playtest, in
which case it gets a hotfix release, still announced.

**Play 2: a change request** (new field, query, rename, knob or rule). Bridge → backend: what Benito wants, why (the
design intent), and the exact payload shape the client wants. The backend replies yes / no / alternative, plus which
release it lands in. Field renames are cheap; ask first.

**Play 3: balance.** Benito tunes through room settings: every setting is a knob, listed by `GET /sim/settings` once
the simulator API is live. Workflow: play with knobs on STABLE (`llm:false` for speed) → run batches with
`POST /sim/run` / `POST /sim/compare` → send the backend the settings Benito likes. The backend makes them the defaults
in the next release. Never change code to balance; change settings.

**Play 4: a release** (backend → bridge).
1. Both test suites are green (backend: `cargo test --workspace`; bridge: the repo suite plus `client/fn7`).
2. The backend verifies the build on a private DEV server, then **announces before deploying**, with the tag, the
   changes, and any payload changes (before → after).
3. The bridge replies "ok" (or "hold": Benito is mid-playtest).
4. The backend deploys to STABLE and verifies through the public URL: a full match, the queries, admin.
5. The bridge pulls the tag and confirms its client still passes.

**Play 5: the bridge pushes.** The bridge may push to `Aftersource/final_night_7` **only when the backend explicitly
instructs it**, and then only what was asked:
- on a branch `bridge/<topic>` unless told otherwise;
- with the commit message the backend gives;
- never to `main`, never tags, never force-push.

Client code lives in Benito's own repo, not ours.

**Play 6: secrets.** The access key and admin token live in environment variables (`FN7_KEY`, `FN7_ADMIN`). Never put
them in a repo, a log, a screenshot or a message to anyone but the backend. If one leaks, tell the backend at once; it
rotates them.

**Play 7: the unexpected.** Anything not covered here: ask the backend first, then act. When in doubt, don't push,
don't deploy, don't delete.

## 4. Release notes format
```
RELEASE playtest-N  (rules X.Y.Z, engine <commit>)  deploy at: <time>
new: ...
fixed: ...
payload changes: <field: before → after>   (none = "none")
client action needed: ...                  (none = "none")
```

## 5. What the bridge can use (all on STABLE)
- Game: `wss://<URL>/ws?key=<KEY>`: protocol `docs/CLIENT_API.md`, reference client `docs/clients/fn7_client.mjs`.
- Game queries over HTTP: `GET /api/rooms/{code}/query/{name}?key=<KEY>` (public ones): `globe` (main poll),
  `events?since=`, `region`, `node`, `person`, `army`, `standings`, `powers`, `gods`, `piles_final`.
- Admin (time travel, fork, recordings): `Authorization: Bearer <ADMIN>`. See `docs/AGENT_GUIDE.md` §11.
- Simulator API `/sim/*` (balance batches): lands in an upcoming release, behind the admin token; usage in
  `docs/AGENT_GUIDE.md` §12 (check: `GET /sim/settings` answers 401/200 instead of 404 once it is live).
- Images: `docs/MEDIA.md` (placeholders until Chris sets a budget).

## 6. Rhythm
- The bridge sends a short status when it finishes a client milestone, or when Benito finishes a playtest (what felt
  wrong, settings used, match ids).
- The backend sends release notes and answers requests.
- Everything is recorded on the server. Quote match ids instead of describing; the backend can replay any match exactly.
- Hard deadline: the packaged Unreal build is due **Nov 15 2026, 8:59 PM EST**. Releases slow down as it approaches:
  after **Nov 15, 12:00 PM EST**, only blocking fixes.
