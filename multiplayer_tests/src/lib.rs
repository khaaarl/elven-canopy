// Test-only game client for multiplayer integration tests.
//
// Wraps the real `NetClient` (from `elven_canopy_relay::client`) and a real
// `SimState` (from `elven_canopy_sim::sim`) to provide a synchronous,
// test-friendly API for exercising the full multiplayer pipeline:
// host → relay → join → command → turn → sim.step() → verify state.
//
// The only test-specific code here is the synchronous polling wrappers
// (blocking loops around `NetClient::poll()`). All networking and sim
// logic uses the same code paths as the real game.
//
// See also: `tests/full_pipeline.rs` for the integration test scenarios.

use std::thread;
use std::time::{Duration, Instant};

use elven_canopy_protocol::message::ServerMessage;
use elven_canopy_relay::client::NetClient;
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

    /// Host only: send StartGame to begin the multiplayer session.
    pub fn send_start_game(&mut self, seed: i64, config_json: &str) {
        self.client
            .send_start_game(seed, config_json)
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
                if let ServerMessage::GameStart { seed, config_json } = msg {
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

    /// Send Goodbye and close the connection.
    pub fn disconnect(&mut self) {
        self.client.disconnect();
    }
}
