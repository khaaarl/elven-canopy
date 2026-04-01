// Worldgen framework — entry point for world generation during StartGame.
//
// This module establishes the generator sequencing pattern used during game
// initialization. When a new game starts, `run_worldgen()` creates a
// dedicated worldgen PRNG (seeded from the world seed), then runs generators
// in a defined order:
//
//   1. **Tree generation** — produces the player's home tree geometry, then
//      scatters lesser (non-sentient) trees across the forest floor via
//      rejection-sampled random placement (see `generate_lesser_trees()`).
//   2. **Fruit generation** — creates 20-40+ unique fruit species with
//      composable parts, properties, coverage constraints, and Vaelith names
//      (see `fruit.rs`).
//   3. **Civilization generation** — creates the player's elf civ (CivId(0),
//      player-controlled) and AI civs from a weighted species distribution.
//      Elf civs get Vaelith names; others get placeholder phonetic names from
//      per-species syllable tables. Each civ gets a species-biased culture tag
//      and optional minority species.
//   4. **Diplomacy generation** — for each ordered civ pair, rolls per-direction
//      awareness (base 50%, species/hostility bonuses, player cap), then assigns
//      an opinion from the species-affinity default table with ~30% random
//      perturbation.
//   5. **Knowledge distribution** — placeholder, will be implemented by F-civ-knowledge.
//
// After all generators complete, the runtime PRNG is derived from the worldgen
// PRNG's state, ensuring the worldgen sequence doesn't affect runtime randomness
// order and that the entire pipeline is deterministic from the world seed.
//
// The `WorldgenResult` struct carries all outputs back to `SimState::with_config()`,
// which uses them to populate the sim's initial state. This includes the `SimDb`
// (pre-populated with civilization and relationship rows) and the player's civ ID.
//
// **WorldgenConfig** is a subsection of `GameConfig` that groups configuration
// for worldgen generators (holds `FruitConfig` and `CivConfig`). The existing
// tree profile config stays at the top level of `GameConfig`.
//
// The test helper `flat_world_sim()` (in `sim/tests/test_helpers.rs`) creates
// treeless flat worlds for tests by calling `generate_civilizations()` and
// `generate_diplomacy()` directly with cached geometry.
//
// **Critical constraint: determinism.** All generators use the worldgen PRNG
// exclusively. No iterated HashMap — use BTreeMap for ordered iteration,
// LookupMap for point queries. No system time, no OS entropy. The generator
// order is fixed and must not change without updating all downstream seeds.

use std::collections::BTreeMap;

use crate::config::{CivConfig, FruitConfig, GameConfig};
use crate::db::{CivRelationship, Civilization, SimDb};

/// Logging callback for worldgen timing. The callback receives a message
/// string for each step start/finish. Callers can route this to
/// `godot_print!`, `eprintln!`, or any other sink.
pub type WgLog = Box<dyn Fn(&str)>;

/// Default log function: prints to stderr.
pub fn stderr_log() -> WgLog {
    Box::new(|msg| eprintln!("{msg}"))
}

/// No-op log function (for tests).
pub fn noop_log() -> WgLog {
    Box::new(|_| {})
}

/// Timer for worldgen steps. Logs "starting: X" on creation, "X took Y" on
/// drop. Always active (worldgen runs once at game start).
struct WgTimer<'a> {
    label: &'static str,
    start: std::time::Instant,
    log: &'a dyn Fn(&str),
}

impl<'a> WgTimer<'a> {
    fn new(label: &'static str, log: &'a dyn Fn(&str)) -> Self {
        log(&format!("[worldgen] starting: {label}"));
        Self {
            label,
            start: std::time::Instant::now(),
            log,
        }
    }
}

impl Drop for WgTimer<'_> {
    fn drop(&mut self) {
        (self.log)(&format!(
            "[worldgen] {} took {:.1?}",
            self.label,
            self.start.elapsed()
        ));
    }
}

/// Start a worldgen timing step. The timer logs elapsed time when dropped.
macro_rules! wg_time {
    ($label:expr, $log:expr) => {
        let _wg_timer = WgTimer::new($label, $log);
    };
}
use crate::db::{GreatTreeInfo, Tree};
use crate::nav::{self, NavGraph};
use crate::structural;
use crate::tree_gen;
use crate::types::{CivId, CivOpinion, CivSpecies, CultureTag, TreeId, VoxelCoord};
use crate::world::VoxelWorld;
use elven_canopy_prng::GameRng;

/// Configuration for worldgen generators. Subsection of `GameConfig`.
///
/// The existing tree profile config stays at the top level of `GameConfig`
/// since tree generation predates this framework.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WorldgenConfig {
    /// Fruit species generation configuration.
    #[serde(default)]
    pub fruit: FruitConfig,
    /// Civilization generation configuration.
    #[serde(default)]
    pub civs: crate::config::CivConfig,
}

/// Output of the worldgen pipeline, consumed by `SimState::with_config()` to
/// populate the sim's initial state.
pub struct WorldgenResult {
    /// The runtime PRNG, seeded from the worldgen PRNG's final state.
    pub runtime_rng: GameRng,

    /// The voxel world with tree geometry and terrain placed.
    pub world: VoxelWorld,

    /// The player's home tree ID.
    pub player_tree_id: TreeId,

    /// Standard navigation graph (1x1x1 creatures).
    pub nav_graph: NavGraph,

    /// Large navigation graph (2x2x2 creatures like elephants).
    pub large_nav_graph: NavGraph,

    /// The SimDb, populated by worldgen generators (civilizations, etc.).
    pub db: SimDb,

    /// The player-controlled civilization's ID (always CivId(0)).
    pub player_civ_id: CivId,
}

/// Run the full worldgen pipeline: generate the world from a seed and config.
///
/// Creates a dedicated worldgen PRNG from the world seed, runs generators in
/// order, then derives the runtime PRNG from the worldgen PRNG's final state.
/// This separation ensures worldgen-only changes (e.g., adding a new generator)
/// don't shift the runtime PRNG sequence, as long as the worldgen PRNG is
/// consumed identically.
pub fn run_worldgen(seed: u64, config: &GameConfig, log: &WgLog) -> WorldgenResult {
    wg_time!("run_worldgen (total)", log.as_ref());

    // Worldgen PRNG: dedicated instance seeded from the world seed.
    // All worldgen generators draw from this PRNG in a fixed order.
    let mut wg_rng = GameRng::new(seed);

    // Generate IDs first — order matters for determinism.
    // Burn two PRNG draws to maintain deterministic sequence after PlayerId
    // removal in F-player-identity (PlayerId::new consumed next_128_bits =
    // 2 × next_u64).
    let _compat = wg_rng.next_128_bits();
    let player_tree_id = TreeId::new(&mut wg_rng);

    // --- Generator 1: Terrain + Trees ---
    let (world, home_tree, lesser_trees) = {
        wg_time!("terrain + tree generation", log.as_ref());
        generate_trees(&mut wg_rng, config, player_tree_id, log)
    };

    // Load lexicon once — used by fruit naming and civ naming.
    let lexicon = elven_canopy_lang::default_lexicon();

    // --- Generator 2: Fruits ---
    let fruit_species = {
        wg_time!("fruit species generation", log.as_ref());
        let mut species = crate::fruit::generate_fruit_species(&mut wg_rng, &config.worldgen.fruit);
        crate::fruit::assign_fruit_names(
            &mut species,
            &mut wg_rng,
            &config.worldgen.fruit,
            &lexicon,
        );
        species
    };

    // Assign a fruit species to the home tree. Pick a random common species
    // so the player's starting tree always produces an accessible fruit.
    let mut home_tree = home_tree;
    if !fruit_species.is_empty() {
        let common_species: Vec<_> = fruit_species
            .iter()
            .filter(|f| f.rarity == crate::fruit::Rarity::Common)
            .collect();
        let chosen = if common_species.is_empty() {
            &fruit_species[0]
        } else {
            let idx = wg_rng.next_u64() as usize % common_species.len();
            common_species[idx]
        };
        home_tree.fruit_species_id = Some(chosen.id);
    }

    // --- Generator 3: Civilizations ---
    let mut db = SimDb::new();

    // Insert fruit species into SimDb.
    for fruit in &fruit_species {
        let _ = db.insert_fruit_species(fruit.clone());
    }

    let player_civ_id;
    {
        wg_time!("civilization generation", log.as_ref());
        player_civ_id =
            generate_civilizations(&mut wg_rng, &config.worldgen.civs, &mut db, &lexicon);
        home_tree.owner = Some(player_civ_id);
    }

    // --- Generator 4: Diplomacy ---
    {
        wg_time!("diplomacy generation", log.as_ref());
        generate_diplomacy(&mut wg_rng, &config.worldgen.civs, &mut db);
    }

    // --- Generator 5: Knowledge distribution (placeholder) ---
    // Will be implemented by F-civ-knowledge. The generator will populate
    // CivFruitKnowledge tables.

    // --- Insert trees into SimDb ---
    // Home tree gets both a Tree row and a GreatTreeInfo row.
    let great_tree_info = GreatTreeInfo {
        id: home_tree.id,
        mana_stored: config.starting_mana,
        mana_capacity: config.starting_mana_capacity,
        fruit_production_rate_ppm: config.fruit_production_rate_ppm,
        carrying_capacity: 20,
        current_load: 0,
    };
    let _ = db.insert_tree(home_tree);
    let _ = db.insert_great_tree_info(great_tree_info);

    // Lesser trees get only a Tree row (no GreatTreeInfo).
    // A configurable fraction of them are assigned a random fruit species.
    let fruit_fraction = config.lesser_trees.fruit_bearing_fraction;
    for mut lesser in lesser_trees {
        if !fruit_species.is_empty() && fruit_fraction > 0.0 {
            // Roll to decide if this tree bears fruit (integer comparison
            // against PPM threshold for determinism — no floats in the hot path).
            let threshold = (fruit_fraction * 1_000_000.0) as u64;
            let roll = wg_rng.next_u64() % 1_000_000;
            if roll < threshold {
                let idx = wg_rng.next_u64() as usize % fruit_species.len();
                lesser.fruit_species_id = Some(fruit_species[idx].id);
            }
        }
        let _ = db.insert_tree(lesser);
    }

    // Build nav graphs from the completed voxel world.
    let nav_graph = {
        wg_time!("nav graph", log.as_ref());
        nav::build_nav_graph(&world, &BTreeMap::new())
    };
    let large_nav_graph = {
        wg_time!("large nav graph", log.as_ref());
        nav::build_large_nav_graph(&world)
    };

    // Derive the runtime PRNG from the worldgen PRNG's current state.
    // This uses the worldgen PRNG to generate a new seed, ensuring the
    // runtime PRNG is deterministically derived but independent of the
    // exact number of draws made during worldgen.
    let runtime_seed = wg_rng.next_u64();
    let runtime_rng = GameRng::new(runtime_seed);

    WorldgenResult {
        runtime_rng,
        world,
        player_tree_id,
        nav_graph,
        large_nav_graph,
        db,
        player_civ_id,
    }
}

/// Tree generator: produces the player's home tree, lesser trees, and
/// populates the voxel world.
///
/// Extracted from the former `SimState::with_config()` inline logic. Runs the
/// energy-based recursive tree generation with structural validation retry loop,
/// then places lesser trees via rejection sampling.
fn generate_trees(
    rng: &mut GameRng,
    config: &GameConfig,
    player_tree_id: TreeId,
    log: &WgLog,
) -> (VoxelWorld, Tree, Vec<Tree>) {
    let (ws_x, ws_y, ws_z) = config.world_size;
    let center_x = ws_x as i32 / 2;
    let center_z = ws_z as i32 / 2;

    let mut world = VoxelWorld::new(ws_x, ws_y, ws_z);
    let mut tree_result = None;

    for _attempt in 0..config.structural.tree_gen_max_retries {
        {
            wg_time!("tree_gen::generate_terrain", log.as_ref());
            tree_gen::generate_terrain(&mut world, config, rng);
        }
        let candidate = {
            wg_time!("tree_gen::generate_tree (tree geometry)", log.as_ref());
            tree_gen::generate_tree(&mut world, config, rng, log.as_ref())
        };
        let valid = {
            wg_time!("structural::validate_tree", log.as_ref());
            structural::validate_tree(&world, config)
        };
        if valid {
            tree_result = Some(candidate);
            break;
        }
        // Clear world for retry. Terrain + tree will be regenerated.
        world = VoxelWorld::new(ws_x, ws_y, ws_z);
    }

    let tree_result = tree_result.expect(
        "Tree generation failed structural validation after max retries. \
         Tree profile parameters are incompatible with material properties.",
    );

    // Record the base Y for the tree position field. Note: this runs after
    // tree generation, so the center column contains Trunk voxels — the scan
    // stops at the trunk and returns floor_y (matching the old behavior).
    let main_surface_y = tree_gen::terrain_surface_y(
        &world,
        config.floor_y,
        config.terrain_max_height,
        center_x,
        center_z,
    );

    let home_tree = Tree {
        id: player_tree_id,
        position: VoxelCoord::new(center_x, main_surface_y, center_z),
        health: 100,
        growth_level: 1,
        owner: None, // Set after civ generation in run_worldgen.
        trunk_voxels: tree_result.trunk_voxels,
        branch_voxels: tree_result.branch_voxels,
        leaf_voxels: tree_result.leaf_voxels,
        root_voxels: tree_result.root_voxels,
        fruit_species_id: None,
    };

    // --- Lesser trees ---
    let lesser_trees = {
        wg_time!("lesser tree generation", log.as_ref());
        generate_lesser_trees(rng, config, &mut world, center_x, center_z, log)
    };

    (world, home_tree, lesser_trees)
}

/// Place lesser trees on the forest floor via rejection sampling.
///
/// Draws random (x, z) positions within the forest floor extent and rejects
/// candidates that are too close to the main tree center or to any
/// already-placed lesser tree. Each accepted position gets a tree generated
/// from a randomly selected profile.
fn generate_lesser_trees(
    rng: &mut GameRng,
    config: &GameConfig,
    world: &mut VoxelWorld,
    main_center_x: i32,
    main_center_z: i32,
    log: &WgLog,
) -> Vec<Tree> {
    let lt_config = &config.lesser_trees;
    if lt_config.count == 0 || lt_config.profiles.is_empty() {
        return Vec::new();
    }

    // Place lesser trees across the entire world (clamped to 1 voxel inside
    // world bounds to avoid edge issues).
    let (ws_x, _, ws_z) = config.world_size;
    let min_x = 1_i32;
    let max_x = ws_x as i32 - 2;
    let min_z = 1_i32;
    let max_z = ws_z as i32 - 2;
    if max_x <= min_x || max_z <= min_z {
        return Vec::new();
    }
    let range_x = (max_x - min_x + 1) as u64;
    let range_z = (max_z - min_z + 1) as u64;

    let min_dist_main_sq =
        lt_config.min_distance_from_main as i64 * lt_config.min_distance_from_main as i64;
    let min_dist_between_sq =
        lt_config.min_distance_between as i64 * lt_config.min_distance_between as i64;

    let mut placed_positions: Vec<(i32, i32)> = Vec::new();
    let mut lesser_trees: Vec<Tree> = Vec::new();
    let mut attempts = 0u32;

    while (lesser_trees.len() as u32) < lt_config.count
        && attempts < lt_config.max_placement_attempts
    {
        attempts += 1;

        // Draw random position within forest floor bounds.
        let x = min_x + (rng.next_u64() % range_x) as i32;
        let z = min_z + (rng.next_u64() % range_z) as i32;

        // Reject if too close to main tree.
        let dx_main = (x - main_center_x) as i64;
        let dz_main = (z - main_center_z) as i64;
        if dx_main * dx_main + dz_main * dz_main < min_dist_main_sq {
            continue;
        }

        // Reject if too close to any already-placed lesser tree.
        let too_close = placed_positions.iter().any(|&(px, pz)| {
            let dx = (x - px) as i64;
            let dz = (z - pz) as i64;
            dx * dx + dz * dz < min_dist_between_sq
        });
        if too_close {
            continue;
        }

        // Find the terrain surface and reject if it's occupied by tree material.
        let surface_y =
            tree_gen::terrain_surface_y(world, config.floor_y, config.terrain_max_height, x, z);

        // Reject if the voxel above the surface is already occupied (main tree).
        let above_surface = world.get(VoxelCoord::new(x, surface_y + 1, z));
        if above_surface != crate::types::VoxelType::Air {
            continue;
        }

        // Pick a random profile.
        let profile_idx = rng.next_u64() as usize % lt_config.profiles.len();
        let profile = &lt_config.profiles[profile_idx];

        // Sink up to 2 voxels into the dirt so the trunk base looks planted
        // regardless of local terrain variation. Clamp to floor_y.
        let sink = 2.min(surface_y - config.floor_y);
        let base_y = surface_y - sink;
        let tree_id = TreeId::new(rng);
        let noop_log: &dyn Fn(&str) = &|_| {};
        let result = tree_gen::generate_tree_at(world, profile, base_y, x, z, rng, noop_log);

        let tree = Tree {
            id: tree_id,
            position: VoxelCoord::new(x, surface_y, z),
            health: 100,
            growth_level: 1,
            owner: None,
            trunk_voxels: result.trunk_voxels,
            branch_voxels: result.branch_voxels,
            leaf_voxels: result.leaf_voxels,
            root_voxels: result.root_voxels,
            fruit_species_id: None,
        };

        placed_positions.push((x, z));
        lesser_trees.push(tree);
    }

    log(&format!(
        "[worldgen]   placed {}/{} lesser trees ({} attempts)",
        lesser_trees.len(),
        lt_config.count,
        attempts,
    ));

    lesser_trees
}

// ---------------------------------------------------------------------------
// Civilization generator
// ---------------------------------------------------------------------------

/// Generate civilizations according to config. The player's elf civ is always
/// created first as `CivId(0)` with `player_controlled = true`. Remaining civs
/// are drawn from the weighted species distribution.
///
/// Returns the player's `CivId`.
pub(crate) fn generate_civilizations(
    rng: &mut GameRng,
    config: &CivConfig,
    db: &mut SimDb,
    lexicon: &elven_canopy_lang::Lexicon,
) -> CivId {
    let player_civ_id = CivId(0);

    // Player's elf civ is always first.
    let player_name = {
        let vname = elven_canopy_lang::names::generate_name(lexicon, rng);
        // Use the surname as the civilization name (like a clan/house name).
        vname.surname
    };

    let player_civ = Civilization {
        id: player_civ_id,
        name: player_name,
        primary_species: CivSpecies::Elf,
        minority_species: Vec::new(),
        culture_tag: CultureTag::Woodland,
        player_controlled: true,
    };
    db.insert_civilization(player_civ).unwrap();

    // Create default military groups for the player civ.
    create_default_military_groups(db, player_civ_id);

    // Build the cumulative weight table for species selection.
    let total_weight: u64 = config.species_weights.values().map(|&w| w as u64).sum();
    if total_weight == 0 {
        return player_civ_id;
    }

    // Generate remaining civs.
    for i in 1..config.civ_count {
        let civ_id = CivId(i);
        let species = pick_weighted_species(rng, &config.species_weights, total_weight);
        let name = generate_civ_name(rng, species, lexicon);
        let culture_tag = pick_culture_tag(rng, species);
        let minority_species = pick_minority_species(rng, species);

        let civ = Civilization {
            id: civ_id,
            name,
            primary_species: species,
            minority_species,
            culture_tag,
            player_controlled: false,
        };
        db.insert_civilization(civ).unwrap();

        // Create default military groups for this AI civ.
        create_default_military_groups(db, civ_id);
    }

    // Guarantee: at least one AI civ must be a hostile species (one whose
    // default opinion toward Elves is Hostile — currently Goblin and Orc).
    // If the random rolls didn't produce one, convert the last AI civ.
    if config.civ_count >= 2 {
        let has_hostile_species = db.civilizations.iter_all().any(|c| {
            c.id != player_civ_id
                && species_default_opinion(c.primary_species, CivSpecies::Elf)
                    == CivOpinion::Hostile
        });

        if !has_hostile_species {
            let last_ai_id = CivId(config.civ_count - 1);
            let new_name = generate_civ_name(rng, CivSpecies::Goblin, lexicon);
            let new_culture = pick_culture_tag(rng, CivSpecies::Goblin);
            if let Some(mut c) = db.civilizations.get(&last_ai_id) {
                c.primary_species = CivSpecies::Goblin;
                c.name = new_name.clone();
                c.culture_tag = new_culture;
                c.minority_species = Vec::new();
                let _ = db.update_civilization(c);
            }
        }
    }

    player_civ_id
}

/// Create the two default military groups for a newly created civilization:
/// - "Civilians" (default, passive with 100% disengage = always flee)
/// - "Soldiers" (non-default, aggressive with prefer ranged)
fn create_default_military_groups(db: &mut SimDb, civ_id: CivId) {
    use crate::building::LogisticsWant;
    use crate::inventory::{ItemKind, MaterialFilter};
    use crate::species::{
        AmmoExhaustedBehavior, EngagementInitiative, EngagementStyle, WeaponPreference,
    };
    let _ = db.insert_military_group_auto(|id| crate::db::MilitaryGroup {
        id,
        civ_id,
        name: "Civilians".to_string(),
        is_default_civilian: true,
        engagement_style: EngagementStyle {
            weapon_preference: WeaponPreference::PreferRanged,
            ammo_exhausted: AmmoExhaustedBehavior::Flee,
            initiative: EngagementInitiative::Defensive,
            disengage_threshold_pct: 100,
        },
        equipment_wants: Vec::new(),
    });
    let _ = db.insert_military_group_auto(|id| crate::db::MilitaryGroup {
        id,
        civ_id,
        name: "Soldiers".to_string(),
        is_default_civilian: false,
        engagement_style: EngagementStyle {
            weapon_preference: WeaponPreference::PreferRanged,
            ammo_exhausted: AmmoExhaustedBehavior::SwitchToMelee,
            initiative: EngagementInitiative::Aggressive,
            disengage_threshold_pct: 0,
        },
        equipment_wants: vec![
            LogisticsWant {
                item_kind: ItemKind::Bow,
                material_filter: MaterialFilter::Any,
                target_quantity: 1,
            },
            LogisticsWant {
                item_kind: ItemKind::Arrow,
                material_filter: MaterialFilter::Any,
                target_quantity: 20,
            },
        ],
    });
}

/// Pick a species from the weighted distribution.
fn pick_weighted_species(
    rng: &mut GameRng,
    weights: &BTreeMap<CivSpecies, u16>,
    total_weight: u64,
) -> CivSpecies {
    let roll = rng.next_u64() % total_weight;
    let mut cumulative = 0u64;
    for (&species, &weight) in weights {
        cumulative += weight as u64;
        if roll < cumulative {
            return species;
        }
    }
    // Fallback (should not happen with valid weights).
    CivSpecies::Human
}

/// Generate a name for a civilization. Elf civs get Vaelith names; others get
/// placeholder phonetic names from per-species syllable tables.
fn generate_civ_name(
    rng: &mut GameRng,
    species: CivSpecies,
    lexicon: &elven_canopy_lang::Lexicon,
) -> String {
    if species == CivSpecies::Elf {
        let vname = elven_canopy_lang::names::generate_name(lexicon, rng);
        return vname.surname;
    }

    // Placeholder phonetic names: 2-3 syllables from per-species tables.
    let (consonants, vowels) = match species {
        CivSpecies::Elf => unreachable!(),
        CivSpecies::Human => (
            &["Br", "Th", "St", "M", "L", "R", "W", "N"][..],
            &["a", "e", "i", "o", "u"][..],
        ),
        CivSpecies::Dwarf => (
            &["Kh", "Gr", "Dr", "Th", "B", "Z", "N", "D"][..],
            &["a", "o", "u", "i"][..],
        ),
        CivSpecies::Goblin => (
            &["Gr", "Sk", "Z", "Kr", "Sn", "Gl", "N"][..],
            &["a", "i", "u", "e"][..],
        ),
        CivSpecies::Orc => (
            &["Gr", "Kr", "Th", "B", "M", "Gor", "Ur"][..],
            &["a", "o", "u"][..],
        ),
        CivSpecies::Troll => (
            &["Tr", "Gr", "Kr", "Br", "Th", "Sk"][..],
            &["o", "u", "a"][..],
        ),
    };

    let syllable_count = 2 + (rng.next_u64() % 2) as usize; // 2-3 syllables
    let mut name = String::new();
    for _ in 0..syllable_count {
        let c = consonants[rng.next_u64() as usize % consonants.len()];
        let v = vowels[rng.next_u64() as usize % vowels.len()];
        name.push_str(c);
        name.push_str(v);
    }

    // Capitalize first letter (already done since consonants are capitalized).
    // Lowercase the rest after the first character.
    let mut result = String::with_capacity(name.len());
    for (i, ch) in name.chars().enumerate() {
        if i == 0 {
            result.extend(ch.to_uppercase());
        } else {
            result.extend(ch.to_lowercase());
        }
    }
    result
}

/// Pick a culture tag with species-biased weights.
fn pick_culture_tag(rng: &mut GameRng, species: CivSpecies) -> CultureTag {
    // (tag, weight) pairs per species. Higher weight = more likely.
    let weights: &[(CultureTag, u16)] = match species {
        CivSpecies::Elf => &[
            (CultureTag::Woodland, 40),
            (CultureTag::Coastal, 25),
            (CultureTag::Mountain, 10),
            (CultureTag::Nomadic, 15),
            (CultureTag::Subterranean, 5),
            (CultureTag::Martial, 5),
        ],
        CivSpecies::Human => &[
            (CultureTag::Woodland, 15),
            (CultureTag::Coastal, 20),
            (CultureTag::Mountain, 15),
            (CultureTag::Nomadic, 20),
            (CultureTag::Subterranean, 10),
            (CultureTag::Martial, 20),
        ],
        CivSpecies::Dwarf => &[
            (CultureTag::Mountain, 40),
            (CultureTag::Subterranean, 35),
            (CultureTag::Woodland, 5),
            (CultureTag::Coastal, 5),
            (CultureTag::Nomadic, 5),
            (CultureTag::Martial, 10),
        ],
        CivSpecies::Goblin => &[
            (CultureTag::Subterranean, 35),
            (CultureTag::Martial, 30),
            (CultureTag::Mountain, 15),
            (CultureTag::Woodland, 10),
            (CultureTag::Nomadic, 10),
            (CultureTag::Coastal, 0),
        ],
        CivSpecies::Orc => &[
            (CultureTag::Martial, 40),
            (CultureTag::Nomadic, 25),
            (CultureTag::Mountain, 15),
            (CultureTag::Subterranean, 10),
            (CultureTag::Woodland, 5),
            (CultureTag::Coastal, 5),
        ],
        CivSpecies::Troll => &[
            (CultureTag::Mountain, 30),
            (CultureTag::Subterranean, 25),
            (CultureTag::Woodland, 20),
            (CultureTag::Nomadic, 15),
            (CultureTag::Martial, 10),
            (CultureTag::Coastal, 0),
        ],
    };

    let total: u64 = weights.iter().map(|(_, w)| *w as u64).sum();
    let roll = rng.next_u64() % total;
    let mut cumulative = 0u64;
    for &(tag, w) in weights {
        cumulative += w as u64;
        if roll < cumulative {
            return tag;
        }
    }
    CultureTag::Woodland // fallback
}

/// Pick minority species for a civilization based on its primary species.
fn pick_minority_species(rng: &mut GameRng, primary: CivSpecies) -> Vec<CivSpecies> {
    let mut minorities = Vec::new();

    match primary {
        CivSpecies::Goblin => {
            // 40% chance of Troll minority.
            if rng.next_u64() % 100 < 40 {
                minorities.push(CivSpecies::Troll);
            }
        }
        CivSpecies::Orc => {
            // 30% chance of Goblin minority, 20% chance of Troll minority.
            if rng.next_u64() % 100 < 30 {
                minorities.push(CivSpecies::Goblin);
            }
            if rng.next_u64() % 100 < 20 {
                minorities.push(CivSpecies::Troll);
            }
        }
        _ => {
            // Elf, Human, Dwarf, Troll civs are typically mono-species.
            // Consume one PRNG draw for determinism.
            let _ = rng.next_u64();
        }
    }

    minorities.sort();
    minorities
}

// ---------------------------------------------------------------------------
// Diplomacy generator
// ---------------------------------------------------------------------------

/// Generate asymmetric diplomacy relationships between all civilization pairs.
///
/// For each ordered pair (i, j), independently rolls awareness per direction.
/// For aware pairs, assigns initial opinion from a species-affinity default
/// table, then applies random perturbation (~30% chance of shifting one step).
///
/// **Guarantee:** After generation, the player civ (CivId(0)) will know at
/// least one hostile civ. If the random rolls don't produce one, a post-pass
/// forces awareness of the first available hostile-species civ.
pub(crate) fn generate_diplomacy(rng: &mut GameRng, config: &CivConfig, db: &mut SimDb) {
    let civ_ids: Vec<(CivId, CivSpecies)> = db
        .civilizations
        .iter_all()
        .map(|c| (c.id, c.primary_species))
        .collect();

    let civ_count = civ_ids.len();

    for i in 0..civ_count {
        for j in 0..civ_count {
            if i == j {
                continue;
            }

            let (civ_a, species_a) = civ_ids[i];
            let (civ_b, species_b) = civ_ids[j];

            // Roll awareness: base 50%, same-species +25%, hostile species +15%
            let mut awareness_pct = 50u64;
            if species_a == species_b {
                awareness_pct += 25;
            }
            let default_opinion = species_default_opinion(species_a, species_b);
            if default_opinion == CivOpinion::Hostile {
                awareness_pct += 15;
            } else if default_opinion == CivOpinion::Friendly {
                awareness_pct += 10;
            }
            // Cap at 95% to keep some mystery.
            awareness_pct = awareness_pct.min(95);

            // Limit starting known civs for the player's civ.
            let player_aware_count = db
                .civ_relationships
                .by_from_civ(&CivId(0), tabulosity::QueryOpts::ASC)
                .len();

            // If this is the player's civ looking outward, respect the cap.
            if civ_a == CivId(0) && player_aware_count >= config.player_starting_known_civs as usize
            {
                let _ = rng.next_u64(); // consume for determinism
                continue;
            }

            let roll = rng.next_u64() % 100;
            if roll >= awareness_pct {
                continue;
            }

            // Aware — assign opinion with perturbation.
            let mut opinion = default_opinion;

            // ~30% chance of one-step perturbation in either direction.
            let perturb_roll = rng.next_u64() % 100;
            if perturb_roll < 15 {
                opinion = opinion.shift_friendlier();
            } else if perturb_roll < 30 {
                opinion = opinion.shift_hostile();
            }

            db.insert_civ_relationship(CivRelationship {
                from_civ: civ_a,
                to_civ: civ_b,
                opinion,
            })
            .unwrap();
        }
    }

    // Post-pass: guarantee bidirectional hostile awareness between the player
    // and at least one hostile-species civ. This ensures raids always have a
    // valid source (they hate us) and the player always sees an enemy in the
    // elfcyclopedia (we hate them).
    //
    // Find the first hostile-species civ and ensure both directions are Hostile.
    for &(civ_id, civ_species) in &civ_ids {
        if civ_id == CivId(0) {
            continue;
        }
        if species_default_opinion(civ_species, CivSpecies::Elf) != CivOpinion::Hostile {
            continue;
        }

        ensure_hostile_rel(db, civ_id, CivId(0));
        ensure_hostile_rel(db, CivId(0), civ_id);
        break;
    }
}

/// Ensure a Hostile relationship exists from `from_civ` to `to_civ`.
/// If a relationship already exists, upgrade it to Hostile. Otherwise insert one.
fn ensure_hostile_rel(db: &mut SimDb, from_civ: CivId, to_civ: CivId) {
    if let Some(mut rel) = db.civ_relationships.get(&(from_civ, to_civ)) {
        if rel.opinion != CivOpinion::Hostile {
            rel.opinion = CivOpinion::Hostile;
            let _ = db.update_civ_relationship(rel);
        }
    } else {
        db.insert_civ_relationship(CivRelationship {
            from_civ,
            to_civ,
            opinion: CivOpinion::Hostile,
        })
        .unwrap();
    }
}

/// Default diplomatic opinion based on species pairing.
pub(crate) fn species_default_opinion(from: CivSpecies, to: CivSpecies) -> CivOpinion {
    use CivSpecies::*;
    match (from, to) {
        // Same species — generally positive.
        (a, b) if a == b => CivOpinion::Friendly,
        // Elf relations.
        (Elf, Human) | (Human, Elf) => CivOpinion::Neutral,
        (Elf, Dwarf) | (Dwarf, Elf) => CivOpinion::Neutral,
        // Dwarf-Human.
        (Dwarf, Human) | (Human, Dwarf) => CivOpinion::Neutral,
        // Goblin is suspicious/hostile to most.
        (Goblin, Orc) | (Orc, Goblin) => CivOpinion::Suspicious,
        (Goblin, Troll) | (Troll, Goblin) => CivOpinion::Neutral,
        (Goblin, _) => CivOpinion::Hostile,
        (_, Goblin) => CivOpinion::Suspicious,
        // Orc is hostile to most.
        (Orc, Troll) | (Troll, Orc) => CivOpinion::Suspicious,
        (Orc, _) => CivOpinion::Hostile,
        (_, Orc) => CivOpinion::Hostile,
        // Troll is suspicious of others.
        (Troll, _) | (_, Troll) => CivOpinion::Suspicious,
        // Human-to-human or other fallback.
        _ => CivOpinion::Neutral,
    }
}
