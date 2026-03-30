// Chunk mesh cache with MegaChunk spatial hierarchy for the gdext bridge.
//
// Sits between the sim's pure `mesh_gen` module and the Godot-facing
// `sim_bridge.rs`. Organises chunks into MegaChunks (16×16 horizontal groups)
// for fast draw-distance, frustum culling, and shadow-only culling. Meshes are
// generated lazily — only when a chunk first enters the visible or shadow set —
// and evicted via LRU when a configurable memory budget is exceeded.
//
// ## Visibility pipeline (per frame)
//
// Each chunk is in one of three states: **visible** (in camera frustum),
// **shadow-only** (outside frustum but inside the shadow caster volume), or
// **hidden**. The shadow caster volume is a light-space oriented bounding box
// around the frustum, extended backward along the light direction by the draw
// distance (see `build_shadow_planes()`).
//
// 1. GDScript sends the light direction, camera position, and 6 frustum planes.
// 2. `update_visibility()` builds the shadow volume planes from the frustum and
//    light direction.
// 3. Coarse pass: each MegaChunk AABB is tested against draw distance, frustum,
//    and shadow volume.
// 4. Fine pass: individual chunk AABBs are classified as visible, shadow-only,
//    or hidden.
// 5. Newly-visible/shadow chunks without cached meshes are submitted to
//    background rayon workers via `submit_chunk()`. Each submission extracts
//    a lightweight `ChunkNeighborhood` snapshot, then spawns the actual mesh
//    generation off the main thread. Completed meshes are drained from an
//    mpsc channel at the start of the next `update_visibility()` call.
// 6. Delta lists (show, hide, to_shadow, from_shadow, generated, evicted) are
//    produced for GDScript to toggle MeshInstance3D visibility, cast_shadow
//    settings, and create/free nodes.
//
// ## LRU eviction
//
// Every cached chunk has a `last_accessed` frame stamp. When total cached mesh
// bytes exceed `memory_budget`, the least-recently-accessed chunk NOT in the
// visible or shadow set is evicted (mesh data freed). GDScript frees the
// corresponding MeshInstance3D node.
//
// ## Dirty chunk deferral
//
// `update_dirty()` only regenerates dirty chunks in the visible or shadow set.
// Non-visible/non-shadow dirty chunks stay in the dirty set and are rebuilt
// when they next enter visibility or the shadow set.
//
// Also owns the `TilingCache` (global prime-period tiling textures) which
// is built once and shared across all chunks — it doesn't depend on world
// state, only on the noise parameters baked into `texture_gen.rs`.
//
// See also: `mesh_gen.rs` (sim crate) for the core mesh generation algorithm,
// `texture_gen.rs` (sim crate) for the tiling texture system,
// `sim_bridge.rs` for the Godot-facing methods that convert `ChunkMesh` into
// `ArrayMesh` objects and upload tiling textures, `tree_renderer.gd` for the
// GDScript rendering side.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use rayon::ThreadPool;

use elven_canopy_sim::chunk_neighborhood::ChunkNeighborhood;
use elven_canopy_sim::mesh_gen::{
    CHUNK_SIZE, ChunkCoord, ChunkMesh, MeshPipelineConfig, generate_chunk_mesh, produces_geometry,
    voxel_to_chunk,
};
use elven_canopy_sim::texture_gen::TilingCache;
use elven_canopy_sim::types::VoxelCoord;
use elven_canopy_sim::world::VoxelWorld;

/// Side length of a MegaChunk in chunks (16 chunks = 256 voxels per side).
pub const MEGA_SIZE: i32 = 16;

// ---------------------------------------------------------------------------
// MegaChunk coordinate
// ---------------------------------------------------------------------------

/// Horizontal (XZ) coordinate of a MegaChunk. Each unit spans `MEGA_SIZE`
/// chunks. There is no Y component — a MegaChunk is a full-height column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MegaChunkCoord {
    pub mx: i32,
    pub mz: i32,
}

impl MegaChunkCoord {
    #[cfg(test)]
    pub const fn new(mx: i32, mz: i32) -> Self {
        Self { mx, mz }
    }
}

/// Convert a chunk coordinate to the MegaChunk that contains it.
pub fn chunk_to_mega(coord: ChunkCoord) -> MegaChunkCoord {
    MegaChunkCoord {
        mx: coord.cx.div_euclid(MEGA_SIZE),
        mz: coord.cz.div_euclid(MEGA_SIZE),
    }
}

// ---------------------------------------------------------------------------
// Axis-aligned bounding box (voxel space, f32)
// ---------------------------------------------------------------------------

/// Axis-aligned bounding box in world (voxel) coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb3 {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb3 {
    /// AABB for a single chunk (16³ voxels starting at chunk_coord * 16).
    pub fn from_chunk(c: ChunkCoord) -> Self {
        let s = CHUNK_SIZE as f32;
        Self {
            min: [c.cx as f32 * s, c.cy as f32 * s, c.cz as f32 * s],
            max: [
                (c.cx + 1) as f32 * s,
                (c.cy + 1) as f32 * s,
                (c.cz + 1) as f32 * s,
            ],
        }
    }

    /// Expand this AABB to also contain `other`.
    pub fn union(self, other: Aabb3) -> Self {
        Self {
            min: [
                self.min[0].min(other.min[0]),
                self.min[1].min(other.min[1]),
                self.min[2].min(other.min[2]),
            ],
            max: [
                self.max[0].max(other.max[0]),
                self.max[1].max(other.max[1]),
                self.max[2].max(other.max[2]),
            ],
        }
    }

    /// Center point of the AABB.
    pub fn center(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// Squared XZ distance from a point to the nearest point on this AABB
    /// (ignoring Y). Returns 0.0 if the point is inside the XZ footprint.
    pub fn horizontal_distance_sq(&self, px: f32, pz: f32) -> f32 {
        let dx = px - px.clamp(self.min[0], self.max[0]);
        let dz = pz - pz.clamp(self.min[2], self.max[2]);
        dx * dx + dz * dz
    }

    /// Returns true if this AABB is entirely outside the frustum defined by
    /// the given planes. Each plane is `[nx, ny, nz, d]` matching Godot's
    /// `Plane` class where `distance_to(p) = normal.dot(p) - d`.
    ///
    /// Godot's `Camera3D::get_frustum()` returns planes with **outward-facing
    /// normals** (near plane normal points back toward the camera, far plane
    /// normal points into the distance, etc.). A point is inside the frustum
    /// when `n·p - d < 0` for all 6 planes — i.e., on the negative (inward)
    /// side of every outward-facing plane.
    ///
    /// To cull an AABB, we test the **N-vertex** (the corner that minimizes
    /// `n·p`). If even the N-vertex satisfies `n·p - d >= 0`, the entire
    /// AABB is on the outward side of that plane and can be culled.
    pub fn is_outside_frustum(&self, planes: &[[f32; 4]]) -> bool {
        for plane in planes {
            let [nx, ny, nz, d] = *plane;
            // N-vertex: the corner of the AABB that minimizes n·p.
            // If even this "most inside" corner is on the outside, the
            // entire AABB is outside this plane.
            let nvx = if nx >= 0.0 { self.min[0] } else { self.max[0] };
            let nvy = if ny >= 0.0 { self.min[1] } else { self.max[1] };
            let nvz = if nz >= 0.0 { self.min[2] } else { self.max[2] };
            if nx * nvx + ny * nvy + nz * nvz - d >= 0.0 {
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Shadow volume construction (light-space AABB)
// ---------------------------------------------------------------------------

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-10 {
        None
    } else {
        Some([v[0] / len, v[1] / len, v[2] / len])
    }
}

/// Build the set of planes that define the shadow caster volume for a
/// directional light.
///
/// Uses a **light-space AABB** approach: the 8 frustum corners are
/// transformed into a coordinate system aligned with the light direction,
/// the AABB is computed in that space, the near side is extended backward
/// along the light axis by `extend_distance`, and the 6 AABB planes are
/// transformed back to world space.
///
/// This produces a tight oriented bounding box that varies smoothly with
/// camera movement — no silhouette classification, no discontinuous flips.
///
/// `extend_distance` controls how far upstream (toward the light source)
/// the volume extends beyond the frustum. Typically set to the draw
/// distance so that shadow casters within range are included.
///
/// The returned planes use the same convention as `is_outside_frustum()`:
/// outward-facing normals, inside when `n·p - d < 0`.
pub fn build_shadow_planes(
    frustum: &[[f32; 4]],
    light_dir: [f32; 3],
    extend_distance: f32,
) -> Vec<[f32; 4]> {
    if frustum.len() < 6 {
        return Vec::new();
    }

    // Build a right-handed orthonormal basis where Z = light_dir (toward scene).
    let z_axis = light_dir;
    // Choose an up hint that isn't parallel to light_dir.
    let up_hint = if light_dir[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let Some(x_axis) = normalize3(cross3(up_hint, z_axis)) else {
        return Vec::new();
    };
    let y_axis = cross3(z_axis, x_axis); // already unit length

    // Extract the 8 frustum corners by intersecting triples of planes.
    let corners = frustum_corners(frustum);
    if corners.is_empty() {
        return Vec::new();
    }

    // Project corners into light space.
    let mut ls_min = [f32::MAX; 3];
    let mut ls_max = [f32::MIN; 3];
    for c in &corners {
        let lx = dot3(*c, x_axis);
        let ly = dot3(*c, y_axis);
        let lz = dot3(*c, z_axis);
        ls_min[0] = ls_min[0].min(lx);
        ls_min[1] = ls_min[1].min(ly);
        ls_min[2] = ls_min[2].min(lz);
        ls_max[0] = ls_max[0].max(lx);
        ls_max[1] = ls_max[1].max(ly);
        ls_max[2] = ls_max[2].max(lz);
    }

    // Extend the near side backward along the light axis (toward the light
    // source, i.e., negative Z in light space) so shadow casters upstream of
    // the frustum are included.
    ls_min[2] -= extend_distance;

    // Build 6 planes from the light-space AABB, transformed back to world space.
    // Each AABB face has an outward normal in light space; we express it in world
    // space and compute d from a point on the face.
    vec![
        // +X face: outward normal = +x_axis, d = ls_max[0]
        [x_axis[0], x_axis[1], x_axis[2], ls_max[0]],
        // -X face: outward normal = -x_axis, d = -ls_min[0]
        [-x_axis[0], -x_axis[1], -x_axis[2], -ls_min[0]],
        // +Y face: outward normal = +y_axis, d = ls_max[1]
        [y_axis[0], y_axis[1], y_axis[2], ls_max[1]],
        // -Y face: outward normal = -y_axis, d = -ls_min[1]
        [-y_axis[0], -y_axis[1], -y_axis[2], -ls_min[1]],
        // +Z face (downstream, far from light): outward normal = +z_axis, d = ls_max[2]
        [z_axis[0], z_axis[1], z_axis[2], ls_max[2]],
        // -Z face (upstream, toward light): outward normal = -z_axis, d = -ls_min[2]
        [-z_axis[0], -z_axis[1], -z_axis[2], -ls_min[2]],
    ]
}

/// The 8 frustum corner triples (indices into the 6 frustum planes).
/// Godot order: near(0), far(1), left(2), right(3), top(4), bottom(5).
const CORNER_TRIPLES: [(usize, usize, usize); 8] = [
    (0, 2, 4), // near-left-top
    (0, 3, 4), // near-right-top
    (0, 2, 5), // near-left-bottom
    (0, 3, 5), // near-right-bottom
    (1, 2, 4), // far-left-top
    (1, 3, 4), // far-right-top
    (1, 2, 5), // far-left-bottom
    (1, 3, 5), // far-right-bottom
];

/// Extract the 8 frustum corners by intersecting triples of adjacent planes.
fn frustum_corners(frustum: &[[f32; 4]]) -> Vec<[f32; 3]> {
    let mut corners = Vec::with_capacity(8);
    for &(a, b, c) in &CORNER_TRIPLES {
        if let Some(p) = intersect_three_planes(frustum[a], frustum[b], frustum[c]) {
            corners.push(p);
        }
    }
    corners
}

/// Intersect three planes to find a single point.
/// Each plane is `[nx, ny, nz, d]` with convention `n·p = d` on the plane.
fn intersect_three_planes(p1: [f32; 4], p2: [f32; 4], p3: [f32; 4]) -> Option<[f32; 3]> {
    let n1 = [p1[0], p1[1], p1[2]];
    let n2 = [p2[0], p2[1], p2[2]];
    let n3 = [p3[0], p3[1], p3[2]];

    let det = n1[0] * (n2[1] * n3[2] - n2[2] * n3[1]) - n1[1] * (n2[0] * n3[2] - n2[2] * n3[0])
        + n1[2] * (n2[0] * n3[1] - n2[1] * n3[0]);

    if det.abs() < 1e-10 {
        return None;
    }

    let inv = 1.0 / det;
    let x = (p1[3] * (n2[1] * n3[2] - n2[2] * n3[1]) - p2[3] * (n1[1] * n3[2] - n1[2] * n3[1])
        + p3[3] * (n1[1] * n2[2] - n1[2] * n2[1]))
        * inv;
    let y = (p2[3] * (n1[0] * n3[2] - n1[2] * n3[0]) - p1[3] * (n2[0] * n3[2] - n2[2] * n3[0])
        + p3[3] * (n1[2] * n2[0] - n1[0] * n2[2]))
        * inv;
    let z = (p1[3] * (n2[0] * n3[1] - n2[1] * n3[0]) - p2[3] * (n1[0] * n3[1] - n1[1] * n3[0])
        + p3[3] * (n1[0] * n2[1] - n1[1] * n2[0]))
        * inv;

    Some([x, y, z])
}

// ---------------------------------------------------------------------------
// MegaChunk
// ---------------------------------------------------------------------------

/// A 16×16 horizontal group of chunk columns. Stores the set of chunk
/// coordinates known to contain renderable geometry (from the initial world
/// scan) and a coarse AABB for fast frustum/distance testing.
pub struct MegaChunk {
    /// Chunk coords with geometry (may or may not have cached meshes yet).
    pub chunks: BTreeSet<ChunkCoord>,
    /// AABB enclosing all chunks in `chunks`. None if empty.
    pub aabb: Option<Aabb3>,
}

impl MegaChunk {
    pub fn new() -> Self {
        Self {
            chunks: BTreeSet::new(),
            aabb: None,
        }
    }

    /// Add a chunk and expand the AABB.
    pub fn add_chunk(&mut self, coord: ChunkCoord) {
        self.chunks.insert(coord);
        let chunk_aabb = Aabb3::from_chunk(coord);
        self.aabb = Some(match self.aabb {
            Some(a) => a.union(chunk_aabb),
            None => chunk_aabb,
        });
    }

    /// Remove a chunk and recompute the AABB from the remaining chunks.
    pub fn remove_chunk(&mut self, coord: &ChunkCoord) {
        self.chunks.remove(coord);
        self.recompute_aabb();
    }

    /// Recompute AABB from scratch (used after removal).
    fn recompute_aabb(&mut self) {
        self.aabb = self
            .chunks
            .iter()
            .map(|c| Aabb3::from_chunk(*c))
            .reduce(|a, b| a.union(b));
    }
}

// ---------------------------------------------------------------------------
// Performance timing
// ---------------------------------------------------------------------------

/// Collects per-frame and per-operation timing samples for profiling.
/// Stores durations in microseconds. Call `print_summary()` on shutdown to
/// dump percentile tables to stdout.
pub struct PerfStats {
    /// Per-frame: total `update_visibility()` wall time.
    pub visibility_total_us: Vec<u32>,
    /// Per-frame: coarse + fine culling pass within `update_visibility()`.
    pub culling_us: Vec<u32>,
    /// Per-chunk: single chunk mesh generation (inside Rayon workers).
    pub gen_single_chunk_us: Vec<u32>,
    /// Per-frame: total `update_dirty()` wall time.
    /// Only recorded when at least one dirty chunk is processed.
    pub dirty_update_us: Vec<u32>,
    /// Per-chunk: `build_chunk_array_mesh` (Rust→Godot array conversion).
    pub array_mesh_build_us: Vec<u32>,
    /// Per-frame: LRU eviction pass within `update_visibility()`.
    /// Only recorded when eviction actually runs.
    pub eviction_us: Vec<u32>,
    /// Per-frame: number of chunks that passed frustum culling.
    pub visible_chunk_counts: Vec<u32>,
    /// Per-frame: number of chunks generated.
    pub gen_chunk_counts: Vec<u32>,
}

impl PerfStats {
    pub fn new() -> Self {
        Self {
            visibility_total_us: Vec::new(),
            culling_us: Vec::new(),
            gen_single_chunk_us: Vec::new(),
            dirty_update_us: Vec::new(),
            array_mesh_build_us: Vec::new(),
            eviction_us: Vec::new(),
            visible_chunk_counts: Vec::new(),
            gen_chunk_counts: Vec::new(),
        }
    }

    /// Append a microsecond sample to a metric's sample list.
    fn record_us(samples: &mut Vec<u32>, us: u32) {
        samples.push(us);
    }

    /// Print a summary table with p50/p90/p99/max percentiles for each metric.
    pub fn print_summary(&self) {
        eprintln!("=== Mesh Perf Stats ===");
        Self::print_metric("visibility_total", &self.visibility_total_us);
        Self::print_metric("  culling", &self.culling_us);
        Self::print_metric("  eviction", &self.eviction_us);
        Self::print_metric("gen_single_chunk", &self.gen_single_chunk_us);
        Self::print_metric("dirty_update", &self.dirty_update_us);
        Self::print_metric("array_mesh_build", &self.array_mesh_build_us);
        Self::print_count_metric("visible_chunks", &self.visible_chunk_counts);
        Self::print_count_metric("gen_chunks/frame", &self.gen_chunk_counts);
    }

    fn print_metric(name: &str, samples: &[u32]) {
        if samples.is_empty() {
            eprintln!("  {name}: (no samples)");
            return;
        }
        let mut sorted: Vec<u32> = samples.to_vec();
        sorted.sort_unstable();
        let n = sorted.len();
        let p50 = sorted[n / 2];
        let p90 = sorted[n * 90 / 100];
        let p99 = sorted[n * 99 / 100];
        let max = sorted[n - 1];
        let mean = sorted.iter().map(|&x| x as u64).sum::<u64>() / n as u64;
        eprintln!(
            "  {name}: n={n}  mean={mean}us  p50={p50}us  p90={p90}us  p99={p99}us  max={max}us"
        );
    }

    fn print_count_metric(name: &str, samples: &[u32]) {
        if samples.is_empty() {
            eprintln!("  {name}: (no samples)");
            return;
        }
        let mut sorted: Vec<u32> = samples.to_vec();
        sorted.sort_unstable();
        let n = sorted.len();
        let p50 = sorted[n / 2];
        let p90 = sorted[n * 90 / 100];
        let p99 = sorted[n * 99 / 100];
        let max = sorted[n - 1];
        let mean = sorted.iter().map(|&x| x as u64).sum::<u64>() / n as u64;
        eprintln!("  {name}: n={n}  mean={mean}  p50={p50}  p90={p90}  p99={p99}  max={max}");
    }
}

// ---------------------------------------------------------------------------
// MeshCache
// ---------------------------------------------------------------------------

/// Result from a background mesh generation worker.
struct MeshWorkResult {
    coord: ChunkCoord,
    sim_tick: u64,
    /// `None` when the worker bailed early due to cancellation.
    mesh: Option<ChunkMesh>,
    gen_us: u32,
}

/// Caches chunk meshes with MegaChunk spatial hierarchy, draw-distance
/// culling, frustum culling, lazy mesh generation, and LRU eviction.
///
/// Mesh generation is fully asynchronous: chunks are submitted to rayon
/// background workers via `rayon::spawn()`, and completed meshes are
/// drained from an mpsc channel each frame. The main thread only does
/// fast `ChunkNeighborhood` extraction (copying ~8K voxels) and culling.
///
/// Also supports an optional Y cutoff for the height-hiding feature.
pub struct MeshCache {
    /// Cached mesh data per chunk (keyed by ChunkCoord).
    chunks: BTreeMap<ChunkCoord, ChunkMesh>,
    /// Chunks that need regeneration (dirty tracking from voxel edits).
    dirty: BTreeSet<ChunkCoord>,
    /// Optional Y cutoff for height hiding.
    y_cutoff: Option<i32>,
    /// Global tiling texture cache. Built once, independent of world state.
    tiling_cache: TilingCache,
    /// World chunk bounds.
    cx_max: i32,
    cy_max: i32,
    cz_max: i32,

    // -- MegaChunk hierarchy --
    /// MegaChunk spatial index. Populated by `scan_nonempty_chunks()`.
    megachunks: BTreeMap<MegaChunkCoord, MegaChunk>,

    // -- Visibility state --
    /// Chunks currently considered visible (within draw distance + frustum).
    visible_set: BTreeSet<ChunkCoord>,
    /// Chunks that should become visible this frame (were not visible before).
    chunks_to_show: Vec<ChunkCoord>,
    /// Chunks that should become hidden this frame (were visible before).
    chunks_to_hide: Vec<ChunkCoord>,
    /// Subset of `chunks_to_show` that are freshly generated (need new
    /// MeshInstance3D creation, not just `.visible = true`).
    chunks_generated: Vec<ChunkCoord>,
    /// Chunks evicted from the LRU cache this frame (need MeshInstance3D freed).
    chunks_evicted: Vec<ChunkCoord>,
    /// Chunks currently in shadow-only state (outside frustum, inside shadow volume).
    shadow_set: BTreeSet<ChunkCoord>,
    /// Chunks entering shadow-only state this frame.
    chunks_to_shadow: Vec<ChunkCoord>,
    /// Chunks leaving shadow-only state to hidden this frame.
    chunks_from_shadow: Vec<ChunkCoord>,

    /// Chunks known to produce empty meshes (interior volumes where all faces
    /// are culled). Skipped during visibility to avoid regenerating every frame.
    /// Cleared for a chunk when it's marked dirty (voxel edit may expose faces).
    empty_chunks: BTreeSet<ChunkCoord>,

    // -- LRU tracking --
    /// Last-accessed frame stamp per cached chunk.
    lru_stamps: BTreeMap<ChunkCoord, u64>,
    /// Estimated byte size per cached chunk.
    chunk_bytes: BTreeMap<ChunkCoord, usize>,
    /// Total bytes across all cached chunk meshes.
    total_cached_bytes: usize,
    /// Monotonic frame counter, incremented each `update_visibility()`.
    frame_counter: u64,

    // -- Configuration --
    /// Draw distance in voxels (XZ). Chunks beyond this are hidden.
    /// 0.0 means unlimited (everything visible).
    draw_distance_voxels: f32,
    /// Memory budget in bytes. 0 means unlimited (no eviction).
    memory_budget: usize,
    /// Light direction for shadow-only culling. Unit vector pointing from
    /// light source toward scene. `None` disables shadow-only (all culled
    /// chunks are hidden).
    light_direction: Option<[f32; 3]>,

    /// Accumulated performance timing samples.
    pub perf: PerfStats,

    // -- Async mesh generation --
    /// Sender for submitting completed meshes from background workers.
    /// Cloned into each `rayon::spawn` closure.
    mesh_tx: mpsc::Sender<MeshWorkResult>,
    /// Receiver for draining completed meshes on the main thread.
    mesh_rx: mpsc::Receiver<MeshWorkResult>,
    /// Chunks currently being generated in the background. Prevents
    /// duplicate submissions.
    in_flight: BTreeSet<ChunkCoord>,
    /// Sim tick at which each cached chunk was generated. Used for
    /// freshness checks when two results for the same chunk race.
    cached_ticks: BTreeMap<ChunkCoord, u64>,
    /// Dedicated rayon thread pool for mesh generation. Uses fewer threads
    /// than the total CPU count to leave headroom for the main/render thread
    /// and OS/background tasks.
    mesh_pool: ThreadPool,
    /// Cancellation flag for background workers. Set to `true` during
    /// shutdown so pending/in-progress workers bail early instead of
    /// running the full mesh pipeline.
    cancel: Arc<AtomicBool>,
    /// Mesh pipeline configuration (smoothing, decimation, etc.). Cloned into
    /// each background worker closure so workers don't depend on global state.
    pub mesh_config: MeshPipelineConfig,
}

impl MeshCache {
    pub fn new() -> Self {
        let (mesh_tx, mesh_rx) = mpsc::channel();
        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let mesh_threads = num_cpus.saturating_sub(2).max(1);
        let mesh_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(mesh_threads)
            .thread_name(|i| format!("mesh-worker-{i}"))
            .build()
            .expect("failed to create mesh worker pool");
        Self {
            chunks: BTreeMap::new(),
            dirty: BTreeSet::new(),
            y_cutoff: None,
            tiling_cache: TilingCache::new(),
            cx_max: 0,
            cy_max: 0,
            cz_max: 0,
            megachunks: BTreeMap::new(),
            visible_set: BTreeSet::new(),
            shadow_set: BTreeSet::new(),
            chunks_to_show: Vec::new(),
            chunks_to_hide: Vec::new(),
            chunks_to_shadow: Vec::new(),
            chunks_from_shadow: Vec::new(),
            chunks_generated: Vec::new(),
            chunks_evicted: Vec::new(),
            empty_chunks: BTreeSet::new(),
            lru_stamps: BTreeMap::new(),
            chunk_bytes: BTreeMap::new(),
            total_cached_bytes: 0,
            frame_counter: 0,
            draw_distance_voxels: 50.0,
            memory_budget: 0,
            light_direction: None,
            perf: PerfStats::new(),
            mesh_tx,
            mesh_rx,
            in_flight: BTreeSet::new(),
            cached_ticks: BTreeMap::new(),
            mesh_pool,
            cancel: Arc::new(AtomicBool::new(false)),
            mesh_config: MeshPipelineConfig::default(),
        }
    }

    // -- Configuration setters --

    pub fn set_draw_distance(&mut self, voxels: f32) {
        self.draw_distance_voxels = voxels;
    }

    pub fn set_memory_budget(&mut self, bytes: usize) {
        self.memory_budget = bytes;
    }

    /// Block until all in-flight background mesh generations complete,
    /// then drain their results into the cache. Test-only convenience
    /// method to make async generation synchronous in tests.
    #[cfg(test)]
    pub fn flush_in_flight(&mut self) {
        while !self.in_flight.is_empty() {
            // Block for one result, then drain any others that arrived.
            if let Ok(result) = self.mesh_rx.recv() {
                self.handle_completed_result(result);
            }
            // Non-blocking drain of any additional results.
            while let Ok(result) = self.mesh_rx.try_recv() {
                self.handle_completed_result(result);
            }
        }
    }

    /// Block until all in-flight background mesh generations complete,
    /// then re-queue results on the channel so that the next
    /// `drain_completed()` (called at the start of `update_visibility()`)
    /// picks them up and populates `chunks_generated` at the right time.
    /// Use this instead of `flush_in_flight()` when the test needs to
    /// verify delta lists (`chunks_to_show`, `chunks_to_shadow`, etc.)
    /// that depend on `chunks_generated` being populated during the
    /// `update_visibility()` diff phase.
    #[cfg(test)]
    pub fn await_in_flight(&mut self) {
        let mut buffered = Vec::new();
        while !self.in_flight.is_empty() {
            if let Ok(result) = self.mesh_rx.recv() {
                self.in_flight.remove(&result.coord);
                buffered.push(result);
            }
            while let Ok(result) = self.mesh_rx.try_recv() {
                self.in_flight.remove(&result.coord);
                buffered.push(result);
            }
        }
        // Re-send results so drain_completed can pick them up.
        for result in buffered {
            let _ = self.mesh_tx.send(result);
        }
    }

    /// Graceful shutdown: signal all background workers to bail early, then
    /// block until every in-flight task has reported back. After this returns,
    /// the thread pool can be safely dropped without racing Godot teardown.
    pub fn shutdown(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Block until every in-flight worker has sent its result (real or
        // cancelled). Workers always send exactly one MeshWorkResult, so we
        // just need to drain `in_flight.len()` results.
        while !self.in_flight.is_empty() {
            if let Ok(result) = self.mesh_rx.recv() {
                self.in_flight.remove(&result.coord);
            }
        }
    }

    pub fn set_light_direction(&mut self, dir: Option<[f32; 3]>) {
        self.light_direction = dir;
    }

    pub fn total_cached_bytes(&self) -> usize {
        self.total_cached_bytes
    }

    // -- World scan (replaces build_all for initial setup) --

    /// Scan the world for chunks that contain renderable geometry and populate
    /// the MegaChunk spatial index. **Does not generate any meshes** — meshes
    /// are created lazily when chunks enter visibility.
    ///
    /// For each chunk-sized column footprint, walks the RLE spans via
    /// `column_spans()` and checks whether any geometry-producing voxel type
    /// overlaps the chunk's Y range. This is much cheaper than generating full
    /// meshes for the entire world.
    pub fn scan_nonempty_chunks(&mut self, world: &VoxelWorld) {
        self.chunks.clear();
        self.dirty.clear();

        self.megachunks.clear();
        self.visible_set.clear();
        self.shadow_set.clear();
        self.empty_chunks.clear();
        self.lru_stamps.clear();
        self.chunk_bytes.clear();
        self.total_cached_bytes = 0;
        self.in_flight.clear();
        self.cached_ticks.clear();
        // Drain and discard any in-flight results from a previous scan.
        while self.mesh_rx.try_recv().is_ok() {}

        let cx_max = (world.size_x as i32 + CHUNK_SIZE - 1) / CHUNK_SIZE;
        let cy_max = (world.size_y as i32 + CHUNK_SIZE - 1) / CHUNK_SIZE;
        let cz_max = (world.size_z as i32 + CHUNK_SIZE - 1) / CHUNK_SIZE;
        self.cx_max = cx_max;
        self.cy_max = cy_max;
        self.cz_max = cz_max;

        // For each XZ column of chunks, scan the voxel column spans to find
        // which chunk Y-levels contain geometry.
        for cz in 0..cz_max {
            for cx in 0..cx_max {
                // Collect which chunk Y-levels have geometry in this column.
                let mut nonempty_cys: BTreeSet<i32> = BTreeSet::new();

                let base_x = cx * CHUNK_SIZE;
                let base_z = cz * CHUNK_SIZE;

                for lz in 0..CHUNK_SIZE {
                    let wz = base_z + lz;
                    if wz < 0 || (wz as u32) >= world.size_z {
                        continue;
                    }
                    for lx in 0..CHUNK_SIZE {
                        let wx = base_x + lx;
                        if wx < 0 || (wx as u32) >= world.size_x {
                            continue;
                        }

                        for (vt, y_start, y_end) in world.column_spans(wx as u32, wz as u32) {
                            if !produces_geometry(vt) {
                                continue;
                            }
                            // Which chunk Y-levels does this span touch?
                            let cy_lo = (y_start as i32).div_euclid(CHUNK_SIZE);
                            let cy_hi = (y_end as i32).div_euclid(CHUNK_SIZE);
                            for cy in cy_lo..=cy_hi {
                                if cy >= 0 && cy < cy_max {
                                    nonempty_cys.insert(cy);
                                }
                            }
                        }
                    }
                }

                // Register non-empty chunks with their MegaChunk.
                for cy in nonempty_cys {
                    let chunk_coord = ChunkCoord::new(cx, cy, cz);
                    let mega_coord = chunk_to_mega(chunk_coord);
                    self.megachunks
                        .entry(mega_coord)
                        .or_insert_with(MegaChunk::new)
                        .add_chunk(chunk_coord);
                }
            }
        }
    }

    /// Legacy full build for backward compatibility. Generates all chunk meshes
    /// eagerly. Use `scan_nonempty_chunks()` + `update_visibility()` for the
    /// lazy pipeline instead.
    #[cfg(test)]
    pub fn build_all(
        &mut self,
        world: &VoxelWorld,
        grassless: &std::collections::BTreeSet<VoxelCoord>,
    ) {
        self.scan_nonempty_chunks(world);

        // Generate all meshes eagerly (no visibility filtering).
        let all_chunks: Vec<ChunkCoord> = self
            .megachunks
            .values()
            .flat_map(|mc| mc.chunks.iter().copied())
            .collect();
        for coord in all_chunks {
            let mesh = elven_canopy_sim::mesh_gen::generate_chunk_mesh_from_world(
                world,
                coord,
                self.y_cutoff,
                grassless,
                &self.mesh_config,
            );
            if !mesh.is_empty() {
                let bytes = mesh.estimate_byte_size();
                self.total_cached_bytes += bytes;
                self.chunk_bytes.insert(coord, bytes);
                self.lru_stamps.insert(coord, self.frame_counter);
                self.chunks.insert(coord, mesh);
                self.visible_set.insert(coord);
            }
        }
    }

    // -- Async mesh generation --

    /// Submit a chunk for background mesh generation.
    /// Extracts a `ChunkNeighborhood` from the world (fast) and spawns
    /// a rayon task to generate the mesh.
    fn submit_chunk(
        &mut self,
        world: &VoxelWorld,
        coord: ChunkCoord,
        grassless: &std::collections::BTreeSet<VoxelCoord>,
    ) {
        if self.in_flight.contains(&coord) {
            return;
        }
        let neighborhood = ChunkNeighborhood::extract(world, coord, self.y_cutoff, grassless);
        let tx = self.mesh_tx.clone();
        let config = self.mesh_config;
        let cancel = Arc::clone(&self.cancel);
        self.in_flight.insert(coord);
        self.dirty.remove(&coord);
        self.mesh_pool.spawn(move || {
            // Bail early if shutdown has been requested. The main thread is
            // waiting for all in_flight tasks to report back, so we still
            // send a result (with mesh: None) even when cancelled.
            if cancel.load(Ordering::Relaxed) {
                let _ = tx.send(MeshWorkResult {
                    coord: neighborhood.chunk,
                    sim_tick: neighborhood.sim_tick,
                    mesh: None,
                    gen_us: 0,
                });
                return;
            }
            let t = Instant::now();
            let mesh = generate_chunk_mesh(&neighborhood, &config);
            let gen_us = t.elapsed().as_micros() as u32;
            // If the receiver is dropped (MeshCache destroyed), the send
            // silently fails — that's fine.
            let _ = tx.send(MeshWorkResult {
                coord: neighborhood.chunk,
                sim_tick: neighborhood.sim_tick,
                mesh: Some(mesh),
                gen_us,
            });
        });
    }

    /// Drain completed mesh results from background workers and insert
    /// them into the cache. Returns the number of meshes inserted.
    ///
    /// For each result:
    /// - Removes the chunk from `in_flight`.
    /// - Discards stale results (sim_tick older than the cached version).
    /// - Inserts the mesh into the cache and adds to `chunks_generated`.
    pub fn drain_completed(&mut self) -> usize {
        let mut count = 0;
        while let Ok(result) = self.mesh_rx.try_recv() {
            if self.handle_completed_result(result) {
                count += 1;
            }
        }
        count
    }

    /// Process a single completed mesh result. Returns true if a non-empty
    /// mesh was inserted into the cache.
    fn handle_completed_result(&mut self, result: MeshWorkResult) -> bool {
        self.in_flight.remove(&result.coord);

        // Cancelled workers send mesh: None — just clear in_flight.
        let Some(mesh) = result.mesh else {
            return false;
        };

        self.perf.gen_single_chunk_us.push(result.gen_us);

        // Freshness check: discard if a newer version is already cached.
        if self
            .cached_ticks
            .get(&result.coord)
            .is_some_and(|&tick| result.sim_tick < tick)
        {
            return false;
        }

        let coord = result.coord;
        if mesh.is_empty() {
            // Chunk produces no geometry — remember it so we don't
            // regenerate every frame.
            self.chunks.remove(&coord);
            self.lru_stamps.remove(&coord);
            if let Some(bytes) = self.chunk_bytes.remove(&coord) {
                self.total_cached_bytes = self.total_cached_bytes.saturating_sub(bytes);
            }
            self.cached_ticks.remove(&coord);
            self.empty_chunks.insert(coord);
            // Notify GDScript to free the MeshInstance3D if the chunk was
            // previously visible or shadow-only.
            if self.visible_set.remove(&coord) {
                self.chunks_to_hide.push(coord);
            }
            if self.shadow_set.remove(&coord) {
                self.chunks_from_shadow.push(coord);
            }
            let mega_coord = chunk_to_mega(coord);
            if let Some(mc) = self.megachunks.get_mut(&mega_coord) {
                mc.remove_chunk(&coord);
            }
            false
        } else {
            // Remove old byte count before inserting the new mesh.
            if let Some(old_bytes) = self.chunk_bytes.remove(&coord) {
                self.total_cached_bytes = self.total_cached_bytes.saturating_sub(old_bytes);
            }
            let bytes = mesh.estimate_byte_size();
            self.total_cached_bytes += bytes;
            self.chunk_bytes.insert(coord, bytes);
            self.lru_stamps.insert(coord, self.frame_counter);
            self.chunks.insert(coord, mesh);
            self.cached_ticks.insert(coord, result.sim_tick);
            self.chunks_generated.push(coord);
            true
        }
    }

    // -- Visibility --

    /// Update the visible set based on camera position and frustum planes.
    ///
    /// `cam_pos` is `[x, y, z]` in voxel space. `frustum_planes` is a slice
    /// of 6 planes, each `[nx, ny, nz, d]` in Godot convention.
    ///
    /// Returns the number of chunk meshes generated this frame.
    pub fn update_visibility(
        &mut self,
        world: &VoxelWorld,
        cam_pos: [f32; 3],
        frustum_planes: &[[f32; 4]],
        grassless: &std::collections::BTreeSet<VoxelCoord>,
    ) -> usize {
        let t_total = Instant::now();

        self.frame_counter += 1;
        self.chunks_to_show.clear();
        self.chunks_to_hide.clear();
        self.chunks_to_shadow.clear();
        self.chunks_from_shadow.clear();
        self.chunks_generated.clear();
        self.chunks_evicted.clear();

        // Drain completed meshes from background workers before culling,
        // so freshly completed chunks are available for the delta lists.
        let gen_count = self.drain_completed();

        let draw_dist_sq = if self.draw_distance_voxels > 0.0 {
            self.draw_distance_voxels * self.draw_distance_voxels
        } else {
            f32::MAX
        };

        let mut new_visible: BTreeSet<ChunkCoord> = BTreeSet::new();
        let mut new_shadow: BTreeSet<ChunkCoord> = BTreeSet::new();

        // Build shadow volume planes (once per frame).
        let shadow_planes: Vec<[f32; 4]> = match self.light_direction {
            Some(dir) if frustum_planes.len() >= 6 => {
                let extend = if self.draw_distance_voxels > 0.0 {
                    self.draw_distance_voxels
                } else {
                    // Unlimited draw distance: extend by the world diagonal.
                    let wx = self.cx_max as f32 * CHUNK_SIZE as f32;
                    let wy = self.cy_max as f32 * CHUNK_SIZE as f32;
                    let wz = self.cz_max as f32 * CHUNK_SIZE as f32;
                    (wx * wx + wy * wy + wz * wz).sqrt()
                };
                build_shadow_planes(frustum_planes, dir, extend)
            }
            _ => Vec::new(),
        };
        let have_shadow = !shadow_planes.is_empty();

        let mut pending_this_frame: BTreeSet<ChunkCoord> = BTreeSet::new();

        // -- Culling pass (timed) --
        let t_cull = Instant::now();

        // Coarse pass: MegaChunk draw-distance + frustum + shadow volume.
        for mega in self.megachunks.values() {
            let aabb = match mega.aabb {
                Some(a) => a,
                None => continue,
            };

            // Draw distance (XZ only). This applies to both visible and shadow.
            if aabb.horizontal_distance_sq(cam_pos[0], cam_pos[2]) > draw_dist_sq {
                continue;
            }

            // Coarse frustum and shadow volume tests.
            let mega_in_frustum =
                frustum_planes.len() < 6 || !aabb.is_outside_frustum(frustum_planes);
            let mega_in_shadow = have_shadow && !aabb.is_outside_frustum(&shadow_planes);

            if !mega_in_frustum && !mega_in_shadow {
                continue;
            }

            // Fine pass: per-chunk tests.
            for &chunk_coord in &mega.chunks {
                if self.empty_chunks.contains(&chunk_coord) {
                    continue;
                }

                let chunk_aabb = Aabb3::from_chunk(chunk_coord);

                // Per-chunk draw distance.
                if chunk_aabb.horizontal_distance_sq(cam_pos[0], cam_pos[2]) > draw_dist_sq {
                    continue;
                }

                let in_frustum =
                    frustum_planes.len() < 6 || !chunk_aabb.is_outside_frustum(frustum_planes);

                if in_frustum {
                    new_visible.insert(chunk_coord);
                } else if have_shadow && !chunk_aabb.is_outside_frustum(&shadow_planes) {
                    new_shadow.insert(chunk_coord);
                } else {
                    continue;
                }

                // Ensure mesh exists (both visible and shadow-only need meshes).
                if self.chunks.contains_key(&chunk_coord) {
                    self.lru_stamps.insert(chunk_coord, self.frame_counter);
                } else if !self.in_flight.contains(&chunk_coord) {
                    pending_this_frame.insert(chunk_coord);
                }
            }
        }

        let cull_us = t_cull.elapsed().as_micros() as u32;
        PerfStats::record_us(&mut self.perf.culling_us, cull_us);
        PerfStats::record_us(
            &mut self.perf.visible_chunk_counts,
            new_visible.len() as u32,
        );

        // Submit pending chunks for background mesh generation.
        // Nearest-to-camera chunks submitted first. External spawns go into
        // rayon's global injector queue (FIFO), so nearest chunks are picked
        // up by workers first.
        let mut pending_sorted: Vec<ChunkCoord> = pending_this_frame.iter().copied().collect();
        pending_sorted.sort_by(|a, b| {
            let da = chunk_distance_sq(*a, cam_pos);
            let db = chunk_distance_sq(*b, cam_pos);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut submit_count = 0usize;
        for coord in pending_sorted {
            if !new_visible.contains(&coord) && !new_shadow.contains(&coord) {
                continue;
            }
            self.submit_chunk(world, coord, grassless);
            submit_count += 1;
        }
        PerfStats::record_us(&mut self.perf.gen_chunk_counts, submit_count as u32);

        // Diff: compute show/hide/shadow delta lists.
        //
        // State transitions:
        //   hidden  → visible:     chunks_to_show (cast_shadow ON, visible true)
        //   hidden  → shadow-only: chunks_to_shadow (cast_shadow SHADOWS_ONLY, visible true)
        //   shadow  → visible:     chunks_to_show (cast_shadow ON)
        //   shadow  → hidden:      chunks_from_shadow (visible false)
        //   visible → shadow-only: chunks_to_shadow (cast_shadow SHADOWS_ONLY)
        //   visible → hidden:      chunks_to_hide (visible false)
        //
        // Note: chunks_to_show doubles as the "restore full rendering" signal,
        // covering both hidden→visible and shadow→visible transitions.

        for &coord in &new_visible {
            if !self.visible_set.contains(&coord) && self.chunks.contains_key(&coord) {
                // hidden→visible or shadow→visible
                self.chunks_to_show.push(coord);
            }
        }
        // Ensure all generated chunks are in the show or shadow list.
        for &coord in &self.chunks_generated {
            if new_visible.contains(&coord) {
                if !self.chunks_to_show.contains(&coord) {
                    self.chunks_to_show.push(coord);
                }
            } else if new_shadow.contains(&coord) && !self.chunks_to_shadow.contains(&coord) {
                self.chunks_to_shadow.push(coord);
            }
        }
        for &coord in &new_shadow {
            // Skip if already added by the chunks_generated loop above.
            if self.chunks_to_shadow.contains(&coord) {
                continue;
            }
            if !self.shadow_set.contains(&coord)
                && !self.visible_set.contains(&coord)
                && self.chunks.contains_key(&coord)
            {
                // hidden→shadow
                self.chunks_to_shadow.push(coord);
            } else if self.visible_set.contains(&coord) {
                // visible→shadow
                self.chunks_to_shadow.push(coord);
            }
        }
        for &coord in &self.visible_set {
            if !new_visible.contains(&coord) && !new_shadow.contains(&coord) {
                // visible→hidden
                self.chunks_to_hide.push(coord);
            }
        }
        for &coord in &self.shadow_set {
            if !new_visible.contains(&coord) && !new_shadow.contains(&coord) {
                // shadow→hidden
                self.chunks_from_shadow.push(coord);
            }
            // shadow→visible is already covered by chunks_to_show above
        }

        self.visible_set = new_visible;
        self.shadow_set = new_shadow;

        // LRU eviction. Chunks that just left visibility may be evicted.
        if self.memory_budget > 0 {
            let t_evict = Instant::now();
            self.evict_lru();
            let evict_us = t_evict.elapsed().as_micros() as u32;
            if evict_us > 0 || !self.chunks_evicted.is_empty() {
                PerfStats::record_us(&mut self.perf.eviction_us, evict_us);
            }
            // Evicted chunks don't need a hide/shadow toggle — they're being freed.
            // Remove them from delta lists to avoid double-processing.
            if !self.chunks_evicted.is_empty() {
                let evicted_set: BTreeSet<ChunkCoord> =
                    self.chunks_evicted.iter().copied().collect();
                self.chunks_to_hide.retain(|c| !evicted_set.contains(c));
                self.chunks_from_shadow.retain(|c| !evicted_set.contains(c));
            }
        }

        let total_us = t_total.elapsed().as_micros() as u32;
        PerfStats::record_us(&mut self.perf.visibility_total_us, total_us);

        gen_count
    }

    /// Evict least-recently-accessed chunks until under memory budget.
    fn evict_lru(&mut self) {
        while self.total_cached_bytes > self.memory_budget {
            // Find the chunk with the oldest stamp that is NOT visible or shadow-only.
            let victim = self
                .lru_stamps
                .iter()
                .filter(|(coord, _)| {
                    !self.visible_set.contains(coord) && !self.shadow_set.contains(coord)
                })
                .min_by_key(|&(_, &stamp)| stamp)
                .map(|(&coord, _)| coord);

            match victim {
                Some(coord) => {
                    self.lru_stamps.remove(&coord);
                    self.chunks.remove(&coord);
                    self.cached_ticks.remove(&coord);
                    if let Some(bytes) = self.chunk_bytes.remove(&coord) {
                        self.total_cached_bytes = self.total_cached_bytes.saturating_sub(bytes);
                    }
                    self.chunks_evicted.push(coord);
                }
                None => break, // All remaining chunks are visible; can't evict.
            }
        }
    }

    // -- Delta accessors (for the bridge) --

    /// Chunks that should become visible (set `.visible = true`).
    /// Includes both previously-cached chunks re-entering view and freshly
    /// generated ones.
    pub fn chunks_to_show(&self) -> &[ChunkCoord] {
        &self.chunks_to_show
    }

    /// Chunks that should become hidden (set `.visible = false`).
    pub fn chunks_to_hide(&self) -> &[ChunkCoord] {
        &self.chunks_to_hide
    }

    /// Freshly generated chunks (subset of `chunks_to_show`). These need new
    /// MeshInstance3D nodes, not just a visibility toggle.
    pub fn chunks_generated(&self) -> &[ChunkCoord] {
        &self.chunks_generated
    }

    /// Chunks entering shadow-only state (set `SHADOWS_ONLY` + visible).
    pub fn chunks_to_shadow(&self) -> &[ChunkCoord] {
        &self.chunks_to_shadow
    }

    /// Chunks leaving shadow-only state to fully hidden (set `.visible = false`).
    pub fn chunks_from_shadow(&self) -> &[ChunkCoord] {
        &self.chunks_from_shadow
    }

    /// Chunks evicted from the LRU cache. Their MeshInstance3D nodes should
    /// be freed.
    pub fn chunks_evicted(&self) -> &[ChunkCoord] {
        &self.chunks_evicted
    }

    // -- Dirty tracking (unchanged API, but now visibility-aware) --

    /// Mark chunks as dirty based on a list of modified voxel coordinates.
    /// Also marks neighbor chunks when a voxel sits at a chunk boundary.
    pub fn mark_dirty_voxels(&mut self, coords: &[VoxelCoord]) {
        for &coord in coords {
            let chunk = voxel_to_chunk(coord);
            self.dirty.insert(chunk);
            self.empty_chunks.remove(&chunk);

            let local_x = coord.x.rem_euclid(CHUNK_SIZE);
            let local_y = coord.y.rem_euclid(CHUNK_SIZE);
            let local_z = coord.z.rem_euclid(CHUNK_SIZE);

            if local_x == 0 {
                let neighbor = ChunkCoord::new(chunk.cx - 1, chunk.cy, chunk.cz);
                self.dirty.insert(neighbor);
                self.empty_chunks.remove(&neighbor);
            }
            if local_x == CHUNK_SIZE - 1 {
                let neighbor = ChunkCoord::new(chunk.cx + 1, chunk.cy, chunk.cz);
                self.dirty.insert(neighbor);
                self.empty_chunks.remove(&neighbor);
            }
            if local_y == 0 {
                let neighbor = ChunkCoord::new(chunk.cx, chunk.cy - 1, chunk.cz);
                self.dirty.insert(neighbor);
                self.empty_chunks.remove(&neighbor);
            }
            if local_y == CHUNK_SIZE - 1 {
                let neighbor = ChunkCoord::new(chunk.cx, chunk.cy + 1, chunk.cz);
                self.dirty.insert(neighbor);
                self.empty_chunks.remove(&neighbor);
            }
            if local_z == 0 {
                let neighbor = ChunkCoord::new(chunk.cx, chunk.cy, chunk.cz - 1);
                self.dirty.insert(neighbor);
                self.empty_chunks.remove(&neighbor);
            }
            if local_z == CHUNK_SIZE - 1 {
                let neighbor = ChunkCoord::new(chunk.cx, chunk.cy, chunk.cz + 1);
                self.dirty.insert(neighbor);
                self.empty_chunks.remove(&neighbor);
            }
        }

        // Also update megachunk registrations for the directly modified chunks
        // (a voxel edit might make a previously-empty chunk non-empty or vice
        // versa). Full accuracy requires mesh gen, so we conservatively add the
        // chunk to its megachunk now and let mesh gen sort it out.
        for &coord in coords {
            let chunk = voxel_to_chunk(coord);
            let mega_coord = chunk_to_mega(chunk);
            self.megachunks
                .entry(mega_coord)
                .or_insert_with(MegaChunk::new)
                .add_chunk(chunk);
        }
    }

    /// Submit dirty visible/shadow chunks for background regeneration.
    /// Returns the number of chunks submitted. Non-visible dirty chunks
    /// remain dirty and will be submitted when they enter visibility.
    ///
    /// Dirty flags are cleared at submission time (not at completion),
    /// so new dirty marks arriving while a chunk is in-flight are preserved
    /// and will trigger another regeneration.
    pub fn update_dirty(
        &mut self,
        world: &VoxelWorld,
        grassless: &std::collections::BTreeSet<VoxelCoord>,
    ) -> usize {
        // Only process dirty chunks that are currently visible or shadow-only
        // and not already in-flight.
        let visible_dirty: Vec<ChunkCoord> = self
            .dirty
            .iter()
            .copied()
            .filter(|c| {
                !self.in_flight.contains(c)
                    && (self.visible_set.contains(c) || self.shadow_set.contains(c))
            })
            .collect();
        if visible_dirty.is_empty() {
            return 0;
        }

        let t_dirty = Instant::now();

        // Submit each dirty chunk for background generation.
        // submit_chunk() clears the dirty flag for each chunk at submission
        // time, so new dirty marks arriving while in-flight are preserved.
        for &coord in &visible_dirty {
            self.submit_chunk(world, coord, grassless);
        }

        let dirty_us = t_dirty.elapsed().as_micros() as u32;
        PerfStats::record_us(&mut self.perf.dirty_update_us, dirty_us);

        visible_dirty.len()
    }

    // -- Accessors (existing API, preserved) --

    /// Get all non-empty chunk coordinates (all cached chunks).
    pub fn chunk_coords(&self) -> Vec<ChunkCoord> {
        self.chunks.keys().copied().collect()
    }

    /// Number of megachunks in the spatial index.
    pub fn megachunk_count(&self) -> usize {
        self.megachunks.len()
    }

    /// Get the cached mesh for a chunk, if it exists.
    pub fn get_chunk(&self, coord: &ChunkCoord) -> Option<&ChunkMesh> {
        self.chunks.get(coord)
    }

    /// Access the global tiling texture cache.
    pub fn tiling_cache(&self) -> &TilingCache {
        &self.tiling_cache
    }

    /// Set the Y cutoff for height hiding. Dirties all chunks at or between
    /// the old and new cutoff levels so their meshes are regenerated with the
    /// correct boundary faces.
    ///
    /// Pass `None` to disable the cutoff (show all voxels).
    pub fn set_y_cutoff(&mut self, new_cutoff: Option<i32>) {
        if self.y_cutoff == new_cutoff {
            return;
        }

        let (cy_min, cy_max) = match (self.y_cutoff, new_cutoff) {
            (Some(old), Some(new)) => {
                let lo = (old.min(new) - 1).div_euclid(CHUNK_SIZE);
                let hi = old.max(new).div_euclid(CHUNK_SIZE);
                (lo, hi)
            }
            (Some(old), None) | (None, Some(old)) => {
                let lo = (old - 1).div_euclid(CHUNK_SIZE);
                (lo, self.cy_max - 1)
            }
            (None, None) => {
                self.y_cutoff = new_cutoff;
                return;
            }
        };
        let effective_max = if self.y_cutoff.is_none() || new_cutoff.is_none() {
            self.cy_max - 1
        } else {
            cy_max
        };

        for cy in cy_min..=effective_max {
            for cz in 0..self.cz_max {
                for cx in 0..self.cx_max {
                    let chunk = ChunkCoord::new(cx, cy, cz);
                    self.dirty.insert(chunk);
                    self.empty_chunks.remove(&chunk);
                    let mega_coord = chunk_to_mega(chunk);
                    self.megachunks
                        .entry(mega_coord)
                        .or_insert_with(MegaChunk::new)
                        .add_chunk(chunk);
                }
            }
        }

        self.y_cutoff = new_cutoff;
    }

    /// Get the current Y cutoff.
    pub fn y_cutoff(&self) -> Option<i32> {
        self.y_cutoff
    }
}

/// Squared distance from a chunk's center to a point (full 3D).
fn chunk_distance_sq(c: ChunkCoord, pos: [f32; 3]) -> f32 {
    let center = Aabb3::from_chunk(c).center();
    let dx = center[0] - pos[0];
    let dy = center[1] - pos[1];
    let dz = center[2] - pos[2];
    dx * dx + dy * dy + dz * dz
}

#[cfg(test)]
mod tests {
    use super::*;
    use elven_canopy_sim::types::VoxelType;
    use elven_canopy_sim::world::VoxelWorld;

    // -- MegaChunkCoord conversion --

    #[test]
    fn chunk_to_mega_positive() {
        assert_eq!(
            chunk_to_mega(ChunkCoord::new(0, 0, 0)),
            MegaChunkCoord::new(0, 0)
        );
        assert_eq!(
            chunk_to_mega(ChunkCoord::new(15, 5, 15)),
            MegaChunkCoord::new(0, 0)
        );
        assert_eq!(
            chunk_to_mega(ChunkCoord::new(16, 0, 0)),
            MegaChunkCoord::new(1, 0)
        );
        assert_eq!(
            chunk_to_mega(ChunkCoord::new(31, 0, 31)),
            MegaChunkCoord::new(1, 1)
        );
    }

    #[test]
    fn chunk_to_mega_negative() {
        assert_eq!(
            chunk_to_mega(ChunkCoord::new(-1, 0, 0)),
            MegaChunkCoord::new(-1, 0)
        );
        assert_eq!(
            chunk_to_mega(ChunkCoord::new(-16, 0, -16)),
            MegaChunkCoord::new(-1, -1)
        );
        assert_eq!(
            chunk_to_mega(ChunkCoord::new(-17, 0, 0)),
            MegaChunkCoord::new(-2, 0)
        );
    }

    #[test]
    fn chunk_to_mega_boundary() {
        // Exactly at megachunk boundary.
        assert_eq!(
            chunk_to_mega(ChunkCoord::new(32, 0, 0)),
            MegaChunkCoord::new(2, 0)
        );
    }

    // -- Aabb3 --

    #[test]
    fn aabb_from_chunk_origin() {
        let aabb = Aabb3::from_chunk(ChunkCoord::new(0, 0, 0));
        assert_eq!(aabb.min, [0.0, 0.0, 0.0]);
        assert_eq!(aabb.max, [16.0, 16.0, 16.0]);
    }

    #[test]
    fn aabb_from_chunk_offset() {
        let aabb = Aabb3::from_chunk(ChunkCoord::new(2, 1, 3));
        assert_eq!(aabb.min, [32.0, 16.0, 48.0]);
        assert_eq!(aabb.max, [48.0, 32.0, 64.0]);
    }

    #[test]
    fn aabb_union() {
        let a = Aabb3::from_chunk(ChunkCoord::new(0, 0, 0));
        let b = Aabb3::from_chunk(ChunkCoord::new(2, 1, 3));
        let u = a.union(b);
        assert_eq!(u.min, [0.0, 0.0, 0.0]);
        assert_eq!(u.max, [48.0, 32.0, 64.0]);
    }

    #[test]
    fn aabb_center() {
        let aabb = Aabb3::from_chunk(ChunkCoord::new(0, 0, 0));
        assert_eq!(aabb.center(), [8.0, 8.0, 8.0]);
    }

    #[test]
    fn aabb_horizontal_distance_inside() {
        let aabb = Aabb3 {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 16.0, 16.0],
        };
        assert_eq!(aabb.horizontal_distance_sq(8.0, 8.0), 0.0);
    }

    #[test]
    fn aabb_horizontal_distance_outside() {
        let aabb = Aabb3 {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 16.0, 16.0],
        };
        // Point at (20, ?, 8): dx=4, dz=0 → dist_sq=16.
        assert_eq!(aabb.horizontal_distance_sq(20.0, 8.0), 16.0);
    }

    #[test]
    fn aabb_horizontal_distance_corner() {
        let aabb = Aabb3 {
            min: [0.0, 0.0, 0.0],
            max: [16.0, 16.0, 16.0],
        };
        // Point at (19, ?, 19): dx=3, dz=3 → dist_sq=18.
        assert_eq!(aabb.horizontal_distance_sq(19.0, 19.0), 18.0);
    }

    // -- Frustum tests --

    /// A frustum that accepts everything (planes far away with outward normals).
    /// Outward convention: inside when `n·p - d < 0`. Setting d = 1000 means
    /// `n·p - 1000 < 0` for any reasonable point, so everything is inside.
    fn open_frustum() -> Vec<[f32; 4]> {
        vec![
            [1.0, 0.0, 0.0, 1000.0],  // +X outward, boundary at x=1000
            [-1.0, 0.0, 0.0, 1000.0], // -X outward, boundary at x=-1000
            [0.0, 1.0, 0.0, 1000.0],  // +Y outward
            [0.0, -1.0, 0.0, 1000.0], // -Y outward
            [0.0, 0.0, 1.0, 1000.0],  // +Z outward
            [0.0, 0.0, -1.0, 1000.0], // -Z outward
        ]
    }

    #[test]
    fn aabb_inside_open_frustum() {
        let aabb = Aabb3::from_chunk(ChunkCoord::new(0, 0, 0));
        assert!(!aabb.is_outside_frustum(&open_frustum()));
    }

    #[test]
    fn aabb_outside_frustum_left() {
        let aabb = Aabb3::from_chunk(ChunkCoord::new(0, 0, 0));
        // Outward +X normal, d=-100. Inside when x - (-100) < 0, i.e. x < -100.
        // AABB N-vertex min x = 0. 0 - (-100) = 100 >= 0 → outside.
        let planes = vec![
            [1.0, 0.0, 0.0, -100.0],
            [-1.0, 0.0, 0.0, 1000.0],
            [0.0, 1.0, 0.0, 1000.0],
            [0.0, -1.0, 0.0, 1000.0],
            [0.0, 0.0, 1.0, 1000.0],
            [0.0, 0.0, -1.0, 1000.0],
        ];
        assert!(aabb.is_outside_frustum(&planes));
    }

    #[test]
    fn aabb_partial_frustum_not_culled() {
        // AABB straddles a plane boundary — should NOT be culled.
        let aabb = Aabb3 {
            min: [-8.0, -8.0, -8.0],
            max: [8.0, 8.0, 8.0],
        };
        // Outward +X normal, d=0. Inside when x < 0. N-vertex min x = -8.
        // -8 - 0 = -8 < 0 → not outside (AABB straddles). Should NOT be culled.
        let planes = vec![
            [1.0, 0.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 1000.0],
            [0.0, 1.0, 0.0, 1000.0],
            [0.0, -1.0, 0.0, 1000.0],
            [0.0, 0.0, 1.0, 1000.0],
            [0.0, 0.0, -1.0, 1000.0],
        ];
        assert!(!aabb.is_outside_frustum(&planes));
    }

    // -- MegaChunk add/remove --

    #[test]
    fn megachunk_add_chunks_expands_aabb() {
        let mut mc = MegaChunk::new();
        mc.add_chunk(ChunkCoord::new(0, 0, 0));
        assert_eq!(
            mc.aabb.unwrap(),
            Aabb3 {
                min: [0.0, 0.0, 0.0],
                max: [16.0, 16.0, 16.0]
            }
        );

        mc.add_chunk(ChunkCoord::new(2, 3, 1));
        let aabb = mc.aabb.unwrap();
        assert_eq!(aabb.min, [0.0, 0.0, 0.0]);
        assert_eq!(aabb.max, [48.0, 64.0, 32.0]);
    }

    #[test]
    fn megachunk_remove_chunk_recomputes_aabb() {
        let mut mc = MegaChunk::new();
        mc.add_chunk(ChunkCoord::new(0, 0, 0));
        mc.add_chunk(ChunkCoord::new(2, 3, 1));
        mc.remove_chunk(&ChunkCoord::new(2, 3, 1));
        assert_eq!(
            mc.aabb.unwrap(),
            Aabb3 {
                min: [0.0, 0.0, 0.0],
                max: [16.0, 16.0, 16.0]
            }
        );
    }

    #[test]
    fn megachunk_remove_all_chunks_clears_aabb() {
        let mut mc = MegaChunk::new();
        mc.add_chunk(ChunkCoord::new(0, 0, 0));
        mc.remove_chunk(&ChunkCoord::new(0, 0, 0));
        assert!(mc.aabb.is_none());
        assert!(mc.chunks.is_empty());
    }

    // -- scan_nonempty_chunks --

    #[test]
    fn scan_empty_world() {
        let world = VoxelWorld::new(16, 16, 16);
        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        assert!(cache.megachunks.is_empty());
    }

    #[test]
    fn scan_single_voxel() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        // Should have 1 megachunk with 1 chunk.
        assert_eq!(cache.megachunks.len(), 1);
        let mc = cache.megachunks.values().next().unwrap();
        assert_eq!(mc.chunks.len(), 1);
        assert!(mc.chunks.contains(&ChunkCoord::new(0, 0, 0)));
    }

    #[test]
    fn scan_tall_column_spans_multiple_chunk_y_levels() {
        let mut world = VoxelWorld::new(16, 48, 16);
        // Trunk from y=0 to y=47 spans 3 chunk Y levels.
        for y in 0..48 {
            world.set(VoxelCoord::new(4, y, 4), VoxelType::Trunk);
        }
        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        let mc = cache.megachunks.values().next().unwrap();
        assert_eq!(mc.chunks.len(), 3);
        assert!(mc.chunks.contains(&ChunkCoord::new(0, 0, 0)));
        assert!(mc.chunks.contains(&ChunkCoord::new(0, 1, 0)));
        assert!(mc.chunks.contains(&ChunkCoord::new(0, 2, 0)));
    }

    #[test]
    fn scan_finds_chunk_with_dirt_and_trunk() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 0, 8), VoxelType::Dirt);
        world.set(VoxelCoord::new(8, 1, 8), VoxelType::Trunk);
        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        // Should find the chunk because both Dirt and Trunk produce geometry.
        assert_eq!(cache.megachunks.len(), 1);
    }

    // -- estimate_byte_size --

    #[test]
    fn estimate_byte_size_via_build() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mut cache = MeshCache::new();
        cache.build_all(&world, &std::collections::BTreeSet::new());

        assert!(cache.total_cached_bytes > 0);
        let coord = ChunkCoord::new(0, 0, 0);
        let mesh_bytes = cache.chunk_bytes.get(&coord).unwrap();
        let mesh = cache.get_chunk(&coord).unwrap();
        assert_eq!(*mesh_bytes, mesh.estimate_byte_size());
    }

    // -- update_visibility: draw distance --

    #[test]
    fn visibility_draw_distance_excludes_far_chunks() {
        let mut world = VoxelWorld::new(256, 16, 16);
        // Place voxels in two chunks far apart.
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk); // chunk (0,0,0)
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk); // chunk (12,0,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(50.0); // 50 voxels — only reaches chunk 0.

        let cam_pos = [8.0, 8.0, 8.0];
        cache.update_visibility(
            &world,
            cam_pos,
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        assert!(cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)));
        assert!(!cache.visible_set.contains(&ChunkCoord::new(12, 0, 0)));
    }

    #[test]
    fn visibility_unlimited_draw_distance_shows_all() {
        let mut world = VoxelWorld::new(256, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0); // unlimited

        let cam_pos = [8.0, 8.0, 8.0];
        cache.update_visibility(
            &world,
            cam_pos,
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        assert!(cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)));
        assert!(cache.visible_set.contains(&ChunkCoord::new(12, 0, 0)));
    }

    // -- update_visibility: frustum --

    #[test]
    fn visibility_frustum_excludes_behind_camera() {
        let mut world = VoxelWorld::new(256, 16, 256);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk); // chunk (0,0,0)
        world.set(VoxelCoord::new(200, 8, 200), VoxelType::Trunk); // chunk (12,0,12)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0); // unlimited distance

        // Frustum: only accept z > 100 (everything else behind camera).
        // Outward -Z normal, d=-100: inside when -z - (-100) < 0 → z > 100.
        let planes = vec![
            [1.0, 0.0, 0.0, 1000.0],
            [-1.0, 0.0, 0.0, 1000.0],
            [0.0, 1.0, 0.0, 1000.0],
            [0.0, -1.0, 0.0, 1000.0],
            [0.0, 0.0, 1.0, 1000.0],
            [0.0, 0.0, -1.0, -100.0], // outward -Z: culls z < 100
        ];
        let cam_pos = [128.0, 8.0, 128.0];
        cache.update_visibility(&world, cam_pos, &planes, &std::collections::BTreeSet::new());

        // Chunk (0,0,0) has z range [0,16] — entirely behind z=100.
        assert!(!cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)));
        // Chunk (12,0,12) has z range [192,208] — in front.
        assert!(cache.visible_set.contains(&ChunkCoord::new(12, 0, 12)));
    }

    // -- update_visibility: on-demand generation --

    #[test]
    fn visibility_generates_meshes_on_demand() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        assert!(cache.chunks.is_empty()); // No meshes yet.

        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        assert!(cache.chunks.contains_key(&ChunkCoord::new(0, 0, 0)));
        assert_eq!(cache.chunks_generated.len(), 1);
    }

    #[test]
    fn visibility_submits_all_chunks_without_cap() {
        // With async mesh generation, all visible chunks are submitted in a
        // single frame (no per-frame cap). Verify all 3 chunks get generated.
        let mut world = VoxelWorld::new(48, 16, 16);
        // 3 chunks with geometry.
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(24, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(40, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        cache.update_visibility(
            &world,
            [24.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        // All 3 chunks should have been generated.
        assert_eq!(cache.chunks_generated.len(), 3);
        assert_eq!(cache.chunks.len(), 3);
    }

    // -- show/hide deltas --

    #[test]
    fn visibility_show_hide_deltas() {
        let mut world = VoxelWorld::new(256, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(50.0);

        // Frame 1: camera near chunk 0. Submits async work.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        // Wait for rayon to complete, then let the next update_visibility
        // drain results via drain_completed (which populates chunks_generated
        // and feeds the diff phase).
        cache.await_in_flight();
        // Frame 2: drain completed results. chunks_generated feeds chunks_to_show.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(cache.chunks_to_show.len(), 1);
        assert!(cache.chunks_to_hide.is_empty());

        // Frame 3: camera moves to chunk 12. Chunk 0 leaves visibility (hide
        // delta fires immediately since mesh exists). Chunk 12 submitted async.
        cache.update_visibility(
            &world,
            [200.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert!(cache.chunks_to_hide.contains(&ChunkCoord::new(0, 0, 0)));

        cache.await_in_flight();
        // Frame 4: drain chunk 12's mesh. Show delta fires via chunks_generated.
        cache.update_visibility(
            &world,
            [200.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert!(cache.chunks_to_show.contains(&ChunkCoord::new(12, 0, 0)));
    }

    // -- LRU eviction --

    #[test]
    fn lru_eviction_under_budget() {
        let mut world = VoxelWorld::new(256, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0); // unlimited

        // Generate all.
        cache.update_visibility(
            &world,
            [100.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        // Drain completed into cache.
        cache.update_visibility(
            &world,
            [100.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(cache.chunks.len(), 2);
        let total = cache.total_cached_bytes;

        // Set budget to roughly half — but both are visible, so no eviction.
        cache.set_memory_budget(total / 2);
        cache.update_visibility(
            &world,
            [100.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert!(cache.chunks_evicted.is_empty()); // Can't evict visible chunks.

        // Now make only chunk 0 visible (move camera close to it).
        cache.set_draw_distance(50.0);
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        // Chunk 12 is hidden and budget is tight — should be evicted.
        assert!(cache.chunks_evicted.contains(&ChunkCoord::new(12, 0, 0)));
        assert!(!cache.chunks.contains_key(&ChunkCoord::new(12, 0, 0)));
    }

    #[test]
    fn lru_visible_chunks_never_evicted() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_memory_budget(1); // Tiny budget.

        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        // Drain completed into cache and run eviction.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // The only chunk is visible — must not be evicted despite budget.
        assert!(cache.chunks_evicted.is_empty());
        assert!(cache.chunks.contains_key(&ChunkCoord::new(0, 0, 0)));
    }

    // -- update_dirty with visibility --

    #[test]
    fn dirty_visible_chunk_rebuilt() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        // Mark dirty and add a voxel.
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Trunk);
        cache.mark_dirty_voxels(&[VoxelCoord::new(9, 8, 8)]);

        let updated = cache.update_dirty(&world, &std::collections::BTreeSet::new());
        cache.flush_in_flight();
        assert_eq!(updated, 1);
    }

    #[test]
    fn dirty_non_visible_chunk_deferred() {
        let mut world = VoxelWorld::new(256, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(50.0);

        // Only chunk 0 visible.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // Dirty chunk 12 (not visible).
        cache.mark_dirty_voxels(&[VoxelCoord::new(200, 8, 8)]);
        let updated = cache.update_dirty(&world, &std::collections::BTreeSet::new());
        assert_eq!(updated, 0); // Deferred — not rebuilt.
        assert!(cache.dirty.contains(&ChunkCoord::new(12, 0, 0)));
    }

    #[test]
    fn dirty_chunk_rebuilt_when_entering_visibility() {
        let mut world = VoxelWorld::new(256, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(50.0);

        // Only chunk 0 visible.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        // Dirty chunk 12 (not visible) and modify the world.
        world.set(VoxelCoord::new(201, 8, 8), VoxelType::Trunk);
        cache.mark_dirty_voxels(&[VoxelCoord::new(201, 8, 8)]);

        // Now move camera to chunk 12.
        cache.update_visibility(
            &world,
            [200.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        // update_visibility submitted the chunk for async generation, and
        // flush_in_flight completed it. The chunk is now visible with an
        // up-to-date mesh.
        assert!(cache.visible_set.contains(&ChunkCoord::new(12, 0, 0)));
        assert!(
            cache.chunks.contains_key(&ChunkCoord::new(12, 0, 0)),
            "dirty chunk entering visibility should get mesh generated"
        );
    }

    // -- LRU eviction ordering --

    #[test]
    fn lru_evicts_oldest_stamp_first() {
        let mut world = VoxelWorld::new(256, 16, 256);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk); // chunk (0,0,0)
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk); // chunk (12,0,0)
        world.set(VoxelCoord::new(8, 8, 200), VoxelType::Trunk); // chunk (0,0,12)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);

        // Frame 1: all visible (submits all 3 for async gen).
        cache.update_visibility(
            &world,
            [100.0, 8.0, 100.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        // Drain completed into cache.
        cache.update_visibility(
            &world,
            [100.0, 8.0, 100.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert_eq!(cache.chunks.len(), 3);

        // Frame 3: touch chunk (12,0,0) by keeping it visible.
        // Move camera so only chunk (12,0,0) is visible.
        cache.set_draw_distance(50.0);
        cache.update_visibility(
            &world,
            [200.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // Now set tight budget — should evict (0,0,0) before (0,0,12) because
        // (0,0,0) was last touched on frame 1, (0,0,12) was also frame 1, but
        // evict_lru picks the minimum stamp.
        let total = cache.total_cached_bytes;
        cache.set_memory_budget(total / 2);
        // Re-run visibility to trigger eviction.
        cache.update_visibility(
            &world,
            [200.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // At least one non-visible chunk should be evicted.
        assert!(!cache.chunks_evicted.is_empty());
        // The visible chunk must survive.
        assert!(cache.chunks.contains_key(&ChunkCoord::new(12, 0, 0)));
    }

    // -- update_dirty: emptied chunk cleanup --

    #[test]
    fn update_dirty_emptied_chunk_removed_from_visible_set() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        assert!(cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)));

        // Remove the voxel and mark dirty.
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Air);
        cache.mark_dirty_voxels(&[VoxelCoord::new(8, 8, 8)]);
        cache.update_dirty(&world, &std::collections::BTreeSet::new());
        cache.flush_in_flight();

        // Chunk should be gone from visible_set, chunks, lru, and bytes.
        assert!(!cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)));
        assert!(!cache.chunks.contains_key(&ChunkCoord::new(0, 0, 0)));
        assert!(!cache.lru_stamps.contains_key(&ChunkCoord::new(0, 0, 0)));
        assert!(!cache.chunk_bytes.contains_key(&ChunkCoord::new(0, 0, 0)));
    }

    // -- update_dirty: byte accounting --

    #[test]
    fn update_dirty_byte_accounting() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        let bytes_before = cache.total_cached_bytes;
        assert!(bytes_before > 0);

        // Add another voxel — mesh gets bigger.
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Trunk);
        cache.mark_dirty_voxels(&[VoxelCoord::new(9, 8, 8)]);
        cache.update_dirty(&world, &std::collections::BTreeSet::new());
        cache.flush_in_flight();

        // total_cached_bytes should reflect the new (larger) mesh.
        let bytes_after = cache.total_cached_bytes;
        assert!(bytes_after > bytes_before);

        // Verify consistency: total should match the sum of chunk_bytes.
        let sum: usize = cache.chunk_bytes.values().sum();
        assert_eq!(cache.total_cached_bytes, sum);
    }

    // -- mark_dirty_voxels megachunk registration --

    #[test]
    fn mark_dirty_registers_chunk_in_megachunk() {
        let world = VoxelWorld::new(32, 16, 16);
        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        // Empty world — no megachunks.
        assert!(cache.megachunks.is_empty());

        // Mark a voxel dirty — should create a megachunk entry.
        cache.mark_dirty_voxels(&[VoxelCoord::new(8, 8, 8)]);
        let mega_coord = chunk_to_mega(ChunkCoord::new(0, 0, 0));
        assert!(cache.megachunks.contains_key(&mega_coord));
        assert!(
            cache.megachunks[&mega_coord]
                .chunks
                .contains(&ChunkCoord::new(0, 0, 0))
        );
    }

    // -- pending gen dropped when no longer visible --

    #[test]
    fn in_flight_results_for_hidden_chunks_not_shown() {
        // With async mesh generation, all chunks are submitted immediately.
        // If a chunk leaves visibility before its mesh completes, the
        // completed mesh should still be cached but not added to
        // chunks_to_show.
        let mut world = VoxelWorld::new(256, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        // Frame 1: all visible, submits both chunks.
        cache.update_visibility(
            &world,
            [100.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        // Frame 2: restrict visibility to chunk 0 only. Drain will pick up
        // completed meshes but chunk 12 is no longer visible.
        cache.set_draw_distance(50.0);
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // Only chunk 0 should be visible.
        assert!(cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)));
        assert!(!cache.visible_set.contains(&ChunkCoord::new(12, 0, 0)));
    }

    // -- scan with y_cutoff interaction --

    #[test]
    fn scan_registers_chunk_despite_y_cutoff() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        // Set cutoff below the voxel.
        cache.set_y_cutoff(Some(5));
        cache.scan_nonempty_chunks(&world);

        // The chunk should still be registered (scan ignores cutoff).
        assert_eq!(cache.megachunks.len(), 1);

        // But update_visibility should handle the empty mesh gracefully.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        // Drain completed so empty mesh result is processed.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // Chunk was scanned as non-empty but y_cutoff hides all geometry.
        // It should NOT be in visible_set (removed when mesh gen returns empty).
        assert!(!cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)));
    }

    // -- AABB with negative coords --

    #[test]
    fn aabb_from_chunk_negative() {
        let aabb = Aabb3::from_chunk(ChunkCoord::new(-1, -1, -1));
        assert_eq!(aabb.min, [-16.0, -16.0, -16.0]);
        assert_eq!(aabb.max, [0.0, 0.0, 0.0]);
    }

    // -- chunks_generated subset of chunks_to_show --

    #[test]
    fn generated_chunks_always_in_show_list() {
        let mut world = VoxelWorld::new(48, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(24, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(40, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        // Frame 1: generate 1.
        cache.update_visibility(
            &world,
            [24.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        for &c in &cache.chunks_generated {
            assert!(
                cache.chunks_to_show.contains(&c),
                "Generated chunk {:?} not in show list",
                c
            );
        }

        // Frame 2: generate another (carry-over pending).
        cache.update_visibility(
            &world,
            [24.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        for &c in &cache.chunks_generated {
            assert!(
                cache.chunks_to_show.contains(&c),
                "Generated chunk {:?} not in show list on frame 2",
                c
            );
        }
    }

    // -- hide and evict mutually exclusive --

    // -- y_cutoff megachunk re-registration bug --

    #[test]
    fn removing_cutoff_restores_previously_hidden_chunks() {
        // World with voxels at two different heights: chunk cy=0 (low) and cy=2 (high).
        // A 48-tall world gives us 3 chunk Y-levels: 0, 1, 2.
        let mut world = VoxelWorld::new(16, 48, 16);
        world.set(VoxelCoord::new(8, 4, 8), VoxelType::Trunk); // chunk (0,0,0)
        world.set(VoxelCoord::new(8, 36, 8), VoxelType::Trunk); // chunk (0,2,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0); // unlimited

        // Step 1: No cutoff — both chunks should be visible.
        cache.update_visibility(
            &world,
            [8.0, 20.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        // Drain completed into cache.
        cache.update_visibility(
            &world,
            [8.0, 20.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert!(
            cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)),
            "Low chunk should be visible initially"
        );
        assert!(
            cache.visible_set.contains(&ChunkCoord::new(0, 2, 0)),
            "High chunk should be visible initially"
        );

        // Step 2: Set cutoff below the high chunk — it generates an empty mesh
        // and gets removed from visible_set and megachunks.
        cache.set_y_cutoff(Some(20));
        cache.update_dirty(&world, &std::collections::BTreeSet::new());
        cache.flush_in_flight();
        // The high chunk's mesh is now empty, so update_dirty removed it from
        // visible_set and megachunks.
        assert!(
            !cache.visible_set.contains(&ChunkCoord::new(0, 2, 0)),
            "High chunk should be hidden after cutoff"
        );

        // Step 3: Remove the cutoff entirely.
        cache.set_y_cutoff(None);

        // Step 4: Run update_dirty to process the dirty chunks.
        cache.update_dirty(&world, &std::collections::BTreeSet::new());
        cache.flush_in_flight();

        // Step 5: Run update_visibility — the high chunk should reappear.
        cache.update_visibility(
            &world,
            [8.0, 20.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        // Drain completed into cache.
        cache.update_visibility(
            &world,
            [8.0, 20.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // BUG: The high chunk was removed from megachunks in step 2, and
        // set_y_cutoff (step 3) only added it to the dirty set, NOT back
        // into megachunks. So update_visibility never iterates over it,
        // and update_dirty skips it because it's not in visible_set.
        // The chunk is lost.
        assert!(
            cache.visible_set.contains(&ChunkCoord::new(0, 2, 0)),
            "High chunk should be visible again after cutoff removed"
        );
    }

    #[test]
    fn raising_cutoff_restores_chunks_above_old_cutoff() {
        // Similar scenario: verify that raising the cutoff (not just removing)
        // also restores previously-hidden chunks.
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(8, 4, 8), VoxelType::Trunk); // chunk (0,0,0)
        world.set(VoxelCoord::new(8, 36, 8), VoxelType::Trunk); // chunk (0,2,0)
        world.set(VoxelCoord::new(8, 52, 8), VoxelType::Trunk); // chunk (0,3,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);

        // Initial: all visible.
        cache.update_visibility(
            &world,
            [8.0, 30.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        cache.update_visibility(
            &world,
            [8.0, 30.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert!(cache.visible_set.contains(&ChunkCoord::new(0, 2, 0)));
        assert!(cache.visible_set.contains(&ChunkCoord::new(0, 3, 0)));

        // Set cutoff low — hides chunks cy=2 and cy=3.
        cache.set_y_cutoff(Some(20));
        cache.update_dirty(&world, &std::collections::BTreeSet::new());
        cache.flush_in_flight();
        assert!(!cache.visible_set.contains(&ChunkCoord::new(0, 2, 0)));
        assert!(!cache.visible_set.contains(&ChunkCoord::new(0, 3, 0)));

        // Raise cutoff to reveal chunk cy=2 but keep cy=3 hidden.
        cache.set_y_cutoff(Some(48));
        cache.update_dirty(&world, &std::collections::BTreeSet::new());
        cache.flush_in_flight();
        cache.update_visibility(
            &world,
            [8.0, 30.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        cache.update_visibility(
            &world,
            [8.0, 30.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // BUG: chunk (0,2,0) was removed from megachunks and never re-added.
        assert!(
            cache.visible_set.contains(&ChunkCoord::new(0, 2, 0)),
            "Chunk at cy=2 should be visible after raising cutoff above it"
        );
    }

    #[test]
    fn evicted_chunks_not_in_hide_list() {
        let mut world = VoxelWorld::new(256, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);

        // Generate both.
        cache.update_visibility(
            &world,
            [100.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // Tight budget and restrict visibility.
        cache.set_memory_budget(1);
        cache.set_draw_distance(50.0);
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // No chunk should appear in both lists.
        for c in &cache.chunks_evicted {
            assert!(
                !cache.chunks_to_hide.contains(c),
                "Chunk {:?} in both evicted and hide lists",
                c
            );
        }
    }

    // -- empty_chunks caching --

    #[test]
    fn empty_chunk_not_regenerated_every_frame() {
        // A chunk with geometry-producing voxels that are fully interior
        // (all faces culled) generates an empty mesh. It should be cached
        // in empty_chunks and not re-generated on subsequent frames.
        let mut world = VoxelWorld::new(48, 48, 48);
        // Fill a 16³ interior so chunk (1,1,1) is fully surrounded.
        for x in 0..48 {
            for y in 0..48 {
                for z in 0..48 {
                    world.set(VoxelCoord::new(x, y, z), VoxelType::Trunk);
                }
            }
        }

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);

        // Frame 1: submits all chunks for async generation.
        cache.update_visibility(
            &world,
            [24.0, 24.0, 24.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        assert!(
            !cache.empty_chunks.is_empty(),
            "Interior chunks should be in empty_chunks"
        );
        assert!(
            cache.empty_chunks.contains(&ChunkCoord::new(1, 1, 1)),
            "Fully interior chunk (1,1,1) should be in empty_chunks"
        );

        // Frame 2: no new chunks should be submitted (all cached or empty).
        cache.update_visibility(
            &world,
            [24.0, 24.0, 24.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert!(
            cache.in_flight.is_empty(),
            "No chunks should be submitted on second frame"
        );
    }

    #[test]
    fn empty_chunk_cleared_on_mark_dirty() {
        let mut world = VoxelWorld::new(48, 48, 48);
        for x in 0..48 {
            for y in 0..48 {
                for z in 0..48 {
                    world.set(VoxelCoord::new(x, y, z), VoxelType::Trunk);
                }
            }
        }

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);

        cache.update_visibility(
            &world,
            [24.0, 24.0, 24.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        assert!(cache.empty_chunks.contains(&ChunkCoord::new(1, 1, 1)));

        // Clear a voxel inside the interior chunk — should remove from empty_chunks.
        cache.mark_dirty_voxels(&[VoxelCoord::new(20, 20, 20)]);
        assert!(
            !cache.empty_chunks.contains(&ChunkCoord::new(1, 1, 1)),
            "Dirty chunk should be removed from empty_chunks"
        );
    }

    #[test]
    fn empty_chunk_boundary_neighbor_cleared_on_dirty() {
        let mut world = VoxelWorld::new(48, 48, 48);
        for x in 0..48 {
            for y in 0..48 {
                for z in 0..48 {
                    world.set(VoxelCoord::new(x, y, z), VoxelType::Trunk);
                }
            }
        }

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);

        cache.update_visibility(
            &world,
            [24.0, 24.0, 24.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        assert!(cache.empty_chunks.contains(&ChunkCoord::new(1, 1, 1)));

        // Edit a voxel at the boundary of chunk (0,1,1) at local_x=15.
        // Neighbor chunk (1,1,1) should be cleared from empty_chunks.
        cache.mark_dirty_voxels(&[VoxelCoord::new(15, 20, 20)]);
        assert!(
            !cache.empty_chunks.contains(&ChunkCoord::new(1, 1, 1)),
            "Boundary neighbor should be cleared from empty_chunks"
        );
    }

    #[test]
    fn empty_chunk_cleared_on_y_cutoff_change() {
        let mut world = VoxelWorld::new(16, 48, 16);
        // Single voxel in chunk (0,0,0).
        world.set(VoxelCoord::new(8, 4, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);

        // Set cutoff below the voxel — chunk generates empty mesh.
        cache.set_y_cutoff(Some(2));
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        assert!(
            cache.empty_chunks.contains(&ChunkCoord::new(0, 0, 0)),
            "Chunk below cutoff should be in empty_chunks"
        );

        // Remove cutoff — should clear empty_chunks for affected range.
        cache.set_y_cutoff(None);
        assert!(
            !cache.empty_chunks.contains(&ChunkCoord::new(0, 0, 0)),
            "Chunk should be cleared from empty_chunks after cutoff removed"
        );
    }

    #[test]
    fn scan_nonempty_chunks_clears_empty_chunks() {
        let mut world = VoxelWorld::new(48, 48, 48);
        for x in 0..48 {
            for y in 0..48 {
                for z in 0..48 {
                    world.set(VoxelCoord::new(x, y, z), VoxelType::Trunk);
                }
            }
        }

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);

        cache.update_visibility(
            &world,
            [24.0, 24.0, 24.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        assert!(!cache.empty_chunks.is_empty());

        // Re-scanning should clear the empty_chunks set.
        cache.scan_nonempty_chunks(&world);
        assert!(
            cache.empty_chunks.is_empty(),
            "scan_nonempty_chunks should clear empty_chunks"
        );
    }

    #[test]
    fn update_dirty_emptied_chunk_added_to_empty_chunks() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        assert!(cache.chunks.contains_key(&ChunkCoord::new(0, 0, 0)));

        // Remove the voxel and mark dirty.
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Air);
        cache.mark_dirty_voxels(&[VoxelCoord::new(8, 8, 8)]);
        cache.update_dirty(&world, &std::collections::BTreeSet::new());
        cache.flush_in_flight();

        // The chunk should now be in empty_chunks.
        assert!(
            cache.empty_chunks.contains(&ChunkCoord::new(0, 0, 0)),
            "update_dirty should add emptied chunk to empty_chunks"
        );
    }

    // -- build_shadow_planes --

    /// Build a box-shaped frustum centered on (0, 0, 100): X in [-50, 50],
    /// Y in [-50, 50], Z in [1, 200]. Outward normals, inside when n·p - d < 0.
    fn camera_frustum_looking_z() -> Vec<[f32; 4]> {
        vec![
            [0.0, 0.0, -1.0, -1.0], // near: outward -Z, inside when z > 1
            [0.0, 0.0, 1.0, 200.0], // far: outward +Z, inside when z < 200
            [-1.0, 0.0, 0.0, 50.0], // left: outward -X, inside when x > -50
            [1.0, 0.0, 0.0, 50.0],  // right: outward +X, inside when x < 50
            [0.0, 1.0, 0.0, 50.0],  // top: outward +Y, inside when y < 50
            [0.0, -1.0, 0.0, 50.0], // bottom: outward -Y, inside when y > -50
        ]
    }

    #[test]
    fn shadow_planes_chunk_above_camera_with_downward_light() {
        // Camera looking along +Z. Light shines straight down: (0, -1, 0).
        // A chunk directly above the camera (high Y) is upstream of the light
        // and should be inside the shadow volume.
        let frustum = camera_frustum_looking_z();
        let light_dir = [0.0, -1.0, 0.0]; // pointing down

        let shadow_planes = build_shadow_planes(&frustum, light_dir, 500.0);
        assert!(!shadow_planes.is_empty(), "should produce shadow planes");

        // Chunk at (0, 5, 6): Y range [80, 96], well above camera.
        // Z range [96, 112], within frustum Z range.
        let above = Aabb3::from_chunk(ChunkCoord::new(0, 5, 6));
        assert!(
            !above.is_outside_frustum(&shadow_planes),
            "chunk above camera should be inside shadow volume with downward light"
        );
    }

    #[test]
    fn shadow_planes_chunk_below_camera_with_downward_light() {
        // Camera at origin looking +Z. Light shines down.
        // A chunk below the camera (negative Y) is downstream — its shadow goes
        // further down, not into the frustum.
        let frustum = camera_frustum_looking_z();
        let light_dir = [0.0, -1.0, 0.0];

        let shadow_planes = build_shadow_planes(&frustum, light_dir, 500.0);

        // Chunk at (0, -5, 6): Y range [-80, -64], well below camera.
        let below = Aabb3::from_chunk(ChunkCoord::new(0, -5, 6));
        assert!(
            below.is_outside_frustum(&shadow_planes),
            "chunk below camera should be outside shadow volume with downward light"
        );
    }

    #[test]
    fn shadow_planes_chunk_laterally_outside() {
        // Camera looking +Z. Light shines down. A chunk far to the right
        // (high X) can't cast shadows into the frustum even though it's
        // upstream vertically.
        let frustum = camera_frustum_looking_z();
        let light_dir = [0.0, -1.0, 0.0];

        let shadow_planes = build_shadow_planes(&frustum, light_dir, 500.0);

        // Chunk at (10, 5, 6): X range [160, 176], way off to the right.
        let lateral = Aabb3::from_chunk(ChunkCoord::new(10, 5, 6));
        assert!(
            lateral.is_outside_frustum(&shadow_planes),
            "chunk far to the side should be outside shadow volume"
        );
    }

    #[test]
    fn shadow_planes_diagonal_light() {
        // Camera looking +Z. Light from upper-left: normalized (-0.5, -0.7, -0.5).
        let frustum = camera_frustum_looking_z();
        let light_dir = [-0.5, -0.707, -0.5]; // roughly normalized

        let shadow_planes = build_shadow_planes(&frustum, light_dir, 500.0);
        assert!(!shadow_planes.is_empty());

        // Chunk above and to the left: should be inside shadow volume.
        // X range [-16, 0], Y range [48, 64], Z range [96, 112].
        let upstream = Aabb3::from_chunk(ChunkCoord::new(-1, 3, 6));
        assert!(
            !upstream.is_outside_frustum(&shadow_planes),
            "chunk upstream of diagonal light should be in shadow volume"
        );

        // Chunk below and to the right: downstream, should be outside.
        let downstream = Aabb3::from_chunk(ChunkCoord::new(10, -5, 6));
        assert!(
            downstream.is_outside_frustum(&shadow_planes),
            "chunk downstream of diagonal light should be outside shadow volume"
        );
    }

    #[test]
    fn shadow_planes_empty_with_insufficient_planes() {
        let light_dir = [0.0, -1.0, 0.0];
        let planes = build_shadow_planes(&[], light_dir, 500.0);
        assert!(planes.is_empty());
    }

    #[test]
    fn shadow_planes_frustum_interior_still_inside() {
        // Points inside the original frustum should also be inside the shadow
        // volume (it's a superset).
        let frustum = camera_frustum_looking_z();
        let light_dir = [0.0, -1.0, 0.0];

        let shadow_planes = build_shadow_planes(&frustum, light_dir, 500.0);

        let inside = Aabb3::from_chunk(ChunkCoord::new(0, 0, 6));
        assert!(
            !inside.is_outside_frustum(&shadow_planes),
            "chunk inside the original frustum should also be inside shadow volume"
        );
    }

    // -- Shadow-only visibility integration tests --

    #[test]
    fn shadow_visibility_chunk_above_frustum_becomes_shadow_only() {
        // World: two chunks stacked — one at Y=0 (in frustum), one at Y=3 (above).
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk); // chunk (0,0,0)
        world.set(VoxelCoord::new(8, 56, 8), VoxelType::Trunk); // chunk (0,3,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0); // unlimited
        // Light shines straight down.
        cache.set_light_direction(Some([0.0, -1.0, 0.0]));

        // Frustum: Y in [-10, 20], so chunk at Y=0 is in frustum, Y=3 is above.
        let frustum = vec![
            [0.0, 0.0, -1.0, 500.0], // near
            [0.0, 0.0, 1.0, 500.0],  // far
            [-1.0, 0.0, 0.0, 500.0], // left
            [1.0, 0.0, 0.0, 500.0],  // right
            [0.0, 1.0, 0.0, 20.0],   // top: inside when y < 20
            [0.0, -1.0, 0.0, 10.0],  // bottom: inside when y > -10
        ];

        // Frame 1: submit async work.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &frustum,
            &std::collections::BTreeSet::new(),
        );
        // Wait for rayon to complete.
        cache.await_in_flight();
        // Frame 2: drain completed results. chunks_generated feeds delta lists.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &frustum,
            &std::collections::BTreeSet::new(),
        );

        assert!(
            cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)),
            "chunk at Y=0 should be visible"
        );
        assert!(
            cache.shadow_set.contains(&ChunkCoord::new(0, 3, 0)),
            "chunk at Y=3 should be shadow-only (above frustum, upstream of downward light)"
        );
        // The shadow chunk should be in chunks_to_shadow (mesh arrived this frame).
        assert!(
            cache.chunks_to_shadow().contains(&ChunkCoord::new(0, 3, 0)),
            "chunk at Y=3 should be in chunks_to_shadow"
        );
    }

    #[test]
    fn shadow_visibility_no_light_direction_means_no_shadow_set() {
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(8, 56, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);
        // No light direction set.

        let frustum = vec![
            [0.0, 0.0, -1.0, 500.0],
            [0.0, 0.0, 1.0, 500.0],
            [-1.0, 0.0, 0.0, 500.0],
            [1.0, 0.0, 0.0, 500.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, -1.0, 0.0, 10.0],
        ];

        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &frustum,
            &std::collections::BTreeSet::new(),
        );

        assert!(
            cache.shadow_set.is_empty(),
            "no light direction → no shadow-only chunks"
        );
        assert!(
            !cache.visible_set.contains(&ChunkCoord::new(0, 3, 0)),
            "chunk above frustum should be hidden without light direction"
        );
    }

    #[test]
    fn shadow_visibility_shadow_to_visible_transition() {
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(8, 56, 8), VoxelType::Trunk); // chunk (0,3,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);
        cache.set_light_direction(Some([0.0, -1.0, 0.0]));

        // Frame 1: frustum excludes Y=3 → shadow-only.
        let narrow_frustum = vec![
            [0.0, 0.0, -1.0, 500.0],
            [0.0, 0.0, 1.0, 500.0],
            [-1.0, 0.0, 0.0, 500.0],
            [1.0, 0.0, 0.0, 500.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, -1.0, 0.0, 10.0],
        ];
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &narrow_frustum,
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        assert!(cache.shadow_set.contains(&ChunkCoord::new(0, 3, 0)));

        // Frame 2: frustum includes Y=3 → visible. Drain picks up mesh.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert!(
            cache.visible_set.contains(&ChunkCoord::new(0, 3, 0)),
            "chunk should transition to visible"
        );
        assert!(
            !cache.shadow_set.contains(&ChunkCoord::new(0, 3, 0)),
            "chunk should leave shadow set"
        );
        assert!(
            cache.chunks_to_show().contains(&ChunkCoord::new(0, 3, 0)),
            "shadow→visible should produce chunks_to_show entry"
        );
    }

    #[test]
    fn shadow_visibility_shadow_to_hidden_transition() {
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(8, 56, 8), VoxelType::Trunk); // chunk (0,3,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);
        cache.set_light_direction(Some([0.0, -1.0, 0.0]));

        // Frame 1: shadow-only.
        let narrow_frustum = vec![
            [0.0, 0.0, -1.0, 500.0],
            [0.0, 0.0, 1.0, 500.0],
            [-1.0, 0.0, 0.0, 500.0],
            [1.0, 0.0, 0.0, 500.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, -1.0, 0.0, 10.0],
        ];
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &narrow_frustum,
            &std::collections::BTreeSet::new(),
        );
        assert!(cache.shadow_set.contains(&ChunkCoord::new(0, 3, 0)));

        // Frame 2: disable light direction → shadow→hidden.
        cache.set_light_direction(None);
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &narrow_frustum,
            &std::collections::BTreeSet::new(),
        );
        assert!(cache.shadow_set.is_empty());
        assert!(
            cache
                .chunks_from_shadow()
                .contains(&ChunkCoord::new(0, 3, 0)),
            "shadow→hidden should produce chunks_from_shadow entry"
        );
    }

    #[test]
    fn shadow_visibility_visible_to_shadow_transition() {
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(8, 56, 8), VoxelType::Trunk); // chunk (0,3,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);
        cache.set_light_direction(Some([0.0, -1.0, 0.0]));

        // Frame 1: open frustum → chunk is visible.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        assert!(cache.visible_set.contains(&ChunkCoord::new(0, 3, 0)));
        assert!(!cache.shadow_set.contains(&ChunkCoord::new(0, 3, 0)));

        // Frame 2: narrow frustum excludes Y=3 → visible→shadow.
        let narrow_frustum = vec![
            [0.0, 0.0, -1.0, 500.0],
            [0.0, 0.0, 1.0, 500.0],
            [-1.0, 0.0, 0.0, 500.0],
            [1.0, 0.0, 0.0, 500.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, -1.0, 0.0, 10.0],
        ];
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &narrow_frustum,
            &std::collections::BTreeSet::new(),
        );
        assert!(
            !cache.visible_set.contains(&ChunkCoord::new(0, 3, 0)),
            "chunk should leave visible set"
        );
        assert!(
            cache.shadow_set.contains(&ChunkCoord::new(0, 3, 0)),
            "chunk should enter shadow set"
        );
        assert!(
            cache.chunks_to_shadow().contains(&ChunkCoord::new(0, 3, 0)),
            "visible→shadow should produce chunks_to_shadow entry"
        );
    }

    #[test]
    fn shadow_visibility_visible_to_hidden_not_in_shadow() {
        // A chunk that leaves the frustum AND the shadow volume should go
        // to chunks_to_hide, not chunks_to_shadow.
        let mut world = VoxelWorld::new(16, 64, 16);
        // Chunk at Y=0, below the camera in a downward-light scenario.
        // Its shadow goes further down, not into the frustum.
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk); // chunk (0,0,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);
        cache.set_light_direction(Some([0.0, -1.0, 0.0]));

        // Frame 1: open frustum → visible.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        assert!(cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)));

        // Frame 2: frustum high above → chunk below is hidden (not shadow).
        let high_frustum = vec![
            [0.0, 0.0, -1.0, 500.0],
            [0.0, 0.0, 1.0, 500.0],
            [-1.0, 0.0, 0.0, 500.0],
            [1.0, 0.0, 0.0, 500.0],
            [0.0, 1.0, 0.0, 500.0],
            [0.0, -1.0, 0.0, -100.0], // bottom: inside when y > 100
        ];
        cache.update_visibility(
            &world,
            [8.0, 200.0, 8.0],
            &high_frustum,
            &std::collections::BTreeSet::new(),
        );
        assert!(
            cache.chunks_to_hide().contains(&ChunkCoord::new(0, 0, 0)),
            "chunk below camera with downward light should go to hide, not shadow"
        );
        assert!(
            !cache.chunks_to_shadow().contains(&ChunkCoord::new(0, 0, 0)),
            "chunk should NOT be in shadow list"
        );
    }

    #[test]
    fn shadow_visibility_stable_shadow_no_delta() {
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(8, 56, 8), VoxelType::Trunk); // chunk (0,3,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);
        cache.set_light_direction(Some([0.0, -1.0, 0.0]));

        let narrow_frustum = vec![
            [0.0, 0.0, -1.0, 500.0],
            [0.0, 0.0, 1.0, 500.0],
            [-1.0, 0.0, 0.0, 500.0],
            [1.0, 0.0, 0.0, 500.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, -1.0, 0.0, 10.0],
        ];

        // Frame 1: enters shadow (submits async).
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &narrow_frustum,
            &std::collections::BTreeSet::new(),
        );
        assert!(cache.shadow_set.contains(&ChunkCoord::new(0, 3, 0)));
        // Wait for rayon to complete.
        cache.await_in_flight();

        // Frame 2: drain picks up completed mesh, chunk enters shadow with delta.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &narrow_frustum,
            &std::collections::BTreeSet::new(),
        );
        assert!(!cache.chunks_to_shadow().is_empty());

        // Frame 3: same state → no delta.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &narrow_frustum,
            &std::collections::BTreeSet::new(),
        );
        assert!(cache.shadow_set.contains(&ChunkCoord::new(0, 3, 0)));
        assert!(
            cache.chunks_to_shadow().is_empty(),
            "stable shadow chunk should not re-emit to_shadow"
        );
        assert!(
            cache.chunks_from_shadow().is_empty(),
            "stable shadow chunk should not emit from_shadow"
        );
    }

    #[test]
    fn shadow_visibility_draw_distance_limits_shadow() {
        // Chunk far in XZ (200 voxels away) but upstream of downward light.
        let mut world = VoxelWorld::new(256, 64, 16);
        world.set(VoxelCoord::new(200, 56, 8), VoxelType::Trunk); // chunk (12,3,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(50.0); // 50 voxels — chunk at x=200 is out of range
        cache.set_light_direction(Some([0.0, -1.0, 0.0]));

        let narrow_frustum = vec![
            [0.0, 0.0, -1.0, 500.0],
            [0.0, 0.0, 1.0, 500.0],
            [-1.0, 0.0, 0.0, 500.0],
            [1.0, 0.0, 0.0, 500.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, -1.0, 0.0, 10.0],
        ];
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &narrow_frustum,
            &std::collections::BTreeSet::new(),
        );

        assert!(
            !cache.shadow_set.contains(&ChunkCoord::new(12, 3, 0)),
            "chunk beyond draw distance should not be in shadow set"
        );
    }

    #[test]
    fn shadow_visibility_pending_gen_serves_shadow_chunks() {
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(8, 56, 8), VoxelType::Trunk); // chunk (0,3,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(0.0);
        cache.set_light_direction(Some([0.0, -1.0, 0.0]));

        // Narrow frustum: chunk at Y=3 is outside frustum but in shadow volume.
        let narrow_frustum = vec![
            [0.0, 0.0, -1.0, 500.0],
            [0.0, 0.0, 1.0, 500.0],
            [-1.0, 0.0, 0.0, 500.0],
            [1.0, 0.0, 0.0, 500.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, -1.0, 0.0, 10.0],
        ];
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &narrow_frustum,
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        // The shadow-only chunk should have been generated (mesh exists).
        assert!(
            cache.chunks.contains_key(&ChunkCoord::new(0, 3, 0)),
            "shadow-only chunk should get mesh generated"
        );
        assert!(
            cache.shadow_set.contains(&ChunkCoord::new(0, 3, 0)),
            "chunk should be in shadow set"
        );
    }

    // -- Async pipeline: freshness, dedup, dirty-while-in-flight --

    #[test]
    fn stale_result_discarded_by_sim_tick() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.sim_tick = 10; // Non-zero so staleness check is meaningful.

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        // Generate the chunk and flush so it's cached.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        let coord = ChunkCoord::new(0, 0, 0);
        assert!(cache.chunks.contains_key(&coord));

        // Record the cached tick and manually insert a stale result
        // with a lower sim_tick.
        let cached_tick = *cache.cached_ticks.get(&coord).unwrap();
        let stale_result = super::MeshWorkResult {
            coord,
            sim_tick: cached_tick.saturating_sub(1),
            mesh: ChunkMesh::default(), // empty — would remove chunk if accepted
            gen_us: 0,
        };
        let accepted = cache.handle_completed_result(stale_result);
        assert!(!accepted, "stale result should be discarded");
        // Chunk should still be in cache (not removed by empty mesh).
        assert!(cache.chunks.contains_key(&coord));
    }

    #[test]
    fn duplicate_submit_prevented_by_in_flight() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        let coord = ChunkCoord::new(0, 0, 0);
        let grassless = std::collections::BTreeSet::new();

        // Submit once — should be in-flight.
        cache.submit_chunk(&world, coord, &grassless);
        assert!(cache.in_flight.contains(&coord));

        // Submit again — should be a no-op (still one in-flight).
        cache.submit_chunk(&world, coord, &grassless);
        assert!(cache.in_flight.contains(&coord));

        // Flush — should get exactly one result.
        cache.flush_in_flight();
        assert!(cache.chunks.contains_key(&coord));
    }

    #[test]
    fn dirty_while_in_flight_resubmits_after_completion() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        // Generate and flush so chunk is visible and cached.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();

        let coord = ChunkCoord::new(0, 0, 0);
        assert!(cache.visible_set.contains(&coord));

        // Mark dirty and submit.
        cache.mark_dirty_voxels(&[VoxelCoord::new(8, 8, 8)]);
        let submitted = cache.update_dirty(&world, &std::collections::BTreeSet::new());
        assert_eq!(submitted, 1);
        assert!(cache.in_flight.contains(&coord));

        // While in-flight, mark dirty again (simulating a new voxel edit).
        cache.mark_dirty_voxels(&[VoxelCoord::new(9, 8, 8)]);
        assert!(cache.dirty.contains(&coord));

        // update_dirty should NOT resubmit because chunk is in-flight.
        let submitted2 = cache.update_dirty(&world, &std::collections::BTreeSet::new());
        assert_eq!(submitted2, 0);
        // Dirty flag should still be set.
        assert!(cache.dirty.contains(&coord));

        // Flush the in-flight result.
        cache.flush_in_flight();
        assert!(!cache.in_flight.contains(&coord));

        // Now update_dirty should pick up the chunk again.
        let submitted3 = cache.update_dirty(&world, &std::collections::BTreeSet::new());
        assert_eq!(submitted3, 1);
        cache.flush_in_flight();
    }

    #[test]
    fn empty_mesh_from_dirty_produces_hide_delta() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        // Generate and flush so chunk is visible and cached.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.flush_in_flight();
        // Second update_visibility to get the chunk into visible_set via delta.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        let coord = ChunkCoord::new(0, 0, 0);
        assert!(cache.visible_set.contains(&coord));

        // Remove the voxel so the chunk becomes empty.
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Air);
        cache.mark_dirty_voxels(&[VoxelCoord::new(8, 8, 8)]);

        // Submit dirty and flush (background gen produces empty mesh).
        cache.update_dirty(&world, &std::collections::BTreeSet::new());
        cache.flush_in_flight();

        // The chunk should produce a hide delta so GDScript removes the
        // MeshInstance3D.
        assert!(
            cache.chunks_to_hide.contains(&coord),
            "empty mesh result should produce a hide delta"
        );
        assert!(!cache.visible_set.contains(&coord));
        assert!(cache.empty_chunks.contains(&coord));
    }

    #[test]
    fn generated_chunk_outside_draw_distance_gets_hide_delta() {
        // A chunk is submitted while visible, but by the time its mesh
        // completes and is drained, the camera has moved away. The chunk
        // should appear in chunks_generated (GDScript creates the
        // MeshInstance3D) AND in chunks_to_hide (GDScript immediately
        // hides it). This ensures the node exists for future show deltas.
        let mut world = VoxelWorld::new(256, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk); // chunk (0,0,0)
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk); // chunk (12,0,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(50.0);

        // Frame 1: camera near chunk 0 — chunk 0 submitted.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        // Chunk 0 is in-flight. Await completion so drain picks it up.
        cache.await_in_flight();

        // Frame 2: camera moved to chunk 12, far from chunk 0.
        cache.update_visibility(
            &world,
            [200.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        let c0 = ChunkCoord::new(0, 0, 0);

        // Chunk 0 IS in chunks_generated (MeshInstance3D will be created).
        assert!(
            cache.chunks_generated.contains(&c0),
            "generated chunk should be in chunks_generated even if outside draw distance"
        );
        // Chunk 0 also gets a hide delta (MeshInstance3D immediately hidden).
        assert!(
            cache.chunks_to_hide.contains(&c0),
            "generated chunk outside draw distance should get a hide delta"
        );
        // Mesh is cached for future reuse.
        assert!(cache.chunks.contains_key(&c0));
    }

    #[test]
    fn chunk_generated_while_offscreen_shows_on_reentry() {
        // A chunk completes generation while offscreen, then the camera
        // returns. The chunk should appear via chunks_to_show because
        // its MeshInstance3D was created (and hidden) earlier.
        let mut world = VoxelWorld::new(256, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk); // chunk (0,0,0)
        world.set(VoxelCoord::new(200, 8, 8), VoxelType::Trunk); // chunk (12,0,0)

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_draw_distance(50.0);

        // Frame 1: camera near chunk 0 — submit chunk 0.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        cache.await_in_flight();

        // Frame 2: camera far away — chunk 0 drained + hidden.
        cache.update_visibility(
            &world,
            [200.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );
        // Chunk 0 is now cached but hidden.
        assert!(!cache.visible_set.contains(&ChunkCoord::new(0, 0, 0)));

        // Frame 3: camera returns to chunk 0.
        cache.update_visibility(
            &world,
            [8.0, 8.0, 8.0],
            &open_frustum(),
            &std::collections::BTreeSet::new(),
        );

        let c0 = ChunkCoord::new(0, 0, 0);
        assert!(
            cache.chunks_to_show.contains(&c0),
            "cached chunk re-entering visibility should get a show delta"
        );
        assert!(cache.visible_set.contains(&c0));
    }
}
