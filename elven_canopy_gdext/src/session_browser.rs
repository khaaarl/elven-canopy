// GDExtension class for browsing relay sessions before joining.
//
// `SessionBrowser` is a lightweight Godot RefCounted class that wraps a
// `RelayConnection` (pre-handshake TCP connection to a relay). GDScript
// uses it in the join-game menu to connect to a relay, list available
// sessions, and let the player pick one before transitioning to the
// game scene.
//
// The chosen `session_id` is stored in `GameSession` (autoload) and
// used by `SimBridge.join_game()` when the game scene loads.
//
// This class is separate from `SimBridge` because session browsing
// happens in a menu scene that doesn't have a `SimBridge` node.
//
// See also: join_game_menu.gd (GDScript consumer), client.rs (RelayConnection),
// sim_bridge.rs (join_game method that uses the chosen session_id).

use godot::prelude::*;

use elven_canopy_relay::client::RelayConnection;

/// Godot-exposed session browser for discovering relay sessions.
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct SessionBrowser {
    connection: Option<RelayConnection>,
}

#[godot_api]
impl IRefCounted for SessionBrowser {
    fn init(_base: Base<RefCounted>) -> Self {
        Self { connection: None }
    }
}

#[godot_api]
impl SessionBrowser {
    /// Connect to a relay server. Returns true on success.
    #[func]
    fn connect_to_relay(&mut self, address: GString) -> bool {
        let addr = address.to_string();
        match RelayConnection::connect(&addr) {
            Ok(conn) => {
                self.connection = Some(conn);
                true
            }
            Err(e) => {
                godot_error!("SessionBrowser: connect failed: {e}");
                false
            }
        }
    }

    /// List available sessions on the relay. Returns an Array of Dictionaries,
    /// each containing: session_id (int), name (String), player_count (int),
    /// max_players (int), has_password (bool), game_started (bool).
    /// Returns an empty array on error.
    #[func]
    fn list_sessions(&mut self) -> Array<VarDictionary> {
        let conn = match &mut self.connection {
            Some(c) => c,
            None => {
                godot_error!("SessionBrowser: not connected");
                return Array::new();
            }
        };

        match conn.list_sessions() {
            Ok(sessions) => {
                let mut arr = Array::new();
                for s in sessions {
                    let mut dict = VarDictionary::new();
                    dict.set("session_id", s.session_id.0 as i64);
                    dict.set("name", s.name.to_godot());
                    dict.set("player_count", s.player_count as i64);
                    dict.set("max_players", s.max_players as i64);
                    dict.set("has_password", s.has_password);
                    dict.set("game_started", s.game_started);
                    arr.push(&dict);
                }
                arr
            }
            Err(e) => {
                godot_error!("SessionBrowser: list_sessions failed: {e}");
                Array::new()
            }
        }
    }

    /// Disconnect from the relay (drops the TCP connection).
    #[func]
    fn disconnect_relay(&mut self) {
        self.connection = None;
    }

    /// Returns true if currently connected to a relay.
    #[func]
    fn is_relay_connected(&self) -> bool {
        self.connection.is_some()
    }
}
