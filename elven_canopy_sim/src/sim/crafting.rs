// Crafting and cooking system — recipe execution, active recipe management.
//
// Implements the unified crafting monitor that creates and manages crafting
// tasks based on active recipes at workstations. Handles both legacy cooking
// (bread from fruit) and the general crafting pipeline (component recipes).
// Includes recipe queue management (add, remove, reorder, enable/disable).
//
// See also: `recipe.rs` (recipe catalog and definitions), `logistics.rs`
// (item hauling to workstations), `inventory_mgmt.rs` (item operations).
use super::*;
use crate::db::ActionKind;
use crate::event::ScheduledEventKind;
use crate::inventory;
use crate::task;

impl SimState {
    /// Start a Cook action: set action kind and schedule next activation.
    /// Cook is a single-action task. Legacy path — new bread tasks go through
    /// `start_craft_action` via the unified crafting monitor.
    pub(crate) fn start_cook_action(&mut self, creature_id: CreatureId) {
        // Look up bread recipe work_ticks from catalog; fall back to 5000.
        let duration = self
            .recipe_catalog
            .iter()
            .find(|(_, d)| d.display_name == "Bread")
            .map(|(_, d)| d.work_ticks)
            .unwrap_or(5000);
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Cook;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Cook action: consume reserved fruit, produce bread.
    /// Always returns true (single-action task).
    pub(crate) fn resolve_cook_action(&mut self, creature_id: CreatureId) -> bool {
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
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::CookAt) {
                Some(s) => s,
                None => return false,
            };

        // Cooking complete — consume fruit, produce bread.
        let fruit_input = self.config.cook_fruit_input;
        let bread_output = self.config.cook_bread_output;
        let inv_id = self.structure_inv(structure_id);
        let removed = self.inv_remove_reserved_items(
            inv_id,
            inventory::ItemKind::Fruit,
            fruit_input,
            task_id,
        );
        if removed < fruit_input {
            self.inv_clear_reservations(inv_id, task_id);
        } else {
            self.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, bread_output, None, None);
        }
        self.complete_task(task_id);
        true
    }

    /// Clean up a Cook task on node invalidation: release reserved fruit in
    /// the kitchen's inventory and set the task to Complete so the kitchen
    /// monitor can create a fresh task on the next heartbeat.
    pub(crate) fn cleanup_cook_task(&mut self, task_id: TaskId) {
        let structure_id =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::CookAt) {
                Some(s) => s,
                None => return,
            };
        self.inv_clear_reservations(self.structure_inv(structure_id), task_id);
        if let Some(mut t) = self.db.tasks.get(&task_id) {
            t.state = task::TaskState::Complete;
            let _ = self.db.tasks.update_no_fk(t);
        }
    }

    /// Start a Craft action: set action kind and schedule next activation
    /// after `recipe.work_ticks`. Craft is a single-action task.
    pub(crate) fn start_craft_action(&mut self, creature_id: CreatureId, task_id: TaskId) {
        // Look up the recipe to get work_ticks. Try config recipes first,
        // then fall back to the unified recipe catalog.
        let craft_data = self.task_craft_data(task_id);
        let duration = craft_data
            .as_ref()
            .and_then(|d| {
                self.config
                    .recipes
                    .iter()
                    .find(|r| r.id == d.recipe_id)
                    .map(|r| r.work_ticks)
            })
            .or_else(|| {
                craft_data.as_ref().and_then(|d| {
                    d.active_recipe_id.and_then(|ar_id| {
                        self.db.active_recipes.get(&ar_id).and_then(|ar| {
                            crate::recipe::RecipeKey::from_json(&ar.recipe_key_json).and_then(
                                |key| self.recipe_catalog.get(&key).map(|def| def.work_ticks),
                            )
                        })
                    })
                })
            })
            .unwrap_or(5000);

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

    /// Resolve a completed Craft action: consume reserved inputs, produce
    /// outputs with subcomponents. Always returns true (single-action task).
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

        // Look up recipe via TaskCraftData. Try config recipes first (legacy
        // workshop path), then fall back to the unified recipe catalog.
        let craft_data = match self.task_craft_data(task_id) {
            Some(d) => d,
            None => {
                self.complete_task(task_id);
                return true;
            }
        };

        // Try config recipe lookup first (legacy path).
        let config_recipe = self
            .config
            .recipes
            .iter()
            .find(|r| r.id == craft_data.recipe_id)
            .cloned();

        // Catalog lookup via active_recipe_id (unified path). Cloned to
        // avoid holding an immutable borrow on self.recipe_catalog.
        let catalog_def = craft_data.active_recipe_id.and_then(|ar_id| {
            self.db.active_recipes.get(&ar_id).and_then(|ar| {
                crate::recipe::RecipeKey::from_json(&ar.recipe_key_json)
                    .and_then(|key| self.recipe_catalog.get(&key).cloned())
            })
        });

        let inv_id = self.structure_inv(structure_id);

        // Resolve via whichever path found the recipe.
        match (&config_recipe, &catalog_def) {
            (Some(recipe), _) => {
                // Legacy config recipe path.
                for input in &recipe.inputs {
                    self.inv_remove_reserved_items(
                        inv_id,
                        input.item_kind,
                        input.quantity,
                        task_id,
                    );
                }
                for output in &recipe.outputs {
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
                    self.record_subcomponents(inv_id, output, &recipe.subcomponent_records);
                }
            }
            (None, Some(def)) => {
                // Unified catalog recipe path.
                for input in &def.inputs {
                    self.inv_remove_reserved_items(
                        inv_id,
                        input.item_kind,
                        input.quantity,
                        task_id,
                    );
                }
                for output in &def.outputs {
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
                    self.record_subcomponents(inv_id, output, &def.subcomponent_records);
                }
            }
            (None, None) => {
                // Recipe not found in either path — skip.
            }
        }

        self.complete_task(task_id);
        true
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
                let _ = self.db.item_subcomponents.insert_auto_no_fk(|id| {
                    crate::db::ItemSubcomponent {
                        id,
                        item_stack_id: stack_id,
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
        // Collect structure IDs with crafting_enabled.
        let crafting_sids: Vec<StructureId> = self
            .db
            .structures
            .iter_all()
            .filter(|s| s.crafting_enabled && s.furnishing.is_some())
            .map(|s| s.id)
            .collect();

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
                    (r.role == crate::db::TaskStructureRole::CraftAt
                        || r.role == crate::db::TaskStructureRole::CookAt)
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
                crate::recipe::RecipeKey,
                crate::recipe::RecipeDef,
            )> = None;
            for ar in &active_recipes {
                if !ar.enabled {
                    continue;
                }

                let Some(recipe_key) = crate::recipe::RecipeKey::from_json(&ar.recipe_key_json)
                else {
                    continue;
                };
                let Some(recipe_def) = self.recipe_catalog.get(&recipe_key) else {
                    continue;
                };

                // Compute runs_needed from per-output targets.
                let targets = self
                    .db
                    .active_recipe_targets
                    .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC);

                let runs_needed = self.compute_runs_needed(recipe_def, &targets, inv_id);
                if runs_needed == 0 {
                    continue;
                }

                // Check if all inputs are available (unreserved).
                let all_available = recipe_def.inputs.iter().all(|input| {
                    self.inv_unreserved_item_count(inv_id, input.item_kind, input.material_filter)
                        >= input.quantity
                });
                if all_available {
                    chosen = Some((ar.id, recipe_key, recipe_def.clone()));
                    break;
                }
            }

            let (ar_id, recipe_key, recipe_def) = match chosen {
                Some(c) => c,
                None => continue,
            };

            // Find nav node inside the building.
            let interior_pos = self.db.structures.get(&sid).unwrap().anchor;
            let location = match self.nav_graph.find_nearest_node(interior_pos) {
                Some(n) => n,
                None => continue,
            };

            // Reserve all inputs.
            let task_id = TaskId::new(&mut self.rng);
            for input in &recipe_def.inputs {
                self.inv_reserve_items(
                    inv_id,
                    input.item_kind,
                    input.material_filter,
                    input.quantity,
                    task_id,
                );
            }

            // Create a Craft task. We reuse the existing Craft TaskKind,
            // storing the recipe's old-style ID for backward compatibility
            // with resolve_craft_action. We also look up the old recipe ID
            // from the catalog for the task kind.
            let recipe_id = self.recipe_key_to_legacy_id(&recipe_key);
            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::Craft {
                    structure_id: sid,
                    recipe_id,
                },
                state: task::TaskState::Available,
                location,
                progress: 0.0,
                total_cost: recipe_def.work_ticks as f32,
                required_species: recipe_def.required_species,
                origin: task::TaskOrigin::Automated,
                target_creature: None,
            };
            self.insert_task(new_task);

            // Update the TaskCraftData to record the active_recipe_id.
            if let Some(tcd) = self.task_craft_data(task_id) {
                let _ = self.db.task_craft_data.modify_unchecked(&tcd.id, |d| {
                    d.active_recipe_id = Some(ar_id);
                });
            }
        }
    }

    /// Compute how many runs of a recipe are needed to satisfy all output targets.
    /// Returns 0 if all targets are met or all targets are zero.
    pub(crate) fn compute_runs_needed(
        &self,
        recipe_def: &crate::recipe::RecipeDef,
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
            // Find the output quantity for this item in the recipe's outputs.
            // Use 1 as a fallback to avoid division by zero.
            let output_qty = recipe_def
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
    ///
    /// Auto-logistics wants are only generated for `ActiveRecipe` rows with
    /// `auto_logistics = true` and `enabled = true` on buildings with
    /// `crafting_enabled = true`. The auto-want for each input is
    /// `input.quantity * (runs_needed + spare_iterations)`, summed across all
    /// qualifying recipes. The final want per `(ItemKind, MaterialFilter)` is
    /// the **sum** of explicit wants and auto-wants (not max).
    pub(crate) fn compute_effective_wants(
        &self,
        structure_id: StructureId,
    ) -> Vec<crate::building::LogisticsWant> {
        let structure = match self.db.structures.get(&structure_id) {
            Some(s) => s,
            None => return vec![],
        };
        let inv_id = structure.inventory_id;

        // Start with explicit wants from the DB.
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

        // Add auto-logistics from active recipes if crafting is enabled.
        if structure.crafting_enabled {
            let active_recipes = self.db.active_recipes.by_structure_sort(
                &structure_id,
                tabulosity::MatchAll,
                tabulosity::QueryOpts::ASC,
            );
            for ar in &active_recipes {
                if !ar.enabled || !ar.auto_logistics {
                    continue;
                }

                let Some(recipe_key) = crate::recipe::RecipeKey::from_json(&ar.recipe_key_json)
                else {
                    continue;
                };
                let Some(recipe_def) = self.recipe_catalog.get(&recipe_key) else {
                    continue;
                };

                let targets = self
                    .db
                    .active_recipe_targets
                    .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC);
                let runs_needed = self.compute_runs_needed(recipe_def, &targets, inv_id);

                let total_runs = runs_needed + ar.spare_iterations;
                if total_runs == 0 {
                    continue;
                }

                for input in &recipe_def.inputs {
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

    /// Map a RecipeKey back to a legacy recipe ID string for backward
    /// compatibility with TaskKind::Craft { recipe_id }.
    pub(crate) fn recipe_key_to_legacy_id(&self, key: &crate::recipe::RecipeKey) -> String {
        // Check bread recipe first.
        let bread_key = {
            let def = self
                .recipe_catalog
                .iter()
                .find(|(_, d)| d.display_name == "Bread")
                .map(|(_, d)| d);
            def.map(|d| d.key.clone())
        };
        if bread_key.as_ref() == Some(key) {
            return "bread".to_string();
        }
        // Check config recipes.
        for recipe in &self.config.recipes {
            let def_key = crate::recipe::convert_config_recipe_key(recipe);
            if def_key == *key {
                return recipe.id.clone();
            }
        }
        // Fallback: serialize the key.
        key.to_json()
    }

    /// Remove `ActiveRecipe` rows whose `recipe_key_json` no longer matches
    /// any entry in the current recipe catalog. Called during
    /// `rebuild_transient_state()` (i.e., on save load) to handle recipes
    /// removed between game versions. Creates a notification for each orphan.
    /// Uses `db.remove_active_recipe()` (DB-level cascade) rather than
    /// `self.remove_active_recipe()` because no creatures are mid-action
    /// during transient state rebuild.
    pub(crate) fn cleanup_orphaned_active_recipes(&mut self) {
        let orphan_ids: Vec<(crate::types::ActiveRecipeId, String, String)> = self
            .db
            .active_recipes
            .iter_all()
            .filter(|ar| {
                crate::recipe::RecipeKey::from_json(&ar.recipe_key_json)
                    .and_then(|key| self.recipe_catalog.get(&key))
                    .is_none()
            })
            .map(|ar| {
                let building_name = self
                    .db
                    .structures
                    .get(&ar.structure_id)
                    .and_then(|s| s.name.clone())
                    .unwrap_or_else(|| "a building".to_string());
                (ar.id, ar.recipe_display_name.clone(), building_name)
            })
            .collect();
        for (ar_id, recipe_name, building_name) in &orphan_ids {
            let msg = format!(
                "Recipe \"{}\" on {} is no longer available and has been removed.",
                recipe_name, building_name
            );
            let _ = self
                .db
                .notifications
                .insert_auto_no_fk(|id| crate::db::Notification {
                    id,
                    tick: self.tick,
                    message: msg,
                });
            let _ = self.db.remove_active_recipe(ar_id);
        }
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

    /// Add a recipe to a building's active recipe list. Validates recipe exists
    /// in catalog, building's FurnishingType matches, and recipe isn't already
    /// active on this structure.
    pub(crate) fn add_active_recipe(
        &mut self,
        structure_id: StructureId,
        recipe_key: crate::recipe::RecipeKey,
    ) {
        let Some(structure) = self.db.structures.get(&structure_id) else {
            return;
        };
        let Some(ft) = structure.furnishing else {
            return;
        };
        let Some(recipe_def) = self.recipe_catalog.get(&recipe_key) else {
            return;
        };
        if !recipe_def.furnishing_types.contains(&ft) {
            return;
        }

        // Check for duplicate: same recipe_key already active on this structure.
        let key_json = recipe_key.to_json();
        let existing = self
            .db
            .active_recipes
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
        if existing.iter().any(|ar| ar.recipe_key_json == key_json) {
            return;
        }

        // Compute next sort_order (globally unique).
        let max_sort = self
            .db
            .active_recipes
            .iter_all()
            .map(|ar| ar.sort_order)
            .max()
            .unwrap_or(0);
        let sort_order = max_sort + 1;

        let display_name = recipe_def.display_name.clone();
        let outputs: Vec<_> = recipe_def
            .outputs
            .iter()
            .map(|o| (o.item_kind, o.material))
            .collect();

        // Insert the active recipe.
        let ar_id = self
            .db
            .insert_active_recipe_auto(|id| crate::db::ActiveRecipe {
                id,
                structure_id,
                recipe_key_json: key_json,
                recipe_display_name: display_name,
                enabled: true,
                sort_order,
                auto_logistics: true,
                spare_iterations: 0,
            })
            .expect("ActiveRecipe insert should not violate FK");

        // Create one target row per recipe output, all at 0.
        for (item_kind, material) in outputs {
            let _ = self
                .db
                .insert_active_recipe_target_auto(|id| crate::db::ActiveRecipeTarget {
                    id,
                    active_recipe_id: ar_id,
                    output_item_kind: item_kind,
                    output_material: material,
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
            .filter(|tcd| tcd.active_recipe_id == Some(active_recipe_id))
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
