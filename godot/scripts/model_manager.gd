## Autoload singleton managing LLM model download and availability.
##
## Handles background download of the LLM model file (Qwen 3 1.7B Q5_K_M GGUF)
## from HuggingFace, SHA-256 verification, and storage in user://models/.
## The model is optional — the game works without it (creatures use purely
## rules-based behavior). Players opt in via the Settings panel.
##
## States:
##   NOT_DOWNLOADED — no model file on disk
##   DOWNLOADING    — HTTPRequest in progress, progress_fraction updated
##   VERIFYING      — download complete, computing SHA-256
##   READY          — model file exists and is verified
##   FAILED         — download or verification failed (error_message set)
##
## Emits `state_changed` whenever the state transitions. UI code (settings
## panel, main menu) connects to this signal to update display.
##
## See also: settings_panel.gd (UI integration), game_config.gd (llm_enabled
## setting).

extends Node

signal state_changed

enum State {
	NOT_DOWNLOADED,
	DOWNLOADING,
	VERIFYING,
	READY,
	FAILED,
}

## Model manifest — all the info needed to download and verify the model.
const _MODEL_URL_BASE := "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF"
const MODEL_URL := _MODEL_URL_BASE + "/resolve/main/Qwen3-1.7B-Q5_K_M.gguf"
const MODEL_FILENAME := "Qwen3-1.7B-Q5_K_M.gguf"
const MODEL_EXPECTED_SIZE := 1257880128  ## bytes (~1.17 GB)
## SHA-256 of the GGUF file. Empty string skips verification (set this once
## we've confirmed the hash from an actual download).
const MODEL_SHA256 := ""
const MODELS_DIR := "user://models"

var state: State = State.NOT_DOWNLOADED
var error_message: String = ""
## Fraction of download complete (0.0–1.0). Only meaningful in DOWNLOADING state.
var progress_fraction: float = 0.0
## Bytes downloaded so far. Only meaningful in DOWNLOADING state.
var downloaded_bytes: int = 0

var _http_request: HTTPRequest = null


func _ready() -> void:
	_check_existing_model()


## Returns the absolute filesystem path to the model file, or empty string if
## not downloaded. Use this to pass the path to the LLM inference engine.
func get_model_path() -> String:
	if state != State.READY:
		return ""
	return ProjectSettings.globalize_path(MODELS_DIR.path_join(MODEL_FILENAME))


## Start downloading the model. No-op if already downloading or ready.
func start_download() -> void:
	if state == State.DOWNLOADING or state == State.READY:
		return

	DirAccess.make_dir_recursive_absolute(MODELS_DIR)

	# Clean up any partial download.
	var partial_path := MODELS_DIR.path_join(MODEL_FILENAME + ".part")
	if FileAccess.file_exists(partial_path):
		DirAccess.remove_absolute(partial_path)

	_http_request = HTTPRequest.new()
	# Download directly to a .part file to avoid leaving a corrupt model file
	# if the download is interrupted.
	_http_request.download_file = ProjectSettings.globalize_path(partial_path)
	# 64 KB download buffer — large enough for throughput, small enough for
	# responsive progress updates.
	_http_request.download_chunk_size = 65536
	_http_request.use_threads = true
	add_child(_http_request)
	_http_request.request_completed.connect(_on_download_completed)

	var err := _http_request.request(MODEL_URL)
	if err != OK:
		error_message = "HTTP request failed to start (error %d)" % err
		_set_state(State.FAILED)
		_cleanup_http()
		return

	downloaded_bytes = 0
	progress_fraction = 0.0
	error_message = ""
	_set_state(State.DOWNLOADING)


## Cancel an in-progress download.
func cancel_download() -> void:
	if state != State.DOWNLOADING:
		return
	_cleanup_http()

	# Remove partial file.
	var partial_path := MODELS_DIR.path_join(MODEL_FILENAME + ".part")
	if FileAccess.file_exists(partial_path):
		DirAccess.remove_absolute(partial_path)

	_set_state(State.NOT_DOWNLOADED)


## Delete the downloaded model file (and any partial download).
func delete_model() -> void:
	if state == State.DOWNLOADING:
		cancel_download()

	var model_path := MODELS_DIR.path_join(MODEL_FILENAME)
	if FileAccess.file_exists(model_path):
		DirAccess.remove_absolute(model_path)

	# Also clean up .part file (e.g., if called during VERIFYING state).
	_remove_partial()

	_set_state(State.NOT_DOWNLOADED)


func _process(_delta: float) -> void:
	if state != State.DOWNLOADING or _http_request == null:
		return

	# Update progress from the HTTPRequest's body size.
	var body_size := _http_request.get_body_size()
	var downloaded := _http_request.get_downloaded_bytes()
	downloaded_bytes = downloaded
	if body_size > 0:
		progress_fraction = float(downloaded) / float(body_size)
	elif MODEL_EXPECTED_SIZE > 0:
		# Server didn't send Content-Length; estimate from manifest.
		progress_fraction = float(downloaded) / float(MODEL_EXPECTED_SIZE)


func _on_download_completed(
	result: int, response_code: int, _headers: PackedStringArray, _body: PackedByteArray
) -> void:
	_cleanup_http()

	if result != HTTPRequest.RESULT_SUCCESS:
		error_message = "Download failed (result %d)" % result
		_set_state(State.FAILED)
		_remove_partial()
		return

	if response_code < 200 or response_code >= 300:
		error_message = "Download failed (HTTP %d)" % response_code
		_set_state(State.FAILED)
		_remove_partial()
		return

	var partial_path := MODELS_DIR.path_join(MODEL_FILENAME + ".part")
	var final_path := MODELS_DIR.path_join(MODEL_FILENAME)

	if not FileAccess.file_exists(partial_path):
		error_message = "Download completed but file not found on disk"
		_set_state(State.FAILED)
		return

	# Verify file size.
	var file := FileAccess.open(partial_path, FileAccess.READ)
	if file == null:
		error_message = "Cannot open downloaded file for verification"
		_set_state(State.FAILED)
		_remove_partial()
		return
	var file_size := file.get_length()
	file.close()

	if MODEL_EXPECTED_SIZE > 0 and file_size != MODEL_EXPECTED_SIZE:
		error_message = (
			"Size mismatch: expected %d bytes, got %d" % [MODEL_EXPECTED_SIZE, file_size]
		)
		_set_state(State.FAILED)
		_remove_partial()
		return

	# SHA-256 verification (if hash is configured).
	if not MODEL_SHA256.is_empty():
		_set_state(State.VERIFYING)
		# Synchronous hashing — blocks the main thread for a few seconds on
		# a ~1.2 GB file. If this causes frame hitches, move to a thread.
		var computed_hash := _compute_sha256(partial_path)
		if computed_hash != MODEL_SHA256:
			error_message = "SHA-256 mismatch: expected %s, got %s" % [MODEL_SHA256, computed_hash]
			_set_state(State.FAILED)
			_remove_partial()
			return

	# Rename .part → final filename.
	var rename_err := DirAccess.rename_absolute(partial_path, final_path)
	if rename_err != OK:
		error_message = "Failed to rename downloaded file (error %d)" % rename_err
		_set_state(State.FAILED)
		return

	progress_fraction = 1.0
	_set_state(State.READY)


## Check if the model file already exists on disk at startup.
func _check_existing_model() -> void:
	var model_path := MODELS_DIR.path_join(MODEL_FILENAME)
	if not FileAccess.file_exists(model_path):
		_set_state(State.NOT_DOWNLOADED)
		return

	if MODEL_EXPECTED_SIZE <= 0:
		_set_state(State.READY)
		return

	var file := FileAccess.open(model_path, FileAccess.READ)
	if file == null:
		_set_state(State.NOT_DOWNLOADED)
		return

	var file_size := file.get_length()
	file.close()
	if file_size == MODEL_EXPECTED_SIZE:
		_set_state(State.READY)
		return

	# Wrong size — probably a partial/corrupt download. Remove it.
	DirAccess.remove_absolute(model_path)
	_set_state(State.NOT_DOWNLOADED)


func _set_state(new_state: State) -> void:
	if state == new_state:
		return
	state = new_state
	state_changed.emit()


func _cleanup_http() -> void:
	if _http_request != null:
		_http_request.cancel_request()
		_http_request.queue_free()
		_http_request = null


func _remove_partial() -> void:
	var partial_path := MODELS_DIR.path_join(MODEL_FILENAME + ".part")
	if FileAccess.file_exists(partial_path):
		DirAccess.remove_absolute(partial_path)


## Compute SHA-256 of a file. Returns lowercase hex string.
static func _compute_sha256(path: String) -> String:
	var ctx := HashingContext.new()
	ctx.start(HashingContext.HASH_SHA256)

	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		return ""

	# Read in 1 MB chunks to avoid loading the entire file into memory.
	var chunk_size := 1048576
	var file_length := file.get_length()
	while file.get_position() < file_length:
		var remaining := file_length - file.get_position()
		var read_size := mini(chunk_size, remaining)
		var chunk := file.get_buffer(read_size)
		ctx.update(chunk)

	file.close()
	return ctx.finish().hex_encode()


## Format a byte count as a human-readable string (e.g., "1.17 GB").
static func format_bytes(bytes: int) -> String:
	if bytes < 1024:
		return "%d B" % bytes
	if bytes < 1048576:
		return "%.1f KB" % (float(bytes) / 1024.0)
	if bytes < 1073741824:
		return "%.1f MB" % (float(bytes) / 1048576.0)
	return "%.2f GB" % (float(bytes) / 1073741824.0)
