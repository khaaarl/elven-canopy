// Troll sprite generation (96x80).
//
// Massive brutish hostile with heavy brow, deep-set glowing eyes, wide jaw
// with teeth, optional horns (short or curved), semi-transparent moss patches,
// enormous fists, and thick stumpy legs.
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const SKIN_COLORS: [Color; 4] = [
    Color::rgb(0.38, 0.42, 0.35),
    Color::rgb(0.32, 0.38, 0.30),
    Color::rgb(0.42, 0.45, 0.38),
    Color::rgb(0.35, 0.35, 0.32),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HornStyle {
    Short,
    Curved,
    None,
}

const HORN_STYLES: [HornStyle; 3] = [HornStyle::Short, HornStyle::Curved, HornStyle::None];

#[derive(Clone, Debug)]
pub struct TrollParams {
    pub skin_color: Color,
    pub horn_style: HornStyle,
}

pub fn params_from_seed(seed: i64) -> TrollParams {
    let h = knuth_hash(seed);
    TrollParams {
        skin_color: SKIN_COLORS[(h % 4) as usize],
        horn_style: HORN_STYLES[((h / 17) % 3) as usize],
    }
}

pub fn create_sprite(p: &TrollParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(96, 80);
    let skin = p.skin_color;
    let skin_dark = skin.darken(0.12);
    let skin_light = skin.lighten(0.08);
    let outline = Color::rgb(0.15, 0.15, 0.12);
    let eye_color = Color::rgb(0.90, 0.60, 0.10);
    let horn_color = Color::rgb(0.50, 0.45, 0.35);
    let moss = Color::from_f32(0.30, 0.45, 0.20, 0.60);

    // Body
    let body_cx = 48;
    let body_cy = 46;
    img.draw_ellipse(body_cx, body_cy, 28, 22, outline);
    img.draw_ellipse(body_cx, body_cy, 26, 20, skin);
    img.draw_ellipse(body_cx, body_cy + 2, 20, 14, skin_light);

    // Head
    let head_cx = 28;
    let head_cy = 22;
    img.draw_circle(head_cx, head_cy, 16, outline);
    img.draw_circle(head_cx, head_cy, 14, skin);
    // Heavy brow
    img.draw_hline(head_cx - 12, head_cx + 12, head_cy - 8, skin_dark);
    img.draw_hline(head_cx - 11, head_cx + 11, head_cy - 9, skin_dark);
    img.draw_hline(head_cx - 10, head_cx + 10, head_cy - 10, skin_dark);

    // Horns
    match p.horn_style {
        HornStyle::Short => {
            img.draw_rect(head_cx - 10, head_cy - 16, 4, 6, horn_color);
            img.draw_rect(head_cx + 7, head_cy - 16, 4, 6, horn_color);
        }
        HornStyle::Curved => {
            for i in 0..8 {
                img.set_px(head_cx - 10 - i / 3, head_cy - 12 - i, horn_color);
                img.set_px(head_cx - 9 - i / 3, head_cy - 12 - i, horn_color);
                img.set_px(head_cx + 10 + i / 3, head_cy - 12 - i, horn_color);
                img.set_px(head_cx + 11 + i / 3, head_cy - 12 - i, horn_color);
            }
        }
        HornStyle::None => {}
    }

    // Eyes
    img.draw_rect(head_cx - 8, head_cy - 4, 6, 5, outline);
    img.draw_rect(head_cx - 6, head_cy - 2, 2, 2, eye_color);
    img.draw_rect(head_cx + 3, head_cy - 4, 6, 5, outline);
    img.draw_rect(head_cx + 5, head_cy - 2, 2, 2, eye_color);
    img.set_px(head_cx - 6, head_cy - 4, eye_color);
    img.set_px(head_cx + 5, head_cy - 4, eye_color);

    // Nose
    img.draw_rect(head_cx - 3, head_cy + 2, 6, 4, skin_dark);

    // Jaw + teeth
    img.draw_rect(head_cx - 8, head_cy + 8, 16, 4, skin_dark);
    let tooth_color = Color::rgb(0.85, 0.82, 0.75);
    let mut tx = head_cx - 6;
    while tx < head_cx + 6 {
        img.draw_rect(tx, head_cy + 6, 2, 3, tooth_color);
        tx += 4;
    }

    // Moss patches
    img.draw_circle(body_cx - 12, body_cy - 8, 4, moss);
    img.draw_circle(body_cx + 15, body_cy - 4, 3, moss);
    img.draw_circle(body_cx - 8, body_cy + 10, 5, moss);

    // Arms
    img.draw_rect(body_cx - 30, body_cy - 10, 8, 24, outline);
    img.draw_rect(body_cx - 28, body_cy - 8, 4, 20, skin_dark);
    img.draw_rect(body_cx + 23, body_cy - 10, 8, 24, outline);
    img.draw_rect(body_cx + 25, body_cy - 8, 4, 20, skin_dark);
    // Fists
    img.draw_circle(body_cx - 26, body_cy + 14, 5, outline);
    img.draw_circle(body_cx - 26, body_cy + 14, 3, skin_dark);
    img.draw_circle(body_cx + 27, body_cy + 14, 5, outline);
    img.draw_circle(body_cx + 27, body_cy + 14, 3, skin_dark);

    // Legs
    img.draw_rect(body_cx - 16, body_cy + 16, 12, 14, outline);
    img.draw_rect(body_cx - 14, body_cy + 16, 8, 12, skin_dark);
    img.draw_rect(body_cx + 5, body_cy + 16, 12, 14, outline);
    img.draw_rect(body_cx + 7, body_cy + 16, 8, 12, skin_dark);

    img
}
