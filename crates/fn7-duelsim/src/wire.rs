//! The wire encoding of a bake: quantised keyframes in little-endian byte arrays, sent as base64 in JSON.
//!
//! **Body track** (`bodies[].k`): `u16 first` (the first key's time, in samples from the panel's start; a sample is
//! 1/60 s), then 11-byte keys: `u8 dt` (samples since the previous key; 0 for the first), `i16 x, y, z` (millimetres,
//! the stage frame), `u32 q` (the rotation, smallest-three: bits 30-31 = the index of the largest component of
//! `[x, y, z, w]`, which is positive and left out; the other three in index order, 10 bits each, value =
//! `round((c / 0.70710678 + 1) * 511.5)`).
//!
//! **Root track** (`fighters.a.root`): `u16 first`, then 16-byte keys: `u8 dt`, `i16 x, y, z` (mm), `i16 yaw, pitch,
//! roll` (milliradians; yaw is continuous, it does not wrap), `i8 lean_x, lean_z` (units of 4 mrad: the lean of the
//! body in world x and z), `u8 flags` (1 = in the air, 2 = hung / not planted, 4 = low).
//!
//! Between keys: positions and angles interpolate linearly, rotations by normalised lerp (shortest way), flags hold.

use crate::math::*;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub fn b64(bytes: &[u8]) -> String {
    let mut o = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        o.push(B64[(n >> 18) as usize & 63] as char); o.push(B64[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' }); o.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    o
}
pub fn unb64(s: &str) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len() / 4 * 3); let (mut acc, mut bits) = (0u32, 0);
    for ch in s.bytes() {
        let Some(v) = B64.iter().position(|b| *b == ch) else { continue };
        acc = acc << 6 | v as u32; bits += 6;
        if bits >= 8 { bits -= 8; o.push((acc >> bits) as u8); }
    }
    o
}
pub fn q_mm(x: f64) -> i16 { clamp((x * 1000.0 + 0.5).floor(), -32767.0, 32767.0) as i16 }
pub fn q_mrad(x: f64) -> i16 { clamp((x * 1000.0 + 0.5).floor(), -32767.0, 32767.0) as i16 }
pub fn q_lean(x: f64) -> i8 { clamp((x * 250.0 + 0.5).floor(), -127.0, 127.0) as i8 }
pub fn q_quat(q: Q) -> u32 {
    let mut big = 0;
    for i in 1..4 { if q[i].abs() > q[big].abs() { big = i; } }
    let s = if q[big] < 0.0 { -1.0 } else { 1.0 };
    let mut out = (big as u32) << 30; let mut sh = 20i32;
    for i in 0..4 { if i == big { continue; }
        let v = clamp(((q[i] * s / std::f64::consts::FRAC_1_SQRT_2 + 1.0) * 511.5 + 0.5).floor(), 0.0, 1023.0) as u32;
        out |= v << sh; sh -= 10; }
    out
}
pub fn unq_quat(u: u32) -> Q {
    let big = (u >> 30) as usize; let mut q = [0.0; 4]; let (mut sh, mut sum) = (20i32, 0.0);
    for i in 0..4 { if i == big { continue; } let v = ((u >> sh) & 1023) as f64 / 511.5 - 1.0; q[i] = v * std::f64::consts::FRAC_1_SQRT_2; sum += q[i] * q[i]; sh -= 10; }
    q[big] = (1.0 - sum).max(0.0).sqrt();
    qnorm(q)
}

/// One sample of a body: the sample index (1/60 s, from the panel's start), its origin and rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyKey { pub i: u32, pub p: V3, pub q: Q }
/// One sample of a fighter's root.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RootKey { pub i: u32, pub p: V3, pub yaw: f64, pub pitch: f64, pub roll: f64, pub lean: [f64; 2], pub flags: u8 }

pub fn pack_body(keys: &[BodyKey]) -> Vec<u8> {
    let mut o = vec![]; if keys.is_empty() { return o; }
    o.extend((keys[0].i as u16).to_le_bytes());
    let mut prev = keys[0].i;
    let mut last = keys[0];
    for k in keys {
        while k.i - prev > 255 { prev += 255; o.push(255); for c in last.p { o.extend(q_mm(c).to_le_bytes()); } o.extend(q_quat(last.q).to_le_bytes()); } // a long rest: repeat the pose
        o.push((k.i - prev) as u8); prev = k.i; last = *k;
        for c in k.p { o.extend(q_mm(c).to_le_bytes()); }
        o.extend(q_quat(k.q).to_le_bytes());
    }
    o
}
pub fn unpack_body(b: &[u8]) -> Vec<BodyKey> {
    let mut o = vec![]; if b.len() < 2 { return o; }
    let mut i = u16::from_le_bytes([b[0], b[1]]) as u32;
    for k in b[2..].chunks_exact(11) {
        i += k[0] as u32; let r = |n: usize| i16::from_le_bytes([k[n], k[n + 1]]) as f64 / 1000.0;
        o.push(BodyKey { i, p: [r(1), r(3), r(5)], q: unq_quat(u32::from_le_bytes([k[7], k[8], k[9], k[10]])) });
    }
    o
}
pub fn pack_root(keys: &[RootKey]) -> Vec<u8> {
    let mut o = vec![]; if keys.is_empty() { return o; }
    o.extend((keys[0].i as u16).to_le_bytes());
    let mut prev = keys[0].i;
    for k in keys {
        o.push((k.i - prev) as u8); prev = k.i;
        for c in k.p { o.extend(q_mm(c).to_le_bytes()); }
        for c in [k.yaw, k.pitch, k.roll] { o.extend(q_mrad(c).to_le_bytes()); }
        o.push(q_lean(k.lean[0]) as u8); o.push(q_lean(k.lean[1]) as u8); o.push(k.flags);
    }
    o
}
pub fn unpack_root(b: &[u8]) -> Vec<RootKey> {
    let mut o = vec![]; if b.len() < 2 { return o; }
    let mut i = u16::from_le_bytes([b[0], b[1]]) as u32;
    for k in b[2..].chunks_exact(16) {
        i += k[0] as u32; let r = |n: usize| i16::from_le_bytes([k[n], k[n + 1]]) as f64 / 1000.0;
        o.push(RootKey { i, p: [r(1), r(3), r(5)], yaw: r(7), pitch: r(9), roll: r(11), lean: [(k[13] as i8) as f64 / 250.0, (k[14] as i8) as f64 / 250.0], flags: k[15] });
    }
    o
}
/// A body's pose at sample time `x` (fractional samples from the panel's start) from its keys; None before the first.
pub fn body_at(keys: &[BodyKey], x: f64) -> Option<(V3, Q)> {
    if keys.is_empty() || x < keys[0].i as f64 { return None; }
    let mut j = 0; while j + 1 < keys.len() && keys[j + 1].i as f64 <= x { j += 1; }
    if j + 1 >= keys.len() { return Some((keys[j].p, keys[j].q)); }
    let (a, b) = (&keys[j], &keys[j + 1]); let f = (x - a.i as f64) / (b.i - a.i) as f64;
    Some((mix(a.p, b.p, f), qnlerp(a.q, b.q, f)))
}

/// Keep only the keys a linear playback needs : every dropped sample is within `tol_p`
/// metres and `tol_q` (about half the angle, radians) of the line between the keys around it. Gaps never exceed 255.
pub fn reduce_body(s: &[BodyKey], tol_p: f64, tol_q: f64) -> Vec<BodyKey> {
    reduce(s, |a, b, m| { let f = (m.i - a.i) as f64 / (b.i - a.i) as f64; (dist(mix(a.p, b.p, f), m.p) / tol_p).max((1.0 - qdot(qnlerp(a.q, b.q, f), m.q).abs()) / (tol_q * tol_q / 2.0)) }, |k| k.i)
}
pub fn reduce_root(s: &[RootKey], tol_p: f64, tol_a: f64, tol_lean: f64) -> Vec<RootKey> {
    reduce(s, |a, b, m| {
        if a.flags != m.flags { return 1e9; }
        let f = (m.i - a.i) as f64 / (b.i - a.i) as f64;
        (dist(mix(a.p, b.p, f), m.p) / tol_p).max((lerp(a.yaw, b.yaw, f) - m.yaw).abs() / tol_a).max((lerp(a.pitch, b.pitch, f) - m.pitch).abs() / tol_a)
            .max((lerp(a.roll, b.roll, f) - m.roll).abs() / tol_a).max((lerp(a.lean[0], b.lean[0], f) - m.lean[0]).abs() / tol_lean).max((lerp(a.lean[1], b.lean[1], f) - m.lean[1]).abs() / tol_lean)
    }, |k| k.i)
}
/// `err(a, b, m)`: how far sample m is from the line a..b, in tolerances (over 1 = a key is needed). Greedy: each
/// key reaches as far as every sample in between still fits.
fn reduce<K: Copy>(s: &[K], err: impl Fn(&K, &K, &K) -> f64, idx: impl Fn(&K) -> u32) -> Vec<K> {
    let mut out = vec![]; if s.is_empty() { return out; }
    let mut a = 0; out.push(s[0]);
    while a + 1 < s.len() {
        let mut b = a + 1;
        while b + 1 < s.len() && idx(&s[b + 1]) - idx(&s[a]) <= 255 && (a + 1..b + 1).all(|m| err(&s[a], &s[b + 1], &s[m]) <= 1.0) { b += 1; }
        out.push(s[b]); a = b;
    }
    out
}
