// Creature activation chain — task selection, claiming, and behavior execution.
//
// The activation system drives all creature decisions. Each creature has a
// `CreatureActivation` event that fires periodically, performing one action
// (walk one nav edge or do one unit of task work) and scheduling the next
// activation. `process_creature_activation` is the main entry point,
// implementing the flee → task → wander decision cascade.
//
// Also contains heartbeat-driven military equipment logic:
// - `military_equipment_drop`: drops unowned items that don't satisfy the
//   creature's military group equipment wants (Phase 2b¾a).
// - `check_military_equipment_wants`: creates `AcquireMilitaryEquipment`
//   tasks for unsatisfied group equipment wants (Phase 2b¾b).
//
// See also: `movement.rs` (walking helpers), `needs.rs` (want-driven tasks),
// `combat.rs` (flee behavior).
use super::*;
use crate::db::ActionKind;
use crate::event::{ScheduledEventKind, SimEvent};
use crate::inventory;
use crate::pathfinding;
use crate::preemption;
use crate::task;

impl SimState {
    /// Spawn initial creatures and ground piles from `config.initial_creatures`
    /// and `config.initial_ground_piles`. Called once when a new game starts.
    pub fn spawn_initial_creatures(&mut self, events: &mut Vec<SimEvent>) {
        let specs = self.config.initial_creatures.clone();
        for spec in &specs {
            let species_data = match self.species_table.get(&spec.species) {
                Some(sd) => sd.clone(),
                None => continue,
            };
            for i in 0..spec.count {
                let creature_id =
                    match self.spawn_creature(spec.species, spec.spawn_position, events) {
                        Some(id) => id,
                        None => continue,
                    };

                // Apply per-creature food override.
                if let Some(&pct) = spec.food_pcts.get(i) {
                    let _ = self
                        .db
                        .creatures
                        .modify_unchecked(&creature_id, |creature| {
                            creature.food = species_data.food_max * pct as i64 / 100;
                        });
                }

                // Apply per-creature rest override.
                if let Some(&pct) = spec.rest_pcts.get(i) {
                    let _ = self
                        .db
                        .creatures
                        .modify_unchecked(&creature_id, |creature| {
                            creature.rest = species_data.rest_max * pct as i64 / 100;
                        });
                }

                // Apply per-creature bread count.
                if let Some(&count) = spec.bread_counts.get(i)
                    && count > 0
                {
                    let inv_id = self.creature_inv(creature_id);
                    self.inv_add_simple_item(
                        inv_id,
                        crate::inventory::ItemKind::Bread,
                        count,
                        Some(creature_id),
                        None,
                    );
                }

                // Apply per-creature starting equipment.
                if let Some(equip_list) = spec.initial_equipment.get(i) {
                    let inv_id = self.creature_inv(creature_id);
                    for equip in equip_list {
                        let slot = equip.item_kind.equip_slot();
                        self.inv_add_item(
                            inv_id,
                            equip.item_kind,
                            1,
                            Some(creature_id),
                            None,
                            equip.material,
                            0,
                            None,
                            slot,
                        );
                        // Apply dye color if specified.
                        if let Some(dye) = equip.dye_color
                            && let Some(slot) = slot
                            && let Some(stack) = self.inv_equipped_in_slot(inv_id, slot)
                        {
                            let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                                s.dye_color = Some(dye);
                            });
                        }
                    }
                }
            }
        }

        let pile_specs = self.config.initial_ground_piles.clone();
        for pile_spec in &pile_specs {
            let pos = self.find_surface_position(pile_spec.position.x, pile_spec.position.z);
            let pile_id = self.ensure_ground_pile(pos);
            let pile = self.db.ground_piles.get(&pile_id).unwrap();
            let pile_inv = pile.inventory_id;
            self.inv_add_item(
                pile_inv,
                pile_spec.item_kind,
                pile_spec.quantity,
                None,
                None,
                pile_spec.material,
                0,
                None,
                None,
            );
            // Apply dye color if specified. Finds the first undyed stack
            // matching (kind, material) — avoid multiple specs with the same
            // kind+material but different dye colors at the same position.
            if let Some(dye) = pile_spec.dye_color {
                for stack in self.inv_items(pile_inv) {
                    if stack.kind == pile_spec.item_kind
                        && stack.material == pile_spec.material
                        && stack.dye_color.is_none()
                    {
                        let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                            s.dye_color = Some(dye);
                        });
                        break;
                    }
                }
            }
        }
    }

    /// Abort a creature's current action, cleaning up any per-action state.
    ///
    /// For Move actions, deletes the `MoveAction` row. Clears `action_kind`
    /// and `next_available_tick` on the creature. Does NOT unassign the
    /// creature's task or clear its path — callers handle that.
    pub(crate) fn abort_current_action(&mut self, creature_id: CreatureId) {
        let action_kind = match self.db.creatures.get(&creature_id) {
            Some(c) => c.action_kind,
            None => return,
        };
        if action_kind == ActionKind::Move {
            let _ = self.db.move_actions.remove_no_fk(&creature_id);
        }
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::NoAction;
            c.next_available_tick = None;
        });
        // Remove orphaned CreatureActivation events from the queue.
        // Without this, the old activation fires on a creature whose action
        // state has been cleared, causing double-activations and erratic
        // movement (B-erratic-movement).
        self.event_queue.cancel_creature_activations(creature_id);
    }

    /// Creature activation: fires when a creature's current action completes
    /// (at `next_available_tick`) or when the creature is idle.
    ///
    /// Unified pipeline for both ground and flying creatures. Branching occurs
    /// only at: gravity (skip for flyers), nav-node lookup (None for flyers),
    /// flee dispatch, and wander dispatch. Everything else is shared.
    ///
    /// Flow:
    /// 1. Resolve the completed action's effects (e.g., delete MoveAction row).
    /// 2. Clear action state.
    /// 3. Decision cascade: flee → autonomous combat → continue task → find new task → wander.
    pub(crate) fn process_creature_activation(
        &mut self,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };
        let species = creature.species;
        let is_flying = self.species_table[&species]
            .flight_ticks_per_voxel
            .is_some();

        // Gravity check: ground creatures only.
        if !is_flying && !self.creature_is_supported(creature_id) {
            self.apply_single_creature_gravity(creature_id, events);
            return;
        }

        // Nav-node lookup: ground creatures get a NavNodeId, flyers get None.
        let (mut current_node, action_kind) = if is_flying {
            let action_kind = self
                .db
                .creatures
                .get(&creature_id)
                .map(|c| c.action_kind)
                .unwrap_or(ActionKind::NoAction);
            (None, action_kind)
        } else {
            let creature = match self.db.creatures.get(&creature_id) {
                Some(c) if c.vital_status == VitalStatus::Alive => c,
                _ => return,
            };
            let node = self
                .graph_for_species(creature.species)
                .node_at(creature.position)
                .unwrap(); // safe: creature_is_supported passed
            (Some(node), creature.action_kind)
        };

        // Guard: dead nav node resnap (ground creatures only).
        if let Some(cn) = current_node {
            if !self.graph_for_species(species).is_node_alive(cn) {
                self.abort_current_action(creature_id);
                let pos = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .map(|c| c.position)
                    .unwrap();
                let graph = self.graph_for_species(species);
                let new_node = match graph.find_nearest_node(pos) {
                    Some(n) => n,
                    None => return,
                };
                let new_pos = graph.node(new_node).position;
                let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                    c.position = new_pos;
                    c.path = None;
                });
                self.update_creature_spatial_index(creature_id, species, pos, new_pos);
                self.event_queue.schedule(
                    self.tick + 1,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
                return;
            }
        }

        // --- Step 1: Resolve completed action (shared) ---
        if action_kind == ActionKind::Move {
            let _ = self.db.move_actions.remove_no_fk(&creature_id);
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.action_kind = ActionKind::NoAction;
                c.next_available_tick = None;
            });
        }
        if action_kind == ActionKind::Build {
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.action_kind = ActionKind::NoAction;
                c.next_available_tick = None;
            });
            let completed = self.resolve_build_action(creature_id);
            // Re-read current_node for ground creatures.
            if !is_flying {
                current_node = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| self.graph_for_species(c.species).node_at(c.position));
                if current_node.is_none() {
                    return;
                }
            }
            if !completed {
                let task_id = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| c.current_task);
                if let Some(task_id) = task_id {
                    self.execute_task_behavior(creature_id, task_id, current_node, events);
                    return;
                }
            }
        }

        // Resolve simple work actions (shared).
        if matches!(
            action_kind,
            ActionKind::Furnish
                | ActionKind::Craft
                | ActionKind::Sleep
                | ActionKind::Eat
                | ActionKind::Harvest
                | ActionKind::AcquireItem
                | ActionKind::AcquireMilitaryEquipment
                | ActionKind::PickUp
                | ActionKind::DropOff
                | ActionKind::Mope
                | ActionKind::MeleeStrike
                | ActionKind::Shoot
        ) {
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.action_kind = ActionKind::NoAction;
                c.next_available_tick = None;
            });
            let completed = self.resolve_work_action(creature_id, action_kind);
            if !completed {
                let task_id = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| c.current_task);
                if let Some(task_id) = task_id {
                    self.execute_task_behavior(creature_id, task_id, current_node, events);
                    return;
                }
            }
        }

        // --- Flee check ---
        // Dispatch: ground uses nav-graph edges (ground_flee_step),
        // flying uses fly_wander (B-flying-flee tracks proper directional flee).
        if self.should_flee(creature_id, species) {
            if let Some(cn) = current_node {
                if self.ground_flee_step(creature_id, cn, species) {
                    return;
                }
            } else {
                // Flying flee: wander away (imperfect but functional).
                self.fly_wander(creature_id, events);
                return;
            }
        }

        // --- Autonomous combat check (shared) ---
        {
            let style = self.resolve_engagement_style(creature_id);
            let detection_range_sq = self.species_table[&species].hostile_detection_range_sq;
            if detection_range_sq > 0
                && matches!(
                    style.initiative,
                    crate::species::EngagementInitiative::Aggressive
                        | crate::species::EngagementInitiative::Defensive
                )
            {
                let should_try_combat = if let Some(task_id) = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| c.current_task)
                    && let Some(task) = self.db.tasks.get(&task_id)
                {
                    let current_level = preemption::preemption_level(task.kind_tag, task.origin);
                    current_level == preemption::PreemptionLevel::Autonomous
                } else {
                    true
                };

                if should_try_combat {
                    let creature = self.db.creatures.get(&creature_id).unwrap();
                    let has_targets = !self
                        .detect_hostile_targets(
                            creature_id,
                            species,
                            creature.position,
                            creature.civ_id,
                            detection_range_sq,
                        )
                        .is_empty();

                    if has_targets {
                        if let Some(task_id) = self
                            .db
                            .creatures
                            .get(&creature_id)
                            .and_then(|c| c.current_task)
                        {
                            self.interrupt_task(creature_id, task_id);
                        }
                        if self.hostile_pursue(creature_id, current_node, species, events) {
                            return;
                        }
                    }
                }
            }
        }

        // --- Decision cascade (shared) ---
        let current_task = self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task);

        if let Some(task_id) = current_task {
            self.execute_task_behavior(creature_id, task_id, current_node, events);
        } else {
            if let Some(task_id) = self.find_available_task(creature_id) {
                self.claim_task(creature_id, task_id);
                self.execute_task_behavior(creature_id, task_id, current_node, events);
            } else {
                self.wander_dispatch(creature_id, current_node, events);
            }
        }
    }

    /// Find the nearest available task this creature can work on.
    /// Uses Dijkstra search on the nav graph to prefer tasks closest by
    /// actual travel cost, not just insertion order. Respects species
    /// restrictions: tasks with `required_species` are only visible to
    /// matching creatures.
    pub(crate) fn find_available_task(&self, creature_id: CreatureId) -> Option<TaskId> {
        let creature = self.db.creatures.get(&creature_id)?;
        let species = creature.species;
        let is_flying = self.species_table[&species]
            .flight_ticks_per_voxel
            .is_some();
        let creature_pos = creature.position;
        let current_node = if is_flying {
            None
        } else {
            Some(self.graph_for_species(species).node_at(creature_pos)?)
        };
        let is_nonmagical = creature.mp_max == 0;
        // Minimum mana needed to attempt one mana-requiring work action.
        let min_mana_cost = self.mana_cost_per_action(None);
        let has_mana_for_work = creature.mp >= min_mana_cost;

        // Minimum mana for a Grow-verb craft action (may differ from build cost).
        let min_grow_mana_cost = self.mana_cost_for_grow_action();
        let has_mana_for_grow = creature.mp >= min_grow_mana_cost;

        // Collect all candidate tasks (id + location coord) that this creature can work.
        let candidates: Vec<(TaskId, VoxelCoord)> = self
            .db
            .tasks
            .iter_all()
            .filter(|t| {
                if t.state != task::TaskState::Available {
                    return false;
                }
                if t.required_species.is_some_and(|s| s != species) {
                    return false;
                }
                // Build/Furnish: nonmagical creatures cannot claim, magical need mana.
                if t.kind_tag.requires_mana() {
                    return !is_nonmagical && has_mana_for_work;
                }
                // Grow-verb Craft tasks also require mana.
                if t.kind_tag == crate::db::TaskKindTag::Craft {
                    let is_grow = self
                        .task_craft_data(t.id)
                        .is_some_and(|d| d.recipe.verb() == crate::recipe::RecipeVerb::Grow);
                    if is_grow {
                        return !is_nonmagical && has_mana_for_grow;
                    }
                }
                true
            })
            .map(|t| (t.id, t.location))
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // Single candidate — skip distance calculation.
        if candidates.len() == 1 {
            return Some(candidates[0].0);
        }

        // Flying creatures: pick nearest task by squared Euclidean distance.
        // Ground creatures: use Dijkstra on the nav graph for true path cost.
        if is_flying {
            let nearest = candidates
                .iter()
                .map(|&(id, loc)| {
                    let dx = creature_pos.x as i64 - loc.x as i64;
                    let dy = creature_pos.y as i64 - loc.y as i64;
                    let dz = creature_pos.z as i64 - loc.z as i64;
                    (id, dx * dx + dy * dy + dz * dz)
                })
                .min_by_key(|&(id, dist)| (dist, id))?;
            return Some(nearest.0);
        }

        let current_node = current_node.unwrap();
        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);

        // Resolve VoxelCoords to NavNodeIds for Dijkstra, keeping the association.
        let resolved: Vec<(TaskId, NavNodeId)> = candidates
            .iter()
            .filter_map(|&(id, coord)| graph.find_nearest_node(coord).map(|nav| (id, nav)))
            .collect();

        if resolved.is_empty() {
            return None;
        }

        let target_nodes: Vec<NavNodeId> = resolved.iter().map(|&(_, nav)| nav).collect();

        let nearest_node = pathfinding::dijkstra_nearest(
            graph,
            current_node,
            &target_nodes,
            species_data.walk_ticks_per_voxel,
            species_data.climb_ticks_per_voxel,
            species_data.wood_ladder_tpv,
            species_data.rope_ladder_tpv,
            None, // no edge filter
        )?;

        // Find the task at the nearest node. If multiple tasks share a location,
        // the first in iteration order wins (deterministic tiebreaker).
        resolved
            .iter()
            .find(|&&(_, nav)| nav == nearest_node)
            .map(|&(id, _)| id)
    }

    /// Assign a creature to a task.
    pub(crate) fn claim_task(&mut self, creature_id: CreatureId, task_id: TaskId) {
        if let Some(mut task) = self.db.tasks.get(&task_id) {
            task.state = task::TaskState::InProgress;
            let _ = self.db.tasks.update_no_fk(task);
        }
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.current_task = Some(task_id);
            let _ = self.db.creatures.update_no_fk(creature);
        }
    }

    /// Execute one activation's worth of task behavior. Works for both ground
    /// and flying creatures — `current_node` is `None` for flyers.
    pub(crate) fn execute_task_behavior(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        current_node: Option<NavNodeId>,
        events: &mut Vec<SimEvent>,
    ) {
        let is_flying = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| {
                self.species_table[&c.species]
                    .flight_ticks_per_voxel
                    .is_some()
            })
            .unwrap_or(false);

        let (mut task_location_coord, target_creature) = match self.db.tasks.get(&task_id) {
            Some(t) => (t.location, t.target_creature),
            None => {
                // Task was removed — abort action, unassign, and wander.
                self.abort_current_action(creature_id);
                if let Some(mut c) = self.db.creatures.get(&creature_id) {
                    c.current_task = None;
                    c.path = None;
                    let _ = self.db.creatures.update_no_fk(c);
                }
                self.wander_dispatch(creature_id, current_node, events);
                return;
            }
        };

        // --- Dynamic pursuit: track moving target creature ---
        if let Some(target_id) = target_creature {
            let target_pos = self.db.creatures.get(&target_id).map(|c| c.position);
            match target_pos {
                None => {
                    // Target creature is gone — abandon.
                    self.interrupt_task(creature_id, task_id);
                    self.wander_dispatch(creature_id, current_node, events);
                    return;
                }
                Some(target_coord) => {
                    if target_coord != task_location_coord {
                        // Target moved — update task location and invalidate path.
                        task_location_coord = target_coord;
                        let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
                            t.location = target_coord;
                        });
                        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                            c.path = None;
                        });
                    }
                }
            }
        }

        let species = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| c.species)
            .unwrap_or(Species::Elf);

        // Ground creatures: resolve task location to nav node and check liveness.
        // Flying creatures skip nav-node resolution entirely.
        let task_location_node: Option<NavNodeId> = if !is_flying {
            let graph = self.graph_for_species(species);
            match graph.find_nearest_node(task_location_coord) {
                Some(n) => Some(n),
                None => {
                    // No reachable nav node for the task location — abandon.
                    self.interrupt_task(creature_id, task_id);
                    if let Some(c) = self.db.creatures.get(&creature_id) {
                        let old_pos = c.position;
                        let graph = self.graph_for_species(species);
                        if let Some(new_node) = graph.find_nearest_node(old_pos) {
                            self.ground_wander(creature_id, new_node, events);
                        }
                    }
                    return;
                }
            }
        } else {
            None
        };

        // Ground creatures: check that both current_node and task_location are
        // still alive in the nav graph. Flying creatures skip this check.
        if let (Some(cn), Some(tl)) = (current_node, task_location_node) {
            let graph = self.graph_for_species(species);
            if !graph.is_node_alive(cn) || !graph.is_node_alive(tl) {
                self.interrupt_task(creature_id, task_id);
                let graph = self.graph_for_species(species);
                if let Some(c) = self.db.creatures.get(&creature_id) {
                    let old_pos = c.position;
                    if let Some(new_node) = graph.find_nearest_node(old_pos) {
                        let new_pos = graph.node(new_node).position;
                        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                            c.position = new_pos;
                        });
                        self.update_creature_spatial_index(creature_id, species, old_pos, new_pos);
                        self.ground_wander(creature_id, new_node, events);
                    }
                }
                return;
            }
        }

        // AttackTarget tasks: check combat range before walking.
        let task_kind_tag = self
            .db
            .tasks
            .get(&task_id)
            .map(|t| t.kind_tag)
            .unwrap_or(crate::db::TaskKindTag::GoTo);
        if task_kind_tag == crate::db::TaskKindTag::AttackTarget {
            if let Some(attack_data) = self.task_attack_target_data(task_id) {
                let target_id = attack_data.target;
                let target_dead = self
                    .db
                    .creatures
                    .get(&target_id)
                    .is_none_or(|c| c.vital_status == VitalStatus::Dead);
                if target_dead {
                    self.complete_task(task_id);
                    self.schedule_reactivation(creature_id);
                    return;
                }
            }

            // Try combat actions at range (melee or ranged).
            if self.try_attack_target_combat(creature_id, task_id, events) {
                return;
            }

            // Not in combat range — walk toward target.
            if !self.at_task_location(creature_id, current_node, task_location_node, task_location_coord) {
                self.walk_toward_attack_target(
                    creature_id,
                    task_id,
                    task_location_coord,
                    current_node,
                    events,
                );
                return;
            }
            // At location but not in range (target just moved) — re-activate.
            self.event_queue.schedule(
                self.tick + 1,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return;
        }

        if task_kind_tag == crate::db::TaskKindTag::AttackMove {
            self.execute_attack_move(creature_id, task_id, task_location_coord, current_node, events);
            return;
        }

        if self.at_task_location(creature_id, current_node, task_location_node, task_location_coord) {
            // At task location — run the kind-specific completion/work logic.
            self.execute_task_at_location(creature_id, task_id, events);
        } else {
            // Not at location — walk toward it.
            self.walk_toward_task(creature_id, task_location_coord, current_node, events);
        }
    }

    /// Check if a creature is at the task location. Ground creatures compare
    /// nav node IDs; flying creatures compare position proximity (within 1 voxel).
    pub(crate) fn at_task_location(
        &self,
        creature_id: CreatureId,
        current_node: Option<NavNodeId>,
        task_location_node: Option<NavNodeId>,
        task_location_coord: VoxelCoord,
    ) -> bool {
        // Ground creatures: compare nav node IDs.
        if let (Some(cn), Some(tl)) = (current_node, task_location_node) {
            return cn == tl;
        }
        // Flying creatures: position proximity.
        let pos = match self.db.creatures.get(&creature_id) {
            Some(c) => c.position,
            None => return false,
        };
        let dx = (pos.x - task_location_coord.x).abs();
        let dy = (pos.y - task_location_coord.y).abs();
        let dz = (pos.z - task_location_coord.z).abs();
        dx <= 1 && dy <= 1 && dz <= 1
    }

    /// Dispatch to the appropriate wander function based on locomotion type.
    fn wander_dispatch(
        &mut self,
        creature_id: CreatureId,
        current_node: Option<NavNodeId>,
        events: &mut Vec<SimEvent>,
    ) {
        if let Some(cn) = current_node {
            self.ground_wander(creature_id, cn, events);
        } else {
            self.fly_wander(creature_id, events);
        }
    }

    /// Execute task-kind-specific logic when the creature is at the task location.
    pub(crate) fn execute_task_at_location(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        events: &mut Vec<SimEvent>,
    ) {
        let task = match self.db.tasks.get(&task_id) {
            Some(t) => t,
            None => return,
        };

        match task.kind_tag {
            crate::db::TaskKindTag::GoTo => {
                self.complete_task(task_id);
            }
            crate::db::TaskKindTag::EatBread | crate::db::TaskKindTag::EatFruit => {
                self.start_simple_action(
                    creature_id,
                    ActionKind::Eat,
                    self.config.eat_action_ticks,
                );
                return;
            }
            crate::db::TaskKindTag::Build => {
                let project_id = match self.task_project_id(task_id) {
                    Some(p) => p,
                    None => return,
                };
                self.start_build_action(creature_id, task_id, project_id);
                return;
            }
            crate::db::TaskKindTag::Furnish => {
                self.start_furnish_action(creature_id);
                return;
            }
            crate::db::TaskKindTag::Sleep => {
                self.start_sleep_action(creature_id, task_id);
                return;
            }
            crate::db::TaskKindTag::Haul => {
                // Determine phase — PickUp or DropOff.
                let phase = self
                    .task_haul_data(task_id)
                    .map(|h| h.phase)
                    .unwrap_or(task::HaulPhase::GoingToSource);
                match phase {
                    task::HaulPhase::GoingToSource => self.start_pickup_action(creature_id),
                    task::HaulPhase::GoingToDestination => self.start_dropoff_action(creature_id),
                }
                return;
            }
            crate::db::TaskKindTag::Harvest => {
                self.start_simple_action(
                    creature_id,
                    ActionKind::Harvest,
                    self.config.harvest_action_ticks,
                );
                return;
            }
            crate::db::TaskKindTag::AcquireItem => {
                self.start_simple_action(
                    creature_id,
                    ActionKind::AcquireItem,
                    self.config.acquire_item_action_ticks,
                );
                return;
            }
            crate::db::TaskKindTag::AcquireMilitaryEquipment => {
                self.start_simple_action(
                    creature_id,
                    ActionKind::AcquireMilitaryEquipment,
                    self.config.acquire_item_action_ticks,
                );
                return;
            }
            crate::db::TaskKindTag::Mope => {
                self.start_mope_action(creature_id);
                return;
            }
            crate::db::TaskKindTag::Craft => {
                self.start_craft_action(creature_id, task_id);
                return;
            }
            crate::db::TaskKindTag::AttackTarget => {
                self.execute_attack_target_at_location(creature_id, task_id, events);
                return;
            }
            crate::db::TaskKindTag::AttackMove => {
                // Handled in execute_task_behavior before reaching here.
                return;
            }
        }

        // Schedule next activation (creature is now idle, will wander or pick
        // up another task).
        self.event_queue.schedule(
            self.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Schedule a creature reactivation on the next tick so it enters the
    /// decision cascade (picks up a new task or wanders). Use this after
    /// `complete_task` in code paths that `return` before reaching the
    /// cascade in `process_creature_activation`.
    pub(crate) fn schedule_reactivation(&mut self, creature_id: CreatureId) {
        self.event_queue.schedule(
            self.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Check if a creature should start moping due to low mood. Called during
    /// heartbeat Phase 2b½, after hunger/sleep but before item acquisition.
    /// Only fires for elves (only species with meaningful thoughts). Uses a
    /// Poisson-like integer probability: `roll % mean < elapsed`.
    pub(crate) fn check_mope(&mut self, creature_id: CreatureId) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };

        // Only elves mope (only species with thoughts).
        if creature.species != Species::Elf {
            return;
        }

        let (_, tier) = self.mood_for_creature(creature_id);
        let mean = self.config.mood_consequences.mope_mean_ticks(tier);
        if mean == 0 {
            return; // This tier never mopes.
        }

        // Preemption check: can mope interrupt the current task?
        // Uses the formal preemption system instead of ad-hoc checks.
        // Mope is Mood(4), which preempts Idle(0), Autonomous(1), and
        // PlayerDirected(2) but NOT Survival(3) (hardcoded exception
        // prevents starvation spiral).
        if let Some(task_id) = creature.current_task
            && let Some(current_task) = self.db.tasks.get(&task_id)
        {
            let current_level =
                preemption::preemption_level(current_task.kind_tag, current_task.origin);
            let current_origin = current_task.origin;
            if !preemption::can_preempt(
                current_level,
                current_origin,
                preemption::PreemptionLevel::Mood,
                task::TaskOrigin::Autonomous,
            ) {
                return;
            }
        }

        // Probability roll: mope if `roll % mean < elapsed`.
        let species_data = &self.species_table[&Species::Elf];
        let elapsed = species_data.heartbeat_interval_ticks;
        let roll = self.rng.next_u64();
        if roll % mean >= elapsed {
            return; // Roll failed.
        }

        // If creature has an in-progress task, interrupt it.
        if let Some(old_task_id) = creature.current_task {
            self.interrupt_task(creature_id, old_task_id);
        }

        // Determine mope location: assigned home bed coord, else current position.
        let mope_coord = self
            .find_assigned_home_bed(creature_id)
            .map(|(bed_coord, _, _)| bed_coord)
            .or_else(|| self.db.creatures.get(&creature_id).map(|c| c.position));
        let mope_coord = match mope_coord {
            Some(c) => c,
            None => return,
        };

        let task_id = TaskId::new(&mut self.rng);
        let duration = self.config.mood_consequences.mope_duration_ticks;
        let new_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Mope,
            state: task::TaskState::InProgress,
            location: mope_coord,
            progress: 0,
            total_cost: duration as i64,
            required_species: None,
            origin: task::TaskOrigin::Autonomous,
            target_creature: None,
        };
        self.insert_task(new_task);
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.current_task = Some(task_id);
            let name = creature.name.clone();
            let _ = self.db.creatures.update_no_fk(creature);

            let tier_label = tier.label();
            let msg = if name.is_empty() {
                format!("An elf is moping ({tier_label})")
            } else {
                format!("{name} is moping ({tier_label})")
            };
            self.add_notification(msg);
        }
    }

    /// Check a creature's personal wants and create an AcquireItem task for
    /// the first unsatisfied want. Called during heartbeat Phase 2c when the
    /// creature is idle.
    pub(crate) fn check_creature_wants(&mut self, creature_id: CreatureId) {
        // Gather want info from creature (borrow creature briefly, then release).
        let owned_counts = {
            let creature = match self.db.creatures.get(&creature_id) {
                Some(c) => c,
                None => return,
            };
            let wants = self.inv_wants(creature.inventory_id);
            if wants.is_empty() {
                return;
            }
            wants
                .iter()
                .map(|w| {
                    let owned =
                        self.inv_count_owned(creature.inventory_id, w.item_kind, creature_id);
                    (w.item_kind, w.material_filter, w.target_quantity, owned)
                })
                .collect::<Vec<(inventory::ItemKind, inventory::MaterialFilter, u32, u32)>>()
        };

        // Find first unsatisfied want.
        for (item_kind, filter, target, owned) in &owned_counts {
            if *owned >= *target {
                continue;
            }
            let needed = *target - *owned;

            // Find a source.
            let (source, quantity, nav_node) =
                match self.find_item_source(*item_kind, *filter, needed) {
                    Some(s) => s,
                    None => continue, // No source for this kind; try next want.
                };

            // Reserve items at source.
            let task_id = TaskId::new(&mut self.rng);
            match &source {
                task::HaulSource::GroundPile(pos) => {
                    if let Some(pile) = self
                        .db
                        .ground_piles
                        .by_position(pos, tabulosity::QueryOpts::ASC)
                        .into_iter()
                        .next()
                    {
                        self.inv_reserve_unowned_items(
                            pile.inventory_id,
                            *item_kind,
                            *filter,
                            quantity,
                            task_id,
                        );
                    }
                }
                task::HaulSource::Building(sid) => {
                    if let Some(structure) = self.db.structures.get(sid) {
                        self.inv_reserve_unowned_items(
                            structure.inventory_id,
                            *item_kind,
                            *filter,
                            quantity,
                            task_id,
                        );
                    }
                }
            }

            // Create AcquireItem task, directly assigned (same pattern as EatBread).
            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::AcquireItem {
                    source,
                    item_kind: *item_kind,
                    quantity,
                },
                state: task::TaskState::InProgress,
                location: nav_node,
                progress: 0,
                total_cost: 0,
                required_species: None,
                origin: task::TaskOrigin::Autonomous,
                target_creature: None,
            };
            self.insert_task(new_task);
            if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                creature.current_task = Some(task_id);
                let _ = self.db.creatures.update_no_fk(creature);
            }
            return; // One task per heartbeat.
        }
    }

    /// Drop items the creature shouldn't be carrying. An item is dropped if:
    /// - It's owned by another creature (always drop), OR
    /// - It's unowned AND doesn't satisfy any of the creature's military
    ///   equipment wants AND isn't reserved by the creature's current task.
    ///
    /// Items owned by the creature itself are never dropped. Dropped items go
    /// to a ground pile at the creature's current position.
    pub(crate) fn military_equipment_drop(&mut self, creature_id: CreatureId) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        // Only elves have military groups and equipment management.
        if creature.species != Species::Elf {
            return;
        }
        let inv_id = creature.inventory_id;
        let current_task = creature.current_task;
        let creature_position = creature.position;

        // Get equipment wants from the creature's military group.
        let equipment_wants = creature
            .military_group
            .and_then(|gid| self.db.military_groups.get(&gid))
            .map(|g| g.equipment_wants.clone())
            .unwrap_or_default();

        // Scan inventory for items to drop.
        let stacks: Vec<crate::db::ItemStack> = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);

        // Build a map of how many items of each (kind, filter) are wanted vs
        // held, so we can determine which unowned items exceed the want.
        // Track remaining want capacity per want entry.
        let mut want_remaining: Vec<(inventory::ItemKind, inventory::MaterialFilter, u32)> =
            equipment_wants
                .iter()
                .map(|w| (w.item_kind, w.material_filter, w.target_quantity))
                .collect();

        // First pass: subtract owned-by-self items from want capacity (they
        // always satisfy wants and are never dropped).
        for stack in &stacks {
            if stack.owner != Some(creature_id) {
                continue;
            }
            for (kind, filter, remaining) in &mut want_remaining {
                if stack.kind == *kind && filter.matches(stack.material) && *remaining > 0 {
                    let satisfy = stack.quantity.min(*remaining);
                    *remaining -= satisfy;
                    break;
                }
            }
        }

        // Second pass: identify unowned items to keep (satisfying remaining wants)
        // and items to drop.
        let mut stacks_to_drop: Vec<(ItemStackId, u32)> = Vec::new();
        for stack in &stacks {
            // Never drop items owned by self.
            if stack.owner == Some(creature_id) {
                continue;
            }
            // Always drop items owned by someone else (even if equipped).
            if stack.owner.is_some() {
                stacks_to_drop.push((stack.id, stack.quantity));
                continue;
            }
            // Never drop equipped unowned items (e.g. clothing auto-equipped
            // via personal AcquireItem).
            if stack.equipped_slot.is_some() {
                continue;
            }
            // Unowned items: check if reserved by current task.
            if let Some(task_id) = current_task
                && stack.reserved_by == Some(task_id)
            {
                continue;
            }
            // Unowned items: check if they satisfy a military want.
            let mut qty_to_keep = 0u32;
            for (kind, filter, remaining) in &mut want_remaining {
                if stack.kind == *kind && filter.matches(stack.material) && *remaining > 0 {
                    let satisfy = stack.quantity.min(*remaining);
                    *remaining -= satisfy;
                    qty_to_keep += satisfy;
                    break;
                }
            }
            let qty_to_drop = stack.quantity.saturating_sub(qty_to_keep);
            if qty_to_drop > 0 {
                stacks_to_drop.push((stack.id, qty_to_drop));
            }
        }

        if stacks_to_drop.is_empty() {
            return;
        }

        // Find or create a ground pile at the creature's position.
        let creature_pos = creature_position;
        let pile_id = self.ensure_ground_pile(creature_pos);
        let pile_inv = match self.db.ground_piles.get(&pile_id) {
            Some(p) => p.inventory_id,
            None => return,
        };

        // Move items to the ground pile.
        for (stack_id, qty) in stacks_to_drop {
            if let Some(split_id) = self.inv_split_stack(stack_id, qty)
                && let Some(mut moved) = self.db.item_stacks.get(&split_id)
            {
                moved.inventory_id = pile_inv;
                moved.owner = None;
                moved.reserved_by = None;
                let _ = self.db.item_stacks.update_no_fk(moved);
            }
        }

        // Normalize both inventories.
        self.inv_normalize(inv_id);
        self.inv_normalize(pile_inv);
    }

    /// Check a creature's military group equipment wants and create an
    /// `AcquireMilitaryEquipment` task for the first unsatisfied want.
    /// Called during heartbeat Phase 2b¾ when the creature is idle.
    pub(crate) fn check_military_equipment_wants(&mut self, creature_id: CreatureId) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let group_id = match creature.military_group {
            Some(gid) => gid,
            None => return,
        };
        let group = match self.db.military_groups.get(&group_id) {
            Some(g) => g,
            None => return,
        };
        if group.equipment_wants.is_empty() {
            return;
        }
        let inv_id = creature.inventory_id;
        let wants = group.equipment_wants.clone();

        // Check each want against current inventory.
        for want in &wants {
            let have = self.inv_count_owned_or_unowned(
                inv_id,
                want.item_kind,
                want.material_filter,
                creature_id,
            );
            if have >= want.target_quantity {
                continue;
            }
            let needed = want.target_quantity - have;

            // Find a source.
            let (source, quantity, nav_node) =
                match self.find_item_source(want.item_kind, want.material_filter, needed) {
                    Some(s) => s,
                    None => continue,
                };

            // Reserve items at source.
            let task_id = TaskId::new(&mut self.rng);
            match &source {
                task::HaulSource::GroundPile(pos) => {
                    if let Some(pile) = self
                        .db
                        .ground_piles
                        .by_position(pos, tabulosity::QueryOpts::ASC)
                        .into_iter()
                        .next()
                    {
                        self.inv_reserve_unowned_items(
                            pile.inventory_id,
                            want.item_kind,
                            want.material_filter,
                            quantity,
                            task_id,
                        );
                    }
                }
                task::HaulSource::Building(sid) => {
                    if let Some(structure) = self.db.structures.get(sid) {
                        self.inv_reserve_unowned_items(
                            structure.inventory_id,
                            want.item_kind,
                            want.material_filter,
                            quantity,
                            task_id,
                        );
                    }
                }
            }

            // Create AcquireMilitaryEquipment task, directly assigned.
            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::AcquireMilitaryEquipment {
                    source,
                    item_kind: want.item_kind,
                    quantity,
                },
                state: task::TaskState::InProgress,
                location: nav_node,
                progress: 0,
                total_cost: 0,
                required_species: None,
                origin: task::TaskOrigin::Autonomous,
                target_creature: None,
            };
            self.insert_task(new_task);
            if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                creature.current_task = Some(task_id);
                let _ = self.db.creatures.update_no_fk(creature);
            }
            return; // One task per heartbeat.
        }
    }

    // process_flying_creature_activation has been merged into
    // process_creature_activation (B-flying-tasks).
}
