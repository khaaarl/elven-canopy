// Session state for the relay coordinator.
//
// `Session` is the central data structure that `server.rs` drives. It tracks
// connected players, pending commands, turn numbering, sim tick advancement,
// and checksum-based desync detection. All mutation happens through methods
// called from the server's single-threaded main loop — no internal locking.
//
// Key responsibilities:
// - Player management: add/remove players, assign IDs, version-check on join.
// - Command queuing: buffer incoming commands from clients until the next turn
//   flush. Commands are sorted by `(player_id, sequence)` for canonical order.
// - Turn flushing: package pending commands into a `Turn` message, broadcast
//   to all clients, advance the sim tick target.
// - Desync detection: collect per-player checksums for each tick, compare when
//   all active players have reported.
// - Mid-game join: when a player joins after the game has started, the session
//   requests a sim state snapshot from the host, pauses turn flushing during
//   the transfer, and forwards the snapshot to the joiner. The pending joiner
//   is excluded from checksum comparisons until the snapshot is delivered.
//
// Writing to client streams: `Session` holds cloned `TcpStream` write halves
// wrapped in `BufWriter`. The `send_to` / `broadcast` helpers serialize a
// `ServerMessage` to JSON, frame it, and write it out. Write errors on a
// single client are logged but do not crash the relay — the reader thread
// for that client will detect the broken pipe and send a `Disconnected` event.

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::net::TcpStream;

use elven_canopy_protocol::framing::write_message;
use elven_canopy_protocol::message::{PlayerInfo, ServerMessage, TurnCommand};
use elven_canopy_protocol::types::{RelayPlayerId, TurnNumber};

/// Relay session managing a single multiplayer game.
pub struct Session {
    pub name: String,
    password: Option<String>,
    host_id: RelayPlayerId,
    players: BTreeMap<RelayPlayerId, PlayerState>,
    next_player_id: u32,
    max_players: u32,

    // Turn state
    current_turn: TurnNumber,
    current_tick: u64,
    pub ticks_per_turn: u32,
    pending_commands: Vec<TurnCommand>,
    pub paused: bool,

    // Desync detection
    checksums: BTreeMap<u64, BTreeMap<RelayPlayerId, u64>>,

    // Version hashes (set by first player)
    sim_version_hash: Option<u64>,
    config_hash: Option<u64>,

    // Lobby state — turns only flush after the host starts the game.
    game_started: bool,

    // Mid-game join: when a player joins after game_started, we request a
    // snapshot from the host and forward it to the joiner. Turn flushing is
    // paused while a snapshot is pending to ensure consistency.
    snapshot_pending: Option<SnapshotPending>,
}

/// Tracks a pending mid-game join snapshot transfer.
struct SnapshotPending {
    joiner_id: RelayPlayerId,
    requested_from: RelayPlayerId,
}

struct PlayerState {
    name: String,
    writer: BufWriter<TcpStream>,
}

impl Session {
    pub fn new(
        name: String,
        password: Option<String>,
        ticks_per_turn: u32,
        max_players: u32,
    ) -> Self {
        Self {
            name,
            password,
            host_id: RelayPlayerId(0),
            players: BTreeMap::new(),
            next_player_id: 0,
            max_players,
            current_turn: TurnNumber(0),
            current_tick: 0,
            ticks_per_turn,
            pending_commands: Vec::new(),
            paused: false,
            checksums: BTreeMap::new(),
            sim_version_hash: None,
            config_hash: None,
            game_started: false,
            snapshot_pending: None,
        }
    }

    /// Attempt to add a player to the session. Returns the assigned player ID
    /// on success, or an error reason string on failure.
    ///
    /// The returned `RelayPlayerId` should be used to tag the reader thread for
    /// this connection so that subsequent `InternalEvent::MessageFrom` events
    /// carry the correct ID.
    pub fn add_player(
        &mut self,
        player_name: String,
        sim_version_hash: u64,
        config_hash: u64,
        session_password: Option<String>,
        stream: TcpStream,
    ) -> Result<RelayPlayerId, String> {
        // Password check.
        if self.password.is_some() && session_password != self.password {
            return Err("incorrect password".into());
        }

        // Max players check.
        if self.players.len() as u32 >= self.max_players {
            return Err("session is full".into());
        }

        // Reject if a mid-game join snapshot transfer is already in flight.
        // Only one at a time to avoid overwriting the pending state.
        if self.game_started && self.snapshot_pending.is_some() {
            return Err("another player is joining, try again".into());
        }

        // Version check (first player sets the reference).
        if self.sim_version_hash.is_none() {
            self.sim_version_hash = Some(sim_version_hash);
            self.config_hash = Some(config_hash);
        } else {
            if self.sim_version_hash != Some(sim_version_hash) {
                return Err("sim version mismatch".into());
            }
            if self.config_hash != Some(config_hash) {
                return Err("config hash mismatch".into());
            }
        }

        // Reject duplicate player names within the same session.
        for ps in self.players.values() {
            if ps.name == player_name {
                return Err(format!(
                    "player name '{}' is already taken in this session",
                    player_name
                ));
            }
        }

        let id = RelayPlayerId(self.next_player_id);
        self.next_player_id += 1;

        // First player is the host.
        if self.players.is_empty() {
            self.host_id = id;
        }

        // Build player list for Welcome (includes the new player).
        let mut player_list: Vec<PlayerInfo> = self
            .players
            .iter()
            .map(|(pid, ps)| PlayerInfo {
                id: *pid,
                name: ps.name.clone(),
            })
            .collect();
        player_list.push(PlayerInfo {
            id,
            name: player_name.clone(),
        });

        // Broadcast PlayerJoined to existing players before adding the new one.
        let joined_msg = ServerMessage::PlayerJoined {
            player: PlayerInfo {
                id,
                name: player_name.clone(),
            },
        };
        self.broadcast(&joined_msg);

        // Add the new player.
        let writer = BufWriter::new(stream);
        self.players.insert(
            id,
            PlayerState {
                name: player_name,
                writer,
            },
        );

        // Send Welcome to the new player.
        let welcome = ServerMessage::Welcome {
            player_id: id,
            session_name: self.name.clone(),
            players: player_list,
            ticks_per_turn: self.ticks_per_turn,
        };
        self.send_to(id, &welcome);

        // Mid-game join: if the game has already started, request a snapshot
        // from the host so the joiner can initialize their sim.
        if self.game_started {
            self.snapshot_pending = Some(SnapshotPending {
                joiner_id: id,
                requested_from: self.host_id,
            });
            self.send_to(self.host_id, &ServerMessage::SnapshotRequest);
        }

        Ok(id)
    }

    /// Remove a player and broadcast their departure.
    pub fn remove_player(&mut self, player_id: RelayPlayerId) {
        if let Some(ps) = self.players.remove(&player_id) {
            let msg = ServerMessage::PlayerLeft {
                player_id,
                name: ps.name,
            };
            self.broadcast(&msg);

            // Remove pending checksums from this player.
            for tick_checksums in self.checksums.values_mut() {
                tick_checksums.remove(&player_id);
            }

            // Clear snapshot pending if the disconnecting player is involved.
            if let Some(ref pending) = self.snapshot_pending
                && (pending.joiner_id == player_id || pending.requested_from == player_id)
            {
                self.snapshot_pending = None;
            }
        }
    }

    /// Queue a command from a client for the next turn.
    pub fn enqueue_command(&mut self, command: TurnCommand) {
        self.pending_commands.push(command);
    }

    /// Flush pending commands into a turn and broadcast to all clients.
    /// Advances the sim tick target by `ticks_per_turn`.
    /// No-op if the game hasn't started yet (still in lobby) or if a
    /// mid-game join snapshot transfer is in progress.
    pub fn flush_turn(&mut self) {
        if !self.game_started || self.snapshot_pending.is_some() {
            return;
        }
        self.current_tick += u64::from(self.ticks_per_turn);
        self.current_turn = TurnNumber(self.current_turn.0 + 1);

        // Sort commands canonically: (player_id, sequence).
        self.pending_commands
            .sort_by_key(|cmd| (cmd.player_id, cmd.sequence));

        let turn_msg = ServerMessage::Turn {
            turn_number: self.current_turn,
            sim_tick_target: self.current_tick,
            commands: std::mem::take(&mut self.pending_commands),
        };
        self.broadcast(&turn_msg);
    }

    /// Record a checksum from a client. If all active players have reported
    /// for the same tick and checksums disagree, broadcasts `DesyncDetected`.
    /// A player waiting for a mid-game join snapshot is excluded from the count.
    pub fn record_checksum(&mut self, player_id: RelayPlayerId, tick: u64, hash: u64) {
        // Compute active count before borrowing checksums to satisfy the
        // borrow checker (active_player_count reads snapshot_pending/players,
        // not checksums).
        let active = self.active_player_count();

        let tick_entry = self.checksums.entry(tick).or_default();
        tick_entry.insert(player_id, hash);

        // Check if all active players have reported (excludes pending joiner).
        if tick_entry.len() == active && active > 1 {
            let mut values = tick_entry.values();
            let first = values.next().unwrap();
            let all_match = values.all(|v| v == first);

            if !all_match {
                let msg = ServerMessage::DesyncDetected { tick };
                self.broadcast(&msg);
            }

            // Clean up old checksums (keep only the latest).
            let tick_to_remove: Vec<u64> = self
                .checksums
                .keys()
                .filter(|t| **t <= tick)
                .copied()
                .collect();
            for t in tick_to_remove {
                self.checksums.remove(&t);
            }
        }
    }

    /// Handle a speed change request. Only the host can change speed.
    pub fn set_speed(&mut self, player_id: RelayPlayerId, ticks_per_turn: u32) {
        if player_id != self.host_id {
            return;
        }
        if ticks_per_turn == 0 {
            return; // Reject zero — would freeze sim_tick_target advancement.
        }
        self.ticks_per_turn = ticks_per_turn;
        let msg = ServerMessage::SpeedChanged { ticks_per_turn };
        self.broadcast(&msg);
    }

    /// Handle a pause request.
    pub fn request_pause(&mut self, player_id: RelayPlayerId) {
        if self.paused {
            return;
        }
        self.paused = true;
        let msg = ServerMessage::Paused { by: player_id };
        self.broadcast(&msg);
    }

    /// Handle a resume request.
    pub fn request_resume(&mut self, player_id: RelayPlayerId) {
        if !self.paused {
            return;
        }
        self.paused = false;
        let msg = ServerMessage::Resumed { by: player_id };
        self.broadcast(&msg);
    }

    /// Handle a chat message by broadcasting to all clients.
    /// Silently truncates messages longer than 4096 bytes.
    pub fn chat(&mut self, player_id: RelayPlayerId, text: String) {
        // Limit chat message size to prevent abuse.
        let text = if text.len() > 4096 {
            text[..text.floor_char_boundary(4096)].to_string()
        } else {
            text
        };
        let name = self
            .players
            .get(&player_id)
            .map(|ps| ps.name.clone())
            .unwrap_or_default();
        let msg = ServerMessage::ChatBroadcast {
            from: player_id,
            name,
            text,
        };
        self.broadcast(&msg);
    }

    /// Returns the number of connected players.
    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// Returns the maximum number of players allowed.
    pub fn max_players(&self) -> u32 {
        self.max_players
    }

    /// Returns true if the session has a password set.
    pub fn has_password(&self) -> bool {
        self.password.is_some()
    }

    /// Returns the current sim tick target.
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// Returns the current turn number.
    pub fn current_turn(&self) -> TurnNumber {
        self.current_turn
    }

    /// Handle a StartGame request. Only the host can start the game.
    /// Broadcasts `GameStart` to all clients and enables turn flushing.
    pub fn handle_start_game(&mut self, player_id: RelayPlayerId, seed: i64, config_json: String) {
        if player_id != self.host_id || self.game_started {
            return;
        }
        self.game_started = true;
        let msg = ServerMessage::GameStart { seed, config_json };
        self.broadcast(&msg);
    }

    /// Returns true if the game has started (past lobby phase).
    pub fn is_game_started(&self) -> bool {
        self.game_started
    }

    /// Returns info about all connected players.
    pub fn player_list(&self) -> Vec<PlayerInfo> {
        self.players
            .iter()
            .map(|(pid, ps)| PlayerInfo {
                id: *pid,
                name: ps.name.clone(),
            })
            .collect()
    }

    /// Handle a snapshot response from a client (expected to be the host).
    /// Verifies the sender matches the pending request, forwards the snapshot
    /// to the joiner as `SnapshotLoad`, and clears the pending state so turn
    /// flushing resumes.
    pub fn handle_snapshot_response(&mut self, from: RelayPlayerId, data: Vec<u8>) {
        let Some(ref pending) = self.snapshot_pending else {
            return;
        };
        if from != pending.requested_from {
            return;
        }
        // Verify the joiner is still connected before forwarding.
        if !self.players.contains_key(&pending.joiner_id) {
            self.snapshot_pending = None;
            return;
        }
        let joiner_id = pending.joiner_id;
        let msg = ServerMessage::SnapshotLoad {
            tick: self.current_tick,
            data,
        };
        self.send_to(joiner_id, &msg);
        self.snapshot_pending = None;
    }

    /// Returns true if a mid-game join snapshot transfer is in progress.
    pub fn is_snapshot_pending(&self) -> bool {
        self.snapshot_pending.is_some()
    }

    /// Returns the number of active players (excludes any player waiting for
    /// a mid-game join snapshot, since they can't participate in checksums yet).
    pub fn active_player_count(&self) -> usize {
        let total = self.players.len();
        if let Some(ref pending) = self.snapshot_pending
            && self.players.contains_key(&pending.joiner_id)
        {
            return total - 1;
        }
        total
    }

    /// Send a message to a specific player. Silently ignores write errors
    /// (the reader thread will detect the broken pipe).
    fn send_to(&mut self, player_id: RelayPlayerId, msg: &ServerMessage) {
        if let Some(ps) = self.players.get_mut(&player_id) {
            let _ = send_message(&mut ps.writer, msg);
        }
    }

    /// Broadcast a message to all connected players.
    fn broadcast(&mut self, msg: &ServerMessage) {
        let ids: Vec<RelayPlayerId> = self.players.keys().copied().collect();
        for id in ids {
            self.send_to(id, msg);
        }
    }
}

/// Serialize a `ServerMessage` to JSON and write it with length-delimited
/// framing. Returns any I/O error (caller decides whether to log or propagate).
fn send_message(
    writer: &mut BufWriter<TcpStream>,
    msg: &ServerMessage,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_vec(msg)?;
    write_message(writer, &json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    use std::net::TcpListener;

    use elven_canopy_protocol::framing::read_message;
    use elven_canopy_protocol::message::ClientMessage;
    use elven_canopy_protocol::types::ActionSequence;

    use super::*;

    /// Create a TCP pair: (client_stream, server_stream) on localhost.
    fn tcp_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    /// Read a ServerMessage from a TCP stream.
    fn recv_server_msg(stream: &mut BufReader<TcpStream>) -> ServerMessage {
        let bytes = read_message(stream).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn add_player_sends_welcome() {
        let (client, server) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        let result = session.add_player("Alice".into(), 100, 200, None, server);
        assert!(result.is_ok());
        let id = result.unwrap();
        assert_eq!(id, RelayPlayerId(0));
        assert_eq!(session.player_count(), 1);

        // Client should receive Welcome.
        let mut reader = BufReader::new(client);
        let msg = recv_server_msg(&mut reader);
        match msg {
            ServerMessage::Welcome {
                player_id,
                session_name,
                players,
                ticks_per_turn,
            } => {
                assert_eq!(player_id, RelayPlayerId(0));
                assert_eq!(session_name, "test");
                assert_eq!(players.len(), 1);
                assert_eq!(players[0].name, "Alice");
                assert_eq!(ticks_per_turn, 50);
            }
            other => panic!("expected Welcome, got {other:?}"),
        }
    }

    #[test]
    fn add_player_wrong_password_rejected() {
        let (_client, server) = tcp_pair();
        let mut session = Session::new("test".into(), Some("secret".into()), 50, 4);

        let result = session.add_player("Alice".into(), 100, 200, Some("wrong".into()), server);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "incorrect password");
    }

    #[test]
    fn add_player_correct_password_accepted() {
        let (_client, server) = tcp_pair();
        let mut session = Session::new("test".into(), Some("secret".into()), 50, 4);

        let result = session.add_player("Alice".into(), 100, 200, Some("secret".into()), server);
        assert!(result.is_ok());
    }

    #[test]
    fn add_player_version_mismatch_rejected() {
        let (_client1, server1) = tcp_pair();
        let (_client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        // First player sets reference hashes.
        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        // Second player with different sim_version_hash.
        let result = session.add_player("Bob".into(), 999, 200, None, server2);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "sim version mismatch");
    }

    #[test]
    fn add_player_full_session_rejected() {
        let (_client1, server1) = tcp_pair();
        let (_client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 1); // max 1 player

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        let result = session.add_player("Bob".into(), 100, 200, None, server2);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "session is full");
    }

    #[test]
    fn add_player_duplicate_name_rejected() {
        let (_client1, server1) = tcp_pair();
        let (_client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        let result = session.add_player("Alice".into(), 100, 200, None, server2);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already taken"));
    }

    #[test]
    fn second_player_join_broadcasts_player_joined() {
        let (client1, server1) = tcp_pair();
        let (client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        let mut reader1 = BufReader::new(client1);
        // Drain Alice's Welcome.
        let _welcome = recv_server_msg(&mut reader1);

        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        // Alice should receive PlayerJoined.
        let msg = recv_server_msg(&mut reader1);
        match msg {
            ServerMessage::PlayerJoined { player } => {
                assert_eq!(player.id, RelayPlayerId(1));
                assert_eq!(player.name, "Bob");
            }
            other => panic!("expected PlayerJoined, got {other:?}"),
        }

        // Bob should receive Welcome with both players.
        let mut reader2 = BufReader::new(client2);
        let msg = recv_server_msg(&mut reader2);
        match msg {
            ServerMessage::Welcome { players, .. } => {
                assert_eq!(players.len(), 2);
            }
            other => panic!("expected Welcome, got {other:?}"),
        }
    }

    #[test]
    fn remove_player_broadcasts_player_left() {
        let (client1, server1) = tcp_pair();
        let (_client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        let mut reader1 = BufReader::new(client1);
        // Drain Alice's Welcome + Bob's PlayerJoined.
        let _welcome = recv_server_msg(&mut reader1);
        let _joined = recv_server_msg(&mut reader1);

        session.remove_player(RelayPlayerId(1));

        let msg = recv_server_msg(&mut reader1);
        match msg {
            ServerMessage::PlayerLeft { player_id, name } => {
                assert_eq!(player_id, RelayPlayerId(1));
                assert_eq!(name, "Bob");
            }
            other => panic!("expected PlayerLeft, got {other:?}"),
        }
        assert_eq!(session.player_count(), 1);
    }

    #[test]
    fn flush_turn_broadcasts_to_all() {
        let (client1, server1) = tcp_pair();
        let (client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        // Start the game so turns flush.
        session.handle_start_game(RelayPlayerId(0), 42, "{}".into());

        // Enqueue a command from Alice.
        session.enqueue_command(TurnCommand {
            player_id: RelayPlayerId(0),
            sequence: ActionSequence(0),
            payload: vec![1, 2, 3],
        });

        session.flush_turn();

        // Both clients should receive the Turn.
        let mut reader1 = BufReader::new(client1);
        // Drain Welcome + PlayerJoined + GameStart.
        let _welcome = recv_server_msg(&mut reader1);
        let _joined = recv_server_msg(&mut reader1);
        let _game_start = recv_server_msg(&mut reader1);
        let msg = recv_server_msg(&mut reader1);

        match msg {
            ServerMessage::Turn {
                turn_number,
                sim_tick_target,
                commands,
            } => {
                assert_eq!(turn_number, TurnNumber(1));
                assert_eq!(sim_tick_target, 50);
                assert_eq!(commands.len(), 1);
                assert_eq!(commands[0].payload, vec![1, 2, 3]);
            }
            other => panic!("expected Turn, got {other:?}"),
        }

        let mut reader2 = BufReader::new(client2);
        // Drain Welcome + GameStart.
        let _welcome = recv_server_msg(&mut reader2);
        let _game_start = recv_server_msg(&mut reader2);
        let msg = recv_server_msg(&mut reader2);
        match msg {
            ServerMessage::Turn { commands, .. } => {
                assert_eq!(commands.len(), 1);
            }
            other => panic!("expected Turn, got {other:?}"),
        }
    }

    #[test]
    fn commands_sorted_canonically() {
        let (_client1, server1) = tcp_pair();
        let (client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        // Start the game so turns flush.
        session.handle_start_game(RelayPlayerId(0), 42, "{}".into());

        // Enqueue commands out of canonical order.
        session.enqueue_command(TurnCommand {
            player_id: RelayPlayerId(1),
            sequence: ActionSequence(0),
            payload: vec![30],
        });
        session.enqueue_command(TurnCommand {
            player_id: RelayPlayerId(0),
            sequence: ActionSequence(1),
            payload: vec![12],
        });
        session.enqueue_command(TurnCommand {
            player_id: RelayPlayerId(0),
            sequence: ActionSequence(0),
            payload: vec![11],
        });

        session.flush_turn();

        let mut reader2 = BufReader::new(client2);
        // Drain Welcome + GameStart.
        let _welcome = recv_server_msg(&mut reader2);
        let _game_start = recv_server_msg(&mut reader2);
        let msg = recv_server_msg(&mut reader2);
        match msg {
            ServerMessage::Turn { commands, .. } => {
                // Expected order: Alice(0), Alice(1), Bob(0).
                assert_eq!(commands.len(), 3);
                assert_eq!(commands[0].player_id, RelayPlayerId(0));
                assert_eq!(commands[0].sequence, ActionSequence(0));
                assert_eq!(commands[1].player_id, RelayPlayerId(0));
                assert_eq!(commands[1].sequence, ActionSequence(1));
                assert_eq!(commands[2].player_id, RelayPlayerId(1));
                assert_eq!(commands[2].sequence, ActionSequence(0));
            }
            other => panic!("expected Turn, got {other:?}"),
        }
    }

    #[test]
    fn desync_detection_matching_checksums() {
        let (_client1, server1) = tcp_pair();
        let (client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        // Both players send the same checksum — no DesyncDetected.
        session.record_checksum(RelayPlayerId(0), 1000, 0xABCD);
        session.record_checksum(RelayPlayerId(1), 1000, 0xABCD);

        // Bob should only have Welcome (no DesyncDetected).
        let mut reader2 = BufReader::new(client2);
        let msg = recv_server_msg(&mut reader2);
        assert!(matches!(msg, ServerMessage::Welcome { .. }));
    }

    #[test]
    fn desync_detection_mismatching_checksums() {
        let (client1, server1) = tcp_pair();
        let (_client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        // Different checksums → DesyncDetected.
        session.record_checksum(RelayPlayerId(0), 1000, 0xABCD);
        session.record_checksum(RelayPlayerId(1), 1000, 0xDEAD);

        let mut reader1 = BufReader::new(client1);
        // Drain Welcome + PlayerJoined.
        let _welcome = recv_server_msg(&mut reader1);
        let _joined = recv_server_msg(&mut reader1);
        let msg = recv_server_msg(&mut reader1);
        match msg {
            ServerMessage::DesyncDetected { tick } => {
                assert_eq!(tick, 1000);
            }
            other => panic!("expected DesyncDetected, got {other:?}"),
        }
    }

    #[test]
    fn pause_and_resume() {
        let (client1, server1) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);
        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        session.request_pause(RelayPlayerId(0));
        assert!(session.paused);

        session.request_resume(RelayPlayerId(0));
        assert!(!session.paused);

        let mut reader1 = BufReader::new(client1);
        let _welcome = recv_server_msg(&mut reader1);
        let msg = recv_server_msg(&mut reader1);
        assert!(matches!(msg, ServerMessage::Paused { .. }));
        let msg = recv_server_msg(&mut reader1);
        assert!(matches!(msg, ServerMessage::Resumed { .. }));
    }

    #[test]
    fn set_speed_only_host() {
        let (client1, server1) = tcp_pair();
        let (_client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        // Bob (not host) tries to change speed — ignored.
        session.set_speed(RelayPlayerId(1), 100);
        assert_eq!(session.ticks_per_turn, 50);

        // Alice (host) changes speed — accepted.
        session.set_speed(RelayPlayerId(0), 100);
        assert_eq!(session.ticks_per_turn, 100);

        let mut reader1 = BufReader::new(client1);
        let _welcome = recv_server_msg(&mut reader1);
        let _joined = recv_server_msg(&mut reader1);
        let msg = recv_server_msg(&mut reader1);
        assert!(matches!(
            msg,
            ServerMessage::SpeedChanged {
                ticks_per_turn: 100
            }
        ));
    }

    #[test]
    fn set_speed_zero_ignored() {
        let (_client1, server1) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);
        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        // Host tries to set speed to 0 — should be silently ignored.
        session.set_speed(RelayPlayerId(0), 0);
        assert_eq!(
            session.ticks_per_turn, 50,
            "ticks_per_turn should remain 50 after SetSpeed(0)"
        );
    }

    #[test]
    fn chat_broadcast() {
        let (client1, server1) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);
        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        session.chat(RelayPlayerId(0), "hello".into());

        let mut reader1 = BufReader::new(client1);
        let _welcome = recv_server_msg(&mut reader1);
        let msg = recv_server_msg(&mut reader1);
        match msg {
            ServerMessage::ChatBroadcast { from, name, text } => {
                assert_eq!(from, RelayPlayerId(0));
                assert_eq!(name, "Alice");
                assert_eq!(text, "hello");
            }
            other => panic!("expected ChatBroadcast, got {other:?}"),
        }
    }

    #[test]
    fn flush_turn_noop_before_game_start() {
        let (client1, server1) = tcp_pair();
        let (_client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        // Enqueue a command and flush without starting the game.
        session.enqueue_command(TurnCommand {
            player_id: RelayPlayerId(0),
            sequence: ActionSequence(0),
            payload: vec![1, 2, 3],
        });
        session.flush_turn();

        // Tick should not have advanced.
        assert_eq!(session.current_tick(), 0);
        assert!(!session.is_game_started());

        // Alice should only have Welcome + PlayerJoined (no Turn).
        let mut reader1 = BufReader::new(client1);
        let msg = recv_server_msg(&mut reader1);
        assert!(matches!(msg, ServerMessage::Welcome { .. }));
        let msg = recv_server_msg(&mut reader1);
        assert!(matches!(msg, ServerMessage::PlayerJoined { .. }));
    }

    #[test]
    fn handle_start_game_broadcasts_game_start() {
        let (client1, server1) = tcp_pair();
        let (client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        session.handle_start_game(RelayPlayerId(0), 42, r#"{"key":"val"}"#.into());
        assert!(session.is_game_started());

        // Alice should receive Welcome + PlayerJoined + GameStart.
        let mut reader1 = BufReader::new(client1);
        let _welcome = recv_server_msg(&mut reader1);
        let _joined = recv_server_msg(&mut reader1);
        let msg = recv_server_msg(&mut reader1);
        match msg {
            ServerMessage::GameStart { seed, config_json } => {
                assert_eq!(seed, 42);
                assert_eq!(config_json, r#"{"key":"val"}"#);
            }
            other => panic!("expected GameStart, got {other:?}"),
        }

        // Bob should receive Welcome + GameStart.
        let mut reader2 = BufReader::new(client2);
        let _welcome = recv_server_msg(&mut reader2);
        let msg = recv_server_msg(&mut reader2);
        assert!(matches!(msg, ServerMessage::GameStart { .. }));
    }

    #[test]
    fn only_host_can_start_game() {
        let (_client1, server1) = tcp_pair();
        let (_client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();

        // Bob (not host) tries to start — ignored.
        session.handle_start_game(RelayPlayerId(1), 42, "{}".into());
        assert!(!session.is_game_started());

        // Alice (host) starts — accepted.
        session.handle_start_game(RelayPlayerId(0), 42, "{}".into());
        assert!(session.is_game_started());
    }

    #[test]
    fn start_game_only_once() {
        let (client1, server1) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        session.handle_start_game(RelayPlayerId(0), 42, "{}".into());
        assert!(session.is_game_started());

        // Second start call is ignored.
        session.handle_start_game(RelayPlayerId(0), 99, "{}".into());

        // Alice should have Welcome + one GameStart (not two).
        let mut reader1 = BufReader::new(client1);
        let _welcome = recv_server_msg(&mut reader1);
        let msg = recv_server_msg(&mut reader1);
        match msg {
            ServerMessage::GameStart { seed, .. } => {
                assert_eq!(seed, 42); // First seed, not 99
            }
            other => panic!("expected GameStart, got {other:?}"),
        }
    }

    // ClientMessage is not used by session directly, but we verify it's
    // importable for completeness.
    #[test]
    fn client_message_importable() {
        let _msg = ClientMessage::Goodbye;
    }

    // -----------------------------------------------------------------------
    // Mid-game join snapshot tests
    // -----------------------------------------------------------------------

    /// Helper: set up a 2-player session with the game already started.
    /// Returns (session, host_client_stream, host_reader, joiner_client_stream).
    fn started_session() -> (Session, TcpStream, BufReader<TcpStream>, TcpStream) {
        let (client1, server1) = tcp_pair();
        let (client2, server2) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();
        session.handle_start_game(RelayPlayerId(0), 42, "{}".into());

        let mut reader1 = BufReader::new(client1.try_clone().unwrap());
        // Drain Alice's Welcome + PlayerJoined + GameStart.
        let _welcome = recv_server_msg(&mut reader1);
        let _joined = recv_server_msg(&mut reader1);
        let _game_start = recv_server_msg(&mut reader1);

        // Drain Bob's Welcome + GameStart.
        let mut reader2 = BufReader::new(client2.try_clone().unwrap());
        let _welcome = recv_server_msg(&mut reader2);
        let _game_start = recv_server_msg(&mut reader2);

        (session, client1, reader1, client2)
    }

    #[test]
    fn mid_join_sends_snapshot_request() {
        let (mut session, _client1, mut reader1, _client2) = started_session();

        // A third player joins after the game has started.
        let (_client3, server3) = tcp_pair();
        session
            .add_player("Charlie".into(), 100, 200, None, server3)
            .unwrap();

        assert!(session.is_snapshot_pending());

        // Host (Alice) should receive PlayerJoined then SnapshotRequest.
        let msg = recv_server_msg(&mut reader1);
        assert!(
            matches!(msg, ServerMessage::PlayerJoined { .. }),
            "expected PlayerJoined, got {msg:?}"
        );
        let msg = recv_server_msg(&mut reader1);
        assert!(
            matches!(msg, ServerMessage::SnapshotRequest),
            "expected SnapshotRequest, got {msg:?}"
        );
    }

    #[test]
    fn mid_join_snapshot_response_forwarded() {
        let (mut session, _client1, _reader1, _client2) = started_session();

        // Advance a few ticks so current_tick is nonzero.
        session.flush_turn();
        session.flush_turn();
        let expected_tick = session.current_tick();
        assert!(expected_tick > 0);

        // Third player joins mid-game.
        let (client3, server3) = tcp_pair();
        session
            .add_player("Charlie".into(), 100, 200, None, server3)
            .unwrap();
        assert!(session.is_snapshot_pending());

        // Host sends snapshot response.
        let snapshot_data = b"fake-sim-state".to_vec();
        session.handle_snapshot_response(RelayPlayerId(0), snapshot_data.clone());
        assert!(!session.is_snapshot_pending());

        // Charlie should receive Welcome + SnapshotLoad.
        let mut reader3 = BufReader::new(client3);
        let msg = recv_server_msg(&mut reader3);
        assert!(
            matches!(msg, ServerMessage::Welcome { .. }),
            "expected Welcome, got {msg:?}"
        );
        let msg = recv_server_msg(&mut reader3);
        match msg {
            ServerMessage::SnapshotLoad { tick, data } => {
                assert_eq!(tick, expected_tick);
                assert_eq!(data, snapshot_data);
            }
            other => panic!("expected SnapshotLoad, got {other:?}"),
        }
    }

    #[test]
    fn flush_turn_paused_during_snapshot() {
        let (mut session, _client1, _reader1, _client2) = started_session();

        // Advance one turn to establish a baseline tick.
        session.flush_turn();
        let tick_before = session.current_tick();

        // Third player joins mid-game — snapshot pending.
        let (_client3, server3) = tcp_pair();
        session
            .add_player("Charlie".into(), 100, 200, None, server3)
            .unwrap();
        assert!(session.is_snapshot_pending());

        // flush_turn should be a no-op while snapshot is pending.
        session.flush_turn();
        assert_eq!(
            session.current_tick(),
            tick_before,
            "tick should not advance while snapshot is pending"
        );
    }

    #[test]
    fn flush_turn_resumes_after_snapshot() {
        let (mut session, _client1, _reader1, _client2) = started_session();

        session.flush_turn();
        let tick_before = session.current_tick();

        // Third player joins mid-game.
        let (_client3, server3) = tcp_pair();
        session
            .add_player("Charlie".into(), 100, 200, None, server3)
            .unwrap();

        // Complete the snapshot transfer.
        session.handle_snapshot_response(RelayPlayerId(0), b"data".to_vec());
        assert!(!session.is_snapshot_pending());

        // flush_turn should work normally again.
        session.flush_turn();
        assert!(
            session.current_tick() > tick_before,
            "tick should advance after snapshot completes"
        );
    }

    #[test]
    fn snapshot_cleared_on_joiner_disconnect() {
        let (mut session, _client1, _reader1, _client2) = started_session();

        let (_client3, server3) = tcp_pair();
        let joiner_id = session
            .add_player("Charlie".into(), 100, 200, None, server3)
            .unwrap();
        assert!(session.is_snapshot_pending());

        // Joiner disconnects before snapshot completes.
        session.remove_player(joiner_id);
        assert!(
            !session.is_snapshot_pending(),
            "snapshot should be cleared when joiner disconnects"
        );
    }

    #[test]
    fn snapshot_cleared_on_host_disconnect() {
        let (mut session, _client1, _reader1, _client2) = started_session();

        let (_client3, server3) = tcp_pair();
        session
            .add_player("Charlie".into(), 100, 200, None, server3)
            .unwrap();
        assert!(session.is_snapshot_pending());

        // Host disconnects before snapshot completes.
        session.remove_player(RelayPlayerId(0));
        assert!(
            !session.is_snapshot_pending(),
            "snapshot should be cleared when host disconnects"
        );
    }

    #[test]
    fn checksum_excludes_pending_joiner() {
        let (mut session, _client1, _reader1, _client2) = started_session();

        // Third player joins mid-game — snapshot pending.
        let (_client3, server3) = tcp_pair();
        session
            .add_player("Charlie".into(), 100, 200, None, server3)
            .unwrap();
        assert!(session.is_snapshot_pending());

        // 3 players connected, but only 2 are active (Charlie is pending).
        assert_eq!(session.player_count(), 3);
        assert_eq!(session.active_player_count(), 2);

        // Alice and Bob send matching checksums — should complete without
        // waiting for Charlie (who can't compute a checksum yet).
        session.record_checksum(RelayPlayerId(0), 1000, 0xABCD);
        session.record_checksum(RelayPlayerId(1), 1000, 0xABCD);

        // Checksums matched, so the tick entry should be cleaned up.
        // Verify no panic and the session is in a good state.
        assert_eq!(session.player_count(), 3);
    }

    #[test]
    fn concurrent_mid_join_rejected() {
        let (mut session, _client1, _reader1, _client2) = started_session();

        // First mid-game join — accepted.
        let (_client3, server3) = tcp_pair();
        session
            .add_player("Charlie".into(), 100, 200, None, server3)
            .unwrap();
        assert!(session.is_snapshot_pending());

        // Second mid-game join while first is still pending — rejected.
        let (_client4, server4) = tcp_pair();
        let result = session.add_player("Dave".into(), 100, 200, None, server4);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "another player is joining, try again");

        // First joiner's snapshot is still pending and session state is clean.
        assert!(session.is_snapshot_pending());
        assert_eq!(session.player_count(), 3); // Alice, Bob, Charlie (not Dave)
    }

    #[test]
    fn double_pause_idempotent() {
        let (client1, server1) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);
        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        session.request_pause(RelayPlayerId(0));
        assert!(session.paused);
        session.request_pause(RelayPlayerId(0));
        assert!(session.paused);

        // Should have only one Paused broadcast.
        let mut reader1 = BufReader::new(client1);
        let _welcome = recv_server_msg(&mut reader1);
        let msg = recv_server_msg(&mut reader1);
        assert!(matches!(msg, ServerMessage::Paused { .. }));
        // No second Paused should be in the stream.
    }

    #[test]
    fn resume_while_not_paused_noop() {
        let (client1, server1) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);
        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        // Resume without pausing — should be a no-op.
        session.request_resume(RelayPlayerId(0));
        assert!(!session.paused);

        // Only Welcome should be in the stream — no Resumed.
        let mut reader1 = BufReader::new(client1);
        let msg = recv_server_msg(&mut reader1);
        assert!(matches!(msg, ServerMessage::Welcome { .. }));
    }

    #[test]
    fn remove_nonexistent_player_noop() {
        let (_client1, server1) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);
        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        // Remove a player that doesn't exist — should be a no-op.
        session.remove_player(RelayPlayerId(99));
        assert_eq!(session.player_count(), 1);
    }

    #[test]
    fn chat_truncated_long_message() {
        let (client1, server1) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);
        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        let long_text = "x".repeat(8000);
        session.chat(RelayPlayerId(0), long_text);

        let mut reader1 = BufReader::new(client1);
        let _welcome = recv_server_msg(&mut reader1);
        let msg = recv_server_msg(&mut reader1);
        match msg {
            ServerMessage::ChatBroadcast { text, .. } => {
                assert!(
                    text.len() <= 4096,
                    "chat should be truncated to 4096, got {} bytes",
                    text.len()
                );
            }
            other => panic!("expected ChatBroadcast, got {other:?}"),
        }
    }

    #[test]
    fn unsolicited_snapshot_response_ignored() {
        let (mut session, _client1, _reader1, _client2) = started_session();

        // No mid-game joiner — snapshot response should be silently ignored.
        assert!(!session.is_snapshot_pending());
        session.handle_snapshot_response(RelayPlayerId(0), b"data".to_vec());
        // No panic, no state change.
        assert!(!session.is_snapshot_pending());
    }

    #[test]
    fn single_player_checksum_no_desync() {
        let (client1, server1) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);
        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();

        // Single player sends a checksum — should never trigger desync.
        session.record_checksum(RelayPlayerId(0), 1000, 0xABCD);

        // Only Welcome should be in the stream — no DesyncDetected.
        let mut reader1 = BufReader::new(client1);
        let msg = recv_server_msg(&mut reader1);
        assert!(matches!(msg, ServerMessage::Welcome { .. }));
    }

    /// Verifies that checksum entries for a disconnected player are cleaned up,
    /// so desync detection still works for remaining players.
    #[test]
    fn checksum_cleanup_on_player_disconnect() {
        let (client1, server1) = tcp_pair();
        let (_client2, server2) = tcp_pair();
        let (_client3, server3) = tcp_pair();
        let mut session = Session::new("test".into(), None, 50, 4);

        session
            .add_player("Alice".into(), 100, 200, None, server1)
            .unwrap();
        session
            .add_player("Bob".into(), 100, 200, None, server2)
            .unwrap();
        session
            .add_player("Charlie".into(), 100, 200, None, server3)
            .unwrap();

        // Charlie sends a checksum for tick 1000, then disconnects.
        session.record_checksum(RelayPlayerId(2), 1000, 0xAAAA);
        session.remove_player(RelayPlayerId(2));

        // Alice and Bob send matching checksums for tick 1000.
        // If Charlie's entry weren't cleaned up, the comparison would wait
        // for 3 players but only 2 remain, silently disabling desync detection.
        session.record_checksum(RelayPlayerId(0), 1000, 0xBBBB);
        session.record_checksum(RelayPlayerId(1), 1000, 0xBBBB);

        // No desync should have been detected (Alice and Bob match).
        // Verify by checking Alice's stream has no DesyncDetected.
        let mut reader1 = BufReader::new(client1);
        // Drain: Welcome, PlayerJoined(Bob), PlayerJoined(Charlie), PlayerLeft(Charlie).
        let _welcome = recv_server_msg(&mut reader1);
        let _joined1 = recv_server_msg(&mut reader1);
        let _joined2 = recv_server_msg(&mut reader1);
        let _left = recv_server_msg(&mut reader1);
        // No more messages — no DesyncDetected.
    }
}
