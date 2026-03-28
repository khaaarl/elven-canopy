// Tests for the social opinion system (F-social-opinions).
//
// Covers: OpinionKind serde, CreatureOpinion table CRUD, social skill checks,
// upsert logic, decay, pre-game bootstrap, and save/load roundtrip.

use super::*;
use crate::config::SocialConfig;
use crate::db::CreatureOpinion;
use crate::sim::social::{SkillPicker, social_impression_delta};
use crate::types::OpinionKind;

// ---------------------------------------------------------------------------
// Step 1: Schema tests — OpinionKind serde, table CRUD
// ---------------------------------------------------------------------------

#[test]
fn opinion_kind_serde_roundtrip() {
    for kind in [
        OpinionKind::Friendliness,
        OpinionKind::Respect,
        OpinionKind::Fear,
        OpinionKind::Attraction,
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        let restored: OpinionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, restored, "roundtrip failed for {kind:?}");
    }
}

#[test]
fn creature_opinion_insert_and_query() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    let opinion = CreatureOpinion {
        creature_id: elf_a,
        kind: OpinionKind::Friendliness,
        target_id: elf_b,
        intensity: 10,
    };
    sim.db.upsert_creature_opinion(opinion.clone());

    let row = sim
        .db
        .creature_opinions
        .get(&(elf_a, OpinionKind::Friendliness, elf_b));
    assert!(row.is_some());
    assert_eq!(row.unwrap().intensity, 10);
}

#[test]
fn creature_opinion_asymmetric() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.db.upsert_creature_opinion(CreatureOpinion {
        creature_id: elf_a,
        kind: OpinionKind::Friendliness,
        target_id: elf_b,
        intensity: 15,
    });
    sim.db.upsert_creature_opinion(CreatureOpinion {
        creature_id: elf_b,
        kind: OpinionKind::Friendliness,
        target_id: elf_a,
        intensity: 3,
    });

    let a_of_b = sim
        .db
        .creature_opinions
        .get(&(elf_a, OpinionKind::Friendliness, elf_b))
        .unwrap()
        .intensity;
    let b_of_a = sim
        .db
        .creature_opinions
        .get(&(elf_b, OpinionKind::Friendliness, elf_a))
        .unwrap()
        .intensity;
    assert_eq!(a_of_b, 15);
    assert_eq!(b_of_a, 3);
}

#[test]
fn creature_opinion_multiple_kinds() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.db.upsert_creature_opinion(CreatureOpinion {
        creature_id: elf_a,
        kind: OpinionKind::Friendliness,
        target_id: elf_b,
        intensity: 10,
    });
    sim.db.upsert_creature_opinion(CreatureOpinion {
        creature_id: elf_a,
        kind: OpinionKind::Respect,
        target_id: elf_b,
        intensity: 5,
    });

    let opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    assert_eq!(opinions.len(), 2);
}

#[test]
fn creature_opinion_remove() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.db.upsert_creature_opinion(CreatureOpinion {
        creature_id: elf_a,
        kind: OpinionKind::Friendliness,
        target_id: elf_b,
        intensity: 10,
    });

    let key = (elf_a, OpinionKind::Friendliness, elf_b);
    assert!(sim.db.creature_opinions.get(&key).is_some());
    sim.db
        .remove_creature_opinion(&key)
        .expect("remove should succeed");
    assert!(
        sim.db.creature_opinions.get(&key).is_none(),
        "opinion should be removed"
    );
}

// ---------------------------------------------------------------------------
// Step 2: SocialConfig serde
// ---------------------------------------------------------------------------

#[test]
fn social_config_serde_roundtrip() {
    let cfg = SocialConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let restored: SocialConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        restored.opinion_decay_chance_ppm,
        cfg.opinion_decay_chance_ppm
    );
    assert_eq!(
        restored.bootstrap_interactions_min,
        cfg.bootstrap_interactions_min
    );
    assert_eq!(
        restored.bootstrap_interactions_max,
        cfg.bootstrap_interactions_max
    );
    assert_eq!(
        restored.skill_advance_probability_permille,
        cfg.skill_advance_probability_permille
    );
}

#[test]
fn social_config_default_from_empty_json() {
    let restored: SocialConfig = serde_json::from_str("{}").unwrap();
    let defaults = SocialConfig::default();
    assert_eq!(
        restored.opinion_decay_chance_ppm,
        defaults.opinion_decay_chance_ppm
    );
    assert_eq!(
        restored.bootstrap_interactions_min,
        defaults.bootstrap_interactions_min
    );
}

// ---------------------------------------------------------------------------
// Step 3: Social skill check
// ---------------------------------------------------------------------------

#[test]
fn social_impression_delta_high_cha_mostly_positive() {
    // With CHA=100 and skill=0, roll centers at 100 — almost always > 50.
    let mut rng = elven_canopy_prng::GameRng::new(42);
    let mut total = 0i64;
    for _ in 0..100 {
        total += social_impression_delta(100, 0, &mut rng);
    }
    // With center at 100 and stdev 50, the vast majority should be +2.
    assert!(
        total > 150,
        "high CHA should produce mostly +2 deltas, got sum {total}"
    );
}

#[test]
fn social_impression_delta_low_cha_mostly_negative() {
    // With CHA=-100 and skill=0, roll centers at -100 — almost always ≤ -50.
    let mut rng = elven_canopy_prng::GameRng::new(42);
    let mut total = 0i64;
    for _ in 0..100 {
        total += social_impression_delta(-100, 0, &mut rng);
    }
    assert!(
        total < -50,
        "low CHA should produce mostly -1 deltas, got sum {total}"
    );
}

#[test]
fn social_impression_delta_deterministic() {
    let mut rng1 = elven_canopy_prng::GameRng::new(99);
    let mut rng2 = elven_canopy_prng::GameRng::new(99);
    for _ in 0..20 {
        let d1 = social_impression_delta(30, 5, &mut rng1);
        let d2 = social_impression_delta(30, 5, &mut rng2);
        assert_eq!(d1, d2);
    }
}

#[test]
fn social_skill_value_best_social_picks_higher() {
    let mut sim = test_sim(42);
    let elf = spawn_creature(&mut sim, Species::Elf);

    // Set Influence=20, Culture=10 — BestSocial should pick Influence.
    use crate::db::CreatureTrait;
    sim.db.upsert_creature_trait(CreatureTrait {
        creature_id: elf,
        trait_kind: TraitKind::Influence,
        value: TraitValue::Int(20),
    });
    sim.db.upsert_creature_trait(CreatureTrait {
        creature_id: elf,
        trait_kind: TraitKind::Culture,
        value: TraitValue::Int(10),
    });
    assert_eq!(sim.social_skill_value(elf, SkillPicker::BestSocial), 20);

    // Flip: Culture=30 — BestSocial should now pick Culture.
    sim.db.upsert_creature_trait(CreatureTrait {
        creature_id: elf,
        trait_kind: TraitKind::Culture,
        value: TraitValue::Int(30),
    });
    assert_eq!(sim.social_skill_value(elf, SkillPicker::BestSocial), 30);

    // Culture picker always picks Culture regardless of Influence.
    assert_eq!(sim.social_skill_value(elf, SkillPicker::Culture), 30);
}

// ---------------------------------------------------------------------------
// Step 4: Upsert opinion helper
// ---------------------------------------------------------------------------

#[test]
fn upsert_opinion_creates_new_row() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 5);

    let row = sim
        .db
        .creature_opinions
        .get(&(elf_a, OpinionKind::Friendliness, elf_b))
        .unwrap();
    assert_eq!(row.intensity, 5);
}

#[test]
fn upsert_opinion_accumulates() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 5);
    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 3);

    let row = sim
        .db
        .creature_opinions
        .get(&(elf_a, OpinionKind::Friendliness, elf_b))
        .unwrap();
    assert_eq!(row.intensity, 8);
}

#[test]
fn upsert_opinion_prunes_zero() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 5);
    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, -5);

    assert!(
        sim.db
            .creature_opinions
            .get(&(elf_a, OpinionKind::Friendliness, elf_b))
            .is_none(),
        "zero-intensity row should be pruned"
    );
}

#[test]
fn upsert_opinion_zero_delta_is_noop() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 0);

    assert!(
        sim.db
            .creature_opinions
            .get(&(elf_a, OpinionKind::Friendliness, elf_b))
            .is_none(),
        "zero delta should not create a row"
    );
}

// ---------------------------------------------------------------------------
// Step 5: Decay
// ---------------------------------------------------------------------------

#[test]
fn decay_positive_toward_zero() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 3);
    sim.decay_opinions(elf_a);

    let row = sim
        .db
        .creature_opinions
        .get(&(elf_a, OpinionKind::Friendliness, elf_b))
        .unwrap();
    assert_eq!(row.intensity, 2);
}

#[test]
fn decay_negative_toward_zero() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, -3);
    sim.decay_opinions(elf_a);

    let row = sim
        .db
        .creature_opinions
        .get(&(elf_a, OpinionKind::Friendliness, elf_b))
        .unwrap();
    assert_eq!(row.intensity, -2);
}

#[test]
fn decay_prunes_at_zero() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 1);
    sim.decay_opinions(elf_a);

    assert!(
        sim.db
            .creature_opinions
            .get(&(elf_a, OpinionKind::Friendliness, elf_b))
            .is_none(),
        "intensity 1 should decay to 0 and be pruned"
    );
}

#[test]
fn decay_multiple_kinds() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 5);
    sim.upsert_opinion(elf_a, OpinionKind::Respect, elf_b, -2);
    sim.decay_opinions(elf_a);

    assert_eq!(
        sim.db
            .creature_opinions
            .get(&(elf_a, OpinionKind::Friendliness, elf_b))
            .unwrap()
            .intensity,
        4
    );
    assert_eq!(
        sim.db
            .creature_opinions
            .get(&(elf_a, OpinionKind::Respect, elf_b))
            .unwrap()
            .intensity,
        -1
    );
}

#[test]
fn decay_fires_via_heartbeat_over_time() {
    // Set decay chance to 100% so every heartbeat triggers decay.
    let mut config = test_config();
    config.social.opinion_decay_chance_ppm = 1_000_000; // 100%
    // Disable food/rest decay so the elf stays idle and doesn't complicate things.
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let heartbeat_interval = elf_species.heartbeat_interval_ticks;
    let mut sim = SimState::with_config(99, config);

    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 5);

    // Advance enough ticks for several heartbeats to fire.
    let target_tick = sim.tick + heartbeat_interval * 10;
    sim.step(&[], target_tick);

    // With 100% decay chance, intensity should have decreased from 5.
    let remaining = sim
        .db
        .creature_opinions
        .get(&(elf_a, OpinionKind::Friendliness, elf_b))
        .map(|r| r.intensity)
        .unwrap_or(0);
    assert!(
        remaining < 5,
        "intensity should have decayed from 5, got {remaining}"
    );
}

// ---------------------------------------------------------------------------
// Step 6: Pre-game bootstrap
// ---------------------------------------------------------------------------

#[test]
fn bootstrap_creates_opinions_between_starting_elves() {
    // Use a config with exactly 3 starting elves.
    let mut config = test_config();
    config.initial_creatures.clear();
    config
        .initial_creatures
        .push(crate::config::InitialCreatureSpec {
            species: Species::Elf,
            count: 3,
            spawn_position: VoxelCoord::new(32, 1, 32),
            food_pcts: vec![],
            rest_pcts: vec![],
            bread_counts: vec![],
            initial_equipment: vec![],
        });
    let mut sim = SimState::with_config(77, config);
    let mut events = vec![];
    sim.spawn_initial_creatures(&mut events);

    // Collect all elf IDs.
    let elves: Vec<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .map(|c| c.id)
        .collect();
    assert_eq!(elves.len(), 3, "should have 3 starting elves");

    // Each ordered pair should have a Friendliness opinion.
    for &a in &elves {
        for &b in &elves {
            if a == b {
                continue;
            }
            let opinion = sim
                .db
                .creature_opinions
                .get(&(a, OpinionKind::Friendliness, b));
            assert!(
                opinion.is_some(),
                "elf {a:?} should have a Friendliness opinion of {b:?}"
            );
        }
    }
}

#[test]
fn bootstrap_opinions_are_asymmetric() {
    let mut config = test_config();
    config.initial_creatures.clear();
    config
        .initial_creatures
        .push(crate::config::InitialCreatureSpec {
            species: Species::Elf,
            count: 2,
            spawn_position: VoxelCoord::new(32, 1, 32),
            food_pcts: vec![],
            rest_pcts: vec![],
            bread_counts: vec![],
            initial_equipment: vec![],
        });
    let mut sim = SimState::with_config(55, config);
    let mut events = vec![];
    sim.spawn_initial_creatures(&mut events);

    let elves: Vec<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .map(|c| c.id)
        .collect();
    assert_eq!(elves.len(), 2);

    let a_of_b = sim
        .db
        .creature_opinions
        .get(&(elves[0], OpinionKind::Friendliness, elves[1]))
        .map(|r| r.intensity);
    let b_of_a = sim
        .db
        .creature_opinions
        .get(&(elves[1], OpinionKind::Friendliness, elves[0]))
        .map(|r| r.intensity);

    // Both should exist and likely differ (different CHA, different random rolls).
    assert!(a_of_b.is_some(), "A should have opinion of B");
    assert!(b_of_a.is_some(), "B should have opinion of A");
    // Don't assert inequality — they could rarely be equal by chance.
}

#[test]
fn bootstrap_advances_social_skills() {
    let mut config = test_config();
    config.initial_creatures.clear();
    config
        .initial_creatures
        .push(crate::config::InitialCreatureSpec {
            species: Species::Elf,
            count: 3,
            spawn_position: VoxelCoord::new(32, 1, 32),
            food_pcts: vec![],
            rest_pcts: vec![],
            bread_counts: vec![],
            initial_equipment: vec![],
        });
    // Increase bootstrap count to make skill advancement likely.
    config.social.bootstrap_interactions_min = 20;
    config.social.bootstrap_interactions_max = 30;
    let mut sim = SimState::with_config(88, config);
    let mut events = vec![];
    sim.spawn_initial_creatures(&mut events);

    let elves: Vec<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .map(|c| c.id)
        .collect();

    // At least one elf should have gained some Influence or Culture from bootstrap.
    let any_advanced = elves.iter().any(|&elf| {
        let influence = sim.trait_int(elf, TraitKind::Influence, 0);
        let culture = sim.trait_int(elf, TraitKind::Culture, 0);
        influence > 0 || culture > 0
    });
    assert!(
        any_advanced,
        "at least one elf should have advanced a social skill during bootstrap"
    );
}

// ---------------------------------------------------------------------------
// Step 7: Save/load roundtrip
// ---------------------------------------------------------------------------

#[test]
fn opinion_serde_roundtrip_via_simstate() {
    let mut sim = test_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 12);
    sim.upsert_opinion(elf_a, OpinionKind::Respect, elf_b, -3);

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let f = restored
        .db
        .creature_opinions
        .get(&(elf_a, OpinionKind::Friendliness, elf_b))
        .unwrap();
    assert_eq!(f.intensity, 12);

    let r = restored
        .db
        .creature_opinions
        .get(&(elf_a, OpinionKind::Respect, elf_b))
        .unwrap();
    assert_eq!(r.intensity, -3);
}

// ---------------------------------------------------------------------------
// Additional edge-case tests (once-over)
// ---------------------------------------------------------------------------

#[test]
fn decay_no_opinions_is_noop() {
    let mut sim = test_sim(42);
    let elf = spawn_creature(&mut sim, Species::Elf);
    // Should not panic or error when there are no opinion rows.
    sim.decay_opinions(elf);
}

#[test]
fn bootstrap_single_elf_creates_no_opinions() {
    let mut sim = test_sim(42);
    let elf = spawn_creature(&mut sim, Species::Elf);
    sim.bootstrap_social_opinions(&[elf]);
    let opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf, tabulosity::QueryOpts::ASC);
    assert!(opinions.is_empty(), "single elf should produce no opinions");
}
