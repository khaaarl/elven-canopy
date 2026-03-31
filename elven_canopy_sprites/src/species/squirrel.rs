// Squirrel sprite generation (32x32).
//
// Small chibi squirrel with round head, big cute eyes, pointed ear tufts,
// belly patch, tiny arms, and a prominent bushy tail (fluffy, extra fluffy,
// or curled variant).
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const FUR_COLORS: [Color; 4] = [
    Color::rgb(0.65, 0.30, 0.18), // red
    Color::rgb(0.68, 0.52, 0.28), // golden
    Color::rgb(0.45, 0.32, 0.20), // brown
    Color::rgb(0.55, 0.52, 0.48), // grey
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TailType {
    Fluffy,
    ExtraFluffy,
    Curled,
}

const TAIL_TYPES: [TailType; 3] = [TailType::Fluffy, TailType::ExtraFluffy, TailType::Curled];

#[derive(Clone, Debug)]
pub struct SquirrelParams {
    pub fur_color: Color,
    pub tail_type: TailType,
}

pub fn params_from_seed(seed: i64) -> SquirrelParams {
    let h = knuth_hash(seed);
    SquirrelParams {
        fur_color: FUR_COLORS[(h % 4) as usize],
        tail_type: TAIL_TYPES[((h / 23) % 3) as usize],
    }
}

pub fn params_from_traits(traits: &super::TraitMap) -> SquirrelParams {
    use elven_canopy_sim::types::TraitKind;
    let base_idx = super::trait_idx(traits, TraitKind::FurColor, 0) % FUR_COLORS.len();
    let blend_target = super::trait_i64(traits, TraitKind::FurBlendTarget, -1);
    let blend_weight = super::trait_i64(traits, TraitKind::FurBlendWeight, 0);
    let value = super::trait_i64(traits, TraitKind::FurValue, 0);
    let saturation = super::trait_i64(traits, TraitKind::FurSaturation, 0);
    SquirrelParams {
        fur_color: super::resolve_hue(&FUR_COLORS, base_idx, blend_target, blend_weight)
            .apply_value(value)
            .apply_saturation(saturation),
        tail_type: TAIL_TYPES[super::trait_idx(traits, TraitKind::TailType, 0) % TAIL_TYPES.len()],
    }
}

pub fn create_sprite(p: &SquirrelParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(32, 32);
    let fur_color = p.fur_color;
    let fur_dark = fur_color.darken(0.12);
    let fur_light = fur_color.lighten(0.12);
    let outline = Color::rgb(0.15, 0.12, 0.10);
    let belly_color = Color::rgb(0.90, 0.85, 0.75);
    let eye_color = Color::rgb(0.10, 0.08, 0.06);
    let white = Color::rgb(1.0, 1.0, 1.0);
    let nose_color = Color::rgb(0.30, 0.20, 0.15);

    // Head
    let head_cx = 12;
    let head_cy = 9;
    img.draw_circle(head_cx, head_cy, 7, outline);
    img.draw_circle(head_cx, head_cy, 6, fur_color);
    img.draw_ellipse(head_cx, head_cy + 1, 4, 3, fur_light);

    // Eyes
    img.draw_rect(head_cx - 4, head_cy - 2, 3, 3, outline);
    img.draw_rect(head_cx - 3, head_cy - 1, 1, 1, eye_color);
    img.set_px(head_cx - 3, head_cy - 2, white);
    img.draw_rect(head_cx + 2, head_cy - 2, 3, 3, outline);
    img.draw_rect(head_cx + 3, head_cy - 1, 1, 1, eye_color);
    img.set_px(head_cx + 3, head_cy - 2, white);

    // Nose
    img.set_px(head_cx, head_cy + 2, nose_color);

    // Ear tufts
    img.set_px(head_cx - 5, head_cy - 6, fur_dark);
    img.set_px(head_cx - 4, head_cy - 6, fur_color);
    img.set_px(head_cx - 5, head_cy - 7, fur_dark);
    img.set_px(head_cx + 5, head_cy - 6, fur_dark);
    img.set_px(head_cx + 4, head_cy - 6, fur_color);
    img.set_px(head_cx + 5, head_cy - 7, fur_dark);

    // Body
    let body_cx = 14;
    let body_cy = 19;
    img.draw_ellipse(body_cx, body_cy, 6, 5, outline);
    img.draw_ellipse(body_cx, body_cy, 5, 4, fur_color);
    img.draw_ellipse(body_cx, body_cy + 1, 3, 2, belly_color);

    // Arms
    img.draw_rect(body_cx - 6, 17, 2, 4, fur_dark);
    img.draw_rect(body_cx + 5, 17, 2, 4, fur_dark);

    // Legs
    img.draw_rect(body_cx - 4, 23, 3, 3, outline);
    img.draw_rect(body_cx - 3, 23, 1, 2, fur_dark);
    img.draw_rect(body_cx + 2, 23, 3, 3, outline);
    img.draw_rect(body_cx + 3, 23, 1, 2, fur_dark);

    // Tail
    match p.tail_type {
        TailType::Fluffy => {
            img.draw_ellipse(25, 12, 5, 7, fur_color);
            img.draw_ellipse(25, 11, 4, 5, fur_light);
        }
        TailType::ExtraFluffy => {
            img.draw_ellipse(25, 11, 6, 8, fur_color);
            img.draw_ellipse(25, 10, 5, 6, fur_light);
            img.draw_circle(26, 5, 3, fur_color);
        }
        TailType::Curled => {
            img.draw_ellipse(24, 13, 5, 6, fur_color);
            img.draw_ellipse(24, 12, 4, 4, fur_light);
            img.draw_circle(27, 8, 3, fur_color);
            img.draw_circle(27, 8, 2, fur_light);
        }
    }

    img
}
