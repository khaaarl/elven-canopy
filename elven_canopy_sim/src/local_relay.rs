// LocalRelay — accumulator-based tick pacer for single-player.
//
// Converts wall-clock time deltas (from Godot's `_process(delta)`) into
// `SessionMessage::AdvanceTo` messages that the `GameSession` can process.
// This is the single-player equivalent of the multiplayer relay's turn
// pacing — instead of waiting for network turns, it simply accumulates
// real time and emits tick-advance messages.
//
// Lives in the sim crate (not gdext) so it can be unit-tested without
// Godot dependencies. The gdext `SimBridge` owns an `Option<LocalRelay>`
// and calls `update()` each frame.
//
// Key behaviors:
// - Accumulates fractional ticks across frames (sub-tick precision).
// - Caps the accumulator at 0.1s worth of real time to prevent
//   spiral-of-death when frames take too long (e.g., window drag).
// - Returns `None` when no full tick has elapsed (or speed is 0).
// - `render_tick()` provides sub-tick interpolation for smooth rendering.
//
// See also: `session.rs` for the `GameSession` that processes the
// `AdvanceTo` messages, `sim_bridge.rs` for the frame loop that calls
// `update()`.
//
// **Not subject to the strict determinism constraint** — this is a
// presentation-layer pacer. The sim itself is deterministic; the relay
// just decides *when* to advance it.

use crate::session::SessionMessage;

/// Accumulator-based tick pacer for single-player mode.
///
/// Each frame, `update(delta, speed_multiplier, current_tick)` converts
/// wall-clock time into an optional `AdvanceTo` message. The accumulator
/// carries fractional ticks between frames for sub-tick precision.
pub struct LocalRelay {
    /// Fractional ticks accumulated but not yet emitted.
    accumulator: f64,
    /// Seconds per simulation tick (e.g., 0.001 for 1000 ticks/sec).
    seconds_per_tick: f64,
}

/// Maximum wall-clock delta to accumulate per frame (seconds).
/// Prevents spiral-of-death when the game is paused or a frame takes
/// too long (e.g., window drag, debugger breakpoint).
const MAX_DELTA: f64 = 0.1;

impl LocalRelay {
    /// Create a new relay with the given tick rate.
    ///
    /// `seconds_per_tick` is typically `config.tick_duration_ms as f64 / 1000.0`
    /// (e.g., 0.001 for the default 1ms tick).
    pub fn new(seconds_per_tick: f64) -> Self {
        Self {
            accumulator: 0.0,
            seconds_per_tick,
        }
    }

    /// Accumulate wall-clock time and return an `AdvanceTo` message if at
    /// least one full tick has elapsed.
    ///
    /// - `delta`: wall-clock seconds since last frame.
    /// - `speed_multiplier`: from `GameSession::speed_multiplier()` (0.0 when
    ///   paused, 1.0/2.0/5.0 for Normal/Fast/VeryFast).
    /// - `current_tick`: the session's current tick (used to compute the
    ///   target tick for the `AdvanceTo` message).
    ///
    /// Returns `None` if speed is zero (paused) or no full tick elapsed.
    pub fn update(
        &mut self,
        delta: f64,
        speed_multiplier: f64,
        current_tick: u64,
    ) -> Option<SessionMessage> {
        if speed_multiplier <= 0.0 {
            return None;
        }

        let clamped_delta = delta.min(MAX_DELTA);
        self.accumulator += clamped_delta * speed_multiplier;

        let ticks = (self.accumulator / self.seconds_per_tick) as u64;
        if ticks == 0 {
            return None;
        }

        self.accumulator -= ticks as f64 * self.seconds_per_tick;

        Some(SessionMessage::AdvanceTo {
            tick: current_tick + ticks,
        })
    }

    /// Return the fractional render tick for smooth interpolation.
    ///
    /// This is `current_tick + fraction`, where `fraction` is the
    /// accumulated-but-not-yet-emitted time expressed as a tick fraction.
    /// Renderers use this to interpolate creature positions between
    /// discrete sim ticks.
    pub fn render_tick(&self, current_tick: u64) -> f64 {
        current_tick as f64 + self.accumulator / self.seconds_per_tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPT: f64 = 0.001; // 1ms per tick (1000 ticks/sec)

    #[test]
    fn basic_tick_advancement() {
        let mut relay = LocalRelay::new(SPT);
        // 16ms at 1x speed → 16 ticks.
        let msg = relay.update(0.016, 1.0, 0);
        match msg {
            Some(SessionMessage::AdvanceTo { tick }) => assert_eq!(tick, 16),
            other => panic!("Expected AdvanceTo(16), got {other:?}"),
        }
    }

    #[test]
    fn speed_multiplier() {
        let mut relay = LocalRelay::new(SPT);
        // 16ms at 2x speed → 32 ticks.
        let msg = relay.update(0.016, 2.0, 0);
        match msg {
            Some(SessionMessage::AdvanceTo { tick }) => assert_eq!(tick, 32),
            other => panic!("Expected AdvanceTo(32), got {other:?}"),
        }
    }

    #[test]
    fn accumulator_carryover() {
        let mut relay = LocalRelay::new(SPT);
        // 0.5ms → 0 ticks, should return None.
        let msg = relay.update(0.0005, 1.0, 0);
        assert!(msg.is_none(), "Expected None for sub-tick delta");

        // Another 0.5ms → now 1.0ms accumulated → 1 tick.
        let msg = relay.update(0.0005, 1.0, 0);
        match msg {
            Some(SessionMessage::AdvanceTo { tick }) => assert_eq!(tick, 1),
            other => panic!("Expected AdvanceTo(1), got {other:?}"),
        }
    }

    #[test]
    fn spiral_of_death_cap() {
        let mut relay = LocalRelay::new(SPT);
        // 1 second at 1x → capped at 0.1s → 100 ticks (not 1000).
        let msg = relay.update(1.0, 1.0, 0);
        match msg {
            Some(SessionMessage::AdvanceTo { tick }) => assert_eq!(tick, 100),
            other => panic!("Expected AdvanceTo(100), got {other:?}"),
        }
    }

    #[test]
    fn render_tick_fraction() {
        let mut relay = LocalRelay::new(SPT);
        // Accumulate 0.5ms (half a tick) — no tick emitted.
        relay.update(0.0005, 1.0, 100);
        let rt = relay.render_tick(100);
        // Should be approximately 100.5.
        assert!((rt - 100.5).abs() < 0.01, "Expected ~100.5, got {rt}");
    }

    #[test]
    fn paused_returns_none() {
        let mut relay = LocalRelay::new(SPT);
        // speed_multiplier 0.0 → paused → None.
        let msg = relay.update(0.016, 0.0, 0);
        assert!(msg.is_none(), "Expected None when paused");
    }
}
