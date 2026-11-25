use mcle_sdk::IdGen;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct SeededIds {
    state: AtomicU64,
}

impl SeededIds {
    pub fn new(seed: u64) -> Self {
        SeededIds {
            state: AtomicU64::new(seed ^ 0x5EED_5EED_5EED_5EED),
        }
    }
}

impl IdGen for SeededIds {
    fn next_u64(&self) -> u64 {
        let mut z = self
            .state
            .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
            .wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = SeededIds::new(42);
        let b = SeededIds::new(42);
        assert_eq!(a.uuid(), b.uuid());
        assert_eq!(a.hex(8), b.hex(8));
        assert_ne!(SeededIds::new(1).uuid(), SeededIds::new(2).uuid());
        assert_eq!(a.uuid().get_version_num(), 4);
    }
}
