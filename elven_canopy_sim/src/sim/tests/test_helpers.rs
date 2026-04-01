//! Shared test helpers: sim construction, creature spawning, force-idle,
//! position manipulation, and building insertion. Used by all test
//! submodules via `use super::*`.

use super::*;
use std::sync::LazyLock;

/// The fresh seed for this test run — generated once, shared by all tests.
/// Printed on first access so any failure in the run can be reproduced.
static FRESH_SEED: LazyLock<u64> = LazyLock::new(|| {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    // Mix with PID to differentiate parallel CI runners on the same machine.
    let pid = std::process::id() as u64;
    let seed = nanos.wrapping_add(pid.wrapping_mul(0x517cc1b727220a95));
    eprintln!("fresh_test_seed for this run: {seed}");
    seed
});

/// Hardcoded seed for existing tests. These tests have been hardened
/// against many fragility patterns (activation races, stat-dependent
/// behavior, fruit existence, etc.) but haven't yet been validated
/// with fully random seeds. To advance them to `fresh_test_seed()`,
/// switch them one at a time and fix any flakes that surface.
pub(super) fn legacy_test_seed() -> u64 {
    42
}

/// Return the fresh seed for this test run. The seed is generated once
/// per process (via `FRESH_SEED`) and reused by all tests, so
/// `test_sim(fresh_test_seed())` and `flat_world_sim(fresh_test_seed())`
/// hit their respective caches. Printed to stderr on first use (always
/// visible, even for passing runs) so any failure can be reproduced.
///
/// Use this for NEW tests. Existing tests use `legacy_test_seed()`.
#[allow(dead_code)] // Will be used as new tests are added.
pub(super) fn fresh_test_seed() -> u64 {
    *FRESH_SEED
}

/// Cached seed-42 SimState for test_sim(42) performance. Most legacy tests
/// use seed 42 via legacy_test_seed(), so this cache covers them all.
pub(super) static CACHED_SIM_42: LazyLock<SimState> =
    LazyLock::new(|| SimState::with_config(42, test_config()));

/// Cached fresh-seed SimState for test_sim(fresh_test_seed()) performance.
/// Built once per test run with the fresh seed.
static CACHED_SIM_FRESH: LazyLock<SimState> =
    LazyLock::new(|| SimState::with_config(*FRESH_SEED, test_config()));

/// Cached flat-world geometry (world + nav graphs). These are seed-independent
/// (terrain_max_height=0 makes zero PRNG calls), so one copy serves all seeds.
/// The expensive nav graph build (~150ms) runs once; `flat_world_sim` clones
/// the geometry and overlays seed-dependent civs/diplomacy/RNG.
static FLAT_GEOMETRY: LazyLock<(
    crate::world::VoxelWorld,
    crate::nav::NavGraph,
    crate::nav::NavGraph,
)> = LazyLock::new(|| {
    use crate::nav;
    let config = flat_world_config();
    let (ws_x, ws_y, ws_z) = config.world_size;
    let mut world = crate::world::VoxelWorld::new(ws_x, ws_y, ws_z);
    // terrain_max_height=0: flat ground, no PRNG calls.
    crate::tree_gen::generate_terrain(&mut world, &config, &mut elven_canopy_prng::GameRng::new(0));
    world.clear_dirty_voxels();
    world.repack_all();
    let nav_graph = nav::build_nav_graph(&world, &BTreeMap::new());
    let large_nav_graph = nav::build_large_nav_graph(&world);
    (world, nav_graph, large_nav_graph)
});

/// Test config with a small 64^3 world and reduced tree energy.
/// Matches the approach used by nav::tests and tree_gen::tests.
/// This is ~64x fewer voxels than the default 256×128×256 world,
/// making SimState construction dramatically faster in debug builds.
/// Terrain is disabled (terrain_max_height = 0) to preserve existing
/// test behavior (flat forest floor).
pub(super) fn test_config() -> GameConfig {
    let mut config = GameConfig {
        world_size: (64, 64, 64),
        floor_y: 0,
        ..GameConfig::default()
    };
    config.tree_profile.growth.initial_energy = 50.0;
    config.terrain_max_height = 0;
    // Pin leaf config so tests don't break when visual defaults change.
    config.tree_profile.leaves.leaf_density = 0.65;
    config.tree_profile.leaves.leaf_size = 5;
    // Disable lesser trees in tests to avoid PRNG sequence shifts when the
    // default count changes. Tests that specifically exercise lesser trees
    // enable them explicitly.
    config.lesser_trees.count = 0;
    // Adjust spawn positions for the small test world (center=32, floor_y=0).
    for spec in &mut config.initial_creatures {
        spec.spawn_position = VoxelCoord::new(32, 1, 32);
    }
    for pile in &mut config.initial_ground_piles {
        pile.position = VoxelCoord::new(32, 1, 42);
    }
    config
}

/// Create a test SimState with a small world for fast tests.
/// Seed 42 and the fresh seed clone from cached instances; other seeds
/// construct fresh.
pub(super) fn test_sim(seed: u64) -> SimState {
    if seed == 42 {
        CACHED_SIM_42.clone()
    } else if seed == *FRESH_SEED {
        CACHED_SIM_FRESH.clone()
    } else {
        SimState::with_config(seed, test_config())
    }
}

/// Config for a flat, treeless test world. Same small 64^3 dimensions as
/// `test_config()`, but the resulting world has no tree geometry — just
/// flat ground at `floor_y` with open air above. Ideal for combat,
/// projectile, and movement tests that need clear LOS and predictable
/// terrain without coupling to tree generation output.
pub(super) fn flat_world_config() -> GameConfig {
    let mut config = GameConfig {
        world_size: (64, 64, 64),
        floor_y: 0,
        ..GameConfig::default()
    };
    config.terrain_max_height = 0;
    // Spawn positions adjusted for the small world center.
    for spec in &mut config.initial_creatures {
        spec.spawn_position = VoxelCoord::new(32, 1, 32);
    }
    for pile in &mut config.initial_ground_piles {
        pile.position = VoxelCoord::new(32, 1, 42);
    }
    config
}

/// Create a test SimState with a flat, treeless world. Clones cached
/// geometry (world + nav graphs) and overlays seed-dependent state
/// (civs, diplomacy, tree row, RNG). Fast because the expensive nav
/// graph build is amortized across all callers.
///
/// The home tree DB row exists (for `player_tree_id` lookups) but has no
/// voxels. Creatures can be spawned and will snap to the flat ground's
/// nav nodes.
pub(super) fn flat_world_sim(seed: u64) -> SimState {
    use crate::db::{GreatTreeInfo, SimDb, Tree};
    use crate::types::TreeId;
    use crate::worldgen;
    use elven_canopy_prng::GameRng;

    let config = flat_world_config();
    let (ref world, ref nav, ref large_nav) = *FLAT_GEOMETRY;

    // Seed-dependent: IDs, civs, diplomacy, runtime RNG.
    let mut rng = GameRng::new(seed);
    let _compat = rng.next_128_bits();
    let player_tree_id = TreeId::new(&mut rng);
    // Terrain generation is a no-op for flat worlds (terrain_max_height=0),
    // but call it anyway to keep the PRNG sequence in sync with
    // run_worldgen_flat.
    crate::tree_gen::generate_terrain(
        &mut crate::world::VoxelWorld::new(1, 1, 1),
        &config,
        &mut rng,
    );

    let lexicon = elven_canopy_lang::default_lexicon();
    let mut db = SimDb::new();
    let player_civ_id =
        worldgen::generate_civilizations(&mut rng, &config.worldgen.civs, &mut db, &lexicon);
    worldgen::generate_diplomacy(&mut rng, &config.worldgen.civs, &mut db);

    let center_x = config.world_size.0 as i32 / 2;
    let center_z = config.world_size.2 as i32 / 2;
    let home_tree = Tree {
        id: player_tree_id,
        position: VoxelCoord::new(center_x, config.floor_y, center_z),
        health: 100,
        growth_level: 1,
        owner: Some(player_civ_id),
        trunk_voxels: Vec::new(),
        branch_voxels: Vec::new(),
        leaf_voxels: Vec::new(),
        root_voxels: Vec::new(),
        fruit_species_id: None,
    };
    let great_tree_info = GreatTreeInfo {
        id: player_tree_id,
        mana_stored: config.starting_mana,
        mana_capacity: config.starting_mana_capacity,
        fruit_production_rate_ppm: config.fruit_production_rate_ppm,
        carrying_capacity: 20,
        current_load: 0,
    };
    let _ = db.insert_tree(home_tree);
    let _ = db.insert_great_tree_info(great_tree_info);

    let runtime_seed = rng.next_u64();
    let species_table = config.species.clone();

    let mut state = SimState {
        tick: 0,
        rng: GameRng::new(runtime_seed),
        config,
        event_queue: EventQueue::new(),
        db,
        placed_voxels: Vec::new(),
        carved_voxels: Vec::new(),
        face_data_list: Vec::new(),
        face_data: BTreeMap::new(),
        ladder_orientations_list: Vec::new(),
        ladder_orientations: BTreeMap::new(),
        next_structure_id: 0,
        player_tree_id,
        player_civ_id: Some(player_civ_id),
        world: world.clone(),
        nav_graph: nav.clone(),
        large_nav_graph: large_nav.clone(),
        species_table,
        lexicon: Some(lexicon),
        last_build_message: None,
        structure_voxels: BTreeMap::new(),
        spatial_index: BTreeMap::new(),
        mana_wasted_positions: Vec::new(),
        grassless: BTreeSet::new(),
    };

    // Schedule heartbeats (logistics, grass regrowth). No tree heartbeat
    // since the tree has no voxels to grow.
    let logistics_interval = state.config.logistics_heartbeat_interval_ticks;
    state
        .event_queue
        .schedule(logistics_interval, ScheduledEventKind::LogisticsHeartbeat);
    let grass_interval = state.config.grass_regrowth_interval_ticks;
    state
        .event_queue
        .schedule(grass_interval, ScheduledEventKind::GrassRegrowth);

    state
}

// ---------------------------------------------------------------------------
// Creature biology traits
// ---------------------------------------------------------------------------

/// Helper: spawn a creature and return its ID.
///
/// Snapshots existing creature IDs before spawning so that the returned
/// ID is always the *newly created* creature, not a pre-existing one of
/// the same species.
pub(super) fn spawn_creature(sim: &mut SimState, species: Species) -> CreatureId {
    let existing: std::collections::BTreeSet<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == species)
        .map(|c| c.id)
        .collect();
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let tick = sim.tick + 1;
    let cmd = SimCommand {
        player_name: String::new(),
        tick,
        action: SimAction::SpawnCreature {
            species,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], tick + 1);
    sim.db
        .creatures
        .iter_all()
        .find(|c| c.species == species && !existing.contains(&c.id))
        .expect("newly spawned creature should exist")
        .id
}

/// Helper: spawn an elf and return its CreatureId.
pub(super) fn spawn_elf(sim: &mut SimState) -> CreatureId {
    let existing: std::collections::BTreeSet<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .map(|c| c.id)
        .collect();
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], sim.tick + 2);
    sim.db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf && !existing.contains(&c.id))
        .unwrap()
        .id
}

/// Helper: insert a minimal blueprint row so that `structures.project_id` FK
/// validation passes when production code calls `update_structure`.
pub(super) fn insert_stub_blueprint(sim: &mut SimState, project_id: ProjectId) {
    sim.db
        .insert_blueprint(crate::db::Blueprint {
            id: project_id,
            build_type: BuildType::Building,
            voxels: Vec::new(),
            priority: crate::types::Priority::Normal,
            state: crate::blueprint::BlueprintState::Complete,
            task_id: None,
            composition_id: None,
            face_layout: None,
            stress_warning: false,
            original_voxels: Vec::new(),
        })
        .unwrap();
}

/// Helper: insert a minimal task row into the DB so that FK validation passes
/// when other tables (e.g. `item_stacks.reserved_by`) reference this task ID.
/// The task has no meaningful kind or location — it exists only to satisfy FKs.
pub(super) fn insert_stub_task(sim: &mut SimState, task_id: TaskId) {
    sim.db
        .insert_task(crate::db::Task {
            id: task_id,
            kind_tag: TaskKindTag::GoTo,
            state: TaskState::Available,
            location: VoxelCoord::new(0, 0, 0),
            progress: 0,
            total_cost: 0,
            required_species: None,
            origin: TaskOrigin::Automated,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        })
        .unwrap();
}

/// Helper: insert a GoTo task at the given nav node (elf-only).
pub(super) fn insert_goto_task(sim: &mut SimState, location: NavNodeId) -> TaskId {
    let task_id = TaskId::new(&mut sim.rng);
    let location_coord = sim.nav_graph.node(location).position;
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: location_coord,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(task);
    task_id
}

// -----------------------------------------------------------------------
// Blueprint / construction tests
// -----------------------------------------------------------------------

/// Find an Air voxel that is face-adjacent to a trunk voxel.
/// Panics if none found (should never happen with a generated tree).
pub(super) fn find_air_adjacent_to_trunk(sim: &SimState) -> VoxelCoord {
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    for &trunk_coord in &tree.trunk_voxels {
        for &(dx, dy, dz) in &[
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            let neighbor =
                VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y + dy, trunk_coord.z + dz);
            if sim.world.in_bounds(neighbor) && sim.world.get(neighbor) == VoxelType::Air {
                return neighbor;
            }
        }
    }
    panic!("No air voxel adjacent to trunk found");
}

// -----------------------------------------------------------------------
// Build work + incremental materialization tests
// -----------------------------------------------------------------------

/// Helper: find N air voxels adjacent to trunk, all face-adjacent to
/// each other or to solid geometry (valid for a multi-voxel blueprint).
pub(super) fn find_air_strip_adjacent_to_trunk(sim: &SimState, count: usize) -> Vec<VoxelCoord> {
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    // Find a trunk voxel with an air voxel to the +x side, then extend
    // in the +x direction.
    for &trunk_coord in &tree.trunk_voxels {
        let start = VoxelCoord::new(trunk_coord.x + 1, trunk_coord.y, trunk_coord.z);
        if !sim.world.in_bounds(start) || sim.world.get(start) != VoxelType::Air {
            continue;
        }
        let mut strip = vec![start];
        for i in 1..count {
            let next = VoxelCoord::new(start.x + i as i32, start.y, start.z);
            if !sim.world.in_bounds(next) || sim.world.get(next) != VoxelType::Air {
                break;
            }
            strip.push(next);
        }
        if strip.len() == count {
            return strip;
        }
    }
    panic!("Could not find {count} air voxels adjacent to trunk");
}

/// Helper: create a sim with fast build speed for testing.
pub(super) fn build_test_sim() -> SimState {
    let mut config = test_config();
    // Fast builds: 1 tick per voxel for quick test completion.
    config.build_work_ticks_per_voxel = 1;
    SimState::with_config(42, config)
}

// --- DesignateBuilding tests ---

/// Find a ground-level position where a 3x3 building can be placed.
/// Needs solid foundation at y=0 and air above at y=1.
pub(super) fn find_building_site(sim: &SimState) -> VoxelCoord {
    let (sx, _, sz) = sim.config.world_size;
    for x in 1..(sx as i32 - 4) {
        for z in 1..(sz as i32 - 4) {
            let mut all_solid = true;
            let mut all_air = true;
            for dx in 0..3 {
                for dz in 0..3 {
                    let foundation = VoxelCoord::new(x + dx, 0, z + dz);
                    if !sim.world.get(foundation).is_solid() {
                        all_solid = false;
                    }
                    let above = VoxelCoord::new(x + dx, 1, z + dz);
                    if sim.world.get(above) != VoxelType::Air {
                        all_air = false;
                    }
                }
            }
            if all_solid && all_air {
                return VoxelCoord::new(x, 0, z);
            }
        }
    }
    panic!("No valid 3x3 building site found");
}

// --- Furnishing tests ---

/// Insert a completed building into the sim's structures. Returns the
/// StructureId. The `anchor` parameter is the foundation level (solid
/// ground); the CompletedStructure's anchor is set one level higher to
/// match `from_blueprint()`, which computes the bounding box from the
/// BuildingInterior voxels (not the foundation). The building is 3x3x1
/// with solid foundation below and BuildingInterior above.
pub(super) fn insert_completed_building(sim: &mut SimState, anchor: VoxelCoord) -> StructureId {
    let id = StructureId(sim.next_structure_id);
    sim.next_structure_id += 1;

    // Place BuildingInterior voxels in the world and record face data.
    // compute_building_face_layout treats `anchor` as foundation level
    // and creates interior voxels at anchor.y + 1.
    let face_layout = crate::building::compute_building_face_layout(anchor, 3, 3, 1);
    for (&coord, fd) in &face_layout {
        sim.world.set(coord, VoxelType::BuildingInterior);
        sim.face_data.insert(coord, fd.clone());
        sim.face_data_list.push((coord, fd.clone()));
        sim.placed_voxels.push((coord, VoxelType::BuildingInterior));
        sim.structure_voxels.insert(coord, id);
    }

    // Place the foundation as solid GrownWall underneath.
    for z in anchor.z..anchor.z + 3 {
        for x in anchor.x..anchor.x + 3 {
            let foundation = VoxelCoord::new(x, anchor.y, z);
            if sim.world.get(foundation) == VoxelType::Air {
                sim.world.set(foundation, VoxelType::GrownWall);
                sim.placed_voxels.push((foundation, VoxelType::GrownWall));
            }
        }
    }

    // The CompletedStructure anchor is the bounding-box min of the
    // blueprint voxels (BuildingInterior), which is one above foundation.
    let interior_anchor = VoxelCoord::new(anchor.x, anchor.y + 1, anchor.z);

    let project_id = ProjectId::new(&mut sim.rng);

    // Insert blueprint first — structure FK (project_id) references it.
    sim.db
        .insert_blueprint(crate::db::Blueprint {
            id: project_id,
            build_type: BuildType::Building,
            voxels: Vec::new(),
            priority: crate::types::Priority::Normal,
            state: crate::blueprint::BlueprintState::Complete,
            task_id: None,
            composition_id: None,
            face_layout: None,
            stress_warning: false,
            original_voxels: Vec::new(),
        })
        .unwrap();

    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    sim.db
        .insert_structure(CompletedStructure {
            id,
            project_id,
            build_type: BuildType::Building,
            anchor: interior_anchor,
            width: 3,
            depth: 3,
            height: 1,
            completed_tick: sim.tick,
            name: None,
            furnishing: None,
            inventory_id: inv_id,
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        })
        .unwrap();

    // Rebuild nav graph so there are nav nodes inside the building.
    sim.nav_graph = nav::build_nav_graph(&sim.world, &sim.face_data);

    id
}

// -----------------------------------------------------------------------
// Melee attack tests
// -----------------------------------------------------------------------

/// Spawn a creature of the given species near the tree and return its ID.
pub(super) fn spawn_species(sim: &mut SimState, species: Species) -> CreatureId {
    let existing: std::collections::BTreeSet<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == species)
        .map(|c| c.id)
        .collect();
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            species,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], sim.tick + 2);
    sim.db
        .creatures
        .iter_all()
        .find(|c| c.species == species && !existing.contains(&c.id))
        .unwrap()
        .id
}

/// Look up a creature's current nav node from its position.
pub(super) fn creature_node(sim: &SimState, creature_id: CreatureId) -> NavNodeId {
    let creature = sim.db.creatures.get(&creature_id).unwrap();
    sim.graph_for_species(creature.species)
        .node_at(creature.position)
        .expect("creature should have a nav node at its position")
}

/// Force a creature to a specific position, updating the spatial index.
pub(super) fn force_position(sim: &mut SimState, creature_id: CreatureId, new_pos: VoxelCoord) {
    let creature = sim.db.creatures.get(&creature_id).unwrap();
    let old_pos = creature.position;
    let species = creature.species;
    let footprint = sim.species_table[&species].footprint;
    SimState::deregister_creature_from_index(
        &mut sim.spatial_index,
        creature_id,
        old_pos,
        footprint,
    );
    let mut creature = sim.db.creatures.get(&creature_id).unwrap();
    creature.position = new_pos;
    sim.db.update_creature(creature).unwrap();
    SimState::register_creature_in_index(&mut sim.spatial_index, creature_id, new_pos, footprint);
}

/// Set a creature's trait to a specific value (insert or modify).
pub(super) fn set_trait(sim: &mut SimState, creature_id: CreatureId, kind: TraitKind, value: i64) {
    let trait_row = crate::db::CreatureTrait {
        creature_id,
        trait_kind: kind,
        value: TraitValue::Int(value),
    };
    sim.db.upsert_creature_trait(trait_row).unwrap();
}

/// Make a creature idle (NoAction, no next_available_tick, no task).
pub(super) fn force_idle(sim: &mut SimState, creature_id: CreatureId) {
    let mut creature = sim.db.creatures.get(&creature_id).unwrap();
    creature.action_kind = ActionKind::NoAction;
    creature.next_available_tick = None;
    creature.current_task = None;
    creature.path = None;
    sim.db.update_creature(creature).unwrap();
}

/// Like `force_idle` but also suppresses activation so the creature
/// won't be polled. Use when you need the creature truly quiescent so
/// you can precisely control activation counts.
///
/// Sets `next_available_tick = Some(u64::MAX)` rather than `None`
/// because `None` sorts before `Some(0)` in Rust's `Option` ordering,
/// so `poll_ready_creatures` (which queries `None..=Some(tick)`) would
/// treat `None` as "immediately ready." `u64::MAX` is unreachable.
pub(super) fn force_idle_and_cancel_activations(sim: &mut SimState, creature_id: CreatureId) {
    force_idle(sim, creature_id);
    // Suppress: u64::MAX is never <= current tick.
    if let Some(mut c) = sim.db.creatures.get(&creature_id) {
        c.next_available_tick = Some(u64::MAX);
        let _ = sim.db.update_creature(c);
    }
}

/// Prevent a creature from being activated by poll-based activation
/// until the specified tick. Sets `next_available_tick` to `tick` so
/// the creature is available (not on cooldown) but won't be polled
/// before that tick. Since commands run before poll at each tick, a
/// command at `tick` will fire before the creature's poll activation.
///
/// For creatures that should never be activated (e.g., bystanders in a
/// test), pass `u64::MAX`.
pub(super) fn suppress_activation_until(sim: &mut SimState, creature_id: CreatureId, tick: u64) {
    let mut creature = sim.db.creatures.get(&creature_id).unwrap();
    creature.next_available_tick = Some(tick);
    sim.db.update_creature(creature).unwrap();
}

/// Convenience: suppress activation until the next tick (sim.tick + 1).
/// Use for creatures that will receive a command at `sim.tick + 1`.
/// The creature is idle with `next_available_tick = Some(sim.tick + 1)`,
/// meaning the command (which runs before poll at that tick) fires first,
/// and the creature won't be polled before then.
pub(super) fn suppress_activation(sim: &mut SimState, creature_id: CreatureId) {
    let tick = sim.tick + 1;
    suppress_activation_until(sim, creature_id, tick);
}

/// Force a creature to be activated at a specific tick by poll-based
/// activation. Sets `next_available_tick` to the given tick and ensures
/// the creature is idle (NoAction) so it enters the decision cascade.
pub(super) fn schedule_activation_at(sim: &mut SimState, creature_id: CreatureId, tick: u64) {
    let mut creature = sim.db.creatures.get(&creature_id).unwrap();
    creature.action_kind = ActionKind::NoAction;
    creature.next_available_tick = Some(tick);
    sim.db.update_creature(creature).unwrap();
}

/// Zero all 8 stat traits for a creature and reset HP to the species base.
/// Use in tests that were written before the creature-stats feature, so that
/// stat modifiers (CON → HP, STR → melee damage, DEX → arrow deviation)
/// don't perturb the hardcoded expected values.
pub(super) fn zero_creature_stats(sim: &mut SimState, creature_id: CreatureId) {
    use crate::stats::STAT_TRAIT_KINDS;
    use crate::types::TraitValue;
    for kind in STAT_TRAIT_KINDS {
        let mut t = sim.db.creature_traits.get(&(creature_id, kind)).unwrap();
        t.value = TraitValue::Int(0);
        sim.db.update_creature_trait(t).unwrap();
    }
    // Reset HP to species base (undo CON modifier applied at spawn).
    let species = sim.db.creatures.get(&creature_id).unwrap().species;
    let base_hp = sim.species_table[&species].hp_max;
    let mut creature = sim.db.creatures.get(&creature_id).unwrap();
    creature.hp_max = base_hp;
    creature.hp = base_hp;
    sim.db.update_creature(creature).unwrap();
}

/// Give a creature a large attack advantage so melee/ranged attacks always
/// hit (Striking/Archery far exceeds any defender's Evasion + AGI).
/// Also sets crit threshold very high so guaranteed-hit tests don't produce
/// unexpected critical damage. Does NOT touch DEX — changing DEX alters
/// arrow deviation RNG consumption, which shifts the PRNG sequence and
/// breaks timing-sensitive tests.
pub(super) fn force_guaranteed_hits(sim: &mut SimState, creature_id: CreatureId) {
    use crate::db::CreatureTrait;
    use crate::types::TraitValue;
    // Set Striking and Archery to 500 (huge attack bonus).
    // With zeroed defender stats, attacker_total = 500 + DEX + quasi_normal.
    // Even with DEX=0, min attacker_total = 500 + 0 + (-300) = 200 > 0, so
    // always hits.
    for skill in [TraitKind::Striking, TraitKind::Archery] {
        sim.db
            .upsert_creature_trait(CreatureTrait {
                creature_id,
                trait_kind: skill,
                value: TraitValue::Int(500),
            })
            .unwrap();
    }
    // Raise crit threshold so the large attack bonus guarantees a normal Hit,
    // not a CriticalHit (which would double damage and break assertions).
    sim.config.evasion_crit_threshold = 100_000;
}

// -----------------------------------------------------------------------
// Voxel exclusion (F-voxel-exclusion): creatures cannot enter voxels
// occupied by hostile creatures.
// -----------------------------------------------------------------------

/// Helper: place a creature at a specific nav node, updating position
/// and spatial index.
pub(super) fn force_to_node(sim: &mut SimState, creature_id: CreatureId, node_id: NavNodeId) {
    let node_pos = sim.nav_graph.node(node_id).position;
    force_position(sim, creature_id, node_pos);
}

impl SimState {
    /// Test helper: count pending activations for a creature.
    /// In the poll-based model, a creature has 0 or 1 pending activations
    /// depending on whether `next_available_tick` is `Some`.
    pub(super) fn count_pending_activations_for(&self, creature_id: CreatureId) -> usize {
        match self.db.creatures.get(&creature_id) {
            Some(c) if c.next_available_tick.is_some() => 1,
            _ => 0,
        }
    }
}

// -----------------------------------------------------------------------
// Additional shared helpers used by multiple submodules
// -----------------------------------------------------------------------

/// Helper: assign a creature to a military group.
pub(super) fn set_military_group(
    sim: &mut SimState,
    creature_id: CreatureId,
    group: Option<MilitaryGroupId>,
) {
    let mut creature = sim.db.creatures.get(&creature_id).unwrap();
    creature.military_group = group;
    sim.db.update_creature(creature).unwrap();
}

/// Helper: find the player civ's civilian group.
pub(super) fn civilian_group(sim: &SimState) -> crate::db::MilitaryGroup {
    let civ_id = sim.player_civ_id.unwrap();
    sim.db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| g.is_default_civilian)
        .expect("player civ should have a civilian group")
}

/// Helper: find the player civ's soldiers group.
pub(super) fn soldiers_group(sim: &SimState) -> crate::db::MilitaryGroup {
    let civ_id = sim.player_civ_id.unwrap();
    sim.db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| !g.is_default_civilian && g.name == "Soldiers")
        .expect("player civ should have a soldiers group")
}

/// Helper: ensure a hostile civ exists with bidirectional hostility to the player.
pub(super) fn ensure_hostile_civ(sim: &mut SimState) -> CivId {
    let player_civ = sim.player_civ_id.unwrap();

    let hostile_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| {
            c.id != player_civ
                && matches!(
                    c.primary_species,
                    CivSpecies::Goblin | CivSpecies::Orc | CivSpecies::Troll
                )
        })
        .map(|c| c.id);

    // If worldgen didn't produce a hostile-species civ (possible with some
    // random seeds), create one explicitly so raid tests are not flaky.
    let hostile_civ_id = hostile_civ.unwrap_or_else(|| {
        let max_id = sim
            .db
            .civilizations
            .iter_all()
            .map(|c| c.id.0)
            .max()
            .unwrap_or(0);
        let new_id = CivId(max_id + 1);
        sim.db
            .insert_civilization(crate::db::Civilization {
                id: new_id,
                name: "Test Goblins".to_string(),
                primary_species: CivSpecies::Goblin,
                minority_species: Vec::new(),
                culture_tag: CultureTag::Martial,
                player_controlled: false,
            })
            .unwrap();
        new_id
    });

    remove_all_hostile_rels(sim);

    // Remove any existing (possibly non-hostile) relationships between the
    // hostile civ and the player so discover_civ can insert Hostile ones.
    // Without this, discover_civ returns early if a Neutral/Friendly
    // relationship already exists from worldgen.
    let _ = sim
        .db
        .remove_civ_relationship(&(hostile_civ_id, player_civ));
    let _ = sim
        .db
        .remove_civ_relationship(&(player_civ, hostile_civ_id));

    sim.discover_civ(hostile_civ_id, player_civ, CivOpinion::Hostile);
    sim.discover_civ(player_civ, hostile_civ_id, CivOpinion::Hostile);

    hostile_civ_id
}

/// Helper: remove all hostile relationships involving the player civ.
pub(super) fn remove_all_hostile_rels(sim: &mut SimState) {
    let player_civ = sim.player_civ_id.unwrap();
    let forward_ids: Vec<_> = sim
        .db
        .civ_relationships
        .by_from_civ(&player_civ, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.opinion == CivOpinion::Hostile)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in forward_ids {
        sim.db.remove_civ_relationship(&pk).unwrap();
    }
    let reverse_ids: Vec<_> = sim
        .db
        .civ_relationships
        .by_to_civ(&player_civ, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.opinion == CivOpinion::Hostile)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in reverse_ids {
        sim.db.remove_civ_relationship(&pk).unwrap();
    }
}

/// Helper: insert a logistics-enabled building at `anchor`.
pub(super) fn insert_building(
    sim: &mut SimState,
    anchor: VoxelCoord,
    logistics_priority: Option<u8>,
    wants: Vec<crate::building::LogisticsWant>,
) -> StructureId {
    let sid = StructureId(sim.next_structure_id);
    sim.next_structure_id += 1;
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: sid,
            project_id,
            build_type: BuildType::Building,
            anchor,
            width: 3,
            depth: 3,
            height: 2,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::Storehouse),
            inventory_id: inv_id,
            logistics_priority,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        })
        .unwrap();
    sim.set_inv_wants(inv_id, &wants);
    sid
}

/// Helper: create a completed building, furnish as Home, and place 1 bed.
pub(super) fn insert_completed_home(sim: &mut SimState, anchor: VoxelCoord) -> StructureId {
    let structure_id = insert_completed_building(sim, anchor);

    let structure = sim.db.structures.get(&structure_id).unwrap();
    let interior = structure.floor_interior_positions();
    let bed_pos = interior[0];

    let mut structure = sim.db.structures.get(&structure_id).unwrap();
    structure.furnishing = Some(FurnishingType::Home);
    sim.db.update_structure(structure).unwrap();

    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            structure_id,
            coord: bed_pos,
            placed: true,
        })
        .unwrap();

    structure_id
}

/// Spawn a hornet at a specific air position.
pub(super) fn spawn_hornet_at(sim: &mut SimState, pos: VoxelCoord) -> CreatureId {
    let existing: std::collections::BTreeSet<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Hornet)
        .map(|c| c.id)
        .collect();
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            species: Species::Hornet,
            position: pos,
        },
    };
    sim.step(&[cmd], sim.tick + 2);
    sim.db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Hornet && !existing.contains(&c.id))
        .expect("hornet should exist")
        .id
}

/// Arm a creature with a bow and arrows.
pub(super) fn arm_with_bow_and_arrows(sim: &mut SimState, creature_id: CreatureId, arrows: u32) {
    let inv_id = sim.creature_inv(creature_id);
    sim.inv_add_item(
        inv_id,
        ItemKind::Bow,
        1,
        Some(creature_id),
        None,
        None,
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        ItemKind::Arrow,
        arrows,
        Some(creature_id),
        None,
        None,
        0,
        None,
        None,
    );
}

/// Find two connected nav nodes (A has an edge to B).
pub(super) fn find_connected_pair(sim: &SimState) -> (NavNodeId, NavNodeId) {
    for node in sim.nav_graph.live_nodes() {
        if !node.edge_indices.is_empty() {
            let edge = sim.nav_graph.edge(node.edge_indices[0]);
            return (node.id, edge.to);
        }
    }
    panic!("No connected nav nodes found in test sim");
}

/// Find three connected nav nodes in a chain: A->B->C.
pub(super) fn find_chain_of_three(sim: &SimState) -> (NavNodeId, NavNodeId, NavNodeId) {
    for node_b in sim.nav_graph.live_nodes() {
        if node_b.edge_indices.len() >= 2 {
            let edge_0 = sim.nav_graph.edge(node_b.edge_indices[0]);
            let edge_1 = sim.nav_graph.edge(node_b.edge_indices[1]);
            if edge_0.to != edge_1.to {
                return (edge_0.to, node_b.id, edge_1.to);
            }
        }
    }
    panic!("No chain of three nav nodes found");
}

/// Spawn an elf using SpawnCreature command and return the newly spawned ID.
pub(super) fn spawn_test_elf(sim: &mut SimState) -> CreatureId {
    let existing: std::collections::BTreeSet<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .map(|c| c.id)
        .collect();
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick.max(1),
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], sim.tick.max(1) + 1);
    sim.db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf && !existing.contains(&c.id))
        .expect("elf should exist after spawn")
        .id
}

/// Set up a crafting building (workshop/kitchen) via the furnishing command.
pub(super) fn setup_crafting_building(
    sim: &mut SimState,
    furnishing_type: FurnishingType,
) -> StructureId {
    let anchor = find_building_site(sim);
    let structure_id = insert_completed_building(sim, anchor);
    let furnish_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type,
            greenhouse_species: None,
        },
    };
    sim.step(&[furnish_cmd], sim.tick + 1);
    structure_id
}

/// Place all furniture items in a structure (skip the furnishing task).
pub(super) fn place_all_furniture(sim: &mut SimState, structure_id: StructureId) {
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
}

/// Insert a test fruit species with known properties for deterministic tests.
pub(super) fn insert_test_fruit_species(sim: &mut SimState) -> crate::fruit::FruitSpeciesId {
    use crate::fruit::{
        FruitAppearance, FruitColor, FruitPart, FruitShape, FruitSpecies, GrowthHabitat, PartType,
        Rarity,
    };
    use std::collections::BTreeSet;
    let id = crate::types::FruitSpeciesId(999);
    let species = FruitSpecies {
        id,
        vaelith_name: "Testaleth".to_string(),
        english_gloss: "test-berry".to_string(),
        parts: vec![
            FruitPart {
                part_type: PartType::Flesh,
                properties: BTreeSet::new(),
                pigment: None,
                component_units: 37,
            },
            FruitPart {
                part_type: PartType::Fiber,
                properties: BTreeSet::new(),
                pigment: None,
                component_units: 52,
            },
            FruitPart {
                part_type: PartType::Seed,
                properties: BTreeSet::new(),
                pigment: None,
                component_units: 15,
            },
        ],
        habitat: GrowthHabitat::Branch,
        rarity: Rarity::Common,
        greenhouse_cultivable: true,
        appearance: FruitAppearance {
            exterior_color: FruitColor {
                r: 200,
                g: 100,
                b: 50,
            },
            shape: FruitShape::Round,
            size_percent: 100,
            glows: false,
        },
    };
    sim.db.insert_fruit_species(species).unwrap();
    id
}

/// Ensure the home tree has at least one fruit. If the tree already has
/// fruit, does nothing. Otherwise, finds a leaf voxel and places a Fruit
/// one voxel below it as a `TreeFruit` row. Panics if the tree has no
/// leaves (which would indicate a broken worldgen for any test_sim seed).
///
/// This guarantees fruit-dependent tests always have fruit to work with,
/// regardless of PRNG-dependent fruit spawning during worldgen.
pub(super) fn ensure_tree_has_fruit(sim: &mut SimState) -> VoxelCoord {
    let tree_id = sim.player_tree_id;

    // If the tree already has fruit, return the first one.
    let existing: Vec<_> = sim
        .db
        .tree_fruits
        .by_tree_id(&tree_id, tabulosity::QueryOpts::ASC);
    if let Some(tf) = existing.first() {
        return tf.position;
    }

    let tree = sim.db.trees.get(&tree_id).unwrap();

    // Tree has no fruit — place one manually.
    assert!(
        !tree.leaf_voxels.is_empty(),
        "Tree must have leaf voxels to place fruit"
    );

    // Find a leaf with air below it (valid fruit position).
    let fruit_pos = tree
        .leaf_voxels
        .iter()
        .map(|&leaf| VoxelCoord::new(leaf.x, leaf.y - 1, leaf.z))
        .find(|&pos| sim.world.in_bounds(pos) && sim.world.get(pos) == VoxelType::Air)
        .expect("No valid fruit position found (all positions below leaves are blocked)");

    // Ensure the tree has a fruit species. If not, insert a test one.
    let species_id = tree.fruit_species_id.unwrap_or_else(|| {
        let id = insert_test_fruit_species(sim);
        let mut t = sim.db.trees.get(&tree_id).unwrap();
        t.fruit_species_id = Some(id);
        let _ = sim.db.update_tree(t);
        id
    });

    // Place the fruit voxel and insert a TreeFruit row.
    sim.set_voxel(fruit_pos, VoxelType::Fruit);
    let _ = sim.db.insert_tree_fruit_auto(|id| crate::db::TreeFruit {
        id,
        tree_id,
        position: fruit_pos,
        species_id,
    });
    fruit_pos
}

/// Set up an extraction kitchen: furnish a building as Kitchen, add an
/// extraction recipe for a test fruit species, and set nonzero targets.
/// Returns (structure_id, fruit_species_id).
pub(super) fn setup_extraction_kitchen(
    sim: &mut SimState,
) -> (StructureId, crate::fruit::FruitSpeciesId) {
    let species_id = insert_test_fruit_species(sim);
    let structure_id = setup_crafting_building(sim, FurnishingType::Kitchen);

    // Add the extraction recipe for our test species to the kitchen.
    sim.add_active_recipe(
        structure_id,
        Recipe::Extract,
        Some(Material::FruitSpecies(species_id)),
    );

    // Set nonzero targets for the outputs so the monitor will schedule work.
    let active_recipes = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    let ar = active_recipes
        .iter()
        .find(|r| {
            r.recipe == Recipe::Extract && r.material == Some(Material::FruitSpecies(species_id))
        })
        .expect("active recipe should exist");

    let targets = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC);
    for target in &targets {
        let set_cmd = SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeOutputTarget {
                active_recipe_target_id: target.id,
                target_quantity: 100,
            },
        };
        sim.step(&[set_cmd], sim.tick + 1);
    }

    (structure_id, species_id)
}

/// Insert a fruit species with Starchy flesh + FibrousFine fiber +
/// pigmented rind, enabling the full Extract->Mill->Bake and Spin->Weave chains.
pub(super) fn insert_full_chain_fruit_species(sim: &mut SimState) -> crate::fruit::FruitSpeciesId {
    use crate::fruit::{
        DyeColor, FruitAppearance, FruitColor, FruitPart, FruitShape, FruitSpecies, GrowthHabitat,
        PartProperty, PartType, Rarity,
    };
    use std::collections::BTreeSet;
    let id = crate::types::FruitSpeciesId(998);
    let mut starchy_props = BTreeSet::new();
    starchy_props.insert(PartProperty::Starchy);
    let mut fine_fiber_props = BTreeSet::new();
    fine_fiber_props.insert(PartProperty::FibrousFine);
    let species = FruitSpecies {
        id,
        vaelith_name: "Chainberry".to_string(),
        english_gloss: "chain-berry".to_string(),
        parts: vec![
            FruitPart {
                part_type: PartType::Flesh,
                properties: starchy_props,
                pigment: Some(DyeColor::Red),
                component_units: 40,
            },
            FruitPart {
                part_type: PartType::Fiber,
                properties: fine_fiber_props,
                pigment: None,
                component_units: 30,
            },
        ],
        habitat: GrowthHabitat::Branch,
        rarity: Rarity::Common,
        greenhouse_cultivable: false,
        appearance: FruitAppearance {
            exterior_color: FruitColor {
                r: 200,
                g: 50,
                b: 50,
            },
            shape: FruitShape::Round,
            size_percent: 100,
            glows: false,
        },
    };
    sim.db.insert_fruit_species(species).unwrap();
    id
}

/// Add an active recipe with output targets to a crafting building.
pub(super) fn add_recipe_with_targets(
    sim: &mut SimState,
    structure_id: StructureId,
    recipe: Recipe,
    material: Option<Material>,
    target_qty: u32,
) -> crate::types::ActiveRecipeId {
    sim.add_active_recipe(structure_id, recipe, material);
    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == recipe && r.material == material)
        .expect("recipe should be added");
    let ar_id = ar.id;
    let targets = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar_id, tabulosity::QueryOpts::ASC);
    for mut target in targets {
        target.target_quantity = target_qty;
        sim.db.update_active_recipe_target(target).unwrap();
    }
    ar_id
}
/// Spawn multiple test elves and return only the newly spawned IDs.
pub(super) fn spawn_test_elves(sim: &mut SimState, count: usize) -> Vec<CreatureId> {
    let existing: std::collections::BTreeSet<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .map(|c| c.id)
        .collect();
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    for _ in 0..count {
        let mut events = Vec::new();
        sim.spawn_creature(Species::Elf, tree_pos, &mut events);
    }
    sim.db
        .creatures
        .iter_all()
        .filter(|c| {
            c.species == Species::Elf
                && c.vital_status == VitalStatus::Alive
                && !existing.contains(&c.id)
        })
        .map(|c| c.id)
        .collect()
}

/// Helper: assign a creature path.
pub(super) fn assign_path(sim: &mut SimState, creature_id: CreatureId, path_id: PathId) {
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::AssignPath {
            creature_id,
            path_id,
        },
    };
    sim.step(&[cmd], sim.tick + 1);
}

/// Helper: send DesignateTame for a creature.
pub(super) fn designate_tame(sim: &mut SimState, target_id: CreatureId) {
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateTame { target_id },
    };
    sim.step(&[cmd], sim.tick + 1);
}

/// Helper: send CancelTameDesignation for a creature.
pub(super) fn cancel_tame_designation(sim: &mut SimState, target_id: CreatureId) {
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::CancelTameDesignation { target_id },
    };
    sim.step(&[cmd], sim.tick + 1);
}
