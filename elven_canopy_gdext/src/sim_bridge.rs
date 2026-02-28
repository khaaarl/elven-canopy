// GDExtension bridge class for the simulation.
//
// Exposes a `SimBridge` node that Godot scenes can use to create, step, and
// query the simulation. This is the sole interface between GDScript and the
// Rust sim — all sim interaction goes through methods on this class.
//
// ## What it exposes
//
// - **Lifecycle:** `init_sim(seed)`, `init_sim_with_tree_profile_json(seed, json)`,
//   `step_to_tick(tick)`, `current_tick()`, `is_initialized()`,
//   `tick_duration_ms()`.
// - **Save/load:** `save_game_json()` returns the sim state as a JSON string,
//   `load_game_json(json)` replaces the current sim from a JSON string.
//   File I/O is handled in GDScript via Godot's `user://` paths.
// - **World data:** `get_trunk_voxels()`, `get_branch_voxels()`,
//   `get_root_voxels()`, `get_leaf_voxels()`, `get_fruit_voxels()` — flat
//   `PackedInt32Array` of (x,y,z) triples for voxel mesh rendering.
// - **Creature positions:** `get_creature_positions(species_name, render_tick)`
//   — generic `PackedVector3Array` for billboard sprite placement, replacing
//   the per-species `get_elf_positions()` / `get_capybara_positions()` (which
//   remain as thin wrappers). The `render_tick` parameter (a fractional tick
//   computed by `main.gd` as `current_tick + accumulator_fraction`) enables
//   smooth interpolation between nav nodes via `Creature::interpolated_position()`.
// - **Creature info:** `get_creature_info(species_name, index, render_tick)` —
//   returns a `VarDictionary` with species, interpolated position (x/y/z),
//   task status, food level, and food_max for the creature at the given
//   species-filtered index. Used by the creature info panel for display and
//   follow-mode tracking.
// - **Task list:** `get_active_tasks()` — returns a `VarArray` of
//   `VarDictionary`, one per non-complete task. Each dict includes short/full
//   ID, kind, state, progress/total_cost, location coordinates, and an
//   assignees array with creature species and index. Used by `task_panel.gd`.
// - **Nav nodes:** `get_all_nav_nodes()`, `get_ground_nav_nodes()` — for
//   debug visualization. `get_visible_nav_nodes(cam_pos)`,
//   `get_visible_ground_nav_nodes(cam_pos)` — filtered by voxel-based
//   occlusion (3D DDA raycast in `world.rs`) so the placement UI only snaps
//   to nodes the camera can actually see.
// - **Commands:** `spawn_creature(species_name, x,y,z)` — generic creature
//   spawner replacing `spawn_elf()` / `spawn_capybara()` (which remain as
//   thin wrappers). Also `create_goto_task(x,y,z)`, `designate_build(x,y,z)`,
//   `designate_build_rect(x,y,z,width,depth)` — each constructs a
//   `SimCommand` and immediately steps the sim by one tick to apply it.
// - **Construction:** `validate_build_position(x,y,z)` checks whether a
//   voxel is valid for building (Air + adjacent to solid) — used for
//   single-voxel preview. `validate_build_air(x,y,z)` checks only
//   in-bounds + Air (no adjacency), and `has_solid_neighbor(x,y,z)`
//   checks adjacency alone — used together for multi-voxel rectangle
//   validation where adjacency applies to the rectangle as a whole.
//   `get_blueprint_voxels()` returns flat (x,y,z) triples for unplaced
//   voxels in `Designated` blueprints (excludes already-materialized
//   voxels). `get_platform_voxels()` returns flat (x,y,z) triples for
//   voxels materialized by elf construction work. Both consumed by
//   `blueprint_renderer.gd`.
// - **Stats:** `creature_count_by_name(species_name)` — generic replacement
//   for `elf_count()` / `capybara_count()` (which remain as thin wrappers).
//   Also `fruit_count()`, `home_tree_mana()`.
// - **Species queries:** `is_species_ground_only(species_name)` — used by
//   the placement controller to decide which nav nodes to show.
//   `get_all_species_names()` — returns all species names for UI iteration.
//
// All array data uses packed Godot types (`PackedInt32Array`,
// `PackedVector3Array`) for efficient transfer across the GDExtension
// boundary — no per-element marshalling.
//
// See also: `lib.rs` for the GDExtension entry point, the
// `elven_canopy_sim` crate for all simulation logic, `command.rs` for
// `SimCommand`/`SimAction`, `placement_controller.gd` and
// `spawn_toolbar.gd` for spawning/placement callers,
// `selection_controller.gd` and `creature_info_panel.gd` for creature
// query callers, `construction_controller.gd` for build placement,
// `blueprint_renderer.gd` for blueprint visualization.

use elven_canopy_sim::blueprint::BlueprintState;
use elven_canopy_sim::command::{SimAction, SimCommand};
use elven_canopy_sim::config::{GameConfig, TreeProfile};
use elven_canopy_sim::sim::SimState;
use elven_canopy_sim::task::TaskState;
use elven_canopy_sim::types::{BuildType, Priority, Species, VoxelCoord, VoxelType};
use godot::prelude::*;

/// Parse a species name string into a `Species` enum variant.
fn parse_species(name: &str) -> Option<Species> {
    match name {
        "Elf" => Some(Species::Elf),
        "Capybara" => Some(Species::Capybara),
        "Boar" => Some(Species::Boar),
        "Deer" => Some(Species::Deer),
        "Monkey" => Some(Species::Monkey),
        "Squirrel" => Some(Species::Squirrel),
        _ => None,
    }
}

/// Convert a `Species` enum variant to its display string.
fn species_name(species: Species) -> &'static str {
    match species {
        Species::Elf => "Elf",
        Species::Capybara => "Capybara",
        Species::Boar => "Boar",
        Species::Deer => "Deer",
        Species::Monkey => "Monkey",
        Species::Squirrel => "Squirrel",
    }
}

/// Godot node that owns and drives the simulation.
///
/// Add this as a child node in your main scene. Call `init_sim()` from
/// GDScript to create the simulation, then `step_to_tick()` each frame
/// to advance it.
#[derive(GodotClass)]
#[class(base=Node)]
pub struct SimBridge {
    base: Base<Node>,
    sim: Option<SimState>,
}

#[godot_api]
impl INode for SimBridge {
    fn init(base: Base<Node>) -> Self {
        Self { base, sim: None }
    }
}

#[godot_api]
impl SimBridge {
    /// Initialize the simulation with the given seed and default config.
    #[func]
    fn init_sim(&mut self, seed: i64) {
        self.sim = Some(SimState::new(seed as u64));
        godot_print!("SimBridge: simulation initialized with seed {seed}");
    }

    /// Initialize the simulation with the given seed and a custom tree profile.
    ///
    /// The `tree_profile_json` parameter is a JSON string matching the
    /// `TreeProfile` serde schema (see `config.rs`). If parsing fails, falls
    /// back to the default Fantasy Mega profile.
    #[func]
    fn init_sim_with_tree_profile_json(&mut self, seed: i64, tree_profile_json: GString) {
        let profile: TreeProfile = serde_json::from_str(&tree_profile_json.to_string())
            .unwrap_or_else(|e| {
                godot_warn!("Failed to parse tree profile JSON: {e}, using default");
                TreeProfile::fantasy_mega()
            });
        let config = GameConfig {
            tree_profile: profile,
            ..Default::default()
        };
        self.sim = Some(SimState::with_config(seed as u64, config));
        godot_print!("SimBridge: simulation initialized with seed {seed} and custom tree profile");
    }

    /// Advance the simulation to the target tick, processing all events.
    #[func]
    fn step_to_tick(&mut self, target_tick: i64) {
        if let Some(sim) = &mut self.sim {
            sim.step(&[], target_tick as u64);
        }
    }

    /// Return the current simulation tick.
    #[func]
    fn current_tick(&self) -> i64 {
        self.sim.as_ref().map_or(0, |s| s.tick as i64)
    }

    /// Return the mana stored in the player's home tree.
    #[func]
    fn home_tree_mana(&self) -> f32 {
        self.sim.as_ref().map_or(0.0, |s| {
            s.trees
                .get(&s.player_tree_id)
                .map_or(0.0, |t| t.mana_stored)
        })
    }

    /// Return true if the simulation has been initialized.
    #[func]
    fn is_initialized(&self) -> bool {
        self.sim.is_some()
    }

    /// Return the simulation tick duration in milliseconds. The GDScript
    /// frame loop uses this to compute how many ticks to advance per frame
    /// (tick_duration_ms=1 → 1000 ticks/sec).
    #[func]
    fn tick_duration_ms(&self) -> i32 {
        self.sim
            .as_ref()
            .map_or(1, |s| s.config.tick_duration_ms as i32)
    }

    /// Return trunk voxel positions as a flat PackedInt32Array (x,y,z triples).
    #[func]
    fn get_trunk_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let tree = match sim.trees.get(&sim.player_tree_id) {
            Some(t) => t,
            None => return PackedInt32Array::new(),
        };
        let mut arr = PackedInt32Array::new();
        for v in &tree.trunk_voxels {
            arr.push(v.x);
            arr.push(v.y);
            arr.push(v.z);
        }
        arr
    }

    /// Return branch voxel positions as a flat PackedInt32Array (x,y,z triples).
    #[func]
    fn get_branch_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let tree = match sim.trees.get(&sim.player_tree_id) {
            Some(t) => t,
            None => return PackedInt32Array::new(),
        };
        let mut arr = PackedInt32Array::new();
        for v in &tree.branch_voxels {
            arr.push(v.x);
            arr.push(v.y);
            arr.push(v.z);
        }
        arr
    }

    /// Return leaf voxel positions as a flat PackedInt32Array (x,y,z triples).
    #[func]
    fn get_leaf_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let tree = match sim.trees.get(&sim.player_tree_id) {
            Some(t) => t,
            None => return PackedInt32Array::new(),
        };
        let mut arr = PackedInt32Array::new();
        for v in &tree.leaf_voxels {
            arr.push(v.x);
            arr.push(v.y);
            arr.push(v.z);
        }
        arr
    }

    /// Return root voxel positions as a flat PackedInt32Array (x,y,z triples).
    #[func]
    fn get_root_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let tree = match sim.trees.get(&sim.player_tree_id) {
            Some(t) => t,
            None => return PackedInt32Array::new(),
        };
        let mut arr = PackedInt32Array::new();
        for v in &tree.root_voxels {
            arr.push(v.x);
            arr.push(v.y);
            arr.push(v.z);
        }
        arr
    }

    /// Return fruit voxel positions as a flat PackedInt32Array (x,y,z triples).
    #[func]
    fn get_fruit_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let tree = match sim.trees.get(&sim.player_tree_id) {
            Some(t) => t,
            None => return PackedInt32Array::new(),
        };
        let mut arr = PackedInt32Array::new();
        for v in &tree.fruit_positions {
            arr.push(v.x);
            arr.push(v.y);
            arr.push(v.z);
        }
        arr
    }

    /// Return the number of fruit on the player's home tree.
    #[func]
    fn fruit_count(&self) -> i32 {
        self.sim.as_ref().map_or(0, |s| {
            s.trees
                .get(&s.player_tree_id)
                .map_or(0, |t| t.fruit_positions.len() as i32)
        })
    }

    /// Return elf positions. Legacy wrapper — delegates to `get_creature_positions`.
    #[func]
    fn get_elf_positions(&self, render_tick: f64) -> PackedVector3Array {
        self.get_creature_positions(GString::from("Elf"), render_tick)
    }

    /// Return the number of elves. Legacy wrapper — delegates to `creature_count_by_name`.
    #[func]
    fn elf_count(&self) -> i32 {
        self.creature_count_by_name(GString::from("Elf"))
    }

    /// Spawn a creature of the named species at the given voxel position.
    ///
    /// Generic replacement for `spawn_elf()` / `spawn_capybara()`. Species
    /// name must match a `Species` enum variant ("Elf", "Capybara", "Boar",
    /// "Deer", "Monkey", "Squirrel"). Unknown names are silently ignored.
    #[func]
    fn spawn_creature(&mut self, species_name: GString, x: i32, y: i32, z: i32) {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return;
        };
        let Some(sim) = &mut self.sim else { return };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action: SimAction::SpawnCreature {
                species,
                position: VoxelCoord::new(x, y, z),
            },
        };
        sim.step(&[cmd], next_tick);
    }

    /// Spawn an elf at the given voxel position.
    /// Legacy wrapper — delegates to `spawn_creature("Elf", ...)`.
    #[func]
    fn spawn_elf(&mut self, x: i32, y: i32, z: i32) {
        self.spawn_creature(GString::from("Elf"), x, y, z);
    }

    /// Return capybara positions. Legacy wrapper — delegates to `get_creature_positions`.
    #[func]
    fn get_capybara_positions(&self, render_tick: f64) -> PackedVector3Array {
        self.get_creature_positions(GString::from("Capybara"), render_tick)
    }

    /// Return the number of capybaras. Legacy wrapper — delegates to `creature_count_by_name`.
    #[func]
    fn capybara_count(&self) -> i32 {
        self.creature_count_by_name(GString::from("Capybara"))
    }

    /// Return all nav node positions as a PackedVector3Array.
    #[func]
    fn get_all_nav_nodes(&self) -> PackedVector3Array {
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for node in sim.nav_graph.live_nodes() {
            arr.push(Vector3::new(
                node.position.x as f32,
                node.position.y as f32,
                node.position.z as f32,
            ));
        }
        arr
    }

    /// Return ground-level (ForestFloor surface type) nav node positions as a
    /// PackedVector3Array.
    #[func]
    fn get_ground_nav_nodes(&self) -> PackedVector3Array {
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for id in sim.nav_graph.ground_node_ids() {
            let node = sim.nav_graph.node(id);
            arr.push(Vector3::new(
                node.position.x as f32,
                node.position.y as f32,
                node.position.z as f32,
            ));
        }
        arr
    }

    /// Return all nav node positions visible from the given camera position
    /// (not occluded by solid voxels). Used for elf placement.
    #[func]
    fn get_visible_nav_nodes(&self, camera_pos: Vector3) -> PackedVector3Array {
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let cam = [camera_pos.x, camera_pos.y, camera_pos.z];
        let mut arr = PackedVector3Array::new();
        for node in sim.nav_graph.live_nodes() {
            let p = node.position;
            let target = [p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5];
            if !sim.world.raycast_hits_solid(cam, target) {
                arr.push(Vector3::new(p.x as f32, p.y as f32, p.z as f32));
            }
        }
        arr
    }

    /// Return ground-level (ForestFloor surface type) nav node positions
    /// visible from the given camera position (not occluded by solid voxels).
    /// Used for capybara placement.
    #[func]
    fn get_visible_ground_nav_nodes(&self, camera_pos: Vector3) -> PackedVector3Array {
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let cam = [camera_pos.x, camera_pos.y, camera_pos.z];
        let mut arr = PackedVector3Array::new();
        for id in sim.nav_graph.ground_node_ids() {
            let p = sim.nav_graph.node(id).position;
            let target = [p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5];
            if !sim.world.raycast_hits_solid(cam, target) {
                arr.push(Vector3::new(p.x as f32, p.y as f32, p.z as f32));
            }
        }
        arr
    }

    /// Create a GoTo task at the given voxel position (snapped to nearest nav node).
    /// Only an idle elf will claim it and walk to that location.
    #[func]
    fn create_goto_task(&mut self, x: i32, y: i32, z: i32) {
        let Some(sim) = &mut self.sim else { return };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action: SimAction::CreateTask {
                kind: elven_canopy_sim::task::TaskKind::GoTo,
                position: VoxelCoord::new(x, y, z),
                required_species: Some(Species::Elf),
            },
        };
        sim.step(&[cmd], next_tick);
    }

    /// Return info about the creature at the given species-filtered index.
    ///
    /// The index corresponds to the creature's position in the iteration
    /// order of `get_elf_positions()` or `get_capybara_positions()` — i.e.,
    /// BTreeMap order filtered by species. The `render_tick` parameter is
    /// used for position interpolation (same as the position getters).
    ///
    /// Returns a VarDictionary with keys: "species", "x", "y", "z", "has_task".
    /// Returns an empty VarDictionary if species is unknown or index is out of
    /// bounds.
    #[func]
    fn get_creature_info(
        &self,
        species_name: GString,
        index: i32,
        render_tick: f64,
    ) -> VarDictionary {
        let Some(sim) = &self.sim else {
            return VarDictionary::new();
        };
        let Some(species) = parse_species(&species_name.to_string()) else {
            return VarDictionary::new();
        };
        let creature = sim
            .creatures
            .values()
            .filter(|c| c.species == species)
            .nth(index as usize);
        match creature {
            Some(c) => {
                let (x, y, z) = c.interpolated_position(render_tick);
                let mut dict = VarDictionary::new();
                dict.set("species", species_name.clone());
                dict.set("x", x);
                dict.set("y", y);
                dict.set("z", z);
                dict.set("has_task", c.current_task.is_some());
                dict.set("food", c.food);
                let food_max = sim.species_table[&species].food_max;
                dict.set("food_max", food_max);
                dict
            }
            None => VarDictionary::new(),
        }
    }

    /// Return all non-complete tasks as a `VarArray` of dictionaries.
    ///
    /// Each dictionary contains: `id` (short hex), `id_full` (full UUID),
    /// `kind` ("GoTo" or "Build"), `state` ("Available" or "In Progress"),
    /// `progress`, `total_cost`, `location_x/y/z`, and `assignees` (array
    /// of dictionaries with `id_short`, `species`, `index`).
    ///
    /// The creature `index` matches the species-filtered iteration order used
    /// by `get_creature_positions()`, so GDScript can use it directly for
    /// camera follow and selection.
    #[func]
    fn get_active_tasks(&self) -> VarArray {
        let Some(sim) = &self.sim else {
            return VarArray::new();
        };

        let mut result = VarArray::new();
        for task in sim.tasks.values() {
            if task.state == TaskState::Complete {
                continue;
            }

            let mut dict = VarDictionary::new();

            // Task ID — short (first 8 hex chars) and full UUID.
            let id_full = task.id.0.to_string();
            let id_short: String = id_full.chars().take(8).collect();
            dict.set("id", GString::from(&id_short));
            dict.set("id_full", GString::from(&id_full));

            // Kind.
            let kind_str = match &task.kind {
                elven_canopy_sim::task::TaskKind::GoTo => "GoTo",
                elven_canopy_sim::task::TaskKind::Build { .. } => "Build",
            };
            dict.set("kind", GString::from(kind_str));

            // State.
            let state_str = match task.state {
                TaskState::Available => "Available",
                TaskState::InProgress => "In Progress",
                TaskState::Complete => unreachable!(),
            };
            dict.set("state", GString::from(state_str));

            // Progress.
            dict.set("progress", task.progress);
            dict.set("total_cost", task.total_cost);

            // Location — resolve NavNodeId to VoxelCoord.
            let pos = sim.nav_graph.node(task.location).position;
            dict.set("location_x", pos.x);
            dict.set("location_y", pos.y);
            dict.set("location_z", pos.z);

            // Assignees — resolve CreatureId to species name + index.
            let mut assignees_arr = VarArray::new();
            for assignee_id in &task.assignees {
                if let Some(creature) = sim.creatures.get(assignee_id) {
                    let mut a = VarDictionary::new();
                    let cid_full = assignee_id.0.to_string();
                    let cid_short: String = cid_full.chars().take(8).collect();
                    a.set("id_short", GString::from(&cid_short));

                    let sp = species_name(creature.species);
                    a.set("species", GString::from(sp));

                    // Compute the species-filtered index: count how many
                    // creatures of the same species come before this one in
                    // BTreeMap iteration order.
                    let index = sim
                        .creatures
                        .iter()
                        .filter(|(_, c)| c.species == creature.species)
                        .position(|(id, _)| *id == *assignee_id)
                        .unwrap_or(0);
                    a.set("index", index as i32);

                    assignees_arr.push(&a.to_variant());
                }
            }
            dict.set("assignees", assignees_arr);

            result.push(&dict.to_variant());
        }
        result
    }

    /// Return positions for any species as a PackedVector3Array, interpolated
    /// to the given render tick for smooth movement between nav nodes.
    /// Generic replacement for `get_elf_positions()` / `get_capybara_positions()`.
    #[func]
    fn get_creature_positions(
        &self,
        species_name: GString,
        render_tick: f64,
    ) -> PackedVector3Array {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return PackedVector3Array::new();
        };
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for creature in sim.creatures.values().filter(|c| c.species == species) {
            let (x, y, z) = creature.interpolated_position(render_tick);
            arr.push(Vector3::new(x, y, z));
        }
        arr
    }

    /// Return the number of creatures of the named species.
    /// Generic replacement for `elf_count()` / `capybara_count()`.
    #[func]
    fn creature_count_by_name(&self, species_name: GString) -> i32 {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return 0;
        };
        self.sim
            .as_ref()
            .map_or(0, |s| s.creature_count(species) as i32)
    }

    /// Return whether the named species is ground-only (cannot climb).
    /// Used by the placement controller to decide which nav nodes to show.
    #[func]
    fn is_species_ground_only(&self, species_name: GString) -> bool {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return false;
        };
        let Some(sim) = &self.sim else { return false };
        sim.species_table
            .get(&species)
            .is_some_and(|s| s.ground_only)
    }

    /// Return the names of all species known to the simulation.
    /// Used by UI code to iterate over species without hardcoding names.
    #[func]
    fn get_all_species_names(&self) -> PackedStringArray {
        let mut arr = PackedStringArray::new();
        arr.push("Elf");
        arr.push("Capybara");
        arr.push("Boar");
        arr.push("Deer");
        arr.push("Monkey");
        arr.push("Squirrel");
        arr
    }

    /// Serialize the current simulation state to a JSON string.
    ///
    /// Returns the JSON string, or an empty string on error. The caller
    /// (GDScript) is responsible for writing the string to disk via Godot's
    /// file I/O — the sim crate has no filesystem access.
    #[func]
    fn save_game_json(&self) -> GString {
        let Some(sim) = &self.sim else {
            return GString::new();
        };
        match sim.to_json() {
            Ok(json) => GString::from(&json),
            Err(e) => {
                godot_error!("SimBridge: failed to serialize sim state: {e}");
                GString::new()
            }
        }
    }

    /// Replace the current simulation state with one deserialized from JSON.
    ///
    /// Returns `true` on success. On failure, the previous sim state is
    /// preserved (or cleared if there was none).
    #[func]
    fn load_game_json(&mut self, json: GString) -> bool {
        match SimState::from_json(&json.to_string()) {
            Ok(state) => {
                godot_print!(
                    "SimBridge: loaded save (tick={}, creatures={})",
                    state.tick,
                    state.creatures.len()
                );
                self.sim = Some(state);
                true
            }
            Err(e) => {
                godot_error!("SimBridge: failed to load save: {e}");
                false
            }
        }
    }

    /// Spawn a capybara at the given voxel position.
    /// Legacy wrapper — delegates to `spawn_creature("Capybara", ...)`.
    #[func]
    fn spawn_capybara(&mut self, x: i32, y: i32, z: i32) {
        self.spawn_creature(GString::from("Capybara"), x, y, z);
    }

    /// Check whether a single voxel is a valid build position.
    ///
    /// A position is valid if it is in-bounds, Air, and has at least one
    /// face-adjacent solid voxel. Used by the construction ghost mesh to
    /// show blue (valid) vs red (invalid) preview color for single-voxel
    /// placement.
    #[func]
    fn validate_build_position(&self, x: i32, y: i32, z: i32) -> bool {
        let Some(sim) = &self.sim else { return false };
        let coord = VoxelCoord::new(x, y, z);
        sim.world.in_bounds(coord)
            && sim.world.get(coord) == VoxelType::Air
            && sim.world.has_solid_face_neighbor(coord)
    }

    /// Check whether a single voxel is in-bounds and Air (buildable).
    ///
    /// Unlike `validate_build_position`, this does NOT require adjacency
    /// to a solid voxel. Used by multi-voxel rectangle validation where
    /// the adjacency requirement applies to the rectangle as a whole (at
    /// least one voxel must touch solid), not to every individual voxel.
    #[func]
    fn validate_build_air(&self, x: i32, y: i32, z: i32) -> bool {
        let Some(sim) = &self.sim else { return false };
        let coord = VoxelCoord::new(x, y, z);
        sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air
    }

    /// Check whether a single voxel has at least one face-adjacent solid
    /// voxel. Used alongside `validate_build_air` for multi-voxel rectangle
    /// validation.
    #[func]
    fn has_solid_neighbor(&self, x: i32, y: i32, z: i32) -> bool {
        let Some(sim) = &self.sim else { return false };
        sim.world.has_solid_face_neighbor(VoxelCoord::new(x, y, z))
    }

    /// Designate a single-voxel platform blueprint at the given position.
    ///
    /// Constructs a `DesignateBuild` command and steps the sim by one tick,
    /// following the same pattern as `spawn_elf()` / `create_goto_task()`.
    /// The sim validates the position internally and silently ignores invalid
    /// designations.
    #[func]
    fn designate_build(&mut self, x: i32, y: i32, z: i32) {
        let Some(sim) = &mut self.sim else { return };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![VoxelCoord::new(x, y, z)],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], next_tick);
    }

    /// Designate a rectangular platform blueprint.
    ///
    /// `x, y, z` is the min-corner of the rectangle (GDScript computes this
    /// from the center focus voxel and the current dimensions). `width` and
    /// `depth` are the size in X and Z (clamped to >= 1). All voxels share
    /// the same Y. Same command pattern as `designate_build()`.
    #[func]
    fn designate_build_rect(&mut self, x: i32, y: i32, z: i32, width: i32, depth: i32) {
        let Some(sim) = &mut self.sim else { return };
        let w = width.max(1);
        let d = depth.max(1);
        let mut voxels = Vec::with_capacity((w * d) as usize);
        for dx in 0..w {
            for dz in 0..d {
                voxels.push(VoxelCoord::new(x + dx, y, z + dz));
            }
        }
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], next_tick);
    }

    /// Designate a building at the given anchor position.
    ///
    /// `x, y, z` is the anchor (min corner at foundation level). `width` and
    /// `depth` are the building footprint, `height` is the number of floors.
    /// Same command pattern as `designate_build()`.
    #[func]
    fn designate_building(&mut self, x: i32, y: i32, z: i32, width: i32, depth: i32, height: i32) {
        let Some(sim) = &mut self.sim else { return };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action: SimAction::DesignateBuilding {
                anchor: VoxelCoord::new(x, y, z),
                width,
                depth,
                height,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], next_tick);
    }

    /// Validate whether a building can be placed at the given anchor.
    ///
    /// Checks that all foundation voxels (at anchor.y) are solid and all
    /// interior voxels (above foundation) are Air and in-bounds.
    #[func]
    fn validate_building_position(
        &self,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        depth: i32,
        height: i32,
    ) -> bool {
        let Some(sim) = &self.sim else { return false };
        if width < 3 || depth < 3 || height < 1 {
            return false;
        }
        // Check foundation.
        for dx in 0..width {
            for dz in 0..depth {
                let coord = VoxelCoord::new(x + dx, y, z + dz);
                if !sim.world.in_bounds(coord) || !sim.world.get(coord).is_solid() {
                    return false;
                }
            }
        }
        // Check interior.
        for dy in 1..=height {
            for dx in 0..width {
                for dz in 0..depth {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    if !sim.world.in_bounds(coord) || sim.world.get(coord) != VoxelType::Air {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Return building face data as a flat PackedInt32Array of quintuples:
    /// (x, y, z, face_direction, face_type) for every non-Open face.
    ///
    /// face_direction: 0=PosX, 1=NegX, 2=PosY, 3=NegY, 4=PosZ, 5=NegZ
    /// face_type: 0=Open, 1=Wall, 2=Window, 3=Door, 4=Ceiling, 5=Floor
    #[func]
    fn get_building_face_data(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for (coord, fd) in &sim.face_data {
            for (dir_idx, &face) in fd.faces.iter().enumerate() {
                if face == elven_canopy_sim::types::FaceType::Open {
                    continue;
                }
                arr.push(coord.x);
                arr.push(coord.y);
                arr.push(coord.z);
                arr.push(dir_idx as i32);
                let face_int = match face {
                    elven_canopy_sim::types::FaceType::Open => 0,
                    elven_canopy_sim::types::FaceType::Wall => 1,
                    elven_canopy_sim::types::FaceType::Window => 2,
                    elven_canopy_sim::types::FaceType::Door => 3,
                    elven_canopy_sim::types::FaceType::Ceiling => 4,
                    elven_canopy_sim::types::FaceType::Floor => 5,
                };
                arr.push(face_int);
            }
        }
        arr
    }

    /// Return unplaced voxels from `Designated` blueprints as a flat
    /// PackedInt32Array of (x,y,z) triples.
    ///
    /// Only includes voxels that are still Air in the world — voxels that
    /// have already been materialized by construction work are excluded.
    /// Used by the blueprint renderer to show translucent ghost cubes for
    /// planned (not-yet-built) construction. Same format as
    /// `get_trunk_voxels()` etc.
    #[func]
    fn get_blueprint_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for bp in sim.blueprints.values() {
            if bp.state == BlueprintState::Designated {
                for v in &bp.voxels {
                    if sim.world.get(*v) == VoxelType::Air {
                        arr.push(v.x);
                        arr.push(v.y);
                        arr.push(v.z);
                    }
                }
            }
        }
        arr
    }

    /// Return materialized construction voxels (platforms, walls, etc.) as a
    /// flat PackedInt32Array of (x,y,z) triples.
    ///
    /// These are voxels that have been placed by elf construction work — they
    /// exist as solid geometry in the world but are not part of the original
    /// tree. Used by the blueprint renderer to show built voxels as wood.
    ///
    /// Excludes `BuildingInterior` voxels — those are rendered by the building
    /// renderer as oriented face quads, not solid cubes.
    #[func]
    fn get_platform_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for &(coord, voxel_type) in &sim.placed_voxels {
            if voxel_type == VoxelType::BuildingInterior {
                continue;
            }
            arr.push(coord.x);
            arr.push(coord.y);
            arr.push(coord.z);
        }
        arr
    }
}
