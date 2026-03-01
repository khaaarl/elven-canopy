// elven_canopy_protocol — wire protocol for multiplayer relay communication.
//
// This crate defines the message types, framing, and serialization used by the
// relay coordinator (`elven_canopy_relay`) and game clients to communicate over
// TCP. It is shared between both sides and has no dependency on the sim or
// Godot crates.
//
// Module overview:
// - `types.rs`:    Core ID types — `RelayPlayerId`, `TurnNumber`, `ActionSequence`.
// - `message.rs`:  Client-to-relay and relay-to-client message enums, plus
//                  supporting structs (`TurnCommand`, `PlayerInfo`).
// - `framing.rs`:  Length-delimited framing over any `Read`/`Write` stream:
//                  4-byte big-endian length prefix, then JSON payload.
//
// Design decisions:
// - **JSON serialization.** Matches the sim's existing serde_json usage. Binary
//   framing can be swapped in later if bandwidth matters.
// - **Commands as opaque `Vec<u8>`.** The relay never inspects command payloads.
//   This keeps the protocol crate independent of the sim crate.
// - **No async runtime.** Uses `std::io::Read`/`Write` for framing, compatible
//   with both blocking TCP streams and buffered wrappers.

pub mod framing;
pub mod message;
pub mod types;

pub use framing::{MAX_MESSAGE_SIZE, read_message, write_message};
pub use message::{ClientMessage, PlayerInfo, ServerMessage, TurnCommand};
pub use types::{ActionSequence, RelayPlayerId, TurnNumber};

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// Serialize a ClientMessage to JSON, frame it, read it back, deserialize.
    fn client_roundtrip(msg: &ClientMessage) {
        let json = serde_json::to_vec(msg).unwrap();
        let mut wire = Vec::new();
        write_message(&mut wire, &json).unwrap();

        let mut cursor = Cursor::new(&wire);
        let recovered_json = read_message(&mut cursor).unwrap();
        let recovered: ClientMessage = serde_json::from_slice(&recovered_json).unwrap();
        assert_eq!(&recovered, msg);
    }

    /// Serialize a ServerMessage to JSON, frame it, read it back, deserialize.
    fn server_roundtrip(msg: &ServerMessage) {
        let json = serde_json::to_vec(msg).unwrap();
        let mut wire = Vec::new();
        write_message(&mut wire, &json).unwrap();

        let mut cursor = Cursor::new(&wire);
        let recovered_json = read_message(&mut cursor).unwrap();
        let recovered: ServerMessage = serde_json::from_slice(&recovered_json).unwrap();
        assert_eq!(&recovered, msg);
    }

    #[test]
    fn roundtrip_hello() {
        client_roundtrip(&ClientMessage::Hello {
            protocol_version: 1,
            player_name: "TestElf".into(),
            sim_version_hash: 0xDEAD_BEEF,
            config_hash: 0xCAFE_BABE,
            session_password: Some("secret".into()),
        });
    }

    #[test]
    fn roundtrip_hello_no_password() {
        client_roundtrip(&ClientMessage::Hello {
            protocol_version: 1,
            player_name: "TestElf".into(),
            sim_version_hash: 0xDEAD_BEEF,
            config_hash: 0xCAFE_BABE,
            session_password: None,
        });
    }

    #[test]
    fn roundtrip_command() {
        client_roundtrip(&ClientMessage::Command {
            sequence: ActionSequence(42),
            payload: vec![1, 2, 3, 4, 5],
        });
    }

    #[test]
    fn roundtrip_checksum() {
        client_roundtrip(&ClientMessage::Checksum {
            tick: 1000,
            hash: 0x1234_5678_9ABC_DEF0,
        });
    }

    #[test]
    fn roundtrip_set_speed() {
        client_roundtrip(&ClientMessage::SetSpeed {
            ticks_per_turn: 100,
        });
    }

    #[test]
    fn roundtrip_request_pause() {
        client_roundtrip(&ClientMessage::RequestPause);
    }

    #[test]
    fn roundtrip_request_resume() {
        client_roundtrip(&ClientMessage::RequestResume);
    }

    #[test]
    fn roundtrip_chat() {
        client_roundtrip(&ClientMessage::Chat {
            text: "Hello everyone!".into(),
        });
    }

    #[test]
    fn roundtrip_snapshot_response() {
        client_roundtrip(&ClientMessage::SnapshotResponse {
            data: vec![0xFF; 256],
        });
    }

    #[test]
    fn roundtrip_start_game() {
        client_roundtrip(&ClientMessage::StartGame {
            seed: 12345,
            config_json: r#"{"tick_duration_ms":1}"#.into(),
        });
    }

    #[test]
    fn roundtrip_goodbye() {
        client_roundtrip(&ClientMessage::Goodbye);
    }

    #[test]
    fn roundtrip_welcome() {
        server_roundtrip(&ServerMessage::Welcome {
            player_id: RelayPlayerId(1),
            session_name: "amber-willow-42".into(),
            players: vec![
                PlayerInfo {
                    id: RelayPlayerId(0),
                    name: "Host".into(),
                },
                PlayerInfo {
                    id: RelayPlayerId(1),
                    name: "Guest".into(),
                },
            ],
            ticks_per_turn: 50,
        });
    }

    #[test]
    fn roundtrip_rejected() {
        server_roundtrip(&ServerMessage::Rejected {
            reason: "version mismatch".into(),
        });
    }

    #[test]
    fn roundtrip_turn() {
        server_roundtrip(&ServerMessage::Turn {
            turn_number: TurnNumber(10),
            sim_tick_target: 500,
            commands: vec![
                TurnCommand {
                    player_id: RelayPlayerId(0),
                    sequence: ActionSequence(1),
                    payload: vec![10, 20],
                },
                TurnCommand {
                    player_id: RelayPlayerId(1),
                    sequence: ActionSequence(0),
                    payload: vec![30],
                },
            ],
        });
    }

    #[test]
    fn roundtrip_turn_empty() {
        server_roundtrip(&ServerMessage::Turn {
            turn_number: TurnNumber(99),
            sim_tick_target: 4950,
            commands: vec![],
        });
    }

    #[test]
    fn roundtrip_player_joined() {
        server_roundtrip(&ServerMessage::PlayerJoined {
            player: PlayerInfo {
                id: RelayPlayerId(3),
                name: "Newcomer".into(),
            },
        });
    }

    #[test]
    fn roundtrip_player_left() {
        server_roundtrip(&ServerMessage::PlayerLeft {
            player_id: RelayPlayerId(2),
            name: "Leaver".into(),
        });
    }

    #[test]
    fn roundtrip_desync_detected() {
        server_roundtrip(&ServerMessage::DesyncDetected { tick: 5000 });
    }

    #[test]
    fn roundtrip_snapshot_request() {
        server_roundtrip(&ServerMessage::SnapshotRequest);
    }

    #[test]
    fn roundtrip_snapshot_load() {
        server_roundtrip(&ServerMessage::SnapshotLoad {
            tick: 3000,
            data: vec![0xAB; 128],
        });
    }

    #[test]
    fn roundtrip_paused() {
        server_roundtrip(&ServerMessage::Paused {
            by: RelayPlayerId(0),
        });
    }

    #[test]
    fn roundtrip_resumed() {
        server_roundtrip(&ServerMessage::Resumed {
            by: RelayPlayerId(1),
        });
    }

    #[test]
    fn roundtrip_chat_broadcast() {
        server_roundtrip(&ServerMessage::ChatBroadcast {
            from: RelayPlayerId(0),
            name: "Host".into(),
            text: "Welcome to the game!".into(),
        });
    }

    #[test]
    fn roundtrip_speed_changed() {
        server_roundtrip(&ServerMessage::SpeedChanged {
            ticks_per_turn: 100,
        });
    }

    #[test]
    fn roundtrip_game_start() {
        server_roundtrip(&ServerMessage::GameStart {
            seed: 98765,
            config_json: r#"{"tick_duration_ms":1}"#.into(),
        });
    }
}
