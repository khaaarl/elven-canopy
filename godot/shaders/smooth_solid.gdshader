// Procedural noise shader for smooth solid voxel surfaces (bark and ground).
//
// Replaces the old texture-array tiling system with GPU-computed value noise.
// Uses integer hashing and triangle-wave interpolation — no trig functions,
// no texture lookups. World-space position determines the noise coordinates,
// and the face normal selects the 2D projection plane (same axis-pair logic
// as the old shader).
//
// Two octaves of noise are blended and multiplied by the vertex color to
// produce organic-looking surface variation. Bark surfaces can use
// anisotropic scaling (compress Y axis for vertical grain lines) via the
// `y_scale` uniform; ground surfaces set `y_scale = 1.0` for isotropic noise.
//
// A directional face tint simulates sky-dome ambient lighting: upward-facing
// surfaces are shifted warm (yellow-ish) and downward-facing surfaces cool
// (blue-ish), controlled by the `face_tint_strength` uniform.
//
// See also: smooth_mesh.rs (geometry pipeline), tree_renderer.gd (material
// setup), bark_ground.gdshader (old texture-based approach, kept for
// reference).

shader_type spatial;

// Y-axis frequency multiplier for anisotropic noise. Bark uses ~3.0 for
// vertical grain lines; ground uses 1.0 for isotropic noise.
uniform float y_scale : hint_range(0.1, 5.0) = 1.0;

// Directional face tint strength. Controls how much upward-facing surfaces
// are warmed and downward-facing surfaces are cooled, simulating sky-dome
// ambient lighting. 0.0 = no tinting, 1.0 = full effect.
uniform float face_tint_strength : hint_range(0.0, 1.0) = 0.0;

// Overall noise frequency. Higher = more detail, smaller features.
uniform float noise_freq : hint_range(0.5, 32.0) = 2.0;

// Noise strength: how much the noise modulates the vertex color.
// 0.0 = flat vertex color, 1.0 = full noise modulation.
uniform float noise_strength : hint_range(0.0, 1.0) = 0.4;

// World-space position and normal passed from vertex to fragment shader.
varying vec3 world_pos;
varying vec3 world_normal;

void vertex() {
	world_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	world_normal = (MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz;
}

// Integer hash: maps 2D integer coords to a pseudo-random float in [0, 1].
// Pure integer arithmetic — no trig.
float hash2(int x, int y) {
	int h = x * 73856093 ^ y * 19349663;
	h = (h ^ (h >> 13)) * 1274126177;
	h = h ^ (h >> 16);
	return float(h & 0xFFFF) / 65535.0;
}

// 2D value noise at the given coordinates. Uses integer hashing at grid
// points and bilinear interpolation with smoothstep weights.
float value_noise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);

	float tx = f.x * f.x * (3.0 - 2.0 * f.x);
	float ty = f.y * f.y * (3.0 - 2.0 * f.y);

	int ix = int(i.x);
	int iy = int(i.y);

	// Four corner values.
	float v00 = hash2(ix, iy);
	float v10 = hash2(ix + 1, iy);
	float v01 = hash2(ix, iy + 1);
	float v11 = hash2(ix + 1, iy + 1);

	// Bilinear interpolation.
	float a = mix(v00, v10, tx);
	float b = mix(v01, v11, tx);
	return mix(a, b, ty);
}

// Multi-octave value noise (2 octaves for speed).
float fbm(vec2 p) {
	float val = 0.0;
	float amp = 0.6;
	float freq = 1.0;
	for (int i = 0; i < 2; i++) {
		val += amp * value_noise(p * freq);
		freq *= 2.17;  // non-integer ratio avoids grid alignment
		amp *= 0.45;    // persistence
	}
	return val;
}

void fragment() {
	vec3 abs_n = abs(world_normal);
	// Normalize the absolute normal weights so they sum to 1.
	// This gives smooth blending between projection planes.
	float weight_sum = abs_n.x + abs_n.y + abs_n.z;
	vec3 blend = abs_n / max(weight_sum, 0.001);

	vec3 scaled_pos = vec3(world_pos.x, world_pos.y * y_scale, world_pos.z);

	// Triplanar mapping: sample noise from all three projection planes
	// and blend by normal weight. This eliminates hard seams at axis
	// boundaries on chamfered/smoothed edges where the normal transitions
	// between face directions.
	float noise_y = fbm(scaled_pos.xz * noise_freq);  // Y face → XZ plane
	float noise_x = fbm(vec2(scaled_pos.z, scaled_pos.y) * noise_freq);  // X face → ZY
	float noise_z = fbm(vec2(scaled_pos.x, scaled_pos.y) * noise_freq);  // Z face → XY

	float noise = blend.y * noise_y + blend.x * noise_x + blend.z * noise_z;

	// Map noise from [0, ~0.7] to a modulation range centered on 1.0.
	// With noise_strength=0.4: range is roughly [0.72, 1.28].
	float modulation = 1.0 + (noise - 0.35) * noise_strength * 2.0;

	// Directional face tint: dot(normal, up) ranges from -1 (bottom) to +1
	// (top). Upward faces get a warm (yellow-ish) shift simulating sky light;
	// downward faces get a cool (blue-ish) shift simulating indirect/bounce
	// light. Side faces stay neutral. The tint is multiplied into the albedo
	// so it interacts naturally with both the noise modulation and Godot's
	// PBR lighting.
	float up_dot = dot(normalize(world_normal), vec3(0.0, 1.0, 0.0));
	// Warm shift for top faces, cool shift for bottom faces.
	vec3 tint = vec3(1.0) + up_dot * face_tint_strength * vec3(0.08, 0.04, -0.12);

	ALBEDO = COLOR.rgb * modulation * tint;
}
