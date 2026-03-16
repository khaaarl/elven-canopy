// Chibi elf sprite generation (48x48).
//
// Generates a deterministic chibi-style elf with hair color/style, eye color,
// skin tone, and role-based outfit. Roles: warrior, mage, archer, healer, bard.
// Each role has distinct outfit colors and accessories (headband, pointed cap,
// quiver, circlet, feathered cap).
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const HAIR_COLORS: [Color; 7] = [
    Color::rgb(0.95, 0.85, 0.40), // blonde
    Color::rgb(0.85, 0.30, 0.20), // red
    Color::rgb(0.20, 0.65, 0.30), // forest green
    Color::rgb(0.35, 0.50, 0.90), // blue
    Color::rgb(0.82, 0.82, 0.88), // silver
    Color::rgb(0.50, 0.30, 0.15), // brown
    Color::rgb(0.90, 0.50, 0.70), // pink
];

const EYE_COLORS: [Color; 5] = [
    Color::rgb(0.30, 0.50, 0.90), // blue
    Color::rgb(0.25, 0.70, 0.35), // green
    Color::rgb(0.85, 0.65, 0.20), // amber
    Color::rgb(0.60, 0.30, 0.80), // violet
    Color::rgb(0.45, 0.30, 0.20), // brown
];

const SKIN_TONES: [Color; 4] = [
    Color::rgb(0.93, 0.80, 0.65), // fair
    Color::rgb(0.85, 0.70, 0.55), // light
    Color::rgb(0.72, 0.55, 0.40), // medium
    Color::rgb(0.55, 0.38, 0.25), // dark
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HairStyle {
    StraightBangs,
    SideSwept,
    Wild,
}

const HAIR_STYLES: [HairStyle; 3] = [
    HairStyle::StraightBangs,
    HairStyle::SideSwept,
    HairStyle::Wild,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Warrior,
    Mage,
    Archer,
    Healer,
    Bard,
}

const ROLES: [Role; 5] = [
    Role::Warrior,
    Role::Mage,
    Role::Archer,
    Role::Healer,
    Role::Bard,
];

/// Outfit base + dark color per role.
fn role_outfit_colors(role: Role) -> (Color, Color) {
    match role {
        Role::Warrior => (Color::rgb(0.55, 0.20, 0.15), Color::rgb(0.40, 0.15, 0.10)),
        Role::Mage => (Color::rgb(0.25, 0.20, 0.65), Color::rgb(0.15, 0.12, 0.50)),
        Role::Archer => (Color::rgb(0.20, 0.50, 0.20), Color::rgb(0.15, 0.38, 0.15)),
        Role::Healer => (Color::rgb(0.90, 0.90, 0.85), Color::rgb(0.75, 0.75, 0.70)),
        Role::Bard => (Color::rgb(0.80, 0.55, 0.15), Color::rgb(0.65, 0.40, 0.10)),
    }
}

#[derive(Clone, Debug)]
pub struct ElfParams {
    pub hair_color: Color,
    pub eye_color: Color,
    pub skin_tone: Color,
    pub hair_style: HairStyle,
    pub role: Role,
}

pub fn params_from_seed(seed: i64) -> ElfParams {
    let h = knuth_hash(seed);
    ElfParams {
        hair_color: HAIR_COLORS[(h % 7) as usize],
        eye_color: EYE_COLORS[((h / 7) % 5) as usize],
        skin_tone: SKIN_TONES[((h / 31) % 4) as usize],
        hair_style: HAIR_STYLES[((h / 131) % 3) as usize],
        role: ROLES[((h / 541) % 5) as usize],
    }
}

pub fn create_sprite(p: &ElfParams) -> PixelBuffer {
    let (w, h) = (48i32, 48i32);
    let mut img = PixelBuffer::new(w as u32, h as u32);

    let skin = p.skin_tone;
    let hair = p.hair_color;
    let eyes = p.eye_color;
    let style = p.hair_style;
    let role = p.role;

    let (outfit, outfit_dark) = role_outfit_colors(role);
    let skin_dark = skin.darken(0.12);
    let hair_dark = hair.darken(0.15);
    let outline = Color::rgb(0.15, 0.12, 0.10);
    let white = Color::rgb(1.0, 1.0, 1.0);
    let black = Color::rgb(0.08, 0.06, 0.06);
    let mouth = Color::rgb(0.75, 0.40, 0.40);
    let boot_color = Color::rgb(0.35, 0.22, 0.12);
    let belt_color = outfit.darken(0.20);

    let cx = w / 2; // 24

    // 1. Hair back layer
    match style {
        HairStyle::StraightBangs => {
            img.draw_ellipse(cx, 16, 13, 14, hair_dark);
        }
        HairStyle::SideSwept => {
            img.draw_ellipse(cx + 2, 16, 14, 13, hair_dark);
        }
        HairStyle::Wild => {
            img.draw_ellipse(cx, 15, 14, 15, hair_dark);
            img.draw_circle(cx - 10, 8, 4, hair_dark);
            img.draw_circle(cx + 10, 8, 4, hair_dark);
            img.draw_circle(cx - 7, 3, 3, hair_dark);
            img.draw_circle(cx + 7, 3, 3, hair_dark);
        }
    }

    // 2. Head
    let head_cy = 14;
    let head_r = 11;
    img.draw_circle(cx, head_cy, head_r + 1, outline);
    img.draw_circle(cx, head_cy, head_r, skin);
    // Cheek blush
    let blush = Color::from_f32(0.90, 0.60, 0.55, 0.45);
    img.draw_ellipse(cx - 7, 18, 3, 2, blush);
    img.draw_ellipse(cx + 7, 18, 3, 2, blush);

    // 3. Pointed elf ears
    for i in 0..5 {
        img.set_px(cx - head_r - 1 - i, head_cy - 2 - i, outline);
        img.set_px(cx - head_r - i, head_cy - 1 - i, skin);
        img.set_px(cx - head_r - i, head_cy - i, skin);
        img.set_px(cx + head_r + 1 + i, head_cy - 2 - i, outline);
        img.set_px(cx + head_r + i, head_cy - 1 - i, skin);
        img.set_px(cx + head_r + i, head_cy - i, skin);
    }

    // 4. Big anime eyes
    let eye_y = head_cy - 1;
    let left_eye_x = cx - 6;
    let right_eye_x = cx + 2;

    for ex in 0..5 {
        for ey in 0..5 {
            if ey == 0 || ey == 4 || ex == 0 || ex == 4 {
                img.set_px(left_eye_x + ex, eye_y + ey, outline);
                img.set_px(right_eye_x + ex, eye_y + ey, outline);
            } else {
                img.set_px(left_eye_x + ex, eye_y + ey, eyes);
                img.set_px(right_eye_x + ex, eye_y + ey, eyes);
            }
        }
    }

    // Pupils
    for px in 0..2 {
        for py in 0..2 {
            img.set_px(left_eye_x + 2 + px, eye_y + 2 + py, black);
            img.set_px(right_eye_x + 2 + px, eye_y + 2 + py, black);
        }
    }

    // White highlights
    img.set_px(left_eye_x + 1, eye_y + 1, white);
    img.set_px(right_eye_x + 1, eye_y + 1, white);

    // 5. Tiny mouth
    img.draw_hline(cx - 1, cx + 1, head_cy + 6, mouth);

    // 6. Hair front layer (bangs)
    match style {
        HairStyle::StraightBangs => {
            img.draw_rect(cx - 10, 3, 20, 7, hair);
            img.draw_ellipse(cx, 5, 11, 5, hair);
            let mut i = -9;
            while i < 10 {
                img.set_px(cx + i, 10, hair);
                img.set_px(cx + i + 1, 11, hair);
                i += 3;
            }
        }
        HairStyle::SideSwept => {
            img.draw_ellipse(cx + 1, 5, 11, 5, hair);
            for i in 0..10 {
                img.draw_hline(cx - 10 + i, cx + 10, 4 + i / 3, hair);
                if 4 + i / 3 > 8 {
                    break;
                }
            }
            img.draw_rect(cx - 10, 3, 22, 6, hair);
            img.draw_circle(cx + 11, 8, 3, hair);
        }
        HairStyle::Wild => {
            img.draw_ellipse(cx, 5, 12, 5, hair);
            let mut spike = -8;
            while spike <= 8 {
                img.draw_vline(cx + spike, 0, 6, hair);
                img.set_px(cx + spike - 1, 1, hair);
                img.set_px(cx + spike + 1, 1, hair);
                spike += 4;
            }
        }
    }

    // 7. Body / outfit
    let body_top = 25;
    let body_bot = 36;

    match role {
        Role::Warrior => {
            for y in body_top..=body_bot {
                let hw = if y < body_top + 3 { 9 } else { 7 };
                img.draw_hline(cx - hw, cx + hw, y, outfit);
            }
            img.draw_ellipse(cx - 9, body_top + 1, 3, 2, outfit_dark);
            img.draw_ellipse(cx + 9, body_top + 1, 3, 2, outfit_dark);
        }
        Role::Mage => {
            for y in body_top..=(body_bot + 2) {
                let hw = 6 + (y - body_top) / 3;
                img.draw_hline(cx - hw, cx + hw, y, outfit);
            }
            img.draw_hline(cx - 10, cx + 10, body_bot + 2, outfit.lighten(0.3));
        }
        Role::Archer => {
            for y in body_top..=body_bot {
                img.draw_hline(cx - 7, cx + 7, y, outfit);
            }
        }
        Role::Healer => {
            for y in body_top..=(body_bot + 1) {
                let hw = 6 + (y - body_top) / 3;
                img.draw_hline(cx - hw, cx + hw, y, outfit);
            }
            let red_cross = Color::rgb(0.85, 0.20, 0.20);
            img.draw_vline(cx, body_top + 2, body_top + 6, red_cross);
            img.draw_hline(cx - 2, cx + 2, body_top + 4, red_cross);
        }
        Role::Bard => {
            for y in body_top..=body_bot {
                let c = if (y - body_top) % 4 < 2 {
                    outfit
                } else {
                    outfit.lighten(0.15)
                };
                img.draw_hline(cx - 7, cx + 7, y, c);
            }
        }
    }

    // 8. Belt / sash
    img.draw_hline(cx - 7, cx + 7, body_top + 6, belt_color);
    img.draw_hline(cx - 7, cx + 7, body_top + 7, belt_color);
    let buckle = belt_color.lighten(0.4);
    img.set_px(cx, body_top + 6, buckle);
    img.set_px(cx, body_top + 7, buckle);

    // 9. Stubby arms + hands
    img.draw_rect(cx - 10, body_top + 2, 3, 7, skin);
    img.draw_rect(cx - 11, body_top + 2, 1, 7, outline);
    img.draw_rect(cx - 10, body_top + 9, 3, 2, skin);
    img.draw_rect(cx + 8, body_top + 2, 3, 7, skin);
    img.draw_rect(cx + 11, body_top + 2, 1, 7, outline);
    img.draw_rect(cx + 8, body_top + 9, 3, 2, skin);

    // 10. Short legs + chunky boots
    let leg_top = body_bot + 1;
    let leg_bot = 42;
    let boot_top = 43;

    img.draw_rect(cx - 5, leg_top, 4, leg_bot - leg_top, skin_dark);
    img.draw_rect(cx + 2, leg_top, 4, leg_bot - leg_top, skin_dark);
    img.draw_rect(cx - 6, boot_top, 6, 5, boot_color);
    img.draw_rect(cx + 1, boot_top, 6, 5, boot_color);

    // 11. Accessories by role
    match role {
        Role::Warrior => {
            let headband = Color::rgb(0.80, 0.15, 0.10);
            img.draw_hline(cx - 10, cx + 10, 8, headband);
            img.draw_hline(cx - 10, cx + 10, 9, headband);
        }
        Role::Mage => {
            for i in 0..5 {
                img.draw_hline(cx - 4 + i, cx + 4 - i, i, outfit.lighten(0.1));
            }
            img.set_px(cx, 2, Color::rgb(1.0, 0.95, 0.30));
        }
        Role::Archer => {
            let quiver = Color::rgb(0.50, 0.35, 0.15);
            img.draw_vline(cx + 10, body_top - 2, body_top + 8, quiver);
            img.draw_vline(cx + 11, body_top - 3, body_top + 7, quiver);
            let arrow_tip = Color::rgb(0.70, 0.70, 0.70);
            img.set_px(cx + 10, body_top - 3, arrow_tip);
            img.set_px(cx + 11, body_top - 4, arrow_tip);
        }
        Role::Healer => {
            let circlet = Color::rgb(0.90, 0.85, 0.30);
            img.draw_hline(cx - 8, cx + 8, 5, circlet);
            img.set_px(cx, 4, Color::rgb(0.30, 0.80, 0.90));
        }
        Role::Bard => {
            let feather = Color::rgb(0.85, 0.25, 0.25);
            img.draw_circle(cx + 8, 3, 2, feather);
            img.draw_vline(cx + 8, 0, 2, feather);
            img.set_px(cx + 9, 0, feather);
        }
    }

    img
}
