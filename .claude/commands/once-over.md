# Once-Over: Final Quality Review

Perform a final quality review of the current feature branch before merging to
main. This review should catch bugs, quality issues, missing test coverage,
documentation staleness, and anything that would be embarrassing to merge.

## Procedure

1. **Identify changed files.** Run `git diff main --name-only` to get the list
   of files changed on this branch. Also run `git diff main` to get the full
   diff — you'll pass both to the agents.

2. **Spawn three review agents in parallel.** Each agent receives the file list
   and full diff. Launching them in parallel keeps total wall-clock time low
   while giving each agent a focused mandate.

   ### Agent A — Code quality, documentation, and consistency

   This agent handles everything except test coverage analysis.

   **Code quality:**
   - Read every changed file in full.
   - Look for bugs, logic errors, off-by-one errors, missed error handling.
   - Check for security issues (injection, unchecked input at boundaries).
   - Check for violations of codebase patterns described in CLAUDE.md (e.g.,
     determinism constraints in the sim crate, no HashMap, no stdlib PRNG).
   - Check that new code matches the style and conventions of surrounding code.
   - Look for dead code, unused imports, TODO/FIXME/HACK comments that should
     be resolved before merge.

   **Test quality (existing tests only — coverage gaps are Agent B's job):**
   - Read all new and modified tests.
   - Check that tests actually assert the behavior they claim to test (not
     just "does it run without panicking").
   - Verify test names accurately describe what they test.

   **Documentation accuracy:**
   - Check that module docstrings in changed files are accurate and up to date.
   - Check module docstrings in **sibling files** that reference changed
     modules — a renamed function, new parameter, or shifted responsibility
     can leave other files' docstrings silently wrong.
   - Check that CLAUDE.md sections (especially "Implementation Status" and
     "Project Structure") are still accurate after the changes.
   - Check that tracker.md entries for related features are up to date.
   - Check that `docs/project_structure.md` is still accurate — especially
     if files were added, removed, renamed, or restructured.

   **Consistency:**
   - If the feature adds new protocol messages, config fields, or public API,
     verify they're used consistently across all layers (sim, relay, gdext,
     tests).
   - Check that error messages are helpful and consistent in style.

   ### Agent B — Test coverage gap analysis

   This agent's job is **generative**: find code paths that lack test coverage
   and propose specific tests that should be written.

   - For each changed or added module, read the implementation and enumerate
     the distinct code paths: happy path, error/rejection paths, guard
     clauses, match arms, boundary conditions.
   - Cross-reference against existing tests (read the test files) to identify
     paths that have **no corresponding test**.
   - Check for missing interaction tests — what happens when this feature
     intersects with other systems? If a new task can be interrupted, is
     that tested? If a new item can be serialized, is the serde roundtrip
     tested?
   - Check that every new enum variant, config field, or persisted type has
     a serde roundtrip test if sibling variants do.
   - **Output a concrete list of proposed tests**, each with:
     - A descriptive test name (e.g., `test_assign_task_returns_none_when_inventory_full`).
     - One or two sentences explaining what it verifies and why it matters.
     - Which file the test should live in.
   - Prioritize the list: most important (likely bugs, untested error paths)
     first, nice-to-haves last.

   ### Agent C — Adversarial corner-case hunting

   This agent thinks like an attacker or a fuzzer. Its goal is to find
   scenarios where the new code would misbehave and propose tests that
   prove (or disprove) correctness.

   - Read every changed file and understand the invariants the code assumes.
   - Actively try to break those invariants. Consider:
     - Integer overflow or underflow (especially in index math, quantities,
       coordinates).
     - Empty collections where the code assumes non-empty (`.unwrap()` on
       `.first()`, `.next()`, division by `.len()`).
     - Off-by-one errors in loops, ranges, and slice indices.
     - State machine transitions that could leave inconsistent state (e.g.,
       an elf assigned to a destroyed building, a task half-completed when
       the entity dies).
     - Re-entrancy or ordering issues (what if event A triggers event B
       which modifies state that event A is iterating over?).
     - Determinism violations if sim code is touched: iteration order over
       collections, floating-point inconsistencies, anything that could
       diverge across platforms.
   - For each potential issue found, **propose a specific test** with:
     - A descriptive test name.
     - A short description of the scenario and what could go wrong.
     - Which file the test should live in.
   - If the agent finds an actual bug (not just a missing test), flag it
     prominently at the top of the output.

3. **Triage findings from all three agents.** Organize the combined results
   into:
   - **Bugs / must-fix** — actual defects or high-confidence issues.
   - **Missing tests — high priority** — untested error paths, untested
     interactions with other systems, missing serde roundtrips.
   - **Missing tests — nice to have** — additional coverage that would be
     good but isn't critical.
   - **Documentation / style** — docstring staleness, naming, consistency.

4. **Fix bugs and write missing tests.** Fix all must-fix items and write the
   high-priority missing tests directly — don't just report them back to the
   user. For nice-to-have tests, use your judgment: write them if they're
   quick and valuable, otherwise mention them in your summary. For
   documentation and style issues, fix them directly.

5. **Run checks.** After any fixes, run `scripts/build.sh check` and
   `scripts/build.sh quicktest` to confirm everything still passes.

6. **Commit fixes.** If you made changes, commit them to the feature branch
   with a message like "Once-over fixes: [brief summary]" and push.
