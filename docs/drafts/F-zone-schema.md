# Zone Schema (F-zone-schema)

**Status:** Draft — decisions solidifying, open questions remain.

---

## Goal

Add zone identity to all spatial data so the save format supports multiple zones, while maintaining functional equivalence to the current single-zone game. This is the schema foundation for F-zone-world; no multi-zone simulation logic is included here.

## Key Concepts

**Zone (SimDb row):** Metadata for a location in the world. Fields: `zone_id: ZoneId`, `seed: u64`, `zone_type: ZoneType`, `zone_size: (u32, u32, u32)`, `floor_y: i32`. Every zone has a row whether or not its voxels are materialized. Note: the current `config.world_size` is misnamed — it is really the zone size. `floor_y` is also per-zone, not global. Constraint: `zone_size.1` (Y dimension) must be in `[1, 255]` due to the RLE column representation using `u8` Y coordinates internally.

**ZoneType:** A Rust enum. Starting variant: `GreatTreeForest`. Named clearly to indicate it is one of potentially many zone types. Future variants might include `LesserForest`, `Grassland`, etc.

**VoxelZone (materialized data):** The current `VoxelWorld` type, renamed. Contains the RLE voxel grid plus all zone-local state currently on SimState: `placed_voxels`, `carved_voxels`, `grassless`, `face_data`/`face_data_list`, `ladder_orientations`/`ladder_orientations_list`, `structure_voxels`, `mana_wasted_positions`. Only active zones have a VoxelZone — a world map may have huge numbers of zones that are never visited.

**SimState field:** `voxel_zones: BTreeMap<ZoneId, VoxelZone>`. Not `zones` — that would conflate the materialized voxel data with the DB row.

## Schema Changes

- **Zone table** in SimDb: `zone_id: ZoneId`, `seed: u64`, `zone_type: ZoneType`, `zone_size: (u32, u32, u32)`, `floor_y: i32`.
- **`zone_id: ZoneId`** (required) FK on all spatially-located tables except Creature: Tree, TreeFruit, Blueprint, CompletedStructure, GroundPile, Projectile, Task, Strut, Furniture, Activity. These entities are zone-bound and cannot exist outside a zone.
- **`zone_id: Option<ZoneId>`** on Creature only. **THIS IS A FINAL DECISION.** Creatures are the only entities that could plausibly be between zones. Using `Option` avoids a breaking schema migration when inter-zone transit is implemented in F-zone-world. All other spatial tables use required `ZoneId` because those entities are permanently zone-bound. Do not revisit this decision.
- **Index changes** (using F-tab-spatial-2, already landed):
  - *R\*-tree spatial index:* Creature's `creature_spatial` becomes a compound `(zone_id, position)` partial spatial index, filtering out `zone_id: None` and non-spatially-present creatures.
  - *B-tree unique indexes:* TreeFruit and GroundPile have unique indexes on `position` — these become compound `(zone_id, position)` unique indexes since coordinates can repeat across zones.
  - *Other spatial tables:* Tree, Blueprint, CompletedStructure, Projectile, Task, Strut, Furniture, Activity have no spatial indexes today and gain them only if query patterns demand it; for now, most are accessed by PK or FK lookup.
- **Child tables** (Thought, CreatureTrait, CreatureGenome, CreatureOpinion, PathAssignment, MoveAction, all Task extension tables including TaskHaulData/TaskAttackMoveData/TaskVoxelRef, ActivityParticipant, ActivityDanceData, ActivityStructureRef, etc.) do NOT get zone_id — they inherit zone identity from their parent via FK. Some child tables have VoxelCoord fields (e.g., TaskHaulData.destination_coord, TaskAttackMoveData.destination, MoveAction interpolation data, ActivityParticipant.assigned_position) — these coordinates are implicitly in the parent's zone.
- **Non-zone tables:** Civilization, FruitSpecies, CivRelationship, Player, SelectionGroup, MilitaryGroup, Inventory, ItemStack, ItemSubcomponent, ItemEnchantment, EnchantmentEffect, LogisticsWantRow, ActiveRecipe, ActiveRecipeTarget, MusicComposition, Notification, TameDesignation — these are either world-level, abstract, or inherit zone from a parent entity.
- **Non-spatial indexes** (Task.state, Blueprint.state, CompletedStructure.furnishing, etc.) deferred to F-zone-world. Current access patterns are PK lookups or `iter_all()` with manual filters — no indexed queries exist that would break. These indexes are transient and can be regenerated quickly when changed.

## Active Zone

`SimBridge` (GDext bridge) owns an `active_zone_id: ZoneId` field — this is client-side viewing state, not part of the deterministic sim or the save format. Set on init/load from `player_tree_id` → tree's `zone_id`. GDScript remains zone-unaware for F-zone-schema.

13 spatial `SimAction` variants carry VoxelCoords directly and gain a `zone_id: ZoneId` field: `DesignateBuild`, `DesignateBuilding`, `DesignateCarve`, `DesignateLadder`, `SpawnCreature`, `CreateTask`, `AddGroundPileItem`, `DirectedGoTo`, `AttackMove`, `GroupGoTo`, `GroupAttackMove`, `DebugSpawnProjectile`, `CreateActivity`. The bridge stamps `self.active_zone_id` when constructing these actions. The sim validates zone existence on every spatial command. Entity-targeted commands (`AttackCreature`, `AssignHome`, etc.) resolve zone from the entity's own `zone_id` column — no additional field needed.

The `voxel_zones` map on SimState should be private, with accessor methods (`voxel_zone(id)`, `voxel_zone_mut(id)`, `home_voxel_zone()`) to prevent accidental wrong-zone indexing. Command-sourced zone_ids from the bridge must use `.get()` with graceful rejection, not `[]` indexing that would panic on invalid input.

## Worldgen / Zonegen / Zone Manifestation

Current `run_worldgen` entangles world-level setup with zone-level generation. This splits into three layers:

- **Worldgen (`run_worldgen`):** The broadest function. Creates civs, diplomacy, fruit species, and Zone table rows in SimDb. Draws zone seeds from the world PRNG before any zone manifestation runs (determinism: future zones don't shift the sequence). Calls zone manifestation for the home zone.

- **Zone manifestation (`manifest_zone` or similar):** Given a Zone row (with seed, type, size, floor_y), materializes everything needed for a zone to be playable: generates the VoxelZone (terrain + trees as voxels), assigns tree IDs and inserts Tree rows into SimDb, assigns fruit species to trees, creates GreatTreeInfo if applicable, schedules zone heartbeats. This is NOT just geometry — it produces a fully integrated, playable zone with DB state. Fruit assignment uses the world PRNG (not zone PRNG) to maintain the current deterministic draw sequence.

- **Terrain/tree generation (internal to manifestation):** Pure geometry functions that produce voxel data and tree geometry structs. The current `generate_trees()` becomes a `GreatTreeForest`-specific function. `generate_lesser_trees()` gains an `Option<(i32, i32)>` main-tree exclusion parameter so zones without a great tree can reuse the placement logic.

For this task: worldgen creates one Zone row, then manifests it as the home zone.

## Save Format

Old saves break. No migration, no save_version field, no compatibility shim. F-save-stable is explicitly blocked by F-zone-schema — there is no save compatibility contract. The structural changes (`world` → `voxel_zones`, fields moving into VoxelZone, zone_id columns on all spatial tables) are too deep for a shim to be worthwhile.

## Implementation Ordering

**Prerequisite:** F-remove-navgraph should land first. Avoids migrating nav graph fields into VoxelZone only to delete them.

1. **VoxelWorld → VoxelZone rename.** Type alias bridge, then mechanical find-replace (~400 occurrences, 22 files). Parallelizable across worktrees (sim, graphics, gdext). Note: the graphics crate uses `VoxelWorld` in its public API — the type alias must be pub and re-exported. After this step, SimState has `world: VoxelZone` (just the type rename; the field is still called `world` until Step 4). (~3-5 commits)
2. **ZoneId type + Zone table.** Added to SimDb. Worldgen inserts one row. Additive only. (1 commit)
3. **Worldgen/manifestation split.** Extract `manifest_zone` from `run_worldgen`. Update test helpers. (1-2 commits)
4. **SimState restructure.** `world` → `voxel_zones: BTreeMap<ZoneId, VoxelZone>`. Move zone-local fields onto VoxelZone. Add private field + accessor methods. Staged: add new field + accessors alongside old, migrate callers, remove old field, update GDext bridge. Highest-risk step. (~3-5 commits)
5. **Add zone_id columns** to 11 spatial DB tables. Update compound spatial indexes. Move `zone_size` and `floor_y` from GameConfig to Zone table. (2-3 commits)
6. **Cleanup.** Break old saves with clear error in `from_json`. Final integration testing. (1-2 commits)

Total: ~12-18 commits. Step 4 is the critical path.

## Rejected Directions

- **World map as a Zone row** with a sentinel ID: violates type identity. The world map is a different concept and must be a different type.
- **`VoxelZone` containing a `VoxelWorld`:** the rename is *of* `VoxelWorld`, not a wrapper around it.
- **Single enlarged VoxelWorld with offset zones:** wastes space, complicates coordinates, no benefit over per-zone grids.
- **`zones:` as the SimState field name:** conflates materialized voxel data with the Zone DB concept.
- **Including `zone_id: None` rows in spatial indexes** (e.g. as their own R-tree partition): wastes recomputation. In-transit entities have no meaningful position; filter them out at the index level.
- **`materialized: bool` on Zone table:** redundant with `voxel_zones.contains_key()`. Synchronization hazard.
- **Zonegen returns pure geometry with no DB integration:** The manifestation function must produce a fully playable zone, not just voxels. Splitting DB integration from voxel generation creates an incomplete abstraction — fruit assignment, tree ID allocation, heartbeat scheduling all belong together.
- **Sim-authoritative viewing state:** Putting `viewing_zone_id` on the Player table pollutes the deterministic sim with UI state and adds unnecessary command latency.

## Known Landmines

These are implicit single-zone assumptions discovered in the codebase that must be addressed during or after this work:

- **`TriggerRaid`** (`command.rs`): Computes spawn positions from `config.world_size` and `config.floor_y`, assuming one world. Must use the target zone's `zone_size`/`floor_y` instead. For F-zone-schema (one zone) the behavior is unchanged, but the code path must be updated to read from the Zone row.
- **`GameConfig` single-zone fields:** `world_size`, `floor_y`, `tree_profile`, `lesser_trees` are top-level and implicitly per-zone. `zone_size` and `floor_y` move to the Zone table in this work. GameConfig keeps `default_zone_size` and `default_floor_y` as defaults for zone creation; callers that currently read `config.world_size`/`config.floor_y` must be migrated to read from the Zone row (via a zone accessor or by looking up the active zone). Tree profile and lesser tree config remain in GameConfig for now (only one zone type exists) but will need per-zone-type config in F-zone-world.
- **Event handlers reference zone-local data:** `GrassRegrowth` iterates `self.grassless`, `LogisticsHeartbeat` iterates all structures, `TreeHeartbeat` piggybacks all fruit-bearing trees. After zone-local data moves into VoxelZone, these handlers must access data through the zone accessor. For F-zone-schema (one zone) the behavior is identical, but the code paths change.
- **Borrow management in sim_bridge:** Moving `grassless` into VoxelZone means bridge code that needs both `&voxel_zone` and `&mut voxel_zone.grassless` must be careful about mutable borrows. `update_world_mesh()` calls both `drain_dirty_voxels()` (mut) and passes `&zone.grassless` to mesh cache — these cannot overlap.
- **Initial creature spawning:** `spawn_initial_creatures()` uses config-defined spawn positions (bare VoxelCoord, no zone). Must set `zone_id` to home zone on all spawned creatures.
- **`voxel_zones` accessor safety:** Command-sourced zone_ids from the bridge must not trigger panics. Use `.get()` with graceful rejection in `apply_command()`, even though `home_voxel_zone()` can safely unwrap (the home zone is always materialized).
- **Cross-crate re-export for VoxelZone:** The graphics crate uses `VoxelWorld` in its public API (~38 occurrences). The type alias during rename must be pub and re-exported through `elven_canopy_sim`'s public API.
- **Merge conflict risk:** The ~400-occurrence rename creates a large diff. Coordinate timing so no other feature branches are mid-flight on files that reference VoxelWorld.

## Resolved Questions

- **Event queue:** Stays global. Resolve zone from entity ID at dispatch time.
- **SimState zone-locality audit:**
  - *Zone-local, persisted* (moves to VoxelZone): `world` (the RLE voxel grid), `placed_voxels`, `carved_voxels`, `face_data_list`, `ladder_orientations_list`, `grassless`.
  - *Zone-local, transient* (`#[serde(skip)]`, rebuilt): `face_data`, `ladder_orientations`, `structure_voxels`, `mana_wasted_positions`.
  - *Global, persisted* (stays on SimState): `tick`, `rng`, `config`, `event_queue`, `db`, `next_structure_id`, `player_tree_id`, `player_civ_id`.
  - *Global, transient:* `species_table`, `lexicon`, `last_build_message`.
  - *Excluded:* `nav_graph`, `large_nav_graph` — removed by F-remove-navgraph before this work begins.
- **Test helpers:** `test_sim`/`flat_world_sim` produce a single-zone SimState. Add `home_voxel_zone()`/`home_voxel_zone_mut()` accessors so most tests don't need to think about zones. The VoxelWorld→VoxelZone rename is mechanical (~400 occurrences across sim, graphics, gdext).
- **NavGraph:** Excluded from this design — being removed in parallel (F-remove-navgraph).
- **`manifest_zone` signature:** Takes `&mut SimDb` directly (it inserts Tree rows, GreatTreeInfo, schedules heartbeats). Internal geometry functions are pure, but manifestation itself mutates DB state.

## Open Questions

- Per-zone-type config: how does `manifest_zone` get tree profiles, lesser tree config, etc.? From GameConfig? From the Zone row? From the ZoneType enum?
- `GreatTreeInfo` creation: currently tied to the home tree specifically. In `manifest_zone`, a `GreatTreeForest` zone creates a GreatTreeInfo for its main tree. Whether non-home zones can have great trees (and thus GreatTreeInfo) is an F-zone-world question.
