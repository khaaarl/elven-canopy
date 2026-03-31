// Boar sprite generation (44x36).
//
// Side-facing profile with bristly back, snout, tusks of varying size,
// and four stubby legs. Compact quadruped with a short curly tail.
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const BODY_COLORS: [Color; 4] = [
    Color::rgb(0.50, 0.38, 0.25), // muddy-brown
    Color::rgb(0.55, 0.35, 0.25), // reddish-brown
    Color::rgb(0.35, 0.28, 0.20), // dark-brown
    Color::rgb(0.48, 0.42, 0.38), // grey-brown
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuskSize {
    Small,
    Medium,
    Large,
}

const TUSK_SIZES: [TuskSize; 3] = [TuskSize::Small, TuskSize::Medium, TuskSize::Large];

#[derive(Clone, Debug)]
pub struct BoarParams {
    pub body_color: Color,
    pub tusk_size: TuskSize,
}

pub fn params_from_seed(seed: i64) -> BoarParams {
    let h = knuth_hash(seed);
    BoarParams {
        body_color: BODY_COLORS[(h % 4) as usize],
        tusk_size: TUSK_SIZES[((h / 17) % 3) as usize],
    }
}

pub fn params_from_traits(traits: &super::TraitMap) -> BoarParams {
    use elven_canopy_sim::types::TraitKind;
    let base_idx = super::trait_idx(traits, TraitKind::BodyColor, 0) % BODY_COLORS.len();
    let blend_target = super::trait_i64(traits, TraitKind::BodyBlendTarget, -1);
    let blend_weight = super::trait_i64(traits, TraitKind::BodyBlendWeight, 0);
    let value = super::trait_i64(traits, TraitKind::BodyValue, 0);
    let saturation = super::trait_i64(traits, TraitKind::BodySaturation, 0);
    BoarParams {
        body_color: super::resolve_hue(&BODY_COLORS, base_idx, blend_target, blend_weight)
            .apply_value(value)
            .apply_saturation(saturation),
        tusk_size: TUSK_SIZES[super::trait_idx(traits, TraitKind::TuskSize, 0) % TUSK_SIZES.len()],
    }
}

pub fn create_sprite(p: &BoarParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(44, 36);
    let body_color = p.body_color;
    let body_dark = body_color.darken(0.12);
    let body_light = body_color.lighten(0.10);
    let outline = Color::rgb(0.15, 0.12, 0.10);
    let eye_color = Color::rgb(0.10, 0.08, 0.06);
    let white = Color::rgb(1.0, 1.0, 1.0);
    let snout_color = Color::rgb(0.65, 0.45, 0.40);
    let tusk_color = Color::rgb(0.90, 0.88, 0.80);

    // Body
    let body_cx = 24;
    let body_cy = 20;
    img.draw_ellipse(body_cx, body_cy, 16, 10, outline);
    img.draw_ellipse(body_cx, body_cy, 15, 9, body_color);
    img.draw_ellipse(body_cx + 1, body_cy + 2, 10, 5, body_light);

    // Bristly back
    let mut bx = body_cx - 10;
    while bx < body_cx + 10 {
        img.set_px(bx, body_cy - 8, body_dark);
        img.set_px(bx + 1, body_cy - 9, body_dark);
        img.set_px(bx, body_cy - 7, body_dark);
        bx += 2;
    }

    // Head
    let head_cx = 7;
    let head_cy = 14;
    img.draw_circle(head_cx, head_cy, 8, outline);
    img.draw_circle(head_cx, head_cy, 7, body_color);

    // Snout
    img.draw_ellipse(2, head_cy + 2, 3, 2, snout_color);
    img.set_px(1, head_cy + 2, snout_color.darken(0.2));
    img.set_px(3, head_cy + 2, snout_color.darken(0.2));

    // Eyes
    img.draw_rect(head_cx - 3, head_cy - 3, 2, 2, eye_color);
    img.set_px(head_cx - 3, head_cy - 3, white);

    // Ears
    img.draw_circle(head_cx - 4, head_cy - 6, 2, body_dark);
    img.draw_circle(head_cx + 1, head_cy - 6, 2, body_dark);

    // Tusks
    let tusk_len = match p.tusk_size {
        TuskSize::Small => 2,
        TuskSize::Medium => 3,
        TuskSize::Large => 4,
    };
    img.draw_vline(2, head_cy + 4, head_cy + 4 + tusk_len, tusk_color);
    img.draw_vline(5, head_cy + 4, head_cy + 4 + tusk_len, tusk_color);

    // Legs
    let leg_y = body_cy + 7;
    img.draw_rect(12, leg_y, 4, 5, outline);
    img.draw_rect(13, leg_y, 2, 4, body_dark);
    img.draw_rect(18, leg_y, 4, 5, outline);
    img.draw_rect(19, leg_y, 2, 4, body_dark);
    img.draw_rect(28, leg_y, 4, 5, outline);
    img.draw_rect(29, leg_y, 2, 4, body_dark);
    img.draw_rect(34, leg_y, 4, 5, outline);
    img.draw_rect(35, leg_y, 2, 4, body_dark);

    // Tail
    img.set_px(39, body_cy - 3, body_dark);
    img.set_px(40, body_cy - 4, body_dark);
    img.set_px(41, body_cy - 3, body_dark);

    img
}
