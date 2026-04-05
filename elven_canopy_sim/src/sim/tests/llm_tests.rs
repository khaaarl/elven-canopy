//! Tests for the LLM outbox mechanism: request emission, drain via StepResult,
//! deadline expiry, result processing, and serde roundtrip.
//!
//! These tests exercise the sim-side infrastructure only. Nothing emits LLM
//! requests at runtime yet (that comes with F-llm-social-chat etc.), so tests
//! manually push requests and inject results.
//!
//! See also: `llm.rs` for types, `sim/mod.rs` for outbox fields and drain
//! logic, `command.rs` for `SimAction::LlmResult`.

use super::*;
use crate::llm::{
    InferenceMetadata, LlmRequestKind, OutboundRequest, PendingLlmRequest, PreambleSection,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a dummy `CreatureId` from the sim's PRNG. The creature doesn't need
/// to exist in the DB — pending LLM requests only store the ID.
fn dummy_creature_id(sim: &mut SimState) -> CreatureId {
    CreatureId::new(&mut sim.rng)
}

/// Insert a pending LLM request into the sim and return its request ID.
fn insert_pending_request(
    sim: &mut SimState,
    creature_id: CreatureId,
    deadline_tick: u64,
    kind: LlmRequestKind,
) -> u64 {
    let id = sim.next_request_id;
    sim.next_request_id += 1;
    sim.pending_llm_requests.insert(
        id,
        PendingLlmRequest {
            request_id: id,
            creature_id,
            request_kind: kind,
            deadline_tick,
        },
    );
    id
}

fn make_llm_result_cmd(sim: &SimState, request_id: u64, result_json: &str) -> SimCommand {
    SimCommand {
        player_name: "[LLM]".to_string(),
        tick: sim.tick,
        action: SimAction::LlmResult {
            request_id,
            result_json: result_json.to_string(),
            metadata: InferenceMetadata::default(),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn outbound_requests_drained_in_step_result() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let cid = dummy_creature_id(&mut sim);

    sim.outbound_requests.push(OutboundRequest::LlmInference {
        request_id: 0,
        creature_id: cid,
        preambles: vec![],
        prompt: "test".to_string(),
        response_schema: "{}".to_string(),
        deadline_tick: 100,
        max_tokens: 50,
    });
    assert_eq!(sim.outbound_requests.len(), 1);

    let result = sim.step(&[], sim.tick + 1);
    assert_eq!(result.outbound_requests.len(), 1);
    assert!(sim.outbound_requests.is_empty());
}

#[test]
fn outbound_requests_cleared_at_step_start() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let cid = dummy_creature_id(&mut sim);

    sim.outbound_requests.push(OutboundRequest::LlmInference {
        request_id: 0,
        creature_id: cid,
        preambles: vec![],
        prompt: "test".to_string(),
        response_schema: "{}".to_string(),
        deadline_tick: 100,
        max_tokens: 50,
    });
    let result = sim.step(&[], sim.tick + 1);
    assert_eq!(result.outbound_requests.len(), 1);

    // Second step with no new requests — StepResult should be empty.
    let result2 = sim.step(&[], sim.tick + 1);
    assert!(result2.outbound_requests.is_empty());
}

#[test]
fn next_request_id_monotonically_increases() {
    let mut sim = flat_world_sim(fresh_test_seed());
    assert_eq!(sim.next_request_id, 0);

    let id0 = sim.next_request_id;
    sim.next_request_id += 1;
    let id1 = sim.next_request_id;
    sim.next_request_id += 1;
    let id2 = sim.next_request_id;

    assert_eq!(id0, 0);
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
}

#[test]
fn llm_result_removes_pending_request() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let cid = dummy_creature_id(&mut sim);
    let target_cid = dummy_creature_id(&mut sim);
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(
        &mut sim,
        cid,
        deadline,
        LlmRequestKind::SocialChat {
            target_creature_id: target_cid,
        },
    );
    assert!(sim.pending_llm_requests.contains_key(&req_id));

    let cmd = make_llm_result_cmd(&sim, req_id, r#"{"message": "hello"}"#);
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    assert!(!sim.pending_llm_requests.contains_key(&req_id));
}

#[test]
fn llm_result_for_unknown_request_is_discarded() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let bogus_id = 9999;
    assert!(!sim.pending_llm_requests.contains_key(&bogus_id));

    let cmd = make_llm_result_cmd(&sim, bogus_id, "{}");
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    assert!(sim.pending_llm_requests.is_empty());
}

#[test]
fn llm_result_for_expired_request_is_discarded() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let cid = dummy_creature_id(&mut sim);
    let target_cid = dummy_creature_id(&mut sim);
    // Deadline is at tick 5, but we'll advance past it.
    let req_id = insert_pending_request(
        &mut sim,
        cid,
        5,
        LlmRequestKind::SocialChat {
            target_creature_id: target_cid,
        },
    );

    sim.step(&[], 10);
    // The expiry sweep should have removed it.
    assert!(!sim.pending_llm_requests.contains_key(&req_id));
}

#[test]
fn llm_result_at_exact_deadline_tick_is_discarded() {
    // deadline_tick <= current_tick means expired.
    let mut sim = flat_world_sim(fresh_test_seed());
    let cid = dummy_creature_id(&mut sim);
    let target_cid = dummy_creature_id(&mut sim);
    let deadline = sim.tick + 5;
    let req_id = insert_pending_request(
        &mut sim,
        cid,
        deadline,
        LlmRequestKind::SocialChat {
            target_creature_id: target_cid,
        },
    );

    sim.step(&[], deadline);
    assert!(!sim.pending_llm_requests.contains_key(&req_id));
}

#[test]
fn deadline_expiry_preserves_non_expired_requests() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let cid = dummy_creature_id(&mut sim);
    let target_a = dummy_creature_id(&mut sim);
    let target_b = dummy_creature_id(&mut sim);

    let expired_id = insert_pending_request(
        &mut sim,
        cid,
        5,
        LlmRequestKind::SocialChat {
            target_creature_id: target_a,
        },
    );
    let alive_id = insert_pending_request(
        &mut sim,
        cid,
        100,
        LlmRequestKind::SocialChat {
            target_creature_id: target_b,
        },
    );

    sim.step(&[], 10);

    assert!(!sim.pending_llm_requests.contains_key(&expired_id));
    assert!(sim.pending_llm_requests.contains_key(&alive_id));
}

#[test]
fn llm_result_before_deadline_succeeds() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let cid = dummy_creature_id(&mut sim);
    let target_cid = dummy_creature_id(&mut sim);
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(
        &mut sim,
        cid,
        deadline,
        LlmRequestKind::SocialChat {
            target_creature_id: target_cid,
        },
    );

    let cmd = make_llm_result_cmd(&sim, req_id, r#"{"message": "hi"}"#);
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    assert!(!sim.pending_llm_requests.contains_key(&req_id));
}

#[test]
fn pending_llm_requests_serde_roundtrip() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let cid = dummy_creature_id(&mut sim);
    let target_a = dummy_creature_id(&mut sim);
    let target_b = dummy_creature_id(&mut sim);
    let tick = sim.tick;

    insert_pending_request(
        &mut sim,
        cid,
        tick + 100,
        LlmRequestKind::SocialChat {
            target_creature_id: target_a,
        },
    );
    insert_pending_request(
        &mut sim,
        cid,
        tick + 200,
        LlmRequestKind::SocialChat {
            target_creature_id: target_b,
        },
    );

    let json = serde_json::to_string(&sim).expect("serialize");
    let restored: SimState = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.pending_llm_requests.len(), 2);
    assert_eq!(restored.next_request_id, sim.next_request_id);
    // Outbound requests (transient) should not survive serde.
    assert!(restored.outbound_requests.is_empty());
}

#[test]
fn outbound_request_serde_roundtrip() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let cid = dummy_creature_id(&mut sim);

    let req = OutboundRequest::LlmInference {
        request_id: 42,
        creature_id: cid,
        preambles: vec![
            PreambleSection::WellKnown("base_rules".to_string()),
            PreambleSection::Literal("You are an elf.".to_string()),
        ],
        prompt: "What do you want to do?".to_string(),
        response_schema: r#"{"type": "object"}"#.to_string(),
        deadline_tick: 500,
        max_tokens: 100,
    };

    let json = serde_json::to_string(&req).expect("serialize");
    let restored: OutboundRequest = serde_json::from_str(&json).expect("deserialize");

    match &restored {
        OutboundRequest::LlmInference {
            request_id,
            creature_id,
            preambles,
            prompt,
            max_tokens,
            ..
        } => {
            assert_eq!(*request_id, 42);
            assert_eq!(*creature_id, cid);
            assert_eq!(preambles.len(), 2);
            assert_eq!(prompt, "What do you want to do?");
            assert_eq!(*max_tokens, 100);
        }
    }
}

#[test]
fn inference_metadata_serde_roundtrip() {
    let meta = InferenceMetadata {
        latency_ms: 1234,
        token_count: 50,
        cache_hit: true,
        prefill_tokens: 30,
        decode_tokens: 20,
    };

    let json = serde_json::to_string(&meta).expect("serialize");
    let restored: InferenceMetadata = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.latency_ms, 1234);
    assert_eq!(restored.token_count, 50);
    assert!(restored.cache_hit);
    assert_eq!(restored.prefill_tokens, 30);
    assert_eq!(restored.decode_tokens, 20);
}

#[test]
fn session_llm_result_applies_to_sim() {
    use crate::session::{GameSession, SessionMessage};

    // Use a flat_world_sim serialized as JSON, same as other session tests.
    let sim = flat_world_sim(fresh_test_seed());
    let json = serde_json::to_string(&sim).expect("serialize");

    let mut session = GameSession::new_singleplayer();
    session.process(SessionMessage::LoadSim { json });

    // Insert a pending LLM request directly into the sim.
    let sim = session.sim.as_mut().unwrap();
    let cid = dummy_creature_id(sim);
    let target_cid = dummy_creature_id(sim);
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(
        sim,
        cid,
        deadline,
        LlmRequestKind::SocialChat {
            target_creature_id: target_cid,
        },
    );
    assert!(sim.pending_llm_requests.contains_key(&req_id));

    // Process LlmResult through the session — should remove pending request.
    session.process(SessionMessage::LlmResult {
        request_id: req_id,
        result_json: r#"{"message": "hello"}"#.to_string(),
        metadata: InferenceMetadata::default(),
    });

    let sim = session.sim.as_mut().unwrap();
    assert!(!sim.pending_llm_requests.contains_key(&req_id));
}
