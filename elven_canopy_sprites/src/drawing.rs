// RGBA pixel buffer with drawing primitives for procedural sprites.
//
// `PixelBuffer` holds a flat `Vec<u8>` in RGBA8 format (4 bytes per pixel,
// row-major). All drawing operations are bounds-checked — out-of-bounds
// pixels are silently ignored, matching the original GDScript behavior.
//
// Primitives provided: `set_px`, `draw_circle`, `draw_ellipse`, `draw_rect`,
// `draw_hline`, `draw_vline`. The algorithms match the GDScript originals
// pixel-for-pixel to ensure visual consistency.
//
// See also: `color.rs` for the Color type, `lib.rs` for the public API.

use crate::color::Color;

/// An RGBA8 pixel buffer with drawing primitives.
pub struct PixelBuffer {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl PixelBuffer {
    /// Create a new pixel buffer filled with transparent black.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
        }
    }

    /// Create a pixel buffer from pre-existing RGBA8 data.
    pub fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Self {
        debug_assert_eq!(data.len(), (width * height * 4) as usize);
        Self {
            width,
            height,
            data,
        }
    }

    /// Buffer width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Buffer height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Consume the buffer and return the raw RGBA8 byte data.
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    /// Borrow the raw RGBA8 byte data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Set a single pixel. Out-of-bounds coordinates are silently ignored.
    pub fn set_px(&mut self, x: i32, y: i32, color: Color) {
        if x >= 0 && (x as u32) < self.width && y >= 0 && (y as u32) < self.height {
            let idx = ((y as u32 * self.width + x as u32) * 4) as usize;
            self.data[idx] = color.r;
            self.data[idx + 1] = color.g;
            self.data[idx + 2] = color.b;
            self.data[idx + 3] = color.a;
        }
    }

    /// Get the alpha value at (x, y). Returns 0 for out-of-bounds.
    pub fn get_alpha(&self, x: i32, y: i32) -> u8 {
        if x >= 0 && (x as u32) < self.width && y >= 0 && (y as u32) < self.height {
            self.data[((y as u32 * self.width + x as u32) * 4 + 3) as usize]
        } else {
            0
        }
    }

    /// Get the color at (x, y). Returns transparent black for out-of-bounds.
    pub fn get_px(&self, x: i32, y: i32) -> Color {
        if x >= 0 && (x as u32) < self.width && y >= 0 && (y as u32) < self.height {
            let idx = ((y as u32 * self.width + x as u32) * 4) as usize;
            Color::from_u8(
                self.data[idx],
                self.data[idx + 1],
                self.data[idx + 2],
                self.data[idx + 3],
            )
        } else {
            Color::TRANSPARENT
        }
    }

    /// Draw a filled circle. Matches GDScript `_draw_circle`.
    pub fn draw_circle(&mut self, cx: i32, cy: i32, r: i32, color: Color) {
        for py in (cy - r)..=(cy + r) {
            for px in (cx - r)..=(cx + r) {
                if (px - cx) * (px - cx) + (py - cy) * (py - cy) <= r * r {
                    self.set_px(px, py, color);
                }
            }
        }
    }

    /// Draw a filled ellipse. Matches GDScript `_draw_ellipse`.
    pub fn draw_ellipse(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, color: Color) {
        if rx == 0 || ry == 0 {
            return;
        }
        for py in (cy - ry)..=(cy + ry) {
            for px in (cx - rx)..=(cx + rx) {
                let dx = (px - cx) as f32 / rx as f32;
                let dy = (py - cy) as f32 / ry as f32;
                if dx * dx + dy * dy <= 1.0 {
                    self.set_px(px, py, color);
                }
            }
        }
    }

    /// Draw a filled rectangle. Matches GDScript `_draw_rect`.
    pub fn draw_rect(&mut self, x0: i32, y0: i32, w: i32, h: i32, color: Color) {
        for py in y0..(y0 + h) {
            for px in x0..(x0 + w) {
                self.set_px(px, py, color);
            }
        }
    }

    /// Rotate the buffer 90° clockwise: (x, y) → (height-1-y, x).
    /// Returns a new buffer with width = old height, height = old width.
    pub fn rotate_90_cw(&self) -> Self {
        let w = self.width as usize;
        let h = self.height as usize;
        let mut dst = vec![0u8; self.data.len()];
        for y in 0..h {
            for x in 0..w {
                let src_idx = (y * w + x) * 4;
                let new_x = h - 1 - y;
                let new_y = x;
                let dst_idx = (new_y * h + new_x) * 4;
                dst[dst_idx..dst_idx + 4].copy_from_slice(&self.data[src_idx..src_idx + 4]);
            }
        }
        Self {
            width: self.height,
            height: self.width,
            data: dst,
        }
    }

    /// Draw a horizontal line from x0 to x1 (inclusive). Matches GDScript `_draw_hline`.
    pub fn draw_hline(&mut self, x0: i32, x1: i32, y: i32, color: Color) {
        for px in x0..=x1 {
            self.set_px(px, y, color);
        }
    }

    /// Draw a vertical line from y0 to y1 (inclusive). Matches GDScript `_draw_vline`.
    pub fn draw_vline(&mut self, x: i32, y0: i32, y1: i32, color: Color) {
        for py in y0..=y1 {
            self.set_px(x, py, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_transparent() {
        let buf = PixelBuffer::new(4, 4);
        assert_eq!(buf.data().len(), 4 * 4 * 4);
        assert!(buf.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn set_px_writes_color() {
        let mut buf = PixelBuffer::new(4, 4);
        let c = Color::rgb(1.0, 0.5, 0.0);
        buf.set_px(2, 3, c);
        let got = buf.get_px(2, 3);
        assert_eq!(got, c);
    }

    #[test]
    fn set_px_out_of_bounds_is_noop() {
        let mut buf = PixelBuffer::new(4, 4);
        let c = Color::rgb(1.0, 0.0, 0.0);
        buf.set_px(-1, 0, c);
        buf.set_px(0, -1, c);
        buf.set_px(4, 0, c);
        buf.set_px(0, 4, c);
        // Buffer should still be all zeros.
        assert!(buf.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn draw_circle_radius_1() {
        let mut buf = PixelBuffer::new(8, 8);
        let c = Color::rgb(1.0, 0.0, 0.0);
        buf.draw_circle(4, 4, 1, c);
        // Center and 4 cardinal neighbors should be filled.
        assert_eq!(buf.get_px(4, 4), c);
        assert_eq!(buf.get_px(3, 4), c);
        assert_eq!(buf.get_px(5, 4), c);
        assert_eq!(buf.get_px(4, 3), c);
        assert_eq!(buf.get_px(4, 5), c);
        // Diagonals should NOT be filled for r=1 (distance = sqrt(2) > 1).
        assert_eq!(buf.get_px(3, 3), Color::TRANSPARENT);
    }

    #[test]
    fn draw_ellipse_fills_center() {
        let mut buf = PixelBuffer::new(16, 16);
        let c = Color::rgb(0.0, 1.0, 0.0);
        buf.draw_ellipse(8, 8, 3, 2, c);
        assert_eq!(buf.get_px(8, 8), c);
        // Horizontal extent.
        assert_eq!(buf.get_px(5, 8), c);
        assert_eq!(buf.get_px(11, 8), c);
        // Just outside horizontal extent.
        assert_eq!(buf.get_px(4, 8), Color::TRANSPARENT);
    }

    #[test]
    fn draw_rect_fills_area() {
        let mut buf = PixelBuffer::new(8, 8);
        let c = Color::rgb(0.0, 0.0, 1.0);
        buf.draw_rect(1, 2, 3, 2, c);
        assert_eq!(buf.get_px(1, 2), c);
        assert_eq!(buf.get_px(3, 3), c);
        // Just outside.
        assert_eq!(buf.get_px(4, 2), Color::TRANSPARENT);
        assert_eq!(buf.get_px(1, 4), Color::TRANSPARENT);
    }

    #[test]
    fn draw_hline_inclusive() {
        let mut buf = PixelBuffer::new(8, 8);
        let c = Color::rgb(1.0, 1.0, 0.0);
        buf.draw_hline(2, 5, 3, c);
        for x in 2..=5 {
            assert_eq!(buf.get_px(x, 3), c);
        }
        assert_eq!(buf.get_px(1, 3), Color::TRANSPARENT);
        assert_eq!(buf.get_px(6, 3), Color::TRANSPARENT);
    }

    #[test]
    fn draw_vline_inclusive() {
        let mut buf = PixelBuffer::new(8, 8);
        let c = Color::rgb(0.0, 1.0, 1.0);
        buf.draw_vline(3, 1, 4, c);
        for y in 1..=4 {
            assert_eq!(buf.get_px(3, y), c);
        }
        assert_eq!(buf.get_px(3, 0), Color::TRANSPARENT);
        assert_eq!(buf.get_px(3, 5), Color::TRANSPARENT);
    }

    #[test]
    fn get_alpha_out_of_bounds_returns_zero() {
        let buf = PixelBuffer::new(4, 4);
        assert_eq!(buf.get_alpha(-1, 0), 0);
        assert_eq!(buf.get_alpha(4, 0), 0);
    }

    #[test]
    fn into_data_returns_correct_length() {
        let buf = PixelBuffer::new(16, 16);
        let data = buf.into_data();
        assert_eq!(data.len(), 16 * 16 * 4);
    }

    #[test]
    fn draw_ellipse_zero_radius_is_noop() {
        let mut buf = PixelBuffer::new(8, 8);
        buf.draw_ellipse(4, 4, 0, 3, Color::rgb(1.0, 0.0, 0.0));
        assert!(buf.data().iter().all(|&b| b == 0));
        buf.draw_ellipse(4, 4, 3, 0, Color::rgb(1.0, 0.0, 0.0));
        assert!(buf.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn rotate_90_cw_non_square() {
        // 3x2 buffer (W=3, H=2) rotated CW should become 2x3 (W=2, H=3).
        // Original layout (row-major, each letter = 1 pixel):
        //   A B C
        //   D E F
        // After 90° CW:
        //   D A
        //   E B
        //   F C
        let mut buf = PixelBuffer::new(3, 2);
        let red = Color::rgb(1.0, 0.0, 0.0);
        let blue = Color::rgb(0.0, 0.0, 1.0);
        buf.set_px(0, 0, red); // A = red
        buf.set_px(2, 1, blue); // F = blue

        let rot = buf.rotate_90_cw();
        assert_eq!(rot.width(), 2);
        assert_eq!(rot.height(), 3);
        // A was at (0,0), should now be at (h-1-0, 0) = (1, 0).
        assert_eq!(rot.get_px(1, 0).r, 255);
        // F was at (2,1), should now be at (h-1-1, 2) = (0, 2).
        assert_eq!(rot.get_px(0, 2).b, 255);
    }

    #[test]
    fn rotate_90_cw_1x1() {
        let mut buf = PixelBuffer::new(1, 1);
        buf.set_px(0, 0, Color::rgb(0.5, 0.5, 0.5));
        let rot = buf.rotate_90_cw();
        assert_eq!(rot.width(), 1);
        assert_eq!(rot.height(), 1);
        let px = rot.get_px(0, 0);
        assert_eq!(px.r, 127); // 0.5 * 255 ≈ 127
    }
}
