//! Tests for the logistics system: haul tasks, wants/gap/surplus calculation,
//! reservations, ownership tracking, personal and military acquisition,
//! auto-logistics from recipes, and haul lifecycle (pickup/dropoff/cleanup).
//! Corresponds to `sim/logistics.rs`.

use super::*;

#[test]
fn logistics_heartbeat_creates_haul_tasks() {
    let mut sim = test_sim(legacy_test_seed());

    // Place a ground pile with bread.
    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            10,
            None,
            None,
        );
    }

    // Create a building that wants bread.
    let building_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    let _sid = insert_building(
        &mut sim,
        building_anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );

    // Run logistics heartbeat manually.
    sim.process_logistics_heartbeat();

    // Should have created a haul task.
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(haul_tasks.len(), 1, "Expected 1 haul task");

    let haul = sim
        .task_haul_data(haul_tasks[0].id)
        .expect("Haul task should have haul data");
    assert_eq!(haul.item_kind, crate::inventory::ItemKind::Bread);
    assert_eq!(haul.quantity, 5);
    assert_eq!(haul.source_kind, crate::db::HaulSourceKind::Pile);
    assert_eq!(haul.phase, task::HaulPhase::GoingToSource);

    // Ground pile items should be reserved.
    let pile = sim
        .db
        .ground_piles
        .by_position(&pile_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    let unreserved = sim.inv_unreserved_item_count(
        pile.inventory_id,
        crate::inventory::ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert_eq!(unreserved, 5, "5 items should remain unreserved");
}

#[test]
fn logistics_respects_priority() {
    let mut sim = test_sim(legacy_test_seed());

    // Place bread on the ground.
    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            3,
            None,
            None,
        );
    }

    // High-priority building wants 2 bread.
    let high_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    insert_building(
        &mut sim,
        high_anchor,
        Some(10),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 2,
        }],
    );

    // Low-priority building wants 2 bread.
    let low_anchor = VoxelCoord::new(pile_pos.x + 6, pile_pos.y, pile_pos.z);
    insert_building(
        &mut sim,
        low_anchor,
        Some(1),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 2,
        }],
    );

    sim.process_logistics_heartbeat();

    // Should create 2 haul tasks: one for high-priority (2 bread), one for
    // low-priority (1 remaining bread).
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(haul_tasks.len(), 2, "Expected 2 haul tasks");

    // All bread should be reserved.
    let pile = sim
        .db
        .ground_piles
        .by_position(&pile_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    let unreserved = sim.inv_unreserved_item_count(
        pile.inventory_id,
        crate::inventory::ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert_eq!(unreserved, 0, "All bread should be reserved");
}

#[test]
fn logistics_skips_reserved_items() {
    let mut sim = test_sim(legacy_test_seed());

    // Place bread on the ground, some already reserved.
    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let task_id = TaskId::new(&mut sim.rng);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            5,
            None,
            None,
        );
        sim.inv_reserve_items(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            crate::inventory::MaterialFilter::Any,
            3,
            task_id,
        );
    }

    // Building wants 5 bread.
    let anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    insert_building(
        &mut sim,
        anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );

    sim.process_logistics_heartbeat();

    // Should only create a task for 2 unreserved bread, not all 5.
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(haul_tasks.len(), 1);
    let haul = sim
        .task_haul_data(haul_tasks[0].id)
        .expect("Haul task should have haul data");
    assert_eq!(haul.quantity, 2, "Only 2 unreserved bread available");
}

#[test]
fn logistics_counts_in_transit() {
    let mut sim = test_sim(legacy_test_seed());

    // Place 10 bread on ground.
    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            10,
            None,
            None,
        );
    }

    let anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    let sid = insert_building(
        &mut sim,
        anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 8,
        }],
    );

    // Manually create an in-transit haul task for 5 bread.
    let fake_task_id = TaskId::new(&mut sim.rng);
    let existing_haul = Task {
        id: fake_task_id,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
            source: task::HaulSource::GroundPile(pile_pos),
            destination: sid,
            phase: task::HaulPhase::GoingToSource,
            destination_coord: VoxelCoord::new(0, 0, 0),
        },
        state: TaskState::InProgress,
        location: VoxelCoord::new(0, 0, 0),
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(existing_haul);

    sim.process_logistics_heartbeat();

    // In-transit counts as 5, target is 8, so need 3 more.
    let new_haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.id != fake_task_id && t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(new_haul_tasks.len(), 1, "Expected 1 new haul task");
    let haul = sim
        .task_haul_data(new_haul_tasks[0].id)
        .expect("Haul task should have haul data");
    assert_eq!(haul.quantity, 3);
}

#[test]
fn logistics_pulls_from_lower_priority_building() {
    let mut sim = test_sim(legacy_test_seed());

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Building A (priority 3) has bread.
    let anchor_a = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let sid_a = insert_building(&mut sim, anchor_a, Some(3), Vec::new());
    sim.inv_add_simple_item(
        sim.structure_inv(sid_a),
        crate::inventory::ItemKind::Bread,
        5,
        None,
        None,
    );

    // Building B (priority 5) wants bread.
    let anchor_b = VoxelCoord::new(tree_pos.x + 6, tree_pos.y, tree_pos.z);
    insert_building(
        &mut sim,
        anchor_b,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 3,
        }],
    );

    sim.process_logistics_heartbeat();

    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(haul_tasks.len(), 1);
    let haul = sim
        .task_haul_data(haul_tasks[0].id)
        .expect("Haul task should have haul data");
    assert_eq!(haul.source_kind, crate::db::HaulSourceKind::Building);
    let source_sid = sim
        .task_structure_ref(
            haul_tasks[0].id,
            crate::db::TaskStructureRole::HaulSourceBuilding,
        )
        .expect("Should have source building ref");
    assert_eq!(source_sid, sid_a, "Should pull from building A");
    assert_eq!(haul.quantity, 3);
}

#[test]
fn logistics_surplus_source_from_higher_priority_building() {
    let mut sim = test_sim(legacy_test_seed());

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Kitchen (priority 8) has 10 bread, wants 0 bread → 10 surplus.
    let anchor_k = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let sid_k = insert_building(&mut sim, anchor_k, Some(8), Vec::new());
    sim.inv_add_simple_item(
        sim.structure_inv(sid_k),
        crate::inventory::ItemKind::Bread,
        10,
        None,
        None,
    );

    // Storehouse (priority 2) wants 5 bread.
    let anchor_s = VoxelCoord::new(tree_pos.x + 7, tree_pos.y, tree_pos.z);
    insert_building(
        &mut sim,
        anchor_s,
        Some(2),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );

    sim.process_logistics_heartbeat();

    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(
        haul_tasks.len(),
        1,
        "Should create a haul task for surplus bread"
    );
    let haul = sim
        .task_haul_data(haul_tasks[0].id)
        .expect("Haul task should have haul data");
    assert_eq!(haul.source_kind, crate::db::HaulSourceKind::Building);
    let source_sid = sim
        .task_structure_ref(
            haul_tasks[0].id,
            crate::db::TaskStructureRole::HaulSourceBuilding,
        )
        .expect("Should have source building ref");
    assert_eq!(
        source_sid, sid_k,
        "Should pull from the kitchen (surplus source)"
    );
    assert_eq!(haul.quantity, 5);
}

#[test]
fn logistics_caps_tasks_per_heartbeat() {
    let mut sim = test_sim(legacy_test_seed());
    // Override max tasks to 2.
    sim.config.max_haul_tasks_per_heartbeat = 2;

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    {
        let pile_id = sim.ensure_ground_pile(tree_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            100,
            None,
            None,
        );
    }

    // Create 5 buildings that each want 10 bread.
    for i in 0..5 {
        let anchor = VoxelCoord::new(tree_pos.x + 3 * (i + 1), tree_pos.y, tree_pos.z);
        insert_building(
            &mut sim,
            anchor,
            Some(5),
            vec![crate::building::LogisticsWant {
                item_kind: crate::inventory::ItemKind::Bread,
                material_filter: crate::inventory::MaterialFilter::Any,
                target_quantity: 10,
            }],
        );
    }

    sim.process_logistics_heartbeat();

    let haul_count = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .count();
    assert_eq!(haul_count, 2, "Should be capped at 2 tasks per heartbeat");
}

#[test]
fn harvest_task_creates_ground_pile() {
    let mut sim = test_sim(legacy_test_seed());
    let fruit_pos = ensure_tree_has_fruit(&mut sim);

    // Spawn an elf near the fruit.
    let elf_id = spawn_elf(&mut sim);

    // Find the walkable position nearest to the fruit.
    let elf_pos = find_walkable(&sim, fruit_pos, 10).unwrap();
    {
        let mut elf = sim.db.creatures.get(&elf_id).unwrap();
        elf.position = VoxelBox::point(elf_pos);
        sim.db.update_creature(elf).unwrap();
    }

    // Create a Harvest task at the fruit nav node.
    let task_id = TaskId::new(&mut sim.rng);
    let harvest_task = Task {
        id: task_id,
        kind: TaskKind::Harvest { fruit_pos },
        state: TaskState::InProgress,
        location: elf_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(harvest_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Execute the task directly (resolve the harvest action).
    sim.resolve_harvest_action(elf_id, task_id, fruit_pos);

    // Assert: fruit voxel removed from world.
    assert_eq!(
        sim.world.get(fruit_pos),
        VoxelType::Air,
        "Fruit voxel should be removed"
    );

    // Assert: fruit removed from TreeFruit table.
    assert!(
        sim.db
            .tree_fruits
            .by_position(&VoxelBox::point(fruit_pos), tabulosity::QueryOpts::ASC)
            .is_empty(),
        "Fruit should be removed from TreeFruit table"
    );

    // Assert: ground pile created with 1 Fruit. The pile may have been
    // snapped down to the nearest surface if the elf was up on the tree.
    let pile = sim
        .db
        .ground_piles
        .iter_all()
        .find(|p| p.position.x == elf_pos.x && p.position.z == elf_pos.z)
        .expect("Ground pile should exist in elf's column");
    assert_eq!(
        sim.inv_item_count(
            pile.inventory_id,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Any
        ),
        1,
        "Ground pile should have 1 fruit"
    );

    // Assert: task completed.
    assert_eq!(
        sim.db.tasks.get(&task_id).unwrap().state,
        TaskState::Complete,
        "Harvest task should be complete"
    );
}

#[test]
fn logistics_heartbeat_creates_harvest_tasks() {
    let mut sim = test_sim(legacy_test_seed());
    ensure_tree_has_fruit(&mut sim);

    let fruit_count = sim
        .db
        .tree_fruits
        .count_by_tree_id(&sim.player_tree_id, tabulosity::QueryOpts::ASC);
    assert!(fruit_count > 0);

    // Ensure no ground piles with fruit exist.
    assert_eq!(sim.db.ground_piles.len(), 0);

    // Create a building that wants fruit (kitchen with logistics).
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let site = VoxelCoord::new(tree_pos.x + 3, 0, tree_pos.z);
    let kitchen_priority = sim.config.kitchen_default_priority;
    let sid = insert_building(
        &mut sim,
        site,
        Some(kitchen_priority),
        vec![building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );
    {
        let mut s = sim.db.structures.get(&sid).unwrap();
        s.furnishing = Some(FurnishingType::Kitchen);
        sim.db.update_structure(s).unwrap();
    }

    // Run logistics heartbeat.
    sim.process_logistics_heartbeat();

    // Assert: at least one Harvest task was created.
    let harvest_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Harvest)
        .collect();
    assert!(
        !harvest_tasks.is_empty(),
        "Logistics heartbeat should create Harvest tasks when buildings want fruit"
    );

    // Each harvest task should target a valid fruit position.
    for task in &harvest_tasks {
        let fruit_pos = sim
            .task_voxel_ref(task.id, crate::db::TaskVoxelRole::FruitTarget)
            .expect("Harvest task should have a FruitTarget voxel ref");
        assert_eq!(
            sim.world.get(fruit_pos),
            VoxelType::Fruit,
            "Harvest task should target an actual fruit voxel"
        );
        assert_eq!(task.state, TaskState::Available);
        assert_eq!(task.required_species, Some(Species::Elf));
        assert_eq!(task.origin, TaskOrigin::Automated);
    }
}

#[test]
fn haul_source_empty_cancels() {
    let mut sim = test_sim(legacy_test_seed());

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let sid = insert_building(&mut sim, anchor, Some(5), Vec::new());

    // Create a haul task with source pointing to a non-existent ground pile.
    let source_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    let task_id = TaskId::new(&mut sim.rng);
    let source_walkable = find_walkable(&sim, source_pos, 10).unwrap();

    let haul_task = Task {
        id: task_id,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
            source: task::HaulSource::GroundPile(source_pos),
            destination: sid,
            phase: task::HaulPhase::GoingToSource,
            destination_coord: anchor,
        },
        state: TaskState::InProgress,
        location: source_walkable,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_task);

    // Spawn an elf and manually assign it to the haul task at the source.
    let elf_id = spawn_elf(&mut sim);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }
    let source_pos = source_walkable;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(source_pos);
        sim.db.update_creature(c).unwrap();
    }
    {
        let mut t = sim.db.tasks.get(&task_id).unwrap();
        t.state = task::TaskState::InProgress;
        sim.db.update_task(t).unwrap();
    }

    // Execute the task — no ground pile exists, so pickup should find 0 items.
    sim.resolve_pickup_action(elf_id);

    // Task should be completed (cancelled due to empty source).
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(task.state, TaskState::Complete, "Task should be completed");
}

// -----------------------------------------------------------------------
// Logistics ownership tests: owned items must not be hauled or counted
// by building logistics wants.
// -----------------------------------------------------------------------

#[test]
fn logistics_ignores_owned_items_in_ground_pile() {
    // Owned bread in a ground pile must NOT be hauled to a building.
    let mut sim = test_sim(legacy_test_seed());

    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);

    // Place 5 owned bread (owned by elf) in a ground pile.
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            5,
            Some(elf_id),
            None,
        );
    }

    // Building wants 5 bread.
    let building_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    let _sid = insert_building(
        &mut sim,
        building_anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );

    sim.process_logistics_heartbeat();

    // No haul task should be created — all bread is owned.
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(
        haul_tasks.len(),
        0,
        "Owned items must not be hauled by logistics"
    );
}

#[test]
fn logistics_hauls_unowned_but_skips_owned_in_same_pile() {
    // A ground pile with 3 owned + 5 unowned bread: only the 5 unowned
    // should be hauled.
    let mut sim = test_sim(legacy_test_seed());

    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);

    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        // 3 owned bread.
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            3,
            Some(elf_id),
            None,
        );
        // 5 unowned bread.
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            5,
            None,
            None,
        );
    }

    // Building wants 10 bread.
    let building_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    let _sid = insert_building(
        &mut sim,
        building_anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 10,
        }],
    );

    sim.process_logistics_heartbeat();

    // Haul task should haul at most 5 (the unowned bread).
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(
        haul_tasks.len(),
        1,
        "Expected 1 haul task for unowned items"
    );
    let haul = sim.task_haul_data(haul_tasks[0].id).unwrap();
    assert_eq!(haul.quantity, 5, "Should only haul the 5 unowned bread");
}

#[test]
fn logistics_ignores_owned_items_in_lower_priority_building() {
    // Phase 2 source (lower-priority building): owned items must not be
    // pulled from it.
    let mut sim = test_sim(legacy_test_seed());

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);

    // Low-priority building with only owned bread.
    let low_anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let low_sid = insert_building(&mut sim, low_anchor, Some(1), Vec::new());
    let low_inv = sim.db.structures.get(&low_sid).unwrap().inventory_id;
    sim.inv_add_simple_item(
        low_inv,
        crate::inventory::ItemKind::Bread,
        5,
        Some(elf_id),
        None,
    );

    // High-priority building wants bread.
    let high_anchor = VoxelCoord::new(tree_pos.x + 6, tree_pos.y, tree_pos.z);
    let _high_sid = insert_building(
        &mut sim,
        high_anchor,
        Some(10),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );

    sim.process_logistics_heartbeat();

    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(
        haul_tasks.len(),
        0,
        "Owned items in lower-priority building must not be hauled"
    );
}

#[test]
fn logistics_surplus_ignores_owned_items() {
    // Phase 3 surplus source: a building with 10 bread (5 owned + 5 unowned)
    // and want target of 5 should have surplus of 0 (only 5 unowned, all
    // needed to meet the want).
    let mut sim = test_sim(legacy_test_seed());

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);

    // Source building: priority 5, wants 5 bread, has 5 owned + 5 unowned.
    let src_anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let src_sid = insert_building(
        &mut sim,
        src_anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );
    let src_inv = sim.db.structures.get(&src_sid).unwrap().inventory_id;
    sim.inv_add_simple_item(
        src_inv,
        crate::inventory::ItemKind::Bread,
        5,
        Some(elf_id),
        None,
    );
    sim.inv_add_simple_item(src_inv, crate::inventory::ItemKind::Bread, 5, None, None);

    // Destination building: same priority, wants bread.
    let dst_anchor = VoxelCoord::new(tree_pos.x + 6, tree_pos.y, tree_pos.z);
    let _dst_sid = insert_building(
        &mut sim,
        dst_anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );

    sim.process_logistics_heartbeat();

    // Source has exactly 5 unowned = 5 wanted, so surplus = 0.
    // No haul task should be created.
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(
        haul_tasks.len(),
        0,
        "No surplus when owned items are excluded from held count"
    );
}

#[test]
fn logistics_current_inventory_excludes_owned_items() {
    // A building that wants 5 bread and has 5 owned bread should still
    // create a haul task because the owned bread doesn't satisfy the want.
    let mut sim = test_sim(legacy_test_seed());

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);

    // Building wants 5 bread and already contains 5 owned bread.
    let anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let sid = insert_building(
        &mut sim,
        anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );
    let inv = sim.db.structures.get(&sid).unwrap().inventory_id;
    sim.inv_add_simple_item(
        inv,
        crate::inventory::ItemKind::Bread,
        5,
        Some(elf_id),
        None,
    );

    // Place unowned bread in a ground pile as a source.
    let pile_pos = tree_pos;
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            5,
            None,
            None,
        );
    }

    sim.process_logistics_heartbeat();

    // Should create a haul task for 5 bread — the owned bread in the
    // building doesn't count toward satisfying the want.
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(
        haul_tasks.len(),
        1,
        "Owned bread in building should not satisfy the want"
    );
    let haul = sim.task_haul_data(haul_tasks[0].id).unwrap();
    assert_eq!(haul.quantity, 5);
}

#[test]
fn logistics_reserve_haul_items_skips_owned_stacks() {
    // Directly test that reserve_haul_items only reserves unowned items.
    let mut sim = test_sim(legacy_test_seed());

    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);

    let pile_id = sim.ensure_ground_pile(pile_pos);
    let pile = sim.db.ground_piles.get(&pile_id).unwrap();
    // 3 owned + 5 unowned.
    sim.inv_add_simple_item(
        pile.inventory_id,
        crate::inventory::ItemKind::Bread,
        3,
        Some(elf_id),
        None,
    );
    sim.inv_add_simple_item(
        pile.inventory_id,
        crate::inventory::ItemKind::Bread,
        5,
        None,
        None,
    );

    let task_id = TaskId::new(&mut sim.rng);
    insert_stub_task(&mut sim, task_id);
    sim.reserve_haul_items(
        &task::HaulSource::GroundPile(pile_pos),
        crate::inventory::ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
        5,
        task_id,
    );

    // Verify: only unowned items should be reserved.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&pile.inventory_id, tabulosity::QueryOpts::ASC);
    let owned_reserved: u32 = stacks
        .iter()
        .filter(|s| s.owner == Some(elf_id) && s.reserved_by.is_some())
        .map(|s| s.quantity)
        .sum();
    assert_eq!(
        owned_reserved, 0,
        "Owned items must not be reserved by haul tasks"
    );

    let unowned_reserved: u32 = stacks
        .iter()
        .filter(|s| s.owner.is_none() && s.reserved_by.is_some())
        .map(|s| s.quantity)
        .sum();
    assert_eq!(
        unowned_reserved, 5,
        "All 5 unowned items should be reserved"
    );
}

#[test]
fn crafting_reserve_skips_owned_items() {
    // inv_reserve_items (used by crafting) should also skip owned items
    // so that an elf's personal belongings stored in a workshop aren't
    // consumed by a recipe.
    let mut sim = test_sim(legacy_test_seed());

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);

    let anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let sid = insert_building(&mut sim, anchor, Some(5), Vec::new());
    let inv = sim.db.structures.get(&sid).unwrap().inventory_id;

    // 3 owned + 5 unowned arrows.
    sim.inv_add_simple_item(
        inv,
        crate::inventory::ItemKind::Arrow,
        3,
        Some(elf_id),
        None,
    );
    sim.inv_add_simple_item(inv, crate::inventory::ItemKind::Arrow, 5, None, None);

    let task_id = TaskId::new(&mut sim.rng);
    sim.inv_reserve_items(
        inv,
        crate::inventory::ItemKind::Arrow,
        crate::inventory::MaterialFilter::Any,
        5,
        task_id,
    );

    // Verify: only unowned items should be reserved.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv, tabulosity::QueryOpts::ASC);
    let owned_reserved: u32 = stacks
        .iter()
        .filter(|s| s.owner == Some(elf_id) && s.reserved_by.is_some())
        .map(|s| s.quantity)
        .sum();
    assert_eq!(
        owned_reserved, 0,
        "Owned items must not be reserved for crafting"
    );
}

// -----------------------------------------------------------------------
// Personal acquisition: prefer reclaiming owned items
// -----------------------------------------------------------------------

#[test]
fn personal_acquisition_prefers_owned_items_in_ground_pile() {
    // An elf who wants bread and has owned bread in a ground pile should
    // reclaim it rather than acquiring different unowned bread.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
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

    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Pile A: 2 bread owned by this elf.
    let owned_pile_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(owned_pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            inventory::ItemKind::Bread,
            2,
            Some(elf_id),
            None,
        );
    }

    // Pile B: 5 unowned bread at a different location.
    let unowned_pile_pos = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(unowned_pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bread, 5, None, None);
    }

    // Directly call check_creature_wants.
    sim.check_creature_wants(elf_id);

    // Elf should have an AcquireItem task targeting the owned pile.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let task_id = elf.current_task.expect("Elf should have a task");
    let acquire = sim
        .task_acquire_data(task_id)
        .expect("Task should be AcquireItem");
    assert_eq!(acquire.item_kind, inventory::ItemKind::Bread);
    assert_eq!(
        acquire.source_kind,
        crate::db::HaulSourceKind::Pile,
        "Source should be a ground pile"
    );

    // The owned pile's bread should be reserved.
    let owned_pile = sim
        .db
        .ground_piles
        .by_position(&owned_pile_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    let reserved: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&owned_pile.inventory_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.reserved_by.is_some())
        .map(|s| s.quantity)
        .sum();
    assert_eq!(reserved, 2, "Owned bread in the pile should be reserved");

    // The unowned pile should have NO reservations.
    let unowned_pile = sim
        .db
        .ground_piles
        .by_position(&unowned_pile_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    let unowned_reserved: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&unowned_pile.inventory_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.reserved_by.is_some())
        .map(|s| s.quantity)
        .sum();
    assert_eq!(
        unowned_reserved, 0,
        "Unowned bread should not be reserved when owned bread is available"
    );
}

#[test]
fn personal_acquisition_falls_back_to_unowned() {
    // When no owned items exist elsewhere, the elf should acquire unowned items.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
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

    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Only unowned bread available.
    let pile_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bread, 5, None, None);
    }

    sim.check_creature_wants(elf_id);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Elf should acquire unowned bread when no owned items exist"
    );
}

#[test]
fn personal_acquisition_skips_other_creatures_owned_items() {
    // Elf A's owned bread should not be reclaimed by elf B.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
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

    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Pile has bread owned by elf A — no unowned bread anywhere.
    let pile_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            inventory::ItemKind::Bread,
            5,
            Some(elf_a),
            None,
        );
    }

    // Elf B wants bread but only elf A's owned bread exists.
    sim.check_creature_wants(elf_b);

    let elf_b_data = sim.db.creatures.get(&elf_b).unwrap();
    assert!(
        elf_b_data.current_task.is_none(),
        "Elf B must not acquire elf A's owned bread"
    );
}

#[test]
fn military_acquisition_prefers_owned_items() {
    // A soldier with owned bow in a ground pile should reclaim it rather
    // than acquiring a different unowned bow.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
    sim.species_table = sim
        .config
        .species
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();

    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Create military group wanting a bow.
    let civ_id = sim.player_civ_id.unwrap();
    let group_id = sim
        .db
        .insert_military_group_auto(|id| crate::db::MilitaryGroup {
            id,
            civ_id,
            name: "Test".into(),
            is_default_civilian: false,
            engagement_style: Default::default(),
            equipment_wants: vec![building::LogisticsWant {
                item_kind: inventory::ItemKind::Bow,
                material_filter: inventory::MaterialFilter::Any,
                target_quantity: 1,
            }],
        })
        .unwrap();
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.military_group = Some(group_id);
        sim.db.update_creature(c).unwrap();
    }

    // Pile A: 1 bow owned by this elf.
    let owned_pile_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(owned_pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            inventory::ItemKind::Bow,
            1,
            Some(elf_id),
            None,
        );
    }

    // Pile B: 1 unowned bow.
    let unowned_pile_pos = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(unowned_pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bow, 1, None, None);
    }

    sim.check_military_equipment_wants(elf_id);

    // Should have a task.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let task_id = elf.current_task.expect("Elf should have acquisition task");

    // The owned bow should be reserved, the unowned bow should not.
    let owned_pile = sim
        .db
        .ground_piles
        .by_position(&owned_pile_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    let owned_reserved: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&owned_pile.inventory_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.reserved_by == Some(task_id))
        .map(|s| s.quantity)
        .sum();
    assert_eq!(owned_reserved, 1, "Owned bow should be reserved");

    let unowned_pile = sim
        .db
        .ground_piles
        .by_position(&unowned_pile_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    let unowned_reserved: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&unowned_pile.inventory_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.reserved_by.is_some())
        .map(|s| s.quantity)
        .sum();
    assert_eq!(
        unowned_reserved, 0,
        "Unowned bow should not be reserved when owned bow is available"
    );
}

#[test]
fn find_owned_item_source_searches_buildings() {
    // Owned items in a building should be found by find_owned_item_source.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let sid = insert_building(&mut sim, anchor, Some(5), Vec::new());
    let inv = sim.db.structures.get(&sid).unwrap().inventory_id;
    sim.inv_add_simple_item(inv, inventory::ItemKind::Bread, 3, Some(elf_id), None);

    let result = sim.find_owned_item_source(
        inventory::ItemKind::Bread,
        inventory::MaterialFilter::Any,
        3,
        elf_id,
    );
    assert!(result.is_some(), "Should find owned bread in building");
    let (source, qty, _) = result.unwrap();
    assert_eq!(qty, 3);
    assert!(
        matches!(source, task::HaulSource::Building(_)),
        "Source should be a building"
    );
}

#[test]
fn military_acquisition_suppressed_during_combat() {
    // When hostiles are in detection range, check_military_equipment_wants
    // should skip acquisition entirely so the creature stays idle for combat.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
    sim.species_table = sim
        .config
        .species
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();

    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Put elf in soldiers group wanting a bow.
    let soldiers = sim
        .db
        .military_groups
        .by_civ_id(&sim.player_civ_id.unwrap(), tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| !g.is_default_civilian)
        .unwrap();
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.military_group = Some(soldiers.id);
        sim.db.update_creature(c).unwrap();
    }

    // Place an unowned bow in a ground pile (elf could acquire it).
    let pile_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bow, 1, None, None);
    }

    // Spawn a troll near the elf so hostiles are in detection range.
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    let mut events = Vec::new();
    let troll_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    sim.spawn_creature(Species::Troll, troll_pos, &mut events);

    // Call check_military_equipment_wants — should do nothing due to hostiles.
    sim.check_military_equipment_wants(elf_id);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_none(),
        "Should not create acquisition task when hostiles are nearby"
    );
}

#[test]
fn military_acquisition_falls_back_to_unowned() {
    // When no owned items exist elsewhere, military acquisition should
    // fall back to acquiring unowned items.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
    sim.species_table = sim
        .config
        .species
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();

    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Put elf in soldiers group wanting a bow.
    let soldiers = sim
        .db
        .military_groups
        .by_civ_id(&sim.player_civ_id.unwrap(), tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| !g.is_default_civilian)
        .unwrap();
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.military_group = Some(soldiers.id);
        sim.db.update_creature(c).unwrap();
    }

    // Only unowned bow available.
    let pile_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bow, 1, None, None);
    }

    sim.check_military_equipment_wants(elf_id);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Should acquire unowned bow when no owned items exist"
    );
}

#[test]
fn military_acquisition_skips_other_creatures_owned_items() {
    // Soldier B should not reclaim soldier A's owned equipment.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
    sim.species_table = sim
        .config
        .species
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();

    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Put elf_b in soldiers group wanting a bow.
    let soldiers = sim
        .db
        .military_groups
        .by_civ_id(&sim.player_civ_id.unwrap(), tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| !g.is_default_civilian)
        .unwrap();
    {
        let mut c = sim.db.creatures.get(&elf_b).unwrap();
        c.military_group = Some(soldiers.id);
        sim.db.update_creature(c).unwrap();
    }

    // Only elf_a's owned bow exists — no unowned bows.
    let pile_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            inventory::ItemKind::Bow,
            1,
            Some(elf_a),
            None,
        );
    }

    sim.check_military_equipment_wants(elf_b);

    let elf_b_data = sim.db.creatures.get(&elf_b).unwrap();
    assert!(
        elf_b_data.current_task.is_none(),
        "Soldier B must not reclaim soldier A's owned bow"
    );
}

#[test]
fn find_owned_item_source_skips_reserved_owned_items() {
    // Owned items already reserved by another task should not be found
    // by find_owned_item_source (prevents double-reservation).
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let pile_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    let pile_id = sim.ensure_ground_pile(pile_pos);
    let pile = sim.db.ground_piles.get(&pile_id).unwrap();
    sim.inv_add_simple_item(
        pile.inventory_id,
        inventory::ItemKind::Bread,
        3,
        Some(elf_id),
        None,
    );

    // Reserve all owned items for a fake task.
    let fake_task_id = TaskId::new(&mut sim.rng);
    insert_stub_task(&mut sim, fake_task_id);
    sim.inv_reserve_owned_items(
        pile.inventory_id,
        inventory::ItemKind::Bread,
        inventory::MaterialFilter::Any,
        3,
        fake_task_id,
        elf_id,
    );

    // Now find_owned_item_source should return None — everything is reserved.
    let result = sim.find_owned_item_source(
        inventory::ItemKind::Bread,
        inventory::MaterialFilter::Any,
        3,
        elf_id,
    );
    assert!(
        result.is_none(),
        "Reserved owned items should not be found as a reclaim source"
    );
}

#[test]
fn inv_reserve_owned_items_splits_stack_preserving_owner() {
    // When reserving a partial owned stack, both the remaining and reserved
    // halves should preserve the owner field.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let pile_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
    let pile_id = sim.ensure_ground_pile(pile_pos);
    let pile = sim.db.ground_piles.get(&pile_id).unwrap();
    sim.inv_add_simple_item(
        pile.inventory_id,
        inventory::ItemKind::Bread,
        5,
        Some(elf_id),
        None,
    );

    let task_id = TaskId::new(&mut sim.rng);
    insert_stub_task(&mut sim, task_id);
    let material = sim.inv_reserve_owned_items(
        pile.inventory_id,
        inventory::ItemKind::Bread,
        inventory::MaterialFilter::Any,
        3,
        task_id,
        elf_id,
    );

    // Check stacks after split.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&pile.inventory_id, tabulosity::QueryOpts::ASC);

    let reserved: Vec<_> = stacks.iter().filter(|s| s.reserved_by.is_some()).collect();
    let unreserved: Vec<_> = stacks.iter().filter(|s| s.reserved_by.is_none()).collect();

    assert_eq!(reserved.len(), 1, "Should have one reserved stack");
    assert_eq!(reserved[0].quantity, 3);
    assert_eq!(
        reserved[0].owner,
        Some(elf_id),
        "Reserved stack must keep owner"
    );

    assert_eq!(unreserved.len(), 1, "Should have one unreserved stack");
    assert_eq!(unreserved[0].quantity, 2);
    assert_eq!(
        unreserved[0].owner,
        Some(elf_id),
        "Unreserved stack must keep owner"
    );

    // Return value should be the material (None for bread).
    assert_eq!(material, None);
}

// =========================================================================
// Auto-logistics (unified crafting)
// =========================================================================

#[test]
fn set_recipe_auto_logistics_updates_fields() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar_id = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0]
        .id;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeAutoLogistics {
            active_recipe_id: ar_id,
            auto_logistics: false,
            spare_iterations: 5,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let ar = sim.db.active_recipes.get(&ar_id).unwrap();
    assert!(!ar.auto_logistics);
    assert_eq!(ar.spare_iterations, 5);
}

#[test]
fn auto_logistics_generates_wants_from_active_recipe() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Clear old workshop explicit wants and enable crafting.
    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow).
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    // Set target: 1 bow.
    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 1,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    // Auto-logistics is enabled by default. runs_needed = ceil(1/1) = 1.
    // Input: 1 Bowstring per run → auto_want = 1 * 1 = 1.
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring)
        .expect("Should have a Bowstring want from auto-logistics");
    assert_eq!(bowstring_want.target_quantity, 1);
}

#[test]
fn auto_logistics_spare_iterations_add_extra_wants() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Clear old workshop explicit wants and enable crafting.
    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow).
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];

    // Set target: 1 bow, spare_iterations: 3.
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 1,
        },
    };
    let set_spare_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeAutoLogistics {
            active_recipe_id: ar.id,
            auto_logistics: true,
            spare_iterations: 3,
        },
    };
    sim.step(&[set_target_cmd, set_spare_cmd], sim.tick + 1);

    // runs_needed = 1 (ceil(1/1)), spare = 3, total = 4.
    // auto_want = 1 * 4 = 4 Bowstring.
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring)
        .expect("Should have a Bowstring want");
    assert_eq!(bowstring_want.target_quantity, 4);
}

#[test]
fn auto_logistics_sums_with_explicit_wants() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let enable_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCraftingEnabled {
            structure_id,
            enabled: true,
        },
    };
    sim.step(&[enable_cmd], sim.tick + 1);

    // Enable logistics and set an explicit want of 5 Bowstring.
    let logistics_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsPriority {
                structure_id,
                priority: Some(5),
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![crate::building::LogisticsWant {
                    item_kind: inventory::ItemKind::Bowstring,
                    material_filter: inventory::MaterialFilter::Any,
                    target_quantity: 5,
                }],
            },
        },
    ];
    sim.step(&logistics_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow) with target 1.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 1,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    // Explicit = 5, auto = 1 → merged = 6.
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring)
        .expect("Should have Bowstring want");
    assert_eq!(bowstring_want.target_quantity, 6);
}

#[test]
fn auto_logistics_disabled_when_crafting_disabled() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Clear explicit wants (crafting starts enabled, we disable it below).
    let clear_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetLogisticsWants {
            structure_id,
            wants: vec![],
        },
    };
    sim.step(&[clear_cmd], sim.tick + 1);

    // Manually add Grow Oak Bow recipe and set a target.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 1,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    // Now disable crafting.
    let disable_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCraftingEnabled {
            structure_id,
            enabled: false,
        },
    };
    sim.step(&[disable_cmd], sim.tick + 1);

    // crafting_enabled is false → no auto-logistics wants.
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring);
    assert!(
        bowstring_want.is_none(),
        "Should not generate auto-logistics when crafting is disabled"
    );
}

#[test]
fn auto_logistics_disabled_per_recipe() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow).
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];

    // Set target, then disable auto-logistics.
    let cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeOutputTarget {
                active_recipe_target_id: target.id,
                target_quantity: 1,
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeAutoLogistics {
                active_recipe_id: ar.id,
                auto_logistics: false,
                spare_iterations: 0,
            },
        },
    ];
    sim.step(&cmds, sim.tick + 1);

    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring);
    assert!(
        bowstring_want.is_none(),
        "Should not generate auto-logistics when recipe auto_logistics is false"
    );
}

#[test]
fn auto_logistics_no_input_recipe_generates_no_wants() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Arrow recipe has no inputs. Add it manually (not auto-added).
    sim.add_active_recipe(structure_id, Recipe::GrowArrow, Some(Material::Oak));

    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowArrow && r.material == Some(Material::Oak))
        .unwrap();
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 100,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    // No inputs → no auto-logistics wants.
    let wants = sim.compute_effective_wants(structure_id);
    assert!(
        wants.is_empty(),
        "Arrow has no inputs, should generate no wants"
    );
}

#[test]
fn auto_logistics_spare_iterations_when_target_met() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow).
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];

    // Set target: 1 bow, spare: 2.
    let cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeOutputTarget {
                active_recipe_target_id: target.id,
                target_quantity: 1,
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeAutoLogistics {
                active_recipe_id: ar.id,
                auto_logistics: true,
                spare_iterations: 2,
            },
        },
    ];
    sim.step(&cmds, sim.tick + 1);

    // Add 1 oak bow to the building's inventory so target is met.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bow,
        1,
        None,
        None,
        Some(inventory::Material::Oak),
        0,
        None,
        None,
    );

    // runs_needed = 0 (target met), spare = 2, total = 2.
    // auto_want = 1 * 2 = 2 Bowstring (stockpiling for spare iterations).
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring)
        .expect("Spare iterations should still generate wants when target is met");
    assert_eq!(bowstring_want.target_quantity, 2);
}

#[test]
fn remove_active_recipe_cleans_up_pending_craft_task() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Place furniture so the building is functional.
    let furn_ids: Vec<_> = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|f| f.id)
        .collect();
    for fid in furn_ids {
        let mut f = sim.db.furniture.get(&fid).unwrap();
        f.placed = true;
        sim.db.update_furniture(f).unwrap();
    }

    // Manually add Grow Oak Bow recipe (1 Bowstring → 1 Bow) and set a target.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowBow && r.material == Some(Material::Oak))
        .unwrap();
    let target = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    let target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 100,
        },
    };
    sim.step(&[target_cmd], sim.tick + 1);

    // Stock building with bowstring and run monitor to create a craft task.
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bowstring, 1, None, None);
    sim.process_unified_crafting_monitor();

    // Verify craft task exists with reserved items.
    let craft_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state != TaskState::Complete)
        .collect();
    assert_eq!(craft_tasks.len(), 1, "Should have 1 pending craft task");
    let task_id = craft_tasks[0].id;

    let reserved = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.reserved_by == Some(task_id))
        .count();
    assert!(reserved > 0, "Fruit should be reserved");

    // Remove the active recipe.
    let rm_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::RemoveActiveRecipe {
            active_recipe_id: ar.id,
        },
    };
    sim.step(&[rm_cmd], sim.tick + 1);

    // The recipe and targets should be gone.
    assert!(sim.db.active_recipes.get(&ar.id).is_none());
    assert!(
        sim.db
            .active_recipe_targets
            .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)
            .is_empty()
    );
}

// =========================================================================
// Wants, gap, surplus, material display
// =========================================================================

#[test]
fn overlapping_wants_additive() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim
        .db
        .insert_inventory_auto(|id| crate::db::Inventory {
            id,
            owner_kind: crate::db::InventoryOwnerKind::Structure,
        })
        .unwrap();

    let species_a = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(1));
    let wants = vec![
        building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 5,
        },
        building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Specific(species_a),
            target_quantity: 10,
        },
    ];
    sim.set_inv_wants(inv_id, &wants);

    // Both should be stored independently.
    let stored = sim.inv_wants(inv_id);
    assert_eq!(stored.len(), 2);

    // Per-filter queries.
    assert_eq!(
        sim.inv_want_target(
            inv_id,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Any
        ),
        5
    );
    assert_eq!(
        sim.inv_want_target(
            inv_id,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Specific(species_a)
        ),
        10
    );

    // Total for item kind sums both.
    assert_eq!(
        sim.inv_want_target_total(inv_id, inventory::ItemKind::Fruit),
        15
    );
}

#[test]
fn gap_calculation_with_material_filter() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim
        .db
        .insert_inventory_auto(|id| crate::db::Inventory {
            id,
            owner_kind: crate::db::InventoryOwnerKind::Structure,
        })
        .unwrap();

    let species_a = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(1));

    // Set up wants: "Any Fruit: 5" and "Species A Fruit: 10".
    let wants = vec![
        building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 5,
        },
        building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Specific(species_a),
            target_quantity: 10,
        },
    ];
    sim.set_inv_wants(inv_id, &wants);

    // Add 12 of species A to the inventory.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Fruit,
        12,
        None,
        None,
        Some(species_a),
        0,
        None,
        None,
    );

    // Any want: current=12 (all fruit), target=5 → gap=0.
    let any_current = sim.inv_item_count(
        inv_id,
        inventory::ItemKind::Fruit,
        inventory::MaterialFilter::Any,
    );
    assert_eq!(any_current, 12);
    assert!(any_current >= 5, "Any want should be satisfied");

    // Specific want: current=12, target=10 → gap=0.
    let specific_current = sim.inv_item_count(
        inv_id,
        inventory::ItemKind::Fruit,
        inventory::MaterialFilter::Specific(species_a),
    );
    assert_eq!(specific_current, 12);
    assert!(specific_current >= 10, "Specific want should be satisfied");
}

#[test]
fn surplus_uses_want_target_total() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim
        .db
        .insert_inventory_auto(|id| crate::db::Inventory {
            id,
            owner_kind: crate::db::InventoryOwnerKind::Structure,
        })
        .unwrap();

    let species_a = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(1));
    let species_b = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(2));

    // Want: "Species A Fruit: 10"
    let wants = vec![building::LogisticsWant {
        item_kind: inventory::ItemKind::Fruit,
        material_filter: inventory::MaterialFilter::Specific(species_a),
        target_quantity: 10,
    }];
    sim.set_inv_wants(inv_id, &wants);

    // Hold 8 of species B.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Fruit,
        8,
        None,
        None,
        Some(species_b),
        0,
        None,
        None,
    );

    // Surplus for Specific(species_b): held=8, total wanted=10 → surplus=0.
    let held = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Fruit,
        inventory::MaterialFilter::Specific(species_b),
    );
    let wanted = sim.inv_want_target_total(inv_id, inventory::ItemKind::Fruit);
    let surplus = held.saturating_sub(wanted);
    assert_eq!(
        surplus, 0,
        "Conservative surplus: total wanted (10) > held species B (8)"
    );
}

#[test]
fn task_haul_data_serde_backward_compat() {
    // Old TaskHaulData without material_filter and hauled_material fields.
    let json = r#"{
            "id": 1,
            "task_id": "00000000-0000-0000-0000-000000000001",
            "item_kind": "Bread",
            "quantity": 5,
            "phase": "GoingToSource",
            "source_kind": "Pile",
            "destination_coord": [0, 0, 0]
        }"#;
    let data: crate::db::TaskHaulData = serde_json::from_str(json).unwrap();
    assert_eq!(data.material_filter, inventory::MaterialFilter::Any);
    assert_eq!(data.hauled_material, None);
}

#[test]
fn material_item_display_name_fruit_species() {
    let sim = test_sim(legacy_test_seed());
    // The test_sim worldgen should have created fruit species.
    // Verify that material_item_display_name uses the Vaelith name.
    if let Some(species) = sim.db.fruit_species.iter_all().next() {
        let mat = inventory::Material::FruitSpecies(species.id);
        let name = sim.material_item_display_name(inventory::ItemKind::Fruit, mat);
        // Should contain the Vaelith name, not generic "Fruit".
        assert!(
            name.contains(&species.vaelith_name),
            "Display name '{name}' should contain Vaelith name '{}'",
            species.vaelith_name
        );
    }
}

#[test]
fn material_item_display_name_wood() {
    let sim = test_sim(legacy_test_seed());
    let name = sim.material_item_display_name(inventory::ItemKind::Bow, inventory::Material::Oak);
    assert_eq!(name, "Oak Bow");
}

#[test]
fn logistics_heartbeat_specific_filter_hauls_correct_species() {
    let mut sim = test_sim(legacy_test_seed());

    let species_a = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(1));
    let species_b = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(2));

    // Place a ground pile with mixed fruit.
    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_item(
            pile.inventory_id,
            inventory::ItemKind::Fruit,
            3,
            None,
            None,
            Some(species_a),
            0,
            None,
            None,
        );
        sim.inv_add_item(
            pile.inventory_id,
            inventory::ItemKind::Fruit,
            5,
            None,
            None,
            Some(species_b),
            0,
            None,
            None,
        );
    }

    // Create building wanting only species B fruit.
    let building_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    let _sid = insert_building(
        &mut sim,
        building_anchor,
        Some(5),
        vec![building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Specific(species_b),
            target_quantity: 3,
        }],
    );

    sim.process_logistics_heartbeat();

    // Should have created a haul task.
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(haul_tasks.len(), 1, "Expected 1 haul task");

    let haul = sim
        .task_haul_data(haul_tasks[0].id)
        .expect("Haul task should have haul data");
    assert_eq!(haul.item_kind, inventory::ItemKind::Fruit);
    assert_eq!(haul.quantity, 3);
    // The haul should track the actual material and the filter.
    assert_eq!(
        haul.material_filter,
        inventory::MaterialFilter::Specific(species_b)
    );
    assert_eq!(haul.hauled_material, Some(species_b));

    // Species A should be completely unreserved.
    let pile = sim
        .db
        .ground_piles
        .by_position(&pile_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        sim.inv_unreserved_item_count(
            pile.inventory_id,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Specific(species_a)
        ),
        3,
        "Species A should be untouched"
    );
}

#[test]
fn in_transit_counting_uses_hauled_material() {
    let mut sim = test_sim(legacy_test_seed());

    let species_a = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(1));
    let species_b = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(2));

    // Place ground piles with species A and B.
    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_item(
            pile.inventory_id,
            inventory::ItemKind::Fruit,
            10,
            None,
            None,
            Some(species_a),
            0,
            None,
            None,
        );
        sim.inv_add_item(
            pile.inventory_id,
            inventory::ItemKind::Fruit,
            10,
            None,
            None,
            Some(species_b),
            0,
            None,
            None,
        );
    }

    // Create building wanting "Any Fruit: 20".
    let building_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    let sid = insert_building(
        &mut sim,
        building_anchor,
        Some(5),
        vec![building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 20,
        }],
    );

    sim.process_logistics_heartbeat();

    // Should have created a haul task.
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert!(!haul_tasks.is_empty(), "Expected at least 1 haul task");

    let haul = sim
        .task_haul_data(haul_tasks[0].id)
        .expect("Haul task should have haul data");
    let hauled_mat = haul.hauled_material;

    // In-transit with Any filter should count this haul.
    let any_in_transit = sim.count_in_transit_items(
        sid,
        inventory::ItemKind::Fruit,
        inventory::MaterialFilter::Any,
    );
    assert!(
        any_in_transit > 0,
        "Any filter should count in-transit items"
    );

    // In-transit with Specific(hauled material) should also count it.
    if let Some(mat) = hauled_mat {
        let specific_in_transit = sim.count_in_transit_items(
            sid,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Specific(mat),
        );
        assert!(
            specific_in_transit > 0,
            "Specific filter matching hauled material should count"
        );

        // In-transit with a different Specific filter should NOT count it.
        let other_mat = if mat == species_a {
            species_b
        } else {
            species_a
        };
        let other_in_transit = sim.count_in_transit_items(
            sid,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Specific(other_mat),
        );
        assert_eq!(
            other_in_transit, 0,
            "Specific filter for different material should not count in-transit"
        );
    }
}

#[test]
fn haul_preserves_material_through_pickup_and_dropoff() {
    let mut sim = test_sim(legacy_test_seed());

    let species_a = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(1));

    // Place a ground pile with fruit that has a species material.
    let pile_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_id = sim.ensure_ground_pile(pile_pos);
    let pile = sim.db.ground_piles.get(&pile_id).unwrap();
    sim.inv_add_item(
        pile.inventory_id,
        inventory::ItemKind::Fruit,
        5,
        None,
        None,
        Some(species_a),
        0,
        None,
        None,
    );

    // Create a building with a Specific want for species_a fruit.
    let building_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    let structure_id = insert_building(
        &mut sim,
        building_anchor,
        Some(5),
        vec![building::LogisticsWant {
            item_kind: inventory::ItemKind::Fruit,
            material_filter: inventory::MaterialFilter::Specific(species_a),
            target_quantity: 10,
        }],
    );
    let dest_inv = sim.db.structures.get(&structure_id).unwrap().inventory_id;

    // Run logistics heartbeat to create haul task.
    sim.process_logistics_heartbeat();

    // Find the haul task and verify hauled_material is set.
    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == crate::db::TaskKindTag::Haul)
        .collect();
    assert_eq!(haul_tasks.len(), 1, "Should have one haul task");
    let haul_data = sim.task_haul_data(haul_tasks[0].id).unwrap();
    assert_eq!(haul_data.hauled_material, Some(species_a));

    // Spawn an elf and fast-forward to let it complete the haul.
    let _elf_id = sim.spawn_creature(crate::types::Species::Elf, pile_pos, &mut Vec::new());
    for _ in 0..500 {
        sim.step(&[], sim.tick + 100);
    }

    // Check the destination building's inventory: fruit should have the
    // species material, not None.
    let fruit_stacks: Vec<_> = sim
        .inv_items(dest_inv)
        .into_iter()
        .filter(|s| s.kind == inventory::ItemKind::Fruit)
        .collect();
    if !fruit_stacks.is_empty() {
        for stack in &fruit_stacks {
            assert_eq!(
                stack.material,
                Some(species_a),
                "Hauled fruit should preserve species material in destination inventory"
            );
        }
    }
}

// =========================================================================
// Shoes acquisition
// =========================================================================

#[test]
fn elf_wanting_shoes_ignores_boots() {
    // Boots and Shoes are distinct item kinds. An elf wanting Shoes
    // should not pick up Boots (which are armor, not clothing).
    let mut sim = test_sim(legacy_test_seed());

    sim.config.elf_default_wants = vec![crate::building::LogisticsWant {
        item_kind: inventory::ItemKind::Shoes,
        material_filter: inventory::MaterialFilter::Any,
        target_quantity: 1,
    }];
    sim.config.elf_starting_bread = 0;

    let elf_id = spawn_elf(&mut sim);

    // Place armor boots on the ground near the tree.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x + 1, 1, tree_pos.z);
    let pile_id = sim.ensure_ground_pile(pile_pos);
    let pile_inv = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;
    sim.inv_add_item(
        pile_inv,
        inventory::ItemKind::Boots,
        1,
        None,
        None,
        Some(inventory::Material::Oak),
        0,
        None,
        None,
    );

    // Elf wants Shoes, only Boots available — should not acquire.
    sim.check_creature_wants(elf_id);

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_none(),
        "Elf should not acquire Boots when wanting Shoes"
    );
}

#[test]
fn elf_acquires_shoes_as_clothing() {
    // Shoes are clothing — elves wanting shoes should pick them up.
    let mut sim = test_sim(legacy_test_seed());

    sim.config.elf_default_wants = vec![crate::building::LogisticsWant {
        item_kind: inventory::ItemKind::Shoes,
        material_filter: inventory::MaterialFilter::Any,
        target_quantity: 1,
    }];
    sim.config.elf_starting_bread = 0;

    let elf_id = spawn_elf(&mut sim);

    // Place shoes on the ground.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x + 1, 1, tree_pos.z);
    let pile_id = sim.ensure_ground_pile(pile_pos);
    let pile_inv = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;
    let fruit_mat = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(0));
    sim.inv_add_item(
        pile_inv,
        inventory::ItemKind::Shoes,
        1,
        None,
        None,
        Some(fruit_mat),
        0,
        None,
        None,
    );

    // Run heartbeat — elf should create an AcquireItem task for shoes.
    sim.check_creature_wants(elf_id);

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Elf should acquire shoes as clothing"
    );
}

// =========================================================================
// Haul durability preservation
// =========================================================================

#[test]
fn haul_dropoff_preserves_durability() {
    // Items moved between inventories via inv_move_items must preserve
    // all properties including quality and durability.
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);

    // Put damaged arrows in source (simulating creature carrying hauled items).
    sim.inv_add_item_with_durability(
        src,
        inventory::ItemKind::Arrow,
        5,
        None,
        None,
        Some(inventory::Material::Oak),
        3, // quality
        2, // current_hp (damaged)
        3, // max_hp
        None,
        None,
    );

    // Move items the way haul dropoff now does.
    let material = Some(inventory::Material::Oak);
    sim.inv_move_items(
        src,
        dst,
        Some(inventory::ItemKind::Arrow),
        Some(material),
        Some(5),
    );

    let dst_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    assert_eq!(dst_stacks.len(), 1);
    let stack = &dst_stacks[0];
    assert_eq!(stack.quality, 3, "quality must survive haul dropoff");
    assert_eq!(stack.current_hp, 2, "current_hp must survive haul dropoff");
    assert_eq!(
        stack.material,
        Some(inventory::Material::Oak),
        "material must survive haul dropoff"
    );
}

// =========================================================================
// Logistics heartbeat triggers creature gravity
// =========================================================================

// ── Logistics ownership leak tests ──────────────────────────────────────────

#[test]
fn haul_dropoff_does_not_move_owned_bread_into_building() {
    // An elf carrying 3 personally-owned bread + 5 reserved-for-haul bread
    // resolves a dropoff. Only the 5 reserved items should land in the
    // building; the 3 owned bread must stay in the elf's inventory.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);
    let creature_inv = sim.creature_inv(elf_id);

    // Destination building.
    let building_anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let dest_sid = insert_building(
        &mut sim,
        building_anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 10,
        }],
    );
    let dest_inv = sim.db.structures.get(&dest_sid).unwrap().inventory_id;

    // Dummy source pile position (not actually used by dropoff).
    let pile_pos = tree_pos;

    // Create the haul task in GoingToDestination phase.
    let task_id = TaskId::new(&mut sim.rng);
    let haul_task = Task {
        id: task_id,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
            source: task::HaulSource::GroundPile(pile_pos),
            destination: dest_sid,
            phase: task::HaulPhase::GoingToDestination,
            destination_coord: building_anchor,
        },
        state: TaskState::InProgress,
        location: building_anchor,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_task);
    // Fix phase to GoingToDestination (insert_task sets from the kind).
    {
        let mut h = sim.db.task_haul_data.get(&task_id).unwrap();
        h.phase = crate::task::HaulPhase::GoingToDestination;
        sim.db.update_task_haul_data(h).unwrap();
    }

    // Give elf 3 personally owned bread.
    sim.inv_add_simple_item(
        creature_inv,
        crate::inventory::ItemKind::Bread,
        3,
        Some(elf_id),
        None,
    );

    // Give elf 5 bread reserved by the haul task (simulating post-pickup).
    sim.inv_add_simple_item(
        creature_inv,
        crate::inventory::ItemKind::Bread,
        5,
        None,
        Some(task_id),
    );

    // Assign task to elf.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Resolve the dropoff.
    sim.resolve_dropoff_action(elf_id);

    // Building should have exactly 5 unowned bread.
    let building_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&dest_inv, tabulosity::QueryOpts::ASC);
    let building_bread: u32 = building_stacks
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(building_bread, 5, "Building should have exactly 5 bread");
    for stack in &building_stacks {
        assert_eq!(stack.owner, None, "All bread in building should be unowned");
        assert_eq!(
            stack.reserved_by, None,
            "Deposited bread should not be reserved"
        );
    }

    // Elf should still have 3 owned bread.
    let elf_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&creature_inv, tabulosity::QueryOpts::ASC);
    let elf_bread: u32 = elf_stacks
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(elf_bread, 3, "Elf should still have 3 owned bread");
}

#[test]
fn haul_pickup_preserves_reservation_on_carried_items() {
    // After pickup, the hauled items in the creature's inventory must
    // retain reserved_by == task_id so that dropoff can identify them.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);
    let creature_inv = sim.creature_inv(elf_id);

    // Give elf 3 personally owned bread (pre-existing in inventory).
    sim.inv_add_simple_item(
        creature_inv,
        crate::inventory::ItemKind::Bread,
        3,
        Some(elf_id),
        None,
    );

    // Ground pile with 5 bread reserved by the haul task.
    let pile_pos = tree_pos;
    let pile_id = sim.ensure_ground_pile(pile_pos);
    let pile_inv = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;

    // Create the haul task before adding reserved items so the task row
    // exists for FK validation on item_stacks.reserved_by.
    let task_id = TaskId::new(&mut sim.rng);
    let building_anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let dest_sid = insert_building(&mut sim, building_anchor, Some(5), Vec::new());
    let haul_task = Task {
        id: task_id,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
            source: task::HaulSource::GroundPile(pile_pos),
            destination: dest_sid,
            phase: task::HaulPhase::GoingToSource,
            destination_coord: building_anchor,
        },
        state: TaskState::InProgress,
        location: pile_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_task);

    sim.inv_add_simple_item(
        pile_inv,
        crate::inventory::ItemKind::Bread,
        5,
        None,
        Some(task_id),
    );

    // Assign task to elf.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Resolve pickup.
    sim.resolve_pickup_action(elf_id);

    // Elf should now have 3 owned + 5 reserved bread.
    let elf_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&creature_inv, tabulosity::QueryOpts::ASC);
    let owned_bread: u32 = elf_stacks
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread && s.owner == Some(elf_id))
        .map(|s| s.quantity)
        .sum();
    assert_eq!(owned_bread, 3, "Elf should still have 3 owned bread");

    let reserved_bread: u32 = elf_stacks
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread && s.reserved_by == Some(task_id))
        .map(|s| s.quantity)
        .sum();
    assert_eq!(
        reserved_bread, 5,
        "Hauled bread must stay reserved_by the task after pickup"
    );
}

#[test]
fn haul_cleanup_going_to_dest_drops_only_reserved_items() {
    // When a haul task is abandoned in GoingToDestination phase, only the
    // items reserved by that task should be dropped. The creature's
    // personally owned items must stay in their inventory.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);
    let creature_inv = sim.creature_inv(elf_id);

    // Destination building.
    let building_anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let dest_sid = insert_building(&mut sim, building_anchor, Some(5), Vec::new());

    // Create haul task in GoingToDestination phase.
    let pile_pos = tree_pos;
    let task_id = TaskId::new(&mut sim.rng);
    let haul_task = Task {
        id: task_id,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
            source: task::HaulSource::GroundPile(pile_pos),
            destination: dest_sid,
            phase: task::HaulPhase::GoingToDestination,
            destination_coord: building_anchor,
        },
        state: TaskState::InProgress,
        location: building_anchor,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_task);
    {
        let mut h = sim.db.task_haul_data.get(&task_id).unwrap();
        h.phase = crate::task::HaulPhase::GoingToDestination;
        sim.db.update_task_haul_data(h).unwrap();
    }

    // Give elf 3 owned bread + 5 reserved-by-task bread.
    sim.inv_add_simple_item(
        creature_inv,
        crate::inventory::ItemKind::Bread,
        3,
        Some(elf_id),
        None,
    );
    sim.inv_add_simple_item(
        creature_inv,
        crate::inventory::ItemKind::Bread,
        5,
        None,
        Some(task_id),
    );

    // Assign task to elf.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Abandon the haul.
    sim.cleanup_haul_task(elf_id, task_id);

    // Elf should still have 3 owned bread.
    let elf_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&creature_inv, tabulosity::QueryOpts::ASC);
    let elf_bread: u32 = elf_stacks
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(
        elf_bread, 3,
        "Elf should still have 3 owned bread after haul cleanup"
    );

    // Ground pile should have 5 unowned, unreserved bread.
    // The pile may be at a slightly different Y than the elf (surface vs
    // walk position), so find any pile near the elf's XZ.
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    let pile = sim
        .db
        .ground_piles
        .iter_all()
        .find(|p| p.position.x == elf_pos.x && p.position.z == elf_pos.z)
        .expect("Ground pile should exist near elf after haul cleanup");
    let pile_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&pile.inventory_id, tabulosity::QueryOpts::ASC);
    let pile_bread: u32 = pile_stacks
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(pile_bread, 5, "Ground pile should have 5 dropped bread");
    for stack in &pile_stacks {
        assert_eq!(stack.owner, None, "Dropped bread should be unowned");
        assert_eq!(
            stack.reserved_by, None,
            "Dropped bread should be unreserved"
        );
    }
}

#[test]
fn haul_cleanup_going_to_source_clears_reservation_at_source() {
    // When a haul task is abandoned in GoingToSource phase, the reserved
    // items at the source must be unreserved so they become available for
    // other tasks.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);

    // Ground pile with 5 bread, to be reserved by the haul task.
    let pile_pos = tree_pos;
    let pile_id = sim.ensure_ground_pile(pile_pos);
    let pile_inv = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;

    // Create haul task before adding reserved items so the task row
    // exists for FK validation on item_stacks.reserved_by.
    let task_id = TaskId::new(&mut sim.rng);
    let building_anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let dest_sid = insert_building(&mut sim, building_anchor, Some(5), Vec::new());
    let haul_task = Task {
        id: task_id,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
            source: task::HaulSource::GroundPile(pile_pos),
            destination: dest_sid,
            phase: task::HaulPhase::GoingToSource,
            destination_coord: building_anchor,
        },
        state: TaskState::InProgress,
        location: pile_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_task);

    sim.inv_add_simple_item(
        pile_inv,
        crate::inventory::ItemKind::Bread,
        5,
        None,
        Some(task_id),
    );

    // Assign task to elf.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Abandon the haul.
    sim.cleanup_haul_task(elf_id, task_id);

    // All 5 bread in the pile should now be unreserved.
    let pile_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&pile_inv, tabulosity::QueryOpts::ASC);
    let total_bread: u32 = pile_stacks
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(total_bread, 5, "Pile should still have 5 bread");
    for stack in &pile_stacks {
        assert_eq!(
            stack.reserved_by, None,
            "All bread in pile should be unreserved after cleanup"
        );
    }
}

#[test]
fn inv_move_reserved_items_no_matching_stacks_returns_zero() {
    // When no stacks in the source are reserved by the given task,
    // inv_move_reserved_items should return 0 and leave everything intact.
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);

    // Add unreserved bread to source.
    sim.inv_add_simple_item(src, crate::inventory::ItemKind::Bread, 5, None, None);

    let fake_task_id = TaskId::new(&mut sim.rng);
    let moved = sim.inv_move_reserved_items(src, dst, fake_task_id);
    assert_eq!(moved, 0, "Should move nothing when no stacks match");

    // Source should still have 5 bread.
    let src_bread: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(src_bread, 5, "Source bread should be unchanged");

    // Destination should be empty.
    let dst_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    assert!(dst_stacks.is_empty(), "Destination should be empty");
}

#[test]
fn inv_move_reserved_items_clears_reservation_and_merges_at_dest() {
    // After inv_move_reserved_items deposits items, the reservation should
    // be cleared and the items should merge with any existing matching
    // unreserved stacks at the destination.
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);

    let task_id = TaskId::new(&mut sim.rng);
    insert_stub_task(&mut sim, task_id);

    // Destination already has 3 unreserved bread.
    sim.inv_add_simple_item(dst, crate::inventory::ItemKind::Bread, 3, None, None);

    // Source has 5 bread reserved by the task.
    sim.inv_add_simple_item(
        src,
        crate::inventory::ItemKind::Bread,
        5,
        None,
        Some(task_id),
    );

    let moved = sim.inv_move_reserved_items(src, dst, task_id);
    assert_eq!(moved, 5, "Should move all 5 reserved bread");

    // Destination should have 8 unreserved bread in a single merged stack.
    let dst_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    let bread_stacks: Vec<_> = dst_stacks
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .collect();
    assert_eq!(bread_stacks.len(), 1, "Should merge into a single stack");
    assert_eq!(bread_stacks[0].quantity, 8, "Should be 3 + 5 = 8 bread");
    assert_eq!(
        bread_stacks[0].reserved_by, None,
        "Merged stack should be unreserved"
    );
}

#[test]
fn two_concurrent_haul_tasks_dropoff_only_moves_own_reserved_items() {
    // Two elves each carry bread reserved by their own haul task. When
    // elf A drops off, only task A's items should move — elf B's reserved
    // items (in a different inventory) are unaffected.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    let inv_a = sim.creature_inv(elf_a);
    let inv_b = sim.creature_inv(elf_b);

    // Two destination buildings.
    let anchor_a = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let anchor_b = VoxelCoord::new(tree_pos.x + 6, tree_pos.y, tree_pos.z);
    let dest_a = insert_building(&mut sim, anchor_a, Some(5), Vec::new());
    let dest_b = insert_building(&mut sim, anchor_b, Some(5), Vec::new());
    let dest_inv_a = sim.db.structures.get(&dest_a).unwrap().inventory_id;
    let dest_inv_b = sim.db.structures.get(&dest_b).unwrap().inventory_id;

    // Task A: elf A carries 4 reserved bread.
    let task_a = TaskId::new(&mut sim.rng);
    let pile_pos = tree_pos;
    let haul_a = Task {
        id: task_a,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 4,
            source: task::HaulSource::GroundPile(pile_pos),
            destination: dest_a,
            phase: task::HaulPhase::GoingToDestination,
            destination_coord: anchor_a,
        },
        state: TaskState::InProgress,
        location: anchor_a,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_a);
    {
        let mut h = sim.db.task_haul_data.get(&task_a).unwrap();
        h.phase = crate::task::HaulPhase::GoingToDestination;
        sim.db.update_task_haul_data(h).unwrap();
    }
    sim.inv_add_simple_item(
        inv_a,
        crate::inventory::ItemKind::Bread,
        4,
        None,
        Some(task_a),
    );
    {
        let mut c = sim.db.creatures.get(&elf_a).unwrap();
        c.current_task = Some(task_a);
        sim.db.update_creature(c).unwrap();
    }

    // Task B: elf B carries 3 reserved bread.
    let task_b = TaskId::new(&mut sim.rng);
    let haul_b = Task {
        id: task_b,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 3,
            source: task::HaulSource::GroundPile(pile_pos),
            destination: dest_b,
            phase: task::HaulPhase::GoingToDestination,
            destination_coord: anchor_b,
        },
        state: TaskState::InProgress,
        location: anchor_b,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_b);
    {
        let mut h = sim.db.task_haul_data.get(&task_b).unwrap();
        h.phase = crate::task::HaulPhase::GoingToDestination;
        sim.db.update_task_haul_data(h).unwrap();
    }
    sim.inv_add_simple_item(
        inv_b,
        crate::inventory::ItemKind::Bread,
        3,
        None,
        Some(task_b),
    );
    {
        let mut c = sim.db.creatures.get(&elf_b).unwrap();
        c.current_task = Some(task_b);
        sim.db.update_creature(c).unwrap();
    }

    // Drop off task A only.
    sim.resolve_dropoff_action(elf_a);

    // Building A should have 4 bread.
    let a_bread: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&dest_inv_a, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(a_bread, 4, "Building A should have 4 bread from task A");

    // Building B should still be empty.
    let b_bread: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&dest_inv_b, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(
        b_bread, 0,
        "Building B should be empty — task B not dropped off yet"
    );

    // Elf B should still have 3 reserved bread.
    let b_reserved: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_b, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread && s.reserved_by == Some(task_b))
        .map(|s| s.quantity)
        .sum();
    assert_eq!(b_reserved, 3, "Elf B's reserved bread should be untouched");
}

#[test]
fn death_during_haul_drops_all_items_unreserved_and_unowned() {
    // An elf carrying 3 owned bread + 5 reserved (hauled) bread dies.
    // All 8 bread should end up in a ground pile, unowned and unreserved.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);
    let creature_inv = sim.creature_inv(elf_id);

    // Destination building.
    let building_anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let dest_sid = insert_building(&mut sim, building_anchor, Some(5), Vec::new());

    // Create haul task in GoingToDestination phase.
    let pile_pos = tree_pos;
    let task_id = TaskId::new(&mut sim.rng);
    let haul_task = Task {
        id: task_id,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
            source: task::HaulSource::GroundPile(pile_pos),
            destination: dest_sid,
            phase: task::HaulPhase::GoingToDestination,
            destination_coord: building_anchor,
        },
        state: TaskState::InProgress,
        location: building_anchor,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_task);
    {
        let mut h = sim.db.task_haul_data.get(&task_id).unwrap();
        h.phase = crate::task::HaulPhase::GoingToDestination;
        sim.db.update_task_haul_data(h).unwrap();
    }

    // Give elf 3 owned bread + 5 reserved bread.
    sim.inv_add_simple_item(
        creature_inv,
        crate::inventory::ItemKind::Bread,
        3,
        Some(elf_id),
        None,
    );
    sim.inv_add_simple_item(
        creature_inv,
        crate::inventory::ItemKind::Bread,
        5,
        None,
        Some(task_id),
    );

    // Assign task to elf.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Kill the elf.
    let mut events = Vec::new();
    sim.handle_creature_death(elf_id, crate::types::DeathCause::Debug, &mut events);

    // Find any ground piles near the elf's death position.
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    let all_bread_in_piles: Vec<_> = sim
        .db
        .ground_piles
        .iter_all()
        .filter(|p| p.position.x == elf_pos.x && p.position.z == elf_pos.z)
        .flat_map(|p| {
            sim.db
                .item_stacks
                .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
        })
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .collect();

    let total_bread: u32 = all_bread_in_piles.iter().map(|s| s.quantity).sum();
    assert_eq!(total_bread, 8, "All 8 bread should be in ground pile(s)");

    for stack in &all_bread_in_piles {
        assert_eq!(stack.owner, None, "All dropped bread should be unowned");
        assert_eq!(
            stack.reserved_by, None,
            "All dropped bread should be unreserved"
        );
    }

    // Elf's inventory should be empty.
    let elf_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&creature_inv, tabulosity::QueryOpts::ASC);
    assert!(
        elf_stacks.is_empty(),
        "Dead elf's inventory should be empty"
    );
}

#[test]
fn haul_dropoff_does_not_affect_other_item_kinds_in_inventory() {
    // An elf carrying a personally-owned spear + reserved bread drops off
    // the bread. The spear must remain in the elf's inventory.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    sim.config.elf_starting_bows = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);
    let creature_inv = sim.creature_inv(elf_id);

    // Destination building.
    let building_anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let dest_sid = insert_building(&mut sim, building_anchor, Some(5), Vec::new());
    let dest_inv = sim.db.structures.get(&dest_sid).unwrap().inventory_id;

    // Create haul task.
    let pile_pos = tree_pos;
    let task_id = TaskId::new(&mut sim.rng);
    let haul_task = Task {
        id: task_id,
        kind: TaskKind::Haul {
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
            source: task::HaulSource::GroundPile(pile_pos),
            destination: dest_sid,
            phase: task::HaulPhase::GoingToDestination,
            destination_coord: building_anchor,
        },
        state: TaskState::InProgress,
        location: building_anchor,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_task);
    {
        let mut h = sim.db.task_haul_data.get(&task_id).unwrap();
        h.phase = crate::task::HaulPhase::GoingToDestination;
        sim.db.update_task_haul_data(h).unwrap();
    }

    // Give elf a personally-owned spear + reserved bread.
    sim.inv_add_simple_item(
        creature_inv,
        crate::inventory::ItemKind::Spear,
        1,
        Some(elf_id),
        None,
    );
    sim.inv_add_simple_item(
        creature_inv,
        crate::inventory::ItemKind::Bread,
        5,
        None,
        Some(task_id),
    );

    // Assign task.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Drop off.
    sim.resolve_dropoff_action(elf_id);

    // Building should have exactly 5 bread.
    let building_bread: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&dest_inv, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(building_bread, 5, "Building should have 5 bread");

    // Building should have no spears.
    let building_spears: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&dest_inv, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Spear)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(building_spears, 0, "Building should have no spears");

    // Elf should still have the spear.
    let elf_spears: u32 = sim
        .db
        .item_stacks
        .by_inventory_id(&creature_inv, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.kind == crate::inventory::ItemKind::Spear)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(elf_spears, 1, "Elf should still have their spear");
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

/// Verify that set_inv_wants correctly removes and re-inserts logistics want
/// rows using the new compound PK (inventory_id, seq).
#[test]
fn logistics_want_row_remove_and_reinsert_with_compound_pk() {
    let mut sim = test_sim(legacy_test_seed());
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

// -----------------------------------------------------------------------
// AcquireItem tests
// -----------------------------------------------------------------------

#[test]
fn acquire_item_picks_up_and_owns() {
    let mut sim = test_sim(legacy_test_seed());

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
    let pile_nav_pos = find_walkable(&sim, pile_pos, 10).unwrap();
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(pile_nav_pos);
        sim.db.update_creature(c).unwrap();
    }

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
        location: pile_nav_pos,
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
        sim.db.update_creature(c).unwrap();
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
    let mut sim = test_sim(legacy_test_seed());
    // Disable hunger/tiredness so elf stays idle.
    sim.config.elf_starting_bread = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
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
    let mut sim = test_sim(legacy_test_seed());
    // Disable hunger/tiredness.
    sim.config.elf_starting_bread = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
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
fn elf_at_want_target_does_not_acquire() {
    let mut sim = test_sim(legacy_test_seed());
    // Disable hunger/tiredness.
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
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
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim
        .db
        .insert_inventory_auto(|id| crate::db::Inventory {
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
