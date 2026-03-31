// RGBA color type for sprite generation.
//
// Stores color as four u8 channels (RGBA). Provides conversion helpers
// between u8 and f32 representations, plus darken/lighten operations
// matching the original GDScript behavior exactly.
//
// See also: `drawing.rs` for the pixel buffer that uses these colors.

/// An RGBA color with u8 channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create a color from f32 components (0.0–1.0), matching GDScript's
    /// `Color(r, g, b, a)` constructor.
    pub const fn from_f32(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: float_to_u8(r),
            g: float_to_u8(g),
            b: float_to_u8(b),
            a: float_to_u8(a),
        }
    }

    /// Create a fully opaque color from f32 RGB.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::from_f32(r, g, b, 1.0)
    }

    /// Create a color from u8 RGBA components.
    pub const fn from_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Convert from the sim's `ItemColor` (material-derived or dye color).
    pub const fn from_item_color(ic: elven_canopy_sim::inventory::ItemColor) -> Self {
        Self {
            r: ic.r,
            g: ic.g,
            b: ic.b,
            a: 255,
        }
    }

    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// Darken by subtracting `amount` from each RGB channel (in 0.0–1.0 space).
    /// Matches GDScript `_darken(color, amount)`.
    pub fn darken(self, amount: f32) -> Self {
        Self {
            r: sub_f32(self.r, amount),
            g: sub_f32(self.g, amount),
            b: sub_f32(self.b, amount),
            a: self.a,
        }
    }

    /// Lighten by adding `amount` to each RGB channel (in 0.0–1.0 space).
    /// Matches GDScript `_lighten(color, amount)`.
    pub fn lighten(self, amount: f32) -> Self {
        Self {
            r: add_f32(self.r, amount),
            g: add_f32(self.g, amount),
            b: add_f32(self.b, amount),
            a: self.a,
        }
    }

    /// Linearly interpolate between `self` and `other`.
    /// `weight` is 0–255: 0 = fully self, 255 = fully other.
    pub fn blend(self, other: Color, weight: u8) -> Self {
        let w = weight as u16;
        let inv = 255 - w;
        Self {
            r: ((self.r as u16 * inv + other.r as u16 * w + 127) / 255) as u8,
            g: ((self.g as u16 * inv + other.g as u16 * w + 127) / 255) as u8,
            b: ((self.b as u16 * inv + other.b as u16 * w + 127) / 255) as u8,
            a: self.a,
        }
    }

    /// Apply a genome-derived value axis (dark ↔ light) to this color.
    ///
    /// `trait_value` is centered on 0 with typical range ~[-150, +150].
    /// Positive = lighter, negative = darker. The effect is scaled so that
    /// ±100 corresponds to ~±0.15 in RGB space (noticeable but not extreme).
    pub fn apply_value(self, trait_value: i64) -> Self {
        // Scale: 100 trait units → 0.15 RGB shift.
        let amount = trait_value as f32 * 0.0015;
        if amount >= 0.0 {
            self.lighten(amount)
        } else {
            self.darken(-amount)
        }
    }

    /// Apply a genome-derived saturation axis (muted ↔ vivid) to this color.
    ///
    /// `trait_value` is centered on 0 with typical range ~[-150, +150].
    /// Positive = more vivid (push channels away from grey), negative = more
    /// muted (push channels toward grey). ±100 → ~±20% saturation shift.
    pub fn apply_saturation(self, trait_value: i64) -> Self {
        // Compute grey (average luminance).
        let grey = (self.r as u16 + self.g as u16 + self.b as u16) / 3;
        // Scale: 100 trait units → 0.20 interpolation toward/away from grey.
        let factor = (trait_value as f32 * 0.002).clamp(-0.8, 0.8);
        // Positive factor: push away from grey (more saturated).
        // Negative factor: push toward grey (more muted).
        // new = channel + (channel - grey) * factor
        let apply = |ch: u8| -> u8 {
            let diff = ch as f32 - grey as f32;
            let new = ch as f32 + diff * factor;
            new.clamp(0.0, 255.0) as u8
        };
        Self {
            r: apply(self.r),
            g: apply(self.g),
            b: apply(self.b),
            a: self.a,
        }
    }
}

impl From<elven_canopy_sim::inventory::ItemColor> for Color {
    fn from(ic: elven_canopy_sim::inventory::ItemColor) -> Self {
        Self::from_item_color(ic)
    }
}

/// Clamp and convert f32 (0.0–1.0) to u8 (0–255). Const-compatible.
const fn float_to_u8(v: f32) -> u8 {
    let clamped = if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    };
    (clamped * 255.0) as u8
}

/// Subtract `amount` (0.0–1.0) from a u8 channel, clamping.
fn sub_f32(val: u8, amount: f32) -> u8 {
    let v = val as f32 / 255.0 - amount;
    (v.clamp(0.0, 1.0) * 255.0) as u8
}

/// Add `amount` (0.0–1.0) to a u8 channel, clamping.
fn add_f32(val: u8, amount: f32) -> u8 {
    let v = val as f32 / 255.0 + amount;
    (v.clamp(0.0, 1.0) * 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_from_f32_roundtrip() {
        let c = Color::rgb(0.5, 0.0, 1.0);
        assert_eq!(c.r, 127);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 255);
        assert_eq!(c.a, 255);
    }

    #[test]
    fn darken_clamps_at_zero() {
        let c = Color::rgb(0.1, 0.0, 0.5);
        let d = c.darken(0.3);
        assert_eq!(d.g, 0); // can't go below 0
        assert!(d.r == 0 || d.r < c.r);
    }

    #[test]
    fn lighten_clamps_at_max() {
        let c = Color::rgb(0.9, 1.0, 0.5);
        let l = c.lighten(0.3);
        assert_eq!(l.g, 255); // can't go above 255
        assert!(l.r == 255 || l.r > c.r);
    }

    #[test]
    fn transparent_is_zero() {
        let c = Color::TRANSPARENT;
        assert_eq!(c.r, 0);
        assert_eq!(c.a, 0);
    }

    #[test]
    fn darken_preserves_alpha() {
        let c = Color::from_f32(0.5, 0.5, 0.5, 0.7);
        let d = c.darken(0.2);
        assert_eq!(d.a, c.a);
    }

    #[test]
    fn lighten_preserves_alpha() {
        let c = Color::from_f32(0.5, 0.5, 0.5, 0.4);
        let l = c.lighten(0.2);
        assert_eq!(l.a, c.a);
    }

    #[test]
    fn apply_value_positive_lightens() {
        let c = Color::rgb(0.5, 0.3, 0.2);
        let lighter = c.apply_value(100);
        assert!(
            lighter.r > c.r,
            "positive value should lighten: r {} > {}",
            lighter.r,
            c.r
        );
        assert!(
            lighter.g > c.g,
            "positive value should lighten: g {} > {}",
            lighter.g,
            c.g
        );
    }

    #[test]
    fn apply_value_negative_darkens() {
        let c = Color::rgb(0.5, 0.3, 0.2);
        let darker = c.apply_value(-100);
        assert!(
            darker.r < c.r,
            "negative value should darken: r {} < {}",
            darker.r,
            c.r
        );
    }

    #[test]
    fn apply_value_zero_unchanged() {
        let c = Color::rgb(0.5, 0.3, 0.7);
        let same = c.apply_value(0);
        assert_eq!(c, same);
    }

    #[test]
    fn apply_saturation_positive_vivifies() {
        // A color with unequal channels should become more vivid.
        let c = Color::from_u8(200, 100, 50, 255);
        let vivid = c.apply_saturation(100);
        // Higher channel should go higher, lower should go lower.
        assert!(vivid.r >= c.r, "vivid r should increase or stay");
        assert!(
            vivid.b <= c.b,
            "vivid b (below grey) should decrease or stay"
        );
    }

    #[test]
    fn apply_saturation_negative_mutes() {
        let c = Color::from_u8(200, 100, 50, 255);
        let muted = c.apply_saturation(-100);
        // Channels should move toward grey.
        let grey = (200 + 100 + 50) / 3; // ~116
        assert!(muted.r < c.r, "muted r should decrease toward grey");
        assert!(muted.b > c.b, "muted b should increase toward grey {grey}");
    }

    #[test]
    fn apply_saturation_preserves_alpha() {
        let c = Color::from_f32(0.5, 0.3, 0.7, 0.4);
        let s = c.apply_saturation(50);
        assert_eq!(s.a, c.a);
    }

    #[test]
    fn blend_weight_zero_returns_self() {
        let a = Color::rgb(0.8, 0.2, 0.4);
        let b = Color::rgb(0.2, 0.8, 0.6);
        let blended = a.blend(b, 0);
        assert_eq!(blended, a);
    }

    #[test]
    fn blend_weight_255_returns_other() {
        let a = Color::rgb(0.8, 0.2, 0.4);
        let b = Color::rgb(0.2, 0.8, 0.6);
        let blended = a.blend(b, 255);
        assert_eq!(blended.r, b.r);
        assert_eq!(blended.g, b.g);
        assert_eq!(blended.b, b.b);
    }

    #[test]
    fn blend_weight_128_midpoint() {
        let a = Color::from_u8(0, 0, 0, 255);
        let b = Color::from_u8(254, 254, 254, 255);
        let blended = a.blend(b, 128);
        // Midpoint of 0 and 254 should be approximately 127.
        assert!(
            (125..=129).contains(&blended.r),
            "midpoint blend r={}",
            blended.r
        );
    }

    #[test]
    fn blend_preserves_alpha_of_self() {
        let a = Color::from_u8(100, 100, 100, 200);
        let b = Color::from_u8(200, 200, 200, 50);
        let blended = a.blend(b, 128);
        assert_eq!(blended.a, 200, "blend should preserve self's alpha");
    }

    #[test]
    fn from_item_color() {
        let ic = elven_canopy_sim::inventory::ItemColor::new(100, 200, 50);
        let c = Color::from(ic);
        assert_eq!(c.r, 100);
        assert_eq!(c.g, 200);
        assert_eq!(c.b, 50);
        assert_eq!(c.a, 255);
    }
}
