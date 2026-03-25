#!/usr/bin/env bash
# Wait for GitHub Actions CI to complete on a specific commit.
#
# Usage: scripts/wait-for-ci.sh [COMMIT_SHA]
#
# If no SHA is provided, defaults to HEAD. Polls `gh run list --commit <SHA>`
# until a CI run appears and resolves. Exits 0 on success, 1 on failure.
#
# Early exit: stops polling as soon as any job fails (no point waiting for the
# rest) or when all non-coverage jobs have passed (coverage is slow and not
# worth blocking on).
#
# If the commit only touched files outside CI's path filters, detects this
# locally and exits 0 immediately. If the commit *should* trigger CI but no
# run appears within the timeout, exits with an error.

set -euo pipefail

COMMIT="${1:-$(git rev-parse HEAD)}"
APPEAR_POLL=30   # Slow poll while waiting for the run to appear.
JOB_POLL=10      # Fast poll once the run is in progress.
# How long to wait for a run to appear (only used when we expect CI to trigger).
APPEAR_TIMEOUT=180
# How long to wait for a run to finish after it appears.
RUN_TIMEOUT=600

# Jobs we don't need to wait for — they're slow and non-blocking.
SKIP_JOBS="coverage"

# --- Phase 0: Check if the commit should trigger CI at all. ---
# These patterns mirror the `paths:` filter in .github/workflows/ci.yml.
# If none of the changed files match, CI won't run.
CI_PATH_PATTERNS=(
    '\.rs$'
    '(^|/)Cargo\.toml$'
    '(^|/)Cargo\.lock$'
    '(^|/)rustfmt\.toml$'
    'elven_canopy_.*/Cargo\.toml$'
    '^tabulosity/'
    '^tabulosity_derive/'
    '^multiplayer_tests/'
    'godot/scripts/.*\.gd$'
    'godot/test/.*\.gd$'
    'godot/addons/gut/'
    'godot/\.gutconfig\.json$'
    '\.gdlintrc$'
    'python/requirements-dev\.txt$'
    '\.github/workflows/ci\.yml$'
)

# Ensure the commit is available locally (it may be on a remote branch we
# haven't fetched yet).
if ! git cat-file -e "$COMMIT" 2>/dev/null; then
    git fetch origin 2>/dev/null
fi
changed_files=$(git diff-tree --no-commit-id --name-only -r "$COMMIT")
triggers_ci=false
for file in $changed_files; do
    for pattern in "${CI_PATH_PATTERNS[@]}"; do
        if echo "$file" | grep -qE "$pattern"; then
            triggers_ci=true
            break 2
        fi
    done
done

if [ "$triggers_ci" = "false" ]; then
    echo "Commit ${COMMIT:0:12} only touched files outside CI path filters — no CI run expected."
    echo "CI_RESULT=skipped"
    exit 0
fi

echo "Waiting for CI on commit ${COMMIT:0:12}..."

# --- Phase 1: Wait for the run to appear. ---
elapsed=0
run_id=""
while [ -z "$run_id" ]; do
    run_id=$(gh run list --commit "$COMMIT" --json databaseId,workflowName \
        --jq '.[] | select(.workflowName == "CI") | .databaseId' 2>/dev/null | head -n1 || true)
    if [ -n "$run_id" ]; then
        break
    fi
    if [ "$elapsed" -ge "$APPEAR_TIMEOUT" ]; then
        echo "ERROR: Commit touches CI paths but no run appeared after ${APPEAR_TIMEOUT}s."
        echo "CI_RESULT=error"
        exit 1
    fi
    sleep "$APPEAR_POLL"
    elapsed=$((elapsed + APPEAR_POLL))
done

echo "Found CI run $run_id. Polling jobs..."

# --- Phase 2: Poll jobs until resolved. ---
elapsed=0
while true; do
    # Fetch all jobs as tab-separated: name\tstatus\tconclusion
    jobs_tsv=$(gh run view "$run_id" --json jobs \
        --jq '.jobs[] | [.name, .status, .conclusion] | @tsv' || true)
    # If gh failed (network blip, rate limit), skip this iteration and retry.
    if [ -z "$jobs_tsv" ]; then
        echo "  (gh query failed, retrying...)"
        sleep "$JOB_POLL"
        elapsed=$((elapsed + JOB_POLL))
        continue
    fi

    # Count totals.
    total=0
    completed=0
    failed=0
    failed_names=""
    skippable_pending=0
    non_skip_passed=0
    non_skip_total=0

    while IFS=$'\t' read -r name status conclusion; do
        [ -z "$name" ] && continue
        total=$((total + 1))

        is_skippable=false
        for skip in $SKIP_JOBS; do
            if [ "$name" = "$skip" ]; then
                is_skippable=true
                break
            fi
        done

        if [ "$status" = "completed" ]; then
            completed=$((completed + 1))
            if [ "$conclusion" != "success" ] && [ "$conclusion" != "skipped" ]; then
                failed=$((failed + 1))
                failed_names="$failed_names $name"
            elif [ "$conclusion" = "success" ] && [ "$is_skippable" = "false" ]; then
                non_skip_passed=$((non_skip_passed + 1))
            fi
        else
            if [ "$is_skippable" = "true" ]; then
                skippable_pending=$((skippable_pending + 1))
            fi
        fi

        if [ "$is_skippable" = "false" ]; then
            non_skip_total=$((non_skip_total + 1))
        fi
    done <<< "$jobs_tsv"

    # Early exit: any failure.
    if [ "$failed" -gt 0 ]; then
        echo ""
        echo "CI FAILED — $failed job(s) failed:$failed_names"
        echo ""
        echo "$jobs_tsv"
        echo ""
        echo "CI_RESULT=failure"
        echo "CI_RUN_ID=$run_id"
        exit 1
    fi

    # Early success: all non-skippable jobs passed (skippable ones still running).
    if [ "$non_skip_passed" -eq "$non_skip_total" ] && [ "$non_skip_total" -gt 0 ]; then
        if [ "$skippable_pending" -gt 0 ]; then
            echo "All required jobs passed ($non_skip_passed/$non_skip_total). Skipping wait for: $SKIP_JOBS"
        else
            echo "All $total jobs passed."
        fi
        echo ""
        echo "CI_RESULT=success"
        echo "CI_RUN_ID=$run_id"
        exit 0
    fi

    # All completed, none failed — success.
    if [ "$completed" -eq "$total" ] && [ "$total" -gt 0 ]; then
        echo "All $total jobs completed."
        echo ""
        echo "CI_RESULT=success"
        echo "CI_RUN_ID=$run_id"
        exit 0
    fi

    # Timeout check.
    if [ "$elapsed" -ge "$RUN_TIMEOUT" ]; then
        echo "CI run $run_id still in progress after ${RUN_TIMEOUT}s — timed out."
        echo ""
        echo "$jobs_tsv"
        echo ""
        echo "CI_RESULT=timeout"
        echo "CI_RUN_ID=$run_id"
        exit 1
    fi

    # Status line.
    echo "  $completed/$total jobs done (${elapsed}s elapsed)..."
    sleep "$JOB_POLL"
    elapsed=$((elapsed + JOB_POLL))
done
