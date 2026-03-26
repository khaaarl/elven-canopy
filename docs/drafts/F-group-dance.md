# Dance Generator

**Status:** Draft — exploratory, many open questions
**Tracker item:** F-group-dance (in progress, unblocked by F-group-activity)
**Date:** 2026-03-25

## Vision

Elves gather on a flat rectangular floor and perform coordinated dances — groups of sprites sliding between voxel positions in intricate, synchronized geometric patterns. The long-term aspiration is something like a living Celtic knot: dozens of elves weaving through each other in precise interlocking paths that *almost* collide but never do, creating mesmerizing patterns that look distinctly non-human and unmistakably elven.

Scottish country dances are the loose real-world model. In Scottish dances, a finite vocabulary of "figures" (circle, chain, do-si-do, advance & retire, etc.) are sequenced into a complete dance. Each figure moves a subset of participants to new positions over a fixed number of bars. Our generator follows the same principle — a dance is a sequence of figures drawn from a vocabulary — but on a voxel grid, with a computer's ability to choreograph far more intricate simultaneous movement than any human dance master could coordinate.

The dance is synchronized to a song produced by the music generator. Both use eighth-note beats as their atomic time unit. The dance generator receives the song length and produces a choreography plan of matching duration.

## Current State

The group activity framework is implemented (`activity.rs`). `ActivityKind::Dance` exists with:
- Open recruitment (idle elves volunteer), player-civ + elf-only eligibility
- `DeparturePolicy::Continue` (dance keeps going if someone leaves)
- Late join allowed
- V1 choreographed execution: dance plan generated on Executing transition, creatures move via timed Move actions synchronized to music beats
- Composed music accompaniment via the music composition system
- Completion awards `DancedInGroup` thought to all participants
- Debug Dance button in F12 toolbar picks an existing dance hall and creates a linked dance activity

**What this feature replaces:** The stub execution with actual choreography — elves moving in coordinated patterns instead of standing in place accumulating progress ticks.

## Dance Hall Furnishing

Dances happen in dance halls. `FurnishingType::DanceHall` is a new furnishing type that designates a building as a dance venue. Unlike other furnishing types, dance halls have **no furniture** — the entire interior is an open floor for dancing.

Implementation notes:
- Add `DanceHall` variant to `FurnishingType` enum in `types.rs`.
- `furnish_structure()` in `construction.rs`: add an early-exit branch for DanceHall that sets the furnishing type on the structure and skips the furniture pipeline entirely (no `compute_furniture_positions`, no `Furniture` rows, no `Furnish` task). The building is immediately "furnished" on designation. `furniture_kind()` can return any value for DanceHall since the early-exit ensures it's never called in the furniture pipeline path.
- `display_str()` returns `"Dance Hall"`.
- Add `"DanceHall"` match arm in `sim_bridge.rs` `furnish_structure()` (string-to-enum parsing).
- Add to the furnishing picker in `structure_info_panel.gd`.
- Info panel display: the current format `"Dance Hall (0 items)"` looks wrong. Add a special case: when `planned_furniture_count == 0` and the building is fully furnished, display just the furnishing name with no furniture count.
- The usable dance floor is the full building interior: `floor_interior_positions()` on `CompletedStructure` gives all ground-floor voxels (width × depth positions starting at `anchor`). The door is just a face on a normal interior voxel, so no positions need excluding — all interior voxels are danceable.

### Debug Dance Button

Currently the debug dance button enters placement mode and creates a dance at an arbitrary voxel. This should change: clicking Debug Dance picks an arbitrary existing dance hall and creates a dance activity located at that building's interior. If no dance hall exists, the button does nothing (or shows a notification).

The activity's `location` should be a nav node inside the dance hall (same pattern as `furnish_structure` finding a nav node for task location). The dance generator receives the building's interior dimensions from the `CompletedStructure`.

## V1: Sequential Figures (Proof of Concept)

The first version keeps things simple: one figure plays at a time, involving all (or most) participants. No simultaneous overlapping movement, no collision detection needed.

### Timing

The atomic unit is the eighth-note beat, matching the music generator (`elven_canopy_music`). Music at the default 60 BPM plays quarter notes at 1/second, so eighth notes at 2/second — this is quite slow for dancing. The dance generator should support a **tempo multiplier** that subdivides the beat grid: at 2× multiplier, each eighth-note beat is divided into two dance steps (effectively sixteenth-note resolution), at 4× into four. All dance steps still align to a subdivision of the music's beat grid — no free-floating timing. Mixing subdivisions within a dance (some figures at 1× eighth notes, others at 2× sixteenth notes) would add visual rhythm variation.

Figures occupy whole numbers of dance steps. At 1× tempo, most figures would be 4, 8, or 16 steps (= eighth-note beats). At 2× tempo, those same figures play in half the musical time. The generator fills the song's beat count by sequencing figures end-to-end.

### Tick-Beat Mapping

The sim runs on discrete event ticks, not real-time. At plan generation time, the dance plan converts beats to sim ticks:

```
ticks_per_second = 1000 / config.tick_duration_ms   // currently 1000
ticks_per_beat = ticks_per_second * 60 / (tempo_bpm * 2)  // integer division
```

(The `* 2` is because we're counting eighth-note beats, not quarter notes. `tick_duration_ms` is 1 by default, giving 1000 ticks/second.) This is integer division — at 60 BPM, `ticks_per_beat` = 500 exactly. At non-round tempos (e.g., 90 BPM → 333), there is a small rounding error per beat.

With the tempo multiplier (1, 2, or 4), each beat is subdivided into `tempo_multiplier` dance steps. To prevent cumulative drift, waypoint ticks are computed from the absolute step index rather than accumulated:

```
waypoint_tick = execution_start_tick
              + step_index * 60 * ticks_per_second / (tempo_bpm * 2 * tempo_multiplier)
```

This keeps the last step within one tick of the mathematically correct time regardless of tempo.

Waypoints are stored with **absolute tick values**, not beat or step offsets. The sim sets elf positions at the exact tick each waypoint specifies. Godot handles visual interpolation between position changes.

This means the dance stays in sync with sim time regardless of speed — fast-forward simply executes ticks faster, and elves dance at the accelerated rate. The dance plan doesn't need to know about real time or playback speed.

### Formation

Before figures begin, elves need starting positions. The formation is determined by floor dimensions and participant count. The dance floor dimensions come from the dance hall's `CompletedStructure` fields: `width` (x-axis) and `depth` (z-axis). Since dance halls have no furniture, the entire interior is usable (`floor_interior_positions()` = width × depth voxels).

- **Longwise set** (W ≥ 3, participants even): two parallel lines along the long axis, partners facing across. This is the Scottish country dance default. Number of couples = participants / 2, needing ~2 tiles per couple along the long axis.
- **Ring** (W ≥ 3, D ≥ 3, participants ≤ perimeter cell count): elves on the perimeter of a rectangle. 4 elves = 2×2 corners. 8 elves = 3×3 ring (8 non-center cells). Larger rings on bigger floors are possible but look like square conveyor belts — which might be fine. Perimeter cell count is `2*(W-1) + 2*(D-1)`.
- **Grid** (any size, many participants): elves in a rectangular grid pattern with spacing. Good for advance & retire, less good for partner work.
- **Cramped** (tiny floor or very crowded): everyone stands roughly in place. Only "set in place" figures available.

Formation selection is a function of `(width, depth, count)` and could be random (weighted by suitability) or deterministic per seed. Open question: should the generator ever *reject* a dance (too many elves, floor too small), or always produce something, even if it's just elves swaying in place?

**Tentative answer:** always produce something. Graceful degradation over hard failure.

### Figure Vocabulary

These are conceptual descriptions of figures that could work on a voxel grid with position-only movement. All are tentative — the actual voxel paths will be discovered and refined during implementation. Grid constraints mean traditional dance figures won't translate directly; the names are evocative, not prescriptive.

**Line figures** (work with longwise set or grid formations):
- **Advance & retire** (4–8 steps): a row of elves slides forward N tiles, then back. Simple, visually clear as coordinated movement.
- **Chain / grand chain** (8–16 steps): two facing lines, elves weave past each other along the line, alternating which side they pass on. Looks like dancing even with pure sliding.

**Partner figures** (work with longwise set, need even count or explicit sit-out):
- **Swap** (2–4 steps): two elves exchange positions. The atomic partner move.
- **Do-si-do** (4–8 steps): two elves loop around each other via a rectangular path. The exact voxel geometry will depend on available space — may be a 4-step half-perimeter of a 2×2 area, or something simpler.

**Group figures** (work with ring or grid formations):
- **Ring rotation** (4–8 steps): elves on a rectangular perimeter all shift one position clockwise or counterclockwise. Looks best at 3×3 (8 elves). At 2×2 (4 elves), it's just a 4-way rotation.
- **Promenade** (4–8 steps): pairs walk side-by-side along a path, wrapping at ends.

**Filler:**
- **Set in place** (2–4 steps): elf holds position. Used for sitting out, transitions, or cramped floors.

Each figure is a pure function: given participant starting positions and the figure parameters, emit a list of `(tick_offset, elf_slot, new_voxel_position)` waypoints. Participants end a figure at well-defined positions, which become the starting positions for the next figure.

**Within-figure collision:** V1 does not enforce per-step collision avoidance within figures. If two elves briefly share a voxel during a figure (e.g., passing each other in a chain), this is visually acceptable — they're performing a coordinated dance move and will separate on the next step. V2 introduces strict per-beat collision avoidance.

### Generator Algorithm (V1)

```
input: floor_rect, participant_count, song_length_beats, tempo_multiplier, seed
output: DancePlan (formation + figure sequence with waypoints)

1. Pick a formation based on floor_rect and participant_count.
   Assign each participant a starting VoxelCoord.

2. current_step = 0
   total_steps = song_length_beats * tempo_multiplier
   current_positions = formation starting positions

3. While current_step < total_steps:
     a. Enumerate which figures are *compatible*:
        - Participants' current positions allow the figure's movement
        - All waypoints stay within floor_rect
        - Duration fits remaining steps (or can be truncated)
     b. Pick one (PRNG-weighted).
     c. Generate waypoints for chosen figure.
     d. Append figure to plan.
     e. Update current_positions to figure's ending positions.
     f. current_step += figure.duration_steps

4. Return DancePlan.
```

### Data Model (Tentative)

```
DancePlan {
    formation: Formation,
    figures: Vec<PlannedFigure>,
    total_ticks: u64,           // plan duration in sim ticks
}

Formation {
    kind: FormationKind,
    positions: Vec<VoxelCoord>,  // one per participant slot
}

PlannedFigure {
    kind: FigureKind,
    start_tick: u64,
    duration_ticks: u64,
    participants: Vec<usize>,        // indices into formation slots
    waypoints: Vec<Waypoint>,
}

Waypoint {
    tick: u64,              // absolute sim tick
    slot: usize,            // which participant
    position: VoxelCoord,   // where they move to
}
```

The sim steps through the plan: each activation, check if any waypoints are due for this creature's slot and execute them. To avoid O(total_waypoints) scans, each participant tracks a cursor (index into the sorted waypoint list) that advances monotonically. Elves slide between waypoints; Godot interpolates the visual movement.

All types must derive `Serialize`/`Deserialize` for save/load persistence.

### Integration with Activity System

The current stub in `execute_dance_behavior()` increments progress each activation tick. The real implementation replaces this:

1. **On transition to Executing:** Generate the `DancePlan` from the dance hall's dimensions, participant count, and sim PRNG. Store it in the `ActivityDanceData` extension table.
2. **During execution:** Activation-driven, fitting the existing architecture. Each time `execute_dance_behavior()` fires for a creature, it checks the plan for any due waypoints for that creature's slot (based on elapsed ticks since `execution_start_tick`), executing any that have been reached. **Movement timing is determined by the dance plan, not creature speed** — the absolute tick values in waypoints govern when elves move, regardless of their individual movement stats. Currently, creatures in activities are reactivated every tick (`schedule_reactivation` at tick+1), so waypoints are never missed. If this becomes a performance concern, reactivation could be scheduled at the next waypoint tick instead of every tick — but for V1 the per-tick approach is fine. The activation loop's normal preemption checks still apply, so creatures can be interrupted to flee, fight, etc.
3. **Completion:** The dance completes when the plan's duration has elapsed (elapsed ticks since `execution_start_tick >= total_ticks`). This replaces the current `progress >= total_cost` check.
4. **Interruption:** If a participant is preempted (e.g., by combat), they leave mid-dance. Their slot in the plan simply goes unexecuted for remaining waypoints. The dance continues per `DeparturePolicy::Continue`.
5. **Late join:** Currently `allows_late_join` is true for dances. For V1, late joiners are assigned the "set in place" filler — they stand at their assembly position for the remainder of the dance. The precomputed plan doesn't change; only original participants have choreographed paths. (Open question for V2: could the plan be re-generated or extended to incorporate latecomers?)

### Extension Table and Structure Link

The dance hall is linked to the activity via an `ActivityStructureRef` table, mirroring the `TaskStructureRef` / `TaskStructureRole` pattern used by tasks. Schema: `(activity_id: ActivityId, seq: u64 [auto_increment], structure_id: StructureId, role: ActivityStructureRole)`. For dances, the role is `ActivityStructureRole::DanceVenue`. The table has an FK on `activity_id` (cascade delete) and `structure_id`.

Dance-specific extension data:

```
ActivityDanceData {
    #[primary_key]
    activity_id: ActivityId,
    plan: DancePlan,                // precomputed choreography
}
```

The plan is generated once when the activity enters `Executing` phase, seeded from the sim PRNG. The dance hall's dimensions are read from the `CompletedStructure` via the activity-structure ref at that point. The extension table is persisted across save/load. Dance progress is derived from `Activity.execution_start_tick` (already stored on the base `Activity` row) — elapsed ticks = `current_tick - execution_start_tick`. No separate offset field needed.

### Where It Lives

A module within `elven_canopy_sim` — dance positions are sim state (where elves physically are), and the generator must be deterministic for the same reasons as everything else in the sim. Probably `sim/dance.rs` or a `sim/dance/` directory if it grows. The dance generator function is deterministic: `(floor_rect, participant_count, beat_count, tempo_multiplier, &mut GameRng) -> DancePlan`.

## V2: Celtic Knot Dances (Future)

V1 sequences figures one at a time. The exciting version runs multiple figures *simultaneously on overlapping space*, with per-step collision avoidance. This is where the Celtic knot visual emerges: interlocking rings rotating through each other, pairs threading through gaps in a moving formation, elves weaving in patterns that look impossibly intricate but are perfectly choreographed.

### What Changes

The core difference is **collision-aware choreography**. In V1, only one figure moves at a time, so collisions are limited to brief within-figure overlaps. In V2, multiple movement patterns overlap in space, and the generator must guarantee that no two elves share a voxel on any step.

This is closer to a constraint satisfaction problem:
- Each elf has a path (sequence of voxel positions per step)
- No two paths may coincide at the same step
- Paths should form visually appealing geometric patterns
- The overall pattern should tile/repeat in aesthetically pleasing ways

**Possible approaches** (all speculative):

- **Template-based weaving**: Define interlocking path templates (e.g., "two counter-rotating rings offset by 1 beat") and verify collision-freedom. This is the most controllable approach.
- **Constraint solver**: Define the desired pattern properties (symmetry, density, path smoothness) and search for collision-free path assignments. Could use simulated annealing like the music generator.
- **Layered generation**: Generate a "base" pattern (e.g., a ring rotation), then overlay additional patterns that thread through gaps in the base, checking collisions incrementally.

### Open Questions (V2)

These are genuinely uncertain — not "we'll figure it out later" but "the right answer depends on experimentation":

- How much of the Celtic knot effect comes from the *paths* vs. the *timing*? Two rings rotating through each other look like a knot because of when elves pass through shared space. The timing might matter more than the geometry.
- Should the generator understand musical structure (phrase boundaries, accent patterns) and align visual climaxes to musical ones? This would make dances feel choreographed rather than mechanical. Note: `generate_piece` currently returns only the `Grid` (note-level score); the `StructurePlan` (containing phrase boundaries, imitation points) is internal to the pipeline. Exposing it — or having the dance generator independently derive it via the same seed — would be a prerequisite for phrase-aligned choreography.
- Is per-step collision checking sufficient, or do we need interpolation checking? (If elf A is at (1,0) on step 3 and (0,0) on step 4, and elf B is at (0,0) on step 3 and (1,0) on step 4, they "pass through" each other. Is that a collision?)
- Non-rectangular floors: the tracker could eventually have a separate item for this. Irregular floor shapes (L-shaped rooms, circular clearings) would dramatically change which patterns are possible. But rectangular is enough for a long time.

## Relationship to Other Systems

- **Music generator** (`elven_canopy_music`): the song is generated first (via `generate_piece`), and the dance is then generated to match the song's `num_beats`. The dance generator is seeded deterministically from the activity's PRNG, which is derived from the sim seed. The same activity parameters (seed, floor, participants) always produce the same dance. Future phrase-aligned choreography would require exposing the music crate's `StructurePlan` (currently internal to `generate_piece`).
- **Group activities** (`activity.rs`): owns the lifecycle. `ActivityKind::Dance` is already implemented with open recruitment, continue-on-departure, and late join. The dance generator replaces the stub `execute_dance_behavior()`. Thoughts `EnjoyingDance` (during) and `DancedInGroup` (completion) are already wired.
- **Buildings**: the dance floor is a dance hall — a building furnished with `FurnishingType::DanceHall`. Floor dimensions come from `CompletedStructure` width/depth. The entire interior is usable (no furniture to work around).
- **Mood**: participating in a dance generates `EnjoyingDance` and `DancedInGroup` thoughts (already implemented). The quality/intricacy of the dance could scale the mood effect in the future. Spectator mood effects are a separate feature.
- **Concert hall** (F-bldg-concert): a related but distinct venue. Concert halls have benches (audience seating). Dance halls are open floor. A future feature might allow dances in concert halls too, but the bench furniture would constrain the usable floor area.

## What This Document Does NOT Cover

- How elves are recruited for a dance (group activity layer — already implemented)
- How elves get to the dance floor (GoTo tasks via activity assembly — already implemented)
- Musical accompaniment details (separate from dance generation)
- Sprite animation beyond sliding (future work, not needed for V1 or V2)
- Audience mechanics (elves watching a dance — separate feature)
- Non-rectangular dance floors (separate tracker item if pursued)
