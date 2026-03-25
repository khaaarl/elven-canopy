# Once-Over: Final Quality Review

Perform a quality review of the current feature branch, typically before
merging to main. This review should catch bugs, quality issues, missing
test coverage, documentation staleness, and anything that would be
embarrassing to merge. The user may ask for a once-over prior to merging,
or while in the middle of development. The user might ask for multiple
once-overs in succession, which helps check for issues more thoroughly.

## Procedure

1. **Gather inputs.** Capture diffs and write a shared context file:
   - `git diff main --name-only > .tmp/once-over-files.txt`
   - `git diff main > .tmp/once-over-diff.txt`
   - Write `.tmp/once-over-context.md` containing, in order:
     1. **Tracker IDs** — the tracker item ID(s) for the work on this branch.
     2. **Design doc paths** — paths to any design docs linked from the
        tracker or referenced in conversation.
     3. **Summary** — a brief summary of what the user asked for and why.
     4. **User requests (verbatim)** — the user's literal text requests from
        the conversation that prompted the work. Copy verbatim — do not
        paraphrase or abbreviate.

   These files persist in `.tmp/` so agents can re-read them during their
   review and so you can reference them during triage.

2. **Spawn four review agents in parallel.** Each agent reads the shared
   context file and its own instruction file. Launching them in parallel
   keeps wall-clock time low.

   | Agent | Instruction file |
   |-------|-----------------|
   | A — Code quality, docs, consistency | `docs/once-over/code-quality.md` |
   | B — Test coverage gap analysis | `docs/once-over/test-coverage.md` |
   | C — Adversarial corner-case hunting | `docs/once-over/corner-cases.md` |
   | D — Spec adherence & shortcut detection | `docs/once-over/spec-adherence.md` |

   Use the Agent tool to spawn each agent with the prompt: "Read
   `docs/once-over/<your-file>.md` for your mandate. Read
   `.tmp/once-over-context.md`, `.tmp/once-over-files.txt`, and
   `.tmp/once-over-diff.txt` for shared inputs."

3. **Triage findings from all four agents.** Organize the combined results
   into:
   - **Bugs / must-fix** — actual defects or high-confidence issues.
   - **Spec violations / must-fix** — requirements missing or implemented
     incorrectly (from Agent D).
   - **Spec ambiguities — need user decision** — spec is unclear and the code
     made an assumption (from Agent D).
   - **Missing tests — high priority** — untested error paths, untested
     interactions, missing serde roundtrips.
   - **Missing tests — nice to have** — additional coverage, not critical.
   - **Shortcuts / quality issues** — implementations that work but should be
     done properly (from Agent D).
   - **Test integrity issues** — tests that don't test what they claim
     (from Agents A and D).
   - **Documentation / style** — docstring staleness, naming, consistency.

4. **Fix bugs and write missing tests.** Fix all must-fix items and write the
   high-priority missing tests directly — don't just report them. For
   nice-to-have tests, use your judgment: write if quick and valuable,
   otherwise mention in summary. Fix documentation and style issues directly.
   For spec violations, fix any with a clear correct implementation; escalate
   ambiguous ones to the user. Fix shortcuts/quality issues.

   **Pre-existing bugs.** If any agent flags a bug that is unrelated to the
   current branch's work (e.g., a latent issue in code that was only read,
   not changed), highlight it prominently to the user and suggest adding it
   as a tracker bug via `scripts/tracker.py add`.

   **Dismissals.** When dismissing an agent's finding, you MUST include a
   concise but thorough summary of the complaint and a clear explanation of
   why it is safe to dismiss. Do not silently drop findings — the user needs
   to see what was raised and why you consider it resolved. A bare "not
   relevant" or "already handled" is not sufficient; cite the specific code
   or reasoning that makes the finding a non-issue.

5. **Verify via CI.** After any fixes, commit and push to the feature branch,
   then wait for CI:

   ```
   git push
   scripts/wait-for-ci.sh
   ```

   The script polls GitHub Actions CI on the exact pushed commit SHA. It exits
   early on any job failure and doesn't block on the slow `coverage` job. If
   CI fails, diagnose and fix the issue, then re-push and re-run.

6. **Commit fixes.** If you made changes, commit them to the feature branch
   with a message like "Once-over fixes: [brief summary]" and push. (If step
   5 already pushed the fix commits, this step is done.)
