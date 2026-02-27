// GDExtension bridge class for the simulation.
//
// Exposes a `SimBridge` node that Godot scenes can use to create, step, and
// query the simulation. This is the sole interface between GDScript and the
// Rust sim — all sim interaction goes through methods on this class.
//
// ## What it exposes
//
// - **Lifecycle:** `init_sim(seed)`, `init_sim_with_tree_profile_json(seed, json)`,
//   `step_to_tick(tick)`, `current_tick()`, `is_initialized()`.
// - **Save/load:** `save_game_json()` returns the sim state as a JSON string,
//   `load_game_json(json)` replaces the current sim from a JSON string.
//   File I/O is handled in GDScript via Godot's `user://` paths.
// - **World data:** `get_trunk_voxels()`, `get_branch_voxels()`,
//   `get_root_voxels()`, `get_leaf_voxels()`, `get_fruit_voxels()` — flat
//   `PackedInt32Array` of (x,y,z) triples. The raw voxel getters still exist
//   but wood rendering now uses the beveled mesh API (see below).
// - **Mesh generation:** `set_mesh_config_json(json)` loads bevel/bark config,
//   `generate_wood_meshes()` builds beveled ArrayMesh geometry for all wood
//   types, then per-type getters (`get_trunk_mesh_vertices()`, etc.) return
//   the cached results as packed arrays. `get_bark_texture(type)` returns a
//   procedural RGBA8 bark texture as `[width_le, height_le, pixels...]`.
// - **Creature positions:** `get_elf_positions()`, `get_capybara_positions()`
//   — `PackedVector3Array` for billboard sprite placement. Internally, all
//   creatures are unified `Creature` entities with a `species` field; the
//   bridge filters by species so the GDScript API has clean per-species calls.
// - **Creature info:** `get_creature_info(species_name, index)` — returns a
//   `VarDictionary` with species, position (x/y/z), and task status for the
//   creature at the given species-filtered index. Used by the creature info
//   panel for display and follow-mode tracking.
// - **Nav nodes:** `get_all_nav_nodes()`, `get_ground_nav_nodes()` — for
//   debug visualization. `get_visible_nav_nodes(cam_pos)`,
//   `get_visible_ground_nav_nodes(cam_pos)` — filtered by voxel-based
//   occlusion (3D DDA raycast in `world.rs`) so the placement UI only snaps
//   to nodes the camera can actually see.
// - **Commands:** `spawn_elf(x,y,z)`, `spawn_capybara(x,y,z)`,
//   `create_goto_task(x,y,z)` — each constructs a `SimCommand` and
//   immediately steps the sim by one tick to apply it.
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
// query callers.

use elven_canopy_sim::command::{SimAction, SimCommand};
use elven_canopy_sim::config::{GameConfig, TreeProfile};
use elven_canopy_sim::sim::SimState;
use elven_canopy_sim::tree_mesh::{self, MeshConfig, MeshData};
use elven_canopy_sim::types::{Species, VoxelCoord};
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
    mesh_config: MeshConfig,
    trunk_mesh: Option<MeshData>,
    branch_mesh: Option<MeshData>,
    root_mesh: Option<MeshData>,
}

#[godot_api]
impl INode for SimBridge {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            sim: None,
            mesh_config: MeshConfig::default(),
            trunk_mesh: None,
            branch_mesh: None,
            root_mesh: None,
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
        let mut config = GameConfig::default();
        config.tree_profile = profile;
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

    /// Return elf positions as a PackedVector3Array.
    #[func]
    fn get_elf_positions(&self) -> PackedVector3Array {
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for creature in sim.creatures.values().filter(|c| c.species == Species::Elf) {
            arr.push(Vector3::new(
                creature.position.x as f32,
                creature.position.y as f32,
                creature.position.z as f32,
            ));
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

    /// Return capybara positions as a PackedVector3Array.
    #[func]
    fn get_capybara_positions(&self) -> PackedVector3Array {
        let Some(sim) = &self.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for creature in sim.creatures.values().filter(|c| c.species == Species::Capybara) {
            arr.push(Vector3::new(
                creature.position.x as f32,
                creature.position.y as f32,
                creature.position.z as f32,
            ));
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
    /// BTreeMap order filtered by species.
    ///
    /// Returns a VarDictionary with keys: "species", "x", "y", "z", "has_task".
    /// Returns an empty VarDictionary if species is unknown or index is out of
    /// bounds.
    #[func]
    fn get_creature_info(&self, species_name: GString, index: i32) -> VarDictionary {
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
                let mut dict = VarDictionary::new();
                dict.set("species", species_name.clone());
                dict.set("x", c.position.x);
                dict.set("y", c.position.y);
                dict.set("z", c.position.z);
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
                godot_print!("SimBridge: loaded save (tick={}, creatures={})",
                    state.tick, state.creatures.len());
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

    // -----------------------------------------------------------------------
    // Beveled wood mesh generation
    // -----------------------------------------------------------------------

    /// Parse mesh config JSON (bevel, bark texture params) into the cached
    /// `MeshConfig`. On parse failure, warns and keeps the existing defaults.
    #[func]
    fn set_mesh_config_json(&mut self, json: GString) {
        match serde_json::from_str::<MeshConfig>(&json.to_string()) {
            Ok(cfg) => {
                self.mesh_config = cfg;
                godot_print!("SimBridge: loaded mesh config (bevel_inset={})", self.mesh_config.bevel_inset);
            }
            Err(e) => {
                godot_warn!("SimBridge: failed to parse mesh config JSON: {e}, keeping defaults");
            }
        }
    }

    /// Generate beveled meshes for all 3 wood types (trunk, branch, root)
    /// from the sim's voxel world and tree voxel lists. Results are cached;
    /// call the `get_*_mesh_*()` getters to retrieve them.
    #[func]
    fn generate_wood_meshes(&mut self) {
        let Some(sim) = &self.sim else { return };
        let tree = match sim.trees.get(&sim.player_tree_id) {
            Some(t) => t,
            None => return,
        };
        self.trunk_mesh = Some(tree_mesh::generate_tree_mesh(
            &sim.world,
            &tree.trunk_voxels,
            &self.mesh_config,
        ));
        self.branch_mesh = Some(tree_mesh::generate_tree_mesh(
            &sim.world,
            &tree.branch_voxels,
            &self.mesh_config,
        ));
        self.root_mesh = Some(tree_mesh::generate_tree_mesh(
            &sim.world,
            &tree.root_voxels,
            &self.mesh_config,
        ));
        godot_print!(
            "SimBridge: generated wood meshes (trunk={} verts, branch={} verts, root={} verts)",
            self.trunk_mesh.as_ref().map_or(0, |m| m.vertices.len() / 3),
            self.branch_mesh.as_ref().map_or(0, |m| m.vertices.len() / 3),
            self.root_mesh.as_ref().map_or(0, |m| m.vertices.len() / 3),
        );
    }

    // --- Trunk mesh getters ---

    #[func]
    fn get_trunk_mesh_vertices(&self) -> PackedFloat32Array {
        mesh_data_to_float_array(self.trunk_mesh.as_ref(), |m| &m.vertices)
    }

    #[func]
    fn get_trunk_mesh_normals(&self) -> PackedFloat32Array {
        mesh_data_to_float_array(self.trunk_mesh.as_ref(), |m| &m.normals)
    }

    #[func]
    fn get_trunk_mesh_uvs(&self) -> PackedFloat32Array {
        mesh_data_to_float_array(self.trunk_mesh.as_ref(), |m| &m.uvs)
    }

    #[func]
    fn get_trunk_mesh_indices(&self) -> PackedInt32Array {
        mesh_data_to_int_array(self.trunk_mesh.as_ref(), |m| &m.indices)
    }

    // --- Branch mesh getters ---

    #[func]
    fn get_branch_mesh_vertices(&self) -> PackedFloat32Array {
        mesh_data_to_float_array(self.branch_mesh.as_ref(), |m| &m.vertices)
    }

    #[func]
    fn get_branch_mesh_normals(&self) -> PackedFloat32Array {
        mesh_data_to_float_array(self.branch_mesh.as_ref(), |m| &m.normals)
    }

    #[func]
    fn get_branch_mesh_uvs(&self) -> PackedFloat32Array {
        mesh_data_to_float_array(self.branch_mesh.as_ref(), |m| &m.uvs)
    }

    #[func]
    fn get_branch_mesh_indices(&self) -> PackedInt32Array {
        mesh_data_to_int_array(self.branch_mesh.as_ref(), |m| &m.indices)
    }

    // --- Root mesh getters ---

    #[func]
    fn get_root_mesh_vertices(&self) -> PackedFloat32Array {
        mesh_data_to_float_array(self.root_mesh.as_ref(), |m| &m.vertices)
    }

    #[func]
    fn get_root_mesh_normals(&self) -> PackedFloat32Array {
        mesh_data_to_float_array(self.root_mesh.as_ref(), |m| &m.normals)
    }

    #[func]
    fn get_root_mesh_uvs(&self) -> PackedFloat32Array {
        mesh_data_to_float_array(self.root_mesh.as_ref(), |m| &m.uvs)
    }

    #[func]
    fn get_root_mesh_indices(&self) -> PackedInt32Array {
        mesh_data_to_int_array(self.root_mesh.as_ref(), |m| &m.indices)
    }

    // --- Bark texture ---

    /// Generate a bark texture for the given wood type ("trunk", "branch", or
    /// "root"). Returns a PackedByteArray: first 8 bytes are width and height
    /// as little-endian u32, followed by RGBA8 pixel data.
    #[func]
    fn get_bark_texture(&self, wood_type: GString) -> PackedByteArray {
        let base_color = match wood_type.to_string().as_str() {
            "trunk" => self.mesh_config.trunk_color,
            "branch" => self.mesh_config.branch_color,
            "root" => self.mesh_config.root_color,
            _ => {
                godot_warn!("SimBridge: unknown wood type '{wood_type}', using trunk color");
                self.mesh_config.trunk_color
            }
        };
        let tex = tree_mesh::generate_bark_texture(&self.mesh_config, base_color);
        let mut arr = PackedByteArray::new();
        arr.extend(tex.width.to_le_bytes());
        arr.extend(tex.height.to_le_bytes());
        arr.extend(tex.pixels.iter().copied());
        arr
    }
}

// ---------------------------------------------------------------------------
// Helpers for packing mesh data into Godot arrays
// ---------------------------------------------------------------------------

fn mesh_data_to_float_array(
    mesh: Option<&MeshData>,
    field: fn(&MeshData) -> &Vec<f32>,
) -> PackedFloat32Array {
    let Some(m) = mesh else {
        return PackedFloat32Array::new();
    };
    let data = field(m);
    let mut arr = PackedFloat32Array::new();
    for &v in data {
        arr.push(v);
    }
    arr
}

fn mesh_data_to_int_array(
    mesh: Option<&MeshData>,
    field: fn(&MeshData) -> &Vec<u32>,
) -> PackedInt32Array {
    let Some(m) = mesh else {
        return PackedInt32Array::new();
    };
    let data = field(m);
    let mut arr = PackedInt32Array::new();
    for &v in data {
        arr.push(v as i32);
    }
    arr
}
