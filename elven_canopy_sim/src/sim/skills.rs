// Creature skill advancement (F-creature-skills).
//
// Implements probabilistic skill advancement: each relevant action rolls
// against a base probability, modulated by current skill level (higher skill
// → lower chance) and the creature's Intelligence stat (higher INT → faster
// learning). The decay formula is:
//
//   adjusted = base_prob * decay_base / (decay_base + current_skill)
//   adjusted = apply_stat_multiplier(adjusted, intelligence)
//   adjusted = min(adjusted, 1000)
//
// Skills are stored as `TraitKind` variants in the `creature_traits` table
// with `TraitValue::Int` values. Missing rows imply skill 0.
//
// See also: `stats.rs` for `SKILL_TRAIT_KINDS` and the exponential multiplier
// table, `config.rs` for `SkillConfig`, `types.rs` for skill `TraitKind`
// variants, `creature.rs` for `trait_int()` and `insert_trait()`.

use crate::db::CreatureTrait;
use crate::types::{CreatureId, TraitKind, TraitValue};

impl super::SimState {
    /// Roll for skill advancement after a relevant action. Always consumes
    /// exactly 1 PRNG call regardless of outcome (cap, failed roll, or
    /// success) to keep the PRNG stream position-stable.
    pub(crate) fn try_advance_skill(
        &mut self,
        creature_id: CreatureId,
        skill: TraitKind,
        base_probability_permille: u32,
    ) {
        // Always consume 1 PRNG call so the stream is independent of whether
        // any creature is at their skill cap.
        let roll = self.rng.next_u64() % 1000;

        let current = self.trait_int(creature_id, skill, 0);
        let cap = self.config.skills.default_skill_cap;
        if current >= cap {
            return;
        }

        let decay = self.config.skills.advancement_decay_base.max(1) as u64;
        let adjusted = base_probability_permille as u64 * decay / (decay + current.max(0) as u64);

        // Apply Intelligence multiplier (smarter creatures learn faster).
        let intelligence = self.trait_int(creature_id, TraitKind::Intelligence, 0);
        let adjusted =
            (crate::stats::apply_stat_multiplier(adjusted as i64, intelligence) as u64).min(1000);

        if roll < adjusted {
            let new_val = current + 1;
            if self.db.creature_traits.get(&(creature_id, skill)).is_some() {
                let _ = self
                    .db
                    .creature_traits
                    .modify_unchecked(&(creature_id, skill), |row| {
                        row.value = TraitValue::Int(new_val)
                    });
            } else {
                let _ = self.db.creature_traits.insert_no_fk(CreatureTrait {
                    creature_id,
                    trait_kind: skill,
                    value: TraitValue::Int(new_val),
                });
            }
        }
    }
}
