## Central creature sprite cache — single source of truth for creature textures.
##
## All creature sprite consumers (creature_renderer.gd, units_panel.gd,
## group_info_panel.gd, etc.) get sprites through this class. Each call
## checks a GDScript-side cache first; on miss or change, delegates to
## SimBridge.get_creature_sprite_by_id(), which maintains a Rust-side cache
## with change detection (trait changes, equipment swaps). The GDScript
## cache avoids redundant GPU texture uploads when sprites haven't changed.
##
## See also: sim_bridge.rs get_creature_sprite_by_id() for the Rust-side
## cache, creature_renderer.gd for the in-world renderer.

class_name CreatureSprites

## Default Y offset for species not yet in the table.
const DEFAULT_Y_OFFSET := 0.48

## creature_id -> Texture2D (normal upright sprite).
static var _cache: Dictionary = {}
## creature_id -> Texture2D (90-degree CW rotated for incapacitated display).
static var _fallen_cache: Dictionary = {}
## species_name -> sprite Y offset (float). Populated from bridge at startup.
static var _species_y_offsets: Dictionary = {}


## Return the normal (upright) sprite for a creature. Uses the GDScript-side
## cache to avoid redundant GPU texture uploads; delegates to the bridge
## (which has its own Rust-side cache with change detection) on miss.
static func get_sprite(bridge: SimBridge, creature_id: String) -> Texture2D:
	var data: Dictionary = bridge.get_creature_sprite_by_id(creature_id)
	if data.is_empty():
		return _cache.get(creature_id)
	if data.get("changed", true):
		_cache[creature_id] = data.get("normal")
		_fallen_cache[creature_id] = data.get("fallen")
	return _cache.get(creature_id)


## Return the fallen (90-degree CW rotated) sprite for a creature.
static func get_fallen_sprite(bridge: SimBridge, creature_id: String) -> Texture2D:
	var data: Dictionary = bridge.get_creature_sprite_by_id(creature_id)
	if data.is_empty():
		return _fallen_cache.get(creature_id)
	if data.get("changed", true):
		_cache[creature_id] = data.get("normal")
		_fallen_cache[creature_id] = data.get("fallen")
	return _fallen_cache.get(creature_id)


## Populate _species_y_offsets from bridge data. Call once at startup.
static func load_species_display_info(bridge: SimBridge) -> void:
	_species_y_offsets.clear()
	for entry in bridge.get_species_display_info():
		var sp_name: String = entry.get("name", "")
		var y_off: float = entry.get("sprite_y_offset", DEFAULT_Y_OFFSET)
		if sp_name != "":
			_species_y_offsets[sp_name] = y_off


## Return the sprite Y offset for a species.
static func get_y_offset(species: String) -> float:
	return _species_y_offsets.get(species, DEFAULT_Y_OFFSET)


## Clear all cached sprites. Call on save load or new game.
## Note: _species_y_offsets is NOT cleared here — it comes from GameConfig
## (static per session) and is only loaded once during setup. Sprite caches
## depend on creature state (which changes on load), but species config does not.
static func clear() -> void:
	_cache.clear()
	_fallen_cache.clear()
