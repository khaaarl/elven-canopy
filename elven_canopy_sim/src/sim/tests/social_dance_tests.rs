//! Tests for dance ↔ social opinions integration (F-social-dance).
//! Covers: social impression checks during dance execution, Culture skill
//! advancement, dance-specific thoughts (EnjoyedDanceWith, AwkwardDanceWith),
//! and the DancedWithFriend completion bonus.
//!
//! See also: `activity_tests.rs` for general dance lifecycle tests,
//! `social_tests.rs` for the social opinion system, `dinner_party_tests.rs`
//! for the analogous dinner party ↔ social integration.

use super::*;

/// Set up a dance activity in Executing phase with the given elves.
/// Returns the activity ID. All elves are placed at `location`, volunteered,
/// and marked as arrived (which transitions to Executing and generates a plan).
fn setup_executing_dance(
    sim: &mut SimState,
    elves: &[CreatureId],
    location: VoxelCoord,
) -> crate::types::ActivityId {
    let before: std::collections::BTreeSet<_> =
        sim.db.activities.iter_all().map(|a| a.id).collect();
    let mut events = Vec::new();
    sim.handle_create_activity(
        ActivityKind::Dance,
        location,
        Some(elves.len() as u16),
        Some(elves.len() as u16),
        TaskOrigin::PlayerDirected,
        &mut events,
    );
    let activity_id = sim
        .db
        .activities
        .iter_all()
        .map(|a| a.id)
        .find(|id| !before.contains(id))
        .expect("no new activity created");

    // Clear tasks and volunteer + arrive all elves.
    for eid in elves {
        let mut c = sim.db.creatures.get(eid).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }
    for eid in elves {
        sim.volunteer_for_activity(activity_id, *eid, &mut events);
    }
    for eid in elves {
        sim.on_activity_participant_arrived(activity_id, *eid, &mut events);
    }

    // Verify we're in Executing.
    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(
        activity.phase,
        ActivityPhase::Executing,
        "Dance should be in Executing phase"
    );
    activity_id
}

// ---------------------------------------------------------------------------
// Social impression checks during dance execution
// ---------------------------------------------------------------------------

#[test]
fn dance_social_check_upserts_friendliness() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Zero all stats and skills, then set CHA high so impression checks
    // reliably produce non-zero deltas (roll = CHA + Culture + noise(σ=50);
    // with CHA=100 and Culture=0 the roll almost always exceeds +50).
    for &eid in &elves {
        zero_creature_stats(&mut sim, eid);
        set_trait(&mut sim, eid, TraitKind::Charisma, 100);
    }

    let activity_id = setup_executing_dance(&mut sim, &elves, location);

    sim.config.activity.dance_impressions_per_elf = 4;

    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let total_ticks = dance_data.plan.total_ticks;

    // Step through each quarter of the dance to trigger all 4 impressions.
    let mut events = Vec::new();
    for i in 1..=4 {
        sim.tick = execution_start + i * total_ticks / 4;
        sim.execute_activity_behavior(elves[0], activity_id, &mut events);
    }

    // Check that elf 0 has at least one Friendliness opinion toward another dancer.
    let opinions = sim
        .db
        .creature_opinions
        .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC);
    let dance_opinions: Vec<_> = opinions
        .iter()
        .filter(|o| {
            o.kind == OpinionKind::Friendliness
                && (o.target_id == elves[1] || o.target_id == elves[2])
        })
        .collect();

    assert!(
        !dance_opinions.is_empty(),
        "Dancing elf should have formed Friendliness opinions toward co-dancers"
    );
}

#[test]
fn dance_social_check_uses_culture_skill() {
    // Verify that dance social checks advance the Culture skill specifically,
    // not Influence (even if Influence is higher).
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Give elf 0 high Influence, low Culture — dance should still advance Culture.
    zero_creature_stats(&mut sim, elves[0]);
    set_trait(&mut sim, elves[0], TraitKind::Influence, 20);
    set_trait(&mut sim, elves[0], TraitKind::Culture, 1);
    set_trait(&mut sim, elves[0], TraitKind::Charisma, 10);

    let activity_id = setup_executing_dance(&mut sim, &elves, location);

    // Force high skill advancement probability.
    sim.config.social.skill_advance_probability_permille = 1000; // 100%
    sim.config.activity.dance_impressions_per_elf = 20;

    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let total_ticks = dance_data.plan.total_ticks;

    let culture_before = sim.trait_int(elves[0], TraitKind::Culture, 0);
    let influence_before = sim.trait_int(elves[0], TraitKind::Influence, 0);

    // Step through each twentieth of the dance to trigger all 20 impressions.
    let mut events = Vec::new();
    for i in 1..=20 {
        sim.tick = execution_start + i * total_ticks / 20;
        sim.execute_activity_behavior(elves[0], activity_id, &mut events);
    }

    let culture_after = sim.trait_int(elves[0], TraitKind::Culture, 0);
    let influence_after = sim.trait_int(elves[0], TraitKind::Influence, 0);

    assert!(
        culture_after > culture_before,
        "Culture skill should advance during dance (before={culture_before}, after={culture_after})"
    );
    assert_eq!(
        influence_after, influence_before,
        "Influence skill should NOT advance during dance"
    );
}

#[test]
fn dance_social_check_awards_thoughts() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_id = setup_executing_dance(&mut sim, &elves, location);

    // Many impressions to ensure at least one positive or negative.
    sim.config.activity.dance_impressions_per_elf = 20;

    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let total_ticks = dance_data.plan.total_ticks;

    let mut events = Vec::new();
    for i in 1..=20 {
        sim.tick = execution_start + i * total_ticks / 20;
        sim.execute_activity_behavior(elves[0], activity_id, &mut events);
    }

    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC);
    let has_dance_social_thought = thoughts.iter().any(|t| {
        matches!(
            &t.kind,
            ThoughtKind::EnjoyedDanceWith(_) | ThoughtKind::AwkwardDanceWith(_)
        )
    });

    assert!(
        has_dance_social_thought,
        "Dancer should have EnjoyedDanceWith or AwkwardDanceWith thoughts"
    );
}

// ---------------------------------------------------------------------------
// Friend joy bonus on dance completion
// ---------------------------------------------------------------------------

#[test]
fn dance_completion_friend_bonus() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Pre-seed a positive Friendliness opinion so elves[0] considers elves[1]
    // an Acquaintance (>= threshold, default 5).
    sim.upsert_opinion(elves[0], OpinionKind::Friendliness, elves[1], 10);

    let activity_id = setup_executing_dance(&mut sim, &elves, location);

    // Disable in-dance social checks so the pre-seeded opinion is the only
    // one and cannot be diluted by random impression deltas.
    sim.config.activity.dance_impressions_per_elf = 0;

    // Fast-forward past the dance duration to trigger completion.
    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let end_tick = execution_start + dance_data.plan.total_ticks;

    sim.tick = end_tick;
    let mut events = Vec::new();
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // Elf 0 should have the DancedWithFriend thought.
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC);
    assert!(
        thoughts
            .iter()
            .any(|t| t.kind == ThoughtKind::DancedWithFriend),
        "Dancer who danced with a friend should get DancedWithFriend thought"
    );
}

#[test]
fn dance_completion_no_friend_bonus_without_friends() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // No pre-existing opinions — all strangers.
    let activity_id = setup_executing_dance(&mut sim, &elves, location);

    // Disable social checks during dance so no opinions form.
    sim.config.activity.dance_impressions_per_elf = 0;

    // Fast-forward past the dance duration.
    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let end_tick = execution_start + dance_data.plan.total_ticks;

    sim.tick = end_tick;
    let mut events = Vec::new();
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // Elf 0 should NOT have DancedWithFriend.
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC);
    assert!(
        !thoughts
            .iter()
            .any(|t| t.kind == ThoughtKind::DancedWithFriend),
        "Dancer with no friends should not get DancedWithFriend thought"
    );
}

// ---------------------------------------------------------------------------
// Serde roundtrip for new ThoughtKind variants
// ---------------------------------------------------------------------------

#[test]
fn social_dance_thought_kinds_serde_roundtrip() {
    let kinds = [
        ThoughtKind::EnjoyedDanceWith("Aelindra".into()),
        ThoughtKind::AwkwardDanceWith("Thaeron".into()),
        ThoughtKind::DancedWithFriend,
    ];
    for kind in &kinds {
        let json = serde_json::to_string(kind).unwrap();
        let restored: ThoughtKind = serde_json::from_str(&json).unwrap();
        assert_eq!(
            *kind, restored,
            "ThoughtKind serde roundtrip failed for {json}"
        );
    }
}

// ---------------------------------------------------------------------------
// Impression timing (spread across duration)
// ---------------------------------------------------------------------------

#[test]
fn dance_impressions_spread_across_duration() {
    // Verify that impressions are distributed across the dance duration,
    // not all bunched at the start.
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_id = setup_executing_dance(&mut sim, &elves, location);
    sim.config.activity.dance_impressions_per_elf = 2;

    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let total_ticks = dance_data.plan.total_ticks;

    // Activate at 10% into the dance — first impression triggers at 50%, so
    // no impressions should have been made yet.
    sim.tick = execution_start + total_ticks / 10;
    let mut events = Vec::new();
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    let p = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[0]))
        .unwrap();
    assert_eq!(
        p.impressions_made, 0,
        "At 10% into the dance (before 50% threshold), no impressions should fire"
    );

    // Activate at 75% — first impression (at 50%) should have fired, but the
    // second (at 100%) should not.
    sim.tick = execution_start + total_ticks * 3 / 4;
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    let p = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[0]))
        .unwrap();
    assert_eq!(
        p.impressions_made, 1,
        "At 75% into the dance, exactly 1 impression should have fired (threshold at 50%)"
    );
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn dance_social_check_solo_dancer_no_panic() {
    // A solo dancer (only Arrived participant) should skip social checks
    // without panic when there are no other participants to impress.
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_id = setup_executing_dance(&mut sim, &elves, location);
    sim.config.activity.dance_impressions_per_elf = 4;

    // Remove participants 1 and 2 so elf 0 is alone.
    let _ = sim.db.remove_activity_participant(&(activity_id, elves[1]));
    let _ = sim.db.remove_activity_participant(&(activity_id, elves[2]));

    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let total_ticks = dance_data.plan.total_ticks;

    // Step through the dance — should not panic.
    let mut events = Vec::new();
    sim.tick = execution_start + total_ticks / 2;
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // No opinions should have been formed (nobody to dance with).
    let opinions = sim
        .db
        .creature_opinions
        .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC);
    assert!(
        opinions.is_empty(),
        "Solo dancer should not form any opinions"
    );
}

#[test]
fn dance_zero_impressions_completes_cleanly() {
    // With dance_impressions_per_elf = 0, the dance should complete normally
    // without any social checks or opinions.
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_id = setup_executing_dance(&mut sim, &elves, location);
    sim.config.activity.dance_impressions_per_elf = 0;

    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let end_tick = execution_start + dance_data.plan.total_ticks;

    sim.tick = end_tick;
    let mut events = Vec::new();
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // Activity should have completed.
    let activity = sim.db.activities.get(&activity_id);
    assert!(
        activity.is_none()
            || matches!(
                activity.unwrap().phase,
                ActivityPhase::Complete | ActivityPhase::Cancelled
            ),
        "Dance with 0 impressions should complete cleanly"
    );
}

#[test]
fn dance_friend_bonus_is_asymmetric() {
    // Elf A has Acquaintance-level opinion of elf B, but elf B has no opinion
    // of elf A. Only elf A should get DancedWithFriend.
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Only A -> B friendship, not B -> A.
    sim.upsert_opinion(elves[0], OpinionKind::Friendliness, elves[1], 10);

    let activity_id = setup_executing_dance(&mut sim, &elves, location);
    sim.config.activity.dance_impressions_per_elf = 0;

    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let end_tick = execution_start + dance_data.plan.total_ticks;

    // Complete the dance by having elf 0 trigger completion.
    sim.tick = end_tick;
    let mut events = Vec::new();
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // Elf 0 (A) should have DancedWithFriend.
    let thoughts_a = sim
        .db
        .thoughts
        .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC);
    assert!(
        thoughts_a
            .iter()
            .any(|t| t.kind == ThoughtKind::DancedWithFriend),
        "Elf A (who likes elf B) should get DancedWithFriend"
    );

    // Elf 1 (B) should NOT have DancedWithFriend (no opinion toward A).
    let thoughts_b = sim
        .db
        .thoughts
        .by_creature_id(&elves[1], tabulosity::QueryOpts::ASC);
    assert!(
        !thoughts_b
            .iter()
            .any(|t| t.kind == ThoughtKind::DancedWithFriend),
        "Elf B (who has no opinion of elf A) should NOT get DancedWithFriend"
    );
}

#[test]
fn dance_friend_bonus_below_threshold_no_bonus() {
    // A positive opinion below the acquaintance threshold should not trigger
    // the DancedWithFriend bonus.
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    // Pre-seed opinion below acquaintance threshold (default 5).
    sim.upsert_opinion(elves[0], OpinionKind::Friendliness, elves[1], 3);

    let activity_id = setup_executing_dance(&mut sim, &elves, location);
    sim.config.activity.dance_impressions_per_elf = 0;

    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let end_tick = execution_start + dance_data.plan.total_ticks;

    sim.tick = end_tick;
    let mut events = Vec::new();
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC);
    assert!(
        !thoughts
            .iter()
            .any(|t| t.kind == ThoughtKind::DancedWithFriend),
        "Opinion below acquaintance threshold should not trigger DancedWithFriend"
    );
}

#[test]
fn dance_impressions_made_serde_roundtrip() {
    // Verify that a non-zero impressions_made value survives serde roundtrip,
    // proving mid-dance save/load preserves impression progress.
    let mut rng = GameRng::new(42);
    let participant = crate::db::ActivityParticipant {
        activity_id: ActivityId::new(&mut rng),
        creature_id: CreatureId::new(&mut rng),
        role: ParticipantRole::Member,
        status: ParticipantStatus::Arrived,
        assigned_position: VoxelCoord::new(10, 1, 10),
        travel_task: None,
        dance_slot: Some(0),
        waypoint_cursor: 5,
        impressions_made: 3,
    };
    let json = serde_json::to_string(&participant).unwrap();
    let restored: crate::db::ActivityParticipant = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.impressions_made, 3);
    assert_eq!(restored.waypoint_cursor, 5);
}

#[test]
fn dance_single_impression_fires_at_completion_boundary() {
    // With dance_impressions_per_elf = 1, the impression threshold is at
    // exactly total_ticks. Since impression checks run before the completion
    // check, the single impression should fire and the dance should complete
    // on the same activation.
    let mut sim = flat_world_sim(fresh_test_seed());
    let elves = spawn_test_elves(&mut sim, 3);
    let location = sim.db.creatures.get(&elves[0]).unwrap().position.min;

    let activity_id = setup_executing_dance(&mut sim, &elves, location);
    sim.config.activity.dance_impressions_per_elf = 1;

    let activity = sim.db.activities.get(&activity_id).unwrap();
    let execution_start = activity.execution_start_tick.unwrap();
    let dance_data = sim.db.activity_dance_data.get(&activity_id).unwrap();
    let total_ticks = dance_data.plan.total_ticks;

    // Check impressions just before completion to verify the impression fires.
    // (At exactly total_ticks, the impression check runs, then completion
    // deletes the participant — so we check one tick before to verify the
    // impression counter, then trigger completion.)
    sim.tick = execution_start + total_ticks - 1;
    let mut events = Vec::new();
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // At total_ticks - 1, elapsed < total_ticks/1 = total_ticks, so no
    // impression yet.
    let p = sim
        .db
        .activity_participants
        .get(&(activity_id, elves[0]))
        .unwrap();
    assert_eq!(
        p.impressions_made, 0,
        "Before the threshold tick, no impression should fire"
    );

    // Now activate at exactly total_ticks — impression fires, then completion.
    sim.tick = execution_start + total_ticks;
    sim.execute_activity_behavior(elves[0], activity_id, &mut events);

    // Activity should have completed (participant removed by cascade delete).
    let activity = sim.db.activities.get(&activity_id);
    assert!(
        activity.is_none()
            || matches!(
                activity.unwrap().phase,
                ActivityPhase::Complete | ActivityPhase::Cancelled
            ),
        "Dance should complete after the single impression"
    );

    // The impression check ran before completion, so a thought should exist
    // (EnjoyedDanceWith, AwkwardDanceWith, or DancedInGroup from completion).
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&elves[0], tabulosity::QueryOpts::ASC);
    assert!(
        thoughts.iter().any(|t| matches!(
            &t.kind,
            ThoughtKind::EnjoyedDanceWith(_)
                | ThoughtKind::AwkwardDanceWith(_)
                | ThoughtKind::DancedInGroup
        )),
        "Dance completion with single impression should produce dance thoughts"
    );
}
