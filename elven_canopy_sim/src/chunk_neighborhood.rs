// Self-contained voxel snapshot for one chunk's mesh generation.
//
// A `ChunkNeighborhood` captures the voxel data needed to generate a single
// chunk's mesh: the chunk's own 16x16x16 region plus a border for cross-chunk
// smoothing. It is extracted from `VoxelWorld` on the main thread (fast — just
// copying voxel types and RLE spans), then sent to a background worker for
// mesh generation with no further world access.
//
// This is the core isolation mechanism for off-main-thread mesh generation:
// workers operate on owned `ChunkNeighborhood` values, not shared references
// to the world, so no locks are held during the expensive smoothing/decimation
// passes.
//
// See also: `mesh_gen.rs` for the mesh generation algorithm that consumes this,
// `world.rs` for the `VoxelWorld` that produces it, `mesh_cache.rs` (gdext
// crate) for the async pipeline that coordinates extraction and generation.

use std::collections::BTreeSet;

use crate::mesh_gen::{CHUNK_SIZE, ChunkCoord, SMOOTH_BORDER};
use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;

/// Border size around the chunk needed for smooth mesh cross-boundary
/// consistency. Re-exported from `mesh_gen::SMOOTH_BORDER` so both modules
/// stay in sync at compile time.
const BORDER: i32 = SMOOTH_BORDER;

/// A self-contained snapshot of the voxel data surrounding one chunk,
/// sufficient to generate its mesh without access to the full `VoxelWorld`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChunkNeighborhood {
    /// The chunk this neighborhood was extracted for.
    pub chunk: ChunkCoord,
    /// Sim tick at the time of extraction (for freshness checks).
    pub sim_tick: u64,
    /// Y cutoff for height hiding (voxels at or above this Y are treated as air).
    pub y_cutoff: Option<i32>,

    /// World dimensions (needed for border clamping in mesh_gen).
    pub world_size_x: u32,
    pub world_size_y: u32,
    pub world_size_z: u32,

    // -- Voxel data for point lookups (get) --
    /// Origin corner (minimum x, y, z) of the captured region in voxel coords.
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    /// Dimensions of the captured region.
    extent_x: i32,
    extent_y: i32,
    extent_z: i32,
    /// Flat voxel array, indexed as `[((z - origin_z) * extent_x + (x - origin_x)) * extent_y + (y - origin_y)]`.
    voxels: Vec<VoxelType>,

    // -- Column span data for the smooth mesh pass --
    /// Flat packed column spans: each entry is `(VoxelType, y_start, y_end)`.
    /// Columns are stored contiguously; `span_index` maps each column to its
    /// range within this vec.
    spans: Vec<(VoxelType, u8, u8)>,
    /// Index into `spans` for each column in the captured XZ region.
    /// Column `(x, z)` maps to index `(z - col_origin_z) * col_extent_x + (x - col_origin_x)`.
    /// Each entry is `(offset, length)` into the `spans` vec.
    span_index: Vec<(u32, u16)>,
    /// Origin and extent of the column region (may differ from voxel region
    /// due to clamping — columns use unsigned coords).
    col_origin_x: i32,
    col_origin_z: i32,
    col_extent_x: i32,

    /// Grassless voxels within the captured region.
    pub grassless: BTreeSet<VoxelCoord>,
}

impl ChunkNeighborhood {
    /// Extract a neighborhood from the world for the given chunk.
    ///
    /// This copies voxel data for the chunk's region plus a `BORDER`-voxel
    /// margin in each direction. Designed to be called on the main thread
    /// while holding read access to the world; the resulting value is then
    /// sent to a background worker.
    pub fn extract(
        world: &VoxelWorld,
        chunk: ChunkCoord,
        y_cutoff: Option<i32>,
        grassless: &BTreeSet<VoxelCoord>,
    ) -> Self {
        let base_x = chunk.cx * CHUNK_SIZE;
        let base_y = chunk.cy * CHUNK_SIZE;
        let base_z = chunk.cz * CHUNK_SIZE;

        // Capture region with border, clamped to world bounds.
        let min_x = (base_x - BORDER).max(0);
        let min_y = (base_y - BORDER).max(0);
        let min_z = (base_z - BORDER).max(0);
        let max_x = (base_x + CHUNK_SIZE + BORDER).min(world.size_x as i32);
        let max_y = (base_y + CHUNK_SIZE + BORDER).min(world.size_y as i32);
        let max_z = (base_z + CHUNK_SIZE + BORDER).min(world.size_z as i32);

        let extent_x = max_x - min_x;
        let extent_y = max_y - min_y;
        let extent_z = max_z - min_z;

        // Copy voxels into a flat array.
        let total = (extent_x * extent_y * extent_z) as usize;
        let mut voxels = vec![VoxelType::Air; total];
        for z in min_z..max_z {
            for x in min_x..max_x {
                for (vt, y_start, y_end) in world.column_spans(x as u32, z as u32) {
                    let y_lo = (y_start as i32).max(min_y);
                    let y_hi = (y_end as i32).min(max_y - 1);
                    for y in y_lo..=y_hi {
                        let idx = ((z - min_z) * extent_x + (x - min_x)) * extent_y + (y - min_y);
                        voxels[idx as usize] = vt;
                    }
                }
            }
        }

        // Copy column spans into flat packed storage.
        // The column region for column_spans uses the same XZ range as the
        // voxel capture region (columns are indexed by unsigned x, z and the
        // min values are already >= 0).
        let col_origin_x = min_x;
        let col_origin_z = min_z;
        let col_extent_x = extent_x;
        let col_extent_z = extent_z;
        let num_cols = (col_extent_x * col_extent_z) as usize;
        let mut spans = Vec::new();
        let mut span_index = Vec::with_capacity(num_cols);

        for z in min_z..max_z {
            for x in min_x..max_x {
                let offset = spans.len() as u32;
                for span in world.column_spans(x as u32, z as u32) {
                    spans.push(span);
                }
                let length = (spans.len() as u32 - offset) as u16;
                span_index.push((offset, length));
            }
        }

        // Extract the subset of grassless voxels within the capture region.
        // VoxelCoord's Ord is lexicographic (x, y, z), so the range covers
        // all coords with x in [min_x, max_x). The filter ensures y and z
        // are also within bounds.
        let local_grassless: BTreeSet<VoxelCoord> = grassless
            .range(
                VoxelCoord::new(min_x, i32::MIN, i32::MIN)
                    ..VoxelCoord::new(max_x, i32::MIN, i32::MIN),
            )
            .copied()
            .filter(|c| c.y >= min_y && c.y < max_y && c.z >= min_z && c.z < max_z)
            .collect();

        Self {
            chunk,
            sim_tick: world.sim_tick,
            y_cutoff,
            world_size_x: world.size_x,
            world_size_y: world.size_y,
            world_size_z: world.size_z,
            origin_x: min_x,
            origin_y: min_y,
            origin_z: min_z,
            extent_x,
            extent_y,
            extent_z,
            voxels,
            spans,
            span_index,
            col_origin_x,
            col_origin_z,
            col_extent_x,
            grassless: local_grassless,
        }
    }

    /// Look up a voxel type at the given world coordinate.
    /// Returns `Air` for coordinates outside the captured region (matching
    /// `VoxelWorld::get()` behavior for out-of-bounds coords).
    pub fn get(&self, coord: VoxelCoord) -> VoxelType {
        let lx = coord.x - self.origin_x;
        let ly = coord.y - self.origin_y;
        let lz = coord.z - self.origin_z;
        if lx < 0
            || ly < 0
            || lz < 0
            || lx >= self.extent_x
            || ly >= self.extent_y
            || lz >= self.extent_z
        {
            return VoxelType::Air;
        }
        let idx = (lz * self.extent_x + lx) * self.extent_y + ly;
        self.voxels[idx as usize]
    }

    /// Iterate the column spans for the given world coordinate.
    /// The column at `(x, z)` must be within the captured region.
    ///
    /// Returns an iterator yielding `(VoxelType, y_start, y_end)` triples
    /// that fully tile the column, matching `VoxelWorld::column_spans()`.
    pub fn column_spans(&self, x: u32, z: u32) -> impl Iterator<Item = (VoxelType, u8, u8)> + '_ {
        let lx = x as i32 - self.col_origin_x;
        let lz = z as i32 - self.col_origin_z;
        let col_idx = (lz * self.col_extent_x + lx) as usize;
        let (offset, length) = self.span_index[col_idx];
        self.spans[offset as usize..(offset as usize + length as usize)]
            .iter()
            .copied()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a small world (one chunk = 16x16x16).
    fn one_chunk_world() -> VoxelWorld {
        VoxelWorld::new(16, 16, 16)
    }

    #[test]
    fn extract_empty_world() {
        let world = one_chunk_world();
        let nh =
            ChunkNeighborhood::extract(&world, ChunkCoord::new(0, 0, 0), None, &BTreeSet::new());
        // All voxels should be Air.
        for z in 0..16 {
            for x in 0..16 {
                for y in 0..16 {
                    assert_eq!(nh.get(VoxelCoord::new(x, y, z)), VoxelType::Air);
                }
            }
        }
    }

    #[test]
    fn extract_preserves_voxels() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(5, 5, 5), VoxelType::Trunk);
        world.set(VoxelCoord::new(10, 3, 7), VoxelType::Dirt);

        let nh =
            ChunkNeighborhood::extract(&world, ChunkCoord::new(0, 0, 0), None, &BTreeSet::new());

        assert_eq!(nh.get(VoxelCoord::new(5, 5, 5)), VoxelType::Trunk);
        assert_eq!(nh.get(VoxelCoord::new(10, 3, 7)), VoxelType::Dirt);
        assert_eq!(nh.get(VoxelCoord::new(0, 0, 0)), VoxelType::Air);
    }

    #[test]
    fn out_of_bounds_returns_air() {
        let world = one_chunk_world();
        let nh =
            ChunkNeighborhood::extract(&world, ChunkCoord::new(0, 0, 0), None, &BTreeSet::new());
        assert_eq!(nh.get(VoxelCoord::new(-1, 0, 0)), VoxelType::Air);
        assert_eq!(nh.get(VoxelCoord::new(20, 0, 0)), VoxelType::Air);
    }

    #[test]
    fn column_spans_match_world() {
        let mut world = VoxelWorld::new(32, 32, 32);
        // Build a column with multiple span types.
        for y in 0..5 {
            world.set(VoxelCoord::new(8, y, 8), VoxelType::Dirt);
        }
        for y in 5..10 {
            world.set(VoxelCoord::new(8, y, 8), VoxelType::Trunk);
        }

        let nh =
            ChunkNeighborhood::extract(&world, ChunkCoord::new(0, 0, 0), None, &BTreeSet::new());

        let world_spans: Vec<_> = world.column_spans(8, 8).collect();
        let nh_spans: Vec<_> = nh.column_spans(8, 8).collect();
        assert_eq!(world_spans, nh_spans);
    }

    #[test]
    fn grassless_subset_filtered() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(3, 3, 3), VoxelType::Dirt);

        let mut grassless = BTreeSet::new();
        grassless.insert(VoxelCoord::new(3, 3, 3)); // in region
        grassless.insert(VoxelCoord::new(50, 50, 50)); // out of region

        let nh = ChunkNeighborhood::extract(&world, ChunkCoord::new(0, 0, 0), None, &grassless);

        assert!(nh.grassless.contains(&VoxelCoord::new(3, 3, 3)));
        assert!(!nh.grassless.contains(&VoxelCoord::new(50, 50, 50)));
    }

    #[test]
    fn sim_tick_captured() {
        let mut world = one_chunk_world();
        world.sim_tick = 42;
        let nh =
            ChunkNeighborhood::extract(&world, ChunkCoord::new(0, 0, 0), None, &BTreeSet::new());
        assert_eq!(nh.sim_tick, 42);
    }

    #[test]
    fn y_cutoff_stored() {
        let world = one_chunk_world();
        let nh = ChunkNeighborhood::extract(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(10),
            &BTreeSet::new(),
        );
        assert_eq!(nh.y_cutoff, Some(10));
    }

    #[test]
    fn border_captures_neighbor_chunk_voxels() {
        // World with 2 chunks in X. Place a voxel at x=17 (chunk 1),
        // which should be captured by chunk 0's border (extends to x=18).
        let mut world = VoxelWorld::new(32, 16, 16);
        world.set(VoxelCoord::new(17, 8, 8), VoxelType::Branch);

        let nh =
            ChunkNeighborhood::extract(&world, ChunkCoord::new(0, 0, 0), None, &BTreeSet::new());

        assert_eq!(nh.get(VoxelCoord::new(17, 8, 8)), VoxelType::Branch);
    }

    #[test]
    fn neighborhood_mesh_matches_world_mesh() {
        // Verify that generating a mesh via ChunkNeighborhood produces
        // identical output to the convenience wrapper that reads the world
        // directly. This is the core equivalence guarantee of the feature.
        use crate::mesh_gen::{
            generate_chunk_mesh, generate_chunk_mesh_from_world, set_decimation_enabled,
        };

        set_decimation_enabled(false);
        let mut world = VoxelWorld::new(32, 16, 32);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Branch);
        world.set(VoxelCoord::new(8, 0, 8), VoxelType::Dirt);

        let chunk = ChunkCoord::new(0, 0, 0);
        let grassless = BTreeSet::new();

        // Path A: world → generate_chunk_mesh_from_world (extracts + generates).
        let mesh_a = generate_chunk_mesh_from_world(&world, chunk, None, &grassless);

        // Path B: manual extract → generate_chunk_mesh.
        let nh = ChunkNeighborhood::extract(&world, chunk, None, &grassless);
        let mesh_b = generate_chunk_mesh(&nh);

        // Meshes must be bit-identical.
        assert_eq!(mesh_a.bark.vertices, mesh_b.bark.vertices);
        assert_eq!(mesh_a.bark.normals, mesh_b.bark.normals);
        assert_eq!(mesh_a.bark.indices, mesh_b.bark.indices);
        assert_eq!(mesh_a.bark.colors, mesh_b.bark.colors);
        assert_eq!(mesh_a.ground.vertices, mesh_b.ground.vertices);
        assert_eq!(mesh_a.ground.indices, mesh_b.ground.indices);
        assert_eq!(mesh_a.leaf.vertices, mesh_b.leaf.vertices);
        assert_eq!(mesh_a.leaf.indices, mesh_b.leaf.indices);
    }
}
