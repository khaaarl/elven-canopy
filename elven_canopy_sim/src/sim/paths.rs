// Elf path assignment logic (F-path-core).
//
// Paths are disciplines elves commit to, affecting which skills receive
// elevated caps and extra advancement rolls. Currently three paths exist:
// Outcast (default, no bonuses), Warrior (Striking/Archery/Evasion), and
// Scout (Beastcraft/Ranging).
//
// This module handles:
// - Path assignment via `SimAction::AssignPath`
// - Querying a creature's current path
// - Skill cap lookups through path config
// - Auto-assigning Outcast to unpathed creatures on load
//
// See also: `types.rs` for `PathId` / `PathCategory`, `config.rs` for
// `PathConfig` / `PathDef`, `db.rs` for `PathAssignment` table,
// `skills.rs` for how paths modify skill advancement.

use crate::db::PathAssignment;
use crate::event::SimEventKind;
use crate::types::{CreatureId, PathId, Species, TraitKind};

impl super::SimState {
    /// Get the path assigned to a creature, or `None` if no assignment exists.
    pub fn creature_path(&self, creature_id: CreatureId) -> Option<PathId> {
        self.db
            .path_assignments
            .get(&creature_id)
            .map(|pa| pa.path_id)
    }

    /// Get the skill cap for a given creature and skill, accounting for their
    /// path. If the creature's path has an elevated cap for this skill, that
    /// cap is used; otherwise falls back to `SkillConfig::default_skill_cap`.
    pub(crate) fn skill_cap_for(&self, creature_id: CreatureId, skill: TraitKind) -> i64 {
        if let Some(path_id) = self.creature_path(creature_id)
            && let Some(path_def) = self.config.paths.paths.get(&path_id)
            && let Some(&cap) = path_def.skill_caps.get(&skill)
        {
            return cap;
        }
        self.config.skills.default_skill_cap
    }

    /// Get the number of extra advancement rolls for a creature's path and
    /// skill. Returns 0 if the creature has no path, the skill is not
    /// associated with the path, or the path is Outcast.
    pub(crate) fn extra_advancement_rolls(&self, creature_id: CreatureId, skill: TraitKind) -> u32 {
        if let Some(path_id) = self.creature_path(creature_id)
            && let Some(path_def) = self.config.paths.paths.get(&path_id)
            && path_def.skill_caps.contains_key(&skill)
        {
            return path_def.extra_advancement_rolls;
        }
        0
    }

    /// Assign a path to a creature. Replaces any existing assignment.
    /// Emits a `PathAssigned` event.
    pub(crate) fn assign_path(
        &mut self,
        creature_id: CreatureId,
        path_id: PathId,
        events: &mut Vec<crate::event::SimEvent>,
    ) {
        // Validate creature exists and is an elf. Non-elf creatures do not
        // participate in the path system.
        let Some(creature) = self.db.creatures.get(&creature_id) else {
            return;
        };
        if creature.species != Species::Elf {
            return;
        }

        // Upsert: replace old assignment if any, or insert new one.
        let _ = self.db.upsert_path_assignment(PathAssignment {
            creature_id,
            path_id,
        });

        events.push(crate::event::SimEvent {
            tick: self.tick,
            kind: SimEventKind::PathAssigned {
                creature_id,
                path_id,
            },
        });
    }

    /// Ensure all living elves have a path assignment. Called on load to
    /// backfill old saves where path assignments didn't exist.
    pub(crate) fn backfill_outcast_paths(&mut self) {
        let unpathed: Vec<CreatureId> = self
            .db
            .creatures
            .iter_all()
            .filter(|c| {
                c.species == Species::Elf
                    && c.vital_status != crate::types::VitalStatus::Dead
                    && self.db.path_assignments.get(&c.id).is_none()
            })
            .map(|c| c.id)
            .collect();

        for creature_id in unpathed {
            let _ = self.db.insert_path_assignment(PathAssignment {
                creature_id,
                path_id: PathId::Outcast,
            });
        }
    }
}
