// Movement system — navigation commands, pathfinding dispatch, and step execution.
//
// Handles player-directed movement commands (GoTo, group GoTo), unit spreading
// for group formations, task-oriented walking (`walk_toward_task`), single-step
// movement execution with voxel exclusion, idle wandering, and command queue
// management (`find_queue_tail` for shift+right-click sequential queuing).
//
// See also: `pathfinding.rs` (A* for ground and flight), `nav.rs` (edge
// types and distance constants), `combat.rs` (attack-move walking, flee steps).
use super::*;
use crate::db::{ActionKind, MoveAction};
use crate::event::SimEvent;
use crate::preemption;
use crate::task;

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
        let zone_id = creature.zone_id.unwrap();

        // Ground creatures need a walkable position at the destination.
        // Snap the task location to the nearest walkable voxel so that
        // find_path can resolve it exactly.
        // Flying creatures can go anywhere in open air.
        let species = creature.species;
        let species_data_mv = &self.species_table[&species];
        let is_flying = creature.movement_category.is_flyer();
        let footprint = species_data_mv.footprint;
        let can_climb = creature.movement_category.can_climb();
        let task_location = if is_flying {
            position
        } else {
            let zone = self.voxel_zone(zone_id).unwrap();
            match crate::walkability::find_nearest_walkable(
                zone,
                &zone.face_data,
                position,
                5,
                footprint,
                can_climb,
            ) {
                Some(pos) => pos,
                None => return,
            }
        };

        // Queue mode: append to the creature's command queue instead of preempting.
        if queue && let Some(tail_id) = self.find_queue_tail(creature_id) {
            let task_id = TaskId::new(&mut self.rng);
            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::GoTo,
                state: task::TaskState::Available,
                location: task_location,
                progress: 0,
                total_cost: 0,
                required_species: Some(species),
                origin: task::TaskOrigin::PlayerDirected,
                target_creature: None,
                restrict_to_creature_id: Some(creature_id),
                prerequisite_task_id: Some(tail_id),
                required_civ_id: None,
            };
            // TODO(F-zone-world): derive zone from creature/entity context
            self.insert_task(self.home_zone_id(), new_task);
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
            location: task_location,
            progress: 0,
            total_cost: 0,
            required_species: Some(species),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        // TODO(F-zone-world): derive zone from creature/entity context
        self.insert_task(self.home_zone_id(), new_task);
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
        // Use the first alive creature's footprint for walkability checks.
        // All creatures in a group move share the same footprint (elf groups
        // are all [1,1,1]; large creatures don't participate in group moves).
        let first_creature = creature_ids
            .iter()
            .filter_map(|&cid| self.db.creatures.get(&cid))
            .find(|c| c.vital_status == VitalStatus::Alive);
        let first_footprint = first_creature
            .as_ref()
            .map(|c| self.species_table[&c.species].footprint)
            .unwrap_or([1, 1, 1]);
        let first_can_climb = first_creature
            .as_ref()
            .map(|c| c.movement_category.can_climb())
            .unwrap_or(false);

        let zone_id = creature_ids
            .iter()
            .filter_map(|&cid| self.db.creatures.get(&cid))
            .filter(|c| c.vital_status == VitalStatus::Alive)
            .filter_map(|c| c.zone_id)
            .next()
            .unwrap_or_else(|| self.home_zone_id()); // TODO(F-zone-world): derive from entity context
        let zone = self.voxel_zone(zone_id).unwrap();
        let center = match crate::walkability::find_nearest_walkable(
            zone,
            &zone.face_data,
            target,
            5,
            first_footprint,
            first_can_climb,
        ) {
            Some(p) => p,
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
                Some((cid, c.position.min))
            })
            .collect();

        if creatures.is_empty() {
            return Vec::new();
        }

        // Get spread destinations via BFS on walkable voxel grid.
        let dest_coords = crate::walkability::spread_destinations(
            zone,
            &zone.face_data,
            center,
            creatures.len(),
            first_footprint,
            first_can_climb,
        );
        let dest_positions: Vec<(usize, VoxelCoord)> = dest_coords
            .iter()
            .enumerate()
            .map(|(i, &p)| (i, p))
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
        // than available positions) get the center.
        for (ci, &(cid, _)) in creatures.iter().enumerate() {
            if !used_creatures[ci] {
                assignments.push((cid, center));
            }
        }

        assignments
    }

    /// Move one step toward a target position. Flying creatures use voxel-grid
    /// A* via `fly_toward_target`; ground creatures use voxel-direct A* and
    /// `ground_move_one_step`. Both store cached paths in `creature.path` as
    /// `Vec<VoxelCoord>`.
    pub(crate) fn walk_toward_task(
        &mut self,
        creature_id: CreatureId,
        target_coord: VoxelCoord,
        current_pos: Option<VoxelCoord>,
        events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let zone_id = creature.zone_id.unwrap();
        let species = creature.species;

        // Flying creatures: delegate to flight pathfinding.
        if creature.movement_category.is_flyer() {
            if !self.fly_toward_target(creature_id, target_coord, events) {
                // Can't reach target — unassign and wander.
                self.unassign_creature_from_task(creature_id);
                self.fly_wander(creature_id, events);
            }
            return;
        }

        // Ground creatures: use voxel-direct A* via find_path.
        let current_pos = match current_pos {
            Some(n) => n,
            None => return,
        };

        // Check if we already have a cached path with a walkable next step.
        let creature = self.db.creatures.get(&creature_id).unwrap();
        let species_data_goto = &self.species_table[&species];
        let footprint = species_data_goto.footprint;
        let can_climb = creature.movement_category.can_climb();
        let next_pos = if let Some(ref path) = creature.path {
            if let Some(&next) = path.remaining_positions.first() {
                // Validate: next position must still be walkable.
                let zone = self.voxel_zone(zone_id).unwrap();
                let walkable = crate::walkability::footprint_walkable(
                    zone,
                    &zone.face_data,
                    next,
                    footprint,
                    can_climb,
                );
                if walkable {
                    Some(next)
                } else {
                    None // world changed; repath
                }
            } else {
                None
            }
        } else {
            None
        };

        let dest_pos = if let Some(pos) = next_pos {
            pos
        } else {
            // Compute path to task location.
            let path_result = self.find_path(
                creature_id,
                target_coord,
                &crate::pathfinding::PathOpts::default(),
            );

            let path_result = match path_result {
                Ok(r) if r.positions.len() >= 2 => r,
                _ => {
                    // Can't reach task — unassign and wander.
                    self.unassign_creature_from_task(creature_id);
                    self.ground_wander(creature_id, current_pos, events);
                    return;
                }
            };

            let first_dest = path_result.positions[1];

            // Store remaining path as positions for future activations.
            let remaining_positions: Vec<VoxelCoord> = path_result.positions[1..].to_vec();
            if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                creature.path = Some(CreaturePath {
                    remaining_positions,
                });
                let _ = self.db.update_creature(creature);
            }

            first_dest
        };

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

        // Derive edge type for speed computation and compute traversal delay.
        let old_pos = self.db.creatures.get(&creature_id).unwrap().position.min;
        let from_surface = {
            let zone = self.voxel_zone(zone_id).unwrap();
            crate::walkability::derive_surface_type(zone, &zone.face_data, old_pos)
        };
        let to_surface = {
            let zone = self.voxel_zone(zone_id).unwrap();
            crate::walkability::derive_surface_type(zone, &zone.face_data, dest_pos)
        };
        let edge_type =
            crate::walkability::derive_edge_type(from_surface, to_surface, old_pos, dest_pos);

        let species_data = &self.species_table[&species];
        let creature_ref = self.db.creatures.get(&creature_id).unwrap();
        let agility = self.trait_int(creature_id, TraitKind::Agility, 0);
        let base_tpv = crate::stats::creature_base_tpv(species_data.move_ticks_per_voxel, agility);
        let tpv = creature_ref
            .movement_category
            .tpv_for_edge_type(edge_type, base_tpv)
            .unwrap_or(base_tpv);
        let dx = dest_pos.x - old_pos.x;
        let dy = dest_pos.y - old_pos.y;
        let dz = dest_pos.z - old_pos.z;
        let distance = crate::nav::scaled_distance(dx, dy, dz);
        let delay = (distance as u64 * tpv)
            .div_ceil(crate::nav::DIST_SCALE as u64)
            .max(1);

        let tick = self.tick;
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.position = creature.position.with_anchor(dest_pos);

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
        current_pos: VoxelCoord,
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
        ) && self.hostile_pursue(creature_id, None, species, events)
        {
            return;
        }

        self.ground_random_wander(creature_id, current_pos, species);
    }

    /// Move a creature one step to an adjacent voxel: update position,
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
        dest_pos: VoxelCoord,
    ) -> bool {
        self.ground_move_one_step_inner(creature_id, species, dest_pos, false)
    }

    /// Inner implementation of `ground_move_one_step` with an optional exclusion bypass.
    pub(crate) fn ground_move_one_step_inner(
        &mut self,
        creature_id: CreatureId,
        species: Species,
        dest_pos: VoxelCoord,
        skip_exclusion: bool,
    ) -> bool {
        let species_data = &self.species_table[&species];

        // Voxel exclusion: reject the move if any destination footprint voxel
        // is occupied by a living hostile creature.
        if !skip_exclusion {
            let footprint = species_data.footprint;
            if self.destination_blocked_by_hostile(creature_id, dest_pos, footprint) {
                let retry_delay = self.config.voxel_exclusion_retry_ticks;
                self.set_creature_activation_tick(creature_id, self.tick + retry_delay);
                return false;
            }
        }

        // Derive edge type from voxel surfaces for speed computation.
        let creature_row = self.db.creatures.get(&creature_id).unwrap();
        let old_pos = creature_row.position.min;
        let zone_id = creature_row.zone_id.unwrap();
        let from_surface = {
            let zone = self.voxel_zone(zone_id).unwrap();
            crate::walkability::derive_surface_type(zone, &zone.face_data, old_pos)
        };
        let to_surface = {
            let zone = self.voxel_zone(zone_id).unwrap();
            crate::walkability::derive_surface_type(zone, &zone.face_data, dest_pos)
        };
        let edge_type =
            crate::walkability::derive_edge_type(from_surface, to_surface, old_pos, dest_pos);

        // Compute stat-modified ticks-per-voxel.
        let creature_ref = self.db.creatures.get(&creature_id).unwrap();
        let agility = self.trait_int(creature_id, TraitKind::Agility, 0);
        let base_tpv = crate::stats::creature_base_tpv(species_data.move_ticks_per_voxel, agility);
        let tpv = creature_ref
            .movement_category
            .tpv_for_edge_type(edge_type, base_tpv)
            .unwrap_or(base_tpv);
        let dx = dest_pos.x - old_pos.x;
        let dy = dest_pos.y - old_pos.y;
        let dz = dest_pos.z - old_pos.z;
        let distance = crate::nav::scaled_distance(dx, dy, dz);
        let delay = (distance as u64 * tpv)
            .div_ceil(crate::nav::DIST_SCALE as u64)
            .max(1);

        // Move creature to the destination.
        let tick = self.tick;
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.position = creature.position.with_anchor(dest_pos);

            // Set action state.
            creature.action_kind = ActionKind::Move;
            creature.next_available_tick = Some(tick + delay);
            let _ = self.db.update_creature(creature);
        }

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

    /// Random wander: pick a random walkable neighbor and move one step.
    pub(crate) fn ground_random_wander(
        &mut self,
        creature_id: CreatureId,
        current_pos: VoxelCoord,
        species: Species,
    ) {
        let creature_ref = self.db.creatures.get(&creature_id).unwrap();
        let category = creature_ref.movement_category;
        let species_data = &self.species_table[&species];
        let footprint = species_data.footprint;
        let can_climb = category.can_climb();
        let zone_id = self
            .db
            .creatures
            .get(&creature_id)
            .unwrap()
            .zone_id
            .unwrap();

        // Collect walkable neighbors via ground_neighbors, respecting edge-type filtering.
        let from_surface = {
            let zone = self.voxel_zone(zone_id).unwrap();
            crate::walkability::derive_surface_type(zone, &zone.face_data, current_pos)
        };
        let mut eligible: Vec<VoxelCoord> = Vec::new();
        let neighbors: Vec<_> = {
            let zone = self.voxel_zone(zone_id).unwrap();
            crate::walkability::ground_neighbors(
                zone,
                &zone.face_data,
                current_pos,
                footprint,
                can_climb,
            )
        };
        for (neighbor, _dist) in neighbors {
            // Check edge type allowed via movement category.
            let to_surface = {
                let zone = self.voxel_zone(zone_id).unwrap();
                crate::walkability::derive_surface_type(zone, &zone.face_data, neighbor)
            };
            let edge_type = crate::walkability::derive_edge_type(
                from_surface,
                to_surface,
                current_pos,
                neighbor,
            );
            // Use a dummy base_tpv=1 — we only care about None vs Some for filtering.
            if category.tpv_for_edge_type(edge_type, 1).is_none() {
                continue;
            }
            eligible.push(neighbor);
        }

        if eligible.is_empty() {
            self.set_creature_activation_tick(creature_id, self.tick + 1000);
            return;
        }

        // Voxel exclusion: filter out neighbors occupied by hostiles.
        let unblocked: Vec<VoxelCoord> = eligible
            .iter()
            .copied()
            .filter(|&dest| !self.destination_blocked_by_hostile(creature_id, dest, footprint))
            .collect();

        if unblocked.is_empty() {
            self.set_creature_activation_tick(
                creature_id,
                self.tick + self.config.voxel_exclusion_retry_ticks,
            );
            return;
        }

        // Pick a random eligible neighbor.
        let chosen_idx = self.rng.range_u64(0, unblocked.len() as u64) as usize;
        let dest_pos = unblocked[chosen_idx];

        self.ground_move_one_step(creature_id, species, dest_pos);
    }

    // -----------------------------------------------------------------------
    // Flying creature movement (pathfinding.rs — A* on voxel grid)
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
        let zone_id = creature.zone_id.unwrap();
        let species = creature.species;
        if !creature.movement_category.is_flyer() {
            return false;
        }
        let agility = self.trait_int(creature_id, TraitKind::Agility, 0);
        let flight_tpv = crate::stats::creature_base_tpv(
            self.species_table[&species].move_ticks_per_voxel,
            agility,
        );
        // Check if we have a cached flight path with remaining steps.
        let next_pos = if let Some(ref path) = creature.path {
            if let Some(&next) = path.remaining_positions.first() {
                // Validate that the next position is still flyable (full footprint).
                let footprint = self.species_table[&species].footprint;
                if !crate::pathfinding::footprint_flyable(
                    self.voxel_zone(zone_id).unwrap(),
                    next,
                    footprint,
                ) {
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
            let path_result = self.find_path(
                creature_id,
                target_pos,
                &crate::pathfinding::PathOpts::default(),
            );
            let path_result = match path_result {
                Ok(r) if r.positions.len() >= 2 => r,
                _ => return false, // no path
            };

            let next = path_result.positions[1];

            // Store remaining path (skip start, which is current position).
            let remaining_positions: Vec<VoxelCoord> = path_result.positions[1..].to_vec();
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
            Some(c) => c.position.min,
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
            creature.position = creature.position.with_anchor(dest_pos);
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
        let zone_id = creature.zone_id.unwrap();
        let species = creature.species;
        if !creature.movement_category.is_flyer() {
            return;
        }
        let agility = self.trait_int(creature_id, TraitKind::Agility, 0);
        let flight_tpv = crate::stats::creature_base_tpv(
            self.species_table[&species].move_ticks_per_voxel,
            agility,
        );
        let pos = creature.position.min;

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
            if crate::pathfinding::footprint_flyable(
                self.voxel_zone(zone_id).unwrap(),
                neighbor,
                footprint,
            ) {
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
