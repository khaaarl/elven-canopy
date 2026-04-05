// Wild herbivore grazing — grass search, graze resolution, and regrowth.
//
// Implements F-wild-grazing: herbivorous creatures autonomously graze on
// grassy dirt surfaces when hungry. By default, any exposed dirt voxel
// (dirt with air above it) is considered grassy. The `grassless` set on
// `SimState` stores the exceptions — dirt voxels that have been grazed
// or freshly exposed by voxel changes. A periodic regrowth sweep
// probabilistically removes entries from the grassless set, allowing
// grass to grow back.
//
// Key functions:
// - `find_nearest_grass()` — Dijkstra search from a creature's position,
//   checking adjacent surface dirt for grassiness (not in grassless set).
// - `resolve_graze_action()` — restores food, adds voxel to grassless set,
//   marks dirty for mesh regeneration.
// - `process_grass_regrowth()` — periodic sweep that randomly regrows
//   grassless dirt voxels.
// - `is_grassy_dirt()` — checks if a voxel is exposed dirt not in the
//   grassless set.
//
// See also: `needs.rs` (hunger-driven task creation), `activation.rs`
// (task behavior dispatch), `elven_canopy_graphics::mesh_gen` (grassless dirt coloring),
// `species.rs` (`is_grazer`, `graze_food_restore_pct`).

use super::*;

impl SimState {
    /// Check whether a dirt voxel is "grassy" — exposed dirt (air above) that
    /// is not in the grassless set.
    pub(crate) fn is_grassy_dirt(&self, coord: VoxelCoord) -> bool {
        let vt = self.world.get(coord);
        if vt != VoxelType::Dirt {
            return false;
        }
        let above = VoxelCoord::new(coord.x, coord.y + 1, coord.z);
        let above_vt = self.world.get(above);
        if above_vt.is_solid() {
            return false;
        }
        !self.grassless.contains(&coord)
    }

    /// Find the nearest reachable grassy dirt surface for a creature, using
    /// Dijkstra over the walkable voxel grid. For each position visited,
    /// checks the surface below for grassiness.
    /// Returns the grass voxel coordinate, or `None` if no grass is reachable.
    ///
    /// This uses a bespoke inline Dijkstra rather than the standard
    /// `find_nearest` API because grass is so astronomically numerous
    /// (nearly every exposed dirt voxel on the ground floor) that
    /// enumerating all grass positions into a candidate list would itself
    /// be prohibitive. Instead, we expand outward from the creature and
    /// test each visited position on the fly. Since grass is almost
    /// everywhere, Dijkstra terminates in very few expansions — typically
    /// the first or second ground-level position is grassy.
    pub(crate) fn find_nearest_grass(&self, creature_id: CreatureId) -> Option<VoxelCoord> {
        let creature = self.db.creatures.get(&creature_id)?;
        let start = creature.position.min;
        let species_data = &self.species_table[&creature.species];
        let footprint = species_data.footprint;
        let can_climb = species_data.climb_ticks_per_voxel.is_some();
        if !crate::walkability::footprint_walkable(
            &self.world,
            &self.face_data,
            start,
            footprint,
            can_climb,
        ) {
            return None;
        }

        // Custom Dijkstra over the voxel grid that stops at the first grassy
        // position. Uses `ground_neighbors` for neighbor expansion (handles
        // walkability, face-blocking, and large-creature surface snapping).
        let mut dist: std::collections::BTreeMap<VoxelCoord, u64> =
            std::collections::BTreeMap::new();
        let mut heap = std::collections::BinaryHeap::new();

        dist.insert(start, 0);
        heap.push(std::cmp::Reverse((0u64, start)));

        while let Some(std::cmp::Reverse((cost, pos))) = heap.pop() {
            if dist.get(&pos).is_some_and(|&d| cost > d) {
                continue;
            }

            // Check if the surface below this position is grassy dirt.
            let surface_below = VoxelCoord::new(pos.x, pos.y - 1, pos.z);
            if self.is_grassy_dirt(surface_below) {
                return Some(surface_below);
            }

            // Expand neighbors via ground_neighbors (handles walkability,
            // face-blocking, and large-creature surface snapping).
            for (neighbor, dist_scaled) in crate::walkability::ground_neighbors(
                &self.world,
                &self.face_data,
                pos,
                footprint,
                can_climb,
            ) {
                // Derive edge type to check species restrictions and speed.
                let from_surface =
                    crate::walkability::derive_surface_type(&self.world, &self.face_data, pos);
                let to_surface =
                    crate::walkability::derive_surface_type(&self.world, &self.face_data, neighbor);
                let edge_type =
                    crate::walkability::derive_edge_type(from_surface, to_surface, pos, neighbor);

                // Check edge type allowed.
                if let Some(allowed) = &species_data.allowed_edge_types
                    && !allowed.contains(&edge_type)
                {
                    continue;
                }

                let tpv = match edge_type {
                    crate::nav::EdgeType::TrunkClimb | crate::nav::EdgeType::GroundToTrunk => {
                        match species_data.climb_ticks_per_voxel {
                            Some(t) => t,
                            None => continue,
                        }
                    }
                    crate::nav::EdgeType::WoodLadderClimb => match species_data.wood_ladder_tpv {
                        Some(t) => t,
                        None => continue,
                    },
                    crate::nav::EdgeType::RopeLadderClimb => match species_data.rope_ladder_tpv {
                        Some(t) => t,
                        None => continue,
                    },
                    _ => species_data.walk_ticks_per_voxel,
                };
                let edge_cost = dist_scaled * tpv;
                let new_cost = cost.saturating_add(edge_cost);

                if !dist.contains_key(&neighbor) || new_cost < dist[&neighbor] {
                    dist.insert(neighbor, new_cost);
                    heap.push(std::cmp::Reverse((new_cost, neighbor)));
                }
            }
        }

        None
    }

    /// Resolve a completed Graze action: restore food, add the grazed voxel
    /// to the grassless set, mark the voxel dirty for mesh regeneration,
    /// and complete the task. Always returns true.
    pub(crate) fn resolve_graze_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        grass_pos: VoxelCoord,
    ) -> bool {
        // Restore food.
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            let species_data = &self.species_table[&creature.species];
            let restore = species_data.food_max * species_data.graze_food_restore_pct / 100;
            let food_max = species_data.food_max;
            creature.food = (creature.food + restore).min(food_max);
            let _ = self.db.update_creature(creature);
        }

        // Add to grassless set.
        self.grassless.insert(grass_pos);

        // Mark dirty for mesh regeneration. We don't change the actual voxel
        // type (it's still Dirt), but the mesh color changes, so we need the
        // chunk to be regenerated.
        self.world.mark_dirty(grass_pos);

        self.complete_task(task_id);
        true
    }

    /// Periodic grass regrowth sweep. Iterates all grassless dirt voxels and
    /// probabilistically regrows each one (removes from the grassless set).
    /// Uses the sim PRNG for determinism.
    pub(crate) fn process_grass_regrowth(&mut self) {
        let chance_pct = self.config.grass_regrowth_chance_pct;
        if chance_pct == 0 || self.grassless.is_empty() {
            return;
        }

        // Collect coords to potentially regrow. We iterate the full set
        // deterministically (BTreeSet iteration is ordered).
        let coords: Vec<VoxelCoord> = self.grassless.iter().copied().collect();

        for coord in coords {
            // Roll regrowth chance.
            let roll = self.rng.next_u32() % 100;
            if roll < chance_pct {
                self.grassless.remove(&coord);
                // Mark dirty for mesh regeneration.
                self.world.mark_dirty(coord);
            }
        }
    }

    /// Backfill for saves that predate F-wild-grazing: schedule a
    /// `GrassRegrowth` event if one isn't already in the queue.
    pub(crate) fn backfill_grass_regrowth_event(&mut self) {
        let has_regrowth = self
            .event_queue
            .iter()
            .any(|e| matches!(e.kind, crate::event::ScheduledEventKind::GrassRegrowth));
        if !has_regrowth {
            let next_tick = self.tick + self.config.grass_regrowth_interval_ticks;
            self.event_queue
                .schedule(next_tick, crate::event::ScheduledEventKind::GrassRegrowth);
        }
    }

    /// Set a voxel and check for freshly exposed dirt. Wraps `world.set()`
    /// with grass-awareness: if a solid voxel is replaced by a non-solid one,
    /// the dirt voxel below (if any) becomes freshly exposed and is added to
    /// the grassless set. Also handles the reverse: if a non-solid voxel is
    /// replaced by a solid one, dirt directly below it is no longer exposed
    /// and is removed from the grassless set (it will re-enter as grassless
    /// if later re-exposed).
    ///
    /// All sim-side voxel mutations should use this instead of raw
    /// `world.set()` to keep the grassless set consistent.
    pub(crate) fn set_voxel(&mut self, coord: VoxelCoord, new_type: VoxelType) {
        let old_type = self.world.get(coord);
        self.world.set(coord, new_type);

        // Case 1: solid→non-solid at coord — dirt below may be freshly exposed.
        if old_type.is_solid() && !new_type.is_solid() {
            let below = VoxelCoord::new(coord.x, coord.y - 1, coord.z);
            if self.world.get(below) == VoxelType::Dirt {
                self.grassless.insert(below);
                self.world.mark_dirty(below);
            }
        }

        // Case 2: non-solid→solid at coord — dirt below is now covered,
        // remove from grassless (no longer visible).
        if !old_type.is_solid() && new_type.is_solid() {
            let below = VoxelCoord::new(coord.x, coord.y - 1, coord.z);
            if self.grassless.remove(&below) {
                self.world.mark_dirty(below);
            }
        }
    }
}
