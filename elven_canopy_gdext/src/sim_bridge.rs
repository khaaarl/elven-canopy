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
// - **Creature positions:** `get_elf_positions(render_tick)`,
//   `get_capybara_positions(render_tick)` — `PackedVector3Array` for billboard
//   sprite placement. The `render_tick` parameter (a fractional tick computed
//   by `main.gd` as `current_tick + accumulator_fraction`) enables smooth
//   interpolation between nav nodes via `Creature::interpolated_position()`.
//   Internally, all creatures are unified `Creature` entities with a `species`
//   field; the bridge filters by species so the GDScript API has clean
//   per-species calls.
// - **Creature info:** `get_creature_info(species_name, index, render_tick)` —
//   returns a `VarDictionary` with species, interpolated position (x/y/z),
//   task status, food level, and food_max for the creature at the given
//   species-filtered index. Used by the creature info panel for display and
//   follow-mode tracking.
// - **Nav nodes:** `get_all_nav_nodes()`, `get_ground_nav_nodes()` — for
//   debug visualization. `get_visible_nav_nodes(cam_pos)`,
//   `get_visible_ground_nav_nodes(cam_pos)` — filtered by voxel-based
//   occlusion (3D DDA raycast in `world.rs`) so the placement UI only snaps
//   to nodes the camera can actually see.
// - **Commands:** `spawn_elf(x,y,z)`, `spawn_capybara(x,y,z)`,
//   `create_goto_task(x,y,z)`, `designate_build(x,y,z)`,
//   `designate_build_rect(x,y,z,width,depth)` — each constructs a
//   `SimCommand` and immediately steps the sim by one tick to apply it.
// - **Construction:** `validate_build_position(x,y,z)` checks whether a
//   voxel is valid for building (Air + adjacent to solid) — used for
//   single-voxel preview. `validate_build_air(x,y,z)` checks only
//   in-bounds + Air (no adjacency), and `has_solid_neighbor(x,y,z)`
//   checks adjacency alone — used together for multi-voxel rectangle
//   validation where adjacency applies to the rectangle as a whole.
//   `get_blueprint_voxels()` returns flat (x,y,z) triples for all
//   `Designated` blueprints, consumed by `blueprint_renderer.gd`.
// - **Stats:** `elf_count()`, `capybara_count()`, `fruit_count()`,
//   `home_tree_mana()`.
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
use elven_canopy_sim::types::{BuildType, Priority, Species, VoxelCoord, VoxelType};
use godot::prelude::*;

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

    /// Return elf positions as a PackedVector3Array, interpolated to the
    /// given render tick for smooth movement between nav nodes. Pass the
    /// sim's `current_tick()` for non-interpolated positions.
    #[func]
    fn get_elf_positions(&self, render_tick: f64) -> PackedVector3Array {
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for creature in sim.creatures.values().filter(|c| c.species == Species::Elf) {
            let (x, y, z) = creature.interpolated_position(render_tick);
            arr.push(Vector3::new(x, y, z));
        }
        arr
    }

    /// Return the number of elves.
    #[func]
    fn elf_count(&self) -> i32 {
        self.sim
            .as_ref()
            .map_or(0, |s| s.creature_count(Species::Elf) as i32)
    }

    /// Spawn an elf at the given voxel position.
    #[func]
    fn spawn_elf(&mut self, x: i32, y: i32, z: i32) {
        let Some(sim) = &mut self.sim else { return };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action: SimAction::SpawnElf {
                position: VoxelCoord::new(x, y, z),
            },
        };
        sim.step(&[cmd], next_tick);
    }

    /// Return capybara positions as a PackedVector3Array, interpolated to
    /// the given render tick for smooth movement between nav nodes. Pass the
    /// sim's `current_tick()` for non-interpolated positions.
    #[func]
    fn get_capybara_positions(&self, render_tick: f64) -> PackedVector3Array {
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for creature in sim
            .creatures
            .values()
            .filter(|c| c.species == Species::Capybara)
        {
            let (x, y, z) = creature.interpolated_position(render_tick);
            arr.push(Vector3::new(x, y, z));
        }
        arr
    }

    /// Return the number of capybaras.
    #[func]
    fn capybara_count(&self) -> i32 {
        self.sim
            .as_ref()
            .map_or(0, |s| s.creature_count(Species::Capybara) as i32)
    }

    /// Return all nav node positions as a PackedVector3Array.
    #[func]
    fn get_all_nav_nodes(&self) -> PackedVector3Array {
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for node in &sim.nav_graph.nodes {
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
        for node in &sim.nav_graph.nodes {
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
        let species = match species_name.to_string().as_str() {
            "Elf" => Species::Elf,
            "Capybara" => Species::Capybara,
            _ => return VarDictionary::new(),
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
    #[func]
    fn spawn_capybara(&mut self, x: i32, y: i32, z: i32) {
        let Some(sim) = &mut self.sim else { return };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action: SimAction::SpawnCapybara {
                position: VoxelCoord::new(x, y, z),
            },
        };
        sim.step(&[cmd], next_tick);
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

    /// Return all voxels from `Designated` blueprints as a flat
    /// PackedInt32Array of (x,y,z) triples.
    ///
    /// Used by the blueprint renderer to show translucent ghost cubes for
    /// planned construction. Same format as `get_trunk_voxels()` etc.
    #[func]
    fn get_blueprint_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for bp in sim.blueprints.values() {
            if bp.state == BlueprintState::Designated {
                for v in &bp.voxels {
                    arr.push(v.x);
                    arr.push(v.y);
                    arr.push(v.z);
                }
            }
        }
        arr
    }
}
