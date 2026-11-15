//! How cells are stored: sparse, delta-encoded, quantized (every layer is one byte), base64 in the JSON state.
//!
//! A packed block lists the cells whose bytes differ between two fields (`prev` → `cur`), in index order:
//!
//! ```text
//! varint  index − previous index in the block (the first: the index itself)
//! varint  mask: bit k set = layer k differs
//! bytes   the new value of each layer in the mask, lowest bit first (0 = back to base)
//! ```
//!
//! The state's `cells` is the block against an empty field (every dirty cell, its non-zero layers); a keyframe's is
//! the block against the keyframe before it. Varints are LEB128.

use base64::Engine;

use crate::sim::N_LAYERS;

pub type Sparse = Vec<(u16, [u8; N_LAYERS])>;

fn put(out: &mut Vec<u8>, mut x: u32) {
    loop {
        let b = (x & 0x7F) as u8;
        x >>= 7;
        if x == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
}

fn get(b: &[u8], at: &mut usize) -> Option<u32> {
    let (mut x, mut shift) = (0u32, 0);
    loop {
        let v = *b.get(*at)?;
        *at += 1;
        x |= ((v & 0x7F) as u32) << shift;
        if v & 0x80 == 0 {
            return Some(x);
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
}

/// The block `prev` → `cur` as bytes (both sorted by index).
pub fn pack_bytes(prev: &[(u16, [u8; N_LAYERS])], cur: &[(u16, [u8; N_LAYERS])]) -> Vec<u8> {
    const ZERO: [u8; N_LAYERS] = [0; N_LAYERS];
    let mut out = Vec::with_capacity(cur.len() * 6);
    let (mut a, mut b, mut last) = (0, 0, 0u32);
    while a < prev.len() || b < cur.len() {
        let ia = prev.get(a).map(|x| x.0 as u32).unwrap_or(u32::MAX);
        let ib = cur.get(b).map(|x| x.0 as u32).unwrap_or(u32::MAX);
        let i = ia.min(ib);
        let old = if ia == i { a += 1; &prev[a - 1].1 } else { &ZERO };
        let new = if ib == i { b += 1; &cur[b - 1].1 } else { &ZERO };
        if old == new {
            continue;
        }
        let mut mask = 0u32;
        for k in 0..N_LAYERS {
            if old[k] != new[k] {
                mask |= 1 << k;
            }
        }
        put(&mut out, i - last);
        last = i;
        put(&mut out, mask);
        for k in 0..N_LAYERS {
            if mask & (1 << k) != 0 {
                out.push(new[k]);
            }
        }
    }
    out
}

/// Apply a block to `prev`; cells back at base are dropped. A damaged block applies as far as it reads.
pub fn apply_bytes(prev: Sparse, block: &[u8]) -> Sparse {
    let mut changes: Sparse = vec![];
    let (mut at, mut last) = (0usize, 0u32);
    let mut map: std::collections::BTreeMap<u16, [u8; N_LAYERS]> = prev.into_iter().collect();
    while at < block.len() {
        let (Some(d), Some(mask)) = (get(block, &mut at), get(block, &mut at)) else { break };
        last += d;
        if last > u16::MAX as u32 {
            break;
        }
        let mut cell = map.get(&(last as u16)).copied().unwrap_or([0; N_LAYERS]);
        let mut ok = true;
        for (k, v) in cell.iter_mut().enumerate() {
            if mask & (1 << k) != 0 {
                match block.get(at) {
                    Some(x) => *v = *x,
                    None => ok = false,
                }
                at += 1;
            }
        }
        if !ok {
            break;
        }
        changes.push((last as u16, cell));
    }
    for (i, cell) in changes {
        if cell == [0; N_LAYERS] {
            map.remove(&i);
        } else {
            map.insert(i, cell);
        }
    }
    map.into_iter().collect()
}

pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes)
}

pub fn un64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD_NO_PAD.decode(s.trim_end_matches('=')).unwrap_or_default()
}

pub fn pack(prev: &[(u16, [u8; N_LAYERS])], cur: &[(u16, [u8; N_LAYERS])]) -> String {
    b64(&pack_bytes(prev, cur))
}

pub fn apply(prev: Sparse, block: &str) -> Sparse {
    apply_bytes(prev, &un64(block))
}

pub fn unpack(block: &str) -> Sparse {
    apply(vec![], block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_round_trip_and_chain() {
        let cell = |x: u8| {
            let mut c = [0u8; N_LAYERS];
            c[x as usize % N_LAYERS] = x;
            c[2] = 7;
            c
        };
        let a: Sparse = vec![(3, cell(1)), (700, cell(9)), (26_000, cell(200))];
        let b: Sparse = vec![(3, cell(1)), (4, cell(5)), (26_000, cell(201))];
        assert_eq!(unpack(&pack(&[], &a)), a);
        let d = pack(&a, &b);
        assert_eq!(apply(a.clone(), &d), b);
        assert!(pack(&a, &a).is_empty());
        // A cell with two non-zero layers costs 1–3 bytes of index, 1–3 of mask and 2 of values.
        assert!(pack_bytes(&[], &a).len() <= 3 * 8);
        // Garbage never panics.
        let _ = apply(a, "!!!not base64");
        let _ = apply_bytes(vec![], &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }
}
