//! fn7-agents: AI players that play Final Night 7 through a real Theatre server over WebSocket, exactly like a client.
//! Each player has two hands. Every beat, at Compose, it sends `action.begin` (it started typing: the God of Time
//! interaction, which may slow time), writes its orders, and submits `action.submit { round, text, fields: { left,
//! right, zoom }, window_id? }` into the open slow window (its own, or a rival's: hidden meddling), or as a plain
//! Compose action when no window opened. God powers go separately as instant actions (`instant: true`). Every frame in both directions is recorded verbatim (ms timestamps) as `frame` records in a
//! client run (`<record dir>/<run>.jsonl`, the Theatre core's JSONL format, so `fn7-project` ingests it like a server
//! recording); with `--claude`, every LLM call the players make is recorded as `llm_call` / `http` records.
//!
//!   fn7-agents --url ws://127.0.0.1:8080/ws --players 4 --rounds 20 --beat-ms 6000 --wpm 150 --record runs/agents
//!   fn7-agents --claude ...                       # Claude writes each hand's lines (ANTHROPIC_API_KEY from env/.env)
//!   fn7-agents --resume runs/agents/<run>-players.json   # rejoin a running or restored match

use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use fn7_tools::players::{self, Claude, Script, Submission};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use theatre_trace::{self as trace, JsonlSink, Sink};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

#[derive(Parser, Clone)]
#[command(
    name = "fn7-agents",
    about = "AI players for Final Night 7 over WebSocket, every frame recorded"
)]
struct Args {
    #[arg(long, default_value = "ws://127.0.0.1:8080/ws")]
    url: String,
    #[arg(long, default_value_t = 4)]
    players: usize,
    /// Beats in the match (one Theatre round per beat).
    #[arg(long, default_value_t = 20)]
    rounds: u32,
    /// Beat length; Compose lasts this long.
    #[arg(long, default_value_t = 6000)]
    beat_ms: u64,
    /// Speaking speed of the scripted players (words per minute, split over both hands). Also the budget for --claude.
    #[arg(long, default_value_t = 150.0)]
    wpm: f64,
    /// Recording directory: the client run is written as `<dir>/<run>.jsonl` (the core's format; `fn7-project` ingests
    /// it), resume tokens as `<dir>/<run>-players.json`.
    #[arg(long, default_value = "runs/agents")]
    record: String,
    /// Seeds the phrase bank (player i uses seed + i) and is passed to the room.
    #[arg(long, default_value_t = 7)]
    seed: u64,
    /// Rejoin with a players file of resume tokens (`{ "players": [{ "name", "resume_token" }] }`).
    #[arg(long)]
    resume: Option<String>,
    /// Let Claude write each hand's lines from the brief and private payloads.
    #[arg(long)]
    claude: bool,
    /// Give up after this many seconds.
    #[arg(long, default_value_t = 3600)]
    timeout: u64,
}

const PLAYERS: &[(&str, &str)] = &[
    (
        "Rook",
        "a reckless hot-dropper who loves explosions and picking fights with gods",
    ),
    (
        "Juniper",
        "a sly trickster who recruits everyone and betrays them at the funniest moment",
    ),
    (
        "Quill",
        "a cautious schemer who works through subagents and possessed locals, never in person",
    ),
    (
        "Moth",
        "a sneaky survivor who hides, bargains with Time and lets debts kill everyone else",
    ),
    (
        "Brick",
        "a loyal brawler who protects his followers and hits hard",
    ),
    (
        "Vesper",
        "a zealot who invokes every god at once and pays whatever they ask",
    ),
];

fn persona_of(name: &str) -> &'static str {
    PLAYERS
        .iter()
        .find(|p| p.0 == name)
        .map(|p| p.1)
        .unwrap_or("a resourceful demigod")
}

/// Resume tokens collected from `auth.ok`, written next to the recording for `--resume`.
#[derive(Default)]
struct Tokens {
    path: String,
    players: Mutex<Vec<(String, String)>>,
}

impl Tokens {
    fn put(&self, name: &str, token: &str) {
        let mut p = self.players.lock().unwrap();
        match p.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = token.to_string(),
            None => p.push((name.to_string(), token.to_string())),
        }
        let v = json!({ "players": p.iter().map(|(n, t)| json!({ "name": n, "resume_token": t })).collect::<Vec<_>>() });
        if let Err(e) = std::fs::write(
            &self.path,
            serde_json::to_string_pretty(&v).unwrap_or_default(),
        ) {
            tracing::warn!(error = %e, path = %self.path, "could not write resume tokens");
        }
    }
}

struct Seat {
    name: String,
    persona: String,
    seat: Option<u64>,
    player_id: Option<String>,
    alive: bool,
    brief: Value,
    private: Value,
    private_state: Value,
    script: Script,
    last_round: Option<u32>,
    /// The open slow window `(window_id, by_seat)`, from `window.opened` / `window.closed`.
    window: Option<(u64, u64)>,
    /// Orders written but not yet sent (waiting to learn whether our begin opened a window).
    pending: Option<(u32, Submission)>,
    /// The request id of this beat's `action.begin` (its `ack` comes after any `window.opened` it caused).
    begin_id: Option<String>,
}

impl Seat {
    /// Seat status from a `Room` (room.snapshot / room.updated): eliminated or left means stop acting.
    fn read_room(&mut self, room: &Value) {
        if let Some(n) = room["you"]["seat"].as_u64() {
            self.seat = Some(n);
        }
        for s in room["seats"].as_array().into_iter().flatten() {
            let mine = match (&self.player_id, s["player_id"].as_str()) {
                (Some(me), Some(id)) => me == id,
                _ => self.seat.is_some() && s["seat"].as_u64() == self.seat,
            };
            if mine {
                self.seat = s["seat"].as_u64().or(self.seat);
                if matches!(s["status"].as_str(), Some("eliminated") | Some("left")) {
                    self.alive = false;
                }
            }
        }
    }
}

fn status_active(v: &Value) -> Option<bool> {
    v.as_str()
        .or_else(|| v["status"].as_str())
        .map(|s| s == "active")
}

async fn compose(args: &Args, s: &mut Seat, round: u32, claude: Option<&Claude>) -> Submission {
    let n = players::words_per_beat(args.wpm, args.beat_ms);
    let (l, r) = players::split_hands(n);
    if let Some(c) = claude {
        let user = players::hands_prompt(
            &s.name,
            &s.persona,
            round,
            (l, r),
            &s.brief,
            &s.private,
            &s.private_state,
        );
        match c.hands(&s.name, round, user, (l, r)).await {
            Ok((left, right)) if !(left.is_empty() && right.is_empty()) => {
                // The zoom field follows what the player asked for, else stays where it was.
                let mut sub = s.script.beat(0);
                sub.left = left;
                sub.right = right;
                let said = format!("{} {}", sub.left, sub.right).to_lowercase();
                if said.contains("zoom out") {
                    sub.zoom = "world".into();
                } else if said.contains("zoom into") || said.contains("zoom in") {
                    sub.zoom = if said.contains("district")
                        || said.contains("docks")
                        || said.contains("quarter")
                    {
                        "district".into()
                    } else {
                        "character".into()
                    };
                }
                return sub;
            }
            Ok(_) => trace::event("agent_llm_empty", Some(&s.name), json!({ "round": round })),
            Err(e) => trace::event(
                "agent_llm_error",
                Some(&s.name),
                json!({ "round": round, "error": format!("{e:#}") }),
            ),
        }
    }
    s.script.beat(n)
}

async fn submit(
    args: &Args,
    s: &mut Seat,
    round: u32,
    claude: Option<&Claude>,
    out: &mpsc::UnboundedSender<String>,
) {
    if !s.alive || s.last_round == Some(round) {
        return;
    }
    s.last_round = Some(round);
    // Text entry is the God of Time interaction: say "I started typing" first (it may slow time), then write.
    let bid = format!("{}-begin-{round}", s.name);
    let _ = out.send(players::action_begin(&bid));
    let sub = compose(args, s, round, claude).await;
    if let Some(c) = &sub.cast {
        // God powers are a separate path: an instant action, never carrying orders.
        let _ = out.send(players::cast_submit(round, c, Some(&format!("{}-cast-{round}", s.name))));
    }
    s.begin_id = Some(bid);
    s.pending = Some((round, sub));
    if let Some((w, _)) = s.window {
        flush(s, out, Some(w));
    }
}

/// Send the written orders: into slow window `window` (with its id), or as a Compose action.
fn flush(s: &mut Seat, out: &mpsc::UnboundedSender<String>, window: Option<u64>) {
    if let Some((round, sub)) = s.pending.take() {
        let id = format!("{}-orders-{round}", s.name);
        trace::event(
            "agent_orders",
            Some(&s.name),
            json!({ "round": round, "window_id": window, "words": sub.words() }),
        );
        let _ = out.send(players::action_submit(round, &sub, window, Some(&id)));
    }
}

#[allow(clippy::too_many_arguments)]
async fn play(
    args: Args,
    idx: usize,
    resume: Option<(String, String)>,
    claude: Option<Arc<Claude>>,
    tokens: Arc<Tokens>,
    code_tx: watch::Sender<Option<String>>,
    mut code_rx: watch::Receiver<Option<String>>,
) -> anyhow::Result<Value> {
    let name = match &resume {
        Some((n, _)) => n.clone(),
        None => PLAYERS[idx % PLAYERS.len()].0.to_string(),
    };
    let host = idx == 0 && resume.is_none();
    let (ws, _) = tokio_tungstenite::connect_async(&args.url).await?;
    let (mut sink, mut stream) = ws.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();

    // Writer: every frame we send is recorded, verbatim.
    let ctx = trace::current();
    let writer = tokio::spawn(async move {
        let fut = async {
            while let Some(raw) = out_rx.recv().await {
                trace::frame("sent", &raw);
                if sink.send(Message::Text(raw.into())).await.is_err() {
                    break;
                }
            }
        };
        match ctx {
            Some(c) => trace::within(c, fut).await,
            None => fut.await,
        }
    });
    // Keepalive: the server closes sockets that send nothing for 60 s.
    let ping_tx = out_tx.clone();
    let pinger = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            if ping_tx.send(players::ping()).is_err() {
                break;
            }
        }
    });

    let mut s = Seat {
        persona: persona_of(&name).to_string(),
        name: name.clone(),
        seat: None,
        player_id: None,
        alive: true,
        brief: Value::Null,
        private: Value::Null,
        private_state: Value::Null,
        script: Script::new(args.seed.wrapping_add(idx as u64)),
        last_round: None,
        window: None,
        pending: None,
        begin_id: None,
    };
    let claude = claude.as_deref();
    let mut started = false;
    let mut logged_in = false;
    let mut joined = false;
    let mut summary = Value::Null;
    let send = |raw: String| {
        let _ = out_tx.send(raw);
    };
    loop {
        let raw = tokio::select! {
            m = stream.next() => match m {
                Some(Ok(Message::Text(t))) => t.to_string(),
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => continue,
                Some(Err(e)) => { trace::event("agent_ws_error", Some(&name), json!({ "error": e.to_string() })); break }
            },
            _ = code_rx.changed(), if !host && resume.is_none() => {
                // Join only after auth.ok; until then the code waits.
                if logged_in && !joined {
                    if let Some(code) = code_rx.borrow().clone() {
                        joined = true;
                        send(players::room_join(&code));
                    }
                }
                continue;
            }
        };
        trace::frame("received", &raw);
        let v: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        let d = &v["d"];
        match v["t"].as_str().unwrap_or("") {
            "server.hello" => match &resume {
                Some((_, token)) => send(players::resume(token)),
                None => send(players::login(&name)),
            },
            "auth.ok" => {
                logged_in = true;
                s.player_id = d["player_id"].as_str().map(str::to_string);
                if let Some(t) = d["resume_token"].as_str() {
                    tokens.put(&name, t);
                }
                if resume.is_some() {
                    // The server follows with room.snapshot + match.snapshot.
                } else if host {
                    send(players::room_create(players::match_settings(
                        args.rounds,
                        args.beat_ms,
                        Some(args.seed),
                    )));
                } else if !joined {
                    if let Some(code) = code_rx.borrow().clone() {
                        joined = true;
                        send(players::room_join(&code));
                    }
                }
            }
            "room.snapshot" => {
                s.read_room(d);
                if host {
                    let _ = code_tx.send(d["code"].as_str().map(str::to_string));
                }
                if d["state"].as_str().is_none_or(|st| st == "open") {
                    send(players::room_ready());
                }
            }
            "room.updated" => {
                s.read_room(d);
                if host && !started {
                    let seats = d["seats"].as_array().cloned().unwrap_or_default();
                    let humans = seats
                        .iter()
                        .filter(|x| !x["is_bot"].as_bool().unwrap_or(false))
                        .count();
                    if humans >= args.players
                        && seats.iter().all(|x| {
                            x["ready"].as_bool().unwrap_or(false)
                                || x["is_bot"].as_bool().unwrap_or(false)
                        })
                    {
                        started = true;
                        send(players::room_start());
                    }
                }
            }
            "match.started" => {
                started = true;
                for st in d["seats"].as_array().into_iter().flatten() {
                    if s.player_id.is_some() && st["player_id"].as_str() == s.player_id.as_deref() {
                        s.seat = st["seat"].as_u64();
                    }
                }
            }
            "match.snapshot" => {
                // Rejoined a running match: take the state from the snapshot and act if an action is pending.
                let round = d["round"].as_u64().unwrap_or(0) as u32;
                let you = &d["you"];
                s.seat = you["seat"].as_u64().or(s.seat);
                if let Some(a) = status_active(&you["status"]) {
                    s.alive = a;
                }
                s.brief = json!({ "round": round, "public": d["brief"], "private": you["brief"] });
                s.private = you["outcome"].clone();
                s.private_state = you["private_state"].clone();
                if you["pending"].as_str() == Some("action") && you["action"].is_null() {
                    submit(&args, &mut s, round, claude, &out_tx).await;
                }
            }
            "round.brief" => {
                if d["private"]["alive"] == Value::Bool(false) {
                    s.alive = false;
                }
                s.brief = d.clone();
            }
            "round.private" => s.private = d["private"].clone(),
            "round.private_state" => {
                s.private_state = d["state"].clone();
                if s.private_state["you"]["alive"] == Value::Bool(false) {
                    s.alive = false;
                }
            }
            "seat.eliminated" => {
                if d["seat"].as_u64().is_some() && d["seat"].as_u64() == s.seat {
                    s.alive = false;
                    trace::event("agent_eliminated", Some(&name), d.clone());
                }
            }
            "phase.changed" => {
                if d["phase"].as_str() == Some("compose") {
                    let round = d["round"].as_u64().unwrap_or(0) as u32;
                    submit(&args, &mut s, round, claude, &out_tx).await;
                }
            }
            "window.opened" => {
                let (id, by) = (d["window_id"].as_u64().unwrap_or(0), d["by_seat"].as_u64().unwrap_or(u64::MAX));
                s.window = Some((id, by));
                // Our own window, or a rival's we can meddle in: the written orders go in now.
                if s.pending.is_some() {
                    flush(&mut s, &out_tx, Some(id));
                }
            }
            "window.closed" => {
                if s.window.map(|w| w.0) == d["window_id"].as_u64() {
                    s.window = None;
                }
            }
            "ack" => {
                // Our begin was acknowledged without a window (refused: no bolt, or none needed): plain Compose.
                if v["re"].as_str().is_some() && v["re"].as_str() == s.begin_id.as_deref() && s.pending.is_some() {
                    let w = s.window.map(|w| w.0);
                    flush(&mut s, &out_tx, w);
                }
            }
            "action.received" => {
                if d["accepted"] == Value::Bool(false) {
                    trace::event("agent_rejected", Some(&name), d.clone());
                }
            }
            "match.ended" => {
                summary = d.clone();
                break;
            }
            "error" => trace::event("agent_server_error", Some(&name), d.clone()),
            _ => {}
        }
    }
    pinger.abort();
    drop(out_tx);
    let _ = writer.await;
    Ok(summary)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let args = Args::parse();
    let claude = if args.claude {
        let c = Claude::from_env(Duration::from_secs(60))
            .ok_or_else(|| anyhow::anyhow!("--claude needs ANTHROPIC_API_KEY (env or .env)"))??;
        Some(Arc::new(c))
    } else {
        None
    };
    let resumes: Vec<(String, String)> = match &args.resume {
        Some(path) => {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
            v["players"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| {
                    Some((
                        p["name"].as_str()?.to_string(),
                        p["resume_token"].as_str()?.to_string(),
                    ))
                })
                .collect()
        }
        None => vec![],
    };
    if args.resume.is_some() && resumes.is_empty() {
        anyhow::bail!(
            "no players with resume tokens in {}",
            args.resume.as_deref().unwrap_or("")
        );
    }
    let count = if resumes.is_empty() {
        args.players
    } else {
        resumes.len()
    };
    let names: Vec<String> = (0..count)
        .map(|i| {
            resumes
                .get(i)
                .map(|r| r.0.clone())
                .unwrap_or_else(|| PLAYERS[i % PLAYERS.len()].0.to_string())
        })
        .collect();

    let sink = JsonlSink::new(&args.record)?;
    let run = format!("fn7-agents-{}", uuid::Uuid::new_v4());
    let rec_path = std::path::Path::new(&args.record).join(format!("{run}.jsonl"));
    sink.write(trace::Record::RunStart {
        run: run.clone(),
        plugin: "fn7-agents".into(),
        at: trace::now_ms(),
        settings: json!({ "url": args.url, "players": count, "rounds": args.rounds, "beat_ms": args.beat_ms, "wpm": args.wpm,
                          "words_per_beat": players::words_per_beat(args.wpm, args.beat_ms), "seed": args.seed, "claude": args.claude }),
        meta: json!({ "source": "agent client", "resume": args.resume, "model": claude.as_ref().map(|c| c.model.clone()),
                      "players": names.iter().map(|n| json!({ "name": n, "persona": persona_of(n) })).collect::<Vec<_>>() }),
    });
    let tokens = Arc::new(Tokens {
        path: std::path::Path::new(&args.record)
            .join(format!("{run}-players.json"))
            .display()
            .to_string(),
        ..Default::default()
    });
    let (code_tx, code_rx) = watch::channel(None::<String>);
    let mut tasks = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let mut ctx = trace::Ctx::new(sink.clone() as Arc<dyn Sink>, run.clone());
        ctx.actor = Some(name.clone());
        let (a, r, c, t, tx, rx) = (
            args.clone(),
            resumes.get(i).cloned(),
            claude.clone(),
            tokens.clone(),
            code_tx.clone(),
            code_rx.clone(),
        );
        tasks.push(tokio::spawn(trace::within(ctx, async move {
            play(a, i, r, c, t, tx, rx).await
        })));
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let results = match tokio::time::timeout(
        Duration::from_secs(args.timeout),
        futures::future::join_all(tasks),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => {
            eprintln!("timed out after {}s", args.timeout);
            vec![]
        }
    };
    let mut summary = Value::Null;
    for r in results {
        match r {
            Ok(Ok(s)) if !s.is_null() => summary = s,
            Ok(Err(e)) => eprintln!("agent error: {e:#}"),
            Err(e) => eprintln!("agent task failed: {e}"),
            _ => {}
        }
    }
    sink.write(trace::Record::RunEnd {
        run: run.clone(),
        at: trace::now_ms(),
        summary: summary.clone(),
    });
    sink.flush();
    println!(
        "agents run {run} recorded to {} (resume tokens: {})",
        rec_path.display(),
        tokens.path
    );
    if let Some(w) = summary["winners"].as_array() {
        println!(
            "winners (seats): {}",
            w.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}
