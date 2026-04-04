// Wyvern sprite generation (96x80).
//
// Large flying predator viewed from above/side: elongated reptilian body,
// two massive bat-like wings, long neck with horned head, thick tail, and
// two hind legs. Covered in scales with color/pattern variation. Bigger
// and more fearsome than the giant hornet — occupies a 2×2×2 footprint.
//
// See also: `species.rs` for the dispatcher.

use crate::color::Color;
use crate::drawing::PixelBuffer;

const BODY_COLORS: [Color; 4] = [
    Color::rgb(0.25, 0.55, 0.30), // emerald
    Color::rgb(0.70, 0.25, 0.20), // crimson
    Color::rgb(0.35, 0.30, 0.65), // indigo
    Color::rgb(0.60, 0.48, 0.28), // bronze
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalePattern {
    /// Smooth overlapping scales.
    Smooth,
    /// Ridged armored plates.
    Ridged,
    /// Speckled pattern with lighter flecks.
    Speckled,
}

const SCALE_PATTERNS: [ScalePattern; 3] = [
    ScalePattern::Smooth,
    ScalePattern::Ridged,
    ScalePattern::Speckled,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HornStyle {
    /// Two curved horns.
    Curved,
    /// Single straight crest.
    Crest,
    /// Branching antler-like horns.
    Branching,
}

const HORN_STYLES: [HornStyle; 3] = [HornStyle::Curved, HornStyle::Crest, HornStyle::Branching];

#[derive(Clone, Debug, PartialEq)]
pub struct WyvernParams {
    pub body_color: Color,
    pub scale_pattern: ScalePattern,
    pub horn_style: HornStyle,
}

pub fn params_from_traits(traits: &super::TraitMap) -> WyvernParams {
    use elven_canopy_sim::types::TraitKind;
    let base_idx = super::trait_idx(traits, TraitKind::BodyColor, 0) % BODY_COLORS.len();
    let blend_target = super::trait_i64(traits, TraitKind::BodyBlendTarget, -1);
    let blend_weight = super::trait_i64(traits, TraitKind::BodyBlendWeight, 0);
    let value = super::trait_i64(traits, TraitKind::BodyValue, 0);
    let saturation = super::trait_i64(traits, TraitKind::BodySaturation, 0);
    WyvernParams {
        body_color: super::resolve_hue(&BODY_COLORS, base_idx, blend_target, blend_weight)
            .apply_value(value)
            .apply_saturation(saturation),
        scale_pattern: SCALE_PATTERNS
            [super::trait_idx(traits, TraitKind::ScalePattern, 0) % SCALE_PATTERNS.len()],
        horn_style: HORN_STYLES
            [super::trait_idx(traits, TraitKind::HornStyle, 0) % HORN_STYLES.len()],
    }
}

pub fn create_sprite(p: &WyvernParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(96, 80);
    let body = p.body_color;
    let body_dark = body.darken(0.15);
    let body_light = body.lighten(0.12);
    let belly = body.lighten(0.25);
    let outline = Color::rgb(0.08, 0.06, 0.04);
    let eye_color = Color::rgb(0.90, 0.60, 0.10); // amber eyes
    let wing_membrane = body.lighten(0.08);
    let wing_bone = body_dark;
    let claw_color = Color::rgb(0.20, 0.18, 0.15);
    let horn_color = Color::rgb(0.60, 0.55, 0.45);

    let cx = 48; // center x

    // --- Tail (drawn first, behind body) ---
    let tail_start_y = 58;
    for i in 0..18 {
        let tx = cx + i / 3;
        let ty = tail_start_y + i;
        let w = (5 - i / 4).max(1);
        img.draw_hline(tx - w, tx + w, ty, body_dark);
        if w > 1 {
            img.draw_hline(tx - w + 1, tx + w - 1, ty, body);
        }
    }

    // --- Wings (behind body, drawn before body for layering) ---
    // Left wing
    let wing_y = 30;
    img.draw_ellipse(cx - 28, wing_y, 22, 14, outline);
    img.draw_ellipse(cx - 28, wing_y, 21, 13, wing_membrane);
    // Wing bones (3 radiating lines from wing root)
    for i in 0..3 {
        let angle_x = -15 - i * 5;
        let angle_y = -8 + i * 6;
        img.draw_hline(cx - 8, cx - 8 + angle_x, wing_y + angle_y, wing_bone);
    }

    // Right wing
    img.draw_ellipse(cx + 28, wing_y, 22, 14, outline);
    img.draw_ellipse(cx + 28, wing_y, 21, 13, wing_membrane);
    for i in 0..3 {
        let angle_x = 15 + i * 5;
        let angle_y = -8 + i * 6;
        img.draw_hline(cx + 8, cx + 8 + angle_x, wing_y + angle_y, wing_bone);
    }

    // --- Body (main torso) ---
    img.draw_ellipse(cx, 38, 14, 20, outline);
    img.draw_ellipse(cx, 38, 13, 19, body);
    // Belly highlight
    img.draw_ellipse(cx, 42, 8, 10, belly);

    // Scale pattern on body
    match p.scale_pattern {
        ScalePattern::Smooth => {
            // Subtle horizontal lines suggesting overlapping scales.
            for y in (26..54).step_by(3) {
                img.draw_hline(cx - 10, cx + 10, y, body_dark);
            }
        }
        ScalePattern::Ridged => {
            // Dorsal ridge down the center + cross ridges.
            img.draw_vline(cx, 22, 55, body_dark);
            for y in (25..52).step_by(5) {
                img.draw_hline(cx - 6, cx + 6, y, body_dark);
            }
        }
        ScalePattern::Speckled => {
            // Scattered lighter flecks.
            for y in (24..54).step_by(4) {
                for x in (cx - 10..cx + 10).step_by(5) {
                    img.set_px(x, y, body_light);
                    img.set_px(x + 2, y + 2, body_light);
                }
            }
        }
    }

    // --- Neck ---
    let neck_top = 12;
    let neck_bot = 22;
    for y in neck_top..neck_bot {
        let w = 5 + (y - neck_top) / 2;
        img.draw_hline(cx - w, cx + w, y, outline);
        img.draw_hline(cx - w + 1, cx + w - 1, y, body);
    }

    // --- Head ---
    let head_cx = cx;
    let head_cy = 8;
    img.draw_ellipse(head_cx, head_cy, 8, 6, outline);
    img.draw_ellipse(head_cx, head_cy, 7, 5, body);

    // Snout
    img.draw_ellipse(head_cx, head_cy - 5, 4, 2, body_dark);

    // Eyes
    img.draw_circle(head_cx - 4, head_cy - 1, 2, eye_color);
    img.draw_circle(head_cx + 4, head_cy - 1, 2, eye_color);
    img.set_px(head_cx - 4, head_cy - 1, outline); // pupil
    img.set_px(head_cx + 4, head_cy - 1, outline);

    // Nostrils
    img.set_px(head_cx - 2, head_cy - 6, outline);
    img.set_px(head_cx + 2, head_cy - 6, outline);

    // --- Horns ---
    match p.horn_style {
        HornStyle::Curved => {
            // Two curved horns sweeping back.
            for i in 0..6 {
                img.set_px(head_cx - 6 - i, head_cy - 3 + i / 2, horn_color);
                img.set_px(head_cx + 6 + i, head_cy - 3 + i / 2, horn_color);
            }
        }
        HornStyle::Crest => {
            // Single dorsal crest ridge.
            for i in 0..8 {
                img.set_px(head_cx, head_cy - 6 - i, horn_color);
                if i > 2 {
                    img.set_px(head_cx - 1, head_cy - 6 - i, horn_color);
                    img.set_px(head_cx + 1, head_cy - 6 - i, horn_color);
                }
            }
        }
        HornStyle::Branching => {
            // Branching antler-like horns.
            for i in 0..5 {
                img.set_px(head_cx - 5 - i, head_cy - 4 - i, horn_color);
                img.set_px(head_cx + 5 + i, head_cy - 4 - i, horn_color);
            }
            // Branches
            img.set_px(head_cx - 8, head_cy - 5, horn_color);
            img.set_px(head_cx - 9, head_cy - 4, horn_color);
            img.set_px(head_cx + 8, head_cy - 5, horn_color);
            img.set_px(head_cx + 9, head_cy - 4, horn_color);
        }
    }

    // --- Hind legs ---
    let leg_y = 56;
    // Left leg
    img.draw_rect(cx - 10, leg_y, 6, 10, outline);
    img.draw_rect(cx - 9, leg_y + 1, 4, 8, body_dark);
    // Claws
    img.set_px(cx - 10, leg_y + 10, claw_color);
    img.set_px(cx - 8, leg_y + 10, claw_color);
    img.set_px(cx - 6, leg_y + 10, claw_color);

    // Right leg
    img.draw_rect(cx + 4, leg_y, 6, 10, outline);
    img.draw_rect(cx + 5, leg_y + 1, 4, 8, body_dark);
    img.set_px(cx + 4, leg_y + 10, claw_color);
    img.set_px(cx + 6, leg_y + 10, claw_color);
    img.set_px(cx + 8, leg_y + 10, claw_color);

    img
}
