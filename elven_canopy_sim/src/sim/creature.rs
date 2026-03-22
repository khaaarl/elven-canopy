// Creature lifecycle — spawning, surface placement, pile management, gravity, and task cleanup.
//
// Handles creature spawning (with species-specific nav graph snapping),
// biological trait rolling (hair/eye/skin/body colors etc. stored in the
// `creature_traits` table), surface position finding, ground pile creation
// and gravity (both pile and creature), and the task interruption/preemption/
// cleanup pipeline used when creatures die, flee, or receive new player
// commands.
//
// Creature gravity (F-creature-gravity): creatures on unsupported voxels fall
// to the nearest valid position below, taking damage proportional to distance.
// Support rules vary by creature type — see `creature_is_supported()`.
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
    /// Elves are automatically affiliated with the player's civ; other species
    /// are unaffiliated. Use `spawn_creature_with_civ()` for explicit civ control.
    pub(crate) fn spawn_creature(
        &mut self,
        species: Species,
        position: VoxelCoord,
        events: &mut Vec<SimEvent>,
    ) -> Option<CreatureId> {
        // Elves belong to the player's civ; other species are unaffiliated.
        let civ_id = if species == Species::Elf {
            self.player_civ_id
        } else {
            None
        };
        self.spawn_creature_with_civ(species, position, civ_id, events)
    }

    /// Spawn a creature at the nearest nav node to the given position with an
    /// explicit civ affiliation. Ground-only species snap to ground nodes;
    /// others snap to any node.
    pub(crate) fn spawn_creature_with_civ(
        &mut self,
        species: Species,
        position: VoxelCoord,
        civ_id: Option<CivId>,
        events: &mut Vec<SimEvent>,
    ) -> Option<CreatureId> {
        let species_data = &self.species_table[&species];
        let food_max = species_data.food_max;
        let rest_max = species_data.rest_max;
        let hp_max = species_data.hp_max;
        let mp_max = species_data.mp_max;
        let heartbeat_interval = species_data.heartbeat_interval_ticks;
        let ground_only = species_data.ground_only;
        let is_flyer = species_data.flight_ticks_per_voxel.is_some();

        // Flying creatures spawn at the raw position (entire footprint must be
        // flyable); ground creatures snap to the nearest nav node.
        let node_pos = if is_flyer {
            let footprint = species_data.footprint;
            if !crate::flight_pathfinding::footprint_flyable(&self.world, position, footprint) {
                return None;
            }
            position
        } else {
            let graph = self.graph_for_species(species);
            let nearest_node = if ground_only {
                graph.find_nearest_ground_node(position)
            } else {
                graph.find_nearest_node(position)
            };
            let nearest_node = nearest_node?;
            graph.node(nearest_node).position
        };
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

        let creature = crate::db::Creature {
            id: creature_id,
            species,
            position: node_pos,
            name,
            name_meaning,
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

        // Apply Constitution to HP max (after traits are rolled).
        let constitution = self.trait_int(creature_id, TraitKind::Constitution, 0);
        if constitution != 0 {
            let effective_hp = crate::stats::apply_stat_multiplier(hp_max, constitution).max(1);
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.hp_max = effective_hp;
                c.hp = effective_hp; // spawn at full HP
            });
        }

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

    /// Roll biological traits and creature stats for a newly spawned creature.
    /// Visual traits (hair color, body color, etc.) use one PRNG call for a
    /// bio seed, then Knuth hashing for palette indices. Creature stats
    /// (Strength through Charisma) use 12 PRNG calls each for a normal-ish
    /// distribution from the species config. Total: 1 + 96 PRNG calls.
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
            Species::Hornet => {
                self.insert_trait(
                    creature_id,
                    TraitKind::BodyColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::StripePattern,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::WingStyle,
                    TraitValue::Int(((h / 41) % 3) as i64),
                );
            }
            Species::Wyvern => {
                self.insert_trait(
                    creature_id,
                    TraitKind::BodyColor,
                    TraitValue::Int((h % 4) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::ScalePattern,
                    TraitValue::Int(((h / 11) % 3) as i64),
                );
                self.insert_trait(
                    creature_id,
                    TraitKind::HornStyle,
                    TraitValue::Int(((h / 41) % 3) as i64),
                );
            }
        }

        // Roll creature stats from species-specific distributions.
        // Uses sum-of-12-uniform-samples for a normal-ish distribution.
        // Stats are rolled in STAT_TRAIT_KINDS order after visual traits.
        // Extract distribution params up front to avoid borrow conflicts.
        let stat_params: Vec<(TraitKind, i64, i64)> = crate::stats::STAT_TRAIT_KINDS
            .iter()
            .map(|&kind| {
                let (mean, stdev) = self.species_table[&species]
                    .stat_distributions
                    .get(&kind)
                    .map(|d| (d.mean as i64, d.stdev as i64))
                    .unwrap_or((0, 5));
                (kind, mean, stdev)
            })
            .collect();
        for (stat_kind, mean, stdev) in stat_params {
            let sum: i64 = (0..12)
                .map(|_| self.rng.range_i64_inclusive(-stdev, stdev))
                .sum();
            let stat_value = mean + sum / 2;
            self.insert_trait(creature_id, stat_kind, TraitValue::Int(stat_value));
        }
    }

    /// Insert a single trait row for a creature.
    fn insert_trait(&mut self, creature_id: CreatureId, trait_kind: TraitKind, value: TraitValue) {
        let _ = self.db.creature_traits.insert_no_fk(CreatureTrait {
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
            .get(&(creature_id, kind))
            .map(|t| t.value.as_int(default))
            .unwrap_or(default)
    }

    /// Look up a text trait value for a creature, returning `default` if
    /// the trait is missing or holds a non-text value.
    #[allow(dead_code)]
    pub fn trait_text(&self, creature_id: CreatureId, kind: TraitKind, default: &str) -> String {
        self.db
            .creature_traits
            .get(&(creature_id, kind))
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
    /// returns the air voxel directly above it. Falls back to `floor_y + 1` if
    /// no solid voxel is found (terrain at `floor_y` is always solid).
    pub(crate) fn find_surface_below(&self, x: i32, start_y: i32, z: i32) -> VoxelCoord {
        for y in (0..start_y).rev() {
            if self.world.get(VoxelCoord::new(x, y, z)).is_solid() {
                return VoxelCoord::new(x, y + 1, z);
            }
        }
        // Shouldn't happen (terrain is always solid), but safe fallback.
        VoxelCoord::new(x, self.config.floor_y + 1, z)
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

    /// Check whether a creature is supported at its current position.
    /// Support rules:
    /// - Flying: always supported (exempt from gravity).
    /// - 1x1 climber (`ground_only` = false): supported if a valid nav node
    ///   exists at the creature's position.
    /// - 1x1 ground-only (`ground_only` = true): supported if a valid nav node
    ///   exists AND the voxel below is solid.
    /// - 2x2x2: supported if a valid nav node exists in the large nav graph.
    pub(crate) fn creature_is_supported(&self, creature_id: CreatureId) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return true, // dead or missing — not our problem
        };
        let species_data = &self.species_table[&creature.species];
        if species_data.flight_ticks_per_voxel.is_some() {
            return true; // flying creatures are always supported
        }
        let graph = self.graph_for_species(creature.species);
        let has_node = graph.node_at(creature.position).is_some();
        if !has_node {
            return false;
        }
        if species_data.ground_only {
            // Ground-only creatures also need solid below.
            let below = VoxelCoord::new(
                creature.position.x,
                creature.position.y - 1,
                creature.position.z,
            );
            self.world.get(below).is_solid()
        } else {
            // Climber with a valid nav node — supported.
            true
        }
    }

    /// Scan downward from a creature's current position to find a valid
    /// landing position. Returns `None` if no valid position exists above
    /// Y=0 (degenerate case — caller should teleport to nearest nav node).
    pub(crate) fn find_creature_landing(
        &self,
        species: Species,
        pos: VoxelCoord,
    ) -> Option<VoxelCoord> {
        let species_data = &self.species_table[&species];
        let is_large = species_data.footprint[0] > 1 || species_data.footprint[2] > 1;

        if is_large {
            // 2x2x2: large_node_surface_y computes the single valid surface Y
            // for the 2x2 footprint at this anchor column. If it exists and is
            // below the creature, that's the landing. No iteration needed.
            let ax = pos.x;
            let az = pos.z;
            if let Some(surface_y) = crate::nav::large_node_surface_y(&self.world, ax, az)
                && surface_y < pos.y
            {
                let landing = VoxelCoord::new(ax, surface_y, az);
                if self.large_nav_graph.node_at(landing).is_some() {
                    return Some(landing);
                }
            }
            // No valid large node below — degenerate.
            None
        } else {
            // 1x1: scan downward for a Y that meets support criteria.
            let graph = self.graph_for_species(species);
            for y in (1..pos.y).rev() {
                let candidate = VoxelCoord::new(pos.x, y, pos.z);
                let has_node = graph.node_at(candidate).is_some();
                if !has_node {
                    continue;
                }
                if species_data.ground_only {
                    // Need solid below.
                    let below = VoxelCoord::new(pos.x, y - 1, pos.z);
                    if self.world.get(below).is_solid() {
                        return Some(candidate);
                    }
                } else {
                    // Climber — nav node is enough.
                    return Some(candidate);
                }
            }
            None
        }
    }

    /// Apply gravity to a single creature: move it to the landing position,
    /// apply fall damage, emit events, and schedule a new activation.
    /// Returns `true` if the creature fell.
    pub(crate) fn apply_single_creature_gravity(
        &mut self,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };
        let species = creature.species;
        let old_pos = creature.position;

        // Flying creatures are exempt.
        if self.species_table[&species]
            .flight_ticks_per_voxel
            .is_some()
        {
            return false;
        }

        if self.creature_is_supported(creature_id) {
            return false;
        }

        // Find a landing position.
        let landing = match self.find_creature_landing(species, old_pos) {
            Some(pos) => pos,
            None => {
                // Degenerate: no valid landing column. Teleport to nearest
                // nav node.
                let graph = self.graph_for_species(species);
                match graph.find_nearest_node(old_pos) {
                    Some(n) => graph.node(n).position,
                    None => return false, // no nav nodes at all — nothing to do
                }
            }
        };

        if landing == old_pos {
            return false;
        }

        let fall_distance = (old_pos.y - landing.y).max(0) as i64;

        // Abort current action and task.
        self.abort_current_action(creature_id);
        if let Some(task_id) = self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            self.interrupt_task(creature_id, task_id);
        }

        // Move creature to landing position.
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.position = landing;
            c.path = None;
        });
        self.update_creature_spatial_index(creature_id, species, old_pos, landing);

        // Apply fall damage.
        let damage = fall_distance * self.config.fall_damage_per_voxel;
        let remaining_hp = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| (c.hp - damage).max(0))
            .unwrap_or(0);

        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::CreatureFell {
                creature_id,
                from: old_pos,
                to: landing,
                damage,
                remaining_hp,
            },
        });

        if damage > 0 {
            self.apply_damage_with_cause(creature_id, damage, DeathCause::Falling, events);
        }

        // Schedule a new activation so the creature resumes behavior (if alive).
        if self
            .db
            .creatures
            .get(&creature_id)
            .is_some_and(|c| c.vital_status == VitalStatus::Alive)
        {
            self.event_queue.schedule(
                self.tick + 1,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
        }

        true
    }

    /// Sweep all alive, non-flying creatures for gravity: if any are
    /// unsupported, make them fall. Called from `LogisticsHeartbeat`.
    /// Returns the number of creatures that fell.
    pub(crate) fn apply_creature_gravity(&mut self, events: &mut Vec<SimEvent>) -> usize {
        // Snapshot creature IDs to avoid modifying tables during iteration.
        let candidates: Vec<CreatureId> = self
            .db
            .creatures
            .iter_all()
            .filter(|c| {
                c.vital_status == VitalStatus::Alive
                    && self.species_table[&c.species]
                        .flight_ticks_per_voxel
                        .is_none()
            })
            .map(|c| c.id)
            .collect();

        let mut fell_count = 0;
        for creature_id in candidates {
            if self.apply_single_creature_gravity(creature_id, events) {
                fell_count += 1;
            }
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
    /// For all other action kinds (Build, Eat, MeleeStrike, etc.), the
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
