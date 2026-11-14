//! Deterministic randomness that never changes across crate versions (SplitMix64), so recordings replay exactly
//! and a cluster's n-th materialized person is always the same person.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    /// A stream derived from a seed and labels (e.g. `derive(cluster.seed, &[index])`).
    pub fn derive(seed: u64, parts: &[u64]) -> Self {
        let mut r = Self(seed ^ 0x9E37_79B9_7F4A_7C15);
        for p in parts {
            r.0 ^= p.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            r.next_u64();
        }
        r
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform in [0, n).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next_u64() % n
        }
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> Option<&'a T> {
        if xs.is_empty() {
            None
        } else {
            xs.get(self.below(xs.len() as u64) as usize)
        }
    }
}

/// Stable 64-bit hash of a string (FNV-1a), for seeds derived from names.
pub fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable() {
        let mut a = Rng::derive(42, &[7]);
        let mut b = Rng::derive(42, &[7]);
        let xs: Vec<u64> = (0..5).map(|_| a.next_u64()).collect();
        assert_eq!(xs, (0..5).map(|_| b.next_u64()).collect::<Vec<_>>());
        assert_ne!(Rng::derive(42, &[8]).next_u64(), xs[0]);
        // Pinned so a refactor can't silently change every recorded match.
        assert_eq!(Rng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(hash_str("time"), hash_str("time"));
    }
}
