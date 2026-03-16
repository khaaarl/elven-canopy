// Monkey sprite generation (40x44).
//
// Chibi-style monkey with big round head, expressive eyes, round ears,
// long dangling arms with hands, and a curly sinusoidal tail. Face marking
// variants: plain, light muzzle, eye patches.
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const FUR_COLORS: [Color; 4] = [
    Color::rgb(0.55, 0.38, 0.22),
    Color::rgb(0.70, 0.52, 0.30),
    Color::rgb(0.42, 0.30, 0.18),
    Color::rgb(0.62, 0.45, 0.25),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceMarking {
    Plain,
    LightMuzzle,
    EyePatches,
}

const FACE_MARKINGS: [FaceMarking; 3] = [
    FaceMarking::Plain,
    FaceMarking::LightMuzzle,
    FaceMarking::EyePatches,
];

#[derive(Clone, Debug)]
pub struct MonkeyParams {
    pub fur_color: Color,
    pub face_marking: FaceMarking,
}

pub fn params_from_seed(seed: i64) -> MonkeyParams {
    let h = knuth_hash(seed);
    MonkeyParams {
        fur_color: FUR_COLORS[(h % 4) as usize],
        face_marking: FACE_MARKINGS[((h / 19) % 3) as usize],
    }
}

pub fn create_sprite(p: &MonkeyParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(40, 44);
    let fur_color = p.fur_color;
    let fur_dark = fur_color.darken(0.12);
    let fur_light = fur_color.lighten(0.10);
    let outline = Color::rgb(0.15, 0.12, 0.10);
    let face_color = Color::rgb(0.85, 0.70, 0.55);
    let eye_color = Color::rgb(0.10, 0.08, 0.06);
    let white = Color::rgb(1.0, 1.0, 1.0);
    let mouth_color = Color::rgb(0.70, 0.40, 0.35);

    // Head
    let head_cx = 20;
    let head_cy = 12;
    img.draw_circle(head_cx, head_cy, 10, outline);
    img.draw_circle(head_cx, head_cy, 9, fur_color);

    // Face area
    img.draw_ellipse(head_cx, head_cy + 1, 6, 6, face_color);

    // Face markings
    match p.face_marking {
        FaceMarking::LightMuzzle => {
            img.draw_ellipse(head_cx, head_cy + 3, 4, 3, face_color.lighten(0.15));
        }
        FaceMarking::EyePatches => {
            img.draw_circle(head_cx - 4, head_cy - 1, 2, fur_dark);
            img.draw_circle(head_cx + 4, head_cy - 1, 2, fur_dark);
        }
        FaceMarking::Plain => {}
    }

    // Eyes
    img.draw_rect(head_cx - 6, head_cy - 2, 4, 4, outline);
    img.draw_rect(head_cx - 5, head_cy - 1, 2, 2, eye_color);
    img.set_px(head_cx - 5, head_cy - 1, white);
    img.draw_rect(head_cx + 3, head_cy - 2, 4, 4, outline);
    img.draw_rect(head_cx + 4, head_cy - 1, 2, 2, eye_color);
    img.set_px(head_cx + 4, head_cy - 1, white);

    // Nose and mouth
    img.set_px(head_cx - 1, head_cy + 3, face_color.darken(0.2));
    img.set_px(head_cx + 1, head_cy + 3, face_color.darken(0.2));
    img.draw_hline(head_cx - 2, head_cx + 2, head_cy + 5, mouth_color);

    // Ears
    img.draw_circle(head_cx - 9, head_cy - 3, 3, outline);
    img.draw_circle(head_cx - 9, head_cy - 3, 2, fur_color);
    img.set_px(head_cx - 9, head_cy - 3, face_color);
    img.draw_circle(head_cx + 9, head_cy - 3, 3, outline);
    img.draw_circle(head_cx + 9, head_cy - 3, 2, fur_color);
    img.set_px(head_cx + 9, head_cy - 3, face_color);

    // Body
    let body_top = 22;
    for y in body_top..(body_top + 10) {
        img.draw_hline(head_cx - 6, head_cx + 6, y, fur_color);
    }
    img.draw_ellipse(head_cx, body_top + 5, 4, 3, fur_light);

    // Arms
    img.draw_rect(head_cx - 9, body_top, 3, 10, outline);
    img.draw_rect(head_cx - 8, body_top, 1, 9, fur_color);
    img.draw_circle(head_cx - 8, body_top + 10, 2, face_color);
    img.draw_rect(head_cx + 7, body_top, 3, 10, outline);
    img.draw_rect(head_cx + 8, body_top, 1, 9, fur_color);
    img.draw_circle(head_cx + 8, body_top + 10, 2, face_color);

    // Legs
    let leg_y = body_top + 10;
    img.draw_rect(head_cx - 5, leg_y, 4, 5, outline);
    img.draw_rect(head_cx - 4, leg_y, 2, 4, fur_dark);
    img.draw_rect(head_cx + 2, leg_y, 4, 5, outline);
    img.draw_rect(head_cx + 3, leg_y, 2, 4, fur_dark);

    // Curly tail
    for i in 0..8 {
        let tx = head_cx + 7 + i;
        let ty = body_top + 3 + (f32::sin(i as f32 * 0.8) * 2.0) as i32;
        img.set_px(tx, ty, fur_dark);
        img.set_px(tx, ty + 1, fur_dark);
    }

    img
}
