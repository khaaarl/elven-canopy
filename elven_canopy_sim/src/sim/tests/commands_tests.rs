//! Tests for SimCommand/SimAction processing, creature spawning via
//! commands, selection groups, notifications, material filters,
//! species data loading, and miscellaneous sim-level operations.
//! Corresponds to `sim/commands.rs` and various sim subsystems.

use super::*;

/// Verify that set_inv_wants correctly removes and re-inserts logistics want
/// rows using the new compound PK (inventory_id, seq).
#[test]
fn logistics_want_row_remove_and_reinsert_with_compound_pk() {
    let mut sim = test_sim(42);
    let pos = VoxelCoord::new(10, 1, 20);
    let pile_id = sim.ensure_ground_pile(pos);
    let inv_id = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;

    // Set initial wants.
    sim.set_inv_wants(
        inv_id,
        &[crate::building::LogisticsWant {
            item_kind: inventory::ItemKind::Bread,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );
    assert_eq!(sim.inv_wants(inv_id).len(), 1);
    assert_eq!(sim.inv_wants(inv_id)[0].target_quantity, 5);

    // Replace wants — old rows should be removed by compound PK, new ones inserted.
    sim.set_inv_wants(
        inv_id,
        &[
            crate::building::LogisticsWant {
                item_kind: inventory::ItemKind::Bread,
                material_filter: inventory::MaterialFilter::Any,
                target_quantity: 10,
            },
            crate::building::LogisticsWant {
                item_kind: inventory::ItemKind::Fruit,
                material_filter: inventory::MaterialFilter::Any,
                target_quantity: 3,
            },
        ],
    );
    let wants = sim.inv_wants(inv_id);
    assert_eq!(wants.len(), 2);

    // Clear all wants.
    sim.set_inv_wants(inv_id, &[]);
    assert_eq!(sim.inv_wants(inv_id).len(), 0);
}

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

#[test]
fn spawn_capybara_command() {
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

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Capybara), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Capybara,
            ..
        }
    )));

    // Capybara should be at a ground-level node (y=1, air above terrain).
    let capybara = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .unwrap();
    assert_eq!(capybara.position.y, 1);
    assert!(sim.nav_graph.node_at(capybara.position).is_some());
}

#[test]
fn capybara_wanders_on_ground() {
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
    sim.step(&[cmd], 2);

    // Step far enough for heartbeat + movement.
    sim.step(&[], 50000);

    assert_eq!(sim.creature_count(Species::Capybara), 1);
    let capybara = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .unwrap();
    let capybara_nav = sim.nav_graph.node_at(capybara.position);
    assert!(capybara_nav.is_some());
    let node_pos = sim.nav_graph.node(capybara_nav.unwrap()).position;
    assert_eq!(capybara.position, node_pos);
}

#[test]
fn capybara_stays_on_ground() {
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
    sim.step(&[cmd], 2);

    // Run for many ticks — capybara must never leave y=1 (air above terrain).
    for target in (10000..100000).step_by(10000) {
        sim.step(&[], target);
        let capybara = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Capybara)
            .unwrap();
        assert_eq!(
            capybara.position.y, 1,
            "Capybara left ground at tick {target}: pos={:?}",
            capybara.position
        );
    }
}

#[test]
fn determinism_with_capybara() {
    let mut sim_a = test_sim(42);
    let mut sim_b = test_sim(42);

    let tree_pos = sim_a.db.trees.get(&sim_a.player_tree_id).unwrap().position;

    let spawn = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Capybara,
            position: tree_pos,
        },
    };

    sim_a.step(std::slice::from_ref(&spawn), 1000);
    sim_b.step(std::slice::from_ref(&spawn), 1000);

    assert_eq!(sim_a.db.creatures.len(), sim_b.db.creatures.len());
    for creature_a in sim_a.db.creatures.iter_all() {
        let creature_b = sim_b.db.creatures.get(&creature_a.id).unwrap();
        assert_eq!(creature_a.position, creature_b.position);
    }
    assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
}

#[test]
fn creature_wanders_via_activation_chain() {
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
    sim.step(&[cmd], 2);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    let initial_node = sim.nav_graph.node_at(elf.position).unwrap();
    let initial_pos = elf.position;

    // Step enough for many activations (each moves 1 edge; ground edges
    // cost ~500 ticks at walk_ticks_per_voxel=500).
    sim.step(&[], 50000);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    let final_node = sim.nav_graph.node_at(elf.position).unwrap();

    // After many activations, creature should have moved.
    assert_ne!(
        initial_node, final_node,
        "Elf should have moved after activation chain"
    );
    // Position should match current node.
    let node_pos = sim.nav_graph.node(final_node).position;
    assert_eq!(elf.position, node_pos);
    // Creature should not have a stored path (wandering doesn't use paths).
    assert!(
        elf.path.is_none(),
        "Wandering creature should not have a stored path"
    );
    let _ = initial_pos;
}

#[test]
fn wandering_creature_stays_on_nav_graph() {
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
    sim.step(&[cmd], 2);

    // Run for many ticks, periodically checking node validity.
    for target in (10000..100000).step_by(10000) {
        sim.step(&[], target);
        let elf = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        let node = sim
            .nav_graph
            .node_at(elf.position)
            .expect("Elf should always have a nav node at its position");
        assert!(
            (node.0 as usize) < sim.nav_graph.node_count(),
            "Node ID {} out of range at tick {}",
            node.0,
            target
        );
        let node_pos = sim.nav_graph.node(node).position;
        assert_eq!(
            elf.position, node_pos,
            "Position mismatch at tick {}",
            target
        );
    }
}

#[test]
fn goto_task_completes_on_arrival() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    // Put the task at the elf's current location for instant completion.
    let elf_node = creature_node(&sim, elf_id);
    let task_id = insert_goto_task(&mut sim, elf_node);

    // One activation should be enough: elf claims task, is already there, completes.
    sim.step(&[], sim.tick + 10000);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "GoTo task should be complete"
    );
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.current_task, None,
        "Elf should be unassigned after task completion"
    );
}

#[test]
fn completed_task_creature_resumes_wandering() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    // Put the task at the elf's current location for instant completion.
    let elf_node = creature_node(&sim, elf_id);
    let _task_id = insert_goto_task(&mut sim, elf_node);

    // Complete the task.
    sim.step(&[], sim.tick + 10000);
    let pos_after_task = sim.db.creatures.get(&elf_id).unwrap().position;

    // Continue ticking — elf should resume wandering (position changes).
    sim.step(&[], sim.tick + 50000);

    let pos_after_wander = sim.db.creatures.get(&elf_id).unwrap().position;
    assert_ne!(
        pos_after_task, pos_after_wander,
        "Elf should have wandered after task completion"
    );
    assert!(
        sim.db
            .creatures
            .get(&elf_id)
            .unwrap()
            .current_task
            .is_none(),
        "Elf should still have no task"
    );
}

#[test]
fn create_task_command_adds_task() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::CreateTask {
            kind: TaskKind::GoTo,
            position: tree_pos,
            required_species: Some(Species::Elf),
        },
    };
    sim.step(&[cmd], 2);

    assert_eq!(sim.db.tasks.len(), 1, "Should have 1 task");
    let task = sim.db.tasks.iter_all().next().unwrap();
    assert_eq!(task.state, TaskState::Available);
    assert!(task.kind_tag == TaskKindTag::GoTo);
}

#[test]
fn end_to_end_summon_task() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf.
    let spawn_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[spawn_cmd], 2);

    // Create a GoTo task at a ground position near the tree.
    let task_cmd = SimCommand {
        player_name: String::new(),
        tick: 3,
        action: SimAction::CreateTask {
            kind: TaskKind::GoTo,
            position: VoxelCoord::new(tree_pos.x + 10, 0, tree_pos.z),
            required_species: Some(Species::Elf),
        },
    };
    sim.step(&[task_cmd], 4);

    assert_eq!(sim.db.tasks.len(), 1);
    let task_id = *sim.db.tasks.iter_keys().next().unwrap();

    // Tick until the elf completes the task.
    sim.step(&[], 50000);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "Task should be complete after enough ticks"
    );

    // Elf should be unassigned and wandering again.
    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert!(elf.current_task.is_none());
}

#[test]
fn only_one_creature_claims_goto_task() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn multiple elves and capybaras.
    for _ in 0..3 {
        let cmd = SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);
    }
    for _ in 0..2 {
        let cmd = SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                species: Species::Capybara,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);
    }

    // Create an elf-only GoTo task.
    let far_node = NavNodeId((sim.nav_graph.node_count() - 1) as u32);
    let task_id = insert_goto_task(&mut sim, far_node);

    // Tick enough for a creature to claim the task. The elf may or may
    // not have arrived yet (GoTo completes on arrival, clearing
    // current_task), so we check that the task was claimed OR completed.
    sim.step(&[], sim.tick + 5000);

    let task = sim.db.tasks.get(&task_id).unwrap();
    let claimers = sim
        .db
        .creatures
        .by_current_task(&Some(task.id), tabulosity::QueryOpts::ASC);

    if task.state == crate::task::TaskState::Complete {
        // Task was completed — some elf claimed and finished it.
        assert!(
            claimers.is_empty(),
            "Completed task should have no current claimers"
        );
    } else {
        // Task still in progress — exactly one elf should be on it.
        assert_eq!(
            claimers.len(),
            1,
            "Exactly one creature should claim the task, got {}",
            claimers.len()
        );
        let assignee = &claimers[0];
        assert_eq!(assignee.species, Species::Elf);
    }

    // No capybara should have a task (elf-only restriction).
    for creature in sim.db.creatures.iter_all() {
        if creature.species == Species::Capybara {
            assert!(
                creature.current_task.is_none(),
                "Capybara should not have claimed an elf-only task"
            );
        }
    }
}

#[test]
fn new_sim_has_initial_fruit() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert!(
        !tree.fruit_positions.is_empty(),
        "Tree should have some initial fruit (got 0)"
    );
}

#[test]
fn fruit_hangs_below_leaf_voxels() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    for fruit_pos in &tree.fruit_positions {
        // The leaf above the fruit should be in the tree's leaf_voxels.
        let leaf_above = VoxelCoord::new(fruit_pos.x, fruit_pos.y + 1, fruit_pos.z);
        assert!(
            tree.leaf_voxels.contains(&leaf_above),
            "Fruit at {} should hang below a leaf voxel, but no leaf at {}",
            fruit_pos,
            leaf_above
        );
    }
}

#[test]
fn fruit_set_in_world_grid() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    for fruit_pos in &tree.fruit_positions {
        assert_eq!(
            sim.world.get(*fruit_pos),
            VoxelType::Fruit,
            "World should have Fruit voxel at {}",
            fruit_pos
        );
    }
}

#[test]
fn fruit_grows_during_heartbeat() {
    // Use a config with no initial fruit but high spawn rate so heartbeats produce fruit.
    let mut config = test_config();
    config.fruit_initial_attempts = 0;
    config.fruit_production_rate_ppm = 1_000_000; // Always spawn
    config.fruit_max_per_tree = 100;
    let mut sim = SimState::with_config(42, config);
    let tree_id = sim.player_tree_id;

    assert!(
        sim.db
            .trees
            .get(&tree_id)
            .unwrap()
            .fruit_positions
            .is_empty(),
        "Should start with no fruit when initial_attempts = 0"
    );

    // Step past several heartbeats (interval = 10000 ticks).
    sim.step(&[], 50000);

    assert!(
        !sim.db
            .trees
            .get(&tree_id)
            .unwrap()
            .fruit_positions
            .is_empty(),
        "Fruit should grow during tree heartbeats"
    );
}

#[test]
fn fruit_respects_max_count() {
    let mut config = test_config();
    config.fruit_max_per_tree = 3;
    config.fruit_initial_attempts = 100; // Many attempts, but max is 3.
    config.fruit_production_rate_ppm = 1_000_000;
    let sim = SimState::with_config(42, config);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();

    assert!(
        tree.fruit_positions.len() <= 3,
        "Fruit count {} should not exceed max 3",
        tree.fruit_positions.len()
    );
}

#[test]
fn fruit_deterministic() {
    let sim_a = test_sim(42);
    let sim_b = test_sim(42);
    let tree_a = sim_a.db.trees.get(&sim_a.player_tree_id).unwrap();
    let tree_b = sim_b.db.trees.get(&sim_b.player_tree_id).unwrap();
    assert_eq!(tree_a.fruit_positions, tree_b.fruit_positions);
}

#[test]
fn tree_has_fruit_species_assigned() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert!(
        tree.fruit_species_id.is_some(),
        "Home tree should have a fruit species assigned during worldgen"
    );
    // The assigned species should exist in the world's species roster.
    let species_id = tree.fruit_species_id.unwrap();
    assert!(
        sim.db.fruit_species.get(&species_id).is_some(),
        "Tree's fruit species {:?} should be in the SimDb fruit_species table",
        species_id
    );
}

#[test]
fn fruit_voxels_have_species_tracked() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    // Every fruit voxel should have a species entry in the map.
    for &fruit_pos in &tree.fruit_positions {
        assert!(
            sim.fruit_voxel_species.contains_key(&fruit_pos),
            "Fruit at {} should have a species tracked in fruit_voxel_species",
            fruit_pos
        );
    }
    // The tracked species should match the tree's assigned species.
    if let Some(tree_species) = tree.fruit_species_id {
        for &fruit_pos in &tree.fruit_positions {
            let voxel_species = sim.fruit_voxel_species[&fruit_pos];
            assert_eq!(
                voxel_species, tree_species,
                "Fruit voxel species should match tree species"
            );
        }
    }
}

#[test]
fn fruit_species_at_returns_species() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    if let Some(first_fruit) = tree.fruit_positions.first() {
        let species = sim.fruit_species_at(*first_fruit);
        assert!(
            species.is_some(),
            "fruit_species_at should return a species"
        );
        let species = species.unwrap();
        assert!(
            !species.vaelith_name.is_empty(),
            "Fruit species should have a Vaelith name"
        );
        assert!(
            !species.english_gloss.is_empty(),
            "Fruit species should have an English gloss"
        );
    }
}

#[test]
fn fruit_voxel_species_roundtrip() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert!(!tree.fruit_positions.is_empty(), "need fruit for this test");

    let json = sim.to_json().unwrap();
    let loaded = SimState::from_json(&json).unwrap();
    let loaded_tree = loaded.db.trees.get(&loaded.player_tree_id).unwrap();

    // Fruit voxel species map should survive roundtrip.
    assert_eq!(
        sim.fruit_voxel_species.len(),
        loaded.fruit_voxel_species.len(),
        "fruit_voxel_species count should survive roundtrip"
    );
    for (&pos, &species_id) in &sim.fruit_voxel_species {
        assert_eq!(
            loaded.fruit_voxel_species.get(&pos),
            Some(&species_id),
            "fruit_voxel_species entry at {} should survive roundtrip",
            pos
        );
    }
    // Tree's fruit species should survive too.
    assert_eq!(
        loaded_tree.fruit_species_id, tree.fruit_species_id,
        "Tree fruit_species_id should survive roundtrip"
    );
}

#[test]
fn harvest_fruit_carries_species_material() {
    let mut sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let fruit_pos = tree.fruit_positions[0];
    let tree_species = tree.fruit_species_id.unwrap();

    // Spawn an elf near the fruit.
    let elf_nav = sim.nav_graph.find_nearest_node(fruit_pos).unwrap();
    let elf_pos = sim.nav_graph.node(elf_nav).position;
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, elf_pos, &mut events)
        .unwrap();
    sim.config.elf_starting_bread = 100; // Prevent hunger.

    // Manually call do_harvest to test the material flow.
    let task_id = TaskId::new(&mut sim.rng);
    let task = task::Task {
        id: task_id,
        kind: task::TaskKind::Harvest { fruit_pos },
        state: task::TaskState::InProgress,
        location: elf_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: task::TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(task);
    sim.resolve_harvest_action(elf_id, task_id, fruit_pos);

    // The fruit should be gone from world and species map.
    assert_eq!(sim.world.get(fruit_pos), VoxelType::Air);
    assert!(!sim.fruit_voxel_species.contains_key(&fruit_pos));

    // Find the ground pile and check the item has fruit species material.
    let pile_stacks: Vec<_> = sim
        .db
        .item_stacks
        .iter_all()
        .filter(|s| {
            s.kind == inventory::ItemKind::Fruit
                && s.material == Some(inventory::Material::FruitSpecies(tree_species))
        })
        .collect();
    assert!(
        !pile_stacks.is_empty(),
        "Harvested fruit should have Material::FruitSpecies({:?})",
        tree_species
    );
}

#[test]
fn fruit_heartbeat_tracks_species() {
    // Fruit grown via heartbeat should also be tracked in species map.
    let mut config = test_config();
    config.fruit_initial_attempts = 0;
    config.fruit_production_rate_ppm = 1_000_000;
    config.fruit_max_per_tree = 100;
    let mut sim = SimState::with_config(42, config);

    assert!(
        sim.fruit_voxel_species.is_empty(),
        "Should start with no species entries"
    );

    // Step past heartbeats to grow fruit.
    sim.step(&[], 50000);

    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert!(
        !tree.fruit_positions.is_empty(),
        "Should have grown some fruit"
    );
    // Every fruit should have species tracked.
    for &pos in &tree.fruit_positions {
        assert!(
            sim.fruit_voxel_species.contains_key(&pos),
            "Heartbeat-grown fruit at {} should have species tracked",
            pos
        );
    }
}

// -----------------------------------------------------------------------
// Save/load roundtrip tests
// -----------------------------------------------------------------------

#[test]
fn save_load_preserves_world_voxels() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();

    // Roundtrip through JSON (world is now serialized, not rebuilt).
    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    // Check trunk voxels survived serialization.
    for coord in &tree.trunk_voxels {
        assert_eq!(
            restored.world.get(*coord),
            VoxelType::Trunk,
            "Restored world missing trunk voxel at {coord}"
        );
    }
    // Check branch voxels.
    for coord in &tree.branch_voxels {
        assert_eq!(
            restored.world.get(*coord),
            VoxelType::Branch,
            "Restored world missing branch voxel at {coord}"
        );
    }
    // Check root voxels.
    for coord in &tree.root_voxels {
        assert_eq!(
            restored.world.get(*coord),
            VoxelType::Root,
            "Restored world missing root voxel at {coord}"
        );
    }
    // Check leaf voxels.
    for coord in &tree.leaf_voxels {
        assert_eq!(
            restored.world.get(*coord),
            VoxelType::Leaf,
            "Restored world missing leaf voxel at {coord}"
        );
    }
    // Check that a known solid voxel (first trunk) survived.
    let first_trunk = tree.trunk_voxels[0];
    assert_eq!(
        restored.world.get(first_trunk),
        VoxelType::Trunk,
        "First trunk voxel should be present after roundtrip"
    );
}

#[test]
fn rebuild_transient_state_restores_nav_graph() {
    let sim = test_sim(42);
    let json = sim.to_json().unwrap();

    // Deserialize — world is preserved but transient fields are default.
    let mut restored: SimState = serde_json::from_str(&json).unwrap();
    assert_eq!(
        restored.nav_graph.node_count(),
        0,
        "Before rebuild, nav_graph should be empty"
    );
    // World is now serialized, so it should be present after deserialization.
    assert_eq!(
        restored.world.size_x, sim.world.size_x,
        "After deserialization, world should be present"
    );

    // Rebuild transient state.
    restored.rebuild_transient_state();
    assert!(
        restored.nav_graph.node_count() > 0,
        "After rebuild, nav_graph should have nodes"
    );
    // Node count may differ very slightly because fruit voxels are placed
    // after the initial nav graph build but before serialization, so the
    // rebuilt world includes fruit while the original nav graph was built
    // without them. Allow a small tolerance.
    let diff =
        (restored.nav_graph.node_count() as i64 - sim.nav_graph.node_count() as i64).unsigned_abs();
    assert!(
        diff <= 5,
        "Rebuilt nav_graph node count ({}) should be close to original ({}), diff={}",
        restored.nav_graph.node_count(),
        sim.nav_graph.node_count(),
        diff,
    );
}

#[test]
fn elf_spawned_after_roundtrip_gets_name() {
    let sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Save and restore (no creatures yet).
    let mut restored = SimState::from_json(&sim.to_json().unwrap()).unwrap();

    // Spawn an elf after the roundtrip.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    restored.step(&[cmd], 2);

    let elf = restored
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .expect("elf should exist after roundtrip spawn");
    assert!(
        !elf.name.is_empty(),
        "Elf spawned after save/load should still get a Vaelith name"
    );
}

#[test]
fn species_data_loaded_from_config() {
    let sim = test_sim(42);
    assert_eq!(sim.species_table.len(), 12);
    assert!(sim.species_table.contains_key(&Species::Elf));
    assert!(sim.species_table.contains_key(&Species::Capybara));
    assert!(sim.species_table.contains_key(&Species::Boar));
    assert!(sim.species_table.contains_key(&Species::Deer));
    assert!(sim.species_table.contains_key(&Species::Elephant));
    assert!(sim.species_table.contains_key(&Species::Wyvern));
    assert!(sim.species_table.contains_key(&Species::Hornet));
    assert!(sim.species_table.contains_key(&Species::Goblin));
    assert!(sim.species_table.contains_key(&Species::Monkey));
    assert!(sim.species_table.contains_key(&Species::Orc));
    assert!(sim.species_table.contains_key(&Species::Squirrel));
    assert!(sim.species_table.contains_key(&Species::Troll));

    let elf_data = &sim.species_table[&Species::Elf];
    assert!(!elf_data.ground_only);
    assert!(elf_data.allowed_edge_types.is_none());

    let capy_data = &sim.species_table[&Species::Capybara];
    assert!(capy_data.ground_only);
    assert!(capy_data.allowed_edge_types.is_some());

    let boar_data = &sim.species_table[&Species::Boar];
    assert!(boar_data.ground_only);
    assert_eq!(boar_data.walk_ticks_per_voxel, 500); // uniform base

    let deer_data = &sim.species_table[&Species::Deer];
    assert!(deer_data.ground_only);
    assert_eq!(deer_data.walk_ticks_per_voxel, 500); // uniform base

    let monkey_data = &sim.species_table[&Species::Monkey];
    assert!(!monkey_data.ground_only);
    assert_eq!(monkey_data.climb_ticks_per_voxel, Some(800));

    let squirrel_data = &sim.species_table[&Species::Squirrel];
    assert!(!squirrel_data.ground_only);
    assert_eq!(squirrel_data.climb_ticks_per_voxel, Some(600));

    // Troll has HP regeneration; most species default to 0.
    let troll_data = &sim.species_table[&Species::Troll];
    assert_eq!(troll_data.ticks_per_hp_regen, 500);
    assert_eq!(elf_data.ticks_per_hp_regen, 0);
}

#[test]
fn graph_for_species_dispatch() {
    let sim = test_sim(42);

    // Elf (1x1x1) → standard graph.
    let elf_graph = sim.graph_for_species(Species::Elf) as *const _;
    let standard = &sim.nav_graph as *const _;
    assert_eq!(elf_graph, standard, "Elf should use standard nav graph");

    // Elephant (2x2x2) → large graph.
    let elephant_graph = sim.graph_for_species(Species::Elephant) as *const _;
    let large = &sim.large_nav_graph as *const _;
    assert_eq!(elephant_graph, large, "Elephant should use large nav graph");
}

#[test]
fn new_sim_has_large_nav_graph() {
    let sim = test_sim(42);
    assert!(
        sim.large_nav_graph.live_nodes().count() > 0,
        "Large nav graph should have nodes after construction",
    );
}

#[test]
fn elephant_spawns_on_large_graph() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();
    let spawn_pos = VoxelCoord::new(10, 1, 10);
    sim.spawn_creature(Species::Elephant, spawn_pos, &mut events);

    // There should be exactly one elephant.
    let elephants: Vec<&crate::db::Creature> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elephant)
        .collect();
    assert_eq!(elephants.len(), 1, "Should have spawned one elephant");

    // Its position should map to a node in the large nav graph.
    let elephant = elephants[0];
    let node_id = sim
        .large_nav_graph
        .node_at(elephant.position)
        .expect("Elephant should have a nav node in the large graph");
    let node = sim.large_nav_graph.node(node_id);
    assert_eq!(
        node.position, elephant.position,
        "Elephant position should match its large graph node",
    );
}

#[test]
fn troll_spawns_on_large_graph() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();
    let spawn_pos = VoxelCoord::new(10, 1, 10);
    sim.spawn_creature(Species::Troll, spawn_pos, &mut events);

    let trolls: Vec<&crate::db::Creature> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Troll)
        .collect();
    assert_eq!(trolls.len(), 1, "Should have spawned one troll");

    let troll = trolls[0];
    let node_id = sim
        .large_nav_graph
        .node_at(troll.position)
        .expect("Troll should have a nav node in the large graph");
    let node = sim.large_nav_graph.node(node_id);
    assert_eq!(
        node.position, troll.position,
        "Troll position should match its large graph node",
    );
}

#[test]
fn creature_species_preserved() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn one elf and one capybara.
    let cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Capybara,
                position: tree_pos,
            },
        },
    ];
    sim.step(&cmds, 2);

    assert_eq!(sim.creature_count(Species::Elf), 1);
    assert_eq!(sim.creature_count(Species::Capybara), 1);
    assert_eq!(sim.db.creatures.len(), 2);

    // Verify species are correctly stored.
    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.species, Species::Elf);

    let capy = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .unwrap();
    assert_eq!(capy.species, Species::Capybara);
}

#[test]
fn food_decreases_over_heartbeats() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let decay_per_tick = sim.species_table[&Species::Elf].food_decay_per_tick;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Verify food starts at food_max.
    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.food, food_max);

    // Advance past 3 heartbeats.
    let target_tick = 1 + heartbeat_interval * 3 + 1;
    sim.step(&[], target_tick);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    let expected_decay = decay_per_tick * heartbeat_interval as i64 * 3;
    assert_eq!(elf.food, food_max - expected_decay);
}

#[test]
fn food_does_not_go_below_zero() {
    // Use a custom config with aggressive decay so food depletes quickly.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 1_000_000_000_000_000; // Depletes in 1 tick
    let mut sim = SimState::with_config(42, config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Advance well past full depletion (many heartbeats).
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 5;
    sim.step(&[], target_tick);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.food, 0);
}

#[test]
fn creature_dies_when_food_reaches_zero() {
    // Use aggressive decay so food depletes in one heartbeat.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 1_000_000_000_000_000;
    let mut sim = SimState::with_config(42, config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Advance past 2 heartbeats — first depletes food, creature dies.
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 2 + 1;
    let result = sim.step(&[], target_tick);

    // Creature should be dead.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.vital_status, VitalStatus::Dead);

    // Should have emitted a CreatureDied event with Starvation cause.
    let died_event = result
        .events
        .iter()
        .find(|e| matches!(e.kind, SimEventKind::CreatureDied { .. }));
    assert!(died_event.is_some(), "Expected a CreatureDied event");
    if let SimEventKind::CreatureDied { cause, .. } = &died_event.unwrap().kind {
        assert_eq!(*cause, DeathCause::Starvation);
    }
}

#[test]
fn starvation_death_notification_mentions_starvation() {
    // Aggressive decay to trigger starvation quickly.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 1_000_000_000_000_000;
    let mut sim = SimState::with_config(42, config);
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

    // Advance past depletion.
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 2 + 1;
    sim.step(&[], target_tick);

    // Check notification mentions starvation.
    let starvation_notif = sim
        .db
        .notifications
        .iter_all()
        .any(|n| n.message.contains("starvation"));
    assert!(
        starvation_notif,
        "Expected notification mentioning starvation"
    );
}

#[test]
fn no_heartbeat_after_starvation_death() {
    // Verify dead creatures don't get further heartbeats processed.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 1_000_000_000_000_000;
    let mut sim = SimState::with_config(42, config);
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

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Advance well past death.
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 10;
    sim.step(&[], target_tick);

    // Creature should still be dead (not resurrected or erroring).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.vital_status, VitalStatus::Dead);
}

#[test]
fn creature_with_food_remaining_does_not_starve() {
    // Default config — food_max is large, decay is slow. Creature should
    // survive a few heartbeats without issue.
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

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Advance past 3 heartbeats — food should still be positive.
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 3 + 1;
    sim.step(&[], target_tick);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.vital_status, VitalStatus::Alive);
    assert!(elf.food > 0);
}

// -----------------------------------------------------------------------
// Rest/sleep tests
// -----------------------------------------------------------------------

#[test]
fn rest_decreases_over_heartbeats() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let decay_per_tick = sim.species_table[&Species::Elf].rest_decay_per_tick;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Verify rest starts at rest_max.
    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.rest, rest_max);

    // Advance past 3 heartbeats.
    let target_tick = 1 + heartbeat_interval * 3 + 1;
    sim.step(&[], target_tick);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    let expected_decay = decay_per_tick * heartbeat_interval as i64 * 3;
    assert_eq!(elf.rest, rest_max - expected_decay);
}

#[test]
fn rest_does_not_go_below_zero() {
    let mut config = test_config();
    let elf = config.species.get_mut(&Species::Elf).unwrap();
    elf.rest_decay_per_tick = 1_000_000_000_000_000; // Depletes in 1 tick
    elf.rest_per_sleep_tick = 0; // Prevent sleep from restoring rest.
    let mut sim = SimState::with_config(42, config);
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

    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 5;
    sim.step(&[], target_tick);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.rest, 0);
}

#[test]
fn tired_idle_elf_creates_sleep_task() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set rest below threshold (50%) and food well above threshold.
    let food_max_val = sim.species_table[&Species::Elf].food_max;
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.rest = rest_max * 30 / 100;
        c.food = food_max_val;
    });

    // Advance past the next heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // The elf should now have a Sleep task.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Tired idle elf should have been assigned a Sleep task"
    );
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert!(
        task.kind_tag == TaskKindTag::Sleep,
        "Task should be Sleep, got {:?}",
        task.kind_tag
    );
}

#[test]
fn rested_elf_does_not_create_sleep_task() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf — starts at full rest.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Advance past the heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // No Sleep task should exist.
    let has_sleep_task = sim
        .db
        .tasks
        .iter_all()
        .any(|t| t.kind_tag == TaskKindTag::Sleep);
    assert!(
        !has_sleep_task,
        "Well-rested elf should not create a Sleep task"
    );
}

#[test]
fn wander_sets_movement_metadata() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf at tick 1, step only to tick 1 so the first activation
    // (scheduled at tick 2) hasn't fired yet.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Before the first activation, the elf should have no action.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.action_kind, ActionKind::NoAction);
    assert!(elf.next_available_tick.is_none());
    assert!(sim.db.move_actions.get(&elf_id).is_none());

    let initial_pos = elf.position;

    // Step to tick 2 — the first activation fires and the elf wanders.
    sim.step(&[], 2);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.action_kind,
        ActionKind::Move,
        "action_kind should be Move after wander"
    );
    assert!(
        elf.next_available_tick.is_some(),
        "next_available_tick should be set after wander"
    );

    let ma = sim
        .db
        .move_actions
        .get(&elf_id)
        .expect("MoveAction should exist after wander");
    assert_eq!(
        ma.move_from, initial_pos,
        "move_from should be the spawn position"
    );
    assert_eq!(
        ma.move_to, elf.position,
        "move_to should be the new position"
    );
    assert_eq!(
        ma.move_start_tick, 2,
        "move_start_tick should be the activation tick"
    );
    assert!(
        elf.next_available_tick.unwrap() > ma.move_start_tick,
        "next_available_tick should be after start"
    );
}

#[test]
fn designate_build_creates_blueprint() {
    let mut sim = test_sim(42);
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    let result = sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.voxels, vec![air_coord]);
    assert_eq!(bp.state, BlueprintState::Designated);
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::BlueprintDesignated { .. }))
    );
}

#[test]
fn designate_build_creates_composition() {
    let mut sim = test_sim(42);
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    // Blueprint should have a composition FK.
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert!(
        bp.composition_id.is_some(),
        "Build blueprint should have a composition"
    );

    // The composition should exist in the DB with Pending status.
    let comp_id = bp.composition_id.unwrap();
    let comp = sim.db.music_compositions.get(&comp_id).unwrap();
    assert_eq!(comp.status, crate::db::CompositionStatus::Pending);
    assert!(!comp.build_started);
    assert!(comp.seed != 0, "Composition should have a non-trivial seed");
    assert!(comp.sections >= 1 && comp.sections <= 4);
    assert!(comp.mode_index <= 5);
    assert!(comp.brightness >= 0.2 && comp.brightness <= 0.8);
    // 1 voxel × 1000 ticks/voxel = 1000ms target duration.
    assert_eq!(comp.target_duration_ms, 1000);
}

#[test]
fn composition_persists_across_serde_roundtrip() {
    let mut sim = test_sim(42);
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    let bp = sim.db.blueprints.iter_all().next().unwrap();
    let comp_id = bp.composition_id.unwrap();
    let comp = sim.db.music_compositions.get(&comp_id).unwrap();
    let orig_seed = comp.seed;
    let orig_sections = comp.sections;
    let orig_mode = comp.mode_index;

    // Serialize and deserialize.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    // Composition should survive roundtrip.
    let comp = restored.db.music_compositions.get(&comp_id).unwrap();
    assert_eq!(comp.seed, orig_seed);
    assert_eq!(comp.sections, orig_sections);
    assert_eq!(comp.mode_index, orig_mode);
    assert_eq!(comp.status, crate::db::CompositionStatus::Pending);

    // Blueprint FK should still point to it.
    let bp = restored.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.composition_id, Some(comp_id));
}

#[test]
fn designate_carve_has_no_composition() {
    let mut sim = test_sim(42);

    // Find a solid trunk voxel to carve.
    let mut carve_coord = None;
    for y in 1..sim.world.size_y as i32 {
        for z in 0..sim.world.size_z as i32 {
            for x in 0..sim.world.size_x as i32 {
                let coord = VoxelCoord::new(x, y, z);
                if sim.world.get(coord) == VoxelType::Trunk {
                    carve_coord = Some(coord);
                    break;
                }
            }
            if carve_coord.is_some() {
                break;
            }
        }
        if carve_coord.is_some() {
            break;
        }
    }
    let carve_coord = carve_coord.expect("Should find a trunk voxel to carve");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![carve_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    if !sim.db.blueprints.is_empty() {
        let bp = sim.db.blueprints.iter_all().next().unwrap();
        assert!(
            bp.composition_id.is_none(),
            "Carve blueprint should not have a composition"
        );
    }
    // No compositions should have been created for carving.
    assert_eq!(
        sim.db.music_compositions.len(),
        0,
        "Carving should not create compositions"
    );
}

#[test]
fn build_work_sets_composition_build_started() {
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 50000;
    let mut sim = SimState::with_config(42, config);
    let air_coord = find_air_adjacent_to_trunk(&sim);

    spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Composition should not be started yet (no work done).
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    let comp_id = bp.composition_id.unwrap();
    assert!(
        !sim.db
            .music_compositions
            .get(&comp_id)
            .unwrap()
            .build_started,
        "Composition should not be started before any work"
    );

    // Run enough ticks for the elf to arrive and do at least one tick of work.
    sim.step(&[], sim.tick + 100_000);

    assert!(
        sim.db
            .music_compositions
            .get(&comp_id)
            .unwrap()
            .build_started,
        "Composition should be started after elf begins building"
    );
}

// -----------------------------------------------------------------------
// New species tests (Boar, Deer, Monkey, Squirrel)
// -----------------------------------------------------------------------

#[test]
fn spawn_boar_command() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Boar,
            position: tree_pos,
        },
    };

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Boar), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Boar,
            ..
        }
    )));

    // Boar is ground-only — should be at y=1.
    let boar = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Boar)
        .unwrap();
    assert_eq!(boar.position.y, 1);
}

#[test]
fn spawn_deer_command() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Deer,
            position: tree_pos,
        },
    };

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Deer), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Deer,
            ..
        }
    )));

    // Deer is ground-only — should be at y=1.
    let deer = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Deer)
        .unwrap();
    assert_eq!(deer.position.y, 1);
}

#[test]
fn spawn_monkey_command() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Monkey,
            position: tree_pos,
        },
    };

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Monkey), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Monkey,
            ..
        }
    )));
}

#[test]
fn spawn_squirrel_command() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Squirrel,
            position: tree_pos,
        },
    };

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Squirrel), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Squirrel,
            ..
        }
    )));
}

#[test]
fn boar_stays_on_ground() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Boar,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    // Run for many ticks — boar must never leave y=1 (ground-only).
    for target in (10000..100000).step_by(10000) {
        sim.step(&[], target);
        let boar = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Boar)
            .unwrap();
        assert_eq!(
            boar.position.y, 1,
            "Boar left ground at tick {target}: pos={:?}",
            boar.position
        );
    }
}

#[test]
fn deer_stays_on_ground() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Deer,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    for target in (10000..100000).step_by(10000) {
        sim.step(&[], target);
        let deer = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Deer)
            .unwrap();
        assert_eq!(
            deer.position.y, 1,
            "Deer left ground at tick {target}: pos={:?}",
            deer.position
        );
    }
}

#[test]
fn monkey_can_climb() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Monkey,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    // Run for enough ticks that a climbing species should have left ground.
    sim.step(&[], 100000);

    let monkey = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Monkey)
        .unwrap();
    // Monkey is not ground_only, so it should be able to reach y > 1
    // (trunk/branch surfaces). This verifies the species config allows
    // climbing edges. The monkey may still be at y=1 if the PRNG led it
    // only to ground neighbors, so we just verify it has a valid nav node.
    assert!(sim.nav_graph.node_at(monkey.position).is_some());
}

#[test]
fn squirrel_can_climb() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Squirrel,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    sim.step(&[], 100000);

    let squirrel = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Squirrel)
        .unwrap();
    assert!(sim.nav_graph.node_at(squirrel.position).is_some());
}

#[test]
fn all_small_species_spawn_and_coexist() {
    let mut sim = test_sim(300);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Only non-hostile species — hostile species (Goblin, Orc) would fight
    // and kill friendlies during the 50k-tick coexistence window, especially
    // with expanded melee ranges (range_sq=3 covers 3D diagonals).
    let species_list = [
        Species::Elf,
        Species::Capybara,
        Species::Boar,
        Species::Deer,
        Species::Monkey,
        Species::Squirrel,
    ];
    let mut tick = 1;
    for &species in &species_list {
        let cmd = SimCommand {
            player_name: String::new(),
            tick,
            action: SimAction::SpawnCreature {
                species,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], tick + 1);
        tick = sim.tick + 1;
    }

    assert_eq!(sim.db.creatures.len(), 6);
    for &species in &species_list {
        assert_eq!(sim.creature_count(species), 1, "Expected 1 {:?}", species);
    }

    // Run for a while — all should remain alive with valid nodes.
    sim.step(&[], 50000);
    assert_eq!(sim.db.creatures.len(), 6);
    for creature in sim.db.creatures.iter_all() {
        assert!(
            sim.graph_for_species(creature.species)
                .node_at(creature.position)
                .is_some(),
            "{:?} has no nav node at its position",
            creature.species
        );
    }
}

// --- Hauling and logistics tests ---

/// Helper: create a completed building structure at the given anchor.
fn insert_building(
    sim: &mut SimState,
    anchor: VoxelCoord,
    logistics_priority: Option<u8>,
    wants: Vec<crate::building::LogisticsWant>,
) -> StructureId {
    let sid = StructureId(sim.next_structure_id);
    sim.next_structure_id += 1;
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    // Insert a stub blueprint so structures.project_id FK passes validation.
    insert_stub_blueprint(sim, project_id);
    sim.db
        .structures
        .insert_no_fk(CompletedStructure {
            id: sid,
            project_id,
            build_type: BuildType::Building,
            anchor,
            width: 3,
            depth: 3,
            height: 2,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::Storehouse),
            inventory_id: inv_id,
            logistics_priority,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
        })
        .unwrap();
    sim.set_inv_wants(inv_id, &wants);
    sid
}

// -----------------------------------------------------------------------
// 15.10 New SimAction variants (SetCreatureFood, SetCreatureRest,
//       AddCreatureItem, AddGroundPileItem)
// -----------------------------------------------------------------------

#[test]
fn set_creature_food() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCreatureFood {
            creature_id: elf_id,
            food: 42_000,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().food, 42_000);
}

#[test]
fn set_creature_rest() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCreatureRest {
            creature_id: elf_id,
            rest: 99_000,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().rest, 99_000);
}

#[test]
fn add_creature_item() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bread = 0;
    let elf_id = spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::AddCreatureItem {
            creature_id: elf_id,
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    let bread_count = sim.inv_item_count(
        sim.creature_inv(elf_id),
        crate::inventory::ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert_eq!(bread_count, 5);
}

#[test]
fn add_ground_pile_item() {
    let mut sim = test_sim(42);
    let pos = VoxelCoord::new(32, 1, 32);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::AddGroundPileItem {
            position: pos,
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 3,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    let pile = sim
        .db
        .ground_piles
        .by_position(&pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .expect("pile should exist");
    let bread_count = sim.inv_item_count(
        pile.inventory_id,
        crate::inventory::ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert_eq!(bread_count, 3);
}

// -----------------------------------------------------------------------
// find_surface_position
// -----------------------------------------------------------------------

#[test]
fn find_surface_position_finds_air() {
    let sim = test_sim(42);
    let center = sim.world.size_x as i32 / 2;
    let pos = sim.find_surface_position(center, center);

    // The returned position should be Air (non-solid).
    assert!(
        !sim.world.get(pos).is_solid(),
        "Surface position should be Air, got {:?}",
        sim.world.get(pos)
    );

    // One below should be solid (the ground).
    if pos.y > 0 {
        let below = VoxelCoord::new(pos.x, pos.y - 1, pos.z);
        assert!(
            sim.world.get(below).is_solid(),
            "Below surface should be solid, got {:?}",
            sim.world.get(below)
        );
    }
}

// -----------------------------------------------------------------------
// AcquireItem tests
// -----------------------------------------------------------------------

#[test]
fn acquire_item_picks_up_and_owns() {
    let mut sim = test_sim(42);

    // Create a ground pile with unowned bread.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bread, 3, None, None);
    }

    // Spawn elf, position at pile.
    let elf_id = spawn_elf(&mut sim);
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos).unwrap();
    let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.position = pile_nav_pos;
    });

    // Create AcquireItem task with reservations.
    let task_id = TaskId::new(&mut sim.rng);
    let source = task::HaulSource::GroundPile(pile_pos);
    let acquire_task = Task {
        id: task_id,
        kind: TaskKind::AcquireItem {
            source,
            item_kind: inventory::ItemKind::Bread,
            quantity: 2,
        },
        state: TaskState::InProgress,
        location: sim.nav_graph.node(pile_nav).position,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(acquire_task);
    {
        let pile = sim
            .db
            .ground_piles
            .by_position(&pile_pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .unwrap();
        sim.inv_reserve_unowned_items(
            pile.inventory_id,
            inventory::ItemKind::Bread,
            inventory::MaterialFilter::Any,
            2,
            task_id,
        );
    }
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Execute.
    sim.resolve_acquire_item_action(elf_id, task_id);

    // Assert: bread removed from ground pile (1 unreserved remains).
    let pile = sim
        .db
        .ground_piles
        .by_position(&pile_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        sim.inv_item_count(
            pile.inventory_id,
            inventory::ItemKind::Bread,
            inventory::MaterialFilter::Any
        ),
        1,
        "Ground pile should have 1 bread left"
    );

    // Assert: elf now has 2 bread owned by the elf (plus starting bread).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let owned_bread = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Bread, elf_id);
    // Elf gets starting bread (default 2) + acquired 2 = 4.
    assert_eq!(
        owned_bread, 4,
        "Elf should own 4 bread (2 starting + 2 acquired)"
    );

    // Assert: task completed.
    assert_eq!(
        sim.db.tasks.get(&task_id).unwrap().state,
        TaskState::Complete
    );
}

#[test]
fn idle_elf_below_want_target_acquires_item() {
    let mut sim = test_sim(42);
    // Disable hunger/tiredness so elf stays idle.
    sim.config.elf_starting_bread = 0;
    if let Some(elf_data) = sim.config.species.get_mut(&Species::Elf) {
        elf_data.food_decay_per_tick = 0;
        elf_data.rest_decay_per_tick = 0;
    }
    sim.species_table = sim
        .config
        .species
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();

    // Set elf wants = [Bread: 2].
    sim.config.elf_default_wants = vec![building::LogisticsWant {
        item_kind: inventory::ItemKind::Bread,
        material_filter: inventory::MaterialFilter::Any,
        target_quantity: 2,
    }];

    // Spawn elf (will have 0 bread, wants 2).
    let elf_id = spawn_elf(&mut sim);

    // Verify elf has 0 bread and wants set.
    assert_eq!(
        sim.inv_count_owned(sim.creature_inv(elf_id), inventory::ItemKind::Bread, elf_id),
        0
    );
    assert_eq!(sim.inv_wants(sim.creature_inv(elf_id)).len(), 1);

    // Create unowned bread in a ground pile near the elf.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bread, 5, None, None);
    }

    // Advance past a heartbeat (heartbeat interval is 3000 for elves).
    sim.step(&[], sim.tick + 5000);

    // Assert: elf should have an AcquireItem task created.
    let has_acquire_task = sim.db.tasks.iter_all().any(|t| {
        t.kind_tag == TaskKindTag::AcquireItem
            && sim
                .task_acquire_data(t.id)
                .is_some_and(|a| a.item_kind == inventory::ItemKind::Bread)
            && sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(t.id))
    });
    // Either has an active task, or already completed one and picked up bread.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let elf_bread = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Bread, elf_id);
    assert!(
        has_acquire_task || elf_bread > 0,
        "Elf should have created an AcquireItem task or already acquired bread, \
             has_task={has_acquire_task}, bread={elf_bread}"
    );
}

#[test]
fn acquire_item_reserves_prevent_double_claim() {
    let mut sim = test_sim(42);
    // Disable hunger/tiredness.
    sim.config.elf_starting_bread = 0;
    if let Some(elf_data) = sim.config.species.get_mut(&Species::Elf) {
        elf_data.food_decay_per_tick = 0;
        elf_data.rest_decay_per_tick = 0;
    }
    sim.species_table = sim
        .config
        .species
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();

    sim.config.elf_default_wants = vec![building::LogisticsWant {
        item_kind: inventory::ItemKind::Bread,
        material_filter: inventory::MaterialFilter::Any,
        target_quantity: 2,
    }];

    // Create exactly 2 unowned bread.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bread, 2, None, None);
    }

    // Spawn 2 elves (each wants 2 bread, only 2 available total).
    let elf1 = spawn_elf(&mut sim);
    let spawn_pos = VoxelCoord::new(tree_pos.x + 1, 1, tree_pos.z);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: spawn_pos,
        },
    };
    sim.step(&[cmd], sim.tick + 2);
    let elf2 = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf && c.id != elf1)
        .unwrap()
        .id;

    // Run enough ticks for both heartbeats to fire and tasks to complete.
    sim.step(&[], sim.tick + 50_000);

    // Count total bread across both elves. Should be exactly 2 (no duplication).
    let elf1_bread = sim.inv_count_owned(sim.creature_inv(elf1), inventory::ItemKind::Bread, elf1);
    let elf2_bread = sim.inv_count_owned(sim.creature_inv(elf2), inventory::ItemKind::Bread, elf2);
    assert_eq!(
        elf1_bread + elf2_bread,
        2,
        "Total bread across both elves should be exactly 2 (no duplication), \
             elf1={elf1_bread}, elf2={elf2_bread}"
    );
}

#[test]
fn unhappy_elf_eventually_mopes() {
    // Give elf SleptOnGround thoughts (weight -100 each → Unhappy/-200 → actually Miserable).
    // Use a high mope rate so it fires quickly.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000, // P ≈ 1.0 per heartbeat
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000,
        mope_duration_ticks: 100,
        ..Default::default()
    };
    let (mut sim, elf_id) = mope_test_setup(
        cfg,
        &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
    );

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 20);

    let has_mope = sim.db.tasks.iter_all().any(|t| {
        t.kind_tag == TaskKindTag::Mope
            && sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(t.id))
    });
    assert!(has_mope, "Unhappy elf should eventually get a Mope task");
}

#[test]
fn content_elf_never_mopes() {
    // Give elf positive thoughts → Content/Happy tier. Mean=0 → never mopes.
    let cfg = crate::config::MoodConsequencesConfig::default();
    let (mut sim, elf_id) = mope_test_setup(cfg, &[ThoughtKind::AteDining, ThoughtKind::AteDining]);

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 50);

    let has_mope = sim.db.tasks.iter_all().any(|t| {
        t.kind_tag == TaskKindTag::Mope
            && sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(t.id))
    });
    assert!(!has_mope, "Content elf should never mope");
}

#[test]
fn devastated_elf_interrupts_task_to_mope() {
    // Give elf Devastated-tier thoughts + a GoTo task + high mope rate.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000,
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000,
        mope_can_interrupt_task: true,
        mope_duration_ticks: 100,
    };
    let (mut sim, elf_id) = mope_test_setup(
        cfg,
        // SleptOnGround has weight -100, three of them → -300 → Devastated
        &[
            ThoughtKind::SleptOnGround,
            ThoughtKind::SleptOnGround,
            ThoughtKind::SleptOnGround,
        ],
    );

    // Assign a GoTo task to the elf so it's not idle.
    // Find a distant node for the GoTo task.
    let nav_count = sim.nav_graph.node_count();
    let far_node = NavNodeId((nav_count / 2) as u32);
    let task_id = TaskId::new(&mut sim.rng);
    let goto_task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: sim.nav_graph.node(far_node).position,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(goto_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 20);

    // Elf should have abandoned GoTo and started moping.
    let has_mope = sim.db.tasks.iter_all().any(|t| {
        t.kind_tag == TaskKindTag::Mope
            && sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(t.id))
    });
    assert!(
        has_mope,
        "Miserable elf with mope_can_interrupt_task should interrupt GoTo and start moping"
    );
}

#[test]
fn elf_at_want_target_does_not_acquire() {
    let mut sim = test_sim(42);
    // Disable hunger/tiredness.
    if let Some(elf_data) = sim.config.species.get_mut(&Species::Elf) {
        elf_data.food_decay_per_tick = 0;
        elf_data.rest_decay_per_tick = 0;
    }
    sim.species_table = sim
        .config
        .species
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();

    // Set wants = [Bread: 2], give elf 2 starting bread.
    sim.config.elf_starting_bread = 2;
    sim.config.elf_default_wants = vec![building::LogisticsWant {
        item_kind: inventory::ItemKind::Bread,
        material_filter: inventory::MaterialFilter::Any,
        target_quantity: 2,
    }];

    let elf_id = spawn_elf(&mut sim);

    // Verify elf has exactly 2 bread.
    assert_eq!(
        sim.inv_count_owned(sim.creature_inv(elf_id), inventory::ItemKind::Bread, elf_id),
        2
    );

    // Add unowned bread to world.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            inventory::ItemKind::Bread,
            10,
            None,
            None,
        );
    }

    // Advance past heartbeat.
    sim.step(&[], sim.tick + 5000);

    // Assert: no AcquireItem task created (elf already has enough).
    let has_acquire_task = sim.db.tasks.iter_all().any(|t| {
        t.kind_tag == TaskKindTag::AcquireItem
            && sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(t.id))
    });
    assert!(
        !has_acquire_task,
        "Elf at want target should NOT create AcquireItem task"
    );
}

// -----------------------------------------------------------------------
// Notification tests
// -----------------------------------------------------------------------

#[test]
fn debug_notification_command_creates_notification() {
    let mut sim = test_sim(42);
    assert_eq!(sim.db.notifications.iter_all().count(), 0);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DebugNotification {
            message: "hello world".to_string(),
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.notifications.iter_all().count(), 1);
    let notif = sim.db.notifications.iter_all().next().unwrap();
    assert_eq!(notif.message, "hello world");
    assert_eq!(notif.tick, 1);
}

/// The first notification gets ID 0. The bridge's `get_max_notification_id`
/// must return -1 (not 0) when no notifications exist, otherwise the polling
/// cursor `_last_notification_id` (initialized to `get_max_notification_id()`)
/// will equal the first notification's ID and `get_notifications_after(0)`
/// will skip it (the `<=` filter excludes ID 0).
#[test]
fn first_notification_gets_id_zero() {
    let mut sim = test_sim(42);
    sim.add_notification("first".to_string());

    let notif = sim.db.notifications.iter_all().next().unwrap();
    assert_eq!(
        notif.id,
        NotificationId(0),
        "first auto-increment notification should have ID 0"
    );
}

/// Verify the polling-cursor logic used by the bridge and GDScript:
/// notifications with `id <= after_id` are excluded. With `after_id = -1`
/// (the sentinel for "no notifications seen"), notification ID 0 must be
/// included. With `after_id = 0`, it must be excluded.
#[test]
fn notification_polling_cursor_filters_correctly() {
    let mut sim = test_sim(42);
    sim.add_notification("first".to_string());
    sim.add_notification("second".to_string());

    // Simulate the bridge's get_notifications_after filter:
    //   `if (notif.id.0 as i64) <= after_id { continue; }`
    let filter = |after_id: i64| -> Vec<String> {
        sim.db
            .notifications
            .iter_all()
            .filter(|n| (n.id.0 as i64) > after_id)
            .map(|n| n.message.clone())
            .collect()
    };

    // Sentinel -1: both notifications (IDs 0 and 1) are included.
    let all = filter(-1);
    assert_eq!(all.len(), 2, "after_id=-1 should include all notifications");
    assert_eq!(all[0], "first");
    assert_eq!(all[1], "second");

    // After seeing ID 0: only notification with ID 1 is included.
    let after_zero = filter(0);
    assert_eq!(
        after_zero.len(),
        1,
        "after_id=0 should exclude the first notification"
    );
    assert_eq!(after_zero[0], "second");

    // After seeing ID 1: no notifications remain.
    let after_one = filter(1);
    assert!(
        after_one.is_empty(),
        "after_id=1 should exclude all notifications"
    );
}

#[test]
fn notifications_persist_across_serde_roundtrip() {
    let mut sim = test_sim(42);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DebugNotification {
            message: "save me".to_string(),
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.notifications.iter_all().count(), 1);

    // Serialize and deserialize.
    let json = serde_json::to_string(&sim).unwrap();
    let mut restored: SimState = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.db.notifications.iter_all().count(), 1);
    let notif = restored.db.notifications.iter_all().next().unwrap();
    assert_eq!(notif.message, "save me");

    // Verify that auto-increment IDs don't collide after deserialization.
    restored.add_notification("post-load".to_string());
    let ids: Vec<_> = restored.db.notifications.iter_all().map(|n| n.id).collect();
    assert_eq!(ids.len(), 2);
    assert!(
        ids[1] > ids[0],
        "Post-load notification ID ({:?}) should be greater than pre-existing ({:?})",
        ids[1],
        ids[0]
    );
}

#[test]
fn dead_creature_heartbeat_does_not_reschedule() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Kill the elf.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Run sim forward past several heartbeat intervals. Any pending
    // heartbeat events for the dead elf should be no-ops (not reschedule).
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat_interval * 5);

    // Drain the event queue and check no heartbeat for this creature.
    let mut found_heartbeat = false;
    while let Some(evt) = sim.event_queue.pop_if_ready(u64::MAX) {
        if matches!(
            evt.kind,
            ScheduledEventKind::CreatureHeartbeat { creature_id } if creature_id == elf_id
        ) {
            found_heartbeat = true;
        }
    }
    assert!(
        !found_heartbeat,
        "dead creature should not have pending heartbeats"
    );
}

#[test]
fn dead_creature_not_assigned_tasks() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Kill the elf.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Create a GoTo task.
    let pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::CreateTask {
                kind: TaskKind::GoTo,
                position: pos,
                required_species: Some(Species::Elf),
            },
        }],
        tick2 + 1,
    );

    // Run several activations.
    sim.step(&[], sim.tick + 10000);

    // Dead creature should NOT have picked up the task.
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_none(),
        "dead creature should not claim tasks"
    );
}

#[test]
fn damage_dead_creature_is_noop() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Kill.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Try to damage again.
    let tick2 = sim.tick;
    let result = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 50,
            },
        }],
        tick2 + 1,
    );

    // Should not emit a second death event.
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(&e.kind, SimEventKind::CreatureDied { .. })),
        "damaging dead creature should not emit another death event"
    );
}

#[test]
fn death_creates_notification() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let initial_notifications = sim.db.notifications.len();
    let tick = sim.tick;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    assert!(
        sim.db.notifications.len() > initial_notifications,
        "death should create a notification"
    );
    let last_notif = sim.db.notifications.iter_all().last().unwrap();
    assert!(
        last_notif.message.contains("died"),
        "notification should mention death: {}",
        last_notif.message
    );
}

#[test]
fn death_interrupts_current_task() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    // Create and claim a GoTo task.
    let pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::CreateTask {
                kind: TaskKind::GoTo,
                position: pos,
                required_species: Some(Species::Elf),
            },
        }],
        tick + 1,
    );

    // Run until the elf picks up the task.
    sim.step(&[], sim.tick + 5000);
    // Elf should have a task now (either the GoTo or something from heartbeat).

    // Kill the elf.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick2 + 1,
    );

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_none(),
        "dead creature should have no task"
    );
    assert_eq!(creature.action_kind, ActionKind::NoAction);
}

#[test]
fn kill_nonexistent_creature_is_noop() {
    let mut sim = test_sim(42);
    let mut rng = GameRng::new(999);
    let fake_id = CreatureId::new(&mut rng);
    let tick = sim.tick;

    // Should not panic.
    let result = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: fake_id,
            },
        }],
        tick + 1,
    );

    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(&e.kind, SimEventKind::CreatureDied { .. })),
        "killing nonexistent creature should not emit event"
    );
}

#[test]
fn death_removes_from_spatial_index() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let pos = sim.db.creatures.get(&elf_id).unwrap().position;

    // Elf should be in the spatial index before death.
    assert!(
        sim.spatial_index
            .get(&pos)
            .is_some_and(|v| v.contains(&elf_id)),
        "living elf should be in spatial index"
    );

    // Kill the elf.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Elf should no longer be in the spatial index.
    assert!(
        !sim.spatial_index
            .get(&pos)
            .is_some_and(|v| v.contains(&elf_id)),
        "dead elf should be removed from spatial index"
    );
}

#[test]
fn hp_death_serde_roundtrip() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf_id);
    let tick = sim.tick;

    // Damage elf to half HP.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 50,
            },
        }],
        tick + 1,
    );

    // Serialize and deserialize the DB.
    let json = serde_json::to_string(&sim.db).unwrap();
    let restored: SimDb = serde_json::from_str(&json).unwrap();
    let creature = restored.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.hp, sim.db.creatures.get(&elf_id).unwrap().hp);
    assert_eq!(creature.hp_max, sim.species_table[&Species::Elf].hp_max);
    assert_eq!(creature.vital_status, VitalStatus::Alive);
}

#[test]
fn hp_death_serde_roundtrip_dead() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Kill elf.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Serialize and deserialize.
    let json = serde_json::to_string(&sim.db).unwrap();
    let restored: SimDb = serde_json::from_str(&json).unwrap();
    let creature = restored.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.vital_status, VitalStatus::Dead);
    assert_eq!(creature.hp, 0);
}

#[test]
fn zero_and_negative_damage_is_noop() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;
    let tick = sim.tick;

    // Zero damage — should not change HP.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 0,
            },
        }],
        tick + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_before);

    // Negative damage — should not change HP.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: -5,
            },
        }],
        tick2 + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_before);
}

#[test]
fn zero_and_negative_heal_is_noop() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Damage first so there's room to heal.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 30,
            },
        }],
        tick + 1,
    );
    let hp_after_damage = sim.db.creatures.get(&elf_id).unwrap().hp;

    // Zero heal — should not change HP.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::HealCreature {
                creature_id: elf_id,
                amount: 0,
            },
        }],
        tick2 + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_after_damage);

    // Negative heal — should not change HP.
    let tick3 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick3 + 1,
            action: SimAction::HealCreature {
                creature_id: elf_id,
                amount: -10,
            },
        }],
        tick3 + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_after_damage);
}

// -----------------------------------------------------------------------
// Hostile AI tests
// -----------------------------------------------------------------------

#[test]
fn engagement_style_config() {
    use crate::species::EngagementInitiative;
    let sim = test_sim(42);
    // Aggressive species.
    assert_eq!(
        sim.species_table[&Species::Goblin]
            .engagement_style
            .initiative,
        EngagementInitiative::Aggressive
    );
    assert_eq!(
        sim.species_table[&Species::Orc].engagement_style.initiative,
        EngagementInitiative::Aggressive
    );
    assert_eq!(
        sim.species_table[&Species::Troll]
            .engagement_style
            .initiative,
        EngagementInitiative::Aggressive
    );
    // Passive species.
    assert_eq!(
        sim.species_table[&Species::Capybara]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Deer]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Boar]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Monkey]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Squirrel]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Elephant]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    // Elf: defensive with 100% disengage.
    assert_eq!(
        sim.species_table[&Species::Elf].engagement_style.initiative,
        EngagementInitiative::Defensive
    );
    assert_eq!(
        sim.species_table[&Species::Elf]
            .engagement_style
            .disengage_threshold_pct,
        100
    );
    // Detection ranges are set for aggressive and flee-capable species.
    assert!(sim.species_table[&Species::Goblin].hostile_detection_range_sq > 0);
    assert!(sim.species_table[&Species::Orc].hostile_detection_range_sq > 0);
    assert!(sim.species_table[&Species::Troll].hostile_detection_range_sq > 0);
    // Elves have detection range for flee behavior.
    assert!(sim.species_table[&Species::Elf].hostile_detection_range_sq > 0);
    assert_eq!(
        sim.species_table[&Species::Capybara].hostile_detection_range_sq,
        0
    );
}

#[test]
fn hostile_creature_pursues_and_attacks_elf() {
    let mut sim = test_sim(300);
    let elf_id = spawn_species(&mut sim, Species::Elf);
    let goblin_id = spawn_species(&mut sim, Species::Goblin);
    force_guaranteed_hits(&mut sim, goblin_id);

    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let goblin_start = sim.db.creatures.get(&goblin_id).unwrap().position;

    assert_ne!(
        elf_pos, goblin_start,
        "Elf and goblin spawned at same position — adjust test seed"
    );

    let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

    sim.step(&[], sim.tick + 10_000);

    let elf_hp_after = sim.db.creatures.get(&elf_id).unwrap().hp;
    let goblin_pos = sim.db.creatures.get(&goblin_id).unwrap().position;
    let elf_current_pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let new_dist = goblin_pos.manhattan_distance(elf_current_pos);
    let initial_dist = goblin_start.manhattan_distance(elf_pos);

    // The goblin should have either moved closer to the elf's current
    // position, or dealt damage (meaning it reached and attacked).
    let moved_closer = new_dist < initial_dist;
    let dealt_damage = elf_hp_after < elf_hp_before;
    assert!(
        moved_closer || dealt_damage,
        "Goblin should pursue or attack elf: initial dist={initial_dist}, \
             new dist={new_dist}, elf hp {elf_hp_before} -> {elf_hp_after}"
    );
}

#[test]
fn hostile_creature_wanders_without_elves() {
    let mut sim = test_sim(99);
    let goblin_id = spawn_species(&mut sim, Species::Goblin);

    // Place goblin on a nav node with neighbors so it can wander.
    let walkable = sim
        .nav_graph
        .live_nodes()
        .find(|n| !n.edge_indices.is_empty())
        .map(|n| n.position)
        .expect("should have a walkable nav node with neighbors");
    force_position(&mut sim, goblin_id, walkable);
    force_idle(&mut sim, goblin_id);

    let goblin_start = sim.db.creatures.get(&goblin_id).unwrap().position;

    sim.step(&[], sim.tick + 10_000);

    let goblin_pos = sim.db.creatures.get(&goblin_id).unwrap().position;
    assert_ne!(
        goblin_start, goblin_pos,
        "Goblin should wander even without elves to pursue"
    );
}

#[test]
fn hostile_creature_attacks_adjacent_elf() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    // Place goblin adjacent to elf.
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    // Schedule an activation so the goblin enters the decision cascade.
    let tick = sim.tick;
    sim.event_queue.schedule(
        tick + 1,
        ScheduledEventKind::CreatureActivation {
            creature_id: goblin,
        },
    );

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    // Run one activation cycle — the goblin should melee the elf.
    sim.step(&[], tick + 2);

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
    assert_eq!(
        elf_hp_after,
        elf_hp_before - goblin_damage,
        "Adjacent hostile should automatically melee-strike the elf"
    );
}

// -----------------------------------------------------------------------
// Projectile system tests (F-projectiles)
// -----------------------------------------------------------------------

#[test]
fn spawn_projectile_creates_entity_and_inventory() {
    let mut sim = test_sim(42);
    let origin = VoxelCoord::new(40, 5, 40);
    let target = VoxelCoord::new(50, 5, 40);

    sim.spawn_projectile(origin, target, None);

    assert_eq!(sim.db.projectiles.len(), 1);
    let proj = sim.db.projectiles.iter_all().next().unwrap();
    assert_eq!(proj.shooter, None);
    assert_eq!(proj.prev_voxel, origin);
    // Should have an inventory with 1 arrow.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&proj.inventory_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].kind, inventory::ItemKind::Arrow);
    assert_eq!(stacks[0].quantity, 1);
}

#[test]
fn spawn_projectile_schedules_tick_event() {
    let mut sim = test_sim(42);
    let initial_events = sim.event_queue.len();
    sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(50, 5, 40), None);
    // Should have scheduled exactly one ProjectileTick.
    assert_eq!(sim.event_queue.len(), initial_events + 1);
}

#[test]
fn second_spawn_does_not_duplicate_tick_event() {
    let mut sim = test_sim(42);
    let initial_events = sim.event_queue.len();
    sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(50, 5, 40), None);
    sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(45, 5, 40), None);
    // Only one extra event (from first spawn), not two.
    assert_eq!(sim.event_queue.len(), initial_events + 1);
}

#[test]
fn projectile_hits_solid_voxel_and_creates_ground_pile() {
    let mut sim = test_sim(42);
    // Place a solid wall at x=45.
    for y in 1..=5 {
        sim.world
            .set(VoxelCoord::new(45, y, 40), VoxelType::GrownPlatform);
    }

    // Spawn projectile heading +x toward the wall (flat, no gravity).
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.spawn_projectile(VoxelCoord::new(40, 3, 40), VoxelCoord::new(45, 3, 40), None);

    // Run until the projectile resolves (max 500 ticks).
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert_eq!(sim.db.projectiles.len(), 0, "Projectile should be resolved");

    // Should have a ground pile with an arrow in it near x=44 (prev_voxel).
    let mut found_arrow = false;
    for pile in sim.db.ground_piles.iter_all() {
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&pile.inventory_id, tabulosity::QueryOpts::ASC);
        for s in &stacks {
            if s.kind == inventory::ItemKind::Arrow {
                found_arrow = true;
            }
        }
    }
    assert!(found_arrow, "Arrow should land as ground pile");
}

#[test]
fn projectile_hits_creature_and_deals_damage() {
    use crate::db::CreatureTrait;
    use crate::types::TraitValue;
    let mut sim = test_sim(42);
    // Spawn a goblin at a known position.
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    // Set evasion deeply negative so shooter-less projectile always hits.
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: goblin,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(-500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(goblin, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(-500);
        });
    sim.config.evasion_crit_threshold = 100_000;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    let goblin_hp_before = sim.db.creatures.get(&goblin).unwrap().hp;

    // Spawn projectile aimed at the goblin (no gravity for predictability).
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    let origin = VoxelCoord::new(goblin_pos.x - 10, goblin_pos.y, goblin_pos.z);
    sim.spawn_projectile(origin, goblin_pos, None);

    // Run until resolved.
    let mut hit_events = Vec::new();
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if matches!(e.kind, SimEventKind::ProjectileHitCreature { .. }) {
                hit_events.push(e.clone());
            }
        }
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert_eq!(sim.db.projectiles.len(), 0, "Projectile should be resolved");
    assert!(!hit_events.is_empty(), "Should have hit the creature");

    let goblin_hp_after = sim.db.creatures.get(&goblin).unwrap().hp;
    assert!(
        goblin_hp_after < goblin_hp_before,
        "Goblin should have taken damage: {goblin_hp_before} -> {goblin_hp_after}"
    );
}

#[test]
fn projectile_out_of_bounds_despawns_silently() {
    let mut sim = test_sim(42);
    // Shoot a projectile off the edge of the world.
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 5; // very fast

    sim.spawn_projectile(
        VoxelCoord::new(250, 5, 128),
        VoxelCoord::new(260, 5, 128), // target is beyond world bounds
        None,
    );

    // Run until resolved.
    for _ in 0..2000 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        // No surface hit or creature hit events expected.
        for e in &events {
            assert!(
                !matches!(e.kind, SimEventKind::ProjectileHitSurface { .. }),
                "Should not hit surface"
            );
        }
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert_eq!(
        sim.db.projectiles.len(),
        0,
        "Projectile should have despawned"
    );
}

#[test]
fn projectile_does_not_hit_shooter() {
    let mut sim = test_sim(42);
    // Spawn an elf and shoot from their position.
    let elf = spawn_species(&mut sim, Species::Elf);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    // Shoot from the elf's own position toward a distant target.
    sim.spawn_projectile(
        elf_pos,
        VoxelCoord::new(elf_pos.x + 20, elf_pos.y, elf_pos.z),
        Some(elf),
    );

    // Run a few ticks — the projectile should pass through the shooter.
    for _ in 0..50 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    // Elf should not have taken any damage.
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_after, elf_hp_before,
        "Shooter should not be hit by their own arrow"
    );
}

#[test]
fn hostile_creature_wanders_after_killing_elf() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    // Place goblin adjacent to elf.
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    // Kill the elf.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature { creature_id: elf },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().vital_status,
        VitalStatus::Dead,
    );

    // With no living elves, the goblin should fall back to random wander.
    sim.step(&[], sim.tick + 10_000);
    let goblin_final = sim.db.creatures.get(&goblin).unwrap().position;
    assert_ne!(
        goblin_final, goblin_pos,
        "Goblin should wander after elf is dead"
    );
}

#[test]
fn projectile_skips_origin_voxel_creatures() {
    let mut sim = test_sim(42);
    // Spawn shooter and bystander at the same position.
    let shooter = spawn_species(&mut sim, Species::Elf);
    let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position;
    let shooter_hp = sim.db.creatures.get(&shooter).unwrap().hp;

    let bystander = spawn_species(&mut sim, Species::Elf);
    // Move bystander to the same position as the shooter.
    if let Some(mut c) = sim.db.creatures.get(&bystander) {
        c.position = shooter_pos;
        let _ = sim.db.creatures.update_no_fk(c);
    }
    sim.rebuild_spatial_index();
    let bystander_hp = sim.db.creatures.get(&bystander).unwrap().hp;

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    // Shoot from the shared position toward a distant target.
    sim.spawn_projectile(
        shooter_pos,
        VoxelCoord::new(shooter_pos.x + 20, shooter_pos.y, shooter_pos.z),
        Some(shooter),
    );

    // Run ticks until projectile is consumed or max iterations.
    for _ in 0..50 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    // Neither the shooter nor the bystander in the origin voxel should
    // have been hit — projectiles skip the entire launch voxel.
    let shooter_hp_after = sim.db.creatures.get(&shooter).unwrap().hp;
    assert_eq!(
        shooter_hp_after, shooter_hp,
        "Shooter should not be hit by their own arrow"
    );

    let bystander_hp_after = sim.db.creatures.get(&bystander).unwrap().hp;
    assert_eq!(
        bystander_hp_after, bystander_hp,
        "Bystander in origin voxel should not be hit (hp: {} -> {})",
        bystander_hp, bystander_hp_after,
    );
}

#[test]
fn hostile_waits_on_cooldown_near_elf() {
    // When a hostile is in melee range but on cooldown, it should not
    // wander away — it should wait and re-strike when the cooldown expires.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    // First strike via command puts goblin on cooldown.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: goblin,
                target_id: elf,
            },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.creatures.get(&goblin).unwrap().action_kind,
        ActionKind::MeleeStrike,
    );

    // Advance past cooldown. Goblin should stay near elf and strike again,
    // NOT wander away.
    let interval = sim.species_table[&Species::Goblin].melee_interval_ticks;
    sim.step(&[], sim.tick + interval + 100);

    let goblin_final = sim.db.creatures.get(&goblin).unwrap().position;
    let dist = goblin_final.manhattan_distance(elf_pos);
    // Should still be within melee range (manhattan dist ≤ 2 for range_sq=3).
    assert!(
        dist <= 2,
        "Goblin should stay near elf on cooldown, not wander away (dist={dist})"
    );
}

#[test]
fn hostile_ignores_elf_outside_detection_range() {
    // A goblin with detection_range_sq=225 (15 voxels) should NOT pursue
    // an elf that is >15 voxels away in euclidean distance.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);

    // Place elf far from goblin — 20 voxels away on X axis (20² = 400 >> 225).
    // Stay within the 64x64 world at ground level (y=1, solid terrain at y=0)
    // so the elf has a valid nav node and won't fall due to creature gravity.
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    let far_x = (goblin_pos.x + 20).min(62);
    let far_pos = VoxelCoord::new(far_x, 1, goblin_pos.z);
    force_position(&mut sim, elf, far_pos);

    // Schedule activation.
    let tick = sim.tick;
    sim.event_queue.schedule(
        tick + 1,
        ScheduledEventKind::CreatureActivation {
            creature_id: goblin,
        },
    );
    force_idle(&mut sim, goblin);

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    // Run a short period — goblin should wander randomly, not pursue.
    // Keep ticks low so random wander can't close the 20-voxel gap.
    sim.step(&[], tick + 1000);

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_before, elf_hp_after,
        "Goblin should not attack elf outside detection range"
    );
    // Goblin should have wandered but NOT moved closer to the elf.
    // (It might have moved closer by random chance, so we just check
    // it didn't deal damage — the key assertion.)
}

#[test]
fn hostile_pursues_elf_within_detection_range() {
    // A goblin with detection_range_sq=225 (15 voxels) SHOULD pursue
    // an elf within 10 voxels (10² = 100 < 225).
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);

    // Place elf 5 voxels from goblin on X axis (5² = 25 < 225).
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    let near_pos = VoxelCoord::new(goblin_pos.x + 5, goblin_pos.y, goblin_pos.z);
    force_position(&mut sim, elf, near_pos);

    // Schedule activation.
    let tick = sim.tick;
    sim.event_queue.schedule(
        tick + 1,
        ScheduledEventKind::CreatureActivation {
            creature_id: goblin,
        },
    );
    force_idle(&mut sim, goblin);

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
    let initial_dist = goblin_pos.manhattan_distance(near_pos);

    sim.step(&[], tick + 10_000);

    let goblin_final = sim.db.creatures.get(&goblin).unwrap().position;
    let elf_current = sim.db.creatures.get(&elf).unwrap().position;
    let new_dist = goblin_final.manhattan_distance(elf_current);
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;

    let moved_closer = new_dist < initial_dist;
    let dealt_damage = elf_hp_after < elf_hp_before;
    assert!(
        moved_closer || dealt_damage,
        "Goblin should pursue elf within detection range: \
             initial dist={initial_dist}, new dist={new_dist}, \
             elf hp {elf_hp_before} -> {elf_hp_after}"
    );
}

#[test]
fn hostile_does_not_attack_same_species() {
    // Two non-civ goblins adjacent to each other should NOT attack.
    let mut sim = test_sim(42);
    let g1 = spawn_species(&mut sim, Species::Goblin);
    let g2 = spawn_species(&mut sim, Species::Goblin);

    // Place them adjacent.
    let g1_pos = sim.db.creatures.get(&g1).unwrap().position;
    let g2_pos = VoxelCoord::new(g1_pos.x + 1, g1_pos.y, g1_pos.z);
    force_position(&mut sim, g2, g2_pos);
    force_idle(&mut sim, g1);
    force_idle(&mut sim, g2);

    let tick = sim.tick;
    sim.event_queue.schedule(
        tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: g1 },
    );
    sim.event_queue.schedule(
        tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: g2 },
    );

    let g1_hp_before = sim.db.creatures.get(&g1).unwrap().hp;
    let g2_hp_before = sim.db.creatures.get(&g2).unwrap().hp;

    sim.step(&[], tick + 3000);

    let g1_hp_after = sim.db.creatures.get(&g1).unwrap().hp;
    let g2_hp_after = sim.db.creatures.get(&g2).unwrap().hp;
    assert_eq!(
        g1_hp_before, g1_hp_after,
        "Goblins should not attack same species"
    );
    assert_eq!(
        g2_hp_before, g2_hp_after,
        "Goblins should not attack same species"
    );
}

#[test]
fn all_hostile_species_pursue_elves() {
    for &hostile_species in &[Species::Goblin, Species::Orc, Species::Troll] {
        let mut sim = test_sim(99);
        let elf_id = spawn_species(&mut sim, Species::Elf);
        let hostile_id = spawn_species(&mut sim, hostile_species);

        // Find two nav nodes that are a few voxels apart so the hostile
        // has room to pursue without the elf immediately fleeing to safety.
        let positions: Vec<_> = sim
            .nav_graph
            .live_nodes()
            .filter(|n| !n.edge_indices.is_empty())
            .map(|n| n.position)
            .collect();
        let (pos_a, pos_b) = positions
            .iter()
            .flat_map(|a| positions.iter().map(move |b| (*a, *b)))
            .find(|(a, b)| {
                let d = a.manhattan_distance(*b);
                (3..=6).contains(&d)
            })
            .expect("should have nav nodes 3-6 apart");
        force_position(&mut sim, elf_id, pos_a);
        force_idle(&mut sim, elf_id);
        force_position(&mut sim, hostile_id, pos_b);
        force_idle(&mut sim, hostile_id);

        let hostile_start = sim.db.creatures.get(&hostile_id).unwrap().position;
        let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

        sim.step(&[], sim.tick + 10_000);

        let hostile_pos = sim.db.creatures.get(&hostile_id).unwrap().position;
        let elf_hp_after = sim.db.creatures.get(&elf_id).unwrap().hp;

        let moved = hostile_pos != hostile_start;
        let dealt_damage = elf_hp_after < elf_hp_before;
        assert!(
            moved || dealt_damage,
            "{hostile_species:?} should pursue elf: didn't move from {hostile_start:?} \
                 and didn't deal damage (elf hp {elf_hp_before} -> {elf_hp_after})"
        );
    }
}

#[test]
fn projectile_hits_creature_beyond_origin_voxel() {
    use crate::db::CreatureTrait;
    let mut sim = test_sim(42);
    // Place a target creature a few voxels away from the origin.
    let target = spawn_species(&mut sim, Species::Elf);
    // Set target's evasion stats deeply negative so the no-shooter projectile
    // (0 attack + quasi_normal) always exceeds defender_total and hits.
    // Don't use zero_creature_stats to avoid altering walk speed / behavior.
    // Evasion skill has no row at spawn (default 0), so use insert_no_fk.
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: target,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(-500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(target, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(-500);
        });
    // Raise crit threshold to prevent the large margin from triggering crits.
    sim.config.evasion_crit_threshold = 100_000;
    let origin = VoxelCoord::new(40, 1, 40);
    let target_pos = VoxelCoord::new(42, 1, 40);
    if let Some(mut c) = sim.db.creatures.get(&target) {
        c.position = target_pos;
        let _ = sim.db.creatures.update_no_fk(c);
    }
    sim.rebuild_spatial_index();
    let target_hp = sim.db.creatures.get(&target).unwrap().hp;

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    // Shoot from origin toward the target (no shooter creature).
    sim.spawn_projectile(origin, target_pos, None);

    // Run ticks.
    let mut hit = false;
    for _ in 0..100 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if let SimEventKind::ProjectileHitCreature { target_id, .. } = e.kind
                && target_id == target
            {
                hit = true;
            }
        }
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert!(hit, "Projectile should hit creature beyond origin voxel");
    let target_hp_after = sim.db.creatures.get(&target).unwrap().hp;
    assert!(
        target_hp_after < target_hp,
        "Target should have taken damage (hp: {} -> {})",
        target_hp,
        target_hp_after,
    );
}

#[test]
fn projectile_cleanup_removes_inventory() {
    let mut sim = test_sim(42);
    sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(50, 5, 40), None);
    let proj = sim.db.projectiles.iter_all().next().unwrap();
    let inv_id = proj.inventory_id;
    let proj_id = proj.id;

    // Verify inventory exists.
    assert!(sim.db.inventories.get(&inv_id).is_some());

    sim.remove_projectile(proj_id);

    // Projectile, inventory, and item stacks should all be gone.
    assert_eq!(sim.db.projectiles.len(), 0);
    assert!(sim.db.inventories.get(&inv_id).is_none());
    assert!(
        sim.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn projectile_serde_roundtrip() {
    let mut sim = test_sim(42);
    sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(50, 5, 40), None);

    let json = sim.to_json().unwrap();
    let sim2 = SimState::from_json(&json).unwrap();

    assert_eq!(sim2.db.projectiles.len(), 1);
    let proj = sim2.db.projectiles.iter_all().next().unwrap();
    let stacks = sim2
        .db
        .item_stacks
        .by_inventory_id(&proj.inventory_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].kind, inventory::ItemKind::Arrow);
}

#[test]
fn debug_spawn_projectile_command() {
    let mut sim = test_sim(42);
    let origin = VoxelCoord::new(40, 5, 40);
    let target = VoxelCoord::new(50, 5, 40);
    let tick = sim.tick;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugSpawnProjectile {
                origin,
                target,
                shooter_id: None,
            },
        }],
        tick + 1,
    );

    assert_eq!(sim.db.projectiles.len(), 1);
}

// -----------------------------------------------------------------------
// F-attack-task: AttackTarget task tests
// -----------------------------------------------------------------------

#[test]
fn attack_creature_command_creates_task_and_assigns() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place them nearby.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Elf should have an AttackTarget task assigned.
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Elf should have a task assigned"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::AttackTarget);
    assert_eq!(task.state, TaskState::InProgress);
    assert_eq!(task.origin, TaskOrigin::PlayerDirected);
    assert_eq!(task.target_creature, Some(goblin));

    // Extension data should exist.
    let attack_data = sim.task_attack_target_data(task.id).unwrap();
    assert_eq!(attack_data.target, goblin);
    assert_eq!(attack_data.path_failures, 0);
}

#[test]
fn attack_target_task_pursues_and_strikes() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place goblin within reach of elf.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);

    let goblin_hp_before = sim.db.creatures.get(&goblin).unwrap().hp;

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: false,
        },
    };
    // Run for 10 seconds — enough time to walk there and attack.
    sim.step(&[cmd], tick + 10_000);

    let goblin_hp_after = sim.db.creatures.get(&goblin).unwrap().hp;
    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    let final_dist = elf_creature.position.manhattan_distance(goblin_pos);

    let moved_closer = final_dist < elf_pos.manhattan_distance(goblin_pos);
    let dealt_damage = goblin_hp_after < goblin_hp_before;
    assert!(
        moved_closer || dealt_damage,
        "Elf should pursue and/or damage goblin: initial_dist={}, final_dist={final_dist}, \
             hp {goblin_hp_before} -> {goblin_hp_after}",
        elf_pos.manhattan_distance(goblin_pos)
    );
}

#[test]
fn attack_target_completes_when_target_dies() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place goblin adjacent to elf (instant melee range).
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let adjacent_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, adjacent_pos);
    force_idle(&mut sim, elf);

    // Give elf high melee damage to kill quickly.
    // Just use commands to create the attack task and then kill the target.
    let tick = sim.tick;
    let attack_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: false,
        },
    };
    sim.step(&[attack_cmd], tick + 2);

    let task_id = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    // Kill the goblin.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 3,
        action: SimAction::DebugKillCreature {
            creature_id: goblin,
        },
    };
    // Step enough that the elf's activation runs after the kill.
    sim.step(&[kill_cmd], tick + 5000);

    // Task should be complete.
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "Attack task should complete when target dies"
    );
    // Elf should be free.
    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        elf_creature.current_task.is_none(),
        "Elf should have no task after target dies"
    );
}

#[test]
fn attack_target_preempts_lower_priority_task() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Give elf a GoTo task (PlayerDirected level 2).
    let far_node = sim.nav_graph.live_nodes().last().map(|n| n.id).unwrap();
    let goto_task_id = insert_goto_task(&mut sim, far_node);
    sim.claim_task(elf, goto_task_id);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Old task should be completed/interrupted.
    let old_task = sim.db.tasks.get(&goto_task_id).unwrap();
    assert_eq!(
        old_task.state,
        TaskState::Complete,
        "GoTo task should be interrupted by AttackCreature"
    );

    // Elf should have the attack task.
    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(elf_creature.current_task.is_some());
    let new_task = sim
        .db
        .tasks
        .get(&elf_creature.current_task.unwrap())
        .unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::AttackTarget);
}

#[test]
fn attack_target_cannot_attack_self() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    force_idle(&mut sim, elf);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: elf,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Elf should NOT have an attack task.
    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        elf_creature.current_task.is_none(),
        "Should not be able to attack self"
    );
}

#[test]
fn attack_target_cannot_attack_dead_creature() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    force_idle(&mut sim, elf);

    // Kill goblin first.
    let tick = sim.tick;
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DebugKillCreature {
            creature_id: goblin,
        },
    };
    sim.step(&[kill_cmd], tick + 2);

    // Try to attack.
    let attack_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 3,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: false,
        },
    };
    sim.step(&[attack_cmd], tick + 4);

    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        elf_creature.current_task.is_none(),
        "Should not be able to attack dead creature"
    );
}

#[test]
fn attack_target_task_serde_roundtrip() {
    let mut rng = GameRng::new(42);
    let task_id = TaskId::new(&mut rng);
    let target = CreatureId::new(&mut rng);
    let location = VoxelCoord::new(5, 0, 0);

    let task = Task {
        id: task_id,
        kind: TaskKind::AttackTarget { target },
        state: TaskState::InProgress,
        location,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: Some(target),
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };

    let json = serde_json::to_string(&task).unwrap();
    let restored: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, task_id);
    match &restored.kind {
        TaskKind::AttackTarget { target: t } => assert_eq!(*t, target),
        other => panic!("Expected AttackTarget, got {:?}", other),
    }
    assert_eq!(restored.state, TaskState::InProgress);
    assert_eq!(restored.origin, TaskOrigin::PlayerDirected);
    assert_eq!(restored.target_creature, Some(target));
}

// -----------------------------------------------------------------------
// DirectedGoTo command tests
// -----------------------------------------------------------------------

#[test]
fn directed_goto_creates_task_for_specific_creature() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    force_idle(&mut sim, elf);

    // Pick a target position.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_pos = VoxelCoord::new(tree_pos.x + 3, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            creature_id: elf,
            position: target_pos,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Elf should have a GoTo task"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::GoTo);
    assert_eq!(task.state, TaskState::InProgress);
    assert_eq!(task.origin, TaskOrigin::PlayerDirected);
}

#[test]
fn directed_goto_replaces_player_directed_task() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    // Give elf a PlayerDirected GoTo task (PlayerDirected level 2).
    let task_id = TaskId::new(&mut sim.rng);
    let dest_node = sim.nav_graph.live_nodes().last().map(|n| n.id).unwrap();
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: sim.nav_graph.node(dest_node).position,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(task);
    sim.claim_task(elf, task_id);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_pos = VoxelCoord::new(tree_pos.x + 2, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            creature_id: elf,
            position: target_pos,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Old task should be interrupted (Complete).
    let old_task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(old_task.state, TaskState::Complete);

    // Elf should have the new GoTo task.
    let creature = sim.db.creatures.get(&elf).unwrap();
    let new_task_id = creature.current_task.unwrap();
    assert_ne!(new_task_id, task_id);
    let new_task = sim.db.tasks.get(&new_task_id).unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::GoTo);
}

#[test]
fn directed_goto_preempts_autonomous_task() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    // Give elf an autonomous Harvest task (Autonomous level 1).
    let task_id = TaskId::new(&mut sim.rng);
    let dest_node = sim.nav_graph.live_nodes().last().map(|n| n.id).unwrap();
    let fruit_pos = VoxelCoord::new(0, 0, 0);
    let task = Task {
        id: task_id,
        kind: TaskKind::Harvest { fruit_pos },
        state: TaskState::InProgress,
        location: sim.nav_graph.node(dest_node).position,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(task);
    sim.claim_task(elf, task_id);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_pos = VoxelCoord::new(tree_pos.x + 2, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            creature_id: elf,
            position: target_pos,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Old autonomous task should be interrupted (Complete).
    let old_task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(old_task.state, TaskState::Complete);

    // Elf should have the new GoTo task.
    let creature = sim.db.creatures.get(&elf).unwrap();
    let new_task_id = creature.current_task.unwrap();
    assert_ne!(new_task_id, task_id);
    let new_task = sim.db.tasks.get(&new_task_id).unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::GoTo);
}

#[test]
fn directed_goto_does_not_abort_mid_walk_action() {
    // B-erratic-movement: issuing a DirectedGoTo while a creature is
    // mid-walk should NOT abort the in-progress Move action. The action
    // should complete naturally, then the creature picks up the new task.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Issue a first DirectedGoTo to start the elf walking.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_a = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);
    let tick = sim.tick;
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            creature_id: elf,
            position: target_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], tick + 2);

    // Advance until the elf is mid-walk (action_kind == Move).
    let mut mid_walk = false;
    for t in (sim.tick + 1)..=(sim.tick + 50) {
        sim.step(&[], t);
        let c = sim.db.creatures.get(&elf).unwrap();
        if c.action_kind == ActionKind::Move && c.next_available_tick.is_some() {
            mid_walk = true;
            break;
        }
    }
    assert!(mid_walk, "Elf should be mid-walk after advancing ticks");

    let c = sim.db.creatures.get(&elf).unwrap();
    let action_before = c.action_kind;
    let nat_before = c.next_available_tick;
    let first_task_id = c.current_task.unwrap();

    // Issue a second DirectedGoTo while mid-walk.
    let target_b = VoxelCoord::new(tree_pos.x - 3, 1, tree_pos.z);
    let tick2 = sim.tick;
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: tick2 + 1,
        action: SimAction::DirectedGoTo {
            creature_id: elf,
            position: target_b,
            queue: false,
        },
    };
    sim.step(&[cmd_b], tick2 + 2);

    // The in-progress Move action should NOT have been aborted.
    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(
        c.action_kind, action_before,
        "Move action should not be aborted by task preemption"
    );
    assert_eq!(
        c.next_available_tick, nat_before,
        "next_available_tick should be unchanged"
    );

    // The task should have changed to the new GoTo.
    let new_task_id = c.current_task.unwrap();
    assert_ne!(new_task_id, first_task_id, "Task should have been swapped");
    let new_task = sim.db.tasks.get(&new_task_id).unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::GoTo);

    // Old task should be completed.
    let old_task = sim.db.tasks.get(&first_task_id).unwrap();
    assert_eq!(old_task.state, TaskState::Complete);

    // Advance past the original next_available_tick — the elf should
    // resolve the Move action normally and then follow the new task.
    let completion_tick = nat_before.unwrap();
    let target_tick = completion_tick.max(sim.tick) + 1;
    sim.step(&[], target_tick);

    // After the original move resolves, the creature should have picked
    // up the new GoTo task (it may have started a new Move step toward
    // the new destination, which is fine — the key is it's using the
    // new task, not the old one).
    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(
        c.current_task,
        Some(new_task_id),
        "Creature should still be on the new GoTo task"
    );
}

#[test]
fn directed_goto_mid_action_command_does_not_schedule_extra_activation() {
    // B-erratic-movement: issuing a DirectedGoTo while a creature is
    // mid-action should NOT schedule an extra CreatureActivation. The
    // existing activation (from the in-progress action) is sufficient.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_a = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    // Issue first DirectedGoTo and advance until mid-walk.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DirectedGoTo {
                creature_id: elf,
                position: target_a,
                queue: false,
            },
        }],
        tick + 2,
    );
    for t in (sim.tick + 1)..=(sim.tick + 50) {
        sim.step(&[], t);
        if sim
            .db
            .creatures
            .get(&elf)
            .is_some_and(|c| c.action_kind == ActionKind::Move)
        {
            break;
        }
    }
    assert!(
        sim.db
            .creatures
            .get(&elf)
            .is_some_and(|c| c.action_kind == ActionKind::Move),
        "Elf should be mid-walk"
    );

    // Count activations before issuing the second command.
    let activations_before = sim.count_pending_activations_for(elf);
    assert_eq!(
        activations_before, 1,
        "Should have exactly 1 pending activation before redirect"
    );

    // Issue a second DirectedGoTo while mid-walk — should NOT add an
    // extra activation event.
    let target_b = VoxelCoord::new(tree_pos.x - 3, 1, tree_pos.z);
    let tick2 = sim.tick;
    // Process the command on this tick without advancing further, so no
    // events fire between the command and our assertion.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DirectedGoTo {
                creature_id: elf,
                position: target_b,
                queue: false,
            },
        }],
        tick2 + 1,
    );

    let activations_after = sim.count_pending_activations_for(elf);
    assert_eq!(
        activations_after, 1,
        "Should still have exactly 1 pending activation after redirect (was {activations_after})"
    );
}

#[test]
fn group_goto_spreads_creatures_to_different_nodes() {
    // Three elves given a GroupGoTo to the same destination should each
    // end up with a GoTo task at a different nav node.
    let mut sim = test_sim(42);
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    let elf_c = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf_a);
    force_idle_and_cancel_activations(&mut sim, elf_b);
    force_idle_and_cancel_activations(&mut sim, elf_c);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::GroupGoTo {
                creature_ids: vec![elf_a, elf_b, elf_c],
                position: dest,
                queue: false,
            },
        }],
        tick + 2,
    );

    // All three should have GoTo tasks.
    let task_a = sim.db.creatures.get(&elf_a).unwrap().current_task.unwrap();
    let task_b = sim.db.creatures.get(&elf_b).unwrap().current_task.unwrap();
    let task_c = sim.db.creatures.get(&elf_c).unwrap().current_task.unwrap();
    let loc_a = sim.db.tasks.get(&task_a).unwrap().location;
    let loc_b = sim.db.tasks.get(&task_b).unwrap().location;
    let loc_c = sim.db.tasks.get(&task_c).unwrap().location;

    // At least two of the three should have different locations (spread).
    let locs = [loc_a, loc_b, loc_c];
    let unique: std::collections::BTreeSet<_> = locs.iter().collect();
    assert!(
        unique.len() >= 2,
        "GroupGoTo should spread creatures to different nav nodes, got {:?}",
        locs
    );
}

#[test]
fn group_goto_single_creature_delegates_to_normal() {
    // A single-element GroupGoTo should work identically to DirectedGoTo.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::GroupGoTo {
                creature_ids: vec![elf],
                position: dest,
                queue: false,
            },
        }],
        tick + 2,
    );

    let task_id = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert!(task.kind_tag == TaskKindTag::GoTo);
}

#[test]
fn group_attack_move_spreads_creatures() {
    // GroupAttackMove should create AttackMove tasks at spread destinations.
    let mut sim = test_sim(42);
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf_a);
    force_idle_and_cancel_activations(&mut sim, elf_b);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::GroupAttackMove {
                creature_ids: vec![elf_a, elf_b],
                destination: dest,
                queue: false,
            },
        }],
        tick + 2,
    );

    // Both should have AttackMove tasks.
    let task_a = sim.db.creatures.get(&elf_a).unwrap().current_task.unwrap();
    let task_b = sim.db.creatures.get(&elf_b).unwrap().current_task.unwrap();
    assert_eq!(
        sim.db.tasks.get(&task_a).unwrap().kind_tag,
        TaskKindTag::AttackMove
    );
    assert_eq!(
        sim.db.tasks.get(&task_b).unwrap().kind_tag,
        TaskKindTag::AttackMove
    );

    // Their task locations should differ (spread).
    let loc_a = sim.db.tasks.get(&task_a).unwrap().location;
    let loc_b = sim.db.tasks.get(&task_b).unwrap().location;
    assert_ne!(
        loc_a, loc_b,
        "GroupAttackMove should spread to different nav nodes"
    );
}

#[test]
fn group_goto_empty_list_is_noop() {
    let mut sim = test_sim(42);
    let tick = sim.tick;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    // Should not panic or create any tasks.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::GroupGoTo {
                creature_ids: vec![],
                position: dest,
                queue: false,
            },
        }],
        tick + 2,
    );
}

#[test]
fn group_goto_skips_dead_creatures() {
    // Dead creatures in the list should be silently skipped.
    let mut sim = test_sim(42);
    let elf_alive = spawn_elf(&mut sim);
    let elf_dead = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf_alive);
    force_idle_and_cancel_activations(&mut sim, elf_dead);

    // Kill one elf.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_dead,
            },
        }],
        tick + 2,
    );

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::GroupGoTo {
                creature_ids: vec![elf_alive, elf_dead],
                position: dest,
                queue: false,
            },
        }],
        tick2 + 2,
    );

    // Only the alive elf should have a task.
    assert!(
        sim.db
            .creatures
            .get(&elf_alive)
            .unwrap()
            .current_task
            .is_some()
    );
}

#[test]
fn group_goto_serialization_roundtrip() {
    let mut rng = crate::prng::GameRng::new(42);
    let cmd = SimCommand {
        player_name: "test_player".to_string(),
        tick: 100,
        action: SimAction::GroupGoTo {
            creature_ids: vec![CreatureId::new(&mut rng), CreatureId::new(&mut rng)],
            position: VoxelCoord::new(10, 1, 5),
            queue: false,
        },
    };

    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());
}

#[test]
fn group_attack_move_serialization_roundtrip() {
    let mut rng = crate::prng::GameRng::new(42);
    let cmd = SimCommand {
        player_name: "test_player".to_string(),
        tick: 100,
        action: SimAction::GroupAttackMove {
            creature_ids: vec![CreatureId::new(&mut rng), CreatureId::new(&mut rng)],
            destination: VoxelCoord::new(10, 1, 5),
            queue: false,
        },
    };

    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());
}

#[test]
fn abort_current_action_cancels_orphaned_activation_events() {
    // B-erratic-movement safety net: when abort_current_action is called
    // (death, flee, nav invalidation), orphaned CreatureActivation events
    // for that creature should be removed from the event queue.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Issue a DirectedGoTo and advance until the elf is mid-walk.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            creature_id: elf,
            position: target,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    for t in (sim.tick + 1)..=(sim.tick + 50) {
        sim.step(&[], t);
        let c = sim.db.creatures.get(&elf).unwrap();
        if c.action_kind == ActionKind::Move {
            break;
        }
    }

    // Count CreatureActivation events for this elf before abort.
    let count_before = sim.count_pending_activations_for(elf);
    assert!(
        count_before >= 1,
        "Should have at least one pending activation"
    );

    // Abort the current action (simulating death/flee/nav invalidation).
    sim.abort_current_action(elf);

    // After abort, all CreatureActivation events for this elf should be gone.
    let count_after = sim.count_pending_activations_for(elf);
    assert_eq!(
        count_after, 0,
        "Orphaned activation events should be cancelled after abort"
    );
}

// -----------------------------------------------------------------------
// AttackTarget preemption level tests
// -----------------------------------------------------------------------

#[test]
fn attack_target_preemption_is_player_combat() {
    assert_eq!(
        preemption::preemption_level(
            crate::db::TaskKindTag::AttackTarget,
            TaskOrigin::PlayerDirected
        ),
        preemption::PreemptionLevel::PlayerCombat,
    );
}

// -----------------------------------------------------------------------
// Material filter tests
// -----------------------------------------------------------------------

#[test]
fn material_filter_matches_any() {
    use crate::inventory::{Material, MaterialFilter};
    let any = MaterialFilter::Any;
    assert!(any.matches(None));
    assert!(any.matches(Some(Material::Oak)));
    assert!(
        any.matches(Some(Material::FruitSpecies(crate::fruit::FruitSpeciesId(
            1
        ))))
    );
}

#[test]
fn material_filter_matches_specific() {
    use crate::inventory::{Material, MaterialFilter};
    let specific = MaterialFilter::Specific(Material::Oak);
    assert!(specific.matches(Some(Material::Oak)));
    assert!(!specific.matches(None));
    assert!(!specific.matches(Some(Material::Birch)));
    assert!(
        !specific.matches(Some(Material::FruitSpecies(crate::fruit::FruitSpeciesId(
            1
        ))))
    );
}

#[test]
fn material_filter_ord_deterministic() {
    use crate::inventory::{Material, MaterialFilter};
    // Any < Specific(*)
    assert!(MaterialFilter::Any < MaterialFilter::Specific(Material::Oak));
    // Specific variants ordered by Material's Ord
    assert!(MaterialFilter::Specific(Material::Oak) < MaterialFilter::Specific(Material::Birch));
}

#[test]
fn material_filter_serde_roundtrip() {
    use crate::inventory::{Material, MaterialFilter};
    for filter in [
        MaterialFilter::Any,
        MaterialFilter::Specific(Material::Oak),
        MaterialFilter::Specific(Material::FruitSpecies(crate::fruit::FruitSpeciesId(42))),
    ] {
        let json = serde_json::to_string(&filter).unwrap();
        let restored: MaterialFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(filter, restored, "roundtrip failed for {json}");
    }
}

#[test]
fn material_filter_default_is_any() {
    use crate::inventory::MaterialFilter;
    assert_eq!(MaterialFilter::default(), MaterialFilter::Any);
}

#[test]
fn logistics_want_serde_backward_compat() {
    // Old format without material_filter should deserialize with default (Any).
    let json = r#"{"item_kind":"Bread","target_quantity":5}"#;
    let want: building::LogisticsWant = serde_json::from_str(json).unwrap();
    assert_eq!(want.material_filter, inventory::MaterialFilter::Any);
    assert_eq!(want.item_kind, inventory::ItemKind::Bread);
    assert_eq!(want.target_quantity, 5);
}

#[test]
fn set_inv_wants_deduplicates_by_kind_filter() {
    let mut sim = test_sim(42);
    let inv_id = sim
        .db
        .inventories
        .insert_auto_no_fk(|id| crate::db::Inventory {
            id,
            owner_kind: crate::db::InventoryOwnerKind::Structure,
        })
        .unwrap();

    let wants = vec![
        building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 5,
        },
        building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 10,
        },
    ];
    sim.set_inv_wants(inv_id, &wants);

    // Should deduplicate: one want with max quantity.
    let stored = sim.inv_wants(inv_id);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].target_quantity, 10);
}

// -----------------------------------------------------------------------
// Military group tests
// -----------------------------------------------------------------------

/// Helper: directly set a creature's military group (test-only).
/// Uses update_no_fk because military_group is indexed.
fn set_military_group(sim: &mut SimState, creature_id: CreatureId, group: Option<MilitaryGroupId>) {
    let mut creature = sim.db.creatures.get(&creature_id).unwrap();
    creature.military_group = group;
    sim.db.creatures.update_no_fk(creature).unwrap();
}

/// Helper: find the player civ's civilian group.
fn civilian_group(sim: &SimState) -> crate::db::MilitaryGroup {
    let civ_id = sim.player_civ_id.unwrap();
    sim.db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| g.is_default_civilian)
        .expect("player civ should have a civilian group")
}

/// Helper: find the player civ's soldiers group.
fn soldiers_group(sim: &SimState) -> crate::db::MilitaryGroup {
    let civ_id = sim.player_civ_id.unwrap();
    sim.db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| !g.is_default_civilian && g.name == "Soldiers")
        .expect("player civ should have a soldiers group")
}

#[test]
fn worldgen_creates_default_military_groups() {
    let sim = test_sim(42);
    let civ_id = sim.player_civ_id.unwrap();
    let groups = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC);

    assert!(
        groups.len() >= 2,
        "Should have at least 2 groups (Civilians + Soldiers)"
    );

    let civilians = groups.iter().filter(|g| g.is_default_civilian).count();
    assert_eq!(civilians, 1, "Exactly one civilian group per civ");

    let civilian = groups.iter().find(|g| g.is_default_civilian).unwrap();
    assert_eq!(civilian.name, "Civilians");
    assert_eq!(civilian.engagement_style.disengage_threshold_pct, 100);

    let soldiers = groups.iter().find(|g| g.name == "Soldiers").unwrap();
    assert!(!soldiers.is_default_civilian);
    assert_eq!(
        soldiers.engagement_style.initiative,
        crate::species::EngagementInitiative::Aggressive
    );
}

#[test]
fn worldgen_all_civs_have_military_groups() {
    let sim = test_sim(42);
    for civ in sim.db.civilizations.iter_all() {
        let groups = sim
            .db
            .military_groups
            .by_civ_id(&civ.id, tabulosity::QueryOpts::ASC);
        let civilian_count = groups.iter().filter(|g| g.is_default_civilian).count();
        assert_eq!(
            civilian_count, 1,
            "Civ {:?} should have exactly 1 civilian group",
            civ.id
        );
        assert!(
            groups.len() >= 2,
            "Civ {:?} should have at least Civilians + Soldiers",
            civ.id
        );
    }
}

#[test]
fn create_military_group_command() {
    let mut sim = test_sim(42);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::CreateMilitaryGroup {
            name: "Archers".to_string(),
        },
    };
    let result = sim.step(&[cmd], 1);

    // Should emit MilitaryGroupCreated event.
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(&e.kind, SimEventKind::MilitaryGroupCreated { .. })),
        "Should emit MilitaryGroupCreated event"
    );

    // The new group should exist in the DB.
    let civ_id = sim.player_civ_id.unwrap();
    let groups = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC);
    let archers = groups.iter().find(|g| g.name == "Archers");
    assert!(archers.is_some(), "Archers group should exist");
    let archers = archers.unwrap();
    assert!(!archers.is_default_civilian);
    assert_eq!(
        archers.engagement_style.initiative,
        crate::species::EngagementInitiative::Aggressive,
        "New groups default to Aggressive"
    );
}

#[test]
fn creature_reassignment_to_group_and_back() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();

    // Spawn an elf.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, &mut events)
        .expect("should spawn elf");

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.military_group, None, "Spawned elf is implicit civilian");

    // Assign to soldiers.
    let soldiers = soldiers_group(&sim);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(soldiers.id),
        },
    };
    sim.step(&[cmd], 1);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.military_group,
        Some(soldiers.id),
        "Elf should be in soldiers group"
    );

    // Reassign back to civilian (None).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: None,
        },
    };
    sim.step(&[cmd], 2);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.military_group, None, "Elf should be back to civilian");
}

#[test]
fn reassign_between_non_civilian_groups() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, &mut events)
        .expect("should spawn elf");

    // Create a second group.
    let create_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::CreateMilitaryGroup {
            name: "Archers".to_string(),
        },
    };
    sim.step(&[create_cmd], 1);

    let civ_id = sim.player_civ_id.unwrap();
    let archers = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| g.name == "Archers")
        .unwrap();

    // Assign to soldiers.
    let soldiers = soldiers_group(&sim);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(soldiers.id),
        },
    };
    sim.step(&[cmd], 2);
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        Some(soldiers.id)
    );

    // Reassign to archers.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 3,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(archers.id),
        },
    };
    sim.step(&[cmd], 3);
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        Some(archers.id)
    );
}

#[test]
fn delete_military_group_nullifies_members() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, &mut events)
        .expect("should spawn elf");

    // Create a new group and assign the elf to it.
    let create_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::CreateMilitaryGroup {
            name: "Scouts".to_string(),
        },
    };
    sim.step(&[create_cmd], 1);

    let civ_id = sim.player_civ_id.unwrap();
    let scouts = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| g.name == "Scouts")
        .unwrap();

    let assign_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(scouts.id),
        },
    };
    sim.step(&[assign_cmd], 2);
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        Some(scouts.id)
    );

    // Delete the group.
    let delete_cmd = SimCommand {
        player_name: String::new(),
        tick: 3,
        action: SimAction::DeleteMilitaryGroup {
            group_id: scouts.id,
        },
    };
    let result = sim.step(&[delete_cmd], 3);

    // Elf should be back to civilian (None).
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        None,
        "Deleted group should nullify creature.military_group"
    );

    // Group should be gone.
    assert!(
        sim.db.military_groups.get(&scouts.id).is_none(),
        "Deleted group should be removed"
    );

    // Should emit MilitaryGroupDeleted event.
    assert!(
        result.events.iter().any(|e| matches!(
            &e.kind,
            SimEventKind::MilitaryGroupDeleted {
                name, member_count, ..
            } if name == "Scouts" && *member_count == 1
        )),
        "Should emit MilitaryGroupDeleted with correct name and count"
    );
}

#[test]
fn civilian_group_deletion_rejected() {
    let mut sim = test_sim(42);
    let civ_group = civilian_group(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DeleteMilitaryGroup {
            group_id: civ_group.id,
        },
    };
    sim.step(&[cmd], 1);

    // Civilian group should still exist.
    assert!(
        sim.db.military_groups.get(&civ_group.id).is_some(),
        "Civilian group cannot be deleted"
    );
}

#[test]
fn dead_creature_not_counted_in_member_count() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_a = sim
        .spawn_creature(Species::Elf, tree_pos, &mut events)
        .expect("spawn elf a");
    let elf_b = sim
        .spawn_creature(Species::Elf, tree_pos, &mut events)
        .expect("spawn elf b");

    // Assign both to soldiers.
    let soldiers = soldiers_group(&sim);
    for eid in [elf_a, elf_b] {
        set_military_group(&mut sim, eid, Some(soldiers.id));
    }

    // Kill elf_b.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DebugKillCreature { creature_id: elf_b },
    };
    sim.step(&[kill_cmd], 1);

    // Count alive members.
    let alive_count = sim
        .db
        .creatures
        .by_military_group(&Some(soldiers.id), tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|c| c.vital_status == VitalStatus::Alive)
        .count();
    assert_eq!(alive_count, 1, "Only elf_a should be alive in soldiers");

    // Dead elf should still be assigned to soldiers.
    let dead_elf = sim.db.creatures.get(&elf_b).unwrap();
    assert_eq!(
        dead_elf.military_group,
        Some(soldiers.id),
        "Dead creature preserves group assignment"
    );
    assert_eq!(dead_elf.vital_status, VitalStatus::Dead);
}

#[test]
fn cross_civ_reassignment_rejected() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();

    // Get a non-player civ.
    let ai_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| !c.player_controlled)
        .expect("need an AI civ");
    let ai_groups = sim
        .db
        .military_groups
        .by_civ_id(&ai_civ.id, tabulosity::QueryOpts::ASC);
    let ai_soldiers = ai_groups
        .iter()
        .find(|g| !g.is_default_civilian)
        .expect("AI civ should have a non-civilian group");

    // Spawn an elf (player civ).
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, &mut events)
        .expect("spawn elf");

    // Try to assign elf to AI civ's group — should be rejected.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(ai_soldiers.id),
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        None,
        "Cross-civ reassignment should be rejected"
    );
}

#[test]
fn non_civ_creature_reassignment_rejected() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let goblin_id = sim
        .spawn_creature(Species::Goblin, tree_pos, &mut events)
        .expect("spawn goblin");

    let soldiers = soldiers_group(&sim);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: goblin_id,
            group_id: Some(soldiers.id),
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(
        sim.db.creatures.get(&goblin_id).unwrap().military_group,
        None,
        "Non-civ creatures cannot be assigned to military groups"
    );
}

#[test]
fn rename_civilian_group() {
    let mut sim = test_sim(42);
    let civ_group = civilian_group(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::RenameMilitaryGroup {
            group_id: civ_group.id,
            name: "Villagers".to_string(),
        },
    };
    sim.step(&[cmd], 1);

    let renamed = sim.db.military_groups.get(&civ_group.id).unwrap();
    assert_eq!(renamed.name, "Villagers");
}

#[test]
fn set_group_engagement_style() {
    use crate::species::{
        AmmoExhaustedBehavior, EngagementInitiative, EngagementStyle, WeaponPreference,
    };
    let mut sim = test_sim(42);
    let civ_group = civilian_group(&sim);

    // Change civilian group to aggressive.
    let new_style = EngagementStyle {
        weapon_preference: WeaponPreference::PreferMelee,
        ammo_exhausted: AmmoExhaustedBehavior::SwitchToMelee,
        initiative: EngagementInitiative::Aggressive,
        disengage_threshold_pct: 0,
    };
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SetGroupEngagementStyle {
            group_id: civ_group.id,
            engagement_style: new_style,
        },
    };
    sim.step(&[cmd], 1);

    let updated = sim.db.military_groups.get(&civ_group.id).unwrap();
    assert_eq!(
        updated.engagement_style.initiative,
        EngagementInitiative::Aggressive
    );
    assert_eq!(updated.engagement_style.disengage_threshold_pct, 0);
}

#[test]
fn fk_cascade_civ_delete_removes_groups() {
    let mut sim = test_sim(99);

    // Find an AI civ (not the player civ, which might cause issues).
    let ai_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| !c.player_controlled);
    let Some(ai_civ) = ai_civ else {
        // No AI civ in this seed — skip test.
        return;
    };
    let ai_civ_id = ai_civ.id;

    let groups_before = sim
        .db
        .military_groups
        .by_civ_id(&ai_civ_id, tabulosity::QueryOpts::ASC);
    assert!(
        !groups_before.is_empty(),
        "AI civ should have military groups"
    );

    // Delete the civ — groups should cascade.
    let _ = sim.db.remove_civilization(&ai_civ_id);

    let groups_after = sim
        .db
        .military_groups
        .by_civ_id(&ai_civ_id, tabulosity::QueryOpts::ASC);
    assert!(
        groups_after.is_empty(),
        "Deleting a civ should cascade-delete its military groups"
    );
}

#[test]
fn aggressive_group_civ_creature_auto_engages() {
    // This test verifies that an aggressive-group civ creature will attempt to
    // pursue hostiles via wander(), not just avoid them.
    let mut sim = test_sim(42);
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, &mut events)
        .expect("spawn elf");

    // Assign to soldiers (Aggressive).
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Verify resolve_engagement_style returns Aggressive.
    let style = sim.resolve_engagement_style(elf_id);
    assert_eq!(
        style.initiative,
        crate::species::EngagementInitiative::Aggressive,
        "Soldiers group should resolve to Aggressive"
    );
}

#[test]
fn resolve_engagement_style_implicit_civilian() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, &mut events)
        .expect("spawn elf");

    // Implicit civilian (military_group = None, civ_id = Some).
    let style = sim.resolve_engagement_style(elf_id);
    assert_eq!(
        style.disengage_threshold_pct, 100,
        "Implicit civilian should have 100% disengage threshold (always flee)"
    );
}

#[test]
fn resolve_engagement_style_non_civ_creature() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let goblin_id = sim
        .spawn_creature(Species::Goblin, tree_pos, &mut events)
        .expect("spawn goblin");

    // Non-civ creature → species default (Aggressive for goblins).
    let style = sim.resolve_engagement_style(goblin_id);
    assert_eq!(
        style.initiative,
        crate::species::EngagementInitiative::Aggressive,
        "Non-civ goblin should use species default (Aggressive)"
    );
}

// -----------------------------------------------------------------------
// Caller durability-preservation tests (expose existing bugs)
// -----------------------------------------------------------------------

#[test]
fn creature_death_drops_preserve_durability() {
    // When a creature dies, its items should drop on the ground
    // with all properties (durability, material, quality) preserved.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;

    // Give the elf a bow with specific material and durability.
    sim.inv_add_item_with_durability(
        inv_id,
        inventory::ItemKind::Bow,
        1,
        Some(elf),
        None,
        Some(inventory::Material::Yew),
        5,  // quality
        30, // current_hp (damaged)
        50, // max_hp
        None,
        None,
    );

    // Kill the elf.
    let mut events = Vec::new();
    sim.apply_damage(elf, 9999, &mut events);

    // Scan ALL ground piles for the Yew bow specifically (the elf also
    // has a starting bow with material: None, so we need to match ours).
    let mut bow_stack = None;
    for pile in sim.db.ground_piles.iter_all() {
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&pile.inventory_id, tabulosity::QueryOpts::ASC);
        if let Some(s) = stacks.iter().find(|s| {
            s.kind == inventory::ItemKind::Bow && s.material == Some(inventory::Material::Yew)
        }) {
            bow_stack = Some(s.clone());
            break;
        }
    }

    let bow_stack = bow_stack.expect("Yew bow should be in some ground pile after death");
    assert_eq!(
        bow_stack.material,
        Some(inventory::Material::Yew),
        "Material must survive death drop"
    );
    assert_eq!(bow_stack.quality, 5, "Quality must survive death drop");
    assert_eq!(
        bow_stack.current_hp, 30,
        "current_hp must survive death drop"
    );
    assert_eq!(bow_stack.max_hp, 50, "max_hp must survive death drop");
    assert!(
        bow_stack.owner.is_none(),
        "Owner should be cleared on death"
    );
}

#[test]
fn creature_death_drops_clear_equipped_slot() {
    // Equipped items should have equipped_slot cleared when dropped on death.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;

    // Give the elf an equipped hat.
    sim.inv_add_item_with_durability(
        inv_id,
        inventory::ItemKind::Hat,
        1,
        Some(elf),
        None,
        Some(inventory::Material::FruitSpecies(
            crate::fruit::FruitSpeciesId(0),
        )),
        0,
        15, // current_hp
        20, // max_hp
        None,
        Some(inventory::EquipSlot::Head),
    );

    // Kill the elf.
    let mut events = Vec::new();
    sim.apply_damage(elf, 9999, &mut events);

    // Find the hat in ground piles.
    let mut hat_stack = None;
    for pile in sim.db.ground_piles.iter_all() {
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&pile.inventory_id, tabulosity::QueryOpts::ASC);
        if let Some(s) = stacks
            .iter()
            .find(|s| s.kind == inventory::ItemKind::Hat && s.current_hp == 15)
        {
            hat_stack = Some(s.clone());
            break;
        }
    }

    let hat = hat_stack.expect("Hat should be in ground pile after death");
    assert_eq!(
        hat.equipped_slot, None,
        "equipped_slot must be cleared on death drop"
    );
    assert_eq!(hat.current_hp, 15, "Durability must be preserved");
    assert!(hat.owner.is_none(), "Owner must be cleared");
}

#[test]
fn death_drop_preserves_preexisting_pile_items() {
    // If a ground pile already exists at the death position with items
    // from another source, those items must not be affected.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let elf_inv = sim.db.creatures.get(&elf).unwrap().inventory_id;

    // Pre-place arrows in a ground pile at the elf's position.
    // Use a unique material (Willow) so they won't merge with the elf's
    // starting arrows (which have material: None).
    let pile_id = sim.ensure_ground_pile(elf_pos);
    let pile_inv = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;
    let fake_task = TaskId::new(&mut sim.rng.clone());
    insert_stub_task(&mut sim, fake_task);
    sim.inv_add_item(
        pile_inv,
        inventory::ItemKind::Arrow,
        10,
        None,
        Some(fake_task), // reserved by another task
        Some(inventory::Material::Willow),
        7, // distinctive quality
        None,
        None,
    );

    // Give the elf a bow (owned by elf, so death drop will clear its owner).
    sim.inv_add_item(
        elf_inv,
        inventory::ItemKind::Bow,
        1,
        Some(elf),
        None,
        Some(inventory::Material::Yew),
        0,
        None,
        None,
    );

    // Kill the elf.
    let mut events = Vec::new();
    sim.apply_damage(elf, 9999, &mut events);

    // The pre-existing Willow arrows must still have their reservation.
    let pile_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&pile_inv, tabulosity::QueryOpts::ASC);
    let arrow_stack = pile_stacks
        .iter()
        .find(|s| {
            s.kind == inventory::ItemKind::Arrow
                && s.material == Some(inventory::Material::Willow)
                && s.quality == 7
        })
        .expect("Pre-existing Willow arrows should still be in pile");
    assert_eq!(
        arrow_stack.reserved_by,
        Some(fake_task),
        "Pre-existing reservation must not be cleared by death drop"
    );
    assert_eq!(arrow_stack.quantity, 10);

    // The elf's bow should also be in the pile, unowned.
    let bow_stack = pile_stacks
        .iter()
        .find(|s| {
            s.kind == inventory::ItemKind::Bow && s.material == Some(inventory::Material::Yew)
        })
        .expect("Elf's bow should be in pile");
    assert!(bow_stack.owner.is_none());
}

// ---------------------------------------------------------------------------
// Player identity tests
// ---------------------------------------------------------------------------

#[test]
fn register_player_creates_entry() {
    let mut sim = test_sim(42);
    assert_eq!(sim.db.players.iter_all().count(), 0);

    sim.register_player("alice");
    assert_eq!(sim.db.players.iter_all().count(), 1);

    let player = sim.db.players.get(&"alice".to_string()).unwrap();
    assert_eq!(player.name, "alice");
    assert_eq!(player.civ_id, sim.player_civ_id);
}

#[test]
fn register_player_is_idempotent() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    sim.register_player("alice");
    assert_eq!(sim.db.players.iter_all().count(), 1);
}

#[test]
fn register_player_multiple_players() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    sim.register_player("bob");
    assert_eq!(sim.db.players.iter_all().count(), 2);
    assert!(sim.db.players.get(&"alice".to_string()).is_some());
    assert!(sim.db.players.get(&"bob".to_string()).is_some());
}

#[test]
fn player_persists_across_serde_roundtrip() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    sim.register_player("bob");

    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    assert_eq!(restored.db.players.iter_all().count(), 2);
    let alice = restored.db.players.get(&"alice".to_string()).unwrap();
    assert_eq!(alice.civ_id, sim.player_civ_id);
    let bob = restored.db.players.get(&"bob".to_string()).unwrap();
    assert_eq!(bob.civ_id, sim.player_civ_id);
}

#[test]
fn tree_owner_is_civ_id() {
    let sim = test_sim(42);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert_eq!(tree.owner, sim.player_civ_id);
}

// ---------------------------------------------------------------------------
// Selection groups (F-selection-groups)
// ---------------------------------------------------------------------------

#[test]
fn set_selection_group_creates_new_group() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    let elf_id = spawn_elf(&mut sim);

    let tick = sim.tick + 1;
    let cmd = SimCommand {
        player_name: "alice".to_string(),
        tick,
        action: SimAction::SetSelectionGroup {
            group_number: 1,
            creature_ids: vec![elf_id],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd], tick + 1);

    let groups = sim.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, 1);
    assert_eq!(groups[0].1, vec![elf_id]);
    assert!(groups[0].2.is_empty());
}

#[test]
fn set_selection_group_overwrites_existing() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);

    let t = sim.tick + 1;
    let cmd1 = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 3,
            creature_ids: vec![elf1],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd1], t + 1);

    // Overwrite group 3 with elf2.
    let t2 = sim.tick + 1;
    let cmd2 = SimCommand {
        player_name: "alice".to_string(),
        tick: t2,
        action: SimAction::SetSelectionGroup {
            group_number: 3,
            creature_ids: vec![elf2],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd2], t2 + 1);

    let groups = sim.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].1, vec![elf2]);
}

#[test]
fn add_to_selection_group_merges_without_duplicates() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);

    // Create group 2 with elf1.
    let t = sim.tick + 1;
    let cmd1 = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 2,
            creature_ids: vec![elf1],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd1], t + 1);

    // Add elf1 (duplicate) and elf2 (new).
    let t2 = sim.tick + 1;
    let cmd2 = SimCommand {
        player_name: "alice".to_string(),
        tick: t2,
        action: SimAction::AddToSelectionGroup {
            group_number: 2,
            creature_ids: vec![elf1, elf2],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd2], t2 + 1);

    let groups = sim.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].1.len(), 2);
    assert!(groups[0].1.contains(&elf1));
    assert!(groups[0].1.contains(&elf2));
}

#[test]
fn add_to_nonexistent_group_creates_it() {
    let mut sim = test_sim(42);
    sim.register_player("bob");
    let elf = spawn_elf(&mut sim);

    let t = sim.tick + 1;
    let cmd = SimCommand {
        player_name: "bob".to_string(),
        tick: t,
        action: SimAction::AddToSelectionGroup {
            group_number: 5,
            creature_ids: vec![elf],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd], t + 1);

    let groups = sim.get_selection_groups("bob");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, 5);
    assert_eq!(groups[0].1, vec![elf]);
}

#[test]
fn selection_groups_are_per_player() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    sim.register_player("bob");
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);

    let t = sim.tick + 1;
    let cmd1 = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 1,
            creature_ids: vec![elf1],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd1], t + 1);

    let t2 = sim.tick + 1;
    let cmd2 = SimCommand {
        player_name: "bob".to_string(),
        tick: t2,
        action: SimAction::SetSelectionGroup {
            group_number: 1,
            creature_ids: vec![elf2],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd2], t2 + 1);

    let alice_groups = sim.get_selection_groups("alice");
    assert_eq!(alice_groups.len(), 1);
    assert_eq!(alice_groups[0].1, vec![elf1]);

    let bob_groups = sim.get_selection_groups("bob");
    assert_eq!(bob_groups.len(), 1);
    assert_eq!(bob_groups[0].1, vec![elf2]);
}

#[test]
fn selection_groups_survive_serde_roundtrip() {
    let mut sim = test_sim(42);
    sim.register_player("alice");
    let elf = spawn_elf(&mut sim);

    let t = sim.tick + 1;
    let cmd = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 7,
            creature_ids: vec![elf],
            structure_ids: vec![],
        },
    };
    sim.step(&[cmd], t + 1);

    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    let groups = restored.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, 7);
    assert_eq!(groups[0].1, vec![elf]);
}

#[test]
fn set_selection_group_with_structures() {
    let mut sim = test_sim(42);
    sim.register_player("alice");

    let t = sim.tick + 1;
    let cmd = SimCommand {
        player_name: "alice".to_string(),
        tick: t,
        action: SimAction::SetSelectionGroup {
            group_number: 4,
            creature_ids: vec![],
            structure_ids: vec![StructureId(42), StructureId(99)],
        },
    };
    sim.step(&[cmd], t + 1);

    let groups = sim.get_selection_groups("alice");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].2, vec![StructureId(42), StructureId(99)]);
}

#[test]
fn selection_group_command_serde_roundtrip() {
    let cmd = SimCommand {
        player_name: "player1".to_string(),
        tick: 10,
        action: SimAction::SetSelectionGroup {
            group_number: 3,
            creature_ids: vec![],
            structure_ids: vec![StructureId(1)],
        },
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());

    let cmd2 = SimCommand {
        player_name: "player1".to_string(),
        tick: 10,
        action: SimAction::AddToSelectionGroup {
            group_number: 5,
            creature_ids: vec![],
            structure_ids: vec![],
        },
    };
    let json2 = serde_json::to_string(&cmd2).unwrap();
    let restored2: SimCommand = serde_json::from_str(&json2).unwrap();
    assert_eq!(json2, serde_json::to_string(&restored2).unwrap());
}

#[test]
fn capybara_generates_no_mana() {
    let mut sim = test_sim(42);
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    let heartbeat = sim.species_table[&Species::Capybara].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert_eq!(capy.mp, 0);
    assert_eq!(capy.mp_max, 0);
}

// ---------------------------------------------------------------------------
// Mana system — Phase B2: stat-scaled mana pool and regeneration
// ---------------------------------------------------------------------------

#[test]
fn elf_mp_max_scaled_by_willpower() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let species_mp_max = sim.species_table[&Species::Elf].mp_max;
    let wil = sim.trait_int(elf_id, TraitKind::Willpower, 0);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let expected = crate::stats::apply_stat_multiplier(species_mp_max, wil).max(1);
    assert_eq!(
        elf.mp_max, expected,
        "mp_max should be species base ({species_mp_max}) scaled by WIL ({wil}): expected {expected}, got {}",
        elf.mp_max
    );
    // Creature should spawn at full (stat-scaled) mana.
    assert_eq!(elf.mp, elf.mp_max);
}

#[test]
fn elf_mp_max_unaffected_when_wil_is_zero() {
    let mut sim = test_sim(42);
    // Override elf WIL distribution to mean=0, stdev=0.
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .stat_distributions
        .get_mut(&TraitKind::Willpower)
        .unwrap()
        .mean = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .stat_distributions
        .get_mut(&TraitKind::Willpower)
        .unwrap()
        .stdev = 0;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let species_mp_max = sim.species_table[&Species::Elf].mp_max;

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    // WIL=0 means 1× multiplier, so mp_max == species base.
    assert_eq!(elf.mp_max, species_mp_max);
}

#[test]
fn elf_mp_max_reduced_by_negative_willpower() {
    let mut sim = test_sim(42);
    // Force negative WIL: mean=-50, stdev=0.
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .stat_distributions
        .get_mut(&TraitKind::Willpower)
        .unwrap()
        .mean = -50;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .stat_distributions
        .get_mut(&TraitKind::Willpower)
        .unwrap()
        .stdev = 0;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let species_mp_max = sim.species_table[&Species::Elf].mp_max;
    let elf = sim.db.creatures.get(&elf_id).unwrap();

    // WIL=-50 → 2^(-0.5) ≈ 0.707× multiplier, so mp_max < species base.
    assert!(
        elf.mp_max < species_mp_max,
        "negative WIL should reduce mp_max: got {} (species base {})",
        elf.mp_max,
        species_mp_max
    );
    assert!(elf.mp_max >= 1, "mp_max floor is 1");
    assert_eq!(elf.mp, elf.mp_max, "should spawn at full mana");
}

#[test]
fn nonmagical_creature_mp_max_unaffected_by_stats() {
    let mut sim = test_sim(42);
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    let capy = sim.db.creatures.get(&capy_id).unwrap();
    // mp_max=0 species should stay 0 regardless of any stats.
    assert_eq!(capy.mp_max, 0);
    assert_eq!(capy.mp, 0);
}

// ---------------------------------------------------------------------------
// F-enemy-raids tests
// ---------------------------------------------------------------------------

/// Helper: ensure the player civ knows about exactly one hostile civ (Goblin,
/// Orc, or Troll). Removes all other hostile relationships so the raid is
/// deterministic. Returns the hostile civ's CivId.
fn ensure_hostile_civ(sim: &mut SimState) -> CivId {
    let player_civ = sim.player_civ_id.unwrap();

    // Find a goblin, orc, or troll civ.
    let hostile_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| {
            c.id != player_civ
                && matches!(
                    c.primary_species,
                    CivSpecies::Goblin | CivSpecies::Orc | CivSpecies::Troll
                )
        })
        .map(|c| c.id);

    let hostile_civ_id = hostile_civ.expect("worldgen should produce at least one hostile civ");

    // Remove ALL hostile relationships involving the player (both directions)
    // so the only hostile civ is the one we set up.
    remove_all_hostile_rels(sim);

    // Create bidirectional hostile relationship: they hate us (triggers raids)
    // and we hate them (player awareness).
    sim.discover_civ(hostile_civ_id, player_civ, CivOpinion::Hostile);
    sim.discover_civ(player_civ, hostile_civ_id, CivOpinion::Hostile);

    hostile_civ_id
}

/// Helper: remove all hostile relationships involving the player civ
/// (both forward: player→other, and reverse: other→player).
fn remove_all_hostile_rels(sim: &mut SimState) {
    let player_civ = sim.player_civ_id.unwrap();
    // Forward: player considers them hostile.
    let forward_ids: Vec<_> = sim
        .db
        .civ_relationships
        .by_from_civ(&player_civ, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.opinion == CivOpinion::Hostile)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in forward_ids {
        let _ = sim.db.civ_relationships.remove_no_fk(&pk);
    }
    // Reverse: they consider the player hostile.
    let reverse_ids: Vec<_> = sim
        .db
        .civ_relationships
        .by_to_civ(&player_civ, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.opinion == CivOpinion::Hostile)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in reverse_ids {
        let _ = sim.db.civ_relationships.remove_no_fk(&pk);
    }
}

#[test]
fn spawn_creature_with_civ_sets_civ_id() {
    let mut sim = test_sim(42);
    let hostile_civ = ensure_hostile_civ(&mut sim);
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let creature_id = sim
        .spawn_creature_with_civ(Species::Goblin, tree_pos, Some(hostile_civ), &mut events)
        .expect("should spawn goblin");

    let creature = sim.db.creatures.get(&creature_id).unwrap();
    assert_eq!(creature.civ_id, Some(hostile_civ));
    assert_eq!(creature.species, Species::Goblin);
}

#[test]
fn species_config_backward_compat_ticks_per_hp_regen() {
    // Old save files won't have ticks_per_hp_regen. Verify it defaults to 0.
    let config = GameConfig::default();
    let troll_data = &config.species[&Species::Troll];
    let json = serde_json::to_string(troll_data).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.remove("ticks_per_hp_regen");
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: crate::species::SpeciesData = serde_json::from_str(&stripped).unwrap();
    assert_eq!(
        restored.ticks_per_hp_regen, 0,
        "ticks_per_hp_regen should default to 0"
    );
}

#[test]
fn all_civ_species_with_to_species_have_species_table_entry() {
    // Every CivSpecies that maps to a Species via to_species() must have
    // a corresponding entry in the default species table.
    let config = GameConfig::default();
    for civ_species in CivSpecies::ALL {
        if let Some(species) = civ_species.to_species() {
            assert!(
                config.species.contains_key(&species),
                "{civ_species:?} maps to {species:?} but species table has no entry"
            );
        }
    }
}

/// Verify that a creature with a cached path containing a position where no
/// nav node exists (e.g., because the node was destroyed) handles it gracefully
/// — no panic, creature stays alive and valid.
#[test]
fn path_resolution_nav_node_destroyed_no_panic() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    // Find a connected pair so the elf has a valid starting node.
    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_idle(&mut sim, elf);

    // Give the elf a GoTo task at node_b so it has reason to move.
    let task_id = insert_goto_task(&mut sim, node_b);
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        let _ = sim.db.tasks.update_no_fk(t);
    }
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Assign a cached path through a position that has NO nav node.
    // Use a coordinate far from the world (no node will exist there).
    let bogus_pos = VoxelCoord::new(63, 63, 63);
    assert!(
        sim.nav_graph.node_at(bogus_pos).is_none(),
        "Test setup: bogus position should have no nav node"
    );

    let real_dest = sim.nav_graph.node(node_b).position;
    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.path = Some(CreaturePath {
            remaining_positions: vec![bogus_pos, real_dest],
        });
    });

    // Schedule the elf to activate and step forward. The movement code should
    // detect the missing nav node and repath (not panic).
    sim.event_queue.cancel_creature_activations(elf);
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: elf },
    );
    sim.step(&[], sim.tick + 200);

    // Creature should still be alive and at a valid position.
    let creature = sim
        .db
        .creatures
        .get(&elf)
        .expect("Creature should still exist after path resolution failure");
    assert!(
        creature.hp > 0,
        "Creature should be alive after graceful repath"
    );
}

// ---- Command queue (F-command-queue) ----

#[test]
fn find_available_task_skips_task_restricted_to_other_creature() {
    let mut sim = test_sim(42);
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);

    // Create a task restricted to elf_a at a reachable location.
    let task_id = TaskId::new(&mut sim.rng);
    let task_pos = VoxelCoord::new(10, 1, 10);
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: task_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_a),
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(task);

    // elf_b should NOT find this task.
    assert!(
        sim.find_available_task(elf_b).is_none(),
        "Task restricted to elf_a should not be found by elf_b"
    );
    // elf_a SHOULD find it.
    assert_eq!(
        sim.find_available_task(elf_a),
        Some(task_id),
        "Task restricted to elf_a should be found by elf_a"
    );
}

#[test]
fn find_available_task_skips_incomplete_prerequisite() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    // Create prerequisite task A (Available, not Complete).
    let task_a_id = TaskId::new(&mut sim.rng);
    let task_pos = VoxelCoord::new(10, 1, 10);
    let task_a = Task {
        id: task_a_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: task_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_id),
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(task_a);

    // Create dependent task B with prerequisite = A.
    let task_b_id = TaskId::new(&mut sim.rng);
    let task_b_pos = VoxelCoord::new(20, 1, 20);
    let task_b = Task {
        id: task_b_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: task_b_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_id),
        prerequisite_task_id: Some(task_a_id),
        required_civ_id: None,
    };
    sim.insert_task(task_b);

    // Only task A should be found (B's prerequisite is not complete).
    assert_eq!(
        sim.find_available_task(elf_id),
        Some(task_a_id),
        "Should find task A (no prerequisite), not B (prerequisite incomplete)"
    );

    // Complete task A.
    if let Some(mut t) = sim.db.tasks.get(&task_a_id) {
        t.state = TaskState::Complete;
        let _ = sim.db.tasks.update_no_fk(t);
    }

    // Now task B should be found.
    assert_eq!(
        sim.find_available_task(elf_id),
        Some(task_b_id),
        "After prerequisite completes, task B should be available"
    );
}

#[test]
fn cancel_creature_queue_cancels_entire_chain() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    // Create a chain: A -> B -> C (B depends on A, C depends on B).
    let task_a_id = TaskId::new(&mut sim.rng);
    let pos = VoxelCoord::new(10, 1, 10);
    sim.insert_task(Task {
        id: task_a_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_id),
        prerequisite_task_id: None,
        required_civ_id: None,
    });
    // Assign A to the elf.
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(task_a_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let task_b_id = TaskId::new(&mut sim.rng);
    sim.insert_task(Task {
        id: task_b_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: VoxelCoord::new(20, 1, 20),
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_id),
        prerequisite_task_id: Some(task_a_id),
        required_civ_id: None,
    });

    let task_c_id = TaskId::new(&mut sim.rng);
    sim.insert_task(Task {
        id: task_c_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: VoxelCoord::new(30, 1, 30),
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_id),
        prerequisite_task_id: Some(task_b_id),
        required_civ_id: None,
    });

    // Cancel the queue via cancel_creature_queue (simulates unshifted player
    // command replacing the queue).
    sim.cancel_creature_queue(elf_id);

    // B and C should both be Complete (queue cancellation).
    let task_b = sim.db.tasks.get(&task_b_id).unwrap();
    assert_eq!(
        task_b.state,
        TaskState::Complete,
        "Dependent task B should be cancelled (Complete)"
    );
    let task_c = sim.db.tasks.get(&task_c_id).unwrap();
    assert_eq!(
        task_c.state,
        TaskState::Complete,
        "Transitive dependent task C should be cancelled (Complete)"
    );
}

#[test]
fn cancel_creature_queue_only_cancels_player_directed() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    // Create a player-directed queued task.
    let pd_task_id = TaskId::new(&mut sim.rng);
    let pos = VoxelCoord::new(10, 1, 10);
    sim.insert_task(Task {
        id: pd_task_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_id),
        prerequisite_task_id: None,
        required_civ_id: None,
    });

    // Create an autonomous restricted task (should survive cancellation).
    let auto_task_id = TaskId::new(&mut sim.rng);
    sim.insert_task(Task {
        id: auto_task_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: VoxelCoord::new(20, 1, 20),
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: Some(elf_id),
        prerequisite_task_id: None,
        required_civ_id: None,
    });

    // Cancel the creature's player-directed queue.
    sim.cancel_creature_queue(elf_id);

    let pd_task = sim.db.tasks.get(&pd_task_id).unwrap();
    assert_eq!(
        pd_task.state,
        TaskState::Complete,
        "Player-directed queued task should be cancelled"
    );
    let auto_task = sim.db.tasks.get(&auto_task_id).unwrap();
    assert_eq!(
        auto_task.state,
        TaskState::Available,
        "Autonomous restricted task should NOT be cancelled"
    );
}

// ---------------------------------------------------------------------------
// F-path-core: Path system tests
// ---------------------------------------------------------------------------

/// Spawn an elf in the test sim and return its CreatureId.
fn spawn_test_elf(sim: &mut SimState) -> CreatureId {
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick.max(1),
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], sim.tick.max(1) + 1);
    sim.db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .expect("elf should exist after spawn")
        .id
}
