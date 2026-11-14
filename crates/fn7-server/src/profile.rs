//! Persistent profiles in the server (`fn7-profile`; design: `docs/DEMIGODS_AND_ITEMS.md` §6.3, §7–§11).
//!
//! [`Profiles`] is what the server's plugin wrapper ([`crate::media::MediaPlugin`]) holds:
//! - **Queries** `game.profile`, `game.history`, `game.library` ([`Profiles::answer`]): not game state, so the
//!   plugin never sees them; the wrapper answers them from the store, the way it answers `game.media`. Same JSON as
//!   the HTTP routes (`fn7_profile::http`, merged into the server's router in main.rs).
//! - **Match end** ([`Profiles::record_match_end`]): called from the wrapper's `summarize`, after the media layer has
//!   annotated the summary, so image hashes are present. The core's recording of the match (flushed first) gives the
//!   room code, start time, rules version, seats and every elimination; the summary and the final public view come
//!   from the hook. One history row per human seat, idempotent per match id, keyed by the core's match id (the
//!   recording's run id), which is what `/admin/matches/{id}/state` and `/admin/fork` take. A store error is logged
//!   and never fails the match.
//! - **Library registration** ([`Profiles::register_when_ready`]): when `game.media` asks for a portrait or diorama
//!   of a player's demigod (`dg:`/`hd:`), the image is added to that player's library once the media layer marks it
//!   ready (a cached image at once; a pending one by a watcher that polls the media entry; a refused or failed one
//!   never). Images in payloads (portraits, dioramas, statues, items, storyboard) land at match end.
//! - **Backfill** ([`backfill`]): at startup, when the store is empty or `FN7_PROFILE_BACKFILL=1`, every recording
//!   in the recordings directory is streamed in (`ProfileStore::backfill_dir`; matches already stored as ended are
//!   skipped unopened).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use fn7_media::{Media, Status};
use fn7_profile::{ingest, MatchInput, NewImage, ProfileError, ProfileStore, RecordReport, SeatIn, Source};
use serde_json::{json, Value};
use theatre_plugin_api::{QuerySpec, Seat};
use theatre_trace::JsonlSink;

/// The query names the wrapper answers from the store.
pub const QUERIES: [&str; 3] = ["profile", "history", "library"];

/// How long a library watcher waits for a pending image before giving up.
const WATCH_FOR: Duration = Duration::from_secs(15 * 60);

/// The profile store plus what the server attaches after it starts (the recorder). Shared by the plugin wrapper and
/// main.rs behind an `Arc`.
pub struct Profiles {
    store: Arc<ProfileStore>,
    recordings: OnceLock<Recordings>,
    /// `(profile, hash, match)` keys with a watcher running or done (one watcher per image per player per match).
    watching: Mutex<HashSet<String>>,
}

struct Recordings {
    dir: PathBuf,
    sink: Option<Arc<JsonlSink>>,
}

impl Profiles {
    pub fn new(store: Arc<ProfileStore>) -> Arc<Self> {
        Arc::new(Self {
            store,
            recordings: OnceLock::new(),
            watching: Mutex::new(HashSet::new()),
        })
    }

    pub fn store(&self) -> &Arc<ProfileStore> {
        &self.store
    }

    /// Where the core writes `<match_id>.jsonl`, and its sink (flushed before a match-end read). Set once, after the
    /// core's `AppState` exists; until then (or with recording off) the match-end row is built from the hook alone.
    pub fn attach_recordings(&self, dir: PathBuf, sink: Option<Arc<JsonlSink>>) {
        let _ = self.recordings.set(Recordings { dir, sink });
    }

    /// The three query specs (public: a spectator or an HTTP asker names the profile).
    pub fn queries() -> Vec<QuerySpec> {
        let name = json!({ "type": "string", "maxLength": 80,
                           "description": "A display name (any spelling: it is normalized). Default: the asking seat's name." });
        let page = json!({ "type": "integer", "minimum": 0, "description": "0-based page (default 0)." });
        let per = json!({ "type": "integer", "minimum": 1, "maximum": fn7_profile::MAX_PER_PAGE,
                          "description": "Page size (default 20, at most 100)." });
        let spec = |n: &str, d: &str, props: Value| QuerySpec {
            name: n.into(),
            description: d.into(),
            args_schema: json!({ "type": "object", "properties": props, "additionalProperties": false }),
            public: true,
        };
        vec![
            spec("profile",
                 "A player's persistent profile across matches (keyed by normalized display name): description, totals, demigod identities, lookalike names. Same JSON as GET /api/profiles/{name}. An error if the name has no profile yet.",
                 json!({ "name": name })),
            spec("history",
                 "A player's match history, newest first: placement, champion, demigods, notable items, deeds, eliminations and time-travel paths. Same JSON as GET /api/profiles/{name}/history.",
                 json!({ "name": name, "page": page, "per_page": per })),
            spec("library",
                 "A player's image library (portraits, dioramas, statues, moment cards, items), most recently seen first; fetch each by hash from GET /media/index. Same JSON as GET /api/profiles/{name}/library.",
                 json!({ "name": name, "page": page, "per_page": per,
                         "kind": { "type": "string", "description": "Only this kind (portrait, diorama, statue, event, item, army, place)." } })),
        ]
    }

    /// Answer `game.profile` / `game.history` / `game.library`. `name` defaults to the asking seat's display name.
    pub fn answer(&self, state: &Value, seat: Option<Seat>, query: &str, args: &Value) -> Result<Value, String> {
        let name = match args.get("name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
            Some(n) => n.chars().take(80).collect::<String>(),
            None => seat
                .and_then(|s| seats_from_state(state).into_iter().find(|x| x.seat == s))
                .map(|s| s.display_name)
                .filter(|n| !n.is_empty())
                .ok_or("`name` is required (no seat to default to)")?,
        };
        // Over HTTP every arg is a string (`?page=1`): accept numbers and numeric strings.
        let num = |k: &str| {
            let v = args.get(k)?;
            v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).map(|n: u64| n.min(u32::MAX as u64) as u32)
        };
        let (page, per) = (num("page").unwrap_or(0), num("per_page"));
        let e = |e: ProfileError| match e {
            ProfileError::BadName => format!("bad_name: {name:?} is not a usable name"),
            ProfileError::NotFound => format!("not_found: no profile for {name:?} yet"),
            other => format!("store: {other}"),
        };
        let v = match query {
            "profile" => serde_json::to_value(self.store.profile(&name).map_err(e)?.ok_or_else(|| e(ProfileError::NotFound))?),
            "history" => serde_json::to_value(self.store.history(&name, page, per).map_err(e)?),
            "library" => {
                let kind = args.get("kind").and_then(Value::as_str).map(str::trim).filter(|k| !k.is_empty());
                serde_json::to_value(self.store.library(&name, page, per, kind).map_err(e)?)
            }
            other => return Err(format!("unknown profile query {other}")),
        };
        v.map_err(|x| format!("store: {x}"))
    }

    /// Write the history rows of a match that just ended (blocking: call it on `spawn_blocking`). `match_id` is the
    /// core's run id (`None` with recording off: a local id, so the row exists but has no recording to time-travel).
    /// `seats` are the state's seats, used when the recording is missing. Returns what was written.
    pub fn record_match_end(
        &self,
        match_id: Option<String>,
        seats: Vec<SeatIn>,
        summary: Value,
        public_state: Value,
    ) -> Result<RecordReport, String> {
        let id = match_id.unwrap_or_else(|| format!("local-{}", uuid::Uuid::new_v4()));
        let input = self.live_input(&id, seats, summary, public_state);
        self.store.record_match(&input).map_err(|e| e.to_string())
    }

    fn live_input(&self, id: &str, seats: Vec<SeatIn>, summary: Value, public_state: Value) -> MatchInput {
        let now = theatre_trace::now_ms();
        let from_recording = self.recordings.get().and_then(|r| {
            if let Some(s) = &r.sink {
                s.flush();
            }
            let path = r.dir.join(format!("{}.jsonl", sanitize(id)));
            match ingest::read_recording(&path) {
                Ok((m, _)) => m,
                Err(e) => {
                    if path.exists() {
                        tracing::warn!(match_id = %id, error = %e, "profiles: could not read the match's recording");
                    }
                    None
                }
            }
        });
        let ended = json!({ "match_id": id, "summary": summary });
        let live = MatchInput::from_match_ended(seats, &ended, Some(now)).expect("match_id is set");
        let mut m = match from_recording {
            Some(mut r) => {
                r.summary = live.summary.clone();
                r.ended_at = Some(now);
                r.rounds = live.rounds.or(r.rounds);
                r.beats_total = r.beats_total.or(live.beats_total);
                if r.seats.is_empty() {
                    r.seats = live.seats.clone();
                }
                r
            }
            None => live,
        };
        m.match_id = id.to_string();
        // The final public view as clients got it (annotated): portraits, dioramas, visual sheets.
        m.public_state = Some(public_state);
        m.source = Source::Live;
        m
    }

    /// Add `img` to `profile`'s library once the media layer marks it ready: at once when it is ready (cached), via a
    /// watcher when it is pending, never when it is off, refused or failed. One watcher per (profile, hash, match).
    pub fn register_when_ready(self: &Arc<Self>, media: &Media, profile: String, img: NewImage) {
        let key = format!("{}\u{1f}{}\u{1f}{}", profile, img.hash, img.match_id.as_deref().unwrap_or("-"));
        {
            let mut w = self.watching.lock().unwrap_or_else(|p| p.into_inner());
            if !w.insert(key) {
                return;
            }
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            // No runtime (a plain test): only an image that is ready now.
            if media.entry(&img.hash).is_some_and(|e| e.status == Status::Ready) {
                self.add(&profile, &img);
            }
            return;
        };
        let (me, media) = (self.clone(), media.clone());
        rt.spawn(async move {
            let t0 = tokio::time::Instant::now();
            loop {
                match media.entry(&img.hash).map(|e| e.status) {
                    Some(Status::Ready) => {
                        let _ = tokio::task::spawn_blocking(move || me.add(&profile, &img)).await;
                        return;
                    }
                    Some(Status::Pending) if t0.elapsed() < WATCH_FOR => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    _ => return,
                }
            }
        });
    }

    fn add(&self, profile: &str, img: &NewImage) {
        if let Err(e) = self.store.add_image(profile, img) {
            tracing::warn!(profile, hash = %img.hash, error = %e, "profiles: library add failed");
        }
    }
}

/// The run id as the core's recorder names its file.
fn sanitize(run: &str) -> String {
    run.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// The state's seats as the store wants them: `seats[]` (`seat`, `is_bot`, and `display_name` from schema 5), with
/// the name falling back to the seat's demigod's name (before schema 5 the plugin names a seat's demigod after the
/// player).
pub fn seats_from_state(state: &Value) -> Vec<SeatIn> {
    let empty = vec![];
    let dgs = state["demigods"].as_array().unwrap_or(&empty);
    state["seats"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|s| {
            let seat = s["seat"].as_u64()? as u8;
            let name = s["display_name"]
                .as_str()
                .filter(|n| !n.trim().is_empty())
                .map(str::to_string)
                .or_else(|| {
                    let dg = s["demigod"].as_u64();
                    dgs.iter()
                        .find(|d| d["id"].as_u64() == dg && dg.is_some())
                        .or_else(|| dgs.iter().find(|d| d["seat"].as_u64() == Some(seat as u64)))
                        .and_then(|d| d["name"].as_str())
                        .map(str::to_string)
                })
                .unwrap_or_default();
            Some(SeatIn {
                seat,
                display_name: name,
                is_bot: s["is_bot"].as_bool().unwrap_or(false),
                account: s["account"].as_str().map(str::to_string),
            })
        })
        .collect()
}

/// The human player who owns a demigod ref (`dg:N`, or a hand demigod `hd:N` through its `demigod`), with the
/// subject's name: `(player display name, subject name)`. `None` for AI demigods, bots, and anything else.
pub fn owner_of_ref(state: &Value, r: &str) -> Option<(String, String)> {
    let (tag, n) = r.split_once(':')?;
    let n: u64 = n.parse().ok()?;
    let empty = vec![];
    let dgs = state["demigods"].as_array().unwrap_or(&empty);
    let find_dg = |id: u64| dgs.iter().find(|d| d["id"].as_u64() == Some(id));
    let (dg, subject) = match tag {
        "dg" => {
            let d = find_dg(n)?;
            (d, d["name"].as_str()?.to_string())
        }
        "hd" => {
            let h = state["hand_demigods"].as_array()?.iter().find(|h| h["id"].as_u64() == Some(n))?;
            let owner = h["demigod"].as_u64().or_else(|| {
                h["demigod"].as_str().and_then(|s| s.strip_prefix("dg:")).and_then(|x| x.parse().ok())
            })?;
            (find_dg(owner)?, h["name"].as_str()?.to_string())
        }
        _ => return None,
    };
    let seat = dg["seat"].as_u64()? as u8;
    let s = seats_from_state(state).into_iter().find(|s| s.seat == seat)?;
    (!s.is_bot && !s.display_name.is_empty()).then_some((s.display_name, subject))
}

/// The store path (`FN7_PROFILE_DB`, else `<data_dir>/fn7/profiles.sqlite`) opened and migrated.
pub fn open(data_dir: &Path, env: impl Fn(&str) -> Option<String>) -> anyhow::Result<(Arc<ProfileStore>, PathBuf)> {
    let path = fn7_profile::default_path(data_dir, env);
    let store = ProfileStore::open(&path).map_err(|e| anyhow::anyhow!("profile store {}: {e}", path.display()))?;
    Ok((Arc::new(store), path))
}

/// Whether to backfill at startup: the store is empty, or `FN7_PROFILE_BACKFILL=1|true|on|yes` (`0|off` never).
pub fn should_backfill(store: &ProfileStore, env: impl Fn(&str) -> Option<String>) -> bool {
    match env("FN7_PROFILE_BACKFILL").map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if matches!(v.as_str(), "1" | "true" | "on" | "yes") => true,
        Some(v) if matches!(v.as_str(), "0" | "false" | "off" | "no") => false,
        _ => store.match_count().map(|n| n == 0).unwrap_or(false),
    }
}

/// Stream every recording in `dir` into the store (skipping matches already stored as ended). Blocking.
pub fn backfill(store: &ProfileStore, dir: &Path) -> anyhow::Result<fn7_profile::BackfillReport> {
    if !dir.is_dir() {
        return Ok(Default::default());
    }
    store.backfill_dir(dir, true)
}
