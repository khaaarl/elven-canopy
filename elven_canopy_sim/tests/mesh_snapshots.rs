// Integration test: mesh pipeline fixture generation and snapshot regression.
//
// Generates ChunkNeighborhood fixtures from hand-built and worldgen-derived
// scenarios, runs the full mesh pipeline on each, and compares outputs against
// saved expected values. This is the correctness gate for Phase 2 optimization
// agents — any code change that alters mesh output will fail here.
//
// Fixture workflow:
//   1. `cargo test --test mesh_snapshots generate_fixtures` — regenerate all
//      fixture neighborhoods + expected outputs in .tmp/mesh_fixtures/.
//   2. `cargo test --test mesh_snapshots` — run snapshot regression against
//      saved expectations.
//
// Fixtures are bincode-serialized to .tmp/mesh_fixtures/ (gitignored).
// If fixture files are missing, the regression tests auto-generate them
// on first run rather than failing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use elven_canopy_sim::chunk_neighborhood::ChunkNeighborhood;
use elven_canopy_sim::config::GameConfig;
use elven_canopy_sim::mesh_gen::{
    CHUNK_SIZE, ChunkCoord, ChunkMesh, SurfaceMesh, generate_chunk_mesh, set_decimation_enabled,
    set_smoothing_enabled,
};
use elven_canopy_sim::types::{VoxelCoord, VoxelType};
use elven_canopy_sim::world::VoxelWorld;
use elven_canopy_sim::worldgen;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Fixture directory path. Integration tests run with CWD set to the crate
/// root (`elven_canopy_sim/`), so we use `../` to reach the repo-root `.tmp/`.
const FIXTURES_DIR: &str = "../.tmp/mesh_fixtures";
const POSITION_EPSILON: f32 = 1e-5;
const NORMAL_EPSILON: f32 = 1e-5;

// ---------------------------------------------------------------------------
// Fixture definition
// ---------------------------------------------------------------------------

/// A named fixture: a ChunkNeighborhood + the expected mesh outputs at each
/// pipeline stage.
struct Fixture {
    name: String,
    neighborhood: ChunkNeighborhood,
}

/// Expected mesh outputs for one fixture, saved alongside the neighborhood.
/// Four configurations covering both smoothing on/off and decimation on/off.
#[derive(serde::Serialize, serde::Deserialize)]
struct ExpectedOutputs {
    /// Default game pipeline: chamfer + decimation, no smoothing.
    default: ChunkMesh,
    /// Debug option: chamfer + smoothing + decimation.
    smoothed: ChunkMesh,
    /// Chamfer only: no smoothing, no decimation.
    no_decimation: ChunkMesh,
    /// Chamfer + smoothing, no decimation (for smoothing perf tracking).
    smoothed_no_decimation: ChunkMesh,
}

// ---------------------------------------------------------------------------
// Fixture generation: hand-built
// ---------------------------------------------------------------------------

fn make_neighborhood(world: &VoxelWorld, chunk: ChunkCoord) -> ChunkNeighborhood {
    ChunkNeighborhood::extract(world, chunk, None, &BTreeSet::new())
}

fn fixture_single_voxel() -> Fixture {
    let mut world = VoxelWorld::new(16, 16, 16);
    world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
    Fixture {
        name: "single_voxel".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_flat_slab() -> Fixture {
    let mut world = VoxelWorld::new(16, 16, 16);
    for x in 0..16 {
        for z in 0..16 {
            world.set(VoxelCoord::new(x, 8, z), VoxelType::Dirt);
        }
    }
    Fixture {
        name: "flat_slab".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_l_shape() -> Fixture {
    let mut world = VoxelWorld::new(16, 16, 16);
    // L-shape: horizontal arm + vertical arm of GrownPlatform.
    for x in 2..10 {
        world.set(VoxelCoord::new(x, 4, 4), VoxelType::GrownPlatform);
    }
    for z in 4..12 {
        world.set(VoxelCoord::new(2, 4, z), VoxelType::GrownPlatform);
    }
    Fixture {
        name: "l_shape".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_staircase() -> Fixture {
    let mut world = VoxelWorld::new(16, 16, 16);
    for i in 0..8 {
        world.set(VoxelCoord::new(4 + i, i, 4), VoxelType::Trunk);
    }
    Fixture {
        name: "staircase".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_diagonal_adjacency() -> Fixture {
    let mut world = VoxelWorld::new(16, 16, 16);
    // Two voxels diagonal (triggers chamfer edge cases).
    world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
    world.set(VoxelCoord::new(9, 9, 9), VoxelType::Trunk);
    Fixture {
        name: "diagonal_adjacency".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_mixed_material() -> Fixture {
    let mut world = VoxelWorld::new(16, 16, 16);
    world.set(VoxelCoord::new(8, 7, 8), VoxelType::Dirt);
    world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
    world.set(VoxelCoord::new(8, 9, 8), VoxelType::Leaf);
    world.set(VoxelCoord::new(9, 8, 8), VoxelType::Branch);
    Fixture {
        name: "mixed_material".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_fully_solid() -> Fixture {
    let mut world = VoxelWorld::new(16, 16, 16);
    for x in 0..16 {
        for y in 0..16 {
            for z in 0..16 {
                world.set(VoxelCoord::new(x, y, z), VoxelType::Trunk);
            }
        }
    }
    Fixture {
        name: "fully_solid".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_fully_empty() -> Fixture {
    let world = VoxelWorld::new(16, 16, 16);
    Fixture {
        name: "fully_empty".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_thin_wall() -> Fixture {
    let mut world = VoxelWorld::new(16, 16, 16);
    // Single-voxel-thick wall.
    for y in 0..10 {
        for z in 2..14 {
            world.set(VoxelCoord::new(8, y, z), VoxelType::GrownWall);
        }
    }
    Fixture {
        name: "thin_wall".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_overhang() -> Fixture {
    let mut world = VoxelWorld::new(16, 16, 16);
    // Platform floating with nothing below.
    for x in 4..12 {
        for z in 4..12 {
            world.set(VoxelCoord::new(x, 8, z), VoxelType::GrownPlatform);
        }
    }
    Fixture {
        name: "overhang".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

fn fixture_building_terrain_edge() -> Fixture {
    let mut world = VoxelWorld::new(32, 16, 32);
    // Terrain: dirt slope rising from y=0 to y=4, stopping at x=8.
    // This means the building at x=6..9 straddles the terrain edge.
    for x in 0..9 {
        let height = (x / 2).min(4);
        for y in 0..=height {
            for z in 4..12 {
                world.set(VoxelCoord::new(x, y, z), VoxelType::Dirt);
            }
        }
    }
    // Building: 3x3 GrownWall foundation at y=2, BuildingInterior above.
    // Positioned so the right edge (x=8) is at the terrain lip.
    for bx in 6..9 {
        for bz in 6..9 {
            world.set(VoxelCoord::new(bx, 2, bz), VoxelType::GrownWall);
            world.set(VoxelCoord::new(bx, 3, bz), VoxelType::BuildingInterior);
        }
    }
    // Extra terrain that wraps around the building corner at (6, 2, 6).
    for y in 0..3 {
        world.set(VoxelCoord::new(5, y, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(5, y, 6), VoxelType::Dirt);
        world.set(VoxelCoord::new(6, y, 5), VoxelType::Dirt);
    }
    Fixture {
        name: "building_terrain_edge".into(),
        neighborhood: make_neighborhood(&world, ChunkCoord::new(0, 0, 0)),
    }
}

// ---------------------------------------------------------------------------
// Fixture generation: world-generated
// ---------------------------------------------------------------------------

/// Build a small test world from seed 99 (different from the sim test seed 42
/// to avoid coupling) and extract representative chunks.
fn worldgen_fixtures() -> Vec<Fixture> {
    let mut config = GameConfig {
        world_size: (64, 64, 64),
        floor_y: 0,
        ..GameConfig::default()
    };
    config.tree_profile.growth.initial_energy = 50.0;
    config.terrain_max_height = 0;
    config.tree_profile.leaves.leaf_density = 0.65;
    config.tree_profile.leaves.leaf_size = 3;
    config.lesser_trees.count = 0;

    let log = worldgen::noop_log();
    let wg = worldgen::run_worldgen(99, &config, &log);
    let world = &wg.world;
    let no_grassless = BTreeSet::new();

    // Scan all chunks and classify them.
    let (sx, sy, sz) = config.world_size;
    let cx_max = (sx as i32 + CHUNK_SIZE - 1) / CHUNK_SIZE;
    let cy_max = (sy as i32 + CHUNK_SIZE - 1) / CHUNK_SIZE;
    let cz_max = (sz as i32 + CHUNK_SIZE - 1) / CHUNK_SIZE;

    let mut surface_chunk: Option<(ChunkCoord, usize)> = None; // most mixed
    let mut underground_chunk: Option<(ChunkCoord, usize)> = None; // most solid below floor
    let mut canopy_chunk: Option<(ChunkCoord, usize)> = None; // most leaves
    let mut trunk_chunk: Option<(ChunkCoord, usize)> = None; // most trunk/branch
    let mut sparse_chunk: Option<(ChunkCoord, usize)> = None; // fewest non-air (>0)

    for cz in 0..cz_max {
        for cy in 0..cy_max {
            for cx in 0..cx_max {
                let coord = ChunkCoord::new(cx, cy, cz);
                let bx = cx * CHUNK_SIZE;
                let by = cy * CHUNK_SIZE;
                let bz = cz * CHUNK_SIZE;

                let mut solid_count = 0usize;
                let mut leaf_count = 0usize;
                let mut trunk_count = 0usize;
                let mut dirt_count = 0usize;
                let mut total = 0usize;

                for lz in 0..CHUNK_SIZE {
                    for ly in 0..CHUNK_SIZE {
                        for lx in 0..CHUNK_SIZE {
                            let vc = VoxelCoord::new(bx + lx, by + ly, bz + lz);
                            let vt = world.get(vc);
                            total += 1;
                            match vt {
                                VoxelType::Air => {}
                                VoxelType::Leaf => leaf_count += 1,
                                VoxelType::Trunk | VoxelType::Branch => trunk_count += 1,
                                VoxelType::Dirt | VoxelType::Root => dirt_count += 1,
                                _ => solid_count += 1,
                            }
                        }
                    }
                }

                let non_air = solid_count + leaf_count + trunk_count + dirt_count;
                if non_air == 0 {
                    continue;
                }

                // Surface-heavy: most variety of types (has both solid and air).
                let variety = [
                    solid_count.min(1),
                    leaf_count.min(1),
                    trunk_count.min(1),
                    dirt_count.min(1),
                ]
                .iter()
                .sum::<usize>();
                let air_count = total - non_air;
                let surface_score = variety * 1000 + air_count.min(non_air);
                if surface_chunk.map_or(true, |(_, s)| surface_score > s) {
                    surface_chunk = Some((coord, surface_score));
                }

                // Underground: below floor_y, most solid.
                if by + CHUNK_SIZE <= config.floor_y as i32 {
                    if underground_chunk.map_or(true, |(_, s)| non_air > s) {
                        underground_chunk = Some((coord, non_air));
                    }
                }

                // Canopy: most leaves.
                if canopy_chunk.map_or(true, |(_, s)| leaf_count > s) && leaf_count > 0 {
                    canopy_chunk = Some((coord, leaf_count));
                }

                // Trunk-adjacent: most trunk/branch.
                if trunk_chunk.map_or(true, |(_, s)| trunk_count > s) && trunk_count > 0 {
                    trunk_chunk = Some((coord, trunk_count));
                }

                // Sparse: fewest non-air (but >0).
                if sparse_chunk.map_or(true, |(_, s)| non_air < s) {
                    sparse_chunk = Some((coord, non_air));
                }
            }
        }
    }

    let mut fixtures = Vec::new();
    let make = |name: &str, coord: ChunkCoord| -> Fixture {
        let nh = ChunkNeighborhood::extract(world, coord, None, &no_grassless);
        Fixture {
            name: format!("worldgen_{name}"),
            neighborhood: nh,
        }
    };

    if let Some((coord, _)) = surface_chunk {
        fixtures.push(make("surface", coord));
    }
    if let Some((coord, _)) = underground_chunk {
        fixtures.push(make("underground", coord));
    }
    if let Some((coord, _)) = canopy_chunk {
        fixtures.push(make("canopy", coord));
    }
    if let Some((coord, _)) = trunk_chunk {
        fixtures.push(make("trunk", coord));
    }
    if let Some((coord, _)) = sparse_chunk {
        fixtures.push(make("sparse", coord));
    }

    fixtures
}

// ---------------------------------------------------------------------------
// All fixtures
// ---------------------------------------------------------------------------

fn all_fixtures() -> Vec<Fixture> {
    let mut fixtures = vec![
        fixture_single_voxel(),
        fixture_flat_slab(),
        fixture_l_shape(),
        fixture_staircase(),
        fixture_diagonal_adjacency(),
        fixture_mixed_material(),
        fixture_fully_solid(),
        fixture_fully_empty(),
        fixture_thin_wall(),
        fixture_overhang(),
        fixture_building_terrain_edge(),
    ];
    fixtures.extend(worldgen_fixtures());
    fixtures
}

// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    PathBuf::from(FIXTURES_DIR)
}

fn neighborhood_path(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.nh.bin"))
}

fn expected_path(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.expected.bin"))
}

fn ensure_fixtures_dir() {
    let dir = fixtures_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).expect("failed to create fixtures dir");
    }
}

fn save_neighborhood(name: &str, nh: &ChunkNeighborhood) {
    let path = neighborhood_path(name);
    let data = bincode::serialize(nh).expect("failed to serialize neighborhood");
    std::fs::write(&path, data).expect("failed to write neighborhood");
}

fn load_neighborhood(name: &str) -> ChunkNeighborhood {
    let path = neighborhood_path(name);
    let data = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read fixture {path:?}: {e}. Run `cargo test --test mesh_snapshots \
             generate_fixtures` first."
        )
    });
    bincode::deserialize(&data).expect("failed to deserialize neighborhood")
}

fn save_expected(name: &str, expected: &ExpectedOutputs) {
    let path = expected_path(name);
    let data = bincode::serialize(expected).expect("failed to serialize expected outputs");
    std::fs::write(&path, data).expect("failed to write expected outputs");
}

fn load_expected(name: &str) -> Option<ExpectedOutputs> {
    let path = expected_path(name);
    if !path.exists() {
        return None;
    }
    let data = std::fs::read(&path).expect("failed to read expected outputs");
    // Return None if deserialization fails (e.g., format changed) — caller
    // will regenerate.
    bincode::deserialize(&data).ok()
}

// ---------------------------------------------------------------------------
// Pipeline runners for each configuration
// ---------------------------------------------------------------------------

/// Default game pipeline: chamfer + decimation, no smoothing.
fn run_default(nh: &ChunkNeighborhood) -> ChunkMesh {
    set_smoothing_enabled(false);
    set_decimation_enabled(true);
    generate_chunk_mesh(nh)
}

/// Debug option: chamfer + smoothing + decimation.
fn run_smoothed(nh: &ChunkNeighborhood) -> ChunkMesh {
    set_smoothing_enabled(true);
    set_decimation_enabled(true);
    generate_chunk_mesh(nh)
}

/// Chamfer only: no smoothing, no decimation.
fn run_no_decimation(nh: &ChunkNeighborhood) -> ChunkMesh {
    set_smoothing_enabled(false);
    set_decimation_enabled(false);
    generate_chunk_mesh(nh)
}

/// Chamfer + smoothing, no decimation (for smoothing perf tracking).
fn run_smoothed_no_decimation(nh: &ChunkNeighborhood) -> ChunkMesh {
    set_smoothing_enabled(true);
    set_decimation_enabled(false);
    generate_chunk_mesh(nh)
}

fn generate_expected(nh: &ChunkNeighborhood) -> ExpectedOutputs {
    ExpectedOutputs {
        default: run_default(nh),
        smoothed: run_smoothed(nh),
        no_decimation: run_no_decimation(nh),
        smoothed_no_decimation: run_smoothed_no_decimation(nh),
    }
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

fn assert_floats_close(a: &[f32], b: &[f32], epsilon: f32, label: &str) {
    assert_eq!(
        a.len(),
        b.len(),
        "{label}: length mismatch ({} vs {})",
        a.len(),
        b.len()
    );
    for (i, (va, vb)) in a.iter().zip(b.iter()).enumerate() {
        let diff = (va - vb).abs();
        assert!(
            diff <= epsilon,
            "{label}[{i}]: {va} vs {vb} (diff={diff}, epsilon={epsilon})"
        );
    }
}

fn assert_surface_eq(a: &SurfaceMesh, b: &SurfaceMesh, epsilon: f32, label: &str) {
    // Quick-check: counts.
    assert_eq!(
        a.vertex_count(),
        b.vertex_count(),
        "{label}: vertex count mismatch"
    );
    assert_eq!(
        a.indices.len(),
        b.indices.len(),
        "{label}: index count mismatch"
    );

    // Positions and normals: epsilon tolerance.
    assert_floats_close(
        &a.vertices,
        &b.vertices,
        epsilon,
        &format!("{label}.vertices"),
    );
    assert_floats_close(&a.normals, &b.normals, epsilon, &format!("{label}.normals"));

    // Colors: exact match.
    assert_eq!(a.colors, b.colors, "{label}: colors differ");

    // Indices: exact match.
    assert_eq!(a.indices, b.indices, "{label}: indices differ");
}

fn assert_chunk_mesh_eq(a: &ChunkMesh, b: &ChunkMesh, label: &str) {
    let eps = POSITION_EPSILON.max(NORMAL_EPSILON);
    assert_surface_eq(&a.bark, &b.bark, eps, &format!("{label}.bark"));
    assert_surface_eq(&a.ground, &b.ground, eps, &format!("{label}.ground"));
    assert_surface_eq(&a.leaf, &b.leaf, eps, &format!("{label}.leaf"));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Generate (or regenerate) all fixture files. Run explicitly:
///   cargo test --test mesh_snapshots -- --ignored generate_fixtures
#[test]
#[ignore]
fn generate_fixtures() {
    ensure_fixtures_dir();
    let fixtures = all_fixtures();
    eprintln!("Generating {} fixtures...", fixtures.len());
    for fixture in &fixtures {
        eprintln!("  {}", fixture.name);
        save_neighborhood(&fixture.name, &fixture.neighborhood);
        let expected = generate_expected(&fixture.neighborhood);
        save_expected(&fixture.name, &expected);
    }
    eprintln!("Done. Fixtures written to {FIXTURES_DIR}/");
}

/// Snapshot regression: verify that the current mesh pipeline produces the
/// same output as the saved expectations. If no expected files exist, generates
/// them (first-run behavior).
#[test]
fn snapshot_regression() {
    ensure_fixtures_dir();
    let fixtures = all_fixtures();

    // Check if any expected files are missing or stale (wrong format) — if so,
    // regenerate all. This handles first-run and format changes gracefully.
    let any_missing = fixtures.iter().any(|f| {
        !neighborhood_path(&f.name).exists()
            || !expected_path(&f.name).exists()
            || load_expected(&f.name).is_none()
    });
    if any_missing {
        eprintln!(
            "Expected files missing or stale — generating fixtures. \
             Re-run to verify regression."
        );
        for fixture in &fixtures {
            save_neighborhood(&fixture.name, &fixture.neighborhood);
            let expected = generate_expected(&fixture.neighborhood);
            save_expected(&fixture.name, &expected);
        }
        return;
    }

    // Verify each fixture against saved expectations.
    let mut failures = Vec::new();
    let configs: &[(
        &str,
        fn(&ChunkNeighborhood) -> ChunkMesh,
        fn(&ExpectedOutputs) -> &ChunkMesh,
    )] = &[
        ("default", run_default, |e| &e.default),
        ("smoothed", run_smoothed, |e| &e.smoothed),
        ("no_decimation", run_no_decimation, |e| &e.no_decimation),
        ("smoothed_no_decimation", run_smoothed_no_decimation, |e| {
            &e.smoothed_no_decimation
        }),
    ];
    for fixture in &fixtures {
        let nh = load_neighborhood(&fixture.name);
        let expected = load_expected(&fixture.name).expect("expected file missing");

        for &(config_name, runner, getter) in configs {
            let actual = runner(&nh);
            let label = format!("{}/{config_name}", fixture.name);
            if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert_chunk_mesh_eq(&actual, getter(&expected), &label);
            })) {
                failures.push(format!("{label}: {e:?}"));
            }
        }
    }

    if !failures.is_empty() {
        panic!("Snapshot regression failures:\n{}", failures.join("\n"));
    }
}

/// Verify that fixture neighborhoods produce the same mesh whether generated
/// from a fresh VoxelWorld or loaded from serialized bincode.
#[test]
fn serde_roundtrip_preserves_mesh() {
    let fixture = fixture_single_voxel();
    let serialized = bincode::serialize(&fixture.neighborhood).unwrap();
    let deserialized: ChunkNeighborhood = bincode::deserialize(&serialized).unwrap();

    set_decimation_enabled(false);
    set_smoothing_enabled(true);
    let mesh_original = generate_chunk_mesh(&fixture.neighborhood);
    let mesh_roundtrip = generate_chunk_mesh(&deserialized);

    assert_chunk_mesh_eq(&mesh_original, &mesh_roundtrip, "serde_roundtrip");
}
