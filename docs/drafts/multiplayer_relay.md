# Multiplayer Networking — Relay Coordinator Design

Draft design for Elven Canopy's multiplayer architecture. Replaces the
Paxos-like consensus model described in `design_doc.md` §4 with a simpler
relay-coordinator approach. The deterministic sim foundations (command-driven
mutation, seeded PRNG, no HashMap, serializable state) remain unchanged.

**UI is not yet designed.** Section 8 collects initial ideas, but lobby UI,
in-game multiplayer controls, and session management screens need their own
design pass before implementation.

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
generates a **session token** — a short human-readable code (e.g., 6
alphanumeric characters) that other players use to join. The relay listens on
a configurable port.

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
3. The sim handles player departure as a game-level event (their elves become
   idle, their tree goes dormant, etc. — game design TBD).

### Pause and save

Any player can request a pause (sends a `RequestPause` message). The relay
stops advancing turns. All players see the game paused. Unpausing requires
the requesting player (or majority vote — design TBD) to send `RequestResume`.

Saving in multiplayer: one client performs the save (same as single-player),
triggered by the host or by consensus. The relay doesn't need to be involved
beyond pausing the sim during the save.

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

## 8. UI Considerations (Not Yet Designed)

UI for multiplayer has not been discussed in detail. The following are initial
ideas to be refined in a separate design pass.

### Lobby / session management

- **Host Game screen:** Configure game settings (seed, tree params, sim
  speed), start the relay, display the session token / relay address for
  sharing. Option to start as embedded relay or connect to an external relay.
- **Join Game screen:** Enter relay address + session token, or paste a
  combined connection string. Version/config mismatch errors displayed here.
- **Lobby view:** After connecting, show connected players, their readiness
  status, and basic info (player name, chosen tree position). Chat optional
  but nice. Host can kick players, start the game, or adjust settings.

### In-game multiplayer UI

- **Player list:** Small overlay showing connected players (name, color,
  maybe ping/latency indicator).
- **Sim speed controls:** Need to handle consensus — if one player changes
  speed, it affects everyone. Could require host approval, or use "slowest
  player wins" policy.
- **Pause:** Any player can pause? Or only host? Needs a policy decision.
- **Desync notification:** If detected, a prominent warning with options
  (resync, disconnect, ignore).
- **Player join/leave notifications:** Toast messages when players connect
  or disconnect.
- **Chat:** Text chat between players. Optional but standard.

### Connection flow integration

The existing scene flow is: main menu → new game / load → game. Multiplayer
would add:
- Main menu gains "Host Game" and "Join Game" buttons (or a "Multiplayer"
  submenu).
- "Host Game" goes to a lobby screen (extended new game screen).
- "Join Game" goes to a connection screen, then to the lobby.
- The lobby transitions to the game scene when the host starts.
- Mid-game join goes directly to the game scene after state sync.

### Open UI questions

- How does the player choose their tree position in a multi-tree setup?
- How are tree spirits visually distinguished (color, icon, name)?
- Should there be a pre-game map/seed preview in the lobby?
- How does save/load work in multiplayer (who initiates, where is it stored)?
- What happens to a player's elves/tree when they disconnect mid-game?

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

New scripts and scenes for lobby, connection, and in-game multiplayer UI.
These depend on the UI design pass (section 8) and are not specified here.

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
