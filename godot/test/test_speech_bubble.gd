## Unit tests for speech_bubble.gd static geometry and texture helpers.
##
## Covers the permanent infrastructure: rounded-rect hit testing,
## border detection, and background texture generation.
##
## See also: speech_bubble.gd for the implementation.
extends GutTest

const SpeechBubble = preload("res://scripts/speech_bubble.gd")

# -- _in_rounded_rect -------------------------------------------------------


func test_center_pixel_is_inside() -> void:
	assert_true(SpeechBubble._in_rounded_rect(20, 5, 40, 12, 3), "Center of rect should be inside")


func test_top_left_corner_outside_radius() -> void:
	# (0, 0) is outside the corner radius circle centered at (3, 3).
	assert_false(
		SpeechBubble._in_rounded_rect(0, 0, 40, 12, 3),
		"Top-left corner outside radius should be outside"
	)


func test_top_left_corner_inside_radius() -> void:
	# (3, 3) is the corner center — should be inside.
	assert_true(SpeechBubble._in_rounded_rect(3, 3, 40, 12, 3), "Corner center should be inside")


func test_mid_top_edge_inside() -> void:
	# (20, 0) is on the top edge but not in a corner — inside.
	assert_true(
		SpeechBubble._in_rounded_rect(20, 0, 40, 12, 3), "Mid top edge (no corner) should be inside"
	)


func test_mid_left_edge_inside() -> void:
	# (0, 6) is on the left edge, vertically centered — inside.
	assert_true(
		SpeechBubble._in_rounded_rect(0, 6, 40, 12, 3), "Mid left edge (no corner) should be inside"
	)


func test_outside_rect_entirely() -> void:
	assert_false(
		SpeechBubble._in_rounded_rect(50, 50, 40, 12, 3), "Point outside rect should be outside"
	)


# -- _on_rounded_rect_border ------------------------------------------------


func test_top_edge_is_border() -> void:
	# (20, 0) — top edge, not in corner region, should be border.
	assert_true(
		SpeechBubble._on_rounded_rect_border(20, 0, 40, 12, 3),
		"Top edge non-corner pixel should be border"
	)


func test_left_edge_is_border() -> void:
	# (0, 6) — left edge, mid-height, should be border.
	assert_true(
		SpeechBubble._on_rounded_rect_border(0, 6, 40, 12, 3),
		"Left edge mid-height pixel should be border"
	)


func test_interior_is_not_border() -> void:
	# (20, 5) — center, definitely interior.
	assert_false(
		SpeechBubble._on_rounded_rect_border(20, 5, 40, 12, 3),
		"Interior pixel should not be border"
	)


func test_outside_is_not_border() -> void:
	# (0, 0) — outside the rounded rect, not border.
	assert_false(
		SpeechBubble._on_rounded_rect_border(0, 0, 40, 12, 3),
		"Pixel outside rounded rect should not be border"
	)


# -- _get_bg_texture ---------------------------------------------------------


func test_bg_texture_returns_correct_dimensions() -> void:
	var tex: ImageTexture = SpeechBubble._get_bg_texture(80, 30)
	assert_not_null(tex, "Texture should not be null")
	assert_eq(tex.get_width(), 80, "Texture width should match requested")
	assert_eq(tex.get_height(), 30, "Texture height should match requested")


func test_bg_texture_is_cached() -> void:
	var tex1: ImageTexture = SpeechBubble._get_bg_texture(60, 25)
	var tex2: ImageTexture = SpeechBubble._get_bg_texture(60, 25)
	assert_same(tex1, tex2, "Same dimensions should return cached texture instance")


func test_bg_texture_different_sizes_not_same() -> void:
	var tex1: ImageTexture = SpeechBubble._get_bg_texture(50, 20)
	var tex2: ImageTexture = SpeechBubble._get_bg_texture(70, 20)
	assert_ne(tex1, tex2, "Different widths should produce different textures")


# -- _in_rounded_rect: additional corners ------------------------------------


func test_bottom_right_corner_outside_radius() -> void:
	# (39, 11) is the bottom-right corner pixel — outside the corner radius.
	assert_false(
		SpeechBubble._in_rounded_rect(39, 11, 40, 12, 3),
		"Bottom-right corner outside radius should be outside"
	)


func test_bottom_right_corner_inside_radius() -> void:
	# (36, 8) is the bottom-right corner center — should be inside.
	assert_true(
		SpeechBubble._in_rounded_rect(36, 8, 40, 12, 3),
		"Bottom-right corner center should be inside"
	)


# -- Lifecycle tests (scene-tree integration) --------------------------------


func test_show_speech_sets_text_and_activates() -> void:
	var bubble := Node3D.new()
	bubble.set_script(SpeechBubble)
	add_child_autofree(bubble)
	bubble.show_speech("Hello!")
	assert_true(bubble.visible, "Bubble should be visible after show_speech")
	assert_false(bubble.is_expired(), "Bubble should not be expired after show_speech")
	assert_eq(bubble._label.text, "Hello!", "Label text should match")


func test_is_expired_true_before_show() -> void:
	var bubble := Node3D.new()
	bubble.set_script(SpeechBubble)
	add_child_autofree(bubble)
	# Before show_speech, bubble is inactive (not visible, _active = false).
	assert_true(bubble.is_expired(), "Bubble should be expired before show_speech")


func test_show_speech_generates_background_texture() -> void:
	var bubble := Node3D.new()
	bubble.set_script(SpeechBubble)
	add_child_autofree(bubble)
	bubble.show_speech("Hello, friend!")
	assert_not_null(bubble._bg.texture, "Background texture should be generated")
	# Text is ~14 chars; background should be wider than just padding.
	assert_gt(bubble._bg.texture.get_width(), SpeechBubble.PAD_X * 2, "Bg width > padding")
	assert_gt(bubble._bg.texture.get_height(), SpeechBubble.PAD_Y * 2, "Bg height > padding")


func test_show_speech_resets_timer() -> void:
	var bubble := Node3D.new()
	bubble.set_script(SpeechBubble)
	add_child_autofree(bubble)
	bubble.show_speech("First")
	# Simulate some elapsed time by directly setting the timer.
	bubble._display_timer = 2.5
	bubble.show_speech("Second")
	assert_eq(bubble._display_timer, 0.0, "Timer should reset on new show_speech")
	assert_eq(bubble._label.text, "Second", "Text should be updated")


func test_show_speech_kills_active_fade() -> void:
	var bubble := Node3D.new()
	bubble.set_script(SpeechBubble)
	add_child_autofree(bubble)
	bubble.show_speech("Fading soon")
	# Force-start a fade (simulates timer expiry).
	bubble._start_fade()
	assert_not_null(bubble._fade_tween, "Fade tween should be active")
	# Now show new speech — should kill the fade.
	bubble.show_speech("Interrupting!")
	assert_null(bubble._fade_tween, "Fade tween should be killed by new show_speech")
	assert_true(bubble._active, "Bubble should be active after interrupting fade")
	assert_eq(bubble._label.modulate.a, 1.0, "Label alpha should be fully opaque after reset")
	assert_eq(bubble._bg.modulate.a, 1.0, "Bg alpha should be fully opaque after reset")


func test_on_fade_complete_marks_expired() -> void:
	var bubble := Node3D.new()
	bubble.set_script(SpeechBubble)
	add_child_autofree(bubble)
	bubble.show_speech("Goodbye")
	assert_false(bubble.is_expired(), "Should be active")
	# Directly call the fade completion handler.
	bubble._on_fade_complete()
	assert_true(bubble.is_expired(), "Should be expired after fade complete")
	assert_false(bubble.visible, "Should be hidden after fade complete")
