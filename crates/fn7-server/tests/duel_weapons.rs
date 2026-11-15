//! Duel weapons against the real server (state schema 14; docs/DUEL_MOVES.md): the `fn7-server` binary on a private
//! port, mock images, a fake OpenAI-compatible gateway that answers only the duel picker. One human (Ash) and three
//! bots: Ash arms its demigod by will, summons itself next to a rival, challenges it and types: a line the picker
//! stages, a held weapon drawn, hammer-space rocket launchers (a success and a fumble), shots, a repeated line the
//! cache picks at its arrival. Then the duel's data rows and rollups from `/api/duel/lines`, and the recording replays
//! with 0 differences (the picks are fed back from `duel_picks`, no model).
//!
//! Prints what it saw (`cargo test -p fn7-server --test duel_weapons -- --nocapture`).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const ADMIN: &str = "duel-weapons-test";
const KEY: &str = "lines-key";

fn free_port() -> u16 {
    loop {
        let p = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        if p != 8080 {
            return p;
        }
    }
}

/// The fake gateway: the picker's requests get a pick by the line's words; every other mind gets a 400 (its rules
/// fallback). Counts the picker's requests.
async fn gateway(picks: Arc<AtomicU64>) -> u16 {
    use axum::routing::post;
    let port = free_port();
    let app = axum::Router::new().route("/chat/completions", post(move |axum::Json(body): axum::Json<Value>| {
        let picks = picks.clone();
        async move {
            let system = body["messages"][0]["content"].as_str().unwrap_or("");
            if !system.contains("You stage one blow") {
                return (axum::http::StatusCode::BAD_REQUEST, axum::Json(json!({ "error": { "message": "not this mind" } })));
            }
            picks.fetch_add(1, Ordering::Relaxed);
            let user = body["messages"][1]["content"].as_str().unwrap_or("").to_lowercase();
            let line = user.split("<<<begin untrusted duel line").nth(1).unwrap_or("");
            let pick = if line.contains("hit") {
                json!({ "move_class": "strike", "clips": ["roundhouse_kick"], "path": null, "object_id": null, "target_part": "head" })
            } else if line.contains("dance") {
                json!({ "move_class": "taunt", "clips": ["taunt"], "path": null, "object_id": null, "target_part": "chest" })
            } else if user.contains("weapon class launcher") {
                json!({ "move_class": "strike", "clips": ["launcher_fire"], "path": null, "object_id": null, "target_part": "chest" })
            } else if user.contains("weapon class bow") {
                json!({ "move_class": "strike", "clips": ["bow_quick_shot"], "path": null, "object_id": null, "target_part": "chest" })
            } else {
                // A clip of a weapon it may not hold: refused unless that weapon is in hand.
                json!({ "move_class": "strike", "clips": ["gun_side_shot"], "path": null, "object_id": null, "target_part": "belly" })
            };
            (axum::http::StatusCode::OK, axum::Json(json!({ "choices": [{ "index": 0, "message": { "role": "assistant", "content": pick.to_string() } }],
                                                              "usage": { "prompt_tokens": 1800, "completion_tokens": 40 } })))
        }
    }));
    let l = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    port
}

struct Server {
    child: Child,
}

impl Server {
    async fn start(dir: &Path, port: u16, gateway: u16) -> Server {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("server.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_fn7-server"))
            .current_dir(dir)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", dir)
            .env("THEATRE_BIND", format!("127.0.0.1:{port}"))
            .env("THEATRE_DATA_DIR", dir.join("data"))
            .env("THEATRE_PERSIST", "true")
            .env("THEATRE_RECORD", "full")
            .env("THEATRE_ADMIN_TOKEN", ADMIN)
            .env("THEATRE_COUNTDOWN_SECS", "0.2")
            .env("LLM_BASE_URL", format!("http://127.0.0.1:{gateway}"))
            .env("LLM_DEFAULT_MODEL", "fake")
            .env("LLM_RETRIES", "0")
            .env("FN7_IMAGES", "mock")
            .env("FN7_MEDIA_DIR", dir.join("media"))
            .env("FN7_MEDIA_PREWARM", "0")
            .env("FN7_DUEL_LINES_KEY", KEY)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .expect("fn7-server starts");
        let s = Server { child };
        let until = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(r) = reqwest::get(format!("http://127.0.0.1:{port}/healthz")).await {
                if r.status().is_success() {
                    return s;
                }
            }
            assert!(Instant::now() < until, "the server did not come up: {}", std::fs::read_to_string(dir.join("server.log")).unwrap_or_default());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Client {
    ws: Ws,
    inbox: Vec<Value>,
    n: u64,
    /// Every public duel event seen (each `also[]` entry flattened), in order.
    events: Vec<Value>,
    round: u64,
}

impl Client {
    async fn connect(port: u16) -> Client {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws")).await.expect("ws connects");
        let mut c = Client { ws, inbox: vec![], n: 0, events: vec![], round: 0 };
        c.wait("server.hello", |_| true, 30).await.unwrap();
        c
    }

    fn note(&mut self, m: &Value) {
        if m["t"] == "round.started" {
            self.round = self.round.max(m["d"]["round"].as_u64().unwrap_or(0));
        }
        if m["t"] == "phase.event" {
            let p = &m["d"]["public"];
            self.events.push(p.clone());
            self.events.extend(p["also"].as_array().cloned().unwrap_or_default());
        }
        for d in m["d"]["public"]["duels_open"].as_array().into_iter().flatten().chain(m["d"]["public"]["duels"].as_array().into_iter().flatten()) {
            let mut d = d.clone();
            d["event"] = "outcome.duel".into();
            self.events.push(d);
        }
    }

    async fn read(&mut self, until: Instant) -> Option<Value> {
        loop {
            let left = until.checked_duration_since(Instant::now())?;
            match tokio::time::timeout(left, self.ws.next()).await {
                Ok(Some(Ok(Message::Text(t)))) => {
                    let m: Value = serde_json::from_str(&t).ok()?;
                    self.note(&m);
                    return Some(m);
                }
                Ok(Some(Ok(_))) => continue,
                _ => return None,
            }
        }
    }

    async fn wait(&mut self, t: &str, pred: impl Fn(&Value) -> bool, secs: u64) -> Option<Value> {
        let until = Instant::now() + Duration::from_secs(secs);
        let mut from = 0;
        loop {
            if let Some(i) = (from..self.inbox.len()).find(|i| self.inbox[*i]["t"] == t && pred(&self.inbox[*i]["d"])) {
                return Some(self.inbox.remove(i)["d"].take());
            }
            from = self.inbox.len();
            let m = self.read(until).await?;
            self.inbox.push(m);
        }
    }

    /// Read whatever arrives for `ms`.
    async fn drain(&mut self, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        while let Some(m) = self.read(until).await {
            self.inbox.push(m);
        }
    }

    async fn ask(&mut self, t: &str, d: Value) -> (String, Value) {
        self.n += 1;
        let id = format!("ash-{}", self.n);
        self.ws.send(Message::Text(json!({ "t": t, "id": id, "d": d }).to_string().into())).await.expect("send");
        let until = Instant::now() + Duration::from_secs(40);
        let mut from = 0;
        loop {
            if let Some(i) = (from..self.inbox.len()).find(|i| self.inbox[*i]["re"] == id.as_str()) {
                let mut m = self.inbox.remove(i);
                return (m["t"].as_str().unwrap_or("").to_string(), m["d"].take());
            }
            from = self.inbox.len();
            let m = self.read(until).await.unwrap_or_else(|| panic!("no reply to {t}"));
            self.inbox.push(m);
        }
    }

    /// An instant action; its public event (None: refused or nothing to show).
    async fn instant(&mut self, fields: Value) -> Option<Value> {
        let before = self.events.len();
        let (_, d) = self.ask("action.submit", json!({ "round": self.round, "text": "", "instant": true, "fields": fields })).await;
        if d["accepted"] != true {
            eprintln!("  refused: {d}");
            return None;
        }
        self.drain(250).await;
        self.events.get(before).cloned()
    }

    /// The newest panel seen of a duel.
    fn last_panel(&self, id: &str) -> Option<Value> {
        self.events.iter().rev().find(|e| e["event"] == "duel.panel" && e["duel_id"] == id).cloned()
    }
}

fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fn7-duel-weapons-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test(flavor = "multi_thread")]
async fn a_private_server_duel_with_picks_draws_and_projectiles_replays_clean() {
    let dir = temp_dir();
    let picks = Arc::new(AtomicU64::new(0));
    let gw = gateway(picks.clone()).await;
    let port = free_port();
    let server = Server::start(&dir, port, gw).await;
    let mut a = Client::connect(port).await;
    let (t, _) = a.ask("auth.login", json!({ "display_name": "Ash", "client": { "kind": "test", "version": "1" } })).await;
    assert_eq!(t, "auth.ok");
    let settings = json!({ "seed": 606, "beats_total": 200, "llm": true, "flow_ms": 3000, "duel_ai_chance": 0.0, "duel_ai_lines_per_window": 0.0,
                           "dm_encounters_per_beat": 0, "bolt_start": 6, "arena_close_beats": 1000, "duel_deadline_ms": 26_000,
                           "duel_damage_scale": 0.05, "duel_draws": 8, "slow_ms": 3000 });
    let (t, room) = a.ask("room.create", json!({ "visibility": "private", "settings": settings })).await;
    assert_eq!(t, "room.snapshot", "{room}");
    a.ask("room.bot_add", json!({ "count": 3 })).await;
    a.ask("room.ready", json!({ "ready": true })).await;
    a.ask("room.start", json!({})).await;
    let started = a.wait("match.started", |_| true, 30).await.unwrap();
    let match_id = started["match_id"].as_str().unwrap().to_string();

    // ---- arm by will for a few beats (to have a held weapon to draw) ---------------------------------------------------
    let mut held: Vec<Value> = vec![];
    for round in 1..=8u64 {
        a.wait("phase.changed", |d| d["round"] == round && d["phase"] == "compose", 60).await.unwrap();
        if held.is_empty() {
            let text = "arm my demigods with butterfly wing blades";
            a.ask("action.submit", json!({ "round": round, "text": text, "fields": { "left": text } })).await;
        }
        a.wait("round.committed", |d| d["round"] == round, 60).await.unwrap();
        let me = a.wait("round.private_state", |d| d["round"] == round, 10).await;
        held = me.and_then(|m| m["state"]["my_items"].as_array().cloned()).unwrap_or_default().into_iter().filter(|i| i["weapon_class"].is_string()).collect();
        if !held.is_empty() && round >= 3 {
            break;
        }
    }
    eprintln!("held weapons: {}", held.iter().map(|i| format!("{} ({})", i["name"], i["weapon_class"])).collect::<Vec<_>>().join(", "));

    // ---- summon next to a rival, challenge -----------------------------------------------------------------------------
    let c = a.wait("round.committed", |_| true, 0).await;
    let snap = match c { Some(c) => c, None => { a.drain(3500).await; a.wait("round.committed", |_| true, 30).await.unwrap() } };
    let demigods = snap["public_state"]["demigods"].as_array().unwrap().clone();
    let mine = demigods.iter().find(|d| d["seat"] == 0 && d["alive"] == true).unwrap()["ref"].as_str().unwrap().to_string();
    let rival = demigods.iter().find(|d| d["seat"] == 1 && d["alive"] == true).unwrap().clone();
    let near = json!([rival["pos"][0].as_f64().unwrap() + 0.004, rival["pos"][1]]);
    a.drain(300).await;
    a.instant(json!({ "summon": { "demigod": mine, "target": near } })).await.expect("summoning");
    let lock = a.instant(json!({ "challenge": { "demigod": mine, "target": rival["ref"], "level": "people" } })).await.expect("lock");
    assert_eq!(lock["event"], "challenge.lock", "{lock}");
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let scene = a.events.iter().find(|e| e["event"] == "duel.scene" && e["duel_id"] == id.as_str()).cloned().unwrap();
    assert!(scene["clip_fallbacks"].is_object() && scene["list_version"].is_string());

    // ---- the lines --------------------------------------------------------------------------------------------------------
    let mut blows: Vec<Value> = vec![];
    let mut lines: Vec<&str> = vec!["I hit him hard"];
    if !held.is_empty() {
        lines.push("I draw my blade and slash");
    }
    lines.extend(["dance at him", "i pull out a rocket launcher and fire", "i pull out a rocket launcher and fire", "shoot him",
                  "i pull out a rocket launcher and fire", "i pull out a longbow and shoot", "shoot him", "i pull out a rocket launcher and fire",
                  "shoot him", "dance at him", "I hit him hard"]);
    for text in lines {
        if let Some(b) = a.instant(json!({ "duel": { "duel_id": id, "text": text } })).await {
            if b["event"] == "duel.blow" {
                eprintln!("  {:>2} {:<40} class {:<7} clip {:<16} weapon {:<10} draw {} picker {}", b["n"], text, b["choreo"]["a"]["move_class"].as_str().unwrap_or("-"),
                          b["move"]["clip"].as_str().unwrap_or("-"), b["move"]["weapon_class"].as_str().unwrap_or("-"),
                          b["move"]["draw"]["verdict"].as_str().unwrap_or("-"), b["move"]["picker"].as_str().unwrap_or("-"));
                blows.push(b.clone());
                if b["ko"] == true {
                    break;
                }
            }
        }
        // The tick at the panel's input close (the client's job), then a breath.
        if let Some(close) = a.last_panel(&id).and_then(|p| p["input_closes_ms"].as_i64()) {
            let wait = close + 40 - theatre_trace::now_ms();
            if wait > 0 && wait < 7000 {
                tokio::time::sleep(Duration::from_millis(wait as u64)).await;
            }
            a.instant(json!({ "duel_tick": { "duel_id": id } })).await;
        } else {
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
    }
    // Let the duel run out (its deadline) and the resolve end it.
    let end = loop {
        if let Some(e) = a.events.iter().find(|e| e["event"] == "outcome.duel" && e["duel_id"] == id.as_str() && e["deed"].is_string()).cloned() {
            break e;
        }
        if let Some(close) = a.last_panel(&id).and_then(|p| p["input_closes_ms"].as_i64()) {
            let wait = (close + 40 - theatre_trace::now_ms()).clamp(0, 7000);
            tokio::time::sleep(Duration::from_millis(wait as u64)).await;
            a.instant(json!({ "duel_tick": { "duel_id": id } })).await;
        } else {
            a.drain(1000).await;
        }
    };
    eprintln!("the duel ended: {} ({} blows)", end["reason"], end["blows"]);
    let panels: Vec<Value> = a.events.iter().filter(|e| e["event"] == "duel.panel" && e["duel_id"] == id.as_str()).cloned().collect();

    // ---- what it showed ---------------------------------------------------------------------------------------------------
    let segs = |side: &str| -> Vec<Value> { panels.iter().flat_map(|p| p[side]["segs"].as_array().cloned().unwrap_or_default()).collect() };
    let picked = blows.iter().find(|b| b["text"] == "I hit him hard").unwrap();
    assert!(segs("a").iter().any(|s| s["clip"] == "roundhouse_kick"), "the picked clip was staged");
    assert!(picks.load(Ordering::Relaxed) >= 2, "the picker asked the gateway");
    let pre = blows.iter().find(|b| b["move"]["picker"] == "pre").expect("a repeated line (same words, range, weapon) is picked at its arrival");
    eprintln!("picked at arrival (cache): {:?} -> class {}, clip {}", pre["text"], pre["choreo"]["a"]["move_class"], pre["move"]["clip"]);
    let draws: Vec<&str> = blows.iter().filter_map(|b| b["move"]["draw"]["verdict"].as_str()).collect();
    assert!(draws.contains(&"success") && draws.contains(&"fumble"), "{draws:?}");
    if !held.is_empty() {
        assert!(draws.contains(&"drawn"), "the held weapon was drawn: {draws:?}");
    }
    let projectiles: Vec<Value> = panels.iter().flat_map(|p| p["projectiles"].as_array().cloned().unwrap_or_default()).collect();
    let rocket = projectiles.iter().find(|p| p["kind"] == "rocket").expect("a rocket flew");
    assert_eq!(rocket["on_hit"]["aoe_hexes"].as_array().map(Vec::len), Some(7));
    assert!(segs("a").iter().any(|s| s["prop"]["hammer"] == true), "a hammer-space prop in hand");
    assert!(panels.iter().any(|p| p["sim"].is_object()), "live panels carry the duel sim");
    eprintln!("picked line: {} -> clip {} (picker {}); draws {draws:?}; projectiles {:?}", picked["text"], picked["move"]["clip"],
              picked["move"]["picker"], projectiles.iter().map(|p| p["kind"].as_str().unwrap_or("?")).collect::<Vec<_>>());
    eprintln!("a rocket: {}", rocket);
    if let Some(arrow) = projectiles.iter().find(|p| p["kind"] == "arrow") {
        eprintln!("an arrow: {arrow}");
    }

    // ---- the data rows and the rollups -----------------------------------------------------------------------------------
    let get = |by: &str| reqwest::Client::new().get(format!("http://127.0.0.1:{port}/api/duel/lines?by={by}&key={KEY}")).send();
    let lines = get("line").await.unwrap().json::<Value>().await.unwrap();
    assert!(lines["lines"].as_i64().unwrap() >= 8, "{lines}");
    let dis = get("disagreement").await.unwrap().json::<Value>().await.unwrap();
    let unm = get("unmatched").await.unwrap().json::<Value>().await.unwrap();
    let clip = get("clip").await.unwrap().json::<Value>().await.unwrap();
    assert!(unm["rows"].as_array().unwrap().iter().any(|r| r["line"] == "dance at him"), "{unm}");
    assert_eq!(reqwest::get(format!("http://127.0.0.1:{port}/api/duel/lines?by=line")).await.unwrap().status(), 403, "key-gated");
    eprintln!("rollup by=line: {}", lines);
    eprintln!("rollup by=disagreement: {}", dis);
    eprintln!("rollup by=unmatched: {}", unm);
    eprintln!("rollup by=clip: {}", clip);
    let db = rusqlite::Connection::open(dir.join("data/fn7/duel_lines.sqlite")).unwrap();
    let row: String = db.query_row("SELECT row FROM duel_lines WHERE line = 'I hit him hard' ORDER BY n LIMIT 1", [], |r| r.get(0)).unwrap();
    eprintln!("a data row: {row}");

    // ---- the recording replays with 0 differences (the picks fed back, no model) ------------------------------------------
    drop(a);
    let pid = server.child.id();
    Command::new("kill").arg("-INT").arg(pid.to_string()).status().unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    while reqwest::get(format!("http://127.0.0.1:{port}/healthz")).await.is_ok() && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    drop(server);
    let rec = dir.join("data").join("recordings").join(format!("{match_id}.jsonl"));
    let records = fn7_server::replay::read_records(&rec).unwrap();
    let handed: usize = records.iter().filter(|r| r["type"] == "event" && r["kind"] == "duel_picks").map(|r| r["data"]["picks"].as_array().map(Vec::len).unwrap_or(0)).sum();
    assert!(handed >= 2, "the resolves recorded the picks they were handed");
    let rep = fn7_server::replay::verify(&rec, |l| eprintln!("{l}")).await.unwrap();
    eprintln!("replay: {} snapshots, {} identical, {} differences; {} picks recorded", rep.snapshots, rep.identical, rep.differences.len(), handed);
    assert!(rep.differences.is_empty(), "{:?}", rep.differences);
    let _ = std::fs::remove_dir_all(&dir);
}
