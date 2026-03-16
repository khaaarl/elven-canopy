// Procedural sprite generation for all creature species and fruit.
//
// Pure Rust library with no Godot dependency — outputs raw RGBA8 pixel buffers
// (`Vec<u8>`). Consumed by `elven_canopy_gdext` (thin wrapper to convert pixel
// buffers into Godot Image/ImageTexture) and the elfcyclopedia server (which
// encodes them as inline PNG data URIs).
//
// All sprites are deterministically generated — creature sprites from integer
// seeds using Knuth multiplicative hashing, fruit sprites from appearance data
// produced during worldgen. Drawing uses integer pixel math on a `PixelBuffer`
// with bounds-checked set_px, circles, ellipses, rectangles, and lines.
//
// Species sprite dimensions vary (32x32 squirrel up to 96x80 elephant/troll).
// Fruit sprites are always 16x16. Color palettes (hair colors, skin tones,
// species body colors, etc.) live as constants in per-species modules.
//
// See also: `sprite_bridge.rs` in `elven_canopy_gdext` (GDExtension bridge
// converting pixel buffers to Godot ImageTextures),
// `elfcyclopedia_server.rs` (consumer for web-rendered fruit PNG data URIs).

mod color;
mod drawing;
mod fruit;
mod species;

pub use color::Color;
pub use drawing::PixelBuffer;
pub use fruit::create_fruit;
pub use species::{SpriteParams, create_species_sprite, species_params_from_seed};
