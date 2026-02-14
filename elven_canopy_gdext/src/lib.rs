// elven_canopy_gdext — GDExtension bridge between the sim and Godot.
//
// This crate is a thin wrapper that exposes `elven_canopy_sim` to Godot 4
// via gdext (godot-rust). It contains no simulation logic — only translation
// between Godot types and sim types.
//
// Godot calls into this crate to:
// - Create and manage the simulation state.
// - Send `SimCommand`s translated from player input.
// - Query sim state for rendering (entity positions, world data).
// - Receive `SimEvent`s for the narrative log and UI updates.
//
// See also: `elven_canopy_sim` for all simulation logic.

mod sim_bridge;

use godot::prelude::*;

struct ElvenCanopyExtension;

#[gdextension]
unsafe impl ExtensionLibrary for ElvenCanopyExtension {}
