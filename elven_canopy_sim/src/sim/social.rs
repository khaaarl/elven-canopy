// Social opinion system (F-social-opinions).
//
// Implements interpersonal opinions between creatures: the skill check that
// determines how much of an impression one creature makes on another, the
// upsert/decay logic for opinion rows, and the pre-game relationship
// bootstrap for starting elves.
//
// The skill check follows the established pattern used by combat, taming,
// and crafting: `ability_score(s) + skill + quasi_normal(rng, 50)`. The
// result is mapped to a small signed intensity delta (+2, +1, 0, or -1)
// that is upserted into the `CreatureOpinion` table.
//
// See also: `db.rs::CreatureOpinion` (table schema), `types.rs::OpinionKind`
// (opinion kinds), `config.rs::SocialConfig` (tuning parameters),
// `mod.rs` (heartbeat-driven decay roll in `CreatureHeartbeat` handler).

use super::*;
use crate::db::CreatureOpinion;
use crate::types::{OpinionKind, TraitKind};

/// Determines which skill is used for a social impression roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkillPicker {
    /// Use Culture skill (dancing, ceremonies). Used by F-social-dance.
    #[expect(dead_code)]
    Culture,
    /// Use max(Influence, Culture) — casual social interactions where the
    /// creature plays to their strength.
    BestSocial,
}

/// Roll a social impression check for `target_id` and return the opinion
/// intensity delta. The target's CHA + relevant skill + quasi_normal(50)
/// determines the roll; the result is mapped to a delta:
///
/// | Roll   | Delta |
/// |--------|-------|
/// | > 50   | +2    |
/// | 1–50   | +1    |
/// | -49–0  | 0     |
/// | ≤ -50  | -1    |
///
/// Always consumes exactly 12 PRNG calls (from `quasi_normal`).
pub(crate) fn social_impression_delta(
    cha: i64,
    skill_value: i64,
    rng: &mut elven_canopy_prng::GameRng,
) -> i64 {
    let roll = cha + skill_value + elven_canopy_prng::quasi_normal(rng, 50);
    match roll {
        51.. => 2,
        1..=50 => 1,
        -49..=0 => 0,
        _ => -1, // ≤ -50
    }
}

impl SimState {
    /// Pick the relevant skill value for a social interaction.
    pub(crate) fn social_skill_value(&self, creature_id: CreatureId, picker: SkillPicker) -> i64 {
        match picker {
            SkillPicker::Culture => self.trait_int(creature_id, TraitKind::Culture, 0),
            SkillPicker::BestSocial => {
                let influence = self.trait_int(creature_id, TraitKind::Influence, 0);
                let culture = self.trait_int(creature_id, TraitKind::Culture, 0);
                influence.max(culture)
            }
        }
    }

    /// Compute the social impression delta that `target_id` makes, using
    /// their CHA stat and the skill selected by `picker`. Used by
    /// F-social-dance and F-casual-social for runtime social interactions.
    #[expect(dead_code)]
    pub(crate) fn social_impression(
        &self,
        target_id: CreatureId,
        picker: SkillPicker,
        rng: &mut elven_canopy_prng::GameRng,
    ) -> i64 {
        let cha = self.trait_int(target_id, TraitKind::Charisma, 0);
        let skill = self.social_skill_value(target_id, picker);
        social_impression_delta(cha, skill, rng)
    }

    /// Upsert a creature's opinion: if a row exists, add `delta` to intensity;
    /// otherwise insert a new row with intensity = delta. Rows reaching
    /// intensity 0 are pruned.
    pub(crate) fn upsert_opinion(
        &mut self,
        creature_id: CreatureId,
        kind: OpinionKind,
        target_id: CreatureId,
        delta: i64,
    ) {
        if delta == 0 {
            return;
        }
        let key = (creature_id, kind, target_id);
        let new_intensity = if let Some(existing) = self.db.creature_opinions.get(&key) {
            existing.intensity + delta
        } else {
            delta
        };
        if new_intensity == 0 {
            let _ = self.db.remove_creature_opinion(&key);
        } else {
            let _ = self.db.upsert_creature_opinion(CreatureOpinion {
                creature_id,
                kind,
                target_id,
                intensity: new_intensity,
            });
        }
    }

    /// Decay all opinion rows for a creature by 1 toward zero. Prune rows
    /// that reach 0. Called from the creature heartbeat on a probability roll.
    pub(crate) fn decay_opinions(&mut self, creature_id: CreatureId) {
        let opinions: Vec<_> = self
            .db
            .creature_opinions
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        for op in opinions {
            let new_intensity = if op.intensity > 0 {
                op.intensity - 1
            } else {
                op.intensity + 1
            };
            if new_intensity == 0 {
                let _ = self
                    .db
                    .remove_creature_opinion(&(op.creature_id, op.kind, op.target_id));
            } else {
                let _ = self.db.upsert_creature_opinion(CreatureOpinion {
                    intensity: new_intensity,
                    ..op
                });
            }
        }
    }

    /// Simulate pre-game social interactions between starting elves so they
    /// begin with existing relationships and social skill development. Each
    /// ordered pair (A, B) gets a uniformly random number of interactions in
    /// `[bootstrap_min, bootstrap_max]`. Each interaction runs a BestSocial
    /// skill check and upserts a Friendliness opinion, plus attempts skill
    /// advancement for the acting creature.
    pub(crate) fn bootstrap_social_opinions(&mut self, elf_ids: &[CreatureId]) {
        let min = self.config.social.bootstrap_interactions_min;
        let max = self.config.social.bootstrap_interactions_max;
        if min == 0 && max == 0 {
            return;
        }
        let range = max.saturating_sub(min).saturating_add(1); // inclusive range

        let skill_advance_prob = self.config.social.skill_advance_probability_permille;

        for i in 0..elf_ids.len() {
            for j in 0..elf_ids.len() {
                if i == j {
                    continue;
                }
                let subject = elf_ids[i];
                let target = elf_ids[j];

                // Random interaction count for this pair.
                let count = min + (self.rng.next_u64() % range as u64) as u32;

                for _ in 0..count {
                    let cha = self.trait_int(target, TraitKind::Charisma, 0);
                    let skill = self.social_skill_value(target, SkillPicker::BestSocial);
                    let delta = social_impression_delta(cha, skill, &mut self.rng);
                    self.upsert_opinion(subject, OpinionKind::Friendliness, target, delta);

                    // Advance the subject's social skill (whichever is higher).
                    let influence_val = self.trait_int(subject, TraitKind::Influence, 0);
                    let culture_val = self.trait_int(subject, TraitKind::Culture, 0);
                    let skill_to_advance = if influence_val >= culture_val {
                        TraitKind::Influence
                    } else {
                        TraitKind::Culture
                    };
                    self.try_advance_skill(subject, skill_to_advance, skill_advance_prob);
                }
            }
        }
    }
}
