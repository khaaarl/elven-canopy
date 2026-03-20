// Combat system — melee, ranged, projectiles, fleeing, hostile AI, and diplomacy.
//
// Covers the complete combat pipeline: player attack commands, attack-move AI,
// melee strikes (with weapon selection — spears for reach, clubs for damage,
// bare hands as fallback), ranged shooting (bow + arrow with ballistic
// trajectories), projectile simulation, damage application, creature death,
// flee behavior, hostile pursuit, friendly-fire avoidance, and voxel exclusion
// enforcement. Also includes civilization diplomacy, military group management,
// arrow durability degradation on impact (apply_arrow_impact_damage),
// melee weapon degradation on strike (apply_melee_weapon_impact_damage),
// arrow-condition damage scaling (scale_damage_by_arrow_hp), and armor
// damage reduction with equipment degradation (apply_armor_reduction).
//
// See also: `projectile.rs` (sub-voxel trajectory math), `preemption.rs`
// (task priority for combat interruption), `movement.rs` (tactical repositioning).
use super::*;
use crate::db::{ActionKind, MoveAction};
use crate::event::{ScheduledEventKind, SimEvent, SimEventKind};
use crate::inventory;
use crate::pathfinding;
use crate::preemption;
use crate::projectile::SubVoxelVec;
use crate::task;
use crate::types::NavEdgeId;

impl SimState {
    /// Process an `AttackCreature` command: create an AttackTarget task for
    /// the attacker and immediately assign it, preempting lower-priority tasks.
    pub(crate) fn command_attack_creature(
        &mut self,
        attacker_id: CreatureId,
        target_id: CreatureId,
        _events: &mut Vec<SimEvent>,
    ) {
        // Validate: both creatures alive and distinct.
        let attacker = match self.db.creatures.get(&attacker_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };
        let target = match self.db.creatures.get(&target_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };
        if attacker_id == target_id {
            return;
        }

        let target_node = match self
            .graph_for_species(target.species)
            .find_nearest_node(target.position)
        {
            Some(n) => n,
            None => return,
        };

        // Check preemption: can PlayerCombat preempt the current task?
        let mut mid_move = false;
        if let Some(current_task_id) = attacker.current_task
            && let Some(current_task) = self.db.tasks.get(&current_task_id)
        {
            let current_level =
                preemption::preemption_level(current_task.kind_tag, current_task.origin);
            let current_origin = current_task.origin;
            if !preemption::can_preempt(
                current_level,
                current_origin,
                preemption::PreemptionLevel::PlayerCombat,
                task::TaskOrigin::PlayerDirected,
            ) {
                return;
            }
            mid_move = self.preempt_task(attacker_id, current_task_id);
        } else if attacker.action_kind == ActionKind::Move && attacker.next_available_tick.is_some()
        {
            // Mid-wander-move — let the step finish (B-erratic-movement-2).
            mid_move = true;
        }

        // Create and immediately assign the AttackTarget task.
        let task_id = TaskId::new(&mut self.rng);
        let target_pos = self.nav_graph.node(target_node).position;
        let new_task = task::Task {
            id: task_id,
            kind: task::TaskKind::AttackTarget { target: target_id },
            state: task::TaskState::InProgress,
            location: target_pos,
            progress: 0,
            total_cost: 0,
            required_species: Some(attacker.species),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: Some(target_id),
        };
        self.insert_task(new_task);
        // Assign directly — skip Available state.
        if let Some(mut c) = self.db.creatures.get(&attacker_id) {
            c.current_task = Some(task_id);
            c.path = None;
            let _ = self.db.creatures.update_no_fk(c);
        }

        // Only schedule activation if no Move action is in-flight.
        if !mid_move {
            self.event_queue.schedule(
                self.tick + 1,
                ScheduledEventKind::CreatureActivation {
                    creature_id: attacker_id,
                },
            );
        }
    }

    /// Process an `AttackMove` command: create an AttackMove task for the
    /// creature and immediately assign it, preempting lower-priority tasks.
    /// The creature walks toward the destination, engaging hostiles en route.
    pub(crate) fn command_attack_move(
        &mut self,
        creature_id: CreatureId,
        destination: VoxelCoord,
        _events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };

        if self.nav_graph.find_nearest_node(destination).is_none() {
            return;
        }

        // Check preemption: can PlayerCombat preempt the current task?
        let mut mid_move = false;
        if let Some(current_task_id) = creature.current_task
            && let Some(current_task) = self.db.tasks.get(&current_task_id)
        {
            let current_level =
                preemption::preemption_level(current_task.kind_tag, current_task.origin);
            let current_origin = current_task.origin;
            if !preemption::can_preempt(
                current_level,
                current_origin,
                preemption::PreemptionLevel::PlayerCombat,
                task::TaskOrigin::PlayerDirected,
            ) {
                return;
            }
            mid_move = self.preempt_task(creature_id, current_task_id);
        } else if creature.action_kind == ActionKind::Move && creature.next_available_tick.is_some()
        {
            // Mid-wander-move — let the step finish (B-erratic-movement-2).
            mid_move = true;
        }

        let task_id = TaskId::new(&mut self.rng);
        let species = creature.species;
        let new_task = task::Task {
            id: task_id,
            kind: task::TaskKind::AttackMove,
            state: task::TaskState::InProgress,
            location: destination,
            progress: 0,
            total_cost: 0,
            required_species: Some(species),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        self.insert_task(new_task);

        // Insert extension row with destination.
        let _ =
            self.db
                .task_attack_move_data
                .insert_auto_no_fk(|id| crate::db::TaskAttackMoveData {
                    id,
                    task_id,
                    destination,
                });

        // Assign directly — skip Available state.
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.current_task = Some(task_id);
            c.path = None;
            let _ = self.db.creatures.update_no_fk(c);
        }

        if !mid_move {
            self.event_queue.schedule(
                self.tick + 1,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
        }
    }

    /// Process a `GroupAttackMove` command: spread multiple creatures across
    /// nearby nav nodes around the destination. Each creature attack-moves to
    /// its assigned spread position.
    pub(crate) fn command_group_attack_move(
        &mut self,
        creature_ids: &[CreatureId],
        destination: VoxelCoord,
        events: &mut Vec<SimEvent>,
    ) {
        if creature_ids.len() <= 1 {
            if let Some(&cid) = creature_ids.first() {
                self.command_attack_move(cid, destination, events);
            }
            return;
        }
        let destinations = self.compute_spread_assignments(creature_ids, destination);
        for (cid, dest) in destinations {
            self.command_attack_move(cid, dest, events);
        }
    }

    /// Execute the AttackMove task behavior. Called from `execute_task_behavior`
    /// when the task kind is AttackMove.
    ///
    /// Behavior loop per activation:
    /// 1. If `target_creature` is `Some(id)` — pursue and engage.
    ///    - Dead/missing target → clear target, restore location, fall through.
    ///    - Alive → try combat, walk toward target on failure to attack.
    ///    - Path failure → disengage (clear target, restore location).
    /// 2. Scan for hostiles. If found, pick nearest and engage.
    /// 3. Walk toward destination.
    /// 4. If at destination with no target, complete task.
    pub(crate) fn execute_attack_move(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        task_location: NavNodeId,
        current_node: NavNodeId,
        events: &mut Vec<SimEvent>,
    ) {
        let move_data = match self.task_attack_move_data(task_id) {
            Some(d) => d,
            None => {
                self.complete_task(task_id);
                self.schedule_reactivation(creature_id);
                return;
            }
        };
        let destination = move_data.destination;

        // Get the destination nav node (for restoring location after disengage).
        let dest_nav_node = match self.nav_graph.find_nearest_node(destination) {
            Some(n) => n,
            None => {
                self.complete_task(task_id);
                self.schedule_reactivation(creature_id);
                return;
            }
        };

        // Step 1: Check current engagement target.
        let target_creature = self.db.tasks.get(&task_id).and_then(|t| t.target_creature);

        if let Some(target_id) = target_creature {
            let target_alive = self
                .db
                .creatures
                .get(&target_id)
                .is_some_and(|c| c.vital_status == VitalStatus::Alive);
            if !target_alive {
                // Target dead/missing — disengage and fall through to scan.
                self.disengage_attack_move(task_id, dest_nav_node, creature_id);
            } else {
                // Target alive — try combat.
                if self.try_combat_against_target(creature_id, target_id, events) {
                    return;
                }

                // Combat failed — try repositioning for a clear ranged shot.
                if let Some(creature) = self.db.creatures.get(&creature_id) {
                    let species = creature.species;
                    if let Some(edge_idx) = self.find_ranged_reposition_edge(creature_id, target_id)
                    {
                        self.move_one_step(creature_id, species, edge_idx);
                        return;
                    }
                }

                // Not in combat range — walk toward target.
                if current_node != task_location {
                    self.walk_toward_attack_move_target(
                        creature_id,
                        task_id,
                        task_location,
                        current_node,
                        dest_nav_node,
                        events,
                    );
                    return;
                }
                // At target location but not in range — re-activate next tick.
                self.event_queue.schedule(
                    self.tick + 1,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
                return;
            }
        }

        // Step 2: Scan for hostiles (only when not engaged).
        let target_creature_after = self.db.tasks.get(&task_id).and_then(|t| t.target_creature);
        if target_creature_after.is_none()
            && let Some(creature) = self.db.creatures.get(&creature_id)
        {
            let species = creature.species;
            let species_data = &self.species_table[&species];
            let detection_range_sq = species_data.hostile_detection_range_sq;

            if detection_range_sq > 0 {
                let targets = self.detect_hostile_targets(
                    creature_id,
                    species,
                    creature.position,
                    creature.civ_id,
                    detection_range_sq,
                );

                if let Some(&(nearest_id, nearest_node)) = targets.first() {
                    // Engage nearest hostile. Use the target creature's actual
                    // position — the NavNodeId may be a placeholder (u32::MAX)
                    // for flying creatures with no nearby nav node.
                    let nearest_pos = self
                        .db
                        .creatures
                        .get(&nearest_id)
                        .map(|c| c.position)
                        .unwrap_or_else(|| self.nav_graph.node(nearest_node).position);
                    if let Some(mut t) = self.db.tasks.get(&task_id) {
                        t.target_creature = Some(nearest_id);
                        t.location = nearest_pos;
                        let _ = self.db.tasks.update_no_fk(t);
                    }
                    let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                        c.path = None;
                    });
                    // Re-activate immediately to start pursuit.
                    self.event_queue.schedule(
                        self.tick + 1,
                        ScheduledEventKind::CreatureActivation { creature_id },
                    );
                    return;
                }
            }
        }

        // Step 3 & 4: Walk toward destination, complete on arrival.
        // Re-read current node since we may have moved.
        let current = self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| self.graph_for_species(c.species).node_at(c.position))
            .unwrap_or(current_node);

        if current == dest_nav_node {
            // At destination with no target — complete.
            self.complete_task(task_id);
            self.schedule_reactivation(creature_id);
            return;
        }

        // Walk toward destination.
        self.walk_toward_task(creature_id, dest_nav_node, current, events);
    }

    /// Disengage from a combat target during attack-move: clear target_creature
    /// and restore task location to the destination nav node.
    pub(crate) fn disengage_attack_move(
        &mut self,
        task_id: TaskId,
        dest_nav_node: NavNodeId,
        creature_id: CreatureId,
    ) {
        let dest_pos = self.nav_graph.node(dest_nav_node).position;
        if let Some(mut t) = self.db.tasks.get(&task_id) {
            t.target_creature = None;
            t.location = dest_pos;
            let _ = self.db.tasks.update_no_fk(t);
        }
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.path = None;
        });
    }

    /// Walk toward an attack-move engagement target. On pathfinding failure,
    /// immediately disengages (unlike AttackTarget which retries).
    pub(crate) fn walk_toward_attack_move_target(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        task_location: NavNodeId,
        current_node: NavNodeId,
        dest_nav_node: NavNodeId,
        _events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let species = creature.species;
        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);

        // Check for cached path.
        let cn = graph.node_at(creature.position);
        let next_step = if let Some(ref path) = creature.path {
            if let Some(&next_pos) = path.remaining_positions.first() {
                let dest_node = graph.node_at(next_pos);
                let edge_idx =
                    dest_node.and_then(|dn| cn.and_then(|cn| graph.find_edge_to(cn, dn)));
                match (edge_idx, dest_node) {
                    (Some(ei), Some(dn)) => Some((ei, dn)),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        };

        let walk_tpv = species_data.walk_ticks_per_voxel;
        let climb_tpv = species_data.climb_ticks_per_voxel;
        let wood_ladder_tpv = species_data.wood_ladder_tpv;
        let rope_ladder_tpv = species_data.rope_ladder_tpv;

        let (edge_idx, dest_node) = if let Some(step) = next_step {
            step
        } else {
            // Compute a new path.
            let path_result = if let Some(ref allowed) = species_data.allowed_edge_types {
                pathfinding::astar_filtered(
                    graph,
                    current_node,
                    task_location,
                    walk_tpv,
                    climb_tpv,
                    wood_ladder_tpv,
                    rope_ladder_tpv,
                    allowed,
                )
            } else {
                pathfinding::astar(
                    graph,
                    current_node,
                    task_location,
                    walk_tpv,
                    climb_tpv,
                    wood_ladder_tpv,
                    rope_ladder_tpv,
                )
            };

            let path_result = match path_result {
                Some(r) if r.nodes.len() >= 2 => r,
                _ => {
                    // Path failure during engagement — immediately disengage.
                    self.disengage_attack_move(task_id, dest_nav_node, creature_id);
                    self.event_queue.schedule(
                        self.tick + 1,
                        ScheduledEventKind::CreatureActivation { creature_id },
                    );
                    return;
                }
            };

            let first_edge = path_result.edge_indices[0];
            let first_dest = path_result.nodes[1];

            let remaining_positions: Vec<VoxelCoord> = path_result.nodes[1..]
                .iter()
                .map(|&nid| graph.node(nid).position)
                .collect();
            let _ = self
                .db
                .creatures
                .modify_unchecked(&creature_id, |creature| {
                    creature.path = Some(CreaturePath {
                        remaining_positions,
                    });
                });

            (first_edge, first_dest)
        };

        // Move one edge.
        let graph = self.graph_for_species(species);
        let edge = graph.edge(edge_idx);
        let dest_pos = graph.node(dest_node).position;

        // Voxel exclusion: reject move if destination is hostile-occupied.
        let footprint = self.species_table[&species].footprint;
        if self.destination_blocked_by_hostile(creature_id, dest_pos, footprint) {
            // Invalidate cached path so we repath on retry.
            let _ = self
                .db
                .creatures
                .modify_unchecked(&creature_id, |creature| {
                    creature.path = None;
                });
            self.event_queue.schedule(
                self.tick + self.config.voxel_exclusion_retry_ticks,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return;
        }

        let tpv = match edge.edge_type {
            crate::nav::EdgeType::TrunkClimb | crate::nav::EdgeType::GroundToTrunk => {
                climb_tpv.unwrap_or(walk_tpv)
            }
            crate::nav::EdgeType::WoodLadderClimb => wood_ladder_tpv.unwrap_or(walk_tpv),
            crate::nav::EdgeType::RopeLadderClimb => rope_ladder_tpv.unwrap_or(walk_tpv),
            _ => walk_tpv,
        };
        let delay = (edge.distance as u64 * tpv)
            .div_ceil(crate::nav::DIST_SCALE as u64)
            .max(1);

        let old_pos = self.db.creatures.get(&creature_id).unwrap().position;
        let tick = self.tick;
        let _ = self
            .db
            .creatures
            .modify_unchecked(&creature_id, |creature| {
                creature.position = dest_pos;
                creature.action_kind = ActionKind::Move;
                creature.next_available_tick = Some(tick + delay);
                if let Some(ref mut path) = creature.path
                    && !path.remaining_positions.is_empty()
                {
                    path.remaining_positions.remove(0);
                }
            });

        self.update_creature_spatial_index(creature_id, species, old_pos, dest_pos);

        let move_action = MoveAction {
            creature_id,
            move_from: old_pos,
            move_to: dest_pos,
            move_start_tick: tick,
        };
        let _ = self.db.move_actions.remove_no_fk(&creature_id);
        self.db.move_actions.insert_no_fk(move_action).unwrap();

        self.event_queue.schedule(
            self.tick + delay,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Execute combat at the target's location — try melee, then ranged, then
    /// re-activate. Used by both AttackTarget and AttackMove when the creature
    /// has arrived at or is close to the target.
    pub(crate) fn execute_combat_at_location(
        &mut self,
        creature_id: CreatureId,
        target_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) {
        // Check if target is still alive.
        let target = match self.db.creatures.get(&target_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };

        let attacker = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let species = attacker.species;
        let species_data = &self.species_table[&species];
        let attacker_pos = attacker.position;
        let attacker_footprint = species_data.footprint;
        let target_pos = target.position;
        let target_footprint = self.species_table[&target.species].footprint;
        let max_melee_range = self.max_melee_range_sq(creature_id);

        // Try melee if in range (considers both bare hands and melee weapons).
        if max_melee_range > 0
            && in_melee_range(
                attacker_pos,
                attacker_footprint,
                target_pos,
                target_footprint,
                max_melee_range,
            )
        {
            if self.try_melee_strike(creature_id, target_id, events) {
                return;
            }
            // On cooldown — wait.
            if let Some(next_tick) = self
                .db
                .creatures
                .get(&creature_id)
                .and_then(|c| c.next_available_tick)
            {
                self.event_queue.schedule(
                    next_tick,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
            } else {
                self.event_queue.schedule(
                    self.tick + 100,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
            }
            return;
        }

        // Try ranged if have bow + arrow.
        if self.try_shoot_arrow(creature_id, target_id, events) {
            return;
        }

        // Ranged failed (possibly friendly-fire blocked) — try alternate targets.
        if self.try_shoot_any_clear_target(creature_id, events) {
            return;
        }

        // All shots blocked — try repositioning to a neighboring nav node
        // that provides a clear shot.
        if let Some(creature) = self.db.creatures.get(&creature_id) {
            let species = creature.species;
            if let Some(edge_idx) = self.find_ranged_reposition_edge(creature_id, target_id) {
                self.move_one_step(creature_id, species, edge_idx);
                return;
            }
        }

        // If we're at the same node but out of melee range (shouldn't happen
        // normally), re-activate next tick — the dynamic pursuit system in
        // execute_task_behavior will update the location.
        self.event_queue.schedule(
            self.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Execute the AttackTarget task behavior when the creature has arrived
    /// at the target's location (or is close enough to attack).
    pub(crate) fn execute_attack_target_at_location(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        events: &mut Vec<SimEvent>,
    ) {
        let target_id = match self.task_attack_target_data(task_id) {
            Some(d) => d.target,
            None => {
                self.complete_task(task_id);
                self.schedule_reactivation(creature_id);
                return;
            }
        };
        // Check if target is dead — mission accomplished.
        let target_alive = self
            .db
            .creatures
            .get(&target_id)
            .is_some_and(|c| c.vital_status == VitalStatus::Alive);
        if !target_alive {
            self.complete_task(task_id);
            self.schedule_reactivation(creature_id);
            return;
        }
        self.execute_combat_at_location(creature_id, target_id, events);
    }

    /// Try to perform a combat action (melee or ranged) against a target from
    /// the creature's current position. Returns `true` if an action was taken
    /// or the creature is waiting on cooldown. The caller is responsible for
    /// any side effects like resetting path failure counters.
    pub(crate) fn try_combat_against_target(
        &mut self,
        creature_id: CreatureId,
        target_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let attacker = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return false,
        };
        let target = match self.db.creatures.get(&target_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };

        let species = attacker.species;
        let species_data = &self.species_table[&species];
        let attacker_pos = attacker.position;
        let attacker_footprint = species_data.footprint;
        let target_pos = target.position;
        let target_footprint = self.species_table[&target.species].footprint;
        let max_melee_range = self.max_melee_range_sq(creature_id);

        // Try melee if in range (considers both bare hands and melee weapons).
        if max_melee_range > 0
            && in_melee_range(
                attacker_pos,
                attacker_footprint,
                target_pos,
                target_footprint,
                max_melee_range,
            )
        {
            if self.try_melee_strike(creature_id, target_id, events) {
                return true;
            }
            // On cooldown — wait.
            if let Some(next_tick) = self
                .db
                .creatures
                .get(&creature_id)
                .and_then(|c| c.next_available_tick)
            {
                self.event_queue.schedule(
                    next_tick,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
            } else {
                self.event_queue.schedule(
                    self.tick + 100,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
            }
            return true;
        }

        // Try ranged at primary target.
        if self.try_shoot_arrow(creature_id, target_id, events) {
            return true;
        }

        // Primary target blocked (possibly by friendly-fire) — try any other
        // hostile target with a clear flight path.
        if self.try_shoot_any_clear_target(creature_id, events) {
            return true;
        }

        false
    }

    /// Try combat against the AttackTarget task's target. Wraps
    /// `try_combat_against_target` and resets path failure counters on contact.
    pub(crate) fn try_attack_target_combat(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let target_id = match self.task_attack_target_data(task_id) {
            Some(d) => d.target,
            None => return false,
        };

        let result = self.try_combat_against_target(creature_id, target_id, events);
        if result {
            // Reset path failure counter on combat contact.
            if let Some(data) = self.task_attack_target_data(task_id) {
                let data_id = data.id;
                let _ = self
                    .db
                    .task_attack_target_data
                    .modify_unchecked(&data_id, |d| {
                        d.path_failures = 0;
                    });
            }
        }
        result
    }

    /// Walk toward the attack target, with retry-limit handling on pathfinding
    /// failure. On failure, increments the path_failures counter; if it exceeds
    /// `attack_path_retry_limit`, cancels the task.
    pub(crate) fn walk_toward_attack_target(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        task_location: NavNodeId,
        current_node: NavNodeId,
        events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let species = creature.species;
        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);

        // Check for cached path.
        let cn = graph.node_at(creature.position);
        let next_step = if let Some(ref path) = creature.path {
            if let Some(&next_pos) = path.remaining_positions.first() {
                let dest_node = graph.node_at(next_pos);
                let edge_idx =
                    dest_node.and_then(|dn| cn.and_then(|cn| graph.find_edge_to(cn, dn)));
                match (edge_idx, dest_node) {
                    (Some(ei), Some(dn)) => Some((ei, dn)),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        };

        let walk_tpv = species_data.walk_ticks_per_voxel;
        let climb_tpv = species_data.climb_ticks_per_voxel;
        let wood_ladder_tpv = species_data.wood_ladder_tpv;
        let rope_ladder_tpv = species_data.rope_ladder_tpv;

        let (edge_idx, dest_node) = if let Some(step) = next_step {
            // Reset failure counter on successful path usage.
            if let Some(data) = self.task_attack_target_data(task_id)
                && data.path_failures > 0
            {
                let data_id = data.id;
                let _ = self
                    .db
                    .task_attack_target_data
                    .modify_unchecked(&data_id, |d| {
                        d.path_failures = 0;
                    });
            }
            step
        } else {
            // Compute a new path.
            let path_result = if let Some(ref allowed) = species_data.allowed_edge_types {
                pathfinding::astar_filtered(
                    graph,
                    current_node,
                    task_location,
                    walk_tpv,
                    climb_tpv,
                    wood_ladder_tpv,
                    rope_ladder_tpv,
                    allowed,
                )
            } else {
                pathfinding::astar(
                    graph,
                    current_node,
                    task_location,
                    walk_tpv,
                    climb_tpv,
                    wood_ladder_tpv,
                    rope_ladder_tpv,
                )
            };

            let path_result = match path_result {
                Some(r) if r.nodes.len() >= 2 => r,
                _ => {
                    // Pathfinding failed — increment counter, check limit.
                    if let Some(data) = self.task_attack_target_data(task_id) {
                        let new_failures = data.path_failures + 1;
                        if new_failures >= self.config.attack_path_retry_limit {
                            // Too many failures — cancel the attack task.
                            self.interrupt_task(creature_id, task_id);
                            self.wander(creature_id, current_node, events);
                            return;
                        }
                        let data_id = data.id;
                        let _ = self
                            .db
                            .task_attack_target_data
                            .modify_unchecked(&data_id, |d| {
                                d.path_failures = new_failures;
                            });
                    }
                    // Retry next activation.
                    self.event_queue.schedule(
                        self.tick + 500,
                        ScheduledEventKind::CreatureActivation { creature_id },
                    );
                    return;
                }
            };

            let first_edge = path_result.edge_indices[0];
            let first_dest = path_result.nodes[1];

            let remaining_positions: Vec<VoxelCoord> = path_result.nodes[1..]
                .iter()
                .map(|&nid| graph.node(nid).position)
                .collect();
            let _ = self
                .db
                .creatures
                .modify_unchecked(&creature_id, |creature| {
                    creature.path = Some(CreaturePath {
                        remaining_positions,
                    });
                });

            (first_edge, first_dest)
        };

        // Move one edge.
        let graph = self.graph_for_species(species);
        let edge = graph.edge(edge_idx);
        let dest_pos = graph.node(dest_node).position;

        // Voxel exclusion: reject move if destination is hostile-occupied.
        let footprint = self.species_table[&species].footprint;
        if self.destination_blocked_by_hostile(creature_id, dest_pos, footprint) {
            let _ = self
                .db
                .creatures
                .modify_unchecked(&creature_id, |creature| {
                    creature.path = None;
                });
            self.event_queue.schedule(
                self.tick + self.config.voxel_exclusion_retry_ticks,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return;
        }

        let tpv = match edge.edge_type {
            crate::nav::EdgeType::TrunkClimb | crate::nav::EdgeType::GroundToTrunk => {
                climb_tpv.unwrap_or(walk_tpv)
            }
            crate::nav::EdgeType::WoodLadderClimb => wood_ladder_tpv.unwrap_or(walk_tpv),
            crate::nav::EdgeType::RopeLadderClimb => rope_ladder_tpv.unwrap_or(walk_tpv),
            _ => walk_tpv,
        };
        let delay = (edge.distance as u64 * tpv)
            .div_ceil(crate::nav::DIST_SCALE as u64)
            .max(1);

        let old_pos = self.db.creatures.get(&creature_id).unwrap().position;
        let tick = self.tick;
        let _ = self
            .db
            .creatures
            .modify_unchecked(&creature_id, |creature| {
                creature.position = dest_pos;
                creature.action_kind = ActionKind::Move;
                creature.next_available_tick = Some(tick + delay);
                if let Some(ref mut path) = creature.path
                    && !path.remaining_positions.is_empty()
                {
                    path.remaining_positions.remove(0);
                }
            });

        self.update_creature_spatial_index(creature_id, species, old_pos, dest_pos);

        let move_action = MoveAction {
            creature_id,
            move_from: old_pos,
            move_to: dest_pos,
            move_start_tick: tick,
        };
        let _ = self.db.move_actions.remove_no_fk(&creature_id);
        self.db.move_actions.insert_no_fk(move_action).unwrap();

        self.event_queue.schedule(
            self.tick + delay,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Handle a creature's death. Sets `vital_status = Dead`, interrupts any
    /// current task, drops all owned inventory as a ground pile, clears
    /// `assigned_home`, and emits a `CreatureDied` event. The creature row
    /// is NOT deleted — it remains in the database for future states (ghost,
    /// spirit, etc.) and to preserve history.
    ///
    /// Heartbeat and activation events for dead creatures are no-ops: the
    /// handlers check `vital_status` and skip rescheduling.
    pub(crate) fn handle_creature_death(
        &mut self,
        creature_id: CreatureId,
        cause: DeathCause,
        events: &mut Vec<SimEvent>,
    ) {
        let (species, position) = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => (c.species, c.position),
            _ => return, // already dead or doesn't exist
        };

        // 1. Interrupt current task (clears action, drops haul items, etc.)
        if let Some(task_id) = self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            self.interrupt_task(creature_id, task_id);
        } else {
            // No task, but still abort any in-progress action.
            self.abort_current_action(creature_id);
        }

        // 2. Drop all inventory items as a ground pile at death position.
        // Use inv_move_items to preserve all item properties (material,
        // quality, durability, enchantment). Then clear owner/reserved_by
        // on the moved stacks.
        let inv_id = self.db.creatures.get(&creature_id).map(|c| c.inventory_id);
        if let Some(inv_id) = inv_id {
            let has_items = !self
                .db
                .item_stacks
                .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
                .is_empty();
            if has_items {
                let pile_id = self.ensure_ground_pile(position);
                let pile_inv_id = self.db.ground_piles.get(&pile_id).unwrap().inventory_id;
                self.inv_move_items(inv_id, pile_inv_id, None, None, None);
                // Clear owner, reserved_by, and equipped_slot on the dead
                // creature's stacks only (filter by owner to avoid clobbering
                // pre-existing items in the same ground pile).
                // Uses update_no_fk because owner and equipped_slot are indexed.
                let pile_stacks: Vec<_> = self
                    .db
                    .item_stacks
                    .by_inventory_id(&pile_inv_id, tabulosity::QueryOpts::ASC);
                for mut stack in pile_stacks {
                    if stack.owner == Some(creature_id) {
                        stack.owner = None;
                        stack.reserved_by = None;
                        stack.equipped_slot = None;
                        let _ = self.db.item_stacks.update_no_fk(stack);
                    }
                }
                self.inv_normalize(pile_inv_id);
            }
        }

        // 3. Remove from spatial index.
        let footprint = self.species_table[&species].footprint;
        Self::deregister_creature_from_index(
            &mut self.spatial_index,
            creature_id,
            position,
            footprint,
        );

        // 4. Clear assigned_home, set vital_status = Dead, hp = 0.
        // Uses update_no_fk because vital_status is #[indexed].
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.assigned_home = None;
            c.vital_status = VitalStatus::Dead;
            c.hp = 0;
            let _ = self.db.creatures.update_no_fk(c);
        }

        // 5. Emit CreatureDied event.
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::CreatureDied {
                creature_id,
                species,
                position,
                cause,
            },
        });

        // 6. Create a notification for the player.
        let creature_name = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let species_str = format!("{:?}", species);
        let cause_suffix = match cause {
            DeathCause::Starvation => " of starvation",
            _ => "",
        };
        let msg = if creature_name.is_empty() {
            format!("A {} has died{}.", species_str, cause_suffix)
        } else {
            format!(
                "{} ({}) has died{}.",
                creature_name, species_str, cause_suffix
            )
        };
        let _ = self
            .db
            .notifications
            .insert_auto_no_fk(|id| crate::db::Notification {
                id,
                tick: self.tick,
                message: msg,
            });
    }

    /// Apply damage to a creature. Positive `amount` reduces HP. If HP
    /// reaches 0 the creature dies via `handle_creature_death`.
    pub(crate) fn apply_damage(
        &mut self,
        creature_id: CreatureId,
        amount: i64,
        events: &mut Vec<SimEvent>,
    ) {
        if amount <= 0 {
            return;
        }
        let should_die = if let Some(mut c) = self.db.creatures.get(&creature_id) {
            if c.vital_status != VitalStatus::Alive {
                return;
            }
            c.hp = (c.hp - amount).max(0);
            let die = c.hp == 0;
            let _ = self.db.creatures.update_no_fk(c);
            die
        } else {
            return;
        };
        if should_die {
            self.handle_creature_death(creature_id, DeathCause::Damage, events);
        }
    }

    /// Heal a creature. Positive `amount` restores HP up to `hp_max`.
    /// No effect on dead creatures.
    pub(crate) fn apply_heal(&mut self, creature_id: CreatureId, amount: i64) {
        if amount <= 0 {
            return;
        }
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            if c.vital_status != VitalStatus::Alive {
                return;
            }
            c.hp = (c.hp + amount).min(c.hp_max);
        });
    }

    /// Compute the maximum melee range a creature can achieve, considering
    /// both its species base range and any melee weapons in inventory.
    /// Used by callers to decide whether to attempt melee before calling
    /// `try_melee_strike()`.
    pub(crate) fn max_melee_range_sq(&self, creature_id: CreatureId) -> i64 {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return 0,
        };
        let species_data = &self.species_table[&creature.species];
        let mut max_range = if species_data.melee_damage > 0 {
            species_data.melee_range_sq
        } else {
            0
        };
        // Check for melee weapons that might extend range.
        let inv_id = creature.inventory_id;
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        for stack in &stacks {
            if stack.quantity == 0 {
                continue;
            }
            let weapon_range = match stack.kind {
                inventory::ItemKind::Spear => self.config.spear_melee_range_sq,
                inventory::ItemKind::Club => self.config.club_melee_range_sq,
                _ => continue,
            };
            if weapon_range > max_range {
                max_range = weapon_range;
            }
        }
        max_range
    }

    /// Whether a creature can perform any melee attack (has species melee
    /// damage > 0, or has a melee weapon in inventory).
    #[allow(dead_code)] // Used by tests; future use for AI weapon preference checks
    pub(crate) fn can_melee(&self, creature_id: CreatureId) -> bool {
        self.max_melee_range_sq(creature_id) > 0
    }

    /// Select the best melee weapon for a given distance-squared to target.
    /// Returns `(base_damage, range_sq, Option<weapon_stack_id>)`.
    /// `None` stack_id means bare hands (species base damage).
    /// Prefers highest damage among weapons whose range covers the distance.
    /// If no weapon reaches, falls back to bare hands if in species range.
    fn select_melee_weapon(
        &self,
        creature_id: CreatureId,
        distance_sq: i64,
    ) -> Option<(i64, i64, Option<ItemStackId>)> {
        let creature = self.db.creatures.get(&creature_id)?;
        let species_data = &self.species_table[&creature.species];
        let inv_id = creature.inventory_id;

        let mut best: Option<(i64, i64, Option<ItemStackId>)> = None;

        // Check inventory for melee weapons.
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        for stack in &stacks {
            if stack.quantity == 0 {
                continue;
            }
            let (weapon_damage, weapon_range) = match stack.kind {
                inventory::ItemKind::Spear => (
                    self.config.spear_base_damage,
                    self.config.spear_melee_range_sq,
                ),
                inventory::ItemKind::Club => (
                    self.config.club_base_damage,
                    self.config.club_melee_range_sq,
                ),
                _ => continue,
            };
            if distance_sq <= weapon_range {
                match &best {
                    Some((best_dmg, _, _)) if *best_dmg >= weapon_damage => {}
                    _ => best = Some((weapon_damage, weapon_range, Some(stack.id))),
                }
            }
        }

        // Consider bare hands if species has melee damage.
        if species_data.melee_damage > 0 && distance_sq <= species_data.melee_range_sq {
            match &best {
                Some((best_dmg, _, _)) if *best_dmg >= species_data.melee_damage => {}
                _ => {
                    best = Some((species_data.melee_damage, species_data.melee_range_sq, None));
                }
            }
        }

        best
    }

    /// Apply random durability damage to a melee weapon after a strike.
    /// Returns `true` if the weapon broke.
    fn apply_melee_weapon_impact_damage(
        &mut self,
        stack_id: ItemStackId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let min = self.config.melee_weapon_impact_damage_min.max(0);
        let max = self.config.melee_weapon_impact_damage_max;
        if min > max || max <= 0 {
            return false;
        }
        let range = (max - min + 1) as u64;
        // Always consume the PRNG even when the rolled damage is 0 — this
        // keeps the PRNG sequence stable regardless of config, which is
        // important for deterministic replay.
        let damage = min + (self.rng.next_u64() % range) as i32;
        if damage <= 0 {
            return false;
        }
        self.inv_damage_item(stack_id, damage, events)
    }

    /// Attempt a melee strike from attacker against target.
    ///
    /// Selects the best melee weapon for the current distance: prefers highest
    /// damage among weapons/bare-hands whose range covers the target. Weapon
    /// damage replaces the species base damage; STR scaling still applies.
    /// On success: starts MeleeStrike action, applies damage, degrades weapon
    /// (if used), emits CreatureDamaged event. Returns true if the strike was
    /// executed.
    pub(crate) fn try_melee_strike(
        &mut self,
        attacker_id: CreatureId,
        target_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        // 1. Both creatures must exist and be alive.
        let attacker = match self.db.creatures.get(&attacker_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };
        let target = match self.db.creatures.get(&target_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };

        // 2. Attacker must be idle.
        if attacker.action_kind != ActionKind::NoAction {
            return false;
        }
        if let Some(next_tick) = attacker.next_available_tick
            && next_tick > self.tick
        {
            return false;
        }

        // 3. Compute distance and select best weapon for this range.
        let species_data = &self.species_table[&attacker.species];
        let attacker_footprint = species_data.footprint;
        let target_footprint = self.species_table[&target.species].footprint;
        let distance_sq = melee_distance_sq(
            attacker.position,
            attacker_footprint,
            target.position,
            target_footprint,
        );

        let (base_damage, _range_sq, weapon_stack_id) =
            match self.select_melee_weapon(attacker_id, distance_sq) {
                Some(w) => w,
                None => return false, // No weapon or bare hands can reach
            };

        let strength = self.trait_int(attacker_id, TraitKind::Strength, 0);
        let raw_damage = crate::stats::apply_stat_multiplier(base_damage, strength);
        let duration = species_data.melee_interval_ticks;

        // Start the action (sets action_kind + next_available_tick, schedules activation).
        self.start_simple_action(attacker_id, ActionKind::MeleeStrike, duration);

        // Degrade melee weapon if one was used.
        if let Some(stack_id) = weapon_stack_id {
            self.apply_melee_weapon_impact_damage(stack_id, events);
        }

        // Apply armor reduction and degrade equipped armor/clothing.
        let damage = self.apply_armor_reduction(target_id, raw_damage, events);

        // Apply damage (handles death if HP reaches 0).
        self.apply_damage(target_id, damage, events);

        // Emit CreatureDamaged event.
        let remaining_hp = self.db.creatures.get(&target_id).map(|c| c.hp).unwrap_or(0);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::CreatureDamaged {
                attacker_id,
                target_id,
                damage,
                remaining_hp,
            },
        });

        true
    }

    /// Attempt a ranged attack: `attacker_id` shoots an arrow at `target_id`.
    ///
    /// Validates: both alive, attacker is idle (NoAction + cooldown elapsed),
    /// attacker has at least one Bow and one Arrow in inventory, the aim solver
    /// finds a feasible trajectory (`hit_tick.is_some()`), and LOS exists
    /// (voxel DDA ray from attacker to target, checking any occupied voxel of
    /// a multi-voxel target).
    ///
    /// On success: consumes one arrow from attacker inventory, spawns a
    /// Projectile entity, sets `ActionKind::Shoot` with `shoot_cooldown_ticks`
    /// duration, emits `ProjectileLaunched` event. Returns true.
    pub(crate) fn try_shoot_arrow(
        &mut self,
        attacker_id: CreatureId,
        target_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        use crate::inventory::ItemKind;
        use crate::projectile::{SubVoxelCoord, compute_aim_velocity};

        // 1. Both creatures must exist and be alive.
        let attacker = match self.db.creatures.get(&attacker_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };
        let target = match self.db.creatures.get(&target_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };

        // 2. Attacker must be idle.
        if attacker.action_kind != ActionKind::NoAction {
            return false;
        }
        if let Some(next_tick) = attacker.next_available_tick
            && next_tick > self.tick
        {
            return false;
        }

        // 3. Attacker must have a Bow and at least one Arrow.
        let inv_id = attacker.inventory_id;
        if self.inv_item_count(inv_id, ItemKind::Bow, inventory::MaterialFilter::Any) == 0 {
            return false;
        }
        if self.inv_item_count(inv_id, ItemKind::Arrow, inventory::MaterialFilter::Any) == 0 {
            return false;
        }

        let attacker_pos = attacker.position;
        let target_pos = target.position;
        let target_species = target.species;
        let target_footprint = self.species_table[&target_species].footprint;

        // 4. LOS check — try each occupied voxel of the target's footprint.
        let mut has_los = false;
        let mut los_target_voxel = target_pos;
        for dy in 0..target_footprint[1] as i32 {
            for dx in 0..target_footprint[0] as i32 {
                for dz in 0..target_footprint[2] as i32 {
                    let tv =
                        VoxelCoord::new(target_pos.x + dx, target_pos.y + dy, target_pos.z + dz);
                    if self.world.has_los(attacker_pos, tv) {
                        has_los = true;
                        los_target_voxel = tv;
                        break;
                    }
                }
                if has_los {
                    break;
                }
            }
            if has_los {
                break;
            }
        }
        if !has_los {
            return false;
        }

        // 5. Aim feasibility — use the aim solver to check if a trajectory exists.
        // Strength modifies arrow speed (stronger draw → faster arrow).
        let origin_sub = SubVoxelCoord::from_voxel_center(attacker_pos);
        let attacker_str = self.trait_int(attacker_id, TraitKind::Strength, 0);
        let speed = crate::stats::apply_stat_multiplier(self.config.arrow_base_speed, attacker_str);
        let gravity = self.config.arrow_gravity;
        let aim = compute_aim_velocity(origin_sub, los_target_voxel, speed, gravity, 5, 5000);
        if aim.hit_tick.is_none() {
            return false;
        }

        // 5a-ii. Apply DEX-based aim deviation to the velocity vector.
        let dexterity = self.trait_int(attacker_id, TraitKind::Dexterity, 0);
        let aim_velocity = crate::stats::apply_dex_deviation(
            &mut self.rng,
            aim.velocity,
            dexterity,
            self.config.arrow_base_deviation_ppm,
        );

        // 5b. Friendly-fire check — reject if a non-hostile creature is in
        // the flight path between the shooter and the target.
        if self
            .flight_path_blocked_by_friendly(
                attacker_id,
                attacker_pos,
                los_target_voxel,
                aim_velocity,
            )
            .is_some()
        {
            return false;
        }

        // 6. All checks passed — consume arrow and fire.
        // Move one arrow from attacker inventory to a new projectile inventory,
        // preserving all properties (durability, material, quality, enchantment).
        let arrow_stack_id = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|s| s.kind == ItemKind::Arrow)
            .unwrap()
            .id;
        let proj_inv_id = self.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
        self.inv_move_stack(arrow_stack_id, 1, proj_inv_id);

        let was_empty = self.db.projectiles.is_empty();

        let _ = self
            .db
            .projectiles
            .insert_auto_no_fk(|id| crate::db::Projectile {
                id,
                shooter: Some(attacker_id),
                inventory_id: proj_inv_id,
                position: origin_sub,
                velocity: aim_velocity,
                prev_voxel: attacker_pos,
                origin_voxel: attacker_pos,
            });

        if was_empty {
            self.event_queue
                .schedule(self.tick + 1, ScheduledEventKind::ProjectileTick);
        }

        // 7. Start the Shoot action (cooldown).
        let duration = self.config.shoot_cooldown_ticks;
        self.start_simple_action(attacker_id, ActionKind::Shoot, duration);

        // 8. Emit ProjectileLaunched event.
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::ProjectileLaunched {
                attacker_id,
                target_id,
            },
        });

        true
    }

    /// Spawn a projectile from `origin` aimed at `target`. Creates an inventory
    /// with a single arrow, computes aim velocity, and inserts the projectile
    /// into SimDb. Schedules a `ProjectileTick` event if this is the first
    /// in-flight projectile (table was empty before this spawn).
    pub(crate) fn spawn_projectile(
        &mut self,
        origin: VoxelCoord,
        target: VoxelCoord,
        shooter_id: Option<CreatureId>,
    ) {
        use crate::projectile::{SubVoxelCoord, compute_aim_velocity};

        let was_empty = self.db.projectiles.is_empty();

        // Create inventory with a single arrow.
        let inv_id = self.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
        self.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 1, None, None);

        // Compute aim velocity (apply shooter's STR to arrow speed if present).
        let origin_sub = SubVoxelCoord::from_voxel_center(origin);
        let base_speed = self.config.arrow_base_speed;
        let speed = if let Some(sid) = shooter_id {
            let str_stat = self.trait_int(sid, TraitKind::Strength, 0);
            crate::stats::apply_stat_multiplier(base_speed, str_stat)
        } else {
            base_speed
        };
        let gravity = self.config.arrow_gravity;
        let aim = compute_aim_velocity(origin_sub, target, speed, gravity, 5, 5000);

        // Insert projectile into SimDb.
        let _ = self
            .db
            .projectiles
            .insert_auto_no_fk(|id| crate::db::Projectile {
                id,
                shooter: shooter_id,
                inventory_id: inv_id,
                position: origin_sub,
                velocity: aim.velocity,
                prev_voxel: origin,
                origin_voxel: origin,
            });

        // Schedule ProjectileTick if this is the first in-flight projectile.
        if was_empty {
            self.event_queue
                .schedule(self.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    /// Advance all in-flight projectiles by one tick. For each projectile:
    /// save prev_voxel, apply gravity+velocity (symplectic Euler via
    /// `ballistic_step`), check bounds, check solid voxel collision, check
    /// creature collision. Resolved projectiles are removed from the table.
    /// Reschedules itself for tick+1 if projectiles remain.
    pub(crate) fn process_projectile_tick(&mut self, events: &mut Vec<SimEvent>) {
        use crate::projectile::ballistic_step;

        let gravity = self.config.arrow_gravity;
        let (world_sx, world_sy, world_sz) = self.config.world_size;

        // Collect all projectile IDs to iterate (can't mutate DB while iterating).
        let projectile_ids: Vec<ProjectileId> =
            self.db.projectiles.iter_all().map(|p| p.id).collect();

        for proj_id in projectile_ids {
            let proj = match self.db.projectiles.get(&proj_id) {
                Some(p) => p,
                None => continue, // already removed
            };

            // Step 1: save current voxel as prev_voxel.
            let current_voxel = proj.position.to_voxel();

            // Step 2-3: symplectic Euler (gravity, then velocity).
            let (new_pos, new_vel) = ballistic_step(proj.position, proj.velocity, gravity);

            // Step 7 (early): bounds check on i64 BEFORE casting to i32.
            let vx = new_pos.x >> crate::projectile::SUB_VOXEL_SHIFT;
            let vy = new_pos.y >> crate::projectile::SUB_VOXEL_SHIFT;
            let vz = new_pos.z >> crate::projectile::SUB_VOXEL_SHIFT;

            if vx < 0
                || vy < 0
                || vz < 0
                || vx >= world_sx as i64
                || vy >= world_sy as i64
                || vz >= world_sz as i64
            {
                // Out of bounds — despawn silently (arrow lost).
                self.remove_projectile(proj_id);
                continue;
            }

            // Step 4: determine containing voxel (safe to cast now).
            let new_voxel = new_pos.to_voxel();

            // Step 5: solid voxel check.
            if self.world.in_bounds(new_voxel) && self.world.get(new_voxel).is_solid() {
                // Surface hit — transfer arrow to ground pile at prev_voxel.
                self.resolve_projectile_surface_hit(proj_id, current_voxel, events);
                continue;
            }

            // Step 6: creature check via spatial index.
            // Near the origin (Chebyshev distance ≤ 1), skip non-hostile
            // creatures — projectiles don't hit squad-mates standing next to
            // the shooter. Hostile creatures near origin are still valid hits
            // (point-blank shots). Beyond origin area, all alive creatures
            // are candidates.
            let (origin_voxel, shooter_id) = self
                .db
                .projectiles
                .get(&proj_id)
                .map(|p| (p.origin_voxel, p.shooter))
                .unwrap_or((new_voxel, None));
            let near_origin = {
                let dx = (new_voxel.x - origin_voxel.x).abs();
                let dy = (new_voxel.y - origin_voxel.y).abs();
                let dz = (new_voxel.z - origin_voxel.z).abs();
                dx <= 1 && dy <= 1 && dz <= 1
            };
            let creatures_here = self.creatures_at_voxel(new_voxel).to_vec();
            if !creatures_here.is_empty() {
                // Filter to alive creatures, skipping non-hostiles near origin.
                // Sort is preserved from spatial index for determinism.
                let candidates: Vec<CreatureId> = creatures_here
                    .into_iter()
                    .filter(|cid| {
                        let alive = self
                            .db
                            .creatures
                            .get(cid)
                            .is_some_and(|c| c.vital_status == VitalStatus::Alive);
                        if !alive {
                            return false;
                        }
                        // Near origin: skip non-hostile creatures (squad-mates).
                        if near_origin
                            && let Some(sid) = shooter_id
                            && self.is_non_hostile(sid, *cid)
                        {
                            return false;
                        }
                        true
                    })
                    .collect();

                if !candidates.is_empty() {
                    let target_id = if candidates.len() == 1 {
                        candidates[0]
                    } else {
                        candidates[self.rng.next_u64() as usize % candidates.len()]
                    };

                    self.resolve_projectile_creature_hit(
                        proj_id, target_id, new_vel, new_voxel, events,
                    );
                    continue;
                }
            }

            // No collision — update projectile position and velocity.
            if let Some(mut proj) = self.db.projectiles.get(&proj_id) {
                proj.prev_voxel = current_voxel;
                proj.position = new_pos;
                proj.velocity = new_vel;
                let _ = self.db.projectiles.update_no_fk(proj);
            }
        }

        // Reschedule if projectiles remain.
        if !self.db.projectiles.is_empty() {
            self.event_queue
                .schedule(self.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    /// Resolve a projectile hitting a solid surface. Applies random durability
    /// damage to the arrow; if it survives, transfers it to a ground pile at
    /// `prev_voxel`.
    pub(crate) fn resolve_projectile_surface_hit(
        &mut self,
        proj_id: ProjectileId,
        prev_voxel: VoxelCoord,
        events: &mut Vec<SimEvent>,
    ) {
        let proj = match self.db.projectiles.get(&proj_id) {
            Some(p) => p,
            None => return,
        };
        let proj_inv = proj.inventory_id;

        // Apply random durability damage to the arrow.
        let arrow_broke = self.apply_arrow_impact_damage(proj_inv, events);

        if !arrow_broke {
            // Arrow survived — transfer to ground pile.
            let pile_id = self.ensure_ground_pile(prev_voxel);
            let pile_inv = self.db.ground_piles.get(&pile_id).unwrap().inventory_id;
            self.inv_merge(proj_inv, pile_inv);
        }

        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::ProjectileHitSurface {
                position: prev_voxel,
            },
        });

        self.remove_projectile(proj_id);
    }

    /// Resolve a projectile hitting a creature. Computes base damage from
    /// impact velocity, scales it by the arrow's remaining durability, applies
    /// it, then rolls random durability damage on the arrow. If the arrow
    /// survives, transfers it to a ground pile.
    pub(crate) fn resolve_projectile_creature_hit(
        &mut self,
        proj_id: ProjectileId,
        target_id: CreatureId,
        impact_velocity: SubVoxelVec,
        hit_voxel: VoxelCoord,
        events: &mut Vec<SimEvent>,
    ) {
        let proj = match self.db.projectiles.get(&proj_id) {
            Some(p) => p,
            None => return,
        };
        let shooter_id = proj.shooter;
        let proj_inv = proj.inventory_id;

        // Compute damage from impact speed (momentum-based: linear in speed).
        // REFERENCE_SPEED is arrow_base_speed (the "normal" launch speed).
        let impact_speed_sq = impact_velocity.magnitude_sq();
        let impact_speed = crate::projectile::isqrt_i128(impact_speed_sq);
        let reference_speed = self.config.arrow_base_speed as i128;
        let multiplier = self.config.arrow_damage_multiplier.max(1) as i128;
        let base_damage = if reference_speed > 0 {
            (impact_speed * multiplier / reference_speed).max(1) as i64
        } else {
            1
        };

        // Scale damage by arrow durability: a worn arrow hits softer.
        let raw_damage = self.scale_damage_by_arrow_hp(proj_inv, base_damage);

        // Apply armor reduction and degrade equipped armor/clothing.
        let damage = self.apply_armor_reduction(target_id, raw_damage, events);

        // Apply damage.
        self.apply_damage(target_id, damage, events);

        let remaining_hp = self.db.creatures.get(&target_id).map(|c| c.hp).unwrap_or(0);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::ProjectileHitCreature {
                target_id,
                damage,
                remaining_hp,
                shooter_id,
            },
        });

        // Apply random durability damage to the arrow.
        let arrow_broke = self.apply_arrow_impact_damage(proj_inv, events);

        if !arrow_broke {
            // Arrow survived — transfer to ground pile.
            let pile_id = self.ensure_ground_pile(hit_voxel);
            let pile_inv = self.db.ground_piles.get(&pile_id).unwrap().inventory_id;
            self.inv_merge(proj_inv, pile_inv);
        }

        self.remove_projectile(proj_id);
    }

    /// Remove a projectile and clean up its inventory. The Inventory FK
    /// has no cascade from projectile→inventory (the FK direction is
    /// projectile.inventory_id → inventories), so we must explicitly
    /// remove the inventory and its item stacks.
    pub(crate) fn remove_projectile(&mut self, proj_id: ProjectileId) {
        if let Some(proj) = self.db.projectiles.get(&proj_id) {
            let inv_id = proj.inventory_id;
            // Remove projectile first (it references the inventory).
            let _ = self.db.projectiles.remove_no_fk(&proj_id);
            // Remove any remaining item stacks in the inventory.
            let stacks: Vec<ItemStackId> = self
                .db
                .item_stacks
                .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
                .iter()
                .map(|s| s.id)
                .collect();
            for stack_id in stacks {
                let _ = self.db.item_stacks.remove_no_fk(&stack_id);
            }
            // Remove the inventory row itself.
            let _ = self.db.inventories.remove_no_fk(&inv_id);
        }
    }

    /// Roll random durability damage for an arrow on impact and apply it.
    /// Returns `true` if the arrow broke, `false` if it survived (or there
    /// was no arrow in the inventory).
    fn apply_arrow_impact_damage(
        &mut self,
        proj_inv: InventoryId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let min = self.config.arrow_impact_damage_min.max(0);
        let max = self.config.arrow_impact_damage_max;
        if min > max || max <= 0 {
            return false;
        }
        let range = (max - min + 1) as u64;
        // Always consume the PRNG even when the rolled damage is 0 — this
        // keeps the PRNG sequence stable regardless of config, which is
        // important for deterministic replay.
        let damage = min + (self.rng.next_u64() % range) as i32;
        if damage <= 0 {
            return false;
        }
        // Find the arrow stack in the projectile inventory.
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&proj_inv, tabulosity::QueryOpts::ASC);
        let arrow_stack_id = match stacks.iter().find(|s| s.kind == inventory::ItemKind::Arrow) {
            Some(s) => s.id,
            None => return false,
        };
        self.inv_damage_item(arrow_stack_id, damage, events)
    }

    /// Scale creature damage by the arrow's remaining durability. A full-HP
    /// arrow deals full damage; a 2/3-HP arrow deals 2/3 damage, etc.
    /// Indestructible arrows (`max_hp == 0`) always deal full damage.
    /// Minimum 1 damage.
    fn scale_damage_by_arrow_hp(&self, proj_inv: InventoryId, base_damage: i64) -> i64 {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&proj_inv, tabulosity::QueryOpts::ASC);
        let arrow = match stacks.iter().find(|s| s.kind == inventory::ItemKind::Arrow) {
            Some(s) => s,
            None => return base_damage,
        };
        if arrow.max_hp <= 0 {
            return base_damage;
        }
        (base_damage * arrow.current_hp as i64 / arrow.max_hp as i64).max(1)
    }

    /// Compute the total effective armor value for a creature by summing
    /// `effective_armor_value` across all equipped items. Non-armor clothing
    /// returns 0 from `effective_armor_value` so it's safe to check all slots.
    fn compute_creature_armor(&self, creature_id: CreatureId) -> i64 {
        let inv_id = match self.db.creatures.get(&creature_id) {
            Some(c) => c.inventory_id,
            None => return 0,
        };
        let mut total: i64 = 0;
        let params = inventory::ArmorParams {
            worn_pct: self.config.durability_worn_pct,
            damaged_pct: self.config.durability_damaged_pct,
            worn_penalty: self.config.armor_worn_penalty,
            damaged_penalty: self.config.armor_damaged_penalty,
        };
        for slot in [
            inventory::EquipSlot::Head,
            inventory::EquipSlot::Torso,
            inventory::EquipSlot::Legs,
            inventory::EquipSlot::Feet,
            inventory::EquipSlot::Hands,
        ] {
            if let Some(stack) = self.inv_equipped_in_slot(inv_id, slot) {
                total += inventory::effective_armor_value(
                    stack.kind,
                    stack.material,
                    stack.current_hp,
                    stack.max_hp,
                    params,
                ) as i64;
            }
        }
        total
    }

    /// Apply armor/clothing degradation after a creature takes a hit.
    /// Picks a random body location (weighted), and if something is equipped
    /// there, rolls durability damage based on whether the hit penetrated.
    ///
    /// - `raw_damage`: damage before armor reduction
    /// - `total_armor`: the creature's total effective armor at time of hit
    fn apply_armor_degradation(
        &mut self,
        creature_id: CreatureId,
        raw_damage: i64,
        total_armor: i64,
        events: &mut Vec<SimEvent>,
    ) {
        let inv_id = match self.db.creatures.get(&creature_id) {
            Some(c) => c.inventory_id,
            None => return,
        };

        // 1. Pick a body location via weighted random.
        // Weights: [Torso, Legs, Head, Feet, Hands]. Clamp negatives to 0.
        let weights = self.config.armor_degrade_location_weights.map(|w| w.max(0));
        let total_weight: i32 = weights.iter().fold(0i32, |a, &b| a.saturating_add(b));
        if total_weight <= 0 {
            return;
        }
        let roll = (self.rng.next_u64() % total_weight as u64) as i32;
        let slots = [
            inventory::EquipSlot::Torso,
            inventory::EquipSlot::Legs,
            inventory::EquipSlot::Head,
            inventory::EquipSlot::Feet,
            inventory::EquipSlot::Hands,
        ];
        let mut cumulative = 0;
        let mut chosen_slot = slots[0];
        for (i, &w) in weights.iter().enumerate() {
            cumulative += w;
            if roll < cumulative {
                chosen_slot = slots[i];
                break;
            }
        }

        // 2. Check what's equipped there.
        let stack = match self.inv_equipped_in_slot(inv_id, chosen_slot) {
            Some(s) => s,
            None => return, // Nothing equipped — no degradation.
        };

        // 3. Compute degradation amount.
        let penetrated = raw_damage > total_armor;
        let degrade_amount = if penetrated {
            let min_damage = self.config.armor_min_damage.max(0);
            let penetrating_damage = (raw_damage - total_armor).max(min_damage) - min_damage;
            // Roll in [0, 2 * penetrating_damage].
            if penetrating_damage <= 0 {
                // Edge case: armor exactly equals raw_damage - min_damage.
                // Still a penetrating hit, treat like 0 penetration.
                0
            } else {
                // Clamp to i32 range before arithmetic to avoid overflow.
                let clamped = penetrating_damage.min(i32::MAX as i64 / 2);
                let range = (2 * clamped + 1) as u64;
                (self.rng.next_u64() % range) as i32
            }
        } else {
            // Non-penetrating: 1-in-N chance of losing 1 HP.
            let recip = self.config.armor_non_penetrating_degrade_chance_recip;
            if recip > 0 && self.rng.next_u64().is_multiple_of(recip as u64) {
                1
            } else {
                0
            }
        };

        if degrade_amount > 0 {
            self.inv_damage_item(stack.id, degrade_amount, events);
        }
    }

    /// Apply armor reduction to raw damage and return the effective damage.
    /// Also triggers armor degradation on the target.
    fn apply_armor_reduction(
        &mut self,
        target_id: CreatureId,
        raw_damage: i64,
        events: &mut Vec<SimEvent>,
    ) -> i64 {
        let total_armor = self.compute_creature_armor(target_id);
        let min_damage = self.config.armor_min_damage.max(0);
        let effective_damage = (raw_damage - total_armor).max(min_damage);

        // Degrade armor/clothing at the target location.
        self.apply_armor_degradation(target_id, raw_damage, total_armor, events);

        effective_damage
    }

    /// A civ becomes aware of another civ. Creates a CivRelationship row.
    /// No-op if the relationship already exists.
    pub(crate) fn discover_civ(
        &mut self,
        civ_id: CivId,
        discovered_civ: CivId,
        initial_opinion: CivOpinion,
    ) {
        // Check that both civs exist.
        if self.db.civilizations.get(&civ_id).is_none()
            || self.db.civilizations.get(&discovered_civ).is_none()
        {
            return;
        }
        // Check if already aware (lookup-before-insert for compound uniqueness).
        let already_aware = self
            .db
            .civ_relationships
            .by_from_civ(&civ_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|r| r.to_civ == discovered_civ);
        if already_aware {
            return;
        }
        let _ = self
            .db
            .civ_relationships
            .insert_auto_no_fk(|id| crate::db::CivRelationship {
                id,
                from_civ: civ_id,
                to_civ: discovered_civ,
                opinion: initial_opinion,
            });
    }

    /// Get the player-controlled civ's known civilizations for the elfcyclopedia.
    /// Returns a list of (civ, our_opinion, their_opinion) tuples.
    pub fn get_known_civs(&self) -> Vec<(crate::db::Civilization, CivOpinion, Option<CivOpinion>)> {
        let player_civ_id = match self.player_civ_id {
            Some(id) => id,
            None => return Vec::new(),
        };

        // Collect relationship data first to avoid overlapping borrows.
        let our_rels: Vec<(CivId, CivOpinion)> = self
            .db
            .civ_relationships
            .by_from_civ(&player_civ_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .map(|r| (r.to_civ, r.opinion))
            .collect();

        let mut result = Vec::new();
        for (to_civ, our_opinion) in our_rels {
            let civ = match self.db.civilizations.get(&to_civ) {
                Some(c) => c.clone(),
                None => continue,
            };
            // Check if they know about us.
            let their_opinion = self
                .db
                .civ_relationships
                .by_from_civ(&to_civ, tabulosity::QueryOpts::ASC)
                .into_iter()
                .find(|r| r.to_civ == player_civ_id)
                .map(|r| r.opinion);

            result.push((civ, our_opinion, their_opinion));
        }
        result
    }

    /// Update a civ's opinion of another civ. No-op if unaware.
    pub(crate) fn set_civ_opinion(
        &mut self,
        civ_id: CivId,
        target_civ: CivId,
        opinion: CivOpinion,
    ) {
        let rel_id = self
            .db
            .civ_relationships
            .by_from_civ(&civ_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|r| r.to_civ == target_civ)
            .map(|r| r.id);
        if let Some(id) = rel_id {
            let _ = self
                .db
                .civ_relationships
                .modify_unchecked(&id, |r| r.opinion = opinion);
        }
    }

    /// Resolve the effective `EngagementStyle` for a creature.
    ///
    /// - **Civ creatures**: returns the military group's `engagement_style`
    ///   (explicit group or default civilian group).
    /// - **Non-civ creatures**: returns the species' `engagement_style`.
    pub(crate) fn resolve_engagement_style(
        &self,
        creature_id: CreatureId,
    ) -> crate::species::EngagementStyle {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return crate::species::EngagementStyle::default(),
        };

        if let Some(civ_id) = creature.civ_id {
            if let Some(group_id) = creature.military_group
                && let Some(group) = self.db.military_groups.get(&group_id)
            {
                return group.engagement_style;
            }
            // Implicit civilian — look up the civ's default civilian group.
            if let Some(group) = self
                .db
                .military_groups
                .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
                .into_iter()
                .find(|g| g.is_default_civilian)
            {
                return group.engagement_style;
            }
        }

        // Non-civ or fallback: use species default.
        self.species_table[&creature.species].engagement_style
    }

    /// Look up the player's civ id from the cached field on `SimState`.
    pub(crate) fn player_civ(&self) -> Option<CivId> {
        self.player_civ_id
    }

    /// Create a new military group for the player's civ.
    pub(crate) fn create_military_group(&mut self, name: String, events: &mut Vec<SimEvent>) {
        let Some(civ_id) = self.player_civ() else {
            return;
        };
        let group_name = name.clone();
        let result = self
            .db
            .military_groups
            .insert_auto_no_fk(|id| crate::db::MilitaryGroup {
                id,
                civ_id,
                name,
                is_default_civilian: false,
                engagement_style: crate::species::EngagementStyle {
                    weapon_preference: crate::species::WeaponPreference::PreferRanged,
                    ammo_exhausted: crate::species::AmmoExhaustedBehavior::SwitchToMelee,
                    initiative: crate::species::EngagementInitiative::Aggressive,
                    disengage_threshold_pct: 0,
                },
                equipment_wants: Vec::new(),
            });
        if let Ok(group_id) = result {
            self.add_notification(format!("Military group '{group_name}' created."));
            events.push(SimEvent {
                tick: self.tick,
                kind: SimEventKind::MilitaryGroupCreated { group_id },
            });
        }
    }

    /// Delete a non-civilian military group. The FK nullify policy on
    /// `Creature.military_group` automatically unassigns all members.
    pub(crate) fn delete_military_group(
        &mut self,
        group_id: MilitaryGroupId,
        events: &mut Vec<SimEvent>,
    ) {
        let group = match self.db.military_groups.get(&group_id) {
            Some(g) => g.clone(),
            None => return,
        };
        // Cannot delete the civilian group.
        if group.is_default_civilian {
            return;
        }
        // Count alive members and collect all member IDs for manual nullify.
        let members = self
            .db
            .creatures
            .by_military_group(&Some(group_id), tabulosity::QueryOpts::ASC);
        let member_count = members
            .iter()
            .filter(|c| c.vital_status == VitalStatus::Alive)
            .count();
        let member_ids: Vec<CreatureId> = members.iter().map(|c| c.id).collect();

        // Nullify creature.military_group for all members (manual FK nullify).
        // Uses update_no_fk because military_group is an indexed field.
        for cid in member_ids {
            if let Some(mut creature) = self.db.creatures.get(&cid) {
                creature.military_group = None;
                let _ = self.db.creatures.update_no_fk(creature);
            }
        }

        let _ = self.db.military_groups.remove_no_fk(&group_id);

        self.add_notification(format!(
            "Group '{}' disbanded, {} members returned to civilian duty.",
            group.name, member_count
        ));
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::MilitaryGroupDeleted {
                group_id,
                name: group.name,
                member_count,
            },
        });
    }

    /// Reassign a creature to a different military group (or civilian).
    pub(crate) fn reassign_military_group(
        &mut self,
        creature_id: CreatureId,
        group_id: Option<MilitaryGroupId>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        // Must be a civ creature.
        let civ_id = match creature.civ_id {
            Some(id) => id,
            None => return,
        };
        // Must be alive.
        if creature.vital_status != VitalStatus::Alive {
            return;
        }
        // If assigning to a group, validate it belongs to the same civ.
        if let Some(gid) = group_id {
            match self.db.military_groups.get(&gid) {
                Some(g) if g.civ_id == civ_id => {}
                _ => return,
            }
        }
        // Uses update_no_fk because military_group is an indexed field.
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.military_group = group_id;
            let _ = self.db.creatures.update_no_fk(creature);
        }
    }

    /// Rename a military group.
    pub(crate) fn rename_military_group(&mut self, group_id: MilitaryGroupId, name: String) {
        let Some(group) = self.db.military_groups.get(&group_id) else {
            return;
        };
        // Validate it belongs to the player's civ.
        let Some(player_civ) = self.player_civ() else {
            return;
        };
        if group.civ_id != player_civ {
            return;
        }
        let _ = self
            .db
            .military_groups
            .modify_unchecked(&group_id, |g| g.name = name);
    }

    /// Change a military group's engagement style.
    pub(crate) fn set_group_engagement_style(
        &mut self,
        group_id: MilitaryGroupId,
        engagement_style: crate::species::EngagementStyle,
    ) {
        let Some(group) = self.db.military_groups.get(&group_id) else {
            return;
        };
        let Some(player_civ) = self.player_civ() else {
            return;
        };
        if group.civ_id != player_civ {
            return;
        }
        let _ = self
            .db
            .military_groups
            .modify_unchecked(&group_id, |g| g.engagement_style = engagement_style);
    }

    /// Set a military group's equipment wants.
    ///
    /// Rejects the update (with a player notification) if multiple wants
    /// target the same `EquipSlot` — a group can assign at most one
    /// wearable per body slot.
    pub(crate) fn set_group_equipment_wants(
        &mut self,
        group_id: MilitaryGroupId,
        wants: Vec<crate::building::LogisticsWant>,
    ) {
        let Some(group) = self.db.military_groups.get(&group_id) else {
            return;
        };
        let Some(player_civ) = self.player_civ() else {
            return;
        };
        if group.civ_id != player_civ {
            return;
        }

        // Reject if multiple wearable wants share the same equip slot.
        {
            let mut seen_slots: std::collections::BTreeMap<
                crate::inventory::EquipSlot,
                crate::inventory::ItemKind,
            > = std::collections::BTreeMap::new();
            for want in &wants {
                if let Some(slot) = want.item_kind.equip_slot() {
                    if let Some(&existing) = seen_slots.get(&slot) {
                        self.add_notification(format!(
                            "Cannot assign {} — {} slot already assigned to {}",
                            want.item_kind.display_name(),
                            slot.display_name(),
                            existing.display_name(),
                        ));
                        return;
                    }
                    seen_slots.insert(slot, want.item_kind);
                }
            }
        }

        let _ = self
            .db
            .military_groups
            .modify_unchecked(&group_id, |g| g.equipment_wants = wants);
    }

    /// Determine whether a creature should flee from nearby hostiles.
    ///
    /// A creature flees when its resolved `EngagementStyle` says so:
    /// - **Passive initiative**: always flees (never fights).
    /// - **Disengage threshold**: if current HP% ≤ threshold, flee.
    ///   A threshold of 100 means always flee (civilian default).
    ///   A threshold of 0 means never disengage on HP.
    ///
    /// Creatures with a `PlayerCombat`-level task never flee (player override).
    pub(crate) fn should_flee(&self, creature_id: CreatureId, species: Species) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return false,
        };
        let species_data = &self.species_table[&species];

        // Need a detection range to detect threats.
        if species_data.hostile_detection_range_sq == 0 {
            return false;
        }

        // If the creature has a player-directed combat task, it should fight,
        // not flee. The player explicitly ordered this creature to attack.
        if let Some(task_id) = creature.current_task
            && let Some(task) = self.db.tasks.get(&task_id)
        {
            let level = preemption::preemption_level(task.kind_tag, task.origin);
            if level == preemption::PreemptionLevel::PlayerCombat {
                return false;
            }
        }

        // Debug-assert the one-civilian-per-civ invariant for civ creatures.
        #[cfg(debug_assertions)]
        if let Some(civ_id) = creature.civ_id {
            let civilian_count = self
                .db
                .military_groups
                .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
                .iter()
                .filter(|g| g.is_default_civilian)
                .count();
            debug_assert!(
                civilian_count == 1,
                "Expected exactly 1 civilian group for {civ_id}, found {civilian_count}"
            );
        }

        let style = self.resolve_engagement_style(creature_id);

        // Passive creatures always flee (they never initiate or counter-attack).
        if style.initiative == crate::species::EngagementInitiative::Passive {
            return true;
        }

        // Check disengage threshold.
        if style.disengage_threshold_pct >= 100 {
            return true;
        }
        if style.disengage_threshold_pct > 0 {
            let hp_max = species_data.hp_max;
            if hp_max > 0 {
                let hp_pct = (creature.hp * 100 / hp_max) as u8;
                if hp_pct <= style.disengage_threshold_pct {
                    return true;
                }
            }
        }

        false
    }

    /// Perform one greedy retreat step away from the nearest detected threat.
    ///
    /// On each activation, if the creature detects a hostile within its detection
    /// range, it picks the nav neighbor that maximizes squared euclidean distance
    /// from the nearest threat's anchor position. Ties are broken by `NavNodeId`
    /// for determinism. If the creature has a current task, it is interrupted
    /// first.
    ///
    /// Returns `true` if a flee step was taken (or the creature is cornered and
    /// waiting), `false` if no threats are detected (caller should proceed with
    /// normal behavior).
    pub(crate) fn flee_step(
        &mut self,
        creature_id: CreatureId,
        current_node: NavNodeId,
        species: Species,
    ) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return false,
        };
        let pos = creature.position;
        let civ_id = creature.civ_id;
        let detection_range_sq = self.species_table[&species].hostile_detection_range_sq;

        // Detect threats (hostile creatures within range).
        let threats =
            self.detect_hostile_targets(creature_id, species, pos, civ_id, detection_range_sq);

        if threats.is_empty() {
            return false;
        }

        // Interrupt current task if any.
        let current_task = self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task);
        if let Some(task_id) = current_task {
            self.interrupt_task(creature_id, task_id);
        }

        // Nearest threat anchor position (threats are sorted by distance).
        let nearest_threat_pos = match self.db.creatures.get(&threats[0].0) {
            Some(c) => c.position,
            None => return false,
        };

        // Greedy retreat: pick the nav neighbor maximizing distance from the
        // nearest threat. Ties broken by NavNodeId for determinism.
        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);
        let edge_indices = graph.neighbors(current_node);

        if edge_indices.is_empty() {
            // No neighbors (isolated node). Wait and retry.
            self.event_queue.schedule(
                self.tick + 1000,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return true;
        }

        // Filter to eligible edges (respecting allowed_edge_types).
        let eligible_edges: Vec<NavEdgeId> =
            if let Some(ref allowed) = species_data.allowed_edge_types {
                edge_indices
                    .iter()
                    .copied()
                    .filter(|&idx| allowed.contains(&graph.edge(idx).edge_type))
                    .collect()
            } else {
                edge_indices.to_vec()
            };

        if eligible_edges.is_empty() {
            // Cornered — no eligible edges. Wait and retry.
            self.event_queue.schedule(
                self.tick + 1000,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return true;
        }

        // Voxel exclusion: prefer edges not blocked by hostiles. If ALL are
        // blocked, fall back to the full set — a fleeing creature shouldn't
        // freeze in place when completely surrounded.
        let footprint = self.species_table[&species].footprint;
        let unblocked_edges: Vec<NavEdgeId> = eligible_edges
            .iter()
            .copied()
            .filter(|&idx| {
                let dest_pos = graph.node(graph.edge(idx).to).position;
                !self.destination_blocked_by_hostile(creature_id, dest_pos, footprint)
            })
            .collect();
        let all_blocked = unblocked_edges.is_empty();
        let flee_edges = if all_blocked {
            &eligible_edges
        } else {
            &unblocked_edges
        };

        // Pick the edge whose destination maximizes squared distance from threat.
        // Ties broken by NavNodeId (higher = preferred for determinism).
        let best_edge_idx = flee_edges
            .iter()
            .copied()
            .max_by_key(|&idx| {
                let dest_pos = graph.node(graph.edge(idx).to).position;
                let dx = dest_pos.x as i64 - nearest_threat_pos.x as i64;
                let dy = dest_pos.y as i64 - nearest_threat_pos.y as i64;
                let dz = dest_pos.z as i64 - nearest_threat_pos.z as i64;
                let dist_sq = dx * dx + dy * dy + dz * dz;
                (dist_sq, graph.edge(idx).to)
            })
            .unwrap();

        // When all exits are hostile-occupied, skip the exclusion check in
        // move_one_step — a cornered creature should force through rather
        // than freeze in place.
        self.move_one_step_inner(creature_id, species, best_edge_idx, all_blocked);
        true
    }

    /// Hostile AI: detect targets, then attack or pursue based on the
    /// creature's resolved `EngagementStyle`.
    ///
    /// **Weapon preference:**
    /// - `PreferRanged`: try melee if in range, then ranged, then close distance.
    /// - `PreferMelee`: try melee if in range, then close distance; only try
    ///   ranged as a last resort when no path exists.
    ///
    /// **Ammo exhaustion:** if `PreferRanged` and no ammo, either switch to
    /// melee pursuit (`SwitchToMelee`) or disengage (`Flee` — returns false
    /// so the caller falls through to flee/wander).
    ///
    /// Returns `true` if an action was taken (strike or move), `false` if no
    /// target is reachable (caller should fall back to random wander).
    pub(crate) fn hostile_pursue(
        &mut self,
        creature_id: CreatureId,
        current_node: NavNodeId,
        species: Species,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let attacker = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return false,
        };
        let attacker_pos = attacker.position;
        let attacker_civ = attacker.civ_id;
        let detection_range_sq = self.species_table[&species].hostile_detection_range_sq;
        let attacker_footprint = self.species_table[&species].footprint;
        let melee_range_sq = self.max_melee_range_sq(creature_id);

        let style = self.resolve_engagement_style(creature_id);

        // Detect targets at full detection range — defensive creatures can
        // see (and shoot at) hostiles across the full range. The defensive
        // pursuit limit only restricts movement/chasing, not detection.
        let targets = self.detect_hostile_targets(
            creature_id,
            species,
            attacker_pos,
            attacker_civ,
            detection_range_sq,
        );

        if targets.is_empty() {
            return false;
        }

        // --- Always try melee if a target is in range (both preferences). ---
        let melee_target = targets.iter().find(|&&(target_id, _)| {
            let target = match self.db.creatures.get(&target_id) {
                Some(c) => c,
                None => return false,
            };
            let target_footprint = self.species_table[&target.species].footprint;
            in_melee_range(
                attacker_pos,
                attacker_footprint,
                target.position,
                target_footprint,
                melee_range_sq,
            )
        });

        if let Some(&(target_id, _)) = melee_target {
            if self.try_melee_strike(creature_id, target_id, events) {
                return true;
            }
            // Strike failed (cooldown). Wait for cooldown.
            if let Some(next_tick) = self
                .db
                .creatures
                .get(&creature_id)
                .and_then(|c| c.next_available_tick)
            {
                self.event_queue.schedule(
                    next_tick,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
            } else {
                self.event_queue.schedule(
                    self.tick + 100,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
            }
            return true;
        }

        // --- Weapon preference diverges here. ---
        use crate::species::WeaponPreference;

        // For defensive creatures, limit pursuit (movement) to the short
        // defensive range. Melee and ranged attacks use the full target list.
        let pursuit_targets: Vec<(CreatureId, NavNodeId)> =
            if style.initiative == crate::species::EngagementInitiative::Defensive {
                let pursuit_range_sq = self.config.defensive_pursuit_range_sq;
                targets
                    .iter()
                    .filter(|&&(tid, _)| {
                        let tpos = match self.db.creatures.get(&tid) {
                            Some(c) => c.position,
                            None => return false,
                        };
                        let dx = attacker_pos.x as i64 - tpos.x as i64;
                        let dy = attacker_pos.y as i64 - tpos.y as i64;
                        let dz = attacker_pos.z as i64 - tpos.z as i64;
                        dx * dx + dy * dy + dz * dz <= pursuit_range_sq
                    })
                    .copied()
                    .collect()
            } else {
                targets.clone()
            };

        match style.weapon_preference {
            WeaponPreference::PreferRanged => {
                // Try ranged first (full detection range).
                for &(target_id, _) in &targets {
                    if self.try_shoot_arrow(creature_id, target_id, events) {
                        return true;
                    }
                }

                // Ranged failed — check if out of ammo.
                let inv_id = self.db.creatures.get(&creature_id).unwrap().inventory_id;
                let has_bow = self.inv_item_count(
                    inv_id,
                    crate::inventory::ItemKind::Bow,
                    crate::inventory::MaterialFilter::Any,
                ) > 0;
                let has_ammo = self.inv_item_count(
                    inv_id,
                    crate::inventory::ItemKind::Arrow,
                    crate::inventory::MaterialFilter::Any,
                ) > 0;

                if has_bow && !has_ammo {
                    // Ammo exhausted — check behavior.
                    use crate::species::AmmoExhaustedBehavior;
                    match style.ammo_exhausted {
                        AmmoExhaustedBehavior::SwitchToMelee => {
                            // Fall through to close distance (same as melee path).
                        }
                        AmmoExhaustedBehavior::Flee => {
                            // Disengage — return false so caller falls to flee/wander.
                            return false;
                        }
                    }
                }

                // Close distance to nearest target (pursuit range only).
                if pursuit_targets.is_empty() {
                    return false;
                }
                self.pursue_closest_target(creature_id, current_node, species, &pursuit_targets)
            }
            WeaponPreference::PreferMelee => {
                // Try to close distance first (pursuit range only).
                if !pursuit_targets.is_empty()
                    && self.pursue_closest_target(
                        creature_id,
                        current_node,
                        species,
                        &pursuit_targets,
                    )
                {
                    return true;
                }

                // No path or beyond pursuit range — try ranged as last resort.
                for &(target_id, _) in &targets {
                    if self.try_shoot_arrow(creature_id, target_id, events) {
                        return true;
                    }
                }

                false
            }
        }
    }

    /// Pathfind toward the nearest detected target and take one step.
    /// Returns `true` if a step was taken.
    fn pursue_closest_target(
        &mut self,
        creature_id: CreatureId,
        current_node: NavNodeId,
        species: Species,
        targets: &[(CreatureId, NavNodeId)],
    ) -> bool {
        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);

        // Resolve target positions on the attacker's nav graph. Targets may
        // be on a different graph (e.g., 1x1 elf on standard graph vs 2x2
        // troll on large graph), or in the air (flying creatures). We first
        // try find_nearest_node at the target's position; if that fails
        // (e.g., target is a hornet flying above ground), we look for a
        // nav node beneath the target that would put the attacker within
        // melee range.
        let melee_range = self.max_melee_range_sq(creature_id);
        let attacker_footprint = species_data.footprint;
        let target_nodes: Vec<NavNodeId> = targets
            .iter()
            .filter_map(|&(tid, _)| {
                let target = self.db.creatures.get(&tid)?;
                let pos = target.position;
                let target_fp = self.species_table[&target.species].footprint;

                let is_flyer = self.species_table[&target.species]
                    .flight_ticks_per_voxel
                    .is_some();

                // Ground targets: snap to nearest nav node (fast).
                // Flying targets: skip find_nearest_node (O(y²) for high-
                // altitude creatures) and go straight to the bounded
                // melee-reachable search.
                if !is_flyer && let Some(node) = graph.find_nearest_node(pos) {
                    return Some(node);
                }

                // Target has no nearby nav node (flying creature in the
                // air, or ground creature displaced by construction).
                // Search for a nav node where the attacker could stand
                // and be within melee range of the target.
                self.find_melee_reachable_node(
                    graph,
                    pos,
                    target_fp,
                    attacker_footprint,
                    melee_range,
                )
            })
            .collect();
        if target_nodes.is_empty() {
            return false;
        }

        let nearest = crate::pathfinding::dijkstra_nearest(
            graph,
            current_node,
            &target_nodes,
            species_data.walk_ticks_per_voxel,
            species_data.climb_ticks_per_voxel,
            species_data.wood_ladder_tpv,
            species_data.rope_ladder_tpv,
            species_data.allowed_edge_types.as_deref(),
        );

        let target_node = match nearest {
            Some(n) if n == current_node => return false,
            Some(n) => n,
            None => return false,
        };

        let graph = self.graph_for_species(species);
        let path = crate::pathfinding::astar(
            graph,
            current_node,
            target_node,
            species_data.walk_ticks_per_voxel,
            species_data.climb_ticks_per_voxel,
            species_data.wood_ladder_tpv,
            species_data.rope_ladder_tpv,
        );

        let path = match path {
            Some(p) if p.edge_indices.is_empty() => return false,
            Some(p) => p,
            None => return false,
        };

        let first_edge_idx = path.edge_indices[0];
        self.move_one_step(creature_id, species, first_edge_idx);
        true
    }

    /// Find a nav node on `graph` where an attacker standing there would be
    /// within `melee_range_sq` of a target at `target_pos` with `target_fp`
    /// footprint. Used when the target is a flying creature in the air with
    /// no nearby nav node — we search for ground positions beneath/around
    /// the target that let a ground creature reach up and melee.
    ///
    /// Returns `None` if no such node exists (target is too high for any
    /// reachable position to be in melee range).
    fn find_melee_reachable_node(
        &self,
        graph: &crate::nav::NavGraph,
        target_pos: VoxelCoord,
        target_fp: [u8; 3],
        attacker_fp: [u8; 3],
        melee_range_sq: i64,
    ) -> Option<NavNodeId> {
        // Search an expanding box around the target's x,z position.
        // The melee range limits how far horizontally/vertically we need to
        // search: max axis gap ≤ isqrt(melee_range_sq).
        let max_gap = (melee_range_sq as u64).isqrt() as i32 + 1;
        let mut best: Option<(i64, NavNodeId)> = None; // (manhattan_to_target, node_id)

        for dx in -max_gap..=max_gap {
            for dz in -max_gap..=max_gap {
                let col_x = target_pos.x + dx;
                let col_z = target_pos.z + dz;
                // Check all nav nodes in this column.
                for node in graph.nodes_in_column(col_x, col_z) {
                    let node_pos = node.position;
                    let dist = melee_distance_sq(node_pos, attacker_fp, target_pos, target_fp);
                    if dist <= melee_range_sq {
                        // Pick the closest to the target (Manhattan) for
                        // optimal pathfinding.
                        let manhattan = (node_pos.x - target_pos.x).unsigned_abs() as i64
                            + (node_pos.y - target_pos.y).unsigned_abs() as i64
                            + (node_pos.z - target_pos.z).unsigned_abs() as i64;
                        if best.is_none() || manhattan < best.unwrap().0 {
                            best = Some((manhattan, node.id));
                        }
                    }
                }
            }
        }

        best.map(|(_, id)| id)
    }

    /// Detect hostile targets within detection range. Returns a list of
    /// `(CreatureId, NavNodeId)` pairs sorted by squared euclidean distance
    /// (nearest first). **Note:** for flying creatures the `NavNodeId` is a
    /// placeholder (`u32::MAX`) — callers must not dereference it. Use the
    /// creature's position from the DB instead.
    ///
    /// Hostility rules for the initial pass:
    /// - Non-civ aggressive creature (no `civ_id`): targets all living civ
    ///   creatures of a different species. Non-civ aggressives don't attack
    ///   each other.
    /// - Civ creature: targets living creatures whose civ it considers
    ///   `CivOpinion::Hostile`. (Future: also non-civ aggressives.)
    pub(crate) fn detect_hostile_targets(
        &self,
        attacker_id: CreatureId,
        attacker_species: Species,
        attacker_pos: VoxelCoord,
        attacker_civ: Option<CivId>,
        detection_range_sq: i64,
    ) -> Vec<(CreatureId, NavNodeId)> {
        let mut targets: Vec<(CreatureId, NavNodeId, i64)> = Vec::new();
        // Track seen creature IDs to avoid duplicates from multi-voxel footprints.
        let mut seen = std::collections::BTreeSet::new();

        // O(n) scan over all creatures in the spatial index.
        for (&_voxel, creature_ids) in &self.spatial_index {
            for &cid in creature_ids {
                if cid == attacker_id || !seen.insert(cid) {
                    continue;
                }
                let creature = match self.db.creatures.get(&cid) {
                    Some(c) => c,
                    None => continue,
                };
                if creature.vital_status != VitalStatus::Alive {
                    continue;
                }
                // Resolve target's position to a nav node on the attacker's graph
                // (targets may use a different graph, e.g. 1x1 vs 2x2 footprint).
                // Flying creatures may be in open sky with no nearby nav node, so
                // use a placeholder — the NavNodeId is unused by flying callers.
                let is_flyer = self.species_table[&creature.species]
                    .flight_ticks_per_voxel
                    .is_some();
                let node = if is_flyer {
                    NavNodeId(u32::MAX) // placeholder; flying code uses position, not node
                } else {
                    match self
                        .graph_for_species(creature.species)
                        .find_nearest_node(creature.position)
                    {
                        Some(n) => n,
                        None => continue,
                    }
                };

                // Squared 3D euclidean distance (i64 to prevent overflow).
                let dx = attacker_pos.x as i64 - creature.position.x as i64;
                let dy = attacker_pos.y as i64 - creature.position.y as i64;
                let dz = attacker_pos.z as i64 - creature.position.z as i64;
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if dist_sq > detection_range_sq {
                    continue;
                }

                // Check hostility. Non-civ attackers have an additional
                // targeting rule: they only attack civ creatures of a
                // *different* species (don't attack your own kind). This
                // species-difference filter is a targeting constraint, not
                // a diplomatic relation, so it stays here rather than in
                // diplomatic_relation().
                let is_target = if attacker_civ.is_none() {
                    creature.civ_id.is_some() && creature.species != attacker_species
                } else {
                    self.creature_relation(attacker_id, cid).is_hostile()
                };

                if is_target {
                    targets.push((cid, node, dist_sq));
                }
            }
        }

        // Sort by distance (nearest first), break ties by CreatureId for determinism.
        targets.sort_by_key(|&(cid, _, dist)| (dist, cid));

        targets
            .into_iter()
            .map(|(cid, node, _)| (cid, node))
            .collect()
    }

    /// The core diplomatic relation query. Determines how a subject (civ or
    /// creature) feels about an object (civ or creature), based on civilization
    /// relationships and species engagement initiative.
    ///
    /// - **subject** = the perspective holder ("how do I feel about...")
    /// - **object** = the entity being evaluated ("...this thing?")
    ///
    /// Both civ and species are optional on each side. At minimum one should be
    /// present per side; if both are `None` for a side, returns `Neutral`.
    ///
    /// All other relation functions (`creature_relation`, `civ_creature_relation`,
    /// etc.) delegate to this. Combat functions (`is_non_hostile`,
    /// `detect_hostile_targets`) and bridge UI queries (`player_relation`) all
    /// ultimately flow through here.
    pub fn diplomatic_relation(
        &self,
        subject_civ: Option<CivId>,
        subject_species: Option<Species>,
        object_civ: Option<CivId>,
        object_species: Option<Species>,
    ) -> DiplomaticRelation {
        match (subject_civ, object_civ) {
            // Both have civs.
            (Some(s_civ), Some(o_civ)) => {
                if s_civ == o_civ {
                    return DiplomaticRelation::Friendly;
                }
                // Different civs — check CivRelationship from subject → object.
                let hostile = self
                    .db
                    .civ_relationships
                    .by_from_civ(&s_civ, tabulosity::QueryOpts::ASC)
                    .into_iter()
                    .any(|r| r.to_civ == o_civ && r.opinion == CivOpinion::Hostile);
                if hostile {
                    DiplomaticRelation::Hostile
                } else {
                    DiplomaticRelation::Neutral
                }
            }
            // Subject has civ, object doesn't — hostile if object species is aggressive.
            (Some(_), None) => {
                if let Some(species) = object_species
                    && matches!(
                        self.species_table[&species].engagement_style.initiative,
                        crate::species::EngagementInitiative::Aggressive
                    )
                {
                    return DiplomaticRelation::Hostile;
                }
                DiplomaticRelation::Neutral
            }
            // Subject has no civ, object has civ — hostile if subject species is aggressive.
            (None, Some(_)) => {
                if let Some(species) = subject_species
                    && matches!(
                        self.species_table[&species].engagement_style.initiative,
                        crate::species::EngagementInitiative::Aggressive
                    )
                {
                    return DiplomaticRelation::Hostile;
                }
                DiplomaticRelation::Neutral
            }
            // Neither has a civ — non-civ creatures don't fight each other.
            (None, None) => DiplomaticRelation::Neutral,
        }
    }

    /// Directional creature-to-creature relation. "How does `subject` feel
    /// about `object`?" Looks up both creatures' civ and species from the db
    /// and delegates to `diplomatic_relation`.
    ///
    /// Returns `Friendly` if subject == object, `Neutral` if either creature
    /// is missing from the db.
    pub fn creature_relation(&self, subject: CreatureId, object: CreatureId) -> DiplomaticRelation {
        if subject == object {
            return DiplomaticRelation::Friendly;
        }
        let Some(s) = self.db.creatures.get(&subject) else {
            return DiplomaticRelation::Neutral;
        };
        let Some(o) = self.db.creatures.get(&object) else {
            return DiplomaticRelation::Neutral;
        };
        self.diplomatic_relation(s.civ_id, Some(s.species), o.civ_id, Some(o.species))
    }

    /// "How does this creature feel about this civ?" Looks up the creature's
    /// civ and species, then delegates to `diplomatic_relation`.
    ///
    /// Returns `Neutral` if the creature is missing from the db.
    pub fn creature_civ_relation(
        &self,
        subject_creature: CreatureId,
        object_civ: CivId,
    ) -> DiplomaticRelation {
        let Some(c) = self.db.creatures.get(&subject_creature) else {
            return DiplomaticRelation::Neutral;
        };
        self.diplomatic_relation(c.civ_id, Some(c.species), Some(object_civ), None)
    }

    /// "How does this civ feel about this creature?" Looks up the creature's
    /// civ and species, then delegates to `diplomatic_relation`.
    ///
    /// Returns `Neutral` if the creature is missing from the db.
    pub fn civ_creature_relation(
        &self,
        subject_civ: CivId,
        object_creature: CreatureId,
    ) -> DiplomaticRelation {
        let Some(c) = self.db.creatures.get(&object_creature) else {
            return DiplomaticRelation::Neutral;
        };
        self.diplomatic_relation(Some(subject_civ), None, c.civ_id, Some(c.species))
    }

    /// "How does the player's civ feel about this creature?" Sugar for the
    /// most common UI query (minimap dots, selection ring colors).
    ///
    /// Returns `Neutral` if there is no player civ yet.
    pub fn player_relation(&self, creature_id: CreatureId) -> DiplomaticRelation {
        let Some(player_civ) = self.player_civ_id else {
            return DiplomaticRelation::Neutral;
        };
        self.civ_creature_relation(player_civ, creature_id)
    }

    /// Check whether two creatures are non-hostile to each other. Used for
    /// friendly-fire avoidance — an archer should not shoot through a
    /// non-hostile creature. Also used for voxel exclusion (creatures cannot
    /// move into voxels occupied by hostiles).
    ///
    /// Delegates to `creature_relation` — returns `true` when the relation
    /// is not `Hostile`.
    pub(crate) fn is_non_hostile(&self, a: CreatureId, b: CreatureId) -> bool {
        !self.creature_relation(a, b).is_hostile()
    }

    /// Check whether any voxel in the destination footprint is occupied by a
    /// living hostile creature. Used for voxel exclusion — creatures cannot
    /// move into voxels occupied by hostiles. The moving creature itself is
    /// excluded from the check (so a creature doesn't block itself when its
    /// source and destination footprints overlap).
    pub(crate) fn destination_blocked_by_hostile(
        &self,
        creature_id: CreatureId,
        dest_pos: VoxelCoord,
        footprint: [u8; 3],
    ) -> bool {
        for dx in 0..footprint[0] as i32 {
            for dy in 0..footprint[1] as i32 {
                for dz in 0..footprint[2] as i32 {
                    let voxel = VoxelCoord::new(dest_pos.x + dx, dest_pos.y + dy, dest_pos.z + dz);
                    for &occupant_id in self.creatures_at_voxel(voxel) {
                        if occupant_id != creature_id
                            && !self.is_non_hostile(creature_id, occupant_id)
                            && self
                                .db
                                .creatures
                                .get(&occupant_id)
                                .is_some_and(|c| c.vital_status == VitalStatus::Alive)
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Check whether a ballistic flight path from `origin_sub` with `velocity`
    /// would pass through a voxel occupied by a creature that is non-hostile to
    /// `shooter_id`. Used for pre-shot friendly-fire avoidance.
    ///
    /// Simulates the trajectory tick-by-tick (same physics as
    /// `process_projectile_tick`) and checks the spatial index at each new
    /// voxel. Skips the origin voxel and its immediate neighbors for
    /// non-hostile creatures (so squads can stand together), but does NOT
    /// skip them for hostile creatures (point-blank shots are allowed).
    ///
    /// Returns `Some(blocking_id)` if a friendly creature blocks the path,
    /// or `None` if the path is clear.
    pub(crate) fn flight_path_blocked_by_friendly(
        &self,
        shooter_id: CreatureId,
        origin_voxel: VoxelCoord,
        target_voxel: VoxelCoord,
        velocity: crate::projectile::SubVoxelVec,
    ) -> Option<CreatureId> {
        use crate::projectile::{SubVoxelCoord, ballistic_step};

        let gravity = self.config.arrow_gravity;
        let (world_sx, world_sy, world_sz) = self.config.world_size;
        let max_ticks: u32 = 5000;

        let origin_sub = SubVoxelCoord::from_voxel_center(origin_voxel);
        let mut pos = origin_sub;
        let mut vel = velocity;
        let mut prev_voxel = origin_voxel;

        for _tick in 1..=max_ticks {
            (pos, vel) = ballistic_step(pos, vel, gravity);

            // Bounds check (same as process_projectile_tick).
            let vx = pos.x >> crate::projectile::SUB_VOXEL_SHIFT;
            let vy = pos.y >> crate::projectile::SUB_VOXEL_SHIFT;
            let vz = pos.z >> crate::projectile::SUB_VOXEL_SHIFT;
            if vx < 0
                || vy < 0
                || vz < 0
                || vx >= world_sx as i64
                || vy >= world_sy as i64
                || vz >= world_sz as i64
            {
                return None; // Out of bounds — no friendly hit.
            }

            let current_voxel = pos.to_voxel();

            // Solid voxel — stop (arrow would hit surface).
            if self.world.in_bounds(current_voxel) && self.world.get(current_voxel).is_solid() {
                return None;
            }

            // Check creatures at this voxel.
            if current_voxel != prev_voxel {
                let creatures_here = self.creatures_at_voxel(current_voxel);
                for &cid in creatures_here {
                    if cid == shooter_id {
                        continue;
                    }
                    let alive = self
                        .db
                        .creatures
                        .get(&cid)
                        .is_some_and(|c| c.vital_status == VitalStatus::Alive);
                    if !alive {
                        continue;
                    }

                    // Near origin: skip non-hostile (squad members standing
                    // together), but don't skip hostile (point-blank OK).
                    let near_origin = {
                        let dx = (current_voxel.x - origin_voxel.x).abs();
                        let dy = (current_voxel.y - origin_voxel.y).abs();
                        let dz = (current_voxel.z - origin_voxel.z).abs();
                        dx <= 1 && dy <= 1 && dz <= 1
                    };

                    if self.is_non_hostile(shooter_id, cid) {
                        if near_origin {
                            continue; // Skip friendlies near origin.
                        }
                        return Some(cid); // Friendly in the flight path!
                    }
                    // Hostile creature — the arrow would hit them (good), stop.
                    // But only if this is at or past the target, otherwise keep going.
                    // Actually, any hostile hit before the target means the arrow
                    // reaches a valid target, so the path is "not blocked by friendly."
                    return None;
                }
            }

            prev_voxel = current_voxel;

            // Reached target voxel — path is clear.
            if current_voxel == target_voxel {
                return None;
            }

            // Early exit: fell well below target.
            if current_voxel.y < target_voxel.y - 5 && vel.y < 0 {
                return None;
            }
        }

        None // Max ticks — assume clear.
    }

    /// Try to shoot any hostile target that has a clear (no friendly-fire)
    /// flight path. Called when the primary target is blocked by a friendly.
    /// Returns `true` if a shot was taken.
    pub(crate) fn try_shoot_any_clear_target(
        &mut self,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };
        let species = creature.species;
        let pos = creature.position;
        let civ_id = creature.civ_id;
        let detection_range_sq = self.species_table[&species].hostile_detection_range_sq;

        if detection_range_sq <= 0 {
            return false;
        }

        let targets =
            self.detect_hostile_targets(creature_id, species, pos, civ_id, detection_range_sq);

        for (target_id, _node) in targets {
            if self.try_shoot_arrow(creature_id, target_id, events) {
                return true;
            }
        }

        false
    }

    /// Check whether a candidate position would block any nearby friendly
    /// archer's line of fire. Uses a straight-line approximation (cross-product
    /// distance-from-line) since full ballistic trajectory simulation per
    /// friendly would be too expensive.
    ///
    /// Returns `true` if the candidate position is roughly on the firing line
    /// of at least one nearby friendly archer.
    pub(crate) fn position_blocks_friendly_archers(
        &self,
        creature_id: CreatureId,
        candidate_pos: VoxelCoord,
    ) -> bool {
        use crate::inventory::ItemKind;

        if self.db.creatures.get(&creature_id).is_none() {
            return false;
        }

        // Check creatures within a 10-voxel radius (dist_sq <= 100).
        let check_range_sq: i64 = 100;
        let mut seen = std::collections::BTreeSet::new();

        for (&_voxel, creature_ids) in &self.spatial_index {
            for &cid in creature_ids {
                if cid == creature_id || !seen.insert(cid) {
                    continue;
                }
                let other = match self.db.creatures.get(&cid) {
                    Some(c) if c.vital_status == VitalStatus::Alive => c,
                    _ => continue,
                };

                // Must be non-hostile (friendly).
                if !self.is_non_hostile(creature_id, cid) {
                    continue;
                }

                // Must be within check range of the candidate position.
                let dx = candidate_pos.x as i64 - other.position.x as i64;
                let dy = candidate_pos.y as i64 - other.position.y as i64;
                let dz = candidate_pos.z as i64 - other.position.z as i64;
                if dx * dx + dy * dy + dz * dz > check_range_sq {
                    continue;
                }

                // Must have a bow (is an archer).
                let has_bow = self.inv_item_count(
                    other.inventory_id,
                    ItemKind::Bow,
                    crate::inventory::MaterialFilter::Any,
                ) > 0;
                if !has_bow {
                    continue;
                }

                // Must have a target.
                let target_id = match other
                    .current_task
                    .and_then(|tid| self.db.tasks.get(&tid).and_then(|t| t.target_creature))
                {
                    Some(tid) => tid,
                    None => continue,
                };
                let target = match self.db.creatures.get(&target_id) {
                    Some(c) if c.vital_status == VitalStatus::Alive => c,
                    _ => continue,
                };

                // Check if candidate_pos is on the line from other to target.
                // Use cross product: if |AB × AC| / |AB| < threshold, the
                // point C is close to the line AB. We use squared distances
                // to avoid sqrt, and check |AB × AC|² < threshold² * |AB|².
                let ax = other.position.x as i64;
                let ay = other.position.y as i64;
                let az = other.position.z as i64;
                let bx = target.position.x as i64;
                let by = target.position.y as i64;
                let bz = target.position.z as i64;
                let cx = candidate_pos.x as i64;
                let cy = candidate_pos.y as i64;
                let cz = candidate_pos.z as i64;

                let abx = bx - ax;
                let aby = by - ay;
                let abz = bz - az;
                let acx = cx - ax;
                let acy = cy - ay;
                let acz = cz - az;

                // Cross product AB × AC.
                let cross_x = aby * acz - abz * acy;
                let cross_y = abz * acx - abx * acz;
                let cross_z = abx * acy - aby * acx;
                let cross_sq = cross_x * cross_x + cross_y * cross_y + cross_z * cross_z;
                let ab_sq = abx * abx + aby * aby + abz * abz;

                if ab_sq == 0 {
                    continue;
                }

                // Distance from line = |cross| / |AB|.
                // Check: cross_sq < threshold² * ab_sq → within threshold
                // voxels of the line. Use threshold = 1.5 → threshold² = 2.25
                // → cross_sq * 4 < 9 * ab_sq (multiply to stay integer).
                if cross_sq * 4 > 9 * ab_sq {
                    continue;
                }

                // Also check that the candidate is between A and B (not behind
                // or past). Dot product AB · AC should be > 0 and < |AB|².
                let dot = abx * acx + aby * acy + abz * acz;
                if dot > 0 && dot < ab_sq {
                    return true;
                }
            }
        }

        false
    }

    /// Find a neighboring nav node from which the creature has a clear ranged
    /// shot to the target (no friendly-fire blocking). Scores candidates by:
    /// (a) has clear shot (primary), (b) doesn't block nearby elves' lines of
    /// fire (secondary), (c) distance to target. Returns the edge index to
    /// move along, or falls back to a non-blocking position without a clear
    /// shot if no clear-shot candidate exists.
    pub(crate) fn find_ranged_reposition_edge(
        &self,
        creature_id: CreatureId,
        target_id: CreatureId,
    ) -> Option<NavEdgeId> {
        use crate::projectile::{SubVoxelCoord, compute_aim_velocity};

        let creature = self.db.creatures.get(&creature_id)?;
        let species = creature.species;
        let graph = self.graph_for_species(species);
        let current_node = graph.node_at(creature.position)?;

        let target = self.db.creatures.get(&target_id)?;
        if target.vital_status != VitalStatus::Alive {
            return None;
        }
        let target_pos = target.position;
        let target_species = target.species;
        let target_footprint = self.species_table[&target_species].footprint;

        let node = graph.node(current_node);
        let speed = self.config.arrow_base_speed;
        let gravity = self.config.arrow_gravity;

        // Score: (has_clear_shot, doesn't_block_others, -dist_sq)
        // Higher is better for the first two, lower dist is better.
        // best_clear: best candidate with a clear shot.
        // best_fallback: best candidate without a clear shot but not blocking.
        let mut best_clear: Option<(NavEdgeId, bool, i64)> = None; // (edge, non_blocking, dist_sq)
        let mut best_fallback: Option<(NavEdgeId, i64)> = None; // (edge, dist_sq)

        for &edge_idx in &node.edge_indices {
            let edge = graph.edge(edge_idx);
            let neighbor_node = graph.node(edge.to);
            let neighbor_pos = neighbor_node.position;

            let blocks_others = self.position_blocks_friendly_archers(creature_id, neighbor_pos);

            // Score by distance to target.
            let ddx = neighbor_pos.x as i64 - target_pos.x as i64;
            let ddy = neighbor_pos.y as i64 - target_pos.y as i64;
            let ddz = neighbor_pos.z as i64 - target_pos.z as i64;
            let dist_sq = ddx * ddx + ddy * ddy + ddz * ddz;

            // Check LOS from neighbor position to any target footprint voxel.
            let mut has_los = false;
            let mut los_target = target_pos;
            for dy in 0..target_footprint[1] as i32 {
                for dx in 0..target_footprint[0] as i32 {
                    for dz in 0..target_footprint[2] as i32 {
                        let tv = VoxelCoord::new(
                            target_pos.x + dx,
                            target_pos.y + dy,
                            target_pos.z + dz,
                        );
                        if self.world.has_los(neighbor_pos, tv) {
                            has_los = true;
                            los_target = tv;
                            break;
                        }
                    }
                    if has_los {
                        break;
                    }
                }
                if has_los {
                    break;
                }
            }

            if has_los {
                // Check aim feasibility from the neighbor position.
                let origin_sub = SubVoxelCoord::from_voxel_center(neighbor_pos);
                let aim = compute_aim_velocity(origin_sub, los_target, speed, gravity, 5, 5000);
                if aim.hit_tick.is_some() {
                    // Check flight path for friendly-fire.
                    let ff_clear = self
                        .flight_path_blocked_by_friendly(
                            creature_id,
                            neighbor_pos,
                            los_target,
                            aim.velocity,
                        )
                        .is_none();

                    if ff_clear {
                        // This candidate has a clear shot.
                        let non_blocking = !blocks_others;
                        let dominated = best_clear.is_some_and(|(_, best_nb, best_d)| {
                            // Better if: non_blocking and best isn't, or same
                            // blocking status but closer.
                            if non_blocking && !best_nb {
                                false // new is better
                            } else if !non_blocking && best_nb {
                                true // old is better
                            } else {
                                dist_sq >= best_d // prefer closer
                            }
                        });
                        if !dominated {
                            best_clear = Some((edge_idx, non_blocking, dist_sq));
                        }
                        continue;
                    }
                }
            }

            // No clear shot from this position — consider as fallback if it
            // doesn't block other archers.
            if !blocks_others {
                let is_better = best_fallback.is_none_or(|(_, best_d)| dist_sq < best_d);
                if is_better {
                    best_fallback = Some((edge_idx, dist_sq));
                }
            }
        }

        // Prefer clear-shot candidates; fall back to non-blocking position.
        if let Some((edge_idx, _, _)) = best_clear {
            Some(edge_idx)
        } else {
            best_fallback.map(|(edge_idx, _)| edge_idx)
        }
    }
}
