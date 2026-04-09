//! Tests for the dinner party group activity (F-dinner-party).
//! Covers spontaneous organization, hunger-gated volunteering, execution
//! (eating + socializing), completion, and edge cases.

use super::*;

/// Helper: create a furnished dining hall at `pos` with one table and stock
/// `food_count` bread items. Returns the structure ID.
fn create_dining_hall(sim: &mut SimState, pos: VoxelCoord, food_count: u32) -> StructureId {
    let structure_id = StructureId(sim.next_structure_id);
    sim.next_structure_id += 1;
    let hz = sim.home_zone_id();
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            zone_id: hz,
            project_id,
            build_type: BuildType::Building,
            anchor: pos,
            width: 3,
            depth: 3,
            height: 3,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::DiningHall),
            inventory_id: inv_id,
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        })
        .unwrap();
    // Place one table (provides dining_seats_per_table seats).
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            zone_id: hz,
            structure_id,
            coord: pos,
            placed: true,
        })
        .unwrap();
    // Stock food.
    if food_count > 0 {
        sim.inv_add_simple_item(inv_id, ItemKind::Bread, food_count, None, None);
    }
    structure_id
}

// ---------------------------------------------------------------------------
// Spontaneous organization
// ---------------------------------------------------------------------------

#[test]
fn dinner_party_organization_with_eligible_elf() {
    // An elf with food below the organize threshold (60%), near a stocked
    // dining hall, should be able to organize a dinner party.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let hall_id = create_dining_hall(&mut sim, table_pos, 5);

    // Set food to 55% — below organize threshold (60%).
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Force 100% organize chance so PPM roll always succeeds.
    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;

    let mut events = Vec::new();
    let organized = sim.try_organize_spontaneous_dinner_party(elf_id, &mut events);
    assert!(organized, "Elf should organize a dinner party");

    // Verify activity was created.
    let activities: Vec<_> = sim
        .db
        .activities
        .iter_all()
        .filter(|a| a.kind == ActivityKind::DinnerParty)
        .collect();
    assert_eq!(activities.len(), 1, "Should have 1 DinnerParty activity");
    assert_eq!(activities[0].phase, ActivityPhase::Recruiting);

    // Verify structure ref.
    let refs = sim
        .db
        .activity_structure_refs
        .by_structure_id(&hall_id, tabulosity::QueryOpts::ASC);
    assert_eq!(refs.len(), 1);
    assert_eq!(
        refs[0].role,
        crate::db::ActivityStructureRole::DinnerPartyVenue
    );

    // Verify organizer participant.
    let participants = sim
        .db
        .activity_participants
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC);
    assert_eq!(participants.len(), 1);
    assert_eq!(participants[0].role, ParticipantRole::Organizer);
}

#[test]
fn dinner_party_blocked_when_food_above_organize_threshold() {
    // An elf with food above the organize threshold should NOT organize.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 5);

    // Set food to 65% — above organize threshold (60%).
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 65 / 100;
        sim.db.update_creature(c).unwrap();
    }

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;

    let mut events = Vec::new();
    let organized = sim.try_organize_spontaneous_dinner_party(elf_id, &mut events);
    assert!(
        !organized,
        "Elf with food above organize threshold should not organize"
    );
}

#[test]
fn dinner_party_blocked_by_elf_cooldown() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 5);

    // Set food below organize threshold.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 55 / 100;
        // Set cooldown — just attended a dinner party recently.
        c.last_dinner_party_tick = sim.tick;
        sim.db.update_creature(c).unwrap();
    }

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    sim.config.activity.dinner_party_elf_cooldown_ticks = 100_000;

    let mut events = Vec::new();
    let organized = sim.try_organize_spontaneous_dinner_party(elf_id, &mut events);
    assert!(!organized, "Elf on cooldown should not organize");
}

#[test]
fn dinner_party_blocked_by_venue_exclusivity() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 10);

    // Elf A organizes first.
    {
        let mut c = sim.db.creatures.get(&elf_a).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }
    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    let mut events = Vec::new();
    assert!(sim.try_organize_spontaneous_dinner_party(elf_a, &mut events));

    // Elf B tries to organize at the same hall — should fail (venue exclusivity).
    {
        let mut c = sim.db.creatures.get(&elf_b).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }
    let organized = sim.try_organize_spontaneous_dinner_party(elf_b, &mut events);
    assert!(
        !organized,
        "Second dinner party at same hall should be blocked"
    );
}

#[test]
fn dinner_party_blocked_by_insufficient_food() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    // Only 1 food — below min_count of 2.
    create_dining_hall(&mut sim, table_pos, 1);

    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    sim.config.activity.dinner_party_min_count = 2;

    let mut events = Vec::new();
    let organized = sim.try_organize_spontaneous_dinner_party(elf_id, &mut events);
    assert!(!organized, "Should not organize with insufficient food");
}

#[test]
fn dinner_party_new_hall_bypasses_cooldown() {
    // A dining hall that has never hosted a completed dinner party should
    // bypass the hall cooldown (first-dinner-party nudge).
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 5);

    // Hall has never hosted a dinner party (last_dinner_party_completed_tick = 0).
    // Set a very long hall cooldown — it should be bypassed.
    sim.config.activity.dinner_party_hall_cooldown_ticks = 999_999_999;
    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;

    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    let organized = sim.try_organize_spontaneous_dinner_party(elf_id, &mut events);
    assert!(organized, "New hall should bypass cooldown");
}

// ---------------------------------------------------------------------------
// Hunger-gated volunteering
// ---------------------------------------------------------------------------

#[test]
fn dinner_party_volunteer_below_join_threshold() {
    // An elf below the join threshold (70%) should volunteer for an existing
    // dinner party.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let organizer = spawn_creature(&mut sim, Species::Elf);
    let volunteer_elf = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 10);

    // Organizer creates a dinner party.
    {
        let mut c = sim.db.creatures.get(&organizer).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }
    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    let mut events = Vec::new();
    assert!(sim.try_organize_spontaneous_dinner_party(organizer, &mut events));

    // Volunteer elf has food at 65% — below join threshold (70%).
    {
        let mut c = sim.db.creatures.get(&volunteer_elf).unwrap();
        c.food = food_max * 65 / 100;
        sim.db.update_creature(c).unwrap();
    }

    let found = sim.find_open_activity_for_creature(volunteer_elf);
    assert!(
        found.is_some(),
        "Elf below join threshold should find a dinner party to volunteer for"
    );
}

#[test]
fn dinner_party_no_volunteer_above_join_threshold() {
    // An elf above the join threshold (70%) should NOT volunteer.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let organizer = spawn_creature(&mut sim, Species::Elf);
    let full_elf = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 10);

    // Organizer creates a dinner party.
    {
        let mut c = sim.db.creatures.get(&organizer).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }
    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    let mut events = Vec::new();
    assert!(sim.try_organize_spontaneous_dinner_party(organizer, &mut events));

    // Full elf has food at 75% — above join threshold (70%).
    {
        let mut c = sim.db.creatures.get(&full_elf).unwrap();
        c.food = food_max * 75 / 100;
        sim.db.update_creature(c).unwrap();
    }

    let found = sim.find_open_activity_for_creature(full_elf);
    assert!(
        found.is_none(),
        "Elf above join threshold should not volunteer for a dinner party"
    );
}

// ---------------------------------------------------------------------------
// Execution: eating, socializing, completion
// ---------------------------------------------------------------------------

#[test]
fn dinner_party_full_lifecycle() {
    // Organize → quorum → assembly → execution → eating + socializing → completion.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    // Disable food/rest decay so needs-based tasks don't preempt the activity
    // during the step.
    {
        let elf = sim.species_table.get_mut(&Species::Elf).unwrap();
        elf.food_decay_per_tick = 0;
        elf.rest_decay_per_tick = 0;
    }

    // Create dining hall with plenty of food.
    let hall_id = create_dining_hall(&mut sim, table_pos, 10);

    // Config: 100% organize chance, min 2 participants, short duration.
    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    sim.config.activity.dinner_party_min_count = 2;
    sim.config.activity.dinner_party_desired_count = 4;
    sim.config.activity.dinner_party_duration_secs = 5.0;
    sim.config.activity.dinner_party_impressions_per_elf = 2;

    // Spawn two elves, clear any tasks from spawn, and make them hungry
    // enough to organize/join.
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);
    for eid in [elf_a, elf_b] {
        force_idle_and_cancel_activations(&mut sim, eid);
        force_position(&mut sim, eid, table_pos);
        let mut c = sim.db.creatures.get(&eid).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }

    let food_a_before = sim.db.creatures.get(&elf_a).unwrap().food;

    // Elf A organizes.
    let mut events = Vec::new();
    assert!(sim.try_organize_spontaneous_dinner_party(elf_a, &mut events));

    // Elf B volunteers.
    let activity_id = sim.find_open_activity_for_creature(elf_b).unwrap();
    sim.volunteer_for_activity(activity_id, elf_b, &mut events);

    // Check quorum (promotes volunteers to Traveling, transitions to Assembling).
    sim.check_volunteer_quorum(activity_id, &mut events);
    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(
        activity.phase,
        ActivityPhase::Assembling,
        "Activity should be Assembling after quorum"
    );

    // Simulate arrival: set both participants to Arrived and positioned at
    // the dining hall.
    for eid in [elf_a, elf_b] {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, eid)) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
        // Clear current_task (GoTo would have been completed) and ensure
        // activation isn't suppressed.
        if let Some(mut c) = sim.db.creatures.get(&eid) {
            c.current_task = None;
            c.next_available_tick = Some(sim.tick + 1);
            sim.db.update_creature(c).unwrap();
        }
    }

    // Trigger assembly complete check → should begin execution.
    sim.check_assembly_complete(activity_id);
    let activity = sim.db.activities.get(&activity_id).unwrap();
    assert_eq!(
        activity.phase,
        ActivityPhase::Executing,
        "Activity should be Executing after all arrive"
    );
    assert!(activity.total_cost > 0, "Total cost should be set");

    // Advance the sim past the dinner party duration.
    let duration_ticks = activity.total_cost.max(0) as u64;
    sim.step(&[], sim.tick + duration_ticks + 100);

    // Activity should be completed (removed from DB).
    assert!(
        sim.db.activities.get(&activity_id).is_none(),
        "Activity should be deleted after completion"
    );

    // Check eating: both participants' food should have increased.
    let food_a_after = sim.db.creatures.get(&elf_a).unwrap().food;
    let food_b_after = sim.db.creatures.get(&elf_b).unwrap().food;
    assert!(
        food_a_after > food_a_before,
        "Elf A should have eaten (food before={food_a_before}, after={food_a_after})"
    );
    assert!(
        food_b_after > food_a_before,
        "Elf B should have eaten (food before={food_a_before}, after={food_b_after})"
    );

    // Check thoughts: should have EnjoyedDinnerParty and AteDining.
    let thoughts_a: Vec<_> = sim
        .db
        .thoughts
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|t| t.kind.clone())
        .collect();
    assert!(
        thoughts_a
            .iter()
            .any(|t| *t == crate::types::ThoughtKind::EnjoyedDinnerParty),
        "Elf A should have EnjoyedDinnerParty thought, got: {thoughts_a:?}"
    );
    assert!(
        thoughts_a
            .iter()
            .any(|t| *t == crate::types::ThoughtKind::AteDining),
        "Elf A should have AteDining thought, got: {thoughts_a:?}"
    );

    // Check cooldowns set.
    let creature_a = sim.db.creatures.get(&elf_a).unwrap();
    assert!(
        creature_a.last_dinner_party_tick > 0,
        "Elf A's dinner party cooldown should be set"
    );

    // Check hall cooldown set.
    let structure = sim.db.structures.get(&hall_id).unwrap();
    assert!(
        structure.last_dinner_party_completed_tick > 0,
        "Hall's dinner party cooldown should be set"
    );

    // Check participants are released.
    assert!(
        sim.db
            .creatures
            .get(&elf_a)
            .unwrap()
            .current_activity
            .is_none(),
        "Elf A should have no current_activity after completion"
    );
}

#[test]
fn dinner_party_social_impressions_generated() {
    // Verify that social impression checks happen during dinner party execution.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    // Disable food/rest decay so needs-based tasks don't preempt the activity.
    {
        let elf = sim.species_table.get_mut(&Species::Elf).unwrap();
        elf.food_decay_per_tick = 0;
        elf.rest_decay_per_tick = 0;
    }

    create_dining_hall(&mut sim, table_pos, 10);

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    sim.config.activity.dinner_party_min_count = 2;
    sim.config.activity.dinner_party_duration_secs = 5.0;
    // Use many impressions to make it statistically near-impossible for all
    // rolls to land in the zero-delta band (-49..0).
    sim.config.activity.dinner_party_impressions_per_elf = 20;

    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);
    for eid in [elf_a, elf_b] {
        force_idle_and_cancel_activations(&mut sim, eid);
        force_position(&mut sim, eid, table_pos);
        let mut c = sim.db.creatures.get(&eid).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // No pre-existing opinions.
    assert!(
        sim.db.creature_opinions.iter_all().count() == 0,
        "No opinions should exist before dinner party"
    );

    // Organize + volunteer + quorum + arrive + execute.
    let mut events = Vec::new();
    assert!(sim.try_organize_spontaneous_dinner_party(elf_a, &mut events));
    let activity_id = sim.find_open_activity_for_creature(elf_b).unwrap();
    sim.volunteer_for_activity(activity_id, elf_b, &mut events);
    sim.check_volunteer_quorum(activity_id, &mut events);

    // Force arrival.
    for eid in [elf_a, elf_b] {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, eid)) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
        // Clear current_task and ensure activation isn't suppressed.
        if let Some(mut c) = sim.db.creatures.get(&eid) {
            c.current_task = None;
            c.next_available_tick = Some(sim.tick + 1);
            sim.db.update_creature(c).unwrap();
        }
    }
    sim.check_assembly_complete(activity_id);

    // Run past completion.
    let duration_ticks = sim
        .db
        .activities
        .get(&activity_id)
        .unwrap()
        .total_cost
        .max(0) as u64;
    sim.step(&[], sim.tick + duration_ticks + 100);

    // With 20 impression checks per elf, at least some should produce non-zero
    // deltas, creating opinion rows and/or dinner social thoughts.
    let opinion_count = sim.db.creature_opinions.iter_all().count();
    let thoughts_a: Vec<_> = sim
        .db
        .thoughts
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|t| t.kind.clone())
        .collect();
    let has_dinner_social = thoughts_a.iter().any(|t| {
        matches!(
            t,
            crate::types::ThoughtKind::EnjoyedDinnerWith(_)
                | crate::types::ThoughtKind::AwkwardDinnerWith(_)
        )
    });
    assert!(
        has_dinner_social || opinion_count > 0,
        "After 20 impression checks, should have social thoughts or opinions. \
         thoughts={thoughts_a:?}, opinions={opinion_count}"
    );
}

// ---------------------------------------------------------------------------
// Edge cases and integration
// ---------------------------------------------------------------------------

#[test]
fn dinner_party_hall_cooldown_blocks_after_completion() {
    // After a dinner party completes, the hall cooldown should prevent another
    // dinner party from being organized until the cooldown expires.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let hall_id = create_dining_hall(&mut sim, table_pos, 20);

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    sim.config.activity.dinner_party_hall_cooldown_ticks = 100_000;
    sim.config.activity.dinner_party_elf_cooldown_ticks = 0; // No elf cooldown.

    // Advance sim so tick is non-zero (0 is treated as "never hosted").
    sim.step(&[], 100);

    // Mark the hall as having completed a dinner party at the current tick.
    {
        let mut s = sim.db.structures.get(&hall_id).unwrap();
        s.last_dinner_party_completed_tick = sim.tick;
        sim.db.update_structure(s).unwrap();
    }

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    let organized = sim.try_organize_spontaneous_dinner_party(elf_id, &mut events);
    assert!(
        !organized,
        "Should not organize dinner party at hall still on cooldown"
    );
}

#[test]
fn dinner_party_serde_roundtrip_activity_kind() {
    let json = serde_json::to_string(&ActivityKind::DinnerParty).unwrap();
    let restored: ActivityKind = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, ActivityKind::DinnerParty);
}

#[test]
fn dinner_party_serde_roundtrip_structure_role() {
    let role = crate::db::ActivityStructureRole::DinnerPartyVenue;
    let json = serde_json::to_string(&role).unwrap();
    let restored: crate::db::ActivityStructureRole = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, role);
}

#[test]
fn dinner_party_serde_roundtrip_thoughts() {
    for thought in [
        crate::types::ThoughtKind::EnjoyedDinnerParty,
        crate::types::ThoughtKind::EnjoyedDinnerWith("Test".into()),
        crate::types::ThoughtKind::AwkwardDinnerWith("Test".into()),
    ] {
        let json = serde_json::to_string(&thought).unwrap();
        let restored: crate::types::ThoughtKind = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, thought);
    }
}

#[test]
fn dinner_party_not_organized_by_non_civ_creature() {
    // A creature without a civ_id should not organize a dinner party.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    create_dining_hall(&mut sim, table_pos, 10);

    // Spawn an elf and remove civ membership.
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 55 / 100;
        c.civ_id = None;
        sim.db.update_creature(c).unwrap();
    }

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;

    let mut events = Vec::new();
    let organized = sim.try_organize_spontaneous_dinner_party(elf_id, &mut events);
    assert!(
        !organized,
        "Non-civ creature should not organize dinner party"
    );
}

#[test]
fn dinner_party_food_consumed_from_hall_inventory() {
    // Verify that the dining hall's inventory has fewer items after the
    // dinner party completes.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;
    {
        let elf = sim.species_table.get_mut(&Species::Elf).unwrap();
        elf.food_decay_per_tick = 0;
        elf.rest_decay_per_tick = 0;
    }

    let hall_id = create_dining_hall(&mut sim, table_pos, 10);

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    sim.config.activity.dinner_party_min_count = 2;
    sim.config.activity.dinner_party_duration_secs = 2.0;

    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);
    for eid in [elf_a, elf_b] {
        force_idle_and_cancel_activations(&mut sim, eid);
        force_position(&mut sim, eid, table_pos);
        let mut c = sim.db.creatures.get(&eid).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Count food before.
    let inv_id = sim.db.structures.get(&hall_id).unwrap().inventory_id;
    let food_before: u32 = crate::inventory::ItemKind::EDIBLE_KINDS
        .iter()
        .map(|k| sim.inv_unreserved_item_count(inv_id, *k, crate::inventory::MaterialFilter::Any))
        .sum();

    // Full lifecycle.
    let mut events = Vec::new();
    assert!(sim.try_organize_spontaneous_dinner_party(elf_a, &mut events));
    let activity_id = sim.find_open_activity_for_creature(elf_b).unwrap();
    sim.volunteer_for_activity(activity_id, elf_b, &mut events);
    sim.check_volunteer_quorum(activity_id, &mut events);
    for eid in [elf_a, elf_b] {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, eid)) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
        if let Some(mut c) = sim.db.creatures.get(&eid) {
            c.current_task = None;
            c.next_available_tick = Some(sim.tick + 1);
            sim.db.update_creature(c).unwrap();
        }
    }
    sim.check_assembly_complete(activity_id);
    let duration_ticks = sim
        .db
        .activities
        .get(&activity_id)
        .unwrap()
        .total_cost
        .max(0) as u64;
    sim.step(&[], sim.tick + duration_ticks + 100);

    // Count food after.
    let food_after: u32 = crate::inventory::ItemKind::EDIBLE_KINDS
        .iter()
        .map(|k| sim.inv_unreserved_item_count(inv_id, *k, crate::inventory::MaterialFilter::Any))
        .sum();

    assert!(
        food_after < food_before,
        "Food should have been consumed (before={food_before}, after={food_after})"
    );
    // Two participants should each eat one item.
    assert_eq!(
        food_before - food_after,
        2,
        "Each participant should consume exactly 1 food item"
    );
}

#[test]
fn dinner_party_volunteer_blocked_by_elf_cooldown() {
    // An elf below the join threshold but on dinner party cooldown should NOT
    // be offered as a volunteer for an existing dinner party.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;

    let organizer = spawn_creature(&mut sim, Species::Elf);
    let cooldown_elf = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 10);

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    sim.config.activity.dinner_party_elf_cooldown_ticks = 100_000;

    // Organizer creates dinner party.
    {
        let mut c = sim.db.creatures.get(&organizer).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }
    let mut events = Vec::new();
    assert!(sim.try_organize_spontaneous_dinner_party(organizer, &mut events));

    // Cooldown elf is hungry but recently attended a dinner party.
    {
        let mut c = sim.db.creatures.get(&cooldown_elf).unwrap();
        c.food = food_max * 65 / 100;
        c.last_dinner_party_tick = sim.tick;
        sim.db.update_creature(c).unwrap();
    }

    let found = sim.find_open_activity_for_creature(cooldown_elf);
    assert!(
        found.is_none(),
        "Elf on cooldown should not volunteer for dinner party"
    );
}

#[test]
fn dinner_party_eat_graceful_when_no_food_available() {
    // If the hall runs out of food between organization and execution, the
    // participant should not panic — they just don't eat.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;
    {
        let elf = sim.species_table.get_mut(&Species::Elf).unwrap();
        elf.food_decay_per_tick = 0;
        elf.rest_decay_per_tick = 0;
    }

    // Create hall with 2 food (enough for min_count), then drain it.
    let structure_id = create_dining_hall(&mut sim, table_pos, 2);

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    sim.config.activity.dinner_party_min_count = 2;
    sim.config.activity.dinner_party_duration_secs = 2.0;

    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);
    for eid in [elf_a, elf_b] {
        force_idle_and_cancel_activations(&mut sim, eid);
        force_position(&mut sim, eid, table_pos);
        let mut c = sim.db.creatures.get(&eid).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    assert!(sim.try_organize_spontaneous_dinner_party(elf_a, &mut events));
    let activity_id = sim.find_open_activity_for_creature(elf_b).unwrap();
    sim.volunteer_for_activity(activity_id, elf_b, &mut events);
    sim.check_volunteer_quorum(activity_id, &mut events);

    // Drain all food from the hall before execution starts.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    while sim.inv_consume_one_edible(inv_id) {}

    // Force arrival and execution.
    for eid in [elf_a, elf_b] {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, eid)) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
        if let Some(mut c) = sim.db.creatures.get(&eid) {
            c.current_task = None;
            c.next_available_tick = Some(sim.tick + 1);
            sim.db.update_creature(c).unwrap();
        }
    }
    sim.check_assembly_complete(activity_id);

    let duration_ticks = sim
        .db
        .activities
        .get(&activity_id)
        .unwrap()
        .total_cost
        .max(0) as u64;
    sim.step(&[], sim.tick + duration_ticks + 100);

    // Should complete without panicking. Participants get the completion
    // thought even if they didn't eat (they still socialized).
    assert!(sim.db.activities.get(&activity_id).is_none());
    let thoughts: Vec<_> = sim
        .db
        .thoughts
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|t| t.kind.clone())
        .collect();
    assert!(
        thoughts
            .iter()
            .any(|t| *t == crate::types::ThoughtKind::EnjoyedDinnerParty)
    );
}

#[test]
fn dinner_party_zero_impressions_completes_cleanly() {
    // With impressions_per_elf = 0, the dinner party should complete without
    // panic or divide-by-zero.
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let food_max = sim.species_table[&Species::Elf].food_max;
    {
        let elf = sim.species_table.get_mut(&Species::Elf).unwrap();
        elf.food_decay_per_tick = 0;
        elf.rest_decay_per_tick = 0;
    }

    create_dining_hall(&mut sim, table_pos, 10);

    sim.config.activity.dinner_party_organize_chance_ppm = 1_000_000;
    sim.config.activity.dinner_party_min_count = 2;
    sim.config.activity.dinner_party_duration_secs = 2.0;
    sim.config.activity.dinner_party_impressions_per_elf = 0;

    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);
    for eid in [elf_a, elf_b] {
        force_idle_and_cancel_activations(&mut sim, eid);
        force_position(&mut sim, eid, table_pos);
        let mut c = sim.db.creatures.get(&eid).unwrap();
        c.food = food_max * 55 / 100;
        sim.db.update_creature(c).unwrap();
    }

    let mut events = Vec::new();
    assert!(sim.try_organize_spontaneous_dinner_party(elf_a, &mut events));
    let activity_id = sim.find_open_activity_for_creature(elf_b).unwrap();
    sim.volunteer_for_activity(activity_id, elf_b, &mut events);
    sim.check_volunteer_quorum(activity_id, &mut events);

    for eid in [elf_a, elf_b] {
        if let Some(mut p) = sim.db.activity_participants.get(&(activity_id, eid)) {
            p.status = ParticipantStatus::Arrived;
            p.travel_task = None;
            sim.db.update_activity_participant(p).unwrap();
        }
        if let Some(mut c) = sim.db.creatures.get(&eid) {
            c.current_task = None;
            c.next_available_tick = Some(sim.tick + 1);
            sim.db.update_creature(c).unwrap();
        }
    }
    sim.check_assembly_complete(activity_id);

    let duration_ticks = sim
        .db
        .activities
        .get(&activity_id)
        .unwrap()
        .total_cost
        .max(0) as u64;
    sim.step(&[], sim.tick + duration_ticks + 100);

    // Should complete cleanly with no social thoughts.
    assert!(sim.db.activities.get(&activity_id).is_none());
    let thoughts: Vec<_> = sim
        .db
        .thoughts
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|t| t.kind.clone())
        .collect();
    assert!(
        !thoughts.iter().any(|t| matches!(
            t,
            crate::types::ThoughtKind::EnjoyedDinnerWith(_)
                | crate::types::ThoughtKind::AwkwardDinnerWith(_)
        )),
        "No social thoughts with 0 impressions_per_elf"
    );
}
