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
    fn from_item_color() {
        let ic = elven_canopy_sim::inventory::ItemColor::new(100, 200, 50);
        let c = Color::from(ic);
        assert_eq!(c.r, 100);
        assert_eq!(c.g, 200);
        assert_eq!(c.b, 50);
        assert_eq!(c.a, 255);
    }
}
