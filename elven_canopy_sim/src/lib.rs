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
// - `nav.rs`:         Navigation graph structures + construction from tree geometry.
// - `pathfinding.rs`: A* pathfinding over the nav graph.
// - `command.rs`:     SimCommand / SimAction — all sim mutations.
// - `mesh_smooth.rs`: Smooth mesh generation for wood/root voxels (vertex displacement at diagonal neighbors).
// - `event.rs`:       EventQueue (priority queue) + narrative SimEvents.
// - `config.rs`:      GameConfig + TreeProfile — all tunable parameters including nested tree presets.
// - `species.rs`:     SpeciesData — data-driven creature behavior (DF-style).
// - `task.rs`:        Task entities — units of work assigned to creatures.
// - `blueprint.rs`:   Blueprint data model for the construction system.
// - `structural.rs`:  Spring-mass structural integrity solver.
// - `prng`:           Re-exported from `elven_canopy_prng` — xoshiro256++ PRNG with SplitMix64 seeding.
// - `types.rs`:       VoxelCoord, entity IDs, voxel types, Species enum.
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
pub mod command;
pub mod config;
pub mod event;
pub mod mesh_smooth;
pub mod nav;
pub mod pathfinding;
pub use elven_canopy_prng as prng;
pub mod sim;
pub mod species;
pub mod structural;
pub mod task;
pub mod tree_gen;
pub mod types;
pub mod world;
