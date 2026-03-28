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
// ## Spontaneous organization (F-dance-self-org)
//
// Idle elves near furnished dance halls can autonomously organize dances via
// `try_organize_spontaneous_dance()`, called from the activation loop. Gated
// by per-hall cooldown, per-elf cooldown, venue exclusivity (no two dances
// on the same hall), and a probability roll. Newly-furnished halls bypass
// the hall cooldown (first-dance nudge). The organizer creates an Autonomous
// Open-recruitment dance and becomes the first participant with Organizer role.
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
    ) -> ActivityId {
        let activity_id = ActivityId::new(&mut self.rng);
        let departure_policy = default_departure_policy(kind, &self.config.activity);
        let allows_late_join = default_allows_late_join(kind);
        let recruitment = default_recruitment_mode(kind);
        // Dance total_cost is set later by generate_dance_plan (plan.total_ticks).
        let total_cost = 0;

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
            assembly_started_tick: None,
        };
        self.db.insert_activity(activity).unwrap();
        activity_id
    }

    /// Handle `SimAction::StartDebugDance` — find a dance hall and create a
    /// Dance activity linked to it.
    pub(crate) fn handle_start_debug_dance(&mut self, events: &mut Vec<SimEvent>) {
        // Find the first dance-hall-furnished building without an active dance.
        let dance_hall = self.db.structures.iter_all().find(|s| {
            s.furnishing == Some(crate::types::FurnishingType::DanceHall)
                && !self.hall_has_active_dance(s.id)
        });
        let structure = match dance_hall {
            Some(s) => s.clone(),
            None => return, // No available dance hall — do nothing.
        };

        // Use the first interior floor position as the activity location.
        let interior = structure.floor_interior_positions();
        let location = interior.first().copied().unwrap_or(structure.anchor);

        // Create the activity.
        let activity_id = self.handle_create_activity(
            ActivityKind::Dance,
            location,
            Some(3),
            Some(6),
            crate::task::TaskOrigin::PlayerDirected,
            events,
        );

        // Link the activity to the dance hall via ActivityStructureRef.
        let seq = self.db.activity_structure_refs.next_seq();
        let _ = self
            .db
            .insert_activity_structure_ref(crate::db::ActivityStructureRef {
                activity_id,
                seq,
                structure_id: structure.id,
                role: crate::db::ActivityStructureRole::DanceVenue,
            });
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
            dance_slot: None,
            waypoint_cursor: 0,
        };
        self.db.insert_activity_participant(participant).unwrap();

        // Set current_activity on creature.
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.current_activity = Some(activity_id);
            let _ = self.db.update_creature(c);
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
            dance_slot: None,
            waypoint_cursor: 0,
        };
        self.db.insert_activity_participant(participant).unwrap();

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
                // Preserve living organizers who are temporarily busy (have a
                // task like eating/moping). Dead organizers should be pruned
                // so they don't count toward quorum.
                if p.role == ParticipantRole::Organizer
                    && self
                        .db
                        .creatures
                        .get(&p.creature_id)
                        .is_some_and(|c| c.vital_status == VitalStatus::Alive)
                {
                    continue;
                }
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
                if let Some(mut p) = self.db.activity_participants.get(&(activity_id, cid)) {
                    p.status = ParticipantStatus::Traveling;
                    let _ = self.db.update_activity_participant(p);
                }
                if let Some(mut c) = self.db.creatures.get(&cid) {
                    c.current_activity = Some(activity_id);
                    let _ = self.db.update_creature(c);
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
        if let Some(mut p) = self
            .db
            .activity_participants
            .get(&(activity_id, creature_id))
        {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            let _ = self.db.update_activity_participant(p);
        }

        // Clear current_task now that GoTo is done.
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.current_task = None;
            let _ = self.db.update_creature(c);
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

        if arrived_count >= min {
            self.begin_execution(activity_id);
        }
    }

    /// Execute one activation tick of activity behavior for a creature.
    /// Called from the activation loop when a creature has `current_activity`
    /// set and the activity is in `Executing` phase.
    /// Returns an optional reactivation tick. If `Some(tick)`, the creature
    /// should not be reactivated until that tick (e.g., waiting for a dance
    /// move to finish). If `None`, reactivate next tick as usual.
    pub(crate) fn execute_activity_behavior(
        &mut self,
        creature_id: CreatureId,
        activity_id: ActivityId,
        events: &mut Vec<SimEvent>,
    ) -> Option<u64> {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return None,
        };

        if activity.kind == ActivityKind::Dance {
            return self.execute_dance_behavior(creature_id, activity_id, &activity, events);
        }
        // Other activity kinds will be implemented by their respective features.
        None
    }

    /// Generate a dance plan when a dance activity enters Executing phase.
    ///
    /// Reads the dance hall's dimensions from the linked `ActivityStructureRef`,
    /// assigns dance slots to arrived participants, generates the choreography
    /// plan, and stores it in `ActivityDanceData`.
    fn generate_dance_plan(&mut self, activity_id: ActivityId) {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };
        let execution_start_tick = match activity.execution_start_tick {
            Some(t) => t,
            None => return,
        };

        // Find the dance hall via ActivityStructureRef.
        let structure_refs = self
            .db
            .activity_structure_refs
            .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
        let venue_ref = structure_refs
            .iter()
            .find(|r| r.role == crate::db::ActivityStructureRole::DanceVenue);

        // Get floor dimensions from the dance hall structure.
        let (anchor_x, anchor_z, floor_y, width, depth) = if let Some(sr) = venue_ref {
            if let Some(structure) = self.db.structures.get(&sr.structure_id) {
                (
                    structure.anchor.x,
                    structure.anchor.z,
                    structure.anchor.y,
                    structure.width,
                    structure.depth,
                )
            } else {
                // Structure deleted — fall back to activity location.
                (
                    activity.location.x - 1,
                    activity.location.z - 1,
                    activity.location.y,
                    3,
                    3,
                )
            }
        } else {
            // No structure ref (e.g., old debug dance) — use a small default floor
            // centered on the activity location.
            (
                activity.location.x - 1,
                activity.location.z - 1,
                activity.location.y,
                3,
                3,
            )
        };

        // Assign dance slots to arrived participants.
        let participants = self
            .db
            .activity_participants
            .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
        let arrived: Vec<_> = participants
            .iter()
            .filter(|p| p.status == ParticipantStatus::Arrived)
            .collect();
        let participant_count = arrived.len();

        for (slot, p) in arrived.iter().enumerate() {
            if let Some(mut ap) = self
                .db
                .activity_participants
                .get(&(p.activity_id, p.creature_id))
            {
                ap.dance_slot = Some(slot as u16);
                ap.waypoint_cursor = 0;
                let _ = self.db.update_activity_participant(ap);
            }
        }

        // Create a music composition for the dance.
        //
        // The dance duration determines the composition's target_duration_ms,
        // which the rendering layer uses to adjust the generated music's BPM
        // to match. We pick a target dance duration (in seconds), derive the
        // beat count from it using the same TYPICAL_BEATS table as construction,
        // and create both the composition and the dance plan.
        let dance_duration_secs = self.config.activity.dance_duration_secs;
        let ticks_per_second = 1000 / self.config.tick_duration_ms as u64;
        let target_duration_ms = (dance_duration_secs * 1000.0) as u32;

        // Pick section count so ideal BPM is near 78 (middle of Palestrina range).
        const TYPICAL_BEATS: &[(u8, f32)] = &[(1, 55.0), (2, 125.0), (3, 195.0), (4, 270.0)];
        let mut best_sections = 1u8;
        let mut best_dist = f32::MAX;
        for &(s, beats) in TYPICAL_BEATS {
            let ideal_bpm = beats * 30.0 / dance_duration_secs;
            let dist = (ideal_bpm - 78.0).abs();
            if dist < best_dist {
                best_dist = dist;
                best_sections = s;
            }
        }

        // Compute estimated beat count from section count.
        let estimated_beats = match best_sections {
            1 => 55_u64,
            2 => 125,
            3 => 195,
            _ => 270,
        };

        let composition_id = self.create_composition_for_dance(best_sections, target_duration_ms);
        // Mark build_started immediately — dance music plays as soon as
        // the rendering layer finishes generating it.
        if let Some(mut c) = self.db.music_compositions.get(&composition_id) {
            c.build_started = true;
            let _ = self.db.update_music_composition(c);
        }

        let tempo_multiplier = 1_u64;
        let tempo_bpm = 78_u64; // Ideal BPM; actual playback BPM is adjusted by renderer.

        let plan = crate::dance::generate_dance_plan(
            &crate::dance::DancePlanParams {
                anchor_x,
                anchor_z,
                floor_y,
                width,
                depth,
                participant_count,
                song_length_beats: estimated_beats,
                tempo_multiplier,
                execution_start_tick,
                ticks_per_second,
                tempo_bpm,
            },
            &mut self.rng,
        );

        // Store total_ticks as total_cost on the activity for progress display.
        let total_ticks = plan.total_ticks as i64;
        if let Some(mut a) = self.db.activities.get(&activity_id) {
            a.total_cost = total_ticks;
            let _ = self.db.update_activity(a);
        }

        // Store the plan and composition link.
        let dance_data = crate::db::ActivityDanceData {
            activity_id,
            plan,
            composition_id: Some(composition_id),
        };
        let _ = self.db.insert_activity_dance_data(dance_data);
    }

    /// Dance-specific execution behavior.
    ///
    /// Each creature operates on its own activation schedule: when activated,
    /// it looks up the next waypoint in the dance plan and initiates a Move
    /// action timed to arrive on the beat. Returns `Some(tick)` to schedule
    /// reactivation at that tick (when the move completes), or `None` to
    /// reactivate next tick.
    fn execute_dance_behavior(
        &mut self,
        creature_id: CreatureId,
        activity_id: ActivityId,
        activity: &crate::db::Activity,
        events: &mut Vec<SimEvent>,
    ) -> Option<u64> {
        let execution_start = activity.execution_start_tick?;

        // Look up this creature's dance slot and cursor.
        let participant = match self
            .db
            .activity_participants
            .get(&(activity_id, creature_id))
        {
            Some(p) => p.clone(),
            None => return None,
        };
        let slot = match participant.dance_slot {
            Some(s) => s as usize,
            None => {
                // Late joiner without a choreographed slot — stand in place
                // and sleep until the dance ends to avoid per-tick reactivation.
                self.add_creature_thought(creature_id, crate::types::ThoughtKind::EnjoyingDance);
                let end_tick = self
                    .db
                    .activity_dance_data
                    .get(&activity_id)
                    .map(|d| execution_start + d.plan.total_ticks)
                    .unwrap_or(self.tick + 1);
                return Some(end_tick);
            }
        };

        // Look up the dance plan.
        let dance_data = match self.db.activity_dance_data.get(&activity_id) {
            Some(d) => d.clone(),
            None => return None,
        };
        let plan = &dance_data.plan;

        // Add mood boost.
        self.add_creature_thought(creature_id, crate::types::ThoughtKind::EnjoyingDance);

        // Check completion: elapsed ticks >= total_ticks.
        let elapsed = self.tick.saturating_sub(execution_start);
        if elapsed >= plan.total_ticks {
            self.complete_activity(activity_id, events);
            return None; // complete_activity schedules reactivation.
        }

        // Find the next waypoint for this creature's slot.
        if slot >= plan.slot_waypoints.len() {
            return None;
        }
        let waypoints = &plan.slot_waypoints[slot];
        let cursor = participant.waypoint_cursor as usize;

        // Skip any waypoints that are already past (catch-up after lag).
        let mut next_cursor = cursor;
        while next_cursor < waypoints.len() && waypoints[next_cursor].tick <= self.tick {
            next_cursor += 1;
        }

        if next_cursor < waypoints.len() {
            // There's a future waypoint — set up a Move action to arrive
            // at it on its tick (the beat).
            let target_wp = &waypoints[next_cursor];
            let old_pos = self.db.creatures.get(&creature_id).unwrap().position;
            let new_pos = target_wp.position;
            let arrival_tick = target_wp.tick;

            let tick = self.tick;
            if let Some(mut c) = self.db.creatures.get(&creature_id) {
                c.position = new_pos;
                c.action_kind = crate::db::ActionKind::Move;
                c.next_available_tick = Some(arrival_tick);
                let _ = self.db.update_creature(c);
            }

            // Create MoveAction for render interpolation.
            let _ = self.db.remove_move_action(&creature_id);
            self.db
                .insert_move_action(crate::db::MoveAction {
                    creature_id,
                    move_from: old_pos,
                    move_to: new_pos,
                    move_start_tick: tick,
                })
                .unwrap();

            // Advance cursor past all waypoints up to and including this one.
            if let Some(mut ap) = self
                .db
                .activity_participants
                .get(&(activity_id, creature_id))
            {
                ap.waypoint_cursor = (next_cursor + 1) as u32;
                let _ = self.db.update_activity_participant(ap);
            }

            // Reactivate when the move completes (on the beat).
            Some(arrival_tick)
        } else {
            // No more waypoints — hold position until the dance ends.
            let end_tick = execution_start + plan.total_ticks;
            Some(end_tick)
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

            // Update per-creature dance cooldown tracking.
            for cid in &participant_ids {
                if let Some(mut c) = self.db.creatures.get(cid) {
                    c.last_dance_tick = self.tick;
                    let _ = self.db.update_creature(c);
                }
            }

            // Update per-hall dance cooldown tracking.
            let structure_refs = self
                .db
                .activity_structure_refs
                .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
            for sr in &structure_refs {
                if sr.role == crate::db::ActivityStructureRole::DanceVenue
                    && let Some(mut s) = self.db.structures.get(&sr.structure_id)
                {
                    s.last_dance_completed_tick = self.tick;
                    let _ = self.db.update_structure(s);
                }
            }
        }

        // Release all participants and schedule reactivation so they resume
        // normal behavior (find tasks, wander, etc.). Cancel existing
        // activations first to prevent double-activation (B-erratic-movement).
        for cid in &participant_ids {
            if let Some(mut c) = self.db.creatures.get(cid) {
                c.current_activity = None;
                let _ = self.db.update_creature(c);
            }
            self.event_queue.cancel_creature_activations(*cid);
            self.schedule_reactivation(*cid);
        }

        // Drop the dance composition if present.
        if let Some(dance_data) = self.db.activity_dance_data.get(&activity_id)
            && let Some(comp_id) = dance_data.composition_id
        {
            let _ = self.db.remove_music_composition(&comp_id);
        }

        // Delete activity (cascade removes participants + dance data).
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
                let _ = self.db.update_creature(c);
            }
        }

        // Drop the dance composition if present.
        if let Some(dance_data) = self.db.activity_dance_data.get(&activity_id)
            && let Some(comp_id) = dance_data.composition_id
        {
            let _ = self.db.remove_music_composition(&comp_id);
        }

        // Delete activity (cascade removes participants + dance data).
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
            let _ = self.db.update_creature(c);
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
                        let _ = self.db.update_activity(a);
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
        self.db.insert_task(task).unwrap();

        // Assign the task to the creature.
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.current_task = Some(task_id);
            let _ = self.db.update_creature(c);
        }

        // Record the travel task on the participant.
        if let Some(mut p) = self
            .db
            .activity_participants
            .get(&(activity_id, creature_id))
        {
            p.travel_task = Some(task_id);
            let _ = self.db.update_activity_participant(p);
        }
    }

    /// Prune any stale `Volunteered` participant rows for an idle creature.
    /// Called before activity discovery so that creatures who volunteered but
    /// then picked up a task (eating, sleeping, moping) can re-volunteer once
    /// they become idle again. Preserves `Organizer` rows — the organizer of
    /// a spontaneous dance should not lose their role between activation cycles.
    pub(crate) fn prune_stale_volunteer_rows(&mut self, creature_id: CreatureId) {
        let participations = self
            .db
            .activity_participants
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        for p in &participations {
            if p.status == ParticipantStatus::Volunteered && p.role != ParticipantRole::Organizer {
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

            // Per-elf cooldown for dances — recently-danced elves should not
            // volunteer for new dances. Matches the "organize or join" spec.
            if activity.kind == ActivityKind::Dance
                && creature.last_dance_tick > 0
                && self.tick.saturating_sub(creature.last_dance_tick)
                    < self.config.activity.dance_elf_cooldown_ticks
            {
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

    /// Check if an Assembling activity's timeout has expired.
    ///
    /// If enough participants have arrived (>= min_count), the activity starts
    /// execution despite stragglers. Non-arrived participants are either kept
    /// (if `allows_late_join`) or removed. If too few participants arrived,
    /// the activity is cancelled.
    pub(crate) fn check_activity_assembly_timeout(
        &mut self,
        activity_id: ActivityId,
        events: &mut Vec<SimEvent>,
    ) {
        let activity = match self.db.activities.get(&activity_id) {
            Some(a) => a.clone(),
            None => return,
        };
        if activity.phase != ActivityPhase::Assembling {
            return;
        }
        let assembly_tick = match activity.assembly_started_tick {
            Some(t) => t,
            None => return,
        };
        if self.tick.saturating_sub(assembly_tick) < self.config.activity.assembly_timeout_ticks {
            return;
        }

        // Timeout has expired. Check if we have enough arrivals to start.
        let participants = self
            .db
            .activity_participants
            .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
        let arrived_count = participants
            .iter()
            .filter(|p| p.status == ParticipantStatus::Arrived)
            .count() as u16;
        let min = activity.min_count.unwrap_or(1);

        if arrived_count < min {
            // Not enough participants arrived — cancel.
            self.cancel_activity(activity_id, events);
            return;
        }

        // Enough arrived — start execution. Handle stragglers based on
        // allows_late_join: keep Traveling participants if true, remove if false.
        if !activity.allows_late_join {
            let stragglers: Vec<CreatureId> = participants
                .iter()
                .filter(|p| p.status != ParticipantStatus::Arrived)
                .map(|p| p.creature_id)
                .collect();
            for cid in stragglers {
                self.remove_participant(activity_id, cid, events);
            }
        }

        // Transition to Executing.
        self.begin_execution(activity_id);
    }

    /// Resume a Paused activity back to Executing (e.g., when a replacement arrives).
    #[allow(dead_code)] // Will be called when PauseAndWait replacement logic is wired.
    pub(crate) fn resume_activity(&mut self, activity_id: ActivityId) {
        if let Some(mut a) = self.db.activities.get(&activity_id)
            && a.phase == ActivityPhase::Paused
        {
            a.phase = ActivityPhase::Executing;
            a.pause_started_tick = None;
            let _ = self.db.update_activity(a);
        }
    }

    /// Transition an Assembling activity to the Executing phase. Sets the
    /// execution start tick and triggers kind-specific setup (e.g., dance
    /// choreography generation). Used by both `check_assembly_complete` (all
    /// participants arrived) and `check_activity_assembly_timeout` (timeout
    /// with enough arrivals).
    fn begin_execution(&mut self, activity_id: ActivityId) {
        let kind = match self.db.activities.get(&activity_id) {
            Some(a) => a.kind,
            None => return,
        };
        if let Some(mut a) = self.db.activities.get(&activity_id) {
            a.phase = ActivityPhase::Executing;
            a.execution_start_tick = Some(self.tick);
            let _ = self.db.update_activity(a);
        }
        if kind == ActivityKind::Dance {
            self.generate_dance_plan(activity_id);
        }
    }

    /// Helper: update the activity phase. Uses `update_activity` because `phase`
    /// is an indexed field.
    fn set_activity_phase(&mut self, activity_id: ActivityId, phase: ActivityPhase) {
        if let Some(mut a) = self.db.activities.get(&activity_id) {
            a.phase = phase;
            if phase == ActivityPhase::Assembling {
                a.assembly_started_tick = Some(self.tick);
            }
            let _ = self.db.update_activity(a);
        }
    }

    /// Try to have an idle creature spontaneously organize a dance at a nearby
    /// dance hall. Returns `true` if a dance was organized.
    ///
    /// Checks: creature is alive and idle, elf cooldown, nearby dance halls
    /// with hall cooldown / first-dance nudge / venue exclusivity, then rolls
    /// the organize probability.
    pub(crate) fn try_organize_spontaneous_dance(
        &mut self,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c.clone(),
            None => return false,
        };
        // Must be alive, idle, and an elf.
        if creature.vital_status != VitalStatus::Alive {
            return false;
        }
        if creature.current_task.is_some() || creature.current_activity.is_some() {
            return false;
        }
        if creature.species != Species::Elf {
            return false;
        }

        // Elf cooldown.
        if creature.last_dance_tick > 0
            && self.tick.saturating_sub(creature.last_dance_tick)
                < self.config.activity.dance_elf_cooldown_ticks
        {
            return false;
        }

        // Already a participant/volunteer in any activity — skip.
        let existing = self
            .db
            .activity_participants
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        if !existing.is_empty() {
            return false;
        }

        // Find nearby dance halls.
        let search_radius = self.config.activity.volunteer_search_radius.max(0) as u32;
        let pos = creature.position;

        let mut candidate_hall: Option<(crate::types::StructureId, u32)> = None;

        for structure in self.db.structures.iter_all() {
            if structure.furnishing != Some(crate::types::FurnishingType::DanceHall) {
                continue;
            }

            // Distance check.
            let dist = pos.manhattan_distance(structure.anchor);
            if dist > search_radius {
                continue;
            }

            // Venue exclusivity.
            if self.hall_has_active_dance(structure.id) {
                continue;
            }

            // Hall cooldown (skipped for first-dance nudge).
            if structure.last_dance_completed_tick > 0 {
                let elapsed = self
                    .tick
                    .saturating_sub(structure.last_dance_completed_tick);
                if elapsed < self.config.activity.dance_hall_cooldown_ticks {
                    continue;
                }
            }

            // Pick closest hall.
            if candidate_hall.is_none() || dist < candidate_hall.unwrap().1 {
                candidate_hall = Some((structure.id, dist));
            }
        }

        let (hall_id, _) = match candidate_hall {
            Some(h) => h,
            None => return false,
        };

        // Probability roll.
        // Integer PPM comparison for determinism (matches greenhouse pattern).
        if self.rng.next_u32() % 1_000_000 >= self.config.activity.dance_organize_chance_ppm {
            return false;
        }

        // Organize the dance.
        let structure = self.db.structures.get(&hall_id).unwrap().clone();
        let interior = structure.floor_interior_positions();
        let location = interior.first().copied().unwrap_or(structure.anchor);

        let activity_id = self.handle_create_activity(
            ActivityKind::Dance,
            location,
            Some(3),
            Some(6),
            crate::task::TaskOrigin::Autonomous,
            events,
        );

        // Link activity to the dance hall.
        let seq = self.db.activity_structure_refs.next_seq();
        let _ = self
            .db
            .insert_activity_structure_ref(crate::db::ActivityStructureRef {
                activity_id,
                seq,
                structure_id: hall_id,
                role: crate::db::ActivityStructureRole::DanceVenue,
            });

        // The organizer becomes the first participant with Organizer role.
        let participant = crate::db::ActivityParticipant {
            activity_id,
            creature_id,
            role: ParticipantRole::Organizer,
            status: ParticipantStatus::Volunteered,
            assigned_position: location,
            travel_task: None,
            dance_slot: None,
            waypoint_cursor: 0,
        };
        self.db.insert_activity_participant(participant).unwrap();

        true
    }

    /// Check whether a dance hall structure currently has an active (non-complete,
    /// non-cancelled) dance activity linked to it. Used for venue exclusivity —
    /// no two dances may run on the same hall simultaneously.
    pub(crate) fn hall_has_active_dance(&self, structure_id: crate::types::StructureId) -> bool {
        let refs = self
            .db
            .activity_structure_refs
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
        for r in &refs {
            if r.role != crate::db::ActivityStructureRole::DanceVenue {
                continue;
            }
            if let Some(activity) = self.db.activities.get(&r.activity_id)
                && !matches!(
                    activity.phase,
                    ActivityPhase::Complete | ActivityPhase::Cancelled
                )
            {
                return true;
            }
        }
        false
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
