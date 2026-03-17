// Goblin sprite generation (36x40).
//
// Small green-skinned hostile with oversized head, big yellow eyes, wide grin,
// pointy/droopy/wide ear variants, scrawny body with loincloth, and thin limbs.
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const SKIN_COLORS: [Color; 4] = [
    Color::rgb(0.35, 0.55, 0.25),
    Color::rgb(0.40, 0.60, 0.30),
    Color::rgb(0.30, 0.48, 0.22),
    Color::rgb(0.45, 0.55, 0.20),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EarStyle {
    Pointed,
    Droopy,
    Wide,
}

const EAR_STYLES: [EarStyle; 3] = [EarStyle::Pointed, EarStyle::Droopy, EarStyle::Wide];

#[derive(Clone, Debug)]
pub struct GoblinParams {
    pub skin_color: Color,
    pub ear_style: EarStyle,
}

pub fn params_from_seed(seed: i64) -> GoblinParams {
    let h = knuth_hash(seed);
    GoblinParams {
        skin_color: SKIN_COLORS[(h % 4) as usize],
        ear_style: EAR_STYLES[((h / 17) % 3) as usize],
    }
}

pub fn params_from_traits(traits: &super::TraitMap) -> GoblinParams {
    use elven_canopy_sim::types::TraitKind;
    GoblinParams {
        skin_color: SKIN_COLORS
            [super::trait_idx(traits, TraitKind::SkinColor, 0) % SKIN_COLORS.len()],
        ear_style: EAR_STYLES[super::trait_idx(traits, TraitKind::EarStyle, 0) % EAR_STYLES.len()],
    }
}

pub fn create_sprite(p: &GoblinParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(36, 40);
    let skin = p.skin_color;
    let skin_dark = skin.darken(0.15);
    let skin_light = skin.lighten(0.10);
    let outline = Color::rgb(0.12, 0.15, 0.08);
    let eye_color = Color::rgb(0.90, 0.75, 0.10);
    let pupil = Color::rgb(0.10, 0.05, 0.02);
    let mouth = Color::rgb(0.20, 0.10, 0.08);
    let loincloth = Color::rgb(0.45, 0.35, 0.22);

    // Head
    let head_cx = 18;
    let head_cy = 12;
    img.draw_circle(head_cx, head_cy, 10, outline);
    img.draw_circle(head_cx, head_cy, 9, skin);
    img.draw_circle(head_cx - 1, head_cy + 1, 5, skin_light);

    // Ears
    match p.ear_style {
        EarStyle::Pointed => {
            for i in 0..8 {
                img.set_px(head_cx - 10 - i / 2, head_cy - 4 + i, outline);
                img.set_px(head_cx - 9 - i / 2, head_cy - 4 + i, skin_dark);
                img.set_px(head_cx + 10 + i / 2, head_cy - 4 + i, outline);
                img.set_px(head_cx + 9 + i / 2, head_cy - 4 + i, skin_dark);
            }
        }
        EarStyle::Droopy => {
            for i in 0..6 {
                img.set_px(head_cx - 10, head_cy + i, outline);
                img.set_px(head_cx - 9, head_cy + i, skin_dark);
                img.set_px(head_cx + 10, head_cy + i, outline);
                img.set_px(head_cx + 9, head_cy + i, skin_dark);
            }
        }
        EarStyle::Wide => {
            img.draw_ellipse(head_cx - 12, head_cy, 4, 3, outline);
            img.draw_ellipse(head_cx - 12, head_cy, 3, 2, skin_dark);
            img.draw_ellipse(head_cx + 12, head_cy, 4, 3, outline);
            img.draw_ellipse(head_cx + 12, head_cy, 3, 2, skin_dark);
        }
    }

    // Eyes
    img.draw_rect(head_cx - 6, head_cy - 3, 4, 4, eye_color);
    img.draw_rect(head_cx - 5, head_cy - 2, 2, 2, pupil);
    img.draw_rect(head_cx + 3, head_cy - 3, 4, 4, eye_color);
    img.draw_rect(head_cx + 4, head_cy - 2, 2, 2, pupil);

    // Nose
    img.set_px(head_cx - 1, head_cy + 3, skin_dark);
    img.set_px(head_cx + 1, head_cy + 3, skin_dark);

    // Mouth
    img.draw_hline(head_cx - 4, head_cx + 4, head_cy + 6, mouth);
    img.set_px(head_cx - 4, head_cy + 5, mouth);
    img.set_px(head_cx + 4, head_cy + 5, mouth);

    // Body
    let body_cx = 18;
    let body_cy = 27;
    img.draw_ellipse(body_cx, body_cy, 7, 8, outline);
    img.draw_ellipse(body_cx, body_cy, 6, 7, skin);

    // Loincloth
    img.draw_rect(body_cx - 5, body_cy + 2, 10, 4, loincloth);

    // Arms
    img.draw_rect(body_cx - 9, body_cy - 4, 3, 10, outline);
    img.draw_rect(body_cx - 8, body_cy - 3, 1, 8, skin_dark);
    img.draw_rect(body_cx + 7, body_cy - 4, 3, 10, outline);
    img.draw_rect(body_cx + 8, body_cy - 3, 1, 8, skin_dark);

    // Legs
    img.draw_rect(body_cx - 4, body_cy + 6, 3, 7, outline);
    img.draw_rect(body_cx - 3, body_cy + 7, 1, 5, skin_dark);
    img.draw_rect(body_cx + 2, body_cy + 6, 3, 7, outline);
    img.draw_rect(body_cx + 3, body_cy + 7, 1, 5, skin_dark);

    img
}
