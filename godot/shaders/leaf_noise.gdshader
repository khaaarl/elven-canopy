// Procedural noise shader for leaf voxel surfaces.
//
// Replaces the alpha-scissor texture approach with GPU-computed noise.
// Outputs boolean alpha via discard: fragments where the noise value is
// below the threshold are discarded, creating organic-looking leaf edges.
// No UVs needed — world-space position determines noise coordinates.
// Triplanar mapping blends noise from all three projection planes.
// Both sides rendered (cull disabled in the material, not the shader).
//
// See also: smooth_solid.gdshader (solid surface noise),
// tree_renderer.gd (material setup).

shader_type spatial;
render_mode cull_disabled;

// Noise frequency. Higher = smaller leaf features.
uniform float noise_freq : hint_range(1.0, 32.0) = 8.0;

// Alpha threshold: fragments with noise below this are discarded.
// Higher = more holes = sparser leaves.
uniform float alpha_threshold : hint_range(0.0, 1.0) = 0.4;

// World-space position and normal passed from vertex to fragment shader.
varying vec3 world_pos;
varying vec3 world_normal;

void vertex() {
	world_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	world_normal = (MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz;
}

// Integer hash: maps 2D integer coords to a pseudo-random float in [0, 1].
float hash2(int x, int y) {
	int h = x * 73856093 ^ y * 19349663;
	h = (h ^ (h >> 13)) * 1274126177;
	h = h ^ (h >> 16);
	return float(h & 0xFFFF) / 65535.0;
}

// 2D value noise with smoothstep interpolation.
float value_noise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);

	float tx = f.x * f.x * (3.0 - 2.0 * f.x);
	float ty = f.y * f.y * (3.0 - 2.0 * f.y);

	int ix = int(i.x);
	int iy = int(i.y);

	float v00 = hash2(ix, iy);
	float v10 = hash2(ix + 1, iy);
	float v01 = hash2(ix, iy + 1);
	float v11 = hash2(ix + 1, iy + 1);

	float a = mix(v00, v10, tx);
	float b = mix(v01, v11, tx);
	return mix(a, b, ty);
}

// Multi-octave value noise (2 octaves).
float fbm(vec2 p) {
	float val = 0.0;
	float amp = 0.6;
	float freq = 1.0;
	for (int i = 0; i < 2; i++) {
		val += amp * value_noise(p * freq);
		freq *= 2.17;
		amp *= 0.45;
	}
	return val;
}

void fragment() {
	vec3 abs_n = abs(world_normal);
	float weight_sum = abs_n.x + abs_n.y + abs_n.z;
	vec3 blend = abs_n / max(weight_sum, 0.001);

	// Triplanar noise sampling.
	float noise_y = fbm(world_pos.xz * noise_freq);
	float noise_x = fbm(vec2(world_pos.z, world_pos.y) * noise_freq);
	float noise_z = fbm(vec2(world_pos.x, world_pos.y) * noise_freq);

	float noise = blend.y * noise_y + blend.x * noise_x + blend.z * noise_z;

	// Use Godot's alpha scissor pipeline instead of manual discard.
	// This uses the hardware alpha test path which preserves early-Z.
	ALPHA = noise;
	ALPHA_SCISSOR_THRESHOLD = alpha_threshold;

	// Second noise layer at lower frequency for color/brightness variation.
	// Uses an offset to decorrelate from the alpha noise pattern.
	float color_noise_y = fbm((world_pos.xz + vec2(37.1, 91.3)) * noise_freq * 0.5);
	float color_noise_x = fbm((vec2(world_pos.z, world_pos.y) + vec2(37.1, 91.3)) * noise_freq * 0.5);
	float color_noise_z = fbm((vec2(world_pos.x, world_pos.y) + vec2(37.1, 91.3)) * noise_freq * 0.5);
	float color_noise = blend.y * color_noise_y + blend.x * color_noise_x + blend.z * color_noise_z;

	// Strong brightness variation, subtle hue shift, overall darker/bluer.
	float brightness = 0.4 + (noise - 0.3) * 2.0;
	float hue_shift = (color_noise - 0.35) * 0.2;
	vec3 base = COLOR.rgb;
	// Slight blue-green tint on average; hue variation is subtle.
	vec3 tinted = base + vec3(hue_shift * 0.3 - 0.02, hue_shift * 0.1, -hue_shift * 0.2 + 0.03);
	ALBEDO = clamp(tinted * brightness, vec3(0.0), vec3(1.0));
}
