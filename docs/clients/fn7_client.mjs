#!/usr/bin/env node
// Final Night 7 reference client. Node >= 22 (built-in WebSocket), no dependencies.
//
//   node fn7_client.mjs --url ws://10.0.0.34:8080/ws --name Juno --create            # make a room, print its code
//   node fn7_client.mjs --url ws://10.0.0.34:8080/ws --name Kai --join ABC123        # join that room
//   node fn7_client.mjs ... --auto --beats 5                                          # play by itself (scripted lines)
//   node fn7_client.mjs ... --log frames.jsonl                                        # every frame, both directions, as JSONL
//
// Two input paths, kept separate (CLIENT_API.md, "The action"):
//  - Text entry (the God of Time interaction): "/slow" sends fields.slow (you started typing: time slows for
//    everyone); then type "l: ..." (left hand) / "r: ..." (right hand) lines and press Enter on an empty line: that
//    submits your orders (with fields.sent_at and the open window) and closes your window. Typing a line without
//    "/slow" first sends "slow" on its own the moment you start.
//  - Casts (god powers): "b: <target>" throws a lightning bolt now; "c: <power> <target>" casts a power from the
//    palette (round.brief.public.powers). A target is a ref (dg:2), "lon,lat", or a name. Casts never carry orders.
//  "z: region" / "z: character ch:12" -> zoom.  The host types "/start" in the lobby once everyone is ready.

import fs from "node:fs";
import readline from "node:readline";

const arg = (k, d) => { const i = process.argv.indexOf(`--${k}`); return i < 0 ? d : (process.argv[i + 1] ?? true); };
const flag = (k) => process.argv.includes(`--${k}`);
const URL_ = arg("url", "ws://10.0.0.34:8080/ws");
const NAME = arg("name", "player");
const JOIN = arg("join", null);
const CREATE = flag("create");
const AUTO = flag("auto");
const BEATS = Number(arg("beats", 10));
const BEAT_MS = Number(arg("beat-ms", 15000));
const LOG = arg("log", null);
const log = LOG ? fs.createWriteStream(LOG, { flags: "a" }) : null;

const ws = new WebSocket(URL_);
let nextId = 1;
const send = (t, d = {}, withId = true) => {
  const env = withId ? { t, id: `c${nextId++}`, d } : { t, d };
  log?.write(JSON.stringify({ dir: "out", at: Date.now(), ...env }) + "\n");
  ws.send(JSON.stringify(env));
};

const st = { me: null, seat: null, host: false, round: 0, view: null, arena: null, buf: { left: [], right: [], zoom: null },
  submitted: 0, deadline: 0, window: null, slowSent: false, powers: [], inCompose: false, pending: [] };
const say = (...a) => console.log(...a);
const r1 = (x) => (typeof x === "number" ? Math.round(x * 10) / 10 : x);

// One submit per beat; anything that arrives while Compose is closed waits for the next one.
function sendFields(fields) {
  if (!st.view?.alive) return;
  if (!st.inCompose || st.submitted === st.round) { st.pending.push(fields); return; }
  const merged = Object.assign({}, ...st.pending.splice(0), fields);
  send("action.submit", { round: st.round, text: "", fields: { ...merged, sent_at: Date.now() } });
  st.submitted = st.round;
}

// Enter: your orders (closes your window, or goes through with the initiator's if you typed during someone else's).
function submit() {
  const { left, right, zoom } = st.buf;
  if (!left.length && !right.length && !zoom) return;
  sendFields({ left: left.join("\n") || null, right: right.join("\n") || null, zoom, window: st.window });
  st.buf = { left: [], right: [], zoom: null };
  st.slowSent = false;
}

function slow() { if (!st.slowSent) { st.slowSent = true; sendFields({ slow: true }); } }

function target(s) {
  const m = s.match(/^\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*$/);
  return m ? [Number(m[1]), Number(m[2])] : s.trim();
}
function cast(power, where) { sendFields({ casts: [...(st.pending.flatMap((p) => p.casts ?? [])), { power, target: target(where) }] }); }

function autoCast() {
  const rivals = st.view.visible.filter((e) => e.kind === "demigod" && e.ref !== st.view.demigod);
  if (st.view.you.bolts >= 1 && rivals.length && st.round % 3 === 1) return { casts: [{ power: "lightning", target: rivals[0].ref }] };
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
      send("auth.login", { display_name: NAME, client: { kind: "fn7_client.mjs", version: "1" } });
      break;
    case "auth.ok":
      st.me = d.player_id;
      if (CREATE) send("room.create", { visibility: "private", settings: { beats_total: BEATS, beat_ms: BEAT_MS } });
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
        send("room.ready", { ready: true });
        if (AUTO && st.host) setTimeout(() => send("room.start", {}), 1500);
      }
      break;
    }
    case "match.started": say(`match started: ${d.seats.length} seats, rules ${JSON.stringify(d.rules)}`); break;
    case "round.started": say(`\n=== beat ${d.round}  arena ${r1(d.public.arena_radius_km)} km  alive ${d.public.alive}${d.private?.outside_arena ? "  YOU ARE OUTSIDE THE ARENA" : ""}`); break;
    case "round.brief":
      st.view = d.private; st.arena = d.public.arena; st.powers = d.public.powers ?? [];
      st.window = d.public.flow?.slow?.window ?? null;
      say(`you: health ${r1(st.view.you.health)}  bolts ${st.view.you.bolts}  debt ${r1(st.view.you.debt_open)}  zoom ${st.view.zoom}  entropy ${r1(d.public.entropy?.value)} (${d.public.entropy?.key})`);
      break;
    case "phase.changed":
      st.inCompose = d.phase === "compose";
      if (d.phase === "compose") {
        st.round = d.round; st.deadline = d.deadline ?? 0;
        if (AUTO) {
          // Text entry every third beat (it is a Time interaction and costs a bolt), a bolt now and then.
          const c = autoCast();
          if (st.round % 3 === 0) { Object.assign(st.buf, autoLines()); if (c) st.pending.push(c); submit(); }
          else if (c) sendFields(c);
        } else if (st.pending.length) sendFields({});
      }
      break;
    case "round.outcome": {
      // The narrator runs one beat late: previous_caption describes beat caption_beat (null on the first beat).
      const p = d.public, cap = p.previous_caption ?? p.caption;
      if (cap) say(`>> [beat ${p.caption_beat ?? "?"}] ${cap}`);
      (p.slow ?? []).forEach((x) => say(`   ⧗ ${x.text}${x.event === "closed" ? ` (${x.how}, ${x.after_ms} ms)` : ` (${x.beats_left} beats left, window ${x.window})`}`));
      (p.casts ?? []).forEach((c) => say(`   ⚡ ${c.by} casts ${c.power_name}${c.fizzled ? ` — fizzled: ${c.fizzled}` : ` on ${c.place} (${c.region})`}`));
      (p.events ?? []).filter((e) => e.kind !== "cast").slice(0, 6).forEach((e) => say(`   ◦ ${e.text} [${e.region ?? "?"}]`));
      if (p.entropy) say(`   entropy ${r1(p.entropy.value)}/${p.entropy.max} (${p.entropy.key})${p.entropy.heat_death ? "  HEAT DEATH" : ""}`);
      (p.omens ?? []).forEach((o) => say(`   ${o.god}: ${o.speech}`));
      if (p.eliminated?.length) say(`   fell: ${p.eliminated.join(", ")}`);
      break;
    }
    case "round.private":
      if (d.private.held) say("   (your orders are held in someone's slow window)");
      if (d.private.cut_off) say("   YOUR ORDERS WERE CUT OFF: the initiator pressed Enter first");
      d.private.hands.forEach((h, i) => {
        if (h.said) say(`   ${i ? "right" : "left"}: ${h.said}`);
        h.subagents.forEach((a) => say(`     ${a.kind} ${a.name} (${a.agent}${a.body ? `, body ${a.body}` : ""}): ${a.line}`));
      });
      // Your own invocations (even small ones) and the public rulings; news is newest first.
      d.private.omens.forEach((o) => say(`   ~ ${o.god}: ${o.speech}`));
      d.private.news.forEach((n) => say(`   · ${n.text} (${n.how}, ${n.row})`));
      break;
    case "round.private_state": st.view = d.state; break;
    case "seat.eliminated": if (d.seat === st.seat) say(`you fell: ${d.reason} (spectating)`); break;
    case "match.ended":
      if (d.summary.last_caption) say(`>> [beat ${d.summary.last_caption.beat}] ${d.summary.last_caption.caption}`);
      // winners: [] means a built-in AI demigod won (champion_seat null).
      say(`\nMATCH OVER. champion: ${d.summary.champions.map((c) => c.name).join(", ")}${d.summary.champion_seat == null ? " (built-in AI)" : ` (seat ${d.summary.champion_seat})`}\n${d.summary.final_cut?.story ?? ""}`);
      setTimeout(() => process.exit(0), 300);
      break;
    case "error": say(`! ${d.code}: ${d.message}`); break;
  }
};

if (!AUTO) {
  readline.createInterface({ input: process.stdin }).on("line", (line) => {
    const s = line.trim();
    if (s === "/start") return send("room.start", {});
    if (s === "/slow") return slow();
    if (s === "") return submit();
    const [, k, rest] = s.match(/^(l|left|r|right|z|zoom|b|bolt|c|cast)\s*:\s*(.*)$/i) ?? [null, "l", s];
    const key = k.toLowerCase()[0];
    if (key === "b") return cast("lightning", rest);
    if (key === "c") { const [power, ...where] = rest.split(/\s+/); return cast(power, where.join(" ")); }
    if (key === "z") { const [level, focus] = rest.split(/\s+/); st.buf.zoom = focus ? { level, focus } : level; }
    else { slow(); st.buf[key === "r" ? "right" : "left"].push(rest); }
  });
}
