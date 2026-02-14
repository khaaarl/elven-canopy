// GDExtension bridge class for the simulation.
//
// Exposes a `SimBridge` node that Godot scenes can use to create, step, and
// query the simulation. This is the primary interface between GDScript and
// the Rust sim.
//
// Exposes tree voxel data (for rendering), elf and capybara positions
// (for billboard sprites), and spawn commands. All data is returned in
// packed Godot arrays for efficient transfer.
//
// Internally, elves and capybaras are stored as unified `Creature` entities
// with a `species` field. The bridge filters by species when returning
// positions so the GDScript API is unchanged.
//
// See also: `lib.rs` for the GDExtension entry point, and the
// `elven_canopy_sim` crate for all simulation logic.

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
