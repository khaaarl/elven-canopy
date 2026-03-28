// Creature skill advancement and speed effects (F-creature-skills, F-path-core).
//
// ## Advancement
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
// ## Path integration
//
// A creature's path (see `paths.rs`) affects skill advancement in two ways:
// - **Elevated cap**: skills associated with the path use a higher cap
//   (e.g., 200 for Warrior combat skills vs. default 100).
// - **Extra rolls**: associated skills get additional advancement rolls per
//   action (e.g., Warrior gets 2 rolls for Striking instead of 1).
//
// Both are configured per-path in `PathConfig` / `PathDef` (see `config.rs`).
//
// ## Speed effect
//
// Higher skill reduces task duration via additive stat+skill combination:
//
//   effective_ticks = apply_stat_divisor(base_ticks, stat + skill)
//
// Each action type pairs a relevant stat (e.g., Agility for melee, Dexterity
// for crafting) with its corresponding skill. The `skill_modified_duration()`
// helper computes the reduced duration.
//
// Skills are stored as `TraitKind` variants in the `creature_traits` table
// with `TraitValue::Int` values. Missing rows imply skill 0.
//
// See also: `stats.rs` for `SKILL_TRAIT_KINDS` and the exponential multiplier
// table, `config.rs` for `SkillConfig` and `PathConfig`, `types.rs` for skill
// `TraitKind` variants, `creature.rs` for `trait_int()` and `insert_trait()`,
// `paths.rs` for path assignment and cap/roll queries.

use crate::db::CreatureTrait;
use crate::types::{CreatureId, TraitKind, TraitValue};

impl super::SimState {
    /// Roll for skill advancement (learning) after a relevant action. The
    /// skill cap limits how high a creature can *learn* — it does not affect
    /// the benefit of skill already acquired.
    ///
    /// PRNG contract: always consumes exactly `1 + extra_rolls` PRNG calls
    /// regardless of outcome (cap, failed roll, or success). The base roll
    /// always consumes 1 call. If the creature's path grants extra
    /// advancement rolls for this skill, additional rolls are consumed
    /// (and each successful roll increments the skill by 1, up to the cap).
    pub(crate) fn try_advance_skill(
        &mut self,
        creature_id: CreatureId,
        skill: TraitKind,
        base_probability_permille: u32,
    ) {
        let extra_rolls = self.extra_advancement_rolls(creature_id, skill);
        let cap = self.skill_cap_for(creature_id, skill);
        let total_rolls = 1 + extra_rolls;

        for _ in 0..total_rolls {
            // Always consume 1 PRNG call per roll so the stream is
            // independent of whether any creature is at their skill cap.
            let roll = self.rng.next_u64() % 1000;

            let current = self.trait_int(creature_id, skill, 0);
            if current >= cap {
                continue;
            }

            let decay = self.config.skills.advancement_decay_base.max(1) as u64;
            let adjusted =
                base_probability_permille as u64 * decay / (decay + current.max(0) as u64);

            // Apply Intelligence multiplier (smarter creatures learn faster).
            let intelligence = self.trait_int(creature_id, TraitKind::Intelligence, 0);
            let adjusted = (crate::stats::apply_stat_multiplier(adjusted as i64, intelligence)
                as u64)
                .min(1000);

            if roll < adjusted {
                let new_val = current + 1;
                let _ = self.db.upsert_creature_trait(CreatureTrait {
                    creature_id,
                    trait_kind: skill,
                    value: TraitValue::Int(new_val),
                });
            }
        }
    }

    /// Compute a skill-modified task duration. The relevant stat and skill are
    /// added (additive combination) and fed into `apply_stat_divisor`, so
    /// higher stat+skill = fewer ticks. The raw skill value is used without
    /// capping — the skill cap only limits *learning* (advancement), not the
    /// benefit of skill already acquired. Returns at least 1 to prevent
    /// zero-duration actions.
    pub(crate) fn skill_modified_duration(
        &self,
        creature_id: CreatureId,
        base_ticks: u64,
        stat: TraitKind,
        skill: TraitKind,
    ) -> u64 {
        let stat_val = self.trait_int(creature_id, stat, 0);
        let skill_val = self.trait_int(creature_id, skill, 0);
        let combined = stat_val + skill_val;
        crate::stats::apply_stat_divisor(base_ticks as i64, combined).max(1) as u64
    }
}
