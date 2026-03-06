# Merge to Main

Merge the current feature branch to main using the squash-rebase-ff workflow.
This ensures a clean single-commit history on main with proper conflict
detection.

## Prerequisites

- You must be on a feature branch (not main).
- All work should be committed and pushed.
- Consider running `/once-over` first if the changes are complex and the user
  hasn't already asked for one, or if a complicated conflicted rebase just
  happened.

## Procedure

**Delegate the entire merge to a general-purpose agent.** The agent performs all
steps below, returning a summary of what happened (commit hash, any conflicts
resolved, tracker updates made). This keeps the main context window clean.

The agent should follow these steps:

### Step 0: Identify the branch

Run `git branch --show-current` to determine the feature branch name. All
subsequent steps refer to this as `feature/BRANCH`.

### Step 1: Squash into a single commit

Create a temporary local rebase branch and squash all feature commits:

```
git checkout -b feature/BRANCH-rebase feature/BRANCH
git merge-base main feature/BRANCH-rebase
git reset --soft <COMMON-ANCESTOR>
git commit -m "Descriptive commit message summarizing the feature"
```

The commit message should summarize the entire feature — do not repeat
individual commit messages. **Make it substantial** — similar in detail and
scope to the existing commit messages on main. For example:

```
"Add mid-game join with state snapshot (F-mp-mid-join)

Implement session handshake that sends compressed world state ...
refactor LocalRelay to support mid-stream client insertion ..."
```

Include the tracker ID if applicable.

**Do NOT push the -rebase branch to origin.** It is local only.

### Step 2: Pull latest main

```
git checkout main && git pull
```

### Step 3: Rebase onto main

```
git checkout feature/BRANCH-rebase
git rebase main
```

**If the rebase succeeds cleanly**, continue to step 4.

**If conflicts arise:**

1. Run `git status` to identify conflicting files.
2. Read each conflicting file and find the `<<<<<<<` / `=======` / `>>>>>>>`
   markers.
3. Understand the intent of both sides (our feature vs. what landed on main)
   by reading surrounding code and recent main commits if needed.
4. Resolve each conflict, preserving the intent of both sides where possible.
5. Stage resolved files and run `git rebase --continue`.

After conflict resolution:
- Run `scripts/build.sh quicktest` to verify correctness.
- If the conflicts were trivial (ordering, adjacent lines, no semantic
  interaction) and quicktest passes, proceed without asking.
- If the conflicts required non-trivial edits (integrating two features that
  touch the same code), **stop and report back to the outer context** for user
  approval before continuing — tests may not catch all semantic issues.
- If quicktest fails, diagnose and fix, then re-run. If you cannot resolve the
  failures, **stop and report back** for help.
- If anything about the resolution feels wrong or surprising or even just
  suspicious — unexpected interactions, code that doesn't quite make sense,
  unclear intent — investigate thoroughly before proceeding. **Stop and report
  back** if unsure.

### Step 4: Update tracker

If the branch implements a tracked feature or bug:

1. Run `python3 scripts/tracker.py change-state <ID> done` (this updates
   both the summary and detail sections, sorts, and cleans up blocking
   references automatically).
2. Amend the squashed commit to include the tracker update:
   ```
   git add docs/tracker.md
   git commit --amend --no-edit
   ```

If the branch is not a tracked item (e.g., tooling, CLAUDE.md changes), skip
this step.

### Step 5: Fast-forward merge and push (with retry loop)

```
git checkout main
git merge --ff-only feature/BRANCH-rebase
git push
```

**If `git push` fails** (e.g., because another merge landed on main while we
were working), undo the merge commit and go back to step 2:

```
git reset --hard HEAD~1
```

Then repeat from step 2 (pull main, rebase, resolve conflicts, update tracker,
merge, push). Step 4 will be a no-op on retries since the tracker update is
already in the squashed commit. If this fails 5 times, **stop and report back**
— something unusual is happening.

### Step 6: Clean up

```
git branch -d feature/BRANCH-rebase
git branch -D feature/BRANCH
git push origin --delete feature/BRANCH
```

### Step 7: Report back

Return a concise summary to the outer context:
- Final commit hash and message on main.
- Whether conflicts were encountered and how they were resolved.
- Whether the tracker was updated.
- Any issues or concerns.

## After the agent returns

Review the agent's summary. If the agent stopped for user approval (non-trivial
conflicts, test failures), address the issue and re-run or continue manually.

## Why squash first, then rebase?

Rebasing a multi-commit branch onto main can require resolving the same
conflict repeatedly (once per commit). By squashing into one commit first, you
only resolve conflicts once. The `git reset --soft` in step 1 is safe — it
collapses our own feature commits back to the branch point, without touching
main's state.
