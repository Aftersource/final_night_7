# The Theatre — Protocol

WebSocket at `wss://<host>/ws`, JSON text frames (max 64 KiB). One envelope for everything.
All timestamps (`deadline`, `starts_at`, `server_time`, `at`) are **Unix epoch milliseconds**.

## Envelope

```json
{ "t": "room.join", "id": "c-42", "re": null, "d": { "code": "XK4P2Q" } }
```

| Field | Meaning |
|---|---|
| `t` | Message type, `family.name`. |
| `id` | Optional client-chosen id on requests. Set it if you want a reply you can match. |
| `re` | On replies, the `id` of the request being answered. Absent on pushes. |
| `d` | Payload. **Always a JSON object**, never a string. Missing/`null` is treated as `{}`. |

**Replies.** A request with an `id` always gets exactly one reply with `re` = that id:
either the specific reply listed below, a plain `ack` (`d: {}`), or an `error`.
Requests without an `id` get no `ack` (errors are still sent, with `re` absent).

Errors: `t: "error"`, `d: { "code": "...", "message": "..." }`.
Codes: `bad_request`, `unauthorized`, `not_found`, `room_full`, `not_host`, `wrong_phase`,
`too_long`, `rate_limited`, `internal`. `message` is human-readable; switch on `code`.

After `auth.ok`, the socket is authenticated; no per-message token is needed.

**Limits.** 20 messages/sec per socket (token bucket; excess gets `rate_limited`, and a socket
that keeps flooding is closed). A socket that sends nothing for 60 s is closed, so send
`sys.ping` every ~20 s. Sockets that don't authenticate within 30 s are closed.
Display names ≤ 24 chars, actions ≤ 500 chars (all configurable server-side).

---

## sys — connection

| t | Dir | d |
|---|---|---|
| `server.hello` | S→C | `{ protocol: 1, server_version, game }` — sent on connect; `game` is the plugin id, e.g. `"director"` |
| `sys.ping` | C→S | `{}` → reply `sys.pong` |
| `sys.pong` | S→C | `{ server_time }` |

## auth — login & sessions

| t | Dir | d |
|---|---|---|
| `auth.login` | C→S | `{ display_name, client?: { kind, version } }` → reply `auth.ok` |
| `auth.resume` | C→S | `{ resume_token }` → reply `auth.ok`, then `room.snapshot` + `match.snapshot` if you were in a room |
| `auth.ok` | S→C | `{ player_id, display_name, session_token, resume_token }` |
| `auth.logout` | C→S | `{}` — leaves your room and forgets the player (resume token stops working) |

Store `resume_token`. After a dropped connection, open a new socket and send `auth.resume`
instead of `auth.login` to get your seat back. Tokens expire 10 minutes after disconnect and
survive a server restart. If a second socket resumes the same player, the older socket is closed.

## room — lobby

| t | Dir | d |
|---|---|---|
| `room.list` | C→S | `{}` → reply `room.listed` (public rooms only, up to 100) |
| `room.listed` | S→C | `{ rooms: [RoomSummary] }` |
| `room.create` | C→S | `{ visibility: "public"\|"private", settings: {} }` → reply `room.snapshot`; you are host |
| `room.join` | C→S | `{ room_id }` or `{ code }` → reply `room.snapshot` |
| `room.quickjoin` | C→S | `{}` → reply `room.snapshot` (joins an open public quick-play room, or makes one) |
| `room.leave` | C→S | `{}` → `ack`, then push `room.left` |
| `room.ready` | C→S | `{ ready: bool }` |
| `room.settings` | C→S (host) | `{ settings: {} }` — validated by the game; lobby only |
| `room.bot_add` | C→S (host) | `{ count? }` — add `count` bots (default 1); `room_full` if they don't all fit |
| `room.bot_remove` | C→S (host) | `{ seat }` |
| `room.kick` | C→S (host) | `{ seat }` |
| `room.start` | C→S (host) | `{}` — everyone must be ready; bots fill up to the game's minimum |
| `room.rematch` | C→S (host) | `{}` — from `post_match` back to `open` |
| `room.chat` | C→S | `{ text }` |
| `room.snapshot` | S→C | full `Room` (reply to join/create, and on resume) |
| `room.updated` | S→C | full `Room` (sent on any change) |
| `room.countdown` | S→C | `{ starts_at }` — match starts then unless someone un-readies |
| `room.chat_msg` | S→C | `{ seat, display_name, text, at }` |
| `room.left` | S→C | `{ room_id, reason: "left"\|"kicked"\|"closed" }` — you are no longer in the room |

**Starting.** Rooms from `room.create` start when the host sends `room.start`.
Quick-play rooms (from `room.quickjoin`) start automatically once every human is ready.
Either way the server sends `room.countdown`, then `match.started`.
You can be in only one room at a time (`bad_request` "leave your current room first").

**Leaving mid-match.** An active player who leaves (or whose resume token expires) is replaced
by a bot for the rest of the match. An eliminated player who leaves gets status `left`. If every human disconnects, the match pauses at the next round and
resumes when someone reconnects. The room closes when no humans remain.

## match — gameplay

| t | Dir | d |
|---|---|---|
| `match.started` | S→C | `{ match_id, seats: [Seat], rules: {} }` — `rules` = the room's validated settings |
| `match.snapshot` | S→C | see below (on resume/reconnect) |
| `phase.changed` | S→C | `{ round, phase: "start"\|"brief"\|"compose"\|"resolve"\|"reveal"\|"choose"\|"commit", deadline? }` |
| `phase.progress` | S→C | `{ round, phase, done, total, note? }` (submissions in, resolve progress) |
| `round.started` | S→C | `{ round, public: {}, private: {} }` — effects before anyone acts (only sent if the game has any) |
| `round.brief` | S→C | `{ round, public: {}, private: {} }` |
| `action.submit` | C→S | `{ round, text, fields?: {} }` → reply `action.received` (resubmit to edit while Compose is open) |
| `action.received` | S→C | `{ round, accepted: true }` |
| `round.outcome` | S→C | `{ round, public: {} }` — what everyone sees |
| `round.private` | S→C | `{ round, private: {}, choices: [Choice] }` — what only you see |
| `choice.submit` | C→S | `{ round, choice_id }` → reply `choice.received` (resubmit to change while Choose is open) |
| `choice.received` | S→C | `{ round, accepted: true }` |
| `round.committed` | S→C | `{ round, scores: { "<seat>": number }, public_state: {} }` — `public_state` is the game's public view |
| `seat.eliminated` | S→C | `{ seat, round, reason }` — to everyone; followed by `room.updated` |
| `round.private_state` | S→C | `{ round, state: {} }` — after each commit, your own view of the game (players and spectators) |
| `match.ended` | S→C | `{ match_id, winners: [seat], summary: {} }` — room moves to `post_match` |

Phase order: **start → brief → compose → resolve → reveal → choose → commit**.
Every timed phase carries a `deadline`. When it passes, missing actions get the game's default
action and missing choices get the game's default choice; the match never waits forever. If `choices` is empty
the Choose phase is skipped. Untimed phases (start, brief, resolve, commit) send `phase.progress`
at least every 5 s; counted progress is coalesced to at most ~4 frames/s.

**Elimination & spectators.** Seats have a `status`: `active`, `eliminated` or `left`. Only
active seats act and are waited for (submitting while eliminated → `wrong_phase`). Eliminated
players stay in the room as spectators: they receive every public message and their own
`round.private_state`, and can chat. The match ends when the game says so or when too few seats
are active.

```ts
MatchSnapshot {
  match_id, round, phase: Phase | null, deadline: number | null,
  public_state: {},   // the game's public view
  scores: { "<seat>": number },
  started: {},        // public part of this round's round.started, if any
  brief: {},          // public part of the current round's brief, if any
  outcome: {},        // public outcome of the current round, if revealed
  you: {
    seat, status: "active" | "eliminated" | "left",
    private_state: {},                       // the game's view for your seat
    pending: "action" | "choice" | null,     // always null for spectators
    started: {}, brief: {}, outcome: {},     // private parts of this round's messages
    choices: [Choice], action: string | null, choice: string | null
  } | null
}
```

`public` / `private` / `fields` / `rules` / `summary` contents are defined by the game plugin
(for The Director, see [`DIRECTOR.md`](DIRECTOR.md)).

---

## Types

```ts
Room        { room_id, code, visibility, host_seat: number | null,
              state: "open"|"countdown"|"in_match"|"post_match",
              seats: Seat[], settings: {}, limits: { min_players, max_players }, game }
RoomSummary { room_id, code, seats_taken, max_players, state }
Seat        { seat: number, player_id?, display_name, is_bot, ready, connected,
              status: "active"|"eliminated"|"left", eliminated_round? }
Choice      { choice_id, label, detail?, kind?: string }
```

## HTTP

| Path | Meaning |
|---|---|
| `GET /healthz` | `ok` (200) while the server is up |
| `GET /stats` | `{ version, game, players, connected, rooms }` |

## Example session

```
S→C server.hello        {protocol:1, game:"director"}
C→S auth.login          {display_name:"alice"}                      id:"1"
S→C auth.ok             {player_id, session_token, resume_token}     re:"1"
C→S room.quickjoin      {}                                           id:"2"
S→C room.snapshot       {room_id, seats:[...], state:"open"}         re:"2"
C→S room.ready          {ready:true}                                 id:"3"
S→C ack                 {}                                           re:"3"
S→C room.countdown      {starts_at}
S→C match.started       {match_id, seats}
S→C phase.changed       {round:1, phase:"start"}
S→C phase.changed       {round:1, phase:"brief"}
S→C round.brief         {round:1, public:{...}, private:{...}}
S→C phase.changed       {round:1, phase:"compose", deadline}
C→S action.submit       {round:1, text:"I sneak backstage and swap the props"}  id:"4"
S→C action.received     {round:1, accepted:true}                     re:"4"
S→C phase.progress      {round:1, phase:"compose", done:2, total:4}
S→C phase.changed       {round:1, phase:"resolve"}
S→C phase.changed       {round:1, phase:"reveal"}
S→C round.outcome       {round:1, public:{...}}
S→C round.private       {round:1, private:{...}, choices:[...]}
S→C phase.changed       {round:1, phase:"choose", deadline}
C→S choice.submit       {round:1, choice_id:"spotlight:2"}            id:"5"
S→C choice.received     {round:1, accepted:true}                     re:"5"
S→C phase.changed       {round:1, phase:"commit"}
S→C round.committed     {round:1, scores:{...}, public_state:{...}}
S→C round.private_state {round:1, state:{...}}
...
S→C match.ended         {winners:[2], summary:{...}}
```
