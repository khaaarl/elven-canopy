// Protocol messages for client-relay communication.
//
// Two enums define the full protocol vocabulary:
// - `ClientMessage`: sent by game clients to the relay coordinator.
// - `ServerMessage`: sent by the relay coordinator to game clients.
//
// Supporting structs (`TurnCommand`, `PlayerInfo`) are shared by both
// directions. All types derive `Serialize`/`Deserialize` for JSON framing
// (see `framing.rs`).
//
// Commands are opaque byte payloads (`Vec<u8>`) — the relay never inspects
// them. This keeps the protocol crate independent of the sim crate. The client
// serializes a `SimAction` into bytes before sending and deserializes after
// receiving.

use serde::{Deserialize, Serialize};

use crate::types::{ActionSequence, RelayPlayerId, TurnNumber};

/// Messages sent by a client to the relay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Join a session (handshake).
    Hello {
        protocol_version: u32,
        player_name: String,
        sim_version_hash: u64,
        config_hash: u64,
        session_password: Option<String>,
    },
    /// A sim command (opaque payload).
    Command {
        sequence: ActionSequence,
        payload: Vec<u8>,
    },
    /// Periodic state checksum for desync detection.
    Checksum { tick: u64, hash: u64 },
    /// Request to change sim speed (host only).
    SetSpeed { ticks_per_turn: u32 },
    /// Request to pause.
    RequestPause,
    /// Request to resume.
    RequestResume,
    /// Chat message.
    Chat { text: String },
    /// Response to a snapshot request (mid-game join).
    SnapshotResponse { data: Vec<u8> },
    /// Host triggers game start (lobby → playing transition).
    StartGame { seed: i64, config_json: String },
    /// Player is leaving gracefully.
    Goodbye,
}

/// Messages sent by the relay to a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Handshake accepted.
    Welcome {
        player_id: RelayPlayerId,
        session_name: String,
        players: Vec<PlayerInfo>,
        ticks_per_turn: u32,
    },
    /// Handshake rejected.
    Rejected { reason: String },
    /// A batch of commands for one turn.
    Turn {
        turn_number: TurnNumber,
        sim_tick_target: u64,
        commands: Vec<TurnCommand>,
    },
    /// A player connected.
    PlayerJoined { player: PlayerInfo },
    /// A player disconnected.
    PlayerLeft {
        player_id: RelayPlayerId,
        name: String,
    },
    /// Desync detected between clients.
    DesyncDetected { tick: u64 },
    /// Request a state snapshot (for mid-game join).
    SnapshotRequest,
    /// Load this snapshot (sent to joining client).
    SnapshotLoad { tick: u64, data: Vec<u8> },
    /// Session is paused.
    Paused { by: RelayPlayerId },
    /// Session is resumed.
    Resumed { by: RelayPlayerId },
    /// Chat from another player.
    ChatBroadcast {
        from: RelayPlayerId,
        name: String,
        text: String,
    },
    /// Speed changed.
    SpeedChanged { ticks_per_turn: u32 },
    /// Game is starting — all clients should init sim with this seed/config.
    GameStart { seed: i64, config_json: String },
}

/// A single command within a turn, tagged with the originating player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnCommand {
    pub player_id: RelayPlayerId,
    pub sequence: ActionSequence,
    pub payload: Vec<u8>,
}

/// Public identity of a connected player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: RelayPlayerId,
    pub name: String,
}
