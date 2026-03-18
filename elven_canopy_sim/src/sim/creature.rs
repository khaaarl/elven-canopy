// Creature lifecycle — spawning, surface placement, pile management, and task cleanup.
//
// Handles creature spawning (with species-specific nav graph snapping),
// biological trait rolling (hair/eye/skin/body colors etc. stored in the
// `creature_traits` table), surface position finding, ground pile creation
// and gravity, and the task interruption/preemption/cleanup pipeline used
// when creatures die, flee, or receive new player commands.
//
// See also: `activation.rs` (creature decision loop), `combat.rs` (death
// handling), `movement.rs` (movement execution), `types.rs` for `TraitKind`
// and `TraitValue`.
use super::*;
use crate::db::{ActionKind, CreatureTrait};
use crate::event::{ScheduledEventKind, SimEvent, SimEventKind};
use crate::inventory;
use crate::task;

impl SimState {
    /// Spawn a creature at the nearest nav node to the given position.
    /// Ground-only species snap to ground nodes; others snap to any node.
    pub(crate) fn spawn_creature(
        &mut self,
        species: Species,
        position: VoxelCoord,
        events: &mut Vec<SimEvent>,
    ) -> Option<CreatureId> {
        let species_data = &self.species_table[&species];
        let food_max = species_data.food_max;
        let rest_max = species_data.rest_max;
        let hp_max = species_data.hp_max;
        let mp_max = species_data.mp_max;
        let heartbeat_interval = species_data.heartbeat_interval_ticks;
        let ground_only = species_data.ground_only;
        let graph = self.graph_for_species(species);

        let nearest_node = if ground_only {
            graph.find_nearest_ground_node(position)
        } else {
            graph.find_nearest_node(position)
        };

        let nearest_node = nearest_node?;

        let node_pos = graph.node(nearest_node).position;
        let creature_id = CreatureId::new(&mut self.rng);

        // Generate a Vaelith name for elves; other species are unnamed.
        let (name, name_meaning) = if species == Species::Elf {
            if let Some(lexicon) = &self.lexicon {
                let vname = elven_canopy_lang::names::generate_name(lexicon, &mut self.rng);
                (
                    vname.full_name,
                    format!("{} {}", vname.given_meaning, vname.surname_meaning),
                )
            } else {
                (String::new(), String::new())
            }
        } else {
            (String::new(), String::new())
        };

        let default_wants = if species == Species::Elf {
            self.config.elf_default_wants.clone()
        } else {
            Vec::new()
        };

        // Create an inventory for this creature.
        let inv_id = self.create_inventory(crate::db::InventoryOwnerKind::Creature);

        // Elves belong to the player's civ; other species are unaffiliated.
        let civ_id = if species == Species::Elf {
            self.player_civ_id
        } else {
            None
        };

        let creature = crate::db::Creature {
            id: creature_id,
            species,
            position: node_pos,
            name,
            name_meaning,
            current_node: Some(nearest_node),
            path: None,
            current_task: None,
            food: food_max,
            rest: rest_max,
            assigned_home: None,
            inventory_id: inv_id,
            civ_id,
            military_group: None,
            action_kind: ActionKind::NoAction,
            next_available_tick: None,
            hp: hp_max,
            hp_max,
            vital_status: VitalStatus::Alive,
            mp: mp_max,
            mp_max,
            wasted_action_count: 0,
        };

        self.db.creatures.insert_no_fk(creature).unwrap();

        // Roll biological traits from the PRNG and store them.
        self.roll_creature_traits(creature_id, species);

        // Register in spatial index.
        let footprint = self.species_table[&species].footprint;
        Self::register_creature_in_index(&mut self.spatial_index, creature_id, node_pos, footprint);

        // Set default logistics wants for this creature.
        if !default_wants.is_empty() {
            self.set_inv_wants(inv_id, &default_wants);
        }

        // Give elves starting items so they don't immediately forage and can
        // defend themselves.
        if species == Species::Elf {
            if self.config.elf_starting_bread > 0 {
                self.inv_add_simple_item(
                    inv_id,
                    inventory::ItemKind::Bread,
                    self.config.elf_starting_bread,
                    Some(creature_id),
                    None,
                );
            }
            if self.config.elf_starting_bows > 0 {
                self.inv_add_simple_item(
                    inv_id,
                    inventory::ItemKind::Bow,
                    self.config.elf_starting_bows,
                    Some(creature_id),
                    None,
                );
            }
            if self.config.elf_starting_arrows > 0 {
                self.inv_add_simple_item(
                    inv_id,
                    inventory::ItemKind::Arrow,
                    self.config.elf_starting_arrows,
                    Some(creature_id),
                    None,
                );
            }
        }

        // Schedule first activation (drives movement — wander or task work).
        // Fires 1 tick after spawn so the creature starts moving immediately.
        self.event_queue.schedule(
            self.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id },
        );

        // Schedule first heartbeat (periodic non-movement checks).
        let heartbeat_tick = self.tick + heartbeat_interval;
        self.event_queue.schedule(
            heartbeat_tick,
            ScheduledEventKind::CreatureHeartbeat { creature_id },
        );

        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::CreatureArrived {
                creature_id,
                species,
            },
        });
        Some(creature_id)
    }

    // -----------------------------------------------------------------
    // Creature biology traits
    // -----------------------------------------------------------------

    /// Roll biological traits for a newly spawned creature and insert them
    /// into the `creature_traits` table. Consumes exactly one PRNG call
    /// (for the bio seed), then derives all trait indices deterministically
    /// via Knuth hashing — so adding new traits later doesn't shift
    /// existing trait values.
    fn roll_creature_traits(&mut self, creature_id: CreatureId, species: Species) {
        let bio_seed = self.rng.next_u64() as i64;
        self.insert_trait(creature_id, TraitKind::BioSeed, TraitValue::Int(bio_seed));

        // Knuth multiplicative hash to spread bits for palette indexing.
        let h = (bio_seed.wrapping_mul(2_654_435_761)).unsigned_abs();

        match species {
            Species::Elf => {
                self.insert_trait(
                    creature_id,
                    TraitKind::HairColor,
                    TraitValue::Int((h % 7) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::EyeColor,
                    TraitValue::Int(((h / 7) % 5) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::SkinTone,
                    TraitValue::Int(((h / 31) % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::HairStyle,
                    TraitValue::Int(((h / 131) % 3) as i64),
                );
            }
            Species::Capybara => {
                self.insert_trait(
                    creature_id,
                    TraitKind::BodyColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::Accessory,
                    TraitValue::Int(((h / 13) % 4) as i64),
                );
            }
            Species::Boar => {
                self.insert_trait(
                    creature_id,
                    TraitKind::BodyColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::TuskSize,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
            }
            Species::Deer => {
                self.insert_trait(
                    creature_id,
                    TraitKind::BodyColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::AntlerStyle,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::SpotPattern,
                    TraitValue::Int(((h / 41) % 2) as i64),
                );
            }
            Species::Elephant => {
                self.insert_trait(
                    creature_id,
                    TraitKind::BodyColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::TuskType,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
            }
            Species::Goblin => {
                self.insert_trait(
                    creature_id,
                    TraitKind::SkinColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::EarStyle,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
            }
            Species::Monkey => {
                self.insert_trait(
                    creature_id,
                    TraitKind::FurColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::FaceMarking,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
            }
            Species::Orc => {
                self.insert_trait(
                    creature_id,
                    TraitKind::SkinColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::WarPaint,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
            }
            Species::Squirrel => {
                self.insert_trait(
                    creature_id,
                    TraitKind::FurColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::TailType,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
            }
            Species::Troll => {
                self.insert_trait(
                    creature_id,
                    TraitKind::SkinColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::HornStyle,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
            }
        }
    }

    /// Insert a single trait row for a creature.
    fn insert_trait(&mut self, creature_id: CreatureId, trait_kind: TraitKind, value: TraitValue) {
        let _ = self
            .db
            .creature_traits
            .insert_auto_no_fk(|id| CreatureTrait {
                id,
                creature_id,
                trait_kind,
                value: value.clone(),
            });
    }

    /// Look up an integer trait value for a creature, returning `default` if
    /// the trait is missing or holds a non-integer value.
    pub fn trait_int(&self, creature_id: CreatureId, kind: TraitKind, default: i64) -> i64 {
        self.db
            .creature_traits
            .by_creature_trait_kind(&creature_id, &kind, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .map(|t| t.value.as_int(default))
            .unwrap_or(default)
    }

    /// Look up a text trait value for a creature, returning `default` if
    /// the trait is missing or holds a non-text value.
    #[allow(dead_code)]
    pub fn trait_text(&self, creature_id: CreatureId, kind: TraitKind, default: &str) -> String {
        self.db
            .creature_traits
            .by_creature_trait_kind(&creature_id, &kind, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .map(|t| t.value.as_text(default))
            .unwrap_or_else(|| default.to_string())
    }

    /// Find the lowest non-solid Y position at the given (x, z) column.
    /// Returns the first air voxel above solid ground, defaulting to y=1.
    pub(crate) fn find_surface_position(&self, x: i32, z: i32) -> VoxelCoord {
        for y in 1..self.world.size_y as i32 {
            let pos = VoxelCoord::new(x, y, z);
            if !self.world.get(pos).is_solid() {
                return pos;
            }
        }
        VoxelCoord::new(x, 1, z)
    }

    /// Get or create a ground pile at the given position, returning its ID.
    /// If no pile exists at `pos`, inserts a new empty one. If the position
    /// is floating (no solid voxel below), it is snapped down to the nearest
    /// surface before creation. If a pile already exists at the snapped
    /// position, that pile is returned instead of creating a new one.
    pub(crate) fn ensure_ground_pile(&mut self, pos: VoxelCoord) -> GroundPileId {
        // Snap to surface if the position is floating.
        let pos = if pos.y > 0
            && !self
                .world
                .get(VoxelCoord::new(pos.x, pos.y - 1, pos.z))
                .is_solid()
        {
            self.find_surface_below(pos.x, pos.y, pos.z)
        } else {
            pos
        };

        if let Some(pile) = self
            .db
            .ground_piles
            .by_position(&pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
        {
            pile.id
        } else {
            let inv_id = self.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
            self.db
                .ground_piles
                .insert_auto_no_fk(|id| crate::db::GroundPile {
                    id,
                    position: pos,
                    inventory_id: inv_id,
                })
                .unwrap()
        }
    }

    /// Find the surface position below a given Y coordinate in a column.
    /// Scans downward from `start_y - 1` to find the first solid voxel, then
    /// returns the air voxel directly above it. Falls back to y=1 if no solid
    /// voxel is found (ForestFloor at y=0 is always solid).
    pub(crate) fn find_surface_below(&self, x: i32, start_y: i32, z: i32) -> VoxelCoord {
        for y in (0..start_y).rev() {
            if self.world.get(VoxelCoord::new(x, y, z)).is_solid() {
                return VoxelCoord::new(x, y + 1, z);
            }
        }
        // Shouldn't happen (ForestFloor at y=0 is solid), but safe fallback.
        VoxelCoord::new(x, 1, z)
    }

    /// Check all ground piles for gravity: if the voxel below a pile's position
    /// is not solid, the pile falls to the nearest surface below. If a pile
    /// already exists at the landing position, the falling pile's inventory is
    /// merged into it and the floating pile is deleted. Returns the number of
    /// piles that fell.
    pub(crate) fn apply_pile_gravity(&mut self) -> usize {
        // Collect all piles that need to fall. We snapshot first because
        // modifying tables during iteration would invalidate the iterator.
        let floating: Vec<(GroundPileId, VoxelCoord)> = self
            .db
            .ground_piles
            .iter_all()
            .filter_map(|pile| {
                let below = VoxelCoord::new(pile.position.x, pile.position.y - 1, pile.position.z);
                if pile.position.y > 0 && !self.world.get(below).is_solid() {
                    Some((pile.id, pile.position))
                } else {
                    None
                }
            })
            .collect();

        let mut fell_count = 0;
        for (pile_id, old_pos) in floating {
            // The pile may have been deleted by a previous iteration's merge.
            let pile = match self.db.ground_piles.get(&pile_id) {
                Some(p) => p,
                None => continue,
            };
            let landing = self.find_surface_below(old_pos.x, old_pos.y, old_pos.z);
            if landing == old_pos {
                continue; // Already on a surface (race with another pile falling here).
            }

            // Check if a pile already exists at the landing position.
            let existing = self
                .db
                .ground_piles
                .by_position(&landing, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next();

            if let Some(target_pile) = existing {
                // Merge inventories and delete the floating pile.
                let src_inv = pile.inventory_id;
                self.inv_merge(src_inv, target_pile.inventory_id);
                let _ = self.db.ground_piles.remove_no_fk(&pile_id);
                let _ = self.db.inventories.remove_no_fk(&src_inv);
            } else {
                // No pile at landing — remove and re-insert to update the
                // unique position index.
                let inv_id = pile.inventory_id;
                let _ = self.db.ground_piles.remove_no_fk(&pile_id);
                let _ = self
                    .db
                    .ground_piles
                    .insert_auto_no_fk(|new_id| crate::db::GroundPile {
                        id: new_id,
                        position: landing,
                        inventory_id: inv_id,
                    });
            }
            fell_count += 1;
        }
        fell_count
    }

    /// Remove a creature from its assigned task.
    pub(crate) fn unassign_creature_from_task(&mut self, creature_id: CreatureId) {
        let task_id = match self.db.creatures.get(&creature_id) {
            Some(c) => c.current_task,
            None => return,
        };
        if let Some(tid) = task_id
            && let Some(mut task) = self.db.tasks.get(&tid)
        {
            // Check if any other creature is still assigned to this task.
            let remaining = self
                .db
                .creatures
                .by_current_task(&Some(tid), tabulosity::QueryOpts::ASC)
                .into_iter()
                .filter(|c| c.id != creature_id)
                .count();
            if remaining == 0 && matches!(task.state, task::TaskState::InProgress) {
                task.state = task::TaskState::Available;
                let _ = self.db.tasks.update_no_fk(task);
            }
        }
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.current_task = None;
            creature.path = None;
            creature.wasted_action_count = 0;
            let _ = self.db.creatures.update_no_fk(creature);
        }
    }

    /// Hard-interrupt a creature's current task: abort the in-progress action,
    /// clean up task-specific state, and unassign the creature.
    ///
    /// Used for forced interruptions (nav invalidation, mope preemption, death,
    /// flee). Player commands use `preempt_task()` instead, which preserves
    /// mid-Move actions to prevent exploitable double-speed movement.
    pub(crate) fn interrupt_task(&mut self, creature_id: CreatureId, task_id: TaskId) {
        self.abort_current_action(creature_id);
        self.cleanup_and_unassign_task(creature_id, task_id);
    }

    /// Preempt a creature's current task for a player command. If the creature
    /// is mid-Move, the action completes naturally (only the task is swapped).
    /// For all other action kinds (Build, Cook, Eat, MeleeStrike, etc.), the
    /// action is aborted because their resolve functions read `current_task`
    /// for task-specific data and would operate on the wrong (new) task.
    ///
    /// Returns `true` if a Move action is still in-flight (caller should NOT
    /// schedule a new activation — the existing one will fire).
    pub(crate) fn preempt_task(&mut self, creature_id: CreatureId, task_id: TaskId) -> bool {
        let is_mid_move =
            self.db.creatures.get(&creature_id).is_some_and(|c| {
                c.action_kind == ActionKind::Move && c.next_available_tick.is_some()
            });
        if is_mid_move {
            // Move actions are self-contained (resolve just cleans up
            // MoveAction row), so they can complete with the new task.
            self.cleanup_and_unassign_task(creature_id, task_id);
        } else {
            // Non-Move actions couple to task data during resolve, so we
            // must abort the action to avoid operating on the wrong task.
            self.interrupt_task(creature_id, task_id);
        }
        is_mid_move
    }

    /// Shared cleanup for task interruption/preemption: per-kind cleanup
    /// (release reservations, drop carried items), task state transition,
    /// and creature unassignment. Does NOT touch the creature's action state.
    pub(crate) fn cleanup_and_unassign_task(&mut self, creature_id: CreatureId, task_id: TaskId) {
        let kind_tag = match self.db.tasks.get(&task_id) {
            Some(t) => t.kind_tag,
            None => {
                // Task already gone — just clear creature fields.
                if let Some(mut c) = self.db.creatures.get(&creature_id) {
                    c.current_task = None;
                    c.path = None;
                    c.wasted_action_count = 0;
                    let _ = self.db.creatures.update_no_fk(c);
                }
                return;
            }
        };

        // Per-kind cleanup: release reservations, drop carried items, etc.
        match kind_tag {
            crate::db::TaskKindTag::Haul => {
                self.cleanup_haul_task(creature_id, task_id);
            }
            crate::db::TaskKindTag::Cook => {
                self.cleanup_cook_task(task_id);
            }
            crate::db::TaskKindTag::Craft => {
                self.cleanup_craft_task(task_id);
            }
            crate::db::TaskKindTag::Harvest => {
                self.cleanup_harvest_task(task_id);
            }
            crate::db::TaskKindTag::AcquireItem => {
                self.cleanup_acquire_item_task(task_id);
            }
            crate::db::TaskKindTag::AcquireMilitaryEquipment => {
                self.cleanup_acquire_military_equipment_task(task_id);
            }
            // Resumable tasks: return to Available for another creature.
            // unassign_creature_from_task handles reverting InProgress → Available.
            crate::db::TaskKindTag::Build | crate::db::TaskKindTag::Furnish => {}
            // No-cleanup tasks: mark Complete so they aren't re-claimed.
            crate::db::TaskKindTag::GoTo
            | crate::db::TaskKindTag::EatBread
            | crate::db::TaskKindTag::EatFruit
            | crate::db::TaskKindTag::Sleep
            | crate::db::TaskKindTag::Mope
            | crate::db::TaskKindTag::AttackTarget
            | crate::db::TaskKindTag::AttackMove => {
                if let Some(mut t) = self.db.tasks.get(&task_id) {
                    t.state = task::TaskState::Complete;
                    let _ = self.db.tasks.update_no_fk(t);
                }
            }
        }

        // Clear creature assignment. For resumable tasks (Build, Furnish),
        // this reverts the task to Available if no other creatures remain.
        // For non-resumable tasks, the task is already Complete.
        self.unassign_creature_from_task(creature_id);
    }
}
