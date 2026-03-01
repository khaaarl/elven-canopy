// elven_canopy_relay — multiplayer relay coordinator for Elven Canopy.
//
// This crate implements the relay server described in
// `docs/drafts/multiplayer_relay.md`. The relay is a thin message broker: it
// accepts TCP connections from game clients, collects sim commands, batches
// them into numbered turns at a fixed cadence, and broadcasts each turn to all
// connected clients. It never runs the sim — all game logic stays on the
// clients.
//
// Module overview:
// - `session.rs`:  Session state — player roster, turn batching, command
//                  queuing, checksum-based desync detection. The core data
//                  structure that `server.rs` drives.
// - `server.rs`:   TCP listener, reader threads (one per client), and the
//                  main event loop. Uses `std::net` with a thread-per-reader
//                  architecture and an `mpsc` channel to funnel events into
//                  the single-threaded `Session`.
//
// Dependencies: `elven_canopy_protocol` (shared message types and framing).
// No dependency on the sim crate or Godot.
//
// The relay can run as a standalone binary (`main.rs`) or be embedded in a
// game process via the library API (`start_relay`).

pub mod server;
pub mod session;

pub use server::start_relay;
