//! Tests for the creature taming system: tame designation, scout-only
//! claiming, taming rolls, beastcraft advancement, cleanup on death,
//! and serde roundtrip.
//! Corresponds to `sim/taming.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Taming (F-taming)
// ---------------------------------------------------------------------------

#[test]
fn taming_designate_creates_task_and_designation() {
    let mut sim = test_sim(42);
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

    // Designation should exist.
    assert!(sim.db.tame_designations.get(&capy_id).is_some());

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
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    // Elves are player-civ and have tame_difficulty: None, so doubly rejected.
    designate_tame(&mut sim, elf_id);
    assert!(sim.db.tame_designations.is_empty());
}

#[test]
fn taming_cancel_removes_designation_and_task() {
    let mut sim = test_sim(42);
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    designate_tame(&mut sim, capy_id);
    assert!(sim.db.tame_designations.get(&capy_id).is_some());
    let task_count_before = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame)
        .count();
    assert_eq!(task_count_before, 1);

    cancel_tame_designation(&mut sim, capy_id);

    // Designation removed.
    assert!(sim.db.tame_designations.get(&capy_id).is_none());

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
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Give the scout very high stats so the roll always succeeds.
    // Capybara tame_difficulty = 100. WIL+CHA+Beastcraft = 500 >> 100.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 200);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 200);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 100);

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
    assert!(
        sim.db.tame_designations.get(&capy_id).is_none(),
        "Designation should be cleaned up after successful tame"
    );
}

#[test]
fn taming_roll_fails_with_zero_stats() {
    let mut sim = test_sim(42);
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
    assert!(
        sim.db.tame_designations.get(&elephant_id).is_some(),
        "Designation should persist while taming is ongoing"
    );
}

#[test]
fn taming_advances_beastcraft_skill() {
    let mut sim = test_sim(42);
    // Use an elephant (difficulty 250) to ensure many attempts before success.
    let elephant_id = spawn_creature(&mut sim, Species::Elephant);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Set stats high enough to eventually succeed after many attempts.
    // WIL(100) + CHA(100) + Beastcraft(0) = 200 vs difficulty 250 → ~16% chance.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 100);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 100);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 0);

    // Bump skill advance probability so it triggers reliably in tests.
    sim.config.tame_skill_advance_probability = 500; // 50%

    let initial_beastcraft = sim.trait_int(scout_id, TraitKind::Beastcraft, 0);

    designate_tame(&mut sim, elephant_id);

    // Run long enough for many attempts.
    for _ in 0..400 {
        sim.step(&[], sim.tick + 500);
    }

    let final_beastcraft = sim.trait_int(scout_id, TraitKind::Beastcraft, 0);
    assert!(
        final_beastcraft > initial_beastcraft,
        "Beastcraft should have advanced from taming attempts ({} -> {})",
        initial_beastcraft,
        final_beastcraft
    );
}

#[test]
fn taming_target_death_cleans_up() {
    let mut sim = test_sim(42);
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
    assert!(
        sim.db.tame_designations.get(&capy_id).is_none(),
        "Designation should be removed after target death"
    );
}

#[test]
fn taming_designate_dead_creature_is_noop() {
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
    let fake_id = CreatureId::new(&mut elven_canopy_prng::GameRng::new(999));

    // Should not panic or error.
    cancel_tame_designation(&mut sim, fake_id);

    assert!(sim.db.tame_designations.is_empty());
}

#[test]
fn taming_outcast_cannot_claim() {
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Very high stats for guaranteed success.
    set_trait(&mut sim, scout_id, TraitKind::Willpower, 500);
    set_trait(&mut sim, scout_id, TraitKind::Charisma, 500);
    set_trait(&mut sim, scout_id, TraitKind::Beastcraft, 500);

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
    let mut sim = test_sim(42);
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

#[test]
fn taming_target_already_tamed_completes_task() {
    let mut sim = test_sim(42);
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let scout_id = spawn_elf(&mut sim);
    assign_path(&mut sim, scout_id, PathId::Scout);

    // Set difficulty very high so the scout never succeeds via roll.
    sim.config
        .species
        .get_mut(&Species::Capybara)
        .unwrap()
        .tame_difficulty = Some(10_000);

    designate_tame(&mut sim, capy_id);

    // Run until scout claims the task.
    for _ in 0..100 {
        sim.step(&[], sim.tick + 500);
    }

    // Manually tame the capybara (simulating another player in multiplayer).
    if let Some(mut capy) = sim.db.creatures.get(&capy_id) {
        capy.civ_id = sim.player_civ_id;
        sim.db.update_creature(capy).unwrap();
    }

    // Run a few more ticks — scout should detect target is tamed and complete.
    for _ in 0..20 {
        sim.step(&[], sim.tick + 500);
    }

    // Task should be complete, designation removed.
    let active_tame_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Tame && t.state != TaskState::Complete)
        .collect();
    assert!(
        active_tame_tasks.is_empty(),
        "Tame task should complete when target is already tamed"
    );
    assert!(
        sim.db.tame_designations.get(&capy_id).is_none(),
        "Designation should be removed"
    );
}

#[test]
fn taming_serde_roundtrip_preserves_designations_and_tasks() {
    let mut sim = test_sim(42);
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    designate_tame(&mut sim, capy_id);

    // Verify data exists before roundtrip.
    assert!(sim.db.tame_designations.get(&capy_id).is_some());
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
        restored.db.tame_designations.get(&capy_id).is_some(),
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
