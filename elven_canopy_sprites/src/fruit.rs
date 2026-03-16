// Fruit sprite generation (16x16).
//
// Generates deterministic 16x16 fruit sprites from `FruitAppearance` data
// produced during worldgen. Six shape variants (Round, Oblong, Clustered,
// Pod, Nut, Gourd) with per-species color, size scaling, and optional glow.
//
// This is the single source of truth for fruit rendering — used by both
// the GDExtension bridge (in-game billboard textures) and the elfcyclopedia
// server (inline PNG data URIs).
//
// See also: `drawing.rs` for the PixelBuffer, `fruit.rs` in `elven_canopy_sim`
// for `FruitAppearance`/`FruitShape`/`FruitColor`.

use elven_canopy_sim::fruit::{FruitAppearance, FruitShape};

use crate::color::Color;
use crate::drawing::PixelBuffer;

/// Derived color palette for fruit drawing, computed from the base color.
struct FruitPalette {
    outline: Color,
    dark: Color,
    base: Color,
    light: Color,
}

/// Create a 16x16 fruit sprite from appearance data.
pub fn create_fruit(appearance: &FruitAppearance) -> PixelBuffer {
    let mut img = PixelBuffer::new(16, 16);

    let base_color = Color::from_u8(
        appearance.exterior_color.r,
        appearance.exterior_color.g,
        appearance.exterior_color.b,
        255,
    );
    let pal = FruitPalette {
        outline: base_color.darken(0.35),
        dark: base_color.darken(0.15),
        base: base_color,
        light: base_color.lighten(0.15),
    };
    let cx = 8;
    let cy = 8;
    let scale = (appearance.size_percent as f32 / 100.0).clamp(0.6, 1.5);

    match appearance.shape {
        FruitShape::Round => draw_round(&mut img, cx, cy, scale, &pal),
        FruitShape::Oblong => draw_oblong(&mut img, cx, cy, scale, &pal),
        FruitShape::Clustered => draw_clustered(&mut img, cx, cy, scale, &pal),
        FruitShape::Pod => draw_pod(&mut img, cx, cy, scale, &pal),
        FruitShape::Nut => draw_nut(&mut img, cx, cy, scale, &pal),
        FruitShape::Gourd => draw_gourd(&mut img, cx, cy, scale, &pal),
    }

    // Stem for non-clustered shapes.
    if appearance.shape != FruitShape::Clustered {
        let stem = Color::rgb(0.30, 0.50, 0.15);
        img.set_px(cx, 1, stem);
        img.set_px(cx, 2, stem);
    }

    // Glow effect.
    if appearance.glows {
        apply_glow(&mut img, base_color);
    }

    img
}

fn draw_round(img: &mut PixelBuffer, cx: i32, cy: i32, scale: f32, p: &FruitPalette) {
    let r = (5.0 * scale) as i32;
    img.draw_circle(cx, cy, r, p.outline);
    img.draw_circle(cx, cy, r - 1, p.base);
    img.draw_circle(cx + 1, cy + 1, r - 2, p.dark);
    img.draw_circle(cx, cy, r - 2, p.base);
    img.set_px(cx - 2, cy - 2, p.light);
    img.set_px(cx - 1, cy - 2, p.light);
    img.set_px(cx - 2, cy - 1, p.light);
}

fn draw_oblong(img: &mut PixelBuffer, cx: i32, cy: i32, scale: f32, p: &FruitPalette) {
    let rx = (3.0 * scale) as i32;
    let ry = (6.0 * scale) as i32;
    img.draw_ellipse(cx, cy, rx, ry, p.outline);
    img.draw_ellipse(cx, cy, rx - 1, ry - 1, p.base);
    img.draw_ellipse(cx + 1, cy + 1, rx - 2, ry - 2, p.dark);
    img.draw_ellipse(cx, cy, rx - 2, ry - 2, p.base);
    img.set_px(cx - 1, cy - 3, p.light);
    img.set_px(cx - 1, cy - 2, p.light);
}

fn draw_clustered(img: &mut PixelBuffer, cx: i32, cy: i32, scale: f32, p: &FruitPalette) {
    let r = (2.0 * scale) as i32;
    let offsets: [(i32, i32); 6] = [(-3, 3), (0, 3), (3, 3), (-2, 0), (2, 0), (0, -3)];
    for (ox, oy) in offsets {
        let bx = cx + (ox as f32 * scale) as i32;
        let by = cy + (oy as f32 * scale) as i32;
        img.draw_circle(bx, by, r, p.outline);
        img.draw_circle(bx, by, r - 1, p.base);
        img.set_px(bx + 1, by + 1, p.dark);
        img.set_px(bx - 1, by - 1, p.light);
    }
    // Small stem at top.
    let stem = Color::rgb(0.30, 0.50, 0.15);
    img.set_px(cx, cy - (5.0 * scale) as i32, stem);
    img.set_px(cx, cy - (4.0 * scale) as i32, stem);
}

fn draw_pod(img: &mut PixelBuffer, cx: i32, cy: i32, scale: f32, p: &FruitPalette) {
    let rx = (2.0 * scale) as i32;
    let ry = (6.0 * scale) as i32;
    img.draw_ellipse(cx, cy, rx, ry, p.outline);
    img.draw_ellipse(cx, cy, rx - 1, ry - 1, p.base);
    // Seam line.
    for y in (cy - ry + 2)..=(cy + ry - 2) {
        img.set_px(cx, y, p.dark);
    }
    // Highlight.
    for y in (cy - ry + 2)..=(cy + ry - 3) {
        img.set_px(cx - 1, y, p.light);
    }
}

fn draw_nut(img: &mut PixelBuffer, cx: i32, cy: i32, scale: f32, p: &FruitPalette) {
    let r = (4.0 * scale) as i32;
    let cap_color = p.base.darken(0.25);
    let cap_dark = cap_color.darken(0.15);

    // Cap.
    let cap_y = cy - (2.0 * scale) as i32;
    img.draw_ellipse(cx, cap_y, r, (2.5 * scale) as i32, p.outline);
    img.draw_ellipse(cx, cap_y, r - 1, (2.0 * scale) as i32, cap_color);
    // Cross-hatch.
    let mut x = cx - r + 2;
    while x <= cx + r - 2 {
        img.set_px(x, cap_y, cap_dark);
        x += 2;
    }

    // Body.
    let body_y = cy + scale as i32;
    img.draw_ellipse(cx, body_y, r - 1, (3.5 * scale) as i32, p.outline);
    img.draw_ellipse(cx, body_y, r - 2, (3.0 * scale) as i32, p.base);
    // Highlight.
    img.set_px(cx - 1, cy, p.light);
    img.set_px(cx - 2, cy + 1, p.light);
    // Point at bottom.
    img.set_px(cx, cy + (4.0 * scale) as i32, p.dark);
}

fn draw_gourd(img: &mut PixelBuffer, cx: i32, cy: i32, scale: f32, p: &FruitPalette) {
    // Bottom bulge.
    let br = (5.0 * scale) as i32;
    let by = cy + (2.0 * scale) as i32;
    img.draw_ellipse(cx, by, br, (4.0 * scale) as i32, p.outline);
    img.draw_ellipse(cx, by, br - 1, (3.5 * scale) as i32, p.base);
    // Top bulge.
    let tr = (3.0 * scale) as i32;
    let ty = cy - (3.0 * scale) as i32;
    img.draw_ellipse(cx, ty, tr, (2.5 * scale) as i32, p.outline);
    img.draw_ellipse(cx, ty, tr - 1, (2.0 * scale) as i32, p.base);
    // Vertical ridges.
    for x in [cx - 2, cx, cx + 2] {
        for y in (by - (3.0 * scale) as i32)..(by + (3.0 * scale) as i32) {
            img.set_px(x, y, p.dark);
        }
    }
    // Highlight.
    img.set_px(cx - 2, cy - 1, p.light);
    img.set_px(cx - 2, cy, p.light);
}

/// Add a glow effect: place semi-transparent bright pixels around opaque pixels.
fn apply_glow(img: &mut PixelBuffer, base_color: Color) {
    let glow_color = Color::from_f32(
        (base_color.r as f32 / 255.0 + 0.3).min(1.0),
        (base_color.g as f32 / 255.0 + 0.3).min(1.0),
        (base_color.b as f32 / 255.0 + 0.3).min(1.0),
        0.4,
    );

    // Collect opaque positions first.
    let w = img.width() as i32;
    let h = img.height() as i32;
    let mut opaque = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if img.get_alpha(x, y) > 127 {
                opaque.push((x, y));
            }
        }
    }

    // Paint glow in empty neighbors.
    for (px, py) in opaque {
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nx = px + dx;
            let ny = py + dy;
            if nx >= 0 && nx < w && ny >= 0 && ny < h && img.get_alpha(nx, ny) < 25 {
                img.set_px(nx, ny, glow_color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elven_canopy_sim::fruit::FruitColor;

    fn test_appearance(shape: FruitShape) -> FruitAppearance {
        FruitAppearance {
            exterior_color: FruitColor {
                r: 200,
                g: 50,
                b: 50,
            },
            shape,
            size_percent: 100,
            glows: false,
        }
    }

    #[test]
    fn all_shapes_produce_nonempty_sprites() {
        for shape in FruitShape::ALL {
            let buf = create_fruit(&test_appearance(shape));
            assert_eq!(buf.width(), 16);
            assert_eq!(buf.height(), 16);
            let has_opaque = buf.data().chunks(4).any(|px| px[3] > 0);
            assert!(has_opaque, "{shape:?} fruit is completely transparent");
        }
    }

    #[test]
    fn glow_adds_semitransparent_pixels() {
        let mut app = test_appearance(FruitShape::Round);
        app.glows = false;
        let no_glow = create_fruit(&app);

        app.glows = true;
        let with_glow = create_fruit(&app);

        // Glowing version should have more non-transparent pixels.
        let count = |buf: &PixelBuffer| buf.data().chunks(4).filter(|px| px[3] > 0).count();
        assert!(
            count(&with_glow) > count(&no_glow),
            "Glow should add more pixels"
        );
    }

    #[test]
    fn size_percent_affects_sprite() {
        let small = {
            let mut app = test_appearance(FruitShape::Round);
            app.size_percent = 60;
            create_fruit(&app)
        };
        let large = {
            let mut app = test_appearance(FruitShape::Round);
            app.size_percent = 150;
            create_fruit(&app)
        };
        assert_ne!(small.data(), large.data());
    }

    #[test]
    fn clustered_draws_own_stem_not_center_stem() {
        let buf = create_fruit(&test_appearance(FruitShape::Clustered));
        assert!(buf.data().chunks(4).any(|px| px[3] > 0));
        // Clustered draws its own stem at an offset, not the standard (8,1)/(8,2).
        // The standard stem color is rgb(0.30, 0.50, 0.15) = (76, 127, 38).
        // Verify the standard stem positions don't have stem-colored pixels.
        let px_at = |x: i32, y: i32| buf.get_px(x, y);
        let stem_color = crate::Color::rgb(0.30, 0.50, 0.15);
        assert_ne!(
            px_at(8, 1),
            stem_color,
            "center stem should not be drawn for Clustered"
        );
        assert_ne!(
            px_at(8, 2),
            stem_color,
            "center stem should not be drawn for Clustered"
        );
    }

    #[test]
    fn deterministic_output() {
        let app = test_appearance(FruitShape::Gourd);
        let b1 = create_fruit(&app);
        let b2 = create_fruit(&app);
        assert_eq!(b1.data(), b2.data());
    }
}
