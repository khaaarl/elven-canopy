// Core ID types for the multiplayer protocol.
//
// These are lightweight newtypes used by both `message.rs` (protocol messages)
// and the relay's session management (`elven_canopy_relay::session`). They are
// relay-scoped identifiers, not sim UUIDs — the relay assigns compact integer
// IDs to players and turns for efficient wire representation.

use serde::{Deserialize, Serialize};

/// Relay-assigned player ID (compact u32, not a sim UUID).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RelayPlayerId(pub u32);

/// Monotonically increasing turn number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TurnNumber(pub u64);

/// Per-player monotonic command sequence number, preserving local ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ActionSequence(pub u64);
