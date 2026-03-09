// elven_canopy_sim — pure Rust simulation library.
//
// This crate contains all simulation logic for Elven Canopy: world state,
// entity management, event scheduling, the PRNG, and the command interface.
// It has zero Godot dependencies and can be tested, benchmarked, and run
// headless.
//
// Module overview:
// - `sim.rs`:         Top-level SimState, tick loop, command/event processing.
// - `world.rs`:       Dense 3D voxel grid (the world's spatial truth).
// - `tree_gen.rs`:    Energy-based recursive tree generation (trunk, branches, roots, leaves).
// - `mesh_gen.rs`:    Chunk-based voxel mesh generation with per-face culling.
// - `texture_gen.rs`: Procedural face texture generation (3D Perlin noise atlases).
// - `nav.rs`:         Navigation graph structures + construction from tree geometry.
// - `pathfinding.rs`: A* pathfinding over the nav graph.
// - `projectile.rs`:  Integer-only ballistic trajectory math (sub-voxel coords, aim solver, damage).
// - `command.rs`:     SimCommand / SimAction — all sim mutations.
// - `event.rs`:       EventQueue (priority queue) + narrative SimEvents.
// - `config.rs`:      GameConfig + TreeProfile — all tunable parameters including nested tree presets.
// - `species.rs`:     SpeciesData — data-driven creature behavior (DF-style).
// - `db.rs`:          SimDb — tabulosity relational store for all entities (creatures, tasks, structures, etc.).
// - `task.rs`:        Task creation DTOs (`Task`, `TaskKind`) — decomposed into DB tables by `insert_task()`.
// - `blueprint.rs`:   Blueprint data model for the construction system.
// - `structural.rs`:  Spring-mass structural integrity solver.
// - `inventory.rs`:   Item type enum (`ItemKind`). Storage is in `db.rs` tabulosity tables.
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
// seeded xoshiro256++ PRNG (re-exported from `elven_canopy_prng`). No `HashMap`, no system time,
// no OS entropy. Use `BTreeMap` for ordered collections.

pub mod blueprint;
pub mod building;
pub mod checksum;
pub mod command;
pub mod config;
pub mod db;
pub mod event;
pub mod inventory;
pub mod local_relay;
pub mod mesh_gen;
pub mod nav;
pub mod pathfinding;
pub mod projectile;
pub use elven_canopy_prng as prng;
pub use tabulosity;
pub mod session;
pub mod sim;
pub mod species;
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
