## Unit tests for speech_bubble_manager.gd.
##
## The manager's core loop is bridge-dependent (polls get_recent_dialogue),
## so most behavior is covered by integration/puppet tests. These tests
## cover the pool and bubble-management logic that can be exercised without
## a live SimBridge.
##
## See also: speech_bubble_manager.gd for the implementation.
extends GutTest

const SpeechBubbleManager = preload("res://scripts/speech_bubble_manager.gd")


func test_show_bubble_creates_child() -> void:
	var mgr: Node3D = SpeechBubbleManager.new()
	add_child_autofree(mgr)
	mgr._show_bubble("creature-1", "Hello there!")
	assert_eq(mgr._active_bubbles.size(), 1, "Should have one active bubble")
	assert_true(mgr._active_bubbles.has("creature-1"), "Bubble keyed by creature id")


func test_show_bubble_reuses_existing() -> void:
	var mgr: Node3D = SpeechBubbleManager.new()
	add_child_autofree(mgr)
	mgr._show_bubble("creature-1", "Hello!")
	mgr._show_bubble("creature-1", "Updated!")
	assert_eq(mgr._active_bubbles.size(), 1, "Should still have one bubble after update")


func test_show_bubble_pools_expired() -> void:
	var mgr: Node3D = SpeechBubbleManager.new()
	add_child_autofree(mgr)
	mgr._show_bubble("creature-1", "Hello!")
	# Force the bubble to expire by deactivating it (is_expired checks !_active).
	var bubble: Node3D = mgr._active_bubbles["creature-1"]
	bubble._active = false
	mgr._reclaim_expired()
	assert_eq(mgr._active_bubbles.size(), 0, "Expired bubble should be reclaimed")
	assert_eq(mgr._pool.size(), 1, "Expired bubble should be in pool")


func test_show_bubble_reuses_pooled_node() -> void:
	var mgr: Node3D = SpeechBubbleManager.new()
	add_child_autofree(mgr)
	mgr._show_bubble("creature-1", "Hello!")
	var bubble: Node3D = mgr._active_bubbles["creature-1"]
	bubble._active = false
	mgr._reclaim_expired()
	# Now show a new bubble — should reuse the pooled node.
	mgr._show_bubble("creature-2", "Hi!")
	assert_eq(mgr._pool.size(), 0, "Pool should be empty after reuse")
	assert_eq(mgr._active_bubbles.size(), 1, "Should have one active bubble")
	assert_true(mgr._active_bubbles["creature-2"] == bubble, "Should reuse the same node from pool")


func test_multiple_creatures_get_separate_bubbles() -> void:
	var mgr: Node3D = SpeechBubbleManager.new()
	add_child_autofree(mgr)
	mgr._show_bubble("creature-1", "Hello!")
	mgr._show_bubble("creature-2", "Hi there!")
	assert_eq(mgr._active_bubbles.size(), 2, "Two creatures should have two bubbles")
	assert_true(mgr._active_bubbles.has("creature-1"), "First creature has a bubble")
	assert_true(mgr._active_bubbles.has("creature-2"), "Second creature has a bubble")
	assert_true(
		mgr._active_bubbles["creature-1"] != mgr._active_bubbles["creature-2"],
		"Each creature gets a distinct bubble node"
	)


func test_reclaim_does_not_pool_active_bubbles() -> void:
	var mgr: Node3D = SpeechBubbleManager.new()
	add_child_autofree(mgr)
	mgr._show_bubble("creature-1", "Hello!")
	# Bubble is active (not expired) — reclaim should leave it alone.
	mgr._reclaim_expired()
	assert_eq(mgr._active_bubbles.size(), 1, "Active bubble should not be reclaimed")
	assert_eq(mgr._pool.size(), 0, "Pool should be empty")
