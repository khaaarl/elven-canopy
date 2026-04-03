// Greenhouse system — fruit spawning and harvest monitoring.
//
// Manages the periodic `GreenhouseMonitor` event that creates harvest tasks
// for ripe fruit, and the `attempt_fruit_spawn` logic that grows new fruit
// on eligible tree positions during tree heartbeats.
//
// See also: `fruit.rs` (fruit species data), `logistics.rs` (harvest task
// cleanup), `needs.rs` (eating fruit).
use super::*;
use crate::inventory;

impl SimState {
    /// Scan greenhouses and produce fruit when production interval has elapsed.
    /// Called at the end of each logistics heartbeat.
    pub(crate) fn process_greenhouse_monitor(&mut self) {
        let base_ticks = self.config.greenhouse_base_production_ticks;
        if base_ticks == 0 {
            return;
        }

        // Collect (id, species, production_interval, last_tick) to avoid borrow.
        let greenhouses: Vec<(StructureId, FruitSpeciesId, u64, u64)> = self
            .db
            .structures
            .iter_all()
            .filter_map(|s| {
                if s.furnishing == Some(FurnishingType::Greenhouse) && s.greenhouse_enabled {
                    let species_id = s.greenhouse_species?;
                    let area = s.floor_interior_positions().len().max(1) as u64;
                    let interval = base_ticks / area;
                    Some((
                        s.id,
                        species_id,
                        interval,
                        s.greenhouse_last_production_tick,
                    ))
                } else {
                    None
                }
            })
            .collect();

        let tick = self.tick;
        for (sid, species_id, interval, last_tick) in greenhouses {
            if interval == 0 || tick < last_tick + interval {
                continue;
            }

            // Produce fruit into the structure's inventory.
            let inv_id = match self.db.structures.get(&sid) {
                Some(s) => s.inventory_id,
                None => continue,
            };
            self.inv_add_item(
                inv_id,
                inventory::ItemKind::Fruit,
                1,
                None,
                None,
                Some(inventory::Material::FruitSpecies(species_id)),
                0,
                None,
                None,
            );

            // Update last production tick.
            if let Some(mut s) = self.db.structures.get(&sid) {
                s.greenhouse_last_production_tick = tick;
                let _ = self.db.update_structure(s);
            }
        }
    }

    /// Attempt to spawn one fruit on the given tree. Rolls the RNG for spawn
    /// chance and picks a random leaf voxel to hang fruit below. Returns `true`
    /// if a fruit was actually placed.
    ///
    /// This is the single code path for all fruit spawning — both the initial
    /// fast-forward during `with_config()` and the periodic `TreeHeartbeat`.
    pub(crate) fn attempt_fruit_spawn(&mut self, tree_id: TreeId) -> bool {
        let tree = match self.db.trees.get(&tree_id) {
            Some(t) => t,
            None => return false,
        };

        let fruit_count = self
            .db
            .tree_fruits
            .count_by_tree_id(&tree_id, tabulosity::QueryOpts::ASC);
        if fruit_count >= self.config.fruit_max_per_tree as usize {
            return false;
        }

        // Roll spawn chance (integer PPM comparison, deterministic).
        let roll = self.rng.next_u32() % 1_000_000;
        if roll >= self.config.fruit_production_rate_ppm {
            return false;
        }

        if tree.leaf_voxels.is_empty() {
            return false;
        }

        let species_id = match tree.fruit_species_id {
            Some(id) => id,
            None => return false,
        };

        // Pick a random leaf voxel; fruit hangs one voxel below it.
        // Skip leaves that have been carved away.
        let leaf_count = tree.leaf_voxels.len();
        let leaf_idx = self.rng.range_u64(0, leaf_count as u64) as usize;
        let leaf_pos = tree.leaf_voxels[leaf_idx];
        if self.world.get(leaf_pos) == VoxelType::Air {
            return false;
        }
        let fruit_pos = VoxelCoord::new(leaf_pos.x, leaf_pos.y - 1, leaf_pos.z);

        // Position must be in-bounds, currently air, and not already fruited.
        if !self.world.in_bounds(fruit_pos) {
            return false;
        }
        if self.world.get(fruit_pos) != VoxelType::Air {
            return false;
        }
        if !self
            .db
            .tree_fruits
            .by_position(&VoxelBox::point(fruit_pos), tabulosity::QueryOpts::ASC)
            .is_empty()
        {
            return false;
        }

        // Place the fruit voxel and insert a TreeFruit row.
        self.set_voxel(fruit_pos, VoxelType::Fruit);
        let fruit = crate::db::TreeFruit {
            id: crate::types::TreeFruitId(0), // auto-increment
            tree_id,
            position: VoxelBox::point(fruit_pos),
            species_id,
        };
        let _ = self
            .db
            .insert_tree_fruit_auto(|id| crate::db::TreeFruit { id, ..fruit });
        true
    }
}
