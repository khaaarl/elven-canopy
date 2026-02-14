// Simulation events — both the internal scheduling queue and player-visible
// narrative events.
//
// The sim uses a discrete event simulation model (see design doc §5). Entities
// schedule future events into a priority queue ordered by `(tick, sequence)`.
// The sim processes them in order, advancing the clock as needed. Empty ticks
// are free.
//
// This file defines two related but distinct concepts:
// - `ScheduledEvent`: internal events in the priority queue that drive the sim.
// - `SimEvent`: player-visible narrative events emitted as output.
//
// See also: `sim.rs` for the tick loop that processes scheduled events,
// `types.rs` for entity IDs and the `Species` enum.
//
// **Critical constraint: determinism.** Event ordering must be identical
// across all clients. The `(tick, sequence)` key provides a total order.

use crate::types::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

// ---------------------------------------------------------------------------
// Internal scheduled events (priority queue)
// ---------------------------------------------------------------------------

/// An event scheduled for future processing by the simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduledEvent {
    /// The tick at which this event should fire.
    pub tick: u64,
    /// Unique ordering key for deterministic tiebreaking within a tick.
    /// Lower values are processed first.
    pub sequence: u64,
    /// What should happen when this event fires.
    pub kind: ScheduledEventKind,
}

/// The types of internal events the sim can schedule.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ScheduledEventKind {
    /// Periodic heartbeat for a creature (wander decision, need decay, mood).
    CreatureHeartbeat { creature_id: CreatureId },
    /// A creature has finished traversing one nav edge and arrives at the next node.
    CreatureMovementComplete { creature_id: CreatureId, arrived_at: NavNodeId },
    /// Tree heartbeat (fruit production, mana capacity updates).
    TreeHeartbeat { tree_id: TreeId },
}

// We want a min-heap: lowest (tick, sequence) fires first.
// Rust's BinaryHeap is a max-heap, so we reverse the ordering.
impl PartialEq for ScheduledEvent {
    fn eq(&self, other: &Self) -> bool {
        self.tick == other.tick && self.sequence == other.sequence
    }
}

impl Eq for ScheduledEvent {}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse: smallest (tick, sequence) should be "greatest" for the max-heap.
        other
            .tick
            .cmp(&self.tick)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

/// Priority queue of scheduled events. Wraps a `BinaryHeap` with reversed
/// ordering to give us a min-heap (earliest tick fires first).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EventQueue {
    heap: BinaryHeap<ScheduledEvent>,
    /// Monotonic counter for deterministic ordering within a tick.
    next_sequence: u64,
}

impl EventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedule an event at the given tick.
    pub fn schedule(&mut self, tick: u64, kind: ScheduledEventKind) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.heap.push(ScheduledEvent {
            tick,
            sequence,
            kind,
        });
    }

    /// Peek at the next event without removing it.
    pub fn peek_tick(&self) -> Option<u64> {
        self.heap.peek().map(|e| e.tick)
    }

    /// Pop the next event if its tick is <= `up_to_tick`.
    pub fn pop_if_ready(&mut self, up_to_tick: u64) -> Option<ScheduledEvent> {
        if self.heap.peek().is_some_and(|e| e.tick <= up_to_tick) {
            self.heap.pop()
        } else {
            None
        }
    }

    /// Number of pending events.
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Player-visible narrative events (output)
// ---------------------------------------------------------------------------

/// A narrative event emitted by the simulation for the UI / event log.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimEvent {
    pub tick: u64,
    pub kind: SimEventKind,
}

/// Types of narrative events visible to the player.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SimEventKind {
    /// A new creature has arrived (spawn).
    CreatureArrived { creature_id: CreatureId, species: Species },
    /// Construction on a build project has started.
    BuildStarted { project_id: ProjectId },
    /// A build project has been completed.
    BuildCompleted { project_id: ProjectId },
    /// A build project has been cancelled.
    BuildCancelled { project_id: ProjectId },
    /// Simulation speed changed.
    SpeedChanged { speed: SimSpeed },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prng::GameRng;

    #[test]
    fn event_queue_ordering() {
        let mut rng = GameRng::new(42);
        let creature_a = CreatureId::new(&mut rng);
        let creature_b = CreatureId::new(&mut rng);

        let mut queue = EventQueue::new();
        // Schedule out of order.
        queue.schedule(100, ScheduledEventKind::CreatureHeartbeat { creature_id: creature_b });
        queue.schedule(50, ScheduledEventKind::CreatureHeartbeat { creature_id: creature_a });
        queue.schedule(50, ScheduledEventKind::CreatureHeartbeat { creature_id: creature_b });

        // Should pop in tick order, then sequence order within a tick.
        let first = queue.pop_if_ready(200).unwrap();
        assert_eq!(first.tick, 50);
        assert_eq!(first.sequence, 1); // creature_a was scheduled second but at tick 50

        let second = queue.pop_if_ready(200).unwrap();
        assert_eq!(second.tick, 50);
        assert_eq!(second.sequence, 2); // creature_b at tick 50

        let third = queue.pop_if_ready(200).unwrap();
        assert_eq!(third.tick, 100);

        assert!(queue.pop_if_ready(200).is_none());
    }

    #[test]
    fn pop_if_ready_respects_tick_limit() {
        let mut rng = GameRng::new(42);
        let creature = CreatureId::new(&mut rng);

        let mut queue = EventQueue::new();
        queue.schedule(100, ScheduledEventKind::CreatureHeartbeat { creature_id: creature });

        // Not ready yet.
        assert!(queue.pop_if_ready(99).is_none());
        // Ready now.
        assert!(queue.pop_if_ready(100).is_some());
    }

    #[test]
    fn event_queue_serialization() {
        let mut rng = GameRng::new(42);
        let creature = CreatureId::new(&mut rng);

        let mut queue = EventQueue::new();
        queue.schedule(10, ScheduledEventKind::CreatureHeartbeat { creature_id: creature });
        queue.schedule(20, ScheduledEventKind::CreatureHeartbeat { creature_id: creature });

        let json = serde_json::to_string(&queue).unwrap();
        let mut restored: EventQueue = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.len(), 2);
        let first = restored.pop_if_ready(100).unwrap();
        assert_eq!(first.tick, 10);
    }
}
