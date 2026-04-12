// Prime-period tiling texture generation for bark and ground surfaces.
//
// Replaces the old per-face Perlin noise atlas system with a three-cache
// tiling approach. Each cache (A, B, C) generates monochrome R8 tiles that
// tile at different prime periods per world axis:
//
//   Cache A: periods (11, 3, 7)  — 231 tiles × 3 axis pairs = 693 layers
//   Cache B: periods (7, 5, 11)  — 385 tiles × 3 axis pairs = 1155 layers
//   Cache C: periods (5, 7, 5)   — 175 tiles × 3 axis pairs = 525 layers
//
// No two caches share the same period on any axis, so alignment artifacts
// from one cache are masked by the other two. The shader samples all three
// caches, blends additively, and multiplies by vertex color (the existing
// per-voxel-type color ramp).
//
// Two material types produce different tile content from the same period
// structure:
// - **Bark**: anisotropic noise (Y compressed to 37.5% of X/Z frequency)
//   plus domain warping for organic grain lines. Same character as the old
//   per-face atlas bark noise.
// - **Ground**: isotropic fractal noise, plain and uniform.
//
// Bark and ground each get their own 3 Texture2DArrays and shader material
// so the noise character is distinct even though the tiling periods match.
//
// Tiles are 16×16 single-channel (R8) textures stored as `Texture2DArray`
// layers on the GPU. The shader computes tile UVs and layer indices from
// the fragment's world position and face normal — no per-vertex UVs needed.
//
// Each cache has irrational-ish per-axis phase offsets so noise zeros don't
// coincide at the world origin across caches.
//
// Tile content is tileable fractal Perlin noise: the standard improved Perlin
// hash is modified to wrap lattice coordinates at the cache's periods, making
// the noise seamlessly periodic. Five octaves of FBM give multi-scale detail.
//
// See also: `mesh_gen.rs` for chunk mesh generation (no longer handles textures),
// `mesh_cache.rs` (gdext) for tiling cache ownership, `sim_bridge.rs` for
// passing texture data to Godot, `tree_renderer.gd` for shader setup and
// material creation.
//
// **Determinism note:** Like the old atlas system, tile generation is pure and
// deterministic but is a rendering concern — it does not participate in the
// sim's lockstep determinism contract.

/// Side length of each tile in texels.
pub const TILE_SIZE: u32 = 16;

/// Bytes per tile (TILE_SIZE², R8 format — one byte per texel).
pub const TILE_BYTES: usize = (TILE_SIZE * TILE_SIZE) as usize;

/// Base frequency for noise sampling. Controls how many noise features
/// appear per voxel face. With TILE_SIZE=16 and BASE_FREQ=8.0, the
/// coarsest octave has ~8 noise periods per voxel.
const BASE_FREQ: f64 = 8.0;

/// Base frequency as integer for tileable period scaling.
const BASE_FREQ_INT: i32 = 8;

/// Number of fractal noise octaves.
const OCTAVES: u32 = 5;

/// Persistence for fractal noise: amplitude multiplier per octave.
const PERSISTENCE: f64 = 0.65;

/// Bark Y-axis compression factor (3/8 = 0.375). Compresses noise in Y
/// to create vertical grain lines. Chosen as a rational 3/8 so that
/// `py * BASE_FREQ * Y_COMPRESS = py * 3` is always an integer (required
/// for tileable Perlin lattice wrapping). Close to the old value of 0.35.
const BARK_Y_COMPRESS: f64 = 0.375;

/// Bark Y-axis noise-space period multiplier: BASE_FREQ * Y_COMPRESS = 3.
const BARK_Y_PERIOD_MULT: i32 = 3;

/// Domain warp frequency for bark (integer for tileable compatibility).
const BARK_WARP_FREQ: i32 = 3;

/// Domain warp strength for bark.
const BARK_WARP_STRENGTH: f64 = 0.6;

/// Number of tiling caches per material.
pub const CACHE_COUNT: usize = 3;

/// Number of material types.
pub const MATERIAL_COUNT: usize = 2;

/// Number of face axis pair groups.
pub const AXIS_PAIR_COUNT: usize = 3;

/// Which material family a face belongs to, controlling the noise character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MaterialKind {
    /// Bark: trunk, branch, root, construction. Anisotropic + domain warped.
    Bark = 0,
    /// Ground: dirt/grass. Isotropic fractal noise.
    Ground = 1,
}

/// Face axis pair: which two world axes form the face's UV plane.
/// Opposite faces (±X, ±Y, ±Z) share the same axis pair and tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AxisPair {
    /// ±Y faces: U = X, V = Z.
    Xz = 0,
    /// ±X faces: U = Z, V = Y.
    Zy = 1,
    /// ±Z faces: U = X, V = Y.
    Xy = 2,
}

impl AxisPair {
    /// Convert a face direction index (0..5, matching mesh_gen::FACES) to
    /// its axis pair. Faces 0,1 (±X) → Zy; 2,3 (±Y) → Xz; 4,5 (±Z) → Xy.
    pub fn from_face_idx(face_idx: usize) -> Self {
        match face_idx {
            0 | 1 => AxisPair::Zy,
            2 | 3 => AxisPair::Xz,
            4 | 5 => AxisPair::Xy,
            _ => panic!("invalid face_idx: {face_idx}"),
        }
    }
}

/// Cache periods for each axis [x, y, z]. No two caches share the same
/// period on any axis. Y periods are smaller (less vertical variation
/// needed) to keep cache sizes down.
pub const CACHE_PERIODS: [[i32; 3]; CACHE_COUNT] = [
    [11, 3, 7], // Cache A: 231 tiles/axis pair, 693 total
    [7, 5, 11], // Cache B: 385 tiles/axis pair, 1155 total
    [5, 7, 5],  // Cache C: 175 tiles/axis pair, 525 total
];

/// Phase offsets per cache — irrational-ish values so noise zeros don't
/// align at the world origin across caches.
pub const CACHE_PHASES: [[f64; 3]; CACHE_COUNT] =
    [[3.72, 1.41, 2.89], [5.17, 0.83, 4.31], [1.63, 3.94, 6.27]];

/// Two-material tiling texture system. Generates monochrome R8 tiles for
/// bark and ground separately, each with 3 caches × 3 axis pairs.
///
/// Tile data is laid out for direct upload as `Texture2DArray` layers:
/// each cache's data is a flat `Vec<u8>` with layers ordered as
/// `[axis_pair_0 tiles..., axis_pair_1 tiles..., axis_pair_2 tiles...]`.
/// Within each axis pair group, tiles are indexed by
/// `mx * (py * pz) + my * pz + mz`.
pub struct TilingCache {
    /// `caches[material][cache_idx]` — 2 materials × 3 caches = 6 sub-caches.
    caches: [[SubCache; CACHE_COUNT]; MATERIAL_COUNT],
}

struct SubCache {
    periods: [i32; 3],
    /// Number of unique tiles per axis pair: px * py * pz.
    tiles_per_axis_pair: usize,
    /// Total layers: 3 * tiles_per_axis_pair.
    total_layers: usize,
    /// Flat R8 tile data. Length = total_layers * TILE_BYTES.
    data: Vec<u8>,
}

impl Default for TilingCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TilingCache {
    /// Create a new tiling cache and eagerly generate all tiles for both
    /// bark and ground materials.
    ///
    /// Total memory: ~1.2 MB (2 materials × 2373 layers × 256 bytes).
    /// Generation is single-threaded but fast (pure math, no allocation
    /// per tile).
    pub fn new() -> Self {
        let caches = std::array::from_fn(|mat_idx| {
            let material = match mat_idx {
                0 => MaterialKind::Bark,
                _ => MaterialKind::Ground,
            };
            std::array::from_fn(|cache_idx| {
                let periods = CACHE_PERIODS[cache_idx];
                let phases = CACHE_PHASES[cache_idx];
                let tpap = (periods[0] * periods[1] * periods[2]) as usize;
                let total_layers = AXIS_PAIR_COUNT * tpap;
                let mut data = vec![0u8; total_layers * TILE_BYTES];

                for ap in 0..AXIS_PAIR_COUNT {
                    let axis_pair = match ap {
                        0 => AxisPair::Xz,
                        1 => AxisPair::Zy,
                        _ => AxisPair::Xy,
                    };
                    for mx in 0..periods[0] {
                        for my in 0..periods[1] {
                            for mz in 0..periods[2] {
                                let layer = ap * tpap
                                    + (mx * periods[1] * periods[2] + my * periods[2] + mz)
                                        as usize;
                                let offset = layer * TILE_BYTES;
                                generate_tile(
                                    &mut data[offset..offset + TILE_BYTES],
                                    &TileParams {
                                        periods,
                                        phases,
                                        axis_pair,
                                        material,
                                        mx,
                                        my,
                                        mz,
                                    },
                                );
                            }
                        }
                    }
                }

                SubCache {
                    periods,
                    tiles_per_axis_pair: tpap,
                    total_layers,
                    data,
                }
            })
        });

        TilingCache { caches }
    }

    /// Number of Texture2DArray layers for the given cache.
    pub fn layer_count(&self, material: MaterialKind, cache_idx: usize) -> usize {
        self.caches[material as usize][cache_idx].total_layers
    }

    /// Periods [px, py, pz] for the given cache index (same for both materials).
    pub fn periods(&self, cache_idx: usize) -> [i32; 3] {
        self.caches[0][cache_idx].periods
    }

    /// Tiles per axis pair for the given cache index (same for both materials).
    pub fn tiles_per_axis_pair(&self, cache_idx: usize) -> usize {
        self.caches[0][cache_idx].tiles_per_axis_pair
    }

    /// Flat R8 data for all layers of a cache, ready for `Texture2DArray`.
    /// Each layer is TILE_SIZE × TILE_SIZE bytes, laid out sequentially.
    pub fn texture_data(&self, material: MaterialKind, cache_idx: usize) -> &[u8] {
        &self.caches[material as usize][cache_idx].data
    }
}

/// Parameters for tile generation, bundled to avoid too-many-arguments.
struct TileParams {
    periods: [i32; 3],
    phases: [f64; 3],
    axis_pair: AxisPair,
    material: MaterialKind,
    mx: i32,
    my: i32,
    mz: i32,
}

/// Generate one tile's R8 data into the provided buffer.
///
/// The tile represents one voxel face at modular coordinates (mx, my, mz)
/// for the given axis pair and material. Noise character depends on material:
/// bark uses anisotropic + domain-warped noise, ground uses isotropic.
fn generate_tile(buf: &mut [u8], p: &TileParams) {
    let [px, py, pz] = p.periods;
    let [phx, phy, phz] = p.phases;

    for ty in 0..TILE_SIZE {
        for tx in 0..TILE_SIZE {
            // Edge-to-edge sampling: texel 0 = 0.0, texel N-1 = 1.0.
            // This ensures seamless tiling when adjacent tiles meet at
            // voxel boundaries (both tiles sample the same noise at the edge).
            let u = tx as f64 / (TILE_SIZE - 1) as f64;
            let v = ty as f64 / (TILE_SIZE - 1) as f64;

            // Map (u, v) to 3D world coordinates based on the axis pair.
            // The fixed axis gets the modular coordinate directly; the two
            // varying axes span [m, m+1] across the tile.
            let (wx, wy, wz) = match p.axis_pair {
                AxisPair::Xz => (
                    p.mx as f64 + u + phx,
                    p.my as f64 + phy,
                    p.mz as f64 + v + phz,
                ),
                AxisPair::Zy => (
                    p.mx as f64 + phx,
                    p.my as f64 + v + phy,
                    p.mz as f64 + u + phz,
                ),
                AxisPair::Xy => (
                    p.mx as f64 + u + phx,
                    p.my as f64 + v + phy,
                    p.mz as f64 + phz,
                ),
            };

            let noise = sample_material_noise(wx, wy, wz, px, py, pz, p.material);

            // Map [-1, 1] noise to [0, 255] grayscale.
            let val = ((noise * 0.5 + 0.5) * 255.0).clamp(0.0, 255.0) as u8;
            buf[(ty * TILE_SIZE + tx) as usize] = val;
        }
    }
}

/// Sample noise appropriate for the material type.
///
/// Bark uses anisotropic scaling (Y compressed to 37.5% of X/Z frequency)
/// plus domain warping (a separate tileable noise layer displaces the X/Z
/// coordinates of the main noise, creating organic grain wobble). Both the
/// warp and main noise are tileable at the cache's periods.
///
/// Ground uses plain isotropic tileable fractal noise.
fn sample_material_noise(
    wx: f64,
    wy: f64,
    wz: f64,
    px: i32,
    py: i32,
    pz: i32,
    material: MaterialKind,
) -> f64 {
    match material {
        MaterialKind::Bark => {
            // Anisotropic scaling: compress Y to 37.5% of X/Z frequency.
            // Noise-space coords: X and Z scaled by BASE_FREQ, Y by BASE_FREQ * 3/8.
            let bx = wx * BASE_FREQ;
            let by = wy * BASE_FREQ * BARK_Y_COMPRESS;
            let bz = wz * BASE_FREQ;

            // Noise-space tiling periods.
            let npx = px * BASE_FREQ_INT;
            let npy = py * BARK_Y_PERIOD_MULT; // py * 3
            let npz = pz * BASE_FREQ_INT;

            // Domain warping: tileable warp noise displaces X and Z to create
            // organic grain irregularities. Warp noise uses BARK_WARP_FREQ
            // scaling and is sampled at shifted coordinates (offset by large
            // constants) so it's uncorrelated with the main noise.
            let wf = BARK_WARP_FREQ as f64;
            let wpx = px * BARK_WARP_FREQ;
            let wpy = py * BARK_WARP_FREQ;
            let wpz = pz * BARK_WARP_FREQ;

            let warp_x = tileable_perlin_3d(wx * wf + 31.7, wy * wf, wz * wf, wpx, wpy, wpz)
                * BARK_WARP_STRENGTH;
            let warp_z = tileable_perlin_3d(wx * wf, wy * wf + 47.3, wz * wf, wpx, wpy, wpz)
                * BARK_WARP_STRENGTH;

            tileable_fractal_noise_3d(bx + warp_x, by, bz + warp_z, npx, npy, npz, OCTAVES)
        }
        MaterialKind::Ground => {
            let bx = wx * BASE_FREQ;
            let by = wy * BASE_FREQ;
            let bz = wz * BASE_FREQ;
            tileable_fractal_noise_3d(
                bx,
                by,
                bz,
                px * BASE_FREQ_INT,
                py * BASE_FREQ_INT,
                pz * BASE_FREQ_INT,
                OCTAVES,
            )
        }
    }
}

// ============================================================================
// Tileable 3D Perlin noise
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

/// 3D Perlin noise with tileable wrapping. Lattice coordinates wrap at the
/// specified periods, making the noise seamlessly periodic. Returns a value
/// in approximately [-1, 1].
///
/// The periods must be positive integers. For non-tileable noise, use very
/// large periods (effectively non-repeating within the world).
fn tileable_perlin_3d(x: f64, y: f64, z: f64, px: i32, py: i32, pz: i32) -> f64 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let zi = z.floor() as i32;

    let xf = x - x.floor();
    let yf = y - y.floor();
    let zf = z - z.floor();

    let u = fade(xf);
    let v = fade(yf);
    let w = fade(zf);

    // Wrap lattice coordinates to tile periods.
    let x0 = xi.rem_euclid(px);
    let x1 = (xi + 1).rem_euclid(px);
    let y0 = yi.rem_euclid(py);
    let y1 = (yi + 1).rem_euclid(py);
    let z0 = zi.rem_euclid(pz);
    let z1 = (zi + 1).rem_euclid(pz);

    // Hash the 8 corners of the unit cube using wrapped coordinates.
    let aa = perm(perm(x0) + y0);
    let ab = perm(perm(x0) + y1);
    let ba = perm(perm(x1) + y0);
    let bb = perm(perm(x1) + y1);

    let aaa = perm(aa + z0);
    let aab = perm(aa + z1);
    let aba = perm(ab + z0);
    let abb = perm(ab + z1);
    let baa = perm(ba + z0);
    let bab = perm(ba + z1);
    let bba = perm(bb + z0);
    let bbb = perm(bb + z1);

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

/// Fractal Brownian motion with tileable Perlin noise.
///
/// Each octave doubles the frequency and scales the tiling period accordingly,
/// keeping the overall pattern periodic at the base period.
fn tileable_fractal_noise_3d(
    x: f64,
    y: f64,
    z: f64,
    px: i32,
    py: i32,
    pz: i32,
    octaves: u32,
) -> f64 {
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut max_value = 0.0;

    for i in 0..octaves {
        let freq_mult = 1i32 << i;
        total += tileable_perlin_3d(
            x * frequency,
            y * frequency,
            z * frequency,
            px * freq_mult,
            py * freq_mult,
            pz * freq_mult,
        ) * amplitude;
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
    fn tileable_perlin_is_deterministic() {
        let a = tileable_perlin_3d(1.5, 2.7, 3.3, 11, 3, 7);
        let b = tileable_perlin_3d(1.5, 2.7, 3.3, 11, 3, 7);
        assert_eq!(a, b);
    }

    #[test]
    fn tileable_perlin_actually_tiles() {
        // Noise should be identical when offset by a full period on each axis.
        let px = 11;
        let py = 3;
        let pz = 7;
        for i in 0..50 {
            let x = i as f64 * 0.37;
            let y = i as f64 * 0.23;
            let z = i as f64 * 0.41;
            let base = tileable_perlin_3d(x, y, z, px, py, pz);

            let tiled_x = tileable_perlin_3d(x + px as f64, y, z, px, py, pz);
            assert!(
                (base - tiled_x).abs() < 1e-10,
                "X tiling failed at ({x},{y},{z}): {base} vs {tiled_x}"
            );

            let tiled_y = tileable_perlin_3d(x, y + py as f64, z, px, py, pz);
            assert!(
                (base - tiled_y).abs() < 1e-10,
                "Y tiling failed at ({x},{y},{z}): {base} vs {tiled_y}"
            );

            let tiled_z = tileable_perlin_3d(x, y, z + pz as f64, px, py, pz);
            assert!(
                (base - tiled_z).abs() < 1e-10,
                "Z tiling failed at ({x},{y},{z}): {base} vs {tiled_z}"
            );
        }
    }

    #[test]
    fn tileable_perlin_range() {
        for i in 0..1000 {
            let x = i as f64 * 0.13;
            let y = i as f64 * 0.17;
            let z = i as f64 * 0.23;
            let val = tileable_perlin_3d(x, y, z, 11, 5, 7);
            assert!(
                (-1.5..=1.5).contains(&val),
                "Tileable Perlin out of range: {val} at ({x},{y},{z})"
            );
        }
    }

    #[test]
    fn tileable_fractal_noise_tiles() {
        let px = 7;
        let py = 5;
        let pz = 11;
        for i in 0..20 {
            let x = i as f64 * 0.6 + 0.1;
            let y = i as f64 * 0.4 + 0.2;
            let z = i as f64 * 0.5 + 0.3;
            let base = tileable_fractal_noise_3d(x, y, z, px, py, pz, OCTAVES);
            let tiled = tileable_fractal_noise_3d(
                x + px as f64,
                y + py as f64,
                z + pz as f64,
                px,
                py,
                pz,
                OCTAVES,
            );
            assert!(
                (base - tiled).abs() < 1e-10,
                "Fractal tiling failed at ({x},{y},{z}): {base} vs {tiled}"
            );
        }
    }

    #[test]
    fn tileable_perlin_varies() {
        let a = tileable_perlin_3d(0.0, 0.0, 0.0, 11, 3, 7);
        let b = tileable_perlin_3d(0.5, 0.5, 0.5, 11, 3, 7);
        assert!(
            (a - b).abs() > 1e-10,
            "Tileable Perlin should vary: {a} vs {b}"
        );
    }

    #[test]
    fn tiling_cache_creates_expected_layer_counts() {
        let cache = TilingCache::new();
        for mat in [MaterialKind::Bark, MaterialKind::Ground] {
            assert_eq!(cache.layer_count(mat, 0), 693);
            assert_eq!(cache.layer_count(mat, 1), 1155);
            assert_eq!(cache.layer_count(mat, 2), 525);
        }
    }

    #[test]
    fn tiling_cache_data_size_matches_layers() {
        let cache = TilingCache::new();
        for mat in [MaterialKind::Bark, MaterialKind::Ground] {
            for i in 0..CACHE_COUNT {
                assert_eq!(
                    cache.texture_data(mat, i).len(),
                    cache.layer_count(mat, i) * TILE_BYTES,
                    "Cache {i} data size mismatch"
                );
            }
        }
    }

    #[test]
    fn tiling_cache_tiles_have_variation() {
        let cache = TilingCache::new();
        for mat in [MaterialKind::Bark, MaterialKind::Ground] {
            for ci in 0..CACHE_COUNT {
                let data = cache.texture_data(mat, ci);
                let first = data[0];
                let has_variation = data[..TILE_BYTES].iter().any(|&b| b != first);
                assert!(has_variation, "Cache {ci} first tile has no variation");
            }
        }
    }

    #[test]
    fn bark_and_ground_tiles_differ() {
        // Same cache/position should produce different tile content for bark vs ground.
        let cache = TilingCache::new();
        let bark_data = cache.texture_data(MaterialKind::Bark, 0);
        let ground_data = cache.texture_data(MaterialKind::Ground, 0);
        // Compare first tile.
        assert_ne!(
            &bark_data[..TILE_BYTES],
            &ground_data[..TILE_BYTES],
            "Bark and ground first tiles should differ"
        );
    }

    #[test]
    fn adjacent_tiles_match_at_shared_edge() {
        // Two adjacent Xz tiles (Y-faces) at modular x coords 0 and 1.
        let cache = TilingCache::new();
        let periods = cache.periods(0);
        let [_px, py, pz] = periods;
        let tpap = cache.tiles_per_axis_pair(0);

        for mat in [MaterialKind::Bark, MaterialKind::Ground] {
            let data = cache.texture_data(mat, 0);

            let my = 0;
            let mz = 0;
            let ap = AxisPair::Xz as usize;

            let layer0 = ap * tpap + (0 * py * pz + my * pz + mz) as usize;
            let tile0 = &data[layer0 * TILE_BYTES..(layer0 + 1) * TILE_BYTES];

            let layer1 = ap * tpap + (1 * py * pz + my * pz + mz) as usize;
            let tile1 = &data[layer1 * TILE_BYTES..(layer1 + 1) * TILE_BYTES];

            for ty in 0..TILE_SIZE as usize {
                let right_edge = tile0[ty * TILE_SIZE as usize + (TILE_SIZE - 1) as usize];
                let left_edge = tile1[ty * TILE_SIZE as usize];
                assert_eq!(
                    right_edge, left_edge,
                    "{mat:?} edge mismatch at ty={ty}: right={right_edge}, left={left_edge}"
                );
            }
        }
    }

    #[test]
    fn tiles_wrap_around_period_boundary() {
        let cache = TilingCache::new();
        let periods = cache.periods(0);
        let [px, py, pz] = periods;
        let tpap = cache.tiles_per_axis_pair(0);

        for mat in [MaterialKind::Bark, MaterialKind::Ground] {
            let data = cache.texture_data(mat, 0);

            let my = 1;
            let mz = 2;
            let ap = AxisPair::Xz as usize;

            let layer_last = ap * tpap + ((px - 1) * py * pz + my * pz + mz) as usize;
            let tile_last = &data[layer_last * TILE_BYTES..(layer_last + 1) * TILE_BYTES];

            let layer_first = ap * tpap + (0 * py * pz + my * pz + mz) as usize;
            let tile_first = &data[layer_first * TILE_BYTES..(layer_first + 1) * TILE_BYTES];

            for ty in 0..TILE_SIZE as usize {
                let right = tile_last[ty * TILE_SIZE as usize + (TILE_SIZE - 1) as usize];
                let left = tile_first[ty * TILE_SIZE as usize];
                assert_eq!(
                    right, left,
                    "{mat:?} wrap mismatch at ty={ty}: right={right}, left={left}"
                );
            }
        }
    }

    #[test]
    fn axis_pair_from_face_idx_correct() {
        assert_eq!(AxisPair::from_face_idx(0), AxisPair::Zy); // +X
        assert_eq!(AxisPair::from_face_idx(1), AxisPair::Zy); // -X
        assert_eq!(AxisPair::from_face_idx(2), AxisPair::Xz); // +Y
        assert_eq!(AxisPair::from_face_idx(3), AxisPair::Xz); // -Y
        assert_eq!(AxisPair::from_face_idx(4), AxisPair::Xy); // +Z
        assert_eq!(AxisPair::from_face_idx(5), AxisPair::Xy); // -Z
    }

    #[test]
    fn tileable_fractal_noise_range() {
        // Fractal noise is normalized by max_value, should stay in [-1, 1].
        for i in 0..500 {
            let x = i as f64 * 0.19 + 0.05;
            let y = i as f64 * 0.23 + 0.07;
            let z = i as f64 * 0.31 + 0.11;
            let val = tileable_fractal_noise_3d(x, y, z, 88, 24, 56, OCTAVES);
            assert!(
                (-1.0..=1.0).contains(&val),
                "Fractal noise out of [-1,1]: {val} at ({x},{y},{z})"
            );
        }
    }

    #[test]
    fn bark_warp_noise_tiles_correctly() {
        // Bark noise (with domain warping) should tile at the cache periods.
        for ci in 0..CACHE_COUNT {
            let [px, py, pz] = CACHE_PERIODS[ci];
            for i in 0..10 {
                let x = i as f64 * 0.7 + 0.3;
                let y = i as f64 * 0.5 + 0.1;
                let z = i as f64 * 0.6 + 0.2;
                let base = sample_material_noise(x, y, z, px, py, pz, MaterialKind::Bark);
                let tiled = sample_material_noise(
                    x + px as f64,
                    y + py as f64,
                    z + pz as f64,
                    px,
                    py,
                    pz,
                    MaterialKind::Bark,
                );
                assert!(
                    (base - tiled).abs() < 1e-10,
                    "Bark noise tiling failed for cache {ci} at ({x},{y},{z}): {base} vs {tiled}"
                );
            }
        }
    }

    #[test]
    fn cache_periods_no_shared_axis() {
        // Core invariant: no two caches share the same period on any axis.
        for axis in 0..3 {
            let values: Vec<i32> = (0..CACHE_COUNT).map(|c| CACHE_PERIODS[c][axis]).collect();
            for i in 0..values.len() {
                for j in (i + 1)..values.len() {
                    assert_ne!(
                        values[i], values[j],
                        "Caches {i} and {j} share period {} on axis {axis}",
                        values[i]
                    );
                }
            }
        }
    }

    #[test]
    fn tile_pixels_not_saturated() {
        // Tiles should have interior values, not be clamped to all-0 or all-255.
        let cache = TilingCache::new();
        for mat in [MaterialKind::Bark, MaterialKind::Ground] {
            for ci in 0..CACHE_COUNT {
                let data = cache.texture_data(mat, ci);
                let tile = &data[..TILE_BYTES];
                let min = *tile.iter().min().unwrap();
                let max = *tile.iter().max().unwrap();
                assert!(
                    max - min > 20,
                    "{mat:?} cache {ci} first tile has narrow range [{min}, {max}]"
                );
            }
        }
    }

    #[test]
    fn adjacent_tiles_match_all_axis_pairs() {
        // Verify edge matching for all three axis pairs, not just Xz.
        let cache = TilingCache::new();
        let periods = cache.periods(0);
        let [_px, py, pz] = periods;
        let tpap = cache.tiles_per_axis_pair(0);
        let data = cache.texture_data(MaterialKind::Bark, 0);

        // For each axis pair, test U-direction adjacency (incrementing the
        // first varying axis's modular coordinate by 1).
        for (ap_idx, ap) in [AxisPair::Xz, AxisPair::Zy, AxisPair::Xy]
            .iter()
            .enumerate()
        {
            // Increment the U-axis modular coord. For Xz: U=X (mx), Zy: U=Z (mz), Xy: U=X (mx).
            let (m0, mut m1) = ([0i32; 3], [0i32; 3]);
            match ap {
                AxisPair::Xz => {
                    m1[0] = 1; // mx: 0 → 1
                }
                AxisPair::Zy => {
                    m1[2] = 1; // mz: 0 → 1
                }
                AxisPair::Xy => {
                    m1[0] = 1; // mx: 0 → 1
                }
            }

            let layer0 = ap_idx * tpap + (m0[0] * py * pz + m0[1] * pz + m0[2]) as usize;
            let layer1 = ap_idx * tpap + (m1[0] * py * pz + m1[1] * pz + m1[2]) as usize;

            let tile0 = &data[layer0 * TILE_BYTES..(layer0 + 1) * TILE_BYTES];
            let tile1 = &data[layer1 * TILE_BYTES..(layer1 + 1) * TILE_BYTES];

            // Right edge of tile0 (tx=15) should match left edge of tile1 (tx=0).
            for ty in 0..TILE_SIZE as usize {
                let right = tile0[ty * TILE_SIZE as usize + (TILE_SIZE - 1) as usize];
                let left = tile1[ty * TILE_SIZE as usize];
                assert_eq!(
                    right, left,
                    "{ap:?} U-edge mismatch at ty={ty}: right={right}, left={left}"
                );
            }
        }
    }

    #[test]
    fn adjacent_tiles_match_v_direction() {
        // Verify V-direction edge matching (incrementing second varying axis).
        let cache = TilingCache::new();
        let periods = cache.periods(0);
        let [_px, _py, _pz] = periods;
        let tpap = cache.tiles_per_axis_pair(0);
        let data = cache.texture_data(MaterialKind::Bark, 0);

        // Xz axis pair: V = Z. Tiles at mz=0 and mz=1 should match at
        // bottom edge (ty=15) of mz=0 and top edge (ty=0) of mz=1.
        let ap = AxisPair::Xz as usize;
        let layer0 = ap * tpap; // mx=0, my=0, mz=0
        let layer1 = ap * tpap + 1; // mx=0, my=0, mz=1

        let tile0 = &data[layer0 * TILE_BYTES..(layer0 + 1) * TILE_BYTES];
        let tile1 = &data[layer1 * TILE_BYTES..(layer1 + 1) * TILE_BYTES];

        for tx in 0..TILE_SIZE as usize {
            let bottom = tile0[(TILE_SIZE as usize - 1) * TILE_SIZE as usize + tx];
            let top = tile1[tx]; // ty=0
            assert_eq!(
                bottom, top,
                "Xz V-edge mismatch at tx={tx}: bottom={bottom}, top={top}"
            );
        }
    }

    #[test]
    fn tileable_perlin_negative_coords() {
        // Tiling should work for negative input coordinates.
        let px = 7;
        let py = 5;
        let pz = 11;
        for i in 0..20 {
            let x = -(i as f64) * 0.37 - 0.5;
            let y = -(i as f64) * 0.23 - 0.3;
            let z = -(i as f64) * 0.41 - 0.7;
            let base = tileable_perlin_3d(x, y, z, px, py, pz);
            let tiled = tileable_perlin_3d(x + px as f64, y + py as f64, z + pz as f64, px, py, pz);
            assert!(
                (base - tiled).abs() < 1e-10,
                "Negative-coord tiling failed at ({x},{y},{z}): {base} vs {tiled}"
            );
        }
    }

    #[test]
    fn bark_y_period_multiplier_is_exact() {
        // Guard: BARK_Y_COMPRESS * BASE_FREQ must be an exact integer
        // (BARK_Y_PERIOD_MULT) so tileable lattice wrapping works.
        let product = BARK_Y_COMPRESS * BASE_FREQ;
        assert_eq!(
            product, BARK_Y_PERIOD_MULT as f64,
            "BARK_Y_COMPRESS * BASE_FREQ must equal BARK_Y_PERIOD_MULT"
        );
    }
}
