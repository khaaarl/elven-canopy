// Capybara sprite generation (40x32).
//
// A friendly round capybara with body color variants and optional accessories
// (flower crown, scarf, bow). Side-facing profile with wide horizontal body,
// smaller head, snout, stubby legs, and tiny tail.
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const BODY_COLORS: [Color; 4] = [
    Color::rgb(0.68, 0.55, 0.40), // sandy
    Color::rgb(0.65, 0.48, 0.32), // golden-brown
    Color::rgb(0.55, 0.35, 0.22), // russet
    Color::rgb(0.42, 0.28, 0.18), // chocolate
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accessory {
    None,
    FlowerCrown,
    Scarf,
    Bow,
}

const ACCESSORIES: [Accessory; 4] = [
    Accessory::None,
    Accessory::FlowerCrown,
    Accessory::Scarf,
    Accessory::Bow,
];

#[derive(Clone, Debug)]
pub struct CapybaraParams {
    pub body_color: Color,
    pub accessory: Accessory,
}

pub fn params_from_seed(seed: i64) -> CapybaraParams {
    let h = knuth_hash(seed);
    CapybaraParams {
        body_color: BODY_COLORS[(h % 4) as usize],
        accessory: ACCESSORIES[((h / 13) % 4) as usize],
    }
}

pub fn params_from_traits(traits: &super::TraitMap) -> CapybaraParams {
    use elven_canopy_sim::types::TraitKind;
    let base_idx = super::trait_idx(traits, TraitKind::BodyColor, 0) % BODY_COLORS.len();
    let blend_target = super::trait_i64(traits, TraitKind::BodyBlendTarget, -1);
    let blend_weight = super::trait_i64(traits, TraitKind::BodyBlendWeight, 0);
    let value = super::trait_i64(traits, TraitKind::BodyValue, 0);
    let saturation = super::trait_i64(traits, TraitKind::BodySaturation, 0);
    CapybaraParams {
        body_color: super::resolve_hue(&BODY_COLORS, base_idx, blend_target, blend_weight)
            .apply_value(value)
            .apply_saturation(saturation),
        accessory: ACCESSORIES
            [super::trait_idx(traits, TraitKind::Accessory, 0) % ACCESSORIES.len()],
    }
}

pub fn create_sprite(p: &CapybaraParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(40, 32);
    let body_color = p.body_color;
    let body_dark = body_color.darken(0.10);
    let body_light = body_color.lighten(0.10);
    let outline = Color::rgb(0.18, 0.14, 0.10);
    let nose_color = Color::rgb(0.75, 0.50, 0.45);
    let eye_color = Color::rgb(0.10, 0.08, 0.06);
    let white = Color::rgb(1.0, 1.0, 1.0);

    // 1. Body
    let body_cx = 20;
    let body_cy = 18;
    img.draw_ellipse(body_cx, body_cy, 16, 10, outline);
    img.draw_ellipse(body_cx, body_cy, 15, 9, body_color);
    img.draw_ellipse(body_cx + 1, body_cy + 2, 10, 5, body_light);

    // 2. Head
    let head_cx = 6;
    let head_cy = 11;
    img.draw_circle(head_cx, head_cy, 8, outline);
    img.draw_circle(head_cx, head_cy, 7, body_color);
    img.draw_ellipse(head_cx - 1, head_cy + 1, 5, 4, body_light);

    // 3. Snout + nostrils
    img.draw_ellipse(2, head_cy + 2, 3, 2, nose_color);
    img.set_px(1, head_cy + 2, nose_color.darken(0.2));
    img.set_px(3, head_cy + 2, nose_color.darken(0.2));

    // 4. Eyes
    img.draw_rect(head_cx - 2, head_cy - 3, 2, 2, eye_color);
    img.set_px(head_cx - 2, head_cy - 3, white);

    // 5. Ears
    img.draw_circle(head_cx - 3, head_cy - 6, 2, body_dark);
    img.draw_circle(head_cx + 1, head_cy - 6, 2, body_dark);
    img.set_px(head_cx - 3, head_cy - 6, nose_color);
    img.set_px(head_cx + 1, head_cy - 6, nose_color);

    // 6. Four stubby legs
    let leg_color = body_dark;
    let leg_y = body_cy + 7;
    img.draw_rect(9, leg_y, 4, 5, outline);
    img.draw_rect(10, leg_y, 2, 4, leg_color);
    img.draw_rect(15, leg_y, 4, 5, outline);
    img.draw_rect(16, leg_y, 2, 4, leg_color);
    img.draw_rect(25, leg_y, 4, 5, outline);
    img.draw_rect(26, leg_y, 2, 4, leg_color);
    img.draw_rect(31, leg_y, 4, 5, outline);
    img.draw_rect(32, leg_y, 2, 4, leg_color);

    // 7. Tiny tail
    img.set_px(36, body_cy - 2, body_dark);
    img.set_px(37, body_cy - 3, body_dark);
    img.set_px(37, body_cy - 2, body_dark);

    // 8. Accessories
    match p.accessory {
        Accessory::FlowerCrown => {
            let flower_colors = [
                Color::rgb(0.95, 0.40, 0.50),
                Color::rgb(0.95, 0.85, 0.30),
                Color::rgb(0.55, 0.70, 0.95),
            ];
            for (i, &fc) in flower_colors.iter().enumerate() {
                let fx = head_cx - 4 + i as i32 * 3;
                let fy = head_cy - 7;
                img.draw_circle(fx, fy, 1, fc);
                img.set_px(fx, fy, Color::rgb(1.0, 1.0, 0.60));
            }
            img.draw_hline(
                head_cx - 5,
                head_cx + 3,
                head_cy - 6,
                Color::rgb(0.25, 0.60, 0.20),
            );
        }
        Accessory::Scarf => {
            let scarf_color = Color::rgb(0.85, 0.25, 0.25);
            img.draw_hline(head_cx - 1, head_cx + 8, head_cy + 5, scarf_color);
            img.draw_hline(head_cx, head_cx + 9, head_cy + 6, scarf_color);
            img.draw_vline(head_cx + 9, head_cy + 6, head_cy + 9, scarf_color);
            img.draw_vline(head_cx + 10, head_cy + 7, head_cy + 10, scarf_color);
        }
        Accessory::Bow => {
            let bow_color = Color::rgb(0.90, 0.45, 0.60);
            img.draw_ellipse(head_cx - 3, head_cy - 7, 2, 2, bow_color);
            img.draw_ellipse(head_cx + 1, head_cy - 7, 2, 2, bow_color);
            img.set_px(head_cx - 1, head_cy - 7, bow_color.darken(0.2));
        }
        Accessory::None => {}
    }

    img
}
