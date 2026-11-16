#!/usr/bin/env node
// Final Night 7 reference client. Node >= 22 (built-in WebSocket), no dependencies.
//
//   node fn7_client.mjs --url ws://localhost:8080/ws --name Juno --create            # make a room, print its code
//   node fn7_client.mjs --url ws://localhost:8080/ws --name Kai --join ABC123        # join that room
//   node fn7_client.mjs ... --auto --beats 5                                          # play by itself (scripted lines)
//   node fn7_client.mjs ... --log frames.jsonl                                        # every frame, both directions, as JSONL
//   node fn7_client.mjs ... --script lines.txt                                        # "<delay_ms> <input line>" per line, typed for you
//   node fn7_client.mjs --url ws://HOST:PORT/ws --resume <resume_token> --auto        # take your seat back (a dropped
//        connection, or a forked match: POST /admin/fork returns one resume token per human seat)
//
// Two input paths, kept separate (CLIENT_API.md, "Real-time input"):
//  - Text entry (the God of Time interaction): "/slow" sends action.begin (you started typing). If the server opens a
//    window you get window.opened (everyone does, and a phase.event says who slowed time). Type "l: ..." (left hand) /
//    "r: ..." (right hand) lines and press Enter on an empty line: your orders go as action.submit with the open
//    window's window_id (or as a plain Compose action when no window is open). If you opened the window, your Enter
//    closes it and everyone who hadn't submitted is cut off; if someone else's Enter came first you get a private
//    "cut" reply. Typing an order line without "/slow" first sends action.begin the moment you start.
//  - Casts (god powers, any time): "b: <target>" throws a lightning bolt; "c: <power> <target>" casts a power from the
//    palette (round.brief.public.powers). Each is an instant action (instant: true); everyone sees its streak at once
//    (phase.event). A target is a ref (dg:2), "lon,lat", or a name. Casts never carry orders.
//  "z: region" / "z: character ch:12" -> zoom.
//  Mid-match join (room setting allow_midmatch_join): --join CODE during a match; you play from the next beat
//  (seat.joined). Until then your private payloads are null.  The host types "/start" in the lobby once everyone is ready.

import fs from "node:fs";
import readline from "node:readline";

const arg = (k, d) => { const i = process.argv.indexOf(`--${k}`); return i < 0 ? d : (process.argv[i + 1] ?? true); };
const flag = (k) => process.argv.includes(`--${k}`);
const URL_ = arg("url", "ws://localhost:8080/ws");
const NAME = arg("name", "player");
const JOIN = arg("join", null);
const CREATE = flag("create");
const AUTO = flag("auto");
const BEATS = Number(arg("beats", 10));
const BEAT_MS = Number(arg("beat-ms", 15000));
const LOG = arg("log", null);
const SCRIPT = arg("script", null);
const SETTINGS = JSON.parse(arg("settings", "{}"));
const RESUME = arg("resume", null);
const log = LOG ? fs.createWriteStream(LOG, { flags: "a" }) : null;

const ws = new WebSocket(URL_);
let nextId = 1;
const send = (t, d = {}, withId = true) => {
  const env = withId ? { t, id: `c${nextId++}`, d } : { t, d };
  log?.write(JSON.stringify({ dir: "out", at: Date.now(), ...env }) + "\n");
  ws.send(JSON.stringify(env));
};

const st = { me: null, seat: null, host: false, round: 0, view: null, arena: null, buf: { left: [], right: [], zoom: null },
  submitted: 0, deadline: 0, window: null, typingIn: null, begun: false, powers: [], inCompose: false, pending: null };
const say = (...a) => console.log(...a);
const r1 = (x) => (typeof x === "number" ? Math.round(x * 10) / 10 : x);

// Orders: into the open slow window (any phase), else as this round's Compose action (one per round; resubmitting
// edits it). Orders written while neither is open wait for the next Compose.
function sendOrders(fields, windowId = st.window?.id) {
  if (!st.view?.alive) return;
  if (windowId) {
    // The window you typed in, even if it has just closed: then the server answers with a private "cut".
    send("action.submit", { round: st.round, text: "", fields, window_id: windowId });
  } else if (st.inCompose) {
    send("action.submit", { round: st.round, text: "", fields });
    st.submitted = st.round;
  } else {
    st.pending = fields;
  }
}

// Enter: your orders (closes your window; or lands in someone else's if you typed during it and beat their Enter).
function submit() {
  const { left, right, zoom } = st.buf;
  if (!left.length && !right.length && !zoom) return;
  sendOrders({ left: left.join("\n") || null, right: right.join("\n") || null, zoom }, st.typingIn ?? st.window?.id);
  st.buf = { left: [], right: [], zoom: null };
  st.begun = false;
  st.typingIn = null;
}

// "I started typing": the God of Time interaction. Never broadcast; the server may open a window.
function begin() { if (!st.begun) { st.begun = true; st.typingIn = st.window?.id ?? null; send("action.begin", {}); } }

function target(s) {
  const m = s.match(/^\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*$/);
  return m ? [Number(m[1]), Number(m[2])] : s.trim();
}
// A god power: an instant action, accepted in any phase (2/s, burst 4).
function cast(power, where) {
  send("action.submit", { round: st.round, text: "", fields: { cast: { power, target: target(where) } }, instant: true });
}

function autoCast() {
  const rivals = st.view.visible.filter((e) => e.kind === "demigod" && e.ref !== st.view.demigod);
  if (st.view.you.bolts >= 1 && rivals.length && st.round % 3 === 1) return rivals[0].ref;
  return null;
}

function autoLines() {
  const v = st.view, rivals = v.visible.filter((e) => e.kind === "demigod" && e.ref !== v.demigod);
  const crowd = v.visible.find((e) => e.kind === "cluster");
  const left = [], right = [];
  if (rivals.length && v.you.health >= 6) left.push(`attack ${rivals[0].name}`);
  else if (crowd) left.push(`recruit the ${crowd.name}`);
  if (v.you.debt_open > 2) right.push("slow time and delay my debt");
  else if (rivals.length && !v.hands[1].subagents.length) right.push(`send a scout to spy on ${rivals[0].name}`);
  else right.push("summon a storm over my enemies");
  return { left, right };
}

ws.onopen = () => say(`connected ${URL_}`);
ws.onclose = (e) => { say(`closed (${e.code})`); process.exit(0); };
ws.onerror = (e) => say("socket error", e.message ?? "");
setInterval(() => ws.readyState === 1 && send("sys.ping", {}, false), 20000);

ws.onmessage = (ev) => {
  const m = JSON.parse(ev.data);
  log?.write(JSON.stringify({ dir: "in", at: Date.now(), ...m }) + "\n");
  const { t, d = {} } = m;
  switch (t) {
    case "server.hello":
      if (d.game !== "final_night_7") say(`warning: server game is ${d.game}`);
      if (RESUME) send("auth.resume", { resume_token: RESUME });
      else send("auth.login", { display_name: NAME, client: { kind: "fn7_client.mjs", version: "1" } });
      break;
    case "auth.ok":
      st.me = d.player_id;
      if (RESUME) { say(`resumed as ${d.display_name}`); break; } // room.snapshot + match.snapshot follow
      if (CREATE) send("room.create", { visibility: "private", settings: { beats_total: BEATS, beat_ms: BEAT_MS, ...SETTINGS } });
      else if (JOIN) send("room.join", { code: String(JOIN).toUpperCase() });
      else send("room.quickjoin", {});
      break;
    case "room.snapshot":
    case "room.updated": {
      const mine = d.seats.find((s) => s.player_id === st.me);
      st.seat = mine?.seat ?? st.seat;
      st.host = d.host_seat === st.seat;
      if (t === "room.snapshot") {
        say(`room ${d.code} (${d.state}) seats: ${d.seats.map((s) => s.display_name).join(", ")}${st.host ? "  [you are host: type /start]" : ""}`);
        if (d.state === "open") {
          send("room.ready", { ready: true });
          if (AUTO && st.host) setTimeout(() => send("room.start", {}), 1500);
        }
      }
      break;
    }
    case "match.snapshot":
      st.round = d.round ?? st.round;
      say(`match snapshot: round ${d.round}, phase ${d.phase}${d.you ? `, your seat ${d.you.seat}` : ""}`);
      if (d.you?.private_state) st.view = d.you.private_state;
      break;
    case "match.started": say(`match started: ${d.seats.length} seats, rules ${JSON.stringify(d.rules)}`); break;
    case "round.started": say(`\n=== beat ${d.round}  arena ${r1(d.public.arena_radius_km)} km  alive ${d.public.alive}${d.private?.outside_arena ? "  YOU ARE OUTSIDE THE ARENA" : ""}`); break;
    case "round.brief":
      st.view = d.private; st.arena = d.public.arena; st.powers = d.public.powers ?? [];
      // No private view: a mid-match joiner before their first beat (seat status "joining"), or one with no demigod.
      if (!st.view) { say("   (no demigod of yours this beat: joining or spectating)"); break; }
      say(`you: health ${r1(st.view.you.health)}  bolts ${st.view.you.bolts}  debt ${r1(st.view.you.debt_open)}  zoom ${st.view.zoom}  entropy ${r1(d.public.entropy?.value)} (${d.public.entropy?.key})`);
      break;
    case "phase.changed":
      st.inCompose = d.phase === "compose";
      if (d.phase === "compose") {
        st.round = d.round; st.deadline = d.deadline ?? 0;
        if (AUTO && st.view) {
          // Text entry every third beat (a Time interaction: begin, write, Enter), a bolt now and then.
          const c = autoCast();
          if (c) cast("lightning", c);
          if (st.round % 3 === 0) { begin(); Object.assign(st.buf, autoLines()); setTimeout(submit, 400); }
        } else if (st.pending) { const f = st.pending; st.pending = null; sendOrders(f); }
      }
      break;
    case "window.opened":
      st.window = { id: d.window_id, by: d.by_seat };
      if (st.begun && st.typingIn == null) st.typingIn = d.window_id;
      say(`   ⧗ time slows (window ${d.window_id}, seat ${d.by_seat}${d.by_seat === st.seat ? " = you" : ""}, until ${new Date(d.deadline).toLocaleTimeString()})`);
      if (st.pending) { const f = st.pending; st.pending = null; sendOrders(f); }
      break;
    case "window.closed":
      if (st.window?.id === d.window_id) st.window = null;
      say(`   ⧗ time snaps back (window ${d.window_id}: ${d.reason}${d.by_seat != null ? `, seat ${d.by_seat}` : ""})`);
      break;
    case "phase.event": {
      const p = d.public ?? {};
      if (p.event === "slowed") say(`   ⧗ ${p.text}`);
      else if (p.event === "streak") say(`   ⚡ ${p.by} casts ${p.power_name} at ${p.target_name ?? p.place ?? "?"} (${p.region ?? "?"}) [seq ${p.seq}]`);
      else say(`   ! ${JSON.stringify(p)}`);
      break;
    }
    case "action.received":
      if (d.accepted === false && d.reason === "cut") say(`   YOUR ORDERS WERE CUT OFF${d.window_id ? ` (window ${d.window_id})` : ""}: the initiator pressed Enter first`);
      else if (d.accepted === false) say(`   rejected: ${d.reason}`);
      else if (d.window_id) say(`   (orders in, window ${d.window_id})`);
      else if (d.instant) say("   (cast accepted)");
      break;
    case "round.outcome": {
      // The narrator runs one beat late: previous_caption describes beat caption_beat (null on the first beat).
      const p = d.public, cap = p.previous_caption ?? p.caption;
      if (cap) say(`>> [beat ${p.caption_beat ?? "?"}] ${cap}`);
      (p.slow ?? []).forEach((x) => say(`   ⧗ ${x.text}${x.event === "closed" ? ` (${x.how}, ${x.after_ms} ms, debt ${x.debt})` : x.event === "open" ? ` (window ${x.window_id}, ${x.ms_left} ms left)` : ` (debt ${x.debt})`}`));
      (p.casts ?? []).forEach((c) => say(`   ⚡ ${c.by} casts ${c.power_name}${c.fizzled ? ` — fizzled: ${c.fizzled}` : ` on ${c.place} (${c.region})`}`));
      (p.events ?? []).filter((e) => e.kind !== "cast").slice(0, 6).forEach((e) => say(`   ◦ ${e.text} [${e.region ?? "?"}]`));
      if (p.entropy) say(`   entropy ${r1(p.entropy.value)}/${p.entropy.max} (${p.entropy.key})${p.entropy.heat_death ? "  HEAT DEATH" : ""}`);
      (p.omens ?? []).forEach((o) => say(`   ${o.god}: ${o.speech}`));
      if (p.eliminated?.length) say(`   fell: ${p.eliminated.join(", ")}`);
      break;
    }
    case "round.private":
      if (!d.private) break; // joining / no demigod
      d.private.hands.forEach((h, i) => {
        if (h.said) say(`   ${i ? "right" : "left"}: ${h.said}`);
        h.subagents.forEach((a) => say(`     ${a.kind} ${a.name} (${a.agent}${a.body ? `, body ${a.body}` : ""}): ${a.line}`));
      });
      // Your own invocations (even small ones) and the public rulings; news is newest first.
      d.private.omens.forEach((o) => say(`   ~ ${o.god}: ${o.speech}`));
      d.private.news.forEach((n) => say(`   · ${n.text} (${n.how}, ${n.row})`));
      break;
    case "round.private_state": st.view = d.state; break;
    case "seat.eliminated": say(d.seat === st.seat ? `you fell: ${d.reason} (spectating)` : `   seat ${d.seat} fell: ${d.reason}`); break;
    case "seat.joined": say(d.seat === st.seat ? `YOU JOIN at beat ${d.round} (seat ${d.seat}${d.new_seat ? ", new seat" : ", took over a bot"})` : `   ${d.display_name} joins at beat ${d.round} (seat ${d.seat})`); break;
    case "seat.revived": say(d.seat === st.seat ? `YOU ARE BACK: ${d.reason}` : `   seat ${d.seat} is back: ${d.reason}`); break;
    case "match.ended":
      if (d.summary.last_caption) say(`>> [beat ${d.summary.last_caption.beat}] ${d.summary.last_caption.caption}`);
      // winners: [] means a built-in AI demigod won (champion_seat null).
      say(`\nMATCH OVER. champion: ${d.summary.champions.map((c) => c.name).join(", ")}${d.summary.champion_seat == null ? " (built-in AI)" : ` (seat ${d.summary.champion_seat})`}\n${d.summary.final_cut?.story ?? ""}`);
      setTimeout(() => process.exit(0), 300);
      break;
    case "error": say(`! ${d.code}: ${d.message}`); break;
  }
};

function onLine(line) {
  const s = line.trim();
  if (s === "/start") return send("room.start", {});
  if (s === "/slow") return begin();
  if (s === "") return submit();
  const [, k, rest] = s.match(/^(l|left|r|right|z|zoom|b|bolt|c|cast)\s*:\s*(.*)$/i) ?? [null, "l", s];
  const key = k.toLowerCase()[0];
  if (key === "b") return cast("lightning", rest);
  if (key === "c") { const [power, ...where] = rest.split(/\s+/); return cast(power, where.join(" ")); }
  if (key === "z") { const [level, focus] = rest.split(/\s+/); st.buf.zoom = focus ? { level, focus } : level; }
  else { begin(); st.buf[key === "r" ? "right" : "left"].push(rest); }
}

if (SCRIPT) {
  // "<delay_ms> <line>": each line typed after the previous one, delay_ms later ("" lines = Enter).
  const steps = fs.readFileSync(SCRIPT, "utf8").split("\n").filter((l) => l.trim() && !l.startsWith("#"))
    .map((l) => { const m = l.match(/^(\d+)\s?(.*)$/); return [Number(m[1]), m[2] ?? ""]; });
  ws.addEventListener("open", async () => {
    // Time 0 is the connection; a line "<ms> /start" from the host starts the match.
    for (const [ms, line] of steps) { await new Promise((r) => setTimeout(r, ms)); say(`> ${line}`); onLine(line); }
  });
} else if (!AUTO) {
  readline.createInterface({ input: process.stdin }).on("line", onLine);
}
