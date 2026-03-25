// Group activity lifecycle — multi-creature coordination layer.
//
// Activities are a coordination layer above the task system for actions that
// require multiple participants (dances, construction choirs, rituals).
// Activities own tasks (GoTo for assembly) rather than replacing them.
//
// ## Lifecycle
//
// `Recruiting` → `Assembling` → `Executing` → `Complete`
//                                    ↓
//                                 `Paused` (PauseAndWait departure policy)
//
// Any phase can transition to `Cancelled`.
//
// ## Volunteering (Open recruitment)
//
// Idle creatures discover Recruiting/Open activities during their activation
// loop and create `Volunteered` participant rows. The creature's
// `current_activity` is NOT set — they remain free. When enough volunteers
// exist and a quorum check passes (pruning any who became busy), the activity
// transitions to `Assembling` and volunteers are promoted to `Traveling`
// (GoTo tasks created, `current_activity` set).
//
// ## Directed recruitment
//
// `AssignToActivity` commands create participants with `Traveling` status
// directly (skipping `Volunteered`), setting `current_activity` and creating
// GoTo tasks immediately.
//
// See also: `activation.rs` for the activation loop integration,
// `preemption.rs` for activity preemption levels, `db.rs` for the `Activity`
// and `ActivityParticipant` tables, `types.rs` for enums.

use crate::event::SimEvent;
use crate::types::{
    ActivityId, ActivityKind, ActivityPhase, CreatureId, DeparturePolicy, ParticipantRole,
    ParticipantStatus, RecruitmentMode, Species, VitalStatus, VoxelCoord,
};

use super::SimState;

impl SimState {
    /// Handle `SimAction::CreateActivity` — create a new group activity.
    pub(crate) fn handle_create_activity(
        &mut self,
        kind: ActivityKind,
        location: VoxelCoord,
        min_count: Option<u16>,
        desired_count: Option<u16>,
        origin: crate::task::TaskOrigin,
        _events: &mut Vec<SimEvent>,
    ) {
        let activity_id = ActivityId::new(&mut self.rng);
        let departure_policy = default_departure_policy(kind, &self.config.activity);
        let allows_late_join = default_allows_late_join(kind);
        let recruitment = default_recruitment_mode(kind);
        let total_cost = match kind {
            ActivityKind::Dance => self.config.activity.debug_dance_total_cost,
            _ => 0, // Other kinds will set their own cost when implemented.
        };

        // Per-kind eligibility defaults. Dance is restricted to the player's
        // elf civ; other kinds will get their own rules when implemented.
        let (civ_id, required_species) = match kind {
            ActivityKind::Dance => (self.player_civ_id, Some(Species::Elf)),
            _ => (None, None),
        };

        let activity = crate::db::Activity {
            id: activity_id,
            kind,
            phase: ActivityPhase::Recruiting,
            location,
            min_count,
            desired_count,
            progress: 0,
            total_cost,
            origin,
            recruitment,
            departure_policy,
            allows_late_join,
            civ_id,
            required_species,
            execution_start_tick: None,
            pause_started_tick: None,
        };
        self.db.activities.insert_no_fk(activity).unwrap();
    }

    /// Handle `SimAction::CancelActivity` — cancel and clean up an activity.
    pub(crate) fn handle_cancel_activity(
        &mut self,
        activity_id: ActivityId,
        events: &mut Vec<SimEvent>,
    ) {
        self.cancel_activity(activity_id, events);
    }

    /// Handle `SimAction::AssignToActivity` — directed recruitment.
    pub(crate) fn handle_assign_to_activity(
        &mut self,
        activity_id: ActivityId,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };
        // Must be in a phase that accepts new participants.
        if !matches!(
            activity.phase,
            ActivityPhase::Recruiting | ActivityPhase::Assembling
        ) {
            return;
        }
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c.clone(),
            None => return,
        };
        // Must be alive and not already committed to an activity.
        if creature.current_activity.is_some() {
            return;
        }
        if creature.vital_status != VitalStatus::Alive {
            return;
        }
        // Check civ and species eligibility.
        if !self.creature_eligible_for_activity(&activity, creature_id) {
            return;
        }
        // Also reject if the creature is a tentative volunteer for any activity
        // (current_activity is None for volunteers, but participant rows exist).
        let existing_participations = self
            .db
            .activity_participants
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        if !existing_participations.is_empty() {
            return;
        }

        // Create participant with Traveling status (skips Volunteered).
        let participant = crate::db::ActivityParticipant {
            activity_id,
            creature_id,
            role: ParticipantRole::Member,
            status: ParticipantStatus::Traveling,
            assigned_position: activity.location,
            travel_task: None,
        };
        self.db
            .activity_participants
            .insert_no_fk(participant)
            .unwrap();

        // Set current_activity on creature.
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.current_activity = Some(activity_id);
            let _ = self.db.creatures.update_no_fk(c);
        }

        // Create GoTo task for this participant, inheriting the activity's origin.
        self.create_activity_goto(
            activity_id,
            creature_id,
            activity.location,
            activity.origin,
            events,
        );

        // If this is Directed recruitment in Recruiting phase, check if we
        // have enough participants to transition to Assembling.
        if activity.phase == ActivityPhase::Recruiting {
            let count = self
                .db
                .activity_participants
                .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC)
                .len() as u16;
            if activity.min_count.is_none() || count >= activity.min_count.unwrap_or(0) {
                self.set_activity_phase(activity_id, ActivityPhase::Assembling);
            }
        }
    }

    /// Handle `SimAction::RemoveFromActivity` — participant departure.
    pub(crate) fn handle_remove_from_activity(
        &mut self,
        activity_id: ActivityId,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) {
        self.remove_participant(activity_id, creature_id, events);
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// An idle creature volunteers for an Open-recruitment activity.
    /// Creates a `Volunteered` participant row. Does NOT set `current_activity`.
    pub(crate) fn volunteer_for_activity(
        &mut self,
        activity_id: ActivityId,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) {
        // Early guard: creature must be alive and not committed to an activity.
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        if creature.current_activity.is_some() {
            return;
        }
        if creature.vital_status != VitalStatus::Alive {
            return;
        }
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };
        if activity.phase != ActivityPhase::Recruiting {
            return;
        }
        if activity.recruitment != RecruitmentMode::Open {
            return;
        }

        // Check civ and species eligibility.
        if !self.creature_eligible_for_activity(&activity, creature_id) {
            return;
        }

        // Don't volunteer if already committed (Traveling/Arrived) to any
        // activity, or already volunteered for this same activity.
        // If volunteered for a DIFFERENT activity, prune that stale row first
        // (the creature is switching allegiance to the closer/better activity).
        let existing = self
            .db
            .activity_participants
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        for p in &existing {
            if p.activity_id == activity_id {
                return; // Already volunteered/committed for this activity.
            }
            if p.status != ParticipantStatus::Volunteered {
                return; // Committed to another activity.
            }
        }
        // Prune stale Volunteered rows for other activities before volunteering.
        self.prune_stale_volunteer_rows(creature_id);

        let participant = crate::db::ActivityParticipant {
            activity_id,
            creature_id,
            role: ParticipantRole::Member,
            status: ParticipantStatus::Volunteered,
            assigned_position: activity.location,
            travel_task: None,
        };
        self.db
            .activity_participants
            .insert_no_fk(participant)
            .unwrap();

        // Check quorum — maybe we have enough volunteers to start assembling.
        self.check_volunteer_quorum(activity_id, events);
    }

    /// Check if an Open-recruitment activity has enough valid volunteers to
    /// transition from Recruiting to Assembling. Prunes unavailable volunteers
    /// first (those who picked up a task, died, or joined another activity).
    pub(crate) fn check_volunteer_quorum(
        &mut self,
        activity_id: ActivityId,
        events: &mut Vec<SimEvent>,
    ) {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };
        if activity.phase != ActivityPhase::Recruiting {
            return;
        }

        // Prune unavailable volunteers.
        let participants = self
            .db
            .activity_participants
            .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
        let mut to_remove = Vec::new();
        for p in &participants {
            if p.status != ParticipantStatus::Volunteered {
                continue; // Only prune volunteers, not committed participants.
            }
            let available = match self.db.creatures.get(&p.creature_id) {
                Some(c) => {
                    c.vital_status == VitalStatus::Alive
                        && c.current_task.is_none()
                        && c.current_activity.is_none()
                }
                None => false,
            };
            if !available {
                to_remove.push(p.creature_id);
            }
        }
        for cid in &to_remove {
            let _ = self.db.remove_activity_participant(&(activity_id, *cid));
        }

        // Count remaining volunteers.
        let remaining = self
            .db
            .activity_participants
            .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
        let volunteer_count = remaining.len() as u16;
        let min = activity.min_count.unwrap_or(1);

        if volunteer_count >= min {
            // Promote all volunteers to Traveling, create GoTo tasks.
            self.set_activity_phase(activity_id, ActivityPhase::Assembling);
            let to_promote: Vec<CreatureId> = remaining
                .iter()
                .filter(|p| p.status == ParticipantStatus::Volunteered)
                .map(|p| p.creature_id)
                .collect();
            for cid in to_promote {
                let _ = self
                    .db
                    .activity_participants
                    .modify_unchecked(&(activity_id, cid), |p| {
                        p.status = ParticipantStatus::Traveling;
                    });
                if let Some(mut c) = self.db.creatures.get(&cid) {
                    c.current_activity = Some(activity_id);
                    let _ = self.db.creatures.update_no_fk(c);
                }
                self.create_activity_goto(
                    activity_id,
                    cid,
                    activity.location,
                    activity.origin,
                    events,
                );
            }
        }
    }

    /// Called when a participant's GoTo task completes. Marks the participant
    /// as Arrived and checks if the activity can start executing.
    pub(crate) fn on_activity_participant_arrived(
        &mut self,
        activity_id: ActivityId,
        creature_id: CreatureId,
        _events: &mut Vec<SimEvent>,
    ) {
        let _ = self
            .db
            .activity_participants
            .modify_unchecked(&(activity_id, creature_id), |p| {
                p.status = ParticipantStatus::Arrived;
                p.travel_task = None;
            });

        // Clear current_task now that GoTo is done.
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.current_task = None;
            let _ = self.db.creatures.update_no_fk(c);
        }

        // Check if enough participants have arrived to start executing.
        self.check_assembly_complete(activity_id);
    }

    /// Check if enough participants have arrived to transition from
    /// Assembling to Executing.
    fn check_assembly_complete(&mut self, activity_id: ActivityId) {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };
        if activity.phase != ActivityPhase::Assembling {
            return;
        }

        let participants = self
            .db
            .activity_participants
            .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
        let arrived_count = participants
            .iter()
            .filter(|p| p.status == ParticipantStatus::Arrived)
            .count() as u16;
        let min = activity.min_count.unwrap_or(1);

        if arrived_count >= min
            && let Some(mut a) = self.db.activities.get(&activity_id)
        {
            a.phase = ActivityPhase::Executing;
            a.execution_start_tick = Some(self.tick);
            let _ = self.db.activities.update_no_fk(a);
        }
    }

    /// Execute one activation tick of activity behavior for a creature.
    /// Called from the activation loop when a creature has `current_activity`
    /// set and the activity is in `Executing` phase.
    pub(crate) fn execute_activity_behavior(
        &mut self,
        creature_id: CreatureId,
        activity_id: ActivityId,
        events: &mut Vec<SimEvent>,
    ) {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };

        if activity.kind == ActivityKind::Dance {
            self.execute_dance_behavior(creature_id, activity_id, &activity, events);
        }
        // Other activity kinds will be implemented by their respective features.
    }

    /// Dance-specific execution behavior. Each activation contributes 1 unit
    /// of progress and adds an EnjoyingDance thought. When progress reaches
    /// total_cost, the dance completes.
    /// TEMPORARY: Hard-coded for debug dance proof-of-concept.
    fn execute_dance_behavior(
        &mut self,
        creature_id: CreatureId,
        activity_id: ActivityId,
        activity: &crate::db::Activity,
        events: &mut Vec<SimEvent>,
    ) {
        // Contribute progress.
        let _ = self.db.activities.modify_unchecked(&activity_id, |a| {
            a.progress += 1;
        });

        // Add small mood boost (dedup prevents spam).
        self.add_creature_thought(creature_id, crate::types::ThoughtKind::EnjoyingDance);

        // Check completion.
        let updated = self.db.activities.get(&activity_id).unwrap().clone();
        if updated.progress >= activity.total_cost {
            self.complete_activity(activity_id, events);
        }
    }

    /// Complete an activity successfully. Awards completion thoughts, releases
    /// participants, and cleans up.
    fn complete_activity(&mut self, activity_id: ActivityId, _events: &mut Vec<SimEvent>) {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };

        // Award completion thoughts to all participants.
        let participants = self
            .db
            .activity_participants
            .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
        let participant_ids: Vec<CreatureId> = participants.iter().map(|p| p.creature_id).collect();

        if activity.kind == ActivityKind::Dance {
            for cid in &participant_ids {
                self.add_creature_thought(*cid, crate::types::ThoughtKind::DancedInGroup);
            }
        }

        // Release all participants and schedule reactivation so they resume
        // normal behavior (find tasks, wander, etc.). Cancel existing
        // activations first to prevent double-activation (B-erratic-movement).
        for cid in &participant_ids {
            if let Some(mut c) = self.db.creatures.get(cid) {
                c.current_activity = None;
                let _ = self.db.creatures.update_no_fk(c);
            }
            self.event_queue.cancel_creature_activations(*cid);
            self.schedule_reactivation(*cid);
        }

        // Delete activity (cascade removes participants).
        let _ = self.db.remove_activity(&activity_id);
    }

    /// Cancel an activity. Cancels in-flight GoTo tasks, releases participants,
    /// and deletes the activity.
    pub(crate) fn cancel_activity(&mut self, activity_id: ActivityId, _events: &mut Vec<SimEvent>) {
        let participants = self
            .db
            .activity_participants
            .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);

        let creature_ids: Vec<CreatureId> = participants.iter().map(|p| p.creature_id).collect();

        for p in &participants {
            // Cancel GoTo tasks for Traveling participants.
            if let Some(task_id) = p.travel_task {
                self.complete_task(task_id);
            }
            // Clear current_activity for committed participants.
            if matches!(
                p.status,
                ParticipantStatus::Traveling | ParticipantStatus::Arrived
            ) && let Some(mut c) = self.db.creatures.get(&p.creature_id)
            {
                c.current_activity = None;
                if c.current_task == p.travel_task {
                    c.current_task = None;
                }
                let _ = self.db.creatures.update_no_fk(c);
            }
        }

        // Delete activity (cascade removes participants).
        let _ = self.db.remove_activity(&activity_id);

        // Schedule reactivation for all released creatures. Cancel existing
        // activations first to prevent double-activation (B-erratic-movement).
        for cid in &creature_ids {
            self.event_queue.cancel_creature_activations(*cid);
            self.schedule_reactivation(*cid);
        }
    }

    /// Remove a single participant from an activity and apply the departure policy.
    pub(crate) fn remove_participant(
        &mut self,
        activity_id: ActivityId,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };
        let participant = match self
            .db
            .activity_participants
            .get(&(activity_id, creature_id))
        {
            Some(p) => p.clone(),
            None => return,
        };

        // Cancel GoTo if traveling.
        if let Some(task_id) = participant.travel_task {
            self.complete_task(task_id);
        }

        // Clear creature state if committed.
        if matches!(
            participant.status,
            ParticipantStatus::Traveling | ParticipantStatus::Arrived
        ) && let Some(mut c) = self.db.creatures.get(&creature_id)
        {
            c.current_activity = None;
            c.current_task = None;
            let _ = self.db.creatures.update_no_fk(c);
        }

        // Remove participant row.
        let _ = self
            .db
            .remove_activity_participant(&(activity_id, creature_id));

        // Check if anyone remains. If not, cancel regardless of phase.
        let remaining = self
            .db
            .activity_participants
            .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
        if remaining.is_empty()
            && matches!(
                activity.phase,
                ActivityPhase::Assembling | ActivityPhase::Executing | ActivityPhase::Paused
            )
        {
            self.cancel_activity(activity_id, events);
            return;
        }

        // During Assembling: if participant count drops below min_count,
        // revert to Recruiting so new volunteers can fill the gap.
        if activity.phase == ActivityPhase::Assembling {
            let committed_count = remaining.len() as u16;
            let min = activity.min_count.unwrap_or(1);
            if committed_count < min {
                self.set_activity_phase(activity_id, ActivityPhase::Recruiting);
            }
        }

        // Apply departure policy (only relevant during Executing phase).
        if activity.phase == ActivityPhase::Executing {
            match activity.departure_policy {
                DeparturePolicy::Continue => {
                    // Keep going with reduced participants. Nothing to do.
                }
                DeparturePolicy::PauseAndWait { .. } => {
                    if let Some(mut a) = self.db.activities.get(&activity_id) {
                        a.phase = ActivityPhase::Paused;
                        a.pause_started_tick = Some(self.tick);
                        let _ = self.db.activities.update_no_fk(a);
                    }
                }
                DeparturePolicy::CancelOnDeparture => {
                    self.cancel_activity(activity_id, events);
                }
            }
        }
    }

    /// Create a GoTo task for a participant walking to an activity's location.
    fn create_activity_goto(
        &mut self,
        activity_id: ActivityId,
        creature_id: CreatureId,
        location: VoxelCoord,
        origin: crate::task::TaskOrigin,
        _events: &mut Vec<SimEvent>,
    ) {
        use crate::db::TaskKindTag;
        use crate::task::TaskState;
        use crate::types::TaskId;

        let task_id = TaskId::new(&mut self.rng);
        let task = crate::db::Task {
            id: task_id,
            kind_tag: TaskKindTag::GoTo,
            state: TaskState::InProgress,
            origin,
            location,
            progress: 0,
            total_cost: 0,
            required_species: None,
            target_creature: None,
            restrict_to_creature_id: Some(creature_id),
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        self.db.tasks.insert_no_fk(task).unwrap();

        // Assign the task to the creature.
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.current_task = Some(task_id);
            let _ = self.db.creatures.update_no_fk(c);
        }

        // Record the travel task on the participant.
        let _ = self
            .db
            .activity_participants
            .modify_unchecked(&(activity_id, creature_id), |p| {
                p.travel_task = Some(task_id);
            });
    }

    /// Prune any stale `Volunteered` participant rows for an idle creature.
    /// Called before activity discovery so that creatures who volunteered but
    /// then picked up a task (eating, sleeping, moping) can re-volunteer once
    /// they become idle again.
    pub(crate) fn prune_stale_volunteer_rows(&mut self, creature_id: CreatureId) {
        let participations = self
            .db
            .activity_participants
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        for p in &participations {
            if p.status == ParticipantStatus::Volunteered {
                let _ = self
                    .db
                    .remove_activity_participant(&(p.activity_id, creature_id));
            }
        }
    }

    /// Find Open-recruitment activities that an idle creature could volunteer for.
    /// Returns the best match (closest within search radius), or None.
    /// Caller should call `prune_stale_volunteer_rows` first.
    pub(crate) fn find_open_activity_for_creature(
        &self,
        creature_id: CreatureId,
    ) -> Option<ActivityId> {
        let creature = self.db.creatures.get(&creature_id)?;
        if creature.current_activity.is_some() || creature.current_task.is_some() {
            return None;
        }
        if creature.vital_status != VitalStatus::Alive {
            return None;
        }
        // Don't discover activities if already committed (Traveling/Arrived)
        // to another activity.
        let existing = self
            .db
            .activity_participants
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        if existing
            .iter()
            .any(|p| p.status != ParticipantStatus::Volunteered)
        {
            return None;
        }

        let search_radius = self.config.activity.volunteer_search_radius.max(0) as u32;
        let pos = creature.position;

        let mut best: Option<(ActivityId, u32)> = None;

        for activity in self.db.activities.iter_all() {
            if activity.phase != ActivityPhase::Recruiting {
                continue;
            }
            if activity.recruitment != RecruitmentMode::Open {
                continue;
            }

            // Check civ and species eligibility.
            if !self.creature_eligible_for_activity(activity, creature_id) {
                continue;
            }

            // Check if already volunteered.
            if self
                .db
                .activity_participants
                .get(&(activity.id, creature_id))
                .is_some()
            {
                continue;
            }

            // Check if activity already has enough volunteers.
            let desired = activity
                .desired_count
                .or(activity.min_count)
                .unwrap_or(u16::MAX);
            let current = self
                .db
                .activity_participants
                .by_activity_id(&activity.id, tabulosity::QueryOpts::ASC)
                .len() as u16;
            if current >= desired {
                continue;
            }

            let dist = pos.manhattan_distance(activity.location);
            if dist > search_radius {
                continue;
            }

            if best.is_none() || dist < best.unwrap().1 {
                best = Some((activity.id, dist));
            }
        }

        best.map(|(id, _)| id)
    }

    /// Re-create a GoTo task for a Traveling participant whose GoTo was
    /// preempted (e.g., by moping or eating). Called during activation when
    /// the creature has `current_activity` set and the activity is in
    /// Assembling phase but the creature has no `current_task`.
    pub(crate) fn reissue_activity_goto_if_needed(
        &mut self,
        activity_id: ActivityId,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) {
        let participant = match self
            .db
            .activity_participants
            .get(&(activity_id, creature_id))
        {
            Some(p) => p.clone(),
            None => return,
        };
        // Only re-issue for Traveling participants who lost their GoTo task.
        if participant.status != ParticipantStatus::Traveling {
            return;
        }
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        if creature.current_task.is_some() {
            return; // Already has a task (shouldn't happen, but guard).
        }
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a,
            None => return,
        };
        self.create_activity_goto(
            activity_id,
            creature_id,
            activity.location,
            activity.origin,
            events,
        );
    }

    /// Check if a Paused activity's timeout has expired.
    pub(crate) fn check_activity_pause_timeout(
        &mut self,
        activity_id: ActivityId,
        events: &mut Vec<SimEvent>,
    ) {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };
        if activity.phase != ActivityPhase::Paused {
            return;
        }
        if let DeparturePolicy::PauseAndWait { timeout_ticks } = activity.departure_policy
            && let Some(pause_tick) = activity.pause_started_tick
            && self.tick.saturating_sub(pause_tick) >= timeout_ticks
        {
            self.cancel_activity(activity_id, events);
        }
    }

    /// Resume a Paused activity back to Executing (e.g., when a replacement arrives).
    #[allow(dead_code)] // Will be called when PauseAndWait replacement logic is wired.
    pub(crate) fn resume_activity(&mut self, activity_id: ActivityId) {
        if let Some(mut a) = self.db.activities.get(&activity_id)
            && a.phase == ActivityPhase::Paused
        {
            a.phase = ActivityPhase::Executing;
            a.pause_started_tick = None;
            let _ = self.db.activities.update_no_fk(a);
        }
    }

    /// Helper: update the activity phase. Uses `update_no_fk` because `phase`
    /// is an indexed field and `modify_unchecked` panics when indexed fields change.
    fn set_activity_phase(&mut self, activity_id: ActivityId, phase: ActivityPhase) {
        if let Some(mut a) = self.db.activities.get(&activity_id) {
            a.phase = phase;
            let _ = self.db.activities.update_no_fk(a);
        }
    }

    /// Check whether a creature is eligible for an activity based on its
    /// `civ_id` and `required_species` restrictions. Returns `true` if the
    /// creature passes all eligibility checks.
    fn creature_eligible_for_activity(
        &self,
        activity: &crate::db::Activity,
        creature_id: CreatureId,
    ) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return false,
        };
        // Species check.
        if let Some(required) = activity.required_species
            && creature.species != required
        {
            return false;
        }
        // Civ check.
        if let Some(activity_civ) = activity.civ_id
            && creature.civ_id != Some(activity_civ)
        {
            return false;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Per-kind defaults
// ---------------------------------------------------------------------------

fn default_departure_policy(
    kind: ActivityKind,
    config: &crate::config::ActivityConfig,
) -> DeparturePolicy {
    match kind {
        ActivityKind::Dance => DeparturePolicy::Continue,
        ActivityKind::ConstructionChoir => DeparturePolicy::PauseAndWait {
            timeout_ticks: config.pause_timeout_ticks,
        },
        ActivityKind::Ceremony => DeparturePolicy::CancelOnDeparture,
        ActivityKind::CombatSinging | ActivityKind::GroupHaul => DeparturePolicy::Continue,
    }
}

fn default_allows_late_join(kind: ActivityKind) -> bool {
    match kind {
        ActivityKind::Dance => true,
        ActivityKind::ConstructionChoir => false,
        ActivityKind::Ceremony => false,
        ActivityKind::CombatSinging => true,
        ActivityKind::GroupHaul => false,
    }
}

fn default_recruitment_mode(kind: ActivityKind) -> RecruitmentMode {
    match kind {
        ActivityKind::Dance => RecruitmentMode::Open,
        ActivityKind::ConstructionChoir => RecruitmentMode::Directed,
        ActivityKind::Ceremony => RecruitmentMode::Directed,
        ActivityKind::CombatSinging => RecruitmentMode::Directed,
        ActivityKind::GroupHaul => RecruitmentMode::Directed,
    }
}
