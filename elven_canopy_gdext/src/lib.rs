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
// - Connect to a multiplayer relay and exchange turns (via `client.rs` in
//   the relay crate).
//
// Module overview:
// - `sim_bridge.rs`:  The `SimBridge` Godot node — sole interface between
//                     GDScript and Rust. Handles both single-player (direct
//                     sim) and multiplayer (relay-routed) command paths.
//
// The TCP relay client (`NetClient`) lives in `elven_canopy_relay::client`
// so it can be shared with integration tests without a Godot dependency.
//
// See also: `elven_canopy_sim` for all simulation logic,
// `elven_canopy_protocol` for wire message types,
// `elven_canopy_relay` for the relay server and client.

mod sim_bridge;

use godot::prelude::*;

struct ElvenCanopyExtension;

#[gdextension]
unsafe impl ExtensionLibrary for ElvenCanopyExtension {}
