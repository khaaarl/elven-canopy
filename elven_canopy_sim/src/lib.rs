// elven_canopy_sim — pure Rust simulation library.
//
// This crate contains all simulation logic for Elven Canopy: world state,
// entity management, event scheduling, the PRNG, and the command interface.
// It has zero Godot dependencies and can be tested, benchmarked, and run
// headless.
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
pub mod prng;
pub mod sim;
pub mod types;
