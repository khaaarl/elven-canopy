// TCP client for connecting to the multiplayer relay.
//
// Provides a non-blocking interface for the main thread to communicate
// with the relay server. Architecture:
// - `connect()` performs TCP connect + Hello handshake on the calling thread,
//   then spawns a background reader thread.
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
use elven_canopy_protocol::message::{ClientMessage, PlayerInfo, ServerMessage};
use elven_canopy_protocol::types::{ActionSequence, RelayPlayerId};

/// Information returned by a successful `connect()` handshake.
pub struct WelcomeInfo {
    pub player_id: RelayPlayerId,
    pub session_name: String,
    pub _players: Vec<PlayerInfo>,
    pub ticks_per_turn: u32,
}

/// TCP client for relay communication.
pub struct NetClient {
    writer: BufWriter<TcpStream>,
    inbox: Receiver<ServerMessage>,
    _reader_thread: Option<JoinHandle<()>>,
    pub _player_id: RelayPlayerId,
    next_sequence: u64,
}

impl NetClient {
    /// Connect to a relay server, perform the Hello handshake, and spawn a
    /// reader thread. Returns the client and welcome info on success.
    pub fn connect(
        addr: &str,
        player_name: &str,
        sim_version_hash: u64,
        config_hash: u64,
        password: Option<String>,
    ) -> Result<(Self, WelcomeInfo), String> {
        // TCP connect.
        let stream = TcpStream::connect(addr).map_err(|e| format!("connect failed: {e}"))?;

        // Set a read timeout for the handshake.
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();

        let reader_stream = stream
            .try_clone()
            .map_err(|e| format!("clone failed: {e}"))?;
        let mut writer = BufWriter::new(stream);

        // Send Hello.
        let hello = ClientMessage::Hello {
            protocol_version: 1,
            player_name: player_name.into(),
            sim_version_hash,
            config_hash,
            session_password: password,
        };
        send_msg(&mut writer, &hello).map_err(|e| format!("send Hello failed: {e}"))?;

        // Read Welcome or Rejected.
        let mut reader = BufReader::new(reader_stream);
        let response_bytes =
            read_message(&mut reader).map_err(|e| format!("read Welcome failed: {e}"))?;
        let response: ServerMessage = serde_json::from_slice(&response_bytes)
            .map_err(|e| format!("parse Welcome failed: {e}"))?;

        let welcome_info = match response {
            ServerMessage::Welcome {
                player_id,
                session_name,
                players,
                ticks_per_turn,
            } => WelcomeInfo {
                player_id,
                session_name,
                _players: players,
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
        if let Ok(inner) = reader.get_ref().try_clone() {
            inner.set_read_timeout(None).ok();
        }

        // Spawn reader thread.
        let (tx, rx) = mpsc::channel();
        let player_id = welcome_info.player_id;
        let reader_thread = thread::spawn(move || {
            reader_loop(reader, tx);
        });

        Ok((
            Self {
                writer,
                inbox: rx,
                _reader_thread: Some(reader_thread),
                _player_id: player_id,
                next_sequence: 0,
            },
            welcome_info,
        ))
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

    /// Send a chat message.
    pub fn send_chat(&mut self, text: &str) -> Result<(), String> {
        let msg = ClientMessage::Chat { text: text.into() };
        send_msg(&mut self.writer, &msg).map_err(|e| format!("send Chat failed: {e}"))
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
