// End-to-end integration tests for the multiplayer pipeline.
//
// Each test starts a real relay server, connects real NetClient instances
// (via TestGameClient), and verifies the full path:
// host → relay → join → command → turn → sim.step() → identical state.
//
// These tests exercise the same code paths as the live game (NetClient from
// the relay crate, apply_turn_payloads from the sim crate) — the only
// test-specific code is the synchronous polling wrappers in TestGameClient.

use std::thread;
use std::time::Duration;

use elven_canopy_protocol::message::ServerMessage;
use elven_canopy_relay::server::{RelayConfig, RelayHandle, start_relay};
use elven_canopy_sim::command::SimAction;
use elven_canopy_sim::types::{Species, VoxelCoord};
use multiplayer_tests::TestGameClient;

/// Small test world size — 64^3 is ~64x fewer voxels than the default
/// 256x128x256, making SimState construction fast in debug builds.
const TEST_WORLD_SIZE: (u32, u32, u32) = (64, 64, 64);

/// Ticks per turn for tests. Short enough for fast tests, long enough for
/// the relay's turn timer to work reliably.
const TEST_TICKS_PER_TURN: u32 = 50;

/// Start a relay on a random port, connect a host and a joiner.
/// Returns the relay handle and both clients.
fn start_test_session() -> (RelayHandle, TestGameClient, TestGameClient) {
    let config = RelayConfig {
        port: 0,
        session_name: "integration-test".into(),
        password: None,
        ticks_per_turn: TEST_TICKS_PER_TURN,
        max_players: 4,
    };
    let (handle, addr) = start_relay(config).unwrap();
    thread::sleep(Duration::from_millis(50));

    let host = TestGameClient::connect(addr, "Host");
    let joiner = TestGameClient::connect(addr, "Joiner");

    // Drain host's PlayerJoined notification for the joiner.
    thread::sleep(Duration::from_millis(50));
    let _ = host.poll_raw();

    (handle, host, joiner)
}

/// Host starts the game, both clients poll until GameStart and init sims.
fn start_game(host: &mut TestGameClient, joiner: &mut TestGameClient) {
    host.send_start_game(42, "{}");
    host.poll_until_game_start(TEST_WORLD_SIZE);
    joiner.poll_until_game_start(TEST_WORLD_SIZE);
}

// ---------------------------------------------------------------------------
// Test scenarios
// ---------------------------------------------------------------------------

/// Two players connect, host starts the game, both init sims.
/// Verify identical initial state.
#[test]
fn two_player_lifecycle() {
    let (handle, mut host, mut joiner) = start_test_session();
    start_game(&mut host, &mut joiner);

    let host_sim = host.sim.as_ref().unwrap();
    let joiner_sim = joiner.sim.as_ref().unwrap();

    // Both sims should start at tick 0 with the same tree.
    assert_eq!(host_sim.tick, joiner_sim.tick);
    assert_eq!(host_sim.player_tree_id, joiner_sim.player_tree_id);
    assert_eq!(host_sim.creatures.len(), joiner_sim.creatures.len());
    assert_eq!(host_sim.trees.len(), joiner_sim.trees.len());

    // Verify full state match via JSON serialization.
    let host_json = host_sim.to_json().unwrap();
    let joiner_json = joiner_sim.to_json().unwrap();
    assert_eq!(
        host_json, joiner_json,
        "initial sim state should be identical"
    );

    host.disconnect();
    joiner.disconnect();
    handle.stop();
}

/// Host sends a SpawnCreature command. Both clients receive the turn and
/// apply it. Verify both sims have 1 elf at the same position.
#[test]
fn command_round_trip() {
    let (handle, mut host, mut joiner) = start_test_session();
    start_game(&mut host, &mut joiner);

    // Find a valid spawn position: use the home tree's position at ground level.
    let host_sim = host.sim.as_ref().unwrap();
    let tree_pos = host_sim.trees[&host_sim.player_tree_id].position;
    let spawn_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);

    // Host sends SpawnCreature.
    host.send_action(&SimAction::SpawnCreature {
        species: Species::Elf,
        position: spawn_pos,
    });

    // Both poll for the turn containing the command.
    let tick_h = host.poll_until_turn();
    let tick_j = joiner.poll_until_turn();
    assert_eq!(tick_h, tick_j, "both should apply same tick target");

    let host_sim = host.sim.as_ref().unwrap();
    let joiner_sim = joiner.sim.as_ref().unwrap();

    // Both should have exactly 1 elf.
    assert_eq!(host_sim.creature_count(Species::Elf), 1);
    assert_eq!(joiner_sim.creature_count(Species::Elf), 1);

    // Verify full state match.
    let host_json = host_sim.to_json().unwrap();
    let joiner_json = joiner_sim.to_json().unwrap();
    assert_eq!(host_json, joiner_json, "state should match after command");

    host.disconnect();
    joiner.disconnect();
    handle.stop();
}

/// Both clients send commands in the same turn window. Verify identical
/// state on both sides (canonical ordering by the relay).
#[test]
fn bidirectional_commands() {
    let (handle, mut host, mut joiner) = start_test_session();
    start_game(&mut host, &mut joiner);

    let host_sim = host.sim.as_ref().unwrap();
    let tree_pos = host_sim.trees[&host_sim.player_tree_id].position;
    let spawn_pos_1 = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    let spawn_pos_2 = VoxelCoord::new(tree_pos.x + 1, 1, tree_pos.z);

    // Both send spawn commands in the same turn window.
    host.send_action(&SimAction::SpawnCreature {
        species: Species::Elf,
        position: spawn_pos_1,
    });
    joiner.send_action(&SimAction::SpawnCreature {
        species: Species::Elf,
        position: spawn_pos_2,
    });

    // Both poll until they get a turn with commands.
    let tick_h = host.poll_until_turn();
    let tick_j = joiner.poll_until_turn();
    assert_eq!(tick_h, tick_j);

    let host_sim = host.sim.as_ref().unwrap();
    let joiner_sim = joiner.sim.as_ref().unwrap();

    // Both should have exactly 2 elves.
    assert_eq!(host_sim.creature_count(Species::Elf), 2);
    assert_eq!(joiner_sim.creature_count(Species::Elf), 2);

    // Verify full state match.
    let host_json = host_sim.to_json().unwrap();
    let joiner_json = joiner_sim.to_json().unwrap();
    assert_eq!(
        host_json, joiner_json,
        "state should match after bidirectional commands"
    );

    host.disconnect();
    joiner.disconnect();
    handle.stop();
}

/// Over multiple turns: spawn creatures, issue GoTo tasks. After each
/// turn, compare sim state JSON on both sides.
#[test]
fn multi_turn_determinism() {
    let (handle, mut host, mut joiner) = start_test_session();
    start_game(&mut host, &mut joiner);

    let host_sim = host.sim.as_ref().unwrap();
    let tree_pos = host_sim.trees[&host_sim.player_tree_id].position;
    let spawn_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);

    // Turn 1: spawn an elf.
    host.send_action(&SimAction::SpawnCreature {
        species: Species::Elf,
        position: spawn_pos,
    });
    host.poll_until_turn();
    joiner.poll_until_turn();

    let host_json_1 = host.sim.as_ref().unwrap().to_json().unwrap();
    let joiner_json_1 = joiner.sim.as_ref().unwrap().to_json().unwrap();
    assert_eq!(host_json_1, joiner_json_1, "mismatch after turn 1");

    // Turn 2: issue a GoTo task.
    let goto_pos = VoxelCoord::new(tree_pos.x + 2, 1, tree_pos.z);
    host.send_action(&SimAction::CreateTask {
        kind: elven_canopy_sim::task::TaskKind::GoTo,
        position: goto_pos,
        required_species: Some(Species::Elf),
    });
    host.poll_until_turn();
    joiner.poll_until_turn();

    let host_json_2 = host.sim.as_ref().unwrap().to_json().unwrap();
    let joiner_json_2 = joiner.sim.as_ref().unwrap().to_json().unwrap();
    assert_eq!(host_json_2, joiner_json_2, "mismatch after turn 2");

    // Turn 3: spawn another elf.
    host.send_action(&SimAction::SpawnCreature {
        species: Species::Elf,
        position: spawn_pos,
    });
    host.poll_until_turn();
    joiner.poll_until_turn();

    let host_json_3 = host.sim.as_ref().unwrap().to_json().unwrap();
    let joiner_json_3 = joiner.sim.as_ref().unwrap().to_json().unwrap();
    assert_eq!(host_json_3, joiner_json_3, "mismatch after turn 3");

    host.disconnect();
    joiner.disconnect();
    handle.stop();
}

/// No commands sent — wait for empty turns to arrive. Verify both sims'
/// ticks advanced identically.
#[test]
fn empty_turns_advance_tick() {
    let (handle, mut host, mut joiner) = start_test_session();
    start_game(&mut host, &mut joiner);

    let initial_tick = host.sim.as_ref().unwrap().tick;

    // Wait for several turn cadences to pass, then drain all turns.
    // The relay flushes empty turns every ticks_per_turn ms (50ms).
    // Drain repeatedly to handle timing differences between clients.
    thread::sleep(Duration::from_millis(250));
    let _ = host.drain_turns();
    let _ = joiner.drain_turns();
    // Short extra sleep + drain to catch any stragglers.
    thread::sleep(Duration::from_millis(100));
    let _ = host.drain_turns();
    let _ = joiner.drain_turns();

    let host_sim = host.sim.as_ref().unwrap();
    let joiner_sim = joiner.sim.as_ref().unwrap();

    // Both sims should have advanced past the initial tick.
    assert!(
        host_sim.tick > initial_tick,
        "host tick should have advanced from {initial_tick}, is {}",
        host_sim.tick
    );

    // Both sims should be at the same tick.
    assert_eq!(
        host_sim.tick, joiner_sim.tick,
        "both sims should be at the same tick"
    );

    // Tick should be a multiple of ticks_per_turn.
    assert_eq!(
        host_sim.tick % (TEST_TICKS_PER_TURN as u64),
        0,
        "tick should be a multiple of ticks_per_turn"
    );

    // State should still match (only heartbeats/scheduled events ran).
    let host_json = host_sim.to_json().unwrap();
    let joiner_json = joiner_sim.to_json().unwrap();
    assert_eq!(
        host_json, joiner_json,
        "state should match after empty turns"
    );

    host.disconnect();
    joiner.disconnect();
    handle.stop();
}

/// After some commands, the joiner disconnects. Host should receive
/// PlayerLeft and continue receiving turns.
#[test]
fn disconnect_mid_game() {
    let (handle, mut host, mut joiner) = start_test_session();
    start_game(&mut host, &mut joiner);

    // Spawn an elf so there's some state.
    let host_sim = host.sim.as_ref().unwrap();
    let tree_pos = host_sim.trees[&host_sim.player_tree_id].position;
    let spawn_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);

    host.send_action(&SimAction::SpawnCreature {
        species: Species::Elf,
        position: spawn_pos,
    });
    host.poll_until_turn();
    joiner.poll_until_turn();

    // Joiner disconnects.
    joiner.disconnect();
    // Give relay time to process the disconnect.
    thread::sleep(Duration::from_millis(150));

    // Drain turns (keeping sim in sync) and collect non-Turn messages.
    let (_, other_messages) = host.drain_turns();
    let has_player_left = other_messages
        .iter()
        .any(|m| matches!(m, ServerMessage::PlayerLeft { .. }));
    assert!(
        has_player_left,
        "host should receive PlayerLeft, got: {other_messages:?}"
    );

    // Host should still be able to send commands and receive turns.
    host.send_action(&SimAction::SpawnCreature {
        species: Species::Elf,
        position: spawn_pos,
    });
    let tick = host.poll_until_turn();
    assert!(
        tick > 0,
        "host should still receive turns after joiner disconnect"
    );

    assert_eq!(host.sim.as_ref().unwrap().creature_count(Species::Elf), 2);

    host.disconnect();
    handle.stop();
}
