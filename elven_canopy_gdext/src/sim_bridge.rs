// GDExtension bridge class for the simulation.
//
// Exposes a `SimBridge` node that Godot scenes can use to create, step, and
// query the simulation. This is the sole interface between GDScript and the
// Rust sim — all sim interaction goes through methods on this class.
//
// All sim access goes through a `GameSession` (`session.rs`). The session
// owns the `Option<SimState>` and processes all mutations via typed
// `SessionMessage`s. Both singleplayer and multiplayer use a real relay on
// localhost — the relay flushes `Turn` messages at a fixed cadence, and
// `poll_network()` processes them to advance the sim.
//
// ## What it exposes
//
// - **Lifecycle:** `init_sim(seed)`, `init_sim_with_tree_profile_json(seed, json)`,
//   `init_sim_test_config(seed)` (small 64³ world for GUT tests),
//   `current_tick()`, `is_initialized()`, `tick_duration_ms()`,
//   `step_exactly(n)` (pause-safe deterministic tick stepping for tests).
// - **Frame update:** `frame_update(delta)` — unified per-frame entry point.
//   Polls the relay for Turn messages and returns a fractional render_tick
//   for smooth creature interpolation.
// - **Speed control:** `get_sim_speed()` returns the current speed as a string,
//   `sim_speed_multiplier()` returns the time multiplier for tick pacing,
//   `set_sim_speed(speed_name)` applies pause/resume/speed to the session.
//   In multiplayer, sends to the relay first and applies locally only on
//   success (optimistic update).
// - **Save/load:** `save_game_json()` returns the sim state as a JSON string,
//   `load_game_json(json)` replaces the current sim from a JSON string.
//   File I/O is handled in GDScript via Godot's `user://` paths.
// - **World data / chunk mesh:** `build_world_mesh()` scans the world and
//   populates the MegaChunk spatial index (no meshes generated yet).
//   `update_world_mesh()` incrementally regenerates dirty visible chunks.
//   `update_visibility(cam_x,cam_y,cam_z,frustum)` performs draw-distance and
//   frustum culling, generates meshes on demand, and produces delta lists:
//   `get_chunks_to_show/hide()`, `get_chunks_generated()`, `get_chunks_evicted()`.
//   `set_draw_distance()` / `set_mesh_memory_budget()` configure culling/LRU.
//   `build_chunk_array_mesh(cx,cy,cz)` returns a Godot `ArrayMesh` for one chunk.
//   `get_fruit_voxels()` — flat `PackedInt32Array` of (x,y,z,species_id)
//   quads for fruit billboard sprite rendering (fruit is not part of chunk mesh).
// - **Creature positions:** `get_creature_positions(species_name, render_tick)`
//   — `PackedVector3Array` for billboard sprite placement (used by renderers).
//   `get_creature_positions_with_ids(species_name, render_tick)` — four
//   parallel arrays: `ids` (GString UUIDs), `positions` (PackedVector3Array),
//   `is_player_civ` (bools), and `military_group_ids` (i64, -1 for civilians)
//   for selection hit-testing, box-select, and double-click group select.
//   Used by `selection_controller.gd` and `tooltip_controller.gd`.
// - **Projectile data:** `get_projectile_positions(render_tick)` returns
//   interpolated positions (PackedVector3Array), `get_projectile_velocities()`
//   returns velocity vectors for orienting arrow meshes along flight direction.
// - **Notifications:** `get_notifications_after(after_id)` polls for new
//   notifications (returns `VarArray` of dicts with id/tick/message),
//   `get_max_notification_id()` returns the highest ID (for initializing the
//   cursor after load), `send_debug_notification(message)` sends a test
//   notification through the full command pipeline (multiplayer-aware).
// - **Creature info:** `get_creature_info_by_id(creature_id, render_tick)` —
//   returns a `VarDictionary` with species, species_index, interpolated
//   position (x/y/z), task status, task_kind, food level, food_max, rest
//   level, rest_max, name, name_meaning, inventory, thoughts, mood. Primary
//   API for creature info — uses direct CreatureId lookup.
//   `get_creature_info(species_name, index, render_tick)` — legacy API with
//   same dict format but fragile species+index addressing.
// - **Creature summary:** `get_all_creatures_summary()` — returns a `VarArray`
//   of `VarDictionary`, one per creature, sorted (elves first by name, then
//   other species by species+index). Each dict: creature_id, species, index,
//   name, name_meaning, has_task, task_kind. Used by `units_panel.gd`.
// - **Task list:** `get_active_tasks()` — returns a `VarArray` of
//   `VarDictionary`, one per non-complete task. Each dict includes short/full
//   ID, kind, state, origin (PlayerDirected/Autonomous/Automated),
//   progress/total_cost, location coordinates, and an assignees array with
//   creature_id, species, and name. Used by `task_panel.gd`.
// - **Placement:** `snap_placement_to_ray(origin, dir, ground_only,
//   large)` — casts `raycast_solid` along the mouse ray to find solid
//   geometry, then snaps to the nearest walkable position.
// - **Commands:** `spawn_creature(species_name, x,y,z)` — generic creature
//   spawner. Also `create_goto_task(x,y,z)`, `designate_build(x,y,z)`,
//   `designate_build_rect(x,y,z,width,depth)`, etc. Commands are sent to
//   the relay via `apply_or_send()` and applied when the Turn comes back.
//   In test mode (no relay), they are applied directly to the session.
//   Build/carve validation is done upfront by the
//   `validate_*_preview()` query methods — the designation commands
//   themselves are fire-and-forget.
//   `furnish_structure(structure_id, furnishing_type)` begins furnishing a
//   completed building. `get_furniture_positions()` returns flat (x,y,z,kind)
//   quads of placed furniture for rendering.
// - **Selection groups:** `set_selection_group(n, creature_uuids, structure_ids)`
//   and `add_to_selection_group(n, ...)` send commands to persist groups in the
//   sim. `get_all_selection_groups()` returns all groups for the local player
//   (used to hydrate GDScript's local cache after load).
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
//   a structure's custom name. Unified crafting commands:
//   `get_recipe_catalog_for_building(id)`, `set_crafting_enabled(id, enabled)`,
//   `add_active_recipe(id, key_json)`, `remove_active_recipe(ar_id)`,
//   `set_recipe_output_target(target_id, qty)`, `set_recipe_auto_logistics(...)`,
//   `set_recipe_enabled(ar_id, enabled)`, `move_active_recipe_up/down(ar_id)`.
//   `set_logistics_wants(id, wants_json)` — set building logistics wants
//   (each want is an `{item_kind, material_filter, quantity}` triple).
//   `get_logistics_item_kinds()` and `get_logistics_material_options(kind)`
//   — dynamic UI picker data for the two-step logistics want flow.
// - **Ground piles:** `get_ground_piles()` — returns a `VarArray` of
//   `{x, y, z, inventory: [{kind, quantity}]}` dicts.
//   `get_ground_pile_info(x,y,z)` — returns a single pile's dict (same
//   format) or empty dict if no pile at that position. Used by the pile
//   info panel for display and per-frame refresh.
// - **Species queries:** `is_species_ground_only(species_name)` — used by
//   the placement controller to decide which nav nodes to show.
//   `get_all_creature_positions_with_relations()` — all alive creatures with
//     positions and player-relation classification (for minimap).
//   `get_creature_player_relation()` — single-creature player relation query.
// - **Placement raycasting:** `raycast_solid(origin, dir)` — DDA raycast
//   returning the first solid voxel and entry face (actual world only).
//   `raycast_solid_with_blueprints(origin, dir)` — same but blueprint-aware
//   (designated blueprints are treated as their target types).
//   `get_voxel_solidity_slice(y, cx, cz, radius)` — solid/air grid for
//   height-slice wireframe rendering (actual world only).
//   `get_voxel_solidity_slice_with_blueprints(y, cx, cz, radius)` — same
//   but blueprint-aware. `auto_ladder_orientation(x,y,z,h)` — picks the
//   best facing for a ladder column. `get_world_size()` — world dimensions
//   for clamping.
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

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use elven_canopy_protocol::message::ServerMessage;
use elven_canopy_relay::server::{RelayConfig, RelayHandle, start_relay};
use elven_canopy_sim::blueprint::BlueprintState;
use elven_canopy_sim::command::SimAction;
use elven_canopy_sim::config::{GameConfig, TreeProfile};
use elven_canopy_sim::session::{GameSession, SessionMessage, SessionPlayerId, SessionSpeed};
use elven_canopy_sim::structural::{self, ValidationTier};
use elven_canopy_sim::task::{TaskOrigin, TaskState};
use elven_canopy_sim::types::{
    ActiveRecipeId, ActiveRecipeTargetId, BuildType, CreatureId, DiplomaticRelation, FaceDirection,
    FruitSpeciesId, FurnishingType, FurnitureKind, ItemStackId, LadderKind, OpinionKind,
    OverlapClassification, Priority, SimUuid, Species, StructureId, TraitKind, VitalStatus,
    VoxelCoord, VoxelType, ZoneId,
};
use godot::classes::ImageTexture;
use godot::prelude::*;

use elven_canopy_protocol::types::SessionId;
use elven_canopy_relay::client::{NetClient, RelayConnection};

use crate::mesh_cache::MeshCache;
use crate::sprite_bridge::pixel_buffer_to_texture;

/// Wire format for an LLM request payload (sim → relay → inference peer).
/// Serialized as JSON into the opaque `Vec<u8>` in `ClientMessage::LlmRequest`.
#[derive(serde::Serialize, serde::Deserialize)]
struct LlmRequestPayload {
    creature_id: String,
    preambles: Vec<elven_canopy_sim::llm::PreambleSection>,
    prompt: String,
    response_schema: String,
    deadline_tick: u64,
    max_tokens: u32,
}

/// Wire format for an LLM response payload (inference peer → relay → all clients).
/// Serialized as JSON into the opaque `Vec<u8>` in `LlmResult.payload`.
/// `pub(crate)` so `llm_worker.rs` can serialize the same type.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct LlmResponsePayload {
    pub(crate) result_json: String,
    pub(crate) metadata: elven_canopy_sim::llm::InferenceMetadata,
}

/// Compile-time version hash. Bump when making breaking protocol changes.
const SIM_VERSION_HASH: u64 = 1;

/// Global elfcyclopedia server. Lives in a static so it survives SimBridge
/// being freed (e.g., when returning to the main menu) and only starts once.
static ELFCYCLOPEDIA: std::sync::Mutex<Option<crate::elfcyclopedia_server::ElfcyclopediaServer>> =
    std::sync::Mutex::new(None);

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
        "Hornet" => Some(Species::Hornet),
        "Wyvern" => Some(Species::Wyvern),
        _ => None,
    }
}

/// Parse a UUID string into a `CreatureId`.
fn parse_creature_id(uuid_str: &str) -> Option<CreatureId> {
    SimUuid::from_str(uuid_str).map(CreatureId)
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
        Species::Hornet => "Hornet",
        Species::Wyvern => "Wyvern",
    }
}

/// Build a `VarDictionary` with full creature info for the GDScript info panel.
///
/// Shared by `get_creature_info()` (legacy species+index lookup) and
/// `get_creature_info_by_id()` (stable CreatureId lookup). Both resolve
/// the creature reference, then delegate here for dict construction.
fn build_creature_info_dict(
    sim: &elven_canopy_sim::sim::SimState,
    c: &elven_canopy_sim::db::Creature,
    render_tick: f64,
) -> VarDictionary {
    let species = c.species;
    // Compute the species-filtered index for sprite seed consistency
    // with renderers and units_panel.
    let species_index = sim
        .db
        .creatures
        .iter_all()
        .filter(|cr| cr.species == species && cr.vital_status != VitalStatus::Dead)
        .position(|cr| cr.id == c.id)
        .unwrap_or(0);
    let ma = sim.db.move_actions.get(&c.id);
    let (x, y, z): (f32, f32, f32) = c.interpolated_position(render_tick, ma.as_ref());
    let mut dict = VarDictionary::new();
    dict.set("id", GString::from(c.id.0.to_string().as_str()));
    dict.set("species", GString::from(species_name(species)));
    dict.set("species_index", species_index as i32);
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
    if let Some(tid) = &c.current_task
        && let Some(task) = sim.db.tasks.get(tid)
    {
        dict.set("task_location_x", task.location.x);
        dict.set("task_location_y", task.location.y);
        dict.set("task_location_z", task.location.z);
    }
    dict.set("hp", c.hp);
    dict.set("hp_max", c.hp_max);
    dict.set("mp", c.mp);
    dict.set("mp_max", c.mp_max);
    dict.set("food", c.food);
    let food_max = sim.species_table[&species].food_max;
    dict.set("food_max", food_max);
    dict.set("rest", c.rest);
    let rest_max = sim.species_table[&species].rest_max;
    dict.set("rest_max", rest_max);
    dict.set("name", GString::from(c.name.as_str()));
    dict.set("name_meaning", GString::from(c.name_meaning.as_str()));
    dict.set("sex_symbol", GString::from(c.sex.symbol()));
    let assigned_home = match c.assigned_home {
        Some(sid) => sid.0 as i64,
        None => -1,
    };
    dict.set("assigned_home", assigned_home);
    let mut thoughts_arr = VarArray::new();
    let creature_thoughts = sim
        .db
        .thoughts
        .by_creature_id(&c.id, elven_canopy_sim::tabulosity::QueryOpts::ASC);
    for thought in creature_thoughts.iter().rev() {
        let mut td = VarDictionary::new();
        td.set("text", GString::from(thought.kind.description().as_str()));
        td.set("tick", thought.tick as i64);
        thoughts_arr.push(&td.to_variant());
    }
    dict.set("thoughts", thoughts_arr);
    let mood_score: i32 = creature_thoughts
        .iter()
        .map(|t| sim.config.mood.mood_weight(&t.kind))
        .sum();
    let mood_tier = sim.config.mood.tier(mood_score);
    dict.set("mood_score", mood_score);
    dict.set("mood_tier", GString::from(mood_tier.label()));
    let mut inv_arr = VarArray::new();
    for stack in sim.inv_items(c.inventory_id) {
        let mut item_dict = VarDictionary::new();
        item_dict.set("item_stack_id", stack.id.0 as i64);
        item_dict.set(
            "kind",
            GString::from(sim.item_display_name(&stack).as_str()),
        );
        item_dict.set("quantity", stack.quantity as i64);
        if let Some(slot) = stack.equipped_slot {
            item_dict.set("equipped_slot", GString::from(slot.display_name()));
        }
        inv_arr.push(&item_dict.to_variant());
    }
    dict.set("inventory", inv_arr);
    dict.set(
        "incapacitated",
        c.vital_status == VitalStatus::Incapacitated,
    );
    // Military group info (civ creatures only).
    if let Some(civ_id) = c.civ_id {
        let (group_id, group_name) = if let Some(gid) = c.military_group {
            let name = sim
                .db
                .military_groups
                .get(&gid)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            (gid.0 as i64, name)
        } else {
            // Implicit civilian — look up the default civilian group.
            let civ_groups = sim
                .db
                .military_groups
                .by_civ_id(&civ_id, elven_canopy_sim::tabulosity::QueryOpts::ASC);
            match civ_groups.iter().find(|g| g.is_default_civilian) {
                Some(cg) => (cg.id.0 as i64, cg.name.clone()),
                None => (-1, String::new()),
            }
        };
        dict.set("military_group_id", group_id);
        dict.set("military_group_name", GString::from(group_name.as_str()));
    }
    // Path info (F-path-core).
    if let Some(path_id) = sim.creature_path(c.id) {
        dict.set("path_id", GString::from(format!("{path_id:?}").as_str()));
        dict.set("path_name", GString::from(path_id.display_name()));
    } else {
        dict.set("path_id", GString::from(""));
        dict.set("path_name", GString::from(""));
    }

    // Taming info (F-taming). Check if the player's civ has designated this
    // creature, not just any civ (B-tame-civ-id).
    let is_tame_designated = sim
        .player_civ_id
        .is_some_and(|civ| sim.db.tame_designations.get(&(c.id, civ)).is_some());
    dict.set("tame_designated", is_tame_designated);
    let is_tameable = sim
        .config
        .species
        .get(&c.species)
        .and_then(|sd| sd.tame_difficulty)
        .is_some();
    dict.set("is_tameable", is_tameable);
    dict.set("is_wild", c.civ_id.is_none());

    // Creature stats (ability scores).
    for tk in elven_canopy_sim::stats::STAT_TRAIT_KINDS {
        let val = sim
            .db
            .creature_traits
            .get(&(c.id, tk))
            .map(|t| t.value.as_int(0))
            .unwrap_or(0);
        let key = match tk {
            TraitKind::Strength => "stat_str",
            TraitKind::Agility => "stat_agi",
            TraitKind::Dexterity => "stat_dex",
            TraitKind::Constitution => "stat_con",
            TraitKind::Willpower => "stat_wil",
            TraitKind::Intelligence => "stat_int",
            TraitKind::Perception => "stat_per",
            TraitKind::Charisma => "stat_cha",
            _ => continue,
        };
        dict.set(key, val);
    }
    // Big Five personality axes (F-genetics Phase C).
    for tk in elven_canopy_sim::genome::PERSONALITY_TRAIT_KINDS {
        let val = sim
            .db
            .creature_traits
            .get(&(c.id, tk))
            .map(|t| t.value.as_int(0))
            .unwrap_or(0);
        let key = match tk {
            TraitKind::Openness => "personality_o",
            TraitKind::Conscientiousness => "personality_c",
            TraitKind::Extraversion => "personality_e",
            TraitKind::Agreeableness => "personality_a",
            TraitKind::Neuroticism => "personality_n",
            _ => continue,
        };
        dict.set(key, val);
    }
    // Creature skills (F-creature-skills).
    for tk in elven_canopy_sim::stats::SKILL_TRAIT_KINDS {
        let val = sim
            .db
            .creature_traits
            .get(&(c.id, tk))
            .map(|t| t.value.as_int(0))
            .unwrap_or(0);
        let key = match tk {
            TraitKind::Striking => "skill_striking",
            TraitKind::Archery => "skill_archery",
            TraitKind::Evasion => "skill_evasion",
            TraitKind::Ranging => "skill_ranging",
            TraitKind::Herbalism => "skill_herbalism",
            TraitKind::Beastcraft => "skill_beastcraft",
            TraitKind::Cuisine => "skill_cuisine",
            TraitKind::Tailoring => "skill_tailoring",
            TraitKind::Woodcraft => "skill_woodcraft",
            TraitKind::Alchemy => "skill_alchemy",
            TraitKind::Singing => "skill_singing",
            TraitKind::Channeling => "skill_channeling",
            TraitKind::Literature => "skill_literature",
            TraitKind::Art => "skill_art",
            TraitKind::Influence => "skill_influence",
            TraitKind::Culture => "skill_culture",
            TraitKind::Counsel => "skill_counsel",
            _ => continue,
        };
        dict.set(key, val);
    }

    // Social opinions (F-social-opinions).
    let opinions = sim
        .db
        .creature_opinions
        .by_creature_id(&c.id, elven_canopy_sim::tabulosity::QueryOpts::ASC);
    let mut opinion_arr = godot::prelude::VarArray::new();
    for op in &opinions {
        let mut od = VarDictionary::new();
        od.set("target_id", op.target_id.to_string().to_godot());
        let target_name = sim
            .db
            .creatures
            .get(&op.target_id)
            .map(|tc| tc.name.clone())
            .unwrap_or_default();
        od.set("target_name", target_name.to_godot());
        let kind_str = match op.kind {
            OpinionKind::Friendliness => "Friendliness",
            OpinionKind::Respect => "Respect",
            OpinionKind::Fear => "Fear",
            OpinionKind::Attraction => "Attraction",
        };
        od.set("kind", kind_str.to_godot());
        od.set("intensity", op.intensity);
        // For Friendliness, include the category label (Friend, Acquaintance, etc.)
        // so GDScript doesn't need to duplicate the threshold logic.
        if op.kind == OpinionKind::Friendliness {
            od.set(
                "label",
                sim.friendship_category(op.intensity).label().to_godot(),
            );
        }
        opinion_arr.push(&od.to_variant());
    }
    dict.set("social_opinions", opinion_arr.to_variant());

    dict
}

/// Parameters for `start_local_relay_and_connect()`. Bundles session config
/// to stay under the clippy argument limit.
struct LocalRelayOpts<'a> {
    port: u16,
    session_name: &'a str,
    player_name: &'a str,
    password: Option<String>,
    max_players: u32,
    ticks_per_turn: u32,
    turn_cadence_ms: u64,
}

/// Base ticks-per-turn for singleplayer relay sessions at Normal (1x) speed.
/// Smaller than MP's 50 to reduce per-turn batch size and smooth out frame
/// workload. Speed changes multiply this: Fast=20, VeryFast=50.
const SP_BASE_TICKS_PER_TURN: u32 = 10;

/// Speed multiplier as an integer: Normal=1, Fast=2, VeryFast=5.
/// Used to compute `ticks_per_turn = base_ticks_per_turn * multiplier`.
fn speed_multiplier_int(speed: SessionSpeed) -> u32 {
    match speed {
        SessionSpeed::Normal => 1,
        SessionSpeed::Fast => 2,
        SessionSpeed::VeryFast => 5,
    }
}

/// Godot node that owns and drives the simulation.
///
/// Add this as a child node in your main scene. Call `init_sim()` from
/// GDScript to create the simulation, then `frame_update(delta)` each
/// frame — it polls the relay for Turn messages and returns a fractional
/// render_tick for smooth interpolation.
/// Note: desync-detection checksums are currently disabled (too slow —
/// see B-fast-checksum tracker item).
#[derive(GodotClass)]
#[class(base=Node)]
pub struct SimBridge {
    base: Base<Node>,
    session: GameSession,
    local_player_id: SessionPlayerId,
    // Chunk mesh cache — not part of SimState, lives here for rendering.
    mesh_cache: Option<MeshCache>,
    // Multiplayer state
    net_client: Option<NetClient>,
    relay_handle: Option<RelayHandle>,
    is_multiplayer_mode: bool,
    active_zone_id: ZoneId,
    mp_events: Vec<String>,
    /// Current ticks_per_turn from the relay (updated on SpeedChanged).
    mp_ticks_per_turn: u32,
    /// Base ticks_per_turn at 1x speed, set at session creation. Speed
    /// changes multiply this: Normal=1x, Fast=2x, VeryFast=5x.
    base_ticks_per_turn: u32,
    mp_time_since_turn: f64,
    /// Background music composition results, keyed by CompositionId.
    /// Each entry is None while generating, Some(pcm_data) when ready.
    pending_compositions: BTreeMap<u64, Arc<Mutex<Option<Vec<f32>>>>>,
    /// Unified sprite cache keyed by creature identity. Stores normal and
    /// fallen (90° CW rotated) textures alongside the `SpriteKey` that
    /// produced them. All species use the same key structure (`SpriteParams`
    /// from biological traits + equipment). Invalidates per-creature when the
    /// key changes.
    creature_sprite_cache: HashMap<CreatureId, CachedCreatureSprite>,
    /// LLM inference worker thread. Created lazily on first `load_llm_model`
    /// call to avoid spawning a thread for temporary SimBridge instances
    /// (e.g., the one created in `game_session.gd` just for elfcyclopedia).
    llm_worker: Option<crate::llm_worker::LlmWorker>,
    /// Whether this client has an LLM model loaded and can run inference.
    /// Tracks load/unload requests sent to the worker thread. Used to set
    /// `llm_capable` in the Hello handshake and send `LlmCapabilityChanged`
    /// when the state changes mid-session.
    llm_model_loaded: bool,
    /// When true, log LLM prompts, raw responses, latency, and token counts.
    /// Toggled via `set_llm_debug` from GDScript (driven by the `llm_debug`
    /// game config setting).
    llm_debug: bool,
}

/// Cache key for sprite invalidation — captures everything that affects a
/// creature's visual appearance. Species-agnostic: all creatures use the
/// same struct with `SpriteParams` (from biological traits) plus equipment.
#[derive(Clone, Debug, PartialEq)]
struct SpriteKey {
    params: elven_canopy_sprites::SpriteParams,
    equipment: [Option<elven_canopy_sprites::EquipSlotDrawInfo>;
        elven_canopy_sim::inventory::EquipSlot::COUNT],
}

/// A cached creature sprite: the key that produced it, plus normal and fallen
/// (incapacitated) textures.
struct CachedCreatureSprite {
    key: SpriteKey,
    normal: Gd<ImageTexture>,
    fallen: Gd<ImageTexture>,
}

#[godot_api]
impl INode for SimBridge {
    fn init(base: Base<Node>) -> Self {
        // Start the global elfcyclopedia server if not already running.
        Self::ensure_elfcyclopedia_started();

        let mut session = GameSession::new_singleplayer();
        session.set_wg_log(Box::new(|msg| godot_print!("{msg}")));
        Self {
            base,
            session,
            local_player_id: SessionPlayerId::LOCAL,
            mesh_cache: None,
            net_client: None,
            relay_handle: None,
            is_multiplayer_mode: false,
            active_zone_id: ZoneId(1), // placeholder until sim is created
            mp_events: Vec::new(),
            mp_ticks_per_turn: 50,
            base_ticks_per_turn: 50,
            mp_time_since_turn: 0.0,
            pending_compositions: BTreeMap::new(),
            creature_sprite_cache: HashMap::new(),
            llm_worker: None,
            llm_model_loaded: false,
            llm_debug: false,
        }
    }
}

#[godot_api]
impl SimBridge {
    /// Explicitly tear down all Rust state before Godot's shutdown sequence.
    ///
    /// Call this from GDScript before `get_tree().quit()` or on
    /// `NOTIFICATION_WM_CLOSE_REQUEST` to avoid segfaults caused by Godot
    /// freeing objects while Rust destructors still reference them.
    /// Stops the elfcyclopedia HTTP server, joins multiplayer threads, and
    /// drops sim state while the engine is still intact.
    #[func]
    fn shutdown(&mut self) {
        // Stop the elfcyclopedia HTTP server thread.
        let mut guard = ELFCYCLOPEDIA.lock().unwrap();
        if let Some(mut server) = guard.take() {
            server.stop();
        }
        drop(guard);

        // Gracefully disconnect and shut down the relay.
        self.shutdown_relay();

        // Shut down the LLM inference worker thread (if it was started).
        if let Some(worker) = &mut self.llm_worker {
            worker.shutdown();
        }

        // Print accumulated perf stats before tearing down.
        if let Some(cache) = &self.mesh_cache {
            cache.perf.print_summary();
        }

        // Clear sim state.
        let mut session = GameSession::new_singleplayer();
        session.set_wg_log(Box::new(|msg| godot_print!("{msg}")));
        self.session = session;
        self.mesh_cache = None;
        self.pending_compositions.clear();
        // Drop Godot-object references (Gd<ImageTexture>) while the engine is
        // still intact. Without this, they survive until Godot frees the
        // SimBridge node during engine shutdown, which can crash on Windows.
        self.creature_sprite_cache.clear();

        godot_print!("SimBridge: shutdown complete");
    }

    /// Load an LLM model on the background inference thread. Called from
    /// GDScript when `ModelManager` detects the model file is available
    /// (either on startup or after download completes). The model path is
    /// an absolute filesystem path to a GGUF file. `use_gpu` offloads all
    /// model layers to GPU when true. Spawns the worker thread lazily on
    /// first call.
    #[func]
    fn load_llm_model(&mut self, path: GString, use_gpu: bool) {
        let path_str = path.to_string();
        let mode = if use_gpu { "GPU" } else { "CPU" };
        godot_print!("SimBridge: requesting LLM model load ({mode}): {path_str}");
        let worker = self
            .llm_worker
            .get_or_insert_with(crate::llm_worker::LlmWorker::new);
        worker.send(crate::llm_worker::LlmWorkerCmd::LoadModel {
            path: path_str,
            use_gpu,
        });
        if !self.llm_model_loaded {
            self.llm_model_loaded = true;
            self.send_llm_capability_changed(true);
        }
    }

    /// Unload the LLM model, freeing memory. Called from GDScript when the
    /// user deletes the model file via settings.
    #[func]
    fn unload_llm_model(&mut self) {
        if let Some(worker) = &self.llm_worker {
            godot_print!("SimBridge: requesting LLM model unload");
            worker.send(crate::llm_worker::LlmWorkerCmd::UnloadModel);
        }
        if self.llm_model_loaded {
            self.llm_model_loaded = false;
            self.send_llm_capability_changed(false);
        }
    }

    /// Enable or disable debug logging for LLM prompts, responses, and
    /// performance metrics. Called from GDScript when the `llm_debug` setting
    /// changes.
    #[func]
    fn set_llm_debug(&mut self, enabled: bool) {
        self.llm_debug = enabled;
        if enabled {
            godot_print!("SimBridge: LLM debug logging enabled");
        }
    }

    /// Notify the relay that this client's LLM capability has changed.
    /// No-op if not connected to a relay.
    fn send_llm_capability_changed(&mut self, llm_capable: bool) {
        if let Some(client) = &mut self.net_client
            && let Err(e) = client.send_llm_capability_changed(llm_capable)
        {
            godot_error!("SimBridge: failed to send LlmCapabilityChanged: {e}");
        }
    }

    /// Set the local player's display name. Must be called before init_sim()
    /// so the session's PlayerSlot has the correct name when the sim starts.
    /// In single-player, updates the LOCAL slot. In multiplayer, the name is
    /// set via the relay handshake instead.
    #[func]
    fn set_player_name(&mut self, name: GString) {
        let name_str = name.to_string();
        if let Some(slot) = self.session.players.get_mut(&self.local_player_id) {
            slot.name = name_str;
        }
    }

    /// Initialize the simulation with the given seed and default config.
    /// Starts a real relay on localhost and connects via TCP — singleplayer
    /// uses the same code path as multiplayer hosting.
    #[func]
    fn init_sim(&mut self, seed: i64) {
        self.init_sim_via_relay(seed, "{}");
    }

    /// Initialize the simulation with the given seed and a custom tree profile.
    ///
    /// The `tree_profile_json` parameter is a JSON string matching the
    /// `TreeProfile` serde schema (see `config.rs`). If parsing fails, falls
    /// back to the default Fantasy Mega profile.
    #[func]
    fn init_sim_with_tree_profile_json(&mut self, seed: i64, tree_profile_json: GString) {
        self.init_sim_via_relay(seed, &tree_profile_json.to_string());
    }

    /// Shared implementation for `init_sim` and `init_sim_with_tree_profile_json`.
    /// Starts an embedded relay on localhost, sends `StartGame`, and spin-polls
    /// until the sim is created.
    fn init_sim_via_relay(&mut self, seed: i64, config_json: &str) {
        // Clean up any existing relay (defensive — handles double-init).
        self.shutdown_relay();

        let ticks_per_turn = SP_BASE_TICKS_PER_TURN;
        let tick_duration_ms = GameConfig::default().tick_duration_ms as u64;
        let turn_cadence_ms = u64::from(ticks_per_turn) * tick_duration_ms;

        // Get player name from existing session (set by set_player_name before
        // init_sim is called).
        let player_name = self
            .session
            .players
            .get(&self.local_player_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Player".to_string());

        if let Err(e) = self.start_local_relay_and_connect(LocalRelayOpts {
            port: 0, // OS-assigned
            session_name: "singleplayer",
            player_name: &player_name,
            password: None,
            max_players: 1,
            ticks_per_turn,
            turn_cadence_ms,
        }) {
            godot_error!("SimBridge: {e}");
            return;
        }

        // Restore player name on the new multiplayer session.
        if let Some(slot) = self.session.players.get_mut(&self.local_player_id) {
            slot.name = player_name;
        }

        // Send StartGame through the relay.
        if let Some(client) = &mut self.net_client
            && let Err(e) = client.send_start_game(seed, config_json, None)
        {
            godot_error!("SimBridge: send_start_game failed: {e}");
            return;
        }

        // Spin-poll until the GameStart message comes back and creates the sim.
        self.wait_for_sim_ready();
        if self.session.has_sim() {
            if let Some(sim) = &self.session.sim {
                self.active_zone_id = sim.home_zone_id();
            }
            self.rebuild_mesh_cache();
            self.creature_sprite_cache.clear();
            godot_print!("SimBridge: simulation initialized with seed {seed}");
        }
    }

    /// Spin-poll the relay until the sim is created (GameStart processed).
    /// Blocks for up to 5 seconds; logs an error and cleans up on timeout.
    fn wait_for_sim_ready(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !self.session.has_sim() {
            if std::time::Instant::now() >= deadline {
                godot_error!("SimBridge: timed out waiting for sim to be created");
                self.shutdown_relay();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            self.poll_network();
        }
    }

    /// Initialize the simulation with a small, fast-validating config for tests.
    ///
    /// Uses a 64x64x64 world with reduced tree energy (50.0), flat terrain,
    /// and fewer initial creatures. Mirrors the `test_config()` used by Rust
    /// unit tests. The default `fantasy_mega` tree profile is too large to
    /// reliably pass structural validation within the retry budget, so tests
    /// should use this instead of `init_sim`.
    #[func]
    fn init_sim_test_config(&mut self, seed: i64) {
        let mut config = GameConfig {
            world_size: (64, 64, 64),
            floor_y: 0,
            ..GameConfig::default()
        };
        config.tree_profile.growth.initial_energy = 50.0;
        config.terrain_max_height = 0;
        // Adjust spawn positions for the small test world (center=32, floor_y=0).
        for spec in &mut config.initial_creatures {
            spec.spawn_position = VoxelCoord::new(32, 1, 32);
        }
        for pile in &mut config.initial_ground_piles {
            pile.position = VoxelCoord::new(32, 1, 42);
        }
        self.session.process(SessionMessage::StartGame {
            seed: seed as u64,
            config: Box::new(config),
        });
        if let Some(sim) = &self.session.sim {
            self.active_zone_id = sim.home_zone_id();
        }
        self.rebuild_mesh_cache();
        godot_print!("SimBridge: test sim initialized with seed {seed} (small world)");
    }

    /// Advance the simulation to the target tick, processing all events.
    #[func]
    fn step_to_tick(&mut self, target_tick: i64) {
        self.session.process(SessionMessage::AdvanceTo {
            tick: target_tick as u64,
        });
    }

    /// Advance the simulation by exactly `n` ticks from the current tick.
    ///
    /// `n` must be non-negative (negative values are clamped to 0 with a
    /// warning). Calling with `n = 0` is a no-op.
    ///
    /// Works even when the session is paused: temporarily resumes, steps,
    /// then re-pauses. This is the intended API for test time control — the
    /// sim should be paused (speed set to "Paused") to prevent `frame_update`
    /// from also advancing ticks via the relay.
    #[func]
    fn step_exactly(&mut self, n: i64) {
        if n <= 0 {
            if n < 0 {
                godot_warn!("step_exactly called with negative n={n}, ignoring");
            }
            return;
        }
        let was_paused = self.session.is_paused();
        if was_paused {
            self.session.process(SessionMessage::Resume {
                by: self.local_player_id,
            });
        }
        let target = self.session.current_tick() as i64 + n;
        self.step_to_tick(target);
        if was_paused {
            self.session.process(SessionMessage::Pause {
                by: self.local_player_id,
            });
        }
    }

    /// Return the current simulation tick.
    #[func]
    fn current_tick(&self) -> i64 {
        self.session.current_tick() as i64
    }

    /// Return the mana stored in the player's home tree.
    #[func]
    fn home_tree_mana(&self) -> f64 {
        self.session.sim.as_ref().map_or(0.0, |s| {
            s.db.great_tree_infos
                .get(&s.player_tree_id)
                .map_or(0.0, |info| info.mana_stored as f64)
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

    /// Set the simulation speed by name. When connected to a relay (normal
    /// mode), sends pause/resume/set_speed to the relay first, then applies
    /// locally as an optimistic prediction. When no relay is connected (test
    /// mode), applies directly to the session.
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

        // Send to relay first if connected. Only apply locally if send succeeds.
        if let Some(client) = &mut self.net_client {
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
                    let multiplier = speed_multiplier_int(speed);
                    let tpt = self.base_ticks_per_turn * multiplier;
                    if let Err(e) = client.send_set_speed(tpt) {
                        godot_error!("SimBridge: send_set_speed failed: {e}");
                        return;
                    }
                }
            }
        }

        // Apply to session (optimistic update after successful relay send,
        // or sole authority in test mode with no relay).
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

    /// Return fruit voxel positions with species IDs as a flat
    /// PackedInt32Array (x, y, z, species_id quads). Each fruit voxel
    /// includes its species ID so the renderer can look up the correct
    /// sprite texture. Skips voxels that have been carved to Air.
    #[func]
    fn get_fruit_voxels(&self) -> PackedInt32Array {
        let Some(sim) = &self.session.sim else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for tf in sim.db.tree_fruits.iter_all() {
            // Skip voxels carved to Air so the renderer doesn't draw them.
            if sim
                .voxel_zone(self.active_zone_id)
                .unwrap()
                .get(tf.position.min)
                == VoxelType::Air
            {
                continue;
            }
            arr.push(tf.position.min.x);
            arr.push(tf.position.min.y);
            arr.push(tf.position.min.z);
            arr.push(tf.species_id.0 as i32);
        }
        arr
    }

    /// Return appearance data for all fruit species in the world.
    ///
    /// Returns an Array of VarDictionary, one per species. Each dict has:
    /// - "id": int (FruitSpeciesId)
    /// - "shape": String ("Round", "Oblong", "Clustered", "Pod", "Nut", "Gourd")
    /// - "color_r": float (0.0-1.0)
    /// - "color_g": float (0.0-1.0)
    /// - "color_b": float (0.0-1.0)
    /// - "size_percent": int
    /// - "glows": bool
    /// - "name": String (Vaelith name + english gloss)
    #[func]
    fn get_fruit_species_appearances(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let mut arr = VarArray::new();
        for species in sim.db.fruit_species.iter_all() {
            let mut dict = VarDictionary::new();
            dict.set("id", species.id.0 as i32);
            let shape_str = match species.appearance.shape {
                elven_canopy_sim::fruit::FruitShape::Round => "Round",
                elven_canopy_sim::fruit::FruitShape::Oblong => "Oblong",
                elven_canopy_sim::fruit::FruitShape::Clustered => "Clustered",
                elven_canopy_sim::fruit::FruitShape::Pod => "Pod",
                elven_canopy_sim::fruit::FruitShape::Nut => "Nut",
                elven_canopy_sim::fruit::FruitShape::Gourd => "Gourd",
            };
            dict.set("shape", shape_str);
            dict.set(
                "color_r",
                species.appearance.exterior_color.r as f64 / 255.0,
            );
            dict.set(
                "color_g",
                species.appearance.exterior_color.g as f64 / 255.0,
            );
            dict.set(
                "color_b",
                species.appearance.exterior_color.b as f64 / 255.0,
            );
            dict.set("size_percent", species.appearance.size_percent as i32);
            dict.set("glows", species.appearance.glows);
            let name = format!("{} ({})", species.vaelith_name, species.english_gloss);
            dict.set("name", GString::from(name.as_str()));
            arr.push(&dict.to_variant());
        }
        arr
    }

    /// Return the name of the fruit species at a voxel position, or empty
    /// string if no fruit species is tracked there. Returns the Vaelith name
    /// with the English gloss in parentheses: "Thúni Réva (red berry)".
    #[func]
    fn get_fruit_species_name(&self, x: i32, y: i32, z: i32) -> GString {
        let Some(sim) = &self.session.sim else {
            return GString::new();
        };
        let pos = VoxelCoord::new(x, y, z);
        match sim.fruit_species_at(pos) {
            Some(species) => {
                let s = format!("{} ({})", species.vaelith_name, species.english_gloss);
                GString::from(s.as_str())
            }
            None => GString::new(),
        }
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
        let Some(tree) = sim.db.trees.get(&sim.player_tree_id) else {
            return VarDictionary::new();
        };
        let info = sim.db.great_tree_infos.get(&sim.player_tree_id);

        let mut dict = VarDictionary::new();
        dict.set("health", tree.health as i32);
        dict.set("growth_level", tree.growth_level as i32);
        dict.set("mana_stored", info.map_or(0.0, |i| i.mana_stored as f64));
        dict.set(
            "mana_capacity",
            info.map_or(0.0, |i| i.mana_capacity as f64),
        );
        dict.set(
            "fruit_count",
            sim.db
                .tree_fruits
                .count_by_tree_id(&tree.id, elven_canopy_sim::tabulosity::QueryOpts::ASC)
                as i32,
        );
        dict.set(
            "fruit_production_rate",
            info.map_or(0.0, |i| i.fruit_production_rate_ppm as f64 / 1_000_000.0),
        );
        dict.set(
            "carrying_capacity",
            info.map_or(0, |i| i.carrying_capacity as i32),
        );
        dict.set("current_load", info.map_or(0, |i| i.current_load as i32));

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
            s.db.tree_fruits.count_by_tree_id(
                &s.player_tree_id,
                elven_canopy_sim::tabulosity::QueryOpts::ASC,
            ) as i32
        })
    }

    /// Return the number of elves. Legacy wrapper — delegates to `creature_count_by_name`.
    #[func]
    fn elf_count(&self) -> i32 {
        self.creature_count_by_name(GString::from("Elf"))
    }

    /// Route a build/carve action through the relay (normal mode) or directly
    /// to the session (test mode with no relay).
    /// Returns empty — validation feedback comes from the
    /// `validate_*_preview()` methods that GDScript calls before confirming
    /// placement.
    fn apply_build_action(&mut self, action: SimAction) -> GString {
        self.apply_or_send(action);
        GString::new()
    }

    /// Send a SimAction to the relay (normal mode) or apply directly to the
    /// session (test mode with no relay). In relay mode, the command is
    /// serialized and sent over TCP; the relay batches it into a Turn and
    /// sends it back, where `poll_network()` applies it to the session.
    fn apply_or_send(&mut self, action: SimAction) {
        if let Some(client) = &mut self.net_client {
            if let Ok(json) = serde_json::to_vec(&action)
                && let Err(e) = client.send_command(&json)
            {
                godot_error!("SimBridge: send_command failed: {e}");
            }
        } else {
            // Test mode (init_sim_test_config + step_to_tick) — no relay.
            self.session.process(SessionMessage::SimCommand {
                from: self.local_player_id,
                action,
            });
        }
    }

    /// Spawn a creature of the named species at the given voxel position.
    ///
    /// Species name must match a `Species` enum variant ("Elf", "Capybara",
    /// "Boar", "Deer", "Monkey", "Squirrel", etc.). Unknown names are
    /// silently ignored.
    #[func]
    fn spawn_creature(&mut self, species_name: GString, x: i32, y: i32, z: i32) {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::SpawnCreature {
            zone_id: self.active_zone_id,
            species,
            position: VoxelCoord::new(x, y, z),
        });
    }

    /// Test helper: disable food and rest decay for all species.
    /// Prevents elf starvation during long test runs.
    #[func]
    fn debug_disable_needs(&mut self) {
        let Some(sim) = &mut self.session.sim else {
            return;
        };
        for species_data in sim.config.species.values_mut() {
            species_data.food_decay_per_tick = 0;
            species_data.rest_decay_per_tick = 0;
        }
        // Also update the cached species table.
        sim.species_table = sim.config.species.clone();
    }

    /// Test helper: add items directly to a structure's inventory.
    /// `material_json` is a JSON-serialized Material (e.g., `"Oak"` or
    /// `{"FruitSpecies":0}`), or empty for no material.
    #[func]
    fn debug_add_item_to_structure(
        &mut self,
        structure_id: i64,
        item_kind_name: GString,
        quantity: i32,
        material_json: GString,
    ) {
        let Some(sim) = &mut self.session.sim else {
            return;
        };
        let sid = StructureId(structure_id as u64);
        let Some(structure) = sim.db.structures.get(&sid) else {
            return;
        };
        let inv_id = structure.inventory_id;
        let item_kind: elven_canopy_sim::inventory::ItemKind =
            match serde_json::from_str(&format!("\"{item_kind_name}\"")) {
                Ok(k) => k,
                Err(_) => return,
            };
        let mat_str = material_json.to_string();
        let material: Option<elven_canopy_sim::inventory::Material> = if mat_str.is_empty() {
            None
        } else {
            serde_json::from_str(&mat_str).ok()
        };
        sim.inv_add_simple_item(inv_id, item_kind, quantity as u32, None, None);
        // If material is specified, update the most recent stack.
        if let Some(mat) = material {
            let stacks = sim
                .db
                .item_stacks
                .by_inventory_id(&inv_id, elven_canopy_sim::tabulosity::QueryOpts::ASC);
            if let Some(stack) = stacks
                .iter()
                .rev()
                .find(|s| s.kind == item_kind && s.material.is_none())
            {
                let stack_id = stack.id;
                let _ = sim.db.item_stacks.modify_unchecked(&stack_id, |s| {
                    s.material = Some(mat);
                });
            }
        }
    }

    /// Spawn an elf at the given voxel position.
    /// Legacy wrapper — delegates to `spawn_creature("Elf", ...)`.
    #[func]
    fn spawn_elf(&mut self, x: i32, y: i32, z: i32) {
        self.spawn_creature(GString::from("Elf"), x, y, z);
    }

    /// Return the number of capybaras. Legacy wrapper — delegates to `creature_count_by_name`.
    #[func]
    fn capybara_count(&self) -> i32 {
        self.creature_count_by_name(GString::from("Capybara"))
    }

    /// Snap the mouse ray to the nearest walkable position for placement.
    ///
    /// Casts `raycast_solid` along the ray to find where it hits geometry,
    /// computes the air voxel on the entry face, then snaps to the closest
    /// walkable position. Returns `{hit: true, position: Vector3}` or
    /// `{hit: false}`.
    ///
    /// `ground_only`: restrict to ground (Dirt) positions (for ground-only species).
    /// `large`: use the 2x2x2 footprint walkability check.
    #[func]
    fn snap_placement_to_ray(
        &self,
        origin: Vector3,
        dir: Vector3,
        ground_only: bool,
        large: bool,
    ) -> VarDictionary {
        let mut dict = VarDictionary::new();
        let Some(sim) = &self.session.sim else {
            dict.set("hit", false);
            return dict;
        };
        let from = [origin.x, origin.y, origin.z];
        let d = [dir.x, dir.y, dir.z];
        let y_cutoff = self.mesh_cache.as_ref().and_then(|c| c.y_cutoff());

        // Cast ray to find the first solid voxel hit.
        let Some((solid_coord, face)) = sim.raycast_solid(from, d, 500, None, y_cutoff) else {
            dict.set("hit", false);
            return dict;
        };

        // The air voxel the ray was in before hitting solid — offset by
        // the entry face direction.
        let offset = match face {
            0 => (1, 0, 0),  // PosX face
            1 => (-1, 0, 0), // NegX face
            2 => (0, 1, 0),  // PosY face
            3 => (0, -1, 0), // NegY face
            4 => (0, 0, 1),  // PosZ face
            5 => (0, 0, -1), // NegZ face
            _ => (0, 0, 0),
        };
        let air_pos = VoxelCoord::new(
            solid_coord.x + offset.0,
            solid_coord.y + offset.1,
            solid_coord.z + offset.2,
        );

        let footprint: [u8; 3] = if large { [2, 2, 2] } else { [1, 1, 1] };
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        let nearest = if ground_only {
            elven_canopy_sim::walkability::find_nearest_ground_walkable(
                zone,
                &zone.face_data,
                air_pos,
                5,
                footprint,
            )
        } else {
            elven_canopy_sim::walkability::find_nearest_walkable(
                zone,
                &zone.face_data,
                air_pos,
                5,
                footprint,
                true, // non-ground-only creatures can climb
            )
        };

        match nearest {
            Some(p) => {
                dict.set("hit", true);
                dict.set("position", Vector3::new(p.x as f32, p.y as f32, p.z as f32));
            }
            None => {
                dict.set("hit", false);
            }
        }
        dict
    }

    /// Create a GoTo task at the given voxel position (snapped to nearest nav node).
    /// Only an idle elf will claim it and walk to that location.
    #[func]
    fn create_goto_task(&mut self, x: i32, y: i32, z: i32) {
        self.apply_or_send(SimAction::CreateTask {
            zone_id: self.active_zone_id,
            kind: elven_canopy_sim::task::TaskKind::GoTo,
            position: VoxelCoord::new(x, y, z),
            required_species: Some(Species::Elf),
        });
    }

    /// Start a debug Dance activity in an existing dance hall. The sim picks
    /// the first available dance hall, creates the activity linked to it, and
    /// idle elves will discover, volunteer, assemble, and dance.
    #[func]
    fn start_debug_dance(&mut self) {
        self.apply_or_send(SimAction::StartDebugDance);
    }

    /// Send a specific creature to a location. Creates a GoTo task and
    /// immediately assigns it, preempting lower-priority tasks.
    #[func]
    fn directed_goto(&mut self, creature_uuid: GString, x: i32, y: i32, z: i32, queue: bool) {
        let Some(creature_id) = parse_creature_id(&creature_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::DirectedGoTo {
            zone_id: self.active_zone_id,
            creature_id,
            position: VoxelCoord::new(x, y, z),
            queue,
        });
    }

    /// Order a creature to attack-move to a destination. The creature walks
    /// toward the destination, engaging any hostiles detected en route.
    /// Creates an AttackMove task with PlayerCombat preemption.
    #[func]
    fn attack_move(&mut self, creature_uuid: GString, x: i32, y: i32, z: i32, queue: bool) {
        let Some(creature_id) = parse_creature_id(&creature_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::AttackMove {
            zone_id: self.active_zone_id,
            creature_id,
            destination: VoxelCoord::new(x, y, z),
            queue,
        });
    }

    /// Group move: spread multiple creatures across nearby nav nodes around
    /// the destination. `creature_uuids` is an untyped GDScript `Array` of
    /// UUID strings (GDScript arrays are untyped by default, so we must accept
    /// `VarArray` and convert each element).
    #[func]
    fn group_directed_goto(
        &mut self,
        creature_uuids: VarArray,
        x: i32,
        y: i32,
        z: i32,
        queue: bool,
    ) {
        let creature_ids: Vec<CreatureId> = creature_uuids
            .iter_shared()
            .filter_map(|v| parse_creature_id(&v.to_string()))
            .collect();
        if creature_ids.is_empty() {
            return;
        }
        self.apply_or_send(SimAction::GroupGoTo {
            zone_id: self.active_zone_id,
            creature_ids,
            position: VoxelCoord::new(x, y, z),
            queue,
        });
    }

    /// Group attack-move: spread multiple creatures across nearby nav nodes
    /// around the destination. `creature_uuids` is an untyped GDScript `Array`
    /// of UUID strings.
    #[func]
    fn group_attack_move(&mut self, creature_uuids: VarArray, x: i32, y: i32, z: i32, queue: bool) {
        let creature_ids: Vec<CreatureId> = creature_uuids
            .iter_shared()
            .filter_map(|v| parse_creature_id(&v.to_string()))
            .collect();
        if creature_ids.is_empty() {
            return;
        }
        self.apply_or_send(SimAction::GroupAttackMove {
            zone_id: self.active_zone_id,
            creature_ids,
            destination: VoxelCoord::new(x, y, z),
            queue,
        });
    }

    /// Order a creature to attack a target creature. Creates an AttackTarget
    /// task with PlayerCombat preemption.
    #[func]
    fn attack_creature(&mut self, attacker_uuid: GString, target_uuid: GString, queue: bool) {
        let Some(attacker_id) = parse_creature_id(&attacker_uuid.to_string()) else {
            return;
        };
        let Some(target_id) = parse_creature_id(&target_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::AttackCreature {
            attacker_id,
            target_id,
            queue,
        });
    }

    /// Return the UUID string of a creature at the given species-filtered index.
    /// Returns an empty string if species is unknown or index is out of bounds.
    #[func]
    fn get_creature_uuid(&self, species_name: GString, index: i32) -> GString {
        let Some(sim) = &self.session.sim else {
            return GString::new();
        };
        let Some(species) = parse_species(&species_name.to_string()) else {
            return GString::new();
        };
        let creature = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == species && c.vital_status != VitalStatus::Dead)
            .nth(index as usize);
        match creature {
            Some(c) => GString::from(c.id.0.to_string().as_str()),
            None => GString::new(),
        }
    }

    /// Check if two creatures (identified by UUID strings) are hostile to each
    /// other. Returns true if the subject would consider the object hostile.
    /// Delegates to `SimState::creature_relation()` which uses the centralized
    /// diplomatic relation logic (civ relationships + engagement initiative).
    ///
    /// Used by `selection_controller.gd` for right-click attack decisions.
    #[func]
    fn is_hostile_by_id(&self, subject_uuid: GString, object_uuid: GString) -> bool {
        let Some(sim) = &self.session.sim else {
            return false;
        };
        let Some(s_id) = parse_creature_id(&subject_uuid.to_string()) else {
            return false;
        };
        let Some(o_id) = parse_creature_id(&object_uuid.to_string()) else {
            return false;
        };
        sim.creature_relation(s_id, o_id).is_hostile()
    }

    /// Return info about the creature at the given species-filtered index.
    ///
    /// The index corresponds to the creature's position in the iteration
    /// order of `get_creature_positions()` — i.e., BTreeMap order filtered
    /// by species. The `render_tick` parameter is
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
            .filter(|c| c.species == species && c.vital_status != VitalStatus::Dead)
            .nth(index as usize);
        match creature {
            Some(c) => build_creature_info_dict(sim, c, render_tick),
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

        // Build dicts directly per creature, then sort.
        let species_list = [
            Species::Elf,
            Species::Boar,
            Species::Capybara,
            Species::Deer,
            Species::Elephant,
            Species::Monkey,
            Species::Squirrel,
        ];

        let mut entries: Vec<VarDictionary> = Vec::new();

        for &sp in &species_list {
            for (idx, creature) in (0_i32..).zip(
                sim.db
                    .creatures
                    .iter_all()
                    .filter(|c| c.species == sp && c.vital_status == VitalStatus::Alive),
            ) {
                let task_kind = creature
                    .current_task
                    .as_ref()
                    .and_then(|tid| sim.db.tasks.get(tid).map(|t| t.kind_tag.display_name()))
                    .unwrap_or("");

                let path_short = sim
                    .creature_path(creature.id)
                    .map(|p| p.short_name())
                    .unwrap_or("");

                let mut dict = VarDictionary::new();
                dict.set(
                    "creature_id",
                    GString::from(creature.id.0.to_string().as_str()),
                );
                dict.set("species", GString::from(species_name(sp)));
                dict.set("index", idx);
                dict.set("name", GString::from(creature.name.as_str()));
                dict.set(
                    "name_meaning",
                    GString::from(creature.name_meaning.as_str()),
                );
                dict.set("has_task", creature.current_task.is_some());
                dict.set("task_kind", GString::from(task_kind));
                dict.set("path_short", GString::from(path_short));
                entries.push(dict);
            }
        }

        // Sort: elves first (alphabetically by name), then other species
        // (alphabetically by species name, then by index).
        entries.sort_by(|a, b| {
            let a_sp: String = a.get("species").unwrap_or_default().to_string();
            let b_sp: String = b.get("species").unwrap_or_default().to_string();
            let a_is_elf = a_sp == "Elf";
            let b_is_elf = b_sp == "Elf";
            match (a_is_elf, b_is_elf) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (true, true) => {
                    let a_name: String = a.get("name").unwrap_or_default().to_string();
                    let b_name: String = b.get("name").unwrap_or_default().to_string();
                    a_name.cmp(&b_name)
                }
                (false, false) => {
                    let a_idx: i32 = a.get("index").unwrap_or_default().to::<i32>();
                    let b_idx: i32 = b.get("index").unwrap_or_default().to::<i32>();
                    a_sp.cmp(&b_sp).then(a_idx.cmp(&b_idx))
                }
            }
        });

        let mut result = VarArray::new();
        for dict in &entries {
            result.push(&dict.to_variant());
        }
        result
    }

    /// Return all active group activities as a `VarArray` of dictionaries.
    ///
    /// Each dictionary contains: `id_full` (String), `kind` (String),
    /// `phase` (String), `location_x/y/z` (int), `progress` (int),
    /// `total_cost` (int), `min_count` (int), `desired_count` (int),
    /// `participants` (Array of dicts with `creature_id`, `name`, `species`,
    /// `status`). Used by `task_panel.gd` to display the activities section.
    #[func]
    fn get_active_activities(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };

        let mut result = VarArray::new();
        for activity in sim.db.activities.iter_all() {
            // Skip terminal phases.
            if matches!(
                activity.phase,
                elven_canopy_sim::types::ActivityPhase::Complete
                    | elven_canopy_sim::types::ActivityPhase::Cancelled
            ) {
                continue;
            }

            let mut dict = VarDictionary::new();
            let id_full = activity.id.0.to_string();
            dict.set("id_full", GString::from(&id_full));

            let kind_str = match activity.kind {
                elven_canopy_sim::types::ActivityKind::Dance => "Dance",
                elven_canopy_sim::types::ActivityKind::ConstructionChoir => "Construction Choir",
                elven_canopy_sim::types::ActivityKind::CombatSinging => "Combat Singing",
                elven_canopy_sim::types::ActivityKind::GroupHaul => "Group Haul",
                elven_canopy_sim::types::ActivityKind::Ceremony => "Ceremony",
                elven_canopy_sim::types::ActivityKind::DinnerParty => "Dinner Party",
            };
            dict.set("kind", GString::from(kind_str));

            let phase_str = match activity.phase {
                elven_canopy_sim::types::ActivityPhase::Recruiting => "Recruiting",
                elven_canopy_sim::types::ActivityPhase::Assembling => "Assembling",
                elven_canopy_sim::types::ActivityPhase::Executing => "Executing",
                elven_canopy_sim::types::ActivityPhase::Paused => "Paused",
                elven_canopy_sim::types::ActivityPhase::Complete => "Complete",
                elven_canopy_sim::types::ActivityPhase::Cancelled => "Cancelled",
            };
            dict.set("phase", GString::from(phase_str));

            dict.set("location_x", activity.location.x);
            dict.set("location_y", activity.location.y);
            dict.set("location_z", activity.location.z);
            dict.set("progress", activity.progress as i32);
            dict.set("total_cost", activity.total_cost as i32);
            dict.set("min_count", activity.min_count.unwrap_or(0) as i32);
            dict.set("desired_count", activity.desired_count.unwrap_or(0) as i32);

            // Participants.
            let mut participants_arr = VarArray::new();
            for p in sim
                .db
                .activity_participants
                .by_activity_id(&activity.id, elven_canopy_sim::tabulosity::QueryOpts::ASC)
            {
                let mut pd = VarDictionary::new();
                let cid_full = p.creature_id.0.to_string();
                pd.set("creature_id", GString::from(cid_full.as_str()));

                let status_str = match p.status {
                    elven_canopy_sim::types::ParticipantStatus::Volunteered => "Volunteered",
                    elven_canopy_sim::types::ParticipantStatus::Traveling => "Traveling",
                    elven_canopy_sim::types::ParticipantStatus::Arrived => "Arrived",
                };
                pd.set("status", GString::from(status_str));

                // Look up creature name and species.
                if let Some(creature) = sim.db.creatures.get(&p.creature_id) {
                    pd.set("name", GString::from(creature.name.as_str()));
                    pd.set("species", GString::from(species_name(creature.species)));
                    let path_short = sim
                        .creature_path(creature.id)
                        .map(|pp| pp.short_name())
                        .unwrap_or("");
                    pd.set("path_short", GString::from(path_short));
                } else {
                    pd.set("name", GString::from("???"));
                    pd.set("species", GString::from("Unknown"));
                    pd.set("path_short", GString::from(""));
                }

                participants_arr.push(&pd.to_variant());
            }
            dict.set("participants", participants_arr);

            result.push(&dict.to_variant());
        }
        result
    }

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
            dict.set("progress", task.progress as i32);
            dict.set("total_cost", task.total_cost as i32);

            // Location is now stored directly as VoxelCoord.
            dict.set("location_x", task.location.x);
            dict.set("location_y", task.location.y);
            dict.set("location_z", task.location.z);

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
                a.set("creature_id", GString::from(cid_full.as_str()));
                a.set("name", GString::from(creature.name.as_str()));

                let sp = species_name(creature.species);
                a.set("species", GString::from(sp));

                let path_short = sim
                    .creature_path(creature.id)
                    .map(|p| p.short_name())
                    .unwrap_or("");
                a.set("path_short", GString::from(path_short));

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
                BuildType::Wall => "Wall",
                BuildType::Enclosure => "Enclosure",
                BuildType::Building => "Building",
                BuildType::WoodLadder => "WoodLadder",
                BuildType::RopeLadder => "RopeLadder",
                BuildType::Carve => "Carve",
                BuildType::Strut => "Strut",
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
        let y_cutoff = self.mesh_cache.as_ref().and_then(|c| c.y_cutoff());
        match sim.raycast_structure(from, d, 500, y_cutoff) {
            Some(sid) => sid.0 as i64,
            None => -1,
        }
    }

    /// Cast a ray and return detailed structure hit info for roof-click-select.
    ///
    /// Returns `{sid: int, is_roof: bool, roof_y: int}` if a structure voxel
    /// was hit, or `{sid: -1}` if nothing was hit. `is_roof` is true when the
    /// hit voxel is on the topmost Y layer of a Building or Enclosure.
    /// `roof_y` is the Y coordinate of that layer (only meaningful when
    /// `is_roof` is true).
    ///
    /// Used by `selection_controller.gd` to decide whether a click on a
    /// building roof should shield creatures inside from selection.
    #[func]
    fn raycast_structure_detailed(
        &self,
        origin: Vector3,
        dir: Vector3,
        skip_roofs: bool,
    ) -> VarDictionary {
        let mut dict = VarDictionary::new();
        let Some(sim) = &self.session.sim else {
            dict.set("sid", -1_i64);
            return dict;
        };
        let from = [origin.x, origin.y, origin.z];
        let d = [dir.x, dir.y, dir.z];
        let y_cutoff = self.mesh_cache.as_ref().and_then(|c| c.y_cutoff());
        match sim.raycast_structure_with_hit(from, d, 500, skip_roofs, y_cutoff) {
            Some((sid, coord)) => {
                dict.set("sid", sid.0 as i64);
                let structure = sim.db.structures.get(&sid);
                let is_roof = structure.is_some_and(|s| s.is_roof_voxel(coord));
                dict.set("is_roof", is_roof);
                if is_roof {
                    dict.set("roof_y", coord.y as i64);
                }
                dict
            }
            None => {
                dict.set("sid", -1_i64);
                dict
            }
        }
    }

    /// Cast a ray and return the first solid voxel hit and entry face.
    /// Returns `{hit: true, voxel: Vector3i, face: int}` or `{hit: false}`.
    /// Raycasts against the **actual world only** — designated blueprints are
    /// invisible. Use `raycast_solid_with_blueprints()` to also hit blueprint
    /// voxels.
    #[func]
    fn raycast_solid(&self, origin: Vector3, dir: Vector3) -> VarDictionary {
        self.raycast_solid_impl(origin, dir, false)
    }

    /// Cast a ray and return the first solid voxel hit and entry face.
    /// Returns `{hit: true, voxel: Vector3i, face: int}` or `{hit: false}`.
    ///
    /// **Blueprint-aware:** Treats designated (not yet built) blueprints as
    /// their target voxel types — a designated platform reads as solid and
    /// can be "hit" by the ray. Used by `construction_controller.gd` for
    /// building/ladder placement so the player can click on blueprint surfaces.
    #[func]
    fn raycast_solid_with_blueprints(&self, origin: Vector3, dir: Vector3) -> VarDictionary {
        self.raycast_solid_impl(origin, dir, true)
    }

    /// Shared implementation for `raycast_solid` and
    /// `raycast_solid_with_blueprints`.
    fn raycast_solid_impl(
        &self,
        origin: Vector3,
        dir: Vector3,
        include_blueprints: bool,
    ) -> VarDictionary {
        let mut dict = VarDictionary::new();
        let Some(sim) = &self.session.sim else {
            dict.set("hit", false);
            return dict;
        };
        let from = [origin.x, origin.y, origin.z];
        let d = [dir.x, dir.y, dir.z];
        let overlay = if include_blueprints {
            Some(sim.blueprint_overlay())
        } else {
            None
        };
        let y_cutoff = self.mesh_cache.as_ref().and_then(|c| c.y_cutoff());
        match sim.raycast_solid(from, d, 500, overlay.as_ref(), y_cutoff) {
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
    /// Checks the **actual world only** — designated blueprints are invisible.
    /// Use `get_voxel_solidity_slice_with_blueprints()` to include them.
    #[func]
    fn get_voxel_solidity_slice(&self, y: i32, cx: i32, cz: i32, radius: i32) -> PackedByteArray {
        self.get_voxel_solidity_slice_impl(y, cx, cz, radius, false)
    }

    /// Return a square grid of solid/air flags at the given Y-level, centered
    /// on `(cx, cz)` with the given radius. Returns a `PackedByteArray` of
    /// `(2*radius+1)^2` bytes, row-major (X varies fastest). 1=solid, 0=air.
    ///
    /// **Blueprint-aware:** Treats designated (not yet built) blueprints as
    /// their target voxel types — a designated platform reads as solid (1).
    /// Used by `height_grid_renderer.gd` during construction mode so the
    /// grid overlay shows blueprint voxels as solid.
    #[func]
    fn get_voxel_solidity_slice_with_blueprints(
        &self,
        y: i32,
        cx: i32,
        cz: i32,
        radius: i32,
    ) -> PackedByteArray {
        self.get_voxel_solidity_slice_impl(y, cx, cz, radius, true)
    }

    /// Shared implementation for `get_voxel_solidity_slice` and
    /// `get_voxel_solidity_slice_with_blueprints`.
    fn get_voxel_solidity_slice_impl(
        &self,
        y: i32,
        cx: i32,
        cz: i32,
        radius: i32,
        include_blueprints: bool,
    ) -> PackedByteArray {
        let Some(sim) = &self.session.sim else {
            return PackedByteArray::new();
        };
        let overlay = if include_blueprints {
            Some(sim.blueprint_overlay())
        } else {
            None
        };
        let side = (2 * radius + 1) as usize;
        let mut data = Vec::with_capacity(side * side);
        for z in (cz - radius)..=(cz + radius) {
            for x in (cx - radius)..=(cx + radius) {
                let coord = VoxelCoord::new(x, y, z);
                let zone = sim.voxel_zone(self.active_zone_id).unwrap();
                let vt = match &overlay {
                    Some(ov) => ov.effective_type(zone, coord),
                    None => zone.get(coord),
                };
                data.push(if vt.is_solid() { 1u8 } else { 0u8 });
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

    /// Return chunk-column coords `[cx0, cz0, cx1, cz1, ...]` whose terrain
    /// heightmap changed since the last call. The minimap calls this once per
    /// frame to discover which tiles need re-fetching. Drains and clears the
    /// internal dirty set.
    #[func]
    fn drain_dirty_minimap_tiles(&mut self) -> PackedInt32Array {
        let Some(sim) = &mut self.session.sim else {
            return PackedInt32Array::new();
        };
        let dirty = sim
            .voxel_zone_mut(self.active_zone_id)
            .unwrap()
            .drain_dirty_heightmap_tiles();
        let mut result = PackedInt32Array::new();
        result.resize(dirty.len() * 2);
        for (i, (cx, cz)) in dirty.iter().enumerate() {
            result[i * 2] = *cx;
            result[i * 2 + 1] = *cz;
        }
        result
    }

    /// Return heightmap data for a batch of chunk-columns. `coords` is a flat
    /// array of `[cx0, cz0, cx1, cz1, ...]` pairs. Returns 512 bytes per
    /// tile: interleaved `(height, voxel_type)` pairs for 16×16 columns,
    /// row-major X-fastest. Concatenated in request order.
    #[func]
    fn get_minimap_tiles(&self, coords: PackedInt32Array) -> PackedByteArray {
        let Some(sim) = &self.session.sim else {
            return PackedByteArray::new();
        };
        let n = coords.len() / 2;
        let mut pairs = Vec::with_capacity(n);
        for i in 0..n {
            pairs.push((coords[i * 2], coords[i * 2 + 1]));
        }
        PackedByteArray::from(
            sim.voxel_zone(self.active_zone_id)
                .unwrap()
                .heightmap_tiles_batch(&pairs)
                .as_slice(),
        )
    }

    /// Return the world dimensions as `Vector3i(size_x, size_y, size_z)`.
    /// Used by GDScript for clamping placement coordinates to world bounds.
    #[func]
    fn get_world_size(&self) -> Vector3i {
        let Some(sim) = &self.session.sim else {
            return Vector3i::new(0, 0, 0);
        };
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        Vector3i::new(zone.size_x as i32, zone.size_y as i32, zone.size_z as i32)
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
            BuildType::Wall => "Wall",
            BuildType::Enclosure => "Enclosure",
            BuildType::Building => "Building",
            BuildType::WoodLadder => "WoodLadder",
            BuildType::RopeLadder => "RopeLadder",
            BuildType::Carve => "Carve",
            BuildType::Strut => "Strut",
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
            item_dict.set("item_stack_id", stack.id.0 as i64);
            item_dict.set(
                "kind",
                GString::from(sim.item_display_name(&stack).as_str()),
            );
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
            // Serialize material filter as JSON string for GDScript.
            let filter_json = Self::serialize_material_filter(want.material_filter);
            let filter_str = filter_json.to_string();
            want_dict.set("material_filter", GString::from(&filter_str));
            // Build display label: "Any Fruit", "Shinethúni Fruit", "Oak Bow", etc.
            let label = match want.material_filter {
                elven_canopy_sim::inventory::MaterialFilter::Any => {
                    format!("Any {}", want.item_kind.display_name())
                }
                elven_canopy_sim::inventory::MaterialFilter::Specific(mat) => {
                    sim.material_item_display_name(want.item_kind, mat)
                }
                elven_canopy_sim::inventory::MaterialFilter::NonWood => {
                    format!("Non-wood {}", want.item_kind.display_name())
                }
            };
            want_dict.set("label", GString::from(label.as_str()));
            want_dict.set("target_quantity", want.target_quantity as i64);
            wants_arr.push(&want_dict.to_variant());
        }
        dict.set("logistics_wants", wants_arr);

        // Unified crafting data (for Kitchen, Workshop, and future building types).
        dict.set("crafting_enabled", structure.crafting_enabled);

        // Active recipes for this building.
        let active_recipes = sim
            .db
            .active_recipes
            .by_structure_id(&sid, elven_canopy_sim::tabulosity::QueryOpts::ASC);
        // Sort by sort_order (the compound index iterates by structure_id first).
        let mut sorted_recipes = active_recipes;
        sorted_recipes.sort_by_key(|r| r.sort_order);

        let mut recipes_arr = VarArray::new();
        let mut active_count = 0;
        let mut satisfied_count = 0;
        for ar in &sorted_recipes {
            let mut recipe_dict = VarDictionary::new();
            recipe_dict.set("active_recipe_id", ar.id.0 as i64);
            recipe_dict.set("recipe_variant", ar.recipe as u16 as i64);
            let fruit_species: Vec<_> = sim.db.fruit_species.iter_all().cloned().collect();
            let params = elven_canopy_sim::recipe::RecipeParams {
                material: ar.material,
            };
            let display_name = ar.recipe.display_name(&params, &fruit_species);
            recipe_dict.set("recipe_display_name", GString::from(display_name.as_str()));
            if let Some(mat) = ar.material {
                let mat_json = serde_json::to_string(&mat).unwrap_or_default();
                recipe_dict.set("material_json", GString::from(mat_json.as_str()));
            }
            recipe_dict.set("enabled", ar.enabled);
            recipe_dict.set("auto_logistics", ar.auto_logistics);
            recipe_dict.set("spare_iterations", ar.spare_iterations as i64);

            // Per-output targets with stock counts.
            let targets = sim
                .db
                .active_recipe_targets
                .by_active_recipe_id(&ar.id, elven_canopy_sim::tabulosity::QueryOpts::ASC);
            let mut targets_arr = VarArray::new();
            let mut all_satisfied = true;
            let mut any_nonzero_target = false;
            for target in &targets {
                let mut target_dict = VarDictionary::new();
                target_dict.set("target_id", target.id.0 as i64);
                target_dict.set(
                    "item_kind",
                    GString::from(target.output_item_kind.display_name()),
                );
                if let Some(mat) = target.output_material {
                    target_dict.set(
                        "material",
                        GString::from(
                            sim.material_item_display_name(target.output_item_kind, mat)
                                .as_str(),
                        ),
                    );
                }
                target_dict.set("target_quantity", target.target_quantity as i64);

                // Stock count: total items (including reserved) for UI display.
                // The crafting monitor uses unreserved counts to decide when to
                // create tasks, so "satisfied" status and displayed stock may
                // briefly disagree while a craft is in progress.
                let mat_filter = match target.output_material {
                    Some(m) => elven_canopy_sim::inventory::MaterialFilter::Specific(m),
                    None => elven_canopy_sim::inventory::MaterialFilter::Any,
                };
                let stock =
                    sim.inv_item_count(structure.inventory_id, target.output_item_kind, mat_filter);
                target_dict.set("stock", stock as i64);

                if target.target_quantity > 0 {
                    any_nonzero_target = true;
                    if stock < target.target_quantity {
                        all_satisfied = false;
                    }
                }

                targets_arr.push(&target_dict.to_variant());
            }
            recipe_dict.set("targets", targets_arr);

            if ar.enabled && any_nonzero_target {
                active_count += 1;
                if all_satisfied {
                    satisfied_count += 1;
                }
            }

            recipes_arr.push(&recipe_dict.to_variant());
        }
        dict.set("active_recipes", recipes_arr);
        dict.set("active_recipe_count", active_count as i64);
        dict.set("satisfied_recipe_count", satisfied_count as i64);

        // Craft status: check if there's an active craft task for this building.
        let craft_status = if structure.crafting_enabled && !sorted_recipes.is_empty() {
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
            .filter(|c| c.species == Species::Elf && c.vital_status == VitalStatus::Alive)
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

    /// Parse an `ItemKind` from its display name. Returns `None` if unknown.
    fn parse_item_kind(name: &str) -> Option<elven_canopy_sim::inventory::ItemKind> {
        use elven_canopy_sim::inventory::ItemKind;
        match name {
            "Bread" => Some(ItemKind::Bread),
            "Fruit" => Some(ItemKind::Fruit),
            "Bow" => Some(ItemKind::Bow),
            "Arrow" => Some(ItemKind::Arrow),
            "Bowstring" => Some(ItemKind::Bowstring),
            "Pulp" => Some(ItemKind::Pulp),
            "Husk" => Some(ItemKind::Husk),
            "Seed" => Some(ItemKind::Seed),
            "Fiber" => Some(ItemKind::FruitFiber),
            "Sap" => Some(ItemKind::FruitSap),
            "Resin" => Some(ItemKind::FruitResin),
            "Flour" => Some(ItemKind::Flour),
            "Thread" => Some(ItemKind::Thread),
            "Cord" => Some(ItemKind::Cord),
            "Cloth" => Some(ItemKind::Cloth),
            "Tunic" => Some(ItemKind::Tunic),
            "Leggings" => Some(ItemKind::Leggings),
            "Boots" => Some(ItemKind::Boots),
            "Sandals" => Some(ItemKind::Sandals),
            "Shoes" => Some(ItemKind::Shoes),
            "Hat" => Some(ItemKind::Hat),
            "Helmet" => Some(ItemKind::Helmet),
            "Breastplate" => Some(ItemKind::Breastplate),
            "Greaves" => Some(ItemKind::Greaves),
            "Gauntlets" => Some(ItemKind::Gauntlets),
            "Gloves" => Some(ItemKind::Gloves),
            "Dye" => Some(ItemKind::Dye),
            "Spear" => Some(ItemKind::Spear),
            "Club" => Some(ItemKind::Club),
            _ => None,
        }
    }

    /// Parse a `MaterialFilter` from a JSON value (matching serde externally-tagged
    /// enum format). Returns `Any` if missing or malformed.
    fn parse_material_filter(
        val: Option<&serde_json::Value>,
    ) -> elven_canopy_sim::inventory::MaterialFilter {
        use elven_canopy_sim::inventory::MaterialFilter;
        let Some(v) = val else {
            return MaterialFilter::Any;
        };
        if v == "Any" {
            return MaterialFilter::Any;
        }
        if v == "NonWood" {
            return MaterialFilter::NonWood;
        }
        if let Some(obj) = v.as_object()
            && let Some(specific) = obj.get("Specific")
            && let Some(mat) = Self::parse_material_value(specific)
        {
            return MaterialFilter::Specific(mat);
        }
        MaterialFilter::Any
    }

    /// Parse a `Material` from a JSON value. Wood types are bare strings
    /// ("Oak", "Birch", etc.). Fruit species use `{"FruitSpecies": id}`.
    fn parse_material_value(
        val: &serde_json::Value,
    ) -> Option<elven_canopy_sim::inventory::Material> {
        use elven_canopy_sim::inventory::Material;
        if let Some(s) = val.as_str() {
            return match s {
                "Oak" => Some(Material::Oak),
                "Birch" => Some(Material::Birch),
                "Willow" => Some(Material::Willow),
                "Ash" => Some(Material::Ash),
                "Yew" => Some(Material::Yew),
                _ => None,
            };
        }
        if let Some(obj) = val.as_object()
            && let Some(fs_val) = obj.get("FruitSpecies")
        {
            // Godot's JSON parser converts all numbers to float, so 0 becomes
            // 0.0. Try as_u64 first (from Rust-generated JSON), then as_f64
            // (from Godot roundtripped JSON).
            let id = fs_val
                .as_u64()
                .or_else(|| fs_val.as_f64().map(|f| f as u64))?;
            let id16 = u16::try_from(id).ok()?;
            return Some(Material::FruitSpecies(
                elven_canopy_sim::fruit::FruitSpeciesId(id16),
            ));
        }
        None
    }

    /// Serialize a `MaterialFilter` to a `serde_json::Value` matching the serde
    /// externally-tagged enum format.
    fn serialize_material_filter(
        filter: elven_canopy_sim::inventory::MaterialFilter,
    ) -> serde_json::Value {
        use elven_canopy_sim::inventory::{Material, MaterialFilter};
        use serde_json::{Value, json};
        match filter {
            MaterialFilter::Any => Value::String("Any".into()),
            MaterialFilter::Specific(mat) => {
                let mat_val = match mat {
                    Material::Oak => Value::String("Oak".into()),
                    Material::Birch => Value::String("Birch".into()),
                    Material::Willow => Value::String("Willow".into()),
                    Material::Ash => Value::String("Ash".into()),
                    Material::Yew => Value::String("Yew".into()),
                    Material::FruitSpecies(id) => json!({"FruitSpecies": id.0}),
                };
                json!({"Specific": mat_val})
            }
            MaterialFilter::NonWood => Value::String("NonWood".into()),
        }
    }

    /// Set the logistics wants for a building. Expects a JSON string like:
    /// `[{"kind": "Bread", "material_filter": "Any", "quantity": 10}]`
    ///
    /// Material filter encoding (matches serde externally-tagged enum format):
    /// - `"Any"` → `MaterialFilter::Any`
    /// - `{"Specific": "Oak"}` → `MaterialFilter::Specific(Material::Oak)`
    /// - `{"Specific": {"FruitSpecies": 42}}` → `MaterialFilter::Specific(Material::FruitSpecies(...))`
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
            let kind = match Self::parse_item_kind(kind_str) {
                Some(k) => k,
                None => {
                    godot_error!("SimBridge: unknown item kind in logistics wants: '{kind_str}'");
                    continue;
                }
            };
            let material_filter = Self::parse_material_filter(entry.get("material_filter"));
            let quantity = entry.get("quantity").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if quantity > 0 {
                wants.push(elven_canopy_sim::building::LogisticsWant {
                    item_kind: kind,
                    material_filter,
                    target_quantity: quantity,
                });
            }
        }
        self.apply_or_send(SimAction::SetLogisticsWants {
            structure_id: StructureId(structure_id as u64),
            wants,
        });
    }

    /// Get all available item kinds for the logistics UI picker.
    /// Returns a VarArray of dictionaries: `[{"kind": "Bread", "label": "Bread"}, ...]`
    #[func]
    fn get_logistics_item_kinds(&self) -> VarArray {
        use elven_canopy_sim::inventory::ItemKind;
        let mut arr = VarArray::new();
        for kind in [
            ItemKind::Bread,
            ItemKind::Fruit,
            ItemKind::Bow,
            ItemKind::Arrow,
            ItemKind::Bowstring,
            ItemKind::Pulp,
            ItemKind::Husk,
            ItemKind::Seed,
            ItemKind::FruitFiber,
            ItemKind::FruitSap,
            ItemKind::FruitResin,
            ItemKind::Flour,
            ItemKind::Thread,
            ItemKind::Cord,
            ItemKind::Cloth,
            ItemKind::Tunic,
            ItemKind::Leggings,
            ItemKind::Boots,
            ItemKind::Sandals,
            ItemKind::Shoes,
            ItemKind::Hat,
            ItemKind::Helmet,
            ItemKind::Breastplate,
            ItemKind::Greaves,
            ItemKind::Gauntlets,
            ItemKind::Dye,
            ItemKind::Spear,
            ItemKind::Club,
        ] {
            let mut d = VarDictionary::new();
            d.set("kind", GString::from(kind.display_name()));
            d.set("label", GString::from(kind.display_name()));
            if let Some(slot) = kind.equip_slot() {
                d.set("equip_slot", GString::from(slot.display_name()));
            }
            arr.push(&d.to_variant());
        }
        arr
    }

    /// Get material filter options for a given item kind (two-step UI picker).
    /// Returns a VarArray of dictionaries:
    /// `[{"filter": "Any", "label": "Any Fruit"}, {"filter": {"Specific": ...}, "label": "..."}, ...]`
    ///
    /// Always includes an "Any" option. For Fruit and fruit-derived items
    /// (components, flour, thread, cord, bread), includes each fruit species
    /// in the DB. For Bow/Arrow, includes wood materials. For Bowstring,
    /// includes both wood materials and fruit species.
    #[func]
    fn get_logistics_material_options(&self, kind_name: GString) -> VarArray {
        use elven_canopy_sim::inventory::{ItemKind, Material, MaterialFilter};
        let mut arr = VarArray::new();
        let kind_str = kind_name.to_string();
        let kind = match kind_str.as_str() {
            "Bread" => ItemKind::Bread,
            "Fruit" => ItemKind::Fruit,
            "Bow" => ItemKind::Bow,
            "Arrow" => ItemKind::Arrow,
            "Bowstring" => ItemKind::Bowstring,
            "Pulp" => ItemKind::Pulp,
            "Husk" => ItemKind::Husk,
            "Seed" => ItemKind::Seed,
            "Fiber" => ItemKind::FruitFiber,
            "Sap" => ItemKind::FruitSap,
            "Resin" => ItemKind::FruitResin,
            "Flour" => ItemKind::Flour,
            "Thread" => ItemKind::Thread,
            "Cord" => ItemKind::Cord,
            "Cloth" => ItemKind::Cloth,
            "Tunic" => ItemKind::Tunic,
            "Leggings" => ItemKind::Leggings,
            "Boots" => ItemKind::Boots,
            "Sandals" => ItemKind::Sandals,
            "Shoes" => ItemKind::Shoes,
            "Hat" => ItemKind::Hat,
            "Helmet" => ItemKind::Helmet,
            "Breastplate" => ItemKind::Breastplate,
            "Greaves" => ItemKind::Greaves,
            "Gauntlets" => ItemKind::Gauntlets,
            "Dye" => ItemKind::Dye,
            "Spear" => ItemKind::Spear,
            "Club" => ItemKind::Club,
            _ => return arr,
        };

        // Always include "Any" first.
        let any_label = format!("Any {}", kind.display_name());
        let mut any_dict = VarDictionary::new();
        let any_filter_str = Self::serialize_material_filter(MaterialFilter::Any).to_string();
        any_dict.set("filter", GString::from(&any_filter_str));
        any_dict.set("label", GString::from(any_label.as_str()));
        arr.push(&any_dict.to_variant());

        let Some(sim) = &self.session.sim else {
            return arr;
        };

        match kind {
            ItemKind::Fruit
            | ItemKind::Pulp
            | ItemKind::Husk
            | ItemKind::Seed
            | ItemKind::FruitFiber
            | ItemKind::FruitSap
            | ItemKind::FruitResin
            | ItemKind::Flour
            | ItemKind::Thread
            | ItemKind::Cord
            | ItemKind::Cloth
            | ItemKind::Tunic
            | ItemKind::Leggings
            | ItemKind::Sandals
            | ItemKind::Shoes
            | ItemKind::Hat
            | ItemKind::Gloves
            | ItemKind::Dye => {
                // Add each fruit species from the DB.
                for species in sim.db.fruit_species.iter_all() {
                    let mat = Material::FruitSpecies(species.id);
                    let label = sim.material_item_display_name(kind, mat);
                    let filter_str =
                        Self::serialize_material_filter(MaterialFilter::Specific(mat)).to_string();
                    let mut d = VarDictionary::new();
                    d.set("filter", GString::from(&filter_str));
                    d.set("label", GString::from(label.as_str()));
                    arr.push(&d.to_variant());
                }
            }
            ItemKind::Bow
            | ItemKind::Arrow
            | ItemKind::Bowstring
            | ItemKind::Helmet
            | ItemKind::Breastplate
            | ItemKind::Greaves
            | ItemKind::Gauntlets
            | ItemKind::Boots
            | ItemKind::Spear
            | ItemKind::Club => {
                // Add each wood material.
                for mat in [
                    Material::Oak,
                    Material::Birch,
                    Material::Willow,
                    Material::Ash,
                    Material::Yew,
                ] {
                    let label = sim.material_item_display_name(kind, mat);
                    let filter_str =
                        Self::serialize_material_filter(MaterialFilter::Specific(mat)).to_string();
                    let mut d = VarDictionary::new();
                    d.set("filter", GString::from(&filter_str));
                    d.set("label", GString::from(label.as_str()));
                    arr.push(&d.to_variant());
                }
                // Bowstrings can also be crafted from fruit fiber/cord.
                if kind == ItemKind::Bowstring {
                    for species in sim.db.fruit_species.iter_all() {
                        let mat = Material::FruitSpecies(species.id);
                        let label = sim.material_item_display_name(kind, mat);
                        let filter_str =
                            Self::serialize_material_filter(MaterialFilter::Specific(mat))
                                .to_string();
                        let mut d = VarDictionary::new();
                        d.set("filter", GString::from(&filter_str));
                        d.set("label", GString::from(label.as_str()));
                        arr.push(&d.to_variant());
                    }
                }
            }
            ItemKind::Bread => {
                // Species-specific bread variants from component recipe baking.
                for species in sim.db.fruit_species.iter_all() {
                    let mat = Material::FruitSpecies(species.id);
                    let label = sim.material_item_display_name(kind, mat);
                    let filter_str =
                        Self::serialize_material_filter(MaterialFilter::Specific(mat)).to_string();
                    let mut d = VarDictionary::new();
                    d.set("filter", GString::from(&filter_str));
                    d.set("label", GString::from(label.as_str()));
                    arr.push(&d.to_variant());
                }
            }
        }

        arr
    }

    /// Get the recipe catalog for a building, filtered to recipes available
    /// Get the list of Recipe enum variants valid for a building's furnishing
    /// type. Returns an array of recipe dicts with variant int, display name,
    /// category, and whether the recipe has a material parameter.
    #[func]
    fn get_available_recipes(&self, structure_id: i64) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let sid = StructureId(structure_id as u64);
        let Some(structure) = sim.db.structures.get(&sid) else {
            return VarArray::new();
        };
        let Some(ft) = structure.furnishing else {
            return VarArray::new();
        };
        let fruit_species: Vec<_> = sim.db.fruit_species.iter_all().cloned().collect();
        let mut arr = VarArray::new();
        for recipe in &elven_canopy_sim::recipe::ALL_RECIPES {
            if !recipe.furnishing_types().contains(&ft) {
                continue;
            }
            let mut d = VarDictionary::new();
            d.set("recipe", *recipe as u16 as i64);
            let name = format!("{:?}", recipe);
            d.set("display_name", GString::from(name.as_str()));
            let mut category = VarArray::new();
            for part in recipe.category() {
                category.push(&GString::from(part).to_variant());
            }
            d.set("category", category);
            d.set("has_material_param", recipe.has_material_param());

            // Include valid materials for this recipe.
            let mut materials = VarArray::new();
            for mat in recipe.valid_materials(&fruit_species) {
                let mut mat_dict = VarDictionary::new();
                let mat_json = serde_json::to_string(&mat).unwrap_or_default();
                mat_dict.set("material_json", GString::from(mat_json.as_str()));
                let params = elven_canopy_sim::recipe::RecipeParams {
                    material: Some(mat),
                };
                let display = recipe.display_name(&params, &fruit_species);
                mat_dict.set("display_name", GString::from(display.as_str()));
                materials.push(&mat_dict.to_variant());
            }
            d.set("materials", materials);

            arr.push(&d.to_variant());
        }
        arr
    }

    /// Set the unified crafting toggle for a building.
    #[func]
    fn set_crafting_enabled(&mut self, structure_id: i64, enabled: bool) {
        self.apply_or_send(SimAction::SetCraftingEnabled {
            structure_id: StructureId(structure_id as u64),
            enabled,
        });
    }

    /// Add an active recipe to a building by Recipe variant int + material JSON.
    #[func]
    fn add_active_recipe(
        &mut self,
        structure_id: i64,
        recipe_variant: i64,
        material_json: GString,
    ) {
        let recipe = match elven_canopy_sim::recipe::ALL_RECIPES
            .iter()
            .find(|r| **r as u16 as i64 == recipe_variant)
        {
            Some(r) => *r,
            None => {
                godot_error!("SimBridge: invalid recipe variant {recipe_variant}");
                return;
            }
        };
        let mat_str = material_json.to_string();
        let material: Option<elven_canopy_sim::inventory::Material> = if mat_str.is_empty() {
            None
        } else {
            match serde_json::from_str(&mat_str) {
                Ok(m) => Some(m),
                Err(e) => {
                    godot_error!("SimBridge: failed to parse material JSON: {e}");
                    return;
                }
            }
        };
        self.apply_or_send(SimAction::AddActiveRecipe {
            structure_id: StructureId(structure_id as u64),
            recipe,
            material,
        });
    }

    /// Remove an active recipe by its ActiveRecipeId.
    #[func]
    fn remove_active_recipe(&mut self, active_recipe_id: i64) {
        self.apply_or_send(SimAction::RemoveActiveRecipe {
            active_recipe_id: ActiveRecipeId(active_recipe_id as u64),
        });
    }

    /// Set the target quantity for a specific recipe output.
    #[func]
    fn set_recipe_output_target(&mut self, active_recipe_target_id: i64, target_quantity: i32) {
        self.apply_or_send(SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: ActiveRecipeTargetId(active_recipe_target_id as u64),
            target_quantity: target_quantity.max(0) as u32,
        });
    }

    /// Configure auto-logistics for an active recipe.
    #[func]
    fn set_recipe_auto_logistics(
        &mut self,
        active_recipe_id: i64,
        auto_logistics: bool,
        spare_iterations: i32,
    ) {
        self.apply_or_send(SimAction::SetRecipeAutoLogistics {
            active_recipe_id: ActiveRecipeId(active_recipe_id as u64),
            auto_logistics,
            spare_iterations: spare_iterations.max(0) as u32,
        });
    }

    /// Toggle an individual active recipe without removing it.
    #[func]
    fn set_recipe_enabled(&mut self, active_recipe_id: i64, enabled: bool) {
        self.apply_or_send(SimAction::SetRecipeEnabled {
            active_recipe_id: ActiveRecipeId(active_recipe_id as u64),
            enabled,
        });
    }

    /// Move an active recipe up in priority (lower sort_order).
    #[func]
    fn move_active_recipe_up(&mut self, active_recipe_id: i64) {
        self.apply_or_send(SimAction::MoveActiveRecipeUp {
            active_recipe_id: ActiveRecipeId(active_recipe_id as u64),
        });
    }

    /// Move an active recipe down in priority (higher sort_order).
    #[func]
    fn move_active_recipe_down(&mut self, active_recipe_id: i64) {
        self.apply_or_send(SimAction::MoveActiveRecipeDown {
            active_recipe_id: ActiveRecipeId(active_recipe_id as u64),
        });
    }

    /// Return positions for any species as a PackedVector3Array, interpolated
    /// to the given render tick for smooth movement between nav nodes.
    /// Returns positions for any species, used by creature_renderer.gd.
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
        for creature in sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == species && c.vital_status != VitalStatus::Dead)
        {
            let ma = sim.db.move_actions.get(&creature.id);
            let (x, y, z) = creature.interpolated_position(render_tick, ma.as_ref());
            arr.push(Vector3::new(x, y, z));
        }
        arr
    }

    /// Return HP ratios (hp / hp_max, clamped 0.0–1.0) for all non-dead creatures
    /// of the named species, in the same order as `get_creature_positions()`.
    /// Used by GDScript renderers to display overhead HP bars.
    #[func]
    fn get_creature_hp_ratios(&self, species_name: GString) -> PackedFloat32Array {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return PackedFloat32Array::new();
        };
        let Some(sim) = &self.session.sim else {
            return PackedFloat32Array::new();
        };
        let mut arr = PackedFloat32Array::new();
        for creature in sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == species && c.vital_status != VitalStatus::Dead)
        {
            let ratio = if creature.hp_max > 0 {
                creature.hp as f32 / creature.hp_max as f32
            } else {
                1.0
            };
            arr.push(ratio.clamp(0.0, 1.0));
        }
        arr
    }

    /// Return an array of mana ratios (0.0–1.0) for all non-dead creatures
    /// of the named species. Parallel to `get_creature_positions()`.
    /// Creatures with mp_max = 0 return 1.0 (no mana bar shown).
    #[func]
    fn get_creature_mp_ratios(&self, species_name: GString) -> PackedFloat32Array {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return PackedFloat32Array::new();
        };
        let Some(sim) = &self.session.sim else {
            return PackedFloat32Array::new();
        };
        let mut arr = PackedFloat32Array::new();
        for creature in sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == species && c.vital_status != VitalStatus::Dead)
        {
            let ratio = if creature.mp_max > 0 {
                creature.mp as f32 / creature.mp_max as f32
            } else {
                1.0 // nonmagical — always "full" so no bar is shown
            };
            arr.push(ratio.clamp(0.0, 1.0));
        }
        arr
    }

    /// Return a `PackedByteArray` of 0/1 flags indicating which creatures of
    /// the named species are incapacitated. Parallel to `get_creature_positions()`.
    /// Used by GDScript renderers to rotate sprites and change HP bar style.
    #[func]
    fn get_creature_incapacitated(&self, species_name: GString) -> PackedByteArray {
        let Some(species) = parse_species(&species_name.to_string()) else {
            return PackedByteArray::new();
        };
        let Some(sim) = &self.session.sim else {
            return PackedByteArray::new();
        };
        let flags: Vec<u8> = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == species && c.vital_status != VitalStatus::Dead)
            .map(|c| u8::from(c.vital_status == VitalStatus::Incapacitated))
            .collect();
        PackedByteArray::from(flags.as_slice())
    }

    /// Return positions where mana-wasted work actions occurred this step.
    /// Each position is a `Vector3` (x, y, z). The buffer is cleared at the
    /// start of each sim step, so this returns only positions from the most
    /// recent step. Used by `mana_vfx.gd` to spawn floating blue swirls.
    #[func]
    fn get_mana_wasted_positions(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let mut arr = VarArray::new();
        for pos in &sim
            .voxel_zone(self.active_zone_id)
            .unwrap()
            .mana_wasted_positions
        {
            arr.push(
                &Vector3::new(pos.x as f32 + 0.5, pos.y as f32 + 0.5, pos.z as f32 + 0.5)
                    .to_variant(),
            );
        }
        arr
    }

    /// Build a `SpriteKey` for a creature from current sim state.
    /// Species-agnostic: all creatures use the same path — biological traits
    /// for appearance, inventory for equipment.
    fn build_sprite_key(
        sim: &elven_canopy_sim::sim::SimState,
        creature: &elven_canopy_sim::db::Creature,
    ) -> SpriteKey {
        use elven_canopy_sim::inventory::{EquipSlot, WearCategory};
        use elven_canopy_sprites::EquipSlotDrawInfo;

        // Appearance from biological traits (all species).
        let trait_rows = sim
            .db
            .creature_traits
            .by_creature_id(&creature.id, elven_canopy_sim::tabulosity::QueryOpts::ASC);
        let trait_map: elven_canopy_sprites::TraitMap = trait_rows
            .into_iter()
            .map(|t| (t.trait_kind, t.value))
            .collect();
        let params = elven_canopy_sprites::species_params_from_traits(creature.species, &trait_map);

        // Equipment from inventory (all species — non-equipped creatures
        // just have all-None, which is free to compare).
        let worn_pct = sim.config.durability_worn_pct;
        let damaged_pct = sim.config.durability_damaged_pct;
        let mut equipment = [None; EquipSlot::COUNT];
        for stack in sim.inv_items(creature.inventory_id) {
            if let Some(slot) = stack.equipped_slot {
                let color = sim.item_color(&stack);
                equipment[slot as usize] = Some(EquipSlotDrawInfo {
                    kind: stack.kind,
                    color: elven_canopy_sprites::Color::from_item_color(color),
                    wear: WearCategory::from_hp(
                        stack.current_hp,
                        stack.max_hp,
                        worn_pct,
                        damaged_pct,
                    ),
                });
            }
        }

        SpriteKey { params, equipment }
    }

    /// Render a sprite from a `SpriteKey`, returning normal and fallen
    /// (90° CW rotated) textures.
    fn render_sprite(key: &SpriteKey) -> Option<(Gd<ImageTexture>, Gd<ImageTexture>)> {
        let buf = elven_canopy_sprites::create_sprite_with_equipment(&key.params, &key.equipment);
        let normal = pixel_buffer_to_texture(&buf)?;
        let fallen = pixel_buffer_to_texture(&buf.rotate_90_cw())?;
        Some((normal, fallen))
    }

    /// Return positions and status for all alive creatures in one call.
    ///
    /// Sprites are NOT included — callers get sprites per-creature via
    /// `get_creature_sprite_by_id()` (wrapped by `CreatureSprites` in
    /// GDScript). The Rust-side sprite cache handles change detection.
    ///
    /// Returns a `VarDictionary` with parallel arrays (all in BTreeMap
    /// iteration order, no species filtering):
    /// - `creature_ids`: `PackedStringArray` of creature UUID strings
    /// - `species`: `PackedStringArray` of species name strings
    /// - `positions`: `PackedVector3Array` of interpolated positions
    /// - `hp_ratios`: `PackedFloat32Array` (0.0–1.0)
    /// - `mp_ratios`: `PackedFloat32Array` (0.0–1.0, 1.0 for non-mana species)
    /// - `incap_flags`: `PackedByteArray` (1 = incapacitated)
    #[func]
    fn get_creature_render_data(&self, render_tick: f64) -> VarDictionary {
        let mut creature_ids = PackedStringArray::new();
        let mut species_names = PackedStringArray::new();
        let mut positions = PackedVector3Array::new();
        let mut hp_ratios = PackedFloat32Array::new();
        let mut mp_ratios = PackedFloat32Array::new();
        let mut incap_flags = PackedByteArray::new();

        let Some(sim) = &self.session.sim else {
            let mut result = VarDictionary::new();
            result.set("creature_ids", creature_ids);
            result.set("species", species_names);
            result.set("positions", positions);
            result.set("hp_ratios", hp_ratios);
            result.set("mp_ratios", mp_ratios);
            result.set("incap_flags", incap_flags);
            return result;
        };

        for creature in sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.vital_status != VitalStatus::Dead)
        {
            let cid = creature.id;
            creature_ids.push(&GString::from(cid.0.to_string().as_str()));
            species_names.push(&GString::from(species_name(creature.species)));

            let ma = sim.db.move_actions.get(&cid);
            let (x, y, z) = creature.interpolated_position(render_tick, ma.as_ref());
            positions.push(Vector3::new(x, y, z));

            let hp_ratio = if creature.hp_max > 0 {
                creature.hp as f32 / creature.hp_max as f32
            } else {
                1.0
            };
            hp_ratios.push(hp_ratio.clamp(0.0, 1.0));
            let mp_ratio = if creature.mp_max > 0 {
                creature.mp as f32 / creature.mp_max as f32
            } else {
                1.0
            };
            mp_ratios.push(mp_ratio.clamp(0.0, 1.0));
            let is_incap: u8 = if creature.vital_status == VitalStatus::Incapacitated {
                1
            } else {
                0
            };
            incap_flags.push(is_incap);
        }

        let mut result = VarDictionary::new();
        result.set("creature_ids", creature_ids);
        result.set("species", species_names);
        result.set("positions", positions);
        result.set("hp_ratios", hp_ratios);
        result.set("mp_ratios", mp_ratios);
        result.set("incap_flags", incap_flags);
        result
    }

    /// Return the trait-based sprite for a single creature by ID.
    ///
    /// Returns a `VarDictionary` with `normal` and `fallen` `ImageTexture`
    /// keys plus a `changed` bool, or an empty dict if the creature is not
    /// found. Uses the `creature_sprite_cache`.
    #[func]
    fn get_creature_sprite_by_id(&mut self, creature_id: GString) -> VarDictionary {
        let Some(cid) = parse_creature_id(&creature_id.to_string()) else {
            return VarDictionary::new();
        };
        let Some(sim) = &self.session.sim else {
            return VarDictionary::new();
        };
        let Some(creature) = sim.db.creatures.get(&cid) else {
            return VarDictionary::new();
        };

        let key = Self::build_sprite_key(sim, &creature);

        let is_changed = self
            .creature_sprite_cache
            .get(&cid)
            .is_none_or(|cached| cached.key != key);

        if is_changed && let Some((normal, fallen)) = Self::render_sprite(&key) {
            self.creature_sprite_cache.insert(
                cid,
                CachedCreatureSprite {
                    key,
                    normal: normal.clone(),
                    fallen: fallen.clone(),
                },
            );
            let mut result = VarDictionary::new();
            result.set("normal", normal);
            result.set("fallen", fallen);
            result.set("changed", true);
            return result;
        }

        if let Some(cached) = self.creature_sprite_cache.get(&cid) {
            let mut result = VarDictionary::new();
            result.set("normal", cached.normal.clone());
            result.set("fallen", cached.fallen.clone());
            result.set("changed", false);
            result
        } else {
            VarDictionary::new()
        }
    }

    /// Return positions of all in-flight projectiles as a PackedVector3Array.
    /// SubVoxelCoord converted to float world coords (voxel units).
    /// Used by projectile_renderer.gd for placement each frame.
    #[func]
    fn get_projectile_positions(&self, render_tick: f64) -> PackedVector3Array {
        let Some(sim) = &self.session.sim else {
            return PackedVector3Array::new();
        };
        let current_tick = sim.tick as f64;
        let frac = (render_tick - current_tick).clamp(0.0, 1.0) as f32;
        let mut arr = PackedVector3Array::new();
        for proj in sim.db.projectiles.iter_all() {
            let (px, py, pz) = proj.position.to_render_floats();
            let (vx, vy, vz) = proj.velocity.to_render_floats();
            arr.push(Vector3::new(px + vx * frac, py + vy * frac, pz + vz * frac));
        }
        arr
    }

    /// Return velocities of all in-flight projectiles as a PackedVector3Array,
    /// in the same order as `get_projectile_positions()`. Velocity is in
    /// voxels-per-tick (sub-voxel divided by 2^30). Used by the renderer to
    /// orient arrow meshes along the flight direction.
    #[func]
    fn get_projectile_velocities(&self) -> PackedVector3Array {
        let Some(sim) = &self.session.sim else {
            return PackedVector3Array::new();
        };
        let mut arr = PackedVector3Array::new();
        for proj in sim.db.projectiles.iter_all() {
            let (vx, vy, vz) = proj.velocity.to_render_floats();
            arr.push(Vector3::new(vx, vy, vz));
        }
        arr
    }

    /// Return creature positions and metadata for a given species.
    ///
    /// Returns a `VarDictionary` with four parallel arrays:
    /// - `"ids"`: `VarArray` of `GString` creature IDs (UUID strings)
    /// - `"positions"`: `PackedVector3Array` of interpolated positions
    /// - `"is_player_civ"`: `VarArray` of `bool` — true if creature belongs
    ///   to the player's civilization
    /// - `"military_group_ids"`: `VarArray` of `i64` — military group ID, or
    ///   -1 for civilians (no explicit group)
    ///
    /// Used by `selection_controller.gd` for hit-testing, box-select, and
    /// double-click group select.
    #[func]
    fn get_creature_positions_with_ids(
        &self,
        species_name: GString,
        render_tick: f64,
    ) -> VarDictionary {
        let mut dict = VarDictionary::new();
        let mut ids = VarArray::new();
        let mut positions = PackedVector3Array::new();
        let mut is_player_civ = VarArray::new();
        let mut military_group_ids = VarArray::new();
        let Some(species) = parse_species(&species_name.to_string()) else {
            dict.set("ids", ids);
            dict.set("positions", positions);
            dict.set("is_player_civ", is_player_civ);
            dict.set("military_group_ids", military_group_ids);
            return dict;
        };
        let Some(sim) = &self.session.sim else {
            dict.set("ids", ids);
            dict.set("positions", positions);
            dict.set("is_player_civ", is_player_civ);
            dict.set("military_group_ids", military_group_ids);
            return dict;
        };
        let player_civ = sim.player_civ_id;
        for creature in sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == species && c.vital_status != VitalStatus::Dead)
        {
            let ma = sim.db.move_actions.get(&creature.id);
            let (x, y, z) = creature.interpolated_position(render_tick, ma.as_ref());
            ids.push(&GString::from(creature.id.0.to_string().as_str()).to_variant());
            positions.push(Vector3::new(x, y, z));
            let belongs = player_civ.is_some() && creature.civ_id == player_civ;
            is_player_civ.push(&belongs.to_variant());
            let group_id: i64 = creature.military_group.map_or(-1, |g| g.0 as i64);
            military_group_ids.push(&group_id.to_variant());
        }
        dict.set("ids", ids);
        dict.set("positions", positions);
        dict.set("is_player_civ", is_player_civ);
        dict.set("military_group_ids", military_group_ids);
        dict
    }

    /// Look up a creature by its stable ID (UUID string) and return its info.
    ///
    /// Returns the same dictionary format as `get_creature_info()` but uses
    /// a direct ID lookup instead of fragile per-species index addressing.
    /// Returns an empty dictionary if the ID is invalid or the creature is dead.
    #[func]
    fn get_creature_info_by_id(&self, creature_id: GString, render_tick: f64) -> VarDictionary {
        let Some(sim) = &self.session.sim else {
            return VarDictionary::new();
        };
        let Some(cid) = parse_creature_id(&creature_id.to_string()) else {
            return VarDictionary::new();
        };
        let Some(c) = sim.db.creatures.get(&cid) else {
            return VarDictionary::new();
        };
        if c.vital_status == VitalStatus::Dead {
            return VarDictionary::new();
        }
        build_creature_info_dict(sim, &c, render_tick)
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

    /// Return all alive creatures' positions and their diplomatic relation to
    /// the player's civilization, in a single query (no per-species iteration).
    ///
    /// Returns a `VarDictionary` with parallel arrays:
    /// - `"ids"`: `VarArray` of `GString` creature IDs (UUID strings)
    /// - `"positions"`: `PackedVector3Array` of interpolated positions
    /// - `"relations"`: `PackedByteArray` — 0=friendly, 1=hostile, 2=neutral
    ///
    /// Uses `SimState::player_relation()` which delegates to the centralized
    /// `diplomatic_relation()` logic (civ relationships + engagement initiative).
    /// Used by `minimap.gd` for faction-colored creature dots.
    #[func]
    fn get_all_creature_positions_with_relations(&self, render_tick: f64) -> VarDictionary {
        let mut dict = VarDictionary::new();
        let mut ids = VarArray::new();
        let mut positions = PackedVector3Array::new();
        let mut relations: Vec<u8> = Vec::new();
        let Some(sim) = &self.session.sim else {
            dict.set("ids", ids);
            dict.set("positions", positions);
            dict.set("relations", PackedByteArray::new());
            return dict;
        };
        for creature in sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.vital_status != VitalStatus::Dead)
        {
            let ma = sim.db.move_actions.get(&creature.id);
            let (x, y, z) = creature.interpolated_position(render_tick, ma.as_ref());
            ids.push(&GString::from(creature.id.0.to_string().as_str()).to_variant());
            positions.push(Vector3::new(x, y, z));
            let rel = sim.player_relation(creature.id);
            relations.push(match rel {
                DiplomaticRelation::Friendly => 0,
                DiplomaticRelation::Hostile => 1,
                DiplomaticRelation::Neutral => 2,
            });
        }
        dict.set("ids", ids);
        dict.set("positions", positions);
        dict.set("relations", PackedByteArray::from(relations.as_slice()));
        dict
    }

    /// Return the diplomatic relation between the player's civilization and
    /// a single creature, identified by UUID string.
    ///
    /// Returns `"friendly"`, `"hostile"`, or `"neutral"`.
    /// Uses `SimState::player_relation()`.
    /// Used by `selection_highlight.gd` for ring color-coding.
    #[func]
    fn get_creature_player_relation(&self, creature_uuid: GString) -> GString {
        let Some(sim) = &self.session.sim else {
            return GString::from("neutral");
        };
        let Some(cid) = parse_creature_id(&creature_uuid.to_string()) else {
            return GString::from("neutral");
        };
        let rel = sim.player_relation(cid);
        GString::from(match rel {
            DiplomaticRelation::Friendly => "friendly",
            DiplomaticRelation::Hostile => "hostile",
            DiplomaticRelation::Neutral => "neutral",
        })
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
    /// preserved (or cleared if there was none). Does NOT start a relay —
    /// call `start_singleplayer_relay()` afterward for the real game path.
    /// Test paths that use `step_to_tick` / `step_exactly` should NOT call
    /// `start_singleplayer_relay`.
    #[func]
    fn load_game_json(&mut self, json: GString) -> bool {
        // Tear down any existing relay before loading.
        self.shutdown_relay();

        let json_str = json.to_string();
        let events = self
            .session
            .process(SessionMessage::LoadSim { json: json_str });
        let loaded = events
            .iter()
            .any(|e| matches!(e, elven_canopy_sim::session::SessionEvent::SimLoaded));
        if loaded {
            if let Some(sim) = &self.session.sim {
                self.active_zone_id = sim.home_zone_id();
                godot_print!(
                    "SimBridge: loaded save (tick={}, creatures={})",
                    sim.tick,
                    sim.db.creatures.len()
                );
            }
            self.rebuild_mesh_cache();
            // Clear sprite caches so loaded creatures get fresh textures
            // from their trait data rather than stale entries from the
            // previous session.
            self.creature_sprite_cache.clear();
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

    /// Start a localhost relay for the currently loaded sim. Call this after
    /// `load_game_json()` in the real game path. The relay begins flushing
    /// turns from the loaded sim's current tick.
    ///
    /// Do NOT call this in test paths that use `step_to_tick()` /
    /// `step_exactly()` — those require direct session access without a relay.
    #[func]
    fn start_singleplayer_relay(&mut self) {
        let Some(sim) = &self.session.sim else {
            godot_error!("SimBridge: cannot start relay — no sim loaded");
            return;
        };
        let loaded_tick = sim.tick;
        let ticks_per_turn = SP_BASE_TICKS_PER_TURN;
        let tick_duration_ms = sim.config.tick_duration_ms as u64;
        let turn_cadence_ms = u64::from(ticks_per_turn) * tick_duration_ms;
        let loaded_speed = self.session.current_speed();
        let was_paused = self.session.is_paused();

        let player_name = self
            .session
            .players
            .get(&self.local_player_id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Player".to_string());

        // Stash the sim so we can move it to the new multiplayer session.
        let loaded_sim = self.session.sim.take();

        if let Err(e) = self.start_local_relay_and_connect(LocalRelayOpts {
            port: 0,
            session_name: "singleplayer",
            player_name: &player_name,
            password: None,
            max_players: 1,
            ticks_per_turn,
            turn_cadence_ms,
        }) {
            godot_error!("SimBridge: {e}");
            self.session.sim = loaded_sim;
            return;
        }

        // Restore player name and sim state on the new session.
        if let Some(slot) = self.session.players.get_mut(&self.local_player_id) {
            slot.name = player_name;
        }
        self.session.sim = loaded_sim;
        self.session.process(SessionMessage::SetSpeed {
            speed: loaded_speed,
        });
        if was_paused {
            self.session.process(SessionMessage::Pause {
                by: self.local_player_id,
            });
        }

        // Tell the relay about the loaded speed (the session was created at
        // base ticks_per_turn; if the save was at Fast/VeryFast, the relay
        // needs to know).
        if loaded_speed != SessionSpeed::Normal {
            let tpt = self.base_ticks_per_turn * speed_multiplier_int(loaded_speed);
            if let Some(client) = &mut self.net_client {
                let _ = client.send_set_speed(tpt);
            }
        }

        // Tell the relay to start flushing turns from the loaded tick.
        if let Some(client) = &mut self.net_client {
            if let Err(e) = client.send_resume_session(loaded_tick) {
                godot_error!("SimBridge: send_resume_session failed: {e}");
                // Relay won't flush turns — shut it down to avoid a frozen
                // game with no feedback.
                self.shutdown_relay();
                // Mesh cache is still valid from the loaded sim.
                self.rebuild_mesh_cache();
                return;
            }
            if was_paused {
                let _ = client.send_pause();
            }
        }

        self.rebuild_mesh_cache();
    }

    /// Shut down the current relay (if any) and clear network state.
    fn shutdown_relay(&mut self) {
        if let Some(mut client) = self.net_client.take() {
            client.disconnect();
        }
        if let Some(handle) = self.relay_handle.take() {
            handle.stop();
        }
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
                item_dict.set("item_stack_id", stack.id.0 as i64);
                item_dict.set(
                    "kind",
                    GString::from(sim.item_display_name(&stack).as_str()),
                );
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
            item_dict.set("item_stack_id", stack.id.0 as i64);
            item_dict.set(
                "kind",
                GString::from(sim.item_display_name(&stack).as_str()),
            );
            item_dict.set("quantity", stack.quantity as i64);
            inv_arr.push(&item_dict.to_variant());
        }
        dict.set("inventory", inv_arr);
        dict
    }

    /// Return detailed information about a single item stack.
    ///
    /// Returns a `VarDictionary` with: `display_name` (String), `kind` (String,
    /// raw item type), `material` (String or empty), `quality` (i64),
    /// `quality_label` (String or empty), `current_hp` (i64), `max_hp` (i64),
    /// `condition` (String: "", "worn", or "damaged"), `equipped_slot` (String
    /// or empty), `owner_id` (String creature UUID or empty),
    /// `owner_name` (String or empty), `owner_x/y/z` (i32, only if owner exists),
    /// `dye_color` (String or empty), `quantity` (i64).
    /// Returns an empty dictionary if the stack does not exist.
    /// Used by `item_detail_panel.gd` for the item detail popup.
    #[func]
    fn get_item_detail(&self, item_stack_id: i64) -> VarDictionary {
        let Some(sim) = &self.session.sim else {
            return VarDictionary::new();
        };
        let id = ItemStackId(item_stack_id as u64);
        let Some(stack) = sim.db.item_stacks.get(&id) else {
            return VarDictionary::new();
        };

        let mut dict = VarDictionary::new();
        dict.set(
            "display_name",
            GString::from(sim.item_display_name(&stack).as_str()),
        );
        dict.set("kind", GString::from(stack.kind.display_name()));

        // Material — resolve fruit species name if applicable.
        let material_str = match stack.material {
            Some(elven_canopy_sim::inventory::Material::FruitSpecies(fs_id)) => sim
                .db
                .fruit_species
                .get(&fs_id)
                .map(|s| s.vaelith_name.clone())
                .unwrap_or_default(),
            Some(m) => m.display_name().to_string(),
            None => String::new(),
        };
        dict.set("material", GString::from(material_str.as_str()));

        dict.set("quality", stack.quality as i64);
        let qlabel = elven_canopy_sim::inventory::quality_label(stack.quality).unwrap_or("");
        dict.set("quality_label", GString::from(qlabel));

        dict.set("current_hp", stack.current_hp as i64);
        dict.set("max_hp", stack.max_hp as i64);

        let condition = elven_canopy_sim::sim::SimState::condition_label(
            stack.current_hp,
            stack.max_hp,
            sim.config.durability_worn_pct,
            sim.config.durability_damaged_pct,
        )
        .unwrap_or("");
        dict.set("condition", GString::from(condition));

        let slot_str = stack.equipped_slot.map(|s| s.display_name()).unwrap_or("");
        dict.set("equipped_slot", GString::from(slot_str));

        // Owner — resolve creature name and position.
        if let Some(owner_id) = stack.owner
            && let Some(creature) = sim.db.creatures.get(&owner_id)
        {
            dict.set("owner_id", GString::from(owner_id.0.to_string().as_str()));
            dict.set("owner_name", GString::from(creature.name.as_str()));
            dict.set("owner_x", creature.position.min.x);
            dict.set("owner_y", creature.position.min.y);
            dict.set("owner_z", creature.position.min.z);
        } else {
            dict.set("owner_id", GString::from(""));
            dict.set("owner_name", GString::from(""));
        }

        let dye_str = stack
            .dye_color
            .map(|c| c.display_name().to_string())
            .unwrap_or_default();
        dict.set("dye_color", GString::from(dye_str.as_str()));

        dict.set("quantity", stack.quantity as i64);

        // Reservation — if the item is reserved by a task, include task info.
        if let Some(task_id) = stack.reserved_by
            && let Some(task) = sim.db.tasks.get(&task_id)
        {
            dict.set(
                "reserved_task_kind",
                GString::from(task.kind_tag.display_name()),
            );
            let state_str = match task.state {
                elven_canopy_sim::task::TaskState::Available => "Available",
                elven_canopy_sim::task::TaskState::InProgress => "In Progress",
                elven_canopy_sim::task::TaskState::Complete => "Complete",
            };
            dict.set("reserved_task_state", GString::from(state_str));
            // Include the name of the creature assigned to the task, if any.
            if let Some(assignee) = sim
                .db
                .creatures
                .by_current_task(&Some(task_id), elven_canopy_sim::tabulosity::QueryOpts::ASC)
                .first()
            {
                dict.set(
                    "reserved_task_assignee",
                    GString::from(assignee.name.as_str()),
                );
            }
        }

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

    /// Return the highest notification ID currently in the sim database,
    /// or -1 if no notifications exist.
    ///
    /// Used by `main.gd` after loading a save to initialize
    /// `_last_notification_id` so that historical notifications are not
    /// replayed as toasts.  Returns -1 (not 0) for empty because
    /// notification IDs start at 0 and `get_notifications_after` uses a
    /// `<= after_id` filter — returning 0 would skip the first notification.
    #[func]
    fn get_max_notification_id(&self) -> i64 {
        let Some(sim) = &self.session.sim else {
            return -1;
        };
        sim.db
            .notifications
            .iter_all()
            .map(|n| n.id.0 as i64)
            .max()
            .unwrap_or(-1)
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

    /// Trigger a raid from a random hostile civilization (debug).
    #[func]
    fn trigger_raid(&mut self) {
        self.apply_or_send(SimAction::TriggerRaid);
    }

    // -------------------------------------------------------------------
    // HP / damage / death
    // -------------------------------------------------------------------

    /// Kill a creature immediately (debug/testing). Triggers full death
    /// handling: task interruption, inventory drop, event emission, etc.
    #[func]
    fn debug_kill_creature(&mut self, creature_uuid: GString) {
        let Some(creature_id) = parse_creature_id(&creature_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::DebugKillCreature { creature_id });
    }

    /// Deal damage to a creature. If HP reaches 0, the creature dies.
    #[func]
    fn damage_creature(&mut self, creature_uuid: GString, amount: i64) {
        let Some(creature_id) = parse_creature_id(&creature_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::DamageCreature {
            creature_id,
            amount,
        });
    }

    /// Heal a creature (no effect on dead creatures).
    #[func]
    fn heal_creature(&mut self, creature_uuid: GString, amount: i64) {
        let Some(creature_id) = parse_creature_id(&creature_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::HealCreature {
            creature_id,
            amount,
        });
    }

    // -------------------------------------------------------------------
    // Military groups
    // -------------------------------------------------------------------

    /// Returns an array of dicts describing the player civ's military groups.
    ///
    /// Each dict: `{ id: int, name: String, is_civilian: bool,
    /// weapon_preference: String, ammo_exhausted: String,
    /// initiative: String, disengage_threshold_pct: int, member_count: int }`.
    /// The civilian group's member_count is the computed leftover count
    /// (total alive civ creatures minus those explicitly assigned).
    /// All counts only include `vital_status = Alive` creatures.
    #[func]
    fn get_military_groups(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let Some(player_civ) = sim.player_civ_id else {
            return VarArray::new();
        };

        let groups = sim
            .db
            .military_groups
            .by_civ_id(&player_civ, elven_canopy_sim::tabulosity::QueryOpts::ASC);

        // Count all alive civ creatures and those explicitly assigned.
        let alive_civ_creatures: usize = sim
            .db
            .creatures
            .by_civ_id(
                &Some(player_civ),
                elven_canopy_sim::tabulosity::QueryOpts::ASC,
            )
            .iter()
            .filter(|c| c.vital_status == VitalStatus::Alive)
            .count();
        let explicitly_assigned: usize = sim
            .db
            .creatures
            .by_civ_id(
                &Some(player_civ),
                elven_canopy_sim::tabulosity::QueryOpts::ASC,
            )
            .iter()
            .filter(|c| c.vital_status == VitalStatus::Alive && c.military_group.is_some())
            .count();

        let mut arr = VarArray::new();
        for g in &groups {
            let mut dict = VarDictionary::new();
            dict.set("id", g.id.0 as i64);
            dict.set("name", GString::from(g.name.as_str()));
            dict.set("is_civilian", g.is_default_civilian);
            // Expose engagement style fields as individual dict entries.
            use elven_canopy_sim::species::{
                AmmoExhaustedBehavior, EngagementInitiative, WeaponPreference,
            };
            let style = &g.engagement_style;
            dict.set(
                "weapon_preference",
                GString::from(match style.weapon_preference {
                    WeaponPreference::PreferRanged => "PreferRanged",
                    WeaponPreference::PreferMelee => "PreferMelee",
                }),
            );
            dict.set(
                "ammo_exhausted",
                GString::from(match style.ammo_exhausted {
                    AmmoExhaustedBehavior::SwitchToMelee => "SwitchToMelee",
                    AmmoExhaustedBehavior::Flee => "Flee",
                }),
            );
            dict.set(
                "initiative",
                GString::from(match style.initiative {
                    EngagementInitiative::Aggressive => "Aggressive",
                    EngagementInitiative::Defensive => "Defensive",
                    EngagementInitiative::Passive => "Passive",
                }),
            );
            dict.set(
                "disengage_threshold_pct",
                style.disengage_threshold_pct as i64,
            );

            let member_count = if g.is_default_civilian {
                // Implicit civilians = total alive civ - explicitly assigned.
                alive_civ_creatures.saturating_sub(explicitly_assigned)
            } else {
                sim.db
                    .creatures
                    .by_military_group(&Some(g.id), elven_canopy_sim::tabulosity::QueryOpts::ASC)
                    .iter()
                    .filter(|c| c.vital_status == VitalStatus::Alive)
                    .count()
            };
            dict.set("member_count", member_count as i64);

            // Equipment wants.
            let mut wants_arr = VarArray::new();
            for want in &g.equipment_wants {
                let mut want_dict = VarDictionary::new();
                want_dict.set("kind", GString::from(want.item_kind.display_name()));
                let filter_json = Self::serialize_material_filter(want.material_filter);
                let filter_str = filter_json.to_string();
                want_dict.set("material_filter", GString::from(&filter_str));
                let label = match want.material_filter {
                    elven_canopy_sim::inventory::MaterialFilter::Any => {
                        format!("Any {}", want.item_kind.display_name())
                    }
                    elven_canopy_sim::inventory::MaterialFilter::Specific(mat) => {
                        sim.material_item_display_name(want.item_kind, mat)
                    }
                    elven_canopy_sim::inventory::MaterialFilter::NonWood => {
                        format!("Non-wood {}", want.item_kind.display_name())
                    }
                };
                want_dict.set("label", GString::from(label.as_str()));
                want_dict.set("target_quantity", want.target_quantity as i64);
                wants_arr.push(&want_dict.to_variant());
            }
            dict.set("equipment_wants", wants_arr);

            arr.push(&dict.to_variant());
        }
        arr
    }

    /// Returns an array of dicts for living members of a specific military
    /// group. For the civilian group, returns creatures with
    /// `military_group = None` and `vital_status = Alive`.
    ///
    /// Each dict: `{ creature_id: String, name: String, species: String }`.
    #[func]
    fn get_military_group_members(&self, group_id: i64) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };

        let gid = elven_canopy_sim::types::MilitaryGroupId(group_id as u64);
        let group = sim.db.military_groups.get(&gid);

        let members: Vec<_> = match &group {
            Some(g) if g.is_default_civilian => {
                // Civilian group: creatures with military_group = None and matching civ_id.
                sim.db
                    .creatures
                    .by_civ_id(
                        &Some(g.civ_id),
                        elven_canopy_sim::tabulosity::QueryOpts::ASC,
                    )
                    .into_iter()
                    .filter(|c| c.vital_status == VitalStatus::Alive && c.military_group.is_none())
                    .collect()
            }
            Some(_) => {
                // Non-civilian group: creatures explicitly assigned.
                sim.db
                    .creatures
                    .by_military_group(&Some(gid), elven_canopy_sim::tabulosity::QueryOpts::ASC)
                    .into_iter()
                    .filter(|c| c.vital_status == VitalStatus::Alive)
                    .collect()
            }
            None => Vec::new(),
        };

        let mut arr = VarArray::new();
        for c in &members {
            let mut dict = VarDictionary::new();
            dict.set("creature_id", GString::from(c.id.0.to_string().as_str()));
            dict.set("name", GString::from(c.name.as_str()));
            let species_str = format!("{:?}", c.species);
            dict.set("species", GString::from(species_str.as_str()));
            arr.push(&dict.to_variant());
        }
        arr
    }

    /// Create a new military group for the player's civ.
    #[func]
    fn create_military_group(&mut self, name: GString) {
        self.apply_or_send(SimAction::CreateMilitaryGroup {
            name: name.to_string(),
        });
    }

    /// Delete a non-civilian military group. Members return to civilian.
    #[func]
    fn delete_military_group(&mut self, group_id: i64) {
        self.apply_or_send(SimAction::DeleteMilitaryGroup {
            group_id: elven_canopy_sim::types::MilitaryGroupId(group_id as u64),
        });
    }

    /// Reassign a creature to a military group.
    #[func]
    fn reassign_military_group(&mut self, creature_uuid: GString, group_id: i64) {
        let Some(creature_id) = parse_creature_id(&creature_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::ReassignMilitaryGroup {
            creature_id,
            group_id: Some(elven_canopy_sim::types::MilitaryGroupId(group_id as u64)),
        });
    }

    /// Reassign a creature back to civilian status.
    #[func]
    fn reassign_to_civilian(&mut self, creature_uuid: GString) {
        let Some(creature_id) = parse_creature_id(&creature_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::ReassignMilitaryGroup {
            creature_id,
            group_id: None,
        });
    }

    /// Rename a military group (including the civilian group).
    #[func]
    fn rename_military_group(&mut self, group_id: i64, name: GString) {
        self.apply_or_send(SimAction::RenameMilitaryGroup {
            group_id: elven_canopy_sim::types::MilitaryGroupId(group_id as u64),
            name: name.to_string(),
        });
    }

    /// Change a military group's engagement style.
    ///
    /// Parameters are passed as individual fields:
    /// - `weapon_preference`: "PreferRanged" or "PreferMelee"
    /// - `ammo_exhausted`: "SwitchToMelee" or "Flee"
    /// - `initiative`: "Aggressive", "Defensive", or "Passive"
    /// - `disengage_threshold_pct`: 0–100
    #[func]
    fn set_group_engagement_style(
        &mut self,
        group_id: i64,
        weapon_preference: GString,
        ammo_exhausted: GString,
        initiative: GString,
        disengage_threshold_pct: i64,
    ) {
        use elven_canopy_sim::species::{
            AmmoExhaustedBehavior, EngagementInitiative, EngagementStyle, WeaponPreference,
        };
        let wp = match weapon_preference.to_string().as_str() {
            "PreferRanged" => WeaponPreference::PreferRanged,
            "PreferMelee" => WeaponPreference::PreferMelee,
            _ => return,
        };
        let ae = match ammo_exhausted.to_string().as_str() {
            "SwitchToMelee" => AmmoExhaustedBehavior::SwitchToMelee,
            "Flee" => AmmoExhaustedBehavior::Flee,
            _ => return,
        };
        let init = match initiative.to_string().as_str() {
            "Aggressive" => EngagementInitiative::Aggressive,
            "Defensive" => EngagementInitiative::Defensive,
            "Passive" => EngagementInitiative::Passive,
            _ => return,
        };
        let pct = disengage_threshold_pct.clamp(0, 100) as u8;
        self.apply_or_send(SimAction::SetGroupEngagementStyle {
            group_id: elven_canopy_sim::types::MilitaryGroupId(group_id as u64),
            engagement_style: EngagementStyle {
                weapon_preference: wp,
                ammo_exhausted: ae,
                initiative: init,
                disengage_threshold_pct: pct,
            },
        });
    }

    /// Set a military group's equipment wants. Expects a JSON string like:
    /// `[{"kind": "Bow", "material_filter": "Any", "quantity": 1}]`
    ///
    /// Uses the same material filter encoding as `set_logistics_wants`.
    #[func]
    fn set_group_equipment_wants(&mut self, group_id: i64, wants_json: GString) {
        let json_str = wants_json.to_string();
        let parsed: Vec<serde_json::Value> = match serde_json::from_str(&json_str) {
            Ok(v) => v,
            Err(e) => {
                godot_error!("SimBridge: failed to parse equipment wants JSON: {e}");
                return;
            }
        };
        let mut wants = Vec::new();
        for entry in &parsed {
            let kind_str = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let kind = match Self::parse_item_kind(kind_str) {
                Some(k) => k,
                None => {
                    godot_error!("SimBridge: unknown item kind in equipment wants: '{kind_str}'");
                    continue;
                }
            };
            let material_filter = Self::parse_material_filter(entry.get("material_filter"));
            let quantity = entry.get("quantity").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if quantity > 0 {
                wants.push(elven_canopy_sim::building::LogisticsWant {
                    item_kind: kind,
                    material_filter,
                    target_quantity: quantity,
                });
            }
        }
        self.apply_or_send(SimAction::SetGroupEquipmentWants {
            group_id: elven_canopy_sim::types::MilitaryGroupId(group_id as u64),
            wants,
        });
    }

    // -------------------------------------------------------------------
    // Path assignment (F-path-core)
    // -------------------------------------------------------------------

    /// Assign a path to a creature. `path_id` is one of: "Outcast",
    /// "Warrior", "Scout".
    #[func]
    fn assign_path(&mut self, creature_uuid: GString, path_id_str: GString) {
        let Some(creature_id) = parse_creature_id(&creature_uuid.to_string()) else {
            return;
        };
        let path_id = match path_id_str.to_string().as_str() {
            "Outcast" => elven_canopy_sim::types::PathId::Outcast,
            "Warrior" => elven_canopy_sim::types::PathId::Warrior,
            "Scout" => elven_canopy_sim::types::PathId::Scout,
            other => {
                godot_error!("SimBridge: unknown path_id '{other}'");
                return;
            }
        };
        self.apply_or_send(SimAction::AssignPath {
            creature_id,
            path_id,
        });
    }

    /// Get the list of all available path IDs (for UI dropdowns).
    #[func]
    fn get_path_ids(&self) -> VarArray {
        let mut arr = VarArray::new();
        for path_id in elven_canopy_sim::types::PathId::ALL {
            arr.push(&GString::from(format!("{path_id:?}").as_str()).to_variant());
        }
        arr
    }

    /// Get the display name for a path ID string.
    #[func]
    fn get_path_display_name(&self, path_id_str: GString) -> GString {
        match path_id_str.to_string().as_str() {
            "Outcast" => GString::from(elven_canopy_sim::types::PathId::Outcast.display_name()),
            "Warrior" => GString::from(elven_canopy_sim::types::PathId::Warrior.display_name()),
            "Scout" => GString::from(elven_canopy_sim::types::PathId::Scout.display_name()),
            _ => GString::from(""),
        }
    }

    // -------------------------------------------------------------------
    // Taming (F-taming)
    // -------------------------------------------------------------------

    /// Designate a wild creature for taming. Creates a tame designation and
    /// an open Tame task for any available Scout-path elf to claim.
    #[func]
    fn designate_tame(&mut self, target_uuid: GString) {
        let Some(target_id) = parse_creature_id(&target_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::DesignateTame { target_id });
    }

    /// Cancel a tame designation. Removes the designation and cancels any
    /// in-progress taming task on that creature.
    #[func]
    fn cancel_tame_designation(&mut self, target_uuid: GString) {
        let Some(target_id) = parse_creature_id(&target_uuid.to_string()) else {
            return;
        };
        self.apply_or_send(SimAction::CancelTameDesignation { target_id });
    }

    // -------------------------------------------------------------------
    // Music composition
    // -------------------------------------------------------------------

    /// Start background generation for any Pending compositions in the sim.
    ///
    /// Called each frame from GDScript. Checks the sim's MusicComposition
    /// table for rows with status=Pending that don't already have a
    /// background thread running, and spawns generation threads for them.
    #[func]
    fn poll_composition_starts(&mut self) {
        let Some(sim) = &self.session.sim else {
            return;
        };

        for comp in sim.db.music_compositions.iter_all() {
            if comp.status != elven_canopy_sim::db::CompositionStatus::Pending {
                continue;
            }
            let id_val = comp.id.0;
            if self.pending_compositions.contains_key(&id_val) {
                continue; // Already generating
            }

            let result: Arc<Mutex<Option<Vec<f32>>>> = Arc::new(Mutex::new(None));
            let result_clone = Arc::clone(&result);
            self.pending_compositions.insert(id_val, result);

            let params = elven_canopy_music::generate::GenerateParams {
                seed: comp.seed,
                sections: comp.sections as usize,
                mode_index: comp.mode_index as usize,
                brightness: comp.brightness,
                sa_iterations: comp.sa_iterations as usize,
                tempo_bpm: 72, // Placeholder; overridden below.
                max_beats: None,
                voices: Vec::new(), // All four SATB voices by default
            };
            let target_duration_ms = comp.target_duration_ms;

            std::thread::spawn(move || {
                let mut grid = elven_canopy_music::generate::generate_piece(&params);

                // Derive the exact BPM so the rendered PCM matches the build
                // duration. Formula: duration = total_beats * 30 / bpm, so
                // bpm = total_beats * 30 / target_secs.
                let target_secs = target_duration_ms as f32 / 1000.0;
                let exact_bpm = if target_secs > 0.0 {
                    (grid.num_beats as f32 * 30.0 / target_secs).round() as u16
                } else {
                    78 // Fallback: middle of range
                };
                // Clamp to a broad but reasonable range.
                grid.tempo_bpm = exact_bpm.clamp(45, 120);

                let pcm = elven_canopy_music::synth::render_grid_to_pcm(&grid);
                if let Ok(mut lock) = result_clone.lock() {
                    *lock = Some(pcm);
                }
            });
        }
    }

    /// Collect composition IDs that are ready to play.
    ///
    /// A composition is playable when both conditions are met:
    /// 1. The background thread has finished generating PCM data.
    /// 2. An elf has started building (build_started == true in SimDb).
    ///
    /// Returns a PackedInt64Array of CompositionId values. Also marks them
    /// as Ready in the sim database so they won't be re-polled.
    #[func]
    fn poll_ready_compositions(&mut self) -> PackedInt64Array {
        let mut ready_ids = PackedInt64Array::new();
        let mut newly_ready = Vec::new();

        // Check which compositions have finished generating.
        let mut generated = Vec::new();
        for (&id_val, result) in &self.pending_compositions {
            let Ok(lock) = result.lock() else { continue };
            if lock.is_some() {
                generated.push(id_val);
            }
        }

        // Only report compositions where building has actually started.
        if let Some(sim) = &self.session.sim {
            for id_val in generated {
                let comp_id = elven_canopy_sim::types::CompositionId(id_val);
                let started = sim
                    .db
                    .music_compositions
                    .get(&comp_id)
                    .is_some_and(|c| c.build_started);
                if started {
                    ready_ids.push(id_val as i64);
                    newly_ready.push(id_val);
                }
            }
        }

        // Mark as Ready in the sim DB
        if let Some(sim) = &mut self.session.sim {
            for &id_val in &newly_ready {
                let comp_id = elven_canopy_sim::types::CompositionId(id_val);
                let _ = sim
                    .db
                    .music_compositions
                    .modify_unchecked(&comp_id, |comp| {
                        comp.status = elven_canopy_sim::db::CompositionStatus::Ready;
                    });
            }
        }

        ready_ids
    }

    /// Get the PCM audio data for a completed composition.
    ///
    /// Returns a PackedFloat32Array of mono samples at 44100 Hz.
    /// Returns empty if the composition is not ready or not found.
    #[func]
    fn get_composition_pcm(&self, composition_id: i64) -> PackedFloat32Array {
        let id_val = composition_id as u64;
        let Some(result) = self.pending_compositions.get(&id_val) else {
            return PackedFloat32Array::new();
        };
        let Ok(lock) = result.lock() else {
            return PackedFloat32Array::new();
        };
        match lock.as_ref() {
            Some(pcm) => {
                let mut arr = PackedFloat32Array::new();
                arr.resize(pcm.len());
                for (i, &sample) in pcm.iter().enumerate() {
                    arr[i] = sample;
                }
                arr
            }
            None => PackedFloat32Array::new(),
        }
    }

    /// Clean up composition data for a finished/cancelled build.
    ///
    /// Removes the cached PCM from memory. Call when a build completes
    /// or is cancelled and the audio should stop.
    #[func]
    fn drop_composition(&mut self, composition_id: i64) {
        self.pending_compositions.remove(&(composition_id as u64));
    }

    /// Check which playing compositions should stop because their build
    /// finished or was cancelled.
    ///
    /// Takes a PackedInt64Array of currently-playing composition IDs and
    /// returns the subset whose associated blueprint is Complete, cancelled
    /// (removed), or otherwise no longer actively building.
    #[func]
    fn poll_finished_compositions(&self, active_ids: PackedInt64Array) -> PackedInt64Array {
        let mut finished = PackedInt64Array::new();
        let Some(sim) = &self.session.sim else {
            return finished;
        };
        for i in 0..active_ids.len() {
            let id_val = active_ids[i] as u64;
            let comp_id = elven_canopy_sim::types::CompositionId(id_val);

            // Check blueprints first (construction music).
            let bp = sim
                .db
                .blueprints
                .iter_all()
                .find(|b| b.composition_id == Some(comp_id));
            if let Some(b) = bp {
                if b.state == elven_canopy_sim::blueprint::BlueprintState::Complete {
                    finished.push(id_val as i64);
                }
                continue;
            }

            // Check dance activities (dance music). The composition is
            // "finished" when its owning activity is gone (completed or
            // cancelled — both delete the ActivityDanceData row).
            let dance_owns = sim
                .db
                .activity_dance_data
                .iter_all()
                .any(|d| d.composition_id == Some(comp_id));
            if !dance_owns {
                // Neither blueprint nor dance owns it — orphaned, stop.
                finished.push(id_val as i64);
            }
            // Dance still active — keep playing.
        }
        finished
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
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        zone.in_bounds(coord)
            && zone.get(coord) == VoxelType::Air
            && zone.has_solid_face_neighbor(coord)
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
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        if !zone.in_bounds(coord) || zone.get(coord) != VoxelType::Air {
            return false;
        }
        // F-no-bp-overlap: also reject if this voxel is claimed by an
        // existing designated blueprint.
        let overlay = sim.blueprint_overlay();
        !overlay.voxels.contains_key(&coord)
    }

    /// Check whether a single voxel has at least one face-adjacent solid
    /// voxel. Used alongside `validate_build_air` for multi-voxel rectangle
    /// validation.
    #[func]
    fn has_solid_neighbor(&self, x: i32, y: i32, z: i32) -> bool {
        let Some(sim) = &self.session.sim else {
            return false;
        };
        sim.voxel_zone(self.active_zone_id)
            .unwrap()
            .has_solid_face_neighbor(VoxelCoord::new(x, y, z))
    }

    /// Designate a single-voxel platform blueprint at the given position.
    ///
    /// Buffers the command via `apply_build_action` (executes on the next
    /// `frame_update`). Always returns empty — validation feedback is
    /// provided by the `validate_*_preview()` methods before placement.
    #[func]
    fn designate_build(&mut self, x: i32, y: i32, z: i32) -> GString {
        self.apply_build_action(SimAction::DesignateBuild {
            zone_id: self.active_zone_id,
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
            zone_id: self.active_zone_id,
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
            zone_id: self.active_zone_id,
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
            zone_id: self.active_zone_id,
            voxels,
            priority: Priority::Normal,
        })
    }

    /// Designate a support strut between two endpoints.
    ///
    /// Computes the 6-connected voxel line between the endpoints and sends
    /// a `DesignateBuild` command with `BuildType::Strut`. Returns a
    /// validation message (empty = success).
    #[func]
    fn designate_strut(&mut self, ax: i32, ay: i32, az: i32, bx: i32, by: i32, bz: i32) -> GString {
        let endpoint_a = VoxelCoord::new(ax, ay, az);
        let endpoint_b = VoxelCoord::new(bx, by, bz);
        let voxels = endpoint_a.line_to(endpoint_b);
        self.apply_build_action(SimAction::DesignateBuild {
            zone_id: self.active_zone_id,
            build_type: BuildType::Strut,
            voxels,
            priority: Priority::Normal,
        })
    }

    /// Preview-validate a rectangular carve placement.
    ///
    /// **Blueprint-aware:** Treats designated (not yet built) blueprints as
    /// their target voxel types for carvability and structural checks.
    ///
    /// Counts carvable solid voxels (above bedrock, considering overlay)
    /// in the region. Returns a `VarDictionary` with `"tier"` and
    /// `"message"` keys.
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
        let overlay = sim.blueprint_overlay();
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(zone, coord) };
        let w = width.max(1);
        let d = depth.max(1);
        let h = height.max(1);

        // Bounds check.
        for dy in 0..h {
            for dx in 0..w {
                for dz in 0..d {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    if !zone.in_bounds(coord) {
                        return Self::preview_result("Blocked", "Carve position is out of bounds.");
                    }
                }
            }
        }

        // Collect carvable coords: solid, above bedrock (y > 0), and not
        // already claimed by an existing blueprint (F-no-bp-overlap).
        // Must match the filter in `designate_carve()` (construction.rs).
        let mut carve_coords = Vec::new();
        for dy in 0..h {
            for dx in 0..w {
                for dz in 0..d {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    if overlay.voxels.contains_key(&coord) {
                        continue;
                    }
                    let vt = effective_type(coord);
                    if vt.is_solid() && coord.y > 0 {
                        carve_coords.push(coord);
                    }
                }
            }
        }

        if carve_coords.is_empty() {
            return Self::preview_result("Blocked", "Nothing to carve.");
        }

        let struts: Vec<_> = sim.db.struts.iter_all().cloned().collect();
        let validation = structural::validate_carve_fast(
            zone,
            &zone.face_data,
            &carve_coords,
            &sim.config,
            &overlay,
            &struts,
        );
        Self::preview_result_from_tier(validation.tier, &validation.message)
    }

    /// Preview-validate a strut placement between two endpoints.
    ///
    /// **Blueprint-aware:** Treats designated (not yet built) blueprints as
    /// their target voxel types for adjacency and structural checks.
    ///
    /// Computes the 6-connected voxel line between the endpoints, checks
    /// replacement rules, adjacency, overlap, bounds, and structural
    /// integrity. Returns a `VarDictionary` with `"tier"`, `"message"`, and
    /// `"voxels"` (PackedInt32Array of x,y,z triples for each voxel in the
    /// line).
    #[func]
    fn validate_strut_preview(
        &self,
        ax: i32,
        ay: i32,
        az: i32,
        bx: i32,
        by: i32,
        bz: i32,
    ) -> VarDictionary {
        let Some(sim) = &self.session.sim else {
            return Self::preview_result("Blocked", "Simulation not initialized.");
        };
        let endpoint_a = VoxelCoord::new(ax, ay, az);
        let endpoint_b = VoxelCoord::new(bx, by, bz);

        // Compute the voxel line.
        let line = endpoint_a.line_to(endpoint_b);

        // Pack voxel coordinates into the result regardless of validation.
        let mut voxel_array = PackedInt32Array::new();
        for &coord in &line {
            voxel_array.push(coord.x);
            voxel_array.push(coord.y);
            voxel_array.push(coord.z);
        }

        // Minimum length check.
        if line.len() < 2 {
            let mut dict = Self::preview_result("Blocked", "Strut must be at least 2 voxels.");
            dict.set("voxels", voxel_array);
            return dict;
        }

        let overlay = sim.blueprint_overlay();
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(zone, coord) };

        // Bounds check.
        for &coord in &line {
            if !zone.in_bounds(coord) {
                let mut dict = Self::preview_result("Blocked", "Strut extends out of bounds.");
                dict.set("voxels", voxel_array);
                return dict;
            }
        }

        // Blueprint overlap check (F-no-bp-overlap). Struts can overlap with
        // platform blueprints (they pass through flat structures).
        for &coord in &line {
            if let Some(&bp_vt) = overlay.voxels.get(&coord)
                && !matches!(bp_vt, VoxelType::GrownPlatform)
            {
                let mut dict =
                    Self::preview_result("Blocked", "Strut overlaps an existing blueprint.");
                dict.set("voxels", voxel_array);
                return dict;
            }
        }

        // Replacement validation. Struts can pass through natural materials,
        // platforms, and bridges, but not buildings, walls, or ladders.
        for &coord in &line {
            let vt = zone.get(coord);
            match vt {
                VoxelType::Air
                | VoxelType::Leaf
                | VoxelType::Fruit
                | VoxelType::Dirt
                | VoxelType::Trunk
                | VoxelType::Branch
                | VoxelType::Root
                | VoxelType::Strut
                | VoxelType::GrownPlatform => {}
                _ => {
                    let mut dict = Self::preview_result(
                        "Blocked",
                        "Strut cannot pass through buildings, walls, or ladders.",
                    );
                    dict.set("voxels", voxel_array);
                    return dict;
                }
            }
        }

        // Adjacency pre-check: at least one endpoint face-adjacent to solid.
        let has_adj = [endpoint_a, endpoint_b].iter().any(|&ep| {
            FaceDirection::ALL.iter().any(|&dir| {
                let (dx, dy, dz) = dir.to_offset();
                let neighbor = VoxelCoord::new(ep.x + dx, ep.y + dy, ep.z + dz);
                if line.contains(&neighbor) {
                    return false;
                }
                effective_type(neighbor).is_solid()
            })
        });
        if !has_adj {
            let mut dict = Self::preview_result(
                "Blocked",
                "At least one endpoint must be adjacent to a solid voxel.",
            );
            dict.set("voxels", voxel_array);
            return dict;
        }

        // Structural validation. Include the proposed strut in the list so
        // that rod springs are generated for it during weight-flow analysis.
        let mut struts: Vec<_> = sim.db.struts.iter_all().cloned().collect();
        struts.push(elven_canopy_sim::db::Strut {
            id: elven_canopy_sim::types::StrutId(u64::MAX),
            zone_id: elven_canopy_sim::types::ZoneId(0),
            endpoint_a,
            endpoint_b,
            blueprint_id: None,
            structure_id: None,
        });
        let validation = structural::validate_blueprint_fast(
            zone,
            &zone.face_data,
            &line,
            VoxelType::Strut,
            &BTreeMap::new(),
            &sim.config,
            &overlay,
            &struts,
        );

        let mut dict = Self::preview_result_from_tier(validation.tier, &validation.message);
        dict.set("voxels", voxel_array);
        dict
    }

    /// Validate whether a building can be placed at the given anchor.
    ///
    /// **Blueprint-aware:** Treats designated (not yet built) blueprints as
    /// their target voxel types. A designated platform counts as solid
    /// foundation; a designated carve counts as Air interior.
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
        let overlay = sim.blueprint_overlay();
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(zone, coord) };
        // Check foundation (considering blueprint overlay).
        for dx in 0..width {
            for dz in 0..depth {
                let coord = VoxelCoord::new(x + dx, y, z + dz);
                if !zone.in_bounds(coord) || !effective_type(coord).is_solid() {
                    return false;
                }
            }
        }
        // Check interior (considering blueprint overlay).
        for dy in 1..=height {
            for dx in 0..width {
                for dz in 0..depth {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    if !zone.in_bounds(coord) || effective_type(coord) != VoxelType::Air {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Preview-validate a rectangular platform placement.
    ///
    /// **Blueprint-aware:** Treats designated (not yet built) blueprints as
    /// their target voxel types for overlap, adjacency, and structural checks.
    ///
    /// Combines basic checks (in-bounds, Air/overlap-compatible, adjacency)
    /// with structural analysis via `validate_blueprint_fast()`. Returns a
    /// `VarDictionary` with keys:
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
        let overlay = sim.blueprint_overlay();
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(zone, coord) };
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
            if !zone.in_bounds(coord) {
                return Self::preview_result("Blocked", "Build position is out of bounds.");
            }
        }

        // F-no-bp-overlap: reject if any voxel belongs to an existing
        // designated blueprint.
        if voxels.iter().any(|v| overlay.voxels.contains_key(v)) {
            return Self::preview_result("Blocked", "Overlaps an existing blueprint designation.");
        }

        // Overlap-aware classification: Platform allows tree overlap.
        // Uses effective type (world + blueprint overlay) so existing
        // designated blueprints are treated as already built.
        let mut build_voxels = Vec::new();
        for &coord in &voxels {
            match effective_type(coord).classify_for_overlap() {
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
        // Check both actual world and blueprint overlay for adjacency.
        let any_adjacent = build_voxels.iter().any(|&coord| {
            zone.has_solid_face_neighbor(coord)
                || FaceDirection::ALL.iter().any(|&dir| {
                    let (dx, dy, dz) = dir.to_offset();
                    let neighbor = VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
                    overlay
                        .voxels
                        .get(&neighbor)
                        .is_some_and(|vt| vt.is_solid())
                })
        });
        if !any_adjacent {
            return Self::preview_result(
                "Blocked",
                "Must build adjacent to an existing structure.",
            );
        }

        // Structural validation on buildable voxels only.
        let struts: Vec<_> = sim.db.struts.iter_all().cloned().collect();
        let validation = structural::validate_blueprint_fast(
            zone,
            &zone.face_data,
            &build_voxels,
            BuildType::Platform.to_voxel_type(),
            &BTreeMap::new(),
            &sim.config,
            &overlay,
            &struts,
        );
        Self::preview_result_from_tier(validation.tier, &validation.message)
    }

    /// Preview-validate a building placement.
    ///
    /// **Blueprint-aware:** Treats designated (not yet built) blueprints as
    /// their target voxel types for foundation, interior, and structural checks.
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
        let overlay = sim.blueprint_overlay();
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(zone, coord) };

        if width < 3 || depth < 3 || height < 1 {
            return Self::preview_result("Blocked", "Building too small (min 3x3x1).");
        }

        let anchor = VoxelCoord::new(x, y, z);

        // F-no-bp-overlap: reject if any interior voxel belongs to an
        // existing designated blueprint. Checked early so the overlap
        // message takes priority over foundation/interior checks.
        for dy in 1..=height {
            for dx in 0..width {
                for dz in 0..depth {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    if overlay.voxels.contains_key(&coord) {
                        return Self::preview_result(
                            "Blocked",
                            "Overlaps an existing blueprint designation.",
                        );
                    }
                }
            }
        }

        // Validate foundation (all must be solid, considering blueprint overlay).
        for dx in 0..width {
            for dz in 0..depth {
                let coord = VoxelCoord::new(x + dx, y, z + dz);
                if !zone.in_bounds(coord) || !effective_type(coord).is_solid() {
                    return Self::preview_result("Blocked", "Foundation must be on solid ground.");
                }
            }
        }

        // Validate interior (all must be Air, considering blueprint overlay).
        for dy in 1..=height {
            for dx in 0..width {
                for dz in 0..depth {
                    let coord = VoxelCoord::new(x + dx, y + dy, z + dz);
                    if !zone.in_bounds(coord) || effective_type(coord) != VoxelType::Air {
                        return Self::preview_result("Blocked", "Building interior must be clear.");
                    }
                }
            }
        }

        // Compute face layout and run structural validation.
        let face_layout =
            elven_canopy_sim::building::compute_building_face_layout(anchor, width, depth, height);
        let voxels: Vec<VoxelCoord> = face_layout.keys().copied().collect();

        let struts: Vec<_> = sim.db.struts.iter_all().cloned().collect();
        let validation = structural::validate_blueprint_fast(
            zone,
            &zone.face_data,
            &voxels,
            VoxelType::BuildingInterior,
            &face_layout,
            &sim.config,
            &overlay,
            &struts,
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
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        for (coord, fd) in &zone.face_data {
            // Skip ladder voxels — they're rendered by ladder_renderer.gd.
            if zone.get(*coord).is_ladder() {
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
                    if sim.voxel_zone(self.active_zone_id).unwrap().get(*v) != target {
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
                    if sim.voxel_zone(self.active_zone_id).unwrap().get(*v) != VoxelType::Air {
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

    /// Unified per-frame entry point. Polls the relay for Turn messages
    /// and returns a fractional render tick for smooth interpolation.
    ///
    /// When connected to a relay (both SP and MP), polls for turns and
    /// interpolates `render_tick` up to `mp_ticks_per_turn` ahead of the last
    /// tick for smooth movement between turns. Both singleplayer and
    /// multiplayer use the same relay-driven tick pacing — turns arrive
    /// via `poll_network()` and the render tick is interpolated from
    /// wall-clock time since the last turn.
    #[func]
    fn frame_update(&mut self, delta: f64) -> f64 {
        self.update_elfcyclopedia();
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
        // Test mode (init_sim_test_config + step_to_tick) — no relay.
        self.session.current_tick() as f64
    }

    // ========================================================================
    // Chunk mesh methods
    // ========================================================================

    /// Internal: rebuild the mesh cache from the current sim state.
    /// Preserves the current Y cutoff setting across rebuilds.
    fn rebuild_mesh_cache(&mut self) {
        let Some(sim) = &self.session.sim else {
            return;
        };
        let old_cutoff = self.mesh_cache.as_ref().and_then(|c| c.y_cutoff());
        let old_config = self.mesh_cache.as_ref().map(|c| c.mesh_config);
        let mut cache = MeshCache::new();
        if let Some(cutoff) = old_cutoff {
            cache.set_y_cutoff(Some(cutoff));
        }
        if let Some(cfg) = old_config {
            cache.mesh_config = cfg;
        }
        cache.scan_nonempty_chunks(sim.voxel_zone(self.active_zone_id).unwrap());
        self.mesh_cache = Some(cache);
    }

    /// Build the world mesh cache from scratch. Call once after init_sim or
    /// load_game_json. Replaces any existing cache.
    #[func]
    fn build_world_mesh(&mut self) {
        let Some(sim) = &self.session.sim else {
            return;
        };
        let old_cutoff = self.mesh_cache.as_ref().and_then(|c| c.y_cutoff());
        let old_config = self.mesh_cache.as_ref().map(|c| c.mesh_config);
        let mut cache = MeshCache::new();
        if let Some(cutoff) = old_cutoff {
            cache.set_y_cutoff(Some(cutoff));
        }
        if let Some(cfg) = old_config {
            cache.mesh_config = cfg;
        }
        cache.scan_nonempty_chunks(sim.voxel_zone(self.active_zone_id).unwrap());
        godot_print!(
            "SimBridge: scanned world mesh ({} megachunks)",
            cache.megachunk_count()
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
        let dirty = sim
            .voxel_zone_mut(self.active_zone_id)
            .unwrap()
            .drain_dirty_voxels();
        if !dirty.is_empty() {
            cache.mark_dirty_voxels(&dirty);
        }
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        cache.update_dirty(zone, &zone.grassless) as i32
    }

    /// Set the Y cutoff for height hiding. Voxels at or above this Y level
    /// are treated as air during mesh generation. Pass -1 to disable (show all).
    /// Dirties affected chunks so the next `update_world_mesh()` rebuilds them.
    #[func]
    fn set_mesh_y_cutoff(&mut self, y: i32) {
        let Some(cache) = &mut self.mesh_cache else {
            return;
        };
        let cutoff = if y < 0 { None } else { Some(y) };
        cache.set_y_cutoff(cutoff);
    }

    /// Get the current Y cutoff. Returns -1 if disabled.
    #[func]
    fn get_mesh_y_cutoff(&self) -> i32 {
        self.mesh_cache
            .as_ref()
            .and_then(|c| c.y_cutoff())
            .unwrap_or(-1)
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

    /// Build a Godot ArrayMesh for the given chunk. Returns an ArrayMesh with
    /// exactly 3 surfaces: surface 0 = bark, surface 1 = ground, surface 2 =
    /// leaf. Empty surfaces get a minimal single-triangle degenerate surface
    /// so the surface index is always stable (bark=0, ground=1, leaf=2).
    /// Returns a default empty ArrayMesh if the chunk is not in the cache.
    #[func]
    fn build_chunk_array_mesh(
        &mut self,
        cx: i32,
        cy: i32,
        cz: i32,
    ) -> Gd<godot::classes::ArrayMesh> {
        use elven_canopy_graphics::mesh_gen::ChunkCoord;
        use std::time::Instant;

        let t = Instant::now();

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

        let us = t.elapsed().as_micros() as u32;
        if let Some(cache) = &mut self.mesh_cache {
            cache.perf.array_mesh_build_us.push(us);
        }

        array_mesh
    }

    /// Add a surface to the ArrayMesh. If the surface is empty, adds a
    /// degenerate zero-area triangle as a placeholder so the surface index
    /// stays stable.
    fn add_surface_or_placeholder(
        mesh: &mut Gd<godot::classes::ArrayMesh>,
        surface: &elven_canopy_graphics::mesh_gen::SurfaceMesh,
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
    /// to the `ArrayMesh`. Skips empty surfaces. UVs are only included if
    /// the surface has them (leaf surfaces have UVs; bark/ground do not —
    /// the tiling shader derives texture coordinates from world position).
    fn add_surface_to_array_mesh(
        mesh: &mut Gd<godot::classes::ArrayMesh>,
        surface: &elven_canopy_graphics::mesh_gen::SurfaceMesh,
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

        // UVs: only present for surfaces that need them (leaf).
        if !surface.uvs.is_empty() {
            let mut uvs = PackedVector2Array::new();
            for i in 0..vert_count {
                let base = i * 2;
                uvs.push(Vector2::new(surface.uvs[base], surface.uvs[base + 1]));
            }
            arrays.set(4, &Variant::from(uvs)); // ARRAY_TEX_UV
        }

        // 5-11: skip (nil)
        arrays.set(12, &Variant::from(indices)); // ARRAY_INDEX

        mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
    }

    /// Get the R8 texture data for a tiling cache.
    /// `material`: 0=bark, 1=ground. `cache_idx`: 0..2.
    /// Returns a flat PackedByteArray of all layers, each TILE_SIZE×TILE_SIZE
    /// bytes, suitable for building a Texture2DArray on the GDScript side.
    #[func]
    fn get_tiling_texture_data(&self, material: i32, cache_idx: i32) -> PackedByteArray {
        use elven_canopy_graphics::texture_gen::MaterialKind;

        let Some(cache) = &self.mesh_cache else {
            return PackedByteArray::new();
        };
        let mat = match material {
            0 => MaterialKind::Bark,
            1 => MaterialKind::Ground,
            _ => return PackedByteArray::new(),
        };
        let idx = cache_idx as usize;
        if idx >= elven_canopy_graphics::texture_gen::CACHE_COUNT {
            return PackedByteArray::new();
        }
        let data = cache.tiling_cache().texture_data(mat, idx);
        let mut arr = PackedByteArray::new();
        arr.resize(data.len());
        arr.as_mut_slice().copy_from_slice(data);
        arr
    }

    /// Get the number of Texture2DArray layers for a tiling cache.
    /// Layers are the same for bark and ground (same period structure).
    #[func]
    fn get_tiling_layer_count(&self, cache_idx: i32) -> i32 {
        use elven_canopy_graphics::texture_gen::MaterialKind;

        let Some(cache) = &self.mesh_cache else {
            return 0;
        };
        let idx = cache_idx as usize;
        if idx >= elven_canopy_graphics::texture_gen::CACHE_COUNT {
            return 0;
        }
        cache.tiling_cache().layer_count(MaterialKind::Bark, idx) as i32
    }

    /// Get the tiling periods [px, py, pz] for a cache as a Vector3i.
    #[func]
    fn get_tiling_periods(&self, cache_idx: i32) -> Vector3i {
        let Some(cache) = &self.mesh_cache else {
            return Vector3i::ZERO;
        };
        let idx = cache_idx as usize;
        if idx >= elven_canopy_graphics::texture_gen::CACHE_COUNT {
            return Vector3i::ZERO;
        }
        let p = cache.tiling_cache().periods(idx);
        Vector3i::new(p[0], p[1], p[2])
    }

    /// Get the tiles-per-axis-pair count for a cache (px * py * pz).
    #[func]
    fn get_tiling_tiles_per_axis_pair(&self, cache_idx: i32) -> i32 {
        let Some(cache) = &self.mesh_cache else {
            return 0;
        };
        let idx = cache_idx as usize;
        if idx >= elven_canopy_graphics::texture_gen::CACHE_COUNT {
            return 0;
        }
        cache.tiling_cache().tiles_per_axis_pair(idx) as i32
    }

    // ========================================================================
    // MegaChunk visibility methods
    // ========================================================================

    /// Set the draw distance in voxels (XZ). Chunks beyond this radius from
    /// the camera are hidden. Pass 0.0 for unlimited (show everything).
    #[func]
    fn set_draw_distance(&mut self, radius_voxels: f32) {
        if let Some(cache) = &mut self.mesh_cache {
            cache.set_draw_distance(radius_voxels);
        }
    }

    /// Toggle the smoothing pass and smooth normals on/off. When off,
    /// only chamfering is applied and flat per-face normals are used.
    /// Requires a full mesh rebuild to take effect.
    #[func]
    fn set_smoothing_enabled(&mut self, enabled: bool) {
        if let Some(cache) = &mut self.mesh_cache {
            cache.mesh_config.smoothing_enabled = enabled;
            cache.mesh_config.smooth_normals_enabled = enabled;
        }
        self.rebuild_mesh_cache();
    }

    /// Returns whether the smoothing pass is currently enabled.
    #[func]
    fn is_smoothing_enabled(&self) -> bool {
        self.mesh_cache
            .as_ref()
            .is_some_and(|c| c.mesh_config.smoothing_enabled)
    }

    /// Toggle QEM mesh decimation on/off. When enabled, coplanar triangles
    /// are collapsed to reduce triangle count. Requires a full mesh rebuild.
    #[func]
    fn set_decimation_enabled(&mut self, enabled: bool) {
        if let Some(cache) = &mut self.mesh_cache {
            cache.mesh_config.decimation_enabled = enabled;
        }
        self.rebuild_mesh_cache();
    }

    /// Returns whether mesh decimation is currently enabled.
    #[func]
    fn is_decimation_enabled(&self) -> bool {
        self.mesh_cache
            .as_ref()
            .is_some_and(|c| c.mesh_config.decimation_enabled)
    }

    /// Set the maximum quadric error threshold for decimation. Lower values
    /// preserve more detail. Near-zero is lossless for flat-shaded meshes.
    /// Requires a full mesh rebuild to take effect.
    #[func]
    fn set_decimation_max_error(&mut self, max_error: f32) {
        if let Some(cache) = &mut self.mesh_cache {
            cache.mesh_config.decimation_max_error = max_error;
        }
        self.rebuild_mesh_cache();
    }

    /// Enable or disable QEM-only mode (skip retri + collinear passes).
    /// Requires a full mesh rebuild to take effect.
    #[func]
    fn set_qem_only(&mut self, enabled: bool) {
        if let Some(cache) = &mut self.mesh_cache {
            cache.mesh_config.qem_only = enabled;
        }
        self.rebuild_mesh_cache();
    }

    /// Set the mesh memory budget in bytes. Cached chunk meshes beyond this
    /// budget are evicted LRU. Pass 0 for unlimited (no eviction).
    #[func]
    fn set_mesh_memory_budget(&mut self, bytes: i64) {
        if let Some(cache) = &mut self.mesh_cache {
            cache.set_memory_budget(bytes.max(0) as usize);
        }
    }

    /// Export the chunk mesh at the given world position as OBJ text.
    /// `with_decimation`: true for decimated, false for undecimated.
    /// Returns the OBJ text as a string (GDScript writes it to disk).
    #[func]
    fn export_chunk_obj(
        &mut self,
        world_x: f32,
        world_y: f32,
        world_z: f32,
        with_decimation: bool,
    ) -> GString {
        use elven_canopy_graphics::mesh_gen::{
            CHUNK_SIZE, ChunkCoord, chunk_mesh_to_obj, generate_chunk_mesh_with_decimation,
        };

        let Some(sim) = &self.session.sim else {
            return GString::new();
        };

        let cx = (world_x as i32).div_euclid(CHUNK_SIZE);
        let cy = (world_y as i32).div_euclid(CHUNK_SIZE);
        let cz = (world_z as i32).div_euclid(CHUNK_SIZE);
        let chunk = ChunkCoord::new(cx, cy, cz);

        let config = self
            .mesh_cache
            .as_ref()
            .map(|c| c.mesh_config)
            .unwrap_or_default();
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        let mesh = generate_chunk_mesh_with_decimation(
            zone,
            chunk,
            None,
            with_decimation,
            &zone.grassless,
            &config,
        );
        let obj = chunk_mesh_to_obj(&mesh);

        GString::from(obj.as_str())
    }

    /// Update chunk visibility based on camera position and frustum planes.
    /// `frustum` is a PackedFloat32Array of 24 floats: 6 planes × [nx,ny,nz,d].
    /// Returns the number of chunk meshes generated this frame.
    #[func]
    fn update_visibility(
        &mut self,
        cam_x: f32,
        cam_y: f32,
        cam_z: f32,
        frustum: PackedFloat32Array,
    ) -> i32 {
        let Some(sim) = &self.session.sim else {
            return 0;
        };
        let Some(cache) = &mut self.mesh_cache else {
            return 0;
        };
        let cam_pos = [cam_x, cam_y, cam_z];
        let planes: Vec<[f32; 4]> = frustum
            .as_slice()
            .chunks_exact(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect();
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        cache.update_visibility(zone, cam_pos, &planes, &zone.grassless) as i32
    }

    /// Return chunks that should become visible this frame (set .visible=true).
    /// Flat PackedInt32Array of (cx,cy,cz) triples.
    #[func]
    fn get_chunks_to_show(&self) -> PackedInt32Array {
        let Some(cache) = &self.mesh_cache else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for c in cache.chunks_to_show() {
            arr.push(c.cx);
            arr.push(c.cy);
            arr.push(c.cz);
        }
        arr
    }

    /// Return chunks that should become hidden this frame (set .visible=false).
    /// Flat PackedInt32Array of (cx,cy,cz) triples.
    #[func]
    fn get_chunks_to_hide(&self) -> PackedInt32Array {
        let Some(cache) = &self.mesh_cache else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for c in cache.chunks_to_hide() {
            arr.push(c.cx);
            arr.push(c.cy);
            arr.push(c.cz);
        }
        arr
    }

    /// Return freshly generated chunks (subset of chunks_to_show that need
    /// new MeshInstance3D creation). Flat PackedInt32Array of (cx,cy,cz) triples.
    #[func]
    fn get_chunks_generated(&self) -> PackedInt32Array {
        let Some(cache) = &self.mesh_cache else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for c in cache.chunks_generated() {
            arr.push(c.cx);
            arr.push(c.cy);
            arr.push(c.cz);
        }
        arr
    }

    /// Return chunks evicted from the LRU cache (free their MeshInstance3D).
    /// Flat PackedInt32Array of (cx,cy,cz) triples.
    #[func]
    fn get_chunks_evicted(&self) -> PackedInt32Array {
        let Some(cache) = &self.mesh_cache else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for c in cache.chunks_evicted() {
            arr.push(c.cx);
            arr.push(c.cy);
            arr.push(c.cz);
        }
        arr
    }

    /// Set the directional light direction for shadow-only culling.
    /// The direction is a unit vector pointing from the light source toward
    /// the scene. Pass (0,0,0) to disable shadow-only culling.
    #[func]
    fn set_light_direction(&mut self, dx: f32, dy: f32, dz: f32) {
        let Some(cache) = &mut self.mesh_cache else {
            return;
        };
        let len_sq = dx * dx + dy * dy + dz * dz;
        if len_sq < 0.001 {
            cache.set_light_direction(None);
        } else {
            let inv = 1.0 / len_sq.sqrt();
            cache.set_light_direction(Some([dx * inv, dy * inv, dz * inv]));
        }
    }

    /// Return chunks entering shadow-only state this frame.
    /// GDScript should set cast_shadow = SHADOW_CASTING_SETTING_SHADOWS_ONLY
    /// and visible = true. Flat PackedInt32Array of (cx,cy,cz) triples.
    #[func]
    fn get_chunks_to_shadow(&self) -> PackedInt32Array {
        let Some(cache) = &self.mesh_cache else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for c in cache.chunks_to_shadow() {
            arr.push(c.cx);
            arr.push(c.cy);
            arr.push(c.cz);
        }
        arr
    }

    /// Return chunks leaving shadow-only state to fully hidden this frame.
    /// GDScript should set visible = false. Flat PackedInt32Array of
    /// (cx,cy,cz) triples.
    #[func]
    fn get_chunks_from_shadow(&self) -> PackedInt32Array {
        let Some(cache) = &self.mesh_cache else {
            return PackedInt32Array::new();
        };
        let mut arr = PackedInt32Array::new();
        for c in cache.chunks_from_shadow() {
            arr.push(c.cx);
            arr.push(c.cy);
            arr.push(c.cz);
        }
        arr
    }

    /// Return total cached mesh memory in bytes.
    #[func]
    fn get_total_mesh_bytes(&self) -> i64 {
        self.mesh_cache
            .as_ref()
            .map_or(0, |c| c.total_cached_bytes() as i64)
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
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        for &(coord, voxel_type) in &zone.placed_voxels {
            if !voxel_type.is_ladder() {
                continue;
            }
            let face_dir = zone
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
                    if sim.voxel_zone(self.active_zone_id).unwrap().get(coord) == target {
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
    /// **Blueprint-aware:** Treats designated (not yet built) blueprints as
    /// their target voxel types for overlap, anchoring, and structural checks.
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
        let overlay = sim.blueprint_overlay();
        let zone = sim.voxel_zone(self.active_zone_id).unwrap();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(zone, coord) };
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

        // F-no-bp-overlap: reject if any ladder voxel belongs to an
        // existing designated blueprint.
        for dy in 0..height {
            let coord = VoxelCoord::new(x, y + dy, z);
            if overlay.voxels.contains_key(&coord) {
                return Self::preview_result(
                    "Blocked",
                    "Overlaps an existing blueprint designation.",
                );
            }
        }

        // Build column and validate using effective type (world + blueprint overlay).
        let mut build_voxels = Vec::new();
        for dy in 0..height {
            let coord = VoxelCoord::new(x, y + dy, z);
            if !zone.in_bounds(coord) {
                return Self::preview_result("Blocked", "Ladder extends out of bounds.");
            }
            match effective_type(coord).classify_for_overlap() {
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

        // Anchoring check (considers blueprint overlay for adjacent solidity).
        if kind == 0 {
            // Wood: any voxel's ladder face adjacent to solid.
            let any_anchored = build_voxels.iter().any(|&coord| {
                let neighbor = VoxelCoord::new(coord.x + odx, coord.y, coord.z + odz);
                effective_type(neighbor).is_solid()
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
            if !effective_type(top).is_solid() {
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
        let struts: Vec<_> = sim.db.struts.iter_all().cloned().collect();
        let validation = structural::validate_blueprint_fast(
            zone,
            &zone.face_data,
            &build_voxels,
            voxel_type,
            &BTreeMap::new(),
            &sim.config,
            &overlay,
            &struts,
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
            zone_id: self.active_zone_id,
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

    /// Start an embedded relay on localhost, create a session, and connect as
    /// the sole client. Used by both singleplayer (`init_sim`) and multiplayer
    /// (`host_game`). Populates `net_client`, `relay_handle`, `mp_ticks_per_turn`,
    /// `local_player_id`, and sets up a multiplayer-style `GameSession`.
    ///
    /// On failure the relay handle is stopped and an error string is returned.
    fn start_local_relay_and_connect(&mut self, opts: LocalRelayOpts<'_>) -> Result<(), String> {
        let LocalRelayOpts {
            port,
            session_name,
            player_name,
            password,
            max_players,
            ticks_per_turn,
            turn_cadence_ms,
        } = opts;
        let config = RelayConfig {
            port,
            bind_address: "127.0.0.1".into(),
            embedded: true,
            turn_cadence_ms,
        };

        let (handle, addr) =
            start_relay(config).map_err(|e| format!("failed to start relay: {e}"))?;

        // Small delay to let the listener thread start.
        std::thread::sleep(std::time::Duration::from_millis(50));

        let addr_str = format!("{addr}");
        let config_hash = fnv1a_hash("{}");

        // Connect, create the embedded session, and join it.
        let mut conn = match RelayConnection::connect(&addr_str) {
            Ok(c) => c,
            Err(e) => {
                handle.stop();
                return Err(format!("failed to connect to own relay: {e}"));
            }
        };
        if let Err(e) =
            conn.create_session(session_name, password.clone(), ticks_per_turn, max_players)
        {
            handle.stop();
            return Err(format!("failed to create session: {e}"));
        }
        match conn.join_session(
            SessionId(0),
            player_name,
            SIM_VERSION_HASH,
            config_hash,
            password,
            self.llm_model_loaded,
        ) {
            Ok((client, info)) => {
                let pid = SessionPlayerId(info.player_id.0);
                self.local_player_id = pid;
                self.session = GameSession::new_multiplayer(pid, pid);
                self.mp_ticks_per_turn = info.ticks_per_turn;
                self.base_ticks_per_turn = info.ticks_per_turn;
                self.net_client = Some(client);
                self.relay_handle = Some(handle);
                self.mp_time_since_turn = 0.0;
                // Note: caller sets is_multiplayer_mode as needed. SP relay
                // leaves it false; MP host_game sets it true.
                godot_print!(
                    "SimBridge: relay started on {addr_str} as player {}",
                    info.player_id.0
                );
                Ok(())
            }
            Err(e) => {
                handle.stop();
                Err(format!("failed to join own relay: {e}"))
            }
        }
    }

    /// Host a multiplayer game: start an embedded relay server, create a
    /// session with `SessionId(0)`, and connect as the first client.
    /// Returns true on success.
    #[func]
    fn host_game(
        &mut self,
        port: i32,
        session_name: GString,
        password: GString,
        max_players: i32,
        ticks_per_turn: i32,
        player_name: GString,
    ) -> bool {
        let pw = if password.to_string().is_empty() {
            None
        } else {
            Some(password.to_string())
        };
        match self.start_local_relay_and_connect(LocalRelayOpts {
            port: port as u16,
            session_name: &session_name.to_string(),
            player_name: &player_name.to_string(),
            password: pw,
            max_players: max_players as u32,
            ticks_per_turn: ticks_per_turn as u32,
            turn_cadence_ms: u64::from(ticks_per_turn as u32)
                * GameConfig::default().tick_duration_ms as u64,
        }) {
            Ok(()) => {
                self.is_multiplayer_mode = true;
                true
            }
            Err(e) => {
                godot_error!("SimBridge: {e}");
                false
            }
        }
    }

    /// Join a remote multiplayer game. `session_id` is the relay session to
    /// join — use 0 for embedded relays, or the ID from session browsing for
    /// dedicated relays. Returns true on success.
    #[func]
    fn join_game(
        &mut self,
        address: GString,
        player_name: GString,
        password: GString,
        session_id: i64,
    ) -> bool {
        let pw = if password.to_string().is_empty() {
            None
        } else {
            Some(password.to_string())
        };
        let config_hash = fnv1a_hash("{}");
        if session_id < 0 {
            godot_error!("SimBridge: invalid session_id {session_id}");
            return false;
        }
        let sid = SessionId(session_id as u64);

        let conn = match RelayConnection::connect(&address.to_string()) {
            Ok(c) => c,
            Err(e) => {
                godot_error!("SimBridge: join_game connect failed: {e}");
                return false;
            }
        };

        match conn.join_session(
            sid,
            &player_name.to_string(),
            SIM_VERSION_HASH,
            config_hash,
            pw,
            self.llm_model_loaded,
        ) {
            Ok((client, info)) => {
                let pid = SessionPlayerId(info.player_id.0);
                self.local_player_id = pid;
                // In join, host_id is unknown until we receive a GameStart;
                // set to 0 (relay assigns host).
                self.session = GameSession::new_multiplayer(pid, SessionPlayerId(0));
                self.mp_ticks_per_turn = info.ticks_per_turn;
                self.base_ticks_per_turn = info.ticks_per_turn;
                self.net_client = Some(client);
                self.is_multiplayer_mode = true;
                godot_print!(
                    "SimBridge: joined '{}' (session {}) as player {}",
                    info.session_name,
                    session_id,
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
        self.shutdown_relay();
        self.is_multiplayer_mode = false;
        self.local_player_id = SessionPlayerId::LOCAL;
        self.session = GameSession::new_singleplayer();
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
            && let Err(e) = client.send_start_game(seed, &config_json.to_string(), None)
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

        for msg in messages {
            match msg {
                ServerMessage::GameStart {
                    seed, config_json, ..
                } => {
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
                    llm_results,
                    ..
                } => {
                    // Route each command through session.
                    for tc in &commands {
                        if let Ok(action) = serde_json::from_slice::<SimAction>(&tc.payload) {
                            self.session.process(SessionMessage::SimCommand {
                                from: SessionPlayerId(tc.player_id.0),
                                action,
                            });
                        }
                    }
                    // Route LLM results before advancing.
                    for lr in llm_results {
                        if let Ok(payload) =
                            serde_json::from_slice::<LlmResponsePayload>(&lr.payload)
                        {
                            self.session.process(SessionMessage::LlmResult {
                                request_id: lr.request_id,
                                result_json: payload.result_json,
                                metadata: payload.metadata,
                            });
                        }
                    }
                    // Advance the sim.
                    self.session.process(SessionMessage::AdvanceTo {
                        tick: sim_tick_target,
                    });
                    // Drain outbound requests and send to relay.
                    if let Some(sim) = &mut self.session.sim {
                        for req in sim.outbound_requests.drain(..) {
                            if let Some(client) = &mut self.net_client {
                                match req {
                                    elven_canopy_sim::llm::OutboundRequest::LlmInference {
                                        request_id,
                                        preambles,
                                        prompt,
                                        response_schema,
                                        deadline_tick,
                                        max_tokens,
                                        creature_id,
                                    } => {
                                        let payload = LlmRequestPayload {
                                            creature_id: format!("{creature_id}"),
                                            preambles,
                                            prompt,
                                            response_schema,
                                            deadline_tick,
                                            max_tokens,
                                        };
                                        if let Ok(bytes) = serde_json::to_vec(&payload) {
                                            let _ = client.send_llm_request(request_id, bytes);
                                        }
                                    }
                                }
                            }
                        }
                    }
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
                        self.session.process(SessionMessage::LoadSim { json });
                        self.mp_events.push(
                            serde_json::to_string(&serde_json::json!({
                                "type": "snapshot_loaded",
                            }))
                            .unwrap_or_default(),
                        );
                    }
                }
                ServerMessage::SessionResumed { starting_tick } => {
                    // No-op: the sim was already loaded locally before we
                    // sent ResumeSession. The relay is now flushing turns
                    // from this tick.
                    godot_print!("SimBridge: relay resumed session at tick {starting_tick}");
                }
                ServerMessage::SpeedChanged { ticks_per_turn } => {
                    self.mp_ticks_per_turn = ticks_per_turn;
                    let multiplier = if self.base_ticks_per_turn > 0 {
                        ticks_per_turn / self.base_ticks_per_turn
                    } else {
                        1
                    };
                    let speed = match multiplier {
                        0..=1 => SessionSpeed::Normal,
                        2..=4 => SessionSpeed::Fast,
                        _ => SessionSpeed::VeryFast,
                    };
                    self.session.process(SessionMessage::SetSpeed { speed });
                }
                ServerMessage::LlmDispatch {
                    request_id,
                    payload,
                } => {
                    match serde_json::from_slice::<LlmRequestPayload>(&payload) {
                        Ok(req) => {
                            // Build the full prompt from preambles + ephemeral prompt.
                            // For now, preambles are concatenated as-is. KV cache
                            // optimization for well-known sections is a future step.
                            let mut full_prompt = String::new();
                            for section in &req.preambles {
                                match section {
                                    elven_canopy_sim::llm::PreambleSection::WellKnown(key) => {
                                        full_prompt.push_str(key);
                                        full_prompt.push('\n');
                                    }
                                    elven_canopy_sim::llm::PreambleSection::Literal(text) => {
                                        full_prompt.push_str(text);
                                        full_prompt.push('\n');
                                    }
                                }
                            }
                            full_prompt.push_str(&req.prompt);
                            if !req.response_schema.is_empty() {
                                full_prompt
                                    .push_str("\n\nRespond with JSON matching this schema:\n");
                                full_prompt.push_str(&req.response_schema);
                                full_prompt.push('\n');
                            }

                            if self.llm_debug {
                                godot_print!(
                                    "[LLM DEBUG] dispatch request {request_id} creature={} max_tokens={}\n--- PROMPT ---\n{full_prompt}\n--- END PROMPT ---",
                                    req.creature_id,
                                    req.max_tokens,
                                );
                            }
                            if let Some(worker) = &self.llm_worker {
                                worker.send(crate::llm_worker::LlmWorkerCmd::Infer {
                                    request_id,
                                    prompt: full_prompt,
                                    max_tokens: req.max_tokens,
                                });
                            }
                        }
                        Err(e) => {
                            godot_error!(
                                "SimBridge: failed to deserialize LlmDispatch payload for request {request_id}: {e}"
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        // Drain completed LLM inference results and send them back to the
        // relay as LlmResponse messages.
        if let (Some(client), Some(worker)) = (&mut self.net_client, &self.llm_worker) {
            while let Ok(result) = worker.result_rx.try_recv() {
                if self.llm_debug
                    && let Ok(payload) =
                        serde_json::from_slice::<LlmResponsePayload>(&result.payload)
                {
                    let m = &payload.metadata;
                    godot_print!(
                        "[LLM DEBUG] result request {} latency={}ms tokens={} (prefill={} decode={}) cache_hit={}\n--- RESPONSE ---\n{}\n--- END RESPONSE ---",
                        result.request_id,
                        m.latency_ms,
                        m.token_count,
                        m.prefill_tokens,
                        m.decode_tokens,
                        m.cache_hit,
                        payload.result_json,
                    );
                }
                if let Err(e) = client.send_llm_response(result.request_id, result.payload) {
                    godot_error!(
                        "SimBridge: failed to send LlmResponse for request {}: {e}",
                        result.request_id
                    );
                }
            }
        }

        // Desync-detection checksums disabled — see B-fast-checksum.

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

    /// Return all cultivable fruit species as an array of dictionaries with
    /// keys: id (int), name (String), gloss (String). Used by the structure
    /// info panel to populate the greenhouse species picker.
    #[func]
    fn get_cultivable_fruit_species(&self) -> VarArray {
        let Some(sim) = &self.session.sim else {
            return VarArray::new();
        };
        let mut arr = VarArray::new();
        for f in sim.db.fruit_species.iter_all() {
            if !f.greenhouse_cultivable {
                continue;
            }
            let mut dict = VarDictionary::new();
            dict.set("id", f.id.0 as i32);
            let name_str = format!("{} ({})", f.vaelith_name, f.english_gloss);
            dict.set("name", GString::from(name_str.as_str()));
            arr.push(&dict.to_variant());
        }
        arr
    }

    /// Begin furnishing a completed building. `furnishing_type` is a string
    /// matching one of the `FurnishingType` variants ("Dormitory", "Home",
    /// "DiningHall", "Kitchen", "Workshop", "Storehouse", "ConcertHall",
    /// "Greenhouse"). For Greenhouse, `greenhouse_species_id` must be the
    /// ID of a cultivable fruit species (pass -1 for non-greenhouse types).
    /// Ignored if the structure is not a building or already furnished.
    #[func]
    fn furnish_structure(
        &mut self,
        structure_id: i64,
        furnishing_type: GString,
        greenhouse_species_id: i32,
    ) {
        let ft = match furnishing_type.to_string().as_str() {
            "ConcertHall" => FurnishingType::ConcertHall,
            "DanceHall" => FurnishingType::DanceHall,
            "DiningHall" => FurnishingType::DiningHall,
            "Dormitory" => FurnishingType::Dormitory,
            "Greenhouse" => FurnishingType::Greenhouse,
            "Home" => FurnishingType::Home,
            "Kitchen" => FurnishingType::Kitchen,
            "Storehouse" => FurnishingType::Storehouse,
            "Workshop" => FurnishingType::Workshop,
            _ => return,
        };
        let greenhouse_species = if ft == FurnishingType::Greenhouse && greenhouse_species_id >= 0 {
            Some(FruitSpeciesId(greenhouse_species_id as u16))
        } else {
            None
        };
        self.apply_or_send(SimAction::FurnishStructure {
            structure_id: StructureId(structure_id as u64),
            furnishing_type: ft,
            greenhouse_species,
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

    // ========================================================================
    // Elfcyclopedia server
    // ========================================================================

    /// Start the global elfcyclopedia server if not already running.
    fn ensure_elfcyclopedia_started() {
        let mut guard = ELFCYCLOPEDIA.lock().unwrap();
        if guard.is_some() {
            return;
        }
        let species = crate::elfcyclopedia_server::load_species_data();
        match crate::elfcyclopedia_server::ElfcyclopediaServer::start(species) {
            Some(server) => {
                godot_print!(
                    "SimBridge: elfcyclopedia server started at {}",
                    server.url()
                );
                *guard = Some(server);
            }
            None => {
                godot_warn!("SimBridge: failed to bind elfcyclopedia server");
            }
        }
    }

    /// Get the elfcyclopedia URL (empty string if not running).
    #[func]
    fn elfcyclopedia_url(&self) -> GString {
        let guard = ELFCYCLOPEDIA.lock().unwrap();
        match &*guard {
            Some(server) => GString::from(server.url().as_str()),
            None => GString::new(),
        }
    }

    /// Update the elfcyclopedia with current game state. Called each frame.
    fn update_elfcyclopedia(&self) {
        let guard = ELFCYCLOPEDIA.lock().unwrap();
        if let Some(server) = &*guard {
            let tick = self.session.current_tick();
            let (game_name, civs, player_civ_name, fruits) = if let Some(sim) = &self.session.sim {
                let known = sim
                    .get_known_civs()
                    .into_iter()
                    .map(|(civ, our_opinion, their_opinion)| {
                        crate::elfcyclopedia_server::KnownCivEntry {
                            civ_id: civ.id.0,
                            name: civ.name.clone(),
                            primary_species: civ.primary_species.display_str().to_owned(),
                            culture_tag: civ.culture_tag.display_str().to_owned(),
                            our_opinion: our_opinion.display_str().to_owned(),
                            their_opinion: their_opinion.map(|o| o.display_str().to_owned()),
                        }
                    })
                    .collect();
                let pcn = sim
                    .player_civ_id
                    .and_then(|id| sim.db.civilizations.get(&id))
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                let fruit_entries = sim
                    .db
                    .fruit_species
                    .iter_all()
                    .map(Self::fruit_to_entry)
                    .collect();
                ("Elven Canopy".to_owned(), known, pcn, fruit_entries)
            } else {
                (String::new(), Vec::new(), String::new(), Vec::new())
            };
            server.update_data(tick, &game_name, civs, player_civ_name, fruits);
        }
    }

    /// Convert a sim `FruitSpecies` to an elfcyclopedia `FruitEntry`.
    fn fruit_to_entry(
        f: &elven_canopy_sim::fruit::FruitSpecies,
    ) -> crate::elfcyclopedia_server::FruitEntry {
        use elven_canopy_sim::fruit::*;

        let habitat = match f.habitat {
            GrowthHabitat::Branch => "Branch",
            GrowthHabitat::Trunk => "Trunk",
            GrowthHabitat::GroundBush => "Ground Bush",
        };
        let rarity = match f.rarity {
            Rarity::Common => "Common",
            Rarity::Uncommon => "Uncommon",
            Rarity::Rare => "Rare",
        };
        let shape = match f.appearance.shape {
            FruitShape::Round => "Round",
            FruitShape::Oblong => "Oblong",
            FruitShape::Clustered => "Clustered",
            FruitShape::Pod => "Pod",
            FruitShape::Nut => "Nut",
            FruitShape::Gourd => "Gourd",
        };
        let c = &f.appearance.exterior_color;
        let color_hex = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);

        let parts = f
            .parts
            .iter()
            .map(|p| {
                let pt = match p.part_type {
                    PartType::Flesh => "Flesh",
                    PartType::Rind => "Rind",
                    PartType::Seed => "Seed",
                    PartType::Fiber => "Fiber",
                    PartType::Sap => "Sap",
                    PartType::Resin => "Resin",
                };
                let props: Vec<String> = p
                    .properties
                    .iter()
                    .map(|prop| format!("{prop:?}"))
                    .collect();
                let pigment = p.pigment.map(|d| format!("{d:?}"));
                crate::elfcyclopedia_server::FruitPartEntry {
                    part_type: pt.to_owned(),
                    properties: props,
                    pigment,
                    component_units: p.component_units,
                }
            })
            .collect();

        crate::elfcyclopedia_server::FruitEntry {
            id: f.id.0,
            vaelith_name: f.vaelith_name.clone(),
            english_gloss: f.english_gloss.clone(),
            habitat: habitat.to_owned(),
            rarity: rarity.to_owned(),
            shape: shape.to_owned(),
            color_hex,
            glows: f.appearance.glows,
            size_percent: f.appearance.size_percent,
            greenhouse_cultivable: f.greenhouse_cultivable,
            parts,
        }
    }

    // -----------------------------------------------------------------------
    // Selection groups (F-selection-groups)
    // -----------------------------------------------------------------------

    /// Set (overwrite) a numbered selection group. `creature_uuids` is an
    /// untyped GDScript Array of UUID strings, `structure_ids` is an Array
    /// of ints. Sends a `SetSelectionGroup` command to the sim for persistence.
    #[func]
    fn set_selection_group(
        &mut self,
        group_number: i32,
        creature_uuids: VarArray,
        structure_ids: VarArray,
    ) {
        let creature_ids: Vec<elven_canopy_sim::types::CreatureId> = creature_uuids
            .iter_shared()
            .filter_map(|v| parse_creature_id(&v.to_string()))
            .collect();
        let structure_ids: Vec<elven_canopy_sim::types::StructureId> = structure_ids
            .iter_shared()
            .map(|v| elven_canopy_sim::types::StructureId(v.to::<i64>() as u64))
            .collect();
        self.apply_or_send(SimAction::SetSelectionGroup {
            group_number: group_number as u8,
            creature_ids,
            structure_ids,
        });
    }

    /// Add creatures and structures to an existing selection group (or create
    /// it if it doesn't exist). Sends an `AddToSelectionGroup` command.
    #[func]
    fn add_to_selection_group(
        &mut self,
        group_number: i32,
        creature_uuids: VarArray,
        structure_ids: VarArray,
    ) {
        let creature_ids: Vec<elven_canopy_sim::types::CreatureId> = creature_uuids
            .iter_shared()
            .filter_map(|v| parse_creature_id(&v.to_string()))
            .collect();
        let structure_ids: Vec<elven_canopy_sim::types::StructureId> = structure_ids
            .iter_shared()
            .map(|v| elven_canopy_sim::types::StructureId(v.to::<i64>() as u64))
            .collect();
        self.apply_or_send(SimAction::AddToSelectionGroup {
            group_number: group_number as u8,
            creature_ids,
            structure_ids,
        });
    }

    /// Retrieve all selection groups for the local player. Returns an Array
    /// of Dictionaries, each with keys: `group_number` (int), `creature_ids`
    /// (Array of UUID strings), `structure_ids` (Array of ints).
    #[func]
    fn get_all_selection_groups(&self) -> VarArray {
        let mut arr = VarArray::new();
        let Some(sim) = &self.session.sim else {
            return arr;
        };
        let player_name = self
            .session
            .players
            .get(&self.local_player_id)
            .map(|p| p.name.as_str())
            .unwrap_or("");
        for (group_number, creature_ids, structure_ids) in sim.get_selection_groups(player_name) {
            let mut dict = VarDictionary::new();
            dict.set("group_number", group_number as i32);
            let mut cids = VarArray::new();
            for cid in &creature_ids {
                cids.push(&GString::from(&cid.to_string()).to_variant());
            }
            dict.set("creature_ids", cids);
            let mut sids = VarArray::new();
            for sid in &structure_ids {
                sids.push(&(sid.0 as i64).to_variant());
            }
            dict.set("structure_ids", sids);
            arr.push(&dict.to_variant());
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
