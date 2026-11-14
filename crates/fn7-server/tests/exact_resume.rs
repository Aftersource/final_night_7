//! Exact resume against the real server (docs/TIME_TRAVEL.md): the `fn7-server` binary on a private port with its own
//! data directory (`FN7_IMAGES=mock`, no LLM, `THEATRE_PERSIST=true`, `THEATRE_RECORD=full`), driven over WebSockets.
//!
//! One match, in order: a summoning, a challenge and duel lines; (a) the client drops its socket and `auth.resume`s
//! mid-duel with a slow window open, and a viewer joins mid-duel; the duel crosses beats; a second human joins
//! mid-match; (b) the server is killed (`SIGKILL`) with lines typed since the last commit, restarted, and the duel is
//! still open with the same blows, the lost lines delivered to the first resolve; (d) the admin's state at a round and
//! a fork at a beat where the duel is open; (c) both recordings replay with 0 differences and hold every instant
//! action with its arrival stamp.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const ADMIN: &str = "exact-resume-test";

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

/// A free local port that is not the live server's.
fn free_port() -> u16 {
    loop {
        let p = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        if p != 8080 {
            return p;
        }
    }
}

struct Server {
    child: Child,
    #[allow(dead_code)]
    port: u16,
}

impl Server {
    /// Start the server in `dir` (its working directory too, so no project `.env` is read) with a clean environment.
    async fn start(dir: &Path, port: u16) -> Server {
        Self::start_with(dir, port, &[]).await
    }

    async fn start_with(dir: &Path, port: u16, extra: &[(&str, &str)]) -> Server {
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
            .env("LLM_CACHE_DIR", dir.join("llm_cache"))
            .env("FN7_IMAGES", "mock")
            .env("FN7_MEDIA_DIR", dir.join("media"))
            .env("FN7_MEDIA_PREWARM", "0")
            .envs(extra.iter().copied())
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .expect("fn7-server starts");
        let s = Server { child, port };
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

    /// `kill -9`.
    fn kill9(mut self) {
        self.child.kill().unwrap();
        let _ = self.child.wait();
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
    name: &'static str,
}

impl Client {
    async fn connect(port: u16, name: &'static str) -> Client {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws")).await.expect("ws connects");
        let mut c = Client { ws, inbox: vec![], n: 0, name };
        c.wait("server.hello", |_| true).await;
        c
    }

    async fn read(&mut self, until: Instant) -> Option<Value> {
        loop {
            let left = until.checked_duration_since(Instant::now())?;
            match tokio::time::timeout(left, self.ws.next()).await {
                Ok(Some(Ok(Message::Text(t)))) => return serde_json::from_str(&t).ok(),
                Ok(Some(Ok(_))) => continue,
                _ => return None,
            }
        }
    }

    /// The next message of type `t` passing `pred` (earlier ones stay in the inbox for later waits).
    async fn wait(&mut self, t: &str, pred: impl Fn(&Value) -> bool) -> Value {
        self.wait_for(t, pred, 40).await.unwrap_or_else(|| {
            let seen: Vec<String> = self.inbox.iter().rev().take(25).map(|m| m["t"].as_str().unwrap_or("?").to_string()).collect();
            panic!("{}: no `{t}` (last messages, newest first: {seen:?})", self.name)
        })
    }

    async fn wait_for(&mut self, t: &str, pred: impl Fn(&Value) -> bool, secs: u64) -> Option<Value> {
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

    async fn send(&mut self, t: &str, d: Value) -> String {
        self.n += 1;
        let id = format!("{}-{}", self.name, self.n);
        self.ws.send(Message::Text(json!({ "t": t, "id": id, "d": d }).to_string().into())).await.expect("send");
        id
    }

    /// Send and wait for the reply (`re` = the request's id): `(type, payload)`.
    async fn ask(&mut self, t: &str, d: Value) -> (String, Value) {
        let id = self.send(t, d).await;
        let until = Instant::now() + Duration::from_secs(40);
        let mut from = 0;
        loop {
            if let Some(i) = (from..self.inbox.len()).find(|i| self.inbox[*i]["re"] == id.as_str()) {
                let mut m = self.inbox.remove(i);
                return (m["t"].as_str().unwrap_or("").to_string(), m["d"].take());
            }
            from = self.inbox.len();
            let m = self.read(until).await.unwrap_or_else(|| panic!("{}: no reply to {t}", self.name));
            self.inbox.push(m);
        }
    }

    /// An instant action: the `action.received` reply, then (when accepted) the public `phase.event` it made.
    async fn instant(&mut self, round: u64, fields: Value, event: &str) -> Value {
        let (t, d) = self.ask("action.submit", json!({ "round": round, "text": "", "instant": true, "fields": fields })).await;
        assert_eq!((t.as_str(), d["accepted"].as_bool()), ("action.received", Some(true)), "{}: {fields} refused: {d}", self.name);
        self.wait("phase.event", |d| d["public"]["event"] == event).await["public"].take()
    }

    async fn login(&mut self) -> Value {
        let (t, d) = self.ask("auth.login", json!({ "display_name": self.name, "client": { "kind": "test", "version": "1" } })).await;
        assert_eq!(t, "auth.ok", "{d}");
        d
    }

    /// `auth.resume`: `auth.ok`, then `room.snapshot` and `match.snapshot`.
    async fn resume(port: u16, name: &'static str, token: &str) -> (Client, Value) {
        let mut c = Client::connect(port, name).await;
        let (t, d) = c.ask("auth.resume", json!({ "resume_token": token })).await;
        assert_eq!(t, "auth.ok", "resume refused: {d}");
        c.wait("room.snapshot", |_| true).await;
        let snap = c.wait("match.snapshot", |_| true).await;
        (c, snap)
    }
}

fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fn7-exact-resume-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn duel<'a>(present: &'a Value, id: &str) -> &'a Value {
    present["duels"].as_array().and_then(|a| a.iter().find(|d| d["duel_id"] == id)).unwrap_or_else(|| panic!("no duel {id} in {present}"))
}

/// The blows of a duel as a client redraws them: number, die, damage, choreography and the server's stamps.
fn timeline(d: &Value) -> Vec<Value> {
    d["blows"].as_array().unwrap().iter().map(|b| json!([b["n"], b["text"], b["roll"], b["verdict"], b["damage"], b["hp"], b["choreo"], b["at_ms"], b["seq"]])).collect()
}

async fn admin(port: u16, path: &str) -> Value {
    reqwest::Client::new().get(format!("http://127.0.0.1:{port}{path}")).bearer_auth(ADMIN).send().await.unwrap().json().await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_exact_present_survives_reconnects_a_kill_and_a_fork() {
    let dir = temp_dir();
    let port = free_port();
    let server = Server::start(&dir, port).await;

    // ---- a private room: one human, three bots ---------------------------------------------------------------------
    let mut a = Client::connect(port, "Ash").await;
    let me = a.login().await;
    let token = me["resume_token"].as_str().unwrap().to_string();
    let settings = json!({ "seed": 4242, "beats_total": 400, "llm": false, "flow_ms": 2500, "duel_ai_chance": 0.0,
                           "dm_encounters_per_beat": 0, "duel_deadline_ms": 240_000, "duel_damage_scale": 0.05,
                           "slow_ms": 20_000, "allow_midmatch_join": true });
    let (t, room) = a.ask("room.create", json!({ "visibility": "private", "settings": settings })).await;
    assert_eq!(t, "room.snapshot", "{room}");
    let code = room["code"].as_str().unwrap().to_string();
    a.ask("room.bot_add", json!({ "count": 3 })).await;
    a.ask("room.ready", json!({ "ready": true })).await;
    a.ask("room.start", json!({})).await;
    let started = a.wait("match.started", |_| true).await;
    let match_id = started["match_id"].as_str().unwrap().to_string();

    // Beat 1 commits: where everyone stands.
    let c1 = a.wait("round.committed", |d| d["round"] == 1).await;
    let demigods = c1["public_state"]["demigods"].as_array().unwrap().clone();
    let mine = demigods.iter().find(|d| d["seat"] == 0).unwrap()["ref"].as_str().unwrap().to_string();
    let rival = demigods.iter().find(|d| d["seat"] == 1).unwrap().clone();
    let rival_ref = rival["ref"].as_str().unwrap().to_string();
    let near = json!([rival["pos"][0].as_f64().unwrap() + 0.004, rival["pos"][1]]);

    // ---- beat 2: summon next to the rival, challenge it, a first line, and open a slow window -----------------------
    a.wait("phase.changed", |d| d["round"] == 2 && d["phase"] == "compose").await;
    let summoning = a.instant(2, json!({ "summon": { "demigod": mine, "target": near } }), "summoning").await;
    let lock = a.instant(2, json!({ "challenge": { "demigod": mine, "target": rival_ref, "level": "people" } }), "challenge.lock").await;
    let id = lock["duel_id"].as_str().unwrap().to_string();
    let b1 = a.instant(2, json!({ "duel": { "duel_id": id, "text": "I kick your knee into the mud" } }), "duel.blow").await;
    a.ask("action.begin", json!({})).await;
    let window = a.wait("window.opened", |_| true).await;

    // ---- (a) the client drops its socket and resumes mid-duel --------------------------------------------------------
    drop(a);
    let asked = now_ms();
    let (mut a, snap) = Client::resume(port, "Ash", &token).await;
    assert_eq!((snap["match_id"].as_str(), snap["round"].as_u64()), (Some(match_id.as_str()), Some(2)));
    assert!(snap["phase"].is_string(), "the phase: {}", snap["phase"]);
    if snap["phase"] == "compose" {
        assert!(snap["deadline"].as_i64().unwrap() > asked - 5_000, "the phase's deadline: {}", snap["deadline"]);
    }
    assert_eq!(snap["window"]["window_id"], window["window_id"], "the open slow window: {}", snap["window"]);
    assert!(snap["window"]["deadline"].as_i64().unwrap() > asked);
    assert_eq!(snap["you"]["seat"], 0);
    let ps = &snap["public_state"];
    assert_eq!(ps["demigods"].as_array().map(Vec::len), Some(demigods.len()), "every demigod");
    let p = &ps["present"];
    let server_now = p["server_time_ms"].as_i64().unwrap();
    assert!((server_now - asked).abs() < 5_000, "the server's clock, to seek to the exact offset: {server_now} vs {asked}");
    assert_eq!(p["live"], true);
    let events: Vec<&str> = p["events"].as_array().unwrap().iter().map(|e| e["event"].as_str().unwrap_or("")).collect();
    assert_eq!(&events[..3], ["summoning", "challenge.lock", "duel.blow"], "the mid-beat events since the last resolve, in order");
    for (k, e) in [&summoning, &lock, &b1].into_iter().enumerate() {
        assert_eq!((p["events"][k]["seq"].clone(), p["events"][k]["received_at_ms"].clone()), (e["seq"].clone(), e["received_at_ms"].clone()));
    }
    assert!(p["events"][1]["a"]["portrait_image"].is_object(), "the lock's portraits come with it (mock images)");
    let d = duel(p, &id);
    assert_eq!((d["a"]["demigod"].as_str(), d["b"]["demigod"].as_str()), (Some(mine.as_str()), Some(rival_ref.as_str())), "both fighters");
    assert_eq!(d["hp"], b1.get("hp_after").unwrap_or(&b1["hp"]).clone(), "hp now");
    for k in ["range", "initiative", "stances", "started_ms", "deadline_ms", "ends_beat", "clock"] {
        assert!(!d[k].is_null(), "{k}: {d}");
    }
    assert_eq!(d["deadline_ms"], lock["deadline_ms"]);
    assert_eq!(d["started_ms"], lock["received_at_ms"], "the lock's server stamp");
    let blows = d["blows"].as_array().unwrap();
    assert_eq!(blows.len(), 1);
    for k in ["n", "text", "roll", "total", "verdict", "damage", "hp", "choreo", "anim", "at_ms", "seq", "counter"] {
        assert_eq!(blows[0][k], b1[k], "{k}: the blow as it was shown live, with its choreography and stamps");
    }
    let elapsed = d["clock"]["elapsed_ms"].as_i64().unwrap();
    assert!((elapsed - (server_now - lock["received_at_ms"].as_i64().unwrap())).abs() <= 1, "the offset to seek to: {}", d["clock"]);
    // Close the slow window (its Enter), so the beats' text entry is out of the way.
    let (_, r) = a.ask("action.submit", json!({ "round": 2, "text": "hold the line", "window_id": window["window_id"] })).await;
    assert_eq!(r["accepted"], true, "{r}");

    // ---- (a) a viewer joins mid-duel and gets the same --------------------------------------------------------------
    let mut v = Client::connect(port, "Vera").await;
    v.login().await;
    let (t, _) = v.ask("room.join", json!({ "code": code, "viewer": true })).await;
    assert_eq!(t, "room.snapshot");
    let vs = v.wait("match.snapshot", |_| true).await;
    assert!(vs["you"].is_null());
    assert_eq!(timeline(duel(&vs["public_state"]["present"], &id)), timeline(d), "a viewer joining mid-duel gets the same duel");
    let (t, q) = v.ask("game.present", json!({})).await;
    assert_eq!(t, "game.present");
    assert_eq!(timeline(duel(&q, &id)), timeline(d));
    let http: Value = reqwest::get(format!("http://127.0.0.1:{port}/api/rooms/{code}/query/present")).await.unwrap().json().await.unwrap();
    assert_eq!(timeline(duel(&http, &id)), timeline(d), "and over HTTP");
    drop(v);

    // ---- the duel crosses a beat; a second human joins mid-match ----------------------------------------------------
    let out2 = a.wait("round.outcome", |d| d["round"] == 2).await;
    assert_eq!(out2["public"]["duels_open"][0]["duel_id"], id.as_str(), "still open after beat 2: {}", out2["public"]["duels_open"]);
    let mut j = Client::connect(port, "Joan").await;
    j.login().await;
    let (t, _) = j.ask("room.join", json!({ "code": code })).await;
    assert_eq!(t, "room.snapshot");
    a.wait("phase.changed", |d| d["round"] == 3 && d["phase"] == "compose").await;
    let joined = a.wait("seat.joined", |_| true).await;
    assert_eq!(joined["display_name"], "Joan");
    let b2 = a.instant(3, json!({ "duel": { "duel_id": id, "text": "then I smash your jaw with my elbow" } }), "duel.blow").await;
    let (_, q) = a.ask("game.present", json!({})).await;
    let ns: Vec<u64> = duel(&q, &id)["blows"].as_array().unwrap().iter().map(|b| b["n"].as_u64().unwrap()).collect();
    assert_eq!(ns, [b1["n"].as_u64().unwrap(), b2["n"].as_u64().unwrap()], "one timeline: the state's blow, then the journal's");
    assert_eq!(q["events"].as_array().unwrap().iter().filter(|e| e["event"] == "duel.blow").count(), 1, "only the blow since the last resolve");

    // ---- (b) kill -9 mid-beat with a line typed since the last commit ------------------------------------------------
    let c3 = a.wait("round.committed", |d| d["round"] == 3).await;
    assert_eq!(c3["public_state"]["present"]["duels"][0]["blows"].as_array().map(Vec::len), Some(2));
    a.wait("phase.changed", |d| d["round"] == 4 && d["phase"] == "compose").await;
    let b3 = a.instant(4, json!({ "duel": { "duel_id": id, "text": "I sweep your ankle by the river" } }), "duel.blow").await;
    // (Text typed into the beat's Compose, to measure what the core keeps of it.)
    let (_, r) = a.ask("action.submit", json!({ "round": 4, "text": "scout the hills", "fields": { "left": "scout the hills" } })).await;
    assert_eq!(r["accepted"], true, "{r}");
    let (_, before) = a.ask("game.present", json!({})).await;
    let before = duel(&before, &id).clone();
    assert_eq!(timeline(&before).len(), 3);
    let killed_at = now_ms();
    server.kill9();
    drop(a);
    drop(j);
    tokio::time::sleep(Duration::from_millis(2_600)).await;
    let server = Server::start(&dir, port).await;
    let back_at = now_ms();
    let (mut a, snap) = Client::resume(port, "Ash", &token).await;
    assert_eq!(snap["match_id"], match_id.as_str(), "the match is restored");
    let committed = snap["round"].as_u64().unwrap();
    assert!((3..=4).contains(&committed), "it resumes after the last commit on disk: {committed}");
    let pr = &snap["public_state"]["present"];
    let after = duel(pr, &id);
    assert_eq!(timeline(after), timeline(&before), "the duel is still open with the same blows after the kill");
    assert_eq!(after["hp"], before["hp"]);
    for k in ["range", "initiative", "stances", "started_ms", "ends_beat", "n", "typed"] {
        assert_eq!(after[k], before[k], "{k}");
    }
    let compose_kept = snap["you"]["action"].clone();
    let paused_snapshot = snap["paused"] == true;
    if paused_snapshot {
        assert_eq!(pr["paused_since_ms"], b3["received_at_ms"], "the clocks stand still at the last sign of life: {pr}");
        assert_eq!(after["clock"]["running"], false);
        assert_eq!(after["clock"], before["clock"].as_object().map(|_| json!({ "at_ms": b3["received_at_ms"],
            "elapsed_ms": b3["received_at_ms"].as_i64().unwrap() - lock["received_at_ms"].as_i64().unwrap(),
            "remaining_ms": before["deadline_ms"].as_i64().unwrap() - b3["received_at_ms"].as_i64().unwrap(), "running": false })).unwrap());
    }
    // The first Start after the restart: the downtime is a pause, the duel's clock stood still for it.
    let rs = a.wait("round.started", |d| !d["public"]["duel_clock"].is_null()).await;
    let clock = &rs["public"]["duel_clock"];
    let paused = clock["pause"]["ms"].as_i64().unwrap();
    assert!(paused >= 2_000 && paused <= back_at - lock["received_at_ms"].as_i64().unwrap() + 5_000, "the pause: {clock}");
    assert_eq!(clock["duels"][0]["deadline_ms"].as_i64(), Some(before["deadline_ms"].as_i64().unwrap() + paused), "the deadline moved by the pause");
    let round = rs["round"].as_u64().unwrap();
    // ...and its resolve is handed the line the dead process never delivered.
    let out = a.wait("round.outcome", |d| d["round"] == round).await;
    let open = &out["public"]["duels_open"][0];
    assert_eq!((open["duel_id"].as_str(), open["typed"].as_u64()), (Some(id.as_str()), Some(3)), "no accepted line was lost: {open}");
    let log = open["log"].as_array().unwrap();
    let redone = log.iter().find(|b| b["seq"] == b3["seq"] && b["at_ms"] == b3["at_ms"]).expect("the recovered blow is resolved");
    for k in ["n", "roll", "total", "verdict", "damage", "choreo"] {
        assert_eq!(redone[k], b3[k], "{k}: resolved as it was shown before the kill");
    }
    // The duel goes on.
    a.wait("phase.changed", |d| d["round"].as_u64() == Some(round + 1) && d["phase"] == "compose").await;
    let b4 = a.instant(round + 1, json!({ "duel": { "duel_id": id, "text": "I hurl you into the fountain" } }), "duel.blow").await;
    assert_eq!(b4["n"].as_u64(), Some(before["n"].as_u64().unwrap() + 1), "the numbering carries on");
    let measured = format!("kill -9 at {killed_at} ({} ms after the last accepted line), back at {back_at} ({} ms down); resumed after committed beat \
                            {committed} (killed in beat 4's Compose; snapshot paused: {paused_snapshot}); instant actions accepted since that commit: 1, lost: 0; \
                            Compose text typed in the killed beat kept by the core: {compose_kept}; duel clock paused {paused} ms",
                           killed_at - b3["received_at_ms"].as_i64().unwrap(), back_at - killed_at);

    // ---- (d) the admin's state at a round, and a fork there ----------------------------------------------------------
    let fork_round = round + 1;
    let cf = a.wait("round.committed", |d| d["round"].as_u64() == Some(fork_round)).await;
    let at_commit = duel(&cf["public_state"]["present"], &id).clone();
    assert_eq!(timeline(&at_commit).len(), 4);
    let public = admin(port, &format!("/admin/matches/{match_id}/state?after_round={fork_round}&view=public")).await;
    let hist = &public["state"]["present"];
    let hist = if hist.is_null() { &public["present"] } else { hist };
    assert_eq!(timeline(duel(hist, &id)), timeline(&at_commit), "state at a round returns the open duel: {}", public.to_string().chars().take(300).collect::<String>());
    let raw = admin(port, &format!("/admin/matches/{match_id}/state?after_round={fork_round}&view=raw")).await;
    let raw_state = if raw["state"]["dm"].is_object() { &raw["state"] } else { &raw };
    assert_eq!(raw_state["dm"]["duels"][0]["blows"].as_array().map(Vec::len), Some(4), "the whole timeline is in the state");
    let fork: Value = reqwest::Client::new().post(format!("http://127.0.0.1:{port}/admin/fork")).bearer_auth(ADMIN)
        .json(&json!({ "match_id": match_id, "after_round": fork_round })).send().await.unwrap().json().await.unwrap();
    let fork_token = fork.to_string();
    let fork_token = fork["seats"].as_array().or(fork["players"].as_array()).or(fork["tokens"].as_array())
        .and_then(|s| s.iter().find(|x| x["seat"] == 0)).and_then(|s| s["resume_token"].as_str()).map(str::to_string)
        .unwrap_or_else(|| panic!("no resume token for seat 0 in the fork reply: {fork_token}"));
    let (mut f, fsnap) = Client::resume(port, "AshFork", &fork_token).await;
    assert_ne!(fsnap["match_id"], match_id.as_str(), "the fork is its own match");
    let fork_id = fsnap["match_id"].as_str().unwrap().to_string();
    let fd = duel(&fsnap["public_state"]["present"], &id);
    assert_eq!(timeline(fd), timeline(&at_commit), "the forked room has the duel open, as it was at that beat");
    let frs = f.wait("round.started", |d| d["round"].as_u64() == Some(fork_round + 1)).await;
    assert!(frs["public"]["duel_clock"]["pause"]["ms"].as_i64().unwrap_or(0) >= 0);
    f.wait("phase.changed", |d| d["round"].as_u64() == Some(fork_round + 1) && d["phase"] == "compose").await;
    let fb = f.instant(fork_round + 1, json!({ "duel": { "duel_id": id, "text": "I slam your shoulder into the wall" } }), "duel.blow").await;
    assert_eq!(fb["n"].as_u64(), Some(at_commit["n"].as_u64().unwrap() + 1), "and it continues in the fork");
    let fout = f.wait("round.outcome", |d| d["round"].as_u64() == Some(fork_round + 1)).await;
    assert_eq!(fout["public"]["duels_open"][0]["typed"], 5, "{}", fout["public"]["duels_open"]);
    f.wait("round.committed", |d| d["round"].as_u64() == Some(fork_round + 1)).await;
    // The original match went on meanwhile and still has its own duel.
    let (_, q) = a.ask("game.present", json!({})).await;
    assert_eq!(duel(&q, &id)["typed"], 4, "the parent's duel is its own");
    a.wait("round.committed", |d| d["round"].as_u64() == Some(fork_round + 1)).await;

    // ---- (c) the recordings: every instant action with its arrival stamp, and 0 differences on replay ---------------
    drop(a);
    drop(f);
    // SIGINT (the server's shutdown signal): it flushes its recordings and saves its rooms.
    let pid = server.child.id();
    Command::new("kill").arg("-INT").arg(pid.to_string()).status().unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    while reqwest::get(format!("http://127.0.0.1:{port}/healthz")).await.is_ok() && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    drop(server);
    let rec = dir.join("data").join("recordings");
    let parent = rec.join(format!("{match_id}.jsonl"));
    let records = fn7_server::replay::read_records(&parent).unwrap();
    let stamped = |seq: &Value, at: &Value| {
        records.iter().any(|r| r["type"] == "event" && r["kind"] == "instant.submit" && r["data"]["seq"] == *seq && r["data"]["received_at_ms"] == *at)
            || records.iter().any(|r| r["type"] == "event" && r["kind"] == "journal_recovered"
                && r["data"]["actions"].as_array().is_some_and(|a| a.iter().any(|x| x["seq"] == *seq && x["received_at_ms"] == *at)))
    };
    for (name, e) in [("summon", &summoning), ("challenge", &lock), ("line 1", &b1), ("line 2", &b2), ("line 3 (before the kill)", &b3), ("line 4", &b4)] {
        assert!(stamped(&e["seq"], &e["received_at_ms"]), "{name} is in the recording with its arrival stamp");
    }
    assert!(records.iter().any(|r| r["type"] == "event" && r["kind"] == "journal_recovered"), "the recovery is recorded");
    // What the kill cost the recording itself (the core writes it through a buffer): the records of the killed beat.
    let text = std::fs::read_to_string(&parent).unwrap();
    let torn = text.lines().filter(|l| serde_json::from_str::<Value>(l).is_err()).count();
    let line3_arrival = records.iter().any(|r| r["type"] == "event" && r["kind"] == "instant.submit" && r["data"]["seq"] == b3["seq"]
        && r["data"]["received_at_ms"] == b3["received_at_ms"]);
    eprintln!("{measured}; recording: {torn} torn line(s), the killed beat's `instant.submit` record on disk: {line3_arrival} (its action is in `journal_recovered` either way)");
    assert!(records.iter().any(|r| r["type"] == "event" && r["kind"] == "duel_clock" && !r["data"]["pause"].is_null()), "the pause is recorded");
    let mut log = vec![];
    let rep = fn7_server::replay::verify(&parent, |l| log.push(l.to_string())).await.unwrap();
    eprintln!("parent replay: {} snapshots over {} beats, {} identical, {} joins, differences {:?}", rep.snapshots, rep.beats, rep.identical, rep.takeovers, rep.differences);
    assert!(rep.differences.is_empty(), "the killed and resumed match replays with 0 differences: {:?} {log:?}", rep.differences);
    assert!(rep.beats as u64 > fork_round && rep.takeovers == 1, "through the kill and the mid-match join: {} beats, {} joins", rep.beats, rep.takeovers);
    let rep = fn7_server::replay::verify(rec.join(format!("{fork_id}.jsonl")), |_| {}).await.unwrap();
    eprintln!("fork replay: {} snapshots over {} beats, {} identical, differences {:?}", rep.snapshots, rep.beats, rep.identical, rep.differences);
    assert!(rep.differences.is_empty() && rep.snapshots > 0, "the fork replays with 0 differences: {:?}", rep.differences);
    let _ = std::fs::remove_dir_all(dir);
}

/// What `auth.resume` gives back, case by case (reported from the live house: a viewer resumed and got `auth.ok` and
/// nothing else). A SEATED player gets `room.snapshot` + `match.snapshot`, in a normal room and in the house. A VIEWER
/// who dropped is gone from the room (the core removes viewers on disconnect): its resume is `auth.ok` only, and it
/// must `room.join {viewer: true}` again, which gives both snapshots at once.
#[tokio::test(flavor = "multi_thread")]
async fn resume_as_a_viewer_and_as_a_player_in_the_house_and_in_a_room() {
    let dir = temp_dir();
    let port = free_port();
    let house = r#"{"seed":99,"beats_total":400,"llm":false,"flow_ms":2000,"duel_ai_chance":0.0}"#;
    let _server = Server::start_with(&dir, port, &[("THEATRE_HOUSE_ROOM", "true"), ("THEATRE_HOUSE_CODE", "FINAL7"),
        ("THEATRE_HOUSE_SETTINGS", house), ("THEATRE_HOUSE_ATTRACT", "off")]).await;
    let hello = {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws")).await.unwrap();
        let Some(Ok(Message::Text(t))) = ws.next().await else { panic!("no hello") };
        serde_json::from_str::<Value>(&t).unwrap()
    };
    assert_eq!(hello["d"]["api"]["rules_version"], fn7_plugin::RULES_VERSION, "the rules version is in server.hello.api");

    // A normal room, for the same two cases.
    let mut host = Client::connect(port, "Host").await;
    let host_token = host.login().await["resume_token"].as_str().unwrap().to_string();
    let (_, room) = host.ask("room.create", json!({ "visibility": "private", "settings": { "seed": 5, "llm": false, "flow_ms": 2000, "beats_total": 400 } })).await;
    let code = room["code"].as_str().unwrap().to_string();
    host.ask("room.bot_add", json!({ "count": 2 })).await;
    host.ask("room.ready", json!({ "ready": true })).await;
    host.ask("room.start", json!({})).await;
    host.wait("match.started", |_| true).await;

    for (place, join) in [("the house", json!({ "house": true })), ("a room", json!({ "code": code }))] {
        // ---- a viewer ----
        let mut v = Client::connect(port, "Vera").await;
        let token = v.login().await["resume_token"].as_str().unwrap().to_string();
        let mut j = join.clone();
        j["viewer"] = true.into();
        let (t, _) = v.ask("room.join", j.clone()).await;
        assert_eq!(t, "room.snapshot", "{place}");
        v.wait("match.snapshot", |_| true).await;
        let c = v.wait("round.committed", |_| true).await;
        assert!(c["public_state"]["rules"]["version"].is_string(), "{place}: the match's rules version is in the public view");
        // A viewer gets the public outcome of each beat; `round.brief` goes to seats only.
        v.wait("round.outcome", |_| true).await;
        v.wait("round.outcome", |_| true).await;
        assert!(v.inbox.iter().all(|m| m["t"] != "round.brief"), "{place}: viewers are not sent round.brief");
        drop(v);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut v = Client::connect(port, "Vera").await;
        let (t, _) = v.ask("auth.resume", json!({ "resume_token": token })).await;
        assert_eq!(t, "auth.ok");
        let got = v.wait_for("room.snapshot", |_| true, 4).await;
        assert!(got.is_none(), "{place}: a resumed viewer is NOT put back in the room (the core dropped it at the disconnect)");
        assert!(v.inbox.iter().all(|m| m["t"] != "match.snapshot" && m["t"] != "room.left"), "{place}: and is told nothing");
        // The client's move: join again as a viewer.
        let (t, r) = v.ask("room.join", j).await;
        assert_eq!(t, "room.snapshot", "{place}: {r}");
        assert_eq!(r["you"]["viewer"], true);
        let snap = v.wait("match.snapshot", |_| true).await;
        assert!(snap["public_state"]["present"]["server_time_ms"].is_i64(), "{place}: and the present comes with it");
        drop(v);
    }
    // ---- a seated player in a room ----
    drop(host);
    let (mut host, snap) = Client::resume(port, "Host", &host_token).await;
    assert_eq!(snap["you"]["seat"], 0, "a room: a seated player's resume gets both snapshots");
    // ---- a seated player in the house (joins mid-match, plays from the next beat) ----
    let mut s = Client::connect(port, "Sam").await;
    let token = s.login().await["resume_token"].as_str().unwrap().to_string();
    let (t, _) = s.ask("room.join", json!({ "house": true })).await;
    assert_eq!(t, "room.snapshot");
    s.wait("seat.joined", |d| d["display_name"] == "Sam").await;
    drop(s);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (mut s, snap) = Client::resume(port, "Sam", &token).await;
    assert!(snap["you"]["seat"].is_u64(), "the house: a seated player's resume gets both snapshots: {}", snap["you"]);
    assert!(snap["public_state"]["present"]["live"] == true);
    // Seats do get the brief.
    s.wait("round.brief", |_| true).await;
    host.wait("round.brief", |_| true).await;
    let house: Value = reqwest::get(format!("http://127.0.0.1:{port}/api/house")).await.unwrap().json().await.unwrap();
    assert_eq!(house["code"], "FINAL7");
    let _ = std::fs::remove_dir_all(dir);
}
