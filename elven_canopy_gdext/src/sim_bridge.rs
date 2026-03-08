// GDExtension bridge class for the simulation.
//
// Exposes a `SimBridge` node that Godot scenes can use to create, step, and
// query the simulation. This is the sole interface between GDScript and the
// Rust sim — all sim interaction goes through methods on this class.
//
// All sim access goes through a `GameSession` (`session.rs`). The session
// owns the `Option<SimState>` and processes all mutations via typed
// `SessionMessage`s. A `LocalRelay` (`local_relay.rs`) converts wall-clock
// deltas into `AdvanceTo` messages for single-player tick pacing.
//
// ## What it exposes
//
// - **Lifecycle:** `init_sim(seed)`, `init_sim_with_tree_profile_json(seed, json)`,
//   `current_tick()`, `is_initialized()`, `tick_duration_ms()`.
// - **Frame update:** `frame_update(delta)` — unified per-frame entry point.
//   Handles tick pacing (`LocalRelay` in SP, network polling in MP) and
//   returns a fractional render_tick for smooth creature interpolation.
// - **Speed control:** `get_sim_speed()` returns the current speed as a string,
//   `sim_speed_multiplier()` returns the time multiplier for tick pacing,
//   `set_sim_speed(speed_name)` applies pause/resume/speed to the session.
//   In multiplayer, sends to the relay first and applies locally only on
//   success (optimistic update).
// - **Save/load:** `save_game_json()` returns the sim state as a JSON string,
//   `load_game_json(json)` replaces the current sim from a JSON string.
//   File I/O is handled in GDScript via Godot's `user://` paths.
// - **World data / chunk mesh:** `build_world_mesh()` builds the initial
//   chunk mesh cache, `update_world_mesh()` incrementally regenerates dirty
//   chunks, `build_chunk_array_mesh(cx,cy,cz)` returns a Godot `ArrayMesh`
//   for one chunk. `get_fruit_voxels()` — flat `PackedInt32Array` of (x,y,z)
//   triples for fruit SphereMesh rendering (fruit is not part of chunk mesh).
// - **Creature positions:** `get_creature_positions(species_name, render_tick)`
//   — generic `PackedVector3Array` for billboard sprite placement, replacing
//   the per-species `get_elf_positions()` / `get_capybara_positions()` (which
//   remain as thin wrappers). The `render_tick` parameter (a fractional tick
//   returned by `frame_update()`) enables smooth interpolation between nav
//   nodes via `Creature::interpolated_position()`.
// - **Notifications:** `get_notifications_after(after_id)` polls for new
//   notifications (returns `VarArray` of dicts with id/tick/message),
//   `get_max_notification_id()` returns the highest ID (for initializing the
//   cursor after load), `send_debug_notification(message)` sends a test
//   notification through the full command pipeline (multiplayer-aware).
// - **Creature info:** `get_creature_info(species_name, index, render_tick)` —
//   returns a `VarDictionary` with species, interpolated position (x/y/z),
//   task status, task_kind, food level, food_max, rest level, rest_max,
//   name (Vaelith name for elves, empty for other species), name_meaning
//   (English gloss), and inventory (array of {kind, quantity} dicts). Used
//   by the creature info panel for display and follow-mode tracking.
// - **Creature summary:** `get_all_creatures_summary()` — returns a `VarArray`
//   of `VarDictionary`, one per creature, sorted (elves first by name, then
//   other species by species+index). Each dict: species, index, name,
//   name_meaning, has_task, task_kind. Used by `units_panel.gd`.
// - **Task list:** `get_active_tasks()` — returns a `VarArray` of
//   `VarDictionary`, one per non-complete task. Each dict includes short/full
//   ID, kind, state, origin (PlayerDirected/Autonomous/Automated),
//   progress/total_cost, location coordinates, and an assignees array with
//   creature species, index, and name. Used by `task_panel.gd`.
// - **Nav nodes:** `get_all_nav_nodes()`, `get_ground_nav_nodes()` — for
//   debug visualization. `get_visible_nav_nodes(cam_pos)`,
//   `get_visible_ground_nav_nodes(cam_pos)` — filtered by voxel-based
//   occlusion (3D DDA raycast in `world.rs`) so the placement UI only snaps
//   to nodes the camera can actually see.
// - **Commands:** `spawn_creature(species_name, x,y,z)` — generic creature
//   spawner replacing `spawn_elf()` / `spawn_capybara()` (which remain as
//   thin wrappers). Also `create_goto_task(x,y,z)`, `designate_build(x,y,z)`,
//   `designate_build_rect(x,y,z,width,depth)`, etc. All commands are
//   buffered and execute on the next `frame_update()`, with identical
//   behavior in SP and MP. Build/carve validation is done upfront by the
//   `validate_*_preview()` query methods — the designation commands
//   themselves are fire-and-forget.
//   `furnish_structure(structure_id, furnishing_type)` begins furnishing a
//   completed building. `get_furniture_positions()` returns flat (x,y,z,kind)
//   quads of placed furniture for rendering.
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
//   voxels). Materialized construction voxels are now part of the chunk
//   mesh system (rendered by tree_renderer.gd alongside tree geometry).
// - **Carving:** `designate_carve(x,y,z)` and
//   `designate_carve_rect(x,y,z,w,d,h)` designate voxels for removal.
//   `validate_carve_preview(x,y,z,w,d,h)` performs structural integrity
//   analysis on the proposed carve region and returns `{tier, message}`
//   for ghost preview coloring (Ok/Warning/Blocked).
//   `get_carve_blueprint_voxels()` returns flat (x,y,z) triples for
//   uncarved voxels in carve blueprints, consumed by
//   `blueprint_renderer.gd`. All voxel getters (trunk, branch, leaf,
//   root, dirt, fruit, platform) skip voxels carved to Air so they
//   disappear from rendering immediately.
// - **Stats:** `creature_count_by_name(species_name)` — generic replacement
//   for `elf_count()` / `capybara_count()` (which remain as thin wrappers).
//   Also `fruit_count()`, `home_tree_mana()`.
// - **Tree info:** `get_home_tree_info()` — returns a `VarDictionary` with
//   the player's home tree stats: health, growth, mana, fruit, carrying
//   capacity, voxel counts by type, height, spread, and anchor position.
//   Used by `tree_info_panel.gd`.
// - **Structures:** `get_structures()` — returns a `VarArray` of
//   `VarDictionary`, one per completed structure (id, name, kind, location,
//   size). `raycast_structure(origin, dir)` — DDA voxel raycast returning
//   the `StructureId` under the cursor (or -1 for miss).
//   `get_structure_info(id)` — returns a `VarDictionary` with detailed info
//   including `name` (display name) and `has_custom_name` (bool) for the
//   info panel. `rename_structure(id, name)` — set or clear (empty string)
//   a structure's custom name. `set_cooking_config(id, enabled, bread_target)`
//   — configure cooking on a kitchen building.
//   `set_workshop_config(id, enabled, recipe_ids)` — configure workshop
//   recipes. `get_recipes()` — returns all available recipe definitions.
// - **Ground piles:** `get_ground_piles()` — returns a `VarArray` of
//   `{x, y, z, inventory: [{kind, quantity}]}` dicts.
//   `get_ground_pile_info(x,y,z)` — returns a single pile's dict (same
//   format) or empty dict if no pile at that position. Used by the pile
//   info panel for display and per-frame refresh.
// - **Species queries:** `is_species_ground_only(species_name)` — used by
//   the placement controller to decide which nav nodes to show.
//   `get_all_species_names()` — returns all species names for UI iteration.
// - **Placement raycasting:** `raycast_solid(origin, dir)` — DDA raycast
//   returning the first solid voxel and entry face as a Dictionary.
//   `get_voxel_solidity_slice(y, cx, cz, radius)` — solid/air grid for
//   height-slice wireframe rendering. `auto_ladder_orientation(x,y,z,h)`
//   — picks the best facing for a ladder column. `get_world_size()` —
//   world dimensions for clamping.
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
use elven_canopy_sim::checksum::CHECKSUM_INTERVAL_TICKS;
use elven_canopy_sim::command::SimAction;
use elven_canopy_sim::config::{GameConfig, TreeProfile};
use elven_canopy_sim::local_relay::LocalRelay;
use elven_canopy_sim::session::{
    GameSession, SessionMessage, SessionPlayerId, SessionSpeed, speed_to_ticks_per_turn,
    ticks_per_turn_to_speed,
};
use elven_canopy_sim::structural::{self, ValidationTier};
use elven_canopy_sim::task::{TaskOrigin, TaskState};
use elven_canopy_sim::types::{
    BuildType, CreatureId, FaceDirection, FurnishingType, FurnitureKind, LadderKind,
    OverlapClassification, Priority, SimUuid, Species, StructureId, VoxelCoord, VoxelType,
};
use godot::prelude::*;

use elven_canopy_relay::client::NetClient;

use crate::mesh_cache::MeshCache;

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
        "Goblin" => Some(Species::Goblin),
        "Monkey" => Some(Species::Monkey),
        "Orc" => Some(Species::Orc),
        "Squirrel" => Some(Species::Squirrel),
        "Troll" => Some(Species::Troll),
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
        Species::Goblin => "Goblin",
        Species::Monkey => "Monkey",
        Species::Orc => "Orc",
        Species::Squirrel => "Squirrel",
        Species::Troll => "Troll",
    }
}

/// Godot node that owns and drives the simulation.
///
/// Add this as a child node in your main scene. Call `init_sim()` from
/// GDScript to create the simulation, then `frame_update(delta)` each
/// frame — it handles tick pacing (via `LocalRelay` in SP, network polling
/// in MP) and returns a fractional render_tick for smooth interpolation.
/// In multiplayer, `poll_network()` automatically sends a state checksum
/// to the relay every `CHECKSUM_INTERVAL_TICKS` (1000 ticks) for desync
/// detection (see `checksum.rs` in the sim crate).
#[derive(GodotClass)]
#[class(base=Node)]
pub struct SimBridge {
    base: Base<Node>,
    session: GameSession,
    local_relay: Option<LocalRelay>,
    local_player_id: SessionPlayerId,
    // Chunk mesh cache — not part of SimState, lives here for rendering.
    mesh_cache: Option<MeshCache>,
    // Multiplayer state
    net_client: Option<NetClient>,
    relay_handle: Option<RelayHandle>,
    is_multiplayer_mode: bool,
    mp_events: Vec<String>,
    mp_ticks_per_turn: u32,
    mp_time_since_turn: f64,
}

#[godot_api]
impl INode for SimBridge {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            session: GameSession::new_singleplayer(),
            local_relay: None,
            local_player_id: SessionPlayerId::LOCAL,
            mesh_cache: None,
            net_client: None,
            relay_handle: None,
            is_multiplayer_mode: false,
            mp_events: Vec::new(),
            mp_ticks_per_turn: 50,
            mp_time_since_turn: 0.0,
        }
    }
}

#[godot_api]
impl SimBridge {
    /// Initialize the simulation with the given seed and default config.
    #[func]
    fn init_sim(&mut self, seed: i64) {
        let config = GameConfig::default();
        let seconds_per_tick = config.tick_duration_ms as f64 / 1000.0;
        self.session.process(SessionMessage::StartGame {
            seed: seed as u64,
            config: Box::new(config),
        });
        self.local_relay = Some(LocalRelay::new(seconds_per_tick));
        self.rebuild_mesh_cache();
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
        let seconds_per_tick = config.tick_duration_ms as f64 / 1000.0;
        self.session.process(SessionMessage::StartGame {
            seed: seed as u64,
            config: Box::new(config),
        });
        self.local_relay = Some(LocalRelay::new(seconds_per_tick));
        self.rebuild_mesh_cache();
        godot_print!("SimBridge: simulation initialized with seed {seed} and custom tree profile");
    }

    /// Advance the simulation to the target tick, processing all events.
    #[func]
    fn step_to_tick(&mut self, target_tick: i64) {
        self.session.process(SessionMessage::AdvanceTo {
            tick: target_tick as u64,
        });
    }

    /// Return the current simulation tick.
    #[func]
    fn current_tick(&self) -> i64 {
        self.session.current_tick() as i64
    }

    /// Return the mana stored in the player's home tree.
    #[func]
    fn home_tree_mana(&self) -> f32 {
        self.session.sim.as_ref().map_or(0.0, |s| {
            s.trees
                .get(&s.player_tree_id)
                .map_or(0.0, |t| t.mana_stored)
        })
    }

    /// Return true if the simulation has been initialized.
    #[func]
    fn is_initialized(&self) -> bool {
        self.session.has_sim()
    }

    /// Return the simulation tick duration in milliseconds. The GDScript
    /// frame loop uses this to compute how many ticks to advance per frame
    /// (tick_duration_ms=1 → 1000 ticks/sec).
    #[func]
    fn tick_duration_ms(&self) -> i32 {
        self.session
            .sim
            .as_ref()
            .map_or(1, |s| s.config.tick_duration_ms as i32)
    }

    /// Return the current simulation speed as a string ("Paused", "Normal",
    /// "Fast", or "VeryFast").
    #[func]
    fn get_sim_speed(&self) -> GString {
        if self.session.is_paused() {
            return "Paused".into();
        }
        match self.session.current_speed() {
            SessionSpeed::Normal => "Normal",
            SessionSpeed::Fast => "Fast",
            SessionSpeed::VeryFast => "VeryFast",
        }
        .into()
    }

    /// Return the time multiplier for the current simulation speed.
    #[func]
    fn sim_speed_multiplier(&self) -> f64 {
        self.session.speed_multiplier()
    }

    /// Set the simulation speed by name. In single-player, applies directly
    /// to the session. In multiplayer, sends to the relay first — the session
    /// is updated as an optimistic local prediction only after the send
    /// succeeds (the relay broadcast will arrive later, but session ops are
    /// idempotent so the duplicate is harmless). If the relay send fails,
    /// the session is left unchanged to prevent desync.
    #[func]
    fn set_sim_speed(&mut self, speed_name: GString) {
        let speed_str = speed_name.to_string();
        let is_pause = speed_str == "Paused";

        let session_speed = match speed_str.as_str() {
            "Paused" => None,
            "Normal" => Some(SessionSpeed::Normal),
            "Fast" => Some(SessionSpeed::Fast),
            "VeryFast" => Some(SessionSpeed::VeryFast),
            _ => return,
        };

        let was_paused = self.session.is_paused();

        // In MP, send to relay first. Only apply locally if send succeeds.
        if self.is_multiplayer_mode
            && let Some(client) = &mut self.net_client
        {
            if is_pause {
                if let Err(e) = client.send_pause() {
                    godot_error!("SimBridge: send_pause failed: {e}");
                    return;
                }
            } else {
                if was_paused && let Err(e) = client.send_resume() {
                    godot_error!("SimBridge: send_resume failed: {e}");
                    return;
                }
                if let Some(speed) = session_speed {
                    let tpt = speed_to_ticks_per_turn(speed);
                    if let Err(e) = client.send_set_speed(tpt) {
                        godot_error!("SimBridge: send_set_speed failed: {e}");
                        return;
                    }
                }
            }
        }

        // Apply to session (SP: sole authority; MP: optimistic update after
        // successful relay send).
        let pid = self.local_player_id;
        if is_pause {
            self.session.process(SessionMessage::Pause { by: pid });
        } else {
            if was_paused {
                self.session.process(SessionMessage::Resume { by: pid });
            }
            if let Some(speed) = session_speed {
                self.session.process(SessionMessage::SetSpeed { speed });
            }
        }
    }

    /// Return fruit voxel positions as a flat PackedInt32Array (x,y,z triples).
    /// Kept as a separate method because fruit is rendered as SphereMesh
    /// MultiMesh, not part of the chunk mesh system.
    #[func]
    fn get_fruit_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.session.sim else {
            return PackedInt32Array::new();
        };
        let tree = match sim.trees.get(&sim.player_tree_id) {
            Some(t) => t,
            None => return PackedInt32Array::new(),
        };
        let mut arr = PackedInt32Array::new();
        for v in &tree.fruit_positions {
            // Skip voxels carved to Air so the renderer doesn't draw them.
            if sim.world.get(*v) == VoxelType::Air {
                continue;
            }
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
        let Some(sim) = &self.session.sim else {
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
        self.session.sim.as_ref().map_or(0, |s| {
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

    /// Route a build/carve action through the session (SP) or relay (MP).
    /// The command is buffered and executes on the next `frame_update()`,
    /// identical in both modes. Returns empty — validation feedback comes
    /// from the `validate_*_preview()` methods that GDScript calls before
    /// confirming placement.
    fn apply_build_action(&mut self, action: SimAction) -> GString {
        self.apply_or_send(action);
        GString::new()
    }

    /// Apply a SimAction locally (single-player) or send it to the relay
    /// (multiplayer). In single-player, the command is buffered in the
    /// session and executed on the next `frame_update()` call (tick
    /// pacing is driven by `LocalRelay`). In multiplayer, the action is
    /// sent over the network and applied when it comes back in a Turn.
    fn apply_or_send(&mut self, action: SimAction) {
        if self.is_multiplayer_mode {
            if let Some(client) = &mut self.net_client
                && let Ok(json) = serde_json::to_vec(&action)
                && let Err(e) = client.send_command(&json)
            {
                godot_error!("SimBridge: send_command failed: {e}");
            }
        } else {
            self.session.process(SessionMessage::SimCommand {
                from: self.local_player_id,
                action,
            });
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
        let Some(sim) = &self.session.sim else {
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
        let Some(sim) = &self.session.sim else {
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
        let Some(sim) = &self.session.sim else {
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
        let Some(sim) = &self.session.sim else {
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
    /// Returns a VarDictionary with keys: "species", "x", "y", "z", "has_task",
    /// "food", "food_max", "rest", "rest_max", "name", "name_meaning",
    /// "assigned_home", "thoughts". "thoughts" is a VarArray of dicts with
    /// "text" and "tick" keys, most recent first. Returns an empty VarDictionary
    /// if species is unknown or index is out of bounds.
    #[func]
    fn get_creature_info(
        &self,
        species_name: GString,
        index: i32,
        render_tick: f64,
    ) -> VarDictionary {
        let Some(sim) = &self.session.sim else {
            return VarDictionary::new();
        };
        let Some(species) = parse_species(&species_name.to_string()) else {
            return VarDictionary::new();
        };
        let creature = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == species)
            .nth(index as usize);
        match creature {
            Some(c) => {
                let (x, y, z): (f32, f32, f32) = c.interpolated_position(render_tick);
                let mut dict = VarDictionary::new();
                dict.set("species", species_name.clone());
                dict.set("x", x);
                dict.set("y", y);
                dict.set("z", z);
                dict.set("has_task", c.current_task.is_some());
                let task_kind_str = c
                    .current_task
                    .as_ref()
                    .and_then(|tid| sim.db.tasks.get(tid).map(|t| t.kind_tag.display_name()))
                    .unwrap_or("");
                dict.set("task_kind", GString::from(task_kind_str));
                // Task location — resolve NavNodeId to VoxelCoord when a task exists.
                if let Some(tid) = &c.current_task
                    && let Some(task) = sim.db.tasks.get(tid)
                {
                    let task_pos = sim.nav_graph.node(task.location).position;
                    dict.set("task_location_x", task_pos.x);
                    dict.set("task_location_y", task_pos.y);
                    dict.set("task_location_z", task_pos.z);
                }
                dict.set("food", c.food);
                let food_max = sim.species_table[&species].food_max;
                dict.set("food_max", food_max);
                dict.set("rest", c.rest);
                let rest_max = sim.species_table[&species].rest_max;
                dict.set("rest_max", rest_max);
                dict.set("name", GString::from(c.name.as_str()));
                dict.set("name_meaning", GString::from(c.name_meaning.as_str()));
                let assigned_home = match c.assigned_home {
                    Some(sid) => sid.0 as i64,
                    None => -1,
                };
                dict.set("assigned_home", assigned_home);
                // Thoughts: array of dicts with "text" and "tick", most recent first.
                let mut thoughts_arr = VarArray::new();
                let creature_thoughts = sim
                    .db
                    .thoughts
                    .by_creature_id(&c.id, elven_canopy_sim::tabulosity::QueryOpts::ASC);
                for thought in creature_thoughts.iter().rev() {
                    let mut td = VarDictionary::new();
                    td.set("text", GString::from(thought.kind.description()));
                    td.set("tick", thought.tick as i64);
                    thoughts_arr.push(&td.to_variant());
                }
                dict.set("thoughts", thoughts_arr);

                // Mood.
                let mood_score: i32 = creature_thoughts
                    .iter()
                    .map(|t| sim.config.mood.mood_weight(&t.kind))
                    .sum();
                let mood_tier = sim.config.mood.tier(mood_score);
                dict.set("mood_score", mood_score);
                let tier_label: &str = mood_tier.label();
                dict.set("mood_tier", GString::from(tier_label));

                // Inventory.
                let mut inv_arr = VarArray::new();
                for stack in sim.inv_items(c.inventory_id) {
                    let mut item_dict = VarDictionary::new();
                    item_dict.set("kind", GString::from(stack.kind.display_name()));
                    item_dict.set("quantity", stack.quantity as i64);
                    inv_arr.push(&item_dict.to_variant());
                }
                dict.set("inventory", inv_arr);
                dict
            }
            None => VarDictionary::new(),
        }
    }

    /// Return a summary of all creatures as a `VarArray` of dictionaries.
    ///
    /// Each dictionary contains: `species` (String), `index` (i32),
    /// `name` (String — Vaelith name for elves, empty for other species),
    /// `name_meaning` (String), `has_task` (bool), `task_kind` (String).
    ///
    /// Results are sorted: elves first (alphabetically by name), then other
    /// species grouped alphabetically by species name, then by index within
    /// each species.
    ///
    /// Used by `units_panel.gd` for the full creature roster. Returns data
    /// for all creatures in a single call to avoid N individual
    /// `get_creature_info()` round-trips per frame.
    #[func]
    fn get_all_creatures_summary(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };

        // Collect (species, index, name, name_meaning, has_task, task_kind)
        // tuples per species.
        let mut entries: Vec<(&'static str, i32, &str, &str, bool, &str)> = Vec::new();

        // Count per species to compute species-filtered indices.
        let species_list = [
            Species::Elf,
            Species::Boar,
            Species::Capybara,
            Species::Deer,
            Species::Elephant,
            Species::Monkey,
            Species::Squirrel,
        ];

        for &sp in &species_list {
            for (idx, creature) in
                (0_i32..).zip(sim.db.creatures.iter_all().filter(|c| c.species == sp))
            {
                let task_kind = creature
                    .current_task
                    .as_ref()
                    .and_then(|tid| sim.db.tasks.get(tid).map(|t| t.kind_tag.display_name()))
                    .unwrap_or("");

                entries.push((
                    species_name(sp),
                    idx,
                    creature.name.as_str(),
                    creature.name_meaning.as_str(),
                    creature.current_task.is_some(),
                    task_kind,
                ));
            }
        }

        // Sort: elves first (alphabetically by name), then other species
        // (alphabetically by species name, then by index).
        entries.sort_by(|a, b| {
            let a_is_elf = a.0 == "Elf";
            let b_is_elf = b.0 == "Elf";
            match (a_is_elf, b_is_elf) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (true, true) => a.2.cmp(b.2), // both elves: sort by name
                (false, false) => a.0.cmp(b.0).then(a.1.cmp(&b.1)),
            }
        });

        let mut result = VarArray::new();
        for (sp, idx, name, meaning, has_task, task_kind) in &entries {
            let mut dict = VarDictionary::new();
            dict.set("species", GString::from(*sp));
            dict.set("index", *idx);
            dict.set("name", GString::from(*name));
            dict.set("name_meaning", GString::from(*meaning));
            dict.set("has_task", *has_task);
            dict.set("task_kind", GString::from(*task_kind));
            result.push(&dict.to_variant());
        }
        result
    }

    /// Return all non-complete tasks as a `VarArray` of dictionaries.
    ///
    /// Each dictionary contains: `id` (short hex), `id_full` (full UUID),
    /// `kind` ("GoTo", "Build", "EatBread", "EatFruit", "Sleep", "Furnish", "Haul", "Cook",
    /// "Harvest", "AcquireItem", "Moping", or "Craft"),
    /// `origin` ("PlayerDirected", "Autonomous", or "Automated"),
    /// `state` ("Available" or "In Progress"), `progress`, `total_cost`,
    /// `location_x/y/z`, and `assignees` (array of dictionaries with
    /// `id_short`, `name`, `species`, `index`).
    ///
    /// The creature `index` matches the species-filtered iteration order used
    /// by `get_creature_positions()`, so GDScript can use it directly for
    /// camera follow and selection.
    #[func]
    fn get_active_tasks(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };

        let mut result = VarArray::new();
        for task in sim.db.tasks.iter_all() {
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
            dict.set("kind", GString::from(task.kind_tag.display_name()));

            // Origin.
            let origin_str = match task.origin {
                TaskOrigin::PlayerDirected => "PlayerDirected",
                TaskOrigin::Autonomous => "Autonomous",
                TaskOrigin::Automated => "Automated",
            };
            dict.set("origin", GString::from(origin_str));

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

            // Assignees — query creatures assigned to this task.
            let mut assignees_arr = VarArray::new();
            for creature in sim
                .db
                .creatures
                .by_current_task(&Some(task.id), elven_canopy_sim::tabulosity::QueryOpts::ASC)
            {
                let mut a = VarDictionary::new();
                let cid_full = creature.id.0.to_string();
                let cid_short: String = cid_full.chars().take(8).collect();
                a.set("id_short", GString::from(&cid_short));
                a.set("name", GString::from(creature.name.as_str()));

                let sp = species_name(creature.species);
                a.set("species", GString::from(sp));

                // Compute the species-filtered index: count how many
                // creatures of the same species come before this one in
                // iteration order.
                let index = sim
                    .db
                    .creatures
                    .iter_all()
                    .filter(|c| c.species == creature.species)
                    .position(|c| c.id == creature.id)
                    .unwrap_or(0);
                a.set("index", index as i32);

                assignees_arr.push(&a.to_variant());
            }
            dict.set("assignees", assignees_arr);

            result.push(&dict.to_variant());
        }
        result
    }

    /// Return all completed structures as a `VarArray` of dictionaries.
    ///
    /// Each dictionary contains: `id` (int), `build_type` (String),
    /// `name` (String — display name, custom or auto-generated),
    /// `anchor_x/y/z` (int), `width/depth/height` (int).
    /// Used by `structure_list_panel.gd` for the browsable structure list.
    #[func]
    fn get_structures(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let mut result = VarArray::new();
        for structure in sim.db.structures.iter_all() {
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
                BuildType::Carve => "Carve",
            };
            dict.set("build_type", GString::from(build_type_str));
            dict.set("name", GString::from(&structure.display_name()));
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

    /// Cast a ray and return the `StructureId` (as i64) of the first structure
    /// voxel hit, or -1 if no structure was hit. Used by `selection_controller.gd`
    /// to identify which structure the player clicked on.
    #[func]
    fn raycast_structure(&self, origin: Vector3, dir: Vector3) -> i64 {
        let Some(sim) = &self.session.sim else {
            return -1;
        };
        let from = [origin.x, origin.y, origin.z];
        let d = [dir.x, dir.y, dir.z];
        match sim.raycast_structure(from, d, 500) {
            Some(sid) => sid.0 as i64,
            None => -1,
        }
    }

    /// Cast a ray and return the first solid voxel hit and entry face.
    /// Returns `{hit: true, voxel: Vector3i, face: int}` or `{hit: false}`.
    /// Used by `construction_controller.gd` for building/ladder placement.
    #[func]
    fn raycast_solid(&self, origin: Vector3, dir: Vector3) -> VarDictionary {
        let mut dict = VarDictionary::new();
        let Some(sim) = &self.session.sim else {
            dict.set("hit", false);
            return dict;
        };
        let from = [origin.x, origin.y, origin.z];
        let d = [dir.x, dir.y, dir.z];
        match sim.raycast_solid(from, d, 500) {
            Some((coord, face)) => {
                dict.set("hit", true);
                dict.set("voxel", Vector3i::new(coord.x, coord.y, coord.z));
                dict.set("face", face as i32);
            }
            None => {
                dict.set("hit", false);
            }
        }
        dict
    }

    /// Return a square grid of solid/air flags at the given Y-level, centered
    /// on `(cx, cz)` with the given radius. Returns a `PackedByteArray` of
    /// `(2*radius+1)^2` bytes, row-major (X varies fastest). 1=solid, 0=air.
    /// Used by `height_grid_renderer.gd` for wireframe dimming over solid voxels.
    #[func]
    fn get_voxel_solidity_slice(&self, y: i32, cx: i32, cz: i32, radius: i32) -> PackedByteArray {
        let Some(sim) = &self.session.sim else {
            return PackedByteArray::new();
        };
        let side = (2 * radius + 1) as usize;
        let mut data = Vec::with_capacity(side * side);
        for z in (cz - radius)..=(cz + radius) {
            for x in (cx - radius)..=(cx + radius) {
                let coord = VoxelCoord::new(x, y, z);
                data.push(if sim.world.get(coord).is_solid() {
                    1u8
                } else {
                    0u8
                });
            }
        }
        PackedByteArray::from(data.as_slice())
    }

    /// Return the best ladder orientation for a column at `(x, y..y+height, z)`.
    /// Returns the face direction index (0=PosX, 1=NegX, 4=PosZ, 5=NegZ).
    /// Used by `construction_controller.gd` for auto-orientation.
    #[func]
    fn auto_ladder_orientation(&self, x: i32, y: i32, z: i32, height: i32) -> i32 {
        let Some(sim) = &self.session.sim else {
            return 0;
        };
        sim.auto_ladder_orientation(x, y, z, height) as i32
    }

    /// Return the world dimensions as `Vector3i(size_x, size_y, size_z)`.
    /// Used by GDScript for clamping placement coordinates to world bounds.
    #[func]
    fn get_world_size(&self) -> Vector3i {
        let Some(sim) = &self.session.sim else {
            return Vector3i::new(0, 0, 0);
        };
        Vector3i::new(
            sim.world.size_x as i32,
            sim.world.size_y as i32,
            sim.world.size_z as i32,
        )
    }

    /// Return info about a completed structure as a Dictionary. Returns an
    /// empty dict if the structure_id is not found. Used by
    /// `structure_info_panel.gd` for display.
    #[func]
    fn get_structure_info(&self, structure_id: i64) -> VarDictionary {
        let Some(sim) = &self.session.sim else {
            return VarDictionary::new();
        };
        let sid = StructureId(structure_id as u64);
        let Some(structure) = sim.db.structures.get(&sid) else {
            return VarDictionary::new();
        };
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
            BuildType::Carve => "Carve",
        };
        dict.set("build_type", GString::from(build_type_str));
        dict.set("name", GString::from(&structure.display_name()));
        dict.set("has_custom_name", structure.name.is_some());
        dict.set("anchor_x", structure.anchor.x);
        dict.set("anchor_y", structure.anchor.y);
        dict.set("anchor_z", structure.anchor.z);
        dict.set("width", structure.width);
        dict.set("depth", structure.depth);
        dict.set("height", structure.height);
        dict.set("completed_tick", structure.completed_tick as i64);

        // Furnishing data.
        let furnishing_str = match &structure.furnishing {
            Some(ft) => ft.display_str(),
            None => "",
        };
        dict.set("furnishing", GString::from(furnishing_str));
        let furniture_kind_str = match &structure.furnishing {
            Some(ft) => ft.furniture_kind().noun_plural(),
            None => "",
        };
        dict.set("furniture_noun", GString::from(furniture_kind_str));
        let all_furn = sim
            .db
            .furniture
            .by_structure_id(&sid, elven_canopy_sim::tabulosity::QueryOpts::ASC);
        let placed_count = all_furn.iter().filter(|f| f.placed).count();
        dict.set("furniture_count", placed_count as i64);
        dict.set("planned_furniture_count", all_furn.len() as i64);
        // Check if there's an active Furnish task for this structure.
        let is_furnishing = sim
            .db
            .task_structure_refs
            .by_structure_id(&sid, elven_canopy_sim::tabulosity::QueryOpts::ASC)
            .iter()
            .any(|r| {
                r.role == elven_canopy_sim::db::TaskStructureRole::FurnishTarget
                    && sim
                        .db
                        .tasks
                        .get(&r.task_id)
                        .is_some_and(|t| t.state != elven_canopy_sim::task::TaskState::Complete)
            });
        dict.set("is_furnishing", is_furnishing);

        // Home assignment data — query creatures by assigned_home.
        let occupant = sim
            .db
            .creatures
            .by_assigned_home(&Some(sid), elven_canopy_sim::tabulosity::QueryOpts::ASC)
            .into_iter()
            .next();
        let (assigned_elf_id, assigned_elf_name) = if let Some(elf) = occupant {
            (elf.id.0.to_string(), elf.name.clone())
        } else {
            (String::new(), String::new())
        };
        dict.set("assigned_elf_id", GString::from(&assigned_elf_id));
        dict.set("assigned_elf_name", GString::from(&assigned_elf_name));

        // Inventory.
        let mut inv_arr = VarArray::new();
        for stack in sim.inv_items(structure.inventory_id) {
            let mut item_dict = VarDictionary::new();
            item_dict.set("kind", GString::from(stack.kind.display_name()));
            item_dict.set("quantity", stack.quantity as i64);
            inv_arr.push(&item_dict.to_variant());
        }
        dict.set("inventory", inv_arr);

        // Logistics.
        let logistics_priority: i64 = match structure.logistics_priority {
            Some(p) => p as i64,
            None => -1,
        };
        dict.set("logistics_priority", logistics_priority);
        let mut wants_arr = VarArray::new();
        for want in sim.inv_wants(structure.inventory_id) {
            let mut want_dict = VarDictionary::new();
            want_dict.set("kind", GString::from(want.item_kind.display_name()));
            want_dict.set("target_quantity", want.target_quantity as i64);
            wants_arr.push(&want_dict.to_variant());
        }
        dict.set("logistics_wants", wants_arr);

        // Cooking data (for Kitchen buildings).
        dict.set("cooking_enabled", structure.cooking_enabled);
        dict.set(
            "cooking_bread_target",
            structure.cooking_bread_target as i64,
        );
        let cook_status = if structure.furnishing
            == Some(elven_canopy_sim::types::FurnishingType::Kitchen)
            && structure.cooking_enabled
        {
            // Check bread count vs target.
            let bread_count: u32 = sim.inv_item_count(
                structure.inventory_id,
                elven_canopy_sim::inventory::ItemKind::Bread,
            );
            if bread_count >= structure.cooking_bread_target {
                "Bread target reached"
            } else {
                // Check for active Cook task.
                let has_cook_task = sim
                    .db
                    .task_structure_refs
                    .by_structure_id(&sid, elven_canopy_sim::tabulosity::QueryOpts::ASC)
                    .iter()
                    .any(|r| {
                        r.role == elven_canopy_sim::db::TaskStructureRole::CookAt
                            && sim.db.tasks.get(&r.task_id).is_some_and(|t| {
                                t.state != elven_canopy_sim::task::TaskState::Complete
                            })
                    });
                if has_cook_task { "Cooking..." } else { "Idle" }
            }
        } else {
            ""
        };
        dict.set("cook_status", GString::from(cook_status));

        // Workshop data (for Workshop buildings).
        dict.set("workshop_enabled", structure.workshop_enabled);
        let mut recipe_ids_arr = VarArray::new();
        for rid in &structure.workshop_recipe_ids {
            recipe_ids_arr.push(&GString::from(rid.as_str()).to_variant());
        }
        dict.set("workshop_recipe_ids", recipe_ids_arr);
        let craft_status = if structure.furnishing
            == Some(elven_canopy_sim::types::FurnishingType::Workshop)
            && structure.workshop_enabled
        {
            let has_craft_task =
                sim.db
                    .task_structure_refs
                    .by_structure_id(&sid, elven_canopy_sim::tabulosity::QueryOpts::ASC)
                    .iter()
                    .any(|r| {
                        r.role == elven_canopy_sim::db::TaskStructureRole::CraftAt
                            && sim.db.tasks.get(&r.task_id).is_some_and(|t| {
                                t.state != elven_canopy_sim::task::TaskState::Complete
                            })
                    });
            if has_craft_task {
                "Crafting..."
            } else {
                "Idle"
            }
        } else {
            ""
        };
        dict.set("craft_status", GString::from(craft_status));

        dict
    }

    /// Rename a completed structure. Empty string resets to auto-generated default.
    #[func]
    fn rename_structure(&mut self, structure_id: i64, name: GString) {
        let name_str = name.to_string();
        let name_opt = if name_str.is_empty() {
            None
        } else {
            Some(name_str)
        };
        self.apply_or_send(SimAction::RenameStructure {
            structure_id: StructureId(structure_id as u64),
            name: name_opt,
        });
    }

    /// Return all elves as a `VarArray` of dictionaries for the elf picker UI.
    ///
    /// Each dictionary contains: `creature_id` (UUID string), `name`, `name_meaning`,
    /// `rest`, `rest_max`, `index` (species-filtered iteration order), `assigned_home`
    /// (structure ID as i64, or -1 if unassigned).
    #[func]
    fn get_all_elves(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let rest_max = sim.species_table[&Species::Elf].rest_max;
        let mut arr = VarArray::new();
        for (index, creature) in sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elf)
            .enumerate()
        {
            let mut dict = VarDictionary::new();
            dict.set("creature_id", GString::from(&creature.id.0.to_string()));
            dict.set("name", GString::from(creature.name.as_str()));
            dict.set(
                "name_meaning",
                GString::from(creature.name_meaning.as_str()),
            );
            dict.set("rest", creature.rest);
            dict.set("rest_max", rest_max);
            dict.set("index", index as i64);
            let assigned_home = match creature.assigned_home {
                Some(sid) => sid.0 as i64,
                None => -1,
            };
            dict.set("assigned_home", assigned_home);
            arr.push(&dict.to_variant());
        }
        arr
    }

    /// Assign an elf to a home structure, or unassign if `structure_id` is -1.
    ///
    /// `creature_id_str` is the elf's UUID string. The command validates that
    /// the creature is an Elf and the target is a Home-furnished building.
    #[func]
    fn assign_home(&mut self, creature_id_str: GString, structure_id: i64) {
        let uuid_str = creature_id_str.to_string();
        let Some(uuid) = SimUuid::from_str(&uuid_str) else {
            return;
        };
        let sid = if structure_id < 0 {
            None
        } else {
            Some(StructureId(structure_id as u64))
        };
        self.apply_or_send(SimAction::AssignHome {
            creature_id: CreatureId(uuid),
            structure_id: sid,
        });
    }

    /// Set the logistics priority for a building. Pass priority < 0 to disable.
    #[func]
    fn set_logistics_priority(&mut self, structure_id: i64, priority: i32) {
        let p = if priority < 0 {
            None
        } else {
            Some(priority as u8)
        };
        self.apply_or_send(SimAction::SetLogisticsPriority {
            structure_id: StructureId(structure_id as u64),
            priority: p,
        });
    }

    /// Set the logistics wants for a building. Expects a JSON string like:
    /// `[{"kind": "Bread", "quantity": 10}, {"kind": "Fruit", "quantity": 5}]`
    #[func]
    fn set_logistics_wants(&mut self, structure_id: i64, wants_json: GString) {
        let json_str = wants_json.to_string();
        let parsed: Vec<serde_json::Value> = match serde_json::from_str(&json_str) {
            Ok(v) => v,
            Err(e) => {
                godot_error!("SimBridge: failed to parse logistics wants JSON: {e}");
                return;
            }
        };
        let mut wants = Vec::new();
        for entry in &parsed {
            let kind_str = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let kind = match kind_str {
                "Bread" => elven_canopy_sim::inventory::ItemKind::Bread,
                "Fruit" => elven_canopy_sim::inventory::ItemKind::Fruit,
                "Bow" => elven_canopy_sim::inventory::ItemKind::Bow,
                "Arrow" => elven_canopy_sim::inventory::ItemKind::Arrow,
                "Bowstring" => elven_canopy_sim::inventory::ItemKind::Bowstring,
                other => {
                    godot_error!("SimBridge: unknown item kind in logistics wants: '{other}'");
                    continue;
                }
            };
            let quantity = entry.get("quantity").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if quantity > 0 {
                wants.push(elven_canopy_sim::building::LogisticsWant {
                    item_kind: kind,
                    target_quantity: quantity,
                });
            }
        }
        self.apply_or_send(SimAction::SetLogisticsWants {
            structure_id: StructureId(structure_id as u64),
            wants,
        });
    }

    /// Set the cooking configuration for a kitchen building.
    #[func]
    fn set_cooking_config(&mut self, structure_id: i64, cooking_enabled: bool, bread_target: i32) {
        self.apply_or_send(SimAction::SetCookingConfig {
            structure_id: StructureId(structure_id as u64),
            cooking_enabled,
            cooking_bread_target: bread_target.max(0) as u32,
        });
    }

    /// Set the workshop configuration for a workshop building.
    #[func]
    fn set_workshop_config(
        &mut self,
        structure_id: i64,
        enabled: bool,
        recipe_ids: PackedStringArray,
    ) {
        let ids: Vec<String> = recipe_ids
            .as_slice()
            .iter()
            .map(|s| s.to_string())
            .collect();
        self.apply_or_send(SimAction::SetWorkshopConfig {
            structure_id: StructureId(structure_id as u64),
            workshop_enabled: enabled,
            recipe_ids: ids,
        });
    }

    /// Get all available recipes as an array of dictionaries.
    #[func]
    fn get_recipes(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let mut arr = VarArray::new();
        for recipe in &sim.config.recipes {
            let mut d = VarDictionary::new();
            d.set("id", GString::from(&recipe.id));
            d.set("display_name", GString::from(&recipe.display_name));
            d.set("work_ticks", recipe.work_ticks as i64);
            let mut inputs = VarArray::new();
            for input in &recipe.inputs {
                let mut inp = VarDictionary::new();
                inp.set("item_kind", GString::from(input.item_kind.display_name()));
                inp.set("quantity", input.quantity as i64);
                inputs.push(&inp.to_variant());
            }
            d.set("inputs", inputs);
            let mut outputs = VarArray::new();
            for output in &recipe.outputs {
                let mut out = VarDictionary::new();
                out.set("item_kind", GString::from(output.item_kind.display_name()));
                out.set("quantity", output.quantity as i64);
                outputs.push(&out.to_variant());
            }
            d.set("outputs", outputs);
            arr.push(&d.to_variant());
        }
        arr
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
        let Some(sim) = &self.session.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for creature in sim.db.creatures.iter_all().filter(|c| c.species == species) {
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
        self.session
            .sim
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
        let Some(sim) = &self.session.sim else {
            return false;
        };
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
        let Some(sim) = &self.session.sim else {
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
        let Some(sim) = &self.session.sim else {
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
        let Some(sim) = &self.session.sim else {
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
        let json_str = json.to_string();
        let events = self
            .session
            .process(SessionMessage::LoadSim { json: json_str });
        let loaded = events
            .iter()
            .any(|e| matches!(e, elven_canopy_sim::session::SessionEvent::SimLoaded));
        if loaded {
            if let Some(sim) = &self.session.sim {
                godot_print!(
                    "SimBridge: loaded save (tick={}, creatures={})",
                    sim.tick,
                    sim.db.creatures.len()
                );
                let seconds_per_tick = sim.config.tick_duration_ms as f64 / 1000.0;
                self.local_relay = Some(LocalRelay::new(seconds_per_tick));
            }
            self.rebuild_mesh_cache();
            true
        } else {
            for e in &events {
                if let elven_canopy_sim::session::SessionEvent::Error { message } = e {
                    godot_error!("SimBridge: failed to load save: {message}");
                }
            }
            false
        }
    }

    /// Spawn a capybara at the given voxel position.
    /// Legacy wrapper — delegates to `spawn_creature("Capybara", ...)`.
    #[func]
    fn spawn_capybara(&mut self, x: i32, y: i32, z: i32) {
        self.spawn_creature(GString::from("Capybara"), x, y, z);
    }

    /// Return all ground piles as a `VarArray` of dictionaries.
    ///
    /// Each dictionary contains: `x`, `y`, `z` (pile position) and
    /// `inventory` (a `VarArray` of `{kind, quantity}` dicts). Same
    /// inventory format as creature info. Useful for future rendering
    /// and debugging.
    #[func]
    fn get_ground_piles(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let mut result = VarArray::new();
        for pile in sim.db.ground_piles.iter_all() {
            let mut dict = VarDictionary::new();
            dict.set("x", pile.position.x);
            dict.set("y", pile.position.y);
            dict.set("z", pile.position.z);

            let mut inv_arr = VarArray::new();
            for stack in sim.inv_items(pile.inventory_id) {
                let mut item_dict = VarDictionary::new();
                item_dict.set("kind", GString::from(stack.kind.display_name()));
                item_dict.set("quantity", stack.quantity as i64);
                inv_arr.push(&item_dict.to_variant());
            }
            dict.set("inventory", inv_arr);

            result.push(&dict.to_variant());
        }
        result
    }

    /// Return info for a single ground pile at position (x, y, z).
    ///
    /// Returns a dictionary with `x`, `y`, `z`, and `inventory` (same
    /// format as `get_ground_piles()` entries), or an empty dictionary
    /// if no pile exists at that position. Used by `main.gd` for the
    /// pile info panel display and per-frame refresh.
    #[func]
    fn get_ground_pile_info(&self, x: i32, y: i32, z: i32) -> VarDictionary {
        let Some(sim) = &self.session.sim else {
            return VarDictionary::new();
        };
        let coord = VoxelCoord::new(x, y, z);
        let Some(pile) = sim
            .db
            .ground_piles
            .by_position(&coord, elven_canopy_sim::tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
        else {
            return VarDictionary::new();
        };

        let mut dict = VarDictionary::new();
        dict.set("x", pile.position.x);
        dict.set("y", pile.position.y);
        dict.set("z", pile.position.z);

        let mut inv_arr = VarArray::new();
        for stack in sim.inv_items(pile.inventory_id) {
            let mut item_dict = VarDictionary::new();
            item_dict.set("kind", GString::from(stack.kind.display_name()));
            item_dict.set("quantity", stack.quantity as i64);
            inv_arr.push(&item_dict.to_variant());
        }
        dict.set("inventory", inv_arr);
        dict
    }

    /// Return all notifications with ID greater than `after_id`.
    ///
    /// Returns a `VarArray` of `VarDictionary`, each with `id` (i64),
    /// `tick` (i64), and `message` (String). Used by `main.gd` to poll
    /// for new notifications and push them to the toast display.
    #[func]
    fn get_notifications_after(&self, after_id: i64) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let mut result = VarArray::new();
        for notif in sim.db.notifications.iter_all() {
            if (notif.id.0 as i64) <= after_id {
                continue;
            }
            let mut dict = VarDictionary::new();
            dict.set("id", notif.id.0 as i64);
            dict.set("tick", notif.tick as i64);
            dict.set("message", GString::from(&notif.message));
            result.push(&dict.to_variant());
        }
        result
    }

    /// Return the highest notification ID currently in the sim database.
    ///
    /// Used by `main.gd` after loading a save to initialize
    /// `_last_notification_id` so that historical notifications are not
    /// replayed as toasts.
    #[func]
    fn get_max_notification_id(&self) -> i64 {
        let Some(sim) = &self.session.sim else {
            return 0;
        };
        sim.db
            .notifications
            .iter_all()
            .map(|n| n.id.0 as i64)
            .max()
            .unwrap_or(0)
    }

    /// Send a debug notification through the full command pipeline.
    ///
    /// The notification goes through `apply_or_send()` so it's
    /// multiplayer-aware — in MP it's broadcast and applied canonically.
    #[func]
    fn send_debug_notification(&mut self, message: GString) {
        self.apply_or_send(SimAction::DebugNotification {
            message: message.to_string(),
        });
    }

    /// Check whether a single voxel is a valid build position.
    ///
    /// A position is valid if it is in-bounds, Air, and has at least one
    /// face-adjacent solid voxel. Used by the construction ghost mesh to
    /// show blue (valid) vs red (invalid) preview color for single-voxel
    /// placement.
    #[func]
    fn validate_build_position(&self, x: i32, y: i32, z: i32) -> bool {
        let Some(sim) = &self.session.sim else {
            return false;
        };
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
        let Some(sim) = &self.session.sim else {
            return false;
        };
        let coord = VoxelCoord::new(x, y, z);
        sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air
    }

    /// Check whether a single voxel has at least one face-adjacent solid
    /// voxel. Used alongside `validate_build_air` for multi-voxel rectangle
    /// validation.
    #[func]
    fn has_solid_neighbor(&self, x: i32, y: i32, z: i32) -> bool {
        let Some(sim) = &self.session.sim else {
            return false;
        };
        sim.world.has_solid_face_neighbor(VoxelCoord::new(x, y, z))
    }

    /// Designate a single-voxel platform blueprint at the given position.
    ///
    /// Buffers the command via `apply_build_action` (executes on the next
    /// `frame_update`). Always returns empty — validation feedback is
    /// provided by the `validate_*_preview()` methods before placement.
    #[func]
    fn designate_build(&mut self, x: i32, y: i32, z: i32) -> GString {
        self.apply_build_action(SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![VoxelCoord::new(x, y, z)],
            priority: Priority::Normal,
        })
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
        self.apply_build_action(SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels,
            priority: Priority::Normal,
        })
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
        self.apply_build_action(SimAction::DesignateBuilding {
            anchor: VoxelCoord::new(x, y, z),
            width,
            depth,
            height,
            priority: Priority::Normal,
        })
    }

    /// Designate a rectangular prism of voxels for carving (removal to Air).
    ///
    /// `x, y, z` is the min-corner. `width`, `depth`, `height` are dimensions
    /// in X, Z, Y respectively. Returns a validation message (empty = success).
    #[func]
    fn designate_carve(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        depth: i32,
        height: i32,
    ) -> GString {
        let w = width.max(1);
        let d = depth.max(1);
        let h = height.max(1);
        let mut voxels = Vec::with_capacity((w * d * h) as usize);
        for dy in 0..h {
            for dx in 0..w {
                for dz in 0..d {
                    voxels.push(VoxelCoord::new(x + dx, y + dy, z + dz));
                }
            }
        }
        self.apply_build_action(SimAction::DesignateCarve {
            voxels,
            priority: Priority::Normal,
        })
    }

    /// Preview-validate a rectangular carve placement.
    ///
    /// Counts carvable solid voxels (not Air, not ForestFloor) in the region.
    /// Returns a `VarDictionary` with `"tier"` and `"message"` keys.
    #[func]
    fn validate_carve_preview(
        &self,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        depth: i32,
        height: i32,
    ) -> VarDictionary {
        let Some(sim) = &self.session.sim else {
            return Self::preview_result("Blocked", "Simulation not initialized.");
        };
        let w = width.max(1);
        let d = depth.max(1);
        let h = height.max(1);

        // Bounds check.
        for dy in 0..h {
            for dx in 0..w {
                for dz in 0..d {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    if !sim.world.in_bounds(coord) {
                        return Self::preview_result("Blocked", "Carve position is out of bounds.");
                    }
                }
            }
        }

        // Count carvable voxels (solid and not ForestFloor).
        let mut carvable_count = 0;
        for dy in 0..h {
            for dx in 0..w {
                for dz in 0..d {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    let vt = sim.world.get(coord);
                    if vt.is_solid() && vt != VoxelType::ForestFloor {
                        carvable_count += 1;
                    }
                }
            }
        }

        if carvable_count == 0 {
            return Self::preview_result("Blocked", "Nothing to carve.");
        }

        // Collect carvable coords for structural validation.
        let mut carve_coords = Vec::new();
        for dy in 0..h {
            for dx in 0..w {
                for dz in 0..d {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    let vt = sim.world.get(coord);
                    if vt.is_solid() && vt != VoxelType::ForestFloor {
                        carve_coords.push(coord);
                    }
                }
            }
        }

        let validation =
            structural::validate_carve_fast(&sim.world, &sim.face_data, &carve_coords, &sim.config);
        Self::preview_result_from_tier(validation.tier, &validation.message)
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
        let Some(sim) = &self.session.sim else {
            return false;
        };
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
        let Some(sim) = &self.session.sim else {
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
        let Some(sim) = &self.session.sim else {
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
        let Some(sim) = &self.session.sim else {
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
    /// Skips Carve blueprints (those are rendered separately).
    /// Used by the blueprint renderer to show translucent ghost cubes for
    /// planned (not-yet-built) construction. Flat (x,y,z) triples.
    #[func]
    fn get_blueprint_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.session.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for bp in sim.db.blueprints.iter_all() {
            if bp.state == BlueprintState::Designated && bp.build_type != BuildType::Carve {
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

    /// Return voxels from `Designated` Carve blueprints that are still solid
    /// (not yet carved) as a flat PackedInt32Array of (x,y,z) triples.
    ///
    /// Used by the blueprint renderer to show translucent red-orange ghost
    /// cubes for planned carve operations.
    #[func]
    fn get_carve_blueprint_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.session.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for bp in sim.db.blueprints.iter_all() {
            if bp.state == BlueprintState::Designated && bp.build_type == BuildType::Carve {
                for v in &bp.voxels {
                    if sim.world.get(*v) != VoxelType::Air {
                        arr.push(v.x);
                        arr.push(v.y);
                        arr.push(v.z);
                    }
                }
            }
        }
        arr
    }

    // ========================================================================
    // Frame update
    // ========================================================================

    /// Unified per-frame entry point. Polls the network if in multiplayer,
    /// then advances the sim via the local relay in single-player. Returns
    /// the fractional render tick for smooth interpolation.
    ///
    /// In multiplayer mode, polls the network for turns, then interpolates
    /// `render_tick` up to `mp_ticks_per_turn` ahead of the last confirmed
    /// tick for smooth movement between turns. In single-player mode,
    /// delegates tick pacing to the `LocalRelay`.
    #[func]
    fn frame_update(&mut self, delta: f64) -> f64 {
        if self.net_client.is_some() {
            let turns = self.poll_network();
            if turns > 0 {
                self.mp_time_since_turn = 0.0;
            } else {
                self.mp_time_since_turn += delta;
            }
            let spt = self
                .session
                .sim
                .as_ref()
                .map(|s| s.config.tick_duration_ms as f64 / 1000.0)
                .unwrap_or(0.001);
            let ticks_ahead = (self.mp_time_since_turn / spt) as u64;
            let max_ticks = self.mp_ticks_per_turn as u64;
            let capped = ticks_ahead.min(max_ticks);
            return self.session.current_tick() as f64 + capped as f64;
        }
        if let Some(relay) = &mut self.local_relay {
            let mult = self.session.speed_multiplier();
            let tick = self.session.current_tick();
            if let Some(msg) = relay.update(delta, mult, tick) {
                self.session.process(msg);
            }
            return relay.render_tick(self.session.current_tick());
        }
        self.session.current_tick() as f64
    }

    // ========================================================================
    // Chunk mesh methods
    // ========================================================================

    /// Internal: rebuild the mesh cache from the current sim state.
    fn rebuild_mesh_cache(&mut self) {
        let Some(sim) = &self.session.sim else {
            return;
        };
        let mut cache = MeshCache::new();
        cache.build_all(&sim.world);
        self.mesh_cache = Some(cache);
    }

    /// Build the world mesh cache from scratch. Call once after init_sim or
    /// load_game_json. Replaces any existing cache.
    #[func]
    fn build_world_mesh(&mut self) {
        let Some(sim) = &self.session.sim else {
            return;
        };
        let mut cache = MeshCache::new();
        cache.build_all(&sim.world);
        godot_print!(
            "SimBridge: built world mesh ({} non-empty chunks)",
            cache.chunk_coords().len()
        );
        self.mesh_cache = Some(cache);
    }

    /// Drain dirty voxels from the world, mark affected chunks, and regenerate
    /// them. Returns the number of chunks updated (0 if nothing changed).
    ///
    /// NOTE: `drain_dirty_voxels()` mutates `sim.world` directly, bypassing
    /// the session message flow. This is intentional — the dirty-voxel buffer
    /// is render-only metadata (not serialized, not part of sim determinism).
    /// It's a cache-invalidation signal consumed by the mesh cache, not
    /// simulation state.
    #[func]
    fn update_world_mesh(&mut self) -> i32 {
        let Some(sim) = &mut self.session.sim else {
            return 0;
        };
        let Some(cache) = &mut self.mesh_cache else {
            return 0;
        };
        let dirty = sim.world.drain_dirty_voxels();
        if dirty.is_empty() {
            return 0;
        }
        cache.mark_dirty_voxels(&dirty);
        cache.update_dirty(&sim.world) as i32
    }

    /// Return all non-empty chunk coordinates as a flat PackedInt32Array of
    /// (cx, cy, cz) triples. Used by tree_renderer.gd to build initial
    /// MeshInstance3D nodes.
    #[func]
    fn get_mesh_chunk_coords(&self) -> PackedInt32Array {
        let Some(cache) = &self.mesh_cache else {
            return PackedInt32Array::new();
        };
        let coords = cache.chunk_coords();
        let mut arr = PackedInt32Array::new();
        for c in &coords {
            arr.push(c.cx);
            arr.push(c.cy);
            arr.push(c.cz);
        }
        arr
    }

    /// Return chunk coordinates that were updated in the last
    /// `update_world_mesh()` call, as a flat PackedInt32Array of (cx,cy,cz)
    /// triples.
    #[func]
    fn get_dirty_chunk_coords(&self) -> PackedInt32Array {
        let Some(cache) = &self.mesh_cache else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for c in cache.last_updated_coords() {
            arr.push(c.cx);
            arr.push(c.cy);
            arr.push(c.cz);
        }
        arr
    }

    /// Build a Godot ArrayMesh for the given chunk. Returns an ArrayMesh with
    /// exactly 3 surfaces: surface 0 = bark, surface 1 = ground, surface 2 =
    /// leaf. Empty surfaces get a minimal single-triangle degenerate surface
    /// so the surface index is always stable (bark=0, ground=1, leaf=2).
    /// Returns a default empty ArrayMesh if the chunk is not in the cache.
    #[func]
    fn build_chunk_array_mesh(&self, cx: i32, cy: i32, cz: i32) -> Gd<godot::classes::ArrayMesh> {
        use elven_canopy_sim::mesh_gen::ChunkCoord;

        let mut array_mesh = godot::classes::ArrayMesh::new_gd();

        let Some(cache) = &self.mesh_cache else {
            return array_mesh;
        };
        let coord = ChunkCoord::new(cx, cy, cz);
        let Some(chunk_mesh) = cache.get_chunk(&coord) else {
            return array_mesh;
        };

        // Always add all 3 surfaces in fixed order so material assignment
        // by surface index is reliable.
        Self::add_surface_or_placeholder(&mut array_mesh, &chunk_mesh.bark);
        Self::add_surface_or_placeholder(&mut array_mesh, &chunk_mesh.ground);
        Self::add_surface_or_placeholder(&mut array_mesh, &chunk_mesh.leaf);

        array_mesh
    }

    /// Add a surface to the ArrayMesh. If the surface is empty, adds a
    /// degenerate zero-area triangle as a placeholder so the surface index
    /// stays stable.
    fn add_surface_or_placeholder(
        mesh: &mut Gd<godot::classes::ArrayMesh>,
        surface: &elven_canopy_sim::mesh_gen::SurfaceMesh,
    ) {
        if surface.is_empty() {
            Self::add_placeholder_surface(mesh);
        } else {
            Self::add_surface_to_array_mesh(mesh, surface);
        }
    }

    /// Add a degenerate zero-area triangle surface as a placeholder.
    fn add_placeholder_surface(mesh: &mut Gd<godot::classes::ArrayMesh>) {
        use godot::classes::mesh::PrimitiveType;

        let origin = Vector3::ZERO;
        let mut vertices = PackedVector3Array::new();
        vertices.push(origin);
        vertices.push(origin);
        vertices.push(origin);

        let mut normals = PackedVector3Array::new();
        normals.push(Vector3::UP);
        normals.push(Vector3::UP);
        normals.push(Vector3::UP);

        let mut indices = PackedInt32Array::new();
        indices.push(0);
        indices.push(1);
        indices.push(2);

        let mut arrays = VarArray::new();
        arrays.resize(13, &Variant::nil());
        arrays.set(0, &Variant::from(vertices));
        arrays.set(1, &Variant::from(normals));
        arrays.set(12, &Variant::from(indices));

        mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
    }

    /// Helper: convert a `SurfaceMesh` into a Godot surface array and add it
    /// to the `ArrayMesh`. Skips empty surfaces.
    fn add_surface_to_array_mesh(
        mesh: &mut Gd<godot::classes::ArrayMesh>,
        surface: &elven_canopy_sim::mesh_gen::SurfaceMesh,
    ) {
        use godot::classes::mesh::PrimitiveType;

        if surface.is_empty() {
            return;
        }

        let vert_count = surface.vertex_count();

        // Build PackedVector3Array for vertices.
        let mut vertices = PackedVector3Array::new();
        for i in 0..vert_count {
            let base = i * 3;
            vertices.push(Vector3::new(
                surface.vertices[base],
                surface.vertices[base + 1],
                surface.vertices[base + 2],
            ));
        }

        // Build PackedVector3Array for normals.
        let mut normals = PackedVector3Array::new();
        for i in 0..vert_count {
            let base = i * 3;
            normals.push(Vector3::new(
                surface.normals[base],
                surface.normals[base + 1],
                surface.normals[base + 2],
            ));
        }

        // Build PackedColorArray for vertex colors.
        let mut colors = PackedColorArray::new();
        for i in 0..vert_count {
            let base = i * 4;
            colors.push(Color::from_rgba(
                surface.colors[base],
                surface.colors[base + 1],
                surface.colors[base + 2],
                surface.colors[base + 3],
            ));
        }

        // Build PackedVector2Array for UVs.
        let mut uvs = PackedVector2Array::new();
        for i in 0..vert_count {
            let base = i * 2;
            uvs.push(Vector2::new(surface.uvs[base], surface.uvs[base + 1]));
        }

        // Build PackedInt32Array for indices.
        let mut indices = PackedInt32Array::new();
        for &idx in &surface.indices {
            indices.push(idx as i32);
        }

        // Assemble the surface array. Godot expects a VarArray with
        // specific indices (ARRAY_VERTEX=0, ARRAY_NORMAL=1, ARRAY_TANGENT=2,
        // ARRAY_COLOR=3, ARRAY_TEX_UV=4, ..., ARRAY_INDEX=12).
        let mut arrays = VarArray::new();
        arrays.resize(13, &Variant::nil());
        arrays.set(0, &Variant::from(vertices)); // ARRAY_VERTEX
        arrays.set(1, &Variant::from(normals)); // ARRAY_NORMAL
        // 2: ARRAY_TANGENT — skip (nil)
        arrays.set(3, &Variant::from(colors)); // ARRAY_COLOR
        arrays.set(4, &Variant::from(uvs)); // ARRAY_TEX_UV
        // 5-11: skip (nil)
        arrays.set(12, &Variant::from(indices)); // ARRAY_INDEX

        mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
    }

    /// Get the texture atlas pixel data for a chunk surface. Returns a
    /// PackedByteArray of RGBA pixels (empty if no atlas for this surface).
    /// Surface indices: 0=bark, 1=ground.
    #[func]
    fn get_chunk_atlas_data(&self, cx: i32, cy: i32, cz: i32, surface: i32) -> PackedByteArray {
        use elven_canopy_sim::mesh_gen::ChunkCoord;

        let Some(cache) = &self.mesh_cache else {
            return PackedByteArray::new();
        };
        let coord = ChunkCoord::new(cx, cy, cz);
        let Some(chunk_mesh) = cache.get_chunk(&coord) else {
            return PackedByteArray::new();
        };

        let pixels = match surface {
            0 => &chunk_mesh.bark.atlas_pixels,
            1 => &chunk_mesh.ground.atlas_pixels,
            _ => return PackedByteArray::new(),
        };

        if pixels.is_empty() {
            return PackedByteArray::new();
        }

        let mut arr = PackedByteArray::new();
        arr.resize(pixels.len());
        let slice = arr.as_mut_slice();
        slice.copy_from_slice(pixels);
        arr
    }

    /// Get the atlas dimensions for a chunk surface. Returns Vector2i(width, height).
    /// Returns (0,0) if no atlas exists for this surface.
    #[func]
    fn get_chunk_atlas_size(&self, cx: i32, cy: i32, cz: i32, surface: i32) -> Vector2i {
        use elven_canopy_sim::mesh_gen::ChunkCoord;

        let Some(cache) = &self.mesh_cache else {
            return Vector2i::ZERO;
        };
        let coord = ChunkCoord::new(cx, cy, cz);
        let Some(chunk_mesh) = cache.get_chunk(&coord) else {
            return Vector2i::ZERO;
        };

        let (w, h) = match surface {
            0 => (chunk_mesh.bark.atlas_width, chunk_mesh.bark.atlas_height),
            1 => (
                chunk_mesh.ground.atlas_width,
                chunk_mesh.ground.atlas_height,
            ),
            _ => (0, 0),
        };

        Vector2i::new(w as i32, h as i32)
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
        let Some(sim) = &self.session.sim else {
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
        let Some(sim) = &self.session.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for bp in sim.db.blueprints.iter_all() {
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
        let Some(sim) = &self.session.sim else {
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
        self.apply_build_action(SimAction::DesignateLadder {
            anchor: VoxelCoord::new(x, y, z),
            height,
            orientation: face_dir,
            kind: ladder_kind,
            priority: Priority::Normal,
        })
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
                let pid = SessionPlayerId(info.player_id.0);
                self.local_player_id = pid;
                self.session = GameSession::new_multiplayer(pid, pid);
                self.mp_ticks_per_turn = info.ticks_per_turn;
                self.net_client = Some(client);
                self.relay_handle = Some(handle);
                self.is_multiplayer_mode = true;
                self.local_relay = None;
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
                let pid = SessionPlayerId(info.player_id.0);
                self.local_player_id = pid;
                // In join, host_id is unknown until we receive a GameStart;
                // set to 0 (relay assigns host).
                self.session = GameSession::new_multiplayer(pid, SessionPlayerId(0));
                self.mp_ticks_per_turn = info.ticks_per_turn;
                self.net_client = Some(client);
                self.is_multiplayer_mode = true;
                self.local_relay = None;
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
        self.local_player_id = SessionPlayerId::LOCAL;
        self.session = GameSession::new_singleplayer();
        self.local_relay = None;
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
        self.session.is_host(self.local_player_id)
    }

    /// Return true if the multiplayer game has started (past lobby).
    #[func]
    fn is_game_started(&self) -> bool {
        self.session.has_sim()
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
        if !self.session.is_host(self.local_player_id) {
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
        // Collect messages before processing (can't hold shared borrow of
        // net_client while mutating session).
        let messages: Vec<_> = client.poll();
        let mut turns_applied = 0;
        let pid = self.local_player_id;

        for msg in messages {
            match msg {
                ServerMessage::GameStart { seed, config_json } => {
                    let profile: TreeProfile = serde_json::from_str(&config_json)
                        .unwrap_or_else(|_| TreeProfile::fantasy_mega());
                    let config = GameConfig {
                        tree_profile: profile,
                        ..Default::default()
                    };
                    self.session.process(SessionMessage::StartGame {
                        seed: seed as u64,
                        config: Box::new(config),
                    });
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
                    // Route each command through session, then advance.
                    for tc in &commands {
                        if let Ok(action) = serde_json::from_slice::<SimAction>(&tc.payload) {
                            self.session
                                .process(SessionMessage::SimCommand { from: pid, action });
                        }
                    }
                    self.session.process(SessionMessage::AdvanceTo {
                        tick: sim_tick_target,
                    });
                    turns_applied += 1;
                }
                ServerMessage::PlayerJoined { player } => {
                    self.session.process(SessionMessage::PlayerJoined {
                        id: SessionPlayerId(player.id.0),
                        name: player.name.clone(),
                    });
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
                    self.session.process(SessionMessage::PlayerLeft {
                        id: SessionPlayerId(player_id.0),
                    });
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
                    self.session
                        .process(SessionMessage::DesyncDetected { tick });
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "desync",
                            "tick": tick,
                        }))
                        .unwrap_or_default(),
                    );
                }
                ServerMessage::Paused { by } => {
                    self.session.process(SessionMessage::Pause {
                        by: SessionPlayerId(by.0),
                    });
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "paused",
                            "by": by.0,
                        }))
                        .unwrap_or_default(),
                    );
                }
                ServerMessage::Resumed { by } => {
                    self.session.process(SessionMessage::Resume {
                        by: SessionPlayerId(by.0),
                    });
                    self.mp_events.push(
                        serde_json::to_string(&serde_json::json!({
                            "type": "resumed",
                            "by": by.0,
                        }))
                        .unwrap_or_default(),
                    );
                }
                ServerMessage::SnapshotRequest => {
                    if let Some(sim) = &self.session.sim
                        && let Ok(json) = sim.to_json()
                    {
                        let data = json.into_bytes();
                        if let Some(client) = &mut self.net_client {
                            let _ = client.send_snapshot_response(&data);
                        }
                    }
                }
                ServerMessage::SnapshotLoad { tick: _, data } => {
                    if let Ok(json) = String::from_utf8(data) {
                        self.session
                            .process(SessionMessage::LoadSim { json: json.clone() });
                        self.mp_events.push(
                            serde_json::to_string(&serde_json::json!({
                                "type": "snapshot_loaded",
                            }))
                            .unwrap_or_default(),
                        );
                    }
                }
                ServerMessage::SpeedChanged { ticks_per_turn } => {
                    self.mp_ticks_per_turn = ticks_per_turn;
                    self.session.process(SessionMessage::SetSpeed {
                        speed: ticks_per_turn_to_speed(ticks_per_turn),
                    });
                }
                _ => {}
            }
        }

        // After applying turns, check if the sim tick is on a checksum boundary.
        if turns_applied > 0
            && let Some(sim) = &self.session.sim
        {
            let tick = sim.tick;
            if tick > 0 && tick % CHECKSUM_INTERVAL_TICKS == 0 {
                let hash = sim.state_checksum();
                if let Some(client) = &mut self.net_client {
                    let _ = client.send_checksum(tick, hash);
                }
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

    /// Begin furnishing a completed building. `furnishing_type` is a string
    /// matching one of the `FurnishingType` variants ("Dormitory", "Home",
    /// "DiningHall", "Kitchen", "Workshop", "Storehouse", "ConcertHall").
    /// Ignored if the structure is not a building or already furnished.
    #[func]
    fn furnish_structure(&mut self, structure_id: i64, furnishing_type: GString) {
        let ft = match furnishing_type.to_string().as_str() {
            "ConcertHall" => FurnishingType::ConcertHall,
            "DiningHall" => FurnishingType::DiningHall,
            "Dormitory" => FurnishingType::Dormitory,
            "Home" => FurnishingType::Home,
            "Kitchen" => FurnishingType::Kitchen,
            "Storehouse" => FurnishingType::Storehouse,
            "Workshop" => FurnishingType::Workshop,
            _ => return,
        };
        self.apply_or_send(SimAction::FurnishStructure {
            structure_id: StructureId(structure_id as u64),
            furnishing_type: ft,
        });
    }

    /// Return all placed furniture positions across all structures as a flat
    /// PackedInt32Array of (x, y, z, kind) quads. The `kind` value is the
    /// `FurnitureKind` discriminant (0=Bed, 1=Bench, etc.) for rendering
    /// dispatch. Used by furniture_renderer.gd.
    #[func]
    fn get_furniture_positions(&self) -> PackedInt32Array {
        let Some(sim) = &self.session.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for structure in sim.db.structures.iter_all() {
            let kind = match &structure.furnishing {
                Some(ft) => ft.furniture_kind() as i32,
                None => FurnitureKind::Bed as i32,
            };
            for furn in sim
                .db
                .furniture
                .by_structure_id(&structure.id, elven_canopy_sim::tabulosity::QueryOpts::ASC)
            {
                if furn.placed {
                    arr.push(furn.coord.x);
                    arr.push(furn.coord.y);
                    arr.push(furn.coord.z);
                    arr.push(kind);
                }
            }
        }
        arr
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
