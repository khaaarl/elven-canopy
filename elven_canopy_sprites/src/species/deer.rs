// Deer sprite generation (44x44).
//
// Elegant side-facing deer with antler style variants (simple, branched, wide),
// optional spot pattern, slender legs with hooves, and a fluffy tail.
//
// See also: `species.rs` for the dispatcher.

use super::knuth_hash;
use crate::color::Color;
use crate::drawing::PixelBuffer;

const BODY_COLORS: [Color; 4] = [
    Color::rgb(0.72, 0.55, 0.35),
    Color::rgb(0.65, 0.48, 0.30),
    Color::rgb(0.80, 0.62, 0.40),
    Color::rgb(0.58, 0.42, 0.28),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AntlerStyle {
    Simple,
    Branched,
    Wide,
}

const ANTLER_STYLES: [AntlerStyle; 3] = [
    AntlerStyle::Simple,
    AntlerStyle::Branched,
    AntlerStyle::Wide,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotPattern {
    None,
    Spotted,
}

const SPOT_PATTERNS: [SpotPattern; 2] = [SpotPattern::None, SpotPattern::Spotted];

#[derive(Clone, Debug)]
pub struct DeerParams {
    pub body_color: Color,
    pub antler_style: AntlerStyle,
    pub spot_pattern: SpotPattern,
    pub seed: i64,
}

pub fn params_from_seed(seed: i64) -> DeerParams {
    let h = knuth_hash(seed);
    DeerParams {
        body_color: BODY_COLORS[(h % 4) as usize],
        antler_style: ANTLER_STYLES[((h / 11) % 3) as usize],
        spot_pattern: SPOT_PATTERNS[((h / 41) % 2) as usize],
        seed,
    }
}

pub fn params_from_traits(traits: &super::TraitMap) -> DeerParams {
    use elven_canopy_sim::types::TraitKind;
    let bio_seed = traits
        .get(&TraitKind::BioSeed)
        .and_then(|v| match v {
            elven_canopy_sim::types::TraitValue::Int(i) => Some(*i),
            _ => None,
        })
        .unwrap_or(0);
    DeerParams {
        body_color: BODY_COLORS
            [super::trait_idx(traits, TraitKind::BodyColor, 0) % BODY_COLORS.len()],
        antler_style: ANTLER_STYLES
            [super::trait_idx(traits, TraitKind::AntlerStyle, 0) % ANTLER_STYLES.len()],
        spot_pattern: SPOT_PATTERNS
            [super::trait_idx(traits, TraitKind::SpotPattern, 0) % SPOT_PATTERNS.len()],
        seed: bio_seed,
    }
}

pub fn create_sprite(p: &DeerParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(44, 44);
    let body_color = p.body_color;
    let body_dark = body_color.darken(0.10);
    let body_light = body_color.lighten(0.12);
    let outline = Color::rgb(0.18, 0.14, 0.10);
    let eye_color = Color::rgb(0.10, 0.08, 0.06);
    let white = Color::rgb(1.0, 1.0, 1.0);
    let nose_color = Color::rgb(0.30, 0.22, 0.18);
    let antler_color = Color::rgb(0.55, 0.40, 0.25);

    // Body
    let body_cx = 24;
    let body_cy = 26;
    img.draw_ellipse(body_cx, body_cy, 15, 9, outline);
    img.draw_ellipse(body_cx, body_cy, 14, 8, body_color);
    img.draw_ellipse(body_cx + 1, body_cy + 1, 10, 5, body_light);

    // Spots
    if p.spot_pattern == SpotPattern::Spotted {
        let spot_color = body_color.lighten(0.20);
        let sh = p.seed.unsigned_abs();
        for si in 0..5u64 {
            let sx = body_cx - 8 + (sh.wrapping_add(si.wrapping_mul(37)) % 16) as i32;
            let sy = body_cy - 4 + (sh.wrapping_add(si.wrapping_mul(53)) % 8) as i32;
            img.draw_circle(sx, sy, 1, spot_color);
        }
    }

    // Head
    let head_cx = 8;
    let head_cy = 16;
    img.draw_ellipse(head_cx, head_cy, 7, 8, outline);
    img.draw_ellipse(head_cx, head_cy, 6, 7, body_color);
    img.draw_ellipse(head_cx - 1, head_cy + 1, 4, 4, body_light);

    // Nose
    img.draw_ellipse(3, head_cy + 5, 2, 1, nose_color);

    // Eyes
    img.draw_rect(head_cx - 3, head_cy - 3, 3, 3, eye_color);
    img.set_px(head_cx - 3, head_cy - 3, white);
    img.set_px(head_cx - 2, head_cy - 3, white);

    // Ears
    for i in 0..4 {
        img.set_px(head_cx - 5 - i, head_cy - 7 - i, body_dark);
        img.set_px(head_cx - 4 - i, head_cy - 7 - i, body_color);
    }
    for i in 0..4 {
        img.set_px(head_cx + 2 + i, head_cy - 7 - i, body_dark);
        img.set_px(head_cx + 1 + i, head_cy - 7 - i, body_color);
    }

    // Antlers
    match p.antler_style {
        AntlerStyle::Simple => {
            img.draw_vline(head_cx - 2, head_cy - 12, head_cy - 7, antler_color);
            img.draw_vline(head_cx + 2, head_cy - 12, head_cy - 7, antler_color);
            img.set_px(head_cx - 3, head_cy - 11, antler_color);
            img.set_px(head_cx + 3, head_cy - 11, antler_color);
        }
        AntlerStyle::Branched => {
            img.draw_vline(head_cx - 2, head_cy - 13, head_cy - 7, antler_color);
            img.draw_vline(head_cx + 2, head_cy - 13, head_cy - 7, antler_color);
            img.set_px(head_cx - 4, head_cy - 10, antler_color);
            img.set_px(head_cx - 3, head_cy - 10, antler_color);
            img.set_px(head_cx + 4, head_cy - 10, antler_color);
            img.set_px(head_cx + 3, head_cy - 10, antler_color);
            img.set_px(head_cx - 3, head_cy - 12, antler_color);
            img.set_px(head_cx + 3, head_cy - 12, antler_color);
        }
        AntlerStyle::Wide => {
            img.draw_vline(head_cx - 3, head_cy - 11, head_cy - 7, antler_color);
            img.draw_vline(head_cx + 3, head_cy - 11, head_cy - 7, antler_color);
            for i in 0..4 {
                img.set_px(head_cx - 3 - i, head_cy - 11 + i / 2, antler_color);
                img.set_px(head_cx + 3 + i, head_cy - 11 + i / 2, antler_color);
            }
        }
    }

    // Legs
    let leg_y = body_cy + 7;
    img.draw_rect(14, leg_y, 3, 7, outline);
    img.draw_rect(15, leg_y, 1, 6, body_dark);
    img.draw_rect(19, leg_y, 3, 7, outline);
    img.draw_rect(20, leg_y, 1, 6, body_dark);
    img.draw_rect(29, leg_y, 3, 7, outline);
    img.draw_rect(30, leg_y, 1, 6, body_dark);
    img.draw_rect(34, leg_y, 3, 7, outline);
    img.draw_rect(35, leg_y, 1, 6, body_dark);

    // Hooves
    let hoof_color = Color::rgb(0.25, 0.18, 0.12);
    img.draw_rect(14, leg_y + 6, 3, 2, hoof_color);
    img.draw_rect(19, leg_y + 6, 3, 2, hoof_color);
    img.draw_rect(29, leg_y + 6, 3, 2, hoof_color);
    img.draw_rect(34, leg_y + 6, 3, 2, hoof_color);

    // Tail
    img.draw_circle(39, body_cy - 4, 2, body_light);

    img
}
