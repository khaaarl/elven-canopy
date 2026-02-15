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
// - `tree_gen.rs`:    Procedural tree generation (trunk + branches).
// - `nav.rs`:         Navigation graph structures + construction from tree geometry.
// - `pathfinding.rs`: A* pathfinding over the nav graph.
// - `command.rs`:     SimCommand / SimAction — all sim mutations.
// - `event.rs`:       EventQueue (priority queue) + narrative SimEvents.
// - `config.rs`:      GameConfig — all tunable parameters.
// - `species.rs`:     SpeciesData — data-driven creature behavior (DF-style).
// - `task.rs`:        Task entities — units of work assigned to creatures.
// - `prng.rs`:        Xoshiro256++ PRNG with SplitMix64 seeding.
// - `types.rs`:       VoxelCoord, entity IDs, voxel types, Species enum.
//
// The companion crate `elven_canopy_gdext` wraps this library for Godot
// via GDExtension. That boundary is enforced at the compiler level — this
// crate cannot depend on rendering, frame timing, or Godot's RNG.
//
// **Critical constraint: determinism.** The simulation is a pure function:
// `(state, commands) -> (new_state, events)`. All randomness comes from a
// seeded xoshiro256++ PRNG (see `prng.rs`). No `HashMap`, no system time,
// no OS entropy. Use `BTreeMap` for ordered collections.

pub mod command;
pub mod config;
pub mod event;
pub mod nav;
pub mod pathfinding;
pub mod prng;
pub mod sim;
pub mod species;
pub mod task;
pub mod tree_gen;
pub mod types;
pub mod world;
