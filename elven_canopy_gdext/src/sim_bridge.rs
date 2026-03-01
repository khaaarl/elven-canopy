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
//   `validate_platform_preview(x,y,z,w,d)` and
//   `validate_building_preview(x,y,z,w,d,h)` combine basic checks with
//   structural analysis and return `{tier, message}` dictionaries for
//   real-time 3-state ghost preview (Ok/Warning/Blocked).
//   `get_blueprint_voxels()` returns flat (x,y,z) triples for unplaced
//   voxels in `Designated` blueprints (excludes already-materialized
//   voxels). `get_platform_voxels()` returns flat (x,y,z) triples for
//   voxels materialized by elf construction work. Both consumed by
//   `blueprint_renderer.gd`.
// - **Stats:** `creature_count_by_name(species_name)` — generic replacement
//   for `elf_count()` / `capybara_count()` (which remain as thin wrappers).
//   Also `fruit_count()`, `home_tree_mana()`.
// - **Tree info:** `get_home_tree_info()` — returns a `VarDictionary` with
//   the player's home tree stats: health, growth, mana, fruit, carrying
//   capacity, voxel counts by type, height, spread, and anchor position.
//   Used by `tree_info_panel.gd`.
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
// `action_toolbar.gd` for spawning/placement callers,
// `selection_controller.gd` and `creature_info_panel.gd` for creature
// query callers, `construction_controller.gd` for build placement,
// `blueprint_renderer.gd` for blueprint visualization.

use std::collections::BTreeMap;

use elven_canopy_protocol::message::ServerMessage;
use elven_canopy_relay::server::{RelayConfig, RelayHandle, start_relay};
use elven_canopy_sim::blueprint::BlueprintState;
use elven_canopy_sim::command::{SimAction, SimCommand};
use elven_canopy_sim::config::{GameConfig, TreeProfile};
use elven_canopy_sim::sim::SimState;
use elven_canopy_sim::structural::{self, ValidationTier};
use elven_canopy_sim::task::TaskState;
use elven_canopy_sim::types::{
    BuildType, FaceDirection, LadderKind, OverlapClassification, Priority, Species, VoxelCoord,
    VoxelType,
};
use godot::prelude::*;

use elven_canopy_relay::client::NetClient;

/// Compile-time version hash. Bump when making breaking protocol changes.
const SIM_VERSION_HASH: u64 = 1;

/// Parse a species name string into a `Species` enum variant.
fn parse_species(name: &str) -> Option<Species> {
    match name {
        "Elf" => Some(Species::Elf),
        "Capybara" => Some(Species::Capybara),
        "Boar" => Some(Species::Boar),
        "Deer" => Some(Species::Deer),
        "Elephant" => Some(Species::Elephant),
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
        Species::Elephant => "Elephant",
        Species::Monkey => "Monkey",
        Species::Squirrel => "Squirrel",
    }
}

/// Godot node that owns and drives the simulation.
///
/// Add this as a child node in your main scene. Call `init_sim()` from
/// GDScript to create the simulation, then `step_to_tick()` each frame
/// to advance it. In multiplayer mode, call `host_game()` or `join_game()`
/// instead, then `poll_network()` each frame to receive turns.
#[derive(GodotClass)]
#[class(base=Node)]
pub struct SimBridge {
    base: Base<Node>,
    sim: Option<SimState>,
    // Multiplayer state
    net_client: Option<NetClient>,
    relay_handle: Option<RelayHandle>,
    is_multiplayer_mode: bool,
    is_host: bool,
    game_started: bool,
    mp_events: Vec<String>,
    mp_ticks_per_turn: u32,
}

#[godot_api]
impl INode for SimBridge {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            sim: None,
            net_client: None,
            relay_handle: None,
            is_multiplayer_mode: false,
            is_host: false,
            game_started: false,
            mp_events: Vec::new(),
            mp_ticks_per_turn: 50,
        }
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

    /// Return dirt voxel positions as a flat PackedInt32Array (x,y,z triples).
    #[func]
    fn get_dirt_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let tree = match sim.trees.get(&sim.player_tree_id) {
            Some(t) => t,
            None => return PackedInt32Array::new(),
        };
        let mut arr = PackedInt32Array::new();
        for v in &tree.dirt_voxels {
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

    /// Return stats about the player's home tree as a dictionary.
    ///
    /// Keys: health, growth_level, mana_stored, mana_capacity,
    /// fruit_count, fruit_production_rate, carrying_capacity, current_load,
    /// trunk_voxels, branch_voxels, leaf_voxels, root_voxels, total_voxels,
    /// height, spread_x, spread_z, position_x, position_y, position_z.
    #[func]
    fn get_home_tree_info(&self) -> VarDictionary {
        let Some(sim) = &self.sim else {
            return VarDictionary::new();
        };
        let Some(tree) = sim.trees.get(&sim.player_tree_id) else {
            return VarDictionary::new();
        };

        let mut dict = VarDictionary::new();
        dict.set("health", tree.health);
        dict.set("growth_level", tree.growth_level as i32);
        dict.set("mana_stored", tree.mana_stored);
        dict.set("mana_capacity", tree.mana_capacity);
        dict.set("fruit_count", tree.fruit_positions.len() as i32);
        dict.set("fruit_production_rate", tree.fruit_production_rate);
        dict.set("carrying_capacity", tree.carrying_capacity);
        dict.set("current_load", tree.current_load);

        let trunk = tree.trunk_voxels.len() as i32;
        let branch = tree.branch_voxels.len() as i32;
        let leaf = tree.leaf_voxels.len() as i32;
        let root = tree.root_voxels.len() as i32;
        dict.set("trunk_voxels", trunk);
        dict.set("branch_voxels", branch);
        dict.set("leaf_voxels", leaf);
        dict.set("root_voxels", root);
        dict.set("total_voxels", trunk + branch + leaf + root);

        // Compute height and spread from all wood voxels.
        let all_voxels = tree
            .trunk_voxels
            .iter()
            .chain(&tree.branch_voxels)
            .chain(&tree.root_voxels)
            .chain(&tree.leaf_voxels);

        let mut min_x = i32::MAX;
        let mut max_x = i32::MIN;
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        let mut min_z = i32::MAX;
        let mut max_z = i32::MIN;
        let mut count = 0;

        for v in all_voxels {
            min_x = min_x.min(v.x);
            max_x = max_x.max(v.x);
            min_y = min_y.min(v.y);
            max_y = max_y.max(v.y);
            min_z = min_z.min(v.z);
            max_z = max_z.max(v.z);
            count += 1;
        }

        if count > 0 {
            dict.set("height", max_y - min_y + 1);
            dict.set("spread_x", max_x - min_x + 1);
            dict.set("spread_z", max_z - min_z + 1);
        } else {
            dict.set("height", 0);
            dict.set("spread_x", 0);
            dict.set("spread_z", 0);
        }

        dict.set("position_x", tree.position.x);
        dict.set("position_y", tree.position.y);
        dict.set("position_z", tree.position.z);

        dict
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

    /// Apply a SimAction locally (single-player) or send it to the relay
    /// (multiplayer). In multiplayer, the action will come back in a Turn
    /// message and be applied then.
    fn apply_or_send(&mut self, action: SimAction) {
        if self.is_multiplayer_mode {
            if let Some(client) = &mut self.net_client
                && let Ok(json) = serde_json::to_vec(&action)
                && let Err(e) = client.send_command(&json)
            {
                godot_error!("SimBridge: send_command failed: {e}");
            }
        } else if let Some(sim) = &mut self.sim {
            let player_id = sim.player_id;
            let next_tick = sim.tick + 1;
            let cmd = SimCommand {
                player_id,
                tick: next_tick,
                action,
            };
            sim.step(&[cmd], next_tick);
        }
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
        self.apply_or_send(SimAction::SpawnCreature {
            species,
            position: VoxelCoord::new(x, y, z),
        });
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
        self.apply_or_send(SimAction::CreateTask {
            kind: elven_canopy_sim::task::TaskKind::GoTo,
            position: VoxelCoord::new(x, y, z),
            required_species: Some(Species::Elf),
        });
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
                elven_canopy_sim::task::TaskKind::EatFruit { .. } => "EatFruit",
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

    /// Return all completed structures as a `VarArray` of dictionaries.
    ///
    /// Each dictionary contains: `id` (int), `build_type` (String),
    /// `anchor_x/y/z` (int), `width/depth/height` (int).
    /// Used by `structure_list_panel.gd` for the browsable structure list.
    #[func]
    fn get_structures(&self) -> VarArray {
        let Some(sim) = &self.sim else {
            return VarArray::new();
        };
        let mut result = VarArray::new();
        for structure in sim.structures.values() {
            let mut dict = VarDictionary::new();
            dict.set("id", structure.id.0 as i64);
            let build_type_str = match structure.build_type {
                BuildType::Platform => "Platform",
                BuildType::Bridge => "Bridge",
                BuildType::Stairs => "Stairs",
                BuildType::Wall => "Wall",
                BuildType::Enclosure => "Enclosure",
                BuildType::Building => "Building",
                BuildType::WoodLadder => "WoodLadder",
                BuildType::RopeLadder => "RopeLadder",
            };
            dict.set("build_type", GString::from(build_type_str));
            dict.set("anchor_x", structure.anchor.x);
            dict.set("anchor_y", structure.anchor.y);
            dict.set("anchor_z", structure.anchor.z);
            dict.set("width", structure.width);
            dict.set("depth", structure.depth);
            dict.set("height", structure.height);
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
        arr.push("Elephant");
        arr.push("Monkey");
        arr.push("Squirrel");
        arr
    }

    /// Return the footprint `[width_x, height_y, depth_z]` for the named species.
    /// Returns `Vector3i(1,1,1)` if the species is unknown.
    #[func]
    fn get_species_footprint(&self, species_name: GString) -> Vector3i {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return Vector3i::new(1, 1, 1);
        };
        let Some(sim) = &self.sim else {
            return Vector3i::new(1, 1, 1);
        };
        match sim.species_table.get(&species) {
            Some(data) => Vector3i::new(
                data.footprint[0] as i32,
                data.footprint[1] as i32,
                data.footprint[2] as i32,
            ),
            None => Vector3i::new(1, 1, 1),
        }
    }

    /// Return all ground nav nodes from the large (2x2x2) nav graph.
    /// Used by the placement controller for large creature spawn snapping.
    #[func]
    fn get_large_ground_nav_nodes(&self) -> PackedVector3Array {
        let mut arr = PackedVector3Array::new();
        let Some(sim) = &self.sim else {
            return arr;
        };
        for node in sim.large_nav_graph.live_nodes() {
            let p = node.position;
            arr.push(Vector3::new(p.x as f32, p.y as f32, p.z as f32));
        }
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

    /// Set the food level of the nth creature of the given species.
    ///
    /// `index` is the species-filtered iteration index matching the order
    /// used by `get_creature_positions()`. Used by `main.gd` to vary
    /// initial food levels after spawning.
    #[func]
    fn set_creature_food(&mut self, species_name: GString, index: i32, food: i64) {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return;
        };
        let Some(sim) = &mut self.sim else { return };
        let creature_id = sim
            .creatures
            .iter()
            .filter(|(_, c)| c.species == species)
            .nth(index as usize)
            .map(|(_, c)| c.id);
        if let Some(id) = creature_id
            && let Some(creature) = sim.creatures.get_mut(&id)
        {
            creature.food = food;
        }
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
    /// Returns a non-empty string if the sim produced a validation message
    /// (warning or block reason), empty string on silent success.
    #[func]
    fn designate_build(&mut self, x: i32, y: i32, z: i32) -> GString {
        let action = SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![VoxelCoord::new(x, y, z)],
            priority: Priority::Normal,
        };
        if self.is_multiplayer_mode {
            self.apply_or_send(action);
            return GString::new();
        }
        let Some(sim) = &mut self.sim else {
            return GString::new();
        };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action,
        };
        sim.step(&[cmd], next_tick);
        sim.last_build_message
            .as_deref()
            .map_or_else(GString::new, GString::from)
    }

    /// Designate a rectangular platform blueprint.
    ///
    /// `x, y, z` is the min-corner of the rectangle (GDScript computes this
    /// from the center focus voxel and the current dimensions). `width` and
    /// `depth` are the size in X and Z (clamped to >= 1). All voxels share
    /// the same Y. Returns a validation message (empty = success).
    #[func]
    fn designate_build_rect(&mut self, x: i32, y: i32, z: i32, width: i32, depth: i32) -> GString {
        let w = width.max(1);
        let d = depth.max(1);
        let mut voxels = Vec::with_capacity((w * d) as usize);
        for dx in 0..w {
            for dz in 0..d {
                voxels.push(VoxelCoord::new(x + dx, y, z + dz));
            }
        }
        let action = SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels,
            priority: Priority::Normal,
        };
        if self.is_multiplayer_mode {
            self.apply_or_send(action);
            return GString::new();
        }
        let Some(sim) = &mut self.sim else {
            return GString::new();
        };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action,
        };
        sim.step(&[cmd], next_tick);
        sim.last_build_message
            .as_deref()
            .map_or_else(GString::new, GString::from)
    }

    /// Designate a building at the given anchor position.
    ///
    /// `x, y, z` is the anchor (min corner at foundation level). `width` and
    /// `depth` are the building footprint, `height` is the number of floors.
    /// Returns a validation message (empty = success).
    #[func]
    fn designate_building(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        depth: i32,
        height: i32,
    ) -> GString {
        let action = SimAction::DesignateBuilding {
            anchor: VoxelCoord::new(x, y, z),
            width,
            depth,
            height,
            priority: Priority::Normal,
        };
        if self.is_multiplayer_mode {
            self.apply_or_send(action);
            return GString::new();
        }
        let Some(sim) = &mut self.sim else {
            return GString::new();
        };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action,
        };
        sim.step(&[cmd], next_tick);
        sim.last_build_message
            .as_deref()
            .map_or_else(GString::new, GString::from)
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

    /// Preview-validate a rectangular platform placement.
    ///
    /// Combines basic checks (in-bounds, Air, adjacency) with structural
    /// analysis via `validate_blueprint_fast()`. Returns a `VarDictionary`
    /// with keys:
    /// - `"tier"`: `"Ok"`, `"Warning"`, or `"Blocked"`
    /// - `"message"`: human-readable explanation (empty for Ok)
    ///
    /// Read-only — does not step the sim or modify any state.
    #[func]
    fn validate_platform_preview(
        &self,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        depth: i32,
    ) -> VarDictionary {
        let Some(sim) = &self.sim else {
            return Self::preview_result("Blocked", "Simulation not initialized.");
        };
        let w = width.max(1);
        let d = depth.max(1);
        let mut voxels = Vec::with_capacity((w * d) as usize);
        for dx in 0..w {
            for dz in 0..d {
                voxels.push(VoxelCoord::new(x + dx, y, z + dz));
            }
        }

        // Basic bounds check.
        for &coord in &voxels {
            if !sim.world.in_bounds(coord) {
                return Self::preview_result("Blocked", "Build position is out of bounds.");
            }
        }

        // Overlap-aware classification: Platform allows tree overlap.
        let mut build_voxels = Vec::new();
        for &coord in &voxels {
            match sim.world.get(coord).classify_for_overlap() {
                OverlapClassification::Exterior | OverlapClassification::Convertible => {
                    build_voxels.push(coord);
                }
                OverlapClassification::AlreadyWood => {
                    // Skip — already wood.
                }
                OverlapClassification::Blocked => {
                    return Self::preview_result("Blocked", "Build position is not empty.");
                }
            }
        }
        if build_voxels.is_empty() {
            return Self::preview_result(
                "Blocked",
                "Nothing to build — all voxels are already wood.",
            );
        }

        // At least one buildable voxel must be face-adjacent to solid.
        let any_adjacent = build_voxels
            .iter()
            .any(|&coord| sim.world.has_solid_face_neighbor(coord));
        if !any_adjacent {
            return Self::preview_result(
                "Blocked",
                "Must build adjacent to an existing structure.",
            );
        }

        // Structural validation on buildable voxels only.
        let validation = structural::validate_blueprint_fast(
            &sim.world,
            &sim.face_data,
            &build_voxels,
            BuildType::Platform.to_voxel_type(),
            &BTreeMap::new(),
            &sim.config,
        );
        Self::preview_result_from_tier(validation.tier, &validation.message)
    }

    /// Preview-validate a building placement.
    ///
    /// Combines basic checks (size, solid foundation, air interior) with
    /// structural analysis via `validate_blueprint_fast()`. Returns a
    /// `VarDictionary` with `"tier"` and `"message"` keys, same as
    /// `validate_platform_preview()`.
    ///
    /// Read-only — does not step the sim or modify any state.
    #[func]
    fn validate_building_preview(
        &self,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        depth: i32,
        height: i32,
    ) -> VarDictionary {
        let Some(sim) = &self.sim else {
            return Self::preview_result("Blocked", "Simulation not initialized.");
        };

        if width < 3 || depth < 3 || height < 1 {
            return Self::preview_result("Blocked", "Building too small (min 3x3x1).");
        }

        // Validate foundation (all must be solid).
        let anchor = VoxelCoord::new(x, y, z);
        for dx in 0..width {
            for dz in 0..depth {
                let coord = VoxelCoord::new(x + dx, y, z + dz);
                if !sim.world.in_bounds(coord) || !sim.world.get(coord).is_solid() {
                    return Self::preview_result("Blocked", "Foundation must be on solid ground.");
                }
            }
        }

        // Validate interior (all must be Air and in-bounds).
        for dy in 1..=height {
            for dx in 0..width {
                for dz in 0..depth {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    if !sim.world.in_bounds(coord) || sim.world.get(coord) != VoxelType::Air {
                        return Self::preview_result("Blocked", "Building interior must be clear.");
                    }
                }
            }
        }

        // Compute face layout and run structural validation.
        let face_layout =
            elven_canopy_sim::building::compute_building_face_layout(anchor, width, depth, height);
        let voxels: Vec<VoxelCoord> = face_layout.keys().copied().collect();

        let validation = structural::validate_blueprint_fast(
            &sim.world,
            &sim.face_data,
            &voxels,
            VoxelType::BuildingInterior,
            &face_layout,
            &sim.config,
        );
        Self::preview_result_from_tier(validation.tier, &validation.message)
    }

    /// Build a preview result dictionary from a tier string and message.
    fn preview_result(tier: &str, message: &str) -> VarDictionary {
        let mut dict = VarDictionary::new();
        dict.set("tier", GString::from(tier));
        dict.set("message", GString::from(message));
        dict
    }

    /// Build a preview result dictionary from a `ValidationTier`.
    fn preview_result_from_tier(tier: ValidationTier, message: &str) -> VarDictionary {
        let tier_str = match tier {
            ValidationTier::Ok => "Ok",
            ValidationTier::Warning => "Warning",
            ValidationTier::Blocked => "Blocked",
        };
        Self::preview_result(tier_str, message)
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
            // Skip ladder voxels — they're rendered by ladder_renderer.gd.
            if sim.world.get(*coord).is_ladder() {
                continue;
            }
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
                let target = bp.build_type.to_voxel_type();
                for v in &bp.voxels {
                    // A voxel is "unbuilt" if it hasn't been converted to the
                    // target type yet (whether currently Air, Leaf, or Fruit).
                    if sim.world.get(*v) != target {
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
    /// Excludes `BuildingInterior` voxels (rendered by building_renderer.gd as
    /// oriented face quads) and ladder voxels (rendered by ladder_renderer.gd
    /// as thin oriented panels).
    #[func]
    fn get_platform_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for &(coord, voxel_type) in &sim.placed_voxels {
            if voxel_type == VoxelType::BuildingInterior || voxel_type.is_ladder() {
                continue;
            }
            arr.push(coord.x);
            arr.push(coord.y);
            arr.push(coord.z);
        }
        arr
    }

    // ========================================================================
    // Ladder methods
    // ========================================================================

    /// Return completed ladder voxel data as a flat PackedInt32Array of
    /// (x, y, z, face_dir, kind) quintuples.
    ///
    /// - face_dir: 0=PosX, 1=NegX, 2=PosY, 3=NegY, 4=PosZ, 5=NegZ
    /// - kind: 0=Wood, 1=Rope
    #[func]
    fn get_ladder_data(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for &(coord, voxel_type) in &sim.placed_voxels {
            if !voxel_type.is_ladder() {
                continue;
            }
            let face_dir = sim
                .ladder_orientations
                .get(&coord)
                .map_or(0, |d| d.index() as i32);
            let kind = if voxel_type == VoxelType::WoodLadder {
                0
            } else {
                1
            };
            arr.push(coord.x);
            arr.push(coord.y);
            arr.push(coord.z);
            arr.push(face_dir);
            arr.push(kind);
        }
        arr
    }

    /// Return unbuilt ladder blueprint voxels as a flat PackedInt32Array of
    /// (x, y, z, face_dir, kind) quintuples. Same format as `get_ladder_data()`.
    #[func]
    fn get_ladder_blueprint_data(&self) -> PackedInt32Array {
        let Some(sim) = &self.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for bp in sim.blueprints.values() {
            if bp.state != BlueprintState::Designated {
                continue;
            }
            let kind_int = match bp.build_type {
                BuildType::WoodLadder => 0,
                BuildType::RopeLadder => 1,
                _ => continue,
            };
            let target = bp.build_type.to_voxel_type();
            if let Some(layout) = bp.face_layout_map() {
                for &coord in &bp.voxels {
                    if sim.world.get(coord) == target {
                        continue; // already materialized
                    }
                    // Derive face_dir from face layout.
                    let face_dir = if let Some(fd) = layout.get(&coord) {
                        // Find the Wall face whose opposite is Open (the orientation).
                        let mut dir_idx = 0i32;
                        for dir in [
                            FaceDirection::PosX,
                            FaceDirection::NegX,
                            FaceDirection::PosZ,
                            FaceDirection::NegZ,
                        ] {
                            if fd.get(dir) == elven_canopy_sim::types::FaceType::Wall
                                && fd.get(dir.opposite()) == elven_canopy_sim::types::FaceType::Open
                            {
                                dir_idx = dir.index() as i32;
                                break;
                            }
                        }
                        dir_idx
                    } else {
                        0
                    };
                    arr.push(coord.x);
                    arr.push(coord.y);
                    arr.push(coord.z);
                    arr.push(face_dir);
                    arr.push(kind_int);
                }
            }
        }
        arr
    }

    /// Preview-validate a ladder placement. Returns `{tier, message}`.
    ///
    /// - tier: "Ok", "Warning", or "Blocked"
    /// - kind: 0=Wood, 1=Rope
    /// - orientation: 0=PosX, 1=NegX, 4=PosZ, 5=NegZ (FaceDirection index)
    #[func]
    fn validate_ladder_preview(
        &self,
        x: i32,
        y: i32,
        z: i32,
        height: i32,
        orientation: i32,
        kind: i32,
    ) -> VarDictionary {
        let Some(sim) = &self.sim else {
            return Self::preview_result("Blocked", "Simulation not initialized.");
        };
        if height < 1 {
            return Self::preview_result("Blocked", "Height must be at least 1.");
        }
        let face_dir = match orientation {
            0 => FaceDirection::PosX,
            1 => FaceDirection::NegX,
            4 => FaceDirection::PosZ,
            5 => FaceDirection::NegZ,
            _ => return Self::preview_result("Blocked", "Invalid orientation."),
        };
        let (odx, _, odz) = face_dir.to_offset();

        // Build column and validate.
        let mut build_voxels = Vec::new();
        for dy in 0..height {
            let coord = VoxelCoord::new(x, y + dy, z);
            if !sim.world.in_bounds(coord) {
                return Self::preview_result("Blocked", "Ladder extends out of bounds.");
            }
            match sim.world.get(coord).classify_for_overlap() {
                OverlapClassification::Exterior | OverlapClassification::Convertible => {
                    build_voxels.push(coord);
                }
                OverlapClassification::AlreadyWood => {}
                OverlapClassification::Blocked => {
                    return Self::preview_result(
                        "Blocked",
                        "Position blocked by existing construction.",
                    );
                }
            }
        }
        if build_voxels.is_empty() {
            return Self::preview_result(
                "Blocked",
                "Nothing to build — all voxels are already wood.",
            );
        }

        // Anchoring check.
        if kind == 0 {
            // Wood: any voxel's ladder face adjacent to solid.
            let any_anchored = build_voxels.iter().any(|&coord| {
                let neighbor = VoxelCoord::new(coord.x + odx, coord.y, coord.z + odz);
                sim.world.get(neighbor).is_solid()
            });
            if !any_anchored {
                return Self::preview_result(
                    "Blocked",
                    "Wood ladder must be adjacent to a solid surface.",
                );
            }
        } else {
            // Rope: top voxel's ladder face adjacent to solid.
            let top = VoxelCoord::new(x + odx, y + height - 1, z + odz);
            if !sim.world.get(top).is_solid() {
                return Self::preview_result(
                    "Blocked",
                    "Rope ladder must hang from a solid surface at the top.",
                );
            }
        }

        // Structural validation.
        let voxel_type = if kind == 0 {
            VoxelType::WoodLadder
        } else {
            VoxelType::RopeLadder
        };
        let validation = structural::validate_blueprint_fast(
            &sim.world,
            &sim.face_data,
            &build_voxels,
            voxel_type,
            &BTreeMap::new(),
            &sim.config,
        );
        Self::preview_result_from_tier(validation.tier, &validation.message)
    }

    /// Designate a ladder at the given position.
    ///
    /// - kind: 0=Wood, 1=Rope
    /// - orientation: 0=PosX, 1=NegX, 4=PosZ, 5=NegZ (FaceDirection index)
    /// Returns a validation message (empty = success).
    #[func]
    fn designate_ladder(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        height: i32,
        orientation: i32,
        kind: i32,
    ) -> GString {
        let face_dir = match orientation {
            0 => FaceDirection::PosX,
            1 => FaceDirection::NegX,
            4 => FaceDirection::PosZ,
            5 => FaceDirection::NegZ,
            _ => return GString::from("Invalid orientation."),
        };
        let ladder_kind = if kind == 0 {
            LadderKind::Wood
        } else {
            LadderKind::Rope
        };
        let action = SimAction::DesignateLadder {
            anchor: VoxelCoord::new(x, y, z),
            height,
            orientation: face_dir,
            kind: ladder_kind,
            priority: Priority::Normal,
        };
        if self.is_multiplayer_mode {
            self.apply_or_send(action);
            return GString::new();
        }
        let Some(sim) = &mut self.sim else {
            return GString::new();
        };
        let player_id = sim.player_id;
        let next_tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id,
            tick: next_tick,
            action,
        };
        sim.step(&[cmd], next_tick);
        sim.last_build_message
            .as_deref()
            .map_or_else(GString::new, GString::from)
    }

    // ========================================================================
    // Multiplayer methods
    // ========================================================================

    /// Host a multiplayer game: start an embedded relay server and connect
    /// as the first client. Returns true on success.
    #[func]
    fn host_game(
        &mut self,
        port: i32,
        session_name: GString,
        password: GString,
        max_players: i32,
        ticks_per_turn: i32,
    ) -> bool {
        let pw = if password.to_string().is_empty() {
            None
        } else {
            Some(password.to_string())
        };
        let config = RelayConfig {
            port: port as u16,
            session_name: session_name.to_string(),
            password: pw.clone(),
            ticks_per_turn: ticks_per_turn as u32,
            max_players: max_players as u32,
        };

        let (handle, addr) = match start_relay(config) {
            Ok(result) => result,
            Err(e) => {
                godot_error!("SimBridge: failed to start relay: {e}");
                return false;
            }
        };

        // Small delay to let the listener thread start.
        std::thread::sleep(std::time::Duration::from_millis(50));

        let addr_str = format!("{addr}");
        let config_hash = fnv1a_hash("{}");
        match NetClient::connect(&addr_str, "Host", SIM_VERSION_HASH, config_hash, pw) {
            Ok((client, info)) => {
                self.mp_ticks_per_turn = info.ticks_per_turn;
                self.net_client = Some(client);
                self.relay_handle = Some(handle);
                self.is_multiplayer_mode = true;
                self.is_host = true;
                self.game_started = false;
                godot_print!(
                    "SimBridge: hosting on {addr_str} as player {}",
                    info.player_id.0
                );
                true
            }
            Err(e) => {
                godot_error!("SimBridge: failed to connect to own relay: {e}");
                handle.stop();
                false
            }
        }
    }

    /// Join a remote multiplayer game. Returns true on success.
    #[func]
    fn join_game(&mut self, address: GString, player_name: GString, password: GString) -> bool {
        let pw = if password.to_string().is_empty() {
            None
        } else {
            Some(password.to_string())
        };
        let config_hash = fnv1a_hash("{}");
        match NetClient::connect(
            &address.to_string(),
            &player_name.to_string(),
            SIM_VERSION_HASH,
            config_hash,
            pw,
        ) {
            Ok((client, info)) => {
                self.mp_ticks_per_turn = info.ticks_per_turn;
                self.net_client = Some(client);
                self.is_multiplayer_mode = true;
                self.is_host = false;
                self.game_started = false;
                godot_print!(
                    "SimBridge: joined '{}' as player {}",
                    info.session_name,
                    info.player_id.0
                );
                true
            }
            Err(e) => {
                godot_error!("SimBridge: join_game failed: {e}");
                false
            }
        }
    }

    /// Disconnect from multiplayer. Stops the relay if hosting.
    #[func]
    fn disconnect_multiplayer(&mut self) {
        if let Some(client) = &mut self.net_client {
            client.disconnect();
        }
        self.net_client = None;
        if let Some(handle) = self.relay_handle.take() {
            handle.stop();
        }
        self.is_multiplayer_mode = false;
        self.is_host = false;
        self.game_started = false;
        self.mp_events.clear();
        godot_print!("SimBridge: disconnected from multiplayer");
    }

    /// Return true if in multiplayer mode.
    #[func]
    fn is_multiplayer(&self) -> bool {
        self.is_multiplayer_mode
    }

    /// Return true if this client is the host.
    #[func]
    fn is_host(&self) -> bool {
        self.is_host
    }

    /// Return true if the multiplayer game has started (past lobby).
    #[func]
    fn is_game_started(&self) -> bool {
        self.game_started
    }

    /// Return the ticks_per_turn for the multiplayer session.
    #[func]
    fn mp_ticks_per_turn(&self) -> i32 {
        self.mp_ticks_per_turn as i32
    }

    /// Host only: send StartGame to begin the multiplayer game.
    /// The sim will be initialized when the GameStart message comes back.
    #[func]
    fn start_multiplayer_game(&mut self, seed: i64, config_json: GString) {
        if !self.is_host {
            godot_warn!("SimBridge: only the host can start the game");
            return;
        }
        if let Some(client) = &mut self.net_client
            && let Err(e) = client.send_start_game(seed, &config_json.to_string())
        {
            godot_error!("SimBridge: send_start_game failed: {e}");
        }
    }

    /// Return the list of players in the lobby as an array of dictionaries
    /// with "id" and "name" keys. Only meaningful before game start.
    #[func]
    fn get_lobby_players(&self) -> VarArray {
        // The relay sends PlayerJoined/PlayerLeft which we track as events.
        // For now, return a minimal implementation — the lobby overlay will
        // poll this each frame. We'd need to track the player list from
        // Welcome + join/leave events; for v1 this returns empty and the
        // lobby overlay reads mp_events for join/leave notifications.
        VarArray::new()
    }

    /// Poll the network for incoming messages. Processes Turn messages by
    /// applying their commands to the sim. Returns the number of turns applied.
    ///
    /// Other message types (PlayerJoined, PlayerLeft, ChatBroadcast, etc.)
    /// are pushed into `mp_events` as JSON strings for GDScript to read.
    #[func]
    fn poll_network(&mut self) -> i32 {
        let Some(client) = &self.net_client else {
            return 0;
        };
        let messages = client.poll();
        let mut turns_applied = 0;

        for msg in messages {
            match msg {
                ServerMessage::GameStart { seed, config_json } => {
                    self.game_started = true;
                    // Initialize the sim with the received seed/config.
                    let profile: TreeProfile = serde_json::from_str(&config_json)
                        .unwrap_or_else(|_| TreeProfile::fantasy_mega());
                    let config = GameConfig {
                        tree_profile: profile,
                        ..Default::default()
                    };
                    self.sim = Some(SimState::with_config(seed as u64, config));
                    godot_print!("SimBridge: game started with seed {seed}");
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "game_start",
                            "seed": seed,
                        }))
                        .unwrap_or_default(),
                    );
                }
                ServerMessage::Turn {
                    sim_tick_target,
                    commands,
                    ..
                } => {
                    if let Some(sim) = &mut self.sim {
                        let payloads: Vec<&[u8]> =
                            commands.iter().map(|tc| tc.payload.as_slice()).collect();
                        sim.apply_turn_payloads(sim_tick_target, &payloads);
                        turns_applied += 1;
                    }
                }
                ServerMessage::PlayerJoined { player } => {
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "player_joined",
                            "id": player.id.0,
                            "name": player.name,
                        }))
                        .unwrap_or_default(),
                    );
                }
                ServerMessage::PlayerLeft { player_id, name } => {
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "player_left",
                            "id": player_id.0,
                            "name": name,
                        }))
                        .unwrap_or_default(),
                    );
                }
                ServerMessage::ChatBroadcast { from, name, text } => {
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "chat",
                            "from": from.0,
                            "name": name,
                            "text": text,
                        }))
                        .unwrap_or_default(),
                    );
                }
                ServerMessage::DesyncDetected { tick } => {
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "desync",
                            "tick": tick,
                        }))
                        .unwrap_or_default(),
                    );
                }
                ServerMessage::Paused { by } => {
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "paused",
                            "by": by.0,
                        }))
                        .unwrap_or_default(),
                    );
                }
                ServerMessage::Resumed { by } => {
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "resumed",
                            "by": by.0,
                        }))
                        .unwrap_or_default(),
                    );
                }
                // Welcome, Rejected, SnapshotRequest, SnapshotLoad, SpeedChanged
                // are either handled during connect or not yet implemented.
                _ => {}
            }
        }

        turns_applied
    }

    /// Drain queued multiplayer events as a PackedStringArray of JSON strings.
    /// GDScript parses each string to handle join/leave/chat/desync notifications.
    #[func]
    fn poll_mp_events(&mut self) -> PackedStringArray {
        let mut arr = PackedStringArray::new();
        for event in self.mp_events.drain(..) {
            arr.push(&event);
        }
        arr
    }

    /// Send a chat message in multiplayer.
    #[func]
    fn send_chat(&mut self, text: GString) {
        if let Some(client) = &mut self.net_client
            && let Err(e) = client.send_chat(&text.to_string())
        {
            godot_error!("SimBridge: send_chat failed: {e}");
        }
    }
}

/// FNV-1a hash of a string, used for config hash comparison.
fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
