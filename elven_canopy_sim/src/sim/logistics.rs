// Logistics system — hauling, harvesting, and item flow management.
//
// Drives the automatic movement of items between inventories: harvest tasks
// (fruit collection), haul tasks (moving items between structures/piles),
// pickup/dropoff actions, and the logistics heartbeat that creates new haul
// tasks based on active recipe demands and inventory wants.
//
// See also: `crafting.rs` (recipe demand computation), `inventory_mgmt.rs`
// (item reservation and transfer), `needs.rs` (personal item acquisition).
use super::*;
use crate::db::ActionKind;
use crate::event::ScheduledEventKind;
use crate::inventory;
use crate::task;

impl SimState {
    /// Start a PickUp action (haul source pickup): set action kind and
    /// schedule next activation after `haul_pickup_action_ticks`.
    pub(crate) fn start_pickup_action(&mut self, creature_id: CreatureId) {
        let duration = self.config.haul_pickup_action_ticks;
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::PickUp;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed PickUp action: remove reserved items from source,
    /// add to creature inventory, switch haul phase to GoingToDestination.
    /// Returns true if task completed (source empty → cancelled).
    pub(crate) fn resolve_pickup_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let haul = match self.task_haul_data(task_id) {
            Some(h) => h,
            None => return false,
        };
        let source = match self.task_haul_source(task_id, haul.source_kind) {
            Some(s) => s,
            None => return false,
        };
        let item_kind = haul.item_kind;
        let quantity = haul.quantity;
        let destination_coord = haul.destination_coord;

        // Pick up reserved items from source by moving them to creature
        // inventory. This preserves all item properties (durability, etc.).
        let source_inv = match &source {
            task::HaulSource::GroundPile(pos) => self
                .db
                .ground_piles
                .by_position(pos, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
                .map(|p| p.inventory_id),
            task::HaulSource::Building(sid) => self.db.structures.get(sid).map(|s| s.inventory_id),
        };
        let creature_inv = self.creature_inv(creature_id);
        let picked_up = if let Some(src_inv) = source_inv {
            // Find stacks reserved by this task and move them.
            let stacks: Vec<_> = self
                .db
                .item_stacks
                .by_inventory_id(&src_inv, tabulosity::QueryOpts::ASC);
            let mut remaining = quantity;
            let mut moved = 0u32;
            for stack in &stacks {
                if stack.kind == item_kind && stack.reserved_by == Some(task_id) && remaining > 0 {
                    let take = remaining.min(stack.quantity);
                    self.inv_move_stack(stack.id, take, creature_inv);
                    remaining -= take;
                    moved += take;
                }
            }
            // Clear reservation on picked-up items (reservation was for the
            // source inventory; items are now carried by the creature).
            self.inv_clear_reservations(creature_inv, task_id);
            moved
        } else {
            0
        };

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

        if picked_up == 0 {
            // Source empty — cancel task.
            self.complete_task(task_id);
            return true;
        }

        // Switch to GoingToDestination phase.
        let mut updated_haul = haul.clone();
        updated_haul.phase = task::HaulPhase::GoingToDestination;
        updated_haul.quantity = picked_up;
        let _ = self.db.task_haul_data.update_no_fk(updated_haul);
        // Update task location for the new destination.
        let dest_coord = destination_coord;
        let _ = self.db.tasks.modify_unchecked(&task_id, |task| {
            task.location = dest_coord;
        });
        // Clear cached path so creature re-pathfinds to new destination.
        let _ = self
            .db
            .creatures
            .modify_unchecked(&creature_id, |creature| {
                creature.path = None;
            });
        false
    }

    /// Start a DropOff action (haul destination deposit): set action kind and
    /// schedule next activation after `haul_dropoff_action_ticks`.
    pub(crate) fn start_dropoff_action(&mut self, creature_id: CreatureId) {
        let duration = self.config.haul_dropoff_action_ticks;
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::DropOff;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed DropOff action: deposit items at destination,
    /// complete task. Always returns true.
    pub(crate) fn resolve_dropoff_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let haul = match self.task_haul_data(task_id) {
            Some(h) => h,
            None => return false,
        };
        let destination =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::HaulDestination) {
                Some(d) => d,
                None => return false,
            };
        let item_kind = haul.item_kind;
        let quantity = haul.quantity;

        // Deposit items into destination building, preserving all properties.
        let material = haul.hauled_material;
        let creature_inv = self.creature_inv(creature_id);
        let dst_inv = self.structure_inv(destination);
        self.inv_move_items(
            creature_inv,
            dst_inv,
            Some(item_kind),
            Some(material),
            Some(quantity),
        );
        self.complete_task(task_id);
        true
    }

    /// Clean up haul task state when a haul task is abandoned.
    ///
    /// - **GoingToSource:** Release reserved items at source.
    /// - **GoingToDestination:** Creature is carrying items — drop as ground pile.
    pub(crate) fn cleanup_haul_task(&mut self, creature_id: CreatureId, task_id: TaskId) {
        let haul = match self.task_haul_data(task_id) {
            Some(h) => h,
            None => return,
        };
        let source = match self.task_haul_source(task_id, haul.source_kind) {
            Some(s) => s,
            None => return,
        };
        let item_kind = haul.item_kind;
        let quantity = haul.quantity;
        let phase = haul.phase;

        match phase {
            task::HaulPhase::GoingToSource => {
                // Clear reservations at the source.
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
            }
            task::HaulPhase::GoingToDestination => {
                // Creature is carrying items — drop as ground pile at current position.
                let material = haul.hauled_material;
                if let Some(creature) = self.db.creatures.get(&creature_id) {
                    let pos = creature.position;
                    let creature_inv = creature.inventory_id;
                    let pile_id = self.ensure_ground_pile(pos);
                    let pile_inv = self.db.ground_piles.get(&pile_id).unwrap().inventory_id;
                    self.inv_move_items(
                        creature_inv,
                        pile_inv,
                        Some(item_kind),
                        Some(material),
                        Some(quantity),
                    );
                }
            }
        }
    }

    /// Clean up a Harvest task on node invalidation. Harvest tasks have no
    /// reservations, so just mark the task complete so `process_harvest_tasks`
    /// can create a replacement on the next heartbeat.
    pub(crate) fn cleanup_harvest_task(&mut self, task_id: TaskId) {
        let is_harvest = self
            .db
            .tasks
            .get(&task_id)
            .is_some_and(|t| t.kind_tag == crate::db::TaskKindTag::Harvest);
        if is_harvest && let Some(mut t) = self.db.tasks.get(&task_id) {
            t.state = task::TaskState::Complete;
            let _ = self.db.tasks.update_no_fk(t);
        }
    }

    /// Create Harvest tasks when logistics buildings want fruit but not enough
    /// fruit items exist. Scans all trees for unclaimed fruit voxels and creates
    /// up to `max_haul_tasks_per_heartbeat` Harvest tasks.
    ///
    /// Harvest demand is material-unaware — uses `MaterialFilter::Any` for total
    /// fruit demand vs total fruit supply, because the sim can't direct elves to
    /// harvest a specific species (determined by the voxel).
    pub(crate) fn process_harvest_tasks(&mut self) {
        // 1. Sum total fruit demand across logistics-enabled buildings.
        // Uses compute_effective_wants which includes auto-logistics from
        // active recipes, not just explicit LogisticsWantRow entries.
        let logistics_sids: Vec<(StructureId, InventoryId)> = self
            .db
            .structures
            .iter_all()
            .filter(|s| s.logistics_priority.is_some())
            .map(|s| (s.id, s.inventory_id))
            .collect();
        let mut total_demand: u32 = 0;
        for (sid, inv_id) in &logistics_sids {
            let wants = self.compute_effective_wants(*sid);
            let fruit_target: u32 = wants
                .iter()
                .filter(|w| w.item_kind == inventory::ItemKind::Fruit)
                .map(|w| w.target_quantity)
                .sum();
            if fruit_target > 0 {
                let current = self.inv_item_count(
                    *inv_id,
                    inventory::ItemKind::Fruit,
                    inventory::MaterialFilter::Any,
                );
                let in_transit = self.count_in_transit_items(
                    *sid,
                    inventory::ItemKind::Fruit,
                    inventory::MaterialFilter::Any,
                );
                let effective = current + in_transit;
                if fruit_target > effective {
                    total_demand += fruit_target - effective;
                }
            }
        }

        if total_demand == 0 {
            return;
        }

        // 2. Count fruit already available as items (unreserved in ground piles +
        // logistics-enabled building inventories). Non-logistics buildings are excluded
        // because their fruit can't be hauled out.
        let mut available_items: u32 = 0;
        for pile in self.db.ground_piles.iter_all() {
            available_items += self.inv_unreserved_item_count(
                pile.inventory_id,
                inventory::ItemKind::Fruit,
                inventory::MaterialFilter::Any,
            );
        }
        for structure in self.db.structures.iter_all() {
            if structure.logistics_priority.is_some() {
                available_items += self.inv_unreserved_item_count(
                    structure.inventory_id,
                    inventory::ItemKind::Fruit,
                    inventory::MaterialFilter::Any,
                );
            }
        }

        // 3. Count pending Harvest tasks (non-Complete).
        let being_harvested: u32 = self
            .db
            .tasks
            .iter_all()
            .filter(|t| {
                t.state != task::TaskState::Complete
                    && t.kind_tag == crate::db::TaskKindTag::Harvest
            })
            .count() as u32;

        // 4. Compute shortfall.
        let shortfall = total_demand.saturating_sub(available_items + being_harvested);
        if shortfall == 0 {
            return;
        }

        // 5. Collect unclaimed fruit positions (skip those with existing Harvest or EatFruit tasks).
        // FruitTarget role is shared by both Harvest and EatFruit tasks.
        let claimed_positions: Vec<VoxelCoord> = self
            .db
            .task_voxel_refs
            .iter_all()
            .filter(|r| r.role == crate::db::TaskVoxelRole::FruitTarget)
            .filter(|r| {
                self.db
                    .tasks
                    .get(&r.task_id)
                    .is_some_and(|t| t.state != task::TaskState::Complete)
            })
            .map(|r| r.coord)
            .collect();

        let mut unclaimed_fruit: Vec<(VoxelCoord, NavNodeId)> = Vec::new();
        for tree in self.db.trees.iter_all() {
            for &fruit_pos in &tree.fruit_positions {
                if !claimed_positions.contains(&fruit_pos)
                    && let Some(nav_node) = self.nav_graph.find_nearest_node(fruit_pos)
                {
                    unclaimed_fruit.push((fruit_pos, nav_node));
                }
            }
        }

        // 6. Create up to min(shortfall, available_fruit, max_haul_tasks_per_heartbeat) Harvest tasks.
        let max_tasks = self.config.max_haul_tasks_per_heartbeat;
        let to_create = shortfall.min(unclaimed_fruit.len() as u32).min(max_tasks);

        for &(fruit_pos, _nav_node) in unclaimed_fruit.iter().take(to_create as usize) {
            let task_id = TaskId::new(&mut self.rng);
            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::Harvest { fruit_pos },
                state: task::TaskState::Available,
                location: fruit_pos,
                progress: 0,
                total_cost: 0,
                required_species: Some(Species::Elf),
                origin: task::TaskOrigin::Automated,
                target_creature: None,
            };
            self.insert_task(new_task);
        }
    }

    /// Process the logistics heartbeat: scan buildings with logistics config
    /// for unmet wants and create haul tasks to fulfill them.
    pub(crate) fn process_logistics_heartbeat(&mut self) {
        self.process_harvest_tasks();

        // Collect buildings with logistics enabled, sorted by priority desc then StructureId asc.
        let mut logistics_buildings: Vec<(StructureId, u8)> = self
            .db
            .structures
            .iter_all()
            .filter_map(|s| s.logistics_priority.map(|p| (s.id, p)))
            .collect();
        logistics_buildings.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let max_tasks = self.config.max_haul_tasks_per_heartbeat;
        let mut tasks_created = 0u32;

        for (building_id, building_priority) in &logistics_buildings {
            if tasks_created >= max_tasks {
                break;
            }

            if self.db.structures.get(building_id).is_none() {
                continue;
            }
            // Merge explicit logistics wants with auto-logistics from active recipes.
            let wants = self.compute_effective_wants(*building_id);

            for want in &wants {
                if tasks_created >= max_tasks {
                    break;
                }

                let filter = want.material_filter;

                // Count current inventory in this building for this item kind + filter.
                let current = self
                    .db
                    .structures
                    .get(building_id)
                    .map(|s| self.inv_item_count(s.inventory_id, want.item_kind, filter))
                    .unwrap_or(0);

                // Count in-transit items (from active Haul tasks targeting this building).
                let in_transit = self.count_in_transit_items(*building_id, want.item_kind, filter);

                let effective = current + in_transit;
                if effective >= want.target_quantity {
                    continue;
                }

                let needed = want.target_quantity - effective;

                // Find a source for these items.
                if let Some((source, available, source_coord)) = self.find_haul_source(
                    want.item_kind,
                    filter,
                    needed,
                    *building_id,
                    *building_priority,
                ) {
                    let quantity = available.min(needed);

                    // Find destination coordinate.
                    let dest_anchor = self.db.structures.get(building_id).unwrap().anchor;

                    // Reserve items at source.
                    let task_id = TaskId::new(&mut self.rng);
                    let hauled_material =
                        self.reserve_haul_items(&source, want.item_kind, filter, quantity, task_id);

                    // Create haul task.
                    let new_task = task::Task {
                        id: task_id,
                        kind: task::TaskKind::Haul {
                            item_kind: want.item_kind,
                            quantity,
                            source,
                            destination: *building_id,
                            phase: task::HaulPhase::GoingToSource,
                            destination_coord: dest_anchor,
                        },
                        state: task::TaskState::Available,
                        location: source_coord,
                        progress: 0,
                        total_cost: 0,
                        required_species: Some(Species::Elf),
                        origin: task::TaskOrigin::Automated,
                        target_creature: None,
                    };
                    // Store material filter and hauled material on the haul data extension.
                    // These are set after insert_task creates the base TaskHaulData row.
                    self.insert_task(new_task);
                    // Patch the TaskHaulData row with material info.
                    if self.task_haul_data(task_id).is_some() {
                        let _ = self.db.task_haul_data.modify_unchecked(&task_id, |h| {
                            h.material_filter = filter;
                            h.hauled_material = hauled_material;
                        });
                    }
                    tasks_created += 1;
                }
            }
        }

        self.process_unified_crafting_monitor();
        self.process_greenhouse_monitor();
    }

    /// Count items of the given kind that are in-transit to the given building
    /// via active Haul tasks. Uses `hauled_material` (not the haul's
    /// `material_filter`) for matching — a `Specific(X)` request only counts
    /// hauls actually carrying X, while `Any` counts all hauls.
    pub(crate) fn count_in_transit_items(
        &self,
        structure_id: StructureId,
        item_kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
    ) -> u32 {
        // Find haul tasks targeting this structure via HaulDestination refs.
        self.db
            .task_structure_refs
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|r| r.role == crate::db::TaskStructureRole::HaulDestination)
            .filter_map(|r| {
                let task = self.db.tasks.get(&r.task_id)?;
                if task.state == task::TaskState::Complete {
                    return None;
                }
                let haul = self.task_haul_data(r.task_id)?;
                if haul.item_kind == item_kind && filter.matches(haul.hauled_material) {
                    Some(haul.quantity)
                } else {
                    None
                }
            })
            .sum()
    }

    /// Find a source for hauling `needed` items of the given kind and material
    /// filter.
    ///
    /// Three-phase search:
    /// 1. Ground piles (deterministic BTreeMap order).
    /// 2. Buildings with strictly lower logistics priority.
    /// 3. Logistics-enabled buildings with surplus (held > wanted).
    ///
    /// Phase 3 uses `inv_want_target_total` (total wants for the kind, regardless
    /// of filter) to prevent taking items a source building itself wants.
    pub(crate) fn find_haul_source(
        &self,
        item_kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
        needed: u32,
        exclude_building: StructureId,
        requester_priority: u8,
    ) -> Option<(task::HaulSource, u32, VoxelCoord)> {
        // Phase 1: Check ground piles.
        for pile in self.db.ground_piles.iter_all() {
            let available = self.inv_unreserved_item_count(pile.inventory_id, item_kind, filter);
            if available > 0 && self.nav_graph.find_nearest_node(pile.position).is_some() {
                return Some((
                    task::HaulSource::GroundPile(pile.position),
                    available.min(needed),
                    pile.position,
                ));
            }
        }

        // Phase 2: Check other buildings with strictly lower priority.
        for structure in self.db.structures.iter_all() {
            let sid = structure.id;
            if sid == exclude_building {
                continue;
            }
            let Some(src_priority) = structure.logistics_priority else {
                continue;
            };
            if src_priority >= requester_priority {
                continue;
            }
            let available =
                self.inv_unreserved_item_count(structure.inventory_id, item_kind, filter);
            if available > 0 && self.nav_graph.find_nearest_node(structure.anchor).is_some() {
                return Some((
                    task::HaulSource::Building(sid),
                    available.min(needed),
                    structure.anchor,
                ));
            }
        }

        // Phase 3: Check logistics-enabled buildings for surplus items.
        // `held` uses the caller's filter. `wanted` sums all wants for this kind
        // (regardless of filter) to prevent stripping items from buildings with
        // complex multi-material wants.
        for structure in self.db.structures.iter_all() {
            let sid = structure.id;
            if sid == exclude_building {
                continue;
            }
            if structure.logistics_priority.is_none() {
                continue;
            }
            let held = self.inv_unreserved_item_count(structure.inventory_id, item_kind, filter);
            let wanted = self.inv_want_target_total(structure.inventory_id, item_kind);
            let surplus = held.saturating_sub(wanted);
            if surplus > 0 && self.nav_graph.find_nearest_node(structure.anchor).is_some() {
                return Some((
                    task::HaulSource::Building(sid),
                    surplus.min(needed),
                    structure.anchor,
                ));
            }
        }

        None
    }

    /// Reserve items at a haul source for a given task. Returns the material of
    /// the reserved stacks (for `hauled_material` tracking).
    pub(crate) fn reserve_haul_items(
        &mut self,
        source: &task::HaulSource,
        item_kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
        quantity: u32,
        task_id: TaskId,
    ) -> Option<inventory::Material> {
        match source {
            task::HaulSource::GroundPile(pos) => {
                if let Some(pile) = self
                    .db
                    .ground_piles
                    .by_position(pos, tabulosity::QueryOpts::ASC)
                    .into_iter()
                    .next()
                {
                    return self.inv_reserve_items(
                        pile.inventory_id,
                        item_kind,
                        filter,
                        quantity,
                        task_id,
                    );
                }
            }
            task::HaulSource::Building(sid) => {
                if let Some(structure) = self.db.structures.get(sid) {
                    return self.inv_reserve_items(
                        structure.inventory_id,
                        item_kind,
                        filter,
                        quantity,
                        task_id,
                    );
                }
            }
        }
        None
    }

    /// Find a source of unowned, unreserved items for personal acquisition.
    ///
    /// Searches ground piles first (deterministic BTreeMap order), then any
    /// building inventory (ignoring logistics priority — personal acquisition
    /// pulls from anywhere). Returns the source, capped quantity, and nav node.
    pub(crate) fn find_item_source(
        &self,
        kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
        needed: u32,
    ) -> Option<(task::HaulSource, u32, VoxelCoord)> {
        // Check ground piles.
        for pile in self.db.ground_piles.iter_all() {
            let available = self.inv_count_unowned_unreserved(pile.inventory_id, kind, filter);
            if available > 0 && self.nav_graph.find_nearest_node(pile.position).is_some() {
                return Some((
                    task::HaulSource::GroundPile(pile.position),
                    available.min(needed),
                    pile.position,
                ));
            }
        }

        // Check building inventories.
        for structure in self.db.structures.iter_all() {
            let sid = structure.id;
            let available = self.inv_count_unowned_unreserved(structure.inventory_id, kind, filter);
            if available > 0 && self.nav_graph.find_nearest_node(structure.anchor).is_some() {
                return Some((
                    task::HaulSource::Building(sid),
                    available.min(needed),
                    structure.anchor,
                ));
            }
        }

        None
    }
}
