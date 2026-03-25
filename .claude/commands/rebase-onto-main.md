# Rebase onto Main

Safely rebase the current feature branch onto the latest main using a
squash-rebase workflow with backup, automated verification, and qualitative
AI review of auto-merged changes.

This command is used in two contexts:
- **Standalone** — mid-work branch update to incorporate main's changes.
- **From `/merge-to-main`** — as the rebase step before FF merge.

## Prerequisites

- You must be on a feature branch (not main).
- Working tree must be clean (`git status` shows no uncommitted changes).
  If there are uncommitted changes, stop and ask the user what to do.

## Procedure

**When invoked directly (standalone),** delegate the entire rebase to a
general-purpose agent. The agent performs all steps below, returning a summary
of what happened. This keeps the main context window clean.

**When invoked from `/merge-to-main`,** the merge agent should execute these
steps directly — do not spawn a sub-agent.

The steps:

### Step 0: Preflight

1. Run `git branch --show-current` — confirm not on `main`. Call this
   `BRANCH`.
2. Run `git status` — confirm clean working tree.
3. Run `git log --oneline main..HEAD` — note the commit count and messages
   (needed for the squash commit message in step 3).

### Step 1: Backup

Get the current UTC timestamp and create a backup branch:

```
date -u +%Y-%m-%dT%H-%M-%SZ
```

Read the output (e.g., `2026-03-25T14-30-00Z`) and use it to create the
backup branch:

```
git branch backup/BRANCH-pre-rebase-2026-03-25T14-30-00Z
```

This backup preserves the full pre-rebase history and is the safety net for
recovery. **Do not delete this branch during the rebase procedure** — cleanup is the
caller's responsibility (either the user or `/merge-to-main`).

### Step 2: Find merge base and capture pre-rebase intent diff

First, update the local main ref and find the merge base:

```
git fetch origin main:main
git merge-base main BRANCH
```

Read the output hash — call this `MERGE_BASE`. Use it for the intent diff:

```
git diff MERGE_BASE..BRANCH > .tmp/intent-before.diff
git diff MERGE_BASE..BRANCH --stat > .tmp/intent-before-stat.txt
```

This captures what the branch changes relative to the branch point *before*
rebasing. Using the merge base (not `main` directly) ensures correctness even
if main has advanced since the branch was created.

### Step 3: Squash commits

Squash all feature commits into a single commit on the working branch. Use
the `MERGE_BASE` hash from step 2:

```
git reset --soft MERGE_BASE
```

Write the squash commit message to `.tmp/commit-msg.txt` using the Write tool,
then commit with `git commit -F .tmp/commit-msg.txt` and
`rm .tmp/commit-msg.txt`. The message should
concatenate all the original commit messages, preceded by a header line:

```
Squash of BRANCH (N commits)

- <commit message 1>
- <commit message 2>
- ...
```

This is a mechanical concatenation — no summarization needed. (If called from
`/merge-to-main`, that command will craft a better final message later.)

### Step 4: Rebase onto main

```
git rebase main
```

**If the rebase succeeds cleanly**, continue to step 5.

**If conflicts arise:**

1. Run `git status` to identify conflicting files.
2. Read each conflicting file and find the `<<<<<<<` / `=======` / `>>>>>>>`
   markers.
3. Understand the intent of both sides (our feature vs. what landed on main)
   by reading surrounding code and recent main commits if needed.
4. Resolve each conflict, preserving the intent of both sides where possible.
5. Stage resolved files and run `git rebase --continue`.

If resolution requires judgment calls, do your best — the qualitative review
in step 7 will catch issues, and the backup branch means nothing is lost.

If the rebase is hopelessly tangled (e.g., massive restructuring on both
sides), **stop and report back** rather than making uncertain resolutions.

### Step 5: Automated verification

Run checks and tests:

```
scripts/build.sh check 2>&1 | tee .tmp/rebase-check.txt
scripts/build.sh quicktest 2>&1 | tee .tmp/rebase-quicktest.txt
```

If either fails:
- Diagnose and fix the issue.
- Re-run until both pass.
- If you cannot resolve a failure after a thorough attempt, **stop and report
  back** with the failure details and your diagnosis.

### Step 6: Intent diff comparison

After rebasing, the branch is now based on main's tip, so main *is* the merge
base. Capture the post-rebase intent diff and compare:

```
git diff main..HEAD > .tmp/intent-after.diff
git diff main..HEAD --stat > .tmp/intent-after-stat.txt
diff .tmp/intent-before.diff .tmp/intent-after.diff > .tmp/intent-delta.txt || true
```

Note: `diff` returns exit code 1 when files differ — this is expected, not an
error.

The intent delta shows exactly what changed about the branch's effect due to
the rebase. Three possible outcomes:

- **Empty delta** — the rebase was purely mechanical. The branch's net effect
  on the codebase is identical. This is the ideal case.
- **Small delta** — some adaptations were needed (conflict resolution, context
  shifts). Each difference needs qualitative review in step 7.
- **Large delta** — something significant changed. Warrants careful review and
  possibly stopping to report back.

### Step 7: Qualitative review

This is the most important step. Even if tests pass and the intent delta is
small, auto-merged code can have subtle semantic issues.

**If the intent delta is non-empty**, read `.tmp/intent-delta.txt` and for
each difference, assess:

- **Expected adaptation** — our code correctly adjusted to main's changes
  (e.g., a renamed import, a new required field).
- **Suspicious** — the change might be wrong but could be intentional. Flag
  for user review.
- **Red flag** — the change looks incorrect or dangerous. Flag prominently.

**Regardless of intent delta size**, scan the auto-merged regions for common
semantic issues:

- Function signature changes on main where our branch calls the function —
  do the arguments still match the new semantics?
- Config or default value changes on main that our code relies on.
- New enum variants on main where our code has match arms — is the handling
  correct?
- Removed or renamed items on main that our code references (compiler catches
  these, but the *fix* during conflict resolution might be wrong).
- Ordering dependencies — did main change iteration order or initialization
  sequence in a way that affects our code?

To understand what changed on main since the branch diverged, review
`git log --oneline MERGE_BASE..main` and `git diff MERGE_BASE..main --stat`
for an overview of what landed. This context helps identify non-obvious
interactions between the branch's changes and main's changes.

Read `.tmp/intent-after.diff` (the full post-rebase diff) alongside any files
where conflicts were resolved or where main made changes that overlap with the
branch's changes.

### Step 8: Report back

Return a concise summary to the outer context:

- **Backup branch name** — so the user knows what to reset to if needed.
- **Conflict resolution** — were there conflicts? How were they resolved?
  Trivial (adjacent lines, imports) or substantive (overlapping logic)?
- **Test results** — did check and quicktest pass on first try? If not, what
  was fixed?
- **Intent diff assessment** — was the delta empty, small, or large? For
  non-empty deltas, summarize each difference and your assessment.
- **Qualitative concerns** — any suspicious patterns found during the scan,
  even if tests pass. Be specific: name the file, the interaction, and why
  it's concerning.
- **Force-push needed** — if invoked standalone (not from `/merge-to-main`),
  remind the user that the branch history was rewritten and they will need to
  `git push --force` to update the remote.
- **Recommendation** — one of:
  - "Clean rebase, no concerns" — everything looks good.
  - "Clean rebase with minor adaptations" — small intent delta, all changes
    look correct, tests pass.
  - "Rebase completed but needs user review" — concerns flagged that require
    human judgment. List them clearly.
  - "Rebase aborted" — could not complete. Explain why. The backup branch
    is intact.

## After a standalone rebase

The branch's history has been rewritten (squashed and rebased). To update the
remote, the user will need to force-push: `git push --force`. Mention this in
the report if the rebase was invoked standalone (not from `/merge-to-main`).

The squash commit has a mechanical concatenation message. The user may want to
amend it with a better summary before continuing work.

If the rebase involved non-trivial conflict resolution, consider suggesting a
`/once-over` to the user before continuing work.

## Recovery

If anything goes wrong after the rebase, the user can restore the original
state:

```
git checkout BRANCH
git reset --hard backup/BRANCH-pre-rebase-YYYY-MM-DDThh-mm-ssZ
```

The backup branch has the full pre-rebase commit history.

## Why squash first, then rebase?

Rebasing a multi-commit branch onto main can require resolving the same
conflict repeatedly (once per commit). By squashing into one commit first, you
only resolve conflicts once. The backup branch preserves the full original
commit history for reference.
