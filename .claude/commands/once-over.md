# Once-Over: Final Quality Review

Perform a final quality review of the current feature branch before merging to
main. This review should catch bugs, quality issues, documentation staleness,
and anything that would be embarrassing to merge.

## Procedure

1. **Identify changed files.** Run `git diff main --name-only` to get the list
   of files changed on this branch.

2. **Delegate the review to an agent.** Spawn a general-purpose agent with the
   instructions below. The agent does all the reading and analysis, returning a
   concise list of findings. This keeps the main context window clean.

   The agent should:

   ### Code quality
   - Read every changed file in full.
   - Look for bugs, logic errors, off-by-one errors, missed error handling.
   - Check for security issues (injection, unchecked input at boundaries).
   - Check for violations of codebase patterns described in CLAUDE.md (e.g.,
     determinism constraints in the sim crate, no HashMap, no stdlib PRNG).
   - Check that new code matches the style and conventions of surrounding code.
   - Look for dead code, unused imports, TODO/FIXME/HACK comments that should
     be resolved before merge.

   ### Test quality
   - Read all new and modified tests.
   - Check that tests actually assert the behavior they claim to test (not
     just "does it run without panicking").
   - Look for missing edge cases — especially error paths and boundary
     conditions.
   - Verify test names accurately describe what they test.

   ### Documentation accuracy
   - Check that module docstrings in changed files are accurate and up to date.
   - Check module docstrings in **sibling files** that reference changed
     modules — a renamed function, new parameter, or shifted responsibility
     can leave other files' docstrings silently wrong.
   - Check that CLAUDE.md sections (especially "Implementation Status" and
     "Project Structure") are still accurate after the changes.
   - Check that tracker.md entries for related features are up to date.
   - Check that `docs/project_structure.md` is still accurate — especially
     if files were added, removed, renamed, or restructured.

   ### Consistency
   - If the feature adds new protocol messages, config fields, or public API,
     verify they're used consistently across all layers (sim, relay, gdext,
     tests).
   - Check that error messages are helpful and consistent in style.

3. **Review the agent's findings.** If the agent found issues, fix them
   directly — don't just report them back to the user. If the agent found
   nothing, report that the once-over is clean.

4. **Run checks.** After any fixes, run `scripts/build.sh check` and
   `scripts/build.sh test` to confirm everything still passes.

5. **Commit fixes.** If you made changes, commit them to the feature branch
   with a message like "Once-over fixes: [brief summary]" and push.
