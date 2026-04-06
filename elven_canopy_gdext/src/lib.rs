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
// - `session_browser.rs`: `SessionBrowser` RefCounted class for browsing
//                     relay sessions from menu scenes (before `SimBridge`
//                     exists). Used by `join_game_menu.gd`.
// - `llm_worker.rs`:  Long-lived LLM inference worker thread. Receives
//                     dispatched requests, runs inference via `elven_canopy_llm`,
//                     and sends results back for relay routing.
// - `mesh_cache.rs`:  Chunk mesh cache — caches `ChunkMesh` data per 16x16x16
//                     chunk and tracks dirty chunks for incremental updates.
//                     Used by `sim_bridge.rs` to serve chunk ArrayMesh data.
// - `elfcyclopedia_server.rs`: Embedded localhost HTTP server serving a species
//                     bestiary. Runs on a background thread with read-only
//                     access to a shared data snapshot. Owned by SimBridge.
// - `sprite_bridge.rs`: `SpriteGenerator` utility class — converts
//                     `elven_canopy_sprites` pixel buffers into Godot
//                     ImageTextures. Replaces GDScript `SpriteFactory`.
//
// The TCP relay client (`NetClient`) lives in `elven_canopy_relay::client`
// so it can be shared with integration tests without a Godot dependency.
//
// See also: `elven_canopy_sim` for all simulation logic,
// `elven_canopy_protocol` for wire message types,
// `elven_canopy_relay` for the relay server and client.

mod elfcyclopedia_server;
mod llm_worker;
mod mesh_cache;
mod session_browser;
mod sim_bridge;
mod sprite_bridge;

use godot::prelude::*;

struct ElvenCanopyExtension;

#[gdextension]
unsafe impl ExtensionLibrary for ElvenCanopyExtension {}
