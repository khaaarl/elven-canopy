// Tests for the social opinion system (F-social-opinions).
//
// Covers: OpinionKind serde, CreatureOpinion table CRUD, social skill checks,
// upsert logic, decay, pre-game bootstrap, and save/load roundtrip.

use super::*;
use crate::config::SocialConfig;
use crate::db::CreatureOpinion;
use crate::sim::social::{SkillPicker, social_impression_delta};
use crate::types::{FriendshipCategory, OpinionKind, TraitKind};

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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
fn social_impression_delta_maps_roll_to_buckets() {
    // social_impression_delta now takes a pre-computed roll and maps to
    // the four intensity delta buckets.
    assert_eq!(social_impression_delta(100), 2); // > 50
    assert_eq!(social_impression_delta(51), 2); // > 50
    assert_eq!(social_impression_delta(50), 1); // 1–50
    assert_eq!(social_impression_delta(1), 1); // 1–50
    assert_eq!(social_impression_delta(0), 0); // -49–0
    assert_eq!(social_impression_delta(-49), 0); // -49–0
    assert_eq!(social_impression_delta(-50), -1); // ≤ -50
    assert_eq!(social_impression_delta(-200), -1); // ≤ -50
}

#[test]
fn social_impression_high_cha_via_skill_check() {
    // With CHA=100 and skill=0, skill_check centers at 100 — almost always > 50.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_creature(&mut sim, Species::Elf);
    set_trait(&mut sim, elf, TraitKind::Charisma, 100);
    set_trait(&mut sim, elf, TraitKind::Influence, 0);

    let mut total = 0i64;
    for _ in 0..100 {
        let roll = sim.skill_check(elf, &[TraitKind::Charisma], TraitKind::Influence);
        total += social_impression_delta(roll);
    }
    assert!(
        total > 150,
        "high CHA should produce mostly +2 deltas, got sum {total}"
    );
}

#[test]
fn social_impression_low_cha_via_skill_check() {
    // With CHA=-100 and skill=0, skill_check centers at -100 — almost always ≤ -50.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_creature(&mut sim, Species::Elf);
    set_trait(&mut sim, elf, TraitKind::Charisma, -100);
    set_trait(&mut sim, elf, TraitKind::Influence, 0);

    let mut total = 0i64;
    for _ in 0..100 {
        let roll = sim.skill_check(elf, &[TraitKind::Charisma], TraitKind::Influence);
        total += social_impression_delta(roll);
    }
    assert!(
        total < -50,
        "low CHA should produce mostly -1 deltas, got sum {total}"
    );
}

#[test]
fn social_skill_trait_equal_values_picks_influence() {
    // When Influence == Culture, BestSocial picks Influence (>= tie-break).
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_creature(&mut sim, Species::Elf);
    set_trait(&mut sim, elf, TraitKind::Influence, 50);
    set_trait(&mut sim, elf, TraitKind::Culture, 50);
    assert_eq!(
        sim.social_skill_trait(elf, SkillPicker::BestSocial),
        TraitKind::Influence,
        "equal Influence and Culture should tie-break to Influence"
    );
}

#[test]
fn social_skill_trait_best_social_picks_higher() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_creature(&mut sim, Species::Elf);

    // Set Influence=20, Culture=10 — BestSocial should pick Influence.
    set_trait(&mut sim, elf, TraitKind::Influence, 20);
    set_trait(&mut sim, elf, TraitKind::Culture, 10);
    assert_eq!(
        sim.social_skill_trait(elf, SkillPicker::BestSocial),
        TraitKind::Influence
    );

    // Flip: Culture=30 — BestSocial should now pick Culture.
    set_trait(&mut sim, elf, TraitKind::Culture, 30);
    assert_eq!(
        sim.social_skill_trait(elf, SkillPicker::BestSocial),
        TraitKind::Culture
    );

    // Culture picker always picks Culture regardless of Influence.
    assert_eq!(
        sim.social_skill_trait(elf, SkillPicker::Culture),
        TraitKind::Culture
    );
}

// ---------------------------------------------------------------------------
// Step 4: Upsert opinion helper
// ---------------------------------------------------------------------------

#[test]
fn upsert_opinion_creates_new_row() {
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = SimState::with_config(legacy_test_seed(), config);

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
    // Use a very high, fixed interaction count so opinion sums are reliably
    // nonzero regardless of PRNG-rolled CHA/skills (genome-derived stats can
    // produce extreme CHA values that make individual rolls unlikely).
    config.social.bootstrap_interactions_min = 200;
    config.social.bootstrap_interactions_max = 200;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
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
    // High interaction count so opinions survive even with unlucky rolls
    // (rolls in -49..=0 produce delta=0, and +1/-1 deltas can cancel out;
    // with low CHA the net sum can land on exactly 0, removing the row).
    config.social.bootstrap_interactions_min = 200;
    config.social.bootstrap_interactions_max = 200;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
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
    let mut sim = SimState::with_config(legacy_test_seed(), config);
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
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_creature(&mut sim, Species::Elf);
    // Should not panic or error when there are no opinion rows.
    sim.decay_opinions(elf);
}

// ---------------------------------------------------------------------------
// Friendship categories (F-casual-social)
// ---------------------------------------------------------------------------
#[test]
fn friendship_category_default_thresholds() {
    let sim = test_sim(legacy_test_seed());
    assert_eq!(sim.friendship_category(0), FriendshipCategory::Neutral);
    assert_eq!(sim.friendship_category(4), FriendshipCategory::Neutral);
    assert_eq!(sim.friendship_category(-4), FriendshipCategory::Neutral);
    assert_eq!(sim.friendship_category(5), FriendshipCategory::Acquaintance);
    assert_eq!(
        sim.friendship_category(14),
        FriendshipCategory::Acquaintance
    );
    assert_eq!(sim.friendship_category(15), FriendshipCategory::Friend);
    assert_eq!(sim.friendship_category(100), FriendshipCategory::Friend);
    assert_eq!(sim.friendship_category(-5), FriendshipCategory::Disliked);
    assert_eq!(sim.friendship_category(-14), FriendshipCategory::Disliked);
    assert_eq!(sim.friendship_category(-15), FriendshipCategory::Enemy);
    assert_eq!(sim.friendship_category(-100), FriendshipCategory::Enemy);
}

#[test]
fn friendship_category_custom_thresholds() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.social.friendship_acquaintance_threshold = 10;
    sim.config.social.friendship_friend_threshold = 30;
    sim.config.social.friendship_disliked_threshold = -10;
    sim.config.social.friendship_enemy_threshold = -30;

    assert_eq!(sim.friendship_category(9), FriendshipCategory::Neutral);
    assert_eq!(
        sim.friendship_category(10),
        FriendshipCategory::Acquaintance
    );
    assert_eq!(
        sim.friendship_category(29),
        FriendshipCategory::Acquaintance
    );
    assert_eq!(sim.friendship_category(30), FriendshipCategory::Friend);
    assert_eq!(sim.friendship_category(-9), FriendshipCategory::Neutral);
    assert_eq!(sim.friendship_category(-10), FriendshipCategory::Disliked);
    assert_eq!(sim.friendship_category(-29), FriendshipCategory::Disliked);
    assert_eq!(sim.friendship_category(-30), FriendshipCategory::Enemy);
}

#[test]
fn friendship_category_label_text() {
    assert_eq!(FriendshipCategory::Friend.label(), "Friend");
    assert_eq!(FriendshipCategory::Acquaintance.label(), "Acquaintance");
    assert_eq!(FriendshipCategory::Neutral.label(), "");
    assert_eq!(FriendshipCategory::Disliked.label(), "Disliked");
    assert_eq!(FriendshipCategory::Enemy.label(), "Enemy");
}

#[test]
fn friendship_category_serde_roundtrip() {
    for cat in [
        FriendshipCategory::Enemy,
        FriendshipCategory::Disliked,
        FriendshipCategory::Neutral,
        FriendshipCategory::Acquaintance,
        FriendshipCategory::Friend,
    ] {
        let json = serde_json::to_string(&cat).unwrap();
        let restored: FriendshipCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, restored, "roundtrip failed for {cat:?}");
    }
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

#[test]
fn bootstrap_single_elf_creates_no_opinions() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_creature(&mut sim, Species::Elf);
    sim.bootstrap_social_opinions(&[elf]);
    let opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf, tabulosity::QueryOpts::ASC);
    assert!(opinions.is_empty(), "single elf should produce no opinions");
}

// ---------------------------------------------------------------------------
// Friendship threshold-crossing notifications (F-casual-social)
// ---------------------------------------------------------------------------

#[test]
fn upsert_opinion_notifies_on_threshold_cross() {
    let mut sim = test_sim(legacy_test_seed());
    // Disable bootstrap to avoid pre-existing opinions.
    sim.config.social.bootstrap_interactions_min = 0;
    sim.config.social.bootstrap_interactions_max = 0;
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    // Push intensity from 0 to 5 — crosses into Acquaintance.
    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 5);

    let notifications: Vec<_> = sim.db.notifications.iter_all().collect();
    assert!(
        notifications
            .iter()
            .any(|n| n.message.contains("Acquaintance")),
        "should notify on crossing into Acquaintance: {:?}",
        notifications
    );
}

#[test]
fn upsert_opinion_no_notify_within_same_category() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.social.bootstrap_interactions_min = 0;
    sim.config.social.bootstrap_interactions_max = 0;
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    // Push to 5 (Acquaintance) then to 6 (still Acquaintance).
    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 5);
    let count_after_first = sim.db.notifications.iter_all().count();

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 1);
    let count_after_second = sim.db.notifications.iter_all().count();

    assert_eq!(
        count_after_first, count_after_second,
        "no notification when staying in same category"
    );
}

#[test]
fn upsert_opinion_notifies_friend_threshold() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.social.bootstrap_interactions_min = 0;
    sim.config.social.bootstrap_interactions_max = 0;
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    // Jump straight to Friend threshold.
    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 15);

    let notifications: Vec<_> = sim.db.notifications.iter_all().collect();
    assert!(
        notifications.iter().any(|n| n.message.contains("Friend")),
        "should notify on crossing into Friend: {:?}",
        notifications
    );
}

#[test]
fn upsert_opinion_notifies_negative_threshold() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.social.bootstrap_interactions_min = 0;
    sim.config.social.bootstrap_interactions_max = 0;
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    // Push to -5 — crosses into Disliked.
    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, -5);

    let notifications: Vec<_> = sim.db.notifications.iter_all().collect();
    assert!(
        notifications.iter().any(|n| n.message.contains("Disliked")),
        "should notify on crossing into Disliked: {:?}",
        notifications
    );
}

#[test]
fn upsert_opinion_no_notify_for_neutral_return() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.social.bootstrap_interactions_min = 0;
    sim.config.social.bootstrap_interactions_max = 0;
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    // Push to Acquaintance (5), then back to Neutral (0).
    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 5);
    let count_after_acquire = sim.db.notifications.iter_all().count();

    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, -5);
    let count_after_neutral = sim.db.notifications.iter_all().count();

    // Crossing back to Neutral should NOT generate a notification
    // (Neutral label is empty, so no notification).
    assert_eq!(
        count_after_acquire, count_after_neutral,
        "returning to Neutral should not notify"
    );
}

// ---------------------------------------------------------------------------
// Casual chat thought serde (F-casual-social)
// ---------------------------------------------------------------------------

#[test]
fn casual_chat_thought_serde_roundtrip() {
    let pleasant = ThoughtKind::HadPleasantChat("Aelindra".into());
    let json = serde_json::to_string(&pleasant).unwrap();
    let restored: ThoughtKind = serde_json::from_str(&json).unwrap();
    assert_eq!(pleasant, restored);

    let awkward = ThoughtKind::HadAwkwardChat("Thaeron".into());
    let json = serde_json::to_string(&awkward).unwrap();
    let restored: ThoughtKind = serde_json::from_str(&json).unwrap();
    assert_eq!(awkward, restored);
}

// ---------------------------------------------------------------------------
// Casual social interactions (F-casual-social)
// ---------------------------------------------------------------------------

/// Helper: disable bootstrap opinions and set casual social to fire every
/// heartbeat (100% chance) for deterministic testing.
fn setup_casual_social_sim(seed: u64) -> crate::sim::SimState {
    let mut sim = flat_world_sim(seed);
    // Disable bootstrap so starting opinions don't interfere.
    sim.config.social.bootstrap_interactions_min = 0;
    sim.config.social.bootstrap_interactions_max = 0;
    // 100% trigger chance for deterministic tests.
    sim.config.social.casual_social_chance_ppm = 1_000_000;
    sim.config.social.casual_social_radius = 3;
    sim
}

#[test]
fn casual_social_nearby_elves_gain_opinions() {
    let mut sim = setup_casual_social_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    // Both elves spawn at the tree position — same voxel, well within radius.
    // Directly call try_casual_social for elf_a.
    sim.try_casual_social(elf_a);

    // Both creatures should have opinions about each other (bidirectional).
    let a_opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    let b_opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_b, tabulosity::QueryOpts::ASC);

    assert!(
        !a_opinions.is_empty(),
        "elf_a should have an opinion about elf_b"
    );
    assert!(
        !b_opinions.is_empty(),
        "elf_b should have an opinion about elf_a"
    );

    // Verify the opinion targets are correct.
    assert!(
        a_opinions
            .iter()
            .any(|o| o.target_id == elf_b && o.kind == OpinionKind::Friendliness)
    );
    assert!(
        b_opinions
            .iter()
            .any(|o| o.target_id == elf_a && o.kind == OpinionKind::Friendliness)
    );
}

#[test]
fn casual_social_awards_thoughts() {
    let mut sim = setup_casual_social_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let _elf_b = spawn_creature(&mut sim, Species::Elf);

    sim.try_casual_social(elf_a);

    // At least one of the two elves should have a chat thought.
    let a_thoughts: Vec<_> = sim
        .db
        .thoughts
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    let b_thoughts: Vec<_> = sim
        .db
        .thoughts
        .by_creature_id(&_elf_b, tabulosity::QueryOpts::ASC);

    let has_chat_thought = |thoughts: &[crate::db::Thought]| {
        thoughts.iter().any(|t| {
            matches!(
                &t.kind,
                ThoughtKind::HadPleasantChat(_) | ThoughtKind::HadAwkwardChat(_)
            )
        })
    };

    // Both should have thoughts (both got nonzero deltas) or at least one
    // should — delta=0 produces no thought, but with skill checks the
    // probability of both being exactly 0 is low.
    assert!(
        has_chat_thought(&a_thoughts) || has_chat_thought(&b_thoughts),
        "at least one elf should have a chat thought"
    );
}

#[test]
fn casual_social_no_interaction_when_alone() {
    let mut sim = setup_casual_social_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    // Only one elf — no one to interact with.

    sim.try_casual_social(elf_a);

    let opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    assert!(
        opinions.is_empty(),
        "lone elf should have no opinions from casual social"
    );
}

#[test]
fn casual_social_no_interaction_different_civ() {
    let mut sim = setup_casual_social_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    // Spawn a non-civ creature (e.g., capybara) nearby — different civ.
    let _capybara = spawn_creature(&mut sim, Species::Capybara);

    sim.try_casual_social(elf_a);

    let opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    assert!(
        opinions.is_empty(),
        "elf should not casually interact with a non-civ creature"
    );
}

#[test]
fn casual_social_fires_via_heartbeat() {
    let mut sim = setup_casual_social_sim(42);
    // Disable opinion decay so it doesn't interfere.
    sim.config.social.opinion_decay_chance_ppm = 0;
    // Use a very large radius so elves stay in range regardless of wandering.
    sim.config.social.casual_social_radius = 100;
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    // Both elves start with no opinions (bootstrap disabled).
    let a_opinions_before: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    let b_opinions_before: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert!(
        a_opinions_before.is_empty() && b_opinions_before.is_empty(),
        "no opinions before heartbeat"
    );

    // Run enough ticks for several heartbeats (elf heartbeat ~3000 ticks).
    // With 100% chance, every heartbeat should trigger casual social.
    let target_tick = sim.tick + 10_000;
    sim.step(&[], target_tick);

    // After several heartbeats, opinions should have formed.
    let a_opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    let b_opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_b, tabulosity::QueryOpts::ASC);

    assert!(
        !a_opinions.is_empty() || !b_opinions.is_empty(),
        "heartbeat should trigger casual social and produce opinions"
    );
}

#[test]
fn casual_social_advances_skills() {
    let mut sim = setup_casual_social_sim(42);
    // 100% skill advancement chance for this test.
    sim.config.social.skill_advance_probability_permille = 1000;
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    let influence_before_a = sim.trait_int(elf_a, TraitKind::Influence, 0);
    let culture_before_a = sim.trait_int(elf_a, TraitKind::Culture, 0);
    let influence_before_b = sim.trait_int(elf_b, TraitKind::Influence, 0);
    let culture_before_b = sim.trait_int(elf_b, TraitKind::Culture, 0);

    // Run several interactions.
    for _ in 0..10 {
        sim.try_casual_social(elf_a);
    }

    let influence_after_a = sim.trait_int(elf_a, TraitKind::Influence, 0);
    let culture_after_a = sim.trait_int(elf_a, TraitKind::Culture, 0);
    let influence_after_b = sim.trait_int(elf_b, TraitKind::Influence, 0);
    let culture_after_b = sim.trait_int(elf_b, TraitKind::Culture, 0);

    let a_advanced =
        (influence_after_a > influence_before_a) || (culture_after_a > culture_before_a);
    let b_advanced =
        (influence_after_b > influence_before_b) || (culture_after_b > culture_before_b);

    assert!(
        a_advanced && b_advanced,
        "both creatures should advance social skills with 100% chance"
    );
}

#[test]
fn casual_social_out_of_range_no_interaction() {
    let mut sim = setup_casual_social_sim(42);
    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    // Move elf_b far away.
    let far_pos = VoxelCoord::new(100, 51, 100);
    let old_pos = sim.db.creatures.get(&elf_b).unwrap().position;
    if let Some(mut c) = sim.db.creatures.get(&elf_b) {
        c.position = far_pos;
        let _ = sim.db.update_creature(c);
    }
    sim.update_creature_spatial_index(elf_b, Species::Elf, old_pos, far_pos);

    sim.try_casual_social(elf_a);

    let opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    assert!(
        opinions.is_empty(),
        "elf should not interact with out-of-range creature"
    );
}
