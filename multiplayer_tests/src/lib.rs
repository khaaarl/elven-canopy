// Test-only game client for multiplayer integration tests.
//
// Wraps the real `NetClient` (from `elven_canopy_relay::client`) and a real
// `SimState` (from `elven_canopy_sim::sim`) to provide a synchronous,
// test-friendly API for exercising the full multiplayer pipeline:
// host → relay → join → command → turn → sim.step() → verify state.
//
// Also provides checksum helpers (`send_checksum()`, `state_checksum()`) for
// testing desync detection: compute the sim's FNV-1a hash and send it to the
// relay, which compares hashes from all players and broadcasts `DesyncDetected`
// on mismatch. Snapshot helpers (`handle_snapshot_request()`,
// `poll_until_snapshot_load()`) support mid-game join tests where a late
// joiner receives a full sim state from the host.
//
// The only test-specific code here is the synchronous polling wrappers
// (blocking loops around `NetClient::poll()`). All networking and sim
// logic uses the same code paths as the real game.
//
// See also: `tests/full_pipeline.rs` for the integration test scenarios.

use std::thread;
use std::time::{Duration, Instant};

use elven_canopy_protocol::message::ServerMessage;
use elven_canopy_relay::client::{NetClient, RelayConnection};
use elven_canopy_sim::command::SimAction;
use elven_canopy_sim::config::GameConfig;
use elven_canopy_sim::sim::SimState;

/// Default timeout for blocking poll operations.
const POLL_TIMEOUT: Duration = Duration::from_secs(5);

/// Sleep duration between poll attempts.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// A test game client wrapping a real NetClient and SimState.
pub struct TestGameClient {
    client: NetClient,
    pub sim: Option<SimState>,
    pub ticks_per_turn: u32,
}

impl TestGameClient {
    /// Connect to a relay server and perform the Hello handshake.
    /// For joiners connecting to an embedded relay (SessionId(0)).
    pub fn connect(addr: std::net::SocketAddr, name: &str) -> Self {
        let addr_str = addr.to_string();
        let (client, info) = NetClient::connect(&addr_str, name, 1, 0, None)
            .expect("TestGameClient::connect failed");
        Self {
            client,
            sim: None,
            ticks_per_turn: info.ticks_per_turn,
        }
    }

    /// Connect to an embedded relay, create a session, and join it.
    /// For hosts that need to set up the session before joiners connect.
    pub fn connect_and_create(
        addr: std::net::SocketAddr,
        name: &str,
        ticks_per_turn: u32,
        max_players: u32,
    ) -> Self {
        let addr_str = addr.to_string();
        let mut conn =
            RelayConnection::connect(&addr_str).expect("TestGameClient::connect_and_create failed");
        let session_id = conn
            .create_session(name, None, ticks_per_turn, max_players)
            .expect("create_session failed");
        let (client, info) = conn
            .join_session(session_id, name, 1, 0, None)
            .expect("join_session failed");
        Self {
            client,
            sim: None,
            ticks_per_turn: info.ticks_per_turn,
        }
    }

    /// Host only: send StartGame to begin the multiplayer session.
    pub fn send_start_game(&mut self, seed: i64, config_json: &str) {
        self.client
            .send_start_game(seed, config_json, None)
            .expect("send_start_game failed");
    }

    /// Send a SimAction to the relay as a serialized command payload.
    pub fn send_action(&mut self, action: &SimAction) {
        let json = serde_json::to_vec(action).expect("serialize SimAction failed");
        self.client
            .send_command(&json)
            .expect("send_command failed");
    }

    /// Blocking poll until a GameStart message is received. Initializes the
    /// sim with a small test world using the received seed/config. Returns
    /// the (seed, config_json) from the GameStart message.
    pub fn poll_until_game_start(&mut self, world_size: (u32, u32, u32)) -> (i64, String) {
        let start = Instant::now();
        loop {
            assert!(
                start.elapsed() < POLL_TIMEOUT,
                "timed out waiting for GameStart"
            );
            for msg in self.client.poll() {
                if let ServerMessage::GameStart {
                    seed, config_json, ..
                } = msg
                {
                    let mut config = GameConfig {
                        world_size,
                        ..GameConfig::default()
                    };
                    // Use reduced tree energy for fast test world generation.
                    config.tree_profile.growth.initial_energy = 50.0;
                    self.sim = Some(SimState::with_config(seed as u64, config));
                    return (seed, config_json);
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Blocking poll until a Turn message with commands is received. Applies
    /// *all* turns encountered (including empty ones that arrive first) to
    /// keep the sim tick in sync with the relay. Returns the sim_tick_target
    /// of the first turn that contained commands.
    ///
    /// **Caution:** This returns as soon as *any* turn with commands arrives.
    /// When multiple players send commands concurrently, their commands may
    /// land in different relay turns. Calling `poll_until_turn()` on each
    /// client will only guarantee that *one* turn with commands was applied —
    /// later turns (containing other players' commands) may not have arrived
    /// yet. For tests where multiple players send commands in the same window,
    /// use the pause-then-drain pattern instead: sleep briefly, call
    /// `send_pause()` / `poll_until_paused()` on all clients, then
    /// `drain_turns()` to deterministically apply all buffered turns.
    pub fn poll_until_turn(&mut self) -> u64 {
        let start = Instant::now();
        loop {
            assert!(
                start.elapsed() < POLL_TIMEOUT,
                "timed out waiting for Turn with commands"
            );
            let mut found_tick = None;
            for msg in self.client.poll() {
                if let ServerMessage::Turn {
                    sim_tick_target,
                    commands,
                    ..
                } = msg
                {
                    let sim = self.sim.as_mut().expect("sim not initialized");
                    let payloads: Vec<&[u8]> =
                        commands.iter().map(|tc| tc.payload.as_slice()).collect();
                    sim.apply_turn_payloads(sim_tick_target, &payloads);
                    if found_tick.is_none() && !commands.is_empty() {
                        found_tick = Some(sim_tick_target);
                    }
                }
            }
            if let Some(tick) = found_tick {
                return tick;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Non-blocking: drain and apply all pending turns. Non-Turn messages
    /// are collected and returned so callers can inspect them. Returns
    /// (turns_applied, other_messages).
    pub fn drain_turns(&mut self) -> (usize, Vec<ServerMessage>) {
        let mut count = 0;
        let mut other = Vec::new();
        let messages = self.client.poll();
        for msg in messages {
            if let ServerMessage::Turn {
                sim_tick_target,
                commands,
                ..
            } = msg
            {
                let sim = self.sim.as_mut().expect("sim not initialized");
                let payloads: Vec<&[u8]> =
                    commands.iter().map(|tc| tc.payload.as_slice()).collect();
                sim.apply_turn_payloads(sim_tick_target, &payloads);
                count += 1;
            } else {
                other.push(msg);
            }
        }
        (count, other)
    }

    /// Raw poll: return all pending server messages without processing them.
    /// Only use before the sim is initialized (e.g., draining lobby events).
    pub fn poll_raw(&self) -> Vec<ServerMessage> {
        self.client.poll()
    }

    /// Send a state checksum to the relay for desync detection.
    pub fn send_checksum(&mut self, tick: u64, hash: u64) {
        self.client
            .send_checksum(tick, hash)
            .expect("send_checksum failed");
    }

    /// Compute the state checksum of the local sim.
    pub fn state_checksum(&self) -> u64 {
        self.sim
            .as_ref()
            .expect("sim not initialized")
            .state_checksum()
    }

    /// Handle a mid-game join snapshot request from the relay. Blocking poll
    /// until `SnapshotRequest` arrives (applying any in-flight turns while
    /// waiting), then serialize the local sim and send a `SnapshotResponse`.
    pub fn handle_snapshot_request(&mut self) {
        let start = Instant::now();
        loop {
            assert!(
                start.elapsed() < POLL_TIMEOUT,
                "timed out waiting for SnapshotRequest"
            );
            for msg in self.client.poll() {
                match msg {
                    ServerMessage::SnapshotRequest => {
                        let sim = self.sim.as_ref().expect("sim not initialized");
                        let json = sim.to_json().expect("sim serialization failed");
                        let data = json.into_bytes();
                        self.client
                            .send_snapshot_response(&data)
                            .expect("send_snapshot_response failed");
                        return;
                    }
                    ServerMessage::Turn {
                        sim_tick_target,
                        commands,
                        ..
                    } => {
                        // Apply in-flight turns while waiting for the request.
                        let sim = self.sim.as_mut().expect("sim not initialized");
                        let payloads: Vec<&[u8]> =
                            commands.iter().map(|tc| tc.payload.as_slice()).collect();
                        sim.apply_turn_payloads(sim_tick_target, &payloads);
                    }
                    _ => {}
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Blocking poll until a `SnapshotLoad` message arrives. Deserializes the
    /// snapshot to initialize the local sim. Returns the tick from the snapshot.
    pub fn poll_until_snapshot_load(&mut self) -> u64 {
        let start = Instant::now();
        loop {
            assert!(
                start.elapsed() < POLL_TIMEOUT,
                "timed out waiting for SnapshotLoad"
            );
            for msg in self.client.poll() {
                if let ServerMessage::SnapshotLoad { tick, data } = msg {
                    let json = String::from_utf8(data).expect("snapshot data not UTF-8");
                    let sim = SimState::from_json(&json).expect("snapshot deserialization failed");
                    self.sim = Some(sim);
                    return tick;
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Send a raw snapshot response (for edge-case tests).
    pub fn send_snapshot_response(&mut self, data: &[u8]) {
        self.client
            .send_snapshot_response(data)
            .expect("send_snapshot_response failed");
    }

    /// Request the relay to pause turn flushing.
    pub fn send_pause(&mut self) {
        self.client.send_pause().expect("send_pause failed");
    }

    /// Request the relay to resume turn flushing.
    pub fn send_resume(&mut self) {
        self.client.send_resume().expect("send_resume failed");
    }

    /// Request the relay to change the turn cadence.
    pub fn send_set_speed(&mut self, ticks_per_turn: u32) {
        self.client
            .send_set_speed(ticks_per_turn)
            .expect("send_set_speed failed");
    }

    /// Blocking poll until a `Paused` broadcast arrives. Applies any turns
    /// encountered while waiting.
    pub fn poll_until_paused(&mut self) {
        let start = Instant::now();
        loop {
            assert!(
                start.elapsed() < POLL_TIMEOUT,
                "timed out waiting for Paused"
            );
            for msg in self.client.poll() {
                match msg {
                    ServerMessage::Paused { .. } => return,
                    ServerMessage::Turn {
                        sim_tick_target,
                        commands,
                        ..
                    } => {
                        if let Some(sim) = self.sim.as_mut() {
                            let payloads: Vec<&[u8]> =
                                commands.iter().map(|tc| tc.payload.as_slice()).collect();
                            sim.apply_turn_payloads(sim_tick_target, &payloads);
                        }
                    }
                    _ => {}
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Blocking poll until a `Resumed` broadcast arrives. Applies any turns
    /// encountered while waiting.
    pub fn poll_until_resumed(&mut self) {
        let start = Instant::now();
        loop {
            assert!(
                start.elapsed() < POLL_TIMEOUT,
                "timed out waiting for Resumed"
            );
            for msg in self.client.poll() {
                match msg {
                    ServerMessage::Resumed { .. } => return,
                    ServerMessage::Turn {
                        sim_tick_target,
                        commands,
                        ..
                    } => {
                        if let Some(sim) = self.sim.as_mut() {
                            let payloads: Vec<&[u8]> =
                                commands.iter().map(|tc| tc.payload.as_slice()).collect();
                            sim.apply_turn_payloads(sim_tick_target, &payloads);
                        }
                    }
                    _ => {}
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Send Goodbye and close the connection.
    pub fn disconnect(&mut self) {
        self.client.disconnect();
    }
}
