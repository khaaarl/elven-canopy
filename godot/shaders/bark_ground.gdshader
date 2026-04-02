// Prime-period tiling shader for bark and ground surfaces.
//
// Samples three monochrome Texture2DArray caches (A, B, C) at different
// prime tiling periods per world axis. The fragment's world position and
// face normal determine which tile layer to sample and the UV within
// that tile. The three samples are blended additively and multiplied by
// vertex color to produce the final albedo.
//
// No per-vertex UVs are needed — everything is derived from world position.
//
// See also: texture_gen.rs (graphics crate, tile generation), tree_renderer.gd (material setup).

shader_type spatial;
render_mode vertex_lighting;

// Three tiling caches as Texture2DArray samplers (R8 format).
uniform sampler2DArray cache_a : filter_nearest, repeat_disable;
uniform sampler2DArray cache_b : filter_nearest, repeat_disable;
uniform sampler2DArray cache_c : filter_nearest, repeat_disable;

// Periods [px, py, pz] for each cache.
uniform ivec3 periods_a;
uniform ivec3 periods_b;
uniform ivec3 periods_c;

// Tiles per axis pair (px * py * pz) for each cache.
uniform int tpap_a;
uniform int tpap_b;
uniform int tpap_c;

// World-space position and normal passed from vertex to fragment shader.
// NORMAL in the fragment shader is view-space, which rotates with the
// camera — useless for axis-pair detection. We pass the world-space
// normal explicitly so the axis pair is stable regardless of view angle.
varying vec3 world_pos;
varying vec3 world_normal;

void vertex() {
	world_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	// Chunks have no rotation or non-uniform scale, so the model matrix
	// direction transform is just the upper-left 3x3 (w=0 drops translation).
	world_normal = (MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz;
}

// Compute the Texture2DArray layer index and tile UV for a tiling cache.
//
// axis_pair: 0 = Xz (Y faces), 1 = Zy (X faces), 2 = Xy (Z faces)
// tpap: tiles per axis pair (px * py * pz)
int compute_layer(vec3 wp, vec3 abs_n, ivec3 periods, int tpap, out vec2 tile_uv) {
	int axis_pair;
	ivec3 voxel;

	if (abs_n.y >= abs_n.x && abs_n.y >= abs_n.z) {
		// Y face: UV = (X, Z), fixed = Y
		axis_pair = 0;
		voxel = ivec3(int(floor(wp.x)), int(round(wp.y)), int(floor(wp.z)));
		tile_uv = vec2(fract(wp.x), fract(wp.z));
	} else if (abs_n.x >= abs_n.z) {
		// X face: UV = (Z, Y), fixed = X
		axis_pair = 1;
		voxel = ivec3(int(round(wp.x)), int(floor(wp.y)), int(floor(wp.z)));
		tile_uv = vec2(fract(wp.z), fract(wp.y));
	} else {
		// Z face: UV = (X, Y), fixed = Z
		axis_pair = 2;
		voxel = ivec3(int(floor(wp.x)), int(floor(wp.y)), int(round(wp.z)));
		tile_uv = vec2(fract(wp.x), fract(wp.y));
	}

	// Euclidean mod for negative coordinates.
	int mx = ((voxel.x % periods.x) + periods.x) % periods.x;
	int my = ((voxel.y % periods.y) + periods.y) % periods.y;
	int mz = ((voxel.z % periods.z) + periods.z) % periods.z;

	// Layer index: axis_pair * tpap + mx * py * pz + my * pz + mz
	int layer = axis_pair * tpap + mx * periods.y * periods.z + my * periods.z + mz;
	return layer;
}

void fragment() {
	vec3 abs_n = abs(world_normal);
	vec2 uv_a, uv_b, uv_c;

	int layer_a = compute_layer(world_pos, abs_n, periods_a, tpap_a, uv_a);
	int layer_b = compute_layer(world_pos, abs_n, periods_b, tpap_b, uv_b);
	int layer_c = compute_layer(world_pos, abs_n, periods_c, tpap_c, uv_c);

	float val_a = texture(cache_a, vec3(uv_a, float(layer_a))).r;
	float val_b = texture(cache_b, vec3(uv_b, float(layer_b))).r;
	float val_c = texture(cache_c, vec3(uv_c, float(layer_c))).r;

	// Additive blend, normalized to [0, 1].
	float combined = (val_a + val_b + val_c) / 3.0;

	// Multiply by vertex color (material-specific color ramp).
	ALBEDO = COLOR.rgb * combined;
}
