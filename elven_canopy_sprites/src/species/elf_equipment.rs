// Equipment overlay drawing for chibi elf sprites (48x48).
//
// Each equippable ItemKind has a drawing function that paints the equipment
// onto an existing PixelBuffer at the elf's body-part coordinates. Color is
// passed in from the item's resolved color (material-derived or dye override),
// and shading uses `darken()`/`lighten()` — no hard-coded palettes.
//
// Clothing and armor for the same slot look visually distinct: clothing is
// simple and soft, armor is bulkier with highlights. Footwear has three
// tiers: sandals (minimal straps), shoes (closed but short), boots (chunky
// armor). Draw order is
// managed by `apply_equipment_overlays` in `elf.rs` — this module just draws
// individual pieces.
//
// See also: `elf.rs` for base sprite and compositing, `color.rs` for Color.

use crate::color::Color;
use crate::drawing::PixelBuffer;
use elven_canopy_sim::inventory::ItemKind;

/// Center x of the 48x48 elf sprite.
const CX: i32 = 24;

/// Draw the appropriate equipment overlay for the given item kind.
pub fn draw_equipment(buf: &mut PixelBuffer, kind: ItemKind, color: Color) {
    match kind {
        ItemKind::Hat => draw_hat(buf, color),
        ItemKind::Helmet => draw_helmet(buf, color),
        ItemKind::Tunic => draw_tunic(buf, color),
        ItemKind::Breastplate => draw_breastplate(buf, color),
        ItemKind::Leggings => draw_leggings(buf, color),
        ItemKind::Greaves => draw_greaves(buf, color),
        ItemKind::Sandals => draw_sandals(buf, color),
        ItemKind::Shoes => draw_shoes(buf, color),
        ItemKind::Boots => draw_boots(buf, color),
        ItemKind::Gloves => draw_gloves(buf, color),
        ItemKind::Gauntlets => draw_gauntlets(buf, color),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Head slot
// ---------------------------------------------------------------------------

/// Soft cloth cap sitting on top of the head.
fn draw_hat(buf: &mut PixelBuffer, color: Color) {
    let dark = color.darken(0.15);
    // Cap dome: y=4-8, roughly cx±9
    buf.draw_ellipse(CX, 6, 9, 4, color);
    // Brim: darker band at bottom of cap
    buf.draw_hline(CX - 9, CX + 9, 9, dark);
    buf.draw_hline(CX - 8, CX + 8, 10, dark);
}

/// Metal helmet with cheek guards and a highlight stripe.
fn draw_helmet(buf: &mut PixelBuffer, color: Color) {
    let dark = color.darken(0.15);
    let highlight = color.lighten(0.20);
    // Dome: wider coverage, y=3-9
    buf.draw_ellipse(CX, 6, 10, 5, color);
    // Cheek guards extending down sides
    buf.draw_rect(CX - 10, 9, 3, 5, dark);
    buf.draw_rect(CX + 8, 9, 3, 5, dark);
    // Metallic highlight across crown
    buf.draw_hline(CX - 6, CX + 6, 5, highlight);
}

// ---------------------------------------------------------------------------
// Torso slot
// ---------------------------------------------------------------------------

/// Simple cloth tunic covering the torso with short sleeves.
fn draw_tunic(buf: &mut PixelBuffer, color: Color) {
    let dark = color.darken(0.12);
    let collar = color.lighten(0.15);
    let body_top = 25;
    let body_bot = 36;
    // Main body fill
    for y in body_top..=body_bot {
        let hw = if y < body_top + 3 { 8 } else { 7 };
        buf.draw_hline(CX - hw, CX + hw, y, color);
    }
    // Collar highlight at top
    buf.draw_hline(CX - 6, CX + 6, body_top, collar);
    // Short sleeves over arm area
    buf.draw_rect(CX - 10, body_top + 2, 3, 4, color);
    buf.draw_rect(CX + 8, body_top + 2, 3, 4, color);
    // Hem at bottom
    buf.draw_hline(CX - 7, CX + 7, body_bot, dark);
}

/// Metal breastplate with shoulder pauldrons and a chest highlight.
fn draw_breastplate(buf: &mut PixelBuffer, color: Color) {
    let dark = color.darken(0.15);
    let highlight = color.lighten(0.20);
    let body_top = 25;
    let body_bot = 36;
    // Main plate fill
    for y in body_top..=body_bot {
        let hw = if y < body_top + 3 { 9 } else { 7 };
        buf.draw_hline(CX - hw, CX + hw, y, color);
    }
    // Shoulder pauldrons
    buf.draw_ellipse(CX - 9, body_top + 1, 3, 2, dark);
    buf.draw_ellipse(CX + 9, body_top + 1, 3, 2, dark);
    // Metallic highlight across chest
    buf.draw_hline(CX - 5, CX + 5, body_top + 3, highlight);
    buf.draw_hline(CX - 4, CX + 4, body_top + 4, highlight);
    // Darker bottom edge
    buf.draw_hline(CX - 7, CX + 7, body_bot - 1, dark);
    buf.draw_hline(CX - 7, CX + 7, body_bot, dark);
}

// ---------------------------------------------------------------------------
// Legs slot
// ---------------------------------------------------------------------------

/// Cloth leggings covering the upper legs.
fn draw_leggings(buf: &mut PixelBuffer, color: Color) {
    let dark = color.darken(0.12);
    let leg_top = 37;
    let leg_bot = 42;
    // Left leg
    buf.draw_rect(CX - 5, leg_top, 4, leg_bot - leg_top + 1, color);
    // Right leg
    buf.draw_rect(CX + 2, leg_top, 4, leg_bot - leg_top + 1, color);
    // Darkened hem at bottom of each leg
    buf.draw_hline(CX - 5, CX - 2, leg_bot, dark);
    buf.draw_hline(CX + 2, CX + 5, leg_bot, dark);
}

/// Metal greaves: slightly wider legs with a metallic outer edge.
fn draw_greaves(buf: &mut PixelBuffer, color: Color) {
    let highlight = color.lighten(0.20);
    let leg_top = 37;
    let leg_bot = 42;
    // Left leg (slightly wider)
    buf.draw_rect(CX - 6, leg_top, 5, leg_bot - leg_top + 1, color);
    // Right leg (slightly wider)
    buf.draw_rect(CX + 2, leg_top, 5, leg_bot - leg_top + 1, color);
    // Metallic outer edge highlights
    buf.draw_vline(CX - 6, leg_top, leg_bot, highlight);
    buf.draw_vline(CX + 6, leg_top, leg_bot, highlight);
}

// ---------------------------------------------------------------------------
// Feet slot
// ---------------------------------------------------------------------------

/// Light sandals — just a sole and a strap across the top.
fn draw_sandals(buf: &mut PixelBuffer, color: Color) {
    let dark = color.darken(0.15);
    let foot_y = 46;
    // Thin sole under each foot.
    buf.draw_hline(CX - 5, CX - 1, foot_y, color);
    buf.draw_hline(CX + 1, CX + 5, foot_y, color);
    // Darkened bottom edge.
    buf.draw_hline(CX - 5, CX - 1, foot_y + 1, dark);
    buf.draw_hline(CX + 1, CX + 5, foot_y + 1, dark);
    // Single strap across each foot.
    buf.draw_hline(CX - 5, CX - 1, foot_y - 1, color);
    buf.draw_hline(CX + 1, CX + 5, foot_y - 1, color);
}

/// Closed shoes covering the foot, shorter than boots.
fn draw_shoes(buf: &mut PixelBuffer, color: Color) {
    let dark = color.darken(0.15);
    let shoe_top = 45;
    // Left shoe — 3 rows high (shorter than boots' 5).
    buf.draw_rect(CX - 5, shoe_top, 5, 3, color);
    // Right shoe.
    buf.draw_rect(CX + 1, shoe_top, 5, 3, color);
    // Darkened sole at bottom row.
    buf.draw_hline(CX - 5, CX - 1, shoe_top + 2, dark);
    buf.draw_hline(CX + 1, CX + 5, shoe_top + 2, dark);
}

/// Chunky boots in the given color (armor piece).
fn draw_boots(buf: &mut PixelBuffer, color: Color) {
    let dark = color.darken(0.15);
    let boot_top = 43;
    // Left boot
    buf.draw_rect(CX - 6, boot_top, 6, 5, color);
    // Right boot
    buf.draw_rect(CX + 1, boot_top, 6, 5, color);
    // Darkened sole at bottom row
    buf.draw_hline(CX - 6, CX - 1, boot_top + 4, dark);
    buf.draw_hline(CX + 1, CX + 6, boot_top + 4, dark);
}

// ---------------------------------------------------------------------------
// Hands slot
// ---------------------------------------------------------------------------

/// Cloth gloves covering the hands.
fn draw_gloves(buf: &mut PixelBuffer, color: Color) {
    let body_top = 25;
    // Left hand
    buf.draw_rect(CX - 10, body_top + 9, 3, 2, color);
    // Right hand
    buf.draw_rect(CX + 8, body_top + 9, 3, 2, color);
}

/// Metal gauntlets extending up the forearm with a wrist highlight.
fn draw_gauntlets(buf: &mut PixelBuffer, color: Color) {
    let highlight = color.lighten(0.20);
    let body_top = 25;
    // Left gauntlet (extends up forearm)
    buf.draw_rect(CX - 10, body_top + 7, 3, 4, color);
    // Right gauntlet
    buf.draw_rect(CX + 8, body_top + 7, 3, 4, color);
    // Metallic wrist highlight
    buf.draw_hline(CX - 10, CX - 8, body_top + 7, highlight);
    buf.draw_hline(CX + 8, CX + 10, body_top + 7, highlight);
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_COLOR: Color = Color::rgb(0.6, 0.3, 0.2);

    /// All 11 equippable item kinds, used to verify each draws pixels.
    const EQUIPPABLE_KINDS: [ItemKind; 11] = [
        ItemKind::Hat,
        ItemKind::Helmet,
        ItemKind::Tunic,
        ItemKind::Breastplate,
        ItemKind::Leggings,
        ItemKind::Greaves,
        ItemKind::Sandals,
        ItemKind::Shoes,
        ItemKind::Boots,
        ItemKind::Gloves,
        ItemKind::Gauntlets,
    ];

    /// Helper: count non-transparent pixels in a pixel buffer.
    fn count_opaque_pixels(buf: &PixelBuffer) -> usize {
        buf.data().chunks(4).filter(|px| px[3] > 0).count()
    }

    #[test]
    fn each_equipment_kind_draws_pixels() {
        for kind in EQUIPPABLE_KINDS {
            let mut buf = PixelBuffer::new(48, 48);
            let before = count_opaque_pixels(&buf);
            draw_equipment(&mut buf, kind, TEST_COLOR);
            let after = count_opaque_pixels(&buf);
            assert!(after > before, "{kind:?} overlay did not draw any pixels");
        }
    }

    #[test]
    fn non_equipment_item_is_noop() {
        let mut buf = PixelBuffer::new(48, 48);
        let before = buf.data().to_vec();
        draw_equipment(&mut buf, ItemKind::Bread, TEST_COLOR);
        assert_eq!(buf.data(), before.as_slice());
    }

    #[test]
    fn different_colors_produce_different_pixels() {
        let red = Color::rgb(0.9, 0.1, 0.1);
        let blue = Color::rgb(0.1, 0.1, 0.9);
        for kind in EQUIPPABLE_KINDS {
            let mut buf_r = PixelBuffer::new(48, 48);
            let mut buf_b = PixelBuffer::new(48, 48);
            draw_equipment(&mut buf_r, kind, red);
            draw_equipment(&mut buf_b, kind, blue);
            assert_ne!(
                buf_r.data(),
                buf_b.data(),
                "{kind:?} produced identical pixels for different colors"
            );
        }
    }
}
