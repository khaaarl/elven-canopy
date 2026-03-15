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
            }
        }

        let pile_specs = self.config.initial_ground_piles.clone();
        for pile_spec in &pile_specs {
            let pos = self.find_surface_position(pile_spec.position.x, pile_spec.position.z);
            let pile_id = self.ensure_ground_pile(pos);
            let pile = self.db.ground_piles.get(&pile_id).unwrap();
            self.inv_add_item(
                pile.inventory_id,
                pile_spec.item_kind,
                pile_spec.quantity,
                None,
                None,
                pile_spec.material,
                0,
                None,
                None,
            );
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
    /// Flow:
    /// 1. Resolve the completed action's effects (e.g., delete MoveAction row).
    /// 2. Clear action state.
    /// 3. Decision cascade: continue task, find new task, or wander.
    pub(crate) fn process_creature_activation(
        &mut self,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) {
        let (mut current_node, species, action_kind) = {
            let creature = match self.db.creatures.get(&creature_id) {
                Some(c) if c.vital_status == VitalStatus::Alive => c,
                _ => return, // dead or missing — do not reschedule
            };
            let node = match creature.current_node {
                Some(n) => n,
                None => return,
            };
            (node, creature.species, creature.action_kind)
        };

        // Guard: if current_node is a dead slot (removed by incremental nav
        // update), abort any in-progress action and resnap the creature.
        if !self.graph_for_species(species).is_node_alive(current_node) {
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
                c.current_node = Some(new_node);
                c.position = new_pos;
                c.path = None;
            });
            self.update_creature_spatial_index(creature_id, species, pos, new_pos);
            // Action was aborted — skip resolve, schedule a fresh activation
            // so the creature can find a new task or wander.
            self.event_queue.schedule(
                self.tick + 1,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return;
        }

        // --- Step 1: Resolve completed action ---
        if action_kind == ActionKind::Move {
            // Move action completed — clean up the MoveAction row.
            let _ = self.db.move_actions.remove_no_fk(&creature_id);
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.action_kind = ActionKind::NoAction;
                c.next_available_tick = None;
            });
        }
        if action_kind == ActionKind::Build {
            // Resolve the Build action — materialize one voxel.
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.action_kind = ActionKind::NoAction;
                c.next_available_tick = None;
            });
            let completed = self.resolve_build_action(creature_id);
            // Re-read current_node: voxel materialization may have resnaped
            // the creature to a different node.
            current_node = match self
                .db
                .creatures
                .get(&creature_id)
                .and_then(|c| c.current_node)
            {
                Some(n) => n,
                None => return,
            };
            if !completed {
                // Task still in progress — re-enter task behavior to start the
                // next Build action (or walk if creature moved off the location).
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
            // Fall through to decision cascade (task completed or creature
            // lost its task during resolution).
        }

        // Resolve simple work actions (no nav graph changes, just clear state
        // and re-enter task behavior if not completed).
        if matches!(
            action_kind,
            ActionKind::Furnish
                | ActionKind::Cook
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

        // --- Flee check (F-flee) ---
        // Before the decision cascade, check if this creature should flee from
        // a nearby hostile (based on engagement style: passive initiative or
        // disengage threshold exceeded). If a flee step is taken, skip the
        // normal decision cascade entirely.
        if self.should_flee(creature_id, species)
            && self.flee_step(creature_id, current_node, species)
        {
            return;
        }

        // --- Autonomous combat check (F-engagement-style) ---
        // Non-passive creatures with detection range should interrupt
        // low-priority tasks to engage hostiles. This check runs before
        // the task execution so that an elf fetching clothes will stop
        // to shoot at a charging orc.
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
                // Check if the current task (if any) can be preempted by
                // autonomous combat. Only preempts Autonomous-level tasks
                // (haul, cook, craft, harvest, acquire items). Player-directed
                // commands and survival tasks are NOT interrupted — the player's
                // explicit orders take priority over autonomous behavior.
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
                    // No task — try combat before find_available_task claims
                    // a new one (otherwise the creature might pick up a
                    // non-preemptable task and stop fighting).
                    true
                };

                if should_try_combat {
                    // Check if there are actually hostiles to fight before
                    // interrupting the current task. Uses full detection range
                    // — defensive creatures can shoot at targets beyond their
                    // pursuit range. detect_hostile_targets is a read-only scan.
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
                        // Interrupt the task BEFORE calling hostile_pursue.
                        // hostile_pursue may schedule a new activation (via
                        // start_simple_action), and interrupt_task calls
                        // abort_current_action which cancels all scheduled
                        // activations — doing it after would wipe the new one.
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
                        // hostile_pursue failed despite targets detected
                        // (e.g., no path). Fall through to decision cascade.
                    }
                }
            }
        }

        // --- Step 2: Decision cascade ---
        // Re-read current_task since it may have changed during resolution.
        let current_task = self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task);

        if let Some(task_id) = current_task {
            // --- Has task: run task behavior ---
            self.execute_task_behavior(creature_id, task_id, current_node, events);
        } else {
            // --- No task: try to claim one, or wander ---
            if let Some(task_id) = self.find_available_task(creature_id) {
                self.claim_task(creature_id, task_id);
                // Run task behavior immediately on the same activation.
                self.execute_task_behavior(creature_id, task_id, current_node, events);
            } else {
                self.wander(creature_id, current_node, events);
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
        let current_node = creature.current_node?;

        // Collect all candidate tasks (id + location) that this creature can work.
        let candidates: Vec<(TaskId, NavNodeId)> = self
            .db
            .tasks
            .iter_all()
            .filter(|t| {
                t.state == task::TaskState::Available
                    && t.required_species.is_none_or(|s| s == species)
            })
            .map(|t| (t.id, t.location))
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // Single candidate — skip Dijkstra.
        if candidates.len() == 1 {
            return Some(candidates[0].0);
        }

        let target_nodes: Vec<NavNodeId> = candidates.iter().map(|&(_, loc)| loc).collect();

        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);

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
        candidates
            .iter()
            .find(|&&(_, loc)| loc == nearest_node)
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

    /// Execute one activation's worth of task behavior.
    pub(crate) fn execute_task_behavior(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        current_node: NavNodeId,
        events: &mut Vec<SimEvent>,
    ) {
        let (mut task_location, target_creature) = match self.db.tasks.get(&task_id) {
            Some(t) => (t.location, t.target_creature),
            None => {
                // Task was removed — abort action, unassign, and wander.
                self.abort_current_action(creature_id);
                if let Some(mut c) = self.db.creatures.get(&creature_id) {
                    c.current_task = None;
                    c.path = None;
                    let _ = self.db.creatures.update_no_fk(c);
                }
                self.wander(creature_id, current_node, events);
                return;
            }
        };

        // --- Dynamic pursuit: track moving target creature ---
        if let Some(target_id) = target_creature {
            let target_node = self
                .db
                .creatures
                .get(&target_id)
                .and_then(|c| c.current_node);
            match target_node {
                None => {
                    // Target creature is gone or has no nav node — abandon.
                    self.interrupt_task(creature_id, task_id);
                    self.wander(creature_id, current_node, events);
                    return;
                }
                Some(target_nav) => {
                    if target_nav != task_location {
                        // Target moved — update task location and invalidate path.
                        task_location = target_nav;
                        let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
                            t.location = target_nav;
                        });
                        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                            c.path = None;
                        });
                    }
                }
            }
        }

        // Check that both current_node and task_location are still alive in
        // the nav graph. They can become dead slots after incremental updates
        // (e.g. construction solidifying a voxel). If either is dead, abandon
        // the task and wander.
        let species = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| c.species)
            .unwrap_or(Species::Elf);
        let graph = self.graph_for_species(species);
        if !graph.is_node_alive(current_node) || !graph.is_node_alive(task_location) {
            // Clean up action and task state before abandoning.
            self.interrupt_task(creature_id, task_id);
            // Resnap the creature to a live node before wandering.
            let graph = self.graph_for_species(species);
            if let Some(c) = self.db.creatures.get(&creature_id) {
                let old_pos = c.position;
                if let Some(new_node) = graph.find_nearest_node(old_pos) {
                    let new_pos = graph.node(new_node).position;
                    let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                        c.current_node = Some(new_node);
                        c.position = new_pos;
                    });
                    self.update_creature_spatial_index(creature_id, species, old_pos, new_pos);
                    self.wander(creature_id, new_node, events);
                }
            }
            return;
        }

        // AttackTarget tasks: check combat range before walking. The creature
        // may be able to attack from a nearby node without reaching the exact
        // target location. Also handles pathfinding retry limits.
        let task_kind_tag = self
            .db
            .tasks
            .get(&task_id)
            .map(|t| t.kind_tag)
            .unwrap_or(crate::db::TaskKindTag::GoTo);
        if task_kind_tag == crate::db::TaskKindTag::AttackTarget {
            // Check if target is still alive before doing anything.
            if let Some(attack_data) = self.task_attack_target_data(task_id) {
                let target_id = attack_data.target;
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
            }

            // Try combat actions at range (melee or ranged).
            if self.try_attack_target_combat(creature_id, task_id, events) {
                return;
            }

            // Not in combat range — walk toward target.
            if current_node != task_location {
                self.walk_toward_attack_target(
                    creature_id,
                    task_id,
                    task_location,
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
            self.execute_attack_move(creature_id, task_id, task_location, current_node, events);
            return;
        }

        if current_node == task_location {
            // At task location — run the kind-specific completion/work logic.
            self.execute_task_at_location(creature_id, task_id, events);
        } else {
            // Not at location — walk one edge toward it.
            self.walk_toward_task(creature_id, task_location, current_node, events);
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
            crate::db::TaskKindTag::Cook => {
                self.start_cook_action(creature_id);
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

        // Determine mope location: assigned home nav node, else current node.
        let mope_node = self
            .find_assigned_home_bed(creature_id)
            .map(|(_, nav_node, _)| nav_node)
            .or_else(|| {
                self.db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| c.current_node)
            });
        let mope_node = match mope_node {
            Some(n) => n,
            None => return,
        };

        let task_id = TaskId::new(&mut self.rng);
        let duration = self.config.mood_consequences.mope_duration_ticks;
        let new_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Mope,
            state: task::TaskState::InProgress,
            location: mope_node,
            progress: 0.0,
            total_cost: duration as f32,
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
                progress: 0.0,
                total_cost: 0.0,
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
        let current_node = creature.current_node;

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
        let creature_pos = match current_node {
            Some(node) if self.nav_graph.is_node_alive(node) => self.nav_graph.node(node).position,
            _ => return,
        };
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
                progress: 0.0,
                total_cost: 0.0,
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
}
