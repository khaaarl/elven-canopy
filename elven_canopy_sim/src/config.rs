// Data-driven game configuration.
//
// All tunable simulation parameters live here in `GameConfig`, loaded from
// JSON at startup. The sim never uses magic numbers — it reads from the
// config. This enables balance iteration without recompilation, and in
// multiplayer all clients must have identical configs (enforced via hash
// comparison at session handshake).
//
// See also: `sim.rs` which owns the `GameConfig` as part of `SimState`.
//
// **Critical constraint: determinism.** Config values feed directly into
// simulation logic. All clients must use identical configs for identical
// results.

use serde::{Deserialize, Serialize};

/// Top-level game configuration. Loaded from JSON, never mutated at runtime.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameConfig {
    /// Number of real-world milliseconds per simulation tick.
    pub tick_duration_ms: u32,

    /// Interval (in ticks) between elf heartbeat events (need decay, mood
    /// drift, mana generation).
    pub heartbeat_interval_ticks: u64,

    /// Base elf movement speed in voxels per tick on flat surfaces.
    pub elf_base_speed: f32,

    /// Multiplier applied to movement speed when climbing raw trunk.
    pub climb_speed_multiplier: f32,

    /// Multiplier applied to movement speed when using stairs/ramps.
    pub stair_speed_multiplier: f32,

    /// Base mana generated per elf per heartbeat tick.
    pub mana_base_generation_rate: f32,

    /// Range of mood-based multipliers on mana generation.
    /// `(min_multiplier, max_multiplier)` — interpolated from worst to best mood.
    pub mana_mood_multiplier_range: (f32, f32),

    /// Mana cost to grow one voxel of platform.
    pub platform_mana_cost_per_voxel: f32,

    /// Mana cost to grow one voxel of bridge/walkway.
    pub bridge_mana_cost_per_voxel: f32,

    /// Base rate of fruit production per tree per heartbeat tick.
    pub fruit_production_base_rate: f32,

    /// World dimensions in voxels (x, y, z).
    pub world_size: (u32, u32, u32),

    /// Initial mana stored in the player's home tree.
    pub starting_mana: f32,

    /// Maximum mana the starting tree can hold.
    pub starting_mana_capacity: f32,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            tick_duration_ms: 100,
            heartbeat_interval_ticks: 100,
            elf_base_speed: 0.1,
            climb_speed_multiplier: 0.4,
            stair_speed_multiplier: 0.7,
            mana_base_generation_rate: 1.0,
            mana_mood_multiplier_range: (0.2, 2.0),
            platform_mana_cost_per_voxel: 10.0,
            bridge_mana_cost_per_voxel: 15.0,
            fruit_production_base_rate: 0.5,
            world_size: (256, 128, 256),
            starting_mana: 100.0,
            starting_mana_capacity: 500.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_serializes() {
        let config = GameConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let restored: GameConfig = serde_json::from_str(&json).unwrap();
        // Verify a few fields survived the roundtrip.
        assert_eq!(config.tick_duration_ms, restored.tick_duration_ms);
        assert_eq!(
            config.heartbeat_interval_ticks,
            restored.heartbeat_interval_ticks
        );
        assert_eq!(config.world_size, restored.world_size);
    }

    #[test]
    fn config_loads_from_json_string() {
        let json = r#"{
            "tick_duration_ms": 50,
            "heartbeat_interval_ticks": 200,
            "elf_base_speed": 0.2,
            "climb_speed_multiplier": 0.3,
            "stair_speed_multiplier": 0.6,
            "mana_base_generation_rate": 2.0,
            "mana_mood_multiplier_range": [0.1, 3.0],
            "platform_mana_cost_per_voxel": 8.0,
            "bridge_mana_cost_per_voxel": 12.0,
            "fruit_production_base_rate": 0.8,
            "world_size": [128, 64, 128],
            "starting_mana": 200.0,
            "starting_mana_capacity": 1000.0
        }"#;
        let config: GameConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.tick_duration_ms, 50);
        assert_eq!(config.world_size, (128, 64, 128));
    }
}
