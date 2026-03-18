// Chunk mesh cache with MegaChunk spatial hierarchy for the gdext bridge.
//
// Sits between the sim's pure `mesh_gen` module and the Godot-facing
// `sim_bridge.rs`. Organises chunks into MegaChunks (16×16 horizontal groups)
// for fast draw-distance and frustum culling. Meshes are generated lazily —
// only when a chunk first enters the camera's visible set — and evicted via
// LRU when a configurable memory budget is exceeded.
//
// ## Visibility pipeline (per frame)
//
// 1. GDScript sends camera position + 6 frustum planes.
// 2. `update_visibility()` tests each MegaChunk AABB against draw distance
//    (XZ only) and then against the frustum (coarse).
// 3. Individual chunk AABBs within passing MegaChunks are frustum-tested (fine).
// 4. Newly-visible chunks without cached meshes are generated on-demand, up to
//    `max_gen_per_frame` per call.
// 5. Delta lists (show, hide, generated, evicted) are produced for GDScript to
//    toggle MeshInstance3D visibility and create/free nodes.
//
// ## LRU eviction
//
// Every cached chunk has a `last_accessed` frame stamp. When total cached mesh
// bytes exceed `memory_budget`, the least-recently-accessed chunk NOT in the
// visible set is evicted (mesh data freed). GDScript frees the corresponding
// MeshInstance3D node.
//
// ## Dirty chunk deferral
//
// `update_dirty()` only regenerates dirty chunks in the visible set. Non-visible
// dirty chunks stay in the dirty set and are rebuilt when they next enter
// visibility.
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

use elven_canopy_sim::mesh_gen::{
    CHUNK_SIZE, ChunkCoord, ChunkMesh, generate_chunk_mesh, produces_geometry, voxel_to_chunk,
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
        let dx = (px - px.clamp(self.min[0], self.max[0])).abs();
        let dz = (pz - pz.clamp(self.min[2], self.max[2])).abs();
        dx * dx + dz * dz
    }

    /// Returns true if this AABB is entirely outside the frustum defined by
    /// the given planes. Each plane is `[nx, ny, nz, d]` where a point is
    /// inside the half-space when `nx*x + ny*y + nz*z - d >= 0` (Godot
    /// convention: `Plane.normal.dot(point) >= Plane.d`).
    pub fn is_outside_frustum(&self, planes: &[[f32; 4]]) -> bool {
        for plane in planes {
            let [nx, ny, nz, d] = *plane;
            // P-vertex: the corner of the AABB most in the direction of the
            // plane normal. If even the p-vertex is outside, the whole AABB is.
            let px = if nx >= 0.0 { self.max[0] } else { self.min[0] };
            let py = if ny >= 0.0 { self.max[1] } else { self.min[1] };
            let pz = if nz >= 0.0 { self.max[2] } else { self.min[2] };
            if nx * px + ny * py + nz * pz - d < 0.0 {
                return true;
            }
        }
        false
    }
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
// MeshCache
// ---------------------------------------------------------------------------

/// Caches chunk meshes with MegaChunk spatial hierarchy, draw-distance
/// culling, frustum culling, lazy mesh generation, and LRU eviction.
///
/// Also supports an optional Y cutoff for the height-hiding feature.
pub struct MeshCache {
    /// Cached mesh data per chunk (keyed by ChunkCoord).
    chunks: BTreeMap<ChunkCoord, ChunkMesh>,
    /// Chunks that need regeneration (dirty tracking from voxel edits).
    dirty: BTreeSet<ChunkCoord>,
    /// Chunks updated in the most recent `update_dirty()` call.
    last_updated: Vec<ChunkCoord>,
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
    /// Chunks that passed visibility but couldn't be generated this frame
    /// (max_gen_per_frame exceeded). Will be retried next frame.
    pending_gen: BTreeSet<ChunkCoord>,

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
    /// Maximum number of chunk meshes to generate per `update_visibility()` call.
    max_gen_per_frame: usize,
}

impl MeshCache {
    pub fn new() -> Self {
        Self {
            chunks: BTreeMap::new(),
            dirty: BTreeSet::new(),
            last_updated: Vec::new(),
            y_cutoff: None,
            tiling_cache: TilingCache::new(),
            cx_max: 0,
            cy_max: 0,
            cz_max: 0,
            megachunks: BTreeMap::new(),
            visible_set: BTreeSet::new(),
            chunks_to_show: Vec::new(),
            chunks_to_hide: Vec::new(),
            chunks_generated: Vec::new(),
            chunks_evicted: Vec::new(),
            pending_gen: BTreeSet::new(),
            lru_stamps: BTreeMap::new(),
            chunk_bytes: BTreeMap::new(),
            total_cached_bytes: 0,
            frame_counter: 0,
            draw_distance_voxels: 0.0,
            memory_budget: 0,
            max_gen_per_frame: 8,
        }
    }

    // -- Configuration setters --

    pub fn set_draw_distance(&mut self, voxels: f32) {
        self.draw_distance_voxels = voxels;
    }

    pub fn set_memory_budget(&mut self, bytes: usize) {
        self.memory_budget = bytes;
    }

    #[cfg(test)]
    pub fn set_max_gen_per_frame(&mut self, n: usize) {
        self.max_gen_per_frame = n;
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
        self.last_updated.clear();
        self.megachunks.clear();
        self.visible_set.clear();
        self.pending_gen.clear();
        self.lru_stamps.clear();
        self.chunk_bytes.clear();
        self.total_cached_bytes = 0;

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
    pub fn build_all(&mut self, world: &VoxelWorld) {
        self.scan_nonempty_chunks(world);

        // Generate all meshes eagerly (no visibility filtering).
        let all_chunks: Vec<ChunkCoord> = self
            .megachunks
            .values()
            .flat_map(|mc| mc.chunks.iter().copied())
            .collect();
        for coord in all_chunks {
            let mesh = generate_chunk_mesh(world, coord, self.y_cutoff);
            if !mesh.is_empty() {
                let bytes = mesh.estimate_byte_size();
                self.total_cached_bytes += bytes;
                self.chunk_bytes.insert(coord, bytes);
                self.chunks.insert(coord, mesh);
                self.visible_set.insert(coord);
            }
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
    ) -> usize {
        self.frame_counter += 1;
        self.chunks_to_show.clear();
        self.chunks_to_hide.clear();
        self.chunks_generated.clear();
        self.chunks_evicted.clear();

        let draw_dist_sq = if self.draw_distance_voxels > 0.0 {
            self.draw_distance_voxels * self.draw_distance_voxels
        } else {
            f32::MAX
        };

        let mut new_visible: BTreeSet<ChunkCoord> = BTreeSet::new();
        let mut gen_count: usize = 0;

        // Include any chunks still pending from last frame.
        let mut pending_this_frame: BTreeSet<ChunkCoord> = std::mem::take(&mut self.pending_gen);

        // Coarse pass: MegaChunk draw-distance + frustum.
        for mega in self.megachunks.values() {
            let aabb = match mega.aabb {
                Some(a) => a,
                None => continue,
            };

            // Draw distance (XZ only).
            if aabb.horizontal_distance_sq(cam_pos[0], cam_pos[2]) > draw_dist_sq {
                continue;
            }

            // Frustum cull (coarse).
            if frustum_planes.len() >= 6 && aabb.is_outside_frustum(frustum_planes) {
                continue;
            }

            // Fine pass: per-chunk draw-distance + frustum test.
            for &chunk_coord in &mega.chunks {
                let chunk_aabb = Aabb3::from_chunk(chunk_coord);

                // Per-chunk draw distance (the coarse megachunk test may pass
                // even when individual chunks within it are out of range).
                if chunk_aabb.horizontal_distance_sq(cam_pos[0], cam_pos[2]) > draw_dist_sq {
                    continue;
                }

                if frustum_planes.len() >= 6 && chunk_aabb.is_outside_frustum(frustum_planes) {
                    continue;
                }

                new_visible.insert(chunk_coord);

                // Ensure mesh exists.
                if self.chunks.contains_key(&chunk_coord) {
                    // Touch LRU.
                    self.lru_stamps.insert(chunk_coord, self.frame_counter);
                } else {
                    // Need to generate. Add to pending; we'll generate up to the cap.
                    pending_this_frame.insert(chunk_coord);
                }
            }
        }

        // Generate pending meshes up to the per-frame cap.
        // Prioritise chunks that are closest to the camera (sort by distance).
        let mut pending_sorted: Vec<ChunkCoord> = pending_this_frame.iter().copied().collect();
        pending_sorted.sort_by(|a, b| {
            let da = chunk_distance_sq(*a, cam_pos);
            let db = chunk_distance_sq(*b, cam_pos);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });

        self.pending_gen.clear();
        for coord in pending_sorted {
            if !new_visible.contains(&coord) {
                // No longer visible (camera moved between frames). Drop it.
                continue;
            }
            if gen_count >= self.max_gen_per_frame {
                self.pending_gen.insert(coord);
                continue;
            }

            // If dirty, it will be regenerated fresh here, so remove from dirty.
            self.dirty.remove(&coord);

            let mesh = generate_chunk_mesh(world, coord, self.y_cutoff);
            if !mesh.is_empty() {
                let bytes = mesh.estimate_byte_size();
                self.total_cached_bytes += bytes;
                self.chunk_bytes.insert(coord, bytes);
                self.lru_stamps.insert(coord, self.frame_counter);
                self.chunks.insert(coord, mesh);
                self.chunks_generated.push(coord);
                gen_count += 1;
            } else {
                // Scan said it was non-empty but mesh gen disagrees (e.g. due
                // to y_cutoff). Don't show it.
                new_visible.remove(&coord);
            }
        }

        // Diff: compute show/hide lists.
        for &coord in &new_visible {
            if !self.visible_set.contains(&coord) && self.chunks.contains_key(&coord) {
                self.chunks_to_show.push(coord);
            }
        }
        for &coord in &self.visible_set {
            if !new_visible.contains(&coord) {
                self.chunks_to_hide.push(coord);
            }
        }

        // The generated list is also a subset of show.
        // (chunks_generated were just created, so they definitely need showing.)
        // They are already in chunks_to_show from the diff above.

        self.visible_set = new_visible;

        // LRU eviction.
        if self.memory_budget > 0 {
            self.evict_lru();
        }

        gen_count
    }

    /// Evict least-recently-accessed chunks until under memory budget.
    fn evict_lru(&mut self) {
        while self.total_cached_bytes > self.memory_budget {
            // Find the chunk with the oldest stamp that is NOT visible.
            let victim = self
                .lru_stamps
                .iter()
                .filter(|(coord, _)| !self.visible_set.contains(coord))
                .min_by_key(|&(_, &stamp)| stamp)
                .map(|(&coord, _)| coord);

            match victim {
                Some(coord) => {
                    self.lru_stamps.remove(&coord);
                    self.chunks.remove(&coord);
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

            let local_x = coord.x.rem_euclid(CHUNK_SIZE);
            let local_y = coord.y.rem_euclid(CHUNK_SIZE);
            let local_z = coord.z.rem_euclid(CHUNK_SIZE);

            if local_x == 0 {
                self.dirty
                    .insert(ChunkCoord::new(chunk.cx - 1, chunk.cy, chunk.cz));
            }
            if local_x == CHUNK_SIZE - 1 {
                self.dirty
                    .insert(ChunkCoord::new(chunk.cx + 1, chunk.cy, chunk.cz));
            }
            if local_y == 0 {
                self.dirty
                    .insert(ChunkCoord::new(chunk.cx, chunk.cy - 1, chunk.cz));
            }
            if local_y == CHUNK_SIZE - 1 {
                self.dirty
                    .insert(ChunkCoord::new(chunk.cx, chunk.cy + 1, chunk.cz));
            }
            if local_z == 0 {
                self.dirty
                    .insert(ChunkCoord::new(chunk.cx, chunk.cy, chunk.cz - 1));
            }
            if local_z == CHUNK_SIZE - 1 {
                self.dirty
                    .insert(ChunkCoord::new(chunk.cx, chunk.cy, chunk.cz + 1));
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

    /// Regenerate dirty chunks that are in the visible set. Returns the number
    /// of chunks updated. Non-visible dirty chunks remain dirty and will be
    /// rebuilt when they enter visibility.
    pub fn update_dirty(&mut self, world: &VoxelWorld) -> usize {
        // Only process dirty chunks that are currently visible.
        let visible_dirty: Vec<ChunkCoord> = self
            .dirty
            .iter()
            .copied()
            .filter(|c| self.visible_set.contains(c))
            .collect();
        for &coord in &visible_dirty {
            self.dirty.remove(&coord);
        }
        self.last_updated.clear();

        for coord in &visible_dirty {
            // Remove old byte count.
            if let Some(old_bytes) = self.chunk_bytes.remove(coord) {
                self.total_cached_bytes = self.total_cached_bytes.saturating_sub(old_bytes);
            }

            let mesh = generate_chunk_mesh(world, *coord, self.y_cutoff);
            if mesh.is_empty() {
                self.chunks.remove(coord);
                self.lru_stamps.remove(coord);
                // Remove from megachunk.
                let mega_coord = chunk_to_mega(*coord);
                if let Some(mc) = self.megachunks.get_mut(&mega_coord) {
                    mc.remove_chunk(coord);
                }
            } else {
                let bytes = mesh.estimate_byte_size();
                self.total_cached_bytes += bytes;
                self.chunk_bytes.insert(*coord, bytes);
                self.lru_stamps.insert(*coord, self.frame_counter);
                self.chunks.insert(*coord, mesh);
            }
            self.last_updated.push(*coord);
        }

        self.last_updated.len()
    }

    // -- Accessors (existing API, preserved) --

    /// Get all non-empty chunk coordinates (all cached chunks).
    pub fn chunk_coords(&self) -> Vec<ChunkCoord> {
        self.chunks.keys().copied().collect()
    }

    /// Get the chunk coordinates that were updated in the last `update_dirty()`.
    pub fn last_updated_coords(&self) -> &[ChunkCoord] {
        &self.last_updated
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
                    self.dirty.insert(ChunkCoord::new(cx, cy, cz));
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

    /// A frustum that accepts everything (planes far away).
    fn open_frustum() -> Vec<[f32; 4]> {
        vec![
            [1.0, 0.0, 0.0, -1000.0],  // left
            [-1.0, 0.0, 0.0, -1000.0], // right
            [0.0, 1.0, 0.0, -1000.0],  // bottom
            [0.0, -1.0, 0.0, -1000.0], // top
            [0.0, 0.0, 1.0, -1000.0],  // near
            [0.0, 0.0, -1.0, -1000.0], // far
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
        // Plane: +X normal, d=100. Everything must have x >= 100.
        // AABB max x = 16 < 100 → outside.
        let planes = vec![
            [1.0, 0.0, 0.0, 100.0],
            [-1.0, 0.0, 0.0, -1000.0],
            [0.0, 1.0, 0.0, -1000.0],
            [0.0, -1.0, 0.0, -1000.0],
            [0.0, 0.0, 1.0, -1000.0],
            [0.0, 0.0, -1.0, -1000.0],
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
        // Plane: +X normal, d=0. Half the AABB is inside.
        let planes = vec![
            [1.0, 0.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, -1000.0],
            [0.0, 1.0, 0.0, -1000.0],
            [0.0, -1.0, 0.0, -1000.0],
            [0.0, 0.0, 1.0, -1000.0],
            [0.0, 0.0, -1.0, -1000.0],
        ];
        assert!(!aabb.is_outside_frustum(&planes));
    }

    // -- MegaChunk add/remove --

    #[test]
    fn megachunk_add_chunks_expands_aabb() {
        let mut mc = MegaChunk::new(MegaChunkCoord::new(0, 0));
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
        let mut mc = MegaChunk::new(MegaChunkCoord::new(0, 0));
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
        let mut mc = MegaChunk::new(MegaChunkCoord::new(0, 0));
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
    fn scan_ignores_air_and_forest_floor() {
        let mut world = VoxelWorld::new(16, 16, 16);
        // ForestFloor doesn't produce geometry.
        world.set(VoxelCoord::new(8, 0, 8), VoxelType::ForestFloor);
        // But a trunk does.
        world.set(VoxelCoord::new(8, 1, 8), VoxelType::Trunk);
        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);

        // Should find the chunk because of the trunk.
        assert_eq!(cache.megachunks.len(), 1);
    }

    // -- estimate_byte_size --

    #[test]
    fn estimate_byte_size_via_build() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mut cache = MeshCache::new();
        cache.build_all(&world);

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
        cache.set_max_gen_per_frame(100);

        let cam_pos = [8.0, 8.0, 8.0];
        cache.update_visibility(&world, cam_pos, &open_frustum());

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
        cache.set_max_gen_per_frame(100);

        let cam_pos = [8.0, 8.0, 8.0];
        cache.update_visibility(&world, cam_pos, &open_frustum());

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
        cache.set_max_gen_per_frame(100);

        // Frustum: only accept z > 100 (everything else behind camera).
        let planes = vec![
            [1.0, 0.0, 0.0, -1000.0],
            [-1.0, 0.0, 0.0, -1000.0],
            [0.0, 1.0, 0.0, -1000.0],
            [0.0, -1.0, 0.0, -1000.0],
            [0.0, 0.0, 1.0, 100.0], // z must be >= 100
            [0.0, 0.0, -1.0, -1000.0],
        ];
        let cam_pos = [128.0, 8.0, 128.0];
        cache.update_visibility(&world, cam_pos, &planes);

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
        cache.set_max_gen_per_frame(100);

        assert!(cache.chunks.is_empty()); // No meshes yet.

        let gen_count = cache.update_visibility(&world, [8.0, 8.0, 8.0], &open_frustum());

        assert_eq!(gen_count, 1);
        assert!(cache.chunks.contains_key(&ChunkCoord::new(0, 0, 0)));
        assert_eq!(cache.chunks_generated.len(), 1);
    }

    // -- max_gen_per_frame --

    #[test]
    fn visibility_respects_max_gen_per_frame() {
        let mut world = VoxelWorld::new(48, 16, 16);
        // 3 chunks with geometry.
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(24, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(40, 8, 8), VoxelType::Trunk);

        let mut cache = MeshCache::new();
        cache.scan_nonempty_chunks(&world);
        cache.set_max_gen_per_frame(2); // Only 2 per frame.

        let gen1 = cache.update_visibility(&world, [24.0, 8.0, 8.0], &open_frustum());
        assert_eq!(gen1, 2);
        assert_eq!(cache.pending_gen.len(), 1); // 1 left pending.

        let gen2 = cache.update_visibility(&world, [24.0, 8.0, 8.0], &open_frustum());
        assert_eq!(gen2, 1);
        assert!(cache.pending_gen.is_empty());
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
        cache.set_max_gen_per_frame(100);

        // Frame 1: camera near chunk 0.
        cache.update_visibility(&world, [8.0, 8.0, 8.0], &open_frustum());
        assert_eq!(cache.chunks_to_show.len(), 1);
        assert!(cache.chunks_to_hide.is_empty());

        // Frame 2: camera moves to chunk 12.
        cache.update_visibility(&world, [200.0, 8.0, 8.0], &open_frustum());
        // Chunk 0 should hide, chunk 12 should show.
        assert!(cache.chunks_to_hide.contains(&ChunkCoord::new(0, 0, 0)));
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
        cache.set_max_gen_per_frame(100);

        // Generate all.
        cache.update_visibility(&world, [100.0, 8.0, 8.0], &open_frustum());
        assert_eq!(cache.chunks.len(), 2);
        let total = cache.total_cached_bytes;

        // Set budget to roughly half — but both are visible, so no eviction.
        cache.set_memory_budget(total / 2);
        cache.update_visibility(&world, [100.0, 8.0, 8.0], &open_frustum());
        assert!(cache.chunks_evicted.is_empty()); // Can't evict visible chunks.

        // Now make only chunk 0 visible (move camera close to it).
        cache.set_draw_distance(50.0);
        cache.update_visibility(&world, [8.0, 8.0, 8.0], &open_frustum());
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
        cache.set_max_gen_per_frame(100);
        cache.set_memory_budget(1); // Tiny budget.

        cache.update_visibility(&world, [8.0, 8.0, 8.0], &open_frustum());

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
        cache.set_max_gen_per_frame(100);
        cache.update_visibility(&world, [8.0, 8.0, 8.0], &open_frustum());

        // Mark dirty and add a voxel.
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Trunk);
        cache.mark_dirty_voxels(&[VoxelCoord::new(9, 8, 8)]);

        let updated = cache.update_dirty(&world);
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
        cache.set_max_gen_per_frame(100);

        // Only chunk 0 visible.
        cache.update_visibility(&world, [8.0, 8.0, 8.0], &open_frustum());

        // Dirty chunk 12 (not visible).
        cache.mark_dirty_voxels(&[VoxelCoord::new(200, 8, 8)]);
        let updated = cache.update_dirty(&world);
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
        cache.set_max_gen_per_frame(100);

        // Only chunk 0 visible.
        cache.update_visibility(&world, [8.0, 8.0, 8.0], &open_frustum());

        // Dirty chunk 12 (not visible) and modify the world.
        world.set(VoxelCoord::new(201, 8, 8), VoxelType::Trunk);
        cache.mark_dirty_voxels(&[VoxelCoord::new(201, 8, 8)]);

        // Now move camera to chunk 12.
        cache.update_visibility(&world, [200.0, 8.0, 8.0], &open_frustum());

        // update_visibility generated the mesh fresh (dirty chunk is removed
        // from dirty set during on-demand generation).
        assert!(cache.visible_set.contains(&ChunkCoord::new(12, 0, 0)));
        assert!(!cache.dirty.contains(&ChunkCoord::new(12, 0, 0)));
    }
}
