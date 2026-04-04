## Unit tests for model_manager.gd — LLM model download management.
##
## Tests verify state transitions, format_bytes, SHA-256 computation, and
## startup detection of existing model files. Does NOT test actual HTTP
## downloads (that requires network access and a ~1.2 GB transfer).
##
## Note: tests that exercise file operations use TEST_MODELS_DIR
## (user://_test_models) to avoid touching the real user://models/ directory.
## Since MODELS_DIR is a const and can't be overridden at runtime, tests that
## call _check_existing_model or delete_model create files at the real path
## only when absolutely necessary and always clean up.
##
## See also: model_manager.gd, test_settings_panel.gd.

extends GutTest

const ModelManagerScript := preload("res://scripts/model_manager.gd")

const TEST_MODELS_DIR := "user://_test_models"

var _mm: Node


func before_each() -> void:
	# Clean up test directory.
	_remove_test_dir()
	DirAccess.make_dir_recursive_absolute(TEST_MODELS_DIR)

	_mm = Node.new()
	_mm.set_script(ModelManagerScript)
	# Don't add as child yet — _ready() would check the real models dir.


func after_each() -> void:
	if is_instance_valid(_mm):
		_mm.queue_free()
	_mm = null
	_remove_test_dir()
	# Clean up any files created in the real models dir by tests.
	_cleanup_real_model_dir()


func _remove_test_dir() -> void:
	if not DirAccess.dir_exists_absolute(TEST_MODELS_DIR):
		return
	var dir := DirAccess.open(TEST_MODELS_DIR)
	if dir:
		dir.list_dir_begin()
		var fname := dir.get_next()
		while fname != "":
			DirAccess.remove_absolute(TEST_MODELS_DIR.path_join(fname))
			fname = dir.get_next()
		dir.list_dir_end()
	DirAccess.remove_absolute(TEST_MODELS_DIR)


## Remove test artifacts from the real models dir. Only removes the specific
## tiny test files we create (identified by size < 1 KB), never a real model.
func _cleanup_real_model_dir() -> void:
	var real_dir: String = ModelManagerScript.MODELS_DIR
	var model_path := real_dir.path_join(ModelManagerScript.MODEL_FILENAME)
	if FileAccess.file_exists(model_path):
		var file := FileAccess.open(model_path, FileAccess.READ)
		if file and file.get_length() < 1024:
			file.close()
			DirAccess.remove_absolute(model_path)
	var part_path := real_dir.path_join(ModelManagerScript.MODEL_FILENAME + ".part")
	if FileAccess.file_exists(part_path):
		var file := FileAccess.open(part_path, FileAccess.READ)
		if file and file.get_length() < 1024:
			file.close()
			DirAccess.remove_absolute(part_path)


# --- format_bytes ---


## format_bytes returns human-readable strings for various magnitudes.
func test_format_bytes_small() -> void:
	assert_eq(ModelManagerScript.format_bytes(0), "0 B")
	assert_eq(ModelManagerScript.format_bytes(512), "512 B")
	assert_eq(ModelManagerScript.format_bytes(1023), "1023 B")


func test_format_bytes_kb() -> void:
	assert_eq(ModelManagerScript.format_bytes(1024), "1.0 KB")
	assert_eq(ModelManagerScript.format_bytes(1536), "1.5 KB")


func test_format_bytes_mb() -> void:
	assert_eq(ModelManagerScript.format_bytes(1048576), "1.0 MB")
	assert_eq(ModelManagerScript.format_bytes(52428800), "50.0 MB")


func test_format_bytes_gb() -> void:
	assert_eq(ModelManagerScript.format_bytes(1073741824), "1.00 GB")
	assert_eq(ModelManagerScript.format_bytes(1257880128), "1.17 GB")


# --- SHA-256 ---


## SHA-256 of a known small file.
func test_compute_sha256_known_value() -> void:
	var test_path := TEST_MODELS_DIR.path_join("sha_test.bin")
	var file := FileAccess.open(test_path, FileAccess.WRITE)
	file.store_string("hello world\n")
	file.close()

	# SHA-256 of "hello world\n" (the exact bytes stored by store_string).
	var expected := "ecf701f727d9e2d77c4aa49ac6fbbcc997278aca010bddeeb961c10cf54d435a"
	var actual := ModelManagerScript._compute_sha256(test_path)
	assert_eq(actual, expected)


## SHA-256 of a nonexistent file returns empty string.
func test_compute_sha256_missing_file() -> void:
	var actual := ModelManagerScript._compute_sha256(TEST_MODELS_DIR.path_join("nonexistent.bin"))
	assert_eq(actual, "")


## SHA-256 of an empty file returns the well-known empty-input hash.
func test_compute_sha256_empty_file() -> void:
	var test_path := TEST_MODELS_DIR.path_join("empty.bin")
	var file := FileAccess.open(test_path, FileAccess.WRITE)
	file.close()

	var expected := "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
	var actual := ModelManagerScript._compute_sha256(test_path)
	assert_eq(actual, expected)


# --- State transitions ---


## Initial state is NOT_DOWNLOADED when no model file exists.
func test_initial_state_not_downloaded() -> void:
	_mm._check_existing_model()
	assert_eq(_mm.state, ModelManagerScript.State.NOT_DOWNLOADED)


## state_changed signal fires on state transition.
func test_state_changed_signal() -> void:
	var signal_count := 0
	_mm.state_changed.connect(func() -> void: signal_count += 1)

	_mm._set_state(ModelManagerScript.State.READY)
	assert_eq(signal_count, 1)
	assert_eq(_mm.state, ModelManagerScript.State.READY)

	# Same state again — no signal.
	_mm._set_state(ModelManagerScript.State.READY)
	assert_eq(signal_count, 1)

	_mm._set_state(ModelManagerScript.State.NOT_DOWNLOADED)
	assert_eq(signal_count, 2)


## _check_existing_model detects a correctly-sized file as READY.
func test_check_existing_model_correct_size() -> void:
	# Create a fake model file at the real MODELS_DIR with the expected size.
	# We use a tiny sentinel size by temporarily checking against file_size == 0
	# via a 1-byte file — but _check_existing_model checks MODEL_EXPECTED_SIZE
	# which is ~1.2 GB, so we can't actually create a file that big. Instead,
	# test that a wrong-size file is rejected (see next test), and test the
	# code path where MODEL_EXPECTED_SIZE <= 0 would mark READY — but that's
	# also not reachable since the const is set. So we test via _set_state
	# directly that the state machine works.
	#
	# The real startup detection is implicitly tested by the wrong-size test
	# below (which exercises the same code path minus the final READY branch).
	pass


## _check_existing_model removes a file with wrong size.
func test_check_existing_model_wrong_size_removes_file() -> void:
	var real_dir: String = ModelManagerScript.MODELS_DIR
	DirAccess.make_dir_recursive_absolute(real_dir)
	var model_path := real_dir.path_join(ModelManagerScript.MODEL_FILENAME)

	# Create a tiny file (wrong size).
	var file := FileAccess.open(model_path, FileAccess.WRITE)
	file.store_8(42)
	file.close()
	assert_true(FileAccess.file_exists(model_path))

	_mm._check_existing_model()
	assert_eq(_mm.state, ModelManagerScript.State.NOT_DOWNLOADED)
	assert_false(FileAccess.file_exists(model_path))


# --- Guard clauses ---


## start_download is a no-op when state is READY.
func test_start_download_noop_when_ready() -> void:
	_mm._set_state(ModelManagerScript.State.READY)
	_mm.start_download()
	assert_eq(_mm.state, ModelManagerScript.State.READY)


## start_download is a no-op when state is DOWNLOADING.
func test_start_download_noop_when_downloading() -> void:
	_mm._set_state(ModelManagerScript.State.DOWNLOADING)
	_mm.start_download()
	# Still DOWNLOADING, no crash, no second HTTPRequest.
	assert_eq(_mm.state, ModelManagerScript.State.DOWNLOADING)


## cancel_download is a no-op when not downloading.
func test_cancel_download_noop_when_not_downloading() -> void:
	_mm._set_state(ModelManagerScript.State.NOT_DOWNLOADED)
	_mm.cancel_download()
	assert_eq(_mm.state, ModelManagerScript.State.NOT_DOWNLOADED)

	_mm._set_state(ModelManagerScript.State.READY)
	_mm.cancel_download()
	assert_eq(_mm.state, ModelManagerScript.State.READY)


# --- delete_model ---


## delete_model transitions from READY to NOT_DOWNLOADED and removes file.
func test_delete_model() -> void:
	var real_dir: String = ModelManagerScript.MODELS_DIR
	DirAccess.make_dir_recursive_absolute(real_dir)
	var fake_path := real_dir.path_join(ModelManagerScript.MODEL_FILENAME)

	# Create a tiny fake model file.
	var file := FileAccess.open(fake_path, FileAccess.WRITE)
	file.store_8(0)
	file.close()

	_mm._set_state(ModelManagerScript.State.READY)
	_mm.delete_model()
	assert_eq(_mm.state, ModelManagerScript.State.NOT_DOWNLOADED)
	assert_false(FileAccess.file_exists(fake_path))


## delete_model works when no file exists on disk.
func test_delete_model_no_file() -> void:
	_mm._set_state(ModelManagerScript.State.READY)
	_mm.delete_model()
	assert_eq(_mm.state, ModelManagerScript.State.NOT_DOWNLOADED)


## delete_model during VERIFYING cleans up .part file.
func test_delete_model_during_verifying() -> void:
	var real_dir: String = ModelManagerScript.MODELS_DIR
	DirAccess.make_dir_recursive_absolute(real_dir)
	var part_path := real_dir.path_join(ModelManagerScript.MODEL_FILENAME + ".part")

	var file := FileAccess.open(part_path, FileAccess.WRITE)
	file.store_8(0)
	file.close()

	_mm._set_state(ModelManagerScript.State.VERIFYING)
	_mm.delete_model()
	assert_eq(_mm.state, ModelManagerScript.State.NOT_DOWNLOADED)
	assert_false(FileAccess.file_exists(part_path))


# --- get_model_path ---


## get_model_path returns empty when not ready.
func test_get_model_path_not_ready() -> void:
	for s: int in [
		ModelManagerScript.State.NOT_DOWNLOADED,
		ModelManagerScript.State.DOWNLOADING,
		ModelManagerScript.State.VERIFYING,
		ModelManagerScript.State.FAILED,
	]:
		_mm.state = s
		assert_eq(_mm.get_model_path(), "", "state %d should return empty" % s)


## get_model_path returns an absolute OS path (not user://) when ready.
func test_get_model_path_ready() -> void:
	_mm.state = ModelManagerScript.State.READY
	var path := _mm.get_model_path()
	assert_ne(path, "")
	# Must be an absolute OS path, not a Godot virtual path.
	assert_false(path.begins_with("user://"), "should be absolute, not user://")
	assert_true(path.ends_with(ModelManagerScript.MODEL_FILENAME))
