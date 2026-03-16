# Repeated Doc Review: Iterative Document Polish

Repeatedly review and improve a design document until it stabilizes. Each round
a fresh review agent critiques the doc, then a fresh update agent addresses the
critiques. Stops when there are no HIGH/MEDIUM issues and the updater judges
the remaining LOW issues not worth addressing.

**Arguments:** A single document path or name (exact path like
`docs/drafts/mobile_support.md`, or vague like `mobile support`). If none
given, infer from context. The user may also provide intent context (e.g.,
"this is a mobile touch adaptation design") — if so, capture it. If not,
extract a one-sentence summary from the document's own overview section.

## Procedure

### Step 0: Setup

1. **Resolve document path.** Find the document to review. If arguments are
   given, glob/grep to locate the actual file. If no arguments, look at recent
   conversation context for the most relevant doc. If no matching document is
   found, tell the user and stop.
2. **Capture author intent.** A short string describing what the document is
   trying to accomplish and any constraints the user has expressed. This is
   passed to every update agent to prevent drift. If the user provided context,
   use that. Otherwise, read the document's overview/introduction and summarize
   in one sentence.
3. **Check branch.** If not on main, updates will be committed and pushed after
   each round. If on main, updates are written but not committed (per repo
   rules).

### Step 1: Review round

Spawn a review agent. Give it the document path and these instructions:

> Read the target document in full. Read CLAUDE.md for project context,
> constraints, and architecture. Use your own judgement to read additional files
> that seem relevant — source code, tracker entries, sibling design docs, the
> design doc (`docs/design_doc.md`), implementation status. The goal is to have
> enough context to evaluate whether the design doc is accurate and complete.
> Launch sub-agents in parallel if multiple areas need independent research.
>
> Evaluate the document on these axes:
> - **Coverage:** Does it address everything it should? Omitted systems,
>   features, edge cases, interactions?
> - **Accuracy:** Do claims about the codebase, engine, or architecture match
>   reality?
> - **Consistency:** Does it contradict itself? Do different sections agree?
> - **Usability:** For user-facing designs — ambiguities, conflicts, confusing
>   flows?
> - **Feasibility:** Technical claims that seem wrong or risky?
> - **Scope:** Hand-waving critical details while over-specifying trivial ones?
> - **References:** Do cross-references to other docs, tracker items, and
>   design doc sections exist and are they relevant?
>
> Be direct and specific. Focus on real problems, not prose style or formatting.
>
> Write to `.tmp/<doc-name>_review-N.md` (use the next unused N). Structure as:
> - **Overall assessment** (a few sentences).
> - **Numbered criticisms**, each tagged **[HIGH]**, **[MEDIUM]**, or **[LOW]**.
>   Go into as much detail as useful. Order by priority. Include a short
>   suggested fix for each.
> - Any additional **notes or observations** at the end.
>
> Return a TL;DR: one-sentence assessment + each numbered criticism as a concise
> line with priority tag and suggested fix.

**Do NOT add your own commentary or brackets.** Just receive the TL;DR.

### Step 2: Check termination

If the review has **zero criticisms at any level**, the document is stable.
Report final status to the user and stop — do not spawn an update agent.

If the review has **no HIGH or MEDIUM criticisms** (LOW only), proceed to
step 3 — the update agent will decide whether the LOWs are worth addressing.

Otherwise, proceed to step 3 with the full review.

### Step 3: Update round

Spawn an update agent. Give it:
- The current document path.
- The current review file path.
- The author intent string from step 0.
- These instructions:

> You are fixing review issues in a design document. Read the document, the
> review, and CLAUDE.md (for project context and commit conventions). Use your
> own judgement to read additional codebase files if needed to fix accuracy or
> feasibility issues identified in the review.
>
> Address criticisms as follows:
>
> - **HIGH and MEDIUM:** Fix all of these. Make targeted edits — fix the gap or
>   error identified, do not rewrite surrounding sections or redesign the
>   document.
> - **LOW:** Use your judgement. Fix ones that are clearly correct and cheap to
>   address. Skip ones that are subjective, out of scope, or would require
>   significant restructuring. Briefly explain why you skipped any that you
>   chose not to address.
>
> **Author intent:** [insert intent string]. Do not drift from this intent.
> You are polishing, not redesigning.
>
> After making changes:
> 1. Bump the version number in the document (e.g., v3 → v4). If the document
>    has no version number yet, add `**Version:** v2` near the top (after the
>    title/status), treating the pre-review state as v1.
> 2. Append a changelog entry at the very end of the file:
>    `## Changelog`
>    `### vN`
>    `- Brief description of each change made this round.`
>    (Append to existing Changelog section if one exists.)
> 3. If not on main, commit and push. Follow the repo's commit conventions
>    from CLAUDE.md (write message to `.tmp/commit-msg.txt`, commit with
>    `-F`, no `-m` flags or heredocs). Use message format:
>    `<doc-name>: review round N fixes (vX → vY)`
> 4. Return a summary: what you changed, what you skipped and why, and whether
>    you believe the document is now stable (no remaining issues worth fixing).

If the review was LOW-only and the update agent reports that none of the LOWs
were worth addressing, **stop — the document is stable.** Report final status
to the user.

### Step 4: Loop

If the update agent made changes, go back to step 1 for another review round.

**Hard cap: 5 rounds.** If the document hasn't stabilized after 5 rounds, stop
and report to the user with the current state. Something is likely wrong (two
agents disagreeing, scope creep, etc.).

### Step 5: Report

When the loop terminates (either stable or cap reached), tell the user:
- How many rounds ran.
- Final document version.
- Whether it stabilized or hit the cap.
- One-line summary of what changed across all rounds.
- Path to the final review file if the user wants to inspect it.
