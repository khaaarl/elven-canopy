// RLE column-based voxel storage for the game world.
//
// Each (x, z) column stores a sorted list of `Span` entries, where each span
// describes a contiguous vertical run of a single voxel type. Columns are
// organized into 16×16 `ColumnGroup`s sharing XZ alignment with the mesh chunk
// grid. Each group has a single heap allocation (`Vec<Span>`) holding span data
// for all its columns, plus inline per-column metadata (`ColMeta`).
//
// The trailing Air span at the top of each column is omitted (implied). A
// column with 0 stored spans is entirely Air. Span `top_y` values are the
// highest Y coordinate (inclusive) in that run; spans are sorted ascending by
// `top_y`.
//
// `get()` does a linear search (≤6 spans) or binary search (>6 spans) on the
// column's spans. `set()` splits/merges spans in a scratch buffer then writes
// back, moving the column to the group's free tail if it outgrows its slot.
// Full group repacks happen only when the free tail is exhausted.
//
// Also provides `raycast_hits_solid()` (3D DDA, Amanatides & Woo),
// `has_los()` (LOS with transparent Leaf/Fruit), `heightmap()` (top-down
// max-solid-Y), and `has_solid_face_neighbor()` / `has_face_neighbor_of_type()`
// for adjacency queries. All use `get()` internally.
//
// The world is regenerated from seed at load time, so it skips serialization
// (`#[serde(skip)]` on `SimState.world`). After worldgen or load, call
// `repack_all()` to compact groups and reclaim dead space from bulk writes.
// The `Default` impl creates a zero-sized empty world; `SimState::new()`
// constructs the real one from `config.world_size`.
//
// See also: `tree_gen.rs` for populating the world with tree geometry,
// `nav.rs` for the navigation graph built on top of the voxel data,
// `sim/mod.rs` which owns the `VoxelWorld` as part of `SimState`,
// `docs/drafts/rle_voxels.md` for the full design document.
//
// **Critical constraint: determinism.** All world modifications must go
// through deterministic sim logic. No concurrent mutation, no random
// access from rendering threads.

use crate::types::{VoxelCoord, VoxelType};

/// Column group size in the XZ plane (must be a power of 2).
const GROUP_SIZE: u32 = 16;
const GROUP_SHIFT: u32 = 4; // log2(GROUP_SIZE)
const GROUP_MASK: u32 = GROUP_SIZE - 1;
/// Number of columns per group.
const COLS_PER_GROUP: usize = (GROUP_SIZE * GROUP_SIZE) as usize;

/// Threshold for switching from linear to binary search in `get()`.
const LINEAR_SEARCH_LIMIT: usize = 6;

/// If a group's `spans` vec would grow past this many entries, trigger an
/// emergency repack that allocates every column the maximum 255 spans. This
/// prevents `u16` overflow in `ColMeta::data_start` / `ColumnGroup::free_start`.
/// Set to half of u16::MAX to leave headroom.
const GROUP_OVERFLOW_THRESHOLD: u16 = 32_768;

/// A single span in a column: a contiguous vertical run of one voxel type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct Span {
    /// VoxelType discriminant (VoxelType is `#[repr(u8)]`).
    voxel_type: u8,
    /// Highest Y coordinate (inclusive) in this run.
    top_y: u8,
}

/// Per-column metadata, 4 bytes.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
struct ColMeta {
    /// Span index into the group's `spans` vec where this column starts.
    data_start: u16,
    /// Number of spans actually stored for this column.
    num_spans: u8,
    /// Number of span slots allocated (≥ num_spans).
    num_allocated: u8,
}

/// A 16×16 column group with a single heap allocation for span data.
#[derive(Clone, Debug)]
struct ColumnGroup {
    /// Per-column metadata. Index: local_x + local_z * GROUP_SIZE.
    cols: [ColMeta; COLS_PER_GROUP],
    /// Index into `spans` where unallocated free space begins.
    free_start: u16,
    /// Span data for all columns plus free tail space.
    spans: Vec<Span>,
}

impl Default for ColumnGroup {
    fn default() -> Self {
        Self {
            cols: [ColMeta::default(); COLS_PER_GROUP],
            free_start: 0,
            spans: Vec::new(),
        }
    }
}

impl ColumnGroup {
    /// Read the spans for a column (by local column index).
    fn col_spans(&self, col: usize) -> &[Span] {
        let meta = &self.cols[col];
        if meta.num_spans == 0 {
            return &[];
        }
        let start = meta.data_start as usize;
        &self.spans[start..start + meta.num_spans as usize]
    }

    /// Look up the voxel type at a given Y in a column.
    fn get_in_col(&self, col: usize, y: u8) -> VoxelType {
        let meta = &self.cols[col];
        let count = meta.num_spans as usize;
        if count == 0 {
            return VoxelType::Air;
        }
        let start = meta.data_start as usize;
        let spans = &self.spans[start..start + count];

        if count <= LINEAR_SEARCH_LIMIT {
            for span in spans {
                if y <= span.top_y {
                    return VoxelType::from_u8(span.voxel_type);
                }
            }
            VoxelType::Air // above all explicit spans (implicit trailing Air)
        } else {
            match spans.binary_search_by_key(&y, |s| s.top_y) {
                Ok(i) => VoxelType::from_u8(spans[i].voxel_type),
                Err(i) => {
                    if i < count {
                        VoxelType::from_u8(spans[i].voxel_type)
                    } else {
                        VoxelType::Air
                    }
                }
            }
        }
    }

    /// Set a voxel in a column. Returns true if the voxel actually changed.
    fn set_in_col(&mut self, col: usize, y: u8, voxel: VoxelType, max_y: u8) -> bool {
        // Read existing spans into a scratch buffer and compute new spans.
        let old_meta = self.cols[col];
        let old_count = old_meta.num_spans as usize;
        let old_start = old_meta.data_start as usize;

        // Build new span list in scratch buffer. Max possible spans after a
        // single-voxel edit: old_count + 2 (splitting one span into three).
        let mut scratch = [Span {
            voxel_type: 0,
            top_y: 0,
        }; 258]; // 255 max + 3 headroom
        let new_count = Self::compute_new_spans(
            if old_count > 0 {
                &self.spans[old_start..old_start + old_count]
            } else {
                &[]
            },
            y,
            voxel,
            max_y,
            &mut scratch,
        );

        // Check if anything actually changed.
        if new_count == old_count {
            let old_spans = if old_count > 0 {
                &self.spans[old_start..old_start + old_count]
            } else {
                &[][..]
            };
            if old_spans == &scratch[..new_count] {
                return false;
            }
        }

        // Write back the new spans.
        self.write_col_spans(col, &scratch[..new_count]);
        true
    }

    /// Compute the new span list after setting voxel at `y` to `voxel`.
    /// Returns the number of spans written to `out`.
    fn compute_new_spans(
        old_spans: &[Span],
        y: u8,
        voxel: VoxelType,
        _max_y: u8,
        out: &mut [Span],
    ) -> usize {
        let vt = voxel.to_u8();

        // If column is empty (all Air), and we're setting to Air, no-op.
        if old_spans.is_empty() && voxel == VoxelType::Air {
            return 0;
        }

        // If column is empty, setting a non-Air voxel.
        if old_spans.is_empty() {
            if y == 0 {
                // Single voxel at y=0.
                out[0] = Span {
                    voxel_type: vt,
                    top_y: 0,
                };
                return 1;
            }
            // Air below, then our voxel.
            out[0] = Span {
                voxel_type: VoxelType::Air.to_u8(),
                top_y: y - 1,
            };
            out[1] = Span {
                voxel_type: vt,
                top_y: y,
            };
            return 2;
        }

        // General case: rebuild spans, inserting the new voxel.
        // Walk through old spans, reconstructing with the edit applied.
        let mut out_len = 0;
        let mut handled = false;

        for (i, span) in old_spans.iter().enumerate() {
            let span_start = if i == 0 {
                0u8
            } else {
                old_spans[i - 1].top_y + 1
            };

            if !handled && y <= span.top_y {
                // This span contains y. Split/replace as needed.
                handled = true;

                // If already the target type, copy everything as-is.
                if span.voxel_type == vt {
                    out[out_len] = *span;
                    out_len += 1;
                    // Copy remaining spans.
                    for s in &old_spans[i + 1..] {
                        out[out_len] = *s;
                        out_len += 1;
                    }
                    break;
                }

                // Part before y (same type as old span).
                if y > span_start {
                    out[out_len] = Span {
                        voxel_type: span.voxel_type,
                        top_y: y - 1,
                    };
                    out_len += 1;
                }

                // The new voxel.
                out[out_len] = Span {
                    voxel_type: vt,
                    top_y: y,
                };
                out_len += 1;

                // Part after y (same type as old span).
                if y < span.top_y {
                    out[out_len] = Span {
                        voxel_type: span.voxel_type,
                        top_y: span.top_y,
                    };
                    out_len += 1;
                }

                // Copy remaining spans.
                for s in &old_spans[i + 1..] {
                    out[out_len] = *s;
                    out_len += 1;
                }
                break;
            }

            // y is not in this span, copy it.
            out[out_len] = *span;
            out_len += 1;
        }

        // If y is above all explicit spans (in the implicit Air region).
        if !handled {
            let last_top = old_spans.last().unwrap().top_y;
            if voxel != VoxelType::Air {
                // Need to add Air span(s) to bridge the gap, then our voxel.
                if y > last_top + 1 {
                    out[out_len] = Span {
                        voxel_type: VoxelType::Air.to_u8(),
                        top_y: y - 1,
                    };
                    out_len += 1;
                }
                out[out_len] = Span {
                    voxel_type: vt,
                    top_y: y,
                };
                out_len += 1;
            }
            // Setting Air in the implicit Air region: no change (already handled
            // by the equality check earlier in set_in_col, but safe to be here).
        }

        // Merge adjacent spans of the same type.
        let mut merged_len = 0;
        for i in 0..out_len {
            if merged_len > 0 && out[i].voxel_type == out[merged_len - 1].voxel_type {
                // Extend previous span.
                out[merged_len - 1].top_y = out[i].top_y;
            } else {
                out[merged_len] = out[i];
                merged_len += 1;
            }
        }

        // Trim trailing Air span (it's implicit). After the merge pass,
        // at most one trailing Air span can exist.
        if merged_len > 0 && out[merged_len - 1].voxel_type == VoxelType::Air.to_u8() {
            merged_len -= 1;
        }

        merged_len
    }

    /// Write new spans for a column, handling allocation/repack as needed.
    fn write_col_spans(&mut self, col: usize, new_spans: &[Span]) {
        let new_count = new_spans.len();
        let meta = &self.cols[col];
        let old_allocated = meta.num_allocated as usize;

        if new_count <= old_allocated {
            // Fits in place.
            let start = meta.data_start as usize;
            self.spans[start..start + new_count].copy_from_slice(new_spans);
            self.cols[col].num_spans = new_count as u8;
            return;
        }

        // Need more room. Check if free tail has space.
        let generous_alloc = new_count.max(4) + 2; // growth margin
        let alloc = generous_alloc.min(255); // cap at u8 max
        let free = self.free_start as usize;

        // Check overflow guard: if we'd push past the threshold, repack first.
        if free + alloc > GROUP_OVERFLOW_THRESHOLD as usize {
            self.repack_with_extra(col, new_spans, alloc);
            return;
        }

        // Ensure the Vec is large enough.
        if free + alloc > self.spans.len() {
            self.spans.resize(
                free + alloc,
                Span {
                    voxel_type: 0,
                    top_y: 0,
                },
            );
        }

        // Move column to free tail.
        self.spans[free..free + new_count].copy_from_slice(new_spans);
        self.cols[col] = ColMeta {
            data_start: free as u16,
            num_spans: new_count as u8,
            num_allocated: alloc as u8,
        };
        self.free_start = (free + alloc) as u16;
    }

    /// Repack the group compactly, inserting new spans for `target_col`.
    fn repack_with_extra(&mut self, target_col: usize, new_spans: &[Span], target_alloc: usize) {
        // First pass: calculate total space needed.
        let mut total_alloc = 0usize;
        for (i, col_meta) in self.cols.iter().enumerate() {
            if i == target_col {
                total_alloc += target_alloc;
            } else {
                let count = col_meta.num_spans as usize;
                if count > 0 {
                    total_alloc += (count + 2).min(255);
                }
            }
        }

        let free_tail = (total_alloc / 4).max(64);
        let mut new_vec = Vec::with_capacity(total_alloc + free_tail);
        let mut new_cols = [ColMeta::default(); COLS_PER_GROUP];
        let mut pos = 0usize;

        for (i, (col_meta, new_col)) in self.cols.iter().zip(new_cols.iter_mut()).enumerate() {
            if i == target_col {
                let count = new_spans.len();
                new_vec.extend_from_slice(new_spans);
                new_vec.resize(
                    pos + target_alloc,
                    Span {
                        voxel_type: 0,
                        top_y: 0,
                    },
                );
                *new_col = ColMeta {
                    data_start: pos as u16,
                    num_spans: count as u8,
                    num_allocated: target_alloc as u8,
                };
                pos += target_alloc;
            } else {
                let count = col_meta.num_spans as usize;
                if count == 0 {
                    continue;
                }
                let old_start = col_meta.data_start as usize;
                let alloc = (count + 2).min(255);
                new_vec.extend_from_slice(&self.spans[old_start..old_start + count]);
                new_vec.resize(
                    pos + alloc,
                    Span {
                        voxel_type: 0,
                        top_y: 0,
                    },
                );
                *new_col = ColMeta {
                    data_start: pos as u16,
                    num_spans: count as u8,
                    num_allocated: alloc as u8,
                };
                pos += alloc;
            }
        }

        // Add free tail.
        new_vec.resize(
            pos + free_tail,
            Span {
                voxel_type: 0,
                top_y: 0,
            },
        );

        self.spans = new_vec;
        self.cols = new_cols;
        self.free_start = pos as u16;
    }

    /// Compact all columns contiguously with a fresh free tail.
    fn repack(&mut self) {
        // First pass: compute total allocated space needed.
        let mut total_alloc = 0usize;
        for col_meta in &self.cols {
            let count = col_meta.num_spans as usize;
            if count > 0 {
                total_alloc += (count + 2).min(255);
            }
        }

        // Add ~25% free tail, minimum 64 spans.
        let free_tail = (total_alloc / 4).max(64);
        let capacity = total_alloc + free_tail;

        let mut new_vec = Vec::with_capacity(capacity);
        let mut new_cols = [ColMeta::default(); COLS_PER_GROUP];
        let mut pos = 0usize;

        for (col_meta, new_col) in self.cols.iter().zip(new_cols.iter_mut()) {
            let count = col_meta.num_spans as usize;
            if count == 0 {
                continue;
            }
            let old_start = col_meta.data_start as usize;
            let alloc = (count + 2).min(255);
            new_vec.extend_from_slice(&self.spans[old_start..old_start + count]);
            new_vec.resize(
                pos + alloc,
                Span {
                    voxel_type: 0,
                    top_y: 0,
                },
            );
            *new_col = ColMeta {
                data_start: pos as u16,
                num_spans: count as u8,
                num_allocated: alloc as u8,
            };
            pos += alloc;
        }

        // Ensure Vec has room for the free tail.
        new_vec.resize(
            pos + free_tail,
            Span {
                voxel_type: 0,
                top_y: 0,
            },
        );

        self.spans = new_vec;
        self.cols = new_cols;
        self.free_start = pos as u16;
    }
}

/// RLE column-based 3D voxel grid.
#[derive(Clone, Debug, Default)]
pub struct VoxelWorld {
    pub size_x: u32,
    pub size_y: u32,
    pub size_z: u32,
    /// Number of groups in each XZ dimension.
    groups_x: u32,
    #[allow(dead_code)] // Kept for symmetry; used by future bulk iteration APIs.
    groups_z: u32,
    /// Flat array of column groups, indexed by gx + gz * groups_x.
    groups: Vec<ColumnGroup>,
    /// Coordinates modified since the last drain. Used by the mesh cache to
    /// know which chunks need regeneration. Not serialized (the world is
    /// `#[serde(skip)]` on SimState and rebuilt from scratch on load, at which
    /// point the mesh cache does a full rebuild anyway).
    dirty_voxels: Vec<VoxelCoord>,
}

impl VoxelWorld {
    /// Create a new world filled with `Air`.
    pub fn new(size_x: u32, size_y: u32, size_z: u32) -> Self {
        assert!(
            (1..=255).contains(&size_y),
            "World height must be in [1, 255]"
        );
        let groups_x = (size_x + GROUP_MASK) >> GROUP_SHIFT;
        let groups_z = (size_z + GROUP_MASK) >> GROUP_SHIFT;
        let num_groups = (groups_x * groups_z) as usize;
        Self {
            size_x,
            size_y,
            size_z,
            groups_x,
            groups_z,
            groups: (0..num_groups).map(|_| ColumnGroup::default()).collect(),
            dirty_voxels: Vec::new(),
        }
    }

    /// Check whether a coordinate is within bounds.
    pub fn in_bounds(&self, coord: VoxelCoord) -> bool {
        coord.x >= 0
            && coord.y >= 0
            && coord.z >= 0
            && (coord.x as u32) < self.size_x
            && (coord.y as u32) < self.size_y
            && (coord.z as u32) < self.size_z
    }

    /// Compute the group index and local column index for a coordinate.
    /// Caller must ensure the coordinate is in bounds.
    #[inline]
    fn group_and_col(&self, coord: VoxelCoord) -> (usize, usize) {
        let x = coord.x as u32;
        let z = coord.z as u32;
        let gx = x >> GROUP_SHIFT;
        let gz = z >> GROUP_SHIFT;
        let lx = x & GROUP_MASK;
        let lz = z & GROUP_MASK;
        let gi = (gx + gz * self.groups_x) as usize;
        let col = (lx + lz * GROUP_SIZE) as usize;
        (gi, col)
    }

    /// Read a voxel. Returns `Air` for out-of-bounds coordinates.
    pub fn get(&self, coord: VoxelCoord) -> VoxelType {
        if !self.in_bounds(coord) {
            return VoxelType::Air;
        }
        let (gi, col) = self.group_and_col(coord);
        self.groups[gi].get_in_col(col, coord.y as u8)
    }

    /// Write a voxel. No-op for out-of-bounds coordinates. Appends the
    /// coordinate to `dirty_voxels` so the mesh cache knows which chunks
    /// need regeneration.
    pub fn set(&mut self, coord: VoxelCoord, voxel: VoxelType) {
        if !self.in_bounds(coord) {
            return;
        }
        let (gi, col) = self.group_and_col(coord);
        // Compute max_y as u8 here; safe because size_y is in [1, 255] and
        // size_y - 1 is in [0, 254] which fits in u8.
        let max_y = (self.size_y - 1) as u8;
        if self.groups[gi].set_in_col(col, coord.y as u8, voxel, max_y) {
            self.dirty_voxels.push(coord);
        }
    }

    /// Drain all dirty voxel coordinates accumulated since the last drain.
    /// Returns the list and clears the internal buffer.
    pub fn drain_dirty_voxels(&mut self) -> Vec<VoxelCoord> {
        std::mem::take(&mut self.dirty_voxels)
    }

    /// Discard all accumulated dirty voxel coordinates without returning them.
    /// Called after world rebuild (tree generation / save load) where the mesh
    /// cache will do a full rebuild anyway, so the dirty entries are not needed.
    pub fn clear_dirty_voxels(&mut self) {
        self.dirty_voxels.clear();
    }

    /// Compact all column groups, eliminating dead space and fragmentation.
    /// Call after worldgen or save-load to ensure clean steady-state layout.
    pub fn repack_all(&mut self) {
        for group in &mut self.groups {
            group.repack();
        }
    }

    /// Compute a top-down heightmap: for each (x, z) column, find the maximum
    /// Y with a solid voxel. Returns a flat `Vec<u8>` of `size_x * size_z`
    /// entries in row-major order (X varies fastest, then Z). A value of 0
    /// means no solid voxel in the column (possible for all-air columns).
    ///
    /// Used by the minimap to render a terrain overview without per-frame
    /// voxel queries.
    pub fn heightmap(&self) -> Vec<u8> {
        let sx = self.size_x as usize;
        let sz = self.size_z as usize;
        let mut result = vec![0u8; sx * sz];
        for z in 0..sz {
            for x in 0..sx {
                let coord = VoxelCoord::new(x as i32, 0, z as i32);
                let (gi, col) = self.group_and_col(coord);
                let spans = self.groups[gi].col_spans(col);
                // Walk spans in reverse to find the highest solid voxel.
                for span in spans.iter().rev() {
                    let vt = VoxelType::from_u8(span.voxel_type);
                    if vt.is_solid() {
                        result[x + z * sx] = span.top_y;
                        break;
                    }
                }
            }
        }
        result
    }

    /// Returns `true` if any of the 6 face-adjacent voxels (±x, ±y, ±z) is solid.
    ///
    /// Out-of-bounds neighbors return Air (from `get()`), so boundary coords
    /// are handled correctly without special cases.
    pub fn has_solid_face_neighbor(&self, coord: VoxelCoord) -> bool {
        const FACE_OFFSETS: [(i32, i32, i32); 6] = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];
        FACE_OFFSETS.iter().any(|&(dx, dy, dz)| {
            self.get(VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz))
                .is_solid()
        })
    }

    /// Returns `true` if any of the 6 face-adjacent voxels is the given type.
    pub fn has_face_neighbor_of_type(&self, coord: VoxelCoord, voxel_type: VoxelType) -> bool {
        const FACE_OFFSETS: [(i32, i32, i32); 6] = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];
        FACE_OFFSETS.iter().any(|&(dx, dy, dz)| {
            self.get(VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz)) == voxel_type
        })
    }

    /// 3D DDA raycast: returns `true` if any solid (non-Air) voxel lies on the
    /// line segment from `from` to `to` (both in world-space floats).
    ///
    /// Uses the Amanatides & Woo voxel traversal algorithm. Stops early when a
    /// solid voxel is found or the ray leaves the grid. The destination voxel
    /// itself is NOT tested (a nav node sitting on a surface should not
    /// self-occlude).
    pub fn raycast_hits_solid(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        let dir = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];

        // Current voxel coordinates.
        let mut voxel = [
            from[0].floor() as i32,
            from[1].floor() as i32,
            from[2].floor() as i32,
        ];

        // Destination voxel (we stop before testing this one).
        let end_voxel = [
            to[0].floor() as i32,
            to[1].floor() as i32,
            to[2].floor() as i32,
        ];

        // Step direction (+1 or -1) and tMax/tDelta for each axis.
        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];

        for axis in 0..3 {
            if dir[axis] > 0.0 {
                step[axis] = 1;
                t_delta[axis] = 1.0 / dir[axis];
                t_max[axis] = ((voxel[axis] as f32 + 1.0) - from[axis]) / dir[axis];
            } else if dir[axis] < 0.0 {
                step[axis] = -1;
                t_delta[axis] = 1.0 / (-dir[axis]);
                t_max[axis] = (from[axis] - voxel[axis] as f32) / (-dir[axis]);
            }
            // If dir[axis] == 0, step/t_max/t_delta stay at 0/INF/INF — axis never advances.
        }

        // March through voxels until we reach the destination or exceed t=1.
        loop {
            // Don't test the destination voxel (nav node surface shouldn't self-occlude).
            if voxel == end_voxel {
                return false;
            }

            // Test current voxel.
            let vt = self.get(VoxelCoord::new(voxel[0], voxel[1], voxel[2]));
            if vt != VoxelType::Air {
                return true;
            }

            // Advance along the axis with the smallest t_max.
            let min_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };

            // If t_max exceeds 1.0, we've passed the destination without hitting anything.
            if t_max[min_axis] > 1.0 {
                return false;
            }

            voxel[min_axis] += step[min_axis];
            t_max[min_axis] += t_delta[min_axis];
        }
    }

    /// Returns `true` if line-of-sight exists between two voxel positions.
    /// Uses the same DDA algorithm as `raycast_hits_solid`, but only blocks
    /// on voxels where `VoxelType::blocks_los()` is true (Leaf and Fruit are
    /// transparent). Neither the origin nor destination voxel self-occlude.
    ///
    /// For multi-voxel targets, the caller should check LOS to each occupied
    /// voxel and succeed if any ray is clear.
    pub fn has_los(&self, from: VoxelCoord, to: VoxelCoord) -> bool {
        if from == to {
            return true;
        }

        let from_f = [
            from.x as f32 + 0.5,
            from.y as f32 + 0.5,
            from.z as f32 + 0.5,
        ];
        let to_f = [to.x as f32 + 0.5, to.y as f32 + 0.5, to.z as f32 + 0.5];
        let dir = [
            to_f[0] - from_f[0],
            to_f[1] - from_f[1],
            to_f[2] - from_f[2],
        ];

        let mut voxel = [from.x, from.y, from.z];
        let end_voxel = [to.x, to.y, to.z];

        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];

        for axis in 0..3 {
            if dir[axis] > 0.0 {
                step[axis] = 1;
                t_delta[axis] = 1.0 / dir[axis];
                t_max[axis] = ((voxel[axis] as f32 + 1.0) - from_f[axis]) / dir[axis];
            } else if dir[axis] < 0.0 {
                step[axis] = -1;
                t_delta[axis] = 1.0 / (-dir[axis]);
                t_max[axis] = (from_f[axis] - voxel[axis] as f32) / (-dir[axis]);
            }
        }

        // Skip the origin voxel — advance once before checking.
        let min_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
            0
        } else if t_max[1] <= t_max[2] {
            1
        } else {
            2
        };
        if t_max[min_axis] > 1.0 {
            return true; // Adjacent voxels, nothing between them.
        }
        voxel[min_axis] += step[min_axis];
        t_max[min_axis] += t_delta[min_axis];

        loop {
            if voxel == end_voxel {
                return true; // Reached destination without obstruction.
            }

            let vt = self.get(VoxelCoord::new(voxel[0], voxel[1], voxel[2]));
            if vt.blocks_los() {
                return false;
            }

            let min_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };

            if t_max[min_axis] > 1.0 {
                return true;
            }

            voxel[min_axis] += step[min_axis];
            t_max[min_axis] += t_delta[min_axis];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Existing API tests (must all pass unchanged) --

    #[test]
    fn new_world_is_all_air() {
        let world = VoxelWorld::new(4, 4, 4);
        for x in 0..4 {
            for y in 0..4 {
                for z in 0..4 {
                    assert_eq!(world.get(VoxelCoord::new(x, y, z)), VoxelType::Air);
                }
            }
        }
    }

    #[test]
    fn set_and_get() {
        let mut world = VoxelWorld::new(8, 8, 8);
        let coord = VoxelCoord::new(3, 5, 2);
        world.set(coord, VoxelType::Trunk);
        assert_eq!(world.get(coord), VoxelType::Trunk);
        // Neighbors are still air.
        assert_eq!(world.get(VoxelCoord::new(3, 5, 3)), VoxelType::Air);
    }

    #[test]
    fn out_of_bounds_read_returns_air() {
        let world = VoxelWorld::new(4, 4, 4);
        assert_eq!(world.get(VoxelCoord::new(-1, 0, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(0, -1, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(4, 0, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(0, 4, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(100, 100, 100)), VoxelType::Air);
    }

    #[test]
    fn out_of_bounds_write_is_noop() {
        let mut world = VoxelWorld::new(4, 4, 4);
        // Should not panic.
        world.set(VoxelCoord::new(-1, 0, 0), VoxelType::Trunk);
        world.set(VoxelCoord::new(100, 0, 0), VoxelType::Trunk);
    }

    #[test]
    fn default_world_is_empty() {
        let world = VoxelWorld::default();
        assert_eq!(world.size_x, 0);
        assert_eq!(world.size_y, 0);
        assert_eq!(world.size_z, 0);
        // Out-of-bounds read on empty world should still return Air.
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Air);
    }

    #[test]
    fn raycast_hits_solid_voxel() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 4, 8), VoxelType::Trunk);

        // Ray from outside, through the solid voxel, to the other side.
        assert!(world.raycast_hits_solid([0.5, 4.5, 8.5], [15.5, 4.5, 8.5]));
        // Ray that doesn't pass through any solid voxel.
        assert!(!world.raycast_hits_solid([0.5, 0.5, 0.5], [15.5, 0.5, 0.5]));
    }

    #[test]
    fn raycast_does_not_self_occlude_destination() {
        let mut world = VoxelWorld::new(16, 16, 16);
        // Place a solid voxel at the destination — should not count as occluded.
        world.set(VoxelCoord::new(8, 4, 8), VoxelType::Trunk);
        assert!(!world.raycast_hits_solid([0.5, 4.5, 0.5], [8.5, 4.5, 8.5]));
    }

    #[test]
    fn raycast_blocked_before_destination() {
        let mut world = VoxelWorld::new(16, 16, 16);
        // Blocker in the middle, destination beyond it.
        world.set(VoxelCoord::new(5, 4, 8), VoxelType::Trunk);
        assert!(world.raycast_hits_solid([0.5, 4.5, 8.5], [10.5, 4.5, 8.5]));
    }

    #[test]
    fn indexing_is_correct() {
        let mut world = VoxelWorld::new(10, 8, 6);
        // Set a voxel and verify only that exact coord is affected.
        let coord = VoxelCoord::new(5, 3, 4);
        world.set(coord, VoxelType::Branch);
        assert_eq!(world.get(coord), VoxelType::Branch);
        // Adjacent coords should still be air.
        assert_eq!(world.get(VoxelCoord::new(4, 3, 4)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(5, 2, 4)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(5, 3, 3)), VoxelType::Air);
    }

    #[test]
    fn set_tracks_dirty_voxels() {
        let mut world = VoxelWorld::new(8, 8, 8);
        assert!(world.drain_dirty_voxels().is_empty());

        world.set(VoxelCoord::new(1, 2, 3), VoxelType::Trunk);
        world.set(VoxelCoord::new(4, 5, 6), VoxelType::Branch);
        let dirty = world.drain_dirty_voxels();
        assert_eq!(dirty.len(), 2);
        assert_eq!(dirty[0], VoxelCoord::new(1, 2, 3));
        assert_eq!(dirty[1], VoxelCoord::new(4, 5, 6));
        // Second drain is empty.
        assert!(world.drain_dirty_voxels().is_empty());
    }

    #[test]
    fn clear_dirty_voxels_discards_entries() {
        let mut world = VoxelWorld::new(8, 8, 8);
        world.set(VoxelCoord::new(1, 2, 3), VoxelType::Trunk);
        assert!(!world.drain_dirty_voxels().is_empty());

        world.set(VoxelCoord::new(4, 5, 6), VoxelType::Branch);
        world.clear_dirty_voxels();
        assert!(world.drain_dirty_voxels().is_empty());
    }

    #[test]
    fn out_of_bounds_set_does_not_dirty() {
        let mut world = VoxelWorld::new(4, 4, 4);
        world.set(VoxelCoord::new(-1, 0, 0), VoxelType::Trunk);
        world.set(VoxelCoord::new(100, 0, 0), VoxelType::Trunk);
        assert!(world.drain_dirty_voxels().is_empty());
    }

    #[test]
    fn has_solid_face_neighbor_true_when_adjacent() {
        let mut world = VoxelWorld::new(8, 8, 8);
        world.set(VoxelCoord::new(4, 3, 4), VoxelType::Trunk);
        // Air voxel directly above the trunk.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(4, 4, 4)));
        // Air voxel to the +x side.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(5, 3, 4)));
        // Air voxel to the -z side.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(4, 3, 3)));
    }

    #[test]
    fn has_solid_face_neighbor_false_when_isolated() {
        let world = VoxelWorld::new(8, 8, 8);
        // All-air world — no face neighbor is solid.
        assert!(!world.has_solid_face_neighbor(VoxelCoord::new(4, 4, 4)));
    }

    #[test]
    fn has_solid_face_neighbor_at_boundary() {
        let mut world = VoxelWorld::new(8, 8, 8);
        // Place solid at the edge of the world.
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        // Neighbor at (1,0,0) should detect the solid.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(1, 0, 0)));
        // Out-of-bounds neighbors return Air, so (-1,0,0) has no solid neighbor
        // besides (0,0,0) itself.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(0, 1, 0)));
        // Coord at (-1,0,0) is OOB; its neighbors include (0,0,0) which is solid.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(-1, 0, 0)));
    }

    // -- has_los tests --

    #[test]
    fn los_clear_path() {
        let world = VoxelWorld::new(16, 16, 16);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(world.has_los(a, b));
        assert!(world.has_los(b, a)); // symmetry
    }

    #[test]
    fn los_same_voxel() {
        let world = VoxelWorld::new(8, 8, 8);
        let v = VoxelCoord::new(3, 3, 3);
        assert!(world.has_los(v, v));
    }

    #[test]
    fn los_blocked_by_trunk() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(5, 4, 8), VoxelType::Trunk);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(!world.has_los(a, b));
    }

    #[test]
    fn los_leaf_transparent() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(5, 4, 8), VoxelType::Leaf);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(world.has_los(a, b));
    }

    #[test]
    fn los_fruit_transparent() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(5, 4, 8), VoxelType::Fruit);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(world.has_los(a, b));
    }

    #[test]
    fn los_origin_and_dest_not_self_occluding() {
        let mut world = VoxelWorld::new(16, 16, 16);
        // Even if the destination voxel is solid, it shouldn't block LOS.
        world.set(VoxelCoord::new(10, 4, 8), VoxelType::Trunk);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(world.has_los(a, b));
    }

    #[test]
    fn los_adjacent_voxels() {
        let mut world = VoxelWorld::new(8, 8, 8);
        // Adjacent voxels should always have LOS.
        let a = VoxelCoord::new(3, 3, 3);
        let b = VoxelCoord::new(4, 3, 3);
        assert!(world.has_los(a, b));
        // Even diagonally adjacent.
        let c = VoxelCoord::new(4, 4, 4);
        assert!(world.has_los(a, c));

        // Still clear even with solid at destination.
        world.set(VoxelCoord::new(4, 3, 3), VoxelType::Trunk);
        assert!(world.has_los(a, b));
    }

    #[test]
    fn los_diagonal_path() {
        let mut world = VoxelWorld::new(16, 16, 16);
        let a = VoxelCoord::new(2, 4, 2);
        let b = VoxelCoord::new(10, 4, 10);
        assert!(world.has_los(a, b));

        // Block a voxel along the diagonal.
        world.set(VoxelCoord::new(6, 4, 6), VoxelType::Branch);
        assert!(!world.has_los(a, b));
    }

    // -- heightmap tests --

    #[test]
    fn heightmap_empty_world() {
        let world = VoxelWorld::new(4, 8, 4);
        let hm = world.heightmap();
        assert_eq!(hm.len(), 16); // 4 * 4
        assert!(hm.iter().all(|&v| v == 0));
    }

    #[test]
    fn heightmap_returns_max_solid_y() {
        let mut world = VoxelWorld::new(4, 16, 4);
        // Place solids at different heights in the same column (x=1, z=2).
        world.set(VoxelCoord::new(1, 3, 2), VoxelType::ForestFloor);
        world.set(VoxelCoord::new(1, 7, 2), VoxelType::Trunk);
        world.set(VoxelCoord::new(1, 12, 2), VoxelType::Branch);

        let hm = world.heightmap();
        // Column (1,2) should report y=12 (the highest solid).
        assert_eq!(hm[1 + 2 * 4], 12);
        // Other columns should be 0.
        assert_eq!(hm[0 + 0 * 4], 0);
        assert_eq!(hm[3 + 3 * 4], 0);
    }

    #[test]
    fn heightmap_non_solid_types_ignored() {
        let mut world = VoxelWorld::new(4, 8, 4);
        // BuildingInterior is non-solid — should not appear in heightmap.
        world.set(VoxelCoord::new(2, 5, 1), VoxelType::BuildingInterior);
        // But a lower solid should still be picked up.
        world.set(VoxelCoord::new(2, 2, 1), VoxelType::GrownPlatform);

        let hm = world.heightmap();
        assert_eq!(hm[2 + 1 * 4], 2);
    }

    // -- RLE-specific tests --

    #[test]
    fn span_split_middle() {
        // Setting a voxel in the middle of a span splits into 3.
        let mut world = VoxelWorld::new(4, 16, 4);
        // Fill y=0..5 with Dirt.
        for y in 0..6 {
            world.set(VoxelCoord::new(0, y, 0), VoxelType::Dirt);
        }
        // Set middle to Trunk.
        world.set(VoxelCoord::new(0, 3, 0), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 2, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 3, 0)), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 4, 0)), VoxelType::Dirt);
    }

    #[test]
    fn span_split_bottom() {
        let mut world = VoxelWorld::new(4, 8, 4);
        for y in 0..4 {
            world.set(VoxelCoord::new(0, y, 0), VoxelType::Dirt);
        }
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 1, 0)), VoxelType::Dirt);
    }

    #[test]
    fn span_split_top() {
        let mut world = VoxelWorld::new(4, 8, 4);
        for y in 0..4 {
            world.set(VoxelCoord::new(0, y, 0), VoxelType::Dirt);
        }
        world.set(VoxelCoord::new(0, 3, 0), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 2, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 3, 0)), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 4, 0)), VoxelType::Air);
    }

    #[test]
    fn span_replace_single() {
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 3, 0), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 3, 0)), VoxelType::Dirt);
        world.set(VoxelCoord::new(0, 3, 0), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 3, 0)), VoxelType::Trunk);
    }

    #[test]
    fn span_merge_down() {
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        world.set(VoxelCoord::new(0, 1, 0), VoxelType::Trunk);
        // Now set y=1 to Dirt — should merge with y=0.
        world.set(VoxelCoord::new(0, 1, 0), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 1, 0)), VoxelType::Dirt);
        // Check internal span count: should be 1 span (Dirt 0..1).
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 1);
    }

    #[test]
    fn span_merge_up() {
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Trunk);
        world.set(VoxelCoord::new(0, 1, 0), VoxelType::Dirt);
        // Now set y=0 to Dirt — should merge with y=1.
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 1, 0)), VoxelType::Dirt);
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 1);
    }

    #[test]
    fn span_merge_three() {
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        world.set(VoxelCoord::new(0, 1, 0), VoxelType::Trunk);
        world.set(VoxelCoord::new(0, 2, 0), VoxelType::Dirt);
        // Set y=1 to Dirt — should merge all three into one Dirt span.
        world.set(VoxelCoord::new(0, 1, 0), VoxelType::Dirt);
        for y in 0..3 {
            assert_eq!(world.get(VoxelCoord::new(0, y, 0)), VoxelType::Dirt);
        }
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 1);
    }

    #[test]
    fn trailing_air_trim() {
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 5, 0), VoxelType::Trunk);
        // Setting it back to Air should result in 0 spans.
        world.set(VoxelCoord::new(0, 5, 0), VoxelType::Air);
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 0);
    }

    #[test]
    fn set_air_in_implicit_air_is_noop() {
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 5, 0), VoxelType::Air);
        // Should not dirty anything.
        assert!(world.drain_dirty_voxels().is_empty());
    }

    #[test]
    fn set_same_value_is_noop() {
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        world.drain_dirty_voxels();
        // Setting same value again should not dirty.
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        assert!(world.drain_dirty_voxels().is_empty());
    }

    #[test]
    fn roundtrip_all_voxel_types() {
        let mut world = VoxelWorld::new(32, 32, 4);
        let types = [
            VoxelType::Air,
            VoxelType::Trunk,
            VoxelType::Branch,
            VoxelType::GrownPlatform,
            VoxelType::GrownWall,
            VoxelType::GrownStairs,
            VoxelType::Bridge,
            VoxelType::ForestFloor,
            VoxelType::Dirt,
            VoxelType::Leaf,
            VoxelType::Fruit,
            VoxelType::Root,
            VoxelType::BuildingInterior,
            VoxelType::WoodLadder,
            VoxelType::RopeLadder,
            VoxelType::Strut,
        ];
        for (i, &vt) in types.iter().enumerate() {
            let coord = VoxelCoord::new(i as i32, 0, 0);
            world.set(coord, vt);
            assert_eq!(world.get(coord), vt, "Failed roundtrip for {vt:?}");
        }
    }

    #[test]
    fn repack_all_preserves_data() {
        let mut world = VoxelWorld::new(32, 16, 32);
        // Set a bunch of voxels.
        for x in 0..10 {
            for y in 0..5 {
                world.set(VoxelCoord::new(x, y, x), VoxelType::Dirt);
            }
            world.set(VoxelCoord::new(x, 5, x), VoxelType::Trunk);
        }
        // Record all values.
        let mut expected = Vec::new();
        for x in 0..32 {
            for y in 0..16 {
                for z in 0..32 {
                    expected.push(world.get(VoxelCoord::new(x, y, z)));
                }
            }
        }
        // Repack.
        world.repack_all();
        // Verify.
        let mut idx = 0;
        for x in 0..32 {
            for y in 0..16 {
                for z in 0..32 {
                    assert_eq!(
                        world.get(VoxelCoord::new(x, y, z)),
                        expected[idx],
                        "Mismatch at ({x}, {y}, {z})"
                    );
                    idx += 1;
                }
            }
        }
    }

    #[test]
    fn world_non_power_of_two_size() {
        // Ensure worlds whose dimensions aren't multiples of 16 work correctly.
        let mut world = VoxelWorld::new(10, 8, 6);
        let coord = VoxelCoord::new(9, 7, 5);
        world.set(coord, VoxelType::Branch);
        assert_eq!(world.get(coord), VoxelType::Branch);
        // Just past the edge is OOB.
        assert_eq!(world.get(VoxelCoord::new(10, 7, 5)), VoxelType::Air);
    }

    #[test]
    fn oracle_randomized() {
        // Cross-check RLE world against a naive flat array.
        use crate::prng::GameRng;
        let sx = 20u32;
        let sy = 16u32;
        let sz = 20u32;
        let mut world = VoxelWorld::new(sx, sy, sz);
        let mut flat = vec![VoxelType::Air; (sx * sy * sz) as usize];
        let mut rng = GameRng::new(42);

        let types = [
            VoxelType::Air,
            VoxelType::Trunk,
            VoxelType::Dirt,
            VoxelType::Leaf,
            VoxelType::Branch,
            VoxelType::ForestFloor,
        ];

        for _ in 0..2000 {
            let x = (rng.next_u32() % sx) as i32;
            let y = (rng.next_u32() % sy) as i32;
            let z = (rng.next_u32() % sz) as i32;
            let vt = types[(rng.next_u32() as usize) % types.len()];
            world.set(VoxelCoord::new(x, y, z), vt);
            let idx =
                x as usize + z as usize * sx as usize + y as usize * sx as usize * sz as usize;
            flat[idx] = vt;
        }

        // Verify every voxel matches.
        for x in 0..sx as i32 {
            for y in 0..sy as i32 {
                for z in 0..sz as i32 {
                    let idx = x as usize
                        + z as usize * sx as usize
                        + y as usize * sx as usize * sz as usize;
                    assert_eq!(
                        world.get(VoxelCoord::new(x, y, z)),
                        flat[idx],
                        "Mismatch at ({x}, {y}, {z})"
                    );
                }
            }
        }

        // Repack and verify again.
        world.repack_all();
        for x in 0..sx as i32 {
            for y in 0..sy as i32 {
                for z in 0..sz as i32 {
                    let idx = x as usize
                        + z as usize * sx as usize
                        + y as usize * sx as usize * sz as usize;
                    assert_eq!(
                        world.get(VoxelCoord::new(x, y, z)),
                        flat[idx],
                        "Mismatch after repack at ({x}, {y}, {z})"
                    );
                }
            }
        }
    }

    #[test]
    fn voxel_type_from_u8_roundtrip() {
        for i in 0..VoxelType::COUNT as u8 {
            let vt = VoxelType::from_u8(i);
            assert_eq!(vt.to_u8(), i);
        }
        // Out of range returns Air.
        assert_eq!(VoxelType::from_u8(255), VoxelType::Air);
        // Exact boundary: COUNT itself is out of range.
        assert_eq!(VoxelType::from_u8(VoxelType::COUNT as u8), VoxelType::Air);
    }

    #[test]
    fn set_at_y0_empty_column() {
        // Tests the special y=0 path in compute_new_spans for empty columns.
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 1, 0)), VoxelType::Air);
        // Should be exactly 1 span.
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 1);
    }

    #[test]
    fn set_above_all_spans_with_gap() {
        // Tests the gap-bridging Air span in the !handled path.
        let mut world = VoxelWorld::new(4, 16, 4);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        // Set at y=5, creating a gap at y=1..4.
        world.set(VoxelCoord::new(0, 5, 0), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 3, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(0, 5, 0)), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 6, 0)), VoxelType::Air);
    }

    #[test]
    fn set_above_all_spans_adjacent() {
        // Tests extending above last span without a gap, triggering merge.
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        world.set(VoxelCoord::new(0, 1, 0), VoxelType::Dirt);
        // Set y=2 to same type — should merge into one span.
        world.set(VoxelCoord::new(0, 2, 0), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 2, 0)), VoxelType::Dirt);
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 1);
    }

    #[test]
    fn set_at_max_y() {
        // Tests setting a voxel at the world ceiling.
        let mut world = VoxelWorld::new(4, 128, 4);
        world.set(VoxelCoord::new(0, 127, 0), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 127, 0)), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 126, 0)), VoxelType::Air);
        // Set back to Air — column should be empty.
        world.set(VoxelCoord::new(0, 127, 0), VoxelType::Air);
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 0);
    }

    #[test]
    fn set_at_max_y_255() {
        // Tests size_y=255 (the maximum). Ensures y=254 works at the ceiling.
        let mut world = VoxelWorld::new(4, 255, 4);
        world.set(VoxelCoord::new(0, 254, 0), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 254, 0)), VoxelType::Trunk);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 128, 0)), VoxelType::Air);
    }

    #[test]
    #[should_panic(expected = "World height must be in [1, 255]")]
    fn new_world_rejects_size_y_256() {
        VoxelWorld::new(4, 256, 4);
    }

    #[test]
    #[should_panic(expected = "World height must be in [1, 255]")]
    fn new_world_rejects_size_y_0() {
        VoxelWorld::new(4, 0, 4);
    }

    #[test]
    fn binary_search_path() {
        // Create a column with >6 spans to exercise the binary search in get().
        let mut world = VoxelWorld::new(4, 32, 4);
        // Alternating types: Dirt at even Y, Trunk at odd Y, for 16 layers.
        for y in 0..16 {
            let vt = if y % 2 == 0 {
                VoxelType::Dirt
            } else {
                VoxelType::Trunk
            };
            world.set(VoxelCoord::new(0, y, 0), vt);
        }
        // Should have 16 spans (alternating, no merges possible).
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 16);
        // Verify all values through the binary search path.
        for y in 0..16 {
            let expected = if y % 2 == 0 {
                VoxelType::Dirt
            } else {
                VoxelType::Trunk
            };
            assert_eq!(
                world.get(VoxelCoord::new(0, y, 0)),
                expected,
                "Wrong type at y={y}"
            );
        }
        // Above all spans returns Air.
        assert_eq!(world.get(VoxelCoord::new(0, 16, 0)), VoxelType::Air);
    }

    #[test]
    fn set_air_in_middle_of_solid_run() {
        // Setting Air in the middle of a solid span creates a gap without
        // spurious merging or trailing trim.
        let mut world = VoxelWorld::new(4, 8, 4);
        for y in 0..5 {
            world.set(VoxelCoord::new(0, y, 0), VoxelType::Dirt);
        }
        world.set(VoxelCoord::new(0, 2, 0), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(0, 1, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 2, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(0, 3, 0)), VoxelType::Dirt);
    }

    #[test]
    fn size_y_1() {
        // World with only one Y layer.
        let mut world = VoxelWorld::new(4, 1, 4);
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Air);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Dirt);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Air);
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 0);
    }

    #[test]
    fn multiple_columns_same_group_independent() {
        // Verify that writes to different columns in the same 16x16 group
        // don't corrupt each other, including after relocation.
        let mut world = VoxelWorld::new(16, 16, 16);
        // Write to three columns in the same group.
        for y in 0..8 {
            world.set(VoxelCoord::new(0, y, 0), VoxelType::Dirt);
        }
        for y in 0..4 {
            world.set(VoxelCoord::new(1, y, 0), VoxelType::Trunk);
        }
        for y in 0..6 {
            world.set(VoxelCoord::new(0, y, 1), VoxelType::Branch);
        }
        // Now modify column A heavily (may trigger relocation).
        for y in 0..8 {
            let vt = if y % 2 == 0 {
                VoxelType::Leaf
            } else {
                VoxelType::Root
            };
            world.set(VoxelCoord::new(0, y, 0), vt);
        }
        // Verify column B and C are untouched.
        for y in 0..4 {
            assert_eq!(
                world.get(VoxelCoord::new(1, y, 0)),
                VoxelType::Trunk,
                "Column B corrupted at y={y}"
            );
        }
        for y in 0..6 {
            assert_eq!(
                world.get(VoxelCoord::new(0, y, 1)),
                VoxelType::Branch,
                "Column C corrupted at y={y}"
            );
        }
        // Verify column A has the new data.
        for y in 0..8 {
            let expected = if y % 2 == 0 {
                VoxelType::Leaf
            } else {
                VoxelType::Root
            };
            assert_eq!(
                world.get(VoxelCoord::new(0, y, 0)),
                expected,
                "Column A wrong at y={y}"
            );
        }
    }

    #[test]
    fn set_above_last_span_no_gap_different_type() {
        // Setting a different type immediately above the last span should NOT
        // insert a bridging Air span.
        let mut world = VoxelWorld::new(4, 8, 4);
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Dirt);
        world.set(VoxelCoord::new(0, 1, 0), VoxelType::Dirt);
        // Set y=2 to Trunk (different type, immediately adjacent).
        world.set(VoxelCoord::new(0, 2, 0), VoxelType::Trunk);
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        // Should be 2 spans: Dirt 0..1, Trunk 2. No Air bridge.
        assert_eq!(world.groups[gi].cols[col].num_spans, 2);
        assert_eq!(world.get(VoxelCoord::new(0, 1, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 2, 0)), VoxelType::Trunk);
        assert_eq!(world.get(VoxelCoord::new(0, 3, 0)), VoxelType::Air);
    }

    #[test]
    fn set_air_at_y0_with_solid_above() {
        // Setting y=0 to Air when there's solid above should leave the solid
        // intact and make y=0 Air (leading Air is stored explicitly, unlike
        // trailing Air which is implicit).
        let mut world = VoxelWorld::new(4, 8, 4);
        for y in 0..5 {
            world.set(VoxelCoord::new(0, y, 0), VoxelType::Dirt);
        }
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(0, 1, 0)), VoxelType::Dirt);
        assert_eq!(world.get(VoxelCoord::new(0, 4, 0)), VoxelType::Dirt);
        // Should be 2 spans: Air at 0, Dirt at 1..4.
        let (gi, col) = world.group_and_col(VoxelCoord::new(0, 0, 0));
        assert_eq!(world.groups[gi].cols[col].num_spans, 2);
    }
}
