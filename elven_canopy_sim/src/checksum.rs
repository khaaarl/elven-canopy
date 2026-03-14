// State checksum utilities for multiplayer desync detection.
//
// Provides FNV-1a hashing and the checksum interval constant used by the
// multiplayer pipeline. Clients periodically hash their `SimState` (via
// `SimState::state_checksum()` in `sim/mod.rs`) and send the hash to the relay.
// The relay compares hashes from all players at the same tick and broadcasts
// `DesyncDetected` on mismatch (see `elven_canopy_relay::session`).
//
// FNV-1a was chosen for simplicity (6 lines, no deps) and good distribution.
// It's not cryptographic, but desync detection doesn't need that — it just
// needs collision resistance over the space of plausible sim states.
//
// **Determinism note:** The hash input comes from `serde_json::to_vec()` on
// `SimState`, which is deterministic because all collections are `BTreeMap`
// (sorted keys) and floats use Ryu (stable formatting). This is the same
// serialization path used by save/load and the `to_json()` convenience method.

/// How often (in sim ticks) clients should compute and send a state checksum.
/// At 1000 ticks/sim-second, this is once per sim-second.
pub const CHECKSUM_INTERVAL_TICKS: u64 = 1000;

/// FNV-1a 64-bit hash. Simple, fast, no dependencies.
pub fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_empty() {
        // FNV-1a of empty input should be the offset basis.
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn fnv1a_known_value() {
        // Sanity: different inputs produce different hashes.
        let a = fnv1a_64(b"hello");
        let b = fnv1a_64(b"world");
        assert_ne!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn fnv1a_deterministic() {
        let data = b"determinism matters";
        assert_eq!(fnv1a_64(data), fnv1a_64(data));
    }
}
