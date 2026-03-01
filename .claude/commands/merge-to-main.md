# Merge to Main

Merge the current feature branch to main using the squash-rebase-ff workflow.
This ensures a clean single-commit history on main with proper conflict
detection.

## Prerequisites

- You must be on a feature branch (not main).
- All work should be committed and pushed.
- The once-over should already be done (run `/once-over` first if not).

## Procedure

### Step 1: Squash into a single commit

Create a temporary local rebase branch and squash all feature commits:

```
git checkout -b feature/BRANCH-rebase feature/BRANCH
git merge-base main feature/BRANCH-rebase
git reset --soft <COMMON-ANCESTOR>
git commit -m "Descriptive commit message summarizing the feature"
```

The commit message should summarize the entire feature — do not repeat
individual commit messages. Include the tracker ID if applicable, e.g.,
"Add mid-game join with state snapshot (F-mp-mid-join)".

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

**If conflicts arise**, delegate resolution to a general-purpose agent. The
agent should:

1. Run `git status` to identify conflicting files.
2. Read each conflicting file and find the `<<<<<<<` / `=======` / `>>>>>>>`
   markers.
3. Understand the intent of both sides (our feature vs. what landed on main)
   by reading surrounding code and recent main commits if needed.
4. Resolve each conflict, preserving the intent of both sides where possible.
5. Stage resolved files and run `git rebase --continue`.

After conflict resolution (whether by agent or directly):
- Run `scripts/build.sh test` to verify correctness.
- If conflicts required **non-trivial edits** (integrating two features that
  touch the same code), ask the user for permission before continuing. Trivial
  conflicts (adjacent added lines, no semantic interaction) can proceed without
  asking.

### Step 4: Update tracker

If the branch implements a tracked feature or bug:

1. In the summary section: change `[~]` to `[x]` and move the line from
   In Progress to Done (maintain alphabetical order by ID).
2. In the detailed entry: change `**Status:** In Progress` to
   `**Status:** Done`.
3. Run `python3 scripts/fix_tracker.py` to enforce ordering and clean up
   blocking references.
4. Amend the squashed commit to include the tracker update:
   ```
   git add docs/tracker.md
   git commit --amend --no-edit
   ```

If the branch is not a tracked item (e.g., tooling, CLAUDE.md changes), skip
this step.

### Step 5: Fast-forward merge (requires permission)

**Ask the user for permission before this step.**

```
git checkout main
git merge --ff-only feature/BRANCH-rebase
```

### Step 6: Push and clean up

```
git push
git branch -d feature/BRANCH-rebase
git branch -D feature/BRANCH
git push origin --delete feature/BRANCH
```

## Why squash first, then rebase?

Rebasing a multi-commit branch onto main can require resolving the same
conflict repeatedly (once per commit). By squashing into one commit first, you
only resolve conflicts once. The `git reset --soft` in step 1 is safe — it
collapses our own feature commits back to the branch point, without touching
main's state.
