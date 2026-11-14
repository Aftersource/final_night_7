//! The content-addressed image cache on disk: `<dir>/<key>.<ext>` plus a `<key>.json` sidecar (prompt, kind, provider,
//! model, created_at, cost, latency). Files are written to a temp name and renamed, so a reader never sees half an image.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::prompt::ImageKind;

/// The sidecar written next to every cached image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sidecar {
    pub key: String,
    pub kind: ImageKind,
    pub prompt: String,
    pub provider: String,
    pub model: String,
    /// File extension of the image (`png`, `svg`, …).
    pub ext: String,
    pub bytes: u64,
    /// Unix ms.
    pub created_at: i64,
    pub cost_usd: f64,
    pub latency_ms: f64,
    /// The match (recorder run id) that first asked for it, if known.
    #[serde(default)]
    pub match_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaCache {
    dir: PathBuf,
}

/// Image extensions the cache stores and the server serves.
pub const EXTS: [&str; 4] = ["png", "svg", "webp", "jpeg"];

/// A valid image key: 64 lowercase hex chars.
pub fn valid_key(k: &str) -> bool {
    k.len() == 64
        && k.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn content_type(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "jpeg" => "image/jpeg",
        _ => "application/octet-stream",
    }
}

impl MediaCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn image_path(&self, key: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{key}.{ext}"))
    }

    fn sidecar_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    /// The sidecar of a cached image whose file is present.
    pub fn lookup(&self, key: &str) -> Option<Sidecar> {
        if !valid_key(key) {
            return None;
        }
        let s: Sidecar =
            serde_json::from_slice(&std::fs::read(self.sidecar_path(key)).ok()?).ok()?;
        self.image_path(key, &s.ext).is_file().then_some(s)
    }

    /// Store an image and its sidecar (image first, so a sidecar always points at a whole file).
    pub fn store(&self, bytes: &[u8], side: &Sidecar) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.image_path(&side.key, &side.ext);
        write_atomic(&path, bytes)?;
        let json = serde_json::to_vec_pretty(side).map_err(std::io::Error::other)?;
        write_atomic(&self.sidecar_path(&side.key), &json)?;
        Ok(path)
    }

    /// Every cached image (sidecar present and file present).
    pub fn scan(&self) -> Vec<Sidecar> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return vec![];
        };
        let mut v: Vec<Sidecar> = rd
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let key = name.strip_suffix(".json")?;
                self.lookup(key)
            })
            .collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.key.cmp(&b.key)));
        v
    }

    /// Everything ever spent on the images in this cache (USD).
    pub fn total_cost(&self) -> f64 {
        self.scan().iter().map(|s| s.cost_usd).sum()
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp-{}-{n}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
