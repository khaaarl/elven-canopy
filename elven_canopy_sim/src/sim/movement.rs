// Movement system — navigation commands, pathfinding dispatch, and step execution.
//
// Handles player-directed movement commands (GoTo, group GoTo), unit spreading
// for group formations, task-oriented walking (`walk_toward_task`), single-step
// movement execution with voxel exclusion, idle wandering, and command queue
// management (`find_queue_tail` for shift+right-click sequential queuing).
//
// See also: `pathfinding.rs` (A* implementation), `nav.rs` (graph structure),
// `combat.rs` (attack-move walking, flee steps),
// `flight_pathfinding.rs` (vanilla A* on voxel grid for flying creatures).
use super::*;
use crate::db::{ActionKind, MoveAction};
use crate::event::SimEvent;
use crate::pathfinding;
use crate::preemption;
use crate::task;
use crate::types::NavEdgeId;

impl SimState {
    /// Process a `DirectedGoTo` command: create a GoTo task for a specific
    /// creature and immediately assign it, preempting lower-priority tasks.
    pub(crate) fn command_directed_goto(
        &mut self,
        creature_id: CreatureId,
        position: VoxelCoord,
        queue: bool,
        _events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };

        // Ground creatures need a reachable nav node at the destination.
        // Flying creatures can go anywhere in open air.
        let is_flying = self.species_table[&creature.species]
            .flight_ticks_per_voxel
            .is_some();
        if !is_flying && self.nav_graph.find_nearest_node(position).is_none() {
            return;
        }

        let species = creature.species;

        // Queue mode: append to the creature's command queue instead of preempting.
        if queue && let Some(tail_id) = self.find_queue_tail(creature_id) {
            let task_id = TaskId::new(&mut self.rng);
            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::GoTo,
                state: task::TaskState::Available,
                location: position,
                progress: 0,
                total_cost: 0,
                required_species: Some(species),
                origin: task::TaskOrigin::PlayerDirected,
                target_creature: None,
                restrict_to_creature_id: Some(creature_id),
                prerequisite_task_id: Some(tail_id),
                required_civ_id: None,
            };
            self.insert_task(new_task);
            return;
        }
        // Queue mode with no current task falls through to non-queue behavior.

        // Non-queue: cancel any existing command queue for this creature.
        self.cancel_creature_queue(creature_id);

        // Re-fetch creature after potential queue cancellation.
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };

        // Check preemption: can PlayerDirected preempt the current task?
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
                preemption::PreemptionLevel::PlayerDirected,
                task::TaskOrigin::PlayerDirected,
            ) {
                return;
            }
            // Preempt: for mid-Move, let the step complete naturally; for
            // other actions, abort them (B-erratic-movement).
            mid_move = self.preempt_task(creature_id, current_task_id);
        } else if creature.action_kind == ActionKind::Move && creature.next_available_tick.is_some()
        {
            // Creature is mid-wander-move (no task). Let the step finish
            // naturally — the existing activation will resolve the move and
            // pick up the new task (B-erratic-movement-2).
            mid_move = true;
        }

        let task_id = TaskId::new(&mut self.rng);
        let new_task = task::Task {
            id: task_id,
            kind: task::TaskKind::GoTo,
            state: task::TaskState::InProgress,
            location: position,
            progress: 0,
            total_cost: 0,
            required_species: Some(species),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        self.insert_task(new_task);
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.current_task = Some(task_id);
            c.path = None;
            let _ = self.db.update_creature(c);
        }

        // Only schedule a new activation if no Move action is in-flight.
        // If mid-Move, the existing activation will fire at
        // next_available_tick, resolve the step, and pick up the new task.
        if !mid_move {
            self.set_creature_activation_tick(creature_id, self.tick + 1);
        }
    }

    /// Process a `GroupGoTo` command: spread multiple creatures across nearby
    /// nav nodes around the destination. Assigns creatures to destinations
    /// greedily — the closest creature-destination pair is matched first,
    /// then the next closest, and so on.
    pub(crate) fn command_group_goto(
        &mut self,
        creature_ids: &[CreatureId],
        position: VoxelCoord,
        queue: bool,
        events: &mut Vec<SimEvent>,
    ) {
        if creature_ids.len() <= 1 {
            // Single creature — just delegate to the normal handler.
            if let Some(&cid) = creature_ids.first() {
                self.command_directed_goto(cid, position, queue, events);
            }
            return;
        }
        let destinations = self.compute_spread_assignments(creature_ids, position);
        for (cid, dest) in destinations {
            self.command_directed_goto(cid, dest, queue, events);
        }
    }

    /// Find the last task in a creature's command queue. Starts from the
    /// creature's current_task and follows prerequisite_task_id links forward
    /// (finds tasks whose prerequisite is the current tail) to the end.
    ///
    /// If `current_task` is None (e.g., creature is fleeing), searches for
    /// orphaned queued tasks via `restrict_to_creature_id` and walks to the
    /// tail of that chain. This preserves the queue across autonomous
    /// interruptions.
    ///
    /// Returns `None` if the creature has no queue at all, or if a cycle is
    /// detected (defensive guard against corrupted data).
    pub(crate) fn find_queue_tail(&self, creature_id: CreatureId) -> Option<TaskId> {
        let creature = self.db.creatures.get(&creature_id)?;

        let start = if let Some(task_id) = creature.current_task {
            task_id
        } else {
            // No current task — look for orphaned queued tasks restricted
            // to this creature. Find the chain head: the non-Complete task
            // whose prerequisite is Complete or missing (i.e., already
            // satisfied). If multiple exist, pick the first by BTree order
            // for determinism.
            let orphaned = self
                .db
                .tasks
                .by_restrict_to_creature_id(&Some(creature_id), tabulosity::QueryOpts::ASC);
            let head = orphaned.into_iter().find(|t| {
                t.state != task::TaskState::Complete
                    && t.prerequisite_task_id.is_none_or(|pid| {
                        self.db
                            .tasks
                            .get(&pid)
                            .is_none_or(|pt| pt.state == task::TaskState::Complete)
                    })
            })?;
            head.id
        };

        // Walk the chain forward from `start` to find the tail.
        let mut current = start;

        // Cap iterations to prevent infinite loops on corrupted prerequisite
        // chains. 256 is far beyond any realistic queue depth.
        const MAX_CHAIN_LEN: usize = 256;

        for _ in 0..MAX_CHAIN_LEN {
            // Find any non-complete task whose prerequisite is `current`.
            let next = self
                .db
                .tasks
                .by_prerequisite_task_id(&Some(current), tabulosity::QueryOpts::ASC)
                .into_iter()
                .find(|t| t.state != task::TaskState::Complete);
            match next {
                Some(t) => current = t.id,
                None => return Some(current),
            }
        }

        // Chain exceeded max length — likely a cycle. Bail out.
        None
    }

    /// Compute spread destination assignments for a group of creatures moving
    /// to the same target. Uses BFS to find nearby nav nodes, then greedily
    /// assigns each creature to the nearest available destination.
    ///
    /// Returns `(creature_id, voxel_destination)` pairs. Creatures that are
    /// dead or missing are silently excluded.
    pub(crate) fn compute_spread_assignments(
        &self,
        creature_ids: &[CreatureId],
        target: VoxelCoord,
    ) -> Vec<(CreatureId, VoxelCoord)> {
        let center = match self.nav_graph.find_nearest_node(target) {
            Some(n) => n,
            None => return Vec::new(),
        };

        // Collect alive creature positions.
        let creatures: Vec<(CreatureId, VoxelCoord)> = creature_ids
            .iter()
            .filter_map(|&cid| {
                let c = self.db.creatures.get(&cid)?;
                if c.vital_status != VitalStatus::Alive {
                    return None;
                }
                let node = self.nav_graph.find_nearest_node(c.position)?;
                let pos = self.nav_graph.node(node).position;
                Some((cid, pos))
            })
            .collect();

        if creatures.is_empty() {
            return Vec::new();
        }

        // Get spread destinations via BFS.
        let dest_nodes = self.nav_graph.spread_destinations(center, creatures.len());
        let dest_positions: Vec<(usize, VoxelCoord)> = dest_nodes
            .iter()
            .enumerate()
            .map(|(i, &nid)| (i, self.nav_graph.node(nid).position))
            .collect();

        // Greedy assignment: build all (creature, dest) pairs sorted by
        // distance, then assign closest first.
        let mut assignments = Vec::with_capacity(creatures.len());
        let mut used_creatures = vec![false; creatures.len()];
        let mut used_dests = vec![false; dest_positions.len()];

        // Build a sorted list of (distance, creature_idx, dest_idx).
        let mut pairs: Vec<(u32, usize, usize)> = Vec::new();
        for (ci, &(_, cpos)) in creatures.iter().enumerate() {
            for (di, &(_, dpos)) in dest_positions.iter().enumerate() {
                pairs.push((cpos.manhattan_distance(dpos), ci, di));
            }
        }
        pairs.sort();

        for (_, ci, di) in pairs {
            if used_creatures[ci] || used_dests[di] {
                continue;
            }
            used_creatures[ci] = true;
            used_dests[di] = true;
            assignments.push((creatures[ci].0, dest_positions[di].1));
        }

        // Any creatures that didn't get a unique destination (more creatures
        // than available nav nodes) get the center.
        let center_pos = self.nav_graph.node(center).position;
        for (ci, &(cid, _)) in creatures.iter().enumerate() {
            if !used_creatures[ci] {
                assignments.push((cid, center_pos));
            }
        }

        assignments
    }

    /// Move one step toward a target position. Flying creatures use voxel-grid
    /// A* via `fly_toward_target`; ground creatures use nav-graph A* and
    /// `ground_move_one_step`. Both store cached paths in `creature.path` as
    /// `Vec<VoxelCoord>`.
    pub(crate) fn walk_toward_task(
        &mut self,
        creature_id: CreatureId,
        target_coord: VoxelCoord,
        current_node: Option<NavNodeId>,
        events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let species = creature.species;

        // Flying creatures: delegate to flight pathfinding.
        if self.species_table[&species]
            .flight_ticks_per_voxel
            .is_some()
        {
            if !self.fly_toward_target(creature_id, target_coord, events) {
                // Can't reach target — unassign and wander.
                self.unassign_creature_from_task(creature_id);
                self.fly_wander(creature_id, events);
            }
            return;
        }

        // Ground creatures: resolve target to nav node and use graph A*.
        let current_node = match current_node {
            Some(n) => n,
            None => return,
        };
        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);
        let task_location = match graph.find_nearest_node(target_coord) {
            Some(n) => n,
            None => {
                self.unassign_creature_from_task(creature_id);
                self.ground_wander(creature_id, current_node, events);
                return;
            }
        };

        // Check if we already have a path. If so, resolve the next position
        // to a nav node + edge. If not (or path is exhausted), compute a new one.
        let creature = self.db.creatures.get(&creature_id).unwrap();
        let cn = graph.node_at(creature.position);
        let next_step = if let Some(ref path) = creature.path {
            if let Some(&next_pos) = path.remaining_positions.first() {
                // Resolve the stored position to a nav node and find the edge.
                let dest_node = graph.node_at(next_pos);
                let edge_idx =
                    dest_node.and_then(|dn| cn.and_then(|cn| graph.find_edge_to(cn, dn)));
                match (edge_idx, dest_node) {
                    (Some(ei), Some(dn)) => Some((ei, dn)),
                    _ => None, // graph changed; repath
                }
            } else {
                None
            }
        } else {
            None
        };

        // Apply creature stats to movement speeds.
        let agility = self.trait_int(creature_id, TraitKind::Agility, 0);
        let strength = self.trait_int(creature_id, TraitKind::Strength, 0);
        let speeds = crate::stats::CreatureMoveSpeeds::new(species_data, agility, strength);
        let walk_tpv = speeds.walk_tpv;
        let climb_tpv = speeds.climb_tpv;
        let wood_ladder_tpv = speeds.wood_ladder_tpv;
        let rope_ladder_tpv = speeds.rope_ladder_tpv;

        let (edge_idx, dest_node) = if let Some(step) = next_step {
            step
        } else {
            // Compute path to task location.
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
                    // Can't reach task — unassign and wander.
                    self.unassign_creature_from_task(creature_id);
                    self.ground_wander(creature_id, current_node, events);
                    return;
                }
            };

            let first_edge = path_result.edge_indices[0];
            let first_dest = path_result.nodes[1];

            // Store remaining path as positions for future activations.
            let remaining_positions: Vec<VoxelCoord> = path_result.nodes[1..]
                .iter()
                .map(|&nid| graph.node(nid).position)
                .collect();
            if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                creature.path = Some(CreaturePath {
                    remaining_positions,
                });
                let _ = self.db.update_creature(creature);
            }

            (first_edge, first_dest)
        };

        // Move one edge. Compute traversal time from distance * ticks-per-voxel.
        let graph = self.graph_for_species(species);
        let edge = graph.edge(edge_idx);
        let dest_pos = graph.node(dest_node).position;

        // Voxel exclusion: reject move if destination is hostile-occupied.
        let footprint = self.species_table[&species].footprint;
        if self.destination_blocked_by_hostile(creature_id, dest_pos, footprint) {
            if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                creature.path = None;
                creature.next_available_tick =
                    Some(self.tick + self.config.voxel_exclusion_retry_ticks);
                let _ = self.db.update_creature(creature);
            }
            return;
        }

        let tpv = speeds.tpv_for_edge(edge.edge_type);
        let delay = (edge.distance as u64 * tpv)
            .div_ceil(crate::nav::DIST_SCALE as u64)
            .max(1);

        let old_pos = self.db.creatures.get(&creature_id).unwrap().position;
        let tick = self.tick;
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.position = dest_pos;

            // Set action state.
            creature.action_kind = ActionKind::Move;
            creature.next_available_tick = Some(tick + delay);

            // Advance stored path.
            if let Some(ref mut path) = creature.path
                && !path.remaining_positions.is_empty()
            {
                path.remaining_positions.remove(0);
            }
            let _ = self.db.update_creature(creature);
        }

        self.update_creature_spatial_index(creature_id, species, old_pos, dest_pos);

        // Insert MoveAction for render interpolation.
        let move_action = MoveAction {
            creature_id,
            move_from: old_pos,
            move_to: dest_pos,
            move_start_tick: tick,
        };
        // Remove any existing MoveAction (shouldn't happen, but be safe).
        let _ = self.db.remove_move_action(&creature_id);
        self.db.insert_move_action(move_action).unwrap();
    }

    /// Wander: pick a random adjacent nav node and move there.
    ///
    /// Creatures with aggressive or defensive initiative auto-engage detected
    /// hostiles via `hostile_pursue()` before falling back to random wandering.
    /// Passive creatures never pursue.
    pub(crate) fn ground_wander(
        &mut self,
        creature_id: CreatureId,
        current_node: NavNodeId,
        events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let species = creature.species;

        let style = self.resolve_engagement_style(creature_id);

        // Non-passive creatures pursue detected hostile targets instead of
        // wandering randomly. Falls through to random wander if no target
        // is within detection range or reachable.
        use crate::species::EngagementInitiative;
        if matches!(
            style.initiative,
            EngagementInitiative::Aggressive | EngagementInitiative::Defensive
        ) && self.hostile_pursue(creature_id, Some(current_node), species, events)
        {
            return;
        }

        self.ground_random_wander(creature_id, current_node, species);
    }

    /// Move a creature one step along the given nav graph edge: update position,
    /// spatial index, action state, render interpolation, and schedule the next
    /// activation. Shared by random wander, hostile pursuit, and flee.
    ///
    /// Returns `true` if the move succeeded. Returns `false` if the destination
    /// is blocked by a hostile creature (voxel exclusion) — in that case the
    /// creature stays put and a short retry activation is scheduled.
    ///
    /// `skip_exclusion`: if `true`, the voxel exclusion check is bypassed.
    /// Used by `ground_flee_step()` when a cornered creature has no unblocked exit
    /// and must force through a hostile-occupied voxel.
    pub(crate) fn ground_move_one_step(
        &mut self,
        creature_id: CreatureId,
        species: Species,
        edge_idx: NavEdgeId,
    ) -> bool {
        self.ground_move_one_step_inner(creature_id, species, edge_idx, false)
    }

    /// Inner implementation of `ground_move_one_step` with an optional exclusion bypass.
    pub(crate) fn ground_move_one_step_inner(
        &mut self,
        creature_id: CreatureId,
        species: Species,
        edge_idx: NavEdgeId,
        skip_exclusion: bool,
    ) -> bool {
        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);
        let edge = graph.edge(edge_idx);
        let dest_node = edge.to;

        // Voxel exclusion: reject the move if any destination footprint voxel
        // is occupied by a living hostile creature.
        let dest_pos = graph.node(dest_node).position;
        if !skip_exclusion {
            let footprint = species_data.footprint;
            if self.destination_blocked_by_hostile(creature_id, dest_pos, footprint) {
                let retry_delay = self.config.voxel_exclusion_retry_ticks;
                self.set_creature_activation_tick(creature_id, self.tick + retry_delay);
                return false;
            }
        }

        // Compute stat-modified ticks-per-voxel.
        let agility = self.trait_int(creature_id, TraitKind::Agility, 0);
        let strength = self.trait_int(creature_id, TraitKind::Strength, 0);
        let speeds = crate::stats::CreatureMoveSpeeds::new(species_data, agility, strength);
        let tpv = speeds.tpv_for_edge(edge.edge_type);
        let delay = (edge.distance as u64 * tpv)
            .div_ceil(crate::nav::DIST_SCALE as u64)
            .max(1);

        // Move creature to the destination.
        let old_pos = self.db.creatures.get(&creature_id).unwrap().position;
        let tick = self.tick;
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.position = dest_pos;

            // Set action state.
            creature.action_kind = ActionKind::Move;
            creature.next_available_tick = Some(tick + delay);
            let _ = self.db.update_creature(creature);
        }

        self.update_creature_spatial_index(creature_id, species, old_pos, dest_pos);

        // Insert MoveAction for render interpolation.
        let move_action = MoveAction {
            creature_id,
            move_from: old_pos,
            move_to: dest_pos,
            move_start_tick: tick,
        };
        let _ = self.db.remove_move_action(&creature_id);
        self.db.insert_move_action(move_action).unwrap();

        true
    }

    /// Random wander: pick a random eligible edge and move one step.
    pub(crate) fn ground_random_wander(
        &mut self,
        creature_id: CreatureId,
        current_node: NavNodeId,
        species: Species,
    ) {
        // Collect eligible edges before mutably borrowing self (for rng).
        let eligible_edges: Vec<NavEdgeId> = {
            let species_data = &self.species_table[&species];
            let graph = self.graph_for_species(species);
            let edge_indices = graph.neighbors(current_node);
            if edge_indices.is_empty() {
                self.set_creature_activation_tick(creature_id, self.tick + 1000);
                return;
            }
            if let Some(ref allowed) = species_data.allowed_edge_types {
                edge_indices
                    .iter()
                    .copied()
                    .filter(|&idx| allowed.contains(&graph.edge(idx).edge_type))
                    .collect()
            } else {
                edge_indices.to_vec()
            }
        };

        if eligible_edges.is_empty() {
            self.set_creature_activation_tick(creature_id, self.tick + 1000);
            return;
        }

        // Voxel exclusion: filter out edges leading to hostile-occupied voxels.
        let footprint = self.species_table[&species].footprint;
        let unblocked_edges: Vec<NavEdgeId> = {
            let graph = self.graph_for_species(species);
            eligible_edges
                .iter()
                .copied()
                .filter(|&idx| {
                    let dest_id = graph.edge(idx).to;
                    if !graph.is_node_alive(dest_id) {
                        return false;
                    }
                    let dest_pos = graph.node(dest_id).position;
                    !self.destination_blocked_by_hostile(creature_id, dest_pos, footprint)
                })
                .collect()
        };

        let edges_to_pick = if unblocked_edges.is_empty() {
            // All neighbors hostile-occupied — wait rather than walk into a hostile.
            self.set_creature_activation_tick(
                creature_id,
                self.tick + self.config.voxel_exclusion_retry_ticks,
            );
            return;
        } else {
            &unblocked_edges
        };

        // Pick a random eligible edge.
        let chosen_idx = self.rng.range_u64(0, edges_to_pick.len() as u64) as usize;
        let edge_idx = edges_to_pick[chosen_idx];

        self.ground_move_one_step(creature_id, species, edge_idx);
    }

    // -----------------------------------------------------------------------
    // Flying creature movement (flight_pathfinding.rs — vanilla A* on voxel grid)
    // -----------------------------------------------------------------------

    /// Move a flying creature one step along its flight path toward `target_pos`.
    /// Computes a flight path via A* if none is cached, then steps one voxel.
    /// Returns `true` if a step was taken.
    pub(crate) fn fly_toward_target(
        &mut self,
        creature_id: CreatureId,
        target_pos: VoxelCoord,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return false,
        };
        let species = creature.species;
        let flight_tpv = match self.species_table[&species].flight_ticks_per_voxel {
            Some(tpv) => tpv,
            None => return false,
        };
        let current_pos = creature.position;

        // Check if we have a cached flight path with remaining steps.
        let next_pos = if let Some(ref path) = creature.path {
            if let Some(&next) = path.remaining_positions.first() {
                // Validate that the next position is still flyable (full footprint).
                let footprint = self.species_table[&species].footprint;
                if !crate::flight_pathfinding::footprint_flyable(&self.world, next, footprint) {
                    None // obstacle appeared; repath
                } else {
                    Some(next)
                }
            } else {
                None // path exhausted; repath
            }
        } else {
            None
        };

        let next_pos = if let Some(p) = next_pos {
            p
        } else {
            // Compute a new flight path.
            let max_nodes = 10_000;
            let footprint = self.species_table[&species].footprint;
            let path_result = crate::flight_pathfinding::astar_fly(
                &self.world,
                current_pos,
                target_pos,
                flight_tpv,
                max_nodes,
                footprint,
            );
            let path_result = match path_result {
                Some(r) if r.waypoints.len() >= 2 => r,
                _ => return false, // no path
            };

            let next = path_result.waypoints[1];

            // Store remaining path (skip start, which is current position).
            let remaining_positions: Vec<VoxelCoord> = path_result.waypoints[1..].to_vec();
            if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                creature.path = Some(CreaturePath {
                    remaining_positions,
                });
                let _ = self.db.update_creature(creature);
            }

            next
        };

        self.fly_one_step(creature_id, species, next_pos, flight_tpv, events)
    }

    /// Execute a single flight step: move the creature to `dest_pos`, update
    /// spatial index, record MoveAction for render interpolation, and schedule
    /// next activation.
    ///
    /// Returns `true` if the move succeeded.
    pub(crate) fn fly_one_step(
        &mut self,
        creature_id: CreatureId,
        species: Species,
        dest_pos: VoxelCoord,
        flight_tpv: u64,
        _events: &mut Vec<SimEvent>,
    ) -> bool {
        let old_pos = match self.db.creatures.get(&creature_id) {
            Some(c) => c.position,
            None => return false,
        };

        // Voxel exclusion: reject move if destination is hostile-occupied.
        let footprint = self.species_table[&species].footprint;
        if self.destination_blocked_by_hostile(creature_id, dest_pos, footprint) {
            self.set_creature_activation_tick(
                creature_id,
                self.tick + self.config.voxel_exclusion_retry_ticks,
            );
            return false;
        }

        // Compute delay from scaled distance.
        let dx = dest_pos.x - old_pos.x;
        let dy = dest_pos.y - old_pos.y;
        let dz = dest_pos.z - old_pos.z;
        let dist_scaled = crate::nav::scaled_distance(dx, dy, dz);
        let delay = (dist_scaled as u64 * flight_tpv)
            .div_ceil(crate::nav::DIST_SCALE as u64)
            .max(1);

        let tick = self.tick;
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.position = dest_pos;
            creature.action_kind = ActionKind::Move;
            creature.next_available_tick = Some(tick + delay);

            // Advance stored path.
            if let Some(ref mut path) = creature.path
                && !path.remaining_positions.is_empty()
            {
                path.remaining_positions.remove(0);
            }
            let _ = self.db.update_creature(creature);
        }

        self.update_creature_spatial_index(creature_id, species, old_pos, dest_pos);

        // Insert MoveAction for render interpolation.
        let move_action = MoveAction {
            creature_id,
            move_from: old_pos,
            move_to: dest_pos,
            move_start_tick: tick,
        };
        let _ = self.db.remove_move_action(&creature_id);
        self.db.insert_move_action(move_action).unwrap();

        true
    }

    /// Random flying wander: pick a random flyable neighbor voxel and move there.
    pub(crate) fn fly_wander(&mut self, creature_id: CreatureId, events: &mut Vec<SimEvent>) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let species = creature.species;
        let flight_tpv = match self.species_table[&species].flight_ticks_per_voxel {
            Some(tpv) => tpv,
            None => return,
        };
        let pos = creature.position;

        // Collect flyable neighbor positions (check full footprint clearance).
        let footprint = self.species_table[&species].footprint;
        let offsets: [(i32, i32, i32); 6] = [
            (-1, 0, 0),
            (1, 0, 0),
            (0, -1, 0),
            (0, 1, 0),
            (0, 0, -1),
            (0, 0, 1),
        ];
        let mut candidates: Vec<VoxelCoord> = Vec::new();
        for &(dx, dy, dz) in &offsets {
            let neighbor = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
            if crate::flight_pathfinding::footprint_flyable(&self.world, neighbor, footprint) {
                candidates.push(neighbor);
            }
        }

        if candidates.is_empty() {
            // Stuck — schedule retry.
            self.set_creature_activation_tick(creature_id, self.tick + 1000);
            return;
        }

        let chosen_idx = self.rng.range_u64(0, candidates.len() as u64) as usize;
        let dest = candidates[chosen_idx];
        self.fly_one_step(creature_id, species, dest, flight_tpv, events);
    }
}
