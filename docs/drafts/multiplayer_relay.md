# Multiplayer Networking — Relay Coordinator Design

Draft design for Elven Canopy's multiplayer architecture. Replaces the
Paxos-like consensus model described in `design_doc.md` §4 with a simpler
relay-coordinator approach. The deterministic sim foundations (command-driven
mutation, seeded PRNG, no HashMap, serializable state) remain unchanged.

Section 8 covers the UI design: main menu flow, lobby, in-game multiplayer
controls, ESC menu behavior, and save/load semantics.

---

## 1. Why a Relay Instead of Paxos

The design doc (§4) describes a "Paxos-like protocol" for canonical command
ordering. After discussion, we're replacing this with a simpler model: a
single relay coordinator that determines command ordering.

**Reasons:**

- **Paxos is overkill for 2–4 players.** Consensus algorithms solve agreement
  among unreliable nodes at scale. With a handful of players in a cooperative
  sim, losing any player is a game-level event regardless of protocol. There's
  no meaningful fault tolerance to gain.
- **Simpler to implement and debug.** A relay is a straightforward message
  broker — receive commands, assign order, broadcast. Paxos requires leader
  election, proposal rounds, and careful handling of split-brain scenarios.
- **NAT traversal comes for free.** All clients make outbound connections to
  the relay. Outbound connections work through virtually any NAT configuration.
  Only the relay needs to be reachable (public IP or port-forwarded), which is
  a solved problem for whoever chooses to host it.
- **No dependency on any platform.** Unlike Steam Networking or WebRTC with
  STUN/TURN infrastructure, a relay is just a TCP/UDP endpoint. It works on
  LAN, on the internet, behind corporate firewalls — anywhere a client can
  make an outbound connection.

The relay is **not** a game server. It never runs the sim. It's a thin message
broker that orders commands and manages sessions.

---

## 2. Architecture Overview

```
┌──────────┐     ┌──────────┐     ┌──────────┐
│ Client A │     │ Client B │     │ Client C │
│ (sim +   │     │ (sim +   │     │ (sim +   │
│  render) │     │  render) │     │  render) │
└────┬─────┘     └────┬─────┘     └────┬─────┘
     │                │                │
     │  outbound      │  outbound      │  outbound
     │  connection    │  connection    │  connection
     │                │                │
     └───────────┐    │    ┌───────────┘
                 │    │    │
              ┌──▼────▼────▼──┐
              │    Relay       │
              │  Coordinator   │
              │                │
              │ • command      │
              │   ordering     │
              │ • turn         │
              │   batching     │
              │ • session      │
              │   management   │
              │ • checksum     │
              │   comparison   │
              └────────────────┘
```

**Clients** run the full game: Godot rendering, GDExtension bridge, Rust sim.
Each client runs an identical sim instance. Player input becomes `SimCommand`
packets sent to the relay. The client applies commands only after receiving
them back from the relay in canonical order.

**The relay** is a lightweight coordinator. It:
- Accepts client connections and assigns player IDs.
- Receives `SimCommand` packets from clients.
- Batches commands into numbered **turns** at a fixed cadence.
- Broadcasts each turn to all clients.
- Compares periodic state checksums from clients to detect desync.
- Manages session lifecycle (join, leave, pause, save).

The relay has no knowledge of game logic — it doesn't depend on the sim crate.
It only knows about the network protocol: message framing, player IDs, turn
numbers, and checksums.

---

## 3. Turn-Based Command Batching

The relay divides wall-clock time into **turns** at a fixed cadence (e.g.,
every 50–100ms). Each turn has a sequence number and maps to a range of sim
ticks.

### Flow

1. Client generates a `SimCommand` from player input and sends it to the relay.
2. The relay collects all commands received during the current turn window.
3. At the end of the window, the relay creates a `Turn` message:
   ```
   Turn {
       turn_number: u64,
       sim_tick_target: u64,       // sim should advance to this tick
       commands: Vec<SimCommand>,  // canonically ordered
   }
   ```
4. The relay broadcasts the `Turn` to all clients.
5. Each client advances the sim to `sim_tick_target`, applying the turn's
   commands at that tick. All clients apply commands at the same tick in the
   same order, preserving determinism.

### Canonical ordering within a turn

Commands within a single turn are ordered by `(player_id, action_sequence)`.
`action_sequence` is a per-player monotonic counter so that a player's own
commands preserve their local ordering. Between players, `player_id` provides
a stable tiebreaker. This is deterministic and trivial — no voting or
negotiation needed.

### Tick pacing

The relay controls the sim's pace. It decides how many sim ticks each turn
covers based on the agreed sim speed (1x, 2x, 3x, paused). At 1x speed with
50ms turns:
- 1000 ticks/sec × 0.05 sec = 50 sim ticks per turn.

Clients may run ahead locally for smooth rendering (speculative execution) but
must be prepared to roll back if a turn contains unexpected commands. For a
cooperative sim where commands are infrequent, speculative execution is
optional and can be deferred — simply waiting for each turn before advancing
is fine and much simpler.

### Input latency

A player's command takes a round-trip through the relay before taking effect:
client → relay → batched into turn → broadcast back to client. With a 50ms
turn window and 30ms round-trip, that's ~80ms from click to visible result.
For a management sim with no twitch gameplay, this is imperceptible. The
batching window is the dominant factor — shorter windows mean lower latency
but more network overhead. 50–100ms is a reasonable starting range.

### Empty turns

If no player issues commands during a turn window, the relay still sends the
turn (with an empty command list). This tells clients "advance to tick N with
no input" and keeps everyone synchronized. The relay can optimize by sending
a compact "advance N ticks" message rather than a full turn struct.

---

## 4. Session Management

### Session creation

One player (or a dedicated server operator) starts the relay. The relay
listens on a configurable port.

### Session naming and access

Sessions have a **name** — a human-readable identifier displayed in session
lists and used for coordination between players ("join my session, it's called
amber-willow-42"). The relay generates a random default name (two-word-number
pattern) which the host can customize. Session names must be unique within a
relay (relevant for dedicated servers hosting multiple sessions).

Sessions optionally have a **password**. If set, the relay requires it during
the handshake. This provides basic access control — not cryptographic
security, just a barrier against accidental joins or casual griefing on public
relays.

### Game modes

Initial multiplayer supports one mode:

**Shared tree (co-op).** All players control the same tree spirit. Any player
can designate construction, assign tasks, spawn creatures, and issue commands.
Elves and resources belong to the shared pool. No per-player entity ownership,
no per-player fog of war. This requires no changes to the sim's world model —
it's just multiple command sources feeding into one sim.

**Separate trees** are part of the long-term vision but require F-multi-tree
(Phase 7). In this mode, each player controls their own tree spirit with their
own elves, mana, and construction. Trees can be allied (cooperative, shared
elf access) or rival (competitive, fog of war between players). This needs:
per-player entity ownership in the sim, per-player command validation (can't
issue commands to another player's elves), per-player fog of war rendering
(§17), and lobby UI for tree position selection. The separate-tree mode should
be configured during world setup, choosing the number and placement of player
trees and whether the game is cooperative or competitive. See `design_doc.md`
§1 for the vision (co-op shared tree, allied adjacent trees, rival groves,
asymmetric established-vs-sapling). Separate-tree multiplayer is out of scope
for F-multiplayer and will be tracked as part of F-multi-tree.

### Player identity

Each player has a **display name** shown in the lobby, player list overlay,
and notification toasts. Defaults to the OS username, customizable in settings
or at join time. The relay assigns each player a stable `PlayerId` (integer)
and a distinguishing **color** (from a palette of visually distinct colors,
auto-assigned, swappable in the lobby).

### Pre-game join (lobby phase)

Before the game starts, clients join the relay's lobby. The relay performs a
**handshake**:

1. **Version check:** Client sends hashes of its sim version and game config
   (per `design_doc.md` §4). The relay compares against the session's
   reference hashes (set by the first player to connect). Mismatch → reject
   with descriptive error.
2. **Player assignment:** Relay assigns a `PlayerId` and sends the client the
   lobby state: list of connected players, game settings (seed, tree params).

When the host starts the game, the relay broadcasts a `GameStart` message.
All clients generate identical initial sim state from the shared seed — no
state snapshot needed.

### Mid-game join

Joining an in-progress game requires a state snapshot. The relay requests one
from an existing client (since the relay doesn't run the sim). The flow:

1. New client connects and passes version check.
2. Relay pauses turn broadcasting (brief freeze for all players).
3. Relay requests a state snapshot from one existing client.
4. That client serializes its `SimState` and sends it to the relay.
5. Relay forwards the snapshot to the new client.
6. Relay resumes turn broadcasting.

The pause ensures no commands are applied during the snapshot, so the new
client starts from a consistent state. The freeze should be brief (a few
hundred milliseconds for serialization + transfer). This is acceptable for
a rare event like a player joining.

### Leaving

When a client disconnects (intentionally or via timeout), the relay:
1. Removes the player from the active player list.
2. Broadcasts a `PlayerLeft` message to remaining clients.
3. Remaining players see a toast notification ("Player 2 disconnected").

In shared-tree mode, the departure has no sim-level effect — the remaining
players continue with full control over the shared tree. If all players
disconnect, the relay can keep the session alive for a configurable timeout
(allowing reconnection) or close it.

In separate-tree mode (future, with F-multi-tree), the departing player's
elves go idle (wander, continue current tasks but accept no new ones) and
their tree goes dormant. The player can reconnect and resume control. Design
details deferred to F-multi-tree.

### Pause

In multiplayer, the ESC menu opens **without pausing the simulation** — the
game continues running for all players. The menu provides a "Request Pause"
button that sends a pause request to the relay. The relay pauses turn
advancement and broadcasts a notification ("Player 2 requested a pause"). Any
player can send a resume request to unpause.

Exception: if you are the only connected player in the session (all others
have disconnected), ESC pauses the sim immediately, matching single-player
behavior.

### Save

Any player can save the game at any time via the ESC menu. The save writes to
the saving player's local disk — no relay involvement, no pause needed. Since
all clients maintain identical sim state, the save file is valid regardless of
who creates it. Other players see a toast notification ("Player 2 saved the
game").

### Load

Loading a save replaces the entire sim state for all players. Because this is
disruptive, loading requires **confirmation from the session host** (or from
all players — policy TBD). Flow:

1. A player selects "Load Game" from the ESC menu and picks a local save file.
2. The client sends a `RequestLoad` message to the relay with the save file's
   metadata (timestamp, sim tick, session name).
3. The relay broadcasts a confirmation dialog to the host (or all players).
4. If approved: the relay pauses turns, the loading player sends the save data
   through the relay, all clients load the state, the relay resumes.
5. If rejected: the requesting player sees "Load request denied" and the game
   continues.

---

## 5. Desync Detection

Each client periodically computes a state checksum (hash of serialized
`SimState`, or a cheaper incremental hash — design TBD) and sends it to the
relay. The relay compares checksums from all clients for the same tick.

- **Match:** Everything is fine.
- **Mismatch:** The relay broadcasts a `DesyncDetected` message. Recovery
  options:
  - **Resync:** One client (arbitrarily chosen, or the one who joined first)
    sends a full state snapshot. Other clients load it.
  - **Abort:** End the session with an error.

Checksum frequency is configurable — every 1000 ticks (1 sim-second) is a
reasonable starting point. The checksum should be cheap to compute; a hash of
the full serialized state is simple but potentially expensive for large worlds.
An incremental approach (XOR of per-entity hashes, updated on mutation) would
be faster but more complex. Start with the simple approach and optimize if
needed.

---

## 6. Relay Deployment Scenarios

The relay is designed to work identically in all scenarios:

### Player-hosted relay (embedded)

A player clicks "Host Game" in the UI. Their game process starts the relay
in-process (separate thread or async task). They connect to their own relay as
a regular client. Other players connect to their public IP.

**Requirements:** The host needs a reachable IP address — either a public IP,
a port-forwarded router, or a LAN address for local play. This is the same
requirement as hosting any game server.

**Advantages:** No external infrastructure. Zero cost. Works immediately for
LAN play.

**Disadvantages:** Host needs port forwarding for internet play. Host
disconnect ends the game (though host migration is possible — see section 7).

### Dedicated relay server (standalone)

The relay runs as a standalone headless binary on a VPS or cloud instance. Any
player can connect. The relay operator doesn't need to be a player.

**Requirements:** A server with a public IP. A $5/month VPS is more than
sufficient — the relay is extremely lightweight (no sim computation, minimal
bandwidth for command packets).

**Advantages:** Always available. No NAT issues for any player (everyone
connects outbound). The relay doesn't go down when a player leaves.

**Disadvantages:** Ongoing infrastructure cost (minimal). Someone must operate
the server.

### LAN play

Identical to player-hosted, but the relay address is a LAN IP. No NAT issues,
no port forwarding, no internet required. Discovery could use mDNS/Bonjour
broadcast or simply "tell your friend the IP."

---

## 7. Future Considerations

### Host migration

If the player-hosted relay disconnects, the game could recover by having
another player start a new relay and all remaining clients reconnect. This
requires:
- Clients to detect relay disconnection.
- One client to start a new relay (election by lowest player ID, or manual).
- Remaining clients to connect to the new relay.
- A state snapshot from one client to resynchronize.

Not needed for initial implementation. The "dedicated relay" deployment avoids
this entirely.

### Steam integration

Steam could serve as a **discovery mechanism** without replacing the relay
architecture:
- Use Steam lobbies to advertise sessions (relay address + session token).
- Players browse/join via Steam's social features (friend invites, lobby
  browser).
- The actual game traffic still flows through the relay, not Steam's network.

Alternatively, Steam Networking Sockets could replace the raw TCP/UDP
transport between clients and relay, gaining Steam Datagram Relay's NAT
traversal benefits. This would be a transport-layer swap, not an architecture
change.

### Spectator mode

The relay could support spectator connections — clients that receive turns
but never send commands. The spectator runs the sim and renders, but has no
player ID and no input. Useful for streaming or coaching. Spectators could
have configurable delay (e.g., 30-second lag to prevent cheating in
competitive scenarios).

### Replay recording

The relay is the natural place to record replays: it sees the complete
canonical command stream. Save `(seed, config_hash, [Turn])` and you have
a perfect replay file. Any client can play it back without needing the
original players.

---

## 8. UI Design

### Main menu flow

The main menu gains a top-level split between single player and multiplayer:

```
Main Menu
├── Single Player
│   ├── New Game → seed/tree config → play
│   └── Load Game → pick save → play
├── Multiplayer
│   ├── Host Game → new/load → game config → lobby → play
│   └── Connect to Relay → address input → session list → lobby → play
└── Quit
```

**Single Player** is unchanged from the current flow. No networking, no relay,
no turns. The existing New Game and Load Game screens work as-is.

**Multiplayer** has two paths:

### Host Game

The player runs the relay in their game process (embedded). Steps:

1. Choose **New Game** (configure seed, tree params) or **Load Game** (pick a
   save file from a previous session).
2. Configure session settings: session name (random default, editable),
   optional password, max players.
3. The relay starts. The screen shows the relay address (IP:port) and session
   name for sharing with other players.
4. The host enters the lobby as the first player. Other players connect.
5. The host clicks "Start Game" when ready.

### Connect to Relay

The player connects to an existing relay (player-hosted or dedicated server).
Steps:

1. Enter relay address (IP:port or hostname:port).
2. The client connects and sees a **session list** — all active sessions on
   that relay with name, player count, and status (lobby / in-progress). For
   a player-hosted relay, there's exactly one session. For a dedicated server,
   there may be several.
3. Pick a session to join (enter password if required), or **Create New
   Session** (same flow as Host Game step 1–2, but the relay runs remotely).
4. Enter the lobby.

This merges "join an existing game" and "create a new game on a dedicated
server" into one flow — the choice happens after connecting, based on what the
relay offers. There's no need for separate menu items.

### Lobby

The lobby is shown after session creation or joining, before the game starts:

- **Player list:** Connected players with display names, assigned colors, and
  ready status.
- **Game settings:** Seed, tree params, session name (visible to all, editable
  by host only).
- **Session info:** Relay address (for sharing), password status, max players.
- **Chat:** Simple text chat between lobby members.
- **Ready / Start:** Players toggle ready status. Host clicks "Start Game"
  (enabled when the host decides the group is ready).
- **Host controls:** Kick player, change settings, transfer host role.

### In-game multiplayer UI

#### Player list overlay

Small persistent element (top-left or similar) showing connected players:
display name, color dot, and optionally a latency indicator. Compact — should
not interfere with gameplay.

#### Toast notifications

Brief messages for multiplayer events, fading after a few seconds:
- "Player 2 connected" / "Player 2 disconnected"
- "Player 2 saved the game"
- "Player 2 requested a pause" / "Player 2 resumed the game"
- "Desync detected — resyncing..."

Confirmation dialogs for disruptive actions:
- "Player 2 wants to load a save from [timestamp]. Approve?"

#### ESC menu (multiplayer variant)

ESC opens an overlay **without pausing the game** (see section 4, Pause).
Items:

- **Resume** — close the menu.
- **Save Game** — save to local disk. Other players see a toast.
- **Load Game** — pick a local save, send load request (requires host
  approval, see section 4, Load).
- **Request Pause** / **Resume Game** — toggle pause for all players via
  relay.
- **Disconnect** — leave the session, return to main menu.
- **Quit to Desktop** — disconnect and exit.

When you're the last connected player, ESC pauses immediately and the menu
shows standard single-player items without multiplayer-specific options.

#### Sim speed

Sim speed changes affect all players (the relay controls pacing). For
simplicity, **only the host can change sim speed**. Speed controls (pause,
1x, 2x, 3x) appear in the host's UI only. Other players see the current
speed but cannot change it. This avoids consensus complexity. Can be revisited
later if a voting or "slowest wins" model proves desirable.

### Open UI questions

- How are tree spirits visually distinguished in shared-tree mode?
  Color-coded cursors or command highlights could show who issued which
  construction designation.
- Should there be a pre-game map/seed preview in the lobby?
- Should the session list on a dedicated relay show game age, world size, etc.?
- How much lobby state persists if the relay stays running but all players
  leave?
- Should there be a recent-connections history for quick rejoin?

---

## 9. Implementation Plan

### New crate: `elven_canopy_relay`

A new crate in the workspace. **No sim dependency, no Godot dependency.** It
depends only on:
- `serde` / `serde_json` for message serialization.
- A networking library (probably `tokio` + a lightweight framing protocol, or
  raw `std::net` with a simple event loop — TBD).
- Shared message types (could be a small `elven_canopy_protocol` crate, or
  message types defined in the relay crate and depended on by the sim bridge).

The relay crate provides:
- `RelayServer` — the coordinator logic (accept connections, batch turns,
  broadcast, checksum comparison, session management).
- A `main.rs` for running as a standalone headless binary.
- A library API for embedding in the game process.

### Client-side networking (in `elven_canopy_gdext`)

The GDExtension bridge gains networking capabilities:
- Connect to relay, send commands, receive turns.
- Apply turns to the sim at the correct tick.
- Compute and send periodic checksums.
- Handle session events (player join/leave, desync, pause).

This could be a new file (`net_client.rs` or `multiplayer.rs`) in the gdext
crate, exposed to GDScript as a node or set of methods on `SimBridge`.

### GDScript UI

New scripts and scenes for lobby, connection, and in-game multiplayer UI
per section 8: main menu split, lobby screen, session list, player list
overlay, toast notifications, multiplayer ESC menu variant.

### Phasing

1. **Protocol definition:** Define message types, turn structure, handshake
   protocol. Write as code (Rust types with serde) not just documentation.
2. **Relay crate:** Implement `RelayServer` with basic session management,
   turn batching, and broadcasting. Test with a simple CLI client.
3. **Client networking:** Add relay connection to the gdext bridge. Test
   two headless sim instances staying in sync via the relay.
4. **Integration:** Wire networking into the Godot UI. Lobby screen, in-game
   multiplayer controls.
5. **Polish:** Desync detection, mid-game join, host migration, Steam
   discovery integration.

---

## 10. Relationship to Design Doc

This draft **supersedes** the multiplayer synchronization description in
`design_doc.md` §4 ("Multiplayer Synchronization" subsection), which
describes a "Paxos-like protocol."
The relay-coordinator model replaces that with a simpler approach while
preserving all other aspects of §4:

- Deterministic sim: unchanged.
- Command-driven mutation: unchanged.
- Sim speed synchronization: unchanged (relay controls pacing).
- State checksums for desync detection: unchanged.
- Config parity via hash comparison: unchanged (done during relay handshake).
- Replay recording: unchanged (relay records canonical command stream).
- Per-player fog of war (§17): unchanged (sim is omniscient, rendering layer
  filters per player).

When this design is finalized, `design_doc.md` §4 should be updated to
reference the relay model instead of Paxos.
