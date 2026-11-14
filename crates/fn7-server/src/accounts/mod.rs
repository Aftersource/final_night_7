//! Accounts in the server (docs/ACCOUNTS.md; the store is `fn7_profile::accounts`).
//!
//! - [`http`]: the routes (`/api/auth/*`, `/api/me*`, `/api/game/bind`), merged into the server's router in main.rs.
//! - [`google`]: Google sign-in (authorization code + PKCE, ID token verified against the JWKS).
//! - [`limiter`]: rate limits.
//! - [`plugin`]: [`plugin::AccountsPlugin`], the `GamePlugin` wrapper that gives a signed-in player's seat its custom
//!   demigod names and personas, and refuses a seat taken under an account's name by someone else.
//! - this file: [`Accounts`], which the routes, the wrapper and the name guard share.
//!
//! ## How a session becomes a player (no engine change)
//!
//! The engine's `auth.login` takes a display name and nothing else, and no plugin hook sees a socket. So the binding
//! is made **beside** the websocket, with a credential only that socket's owner holds:
//!
//! 1. the client logs in on the websocket with its account's display name → `auth.ok {player_id, resume_token}`;
//! 2. it calls `POST /api/game/bind {resume_token}` with its session (`Authorization: Bearer`). The engine's resume
//!    token is the engine's own proof of that player, so the server now knows *that engine player is this account*
//!    ([`Accounts::bind`]; the token is looked up, never stored);
//! 3. it joins a room and takes a seat as before.
//!
//! What that buys, checked where the server can check it:
//! - **Persona and custom names** only go to a seat whose engine player is bound to the account that holds the seat's
//!   name ([`Accounts::seat_auth`], asked by the wrapper at `on_match_start` and `on_seat_takeover`).
//! - **History** only accrues to an account-held name from a bound seat (`seats[].account` in the state; the store
//!   leaves any other seat under that name out).
//! - **The name itself**: a mid-match takeover under an account's name by an unbound player is refused (they hold the
//!   seat but get no demigod), and the **name guard** ([`Accounts::guard_tick`], every [`GUARD_EVERY`]) disconnects
//!   and forgets any engine player who carries an account's name without being bound: at once if they are in a room,
//!   after [`BIND_GRACE`] otherwise (the time a real client needs between `auth.ok` and the bind call).
//!
//! It is not airtight (docs/ENGINE_SYNC.md has the engine request that would make it so): between two guard ticks an
//! impostor can sit in a room under the name, chat, and in the house reclaim the name's away seat until the tick
//! removes them. `FN7_NAME_GUARD=off` turns the eviction and the takeover refusal off (personas and history still
//! need a bound seat).

pub mod google;
pub mod http;
pub mod limiter;
pub mod plugin;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use fn7_agents::moderate::{self, Judgement, Sheet};
use fn7_plugin::persona::{HandPersona, SeatPersona};
use fn7_profile::accounts::{Account, AccountError, DemigodDoc, NewSession};
use fn7_profile::{normalize_name, skeleton, ProfileStore};
use theatre_plugin_api::LlmProvider;
use uuid::Uuid;

use google::{GoogleConfig, KeyCache};
use limiter::Limiter;

/// How often the name guard looks at the engine's players.
pub const GUARD_EVERY: Duration = Duration::from_millis(250);
/// How long a player outside any room may carry an account's name before binding.
pub const BIND_GRACE: Duration = Duration::from_secs(10);
/// How long a Google sign-in may take from `start` to the callback.
pub const FLOW_TTL: Duration = Duration::from_secs(600);
/// Sign-ins in flight at once (the oldest are dropped past this).
pub const MAX_FLOWS: usize = 2000;

/// One engine player, as the directory reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct EnginePlayer {
    pub id: Uuid,
    pub display_name: String,
    pub room: Option<Uuid>,
}

/// What the accounts layer needs from the engine: its players, and a way to remove one. Implemented over the
/// engine's `AppState` ([`EngineDirectory`]); tests use a fake.
pub trait Directory: Send + Sync {
    fn players(&self) -> Vec<EnginePlayer>;
    /// The player a resume token belongs to.
    fn by_resume_token(&self, token: &str) -> Option<EnginePlayer>;
    /// The room running the match with this run id (the core's match id), when it is recorded.
    fn room_of_run(&self, run: &str) -> Option<Uuid>;
    /// Tell the player why, close its socket and forget it (its seat goes the way a timed-out player's does).
    fn evict(&self, player: Uuid, message: &str);
}

/// [`Directory`] over the engine's state. Reads `players`, `resume_index` and `rooms`; an eviction does what the
/// engine's own session sweep does for an expired player.
pub struct EngineDirectory(pub Arc<theatre_server::AppState>);

impl Directory for EngineDirectory {
    fn players(&self) -> Vec<EnginePlayer> {
        self.0
            .players
            .iter()
            .map(|p| EnginePlayer { id: p.id, display_name: p.display_name.clone(), room: p.room })
            .collect()
    }

    fn by_resume_token(&self, token: &str) -> Option<EnginePlayer> {
        let id = *self.0.resume_index.get(token)?;
        let p = self.0.players.get(&id)?;
        Some(EnginePlayer { id: p.id, display_name: p.display_name.clone(), room: p.room })
    }

    fn room_of_run(&self, run: &str) -> Option<Uuid> {
        self.0.rooms.iter().find(|r| r.run.read().as_deref() == Some(run)).map(|r| *r.key())
    }

    fn evict(&self, player: Uuid, message: &str) {
        let Some((_, p)) = self.0.players.remove(&player) else {
            return;
        };
        self.0.resume_index.remove(&p.resume_token);
        self.0.mark_sessions_dirty();
        if let Some(conn) = &p.conn {
            conn.send(theatre_protocol::Envelope::error(None, theatre_protocol::ErrorCode::Unauthorized, message));
            conn.close();
        }
        if let Some(tx) = p.room.and_then(|r| self.0.room_tx(r)) {
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.spawn(async move {
                    let _ = tx.send(theatre_server::room::RoomCmd::Expired { player_id: player }).await;
                });
            }
        }
    }
}

/// Who a seat's name belongs to, as far as the server can tell.
#[derive(Debug, Clone, PartialEq)]
pub enum SeatAuth {
    /// Nobody holds the name: a guest seat, as before accounts.
    Open,
    /// The name's account, and every engine player carrying the name (in this match's room) is bound to it.
    Account(String),
    /// An account holds the name and a player carrying it is not bound to that account.
    Impostor(String),
    /// An account holds the name but the engine's players cannot be seen (no directory: a simulated match).
    Unverified(String),
}

/// One Google sign-in in flight.
struct Flow {
    state_hash: String,
    nonce: String,
    verifier: String,
    device_label: String,
    preferred_name: Option<String>,
    /// Linking: the account to attach Google to, and the session that asked (revoked when the new one is delivered).
    link: Option<(String, String)>,
    created: Instant,
    status: FlowStatus,
}

enum FlowStatus {
    Pending,
    /// The callback is exchanging the code (a second callback with the same state is a replay).
    Working,
    Done { session: NewSession, account: Account, created: bool },
    Failed { code: String, message: String },
}

/// What a poll answers.
pub enum Poll {
    Pending,
    Complete { session: NewSession, account: Account, created: bool, linked: bool },
    Failed { code: String, message: String },
    Unknown,
}

fn hash_hex(domain: &str, s: &str) -> String {
    use ring::digest;
    let mut c = digest::Context::new(&digest::SHA256);
    c.update(domain.as_bytes());
    c.update(&[0]);
    c.update(s.as_bytes());
    c.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn random_token(prefix: &str) -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0u8; 32];
    SystemRandom::new().fill(&mut b).expect("the OS random source");
    format!("{prefix}{}", google::b64url(&b))
}

#[derive(Debug, Clone)]
pub struct AccountsConfig {
    /// Evict unbound players on an account's name and refuse their takeovers (`FN7_NAME_GUARD`, default on).
    pub guard: bool,
    /// How long the moderation model gets (`FN7_MODERATION_TIMEOUT_MS`, default 8000).
    pub moderation_timeout: Duration,
    /// Rate-limit scale (`FN7_AUTH_RATE`, default 1; 0 = off).
    pub rate_scale: f64,
    /// Names no player's demigod may take or resemble: the built-in AI demigods.
    pub reserved_names: Vec<String>,
    /// How long a player outside any room may carry an account's name before binding ([`BIND_GRACE`]).
    pub bind_grace: Duration,
}

impl Default for AccountsConfig {
    fn default() -> Self {
        Self {
            guard: true,
            moderation_timeout: moderate::TIMEOUT,
            rate_scale: 1.0,
            reserved_names: fn7_plugin::phases::AI_NAMES.iter().map(|n| n.to_string()).collect(),
            bind_grace: BIND_GRACE,
        }
    }
}

impl AccountsConfig {
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Self {
        let mut c = Self::default();
        if let Some(v) = env("FN7_NAME_GUARD") {
            c.guard = !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no");
        }
        if let Some(ms) = env("FN7_MODERATION_TIMEOUT_MS").and_then(|v| v.trim().parse::<u64>().ok()) {
            c.moderation_timeout = Duration::from_millis(ms.clamp(100, 60_000));
        }
        if let Some(s) = env("FN7_AUTH_RATE").and_then(|v| v.trim().parse::<f64>().ok()) {
            c.rate_scale = s.max(0.0);
        }
        c
    }
}

/// The accounts layer, shared behind an `Arc` by the routes, the plugin wrapper and the name guard.
pub struct Accounts {
    store: Arc<ProfileStore>,
    /// The moderation model's provider; `None` = rules only.
    llm: Option<Arc<dyn LlmProvider>>,
    google: Option<GoogleConfig>,
    keys: KeyCache,
    http: reqwest::Client,
    flows: Mutex<HashMap<String, Flow>>,
    pub(crate) limiter: Limiter,
    directory: OnceLock<Arc<dyn Directory>>,
    /// When the guard first saw each engine player.
    first_seen: Mutex<HashMap<Uuid, Instant>>,
    cfg: AccountsConfig,
}

impl Accounts {
    pub fn new(store: Arc<ProfileStore>, llm: Option<Arc<dyn LlmProvider>>, google: Option<GoogleConfig>, cfg: AccountsConfig) -> Arc<Self> {
        Arc::new(Self {
            store,
            llm,
            google,
            keys: KeyCache::new(),
            http: reqwest::Client::builder().timeout(Duration::from_secs(10)).build().expect("http client"),
            flows: Mutex::new(HashMap::new()),
            limiter: Limiter::new(cfg.rate_scale),
            directory: OnceLock::new(),
            first_seen: Mutex::new(HashMap::new()),
            cfg,
        })
    }

    /// From the server's environment: Google from `FN7_GOOGLE_*` / `FN7_PUBLIC_URL`; the moderation model is `llm`
    /// unless `FN7_MODERATION=rules`.
    pub fn from_env(store: Arc<ProfileStore>, llm: Option<Arc<dyn LlmProvider>>, env: impl Fn(&str) -> Option<String>) -> Arc<Self> {
        let rules_only = env("FN7_MODERATION").is_some_and(|v| v.trim().eq_ignore_ascii_case("rules"));
        Self::new(
            store,
            llm.filter(|_| !rules_only),
            GoogleConfig::from_env(&env),
            AccountsConfig::from_env(&env),
        )
    }

    pub fn store(&self) -> &Arc<ProfileStore> {
        &self.store
    }

    pub fn google_configured(&self) -> bool {
        self.google.is_some()
    }

    pub fn moderation_label(&self) -> &'static str {
        if self.llm.is_some() {
            moderate::MODEL
        } else {
            moderate::RULES_MODEL
        }
    }

    pub fn guard_on(&self) -> bool {
        self.cfg.guard
    }

    /// Give the layer the engine's players (once, after the engine's state exists).
    pub fn attach(&self, directory: Arc<dyn Directory>) {
        let _ = self.directory.set(directory);
    }

    // ---- Google sign-in -------------------------------------------------------------------------------------------

    /// Start a sign-in: `(login_url, poll_token, expires_at_ms)`, or `None` when Google is not configured.
    /// `link` = `(account id, session id)` attaches Google to that account instead of signing in.
    pub fn google_start(&self, device_label: &str, preferred_name: Option<String>, link: Option<(String, String)>) -> Option<(String, String, i64)> {
        let cfg = self.google.as_ref()?;
        let poll_token = random_token("fn7p_");
        let state = random_token("");
        let nonce = random_token("");
        let verifier = random_token("");
        let url = cfg.login_url(&state, &nonce, &google::pkce_challenge(&verifier));
        let mut flows = self.flows.lock().unwrap_or_else(|p| p.into_inner());
        flows.retain(|_, f| f.created.elapsed() < FLOW_TTL);
        if flows.len() >= MAX_FLOWS {
            if let Some(oldest) = flows.iter().min_by_key(|(_, f)| f.created).map(|(k, _)| k.clone()) {
                flows.remove(&oldest);
            }
        }
        flows.insert(
            hash_hex("fn7-poll", &poll_token),
            Flow {
                state_hash: hash_hex("fn7-state", &state),
                nonce,
                verifier,
                device_label: device_label.to_string(),
                preferred_name,
                link,
                created: Instant::now(),
                status: FlowStatus::Pending,
            },
        );
        let expires = theatre_trace::now_ms() + FLOW_TTL.as_millis() as i64;
        Some((url, poll_token, expires))
    }

    /// The browser came back from Google. `Ok` = signed in (the poll now completes); `Err((code, message))` otherwise.
    /// A `state` is good for one callback: a second one (a replay) is refused and changes nothing.
    pub async fn google_callback(&self, state: &str, code: Option<&str>, provider_error: Option<&str>) -> Result<(), (String, String)> {
        let Some(cfg) = self.google.clone() else {
            return Err(("google_not_configured".into(), "google sign-in is not configured".into()));
        };
        let state_hash = hash_hex("fn7-state", state);
        // Claim the flow: only a pending one, and only once.
        let (key, nonce, verifier, device_label, preferred, link) = {
            let mut flows = self.flows.lock().unwrap_or_else(|p| p.into_inner());
            flows.retain(|_, f| f.created.elapsed() < FLOW_TTL);
            let Some((key, f)) = flows.iter_mut().find(|(_, f)| f.state_hash == state_hash) else {
                return Err(("unknown_state".into(), "this sign-in link is unknown or has expired: start again from the game".into()));
            };
            if !matches!(f.status, FlowStatus::Pending) {
                return Err(("state_already_used".into(), "this sign-in link was already used: start again from the game".into()));
            }
            f.status = FlowStatus::Working;
            (key.clone(), f.nonce.clone(), f.verifier.clone(), f.device_label.clone(), f.preferred_name.clone(), f.link.clone())
        };
        let outcome = async {
            if let Some(e) = provider_error {
                let e: String = e.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(40).collect();
                return Err(("provider_refused".to_string(), format!("Google did not sign you in ({e})")));
            }
            let code = code.filter(|c| !c.is_empty()).ok_or(("bad_request".to_string(), "no authorization code".to_string()))?;
            let fail = |e: google::OidcError| {
                // The reason (never a token) goes to the log; the client sees the code.
                tracing::warn!(error = %e, "accounts: google sign-in failed");
                (e.code().to_string(), "Google sign-in could not be verified".to_string())
            };
            let id_token = google::exchange_code(&self.http, &cfg, code, &verifier).await.map_err(fail)?;
            let keys = self.keys.keys_for(&self.http, &cfg, google::token_kid(&id_token).as_deref()).await.map_err(fail)?;
            let claims = google::verify_id_token(&id_token, &keys, &cfg, &nonce, theatre_trace::now_ms() / 1000).map_err(fail)?;
            let names: Vec<String> = [preferred, claims.given_name.clone(), claims.name.clone()].into_iter().flatten().collect();
            let store = self.store.clone();
            let link_to = link.as_ref().map(|(a, _)| a.clone());
            tokio::task::spawn_blocking(move || {
                store.google_sign_in(&claims.sub, claims.email.as_deref(), claims.email_verified, &names, &device_label, link_to.as_deref())
            })
            .await
            .map_err(|e| ("internal".to_string(), e.to_string()))?
            .map_err(|e| (e.code().to_string(), e.to_string()))
        }
        .await;
        let mut flows = self.flows.lock().unwrap_or_else(|p| p.into_inner());
        let Some(f) = flows.get_mut(&key) else {
            // The flow expired while we worked: a session nobody can collect must not stay live.
            if let Ok((_, session, _)) = &outcome {
                let _ = self.store.revoke_session(&session.info.id);
            }
            return Err(("unknown_state".into(), "this sign-in took too long: start again from the game".into()));
        };
        match outcome {
            Ok((account, session, created)) => {
                tracing::info!(account = %account.id, created, linked = f.link.is_some(), "accounts: google sign-in");
                f.status = FlowStatus::Done { session, account, created };
                Ok(())
            }
            Err((code, message)) => {
                f.status = FlowStatus::Failed { code: code.clone(), message: message.clone() };
                Err((code, message))
            }
        }
    }

    /// The client asks whether its sign-in finished. A finished flow (done or failed) is answered once and forgotten.
    pub fn poll(&self, poll_token: &str) -> Poll {
        let key = hash_hex("fn7-poll", poll_token);
        let mut flows = self.flows.lock().unwrap_or_else(|p| p.into_inner());
        // An expired flow that finished unseen: its session is revoked, never delivered late.
        let stale: Vec<String> = flows.iter().filter(|(_, f)| f.created.elapsed() >= FLOW_TTL).map(|(k, _)| k.clone()).collect();
        for k in stale {
            if let Some(Flow { status: FlowStatus::Done { session, .. }, .. }) = flows.remove(&k) {
                let _ = self.store.revoke_session(&session.info.id);
            }
        }
        match flows.get(&key).map(|f| &f.status) {
            None => Poll::Unknown,
            Some(FlowStatus::Pending | FlowStatus::Working) => Poll::Pending,
            Some(_) => {
                let f = flows.remove(&key).expect("present");
                match f.status {
                    FlowStatus::Done { session, account, created } => {
                        // A link flow hands over a fresh session: the one that asked is retired.
                        if let Some((_, old_session)) = &f.link {
                            let _ = self.store.revoke_session(old_session);
                        }
                        Poll::Complete { session, account, created, linked: f.link.is_some() }
                    }
                    FlowStatus::Failed { code, message } => Poll::Failed { code, message },
                    _ => Poll::Pending,
                }
            }
        }
    }

    // ---- demigod sheets -------------------------------------------------------------------------------------------

    /// Why `name` cannot be a custom demigod name for `account`'s `hand`, if it cannot (the lookalike check: the
    /// built-in demigods and their hands, other players' names and demigods, the account's other demigod, and any
    /// `"…'s Left Hand"` / `"…'s Right Hand"` that is not the account's own default).
    pub fn demigod_name_conflict(&self, account: &Account, hand: &str, name: &str) -> Result<Option<String>, AccountError> {
        let Some(key) = normalize_name(name) else {
            return Ok(Some("the name has no usable characters".into()));
        };
        let skel = skeleton(&key);
        for reserved in &self.cfg.reserved_names {
            let [l, r] = fn7_plugin::payload::hand_names(reserved);
            for n in [reserved.clone(), l, r] {
                if normalize_name(&n).is_some_and(|k| skeleton(&k) == skel) {
                    return Ok(Some(format!("{n} is one of the game's own demigods")));
                }
            }
        }
        for side in ["left hand", "right hand"] {
            if let Some(owner) = key.strip_suffix(side).map(|p| p.trim_end().trim_end_matches("'s").trim_end_matches("’s").trim()) {
                if !owner.is_empty() && normalize_name(&account.display_name).as_deref() != Some(owner) {
                    return Ok(Some("that reads like another player's default demigod name".into()));
                }
            }
        }
        self.store.demigod_name_conflict(&account.id, hand, name)
    }

    /// Run the moderation pass over a sheet (one LLM call, or the rules).
    pub async fn moderate(&self, sheet: &Sheet) -> Judgement {
        moderate::run(self.llm.as_deref(), sheet, self.cfg.moderation_timeout).await
    }

    /// What `account` brings to `seat`: its two stored demigods (their final text) and its pronouns.
    pub fn seat_persona(&self, account: &str, seat: u8) -> SeatPersona {
        let hand = |h: &str| -> HandPersona {
            match self.store.demigod(account, h) {
                Ok(Some(row)) => {
                    let DemigodDoc { name, personality, style, creed } = row.r#final;
                    HandPersona { name, personality, style, creed }
                }
                _ => HandPersona::default(),
            }
        };
        let pronouns = self
            .store
            .settings_doc(account)
            .ok()
            .flatten()
            .and_then(|(_, d)| d["pronouns"].as_str().map(str::to_string))
            .filter(|p| p != "unspecified" && fn7_profile::settings::PRONOUNS.contains(&p.as_str()))
            .unwrap_or_default();
        SeatPersona { seat, account: account.to_string(), pronouns, left: hand("left"), right: hand("right") }
    }

    // ---- the game connection --------------------------------------------------------------------------------------

    /// Bind the engine player that owns `resume_token` to `account` (through `session_id`).
    /// `Err("not_found")`: no such engine player (or no engine attached); `Err("name_mismatch")`: the player logged
    /// in under a name that is not the account's.
    pub fn bind(&self, account: &Account, session_id: &str, resume_token: &str) -> Result<EnginePlayer, &'static str> {
        let dir = self.directory.get().ok_or("not_found")?;
        let player = dir.by_resume_token(resume_token).ok_or("not_found")?;
        let held = normalize_name(&player.display_name);
        if held.is_none() || (held.as_deref() != Some(account.profile.as_str())) {
            return Err("name_mismatch");
        }
        self.store.bind_player(&player.id.to_string(), &account.id, session_id).map_err(|_| "store")?;
        tracing::info!(player = %player.id, account = %account.id, "accounts: engine player bound");
        Ok(player)
    }

    fn bound_to(&self, player: Uuid) -> Option<String> {
        self.store.binding(&player.to_string()).ok().flatten()
    }

    /// Who the seat named `display_name` belongs to, in the match with run id `run` (see [`SeatAuth`]).
    pub fn seat_auth(&self, display_name: &str, run: Option<&str>) -> SeatAuth {
        let Ok(Some(holder)) = self.store.name_holder(display_name) else {
            return SeatAuth::Open;
        };
        let Some(dir) = self.directory.get() else {
            return SeatAuth::Unverified(holder.account_id);
        };
        let key = normalize_name(display_name);
        let room = run.and_then(|r| dir.room_of_run(r));
        let named: Vec<EnginePlayer> = dir
            .players()
            .into_iter()
            .filter(|p| normalize_name(&p.display_name) == key)
            .filter(|p| room.is_none() || p.room == room)
            .collect();
        if named.is_empty() {
            // Nobody in the room carries the name any more (they left between the join and this hook).
            return SeatAuth::Unverified(holder.account_id);
        }
        if named.iter().all(|p| self.bound_to(p.id).as_deref() == Some(holder.account_id.as_str())) {
            SeatAuth::Account(holder.account_id)
        } else {
            SeatAuth::Impostor(holder.account_id)
        }
    }

    /// One pass of the name guard: evict every engine player who carries an account's name without being bound to
    /// that account (see the module doc for when). Returns the players evicted.
    pub fn guard_tick(&self) -> Vec<Uuid> {
        let Some(dir) = self.directory.get() else {
            return vec![];
        };
        let players = dir.players();
        let now = Instant::now();
        let mut seen = self.first_seen.lock().unwrap_or_else(|p| p.into_inner());
        seen.retain(|id, _| players.iter().any(|p| p.id == *id));
        let mut holders: HashMap<String, Option<fn7_profile::accounts::NameHolder>> = HashMap::new();
        let mut evicted = vec![];
        for p in &players {
            // Every player is timed from the first tick that saw it, account name or not.
            let first = *seen.entry(p.id).or_insert(now);
            let Some(key) = normalize_name(&p.display_name) else { continue };
            let holder = holders.entry(key).or_insert_with(|| self.store.name_holder(&p.display_name).ok().flatten());
            let Some(holder) = holder.as_ref() else { continue };
            if self.bound_to(p.id).as_deref() == Some(holder.account_id.as_str()) {
                continue;
            }
            // Someone already connected under the name when an account took it keeps it until they leave.
            let first_ms = theatre_trace::now_ms() - now.saturating_duration_since(first).as_millis() as i64;
            if holder.since > first_ms {
                continue;
            }
            if !self.cfg.guard {
                continue;
            }
            if p.room.is_some() || now.saturating_duration_since(first) >= self.cfg.bind_grace {
                dir.evict(
                    p.id,
                    "name_reserved: this name belongs to an account. Sign in and bind this connection (POST /api/game/bind) right after auth.ok, or pick another name.",
                );
                tracing::info!(player = %p.id, in_room = p.room.is_some(), "accounts: unbound player on an account's name removed");
                seen.remove(&p.id);
                evicted.push(p.id);
            }
        }
        evicted
    }

    /// Run the name guard until the server stops, and drop bindings of players that are gone (every minute).
    pub fn spawn_guard(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(GUARD_EVERY);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut n: u64 = 0;
            loop {
                tick.tick().await;
                n += 1;
                let me2 = me.clone();
                let prune = n % 240 == 0;
                let _ = tokio::task::spawn_blocking(move || {
                    me2.guard_tick();
                    if prune {
                        if let Some(dir) = me2.directory.get() {
                            let live: Vec<String> = dir.players().iter().map(|p| p.id.to_string()).collect();
                            let _ = me2.store.prune_bindings(&live);
                        }
                    }
                })
                .await;
            }
        })
    }
}
