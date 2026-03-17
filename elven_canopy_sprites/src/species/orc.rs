// Orc sprite generation (48x48).
//
// Burly gray-green hostile with heavy brow ridge, red eyes, protruding tusks,
// optional war paint (stripe or cross), leather armor vest, and thick limbs.
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const SKIN_COLORS: [Color; 4] = [
    Color::rgb(0.38, 0.50, 0.30),
    Color::rgb(0.42, 0.45, 0.28),
    Color::rgb(0.35, 0.42, 0.25),
    Color::rgb(0.48, 0.52, 0.32),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarPaint {
    None,
    Stripe,
    Cross,
}

const WAR_PAINTS: [WarPaint; 3] = [WarPaint::None, WarPaint::Stripe, WarPaint::Cross];

#[derive(Clone, Debug)]
pub struct OrcParams {
    pub skin_color: Color,
    pub war_paint: WarPaint,
}

pub fn params_from_seed(seed: i64) -> OrcParams {
    let h = knuth_hash(seed);
    OrcParams {
        skin_color: SKIN_COLORS[(h % 4) as usize],
        war_paint: WAR_PAINTS[((h / 17) % 3) as usize],
    }
}

pub fn params_from_traits(traits: &super::TraitMap) -> OrcParams {
    use elven_canopy_sim::types::TraitKind;
    OrcParams {
        skin_color: SKIN_COLORS
            [super::trait_idx(traits, TraitKind::SkinColor, 0) % SKIN_COLORS.len()],
        war_paint: WAR_PAINTS[super::trait_idx(traits, TraitKind::WarPaint, 0) % WAR_PAINTS.len()],
    }
}

pub fn create_sprite(p: &OrcParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(48, 48);
    let skin = p.skin_color;
    let skin_dark = skin.darken(0.12);
    let outline = Color::rgb(0.15, 0.15, 0.10);
    let eye_color = Color::rgb(0.85, 0.20, 0.10);
    let pupil = Color::rgb(0.10, 0.05, 0.02);
    let tusk_color = Color::rgb(0.88, 0.85, 0.75);
    let paint_color = Color::rgb(0.70, 0.15, 0.10);
    let armor = Color::rgb(0.35, 0.30, 0.25);
    let armor_dark = Color::rgb(0.25, 0.22, 0.18);

    // Head
    let head_cx = 24;
    let head_cy = 12;
    img.draw_circle(head_cx, head_cy, 11, outline);
    img.draw_circle(head_cx, head_cy, 10, skin);
    // Brow ridge
    img.draw_hline(head_cx - 8, head_cx + 8, head_cy - 5, skin_dark);
    img.draw_hline(head_cx - 7, head_cx + 7, head_cy - 6, skin_dark);

    // Eyes
    img.draw_rect(head_cx - 6, head_cy - 3, 4, 3, eye_color);
    img.draw_rect(head_cx - 5, head_cy - 2, 2, 2, pupil);
    img.draw_rect(head_cx + 3, head_cy - 3, 4, 3, eye_color);
    img.draw_rect(head_cx + 4, head_cy - 2, 2, 2, pupil);

    // Nose
    img.draw_rect(head_cx - 2, head_cy + 1, 4, 3, skin_dark);

    // Tusks
    img.draw_rect(head_cx - 5, head_cy + 5, 2, 4, tusk_color);
    img.draw_rect(head_cx + 4, head_cy + 5, 2, 4, tusk_color);

    // Ears
    img.draw_circle(head_cx - 10, head_cy - 1, 3, outline);
    img.draw_circle(head_cx - 10, head_cy - 1, 2, skin_dark);
    img.draw_circle(head_cx + 10, head_cy - 1, 3, outline);
    img.draw_circle(head_cx + 10, head_cy - 1, 2, skin_dark);

    // War paint
    match p.war_paint {
        WarPaint::Stripe => {
            img.draw_hline(head_cx - 7, head_cx + 7, head_cy - 2, paint_color);
            img.draw_hline(head_cx - 6, head_cx + 6, head_cy - 1, paint_color);
        }
        WarPaint::Cross => {
            img.draw_vline(head_cx, head_cy - 6, head_cy + 3, paint_color);
            img.draw_hline(head_cx - 4, head_cx + 4, head_cy - 2, paint_color);
        }
        WarPaint::None => {}
    }

    // Body
    let body_cx = 24;
    let body_cy = 30;
    img.draw_ellipse(body_cx, body_cy, 12, 10, outline);
    img.draw_ellipse(body_cx, body_cy, 11, 9, skin);

    // Armor vest
    img.draw_ellipse(body_cx, body_cy, 9, 7, armor);
    img.draw_ellipse(body_cx, body_cy + 1, 7, 5, armor_dark);

    // Arms
    img.draw_rect(body_cx - 14, body_cy - 6, 4, 14, outline);
    img.draw_rect(body_cx - 13, body_cy - 5, 2, 12, skin_dark);
    img.draw_rect(body_cx + 11, body_cy - 6, 4, 14, outline);
    img.draw_rect(body_cx + 12, body_cy - 5, 2, 12, skin_dark);

    // Legs
    img.draw_rect(body_cx - 6, body_cy + 8, 5, 8, outline);
    img.draw_rect(body_cx - 5, body_cy + 9, 3, 6, skin_dark);
    img.draw_rect(body_cx + 2, body_cy + 8, 5, 8, outline);
    img.draw_rect(body_cx + 3, body_cy + 9, 3, 6, skin_dark);

    img
}
