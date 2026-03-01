// Integration smoke test for the relay server.
//
// Starts a relay on localhost, connects two mock TCP clients, exercises the
// full protocol lifecycle: handshake, command exchange, turn broadcasting,
// checksum-based desync detection, and graceful disconnect.
//
// Each client is a plain TCP socket using the protocol crate's framing and
// message types — no game code involved. This tests the relay end-to-end
// without any sim or Godot dependency.

use std::io::{BufReader, BufWriter};
use std::net::TcpStream;
use std::time::Duration;

use elven_canopy_protocol::framing::{read_message, write_message};
use elven_canopy_protocol::message::{ClientMessage, ServerMessage};
use elven_canopy_protocol::types::{ActionSequence, RelayPlayerId, TurnNumber};
use elven_canopy_relay::server::{RelayConfig, start_relay};

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

/// Connect to the relay and perform the Hello handshake. Returns the
/// reader/writer pair and the assigned player ID.
fn connect_and_hello(
    addr: std::net::SocketAddr,
    name: &str,
) -> (BufReader<TcpStream>, BufWriter<TcpStream>, RelayPlayerId) {
    let stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let reader_stream = stream.try_clone().unwrap();
    let mut writer = BufWriter::new(stream);
    let mut reader = BufReader::new(reader_stream);

    send(
        &mut writer,
        &ClientMessage::Hello {
            protocol_version: 1,
            player_name: name.into(),
            sim_version_hash: 0xABCD,
            config_hash: 0x1234,
            session_password: None,
        },
    );

    let msg = recv(&mut reader);
    let player_id = match msg {
        ServerMessage::Welcome { player_id, .. } => player_id,
        other => panic!("expected Welcome, got {other:?}"),
    };

    (reader, writer, player_id)
}

#[test]
fn full_session_lifecycle() {
    // 1. Start a relay on a random port.
    let config = RelayConfig {
        port: 0, // OS picks a free port
        session_name: "smoke-test".into(),
        password: None,
        ticks_per_turn: 50,
        max_players: 4,
    };
    let (handle, addr) = start_relay(config).unwrap();

    // Give the listener thread a moment to start.
    std::thread::sleep(Duration::from_millis(50));

    // 2. Connect two clients — both do Hello handshake and receive Welcome.
    let (mut reader_a, mut writer_a, id_a) = connect_and_hello(addr, "Alice");
    assert_eq!(id_a, RelayPlayerId(0));

    let (mut reader_b, mut writer_b, id_b) = connect_and_hello(addr, "Bob");
    assert_eq!(id_b, RelayPlayerId(1));

    // Alice should receive PlayerJoined for Bob.
    let msg = recv(&mut reader_a);
    match msg {
        ServerMessage::PlayerJoined { player } => {
            assert_eq!(player.id, RelayPlayerId(1));
            assert_eq!(player.name, "Bob");
        }
        other => panic!("expected PlayerJoined, got {other:?}"),
    }

    // 3. Host starts the game (required before turns will flush).
    send(
        &mut writer_a,
        &ClientMessage::StartGame {
            seed: 42,
            config_json: "{}".into(),
        },
    );

    // Both clients should receive GameStart.
    let msg_a = recv(&mut reader_a);
    assert!(
        matches!(msg_a, ServerMessage::GameStart { seed: 42, .. }),
        "expected GameStart, got {msg_a:?}"
    );
    let msg_b = recv(&mut reader_b);
    assert!(
        matches!(msg_b, ServerMessage::GameStart { seed: 42, .. }),
        "expected GameStart, got {msg_b:?}"
    );

    // 4. Client A sends a Command. Wait for the turn to be flushed.
    //    Both clients should receive it in the next Turn.
    send(
        &mut writer_a,
        &ClientMessage::Command {
            sequence: ActionSequence(0),
            payload: vec![10, 20, 30],
        },
    );

    // Wait for the turn timer to fire (ticks_per_turn=50, so ~50ms cadence).
    // Read from both clients until we get a Turn message.
    let turn_a = wait_for_turn_with_commands(&mut reader_a);
    let turn_b = wait_for_turn_with_commands(&mut reader_b);

    // Both should receive the same turn with Alice's command.
    assert_eq!(turn_a.0, turn_b.0, "turn numbers should match");
    assert_eq!(turn_a.1, turn_b.1, "sim_tick_targets should match");
    assert_eq!(turn_a.2.len(), 1, "should have 1 command");
    assert_eq!(turn_b.2.len(), 1, "should have 1 command");
    assert_eq!(turn_a.2[0].player_id, RelayPlayerId(0));
    assert_eq!(turn_a.2[0].payload, vec![10, 20, 30]);

    // 5. Both send matching Checksums — no DesyncDetected.
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

    // Wait a bit and drain any turns — should NOT receive DesyncDetected.
    std::thread::sleep(Duration::from_millis(150));
    let messages_a = drain_messages(&mut reader_a);
    assert!(
        !messages_a
            .iter()
            .any(|m| matches!(m, ServerMessage::DesyncDetected { .. })),
        "should not get DesyncDetected with matching checksums"
    );

    // 6. Client B sends a different Checksum — verify DesyncDetected.
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

    // Wait for the relay to process and broadcast DesyncDetected.
    std::thread::sleep(Duration::from_millis(150));
    let messages_a = drain_messages(&mut reader_a);
    assert!(
        messages_a
            .iter()
            .any(|m| matches!(m, ServerMessage::DesyncDetected { tick: 100 })),
        "should get DesyncDetected with mismatching checksums, got: {messages_a:?}"
    );

    // 7. Client A sends Goodbye — Client B should receive PlayerLeft.
    send(&mut writer_a, &ClientMessage::Goodbye);

    // Wait for relay to process the disconnect.
    std::thread::sleep(Duration::from_millis(150));
    let messages_b = drain_messages(&mut reader_b);
    assert!(
        messages_b.iter().any(|m| matches!(
            m,
            ServerMessage::PlayerLeft {
                player_id: RelayPlayerId(0),
                ..
            }
        )),
        "should get PlayerLeft for Alice, got: {messages_b:?}"
    );

    // 8. Graceful shutdown.
    drop(writer_b);
    drop(reader_b);
    handle.stop();
}

#[test]
fn rejected_wrong_password() {
    let config = RelayConfig {
        port: 0,
        session_name: "password-test".into(),
        password: Some("secret".into()),
        ticks_per_turn: 50,
        max_players: 4,
    };
    let (handle, addr) = start_relay(config).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    // First player joins with correct password.
    let (_reader_a, _writer_a, _id_a) =
        connect_and_hello_with_password(addr, "Alice", Some("secret"));

    // Second player tries wrong password.
    let stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let reader_stream = stream.try_clone().unwrap();
    let mut writer = BufWriter::new(stream);
    let mut reader = BufReader::new(reader_stream);

    send(
        &mut writer,
        &ClientMessage::Hello {
            protocol_version: 1,
            player_name: "Intruder".into(),
            sim_version_hash: 0xABCD,
            config_hash: 0x1234,
            session_password: Some("wrong".into()),
        },
    );

    let msg = recv(&mut reader);
    match msg {
        ServerMessage::Rejected { reason } => {
            assert_eq!(reason, "incorrect password");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }

    handle.stop();
}

#[test]
fn chat_between_clients() {
    let config = RelayConfig {
        port: 0,
        session_name: "chat-test".into(),
        password: None,
        ticks_per_turn: 50,
        max_players: 4,
    };
    let (handle, addr) = start_relay(config).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let (mut reader_a, _writer_a, _id_a) = connect_and_hello(addr, "Alice");
    let (_reader_b, mut writer_b, _) = connect_and_hello(addr, "Bob");

    // Drain Alice's PlayerJoined notification.
    let _ = recv(&mut reader_a);

    // Bob sends a chat message.
    send(
        &mut writer_b,
        &ClientMessage::Chat {
            text: "hello!".into(),
        },
    );

    // Wait for relay to process.
    std::thread::sleep(Duration::from_millis(100));
    let messages_a = drain_messages(&mut reader_a);
    assert!(
        messages_a.iter().any(|m| matches!(
            m,
            ServerMessage::ChatBroadcast { text, .. } if text == "hello!"
        )),
        "Alice should receive Bob's chat, got: {messages_a:?}"
    );

    handle.stop();
}

// --- Helpers ---

fn connect_and_hello_with_password(
    addr: std::net::SocketAddr,
    name: &str,
    password: Option<&str>,
) -> (BufReader<TcpStream>, BufWriter<TcpStream>, RelayPlayerId) {
    let stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let reader_stream = stream.try_clone().unwrap();
    let mut writer = BufWriter::new(stream);
    let mut reader = BufReader::new(reader_stream);

    send(
        &mut writer,
        &ClientMessage::Hello {
            protocol_version: 1,
            player_name: name.into(),
            sim_version_hash: 0xABCD,
            config_hash: 0x1234,
            session_password: password.map(String::from),
        },
    );

    let msg = recv(&mut reader);
    let player_id = match msg {
        ServerMessage::Welcome { player_id, .. } => player_id,
        other => panic!("expected Welcome, got {other:?}"),
    };

    (reader, writer, player_id)
}

/// Read messages until we get a Turn with at least one command (skipping
/// empty turns and other message types). Returns (turn_number,
/// sim_tick_target, commands).
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
        {
            if !commands.is_empty() {
                return (turn_number, sim_tick_target, commands);
            }
        }
        // Skip empty turns and other messages.
    }
    panic!("did not receive Turn with commands within 50 reads");
}

/// Drain all currently buffered messages using a short read timeout.
/// The timeout (10ms) must be shorter than the relay's turn cadence (50ms)
/// to avoid reading an endless stream of empty turns.
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
