// Chunk mesh cache for the gdext bridge.
//
// Sits between the sim's pure `mesh_gen` module and the Godot-facing
// `sim_bridge.rs`. Caches `ChunkMesh` data per chunk and tracks which
// chunks are dirty (need regeneration). The bridge calls `build_all()`
// once at init, then `mark_dirty_voxels()` + `update_dirty()` each frame
// for incremental updates.
//
// See also: `mesh_gen.rs` (sim crate) for the core mesh generation algorithm,
// `sim_bridge.rs` for the Godot-facing methods that convert `ChunkMesh` into
// `ArrayMesh` objects.

use std::collections::{BTreeMap, BTreeSet};

use elven_canopy_sim::mesh_gen::{
    CHUNK_SIZE, ChunkCoord, ChunkMesh, generate_chunk_mesh, voxel_to_chunk,
};
use elven_canopy_sim::types::VoxelCoord;
use elven_canopy_sim::world::VoxelWorld;

/// Caches chunk meshes and tracks dirty chunks for incremental updates.
pub struct MeshCache {
    /// Cached mesh data per non-empty chunk.
    chunks: BTreeMap<ChunkCoord, ChunkMesh>,
    /// Chunks that need regeneration.
    dirty: BTreeSet<ChunkCoord>,
    /// Chunks updated in the most recent `update_dirty()` call. Used by the
    /// bridge to know which `MeshInstance3D` nodes to rebuild.
    last_updated: Vec<ChunkCoord>,
}

impl MeshCache {
    pub fn new() -> Self {
        Self {
            chunks: BTreeMap::new(),
            dirty: BTreeSet::new(),
            last_updated: Vec::new(),
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

        for cy in 0..cy_max {
            for cz in 0..cz_max {
                for cx in 0..cx_max {
                    let coord = ChunkCoord::new(cx, cy, cz);
                    let mesh = generate_chunk_mesh(world, coord);
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
            let mesh = generate_chunk_mesh(world, *coord);
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
}
