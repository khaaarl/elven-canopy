//! Tests for the creature skills system: skill advancement, decay,
//! intelligence scaling, skill-modified duration, and cap behavior.
//! Corresponds to `sim/skills.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Creature skills (F-creature-skills)
// ---------------------------------------------------------------------------

#[test]
fn creature_skills_default_to_zero() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // All 17 skills should default to 0 (no trait rows inserted at spawn).
    for &tk in &crate::stats::SKILL_TRAIT_KINDS {
        assert_eq!(
            sim.trait_int(elf_id, tk, -1),
            -1,
            "skill {tk:?} should have no trait row (default -1 sentinel)"
        );
    }
}

#[test]
fn try_advance_skill_guaranteed_at_zero() {
    // With 1000 permille (100%) base probability and skill 0, advancement
    // is guaranteed (before INT modifier, INT 0 = 1x = no change).
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Force INT to 0 so it doesn't modify the probability.
    if let Some(mut row) = sim
        .db
        .creature_traits
        .get(&(elf_id, TraitKind::Intelligence))
    {
        row.value = TraitValue::Int(0);
        sim.db.update_creature_trait(row).unwrap();
    }

    sim.try_advance_skill(elf_id, TraitKind::Striking, 1000);
    assert_eq!(
        sim.trait_int(elf_id, TraitKind::Striking, 0),
        1,
        "skill should advance from 0 to 1 with 1000 permille base"
    );
}

#[test]
fn try_advance_skill_blocked_at_cap() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let cap = sim.config.skills.default_skill_cap;

    // Manually set skill to exactly the cap.
    sim.db
        .insert_creature_trait(crate::db::CreatureTrait {
            creature_id: elf_id,
            trait_kind: TraitKind::Striking,
            value: TraitValue::Int(cap),
        })
        .unwrap();
    assert_eq!(sim.trait_int(elf_id, TraitKind::Striking, 0), cap);

    // Even with 1000 permille, should not advance past cap.
    sim.try_advance_skill(elf_id, TraitKind::Striking, 1000);
    assert_eq!(
        sim.trait_int(elf_id, TraitKind::Striking, 0),
        cap,
        "skill should not exceed cap"
    );
}

#[test]
fn try_advance_skill_decay_reduces_probability() {
    // At skill 100 with decay_base 100, adjusted prob = base * 100/200 = base/2.
    // With base 1000 permille, adjusted = 500 permille (50%). Over many trials
    // the success rate should be near 50%, not 100%.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Force INT to 0.
    if let Some(mut row) = sim
        .db
        .creature_traits
        .get(&(elf_id, TraitKind::Intelligence))
    {
        row.value = TraitValue::Int(0);
        sim.db.update_creature_trait(row).unwrap();
    }

    // Set skill to 100 (halfway to cap at default cap 100 — but we need to
    // raise the cap so the creature can advance).
    sim.config.skills.default_skill_cap = 1000;
    sim.db
        .insert_creature_trait(crate::db::CreatureTrait {
            creature_id: elf_id,
            trait_kind: TraitKind::Cuisine,
            value: TraitValue::Int(100),
        })
        .unwrap();

    // Run 1000 trials, resetting skill to 100 each time so the probability
    // stays constant (we're testing the decay formula, not cumulative drift).
    let mut successes = 0;
    for _ in 0..1000 {
        let mut row = sim
            .db
            .creature_traits
            .get(&(elf_id, TraitKind::Cuisine))
            .unwrap();
        row.value = TraitValue::Int(100);
        sim.db.update_creature_trait(row).unwrap();
        sim.try_advance_skill(elf_id, TraitKind::Cuisine, 1000);
        if sim.trait_int(elf_id, TraitKind::Cuisine, 0) > 100 {
            successes += 1;
        }
    }

    // Expected ~50% (500 permille after decay). Allow wide margin for PRNG.
    assert!(
        successes > 400 && successes < 600,
        "expected ~50% advancement rate at skill 100, got {successes}/1000"
    );
}

#[test]
fn try_advance_skill_intelligence_boosts_probability() {
    // INT +100 doubles the advancement probability (2x multiplier).
    // With base 500 permille, skill 0, INT 0: adjusted = 500, ~50% success.
    // With INT +100: adjusted = 1000, 100% success.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Set INT to +100.
    if let Some(mut row) = sim
        .db
        .creature_traits
        .get(&(elf_id, TraitKind::Intelligence))
    {
        row.value = TraitValue::Int(100);
        sim.db.update_creature_trait(row).unwrap();
    } else {
        sim.db
            .insert_creature_trait(crate::db::CreatureTrait {
                creature_id: elf_id,
                trait_kind: TraitKind::Intelligence,
                value: TraitValue::Int(100),
            })
            .unwrap();
    }

    // 500 permille * 2x (INT +100) = 1000 permille = guaranteed.
    sim.try_advance_skill(elf_id, TraitKind::Striking, 500);
    assert_eq!(
        sim.trait_int(elf_id, TraitKind::Striking, 0),
        1,
        "INT +100 should double 500 permille to 1000 (guaranteed)"
    );
}

#[test]
fn try_advance_skill_intelligence_capped_at_1000() {
    // Even with very high INT, probability should cap at 1000 permille.
    // This test just verifies no panic or overflow with extreme INT values.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    if let Some(mut row) = sim
        .db
        .creature_traits
        .get(&(elf_id, TraitKind::Intelligence))
    {
        row.value = TraitValue::Int(500);
        sim.db.update_creature_trait(row).unwrap();
    } else {
        sim.db
            .insert_creature_trait(crate::db::CreatureTrait {
                creature_id: elf_id,
                trait_kind: TraitKind::Intelligence,
                value: TraitValue::Int(500),
            })
            .unwrap();
    }

    sim.try_advance_skill(elf_id, TraitKind::Striking, 1000);
    assert_eq!(
        sim.trait_int(elf_id, TraitKind::Striking, 0),
        1,
        "extreme INT should not cause overflow"
    );
}

#[test]
fn try_advance_skill_deterministic() {
    // Same seed, same sequence of calls → same result.
    // Returns (skill_level, next_rng_value) so we can verify both the
    // outcome and the PRNG state are deterministic.
    let run = |seed| {
        let mut sim = test_sim(seed);
        let elf_id = spawn_creature(&mut sim, Species::Elf);
        set_trait(&mut sim, elf_id, TraitKind::Intelligence, 0);
        sim.config.skills.default_skill_cap = 1000;
        for _ in 0..500 {
            sim.try_advance_skill(elf_id, TraitKind::Striking, 600);
        }
        let skill = sim.trait_int(elf_id, TraitKind::Striking, 0);
        let rng_state = sim.rng.next_u64();
        (skill, rng_state)
    };
    let seed = legacy_test_seed();
    assert_eq!(
        run(seed),
        run(seed),
        "same seed must produce same skill level and PRNG state"
    );
    // Different seeds must produce different PRNG states. Skill levels
    // might coincidentally match (convergent decay), but PRNG states
    // cannot collide.
    let (_, rng_a) = run(seed);
    let (_, rng_b) = run(seed + 1);
    assert_ne!(
        rng_a, rng_b,
        "different seeds should produce different PRNG states"
    );
}

#[test]
fn try_advance_skill_prng_consumed_at_cap() {
    // Verify that try_advance_skill consumes the same number of PRNG calls
    // whether the creature is at cap or not (1 + extra_rolls per call).
    // This keeps the PRNG stream position-stable regardless of game state.
    // Note: this test uses Outcast path (0 extra rolls) → 1 PRNG call each.
    let seed = legacy_test_seed();
    let mut sim_capped = test_sim(seed);
    let mut sim_uncapped = test_sim(seed);
    let elf_capped = spawn_creature(&mut sim_capped, Species::Elf);
    let elf_uncapped = spawn_creature(&mut sim_uncapped, Species::Elf);

    // Set one creature at cap, leave the other at 0.
    let cap = sim_capped.config.skills.default_skill_cap;
    sim_capped
        .db
        .insert_creature_trait(crate::db::CreatureTrait {
            creature_id: elf_capped,
            trait_kind: TraitKind::Striking,
            value: TraitValue::Int(cap),
        })
        .unwrap();

    // Call try_advance_skill on both — should consume 1 PRNG call each.
    sim_capped.try_advance_skill(elf_capped, TraitKind::Striking, 1000);
    sim_uncapped.try_advance_skill(elf_uncapped, TraitKind::Striking, 1000);

    // Next PRNG call should produce the same value in both sims.
    assert_eq!(
        sim_capped.rng.next_u64(),
        sim_uncapped.rng.next_u64(),
        "PRNG streams must stay in sync regardless of cap status"
    );
}

#[test]
fn try_advance_skill_boundary_99_to_100() {
    // Verify advancement from 99 to 100 (just below default cap), then
    // verify no further advancement at 100.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Force INT to 0.
    if let Some(mut row) = sim
        .db
        .creature_traits
        .get(&(elf_id, TraitKind::Intelligence))
    {
        row.value = TraitValue::Int(0);
        sim.db.update_creature_trait(row).unwrap();
    }

    // Set skill to 99.
    sim.db
        .insert_creature_trait(crate::db::CreatureTrait {
            creature_id: elf_id,
            trait_kind: TraitKind::Striking,
            value: TraitValue::Int(99),
        })
        .unwrap();

    // With 1000 permille base and decay 100: adjusted = 1000 * 100/199 ≈ 502.
    // Try many times to ensure at least one success (very likely with 50% chance).
    for _ in 0..20 {
        sim.try_advance_skill(elf_id, TraitKind::Striking, 1000);
    }
    assert_eq!(
        sim.trait_int(elf_id, TraitKind::Striking, 0),
        100,
        "skill should advance to cap (100) but no further"
    );
}

#[test]
fn try_advance_skill_zero_decay_base_no_panic() {
    // Verify that advancement_decay_base = 0 does not cause division by zero.
    // The implementation clamps decay to at least 1.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    sim.config.skills.advancement_decay_base = 0;

    // Should not panic.
    sim.try_advance_skill(elf_id, TraitKind::Striking, 1000);
}

#[test]
fn skill_modified_duration_no_skill_is_identity() {
    // With stat 0 and skill 0, duration should be unchanged (1x multiplier).
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 0);

    let result =
        sim.skill_modified_duration(elf_id, 1000, TraitKind::Dexterity, TraitKind::Cuisine);
    assert_eq!(result, 1000);
}

#[test]
fn skill_modified_duration_skill_100_halves() {
    // Skill 100 with stat 0: apply_stat_divisor(1000, 100) = 500 (2x speed).
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 0);
    set_trait(&mut sim, elf_id, TraitKind::Cuisine, 100);

    let result =
        sim.skill_modified_duration(elf_id, 1000, TraitKind::Dexterity, TraitKind::Cuisine);
    assert_eq!(result, 500, "skill 100 should halve duration");
}

#[test]
fn skill_modified_duration_stat_plus_skill_additive() {
    // DEX +100, Cuisine +100 → combined 200 → 4x speed → 250 ticks.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 100);
    set_trait(&mut sim, elf_id, TraitKind::Cuisine, 100);

    let result =
        sim.skill_modified_duration(elf_id, 1000, TraitKind::Dexterity, TraitKind::Cuisine);
    assert_eq!(
        result, 250,
        "DEX +100 + Cuisine +100 should quarter duration"
    );
}

#[test]
fn skill_modified_duration_minimum_is_one() {
    // Extremely high stat+skill should not produce zero duration.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 600);
    set_trait(&mut sim, elf_id, TraitKind::Cuisine, 600);

    let result = sim.skill_modified_duration(elf_id, 100, TraitKind::Dexterity, TraitKind::Cuisine);
    assert_eq!(result, 1, "extreme stat+skill should clamp to minimum 1");
}

#[test]
fn skill_modified_duration_ignores_cap() {
    // The skill cap only limits learning (advancement), not the speed benefit.
    // A creature with skill 200 and cap 100 should get the full 200 benefit.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 0);
    set_trait(&mut sim, elf_id, TraitKind::Cuisine, 200);
    assert_eq!(sim.config.skills.default_skill_cap, 100);

    // Skill 200 → 4x speed → 1000/4 = 250 ticks.
    // If the cap were applied, skill would clamp to 100 → 2x → 500 ticks.
    let result =
        sim.skill_modified_duration(elf_id, 1000, TraitKind::Dexterity, TraitKind::Cuisine);
    assert_eq!(
        result, 250,
        "speed should use raw skill (200), not capped (100)"
    );
}

// ---------------------------------------------------------------------------
// skill_check (F-skill-check-helper)
// ---------------------------------------------------------------------------

#[test]
fn skill_check_sums_stats_and_skill() {
    // With quasi_normal stdev=50, giving creature very high stats ensures the
    // roll is positive. We test structural properties, not exact values.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Set known stat and skill values (high enough to overwhelm noise).
    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 500);
    set_trait(&mut sim, elf_id, TraitKind::Striking, 300);

    // With DEX=500, Striking=300, base is 800 + quasi_normal(50).
    // quasi_normal(50) has stdev 50, so ~99.7% chance |noise| < 150.
    // The roll should be solidly positive.
    let roll = sim.skill_check(elf_id, &[TraitKind::Dexterity], TraitKind::Striking);
    assert!(
        roll > 600,
        "roll with DEX=500 + Striking=300 should be well above 600, got {roll}"
    );
}

#[test]
fn skill_check_multiple_stats() {
    // Crafting-style check: DEX + INT + PER + skill.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 200);
    set_trait(&mut sim, elf_id, TraitKind::Intelligence, 200);
    set_trait(&mut sim, elf_id, TraitKind::Perception, 200);
    set_trait(&mut sim, elf_id, TraitKind::Herbalism, 100);

    // Base is 200+200+200+100 = 700 + noise (stdev 50).
    let roll = sim.skill_check(
        elf_id,
        &[
            TraitKind::Dexterity,
            TraitKind::Intelligence,
            TraitKind::Perception,
        ],
        TraitKind::Herbalism,
    );
    assert!(
        roll > 500,
        "roll with 3 stats at 200 + skill 100 should be well above 500, got {roll}"
    );
}

#[test]
fn skill_check_zero_stats_centers_near_zero() {
    // With all stats and skill at 0, the roll is just quasi_normal(50),
    // centered at 0. Over many samples, mean should be near 0.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 0);
    set_trait(&mut sim, elf_id, TraitKind::Striking, 0);

    let mut sum: i64 = 0;
    let n = 1000;
    for _ in 0..n {
        sum += sim.skill_check(elf_id, &[TraitKind::Dexterity], TraitKind::Striking);
    }
    let mean = sum / n;
    assert!(
        mean.abs() < 10,
        "mean of 1000 rolls with zero stats should be near 0, got {mean}"
    );
}

#[test]
fn skill_check_empty_stats_slice() {
    // An empty stats slice should sum to 0, so result is skill + noise.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    set_trait(&mut sim, elf_id, TraitKind::Striking, 500);

    let roll = sim.skill_check(elf_id, &[], TraitKind::Striking);
    assert!(
        roll > 300,
        "roll with no stats and skill=500 should be well above 300, got {roll}"
    );
}
