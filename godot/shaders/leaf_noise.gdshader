// Procedural noise shader for leaf voxel surfaces.
//
// Replaces the alpha-scissor texture approach with GPU-computed noise.
// Outputs boolean alpha via discard: fragments where the noise value is
// below the threshold are discarded, creating organic-looking leaf edges.
// No UVs needed — world-space position determines noise coordinates.
// Triplanar mapping blends noise from all three projection planes.
// Both sides rendered (cull disabled in the material, not the shader).
//
// A directional face tint simulates sky-dome ambient lighting: upward-facing
// surfaces are shifted warm and downward-facing surfaces cool, controlled by
// the `face_tint_strength` uniform.
//
// Wind sway: vertex displacement using sine waves keyed on world position
// and TIME. The sway weight is read from the R vertex color channel (0 =
// anchored at wood, 1 = full sway). This prevents tearing at leaf-wood
// seams. Amplitude also scales with world-space Y for a wind gradient.
//
// Vertex colors are metadata, not visual color:
//   R = sway weight (distance-from-wood, 0–1)
//   G, B, A = reserved for future use
// Base color is derived procedurally by the fragment shader.
//
// See also: smooth_solid.gdshader (solid surface noise),
// tree_renderer.gd (material setup), mesh_gen.rs (graphics crate, sway weight computation).

shader_type spatial;
render_mode cull_disabled;

// Noise frequency. Higher = smaller leaf features.
uniform float noise_freq : hint_range(1.0, 32.0) = 8.0;

// Alpha threshold: fragments with noise below this are discarded.
// Higher = more holes = sparser leaves.
uniform float alpha_threshold : hint_range(0.0, 1.0) = 0.4;

// Directional face tint strength. Controls how much upward-facing surfaces
// are warmed and downward-facing surfaces are cooled, simulating sky-dome
// ambient lighting. 0.0 = no tinting, 1.0 = full effect.
uniform float face_tint_strength : hint_range(0.0, 1.0) = 0.0;

// Wind sway strength (max displacement in world units at full sway weight).
uniform float wind_strength : hint_range(0.0, 2.0) = 0.15;
// Wind animation speed multiplier.
uniform float wind_speed : hint_range(0.0, 5.0) = 0.6;
// Primary wind direction (XZ plane). Normalized internally.
uniform vec3 wind_direction = vec3(1.0, 0.0, 0.4);
// Reference Y for height-based sway scaling. Leaves at this Y get no
// height bonus; leaves above get progressively more sway.
uniform float wind_base_y = 100.0;
// How much extra sway per world unit above wind_base_y.
uniform float wind_height_scale : hint_range(0.0, 0.1) = 0.01;

// World-space position and normal passed from vertex to fragment shader.
// world_pos is the *undisplaced* rest position — used by the fragment shader
// for stable noise sampling so the pattern doesn't swim with wind sway.
varying vec3 world_pos;
varying vec3 world_normal;

void vertex() {
	// Sway weight from vertex color R channel (0 = anchored, 1 = full sway).
	float sway_weight = COLOR.r;

	// Undisplaced world position — stable anchor for fragment noise.
	vec3 rest_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	world_pos = rest_pos;
	world_normal = (MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz;

	// Compute wind displacement.
	vec3 wind_dir = normalize(wind_direction);

	// Two overlapping sine waves at different frequencies for organic motion.
	float phase1 = dot(rest_pos.xz, wind_dir.xz) * 0.8 + TIME * wind_speed * 1.2;
	float phase2 = dot(rest_pos.xz, vec2(wind_dir.z, -wind_dir.x)) * 1.3 + TIME * wind_speed * 0.7;
	float wave = sin(phase1) * 0.7 + sin(phase2) * 0.3;

	// Height-based amplitude: more sway higher up.
	float height_factor = 1.0 + max(rest_pos.y - wind_base_y, 0.0) * wind_height_scale;
	float displacement = wave * wind_strength * sway_weight * height_factor;

	// Displace in wind direction (XZ) with a slight vertical bob.
	// Offset is world-space; transform to local space via the inverse model
	// rotation. For uniform-scale meshes, transpose(mat3(MODEL_MATRIX)) is
	// equivalent to the inverse rotation and avoids a full matrix inverse.
	vec3 world_offset = vec3(wind_dir.x * displacement, displacement * 0.15, wind_dir.z * displacement);
	VERTEX += (transpose(mat3(MODEL_MATRIX)) * world_offset);
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
	// Base leaf green — derived procedurally, not from vertex color (which
	// now carries sway weight metadata, not visual color).
	vec3 base = vec3(0.12, 0.42, 0.08);
	// Slight blue-green tint on average; hue variation is subtle.
	vec3 tinted = base + vec3(hue_shift * 0.3 - 0.02, hue_shift * 0.1, -hue_shift * 0.2 + 0.03);
	// Directional face tint: warm top faces, cool bottom faces.
	float up_dot = dot(normalize(world_normal), vec3(0.0, 1.0, 0.0));
	vec3 face_tint = vec3(1.0) + up_dot * face_tint_strength * vec3(0.08, 0.04, -0.12);

	ALBEDO = clamp(tinted * brightness * face_tint, vec3(0.0), vec3(1.0));
}
