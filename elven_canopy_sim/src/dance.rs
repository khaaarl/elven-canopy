// Dance generator: produces choreographed movement plans for group dances.
//
// A dance plan is a sequence of "figures" (advance & retire, ring rotation,
// swap, set-in-place) arranged end-to-end to fill a given number of music
// beats. Each figure is a pure function that takes starting positions and
// produces waypoints — (tick, slot, position) triples telling the sim where
// each dancer should be and when.
//
// The generator is deterministic: same inputs + PRNG seed = same plan. It
// runs once when a dance activity enters Executing phase, producing a
// `DancePlan` stored in the `ActivityDanceData` extension table. During
// execution, each creature's activation checks the plan for due waypoints.
//
// V1 is sequential: one figure at a time, all participants. No per-step
// collision avoidance needed. V2 (future) will overlap simultaneous figures
// with collision-free interlocking paths.
//
// Related files: `sim/activity.rs` (activity lifecycle integration),
// `db.rs` (ActivityDanceData table), `types.rs` (VoxelCoord).

use serde::{Deserialize, Serialize};

use crate::types::VoxelCoord;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A complete choreography plan for a dance activity.
///
/// Generated once when the activity enters `Executing` phase. Each
/// participant's activation tick consults the plan for due waypoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DancePlan {
    /// The starting formation (positions for each participant slot).
    pub formation: Formation,
    /// Ordered sequence of figures comprising the dance.
    pub figures: Vec<PlannedFigure>,
    /// Total duration of the plan in sim ticks (from execution_start_tick).
    pub total_ticks: u64,
    /// Per-slot sorted waypoint lists for efficient cursor-based lookup.
    /// `slot_waypoints[slot]` is a `Vec<Waypoint>` sorted by tick.
    pub slot_waypoints: Vec<Vec<Waypoint>>,
}

/// Starting formation for the dance — assigns each participant slot a position.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Formation {
    pub kind: FormationKind,
    /// One position per participant slot. Index = slot index.
    pub positions: Vec<VoxelCoord>,
}

/// The type of formation used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FormationKind {
    /// Two parallel lines facing each other along the long axis.
    LongwiseSet,
    /// Participants arranged on the perimeter of a rectangle.
    Ring,
    /// Everyone stands in place (too cramped for real figures).
    Cramped,
}

/// A single figure within the dance plan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedFigure {
    pub kind: FigureKind,
    /// Sim tick when this figure begins (absolute).
    pub start_tick: u64,
    /// Duration in sim ticks.
    pub duration_ticks: u64,
    /// Waypoints for all participants during this figure.
    pub waypoints: Vec<Waypoint>,
}

/// The type of dance figure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FigureKind {
    /// A line of elves slides forward N tiles, then back.
    AdvanceAndRetire,
    /// Elves on a rectangular perimeter shift positions CW or CCW.
    RingRotation,
    /// Two elves exchange positions.
    Swap,
    /// Hold position (filler / transition / cramped floor).
    SetInPlace,
}

/// A single movement instruction: move a participant to a position at a tick.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waypoint {
    /// Absolute sim tick when this movement occurs.
    pub tick: u64,
    /// Which participant slot this applies to.
    pub slot: usize,
    /// Target voxel position.
    pub position: VoxelCoord,
}

// ---------------------------------------------------------------------------
// Formation selection
// ---------------------------------------------------------------------------

/// Select a formation for the given floor dimensions and participant count.
///
/// The floor is a rectangle of `width` (x-axis) x `depth` (z-axis) voxels
/// at y-level `floor_y`. Positions are absolute voxel coordinates starting
/// at `(anchor_x, floor_y, anchor_z)`.
///
/// Formation preference order:
/// 1. **LongwiseSet** — two facing lines, needs width >= 3, even count,
///    and enough length (depth >= count/2).
/// 2. **Ring** — perimeter of a rectangle, needs width >= 3, depth >= 3,
///    and count <= perimeter cell count = 2*(width-1) + 2*(depth-1).
/// 3. **Cramped** — everyone stands in place (fallback).
pub fn select_formation(
    anchor_x: i32,
    anchor_z: i32,
    floor_y: i32,
    width: i32,
    depth: i32,
    count: usize,
    rng: &mut elven_canopy_prng::GameRng,
) -> Formation {
    if count == 0 {
        return Formation {
            kind: FormationKind::Cramped,
            positions: Vec::new(),
        };
    }

    // Try longwise set: two parallel lines along the depth axis.
    if let Some(f) = try_longwise_set(anchor_x, anchor_z, floor_y, width, depth, count) {
        return f;
    }

    // Try ring: participants on the perimeter of a rectangle.
    if let Some(f) = try_ring(anchor_x, anchor_z, floor_y, width, depth, count) {
        return f;
    }

    // Fallback: cramped — assign grid positions.
    cramped_formation(anchor_x, anchor_z, floor_y, width, depth, count, rng)
}

/// Two parallel lines facing each other across the width, centered.
/// Needs width >= 3 (room for two lines with a gap), even participant count,
/// and enough depth to space participants along the lines.
fn try_longwise_set(
    anchor_x: i32,
    anchor_z: i32,
    floor_y: i32,
    width: i32,
    depth: i32,
    count: usize,
) -> Option<Formation> {
    if width < 3 || count < 2 || !count.is_multiple_of(2) {
        return None;
    }

    let pairs = count / 2;
    // Each pair occupies one row along the depth axis; we want at least
    // one empty row between pairs for visual spacing, plus margins.
    // Minimum depth: pairs (one row each) — no spacing required for V1,
    // but we need at least `pairs` rows.
    if pairs as i32 > depth {
        return None;
    }

    // Place the two lines at x-offsets: left line at anchor_x + 0,
    // right line at anchor_x + width - 1 (facing each other).
    let left_x = anchor_x;
    let right_x = anchor_x + width - 1;

    // Center the pairs along the depth axis.
    let start_z = anchor_z + (depth - pairs as i32) / 2;

    let mut positions = Vec::with_capacity(count);
    for i in 0..pairs {
        let z = start_z + i as i32;
        positions.push(VoxelCoord::new(left_x, floor_y, z));
        positions.push(VoxelCoord::new(right_x, floor_y, z));
    }

    Some(Formation {
        kind: FormationKind::LongwiseSet,
        positions,
    })
}

/// Participants on the perimeter of a rectangle.
/// Needs width >= 3, depth >= 3, and count <= perimeter cell count.
fn try_ring(
    anchor_x: i32,
    anchor_z: i32,
    floor_y: i32,
    width: i32,
    depth: i32,
    count: usize,
) -> Option<Formation> {
    if width < 3 || depth < 3 {
        return None;
    }

    let perimeter_cells = 2 * (width - 1) + 2 * (depth - 1);
    if count > perimeter_cells as usize {
        return None;
    }

    // Generate all perimeter positions in clockwise order:
    // top edge (left to right), right edge (top+1 to bottom),
    // bottom edge (right-1 to left), left edge (bottom-1 to top+1).
    let mut perimeter = Vec::with_capacity(perimeter_cells as usize);

    // Top edge: z = anchor_z, x from anchor_x to anchor_x + width - 1
    for x in anchor_x..anchor_x + width {
        perimeter.push(VoxelCoord::new(x, floor_y, anchor_z));
    }
    // Right edge: x = anchor_x + width - 1, z from anchor_z + 1 to anchor_z + depth - 1
    for z in anchor_z + 1..anchor_z + depth {
        perimeter.push(VoxelCoord::new(anchor_x + width - 1, floor_y, z));
    }
    // Bottom edge: z = anchor_z + depth - 1, x from anchor_x + width - 2 to anchor_x
    for x in (anchor_x..anchor_x + width - 1).rev() {
        perimeter.push(VoxelCoord::new(x, floor_y, anchor_z + depth - 1));
    }
    // Left edge: x = anchor_x, z from anchor_z + depth - 2 to anchor_z + 1
    for z in (anchor_z + 1..anchor_z + depth - 1).rev() {
        perimeter.push(VoxelCoord::new(anchor_x, floor_y, z));
    }

    // Evenly space `count` participants around the perimeter.
    let total = perimeter.len();
    let mut positions = Vec::with_capacity(count);
    for i in 0..count {
        let idx = i * total / count;
        positions.push(perimeter[idx]);
    }

    Some(Formation {
        kind: FormationKind::Ring,
        positions,
    })
}

/// Fallback formation: place participants in a grid pattern on the floor.
fn cramped_formation(
    anchor_x: i32,
    anchor_z: i32,
    floor_y: i32,
    width: i32,
    depth: i32,
    count: usize,
    _rng: &mut elven_canopy_prng::GameRng,
) -> Formation {
    let mut positions = Vec::with_capacity(count);
    let mut placed = 0;
    'outer: for z in 0..depth {
        for x in 0..width {
            if placed >= count {
                break 'outer;
            }
            positions.push(VoxelCoord::new(anchor_x + x, floor_y, anchor_z + z));
            placed += 1;
        }
    }
    Formation {
        kind: FormationKind::Cramped,
        positions,
    }
}

// ---------------------------------------------------------------------------
// Figure vocabulary
// ---------------------------------------------------------------------------

/// Timing parameters for drift-free waypoint tick computation.
///
/// Waypoint ticks are computed from an absolute step index relative to the
/// figure's first step: `start_tick + step * tick_numerator / tick_denominator`.
/// This avoids cumulative rounding from a pre-computed `ticks_per_step`.
struct TickParams {
    tick_numerator: u64,
    tick_denominator: u64,
}

impl TickParams {
    /// Compute the absolute tick for the given step offset within a figure.
    fn tick_at(&self, figure_start_tick: u64, step_offset: u64) -> u64 {
        figure_start_tick + step_offset * self.tick_numerator / self.tick_denominator
    }
}

/// Output of a figure generator: waypoints to execute + ending positions.
struct FigureResult {
    waypoints: Vec<Waypoint>,
    /// One per slot — where each participant ends up after this figure.
    ending_positions: Vec<VoxelCoord>,
}

/// Advance and retire: a group of elves slides forward by `advance_steps`
/// tiles in the +z direction (toward the center of a longwise set), then
/// slides back to starting positions. Total duration = 2 * advance_steps
/// dance steps.
///
/// Works for any formation — all participants advance along z.
fn figure_advance_and_retire(
    positions: &[VoxelCoord],
    advance_steps: i32,
    start_tick: u64,
    tp: &TickParams,
) -> FigureResult {
    let mut waypoints = Vec::new();

    for (slot, &start_pos) in positions.iter().enumerate() {
        // Advance phase: move +z one tile per step.
        for s in 1..=advance_steps {
            waypoints.push(Waypoint {
                tick: tp.tick_at(start_tick, s as u64),
                slot,
                position: VoxelCoord::new(start_pos.x, start_pos.y, start_pos.z + s),
            });
        }
        // Retire phase: move back -z one tile per step.
        for s in 1..=advance_steps {
            let z_offset = advance_steps - s;
            waypoints.push(Waypoint {
                tick: tp.tick_at(start_tick, advance_steps as u64 + s as u64),
                slot,
                position: VoxelCoord::new(start_pos.x, start_pos.y, start_pos.z + z_offset),
            });
        }
    }

    FigureResult {
        waypoints,
        ending_positions: positions.to_vec(),
    }
}

/// Ring rotation: all participants shift one position around the perimeter
/// in order (slot 0 -> slot 1's position, slot 1 -> slot 2's position, etc.,
/// last slot -> slot 0's position). One dance step.
fn figure_ring_rotation(
    positions: &[VoxelCoord],
    start_tick: u64,
    tp: &TickParams,
) -> FigureResult {
    let n = positions.len();
    if n == 0 {
        return FigureResult {
            waypoints: Vec::new(),
            ending_positions: Vec::new(),
        };
    }

    let mut waypoints = Vec::with_capacity(n);
    let mut ending = Vec::with_capacity(n);
    let tick = tp.tick_at(start_tick, 1);

    for slot in 0..n {
        let target = positions[(slot + 1) % n];
        waypoints.push(Waypoint {
            tick,
            slot,
            position: target,
        });
        ending.push(target);
    }

    FigureResult {
        waypoints,
        ending_positions: ending,
    }
}

/// Swap: pairs of adjacent slots exchange positions. If odd count, the last
/// participant holds in place. Duration = 1 dance step.
fn figure_swap(positions: &[VoxelCoord], start_tick: u64, tp: &TickParams) -> FigureResult {
    let n = positions.len();
    let mut waypoints = Vec::with_capacity(n);
    let mut ending = positions.to_vec();
    let tick = tp.tick_at(start_tick, 1);

    let mut i = 0;
    while i + 1 < n {
        // Swap slot i and slot i+1.
        waypoints.push(Waypoint {
            tick,
            slot: i,
            position: positions[i + 1],
        });
        waypoints.push(Waypoint {
            tick,
            slot: i + 1,
            position: positions[i],
        });
        ending[i] = positions[i + 1];
        ending[i + 1] = positions[i];
        i += 2;
    }

    FigureResult {
        waypoints,
        ending_positions: ending,
    }
}

/// Set in place: all participants hold their positions. Duration = `steps`
/// dance steps. Produces no waypoints (positions don't change).
fn figure_set_in_place(positions: &[VoxelCoord], _steps: u64) -> FigureResult {
    FigureResult {
        waypoints: Vec::new(),
        ending_positions: positions.to_vec(),
    }
}

/// Duration of a figure in dance steps.
fn figure_duration_steps(kind: FigureKind, advance_steps: i32) -> u64 {
    match kind {
        FigureKind::AdvanceAndRetire => 2 * advance_steps as u64,
        FigureKind::RingRotation => 1,
        FigureKind::Swap => 1,
        FigureKind::SetInPlace => 2, // Filler — hold for 2 steps.
    }
}

// ---------------------------------------------------------------------------
// Plan generation
// ---------------------------------------------------------------------------

/// Parameters for dance plan generation, bundled to satisfy clippy's
/// too-many-arguments limit.
pub struct DancePlanParams {
    pub anchor_x: i32,
    pub anchor_z: i32,
    pub floor_y: i32,
    pub width: i32,
    pub depth: i32,
    pub participant_count: usize,
    pub song_length_beats: u64,
    pub tempo_multiplier: u64,
    pub execution_start_tick: u64,
    pub ticks_per_second: u64,
    pub tempo_bpm: u64,
}

/// Generate a complete dance plan.
///
/// The plan fills `song_length_beats` (eighth-note beats) subdivided by
/// `tempo_multiplier` into dance steps. Waypoint ticks are absolute,
/// computed from `execution_start_tick`.
///
/// Deterministic: same inputs + PRNG state = same plan.
pub fn generate_dance_plan(
    params: &DancePlanParams,
    rng: &mut elven_canopy_prng::GameRng,
) -> DancePlan {
    let DancePlanParams {
        anchor_x,
        anchor_z,
        floor_y,
        width,
        depth,
        participant_count,
        song_length_beats,
        tempo_multiplier,
        execution_start_tick,
        ticks_per_second,
        tempo_bpm,
    } = *params;
    let formation = select_formation(
        anchor_x,
        anchor_z,
        floor_y,
        width,
        depth,
        participant_count,
        rng,
    );

    let total_steps = song_length_beats * tempo_multiplier;
    if total_steps == 0 || participant_count == 0 {
        return DancePlan {
            formation,
            figures: Vec::new(),
            total_ticks: 0,
            slot_waypoints: Vec::new(),
        };
    }

    // Compute ticks per dance step using the drift-free formula from the
    // design doc. We store the per-step divisor for waypoint tick computation.
    // waypoint_tick = execution_start_tick + step_index * 60 * tps / (bpm * 2 * multiplier)
    // We precompute the denominator.
    let tick_numerator = 60 * ticks_per_second;
    let tick_denominator = tempo_bpm * 2 * tempo_multiplier;
    let tp = TickParams {
        tick_numerator,
        tick_denominator,
    };

    let mut figures = Vec::new();
    let mut current_step: u64 = 0;
    let mut current_positions = formation.positions.clone();

    let floor_z_max = anchor_z + depth - 1;

    while current_step < total_steps {
        let remaining = total_steps - current_step;

        // Maximum advance distance: must not push any participant past
        // the floor edge (z < anchor_z + depth). Recomputed each figure
        // since positions shift.
        let max_z = current_positions.iter().map(|p| p.z).max().unwrap_or(0);
        let max_advance = (floor_z_max - max_z).clamp(0, 4);

        // Pick a figure based on formation kind and remaining space.
        let (kind, advance) = pick_figure(
            &formation.kind,
            &current_positions,
            remaining,
            max_advance,
            rng,
        );

        let duration_steps = figure_duration_steps(kind, advance);
        // Clamp to remaining steps.
        let actual_steps = duration_steps.min(remaining);

        let figure_start_tick =
            execution_start_tick + current_step * tick_numerator / tick_denominator;

        let result = execute_figure(kind, &current_positions, advance, figure_start_tick, &tp);

        let figure_duration_ticks = actual_steps * tick_numerator / tick_denominator;

        figures.push(PlannedFigure {
            kind,
            start_tick: figure_start_tick,
            duration_ticks: figure_duration_ticks,
            waypoints: result.waypoints,
        });

        current_positions = result.ending_positions;
        current_step += actual_steps;
    }

    let total_ticks = total_steps * tick_numerator / tick_denominator;

    // Build per-slot sorted waypoint lists for efficient cursor-based lookup.
    let mut slot_waypoints: Vec<Vec<Waypoint>> =
        (0..participant_count).map(|_| Vec::new()).collect();
    for figure in &figures {
        for wp in &figure.waypoints {
            if wp.slot < participant_count {
                slot_waypoints[wp.slot].push(wp.clone());
            }
        }
    }
    // Sort each slot's waypoints by tick (they should already be mostly sorted,
    // but figures are appended sequentially so this is just a safety net).
    for wps in &mut slot_waypoints {
        wps.sort_by_key(|w| w.tick);
    }

    DancePlan {
        formation,
        figures,
        total_ticks,
        slot_waypoints,
    }
}

/// Pick a figure appropriate for the current formation and remaining steps.
fn pick_figure(
    formation_kind: &FormationKind,
    _positions: &[VoxelCoord],
    remaining_steps: u64,
    max_advance: i32,
    rng: &mut elven_canopy_prng::GameRng,
) -> (FigureKind, i32) {
    // Build candidate list based on formation type.
    let mut candidates: Vec<(FigureKind, i32)> = Vec::new();

    match formation_kind {
        FormationKind::LongwiseSet => {
            // Advance & retire with various depths.
            for a in 1..=max_advance {
                if figure_duration_steps(FigureKind::AdvanceAndRetire, a) <= remaining_steps {
                    candidates.push((FigureKind::AdvanceAndRetire, a));
                }
            }
            // Swap is always 1 step.
            if remaining_steps >= 1 {
                candidates.push((FigureKind::Swap, 0));
            }
        }
        FormationKind::Ring => {
            // Ring rotation (1 step).
            if remaining_steps >= 1 {
                candidates.push((FigureKind::RingRotation, 0));
            }
            // Swap.
            if remaining_steps >= 1 {
                candidates.push((FigureKind::Swap, 0));
            }
        }
        FormationKind::Cramped => {
            // Only set-in-place available.
        }
    }

    // Always include set-in-place as a filler.
    if remaining_steps >= 2 {
        candidates.push((FigureKind::SetInPlace, 0));
    }

    if candidates.is_empty() {
        // Very short remainder — just hold.
        return (FigureKind::SetInPlace, 0);
    }

    // Uniform random selection among candidates.
    let idx = rng.next_u64() as usize % candidates.len();
    candidates[idx]
}

/// Execute a figure, dispatching to the appropriate generator function.
fn execute_figure(
    kind: FigureKind,
    positions: &[VoxelCoord],
    advance_steps: i32,
    start_tick: u64,
    tp: &TickParams,
) -> FigureResult {
    match kind {
        FigureKind::AdvanceAndRetire => {
            figure_advance_and_retire(positions, advance_steps, start_tick, tp)
        }
        FigureKind::RingRotation => figure_ring_rotation(positions, start_tick, tp),
        FigureKind::Swap => figure_swap(positions, start_tick, tp),
        FigureKind::SetInPlace => figure_set_in_place(positions, 2),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dance_plan_serde_roundtrip() {
        let plan = DancePlan {
            formation: Formation {
                kind: FormationKind::LongwiseSet,
                positions: vec![
                    VoxelCoord::new(0, 51, 0),
                    VoxelCoord::new(1, 51, 0),
                    VoxelCoord::new(0, 51, 2),
                    VoxelCoord::new(1, 51, 2),
                ],
            },
            figures: vec![
                PlannedFigure {
                    kind: FigureKind::AdvanceAndRetire,
                    start_tick: 1000,
                    duration_ticks: 4000,
                    waypoints: vec![
                        Waypoint {
                            tick: 1000,
                            slot: 0,
                            position: VoxelCoord::new(0, 51, 1),
                        },
                        Waypoint {
                            tick: 2000,
                            slot: 0,
                            position: VoxelCoord::new(0, 51, 0),
                        },
                    ],
                },
                PlannedFigure {
                    kind: FigureKind::SetInPlace,
                    start_tick: 5000,
                    duration_ticks: 1000,
                    waypoints: vec![],
                },
            ],
            total_ticks: 6000,
            slot_waypoints: vec![
                vec![
                    Waypoint {
                        tick: 1000,
                        slot: 0,
                        position: VoxelCoord::new(0, 51, 1),
                    },
                    Waypoint {
                        tick: 2000,
                        slot: 0,
                        position: VoxelCoord::new(0, 51, 0),
                    },
                ],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
        };

        let json = serde_json::to_string(&plan).expect("serialize");
        let round: DancePlan = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(round.formation.kind, plan.formation.kind);
        assert_eq!(round.formation.positions, plan.formation.positions);
        assert_eq!(round.figures.len(), plan.figures.len());
        assert_eq!(round.figures[0].kind, plan.figures[0].kind);
        assert_eq!(round.figures[0].waypoints, plan.figures[0].waypoints);
        assert_eq!(round.total_ticks, plan.total_ticks);
    }

    #[test]
    fn formation_kind_serde_roundtrip() {
        for kind in [
            FormationKind::LongwiseSet,
            FormationKind::Ring,
            FormationKind::Cramped,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize");
            let round: FormationKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(round, kind);
        }
    }

    #[test]
    fn figure_kind_serde_roundtrip() {
        for kind in [
            FigureKind::AdvanceAndRetire,
            FigureKind::RingRotation,
            FigureKind::Swap,
            FigureKind::SetInPlace,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize");
            let round: FigureKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(round, kind);
        }
    }

    // -------------------------------------------------------------------
    // Formation tests
    // -------------------------------------------------------------------

    fn test_rng() -> elven_canopy_prng::GameRng {
        elven_canopy_prng::GameRng::new(42)
    }

    #[test]
    fn longwise_set_on_wide_floor() {
        let mut rng = test_rng();
        // 5 wide, 4 deep, 4 participants (2 pairs)
        let f = select_formation(10, 20, 51, 5, 4, 4, &mut rng);
        assert_eq!(f.kind, FormationKind::LongwiseSet);
        assert_eq!(f.positions.len(), 4);
        // Two lines: left at x=10, right at x=14
        for pos in &f.positions {
            assert!(
                pos.x == 10 || pos.x == 14,
                "expected x=10 or x=14, got {}",
                pos.x
            );
            assert_eq!(pos.y, 51);
        }
        // All positions within floor bounds
        for pos in &f.positions {
            assert!(pos.x >= 10 && pos.x < 15);
            assert!(pos.z >= 20 && pos.z < 24);
        }
    }

    #[test]
    fn longwise_set_rejected_odd_count() {
        let mut rng = test_rng();
        // Odd count -> can't do longwise set
        let f = select_formation(0, 0, 51, 5, 5, 3, &mut rng);
        assert_ne!(f.kind, FormationKind::LongwiseSet);
        assert_eq!(f.positions.len(), 3);
    }

    #[test]
    fn longwise_set_rejected_narrow_floor() {
        let mut rng = test_rng();
        // Width 2 -> too narrow for longwise set
        let f = select_formation(0, 0, 51, 2, 5, 4, &mut rng);
        assert_ne!(f.kind, FormationKind::LongwiseSet);
    }

    #[test]
    fn ring_formation_3x3() {
        let mut rng = test_rng();
        // 3x3 floor, 3 odd participants -> can't do longwise, try ring
        // Perimeter = 2*(3-1) + 2*(3-1) = 8 cells
        let f = select_formation(0, 0, 51, 3, 3, 5, &mut rng);
        assert_eq!(f.kind, FormationKind::Ring);
        assert_eq!(f.positions.len(), 5);
        // All positions on the perimeter of the 3x3 square
        for pos in &f.positions {
            assert_eq!(pos.y, 51);
            let on_edge = pos.x == 0 || pos.x == 2 || pos.z == 0 || pos.z == 2;
            assert!(on_edge, "position {:?} should be on perimeter", pos);
        }
    }

    #[test]
    fn ring_rejected_too_many_participants() {
        let mut rng = test_rng();
        // 3x3 perimeter = 8, but 9 odd participants (can't do longwise either)
        let f = select_formation(0, 0, 51, 3, 3, 9, &mut rng);
        // 9 > 8 perimeter cells, so ring is rejected -> cramped
        assert_eq!(f.kind, FormationKind::Cramped);
    }

    #[test]
    fn cramped_fallback_tiny_floor() {
        let mut rng = test_rng();
        // 2x2 floor, 3 participants -> too narrow for longwise, too small for ring
        let f = select_formation(5, 5, 51, 2, 2, 3, &mut rng);
        assert_eq!(f.kind, FormationKind::Cramped);
        assert_eq!(f.positions.len(), 3);
        // Positions within floor bounds
        for pos in &f.positions {
            assert!(pos.x >= 5 && pos.x < 7);
            assert!(pos.z >= 5 && pos.z < 7);
        }
    }

    #[test]
    fn formation_zero_participants() {
        let mut rng = test_rng();
        let f = select_formation(0, 0, 51, 5, 5, 0, &mut rng);
        assert_eq!(f.kind, FormationKind::Cramped);
        assert!(f.positions.is_empty());
    }

    #[test]
    fn all_positions_within_floor_bounds() {
        let mut rng = test_rng();
        // Test several configurations
        for (w, d, count) in [(3, 3, 4), (5, 5, 8), (4, 6, 6), (3, 3, 8), (10, 10, 3)] {
            let f = select_formation(10, 20, 51, w, d, count, &mut rng);
            assert_eq!(f.positions.len(), count, "w={w} d={d} count={count}");
            for pos in &f.positions {
                assert!(
                    pos.x >= 10 && pos.x < 10 + w,
                    "x={} out of [10, {}), w={w} d={d} count={count} kind={:?}",
                    pos.x,
                    10 + w,
                    f.kind
                );
                assert!(
                    pos.z >= 20 && pos.z < 20 + d,
                    "z={} out of [20, {}), w={w} d={d} count={count} kind={:?}",
                    pos.z,
                    20 + d,
                    f.kind
                );
                assert_eq!(pos.y, 51);
            }
        }
    }

    #[test]
    fn no_duplicate_positions_in_formation() {
        let mut rng = test_rng();
        for (w, d, count) in [(3, 3, 4), (5, 5, 8), (4, 6, 6), (3, 3, 8)] {
            let f = select_formation(0, 0, 51, w, d, count, &mut rng);
            let mut sorted = f.positions.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(
                sorted.len(),
                f.positions.len(),
                "duplicate positions in {:?} formation (w={w}, d={d}, count={count})",
                f.kind
            );
        }
    }

    // -------------------------------------------------------------------
    // Figure tests
    // -------------------------------------------------------------------

    fn test_tp() -> TickParams {
        // 500 ticks per step (equivalent to 60 BPM, 1000 tps, 1x multiplier).
        TickParams {
            tick_numerator: 500,
            tick_denominator: 1,
        }
    }

    #[test]
    fn advance_and_retire_returns_to_start() {
        let positions = vec![VoxelCoord::new(0, 51, 0), VoxelCoord::new(4, 51, 0)];
        let result = figure_advance_and_retire(&positions, 2, 1000, &test_tp());

        // Duration = 2 * 2 = 4 steps, so 4 waypoints per participant = 8 total.
        assert_eq!(result.waypoints.len(), 8);
        // Ending positions == starting positions.
        assert_eq!(result.ending_positions, positions);

        // Check slot 0's path: z goes 0 -> 1 -> 2 -> 1 -> 0
        let slot0: Vec<_> = result.waypoints.iter().filter(|w| w.slot == 0).collect();
        assert_eq!(slot0.len(), 4);
        assert_eq!(slot0[0].position.z, 1); // advance step 1
        assert_eq!(slot0[1].position.z, 2); // advance step 2
        assert_eq!(slot0[2].position.z, 1); // retire step 1
        assert_eq!(slot0[3].position.z, 0); // retire step 2 (back to start)

        // Check timing is correct
        assert_eq!(slot0[0].tick, 1500); // 1000 + 1*500
        assert_eq!(slot0[1].tick, 2000); // 1000 + 2*500
        assert_eq!(slot0[2].tick, 2500); // 1000 + 3*500
        assert_eq!(slot0[3].tick, 3000); // 1000 + 4*500
    }

    #[test]
    fn ring_rotation_shifts_all_one_position() {
        let positions = vec![
            VoxelCoord::new(0, 51, 0),
            VoxelCoord::new(1, 51, 0),
            VoxelCoord::new(1, 51, 1),
            VoxelCoord::new(0, 51, 1),
        ];
        let result = figure_ring_rotation(&positions, 1000, &test_tp());

        assert_eq!(result.waypoints.len(), 4);
        // Each slot moves to the next slot's position.
        assert_eq!(result.ending_positions[0], positions[1]);
        assert_eq!(result.ending_positions[1], positions[2]);
        assert_eq!(result.ending_positions[2], positions[3]);
        assert_eq!(result.ending_positions[3], positions[0]);
        // All at same tick.
        for w in &result.waypoints {
            assert_eq!(w.tick, 1500);
        }
    }

    #[test]
    fn ring_rotation_empty() {
        let result = figure_ring_rotation(&[], 1000, &test_tp());
        assert!(result.waypoints.is_empty());
        assert!(result.ending_positions.is_empty());
    }

    #[test]
    fn swap_pairs_exchange() {
        let positions = vec![
            VoxelCoord::new(0, 51, 0),
            VoxelCoord::new(4, 51, 0),
            VoxelCoord::new(0, 51, 2),
            VoxelCoord::new(4, 51, 2),
        ];
        let result = figure_swap(&positions, 1000, &test_tp());

        // 4 participants -> 2 pairs -> 4 waypoints.
        assert_eq!(result.waypoints.len(), 4);
        // Pair 0-1 swapped, pair 2-3 swapped.
        assert_eq!(result.ending_positions[0], positions[1]);
        assert_eq!(result.ending_positions[1], positions[0]);
        assert_eq!(result.ending_positions[2], positions[3]);
        assert_eq!(result.ending_positions[3], positions[2]);
    }

    #[test]
    fn swap_odd_count_last_holds() {
        let positions = vec![
            VoxelCoord::new(0, 51, 0),
            VoxelCoord::new(4, 51, 0),
            VoxelCoord::new(0, 51, 2),
        ];
        let result = figure_swap(&positions, 1000, &test_tp());

        // 3 participants -> 1 pair swapped, 1 holds.
        assert_eq!(result.waypoints.len(), 2); // Only the pair gets waypoints.
        assert_eq!(result.ending_positions[0], positions[1]);
        assert_eq!(result.ending_positions[1], positions[0]);
        assert_eq!(result.ending_positions[2], positions[2]); // Unchanged.
    }

    #[test]
    fn set_in_place_no_waypoints() {
        let positions = vec![VoxelCoord::new(0, 51, 0), VoxelCoord::new(1, 51, 0)];
        let result = figure_set_in_place(&positions, 4);

        assert!(result.waypoints.is_empty());
        assert_eq!(result.ending_positions, positions);
    }

    // -------------------------------------------------------------------
    // Plan generation tests
    // -------------------------------------------------------------------

    fn make_params(
        ax: i32,
        az: i32,
        w: i32,
        d: i32,
        count: usize,
        beats: u64,
        tempo_mult: u64,
    ) -> DancePlanParams {
        DancePlanParams {
            anchor_x: ax,
            anchor_z: az,
            floor_y: 51,
            width: w,
            depth: d,
            participant_count: count,
            song_length_beats: beats,
            tempo_multiplier: tempo_mult,
            execution_start_tick: 0,
            ticks_per_second: 1000,
            tempo_bpm: 60,
        }
    }

    #[test]
    fn plan_fills_beat_count() {
        let mut rng = test_rng();
        let plan = generate_dance_plan(&make_params(0, 0, 5, 5, 4, 16, 1), &mut rng);

        // At 60 BPM with eighth notes: ticks_per_beat = 1000*60/(60*2) = 500
        // 16 beats * 500 ticks = 8000 total ticks
        assert_eq!(plan.total_ticks, 8000);
        assert_eq!(plan.formation.positions.len(), 4);
        assert!(!plan.figures.is_empty(), "should have at least one figure");

        // All figure durations should sum to total_ticks.
        let sum: u64 = plan.figures.iter().map(|f| f.duration_ticks).sum();
        assert_eq!(sum, plan.total_ticks);
    }

    #[test]
    fn plan_deterministic() {
        let p = make_params(0, 0, 5, 5, 4, 16, 1);
        let mut rng1 = elven_canopy_prng::GameRng::new(123);
        let plan1 = generate_dance_plan(&p, &mut rng1);

        let mut rng2 = elven_canopy_prng::GameRng::new(123);
        let plan2 = generate_dance_plan(&p, &mut rng2);

        assert_eq!(plan1.figures.len(), plan2.figures.len());
        for (f1, f2) in plan1.figures.iter().zip(plan2.figures.iter()) {
            assert_eq!(f1.kind, f2.kind);
            assert_eq!(f1.start_tick, f2.start_tick);
            assert_eq!(f1.waypoints, f2.waypoints);
        }
    }

    #[test]
    fn plan_zero_beats() {
        let mut rng = test_rng();
        let plan = generate_dance_plan(&make_params(0, 0, 5, 5, 4, 0, 1), &mut rng);
        assert_eq!(plan.total_ticks, 0);
        assert!(plan.figures.is_empty());
    }

    #[test]
    fn plan_single_participant() {
        let mut rng = test_rng();
        let plan = generate_dance_plan(&make_params(0, 0, 3, 3, 1, 8, 1), &mut rng);
        assert_eq!(plan.formation.positions.len(), 1);
        assert!(plan.total_ticks > 0);
    }

    #[test]
    fn plan_waypoints_within_floor_bounds() {
        let mut rng = test_rng();
        let (ax, az, w, d) = (10, 20, 5, 5);
        let plan = generate_dance_plan(&make_params(ax, az, w, d, 4, 32, 1), &mut rng);

        for figure in &plan.figures {
            for wp in &figure.waypoints {
                assert!(
                    wp.position.x >= ax && wp.position.x < ax + w,
                    "waypoint x={} out of bounds [{}, {})",
                    wp.position.x,
                    ax,
                    ax + w
                );
                assert!(
                    wp.position.z >= az && wp.position.z < az + d,
                    "waypoint z={} out of bounds [{}, {})",
                    wp.position.z,
                    az,
                    az + d
                );
                assert_eq!(wp.position.y, 51);
            }
        }
    }

    #[test]
    fn plan_waypoints_within_bounds_many_participants() {
        for count in 2..=10 {
            let mut rng = elven_canopy_prng::GameRng::new(count as u64);
            let (ax, az, w, d) = (0, 0, 5, 5);
            let plan = generate_dance_plan(&make_params(ax, az, w, d, count, 32, 1), &mut rng);
            for figure in &plan.figures {
                for wp in &figure.waypoints {
                    assert!(
                        wp.position.x >= ax && wp.position.x < ax + w,
                        "count={count} x={} out of bounds",
                        wp.position.x,
                    );
                    assert!(
                        wp.position.z >= az && wp.position.z < az + d,
                        "count={count} z={} out of bounds",
                        wp.position.z,
                    );
                }
            }
        }
    }

    #[test]
    fn plan_with_tempo_multiplier() {
        let mut rng = test_rng();
        let plan = generate_dance_plan(&make_params(0, 0, 5, 5, 4, 8, 2), &mut rng);
        // ticks_per_step = 1000*60/(60*2*2) = 250
        // 16 steps * 250 = 4000
        assert_eq!(plan.total_ticks, 4000);
    }
}
