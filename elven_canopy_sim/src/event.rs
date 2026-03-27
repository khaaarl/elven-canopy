// Simulation events — both the internal scheduling queue and player-visible
// narrative events.
//
// The sim uses a discrete event simulation model. Entities schedule future
// events into a priority queue ordered by `(tick, sequence)`. The tick loop
// in `sim/mod.rs` pops events up to the current tick and dispatches them.
// Empty ticks are free (the queue skips forward).
//
// This file defines two related but distinct concepts:
//
// ## `ScheduledEvent` — internal sim events (priority queue)
//
// These drive the simulation's behavior. Current event types:
// - `CreatureActivation` — the core creature behavior loop. Each activation,
//   the creature does one action (check for a task, walk 1 nav edge, or do
//   1 unit of work) and schedules its next activation based on how long the
//   action took. See `sim/activation.rs` `process_creature_activation()`.
// - `CreatureHeartbeat` — periodic non-movement checks (mood, mana, needs).
//   Does NOT drive movement — that's entirely the activation chain.
// - `TreeHeartbeat` — periodic tree updates (fruit, mana capacity).
// - `LogisticsHeartbeat` — periodic scan of buildings with logistics config;
//   creates `Haul` tasks to fill unmet item wants.
// - `ProjectileTick` — batched per-tick update of all in-flight projectiles.
//   Scheduled when the first projectile spawns (table 0→1), self-reschedules
//   for tick+1 while projectiles remain. See `sim/combat.rs` `process_projectile_tick()`.
//
// The `EventQueue` wraps a `BinaryHeap` with reversed `Ord` to get min-heap
// behavior (earliest tick pops first). A monotonic `next_sequence` counter
// breaks ties within the same tick deterministically — events scheduled first
// fire first.
//
// Activation and heartbeat events check `vital_status` before processing
// and do not reschedule for dead creatures, effectively terminating their
// event chains.
//
// `cancel_creature_activations()` removes all pending `CreatureActivation`
// events for a creature. Called by `abort_current_action()` when a creature's
// action is forcibly interrupted (death, flee, nav invalidation) to prevent
// orphaned activations from causing double-speed movement (B-erratic-movement).
//
// ## `SimEvent` — player-visible narrative events (output)
//
// Emitted by the sim as output for the UI event log. Not queued — produced
// synchronously during event processing and collected by the caller.
// Includes `CreatureIncapacitated`, `CreatureDied` (with cause: Debug or Damage),
// `CreatureDamaged` (melee strike hit), `ProjectileHitCreature`,
// `ProjectileHitSurface`, and `ItemBroken` (durability reached zero).
//
// See also: `sim/mod.rs` for the tick loop that processes scheduled events,
// `types.rs` for entity IDs and the `Species` enum, `task.rs` for the task
// system that `CreatureActivation` interacts with.
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
    /// Periodic heartbeat for a creature (mood decay, mana generation, need updates).
    /// Does NOT drive movement — that's handled by `CreatureActivation`.
    CreatureHeartbeat { creature_id: CreatureId },
    /// A creature's activation fires: it does one action (walk 1 edge or work)
    /// and schedules the next activation based on how long the action takes.
    CreatureActivation { creature_id: CreatureId },
    /// Tree heartbeat (fruit production, mana capacity updates).
    TreeHeartbeat { tree_id: TreeId },
    /// Logistics heartbeat: scan buildings for unmet wants and create haul tasks.
    LogisticsHeartbeat,
    /// Batched projectile update: advances all in-flight projectiles by one tick.
    /// Scheduled when the first projectile is spawned (table goes 0→1) and
    /// self-reschedules for tick+1 while projectiles remain. Stops when the
    /// table is empty.
    ProjectileTick,
    /// Periodic grass regrowth sweep. Iterates all grassless dirt voxels and
    /// probabilistically regrows each one (removes from the grassless set).
    /// Self-reschedules at `tick + grass_regrowth_interval_ticks`.
    GrassRegrowth,
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

    /// Iterate over all pending events (unordered). Used for backfill checks
    /// that need to detect whether a particular event kind is already scheduled.
    pub fn iter(&self) -> impl Iterator<Item = &ScheduledEvent> {
        self.heap.iter()
    }

    /// Remove all pending `CreatureActivation` events for a specific creature.
    /// Called when a creature's action is forcibly aborted (death, flee, nav
    /// invalidation) to prevent orphaned activations from firing and causing
    /// erratic behavior (B-erratic-movement).
    pub(crate) fn cancel_creature_activations(&mut self, creature_id: CreatureId) {
        let old_heap = std::mem::take(&mut self.heap);
        self.heap = old_heap
            .into_iter()
            .filter(|e| {
                !matches!(
                    &e.kind,
                    ScheduledEventKind::CreatureActivation { creature_id: id }
                    if *id == creature_id
                )
            })
            .collect();
    }

    /// Count pending `CreatureActivation` events for a specific creature.
    /// Used in tests to verify orphaned events are cleaned up.
    #[cfg(test)]
    pub fn count_creature_activations(&self, creature_id: CreatureId) -> usize {
        self.heap
            .iter()
            .filter(|e| {
                matches!(
                    &e.kind,
                    ScheduledEventKind::CreatureActivation { creature_id: id }
                    if *id == creature_id
                )
            })
            .count()
    }
}

// ---------------------------------------------------------------------------
// Player-visible narrative events (output)
// ---------------------------------------------------------------------------

/// A narrative event emitted by the simulation for the UI / event log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimEvent {
    pub tick: u64,
    pub kind: SimEventKind,
}

/// Types of narrative events visible to the player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SimEventKind {
    /// A new creature has arrived (spawn).
    CreatureArrived {
        creature_id: CreatureId,
        species: Species,
    },
    /// A build project has been designated (blueprint created).
    BlueprintDesignated { project_id: ProjectId },
    /// Construction on a build project has started.
    BuildStarted { project_id: ProjectId },
    /// A build project has been completed.
    BuildCompleted { project_id: ProjectId },
    /// A build project has been cancelled.
    BuildCancelled { project_id: ProjectId },
    /// A creature has been incapacitated (HP reached 0). It is bleeding out
    /// and will die unless healed.
    CreatureIncapacitated {
        creature_id: CreatureId,
        species: Species,
        position: VoxelCoord,
    },
    /// A creature has died.
    CreatureDied {
        creature_id: CreatureId,
        species: Species,
        position: VoxelCoord,
        cause: DeathCause,
    },
    /// A melee attack missed (defender evaded).
    MeleeAttackMissed {
        attacker_id: CreatureId,
        target_id: CreatureId,
    },
    /// A melee attack scored a critical hit (double damage).
    MeleeAttackCritical {
        attacker_id: CreatureId,
        target_id: CreatureId,
    },
    /// A creature took melee damage from another creature.
    CreatureDamaged {
        attacker_id: CreatureId,
        target_id: CreatureId,
        damage: i64,
        remaining_hp: i64,
    },
    /// A projectile was evaded by the target creature.
    ProjectileEvaded {
        target_id: CreatureId,
        shooter_id: Option<CreatureId>,
    },
    /// A projectile scored a critical hit on a creature.
    ProjectileCritical {
        target_id: CreatureId,
        shooter_id: Option<CreatureId>,
    },
    /// A projectile hit a creature.
    ProjectileHitCreature {
        target_id: CreatureId,
        damage: i64,
        remaining_hp: i64,
        shooter_id: Option<CreatureId>,
    },
    /// A projectile hit a solid surface and stuck/shattered.
    ProjectileHitSurface { position: VoxelCoord },
    /// A creature launched a projectile at a target.
    ProjectileLaunched {
        attacker_id: CreatureId,
        target_id: CreatureId,
    },
    /// A new military group was created.
    MilitaryGroupCreated { group_id: MilitaryGroupId },
    /// A military group was disbanded. Members returned to civilian status.
    MilitaryGroupDeleted {
        group_id: MilitaryGroupId,
        name: String,
        member_count: usize,
    },
    /// An item broke (current_hp reached 0) and was removed.
    ItemBroken {
        item_kind: crate::inventory::ItemKind,
        material: Option<crate::inventory::Material>,
        owner: Option<CreatureId>,
    },
    /// A creature fell due to gravity (unsupported position).
    CreatureFell {
        creature_id: CreatureId,
        from: VoxelCoord,
        to: VoxelCoord,
        damage: i64,
        remaining_hp: i64,
    },
    /// A creature's path was assigned or changed (F-path-core).
    PathAssigned {
        creature_id: CreatureId,
        path_id: PathId,
    },
    /// A creature was successfully tamed and joined the player's civ (F-taming).
    CreatureTamed {
        creature_id: CreatureId,
        tamer_id: CreatureId,
    },
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
        queue.schedule(
            100,
            ScheduledEventKind::CreatureHeartbeat {
                creature_id: creature_b,
            },
        );
        queue.schedule(
            50,
            ScheduledEventKind::CreatureHeartbeat {
                creature_id: creature_a,
            },
        );
        queue.schedule(
            50,
            ScheduledEventKind::CreatureHeartbeat {
                creature_id: creature_b,
            },
        );

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
        queue.schedule(
            100,
            ScheduledEventKind::CreatureHeartbeat {
                creature_id: creature,
            },
        );

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
        queue.schedule(
            10,
            ScheduledEventKind::CreatureHeartbeat {
                creature_id: creature,
            },
        );
        queue.schedule(
            20,
            ScheduledEventKind::CreatureHeartbeat {
                creature_id: creature,
            },
        );

        let json = serde_json::to_string(&queue).unwrap();
        let mut restored: EventQueue = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.len(), 2);
        let first = restored.pop_if_ready(100).unwrap();
        assert_eq!(first.tick, 10);
    }
}
