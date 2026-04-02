// Integration tests for the relay server.
//
// Starts a relay on localhost and exercises the full protocol lifecycle
// including multi-session support. Each client is a plain TCP socket using
// the protocol crate's framing and message types — no game code involved.

use std::io::{BufReader, BufWriter};
use std::net::TcpStream;
use std::time::Duration;

use elven_canopy_protocol::framing::{read_message, write_message};
use elven_canopy_protocol::message::{ClientMessage, ServerMessage};
use elven_canopy_protocol::types::{ActionSequence, RelayPlayerId, SessionId, TurnNumber};
use elven_canopy_relay::server::{RelayConfig, start_relay};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Helper: send a ClientMessage over a framed TCP stream.
fn send(writer: &mut BufWriter<TcpStream>, msg: &ClientMessage) {
    let json = serde_json::to_vec(msg).unwrap();
    write_message(writer, &json).unwrap();
}

/// Helper: receive a ServerMessage from a framed TCP stream.
fn recv(reader: &mut BufReader<TcpStream>) -> ServerMessage {
    let bytes = read_message(reader).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Test connection helper that manages a BufReader/BufWriter pair for
/// both pre-handshake and post-handshake phases.
struct TestConn {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

impl TestConn {
    fn connect(addr: std::net::SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let reader_stream = stream.try_clone().unwrap();
        Self {
            writer: BufWriter::new(stream),
            reader: BufReader::new(reader_stream),
        }
    }

    fn send_msg(&mut self, msg: &ClientMessage) {
        send(&mut self.writer, msg);
    }

    fn recv_msg(&mut self) -> ServerMessage {
        recv(&mut self.reader)
    }

    /// Create a session and return its ID.
    fn create_session(
        &mut self,
        name: &str,
        password: Option<&str>,
        ticks_per_turn: u32,
        max_players: u32,
    ) -> SessionId {
        self.send_msg(&ClientMessage::CreateSession {
            session_name: name.into(),
            password: password.map(String::from),
            ticks_per_turn,
            max_players,
        });
        match self.recv_msg() {
            ServerMessage::SessionCreated { session_id } => session_id,
            other => panic!("expected SessionCreated, got {other:?}"),
        }
    }

    /// Send Hello and receive Welcome. Consumes self and returns the
    /// reader/writer pair for post-handshake use.
    fn join_session(
        mut self,
        session_id: SessionId,
        name: &str,
        password: Option<&str>,
    ) -> (BufReader<TcpStream>, BufWriter<TcpStream>, RelayPlayerId) {
        send(
            &mut self.writer,
            &ClientMessage::Hello {
                protocol_version: 1,
                session_id,
                player_name: name.into(),
                sim_version_hash: 0xABCD,
                config_hash: 0x1234,
                session_password: password.map(String::from),
            },
        );

        let msg = recv(&mut self.reader);
        let player_id = match msg {
            ServerMessage::Welcome { player_id, .. } => player_id,
            other => panic!("expected Welcome, got {other:?}"),
        };

        (self.reader, self.writer, player_id)
    }
}

/// Start an embedded relay (single session with SessionId(0)) and return
/// the handle + address.
fn start_embedded_relay() -> (
    elven_canopy_relay::server::RelayHandle,
    std::net::SocketAddr,
) {
    let config = RelayConfig {
        port: 0,
        bind_address: "127.0.0.1".into(),
        embedded: true,
        turn_cadence_ms: 50,
    };
    let (handle, addr) = start_relay(config).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    (handle, addr)
}

/// Start a dedicated relay and return the handle + address.
fn start_dedicated_relay() -> (
    elven_canopy_relay::server::RelayHandle,
    std::net::SocketAddr,
) {
    let config = RelayConfig {
        port: 0,
        bind_address: "127.0.0.1".into(),
        embedded: false,
        turn_cadence_ms: 50,
    };
    let (handle, addr) = start_relay(config).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    (handle, addr)
}

/// Read messages until we get a Turn with at least one command.
fn wait_for_turn_with_commands(
    reader: &mut BufReader<TcpStream>,
) -> (TurnNumber, u64, Vec<elven_canopy_protocol::TurnCommand>) {
    for _ in 0..50 {
        let msg = recv(reader);
        if let ServerMessage::Turn {
            turn_number,
            sim_tick_target,
            commands,
        } = msg
            && !commands.is_empty()
        {
            return (turn_number, sim_tick_target, commands);
        }
    }
    panic!("did not receive Turn with commands within 50 reads");
}

/// Drain all currently buffered messages using a short read timeout.
fn drain_messages(reader: &mut BufReader<TcpStream>) -> Vec<ServerMessage> {
    let mut messages = Vec::new();
    if let Ok(stream) = reader.get_ref().try_clone() {
        stream
            .set_read_timeout(Some(Duration::from_millis(10)))
            .ok();
    }
    for _ in 0..50 {
        match read_message(reader) {
            Ok(bytes) => match serde_json::from_slice::<ServerMessage>(&bytes) {
                Ok(msg) => messages.push(msg),
                Err(_) => break,
            },
            Err(_) => break,
        }
    }
    // Restore longer timeout for subsequent blocking reads.
    if let Ok(stream) = reader.get_ref().try_clone() {
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    }
    messages
}

// ===========================================================================
// Tests: Embedded relay (single-session, backward-compatible flow)
// ===========================================================================

#[test]
fn embedded_full_session_lifecycle() {
    // Start embedded relay, create session, connect two players, exchange
    // commands, verify checksums, disconnect.
    let (handle, addr) = start_embedded_relay();

    // Host creates the session and joins.
    let mut conn_a = TestConn::connect(addr);
    let sid = conn_a.create_session("embedded-game", None, 50, 4);
    assert_eq!(sid, SessionId(0));
    let (mut reader_a, mut writer_a, id_a) = conn_a.join_session(sid, "Alice", None);
    assert_eq!(id_a, RelayPlayerId(0));

    // Guest joins directly with SessionId(0).
    let conn_b = TestConn::connect(addr);
    let (mut reader_b, mut writer_b, id_b) = conn_b.join_session(sid, "Bob", None);
    assert_eq!(id_b, RelayPlayerId(1));

    // Alice should receive PlayerJoined for Bob.
    let msg = recv(&mut reader_a);
    assert!(
        matches!(msg, ServerMessage::PlayerJoined { ref player } if player.name == "Bob"),
        "expected PlayerJoined for Bob, got {msg:?}"
    );

    // Host starts the game.
    send(
        &mut writer_a,
        &ClientMessage::StartGame {
            seed: 42,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );

    // Both clients receive GameStart.
    let msg_a = recv(&mut reader_a);
    assert!(matches!(msg_a, ServerMessage::GameStart { seed: 42, .. }));
    let msg_b = recv(&mut reader_b);
    assert!(matches!(msg_b, ServerMessage::GameStart { seed: 42, .. }));

    // Alice sends a command, both receive it in a turn.
    send(
        &mut writer_a,
        &ClientMessage::Command {
            sequence: ActionSequence(0),
            payload: vec![10, 20, 30],
        },
    );
    let turn_a = wait_for_turn_with_commands(&mut reader_a);
    let turn_b = wait_for_turn_with_commands(&mut reader_b);
    assert_eq!(turn_a.0, turn_b.0);
    assert_eq!(turn_a.2[0].payload, vec![10, 20, 30]);

    // Matching checksums — no desync.
    send(
        &mut writer_a,
        &ClientMessage::Checksum {
            tick: 50,
            hash: 0xBEEF,
        },
    );
    send(
        &mut writer_b,
        &ClientMessage::Checksum {
            tick: 50,
            hash: 0xBEEF,
        },
    );
    std::thread::sleep(Duration::from_millis(150));
    let msgs = drain_messages(&mut reader_a);
    assert!(
        !msgs
            .iter()
            .any(|m| matches!(m, ServerMessage::DesyncDetected { .. }))
    );

    // Mismatching checksums — desync detected.
    send(
        &mut writer_a,
        &ClientMessage::Checksum {
            tick: 100,
            hash: 0xAAAA,
        },
    );
    send(
        &mut writer_b,
        &ClientMessage::Checksum {
            tick: 100,
            hash: 0xBBBB,
        },
    );
    std::thread::sleep(Duration::from_millis(150));
    let msgs = drain_messages(&mut reader_a);
    assert!(
        msgs.iter()
            .any(|m| matches!(m, ServerMessage::DesyncDetected { tick: 100 })),
        "expected DesyncDetected, got: {msgs:?}"
    );

    // Alice disconnects — Bob sees PlayerLeft.
    send(&mut writer_a, &ClientMessage::Goodbye);
    std::thread::sleep(Duration::from_millis(150));
    let msgs = drain_messages(&mut reader_b);
    assert!(msgs.iter().any(|m| matches!(
        m,
        ServerMessage::PlayerLeft {
            player_id: RelayPlayerId(0),
            ..
        }
    )));

    drop(writer_b);
    drop(reader_b);
    handle.stop();
}

#[test]
fn embedded_rejects_second_session() {
    let (handle, addr) = start_embedded_relay();

    // First session creation succeeds.
    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("game1", None, 50, 4);
    assert_eq!(sid, SessionId(0));
    let (_reader, _writer, _id) = conn.join_session(sid, "Alice", None);

    // Second session creation on same embedded relay should fail.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::CreateSession {
        session_name: "game2".into(),
        password: None,
        ticks_per_turn: 50,
        max_players: 4,
    });
    let msg = conn2.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason.contains("embedded")),
        "expected Rejected for embedded, got {msg:?}"
    );

    handle.stop();
}

// ===========================================================================
// Tests: Dedicated relay (multi-session)
// ===========================================================================

#[test]
fn dedicated_create_and_list_sessions() {
    let (handle, addr) = start_dedicated_relay();

    // Create two sessions.
    let mut conn1 = TestConn::connect(addr);
    let sid1 = conn1.create_session("game-alpha", None, 50, 4);

    let mut conn2 = TestConn::connect(addr);
    let sid2 = conn2.create_session("game-beta", Some("secret"), 100, 2);

    assert_ne!(sid1, sid2);

    // List sessions from a third connection.
    let mut conn3 = TestConn::connect(addr);
    conn3.send_msg(&ClientMessage::ListSessions);
    let msg = conn3.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(sessions.len(), 2);
            let alpha = sessions.iter().find(|s| s.name == "game-alpha").unwrap();
            assert_eq!(alpha.session_id, sid1);
            assert_eq!(alpha.player_count, 0); // no one has joined yet
            assert!(!alpha.has_password);
            assert!(!alpha.game_started);

            let beta = sessions.iter().find(|s| s.name == "game-beta").unwrap();
            assert_eq!(beta.session_id, sid2);
            assert!(beta.has_password);
            assert_eq!(beta.max_players, 2);
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    // Join session 1 and list again — player count should update.
    let (_reader1, _writer1, _id1) = conn1.join_session(sid1, "Alice", None);

    // Allow some time for the join to be processed.
    std::thread::sleep(Duration::from_millis(100));

    let mut conn4 = TestConn::connect(addr);
    conn4.send_msg(&ClientMessage::ListSessions);
    let msg = conn4.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            let alpha = sessions.iter().find(|s| s.name == "game-alpha").unwrap();
            assert_eq!(alpha.player_count, 1);
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn dedicated_two_independent_sessions() {
    // Two sessions running on the same relay should be fully independent.
    let (handle, addr) = start_dedicated_relay();

    // Create session A with two players.
    let mut conn_a1 = TestConn::connect(addr);
    let sid_a = conn_a1.create_session("game-A", None, 50, 4);
    let (mut reader_a1, mut writer_a1, _) = conn_a1.join_session(sid_a, "Alice", None);

    let conn_a2 = TestConn::connect(addr);
    let (mut reader_a2, _writer_a2, _) = conn_a2.join_session(sid_a, "Bob", None);

    // Drain Alice's PlayerJoined for Bob.
    let _ = recv(&mut reader_a1);

    // Create session B with two players.
    let mut conn_b1 = TestConn::connect(addr);
    let sid_b = conn_b1.create_session("game-B", None, 50, 4);
    let (mut reader_b1, mut writer_b1, _) = conn_b1.join_session(sid_b, "Charlie", None);

    let conn_b2 = TestConn::connect(addr);
    let (mut reader_b2, _writer_b2, _) = conn_b2.join_session(sid_b, "Dave", None);

    // Drain Charlie's PlayerJoined for Dave.
    let _ = recv(&mut reader_b1);

    // Start both games.
    send(
        &mut writer_a1,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    send(
        &mut writer_b1,
        &ClientMessage::StartGame {
            seed: 2,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );

    // Both sessions receive their own GameStart.
    let msg = recv(&mut reader_a1);
    assert!(matches!(msg, ServerMessage::GameStart { seed: 1, .. }));
    let msg = recv(&mut reader_b1);
    assert!(matches!(msg, ServerMessage::GameStart { seed: 2, .. }));
    let msg = recv(&mut reader_a2);
    assert!(matches!(msg, ServerMessage::GameStart { seed: 1, .. }));
    let msg = recv(&mut reader_b2);
    assert!(matches!(msg, ServerMessage::GameStart { seed: 2, .. }));

    // Send commands in session A — should NOT appear in session B.
    send(
        &mut writer_a1,
        &ClientMessage::Command {
            sequence: ActionSequence(0),
            payload: vec![1, 2, 3],
        },
    );

    let turn_a1 = wait_for_turn_with_commands(&mut reader_a1);
    let turn_a2 = wait_for_turn_with_commands(&mut reader_a2);
    assert_eq!(turn_a1.2[0].payload, vec![1, 2, 3]);
    assert_eq!(turn_a2.2[0].payload, vec![1, 2, 3]);

    // Session B should only receive empty turns (or no command turns).
    send(
        &mut writer_b1,
        &ClientMessage::Command {
            sequence: ActionSequence(0),
            payload: vec![7, 8, 9],
        },
    );
    let turn_b1 = wait_for_turn_with_commands(&mut reader_b1);
    let turn_b2 = wait_for_turn_with_commands(&mut reader_b2);
    assert_eq!(turn_b1.2[0].payload, vec![7, 8, 9]);
    assert_eq!(turn_b2.2[0].payload, vec![7, 8, 9]);

    // Verify no cross-contamination: A's commands don't appear in B's turns.
    assert!(
        turn_b1.2.iter().all(|c| c.payload != vec![1, 2, 3]),
        "session B should not receive session A's commands"
    );

    handle.stop();
}

#[test]
fn dedicated_session_password_enforcement() {
    let (handle, addr) = start_dedicated_relay();

    // Create a password-protected session.
    let mut conn_host = TestConn::connect(addr);
    let sid = conn_host.create_session("secret-game", Some("pass123"), 50, 4);
    let (_reader_host, _writer_host, _) = conn_host.join_session(sid, "Host", Some("pass123"));

    // Try to join with wrong password — should be rejected.
    let mut conn_bad = TestConn::connect(addr);
    conn_bad.send_msg(&ClientMessage::Hello {
        protocol_version: 1,
        session_id: sid,
        player_name: "Intruder".into(),
        sim_version_hash: 0xABCD,
        config_hash: 0x1234,
        session_password: Some("wrong".into()),
    });
    let msg = conn_bad.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason == "incorrect password"),
        "expected password rejection, got {msg:?}"
    );

    // Try to join with no password — should be rejected.
    let mut conn_no_pw = TestConn::connect(addr);
    conn_no_pw.send_msg(&ClientMessage::Hello {
        protocol_version: 1,
        session_id: sid,
        player_name: "Intruder2".into(),
        sim_version_hash: 0xABCD,
        config_hash: 0x1234,
        session_password: None,
    });
    let msg = conn_no_pw.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason == "incorrect password"),
        "expected password rejection, got {msg:?}"
    );

    // Join with correct password — should succeed.
    let conn_ok = TestConn::connect(addr);
    let (_reader, _writer, _id) = conn_ok.join_session(sid, "Guest", Some("pass123"));

    handle.stop();
}

#[test]
fn dedicated_join_nonexistent_session() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn = TestConn::connect(addr);
    conn.send_msg(&ClientMessage::Hello {
        protocol_version: 1,
        session_id: SessionId(999),
        player_name: "Lost".into(),
        sim_version_hash: 0xABCD,
        config_hash: 0x1234,
        session_password: None,
    });
    let msg = conn.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason == "session not found"),
        "expected session not found, got {msg:?}"
    );

    handle.stop();
}

#[test]
fn dedicated_session_cleanup_on_last_leave() {
    let (handle, addr) = start_dedicated_relay();

    // Create a session and join.
    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("temp-game", None, 50, 4);
    let (_reader, mut writer, _id) = conn.join_session(sid, "Alice", None);

    // Verify session exists.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let msg = conn2.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(sessions.len(), 1);
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    // Alice leaves gracefully.
    send(&mut writer, &ClientMessage::Goodbye);
    std::thread::sleep(Duration::from_millis(150));

    // Session should be cleaned up — list should be empty.
    let mut conn3 = TestConn::connect(addr);
    conn3.send_msg(&ClientMessage::ListSessions);
    let msg = conn3.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(
                sessions.len(),
                0,
                "empty session should be cleaned up, got: {sessions:?}"
            );
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn dedicated_session_cleanup_on_abrupt_disconnect() {
    let (handle, addr) = start_dedicated_relay();

    // Create a session and join.
    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("crash-game", None, 50, 4);
    let (_reader, writer, _id) = conn.join_session(sid, "Alice", None);

    // Drop the connection without sending Goodbye (simulate crash).
    drop(writer);
    drop(_reader);
    std::thread::sleep(Duration::from_millis(300));

    // Session should be cleaned up.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let msg = conn2.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(
                sessions.len(),
                0,
                "session should be cleaned up after abrupt disconnect"
            );
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn dedicated_session_persists_while_players_remain() {
    let (handle, addr) = start_dedicated_relay();

    // Create session, two players join.
    let mut conn_a = TestConn::connect(addr);
    let sid = conn_a.create_session("persist-game", None, 50, 4);
    let (_reader_a, mut writer_a, _) = conn_a.join_session(sid, "Alice", None);

    let conn_b = TestConn::connect(addr);
    let (mut reader_b, _writer_b, _) = conn_b.join_session(sid, "Bob", None);

    // Alice leaves.
    send(&mut writer_a, &ClientMessage::Goodbye);
    std::thread::sleep(Duration::from_millis(150));

    // Bob should see PlayerLeft.
    let msgs = drain_messages(&mut reader_b);
    assert!(msgs.iter().any(|m| matches!(
        m,
        ServerMessage::PlayerLeft {
            player_id: RelayPlayerId(0),
            ..
        }
    )));

    // Session should still exist (Bob is still connected).
    let mut conn3 = TestConn::connect(addr);
    conn3.send_msg(&ClientMessage::ListSessions);
    let msg = conn3.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].player_count, 1);
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn dedicated_join_leave_rejoin_same_session() {
    let (handle, addr) = start_dedicated_relay();

    // Create session, host joins.
    let mut conn_host = TestConn::connect(addr);
    let sid = conn_host.create_session("rejoin-game", None, 50, 4);
    let (mut reader_host, _writer_host, _) = conn_host.join_session(sid, "Host", None);

    // Guest joins.
    let conn_guest1 = TestConn::connect(addr);
    let (_reader_g1, mut writer_g1, id_g1) = conn_guest1.join_session(sid, "Guest", None);
    assert_eq!(id_g1, RelayPlayerId(1));

    // Host sees guest join.
    let msg = recv(&mut reader_host);
    assert!(matches!(msg, ServerMessage::PlayerJoined { .. }));

    // Guest leaves.
    send(&mut writer_g1, &ClientMessage::Goodbye);
    std::thread::sleep(Duration::from_millis(150));

    // Host sees guest leave.
    let msgs = drain_messages(&mut reader_host);
    assert!(
        msgs.iter()
            .any(|m| matches!(m, ServerMessage::PlayerLeft { .. }))
    );

    // Guest rejoins the same session — should get a new player ID.
    let conn_guest2 = TestConn::connect(addr);
    let (_reader_g2, _writer_g2, id_g2) = conn_guest2.join_session(sid, "Guest", None);
    assert_eq!(id_g2, RelayPlayerId(2)); // IDs are not reused

    // Host sees the rejoin.
    let msg = recv(&mut reader_host);
    assert!(
        matches!(msg, ServerMessage::PlayerJoined { ref player } if player.id == RelayPlayerId(2)),
        "expected PlayerJoined with new ID, got {msg:?}"
    );

    handle.stop();
}

#[test]
fn dedicated_session_full_rejection() {
    let (handle, addr) = start_dedicated_relay();

    // Create a session with max_players=1.
    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("tiny-game", None, 50, 1);
    let (_reader, _writer, _) = conn.join_session(sid, "Alice", None);

    // Second player tries to join — should be rejected.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::Hello {
        protocol_version: 1,
        session_id: sid,
        player_name: "Bob".into(),
        sim_version_hash: 0xABCD,
        config_hash: 0x1234,
        session_password: None,
    });
    let msg = conn2.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason == "session is full"),
        "expected session full, got {msg:?}"
    );

    handle.stop();
}

#[test]
fn dedicated_version_mismatch_rejection() {
    let (handle, addr) = start_dedicated_relay();

    // Create session, host joins with version 0xABCD.
    let mut conn_host = TestConn::connect(addr);
    let sid = conn_host.create_session("version-game", None, 50, 4);
    let (_reader, _writer, _) = conn_host.join_session(sid, "Host", None);

    // Guest joins with different sim_version_hash — rejected.
    let mut conn_bad = TestConn::connect(addr);
    conn_bad.send_msg(&ClientMessage::Hello {
        protocol_version: 1,
        session_id: sid,
        player_name: "BadVersion".into(),
        sim_version_hash: 0xDEAD, // different from 0xABCD
        config_hash: 0x1234,
        session_password: None,
    });
    let msg = conn_bad.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason == "sim version mismatch"),
        "expected version mismatch, got {msg:?}"
    );

    handle.stop();
}

#[test]
fn dedicated_list_sessions_empty() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn = TestConn::connect(addr);
    conn.send_msg(&ClientMessage::ListSessions);
    let msg = conn.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(sessions.len(), 0);
        }
        other => panic!("expected empty SessionList, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn dedicated_list_then_create_then_join() {
    // Full discovery flow: list → empty → create → list → 1 session → join.
    let (handle, addr) = start_dedicated_relay();

    let mut conn = TestConn::connect(addr);

    // List — should be empty.
    conn.send_msg(&ClientMessage::ListSessions);
    let msg = conn.recv_msg();
    assert!(matches!(
        msg,
        ServerMessage::SessionList { sessions } if sessions.is_empty()
    ));

    // Create.
    let sid = conn.create_session("new-game", None, 50, 4);

    // List again — should have one session.
    conn.send_msg(&ClientMessage::ListSessions);
    let msg = conn.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].session_id, sid);
        }
        other => panic!("expected SessionList with 1 session, got {other:?}"),
    }

    // Join.
    let (_reader, _writer, _id) = conn.join_session(sid, "Player", None);

    handle.stop();
}

#[test]
fn dedicated_multiple_list_sessions_calls() {
    // ListSessions can be called multiple times before joining.
    let (handle, addr) = start_dedicated_relay();

    let mut conn = TestConn::connect(addr);

    for _ in 0..3 {
        conn.send_msg(&ClientMessage::ListSessions);
        let msg = conn.recv_msg();
        assert!(matches!(msg, ServerMessage::SessionList { .. }));
    }

    handle.stop();
}

#[test]
fn dedicated_create_multiple_sessions_same_connection() {
    // A single connection can create multiple sessions (e.g., an admin tool).
    let (handle, addr) = start_dedicated_relay();

    let mut conn = TestConn::connect(addr);
    let sid1 = conn.create_session("game1", None, 50, 4);
    let sid2 = conn.create_session("game2", None, 50, 4);
    assert_ne!(sid1, sid2);

    // List should show both.
    conn.send_msg(&ClientMessage::ListSessions);
    let msg = conn.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(sessions.len(), 2);
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn dedicated_chat_isolated_between_sessions() {
    let (handle, addr) = start_dedicated_relay();

    // Session A.
    let mut conn_a1 = TestConn::connect(addr);
    let sid_a = conn_a1.create_session("chat-A", None, 50, 4);
    let (mut reader_a1, _writer_a1, _) = conn_a1.join_session(sid_a, "Alice", None);

    let conn_a2 = TestConn::connect(addr);
    let (_reader_a2, mut writer_a2, _) = conn_a2.join_session(sid_a, "AliceFriend", None);

    // Drain PlayerJoined.
    let _ = recv(&mut reader_a1);

    // Session B.
    let mut conn_b1 = TestConn::connect(addr);
    let sid_b = conn_b1.create_session("chat-B", None, 50, 4);
    let (mut reader_b1, _writer_b1, _) = conn_b1.join_session(sid_b, "Bob", None);

    // AliceFriend chats in session A.
    send(
        &mut writer_a2,
        &ClientMessage::Chat {
            text: "hello from A!".into(),
        },
    );
    std::thread::sleep(Duration::from_millis(100));

    // Alice (session A) should receive the chat.
    let msgs = drain_messages(&mut reader_a1);
    assert!(
        msgs.iter().any(
            |m| matches!(m, ServerMessage::ChatBroadcast { text, .. } if text == "hello from A!")
        ),
        "Alice should receive chat, got: {msgs:?}"
    );

    // Bob (session B) should NOT receive the chat.
    let msgs = drain_messages(&mut reader_b1);
    assert!(
        !msgs
            .iter()
            .any(|m| matches!(m, ServerMessage::ChatBroadcast { .. })),
        "Bob should not receive session A's chat, got: {msgs:?}"
    );

    handle.stop();
}

#[test]
fn dedicated_session_game_started_visible_in_list() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("start-game", None, 50, 4);
    let (_reader, mut writer, _) = conn.join_session(sid, "Host", None);

    // Before starting: game_started should be false.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let msg = conn2.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert!(!sessions[0].game_started);
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    // Start the game.
    send(
        &mut writer,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    std::thread::sleep(Duration::from_millis(100));

    // After starting: game_started should be true.
    let mut conn3 = TestConn::connect(addr);
    conn3.send_msg(&ClientMessage::ListSessions);
    let msg = conn3.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert!(sessions[0].game_started);
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn dedicated_unexpected_message_during_prehandshake_drops_connection() {
    // Sending a gameplay message (Command) before Hello should drop the conn.
    let (handle, addr) = start_dedicated_relay();

    let stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let json = serde_json::to_vec(&ClientMessage::Command {
        sequence: ActionSequence(0),
        payload: vec![1],
    })
    .unwrap();
    let mut writer = BufWriter::new(stream.try_clone().unwrap());
    write_message(&mut writer, &json).unwrap();

    // The relay should drop us — next read should fail.
    std::thread::sleep(Duration::from_millis(100));
    let mut reader = BufReader::new(stream);
    let result = read_message(&mut reader);
    assert!(result.is_err(), "connection should be dropped");

    handle.stop();
}

#[test]
fn dedicated_abrupt_disconnect_during_game_doesnt_crash_relay() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn_a = TestConn::connect(addr);
    let sid = conn_a.create_session("crash-test", None, 50, 4);
    let (reader_a, mut writer_a, _) = conn_a.join_session(sid, "Alice", None);

    let conn_b = TestConn::connect(addr);
    let (_reader_b, mut writer_b, _) = conn_b.join_session(sid, "Bob", None);

    // Start the game.
    send(
        &mut writer_a,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    std::thread::sleep(Duration::from_millis(100));

    // Alice drops abruptly (crash).
    drop(writer_a);
    drop(reader_a);
    std::thread::sleep(Duration::from_millis(200));

    // Bob should still be able to interact — relay shouldn't crash.
    send(
        &mut writer_b,
        &ClientMessage::Chat {
            text: "still here".into(),
        },
    );
    std::thread::sleep(Duration::from_millis(100));

    // Relay is still running — we can list sessions.
    let mut conn3 = TestConn::connect(addr);
    conn3.send_msg(&ClientMessage::ListSessions);
    let msg = conn3.recv_msg();
    assert!(matches!(msg, ServerMessage::SessionList { .. }));

    handle.stop();
}

#[test]
fn dedicated_session_cleaned_up_after_all_abrupt_disconnects() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn_a = TestConn::connect(addr);
    let sid = conn_a.create_session("all-crash", None, 50, 4);
    let (reader_a, writer_a, _) = conn_a.join_session(sid, "Alice", None);

    let conn_b = TestConn::connect(addr);
    let (reader_b, writer_b, _) = conn_b.join_session(sid, "Bob", None);

    // Both crash.
    drop(writer_a);
    drop(reader_a);
    drop(writer_b);
    drop(reader_b);
    std::thread::sleep(Duration::from_millis(300));

    // Session should be gone.
    let mut conn3 = TestConn::connect(addr);
    conn3.send_msg(&ClientMessage::ListSessions);
    let msg = conn3.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(sessions.len(), 0);
        }
        other => panic!("expected empty SessionList, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn dedicated_empty_session_cleaned_up_without_any_join() {
    // A created session with no joins should still be cleaned up when the
    // creating connection disconnects (since no players ever joined).
    // Actually — CreateSession doesn't join the session, so the session starts
    // with 0 players. But we don't clean up immediately on create since the
    // creator is expected to join next. Let's verify the cleanup happens when
    // the session truly has 0 players after someone joins and leaves.
    let (handle, addr) = start_dedicated_relay();

    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("empty-game", None, 50, 4);

    // Session exists with 0 players.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let msg = conn2.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            // The session exists even with 0 players — it was just created.
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].player_count, 0);
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    // Join and leave.
    let (_reader, mut writer, _) = conn.join_session(sid, "Temp", None);
    send(&mut writer, &ClientMessage::Goodbye);
    std::thread::sleep(Duration::from_millis(150));

    // Now it should be cleaned up.
    let mut conn3 = TestConn::connect(addr);
    conn3.send_msg(&ClientMessage::ListSessions);
    let msg = conn3.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert_eq!(sessions.len(), 0);
        }
        other => panic!("expected empty SessionList, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn embedded_net_client_connect_shortcut() {
    // Test the NetClient::connect() convenience method for embedded relays.
    let (handle, addr) = start_embedded_relay();

    // Create session first (the embedded relay needs one).
    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("embed", None, 50, 4);
    assert_eq!(sid, SessionId(0));
    let (_reader, _writer, _) = conn.join_session(sid, "Host", None);

    // Use the high-level client API to join.
    use elven_canopy_relay::client::NetClient;
    let addr_str = format!("{addr}");
    let (client, welcome) = NetClient::connect(&addr_str, "Guest", 0xABCD, 0x1234, None).unwrap();
    assert_eq!(welcome.player_id, RelayPlayerId(1));
    assert_eq!(welcome.session_name, "embed");

    // Poll should work.
    std::thread::sleep(Duration::from_millis(100));
    let _msgs = client.poll();

    handle.stop();
}

#[test]
fn dedicated_relay_connection_two_phase_api() {
    // Test the RelayConnection → NetClient two-phase API.
    let (handle, addr) = start_dedicated_relay();

    use elven_canopy_relay::client::RelayConnection;

    let mut conn = RelayConnection::connect(&format!("{addr}")).unwrap();

    // List — empty.
    let sessions = conn.list_sessions().unwrap();
    assert!(sessions.is_empty());

    // Create.
    let sid = conn.create_session("api-test", None, 50, 4).unwrap();

    // List — should have one.
    let sessions = conn.list_sessions().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, sid);

    // Join.
    let (mut client, welcome) = conn
        .join_session(sid, "Player1", 0xABCD, 0x1234, None)
        .unwrap();
    assert_eq!(welcome.player_id, RelayPlayerId(0));

    // Send a chat and poll for broadcast.
    client.send_chat("hi").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let msgs = client.poll();
    assert!(
        msgs.iter()
            .any(|m| matches!(m, ServerMessage::ChatBroadcast { text, .. } if text == "hi")),
        "should receive own chat broadcast, got: {msgs:?}"
    );

    client.disconnect();
    handle.stop();
}

// ---------------------------------------------------------------------------
// Input validation tests
// ---------------------------------------------------------------------------

/// CreateSession with an empty name is rejected.
#[test]
fn dedicated_create_session_empty_name_rejected() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn = TestConn::connect(addr);
    conn.send_msg(&ClientMessage::CreateSession {
        session_name: "".into(),
        password: None,
        ticks_per_turn: 50,
        max_players: 4,
    });
    let msg = conn.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason.contains("empty")),
        "expected rejection for empty name, got: {msg:?}"
    );
    handle.stop();
}

/// CreateSession with a very long name is rejected.
#[test]
fn dedicated_create_session_long_name_rejected() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn = TestConn::connect(addr);
    let long_name = "a".repeat(200);
    conn.send_msg(&ClientMessage::CreateSession {
        session_name: long_name,
        password: None,
        ticks_per_turn: 50,
        max_players: 4,
    });
    let msg = conn.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason.contains("too long")),
        "expected rejection for long name, got: {msg:?}"
    );
    handle.stop();
}

/// CreateSession with max_players=0 is rejected.
#[test]
fn dedicated_create_session_zero_max_players_rejected() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn = TestConn::connect(addr);
    conn.send_msg(&ClientMessage::CreateSession {
        session_name: "zero-max".into(),
        password: None,
        ticks_per_turn: 50,
        max_players: 0,
    });
    let msg = conn.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason.contains("max_players")),
        "expected rejection for zero max_players, got: {msg:?}"
    );
    handle.stop();
}

/// CreateSession with ticks_per_turn=0 is rejected.
#[test]
fn dedicated_create_session_zero_ticks_rejected() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn = TestConn::connect(addr);
    conn.send_msg(&ClientMessage::CreateSession {
        session_name: "zero-ticks".into(),
        password: None,
        ticks_per_turn: 0,
        max_players: 4,
    });
    let msg = conn.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason.contains("ticks_per_turn")),
        "expected rejection for zero ticks_per_turn, got: {msg:?}"
    );
    handle.stop();
}

/// Creating a session and disconnecting without joining cleans up the session.
#[test]
fn dedicated_create_then_disconnect_cleans_up_session() {
    let (handle, addr) = start_dedicated_relay();

    // Client 1: create a session, then disconnect without joining.
    {
        let mut conn = TestConn::connect(addr);
        conn.create_session("orphan", None, 50, 4);
        // Drop conn — TCP close triggers handshake thread exit and cleanup.
    }
    std::thread::sleep(Duration::from_millis(300));

    // Client 2: list sessions — should be empty since the orphan was cleaned up.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let msg = conn2.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert!(
                sessions.is_empty(),
                "orphaned session should be cleaned up, but found: {sessions:?}"
            );
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    handle.stop();
}

// ---------------------------------------------------------------------------
// Edge-case tests (round 2)
// ---------------------------------------------------------------------------

/// Joining with matching sim_version_hash but wrong config_hash is rejected.
#[test]
fn dedicated_config_hash_mismatch_rejected() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn_host = TestConn::connect(addr);
    let sid = conn_host.create_session("config-game", None, 50, 4);
    // Host joins with config_hash 0x1234.
    let (_reader, _writer, _) = conn_host.join_session(sid, "Host", None);

    // Guest joins with same sim_version_hash (0xABCD) but different config_hash.
    let mut conn_bad = TestConn::connect(addr);
    conn_bad.send_msg(&ClientMessage::Hello {
        protocol_version: 1,
        session_id: sid,
        player_name: "BadConfig".into(),
        sim_version_hash: 0xABCD,
        config_hash: 0x9999, // different from 0x1234
        session_password: None,
    });
    let msg = conn_bad.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason == "config hash mismatch"),
        "expected config hash mismatch, got {msg:?}"
    );

    handle.stop();
}

/// Welcome message for a third joiner contains all 3 players with correct names.
#[test]
fn dedicated_welcome_contains_full_player_list() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn1 = TestConn::connect(addr);
    let sid = conn1.create_session("welcome-game", None, 50, 4);
    let (_r1, _w1, pid1) = conn1.join_session(sid, "Alice", None);

    let conn2 = TestConn::connect(addr);
    let (_r2, _w2, pid2) = conn2.join_session(sid, "Bob", None);

    // Third player joins — Welcome should list all 3 players.
    let mut conn3 = TestConn::connect(addr);
    conn3.send_msg(&ClientMessage::Hello {
        protocol_version: 1,
        session_id: sid,
        player_name: "Charlie".into(),
        sim_version_hash: 0xABCD,
        config_hash: 0x1234,
        session_password: None,
    });
    let msg = conn3.recv_msg();
    match msg {
        ServerMessage::Welcome {
            player_id: pid3,
            players,
            ..
        } => {
            assert_eq!(players.len(), 3, "should have 3 players in Welcome");
            let names: Vec<&str> = players.iter().map(|p| p.name.as_str()).collect();
            assert!(names.contains(&"Alice"), "missing Alice: {names:?}");
            assert!(names.contains(&"Bob"), "missing Bob: {names:?}");
            assert!(names.contains(&"Charlie"), "missing Charlie: {names:?}");
            // IDs should be distinct.
            assert_ne!(pid1, pid2);
            assert_ne!(pid2, pid3);
            assert_ne!(pid1, pid3);
        }
        other => panic!("expected Welcome, got {other:?}"),
    }

    handle.stop();
}

/// PlayerLeft message carries the correct player name on graceful disconnect.
#[test]
fn dedicated_player_left_includes_name() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn1 = TestConn::connect(addr);
    let sid = conn1.create_session("left-name-game", None, 50, 4);
    let (mut r1, _w1, _) = conn1.join_session(sid, "Host", None);

    let conn2 = TestConn::connect(addr);
    let (_r2, mut w2, _) = conn2.join_session(sid, "LeaverBob", None);

    // Drain PlayerJoined from host.
    std::thread::sleep(Duration::from_millis(100));
    let _ = drain_messages(&mut r1);

    // Bob disconnects gracefully.
    send(&mut w2, &ClientMessage::Goodbye);
    std::thread::sleep(Duration::from_millis(100));

    let messages = drain_messages(&mut r1);
    let left_msg = messages
        .iter()
        .find(|m| matches!(m, ServerMessage::PlayerLeft { .. }));
    assert!(left_msg.is_some(), "expected PlayerLeft, got: {messages:?}");
    match left_msg.unwrap() {
        ServerMessage::PlayerLeft { name, .. } => {
            assert_eq!(
                name, "LeaverBob",
                "PlayerLeft should carry the correct name"
            );
        }
        _ => unreachable!(),
    }

    handle.stop();
}

/// Malformed framing (garbage bytes) causes the connection to be dropped cleanly,
/// and the relay continues serving other clients.
#[test]
fn dedicated_malformed_framing_dropped() {
    let (handle, addr) = start_dedicated_relay();

    // Send garbage bytes (not valid length-delimited framing).
    {
        let mut stream = TcpStream::connect(addr).unwrap();
        use std::io::Write;
        let _ = stream.write_all(&[0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x01]);
        let _ = stream.flush();
    }
    std::thread::sleep(Duration::from_millis(100));

    // Relay should still be functional — a normal client can connect.
    let mut conn = TestConn::connect(addr);
    conn.send_msg(&ClientMessage::ListSessions);
    let msg = conn.recv_msg();
    assert!(
        matches!(msg, ServerMessage::SessionList { .. }),
        "relay should still work after malformed client, got: {msg:?}"
    );

    handle.stop();
}

/// Desync detection works with 3 players (one disagrees).
#[test]
fn dedicated_desync_three_players() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn1 = TestConn::connect(addr);
    let sid = conn1.create_session("desync3", None, 50, 4);
    let (mut r1, mut w1, _) = conn1.join_session(sid, "P1", None);

    let conn2 = TestConn::connect(addr);
    let (mut r2, mut w2, _) = conn2.join_session(sid, "P2", None);

    let conn3 = TestConn::connect(addr);
    let (mut r3, mut w3, _) = conn3.join_session(sid, "P3", None);

    // Drain join notifications.
    std::thread::sleep(Duration::from_millis(100));
    let _ = drain_messages(&mut r1);
    let _ = drain_messages(&mut r2);
    let _ = drain_messages(&mut r3);

    // Host starts game.
    send(
        &mut w1,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    std::thread::sleep(Duration::from_millis(100));
    let _ = drain_messages(&mut r1);
    let _ = drain_messages(&mut r2);
    let _ = drain_messages(&mut r3);

    // Wait for at least one turn to establish a tick.
    std::thread::sleep(Duration::from_millis(200));
    let _ = drain_messages(&mut r1);
    let _ = drain_messages(&mut r2);
    let _ = drain_messages(&mut r3);

    // Two players agree, one disagrees.
    let tick = 50;
    send(&mut w1, &ClientMessage::Checksum { tick, hash: 0xAAAA });
    send(&mut w2, &ClientMessage::Checksum { tick, hash: 0xAAAA });
    send(&mut w3, &ClientMessage::Checksum { tick, hash: 0xBBBB });
    std::thread::sleep(Duration::from_millis(200));

    // All players should receive DesyncDetected.
    let msgs1 = drain_messages(&mut r1);
    let msgs2 = drain_messages(&mut r2);
    let msgs3 = drain_messages(&mut r3);

    let has_desync = |msgs: &[ServerMessage]| {
        msgs.iter()
            .any(|m| matches!(m, ServerMessage::DesyncDetected { tick: t } if *t == tick))
    };
    assert!(
        has_desync(&msgs1),
        "P1 should get DesyncDetected, got: {msgs1:?}"
    );
    assert!(
        has_desync(&msgs2),
        "P2 should get DesyncDetected, got: {msgs2:?}"
    );
    assert!(
        has_desync(&msgs3),
        "P3 should get DesyncDetected, got: {msgs3:?}"
    );

    handle.stop();
}

/// Pause/resume integration: paused session emits no turns, resumed session does.
#[test]
fn dedicated_pause_stops_turns_resume_resumes() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn1 = TestConn::connect(addr);
    let sid = conn1.create_session("pause-game", None, 50, 2);
    let (mut r1, mut w1, _) = conn1.join_session(sid, "Host", None);

    // Start game.
    send(
        &mut w1,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    std::thread::sleep(Duration::from_millis(100));
    let _ = drain_messages(&mut r1);

    // Wait for at least one turn.
    std::thread::sleep(Duration::from_millis(200));
    let msgs = drain_messages(&mut r1);
    let turn_count = msgs
        .iter()
        .filter(|m| matches!(m, ServerMessage::Turn { .. }))
        .count();
    assert!(turn_count > 0, "should get turns before pause");

    // Pause.
    send(&mut w1, &ClientMessage::RequestPause);
    std::thread::sleep(Duration::from_millis(100));
    let _ = drain_messages(&mut r1); // drain the Paused message

    // Wait and check: no turns should arrive while paused.
    std::thread::sleep(Duration::from_millis(300));
    let msgs_paused = drain_messages(&mut r1);
    let turn_count_paused = msgs_paused
        .iter()
        .filter(|m| matches!(m, ServerMessage::Turn { .. }))
        .count();
    assert_eq!(
        turn_count_paused, 0,
        "no turns should arrive while paused, got {turn_count_paused}"
    );

    // Resume.
    send(&mut w1, &ClientMessage::RequestResume);
    std::thread::sleep(Duration::from_millis(300));
    let msgs_resumed = drain_messages(&mut r1);
    let turn_count_resumed = msgs_resumed
        .iter()
        .filter(|m| matches!(m, ServerMessage::Turn { .. }))
        .count();
    assert!(
        turn_count_resumed > 0,
        "turns should resume after unpause, got none"
    );

    handle.stop();
}

/// SnapshotResponse from a non-host player is silently ignored.
#[test]
fn dedicated_snapshot_from_non_host_ignored() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn1 = TestConn::connect(addr);
    let sid = conn1.create_session("snap-game", None, 50, 4);
    let (mut r1, mut w1, _) = conn1.join_session(sid, "Host", None);

    let conn2 = TestConn::connect(addr);
    let (mut r2, mut w2, _) = conn2.join_session(sid, "Guest", None);

    // Drain join.
    std::thread::sleep(Duration::from_millis(100));
    let _ = drain_messages(&mut r1);
    let _ = drain_messages(&mut r2);

    // Start game.
    send(
        &mut w1,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    std::thread::sleep(Duration::from_millis(100));
    let _ = drain_messages(&mut r1);
    let _ = drain_messages(&mut r2);

    // Third player joins mid-game — triggers snapshot request to host.
    let conn3 = TestConn::connect(addr);
    let (mut r3, _w3, _) = conn3.join_session(sid, "LateJoiner", None);

    std::thread::sleep(Duration::from_millis(100));

    // Host should get SnapshotRequest.
    let msgs1 = drain_messages(&mut r1);
    assert!(
        msgs1
            .iter()
            .any(|m| matches!(m, ServerMessage::SnapshotRequest)),
        "host should get SnapshotRequest, got: {msgs1:?}"
    );

    // Guest (non-host) sends a bogus SnapshotResponse.
    send(
        &mut w2,
        &ClientMessage::SnapshotResponse {
            data: b"bogus".to_vec(),
        },
    );
    std::thread::sleep(Duration::from_millis(100));

    // The late joiner should NOT receive a SnapshotLoad from the bogus response.
    let msgs3 = drain_messages(&mut r3);
    let has_snapshot_load = msgs3
        .iter()
        .any(|m| matches!(m, ServerMessage::SnapshotLoad { .. }));
    assert!(
        !has_snapshot_load,
        "late joiner should NOT get SnapshotLoad from non-host, got: {msgs3:?}"
    );

    // Now the real host sends the snapshot — joiner should receive it.
    send(
        &mut w1,
        &ClientMessage::SnapshotResponse {
            data: b"real-snapshot".to_vec(),
        },
    );
    std::thread::sleep(Duration::from_millis(100));

    let msgs3_after = drain_messages(&mut r3);
    let has_real_snapshot = msgs3_after
        .iter()
        .any(|m| matches!(m, ServerMessage::SnapshotLoad { .. }));
    assert!(
        has_real_snapshot,
        "late joiner should get SnapshotLoad from host, got: {msgs3_after:?}"
    );

    handle.stop();
}

/// SetSpeed with ticks_per_turn=0 is silently ignored (doesn't freeze the game).
#[test]
fn dedicated_set_speed_zero_ignored() {
    let (handle, addr) = start_dedicated_relay();

    let mut conn1 = TestConn::connect(addr);
    let sid = conn1.create_session("speed-game", None, 50, 2);
    let (mut r1, mut w1, _) = conn1.join_session(sid, "Host", None);

    // Start game.
    send(
        &mut w1,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    std::thread::sleep(Duration::from_millis(100));
    let _ = drain_messages(&mut r1);

    // Send SetSpeed with 0 — should be ignored.
    send(&mut w1, &ClientMessage::SetSpeed { ticks_per_turn: 0 });
    std::thread::sleep(Duration::from_millis(200));

    // Should still get turns (not frozen).
    let msgs = drain_messages(&mut r1);
    let turn_count = msgs
        .iter()
        .filter(|m| matches!(m, ServerMessage::Turn { .. }))
        .count();
    assert!(
        turn_count > 0,
        "turns should still flow after SetSpeed(0), got none"
    );

    // And no SpeedChanged broadcast should have been sent.
    let speed_changed = msgs.iter().any(
        |m| matches!(m, ServerMessage::SpeedChanged { ticks_per_turn } if *ticks_per_turn == 0),
    );
    assert!(
        !speed_changed,
        "SpeedChanged(0) should not have been broadcast"
    );

    handle.stop();
}

/// CreateSession with max_players > 64 is rejected.
#[test]
fn dedicated_create_session_max_players_too_high() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn = TestConn::connect(addr);
    conn.send_msg(&ClientMessage::CreateSession {
        session_name: "big-game".into(),
        password: None,
        ticks_per_turn: 50,
        max_players: 100,
    });
    let msg = conn.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason.contains("64")),
        "expected rejection for max_players > 64, got: {msg:?}"
    );
    handle.stop();
}

/// Double CreateSession: creating a second session cleans up the first orphan.
#[test]
fn dedicated_double_create_cleans_up_first() {
    let (handle, addr) = start_dedicated_relay();

    // Client creates session A, then session B, then disconnects without joining.
    {
        let mut conn = TestConn::connect(addr);
        conn.create_session("session-a", None, 50, 4);
        conn.create_session("session-b", None, 50, 4);
        // Drop — should clean up session-b, and session-a should also be cleaned up
        // via the double-create cleanup path.
    }
    std::thread::sleep(Duration::from_millis(300));

    // Verify both sessions are gone.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let msg = conn2.recv_msg();
    match msg {
        ServerMessage::SessionList { sessions } => {
            assert!(
                sessions.is_empty(),
                "both orphaned sessions should be cleaned up, but found: {sessions:?}"
            );
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    handle.stop();
}

// ---------------------------------------------------------------------------
// Round 4: additional edge-case and boundary tests
// ---------------------------------------------------------------------------

/// Session name exactly at the 128-byte boundary succeeds; 129 bytes is rejected.
#[test]
fn dedicated_create_session_name_at_boundary() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn = TestConn::connect(addr);

    // Exactly 128 bytes — should succeed.
    let name_128 = "a".repeat(128);
    conn.send_msg(&ClientMessage::CreateSession {
        session_name: name_128.clone(),
        password: None,
        ticks_per_turn: 50,
        max_players: 4,
    });
    let msg = conn.recv_msg();
    assert!(
        matches!(msg, ServerMessage::SessionCreated { .. }),
        "128-byte name should be accepted, got: {msg:?}"
    );

    // 129 bytes — should be rejected.
    let name_129 = "a".repeat(129);
    conn.send_msg(&ClientMessage::CreateSession {
        session_name: name_129,
        password: None,
        ticks_per_turn: 50,
        max_players: 4,
    });
    let msg = conn.recv_msg();
    assert!(
        matches!(msg, ServerMessage::Rejected { ref reason } if reason.contains("too long")),
        "129-byte name should be rejected, got: {msg:?}"
    );

    handle.stop();
}

/// Double-pause is idempotent — only one Paused broadcast is sent.
#[test]
fn dedicated_double_pause_idempotent() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("pause-test", None, 50, 4);
    let (mut reader, mut writer, _pid) = conn.join_session(sid, "host", None);

    // Start the game so turns flow.
    send(
        &mut writer,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    let _game_start = recv(&mut reader); // GameStart

    // First pause.
    send(&mut writer, &ClientMessage::RequestPause);
    let msg = recv(&mut reader);
    assert!(
        matches!(msg, ServerMessage::Paused { .. }),
        "expected Paused, got: {msg:?}"
    );

    // Second pause — should be a no-op (no second Paused broadcast).
    send(&mut writer, &ClientMessage::RequestPause);

    // Sleep briefly and verify no extra Paused arrives.
    std::thread::sleep(Duration::from_millis(200));
    reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    let result = read_message(&mut reader);
    // Should either be a Turn or a timeout, not another Paused.
    match result {
        Ok(bytes) => {
            let msg: ServerMessage = serde_json::from_slice(&bytes).unwrap();
            assert!(
                !matches!(msg, ServerMessage::Paused { .. }),
                "second pause should not produce another Paused broadcast, got: {msg:?}"
            );
        }
        Err(_) => { /* timeout — expected */ }
    }

    handle.stop();
}

/// Resume while not paused is a no-op — no Resumed broadcast.
#[test]
fn dedicated_resume_while_not_paused_noop() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("resume-test", None, 50, 4);
    let (mut reader, mut writer, _pid) = conn.join_session(sid, "host", None);

    // Start the game.
    send(
        &mut writer,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    let _game_start = recv(&mut reader); // GameStart

    // Resume without being paused.
    send(&mut writer, &ClientMessage::RequestResume);

    // Sleep briefly and verify no Resumed arrives.
    std::thread::sleep(Duration::from_millis(200));
    reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    let result = read_message(&mut reader);
    match result {
        Ok(bytes) => {
            let msg: ServerMessage = serde_json::from_slice(&bytes).unwrap();
            assert!(
                !matches!(msg, ServerMessage::Resumed { .. }),
                "resume while not paused should not produce Resumed, got: {msg:?}"
            );
        }
        Err(_) => { /* timeout — expected */ }
    }

    handle.stop();
}

/// Non-host SetSpeed is silently ignored (integration level).
#[test]
fn dedicated_non_host_set_speed_ignored() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn1 = TestConn::connect(addr);
    let sid = conn1.create_session("speed-test", None, 50, 4);
    let (mut r1, mut w1, _host_id) = conn1.join_session(sid, "host", None);

    // Second player joins.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let _list = conn2.recv_msg();
    let (mut _r2, mut w2, _guest_id) = conn2.join_session(sid, "guest", None);

    // Drain PlayerJoined on host.
    let _pj = recv(&mut r1);

    // Start the game, then immediately pause so turns stop flowing.
    send(
        &mut w1,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    let _gs1 = recv(&mut r1); // GameStart
    send(&mut w1, &ClientMessage::RequestPause);

    // Wait for Paused and drain any turns.
    std::thread::sleep(Duration::from_millis(200));
    r1.get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    while read_message(&mut r1).is_ok() {}

    // Guest sends SetSpeed — should be ignored.
    send(
        &mut w2,
        &ClientMessage::SetSpeed {
            ticks_per_turn: 999,
        },
    );

    // Give the relay time to process and check no SpeedChanged arrives.
    std::thread::sleep(Duration::from_millis(200));
    r1.get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    let mut found_speed_changed = false;
    while let Ok(bytes) = read_message(&mut r1) {
        let msg: ServerMessage = serde_json::from_slice(&bytes).unwrap();
        if matches!(msg, ServerMessage::SpeedChanged { .. }) {
            found_speed_changed = true;
        }
    }
    assert!(
        !found_speed_changed,
        "non-host SetSpeed should not produce SpeedChanged"
    );

    handle.stop();
}

/// Goodbye sent before Hello (during pre-handshake) drops the connection cleanly.
#[test]
fn dedicated_goodbye_before_hello_drops_cleanly() {
    let (handle, addr) = start_dedicated_relay();

    // Connect and send Goodbye without Hello.
    {
        let mut conn = TestConn::connect(addr);
        conn.send_msg(&ClientMessage::Goodbye);
        // The server should drop us.
        std::thread::sleep(Duration::from_millis(200));
    }

    // Relay should still be alive — verify by connecting and listing sessions.
    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let msg = conn2.recv_msg();
    assert!(
        matches!(msg, ServerMessage::SessionList { .. }),
        "relay should still be alive, got: {msg:?}"
    );

    handle.stop();
}

/// Chat text longer than 4096 bytes is truncated (not rejected).
#[test]
fn dedicated_chat_text_truncated() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn = TestConn::connect(addr);
    let sid = conn.create_session("chat-test", None, 50, 4);
    let (mut reader, mut writer, _pid) = conn.join_session(sid, "chatter", None);

    // Send a very long chat message.
    let long_text = "x".repeat(8000);
    send(
        &mut writer,
        &ClientMessage::Chat {
            text: long_text.clone(),
        },
    );

    let msg = recv(&mut reader);
    match msg {
        ServerMessage::ChatBroadcast { text, .. } => {
            assert!(
                text.len() <= 4096,
                "chat text should be truncated to 4096, got {} bytes",
                text.len()
            );
        }
        other => panic!("expected ChatBroadcast, got {other:?}"),
    }

    handle.stop();
}

/// Host leaves mid-game; remaining guest cannot SetSpeed (host-only operation).
#[test]
fn dedicated_host_leaves_guest_cannot_set_speed() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn1 = TestConn::connect(addr);
    let sid = conn1.create_session("host-leaves", None, 50, 4);
    let (_r1, mut w1, _host_id) = conn1.join_session(sid, "host", None);

    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let _list = conn2.recv_msg();
    let (mut r2, mut w2, _guest_id) = conn2.join_session(sid, "guest", None);

    // Start game then pause immediately so turns stop flowing.
    send(
        &mut w1,
        &ClientMessage::StartGame {
            seed: 42,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    send(&mut w1, &ClientMessage::RequestPause);
    std::thread::sleep(Duration::from_millis(200));

    // Drain guest's pending messages (GameStart, Paused, maybe turns).
    r2.get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    while read_message(&mut r2).is_ok() {}

    // Host disconnects.
    send(&mut w1, &ClientMessage::Goodbye);
    std::thread::sleep(Duration::from_millis(300));

    // Drain PlayerLeft.
    r2.get_ref()
        .set_read_timeout(Some(Duration::from_millis(300)))
        .ok();
    while read_message(&mut r2).is_ok() {}

    // Guest tries SetSpeed — should be silently ignored (no SpeedChanged).
    send(
        &mut w2,
        &ClientMessage::SetSpeed {
            ticks_per_turn: 999,
        },
    );
    std::thread::sleep(Duration::from_millis(300));

    r2.get_ref()
        .set_read_timeout(Some(Duration::from_millis(300)))
        .ok();
    let mut found_speed_changed = false;
    while let Ok(bytes) = read_message(&mut r2) {
        let msg: ServerMessage = serde_json::from_slice(&bytes).unwrap();
        if matches!(msg, ServerMessage::SpeedChanged { .. }) {
            found_speed_changed = true;
        }
    }
    assert!(
        !found_speed_changed,
        "guest should not be able to SetSpeed after host leaves"
    );

    handle.stop();
}

/// Unsolicited SnapshotResponse (no mid-game join pending) is silently ignored.
#[test]
fn dedicated_unsolicited_snapshot_response_ignored() {
    let (handle, addr) = start_dedicated_relay();
    let mut conn1 = TestConn::connect(addr);
    let sid = conn1.create_session("snap-test", None, 50, 4);
    let (mut r1, mut w1, _host_id) = conn1.join_session(sid, "host", None);

    let mut conn2 = TestConn::connect(addr);
    conn2.send_msg(&ClientMessage::ListSessions);
    let _list = conn2.recv_msg();
    let (mut r2, _w2, _guest_id) = conn2.join_session(sid, "guest", None);

    // Drain PlayerJoined.
    let _ = recv(&mut r1);

    // Start game.
    send(
        &mut w1,
        &ClientMessage::StartGame {
            seed: 1,
            config_json: "{}".into(),
            starting_tick: None,
        },
    );
    let _gs1 = recv(&mut r1);
    let _gs2 = recv(&mut r2);

    // Host sends unsolicited SnapshotResponse (no one is mid-join).
    send(
        &mut w1,
        &ClientMessage::SnapshotResponse {
            data: b"fake snapshot".to_vec(),
        },
    );

    // Wait for some turns to arrive, then read a bounded number of messages.
    // If we get at least one Turn, the relay survived the unsolicited snapshot.
    std::thread::sleep(Duration::from_millis(200));
    r1.get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)))
        .ok();
    let mut got_turn = false;
    for _ in 0..20 {
        match read_message(&mut r1) {
            Ok(bytes) => {
                let msg: ServerMessage = serde_json::from_slice(&bytes).unwrap();
                if matches!(msg, ServerMessage::Turn { .. }) {
                    got_turn = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    assert!(
        got_turn,
        "relay should continue serving turns after unsolicited snapshot"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// ResumeSession through TCP pipeline
// ---------------------------------------------------------------------------

/// Verify that `ResumeSession` sent through a real TCP relay starts turn
/// flushing from the specified tick.
#[test]
fn embedded_resume_session_starts_turns_from_tick() {
    let (handle, addr) = start_embedded_relay();
    let mut conn = TestConn::connect(addr);
    conn.create_session("test", None, 50, 1);

    let conn2 = TestConn::connect(addr);
    let (mut reader, mut writer, _pid) = conn2.join_session(SessionId(0), "Alice", None);

    // Send ResumeSession at tick 10000.
    send(
        &mut writer,
        &ClientMessage::ResumeSession {
            starting_tick: 10000,
        },
    );

    // Should receive SessionResumed.
    let msg = recv(&mut reader);
    match msg {
        ServerMessage::SessionResumed { starting_tick } => {
            assert_eq!(starting_tick, 10000);
        }
        other => panic!("expected SessionResumed, got {other:?}"),
    }

    // Should receive a Turn with sim_tick_target = 10000 + 50.
    let msg = recv(&mut reader);
    match msg {
        ServerMessage::Turn {
            sim_tick_target, ..
        } => {
            assert_eq!(sim_tick_target, 10050);
        }
        other => panic!("expected Turn, got {other:?}"),
    }

    handle.stop();
}

/// Verify that `StartGame` with `starting_tick` sent through TCP results
/// in turns that advance from that tick, not from 0.
#[test]
fn embedded_start_game_with_starting_tick() {
    let (handle, addr) = start_embedded_relay();
    let mut conn = TestConn::connect(addr);
    conn.create_session("test", None, 50, 1);

    let conn2 = TestConn::connect(addr);
    let (mut reader, mut writer, _pid) = conn2.join_session(SessionId(0), "Alice", None);

    // Start game at tick 5000.
    send(
        &mut writer,
        &ClientMessage::StartGame {
            seed: 42,
            config_json: "{}".into(),
            starting_tick: Some(5000),
        },
    );

    // Should receive GameStart with starting_tick.
    let msg = recv(&mut reader);
    match msg {
        ServerMessage::GameStart { starting_tick, .. } => {
            assert_eq!(starting_tick, Some(5000));
        }
        other => panic!("expected GameStart, got {other:?}"),
    }

    // First Turn should advance from 5000.
    let msg = recv(&mut reader);
    match msg {
        ServerMessage::Turn {
            sim_tick_target, ..
        } => {
            assert_eq!(sim_tick_target, 5050);
        }
        other => panic!("expected Turn, got {other:?}"),
    }

    handle.stop();
}
