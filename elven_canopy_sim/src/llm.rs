// LLM outbox types for the sim → relay → inference → relay → sim pipeline.
//
// The sim emits `OutboundRequest`s when it needs external resolution (currently
// LLM inference only; the enum is extensible). The hosting layer (gdext) drains
// the outbox after each `step()` call, routes requests through the relay to an
// LLM-capable peer, and feeds results back as `SimAction::LlmResult` commands.
//
// The sim never blocks on inference. If a response arrives before the deadline,
// it's applied; otherwise the request silently expires and the creature keeps
// its current inclinations.
//
// See also: `command.rs` for `SimAction::LlmResult`, `sim/mod.rs` for the
// outbox fields on `SimState` and drain logic in `StepResult`.

use crate::types::CreatureId;
use serde::{Deserialize, Serialize};

/// A request from the sim that needs external resolution.
/// The sim emits these; the hosting layer (gdext) drains and fulfills them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OutboundRequest {
    /// Request LLM inference for a creature decision.
    LlmInference {
        request_id: u64,
        creature_id: CreatureId,
        /// Prompt preamble sections, in order. Each is either a well-known
        /// enum variant (for text that doesn't change during gameplay) or a
        /// literal string (for text that varies per creature/time). The
        /// inference layer decides whether to cache KV state for any section.
        preambles: Vec<PreambleSection>,
        /// The creature-specific ephemeral context (recent thoughts, inbox,
        /// immediate situation). Always processed fresh.
        prompt: String,
        /// The expected JSON shape description, included in the prompt to
        /// guide the model's output. Also used by the sim to validate
        /// responses post-hoc.
        response_schema: String,
        /// Tick by which the response is needed. If missed, the request
        /// expires silently (creature keeps current inclinations).
        deadline_tick: u64,
        /// Maximum tokens to generate. Per-request to allow different features
        /// different output budgets.
        max_tokens: u32,
    },
}

/// A preamble section within an LLM inference request.
///
/// Preambles are concatenated in order before the ephemeral prompt. The
/// inference layer can cache KV state for `WellKnown` sections (whose text
/// is fixed at compile time or loaded from config at startup).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PreambleSection {
    /// A well-known preamble identified by string key. The text is fixed
    /// for the lifetime of the game session. Examples: "base_rules",
    /// "social_chat_format".
    WellKnown(String),
    /// A literal preamble string built from current game state (e.g.,
    /// species/path description with current trait values).
    Literal(String),
}

/// Tracking entry for a pending LLM request. Stored in
/// `SimState::pending_llm_requests` (a `BTreeMap<u64, PendingLlmRequest>`)
/// and serialized as part of sim state for save/load.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingLlmRequest {
    pub request_id: u64,
    pub creature_id: CreatureId,
    /// What kind of request this is — determines how to deserialize and
    /// apply the response.
    pub request_kind: LlmRequestKind,
    pub deadline_tick: u64,
}

/// The kind of LLM request, determining the expected response schema and
/// how the result is applied to sim state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LlmRequestKind {
    /// Social chat: the creature is generating dialogue with another creature.
    SocialChat { target_creature_id: CreatureId },
    // Future: ActivityInclination, Diplomacy, etc.
}

/// Observability metadata returned alongside an LLM inference result.
/// Deserialized from the response payload in gdext and passed through to
/// the sim via `SimAction::LlmResult` for logging/debugging.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InferenceMetadata {
    pub latency_ms: u32,
    pub token_count: u32,
    pub cache_hit: bool,
    pub prefill_tokens: u32,
    pub decode_tokens: u32,
}
