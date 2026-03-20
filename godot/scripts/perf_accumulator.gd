## Lightweight per-frame timing accumulator for profiling GDScript overhead.
##
## Collects microsecond samples for named metrics and prints a percentile
## summary (p50/p90/p99/max) to stdout on demand. Designed to be instantiated
## once in main.gd and passed to renderers that want to report their own
## timing.
##
## Usage:
##   var t := perf.start()       # capture timestamp
##   ... work ...
##   perf.record("metric_name", t)  # record elapsed microseconds
##
## On shutdown, call print_summary() to dump all metrics.
##
## See also: mesh_cache.rs PerfStats for the Rust-side equivalent,
## main.gd which creates the instance and instruments _process(),
## creature renderers which record their own _process() timing.
## Map from metric name (String) → PackedInt32Array of microsecond samples.
var _samples: Dictionary = {}


## Capture the current timestamp. Returns the value to pass to record().
func start() -> int:
	return Time.get_ticks_usec()


## Record elapsed microseconds since the timestamp returned by start().
func record(metric: String, start_usec: int) -> void:
	var elapsed := Time.get_ticks_usec() - start_usec
	if not _samples.has(metric):
		_samples[metric] = PackedInt32Array()
	_samples[metric].append(elapsed)


## Print percentile summary for all metrics to stdout.
func print_summary() -> void:
	print("=== GDScript Perf Stats ===")
	# Sort metric names for stable output.
	var names: Array = _samples.keys()
	names.sort()
	for metric_name in names:
		var arr: PackedInt32Array = _samples[metric_name]
		_print_metric(metric_name, arr)


func _print_metric(metric_name: String, samples: PackedInt32Array) -> void:
	var n := samples.size()
	if n == 0:
		print("  %s: (no samples)" % metric_name)
		return
	# Sort a copy for percentile calculation.
	var sorted := Array(samples)
	sorted.sort()
	var p50: int = sorted[n / 2]
	var p90: int = sorted[n * 90 / 100]
	var p99: int = sorted[n * 99 / 100]
	var mx: int = sorted[n - 1]
	var total: int = 0
	for v in sorted:
		total += v
	@warning_ignore("integer_division")
	var mean: int = total / n
	print(
		(
			"  %s: n=%d  mean=%dus  p50=%dus  p90=%dus  p99=%dus  max=%dus"
			% [metric_name, n, mean, p50, p90, p99, mx]
		)
	)
