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
        // TODO(F-zone-world): handle None zone_id (in-transit creature)
        let zone_id = creature.zone_id.unwrap_or_else(|| self.home_zone_id());
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
                    for &other_id in &self.creatures_at_voxel(zone_id, voxel) {
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
        for &other_id in &self.creatures_at_voxel(zone_id, pos) {
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

        // LLM delegation: if both creatures are idle/Autonomous and neither
        // has a pending LLM request, emit a social chat request and assign
        // both to Conversing tasks. The LLM adds dialogue text and bonus
        // effects; the mechanical resolution above is independent.
        self.try_emit_social_chat_request(creature_id, target_id);
    }

    /// Check whether both creatures are eligible for LLM-delegated conversation
    /// and, if so, assign both to Conversing tasks and emit an OutboundRequest.
    fn try_emit_social_chat_request(&mut self, creature_id: CreatureId, target_id: CreatureId) {
        // Debug prints throughout this function are intentionally unconditional.
        // The LLM pipeline has many failure modes (eligibility, relay routing,
        // inference, JSON parsing, deadline expiry) and these prints are the
        // primary diagnostic tool. Keep them until the feature is mature.
        let creature_name = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let target_name = self
            .db
            .creatures
            .get(&target_id)
            .map(|c| c.name.clone())
            .unwrap_or_default();

        // Both must be alive (dead/incapacitated creatures stay in the DB).
        let is_alive = |id: CreatureId| {
            self.db
                .creatures
                .get(&id)
                .is_some_and(|c| c.vital_status == VitalStatus::Alive)
        };
        if !is_alive(creature_id) || !is_alive(target_id) {
            eprintln!("[SOCIAL CHAT] skip {creature_name} -> {target_name}: one is not alive");
            return;
        }

        // Neither may have a pending LLM request.
        if self
            .pending_llm_requests
            .values()
            .any(|r| r.creature_id == creature_id || r.creature_id == target_id)
        {
            eprintln!("[SOCIAL CHAT] skip {creature_name} -> {target_name}: pending LLM request");
            return;
        }

        // Both must be idle or in an Autonomous-level task (preemptable to
        // Conversing without violating the preemption hierarchy).
        if !self.is_idle_or_autonomous(creature_id) || !self.is_idle_or_autonomous(target_id) {
            eprintln!("[SOCIAL CHAT] skip {creature_name} -> {target_name}: not idle/autonomous");
            return;
        }

        let expires_tick = self.tick + self.config.llm.conversation_timeout_ticks;
        let pos_a = self.db.creatures.get(&creature_id).unwrap().position.min;
        let pos_b = self.db.creatures.get(&target_id).unwrap().position.min;

        // Assign both to Conversing tasks.
        self.assign_conversing(creature_id, target_id, pos_a, expires_tick);
        self.assign_conversing(target_id, creature_id, pos_b, expires_tick);

        // Build and emit the LLM request.
        let (preambles, prompt, response_schema) =
            crate::prompt::build_social_chat_prompt(self, creature_id, target_id);

        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let deadline_tick = self.tick + self.config.llm.deadline_ticks;

        self.pending_llm_requests.insert(
            request_id,
            crate::llm::PendingLlmRequest {
                request_id,
                creature_id,
                request_kind: crate::llm::LlmRequestKind::SocialChat {
                    target_creature_id: target_id,
                },
                deadline_tick,
            },
        );

        self.outbound_requests
            .push(crate::llm::OutboundRequest::LlmInference {
                request_id,
                creature_id,
                preambles,
                prompt: prompt.clone(),
                response_schema,
                deadline_tick,
                max_tokens: self.config.llm.max_tokens,
            });

        eprintln!(
            "[SOCIAL CHAT] EMITTED request {request_id}: {creature_name} -> {target_name}, deadline tick {deadline_tick}, prompt len {}",
            prompt.len()
        );
    }

    /// Check if a creature is idle (no current task) or in an Autonomous-level
    /// task. Used for conversation eligibility — Conversing is Autonomous, so
    /// it can only preempt same-or-lower priority.
    fn is_idle_or_autonomous(&self, creature_id: CreatureId) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return false,
        };
        let task_id = match creature.current_task {
            None => return true, // Idle.
            Some(tid) => tid,
        };
        let task = match self.db.tasks.get(&task_id) {
            Some(t) => t,
            None => return true, // Task gone — effectively idle.
        };
        crate::preemption::preemption_level(task.kind_tag, task.origin)
            == crate::preemption::PreemptionLevel::Autonomous
    }

    /// Create a Conversing task for a creature and assign it as their current
    /// task. This is a cross-creature assignment (called from the initiator's
    /// heartbeat for both participants), following the `try_assign_to_activity()`
    /// pattern.
    fn assign_conversing(
        &mut self,
        creature_id: CreatureId,
        with: CreatureId,
        position: VoxelCoord,
        expires_tick: u64,
    ) {
        // If the creature already has a task, unassign it first.
        if self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
            .is_some()
        {
            self.unassign_creature_from_task(creature_id);
        }

        let task = crate::task::Task {
            id: TaskId::new(&mut self.rng),
            kind: crate::task::TaskKind::Conversing { with, expires_tick },
            state: crate::task::TaskState::InProgress,
            location: position,
            progress: 0,
            total_cost: 0,
            required_species: None,
            origin: crate::task::TaskOrigin::Autonomous,
            target_creature: None,
            restrict_to_creature_id: Some(creature_id),
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        let task_id = task.id;
        self.insert_task(self.home_zone_id(), task);
        self.claim_task(creature_id, task_id);
        self.set_creature_activation_tick(creature_id, self.tick + 1);
    }

    /// Check if a creature has unprocessed inbox messages and, if so, emit an
    /// LLM reply request. Returns `true` if a request was emitted.
    ///
    /// Called from the idle activation cascade and from heartbeat-triggered
    /// inbox processing. The creature must be idle or Autonomous-level and
    /// have no pending LLM request.
    pub(crate) fn try_process_inbox(&mut self, creature_id: CreatureId) -> bool {
        // Must be idle or Autonomous.
        if !self.is_idle_or_autonomous(creature_id) {
            return false;
        }

        // Must not already have a pending LLM request.
        if self
            .pending_llm_requests
            .values()
            .any(|r| r.creature_id == creature_id)
        {
            return false;
        }

        // Find oldest unprocessed inbox message and reply to that sender.
        // Only process messages from the first sender — messages from other
        // senders stay unprocessed and will trigger replies on subsequent
        // activations.
        let messages: Vec<_> = self
            .db
            .creature_messages
            .by_recipient_creature_id(&creature_id, crate::tabulosity::QueryOpts::ASC);
        let unprocessed: Vec<_> = messages.iter().filter(|m| !m.processed).collect();
        if unprocessed.is_empty() {
            return false;
        }

        // Find the first sender who is still alive — skip dead senders to
        // avoid wasted inference and orphaned messages.
        let sender_id = match unprocessed.iter().find(|m| {
            self.db
                .creatures
                .get(&m.sender_creature_id)
                .is_some_and(|c| c.vital_status == VitalStatus::Alive)
        }) {
            Some(m) => m.sender_creature_id,
            None => {
                // All senders are dead — mark everything processed and bail.
                for msg in &unprocessed {
                    let mut updated = (*msg).clone();
                    updated.processed = true;
                    let _ = self.db.update_creature_message(updated);
                }
                return false;
            }
        };
        let from_sender: Vec<_> = unprocessed
            .iter()
            .filter(|m| m.sender_creature_id == sender_id)
            .collect();

        // Mark only this sender's messages as processed.
        for msg in &from_sender {
            let mut updated = (**msg).clone();
            updated.processed = true;
            let _ = self.db.update_creature_message(updated);
        }

        // Enter Conversing with the sender.
        let pos = match self.db.creatures.get(&creature_id) {
            Some(c) => c.position.min,
            None => return false,
        };
        let expires_tick = self.tick + self.config.llm.conversation_timeout_ticks;
        self.assign_conversing(creature_id, sender_id, pos, expires_tick);

        // Build prompt that includes the inbox contents from this sender.
        let inbox_text: String = from_sender
            .iter()
            .map(|m| {
                let name = self
                    .db
                    .creatures
                    .get(&m.sender_creature_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "someone".into());
                format!("{name} said: \"{}\"", m.text)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let (preambles, mut prompt, response_schema) =
            crate::prompt::build_social_chat_prompt(self, creature_id, sender_id);

        // Append inbox contents to the ephemeral prompt.
        prompt.push_str(&format!(
            "\n\nYou received a message:\n{inbox_text}\n\nHow do you respond?"
        ));

        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let deadline_tick = self.tick + self.config.llm.deadline_ticks;

        self.pending_llm_requests.insert(
            request_id,
            crate::llm::PendingLlmRequest {
                request_id,
                creature_id,
                request_kind: crate::llm::LlmRequestKind::SocialChat {
                    target_creature_id: sender_id,
                },
                deadline_tick,
            },
        );

        self.outbound_requests
            .push(crate::llm::OutboundRequest::LlmInference {
                request_id,
                creature_id,
                preambles,
                prompt,
                response_schema,
                deadline_tick,
                max_tokens: self.config.llm.max_tokens,
            });

        true
    }

    /// Garbage-collect creature messages for a creature (F-llm-social-chat).
    /// Removes unprocessed messages older than `config.llm.message_ttl_ticks`
    /// and caps total message count at `config.llm.max_messages_per_creature`.
    pub(crate) fn gc_creature_messages(&mut self, creature_id: CreatureId) {
        let ttl = self.config.llm.message_ttl_ticks;
        let max_per_creature = self.config.llm.max_messages_per_creature as usize;

        // Collect all messages where this creature is recipient or sender.
        let received: Vec<_> = self
            .db
            .creature_messages
            .by_recipient_creature_id(&creature_id, crate::tabulosity::QueryOpts::ASC);
        let sent: Vec<_> = self
            .db
            .creature_messages
            .by_sender_creature_id(&creature_id, crate::tabulosity::QueryOpts::ASC);

        // Fast path: skip GC work if this creature has no messages at all.
        if received.is_empty() && sent.is_empty() {
            return;
        }

        // TTL: remove unprocessed messages older than TTL.
        let cutoff = self.tick.saturating_sub(ttl);
        for msg in received.iter().chain(sent.iter()) {
            if !msg.processed && msg.tick_created < cutoff {
                let _ = self.db.remove_creature_message(&msg.message_id);
            }
        }

        // Cap: re-query after TTL pass so the count reflects actual remaining
        // messages (not the stale pre-TTL snapshot).
        let received2: Vec<_> = self
            .db
            .creature_messages
            .by_recipient_creature_id(&creature_id, crate::tabulosity::QueryOpts::ASC);
        let sent2: Vec<_> = self
            .db
            .creature_messages
            .by_sender_creature_id(&creature_id, crate::tabulosity::QueryOpts::ASC);
        // Cap: only evict processed (history) messages — unprocessed inbox
        // messages must survive until the recipient reads them. Without this
        // guard, a creature with many conversations could lose other creatures'
        // unread messages via cap eviction.
        let mut evictable: Vec<(u64, u64)> = received2
            .iter()
            .chain(sent2.iter())
            .filter(|m| m.processed)
            .map(|m| (m.tick_created, m.message_id))
            .collect();
        // Dedup in case a message appears in both sent and received (same creature).
        evictable.sort();
        evictable.dedup();
        let total_count = {
            let mut all: Vec<u64> = received2
                .iter()
                .chain(sent2.iter())
                .map(|m| m.message_id)
                .collect();
            all.sort();
            all.dedup();
            all.len()
        };
        if total_count > max_per_creature {
            let to_remove = (total_count - max_per_creature).min(evictable.len());
            for (_, msg_id) in evictable.iter().take(to_remove) {
                let _ = self.db.remove_creature_message(msg_id);
            }
        }
    }

    /// Check whether a Conversing task should end (F-llm-social-chat).
    /// Returns `true` if the extension data is missing, the conversation has
    /// timed out, the partner creature is dead, or the partner is no longer in
    /// a Conversing task targeting us.
    pub(crate) fn should_end_conversation(&self, creature_id: CreatureId, task_id: TaskId) -> bool {
        let conv = match self.db.task_conversing_data.get(&task_id) {
            Some(c) => c,
            None => return true,
        };

        // Timed out?
        if conv.expires_tick <= self.tick {
            return true;
        }

        // Partner dead?
        let partner = match self.db.creatures.get(&conv.with) {
            Some(c) => c,
            None => return true,
        };
        if partner.vital_status != VitalStatus::Alive {
            return true;
        }

        // Partner still conversing with us? Collapse the nested checks per clippy.
        if let Some(partner_task_id) = partner.current_task
            && let Some(partner_task) = self.db.tasks.get(&partner_task_id)
            && partner_task.kind_tag == crate::db::TaskKindTag::Conversing
            && let Some(partner_conv) = self.db.task_conversing_data.get(&partner_task_id)
            && partner_conv.with == creature_id
        {
            return false; // Partner is still conversing with us.
        }

        // Partner is not in a Conversing task with us — end.
        true
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
