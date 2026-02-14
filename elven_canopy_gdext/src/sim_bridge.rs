// Minimal GDExtension bridge class for the simulation.
//
// Exposes a `SimBridge` node that Godot scenes can use to create, step, and
// query the simulation. This is the primary interface between GDScript and
// the Rust sim.
//
// In Phase 0 this is skeletal — it can create a sim with a seed, step it
// forward, and report basic state. As features are added, this bridge will
// grow to handle commands, entity queries, and event streaming.
//
// See also: `lib.rs` for the GDExtension entry point, and the
// `elven_canopy_sim` crate for all simulation logic.

use elven_canopy_sim::sim::SimState;
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
}
