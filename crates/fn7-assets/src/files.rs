//! The file side of the registry: content-addressed files in `<root>/<kind dir>/<subkind>/<sha256>.<ext>`.
//! A file is written once and never changed; storing the same bytes again is a no-op.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// How a source file gets into the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// Hard-link (no extra disk; falls back to a copy across volumes). Only for sources that are never rewritten in
    /// place (the media cache writes a temp file and renames it).
    Hard,
    /// Copy (on APFS a clone: no extra disk until one side changes).
    Copy,
}

/// A stored file.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub sha256: String,
    /// Relative to the assets root, `/`-separated.
    pub path: String,
    pub bytes: u64,
    pub mime: &'static str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub tris: Option<u64>,
}

/// `image` → `images`, `model` → `models`, `texture` → `textures`, `animation` → `animations`.
pub fn kind_dir(kind: &str) -> &'static str {
    match kind {
        "model" => "models",
        "texture" => "textures",
        "animation" => "animations",
        _ => "images",
    }
}

fn clean(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c.to_ascii_lowercase() } else { '_' })
        .take(40)
        .collect();
    if t.is_empty() { "other".into() } else { t }
}

pub fn mime_for(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "fbx" => "application/vnd.autodesk.fbx",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

pub fn sha256_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
    Ok((hex::encode(h.finalize()), n))
}

pub fn sha256_bytes(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// The relative path a file of this kind and hash lives at.
pub fn rel_path(kind: &str, subkind: &str, sha256: &str, ext: &str) -> String {
    format!("{}/{}/{}.{}", kind_dir(kind), clean(subkind), sha256, clean(ext))
}

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_else(|| "bin".into())
}

/// Put `src` into the layout under `root` (hash it, then link or copy unless it is already there).
pub fn store_file(root: &Path, src: &Path, kind: &str, subkind: &str, link: Link) -> std::io::Result<Stored> {
    let (sha, bytes) = sha256_file(src)?;
    let ext = ext_of(src);
    let rel = rel_path(kind, subkind, &sha, &ext);
    let dest = root.join(&rel);
    if !dest.is_file() {
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d)?;
        }
        let linked = link == Link::Hard && std::fs::hard_link(src, &dest).is_ok();
        if !linked {
            let tmp = tmp_name(&dest);
            std::fs::copy(src, &tmp)?;
            std::fs::rename(&tmp, &dest)?;
        }
    }
    let (width, height, tris) = measure(&dest, &ext);
    Ok(Stored { sha256: sha, path: rel, bytes, mime: mime_for(&ext), width, height, tris })
}

/// Put bytes into the layout (operator uploads).
pub fn store_bytes(root: &Path, data: &[u8], kind: &str, subkind: &str, ext: &str) -> std::io::Result<Stored> {
    let sha = sha256_bytes(data);
    let ext = clean(ext);
    let rel = rel_path(kind, subkind, &sha, &ext);
    let dest = root.join(&rel);
    if !dest.is_file() {
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = tmp_name(&dest);
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, &dest)?;
    }
    let (width, height, tris) = measure(&dest, &ext);
    Ok(Stored { sha256: sha, path: rel, bytes: data.len() as u64, mime: mime_for(&ext), width, height, tris })
}

fn tmp_name(dest: &Path) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dest.with_extension(format!("tmp-{}-{n}", std::process::id()))
}

/// Width and height of an image, triangles of a GLB, where the header says (never fails: unknown is `None`).
pub fn measure(path: &Path, ext: &str) -> (Option<u32>, Option<u32>, Option<u64>) {
    let head = |n: usize| -> Vec<u8> {
        let mut v = vec![0u8; n];
        let got = std::fs::File::open(path).and_then(|mut f| {
            let mut k = 0;
            while k < n {
                match f.read(&mut v[k..])? {
                    0 => break,
                    r => k += r,
                }
            }
            Ok(k)
        });
        v.truncate(got.unwrap_or(0));
        v
    };
    match ext {
        "png" => {
            let h = head(24);
            if h.len() == 24 && &h[1..4] == b"PNG" {
                let be = |i: usize| u32::from_be_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
                return (Some(be(16)), Some(be(20)), None);
            }
            (None, None, None)
        }
        "jpg" | "jpeg" => {
            let h = head(1 << 18);
            let mut i = 2;
            while i + 9 < h.len() {
                if h[i] != 0xFF {
                    i += 1;
                    continue;
                }
                let m = h[i + 1];
                let len = u16::from_be_bytes([h[i + 2], h[i + 3]]) as usize;
                if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
                    let hh = u16::from_be_bytes([h[i + 5], h[i + 6]]) as u32;
                    let ww = u16::from_be_bytes([h[i + 7], h[i + 8]]) as u32;
                    return (Some(ww), Some(hh), None);
                }
                i += 2 + len;
            }
            (None, None, None)
        }
        "svg" => {
            let h = String::from_utf8_lossy(&head(600)).into_owned();
            let attr = |k: &str| -> Option<u32> {
                let i = h.find(&format!(" {k}=\""))? + k.len() + 3;
                let digits: String = h[i..].chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse().ok()
            };
            (attr("width"), attr("height"), None)
        }
        "glb" => (None, None, glb_tris(path)),
        _ => (None, None, None),
    }
}

/// Triangles in a GLB: the sum over every mesh primitive (triangle mode) of its index count / 3 (or its vertex count
/// / 3 when it has no indices). Reads only the JSON chunk.
fn glb_tris(path: &Path) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = [0u8; 20];
    f.read_exact(&mut h).ok()?;
    if &h[0..4] != b"glTF" || &h[16..20] != b"JSON" {
        return None;
    }
    let len = u32::from_le_bytes([h[12], h[13], h[14], h[15]]) as usize;
    if len > 64 << 20 {
        return None;
    }
    let mut json = vec![0u8; len];
    f.read_exact(&mut json).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&json).ok()?;
    let acc = v.get("accessors")?.as_array()?;
    let count = |i: &serde_json::Value| acc.get(i.as_u64()? as usize)?.get("count")?.as_u64();
    let mut tris = 0u64;
    for m in v.get("meshes")?.as_array()? {
        for p in m.get("primitives").and_then(|p| p.as_array()).into_iter().flatten() {
            if p.get("mode").and_then(|m| m.as_u64()).unwrap_or(4) != 4 {
                continue;
            }
            let n = match p.get("indices") {
                Some(i) => count(i),
                None => p.pointer("/attributes/POSITION").and_then(count),
            };
            tris += n.unwrap_or(0) / 3;
        }
    }
    Some(tris)
}
