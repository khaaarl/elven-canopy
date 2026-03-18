// Chunk mesh cache for the gdext bridge.
//
// Sits between the sim's pure `mesh_gen` module and the Godot-facing
// `sim_bridge.rs`. Caches `ChunkMesh` data per chunk and tracks which
// chunks are dirty (need regeneration). The bridge calls `build_all()`
// once at init, then `mark_dirty_voxels()` + `update_dirty()` each frame
// for incremental updates.
//
// Also owns the `TilingCache` (global prime-period tiling textures) which
// is built once and shared across all chunks — it doesn't depend on world
// state, only on the noise parameters baked into `texture_gen.rs`.
//
// See also: `mesh_gen.rs` (sim crate) for the core mesh generation algorithm,
// `texture_gen.rs` (sim crate) for the tiling texture system,
// `sim_bridge.rs` for the Godot-facing methods that convert `ChunkMesh` into
// `ArrayMesh` objects and upload tiling textures.

use std::collections::{BTreeMap, BTreeSet};

use elven_canopy_sim::mesh_gen::{
    CHUNK_SIZE, ChunkCoord, ChunkMesh, generate_chunk_mesh, voxel_to_chunk,
};
use elven_canopy_sim::texture_gen::TilingCache;
use elven_canopy_sim::types::VoxelCoord;
use elven_canopy_sim::world::VoxelWorld;

/// Caches chunk meshes and tracks dirty chunks for incremental updates.
///
/// Supports an optional Y cutoff for the height-hiding feature: when set,
/// voxels at or above the cutoff Y are treated as air during mesh generation,
/// exposing boundary faces at the cut level. Changing the cutoff automatically
/// dirties the affected chunk rows.
pub struct MeshCache {
    /// Cached mesh data per non-empty chunk.
    chunks: BTreeMap<ChunkCoord, ChunkMesh>,
    /// Chunks that need regeneration.
    dirty: BTreeSet<ChunkCoord>,
    /// Chunks updated in the most recent `update_dirty()` call. Used by the
    /// bridge to know which `MeshInstance3D` nodes to rebuild.
    last_updated: Vec<ChunkCoord>,
    /// Optional Y cutoff for height hiding. Voxels with world Y ≥ this value
    /// produce no geometry, and their neighbors get boundary faces exposed.
    y_cutoff: Option<i32>,
    /// Global tiling texture cache. Built once, independent of world state.
    tiling_cache: TilingCache,
    /// World chunk bounds (set during build_all). Used by set_y_cutoff to dirty
    /// chunks that might not be in the cache yet.
    cx_max: i32,
    cy_max: i32,
    cz_max: i32,
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
        }
    }

    /// Full build: generate meshes for every chunk that might contain voxels.
    /// Skips all-air chunks (empty mesh). Called once at init.
    pub fn build_all(&mut self, world: &VoxelWorld) {
        self.chunks.clear();
        self.dirty.clear();
        self.last_updated.clear();

        let cx_max = (world.size_x as i32 + CHUNK_SIZE - 1) / CHUNK_SIZE;
        let cy_max = (world.size_y as i32 + CHUNK_SIZE - 1) / CHUNK_SIZE;
        let cz_max = (world.size_z as i32 + CHUNK_SIZE - 1) / CHUNK_SIZE;
        self.cx_max = cx_max;
        self.cy_max = cy_max;
        self.cz_max = cz_max;

        for cy in 0..cy_max {
            for cz in 0..cz_max {
                for cx in 0..cx_max {
                    let coord = ChunkCoord::new(cx, cy, cz);
                    let mesh = generate_chunk_mesh(world, coord, self.y_cutoff);
                    if !mesh.is_empty() {
                        self.chunks.insert(coord, mesh);
                    }
                }
            }
        }
    }

    /// Mark chunks as dirty based on a list of modified voxel coordinates.
    /// Also marks neighbor chunks when a voxel sits at a chunk boundary
    /// (local coord 0 or 15), since face culling depends on cross-chunk
    /// neighbor lookups.
    pub fn mark_dirty_voxels(&mut self, coords: &[VoxelCoord]) {
        for &coord in coords {
            let chunk = voxel_to_chunk(coord);
            self.dirty.insert(chunk);

            // Check chunk boundary: if the voxel is at the edge of its chunk,
            // the adjacent chunk's mesh may also need updating.
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
    }

    /// Regenerate all dirty chunks. Returns the number of chunks updated.
    /// The list of updated chunk coordinates is stored in `last_updated` for
    /// the bridge to query.
    pub fn update_dirty(&mut self, world: &VoxelWorld) -> usize {
        let dirty: Vec<ChunkCoord> = self.dirty.iter().copied().collect();
        self.dirty.clear();
        self.last_updated.clear();

        for coord in &dirty {
            let mesh = generate_chunk_mesh(world, *coord, self.y_cutoff);
            if mesh.is_empty() {
                self.chunks.remove(coord);
            } else {
                self.chunks.insert(*coord, mesh);
            }
            self.last_updated.push(*coord);
        }

        self.last_updated.len()
    }

    /// Get all non-empty chunk coordinates.
    pub fn chunk_coords(&self) -> Vec<ChunkCoord> {
        self.chunks.keys().copied().collect()
    }

    /// Get the chunk coordinates that were updated in the last `update_dirty()` call.
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

        // Determine the range of chunk Y-levels that need rebuilding.
        // When the cutoff moves, chunks between the old and new cutoff need
        // regeneration. When enabling/disabling, everything above the cutoff.
        let (cy_min, cy_max) = match (self.y_cutoff, new_cutoff) {
            (Some(old), Some(new)) => {
                // Cutoff moved — dirty chunks between old and new boundaries.
                let lo = (old.min(new) - 1).div_euclid(CHUNK_SIZE);
                let hi = old.max(new).div_euclid(CHUNK_SIZE);
                (lo, hi)
            }
            (Some(old), None) | (None, Some(old)) => {
                // Enabling or disabling — dirty everything from cutoff up.
                let lo = (old - 1).div_euclid(CHUNK_SIZE);
                (lo, self.cy_max - 1)
            }
            (None, None) => {
                self.y_cutoff = new_cutoff;
                return;
            }
        };
        // When enabling with a new cutoff, also dirty from the new cutoff up.
        let effective_max = if self.y_cutoff.is_none() || new_cutoff.is_none() {
            self.cy_max - 1
        } else {
            cy_max
        };

        // Dirty all chunks within the affected Y range across all X/Z.
        // Uses world bounds so we also catch chunks not currently in the cache
        // (they may produce geometry with the new cutoff).
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
