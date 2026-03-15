// Inventory operations — item stacks, reservations, equipment, and durability.
//
// All `inv_*` methods for manipulating tabulosity inventory tables: adding,
// removing, reserving, transferring, equipping, and damaging items. These
// are low-level operations called by crafting, logistics, needs, and combat.
//
// See also: `inventory.rs` (ItemKind enum), `db.rs` (inventory table schema),
// `logistics.rs` (haul-driven transfers), `crafting.rs` (recipe I/O).
use super::*;
use crate::building;
use crate::event::{SimEvent, SimEventKind};
use crate::inventory;
use std::collections::BTreeMap;

impl SimState {
    /// Create a new Inventory row and return its ID.
    pub(crate) fn create_inventory(
        &mut self,
        owner_kind: crate::db::InventoryOwnerKind,
    ) -> InventoryId {
        self.db
            .inventories
            .insert_auto_no_fk(|id| crate::db::Inventory { id, owner_kind })
            .unwrap()
    }

    /// Create a music composition for a construction project. Derives seed
    /// and generation parameters from the sim PRNG. Returns the composition ID.
    pub(crate) fn create_composition(&mut self, voxel_count: usize) -> CompositionId {
        use crate::db::{CompositionStatus, MusicComposition};

        let seed = self.rng.next_u64();

        // Build duration = voxel_count × ticks_per_voxel / 1000 (seconds at 1x).
        let build_ms = (voxel_count as u64 * self.config.build_work_ticks_per_voxel) as u32;
        let build_secs = build_ms as f32 / 1000.0;

        // Pick section count so that the typical grid length for that many
        // sections would need a BPM within the Palestrina range (60–96) to
        // match the build duration.
        //
        // Typical eighth-note beat counts per section count (from structure.rs):
        //   1 section  ≈  55 beats  → duration range at 60–96 BPM: 17–28s
        //   2 sections ≈ 125 beats  → 39–63s
        //   3 sections ≈ 195 beats  → 61–98s
        //   4 sections ≈ 270 beats  → 84–135s
        //
        // For each candidate, the ideal BPM would be:
        //   bpm = typical_beats * 30 / build_secs
        // We pick the section count whose ideal BPM is closest to the middle
        // of the range (78 BPM).
        const TYPICAL_BEATS: &[(u8, f32)] = &[(1, 55.0), (2, 125.0), (3, 195.0), (4, 270.0)];
        let mut best_sections = 1u8;
        let mut best_dist = f32::MAX;
        for &(s, beats) in TYPICAL_BEATS {
            let ideal_bpm = beats * 30.0 / build_secs;
            let dist = (ideal_bpm - 78.0).abs();
            if dist < best_dist {
                best_dist = dist;
                best_sections = s;
            }
        }

        // Random mode (0-5) and brightness (0.2-0.8).
        let mode_index = (self.rng.next_u64() % 6) as u8;
        let brightness = 0.2 + (self.rng.next_u64() % 600) as f32 / 1000.0;
        // SA budget scaled with piece length (longer pieces benefit more).
        let sa_iterations = match best_sections {
            1 => 2000,
            2 => 3000,
            _ => 5000,
        };

        self.db
            .music_compositions
            .insert_auto_no_fk(|id| MusicComposition {
                id,
                seed,
                sections: best_sections,
                mode_index,
                brightness,
                sa_iterations,
                target_duration_ms: build_ms,
                requested_tick: self.tick,
                build_started: false,
                status: CompositionStatus::Pending,
            })
            .unwrap()
    }

    /// Add items to an inventory. Inserts a new stack, then calls
    /// `inv_normalize` to consolidate with any existing matching stacks.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn inv_add_item(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
        owner: Option<CreatureId>,
        reserved_by: Option<TaskId>,
        material: Option<inventory::Material>,
        quality: i32,
        enchantment_id: Option<crate::types::EnchantmentId>,
        equipped_slot: Option<inventory::EquipSlot>,
    ) {
        // Look up default durability from config when creating fresh items.
        let max_hp = self.config.item_durability.get(&kind).copied().unwrap_or(0);
        let current_hp = max_hp;
        let _ = self
            .db
            .item_stacks
            .insert_auto_no_fk(|id| crate::db::ItemStack {
                id,
                inventory_id: inv_id,
                kind,
                quantity,
                material,
                quality,
                current_hp,
                max_hp,
                enchantment_id,
                owner,
                reserved_by,
                equipped_slot,
            });
        self.inv_normalize(inv_id);
    }

    /// Add an item with explicit durability values (current_hp / max_hp).
    /// Used in tests to create items with specific durability state.
    /// For transfers between inventories, prefer `inv_move_stack`.
    #[allow(clippy::too_many_arguments, dead_code)] // Used by tests; future production use for loot drops and crafted items with durability
    pub(crate) fn inv_add_item_with_durability(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
        owner: Option<CreatureId>,
        reserved_by: Option<TaskId>,
        material: Option<inventory::Material>,
        quality: i32,
        current_hp: i32,
        max_hp: i32,
        enchantment_id: Option<crate::types::EnchantmentId>,
        equipped_slot: Option<inventory::EquipSlot>,
    ) {
        let _ = self
            .db
            .item_stacks
            .insert_auto_no_fk(|id| crate::db::ItemStack {
                id,
                inventory_id: inv_id,
                kind,
                quantity,
                material,
                quality,
                current_hp,
                max_hp,
                enchantment_id,
                owner,
                reserved_by,
                equipped_slot,
            });
        self.inv_normalize(inv_id);
    }

    /// Convenience wrapper for adding items with no material, quality, or enchantment.
    pub(crate) fn inv_add_simple_item(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
        owner: Option<CreatureId>,
        reserved_by: Option<TaskId>,
    ) {
        self.inv_add_item(
            inv_id,
            kind,
            quantity,
            owner,
            reserved_by,
            None,
            0,
            None,
            None,
        )
    }

    /// Move all item stacks from `src` into `dst`, then normalize `dst` to
    /// consolidate matching stacks. The source inventory's stacks are deleted
    /// but the Inventory row itself is not removed — the caller decides
    /// whether to clean it up.
    pub(crate) fn inv_merge(&mut self, src: InventoryId, dst: InventoryId) {
        let stacks: Vec<crate::db::ItemStack> = self
            .db
            .item_stacks
            .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
        for stack in stacks {
            let mut moved = stack;
            moved.inventory_id = dst;
            let _ = self.db.item_stacks.update_no_fk(moved);
        }
        self.inv_normalize(dst);
    }

    /// Count the total quantity of a given item kind in an inventory,
    /// filtered by material.
    pub fn inv_item_count(
        &self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
    ) -> u32 {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.kind == kind && filter.matches(s.material))
            .map(|s| s.quantity)
            .sum()
    }

    /// Count items of a given kind owned by a specific creature.
    pub(crate) fn inv_count_owned(
        &self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        owner: CreatureId,
    ) -> u32 {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.kind == kind && s.owner == Some(owner))
            .map(|s| s.quantity)
            .sum()
    }

    /// Count items of a given kind that are owned by the creature or unowned,
    /// filtered by material. Used for military equipment satisfaction checks
    /// where both personal and unowned items count.
    pub(crate) fn inv_count_owned_or_unowned(
        &self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
        creature_id: CreatureId,
    ) -> u32 {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| {
                s.kind == kind
                    && (s.owner == Some(creature_id) || s.owner.is_none())
                    && filter.matches(s.material)
            })
            .map(|s| s.quantity)
            .sum()
    }

    /// Count unreserved items of the given kind, filtered by material.
    pub(crate) fn inv_unreserved_item_count(
        &self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
    ) -> u32 {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.kind == kind && s.reserved_by.is_none() && filter.matches(s.material))
            .map(|s| s.quantity)
            .sum()
    }

    /// Remove up to `quantity` items of the given kind owned by a creature.
    pub(crate) fn inv_remove_owned_item(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        owner: CreatureId,
        quantity: u32,
    ) -> u32 {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut removed = 0u32;
        for stack in &stacks {
            if stack.kind == kind && stack.owner == Some(owner) && remaining > 0 {
                let take = remaining.min(stack.quantity);
                let new_qty = stack.quantity - take;
                if new_qty == 0 {
                    let _ = self.db.item_stacks.remove_no_fk(&stack.id);
                } else {
                    let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                        s.quantity = new_qty;
                    });
                }
                remaining -= take;
                removed += take;
            }
        }
        removed
    }

    /// Reserve up to `quantity` unreserved items of the given kind for a task,
    /// filtered by material. Under `Any` filter, locks in a single material on
    /// the first matching stack (avoids mixed-material hauls). Returns the
    /// material of the reserved stacks.
    pub(crate) fn inv_reserve_items(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
        quantity: u32,
        task_id: TaskId,
    ) -> Option<inventory::Material> {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut locked_material: Option<Option<inventory::Material>> = None;
        for stack in &stacks {
            if stack.kind != kind || stack.reserved_by.is_some() || remaining == 0 {
                continue;
            }
            if !filter.matches(stack.material) {
                continue;
            }
            // Single-material lock: once we pick the first stack's material,
            // only reserve stacks with the same material.
            match locked_material {
                None => locked_material = Some(stack.material),
                Some(locked) if locked != stack.material => continue,
                _ => {}
            }
            let take = remaining.min(stack.quantity);
            if take == stack.quantity {
                // Reserve the entire stack — changes indexed field.
                let mut s = stack.clone();
                s.reserved_by = Some(task_id);
                let _ = self.db.item_stacks.update_no_fk(s);
            } else {
                // Split: reduce this stack and create a new reserved stack.
                let new_qty = stack.quantity - take;
                let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                    s.quantity = new_qty;
                });
                let mat = stack.material;
                let qual = stack.quality;
                let chp = stack.current_hp;
                let mhp = stack.max_hp;
                let ench = stack.enchantment_id;
                let own = stack.owner;
                let _ = self
                    .db
                    .item_stacks
                    .insert_auto_no_fk(|id| crate::db::ItemStack {
                        id,
                        inventory_id: inv_id,
                        kind,
                        quantity: take,
                        material: mat,
                        quality: qual,
                        current_hp: chp,
                        max_hp: mhp,
                        enchantment_id: ench,
                        owner: own,
                        reserved_by: Some(task_id),
                        equipped_slot: None,
                    });
            }
            remaining -= take;
        }
        locked_material.flatten()
    }

    /// Clear all reservations for a task, then re-merge matching stacks.
    pub(crate) fn inv_clear_reservations(&mut self, inv_id: InventoryId, task_id: TaskId) {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        for stack in &stacks {
            if stack.reserved_by == Some(task_id) {
                let mut s = stack.clone();
                s.reserved_by = None;
                let _ = self.db.item_stacks.update_no_fk(s);
            }
        }
        self.inv_normalize(inv_id);
    }

    /// Remove up to `quantity` items reserved by a specific task.
    pub(crate) fn inv_remove_reserved_items(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
        task_id: TaskId,
    ) -> u32 {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut removed = 0u32;
        for stack in &stacks {
            if stack.kind == kind && stack.reserved_by == Some(task_id) && remaining > 0 {
                let take = remaining.min(stack.quantity);
                let new_qty = stack.quantity - take;
                if new_qty == 0 {
                    let _ = self.db.item_stacks.remove_no_fk(&stack.id);
                } else {
                    let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                        s.quantity = new_qty;
                    });
                }
                remaining -= take;
                removed += take;
            }
        }
        removed
    }

    /// Count unowned (`owner == None`) and unreserved items, filtered by material.
    pub(crate) fn inv_count_unowned_unreserved(
        &self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
    ) -> u32 {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| {
                s.kind == kind
                    && s.owner.is_none()
                    && s.reserved_by.is_none()
                    && filter.matches(s.material)
            })
            .map(|s| s.quantity)
            .sum()
    }

    /// Reserve up to `quantity` unowned unreserved items for a task, filtered
    /// by material. Single-material lock applies (same as `inv_reserve_items`).
    pub(crate) fn inv_reserve_unowned_items(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
        quantity: u32,
        task_id: TaskId,
    ) -> u32 {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut reserved = 0u32;
        let mut locked_material: Option<Option<inventory::Material>> = None;
        for stack in &stacks {
            if stack.kind != kind
                || stack.owner.is_some()
                || stack.reserved_by.is_some()
                || remaining == 0
            {
                continue;
            }
            if !filter.matches(stack.material) {
                continue;
            }
            match locked_material {
                None => locked_material = Some(stack.material),
                Some(locked) if locked != stack.material => continue,
                _ => {}
            }
            let take = remaining.min(stack.quantity);
            if take == stack.quantity {
                let mut s = stack.clone();
                s.reserved_by = Some(task_id);
                let _ = self.db.item_stacks.update_no_fk(s);
            } else {
                let new_qty = stack.quantity - take;
                let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                    s.quantity = new_qty;
                });
                let mat = stack.material;
                let qual = stack.quality;
                let chp = stack.current_hp;
                let mhp = stack.max_hp;
                let ench = stack.enchantment_id;
                let _ = self
                    .db
                    .item_stacks
                    .insert_auto_no_fk(|id| crate::db::ItemStack {
                        id,
                        inventory_id: inv_id,
                        kind,
                        quantity: take,
                        material: mat,
                        quality: qual,
                        current_hp: chp,
                        max_hp: mhp,
                        enchantment_id: ench,
                        owner: None,
                        reserved_by: Some(task_id),
                        equipped_slot: None,
                    });
            }
            remaining -= take;
            reserved += take;
        }
        reserved
    }

    /// Consolidate matching stacks within an inventory. Two stacks are
    /// mergeable when they agree on all properties: kind, material, quality,
    /// current_hp, max_hp, enchantment_id, owner, reserved_by, and
    /// equipped_slot. This is the single source of truth for stack-merging
    /// criteria — called after any operation that may create mergeable stacks
    /// (add, move, split, reservation changes, etc.).
    pub(crate) fn inv_normalize(&mut self, inv_id: InventoryId) {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        // Collect merge groups keyed by all stackability fields.
        type MergeKey = (
            inventory::ItemKind,
            Option<inventory::Material>,
            i32,
            i32,
            i32,
            Option<crate::types::EnchantmentId>,
            Option<CreatureId>,
            Option<TaskId>,
            Option<inventory::EquipSlot>,
        );
        type MergeVal = (ItemStackId, u32, Vec<ItemStackId>);
        let mut groups: BTreeMap<MergeKey, MergeVal> = BTreeMap::new();
        for stack in &stacks {
            let key = (
                stack.kind,
                stack.material,
                stack.quality,
                stack.current_hp,
                stack.max_hp,
                stack.enchantment_id,
                stack.owner,
                stack.reserved_by,
                stack.equipped_slot,
            );
            let entry = groups.entry(key).or_insert((stack.id, 0, Vec::new()));
            entry.1 += stack.quantity;
            if stack.id != entry.0 {
                entry.2.push(stack.id);
            }
        }
        for (primary_id, total_qty, duplicates) in groups.values() {
            if !duplicates.is_empty() {
                let qty = *total_qty;
                let _ = self.db.item_stacks.modify_unchecked(primary_id, |s| {
                    s.quantity = qty;
                });
                for dup_id in duplicates {
                    let _ = self.db.item_stacks.remove_no_fk(dup_id);
                }
            }
        }
    }

    /// Split `quantity` items off an existing stack, preserving all properties
    /// (material, quality, current_hp, max_hp, enchantment, owner, reserved_by)
    /// except `equipped_slot` which is always `None` on the new stack.
    ///
    /// - If `quantity == 0`: returns `None`.
    /// - If `quantity >= stack.quantity`: returns `Some(stack_id)` (whole stack,
    ///   equipped_slot unchanged).
    /// - Otherwise: decrements the original stack's quantity and inserts a new
    ///   stack with the split quantity. Returns the new stack's ID. Does NOT
    ///   normalize — the caller may want to modify the split stack first.
    pub(crate) fn inv_split_stack(
        &mut self,
        stack_id: ItemStackId,
        quantity: u32,
    ) -> Option<ItemStackId> {
        if quantity == 0 {
            return None;
        }
        let stack = self.db.item_stacks.get(&stack_id)?;
        if quantity >= stack.quantity {
            return Some(stack_id);
        }
        let new_qty = stack.quantity - quantity;
        // Capture properties before mutating.
        let inv_id = stack.inventory_id;
        let kind = stack.kind;
        let material = stack.material;
        let quality = stack.quality;
        let current_hp = stack.current_hp;
        let max_hp = stack.max_hp;
        let enchantment_id = stack.enchantment_id;
        let owner = stack.owner;
        let reserved_by = stack.reserved_by;
        // Shrink original stack.
        let _ = self
            .db
            .item_stacks
            .modify_unchecked(&stack_id, |s| s.quantity = new_qty);
        // Insert new stack with split quantity.
        let new_id = self
            .db
            .item_stacks
            .insert_auto_no_fk(|id| crate::db::ItemStack {
                id,
                inventory_id: inv_id,
                kind,
                quantity,
                material,
                quality,
                current_hp,
                max_hp,
                enchantment_id,
                owner,
                reserved_by,
                equipped_slot: None,
            })
            .unwrap();
        Some(new_id)
    }

    /// Move `quantity` items from `stack_id` to `dst` inventory, preserving
    /// all properties (material, quality, durability, enchantment, owner,
    /// reserved_by). Uses `inv_split_stack` when moving a partial stack.
    ///
    /// - If `quantity == 0`: returns `None`.
    /// - If `quantity >= stack.quantity`: moves the entire stack.
    /// - Otherwise: splits off `quantity` items and moves the new stack.
    ///
    /// The moved stack is normalized into `dst` (may merge with existing
    /// matching stacks). Returns the ID of the moved stack (post-normalize
    /// it may have been merged away, so the ID may no longer exist).
    pub(crate) fn inv_move_stack(
        &mut self,
        stack_id: ItemStackId,
        quantity: u32,
        dst: InventoryId,
    ) -> Option<ItemStackId> {
        let split_id = self.inv_split_stack(stack_id, quantity)?;
        if let Some(mut stack) = self.db.item_stacks.get(&split_id) {
            stack.inventory_id = dst;
            let _ = self.db.item_stacks.update_no_fk(stack);
        }
        self.inv_normalize(dst);
        Some(split_id)
    }

    /// Move up to `quantity` items from `src` to `dst`, filtered by kind and
    /// material. Preserves all properties (quality, durability, enchantment,
    /// owner, reserved_by).
    ///
    /// - `kind`: if `Some`, only move items of this kind. If `None`, any kind.
    /// - `material`: if `Some(m)`, only move items with that exact material
    ///   (use `Some(None)` for unmaterialed items). If `None`, any material.
    /// - `quantity`: if `Some(n)`, move at most `n` items. If `None`, move all
    ///   matching items.
    ///
    /// Returns the total number of items moved.
    pub(crate) fn inv_move_items(
        &mut self,
        src: InventoryId,
        dst: InventoryId,
        kind: Option<inventory::ItemKind>,
        material: Option<Option<inventory::Material>>,
        quantity: Option<u32>,
    ) -> u32 {
        let stacks: Vec<crate::db::ItemStack> = self
            .db
            .item_stacks
            .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity.unwrap_or(u32::MAX);
        let mut moved = 0u32;
        for stack in &stacks {
            if remaining == 0 {
                break;
            }
            if let Some(k) = kind
                && stack.kind != k
            {
                continue;
            }
            if let Some(m) = material
                && stack.material != m
            {
                continue;
            }
            let take = remaining.min(stack.quantity);
            self.inv_move_stack(stack.id, take, dst);
            remaining -= take;
            moved += take;
        }
        moved
    }

    /// Query the item equipped in a specific slot of an inventory.
    /// Uses the filtered unique compound index `equipped_inv_slot` for O(1) lookup.
    pub(crate) fn inv_equipped_in_slot(
        &self,
        inv_id: InventoryId,
        slot: inventory::EquipSlot,
    ) -> Option<crate::db::ItemStack> {
        self.db
            .item_stacks
            .by_equipped_inv_slot(&inv_id, &Some(slot), tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
    }

    /// Equip a quantity-1 clothing item by setting its `equipped_slot`.
    ///
    /// Returns `false` if: the item is not clothing, the slot is already
    /// occupied, or the stack has quantity > 1 (must split first).
    #[allow(dead_code)] // Used by tests and future manual equip commands
    pub(crate) fn inv_equip_item(&mut self, stack_id: ItemStackId) -> bool {
        let stack = match self.db.item_stacks.get(&stack_id) {
            Some(s) => s,
            None => return false,
        };
        let slot = match stack.kind.equip_slot() {
            Some(s) => s,
            None => return false,
        };
        if stack.quantity > 1 {
            return false;
        }
        if self
            .inv_equipped_in_slot(stack.inventory_id, slot)
            .is_some()
        {
            return false;
        }
        let mut updated = stack;
        updated.equipped_slot = Some(slot);
        let _ = self.db.item_stacks.update_no_fk(updated);
        true
    }

    /// Unequip the item in a slot, clearing `equipped_slot` and normalizing
    /// the inventory (the unequipped item may merge with an existing stack).
    /// Returns the stack ID of the now-unequipped item, if any.
    #[allow(dead_code)] // Used by tests and future manual equip commands
    pub(crate) fn inv_unequip_slot(
        &mut self,
        inv_id: InventoryId,
        slot: inventory::EquipSlot,
    ) -> Option<ItemStackId> {
        let stack = self.inv_equipped_in_slot(inv_id, slot)?;
        let stack_id = stack.id;
        let mut updated = stack;
        updated.equipped_slot = None;
        let _ = self.db.item_stacks.update_no_fk(updated);
        self.inv_normalize(inv_id);
        Some(stack_id)
    }

    /// Reduce one item's `current_hp` by `amount`. If the item has no
    /// durability tracking (`max_hp == 0`) or `amount <= 0`, this is a no-op
    /// and returns `false`. If `current_hp` reaches 0, the item breaks: it is
    /// removed from the inventory and an `ItemBroken` event is emitted.
    /// Returns `true` if the item broke.
    ///
    /// For stacks with quantity > 1, one item is split off before applying
    /// damage so that the rest of the stack retains its original HP.
    pub(crate) fn inv_damage_item(
        &mut self,
        stack_id: ItemStackId,
        amount: i32,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        if amount <= 0 {
            return false;
        }
        let stack = match self.db.item_stacks.get(&stack_id) {
            Some(s) => s,
            None => return false,
        };
        // No durability tracking — indestructible.
        if stack.max_hp == 0 {
            return false;
        }
        // For multi-item stacks, split off one item so the rest keep full HP.
        let target_id = if stack.quantity > 1 {
            // split_stack(1) returns a new stack with qty=1.
            match self.inv_split_stack(stack_id, 1) {
                Some(id) => id,
                None => return false,
            }
        } else {
            stack_id
        };
        // Re-fetch the (possibly split) stack.
        let target = match self.db.item_stacks.get(&target_id) {
            Some(s) => s,
            None => return false,
        };
        let new_hp = (target.current_hp - amount).max(0);
        if new_hp > 0 {
            // Item damaged but not broken.
            let _ = self
                .db
                .item_stacks
                .modify_unchecked(&target_id, |s| s.current_hp = new_hp);
            return false;
        }
        // Item broke — capture fields before removing.
        let item_kind = target.kind;
        let material = target.material;
        let owner = target.owner;
        let inv_id = target.inventory_id;
        let _ = self.db.item_stacks.remove_no_fk(&target_id);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::ItemBroken {
                item_kind,
                material,
                owner,
            },
        });
        // Normalize in case the split created a mergeable state.
        self.inv_normalize(inv_id);
        true
    }

    /// Get the inventory_id for a creature, or return a sentinel. Panics in debug
    /// if the creature doesn't exist.
    pub(crate) fn creature_inv(&self, creature_id: CreatureId) -> InventoryId {
        self.db
            .creatures
            .get(&creature_id)
            .expect("creature must exist")
            .inventory_id
    }

    /// Get the inventory_id for a structure.
    pub(crate) fn structure_inv(&self, structure_id: StructureId) -> InventoryId {
        self.db
            .structures
            .get(&structure_id)
            .expect("structure must exist")
            .inventory_id
    }

    /// Get all item stacks in an inventory as a vec (for bridge/display use).
    pub fn inv_items(&self, inv_id: InventoryId) -> Vec<crate::db::ItemStack> {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
    }

    /// Get all logistics wants for an inventory.
    pub fn inv_wants(&self, inv_id: InventoryId) -> Vec<crate::db::LogisticsWantRow> {
        self.db
            .logistics_want_rows
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
    }

    /// Set logistics wants for an inventory (replaces all existing wants).
    /// Deduplicates input by `(item_kind, material_filter)` pairs, taking the
    /// max quantity for duplicates.
    pub(crate) fn set_inv_wants(&mut self, inv_id: InventoryId, wants: &[building::LogisticsWant]) {
        // Remove existing wants for this inventory.
        let existing = self
            .db
            .logistics_want_rows
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        for row in &existing {
            let _ = self.db.logistics_want_rows.remove_no_fk(&row.id);
        }
        // Deduplicate by (kind, filter), taking max quantity.
        let mut deduped: std::collections::BTreeMap<
            (inventory::ItemKind, inventory::MaterialFilter),
            u32,
        > = std::collections::BTreeMap::new();
        for want in wants {
            let key = (want.item_kind, want.material_filter);
            let entry = deduped.entry(key).or_insert(0);
            *entry = (*entry).max(want.target_quantity);
        }
        // Insert deduplicated wants.
        for ((item_kind, material_filter), target_quantity) in &deduped {
            let _ =
                self.db
                    .logistics_want_rows
                    .insert_auto_no_fk(|id| crate::db::LogisticsWantRow {
                        id,
                        inventory_id: inv_id,
                        item_kind: *item_kind,
                        material_filter: *material_filter,
                        target_quantity: *target_quantity,
                    });
        }
    }

    /// Find the target quantity for a specific `(item_kind, material_filter)`
    /// pair in an inventory's wants, or 0 if no such want exists.
    #[cfg(test)]
    pub(crate) fn inv_want_target(
        &self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        filter: inventory::MaterialFilter,
    ) -> u32 {
        self.db
            .logistics_want_rows
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .find(|w| w.item_kind == kind && w.material_filter == filter)
            .map(|w| w.target_quantity)
            .unwrap_or(0)
    }

    /// Sum target quantities of all wants for a given item kind in an inventory,
    /// regardless of material filter. Used by surplus calculation in
    /// `find_haul_source()` Phase 3.
    pub(crate) fn inv_want_target_total(
        &self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
    ) -> u32 {
        self.db
            .logistics_want_rows
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|w| w.item_kind == kind)
            .map(|w| w.target_quantity)
            .sum()
    }
}
