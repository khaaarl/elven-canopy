//! Tests for the equipment system: equip slots, equipping/unequipping items,
//! auto-equip behavior, military equipment wants, equipment drops on group
//! change, and acquire-military-equipment tasks.
//! Corresponds to equipment handling in `sim/inventory_mgmt.rs`.

use super::*;

// -----------------------------------------------------------------------
// Clothing / equip system tests
// -----------------------------------------------------------------------

#[test]
fn equip_slot_mapping() {
    use inventory::EquipSlot;
    assert_eq!(inventory::ItemKind::Hat.equip_slot(), Some(EquipSlot::Head));
    assert_eq!(
        inventory::ItemKind::Tunic.equip_slot(),
        Some(EquipSlot::Torso)
    );
    assert_eq!(
        inventory::ItemKind::Leggings.equip_slot(),
        Some(EquipSlot::Legs)
    );
    assert_eq!(
        inventory::ItemKind::Boots.equip_slot(),
        Some(EquipSlot::Feet)
    );
    assert_eq!(
        inventory::ItemKind::Gloves.equip_slot(),
        Some(EquipSlot::Hands)
    );
    // Non-clothing items return None.
    assert_eq!(inventory::ItemKind::Bread.equip_slot(), None);
    assert_eq!(inventory::ItemKind::Bow.equip_slot(), None);
    assert_eq!(inventory::ItemKind::Arrow.equip_slot(), None);
    assert_eq!(inventory::ItemKind::Cloth.equip_slot(), None);
}

#[test]
fn inv_equip_item_sets_slot() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        None,
        None,
        None,
        0,
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let stack_id = stacks[0].id;

    assert!(sim.inv_equip_item(stack_id));
    let stack = sim.db.item_stacks.get(&stack_id).unwrap();
    assert_eq!(stack.equipped_slot, Some(inventory::EquipSlot::Torso));
}

#[test]
fn inv_equip_rejects_duplicate_slot() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    // Add two Tunics (qty 1 each).
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        None,
        None,
        Some(inventory::Material::Oak),
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        None,
        None,
        Some(inventory::Material::Yew),
        0,
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 2);

    assert!(sim.inv_equip_item(stacks[0].id));
    assert!(!sim.inv_equip_item(stacks[1].id), "Slot already occupied");
}

#[test]
fn inv_equip_rejects_quantity_gt_1() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        3,
        None,
        None,
        None,
        0,
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert!(
        !sim.inv_equip_item(stacks[0].id),
        "qty > 1 should be rejected"
    );
}

#[test]
fn inv_equip_rejects_non_clothing() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 1, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert!(
        !sim.inv_equip_item(stacks[0].id),
        "Non-clothing items should be rejected"
    );
}

#[test]
fn acquire_item_auto_equips_one_from_multi_qty() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);

    // Create a ground pile with 3 Hats.
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Hat, 3, None, None);
    }

    // Position elf at the pile.
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos, 10).unwrap();
    let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(pile_nav_pos);
        sim.db.update_creature(c).unwrap();
    }

    // Create AcquireItem task for 3 Hats.
    let task_id = TaskId::new(&mut sim.rng);
    let source = task::HaulSource::GroundPile(pile_pos);
    let acquire_task = Task {
        id: task_id,
        kind: TaskKind::AcquireItem {
            source,
            item_kind: inventory::ItemKind::Hat,
            quantity: 3,
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
            inventory::ItemKind::Hat,
            inventory::MaterialFilter::Any,
            3,
            task_id,
        );
    }
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.resolve_acquire_item_action(elf_id, task_id);

    // Should have 3 Hats total: 1 equipped, 2 unequipped.
    let elf_inv = sim.creature_inv(elf_id);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&elf_inv, tabulosity::QueryOpts::ASC);
    let hats: Vec<_> = stacks
        .iter()
        .filter(|s| s.kind == inventory::ItemKind::Hat)
        .collect();
    let total_qty: u32 = hats.iter().map(|s| s.quantity).sum();
    assert_eq!(total_qty, 3, "Should have 3 hats total");
    let equipped_count = hats.iter().filter(|s| s.equipped_slot.is_some()).count();
    assert_eq!(equipped_count, 1, "Exactly one hat should be equipped");
    let equipped = hats.iter().find(|s| s.equipped_slot.is_some()).unwrap();
    assert_eq!(equipped.quantity, 1, "Equipped hat should be qty 1");
}

#[test]
fn equipped_items_dont_merge() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    // Add equipped Tunic.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        None,
        None,
        None,
        0,
        None,
        Some(inventory::EquipSlot::Torso),
    );
    // Add unequipped Tunic.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        None,
        None,
        None,
        0,
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 2, "Equipped and unequipped should not merge");
    let equipped = stacks.iter().find(|s| s.equipped_slot.is_some()).unwrap();
    let unequipped = stacks.iter().find(|s| s.equipped_slot.is_none()).unwrap();
    assert_eq!(equipped.quantity, 1);
    assert_eq!(unequipped.quantity, 1);
}

#[test]
fn inv_unequip_slot_clears_and_normalizes() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    // Add equipped Tunic + unequipped Tunic (same properties).
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        None,
        None,
        None,
        0,
        None,
        Some(inventory::EquipSlot::Torso),
    );
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        None,
        None,
        None,
        0,
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 2);

    // Unequip.
    let unequipped_id = sim.inv_unequip_slot(inv_id, inventory::EquipSlot::Torso);
    assert!(unequipped_id.is_some());

    // After normalize, should merge into a single stack of qty 2.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1, "Should merge after unequip");
    assert_eq!(stacks[0].quantity, 2);
    assert_eq!(stacks[0].equipped_slot, None);
}

#[test]
fn acquire_item_preserves_material() {
    // Test that acquiring an item via resolve_acquire_item_action
    // preserves material and quality (bug fix verification).
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);

    // Create a ground pile with material-bearing Cloth.
    let mat = Some(inventory::Material::FruitSpecies(
        crate::fruit::FruitSpeciesId(0),
    ));
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_item(
            pile.inventory_id,
            inventory::ItemKind::Cloth,
            2,
            None,
            None,
            mat,
            3,
            None,
            None,
        );
    }

    // Position elf at the pile.
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos, 10).unwrap();
    let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(pile_nav_pos);
        sim.db.update_creature(c).unwrap();
    }

    // Create AcquireItem task with reservation.
    let task_id = TaskId::new(&mut sim.rng);
    let source = task::HaulSource::GroundPile(pile_pos);
    let acquire_task = Task {
        id: task_id,
        kind: TaskKind::AcquireItem {
            source,
            item_kind: inventory::ItemKind::Cloth,
            quantity: 1,
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
        sim.inv_reserve_items(
            pile.inventory_id,
            inventory::ItemKind::Cloth,
            inventory::MaterialFilter::Any,
            1,
            task_id,
        );
    }
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.resolve_acquire_item_action(elf_id, task_id);

    // Verify material/quality preserved in creature inventory.
    let elf_inv = sim.creature_inv(elf_id);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&elf_inv, tabulosity::QueryOpts::ASC);
    let cloth = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Cloth)
        .expect("Cloth should be in elf inventory");
    assert_eq!(cloth.material, mat, "Material should be preserved");
    assert_eq!(cloth.quality, 3, "Quality should be preserved");
}

#[test]
fn acquire_item_auto_equips_clothing() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);

    // Create a ground pile with a Tunic.
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Tunic, 1, None, None);
    }

    // Position elf at the pile.
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos, 10).unwrap();
    let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(pile_nav_pos);
        sim.db.update_creature(c).unwrap();
    }

    // Create AcquireItem task with reservation.
    let task_id = TaskId::new(&mut sim.rng);
    let source = task::HaulSource::GroundPile(pile_pos);
    let acquire_task = Task {
        id: task_id,
        kind: TaskKind::AcquireItem {
            source,
            item_kind: inventory::ItemKind::Tunic,
            quantity: 1,
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
            inventory::ItemKind::Tunic,
            inventory::MaterialFilter::Any,
            1,
            task_id,
        );
    }
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.resolve_acquire_item_action(elf_id, task_id);

    // Verify the Tunic is equipped in the Torso slot.
    let elf_inv = sim.creature_inv(elf_id);
    let equipped = sim.inv_equipped_in_slot(elf_inv, inventory::EquipSlot::Torso);
    assert!(equipped.is_some(), "Tunic should be auto-equipped");
    assert_eq!(equipped.unwrap().kind, inventory::ItemKind::Tunic);
}

#[test]
fn acquire_item_does_not_equip_if_slot_occupied() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);

    // Pre-equip a Tunic in the elf's inventory.
    let elf_inv = sim.creature_inv(elf_id);
    sim.inv_add_item(
        elf_inv,
        inventory::ItemKind::Tunic,
        1,
        Some(elf_id),
        None,
        None,
        0,
        None,
        Some(inventory::EquipSlot::Torso),
    );

    // Create a ground pile with another Tunic.
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Tunic, 1, None, None);
    }

    // Position elf at the pile.
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos, 10).unwrap();
    let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(pile_nav_pos);
        sim.db.update_creature(c).unwrap();
    }

    // Create AcquireItem task with reservation.
    let task_id = TaskId::new(&mut sim.rng);
    let source = task::HaulSource::GroundPile(pile_pos);
    let acquire_task = Task {
        id: task_id,
        kind: TaskKind::AcquireItem {
            source,
            item_kind: inventory::ItemKind::Tunic,
            quantity: 1,
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
            inventory::ItemKind::Tunic,
            inventory::MaterialFilter::Any,
            1,
            task_id,
        );
    }
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.resolve_acquire_item_action(elf_id, task_id);

    // Should have 2 Tunics: 1 equipped, 1 not.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&elf_inv, tabulosity::QueryOpts::ASC);
    let tunics: Vec<_> = stacks
        .iter()
        .filter(|s| s.kind == inventory::ItemKind::Tunic)
        .collect();
    assert_eq!(tunics.len(), 2, "Should have 2 separate tunic stacks");
    let equipped_count = tunics.iter().filter(|s| s.equipped_slot.is_some()).count();
    assert_eq!(equipped_count, 1, "Only one should be equipped");
}

#[test]
fn serde_roundtrip_equipped_slot() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Hat,
        1,
        None,
        None,
        None,
        0,
        None,
        Some(inventory::EquipSlot::Head),
    );

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();
    let stacks = restored
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].equipped_slot, Some(inventory::EquipSlot::Head));
}

#[test]
fn serde_backward_compat_no_equipped_slot() {
    // Simulate old save format: ItemStack JSON without equipped_slot field.
    let json = r#"{"id":1,"inventory_id":1,"kind":"Hat","quantity":1,"material":null,"quality":0,"enchantment_id":null,"owner":null,"reserved_by":null}"#;
    let stack: crate::db::ItemStack = serde_json::from_str(json).unwrap();
    assert_eq!(stack.equipped_slot, None);
}

#[test]
fn clothing_wants_in_default_config() {
    let config = crate::config::GameConfig::default();
    let wants = &config.elf_default_wants;
    assert!(wants.len() >= 6, "Should have bread + 5 clothing wants");

    let has_tunic = wants
        .iter()
        .any(|w| w.item_kind == inventory::ItemKind::Tunic);
    let has_leggings = wants
        .iter()
        .any(|w| w.item_kind == inventory::ItemKind::Leggings);
    let has_shoes = wants
        .iter()
        .any(|w| w.item_kind == inventory::ItemKind::Shoes);
    let has_hat = wants
        .iter()
        .any(|w| w.item_kind == inventory::ItemKind::Hat);
    let has_gloves = wants
        .iter()
        .any(|w| w.item_kind == inventory::ItemKind::Gloves);
    assert!(has_tunic, "Should want Tunic");
    assert!(has_leggings, "Should want Leggings");
    assert!(has_shoes, "Should want Shoes");
    assert!(has_hat, "Should want Hat");
    assert!(has_gloves, "Should want Gloves");

    // Shoes want uses Any filter — no need for NonWood workaround since
    // boots are now a separate armor-only item kind.
    let shoes_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Shoes)
        .unwrap();
    assert_eq!(
        shoes_want.material_filter,
        inventory::MaterialFilter::Any,
        "Shoes want should use Any filter"
    );
}

// =========================================================================
// F-military-equip — Military group equipment acquisition
// =========================================================================

#[test]
fn default_soldiers_group_has_equipment_wants() {
    let sim = test_sim(legacy_test_seed());
    let soldiers = soldiers_group(&sim);
    assert_eq!(
        soldiers.equipment_wants.len(),
        2,
        "Soldiers should want bow + arrows"
    );
    assert_eq!(
        soldiers.equipment_wants[0].item_kind,
        inventory::ItemKind::Bow
    );
    assert_eq!(soldiers.equipment_wants[0].target_quantity, 1);
    assert_eq!(
        soldiers.equipment_wants[1].item_kind,
        inventory::ItemKind::Arrow
    );
    assert_eq!(soldiers.equipment_wants[1].target_quantity, 20);
}

#[test]
fn default_civilians_group_has_no_equipment_wants() {
    let sim = test_sim(legacy_test_seed());
    let civilians = civilian_group(&sim);
    assert!(
        civilians.equipment_wants.is_empty(),
        "Civilians should have no equipment wants"
    );
}

#[test]
fn new_military_group_has_empty_equipment_wants() {
    let mut sim = test_sim(legacy_test_seed());
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::CreateMilitaryGroup {
            name: "Rangers".to_string(),
        },
    };
    sim.step(&[cmd], sim.tick + 2);
    let civ_id = sim.player_civ_id.unwrap();
    let rangers = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| g.name == "Rangers")
        .expect("Rangers group should exist");
    assert!(
        rangers.equipment_wants.is_empty(),
        "Newly created group should have no equipment wants"
    );
}

#[test]
fn set_group_equipment_wants_command() {
    let mut sim = test_sim(legacy_test_seed());
    let soldiers = soldiers_group(&sim);
    let new_wants = vec![crate::building::LogisticsWant {
        item_kind: inventory::ItemKind::Arrow,
        material_filter: inventory::MaterialFilter::Any,
        target_quantity: 50,
    }];
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetGroupEquipmentWants {
            group_id: soldiers.id,
            wants: new_wants.clone(),
        },
    };
    sim.step(&[cmd], sim.tick + 2);
    let updated = sim.db.military_groups.get(&soldiers.id).unwrap();
    assert_eq!(updated.equipment_wants.len(), 1);
    assert_eq!(
        updated.equipment_wants[0].item_kind,
        inventory::ItemKind::Arrow
    );
    assert_eq!(updated.equipment_wants[0].target_quantity, 50);
}

#[test]
fn set_group_equipment_wants_rejects_non_player_civ() {
    let mut sim = test_sim(legacy_test_seed());
    // Find an AI civ's group.
    let ai_group = sim
        .db
        .military_groups
        .iter_all()
        .find(|g| g.civ_id != sim.player_civ_id.unwrap())
        .unwrap()
        .clone();
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetGroupEquipmentWants {
            group_id: ai_group.id,
            wants: vec![crate::building::LogisticsWant {
                item_kind: inventory::ItemKind::Bow,
                material_filter: inventory::MaterialFilter::Any,
                target_quantity: 5,
            }],
        },
    };
    sim.step(&[cmd], sim.tick + 2);
    let still_same = sim.db.military_groups.get(&ai_group.id).unwrap();
    assert!(
        still_same.equipment_wants.is_empty(),
        "AI group wants should not be changed by player command"
    );
}

#[test]
fn soldier_acquires_military_equipment_no_ownership_change() {
    let mut sim = test_sim(legacy_test_seed());

    // Disable hunger/tiredness so elf stays idle.
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    // Create a ground pile with an unowned bow.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bow, 1, None, None);
    }

    // Spawn elf, assign to soldiers, position at pile.
    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos, 10).unwrap();
    let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(pile_nav_pos);
        sim.db.update_creature(c).unwrap();
    }

    // Create AcquireMilitaryEquipment task with reservations.
    let task_id = TaskId::new(&mut sim.rng);
    let source = task::HaulSource::GroundPile(pile_pos);
    let acquire_task = Task {
        id: task_id,
        kind: TaskKind::AcquireMilitaryEquipment {
            source,
            item_kind: inventory::ItemKind::Bow,
            quantity: 1,
        },
        state: TaskState::InProgress,
        location: sim.nav_graph.node(pile_nav).position,
        progress: 0,
        total_cost: 0,
        required_species: None,
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
            inventory::ItemKind::Bow,
            inventory::MaterialFilter::Any,
            1,
            task_id,
        );
    }
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Execute.
    sim.resolve_acquire_military_equipment_action(elf_id, task_id);

    // Assert: bow is in elf's inventory but NOT owned by elf.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let owned_bows = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Bow, elf_id);
    assert_eq!(
        owned_bows, 0,
        "Military-acquired bow should NOT be owned by elf"
    );

    let total_bows = sim.inv_count_owned_or_unowned(
        elf.inventory_id,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
        elf_id,
    );
    assert_eq!(
        total_bows, 1,
        "Elf should have 1 bow in inventory (unowned)"
    );

    // Assert: task completed.
    assert_eq!(
        sim.db.tasks.get(&task_id).unwrap().state,
        TaskState::Complete
    );
}

#[test]
fn soldier_with_owned_bow_satisfies_equipment_want() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Give elf an owned bow.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    sim.inv_add_simple_item(
        elf.inventory_id,
        inventory::ItemKind::Bow,
        1,
        Some(elf_id),
        None,
    );

    // Check: should NOT create a task since bow want is satisfied.
    sim.check_military_equipment_wants(elf_id);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    // Elf may have a task for arrows (which are unsatisfied), but NOT for bow.
    if let Some(task_id) = elf.current_task {
        let task = sim.db.tasks.get(&task_id).unwrap();
        if task.kind_tag == crate::db::TaskKindTag::AcquireMilitaryEquipment {
            let acquire = sim.task_acquire_data(task_id).unwrap();
            assert_ne!(
                acquire.item_kind,
                inventory::ItemKind::Bow,
                "Should not try to acquire bow when already have one"
            );
        }
    }
}

#[test]
fn soldier_with_unowned_bow_satisfies_equipment_want() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Give elf an unowned bow (in inventory but owner = None).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    sim.inv_add_simple_item(elf.inventory_id, inventory::ItemKind::Bow, 1, None, None);

    // Check: bow want is satisfied (unowned counts).
    sim.check_military_equipment_wants(elf_id);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    if let Some(task_id) = elf.current_task {
        let task = sim.db.tasks.get(&task_id).unwrap();
        if task.kind_tag == crate::db::TaskKindTag::AcquireMilitaryEquipment {
            let acquire = sim.task_acquire_data(task_id).unwrap();
            assert_ne!(
                acquire.item_kind,
                inventory::ItemKind::Bow,
                "Should not try to acquire bow when already have unowned one"
            );
        }
    }
}

#[test]
fn non_soldier_no_military_equipment_acquisition() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    // Elf is NOT assigned to any military group (civilian by default).
    sim.check_military_equipment_wants(elf_id);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_none(),
        "Civilian elf should not get military equipment task"
    );
}

#[test]
fn soldier_in_group_with_empty_wants_no_acquisition() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    // Create a group with no equipment wants.
    let mut events = Vec::new();
    sim.create_military_group("Empty Squad".to_string(), &mut events);
    let civ_id = sim.player_civ_id.unwrap();
    let empty_squad = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| g.name == "Empty Squad")
        .unwrap();

    let elf_id = spawn_elf(&mut sim);
    set_military_group(&mut sim, elf_id, Some(empty_squad.id));

    sim.check_military_equipment_wants(elf_id);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_none(),
        "Elf in group with empty wants should not acquire"
    );
}

#[test]
fn military_equipment_drop_when_leaving_group() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Give elf an unowned bow (military equipment).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    sim.inv_add_simple_item(elf.inventory_id, inventory::ItemKind::Bow, 1, None, None);

    // Verify elf has bow.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let bows_before = sim.inv_count_owned_or_unowned(
        elf.inventory_id,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
        elf_id,
    );
    assert_eq!(bows_before, 1);

    // Remove from military group (back to civilian).
    set_military_group(&mut sim, elf_id, None);

    // Run drop phase.
    sim.military_equipment_drop(elf_id);

    // Elf should have dropped the unowned bow.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let bows_after = sim.inv_count_owned_or_unowned(
        elf.inventory_id,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
        elf_id,
    );
    assert_eq!(
        bows_after, 0,
        "Unowned bow should be dropped when leaving military group"
    );
}

#[test]
fn military_equipment_drop_when_wants_change() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Give elf an unowned bow.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    sim.inv_add_simple_item(elf.inventory_id, inventory::ItemKind::Bow, 1, None, None);

    // Change soldiers group wants to only arrows (remove bow).
    {
        let mut g = sim.db.military_groups.get(&soldiers.id).unwrap();
        g.equipment_wants = vec![crate::building::LogisticsWant {
            item_kind: inventory::ItemKind::Arrow,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 20,
        }];
        sim.db.update_military_group(g).unwrap();
    }

    // Run drop phase.
    sim.military_equipment_drop(elf_id);

    // Elf should have dropped the unowned bow (no longer wanted).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let bows = sim.inv_count_owned_or_unowned(
        elf.inventory_id,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
        elf_id,
    );
    assert_eq!(
        bows, 0,
        "Unowned bow should be dropped when group no longer wants it"
    );
}

#[test]
fn military_equipment_drop_does_not_drop_owned_items() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    // Elf is civilian (no military group).

    // Give elf an OWNED bow (personal property).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    sim.inv_add_simple_item(
        elf.inventory_id,
        inventory::ItemKind::Bow,
        1,
        Some(elf_id),
        None,
    );

    // Run drop phase.
    sim.military_equipment_drop(elf_id);

    // Elf should NOT have dropped the owned bow.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let bows = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Bow, elf_id);
    assert_eq!(bows, 1, "Owned bow should NOT be dropped");
}

#[test]
fn military_equipment_drop_keeps_wanted_unowned_items() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Give elf an unowned bow that satisfies the soldiers' want.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    sim.inv_add_simple_item(elf.inventory_id, inventory::ItemKind::Bow, 1, None, None);

    // Run drop phase.
    sim.military_equipment_drop(elf_id);

    // Elf should keep the bow (it satisfies military want).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let bows = sim.inv_count_owned_or_unowned(
        elf.inventory_id,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
        elf_id,
    );
    assert_eq!(
        bows, 1,
        "Unowned bow satisfying military want should be kept"
    );
}

#[test]
fn military_equipment_drop_drops_other_owned_creature_items() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);

    // Give elf_a an item owned by elf_b.
    let elf_a_inv = sim.db.creatures.get(&elf_a).unwrap().inventory_id;
    sim.inv_add_simple_item(elf_a_inv, inventory::ItemKind::Bow, 1, Some(elf_b), None);

    // Run drop phase on elf_a.
    sim.military_equipment_drop(elf_a);

    // Elf_a should have dropped the bow (owned by someone else).
    let bows = sim.inv_count_owned_or_unowned(
        elf_a_inv,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
        elf_a,
    );
    assert_eq!(bows, 0, "Item owned by another creature should be dropped");

    // Verify the dropped bow is now unowned in a ground pile (not stuck as
    // owned by elf_b forever).
    let elf_a_pos = sim.db.creatures.get(&elf_a).unwrap().position.min;
    if let Some(pile) = sim
        .db
        .ground_piles
        .by_position(&elf_a_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
    {
        let unowned = sim.inv_count_unowned_unreserved(
            pile.inventory_id,
            inventory::ItemKind::Bow,
            inventory::MaterialFilter::Any,
        );
        assert_eq!(
            unowned, 1,
            "Dropped bow should be unowned in ground pile (owner cleared)"
        );
    }
}

#[test]
fn military_equipment_drop_excess_unowned() {
    let mut sim = test_sim(legacy_test_seed());

    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Soldiers want 1 bow. Give elf 3 unowned bows.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    sim.inv_add_simple_item(elf.inventory_id, inventory::ItemKind::Bow, 3, None, None);

    // Run drop phase.
    sim.military_equipment_drop(elf_id);

    // Elf should keep 1, drop 2.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let bows = sim.inv_count_owned_or_unowned(
        elf.inventory_id,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
        elf_id,
    );
    assert_eq!(bows, 1, "Should keep only 1 bow (want target), drop excess");
}

#[test]
fn military_group_equipment_wants_serde_roundtrip() {
    use crate::building::LogisticsWant;
    let mut sim = test_sim(legacy_test_seed());
    let soldiers = soldiers_group(&sim);

    // Set up some wants.
    {
        let mut g = sim.db.military_groups.get(&soldiers.id).unwrap();
        g.equipment_wants = vec![
            LogisticsWant {
                item_kind: inventory::ItemKind::Bow,
                material_filter: inventory::MaterialFilter::Any,
                target_quantity: 2,
            },
            LogisticsWant {
                item_kind: inventory::ItemKind::Arrow,
                material_filter: inventory::MaterialFilter::Specific(inventory::Material::Oak),
                target_quantity: 30,
            },
        ];
        sim.db.update_military_group(g).unwrap();
    }

    // Serialize and deserialize.
    let json = serde_json::to_string(&sim.db.military_groups.get(&soldiers.id).unwrap()).unwrap();
    let deserialized: crate::db::MilitaryGroup = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.equipment_wants.len(), 2);
    assert_eq!(
        deserialized.equipment_wants[0].item_kind,
        inventory::ItemKind::Bow
    );
    assert_eq!(deserialized.equipment_wants[0].target_quantity, 2);
    assert_eq!(
        deserialized.equipment_wants[1].item_kind,
        inventory::ItemKind::Arrow
    );
    assert_eq!(
        deserialized.equipment_wants[1].material_filter,
        inventory::MaterialFilter::Specific(inventory::Material::Oak)
    );
    assert_eq!(deserialized.equipment_wants[1].target_quantity, 30);
}

#[test]
fn heartbeat_military_equip_creates_task_for_missing_bow() {
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

    // Create a ground pile with a bow.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bow, 1, None, None);
    }

    // Spawn elf, assign to soldiers.
    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Elf should be idle.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_none(),
        "Elf should be idle before check"
    );

    // Check military equipment wants.
    sim.check_military_equipment_wants(elf_id);

    // Elf should now have an AcquireMilitaryEquipment task.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Elf should have a task after check"
    );
    let task_id = elf.current_task.unwrap();
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.kind_tag,
        crate::db::TaskKindTag::AcquireMilitaryEquipment,
        "Task should be AcquireMilitaryEquipment"
    );
    let acquire_data = sim.task_acquire_data(task_id).unwrap();
    assert_eq!(acquire_data.item_kind, inventory::ItemKind::Bow);
}

#[test]
fn elves_spawn_without_bows_and_arrows() {
    let mut sim = SimState::with_config(legacy_test_seed(), test_config());
    // The default config gives 0 starting bows/arrows.
    assert_eq!(sim.config.elf_starting_bows, 0);
    assert_eq!(sim.config.elf_starting_arrows, 0);

    // Spawn an elf and verify no bows/arrows.
    let elf_id = spawn_elf(&mut sim);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let bows = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Bow, elf_id);
    let arrows = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Arrow, elf_id);
    assert_eq!(bows, 0, "Elf should not start with bows");
    assert_eq!(arrows, 0, "Elf should not start with arrows");
}

#[test]
fn military_equipment_drop_does_not_drop_equipped_items() {
    let mut sim = test_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    // Elf is civilian — no military wants.

    // Give elf an unowned tunic and equip it.
    let elf_inv = sim.db.creatures.get(&elf_id).unwrap().inventory_id;
    sim.inv_add_simple_item(elf_inv, inventory::ItemKind::Tunic, 1, None, None);
    // Equip the tunic.
    let mut stack = sim
        .db
        .item_stacks
        .by_inventory_id(&elf_inv, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == inventory::ItemKind::Tunic)
        .unwrap();
    stack.equipped_slot = Some(inventory::EquipSlot::Torso);
    sim.db.update_item_stack(stack).unwrap();

    // Run drop phase — equipped items should be kept even if unowned.
    sim.military_equipment_drop(elf_id);

    let tunics = sim.inv_item_count(
        elf_inv,
        inventory::ItemKind::Tunic,
        inventory::MaterialFilter::Any,
    );
    assert_eq!(tunics, 1, "Equipped unowned tunic should NOT be dropped");
}

#[test]
fn military_equipment_drop_does_not_drop_task_reserved_items() {
    let mut sim = test_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    // No military group — civilian.

    // Give elf an unowned bow.
    let elf_inv = sim.db.creatures.get(&elf_id).unwrap().inventory_id;
    sim.inv_add_simple_item(elf_inv, inventory::ItemKind::Bow, 1, None, None);

    // Create a fake task and reserve the bow for it, assign task to elf.
    let task_id = TaskId::new(&mut sim.rng);
    insert_stub_task(&mut sim, task_id);
    let mut stack = sim
        .db
        .item_stacks
        .by_inventory_id(&elf_inv, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == inventory::ItemKind::Bow)
        .unwrap();
    stack.reserved_by = Some(task_id);
    sim.db.update_item_stack(stack).unwrap();
    let mut creature = sim.db.creatures.get(&elf_id).unwrap();
    creature.current_task = Some(task_id);
    sim.db.update_creature(creature).unwrap();

    // Run drop phase — reserved items for current task should be kept.
    sim.military_equipment_drop(elf_id);

    let bows = sim.inv_item_count(
        elf_inv,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
    );
    assert_eq!(
        bows, 1,
        "Bow reserved by elf's current task should NOT be dropped"
    );
}

#[test]
fn military_equipment_drop_respects_material_filter() {
    let mut sim = test_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Change soldiers to want specifically Oak bows.
    {
        let mut g = sim.db.military_groups.get(&soldiers.id).unwrap();
        g.equipment_wants = vec![crate::building::LogisticsWant {
            item_kind: inventory::ItemKind::Bow,
            material_filter: inventory::MaterialFilter::Specific(inventory::Material::Oak),
            target_quantity: 1,
        }];
        sim.db.update_military_group(g).unwrap();
    }

    let elf_inv = sim.db.creatures.get(&elf_id).unwrap().inventory_id;
    // Give elf an unowned non-Oak bow (material = None, doesn't match Specific(Oak)).
    sim.inv_add_simple_item(elf_inv, inventory::ItemKind::Bow, 1, None, None);

    // Run drop phase — non-Oak bow doesn't match the material filter.
    sim.military_equipment_drop(elf_id);

    let bows = sim.inv_item_count(
        elf_inv,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
    );
    assert_eq!(
        bows, 0,
        "Non-Oak bow should be dropped when group wants Specific(Oak)"
    );
}

#[test]
fn check_military_equipment_wants_overwrites_task_on_non_idle_elf() {
    // check_military_equipment_wants does NOT internally check idle status —
    // the heartbeat gates it with a still_idle check before calling. Verify
    // that the function CAN overwrite an existing task when called directly,
    // which confirms that the heartbeat's idle gate is load-bearing.
    let mut sim = test_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    // Create a ground pile with a bow.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bow, 1, None, None);
    }

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Give elf a GoTo task (non-idle).
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos, 10).unwrap();
    let goto_task_id = insert_goto_task(&mut sim, pile_nav);
    let mut creature = sim.db.creatures.get(&elf_id).unwrap();
    creature.current_task = Some(goto_task_id);
    sim.db.update_creature(creature).unwrap();

    // Call directly (bypassing heartbeat idle gate) — it will overwrite.
    sim.check_military_equipment_wants(elf_id);

    // The function overwrote the task — proving the heartbeat's idle gate matters.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(elf.current_task.is_some(), "Elf should have a task");
    assert_ne!(
        elf.current_task,
        Some(goto_task_id),
        "check_military_equipment_wants should overwrite when called without idle gate"
    );
}

#[test]
fn military_equip_phase_runs_before_personal_wants() {
    // When a soldier is idle and missing both military equipment and personal
    // wants, running both phases in sequence should result in the military
    // equipment task (Phase 2b¾ runs first, makes elf non-idle, Phase 2c
    // is skipped).
    let mut sim = test_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    // Create piles with both a bow and bread (for personal wants).
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bow, 1, None, None);
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bread, 5, None, None);
    }

    // Spawn elf with 0 bread (so it has a personal want for bread).
    sim.config.elf_starting_bread = 0;
    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Run Phase 2b¾ (military equip) then Phase 2c (personal wants),
    // matching the heartbeat's actual order.
    sim.check_military_equipment_wants(elf_id);

    // Phase 2c: only runs if still idle.
    let still_idle = sim
        .db
        .creatures
        .get(&elf_id)
        .is_some_and(|c| c.current_task.is_none());
    if still_idle {
        sim.check_creature_wants(elf_id);
    }

    // Elf should have a military equipment task, not a personal bread task.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(elf.current_task.is_some(), "Elf should have a task");
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert_eq!(
        task.kind_tag,
        crate::db::TaskKindTag::AcquireMilitaryEquipment,
        "Military equip (Phase 2b¾) should preempt personal wants (Phase 2c)"
    );
    // Confirm that if the elf were idle, it WOULD have gotten a bread task.
    assert!(
        !still_idle,
        "Phase 2b¾ should have made elf non-idle, preventing Phase 2c"
    );
}

#[test]
fn cleanup_acquire_military_equipment_clears_reservations() {
    let mut sim = test_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    // Create a ground pile with a bow.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    let pile_inv;
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        pile_inv = pile.inventory_id;
        sim.inv_add_simple_item(pile_inv, inventory::ItemKind::Bow, 1, None, None);
    }

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Create task before reserving so the task row exists for FK validation.
    let task_id = TaskId::new(&mut sim.rng);
    let task = Task {
        id: task_id,
        kind: TaskKind::AcquireMilitaryEquipment {
            source: task::HaulSource::GroundPile(pile_pos),
            item_kind: inventory::ItemKind::Bow,
            quantity: 1,
        },
        state: TaskState::InProgress,
        location: pile_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(task);

    sim.inv_reserve_unowned_items(
        pile_inv,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
        1,
        task_id,
    );

    // Verify bow is reserved.
    let bow_stack = sim
        .db
        .item_stacks
        .by_inventory_id(&pile_inv, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == inventory::ItemKind::Bow)
        .unwrap();
    assert!(
        bow_stack.reserved_by.is_some(),
        "Bow should be reserved before cleanup"
    );

    // Cleanup the task (simulating abandonment).
    sim.cleanup_acquire_military_equipment_task(task_id);

    // Reservation should be cleared.
    let bow_stack_after = sim
        .db
        .item_stacks
        .by_inventory_id(&pile_inv, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == inventory::ItemKind::Bow)
        .unwrap();
    assert!(
        bow_stack_after.reserved_by.is_none(),
        "Bow reservation should be cleared after task cleanup"
    );
}

#[test]
fn acquire_military_equipment_task_serde_roundtrip() {
    let mut rng = GameRng::new(42);
    let task_id = TaskId::new(&mut rng);
    let location = VoxelCoord::new(5, 0, 0);
    let pile_pos = VoxelCoord::new(128, 1, 138);

    let task = Task {
        id: task_id,
        kind: TaskKind::AcquireMilitaryEquipment {
            source: task::HaulSource::GroundPile(pile_pos),
            item_kind: inventory::ItemKind::Bow,
            quantity: 1,
        },
        state: TaskState::InProgress,
        location,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };

    let json = serde_json::to_string(&task).unwrap();
    let restored: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, task_id);
    match &restored.kind {
        TaskKind::AcquireMilitaryEquipment {
            source,
            item_kind,
            quantity,
        } => {
            assert_eq!(*source, task::HaulSource::GroundPile(pile_pos));
            assert_eq!(*item_kind, inventory::ItemKind::Bow);
            assert_eq!(*quantity, 1);
        }
        other => panic!("Expected AcquireMilitaryEquipment, got {:?}", other),
    }
    assert_eq!(restored.state, TaskState::InProgress);
    assert_eq!(restored.origin, TaskOrigin::Autonomous);
}

#[test]
fn military_group_equipment_wants_serde_backward_compat() {
    // Old saves without the `equipment_wants` field should deserialize
    // with an empty vec (via #[serde(default)]).
    let json = r#"{
        "id": 1,
        "civ_id": 1,
        "name": "Soldiers",
        "is_default_civilian": false,
        "engagement_style": {
            "weapon_preference": "PreferRanged",
            "ammo_exhausted": "SwitchToMelee",
            "initiative": "Aggressive",
            "disengage_threshold_pct": 0
        }
    }"#;
    let group: crate::db::MilitaryGroup = serde_json::from_str(json).unwrap();
    assert!(
        group.equipment_wants.is_empty(),
        "Missing equipment_wants field should default to empty vec"
    );
}

// ---------------------------------------------------------------------------
// F-military-armor: auto-equip on military pickup, duplicate slot validation
// ---------------------------------------------------------------------------

#[test]
fn set_group_equipment_wants_rejects_duplicate_equip_slot() {
    let mut sim = test_sim(legacy_test_seed());
    let soldiers = soldiers_group(&sim);
    let initial_notif_count = sim.db.notifications.len();

    // Try to assign both Hat (Head) and Helmet (Head) — same slot.
    let wants = vec![
        crate::building::LogisticsWant {
            item_kind: inventory::ItemKind::Hat,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 1,
        },
        crate::building::LogisticsWant {
            item_kind: inventory::ItemKind::Helmet,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 1,
        },
    ];
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetGroupEquipmentWants {
            group_id: soldiers.id,
            wants,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Wants should NOT have been updated — should still be the original
    // soldiers defaults (Bow + Arrow), not the Hat + Helmet we tried to set.
    let group = sim.db.military_groups.get(&soldiers.id).unwrap();
    assert!(
        !group
            .equipment_wants
            .iter()
            .any(|w| w.item_kind == inventory::ItemKind::Hat),
        "Hat should not appear in wants — duplicate slot should be rejected"
    );

    // A notification should explain the rejection.
    assert!(
        sim.db.notifications.len() > initial_notif_count,
        "Should create a notification for duplicate slot rejection"
    );
    let notif = sim.db.notifications.iter_all().last().unwrap();
    assert!(
        notif.message.contains("Head"),
        "Notification should mention the conflicting slot, got: {}",
        notif.message
    );
}

#[test]
fn set_group_equipment_wants_allows_different_slots() {
    let mut sim = test_sim(legacy_test_seed());
    let soldiers = soldiers_group(&sim);

    // Helmet (Head) and Breastplate (Torso) — different slots, should succeed.
    let wants = vec![
        crate::building::LogisticsWant {
            item_kind: inventory::ItemKind::Helmet,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 1,
        },
        crate::building::LogisticsWant {
            item_kind: inventory::ItemKind::Breastplate,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 1,
        },
    ];
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetGroupEquipmentWants {
            group_id: soldiers.id,
            wants,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    let group = sim.db.military_groups.get(&soldiers.id).unwrap();
    assert_eq!(group.equipment_wants.len(), 2);
}

#[test]
fn set_group_equipment_wants_allows_non_wearable_duplicates() {
    let mut sim = test_sim(legacy_test_seed());
    let soldiers = soldiers_group(&sim);

    // Bow and Arrow — neither is wearable, no slot conflict possible.
    let wants = vec![
        crate::building::LogisticsWant {
            item_kind: inventory::ItemKind::Bow,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 1,
        },
        crate::building::LogisticsWant {
            item_kind: inventory::ItemKind::Arrow,
            material_filter: inventory::MaterialFilter::Any,
            target_quantity: 20,
        },
    ];
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetGroupEquipmentWants {
            group_id: soldiers.id,
            wants,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    let group = sim.db.military_groups.get(&soldiers.id).unwrap();
    assert_eq!(group.equipment_wants.len(), 2);
}

#[test]
fn military_equipment_auto_equips_wearable_on_pickup() {
    let mut sim = test_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    // Place a helmet on the ground.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_item(
            pile.inventory_id,
            inventory::ItemKind::Helmet,
            1,
            None,
            None,
            Some(inventory::Material::Oak),
            0,
            None,
            None,
        );
    }

    // Spawn elf, assign to soldiers, position at pile.
    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos, 10).unwrap();
    let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(pile_nav_pos);
        sim.db.update_creature(c).unwrap();
    }

    // Create AcquireMilitaryEquipment task before reserving.
    let task_id = TaskId::new(&mut sim.rng);
    let acquire_task = Task {
        id: task_id,
        kind: TaskKind::AcquireMilitaryEquipment {
            source: task::HaulSource::GroundPile(pile_pos),
            item_kind: inventory::ItemKind::Helmet,
            quantity: 1,
        },
        state: TaskState::InProgress,
        location: sim.nav_graph.node(pile_nav).position,
        progress: 0,
        total_cost: 0,
        required_species: None,
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
            inventory::ItemKind::Helmet,
            inventory::MaterialFilter::Any,
            1,
            task_id,
        );
    }
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.resolve_acquire_military_equipment_action(elf_id, task_id);

    // Helmet should be in elf's inventory AND equipped.
    let creature_inv = sim.creature_inv(elf_id);
    let equipped = sim.inv_equipped_in_slot(creature_inv, inventory::EquipSlot::Head);
    assert!(
        equipped.is_some(),
        "Helmet should be auto-equipped in Head slot"
    );
    let stack = equipped.unwrap();
    assert_eq!(stack.kind, inventory::ItemKind::Helmet);
    assert_eq!(stack.material, Some(inventory::Material::Oak));
}

#[test]
fn military_equipment_auto_equip_displaces_existing_clothing() {
    let mut sim = test_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    // Place a helmet on the ground.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_item(
            pile.inventory_id,
            inventory::ItemKind::Helmet,
            1,
            None,
            None,
            Some(inventory::Material::Oak),
            0,
            None,
            None,
        );
    }

    // Spawn elf with a hat already equipped.
    let elf_id = spawn_elf(&mut sim);
    let creature_inv = sim.creature_inv(elf_id);
    sim.inv_add_simple_item(creature_inv, inventory::ItemKind::Hat, 1, None, None);
    {
        let hat_stack = sim
            .db
            .item_stacks
            .by_inventory_id(&creature_inv, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|s| s.kind == inventory::ItemKind::Hat)
            .unwrap();
        sim.inv_equip_item(hat_stack.id);
    }
    // Verify hat is equipped.
    assert!(
        sim.inv_equipped_in_slot(creature_inv, inventory::EquipSlot::Head)
            .is_some(),
        "Hat should be equipped before test"
    );

    // Assign to soldiers, position at pile.
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos, 10).unwrap();
    let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(pile_nav_pos);
        sim.db.update_creature(c).unwrap();
    }

    // Create AcquireMilitaryEquipment task before reserving.
    let task_id = TaskId::new(&mut sim.rng);
    let acquire_task = Task {
        id: task_id,
        kind: TaskKind::AcquireMilitaryEquipment {
            source: task::HaulSource::GroundPile(pile_pos),
            item_kind: inventory::ItemKind::Helmet,
            quantity: 1,
        },
        state: TaskState::InProgress,
        location: sim.nav_graph.node(pile_nav).position,
        progress: 0,
        total_cost: 0,
        required_species: None,
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
            inventory::ItemKind::Helmet,
            inventory::MaterialFilter::Any,
            1,
            task_id,
        );
    }
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.resolve_acquire_military_equipment_action(elf_id, task_id);

    // Helmet should now be equipped, displacing the hat.
    let equipped = sim.inv_equipped_in_slot(creature_inv, inventory::EquipSlot::Head);
    assert!(equipped.is_some(), "Head slot should still be occupied");
    let stack = equipped.unwrap();
    assert_eq!(
        stack.kind,
        inventory::ItemKind::Helmet,
        "Helmet should have displaced the hat"
    );

    // Hat should still be in inventory but NOT equipped.
    let all_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&creature_inv, tabulosity::QueryOpts::ASC);
    let hat_stacks: Vec<_> = all_stacks
        .iter()
        .filter(|s| s.kind == inventory::ItemKind::Hat)
        .collect();
    assert_eq!(hat_stacks.len(), 1, "Hat should still be in inventory");
    assert!(
        hat_stacks[0].equipped_slot.is_none(),
        "Hat should be unequipped after displacement"
    );
}

#[test]
fn military_equipment_non_wearable_not_equipped() {
    let mut sim = test_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    // Place a bow on the ground.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    {
        let pile_id = sim.ensure_ground_pile(pile_pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bow, 1, None, None);
    }

    let elf_id = spawn_elf(&mut sim);
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));
    let pile_nav = sim.nav_graph.find_nearest_node(pile_pos, 10).unwrap();
    let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(pile_nav_pos);
        sim.db.update_creature(c).unwrap();
    }

    let task_id = TaskId::new(&mut sim.rng);
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
            inventory::ItemKind::Bow,
            inventory::MaterialFilter::Any,
            1,
            task_id,
        );
    }
    let acquire_task = Task {
        id: task_id,
        kind: TaskKind::AcquireMilitaryEquipment {
            source: task::HaulSource::GroundPile(pile_pos),
            item_kind: inventory::ItemKind::Bow,
            quantity: 1,
        },
        state: TaskState::InProgress,
        location: sim.nav_graph.node(pile_nav).position,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(acquire_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.resolve_acquire_military_equipment_action(elf_id, task_id);

    // Bow is not wearable — no equip slots should be occupied.
    let creature_inv = sim.creature_inv(elf_id);
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        assert!(
            sim.inv_equipped_in_slot(creature_inv, slot).is_none(),
            "No slot should be equipped after picking up a non-wearable"
        );
    }
}

#[test]
fn inv_force_equip_item_displaces_existing() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let inv_id = sim.creature_inv(elf_id);

    // Add and equip a hat.
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Hat, 1, None, None);
    let hat_id = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == inventory::ItemKind::Hat)
        .unwrap()
        .id;
    assert!(sim.inv_equip_item(hat_id));

    // Add a helmet and force-equip it.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Helmet,
        1,
        None,
        None,
        Some(inventory::Material::Oak),
        0,
        None,
        None,
    );
    let helmet_id = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == inventory::ItemKind::Helmet)
        .unwrap()
        .id;
    assert!(sim.inv_force_equip_item(helmet_id));

    // Helmet should be equipped, hat should not.
    let equipped = sim.inv_equipped_in_slot(inv_id, inventory::EquipSlot::Head);
    assert!(equipped.is_some());
    assert_eq!(equipped.unwrap().kind, inventory::ItemKind::Helmet);

    // Hat should be in inventory but unequipped.
    // (Search by kind — hat_id may have been merged by normalize.)
    let hats: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.kind == inventory::ItemKind::Hat)
        .collect();
    assert_eq!(hats.len(), 1);
    assert!(hats[0].equipped_slot.is_none());
}

#[test]
fn inv_force_equip_rejects_non_wearable() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let inv_id = sim.creature_inv(elf_id);

    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bow, 1, None, None);
    let bow_id = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == inventory::ItemKind::Bow)
        .unwrap()
        .id;
    assert!(!sim.inv_force_equip_item(bow_id));
}

/// Spear and Club have no equip slot (no dedicated weapon slot yet).
#[test]
fn weapons_have_no_equip_slot() {
    assert!(ItemKind::Spear.equip_slot().is_none());
    assert!(ItemKind::Club.equip_slot().is_none());
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

// -----------------------------------------------------------------------
// Caller durability-preservation tests (expose existing bugs)
// -----------------------------------------------------------------------

#[test]
fn creature_death_drops_preserve_durability() {
    // When a creature dies, its items should drop on the ground
    // with all properties (durability, material, quality) preserved.
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
