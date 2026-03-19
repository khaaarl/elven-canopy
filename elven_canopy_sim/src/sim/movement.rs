// Movement system — navigation commands, pathfinding dispatch, and step execution.
//
// Handles player-directed movement commands (GoTo, group GoTo), unit spreading
// for group formations, task-oriented walking (`walk_toward_task`), single-step
// movement execution with voxel exclusion, and idle wandering.
//
// See also: `pathfinding.rs` (A* implementation), `nav.rs` (graph structure),
// `combat.rs` (attack-move walking, flee steps).
use super::*;
use crate::db::{ActionKind, MoveAction};
use crate::event::{ScheduledEventKind, SimEvent};
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
        _events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };

        if self.nav_graph.find_nearest_node(position).is_none() {
            return;
        }

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
            required_species: Some(creature.species),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        self.insert_task(new_task);
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.current_task = Some(task_id);
            c.path = None;
            let _ = self.db.creatures.update_no_fk(c);
        }

        // Only schedule a new activation if no Move action is in-flight.
        // If mid-Move, the existing activation will fire at
        // next_available_tick, resolve the step, and pick up the new task.
        if !mid_move {
            self.event_queue.schedule(
                self.tick + 1,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
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
        events: &mut Vec<SimEvent>,
    ) {
        if creature_ids.len() <= 1 {
            // Single creature — just delegate to the normal handler.
            if let Some(&cid) = creature_ids.first() {
                self.command_directed_goto(cid, position, events);
            }
            return;
        }
        let destinations = self.compute_spread_assignments(creature_ids, position);
        for (cid, dest) in destinations {
            self.command_directed_goto(cid, dest, events);
        }
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

    /// Walk one edge toward a task location using a stored or computed A* path.
    pub(crate) fn walk_toward_task(
        &mut self,
        creature_id: CreatureId,
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

        // Check if we already have a path. If so, resolve the next position
        // to a nav node + edge. If not (or path is exhausted), compute a new one.
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
                    self.wander(creature_id, current_node, events);
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

        // Move one edge. Compute traversal time from distance * ticks-per-voxel.
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

        let tpv = speeds.tpv_for_edge(edge.edge_type);
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

                // Set action state.
                creature.action_kind = ActionKind::Move;
                creature.next_available_tick = Some(tick + delay);

                // Advance stored path.
                if let Some(ref mut path) = creature.path
                    && !path.remaining_positions.is_empty()
                {
                    path.remaining_positions.remove(0);
                }
            });

        self.update_creature_spatial_index(creature_id, species, old_pos, dest_pos);

        // Insert MoveAction for render interpolation.
        let move_action = MoveAction {
            creature_id,
            move_from: old_pos,
            move_to: dest_pos,
            move_start_tick: tick,
        };
        // Remove any existing MoveAction (shouldn't happen, but be safe).
        let _ = self.db.move_actions.remove_no_fk(&creature_id);
        self.db.move_actions.insert_no_fk(move_action).unwrap();

        // Schedule next activation.
        self.event_queue.schedule(
            self.tick + delay,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Wander: pick a random adjacent nav node and move there.
    ///
    /// Creatures with aggressive or defensive initiative auto-engage detected
    /// hostiles via `hostile_pursue()` before falling back to random wandering.
    /// Passive creatures never pursue.
    pub(crate) fn wander(
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
        ) && self.hostile_pursue(creature_id, current_node, species, events)
        {
            return;
        }

        self.random_wander(creature_id, current_node, species);
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
    /// Used by `flee_step()` when a cornered creature has no unblocked exit
    /// and must force through a hostile-occupied voxel.
    pub(crate) fn move_one_step(
        &mut self,
        creature_id: CreatureId,
        species: Species,
        edge_idx: NavEdgeId,
    ) -> bool {
        self.move_one_step_inner(creature_id, species, edge_idx, false)
    }

    /// Inner implementation of `move_one_step` with an optional exclusion bypass.
    pub(crate) fn move_one_step_inner(
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
                self.event_queue.schedule(
                    self.tick + retry_delay,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
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
        let _ = self
            .db
            .creatures
            .modify_unchecked(&creature_id, |creature| {
                creature.position = dest_pos;

                // Set action state.
                creature.action_kind = ActionKind::Move;
                creature.next_available_tick = Some(tick + delay);
            });

        self.update_creature_spatial_index(creature_id, species, old_pos, dest_pos);

        // Insert MoveAction for render interpolation.
        let move_action = MoveAction {
            creature_id,
            move_from: old_pos,
            move_to: dest_pos,
            move_start_tick: tick,
        };
        let _ = self.db.move_actions.remove_no_fk(&creature_id);
        self.db.move_actions.insert_no_fk(move_action).unwrap();

        // Schedule next activation based on edge traversal time.
        self.event_queue.schedule(
            self.tick + delay,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
        true
    }

    /// Random wander: pick a random eligible edge and move one step.
    pub(crate) fn random_wander(
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
                self.event_queue.schedule(
                    self.tick + 1000,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
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
            self.event_queue.schedule(
                self.tick + 1000,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
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
            self.event_queue.schedule(
                self.tick + self.config.voxel_exclusion_retry_ticks,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return;
        } else {
            &unblocked_edges
        };

        // Pick a random eligible edge.
        let chosen_idx = self.rng.range_u64(0, edges_to_pick.len() as u64) as usize;
        let edge_idx = edges_to_pick[chosen_idx];

        self.move_one_step(creature_id, species, edge_idx);
    }
}
