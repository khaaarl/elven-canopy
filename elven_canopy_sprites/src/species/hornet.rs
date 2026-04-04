// Giant hornet sprite generation (36x32).
//
// Top-down-ish view of a giant hornet: segmented body (head, thorax,
// abdomen) with characteristic yellow-and-black striping, translucent
// wings, six legs, antennae, and a prominent stinger. Trait-based
// variation in body color, stripe pattern, and wing style.
//
// See also: `species.rs` for the dispatcher.

use crate::color::Color;
use crate::drawing::PixelBuffer;

const BODY_COLORS: [Color; 4] = [
    Color::rgb(0.90, 0.80, 0.20), // bright-yellow
    Color::rgb(0.65, 0.45, 0.15), // dark-amber
    Color::rgb(0.85, 0.55, 0.15), // orange
    Color::rgb(0.75, 0.65, 0.25), // golden
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripePattern {
    /// Wide black bands.
    Thick,
    /// Narrow alternating stripes.
    Thin,
    /// Irregular, broken stripes.
    Spotted,
}

const STRIPE_PATTERNS: [StripePattern; 3] = [
    StripePattern::Thick,
    StripePattern::Thin,
    StripePattern::Spotted,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WingStyle {
    /// Short rounded wings.
    Short,
    /// Medium swept-back wings.
    Medium,
    /// Long narrow wings.
    Long,
}

const WING_STYLES: [WingStyle; 3] = [WingStyle::Short, WingStyle::Medium, WingStyle::Long];

#[derive(Clone, Debug, PartialEq)]
pub struct HornetParams {
    pub body_color: Color,
    pub stripe_pattern: StripePattern,
    pub wing_style: WingStyle,
}

pub fn params_from_traits(traits: &super::TraitMap) -> HornetParams {
    use elven_canopy_sim::types::TraitKind;
    let base_idx = super::trait_idx(traits, TraitKind::BodyColor, 0) % BODY_COLORS.len();
    let blend_target = super::trait_i64(traits, TraitKind::BodyBlendTarget, -1);
    let blend_weight = super::trait_i64(traits, TraitKind::BodyBlendWeight, 0);
    let value = super::trait_i64(traits, TraitKind::BodyValue, 0);
    let saturation = super::trait_i64(traits, TraitKind::BodySaturation, 0);
    HornetParams {
        body_color: super::resolve_hue(&BODY_COLORS, base_idx, blend_target, blend_weight)
            .apply_value(value)
            .apply_saturation(saturation),
        stripe_pattern: STRIPE_PATTERNS
            [super::trait_idx(traits, TraitKind::StripePattern, 0) % STRIPE_PATTERNS.len()],
        wing_style: WING_STYLES
            [super::trait_idx(traits, TraitKind::WingStyle, 0) % WING_STYLES.len()],
    }
}

pub fn create_sprite(p: &HornetParams) -> PixelBuffer {
    let mut img = PixelBuffer::new(36, 32);
    let body = p.body_color;
    let body_dark = body.darken(0.15);
    let black = Color::rgb(0.10, 0.08, 0.05);
    let outline = Color::rgb(0.08, 0.06, 0.04);
    let eye_color = Color::rgb(0.60, 0.10, 0.10); // red compound eyes
    let wing_color = Color::from_f32(0.70, 0.80, 0.90, 0.50); // translucent blue-white
    let wing_vein = Color::from_f32(0.40, 0.50, 0.60, 0.60);
    let leg_color = Color::rgb(0.20, 0.15, 0.10);
    let antenna_color = Color::rgb(0.15, 0.12, 0.08);
    let stinger_color = Color::rgb(0.15, 0.10, 0.05);

    let cx = 18; // center x
    let head_cy = 6;
    let thorax_cy = 13;
    let abdomen_cy = 22;

    // --- Head ---
    img.draw_circle(cx, head_cy, 5, outline);
    img.draw_circle(cx, head_cy, 4, black);

    // Compound eyes (reddish, on sides of head).
    img.draw_circle(cx - 3, head_cy - 1, 2, eye_color);
    img.draw_circle(cx + 3, head_cy - 1, 2, eye_color);

    // Mandibles
    img.set_px(cx - 2, head_cy + 4, body_dark);
    img.set_px(cx + 2, head_cy + 4, body_dark);
    img.set_px(cx - 3, head_cy + 5, body_dark);
    img.set_px(cx + 3, head_cy + 5, body_dark);

    // Antennae
    img.set_px(cx - 2, head_cy - 4, antenna_color);
    img.set_px(cx - 3, head_cy - 5, antenna_color);
    img.set_px(cx - 4, head_cy - 6, antenna_color);
    img.set_px(cx + 2, head_cy - 4, antenna_color);
    img.set_px(cx + 3, head_cy - 5, antenna_color);
    img.set_px(cx + 4, head_cy - 6, antenna_color);

    // --- Thorax ---
    img.draw_ellipse(cx, thorax_cy, 6, 4, outline);
    img.draw_ellipse(cx, thorax_cy, 5, 3, body);

    // --- Wings (behind thorax, drawn before abdomen for layering) ---
    let (wing_rx, wing_ry) = match p.wing_style {
        WingStyle::Short => (7, 4),
        WingStyle::Medium => (9, 5),
        WingStyle::Long => (11, 4),
    };
    // Left wing
    img.draw_ellipse(cx - 5, thorax_cy - 1, wing_rx, wing_ry, wing_color);
    // Wing veins
    img.draw_hline(cx - 5 - wing_rx + 2, cx - 5, thorax_cy - 1, wing_vein);
    // Right wing
    img.draw_ellipse(cx + 5, thorax_cy - 1, wing_rx, wing_ry, wing_color);
    img.draw_hline(cx + 5, cx + 5 + wing_rx - 2, thorax_cy - 1, wing_vein);

    // --- Abdomen (largest segment, with stripes) ---
    img.draw_ellipse(cx, abdomen_cy, 7, 8, outline);
    img.draw_ellipse(cx, abdomen_cy, 6, 7, body);

    // Stripe pattern on abdomen.
    match p.stripe_pattern {
        StripePattern::Thick => {
            // Two wide black bands.
            img.draw_hline(cx - 5, cx + 5, abdomen_cy - 3, black);
            img.draw_hline(cx - 5, cx + 5, abdomen_cy - 2, black);
            img.draw_hline(cx - 5, cx + 5, abdomen_cy + 2, black);
            img.draw_hline(cx - 5, cx + 5, abdomen_cy + 3, black);
        }
        StripePattern::Thin => {
            // Four thin black stripes.
            img.draw_hline(cx - 5, cx + 5, abdomen_cy - 4, black);
            img.draw_hline(cx - 5, cx + 5, abdomen_cy - 1, black);
            img.draw_hline(cx - 5, cx + 5, abdomen_cy + 2, black);
            img.draw_hline(cx - 5, cx + 5, abdomen_cy + 5, black);
        }
        StripePattern::Spotted => {
            // Broken/irregular bands.
            img.draw_hline(cx - 4, cx - 1, abdomen_cy - 3, black);
            img.draw_hline(cx + 1, cx + 4, abdomen_cy - 3, black);
            img.draw_hline(cx - 5, cx + 5, abdomen_cy, black);
            img.draw_hline(cx - 3, cx, abdomen_cy + 3, black);
            img.draw_hline(cx + 2, cx + 4, abdomen_cy + 4, black);
        }
    }

    // --- Stinger ---
    img.set_px(cx, abdomen_cy + 8, stinger_color);
    img.set_px(cx, abdomen_cy + 9, stinger_color);

    // --- Legs (3 pairs, extending from thorax) ---
    // Front pair
    img.set_px(cx - 6, thorax_cy, leg_color);
    img.set_px(cx - 7, thorax_cy + 1, leg_color);
    img.set_px(cx + 6, thorax_cy, leg_color);
    img.set_px(cx + 7, thorax_cy + 1, leg_color);
    // Middle pair
    img.set_px(cx - 6, thorax_cy + 2, leg_color);
    img.set_px(cx - 7, thorax_cy + 3, leg_color);
    img.set_px(cx - 8, thorax_cy + 4, leg_color);
    img.set_px(cx + 6, thorax_cy + 2, leg_color);
    img.set_px(cx + 7, thorax_cy + 3, leg_color);
    img.set_px(cx + 8, thorax_cy + 4, leg_color);
    // Rear pair
    img.set_px(cx - 5, thorax_cy + 4, leg_color);
    img.set_px(cx - 6, thorax_cy + 5, leg_color);
    img.set_px(cx - 7, thorax_cy + 6, leg_color);
    img.set_px(cx + 5, thorax_cy + 4, leg_color);
    img.set_px(cx + 6, thorax_cy + 5, leg_color);
    img.set_px(cx + 7, thorax_cy + 6, leg_color);

    img
}
