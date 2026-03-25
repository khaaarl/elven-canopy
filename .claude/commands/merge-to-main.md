# Merge to Main

Merge the current feature branch to main using the squash-rebase-ff workflow.
This ensures a clean single-commit history on main with proper conflict
detection and verification.

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
subsequent steps refer to this as `BRANCH`.

### Step 1: Rebase onto main

Follow the procedure in `.claude/commands/rebase-onto-main.md`, executing its
steps directly (do not spawn a sub-agent — you are already a delegated agent).
When the rebase procedure completes, review its report.

- If the rebase procedure reports "Clean rebase, no concerns" or "Clean rebase
  with minor adaptations" — proceed to step 2.
- If the rebase procedure reports "Rebase completed but needs user review" —
  **stop and report back to the outer context** with the rebase procedure's
  concerns. The user must approve before continuing.
- If the rebase procedure reports "Rebase aborted" — **stop and report back**
  with the failure details.

### Step 2: Craft the final commit message

The rebase left a mechanical squash commit message (concatenated originals).
Now replace it with a proper summary for main's history.

The commit message should summarize the entire feature — do not repeat
individual commit messages. **Make it substantial** — similar in detail and
scope to the existing commit messages on main. For example:

```
Add mid-game join with state snapshot (F-mp-mid-join)

Implement session handshake that sends compressed world state ...
refactor LocalRelay to support mid-stream client insertion ...
```

Include the tracker ID if applicable. Write the message to
`.tmp/commit-msg.txt` using the Write tool, then amend:

```
git commit --amend -F .tmp/commit-msg.txt
rm .tmp/commit-msg.txt
```

### Step 3: Update tracker

If the branch implements a tracked feature or bug:

1. Assess whether the work is actually complete by reviewing the commit
   messages, the branch's changes, and the tracker item's description. If the
   branch only partially implements the tracked item, use `progress` instead
   of `done`. However, if the tracker item is already marked `done`, do not
   move it backwards.
2. Run `python3 scripts/tracker.py change-state <ID> done` (or `progress`).
3. Amend the commit to include the tracker update:
   ```
   git add docs/tracker.md
   git commit --amend --no-edit
   ```

If the branch is not a tracked item (e.g., tooling, CLAUDE.md changes), skip
this step.

### Step 4: Fast-forward merge and push (with retry loop)

```
git checkout main
git merge --ff-only BRANCH
git push
```

**If `git push` fails** (e.g., because another merge landed on main while we
were working), undo the merge and restore the branch from the most recent
backup so that the retry starts from the original unmodified commits:

```
git reset --hard HEAD~1
git checkout BRANCH
git reset --hard backup/BRANCH-pre-rebase-<most-recent-timestamp>
```

Then go back to step 1. The rebase procedure will create a new backup branch
(with a new timestamp, preserving the old backup too) and re-squash the
original commits onto the now-updated main.

If this fails 5 times, **stop and report back** — something unusual is
happening.

### Step 5: Clean up

After the push to main succeeds:

```
git branch -D BRANCH
git push origin --delete BRANCH
```

If `git push origin --delete` fails (e.g., branch was already deleted from
the remote), that's fine — continue.

Also delete all backup branches for this feature branch. List them first,
then delete each one individually:

```
git branch --list "backup/BRANCH-pre-rebase-*"
git branch -D backup/BRANCH-pre-rebase-2026-03-25T14-30-00Z
git branch -D backup/BRANCH-pre-rebase-2026-03-26T09-15-00Z
...
```

These are safe to remove now — the work is on main and pushed to origin.

### Step 6: Report back

Return a concise summary to the outer context:
- Final commit hash and message on main.
- Whether conflicts were encountered and how they were resolved (from the
  rebase agent's report).
- Whether the tracker was updated.
- Any issues or concerns.

## After the agent returns

Review the agent's summary. If the agent stopped for user approval (non-trivial
conflicts, rebase concerns, test failures), address the issue and re-run or
continue manually.

## Why squash first, then rebase?

Rebasing a multi-commit branch onto main can require resolving the same
conflict repeatedly (once per commit). By squashing into one commit first, you
only resolve conflicts once. The backup branch preserves the full original
commit history for reference.
