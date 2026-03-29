// Creature taming logic (F-taming).
//
// Scout-path elves can tame neutral creatures. The player designates a target
// via the creature info panel; any idle Scout claims the open taming task,
// walks to the creature, and makes repeated probabilistic attempts until
// success. Each attempt rolls `(WIL + CHA + Beastcraft) + quasi_normal(rng, 50)`
// against the species' `tame_difficulty` threshold.
//
// This module handles:
// - `SimAction::DesignateTame` and `SimAction::CancelTameDesignation`
// - Taming task creation and cancellation
// - Tame task execution at location (start TameAttempt action)
// - Taming roll resolution (success/fail, skill advancement, civ change)
//
// See also: `db.rs` for `TameDesignation` / `TaskTameData` / `ActionKind::TameAttempt`,
// `species.rs` for `tame_difficulty` on `SpeciesData`, `config.rs` for
// `tame_attempt_ticks` / `tame_skill_advance_probability`, `task.rs` for
// `TaskKind::Tame`, `activation.rs` for Scout-path claim filter,
// `paths.rs` for Scout-path gating.

use crate::db::{ActionKind, TameDesignation};
use crate::event::{SimEvent, SimEventKind};
use crate::task::{self, TaskOrigin, TaskState};
use crate::types::{CreatureId, Species, TraitKind, VitalStatus};

impl super::SimState {
    /// Handle `SimAction::DesignateTame`: validate the target, insert a
    /// `TameDesignation`, and create an open `Tame` task.
    pub(crate) fn handle_designate_tame(
        &mut self,
        target_id: CreatureId,
        _events: &mut Vec<SimEvent>,
    ) {
        // Validate target creature exists and is alive.
        let creature = match self.db.creatures.get(&target_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };

        // Must be wild (civ_id: None) — not a civ member.
        if creature.civ_id.is_some() {
            return;
        }

        // Species must be tameable.
        let species_data = match self.config.species.get(&creature.species) {
            Some(sd) => sd,
            None => return,
        };
        if species_data.tame_difficulty.is_none() {
            return;
        }

        // Not already designated.
        if self.db.tame_designations.get(&target_id).is_some() {
            return;
        }

        // Insert designation.
        let _ = self.db.insert_tame_designation(TameDesignation {
            creature_id: target_id,
            designated_tick: self.tick,
        });

        // Create the taming task at the target's current position.
        let location = creature.position;
        let task = task::Task {
            id: crate::types::TaskId::new(&mut self.rng),
            kind: task::TaskKind::Tame { target: target_id },
            state: TaskState::Available,
            location,
            progress: 0,
            total_cost: 0, // no progress bar — independent rolls
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: Some(target_id),
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: self.player_civ_id,
        };
        self.insert_task(task);
    }

    /// Handle `SimAction::CancelTameDesignation`: remove the designation and
    /// cancel any in-progress taming task on that creature.
    pub(crate) fn handle_cancel_tame_designation(&mut self, target_id: CreatureId) {
        // Remove designation (if present).
        let _ = self.db.remove_tame_designation(&target_id);

        // Find and cancel any Tame task targeting this creature.
        let task_id = self
            .db
            .task_tame_data
            .iter_all()
            .find(|td| td.target == target_id)
            .map(|td| td.task_id);

        if let Some(tid) = task_id {
            self.complete_task(tid);
        }
    }

    /// Execute a Tame task when the Scout has arrived at the target's location.
    /// Checks if the target is still alive; if so, starts a TameAttempt action.
    /// If dead, completes the task and removes the designation.
    pub(crate) fn execute_tame_at_location(
        &mut self,
        creature_id: CreatureId,
        task_id: crate::types::TaskId,
        _events: &mut Vec<SimEvent>,
    ) {
        // Look up the target from the extension table.
        let target_id = match self.db.task_tame_data.get(&task_id) {
            Some(td) => td.target,
            None => {
                self.complete_task(task_id);
                return;
            }
        };

        // Check target vital status — complete silently if dead.
        let target_alive = self
            .db
            .creatures
            .get(&target_id)
            .is_some_and(|c| c.vital_status == VitalStatus::Alive);
        if !target_alive {
            // Clean up designation and complete.
            let _ = self.db.remove_tame_designation(&target_id);
            self.complete_task(task_id);
            return;
        }

        // Re-validate target isn't already tamed (multiplayer race).
        let target_civ = self.db.creatures.get(&target_id).and_then(|c| c.civ_id);
        if target_civ.is_some() {
            let _ = self.db.remove_tame_designation(&target_id);
            self.complete_task(task_id);
            return;
        }

        // Start a TameAttempt action.
        let duration = self.config.tame_attempt_ticks;
        self.start_simple_action(creature_id, ActionKind::TameAttempt, duration);
    }

    /// Resolve a completed TameAttempt action. Roll the taming check; on
    /// success, change the target's civ_id and emit CreatureTamed. On failure,
    /// re-activate for another attempt.
    pub(crate) fn resolve_tame_attempt(
        &mut self,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        // Find the tame task and its target.
        let task_id = match self.db.creatures.get(&creature_id) {
            Some(c) => c.current_task,
            None => return false,
        };
        let task_id = match task_id {
            Some(tid) => tid,
            None => return false,
        };
        let target_id = match self.db.task_tame_data.get(&task_id) {
            Some(td) => td.target,
            None => return false,
        };

        // Check target is still alive and wild.
        let (target_alive, target_wild, target_species) = match self.db.creatures.get(&target_id) {
            Some(c) => (
                c.vital_status == VitalStatus::Alive,
                c.civ_id.is_none(),
                c.species,
            ),
            None => (false, false, Species::Elf),
        };
        if !target_alive || !target_wild {
            let _ = self.db.remove_tame_designation(&target_id);
            self.complete_task(task_id);
            return true;
        }

        // Look up tame_difficulty.
        let tame_difficulty = match self.config.species.get(&target_species) {
            Some(sd) => match sd.tame_difficulty {
                Some(d) => d,
                None => {
                    self.complete_task(task_id);
                    return true;
                }
            },
            None => {
                self.complete_task(task_id);
                return true;
            }
        };

        // Roll: (WIL + CHA + Beastcraft) + quasi_normal(rng, 50) >= tame_difficulty.
        let tamer_total = self.skill_check(
            creature_id,
            &[TraitKind::Willpower, TraitKind::Charisma],
            TraitKind::Beastcraft,
        );
        let success = tamer_total >= tame_difficulty;

        // Advance Beastcraft skill (always, regardless of outcome).
        let base_prob = self.config.tame_skill_advance_probability;
        self.try_advance_skill(creature_id, TraitKind::Beastcraft, base_prob);

        if success {
            // Tamed! Change target's civ_id to the tamer's.
            let tamer_civ = self.db.creatures.get(&creature_id).and_then(|c| c.civ_id);
            if let Some(civ_id) = tamer_civ
                && let Some(mut target) = self.db.creatures.get(&target_id)
            {
                target.civ_id = Some(civ_id);
                let _ = self.db.update_creature(target);
            }

            // Remove designation and complete task.
            let _ = self.db.remove_tame_designation(&target_id);

            // Create notification.
            let tamer_name = self
                .db
                .creatures
                .get(&creature_id)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            let species_name = format!("{target_species:?}");
            self.add_notification(format!("{tamer_name} tamed {species_name}!"));

            // Emit event.
            events.push(SimEvent {
                tick: self.tick,
                kind: SimEventKind::CreatureTamed {
                    creature_id: target_id,
                    tamer_id: creature_id,
                },
            });

            self.complete_task(task_id);
            true
        } else {
            // Failed — schedule another attempt. The activation loop will
            // re-invoke execute_tame_at_location, which re-checks target
            // status and starts another TameAttempt action.
            false
        }
    }
}
