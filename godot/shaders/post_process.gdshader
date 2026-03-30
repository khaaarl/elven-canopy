// Combined screen-space edge detection and depth fog post-process shader.
//
// A full-screen spatial shader that first applies edge darkening (Sobel filter
// on depth/normal discontinuities), then applies depth-based fog on top. This
// ordering ensures edge outlines appear crisp on nearby geometry and fade
// naturally into fog at distance, rather than darkening already-fogged pixels.
//
// Applied to a MeshInstance3D with a QuadMesh parented to the Camera3D. The
// vertex shader projects the quad to fill the screen. The fragment shader
// reads DEPTH_TEXTURE, NORMAL_ROUGHNESS_TEXTURE, and SCREEN_TEXTURE (only
// available in spatial shaders) and outputs the composited result.
//
// Edge uniforms:
//   edge_enabled     — master toggle for edge detection (0 = off, 1 = on)
//   depth_threshold  — sensitivity for depth edges (lower = more edges)
//   normal_threshold — sensitivity for normal edges (lower = more edges)
//   edge_strength    — overall edge intensity (0 = off, 1 = full)
//   edge_darken      — how much to darken surface color at edges (0–1)
//   edge_saturate    — how much to boost saturation at edges (0–1)
//
// Fog uniforms:
//   fog_enabled      — master toggle for fog (0 = off, 1 = on)
//   fog_begin        — depth (world units) where fog starts
//   fog_end          — depth (world units) where fog reaches full opacity
//   fog_color        — color to blend toward at distance
//
// See also: post_process_controller.gd (runtime controller),
// settings_panel.gd (UI toggles), game_config.gd (persistence).

shader_type spatial;
render_mode unshaded, depth_draw_never, depth_test_disabled, cull_disabled;

uniform sampler2D depth_texture : hint_depth_texture, filter_nearest;
uniform sampler2D normal_texture : hint_normal_roughness_texture, filter_nearest;
uniform sampler2D screen_texture : hint_screen_texture, filter_nearest;

// Edge detection uniforms.
uniform float edge_enabled : hint_range(0.0, 1.0) = 1.0;
uniform float depth_threshold : hint_range(0.0, 10.0) = 1.5;
uniform float normal_threshold : hint_range(0.0, 5.0) = 0.5;
uniform float edge_strength : hint_range(0.0, 1.0) = 0.6;
uniform float edge_darken : hint_range(0.0, 1.0) = 0.55;
uniform float edge_saturate : hint_range(0.0, 1.0) = 0.5;

// Fog uniforms.
uniform float fog_enabled : hint_range(0.0, 1.0) = 0.0;
uniform float fog_begin = 80.0;
uniform float fog_end = 160.0;
uniform vec4 fog_color : source_color = vec4(0.55, 0.7, 0.85, 1.0);

// Reconstruct linear depth from the depth buffer value.
float get_linear_depth(vec2 uv, mat4 inv_proj) {
	float raw_depth = texture(depth_texture, uv).r;
	vec3 ndc = vec3(uv * 2.0 - 1.0, raw_depth);
	vec4 view_pos = inv_proj * vec4(ndc, 1.0);
	return -view_pos.z / view_pos.w;
}

void vertex() {
	// Force the quad to fill the entire screen regardless of camera transform.
	POSITION = vec4(VERTEX.xy, 1.0, 1.0);
}

void fragment() {
	vec2 pixel_size = vec2(1.0 / VIEWPORT_SIZE.x, 1.0 / VIEWPORT_SIZE.y);
	mat4 inv_proj = INV_PROJECTION_MATRIX;

	// Always read center depth (needed for both edges and fog).
	float center_depth = get_linear_depth(SCREEN_UV, inv_proj);

	// Read the original scene color.
	vec3 color = texture(screen_texture, SCREEN_UV).rgb;

	// --- Edge detection (optional) ---
	if (edge_enabled > 0.5) {
		// Sample depth in a 3x3 neighborhood.
		float d_tl = get_linear_depth(SCREEN_UV + vec2(-pixel_size.x,  pixel_size.y), inv_proj);
		float d_t  = get_linear_depth(SCREEN_UV + vec2(          0.0,  pixel_size.y), inv_proj);
		float d_tr = get_linear_depth(SCREEN_UV + vec2( pixel_size.x,  pixel_size.y), inv_proj);
		float d_l  = get_linear_depth(SCREEN_UV + vec2(-pixel_size.x,           0.0), inv_proj);
		float d_r  = get_linear_depth(SCREEN_UV + vec2( pixel_size.x,           0.0), inv_proj);
		float d_bl = get_linear_depth(SCREEN_UV + vec2(-pixel_size.x, -pixel_size.y), inv_proj);
		float d_b  = get_linear_depth(SCREEN_UV + vec2(          0.0, -pixel_size.y), inv_proj);
		float d_br = get_linear_depth(SCREEN_UV + vec2( pixel_size.x, -pixel_size.y), inv_proj);

		// Sobel gradients for depth.
		float depth_gx = d_tl + 2.0 * d_l + d_bl - d_tr - 2.0 * d_r - d_br;
		float depth_gy = d_tl + 2.0 * d_t + d_tr - d_bl - 2.0 * d_b - d_br;
		float depth_edge = sqrt(depth_gx * depth_gx + depth_gy * depth_gy);

		// Sample normals in a 3x3 neighborhood. Normal-roughness texture stores
		// normals in RGB as [0,1]. The Sobel filter computes differences, so the
		// bias cancels out — no need to remap to [-1,1].
		vec3 n_tl = texture(normal_texture, SCREEN_UV + vec2(-pixel_size.x,  pixel_size.y)).rgb;
		vec3 n_t  = texture(normal_texture, SCREEN_UV + vec2(          0.0,  pixel_size.y)).rgb;
		vec3 n_tr = texture(normal_texture, SCREEN_UV + vec2( pixel_size.x,  pixel_size.y)).rgb;
		vec3 n_l  = texture(normal_texture, SCREEN_UV + vec2(-pixel_size.x,           0.0)).rgb;
		vec3 n_r  = texture(normal_texture, SCREEN_UV + vec2( pixel_size.x,           0.0)).rgb;
		vec3 n_bl = texture(normal_texture, SCREEN_UV + vec2(-pixel_size.x, -pixel_size.y)).rgb;
		vec3 n_b  = texture(normal_texture, SCREEN_UV + vec2(          0.0, -pixel_size.y)).rgb;
		vec3 n_br = texture(normal_texture, SCREEN_UV + vec2( pixel_size.x, -pixel_size.y)).rgb;

		// Sobel gradients for normals (per-channel, then magnitude).
		vec3 normal_gx = n_tl + 2.0 * n_l + n_bl - n_tr - 2.0 * n_r - n_br;
		vec3 normal_gy = n_tl + 2.0 * n_t + n_tr - n_bl - 2.0 * n_b - n_br;
		float normal_edge = sqrt(dot(normal_gx, normal_gx) + dot(normal_gy, normal_gy));

		// Combine depth and normal edges with separate thresholds.
		float depth_factor = smoothstep(0.0, depth_threshold, depth_edge);
		float normal_factor = smoothstep(0.0, normal_threshold, normal_edge);
		float edge = max(depth_factor, normal_factor);

		// Darken: lerp toward zero.
		vec3 darkened = color * (1.0 - edge_darken);
		// Boost saturation: pull away from luminance toward the color channel.
		float lum = dot(darkened, vec3(0.2126, 0.7152, 0.0722));
		vec3 edge_col = mix(vec3(lum), darkened, 1.0 + edge_saturate);

		color = mix(color, edge_col, edge * edge_strength);
	}

	// --- Fog (optional, applied after edges) ---
	if (fog_enabled > 0.5) {
		float fog_factor = clamp((center_depth - fog_begin) / (fog_end - fog_begin), 0.0, 1.0);
		color = mix(color, fog_color.rgb, fog_factor);
	}

	ALBEDO = color;
}
