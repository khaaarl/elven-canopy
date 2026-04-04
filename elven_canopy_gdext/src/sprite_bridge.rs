// GDExtension bridge for elven_canopy_sprites — converts pixel buffers to Godot textures.
//
// `SpriteGenerator` is a static utility class exposing fruit sprite
// generation to GDScript. Creature sprites are handled entirely by the
// trait-based pipeline in `sim_bridge.rs` (`get_creature_sprite_by_id`)
// and the `CreatureSprites` GDScript cache.
//
// Also provides the `pixel_buffer_to_texture` helper used by sim_bridge.
//
// See also: `sim_bridge.rs` for the creature sprite cache,
// `creature_sprites.gd` for the GDScript-side central sprite cache,
// `elven_canopy_sprites` for the pure Rust sprite generation.

use elven_canopy_sim::fruit::{FruitAppearance, FruitColor, FruitShape};
use elven_canopy_sprites::{PixelBuffer, create_fruit};
use godot::classes::image::Format;
use godot::classes::{Image, ImageTexture};
use godot::prelude::*;

#[derive(GodotClass)]
#[class(init, base = RefCounted)]
struct SpriteGenerator;

/// Convert a PixelBuffer to a Godot ImageTexture.
pub(crate) fn pixel_buffer_to_texture(buf: &PixelBuffer) -> Option<Gd<ImageTexture>> {
    let data = buf.data();
    let byte_array = PackedByteArray::from(data);
    let image = Image::create_from_data(
        buf.width() as i32,
        buf.height() as i32,
        false,
        Format::RGBA8,
        &byte_array,
    )?;
    ImageTexture::create_from_image(&image)
}

#[godot_api]
impl SpriteGenerator {
    /// Generate a 16x16 fruit sprite texture from appearance parameters.
    #[func]
    fn fruit_sprite(
        shape: GString,
        r: u8,
        g: u8,
        b: u8,
        size_percent: i64,
        glows: bool,
    ) -> Option<Gd<ImageTexture>> {
        let fruit_shape = match shape.to_string().as_str() {
            "Round" => FruitShape::Round,
            "Oblong" => FruitShape::Oblong,
            "Clustered" => FruitShape::Clustered,
            "Pod" => FruitShape::Pod,
            "Nut" => FruitShape::Nut,
            "Gourd" => FruitShape::Gourd,
            _ => {
                godot_warn!("SpriteGenerator: unknown fruit shape '{shape}'");
                FruitShape::Round
            }
        };
        let appearance = FruitAppearance {
            exterior_color: FruitColor { r, g, b },
            shape: fruit_shape,
            size_percent: size_percent.clamp(0, u16::MAX as i64) as u16,
            glows,
        };
        let buf = create_fruit(&appearance);
        pixel_buffer_to_texture(&buf)
    }

    /// Generate a fruit sprite from a dictionary with keys matching SimBridge
    /// fruit data: "shape" (String), "color" (Color), "size_percent" (int),
    /// "glows" (bool). Convenience wrapper matching the old SpriteFactory API.
    #[func]
    fn fruit_sprite_from_dict(params: VarDictionary) -> Option<Gd<ImageTexture>> {
        let shape_str: GString = params
            .get("shape")
            .and_then(|v| v.try_to::<GString>().ok())
            .unwrap_or_else(|| "Round".into());

        let color: godot::builtin::Color = params
            .get("color")
            .and_then(|v| v.try_to::<godot::builtin::Color>().ok())
            .unwrap_or(godot::builtin::Color::from_rgba(0.9, 0.5, 0.2, 1.0));

        let size_pct: i64 = params
            .get("size_percent")
            .and_then(|v| v.try_to::<i64>().ok())
            .unwrap_or(100);

        let glows: bool = params
            .get("glows")
            .and_then(|v| v.try_to::<bool>().ok())
            .unwrap_or(false);

        let r = (color.r * 255.0).clamp(0.0, 255.0) as u8;
        let g = (color.g * 255.0).clamp(0.0, 255.0) as u8;
        let b = (color.b * 255.0).clamp(0.0, 255.0) as u8;

        Self::fruit_sprite(shape_str, r, g, b, size_pct, glows)
    }
}
