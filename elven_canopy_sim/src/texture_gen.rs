// Procedural face texture generation using 3D Perlin noise.
//
// Generates per-face textures that are seamless across adjacent voxel faces
// because the noise is sampled at 3D world coordinates. Each visible face
// gets a small tile (FACE_TEX_SIZE × FACE_TEX_SIZE) packed into a per-chunk
// texture atlas. Two faces sharing an edge — regardless of orientation —
// sample the same noise values along that edge, so there are no seams even
// at corners where three faces meet.
//
// The atlas is a simple grid of tiles. UVs for each face's vertices map into
// the tile's region within the atlas. The calling code (`mesh_gen.rs`) handles
// UV assignment; this module handles noise generation and pixel writing.
//
// See also: `mesh_gen.rs` for atlas UV computation and face emission,
// `sim_bridge.rs` for passing atlas data to Godot, `tree_renderer.gd` for
// creating per-chunk materials from the atlas textures.
//
// **Determinism note:** Perlin noise is computed from a fixed permutation
// table and pure math — fully deterministic. However, like `mesh_gen.rs`,
// this is a rendering concern and does not participate in the sim's lockstep
// determinism contract.

/// Side length of each face texture tile in texels.
pub const FACE_TEX_SIZE: u32 = 16;

/// Base frequency for Perlin noise sampling. Higher = more detail per face.
/// With FACE_TEX_SIZE=16, a base frequency of 8.0 gives ~8 noise periods
/// per voxel face at the coarsest octave — high-frequency, detailed noise.
const BASE_FREQ: f64 = 8.0;

/// Number of fractal noise octaves. More octaves = more fine detail.
/// 5 octaves with high persistence gives a prickly, detailed texture.
const OCTAVES: u32 = 5;

/// Persistence for fractal noise: amplitude multiplier per octave.
/// Higher values (closer to 1.0) preserve more high-frequency energy,
/// giving a sharper, more jagged appearance.
const PERSISTENCE: f64 = 0.65;

/// Which material family a face belongs to, controlling the color tinting
/// applied on top of the noise pattern.
#[derive(Clone, Copy)]
pub enum MaterialKind {
    /// Bark: trunk, branch, root, construction. Warm brownish tint.
    Bark,
    /// Ground: dirt/grass. Cool greenish tint.
    Ground,
}

/// A packed texture atlas containing one FACE_TEX_SIZE × FACE_TEX_SIZE tile
/// per face, arranged in a grid.
pub struct FaceAtlas {
    /// Raw RGBA pixel data (4 bytes per pixel), row-major.
    pub pixels: Vec<u8>,
    /// Atlas width in pixels.
    pub width: u32,
    /// Atlas height in pixels.
    pub height: u32,
    /// Number of tiles per row in the atlas grid.
    pub tiles_per_row: u32,
}

/// Information about a face needed for texture generation.
pub struct FaceTexInfo {
    /// Voxel world-space integer coordinates.
    pub wx: i32,
    pub wy: i32,
    pub wz: i32,
    /// Face direction index (0..5, matching mesh_gen::FACES).
    pub face_idx: usize,
}

/// For each face direction: [origin, u_dir, v_dir] defining how the texture's
/// 2D coordinate system maps to 3D world offsets relative to the voxel origin.
///
/// - origin: corner of the face at texture UV (0,0)
/// - u_dir: unit vector from UV(0,0) toward UV(1,0)
/// - v_dir: unit vector from UV(0,0) toward UV(0,1)
///
/// These are chosen so that adjacent coplanar faces share the same U/V
/// orientation, ensuring the 3D Perlin noise is sampled consistently.
pub const FACE_TEX_MAPPING: [[[f32; 3]; 3]; 6] = [
    // +X: face at x=1 plane, spans Z (U) and Y (V)
    [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]],
    // -X: face at x=0 plane, spans Z (U) and Y (V)
    [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]],
    // +Y: face at y=1 plane, spans X (U) and Z (V)
    [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
    // -Y: face at y=0 plane, spans X (U) and Z (V)
    [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
    // +Z: face at z=1 plane, spans X (U) and Y (V)
    [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    // -Z: face at z=0 plane, spans X (U) and Y (V)
    [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
];

/// Face-local UV coordinates for each vertex of each face direction.
/// Derived from FACE_VERTICES (in mesh_gen.rs) and FACE_TEX_MAPPING above.
/// Used by mesh_gen to compute atlas UVs during the UV fixup pass.
pub const FACE_LOCAL_UVS: [[[f32; 2]; 4]; 6] = [
    // +X: vertices [1,0,1], [1,1,1], [1,1,0], [1,0,0]
    [[1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]],
    // -X: vertices [0,0,0], [0,1,0], [0,1,1], [0,0,1]
    [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
    // +Y: vertices [0,1,0], [1,1,0], [1,1,1], [0,1,1]
    [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    // -Y: vertices [0,0,1], [1,0,1], [1,0,0], [0,0,0]
    [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    // +Z: vertices [0,0,1], [0,1,1], [1,1,1], [1,0,1]
    [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
    // -Z: vertices [1,0,0], [1,1,0], [0,1,0], [0,0,0]
    [[1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]],
];

/// Generate a texture atlas for a set of faces.
///
/// Each face gets a FACE_TEX_SIZE × FACE_TEX_SIZE tile. The atlas is a grid
/// of tiles large enough to hold all faces. Returns an empty atlas if the
/// face list is empty.
pub fn generate_atlas(faces: &[FaceTexInfo], material: MaterialKind) -> FaceAtlas {
    if faces.is_empty() {
        return FaceAtlas {
            pixels: vec![],
            width: 0,
            height: 0,
            tiles_per_row: 0,
        };
    }

    let count = faces.len() as u32;
    let tiles_per_row = (count as f64).sqrt().ceil() as u32;
    let tile_rows = count.div_ceil(tiles_per_row);
    let width = tiles_per_row * FACE_TEX_SIZE;
    let height = tile_rows * FACE_TEX_SIZE;

    let mut pixels = vec![0u8; (width * height * 4) as usize];

    for (i, face) in faces.iter().enumerate() {
        let tile_col = i as u32 % tiles_per_row;
        let tile_row = i as u32 / tiles_per_row;

        let [origin, u_dir, v_dir] = FACE_TEX_MAPPING[face.face_idx];

        for ty in 0..FACE_TEX_SIZE {
            for tx in 0..FACE_TEX_SIZE {
                // Normalized position within the tile (0.0 to 1.0).
                let u = tx as f64 / (FACE_TEX_SIZE - 1) as f64;
                let v = ty as f64 / (FACE_TEX_SIZE - 1) as f64;

                // 3D world position for this texel.
                let wx =
                    face.wx as f64 + origin[0] as f64 + u * u_dir[0] as f64 + v * v_dir[0] as f64;
                let wy =
                    face.wy as f64 + origin[1] as f64 + u * u_dir[1] as f64 + v * v_dir[1] as f64;
                let wz =
                    face.wz as f64 + origin[2] as f64 + u * u_dir[2] as f64 + v * v_dir[2] as f64;

                let noise = sample_material_noise(wx, wy, wz, material);
                let (r, g, b) = colorize(noise, material);

                let px = tile_col * FACE_TEX_SIZE + tx;
                let py = tile_row * FACE_TEX_SIZE + ty;
                let idx = ((py * width + px) * 4) as usize;
                pixels[idx] = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
                pixels[idx + 3] = 255;
            }
        }
    }

    FaceAtlas {
        pixels,
        width,
        height,
        tiles_per_row,
    }
}

/// Sample noise appropriate for the material type.
///
/// Bark uses anisotropic scaling (Y compressed → vertical grain) plus
/// domain warping (one noise layer displaces the coordinates of the main
/// noise, creating organic wobble around knots and grain irregularities).
///
/// Ground uses plain isotropic fractal noise.
fn sample_material_noise(wx: f64, wy: f64, wz: f64, material: MaterialKind) -> f64 {
    match material {
        MaterialKind::Bark => {
            // Anisotropic scaling: compress Y to ~35% of X/Z frequency.
            // This stretches the noise vertically → implicit grain lines.
            let bx = wx * BASE_FREQ;
            let by = wy * BASE_FREQ * 0.35;
            let bz = wz * BASE_FREQ;

            // Domain warping: sample a low-frequency noise to displace the
            // main sample coordinates. This makes grain lines wobble and
            // flow organically rather than running perfectly straight.
            // The warp offset is sampled at different coordinates (shifted
            // by large primes) so it's uncorrelated with the main noise.
            let warp_strength = 0.6;
            let warp_freq = 2.5;
            let warp_x =
                perlin_3d(wx * warp_freq + 31.7, wy * warp_freq, wz * warp_freq) * warp_strength;
            let warp_z =
                perlin_3d(wx * warp_freq, wy * warp_freq + 47.3, wz * warp_freq) * warp_strength;

            fractal_noise_3d(bx + warp_x, by, bz + warp_z, OCTAVES)
        }
        MaterialKind::Ground => {
            fractal_noise_3d(wx * BASE_FREQ, wy * BASE_FREQ, wz * BASE_FREQ, OCTAVES)
        }
    }
}

/// Map a noise value in [-1, 1] to an RGB color appropriate for the material.
/// Wide contrast range (0.35–1.0) so the texture produces visible dark
/// crevices and bright highlights when multiplied with vertex colors.
fn colorize(noise: f64, material: MaterialKind) -> (u8, u8, u8) {
    let val = (0.75 + noise * 0.35).clamp(0.35, 1.0);
    let (rf, gf, bf) = match material {
        MaterialKind::Bark => (val * 1.08, val * 0.92, val * 0.80),
        MaterialKind::Ground => (val * 0.85, val * 1.08, val * 0.82),
    };
    (
        (rf * 255.0).clamp(0.0, 255.0) as u8,
        (gf * 255.0).clamp(0.0, 255.0) as u8,
        (bf * 255.0).clamp(0.0, 255.0) as u8,
    )
}

// ============================================================================
// 3D Perlin noise (improved, deterministic)
// ============================================================================

/// Classic improved Perlin noise permutation table.
#[rustfmt::skip]
const PERM: [u8; 256] = [
    151,160,137, 91, 90, 15,131, 13,201, 95, 96, 53,194,233,  7,225,
    140, 36,103, 30, 69,142,  8, 99, 37,240, 21, 10, 23,190,  6,148,
    247,120,234, 75,  0, 26,197, 62, 94,252,219,203,117, 35, 11, 32,
     57,177, 33, 88,237,149, 56, 87,174, 20,125,136,171,168, 68,175,
     74,165, 71,134,139, 48, 27,166, 77,146,158,231, 83,111,229,122,
     60,211,133,230,220,105, 92, 41, 55, 46,245, 40,244,102,143, 54,
     65, 25, 63,161,  1,216, 80, 73,209, 76,132,187,208, 89, 18,169,
    200,196,135,130,116,188,159, 86,164,100,109,198,173,186,  3, 64,
     52,217,226,250,124,123,  5,202, 38,147,118,126,255, 82, 85,212,
    207,206, 59,227, 47, 16, 58, 17,182,189, 28, 42,223,183,170,213,
    119,248,152,  2, 44,154,163, 70,221,153,101,155,167, 43,172,  9,
    129, 22, 39,253, 19, 98,108,110, 79,113,224,232,178,185,112,104,
    218,246, 97,228,251, 34,242,193,238,210,144, 12,191,179,162,241,
     81, 51,145,235,249, 14,239,107, 49,192,214, 31,181,199,106,157,
    184, 84,204,176,115,121, 50, 45,127,  4,150,254,138,236,205, 93,
    222,114, 67, 29, 24, 72,243,141,128,195, 78, 66,215, 61,156,180,
];

/// Permutation table lookup with wrapping.
fn perm(i: i32) -> i32 {
    PERM[(i & 255) as usize] as i32
}

/// Fade curve: 6t^5 - 15t^4 + 10t^3 (improved Perlin).
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Linear interpolation.
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

/// Gradient function: select one of 12 gradient directions based on hash.
fn grad3(hash: i32, x: f64, y: f64, z: f64) -> f64 {
    let h = hash & 15;
    let u = if h < 8 { x } else { y };
    let v = if h < 4 {
        y
    } else if h == 12 || h == 14 {
        x
    } else {
        z
    };
    (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}

/// 3D Perlin noise. Returns a value in approximately [-1, 1].
pub fn perlin_3d(x: f64, y: f64, z: f64) -> f64 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let zi = z.floor() as i32;

    let xf = x - x.floor();
    let yf = y - y.floor();
    let zf = z - z.floor();

    let u = fade(xf);
    let v = fade(yf);
    let w = fade(zf);

    // Hash the 8 corners of the unit cube.
    let aa = perm(perm(xi) + yi);
    let ab = perm(perm(xi) + yi + 1);
    let ba = perm(perm(xi + 1) + yi);
    let bb = perm(perm(xi + 1) + yi + 1);

    let aaa = perm(aa + zi);
    let aab = perm(aa + zi + 1);
    let aba = perm(ab + zi);
    let abb = perm(ab + zi + 1);
    let baa = perm(ba + zi);
    let bab = perm(ba + zi + 1);
    let bba = perm(bb + zi);
    let bbb = perm(bb + zi + 1);

    // Trilinear interpolation of gradient dot products.
    lerp(
        w,
        lerp(
            v,
            lerp(u, grad3(aaa, xf, yf, zf), grad3(baa, xf - 1.0, yf, zf)),
            lerp(
                u,
                grad3(aba, xf, yf - 1.0, zf),
                grad3(bba, xf - 1.0, yf - 1.0, zf),
            ),
        ),
        lerp(
            v,
            lerp(
                u,
                grad3(aab, xf, yf, zf - 1.0),
                grad3(bab, xf - 1.0, yf, zf - 1.0),
            ),
            lerp(
                u,
                grad3(abb, xf, yf - 1.0, zf - 1.0),
                grad3(bbb, xf - 1.0, yf - 1.0, zf - 1.0),
            ),
        ),
    )
}

/// Fractal Brownian motion: sum multiple octaves of Perlin noise for
/// multi-scale detail.
pub fn fractal_noise_3d(x: f64, y: f64, z: f64, octaves: u32) -> f64 {
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut max_value = 0.0;

    for _ in 0..octaves {
        total += perlin_3d(x * frequency, y * frequency, z * frequency) * amplitude;
        max_value += amplitude;
        amplitude *= PERSISTENCE;
        frequency *= 2.0;
    }

    total / max_value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perlin_noise_is_deterministic() {
        let a = perlin_3d(1.5, 2.7, 3.3);
        let b = perlin_3d(1.5, 2.7, 3.3);
        assert_eq!(a, b);
    }

    #[test]
    fn perlin_noise_range() {
        // Sample many points and verify values are in a reasonable range.
        for i in 0..1000 {
            let x = i as f64 * 0.13;
            let y = i as f64 * 0.17;
            let z = i as f64 * 0.23;
            let val = perlin_3d(x, y, z);
            assert!(
                val >= -1.5 && val <= 1.5,
                "Perlin noise out of expected range: {val} at ({x},{y},{z})"
            );
        }
    }

    #[test]
    fn perlin_noise_varies() {
        let a = perlin_3d(0.0, 0.0, 0.0);
        let b = perlin_3d(0.5, 0.5, 0.5);
        assert!(
            (a - b).abs() > 1e-10,
            "Perlin noise should vary: {a} vs {b}"
        );
    }

    #[test]
    fn fractal_noise_is_deterministic() {
        let a = fractal_noise_3d(1.5, 2.7, 3.3, 4);
        let b = fractal_noise_3d(1.5, 2.7, 3.3, 4);
        assert_eq!(a, b);
    }

    #[test]
    fn generate_atlas_empty() {
        let atlas = generate_atlas(&[], MaterialKind::Bark);
        assert!(atlas.pixels.is_empty());
        assert_eq!(atlas.width, 0);
        assert_eq!(atlas.height, 0);
    }

    #[test]
    fn generate_atlas_single_face() {
        let faces = vec![FaceTexInfo {
            wx: 5,
            wy: 3,
            wz: 7,
            face_idx: 2, // +Y
        }];
        let atlas = generate_atlas(&faces, MaterialKind::Bark);
        assert_eq!(atlas.width, FACE_TEX_SIZE);
        assert_eq!(atlas.height, FACE_TEX_SIZE);
        assert_eq!(
            atlas.pixels.len(),
            (FACE_TEX_SIZE * FACE_TEX_SIZE * 4) as usize
        );
        assert_eq!(atlas.tiles_per_row, 1);

        // All alpha values should be 255.
        for i in 0..(FACE_TEX_SIZE * FACE_TEX_SIZE) as usize {
            assert_eq!(atlas.pixels[i * 4 + 3], 255, "Alpha should be 255");
        }
    }

    #[test]
    fn generate_atlas_layout() {
        // 5 faces → ceil(sqrt(5)) = 3 tiles per row, 2 rows.
        let faces: Vec<FaceTexInfo> = (0..5)
            .map(|i| FaceTexInfo {
                wx: i,
                wy: 0,
                wz: 0,
                face_idx: 0,
            })
            .collect();
        let atlas = generate_atlas(&faces, MaterialKind::Ground);
        assert_eq!(atlas.tiles_per_row, 3);
        assert_eq!(atlas.width, 3 * FACE_TEX_SIZE);
        assert_eq!(atlas.height, 2 * FACE_TEX_SIZE);
    }

    #[test]
    fn adjacent_coplanar_faces_match_at_edge() {
        // Two adjacent +Y faces: voxels at (0,5,0) and (1,5,0).
        // Their shared edge is at world x=1, y=6, z=0..1.
        // The right edge of face 0 (u=1) and the left edge of face 1 (u=0)
        // should sample the same 3D Perlin noise positions.
        let faces = vec![
            FaceTexInfo {
                wx: 0,
                wy: 5,
                wz: 0,
                face_idx: 2,
            }, // +Y
            FaceTexInfo {
                wx: 1,
                wy: 5,
                wz: 0,
                face_idx: 2,
            }, // +Y
        ];
        let atlas = generate_atlas(&faces, MaterialKind::Bark);

        // Face 0 is tile (0,0), face 1 is tile (1,0).
        // Right edge of face 0: tx=15, ty=0..15.
        // Left edge of face 1: tx=0, ty=0..15.
        for ty in 0..FACE_TEX_SIZE {
            let px0 = 0 * FACE_TEX_SIZE + (FACE_TEX_SIZE - 1); // face 0 right edge
            let px1 = 1 * FACE_TEX_SIZE; // face 1 left edge
            let idx0 = ((ty * atlas.width + px0) * 4) as usize;
            let idx1 = ((ty * atlas.width + px1) * 4) as usize;
            assert_eq!(
                &atlas.pixels[idx0..idx0 + 4],
                &atlas.pixels[idx1..idx1 + 4],
                "Edge pixels should match at ty={ty}"
            );
        }
    }

    #[test]
    fn perpendicular_faces_match_at_shared_edge() {
        // A +Y face at (0,5,0) and a +X face at (0,5,0) share an edge
        // at world position x=1, y=6, z=0..1.
        //
        // +Y face: origin=(0,6,0), u_dir=(1,0,0), v_dir=(0,0,1)
        //   Right edge (u=1): world (1, 6, v) for v in 0..1.
        //
        // +X face: origin=(1,5,0), u_dir=(0,0,1), v_dir=(0,1,0)
        //   Top edge (v=1): world (1, 5+1, u) = (1, 6, u) for u in 0..1.
        //
        // Both sample the same line (1, 6, t) for t in [0,1].
        let faces = vec![
            FaceTexInfo {
                wx: 0,
                wy: 5,
                wz: 0,
                face_idx: 2,
            }, // +Y
            FaceTexInfo {
                wx: 0,
                wy: 5,
                wz: 0,
                face_idx: 0,
            }, // +X
        ];
        let atlas = generate_atlas(&faces, MaterialKind::Bark);

        // +Y face (tile 0): right edge at tx=15, ty varies.
        // World pos at (tx=15, ty): (0 + 0 + 1*1.0, 6, 0 + 0 + ty/(15)*1.0)
        //   = (1, 6, ty/15)
        //
        // +X face (tile 1): top edge at ty=15, tx varies.
        // World pos at (tx, ty=15): (1, 5 + 0 + 1*1.0, 0 + tx/(15)*1.0)
        //   = (1, 6, tx/15)
        //
        // When tx_plusY = ty_plusX, they sample the same point.
        for t in 0..FACE_TEX_SIZE {
            // +Y face right edge: tile 0, px = 15, py = t
            let px0 = FACE_TEX_SIZE - 1;
            let py0 = t;
            let idx0 = ((py0 * atlas.width + px0) * 4) as usize;

            // +X face top edge: tile 1, px = FACE_TEX_SIZE + t, py = 15
            let px1 = FACE_TEX_SIZE + t;
            let py1 = FACE_TEX_SIZE - 1;
            let idx1 = ((py1 * atlas.width + px1) * 4) as usize;

            assert_eq!(
                &atlas.pixels[idx0..idx0 + 4],
                &atlas.pixels[idx1..idx1 + 4],
                "Perpendicular edge pixels should match at t={t}"
            );
        }
    }
}
