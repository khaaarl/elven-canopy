// Social opinion system (F-social-opinions, F-casual-social).
//
// Implements interpersonal opinions between creatures: the skill check that
// determines how much of an impression one creature makes on another, the
// upsert/decay logic for opinion rows, the pre-game relationship bootstrap
// for starting elves, and heartbeat-driven casual social interactions.
//
// The skill check follows the established pattern used by combat, taming,
// and crafting: `ability_score(s) + skill + quasi_normal(rng, 50)`. The
// result is mapped to a small signed intensity delta (+2, +1, 0, or -1)
// that is upserted into the `CreatureOpinion` table.
//
// **Casual social** (F-casual-social): During each creature heartbeat, a
// PPM roll may trigger a casual interaction with a nearby same-civ
// creature. Both creatures perform BestSocial skill checks that upsert
// Friendliness opinions and award mood thoughts (pleasant/awkward chat).
// Threshold crossings generate player-visible notifications.
//
// See also: `db.rs::CreatureOpinion` (table schema), `types.rs::OpinionKind`
// (opinion kinds), `types.rs::FriendshipCategory` (threshold tiers),
// `config.rs::SocialConfig` (tuning parameters), `mod.rs` (heartbeat-driven
// decay and casual social rolls in `CreatureHeartbeat` handler).

use super::*;
use crate::db::CreatureOpinion;
use crate::types::{FriendshipCategory, OpinionKind, TraitKind};

/// Determines which skill is used for a social impression roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkillPicker {
    /// Use Culture skill (dancing, ceremonies). Used by F-social-dance.
    Culture,
    /// Use max(Influence, Culture) — casual social interactions where the
    /// creature plays to their strength.
    BestSocial,
}

/// Map a social impression roll to an opinion intensity delta.
///
/// | Roll   | Delta |
/// |--------|-------|
/// | > 50   | +2    |
/// | 1–50   | +1    |
/// | -49–0  | 0     |
/// | ≤ -50  | -1    |
pub(crate) fn social_impression_delta(roll: i64) -> i64 {
    match roll {
        51.. => 2,
        1..=50 => 1,
        -49..=0 => 0,
        _ => -1, // ≤ -50
    }
}

impl SimState {
    /// Pick the relevant skill TraitKind for a social interaction.
    pub(crate) fn social_skill_trait(
        &self,
        creature_id: CreatureId,
        picker: SkillPicker,
    ) -> TraitKind {
        match picker {
            SkillPicker::Culture => TraitKind::Culture,
            SkillPicker::BestSocial => {
                let influence = self.trait_int(creature_id, TraitKind::Influence, 0);
                let culture = self.trait_int(creature_id, TraitKind::Culture, 0);
                if influence >= culture {
                    TraitKind::Influence
                } else {
                    TraitKind::Culture
                }
            }
        }
    }

    /// Map a Friendliness intensity to a coarse `FriendshipCategory` using
    /// the thresholds in `SocialConfig`. Used for UI labels and for
    /// detecting threshold crossings that trigger notifications.
    pub fn friendship_category(&self, intensity: i64) -> FriendshipCategory {
        let cfg = &self.config.social;
        if intensity >= cfg.friendship_friend_threshold {
            FriendshipCategory::Friend
        } else if intensity >= cfg.friendship_acquaintance_threshold {
            FriendshipCategory::Acquaintance
        } else if intensity <= cfg.friendship_enemy_threshold {
            FriendshipCategory::Enemy
        } else if intensity <= cfg.friendship_disliked_threshold {
            FriendshipCategory::Disliked
        } else {
            FriendshipCategory::Neutral
        }
    }

    /// Compute the social impression delta that `target_id` makes, using
    /// their CHA stat and the skill selected by `picker`. Used by
    /// F-social-dance and F-casual-social for runtime social interactions.
    pub(crate) fn social_impression(&mut self, target_id: CreatureId, picker: SkillPicker) -> i64 {
        let skill = self.social_skill_trait(target_id, picker);
        let roll = self.skill_check(target_id, &[TraitKind::Charisma], skill);
        social_impression_delta(roll)
    }

    /// Upsert a creature's opinion: if a row exists, add `delta` to intensity;
    /// otherwise insert a new row with intensity = delta. Rows reaching
    /// intensity 0 are pruned. For Friendliness opinions, detects threshold
    /// crossings and emits a player-visible notification.
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
        let old_intensity = self
            .db
            .creature_opinions
            .get(&key)
            .map_or(0, |o| o.intensity);
        let new_intensity = old_intensity + delta;

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

        // Friendship threshold-crossing notification (F-casual-social).
        if kind == OpinionKind::Friendliness {
            let old_cat = self.friendship_category(old_intensity);
            let new_cat = self.friendship_category(new_intensity);
            if old_cat != new_cat {
                let creature_name = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "???".into());
                let target_name = self
                    .db
                    .creatures
                    .get(&target_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "???".into());
                let label = new_cat.label();
                if !label.is_empty() {
                    self.add_notification(format!(
                        "{creature_name} now considers {target_name}: {label}"
                    ));
                }
            }
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

    /// Attempt a casual social interaction for `creature_id` with a nearby
    /// same-civ creature. Called from the creature heartbeat after a PPM
    /// probability roll passes. Both creatures perform BestSocial skill
    /// checks, upsert Friendliness opinions, attempt skill advancement,
    /// and receive mood thoughts.
    pub(crate) fn try_casual_social(&mut self, creature_id: CreatureId) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return,
        };
        let civ_id = match creature.civ_id {
            Some(civ) => civ,
            None => return, // non-civ creatures don't do casual social
        };
        let pos = creature.position.min;
        let radius = self.config.social.casual_social_radius;

        // Scan nearby voxels for same-civ alive creatures.
        let mut best: Option<(CreatureId, u32)> = None; // (id, manhattan_dist)
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                for dz in -radius..=radius {
                    let dist = dx.unsigned_abs() + dy.unsigned_abs() + dz.unsigned_abs();
                    if dist == 0 || dist > radius as u32 {
                        continue;
                    }
                    let voxel = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
                    for &other_id in &self.creatures_at_voxel(voxel) {
                        if other_id == creature_id {
                            continue;
                        }
                        let other = match self.db.creatures.get(&other_id) {
                            Some(c)
                                if c.vital_status == VitalStatus::Alive
                                    && c.civ_id == Some(civ_id) =>
                            {
                                c
                            }
                            _ => continue,
                        };
                        // Also check same-voxel creatures of the initiator's own voxel
                        // are handled by dx=dy=dz=0 skip — but creatures at voxel `pos`
                        // with dx=0 are not iterated. We handle that below.
                        let _ = other; // used above for filtering
                        let is_better = match best {
                            None => true,
                            Some((_, best_dist)) => {
                                dist < best_dist
                                    || (dist == best_dist && other_id < best.unwrap().0)
                            }
                        };
                        if is_better {
                            best = Some((other_id, dist));
                        }
                    }
                }
            }
        }
        // Also check the initiator's own voxel (dx=dy=dz=0, dist=0 was skipped).
        for &other_id in &self.creatures_at_voxel(pos) {
            if other_id == creature_id {
                continue;
            }
            if let Some(c) = self.db.creatures.get(&other_id)
                && c.vital_status == VitalStatus::Alive
                && c.civ_id == Some(civ_id)
            {
                let is_better = match best {
                    None => true,
                    Some((best_id, best_dist)) => best_dist > 0 || other_id < best_id,
                };
                if is_better {
                    best = Some((other_id, 0));
                }
            }
        }

        let target_id = match best {
            Some((id, _)) => id,
            None => return,
        };

        // Bidirectional interaction: both creatures impress the other.
        let delta_a = self.social_impression(target_id, SkillPicker::BestSocial);
        let delta_b = self.social_impression(creature_id, SkillPicker::BestSocial);

        self.upsert_opinion(creature_id, OpinionKind::Friendliness, target_id, delta_a);
        self.upsert_opinion(target_id, OpinionKind::Friendliness, creature_id, delta_b);

        // Skill advancement for both creatures.
        let skill_prob = self.config.social.skill_advance_probability_permille;
        let advance_skill = |sim: &mut Self, cid: CreatureId| {
            let influence = sim.trait_int(cid, TraitKind::Influence, 0);
            let culture = sim.trait_int(cid, TraitKind::Culture, 0);
            let skill = if influence >= culture {
                TraitKind::Influence
            } else {
                TraitKind::Culture
            };
            sim.try_advance_skill(cid, skill, skill_prob);
        };
        advance_skill(self, creature_id);
        advance_skill(self, target_id);

        // Thoughts: positive or negative chat based on delta.
        // creature_id received delta_a (impression target made on them).
        // target_id received delta_b (impression creature made on them).
        let target_name = self
            .db
            .creatures
            .get(&target_id)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let creature_name = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| c.name.clone())
            .unwrap_or_default();

        if delta_a > 0 {
            self.add_creature_thought(
                creature_id,
                ThoughtKind::HadPleasantChat(target_name.clone()),
            );
        } else if delta_a < 0 {
            self.add_creature_thought(creature_id, ThoughtKind::HadAwkwardChat(target_name));
        }
        if delta_b > 0 {
            self.add_creature_thought(
                target_id,
                ThoughtKind::HadPleasantChat(creature_name.clone()),
            );
        } else if delta_b < 0 {
            self.add_creature_thought(target_id, ThoughtKind::HadAwkwardChat(creature_name));
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
                    let skill_trait = self.social_skill_trait(target, SkillPicker::BestSocial);
                    let roll = self.skill_check(target, &[TraitKind::Charisma], skill_trait);
                    let delta = social_impression_delta(roll);
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
