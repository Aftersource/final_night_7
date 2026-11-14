//! The asset registry against the real server (docs/ASSETS.md): the `fn7-server` binary on a private port with its own
//! data directory and `FN7_IMAGES=mock` (free placeholders; nothing here calls a provider), driven over a WebSocket.
//!
//! One short match is played to its end. Then every image the server was asked for (its `/media/index`) must be in
//! the registry with its exact prompt, timing, cost, file path and, for the match's images, a `uses` row; the file
//! route must serve the bytes the media route serves; the time-travel lookup must answer; and the match's recording
//! must replay with 0 differences (the registry's wrapper reads payloads, it never changes them).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const ADMIN: &str = "assets-test-admin";
const KEY: &str = "assets-test-key";

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
    dir: PathBuf,
}

impl Server {
    /// Start the server in `dir` (its working directory too, so no project `.env` is read) with a clean environment.
    async fn start(dir: &Path, port: u16) -> Server {
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
            .env("FN7_MEDIA_PREWARM_NAMES", "Chris")
            .env("FN7_PROFILE_KEY", KEY)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .expect("fn7-server starts");
        let s = Server { child, dir: dir.to_path_buf() };
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
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Client {
    ws: Ws,
    inbox: Vec<Value>,
    n: u64,
}

impl Client {
    async fn connect(port: u16) -> Client {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws")).await.expect("ws connects");
        let mut c = Client { ws, inbox: vec![], n: 0 };
        c.wait("server.hello", 20).await;
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

    /// The next message of type `t` (earlier ones stay in the inbox).
    async fn wait(&mut self, t: &str, secs: u64) -> Value {
        let until = Instant::now() + Duration::from_secs(secs);
        let mut from = 0;
        loop {
            if let Some(i) = (from..self.inbox.len()).find(|i| self.inbox[*i]["t"] == t) {
                return self.inbox.remove(i)["d"].take();
            }
            from = self.inbox.len();
            let Some(m) = self.read(until).await else {
                let seen: Vec<String> = self.inbox.iter().rev().take(20).map(|m| m["t"].as_str().unwrap_or("?").to_string()).collect();
                panic!("no `{t}` within {secs} s (last messages, newest first: {seen:?})");
            };
            self.inbox.push(m);
        }
    }

    async fn ask(&mut self, t: &str, d: Value) -> (String, Value) {
        self.n += 1;
        let id = format!("c-{}", self.n);
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
}

async fn http(port: u16, path: &str) -> (u16, Value) {
    let sep = if path.contains('?') { '&' } else { '?' };
    let r = reqwest::get(format!("http://127.0.0.1:{port}{path}{sep}key={KEY}")).await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

async fn bytes(port: u16, path: &str) -> (u16, Vec<u8>) {
    let sep = if path.contains('?') { '&' } else { '?' };
    let r = reqwest::get(format!("http://127.0.0.1:{port}{path}{sep}key={KEY}")).await.unwrap();
    (r.status().as_u16(), r.bytes().await.unwrap().to_vec())
}

#[tokio::test(flavor = "multi_thread")]
async fn every_image_of_a_played_match_is_in_the_registry() {
    let dir = std::env::temp_dir().join(format!("fn7-assets-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let _server = Server::start(&dir, port).await;

    // ---- a short match: one human who never types, three bots, no LLM ---------------------------------------------
    let mut a = Client::connect(port).await;
    let (t, _) = a.ask("auth.login", json!({ "display_name": "Ash", "client": { "kind": "test", "version": "1" } })).await;
    assert_eq!(t, "auth.ok");
    let settings = json!({ "seed": 77, "beats_total": 4, "llm": false, "flow_ms": 1500, "first_compose_seconds": 1, "duel_ai_chance": 0.0 });
    let (t, room) = a.ask("room.create", json!({ "visibility": "private", "settings": settings })).await;
    assert_eq!(t, "room.snapshot", "{room}");
    a.ask("room.bot_add", json!({ "count": 3 })).await;
    a.ask("room.ready", json!({ "ready": true })).await;
    a.ask("room.start", json!({})).await;
    let started = a.wait("match.started", 40).await;
    let match_id = started["match_id"].as_str().unwrap().to_string();
    // An image by reference, mid-match, from seat 0.
    let c1 = a.wait("round.committed", 60).await;
    let mine = c1["public_state"]["demigods"].as_array().unwrap().iter().find(|d| d["seat"] == 0).unwrap().clone();
    let (t, by_ref) = a.ask("game.media", json!({ "kind": "diorama", "ref": mine["ref"] })).await;
    assert!(by_ref.to_string().contains("\"hash\""), "{t}: {by_ref}");
    let ended = a.wait("match.ended", 180).await;
    assert_eq!(ended["match_id"], match_id.as_str());

    // ---- everything the media layer was asked for -------------------------------------------------------------------
    // The manifest settles (the queue drains), then the registry's writer catches up.
    let until = Instant::now() + Duration::from_secs(60);
    let items = loop {
        let (_, m) = http(port, "/media/index?since=0").await;
        let items = m["items"].as_array().cloned().unwrap_or_default();
        let pending = items.iter().filter(|i| i["status"] == "pending").count();
        let (_, st) = http(port, "/api/assets/stats").await;
        let known = st["by_kind_status"].as_array().map(|v| v.iter().filter(|r| r["status"] != "pending" && r["subkind"] != "reference").map(|r| r["n"].as_u64().unwrap_or(0)).sum::<u64>()).unwrap_or(0);
        if !items.is_empty() && pending == 0 && known as usize >= items.len() {
            break items;
        }
        assert!(Instant::now() < until, "the registry did not catch up: {} manifest items, {pending} pending, {known} settled assets", items.len());
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    assert!(items.len() >= 6 * 9 + 4, "the prewarm (9 names, 6 images each) and the match's own images: {}", items.len());
    let (mut ready, mut of_match, mut with_ref) = (0, 0, 0);
    for it in &items {
        let hash = it["hash"].as_str().unwrap();
        let (s, d) = http(port, &format!("/api/assets/{hash}")).await;
        assert_eq!(s, 200, "{hash} ({}) is in the registry", it["kind"]);
        let (asset, gens, uses) = (&d["asset"], d["generations"].as_array().unwrap(), d["uses"].as_array().unwrap());
        assert_eq!(asset["media_hash"], hash);
        assert_eq!((asset["kind"].as_str(), asset["subkind"].as_str()), (Some("image"), it["kind"].as_str()));
        assert_eq!(asset["status"], it["status"], "{hash}");
        assert!(!gens.is_empty(), "{hash} has a generation");
        // The prompt, byte for byte what the media layer sent.
        for g in gens {
            assert_eq!(g["prompt"].as_str().unwrap().as_bytes(), it["prompt"].as_str().unwrap().as_bytes(), "{hash}");
        }
        if it["status"] == "ready" {
            ready += 1;
            let g = gens.iter().find(|g| g["status"] == "ok").unwrap_or_else(|| panic!("{hash}: no ok generation: {d}"));
            // Timing and cost.
            for k in ["requested_at", "started_at", "finished_at"] {
                assert!(g[k].as_i64().is_some_and(|t| t > 1_600_000_000_000), "{hash}: {k} = {}", g[k]);
            }
            assert!(g["requested_at"].as_i64() <= g["finished_at"].as_i64() && g["duration_ms"].as_f64().is_some_and(|d| d >= 0.0), "{hash}: {g}");
            assert_eq!((g["cost_usd"].as_f64(), g["cost_basis"].as_str(), g["provider"].as_str()), (Some(0.0), Some("free"), Some("mock")), "{hash}");
            assert!(g["requested_by"].as_str().is_some_and(|b| b == "prewarm" || b.starts_with("match:")), "{hash}: {}", g["requested_by"]);
            // The file: in the layout by its sha256, and the same bytes the media route serves.
            let path = asset["path"].as_str().unwrap();
            let sha = asset["sha256"].as_str().unwrap();
            assert_eq!(path, format!("images/{}/{sha}.svg", it["kind"].as_str().unwrap()));
            assert!(dir.join("data/fn7/assets").join(path).is_file(), "{path} is on disk under the assets root");
            if ready % 7 == 1 {
                let (s1, from_media) = bytes(port, it["url"].as_str().unwrap()).await;
                let (s2, from_assets) = bytes(port, asset["file_url"].as_str().unwrap()).await;
                assert_eq!((s1, s2), (200, 200));
                assert_eq!(from_media, from_assets, "{hash}");
                assert_eq!(from_assets.len() as u64, asset["bytes"].as_u64().unwrap());
            }
        } else {
            assert!(asset["path"].is_null(), "{hash}: a {} image has no file", it["status"]);
        }
        // Images the match showed have a use in it.
        if uses.iter().any(|u| u["match_id"] == match_id.as_str()) {
            of_match += 1;
            assert!(uses.iter().any(|u| u["match_id"] == match_id.as_str() && u["beat"].as_i64().is_some_and(|b| b >= 0)), "{hash}: a beat");
        } else {
            assert!(gens.iter().all(|g| g["requested_by"] == "prewarm"), "{hash}: an image nobody in the match used was the prewarm's: {d}");
        }
        if uses.iter().any(|u| u["subject_ref"].as_str().is_some_and(|r| r.starts_with("dg:"))) {
            with_ref += 1;
        }
    }
    assert!(ready >= 54 && of_match >= 8 && with_ref >= 4, "ready {ready}, used in the match {of_match}, on a demigod {with_ref}");
    eprintln!("{} images: {ready} ready, {of_match} used in the match, {with_ref} on a demigod ref", items.len());

    // ---- the lookups ------------------------------------------------------------------------------------------------
    let name = mine["name"].as_str().unwrap();
    let (s, at) = http(port, &format!("/api/assets/at?match={match_id}&beat=2&demigod={}", name.replace(' ', "%20").replace('\'', "%27"))).await;
    assert_eq!(s, 200);
    let looks = at["items"].as_array().unwrap();
    let portrait = looks.iter().find(|i| i["subkind"] == "portrait").unwrap_or_else(|| panic!("what {name} looked like at beat 2: {at}"));
    assert_eq!(portrait["hash"].as_str().or(portrait["media_hash"].as_str()), mine["portrait_image"]["hash"].as_str());
    assert!(portrait["first_beat"].as_i64().unwrap() <= 2 && portrait["path"].as_str().unwrap().starts_with("images/portrait/"));
    assert_eq!(bytes(port, portrait["file_url"].as_str().unwrap()).await.0, 200);
    let (_, by_ref) = http(port, &format!("/api/assets/at?match={match_id}&beat=2&demigod={}", mine["ref"].as_str().unwrap())).await;
    assert_eq!(by_ref["items"].as_array().unwrap().iter().find(|i| i["subkind"] == "portrait").map(|p| p["id"].clone()), Some(portrait["id"].clone()));
    let (_, list) = http(port, &format!("/api/assets?match={match_id}&limit=500")).await;
    assert_eq!(list["total"].as_u64().unwrap() as usize, of_match, "the list's match filter agrees with the uses");
    let (_, costs) = http(port, "/api/assets/costs?by=match").await;
    let keys: Vec<&str> = costs["items"].as_array().unwrap().iter().filter_map(|i| i["key"].as_str()).collect();
    assert!(keys.contains(&match_id.as_str()) && keys.contains(&"prewarm"), "{keys:?}");
    // The statue of the end screen is the match's, on its demigod.
    let (_, statues) = http(port, &format!("/api/assets?match={match_id}&kind=statue")).await;
    assert!(statues["total"].as_u64().unwrap() >= 1, "{statues}");
    // Without the key: nothing.
    assert_eq!(reqwest::get(format!("http://127.0.0.1:{port}/api/assets")).await.unwrap().status().as_u16(), 403);
    // The media routes answer as before, without a key.
    assert_eq!(reqwest::get(format!("http://127.0.0.1:{port}/media/index?since=0")).await.unwrap().status().as_u16(), 200);

    // ---- the recording replays with 0 differences -------------------------------------------------------------------
    let rec = dir.join("data/recordings").join(format!("{match_id}.jsonl"));
    let until = Instant::now() + Duration::from_secs(20);
    while !rec.is_file() && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let rep = fn7_server::replay::verify(&rec, |_| {}).await.unwrap();
    eprintln!("replay: {} snapshots over {} beats, {} identical, differences {:?}", rep.snapshots, rep.beats, rep.identical, rep.differences);
    assert!(rep.snapshots > 0 && rep.differences.is_empty(), "the match replays with 0 differences: {:?}", rep.differences);
}
