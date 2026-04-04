// Creature lifecycle — spawning, surface placement, pile management, gravity, and task cleanup.
//
// Handles creature spawning (with species-specific walkability snapping),
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
    /// Spawn a creature at the nearest walkable position to the given position.
    /// Ground-only species snap to ground-level walkable voxels.
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
        let footprint = species_data.footprint;
        let sex_weights = species_data.sex_weights;

        // Flying creatures spawn at the raw position (entire footprint must be
        // flyable); ground creatures snap to the nearest nav node.
        let node_pos = if is_flyer {
            let footprint = species_data.footprint;
            if !crate::pathfinding::footprint_flyable(&self.world, position, footprint) {
                return None;
            }
            position
        } else {
            // Ground creature: find nearest walkable position for this footprint.
            let nearest = if ground_only {
                crate::walkability::find_nearest_ground_walkable(
                    &self.world,
                    &self.face_data,
                    position,
                    10,
                    footprint,
                )
            } else {
                crate::walkability::find_nearest_walkable(
                    &self.world,
                    &self.face_data,
                    position,
                    10,
                    footprint,
                )
            };
            nearest?
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

        let sex = crate::species::roll_creature_sex(&sex_weights, &mut self.rng);

        let creature = crate::db::Creature {
            id: creature_id,
            species,
            position: VoxelBox::from_anchor(node_pos, footprint),
            name,
            name_meaning,
            path: None,
            current_task: None,
            current_activity: None,
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
            hp_regen_remainder: 0,
            vital_status: VitalStatus::Alive,
            mp: mp_max,
            mp_max,
            mp_regen_remainder: 0,
            wasted_action_count: 0,
            last_dance_tick: 0,
            last_dinner_party_tick: 0,
            sex,
        };

        self.db.insert_creature(creature).unwrap();

        // Roll biological traits from the PRNG and store them.
        self.roll_creature_traits(creature_id, species);

        // Apply Constitution to HP max (after traits are rolled).
        let constitution = self.trait_int(creature_id, TraitKind::Constitution, 0);
        if constitution != 0 {
            let effective_hp = crate::stats::apply_stat_multiplier(hp_max, constitution).max(1);
            if let Some(mut c) = self.db.creatures.get(&creature_id) {
                c.hp_max = effective_hp;
                c.hp = effective_hp; // spawn at full HP
                let _ = self.db.update_creature(c);
            }
        }

        // Apply Willpower to mana pool size (after traits are rolled).
        // mp_max=0 species (nonmagical) are unaffected.
        let willpower = self.trait_int(creature_id, TraitKind::Willpower, 0);
        if willpower != 0 && mp_max > 0 {
            let effective_mp = crate::stats::apply_stat_multiplier(mp_max, willpower).max(1);
            if let Some(mut c) = self.db.creatures.get(&creature_id) {
                c.mp_max = effective_mp;
                c.mp = effective_mp; // spawn at full mana
                let _ = self.db.update_creature(c);
            }
        }

        // Set default logistics wants for this creature.
        if !default_wants.is_empty() {
            self.set_inv_wants(inv_id, &default_wants);
        }

        // Give elves starting items so they don't immediately forage and can
        // defend themselves. Starting gear is Crude (-1) for early progression.
        if species == Species::Elf {
            let starting_quality = -1; // Crude
            if self.config.elf_starting_bread > 0 {
                self.inv_add_item(
                    inv_id,
                    inventory::ItemKind::Bread,
                    self.config.elf_starting_bread,
                    Some(creature_id),
                    None,
                    None,
                    starting_quality,
                    None,
                    None,
                );
            }
            if self.config.elf_starting_bows > 0 {
                self.inv_add_item(
                    inv_id,
                    inventory::ItemKind::Bow,
                    self.config.elf_starting_bows,
                    Some(creature_id),
                    None,
                    None,
                    starting_quality,
                    None,
                    None,
                );
            }
            if self.config.elf_starting_arrows > 0 {
                self.inv_add_item(
                    inv_id,
                    inventory::ItemKind::Arrow,
                    self.config.elf_starting_arrows,
                    Some(creature_id),
                    None,
                    None,
                    starting_quality,
                    None,
                    None,
                );
            }
        }

        // Assign default path (Outcast) for elves. Other species don't
        // participate in the path system.
        if species == Species::Elf {
            let _ = self.db.insert_path_assignment(crate::db::PathAssignment {
                creature_id,
                path_id: PathId::Outcast,
            });
        }

        // Mark creature for first activation (drives movement — wander or task work).
        // Fires 1 tick after spawn so the creature starts moving immediately.
        self.set_creature_activation_tick(creature_id, self.tick + 1);

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
    /// Visual traits (hair color, body color, etc.) are genome-derived via
    /// species-specific SNP expression. Creature stats (Strength through
    /// Charisma) are derived from a randomly generated genome via weighted-sum
    /// SNP expression (see `genome.rs`). The genome is stored in the
    /// `creature_genomes` table for future inheritance.
    fn roll_creature_traits(&mut self, creature_id: CreatureId, species: Species) {
        // Generate genome and derive ability scores from it.
        //
        // The generic genome encodes ability scores (8 × 32-bit weighted-sum
        // SNP regions) and Big Five personality (5 × 8-bit regions). For
        // non-offspring creatures (wild spawns, starting elves), genome bits
        // are drawn independently at random from the sim PRNG.
        let generic_genome =
            crate::genome::Genome::random(&mut self.rng, crate::genome::GENERIC_GENOME_BITS);

        // Species-specific genome (pigmentation VSH axes, future morphology).
        let species_genome_bits = self.species_table[&species]
            .genome_config
            .species_snps
            .iter()
            .map(|snp| snp.bits)
            .sum::<u32>();
        let species_genome = if species_genome_bits > 0 {
            crate::genome::Genome::random(&mut self.rng, species_genome_bits)
        } else {
            crate::genome::Genome::new(0)
        };

        // Derive ability scores from the generic genome.
        let stat_params: Vec<(TraitKind, i64, i64)> = crate::stats::STAT_TRAIT_KINDS
            .iter()
            .map(|&kind| {
                let (mean, stdev) = self.species_table[&species]
                    .stat_distributions
                    .get(&kind)
                    .map(|d| (d.mean as i64, d.stdev as i64))
                    .unwrap_or((0, 50));
                (kind, mean, stdev)
            })
            .collect();
        for (stat_idx, (stat_kind, mean, stdev)) in stat_params.into_iter().enumerate() {
            let stat_value =
                crate::genome::express_stat(&generic_genome, stat_idx as u32, mean, stdev);
            self.insert_trait(creature_id, stat_kind, TraitValue::Int(stat_value));
        }

        // Derive Big Five personality values from the generic genome.
        for (axis_idx, &trait_kind) in crate::genome::PERSONALITY_TRAIT_KINDS.iter().enumerate() {
            let axis = crate::species::PERSONALITY_AXES[axis_idx];
            let (mean, stdev) = self.species_table[&species]
                .personality_distributions
                .get(&axis)
                .map(|d| (d.mean as i64, d.stdev as i64))
                .unwrap_or((0, 50));
            let value =
                crate::genome::express_personality(&generic_genome, axis_idx as u32, mean, stdev);
            self.insert_trait(creature_id, trait_kind, TraitValue::Int(value));
        }

        // Express species-specific pigmentation traits from the species genome.
        if species_genome.bit_len() > 0 {
            let genome_config = self.species_table[&species].genome_config.clone();
            // XOR-fold creature ID to u64 for categorical tiebreak seed.
            let id_bytes = creature_id.0.as_bytes();
            let tiebreak_seed = u64::from_le_bytes(id_bytes[..8].try_into().unwrap())
                ^ u64::from_le_bytes(id_bytes[8..].try_into().unwrap());

            let expressed = crate::genome::express_species_genome(
                &species_genome,
                &genome_config,
                tiebreak_seed,
            );
            // Blendable hue groups (hair_hue, eye_hue) are re-expressed below
            // with weighted-sum scoring and blend info. Skip them here to
            // avoid redundant double-expression that would be immediately
            // overwritten.
            const BLENDED_HUE_GROUPS: &[&str] =
                &["hair_hue", "eye_hue", "body_hue", "fur_hue", "skin_hue"];
            for (name, value) in &expressed {
                if BLENDED_HUE_GROUPS.contains(&name.as_str()) {
                    continue;
                }
                if let Some(trait_kind) = snp_name_to_trait_kind(name, species) {
                    self.insert_trait(creature_id, trait_kind, TraitValue::Int(*value));
                }
            }

            // Hue blending for adjacent categories on the hue wheel.
            // Hair and eye hue groups get blended expression; the blend
            // target and weight are stored as separate traits.
            let blendable_hue_groups: &[(&str, TraitKind, TraitKind, TraitKind)] = &[
                (
                    "hair_hue",
                    TraitKind::HairColor,
                    TraitKind::HairBlendTarget,
                    TraitKind::HairBlendWeight,
                ),
                (
                    "eye_hue",
                    TraitKind::EyeColor,
                    TraitKind::EyeBlendTarget,
                    TraitKind::EyeBlendWeight,
                ),
                (
                    "body_hue",
                    TraitKind::BodyColor,
                    TraitKind::BodyBlendTarget,
                    TraitKind::BodyBlendWeight,
                ),
                (
                    "fur_hue",
                    TraitKind::FurColor,
                    TraitKind::FurBlendTarget,
                    TraitKind::FurBlendWeight,
                ),
                (
                    "skin_hue",
                    TraitKind::SkinColor,
                    TraitKind::SkinColorBlendTarget,
                    TraitKind::SkinColorBlendWeight,
                ),
            ];
            for &(group_name, hue_kind, target_kind, weight_kind) in blendable_hue_groups {
                if let Some(result) = express_blended_hue_group(
                    &species_genome,
                    &genome_config,
                    group_name,
                    tiebreak_seed,
                ) {
                    match result {
                        crate::genome::CategoricalResult::Single(idx) => {
                            // Overwrite the hue trait with the blended result
                            // (should match express_species_genome, but
                            // re-expressed with weighted sums).
                            self.insert_trait(creature_id, hue_kind, TraitValue::Int(idx as i64));
                            self.insert_trait(creature_id, target_kind, TraitValue::Int(-1));
                            self.insert_trait(creature_id, weight_kind, TraitValue::Int(0));
                        }
                        crate::genome::CategoricalResult::Blend {
                            primary,
                            secondary,
                            weight,
                        } => {
                            self.insert_trait(
                                creature_id,
                                hue_kind,
                                TraitValue::Int(primary as i64),
                            );
                            self.insert_trait(
                                creature_id,
                                target_kind,
                                TraitValue::Int(secondary as i64),
                            );
                            self.insert_trait(
                                creature_id,
                                weight_kind,
                                TraitValue::Int(weight as i64),
                            );
                        }
                    }
                }
            }
        }

        // Store the genome in the creature_genomes table.
        let _ = self.db.insert_creature_genome(crate::db::CreatureGenome {
            creature_id,
            generic_genome,
            species_genome,
        });
    }

    /// Insert a single trait row for a creature.
    pub(crate) fn insert_trait(
        &mut self,
        creature_id: CreatureId,
        trait_kind: TraitKind,
        value: TraitValue,
    ) {
        let _ = self.db.insert_creature_trait(CreatureTrait {
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

    /// Compute the effective hostile detection range² for a creature, applying
    /// the Perception stat multiplier to the linear detection range and
    /// returning the squared result. Perception scales the linear radius
    /// (so PER +100 doubles the radius, quadrupling the squared range).
    /// Returns 0 for species with no detection (passive creatures).
    pub(crate) fn effective_detection_range_sq(
        &self,
        creature_id: CreatureId,
        species: Species,
    ) -> i64 {
        let base_sq = self.species_table[&species].hostile_detection_range_sq;
        if base_sq == 0 {
            return 0;
        }
        let perception = self.trait_int(creature_id, TraitKind::Perception, 0);
        if perception == 0 {
            return base_sq;
        }
        // Apply the multiplier twice: once per dimension of the squared range.
        // This makes Perception scale the linear radius, not the squared range.
        let once = crate::stats::apply_stat_multiplier(base_sq, perception);
        crate::stats::apply_stat_multiplier(once, perception).max(1)
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
                .insert_ground_pile_auto(|id| crate::db::GroundPile {
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
                let _ = self.db.remove_ground_pile(&pile_id);
                let _ = self.db.remove_inventory(&src_inv);
            } else {
                // No pile at landing — remove and re-insert to update the
                // unique position index.
                let inv_id = pile.inventory_id;
                let _ = self.db.remove_ground_pile(&pile_id);
                let _ = self
                    .db
                    .insert_ground_pile_auto(|new_id| crate::db::GroundPile {
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
    /// - Climber (`ground_only` = false): supported if the position is walkable.
    /// - Ground-only: supported if the position is walkable AND the non-climber
    ///   support rules are met (same thresholds as `footprint_walkable`):
    ///   - 1x1: solid at y-1.
    ///   - 2x2x2: 3+ columns with solid at y-1, OR 1+ at y-1 and 2+ at y-2.
    pub(crate) fn creature_is_supported(&self, creature_id: CreatureId) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return true, // dead or missing — not our problem
        };
        let species_data = &self.species_table[&creature.species];
        if species_data.flight_ticks_per_voxel.is_some() {
            return true; // flying creatures are always supported
        }
        let walkable = crate::walkability::footprint_walkable(
            &self.world,
            &self.face_data,
            creature.position.min,
            species_data.footprint,
        );
        if !walkable {
            return false;
        }
        if species_data.ground_only {
            // Ground-only creatures need solid below (non-climber support).
            // Uses the same 3-of-4 / 1+2 thresholds as footprint_walkable.
            let anchor = creature.position.min;
            let fx = species_data.footprint[0] as i32;
            let fz = species_data.footprint[2] as i32;

            // Count columns with solid directly at y-1.
            let mut direct_support = 0u32;
            for dz in 0..fz {
                for dx in 0..fx {
                    if self
                        .world
                        .get(VoxelCoord::new(anchor.x + dx, anchor.y - 1, anchor.z + dz))
                        .is_solid()
                    {
                        direct_support += 1;
                    }
                }
            }
            let total_columns = (fx * fz) as u32;
            if total_columns <= 1 {
                // 1x1: need solid directly below.
                return direct_support >= 1;
            }
            // Large footprint: 3+ columns at y-1, or 1+ at y-1 and 2+ at y-2.
            if direct_support >= 3 {
                return true;
            }
            if direct_support >= 1 {
                let mut deep_support = 0u32;
                for dz in 0..fz {
                    for dx in 0..fx {
                        if self
                            .world
                            .get(VoxelCoord::new(anchor.x + dx, anchor.y - 2, anchor.z + dz))
                            .is_solid()
                        {
                            deep_support += 1;
                        }
                    }
                }
                if deep_support >= 2 {
                    return true;
                }
            }
            false
        } else {
            // Climber with a walkable position — supported.
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
            let wx = self.world.size_x as i32;
            let wz = self.world.size_z as i32;
            // Bounds check: large_node_surface_y accesses (ax..ax+2, az..az+2).
            if ax < 0 || ax + 1 >= wx || az < 0 || az + 1 >= wz {
                return None;
            }
            if let Some(surface_y) = crate::walkability::large_node_surface_y(&self.world, ax, az)
                && surface_y < pos.y
            {
                let landing = VoxelCoord::new(ax, surface_y, az);
                if crate::walkability::footprint_walkable(
                    &self.world,
                    &self.face_data,
                    landing,
                    species_data.footprint,
                ) {
                    return Some(landing);
                }
            }
            // No valid large node below — degenerate.
            None
        } else {
            // 1x1: scan downward for a Y that meets support criteria.
            for y in (1..pos.y).rev() {
                let candidate = VoxelCoord::new(pos.x, y, pos.z);
                if !crate::walkability::footprint_walkable(
                    &self.world,
                    &self.face_data,
                    candidate,
                    species_data.footprint,
                ) {
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
        let old_pos = creature.position.min;

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
                // footprint-walkable position.
                let fp = self.species_table[&species].footprint;
                match crate::walkability::find_nearest_walkable(
                    &self.world,
                    &self.face_data,
                    old_pos,
                    5,
                    fp,
                ) {
                    Some(pos) => pos,
                    None => return false, // no walkable positions — nothing to do
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
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.position = c.position.with_anchor(landing);
            c.path = None;
            let _ = self.db.update_creature(c);
        }

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

        // Mark creature for reactivation so it resumes behavior (if alive).
        if self
            .db
            .creatures
            .get(&creature_id)
            .is_some_and(|c| c.vital_status == VitalStatus::Alive)
        {
            self.set_creature_activation_tick(creature_id, self.tick + 1);
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
                let _ = self.db.update_task(task);
            }
        }
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.current_task = None;
            creature.path = None;
            creature.wasted_action_count = 0;
            let _ = self.db.update_creature(creature);
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
                    let _ = self.db.update_creature(c);
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
            crate::db::TaskKindTag::Build
            | crate::db::TaskKindTag::Furnish
            | crate::db::TaskKindTag::Tame => {}
            crate::db::TaskKindTag::DineAtHall => {
                self.cleanup_dine_at_hall_task(task_id);
            }
            // No-cleanup tasks: mark Complete so they aren't re-claimed.
            crate::db::TaskKindTag::GoTo
            | crate::db::TaskKindTag::EatBread
            | crate::db::TaskKindTag::EatFruit
            | crate::db::TaskKindTag::Graze
            | crate::db::TaskKindTag::Sleep
            | crate::db::TaskKindTag::Mope
            | crate::db::TaskKindTag::AttackTarget
            | crate::db::TaskKindTag::AttackMove => {
                if let Some(mut t) = self.db.tasks.get(&task_id) {
                    t.state = task::TaskState::Complete;
                    let _ = self.db.update_task(t);
                }
            }
        }

        // Do NOT cascade-cancel dependent tasks here. The command queue
        // should survive autonomous interruptions (flee, hunger, sleep).
        // Player commands handle their own queue cancellation via
        // cancel_creature_queue() before preempting, and creature death
        // calls cancel_creature_queue() after interrupt_task().

        // Clear creature assignment. For resumable tasks (Build, Furnish),
        // this reverts the task to Available if no other creatures remain.
        // For non-resumable tasks, the task is already Complete.
        self.unassign_creature_from_task(creature_id);
    }

    /// Cancel all player-directed queued tasks restricted to a creature.
    /// Used when an unshifted right-click replaces the entire command queue.
    pub(crate) fn cancel_creature_queue(&mut self, creature_id: CreatureId) {
        let queued: Vec<TaskId> = self
            .db
            .tasks
            .by_restrict_to_creature_id(&Some(creature_id), tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|t| {
                t.origin == task::TaskOrigin::PlayerDirected && t.state != task::TaskState::Complete
            })
            .map(|t| t.id)
            .collect();

        for tid in queued {
            if let Some(mut t) = self.db.tasks.get(&tid) {
                t.state = task::TaskState::Complete;
                let _ = self.db.update_task(t);
            }
        }
    }
}

/// Map a species genome SNP region or group name to the corresponding TraitKind.
///
/// Returns `None` for names that don't correspond to traits (e.g., reserved
/// SNP regions like `skin_warmth`). The mapping depends on species because
/// different species use different TraitKind variants for their color traits
/// (BodyColor vs FurColor vs SkinColor).
pub(crate) fn snp_name_to_trait_kind(name: &str, species: Species) -> Option<TraitKind> {
    // suppress unused-variable warning — species is needed for future
    // species-polymorphic mappings but all current non-elf species use
    // the same generic names.
    let _ = species;
    match name {
        // Elf-specific pigmentation.
        "hair_hue" => Some(TraitKind::HairColor),
        "hair_value" => Some(TraitKind::HairValue),
        "hair_saturation" => Some(TraitKind::HairSaturation),
        "eye_hue" => Some(TraitKind::EyeColor),
        "eye_value" => Some(TraitKind::EyeValue),
        "eye_saturation" => Some(TraitKind::EyeSaturation),
        "skin_melanin" => Some(TraitKind::SkinMelanin),
        "skin_ruddiness" => Some(TraitKind::SkinRuddiness),
        "skin_tone" => Some(TraitKind::SkinTone),
        "skin_warmth" => None, // Reserved for future use.

        // Body color (capybara, boar, deer, elephant, hornet, wyvern).
        "body_hue" => Some(TraitKind::BodyColor),
        "body_value" => Some(TraitKind::BodyValue),
        "body_saturation" => Some(TraitKind::BodySaturation),

        // Fur color (monkey, squirrel).
        "fur_hue" => Some(TraitKind::FurColor),
        "fur_value" => Some(TraitKind::FurValue),
        "fur_saturation" => Some(TraitKind::FurSaturation),

        // Skin color (goblin, orc, troll).
        "skin_hue" => Some(TraitKind::SkinColor),
        "skin_value" => Some(TraitKind::SkinValue),
        "skin_saturation" => Some(TraitKind::SkinSaturation),

        // Morphological traits (Phase F) — categorical.
        "hair_style" => Some(TraitKind::HairStyle),
        "accessory" => Some(TraitKind::Accessory),
        "tusk_size" => Some(TraitKind::TuskSize),
        "antler_style" => Some(TraitKind::AntlerStyle),
        "spot_pattern" => Some(TraitKind::SpotPattern),
        "tusk_type" => Some(TraitKind::TuskType),
        "ear_style" => Some(TraitKind::EarStyle),
        "face_marking" => Some(TraitKind::FaceMarking),
        "war_paint" => Some(TraitKind::WarPaint),
        "tail_type" => Some(TraitKind::TailType),
        "horn_style" => Some(TraitKind::HornStyle),
        "stripe_pattern" => Some(TraitKind::StripePattern),
        "wing_style" => Some(TraitKind::WingStyle),
        "scale_pattern" => Some(TraitKind::ScalePattern),

        _ => None,
    }
}

/// Find a categorical group in the species genome config and express it
/// with hue-wheel blending. Returns `None` if the group isn't found.
fn express_blended_hue_group(
    genome: &crate::genome::Genome,
    config: &crate::species::SpeciesGenomeConfig,
    group_name: &str,
    tiebreak_seed: u64,
) -> Option<crate::genome::CategoricalResult> {
    // Find all SNP regions in this categorical group and compute the start offset.
    let mut group_start = None;
    let mut bits_per = 0u32;
    let mut num_categories = 0u32;
    let mut offset = 0u32;

    for snp in &config.species_snps {
        if let crate::species::SnpKind::Categorical { group } = &snp.kind
            && group == group_name
        {
            if group_start.is_none() {
                group_start = Some(offset);
                bits_per = snp.bits;
            }
            num_categories += 1;
        }
        offset += snp.bits;
    }

    let start = group_start?;
    if num_categories < 2 {
        return None;
    }

    // Mix tiebreak seed with group name.
    let group_hash = group_name
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    let seed = tiebreak_seed ^ group_hash;

    Some(crate::genome::express_categorical_blended(
        genome,
        start,
        bits_per,
        num_categories,
        seed,
    ))
}
