## Unit tests for the speech bubble manager's temporary thought-to-speech
## text mapping.
##
## TODO(F-llm-convo-ui): TEMPORARY — these tests cover the placeholder
## thought-to-speech mapping. Remove when LLM dialogue replaces it.
##
## See also: speech_bubble_manager.gd for the implementation.
extends GutTest

const SpeechBubbleManager = preload("res://scripts/speech_bubble_manager.gd")


func test_pleasant_chat_produces_speech() -> void:
	var result := SpeechBubbleManager._thought_to_speech_text("Had a pleasant chat with Aelindra")
	assert_ne(result, "", "Pleasant chat should produce speech text")


func test_awkward_chat_produces_speech() -> void:
	var result := SpeechBubbleManager._thought_to_speech_text(
		"Had an awkward exchange with Thaeron"
	)
	assert_ne(result, "", "Awkward chat should produce speech text")


func test_dinner_with_produces_speech() -> void:
	var result := SpeechBubbleManager._thought_to_speech_text("Enjoyed dinner with Aelindra")
	assert_ne(result, "", "Dinner with someone should produce speech text")


func test_awkward_dinner_produces_speech() -> void:
	var result := SpeechBubbleManager._thought_to_speech_text("Awkward dinner with Thaeron")
	assert_ne(result, "", "Awkward dinner should produce speech text")


func test_dance_with_produces_speech() -> void:
	var result := SpeechBubbleManager._thought_to_speech_text("Enjoyed dancing with Aelindra")
	assert_ne(result, "", "Dance with someone should produce speech text")


func test_awkward_dance_produces_speech() -> void:
	var result := SpeechBubbleManager._thought_to_speech_text("Awkward dance with Thaeron")
	assert_ne(result, "", "Awkward dance should produce speech text")


func test_danced_with_friend_produces_speech() -> void:
	var result := SpeechBubbleManager._thought_to_speech_text("Danced with a friend")
	assert_ne(result, "", "Danced with friend should produce speech text")


func test_dinner_party_produces_speech() -> void:
	var result := SpeechBubbleManager._thought_to_speech_text("Enjoyed a dinner party")
	assert_ne(result, "", "Dinner party should produce speech text")


func test_internal_thought_returns_empty() -> void:
	# Internal thoughts should NOT produce speech.
	assert_eq(
		SpeechBubbleManager._thought_to_speech_text("Slept in own home"),
		"",
		"Internal thought should not produce speech"
	)
	assert_eq(
		SpeechBubbleManager._thought_to_speech_text("Ate in a dining hall"),
		"",
		"Internal thought should not produce speech"
	)
	assert_eq(
		SpeechBubbleManager._thought_to_speech_text("Slept on the ground"),
		"",
		"Internal thought should not produce speech"
	)


func test_unknown_thought_returns_empty() -> void:
	assert_eq(
		SpeechBubbleManager._thought_to_speech_text("Some unknown thought"),
		"",
		"Unknown thought should not produce speech"
	)


func test_empty_string_returns_empty() -> void:
	assert_eq(
		SpeechBubbleManager._thought_to_speech_text(""),
		"",
		"Empty description should not produce speech"
	)
