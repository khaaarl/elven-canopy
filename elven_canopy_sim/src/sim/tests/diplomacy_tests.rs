//! Tests for the diplomacy system: civ relationships, discovery,
//! opinion, diplomatic relations, creature-level relations, and
//! the is_non_hostile predicate.
//! Corresponds to `sim/diplomacy.rs`.

use super::*;

// -------------------------------------------------------------------
// Civilization tests
// -------------------------------------------------------------------

#[test]
fn spawned_elf_gets_player_civ_id() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(
        elf.civ_id, sim.player_civ_id,
        "Spawned elf should belong to the player's civilization"
    );
}

#[test]
fn spawned_non_elf_has_no_civ_id() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Capybara,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let capy = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .unwrap();
    assert_eq!(
        capy.civ_id, None,
        "Non-elf creature should not have a civ_id"
    );
}

#[test]
fn discover_civ_creates_relationship() {
    let mut sim = test_sim(42);

    // Get two existing civ IDs from worldgen.
    let civs: Vec<_> = sim.db.civilizations.iter_all().collect();
    assert!(civs.len() >= 2, "Need at least 2 civs for this test");

    let civ_a = civs[0].id;
    let civ_b = civs[1].id;

    // Remove any existing relationship between a→b from worldgen.
    let existing: Vec<_> = sim
        .db
        .civ_relationships
        .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.to_civ == civ_b)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in existing {
        let _ = sim.db.civ_relationships.remove_no_fk(&pk);
    }

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DiscoverCiv {
            civ_id: civ_a,
            discovered_civ: civ_b,
            initial_opinion: CivOpinion::Neutral,
        },
    };
    sim.step(&[cmd], 1);

    let rels = sim
        .db
        .civ_relationships
        .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC);
    let found = rels.iter().any(|r| r.to_civ == civ_b);
    assert!(found, "DiscoverCiv should create a relationship from a→b");
}

#[test]
fn discover_civ_is_idempotent() {
    let mut sim = test_sim(42);
    let civs: Vec<_> = sim.db.civilizations.iter_all().collect();
    assert!(civs.len() >= 2);

    let civ_a = civs[0].id;
    let civ_b = civs[1].id;

    // Remove existing relationship.
    let existing: Vec<_> = sim
        .db
        .civ_relationships
        .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.to_civ == civ_b)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in existing {
        let _ = sim.db.civ_relationships.remove_no_fk(&pk);
    }

    // Discover twice.
    for tick in [1, 2] {
        let cmd = SimCommand {
            player_name: String::new(),
            tick,
            action: SimAction::DiscoverCiv {
                civ_id: civ_a,
                discovered_civ: civ_b,
                initial_opinion: CivOpinion::Neutral,
            },
        };
        sim.step(&[cmd], tick);
    }

    let rels = sim
        .db
        .civ_relationships
        .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC);
    let count = rels.iter().filter(|r| r.to_civ == civ_b).count();
    assert_eq!(
        count, 1,
        "DiscoverCiv should not create duplicate relationships"
    );
}

#[test]
fn discover_civ_noop_for_nonexistent_civ() {
    let mut sim = test_sim(42);
    let rel_count_before = sim.db.civ_relationships.iter_all().count();

    // Use a CivId that doesn't exist.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DiscoverCiv {
            civ_id: CivId(999),
            discovered_civ: CivId(0),
            initial_opinion: CivOpinion::Neutral,
        },
    };
    sim.step(&[cmd], 1);

    let rel_count_after = sim.db.civ_relationships.iter_all().count();
    assert_eq!(
        rel_count_before, rel_count_after,
        "No-op for nonexistent civ"
    );
}

#[test]
fn set_civ_opinion_updates_relationship() {
    let mut sim = test_sim(42);

    // Find an existing relationship from worldgen.
    let rel = sim.db.civ_relationships.iter_all().next();
    assert!(
        rel.is_some(),
        "Need at least one relationship for this test"
    );
    let rel = rel.unwrap();
    let from_civ = rel.from_civ;
    let to_civ = rel.to_civ;
    let new_opinion = if rel.opinion == CivOpinion::Hostile {
        CivOpinion::Friendly
    } else {
        CivOpinion::Hostile
    };

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SetCivOpinion {
            civ_id: from_civ,
            target_civ: to_civ,
            opinion: new_opinion,
        },
    };
    sim.step(&[cmd], 1);

    let updated = sim.db.civ_relationships.get(&(from_civ, to_civ)).unwrap();
    assert_eq!(updated.opinion, new_opinion, "Opinion should be updated");
}

#[test]
fn set_civ_opinion_noop_for_unknown_pair() {
    let mut sim = test_sim(42);

    // Use a CivId pair with no relationship.
    // CivId(999) doesn't exist, so this should be a no-op.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SetCivOpinion {
            civ_id: CivId(999),
            target_civ: CivId(0),
            opinion: CivOpinion::Hostile,
        },
    };
    sim.step(&[cmd], 1);
    // No panic = success.
}

#[test]
fn get_known_civs_returns_player_relationships() {
    let sim = test_sim(42);

    let known = sim.get_known_civs();
    // Should contain entries from worldgen diplomacy (player civ's outgoing rels).
    let player_rels = sim
        .db
        .civ_relationships
        .by_from_civ(&CivId(0), tabulosity::QueryOpts::ASC);

    assert_eq!(
        known.len(),
        player_rels.len(),
        "get_known_civs should return one entry per player-outgoing relationship"
    );
}

#[test]
fn civ_opinion_serde_roundtrip() {
    use crate::types::CivOpinion;
    for &opinion in &[
        CivOpinion::Friendly,
        CivOpinion::Neutral,
        CivOpinion::Suspicious,
        CivOpinion::Hostile,
    ] {
        let json = serde_json::to_string(&opinion).unwrap();
        let restored: CivOpinion = serde_json::from_str(&json).unwrap();
        assert_eq!(opinion, restored);
    }
}

#[test]
fn civ_species_serde_roundtrip() {
    use crate::types::CivSpecies;
    for &species in CivSpecies::ALL.iter() {
        let json = serde_json::to_string(&species).unwrap();
        let restored: CivSpecies = serde_json::from_str(&json).unwrap();
        assert_eq!(species, restored);
    }
}

#[test]
fn culture_tag_serde_roundtrip() {
    use crate::types::CultureTag;
    for &tag in &[
        CultureTag::Woodland,
        CultureTag::Coastal,
        CultureTag::Mountain,
        CultureTag::Nomadic,
        CultureTag::Subterranean,
        CultureTag::Martial,
    ] {
        let json = serde_json::to_string(&tag).unwrap();
        let restored: CultureTag = serde_json::from_str(&json).unwrap();
        assert_eq!(tag, restored);
    }
}

#[test]
fn discover_civ_command_serde_roundtrip() {
    let cmd = SimCommand {
        player_name: "test_player".to_string(),
        tick: 42,
        action: SimAction::DiscoverCiv {
            civ_id: CivId(0),
            discovered_civ: CivId(5),
            initial_opinion: CivOpinion::Suspicious,
        },
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());
}

#[test]
fn set_civ_opinion_command_serde_roundtrip() {
    let cmd = SimCommand {
        player_name: "test_player".to_string(),
        tick: 99,
        action: SimAction::SetCivOpinion {
            civ_id: CivId(1),
            target_civ: CivId(3),
            opinion: CivOpinion::Hostile,
        },
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());
}

#[test]
fn civ_opinion_shift_friendlier() {
    assert_eq!(
        CivOpinion::Hostile.shift_friendlier(),
        CivOpinion::Suspicious
    );
    assert_eq!(
        CivOpinion::Suspicious.shift_friendlier(),
        CivOpinion::Neutral
    );
    assert_eq!(CivOpinion::Neutral.shift_friendlier(), CivOpinion::Friendly);
    assert_eq!(
        CivOpinion::Friendly.shift_friendlier(),
        CivOpinion::Friendly
    );
}

#[test]
fn civ_opinion_shift_hostile() {
    assert_eq!(CivOpinion::Friendly.shift_hostile(), CivOpinion::Neutral);
    assert_eq!(CivOpinion::Neutral.shift_hostile(), CivOpinion::Suspicious);
    assert_eq!(CivOpinion::Suspicious.shift_hostile(), CivOpinion::Hostile);
    assert_eq!(CivOpinion::Hostile.shift_hostile(), CivOpinion::Hostile);
}

// -----------------------------------------------------------------------
// Friendly-fire avoidance (F-friendly-fire)
// -----------------------------------------------------------------------

// -- diplomatic_relation / creature_relation / player_relation tests --

#[test]
fn diplomatic_relation_same_civ_is_friendly() {
    let sim = test_sim(42);
    let player_civ = sim.player_civ_id.unwrap();
    assert_eq!(
        sim.diplomatic_relation(Some(player_civ), None, Some(player_civ), None),
        DiplomaticRelation::Friendly
    );
}

#[test]
fn diplomatic_relation_hostile_civs() {
    let mut sim = test_sim(42);
    let civs: Vec<_> = sim.db.civilizations.iter_all().collect();
    assert!(civs.len() >= 2);
    let civ_a = civs[0].id;
    let civ_b = civs[1].id;

    // Discover civ_b as Hostile from civ_a's perspective.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DiscoverCiv {
                civ_id: civ_a,
                discovered_civ: civ_b,
                initial_opinion: CivOpinion::Hostile,
            },
        }],
        tick + 1,
    );

    assert_eq!(
        sim.diplomatic_relation(Some(civ_a), None, Some(civ_b), None),
        DiplomaticRelation::Hostile
    );
}

#[test]
fn diplomatic_relation_neutral_civs() {
    let mut sim = test_sim(42);
    let civs: Vec<_> = sim.db.civilizations.iter_all().collect();
    assert!(civs.len() >= 2);
    let civ_a = civs[0].id;
    let civ_b = civs[1].id;

    // Remove any existing relationships.
    let existing: Vec<_> = sim
        .db
        .civ_relationships
        .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.to_civ == civ_b)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in existing {
        let _ = sim.db.civ_relationships.remove_no_fk(&pk);
    }

    assert_eq!(
        sim.diplomatic_relation(Some(civ_a), None, Some(civ_b), None),
        DiplomaticRelation::Neutral
    );
}

#[test]
fn diplomatic_relation_civ_vs_aggressive_nonciv() {
    let sim = test_sim(42);
    let player_civ = sim.player_civ_id.unwrap();
    // Goblin is Aggressive — should be Hostile from any civ's perspective.
    assert_eq!(
        sim.diplomatic_relation(Some(player_civ), None, None, Some(Species::Goblin)),
        DiplomaticRelation::Hostile
    );
}

#[test]
fn diplomatic_relation_civ_vs_passive_nonciv() {
    let sim = test_sim(42);
    let player_civ = sim.player_civ_id.unwrap();
    // Deer is Passive — should be Neutral.
    assert_eq!(
        sim.diplomatic_relation(Some(player_civ), None, None, Some(Species::Deer)),
        DiplomaticRelation::Neutral
    );
}

#[test]
fn diplomatic_relation_nonciv_aggressive_vs_civ() {
    let sim = test_sim(42);
    let player_civ = sim.player_civ_id.unwrap();
    // Aggressive non-civ creature looking at a civ → Hostile.
    assert_eq!(
        sim.diplomatic_relation(None, Some(Species::Goblin), Some(player_civ), None),
        DiplomaticRelation::Hostile
    );
}

#[test]
fn diplomatic_relation_nonciv_passive_vs_civ() {
    let sim = test_sim(42);
    let player_civ = sim.player_civ_id.unwrap();
    // Passive non-civ looking at a civ → Neutral.
    assert_eq!(
        sim.diplomatic_relation(None, Some(Species::Deer), Some(player_civ), None),
        DiplomaticRelation::Neutral
    );
}

#[test]
fn diplomatic_relation_nonciv_vs_nonciv() {
    let sim = test_sim(42);
    // Neither has a civ → always Neutral.
    assert_eq!(
        sim.diplomatic_relation(None, Some(Species::Goblin), None, Some(Species::Deer)),
        DiplomaticRelation::Neutral
    );
}

#[test]
fn diplomatic_relation_no_info() {
    let sim = test_sim(42);
    // Both sides have no info at all → Neutral.
    assert_eq!(
        sim.diplomatic_relation(None, None, None, None),
        DiplomaticRelation::Neutral
    );
}

#[test]
fn creature_relation_self_is_friendly() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    assert_eq!(
        sim.creature_relation(elf, elf),
        DiplomaticRelation::Friendly
    );
}

#[test]
fn creature_relation_same_civ_is_friendly() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    assert_eq!(
        sim.creature_relation(elf_a, elf_b),
        DiplomaticRelation::Friendly
    );
}

#[test]
fn creature_relation_missing_creature_is_neutral() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    // Spawn an elf, then remove it so the ID exists but creature is gone.
    let elf = spawn_elf(&mut sim);
    let fake_id = elf;
    let _ = sim.db.creatures.remove_no_fk(&fake_id);
    let elf_b = spawn_elf(&mut sim);
    assert_eq!(
        sim.creature_relation(fake_id, elf_b),
        DiplomaticRelation::Neutral
    );
}

#[test]
fn player_relation_friendly_for_elf() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    assert_eq!(sim.player_relation(elf), DiplomaticRelation::Friendly);
}

#[test]
fn player_relation_hostile_for_aggressive_nonciv() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    assert_eq!(sim.player_relation(goblin), DiplomaticRelation::Hostile);
}

#[test]
fn player_relation_neutral_for_passive_nonciv() {
    let mut sim = test_sim(42);
    let deer = spawn_species(&mut sim, Species::Deer);
    assert_eq!(sim.player_relation(deer), DiplomaticRelation::Neutral);
}

#[test]
fn civ_creature_relation_matches_player_relation() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let player_civ = sim.player_civ_id.unwrap();
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let deer = spawn_species(&mut sim, Species::Deer);
    // civ_creature_relation with player civ should match player_relation.
    assert_eq!(
        sim.civ_creature_relation(player_civ, elf),
        sim.player_relation(elf)
    );
    assert_eq!(
        sim.civ_creature_relation(player_civ, goblin),
        sim.player_relation(goblin)
    );
    assert_eq!(
        sim.civ_creature_relation(player_civ, deer),
        sim.player_relation(deer)
    );
}

#[test]
fn is_non_hostile_same_creature() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    assert!(sim.is_non_hostile(elf, elf));
}

#[test]
fn is_non_hostile_same_civ() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    // Both elves belong to the player civ.
    assert!(sim.is_non_hostile(elf_a, elf_b));
}

#[test]
fn is_non_hostile_civ_vs_passive_non_civ() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    // Deer has Passive combat AI — should be non-hostile.
    let deer = spawn_species(&mut sim, Species::Deer);
    assert!(sim.is_non_hostile(elf, deer));
}

#[test]
fn is_non_hostile_civ_vs_aggressive_non_civ() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    // Goblin has aggressive engagement initiative — should be hostile.
    let goblin = spawn_species(&mut sim, Species::Goblin);
    assert!(!sim.is_non_hostile(elf, goblin));
}

#[test]
fn is_non_hostile_non_civ_same_species() {
    let mut sim = test_sim(42);
    let goblin_a = spawn_species(&mut sim, Species::Goblin);
    let goblin_b = spawn_species(&mut sim, Species::Goblin);
    assert!(sim.is_non_hostile(goblin_a, goblin_b));
}

#[test]
fn is_non_hostile_non_civ_different_species() {
    // Non-civ creatures are always non-hostile to each other, even across
    // species. This matches detect_hostile_targets: non-civ aggressors
    // only target civ creatures.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let deer = spawn_species(&mut sim, Species::Deer);
    assert!(sim.is_non_hostile(goblin, deer));
}

#[test]
fn is_non_hostile_different_civs_neutral() {
    // Two creatures from different civs with no Hostile relationship
    // should be non-hostile.
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;

    // Use two existing civs from worldgen.
    let civs: Vec<_> = sim.db.civilizations.iter_all().collect();
    assert!(civs.len() >= 2, "Need at least 2 civs");
    let civ_a = civs[0].id;
    let civ_b = civs[1].id;

    // Remove any existing relationships so we have a clean slate.
    let existing: Vec<_> = sim
        .db
        .civ_relationships
        .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.to_civ == civ_b)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in existing {
        let _ = sim.db.civ_relationships.remove_no_fk(&pk);
    }

    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    if let Some(mut c) = sim.db.creatures.get(&elf_b) {
        c.civ_id = Some(civ_b);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // No Hostile relationship → non-hostile.
    assert!(
        sim.is_non_hostile(elf_a, elf_b),
        "Different civs with no Hostile relationship should be non-hostile"
    );
}

#[test]
fn is_non_hostile_different_civs_hostile() {
    // Two creatures from different civs with a Hostile relationship
    // should be hostile.
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;

    let civs: Vec<_> = sim.db.civilizations.iter_all().collect();
    assert!(civs.len() >= 2, "Need at least 2 civs");
    let civ_b = civs[1].id;

    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);

    // Assign elf_a to civ_a (it should already be the player civ).
    let elf_a_civ = sim.db.creatures.get(&elf_a).unwrap().civ_id.unwrap();
    // Assign elf_b to civ_b.
    if let Some(mut c) = sim.db.creatures.get(&elf_b) {
        c.civ_id = Some(civ_b);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Discover civ_b as Hostile.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DiscoverCiv {
                civ_id: elf_a_civ,
                discovered_civ: civ_b,
                initial_opinion: CivOpinion::Hostile,
            },
        }],
        tick + 1,
    );

    assert!(
        !sim.is_non_hostile(elf_a, elf_b),
        "Different civs with Hostile relationship should be hostile"
    );
}

#[test]
fn civ_species_to_species_conversion() {
    assert_eq!(CivSpecies::Elf.to_species(), Some(Species::Elf));
    assert_eq!(CivSpecies::Goblin.to_species(), Some(Species::Goblin));
    assert_eq!(CivSpecies::Orc.to_species(), Some(Species::Orc));
    assert_eq!(CivSpecies::Troll.to_species(), Some(Species::Troll));
    assert_eq!(CivSpecies::Human.to_species(), None);
    assert_eq!(CivSpecies::Dwarf.to_species(), None);
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

/// Verify CivRelationship compound PK operations: get, contains, modify_unchecked.
#[test]
fn civ_relationship_compound_pk_operations() {
    let mut sim = test_sim(42);

    let civ_a = CivId(0);
    // Find a target civ that exists.
    let civ_b = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| c.id != civ_a)
        .map(|c| c.id);
    let civ_b = match civ_b {
        Some(id) => id,
        None => return, // Only one civ in this seed, skip.
    };

    // Remove any existing relationship so we start clean.
    let _ = sim.db.civ_relationships.remove_no_fk(&(civ_a, civ_b));

    // Insert a new relationship.
    sim.db
        .civ_relationships
        .insert_no_fk(crate::db::CivRelationship {
            from_civ: civ_a,
            to_civ: civ_b,
            opinion: CivOpinion::Neutral,
        })
        .unwrap();

    // get() returns the relationship.
    let rel = sim.db.civ_relationships.get(&(civ_a, civ_b)).unwrap();
    assert_eq!(rel.opinion, CivOpinion::Neutral);

    // contains() works.
    assert!(sim.db.civ_relationships.contains(&(civ_a, civ_b)));
    assert!(!sim.db.civ_relationships.contains(&(civ_b, CivId(999))));

    // modify_unchecked() changes opinion.
    let _ = sim
        .db
        .civ_relationships
        .modify_unchecked(&(civ_a, civ_b), |r| {
            r.opinion = CivOpinion::Hostile;
        });
    assert_eq!(
        sim.db
            .civ_relationships
            .get(&(civ_a, civ_b))
            .unwrap()
            .opinion,
        CivOpinion::Hostile
    );

    // Duplicate insert fails.
    let err = sim
        .db
        .civ_relationships
        .insert_no_fk(crate::db::CivRelationship {
            from_civ: civ_a,
            to_civ: civ_b,
            opinion: CivOpinion::Friendly,
        });
    assert!(err.is_err());
}
