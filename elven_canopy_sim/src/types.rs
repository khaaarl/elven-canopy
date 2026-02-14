// Core types shared across the simulation.
//
// Defines spatial coordinates (`VoxelCoord`), entity identifiers (strongly-typed
// UUID v4 wrappers), and other foundational types. All types derive `Serialize`
// and `Deserialize` for save/load and multiplayer state transfer.
//
// **Critical constraint: determinism.** Entity IDs are generated from the sim's
// `GameRng` (see `prng.rs`). Do not use external UUID libraries or OS entropy.

use crate::prng::GameRng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

// ---------------------------------------------------------------------------
// Spatial types
// ---------------------------------------------------------------------------

/// A position in the 3D voxel grid. Each component is in voxel units.
///
/// The coordinate system uses right-handed conventions:
/// - X: east  (positive) / west  (negative)
/// - Y: up    (positive) / down  (negative)
/// - Z: south (positive) / north (negative)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VoxelCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl VoxelCoord {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Manhattan distance between two coordinates.
    pub fn manhattan_distance(self, other: Self) -> u32 {
        ((self.x - other.x).unsigned_abs())
            + ((self.y - other.y).unsigned_abs())
            + ((self.z - other.z).unsigned_abs())
    }
}

impl fmt::Display for VoxelCoord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.x, self.y, self.z)
    }
}

// ---------------------------------------------------------------------------
// Entity IDs — deterministic UUID v4
// ---------------------------------------------------------------------------

/// A UUID v4, generated deterministically from the simulation PRNG.
///
/// Layout follows RFC 4122: 128 bits with version nibble (bits 48–51) set
/// to `0100` and variant bits (bits 64–65) set to `10`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SimUuid([u8; 16]);

impl SimUuid {
    /// Generate a deterministic UUID v4 from the simulation PRNG.
    pub fn new_v4(rng: &mut GameRng) -> Self {
        let mut bytes = rng.next_128_bits();
        // Set version nibble (byte 6, upper nibble) to 0100.
        bytes[6] = (bytes[6] & 0x0F) | 0x40;
        // Set variant bits (byte 8, upper 2 bits) to 10.
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        Self(bytes)
    }

    /// Parse a UUID from its 8-4-4-4-12 hex string representation.
    pub fn from_str(s: &str) -> Option<Self> {
        let hex: String = s.chars().filter(|c| *c != '-').collect();
        if hex.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for i in 0..16 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

// Custom serde: serialize as the 8-4-4-4-12 hex string so SimUuid can be
// used as a JSON map key (serde_json requires string keys).
impl Serialize for SimUuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SimUuid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        SimUuid::from_str(&s).ok_or_else(|| serde::de::Error::custom("invalid UUID format"))
    }
}

impl fmt::Debug for SimUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SimUuid({})", self)
    }
}

impl fmt::Display for SimUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Standard 8-4-4-4-12 hex representation.
        let b = &self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3],
            b[4], b[5],
            b[6], b[7],
            b[8], b[9],
            b[10], b[11], b[12], b[13], b[14], b[15],
        )
    }
}

// ---------------------------------------------------------------------------
// Strongly-typed entity ID wrappers
// ---------------------------------------------------------------------------

macro_rules! entity_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub SimUuid);

        impl $name {
            pub fn new(rng: &mut GameRng) -> Self {
                Self(SimUuid::new_v4(rng))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

entity_id!(/// Unique identifier for a tree entity.
TreeId);
entity_id!(/// Unique identifier for an elf entity.
ElfId);
entity_id!(/// Unique identifier for a player (tree spirit).
PlayerId);
entity_id!(/// Unique identifier for a structure (platform, bridge, etc.).
StructureId);
entity_id!(/// Unique identifier for an in-progress build project.
ProjectId);

// ---------------------------------------------------------------------------
// Nav graph IDs — simple integers, not UUIDs, for compactness.
// ---------------------------------------------------------------------------

/// Compact identifier for a navigation graph node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NavNodeId(pub u32);

/// Compact identifier for a navigation graph edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NavEdgeId(pub u32);

// ---------------------------------------------------------------------------
// Simulation enums
// ---------------------------------------------------------------------------

/// Simulation speed settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimSpeed {
    Paused,
    Normal,
    Fast,
    VeryFast,
}

/// Priority level for build projects and tasks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Normal,
    High,
    Urgent,
}

/// Types of structures that can be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildType {
    Platform,
    Bridge,
    Stairs,
    Wall,
    Enclosure,
}

// ---------------------------------------------------------------------------
// Voxel types
// ---------------------------------------------------------------------------

/// The material/type of a single voxel in the world grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoxelType {
    Air,
    Trunk,
    Branch,
    GrownPlatform,
    GrownWall,
    GrownStairs,
    Bridge,
    ForestFloor,
}

impl Default for VoxelType {
    fn default() -> Self {
        Self::Air
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v4_version_and_variant_bits() {
        let mut rng = GameRng::new(42);
        for _ in 0..1000 {
            let uuid = SimUuid::new_v4(&mut rng);
            let bytes = uuid.as_bytes();
            // Version nibble (byte 6 upper) must be 0x4_.
            assert_eq!(bytes[6] >> 4, 4, "UUID version must be 4");
            // Variant bits (byte 8 upper 2) must be 0b10.
            assert_eq!(bytes[8] >> 6, 2, "UUID variant must be RFC 4122");
        }
    }

    #[test]
    fn uuid_determinism() {
        let mut rng_a = GameRng::new(42);
        let mut rng_b = GameRng::new(42);
        for _ in 0..100 {
            assert_eq!(SimUuid::new_v4(&mut rng_a), SimUuid::new_v4(&mut rng_b));
        }
    }

    #[test]
    fn entity_id_determinism() {
        let mut rng_a = GameRng::new(99);
        let mut rng_b = GameRng::new(99);
        assert_eq!(TreeId::new(&mut rng_a), TreeId::new(&mut rng_b));
        assert_eq!(ElfId::new(&mut rng_a), ElfId::new(&mut rng_b));
    }

    #[test]
    fn uuid_display_format() {
        let mut rng = GameRng::new(42);
        let uuid = SimUuid::new_v4(&mut rng);
        let s = uuid.to_string();
        // 8-4-4-4-12 hex = 32 hex chars + 4 dashes = 36 chars
        assert_eq!(s.len(), 36);
        assert_eq!(&s[8..9], "-");
        assert_eq!(&s[13..14], "-");
        assert_eq!(&s[18..19], "-");
        assert_eq!(&s[23..24], "-");
    }

    #[test]
    fn uuid_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let uuid = SimUuid::new_v4(&mut rng);
        let json = serde_json::to_string(&uuid).unwrap();
        let restored: SimUuid = serde_json::from_str(&json).unwrap();
        assert_eq!(uuid, restored);
    }

    #[test]
    fn voxel_coord_manhattan_distance() {
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(3, 4, 5);
        assert_eq!(a.manhattan_distance(b), 12);
        assert_eq!(b.manhattan_distance(a), 12);
    }

    #[test]
    fn voxel_coord_ordering() {
        // Verify VoxelCoord has a total order (needed for BTreeMap keys).
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(1, 0, 0);
        assert!(a < b);
    }
}
