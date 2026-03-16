// Creature needs — eating, sleeping, moping, item acquisition, and military equipment.
//
// Handles the creature want system: hunger (eat fruit or bread), tiredness
// (find bed and sleep), mood consequences (moping when unhappy), personal
// item acquisition (fetching items to satisfy inventory wants), and military
// equipment acquisition (fetching items for group equipment wants without
// changing ownership). These are triggered by the creature heartbeat in
// `process_event`.
//
// See also: `activation.rs` (`check_creature_wants`, `check_mope`,
// `military_equipment_drop`, `check_military_equipment_wants`),
// `inventory_mgmt.rs` (item operations), `greenhouse.rs` (fruit sources).
use super::*;
use crate::db::ActionKind;
use crate::event::ScheduledEventKind;
use crate::inventory;
use crate::pathfinding;
use crate::task;

impl SimState {
    /// Find the nearest reachable fruit for a creature, using Dijkstra over the
    /// nav graph with the creature's species-specific speeds and edge restrictions.
    ///
    /// Returns the fruit voxel coordinate and its nearest nav node, or `None`
    /// if no fruit exists or none is reachable by this creature.
    pub(crate) fn find_nearest_fruit(
        &self,
        creature_id: CreatureId,
    ) -> Option<(VoxelCoord, NavNodeId)> {
        let creature = self.db.creatures.get(&creature_id)?;
        let start_node = creature.current_node?;
        let species_data = &self.species_table[&creature.species];
        let graph = self.graph_for_species(creature.species);

        // Map each fruit position to its nearest nav node, keeping the association.
        let mut nav_to_fruit: Vec<(NavNodeId, VoxelCoord)> = Vec::new();
        let mut target_nodes: Vec<NavNodeId> = Vec::new();
        for tree in self.trees.values() {
            for &fruit_pos in &tree.fruit_positions {
                if let Some(nav_node) = graph.find_nearest_node(fruit_pos) {
                    target_nodes.push(nav_node);
                    nav_to_fruit.push((nav_node, fruit_pos));
                }
            }
        }

        if target_nodes.is_empty() {
            return None;
        }

        let nearest_node = pathfinding::dijkstra_nearest(
            graph,
            start_node,
            &target_nodes,
            species_data.walk_ticks_per_voxel,
            species_data.climb_ticks_per_voxel,
            species_data.wood_ladder_tpv,
            species_data.rope_ladder_tpv,
            species_data.allowed_edge_types.as_deref(),
        )?;

        // Find the fruit_pos associated with this nav node.
        let fruit_pos = nav_to_fruit
            .iter()
            .find(|(n, _)| *n == nearest_node)
            .map(|(_, fp)| *fp)?;

        Some((fruit_pos, nearest_node))
    }

    /// Resolve a completed Eat action for fruit: restore food, remove fruit
    /// from world, generate thought, complete task. Always returns true.
    pub(crate) fn resolve_eat_fruit_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        fruit_pos: VoxelCoord,
    ) -> bool {
        // Restore food.
        if let Some(creature) = self.db.creatures.get(&creature_id) {
            let species_data = &self.species_table[&creature.species];
            let restore = species_data.food_max * species_data.food_restore_pct / 100;
            let food_max = species_data.food_max;
            let _ = self
                .db
                .creatures
                .modify_unchecked(&creature_id, |creature| {
                    creature.food = (creature.food + restore).min(food_max);
                });
        }

        // Remove fruit from world, tree's fruit_positions, and species map.
        if self.world.get(fruit_pos) == VoxelType::Fruit {
            self.world.set(fruit_pos, VoxelType::Air);
        }
        self.fruit_voxel_species.remove(&fruit_pos);
        self.fruit_voxel_species_list
            .retain(|(pos, _)| *pos != fruit_pos);
        for tree in self.trees.values_mut() {
            tree.fruit_positions.retain(|&p| p != fruit_pos);
        }

        // Generate AteMeal thought.
        self.add_creature_thought(creature_id, ThoughtKind::AteMeal);

        self.complete_task(task_id);
        true
    }

    /// Resolve a completed Harvest action: remove fruit voxel, create ground
    /// pile, complete task. Always returns true.
    pub(crate) fn resolve_harvest_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        fruit_pos: VoxelCoord,
    ) -> bool {
        // Check fruit still exists.
        let fruit_exists = self.world.get(fruit_pos) == VoxelType::Fruit;

        if fruit_exists {
            // Look up species before removing from the map.
            let species_id = self.fruit_voxel_species.remove(&fruit_pos);
            self.fruit_voxel_species_list
                .retain(|(pos, _)| *pos != fruit_pos);
            let material = species_id.map(inventory::Material::FruitSpecies);

            // Remove fruit from world and tree's fruit_positions list.
            self.world.set(fruit_pos, VoxelType::Air);
            for tree in self.trees.values_mut() {
                tree.fruit_positions.retain(|&p| p != fruit_pos);
            }

            // Create ground pile at creature's position with species material.
            if let Some(creature) = self.db.creatures.get(&creature_id) {
                let pile_pos = creature.position;
                let pile_id = self.ensure_ground_pile(pile_pos);
                let pile = self.db.ground_piles.get(&pile_id).unwrap();
                self.inv_add_item(
                    pile.inventory_id,
                    inventory::ItemKind::Fruit,
                    1,
                    None,     // owner
                    None,     // reserved_by
                    material, // fruit species
                    0,        // quality
                    None,     // enchantment
                    None,     // equipped_slot
                );
            }
        }

        self.complete_task(task_id);
        true
    }

    /// Resolve a completed AcquireItem action: split reserved items from
    /// source inventory, move to creature inventory preserving all properties
    /// (material, quality, etc.), set ownership, and auto-equip clothing.
    pub(crate) fn resolve_acquire_item_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
    ) -> bool {
        let acquire = match self.task_acquire_data(task_id) {
            Some(a) => a,
            None => return false,
        };
        let source = match self.task_acquire_source(task_id, acquire.source_kind) {
            Some(s) => s,
            None => return false,
        };
        let item_kind = acquire.item_kind;
        let quantity = acquire.quantity;

        // Find the source inventory.
        let source_inv = match &source {
            task::HaulSource::GroundPile(pos) => self
                .db
                .ground_piles
                .by_position(pos, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
                .map(|pile| pile.inventory_id),
            task::HaulSource::Building(sid) => self.db.structures.get(sid).map(|s| s.inventory_id),
        };
        let source_inv = match source_inv {
            Some(inv) => inv,
            None => {
                self.complete_task(task_id);
                return true;
            }
        };

        // Find reserved stacks and split+move them to creature inventory.
        let creature_inv = self.creature_inv(creature_id);
        let stacks: Vec<crate::db::ItemStack> = self
            .db
            .item_stacks
            .by_inventory_id(&source_inv, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut moved_ids: Vec<ItemStackId> = Vec::new();
        for stack in &stacks {
            if stack.kind != item_kind || stack.reserved_by != Some(task_id) || remaining == 0 {
                continue;
            }
            let take = remaining.min(stack.quantity);
            if let Some(split_id) = self.inv_split_stack(stack.id, take) {
                // Move the split stack to creature inventory (uses update_no_fk
                // because inventory_id is an indexed field).
                if let Some(mut moved) = self.db.item_stacks.get(&split_id) {
                    moved.inventory_id = creature_inv;
                    moved.owner = Some(creature_id);
                    moved.reserved_by = None;
                    let _ = self.db.item_stacks.update_no_fk(moved);
                }
                moved_ids.push(split_id);
                remaining -= take;
            }
        }

        // Auto-equip clothing if slot is unoccupied.
        let equip_slot = item_kind.equip_slot();
        if let Some(slot) = equip_slot {
            let slot_occupied = self.inv_equipped_in_slot(creature_inv, slot).is_some();
            if !slot_occupied {
                // Equip the first moved stack (split to qty 1 if needed).
                if let Some(&first_id) = moved_ids.first() {
                    let equip_target = if let Some(stack) = self.db.item_stacks.get(&first_id) {
                        if stack.quantity == 1 {
                            Some(first_id)
                        } else {
                            self.inv_split_stack(first_id, 1)
                        }
                    } else {
                        None
                    };
                    if let Some(equip_id) = equip_target
                        && let Some(mut s) = self.db.item_stacks.get(&equip_id)
                    {
                        s.equipped_slot = Some(slot);
                        let _ = self.db.item_stacks.update_no_fk(s);
                    }
                }
            }
        }

        // Normalize both inventories.
        self.inv_normalize(source_inv);
        self.inv_normalize(creature_inv);

        // Clean up empty ground piles.
        if let task::HaulSource::GroundPile(pos) = &source
            && let Some(pile) = self
                .db
                .ground_piles
                .by_position(pos, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
            && self.inv_items(pile.inventory_id).is_empty()
        {
            let _ = self.db.ground_piles.remove_no_fk(&pile.id);
        }

        self.complete_task(task_id);
        true
    }

    /// Clean up an AcquireItem task on abandonment: clear reservations at
    /// the source. No items are in transit (pickup only happens on arrival).
    pub(crate) fn cleanup_acquire_item_task(&mut self, task_id: TaskId) {
        let acquire = match self.task_acquire_data(task_id) {
            Some(a) => a,
            None => return,
        };
        let source = match self.task_acquire_source(task_id, acquire.source_kind) {
            Some(s) => s,
            None => return,
        };
        match &source {
            task::HaulSource::GroundPile(pos) => {
                if let Some(pile) = self
                    .db
                    .ground_piles
                    .by_position(pos, tabulosity::QueryOpts::ASC)
                    .into_iter()
                    .next()
                {
                    self.inv_clear_reservations(pile.inventory_id, task_id);
                }
            }
            task::HaulSource::Building(sid) => {
                if let Some(structure) = self.db.structures.get(sid) {
                    self.inv_clear_reservations(structure.inventory_id, task_id);
                }
            }
        }
        if let Some(mut t) = self.db.tasks.get(&task_id) {
            t.state = task::TaskState::Complete;
            let _ = self.db.tasks.update_no_fk(t);
        }
    }

    /// Resolve a completed AcquireMilitaryEquipment action: same as
    /// `resolve_acquire_item_action` but does NOT change ownership.
    /// Wearable items are force-equipped (unequipping whatever was in the slot).
    pub(crate) fn resolve_acquire_military_equipment_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
    ) -> bool {
        let acquire = match self.task_acquire_data(task_id) {
            Some(a) => a,
            None => return false,
        };
        let source = match self.task_acquire_source(task_id, acquire.source_kind) {
            Some(s) => s,
            None => return false,
        };
        let item_kind = acquire.item_kind;
        let quantity = acquire.quantity;

        // Find the source inventory.
        let source_inv = match &source {
            task::HaulSource::GroundPile(pos) => self
                .db
                .ground_piles
                .by_position(pos, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
                .map(|pile| pile.inventory_id),
            task::HaulSource::Building(sid) => self.db.structures.get(sid).map(|s| s.inventory_id),
        };
        let source_inv = match source_inv {
            Some(inv) => inv,
            None => {
                self.complete_task(task_id);
                return true;
            }
        };

        // Find reserved stacks and split+move them to creature inventory.
        // Unlike AcquireItem, ownership is NOT changed.
        let creature_inv = self.creature_inv(creature_id);
        let stacks: Vec<crate::db::ItemStack> = self
            .db
            .item_stacks
            .by_inventory_id(&source_inv, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut moved_ids: Vec<ItemStackId> = Vec::new();
        for stack in &stacks {
            if stack.kind != item_kind || stack.reserved_by != Some(task_id) || remaining == 0 {
                continue;
            }
            let take = remaining.min(stack.quantity);
            if let Some(split_id) = self.inv_split_stack(stack.id, take) {
                if let Some(mut moved) = self.db.item_stacks.get(&split_id) {
                    moved.inventory_id = creature_inv;
                    moved.reserved_by = None;
                    // ownership unchanged — stays None or whatever it was
                    let _ = self.db.item_stacks.update_no_fk(moved);
                }
                moved_ids.push(split_id);
                remaining -= take;
            }
        }

        // Auto-equip wearable military equipment, displacing existing items.
        // Non-wearable items (no equip_slot) are skipped.
        if item_kind.equip_slot().is_some()
            && let Some(&first_id) = moved_ids.first()
        {
            let equip_target = if let Some(stack) = self.db.item_stacks.get(&first_id) {
                if stack.quantity == 1 {
                    Some(first_id)
                } else {
                    self.inv_split_stack(first_id, 1)
                }
            } else {
                None
            };
            if let Some(equip_id) = equip_target {
                self.inv_force_equip_item(equip_id);
            }
        }

        // Normalize both inventories.
        self.inv_normalize(source_inv);
        self.inv_normalize(creature_inv);

        // Clean up empty ground piles.
        if let task::HaulSource::GroundPile(pos) = &source
            && let Some(pile) = self
                .db
                .ground_piles
                .by_position(pos, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
            && self.inv_items(pile.inventory_id).is_empty()
        {
            let _ = self.db.ground_piles.remove_no_fk(&pile.id);
        }

        self.complete_task(task_id);
        true
    }

    /// Clean up an AcquireMilitaryEquipment task on abandonment: clear
    /// reservations at the source (same logic as `cleanup_acquire_item_task`).
    pub(crate) fn cleanup_acquire_military_equipment_task(&mut self, task_id: TaskId) {
        self.cleanup_acquire_item_task(task_id);
    }

    /// Resolve a completed EatBread action: remove 1 owned bread, restore food,
    /// generate thought, complete task. Always returns true.
    pub(crate) fn resolve_eat_bread_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
    ) -> bool {
        if let Some(creature) = self.db.creatures.get(&creature_id) {
            let species_data = &self.species_table[&creature.species];
            let restore = species_data.food_max * species_data.bread_restore_pct / 100;
            let food_max = species_data.food_max;
            self.inv_remove_owned_item(
                creature.inventory_id,
                inventory::ItemKind::Bread,
                creature_id,
                1,
            );
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.food = (c.food + restore).min(food_max);
            });
        }

        // Generate AteMeal thought.
        self.add_creature_thought(creature_id, ThoughtKind::AteMeal);

        self.complete_task(task_id);
        true
    }

    /// Find the bed in the creature's assigned home, if any.
    ///
    /// Returns `None` if the creature has no assigned home, the home isn't a
    /// Home, or the home has no placed furniture (bed not yet built). Does NOT
    /// check occupied-bed exclusion — it's the elf's personal bed.
    /// Returns `(bed_pos, nav_node, structure_id)`.
    pub(crate) fn find_assigned_home_bed(
        &self,
        creature_id: CreatureId,
    ) -> Option<(VoxelCoord, NavNodeId, StructureId)> {
        let creature = self.db.creatures.get(&creature_id)?;
        let home_id = creature.assigned_home?;
        let structure = self.db.structures.get(&home_id)?;
        if structure.furnishing != Some(FurnishingType::Home) {
            return None;
        }
        let bed = self
            .db
            .furniture
            .by_structure_id(&home_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|f| f.placed)?;
        let graph = self.graph_for_species(creature.species);
        let nav_node = graph.find_nearest_node(bed.coord)?;
        Some((bed.coord, nav_node, home_id))
    }

    /// Find the nearest reachable dormitory bed for a creature, using Dijkstra
    /// over the nav graph with species-specific speeds and edge restrictions.
    ///
    /// Excludes beds already occupied by an active Sleep task. Returns the bed
    /// position, its nearest nav node, and the structure ID, or `None` if no
    /// unoccupied beds exist or none are reachable.
    pub(crate) fn find_nearest_bed(
        &self,
        creature_id: CreatureId,
    ) -> Option<(VoxelCoord, NavNodeId, StructureId)> {
        let creature = self.db.creatures.get(&creature_id)?;
        let start_node = creature.current_node?;
        let species_data = &self.species_table[&creature.species];
        let graph = self.graph_for_species(creature.species);

        // Collect all occupied bed positions from active Sleep tasks.
        let occupied_beds: Vec<VoxelCoord> = self
            .db
            .task_voxel_refs
            .iter_all()
            .filter(|r| r.role == crate::db::TaskVoxelRole::BedPosition)
            .filter(|r| {
                self.db
                    .tasks
                    .get(&r.task_id)
                    .is_some_and(|t| t.state != task::TaskState::Complete)
            })
            .map(|r| r.coord)
            .collect();

        // Collect unoccupied bed positions from all dormitory structures.
        let mut nav_to_bed: Vec<(NavNodeId, VoxelCoord, StructureId)> = Vec::new();
        let mut target_nodes: Vec<NavNodeId> = Vec::new();
        for structure in self.db.structures.iter_all() {
            if structure.furnishing != Some(FurnishingType::Dormitory) {
                continue;
            }
            for furn in self
                .db
                .furniture
                .by_structure_id(&structure.id, tabulosity::QueryOpts::ASC)
            {
                if !furn.placed || occupied_beds.contains(&furn.coord) {
                    continue;
                }
                if let Some(nav_node) = graph.find_nearest_node(furn.coord) {
                    target_nodes.push(nav_node);
                    nav_to_bed.push((nav_node, furn.coord, structure.id));
                }
            }
        }

        if target_nodes.is_empty() {
            return None;
        }

        let nearest_node = pathfinding::dijkstra_nearest(
            graph,
            start_node,
            &target_nodes,
            species_data.walk_ticks_per_voxel,
            species_data.climb_ticks_per_voxel,
            species_data.wood_ladder_tpv,
            species_data.rope_ladder_tpv,
            species_data.allowed_edge_types.as_deref(),
        )?;

        let (_, bed_pos, structure_id) = nav_to_bed.iter().find(|(n, _, _)| *n == nearest_node)?;

        Some((*bed_pos, nearest_node, *structure_id))
    }

    /// Start a Sleep action: set action kind and schedule next activation
    /// after `sleep_action_ticks`. On first action, check for low ceiling.
    pub(crate) fn start_sleep_action(&mut self, creature_id: CreatureId, task_id: TaskId) {
        let duration = self.config.sleep_action_ticks;

        // On first sleep action, check for low ceiling.
        let progress = self
            .db
            .tasks
            .get(&task_id)
            .map(|t| t.progress)
            .unwrap_or(0.0);
        if progress == 0.0 {
            let location = self.task_sleep_location(task_id);
            if let Some(location) = &location {
                let structure_id = match location {
                    task::SleepLocation::Home(sid) | task::SleepLocation::Dormitory(sid) => {
                        Some(*sid)
                    }
                    task::SleepLocation::Ground => None,
                };
                if let Some(sid) = structure_id
                    && let Some(structure) = self.db.structures.get(&sid)
                    && structure.build_type == BuildType::Building
                    && structure.height == 1
                {
                    self.add_creature_thought(creature_id, ThoughtKind::LowCeiling(sid));
                }
            }
        }

        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Sleep;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Sleep action: restore rest, increment progress,
    /// check for completion or rest full. Returns true if task completed.
    pub(crate) fn resolve_sleep_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };

        // Restore rest: use rest_per_sleep_tick * sleep_action_ticks to get
        // the per-action restore amount (preserves total balance).
        if let Some(creature) = self.db.creatures.get(&creature_id) {
            let species_data = &self.species_table[&creature.species];
            let rest_max = species_data.rest_max;
            let restore = species_data.rest_per_sleep_tick * self.config.sleep_action_ticks as i64;
            let _ = self
                .db
                .creatures
                .modify_unchecked(&creature_id, |creature| {
                    creature.rest = (creature.rest + restore).min(rest_max);
                });
        }

        // Increment progress by 1 (one action).
        let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
            t.progress += 1.0;
        });

        // Check if done by progress or rest full.
        let done = self
            .db
            .tasks
            .get(&task_id)
            .is_some_and(|t| t.progress >= t.total_cost);

        let rest_full = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| {
                let species_data = &self.species_table[&c.species];
                c.rest >= species_data.rest_max
            })
            .unwrap_or(false);

        if done || rest_full {
            let location = self.task_sleep_location(task_id);
            if let Some(location) = &location {
                let thought_kind = match location {
                    task::SleepLocation::Home(sid) => ThoughtKind::SleptInOwnHome(*sid),
                    task::SleepLocation::Dormitory(sid) => ThoughtKind::SleptInDormitory(*sid),
                    task::SleepLocation::Ground => ThoughtKind::SleptOnGround,
                };
                self.add_creature_thought(creature_id, thought_kind);
            }
            self.complete_task(task_id);
            return true;
        }
        false
    }

    /// Start a Mope action: set action kind and schedule next activation
    /// after `mope_action_ticks`.
    pub(crate) fn start_mope_action(&mut self, creature_id: CreatureId) {
        let duration = self.config.mope_action_ticks;
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Mope;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Mope action: increment progress by
    /// `mope_action_ticks`, check for completion. Returns true if done.
    pub(crate) fn resolve_mope_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };

        let increment = self.config.mope_action_ticks as f32;
        let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
            t.progress += increment;
        });

        let done = self
            .db
            .tasks
            .get(&task_id)
            .is_some_and(|t| t.progress >= t.total_cost);

        if done {
            self.complete_task(task_id);
            return true;
        }
        false
    }
}
