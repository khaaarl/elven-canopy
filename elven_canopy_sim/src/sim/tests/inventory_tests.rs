//! Tests for the inventory system: item stacking, ground piles (gravity, merging),
//! durability (damage, breakage, serde), item colors/dye, display names,
//! materials, quality labels, and arrow impact damage.

use super::*;

// ---------------------------------------------------------------------------
// Inventory integration tests
// ---------------------------------------------------------------------------

#[test]
fn elf_spawns_with_starting_items() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let inv_id = sim.creature_inv(elf_id);
    // Bread.
    let bread_count = sim.inv_count_owned(inv_id, inventory::ItemKind::Bread, elf_id);
    assert_eq!(
        bread_count, 2,
        "Elf should spawn with 2 owned bread from elf_starting_bread config"
    );
    // Bow — default is now 0 (military equipment comes from ground piles).
    let bow_count = sim.inv_count_owned(inv_id, inventory::ItemKind::Bow, elf_id);
    assert_eq!(
        bow_count, 0,
        "Elf should spawn with 0 bows (elf_starting_bows = 0)"
    );
    // Arrows — default is now 0.
    let arrow_count = sim.inv_count_owned(inv_id, inventory::ItemKind::Arrow, elf_id);
    assert_eq!(
        arrow_count, 0,
        "Elf should spawn with 0 arrows (elf_starting_arrows = 0)"
    );
    assert_eq!(
        sim.inv_items(inv_id).len(),
        1,
        "Inventory should have exactly one stack (bread)"
    );
}

#[test]
fn creature_add_and_query_bread() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let elf_id = spawn_elf(&mut sim);

    sim.inv_add_simple_item(
        sim.creature_inv(elf_id),
        crate::inventory::ItemKind::Bread,
        5,
        Some(elf_id),
        None,
    );

    let count = sim.inv_item_count(
        sim.creature_inv(elf_id),
        crate::inventory::ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert_eq!(count, 5);
}

#[test]
fn creature_inventory_serialization_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let elf_id = spawn_elf(&mut sim);

    sim.inv_add_simple_item(
        sim.creature_inv(elf_id),
        crate::inventory::ItemKind::Bread,
        3,
        Some(elf_id),
        None,
    );

    // Verify via inv_item_count (serialization of item_stacks is tested separately).
    let count = sim.inv_item_count(
        sim.creature_inv(elf_id),
        crate::inventory::ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert_eq!(count, 3);
}

// ---------------------------------------------------------------------------
// Ground piles
// ---------------------------------------------------------------------------

#[test]
fn ground_piles_in_sim_state() {
    let mut sim = test_sim(legacy_test_seed());
    let pos = VoxelCoord::new(10, 1, 20);
    {
        let pile_id = sim.ensure_ground_pile(pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            4,
            None,
            None,
        );
    }
    assert_eq!(sim.db.ground_piles.len(), 1);
    let pile = sim
        .db
        .ground_piles
        .by_position(&pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        sim.inv_item_count(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            crate::inventory::MaterialFilter::Any
        ),
        4
    );
}

#[test]
fn ground_piles_serialization_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let pos1 = VoxelCoord::new(10, 1, 20);
    let pos2 = VoxelCoord::new(3, 1, 7);
    {
        let pile_id = sim.ensure_ground_pile(pos1);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Fruit,
            2,
            None,
            None,
        );
    }
    {
        let pile_id = sim.ensure_ground_pile(pos2);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            5,
            None,
            None,
        );
    }

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.db.ground_piles.len(), 2);
    let pile1 = restored
        .db
        .ground_piles
        .by_position(&pos1, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(pile1.position, pos1);
    assert_eq!(
        restored.inv_item_count(
            pile1.inventory_id,
            crate::inventory::ItemKind::Fruit,
            crate::inventory::MaterialFilter::Any
        ),
        2
    );
    let pile2 = restored
        .db
        .ground_piles
        .by_position(&pos2, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(pile2.position, pos2);
    assert_eq!(
        restored.inv_item_count(
            pile2.inventory_id,
            crate::inventory::ItemKind::Bread,
            crate::inventory::MaterialFilter::Any
        ),
        5
    );
}

#[test]
fn ground_piles_serde_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let pos = VoxelCoord::new(10, 1, 20);
    {
        let pile_id = sim.ensure_ground_pile(pos);
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            7,
            None,
            None,
        );
    }

    // Serialize → deserialize round-trip.
    let json = sim.to_json().expect("serialization should succeed");
    let restored = SimState::from_json(&json).expect("deserialization should succeed");

    assert_eq!(restored.db.ground_piles.len(), 1);
    let restored_pile = restored
        .db
        .ground_piles
        .by_position(&pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        restored.inv_item_count(
            restored_pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            crate::inventory::MaterialFilter::Any,
        ),
        7
    );

    // Checksums should match.
    assert_eq!(sim.state_checksum(), restored.state_checksum());
}

// ---------------------------------------------------------------------------
// Reserve owned items
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Item serde
// ---------------------------------------------------------------------------

#[test]
fn new_item_kind_serde_roundtrip() {
    use crate::inventory::ItemKind;
    for kind in [
        ItemKind::Bow,
        ItemKind::Arrow,
        ItemKind::Bowstring,
        ItemKind::Pulp,
        ItemKind::Husk,
        ItemKind::Seed,
        ItemKind::FruitFiber,
        ItemKind::FruitSap,
        ItemKind::FruitResin,
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        let restored: ItemKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, restored);
    }
}

#[test]
fn material_enum_serde_roundtrip() {
    use crate::inventory::Material;
    for mat in [
        Material::Oak,
        Material::Birch,
        Material::Willow,
        Material::Ash,
        Material::Yew,
        Material::FruitSpecies(crate::fruit::FruitSpeciesId(42)),
    ] {
        let json = serde_json::to_string(&mat).unwrap();
        let restored: Material = serde_json::from_str(&json).unwrap();
        assert_eq!(mat, restored);
    }
}

#[test]
fn item_stack_serde_backward_compat() {
    // Old JSON without material/quality/enchantment_id should deserialize.
    let json = r#"{
            "id": 1,
            "inventory_id": 1,
            "kind": "Bread",
            "quantity": 5,
            "owner": null,
            "reserved_by": null
        }"#;
    let stack: crate::db::ItemStack = serde_json::from_str(json).unwrap();
    assert_eq!(stack.kind, inventory::ItemKind::Bread);
    assert_eq!(stack.quantity, 5);
    assert!(stack.material.is_none());
    assert_eq!(stack.quality, 0);
    assert!(stack.enchantment_id.is_none());
}

// ---------------------------------------------------------------------------
// Item stacking (add, normalize, material, quality)
// ---------------------------------------------------------------------------

#[test]
fn inv_add_simple_item_stacks_correctly() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 3, None, None);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 2, None, None);
    // Should stack into one row with qty 5.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].quantity, 5);
}

#[test]
fn inv_add_item_material_creates_separate_stacks() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
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
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bow,
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
    assert_eq!(stacks.len(), 2, "Different materials should not stack");
}

#[test]
fn inv_add_item_quality_creates_separate_stacks() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bow,
        1,
        None,
        None,
        None,
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bow,
        1,
        None,
        None,
        None,
        3,
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 2, "Different qualities should not stack");
}

#[test]
fn inv_normalize_respects_material_quality() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    // Create two stacks with same kind but different quality.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Arrow,
        5,
        None,
        None,
        None,
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Arrow,
        3,
        None,
        None,
        None,
        1,
        None,
        None,
    );
    sim.inv_normalize(inv_id);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(
        stacks.len(),
        2,
        "Merge should keep different qualities separate"
    );
}

// ---------------------------------------------------------------------------
// Subcomponent and enchantment cascade delete
// ---------------------------------------------------------------------------

#[test]
fn item_subcomponent_cascade_delete() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bow, 1, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let stack_id = stacks[0].id;

    // Add a subcomponent.
    let seq = sim.db.item_subcomponents.next_seq();
    sim.db
        .insert_item_subcomponent(crate::db::ItemSubcomponent {
            item_stack_id: stack_id,
            seq,
            component_kind: inventory::ItemKind::Bowstring,
            material: None,
            quality: 0,
            quantity_per_item: 1,
        })
        .unwrap();
    assert_eq!(sim.db.item_subcomponents.len(), 1);

    // Delete the item stack — subcomponent should cascade.
    sim.db.remove_item_stack(&stack_id).unwrap();
    assert_eq!(
        sim.db.item_subcomponents.len(),
        0,
        "Subcomponent should cascade delete with parent stack"
    );
}

#[test]
fn enchantment_effect_cascade_delete() {
    let mut sim = test_sim(legacy_test_seed());

    // Create an enchantment.
    let ench_id = sim
        .db
        .insert_item_enchantment_auto(|id| crate::db::ItemEnchantment { id })
        .unwrap();

    // Add an effect.
    let seq = sim.db.enchantment_effects.next_seq();
    sim.db
        .insert_enchantment_effect(crate::db::EnchantmentEffect {
            enchantment_id: ench_id,
            seq,
            effect_kind: inventory::EffectKind::Placeholder,
            magnitude: 10,
            threshold: None,
        })
        .unwrap();
    assert_eq!(sim.db.enchantment_effects.len(), 1);

    // Delete enchantment — effect should cascade.
    sim.db.remove_item_enchantment(&ench_id).unwrap();
    assert_eq!(
        sim.db.enchantment_effects.len(),
        0,
        "Effect should cascade delete with parent enchantment"
    );
}

// ---------------------------------------------------------------------------
// Material filter, reserve items
// ---------------------------------------------------------------------------

#[test]
fn inv_item_count_respects_material_filter() {
    let mut sim = test_sim(legacy_test_seed());
    let pos = VoxelCoord::new(10, 1, 20);
    let pile_id = sim.ensure_ground_pile(pos);
    let inv_id = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;

    let species_a = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(1));
    let species_b = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(2));

    sim.inv_add_item(
        inv_id,
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
        inv_id,
        inventory::ItemKind::Fruit,
        5,
        None,
        None,
        Some(species_b),
        0,
        None,
        None,
    );
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 2, None, None);

    // Any filter counts all fruit.
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Any
        ),
        8
    );
    // Specific filter counts only matching species.
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Specific(species_a)
        ),
        3
    );
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Specific(species_b)
        ),
        5
    );
    // Bread is unmaterialed — Specific for a material should return 0.
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Bread,
            inventory::MaterialFilter::Specific(inventory::Material::Oak)
        ),
        0
    );
}

#[test]
fn inv_reserve_items_single_material_lock() {
    let mut sim = test_sim(legacy_test_seed());
    let pos = VoxelCoord::new(10, 1, 20);
    let pile_id = sim.ensure_ground_pile(pos);
    let inv_id = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;

    let species_a = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(1));
    let species_b = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(2));

    // Add 3 of species A and 5 of species B.
    sim.inv_add_item(
        inv_id,
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
        inv_id,
        inventory::ItemKind::Fruit,
        5,
        None,
        None,
        Some(species_b),
        0,
        None,
        None,
    );

    // Request 6 fruit with Any filter. Should lock in one material.
    let task_id = TaskId::new(&mut sim.rng.clone());
    insert_stub_task(&mut sim, task_id);
    let hauled_material = sim.inv_reserve_items(
        inv_id,
        inventory::ItemKind::Fruit,
        inventory::MaterialFilter::Any,
        6,
        task_id,
    );

    // Should have locked in species_a (first in BTree order) and reserved only 3.
    assert_eq!(hauled_material, Some(species_a));
    let unreserved = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Fruit,
        inventory::MaterialFilter::Any,
    );
    assert_eq!(unreserved, 5, "Only species B (5) should remain unreserved");
}

#[test]
fn inv_reserve_items_specific_filter() {
    let mut sim = test_sim(legacy_test_seed());
    let pos = VoxelCoord::new(10, 1, 20);
    let pile_id = sim.ensure_ground_pile(pos);
    let inv_id = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;

    let species_a = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(1));
    let species_b = inventory::Material::FruitSpecies(crate::fruit::FruitSpeciesId(2));

    sim.inv_add_item(
        inv_id,
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
        inv_id,
        inventory::ItemKind::Fruit,
        5,
        None,
        None,
        Some(species_b),
        0,
        None,
        None,
    );

    // Reserve with Specific(species_b) filter — should only reserve species B.
    let task_id = TaskId::new(&mut sim.rng.clone());
    insert_stub_task(&mut sim, task_id);
    let hauled = sim.inv_reserve_items(
        inv_id,
        inventory::ItemKind::Fruit,
        inventory::MaterialFilter::Specific(species_b),
        10,
        task_id,
    );
    assert_eq!(hauled, Some(species_b));

    // All species A should still be unreserved.
    assert_eq!(
        sim.inv_unreserved_item_count(
            inv_id,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Specific(species_a)
        ),
        3
    );
    // Species B should all be reserved.
    assert_eq!(
        sim.inv_unreserved_item_count(
            inv_id,
            inventory::ItemKind::Fruit,
            inventory::MaterialFilter::Specific(species_b)
        ),
        0
    );
}

// ---------------------------------------------------------------------------
// Split stack
// ---------------------------------------------------------------------------

#[test]
fn inv_split_stack_preserves_properties() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        3,
        None,
        None,
        Some(inventory::Material::FruitSpecies(
            crate::fruit::FruitSpeciesId(0),
        )),
        5,
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1);
    let orig_id = stacks[0].id;

    let new_id = sim.inv_split_stack(orig_id, 1).unwrap();
    assert_ne!(new_id, orig_id, "Split should create a new stack");

    let orig = sim.db.item_stacks.get(&orig_id).unwrap();
    let split = sim.db.item_stacks.get(&new_id).unwrap();
    assert_eq!(orig.quantity, 2);
    assert_eq!(split.quantity, 1);
    // Properties preserved.
    assert_eq!(split.material, orig.material);
    assert_eq!(split.quality, orig.quality);
    assert_eq!(split.current_hp, orig.current_hp);
    assert_eq!(split.max_hp, orig.max_hp);
    assert_eq!(split.kind, orig.kind);
}

#[test]
fn inv_split_stack_whole_stack() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        2,
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
    let orig_id = stacks[0].id;

    // Splitting >= quantity returns the original stack.
    let result = sim.inv_split_stack(orig_id, 2).unwrap();
    assert_eq!(result, orig_id);
    let result = sim.inv_split_stack(orig_id, 5).unwrap();
    assert_eq!(result, orig_id);

    // Splitting 0 returns None.
    assert!(sim.inv_split_stack(orig_id, 0).is_none());
}

// =========================================================================
// Ground pile gravity
// =========================================================================

#[test]
fn pile_on_solid_ground_does_not_fall() {
    let mut sim = test_sim(legacy_test_seed());
    // Place a pile on y=1 (above terrain at y=0 — always solid).
    let pos = VoxelCoord::new(10, 1, 10);
    let pile_id = sim.ensure_ground_pile(pos);
    sim.inv_add_simple_item(
        sim.db.ground_piles.get(&pile_id).unwrap().inventory_id,
        inventory::ItemKind::Bread,
        3,
        None,
        None,
    );

    let fell = sim.apply_pile_gravity();
    assert_eq!(fell, 0);

    // Pile is still at original position.
    let pile = sim.db.ground_piles.get(&pile_id).unwrap();
    assert_eq!(pile.position, pos);
}

#[test]
fn floating_pile_falls_to_surface() {
    let mut sim = test_sim(legacy_test_seed());
    // Create a solid platform at y=5 by setting (10, 5, 10) to Platform.
    let platform_pos = VoxelCoord::new(10, 5, 10);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);

    // Place a pile at y=6 (on top of the platform).
    let pile_pos = VoxelCoord::new(10, 6, 10);
    let pile_id = sim.ensure_ground_pile(pile_pos);
    sim.inv_add_simple_item(
        sim.db.ground_piles.get(&pile_id).unwrap().inventory_id,
        inventory::ItemKind::Bread,
        5,
        None,
        None,
    );

    // Pile should not fall — platform is solid below.
    assert_eq!(sim.apply_pile_gravity(), 0);

    // Remove the platform — pile is now floating.
    sim.world.set(platform_pos, VoxelType::Air);
    let fell = sim.apply_pile_gravity();
    assert_eq!(fell, 1);

    // Pile should have fallen to y=1 (above terrain at y=0).
    // The pile gets a new ID after remove+re-insert, so look up by position.
    let landing = VoxelCoord::new(10, 1, 10);
    let piles_at_landing = sim
        .db
        .ground_piles
        .by_position(&landing, tabulosity::QueryOpts::ASC);
    assert_eq!(piles_at_landing.len(), 1);
    let pile = &piles_at_landing[0];

    // Items should still be there.
    let stacks = sim.inv_items(pile.inventory_id);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].kind, inventory::ItemKind::Bread);
    assert_eq!(stacks[0].quantity, 5);
}

#[test]
fn floating_pile_merges_with_existing_pile() {
    let mut sim = test_sim(legacy_test_seed());
    // Place a pile on the ground at y=1.
    let ground_pos = VoxelCoord::new(15, 1, 15);
    let ground_pile_id = sim.ensure_ground_pile(ground_pos);
    let ground_inv = sim
        .db
        .ground_piles
        .get(&ground_pile_id)
        .unwrap()
        .inventory_id;
    sim.inv_add_simple_item(ground_inv, inventory::ItemKind::Bread, 3, None, None);

    // Create a platform and a pile on top of it.
    let platform_pos = VoxelCoord::new(15, 5, 15);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    let high_pos = VoxelCoord::new(15, 6, 15);
    let high_pile_id = sim.ensure_ground_pile(high_pos);
    let high_inv = sim.db.ground_piles.get(&high_pile_id).unwrap().inventory_id;
    sim.inv_add_simple_item(high_inv, inventory::ItemKind::Fruit, 2, None, None);

    // Remove the platform — high pile should fall and merge with ground pile.
    sim.world.set(platform_pos, VoxelType::Air);
    let fell = sim.apply_pile_gravity();
    assert_eq!(fell, 1);

    // The floating pile should be deleted.
    assert!(sim.db.ground_piles.get(&high_pile_id).is_none());

    // The ground pile should have both item types.
    let ground_pile = sim.db.ground_piles.get(&ground_pile_id).unwrap();
    assert_eq!(ground_pile.position, ground_pos);
    let stacks = sim.inv_items(ground_pile.inventory_id);
    assert_eq!(stacks.len(), 2);
    let bread = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Bread)
        .unwrap();
    let fruit = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Fruit)
        .unwrap();
    assert_eq!(bread.quantity, 3);
    assert_eq!(fruit.quantity, 2);
}

#[test]
fn merge_stacks_same_item_kind() {
    let mut sim = test_sim(legacy_test_seed());
    // Both piles have Bread — after merge, the ground pile should have a
    // single Bread stack with the combined quantity.
    let ground_pos = VoxelCoord::new(20, 1, 20);
    let ground_pile_id = sim.ensure_ground_pile(ground_pos);
    let ground_inv = sim
        .db
        .ground_piles
        .get(&ground_pile_id)
        .unwrap()
        .inventory_id;
    sim.inv_add_simple_item(ground_inv, inventory::ItemKind::Bread, 4, None, None);

    let platform_pos = VoxelCoord::new(20, 3, 20);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    let high_pos = VoxelCoord::new(20, 4, 20);
    let high_pile_id = sim.ensure_ground_pile(high_pos);
    let high_inv = sim.db.ground_piles.get(&high_pile_id).unwrap().inventory_id;
    sim.inv_add_simple_item(high_inv, inventory::ItemKind::Bread, 6, None, None);

    sim.world.set(platform_pos, VoxelType::Air);
    sim.apply_pile_gravity();

    assert!(sim.db.ground_piles.get(&high_pile_id).is_none());
    let stacks = sim.inv_items(ground_inv);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].kind, inventory::ItemKind::Bread);
    assert_eq!(stacks[0].quantity, 10);
}

#[test]
fn pile_falls_to_intermediate_surface() {
    let mut sim = test_sim(legacy_test_seed());
    // Two platforms stacked: y=3 and y=6. Pile at y=7.
    // Remove y=6 — pile should fall to y=4 (on top of y=3 platform), not y=1.
    let lower_platform = VoxelCoord::new(25, 3, 25);
    let upper_platform = VoxelCoord::new(25, 6, 25);
    sim.world.set(lower_platform, VoxelType::GrownPlatform);
    sim.world.set(upper_platform, VoxelType::GrownPlatform);

    let pile_pos = VoxelCoord::new(25, 7, 25);
    let pile_id = sim.ensure_ground_pile(pile_pos);
    sim.inv_add_simple_item(
        sim.db.ground_piles.get(&pile_id).unwrap().inventory_id,
        inventory::ItemKind::Bread,
        1,
        None,
        None,
    );

    // Remove upper platform only.
    sim.world.set(upper_platform, VoxelType::Air);
    sim.apply_pile_gravity();

    // Pile gets a new ID after remove+re-insert, so look up by position.
    let landing = VoxelCoord::new(25, 4, 25);
    let piles = sim
        .db
        .ground_piles
        .by_position(&landing, tabulosity::QueryOpts::ASC);
    assert_eq!(piles.len(), 1, "pile should land on top of lower platform");
}

#[test]
fn multiple_floating_piles_in_same_column() {
    let mut sim = test_sim(legacy_test_seed());
    // Two platforms at y=3 and y=6. Piles at y=4 and y=7.
    // Remove both platforms — both piles should fall to y=1, merging.
    // Use coordinates far from the tree trunk (~32,32) to avoid overlap.
    let p1 = VoxelCoord::new(10, 3, 10);
    let p2 = VoxelCoord::new(10, 6, 10);
    sim.world.set(p1, VoxelType::GrownPlatform);
    sim.world.set(p2, VoxelType::GrownPlatform);

    let pile1_pos = VoxelCoord::new(10, 4, 10);
    let pile1_id = sim.ensure_ground_pile(pile1_pos);
    let pile1_inv = sim.db.ground_piles.get(&pile1_id).unwrap().inventory_id;
    sim.inv_add_simple_item(pile1_inv, inventory::ItemKind::Bread, 2, None, None);

    let pile2_pos = VoxelCoord::new(10, 7, 10);
    let pile2_id = sim.ensure_ground_pile(pile2_pos);
    let pile2_inv = sim.db.ground_piles.get(&pile2_id).unwrap().inventory_id;
    sim.inv_add_simple_item(pile2_inv, inventory::ItemKind::Fruit, 3, None, None);

    sim.world.set(p1, VoxelType::Air);
    sim.world.set(p2, VoxelType::Air);
    let fell = sim.apply_pile_gravity();
    assert_eq!(fell, 2);

    // Both should have ended up at y=1. Only one pile should remain.
    let remaining: Vec<_> = sim
        .db
        .ground_piles
        .iter_all()
        .filter(|p| p.position.x == 10 && p.position.z == 10)
        .collect();
    assert_eq!(remaining.len(), 1);
    let final_pile = &remaining[0];
    assert_eq!(final_pile.position.y, 1);

    // Should have both item types.
    let stacks = sim.inv_items(final_pile.inventory_id);
    let total_items: u32 = stacks.iter().map(|s| s.quantity).sum();
    assert_eq!(total_items, 5);
}

#[test]
fn empty_floating_pile_is_cleaned_up() {
    let mut sim = test_sim(legacy_test_seed());
    // A floating pile with no items should still be moved.
    let platform_pos = VoxelCoord::new(35, 3, 35);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    let pile_pos = VoxelCoord::new(35, 4, 35);
    let _pile_id = sim.ensure_ground_pile(pile_pos);

    sim.world.set(platform_pos, VoxelType::Air);
    let fell = sim.apply_pile_gravity();
    assert_eq!(fell, 1);

    // Pile should have moved to y=1.
    let landing = VoxelCoord::new(35, 1, 35);
    let piles = sim
        .db
        .ground_piles
        .by_position(&landing, tabulosity::QueryOpts::ASC);
    assert_eq!(piles.len(), 1);
}

#[test]
fn inv_merge_combines_inventories() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::GroundPile);

    // Same kind in both — should combine into one stack.
    sim.inv_add_simple_item(src, inventory::ItemKind::Bread, 3, None, None);
    sim.inv_add_simple_item(dst, inventory::ItemKind::Bread, 2, None, None);
    // Different kind in src — should become a new stack in dst.
    sim.inv_add_simple_item(src, inventory::ItemKind::Fruit, 1, None, None);

    sim.inv_merge(src, dst);

    // Source should be empty.
    assert!(sim.inv_items(src).is_empty());

    // Destination should have 2 stacks: Bread(5) and Fruit(1).
    let stacks = sim.inv_items(dst);
    assert_eq!(stacks.len(), 2);
    let bread = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Bread)
        .unwrap();
    let fruit = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Fruit)
        .unwrap();
    assert_eq!(bread.quantity, 5);
    assert_eq!(fruit.quantity, 1);
}

#[test]
fn ensure_ground_pile_snaps_floating_position_to_surface() {
    let mut sim = test_sim(legacy_test_seed());
    // Request a pile at y=10 with no solid voxel below (except floor at y=0).
    let floating_pos = VoxelCoord::new(40, 10, 40);
    let pile_id = sim.ensure_ground_pile(floating_pos);

    // Pile should have been snapped to y=1 (above terrain).
    let pile = sim.db.ground_piles.get(&pile_id).unwrap();
    assert_eq!(pile.position, VoxelCoord::new(40, 1, 40));
}

#[test]
fn ensure_ground_pile_snaps_to_intermediate_platform() {
    let mut sim = test_sim(legacy_test_seed());
    // Platform at y=5, request pile at y=10.
    sim.world
        .set(VoxelCoord::new(42, 5, 42), VoxelType::GrownPlatform);
    let pile_id = sim.ensure_ground_pile(VoxelCoord::new(42, 10, 42));

    let pile = sim.db.ground_piles.get(&pile_id).unwrap();
    assert_eq!(pile.position, VoxelCoord::new(42, 6, 42));
}

#[test]
fn ensure_ground_pile_merges_when_snapped_to_existing() {
    let mut sim = test_sim(legacy_test_seed());
    // Create a pile at y=1.
    let ground_pos = VoxelCoord::new(44, 1, 44);
    let ground_pile_id = sim.ensure_ground_pile(ground_pos);
    let ground_inv = sim
        .db
        .ground_piles
        .get(&ground_pile_id)
        .unwrap()
        .inventory_id;
    sim.inv_add_simple_item(ground_inv, inventory::ItemKind::Bread, 5, None, None);

    // Request a pile at y=8 (floating) — should snap to y=1 and return
    // the existing pile instead of creating a new one.
    let returned_id = sim.ensure_ground_pile(VoxelCoord::new(44, 8, 44));
    assert_eq!(returned_id, ground_pile_id);

    // Only one pile at this column.
    let piles = sim
        .db
        .ground_piles
        .by_position(&ground_pos, tabulosity::QueryOpts::ASC);
    assert_eq!(piles.len(), 1);
}

// -----------------------------------------------------------------------
// Item durability tests
// -----------------------------------------------------------------------

#[test]
fn inv_add_item_assigns_durability_from_config() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    // Arrow has default durability of 3 in config.
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 5, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].max_hp, 3);
    assert_eq!(stacks[0].current_hp, 3);
}

#[test]
fn inv_add_item_no_durability_for_unconfigured_kinds() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    // Bread has no durability config — should be 0/0 (indestructible).
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 3, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks[0].max_hp, 0);
    assert_eq!(stacks[0].current_hp, 0);
}

#[test]
fn durability_stacking_same_hp_merges() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    // Two batches of arrows with same durability should merge.
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 3, None, None);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 2, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1, "Same-durability arrows should stack");
    assert_eq!(stacks[0].quantity, 5);
}

#[test]
fn durability_stacking_different_hp_separate() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    // Add arrows at full HP.
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 3, None, None);
    // Add arrows with reduced current_hp via explicit durability.
    sim.inv_add_item_with_durability(
        inv_id,
        inventory::ItemKind::Arrow,
        2,
        None,
        None,
        None,
        0,
        2, // current_hp
        3, // max_hp
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(
        stacks.len(),
        2,
        "Arrows with different current_hp should not stack"
    );
}

#[test]
fn inv_split_stack_preserves_durability() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 5, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let orig_id = stacks[0].id;

    let new_id = sim.inv_split_stack(orig_id, 2).unwrap();
    assert_ne!(new_id, orig_id);

    let orig = sim.db.item_stacks.get(&orig_id).unwrap();
    let split = sim.db.item_stacks.get(&new_id).unwrap();
    assert_eq!(orig.quantity, 3);
    assert_eq!(split.quantity, 2);
    assert_eq!(split.current_hp, orig.current_hp);
    assert_eq!(split.max_hp, orig.max_hp);
    assert_eq!(split.max_hp, 3, "Arrow max_hp from config");
}

#[test]
fn inv_merge_preserves_durability() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(src, inventory::ItemKind::Arrow, 3, None, None);
    sim.inv_add_simple_item(dst, inventory::ItemKind::Arrow, 2, None, None);

    sim.inv_merge(src, dst);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1, "Same-durability arrows should merge");
    assert_eq!(stacks[0].quantity, 5);
    assert_eq!(stacks[0].current_hp, 3);
    assert_eq!(stacks[0].max_hp, 3);
}

#[test]
fn inv_merge_keeps_different_durability_separate() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    // Full-HP arrows in dst.
    sim.inv_add_simple_item(dst, inventory::ItemKind::Arrow, 3, None, None);
    // Damaged arrows in src.
    sim.inv_add_item_with_durability(
        src,
        inventory::ItemKind::Arrow,
        2,
        None,
        None,
        None,
        0,
        1, // current_hp
        3, // max_hp
        None,
        None,
    );

    sim.inv_merge(src, dst);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    assert_eq!(
        stacks.len(),
        2,
        "Different current_hp should remain separate after merge"
    );
}

#[test]
fn inv_damage_item_reduces_hp() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 1, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let stack_id = stacks[0].id;

    let mut events = Vec::new();
    let broke = sim.inv_damage_item(stack_id, 1, &mut events);
    assert!(!broke, "Arrow should not break from 1 damage (3 HP)");
    assert!(events.is_empty());

    let stack = sim.db.item_stacks.get(&stack_id).unwrap();
    assert_eq!(stack.current_hp, 2);
}

#[test]
fn inv_damage_item_breaks_at_zero() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 1, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let stack_id = stacks[0].id;

    let mut events = Vec::new();
    let broke = sim.inv_damage_item(stack_id, 3, &mut events);
    assert!(broke, "Arrow should break from 3 damage (3 HP)");
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0].kind, SimEventKind::ItemBroken { item_kind, .. } if *item_kind == inventory::ItemKind::Arrow)
    );
    // Stack should be removed.
    assert!(sim.db.item_stacks.get(&stack_id).is_none());
}

#[test]
fn inv_damage_item_noop_on_indestructible() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 5, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let stack_id = stacks[0].id;

    let mut events = Vec::new();
    let broke = sim.inv_damage_item(stack_id, 100, &mut events);
    assert!(!broke, "Indestructible items should not break");
    assert!(events.is_empty());
    let stack = sim.db.item_stacks.get(&stack_id).unwrap();
    assert_eq!(stack.quantity, 5, "Bread should remain unchanged");
}

#[test]
fn inv_damage_item_breaks_one_from_stack() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 5, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let stack_id = stacks[0].id;

    let mut events = Vec::new();
    let broke = sim.inv_damage_item(stack_id, 3, &mut events);
    assert!(broke, "Should break one arrow from the stack");
    assert_eq!(events.len(), 1);

    // Stack should still exist with qty 4.
    let stack = sim.db.item_stacks.get(&stack_id).unwrap();
    assert_eq!(stack.quantity, 4);
    // Remaining arrows should still have full HP (the broken one is gone).
    assert_eq!(stack.current_hp, 3);
}

#[test]
fn inv_damage_item_partial_on_multi_stack_splits() {
    // Partial damage on a multi-item stack should split off one item
    // and only damage that one, leaving the rest at full HP.
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 5, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let stack_id = stacks[0].id;

    let mut events = Vec::new();
    let broke = sim.inv_damage_item(stack_id, 1, &mut events);
    assert!(!broke, "Arrow should not break from 1 damage (3 HP)");
    assert!(events.is_empty());

    // Should now have 2 stacks: 4 at full HP, 1 at reduced HP.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 2, "Should split into damaged and undamaged");
    let full_hp_stack = stacks.iter().find(|s| s.current_hp == 3).unwrap();
    let damaged_stack = stacks.iter().find(|s| s.current_hp == 2).unwrap();
    assert_eq!(full_hp_stack.quantity, 4);
    assert_eq!(damaged_stack.quantity, 1);
}

#[test]
fn inv_damage_item_zero_amount_noop() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 3, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let stack_id = stacks[0].id;

    let mut events = Vec::new();
    let broke = sim.inv_damage_item(stack_id, 0, &mut events);
    assert!(!broke);
    assert!(events.is_empty());
    let stack = sim.db.item_stacks.get(&stack_id).unwrap();
    assert_eq!(stack.current_hp, 3);
    assert_eq!(stack.quantity, 3);
}

#[test]
fn durability_serde_backward_compat() {
    // Old JSON without current_hp/max_hp should deserialize with 0/0.
    let json = r#"{
            "id": 1,
            "inventory_id": 1,
            "kind": "Arrow",
            "quantity": 5,
            "material": null,
            "quality": 0,
            "enchantment_id": null,
            "owner": null,
            "reserved_by": null
        }"#;
    let stack: crate::db::ItemStack = serde_json::from_str(json).unwrap();
    assert_eq!(stack.current_hp, 0);
    assert_eq!(stack.max_hp, 0);
}

#[test]
fn durability_serde_roundtrip() {
    let json = r#"{
            "id": 1,
            "inventory_id": 1,
            "kind": "Arrow",
            "quantity": 5,
            "material": null,
            "quality": 0,
            "current_hp": 2,
            "max_hp": 3,
            "enchantment_id": null,
            "owner": null,
            "reserved_by": null
        }"#;
    let stack: crate::db::ItemStack = serde_json::from_str(json).unwrap();
    assert_eq!(stack.current_hp, 2);
    assert_eq!(stack.max_hp, 3);

    let serialized = serde_json::to_string(&stack).unwrap();
    let restored: crate::db::ItemStack = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored.current_hp, 2);
    assert_eq!(restored.max_hp, 3);
}

#[test]
fn item_durability_config_defaults() {
    let config = crate::config::GameConfig::default();
    // Arrows: small range for stacking.
    assert_eq!(
        config.item_durability.get(&inventory::ItemKind::Arrow),
        Some(&3)
    );
    // Bow: moderate.
    assert_eq!(
        config.item_durability.get(&inventory::ItemKind::Bow),
        Some(&50)
    );
    // Breastplate: high.
    assert_eq!(
        config
            .item_durability
            .get(&inventory::ItemKind::Breastplate),
        Some(&60)
    );
    // Bread: not in map (indestructible).
    assert_eq!(
        config.item_durability.get(&inventory::ItemKind::Bread),
        None
    );
}

#[test]
fn inv_add_item_with_durability_explicit() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_item_with_durability(
        inv_id,
        inventory::ItemKind::Arrow,
        3,
        None,
        None,
        None,
        0,
        1, // current_hp
        3, // max_hp
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].current_hp, 1);
    assert_eq!(stacks[0].max_hp, 3);
}

#[test]
fn inv_reserve_items_preserves_durability() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 5, None, None);

    let task_id = TaskId::new(&mut sim.rng.clone());
    insert_stub_task(&mut sim, task_id);
    sim.inv_reserve_items(
        inv_id,
        inventory::ItemKind::Arrow,
        inventory::MaterialFilter::Any,
        2,
        task_id,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    // Should have 2 stacks: 3 unreserved, 2 reserved — both with same durability.
    assert_eq!(stacks.len(), 2);
    for s in &stacks {
        assert_eq!(s.current_hp, 3);
        assert_eq!(s.max_hp, 3);
    }
}

// -----------------------------------------------------------------------
// inv_move_stack / inv_move_items tests
// -----------------------------------------------------------------------

#[test]
fn inv_move_stack_basic() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_item_with_durability(
        src,
        inventory::ItemKind::Arrow,
        5,
        None,
        None,
        Some(inventory::Material::Oak),
        2, // quality
        2, // current_hp
        3, // max_hp
        None,
        None,
    );
    let stack_id = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC)[0]
        .id;

    // Move 3 of 5 arrows to dst.
    let moved_id = sim.inv_move_stack(stack_id, 3, dst).unwrap();

    // Source should have 2 remaining.
    let src_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
    assert_eq!(src_stacks.len(), 1);
    assert_eq!(src_stacks[0].quantity, 2);

    // Dst should have 3, with all properties preserved.
    let moved = sim.db.item_stacks.get(&moved_id).unwrap();
    assert_eq!(moved.inventory_id, dst);
    assert_eq!(moved.quantity, 3);
    assert_eq!(moved.material, Some(inventory::Material::Oak));
    assert_eq!(moved.quality, 2);
    assert_eq!(moved.current_hp, 2);
    assert_eq!(moved.max_hp, 3);
}

#[test]
fn inv_move_stack_whole_stack() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(src, inventory::ItemKind::Arrow, 3, None, None);
    let stack_id = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC)[0]
        .id;

    // Move entire stack (quantity >= stack quantity).
    let moved_id = sim.inv_move_stack(stack_id, 5, dst).unwrap();
    assert_eq!(moved_id, stack_id, "Whole-stack move returns same ID");

    // Source should be empty.
    let src_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
    assert!(src_stacks.is_empty());

    // Dst should have the stack.
    let moved = sim.db.item_stacks.get(&moved_id).unwrap();
    assert_eq!(moved.inventory_id, dst);
    assert_eq!(moved.quantity, 3);
    assert_eq!(moved.current_hp, 3);
    assert_eq!(moved.max_hp, 3);
}

#[test]
fn inv_move_stack_preserves_owner_and_reserved() {
    // inv_move_stack should NOT clear owner or reserved_by.
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let fake_owner = spawn_elf(&mut sim);
    let fake_task = TaskId::new(&mut sim.rng.clone());
    insert_stub_task(&mut sim, fake_task);
    sim.inv_add_item(
        src,
        inventory::ItemKind::Bow,
        1,
        Some(fake_owner),
        Some(fake_task),
        Some(inventory::Material::Yew),
        0,
        None,
        None,
    );
    let stack_id = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC)[0]
        .id;

    let moved_id = sim.inv_move_stack(stack_id, 1, dst).unwrap();
    let moved = sim.db.item_stacks.get(&moved_id).unwrap();
    assert_eq!(moved.owner, Some(fake_owner), "Owner must be preserved");
    assert_eq!(
        moved.reserved_by,
        Some(fake_task),
        "reserved_by must be preserved"
    );
}

#[test]
fn inv_move_stack_merges_at_destination() {
    // If dst already has matching items, the moved stack should merge.
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(src, inventory::ItemKind::Arrow, 3, None, None);
    sim.inv_add_simple_item(dst, inventory::ItemKind::Arrow, 2, None, None);
    let stack_id = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC)[0]
        .id;

    sim.inv_move_stack(stack_id, 3, dst);

    let dst_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    assert_eq!(dst_stacks.len(), 1, "Should merge into one stack");
    assert_eq!(dst_stacks[0].quantity, 5);
}

#[test]
fn inv_move_stack_zero_quantity_noop() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(src, inventory::ItemKind::Arrow, 3, None, None);
    let stack_id = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC)[0]
        .id;

    assert!(sim.inv_move_stack(stack_id, 0, dst).is_none());
    // Source unchanged.
    let src_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
    assert_eq!(src_stacks[0].quantity, 3);
}

#[test]
fn inv_move_items_basic() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_item_with_durability(
        src,
        inventory::ItemKind::Arrow,
        5,
        None,
        None,
        Some(inventory::Material::Oak),
        0,
        2, // current_hp (damaged)
        3, // max_hp
        None,
        None,
    );
    sim.inv_add_simple_item(src, inventory::ItemKind::Bread, 3, None, None);

    // Move 3 arrows (by kind, any material).
    let moved = sim.inv_move_items(
        src,
        dst,
        Some(inventory::ItemKind::Arrow),
        None, // any material
        Some(3),
    );
    assert_eq!(moved, 3);

    // Dst should have 3 arrows with preserved durability.
    let dst_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    assert_eq!(dst_stacks.len(), 1);
    assert_eq!(dst_stacks[0].kind, inventory::ItemKind::Arrow);
    assert_eq!(dst_stacks[0].quantity, 3);
    assert_eq!(dst_stacks[0].current_hp, 2);
    assert_eq!(dst_stacks[0].max_hp, 3);
    assert_eq!(dst_stacks[0].material, Some(inventory::Material::Oak));

    // Source should have 2 arrows + 3 bread.
    let src_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
    assert_eq!(src_stacks.len(), 2);
}

#[test]
fn inv_move_items_filter_by_material() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_item(
        src,
        inventory::ItemKind::Bow,
        2,
        None,
        None,
        Some(inventory::Material::Oak),
        0,
        None,
        None,
    );
    sim.inv_add_item(
        src,
        inventory::ItemKind::Bow,
        3,
        None,
        None,
        Some(inventory::Material::Yew),
        0,
        None,
        None,
    );

    // Move only Yew bows.
    let moved = sim.inv_move_items(
        src,
        dst,
        Some(inventory::ItemKind::Bow),
        Some(Some(inventory::Material::Yew)),
        Some(2),
    );
    assert_eq!(moved, 2);

    // Dst: 2 Yew bows.
    let dst_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    assert_eq!(dst_stacks.len(), 1);
    assert_eq!(dst_stacks[0].material, Some(inventory::Material::Yew));
    assert_eq!(dst_stacks[0].quantity, 2);

    // Source: 2 Oak + 1 Yew.
    let src_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
    assert_eq!(src_stacks.len(), 2);
}

#[test]
fn inv_move_items_no_kind_filter_moves_all() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(src, inventory::ItemKind::Arrow, 3, None, None);
    sim.inv_add_simple_item(src, inventory::ItemKind::Bread, 2, None, None);

    // Move everything (no kind filter, no quantity limit).
    let moved = sim.inv_move_items(src, dst, None, None, None);
    assert_eq!(moved, 5);

    let src_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
    assert!(src_stacks.is_empty());

    let dst_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    assert_eq!(dst_stacks.len(), 2);
}

#[test]
fn inv_move_items_partial_when_not_enough() {
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.inv_add_simple_item(src, inventory::ItemKind::Arrow, 3, None, None);

    // Request 10 but only 3 available.
    let moved = sim.inv_move_items(src, dst, Some(inventory::ItemKind::Arrow), None, Some(10));
    assert_eq!(moved, 3);
}

#[test]
fn inv_move_stack_nonexistent_returns_none() {
    let mut sim = test_sim(legacy_test_seed());
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let fake_id = ItemStackId(999_999);
    assert!(sim.inv_move_stack(fake_id, 5, dst).is_none());
    // Dst should remain empty.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    assert!(stacks.is_empty());
}

#[test]
fn inv_damage_item_nonexistent_returns_false() {
    let mut sim = test_sim(legacy_test_seed());
    let fake_id = ItemStackId(999_999);
    let mut events = Vec::new();
    assert!(!sim.inv_damage_item(fake_id, 5, &mut events));
    assert!(events.is_empty());
}

#[test]
fn inv_move_items_multi_durability_stacks() {
    // Moving items that span multiple stacks with different durability
    // should preserve each stack's durability independently.
    let mut sim = test_sim(legacy_test_seed());
    let src = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    let dst = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);

    // 3 full-HP arrows + 2 damaged arrows.
    sim.inv_add_simple_item(src, inventory::ItemKind::Arrow, 3, None, None);
    sim.inv_add_item_with_durability(
        src,
        inventory::ItemKind::Arrow,
        2,
        None,
        None,
        None,
        0,
        1, // current_hp (damaged)
        3, // max_hp
        None,
        None,
    );

    // Move 4 arrows total.
    let moved = sim.inv_move_items(src, dst, Some(inventory::ItemKind::Arrow), None, Some(4));
    assert_eq!(moved, 4);

    // Dst should have items from both durability tiers.
    let dst_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&dst, tabulosity::QueryOpts::ASC);
    let total_qty: u32 = dst_stacks.iter().map(|s| s.quantity).sum();
    assert_eq!(total_qty, 4);

    // Source should have 1 remaining.
    let src_stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
    let src_total: u32 = src_stacks.iter().map(|s| s.quantity).sum();
    assert_eq!(src_total, 1);

    // Verify both durability tiers exist in dst.
    let full_hp: u32 = dst_stacks
        .iter()
        .filter(|s| s.current_hp == 3)
        .map(|s| s.quantity)
        .sum();
    let damaged: u32 = dst_stacks
        .iter()
        .filter(|s| s.current_hp == 1)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(full_hp, 3, "All 3 full-HP arrows should move first");
    assert_eq!(damaged, 1, "1 damaged arrow should move to fill the 4");
}

// ---------------------------------------------------------------------------
// Condition label / display name with durability
// ---------------------------------------------------------------------------

#[test]
fn condition_label_full_hp_returns_none() {
    assert!(SimState::condition_label(3, 3, 70, 40).is_none());
}

#[test]
fn condition_label_indestructible_returns_none() {
    assert!(SimState::condition_label(0, 0, 70, 40).is_none());
}

#[test]
fn condition_label_worn_threshold() {
    // 70% exactly → worn
    assert_eq!(SimState::condition_label(70, 100, 70, 40), Some("(worn)"));
    // 71% → no label
    assert!(SimState::condition_label(71, 100, 70, 40).is_none());
}

#[test]
fn condition_label_damaged_threshold() {
    // 40% exactly → damaged
    assert_eq!(
        SimState::condition_label(40, 100, 70, 40),
        Some("(damaged)")
    );
    // 41% → worn (not damaged)
    assert_eq!(SimState::condition_label(41, 100, 70, 40), Some("(worn)"));
}

#[test]
fn condition_label_arrow_hp_values() {
    // Arrow: 3/3 → no label
    assert!(SimState::condition_label(3, 3, 70, 40).is_none());
    // Arrow: 2/3 = 66% → worn (66 <= 70)
    assert_eq!(SimState::condition_label(2, 3, 70, 40), Some("(worn)"));
    // Arrow: 1/3 = 33% → damaged (33 <= 40)
    assert_eq!(SimState::condition_label(1, 3, 70, 40), Some("(damaged)"));
}

#[test]
fn condition_label_custom_thresholds() {
    // With worn=50, damaged=20: 60% is fine, 50% is worn, 20% is damaged
    assert!(SimState::condition_label(60, 100, 50, 20).is_none());
    assert_eq!(SimState::condition_label(50, 100, 50, 20), Some("(worn)"));
    assert_eq!(
        SimState::condition_label(20, 100, 50, 20),
        Some("(damaged)")
    );
}

#[test]
fn condition_label_current_hp_exceeds_max_hp() {
    // Shouldn't happen, but verify it reports as full health.
    assert!(SimState::condition_label(5, 3, 70, 40).is_none());
}

#[test]
fn item_display_name_shows_condition_worn() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_item_with_durability(
        inv_id,
        inventory::ItemKind::Arrow,
        1,
        None,
        None,
        None,
        0,
        2, // current_hp
        3, // max_hp — 66% → worn
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(sim.item_display_name(&stacks[0]), "Fine Arrow (worn)");
}

#[test]
fn item_display_name_shows_condition_damaged() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_item_with_durability(
        inv_id,
        inventory::ItemKind::Arrow,
        1,
        None,
        None,
        None,
        0,
        1, // current_hp
        3, // max_hp — 33% → damaged
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(sim.item_display_name(&stacks[0]), "Fine Arrow (damaged)");
}

#[test]
fn item_display_name_no_condition_at_full_hp() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 1, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(sim.item_display_name(&stacks[0]), "Fine Arrow");
}

#[test]
fn item_display_name_equipped_and_damaged() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    sim.inv_add_item_with_durability(
        inv_id,
        inventory::ItemKind::Hat,
        1,
        None,
        None,
        None,
        0,
        5,  // current_hp
        20, // max_hp — 25% → damaged
        None,
        Some(inventory::EquipSlot::Head),
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(
        sim.item_display_name(&stacks[0]),
        "Fine Hat (equipped) (damaged)"
    );
}

#[test]
fn item_display_name_indestructible_no_condition() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    // Bread has no durability (max_hp=0).
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 1, None, None);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert_eq!(sim.item_display_name(&stacks[0]), "Fine Bread");
}

#[test]
fn item_display_name_shows_equipped_suffix() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    // Unequipped item.
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
    // Equipped item.
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
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let tunic = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Tunic)
        .unwrap();
    let hat = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Hat)
        .unwrap();
    assert_eq!(sim.item_display_name(tunic), "Fine Tunic");
    assert_eq!(sim.item_display_name(hat), "Fine Hat (equipped)");
}

#[test]
fn item_display_name_dye_color_prefix() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    // Undyed oak helmet.
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
    // Dyed tunic (no material).
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
    let helmet = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Helmet)
        .unwrap();
    let tunic = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Tunic)
        .unwrap();

    // Undyed oak helmet: "Fine Oak Helmet".
    assert_eq!(sim.item_display_name(helmet), "Fine Oak Helmet");

    // Now dye the tunic blue.
    let tunic_id = tunic.id;
    let mut tunic_row = sim.db.item_stacks.get(&tunic_id).unwrap();
    tunic_row.dye_color = Some(inventory::ItemColor::new(50, 70, 180));
    sim.db.update_item_stack(tunic_row).unwrap();
    let tunic_dyed = sim.db.item_stacks.get(&tunic_id).unwrap();
    assert_eq!(sim.item_display_name(&tunic_dyed), "Fine Blue Tunic");

    // Dyed oak breastplate.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Breastplate,
        1,
        None,
        None,
        Some(inventory::Material::Oak),
        0,
        None,
        None,
    );
    let stacks2 = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let bp = stacks2
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Breastplate)
        .unwrap();
    let bp_id = bp.id;
    let mut bp_row = sim.db.item_stacks.get(&bp_id).unwrap();
    bp_row.dye_color = Some(inventory::ItemColor::new(180, 40, 40));
    sim.db.update_item_stack(bp_row).unwrap();
    let bp_dyed = sim.db.item_stacks.get(&bp_id).unwrap();
    assert_eq!(sim.item_display_name(&bp_dyed), "Fine Red Oak Breastplate");
}

// ---------------------------------------------------------------------------
// Arrow durability on impact
// ---------------------------------------------------------------------------

#[test]
fn arrow_surface_hit_always_destroyed_when_min_equals_max_hp() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.config.arrow_impact_damage_min = 3;
    sim.config.arrow_impact_damage_max = 3;

    // Place a solid wall.
    for y in 1..=5 {
        sim.world
            .set(VoxelCoord::new(45, y, 40), VoxelType::GrownPlatform);
    }

    sim.spawn_projectile(VoxelCoord::new(40, 3, 40), VoxelCoord::new(45, 3, 40), None);

    let mut all_events = Vec::new();
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        all_events.extend(events);
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert!(sim.db.projectiles.is_empty(), "Projectile should resolve");
    // Arrow should have been destroyed — no arrows in any ground pile.
    let arrow_count: u32 = sim
        .db
        .ground_piles
        .iter_all()
        .flat_map(|p| {
            sim.db
                .item_stacks
                .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
        })
        .filter(|s| s.kind == inventory::ItemKind::Arrow)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(arrow_count, 0, "Arrow should be destroyed on impact");

    // Should have an ItemBroken event.
    assert!(
        all_events.iter().any(
            |e| matches!(&e.kind, SimEventKind::ItemBroken { item_kind, .. }
                if *item_kind == inventory::ItemKind::Arrow)
        ),
        "Should emit ItemBroken event"
    );
}

#[test]
fn arrow_surface_hit_survives_when_no_damage() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.config.arrow_impact_damage_min = 0;
    sim.config.arrow_impact_damage_max = 0;

    for y in 1..=5 {
        sim.world
            .set(VoxelCoord::new(45, y, 40), VoxelType::GrownPlatform);
    }

    sim.spawn_projectile(VoxelCoord::new(40, 3, 40), VoxelCoord::new(45, 3, 40), None);

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

    // Arrow should survive at full HP in a ground pile.
    let arrows: Vec<_> = sim
        .db
        .ground_piles
        .iter_all()
        .flat_map(|p| {
            sim.db
                .item_stacks
                .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
        })
        .filter(|s| s.kind == inventory::ItemKind::Arrow)
        .collect();
    assert_eq!(arrows.len(), 1, "Arrow should be in a ground pile");
    assert_eq!(arrows[0].current_hp, 3, "Arrow should be at full HP");
}

#[test]
fn arrow_creature_hit_always_destroyed_when_min_equals_max_hp() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.config.arrow_impact_damage_min = 3;
    sim.config.arrow_impact_damage_max = 3;

    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    let origin = VoxelCoord::new(goblin_pos.x - 10, goblin_pos.y, goblin_pos.z);
    sim.spawn_projectile(origin, goblin_pos, None);

    let mut all_events = Vec::new();
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        all_events.extend(events);
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert!(sim.db.projectiles.is_empty());

    // Arrow should be destroyed.
    let arrow_count: u32 = sim
        .db
        .ground_piles
        .iter_all()
        .flat_map(|p| {
            sim.db
                .item_stacks
                .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
        })
        .filter(|s| s.kind == inventory::ItemKind::Arrow)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(arrow_count, 0, "Arrow should be destroyed");

    // Should have ItemBroken event.
    assert!(all_events.iter().any(
        |e| matches!(&e.kind, SimEventKind::ItemBroken { item_kind, .. }
            if *item_kind == inventory::ItemKind::Arrow)
    ));
}

#[test]
fn arrow_creature_hit_survives_when_no_damage() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.config.arrow_impact_damage_min = 0;
    sim.config.arrow_impact_damage_max = 0;

    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    let origin = VoxelCoord::new(goblin_pos.x - 10, goblin_pos.y, goblin_pos.z);
    sim.spawn_projectile(origin, goblin_pos, None);

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

    // Arrow should survive at full HP.
    let arrows: Vec<_> = sim
        .db
        .ground_piles
        .iter_all()
        .flat_map(|p| {
            sim.db
                .item_stacks
                .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
        })
        .filter(|s| s.kind == inventory::ItemKind::Arrow)
        .collect();
    assert_eq!(arrows.len(), 1, "Arrow should be in a ground pile");
    assert_eq!(arrows[0].current_hp, 3);
}

#[test]
fn arrow_impact_damage_is_deterministic() {
    // Same seed should produce identical results.
    let run = |seed: u64| -> (u32, i32) {
        let mut sim = test_sim(seed);
        sim.config.arrow_gravity = 0;
        sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
        sim.config.arrow_impact_damage_min = 0;
        sim.config.arrow_impact_damage_max = 3;

        for y in 1..=5 {
            sim.world
                .set(VoxelCoord::new(45, y, 40), VoxelType::GrownPlatform);
        }

        sim.spawn_projectile(VoxelCoord::new(40, 3, 40), VoxelCoord::new(45, 3, 40), None);

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

        let arrows: Vec<_> = sim
            .db
            .ground_piles
            .iter_all()
            .flat_map(|p| {
                sim.db
                    .item_stacks
                    .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
            })
            .filter(|s| s.kind == inventory::ItemKind::Arrow)
            .collect();
        if arrows.is_empty() {
            (0, 0) // arrow destroyed
        } else {
            (arrows[0].quantity, arrows[0].current_hp)
        }
    };

    let seed = legacy_test_seed();
    let r1 = run(seed);
    let r2 = run(seed);
    assert_eq!(r1, r2, "Same seed must produce identical results");
}

#[test]
fn arrow_impact_partial_damage_reduces_hp() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    // Force exactly 1 damage.
    sim.config.arrow_impact_damage_min = 1;
    sim.config.arrow_impact_damage_max = 1;

    for y in 1..=5 {
        sim.world
            .set(VoxelCoord::new(45, y, 40), VoxelType::GrownPlatform);
    }

    sim.spawn_projectile(VoxelCoord::new(40, 3, 40), VoxelCoord::new(45, 3, 40), None);

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

    let arrows: Vec<_> = sim
        .db
        .ground_piles
        .iter_all()
        .flat_map(|p| {
            sim.db
                .item_stacks
                .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
        })
        .filter(|s| s.kind == inventory::ItemKind::Arrow)
        .collect();
    assert_eq!(arrows.len(), 1, "Arrow should survive with 1 damage");
    assert_eq!(arrows[0].current_hp, 2, "Arrow should have 2/3 HP");
}

#[test]
fn arrow_impact_config_defaults_serde_roundtrip() {
    let config = GameConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let restored: GameConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.arrow_impact_damage_min, 0);
    assert_eq!(restored.arrow_impact_damage_max, 3);
    assert_eq!(restored.durability_worn_pct, 70);
    assert_eq!(restored.durability_damaged_pct, 40);
}

#[test]
fn arrow_impact_config_missing_fields_use_defaults() {
    // Simulate loading a save from before these config fields existed:
    // serialize defaults, strip the new fields, and re-deserialize.
    let config = GameConfig::default();
    let mut val: serde_json::Value = serde_json::to_value(&config).unwrap();
    let obj = val.as_object_mut().unwrap();
    obj.remove("arrow_impact_damage_min");
    obj.remove("arrow_impact_damage_max");
    obj.remove("durability_worn_pct");
    obj.remove("durability_damaged_pct");
    let config: GameConfig = serde_json::from_value(val).unwrap();
    assert_eq!(config.arrow_impact_damage_min, 0);
    assert_eq!(config.arrow_impact_damage_max, 3);
    assert_eq!(config.durability_worn_pct, 70);
    assert_eq!(config.durability_damaged_pct, 40);
}

#[test]
fn arrow_cumulative_damage_across_impacts() {
    // An arrow that took 1 damage (2/3 HP) should break when it takes 2 more.
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
    sim.inv_add_item_with_durability(
        inv_id,
        inventory::ItemKind::Arrow,
        1,
        None,
        None,
        None,
        0,
        2, // current_hp (already damaged once)
        3, // max_hp
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let stack_id = stacks[0].id;

    let mut events = Vec::new();
    // Apply 2 more damage — should break.
    let broke = sim.inv_damage_item(stack_id, 2, &mut events);
    assert!(broke, "Arrow at 2/3 HP should break from 2 damage");
    assert!(sim.db.item_stacks.get(&stack_id).is_none());
    assert!(events.iter().any(
        |e| matches!(&e.kind, SimEventKind::ItemBroken { item_kind, .. }
            if *item_kind == inventory::ItemKind::Arrow)
    ));
}

#[test]
fn damaged_and_full_hp_arrows_stay_separate_in_ground_pile() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
    // Add full-HP arrows.
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 5, None, None);
    // Add damaged arrows (2/3 HP).
    sim.inv_add_item_with_durability(
        inv_id,
        inventory::ItemKind::Arrow,
        3,
        None,
        None,
        None,
        0,
        2, // current_hp
        3, // max_hp
        None,
        None,
    );
    sim.inv_normalize(inv_id);

    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    // Should be 2 separate stacks: one at 3/3 HP, one at 2/3 HP.
    assert_eq!(stacks.len(), 2, "Different HP levels must stay separate");
    let full_hp: u32 = stacks
        .iter()
        .filter(|s| s.current_hp == 3)
        .map(|s| s.quantity)
        .sum();
    let damaged: u32 = stacks
        .iter()
        .filter(|s| s.current_hp == 2)
        .map(|s| s.quantity)
        .sum();
    assert_eq!(full_hp, 5);
    assert_eq!(damaged, 3);
}

#[test]
fn damaged_arrow_survives_serde_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    // Place a damaged arrow in a ground pile.
    let pos = VoxelCoord::new(128, 1, 128);
    let pile_id = sim.ensure_ground_pile(pos);
    let pile_inv = sim.db.ground_piles.get(&pile_id).unwrap().inventory_id;
    sim.inv_add_item_with_durability(
        pile_inv,
        inventory::ItemKind::Arrow,
        1,
        None,
        None,
        None,
        0,
        1, // current_hp — damaged
        3, // max_hp
        None,
        None,
    );

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let restored_pile = restored.db.ground_piles.get(&pile_id).unwrap();
    let stacks = restored
        .db
        .item_stacks
        .by_inventory_id(&restored_pile.inventory_id, tabulosity::QueryOpts::ASC);
    let arrow = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Arrow)
        .expect("Arrow should survive roundtrip");
    assert_eq!(arrow.current_hp, 1, "current_hp should be preserved");
    assert_eq!(arrow.max_hp, 3, "max_hp should be preserved");
}

#[test]
fn arrow_impact_no_damage_when_min_exceeds_max() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.config.arrow_impact_damage_min = 5;
    sim.config.arrow_impact_damage_max = 2; // min > max → no damage

    for y in 1..=5 {
        sim.world
            .set(VoxelCoord::new(45, y, 40), VoxelType::GrownPlatform);
    }

    sim.spawn_projectile(VoxelCoord::new(40, 3, 40), VoxelCoord::new(45, 3, 40), None);

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

    // Arrow should survive at full HP.
    let arrows: Vec<_> = sim
        .db
        .ground_piles
        .iter_all()
        .flat_map(|p| {
            sim.db
                .item_stacks
                .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
        })
        .filter(|s| s.kind == inventory::ItemKind::Arrow)
        .collect();
    assert_eq!(arrows.len(), 1, "Arrow should survive");
    assert_eq!(arrows[0].current_hp, 3, "Arrow should be at full HP");
}

// ---------------------------------------------------------------------------
// Item color (F-item-color)
// ---------------------------------------------------------------------------

/// Helper: build an ItemStack value without inserting into the DB.
fn fake_stack(
    kind: inventory::ItemKind,
    material: Option<inventory::Material>,
    dye_color: Option<inventory::ItemColor>,
) -> crate::db::ItemStack {
    crate::db::ItemStack {
        id: crate::types::ItemStackId(9999),
        inventory_id: crate::types::InventoryId(1),
        kind,
        quantity: 1,
        material,
        quality: 0,
        current_hp: 0,
        max_hp: 0,
        enchantment_id: None,
        owner: None,
        reserved_by: None,
        equipped_slot: None,
        dye_color,
    }
}

#[test]
fn item_color_no_material_returns_default() {
    let sim = test_sim(legacy_test_seed());
    let stack = fake_stack(inventory::ItemKind::Bread, None, None);
    assert_eq!(sim.item_color(&stack), inventory::DEFAULT_ITEM_COLOR);
}

#[test]
fn item_color_wood_material_returns_muted_base() {
    let sim = test_sim(legacy_test_seed());
    let stack = fake_stack(
        inventory::ItemKind::Bow,
        Some(inventory::Material::Oak),
        None,
    );
    let color = sim.item_color(&stack);
    let expected = inventory::Material::Oak.base_color().muted();
    assert_eq!(color, expected);
}

#[test]
fn item_color_dye_overrides_material() {
    let sim = test_sim(legacy_test_seed());
    let dye = inventory::ItemColor::new(200, 50, 50);
    let stack = fake_stack(
        inventory::ItemKind::Tunic,
        Some(inventory::Material::Oak),
        Some(dye),
    );
    // Dyed color should be returned as-is, not muted.
    assert_eq!(sim.item_color(&stack), dye);
}

#[test]
fn item_color_fruit_material_uses_appearance_color_muted() {
    let sim = test_sim(legacy_test_seed());
    let species = sim.db.fruit_species.iter_all().next().unwrap().clone();
    let expected = inventory::ItemColor::from(species.appearance.exterior_color).muted();
    let stack = fake_stack(
        inventory::ItemKind::Fruit,
        Some(inventory::Material::FruitSpecies(species.id)),
        None,
    );
    assert_eq!(sim.item_color(&stack), expected);
}

#[test]
fn item_color_unknown_fruit_species_uses_generic_fallback() {
    let sim = test_sim(legacy_test_seed());
    // Use a fruit species ID that doesn't exist in the DB.
    let bogus_id = crate::fruit::FruitSpeciesId(65535);
    assert!(sim.db.fruit_species.get(&bogus_id).is_none());
    let stack = fake_stack(
        inventory::ItemKind::Fruit,
        Some(inventory::Material::FruitSpecies(bogus_id)),
        None,
    );
    let expected = inventory::Material::FruitSpecies(bogus_id)
        .base_color()
        .muted();
    assert_eq!(sim.item_color(&stack), expected);
}

#[test]
fn inv_normalize_keeps_differently_dyed_stacks_separate() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let inv_id = sim.creature_inv(elf_id);
    // Add two stacks of the same item with different dye colors.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        Some(elf_id),
        None,
        None,
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        Some(elf_id),
        None,
        None,
        0,
        None,
        None,
    );
    // Dye one of them.
    let stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.kind == inventory::ItemKind::Tunic)
        .collect();
    // After normalize, the two undyed tunics should have merged.
    assert_eq!(stacks.len(), 1, "undyed tunics should merge");
    assert_eq!(stacks[0].quantity, 2);
    // Now dye one unit: split it off and apply dye.
    let split_id = sim.inv_split_stack(stacks[0].id, 1).unwrap();
    let dye = inventory::ItemColor::new(200, 0, 0);
    let mut split_row = sim.db.item_stacks.get(&split_id).unwrap();
    split_row.dye_color = Some(dye);
    sim.db.update_item_stack(split_row).unwrap();
    sim.inv_normalize(inv_id);
    // Should now have two separate stacks: one undyed, one dyed.
    let stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.kind == inventory::ItemKind::Tunic)
        .collect();
    assert_eq!(stacks.len(), 2, "dyed and undyed tunics should not merge");
}

#[test]
fn inv_normalize_merges_same_dye_color_stacks() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let inv_id = sim.creature_inv(elf_id);
    let dye = inventory::ItemColor::new(0, 100, 200);
    // Add two stacks with the same dye color.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        Some(elf_id),
        None,
        None,
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        Some(elf_id),
        None,
        None,
        0,
        None,
        None,
    );
    // Dye both.
    let ids: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.kind == inventory::ItemKind::Tunic)
        .map(|s| s.id)
        .collect();
    for id in &ids {
        let mut row = sim.db.item_stacks.get(id).unwrap();
        row.dye_color = Some(dye);
        sim.db.update_item_stack(row).unwrap();
    }
    sim.inv_normalize(inv_id);
    let stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.kind == inventory::ItemKind::Tunic)
        .collect();
    assert_eq!(stacks.len(), 1, "same-dyed tunics should merge");
    assert_eq!(stacks[0].quantity, 2);
    assert_eq!(stacks[0].dye_color, Some(dye));
}

#[test]
fn item_color_each_wood_type_distinct() {
    let sim = test_sim(legacy_test_seed());
    let colors: Vec<_> = inventory::Material::WOOD_TYPES
        .iter()
        .map(|wood| {
            let stack = fake_stack(inventory::ItemKind::Bow, Some(*wood), None);
            (*wood, sim.item_color(&stack))
        })
        .collect();
    for i in 0..colors.len() {
        for j in (i + 1)..colors.len() {
            assert_ne!(
                colors[i].1, colors[j].1,
                "{:?} and {:?} should produce different item colors",
                colors[i].0, colors[j].0
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Weapon item kinds (spear, club)
// ---------------------------------------------------------------------------

/// Spear and Club ItemKind serde roundtrip.
#[test]
fn item_kind_spear_club_serde_roundtrip() {
    for kind in [ItemKind::Spear, ItemKind::Club] {
        let json = serde_json::to_string(&kind).unwrap();
        let restored: ItemKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, restored, "roundtrip failed for {json}");
    }
}

/// Spear and Club have durability configured.
#[test]
fn weapon_durability_configured() {
    let config = crate::config::GameConfig::default();
    assert!(config.item_durability.contains_key(&ItemKind::Spear));
    assert!(config.item_durability.contains_key(&ItemKind::Club));
    assert!(config.item_durability[&ItemKind::Spear] > 0);
    assert!(config.item_durability[&ItemKind::Club] > 0);
}

/// is_melee_weapon returns true for Spear and Club, false for others.
#[test]
fn is_melee_weapon_classification() {
    assert!(ItemKind::Spear.is_melee_weapon());
    assert!(ItemKind::Club.is_melee_weapon());
    assert!(!ItemKind::Bow.is_melee_weapon());
    assert!(!ItemKind::Arrow.is_melee_weapon());
    assert!(!ItemKind::Bread.is_melee_weapon());
    assert!(!ItemKind::Helmet.is_melee_weapon());
}
// ---------------------------------------------------------------------------
// Footwear item kinds (sandals, shoes)
// ---------------------------------------------------------------------------

/// Sandals and Shoes ItemKind serde roundtrip.
#[test]
fn item_kind_sandals_shoes_serde_roundtrip() {
    for kind in [ItemKind::Sandals, ItemKind::Shoes] {
        let json = serde_json::to_string(&kind).unwrap();
        let restored: ItemKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, restored, "roundtrip failed for {json}");
    }
    // Verify exact serialization names (discriminant stability).
    assert_eq!(
        serde_json::to_string(&ItemKind::Sandals).unwrap(),
        "\"Sandals\""
    );
    assert_eq!(
        serde_json::to_string(&ItemKind::Shoes).unwrap(),
        "\"Shoes\""
    );
}

/// Default durability for all three footwear types.
#[test]
fn footwear_durability_defaults() {
    let config = crate::config::GameConfig::default();
    let dur = &config.item_durability;
    assert_eq!(dur[&ItemKind::Sandals], 15, "Sandals should have 15 HP");
    assert_eq!(dur[&ItemKind::Shoes], 20, "Shoes should have 20 HP");
    assert_eq!(dur[&ItemKind::Boots], 30, "Boots (armor) should have 30 HP");
}

// ---------------------------------------------------------------------------
// F-item-quality: quality labels and display
// ---------------------------------------------------------------------------

#[test]
fn quality_label_returns_correct_strings() {
    assert_eq!(inventory::quality_label(-1), Some("Crude"));
    assert_eq!(inventory::quality_label(0), Some("Fine"));
    assert_eq!(inventory::quality_label(1), Some("Superior"));
    // Future tiers and out-of-range values return None.
    assert_eq!(inventory::quality_label(2), None);
    assert_eq!(inventory::quality_label(3), None);
    assert_eq!(inventory::quality_label(-2), None);
}

#[test]
fn item_display_name_shows_quality_prefix() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);

    // Crude bread.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bread,
        1,
        None,
        None,
        None,
        -1,
        None,
        None,
    );
    // Fine bread (quality 0).
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bread,
        1,
        None,
        None,
        None,
        0,
        None,
        None,
    );
    // Superior oak bow.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bow,
        1,
        None,
        None,
        Some(inventory::Material::Oak),
        1,
        None,
        None,
    );

    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let crude_bread = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Bread && s.quality == -1)
        .unwrap();
    let fine_bread = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Bread && s.quality == 0)
        .unwrap();
    let superior_bow = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Bow && s.quality == 1)
        .unwrap();

    assert_eq!(sim.item_display_name(crude_bread), "Crude Bread");
    assert_eq!(sim.item_display_name(fine_bread), "Fine Bread");
    assert_eq!(sim.item_display_name(superior_bow), "Superior Oak Bow");
}

#[test]
fn item_display_name_quality_with_dye_color() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);

    // Crude blue tunic.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Tunic,
        1,
        None,
        None,
        None,
        -1,
        None,
        None,
    );
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    let tunic = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Tunic)
        .unwrap();
    let tunic_id = tunic.id;
    let mut tunic_row2 = sim.db.item_stacks.get(&tunic_id).unwrap();
    tunic_row2.dye_color = Some(inventory::ItemColor::new(50, 70, 180));
    sim.db.update_item_stack(tunic_row2).unwrap();
    let tunic_dyed = sim.db.item_stacks.get(&tunic_id).unwrap();
    assert_eq!(sim.item_display_name(&tunic_dyed), "Crude Blue Tunic");
}

#[test]
fn quality_from_roll_thresholds() {
    use super::crafting::quality_from_roll;
    // Boundary tests per design doc.
    assert_eq!(quality_from_roll(i64::MIN), -1); // far below
    assert_eq!(quality_from_roll(-100), -1);
    assert_eq!(quality_from_roll(0), -1);
    assert_eq!(quality_from_roll(49), -1);
    assert_eq!(quality_from_roll(50), 0); // Fine threshold
    assert_eq!(quality_from_roll(100), 0);
    assert_eq!(quality_from_roll(249), 0);
    assert_eq!(quality_from_roll(250), 1); // Superior threshold
    assert_eq!(quality_from_roll(500), 1);
    assert_eq!(quality_from_roll(i64::MAX), 1);
}

#[test]
fn elf_starting_gear_is_crude() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let inv_id = sim.creature_inv(elf_id);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    // Elves start with bread, bows, and arrows — all should be Crude (-1).
    for stack in &stacks {
        assert_eq!(
            stack.quality,
            -1,
            "starting {} should be Crude (-1), got {}",
            stack.kind.display_name(),
            stack.quality,
        );
    }
    // Sanity check: the elf has at least some items.
    assert!(!stacks.is_empty(), "elf should have starting gear");
}

#[test]
fn initial_equipment_is_crude() {
    // Items added via InitialEquipSpec (activation.rs) should be Crude.
    let mut config = test_config();
    use crate::config::InitialEquipSpec;
    // Add a creature spec with initial equipment.
    if let Some(spec) = config.initial_creatures.first_mut() {
        spec.initial_equipment = vec![vec![InitialEquipSpec {
            item_kind: inventory::ItemKind::Helmet,
            material: Some(Material::Oak),
            dye_color: None,
        }]];
    }
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let mut events = Vec::new();
    sim.spawn_initial_creatures(&mut events);

    // Find any helmets in any creature inventory.
    let helmets: Vec<_> = sim
        .db
        .item_stacks
        .iter_all()
        .filter(|s| s.kind == inventory::ItemKind::Helmet)
        .collect();
    // Filter to only helmets with an owner (equipped starting gear, not
    // dropped/ground items). The helmet in our test config is given to elf 0.
    let owned_helmets: Vec<_> = helmets.iter().filter(|s| s.owner.is_some()).collect();
    assert!(
        !owned_helmets.is_empty(),
        "should have spawned owned helmets"
    );
    for h in &owned_helmets {
        assert_eq!(
            h.quality, -1,
            "initial equipment helmet should be Crude (-1)"
        );
    }
}

#[test]
fn items_different_quality_do_not_stack() {
    let mut sim = test_sim(legacy_test_seed());
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Creature);
    // Add crude and fine bread.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bread,
        3,
        None,
        None,
        None,
        -1,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bread,
        2,
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
    let bread_stacks: Vec<_> = stacks
        .iter()
        .filter(|s| s.kind == inventory::ItemKind::Bread)
        .collect();
    assert_eq!(
        bread_stacks.len(),
        2,
        "crude and fine bread should not merge"
    );
    let crude = bread_stacks.iter().find(|s| s.quality == -1).unwrap();
    let fine = bread_stacks.iter().find(|s| s.quality == 0).unwrap();
    assert_eq!(crude.quantity, 3);
    assert_eq!(fine.quantity, 2);
}

#[test]
fn starting_ground_pile_items_are_crude() {
    let config = test_config();
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let mut events = Vec::new();
    sim.spawn_initial_creatures(&mut events);

    // Check all ground pile items.
    let pile_inv_ids: Vec<_> = sim
        .db
        .ground_piles
        .iter_all()
        .map(|p| p.inventory_id)
        .collect();
    for inv_id in &pile_inv_ids {
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(inv_id, tabulosity::QueryOpts::ASC);
        for stack in &stacks {
            assert_eq!(
                stack.quality,
                -1,
                "ground pile {} should be Crude (-1), got {}",
                stack.kind.display_name(),
                stack.quality,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// inv_move_reserved_items
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn new_sim_has_initial_fruit() {
    // Verify that we can always produce fruit on the home tree,
    // either from worldgen or via ensure_tree_has_fruit.
    let mut sim = test_sim(legacy_test_seed());
    let fruit_pos = ensure_tree_has_fruit(&mut sim);
    let fruits = sim
        .db
        .tree_fruits
        .by_position(&fruit_pos, tabulosity::QueryOpts::ASC);
    assert!(
        !fruits.is_empty(),
        "Tree should have fruit after ensure_tree_has_fruit"
    );
}

#[test]
fn fruit_hangs_below_leaf_voxels() {
    let sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;
    let tree = sim.db.trees.get(&tree_id).unwrap();
    let fruits = sim
        .db
        .tree_fruits
        .by_tree_id(&tree_id, tabulosity::QueryOpts::ASC);
    for tf in &fruits {
        // The leaf above the fruit should be in the tree's leaf_voxels.
        let leaf_above = VoxelCoord::new(tf.position.x, tf.position.y + 1, tf.position.z);
        assert!(
            tree.leaf_voxels.contains(&leaf_above),
            "Fruit at {} should hang below a leaf voxel, but no leaf at {}",
            tf.position,
            leaf_above
        );
    }
}

#[test]
fn fruit_set_in_world_grid() {
    let sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;
    let fruits = sim
        .db
        .tree_fruits
        .by_tree_id(&tree_id, tabulosity::QueryOpts::ASC);
    for tf in &fruits {
        assert_eq!(
            sim.world.get(tf.position),
            VoxelType::Fruit,
            "World should have Fruit voxel at {}",
            tf.position
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
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree_id = sim.player_tree_id;

    // Replace the tree's leaf list with a single known-good leaf that has
    // air below it.  Random worldgen leaf voxels often lack air beneath
    // them, causing `attempt_fruit_spawn` to silently fail every heartbeat.
    {
        let tree = sim.db.trees.get(&tree_id).unwrap();
        let leaf_pos = VoxelCoord::new(tree.position.x + 3, tree.position.y + 8, tree.position.z);
        let needs_species = tree.fruit_species_id.is_none();
        sim.set_voxel(leaf_pos, VoxelType::Leaf);
        let below = VoxelCoord::new(leaf_pos.x, leaf_pos.y - 1, leaf_pos.z);
        sim.set_voxel(below, VoxelType::Air);

        if needs_species {
            let species_id = insert_test_fruit_species(&mut sim);
            let mut t = sim.db.trees.get(&tree_id).unwrap();
            t.leaf_voxels = vec![leaf_pos];
            t.fruit_species_id = Some(species_id);
            let _ = sim.db.update_tree(t);
        } else {
            let mut t = sim.db.trees.get(&tree_id).unwrap();
            t.leaf_voxels = vec![leaf_pos];
            let _ = sim.db.update_tree(t);
        }
    }

    assert_eq!(
        sim.db
            .tree_fruits
            .count_by_tree_id(&tree_id, tabulosity::QueryOpts::ASC),
        0,
        "Should start with no fruit when initial_attempts = 0"
    );

    // Step past several heartbeats (interval = 10000 ticks).
    sim.step(&[], 50000);

    assert!(
        sim.db
            .tree_fruits
            .count_by_tree_id(&tree_id, tabulosity::QueryOpts::ASC)
            > 0,
        "Fruit should grow during tree heartbeats"
    );
}

#[test]
fn fruit_respects_max_count() {
    let mut config = test_config();
    config.fruit_max_per_tree = 3;
    config.fruit_initial_attempts = 100; // Many attempts, but max is 3.
    config.fruit_production_rate_ppm = 1_000_000;
    let sim = SimState::with_config(legacy_test_seed(), config);
    let fruit_count = sim
        .db
        .tree_fruits
        .count_by_tree_id(&sim.player_tree_id, tabulosity::QueryOpts::ASC);

    assert!(
        fruit_count <= 3,
        "Fruit count {} should not exceed max 3",
        fruit_count
    );
}

#[test]
fn fruit_deterministic() {
    let seed = legacy_test_seed();
    let sim_a = test_sim(seed);
    let sim_b = test_sim(seed);
    let fruits_a: Vec<_> = sim_a
        .db
        .tree_fruits
        .by_tree_id(&sim_a.player_tree_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|tf| tf.position)
        .collect();
    let fruits_b: Vec<_> = sim_b
        .db
        .tree_fruits
        .by_tree_id(&sim_b.player_tree_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|tf| tf.position)
        .collect();
    assert_eq!(fruits_a, fruits_b);
}

#[test]
fn tree_has_fruit_species_assigned() {
    let sim = test_sim(legacy_test_seed());
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
    let sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;
    let tree = sim.db.trees.get(&tree_id).unwrap();
    let fruits = sim
        .db
        .tree_fruits
        .by_tree_id(&tree_id, tabulosity::QueryOpts::ASC);
    // Every TreeFruit row should have a species that matches the tree's.
    let tree_species = tree
        .fruit_species_id
        .expect("Home tree should always have a fruit species assigned");
    for tf in &fruits {
        assert_eq!(
            tf.species_id, tree_species,
            "Fruit species should match tree species"
        );
    }
}

#[test]
fn fruit_species_at_returns_species() {
    let sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;
    let fruits = sim
        .db
        .tree_fruits
        .by_tree_id(&tree_id, tabulosity::QueryOpts::ASC);
    if let Some(first_fruit) = fruits.first() {
        let species = sim.fruit_species_at(first_fruit.position);
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
fn tree_fruit_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    ensure_tree_has_fruit(&mut sim);
    let tree_id = sim.player_tree_id;

    let json = sim.to_json().unwrap();
    let loaded = SimState::from_json(&json).unwrap();

    // TreeFruit rows should survive roundtrip.
    let orig_fruits: Vec<_> = sim
        .db
        .tree_fruits
        .by_tree_id(&tree_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|tf| (tf.position, tf.species_id))
        .collect();
    let loaded_fruits: Vec<_> = loaded
        .db
        .tree_fruits
        .by_tree_id(&tree_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|tf| (tf.position, tf.species_id))
        .collect();
    assert_eq!(
        orig_fruits, loaded_fruits,
        "TreeFruit rows should survive roundtrip"
    );

    // Tree's fruit species should survive too.
    let tree = sim.db.trees.get(&tree_id).unwrap();
    let loaded_tree = loaded.db.trees.get(&tree_id).unwrap();
    assert_eq!(
        loaded_tree.fruit_species_id, tree.fruit_species_id,
        "Tree fruit_species_id should survive roundtrip"
    );
}

#[test]
fn harvest_fruit_carries_species_material() {
    let mut sim = test_sim(legacy_test_seed());
    let fruit_pos = ensure_tree_has_fruit(&mut sim);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let tree_species = tree.fruit_species_id.unwrap();

    // Spawn an elf near the fruit.
    let elf_nav = sim.nav_graph.find_nearest_node(fruit_pos, 10).unwrap();
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

    // The fruit should be gone from world and TreeFruit table.
    assert_eq!(sim.world.get(fruit_pos), VoxelType::Air);
    assert!(
        sim.db
            .tree_fruits
            .by_position(&fruit_pos, tabulosity::QueryOpts::ASC)
            .is_empty()
    );

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
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    // Replace the tree's leaf list with a single known-good leaf that has
    // air below it.  Random worldgen leaf voxels often lack air beneath
    // them, causing `attempt_fruit_spawn` to silently fail every heartbeat.
    {
        let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
        let leaf_pos = VoxelCoord::new(tree.position.x + 3, tree.position.y + 8, tree.position.z);
        let needs_species = tree.fruit_species_id.is_none();
        sim.set_voxel(leaf_pos, VoxelType::Leaf);
        let below = VoxelCoord::new(leaf_pos.x, leaf_pos.y - 1, leaf_pos.z);
        sim.set_voxel(below, VoxelType::Air);

        if needs_species {
            let species_id = insert_test_fruit_species(&mut sim);
            let mut t = sim.db.trees.get(&sim.player_tree_id).unwrap();
            t.leaf_voxels = vec![leaf_pos];
            t.fruit_species_id = Some(species_id);
            let _ = sim.db.update_tree(t);
        } else {
            let mut t = sim.db.trees.get(&sim.player_tree_id).unwrap();
            t.leaf_voxels = vec![leaf_pos];
            let _ = sim.db.update_tree(t);
        }
    }

    assert_eq!(
        sim.db
            .tree_fruits
            .count_by_tree_id(&sim.player_tree_id, tabulosity::QueryOpts::ASC),
        0,
        "Should start with no fruit"
    );

    // Step past heartbeats to grow fruit.
    sim.step(&[], 50000);

    let fruits = sim
        .db
        .tree_fruits
        .by_tree_id(&sim.player_tree_id, tabulosity::QueryOpts::ASC);
    assert!(!fruits.is_empty(), "Should have grown some fruit");
    // Every fruit should have a valid species.
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let tree_species = tree.fruit_species_id.unwrap();
    for tf in &fruits {
        assert_eq!(
            tf.species_id, tree_species,
            "Heartbeat-grown fruit at {} should have correct species",
            tf.position
        );
    }
}

#[test]
fn attempt_fruit_spawn_no_op_when_species_none() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;
    // Clear the tree's fruit species.
    let mut tree = sim.db.trees.get(&tree_id).unwrap();
    tree.fruit_species_id = None;
    sim.db.update_tree(tree).unwrap();

    let before = sim.db.tree_fruits.len();
    let spawned = sim.attempt_fruit_spawn(tree_id);
    assert!(!spawned, "Should not spawn fruit when species is None");
    assert_eq!(sim.db.tree_fruits.len(), before);
}

#[test]
fn tree_fruit_cascade_delete_on_tree_removal() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;
    ensure_tree_has_fruit(&mut sim);

    let fruit_count = sim
        .db
        .tree_fruits
        .count_by_tree_id(&tree_id, tabulosity::QueryOpts::ASC);
    assert!(fruit_count > 0, "Tree should have fruit before removal");

    // Remove the great_tree_info first (FK child of tree).
    let _ = sim.db.remove_great_tree_info(&tree_id);
    // Remove the tree — should cascade to TreeFruit rows.
    let _ = sim.db.remove_tree(&tree_id);

    assert_eq!(
        sim.db
            .tree_fruits
            .count_by_tree_id(&tree_id, tabulosity::QueryOpts::ASC),
        0,
        "TreeFruit rows should be cascade-deleted when tree is removed"
    );
}

#[test]
fn tree_fruit_position_unique_index_prevents_duplicates() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;
    let species_id = sim
        .db
        .trees
        .get(&tree_id)
        .unwrap()
        .fruit_species_id
        .unwrap();

    let pos = VoxelCoord::new(10, 60, 10);
    let result1 = sim.db.insert_tree_fruit_auto(|id| crate::db::TreeFruit {
        id,
        tree_id,
        position: pos,
        species_id,
    });
    assert!(result1.is_ok(), "First insert should succeed");

    let result2 = sim.db.insert_tree_fruit_auto(|id| crate::db::TreeFruit {
        id,
        tree_id,
        position: pos,
        species_id,
    });
    assert!(
        result2.is_err(),
        "Duplicate position should be rejected by unique index"
    );
}

#[test]
fn fruit_species_at_returns_none_for_empty_position() {
    let sim = test_sim(legacy_test_seed());
    let bogus = VoxelCoord::new(0, 0, 0);
    assert!(
        sim.fruit_species_at(bogus).is_none(),
        "Should return None for a position with no fruit"
    );
}

#[test]
fn wild_fruit_initial_spawn_on_lesser_trees() {
    // A full sim with fruit-bearing lesser trees should have TreeFruit rows
    // on lesser trees after the initial fruit spawn fast-forward.
    let mut config = test_config();
    config.lesser_trees.count = 10;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;
    config.lesser_trees.fruit_bearing_fraction = 1.0;
    config.fruit_initial_attempts = 12;
    config.fruit_production_rate_ppm = 1_000_000; // Always spawn
    config.fruit_max_per_tree = 20;
    let sim = SimState::with_config(fresh_test_seed(), config);

    // Find fruit-bearing lesser trees.
    let lesser_with_fruit: Vec<_> = sim
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != sim.player_tree_id && t.fruit_species_id.is_some())
        .collect();
    assert!(
        !lesser_with_fruit.is_empty(),
        "Should have fruit-bearing lesser trees"
    );

    // At least some lesser trees should have spawned fruit.
    let mut any_has_fruit = false;
    for tree in &lesser_with_fruit {
        let count = sim
            .db
            .tree_fruits
            .count_by_tree_id(&tree.id, tabulosity::QueryOpts::ASC);
        if count > 0 {
            any_has_fruit = true;
        }
    }
    assert!(
        any_has_fruit,
        "At least one lesser tree should have initial fruit"
    );
}

#[test]
fn wild_fruit_regrows_on_lesser_trees() {
    // Fruit should regrow on lesser trees during tree heartbeats.
    let mut config = test_config();
    config.lesser_trees.count = 3;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;
    config.lesser_trees.fruit_bearing_fraction = 1.0;
    config.fruit_initial_attempts = 0; // No initial fruit
    config.fruit_production_rate_ppm = 1_000_000; // Always spawn
    config.fruit_max_per_tree = 20;
    let mut sim = SimState::with_config(fresh_test_seed(), config);

    // Find a fruit-bearing lesser tree (must exist with count=3 and fraction=1.0).
    let lesser_id = sim
        .db
        .trees
        .iter_all()
        .find(|t| t.id != sim.player_tree_id && t.fruit_species_id.is_some())
        .expect("Should have at least one fruit-bearing lesser tree")
        .id;

    // Ensure the tree has exactly one leaf voxel with air below it for fruit
    // placement.  The fruit-spawn code picks a *random* leaf, so if worldgen
    // produced many leaves (most without air below), the test becomes flaky.
    // Replacing the leaf list with a single known-good leaf eliminates that.
    // Use the tree's own x/z (always in-bounds) and fixed y values well above
    // any tree geometry to avoid OOB when the tree is near a world edge.
    {
        let tree = sim.db.trees.get(&lesser_id).unwrap();
        let leaf_pos = VoxelCoord::new(tree.position.x, 10, tree.position.z);
        sim.set_voxel(leaf_pos, VoxelType::Leaf);
        let below = VoxelCoord::new(tree.position.x, 9, tree.position.z);
        sim.set_voxel(below, VoxelType::Air);

        let mut t = sim.db.trees.get(&lesser_id).unwrap();
        t.leaf_voxels = vec![leaf_pos];
        let _ = sim.db.update_tree(t);
    }

    // No fruit initially.
    assert_eq!(
        sim.db
            .tree_fruits
            .count_by_tree_id(&lesser_id, tabulosity::QueryOpts::ASC),
        0,
    );

    // Step past heartbeats.
    sim.step(&[], 50000);

    assert!(
        sim.db
            .tree_fruits
            .count_by_tree_id(&lesser_id, tabulosity::QueryOpts::ASC)
            > 0,
        "Lesser tree should have grown fruit during heartbeats"
    );
}

#[test]
fn add_creature_item() {
    let mut sim = test_sim(legacy_test_seed());
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
    let mut sim = test_sim(legacy_test_seed());
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
fn death_drop_preserves_preexisting_pile_items() {
    // If a ground pile already exists at the death position with items
    // from another source, those items must not be affected.
    let mut sim = test_sim(legacy_test_seed());
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
