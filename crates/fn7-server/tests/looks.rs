//! Gear looks through the server (mock images only; no LLM, no paid API): a recorded match in which a human arms
//! their demigods. The look changes with `pending`, the mock images arrive and the next public payload says so, a
//! reconnect mid-way (`public_view`, `seat_view`) shows the right look, snapshots at earlier beats show the earlier
//! look, the built-in AI's demigods keep their base images, the profile library records each gear picture with its
//! match, beat and loadout, the recording replays with zero differences, and a later process over the same cache
//! resolves every look without generating anything.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fn7_media::{ImageGenerator, Media, MediaConfig, MockGenerator, Mode};
use fn7_plugin::testkit::Failing;
use fn7_plugin::FinalNight7Plugin;
use fn7_profile::ProfileStore;
use fn7_server::media::MediaPlugin;
use fn7_server::profile::Profiles;
use fn7_server::record::{recorder, Recorded};
use serde_json::{json, Value};
use theatre_plugin_api::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fn7-looks-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn media(dir: &std::path::Path, gen: MockGenerator) -> Media {
    let cfg = MediaConfig { mode: Mode::Mock, dir: dir.join("media"), ..Default::default() };
    Media::start(cfg, Some(Arc::new(gen) as Arc<dyn ImageGenerator>))
}

fn demigod<'a>(view: &'a Value, r: &str) -> &'a Value {
    view["demigods"].as_array().unwrap().iter().find(|d| d["ref"] == r).unwrap_or_else(|| panic!("{r} in the view"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_human_arms_their_demigods_the_look_swaps_in_and_the_recording_replays_clean() {
    let dir = temp_dir("match");
    // Slow enough that an image asked for in a resolve is still rendering when its payload goes out.
    let media_ = media(&dir, MockGenerator { delay: Duration::from_millis(120), ..Default::default() });
    let rec = recorder(dir.join("rec"), theatre_trace::Level::Full).unwrap();
    let store = Arc::new(ProfileStore::open(&dir.join("fn7").join("profiles.sqlite")).unwrap());
    let profiles = Profiles::new(store.clone());
    profiles.attach_recordings(rec.dir.clone(), Some(rec.sink.clone()));
    let p = Recorded::new(
        MediaPlugin::new(FinalNight7Plugin::new().with_media_style(media_.style().ruleset_json()), media_.clone()).with_profiles(profiles.clone()),
        rec.run("looks1"),
    );
    let llm = Failing;
    let seats: Vec<SeatInfo> = [("Chris", false), ("Benito", false), ("Cyra", true), ("Dune", true)]
        .iter().enumerate().map(|(i, (n, bot))| SeatInfo { seat: i as u8, display_name: n.to_string(), is_bot: *bot }).collect();
    let settings = p.validate_settings(&json!({ "seed": 31, "beats_total": 40, "llm": false, "duel_ai_chance": 0.0, "dm_encounters_per_beat": 0,
        "bolt_start": 6, "arena_close_beats": 1000, "look_debounce_beats": 2 })).unwrap();
    let mut state = p.on_match_start(&seats, &settings, &llm).await.unwrap();
    // The clients join (their `match.snapshot` carries the public view): the base portraits and dioramas are made.
    p.public_view(&state);
    assert!(media_.wait_idle(Duration::from_secs(20)).await);
    let mut before: Option<Value> = None; // a state from before any gear
    let mut first: Option<(u32, Value, Value)> = None; // (beat, the pending event, the state after it)
    let mut ready_event: Option<Value> = None;
    let mut all_events: Vec<Value> = vec![];
    for round in 1..=12u32 {
        let start = p.on_round_start(&state, round, &llm).await.unwrap();
        state = start.state.clone();
        // A look announced while its images rendered is announced again, ready, in the next public payload.
        if let Some(l) = start.public["looks"].as_array() {
            for e in l {
                assert_eq!((e["event"].as_str(), e["ready"].as_bool()), (Some("look.changed"), Some(true)), "{e}");
                assert!(e.get("pending").is_none() && e["portrait_image"]["status"] == "ready" && e["diorama_image"]["status"] == "ready", "{e}");
                ready_event.get_or_insert(e.clone());
            }
        }
        let mut actions: Vec<Action> = vec![];
        for s in &seats {
            actions.push(if s.is_bot { p.bot_action(&state, s.seat, &llm).await.unwrap() } else { p.default_action(&state, s.seat) });
        }
        if round == 2 {
            before = Some(state.clone());
        }
        if (3..=5).contains(&round) || round == 8 {
            let text = if round == 8 { "arm my demigods with moonsteel plate armor" } else { "arm my demigods with butterfly wing blades" };
            actions[0] = fn7_plugin::actions::action(0, text, "", None);
        }
        let outcome = p.resolve_round(&state, round, &actions, &RoundContext::default(), &llm, &NoProgress).await.unwrap();
        let committed = p.commit_round(&outcome.state, round, &[], &llm).await.unwrap();
        state = committed.state.clone();
        let evs: Vec<Value> = outcome.public["looks"].as_array().cloned().unwrap_or_default();
        for e in &evs {
            assert_eq!(e["player"], "Chris", "only the human who armed changes look: {e}");
            assert!(e.get("base_portrait").is_none() && e.get("portrait_subject").is_none(), "the inputs are consumed: {e}");
            assert!(outcome.public["dm_events"].as_array().unwrap().iter().any(|d| d == e), "the same event rides in dm_events");
        }
        if first.is_none() {
            if let Some(e) = evs.iter().find(|e| e["image_change"] == true) {
                first = Some((round, e.clone(), state.clone()));
            }
        }
        all_events.extend(evs);
        // The images of this beat arrive before the next one starts.
        assert!(media_.wait_idle(Duration::from_secs(20)).await);
    }
    let (beat, pending, after) = first.expect("an arming changed a look within the first beats");
    let who = pending["demigod"].as_str().unwrap().to_string();

    // At acquisition: the gear images are pending, the payload keeps the previous (base) images.
    let gear_p = pending["pending"]["portrait_image"]["hash"].as_str().expect("pending portrait").to_string();
    let gear_d = pending["pending"]["diorama_image"]["hash"].as_str().expect("pending diorama").to_string();
    assert_eq!(pending["pending"]["portrait_image"]["status"], "pending");
    let base = p.public_view(before.as_ref().unwrap());
    let base_d = demigod(&base, &who);
    assert_eq!(pending["portrait_image"]["hash"], base_d["portrait_image"]["hash"], "the base portrait stands in");
    assert_eq!(pending["portrait_image"]["status"], "ready");
    assert!(pending["gained"]["name"].as_str().unwrap().contains("Blade"), "{pending}");
    assert_eq!(pending["loadout"][0]["slot"], "weapon");
    assert!(pending["pending"]["portrait_image"]["prompt"].as_str().unwrap().contains("gripped in the right hand"));

    // Then ready: announced once more with the same hashes.
    let ready = ready_event.expect("the ready announcement");
    assert_eq!((ready["demigod"].as_str(), ready["portrait_image"]["hash"].as_str(), ready["diorama_image"]["hash"].as_str()),
               (Some(who.as_str()), Some(gear_p.as_str()), Some(gear_d.as_str())));
    assert_eq!(ready["memory"], pending["memory"]);

    // A reconnect mid-way (a snapshot's public and seat views of the state after that beat): the right look, ready.
    let pv = p.public_view(&after);
    let l = &demigod(&pv, &who)["look"];
    assert_eq!((l["portrait_image"]["hash"].as_str(), l["portrait_image"]["status"].as_str(), l["since_beat"].as_u64()),
               (Some(gear_p.as_str()), Some("ready"), Some(beat as u64)), "{l}");
    assert_eq!(l["diorama_image"]["hash"], gear_d.as_str());
    assert!(l.get("pending").is_none() && l["gear"] == true);
    assert_eq!(demigod(&pv, &who)["portrait_image"]["hash"], base_d["portrait_image"]["hash"], "the identity image is still there");
    let sv = p.seat_view(&after, 0);
    let mine = sv["demigods"].as_array().unwrap().iter().find(|d| d["ref"] == who.as_str()).unwrap();
    assert_eq!(mine["look"]["portrait_image"]["hash"], gear_p.as_str());
    // Time travel: the state from before the arming still shows the base look; the final state shows the final one.
    assert_eq!(base_d["look"]["portrait_image"]["hash"], base_d["portrait_image"]["hash"]);
    assert_eq!(base_d["look"]["gear"], false);
    let fin = p.public_view(&state);
    let lf = &demigod(&fin, &who)["look"];
    assert_eq!(lf["portrait_image"]["status"], "ready");
    assert!(all_events.len() >= 2, "blades for both hands at least: {}", all_events.len());
    // The built-in bots' demigods never get a gear picture, whatever they carry.
    for d in fin["demigods"].as_array().unwrap().iter().filter(|d| d["controller"] == "ai") {
        assert_eq!(d["look"]["gear"], false, "{}", d["name"]);
        assert_eq!(d["look"]["portrait_image"]["hash"], d["portrait_image"]["hash"]);
    }
    // Items with a picture carry it everywhere they are listed.
    let it = &l["loadout"][0];
    assert_eq!((it["image"]["kind"].as_str(), it["image"]["status"].as_str()), (Some("item"), Some("ready")), "{it}");
    let peg = demigod(&pv, &who)["story"]["items"].as_array().unwrap().iter().find(|x| x["ref"] == it["item"]).unwrap();
    assert_eq!(peg["image"]["hash"], it["image"]["hash"]);
    let my_item = sv["my_items"].as_array().unwrap().iter().find(|x| x["ref"] == it["item"]).unwrap();
    assert_eq!(my_item["image"]["hash"], it["image"]["hash"]);

    // The library: the gear portrait and diorama, with the match, the beat and the loadout.
    let (_, summary) = (0, p.summarize(&state, &llm).await.unwrap());
    assert!(media_.wait_idle(Duration::from_secs(20)).await);
    tokio::time::sleep(Duration::from_millis(600)).await; // the library watchers
    rec.flush();
    let lib = store.library("chris", 0, Some(100), Some("portrait")).unwrap();
    let g = lib.items.iter().find(|i| i.hash == gear_p).unwrap_or_else(|| panic!("the gear portrait is in the library: {:?}", lib.items.iter().map(|i| &i.caption).collect::<Vec<_>>()));
    assert_eq!(g.subject.as_deref(), Some(who.as_str()));
    assert_eq!((g.looks[0]["match_id"].as_str(), g.looks[0]["beat"].as_u64()), (Some("looks1"), Some(beat as u64)), "{:?}", g.looks);
    assert!(g.looks[0]["loadout"][0].as_str().unwrap().contains("Blade") && g.looks[0]["loadout_hash"].as_str().unwrap().len() == 64);
    assert!(g.caption.contains("portrait with"), "{}", g.caption);
    assert!(store.library("chris", 0, Some(100), Some("diorama")).unwrap().items.iter().any(|i| i.hash == gear_d));
    assert!(store.library("chris", 0, Some(100), Some("item")).unwrap().total >= 1, "the item pictures too (the piles)");
    assert!(summary.summary["players"][0]["demigods"][0]["looks"].is_array());

    // The recording replays with zero differences (the look step is rules only; the summary too).
    let path = dir.join("rec").join("looks1.jsonl");
    let rep = fn7_server::replay::verify(&path, |_| {}).await.unwrap();
    assert!(rep.snapshots > 20 && rep.differences.is_empty(), "{:?}", rep.differences);
    assert_eq!((rep.identical, rep.summary_identical), (rep.snapshots, Some(true)));
    // Every snapshot of the recording, shown again by another process over the same cache (a scrub, a fork, a
    // replay through the server): each look resolves to a file that exists, and nothing is generated (this
    // generator fails whatever it is asked).
    let later = media(&dir, MockGenerator { fail: true, ..Default::default() });
    let viewer = MediaPlugin::new(FinalNight7Plugin::new().with_media_style(later.style().ruleset_json()), later.clone());
    let text = std::fs::read_to_string(&path).unwrap();
    let mut seen_gear = std::collections::BTreeSet::new();
    let mut n = 0;
    for line in text.lines().filter(|l| l.starts_with("{\"type\":\"snapshot\"") && l.contains("\"label\":\"commit_round\"")) {
        let snap: Value = serde_json::from_str(line).unwrap();
        let v = viewer.public_view(&snap["state"]);
        for d in v["demigods"].as_array().unwrap() {
            let l = &d["look"];
            assert_eq!((l["portrait_image"]["status"].as_str(), l["diorama_image"]["status"].as_str()), (Some("ready"), Some("ready")), "{l}");
            assert!(l.get("pending").is_none());
            if l["gear"] == true {
                seen_gear.insert(l["portrait_image"]["hash"].as_str().unwrap().to_string());
            }
        }
        n += 1;
    }
    assert!(n >= 10 && seen_gear.contains(&gear_p), "{n} snapshots, {} gear looks", seen_gear.len());
    assert!(later.wait_idle(Duration::from_secs(5)).await);
    let not_ready: Vec<String> = later.manifest(0)["items"].as_array().unwrap().iter().filter(|e| e["status"] != "ready")
        // (An omen's moment card is not a look: a fresh process asks for those under its interim rule.)
        .filter(|e| matches!(e["kind"].as_str(), Some("portrait" | "diorama" | "item")))
        .map(|e| format!("{} {}: {}", e["kind"], e["status"], e["prompt"].as_str().unwrap_or("").chars().take(160).collect::<String>())).collect();
    assert!(not_ready.is_empty(), "nothing was generated, refused or failed: {not_ready:#?}");
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------------------------------------------------
// End to end: the real server binary on a private port, driven over WebSockets
// ---------------------------------------------------------------------------------------------------------------------

mod e2e {
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use futures::{SinkExt, StreamExt};
    use serde_json::{json, Value};
    use tokio_tungstenite::tungstenite::Message;

    type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    const ADMIN: &str = "looks-test";

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
    }

    impl Server {
        /// Start `bin` in `dir` (its working directory too) with a clean environment: mock images, no LLM key.
        async fn start(bin: &str, dir: &Path, port: u16) -> Server {
            let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("server.log")).unwrap();
            let child = Command::new(bin)
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
            let until = Instant::now() + Duration::from_secs(60);
            let mut from = 0;
            loop {
                if let Some(i) = (from..self.inbox.len()).find(|i| self.inbox[*i]["t"] == t && pred(&self.inbox[*i]["d"])) {
                    return self.inbox.remove(i)["d"].take();
                }
                from = self.inbox.len();
                match self.read(until).await {
                    Some(m) => self.inbox.push(m),
                    None => {
                        let seen: Vec<String> = self.inbox.iter().rev().take(25).map(|m| m["t"].as_str().unwrap_or("?").to_string()).collect();
                        panic!("{}: no `{t}` (last messages, newest first: {seen:?})", self.name)
                    }
                }
            }
        }

        async fn ask(&mut self, t: &str, d: Value) -> (String, Value) {
            self.n += 1;
            let id = format!("{}-{}", self.name, self.n);
            self.ws.send(Message::Text(json!({ "t": t, "id": id, "d": d }).to_string().into())).await.expect("send");
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
    }

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("fn7-looks-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    async fn http(port: u16, path: &str, admin: bool) -> reqwest::Response {
        let mut r = reqwest::Client::new().get(format!("http://127.0.0.1:{port}{path}"));
        if admin {
            r = r.bearer_auth(ADMIN);
        }
        r.send().await.unwrap()
    }

    fn mine<'a>(public_state: &'a Value, r: &str) -> &'a Value {
        public_state["demigods"].as_array().unwrap().iter().find(|d| d["ref"] == r).unwrap_or_else(|| panic!("{r} in the public state"))
    }

    /// What one played match showed.
    struct Played {
        match_id: String,
        token: String,
        /// The first `look.changed` with `image_change` (as sent, at its beat) and its round.
        changed: Option<(u64, Value)>,
        /// Its second sending (`ready: true`).
        ready: Option<Value>,
        /// Every `look.changed` seen.
        events: usize,
    }

    /// One human (Ash) and three bots in a private room; Ash arms their demigods by will for a few beats, then plays
    /// on quietly until `beats` beats have committed.
    async fn play(port: u16, beats: u64) -> (Client, Played) {
        let mut a = Client::connect(port, "Ash").await;
        let (t, me) = a.ask("auth.login", json!({ "display_name": "Ash", "client": { "kind": "test", "version": "1" } })).await;
        assert_eq!(t, "auth.ok", "{me}");
        let token = me["resume_token"].as_str().unwrap().to_string();
        let settings = json!({ "seed": 606, "beats_total": 200, "llm": false, "flow_ms": 1500, "duel_ai_chance": 0.0, "dm_encounters_per_beat": 0,
                               "bolt_start": 6, "arena_close_beats": 1000, "look_debounce_beats": 2, "slow_ms": 3000 });
        let (t, room) = a.ask("room.create", json!({ "visibility": "private", "settings": settings })).await;
        assert_eq!(t, "room.snapshot", "{room}");
        a.ask("room.bot_add", json!({ "count": 3 })).await;
        a.ask("room.ready", json!({ "ready": true })).await;
        a.ask("room.start", json!({})).await;
        let started = a.wait("match.started", |_| true).await;
        let mut p = Played { match_id: started["match_id"].as_str().unwrap().to_string(), token, changed: None, ready: None, events: 0 };
        for round in 1..=beats {
            let rs = a.wait("round.started", |d| d["round"] == round).await;
            if let Some(e) = rs["public"]["looks"].as_array().and_then(|l| l.iter().find(|e| e["ready"] == true)) {
                p.ready.get_or_insert(e.clone());
            }
            if (2..=5).contains(&round) {
                a.wait("phase.changed", |d| d["round"] == round && d["phase"] == "compose").await;
                let text = "arm my demigods with butterfly wing blades";
                let (_, r) = a.ask("action.submit", json!({ "round": round, "text": text, "fields": { "left": text } })).await;
                assert_eq!(r["accepted"], true, "{r}");
            }
            let out = a.wait("round.outcome", |d| d["round"] == round).await;
            let looks = out["public"]["looks"].as_array().cloned().unwrap_or_default();
            p.events += looks.iter().filter(|e| e["ready"] != true).count();
            if let Some(e) = looks.iter().find(|e| e["ready"] == true) {
                p.ready.get_or_insert(e.clone());
            }
            if p.changed.is_none() {
                if let Some(e) = looks.iter().find(|e| e["image_change"] == true && e["gear"] == true) {
                    p.changed = Some((round, e.clone()));
                }
            }
            a.wait("round.committed", |d| d["round"] == round).await;
        }
        (a, p)
    }

    /// The whole story against this build's server: a human arms their demigods, the look changes with `pending`, the
    /// mock images arrive (the manifest, the files, the `ready` event), a reconnect mid-way shows the right look, the
    /// admin's state at an earlier round shows the earlier look, and the recording replays with zero differences.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_private_server_shows_the_look_change_the_images_arrive_and_a_reconnect_is_right() {
        let dir = temp_dir();
        let port = free_port();
        let server = Server::start(env!("CARGO_BIN_EXE_fn7-server"), &dir, port).await;
        let (a, p) = play(port, 9).await;
        let (round, e) = p.changed.clone().expect("an arming changed a look");
        let who = e["demigod"].as_str().unwrap().to_string();
        assert_eq!((e["event"].as_str(), e["player"].as_str()), (Some("look.changed"), Some("Ash")), "{e}");
        assert!(e["gained"]["name"].as_str().unwrap().contains("Blade"), "{e}");
        // At its beat: the new images were still on the way.
        let gp = e["pending"]["portrait_image"]["hash"].as_str().unwrap_or_else(|| panic!("pending at acquisition: {e}")).to_string();
        let gd = e["pending"]["diorama_image"]["hash"].as_str().unwrap().to_string();
        assert_ne!(e["portrait_image"]["hash"], gp.as_str(), "the previous image stands in");
        // The mock images arrived: the manifest says ready and the files are served.
        let index: Value = http(port, "/media/index?since=0", false).await.json().await.unwrap();
        for h in [&gp, &gd] {
            let it = index["items"].as_array().unwrap().iter().find(|i| i["hash"] == h.as_str()).unwrap_or_else(|| panic!("{h} in the manifest"));
            assert_eq!(it["status"], "ready", "{it}");
            assert!(http(port, it["url"].as_str().unwrap(), false).await.status().is_success());
        }
        // The event came again, ready, with the final images.
        let r = p.ready.clone().expect("the ready announcement");
        assert_eq!((r["demigod"].as_str(), r["memory"].clone(), r["portrait_image"]["hash"].as_str(), r["portrait_image"]["status"].as_str()),
                   (Some(who.as_str()), e["memory"].clone(), Some(gp.as_str()), Some("ready")));
        assert!(r.get("pending").is_none());
        // A reconnect mid-way: the snapshot's public state and the seat's own view show the look as it is now.
        drop(a);
        let mut b = Client::connect(port, "Ash").await;
        let (t, d) = b.ask("auth.resume", json!({ "resume_token": p.token })).await;
        assert_eq!(t, "auth.ok", "{d}");
        let snap = b.wait("match.snapshot", |_| true).await;
        let l = &mine(&snap["public_state"], &who)["look"];
        assert_eq!((l["gear"].as_bool(), l["portrait_image"]["status"].as_str(), l["diorama_image"]["status"].as_str()), (Some(true), Some("ready"), Some("ready")), "{l}");
        assert!(l.get("pending").is_none() && l["loadout"][0]["name"].as_str().unwrap().contains("Blade"));
        if l["since_beat"].as_u64() == Some(round) {
            assert_eq!((l["portrait_image"]["hash"].as_str(), l["diorama_image"]["hash"].as_str()), (Some(gp.as_str()), Some(gd.as_str())));
        }
        let own = snap["you"]["private_state"]["demigods"].as_array().unwrap().iter().find(|x| x["ref"] == who.as_str()).unwrap();
        assert_eq!(own["look"]["portrait_image"], l["portrait_image"]);
        // Time travel through the server: the state after beat 1 shows the base look, the state after the arming's
        // beat shows that beat's gear look, both as cached files.
        let at = |n: u64| format!("/admin/matches/{}/state?after_round={n}&view=public", p.match_id);
        let early: Value = http(port, &at(1), true).await.json().await.unwrap();
        let then: Value = http(port, &at(round), true).await.json().await.unwrap();
        let find = |v: &Value| -> Value {
            fn dig<'a>(v: &'a Value, who: &str) -> Option<&'a Value> {
                match v {
                    Value::Object(o) => {
                        if let Some(d) = o.get("demigods").and_then(Value::as_array).and_then(|a| a.iter().find(|d| d["ref"] == who && d.get("look").is_some())) {
                            return Some(d);
                        }
                        o.values().find_map(|x| dig(x, who))
                    }
                    Value::Array(a) => a.iter().find_map(|x| dig(x, who)),
                    _ => None,
                }
            }
            dig(v, &who).unwrap_or_else(|| panic!("{who} in the admin state: {}", v.to_string().chars().take(300).collect::<String>())).clone()
        };
        let (d1, dn) = (find(&early), find(&then));
        assert_eq!((d1["look"]["gear"].as_bool(), d1["look"]["portrait_image"]["hash"].clone()), (Some(false), d1["portrait_image"]["hash"].clone()));
        assert_eq!((dn["look"]["portrait_image"]["hash"].as_str(), dn["look"]["portrait_image"]["status"].as_str(), dn["look"]["since_beat"].as_u64()),
                   (Some(gp.as_str()), Some("ready"), Some(round)));
        // The recording replays clean.
        drop(b);
        drop(server);
        let rec = dir.join("data").join("recordings").join(format!("{}.jsonl", p.match_id));
        let rep = fn7_server::replay::verify(&rec, |_| {}).await.unwrap();
        assert!(rep.snapshots >= 20 && rep.differences.is_empty(), "{} snapshots: {:?}", rep.snapshots, rep.differences);
        eprintln!("e2e: {} look events; look.changed at beat {round}; replay {} snapshots, {} identical", p.events, rep.snapshots, rep.identical);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A match recorded by an OLDER server binary (`FN7_OLD_SERVER_BIN`, e.g. the deployed rules 0.9.0 / schema 10
    /// build), with the same armings, replays on this build with zero differences: gear looks change nothing in a
    /// state from before schema 11. Run with `FN7_OLD_SERVER_BIN=/path/to/fn7-server cargo test --test looks -- --ignored`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs FN7_OLD_SERVER_BIN: an older fn7-server binary"]
    async fn a_match_recorded_by_an_older_server_replays_unchanged() {
        let bin = std::env::var("FN7_OLD_SERVER_BIN").expect("FN7_OLD_SERVER_BIN");
        let dir = temp_dir();
        let port = free_port();
        let server = Server::start(&bin, &dir, port).await;
        let (a, p) = play(port, 12).await;
        assert!(p.changed.is_none() && p.events == 0, "the old server knows no looks");
        drop(a);
        drop(server);
        let rec = dir.join("data").join("recordings").join(format!("{}.jsonl", p.match_id));
        let text = std::fs::read_to_string(&rec).unwrap();
        let schema = text.lines().find(|l| l.starts_with("{\"type\":\"snapshot\"")).and_then(|l| serde_json::from_str::<Value>(l).ok()).map(|s| s["state"]["meta"]["schema"].clone());
        let armed = text.matches("is armed with").count();
        let rep = fn7_server::replay::verify(&rec, |_| {}).await.unwrap();
        eprintln!("old server: schema {schema:?}, {armed} armings in the recording; replay {} snapshots, {} identical, {:?}", rep.snapshots, rep.identical, rep.differences);
        assert!(armed > 0, "the recorded match armed its demigods");
        assert!(rep.snapshots >= 30 && rep.differences.is_empty(), "{:?}", rep.differences);
        let _ = std::fs::remove_dir_all(dir);
    }
}
