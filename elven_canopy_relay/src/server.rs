// TCP server and main event loop for the relay coordinator.
//
// Architecture: thread-per-reader with a central `mpsc` channel.
//
// - **Listener thread** (`TcpListener::accept()` loop): accepts new TCP
//   connections and spawns a **handshake thread** for each.
// - **Handshake threads** (one per pending connection): handle the pre-handshake
//   phase (`ListSessions` / `CreateSession`) and then the `Hello` handshake.
//   Pre-handshake messages that need relay state (list/create) are forwarded to
//   the main thread via events. The `Hello` is also forwarded so the main thread
//   can add the player to the session.
// - **Reader threads** (one per joined client): call `framing::read_message()`
//   in a loop, deserialize `ClientMessage`, and send `InternalEvent::MessageFrom`
//   to the main thread. On error/EOF, send `InternalEvent::Disconnected`.
// - **Main thread**: owns the `RelayState` (all sessions), receives events from
//   the channel, and dispatches them. Uses `recv_timeout` with a short cadence —
//   when the timeout fires, it flushes turns on all active sessions.
//
// The main thread is the only writer to client TCP streams owned by sessions
// (via `Session::broadcast`/`send_to`). Handshake threads write directly to
// their own streams (before the session owns them). Reader threads only read.
//
// **Multi-session support:** A relay can host multiple simultaneous game
// sessions. Embedded (player-hosted) relays set `embedded = true`, which
// restricts session creation to the well-known `SessionId(0)`.
//
// Shutdown: the main thread checks a `keep_running` flag (set to false by
// `stop_relay`) and breaks out of the event loop.

use std::collections::BTreeMap;
use std::io::{BufReader, BufWriter};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use elven_canopy_protocol::framing::{read_message, write_message};
use elven_canopy_protocol::message::{ClientMessage, ServerMessage, SessionInfo, TurnCommand};
use elven_canopy_protocol::types::{RelayPlayerId, SessionId};

use crate::session::Session;

/// Events sent from listener/handshake/reader threads to the main thread.
enum InternalEvent {
    /// A client wants a list of sessions.
    RequestListSessions {
        reply_tx: mpsc::Sender<ServerMessage>,
    },
    /// A client wants to create a new session.
    RequestCreateSession {
        session_name: String,
        password: Option<String>,
        ticks_per_turn: u32,
        max_players: u32,
        reply_tx: mpsc::Sender<ServerMessage>,
    },
    /// A client has sent Hello and wants to join a session.
    RequestJoin {
        session_id: SessionId,
        player_name: String,
        sim_version_hash: u64,
        config_hash: u64,
        session_password: Option<String>,
        llm_capable: bool,
        stream: TcpStream,
        reader: BufReader<TcpStream>,
        reply_tx: mpsc::Sender<Result<RelayPlayerId, String>>,
    },
    /// A joined client sent a gameplay message.
    MessageFrom {
        session_id: SessionId,
        player_id: RelayPlayerId,
        message: ClientMessage,
    },
    /// A joined client disconnected.
    Disconnected {
        session_id: SessionId,
        player_id: RelayPlayerId,
    },
    /// A handshake thread created a session but the client disconnected before
    /// joining. The session may be empty and should be cleaned up.
    CleanupOrphanedSession { session_id: SessionId },
}

/// Handle returned by `start_relay` to control the running server.
pub struct RelayHandle {
    keep_running: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RelayHandle {
    /// Signal the relay to stop and wait for it to shut down.
    pub fn stop(mut self) {
        self.shutdown_inner();
    }

    /// Shared shutdown logic: signal the flag and join the thread.
    fn shutdown_inner(&mut self) {
        self.keep_running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RelayHandle {
    fn drop(&mut self) {
        self.shutdown_inner();
    }
}

/// Configuration for starting a relay server.
pub struct RelayConfig {
    pub port: u16,
    /// Bind address. Embedded relays use `127.0.0.1` (localhost only);
    /// dedicated relays default to `0.0.0.0` (all interfaces) so remote
    /// clients can connect.
    pub bind_address: String,
    /// If true, this is an embedded relay (player-hosted). Only `SessionId(0)`
    /// can be created, and only one session is allowed. Joiners skip session
    /// discovery and go straight to `Hello` with `SessionId(0)`.
    pub embedded: bool,
    /// Default turn cadence used for the event loop timeout. Individual sessions
    /// may have different `ticks_per_turn` values.
    pub turn_cadence_ms: u64,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            port: 7878,
            bind_address: "0.0.0.0".into(),
            embedded: false,
            turn_cadence_ms: 50,
        }
    }
}

/// State for all sessions managed by this relay.
struct RelayState {
    sessions: BTreeMap<SessionId, Session>,
    next_session_id: u64,
    embedded: bool,
}

impl RelayState {
    fn new(embedded: bool) -> Self {
        Self {
            sessions: BTreeMap::new(),
            // Dedicated relays start IDs at 1 to leave 0 for embedded use.
            next_session_id: 1,
            embedded,
        }
    }

    /// Maximum number of sessions a dedicated relay will host.
    const MAX_SESSIONS: usize = 100;

    /// Create a new session. Returns the assigned `SessionId` on success.
    fn create_session(
        &mut self,
        session_name: String,
        password: Option<String>,
        ticks_per_turn: u32,
        max_players: u32,
    ) -> Result<SessionId, String> {
        // Input validation.
        if session_name.is_empty() {
            return Err("session name must not be empty".into());
        }
        if session_name.len() > 128 {
            return Err("session name too long (max 128 bytes)".into());
        }
        if max_players == 0 {
            return Err("max_players must be at least 1".into());
        }
        if max_players > 64 {
            return Err("max_players cannot exceed 64".into());
        }
        if ticks_per_turn == 0 {
            return Err("ticks_per_turn must be at least 1".into());
        }
        if !self.embedded && self.sessions.len() >= Self::MAX_SESSIONS {
            return Err("relay has reached the maximum number of sessions".into());
        }

        if self.embedded {
            let sid = SessionId(0);
            if self.sessions.contains_key(&sid) {
                return Err("embedded relay already has a session".into());
            }
            self.sessions.insert(
                sid,
                Session::new(session_name, password, ticks_per_turn, max_players),
            );
            return Ok(sid);
        }

        let sid = SessionId(self.next_session_id);
        self.next_session_id += 1;
        self.sessions.insert(
            sid,
            Session::new(session_name, password, ticks_per_turn, max_players),
        );
        Ok(sid)
    }

    /// Build a list of session summaries for `SessionList`.
    fn session_list(&self) -> Vec<SessionInfo> {
        self.sessions
            .iter()
            .map(|(sid, session)| SessionInfo {
                session_id: *sid,
                name: session.name.clone(),
                player_count: session.player_count() as u32,
                max_players: session.max_players(),
                has_password: session.has_password(),
                game_started: session.is_game_started(),
            })
            .collect()
    }

    /// Remove a session if it has no players remaining.
    fn cleanup_empty_session(&mut self, session_id: SessionId) {
        if let Some(session) = self.sessions.get(&session_id)
            && session.player_count() == 0
        {
            self.sessions.remove(&session_id);
        }
    }
}

/// Start the relay server on a background thread. Returns a handle for
/// stopping it and the actual bound address (useful when port 0 is used
/// to let the OS pick a free port).
pub fn start_relay(config: RelayConfig) -> std::io::Result<(RelayHandle, std::net::SocketAddr)> {
    let listener = TcpListener::bind(format!("{}:{}", config.bind_address, config.port))?;
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
    let mut state = RelayState::new(config.embedded);

    let (tx, rx): (Sender<InternalEvent>, Receiver<InternalEvent>) = mpsc::channel();

    // Set the listener to non-blocking so the accept thread can check
    // keep_running periodically.
    listener.set_nonblocking(true).ok();

    // Listener thread: accepts new connections and spawns handshake threads.
    let keep_running_listener = keep_running.clone();
    let tx_listener = tx.clone();
    thread::spawn(move || {
        while keep_running_listener.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    stream.set_nodelay(true).ok();
                    stream.set_nonblocking(false).ok();
                    let tx_handshake = tx_listener.clone();
                    let kr = keep_running_listener.clone();
                    thread::spawn(move || {
                        handshake_thread(stream, tx_handshake, kr);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
    });

    let turn_duration = Duration::from_millis(config.turn_cadence_ms);
    let mut last_flush = std::time::Instant::now();

    // Main event loop.
    while keep_running.load(Ordering::SeqCst) {
        // Calculate remaining time until next turn flush.
        let elapsed = last_flush.elapsed();
        let wait = turn_duration.saturating_sub(elapsed);

        match rx.recv_timeout(wait) {
            Ok(event) => {
                handle_event(&mut state, event, &tx, &keep_running);
                // Drain any additional events that arrived during handling.
                while let Ok(event) = rx.try_recv() {
                    handle_event(&mut state, event, &tx, &keep_running);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Turn timer fired.
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Flush turns only when the cadence interval has elapsed.
        if last_flush.elapsed() >= turn_duration {
            for session in state.sessions.values_mut() {
                if !session.paused && session.player_count() > 0 {
                    session.flush_turn();
                }
            }
            last_flush = std::time::Instant::now();
        }
    }
}

/// Handshake thread: runs the pre-handshake loop (ListSessions, CreateSession)
/// and then forwards the Hello to the main thread for session joining.
fn handshake_thread(stream: TcpStream, tx: Sender<InternalEvent>, keep_running: Arc<AtomicBool>) {
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });

    // Track all sessions this client created. If the client disconnects
    // without joining, we send cleanup events so the main thread can remove
    // any that are still empty.
    let mut created_sessions: Vec<SessionId> = Vec::new();

    loop {
        if !keep_running.load(Ordering::SeqCst) {
            break;
        }

        let bytes = match read_message(&mut reader) {
            Ok(b) => b,
            Err(_) => break,
        };
        let msg: ClientMessage = match serde_json::from_slice(&bytes) {
            Ok(m) => m,
            Err(_) => break,
        };

        match msg {
            ClientMessage::ListSessions => {
                let (reply_tx, reply_rx) = mpsc::channel();
                if tx
                    .send(InternalEvent::RequestListSessions { reply_tx })
                    .is_err()
                {
                    break;
                }
                if let Ok(response) = reply_rx.recv() {
                    send_server_msg(&stream, &response);
                }
            }
            ClientMessage::CreateSession {
                session_name,
                password,
                ticks_per_turn,
                max_players,
            } => {
                let (reply_tx, reply_rx) = mpsc::channel();
                if tx
                    .send(InternalEvent::RequestCreateSession {
                        session_name,
                        password,
                        ticks_per_turn,
                        max_players,
                        reply_tx,
                    })
                    .is_err()
                {
                    break;
                }
                if let Ok(response) = reply_rx.recv() {
                    if let ServerMessage::SessionCreated { session_id } = &response {
                        // Track all sessions this client created. On disconnect,
                        // all empty ones will be cleaned up.
                        created_sessions.push(*session_id);
                    }
                    send_server_msg(&stream, &response);
                }
            }
            ClientMessage::Hello {
                protocol_version: _,
                session_id,
                player_name,
                sim_version_hash,
                config_hash,
                session_password,
                llm_capable,
            } => {
                // Forward the join request to the main thread.
                let write_stream = match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => break,
                };

                let (reply_tx, reply_rx) = mpsc::channel();
                if tx
                    .send(InternalEvent::RequestJoin {
                        session_id,
                        player_name,
                        sim_version_hash,
                        config_hash,
                        session_password,
                        llm_capable,
                        stream: write_stream,
                        reader,
                        reply_tx,
                    })
                    .is_err()
                {
                    break;
                }

                // Wait for the main thread to process the join.
                match reply_rx.recv() {
                    Ok(Ok(_player_id)) => {
                        // Success — the main thread added us to the session and
                        // sent Welcome. The main thread spawns the reader loop
                        // using the `reader` we gave it. Remove the joined
                        // session from cleanup tracking (it's no longer orphaned).
                        created_sessions.retain(|s| *s != session_id);
                    }
                    Ok(Err(reason)) => {
                        // Rejected — main thread sent Rejected. We're done.
                        send_server_msg(&stream, &ServerMessage::Rejected { reason });
                    }
                    Err(_) => {
                        // Main thread dropped — shutting down.
                    }
                }
                // Hello terminates the pre-handshake phase regardless of outcome.
                break;
            }
            _ => {
                // Unexpected message during pre-handshake — drop connection.
                break;
            }
        }
    }

    // Clean up any sessions this client created but never joined. Each one
    // may be empty (no players) and should be removed.
    for session_id in created_sessions {
        let _ = tx.send(InternalEvent::CleanupOrphanedSession { session_id });
    }
}

/// Dispatch a single event.
fn handle_event(
    state: &mut RelayState,
    event: InternalEvent,
    tx: &Sender<InternalEvent>,
    keep_running: &Arc<AtomicBool>,
) {
    match event {
        InternalEvent::RequestListSessions { reply_tx } => {
            let list = state.session_list();
            let _ = reply_tx.send(ServerMessage::SessionList { sessions: list });
        }
        InternalEvent::RequestCreateSession {
            session_name,
            password,
            ticks_per_turn,
            max_players,
            reply_tx,
        } => {
            let response =
                match state.create_session(session_name, password, ticks_per_turn, max_players) {
                    Ok(sid) => ServerMessage::SessionCreated { session_id: sid },
                    Err(reason) => ServerMessage::Rejected { reason },
                };
            let _ = reply_tx.send(response);
        }
        InternalEvent::RequestJoin {
            session_id,
            player_name,
            sim_version_hash,
            config_hash,
            session_password,
            llm_capable,
            stream,
            reader,
            reply_tx,
        } => {
            let session = match state.sessions.get_mut(&session_id) {
                Some(s) => s,
                None => {
                    let _ = reply_tx.send(Err("session not found".into()));
                    return;
                }
            };

            match session.add_player(
                player_name,
                sim_version_hash,
                config_hash,
                session_password,
                llm_capable,
                stream,
            ) {
                Ok(player_id) => {
                    let _ = reply_tx.send(Ok(player_id));

                    // Clear the 30-second handshake read timeout before
                    // entering the long-lived reader loop. Set directly on
                    // the reader's inner stream (not a clone) to ensure the
                    // change takes effect on Windows where socket options
                    // may be per-handle.
                    reader.get_ref().set_read_timeout(None).ok();
                    let tx_reader = tx.clone();
                    let keep_running_reader = keep_running.clone();
                    thread::spawn(move || {
                        reader_loop(
                            reader,
                            session_id,
                            player_id,
                            tx_reader,
                            keep_running_reader,
                        );
                    });
                }
                Err(reason) => {
                    let _ = reply_tx.send(Err(reason));
                }
            }
        }
        InternalEvent::MessageFrom {
            session_id,
            player_id,
            message,
        } => {
            if let Some(session) = state.sessions.get_mut(&session_id) {
                handle_message(session, player_id, message);
            }
        }
        InternalEvent::Disconnected {
            session_id,
            player_id,
        } => {
            if let Some(session) = state.sessions.get_mut(&session_id) {
                session.remove_player(player_id);
            }
            state.cleanup_empty_session(session_id);
        }
        InternalEvent::CleanupOrphanedSession { session_id } => {
            state.cleanup_empty_session(session_id);
        }
    }
}

/// Send a `ServerMessage` directly on a TCP stream.
fn send_server_msg(stream: &TcpStream, msg: &ServerMessage) {
    use std::io::Write;
    if let Ok(json) = serde_json::to_vec(msg) {
        let mut writer = BufWriter::new(stream);
        if write_message(&mut writer, &json).is_ok() {
            let _ = writer.flush();
        }
    }
}

/// Reader loop for a single client. Runs in its own thread.
fn reader_loop(
    mut reader: BufReader<TcpStream>,
    session_id: SessionId,
    player_id: RelayPlayerId,
    tx: Sender<InternalEvent>,
    keep_running: Arc<AtomicBool>,
) {
    while keep_running.load(Ordering::SeqCst) {
        match read_message(&mut reader) {
            Ok(bytes) => match serde_json::from_slice::<ClientMessage>(&bytes) {
                Ok(ClientMessage::Goodbye) => {
                    let _ = tx.send(InternalEvent::Disconnected {
                        session_id,
                        player_id,
                    });
                    break;
                }
                Ok(message) => {
                    let _ = tx.send(InternalEvent::MessageFrom {
                        session_id,
                        player_id,
                        message,
                    });
                }
                Err(_) => {
                    let _ = tx.send(InternalEvent::Disconnected {
                        session_id,
                        player_id,
                    });
                    break;
                }
            },
            Err(_) => {
                let _ = tx.send(InternalEvent::Disconnected {
                    session_id,
                    player_id,
                });
                break;
            }
        }
    }
}

/// Handle a client message from a reader thread.
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
        ClientMessage::StartGame {
            seed,
            config_json,
            starting_tick,
        } => {
            session.handle_start_game(player_id, seed, config_json, starting_tick);
        }
        ClientMessage::ResumeSession { starting_tick } => {
            session.handle_resume_session(player_id, starting_tick);
        }
        ClientMessage::SnapshotResponse { data } => {
            session.handle_snapshot_response(player_id, data);
        }
        ClientMessage::LlmRequest {
            request_id,
            payload,
        } => {
            session.handle_llm_request(player_id, request_id, payload);
        }
        ClientMessage::LlmResponse {
            request_id,
            payload,
        } => {
            session.handle_llm_response(player_id, request_id, payload);
        }
        ClientMessage::LlmCapabilityChanged { llm_capable } => {
            session.handle_llm_capability_changed(player_id, llm_capable);
        }
        ClientMessage::Hello { .. }
        | ClientMessage::Goodbye
        | ClientMessage::ListSessions
        | ClientMessage::CreateSession { .. } => {
            // These are handled during connection setup / reader loop.
        }
    }
}
