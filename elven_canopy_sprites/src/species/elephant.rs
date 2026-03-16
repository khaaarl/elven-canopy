// Elephant sprite generation (96x80).
//
// Large gray body with overlapping head, big flapping ears with inner color,
// a long curving trunk, optional tusks (short, long, none), four thick legs,
// and a short tail.
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const BODY_COLORS: [Color; 4] = [
    Color::rgb(0.55, 0.53, 0.50),
    Color::rgb(0.48, 0.45, 0.42),
    Color::rgb(0.62, 0.58, 0.55),
    Color::rgb(0.50, 0.47, 0.45),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuskType {
    Short,
    Long,
    None,
}

const TUSK_TYPES: [TuskType; 3] = [TuskType::Short, TuskType::Long, TuskType::None];

#[derive(Clone, Debug)]
pub struct ElephantParams {
    pub body_color: Color,
    pub tusk_type: TuskType,
}

pub fn params_from_seed(seed: i64) -> ElephantParams {
    let h = knuth_hash(seed);
    ElephantParams {
        body_color: BODY_COLORS[(h % 4) as usize],
        tusk_type: TUSK_TYPES[((h / 17) % 3) as usize],
    }
}

pub fn create_sprite(p: &ElephantParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(96, 80);
    let body_color = p.body_color;
    let body_dark = body_color.darken(0.10);
    let body_light = body_color.lighten(0.10);
    let outline = Color::rgb(0.20, 0.18, 0.16);
    let eye_color = Color::rgb(0.10, 0.08, 0.06);
    let white = Color::rgb(1.0, 1.0, 1.0);
    let tusk_color = Color::rgb(0.92, 0.88, 0.80);
    let inner_ear = Color::rgb(0.65, 0.50, 0.48);

    // Body
    let body_cx = 48;
    let body_cy = 44;
    img.draw_ellipse(body_cx, body_cy, 28, 20, outline);
    img.draw_ellipse(body_cx, body_cy, 26, 18, body_color);
    img.draw_ellipse(body_cx, body_cy + 2, 20, 12, body_light);

    // Head
    let head_cx = 22;
    let head_cy = 28;
    img.draw_circle(head_cx, head_cy, 18, outline);
    img.draw_circle(head_cx, head_cy, 16, body_color);

    // Big ears
    img.draw_ellipse(head_cx - 16, head_cy - 2, 8, 14, outline);
    img.draw_ellipse(head_cx - 16, head_cy - 2, 6, 12, body_dark);
    img.draw_ellipse(head_cx - 16, head_cy - 2, 4, 8, inner_ear);
    img.draw_ellipse(head_cx + 16, head_cy - 2, 8, 14, outline);
    img.draw_ellipse(head_cx + 16, head_cy - 2, 6, 12, body_dark);
    img.draw_ellipse(head_cx + 16, head_cy - 2, 4, 8, inner_ear);

    // Eyes
    img.draw_rect(head_cx - 8, head_cy - 4, 6, 6, outline);
    img.draw_rect(head_cx - 6, head_cy - 2, 2, 2, eye_color);
    img.draw_rect(head_cx - 6, head_cy - 4, 2, 2, white);
    img.draw_rect(head_cx + 4, head_cy - 4, 6, 6, outline);
    img.draw_rect(head_cx + 6, head_cy - 2, 2, 2, eye_color);
    img.draw_rect(head_cx + 6, head_cy - 4, 2, 2, white);

    // Trunk
    for i in 0..20 {
        let tx = head_cx - 2;
        let ty = head_cy + 10 + i;
        img.draw_rect(tx, ty, 6, 1, outline);
        img.draw_rect(tx + 1, ty, 4, 1, body_color);
    }
    img.draw_rect(head_cx + 2, head_cy + 28, 2, 2, outline);
    img.draw_rect(head_cx + 4, head_cy + 28, 2, 2, outline);

    // Tusks
    match p.tusk_type {
        TuskType::Short => {
            img.draw_rect(head_cx - 6, head_cy + 12, 4, 8, tusk_color);
            img.draw_rect(head_cx + 4, head_cy + 12, 4, 8, tusk_color);
        }
        TuskType::Long => {
            img.draw_rect(head_cx - 6, head_cy + 10, 4, 14, tusk_color);
            img.draw_rect(head_cx + 4, head_cy + 10, 4, 14, tusk_color);
        }
        TuskType::None => {}
    }

    // Thick legs
    img.draw_rect(body_cx - 20, 60, 10, 16, outline);
    img.draw_rect(body_cx - 18, 60, 6, 14, body_dark);
    img.draw_rect(body_cx - 4, 60, 10, 16, outline);
    img.draw_rect(body_cx - 2, 60, 6, 14, body_dark);
    img.draw_rect(body_cx + 8, 60, 10, 16, outline);
    img.draw_rect(body_cx + 10, 60, 6, 14, body_dark);
    img.draw_rect(body_cx + 20, 60, 10, 16, outline);
    img.draw_rect(body_cx + 22, 60, 6, 14, body_dark);

    // Short tail
    img.draw_rect(body_cx + 26, 36, 4, 6, body_dark);
    img.draw_rect(body_cx + 28, 42, 2, 2, outline);

    img
}
