// Unified pathfinding for ground and flying creatures on the voxel grid.
//
// Ground creatures use A* on the voxel grid (`astar_ground`) with walkability
// checks, edge-type filtering, and species-specific traversal costs. Flying
// creatures use A* on the 3D voxel grid (`astar_fly`) with footprint clearance
// checks. Both return the same `PathResult` type.
//
// Multi-target "nearest" searches use interleaved A* (`nearest_astar_ground`,
// `nearest_astar_fly`): per-candidate open sets with candidate-specific
// heuristics share a single g_score/closed set. The search expands the
// globally smallest f-value across all candidates, and prunes candidates whose
// minimum f exceeds the best completed path cost. This focuses work toward
// likely-nearest candidates rather than expanding uniformly like Dijkstra.
// For the special case of extremely numerous nearby candidates (e.g., grass),
// callers use bespoke inline Dijkstra instead (see `grazing.rs`).
//
// All path-finding functions take a `&PathOpts` parameter that bundles two
// independent limits: a **path length limit** (max edges/hops in the result)
// and a **work budget** (max heap pops / node expansions, bounding CPU time).
// Both can be `Auto` (callee picks a reasonable default based on heuristic
// distance) or `Exact(n)`. Functions return `Result<T, PathError>` with
// structured error variants so callers can distinguish "too far" from "too
// expensive" from "truly unreachable."
//
// See also: `nav.rs` for edge type constants, `walkability.rs` for walkable
// position queries, `world.rs` for the `VoxelWorld`, `sim/movement.rs` which
// consumes path results for step-by-step movement.
//
// **Critical constraint: determinism.** All functions are pure functions of
// their inputs — no randomness, no floats, all integer arithmetic. Uses
// `BTreeMap` for visited sets (ordered by `VoxelCoord`) and `VoxelCoord`
// tiebreaking in the priority queue.

use crate::nav::EdgeType;
use crate::types::{FaceData, VoxelCoord};
use crate::walkability;
use crate::world::VoxelWorld;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::BinaryHeap;

use crate::nav::DIST_SCALE;

// ---------------------------------------------------------------------------
// Path options and error types
// ---------------------------------------------------------------------------

/// Structured error returned by all pathfinding functions.
///
/// Replaces the old `Option`-based returns so callers can distinguish
/// "too far" from "too expensive to compute" from "truly unreachable."
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathError {
    /// A path may exist but exceeds the hop-count budget.
    ExceededPathLen { limit: u32 },
    /// Search terminated after expanding too many nodes (CPU protection).
    ExceededWorkBudget { limit: u32, expanded: u32 },
    /// No path exists (disconnected graph region, fully walled off, etc.).
    Unreachable,
    /// Creature's position doesn't map to a nav node (ground creatures) or
    /// is in a non-flyable voxel (flyers).
    StartNotOnGraph,
    /// Start position fails footprint clearance check (flyers).
    StartBlockedByFootprint,
    /// Goal position doesn't map to a nav node or is non-flyable.
    TargetNotOnGraph,
    /// Candidate list was empty after filtering (`find_nearest` only).
    NoTargets,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExceededPathLen { limit } => {
                write!(f, "path length exceeds limit ({limit} hops)")
            }
            Self::ExceededWorkBudget { limit, expanded } => {
                write!(
                    f,
                    "work budget exhausted ({expanded}/{limit} nodes expanded)"
                )
            }
            Self::Unreachable => write!(f, "target unreachable"),
            Self::StartNotOnGraph => write!(f, "start position not walkable"),
            Self::StartBlockedByFootprint => {
                write!(f, "start position blocked by footprint")
            }
            Self::TargetNotOnGraph => write!(f, "target position not walkable"),
            Self::NoTargets => write!(f, "no valid targets"),
        }
    }
}

/// How to cap the number of edges (hops) in a path result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathLenLimit {
    /// Callee picks a reasonable limit based on heuristic distance to the
    /// target(s): `manhattan(start, furthest_goal) * 3 + 100`.
    Auto,
    /// Exact hop-count cap.
    Exact(u32),
}

/// How to cap the total number of node expansions (heap pops) in a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkBudget {
    /// Callee picks a reasonable budget derived from the resolved path
    /// length limit and graph type. Nav graph: `path_len * 30`. Flight:
    /// `path_len * 100`. Nearest-among-N: multiply by `1 + isqrt(N)`.
    Auto,
    /// Exact node-expansion cap.
    Exact(u32),
}

/// Bundled search constraints for all pathfinding functions.
///
/// Both fields default to `Auto`, which lets the callee pick reasonable
/// bounds based on heuristic distance and graph type. Use the builder
/// methods `with_path_len()` / `with_work()` to override individual limits.
///
/// ```ignore
/// PathOpts::default()                    // both auto
/// PathOpts::default().with_path_len(500) // exact path len, auto work
/// PathOpts::default().with_work(10000)   // auto path len, exact work
/// ```
#[derive(Clone, Debug)]
pub struct PathOpts {
    pub path_len: PathLenLimit,
    pub work: WorkBudget,
}

impl Default for PathOpts {
    fn default() -> Self {
        Self {
            path_len: PathLenLimit::Auto,
            work: WorkBudget::Auto,
        }
    }
}

impl PathOpts {
    /// Set an exact path-length limit (max hops in the result).
    pub fn with_path_len(mut self, limit: u32) -> Self {
        self.path_len = PathLenLimit::Exact(limit);
        self
    }

    /// Set an exact work budget (max node expansions / heap pops).
    pub fn with_work(mut self, budget: u32) -> Self {
        self.work = WorkBudget::Exact(budget);
        self
    }
}

/// Resolved concrete limits for a single-target search.
struct ResolvedLimits {
    path_len: u32,
    work: u32,
}

/// Integer square root (floor). Used for work budget scaling with candidate
/// count in nearest-among-N searches.
///
/// Pure integer implementation (no floating-point) to comply with the sim
/// crate's determinism constraint. Uses bit-shifting to find the initial
/// estimate, then refines with integer Newton's method.
fn isqrt(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    // Initial estimate via bit-shifting: 2^(ceil(bits/2)).
    let shift = (32 - n.leading_zeros()).div_ceil(2);
    let mut x = 1u32 << shift;
    // Integer Newton's method: x = (x + n/x) / 2.
    loop {
        let next = (x + n / x) / 2;
        if next >= x {
            break;
        }
        x = next;
    }
    // x is now the floor of sqrt(n). Verify using u64 to avoid overflow.
    debug_assert!((x as u64) * (x as u64) <= n as u64);
    debug_assert!(((x + 1) as u64) * ((x + 1) as u64) > n as u64);
    x
}

/// Resolve `PathOpts` into concrete limits for a single-target search.
///
/// - `manhattan_to_target`: Manhattan distance from start to goal (or
///   furthest candidate for nearest-among-N).
/// - `is_flight`: true for voxel-grid flight A*, false for ground A*.
/// - `n_candidates`: number of candidates (1 for single-target searches).
fn resolve_limits(
    opts: &PathOpts,
    manhattan_to_target: u32,
    is_flight: bool,
    n_candidates: u32,
) -> ResolvedLimits {
    let path_len = match opts.path_len {
        PathLenLimit::Exact(n) => n,
        PathLenLimit::Auto => manhattan_to_target.saturating_mul(3).saturating_add(100),
    };
    let base_work = match opts.work {
        WorkBudget::Exact(n) => n,
        WorkBudget::Auto => {
            let multiplier: u32 = if is_flight { 100 } else { 30 };
            path_len.saturating_mul(multiplier)
        }
    };
    let work = if n_candidates > 1 && matches!(opts.work, WorkBudget::Auto) {
        base_work.saturating_mul(1 + isqrt(n_candidates))
    } else {
        base_work
    };
    ResolvedLimits { path_len, work }
}

/// The result of a successful pathfinding search (ground or flight).
///
/// `positions` contains the voxel coordinates from start to goal (inclusive).
#[derive(Clone, Debug, PartialEq)]
pub struct PathResult {
    /// Voxel positions from start to goal (inclusive).
    pub positions: Vec<VoxelCoord>,
    /// Total traversal cost (distance_scaled × tpv, in DIST_SCALE units).
    pub total_cost: i64,
}

/// Bundled speed parameters for ground pathfinding.
///
/// Combines the four per-species ticks-per-voxel values and the optional
/// edge-type filter into a single struct to reduce argument lists. Construct
/// from `SpeciesData` (raw speeds) or `CreatureMoveSpeeds` (stat-modified).
pub struct GroundSpeeds<'a> {
    pub walk_tpv: u64,
    pub climb_tpv: Option<u64>,
    pub wood_ladder_tpv: Option<u64>,
    pub rope_ladder_tpv: Option<u64>,
    pub allowed_edges: Option<&'a [EdgeType]>,
}

impl<'a> GroundSpeeds<'a> {
    /// Build from raw species data fields.
    pub fn from_species(species_data: &'a crate::species::SpeciesData) -> Self {
        Self {
            walk_tpv: species_data.walk_ticks_per_voxel,
            climb_tpv: species_data.climb_ticks_per_voxel,
            wood_ladder_tpv: species_data.wood_ladder_tpv,
            rope_ladder_tpv: species_data.rope_ladder_tpv,
            allowed_edges: species_data.allowed_edge_types.as_deref(),
        }
    }

    /// Build from stat-modified creature move speeds plus an edge filter.
    pub fn from_move_speeds(
        speeds: &crate::stats::CreatureMoveSpeeds,
        allowed_edges: Option<&'a [EdgeType]>,
    ) -> Self {
        Self {
            walk_tpv: speeds.walk_tpv,
            climb_tpv: speeds.climb_tpv,
            wood_ladder_tpv: speeds.wood_ladder_tpv,
            rope_ladder_tpv: speeds.rope_ladder_tpv,
            allowed_edges,
        }
    }
}

// ---------------------------------------------------------------------------
// Flight A* (flying creatures)
// ---------------------------------------------------------------------------

/// Precomputed scaled distances for the 26 neighbor offsets.
/// Face-adjacent (6): distance = DIST_SCALE (1024)
/// Edge-diagonal (12): distance = floor(sqrt(2) * 1024) = 1448
/// Corner-diagonal (8): distance = floor(sqrt(3) * 1024) = 1773
pub const NEIGHBOR_OFFSETS: [(i32, i32, i32, u64); 26] = [
    // Face-adjacent (6)
    (-1, 0, 0, 1024),
    (1, 0, 0, 1024),
    (0, -1, 0, 1024),
    (0, 1, 0, 1024),
    (0, 0, -1, 1024),
    (0, 0, 1, 1024),
    // Edge-diagonal (12)
    (-1, -1, 0, 1448),
    (-1, 1, 0, 1448),
    (1, -1, 0, 1448),
    (1, 1, 0, 1448),
    (-1, 0, -1, 1448),
    (-1, 0, 1, 1448),
    (1, 0, -1, 1448),
    (1, 0, 1, 1448),
    (0, -1, -1, 1448),
    (0, -1, 1, 1448),
    (0, 1, -1, 1448),
    (0, 1, 1, 1448),
    // Corner-diagonal (8)
    (-1, -1, -1, 1773),
    (-1, -1, 1, 1773),
    (-1, 1, -1, 1773),
    (-1, 1, 1, 1773),
    (1, -1, -1, 1773),
    (1, -1, 1, 1773),
    (1, 1, -1, 1773),
    (1, 1, 1, 1773),
];

/// Entry in the A* open set (min-heap via reversed ordering).
struct OpenEntry {
    pos: VoxelCoord,
    f_score: i64,
}

impl PartialEq for OpenEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f_score == other.f_score && self.pos == other.pos
    }
}

impl Eq for OpenEntry {}

impl PartialOrd for OpenEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OpenEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed for min-heap: smallest f_score is "greatest".
        // Tiebreaker: VoxelCoord ordering for determinism.
        other
            .f_score
            .cmp(&self.f_score)
            .then_with(|| other.pos.cmp(&self.pos))
    }
}

/// 3D octile heuristic (admissible and consistent for 26-connected grids).
///
/// Octile distance in 3D: let d1 ≤ d2 ≤ d3 be the sorted absolute deltas.
/// Cost = d1 * corner_dist + (d2 - d1) * edge_dist + (d3 - d2) * face_dist,
/// where face_dist = DIST_SCALE, edge_dist = 1448, corner_dist = 1773.
///
/// Multiplied by `flight_tpv` to match the edge cost scale.
fn octile_heuristic_3d(a: VoxelCoord, b: VoxelCoord, flight_tpv: u64) -> i64 {
    let dx = (a.x - b.x).unsigned_abs();
    let dy = (a.y - b.y).unsigned_abs();
    let dz = (a.z - b.z).unsigned_abs();

    // Sort deltas: d1 ≤ d2 ≤ d3.
    let (mut d1, mut d2, mut d3) = (dx, dy, dz);
    if d1 > d2 {
        std::mem::swap(&mut d1, &mut d2);
    }
    if d2 > d3 {
        std::mem::swap(&mut d2, &mut d3);
    }
    if d1 > d2 {
        std::mem::swap(&mut d1, &mut d2);
    }

    let face: u64 = DIST_SCALE as u64; // 1024
    let edge: u64 = 1448; // floor(sqrt(2) * 1024)
    let corner: u64 = 1773; // floor(sqrt(3) * 1024)

    let h = d1 as u64 * corner + (d2 - d1) as u64 * edge + (d3 - d2) as u64 * face;

    (h * flight_tpv) as i64
}

/// Check whether all voxels in a footprint anchored at `anchor` are flyable.
/// The anchor is the min-corner (smallest x, y, z) of the bounding box.
pub fn footprint_flyable(world: &VoxelWorld, anchor: VoxelCoord, footprint: [u8; 3]) -> bool {
    for dx in 0..footprint[0] as i32 {
        for dy in 0..footprint[1] as i32 {
            for dz in 0..footprint[2] as i32 {
                let v = VoxelCoord::new(anchor.x + dx, anchor.y + dy, anchor.z + dz);
                if !world.in_bounds(v) || !world.get(v).is_flyable() {
                    return false;
                }
            }
        }
    }
    true
}

/// Find the shortest flight path from `start` to `goal` through flyable voxels.
///
/// **Prefer `SimState::find_path()`** unless you need direct control over
/// flight parameters. `find_path` handles species dispatch, speed lookup,
/// and footprint lookup automatically.
///
/// The `footprint` `[width_x, height_y, depth_z]` specifies the creature's
/// bounding box. For 1×1×1 creatures this is `[1,1,1]`; for 2×2×2 it's
/// `[2,2,2]`. All voxels in the footprint must be flyable at every position.
///
/// Search bounded by `opts` (path length and work budget).
///
/// Returns `Err(PathError)` with a structured error if the search fails.
pub fn astar_fly(
    world: &VoxelWorld,
    start: VoxelCoord,
    goal: VoxelCoord,
    flight_tpv: u64,
    opts: &PathOpts,
    footprint: [u8; 3],
) -> Result<PathResult, PathError> {
    if !footprint_flyable(world, start, footprint) {
        return Err(PathError::StartBlockedByFootprint);
    }
    if !footprint_flyable(world, goal, footprint) {
        return Err(PathError::TargetNotOnGraph);
    }
    if start == goal {
        return Ok(PathResult {
            positions: vec![start],
            total_cost: 0,
        });
    }

    let manhattan = start.manhattan_distance(goal);
    let limits = resolve_limits(opts, manhattan, true, 1);

    // g_score and came_from stored in a BTreeMap keyed by VoxelCoord
    // (deterministic iteration order, no hash-order dependence).
    let mut g_score: BTreeMap<VoxelCoord, i64> = BTreeMap::new();
    let mut came_from: BTreeMap<VoxelCoord, VoxelCoord> = BTreeMap::new();
    let mut depth: BTreeMap<VoxelCoord, u32> = BTreeMap::new();
    let mut open = BinaryHeap::new();

    g_score.insert(start, 0);
    depth.insert(start, 0);
    open.push(OpenEntry {
        pos: start,
        f_score: octile_heuristic_3d(start, goal, flight_tpv),
    });

    let mut expanded: u32 = 0;
    let mut hit_path_len_limit = false;

    while let Some(OpenEntry { pos, f_score }) = open.pop() {
        if pos == goal {
            // Reconstruct path.
            let mut path = vec![goal];
            let mut current = goal;
            while let Some(&prev) = came_from.get(&current) {
                path.push(prev);
                current = prev;
            }
            path.reverse();
            let total_cost = g_score[&goal];
            return Ok(PathResult {
                positions: path,
                total_cost,
            });
        }

        let current_g = match g_score.get(&pos) {
            Some(&g) => g,
            None => continue,
        };

        // Skip stale entries: this node was re-pushed with a better g_score,
        // and the better entry has already been (or will be) expanded.
        let h = octile_heuristic_3d(pos, goal, flight_tpv);
        if current_g + h < f_score {
            continue;
        }

        expanded += 1;
        if expanded > limits.work {
            return Err(PathError::ExceededWorkBudget {
                limit: limits.work,
                expanded,
            });
        }

        let current_depth = depth.get(&pos).copied().unwrap_or(0);

        // If we're already at max depth, don't expand further.
        if current_depth >= limits.path_len {
            hit_path_len_limit = true;
            continue;
        }

        for &(dx, dy, dz, dist_scaled) in &NEIGHBOR_OFFSETS {
            let nx = pos.x + dx;
            let ny = pos.y + dy;
            let nz = pos.z + dz;
            let neighbor = VoxelCoord::new(nx, ny, nz);

            if !footprint_flyable(world, neighbor, footprint) {
                continue;
            }

            let move_cost = (dist_scaled * flight_tpv) as i64;
            let tentative_g = current_g + move_cost;

            if tentative_g < g_score.get(&neighbor).copied().unwrap_or(i64::MAX) {
                g_score.insert(neighbor, tentative_g);
                came_from.insert(neighbor, pos);
                depth.insert(neighbor, current_depth + 1);
                let h = octile_heuristic_3d(neighbor, goal, flight_tpv);
                open.push(OpenEntry {
                    pos: neighbor,
                    f_score: tentative_g + h,
                });
            }
        }
    }

    if hit_path_len_limit {
        Err(PathError::ExceededPathLen {
            limit: limits.path_len,
        })
    } else {
        Err(PathError::Unreachable)
    }
}

/// Find the nearest reachable candidate from `start` using interleaved
/// flight A*.
///
/// Maintains per-target open sets with target-specific heuristics, sharing
/// a single g_score/closed set across all targets. Returns the first target
/// reached by travel cost.
///
/// Search bounded by `opts` (path length and work budget).
///
/// Returns `Err(PathError)` if no candidate is reachable.
pub fn nearest_astar_fly(
    world: &VoxelWorld,
    start: VoxelCoord,
    candidates: &[VoxelCoord],
    flight_tpv: u64,
    opts: &PathOpts,
    footprint: [u8; 3],
) -> Result<VoxelCoord, PathError> {
    if candidates.is_empty() {
        return Err(PathError::NoTargets);
    }

    if candidates.contains(&start) {
        return Ok(start);
    }

    if !footprint_flyable(world, start, footprint) {
        return Err(PathError::StartBlockedByFootprint);
    }

    let mut candidate_indices: Vec<usize> = (0..candidates.len())
        .filter(|&i| footprint_flyable(world, candidates[i], footprint))
        .collect();
    if candidate_indices.is_empty() {
        return Err(PathError::NoTargets);
    }
    candidate_indices.sort_by_key(|&i| octile_heuristic_3d(start, candidates[i], flight_tpv));

    let max_manhattan = candidate_indices
        .iter()
        .map(|&i| start.manhattan_distance(candidates[i]))
        .max()
        .unwrap_or(0);
    let limits = resolve_limits(opts, max_manhattan, true, candidate_indices.len() as u32);

    let mut g_score: BTreeMap<VoxelCoord, i64> = BTreeMap::new();
    let mut depth: BTreeMap<VoxelCoord, u32> = BTreeMap::new();
    let mut closed: std::collections::BTreeSet<VoxelCoord> = std::collections::BTreeSet::new();

    g_score.insert(start, 0);
    depth.insert(start, 0);

    let mut open_sets: Vec<BinaryHeap<OpenEntry>> = candidate_indices
        .iter()
        .map(|&i| {
            let mut heap = BinaryHeap::new();
            let h = octile_heuristic_3d(start, candidates[i], flight_tpv);
            heap.push(OpenEntry {
                pos: start,
                f_score: h,
            });
            heap
        })
        .collect();

    let mut active: Vec<bool> = vec![true; candidate_indices.len()];
    let mut best_cost: Option<(VoxelCoord, i64)> = None;
    let mut expanded: u32 = 0;
    let mut hit_path_len_limit = false;

    loop {
        let mut best_ci: Option<usize> = None;
        let mut best_f = i64::MAX;
        for (ci, heap) in open_sets.iter().enumerate() {
            if !active[ci] {
                continue;
            }
            if let Some(top) = heap.peek() {
                if top.f_score < best_f
                    || (top.f_score == best_f
                        && best_ci.is_none_or(|prev| top.pos < open_sets[prev].peek().unwrap().pos))
                {
                    best_f = top.f_score;
                    best_ci = Some(ci);
                }
            } else {
                active[ci] = false;
            }
        }

        let ci = match best_ci {
            Some(ci) => ci,
            None => break,
        };

        if let Some((_, cost)) = best_cost
            && best_f >= cost
        {
            break;
        }

        let entry = open_sets[ci].pop().unwrap();
        let pos = entry.pos;

        if !closed.contains(&pos) {
            let target = candidates[candidate_indices[ci]];
            if pos == target {
                let cost = g_score[&pos];
                match best_cost {
                    None => best_cost = Some((pos, cost)),
                    Some((_, prev)) if cost < prev => best_cost = Some((pos, cost)),
                    _ => {}
                }
                active[ci] = false;

                let best_c = best_cost.unwrap().1;
                for (oci, heap) in open_sets.iter().enumerate() {
                    if active[oci] {
                        if let Some(top) = heap.peek() {
                            if top.f_score >= best_c {
                                active[oci] = false;
                            }
                        } else {
                            active[oci] = false;
                        }
                    }
                }
                continue;
            }
        }

        if !closed.insert(pos) {
            continue;
        }

        expanded += 1;
        if expanded > limits.work {
            return Err(PathError::ExceededWorkBudget {
                limit: limits.work,
                expanded,
            });
        }

        let current_g = match g_score.get(&pos) {
            Some(&g) => g,
            None => continue,
        };

        let current_depth = depth.get(&pos).copied().unwrap_or(0);
        if current_depth >= limits.path_len {
            hit_path_len_limit = true;
            continue;
        }

        for &(dx, dy, dz, dist_scaled) in &NEIGHBOR_OFFSETS {
            let neighbor = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);

            if closed.contains(&neighbor) || !footprint_flyable(world, neighbor, footprint) {
                continue;
            }

            let move_cost = (dist_scaled * flight_tpv) as i64;
            let tentative_g = current_g + move_cost;

            if tentative_g < g_score.get(&neighbor).copied().unwrap_or(i64::MAX) {
                g_score.insert(neighbor, tentative_g);
                depth.insert(neighbor, current_depth + 1);

                for (oci, heap) in open_sets.iter_mut().enumerate() {
                    if !active[oci] {
                        continue;
                    }
                    let h = octile_heuristic_3d(
                        neighbor,
                        candidates[candidate_indices[oci]],
                        flight_tpv,
                    );
                    let f = tentative_g + h;
                    if let Some((_, best_c)) = best_cost
                        && f >= best_c
                    {
                        continue;
                    }
                    heap.push(OpenEntry {
                        pos: neighbor,
                        f_score: f,
                    });
                }
            }
        }
    }

    match best_cost {
        Some((coord, _)) => Ok(coord),
        None if hit_path_len_limit => Err(PathError::ExceededPathLen {
            limit: limits.path_len,
        }),
        None => Err(PathError::Unreachable),
    }
}

/// Find the nearest reachable candidate from `start` by flight cost.
///
/// **Prefer `SimState::find_nearest()`** unless you need direct control over
/// flight parameters. `find_nearest` handles species dispatch, speed lookup,
/// footprint lookup, and index mapping automatically.
///
/// Delegates to `nearest_astar_fly` (interleaved A*), which uses heuristic
/// guidance to focus the search toward candidates and prune distant ones early.
///
/// Returns `None` if no candidate is reachable.
pub fn nearest_fly(
    world: &VoxelWorld,
    start: VoxelCoord,
    candidates: &[VoxelCoord],
    flight_tpv: u64,
    opts: &PathOpts,
    footprint: [u8; 3],
) -> Result<VoxelCoord, PathError> {
    nearest_astar_fly(world, start, candidates, flight_tpv, opts, footprint)
}

// ---------------------------------------------------------------------------
// Ground A* (voxel-direct, replacing nav-graph A*)
// ---------------------------------------------------------------------------

/// Compute the ticks-per-voxel for traversing an edge of the given type.
/// Returns `None` if the species cannot traverse this edge type (e.g., a
/// ground-only creature encountering a climb edge with no climb speed).
fn tpv_for_edge_type(edge_type: EdgeType, speeds: &GroundSpeeds) -> Option<u64> {
    match edge_type {
        EdgeType::TrunkClimb | EdgeType::GroundToTrunk => speeds.climb_tpv,
        EdgeType::WoodLadderClimb => speeds.wood_ladder_tpv,
        EdgeType::RopeLadderClimb => speeds.rope_ladder_tpv,
        _ => Some(speeds.walk_tpv),
    }
}

/// Find the shortest ground path from `start` to `goal` using A* on the voxel
/// grid with walkability checks.
///
/// This is the voxel-direct replacement for `astar_navgraph`. Instead of
/// searching a pre-computed nav graph, it expands 26-neighbors on the voxel
/// grid, checking walkability and deriving edge types at search time.
///
/// The `footprint` `[width_x, height_y, depth_z]` specifies the creature's
/// bounding box. For 1×1×1 creatures this is `[1,1,1]`; for 2×2×2 it's
/// `[2,2,2]`. All voxels in the footprint must be walkable at every position.
///
/// Search bounded by `opts` (path length and work budget).
///
/// Returns `Err(PathError)` with a structured error if the search fails.
pub fn astar_ground(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    start: VoxelCoord,
    goal: VoxelCoord,
    speeds: &GroundSpeeds,
    opts: &PathOpts,
    footprint: [u8; 3],
) -> Result<PathResult, PathError> {
    if !walkability::footprint_walkable(world, face_data, start, footprint) {
        return Err(PathError::StartNotOnGraph);
    }
    if !walkability::footprint_walkable(world, face_data, goal, footprint) {
        return Err(PathError::TargetNotOnGraph);
    }
    if start == goal {
        return Ok(PathResult {
            positions: vec![start],
            total_cost: 0,
        });
    }

    let manhattan = start.manhattan_distance(goal);
    let limits = resolve_limits(opts, manhattan, false, 1);

    let walk_tpv = speeds.walk_tpv;

    let mut g_score: BTreeMap<VoxelCoord, i64> = BTreeMap::new();
    let mut came_from: BTreeMap<VoxelCoord, VoxelCoord> = BTreeMap::new();
    let mut depth: BTreeMap<VoxelCoord, u32> = BTreeMap::new();
    let mut open = BinaryHeap::new();

    g_score.insert(start, 0);
    depth.insert(start, 0);
    open.push(OpenEntry {
        pos: start,
        f_score: octile_heuristic_3d(start, goal, walk_tpv),
    });

    let mut expanded: u32 = 0;
    let mut hit_path_len_limit = false;

    while let Some(OpenEntry { pos, f_score }) = open.pop() {
        if pos == goal {
            // Reconstruct path.
            let mut path = vec![goal];
            let mut current = goal;
            while let Some(&prev) = came_from.get(&current) {
                path.push(prev);
                current = prev;
            }
            path.reverse();
            let total_cost = g_score[&goal];
            return Ok(PathResult {
                positions: path,
                total_cost,
            });
        }

        let current_g = match g_score.get(&pos) {
            Some(&g) => g,
            None => continue,
        };

        // Skip stale entries.
        let h = octile_heuristic_3d(pos, goal, walk_tpv);
        if current_g + h < f_score {
            continue;
        }

        expanded += 1;
        if expanded > limits.work {
            return Err(PathError::ExceededWorkBudget {
                limit: limits.work,
                expanded,
            });
        }

        let current_depth = depth.get(&pos).copied().unwrap_or(0);
        if current_depth >= limits.path_len {
            hit_path_len_limit = true;
            continue;
        }

        // Derive surface type at current position for edge-type computation.
        let from_surface = walkability::derive_surface_type(world, face_data, pos);

        // Expand all valid neighbors (including surface-snap for large creatures).
        for (neighbor, dist_scaled) in
            walkability::ground_neighbors(world, face_data, pos, footprint)
        {
            // Derive edge type and compute cost.
            let to_surface = walkability::derive_surface_type(world, face_data, neighbor);
            let edge_type = walkability::derive_edge_type(from_surface, to_surface, pos, neighbor);

            // Check allowed edge types.
            if let Some(allowed) = speeds.allowed_edges
                && !allowed.contains(&edge_type)
            {
                continue;
            }

            let tpv = match tpv_for_edge_type(edge_type, speeds) {
                Some(t) => t,
                None => continue, // species cannot traverse this edge type
            };

            let move_cost = (dist_scaled * tpv) as i64;
            let tentative_g = current_g + move_cost;

            if tentative_g < g_score.get(&neighbor).copied().unwrap_or(i64::MAX) {
                g_score.insert(neighbor, tentative_g);
                came_from.insert(neighbor, pos);
                depth.insert(neighbor, current_depth + 1);
                let h = octile_heuristic_3d(neighbor, goal, walk_tpv);
                open.push(OpenEntry {
                    pos: neighbor,
                    f_score: tentative_g + h,
                });
            }
        }
    }

    if hit_path_len_limit {
        Err(PathError::ExceededPathLen {
            limit: limits.path_len,
        })
    } else {
        Err(PathError::Unreachable)
    }
}

/// Find the nearest reachable candidate from `start` using voxel-direct ground
/// A* with interleaved multi-target search.
///
/// This is the voxel-direct replacement for `nearest_astar_navgraph`. Uses the
/// same interleaved A* algorithm as `nearest_astar_fly` but with ground
/// walkability checks and edge-type cost differentiation.
pub fn nearest_astar_ground(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    start: VoxelCoord,
    candidates: &[VoxelCoord],
    speeds: &GroundSpeeds,
    opts: &PathOpts,
    footprint: [u8; 3],
) -> Result<VoxelCoord, PathError> {
    if candidates.is_empty() {
        return Err(PathError::NoTargets);
    }

    if candidates.contains(&start) {
        return Ok(start);
    }

    if !walkability::footprint_walkable(world, face_data, start, footprint) {
        return Err(PathError::StartNotOnGraph);
    }

    // Pre-filter: only candidates with walkable positions.
    let walk_tpv = speeds.walk_tpv;
    let mut candidate_indices: Vec<usize> = (0..candidates.len())
        .filter(|&i| walkability::footprint_walkable(world, face_data, candidates[i], footprint))
        .collect();
    if candidate_indices.is_empty() {
        return Err(PathError::NoTargets);
    }
    candidate_indices.sort_by_key(|&i| octile_heuristic_3d(start, candidates[i], walk_tpv));

    let max_manhattan = candidate_indices
        .iter()
        .map(|&i| start.manhattan_distance(candidates[i]))
        .max()
        .unwrap_or(0);
    let limits = resolve_limits(opts, max_manhattan, false, candidate_indices.len() as u32);

    let mut g_score: BTreeMap<VoxelCoord, i64> = BTreeMap::new();
    let mut depth: BTreeMap<VoxelCoord, u32> = BTreeMap::new();
    let mut closed: std::collections::BTreeSet<VoxelCoord> = std::collections::BTreeSet::new();

    g_score.insert(start, 0);
    depth.insert(start, 0);

    let mut open_sets: Vec<BinaryHeap<OpenEntry>> = candidate_indices
        .iter()
        .map(|&i| {
            let mut heap = BinaryHeap::new();
            let h = octile_heuristic_3d(start, candidates[i], walk_tpv);
            heap.push(OpenEntry {
                pos: start,
                f_score: h,
            });
            heap
        })
        .collect();

    let mut active: Vec<bool> = vec![true; candidate_indices.len()];
    let mut best_cost: Option<(VoxelCoord, i64)> = None;
    let mut expanded: u32 = 0;
    let mut hit_path_len_limit = false;

    loop {
        // Find active candidate with globally smallest top-of-heap f.
        let mut best_ci: Option<usize> = None;
        let mut best_f = i64::MAX;
        for (ci, heap) in open_sets.iter().enumerate() {
            if !active[ci] {
                continue;
            }
            if let Some(top) = heap.peek() {
                if top.f_score < best_f
                    || (top.f_score == best_f
                        && best_ci.is_none_or(|prev| top.pos < open_sets[prev].peek().unwrap().pos))
                {
                    best_f = top.f_score;
                    best_ci = Some(ci);
                }
            } else {
                active[ci] = false;
            }
        }

        let ci = match best_ci {
            Some(ci) => ci,
            None => break,
        };

        if let Some((_, cost)) = best_cost
            && best_f >= cost
        {
            break;
        }

        let entry = open_sets[ci].pop().unwrap();
        let pos = entry.pos;

        // Check if this node is the candidate for this open set.
        if !closed.contains(&pos) {
            let target = candidates[candidate_indices[ci]];
            if pos == target {
                let cost = g_score[&pos];
                match best_cost {
                    None => best_cost = Some((pos, cost)),
                    Some((_, prev)) if cost < prev => best_cost = Some((pos, cost)),
                    _ => {}
                }
                active[ci] = false;

                let best_c = best_cost.unwrap().1;
                for (oci, heap) in open_sets.iter().enumerate() {
                    if active[oci] {
                        if let Some(top) = heap.peek() {
                            if top.f_score >= best_c {
                                active[oci] = false;
                            }
                        } else {
                            active[oci] = false;
                        }
                    }
                }
                continue;
            }
        }

        if !closed.insert(pos) {
            continue;
        }

        expanded += 1;
        if expanded > limits.work {
            return Err(PathError::ExceededWorkBudget {
                limit: limits.work,
                expanded,
            });
        }

        let current_g = match g_score.get(&pos) {
            Some(&g) => g,
            None => continue,
        };

        let current_depth = depth.get(&pos).copied().unwrap_or(0);
        if current_depth >= limits.path_len {
            hit_path_len_limit = true;
            continue;
        }

        let from_surface = walkability::derive_surface_type(world, face_data, pos);

        for (neighbor, dist_scaled) in
            walkability::ground_neighbors(world, face_data, pos, footprint)
        {
            if closed.contains(&neighbor) {
                continue;
            }

            let to_surface = walkability::derive_surface_type(world, face_data, neighbor);
            let edge_type = walkability::derive_edge_type(from_surface, to_surface, pos, neighbor);

            if let Some(allowed) = speeds.allowed_edges
                && !allowed.contains(&edge_type)
            {
                continue;
            }

            let tpv = match tpv_for_edge_type(edge_type, speeds) {
                Some(t) => t,
                None => continue,
            };

            let move_cost = (dist_scaled * tpv) as i64;
            let tentative_g = current_g + move_cost;

            if tentative_g < g_score.get(&neighbor).copied().unwrap_or(i64::MAX) {
                g_score.insert(neighbor, tentative_g);
                depth.insert(neighbor, current_depth + 1);

                for (oci, heap) in open_sets.iter_mut().enumerate() {
                    if !active[oci] {
                        continue;
                    }
                    let h =
                        octile_heuristic_3d(neighbor, candidates[candidate_indices[oci]], walk_tpv);
                    let f = tentative_g + h;
                    if let Some((_, best_c)) = best_cost
                        && f >= best_c
                    {
                        continue;
                    }
                    heap.push(OpenEntry {
                        pos: neighbor,
                        f_score: f,
                    });
                }
            }
        }
    }

    match best_cost {
        Some((coord, _)) => Ok(coord),
        None if hit_path_len_limit => Err(PathError::ExceededPathLen {
            limit: limits.path_len,
        }),
        None => Err(PathError::Unreachable),
    }
}

/// Find the nearest reachable candidate from `start` by ground travel cost.
///
/// **Prefer `SimState::find_nearest()`** unless you need direct control over
/// ground pathfinding parameters. `find_nearest` handles species dispatch,
/// speed lookup, footprint lookup, and index mapping automatically.
///
/// Delegates to `nearest_astar_ground` (interleaved A*).
pub fn nearest_ground(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    start: VoxelCoord,
    candidates: &[VoxelCoord],
    speeds: &GroundSpeeds,
    opts: &PathOpts,
    footprint: [u8; 3],
) -> Result<VoxelCoord, PathError> {
    nearest_astar_ground(world, face_data, start, candidates, speeds, opts, footprint)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::EdgeType;
    use crate::types::{VoxelCoord, VoxelType};
    use crate::world::VoxelWorld;

    /// Default speeds for tests: walk_tpv=1, climb_tpv=2, no ladders, all edges.
    fn test_speeds() -> GroundSpeeds<'static> {
        GroundSpeeds {
            walk_tpv: 1,
            climb_tpv: Some(2),
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: None,
        }
    }

    /// Speeds with an edge filter.
    fn test_speeds_filtered(allowed: &[EdgeType]) -> GroundSpeeds<'_> {
        GroundSpeeds {
            walk_tpv: 1,
            climb_tpv: Some(2),
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: Some(allowed),
        }
    }

    // -----------------------------------------------------------------------
    // PathOpts / resolve_limits tests
    // -----------------------------------------------------------------------

    #[test]
    fn path_opts_default_is_auto() {
        let opts = PathOpts::default();
        assert_eq!(opts.path_len, PathLenLimit::Auto);
        assert_eq!(opts.work, WorkBudget::Auto);
    }

    #[test]
    fn path_opts_builder_sets_exact() {
        let opts = PathOpts::default().with_path_len(500).with_work(10000);
        assert_eq!(opts.path_len, PathLenLimit::Exact(500));
        assert_eq!(opts.work, WorkBudget::Exact(10000));
    }

    #[test]
    fn resolve_limits_exact_passthrough() {
        let opts = PathOpts::default().with_path_len(42).with_work(999);
        let r = resolve_limits(&opts, 1000, false, 1);
        assert_eq!(r.path_len, 42);
        assert_eq!(r.work, 999);
    }

    #[test]
    fn resolve_limits_auto_single_target_navgraph() {
        let opts = PathOpts::default();
        // manhattan=10 → path_len = 10*3 + 100 = 130, work = 130*30 = 3900
        let r = resolve_limits(&opts, 10, false, 1);
        assert_eq!(r.path_len, 130);
        assert_eq!(r.work, 3900);
    }

    #[test]
    fn resolve_limits_auto_single_target_flight() {
        let opts = PathOpts::default();
        // manhattan=10 → path_len = 130, work = 130*100 = 13000
        let r = resolve_limits(&opts, 10, true, 1);
        assert_eq!(r.path_len, 130);
        assert_eq!(r.work, 13000);
    }

    #[test]
    fn resolve_limits_auto_nearest_scales_with_candidates() {
        let opts = PathOpts::default();
        // manhattan=10, n=4 → path_len=130, base_work=3900, isqrt(4)=2 → work=3900*3=11700
        let r = resolve_limits(&opts, 10, false, 4);
        assert_eq!(r.path_len, 130);
        assert_eq!(r.work, 11700);

        // n=16 → isqrt(16)=4 → work=3900*5=19500
        let r2 = resolve_limits(&opts, 10, false, 16);
        assert_eq!(r2.work, 19500);

        // n=1 → no scaling (single target)
        let r3 = resolve_limits(&opts, 10, false, 1);
        assert_eq!(r3.work, 3900);
    }

    #[test]
    fn resolve_limits_exact_work_not_scaled_by_candidates() {
        // When work is Exact, candidate count should NOT scale it.
        let opts = PathOpts::default().with_work(1000);
        let r = resolve_limits(&opts, 10, false, 100);
        assert_eq!(r.work, 1000);
    }

    #[test]
    fn isqrt_correctness() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(8), 2);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(100), 10);
        assert_eq!(isqrt(u32::MAX), 65535);
    }
    // -----------------------------------------------------------------------
    // Flight pathfinding tests
    // -----------------------------------------------------------------------

    /// Create a small empty world (all air) for testing.
    fn empty_world(sx: u32, sy: u32, sz: u32) -> VoxelWorld {
        VoxelWorld::new(sx, sy, sz)
    }

    const FP1: [u8; 3] = [1, 1, 1];
    const FP2: [u8; 3] = [2, 2, 2];

    #[test]
    fn fly_same_start_and_goal() {
        let world = empty_world(16, 16, 16);
        let pos = VoxelCoord::new(5, 5, 5);
        let result = astar_fly(&world, pos, pos, 250, &PathOpts::default(), FP1).unwrap();
        assert_eq!(result.positions, vec![pos]);
        assert_eq!(result.total_cost, 0);
    }

    #[test]
    fn fly_straight_line_path() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);
        let result = astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).unwrap();
        assert_eq!(result.positions.len(), 6);
        assert_eq!(result.positions[0], start);
        assert_eq!(result.positions[5], goal);
        assert_eq!(result.total_cost, 5 * 1024 * 250);
    }

    #[test]
    fn fly_diagonal_path() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 2, 2);
        let goal = VoxelCoord::new(5, 5, 5);
        let result = astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).unwrap();
        assert_eq!(result.positions.len(), 4);
        assert_eq!(result.total_cost, 3 * 1773 * 250);
    }

    #[test]
    fn fly_blocked_by_solid() {
        let mut world = empty_world(8, 8, 8);
        for y in 0..8 {
            for z in 0..8 {
                world.set(VoxelCoord::new(4, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(2, 4, 4);
        let goal = VoxelCoord::new(6, 4, 4);
        assert!(astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).is_err());
    }

    #[test]
    fn fly_path_around_obstacle() {
        let mut world = empty_world(16, 16, 16);
        for y in 3..8 {
            for z in 3..8 {
                world.set(VoxelCoord::new(5, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(3, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);
        let result = astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).unwrap();
        assert_eq!(*result.positions.first().unwrap(), start);
        assert_eq!(*result.positions.last().unwrap(), goal);
        assert!(result.total_cost > 0);
    }

    #[test]
    fn fly_solid_start_returns_none() {
        let mut world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        world.set(start, VoxelType::Trunk);
        let goal = VoxelCoord::new(5, 5, 5);
        assert!(astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).is_err());
    }

    #[test]
    fn fly_solid_goal_returns_none() {
        let mut world = empty_world(8, 8, 8);
        let goal = VoxelCoord::new(3, 3, 3);
        world.set(goal, VoxelType::Trunk);
        let start = VoxelCoord::new(5, 5, 5);
        assert!(astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).is_err());
    }

    #[test]
    fn fly_out_of_bounds_returns_none() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(5, 5, 5);
        let goal = VoxelCoord::new(100, 5, 5);
        assert!(astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).is_err());
    }

    #[test]
    fn fly_max_path_len_cutoff() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);

        let opts = |pl| PathOpts::default().with_path_len(pl).with_work(u32::MAX);

        // Path is 5 steps. max_path_len=5 should succeed.
        let result = astar_fly(&world, start, goal, 250, &opts(5), FP1);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().positions.len(), 6);

        // max_path_len=4 should fail (path requires 5 steps).
        let result = astar_fly(&world, start, goal, 250, &opts(4), FP1);
        assert!(result.is_err());

        // max_path_len=0 for same position should succeed.
        let result = astar_fly(&world, start, start, 250, &opts(0), FP1);
        assert!(result.is_ok());
    }

    #[test]
    fn fly_through_leaves_and_fruit() {
        let mut world = empty_world(8, 8, 8);
        world.set(VoxelCoord::new(4, 4, 4), VoxelType::Leaf);
        let start = VoxelCoord::new(3, 4, 4);
        let goal = VoxelCoord::new(5, 4, 4);
        let result = astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).unwrap();
        assert_eq!(result.positions.len(), 3);
        assert_eq!(result.positions[1], VoxelCoord::new(4, 4, 4));
        assert_eq!(result.total_cost, 2 * 1024 * 250);

        world.set(VoxelCoord::new(4, 4, 4), VoxelType::Fruit);
        let result = astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).unwrap();
        assert_eq!(result.positions.len(), 3);
        assert_eq!(result.positions[1], VoxelCoord::new(4, 4, 4));
    }

    #[test]
    fn fly_through_ladders() {
        let mut world = empty_world(8, 8, 8);
        world.set(VoxelCoord::new(4, 4, 4), VoxelType::WoodLadder);
        let start = VoxelCoord::new(3, 4, 4);
        let goal = VoxelCoord::new(5, 4, 4);
        let result = astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1).unwrap();
        assert_eq!(result.positions.len(), 3);
    }

    #[test]
    fn fly_footprint_2x2x2_straight_line() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);
        let result = astar_fly(&world, start, goal, 250, &PathOpts::default(), FP2).unwrap();
        assert_eq!(result.positions.len(), 6);
        assert_eq!(result.total_cost, 5 * 1024 * 250);
    }

    #[test]
    fn fly_footprint_2x2x2_blocked_by_partial_obstruction() {
        let mut world = empty_world(16, 16, 16);
        for y in 0..16 {
            for z in 0..16 {
                world.set(VoxelCoord::new(5, y, z), VoxelType::Trunk);
            }
        }
        world.set(VoxelCoord::new(5, 5, 5), VoxelType::Air);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(8, 5, 5);

        let result_1x1 = astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1);
        assert!(result_1x1.is_ok(), "1x1x1 should find path through gap");

        assert!(
            astar_fly(&world, start, goal, 250, &PathOpts::default(), FP2).is_err(),
            "2x2x2 should not fit through 1-voxel gap"
        );
    }

    #[test]
    fn fly_footprint_2x2x2_blocked_at_start() {
        let mut world = empty_world(8, 8, 8);
        world.set(VoxelCoord::new(3, 4, 3), VoxelType::Trunk);
        let start = VoxelCoord::new(2, 3, 2);
        let goal = VoxelCoord::new(5, 3, 5);
        assert!(astar_fly(&world, start, goal, 250, &PathOpts::default(), FP2).is_err());
    }

    #[test]
    fn fly_footprint_2x2x2_at_world_boundary() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(7, 3, 3);
        let goal = VoxelCoord::new(5, 3, 3);
        assert!(
            astar_fly(&world, start, goal, 250, &PathOpts::default(), FP2).is_err(),
            "2x2x2 at world boundary should not start (footprint out of bounds)"
        );
        let start2 = VoxelCoord::new(3, 3, 3);
        let goal2 = VoxelCoord::new(7, 3, 3);
        assert!(
            astar_fly(&world, start2, goal2, 250, &PathOpts::default(), FP2).is_err(),
            "2x2x2 at world boundary should not reach goal (footprint out of bounds)"
        );
        let result = astar_fly(
            &world,
            VoxelCoord::new(3, 3, 3),
            VoxelCoord::new(7, 3, 3),
            250,
            &PathOpts::default(),
            FP1,
        );
        assert!(result.is_ok(), "1x1x1 should reach world boundary");
    }

    // -----------------------------------------------------------------------
    // nearest_fly tests
    // -----------------------------------------------------------------------

    #[test]
    fn nearest_fly_finds_closest() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(5, 5, 5);
        let near = VoxelCoord::new(7, 5, 5); // 2 steps
        let far = VoxelCoord::new(12, 5, 5); // 7 steps

        let result = nearest_fly(&world, start, &[near, far], 250, &PathOpts::default(), FP1);
        assert_eq!(result, Ok(near));
    }

    #[test]
    fn nearest_fly_start_is_candidate() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        let result = nearest_fly(&world, start, &[start], 250, &PathOpts::default(), FP1);
        assert_eq!(result, Ok(start));
    }

    #[test]
    fn nearest_fly_no_candidates() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        let result = nearest_fly(&world, start, &[], 250, &PathOpts::default(), FP1);
        assert!(result.is_err());
    }

    #[test]
    fn nearest_fly_unreachable_candidate() {
        let mut world = empty_world(8, 8, 8);
        // Full wall blocking.
        for y in 0..8 {
            for z in 0..8 {
                world.set(VoxelCoord::new(4, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(2, 4, 4);
        let goal = VoxelCoord::new(6, 4, 4);
        let result = nearest_fly(&world, start, &[goal], 250, &PathOpts::default(), FP1);
        assert!(result.is_err());
    }

    #[test]
    fn nearest_fly_skips_unreachable_picks_reachable() {
        let mut world = empty_world(16, 16, 16);
        // Wall blocking access to one candidate.
        for y in 0..16 {
            for z in 0..16 {
                world.set(VoxelCoord::new(10, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(5, 5, 5);
        let blocked = VoxelCoord::new(12, 5, 5);
        let reachable = VoxelCoord::new(7, 5, 5);

        let result = nearest_fly(
            &world,
            start,
            &[blocked, reachable],
            250,
            &PathOpts::default(),
            FP1,
        );
        assert_eq!(result, Ok(reachable));
    }

    // -----------------------------------------------------------------------
    // nearest_astar_fly tests
    // -----------------------------------------------------------------------

    #[test]
    fn nearest_astar_fly_finds_closest() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(5, 5, 5);
        let near = VoxelCoord::new(7, 5, 5); // 2 steps
        let far = VoxelCoord::new(12, 5, 5); // 7 steps

        let result = nearest_astar_fly(&world, start, &[near, far], 250, &PathOpts::default(), FP1);
        assert_eq!(result, Ok(near));
    }

    #[test]
    fn nearest_astar_fly_start_is_candidate() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        let result = nearest_astar_fly(&world, start, &[start], 250, &PathOpts::default(), FP1);
        assert_eq!(result, Ok(start));
    }

    #[test]
    fn nearest_astar_fly_no_candidates() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        let result = nearest_astar_fly(&world, start, &[], 250, &PathOpts::default(), FP1);
        assert!(result.is_err());
    }

    #[test]
    fn nearest_astar_fly_unreachable_candidate() {
        let mut world = empty_world(8, 8, 8);
        for y in 0..8 {
            for z in 0..8 {
                world.set(VoxelCoord::new(4, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(2, 4, 4);
        let goal = VoxelCoord::new(6, 4, 4);
        let result = nearest_astar_fly(&world, start, &[goal], 250, &PathOpts::default(), FP1);
        assert!(result.is_err());
    }

    #[test]
    fn nearest_astar_fly_skips_unreachable_picks_reachable() {
        let mut world = empty_world(16, 16, 16);
        for y in 0..16 {
            for z in 0..16 {
                world.set(VoxelCoord::new(10, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(5, 5, 5);
        let blocked = VoxelCoord::new(12, 5, 5);
        let reachable = VoxelCoord::new(7, 5, 5);

        let result = nearest_astar_fly(
            &world,
            start,
            &[blocked, reachable],
            250,
            &PathOpts::default(),
            FP1,
        );
        assert_eq!(result, Ok(reachable));
    }

    #[test]
    fn nearest_astar_fly_max_path_len_cutoff() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let near = VoxelCoord::new(4, 5, 5); // 2 steps
        let far = VoxelCoord::new(7, 5, 5); // 5 steps

        let opts = |pl| PathOpts::default().with_path_len(pl).with_work(u32::MAX);

        // max_path_len=3 reaches near (2 steps) but not far (5 steps).
        let result = nearest_astar_fly(&world, start, &[near, far], 250, &opts(3), FP1);
        assert_eq!(result, Ok(near));

        // max_path_len=1 reaches neither (near is 2 steps).
        let result = nearest_astar_fly(&world, start, &[near, far], 250, &opts(1), FP1);
        assert!(result.is_err());
    }

    #[test]
    fn nearest_astar_fly_agrees_with_sequential() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(5, 5, 5);
        // Candidates at different distances so there's a unique winner.
        let c1 = VoxelCoord::new(8, 5, 5); // 3 steps
        let c2 = VoxelCoord::new(5, 10, 5); // 5 steps
        let c3 = VoxelCoord::new(5, 5, 12); // 7 steps

        let candidates = &[c1, c2, c3];

        // Independent sequential reference: run point-to-point A* to each
        // candidate and pick the cheapest. This is the old nearest_fly logic
        // before it was changed to delegate to interleaved A*.
        let sequential = {
            let mut best: Option<(VoxelCoord, i64)> = None;
            for &c in candidates {
                if let Ok(r) = astar_fly(&world, start, c, 250, &PathOpts::default(), FP1) {
                    match best {
                        None => best = Some((c, r.total_cost)),
                        Some((_, prev)) if r.total_cost < prev => {
                            best = Some((c, r.total_cost));
                        }
                        _ => {}
                    }
                }
            }
            best.map(|(coord, _)| coord)
        };

        let interleaved =
            nearest_astar_fly(&world, start, candidates, 250, &PathOpts::default(), FP1);
        assert_eq!(sequential, interleaved.ok());
    }

    #[test]
    fn nearest_astar_fly_start_not_flyable() {
        let mut world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        world.set(start, VoxelType::Trunk);
        let goal = VoxelCoord::new(5, 5, 5);

        let result = nearest_astar_fly(&world, start, &[goal], 250, &PathOpts::default(), FP1);
        assert!(result.is_err());
    }

    #[test]
    fn nearest_astar_fly_candidate_not_flyable_filtered() {
        let mut world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(5, 5, 5);
        let blocked = VoxelCoord::new(8, 5, 5);
        let reachable = VoxelCoord::new(5, 8, 5);
        world.set(blocked, VoxelType::Trunk);

        // blocked candidate is filtered out in pre-filter, reachable wins.
        let result = nearest_astar_fly(
            &world,
            start,
            &[blocked, reachable],
            250,
            &PathOpts::default(),
            FP1,
        );
        assert_eq!(result, Ok(reachable));
    }

    #[test]
    fn nearest_astar_fly_all_candidates_unflyable() {
        let mut world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        let c1 = VoxelCoord::new(5, 5, 5);
        let c2 = VoxelCoord::new(6, 6, 6);
        world.set(c1, VoxelType::Trunk);
        world.set(c2, VoxelType::Trunk);

        let result = nearest_astar_fly(&world, start, &[c1, c2], 250, &PathOpts::default(), FP1);
        assert!(result.is_err());
    }

    #[test]
    fn nearest_astar_fly_large_footprint() {
        let mut world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let near = VoxelCoord::new(7, 5, 5);
        let far = VoxelCoord::new(12, 5, 5);

        // Both reachable for 2x2x2 in open world.
        let result = nearest_astar_fly(&world, start, &[near, far], 250, &PathOpts::default(), FP2);
        assert_eq!(result, Ok(near));

        // Block the path with a wall that 2x2x2 can't pass through.
        for y in 0..16 {
            for z in 0..16 {
                world.set(VoxelCoord::new(5, y, z), VoxelType::Trunk);
            }
        }
        // Near is now on the other side of the wall.
        let result = nearest_astar_fly(&world, start, &[near, far], 250, &PathOpts::default(), FP2);
        assert!(result.is_err());
    }
    // -----------------------------------------------------------------------
    // Work budget and specific error variant tests (once-over additions)
    // -----------------------------------------------------------------------

    #[test]
    fn astar_fly_work_budget_exceeded() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(12, 5, 5);

        // Work budget of 1 — can only expand start node, won't reach goal.
        let opts = PathOpts::default().with_path_len(u32::MAX).with_work(1);
        let result = astar_fly(&world, start, goal, 250, &opts, FP1);
        assert!(matches!(result, Err(PathError::ExceededWorkBudget { .. })));

        // Generous budget — should find path.
        let opts = PathOpts::default()
            .with_path_len(u32::MAX)
            .with_work(100_000);
        let result = astar_fly(&world, start, goal, 250, &opts, FP1);
        assert!(result.is_ok());
    }

    #[test]
    fn astar_fly_start_blocked_returns_specific_error() {
        let mut world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        world.set(start, VoxelType::Trunk);
        let goal = VoxelCoord::new(5, 5, 5);
        assert_eq!(
            astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1),
            Err(PathError::StartBlockedByFootprint)
        );
    }

    #[test]
    fn astar_fly_goal_blocked_returns_specific_error() {
        let mut world = empty_world(8, 8, 8);
        let goal = VoxelCoord::new(3, 3, 3);
        world.set(goal, VoxelType::Trunk);
        let start = VoxelCoord::new(5, 5, 5);
        assert_eq!(
            astar_fly(&world, start, goal, 250, &PathOpts::default(), FP1),
            Err(PathError::TargetNotOnGraph)
        );
    }

    #[test]
    fn astar_fly_path_len_exceeded_returns_specific_error() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);
        // Path is 5 steps. max_path_len=4 should give ExceededPathLen.
        let opts = PathOpts::default().with_path_len(4).with_work(u32::MAX);
        assert_eq!(
            astar_fly(&world, start, goal, 250, &opts, FP1),
            Err(PathError::ExceededPathLen { limit: 4 })
        );
    }

    #[test]
    fn nearest_fly_no_candidates_returns_no_targets() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        assert_eq!(
            nearest_fly(&world, start, &[], 250, &PathOpts::default(), FP1),
            Err(PathError::NoTargets)
        );
    }

    #[test]
    fn nearest_astar_fly_work_budget_exceeded() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let far = VoxelCoord::new(12, 5, 5);

        // Work budget of 1 — won't reach far candidate.
        let opts = PathOpts::default().with_path_len(u32::MAX).with_work(1);
        let result = nearest_astar_fly(&world, start, &[far], 250, &opts, FP1);
        assert!(matches!(result, Err(PathError::ExceededWorkBudget { .. })));

        // Generous budget — should find it.
        let opts = PathOpts::default()
            .with_path_len(u32::MAX)
            .with_work(100_000);
        let result = nearest_astar_fly(&world, start, &[far], 250, &opts, FP1);
        assert_eq!(result, Ok(far));
    }

    #[test]
    fn nearest_astar_fly_start_blocked_returns_specific_error() {
        let mut world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        world.set(start, VoxelType::Trunk);
        let goal = VoxelCoord::new(5, 5, 5);
        assert_eq!(
            nearest_astar_fly(&world, start, &[goal], 250, &PathOpts::default(), FP1),
            Err(PathError::StartBlockedByFootprint)
        );
    }

    #[test]
    fn nearest_astar_fly_all_candidates_unflyable_returns_no_targets() {
        let mut world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        let c1 = VoxelCoord::new(5, 5, 5);
        let c2 = VoxelCoord::new(6, 6, 6);
        world.set(c1, VoxelType::Trunk);
        world.set(c2, VoxelType::Trunk);
        assert_eq!(
            nearest_astar_fly(&world, start, &[c1, c2], 250, &PathOpts::default(), FP1),
            Err(PathError::NoTargets)
        );
    }
    #[test]
    fn resolve_limits_saturation() {
        // Verify Auto with extreme manhattan doesn't overflow.
        let opts = PathOpts::default();
        let r = resolve_limits(&opts, u32::MAX, false, 1);
        assert_eq!(r.path_len, u32::MAX); // saturating_mul(3) + 100 saturates
        assert_eq!(r.work, u32::MAX); // u32::MAX * 30 saturates

        let r = resolve_limits(&opts, u32::MAX, true, 1);
        assert_eq!(r.work, u32::MAX); // u32::MAX * 100 saturates

        // With many candidates — still saturates.
        let r = resolve_limits(&opts, u32::MAX, false, 100);
        assert_eq!(r.work, u32::MAX);
    }

    #[test]
    fn resolve_limits_auto_with_zero_manhattan() {
        // When manhattan=0, the +100 additive constant prevents zero budgets.
        let opts = PathOpts::default();
        let r = resolve_limits(&opts, 0, false, 1);
        assert_eq!(r.path_len, 100);
        assert_eq!(r.work, 3000); // 100 * 30

        let r = resolve_limits(&opts, 0, true, 1);
        assert_eq!(r.work, 10000); // 100 * 100
    }

    #[test]
    fn resolve_limits_exact_path_len_with_auto_work() {
        // Auto work should derive from the exact path_len, not manhattan.
        let opts = PathOpts::default().with_path_len(50);
        let r = resolve_limits(&opts, 1000, false, 1);
        assert_eq!(r.path_len, 50);
        assert_eq!(r.work, 1500); // 50 * 30, not 1000*3+100 * 30
    }
    #[test]
    fn nearest_astar_fly_max_path_len_exceeded_returns_specific_error() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let far = VoxelCoord::new(7, 5, 5); // 5 steps away

        // path_len=3 is too short to reach far.
        let opts = PathOpts::default().with_path_len(3).with_work(u32::MAX);
        let result = nearest_astar_fly(&world, start, &[far], 250, &opts, FP1);
        assert_eq!(result, Err(PathError::ExceededPathLen { limit: 3 }));
    }

    // -----------------------------------------------------------------------
    // Ground A* tests (voxel-direct)
    // -----------------------------------------------------------------------

    /// Create a world with a flat dirt floor at y=floor_y. Air above, dirt at
    /// floor_y, solid below. Walkable positions are at y=floor_y+1.
    fn ground_world(sx: u32, sy: u32, sz: u32, floor_y: i32) -> VoxelWorld {
        let mut world = VoxelWorld::new(sx, sy, sz);
        for x in 0..sx as i32 {
            for z in 0..sz as i32 {
                world.set(VoxelCoord::new(x, floor_y, z), VoxelType::Dirt);
            }
        }
        world
    }

    /// Empty face data (no buildings).
    fn no_faces() -> BTreeMap<VoxelCoord, crate::types::FaceData> {
        BTreeMap::new()
    }

    #[test]
    fn astar_ground_same_start_and_goal() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let pos = VoxelCoord::new(5, 6, 5);
        let result = astar_ground(
            &world,
            &fd,
            pos,
            pos,
            &test_speeds(),
            &PathOpts::default(),
            FP1,
        );
        let path = result.unwrap();
        assert_eq!(path.positions, vec![pos]);
        assert_eq!(path.total_cost, 0);
    }

    #[test]
    fn astar_ground_straight_line_on_flat_dirt() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let start = VoxelCoord::new(2, 6, 5);
        let goal = VoxelCoord::new(5, 6, 5);
        let result = astar_ground(
            &world,
            &fd,
            start,
            goal,
            &test_speeds(),
            &PathOpts::default(),
            FP1,
        );
        let path = result.unwrap();
        // Path should start at start and end at goal.
        assert_eq!(*path.positions.first().unwrap(), start);
        assert_eq!(*path.positions.last().unwrap(), goal);
        // All positions should be at y=6 (walking on flat ground).
        assert!(path.positions.iter().all(|p| p.y == 6));
        assert!(path.total_cost > 0);
    }

    #[test]
    fn astar_ground_start_not_walkable() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        // y=5 is dirt (solid) — not walkable.
        let start = VoxelCoord::new(5, 5, 5);
        let goal = VoxelCoord::new(8, 6, 5);
        let result = astar_ground(
            &world,
            &fd,
            start,
            goal,
            &test_speeds(),
            &PathOpts::default(),
            FP1,
        );
        assert_eq!(result, Err(PathError::StartNotOnGraph));
    }

    #[test]
    fn astar_ground_goal_not_walkable() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let start = VoxelCoord::new(5, 6, 5);
        // y=10 is open air with no adjacent solid — not walkable.
        let goal = VoxelCoord::new(5, 10, 5);
        let result = astar_ground(
            &world,
            &fd,
            start,
            goal,
            &test_speeds(),
            &PathOpts::default(),
            FP1,
        );
        assert_eq!(result, Err(PathError::TargetNotOnGraph));
    }

    #[test]
    fn astar_ground_diagonal_movement() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let start = VoxelCoord::new(2, 6, 2);
        let goal = VoxelCoord::new(5, 6, 5);
        let result = astar_ground(
            &world,
            &fd,
            start,
            goal,
            &test_speeds(),
            &PathOpts::default(),
            FP1,
        );
        let path = result.unwrap();
        assert_eq!(*path.positions.first().unwrap(), start);
        assert_eq!(*path.positions.last().unwrap(), goal);
        // Diagonal movement should give a shorter path (3 diagonal steps)
        // than pure cardinal (6 steps).
        assert!(
            path.positions.len() <= 5,
            "diagonal path should be short, got {} steps",
            path.positions.len()
        );
    }

    #[test]
    fn astar_ground_wall_blocks_path() {
        // Create a wall that forces a detour.
        let mut world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        // Build a solid wall at x=5, z=2..8, y=6..8
        for z in 2..8 {
            for y in 6..9 {
                world.set(VoxelCoord::new(5, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(3, 6, 5);
        let goal = VoxelCoord::new(7, 6, 5);
        let result = astar_ground(
            &world,
            &fd,
            start,
            goal,
            &test_speeds(),
            &PathOpts::default(),
            FP1,
        );
        let path = result.unwrap();
        assert_eq!(*path.positions.first().unwrap(), start);
        assert_eq!(*path.positions.last().unwrap(), goal);
        // The path should go around the wall, so it should be longer than 4 steps.
        assert!(path.positions.len() > 5, "path should detour around wall");
    }

    #[test]
    fn astar_ground_climb_via_trunk() {
        // Create a world with dirt floor at y=5 and trunk surface for climbing.
        let mut world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        // Place trunk at x=5, y=5..10 to create climbable surface.
        for y in 5..10 {
            world.set(VoxelCoord::new(5, y, 5), VoxelType::Trunk);
        }
        // Start on the ground, goal above the trunk.
        let start = VoxelCoord::new(4, 6, 5);
        // Position at y=9, next to trunk at y=8 — walkable because adjacent to trunk.
        let goal = VoxelCoord::new(4, 9, 5);
        let speeds = test_speeds(); // climb_tpv = Some(2)
        let result = astar_ground(&world, &fd, start, goal, &speeds, &PathOpts::default(), FP1);
        let path = result.unwrap();
        assert_eq!(*path.positions.first().unwrap(), start);
        assert_eq!(*path.positions.last().unwrap(), goal);
        // Cost should reflect climbing (tpv=2 for climb edges).
        assert!(path.total_cost > 0);
    }

    #[test]
    fn astar_ground_edge_filter_blocks_climb() {
        // Same setup as climb test, but species cannot climb.
        let mut world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        for y in 5..10 {
            world.set(VoxelCoord::new(5, y, 5), VoxelType::Trunk);
        }
        let start = VoxelCoord::new(4, 6, 5);
        let goal = VoxelCoord::new(4, 9, 5);
        // Only allow Ground and BranchWalk — no climbing.
        let allowed = [EdgeType::Ground, EdgeType::BranchWalk];
        let speeds = test_speeds_filtered(&allowed);
        let result = astar_ground(&world, &fd, start, goal, &speeds, &PathOpts::default(), FP1);
        // Should fail — no path without climbing.
        assert!(result.is_err());
    }

    #[test]
    fn astar_ground_work_budget_exceeded() {
        let world = ground_world(32, 16, 32, 5);
        let fd = no_faces();
        let start = VoxelCoord::new(2, 6, 2);
        let goal = VoxelCoord::new(28, 6, 28);
        // Tiny work budget.
        let opts = PathOpts::default().with_work(10);
        let result = astar_ground(&world, &fd, start, goal, &test_speeds(), &opts, FP1);
        match result {
            Err(PathError::ExceededWorkBudget { .. }) => {} // expected
            other => panic!("expected ExceededWorkBudget, got {other:?}"),
        }
    }

    #[test]
    fn nearest_ground_finds_closest() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let start = VoxelCoord::new(5, 6, 5);
        let near = VoxelCoord::new(6, 6, 5);
        let far = VoxelCoord::new(10, 6, 5);
        let result = nearest_ground(
            &world,
            &fd,
            start,
            &[far, near],
            &test_speeds(),
            &PathOpts::default(),
            FP1,
        );
        assert_eq!(result.unwrap(), near);
    }

    #[test]
    fn nearest_ground_empty_candidates() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let start = VoxelCoord::new(5, 6, 5);
        let result = nearest_ground(
            &world,
            &fd,
            start,
            &[],
            &test_speeds(),
            &PathOpts::default(),
            FP1,
        );
        assert_eq!(result, Err(PathError::NoTargets));
    }

    #[test]
    fn nearest_ground_start_is_candidate() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let start = VoxelCoord::new(5, 6, 5);
        let result = nearest_ground(
            &world,
            &fd,
            start,
            &[start, VoxelCoord::new(8, 6, 5)],
            &test_speeds(),
            &PathOpts::default(),
            FP1,
        );
        assert_eq!(result.unwrap(), start);
    }

    #[test]
    fn test_astar_ground_2x2x2_flat_dirt() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let start = VoxelCoord::new(2, 6, 5);
        let goal = VoxelCoord::new(5, 6, 5);
        let result = astar_ground(
            &world,
            &fd,
            start,
            goal,
            &test_speeds(),
            &PathOpts::default(),
            [2, 2, 2],
        );
        let path = result.expect("should find path for 2x2x2 on flat dirt");
        assert_eq!(*path.positions.first().unwrap(), start);
        assert_eq!(*path.positions.last().unwrap(), goal);
        assert!(
            path.positions.iter().all(|p| p.y == 6),
            "All positions should be at y=6 on flat ground"
        );
    }
}
