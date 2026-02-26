// GDExtension bridge class for the simulation.
//
// Exposes a `SimBridge` node that Godot scenes can use to create, step, and
// query the simulation. This is the sole interface between GDScript and the
// Rust sim — all sim interaction goes through methods on this class.
//
// ## What it exposes
//
// - **Lifecycle:** `init_sim(seed)`, `step_to_tick(tick)`, `current_tick()`,
//   `is_initialized()`.
// - **World data:** `get_trunk_voxels()`, `get_branch_voxels()`,
//   `get_root_voxels()`, `get_leaf_voxels()`, `get_fruit_voxels()` — flat
//   `PackedInt32Array` of (x,y,z) triples for voxel mesh rendering.
// - **Creature positions:** `get_elf_positions()`, `get_capybara_positions()`
//   — `PackedVector3Array` for billboard sprite placement. Internally, all
//   creatures are unified `Creature` entities with a `species` field; the
//   bridge filters by species so the GDScript API has clean per-species calls.
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
// `spawn_toolbar.gd` for the GDScript callers.

use elven_canopy_sim::command::{SimAction, SimCommand};
use elven_canopy_sim::sim::SimState;
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
}
