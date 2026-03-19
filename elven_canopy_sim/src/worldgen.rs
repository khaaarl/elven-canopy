// Worldgen framework — entry point for world generation during StartGame.
//
// This module establishes the generator sequencing pattern used during game
// initialization. When a new game starts, `run_worldgen()` creates a
// dedicated worldgen PRNG (seeded from the world seed), then runs generators
// in a defined order:
//
//   1. **Tree generation** — produces the player's home tree geometry (existing
//      logic extracted from the sim module).
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
use crate::nav::{self, NavGraph};
use crate::sim::Tree;
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

    /// The player's home tree entity.
    pub home_tree: Tree,

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

    // --- Generator 1: Tree ---
    let (world, home_tree) = {
        wg_time!("tree generation + terrain", log.as_ref());
        generate_tree(&mut wg_rng, config, player_tree_id, log)
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
        let _ = db.fruit_species.insert_no_fk(fruit.clone());
    }

    {
        wg_time!("civilization generation", log.as_ref());
        let player_civ_id =
            generate_civilizations(&mut wg_rng, &config.worldgen.civs, &mut db, &lexicon);
        home_tree.owner = Some(player_civ_id);
    }

    let player_civ_id = home_tree.owner.unwrap();

    // --- Generator 4: Diplomacy ---
    {
        wg_time!("diplomacy generation", log.as_ref());
        generate_diplomacy(&mut wg_rng, &config.worldgen.civs, &mut db);
    }

    // --- Generator 5: Knowledge distribution (placeholder) ---
    // Will be implemented by F-civ-knowledge. The generator will populate
    // CivFruitKnowledge tables.

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
        home_tree,
        nav_graph,
        large_nav_graph,
        db,
        player_civ_id,
    }
}

/// Tree generator: produces the player's home tree and populates the voxel world.
///
/// Extracted from the former `SimState::with_config()` inline logic. Runs the
/// energy-based recursive tree generation with structural validation retry loop.
fn generate_tree(
    rng: &mut GameRng,
    config: &GameConfig,
    player_tree_id: TreeId,
    log: &WgLog,
) -> (VoxelWorld, Tree) {
    let (ws_x, ws_y, ws_z) = config.world_size;
    let center_x = ws_x as i32 / 2;
    let center_z = ws_z as i32 / 2;

    let mut world = VoxelWorld::new(ws_x, ws_y, ws_z);
    let mut tree_result = None;

    for _attempt in 0..config.structural.tree_gen_max_retries {
        let candidate = {
            wg_time!(
                "tree_gen::generate_tree (terrain + tree geometry)",
                log.as_ref()
            );
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
        // Clear world for retry. Terrain will be regenerated by generate_tree.
        world = VoxelWorld::new(ws_x, ws_y, ws_z);
    }

    let tree_result = tree_result.expect(
        "Tree generation failed structural validation after max retries. \
         Tree profile parameters are incompatible with material properties.",
    );

    let home_tree = Tree {
        id: player_tree_id,
        position: VoxelCoord::new(center_x, config.floor_y, center_z),
        health: 100,
        growth_level: 1,
        mana_stored: config.starting_mana_mm,
        mana_capacity: config.starting_mana_capacity_mm,
        fruit_production_rate_ppm: config.fruit_production_rate_ppm,
        carrying_capacity: 20,
        current_load: 0,
        owner: None, // Set after civ generation in run_worldgen.
        trunk_voxels: tree_result.trunk_voxels,
        branch_voxels: tree_result.branch_voxels,
        leaf_voxels: tree_result.leaf_voxels,
        root_voxels: tree_result.root_voxels,
        fruit_positions: Vec::new(),
        fruit_species_id: None,
    };

    (world, home_tree)
}

// ---------------------------------------------------------------------------
// Civilization generator
// ---------------------------------------------------------------------------

/// Generate civilizations according to config. The player's elf civ is always
/// created first as `CivId(0)` with `player_controlled = true`. Remaining civs
/// are drawn from the weighted species distribution.
///
/// Returns the player's `CivId`.
fn generate_civilizations(
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
    db.civilizations.insert_no_fk(player_civ).unwrap();

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
        db.civilizations.insert_no_fk(civ).unwrap();

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
            let _ = db.civilizations.modify_unchecked(&last_ai_id, |c| {
                c.primary_species = CivSpecies::Goblin;
                c.name = new_name.clone();
                c.culture_tag = new_culture;
                c.minority_species = Vec::new();
            });
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
    let _ = db
        .military_groups
        .insert_auto_no_fk(|id| crate::db::MilitaryGroup {
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
    let _ = db
        .military_groups
        .insert_auto_no_fk(|id| crate::db::MilitaryGroup {
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
fn generate_diplomacy(rng: &mut GameRng, config: &CivConfig, db: &mut SimDb) {
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

            db.civ_relationships
                .insert_auto_no_fk(|id| CivRelationship {
                    id,
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
    let existing = db
        .civ_relationships
        .by_from_civ(&from_civ, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.to_civ == to_civ);

    if let Some(rel) = existing {
        if rel.opinion != CivOpinion::Hostile {
            let _ = db.civ_relationships.modify_unchecked(&rel.id, |r| {
                r.opinion = CivOpinion::Hostile;
            });
        }
    } else {
        db.civ_relationships
            .insert_auto_no_fk(|id| CivRelationship {
                id,
                from_civ,
                to_civ,
                opinion: CivOpinion::Hostile,
            })
            .unwrap();
    }
}

/// Default diplomatic opinion based on species pairing.
fn species_default_opinion(from: CivSpecies, to: CivSpecies) -> CivOpinion {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Small-world config for fast tests (matches sim/mod.rs test_config pattern).
    fn test_config() -> GameConfig {
        let mut config = GameConfig {
            world_size: (64, 64, 64),
            floor_y: 0,
            ..GameConfig::default()
        };
        config.tree_profile.growth.initial_energy = 50.0;
        config.terrain_max_height = 0;
        config
    }

    #[test]
    fn worldgen_is_deterministic() {
        // Same seed + config must produce identical results.
        let seed = 42;
        let config = test_config();

        let result1 = run_worldgen(seed, &config, &noop_log());
        let result2 = run_worldgen(seed, &config, &noop_log());

        // Tree geometry must match.
        assert_eq!(
            result1.home_tree.trunk_voxels,
            result2.home_tree.trunk_voxels
        );
        assert_eq!(
            result1.home_tree.branch_voxels,
            result2.home_tree.branch_voxels
        );
        assert_eq!(result1.home_tree.leaf_voxels, result2.home_tree.leaf_voxels);
        assert_eq!(result1.home_tree.root_voxels, result2.home_tree.root_voxels);

        // IDs must match.
        assert_eq!(result1.home_tree.id, result2.home_tree.id);
        assert_eq!(result1.player_civ_id, result2.player_civ_id);

        // Nav graphs must match (node + edge counts).
        assert_eq!(
            result1.nav_graph.node_count(),
            result2.nav_graph.node_count()
        );
        assert_eq!(
            result1.nav_graph.edge_count(),
            result2.nav_graph.edge_count()
        );
        assert_eq!(
            result1.large_nav_graph.node_count(),
            result2.large_nav_graph.node_count()
        );
        assert_eq!(
            result1.large_nav_graph.edge_count(),
            result2.large_nav_graph.edge_count()
        );
    }

    #[test]
    fn different_seeds_produce_different_worlds() {
        let config = test_config();

        let result1 = run_worldgen(1, &config, &noop_log());
        let result2 = run_worldgen(2, &config, &noop_log());

        // Different seeds should produce different tree geometry.
        // (Technically could collide, but astronomically unlikely.)
        assert_ne!(
            result1.home_tree.trunk_voxels,
            result2.home_tree.trunk_voxels
        );
    }

    #[test]
    fn runtime_rng_differs_from_worldgen_start() {
        // The runtime PRNG should not be the same as the initial worldgen PRNG.
        // This verifies the derivation step works.
        let seed = 42;
        let config = test_config();

        let result = run_worldgen(seed, &config, &noop_log());

        // The runtime RNG should produce different values than a fresh RNG
        // with the same seed.
        let mut fresh_rng = GameRng::new(seed);
        let mut runtime_rng = result.runtime_rng;

        assert_ne!(fresh_rng.next_u64(), runtime_rng.next_u64());
    }

    #[test]
    fn worldgen_config_default_is_empty() {
        // WorldgenConfig defaults to an empty struct (no fruit/civ config yet).
        let wc = WorldgenConfig::default();
        // Just verify it round-trips through serde.
        let json = serde_json::to_string(&wc).unwrap();
        let _: WorldgenConfig = serde_json::from_str(&json).unwrap();
    }

    // -------------------------------------------------------------------
    // Civilization worldgen tests
    // -------------------------------------------------------------------

    #[test]
    fn worldgen_creates_player_civ() {
        let config = test_config();
        let result = run_worldgen(42, &config, &noop_log());

        // Player civ is always CivId(0) and player-controlled.
        let player_civ = result.db.civilizations.get(&CivId(0)).unwrap();
        assert!(player_civ.player_controlled);
        assert_eq!(player_civ.primary_species, CivSpecies::Elf);
        assert_eq!(result.player_civ_id, CivId(0));
    }

    #[test]
    fn worldgen_creates_correct_civ_count() {
        let mut config = test_config();
        config.worldgen.civs.civ_count = 5;
        let result = run_worldgen(42, &config, &noop_log());

        let civs: Vec<_> = result.db.civilizations.iter_all().collect();
        assert_eq!(civs.len(), 5);

        // CivId(0) is the player civ; CivId(1)..CivId(4) are AI civs.
        for i in 0..5 {
            assert!(result.db.civilizations.get(&CivId(i as u16)).is_some());
        }
    }

    #[test]
    fn worldgen_ai_civs_are_not_player_controlled() {
        let mut config = test_config();
        config.worldgen.civs.civ_count = 3;
        let result = run_worldgen(42, &config, &noop_log());

        for civ in result.db.civilizations.iter_all() {
            if civ.id == CivId(0) {
                assert!(civ.player_controlled);
            } else {
                assert!(!civ.player_controlled);
            }
        }
    }

    #[test]
    fn worldgen_diplomacy_creates_relationships() {
        let mut config = test_config();
        config.worldgen.civs.civ_count = 4;
        let result = run_worldgen(42, &config, &noop_log());

        // With 4 civs, there should be some relationships (but not necessarily all,
        // since awareness is probabilistic).
        let rels: Vec<_> = result.db.civ_relationships.iter_all().collect();
        assert!(
            !rels.is_empty(),
            "4 civs should produce at least some diplomatic relationships"
        );
    }

    #[test]
    fn worldgen_player_known_civs_capped() {
        let mut config = test_config();
        config.worldgen.civs.civ_count = 20;
        config.worldgen.civs.player_starting_known_civs = 3;
        let result = run_worldgen(42, &config, &noop_log());

        // Player civ should know at most 3 other civs.
        let player_rels = result
            .db
            .civ_relationships
            .by_from_civ(&CivId(0), tabulosity::QueryOpts::ASC);
        assert!(
            player_rels.len() <= 3,
            "Player should know at most 3 civs, got {}",
            player_rels.len()
        );
    }

    #[test]
    fn worldgen_civ_determinism() {
        // Same seed + config must produce identical civilizations.
        let mut config = test_config();
        config.worldgen.civs.civ_count = 8;
        let r1 = run_worldgen(42, &config, &noop_log());
        let r2 = run_worldgen(42, &config, &noop_log());

        let civs1: Vec<_> = r1.db.civilizations.iter_all().collect();
        let civs2: Vec<_> = r2.db.civilizations.iter_all().collect();
        assert_eq!(civs1.len(), civs2.len());
        for (c1, c2) in civs1.iter().zip(civs2.iter()) {
            assert_eq!(c1.id, c2.id);
            assert_eq!(c1.name, c2.name);
            assert_eq!(c1.primary_species, c2.primary_species);
            assert_eq!(c1.culture_tag, c2.culture_tag);
            assert_eq!(c1.player_controlled, c2.player_controlled);
        }

        // Relationships must also match.
        let rels1: Vec<_> = r1.db.civ_relationships.iter_all().collect();
        let rels2: Vec<_> = r2.db.civ_relationships.iter_all().collect();
        assert_eq!(rels1.len(), rels2.len());
        for (r1, r2) in rels1.iter().zip(rels2.iter()) {
            assert_eq!(r1.from_civ, r2.from_civ);
            assert_eq!(r1.to_civ, r2.to_civ);
            assert_eq!(r1.opinion, r2.opinion);
        }
    }

    #[test]
    fn worldgen_different_seeds_produce_different_civs() {
        let mut config = test_config();
        config.worldgen.civs.civ_count = 10;
        let r1 = run_worldgen(1, &config, &noop_log());
        let r2 = run_worldgen(2, &config, &noop_log());

        // Names should differ with different seeds.
        let names1: Vec<_> = r1
            .db
            .civilizations
            .iter_all()
            .map(|c| c.name.clone())
            .collect();
        let names2: Vec<_> = r2
            .db
            .civilizations
            .iter_all()
            .map(|c| c.name.clone())
            .collect();
        assert_ne!(names1, names2);
    }

    #[test]
    fn worldgen_all_civs_have_names() {
        let mut config = test_config();
        config.worldgen.civs.civ_count = 10;
        let result = run_worldgen(42, &config, &noop_log());

        for civ in result.db.civilizations.iter_all() {
            assert!(!civ.name.is_empty(), "CivId({}) has empty name", civ.id.0);
        }
    }

    #[test]
    fn worldgen_bidirectional_hostile_awareness() {
        // Worldgen must guarantee that at least one hostile-species civ has
        // bidirectional hostile awareness with the player: they hate us (so
        // they can raid) AND we hate them (so the player sees the threat).
        // Test across multiple seeds to catch probabilistic failures.
        for seed in 0..20 {
            let config = test_config();
            let result = run_worldgen(seed, &config, &noop_log());

            // Check reverse: at least one civ hates the player.
            let hates_player: Vec<_> = result
                .db
                .civ_relationships
                .by_to_civ(&CivId(0), tabulosity::QueryOpts::ASC)
                .into_iter()
                .filter(|r| r.opinion == CivOpinion::Hostile)
                .collect();

            assert!(
                !hates_player.is_empty(),
                "Seed {seed}: at least one civ must consider the player hostile"
            );

            // Check forward: the player hates at least one civ.
            let player_hates: Vec<_> = result
                .db
                .civ_relationships
                .by_from_civ(&CivId(0), tabulosity::QueryOpts::ASC)
                .into_iter()
                .filter(|r| r.opinion == CivOpinion::Hostile)
                .collect();

            assert!(
                !player_hates.is_empty(),
                "Seed {seed}: the player must be aware of at least one hostile civ"
            );
        }
    }

    #[test]
    fn species_default_opinion_is_symmetric_for_same_species() {
        // Same species → Friendly for all.
        for &species in CivSpecies::ALL.iter() {
            assert_eq!(
                species_default_opinion(species, species),
                CivOpinion::Friendly,
                "Same-species opinion for {:?} should be Friendly",
                species
            );
        }
    }
}
