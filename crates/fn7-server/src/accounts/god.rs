//! The god form (docs/ACCOUNTS.md § 14): each player's god sits at their seat as a stone statue, and the player
//! chooses how it looks.
//!
//! | Route | Auth | Answer |
//! |---|---|---|
//! | `GET /api/me/god` | session | the god form, its newest preview, its portrait (the accepted preview), its statues, the limits and the presets |
//! | `PUT /api/me/god` `{description, stone, pose}` | session | the moderation answer (as the demigod sheets: `ok` / `rephrase` 200, `refuse` 422) |
//! | `POST /api/me/god/preview` | session | `{status: pending\|ready, hash, url, prompt, charged, previews_left_today}` |
//! | `POST /api/me/god/accept` `{hash?}` | session | `{accepted: {...}, statue: "not_enabled" \| {...}}` |
//!
//! and, for every client at the table, `GET /api/assets/figures?name=<display name>` answers `god` ([`GodForm`]
//! implements [`fn7_assets::http::Gods`]).
//!
//! - **The words are the player's**: the description (≤ 600 characters) and a stone in their own words (≤ 60) go
//!   through the same one moderation pass as the demigod sheets ([`Accounts::moderate_god`]): verbatim unless they
//!   break a content rule.
//! - **The preview** is ONE reference image through fn7-media (the house's marble-statue prompt, as in the Unreal
//!   repo's `figures/*_god/provenance.json`, with the player's stone, pose and words: [`god_prompt`]). fn7-media keys
//!   it by the prompt, so the same (description, stone, pose) is the same picture and asking again is free. New
//!   pictures are capped per account per day (`god_preview_per_day`) and spend from the global image budget. The
//!   image is claimed in the asset registry first (`image/god_preview`, subject `god:<display name>`, the account in
//!   `meta.account` and `requested_account`), so the media layer's live rows land on that row with the exact prompt
//!   and cost.
//! - **Accepting** makes the preview the account's god portrait at once. A Meshy statue is queued only when
//!   `god_statue_meshy` is on (default OFF: credits are real money), at most `god_statues_per_account`; the finished
//!   job goes into the registry through the job-record path (`fn7_assets::provenance::import_jobs`).
//!
//! Nothing here touches a match: no state, no payload, no recording changes.

#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fn7_agents::moderate::{self, GodText, Verdict};
use fn7_assets::live::{Asker, LiveHook};
use fn7_assets::query::AccountGod;
use fn7_assets::{asset_id_for_media, AssetIn, Registry, Subject};
use fn7_media::{ImageKind, ImagePrompt, Media};
use fn7_profile::accounts::{Account, AccountError};
use fn7_profile::gods::{GodDoc, GodPreview, GodRow, GodStatue};
use serde::Deserialize;
use serde_json::{json, Value};
use theatre_server::axum::body::Bytes;
use theatre_server::axum::extract::State;
use theatre_server::axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use theatre_server::axum::response::{IntoResponse, Response};
use theatre_server::axum::routing::{get, post};
use theatre_server::axum::{Json, Router};

use super::http::{account_error, authed, authed_limited, blocking, check_text, fail, finish, limited, ok, parse, SMALL_BODY};
use super::limiter::Class;
use super::Accounts;

/// The stones: `(id, label, what it is carved from, how it reads)`. `white_marble` is the default.
pub const STONES: [(&str, &str, &str, &str); 7] = [
    ("white_marble", "White marble", "white marble with soft grey veining", "no colour, no paint, only pale marble"),
    ("black_onyx", "Black onyx", "polished black onyx with faint pale banding", "no paint, only glossy black stone"),
    ("granite", "Granite", "speckled grey granite", "no paint, only rough speckled granite"),
    ("sandstone", "Sandstone", "warm weathered sandstone", "no paint, only sandy ochre stone"),
    ("jade", "Jade", "deep green jade, softly translucent at the edges", "no paint, only green jade"),
    ("bronze", "Bronze", "cast bronze with a dark green-brown patina and worn bright highlights", "no paint, only bronze"),
    ("gold_leaf", "Gold leaf", "stone covered all over in gilded gold leaf", "no paint, only gold leaf over stone"),
];

/// The poses: `(id, label, the phrase the prompt uses)`, from the house's god statues of 2026-11-14 (`custom`: the
/// player's description says the pose).
pub const POSES: [(&str, &str, &str); 8] = [
    ("thinker", "The Thinker", "seated on a rough stone block in the pose of The Thinker: leaning forward, one elbow on the knee, chin resting on the back of the hand, the other forearm across the thigh, deep in thought"),
    ("david", "Michelangelo's David", "standing in the relaxed contrapposto pose of Michelangelo's David: weight on one leg, the other knee slightly bent, one arm hanging at the side, the other hand raised to the shoulder, head turned to the side with a stern gaze"),
    ("moses_seated", "Moses, seated", "seated like Michelangelo's Moses: upright on a stone throne, one hand in a long wild beard, the other resting on a tablet, head turned sharply to the side"),
    ("discus_thrower", "The Discus Thrower", "in the wound-up pose of the Discobolus, the discus thrower: crouched and twisted at the waist, one arm swung back holding a disc, the other hanging across the knee"),
    ("atlas", "Atlas", "kneeling on one knee as Atlas, straining to hold a great sphere on the shoulders and the back of the neck with both hands"),
    ("winged_victory", "Winged Victory", "striding forward like the Winged Victory of Samothrace, great wings spread, robes blown back by the wind"),
    ("boxer_at_rest", "The Boxer at Rest", "seated like the Boxer at Rest: hunched forward on a stone block, forearms on the thighs, hands loosely wrapped, head turned to look up over the shoulder"),
    ("custom", "Your own", "in the pose the player describes"),
];

/// The picture's size (the house's god images were 1024x1536: a full-length statue).
pub const PREVIEW_SIZE: &str = "1024x1536";
/// The default stone and pose.
pub const DEFAULT_STONE: &str = "white_marble";
pub const DEFAULT_POSE: &str = "thinker";

fn stone_of(id: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    STONES.iter().find(|s| s.0 == id)
}

fn pose_of(id: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    POSES.iter().find(|p| p.0 == id)
}

/// The exact prompt of a god form's reference image: the house's marble-statue prompt (the Unreal repo's
/// `figures/*_god/provenance.json`) with the player's stone, pose and words. Deterministic: the same form gives the
/// same bytes, so fn7-media's key (and its cache) follows the form.
pub fn god_prompt(doc: &GodDoc) -> String {
    let (material, reads) = match stone_of(&doc.stone) {
        Some(s) => (s.2.to_string(), s.3.to_string()),
        None => {
            let own = one_line(&doc.stone);
            (own.clone(), format!("no paint, only {own}"))
        }
    };
    let pose = pose_of(&doc.pose).map(|p| p.2).unwrap_or(POSES[0].2);
    let words = one_line(&doc.description);
    let words = words.trim_end_matches(['.', ' ']);
    let described = if words.is_empty() { String::new() } else { format!(" The god, as its player describes it: {words}.") };
    format!(
        "A single monumental statue of a god carved from {material}, in the manner of classical sculpture, {pose}.{described} \
Rendered entirely in stone: {reads}. Carved in a slightly blocky, faceted, simplified way, like a chunky low-poly \
sculpture: broad planes, strong chisel facets, simplified hands and face, a strong readable silhouette. The whole statue \
visible from head to toe including its plain rough-hewn stone block seat or base, limbs clearly separated from the body. \
Plain flat mid-grey background, soft even studio light, no cast shadow on the background, no text, no other figures, no \
scenery."
    )
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The fn7-media prompt of a god form (no house style, no references: the prompt is the whole picture).
pub fn god_image_prompt(doc: &GodDoc) -> ImagePrompt {
    ImagePrompt {
        // A portrait's lane: first in the queue, and an opaque background (the prompt asks for plain grey).
        kind: ImageKind::Portrait,
        subject: format!("god form: {}", one_line(&doc.description)),
        text: god_prompt(doc),
        size: PREVIEW_SIZE.into(),
        refs: vec![],
        base: None,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------------------------------

/// The god form's server settings (env).
#[derive(Debug, Clone)]
pub struct GodConfig {
    /// `god_preview_per_day` (`FN7_GOD_PREVIEW_PER_DAY`, default 6): new preview pictures per account in any 24 hours.
    /// Cached repeats are free.
    pub preview_per_day: u32,
    /// `god_statue_meshy` (`FN7_GOD_STATUE_MESHY`, default **off**): accepting a preview queues a Meshy statue.
    pub statue_meshy: bool,
    /// `god_statues_per_account` (`FN7_GOD_STATUES_PER_ACCOUNT`, default 1).
    pub statues_per_account: u32,
    /// What one statue is expected to cost, in credits (`FN7_GOD_STATUE_CREDITS`, default 30), recorded with basis
    /// `estimated` when the provider does not report it.
    pub statue_credits: f64,
    /// How long a statue may take before it is recorded as failed (`FN7_GOD_STATUE_TIMEOUT_S`, default 1800).
    pub statue_timeout: Duration,
}

impl Default for GodConfig {
    fn default() -> Self {
        Self { preview_per_day: 6, statue_meshy: false, statues_per_account: 1, statue_credits: 30.0, statue_timeout: Duration::from_secs(1800) }
    }
}

impl GodConfig {
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Self {
        let mut c = Self::default();
        let num = |k: &str| env(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|x| x.is_finite() && *x >= 0.0);
        if let Some(n) = num("FN7_GOD_PREVIEW_PER_DAY") {
            c.preview_per_day = n.min(10_000.0) as u32;
        }
        if let Some(v) = env("FN7_GOD_STATUE_MESHY") {
            c.statue_meshy = matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "on" | "true" | "yes");
        }
        if let Some(n) = num("FN7_GOD_STATUES_PER_ACCOUNT") {
            c.statues_per_account = n.min(1000.0) as u32;
        }
        if let Some(n) = num("FN7_GOD_STATUE_CREDITS") {
            c.statue_credits = n;
        }
        if let Some(n) = num("FN7_GOD_STATUE_TIMEOUT_S") {
            c.statue_timeout = Duration::from_secs_f64(n.max(10.0));
        }
        c
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Meshy
// ---------------------------------------------------------------------------------------------------------------------

/// A finished (or failed) image-to-3D task.
#[derive(Debug, Clone, Default)]
pub struct MeshyResult {
    /// `SUCCEEDED`, `FAILED`, `CANCELED`, `EXPIRED`, `TIMEOUT`.
    pub status: String,
    /// Credits the provider reports for the task, when it does.
    pub credits: Option<f64>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub glb: Option<Vec<u8>>,
    pub preview: Option<Vec<u8>>,
    pub error: Option<String>,
}

/// The image-to-3D provider. [`HttpMeshy`] is the real one (built only when `god_statue_meshy` is on and
/// `MESHY_API_KEY` is set); tests use a fake.
#[async_trait]
pub trait MeshyClient: Send + Sync {
    /// The endpoint, for the record.
    fn endpoint(&self) -> String;
    /// Start a task from an image; the provider's task id.
    async fn submit(&self, image: &[u8], mime: &str, params: &Value) -> anyhow::Result<String>;
    /// Wait for the task to end and fetch its files.
    async fn wait(&self, task_id: &str, timeout: Duration) -> anyhow::Result<MeshyResult>;
}

/// The parameters every god statue is made with (the house's statues of 2026-11-14).
pub fn statue_params() -> Value {
    json!({ "ai_model": "latest", "should_texture": true, "enable_pbr": true, "texture_resolution": "2k", "target_formats": ["glb"], "should_remesh": true, "target_polycount": 30000 })
}

/// Meshy's REST API (`POST /openapi/v1/image-to-3d`, then `GET /openapi/v1/image-to-3d/{id}` until it ends).
pub struct HttpMeshy {
    key: String,
    base: String,
    http: reqwest::Client,
}

impl HttpMeshy {
    /// `MESHY_API_KEY` (and `MESHY_BASE_URL`, default `https://api.meshy.ai`); `None` without a key.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let key = env("MESHY_API_KEY").map(|k| k.trim().to_string()).filter(|k| !k.is_empty())?;
        let base = env("MESHY_BASE_URL").map(|b| b.trim().trim_end_matches('/').to_string()).filter(|b| !b.is_empty())
            .unwrap_or_else(|| "https://api.meshy.ai".into());
        let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).build().ok()?;
        Some(Self { key, base, http })
    }
}

fn b64_std(data: &[u8]) -> String {
    let mut s = super::google::b64url(data).replace('-', "+").replace('_', "/");
    while !s.len().is_multiple_of(4) {
        s.push('=');
    }
    s
}

#[async_trait]
impl MeshyClient for HttpMeshy {
    fn endpoint(&self) -> String {
        format!("{}/openapi/v1/image-to-3d", self.base)
    }

    async fn submit(&self, image: &[u8], mime: &str, params: &Value) -> anyhow::Result<String> {
        let mut body = params.clone();
        body["image_url"] = json!(format!("data:{mime};base64,{}", b64_std(image)));
        let res = self.http.post(self.endpoint()).bearer_auth(&self.key).json(&body).send().await?;
        let status = res.status();
        let v: Value = res.json().await.unwrap_or(Value::Null);
        anyhow::ensure!(status.is_success(), "meshy answered {status}");
        v["result"].as_str().map(str::to_string).ok_or_else(|| anyhow::anyhow!("meshy gave no task id"))
    }

    async fn wait(&self, task_id: &str, timeout: Duration) -> anyhow::Result<MeshyResult> {
        let url = format!("{}/{task_id}", self.endpoint());
        let t0 = std::time::Instant::now();
        loop {
            let v: Value = self.http.get(&url).bearer_auth(&self.key).send().await?.json().await?;
            let status = v["status"].as_str().unwrap_or("").to_string();
            match status.as_str() {
                "SUCCEEDED" => {
                    let fetch = |u: Option<&str>| {
                        let u = u.map(str::to_string);
                        let http = self.http.clone();
                        async move {
                            match u {
                                Some(u) => http.get(u).send().await.ok()?.bytes().await.ok().map(|b| b.to_vec()),
                                None => None,
                            }
                        }
                    };
                    return Ok(MeshyResult {
                        status,
                        credits: v["consumed_credits"].as_f64(),
                        started_at: v["started_at"].as_i64(),
                        finished_at: v["finished_at"].as_i64(),
                        glb: fetch(v["model_urls"]["glb"].as_str()).await,
                        preview: fetch(v["thumbnail_url"].as_str()).await,
                        error: None,
                    });
                }
                "FAILED" | "CANCELED" | "EXPIRED" => {
                    return Ok(MeshyResult {
                        error: Some(v["task_error"]["message"].as_str().unwrap_or("the task did not succeed").chars().take(300).collect()),
                        status,
                        started_at: v["started_at"].as_i64(),
                        finished_at: v["finished_at"].as_i64(),
                        ..Default::default()
                    })
                }
                _ if t0.elapsed() > timeout => {
                    return Ok(MeshyResult { status: "TIMEOUT".into(), error: Some("the statue took too long".into()), ..Default::default() })
                }
                _ => tokio::time::sleep(Duration::from_secs(10)).await,
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The service
// ---------------------------------------------------------------------------------------------------------------------

/// The god form: the routes' state, and the figures endpoint's [`fn7_assets::http::Gods`].
pub struct GodForm {
    accounts: Arc<Accounts>,
    media: Media,
    registry: Option<Arc<Registry>>,
    meshy: Option<Arc<dyn MeshyClient>>,
    cfg: GodConfig,
}

impl GodForm {
    pub fn new(accounts: Arc<Accounts>, media: Media, registry: Option<Arc<Registry>>, meshy: Option<Arc<dyn MeshyClient>>, cfg: GodConfig) -> Arc<Self> {
        Arc::new(Self { accounts, media, registry, meshy, cfg })
    }

    /// From the environment: [`GodConfig::from_env`], and [`HttpMeshy`] only when `god_statue_meshy` is on.
    pub fn from_env(accounts: Arc<Accounts>, media: Media, registry: Option<Arc<Registry>>, env: impl Fn(&str) -> Option<String>) -> Arc<Self> {
        let cfg = GodConfig::from_env(&env);
        let meshy = if cfg.statue_meshy { HttpMeshy::from_env(&env).map(|m| Arc::new(m) as Arc<dyn MeshyClient>) } else { None };
        Self::new(accounts, media, registry, meshy, cfg)
    }

    pub fn config(&self) -> &GodConfig {
        &self.cfg
    }

    /// Where an image stands: `(pending | ready | refused | failed | missing, url)`.
    fn image_status(&self, hash: &str) -> (&'static str, Option<String>) {
        if let Some(e) = self.media.entry(hash) {
            let s = match e.status {
                fn7_media::Status::Pending => "pending",
                fn7_media::Status::Ready => "ready",
                fn7_media::Status::Failed => "failed",
                fn7_media::Status::Refused => "refused",
                fn7_media::Status::Off => "off",
            };
            return (s, e.url);
        }
        match self.media.cache().lookup(hash) {
            Some(side) => ("ready", Some(format!("{}/{hash}.{}", self.media.config().url_prefix, side.ext))),
            None => ("missing", None),
        }
    }

    fn previews_left(&self, account: &str) -> Result<(u32, Option<i64>), AccountError> {
        let since = theatre_trace::now_ms() - DAY_MS;
        let (n, oldest) = self.accounts.store().god_previews_since(account, since)?;
        Ok((self.cfg.preview_per_day.saturating_sub(n as u32), oldest))
    }

    fn view(&self, account: &Account) -> Result<Value, AccountError> {
        let store = self.accounts.store();
        let row = store.god(&account.id)?;
        let preview = store.god_preview(&account.id, None)?;
        let accepted = store.accepted_god(&account.id)?;
        let statues = store.god_statues(&account.id)?;
        let (left, _) = self.previews_left(&account.id)?;
        Ok(god_view(self, row.as_ref(), preview.as_ref(), accepted.as_ref(), &statues, left))
    }
}

const DAY_MS: i64 = 24 * 3600 * 1000;

fn options() -> Value {
    json!({
        "stones": STONES.iter().map(|s| json!({ "id": s.0, "label": s.1 })).collect::<Vec<_>>(),
        "poses": POSES.iter().map(|p| json!({ "id": p.0, "label": p.1 })).collect::<Vec<_>>(),
    })
}

fn god_view(g: &GodForm, row: Option<&GodRow>, preview: Option<&GodPreview>, accepted: Option<&fn7_profile::gods::GodAccepted>, statues: &[GodStatue], left: u32) -> Value {
    let doc = row.map(|r| r.r#final.clone()).unwrap_or_default();
    let image = |hash: &str| {
        let (status, url) = g.image_status(hash);
        (status, url)
    };
    json!({
        "saved": row.is_some(),
        "description": doc.description,
        "stone": doc.stone,
        "stone_custom": stone_of(&doc.stone).is_none(),
        "pose": doc.pose,
        "moderation": row.map(|r| json!({ "verdict": r.verdict, "reason": r.reason, "model": r.model, "at": r.moderated_at, "original": r.original })),
        "preview": preview.map(|p| {
            let (status, url) = image(&p.media_hash);
            json!({ "hash": p.media_hash, "url": url, "status": status, "prompt": p.prompt, "at": p.at,
                    "description": p.doc.description, "stone": p.doc.stone, "pose": p.doc.pose,
                    "accepted": accepted.is_some_and(|a| a.media_hash == p.media_hash) })
        }),
        "portrait": accepted.map(|a| {
            let (status, url) = image(&a.media_hash);
            json!({ "hash": a.media_hash, "url": url, "status": status, "accepted_at": a.accepted_at,
                    "description": a.doc.description, "stone": a.doc.stone, "pose": a.doc.pose })
        }),
        "statues": statues,
        "statue_meshy": g.cfg.statue_meshy,
        "limits": {
            "description": moderate::MAX_GOD_DESCRIPTION, "stone": moderate::MAX_GOD_STONE,
            "preview_per_day": g.cfg.preview_per_day, "previews_left_today": left,
            "statues_per_account": g.cfg.statues_per_account,
        },
        "options": options(),
    })
}

type St = State<Arc<GodForm>>;

pub fn router(gods: Arc<GodForm>) -> Router {
    Router::new()
        .route("/api/me/god", get(get_god).put(put_god))
        .route("/api/me/god/preview", post(preview))
        .route("/api/me/god/accept", post(accept))
        .with_state(gods)
}

async fn get_god(State(g): St, h: HeaderMap) -> Response {
    let (account, _) = match authed_limited(&g.accounts, &h, Class::Read).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let g2 = g.clone();
    match tokio::task::spawn_blocking(move || g2.view(&account)).await {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => account_error(e),
        Err(_) => fail(StatusCode::INTERNAL_SERVER_ERROR, "internal", "the request could not be completed"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GodBody {
    #[serde(default)]
    description: String,
    #[serde(default)]
    stone: Option<String>,
    #[serde(default)]
    pose: Option<String>,
}

/// Check the form's shape (never altering the text): `Ok((doc, custom stone text))`.
fn check_form(b: GodBody) -> Result<(GodDoc, Option<String>), String> {
    check_text("description", &b.description, moderate::MAX_GOD_DESCRIPTION, true)?;
    let stone = b.stone.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_STONE.into());
    let custom_stone = if stone_of(&stone).is_some() {
        None
    } else {
        check_text("stone", &stone, moderate::MAX_GOD_STONE, false)?;
        if !stone.chars().any(char::is_alphabetic) {
            return Err("stone needs at least one letter".into());
        }
        Some(stone.clone())
    };
    let pose = b.pose.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| DEFAULT_POSE.into());
    if pose_of(&pose).is_none() {
        let ids: Vec<&str> = POSES.iter().map(|p| p.0).collect();
        return Err(format!("pose is one of {}", ids.join(", ")));
    }
    if pose == "custom" && b.description.trim().is_empty() {
        return Err("a custom pose needs a description that says it".into());
    }
    Ok((GodDoc { description: b.description, stone, pose }, custom_stone))
}

async fn put_god(State(g): St, h: HeaderMap, body: Bytes) -> Response {
    let (account, _) = match authed(&g.accounts, &h).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let b: GodBody = match parse(&body, SMALL_BODY) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let (original, custom_stone) = match check_form(b) {
        Ok(x) => x,
        Err(e) => return fail(StatusCode::BAD_REQUEST, "invalid", e),
    };
    let st = g.accounts.clone();
    let id = account.id.clone();
    let stored = match blocking(&st, move |a| a.store().god(&id)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return account_error(e),
        Err(r) => return r,
    };
    // Unchanged since the last save: the stored answer stands, no new pass.
    if let Some(row) = stored.as_ref().filter(|r| r.original == original) {
        return answer(&g, &account, row, &[]).await;
    }
    if let Err(w) = st.limiter.check(Class::Moderate, &account.id) {
        return limited(w);
    }
    // The one moderation pass over the player's words (the description, and the stone when it is theirs).
    let text = GodText { description: original.description.clone(), stone: custom_stone };
    let j = st.moderate_god(&text).await;
    tracing::info!(account = %account.id, verdict = j.verdict.as_str(), model = %j.model, fallback = ?j.fallback, "accounts: god form judged");
    let fin = GodDoc {
        description: j.r#final.description.clone(),
        stone: j.r#final.stone.clone().unwrap_or_else(|| original.stone.clone()),
        pose: original.pose.clone(),
    };
    let (acc, orig, judged, fin2) = (account.clone(), original.clone(), j.clone(), fin.clone());
    let res = blocking(&st, move |a| -> Result<Option<GodRow>, AccountError> {
        if judged.verdict == Verdict::Refuse {
            a.store().log_god_refusal(&acc.id, &orig, &judged.reason, &judged.categories, &judged.model)?;
            return Ok(None);
        }
        a.store().put_god(&acc.id, &orig, &fin2, judged.verdict.as_str(), &judged.reason, &judged.categories, &judged.model).map(Some)
    })
    .await;
    match res {
        Ok(Ok(Some(row))) => answer(&g, &account, &row, &j.categories).await,
        Ok(Ok(None)) => {
            let g2 = g.clone();
            let acc = account.clone();
            let current = tokio::task::spawn_blocking(move || g2.view(&acc)).await.ok().and_then(Result::ok).unwrap_or(Value::Null);
            finish(
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({
                        "error": "refused", "detail": j.reason, "verdict": "refuse", "reason": j.reason, "categories": j.categories,
                        "model": j.model, "original": original,
                        // Nothing was saved: this is what the account still has.
                        "god": current,
                    })),
                )
                    .into_response(),
            )
        }
        Ok(Err(e)) => account_error(e),
        Err(r) => r,
    }
}

/// The `PUT` answer for a saved form: `ok` (verbatim) or `rephrase` (with both versions and the reason).
async fn answer(g: &Arc<GodForm>, account: &Account, row: &GodRow, categories: &[String]) -> Response {
    let g2 = g.clone();
    let acc = account.clone();
    let view = match tokio::task::spawn_blocking(move || g2.view(&acc)).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return account_error(e),
        Err(_) => return fail(StatusCode::INTERNAL_SERVER_ERROR, "internal", "the request could not be completed"),
    };
    let mut v = json!({ "verdict": row.verdict, "model": row.model, "god": view });
    if row.verdict == "rephrase" {
        v["reason"] = row.reason.clone().into();
        v["categories"] = json!(categories);
        v["original"] = json!(row.original);
        v["final"] = json!(row.r#final);
    }
    ok(v)
}

fn no_body(body: &Bytes) -> Result<(), Response> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    parse::<Empty>(body, SMALL_BODY).map(|_| ())
}

async fn preview(State(g): St, h: HeaderMap, body: Bytes) -> Response {
    let (account, _) = match authed_limited(&g.accounts, &h, Class::Write).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Err(r) = no_body(&body) {
        return r;
    }
    if !g.media.enabled() {
        return fail(StatusCode::SERVICE_UNAVAILABLE, "images_off", "images are off on this server (FN7_IMAGES)");
    }
    let g2 = g.clone();
    let res = tokio::task::spawn_blocking(move || -> Result<Response, AccountError> {
        let store = g2.accounts.store();
        let Some(row) = store.god(&account.id)? else {
            return Ok(fail(StatusCode::NOT_FOUND, "not_found", "save your god form first (PUT /api/me/god)"));
        };
        let doc = row.r#final;
        let prompt = god_image_prompt(&doc);
        let hash = g2.media.key_for(&prompt);
        // Known to the media layer (asked for before, or on disk): free, and never generated twice.
        let known = g2.media.peek(&prompt).is_some();
        let (left, oldest) = g2.previews_left(&account.id)?;
        if !known && left == 0 {
            let wait = oldest.map(|o| ((o + DAY_MS - theatre_trace::now_ms()) / 1000).max(1) as u64).unwrap_or(60);
            let mut r = fail(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                format!("at most {} new god previews a day: try again in {wait} s (an image you already made is free)", g2.cfg.preview_per_day),
            );
            if let Ok(v) = HeaderValue::from_str(&wait.to_string()) {
                r.headers_mut().insert(header::RETRY_AFTER, v);
            }
            return Ok(r);
        }
        // Claim the image in the registry first: the media layer's live rows then land on a god preview of this
        // account, with the exact prompt and cost.
        if let Some(reg) = &g2.registry {
            let claim = AssetIn {
                id: asset_id_for_media(&hash),
                kind: "image".into(),
                subkind: "god_preview".into(),
                subject: Some(Subject::named("god", &account.display_name)),
                subject_strong: true,
                created_at: theatre_trace::now_ms(),
                status: "pending".into(),
                media_hash: Some(hash.clone()),
                prompt_version: Some(fn7_media::PROMPT_VERSION.into()),
                source: "live".into(),
                meta: Some(json!({ "account": account.id, "description": doc.description, "stone": doc.stone, "pose": doc.pose, "size": PREVIEW_SIZE })),
                ..Default::default()
            };
            if let Err(e) = reg.upsert_asset(&claim) {
                tracing::warn!(error = %e, "god form: the registry claim failed");
            }
        }
        let asker = Asker { seat: None, account: Some(account.id.clone()) };
        let r = LiveHook::asking_sync(asker, || g2.media.request(&prompt, None));
        if r.status == "off" {
            let why = g2.media.entry(&r.hash).and_then(|e| e.reason).unwrap_or_else(|| "images are off".into());
            let code = if why.contains("budget") { "budget_reached" } else { "image_refused" };
            tracing::info!(account = %account.id, reason = %why, "god form: preview refused");
            return Ok(fail(StatusCode::SERVICE_UNAVAILABLE, code, why));
        }
        store.note_god_preview(&account.id, &r.hash, &doc, &prompt.text, !known)?;
        let left = if known { left } else { left.saturating_sub(1) };
        tracing::info!(account = %account.id, hash = %r.hash, charged = !known, status = r.status, "god form: preview");
        Ok(ok(json!({
            "status": r.status, "hash": r.hash, "url": r.url, "prompt": prompt.text, "charged": !known,
            "previews_left_today": left, "description": doc.description, "stone": doc.stone, "pose": doc.pose,
        })))
    })
    .await;
    match res {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => account_error(e),
        Err(_) => fail(StatusCode::INTERNAL_SERVER_ERROR, "internal", "the request could not be completed"),
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct AcceptBody {
    #[serde(default)]
    hash: Option<String>,
}

async fn accept(State(g): St, h: HeaderMap, body: Bytes) -> Response {
    let (account, _) = match authed_limited(&g.accounts, &h, Class::Write).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let b: AcceptBody = match parse(&body, SMALL_BODY) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let g2 = g.clone();
    let acc = account.clone();
    // The portrait, at once.
    let step = tokio::task::spawn_blocking(move || -> Result<Result<(GodPreview, fn7_profile::gods::GodAccepted, Option<String>), Response>, AccountError> {
        let store = g2.accounts.store();
        let Some(p) = store.god_preview(&acc.id, b.hash.as_deref().filter(|h| !h.is_empty()))? else {
            return Ok(Err(fail(StatusCode::NOT_FOUND, "not_found", "no such preview: make one first (POST /api/me/god/preview)")));
        };
        let (status, url) = g2.image_status(&p.media_hash);
        if status != "ready" {
            return Ok(Err(fail(
                StatusCode::CONFLICT,
                "preview_not_ready",
                format!("the preview is {status}: accept it once it is ready (GET /api/me/god)"),
            )));
        }
        let a = store.accept_god(&acc.id, &p)?;
        if let Some(reg) = &g2.registry {
            let id = asset_id_for_media(&p.media_hash);
            let r = reg.with(|c| {
                c.execute(
                    "UPDATE assets SET meta = json_set(COALESCE(meta, '{}'), '$.accepted_at', ?2, '$.accepted_by', ?3), updated_at = ?2 WHERE id = ?1",
                    rusqlite::params![id, a.accepted_at, acc.id],
                )?;
                Ok(())
            });
            if let Err(e) = r {
                tracing::warn!(error = %e, "god form: marking the accepted preview failed");
            }
        }
        tracing::info!(account = %acc.id, hash = %p.media_hash, "god form: preview accepted");
        Ok(Ok((p, a, url)))
    })
    .await;
    let (p, accepted, url) = match step {
        Ok(Ok(Ok(x))) => x,
        Ok(Ok(Err(r))) => return r,
        Ok(Err(e)) => return account_error(e),
        Err(_) => return fail(StatusCode::INTERNAL_SERVER_ERROR, "internal", "the request could not be completed"),
    };
    let statue = queue_statue(&g, &account, &p).await;
    ok(json!({
        "accepted": { "hash": accepted.media_hash, "url": url, "status": "ready", "accepted_at": accepted.accepted_at,
                      "description": accepted.doc.description, "stone": accepted.doc.stone, "pose": accepted.doc.pose },
        "statue": statue,
    }))
}

/// Queue the statue of an accepted preview, when the server allows it. `"not_enabled"` when `god_statue_meshy` is
/// off; `"not_configured"` when it is on without a Meshy key; otherwise the statue's record.
async fn queue_statue(g: &Arc<GodForm>, account: &Account, p: &GodPreview) -> Value {
    if !g.cfg.statue_meshy {
        return json!("not_enabled");
    }
    let Some(meshy) = g.meshy.clone() else {
        return json!("not_configured");
    };
    let store = g.accounts.store().clone();
    let id = account.id.clone();
    let statues = match tokio::task::spawn_blocking(move || store.god_statues(&id)).await {
        Ok(Ok(s)) => s,
        _ => return json!({ "status": "failed", "error": "the account store failed" }),
    };
    // One statue per accepted image: the same image again is the statue it already has.
    if let Some(s) = statues.iter().rev().find(|s| s.media_hash == p.media_hash && s.status != "failed") {
        return json!({ "status": s.status, "task_id": s.task_id, "provider": s.provider, "credits": s.credits, "asset_id": s.asset_id, "cached": true });
    }
    let live = statues.iter().filter(|s| s.status != "failed").count() as u32;
    if live >= g.cfg.statues_per_account {
        return json!({ "status": "cap_reached", "cap": g.cfg.statues_per_account, "statues": live });
    }
    let Some(side) = g.media.cache().lookup(&p.media_hash) else {
        return json!({ "status": "failed", "error": "the accepted image is not on disk" });
    };
    let path = g.media.cache().image_path(&p.media_hash, &side.ext);
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return json!({ "status": "failed", "error": "the accepted image could not be read" });
    };
    let mime = fn7_media::cache::content_type(&side.ext);
    let params = statue_params();
    let requested_at = theatre_trace::now_ms();
    let task = match meshy.submit(&bytes, mime, &params).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(account = %account.id, error = %e, "god form: the statue could not be queued");
            return json!({ "status": "failed", "error": "the statue could not be queued" });
        }
    };
    let store = g.accounts.store().clone();
    let (id, t, hash, est) = (account.id.clone(), task.clone(), p.media_hash.clone(), g.cfg.statue_credits);
    let _ = tokio::task::spawn_blocking(move || store.add_god_statue(&id, &t, &hash, "meshy", Some(est))).await;
    tracing::info!(account = %account.id, task = %task, "god form: statue queued");
    let job = StatueJob {
        account: account.id.clone(),
        name: account.display_name.clone(),
        task: task.clone(),
        source: path,
        params,
        requested_at,
        endpoint: meshy.endpoint(),
    };
    let g2 = g.clone();
    tokio::spawn(async move { g2.finish_statue(meshy, job).await });
    json!({ "status": "queued", "task_id": task, "provider": "meshy", "credits_estimate": g.cfg.statue_credits })
}

struct StatueJob {
    account: String,
    name: String,
    task: String,
    source: PathBuf,
    params: Value,
    requested_at: i64,
    endpoint: String,
}

impl GodForm {
    /// Wait for a queued statue, then record the job in the registry through the job-record path and mark it done.
    async fn finish_statue(self: Arc<Self>, meshy: Arc<dyn MeshyClient>, job: StatueJob) {
        let result = match meshy.wait(&job.task, self.cfg.statue_timeout).await {
            Ok(r) => r,
            Err(e) => MeshyResult { status: "FAILED".into(), error: Some(format!("{e}").chars().take(300).collect()), ..Default::default() },
        };
        let me = self.clone();
        let _ = tokio::task::spawn_blocking(move || me.record_statue(&job, &result)).await;
    }

    fn record_statue(&self, job: &StatueJob, r: &MeshyResult) {
        let ok = r.status == "SUCCEEDED" && r.glb.is_some();
        let (credits, basis) = match r.credits {
            Some(c) => (c, "credits_reported"),
            None => (self.cfg.statue_credits, "estimated"),
        };
        let asset = self.registry.as_ref().and_then(|reg| match write_job_record(reg, job, r, credits, basis) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(task = %job.task, error = %e, "god form: the statue's job record failed");
                None
            }
        });
        let error = if ok { None } else { Some(r.error.clone().unwrap_or_else(|| "the statue did not succeed".into())) };
        let _ = self.accounts.store().finish_god_statue(&job.task, if ok { "ready" } else { "failed" }, Some(credits), asset.as_deref(), error.as_deref());
        tracing::info!(account = %job.account, task = %job.task, ok, "god form: statue finished");
    }
}

/// Write the finished job's files under `<assets>/incoming/god/<task>/`, append its record to
/// `<assets>/jobs/god_statues.jsonl`, and import it (`fn7_assets::provenance::import_jobs`). The statue's asset id.
fn write_job_record(reg: &Registry, job: &StatueJob, r: &MeshyResult, credits: f64, basis: &str) -> anyhow::Result<Option<String>> {
    let root = reg.root().to_path_buf();
    let slug: String = job.task.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(80).collect();
    let rel_dir = format!("incoming/god/{slug}");
    let dir = root.join(&rel_dir);
    std::fs::create_dir_all(&dir)?;
    let ext = job.source.extension().and_then(|e| e.to_str()).unwrap_or("png");
    let src_rel = format!("{rel_dir}/source.{ext}");
    std::fs::copy(&job.source, root.join(&src_rel))?;
    let ok = r.status == "SUCCEEDED" && r.glb.is_some();
    let mut outputs = vec![];
    if let (true, Some(glb)) = (ok, &r.glb) {
        std::fs::write(dir.join("statue.glb"), glb)?;
        outputs.push(json!({ "path": format!("{rel_dir}/statue.glb"), "role": "primary" }));
    }
    if let (true, Some(png)) = (ok, &r.preview) {
        std::fs::write(dir.join("preview.png"), png)?;
        outputs.push(json!({ "path": format!("{rel_dir}/preview.png"), "role": "preview", "optional": true }));
    }
    let record = json!({
        "v": 1, "at": theatre_trace::now_ms(), "provider": "meshy", "operation": "image-to-3d", "task_id": job.task,
        "status": r.status, "error": r.error, "credits": credits, "cost_basis": basis,
        "requested_at": job.requested_at, "started_at": r.started_at, "finished_at": r.finished_at.or(Some(theatre_trace::now_ms())),
        "endpoint": job.endpoint, "model": null, "prompt": null, "negative_prompt": null, "params": job.params,
        "subject": { "kind": "god", "name": job.name, "slug": job.account },
        "asset": { "kind": "model", "subkind": "statue", "variant": "" },
        "inputs": [ { "path": src_rel, "role": "source" } ],
        "outputs": outputs,
        "requested_by": format!("account:{}", job.account), "tool": "fn7-server god form",
    });
    let line = format!("{record}\n");
    let jobs = root.join("jobs");
    std::fs::create_dir_all(&jobs)?;
    use std::io::Write;
    std::fs::OpenOptions::new().create(true).append(true).open(jobs.join("god_statues.jsonl"))?.write_all(line.as_bytes())?;
    let one = dir.join("job.jsonl");
    std::fs::write(&one, &line)?;
    fn7_assets::provenance::import_jobs(reg, &one, Some(Path::new(&root)))?;
    reg.with(|c| {
        c.execute("UPDATE generations SET requested_account = ?2 WHERE provider = 'meshy' AND provider_task_id = ?1", rusqlite::params![job.task, job.account])?;
        Ok(c.query_row(
            "SELECT asset_id FROM generations WHERE provider = 'meshy' AND provider_task_id = ?1 LIMIT 1",
            [&job.task],
            |row| row.get::<_, String>(0),
        )
        .ok())
    })
}

impl fn7_assets::http::Gods for GodForm {
    fn god(&self, name: &str) -> Option<AccountGod> {
        let store = self.accounts.store();
        let holder = store.name_holder(name).ok()??;
        let a = store.accepted_god(&holder.account_id).ok()??;
        let (status, url) = self.image_status(&a.media_hash);
        let statues = store.god_statues(&holder.account_id).unwrap_or_default();
        let ready = statues.iter().rev().find(|s| s.status == "ready" && s.asset_id.is_some());
        Some(AccountGod {
            account: holder.account_id,
            media_hash: a.media_hash,
            url,
            status: status.into(),
            statue_asset: ready.and_then(|s| s.asset_id.clone()),
            statue_status: statues.last().map(|s| s.status.clone()),
            description: a.doc.description,
            stone: a.doc.stone,
            pose: a.doc.pose,
            accepted_at: a.accepted_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_is_the_house_marble_prompt_with_the_players_choices() {
        let d = GodDoc { description: "A  hunched old\ngod with a lantern.".into(), stone: "white_marble".into(), pose: "thinker".into() };
        let p = god_prompt(&d);
        assert!(p.starts_with("A single monumental statue of a god carved from white marble with soft grey veining, in the manner of classical sculpture, seated on a rough stone block in the pose of The Thinker"));
        assert!(p.contains(" The god, as its player describes it: A hunched old god with a lantern. Rendered entirely in stone: no colour, no paint, only pale marble."));
        assert!(p.ends_with("no text, no other figures, no scenery."));
        assert_eq!(p, god_prompt(&d), "deterministic");
        let own = GodDoc { description: String::new(), stone: "cracked obsidian".into(), pose: "custom".into() };
        let p = god_prompt(&own);
        assert!(p.contains("carved from cracked obsidian, in the manner of classical sculpture, in the pose the player describes."));
        assert!(p.contains("no paint, only cracked obsidian"));
        assert!(!p.contains("as its player describes it"));
        for (id, ..) in STONES {
            assert!(stone_of(id).is_some());
        }
        assert_eq!(POSES.len(), 8);
        assert_eq!(god_image_prompt(&d).size, "1024x1536");
    }
}
