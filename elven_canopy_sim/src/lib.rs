// elven_canopy_sim — pure Rust simulation library.
//
// This crate contains all simulation logic for Elven Canopy: world state,
// entity management, event scheduling, the PRNG, and the command interface.
// It has zero Godot dependencies and can be tested, benchmarked, and run
// headless.
//
// Module overview:
// - `sim/`:           SimState and all simulation logic, split into focused sub-modules
//                     (mod.rs, activation, combat, construction, crafting, creature,
//                     greenhouse, inventory_mgmt, logistics, movement, needs, task_helpers).
// - `world.rs`:       RLE column-based voxel grid (the world's spatial truth, Y-up).
// - `tree_gen.rs`:    Energy-based recursive tree generation (trunk, branches, roots, leaves).
// - `mesh_gen.rs`:    Chunk-based voxel mesh generation with smooth surface rendering.
// - `smooth_mesh.rs`: Smooth mesh pipeline: subdivision, anchoring, chamfer, smoothing.
// - `mesh_decimation.rs`: QEM edge-collapse decimation + coplanar retri + collinear collapse.
// - `texture_gen.rs`: Procedural face texture generation (kept for reference, not active).
// - `nav.rs`:         Navigation graph structures + construction from tree geometry.
// - `flight_pathfinding.rs`: Vanilla A* on voxel grid for flying creatures (26-connected).
// - `pathfinding.rs`: A* pathfinding over the nav graph.
// - `preemption.rs`:  Task priority levels and preemption rules (Mood vs Survival, PlayerDirected override, etc.).
// - `projectile.rs`:  Integer-only ballistic trajectory math (sub-voxel coords, aim solver).
// - `command.rs`:     SimCommand / SimAction — all sim mutations.
// - `event.rs`:       EventQueue (priority queue) + narrative SimEvents.
// - `fruit.rs`:       Procedural fruit species: types, generation, coverage, Vaelith naming.
// - `config.rs`:      GameConfig + TreeProfile + FruitConfig — all tunable parameters.
// - `species.rs`:     SpeciesData — data-driven creature behavior (DF-style).
// - `stats.rs`:       Creature stat multiplier table (2^20 fixed-point exponential).
// - `db.rs`:          SimDb — tabulosity relational store for all entities (creatures, tasks, structures, etc.).
// - `task.rs`:        Task creation DTOs (`Task`, `TaskKind`) — decomposed into DB tables by `insert_task()`.
// - `blueprint.rs`:   Blueprint data model for the construction system.
// - `structural.rs`:  Spring-mass structural integrity solver.
// - `inventory.rs`:   Item type enum (`ItemKind`). Storage is in `db.rs` tabulosity tables.
// - `lookup_map.rs`:  LookupMap — non-iterable HashMap wrapper for deterministic point-query access.
// - `session.rs`:     GameSession — message-driven session management (players, commands, pause/resume).
// - `local_relay.rs`: LocalRelay — accumulator-based tick pacer for single-player mode.
// - `checksum.rs`:    FNV-1a hashing + interval constant for multiplayer desync detection.
// - `prng`:           Re-exported from `elven_canopy_prng` — xoshiro256++ PRNG with SplitMix64 seeding.
// - `types.rs`:       VoxelCoord, entity IDs, voxel types, Species enum.
// - `worldgen.rs`:    Worldgen framework — generator sequencing, worldgen PRNG, WorldgenConfig.
//
// The companion crate `elven_canopy_gdext` wraps this library for Godot
// via GDExtension. That boundary is enforced at the compiler level — this
// crate cannot depend on rendering, frame timing, or Godot's RNG.
//
// **Critical constraint: determinism.** The simulation is a pure function:
// `(state, commands) -> (new_state, events)`. All randomness comes from a
// seeded xoshiro256++ PRNG (re-exported from `elven_canopy_prng`). No iterating `HashMap` —
// use `BTreeMap` for ordered iteration, `LookupMap` for point-query-only O(1) access.
// No system time, no OS entropy.

pub mod blueprint;
pub mod building;
pub mod checksum;
pub mod command;
pub mod config;
pub mod db;
pub mod event;
pub mod flight_pathfinding;
pub mod fruit;
pub mod inventory;
pub mod local_relay;
pub mod lookup_map;
pub mod mesh_decimation;
pub mod mesh_gen;
pub mod nav;
pub mod pathfinding;
pub mod preemption;
pub mod projectile;
pub use elven_canopy_prng as prng;
pub use tabulosity;
pub mod recipe;
pub mod session;
pub mod sim;
pub mod smooth_mesh;
pub mod species;
pub mod stats;
pub mod structural;
pub mod task;
pub mod texture_gen;
pub mod tree_gen;
pub mod types;
pub mod world;
pub mod worldgen;

#[cfg(test)]
mod tests {
    #[test]
    fn test_lang_crate_name_generation() {
        use elven_canopy_lang::{default_lexicon, names::generate_name};
        use elven_canopy_prng::GameRng;

        let lexicon = default_lexicon();
        let mut rng = GameRng::new(42);
        let name = generate_name(&lexicon, &mut rng);

        assert!(!name.full_name.is_empty(), "Name should not be empty");
        assert!(!name.given.is_empty(), "Given name should not be empty");
        assert!(!name.surname.is_empty(), "Surname should not be empty");
        assert!(
            name.full_name.contains(' '),
            "Full name should have a space"
        );

        // Determinism: same seed produces same name
        let mut rng2 = GameRng::new(42);
        let name2 = generate_name(&lexicon, &mut rng2);
        assert_eq!(name.full_name, name2.full_name);
    }
}
