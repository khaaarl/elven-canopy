// TCP server and main event loop for the relay coordinator.
//
// Architecture: thread-per-reader with a central `mpsc` channel.
//
// - **Listener thread** (`TcpListener::accept()` loop): accepts new TCP
//   connections and sends `InternalEvent::NewConnection` to the main thread.
// - **Reader threads** (one per client): call `framing::read_message()` in a
//   loop, deserialize `ClientMessage`, and send `InternalEvent::MessageFrom`
//   to the main thread. On error/EOF, send `InternalEvent::Disconnected`.
// - **Main thread**: owns the `Session`, receives events from the channel,
//   and dispatches them. Uses `recv_timeout` with the turn cadence as the
//   timeout — when the timeout fires (no events waiting), it flushes the
//   current turn. This gives us a simple timer without a separate timer thread.
//
// The main thread is the only writer to client TCP streams (via
// `Session::broadcast`/`send_to`). Reader threads only read from streams.
// This avoids concurrent read/write on the same `TcpStream`, which is safe
// on most platforms but fragile.
//
// Shutdown: the main thread checks a `keep_running` flag (set to false by
// `stop_relay`) and breaks out of the event loop.

use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use elven_canopy_protocol::framing::{read_message, write_message};
use elven_canopy_protocol::message::{ClientMessage, ServerMessage, TurnCommand};
use elven_canopy_protocol::types::RelayPlayerId;

use crate::session::Session;

/// Events sent from listener/reader threads to the main thread.
enum InternalEvent {
    NewConnection {
        stream: TcpStream,
    },
    MessageFrom {
        player_id: RelayPlayerId,
        message: ClientMessage,
    },
    Disconnected {
        player_id: RelayPlayerId,
    },
}

/// Handle returned by `start_relay` to control the running server.
pub struct RelayHandle {
    keep_running: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RelayHandle {
    /// Signal the relay to stop and wait for it to shut down.
    pub fn stop(self) {
        self.keep_running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread {
            let _ = handle.join();
        }
    }
}

/// Configuration for starting a relay server.
pub struct RelayConfig {
    pub port: u16,
    pub session_name: String,
    pub password: Option<String>,
    pub ticks_per_turn: u32,
    pub max_players: u32,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            port: 7878,
            session_name: "elven-canopy-session".into(),
            password: None,
            ticks_per_turn: 50,
            max_players: 4,
        }
    }
}

/// Start the relay server on a background thread. Returns a handle for
/// stopping it and the actual bound address (useful when port 0 is used
/// to let the OS pick a free port).
pub fn start_relay(config: RelayConfig) -> std::io::Result<(RelayHandle, std::net::SocketAddr)> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", config.port))?;
    let addr = listener.local_addr()?;
    let keep_running = Arc::new(AtomicBool::new(true));
    let keep_running_clone = keep_running.clone();

    let thread = thread::spawn(move || {
        run_relay(listener, config, keep_running_clone);
    });

    Ok((
        RelayHandle {
            keep_running,
            thread: Some(thread),
        },
        addr,
    ))
}

/// Main relay loop. Runs until `keep_running` is set to false.
fn run_relay(listener: TcpListener, config: RelayConfig, keep_running: Arc<AtomicBool>) {
    let mut session = Session::new(
        config.session_name,
        config.password,
        config.ticks_per_turn,
        config.max_players,
    );

    let (tx, rx): (Sender<InternalEvent>, Receiver<InternalEvent>) = mpsc::channel();

    // Set the listener to non-blocking so the accept thread can check
    // keep_running periodically.
    listener.set_nonblocking(true).ok();

    // Listener thread: accepts new connections.
    let keep_running_listener = keep_running.clone();
    let tx_listener = tx.clone();
    thread::spawn(move || {
        while keep_running_listener.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    stream.set_nonblocking(false).ok();
                    let _ = tx_listener.send(InternalEvent::NewConnection { stream });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
    });

    let turn_duration = Duration::from_millis(u64::from(config.ticks_per_turn));

    // Main event loop.
    while keep_running.load(Ordering::SeqCst) {
        match rx.recv_timeout(turn_duration) {
            Ok(event) => {
                handle_event(&mut session, event, &tx, &keep_running);
                // Drain any additional events that arrived during handling.
                while let Ok(event) = rx.try_recv() {
                    handle_event(&mut session, event, &tx, &keep_running);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Turn timer fired — flush even if no commands arrived.
                if !session.paused && session.player_count() > 0 {
                    session.flush_turn();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Dispatch a single event to the session.
fn handle_event(
    session: &mut Session,
    event: InternalEvent,
    tx: &Sender<InternalEvent>,
    keep_running: &Arc<AtomicBool>,
) {
    match event {
        InternalEvent::NewConnection { stream } => {
            handle_new_connection(session, stream, tx, keep_running);
        }
        InternalEvent::MessageFrom { player_id, message } => {
            handle_message(session, player_id, message);
        }
        InternalEvent::Disconnected { player_id } => {
            session.remove_player(player_id);
        }
    }
}

/// Handle a new TCP connection: read the Hello handshake, add the player to
/// the session, and spawn a reader thread.
fn handle_new_connection(
    session: &mut Session,
    stream: TcpStream,
    tx: &Sender<InternalEvent>,
    keep_running: &Arc<AtomicBool>,
) {
    // Set a read timeout so the handshake doesn't block forever.
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

    // Read the Hello message.
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });

    let hello_bytes = match read_message(&mut reader) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };

    let hello: ClientMessage = match serde_json::from_slice(&hello_bytes) {
        Ok(msg) => msg,
        Err(_) => return,
    };

    match hello {
        ClientMessage::Hello {
            protocol_version: _,
            player_name,
            sim_version_hash,
            config_hash,
            session_password,
        } => {
            // Try to clone the stream for the session's write half.
            let write_stream = match stream.try_clone() {
                Ok(s) => s,
                Err(_) => return,
            };

            match session.add_player(
                player_name,
                sim_version_hash,
                config_hash,
                session_password,
                write_stream,
            ) {
                Ok(player_id) => {
                    // Clear read timeout for the long-lived reader loop.
                    stream.set_read_timeout(None).ok();

                    // Spawn a reader thread for this client.
                    let tx_reader = tx.clone();
                    let keep_running_reader = keep_running.clone();
                    thread::spawn(move || {
                        reader_loop(reader, player_id, tx_reader, keep_running_reader);
                    });
                }
                Err(reason) => {
                    // Send Rejected and close the connection.
                    let rejected = ServerMessage::Rejected { reason };
                    if let Ok(json) = serde_json::to_vec(&rejected) {
                        let mut writer = std::io::BufWriter::new(stream);
                        let _ = write_message(&mut writer, &json);
                    }
                }
            }
        }
        _ => {
            // Expected Hello as first message — drop the connection.
        }
    }
}

/// Reader loop for a single client. Runs in its own thread.
fn reader_loop(
    mut reader: BufReader<TcpStream>,
    player_id: RelayPlayerId,
    tx: Sender<InternalEvent>,
    keep_running: Arc<AtomicBool>,
) {
    while keep_running.load(Ordering::SeqCst) {
        match read_message(&mut reader) {
            Ok(bytes) => match serde_json::from_slice::<ClientMessage>(&bytes) {
                Ok(ClientMessage::Goodbye) => {
                    let _ = tx.send(InternalEvent::Disconnected { player_id });
                    break;
                }
                Ok(message) => {
                    let _ = tx.send(InternalEvent::MessageFrom { player_id, message });
                }
                Err(_) => {
                    // Malformed message — disconnect.
                    let _ = tx.send(InternalEvent::Disconnected { player_id });
                    break;
                }
            },
            Err(_) => {
                // Read error or EOF — disconnect.
                let _ = tx.send(InternalEvent::Disconnected { player_id });
                break;
            }
        }
    }
}

/// Handle a client message that isn't Hello or Goodbye (those are handled
/// during connection setup and in the reader loop respectively).
fn handle_message(session: &mut Session, player_id: RelayPlayerId, message: ClientMessage) {
    match message {
        ClientMessage::Command { sequence, payload } => {
            session.enqueue_command(TurnCommand {
                player_id,
                sequence,
                payload,
            });
        }
        ClientMessage::Checksum { tick, hash } => {
            session.record_checksum(player_id, tick, hash);
        }
        ClientMessage::SetSpeed { ticks_per_turn } => {
            session.set_speed(player_id, ticks_per_turn);
        }
        ClientMessage::RequestPause => {
            session.request_pause(player_id);
        }
        ClientMessage::RequestResume => {
            session.request_resume(player_id);
        }
        ClientMessage::Chat { text } => {
            session.chat(player_id, text);
        }
        ClientMessage::StartGame { seed, config_json } => {
            session.handle_start_game(player_id, seed, config_json);
        }
        ClientMessage::SnapshotResponse { .. } => {
            // Mid-game join snapshot handling — not yet implemented.
        }
        ClientMessage::Hello { .. } | ClientMessage::Goodbye => {
            // Hello is handled during connection setup, Goodbye in the reader loop.
        }
    }
}
