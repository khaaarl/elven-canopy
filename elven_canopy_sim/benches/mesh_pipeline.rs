// Criterion benchmarks for the chunk mesh generation pipeline.
//
// Measures per-chunk mesh generation cost across hand-built and world-generated
// fixtures. Benchmark groups cover the real-world pipeline and optional
// smoothing, plus individual sub-stages:
//
// Full pipeline groups (end-to-end generate_chunk_mesh):
// - `default`: chamfer + decimation, no smoothing (the real-world game pipeline)
// - `smoothed`: chamfer + smoothing + decimation (debug option)
// - `no_decimation`: chamfer only (no smoothing, no decimation)
// - `smoothed_no_decimation`: chamfer + smoothing (no decimation)
//
// Per-stage groups (isolated sub-stages on pre-built SmoothMesh):
// - `stage_face_gen`: just the face-generation loop (build_smooth_mesh)
// - `stage_chamfer`: chamfer pass only (smoothing disabled) on pre-built SmoothMesh
// - `stage_chamfer_smooth`: chamfer + smoothing on pre-built SmoothMesh
// - `stage_decimation`: decimation on a pre-chamfered SmoothMesh
// - `stage_flatten`: flatten SmoothMesh to ChunkMesh
//
// Fixtures are bincode-serialized ChunkNeighborhood files in .tmp/mesh_fixtures/.
// If missing, run: cargo test -p elven_canopy_sim --test mesh_snapshots
//
// Run with: cargo bench -p elven_canopy_sim

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::path::PathBuf;
use std::time::Duration;

use elven_canopy_sim::chunk_neighborhood::ChunkNeighborhood;
use elven_canopy_sim::mesh_gen::{
    build_smooth_mesh, flatten_to_chunk_mesh, generate_chunk_mesh, run_chamfer_smooth,
    run_decimation, set_decimation_enabled, set_smoothing_enabled,
};

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

/// Fixture directory relative to the crate root (where `cargo bench` runs).
const FIXTURES_DIR: &str = "../.tmp/mesh_fixtures";

struct NamedFixture {
    name: String,
    neighborhood: ChunkNeighborhood,
}

fn load_all_fixtures() -> Vec<NamedFixture> {
    let dir = PathBuf::from(FIXTURES_DIR);
    if !dir.exists() {
        panic!(
            "Fixture directory {FIXTURES_DIR} not found. \
             Run `cargo test -p elven_canopy_sim --test mesh_snapshots` first."
        );
    }
    let mut fixtures = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("failed to read fixtures dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "bin")
                && e.path()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .ends_with(".nh.bin")
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = path
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_suffix(".nh")
            .unwrap()
            .to_string();
        let data = std::fs::read(&path).unwrap_or_else(|e| panic!("Failed to read {path:?}: {e}"));
        let nh: ChunkNeighborhood = bincode::deserialize(&data)
            .unwrap_or_else(|e| panic!("Failed to deserialize {path:?}: {e}"));
        fixtures.push(NamedFixture {
            name,
            neighborhood: nh,
        });
    }
    if fixtures.is_empty() {
        panic!(
            "No fixtures found in {FIXTURES_DIR}. \
             Run `cargo test -p elven_canopy_sim --test mesh_snapshots` first."
        );
    }
    fixtures
}

// ---------------------------------------------------------------------------
// Full pipeline benchmark groups
// ---------------------------------------------------------------------------

/// Default game pipeline: chamfer + decimation, no smoothing.
fn bench_default(c: &mut Criterion) {
    let fixtures = load_all_fixtures();
    let mut group = c.benchmark_group("default");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    for f in &fixtures {
        group.bench_with_input(
            BenchmarkId::new("chunk", &f.name),
            &f.neighborhood,
            |b, nh| {
                b.iter(|| {
                    set_smoothing_enabled(false);
                    set_decimation_enabled(true);
                    generate_chunk_mesh(nh)
                })
            },
        );
    }
    group.finish();
}

/// Debug option: chamfer + smoothing + decimation.
fn bench_smoothed(c: &mut Criterion) {
    let fixtures = load_all_fixtures();
    let mut group = c.benchmark_group("smoothed");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    for f in &fixtures {
        group.bench_with_input(
            BenchmarkId::new("chunk", &f.name),
            &f.neighborhood,
            |b, nh| {
                b.iter(|| {
                    set_smoothing_enabled(true);
                    set_decimation_enabled(true);
                    generate_chunk_mesh(nh)
                })
            },
        );
    }
    group.finish();
}

/// Chamfer only: no smoothing, no decimation.
fn bench_no_decimation(c: &mut Criterion) {
    let fixtures = load_all_fixtures();
    let mut group = c.benchmark_group("no_decimation");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    for f in &fixtures {
        group.bench_with_input(
            BenchmarkId::new("chunk", &f.name),
            &f.neighborhood,
            |b, nh| {
                b.iter(|| {
                    set_smoothing_enabled(false);
                    set_decimation_enabled(false);
                    generate_chunk_mesh(nh)
                })
            },
        );
    }
    group.finish();
}

/// Chamfer + smoothing, no decimation (for smoothing perf tracking).
fn bench_smoothed_no_decimation(c: &mut Criterion) {
    let fixtures = load_all_fixtures();
    let mut group = c.benchmark_group("smoothed_no_decimation");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    for f in &fixtures {
        group.bench_with_input(
            BenchmarkId::new("chunk", &f.name),
            &f.neighborhood,
            |b, nh| {
                b.iter(|| {
                    set_smoothing_enabled(true);
                    set_decimation_enabled(false);
                    generate_chunk_mesh(nh)
                })
            },
        );
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Per-stage benchmark groups
// ---------------------------------------------------------------------------

fn bench_stage_face_gen(c: &mut Criterion) {
    let fixtures = load_all_fixtures();
    let mut group = c.benchmark_group("stage_face_gen");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    for f in &fixtures {
        group.bench_with_input(
            BenchmarkId::new("chunk", &f.name),
            &f.neighborhood,
            |b, nh| b.iter(|| build_smooth_mesh(nh)),
        );
    }
    group.finish();
}

/// Chamfer pass only (no smoothing) on a pre-built SmoothMesh.
fn bench_stage_chamfer(c: &mut Criterion) {
    let fixtures = load_all_fixtures();
    let mut group = c.benchmark_group("stage_chamfer");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    let prepared: Vec<_> = fixtures
        .iter()
        .filter_map(|f| build_smooth_mesh(&f.neighborhood).map(|sm| (&f.name, sm)))
        .collect();

    for (name, base_mesh) in &prepared {
        group.bench_with_input(BenchmarkId::new("chunk", name), base_mesh, |b, sm| {
            b.iter_batched(
                || sm.clone(),
                |mut mesh| {
                    set_smoothing_enabled(false);
                    run_chamfer_smooth(&mut mesh);
                    mesh
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

/// Chamfer + smoothing on a pre-built SmoothMesh.
fn bench_stage_chamfer_smooth(c: &mut Criterion) {
    let fixtures = load_all_fixtures();
    let mut group = c.benchmark_group("stage_chamfer_smooth");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    let prepared: Vec<_> = fixtures
        .iter()
        .filter_map(|f| build_smooth_mesh(&f.neighborhood).map(|sm| (&f.name, sm)))
        .collect();

    for (name, base_mesh) in &prepared {
        group.bench_with_input(BenchmarkId::new("chunk", name), base_mesh, |b, sm| {
            b.iter_batched(
                || sm.clone(),
                |mut mesh| {
                    set_smoothing_enabled(true);
                    run_chamfer_smooth(&mut mesh);
                    mesh
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_stage_decimation(c: &mut Criterion) {
    let fixtures = load_all_fixtures();
    let mut group = c.benchmark_group("stage_decimation");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    // Pre-build and chamfer SmoothMeshes (no smoothing — matches default pipeline).
    let prepared: Vec<_> = fixtures
        .iter()
        .filter_map(|f| {
            build_smooth_mesh(&f.neighborhood).map(|mut sm| {
                set_smoothing_enabled(false);
                run_chamfer_smooth(&mut sm);
                (&f.name, f.neighborhood.chunk, sm)
            })
        })
        .collect();

    for (name, chunk, base_mesh) in &prepared {
        group.bench_with_input(BenchmarkId::new("chunk", name), base_mesh, |b, sm| {
            b.iter_batched(
                || sm.clone(),
                |mut mesh| {
                    set_decimation_enabled(true);
                    run_decimation(&mut mesh, *chunk);
                    mesh
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_stage_flatten(c: &mut Criterion) {
    let fixtures = load_all_fixtures();
    let mut group = c.benchmark_group("stage_flatten");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    // Pre-build, chamfer, and decimate (no smoothing — matches default pipeline).
    let prepared: Vec<_> = fixtures
        .iter()
        .filter_map(|f| {
            build_smooth_mesh(&f.neighborhood).map(|mut sm| {
                set_smoothing_enabled(false);
                set_decimation_enabled(true);
                run_chamfer_smooth(&mut sm);
                run_decimation(&mut sm, f.neighborhood.chunk);
                (&f.name, f.neighborhood.chunk, sm)
            })
        })
        .collect();

    for (name, chunk, mesh) in &prepared {
        group.bench_with_input(BenchmarkId::new("chunk", name), mesh, |b, sm| {
            b.iter(|| flatten_to_chunk_mesh(sm, *chunk))
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_default,
    bench_smoothed,
    bench_no_decimation,
    bench_smoothed_no_decimation,
    bench_stage_face_gen,
    bench_stage_chamfer,
    bench_stage_chamfer_smooth,
    bench_stage_decimation,
    bench_stage_flatten,
);
criterion_main!(benches);
