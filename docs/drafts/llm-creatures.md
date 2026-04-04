# LLM-Driven Creature Decisions — Design Draft

> **Status:** Research complete, design in progress. Infrastructure decisions
> (library, model size, relay pipeline) are firming up. Prompt engineering and
> feature-specific details (social chat, activities, diplomacy) are still
> speculative.

**Version:** v6

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

## Blocking Dependency: B-local-relay

**F-llm-creatures is blocked by B-local-relay.** The current `LocalRelay` is a
tick-pacing timer that doesn't route messages or use networking. Singleplayer
commands bypass the relay entirely. The fix is to delete `LocalRelay` and launch
the real relay (`elven_canopy_relay`) in singleplayer mode with localhost-only
options — same relay, same code, same TCP networking. See B-local-relay in the
tracker for full details.

## Infrastructure Implementation Details (F-llm-creatures)

### Crate Structure

- **`elven_canopy_llm`** (new crate): wraps llama.cpp bindings, handles model
  loading, inference execution, KV cache management. Always compiled — every
  build includes LLM support.
- **`elven_canopy_sim`**: does NOT depend on the LLM crate. The sim emits LLM
  requests via an **outbox** mechanism (see "Sim Outbox" below). The sim
  consumes LLM responses as canonical inputs, same as player commands.
- **`elven_canopy_gdext`**: hosts the inference thread. Reads LLM requests from
  the sim's outbox, routes them through the relay, dispatches to the local LLM
  engine (if this player is LLM-capable), and feeds responses back as sim
  inputs.
- **`elven_canopy_relay` / `elven_canopy_protocol`**: new message types for LLM
  request dispatch and response canonicalization.

The sim's outbox is a general-purpose mechanism — it doesn't know about LLMs
specifically, just that it has outbound requests that need external resolution.

### Inference Library: `llama-cpp-2`

**Decision: use `llama-cpp-2`** (crate `llama-cpp-2`, repo
`utilityai/llama-cpp-rs`). This is the only actively maintained Rust binding
for llama.cpp. The other main contender (`edgenai/llama_cpp-rs`) has been
unmaintained since November 2023 and is missing years of upstream improvements.

Key capabilities confirmed through research (April 2026):

- **KV cache save/restore:** Exposed via `context::session` module (as of
  v0.1.141; verify exact API at implementation time since the crate evolves
  rapidly). Process a shared preamble once, save the KV state, restore it for
  each per-creature query. This directly enables the tiered prompt caching
  design (see "Prompt Caching" below).
- **Grammar-constrained generation:** `json_schema_to_grammar()` converts a
  JSON schema string to a GBNF grammar, which is passed to the sampler chain.
  The model is forced to produce valid JSON matching the schema — no parsing
  failures, no hallucinated output formats.
- **GGUF model loading:** `LlamaModel::load_from_file(path, params)` loads from
  any filesystem path. Models are self-contained GGUF files (weights + tokenizer
  + metadata).
- **Thread safety:** A `LlamaModel` is shareable across threads (read-only after
  load). Each `LlamaContext` must be used from one thread at a time, but
  multiple contexts can share the same model. This fits naturally with the
  existing async mesh worker pattern (worker threads + mpsc channel).
- **GPU backend feature flags:** `cuda`, `vulkan`, `rocm` as Cargo features on
  `elven_canopy_llm`. The underlying `llama-cpp-sys-2` drives CMake in its
  `build.rs` and handles cross-platform builds automatically. The base crate
  (CPU inference) is always compiled; GPU backends are additive.

Build requirements: CMake + C++ compiler + `libclang-dev` (always required for
bindgen). CUDA Toolkit, Vulkan SDK, or ROCm are only required for their
respective GPU feature flags.

First build is slow (compiling llama.cpp's C++). Subsequent builds are cached.

The `llama-cpp-2` API is a thin wrapper that mirrors llama.cpp's C API directly.
It prioritizes completeness over safety — the docs note it is "not safe" in that
misuse of the llama.cpp API can trigger UB. This is acceptable for our use case
since the LLM crate is a controlled internal consumer, not a public API.

Cargo integration:
```toml
[dependencies]
llama-cpp-2 = "0.1"

[features]
default = []
cuda = ["llama-cpp-2/cuda"]
vulkan = ["llama-cpp-2/vulkan"]
rocm = ["llama-cpp-2/rocm"]
```

### GPU Backend Selection

llama.cpp supports CUDA, Vulkan, ROCm, Metal, and CPU. The GPU backends are
selected at **build time** via Cargo feature flags (`llm-cuda`, `llm-vulkan`,
`llm-rocm`) — each compiles llama.cpp with a different CMake backend
configuration. A single binary compiled with `llm-vulkan` cannot use CUDA at
runtime. The **runtime** choice is between GPU inference (whichever backend was
compiled in) and CPU fallback, which llama.cpp always supports regardless of
the compiled GPU backend.

**Build-time backend defaults per platform:**
- **Linux/Windows:** Vulkan (cross-platform, works on NVIDIA/AMD/Intel; player
  already has a Vulkan-capable GPU since Godot requires one)
- **macOS:** Metal (native Apple GPU support, compiled via default llama.cpp
  build on macOS — no feature flag needed)
- **Optional accelerated builds:** CUDA (NVIDIA, ~20-30% faster than Vulkan on
  NVIDIA hardware) and ROCm (AMD, Linux) available as opt-in build variants for
  players who want maximum performance and have the requisite SDK installed.

**Backend performance context:**
- Vulkan prefill performance lags CUDA significantly (one benchmark: 524 t/s
  CUDA vs 84 t/s Vulkan on A100), but for a 1.7B model with multi-second
  latency budget this may be acceptable. Some stability issues remain with
  certain quantization formats as of early 2026.
- CPU is slowest but zero GPU contention with Godot. Viability depends on
  benchmarking with our target model size.

The right answer depends on benchmarking with our specific model and workload.
The architecture supports all backends via feature flags and makes it easy to
ship different builds. We start with Vulkan as the default distribution build
and add CUDA/ROCm as opt-in acceleration.

**GPU contention with Godot:** Inference runs on a separate thread. The GPU
driver handles resource sharing between Godot's Vulkan rendering and llama.cpp's
compute. Per-token decode is ~5–20ms on a 1.5B model; prompt prefill is
~50–200ms and more likely to cause frame hitches. For a non-competitive game
with intermittent inference, minor frame time variance is likely invisible.

**VRAM budget:** A 1.7B Q5_K_M model needs ~1-2 GB VRAM. Godot's Vulkan
renderer typically uses 500MB-1.5GB depending on scene complexity. On a 4GB GPU
(common mid-range), this is tight. On an 8GB GPU, comfortable. Mitigation:
- At startup, query available VRAM (llama.cpp exposes this for Vulkan/CUDA).
  If insufficient for GPU inference, fall back to CPU automatically.
- Expose a setting: "LLM acceleration: Auto / GPU / CPU". Auto uses the VRAM
  check; GPU uses the compiled-in backend; CPU forces CPU-only inference
  regardless of available GPU. Manual overrides for users who know their
  hardware.
- CPU inference on a 1.7B model is slower (~1-5s per request on modern CPUs)
  but within the latency budget and avoids all GPU contention.

### Model Selection

**Target range: 1.5–3B parameters, quantized.** Research confirms that 1.5B is
the practical floor for reliable structured output (JSON schema following with
semantically meaningful values). Sub-1B models can produce valid JSON with
grammar constraints but fill fields with poor-quality values.

**Primary candidate: Qwen 3 1.7B** (non-thinking mode)
- Apache 2.0 license — fully redistributable, no restrictions, no branding
  requirements
- Best-in-class structured output at this size tier (inherited from Qwen 2.5's
  JSON optimization plus Qwen 3 improvements)
- Official GGUF from the Qwen team on HuggingFace
- Supports 128K context (our ~600 tokens is trivially within range)
- Q5_K_M: ~1.3 GB on disk, ~1-2 GB VRAM
- Q8_0: ~1.8 GB on disk, ~2 GB VRAM
- Non-thinking mode must be used for structured output (thinking mode does not
  support grammar-constrained generation)

**Runner-up: Qwen 2.5 1.5B** — more battle-tested, slightly smaller, same
Apache 2.0 license. The practical difference from Qwen 3 1.7B is small.

**Worth monitoring: Gemma 4 E2B** — Apache 2.0 (a change from Gemma 1-3's
restrictive custom license), edge-optimized, released April 2026. Too new for
structured output benchmarks but the right profile for our use case.

**Rejected alternatives:**
- **Gemma 1/2/3:** Custom "Gemma Terms of Use" license (NOT Apache 2.0 despite
  marketing) with a remote usage restriction clause — dealbreaker for game
  distribution. Note: Gemma 4 switched to Apache 2.0, which is why it's listed
  as a candidate above while Gemma 1-3 are rejected.
- **Llama 3.2 (1B, 3B):** Custom license requiring "Built with Llama" branding
  and with a 700M MAU threshold clause. Less attractive than Apache 2.0
  alternatives of equal or better quality.
- **Phi-4 Mini (3.8B):** MIT license (excellent), but 3.8B is above our target
  range for per-NPC inference throughput.
- **SmolLM2 (1.7B):** Apache 2.0 but weaker than Qwen at structured output in
  benchmarks at the same size.

**Quantization:** Small models are more sensitive to quantization than large
ones. Q4 at 1.7B has measurable quality degradation on reasoning tasks.
Recommendation: **Q5_K_M** (best balance of size and quality for sub-2B models)
or **Q8_0** (nearly identical to FP16, ~500 MB larger). Q4_K_M is acceptable
only if disk size is critical. Q3 and below are not recommended for sub-2B
models.

The model should be configurable so power users can swap in something larger or
a fine-tuned variant. The default ships with the game's download mechanism; the
config points to a GGUF path.

**No fine-tuning planned initially.** Few-shot prompting, prompt engineering,
and grammar-constrained generation only. Fine-tuning is a much later
optimization if the approach proves out.

### Sim Outbox

The sim currently has no mechanism for emitting requests that need external
resolution. All outputs are events (for UI consumption) or state changes. LLM
requests are a new category: the sim decides it needs an external decision, but
the sim itself cannot perform inference.

**Design:** A `Vec<OutboundRequest>` on the sim state, drained by the caller
after each `step()` call, analogous to how events work.

```rust
/// A request from the sim that needs external resolution.
/// The sim emits these; the hosting layer (gdext) drains and fulfills them.
pub enum OutboundRequest {
    /// Request LLM inference for a creature decision.
    LlmInference {
        request_id: u64,
        creature_id: CreatureId,
        /// Prompt preamble sections, in order. Each is either a well-known
        /// enum variant (for text that doesn't change during gameplay, e.g.,
        /// base game rules) or a literal string (for text that varies, e.g.,
        /// species/path description built from current game state). The
        /// inference layer decides whether to cache KV state for any section.
        preambles: Vec<PreambleSection>,
        /// The creature-specific ephemeral context (recent thoughts, inbox,
        /// immediate situation). Always processed fresh.
        prompt: String,
        /// The JSON schema the response must conform to.
        response_schema: String,
        /// Tick by which the response is needed. If missed, the request is
        /// treated as a failure (creature keeps current inclinations).
        deadline_tick: u64,
        /// Maximum tokens to generate (`n_predict` in llama.cpp). Per-request
        /// to allow different features different output budgets (e.g., social
        /// chat ~50 tokens, diplomacy responses may be longer).
        max_tokens: u32,
    },
    // Future variants: HTTP callouts, analytics events, etc.
}

pub enum PreambleSection {
    /// A well-known preamble identified by string key. The text is fixed
    /// at compile time or loaded from config at startup and doesn't change
    /// during gameplay. The inference layer maintains a registry of known
    /// preamble texts keyed by these strings and can cache KV state for them.
    /// Examples: "base_rules", "social_chat_format", "activity_format".
    /// Implementation note: use `Cow<'static, str>` or intern the keys to
    /// avoid per-request allocation for these fixed strings.
    WellKnown(String),
    /// A literal preamble string built from current game state (e.g.,
    /// species/path description with current trait values). May vary per
    /// creature or over time.
    Literal(String),
}
```

The sim assigns monotonically increasing `request_id`s (from a
`next_request_id: u64` counter on `SimState`, same pattern as
`next_structure_id`) and tracks pending requests in a
`BTreeMap<u64, PendingLlmRequest>` on `SimState`:

```rust
pub struct PendingLlmRequest {
    pub request_id: u64,
    pub creature_id: CreatureId,
    /// What kind of request this is — determines how to deserialize and
    /// apply the response.
    pub request_kind: LlmRequestKind,
    pub deadline_tick: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LlmRequestKind {
    SocialChat {
        /// The other creature in the interaction.
        target_creature_id: CreatureId,
    },
    // Future: ActivityInclination, Diplomacy, etc.
}
```

A `BTreeMap` (not `HashMap`) ensures deterministic iteration order when
processing expirations or iterating for save/load. The creature's
"has pending request" state is derived: check if any entry in the map has a
matching `creature_id`, or maintain a secondary `BTreeSet<CreatureId>` for O(1)
lookup. Both the map and set are serialized as part of sim state.

When a response arrives (via `SimAction::LlmResult` in a Turn), the sim matches
it by `request_id`, validates the deadline hasn't passed, and applies the result.

Key properties:
- The sim never blocks on inference. It emits the request and continues.
- If the deadline passes without a response, the request silently expires. The
  creature keeps its current inclinations. No error, no fallback code path —
  the existing task system handles everything.
- The outbox is deterministic: given the same sim state, the same requests are
  emitted. The *responses* are non-deterministic (LLM output), but they enter
  the sim as canonical external inputs, same as player commands.
- The outbox is drained by the hosting layer (gdext) after every `step()` call,
  not just at turn boundaries. After B-local-relay, `step()` is called once per
  Turn (advancing potentially multiple ticks). Requests emitted during any tick
  within that step are all drained together after the step returns. This is
  fine — the drain cadence affects latency (when the request enters the relay
  pipeline) but not correctness.
- The outbox mechanism is generic. `OutboundRequest` is an enum that currently
  has one variant for LLM inference, but is designed to be extensible for any
  future external-resolution need.

### LLM Message Routing in the Relay

**Decision: Hybrid — dedicated dispatch, turn-based delivery.**

LLM requests originate from the sim (not from a player), need to be dispatched
to an LLM-capable peer, and produce async results that must be canonicalized
across all players. The chosen approach uses dedicated protocol messages for
request dispatch (relay selects which peer runs inference) but delivers results
inside Turn messages for simple canonicalization.

**Message flow:**
1. Sim emits `OutboundRequest`; gdext drains the outbox
2. gdext sends `ClientMessage::LlmRequest { request_id, preambles, prompt, response_schema, deadline_tick }` to the relay
3. Relay selects an LLM-capable peer and forwards: `ServerMessage::LlmDispatch { request_id, preambles, prompt, response_schema }`
4. Chosen peer runs inference, sends back: `ClientMessage::LlmResponse { request_id, result, metadata }`
5. Relay buffers the result until the next turn flush
6. Turn flush includes LLM results: `ServerMessage::Turn { commands, llm_results: Vec<LlmResult>, ... }`
7. All players apply commands AND `llm_results` at the turn's tick

**Why this approach:**
- **Clean dispatch:** The relay explicitly manages peer selection,
  load-balancing, and failover. LLM capability is a first-class protocol
  concept.
- **Simple canonicalization:** Results arrive inside Turns, so all players
  apply them at the same tick. No separate tick-stamping or buffering mechanism.
  Desync detection covers LLM results automatically (they're part of the Turn).
- **Minimal sim changes:** The Turn already delivers commands; now it also
  delivers LLM results. The sim processes both in the same code path.

**Tradeoff:** Turn-boundary latency — a response ready mid-turn waits for the
next flush (~50ms default). Acceptable given the multi-second latency budget.

**Alternatives considered:**
- *LLM as virtual player (Option A):* LLM responses piggyback on the existing
  command pipeline. Minimal protocol changes, but the relay can't manage
  dispatch — all LLM-capable peers must coordinate externally to avoid duplicate
  work. Rejected because dispatch coordination is a critical feature.
- *Fully dedicated channel (Option B):* LLM results delivered immediately via
  dedicated messages, bypassing turns. Lower latency but requires a separate
  canonicalization mechanism (tick-stamping or buffering) to maintain
  determinism. The complexity isn't justified given our multi-second budget.

#### New Protocol Message Types

```rust
// Client → Relay
pub enum ClientMessage {
    // ... existing variants ...

    /// Submit an LLM inference request. The relay dispatches to a capable peer.
    LlmRequest {
        request_id: u64,
        /// Serialized preambles + prompt + schema. Opaque to the relay —
        /// forwarded verbatim to the dispatched peer.
        payload: Vec<u8>,
    },

    /// Return an LLM inference result (sent by the peer that ran inference).
    LlmResponse {
        request_id: u64,
        /// Serialized result JSON + metadata. Opaque to the relay.
        payload: Vec<u8>,
    },
}

// Relay → Client
pub enum ServerMessage {
    // ... existing variants ...

    /// Dispatch an inference request to this peer (relay chose you to run it).
    LlmDispatch {
        request_id: u64,
        /// The original request payload, forwarded verbatim.
        payload: Vec<u8>,
    },

    Turn {
        turn_number: TurnNumber,
        sim_tick_target: u64,
        commands: Vec<TurnCommand>,
        /// LLM results that completed since the last turn. Applied at the
        /// same tick as the turn's commands. Default empty for backward
        /// compatibility with older clients (`#[serde(default)]`).
        #[serde(default)]
        llm_results: Vec<LlmResult>,
    },
}

pub struct LlmResult {
    pub request_id: u64,
    /// Serialized result JSON + metadata. Deserialized by the sim layer.
    pub payload: Vec<u8>,
}
```

The relay treats LLM payloads as opaque bytes — it never inspects the prompt,
schema, or result. This keeps the relay sim-agnostic, consistent with the
existing command payload design.

**Payload serialization:** LLM payloads use JSON via serde, the same convention
as `TurnCommand` payloads. Two structs define the wire format:

- `LlmRequestPayload { preambles, prompt, response_schema, deadline_tick }` —
  serialized by the sending peer into `LlmRequest.payload`.
- `LlmResponsePayload { result_json: String, metadata: InferenceMetadata }` —
  serialized by the inference peer into `LlmResponse.payload`.

Both structs live in `elven_canopy_protocol` (alongside `TurnCommand`) so that
all peers can serialize/deserialize without depending on `elven_canopy_sim`. The
relay never deserializes these — it forwards `Vec<u8>` verbatim.

#### Relay-Side Dispatch Logic

The relay tracks which peers are LLM-capable (see "Multiplayer Capability
Signaling"). When it receives `ClientMessage::LlmRequest`:

1. Select a capable peer. Initially: prefer the peer with the fewest
   outstanding dispatched requests (simple load balancing). If no capable
   peer is available, drop the request (it will expire at deadline).
2. Forward `ServerMessage::LlmDispatch` to the chosen peer.
3. Track the outstanding request (request_id → dispatched_to_peer).
4. When `ClientMessage::LlmResponse` arrives, buffer it.
5. On next turn flush, include all buffered results in `llm_results`.

If the dispatched peer disconnects before responding, the request is dropped
(deadline expiry handles it). No retry logic in the relay — the sim will
re-trigger naturally if the interaction matters.

**Relay-side data structures:** The `Session` struct gets two new fields:
- `pending_llm_results: Vec<LlmResult>` — buffered results awaiting the next
  turn flush. Drained with `std::mem::take()` in `flush_turn()`, same pattern
  as `pending_commands`. A turn with LLM results but no player commands is
  valid (empty `commands`, non-empty `llm_results`).
- `outstanding_dispatches: BTreeMap<u64, RelayPlayerId>` — maps `request_id` to
  the peer the request was dispatched to. Entries are added on dispatch, removed
  when the response arrives. On peer disconnect, entries for that peer are
  removed (the requests expire at deadline).

**Backpressure:** LLM request payloads are small (~5-10KB of natural language
text). The relay already uses synchronous per-peer writes with no backpressure
management — LLM dispatch doesn't meaningfully worsen this. If needed, prompt
text compresses well (~3-5x with gzip); the relay forwards opaque `Vec<u8>`
payloads and doesn't need to decompress. But this optimization is unlikely to
be needed given the small payload sizes.

#### gdext Turn Processing

When `sim_bridge.poll_network()` receives a `ServerMessage::Turn`, it already
iterates over `commands` and converts each to a `SessionMessage::SimCommand`.
LLM results are processed identically — each `LlmResult` in the Turn is
deserialized and routed through the same session path:

```rust
// In sim_bridge.rs poll_network(), after processing commands:
for llm_result in &turn.llm_results {
    let action = SimAction::LlmResult {
        request_id: llm_result.request_id,
        result_json: /* deserialized from llm_result.payload */,
        metadata: /* deserialized from llm_result.payload */,
    };
    session.process(SessionMessage::LlmResult { action });
}
// Then: session.process(SessionMessage::AdvanceTo { tick: sim_tick_target });
```

LLM results use a new `SessionMessage::LlmResult` variant (not `SimCommand`)
to avoid the player name lookup in `Session::process()` — there is no player
associated with an LLM result. This requires a new match arm in
`GameSession::process()` (`session.rs`) that constructs a `SimCommand` with a
fixed `"[LLM]"` attribution string and routes to `sim.apply_command()`, following
the same pattern as the existing `SessionMessage::SimCommand` arm but without the
player ID lookup.

#### SimAction::LlmResult

```rust
pub enum SimAction {
    // ... existing variants ...

    /// Apply an LLM inference result. Delivered by the relay via Turn.
    LlmResult {
        request_id: u64,
        /// The raw JSON result string (guaranteed valid by grammar constraint).
        result_json: String,
        /// Observability metadata.
        metadata: InferenceMetadata,
    },
}

/// Defined in `elven_canopy_sim` (alongside SimAction). The protocol crate
/// does not need this struct — it treats LLM payloads as opaque bytes.
/// Deserialization happens in gdext when converting Turn llm_results into
/// SimAction::LlmResult values.
pub struct InferenceMetadata {
    pub latency_ms: u32,
    pub token_count: u32,
    pub cache_hit: bool,
    pub prefill_tokens: u32,
    pub decode_tokens: u32,
}
```

#### Sim-Side Response Processing

Processing in `SimState::apply_command()` for `SimAction::LlmResult`:
1. Look up `request_id` in the pending requests table.
2. If not found (expired, cancelled, or duplicate): discard silently.
3. If found: validate `deadline_tick` hasn't passed. If expired: discard.
4. Deserialize `result_json` according to the request's `request_kind` (which
   determines the expected schema — social chat, activity, etc.).
5. If deserialization fails or the result is semantically invalid: discard
   (log for observability).
6. Apply the mechanical effects (opinion change, dialogue storage, activity
   creation, etc.) based on the deserialized result.
7. Remove the request from the pending table; clear the creature's
   pending-request flag.
8. Log the result for observability (if LLM debug logging is enabled).

### LLM Request/Response Wire Format

Requests are semi-structured:
- `request_id: u64` — monotonically increasing, for correlation
- `creature_id: CreatureId` — which creature this decision is for
- `preambles: Vec<PreambleSection>` — ordered preamble sections, each either a
  well-known enum variant (fixed text the inference layer can cache) or a
  literal string (game-state-dependent text). The sim provides the content; the
  inference layer decides whether to cache KV state for any section.
- `prompt: String` — the creature-specific ephemeral context
- `response_schema: String` — JSON schema for grammar-constrained generation
- `deadline_tick: u64` — tick by which the response is needed

Responses contain:
- `request_id: u64` — correlation
- `result: String` — the LLM's JSON output (guaranteed valid by grammar
  constraint)
- `metadata: InferenceMetadata` — `{ latency_ms: u32, token_count: u32,
  cache_hit: bool, prefill_tokens: u32, decode_tokens: u32 }` — for
  observability. Same struct used in `SimAction::LlmResult`.

The envelope is generic enough to serve social chat, activities, diplomacy, and
inner monologue. The `response_schema` field means different features can
request different output structures through the same pipeline.

### Output Schema Design

> **Decision:** Option 3 (minimal enum) for social chat. See "Decision" below.

The `response_schema` field in the wire format specifies the JSON schema that
grammar-constrained generation enforces. Different features need different output
structures. The question is how to organize this.

#### Option 1: Per-feature schemas, opaque to infrastructure

Each feature (social chat, activities, diplomacy) defines its own JSON schema.
The infrastructure treats the schema and the response as opaque strings — it
passes the schema to the grammar engine and returns the raw JSON. The
feature-specific sim code that emitted the request is responsible for
deserializing the response into the appropriate type.

The following examples use informal notation to show the *shape* of each
schema's output. In implementation, `response_schema` would contain actual
JSON Schema documents (with `"type"`, `"enum"`, `"properties"`, etc.) that are
passed to `json_schema_to_grammar()`.

**Social chat (informal output shape):**
```json
{
  "dialogue": "string (1-2 sentences the creature says)",
  "tone": "friendly | neutral | cold | hostile | flirtatious",
  "action": null | {
    "kind": "invite_to_activity | share_gossip | compliment | insult | ask_favor",
    "detail": "string (short description, e.g. 'tonight's dance')"
  }
}
```

**Activity scheduling (informal output shape):**
```json
{
  "organize": true | false,
  "activity_kind": "dance | dinner_party | ceremony",
  "reason": "string (1 sentence, for inner monologue / player display)"
}
```

**Diplomacy (informal output shape):**
```json
{
  "posture": "friendly | neutral | suspicious | hostile",
  "action": null | {
    "kind": "propose_trade | send_envoy | declare_war | offer_alliance",
    "detail": "string"
  },
  "reason": "string (1 sentence)"
}
```

**Pros:** Each feature gets a schema tailored to its needs. Simple to reason
about — the code that emits the request knows exactly what shape the response
will be. New features add new schemas without touching existing ones. Schemas
can be as simple or complex as the feature requires.

**Cons:** The sim needs to know the schema at emit time and the deserialization
type at consume time. Multiple schema definitions to maintain. No shared
structure across features.

#### Option 2: Universal envelope with feature-specific payload

A common outer structure wraps every response, with a feature-specific inner
payload. The outer envelope carries fields that are useful regardless of feature
(free-text, confidence, inner thought).

```json
{
  "text": "string (optional free-text output, displayed to player)",
  "thought": "string (optional inner monologue, goes to thoughts table)",
  "decision": { ... feature-specific payload ... }
}
```

The `decision` field's schema varies by request type. The `text` and `thought`
fields are always available — a social chat uses `text` for dialogue, an
activity decision might only use `decision`, and any request can optionally
produce a `thought`.

**Pros:** Consistent handling of cross-cutting concerns (display text, inner
monologue) without each feature reinventing them. The infrastructure can extract
`text` and `thought` generically; only `decision` needs feature-specific
parsing. Inner monologue (F-llm-monologue) gets "free" integration — any
request can produce a thought as a side effect, even before the monologue
feature is explicitly built.

**Cons:** The universal fields may not make sense for all features (does
diplomacy need `text`? probably not initially). The envelope adds a small amount
of schema overhead. Slightly more complex to define the grammar since the outer
structure is fixed but the inner varies.

#### Option 3: Minimal enum — just pick an action

For the initial features, the LLM's job is simple: pick from a small set of
options. Rather than open-ended JSON, the schema is essentially an enum with
optional parameters.

```json
{
  "choice": "greet_warmly | greet_coldly | ignore | invite_to_dance | share_gossip",
  "say": "string (1 sentence)"
}
```

The set of valid `choice` values is baked into the schema per request. The
grammar ensures the LLM can only pick from the valid options. The `say` field
gives the LLM creative freedom for display text while the `choice` drives
mechanical effects.

**Pros:** Maximally constrained — the model can't hallucinate invalid actions.
Simple to parse. The mechanical effect of each choice is defined in Rust, not
inferred from free-text. Works well with small models that struggle with complex
structured output. Easy to test: verify that each valid choice maps to the
correct sim effect.

**Cons:** Less expressive. Adding new choices requires updating both the schema
and the Rust handler. The `choice` set must be curated per request type. Doesn't
scale well if we want richer decisions later (but can always be upgraded to
Option 1 or 2).

#### Decision

**Option 3 (minimal enum) for social chat.** The model picks a social action
from a curated list and generates a sentence of dialogue. The sim maps the
choice to mechanical effects (opinion change, activity invitation, etc.) using
the same code that handles the non-LLM path. This is the safest design for
small models — it minimizes the semantic burden on the model and keeps the sim
in full control of what actions are actually valid.

**Choice-to-operation mapping (social chat):** Each `choice` value maps to a
concrete sim operation. The initial set for social chat:

| `choice` value     | Sim operation                                                                 |
|--------------------|-------------------------------------------------------------------------------|
| `greet_warmly`     | No bonus effect beyond baseline mechanical resolution.                        |
| `greet_coldly`     | No bonus effect beyond baseline mechanical resolution.                        |
| `ignore`           | No bonus effect; dialogue text is empty or an inner thought.                  |
| `invite_to_dance`  | Sets a `wants_to_organize_dance` flag on the initiator. The creature is mid-conversation (Conversing task) when the LLM result arrives, so calling `try_organize_spontaneous_dance()` directly would fail its idle check. Instead, the flag is picked up on the creature's next idle activation after the Conversing task ends, at which point the normal eligibility path runs (venue, cooldown, organizer checks). On failure, no effect — the invitation is a social gesture that didn't pan out. |
| `share_gossip`     | Creates a thought on the target creature (e.g., "Heard gossip from X").       |

The `SocialChatChoice` Rust enum (see "Schema construction and handler sync")
is the single source of truth for this mapping. Each variant's handler is a
`match` arm in the LLM result processing code — adding a new choice requires
adding both the enum variant and its handler, enforced by exhaustive match.

If we need more expressiveness later, the migration to per-feature schemas
(Option 1) is straightforward: the `response_schema` field already supports
arbitrary schemas, so upgrading from a constrained enum to a richer schema is
just a schema change, not a pipeline change. Option 2 (universal envelope)
remains worth considering when F-llm-monologue comes online, for the "free"
`thought` field on every request.

### Model Download

Not bundled with the game — too large for players who won't use it.

- On first launch (or via settings), prompt: *"Enable AI creature personalities?
  (requires ~1.5 GB download)"*
- Download in background; game is fully playable without it
- Store in platform-conventional data directory alongside saves/config
  (`~/.local/share/elven_canopy/models/`, `%APPDATA%/ElvenCanopy/models/`, etc.)
- Manifest file (model name, expected SHA256, download URL, quantization level)
  for checksum verification and version management
- If a game update requires a different model, detect stale model via manifest
  and re-download
- HTTP client, resume support — details deferred to implementation
- Hosting: Qwen 3 is Apache 2.0, so redistribution is legal. Options: link
  directly to HuggingFace (free, but depends on third-party uptime), or
  self-host on project CDN (reliable, but ~1.3GB per download adds up at
  scale). Decision deferred to implementation.

### Multiplayer Capability Signaling

When a player joins a session, they signal whether they have the LLM model
downloaded and ready. The relay uses this to decide which player(s) to dispatch
inference requests to.

- If multiple players have the model, the relay can load-balance across them
  (prefer the peer with fewest outstanding requests).
- If the only LLM-capable player disconnects, inference gracefully fails and
  creatures revert to rules-based behavior (the standard fallback — existing
  task and social systems handle everything).
- A multiplayer game gets LLM features as long as *any* player has the model.

Capability is signaled via a new field in the `ClientMessage::Hello` enum
variant (an inline variant, not a named struct — matching the existing code in
`message.rs`):

```rust
pub enum ClientMessage {
    Hello {
        // ... existing fields (protocol_version, session_id, etc.) ...
        /// Whether this player has the LLM model downloaded and ready for
        /// inference. The relay uses this to decide dispatch targets.
        /// `#[serde(default)]` for backward compatibility — older clients
        /// that omit this field deserialize as `false`.
        #[serde(default)]
        llm_capable: bool,
    },
    // ...
}
```

The relay stores this per-player in the session's `PlayerState`. When a player's
capability changes mid-session (e.g., model finishes downloading), they send a
new `ClientMessage::LlmCapabilityChanged { capable: bool }`. The relay's
`handle_message` updates the player's stored capability flag.

Edge case: if the last LLM-capable peer becomes incapable (or disconnects) while
requests are outstanding, those requests are simply not fulfilled — they expire
at their deadline. No special cancellation logic needed. The sim treats it
identically to "inference was slow and timed out."

### Observability and Debugging

When something goes wrong (nonsensical output, performance degradation, prompt
issues), debugging needs visibility into the pipeline:

- Log prompts, raw responses, latency, cache hits/misses, token counts
- Track inference failures and retry/timeout counts
- Per-creature decision history (what was requested, what came back, how it
  was applied)
- This logging should be toggleable (off by default, verbose when debugging)
- Metrics exposed to the game UI (debug overlay): inference queue depth,
  average latency, cache hit rate, failures/minute

### Decision Scheduling

When does the sim emit an LLM request for a creature?

**Initially: event-triggered only.** The sim emits an LLM request when a social
interaction is triggered (the existing casual social heartbeat PPM roll fires).
Instead of resolving the interaction purely mechanically, the sim emits an
`OutboundRequest` and the interaction is held pending until the response arrives
or the deadline expires.

Future features (F-llm-activities, F-llm-monologue) may add a periodic
heartbeat-based trigger where creatures periodically ask the LLM "what do I
feel like doing?" But this is out of scope for the initial infrastructure.

**One request at a time per creature.** The sim must not emit a new LLM request
for a creature that already has one in flight. This is tracked in the sim state
(a set of creature IDs with pending requests, or a field on the creature row).
If a new trigger fires while a request is pending, the trigger is resolved
mechanically (the existing non-LLM path) rather than queued.

**Tradeoff:** When F-llm-activities adds heartbeat-based inclination requests,
socially active creatures may rarely get inclination updates (their slot is
occupied by social chat). If this becomes a problem, the fix is to allow
independent request slots per request type (one social chat + one inclination
in flight simultaneously). But for the initial social-chat-only scope, a single
slot is simpler and sufficient.

### Request Lifecycle and Cancellation

In-flight LLM requests can be affected by game events:

- **Creature dies:** Cancel the request. The sim marks the request as cancelled;
  if a response arrives later, it is discarded (the creature no longer exists).
- **Creature enters combat / flees:** Do NOT cancel. The latency is ~1 second;
  the social interaction that triggered the request is still valid and should
  still resolve. The creature might be mid-conversation when attacked — the
  conversation result still applies. (For future inclination-change requests,
  the same logic applies: the creature's "what do I feel like doing?" thought
  is still valid even if they're temporarily interrupted.)
- **Save/load:** The `pending_llm_requests` table is part of serialized sim
  state (the outbox itself is a transient `Vec` drained after each `step()`).
  On load, the sim regenerates outbox entries from pending requests whose
  deadline hasn't passed — this re-gathers creature context and rebuilds the
  prompt from current state, which may differ slightly from the original
  emission but is acceptable since LLM output is a soft nudge. The hosting
  layer drains these regenerated entries and re-submits them. Since deadlines
  are in sim ticks (not wall clock) and the sim doesn't advance during
  save/load, requests don't expire spuriously. This avoids silently losing
  social interactions that were triggered by a PPM roll that won't re-fire.
- **Deadline expiry:** The sim checks `deadline_tick` when a response arrives.
  If the deadline has passed, the response is silently discarded. The creature's
  pending-request flag is cleared, allowing new requests.

### Deadline Tuning

The `deadline_tick` on each `OutboundRequest` determines how long the sim waits
for a response before giving up. This is a key tuning parameter: too short and
most requests expire before inference completes (especially CPU inference at
~1-5s); too long and creatures stand idle in Conversing tasks for extended
periods when the LLM is unavailable.

**Deadline calculation:** `current_tick + config.llm.deadline_ticks`, where
`deadline_ticks` is a `GameConfig` parameter. Reasonable default: **300-600
ticks** (5-10 seconds at 60 TPS), covering single-turn inference latency with
margin.

**Conversing task `expires_tick`:** For a multi-turn social exchange, the
Conversing task's `expires_tick` should be the *last* possible response deadline
in the exchange, not the first. Concretely: `current_tick +
config.llm.conversation_timeout_ticks`, where `conversation_timeout_ticks` is
larger than `deadline_ticks` — e.g., **600-900 ticks** (10-15 seconds) to
cover a 2-3 turn exchange with relay round-trip overhead. If all turns complete
early, the Conversing task ends immediately; the timeout is only a safety valve.

**Idle time tradeoff:** At 60 TPS, 10 seconds is 600 ticks of a creature
standing in a Conversing task instead of doing useful work. This is acceptable
for occasional social interactions but would be problematic at high frequency.
The existing one-request-at-a-time constraint and PPM-based trigger rate
naturally limit how often a creature enters this state.

### Model-Not-Downloaded Behavior

When the model is not downloaded, the sim still emits `OutboundRequest`s into
the outbox. The hosting layer (gdext) drains them and does nothing — no relay
routing, no inference. The requests hit their deadline and expire silently. The
sim's behavior is identical to "LLM is available but every request times out."

This means:
- The sim code has no conditional compilation around LLM. The outbox, request
  emission, response handling, and deadline expiry all exist unconditionally.
- There are no `#[cfg]` boundaries anywhere in the codebase related to LLM.
  `elven_canopy_llm` is always compiled. GPU backend selection (`cuda`,
  `vulkan`, `rocm`) is the only feature-flag axis.
- Unit tests for the sim can test prompt construction and response handling
  without any LLM dependency — they just verify the outbox contents and feed
  synthetic responses back in.

### Testing Strategy

**Sim-level tests (no LLM required):**
- **Prompt construction:** Trigger a social interaction, drain the outbox,
  verify the `OutboundRequest` contains the expected prompt text (creature
  names, opinions, mood, situation description). These are pure sim tests.
- **Response handling:** Feed a synthetic JSON response (matching the expected
  schema) into the sim as a canonical input. Verify the sim applies the correct
  mechanical effects (opinion changes, activity creation, dialogue storage).
- **Malformed response rejection:** Feed responses that are syntactically valid
  JSON but semantically invalid (unknown action enum value, references a
  nonexistent creature, etc.). Verify the sim rejects them gracefully.
- **Deadline expiry:** Emit a request, advance the sim past the deadline without
  providing a response. Verify the creature's pending flag clears and the
  creature proceeds with normal behavior.
- **Cancellation on death:** Emit a request, kill the creature, feed a response.
  Verify the response is discarded.
- **One-at-a-time enforcement:** Trigger two social interactions for the same
  creature in quick succession. Verify only one `OutboundRequest` is emitted;
  the second interaction resolves mechanically.

No actual LLM runs in any sim test. The TDD workflow is: write test that checks
outbox/response behavior → make it pass with sim code → repeat.

**LLM crate tests (not in CI):**
- Integration tests that load a real GGUF model, run inference with a test
  prompt and grammar, and verify the output is valid JSON matching the schema.
  These are slow, require the model on disk, and are for manual validation
  during development — not part of the CI pipeline.

## Architecture

### Determinism

LLM outputs are non-deterministic across hardware/runtimes, but this is fine
because they're treated as **external canonical inputs**, structurally identical
to player commands.

- At tick `n`, the sim packages a context snapshot into an `OutboundRequest` and
  adds it to the outbox.
- The hosting layer (gdext) drains the outbox and routes the request through the
  relay to an LLM-capable peer.
- The sim continues processing ticks normally. It does not block.
- The LLM result arrives via the relay (as part of a Turn or as a dedicated
  message) and enters the sim as a canonical input.
- All instances in multiplayer apply the same result at the same tick.
- If the result doesn't arrive by `deadline_tick`, the request expires silently.
  The creature keeps its current inclinations. No error path, no retry — the
  existing task system handles everything.

This is the same pattern you'd use for any async oracle. The sim doesn't care
*how* the decision was made.

### Relay as the ONLY Path (CRITICAL)

**ALL LLM inference — singleplayer and multiplayer alike — goes through the
relay.** There is NO special singleplayer shortcut. The relay is the sole
mechanism by which LLM requests are dispatched and results are canonicalized.

This is non-negotiable. A singleplayer-only code path WILL introduce bugs that
only surface in multiplayer, which is the hardest place to debug them. The relay
path must be exercised in every mode, always.

In singleplayer, the relay runs on localhost (see B-local-relay), dispatches the
inference request to the local machine, and returns the result through the same
canonical pipeline as multiplayer. The code that submits an LLM request and the
code that consumes the result must be identical regardless of player count.

### Creature Activation Integration Points

The existing creature activation cascade (`activation.rs`) has clean insertion
points for LLM-driven decisions:

1. **Task preference weights** — `find_available_task()` currently picks the
   closest available task by travel cost. LLM inclinations could add weighted
   scoring: "I feel like crafting" biases toward Craft tasks, "I'd rather be
   social" biases toward DineAtHall or activity volunteering. The task system
   remains the final authority — inclinations are soft preferences, not commands.

2. **Wander/idle state** — When creatures have no task, they wander randomly on
   the nav graph. This is where LLM-driven "I want to do X" impulses would
   fire: seek out a friend, go to the dance hall, wander to a scenic spot. The
   LLM produces an intent; the activation system translates it into a concrete
   task if possible.

3. **Spontaneous organization** — Dance and dinner party organization currently
   uses probability rolls + cooldowns. LLM could replace the probability roll
   with a richer decision: mood, recent social interactions, relationships with
   nearby elves, personality traits all factor in.

4. **Social interactions** — Casual social triggers during heartbeats. In
   F-llm-social-chat, LLM-generated dialogue replaces purely mechanical
   resolution. The LLM produces greeting text; the mechanical skill check still
   happens; the result goes into the other creature's inbox.

5. **Activity volunteering** — `find_open_activity_for_creature()` currently
   discovers activities and volunteers if eligible. LLM inclinations could
   weight activities by personality: shy elves avoid large groups, extroverts
   seek them out.

All of these insertion points consume LLM output as *preferences*, not commands.
The existing systems remain the final authority on feasibility, timing, and
execution. The LLM can never put a creature in an invalid state.

### Context Available for Prompts

The sim database provides rich context for LLM prompts:

- `mood_for_creature()` → mood score and MoodTier (7 tiers from Devastated to
  Elated)
- `db.thoughts.by_creature_id()` → recent thoughts with timestamps
- `db.creature_opinions.by_creature_id()` → asymmetric opinions (Friendliness,
  Respect, Fear, Attraction) toward other creatures
- `db.creatures.get(id)` → food, rest, position, current task/activity
- `SimState::trait_int()` / `trait_level()` → ability scores (8 stats:
  STR/DEX/CON/INT/WIL/CHA/PER/LCK), skills (17 skill TraitKind variants),
  personality (Big Five when wired through from genome)
- `db.item_stacks.by_inventory_id()` → what creature owns/carries
- `creature.military_group` → group membership and engagement style
- `creature.path` → Outcast/Warrior/Scout life path
- `creature.sex` → Male/Female/None

This context is gathered by the sim when it emits an `OutboundRequest`. The
prompt is constructed in the sim (it has access to all the data), serialized
into the request, and sent to the inference engine which treats it as opaque
text.

**Prompt construction infrastructure:** Turning structured sim data into
readable prompt text is a significant sub-task of F-llm-creatures. This needs
a prompt builder module in the sim crate — shared utilities for rendering
creature context (mood, relationships, recent thoughts) into natural language
text, Vaelith name formatting, preamble assembly, and schema construction. Each
feature (social chat, activities, diplomacy) will add its own prompt templates,
but the shared infrastructure (context rendering, preamble management, schema
generation) belongs in F-llm-creatures.

**Schema construction and handler sync:** The `response_schema` field contains
JSON Schema documents whose valid `choice` values are context-dependent (e.g.,
the set of social actions depends on the creature's state). To prevent
schema/handler drift — where the schema allows a choice that the Rust handler
doesn't recognize, or vice versa — the valid choice set should be defined as a
Rust enum (e.g., `SocialChatChoice`) with a method that generates the JSON
Schema `enum` array. The same enum is used in the `match` arm that handles the
deserialized response. This ensures the schema and the handler are derived from
the same source of truth. The prompt builder module owns these enum-to-schema
helpers.

**Naming in prompts:** Creatures have Vaelith (elvish) names with meanings
(e.g., "Thandril" meaning "wise-shadow"). A 1.7B model may not handle fantasy
names well — they're out-of-distribution for most training data. Options:
use Vaelith names with meaning annotations ("Thandril (wise-shadow)"), use
meanings only ("Wise-Shadow"), or use simple placeholder labels ("your friend",
"the warrior nearby"). Needs experimentation. The prompt construction code
should make this easy to swap.

## Social Conversation Model

When the existing system triggers a casual social interaction — via the
heartbeat-driven PPM roll in `try_casual_social()`, which fires independently
of what either creature is doing — it can delegate to the LLM pipeline instead
of resolving purely mechanically:

1. Heartbeat PPM roll fires for Elfandriel, selecting nearby Thandril as the
   interaction target (same as today).
2. **Mechanical effects apply immediately**, exactly as `try_casual_social()` does
   today: the social skill check fires, opinion changes are applied, Friendliness/
   Respect/Fear/Attraction modifiers are resolved. This happens synchronously in
   the same tick, regardless of LLM availability.
3. Simultaneously, an LLM request is emitted for Elfandriel: "you're about to
   pass Thandril, your friend (opinion: 65). What do you say?" When the response
   arrives, it produces dialogue text and a `choice` (e.g., `invite_to_dance`).
   The dialogue is stored for display; the `choice` may trigger **bonus** effects
   (e.g., creating a dance invitation) that go beyond the baseline mechanical
   resolution. If the LLM request times out, the interaction is complete with
   just the mechanical effects — no degradation.
4. The greeting text + skill check result goes into Thandril's **inbox**.
5. Thandril's LLM processes inbox on its next cycle: sees the greeting, the
   skill check outcome, and its own context. Produces a response and possibly
   a follow-up action (e.g. "invite Elfandriel to tonight's dance").
6. Response goes back to Elfandriel's inbox, plus mechanical effects (dance
   invitation creates a pending activity).

The multi-turn exchange takes several LLM cycles (~5-10 seconds total), which
maps naturally to two creatures stopping, chatting, and moving on.

**Creature anchoring during conversations:** Both creatures must be held in place
for the duration of the exchange. Without this, the normal activation cascade
(`process_creature_activation()`) would reassign them to other tasks mid-
conversation. The mechanism: when a social interaction is delegated to LLM, **both
creatures enter Conversing immediately** in the heartbeat handler that triggers
the interaction — not deferred to inbox processing. The initiator's LLM request
is emitted simultaneously. The target's Conversing task ensures they are anchored
when their inbox is processed on the next activation; because they are already in
a Conversing task, no race condition can cause them to pick up a higher-priority
task between initiation and inbox processing.

**Cross-creature task assignment:** Assigning Conversing to the target creature
from the initiator's heartbeat handler is a cross-creature task assignment — a
pattern not used in the current activation pipeline, where task transitions are
always handled by the creature's own activation. The closest analogue is the
group activity system's `try_assign_to_activity()` (activity.rs), which modifies
another creature's state from a shared code path. The implementation should
follow that pattern: direct `db.creatures.modify()` on the target, going through
the same preemption checks that `try_assign_to_activity()` uses.

Both creatures enter a `TaskKind::Conversing { with: CreatureId, expires_tick: u64 }`
task. This is at `PreemptionLevel::Autonomous` (level 1) — a conversation is
low-priority background activity. "Blocks task reassignment" means blocks
*same-or-lower-priority* reassignment; higher-priority tasks (eating, sleeping,
combat, fleeing) will preempt the conversation normally, ending it early. The
`expires_tick` is a safety valve (the last possible response deadline in the
multi-turn exchange, plus margin — see "Deadline Tuning" below) so creatures are
never stuck permanently if the exchange fails partway through. If one creature
dies or enters combat during the conversation, the other's Conversing task is
cancelled and they resume normal activation. **Cancellation mechanism:** on each
Conversing activation, the creature checks whether its partner is still alive
and still in a Conversing task targeting it; if not, the task completes
immediately. This is consistent with the existing activation model (no
cross-creature task cancellation needed) and responds within one tick. This is
analogous to how group
activities hold creatures in an assembly/execution phase — the Conversing task is
lighter-weight but follows the same "block reassignment until done" pattern.

**Eligibility check for entering a conversation:** Because Conversing is
Autonomous, it can only preempt Idle or other Autonomous-level tasks. Before
emitting an LLM social request, the sim checks that *both* creatures are
currently idle or in an Autonomous-level task (and thus can be preempted to
Conversing). If either creature is at Survival level or higher (eating, sleeping,
combat, fleeing, moping), the interaction is resolved mechanically as today (no
LLM). This keeps the LLM path as a flavor enhancement that fires only when
conditions are right, rather than requiring changes to the preemption system.

**Adding `Conversing` to the task system:** `TaskKind::Conversing` requires
additions to four exhaustive match sites, all enforced by compile errors:

1. **`TaskKindTag` enum** (`db.rs`) — new `Conversing` discriminant.
2. **`TaskKindTag::from_kind()`** (`db.rs`) — map `TaskKind::Conversing` to the tag.
3. **`preemption_level()`** (`preemption.rs`) — map `Conversing` to `PreemptionLevel::Autonomous` for all origins.
4. **`TaskKindTag::display_name()`** (`db.rs`) — return `"Chatting"`.
5. **`requires_mana_correct_for_all_task_kinds` test** (`mana_tests.rs`) — add `assert!(!TaskKindTag::Conversing.requires_mana())`. The `requires_mana()` method uses `matches!()` so it won't cause a compile error, but this exhaustive test will fail if the new variant isn't covered.

`TaskKind::Conversing` is a new serde variant. This follows the existing pattern where new task kinds are forward-incompatible — older game versions cannot load saves containing unknown task kinds. No special handling is needed.

**Duration model:** Conversing uses an `expires_tick` field rather than the
standard `total_cost`/`progress` pattern used by Sleep, Mope, and Graze. This is
because conversation duration is externally determined (by LLM response timing)
rather than a fixed tick count known at task creation. The activation cascade
needs a code path to check `expires_tick` against the current tick — on expiry,
the task completes and the creature resumes normal activation.

### Inbox Design

The inbox is a per-creature queue of messages from other creatures, waiting to be
incorporated into the creature's next LLM prompt. It is separate from the outbox
(which carries requests *to* the LLM) — the inbox carries creature-to-creature
messages that result *from* LLM responses.

**Data structure:** A tabulosity table `creature_messages` with columns:

```rust
pub struct CreatureMessage {
    /// Assigned from `next_message_id: u64` counter on `SimState`, following the
    /// existing pattern for `next_structure_id`. Monotonic counter chosen for
    /// simplicity; PRNG-derived UUIDs (as used by `TaskId`, `ProjectId`) would
    /// also work since PRNG state is deterministic at the point of application.
    pub message_id: u64,
    pub recipient_creature_id: CreatureId,
    pub sender_creature_id: CreatureId,
    pub text: String,
    pub tone: String,     // e.g., "friendly", "cold" — from LLM output
    pub tick_created: u64,
    pub processed: bool,  // false = inbox, true = history
}
```

Indexed by `recipient_creature_id` for inbox queries, and by `sender_creature_id`
for conversation history lookups.

**How messages enter the inbox:** When the sim processes a `SimAction::LlmResult`
for a social chat request, the LLM output contains dialogue text and a tone. The
sim writes a `CreatureMessage` row with `processed: false` targeting the other
creature in the interaction.

**How inbox triggers LLM requests:** When a creature has unprocessed inbox
messages, its next activation should emit an LLM request (bypassing the normal
PPM roll for social interactions). Concretely: early in the creature's activation
check, if unprocessed inbox messages exist, no LLM request is already in flight,
and the creature is idle or in an Autonomous-level task (same eligibility check
as conversation entry — a creature mid-combat or mid-sleep doesn't stop to
reply), emit an LLM request whose prompt includes the inbox contents. If the
creature is busy with higher-priority work, the inbox message waits — the TTL
garbage collection handles the case where it is never consumed. This ensures
responsive multi-turn conversations when creatures are available, rather than
waiting for a probabilistic social trigger.

**Interaction with one-request-at-a-time constraint:** If the creature already
has a pending LLM request when a message arrives, the inbox message waits until
the current request completes and the creature's next activation fires.

**Garbage collection:** Processed messages are retained for conversation history
display. Unprocessed messages that are older than a configurable TTL (e.g., 1
in-game day) are garbage collected by a heartbeat — if the recipient never
processed them, the conversation simply didn't happen. This handles the partial-
availability case where LLM goes down after one side of a conversation completes.

### Message Storage

Messages are small (a sentence or two each) and stored in the `creature_messages`
table. Viewable in the creature detail panel as a conversation log. Garbage
collected over time — keep the last N per creature or everything from the last
in-game week, whichever is more.

## Inner Monologue (speculative, deferred)

The LLM could produce "inner monologue" text as an optional output alongside
its decisions. Part of the prompt would include a summary of past inner
monologues, creating a feedback loop where creatures develop emergent
personalities.

The Big Five personality traits (from genetics) set the *interpretive lens*:
high neuroticism in the prompt means the LLM actually reads negative intent
into ambiguous situations. **Dependency note:** The genome system has Big Five
personality SNP regions and `express_personality()` functions, but these are not
yet exposed through the creature info APIs or wired into prompts. F-llm-social-
chat can work without personality traits (use stats + path as personality
proxy); F-llm-monologue requires them. A creature might receive a compliment with a low
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
- **Throughput (worst case):** ~100 creatures × 1/minute ≈ 1-2 calls/sec
  average. In practice lower — many elves will be sleeping, in combat, or
  otherwise not triggering social interactions. Stagger decision timers with
  jitter to smooth bursts.
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
- ~50 tokens: output budget (enforced via `n_predict` / max tokens parameter on
  inference requests to prevent unbounded generation in free-text fields like
  `say`, which could cause deadline expiry spikes)

## Prompt Caching (KV Cache Reuse)

llama.cpp supports saving and restoring KV cache state, confirmed via the
`llama-cpp-2` crate's `context::session` module. The prompt should be structured
in cleanly separated tiers to exploit this:

1. **Base preamble** — game rules, output format, decision categories. Processed
   once at model load, cached permanently. One copy; size depends on model
   architecture (rough estimate ~40-100MB for ~200 tokens on a 1.7B model — needs
   benchmarking with the actual model). Always cached.

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

## Open Questions

- **Output schema evolution:** Starting with minimal enum (Option 3) for social
  chat. May graduate to per-feature schemas (Option 1) if more expressiveness
  is needed. Needs validation with actual model output.
- **Prompt engineering:** Grammar-constrained generation eliminates format
  failures, but semantic quality with 1.7B models needs experimentation. How
  much context is too much? Do few-shot examples help at this model size, or do
  they eat too much of the token budget?
- **Vaelith names in prompts:** Fantasy names may confuse small models. Need to
  experiment with name+meaning annotations vs meanings-only vs generic labels.
- **Batching tradeoffs:** Does batching multiple creatures improve quality
  (coordination) or hurt it (omniscience)?
- **Backend benchmarking:** Vulkan vs CPU for 1.7B models at our workload.
  Need actual numbers before committing to a default backend.
- **Distribution mechanics:** HuggingFace link vs self-hosted CDN, download
  resumption, model versioning across game updates.
- **Thought system integration:** Leaning toward using the existing thoughts
  table rather than a separate system, but not yet decided.
- **LLM crate threading:** Thread pool vs single worker thread for inference.
  Depends on what's ergonomic for `llama-cpp-2`'s context model.
- **Big Five wiring:** Genome has personality SNP regions and
  `express_personality()`, but these aren't yet exposed through creature info
  APIs. F-llm-social-chat can use stats+path as a personality proxy;
  F-llm-monologue will need full Big Five access.

## Resolved Questions

- **Relay message routing:** Option C (hybrid — dedicated dispatch, turn-based
  delivery). Clean dispatch with simple canonicalization. (See "LLM Message
  Routing in the Relay.")
- **Response return path:** `SimAction::LlmResult` variant processed in
  `apply_command()`, matched by `request_id` against pending requests table.
  (See "Sim-Side Response Processing.")
- **Preamble in outbox:** The sim provides preamble content (either a well-known
  string key or literal text), not caching hints. The inference layer decides
  whether to cache. (See `PreambleSection` enum.)
- **Pending requests:** `BTreeMap<u64, PendingLlmRequest>` on SimState, with
  `LlmRequestKind` enum for feature-specific context. BTreeMap for deterministic
  ordering. (See "Sim Outbox.")
- **Turn serde compatibility:** New `llm_results` field uses `#[serde(default)]`
  for backward compatibility with older clients.
- **gdext processing flow:** LLM results in Turns are converted to
  `SimAction::LlmResult` and processed via `SessionMessage::LlmResult` (new
  variant with new match arm in `GameSession::process()`, avoids player name
  lookup, uses `"[LLM]"` attribution). (See "gdext Turn Processing.")
- **InferenceMetadata placement:** Defined in `elven_canopy_sim` alongside
  `SimAction`. Protocol crate treats LLM payloads as opaque bytes.
- **Relay LLM buffer:** `pending_llm_results: Vec<LlmResult>` on Session,
  drained by `flush_turn()`. `outstanding_dispatches: BTreeMap<u64,
  RelayPlayerId>` for tracking dispatched requests.
- **Save/load:** The pending requests table (not the transient outbox) is
  serialized sim state. On load, the sim regenerates outbox entries from pending
  requests and the hosting layer re-submits them. (See "Request Lifecycle.")
- **Capability signaling:** New `llm_capable: bool` field in `Hello` enum
  variant (inline, with `#[serde(default)]`), plus `LlmCapabilityChanged`
  message for mid-session updates. (See "Multiplayer Capability Signaling.")
- **VRAM budget:** Query available VRAM at startup; fall back to CPU if
  insufficient. Expose Auto/GPU/CPU setting. (See "GPU Backend Selection.")
- **Testing:** Sim tests verify prompt construction and response handling with
  synthetic data — no LLM in CI. (See "Testing Strategy.")
- **Feature flag boundaries:** Sim always emits outbox requests; they go nowhere
  when LLM is unavailable and expire at deadline. No conditional compilation in
  the sim crate. (See "Feature Flag Boundaries.")
- **Decision scheduling:** Event-triggered initially (social interaction). One
  request at a time per creature. (See "Decision Scheduling.")
- **Request cancellation:** Cancel on death only. Combat/flee do not cancel —
  latency is ~1s and the triggering event is still valid. (See "Request
  Lifecycle and Cancellation.")
- **Conversing preemption level:** Autonomous (level 1). Conversations yield to
  survival, mood, and combat tasks. "Block reassignment" means same-or-lower
  priority only. (See "Creature anchoring during conversations.")
- **Conversation eligibility:** Both creatures must be idle or Autonomous before
  entering a Conversing task. If either is at Survival or higher, resolve
  mechanically. Same check for inbox-triggered LLM requests. (See "Eligibility
  check for entering a conversation.")
- **Deadline tuning:** `config.llm.deadline_ticks` (300-600 default) for single
  requests; `config.llm.conversation_timeout_ticks` (600-900 default) for
  Conversing task expiry. (See "Deadline Tuning.")

## Changelog

### v2

- Added creature anchoring mechanism (`TaskKind::Conversing`) to hold creatures in place during multi-turn LLM conversations, with cancellation on death/combat and expiry safety valve. (Review #4, HIGH #1)
- Added Inbox Design subsection with `creature_messages` tabulosity table schema, message lifecycle (entry, trigger, GC), interaction with one-request-at-a-time constraint, and activation-triggered inbox processing that bypasses PPM rolls. (Review #4, HIGH #2 and LOW #8)
- Corrected GPU backend selection: clarified that backends are compile-time Cargo feature flags (not runtime-detected), with per-platform build defaults (Vulkan on Linux/Windows, Metal on macOS) and runtime GPU-vs-CPU choice. (Review #4, MEDIUM #3)
- Clarified mechanical-vs-LLM resolution timing: mechanical effects (skill checks, opinion changes) apply immediately as today; LLM output adds dialogue text and optional bonus effects asynchronously. Partial LLM availability handled by inbox GC. (Review #4, MEDIUM #4)
- Added schema construction and handler sync design: valid choice sets defined as Rust enums with JSON Schema generation methods, ensuring schema and handler share a single source of truth. (Review #4, MEDIUM #5)
- Qualified KV cache size estimate with back-of-envelope range and "needs benchmarking" note. (Review #4, LOW #6)
- Added `n_predict` / max tokens cap mention in prompt budget to prevent unbounded generation. (Review #4, LOW #7)

### v3

- Specified `Conversing` preemption level as Autonomous (level 1), clarified "blocks reassignment" means same-or-lower priority only, and listed the four exhaustive match sites that need updating. (Review #5, HIGH #1)
- Added eligibility check: both creatures must be idle or Autonomous-level before entering a Conversing task; otherwise resolve mechanically. (Review #5, HIGH #2)
- Corrected social interaction trigger description: heartbeat PPM roll, not "two elves passing each other." (Review #5, HIGH #2)
- Added preemption eligibility check to inbox-driven LLM requests: creature must be idle or Autonomous, otherwise inbox waits. (Review #5, MEDIUM #3)
- Corrected `Hello` pseudo-code to show inline enum variant (matching actual `message.rs`) with `#[serde(default)]` for backward compatibility. (Review #5, MEDIUM #4)
- Added "Deadline Tuning" subsection specifying `config.llm.deadline_ticks` (300-600 default), Conversing task `expires_tick` relationship, and idle-time tradeoff. (Review #5, MEDIUM #5)
- Specified `message_id` source as monotonic `next_message_id` counter on `SimState`. (Review #5, LOW #6)
- Acknowledged `expires_tick` divergence from `total_cost`/`progress` pattern, with rationale. (Review #5, LOW #7)

### v4

- Corrected `message_id` analogy: references `next_structure_id` (the actual monotonic counter) instead of incorrectly claiming `TaskId`/`ProjectId` use monotonic counters. Same fix applied to `request_id` description. (Review #6, MEDIUM #1)
- Added fifth exhaustive match site for `TaskKind::Conversing`: `requires_mana_correct_for_all_task_kinds` test in `mana_tests.rs`. (Review #6, MEDIUM #2)
- Specified conversation cancellation mechanism: on each Conversing activation, check partner is still alive and still in a reciprocal Conversing task; if not, complete immediately. (Review #6, MEDIUM #3)
- Clarified save/load persistence: the `pending_llm_requests` table persists (not the transient outbox); on load the sim regenerates outbox entries from pending requests with rebuilt prompts. (Review #6, LOW #4)
- Added serde forward-compatibility note for `TaskKind::Conversing` variant. (Review #6, LOW #5)
- Added `#[derive(Serialize, Deserialize)]` to `LlmRequestKind` pseudocode for consistency. (Review #6, LOW #6)

### v5

- Added choice-to-sim-operation mapping table for social chat `SocialChatChoice` enum, specifying the concrete sim operation for each `choice` value (e.g., `invite_to_dance` defers to `try_organize_spontaneous_dance()`). (Review #7, MEDIUM #1)
- Specified that both creatures enter Conversing immediately in the heartbeat handler, not deferred to inbox processing, eliminating the race condition where the target could pick up a higher-priority task before processing their inbox. (Review #7, MEDIUM #2)
- Specified LLM payload serialization format: JSON via serde (same convention as `TurnCommand`), with `LlmRequestPayload` and `LlmResponsePayload` structs defined in `elven_canopy_protocol`. (Review #7, MEDIUM #3)
- Clarified that `SessionMessage::LlmResult` requires a new match arm in `GameSession::process()` (session.rs) with `"[LLM]"` attribution, following the `SimCommand` arm pattern. (Review #7, LOW #4)
- Added `max_tokens: u32` field to `OutboundRequest::LlmInference` for per-request output budget control. (Review #7, LOW #5)

### v6

- Fixed `invite_to_dance` choice handler: corrected function name to `try_organize_spontaneous_dance()` and specified deferred-organization approach (`wants_to_organize_dance` flag checked on next idle activation) to avoid the idle-check conflict with the active Conversing task. (Review #8, MEDIUM #1)
- Added cross-creature task assignment note for Conversing: acknowledged that assigning a task to the target from the initiator's heartbeat is a new pattern, referencing `try_assign_to_activity()` as the existing analogue. (Review #8, MEDIUM #2)
- Fixed duplicate step numbering in conversation flow (two steps labeled "4" → renumbered to 4, 5, 6). (Review #8, LOW #3)
