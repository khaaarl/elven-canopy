// TCP client for connecting to the multiplayer relay.
//
// Provides a non-blocking interface for the main thread to communicate
// with the relay server. Architecture:
// - `RelayConnection::connect()` establishes the TCP transport.
// - Pre-handshake methods (`list_sessions`, `create_session`) can be called
//   before joining a session.
// - `join_session()` sends the `Hello` handshake, receives `Welcome`, and
//   spawns a background reader thread, returning a `NetClient`.
// - The reader thread calls `read_message()` in a loop, deserializes
//   `ServerMessage`, and pushes into an `mpsc` channel.
// - The main thread holds a `BufWriter<TcpStream>` for sending.
// - `poll()` drains the inbox non-blocking, returning all queued messages.
//
// This separation ensures the main thread never blocks on network I/O. The
// reader thread handles the blocking reads, and the writer flushes
// synchronously (acceptable for the small messages we send).
//
// This module lives in the relay crate (not gdext) because it has zero Godot
// dependencies — it's purely std TCP + protocol framing + mpsc. Living here
// makes it available to any crate (including integration tests) without
// pulling in Godot.
//
// See also: `sim_bridge.rs` in the gdext crate, which owns a `NetClient`
// and calls its methods from `#[func]` methods exposed to GDScript.

use std::io::{BufReader, BufWriter};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

use elven_canopy_protocol::framing::{read_message, write_message};
use elven_canopy_protocol::message::{ClientMessage, PlayerInfo, ServerMessage, SessionInfo};
use elven_canopy_protocol::types::{ActionSequence, RelayPlayerId, SessionId};

/// Information returned by a successful `join_session()` handshake.
pub struct WelcomeInfo {
    pub player_id: RelayPlayerId,
    pub session_name: String,
    pub players: Vec<PlayerInfo>,
    pub ticks_per_turn: u32,
}

/// A TCP connection to a relay server, before joining a session.
///
/// In this state you can call `list_sessions()` and `create_session()` to
/// discover or create sessions. Once you have a `SessionId`, call
/// `join_session()` to complete the handshake and get a `NetClient`.
pub struct RelayConnection {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl RelayConnection {
    /// Establish a TCP connection to the relay. Does not send any messages.
    pub fn connect(addr: &str) -> Result<Self, String> {
        let stream = TcpStream::connect(addr).map_err(|e| format!("connect failed: {e}"))?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();

        let reader_stream = stream
            .try_clone()
            .map_err(|e| format!("clone failed: {e}"))?;
        let reader = BufReader::new(reader_stream);

        Ok(Self { stream, reader })
    }

    /// Request a list of active sessions on the relay.
    pub fn list_sessions(&mut self) -> Result<Vec<SessionInfo>, String> {
        send_client_msg(&self.stream, &ClientMessage::ListSessions)?;
        let response = recv_server_msg(&mut self.reader)?;
        match response {
            ServerMessage::SessionList { sessions } => Ok(sessions),
            other => Err(format!("expected SessionList, got {other:?}")),
        }
    }

    /// Create a new session on the relay. Returns the assigned `SessionId`.
    pub fn create_session(
        &mut self,
        session_name: &str,
        password: Option<String>,
        ticks_per_turn: u32,
        max_players: u32,
    ) -> Result<SessionId, String> {
        send_client_msg(
            &self.stream,
            &ClientMessage::CreateSession {
                session_name: session_name.into(),
                password,
                ticks_per_turn,
                max_players,
            },
        )?;
        let response = recv_server_msg(&mut self.reader)?;
        match response {
            ServerMessage::SessionCreated { session_id } => Ok(session_id),
            ServerMessage::Rejected { reason } => Err(format!("rejected: {reason}")),
            other => Err(format!("expected SessionCreated, got {other:?}")),
        }
    }

    /// Join a session by sending `Hello` and completing the handshake.
    /// Consumes this `RelayConnection` and returns a `NetClient`.
    pub fn join_session(
        mut self,
        session_id: SessionId,
        player_name: &str,
        sim_version_hash: u64,
        config_hash: u64,
        password: Option<String>,
    ) -> Result<(NetClient, WelcomeInfo), String> {
        let hello = ClientMessage::Hello {
            protocol_version: 1,
            session_id,
            player_name: player_name.into(),
            sim_version_hash,
            config_hash,
            session_password: password,
        };
        send_client_msg(&self.stream, &hello)?;

        let response = recv_server_msg(&mut self.reader)?;
        let welcome_info = match response {
            ServerMessage::Welcome {
                player_id,
                session_name,
                players,
                ticks_per_turn,
            } => WelcomeInfo {
                player_id,
                session_name,
                players,
                ticks_per_turn,
            },
            ServerMessage::Rejected { reason } => {
                return Err(format!("rejected: {reason}"));
            }
            other => {
                return Err(format!("unexpected response: {other:?}"));
            }
        };

        // Clear read timeout for the long-lived reader loop.
        if let Ok(inner) = self.reader.get_ref().try_clone() {
            inner.set_read_timeout(None).ok();
        }

        let writer = BufWriter::new(self.stream);

        // Spawn reader thread.
        let (tx, rx) = mpsc::channel();
        let player_id = welcome_info.player_id;
        let reader_thread = thread::spawn(move || {
            reader_loop(self.reader, tx);
        });

        Ok((
            NetClient {
                writer,
                inbox: rx,
                _reader_thread: Some(reader_thread),
                player_id,
                next_sequence: 0,
            },
            welcome_info,
        ))
    }
}

/// TCP client for relay communication, after joining a session.
pub struct NetClient {
    writer: BufWriter<TcpStream>,
    inbox: Receiver<ServerMessage>,
    _reader_thread: Option<JoinHandle<()>>,
    pub player_id: RelayPlayerId,
    next_sequence: u64,
}

impl NetClient {
    /// Convenience: connect to an embedded relay (or any single-session relay)
    /// by combining `RelayConnection::connect` + `join_session` with the
    /// well-known `SessionId(0)`.
    pub fn connect(
        addr: &str,
        player_name: &str,
        sim_version_hash: u64,
        config_hash: u64,
        password: Option<String>,
    ) -> Result<(Self, WelcomeInfo), String> {
        let conn = RelayConnection::connect(addr)?;
        conn.join_session(
            SessionId(0),
            player_name,
            sim_version_hash,
            config_hash,
            password,
        )
    }

    /// Send a sim command (opaque payload bytes) to the relay.
    pub fn send_command(&mut self, payload: &[u8]) -> Result<(), String> {
        let seq = ActionSequence(self.next_sequence);
        self.next_sequence += 1;
        let msg = ClientMessage::Command {
            sequence: seq,
            payload: payload.to_vec(),
        };
        send_msg(&mut self.writer, &msg).map_err(|e| format!("send Command failed: {e}"))
    }

    /// Send StartGame (host only).
    pub fn send_start_game(&mut self, seed: i64, config_json: &str) -> Result<(), String> {
        let msg = ClientMessage::StartGame {
            seed,
            config_json: config_json.into(),
        };
        send_msg(&mut self.writer, &msg).map_err(|e| format!("send StartGame failed: {e}"))
    }

    /// Send a state checksum for desync detection.
    pub fn send_checksum(&mut self, tick: u64, hash: u64) -> Result<(), String> {
        let msg = ClientMessage::Checksum { tick, hash };
        send_msg(&mut self.writer, &msg).map_err(|e| format!("send Checksum failed: {e}"))
    }

    /// Send a snapshot response (serialized sim state) back to the relay.
    /// Used during mid-game join: the relay sends SnapshotRequest to the host,
    /// and the host replies with SnapshotResponse containing the full sim state.
    pub fn send_snapshot_response(&mut self, data: &[u8]) -> Result<(), String> {
        let msg = ClientMessage::SnapshotResponse {
            data: data.to_vec(),
        };
        send_msg(&mut self.writer, &msg).map_err(|e| format!("send SnapshotResponse failed: {e}"))
    }

    /// Send a chat message.
    pub fn send_chat(&mut self, text: &str) -> Result<(), String> {
        let msg = ClientMessage::Chat { text: text.into() };
        send_msg(&mut self.writer, &msg).map_err(|e| format!("send Chat failed: {e}"))
    }

    /// Request the relay to pause turn flushing.
    pub fn send_pause(&mut self) -> Result<(), String> {
        send_msg(&mut self.writer, &ClientMessage::RequestPause)
            .map_err(|e| format!("send RequestPause failed: {e}"))
    }

    /// Request the relay to resume turn flushing.
    pub fn send_resume(&mut self) -> Result<(), String> {
        send_msg(&mut self.writer, &ClientMessage::RequestResume)
            .map_err(|e| format!("send RequestResume failed: {e}"))
    }

    /// Request the relay to change the turn cadence.
    pub fn send_set_speed(&mut self, ticks_per_turn: u32) -> Result<(), String> {
        let msg = ClientMessage::SetSpeed { ticks_per_turn };
        send_msg(&mut self.writer, &msg).map_err(|e| format!("send SetSpeed failed: {e}"))
    }

    /// Send Goodbye and close the connection.
    pub fn disconnect(&mut self) {
        let _ = send_msg(&mut self.writer, &ClientMessage::Goodbye);
    }

    /// Drain all queued server messages (non-blocking).
    pub fn poll(&self) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        while let Ok(msg) = self.inbox.try_recv() {
            messages.push(msg);
        }
        messages
    }
}

/// Serialize a `ClientMessage` to JSON and write with length-delimited framing.
fn send_msg(writer: &mut BufWriter<TcpStream>, msg: &ClientMessage) -> Result<(), String> {
    let json = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    write_message(writer, &json).map_err(|e| e.to_string())
}

/// Send a `ClientMessage` directly on a `TcpStream` (used during pre-handshake).
fn send_client_msg(stream: &TcpStream, msg: &ClientMessage) -> Result<(), String> {
    use std::io::Write;
    let json = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(stream);
    write_message(&mut writer, &json).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

/// Read and deserialize a `ServerMessage` from a buffered reader.
fn recv_server_msg(reader: &mut BufReader<TcpStream>) -> Result<ServerMessage, String> {
    let bytes = read_message(reader).map_err(|e| format!("read failed: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse failed: {e}"))
}

/// Reader thread: read framed messages in a loop, push to channel.
fn reader_loop(mut reader: BufReader<TcpStream>, tx: mpsc::Sender<ServerMessage>) {
    while let Ok(bytes) = read_message(&mut reader) {
        match serde_json::from_slice::<ServerMessage>(&bytes) {
            Ok(msg) => {
                if tx.send(msg).is_err() {
                    break; // Main thread dropped the receiver
                }
            }
            Err(_) => break, // Malformed message
        }
    }
}
