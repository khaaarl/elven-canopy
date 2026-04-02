# LLM-Driven Creature Decisions — Speculative Draft

> **Status:** Early brainstorm. Nothing here is decided. This document captures
> ideas and directions worth exploring, not commitments.

## Concept

Use a small, locally-run LLM to give certain creatures richer inner lives —
spontaneous social initiative, diplomatic reasoning, nuanced task preferences.
The LLM doesn't replace the existing task/activation system; it nudges
*inclinations* that the existing logic consumes.

Most creatures would not use this. Candidates: player-civilization elves,
foreign civilization leaders, diplomats. Common wildlife (wild or tamed),
invaders, and most non-civ creatures would continue with purely rules-based
behavior. (Future exceptions possible: captured prisoners, invader leaders.)

### What the LLM would control

- Social conversations: generating actual dialogue between creatures
- Social initiative: deciding to organize a dance, seek out a friend
- Task preference weights: "I feel like crafting today" vs "I'd rather be
  social" — soft nudges, not hard overrides
- Diplomacy (later): foreign leaders deciding posture toward the player
- Possibly: inner monologue that accumulates into persistent personality

### What the LLM would NOT control

- Moment-to-moment task execution (existing activation pipeline)
- Combat decisions, dodging, pathfinding
- Whether a task is actually feasible (existing task system is final authority)
- Anything that needs sub-second latency

### The "LLM off" experience

When the LLM is unavailable — player opted out, no capable player in the
session, inference failure — the game treats it as if inference always fails.
The existing task and social systems handle everything. Mechanical effects
(skill checks, opinion changes) still happen; there's just no LLM-generated
flavor text or nuanced inclination shifts. No separate code path for LLM-off.

## Feature Decomposition

This work splits into several independent features, each building on the
infrastructure layer:

1. **F-llm-creatures** — Infrastructure: model download, llama.cpp integration,
   sim → relay → LLM → relay → sim pipeline, multiplayer capability signaling.
2. **F-llm-social-chat** — Creatures generate dialogue in social interactions,
   with messages stored and displayed to the player.
3. **F-llm-convo-ui** — Text bubbles in the world view, conversation log in
   creature detail panel.
4. **F-llm-activities** — LLM-driven scheduling of group activities (dances,
   meals, etc.) and task inclination adjustment.
5. **F-llm-monologue** — Creatures produce inner thoughts that accumulate and
   feed back into future prompts, enabling emergent personality drift.
6. **F-llm-diplomacy** — Foreign civilization leaders use LLMs for diplomatic
   decisions (different prompt templates, same infrastructure).

Prompt engineering is spread throughout these features, not a separate item.

## Infrastructure Implementation Details (F-llm-creatures)

These are directional preferences, not final decisions. Part of the
infrastructure work will be fleshing these out in conversation.

### Crate Structure

- **`elven_canopy_llm`** (new crate): wraps llama.cpp bindings, handles model
  loading, inference execution, KV cache management. Optional feature flag so
  the game compiles without it.
- **`elven_canopy_sim`**: does NOT depend on the LLM crate. The sim emits LLM
  requests via an **outbox** mechanism (a new outbound message system — an enum
  that currently has one variant for LLM requests, but is designed to be
  extensible). The sim consumes LLM responses as canonical inputs, same as
  player commands.
- **`elven_canopy_gdext`**: hosts the inference thread. Reads LLM requests from
  the sim's outbox, routes them through the relay, dispatches to the local LLM
  engine (if this player is LLM-capable), and feeds responses back as sim
  inputs.
- **`elven_canopy_relay` / `elven_canopy_protocol`**: new message types for LLM
  request dispatch and response canonicalization.

The sim's outbox is a general-purpose mechanism — it doesn't know about LLMs
specifically, just that it has outbound requests that need external resolution.

### Inference Library Selection

Evaluate `llama-cpp-rs` and `llama-cpp-2` and pick one based on API quality,
build story, and maintenance activity. Both wrap the same underlying C++ library.

### LLM Request/Response Wire Format

Requests are semi-structured:
- Indicate which **preamble(s)** to use (these are known ahead of time and
  identified by name/ID, enabling KV cache reuse on the inference side)
- Freeform prompt text for the creature-specific portion
- Request ID, creature ID, deadline tick for correlation

Responses contain the raw LLM output text plus metadata (latency, cache hit,
token count) for observability.

The exact schema will be designed during implementation, but the envelope
(request ID, creature ID, deadline tick, preamble IDs, prompt, response) should
be generic enough to serve social chat, activities, and diplomacy.

### Model Download

Details (HTTP client, resume support, CDN hosting) deferred to implementation.
Core requirement: user-initiated opt-in download, checksum verification,
platform-appropriate storage location.

## Architecture

### Determinism

LLM outputs are non-deterministic across hardware/runtimes, but this is fine
because they're treated as **external canonical inputs**, structurally identical
to player commands.

- At tick `n`, the sim packages a context snapshot and kicks off an async LLM
  request on a background thread.
- The sim continues processing ticks normally.
- The result is expected by tick `m` (seconds later).
- At tick `m-1`, the sim blocks if the result isn't ready yet.
- The LLM output becomes a canonical state transition — all instances in
  multiplayer apply the same result.

This is the same pattern you'd use for any async oracle. The sim doesn't care
*how* the decision was made.

### Relay as the ONLY Path (CRITICAL)

**ALL LLM inference — singleplayer and multiplayer alike — goes through the
relay.** There is NO special singleplayer shortcut. The relay is the sole
mechanism by which LLM requests are dispatched and results are canonicalized.

This is non-negotiable. A singleplayer-only code path WILL introduce bugs that
only surface in multiplayer, which is the hardest place to debug them. The relay
path must be exercised in every mode, always.

In singleplayer, the relay runs locally (the existing LocalRelay), dispatches
the inference request to the local machine, and returns the result through the
same canonical pipeline as multiplayer. The code that submits an LLM request and
the code that consumes the result must be identical regardless of player count.

### Multiplayer Capability Signaling

When a player joins a session, they signal whether they have the LLM model
downloaded and ready. The relay uses this to decide which player(s) to dispatch
inference requests to.

- If multiple players have the model, the relay can load-balance across them.
- If the only LLM-capable player disconnects, inference gracefully fails and
  creatures revert to rules-based behavior (the standard fallback).
- A multiplayer game gets LLM features as long as *any* player has the model.

### Observability and Debugging

When something goes wrong (nonsensical output, performance degradation, prompt
issues), debugging needs visibility into the pipeline:

- Log prompts, raw responses, latency, cache hits/misses
- Track inference failures and retry counts
- Per-creature decision history (what was requested, what came back, how it
  was applied)
- This logging should be toggleable (off by default, verbose when debugging)

## Social Conversation Model

When the existing system detects a social interaction (e.g. two elves passing
each other), instead of resolving it purely mechanically, it can delegate to
the LLM pipeline:

1. Existing system detects Elfandriel and Thandril will pass each other,
   schedules casual social interaction as it does today.
2. Elfandriel's LLM gets context: "you're about to pass Thandril, your friend
   (opinion: 65). What do you say?" Produces a greeting. A social skill check
   happens mechanically.
3. The greeting text + skill check result goes into Thandril's **inbox**.
4. Thandril's LLM processes inbox on its next cycle: sees the greeting, the
   skill check outcome, and its own context. Produces a response and possibly
   a follow-up action (e.g. "invite Elfandriel to tonight's dance").
5. Response goes back to Elfandriel's inbox, plus mechanical effects (dance
   invitation creates a pending activity).

The multi-turn exchange takes several LLM cycles (~5-10 seconds total), which
maps naturally to two creatures stopping, chatting, and moving on.

### Message Storage

Messages are small (a sentence or two each) and stored in the sim database.
Viewable in the creature detail panel as a conversation log. Garbage collected
over time — keep the last N per creature or everything from the last in-game
week, whichever is more.

Unprocessed messages sit in the creature's inbox until their next LLM cycle.
Once processed and responded to, they move to history.

## Inner Monologue (speculative, deferred)

The LLM could produce "inner monologue" text as an optional output alongside
its decisions. Part of the prompt would include a summary of past inner
monologues, creating a feedback loop where creatures develop emergent
personalities.

The Big Five personality traits (from genetics) set the *interpretive lens*:
high neuroticism in the prompt means the LLM actually reads negative intent
into ambiguous situations. A creature might receive a compliment with a low
skill check and interpret it as an insult, developing a grudge — emergent
behavior no designer would explicitly code.

**Integration with existing thoughts system:** Inner monologue entries could go
into the existing thoughts table rather than a separate system. The player sees
them alongside mechanical thoughts in the same UI. Tonal inconsistency
("Ate a satisfying meal" next to "I can't stop thinking about what Thandril
said") might be charming rather than jarring.

**Context budget concern:** Inner monologue history grows over time but the
prompt budget is tight (~100 tokens for this). Options:
- Rolling window of last ~3 entries (simple, cheap)
- Relevance-based selection (entries mentioning nearby creatures)
- Periodic LLM summarization ("reflect on your week in 2-3 sentences") — the
  summarizer prompt re-anchors to personality traits to prevent drift

This is deferred — the social chat system works without it.

## Latency and Throughput

- **Latency budget:** Multiple seconds per decision. These are coarse-grained
  life choices, not reflexes.
- **Decision frequency:** Roughly once per minute per creature.
- **Throughput:** ~100 creatures × 1/minute ≈ 1-2 calls/sec average. Stagger
  decision timers with jitter to smooth bursts.
- **Batching:** Probably not needed initially. One creature per call is simpler
  and avoids unnatural omniscience. Revisit if throughput becomes a problem.
- **Scaling:** Reduce frequency for less "important" creatures if needed. Most
  creatures (wildlife, invaders, tamed animals) don't use LLM at all.

## Prompt Budget

Small models degrade well before their context window fills. Target ~600 tokens
total:

- ~200 tokens: base preamble (game rules, output format, decision categories)
- ~100 tokens: species/path/personality (Big Five traits from genetics)
- ~100 tokens: recent thoughts summary, current mood, key relationships
- ~100 tokens: inbox (unprocessed messages from other creatures)
- ~50 tokens: immediate situation
- ~50 tokens: output budget

## Prompt Caching (KV Cache Reuse)

llama.cpp supports saving and restoring KV cache state. The prompt should be
structured in cleanly separated tiers to exploit this:

1. **Base preamble** — game rules, output format, decision categories. Processed
   once at model load, cached permanently. One copy, ~100–200MB. Always cached.

2. **Species/path prefix** — e.g. "you are an elf of the Warrior path, your
   values are..." Cached **adaptively based on population**: if 40 Warrior elves
   share this prefix, cache it; if there's one goblin diplomat, process from
   scratch. Threshold could be a fixed count (e.g. N≥5) or dynamic based on
   available VRAM.

3. **Per-creature ephemeral context** — recent thoughts, inbox, immediate
   situation. Always processed fresh, but small (~100-200 tokens).

This turns a ~500-token prefill into a ~100-200-token prefill for the common
case. The caching is purely a performance optimization with zero semantic
impact.

**Important:** the tier separation should be clean from the start even if
caching isn't implemented immediately, so it can be added later without
redesigning prompts.

## Model Selection (speculative)

Target the **0.5–1.5B quantized** range. The task is simple: given a short
context dump (~500 tokens), produce a small structured object or short text
(~50 tokens).

Candidates worth evaluating (landscape will change):
- Qwen 2.5 0.5B / 1.5B — Apache 2.0, strong structured output
- SmolLM2 1.7B — Apache 2.0, designed for on-device
- Gemma 2 2B — permissive license

Licensing matters for game distribution. Apache 2.0 or MIT strongly preferred.
Model should be configurable so power users can swap in something larger.

**No fine-tuning planned initially.** Few-shot prompting and prompt engineering
only. Fine-tuning is a much later optimization if the approach proves out.

Format: GGUF quantized (Q4 or similar), ~500MB–1GB on disk, ~1–2GB VRAM.

## Inference Library (speculative)

**llama.cpp via Rust bindings** is the leading candidate:
- Most mature inference engine
- Widest hardware support (CUDA, ROCm, Vulkan, Metal, CPU)
- GGUF is the standard quantized format
- Active community

Two main Rust binding crates: `llama-cpp-rs` and `llama-cpp-2`. Evaluate both
during implementation and pick based on API quality, build story, and
maintenance activity. Both wrap the same underlying C++ library.

Build complexity (cmake, optional CUDA toolkit) is the main downside. Should be
an **optional feature flag** so the game compiles and runs without it.

Alternatives considered but not preferred:
- Candle (Hugging Face, pure Rust) — less mature for inference, narrower
  hardware support
- Mistral.rs — more server-oriented than embeddable

## GPU Backend (speculative)

**Vulkan as default.** Cross-platform, works on NVIDIA/AMD/Intel, and the player
already has a Vulkan-capable GPU since Godot requires one. No extra SDK
requirements.

- CUDA (NVIDIA) and ROCm (AMD, Linux) as optional higher-performance backends
  for players who have them installed.
- CPU inference as zero-GPU-contention fallback. Slow for larger models but
  possibly adequate for 0.5B with the multi-second latency budget. Worth
  benchmarking.

### GPU contention with Godot

Inference runs on a separate thread. The GPU driver handles resource sharing
between Godot's Vulkan rendering and llama.cpp's Vulkan inference.

- Per-token decode: ~5–20ms on a 1.5B model
- Prompt prefill (processing full input at once): ~50–200ms, the heavier
  operation and more likely source of frame hitches
- llama.cpp does not support pausing mid-token — each token decode is an atomic
  GPU submission. You get control back between tokens but can't yield
  mid-matrix-multiply.

For a non-competitive game with intermittent inference, minor frame time
variance is likely invisible. If profiling shows problems: drop to 0.5B, use
CPU inference, or limit inference to low-GPU moments.

## Model Download and Storage

Not bundled with the game — too large for players who won't use it.

- On first launch (or via settings), prompt: *"Enable AI creature personalities?
  (requires ~1GB download)"*
- Download in background; game is fully playable without it
- Store in platform-conventional data directory alongside saves/config
  (`~/.local/share/elven_canopy/models/`, `%APPDATA%/ElvenCanopy/models/`, etc.)
- Manifest file (model name, expected SHA256, download URL) for checksum
  verification and version management
- If a game update requires a different model, detect stale model via manifest
  and re-download

## Open Questions

- **Prompt design:** What context to include, how to structure output for
  reliable parsing. Constrained/structured generation?
- **Batching tradeoffs:** Does batching multiple creatures improve quality
  (coordination) or hurt it (omniscience)?
- **Save/load:** Inclination state is sim data (easy), but in-flight LLM
  requests at save time need handling.
- **VRAM budget:** How to detect available VRAM for adaptive cache decisions.
  Cache eviction policy.
- **CPU viability:** Is CPU inference on 0.5B fast enough to be a viable default
  rather than just a fallback?
- **Distribution mechanics:** CDN hosting, download resumption, model versioning
  across game updates.
- **Thought system integration:** Leaning toward using the existing thoughts
  table rather than a separate system, but not yet decided.
