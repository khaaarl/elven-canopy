//! Tests for group activities: lifecycle (creation, volunteering, quorum,
//! assembly, execution, completion, cancellation), departure policies,
//! directed recruitment, dances (debug, spontaneous, choreography, cooldowns),
//! and participant management (death, preemption, re-volunteering).
//! Corresponds to `sim/activity.rs`.

use super::*;

/// Create a debug dance activity.
fn create_debug_dance(sim: &mut SimState, location: VoxelCoord) -> crate::types::ActivityId {
    let before: std::collections::BTreeSet<_> =
        sim.db.activities.iter_all().map(|a| a.id).collect();
    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(3),
        Some(3),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut events,
    );
    sim.db
        .activities
        .iter_all()
        .map(|a| a.id)
        .find(|id| !before.contains(id))
        .expect("create_debug_dance: no new activity created")
}

/// Helper: create a furnished dance hall and return its structure ID.
fn create_dance_hall(sim: &mut SimState) -> StructureId {
    let anchor = find_building_site(sim);
    let structure_id = insert_completed_building(sim, anchor);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::DanceHall,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);
    structure_id
}

// ===========================================================================
// Group activity lifecycle tests
// ===========================================================================

#[test]
fn activity_creation_sets_phase_and_defaults() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);

    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.kind, ActivityKind::Dance);
    assert_eq!(activity.phase, ActivityPhase::Recruiting);
    assert_eq!(activity.recruitment, RecruitmentMode::Open);
    assert_eq!(activity.departure_policy, DeparturePolicy::Continue);
    assert!(activity.allows_late_join);
    assert_eq!(activity.min_count, Some(3));
    assert_eq!(activity.desired_count, Some(3));
    assert_eq!(activity.progress, 0);
    // total_cost starts at 0; set to plan.total_ticks when entering Executing.
    assert_eq!(activity.total_cost, 0);
    assert_eq!(activity.location, location);
}

#[test]
fn open_volunteering_creates_participant_without_committing() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);

    // Participant row should exist with Volunteered status.
    let p = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[0]))
        .unwrap();
    assert_eq!(p.status, ParticipantStatus::Volunteered);
    assert_eq!(p.role, ParticipantRole::Member);
    assert!(p.travel_task.is_none());

    // Creature should NOT have current_activity set.
    let creature = sim.db.creatures.get(&elves[0]).unwrap();
    assert!(creature.current_activity.is_none());

    // Activity should still be Recruiting (only 1 of 3 needed).
    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.phase, ActivityPhase::Recruiting);
}

#[test]
fn quorum_promotes_volunteers_to_assembling() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);
    assert!(elves.len() >= 3, "need at least 3 elves for this test");

    // Clear all tasks so elves are truly idle.
    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    // Volunteer 2 — not enough yet.
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    sim.volunteer_for_activity(activity_id, elves[1], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting
    );

    // Volunteer 3 — should trigger quorum, promoting to Assembling.
    sim.volunteer_for_activity(activity_id, elves[2], &mut events);

    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.phase, ActivityPhase::Assembling);

    // All 3 participants should now be Traveling with GoTo tasks.
    for i in 0..3 {
        let p = sim
            .db
            .activity_participants
            .get(&(activity_id, elves[i]))
            .unwrap();
        assert_eq!(p.status, ParticipantStatus::Traveling);
        assert!(p.travel_task.is_some());

        let creature = sim.db.creatures.get(&elves[i]).unwrap();
        assert_eq!(creature.current_activity, Some(activity_id));
        assert!(creature.current_task.is_some());
    }
}

#[test]
fn quorum_prunes_busy_volunteers() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);
    assert!(elves.len() >= 4);

    // Clear all tasks so elves are idle.
    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    // 3 volunteer.
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    sim.volunteer_for_activity(activity_id, elves[1], &mut events);
    // Before 3rd volunteer, give elf[0] a task (making them busy).
    let fake_task_id = TaskId::new(&mut sim.rng.clone());
    // We need to create an actual task for the FK to work.
    let task = crate::db::Task {
        id: fake_task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::InProgress,
        origin: TaskOrigin::Automated,
        location,
        progress: 0,
        total_cost: 0,
        required_species: None,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.db.insert_task(task).unwrap();
    let mut c = sim.db.creatures.get(&elves[0]).unwrap();
    c.current_task = Some(fake_task_id);
    sim.db.update_creature(c).unwrap();

    // 3rd volunteer triggers quorum check, but elf[0] is now busy → pruned.
    sim.volunteer_for_activity(activity_id, elves[2], &mut events);

    // Should still be Recruiting (only 2 valid volunteers remain).
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting
    );
    // elf[0]'s participant row should be removed.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_none()
    );
}

#[test]
fn arrival_transitions_to_executing() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // Simulate GoTo completion for each participant.
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.phase, ActivityPhase::Executing);
    assert!(activity.execution_start_tick.is_some());

    // All participants should be Arrived with no travel_task.
    for i in 0..3 {
        let p = sim
            .db
            .activity_participants
            .get(&(activity_id, elves[i]))
            .unwrap();
        assert_eq!(p.status, ParticipantStatus::Arrived);
        assert!(p.travel_task.is_none());
    }
}

#[test]
fn dance_execution_generates_plan_and_adds_thoughts() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // After entering Executing, a DancePlan should have been generated.
    assert!(
        sim.db.activity_dance_data.get(&activity_id).is_some(),
        "DancePlan should be generated on Executing transition"
    );

    // Participants should have dance slots assigned.
    for i in 0..3 {
        let p = sim
            .db
            .activity_participants
            .get(&(activity_id, elves[i]))
            .unwrap();
        assert!(p.dance_slot.is_some(), "elf {i} should have a dance slot");
    }

    // Execute one activation — EnjoyingDance thought should be added.
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC);
    assert!(
        thoughts
            .iter()
            .any(|t| t.kind == ThoughtKind::EnjoyingDance)
    );
}

#[test]
fn dance_waypoints_move_creatures() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Get the dance plan and find the first waypoint for slot 0.
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let slot0_waypoints = &dance_data.plan.slot_waypoints[0];
    if slot0_waypoints.is_empty() {
        return; // Cramped formation with no movement — valid.
    }

    let first_wp = slot0_waypoints[0].clone();
    let elf0_pos_before = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // First activation: creature should set up a Move action toward the
    // first waypoint, arriving at first_wp.tick.
    let next_tick = sim.execute_activity_behavior(elves[0], activity_id, &mut events);
    assert_eq!(
        next_tick,
        Some(first_wp.tick),
        "should schedule reactivation at first waypoint tick"
    );

    // A MoveAction should exist pointing from old pos to first waypoint.
    let move_action = sim.db.move_actions.get(&elves[0]);
    assert!(
        move_action.is_some(),
        "MoveAction should exist for interpolation"
    );
    let ma = move_action.unwrap();
    assert_eq!(ma.move_from, elf0_pos_before);
    assert_eq!(ma.move_to, first_wp.position);

    // Creature's sim position should already be at the target (sim truth).
    let elf0_pos_after = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    assert_eq!(elf0_pos_after, first_wp.position);

    // Creature should be in Move action state.
    let creature = sim.db.creatures.get(&elves[0]).unwrap();
    assert_eq!(creature.action_kind, crate::db::ActionKind::Move);

    // Cursor should have advanced past the first waypoint.
    let p = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[0]))
        .unwrap();
    assert!(p.waypoint_cursor >= 1, "cursor should have advanced");
}

#[test]
fn dance_completes_with_mood_bonus_and_cleanup() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // The plan should exist and have a total_ticks value.
    let plan_total = sim
        .db
        .activity_dance_data
        .get(&activity_id)
        .unwrap()
        .plan
        .total_ticks;
    assert!(plan_total > 0);

    // Advance sim.tick past the plan's total_ticks to trigger completion.
    let execution_start = sim
        .db
        .activities
        .get(&activity_id)
        .unwrap()
        .execution_start_tick
        .unwrap();
    sim.tick = execution_start + plan_total;

    // One activation should trigger completion.
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // Activity should be deleted.
    assert!(sim.db.activities.get(&activity_id).is_none());

    // All participant rows should be gone.
    for i in 0..3 {
        assert!(
            sim.db
                .activity_participants
                .get(&(activity_id, elves[i]))
                .is_none()
        );
    }

    // All creatures should have current_activity cleared.
    for i in 0..3 {
        let creature = sim.db.creatures.get(&elves[i]).unwrap();
        assert!(creature.current_activity.is_none());
    }

    // DancedInGroup thought should be present on all participants.
    for i in 0..3 {
        let thoughts = sim
            .db
            .thoughts
            .by_creature_id(&elves[i], tabulosity::QueryOpts::ASC);
        assert!(
            thoughts
                .iter()
                .any(|t| t.kind == ThoughtKind::DancedInGroup),
            "elf {} should have DancedInGroup thought",
            i
        );
    }
}

#[test]
fn cancel_activity_releases_participants() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    // All 3 should be Traveling now (quorum met).
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // Cancel.
    sim.cancel_activity(activity_id, &mut events);

    // Activity deleted.
    assert!(sim.db.activities.get(&activity_id).is_none());

    // All participants released.
    for i in 0..3 {
        let creature = sim.db.creatures.get(&elves[i]).unwrap();
        assert!(creature.current_activity.is_none());
        assert!(creature.current_task.is_none());
    }
}

#[test]
fn departure_continue_policy_keeps_activity_running() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );

    // Remove one participant — dance uses Continue policy.
    sim.remove_participant(activity_id, elves[0], &mut events);

    // Activity should still be Executing.
    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.phase, ActivityPhase::Executing);

    // Removed participant is gone.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_none()
    );
    let creature = sim.db.creatures.get(&elves[0]).unwrap();
    assert!(creature.current_activity.is_none());
}

#[test]
fn departure_cancel_on_departure_cancels_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);

    // Create a Ceremony activity (uses CancelOnDeparture).
    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::Ceremony,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Directed recruitment for Ceremony.
    for i in 0..3 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );
    assert_eq!(
        sim.db
            .activities
            .get(&activity_id)
            .unwrap()
            .departure_policy,
        DeparturePolicy::CancelOnDeparture
    );

    // Remove one participant — should cancel the whole activity.
    sim.remove_participant(activity_id, elves[0], &mut events);

    assert!(sim.db.activities.get(&activity_id).is_none());
    for i in 0..3 {
        let creature = sim.db.creatures.get(&elves[i]).unwrap();
        assert!(creature.current_activity.is_none());
    }
}

#[test]
fn departure_pause_and_wait_pauses_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);

    // Create a ConstructionChoir activity (uses PauseAndWait).
    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    for i in 0..3 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );

    // Remove one participant — should pause.
    sim.remove_participant(activity_id, elves[0], &mut events);

    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.phase, ActivityPhase::Paused);
    assert!(activity.pause_started_tick.is_some());
}

#[test]
fn pause_timeout_cancels_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    for i in 0..3 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Pause via departure.
    sim.remove_participant(activity_id, elves[0], &mut events);
    let activity = sim.db.activities.get(&activity_id).unwrap();
    let timeout = match activity.departure_policy {
        DeparturePolicy::PauseAndWait { timeout_ticks } => timeout_ticks,
        _ => panic!("expected PauseAndWait"),
    };

    // Advance tick past timeout.
    sim.tick += timeout + 1;
    sim.check_activity_pause_timeout(activity_id, &mut events);

    // Activity should be cancelled.
    assert!(sim.db.activities.get(&activity_id).is_none());
}

#[test]
fn directed_recruitment_assigns_and_creates_goto() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Assign one creature.
    sim.handle_assign_to_activity(activity_id, elves[0], &mut events);

    let p = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[0]))
        .unwrap();
    assert_eq!(p.status, ParticipantStatus::Traveling);
    assert!(p.travel_task.is_some());

    let creature = sim.db.creatures.get(&elves[0]).unwrap();
    assert_eq!(creature.current_activity, Some(activity_id));
    assert!(creature.current_task.is_some());
}

#[test]
fn all_participants_leaving_cancels_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Remove all 3 — after the last one, activity should be cancelled.
    sim.remove_participant(activity_id, elves[0], &mut events);
    sim.remove_participant(activity_id, elves[1], &mut events);
    sim.remove_participant(activity_id, elves[2], &mut events);

    assert!(sim.db.activities.get(&activity_id).is_none());
}

#[test]
fn double_volunteer_is_no_op() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    let mut c0 = sim.db.creatures.get(&elves[0]).unwrap();
    c0.current_task = None;
    sim.db.update_creature(c0).unwrap();

    let mut events = Vec::new();
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);

    // Should only have 1 participant row.
    let participants = sim
        .db
        .activity_participants
        .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
    assert_eq!(participants.len(), 1);
}

#[test]
fn find_open_activity_discovers_nearby_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let elf_pos = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Create a dance near the elf.
    let activity_id = create_debug_dance(&mut sim, elf_pos);

    // Make elf idle.
    let mut c0 = sim.db.creatures.get(&elves[0]).unwrap();
    c0.current_task = None;
    sim.db.update_creature(c0).unwrap();

    let found = sim.find_open_activity_for_creature(elves[0]);
    assert_eq!(found, Some(activity_id));
}

#[test]
fn find_open_activity_ignores_far_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);

    // Create a dance very far away.
    let far_location = VoxelCoord::new(60, 1, 60);
    // Set search radius to something small.
    sim.config.activity.volunteer_search_radius = 5;
    let _ = create_debug_dance(&mut sim, far_location);

    let mut c0 = sim.db.creatures.get(&elves[0]).unwrap();
    c0.current_task = None;
    sim.db.update_creature(c0).unwrap();

    let found = sim.find_open_activity_for_creature(elves[0]);
    assert!(found.is_none());
}

#[test]
fn end_to_end_dance_via_command_and_activation() {
    // End-to-end: create activity via command, volunteer elves, simulate
    // GoTo arrival, run execution via activation loop, verify completion.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Issue CreateActivity command.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick,
        action: SimAction::CreateActivity {
            zone_id: sim.home_zone_id(),
            kind: ActivityKind::Dance,
            location,
            min_count: Some(3),
            desired_count: Some(3),
            origin: TaskOrigin::PlayerDirected,
        },
    };
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    let activity_id = sim.db.activities.iter_all().next().unwrap().id;
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting
    );

    // Volunteer 3 elves directly (simulating what the activation loop does).
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // Simulate GoTo arrival for all 3.
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );

    // A dance plan should have been generated.
    let plan_total = sim
        .db
        .activity_dance_data
        .get(&activity_id)
        .unwrap()
        .plan
        .total_ticks;
    assert!(plan_total > 0);

    // Advance sim tick past the plan duration and run one activation.
    let execution_start = sim
        .db
        .activities
        .get(&activity_id)
        .unwrap()
        .execution_start_tick
        .unwrap();
    sim.tick = execution_start + plan_total;
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // Activity should be done.
    assert!(sim.db.activities.get(&activity_id).is_none());

    // All elves should have DancedInGroup thought.
    for i in 0..3 {
        let thoughts = sim
            .db
            .thoughts
            .by_creature_id(&elves[i], tabulosity::QueryOpts::ASC);
        assert!(
            thoughts
                .iter()
                .any(|t| t.kind == ThoughtKind::DancedInGroup),
            "elf {} should have DancedInGroup thought after e2e dance",
            i
        );
    }
}

#[test]
fn assign_to_activity_rejects_dead_creature() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;

    // Kill the creature.
    sim.handle_creature_death(elves[0], crate::types::DeathCause::Debug, &mut events);

    // Try to assign — should be rejected.
    sim.handle_assign_to_activity(activity_id, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_none()
    );
}

#[test]
fn assign_to_activity_rejects_creature_already_in_activity() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    // Freeze all elves so none autonomously join an activity before
    // the test can explicitly assign them.
    for &elf in &elves {
        force_idle_and_cancel_activations(&mut sim, elf);
    }
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();

    // Snapshot existing activity IDs so we can identify new ones.
    let before: std::collections::BTreeSet<_> =
        sim.db.activities.iter_all().map(|a| a.id).collect();

    // Create first activity.
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity1 = sim
        .db
        .activities
        .iter_all()
        .find(|a| !before.contains(&a.id))
        .unwrap()
        .id;

    // Assign elf to first activity.
    sim.handle_assign_to_activity(activity1, elves[0], &mut events);
    assert!(
        sim.db
            .creatures
            .get(&elves[0])
            .unwrap()
            .current_activity
            .is_some()
    );

    // Snapshot again before creating the second activity.
    let before2: std::collections::BTreeSet<_> =
        sim.db.activities.iter_all().map(|a| a.id).collect();

    // Create second activity and try to assign same elf — should be rejected.
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity2 = sim
        .db
        .activities
        .iter_all()
        .find(|a| !before2.contains(&a.id))
        .unwrap()
        .id;

    sim.handle_assign_to_activity(activity2, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity2, elves[0]))
            .is_none()
    );
}

#[test]
fn volunteer_rejects_directed_recruitment_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();
    // ConstructionChoir uses Directed recruitment.
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;

    // Try to volunteer — should be rejected.
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_none()
    );
}

#[test]
fn creature_death_during_executing_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );

    // Kill one dancer. Dance uses Continue policy — should keep going.
    sim.handle_creature_death(elves[0], crate::types::DeathCause::Debug, &mut events);

    // Activity should still exist and be Executing (Continue policy).
    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.phase, ActivityPhase::Executing);

    // Dead creature should no longer be a participant.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_none()
    );
}

#[test]
fn creature_death_with_cancel_on_departure_cancels_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::Ceremony,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;

    for eid in &elves[..3] {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    for i in 0..3 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Kill one creature during ceremony — CancelOnDeparture should cancel all.
    sim.handle_creature_death(elves[0], crate::types::DeathCause::Debug, &mut events);

    assert!(sim.db.activities.get(&activity_id).is_none());
}

#[test]
fn resume_activity_from_paused() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;

    for eid in &elves[..3] {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    for i in 0..3 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Trigger pause.
    sim.remove_participant(activity_id, elves[0], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Paused
    );

    // Resume.
    sim.resume_activity(activity_id);
    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.phase, ActivityPhase::Executing);
    assert!(activity.pause_started_tick.is_none());
}

// ===========================================================================
// Corner-case and double-reactivation tests
// ===========================================================================

#[test]
fn no_double_reactivation_after_activity_execution() {
    // Verify that executing activity behavior does not produce duplicate
    // activation events in the event queue.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Clear next_available_tick so we can verify it gets set.
    let mut c = sim.db.creatures.get(&elves[0]).unwrap();
    c.next_available_tick = None;
    sim.db.update_creature(c).unwrap();

    // Run one activation via the activation loop.
    sim.process_creature_activation(elves[0], &mut events);

    // Should have exactly 1 pending activation (next_available_tick is Some).
    assert_eq!(
        sim.count_pending_activations_for(elves[0]),
        1,
        "should have exactly 1 reactivation, not more"
    );
}

#[test]
fn no_double_reactivation_on_activity_completion() {
    // When a dance completes during execute_activity_behavior, complete_activity
    // schedules reactivation. The activation loop should NOT schedule another.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Advance tick past plan duration so the next activation triggers completion.
    let plan_total = sim
        .db
        .activity_dance_data
        .get(&activity_id)
        .unwrap()
        .plan
        .total_ticks;
    let execution_start = sim
        .db
        .activities
        .get(&activity_id)
        .unwrap()
        .execution_start_tick
        .unwrap();
    sim.tick = execution_start + plan_total;

    // Clear next_available_tick for all participants.
    for i in 0..3 {
        let mut c = sim.db.creatures.get(&elves[i]).unwrap();
        c.next_available_tick = None;
        sim.db.update_creature(c).unwrap();
    }

    // Run one activation — this should complete the dance.
    sim.process_creature_activation(elves[0], &mut events);

    // Activity should be gone.
    assert!(sim.db.activities.get(&activity_id).is_none());

    // Each participant should have exactly 1 pending activation (next_available_tick is Some).
    for i in 0..3 {
        let count = sim.count_pending_activations_for(elves[i]);
        assert_eq!(
            count, 1,
            "elf {} should have exactly 1 reactivation after completion, got {}",
            i, count
        );
    }
}

#[test]
fn preempted_goto_during_assembly_reissues_goto() {
    // Scenario: 3 elves volunteer for a dance. All 3 get GoTo tasks during
    // Assembling. Elf C's GoTo is preempted (e.g., by moping). After the
    // preempting task completes, elf C should get a new GoTo to continue
    // walking to the assembly point.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // Elf 2's GoTo task is preempted (simulating mope/eat interruption).
    let participant = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[2]))
        .unwrap();
    let goto_task_id = participant.travel_task.unwrap();
    sim.preempt_task(elves[2], goto_task_id);

    // Elf 2 should still have current_activity but no current_task.
    let c2 = sim.db.creatures.get(&elves[2]).unwrap();
    assert_eq!(c2.current_activity, Some(activity_id));
    assert!(c2.current_task.is_none());

    // Simulate the preempting task completing — creature enters activation
    // loop with current_activity but no task.
    suppress_activation_until(&mut sim, elves[2], u64::MAX);
    sim.process_creature_activation(elves[2], &mut events);

    // reissue_activity_goto_if_needed should have created a new GoTo task.
    let c2 = sim.db.creatures.get(&elves[2]).unwrap();
    assert!(
        c2.current_task.is_some(),
        "elf should have a new GoTo task after preemption"
    );
    assert_eq!(c2.current_activity, Some(activity_id));

    // The participant should have an updated travel_task.
    let participant = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[2]))
        .unwrap();
    assert!(participant.travel_task.is_some());
    assert_ne!(
        participant.travel_task.unwrap(),
        goto_task_id,
        "should be a new task, not the old preempted one"
    );
}

#[test]
fn elves_a_and_b_idle_while_c_is_preempted() {
    // When 2 of 3 required elves have arrived but the 3rd is preempted,
    // the arrived elves should idle (not double-activate, not wander off).
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }

    // Elves 0 and 1 arrive.
    sim.on_activity_participant_arrived(activity_id, elves[0], &mut events);
    sim.on_activity_participant_arrived(activity_id, elves[1], &mut events);

    // Activity should still be Assembling (only 2 of 3 arrived).
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // Clear next_available_tick and run activation for elf 0.
    let mut c = sim.db.creatures.get(&elves[0]).unwrap();
    c.next_available_tick = None;
    sim.db.update_creature(c).unwrap();
    sim.process_creature_activation(elves[0], &mut events);

    // Elf 0 should still be in the activity, not wandering.
    let c0 = sim.db.creatures.get(&elves[0]).unwrap();
    assert_eq!(c0.current_activity, Some(activity_id));
    assert!(c0.current_task.is_none()); // No task, just idling.

    // Should have exactly 1 reactivation (next_available_tick is Some).
    assert_eq!(sim.count_pending_activations_for(elves[0]), 1);
}

#[test]
fn busy_creatures_dont_volunteer() {
    // Creatures with current_task (eating, sleeping, moping) should not
    // discover or volunteer for open activities.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let _ = create_debug_dance(&mut sim, location);

    // Give elf 0 a task.
    let task_id = TaskId::new(&mut sim.rng);
    let task = crate::db::Task {
        id: task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::InProgress,
        origin: TaskOrigin::Automated,
        location,
        progress: 0,
        total_cost: 0,
        required_species: None,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.db.insert_task(task).unwrap();
    let mut c = sim.db.creatures.get(&elves[0]).unwrap();
    c.current_task = Some(task_id);
    sim.db.update_creature(c).unwrap();

    // Should not find any open activity.
    let found = sim.find_open_activity_for_creature(elves[0]);
    assert!(
        found.is_none(),
        "busy creature should not discover activities"
    );
}

#[test]
fn recruiting_activity_reference_cleared_on_activation() {
    // If a creature's current_activity points to an activity still in
    // Recruiting phase, the activation loop treats it as unexpected and
    // clears it. (The original "stale/deleted activity" scenario is now
    // prevented by FK constraints — current_activity cannot reference a
    // nonexistent activity.)
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    // Delete the activity (FK cascade nullifies current_activity).
    let mut events = Vec::new();
    sim.db.remove_activity(&activity_id).unwrap();

    // Manually set a stale current_activity (simulating corruption or
    // a code path that bypasses FK cleanup).
    // Re-insert a stub in Recruiting phase so the FK is satisfied during
    // update_creature. The activation loop treats Recruiting as an unexpected
    // phase and clears current_activity, which is the behavior under test.
    sim.db
        .insert_activity(crate::db::Activity {
            id: activity_id,
            zone_id: sim.home_zone_id(),
            kind: ActivityKind::Dance,
            phase: ActivityPhase::Recruiting,
            location: VoxelCoord::new(0, 0, 0),
            min_count: None,
            desired_count: None,
            progress: 0,
            total_cost: 0,
            origin: TaskOrigin::Automated,
            recruitment: RecruitmentMode::Open,
            departure_policy: DeparturePolicy::Continue,
            allows_late_join: false,
            civ_id: None,
            required_species: None,
            execution_start_tick: None,
            pause_started_tick: None,
            assembly_started_tick: None,
        })
        .unwrap();
    let mut c0 = sim.db.creatures.get(&elves[0]).unwrap();
    c0.current_activity = Some(activity_id);
    sim.db.update_creature(c0).unwrap();

    let c0 = sim.db.creatures.get(&elves[0]).unwrap();
    assert!(c0.current_activity.is_some());

    // Clear next_available_tick and run activation — should clear the stale reference.
    let mut c = sim.db.creatures.get(&elves[0]).unwrap();
    c.next_available_tick = None;
    sim.db.update_creature(c).unwrap();
    sim.process_creature_activation(elves[0], &mut events);

    let c0 = sim.db.creatures.get(&elves[0]).unwrap();
    assert!(
        c0.current_activity.is_none(),
        "stale activity reference should be cleared"
    );
    // Should have a reactivation (next_available_tick is Some).
    assert_eq!(sim.count_pending_activations_for(elves[0]), 1);
}

#[test]
fn cancel_activity_no_double_reactivation() {
    // Cancelling an activity should not produce duplicate activations.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Cancel the activity.
    sim.cancel_activity(activity_id, &mut events);

    // Each creature should have exactly 1 pending activation (next_available_tick is Some).
    for i in 0..3 {
        let count = sim.count_pending_activations_for(elves[i]);
        assert_eq!(
            count, 1,
            "elf {} should have exactly 1 reactivation after cancel, got {}",
            i, count
        );
    }
}

#[test]
fn activity_with_all_elves_busy_stays_recruiting() {
    // If all elves in the sim are busy (have tasks), the dance activity
    // should stay in Recruiting with no volunteers.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let _ = create_debug_dance(&mut sim, location);

    // Give all elves tasks (simulating early game).
    // Generate task IDs up front to avoid borrow issues with sim.rng.
    let task_ids: Vec<TaskId> = (0..elves.len())
        .map(|_| TaskId::new(&mut sim.rng))
        .collect();
    for (eid, task_id) in elves.iter().zip(task_ids.iter()) {
        let task = crate::db::Task {
            id: *task_id,
            zone_id: sim.home_zone_id(),
            kind_tag: TaskKindTag::GoTo,
            state: TaskState::InProgress,
            origin: TaskOrigin::Automated,
            location,
            progress: 0,
            total_cost: 0,
            required_species: None,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        sim.db.insert_task(task).unwrap();
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = Some(*task_id);
        sim.db.update_creature(c).unwrap();
    }

    // No creature should find the activity.
    for eid in &elves {
        assert!(sim.find_open_activity_for_creature(*eid).is_none());
    }

    // Activity should have no participants.
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;
    let participants = sim
        .db
        .activity_participants
        .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
    assert!(participants.is_empty());
}

#[test]
fn partial_arrival_then_full_arrival_transitions_to_executing() {
    // 2 of 3 elves arrive, activity stays in Assembling. When the 3rd
    // arrives later, activity transitions to Executing.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }

    // Only 2 arrive initially.
    sim.on_activity_participant_arrived(activity_id, elves[0], &mut events);
    sim.on_activity_participant_arrived(activity_id, elves[1], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // 3rd arrives later.
    sim.on_activity_participant_arrived(activity_id, elves[2], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );
}

#[test]
fn participant_death_during_assembly_reverts_to_recruiting_and_replacement_joins() {
    // Scenario: 3 elves volunteer, quorum promotes to Assembling. Elf 2 dies
    // mid-assembly (others still en route). Activity should revert to
    // Recruiting. A 4th idle elf discovers it, volunteers, quorum re-triggers,
    // and the activity resumes assembly with the replacement.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    // 3 volunteer → quorum → Assembling with GoTo tasks.
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // Elf 2 dies mid-assembly (others still Traveling).
    sim.handle_creature_death(elves[2], crate::types::DeathCause::Debug, &mut events);

    // Activity should revert to Recruiting (only 2 committed, below min_count=3).
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting,
        "activity should revert to Recruiting when participant count drops below min_count"
    );

    // Elf 2 should be fully removed.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[2]))
            .is_none()
    );

    // Remaining 2 elves should still be participants (Traveling), but their
    // current_activity and current_task should be cleared since we reverted
    // to Recruiting (they're no longer committed).
    // Actually — the remaining elves keep their Traveling status and GoTo tasks.
    // The revert to Recruiting re-opens the activity for new volunteers.
    // When a new volunteer joins and quorum re-triggers, the existing Traveling
    // participants continue their GoTo (they don't need to re-volunteer).

    // Elf 3 (idle) discovers the activity and volunteers.
    let mut c3 = sim.db.creatures.get(&elves[3]).unwrap();
    c3.current_task = None;
    sim.db.update_creature(c3).unwrap();

    sim.volunteer_for_activity(activity_id, elves[3], &mut events);

    // Quorum check: 2 existing Traveling + 1 new Volunteered = 3 >= min_count.
    // Should transition back to Assembling.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
        "activity should return to Assembling when replacement fills the gap"
    );

    // New volunteer should be promoted to Traveling with GoTo.
    let p3 = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[3]))
        .unwrap();
    assert_eq!(p3.status, ParticipantStatus::Traveling);
    assert!(p3.travel_task.is_some());
}

#[test]
fn participant_death_during_assembly_with_some_arrived() {
    // Variant: 2 of 3 elves have already arrived, 3rd dies en route.
    // Activity should revert to Recruiting. A replacement elf volunteers,
    // quorum re-triggers, and assembly resumes.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }

    // Elves 0 and 1 arrive.
    sim.on_activity_participant_arrived(activity_id, elves[0], &mut events);
    sim.on_activity_participant_arrived(activity_id, elves[1], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // Elf 2 dies while still Traveling.
    sim.handle_creature_death(elves[2], crate::types::DeathCause::Debug, &mut events);

    // Should revert to Recruiting.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting,
        "should revert to Recruiting when a death drops count below min_count"
    );

    // Arrived elves (0, 1) should still be participants.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_some()
    );
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[1]))
            .is_some()
    );

    // Replacement elf volunteers.
    let mut c3 = sim.db.creatures.get(&elves[3]).unwrap();
    c3.current_task = None;
    sim.db.update_creature(c3).unwrap();
    sim.volunteer_for_activity(activity_id, elves[3], &mut events);

    // Should be back to Assembling.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // After replacement arrives, should transition to Executing.
    sim.on_activity_participant_arrived(activity_id, elves[3], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing,
        "all 3 arrived (0, 1 earlier + 3 replacement) — should be Executing"
    );
}

#[test]
fn cannot_assign_to_activity_while_volunteered_for_another() {
    // A creature with a Volunteered row in Dance A should be rejected
    // when directed-assigned to Activity B.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Create Dance A (Open recruitment).
    let activity_a = create_debug_dance(&mut sim, location);

    // Create ConstructionChoir B (Directed recruitment).
    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_b = sim
        .db
        .activities
        .iter_all()
        .find(|a| a.kind == ActivityKind::ConstructionChoir)
        .unwrap()
        .id;

    // Elf 0 volunteers for Dance A.
    sim.volunteer_for_activity(activity_a, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_a, elves[0]))
            .is_some()
    );

    // Try to directed-assign elf 0 to Activity B — should be rejected.
    sim.handle_assign_to_activity(activity_b, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_b, elves[0]))
            .is_none(),
        "creature should not be in two activities"
    );
}

#[test]
fn volunteering_for_second_activity_replaces_first() {
    // A creature volunteered for Dance A. When they volunteer for Dance B,
    // the old volunteer row is pruned (they switch allegiance).
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_a = create_debug_dance(&mut sim, location);

    // Elf 0 volunteers for Dance A.
    let mut events = Vec::new();
    sim.volunteer_for_activity(activity_a, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_a, elves[0]))
            .is_some()
    );

    // Create Dance B at same location.
    sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(3),
        Some(3),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_b = sim
        .db
        .activities
        .iter_all()
        .find(|a| a.id != activity_a)
        .unwrap()
        .id;

    // Volunteer for Dance B — should prune Dance A row and create Dance B row.
    sim.volunteer_for_activity(activity_b, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_a, elves[0]))
            .is_none(),
        "old volunteer row for A should be pruned"
    );
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_b, elves[0]))
            .is_some(),
        "new volunteer row for B should exist"
    );
}

#[test]
fn committed_creature_cannot_volunteer_for_another() {
    // A creature committed (Traveling/Arrived) to an activity should NOT
    // be able to volunteer for a second one.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();
    // Create ConstructionChoir (Directed) and assign elf 0.
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_a = sim.db.activities.iter_all().next().unwrap().id;
    sim.handle_assign_to_activity(activity_a, elves[0], &mut events);

    // Elf 0 is now Traveling with current_activity set.
    let c0 = sim.db.creatures.get(&elves[0]).unwrap();
    assert!(
        c0.current_activity.is_some(),
        "elf should have current_activity after directed assignment, got {:?}",
        c0.current_activity
    );
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_a, elves[0]))
            .is_some(),
        "elf should be a participant in activity_a"
    );

    // Create Dance B and try to volunteer.
    let activity_b = create_debug_dance(&mut sim, location);
    sim.volunteer_for_activity(activity_b, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_b, elves[0]))
            .is_none(),
        "committed creature should not volunteer for another activity"
    );
}

#[test]
fn activity_with_min_count_none_starts_on_first_volunteer() {
    // min_count = None → activity starts with any number of participants.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        None,
        None,
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;

    // Single volunteer should trigger quorum immediately.
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
        "min_count=None should start assembling on first volunteer"
    );

    // Single arrival should trigger execution.
    sim.on_activity_participant_arrived(activity_id, elves[0], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing,
        "min_count=None should start executing on first arrival"
    );
}

#[test]
fn creature_discovers_second_activity_after_first_cancelled() {
    // A creature volunteered for Dance A, which gets cancelled.
    // On next activation, the creature should be free to discover Dance B.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_a = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    sim.volunteer_for_activity(activity_a, elves[0], &mut events);

    // Cancel Dance A.
    sim.cancel_activity(activity_a, &mut events);

    // Create Dance B.
    let activity_b = create_debug_dance(&mut sim, location);

    // Elf 0 should now be able to discover Dance B.
    let found = sim.find_open_activity_for_creature(elves[0]);
    assert_eq!(found, Some(activity_b));
}

#[test]
fn stale_volunteer_row_cleared_so_creature_can_revolunteer() {
    // Scenario: Elf A volunteers for a dance. Before quorum, elf A picks up
    // an Eat task (heartbeat). The stale Volunteered row should be cleared
    // when elf A becomes idle again, allowing them to re-volunteer.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();

    // Elf 0 volunteers.
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_some()
    );

    // Elf 0 gets an Eat task (simulating heartbeat).
    let task_id = TaskId::new(&mut sim.rng);
    let task = crate::db::Task {
        id: task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::EatBread,
        state: TaskState::InProgress,
        origin: TaskOrigin::Autonomous,
        location,
        progress: 0,
        total_cost: 0,
        required_species: None,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.db.insert_task(task).unwrap();
    let mut c = sim.db.creatures.get(&elves[0]).unwrap();
    c.current_task = Some(task_id);
    sim.db.update_creature(c).unwrap();

    // Elf 0 finishes eating — clear task.
    sim.complete_task(task_id);
    let c = sim.db.creatures.get(&elves[0]).unwrap();
    assert!(
        c.current_task.is_none(),
        "task should be cleared after completion"
    );

    // Prune stale volunteer rows (normally done by the activation loop
    // before discovery), then re-discover the activity.
    sim.prune_stale_volunteer_rows(elves[0]);
    let found = sim.find_open_activity_for_creature(elves[0]);
    assert_eq!(
        found,
        Some(activity_id),
        "idle creature should rediscover the activity after stale volunteer row is cleared"
    );
}

#[test]
fn recruiting_stall_resolved_by_revolunteering() {
    // Full stall scenario: 2 elves volunteer, both get tasks before a 3rd
    // joins. When they become idle again, they should be able to re-volunteer
    // and eventually reach quorum.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();

    // 2 elves volunteer.
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    sim.volunteer_for_activity(activity_id, elves[1], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting,
        "only 2 of 3 — should stay Recruiting"
    );

    // Both get tasks (simulating heartbeat).
    let task_ids: Vec<TaskId> = (0..2).map(|_| TaskId::new(&mut sim.rng)).collect();
    for (i, tid) in task_ids.iter().enumerate() {
        let task = crate::db::Task {
            id: *tid,
            zone_id: sim.home_zone_id(),
            kind_tag: TaskKindTag::EatBread,
            state: TaskState::InProgress,
            origin: TaskOrigin::Autonomous,
            location,
            progress: 0,
            total_cost: 0,
            required_species: None,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        sim.db.insert_task(task).unwrap();
        let mut c = sim.db.creatures.get(&elves[i]).unwrap();
        c.current_task = Some(*tid);
        sim.db.update_creature(c).unwrap();
    }

    // Both finish tasks.
    for tid in &task_ids {
        sim.complete_task(*tid);
    }

    // All 3 elves are now idle. Prune stale rows (normally done by activation
    // loop), then elves 0 and 1 should re-volunteer, and elf 2 joins fresh.
    sim.prune_stale_volunteer_rows(elves[0]);
    let found0 = sim.find_open_activity_for_creature(elves[0]);
    assert_eq!(
        found0,
        Some(activity_id),
        "elf 0 should rediscover the activity"
    );

    sim.volunteer_for_activity(activity_id, elves[0], &mut events);

    sim.prune_stale_volunteer_rows(elves[1]);
    let found1 = sim.find_open_activity_for_creature(elves[1]);
    assert_eq!(
        found1,
        Some(activity_id),
        "elf 1 should rediscover the activity"
    );

    sim.volunteer_for_activity(activity_id, elves[1], &mut events);
    sim.volunteer_for_activity(activity_id, elves[2], &mut events);

    // 3 volunteers — quorum should fire.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
        "3 volunteers should trigger quorum"
    );
}

// ===========================================================================
// Third-pass corner-case and guard-clause tests
// ===========================================================================

#[test]
fn incapacitated_creature_removed_from_executing_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );

    // Incapacitate elf 0 (not death — HP reaches 0).
    sim.handle_creature_incapacitation(elves[0], &mut events);

    // Elf 0 should be removed from the activity.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_none(),
        "incapacitated creature should be removed from activity"
    );
    // Activity should still be Executing (Continue policy, 2 remain).
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );
}

#[test]
fn last_participant_removed_from_paused_activity_cancels_it() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(2),
        Some(2),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim
        .db
        .activities
        .iter_all()
        .find(|a| a.kind == ActivityKind::ConstructionChoir)
        .unwrap()
        .id;

    for eid in &elves[..2] {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    sim.handle_assign_to_activity(activity_id, elves[0], &mut events);
    sim.handle_assign_to_activity(activity_id, elves[1], &mut events);
    sim.on_activity_participant_arrived(activity_id, elves[0], &mut events);
    sim.on_activity_participant_arrived(activity_id, elves[1], &mut events);

    // Remove one → PauseAndWait → Paused.
    sim.remove_participant(activity_id, elves[0], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Paused
    );

    // Remove the last one → should cancel, not leave an orphan.
    sim.remove_participant(activity_id, elves[1], &mut events);
    assert!(
        sim.db.activities.get(&activity_id).is_none(),
        "removing last participant from Paused activity should cancel it"
    );
}

#[test]
fn volunteer_for_nonexistent_activity_is_noop() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let bogus_id = ActivityId::new(&mut sim.rng);
    let mut events = Vec::new();
    sim.volunteer_for_activity(bogus_id, elves[0], &mut events);
    // No panic, no participant rows.
    assert!(
        sim.db
            .activity_participants
            .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn cancel_nonexistent_activity_is_noop() {
    let mut sim = test_sim(legacy_test_seed());
    let bogus_id = ActivityId::new(&mut sim.rng);
    let mut events = Vec::new();
    // Should not panic.
    sim.cancel_activity(bogus_id, &mut events);
}

#[test]
fn remove_nonexistent_participant_is_noop() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    // Remove a creature that isn't a participant — should not panic.
    sim.remove_participant(activity_id, elves[0], &mut events);

    // Activity should be unaffected.
    assert!(sim.db.activities.get(&activity_id).is_some());
}

#[test]
fn two_creatures_arrive_same_tick() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(2),
        Some(3),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim
        .db
        .activities
        .iter_all()
        .find(|a| a.kind == ActivityKind::Dance)
        .unwrap()
        .id;

    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }

    // 2 arrive simultaneously (same tick). min_count=2 so this should trigger Executing.
    sim.on_activity_participant_arrived(activity_id, elves[0], &mut events);
    sim.on_activity_participant_arrived(activity_id, elves[1], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );

    // 3rd arrival on same tick — also fine, already Executing.
    sim.on_activity_participant_arrived(activity_id, elves[2], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );
}

#[test]
fn pause_timeout_not_expired_keeps_activity() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim
        .db
        .activities
        .iter_all()
        .find(|a| a.kind == ActivityKind::ConstructionChoir)
        .unwrap()
        .id;

    for eid in &elves[..3] {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    for i in 0..3 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Remove one → Paused.
    sim.remove_participant(activity_id, elves[0], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Paused
    );

    // Check timeout 1 tick later (way before timeout expires).
    sim.tick += 1;
    sim.check_activity_pause_timeout(activity_id, &mut events);

    // Activity should still be Paused, not cancelled.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Paused,
        "activity should not be cancelled before timeout expires"
    );
}

#[test]
fn dead_volunteer_pruned_by_quorum_check() {
    // A Volunteered creature dies (current_activity is None, so death handler
    // doesn't call remove_participant). The stale row should be pruned when
    // the next quorum check fires.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    sim.volunteer_for_activity(activity_id, elves[1], &mut events);

    // Kill elf 0 (Volunteered, no current_activity — death handler won't remove).
    sim.handle_creature_death(elves[0], crate::types::DeathCause::Debug, &mut events);

    // Stale row should still exist.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_some()
    );

    // 3rd volunteer triggers quorum check which prunes the dead volunteer.
    sim.volunteer_for_activity(activity_id, elves[2], &mut events);

    // Dead volunteer should be pruned.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_none(),
        "dead volunteer should be pruned by quorum check"
    );

    // Only 2 valid volunteers remain (1, 2) — still below min_count=3.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting
    );
}

#[test]
fn assign_to_executing_phase_rejected() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );

    // Try to direct-assign elf 3 to an Executing activity — should be rejected.
    sim.handle_assign_to_activity(activity_id, elves[3], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[3]))
            .is_none()
    );
}

#[test]
fn activity_serde_roundtrip_preserves_executing_state() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Run a couple activations — dance behavior should execute without error.
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);
    sim.execute_activity_behavior(elves[1], activity_id, &mut events);

    // A dance plan should have been generated.
    assert!(sim.db.activity_dance_data.get(&activity_id).is_some());

    // Roundtrip.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    // Activity preserved.
    let activity = restored.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.phase, ActivityPhase::Executing);
    assert_eq!(activity.kind, ActivityKind::Dance);

    // Dance data preserved.
    assert!(restored.db.activity_dance_data.get(&activity_id).is_some());

    // Participants preserved.
    for i in 0..3 {
        let p = restored
            .db
            .activity_participants
            .get(&(activity_id, elves[i]))
            .unwrap();
        assert_eq!(p.status, ParticipantStatus::Arrived);

        let creature = restored.db.creatures.get(&elves[i]).unwrap();
        assert_eq!(creature.current_activity, Some(activity_id));
    }
}

#[test]
fn activity_config_serde_backward_compat() {
    // Deserializing a GameConfig JSON without the "activity" key should
    // succeed, filling in defaults via serde(default).
    let config = test_config();
    let json = serde_json::to_string(&config).unwrap();

    // Strip the "activity" key from the JSON to simulate an old save.
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value.as_object_mut().unwrap().remove("activity");
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: crate::config::GameConfig = serde_json::from_str(&stripped).unwrap();
    // Should get defaults.
    assert_eq!(restored.activity.volunteer_search_radius, 30);
    assert_eq!(restored.activity.assembly_timeout_ticks, 300_000);
    assert_eq!(restored.activity.pause_timeout_ticks, 60_000);
}

#[test]
fn volunteer_rejects_non_recruiting_phase() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Promote to Assembling via 3 volunteers.
    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // A 4th elf tries to volunteer for the now-Assembling activity.
    sim.volunteer_for_activity(activity_id, elves[3], &mut events);

    // Should be rejected — no participant row created.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[3]))
            .is_none()
    );
}

#[test]
fn desired_count_caps_volunteer_discovery() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Volunteer 3 elves so desired_count is reached. But make quorum fail
    // by giving elf[0] a task before the 3rd volunteer.
    let mut events = Vec::new();
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    sim.volunteer_for_activity(activity_id, elves[1], &mut events);
    // Give elf[0] a task before 3rd volunteer so quorum won't fire.
    let fake_task_id = TaskId::new(&mut sim.rng);
    let task = crate::db::Task {
        id: fake_task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::InProgress,
        origin: TaskOrigin::Automated,
        location,
        progress: 0,
        total_cost: 0,
        required_species: None,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.db.insert_task(task).unwrap();
    let mut c0 = sim.db.creatures.get(&elves[0]).unwrap();
    c0.current_task = Some(fake_task_id);
    sim.db.update_creature(c0).unwrap();

    sim.volunteer_for_activity(activity_id, elves[2], &mut events);
    // Elf[0] pruned, 2 remain (elf[1], elf[2]). Still Recruiting.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting
    );

    // Now volunteer elf[3] — total 3 volunteers (elf[1], elf[2], elf[3]).
    sim.volunteer_for_activity(activity_id, elves[3], &mut events);
    // Quorum should trigger, promoting to Assembling.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // Now elf[4] tries to discover the activity — it's no longer Recruiting.
    let found = sim.find_open_activity_for_creature(elves[4]);
    assert!(
        found.is_none(),
        "should not discover non-Recruiting activity"
    );
}

#[test]
fn cancel_recruiting_activity_with_only_volunteers() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Add 2 volunteers (below quorum).
    let mut events = Vec::new();
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    sim.volunteer_for_activity(activity_id, elves[1], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting
    );

    // Cancel.
    sim.cancel_activity(activity_id, &mut events);

    // Activity gone.
    assert!(sim.db.activities.get(&activity_id).is_none());
    // Volunteer rows gone (cascade delete).
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_none()
    );
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[1]))
            .is_none()
    );
    // Creatures unaffected — they never had current_activity set.
    assert!(
        sim.db
            .creatures
            .get(&elves[0])
            .unwrap()
            .current_activity
            .is_none()
    );
    assert!(
        sim.db
            .creatures
            .get(&elves[1])
            .unwrap()
            .current_activity
            .is_none()
    );
}

#[test]
fn directed_recruitment_three_assigned_transitions_to_assembling() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting
    );

    let elves = spawn_test_elves(&mut sim, 5);
    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Assign 2 — still Recruiting.
    sim.handle_assign_to_activity(activity_id, elves[0], &mut events);
    sim.handle_assign_to_activity(activity_id, elves[1], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Recruiting
    );

    // Assign 3rd — should transition to Assembling.
    sim.handle_assign_to_activity(activity_id, elves[2], &mut events);
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // All 3 should be Traveling.
    for i in 0..3 {
        let p = sim
            .db
            .activity_participants
            .get(&(activity_id, elves[i]))
            .unwrap();
        assert_eq!(p.status, ParticipantStatus::Traveling);
    }
}

// ---------------------------------------------------------------------------
// Activity civ/species eligibility tests
// ---------------------------------------------------------------------------

#[test]
fn wild_capybara_cannot_volunteer_for_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let activity_id = create_debug_dance(&mut sim, tree_pos);

    // Spawn a wild capybara (no civ) at the activity location.
    let mut events = Vec::new();
    let capybara_id = sim
        .spawn_creature(Species::Capybara, tree_pos, sim.home_zone_id(), &mut events)
        .expect("should spawn capybara");
    let mut c = sim.db.creatures.get(&capybara_id).unwrap();
    c.current_task = None;
    sim.db.update_creature(c).unwrap();

    // Capybara should NOT be able to volunteer — wrong species + no civ.
    sim.volunteer_for_activity(activity_id, capybara_id, &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, capybara_id))
            .is_none(),
        "wild capybara should not be accepted into a dance"
    );
}

#[test]
fn other_civ_elf_cannot_volunteer_for_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let activity_id = create_debug_dance(&mut sim, tree_pos);

    // Create an elf belonging to a different civ.
    let hostile_civ = ensure_hostile_civ(&mut sim);
    let mut events = Vec::new();
    let foreign_elf_id = sim
        .spawn_creature_with_civ(
            Species::Elf,
            tree_pos,
            Some(hostile_civ),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn foreign elf");
    let mut c = sim.db.creatures.get(&foreign_elf_id).unwrap();
    c.current_task = None;
    sim.db.update_creature(c).unwrap();

    // Foreign elf should NOT be able to volunteer — wrong civ.
    sim.volunteer_for_activity(activity_id, foreign_elf_id, &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, foreign_elf_id))
            .is_none(),
        "elf from another civ should not be accepted into a dance"
    );
}

#[test]
fn same_civ_capybara_cannot_volunteer_for_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let activity_id = create_debug_dance(&mut sim, tree_pos);

    // Spawn a capybara and give it the player's civ (contrived but tests species filter).
    let player_civ = sim.player_civ_id;
    let mut events = Vec::new();
    let capybara_id = sim
        .spawn_creature_with_civ(
            Species::Capybara,
            tree_pos,
            player_civ,
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn same-civ capybara");
    let mut c = sim.db.creatures.get(&capybara_id).unwrap();
    c.current_task = None;
    sim.db.update_creature(c).unwrap();

    // Capybara should NOT be able to volunteer — wrong species even though right civ.
    sim.volunteer_for_activity(activity_id, capybara_id, &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, capybara_id))
            .is_none(),
        "capybara should not be accepted into a dance even with the right civ"
    );
}

#[test]
fn find_open_activity_ignores_wrong_species() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let _activity_id = create_debug_dance(&mut sim, tree_pos);

    // Spawn a capybara at the activity location.
    let mut events = Vec::new();
    let capybara_id = sim
        .spawn_creature(Species::Capybara, tree_pos, sim.home_zone_id(), &mut events)
        .expect("should spawn capybara");
    let mut c = sim.db.creatures.get(&capybara_id).unwrap();
    c.current_task = None;
    sim.db.update_creature(c).unwrap();

    // Capybara should not discover the dance.
    let found = sim.find_open_activity_for_creature(capybara_id);
    assert!(found.is_none(), "capybara should not discover elf dance");
}

#[test]
fn find_open_activity_ignores_wrong_civ() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let _activity_id = create_debug_dance(&mut sim, tree_pos);

    // Create an elf from a different civ.
    let hostile_civ = ensure_hostile_civ(&mut sim);
    let mut events = Vec::new();
    let foreign_elf_id = sim
        .spawn_creature_with_civ(
            Species::Elf,
            tree_pos,
            Some(hostile_civ),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn foreign elf");
    let mut c = sim.db.creatures.get(&foreign_elf_id).unwrap();
    c.current_task = None;
    sim.db.update_creature(c).unwrap();

    // Foreign elf should not discover the dance.
    let found = sim.find_open_activity_for_creature(foreign_elf_id);
    assert!(found.is_none(), "foreign elf should not discover our dance");
}

#[test]
fn activity_civ_and_species_survive_serde_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let activity_id = create_debug_dance(&mut sim, tree_pos);

    let activity = sim.db.activities.get(&activity_id).unwrap();
    // Debug dance should have player's civ and Elf species requirement.
    assert_eq!(activity.civ_id, sim.player_civ_id);
    assert_eq!(activity.required_species, Some(Species::Elf));

    // Roundtrip.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let activity = restored.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.civ_id, sim.player_civ_id);
    assert_eq!(activity.required_species, Some(Species::Elf));
}

#[test]
fn directed_assign_rejects_wrong_species() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let activity_id = create_debug_dance(&mut sim, tree_pos);

    // Spawn a capybara in the player's civ.
    let player_civ = sim.player_civ_id;
    let mut events = Vec::new();
    let capybara_id = sim
        .spawn_creature_with_civ(
            Species::Capybara,
            tree_pos,
            player_civ,
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn capybara");
    let mut c = sim.db.creatures.get(&capybara_id).unwrap();
    c.current_task = None;
    sim.db.update_creature(c).unwrap();

    // Directed assign should be rejected — wrong species.
    sim.handle_assign_to_activity(activity_id, capybara_id, &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, capybara_id))
            .is_none(),
        "capybara should not be directed-assigned to a dance"
    );
}

#[test]
fn directed_assign_rejects_wrong_civ() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let activity_id = create_debug_dance(&mut sim, tree_pos);

    let hostile_civ = ensure_hostile_civ(&mut sim);
    let mut events = Vec::new();
    let foreign_elf_id = sim
        .spawn_creature_with_civ(
            Species::Elf,
            tree_pos,
            Some(hostile_civ),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn foreign elf");
    let mut c = sim.db.creatures.get(&foreign_elf_id).unwrap();
    c.current_task = None;
    sim.db.update_creature(c).unwrap();

    // Directed assign should be rejected — wrong civ.
    sim.handle_assign_to_activity(activity_id, foreign_elf_id, &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, foreign_elf_id))
            .is_none(),
        "foreign elf should not be directed-assigned to a dance"
    );
}

#[test]
fn dead_creature_cannot_volunteer() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let activity_id = create_debug_dance(&mut sim, tree_pos);
    let elves = spawn_test_elves(&mut sim, 3);

    // Kill elf[0].
    let mut c0 = sim.db.creatures.get(&elves[0]).unwrap();
    c0.vital_status = VitalStatus::Dead;
    c0.current_task = None;
    sim.db.update_creature(c0).unwrap();

    let mut events = Vec::new();
    sim.volunteer_for_activity(activity_id, elves[0], &mut events);
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[0]))
            .is_none(),
        "dead creature should not be able to volunteer"
    );
}

#[test]
fn activity_goto_inherits_player_directed_origin() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Volunteer 3 to trigger quorum → Assembling with GoTo tasks.
    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling
    );

    // The debug dance was created with PlayerDirected origin.
    // GoTo tasks should inherit that origin.
    for i in 0..3 {
        let creature = sim.db.creatures.get(&elves[i]).unwrap();
        let task_id = creature.current_task.expect("should have GoTo task");
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(
            task.origin,
            TaskOrigin::PlayerDirected,
            "GoTo task for elf {i} should inherit PlayerDirected origin from activity"
        );
    }
}

// -----------------------------------------------------------------------
// StartDebugDance integration tests
// -----------------------------------------------------------------------

#[test]
fn start_debug_dance_no_hall_does_nothing() {
    let mut sim = test_sim(legacy_test_seed());

    // No dance hall exists — StartDebugDance should be a no-op.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::StartDebugDance,
    };
    sim.step(&[cmd], sim.tick + 1);

    // No activity should have been created.
    let dances: Vec<_> = sim
        .db
        .activities
        .iter_all()
        .filter(|a| a.kind == ActivityKind::Dance)
        .collect();
    assert!(
        dances.is_empty(),
        "no dance activity should be created without a dance hall"
    );
}

#[test]
fn start_debug_dance_creates_activity_linked_to_hall() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Furnish as a dance hall.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::DanceHall,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Now start a debug dance.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::StartDebugDance,
    };
    sim.step(&[cmd2], sim.tick + 1);

    // A dance activity should exist.
    let dances: Vec<_> = sim
        .db
        .activities
        .iter_all()
        .filter(|a| a.kind == ActivityKind::Dance)
        .collect();
    assert_eq!(dances.len(), 1);
    let activity_id = dances[0].id;

    // ActivityStructureRef should link it to the dance hall.
    let refs = sim
        .db
        .activity_structure_refs
        .by_activity_id(&activity_id, tabulosity::QueryOpts::ASC);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].structure_id, structure_id);
    assert_eq!(refs[0].role, crate::db::ActivityStructureRole::DanceVenue);
}

#[test]
fn dance_completion_cleans_up_dance_data() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Verify dance data exists with a composition.
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let comp_id = dance_data
        .composition_id
        .expect("dance should have a music composition");
    assert!(
        sim.db.music_compositions.get(&comp_id).is_some(),
        "composition should exist in the DB"
    );

    // Advance past plan duration and trigger completion.
    let plan_total = sim
        .db
        .activity_dance_data
        .get(&activity_id)
        .unwrap()
        .plan
        .total_ticks;
    let execution_start = sim
        .db
        .activities
        .get(&activity_id)
        .unwrap()
        .execution_start_tick
        .unwrap();
    sim.tick = execution_start + plan_total;
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // Activity and dance data should both be cleaned up via cascade delete.
    assert!(sim.db.activities.get(&activity_id).is_none());
    assert!(
        sim.db.activity_dance_data.get(&activity_id).is_none(),
        "ActivityDanceData should be cascade-deleted with the activity"
    );
    assert!(
        sim.db.music_compositions.get(&comp_id).is_none(),
        "music composition should be cleaned up when dance completes"
    );
}

#[test]
fn start_debug_dance_command_serde_roundtrip() {
    let cmd = SimCommand {
        player_name: "test".to_string(),
        tick: 42,
        action: SimAction::StartDebugDance,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());
}

// ---------------------------------------------------------------------------
// Backward compat serde tests
// ---------------------------------------------------------------------------

#[test]
fn activity_participant_dance_fields_backward_compat() {
    // Deserializing an ActivityParticipant without dance_slot/waypoint_cursor
    // (old save format) should succeed with defaults.
    let json = r#"{"activity_id":"00000000-0000-0000-0000-000000000001","creature_id":"00000000-0000-0000-0000-000000000002","role":"Member","status":"Arrived","assigned_position":{"x":10,"y":51,"z":20},"travel_task":null}"#;
    let p: crate::db::ActivityParticipant = serde_json::from_str(json).unwrap();
    assert_eq!(p.dance_slot, None);
    assert_eq!(p.waypoint_cursor, 0);
}

#[test]
fn activity_structure_role_serde_roundtrip() {
    let role = crate::db::ActivityStructureRole::DanceVenue;
    let json = serde_json::to_string(&role).unwrap();
    let restored: crate::db::ActivityStructureRole = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, role);
}

#[test]
fn cancel_executing_dance_cleans_up_plan_and_composition() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Dance should be Executing with a plan and composition.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing
    );
    let comp_id = sim
        .db
        .activity_dance_data
        .get(&activity_id)
        .unwrap()
        .composition_id
        .expect("should have composition");
    assert!(sim.db.music_compositions.get(&comp_id).is_some());

    // Cancel the activity while executing.
    sim.cancel_activity(activity_id, &mut events);

    // Everything should be cleaned up.
    assert!(sim.db.activities.get(&activity_id).is_none());
    assert!(sim.db.activity_dance_data.get(&activity_id).is_none());
    assert!(
        sim.db.music_compositions.get(&comp_id).is_none(),
        "composition should be removed on cancel"
    );
}

#[test]
fn dance_config_backward_compat_dance_duration_secs() {
    // Deserializing an ActivityConfig without dance_duration_secs should
    // use the default value (24.0).
    let config = test_config();
    let json = serde_json::to_string(&config.activity).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value.as_object_mut().unwrap().remove("dance_duration_secs");
    let stripped = serde_json::to_string(&value).unwrap();
    let restored: crate::config::ActivityConfig = serde_json::from_str(&stripped).unwrap();
    assert!(
        (restored.dance_duration_secs - 24.0).abs() < 0.01,
        "dance_duration_secs should default to 24.0, got {}",
        restored.dance_duration_secs
    );
}

#[test]
fn serde_roundtrip_preserves_dance_slot_and_cursor() {
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);
    let activity_id = create_debug_dance(&mut sim, location);
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Run one dance activation to advance the cursor.
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);
    let p_before = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[0]))
        .unwrap();
    let slot_before = p_before.dance_slot;
    let cursor_before = p_before.waypoint_cursor;
    assert!(slot_before.is_some());
    assert!(cursor_before > 0, "cursor should have advanced");

    // Roundtrip.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let p_after = restored
        .db
        .activity_participants
        .get(&(activity_id, elves[0]))
        .unwrap();
    assert_eq!(p_after.dance_slot, slot_before);
    assert_eq!(p_after.waypoint_cursor, cursor_before);
}

// F-dance-self-org: Venue exclusivity
// ---------------------------------------------------------------------------

#[test]
fn debug_dance_blocked_when_hall_has_active_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Furnish as a dance hall.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::DanceHall,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Start first debug dance — should succeed.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::StartDebugDance,
    };
    sim.step(&[cmd1], sim.tick + 1);

    let dance_count_1 = sim
        .db
        .activities
        .iter_all()
        .filter(|a| a.kind == ActivityKind::Dance)
        .count();
    assert_eq!(dance_count_1, 1, "first debug dance should be created");

    // Start second debug dance on the same hall — should be blocked.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::StartDebugDance,
    };
    sim.step(&[cmd2], sim.tick + 1);

    let dance_count_2 = sim
        .db
        .activities
        .iter_all()
        .filter(|a| a.kind == ActivityKind::Dance)
        .count();
    assert_eq!(
        dance_count_2, 1,
        "second debug dance should be blocked — hall already has an active dance"
    );
}

// ---------------------------------------------------------------------------
// F-dance-self-org: Spontaneous dance organization
// ---------------------------------------------------------------------------

#[test]
fn idle_elf_near_dance_hall_organizes_spontaneous_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    // Set organize chance to 100% so it always triggers.
    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    // Zero cooldowns for this test.
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    // Spawn an elf near the dance hall.
    let hall = sim.db.structures.get(&structure_id).unwrap().clone();
    let elf_pos = hall.anchor;
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, elf_pos, sim.home_zone_id(), &mut events)
        .unwrap();

    // Make the elf idle (no task, no activity).
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    // Try to organize a spontaneous dance.
    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);

    assert!(organized, "idle elf near hall should organize a dance");

    // A dance activity should exist with Autonomous origin.
    let dances: Vec<_> = sim
        .db
        .activities
        .iter_all()
        .filter(|a| a.kind == ActivityKind::Dance)
        .collect();
    assert_eq!(dances.len(), 1);
    assert_eq!(dances[0].origin, TaskOrigin::Autonomous);

    // The elf should be the organizer participant.
    let participant = sim
        .db
        .activity_participants
        .get(&(dances[0].id, elf_id))
        .expect("organizer should be a participant");
    assert_eq!(participant.role, ParticipantRole::Organizer);
}

#[test]
fn hall_cooldown_prevents_spontaneous_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 100_000;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    // Mark the hall as having had a recent dance.
    let mut s = sim.db.structures.get(&structure_id).unwrap();
    s.last_dance_completed_tick = sim.tick;
    sim.db.update_structure(s).unwrap();

    let hall = sim.db.structures.get(&structure_id).unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(
        !organized,
        "hall cooldown should prevent organizing a dance"
    );
}

#[test]
fn elf_cooldown_prevents_spontaneous_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let _structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 100_000;

    let hall = sim.db.structures.iter_all().next().unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        c.last_dance_tick = sim.tick; // Just danced.
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(!organized, "elf cooldown should prevent organizing a dance");
}

#[test]
fn first_dance_nudge_skips_hall_cooldown() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    // Large hall cooldown that would normally block.
    sim.config.activity.dance_hall_cooldown_ticks = 999_999;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    // last_dance_completed_tick is 0 (default) = never hosted a dance.
    let hall = sim.db.structures.get(&structure_id).unwrap().clone();
    assert_eq!(
        hall.last_dance_completed_tick, 0,
        "new hall should have no dance history"
    );

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(
        organized,
        "first-dance nudge should skip hall cooldown for new halls"
    );
}

#[test]
fn venue_exclusivity_blocks_spontaneous_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    // Start a debug dance (occupies the hall).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::StartDebugDance,
    };
    sim.step(&[cmd], sim.tick + 1);

    assert!(
        sim.hall_has_active_dance(structure_id),
        "hall should have an active dance"
    );

    let hall = sim.db.structures.get(&structure_id).unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(
        !organized,
        "venue exclusivity should block spontaneous dance when hall is occupied"
    );
}

#[test]
fn dance_completion_sets_tracking_ticks() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);
    let elves = spawn_test_elves(&mut sim, 5);

    // Start a debug dance linked to the hall.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::StartDebugDance,
    };
    sim.step(&[cmd], sim.tick + 1);

    let activity_id = sim
        .db
        .activities
        .iter_all()
        .find(|a| a.kind == ActivityKind::Dance)
        .unwrap()
        .id;

    // Clear tasks so elves can volunteer.
    for eid in &elves {
        if let Some(mut c) = sim.db.creatures.get(eid) {
            c.current_task = None;
            sim.db.update_creature(c).unwrap();
        }
    }

    // Volunteer and arrive 3 elves.
    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Fast-forward to after dance completion.
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let exec_start = sim
        .db
        .activities
        .get(&activity_id)
        .unwrap()
        .execution_start_tick
        .unwrap();
    let end_tick = exec_start + dance_data.plan.total_ticks + 1;
    sim.tick = end_tick;

    // Trigger completion by executing the dance behavior for one participant.
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // Activity should be completed (deleted).
    assert!(
        sim.db.activities.get(&activity_id).is_none(),
        "activity should be cleaned up after completion"
    );

    // Structure should have last_dance_completed_tick set.
    let hall = sim.db.structures.get(&structure_id).unwrap();
    assert_eq!(
        hall.last_dance_completed_tick, end_tick,
        "hall should record when last dance completed"
    );

    // Participating elves should have last_dance_tick set.
    for i in 0..3 {
        let creature = sim.db.creatures.get(&elves[i]).unwrap();
        assert_eq!(
            creature.last_dance_tick, end_tick,
            "elf {} should record when they last danced",
            i
        );
    }
}

#[test]
fn zero_chance_prevents_spontaneous_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let _structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 0;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    let hall = sim.db.structures.iter_all().next().unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    // Try many times — with 0% chance, none should succeed.
    let mut any_organized = false;
    for _ in 0..100 {
        if sim.try_organize_spontaneous_dance(elf_id, &mut events) {
            any_organized = true;
            break;
        }
    }
    assert!(
        !any_organized,
        "zero organize chance should never trigger a dance"
    );
}

#[test]
fn non_elf_cannot_organize_spontaneous_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let _structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    let hall = sim.db.structures.iter_all().next().unwrap().clone();
    let mut events = Vec::new();
    // Spawn a capybara instead of an elf.
    let capybara_id = sim
        .spawn_creature(
            Species::Capybara,
            hall.anchor,
            sim.home_zone_id(),
            &mut events,
        )
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&capybara_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(capybara_id, &mut events);
    assert!(!organized, "non-elf creatures should not organize dances");
}

#[test]
fn busy_elf_cannot_organize_spontaneous_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let _structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    let hall = sim.db.structures.iter_all().next().unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();

    // Elf has a task — should not organize.
    // Give it a dummy task reference via the sim's RNG.
    let dummy_task_id = crate::types::TaskId::new(&mut sim.rng);
    insert_stub_task(&mut sim, dummy_task_id);
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(dummy_task_id);
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(
        !organized,
        "elf with a current task should not organize a dance"
    );
}

#[test]
fn serde_roundtrip_preserves_last_dance_tick() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 1);
    let elf_id = elves[0];

    // Set last_dance_tick.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.last_dance_tick = 12345;
    sim.db.update_creature(c).unwrap();

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let creature = restored.db.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.last_dance_tick, 12345);
}

#[test]
fn serde_roundtrip_preserves_last_dance_completed_tick() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    let mut s = sim.db.structures.get(&structure_id).unwrap();
    s.last_dance_completed_tick = 67890;
    sim.db.update_structure(s).unwrap();

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let structure = restored.db.structures.get(&structure_id).unwrap();
    assert_eq!(structure.last_dance_completed_tick, 67890);
}

#[test]
fn spontaneous_dance_config_backward_compat() {
    // Old saves without the new config fields should deserialize with defaults.
    let json = r#"{
        "assembly_timeout_ticks": 300000,
        "volunteer_search_radius": 30,
        "pause_timeout_ticks": 60000
    }"#;
    let config: crate::config::ActivityConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.dance_hall_cooldown_ticks, 300_000);
    assert_eq!(config.dance_elf_cooldown_ticks, 180_000);
    assert_eq!(config.dance_organize_chance_ppm, 20_000);
}

#[test]
fn organizer_retains_role_after_reactivation() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    let hall = sim.db.structures.get(&structure_id).unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(organized);

    let activity_id = sim
        .db
        .activities
        .iter_all()
        .find(|a| a.kind == ActivityKind::Dance)
        .unwrap()
        .id;

    // Simulate what the activation loop does: prune stale rows.
    sim.prune_stale_volunteer_rows(elf_id);

    // The organizer's participant row should survive pruning.
    let participant = sim
        .db
        .activity_participants
        .get(&(activity_id, elf_id))
        .expect("organizer participant row should survive pruning");
    assert_eq!(
        participant.role,
        ParticipantRole::Organizer,
        "organizer role should be preserved"
    );
}

#[test]
fn dead_elf_cannot_organize_spontaneous_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let _structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    let hall = sim.db.structures.iter_all().next().unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();

    // Kill the elf.
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.vital_status = VitalStatus::Dead;
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(!organized, "dead elf should not organize a dance");
}

#[test]
fn elf_with_current_activity_cannot_organize() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    let hall = sim.db.structures.get(&structure_id).unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();

    // Elf is committed to some activity.
    let dummy_activity_id = ActivityId::new(&mut sim.rng);
    sim.db
        .insert_activity(crate::db::Activity {
            id: dummy_activity_id,
            zone_id: sim.home_zone_id(),
            kind: ActivityKind::Dance,
            phase: ActivityPhase::Recruiting,
            location: VoxelCoord::new(0, 0, 0),
            min_count: None,
            desired_count: None,
            progress: 0,
            total_cost: 0,
            origin: TaskOrigin::Automated,
            recruitment: RecruitmentMode::Open,
            departure_policy: DeparturePolicy::Continue,
            allows_late_join: false,
            civ_id: None,
            required_species: None,
            execution_start_tick: None,
            pause_started_tick: None,
            assembly_started_tick: None,
        })
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = Some(dummy_activity_id);
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(
        !organized,
        "elf with current_activity should not organize a dance"
    );
}

#[test]
fn elf_already_volunteer_cannot_organize() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    let hall = sim.db.structures.get(&structure_id).unwrap().clone();
    let location = hall.anchor;
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, location, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    // Create an existing activity and make the elf a volunteer.
    let activity_id = sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(3),
        Some(6),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut events,
    );
    let participant = crate::db::ActivityParticipant {
        activity_id,
        creature_id: elf_id,
        role: ParticipantRole::Member,
        status: ParticipantStatus::Volunteered,
        assigned_position: location,
        travel_task: None,
        dance_slot: None,
        waypoint_cursor: 0,
        impressions_made: 0,
    };
    sim.db.insert_activity_participant(participant).unwrap();

    // Should not organize because the elf has a participant row.
    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(
        !organized,
        "elf with existing participant row should not organize a new dance"
    );
}

#[test]
fn dance_hall_out_of_range_prevents_organizing() {
    let mut sim = test_sim(legacy_test_seed());
    let _structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;
    sim.config.activity.volunteer_search_radius = 5;

    // Spawn elf near the hall, then move it far away.
    let hall = sim.db.structures.iter_all().next().unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    // Teleport elf far from the hall (beyond search radius).
    let far_pos = VoxelCoord::new(hall.anchor.x + 100, hall.anchor.y, hall.anchor.z + 100);
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.position = VoxelBox::point(far_pos);
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    let organized = sim.try_organize_spontaneous_dance(elf_id, &mut events);
    assert!(
        !organized,
        "elf far from dance hall should not organize a dance"
    );
}

#[test]
fn elf_cooldown_prevents_volunteering_for_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let _structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_elf_cooldown_ticks = 100_000;

    let hall = sim.db.structures.iter_all().next().unwrap().clone();
    let location = hall.anchor;

    // Create a recruiting dance activity.
    let mut events = Vec::new();
    let activity_id = sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(3),
        Some(6),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut events,
    );

    // Spawn elf nearby who recently danced.
    let elf_id = sim
        .spawn_creature(Species::Elf, location, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        c.last_dance_tick = sim.tick; // Just danced.
        sim.db.update_creature(c).unwrap();
    }

    // The elf should not discover this dance activity.
    let found = sim.find_open_activity_for_creature(elf_id);
    assert!(
        found.is_none(),
        "elf on cooldown should not discover dance activities"
    );

    // Advance tick past cooldown — now should discover it.
    sim.tick += 100_001;
    let found_after = sim.find_open_activity_for_creature(elf_id);
    assert_eq!(
        found_after,
        Some(activity_id),
        "elf past cooldown should discover dance activities"
    );
}

#[test]
fn organizer_survives_quorum_prune_when_busy() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    let hall = sim.db.structures.get(&structure_id).unwrap().clone();
    let mut events = Vec::new();
    let organizer_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&organizer_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    // Organizer creates a spontaneous dance.
    let organized = sim.try_organize_spontaneous_dance(organizer_id, &mut events);
    assert!(organized);

    let activity_id = sim
        .db
        .activities
        .iter_all()
        .find(|a| a.kind == ActivityKind::Dance)
        .unwrap()
        .id;

    // Organizer picks up a task (e.g., eating) before quorum is reached.
    let dummy_task_id = crate::types::TaskId::new(&mut sim.rng);
    insert_stub_task(&mut sim, dummy_task_id);
    if let Some(mut c) = sim.db.creatures.get(&organizer_id) {
        c.current_task = Some(dummy_task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Another elf volunteers, triggering quorum check.
    let other_elf = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&other_elf) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }
    sim.volunteer_for_activity(activity_id, other_elf, &mut events);

    // The organizer's participant row should survive the quorum prune.
    let organizer_row = sim
        .db
        .activity_participants
        .get(&(activity_id, organizer_id));
    assert!(
        organizer_row.is_some(),
        "living organizer with a task should survive quorum prune"
    );
    assert_eq!(organizer_row.unwrap().role, ParticipantRole::Organizer);
}

#[test]
fn hall_cooldown_expires_allows_new_dance() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 100;
    sim.config.activity.dance_elf_cooldown_ticks = 0;

    // Mark hall as having had a dance recently.
    let mut s = sim.db.structures.get(&structure_id).unwrap();
    s.last_dance_completed_tick = 1000;
    sim.db.update_structure(s).unwrap();

    let hall = sim.db.structures.get(&structure_id).unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        sim.db.update_creature(c).unwrap();
    }

    // At tick 1099: elapsed=99 < 100, should be blocked.
    sim.tick = 1099;
    assert!(
        !sim.try_organize_spontaneous_dance(elf_id, &mut events),
        "hall cooldown should still block at tick 1099"
    );

    // At tick 1100: elapsed=100 >= 100, should pass.
    sim.tick = 1100;
    assert!(
        sim.try_organize_spontaneous_dance(elf_id, &mut events),
        "hall cooldown should expire at tick 1100"
    );
}

#[test]
fn elf_cooldown_expires_allows_organizing() {
    let mut sim = test_sim(legacy_test_seed());
    let _structure_id = create_dance_hall(&mut sim);

    sim.config.activity.dance_organize_chance_ppm = 1_000_000;
    sim.config.activity.dance_hall_cooldown_ticks = 0;
    sim.config.activity.dance_elf_cooldown_ticks = 100;

    let hall = sim.db.structures.iter_all().next().unwrap().clone();
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, hall.anchor, sim.home_zone_id(), &mut events)
        .unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = None;
        c.current_activity = None;
        c.last_dance_tick = 1000;
        sim.db.update_creature(c).unwrap();
    }

    // At tick 1099: elapsed=99 < 100, should be blocked.
    sim.tick = 1099;
    assert!(
        !sim.try_organize_spontaneous_dance(elf_id, &mut events),
        "elf cooldown should still block at tick 1099"
    );

    // At tick 1100: elapsed=100 >= 100, should pass.
    sim.tick = 1100;
    assert!(
        sim.try_organize_spontaneous_dance(elf_id, &mut events),
        "elf cooldown should expire at tick 1100"
    );
}

// ===========================================================================
// Assembly timeout tests (B-assembly-timeout)
// ===========================================================================

#[test]
fn assembly_started_tick_set_on_phase_transition() {
    // When an activity transitions to Assembling, assembly_started_tick
    // should be set to the current sim tick.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Activity starts in Recruiting.
    let a = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(a.phase, ActivityPhase::Recruiting);
    assert_eq!(a.assembly_started_tick, None);

    // Transition to Assembling by reaching quorum.
    sim.tick = 5000;
    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }

    let a = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(a.phase, ActivityPhase::Assembling);
    assert_eq!(a.assembly_started_tick, Some(5000));
}

#[test]
fn assembly_timeout_cancels_below_min_count() {
    // If assembly times out with fewer than min_count arrivals, cancel.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    sim.config.activity.assembly_timeout_ticks = 100;
    sim.tick = 1000;
    let mut events = Vec::new();

    // 3 volunteers → Assembling phase.
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
    );

    // Only 1 participant arrives (below min_count=3).
    sim.on_activity_participant_arrived(activity_id, elves[0], &mut events);

    // Advance past timeout.
    sim.tick = 1101;
    sim.check_activity_assembly_timeout(activity_id, &mut events);

    // Activity should be cancelled.
    assert!(
        sim.db.activities.get(&activity_id).is_none(),
        "activity should be cancelled when assembly times out below min_count"
    );
}

#[test]
fn assembly_timeout_starts_at_min_count_kicks_stragglers() {
    // If assembly times out with arrived >= min_count, start the activity
    // and kick non-arrived participants (when allows_late_join = false).
    //
    // We use directed recruitment with min_count=3, desired=4. Three of
    // four participants arrive — check_assembly_complete fires and transitions
    // to Executing. But the 4th is still Traveling (stuck). We set up the
    // state so check_assembly_complete did NOT fire (simulating a stuck
    // participant scenario where arrivals == min but the check was missed
    // due to ordering).
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Create a non-late-join activity (ConstructionChoir).
    let activity_id = sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(4),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut Vec::new(),
    );
    assert!(
        !sim.db
            .activities
            .get(&activity_id)
            .unwrap()
            .allows_late_join
    );

    sim.config.activity.assembly_timeout_ticks = 100;
    sim.tick = 1000;
    let mut events = Vec::new();

    // Assign 4 participants via directed recruitment.
    for i in 0..4 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
    );

    // Manually mark 3 participants as Arrived (simulating arrival without
    // triggering check_assembly_complete, e.g., they reached the position
    // but check was skipped because they were stuck on an adjacent voxel).
    for i in 0..3 {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, elves[i])) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
    }

    // Advance past timeout.
    sim.tick = 1101;
    sim.check_activity_assembly_timeout(activity_id, &mut events);

    // Activity should now be Executing.
    let a = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(a.phase, ActivityPhase::Executing);

    // The straggler (elves[3], still Traveling) should be removed.
    assert!(
        sim.db
            .activity_participants
            .get(&(activity_id, elves[3]))
            .is_none(),
        "straggler should be kicked when allows_late_join=false"
    );

    // The 3 arrived participants should still be there.
    for i in 0..3 {
        assert!(
            sim.db
                .activity_participants
                .get(&(activity_id, elves[i]))
                .is_some()
        );
    }
}

#[test]
fn assembly_timeout_keeps_travelers_when_late_join_allowed() {
    // If assembly times out with arrived >= min_count and allows_late_join=true,
    // start the activity but keep Traveling participants so they can join later.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Create a Dance activity (allows_late_join=true by default) with
    // min_count=3 and desired_count=5.
    let activity_id = sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(3),
        Some(5),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut Vec::new(),
    );
    assert!(
        sim.db
            .activities
            .get(&activity_id)
            .unwrap()
            .allows_late_join
    );

    sim.config.activity.assembly_timeout_ticks = 100;
    sim.tick = 1000;
    let mut events = Vec::new();

    // Assign 5 participants via directed recruitment.
    for i in 0..5 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
    );

    // Manually mark 3 as Arrived (without triggering check_assembly_complete).
    for i in 0..3 {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, elves[i])) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
    }

    // Advance past timeout.
    sim.tick = 1101;
    sim.check_activity_assembly_timeout(activity_id, &mut events);

    // Activity should now be Executing.
    let a = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(a.phase, ActivityPhase::Executing);

    // All 5 participants should still exist (late joiners kept).
    for i in 0..5 {
        assert!(
            sim.db
                .activity_participants
                .get(&(activity_id, elves[i]))
                .is_some(),
            "participant {} should be kept when allows_late_join=true",
            i
        );
    }
}

#[test]
fn assembly_timeout_not_triggered_before_expiry() {
    // Assembly timeout should not fire before the timeout period has elapsed.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    sim.config.activity.assembly_timeout_ticks = 100;
    sim.tick = 1000;
    let mut events = Vec::new();

    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
    );

    // No arrivals. Check just before timeout.
    sim.tick = 1099;
    sim.check_activity_assembly_timeout(activity_id, &mut events);

    // Should still be assembling.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
    );
}

#[test]
fn serde_roundtrip_preserves_assembly_started_tick() {
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    // Manually set assembly_started_tick.
    if let Some(mut a) = sim.db.activities.get(&activity_id) {
        a.assembly_started_tick = Some(42_000);
        sim.db.update_activity(a).unwrap();
    }

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let a = restored.db.activities.get(&activity_id).unwrap();
    assert_eq!(a.assembly_started_tick, Some(42_000));
}

#[test]
fn assembly_timeout_fires_at_exact_boundary() {
    // Timeout should fire when elapsed == assembly_timeout_ticks (not off-by-one).
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    sim.config.activity.assembly_timeout_ticks = 100;
    sim.tick = 1000;
    let mut events = Vec::new();

    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
    );

    // No arrivals. At exactly tick 1100 (elapsed=100, timeout=100), should fire.
    sim.tick = 1100;
    sim.check_activity_assembly_timeout(activity_id, &mut events);

    // Below min_count → cancelled.
    assert!(
        sim.db.activities.get(&activity_id).is_none(),
        "timeout should fire at exact boundary (elapsed == timeout)"
    );
}

#[test]
fn assembly_timeout_sets_execution_start_tick() {
    // When timeout starts execution, execution_start_tick must be set.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_id = sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(3),
        Some(5),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut Vec::new(),
    );

    sim.config.activity.assembly_timeout_ticks = 100;
    sim.tick = 1000;
    let mut events = Vec::new();

    for i in 0..5 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }

    // 3 arrive manually.
    for i in 0..3 {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, elves[i])) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
    }

    sim.tick = 1101;
    sim.check_activity_assembly_timeout(activity_id, &mut events);

    let a = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(a.phase, ActivityPhase::Executing);
    assert_eq!(
        a.execution_start_tick,
        Some(1101),
        "execution_start_tick should be set to the tick when timeout fired"
    );
}

#[test]
fn assembly_timeout_skipped_when_assembly_started_tick_none() {
    // Old saves may have activities in Assembling with assembly_started_tick=None.
    // The timeout check should be a no-op (graceful backward compat).
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    sim.tick = 1000;
    let mut events = Vec::new();
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
    );

    // Simulate old save: clear assembly_started_tick.
    if let Some(mut a) = sim.db.activities.get(&activity_id) {
        a.assembly_started_tick = None;
        sim.db.update_activity(a).unwrap();
    }

    // Advance far past any reasonable timeout.
    sim.config.activity.assembly_timeout_ticks = 1;
    sim.tick = 999_999;
    sim.check_activity_assembly_timeout(activity_id, &mut events);

    // Activity should still be assembling — timeout skipped.
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
        "timeout should be skipped when assembly_started_tick is None"
    );
}

#[test]
fn assembly_timeout_generates_dance_plan() {
    // When a Dance activity starts via timeout, a dance plan should be generated.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_id = sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(3),
        Some(5),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut Vec::new(),
    );

    sim.config.activity.assembly_timeout_ticks = 100;
    sim.tick = 1000;
    let mut events = Vec::new();

    for i in 0..5 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }

    // 3 arrive manually.
    for i in 0..3 {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, elves[i])) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
    }

    // No dance data before timeout.
    assert!(sim.db.activity_dance_data.get(&activity_id).is_none());

    sim.tick = 1101;
    sim.check_activity_assembly_timeout(activity_id, &mut events);

    // Dance plan should be generated.
    assert!(
        sim.db.activity_dance_data.get(&activity_id).is_some(),
        "dance plan should be generated when timeout starts a Dance activity"
    );
}

#[test]
fn assembly_timeout_cancel_no_double_reactivation() {
    // B-erratic-movement: when assembly timeout cancels the activity via the
    // activation loop, cancel_activity schedules reactivation for all
    // participants. The activation loop must NOT add a second one.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;
    let activity_id = create_debug_dance(&mut sim, location);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    sim.config.activity.assembly_timeout_ticks = 100;
    sim.tick = 1000;
    let mut events = Vec::new();

    // 3 volunteers → Assembling, all Traveling.
    for i in 0..3 {
        sim.volunteer_for_activity(activity_id, elves[i], &mut events);
    }
    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Assembling,
    );

    // No arrivals. Advance past timeout.
    sim.tick = 1101;

    // Clear elves[0]'s current_task so it enters the activity-phase branch
    // (the Assembling branch is only reached when current_task is None,
    // i.e., the GoTo was preempted or completed).
    if let Some(mut c) = sim.db.creatures.get(&elves[0]) {
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Clear next_available_tick so we can verify it gets set.
    let mut c = sim.db.creatures.get(&elves[0]).unwrap();
    c.next_available_tick = None;
    sim.db.update_creature(c).unwrap();

    // Trigger activation loop for elves[0] — this will hit the Assembling
    // branch, fire the timeout (arrived=0 < min=3), and cancel the activity.
    sim.process_creature_activation(elves[0], &mut events);

    // Activity should be cancelled.
    assert!(sim.db.activities.get(&activity_id).is_none());

    // elves[0] should have exactly 1 pending activation (next_available_tick is Some).
    assert_eq!(
        sim.count_pending_activations_for(elves[0]),
        1,
        "cancel via assembly timeout should produce exactly 1 reactivation, not 2 (B-erratic-movement)"
    );
}

#[test]
fn assembly_timeout_start_no_double_reactivation() {
    // B-erratic-movement: when assembly timeout starts execution via the
    // activation loop, the creature should get exactly 1 reactivation.
    let mut sim = test_sim(legacy_test_seed());
    let elves = spawn_test_elves(&mut sim, 5);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_id = sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(3),
        Some(5),
        TaskOrigin::PlayerDirected,
        sim.home_zone_id(),
        &mut Vec::new(),
    );

    sim.config.activity.assembly_timeout_ticks = 100;
    sim.tick = 1000;
    let mut events = Vec::new();

    // Assign 5 participants via directed recruitment.
    for i in 0..5 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }

    // 3 arrive manually.
    for i in 0..3 {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, elves[i])) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
    }

    // Advance past timeout.
    sim.tick = 1101;

    // Clear elves[0]'s current_task so it enters the activity-phase branch.
    // (Arrived participants already had travel_task cleared, but current_task
    // may still reference a completed GoTo.)
    if let Some(mut c) = sim.db.creatures.get(&elves[0]) {
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Clear next_available_tick so we can verify it gets set.
    let mut c = sim.db.creatures.get(&elves[0]).unwrap();
    c.next_available_tick = None;
    sim.db.update_creature(c).unwrap();

    // Trigger activation — timeout fires, transitions to Executing.
    sim.process_creature_activation(elves[0], &mut events);

    assert_eq!(
        sim.db.activities.get(&activity_id).unwrap().phase,
        ActivityPhase::Executing,
    );
    assert_eq!(
        sim.count_pending_activations_for(elves[0]),
        1,
        "start via assembly timeout should produce exactly 1 reactivation (B-erratic-movement)"
    );
}

#[test]
fn pause_timeout_cancel_no_double_reactivation() {
    // B-erratic-movement: when pause timeout cancels the activity via the
    // activation loop, cancel_activity schedules reactivation. The Paused
    // branch must NOT add a second one.
    let mut sim = test_sim(legacy_test_seed());
    let location = VoxelCoord::new(32, 1, 32);

    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::ConstructionChoir,
        location,
        Some(3),
        Some(3),
        TaskOrigin::Automated,
        sim.home_zone_id(),
        &mut events,
    );
    let activity_id = sim.db.activities.iter_all().next().unwrap().id;
    let elves = spawn_test_elves(&mut sim, 5);

    for eid in &elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    for i in 0..3 {
        sim.handle_assign_to_activity(activity_id, elves[i], &mut events);
    }
    for i in 0..3 {
        sim.on_activity_participant_arrived(activity_id, elves[i], &mut events);
    }

    // Pause via departure.
    sim.remove_participant(activity_id, elves[0], &mut events);
    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(activity.phase, ActivityPhase::Paused);

    let timeout = match activity.departure_policy {
        DeparturePolicy::PauseAndWait { timeout_ticks } => timeout_ticks,
        _ => panic!("expected PauseAndWait"),
    };

    // Advance past timeout.
    sim.tick += timeout + 1;

    // elves[1] is an Arrived participant still in the activity.
    // Clear next_available_tick so we can verify it gets set.
    let mut c = sim.db.creatures.get(&elves[1]).unwrap();
    c.next_available_tick = None;
    sim.db.update_creature(c).unwrap();

    // Trigger activation — pause timeout fires, cancels the activity.
    sim.process_creature_activation(elves[1], &mut events);

    // Activity should be cancelled.
    assert!(sim.db.activities.get(&activity_id).is_none());

    // elves[1] should have exactly 1 pending activation (next_available_tick is Some).
    assert_eq!(
        sim.count_pending_activations_for(elves[1]),
        1,
        "cancel via pause timeout should produce exactly 1 reactivation, not 2 (B-erratic-movement)"
    );
}
