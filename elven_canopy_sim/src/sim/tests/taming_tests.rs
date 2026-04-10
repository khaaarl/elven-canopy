//! Tests for the creature taming system: tame designation, scout-only
//! claiming, taming rolls, beastcraft advancement, cleanup on death,
//! and serde roundtrip.
//! Corresponds to `sim/taming.rs`.

use super::*;
use crate::db::Civilization;
use crate::types::{CivId, CivSpecies, CultureTag};

// ---------------------------------------------------------------------------
// Taming (F-taming)
// ---------------------------------------------------------------------------

#[test]
fn taming_designate_creates_task_and_designation() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    // Verify no tame tasks or designations yet.
    assert!(sim.db.tame_designations.is_empty());
    assert!(
        !sim.db
            .tasks
            .iter_all()
            .any(|t| t.kind_tag == TaskKindTag::Tame)
    );

    designate_tame(&mut sim, capy_id);

    // Designation should exist for the player's civ.
    let pciv = sim.player_civ_id.unwrap();
    assert!(sim.db.tame_designations.get(&(capy_id, pciv)).is_some());

    // A Tame task should exist targeting the capybara.
    let tame_task = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == TaskKindTag::Tame)
        .expect("should create a Tame task");
    assert_eq!(tame_task.state, TaskState::Available);
    assert_eq!(tame_task.required_species, Some(Species::Elf));
    assert_eq!(tame_task.target_creature, Some(capy_id));

    // TaskTameData extension row should exist.
    let tame_data = sim.db.task_tame_data.get(&tame_task.id).unwrap();
    assert_eq!(tame_data.target, capy_id);
}

#[test]
fn taming_designate_untameable_is_noop() {
    let mut sim = test_sim(fresh_test_seed());
    let goblin_id = spawn_creature(&mut sim, Species::Goblin);

    designate_tame(&mut sim, goblin_id);

    // Should be rejected — no designation or task.
    assert!(sim.db.tame_designations.is_empty());
    assert!(
        !sim.db
            .tasks
            .iter_all()
            .any(|t| t.kind_tag == TaskKindTag::Tame)
    );
}

#[test]
fn taming_designate_own_civ_creature_is_noop() {
    let mut sim = test_sim(fresh_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Elves are player-civ and have tame_difficulty: None, so doubly rejected.
    designate_tame(&mut sim, elf_id);
    assert!(sim.db.tame_designations.is_empty());
}

#[test]
fn taming_cancel_removes_designation_and_task() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    designate_tame(&mut sim, capy_id);
    let pciv = sim.player_civ_id.unwrap();
    assert!(sim.db.tame_designations.get(&(capy_id, pciv)).is_some());
    let task_count_before = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame)
        .count();
    assert_eq!(task_count_before, 1);

    cancel_tame_designation(&mut sim, capy_id);

    // Designation removed.
    assert!(sim.db.tame_designations.get(&(capy_id, pciv)).is_none());

    // Task should be completed (not available for re-claim).
    let tame_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame)
        .collect();
    assert!(
        tame_tasks.is_empty() || tame_tasks.iter().all(|t| t.state == TaskState::Complete),
        "Tame task should be Complete or removed after cancellation"
    );
}

#[test]
fn taming_only_scouts_can_claim() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let warrior_id = spawn_elf(&mut sim);
    assign_path(&mut sim, warrior_id, PathId::Warrior);

    designate_tame(&mut sim, capy_id);

    // Clear warrior's current task so it's idle.
    let mut warrior = sim.db.creatures.get(&warrior_id).unwrap();
    warrior.current_task = None;
    sim.db.update_creature(warrior).unwrap();

    // Warrior should NOT find the Tame task.
    let found = sim.find_available_task(warrior_id);
    let is_tame = found.is_some_and(|tid| {
        sim.db
            .tasks
            .get(&tid)
            .is_some_and(|t| t.kind_tag == TaskKindTag::Tame)
    });
    assert!(!is_tame, "Warrior should not be able to claim Tame task");

    // Now spawn a Scout — should find it.
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);
    let mut scout = sim.db.creatures.get(&scout_id).unwrap();
    scout.current_task = None;
    sim.db.update_creature(scout).unwrap();

    let found_scout = sim.find_available_task(scout_id);
    let is_tame_scout = found_scout.is_some_and(|tid| {
        sim.db
            .tasks
            .get(&tid)
            .is_some_and(|t| t.kind_tag == TaskKindTag::Tame)
    });
    assert!(is_tame_scout, "Scout should be able to claim Tame task");
}

#[test]
fn taming_roll_succeeds_with_high_stats() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Give the scout very high stats so the roll always succeeds.
    // Capybara tame_difficulty = 100. WIL+CHA+Beastcraft = 500 >> 100.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 200);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 200);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 100);

    // Move scout near capybara and clear any existing task so they're
    // available to pick up the tame designation immediately.
    force_idle(&mut sim, scout_id);
    let capy_pos = sim.db.creatures.get(&capy_id).unwrap().position;
    let mut scout = sim.db.creatures.get(&scout_id).unwrap();
    scout.position = capy_pos;
    let _ = sim.db.upsert_creature(scout);

    designate_tame(&mut sim, capy_id);

    // Run the sim for enough ticks for the scout to walk + attempt.
    // Generous: 100k ticks should be more than enough.
    for _ in 0..200 {
        sim.step(&[], sim.tick + 500);
    }

    // The capybara should now be in the player's civ.
    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert_eq!(
        capy.civ_id, sim.player_civ_id,
        "Capybara should be tamed (civ_id = player)"
    );

    // Designation should be removed.
    let pciv = sim.player_civ_id.unwrap();
    assert!(
        sim.db.tame_designations.get(&(capy_id, pciv)).is_none(),
        "Designation should be cleaned up after successful tame"
    );
}

#[test]
fn taming_roll_fails_with_zero_stats() {
    let mut sim = test_sim(fresh_test_seed());
    let elephant_id = spawn_creature(&mut sim, Species::Elephant);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Zero out all relevant stats and set difficulty extremely high so
    // success is structurally impossible regardless of PRNG rolls.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 0);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 0);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 0);

    // Override elephant difficulty to be unreachable (0 + quasi_normal(50) peaks ~300).
    sim.config
        .species
        .get_mut(&Species::Elephant)
        .unwrap()
        .tame_difficulty = Some(10_000);

    designate_tame(&mut sim, elephant_id);

    // Run for a moderate amount of ticks. The elephant should NOT be tamed.
    for _ in 0..50 {
        sim.step(&[], sim.tick + 500);
    }

    let elephant = sim.db.creatures.get(&elephant_id).unwrap();
    assert!(
        elephant.civ_id.is_none(),
        "Elephant should remain wild with zero stats"
    );

    // Designation should still be active.
    let pciv = sim.player_civ_id.unwrap();
    assert!(
        sim.db.tame_designations.get(&(elephant_id, pciv)).is_some(),
        "Designation should persist while taming is ongoing"
    );
}

#[test]
fn taming_advances_beastcraft_skill() {
    let mut sim = flat_world_sim(fresh_test_seed());
    // Disable food/rest decay so needs-based tasks don't preempt taming.
    for spec in sim.species_table.values_mut() {
        spec.food_decay_per_tick = 0;
        spec.rest_decay_per_tick = 0;
    }

    // Suppress all initial creatures so they don't interfere.
    let initial_ids: Vec<CreatureId> = sim.db.creatures.iter_all().map(|c| c.id).collect();
    for &id in &initial_ids {
        suppress_activation(&mut sim, id);
    }

    // Use an elephant (difficulty 250) to ensure many attempts before success.
    let elephant_id = spawn_creature(&mut sim, Species::Elephant);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Zero stats so outcomes don't depend on random stat rolls.
    zero_creature_stats(&mut sim, scout_id);
    // Set stats high enough to eventually succeed after many attempts.
    // WIL(100) + CHA(100) + Beastcraft(0) = 200 vs difficulty 250 → ~16% chance.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 100);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 100);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 0);

    // Place scout near the elephant so pathing succeeds quickly.
    let elephant_pos = creature_pos(&sim, elephant_id);
    force_position(&mut sim, scout_id, elephant_pos);

    // Guaranteed skill advance on every attempt.
    sim.config.tame_skill_advance_probability = 1000;

    let initial_beastcraft = sim.trait_int(scout_id, TraitKind::Beastcraft, 0);

    designate_tame(&mut sim, elephant_id);

    // Loop until beastcraft advances or we hit the limit.
    let mut advanced = false;
    for i in 0..400 {
        sim.step(&[], sim.tick + 500);
        let bc = sim.trait_int(scout_id, TraitKind::Beastcraft, 0);
        let scout = sim.db.creatures.get(&scout_id).unwrap();
        if i % 50 == 0 || bc > initial_beastcraft {
            eprintln!(
                "taming_skill step {i}: tick={} beastcraft={bc} task={:?} action={:?} \
                 pos=({},{},{})",
                sim.tick,
                scout.current_task,
                scout.action_kind,
                scout.position.min.x,
                scout.position.min.y,
                scout.position.min.z,
            );
        }
        if bc > initial_beastcraft {
            advanced = true;
            break;
        }
    }

    let final_beastcraft = sim.trait_int(scout_id, TraitKind::Beastcraft, 0);
    let scout = sim.db.creatures.get(&scout_id).unwrap();
    assert!(
        advanced,
        "Beastcraft should have advanced from taming attempts ({} -> {}). \
         task={:?}, action={:?}, pos=({},{},{})",
        initial_beastcraft,
        final_beastcraft,
        scout.current_task,
        scout.action_kind,
        scout.position.min.x,
        scout.position.min.y,
        scout.position.min.z,
    );
}

#[test]
fn taming_target_death_cleans_up() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    designate_tame(&mut sim, capy_id);

    // Kill the capybara.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DebugKillCreature {
            creature_id: capy_id,
        },
    };
    sim.step(&[kill_cmd], sim.tick + 1);

    // Run a few more ticks so the taming task activates and sees the dead target.
    for _ in 0..20 {
        sim.step(&[], sim.tick + 500);
    }

    // The tame task should be completed (not dangling).
    let active_tame_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame && t.state != TaskState::Complete)
        .collect();
    assert!(
        active_tame_tasks.is_empty(),
        "Tame task should be completed after target death"
    );

    // Designation should be cleaned up.
    let pciv = sim.player_civ_id.unwrap();
    assert!(
        sim.db.tame_designations.get(&(capy_id, pciv)).is_none(),
        "Designation should be removed after target death"
    );
}

#[test]
fn taming_designate_dead_creature_is_noop() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    // Kill the capybara first.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DebugKillCreature {
            creature_id: capy_id,
        },
    };
    sim.step(&[kill_cmd], sim.tick + 1);

    // Now try to designate the dead creature.
    designate_tame(&mut sim, capy_id);

    assert!(sim.db.tame_designations.is_empty());
    assert!(
        !sim.db
            .tasks
            .iter_all()
            .any(|t| t.kind_tag == TaskKindTag::Tame)
    );
}

#[test]
fn taming_double_designate_is_idempotent() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    designate_tame(&mut sim, capy_id);
    designate_tame(&mut sim, capy_id);

    // Only one designation and one task.
    assert_eq!(sim.db.tame_designations.len(), 1);
    let tame_task_count = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame)
        .count();
    assert_eq!(tame_task_count, 1);
}

#[test]
fn taming_cancel_nonexistent_is_noop() {
    let mut sim = test_sim(fresh_test_seed());
    let fake_id = CreatureId::new(&mut elven_canopy_prng::GameRng::new(999));

    // Should not panic or error.
    cancel_tame_designation(&mut sim, fake_id);

    assert!(sim.db.tame_designations.is_empty());
}

#[test]
fn taming_outcast_cannot_claim() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let outcast_id = spawn_elf(&mut sim);
    // Outcast is the default — no path assignment needed.

    designate_tame(&mut sim, capy_id);

    let mut outcast = sim.db.creatures.get(&outcast_id).unwrap();
    outcast.current_task = None;
    sim.db.update_creature(outcast).unwrap();

    let found = sim.find_available_task(outcast_id);
    let is_tame = found.is_some_and(|tid| {
        sim.db
            .tasks
            .get(&tid)
            .is_some_and(|t| t.kind_tag == TaskKindTag::Tame)
    });
    assert!(!is_tame, "Outcast should not be able to claim Tame task");
}

#[test]
fn taming_success_emits_event_and_notification() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Very high stats for guaranteed success.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 500);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 500);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 500);

    // Move scout near capybara and clear any existing task.
    force_idle(&mut sim, scout_id);
    let capy_pos = sim.db.creatures.get(&capy_id).unwrap().position;
    let mut scout = sim.db.creatures.get(&scout_id).unwrap();
    scout.position = capy_pos;
    let _ = sim.db.upsert_creature(scout);

    let notif_count_before = sim.db.notifications.len();

    designate_tame(&mut sim, capy_id);

    // Run until tamed.
    let mut tamed = false;
    let mut events_collected = Vec::new();
    for _ in 0..200 {
        let result = sim.step(&[], sim.tick + 500);
        events_collected.extend(result.events);
        if sim
            .db
            .creatures
            .get(&capy_id)
            .is_some_and(|c| c.civ_id.is_some())
        {
            tamed = true;
            break;
        }
    }
    assert!(tamed, "Capybara should be tamed with very high stats");

    // Check CreatureTamed event was emitted.
    let has_tamed_event = events_collected.iter().any(|e| {
        matches!(
            &e.kind,
            SimEventKind::CreatureTamed {
                creature_id,
                tamer_id,
            } if *creature_id == capy_id && *tamer_id == scout_id
        )
    });
    assert!(has_tamed_event, "CreatureTamed event should be emitted");

    // Check notification was created.
    assert!(
        sim.db.notifications.len() > notif_count_before,
        "A taming notification should be created"
    );
}

#[test]
fn taming_task_resumable_after_preemption() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Set difficulty very high so taming never succeeds during this test.
    sim.config
        .species
        .get_mut(&Species::Capybara)
        .unwrap()
        .tame_difficulty = Some(10_000);

    designate_tame(&mut sim, capy_id);

    // Run until the scout claims the task.
    for _ in 0..100 {
        sim.step(&[], sim.tick + 500);
    }

    let tame_task = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == TaskKindTag::Tame)
        .expect("Tame task should exist");
    let task_id = tame_task.id;

    // Force the scout to drop the task by making it very hungry (survival preemption).
    let mut scout = sim.db.creatures.get(&scout_id).unwrap();
    scout.food = 0;
    sim.db.update_creature(scout).unwrap();

    // Run a few ticks for the heartbeat to trigger hunger preemption.
    for _ in 0..20 {
        sim.step(&[], sim.tick + 500);
    }

    // The tame task should be Available (not Complete).
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Available,
        "Tame task should revert to Available after preemption, got {:?}",
        task.state
    );
}

/// Taming task should detect when the target creature is already tamed
/// (e.g., by another player in multiplayer) and complete immediately,
/// rather than chasing the target forever. Regression test for B-tame-already.
#[test]
fn taming_target_already_tamed_completes_task() {
    let mut sim = test_sim(fresh_test_seed());

    // Disable food/rest decay so the scout focuses on taming.
    for spec in sim.species_table.values_mut() {
        spec.food_decay_per_tick = 0;
        spec.rest_decay_per_tick = 0;
    }

    // Suppress all initial creatures.
    let initial_ids: Vec<CreatureId> = sim.db.creatures.iter_all().map(|c| c.id).collect();
    for &id in &initial_ids {
        suppress_activation_until(&mut sim, id, u64::MAX);
    }

    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Set difficulty very high so the scout never succeeds via roll.
    sim.config
        .species
        .get_mut(&Species::Capybara)
        .unwrap()
        .tame_difficulty = Some(10_000);

    // Give the scout baseline stats so they attempt the tame task.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 100);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 100);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 100);

    // Move scout to capybara and clear existing tasks on both so
    // neither wanders away during the test.
    force_idle(&mut sim, scout_id);
    force_idle(&mut sim, capy_id);
    let capy_pos = sim.db.creatures.get(&capy_id).unwrap().position;
    let mut scout = sim.db.creatures.get(&scout_id).unwrap();
    scout.position = capy_pos;
    let _ = sim.db.upsert_creature(scout);
    suppress_activation_until(&mut sim, capy_id, u64::MAX);

    designate_tame(&mut sim, capy_id);

    // Run until scout claims the tame task.
    let mut scout_claimed = false;
    for i in 0..100 {
        sim.step(&[], sim.tick + 50);
        let sc = sim.db.creatures.get(&scout_id).unwrap();
        let has_tame = sc
            .current_task
            .and_then(|tid| sim.db.tasks.get(&tid))
            .is_some_and(|t| t.kind_tag == TaskKindTag::Tame);
        eprintln!(
            "  tame_already step {i}: tick={} task={:?} action={:?} nat={:?} has_tame={has_tame}",
            sim.tick, sc.current_task, sc.action_kind, sc.next_available_tick,
        );
        if has_tame {
            scout_claimed = true;
            break;
        }
    }
    assert!(scout_claimed, "Scout should claim tame task");

    // Manually tame the capybara (simulating another player in multiplayer).
    if let Some(mut capy) = sim.db.creatures.get(&capy_id) {
        capy.civ_id = sim.player_civ_id;
        sim.db.update_creature(capy).unwrap();
    }

    // Run more ticks — scout should detect target is tamed and complete.
    let mut completed = false;
    for i in 0..100 {
        sim.step(&[], sim.tick + 50);
        let active_tame: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Tame && t.state != TaskState::Complete)
            .collect();
        let sc = sim.db.creatures.get(&scout_id).unwrap();
        eprintln!(
            "  tame_already_post step {i}: tick={} active_tames={} task={:?} action={:?}",
            sim.tick,
            active_tame.len(),
            sc.current_task,
            sc.action_kind,
        );
        if active_tame.is_empty() {
            completed = true;
            break;
        }
    }
    assert!(
        completed,
        "Tame task should complete when target is already tamed"
    );
    let pciv = sim.player_civ_id.unwrap();
    assert!(
        sim.db.tame_designations.get(&(capy_id, pciv)).is_none(),
        "Designation should be removed"
    );
}

#[test]
fn taming_success_assigns_pet_name() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Very high stats for guaranteed success.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 500);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 500);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 500);

    // Capybara should start unnamed.
    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert!(capy.name.is_empty(), "Wild capybara should be unnamed");

    designate_tame(&mut sim, capy_id);

    // Run until tamed.
    let mut tamed = false;
    for _ in 0..200 {
        sim.step(&[], sim.tick + 500);
        if sim
            .db
            .creatures
            .get(&capy_id)
            .is_some_and(|c| c.civ_id.is_some())
        {
            tamed = true;
            break;
        }
    }
    assert!(tamed, "Capybara should be tamed with very high stats");

    // After taming, the capybara should have a non-empty name and meaning.
    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert!(
        !capy.name.is_empty(),
        "Tamed capybara should receive a pet name"
    );
    assert!(
        !capy.name_meaning.is_empty(),
        "Tamed capybara should receive a name meaning"
    );
    // Pet names are single-part (no spaces).
    assert!(
        !capy.name.contains(' '),
        "Pet name '{}' should be a single word",
        capy.name
    );
}

#[test]
fn taming_notification_includes_pet_name() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    set_trait(&mut sim, scout_id, TraitKind::Willpower, 500);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 500);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 500);

    let notif_count_before = sim.db.notifications.len();

    designate_tame(&mut sim, capy_id);

    // Run until tamed.
    for _ in 0..200 {
        sim.step(&[], sim.tick + 500);
        if sim
            .db
            .creatures
            .get(&capy_id)
            .is_some_and(|c| c.civ_id.is_some())
        {
            break;
        }
    }

    // The notification should include the pet's new name.
    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert!(!capy.name.is_empty());

    let new_notifs: Vec<_> = sim
        .db
        .notifications
        .iter_all()
        .skip(notif_count_before)
        .collect();
    let has_name_notif = new_notifs.iter().any(|n| n.message.contains(&capy.name));
    assert!(
        has_name_notif,
        "Taming notification should include the pet's name '{}', got: {:?}",
        capy.name,
        new_notifs.iter().map(|n| &n.message).collect::<Vec<_>>()
    );
}

#[test]
fn taming_serde_roundtrip_preserves_pet_name() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    set_trait(&mut sim, scout_id, TraitKind::Willpower, 500);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 500);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 500);

    designate_tame(&mut sim, capy_id);

    // Run until tamed.
    for _ in 0..200 {
        sim.step(&[], sim.tick + 500);
        if sim
            .db
            .creatures
            .get(&capy_id)
            .is_some_and(|c| c.civ_id.is_some())
        {
            break;
        }
    }

    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert!(
        !capy.name.is_empty(),
        "Capybara should be named after taming"
    );
    let name_before = capy.name.clone();
    let meaning_before = capy.name_meaning.clone();

    // Serde roundtrip.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let capy_restored = restored.db.creatures.get(&capy_id).unwrap();
    assert_eq!(
        capy_restored.name, name_before,
        "Pet name should survive serde roundtrip"
    );
    assert_eq!(
        capy_restored.name_meaning, meaning_before,
        "Pet name meaning should survive serde roundtrip"
    );
}

#[test]
fn taming_serde_roundtrip_preserves_designations_and_tasks() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    designate_tame(&mut sim, capy_id);

    // Verify data exists before roundtrip.
    let pciv = sim.player_civ_id.unwrap();
    assert!(sim.db.tame_designations.get(&(capy_id, pciv)).is_some());
    let tame_task_count = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame)
        .count();
    assert_eq!(tame_task_count, 1);

    // Serialize and deserialize the entire SimState.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    // Verify data survives roundtrip.
    assert!(
        restored
            .db
            .tame_designations
            .get(&(capy_id, pciv))
            .is_some(),
        "TameDesignation should survive serde roundtrip"
    );
    let restored_tame_tasks = restored
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame)
        .count();
    assert_eq!(
        restored_tame_tasks, 1,
        "Tame task should survive serde roundtrip"
    );

    // Verify TaskTameData extension row survives.
    let task_id = restored
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == TaskKindTag::Tame)
        .unwrap()
        .id;
    let tame_data = restored.db.task_tame_data.get(&task_id);
    assert!(
        tame_data.is_some(),
        "TaskTameData should survive serde roundtrip"
    );
    assert_eq!(tame_data.unwrap().target, capy_id);
}

/// TameDesignation must support multiple civs designating the same creature.
/// The composite PK `(creature_id, civ_id)` allows this; a single-field PK
/// on `creature_id` alone would silently drop competing designations.
/// Regression test for B-tame-civ-id.
#[test]
fn taming_multi_civ_designations_coexist() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let civ2_id = create_second_civ(&mut sim);

    // Player civ designates the capybara.
    designate_tame(&mut sim, capy_id);
    assert_eq!(sim.db.tame_designations.len(), 1);

    // Simulate civ2 designating the same capybara by inserting directly
    // (there's no multi-civ command path yet, but the data layer must allow it).
    let _ = sim.db.insert_tame_designation(crate::db::TameDesignation {
        creature_id: capy_id,
        civ_id: civ2_id,
        designated_tick: sim.tick,
    });

    // Both designations should coexist.
    assert_eq!(
        sim.db.tame_designations.len(),
        2,
        "Two civs should be able to designate the same creature"
    );

    // Cancelling the player's designation should leave civ2's in place.
    cancel_tame_designation(&mut sim, capy_id);
    assert_eq!(
        sim.db.tame_designations.len(),
        1,
        "Only player's designation should be removed on cancel"
    );
    // Remaining designation belongs to civ2.
    let remaining = sim.db.tame_designations.iter_all().next().unwrap();
    assert_eq!(remaining.civ_id, civ2_id);
}

/// When taming succeeds, ALL designations for that creature (from any civ)
/// should be removed, since the creature is no longer wild.
#[test]
fn taming_success_removes_all_civ_designations() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Very high stats for guaranteed success.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 500);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 500);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 500);

    // Move scout near capybara.
    force_idle(&mut sim, scout_id);
    let capy_pos = sim.db.creatures.get(&capy_id).unwrap().position;
    let mut scout = sim.db.creatures.get(&scout_id).unwrap();
    scout.position = capy_pos;
    let _ = sim.db.upsert_creature(scout);

    let civ2_id = create_second_civ(&mut sim);

    // Both civs designate.
    designate_tame(&mut sim, capy_id);
    let _ = sim.db.insert_tame_designation(crate::db::TameDesignation {
        creature_id: capy_id,
        civ_id: civ2_id,
        designated_tick: sim.tick,
    });
    assert_eq!(sim.db.tame_designations.len(), 2);

    // Run until tamed.
    for _ in 0..200 {
        sim.step(&[], sim.tick + 500);
        if sim
            .db
            .creatures
            .get(&capy_id)
            .is_some_and(|c| c.civ_id.is_some())
        {
            break;
        }
    }

    // ALL designations should be removed.
    let remaining: Vec<_> = sim
        .db
        .tame_designations
        .by_creature_id(&capy_id, tabulosity::QueryOpts::ASC);
    assert!(
        remaining.is_empty(),
        "All designations should be removed after taming succeeds, found {}",
        remaining.len()
    );
}

/// Helper: create a second friendly civ for multi-civ tests.
fn create_second_civ(sim: &mut SimState) -> CivId {
    let max_civ = sim
        .db
        .civilizations
        .iter_all()
        .map(|c| c.id.0)
        .max()
        .unwrap_or(0);
    let civ2_id = CivId(max_civ + 1);
    sim.db
        .insert_civilization(Civilization {
            id: civ2_id,
            name: "Friendly Elves 2".to_string(),
            primary_species: CivSpecies::Elf,
            minority_species: Vec::new(),
            culture_tag: CultureTag::Woodland,
            player_controlled: false,
        })
        .unwrap();
    civ2_id
}

/// When the tame target dies, ALL designations from all civs should be
/// removed — not just the civ whose scout is pursuing the target.
#[test]
fn taming_target_death_removes_all_civ_designations() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let civ2_id = create_second_civ(&mut sim);

    // Both civs designate.
    designate_tame(&mut sim, capy_id);
    let _ = sim.db.insert_tame_designation(crate::db::TameDesignation {
        creature_id: capy_id,
        civ_id: civ2_id,
        designated_tick: sim.tick,
    });
    assert_eq!(sim.db.tame_designations.len(), 2);

    // Kill the capybara.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DebugKillCreature {
            creature_id: capy_id,
        },
    };
    sim.step(&[kill_cmd], sim.tick + 1);

    // Both designations should be removed immediately (via handle_creature_death).
    assert!(
        sim.db.tame_designations.is_empty(),
        "All tame designations should be removed when target dies, found {}",
        sim.db.tame_designations.len()
    );
}

/// When taming succeeds, Tame tasks from OTHER civs targeting the same
/// creature should also be completed — not left as orphans for scouts to
/// wastefully pursue.
#[test]
fn taming_success_cancels_other_civs_tame_tasks() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Very high stats for guaranteed success.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 500);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 500);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 500);

    force_idle(&mut sim, scout_id);
    let capy_pos = sim.db.creatures.get(&capy_id).unwrap().position;
    let mut scout = sim.db.creatures.get(&scout_id).unwrap();
    scout.position = capy_pos;
    let _ = sim.db.upsert_creature(scout);

    let civ2_id = create_second_civ(&mut sim);

    // Player designates (creates a real task).
    designate_tame(&mut sim, capy_id);

    // Simulate civ2 also having a task + designation.
    let _ = sim.db.insert_tame_designation(crate::db::TameDesignation {
        creature_id: capy_id,
        civ_id: civ2_id,
        designated_tick: sim.tick,
    });
    let civ2_task = crate::task::Task {
        id: crate::types::TaskId::new(&mut sim.rng),
        kind: crate::task::TaskKind::Tame { target: capy_id },
        state: TaskState::Available,
        location: capy_pos.min,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: crate::task::TaskOrigin::PlayerDirected,
        target_creature: Some(capy_id),
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: Some(civ2_id),
    };
    sim.insert_task(sim.home_zone_id(), civ2_task);

    // Verify two tame tasks exist.
    let tame_task_count = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame)
        .count();
    assert_eq!(tame_task_count, 2);

    // Run until tamed.
    for _ in 0..200 {
        sim.step(&[], sim.tick + 500);
        if sim
            .db
            .creatures
            .get(&capy_id)
            .is_some_and(|c| c.civ_id.is_some())
        {
            break;
        }
    }

    // ALL tame tasks should be completed.
    let active_tame_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame && t.state != TaskState::Complete)
        .collect();
    assert!(
        active_tame_tasks.is_empty(),
        "All civs' tame tasks should be completed after taming succeeds, found {} active",
        active_tame_tasks.len()
    );
}

/// Multi-civ tame designations survive serde roundtrip. Verifies the
/// composite PK `(creature_id, civ_id)` and `creature_id` index are
/// correctly rebuilt after deserialization.
#[test]
fn taming_multi_civ_serde_roundtrip() {
    let mut sim = test_sim(fresh_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let civ2_id = create_second_civ(&mut sim);
    let pciv = sim.player_civ_id.unwrap();

    // Both civs designate.
    designate_tame(&mut sim, capy_id);
    let _ = sim.db.insert_tame_designation(crate::db::TameDesignation {
        creature_id: capy_id,
        civ_id: civ2_id,
        designated_tick: sim.tick,
    });
    assert_eq!(sim.db.tame_designations.len(), 2);

    // Roundtrip.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    // Both designations survive.
    assert_eq!(restored.db.tame_designations.len(), 2);
    assert!(
        restored
            .db
            .tame_designations
            .get(&(capy_id, pciv))
            .is_some()
    );
    assert!(
        restored
            .db
            .tame_designations
            .get(&(capy_id, civ2_id))
            .is_some()
    );

    // Index query works.
    let by_creature: Vec<_> = restored
        .db
        .tame_designations
        .by_creature_id(&capy_id, tabulosity::QueryOpts::ASC);
    assert_eq!(by_creature.len(), 2);
}
