// Procedural sprite generation for all creature species and fruit.
//
// Pure Rust library with no Godot dependency — outputs raw RGBA8 pixel buffers
// (`Vec<u8>`). Consumed by `elven_canopy_gdext` (thin wrapper to convert pixel
// buffers into Godot Image/ImageTexture) and the elfcyclopedia server (which
// encodes them as inline PNG data URIs).
//
// Creature sprites are generated from biological traits via
// `species_params_from_traits` → `create_sprite_with_equipment`. Equipment
// overlays are applied on top of the base sprite for species that have overlay
// art (currently elves). Fruit sprites are generated from appearance data
// produced during worldgen.
//
// Drawing uses integer pixel math on a `PixelBuffer` with bounds-checked
// set_px, circles, ellipses, rectangles, and lines.
//
// Species sprite dimensions vary (32x32 squirrel up to 96x80 elephant/troll).
// Fruit sprites are always 16x16. Color palettes (hair colors, skin tones,
// species body colors, etc.) live as constants in per-species modules.
//
// See also: `sim_bridge.rs` in `elven_canopy_gdext` for the creature sprite
// cache, `sprite_bridge.rs` for the fruit sprite GDExtension bridge,
// `elfcyclopedia_server.rs` (consumer for web-rendered fruit PNG data URIs).

mod color;
mod drawing;
mod fruit;
mod species;

pub use color::Color;
pub use drawing::PixelBuffer;
pub use fruit::create_fruit;
pub use species::elf::EquipSlotDrawInfo;
pub use species::{
    SpriteParams, TraitMap, create_species_sprite, create_sprite_with_equipment,
    species_params_from_traits,
};
