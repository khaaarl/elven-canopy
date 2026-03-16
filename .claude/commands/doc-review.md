# Doc Review: Design Document Critique

Review one or more design documents for completeness, accuracy, and usability.
Produces a detailed review with prioritized criticisms written to `.tmp/`.

**Arguments:** Optional document paths or names. If none given, infer from
conversation context (e.g., the document most recently discussed or edited).
Arguments can be exact paths (e.g., `docs/drafts/mobile_support.md`) or vague
names (e.g., `mobile support`, `construction overlap`) — resolve them by
searching `docs/drafts/` and `docs/`.

## Procedure

1. **Resolve document paths.** Find the document(s) to review. If arguments
   are given, glob/grep to locate the actual files. If no arguments, look at
   recent conversation context for the most relevant doc. If no matching
   document is found, tell the user and stop.

2. **Pick a review filename.** Use the pattern `.tmp/<doc-name>_review-N.md`
   where N is the next unused number (check what already exists). This allows
   multiple review iterations without overwriting previous ones.

3. **Delegate the review to an agent.** Spawn a general-purpose agent with the
   instructions below. The agent does all reading and analysis, writes a review
   file, and returns a TL;DR. This keeps the main context window clean.

   The agent should:

   ### Read and understand
   - Read each target document in full.
   - Read CLAUDE.md for project context, constraints, and architecture.
   - Use its own judgement to read additional files that seem relevant — source
     code, tracker entries, sibling design docs, the design doc
     (`docs/design_doc.md`), implementation status. The goal is to have enough
     context to evaluate whether the design doc is accurate and complete.
   - Launch sub-agents in parallel if multiple areas need independent research
     (e.g., one to audit GDScript controls, one to check tracker coverage).

   ### Evaluate
   - **Coverage:** Does the document address everything it should? Are there
     systems, features, edge cases, or interactions it omits?
   - **Accuracy:** Do claims about the codebase, engine, or architecture match
     reality? Are code references correct?
   - **Consistency:** Does the document contradict itself? Are terms used
     consistently? Do different sections agree on behavior?
   - **Usability:** For designs that describe user-facing behavior — are there
     ambiguities, conflicts, or flows that would confuse the user?
   - **Feasibility:** Are there technical claims that seem wrong or risky?
   - **Scope:** Is the document appropriately scoped, or does it hand-wave
     critical details while over-specifying trivial ones?
   - **References:** Verify cross-references to other docs, tracker items, and
     design doc sections — do they exist and are they relevant?

   Be direct and specific. Focus on real problems, not prose style or
   formatting preferences.

   ### Write the review
   Write to the review file (`.tmp/<doc-name>_review-N.md`). The agent should
   go into as much detail as it finds useful — this is the detailed record.
   Structure it as:

   - **Overall assessment** (a few sentences).
   - **Numbered criticisms**, each tagged **[HIGH]**, **[MEDIUM]**, or **[LOW]**
     priority. For each criticism, the agent can write as much detail, analysis,
     and reasoning as it sees fit. Order by priority (all HIGHs first, then
     MEDIUMs, then LOWs).
   - Any additional **notes or observations** at the end.

   ### Return a TL;DR
   After writing the review file, the agent should return to the outer context:
   - One-sentence overall assessment.
   - Each numbered criticism as a single concise line with its priority tag
     and a short suggested fix.

4. **Present findings.** After the agent returns, present to the user:
   - One-sentence overall assessment.
   - Criticisms in the same numbered order as the review file, each on one
     concise line with its priority tag (HIGH/MEDIUM/LOW).
   - For each, a TERSE recommended fix in square brackets (e.g., "[Fix]",
     "[Add a section]", "[Factual error — X not Y]"). Keep brackets to ~5 words
     when possible; only go longer when the fix isn't obvious.
   - Show the first 9 criticisms individually. If there are more than 9,
     summarize the remainder as a group (e.g., "Plus 4 more: typos, missing
     X, ...") — the user can read the full review file for details.
