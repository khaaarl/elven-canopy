// Crafting system — recipe execution, active recipe management.
//
// Implements the unified crafting monitor that creates and manages crafting
// tasks based on active recipes at workstations. Bread production goes through
// the Extract → Mill → Bake chain. Includes recipe queue management (add,
// remove, reorder, enable/disable).
//
// **Grow-verb mana drain (F-mana-grow-recipes):** Grow recipes (magical wood
// shaping) are multi-action tasks that drain mana per action, using the same
// wasted-action / abandon-threshold mechanics as construction (`construction.rs`).
// Non-Grow recipes remain single-action with no mana cost.
//
// See also: `recipe.rs` (recipe catalog/enum and definitions), `logistics.rs`
// (item hauling to workstations), `inventory_mgmt.rs` (item operations),
// `construction.rs` (mana drain helpers: `mana_cost_for_grow_action`,
// `try_drain_mana`).
use super::*;
use crate::db::ActionKind;
use crate::event::ScheduledEventKind;
use crate::inventory;
use crate::task;

impl SimState {
    /// Start a Craft action: set action kind and schedule next activation.
    ///
    /// For Grow-verb recipes, each action is one `grow_work_ticks_per_action`
    /// step that drains mana on completion (multi-action task). For all other
    /// recipes, the full `work_ticks` runs in a single action (no mana).
    pub(crate) fn start_craft_action(&mut self, creature_id: CreatureId, task_id: TaskId) {
        let craft_data = self.task_craft_data(task_id);
        let fruit_species: Vec<_> = self.db.fruit_species.iter_all().cloned().collect();
        let is_grow = craft_data
            .as_ref()
            .is_some_and(|d| d.recipe.verb() == crate::recipe::RecipeVerb::Grow);
        let duration = if is_grow {
            self.config.grow_recipes.grow_work_ticks_per_action.max(1)
        } else {
            craft_data
                .as_ref()
                .and_then(|d| {
                    let params = crate::recipe::RecipeParams {
                        material: d.material,
                    };
                    d.recipe
                        .resolve(&params, &self.config, &fruit_species)
                        .map(|r| r.work_ticks)
                })
                .unwrap_or(5000)
        };

        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Craft;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Craft action.
    ///
    /// **Grow-verb recipes (multi-action):** Drain mana per action. On success,
    /// increment `progress` by 1. When `progress >= total_cost`, consume inputs
    /// and produce outputs. On mana failure, wasted action (may abandon).
    ///
    /// **All other recipes (single-action):** Consume inputs, produce outputs,
    /// complete immediately (no mana cost).
    pub(crate) fn resolve_craft_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let structure_id =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::CraftAt) {
                Some(s) => s,
                None => return false,
            };

        let craft_data = match self.task_craft_data(task_id) {
            Some(d) => d,
            None => {
                self.complete_task(task_id);
                return true;
            }
        };

        let verb = craft_data.recipe.verb();
        let is_grow = verb == crate::recipe::RecipeVerb::Grow;

        // --- Grow-verb mana drain ---
        if is_grow {
            let cost = self.mana_cost_for_grow_action();
            if cost > 0 && !self.try_drain_mana(creature_id, cost) {
                // Wasted action. If try_drain_mana abandoned the task,
                // the creature's current_task will be None.
                return self
                    .db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| c.current_task)
                    .is_none();
            }

            // Mana drained — increment progress.
            let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
                t.progress += 1;
            });

            // Skill advancement per grow action (not just on completion).
            // Growing equipment is primarily woodcraft with some
            // singing/channeling.
            self.try_advance_skill(creature_id, crate::types::TraitKind::Woodcraft, 1000);
            self.try_advance_skill(creature_id, crate::types::TraitKind::Singing, 500);
            self.try_advance_skill(creature_id, crate::types::TraitKind::Channeling, 500);

            // Check if all actions are done.
            let task = match self.db.tasks.get(&task_id) {
                Some(t) => t,
                None => return true,
            };
            if task.progress < task.total_cost {
                return false; // More actions needed.
            }
        }

        // --- Produce outputs (final action for Grow, only action for others) ---
        let fruit_species: Vec<_> = self.db.fruit_species.iter_all().cloned().collect();
        let params = crate::recipe::RecipeParams {
            material: craft_data.material,
        };
        let resolved = craft_data
            .recipe
            .resolve(&params, &self.config, &fruit_species);

        let inv_id = self.structure_inv(structure_id);

        if let Some(resolved) = &resolved {
            for input in &resolved.inputs {
                self.inv_remove_reserved_items(inv_id, input.item_kind, input.quantity, task_id);
            }
            for output in &resolved.outputs {
                self.inv_add_item(
                    inv_id,
                    output.item_kind,
                    output.quantity,
                    None,
                    None,
                    output.material,
                    output.quality,
                    None,
                    None,
                );
                self.apply_output_dye_color(inv_id, output);
                self.record_subcomponents(inv_id, output, &resolved.subcomponent_records);
            }
        }

        self.complete_task(task_id);

        // Skill advancement per recipe verb (F-creature-skills).
        {
            use crate::recipe::RecipeVerb;
            use crate::types::TraitKind;
            match verb {
                RecipeVerb::Extract | RecipeVerb::Mill | RecipeVerb::Press => {
                    self.try_advance_skill(creature_id, TraitKind::Herbalism, 800);
                }
                RecipeVerb::Spin | RecipeVerb::Twist | RecipeVerb::Weave | RecipeVerb::Sew => {
                    self.try_advance_skill(creature_id, TraitKind::Tailoring, 800);
                }
                RecipeVerb::Bake => {
                    self.try_advance_skill(creature_id, TraitKind::Cuisine, 800);
                }
                RecipeVerb::Assemble => {
                    self.try_advance_skill(creature_id, TraitKind::Woodcraft, 800);
                }
                RecipeVerb::Grow => {
                    // Grow skills are advanced per-action in the mana drain
                    // loop above, not here at completion.
                }
            }
        }

        true
    }

    /// If the recipe output specifies a `dye_color`, set it on the most
    /// recently added matching item stack and re-normalize the inventory
    /// (stacks with different dye_color values must not merge).
    fn apply_output_dye_color(
        &mut self,
        inv_id: InventoryId,
        output: &crate::config::RecipeOutput,
    ) {
        let Some(dye_color) = output.dye_color else {
            return;
        };
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        if let Some(stack) = stacks.iter().rev().find(|s| {
            s.kind == output.item_kind
                && s.material == output.material
                && s.quality == output.quality
                && s.dye_color.is_none()
        }) {
            let stack_id = stack.id;
            let _ = self
                .db
                .item_stacks
                .modify_unchecked(&stack_id, |s| s.dye_color = Some(dye_color));
            self.inv_normalize(inv_id);
        }
    }

    /// Record subcomponent records on the most recently added item stack
    /// matching the given output. Called after `inv_add_item` for crafted items.
    pub(crate) fn record_subcomponents(
        &mut self,
        inv_id: InventoryId,
        output: &crate::config::RecipeOutput,
        subcomponent_records: &[crate::config::RecipeSubcomponentRecord],
    ) {
        if subcomponent_records.is_empty() {
            return;
        }
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        if let Some(output_stack) = stacks.iter().rev().find(|s| {
            s.kind == output.item_kind
                && s.material == output.material
                && s.quality == output.quality
                && s.owner.is_none()
                && s.reserved_by.is_none()
        }) {
            let stack_id = output_stack.id;
            for sub in subcomponent_records {
                let _ = self.db.item_subcomponents.insert_auto_no_fk(|seq| {
                    crate::db::ItemSubcomponent {
                        item_stack_id: stack_id,
                        seq,
                        component_kind: sub.input_kind,
                        material: None,
                        quality: 0,
                        quantity_per_item: sub.quantity_per_item,
                    }
                });
            }
        }
    }

    /// Clean up a Craft task on node invalidation: release reserved inputs in
    /// the workshop's inventory and set the task to Complete.
    pub(crate) fn cleanup_craft_task(&mut self, task_id: TaskId) {
        let structure_id =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::CraftAt) {
                Some(s) => s,
                None => return,
            };
        self.inv_clear_reservations(self.structure_inv(structure_id), task_id);
        if let Some(mut t) = self.db.tasks.get(&task_id) {
            t.state = task::TaskState::Complete;
            let _ = self.db.tasks.update_no_fk(t);
        }
    }

    /// Unified crafting monitor: scan all buildings with `crafting_enabled` and
    /// at least one `ActiveRecipe`, then create Craft tasks for the highest-
    /// priority recipe that has unmet targets and available inputs.
    ///
    /// All crafting buildings (kitchens, workshops, etc.) use this unified system.
    pub(crate) fn process_unified_crafting_monitor(&mut self) {
        let crafting_sids: Vec<StructureId> = self
            .db
            .structures
            .iter_all()
            .filter(|s| s.crafting_enabled && s.furnishing.is_some())
            .map(|s| s.id)
            .collect();

        let fruit_species: Vec<_> = self.db.fruit_species.iter_all().cloned().collect();

        for sid in crafting_sids {
            let structure = match self.db.structures.get(&sid) {
                Some(s) => s,
                None => continue,
            };
            let inv_id = structure.inventory_id;

            // Skip if there's already an active (non-Complete) Craft task for this building.
            let has_active_craft = self
                .db
                .task_structure_refs
                .by_structure_id(&sid, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|r| {
                    r.role == crate::db::TaskStructureRole::CraftAt
                        && self
                            .db
                            .tasks
                            .get(&r.task_id)
                            .is_some_and(|t| t.state != task::TaskState::Complete)
                });
            if has_active_craft {
                continue;
            }

            // Get active recipes in priority order (sort_order ascending).
            let active_recipes = self.db.active_recipes.by_structure_sort(
                &sid,
                tabulosity::MatchAll,
                tabulosity::QueryOpts::ASC,
            );

            // Find first recipe with unmet targets and available inputs.
            let mut chosen: Option<(
                ActiveRecipeId,
                crate::recipe::Recipe,
                Option<inventory::Material>,
                crate::recipe::ResolvedRecipe,
            )> = None;
            for ar in &active_recipes {
                if !ar.enabled {
                    continue;
                }

                let params = crate::recipe::RecipeParams {
                    material: ar.material,
                };
                let Some(resolved) = ar.recipe.resolve(&params, &self.config, &fruit_species)
                else {
                    continue;
                };

                let targets = self
                    .db
                    .active_recipe_targets
                    .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC);

                let runs_needed = self.compute_runs_needed(&resolved, &targets, inv_id);
                if runs_needed == 0 {
                    continue;
                }

                let all_available = resolved.inputs.iter().all(|input| {
                    self.inv_unreserved_item_count(inv_id, input.item_kind, input.material_filter)
                        >= input.quantity
                });
                if all_available {
                    chosen = Some((ar.id, ar.recipe, ar.material, resolved));
                    break;
                }
            }

            let (ar_id, recipe, material, resolved) = match chosen {
                Some(c) => c,
                None => continue,
            };

            let interior_pos = self.db.structures.get(&sid).unwrap().anchor;
            if self.nav_graph.find_nearest_node(interior_pos).is_none() {
                continue;
            }
            let location = interior_pos;

            let task_id = TaskId::new(&mut self.rng);
            for input in &resolved.inputs {
                self.inv_reserve_items(
                    inv_id,
                    input.item_kind,
                    input.material_filter,
                    input.quantity,
                    task_id,
                );
            }

            // Grow-verb recipes use multi-action (total_cost = number of
            // actions), all others use single-action (total_cost = work_ticks).
            let total_cost = if recipe.verb() == crate::recipe::RecipeVerb::Grow {
                let per_action = self.config.grow_recipes.grow_work_ticks_per_action.max(1);
                resolved.work_ticks.div_ceil(per_action) as i64
            } else {
                resolved.work_ticks as i64
            };

            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::Craft {
                    structure_id: sid,
                    active_recipe_id: ar_id,
                },
                state: task::TaskState::Available,
                location,
                progress: 0,
                total_cost,
                required_species: recipe.required_species(),
                origin: task::TaskOrigin::Automated,
                target_creature: None,
                restrict_to_creature_id: None,
                prerequisite_task_id: None,
            };
            self.insert_task(new_task);

            // Overwrite the TaskCraftData with the recipe + material + ar_id
            // (insert_task already created a row via task decomposition).
            if self.task_craft_data(task_id).is_some() {
                let _ = self.db.task_craft_data.modify_unchecked(&task_id, |d| {
                    d.recipe = recipe;
                    d.material = material;
                    d.active_recipe_id = ar_id;
                });
            }
        }
    }

    /// Compute how many runs of a recipe are needed to satisfy all output targets.
    /// Returns 0 if all targets are met or all targets are zero.
    pub(crate) fn compute_runs_needed(
        &self,
        resolved: &crate::recipe::ResolvedRecipe,
        targets: &[crate::db::ActiveRecipeTarget],
        inv_id: InventoryId,
    ) -> u32 {
        let mut runs_needed: u32 = 0;
        for target in targets {
            if target.target_quantity == 0 {
                continue;
            }
            let filter = match target.output_material {
                None => inventory::MaterialFilter::Any,
                Some(m) => inventory::MaterialFilter::Specific(m),
            };
            let stock = self.inv_unreserved_item_count(inv_id, target.output_item_kind, filter);
            let shortfall = target.target_quantity.saturating_sub(stock);
            if shortfall == 0 {
                continue;
            }
            let output_qty = resolved
                .outputs
                .iter()
                .find(|o| {
                    o.item_kind == target.output_item_kind && o.material == target.output_material
                })
                .map(|o| o.quantity.max(1))
                .unwrap_or(1);
            let runs_for_output = shortfall.div_ceil(output_qty);
            runs_needed = runs_needed.max(runs_for_output);
        }
        runs_needed
    }

    /// Compute the effective logistics wants for a building, merging explicit
    /// `LogisticsWantRow` entries with auto-logistics wants from active recipes.
    pub(crate) fn compute_effective_wants(
        &self,
        structure_id: StructureId,
    ) -> Vec<crate::building::LogisticsWant> {
        let structure = match self.db.structures.get(&structure_id) {
            Some(s) => s,
            None => return vec![],
        };
        let inv_id = structure.inventory_id;

        let mut merged: std::collections::BTreeMap<
            (inventory::ItemKind, inventory::MaterialFilter),
            u32,
        > = std::collections::BTreeMap::new();
        for row in self.inv_wants(inv_id) {
            let entry = merged
                .entry((row.item_kind, row.material_filter))
                .or_insert(0);
            *entry += row.target_quantity;
        }

        if structure.crafting_enabled {
            let fruit_species: Vec<_> = self.db.fruit_species.iter_all().cloned().collect();
            let active_recipes = self.db.active_recipes.by_structure_sort(
                &structure_id,
                tabulosity::MatchAll,
                tabulosity::QueryOpts::ASC,
            );
            for ar in &active_recipes {
                if !ar.enabled || !ar.auto_logistics {
                    continue;
                }

                let params = crate::recipe::RecipeParams {
                    material: ar.material,
                };
                let Some(resolved) = ar.recipe.resolve(&params, &self.config, &fruit_species)
                else {
                    continue;
                };

                let targets = self
                    .db
                    .active_recipe_targets
                    .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC);
                let runs_needed = self.compute_runs_needed(&resolved, &targets, inv_id);

                let total_runs = runs_needed + ar.spare_iterations;
                if total_runs == 0 {
                    continue;
                }

                for input in &resolved.inputs {
                    let auto_qty = input.quantity * total_runs;
                    let entry = merged
                        .entry((input.item_kind, input.material_filter))
                        .or_insert(0);
                    *entry += auto_qty;
                }
            }
        }

        merged
            .into_iter()
            .map(
                |((item_kind, material_filter), target_quantity)| crate::building::LogisticsWant {
                    item_kind,
                    material_filter,
                    target_quantity,
                },
            )
            .collect()
    }

    /// Set the unified crafting toggle for a building. Validates the structure
    /// exists and has a furnishing type. No-op for unfurnished structures.
    pub(crate) fn set_crafting_enabled(&mut self, structure_id: StructureId, enabled: bool) {
        if self
            .db
            .structures
            .get(&structure_id)
            .is_none_or(|s| s.furnishing.is_none())
        {
            return;
        }
        let _ = self.db.structures.modify_unchecked(&structure_id, |s| {
            s.crafting_enabled = enabled;
        });
    }

    /// Add a recipe to a building's active recipe list. Validates via
    /// `recipe.resolve()` and `recipe.furnishing_types()`. Rejects duplicates.
    pub(crate) fn add_active_recipe(
        &mut self,
        structure_id: StructureId,
        recipe: crate::recipe::Recipe,
        material: Option<inventory::Material>,
    ) {
        let Some(structure) = self.db.structures.get(&structure_id) else {
            return;
        };
        let Some(ft) = structure.furnishing else {
            return;
        };
        if !recipe.furnishing_types().contains(&ft) {
            return;
        }

        // Validate the recipe resolves with the given material.
        let fruit_species: Vec<_> = self.db.fruit_species.iter_all().cloned().collect();
        let params = crate::recipe::RecipeParams { material };
        let Some(resolved) = recipe.resolve(&params, &self.config, &fruit_species) else {
            return;
        };

        // Check for duplicate: same (recipe, material) already active on this structure.
        let existing = self
            .db
            .active_recipes
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
        if existing
            .iter()
            .any(|ar| ar.recipe == recipe && ar.material == material)
        {
            return;
        }

        let max_sort = self
            .db
            .active_recipes
            .iter_all()
            .map(|ar| ar.sort_order)
            .max()
            .unwrap_or(0);
        let sort_order = max_sort + 1;

        let outputs: Vec<_> = resolved
            .outputs
            .iter()
            .map(|o| (o.item_kind, o.material))
            .collect();

        let ar_id = self
            .db
            .insert_active_recipe_auto(|id| crate::db::ActiveRecipe {
                id,
                structure_id,
                recipe,
                material,
                enabled: true,
                sort_order,
                auto_logistics: true,
                spare_iterations: 0,
            })
            .expect("ActiveRecipe insert should not violate FK");

        for (item_kind, mat) in outputs {
            let _ = self
                .db
                .insert_active_recipe_target_auto(|id| crate::db::ActiveRecipeTarget {
                    id,
                    active_recipe_id: ar_id,
                    output_item_kind: item_kind,
                    output_material: mat,
                    target_quantity: 0,
                });
        }
    }

    /// Remove an active recipe. Interrupts any in-progress craft task for it.
    pub(crate) fn remove_active_recipe(&mut self, active_recipe_id: ActiveRecipeId) {
        let Some(ar) = self.db.active_recipes.get(&active_recipe_id) else {
            return;
        };

        // Find and interrupt any in-progress craft task referencing this recipe.
        for tcd in self
            .db
            .task_craft_data
            .iter_all()
            .filter(|tcd| tcd.active_recipe_id == active_recipe_id)
            .cloned()
            .collect::<Vec<_>>()
        {
            // Find the creature assigned to this task.
            let cid = self
                .db
                .creatures
                .iter_all()
                .find(|c| c.current_task == Some(tcd.task_id))
                .map(|c| c.id);
            if let Some(cid) = cid {
                self.interrupt_task(cid, tcd.task_id);
            }
        }

        // Cascade-delete removes ActiveRecipeTarget rows too.
        let _ = self.db.remove_active_recipe(&ar.id);
    }

    /// Set the target quantity for a specific recipe output.
    pub(crate) fn set_recipe_output_target(
        &mut self,
        target_id: ActiveRecipeTargetId,
        target_quantity: u32,
    ) {
        if self.db.active_recipe_targets.get(&target_id).is_none() {
            return;
        }
        let _ = self
            .db
            .active_recipe_targets
            .modify_unchecked(&target_id, |t| {
                t.target_quantity = target_quantity;
            });
    }

    /// Configure auto-logistics for an active recipe.
    pub(crate) fn set_recipe_auto_logistics(
        &mut self,
        active_recipe_id: ActiveRecipeId,
        auto_logistics: bool,
        spare_iterations: u32,
    ) {
        if self.db.active_recipes.get(&active_recipe_id).is_none() {
            return;
        }
        let _ = self
            .db
            .active_recipes
            .modify_unchecked(&active_recipe_id, |ar| {
                ar.auto_logistics = auto_logistics;
                ar.spare_iterations = spare_iterations;
            });
    }

    /// Toggle an individual active recipe.
    pub(crate) fn set_recipe_enabled(&mut self, active_recipe_id: ActiveRecipeId, enabled: bool) {
        if self.db.active_recipes.get(&active_recipe_id).is_none() {
            return;
        }
        let _ = self
            .db
            .active_recipes
            .modify_unchecked(&active_recipe_id, |ar| {
                ar.enabled = enabled;
            });
    }

    /// Move an active recipe up in priority (swap sort_order with the recipe
    /// above it within the same structure).
    pub(crate) fn move_active_recipe_up(&mut self, active_recipe_id: ActiveRecipeId) {
        let Some(ar) = self.db.active_recipes.get(&active_recipe_id) else {
            return;
        };
        // Find the recipe with the next-lower sort_order in the same structure.
        let siblings = self.db.active_recipes.by_structure_sort(
            &ar.structure_id,
            tabulosity::MatchAll,
            tabulosity::QueryOpts::ASC,
        );
        let mut prev: Option<ActiveRecipeId> = None;
        for sib in &siblings {
            if sib.id == active_recipe_id {
                break;
            }
            prev = Some(sib.id);
        }
        if let Some(prev_id) = prev {
            self.swap_active_recipe_sort_order(active_recipe_id, prev_id);
        }
    }

    /// Move an active recipe down in priority (swap sort_order with the recipe
    /// below it within the same structure).
    pub(crate) fn move_active_recipe_down(&mut self, active_recipe_id: ActiveRecipeId) {
        let Some(ar) = self.db.active_recipes.get(&active_recipe_id) else {
            return;
        };
        let siblings = self.db.active_recipes.by_structure_sort(
            &ar.structure_id,
            tabulosity::MatchAll,
            tabulosity::QueryOpts::ASC,
        );
        let mut found = false;
        let mut next: Option<ActiveRecipeId> = None;
        for sib in &siblings {
            if found {
                next = Some(sib.id);
                break;
            }
            if sib.id == active_recipe_id {
                found = true;
            }
        }
        if let Some(next_id) = next {
            self.swap_active_recipe_sort_order(active_recipe_id, next_id);
        }
    }

    /// Swap sort_order values between two active recipes. Removes and
    /// re-inserts both to avoid unique-index collision on the intermediate state.
    pub(crate) fn swap_active_recipe_sort_order(
        &mut self,
        id_a: ActiveRecipeId,
        id_b: ActiveRecipeId,
    ) {
        let mut row_a = self.db.active_recipes.get(&id_a).unwrap().clone();
        let mut row_b = self.db.active_recipes.get(&id_b).unwrap().clone();
        std::mem::swap(&mut row_a.sort_order, &mut row_b.sort_order);
        // Remove both to clear the unique index entries, then re-insert.
        let _ = self.db.active_recipes.remove_no_fk(&id_a);
        let _ = self.db.active_recipes.remove_no_fk(&id_b);
        let _ = self.db.active_recipes.insert_no_fk(row_a);
        let _ = self.db.active_recipes.insert_no_fk(row_b);
    }
}
