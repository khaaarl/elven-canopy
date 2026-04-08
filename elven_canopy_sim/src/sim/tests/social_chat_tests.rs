//! Tests for LLM-generated social chat conversations (F-llm-social-chat).
//!
//! Covers: Conversing task lifecycle (expiry, partner death, preemption),
//! SocialChatChoice response handling, creature_messages table, and the
//! integration between try_casual_social and the LLM outbox.
//!
//! See also: `social_chat.rs` (choice types), `llm.rs` (outbox types),
//! `social.rs` (casual social trigger + conversation checks).

use super::*;
use crate::db::{CreatureMessage, TaskConversingData};
use crate::llm::{InferenceMetadata, LlmRequestKind, PendingLlmRequest, PreambleSection};
use crate::preemption::PreemptionLevel;
use crate::prompt;
use crate::task::TaskOrigin;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Set up a sim with two co-located idle elves suitable for social chat tests.
/// Returns (sim, elf_a, elf_b). Both elves are at the tree position with high
/// CHA for deterministic positive impressions.
fn setup_social_chat_sim(seed: u64) -> (SimState, CreatureId, CreatureId) {
    let mut sim = flat_world_sim(seed);
    sim.config.social.bootstrap_interactions_min = 0;
    sim.config.social.bootstrap_interactions_max = 0;
    sim.config.social.casual_social_chance_ppm = 1_000_000;
    sim.config.social.casual_social_radius = 3;

    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    force_position(&mut sim, elf_a, tree_pos);
    force_position(&mut sim, elf_b, tree_pos);

    set_trait(&mut sim, elf_a, TraitKind::Charisma, 200);
    set_trait(&mut sim, elf_b, TraitKind::Charisma, 200);

    (sim, elf_a, elf_b)
}

/// Helper: create a Conversing task for a creature and assign it as their
/// current task. Returns the task ID.
fn assign_conversing_task(
    sim: &mut SimState,
    creature_id: CreatureId,
    with: CreatureId,
    expires_tick: u64,
) -> TaskId {
    let pos = sim.db.creatures.get(&creature_id).unwrap().position.min;
    let task = Task {
        id: TaskId::new(&mut sim.rng),
        kind: TaskKind::Conversing { with, expires_tick },
        state: TaskState::InProgress,
        location: pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: Some(creature_id),
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    let task_id = task.id;
    sim.insert_task(sim.home_zone_id(), task);
    sim.claim_task(creature_id, task_id);
    // Schedule activation so the creature processes the task.
    sim.set_creature_activation_tick(creature_id, sim.tick + 1);
    task_id
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

fn insert_pending_request(
    sim: &mut SimState,
    creature_id: CreatureId,
    target_creature_id: CreatureId,
    deadline_tick: u64,
) -> u64 {
    let id = sim.next_request_id;
    sim.next_request_id += 1;
    sim.pending_llm_requests.insert(
        id,
        PendingLlmRequest {
            request_id: id,
            creature_id,
            request_kind: LlmRequestKind::SocialChat { target_creature_id },
            deadline_tick,
        },
    );
    id
}

// ---------------------------------------------------------------------------
// Conversing task tests
// ---------------------------------------------------------------------------

#[test]
fn conversing_preemption_is_autonomous() {
    assert_eq!(
        preemption::preemption_level(TaskKindTag::Conversing, TaskOrigin::Autonomous),
        PreemptionLevel::Autonomous
    );
}

#[test]
fn conversing_does_not_require_mana() {
    assert!(!TaskKindTag::Conversing.requires_mana());
}

#[test]
fn conversing_display_name_is_chatting() {
    assert_eq!(TaskKindTag::Conversing.display_name(), "Chatting");
}

#[test]
fn conversing_task_completes_on_expiry() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let expires = sim.tick + 5;

    let task_a = assign_conversing_task(&mut sim, elf_a, elf_b, expires);
    let _task_b = assign_conversing_task(&mut sim, elf_b, elf_a, expires);

    // Before expiry — task should still be active.
    assert!(sim.db.tasks.get(&task_a).is_some());
    assert_eq!(
        sim.db.tasks.get(&task_a).unwrap().state,
        TaskState::InProgress
    );

    // Advance past expiry tick — step multiple ticks to ensure activation runs.
    for tick in (sim.tick + 1)..=(expires + 2) {
        sim.step(&[], tick);
    }

    // Task should be complete (creature no longer has it as current).
    let creature_a = sim.db.creatures.get(&elf_a).unwrap();
    assert_ne!(creature_a.current_task, Some(task_a));
}

#[test]
fn conversing_task_completes_when_partner_dies() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let expires = sim.tick + 1000;

    let task_a = assign_conversing_task(&mut sim, elf_a, elf_b, expires);
    let _task_b = assign_conversing_task(&mut sim, elf_b, elf_a, expires);

    // Kill partner.
    let mut c = sim.db.creatures.get(&elf_b).unwrap();
    c.vital_status = VitalStatus::Dead;
    sim.db.update_creature(c).unwrap();

    // Advance a few ticks to trigger activation check.
    let start = sim.tick;
    for tick in (start + 1)..=(start + 3) {
        sim.step(&[], tick);
    }

    // Elf A's conversing task should have ended.
    let creature_a = sim.db.creatures.get(&elf_a).unwrap();
    assert_ne!(creature_a.current_task, Some(task_a));
}

#[test]
fn conversing_task_extension_data_roundtrip() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let expires = sim.tick + 500;

    let task_id = assign_conversing_task(&mut sim, elf_a, elf_b, expires);

    let conv_data = sim.db.task_conversing_data.get(&task_id).unwrap();
    assert_eq!(conv_data.with, elf_b);
    assert_eq!(conv_data.expires_tick, expires);
}

// ---------------------------------------------------------------------------
// LLM result handling for SocialChat
// ---------------------------------------------------------------------------

#[test]
fn llm_result_stores_creature_message() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(&mut sim, elf_a, elf_b, deadline);

    let json = r#"{"choice": "greet_warmly", "say": "Hello friend!"}"#;
    let cmd = make_llm_result_cmd(&sim, req_id, json);
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    // Should have stored a message from elf_a to elf_b.
    let messages: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].sender_creature_id, elf_a);
    assert_eq!(messages[0].text, "Hello friend!");
    assert_eq!(messages[0].choice, "greet_warmly");
    assert!(!messages[0].processed);
}

#[test]
fn llm_result_invalid_json_discarded() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(&mut sim, elf_a, elf_b, deadline);

    let cmd = make_llm_result_cmd(&sim, req_id, "not json at all");
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    // No message stored.
    let messages: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert!(messages.is_empty());
}

#[test]
fn llm_result_unknown_choice_discarded() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(&mut sim, elf_a, elf_b, deadline);

    let json = r#"{"choice": "unknown_action", "say": "hi"}"#;
    let cmd = make_llm_result_cmd(&sim, req_id, json);
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    let messages: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert!(messages.is_empty());
}

#[test]
fn llm_result_share_gossip_creates_thought() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(&mut sim, elf_a, elf_b, deadline);

    let json = r#"{"choice": "share_gossip", "say": "Did you hear about the troll?"}"#;
    let cmd = make_llm_result_cmd(&sim, req_id, json);
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    // Target creature should have a gossip thought.
    let thoughts: Vec<_> = sim
        .db
        .thoughts
        .by_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    let has_gossip = thoughts
        .iter()
        .any(|t| matches!(&t.kind, ThoughtKind::HeardGossip(_)));
    assert!(has_gossip, "target should have a HeardGossip thought");
}

#[test]
fn creature_message_serde_roundtrip() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    let msg_id = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_id,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Hello!".into(),
            choice: "greet_warmly".into(),
            tick_created: sim.tick,
            processed: false,
        })
        .unwrap();

    let json = serde_json::to_string(&sim).expect("serialize");
    let restored: SimState = serde_json::from_str(&json).expect("deserialize");

    let messages: Vec<_> = restored
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "Hello!");
    assert_eq!(messages[0].sender_creature_id, elf_a);
}

// ---------------------------------------------------------------------------
// Prompt construction tests
// ---------------------------------------------------------------------------

#[test]
fn social_chat_prompt_contains_creature_names() {
    let (sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let name_a = sim.db.creatures.get(&elf_a).unwrap().name.clone();
    let name_b = sim.db.creatures.get(&elf_b).unwrap().name.clone();

    let (preambles, ephemeral, _schema) = prompt::build_social_chat_prompt(&sim, elf_a, elf_b);

    let preamble_text: String = preambles
        .iter()
        .map(|p| match p {
            PreambleSection::WellKnown(key) => key.clone(),
            PreambleSection::Literal(text) => text.clone(),
        })
        .collect();
    assert!(
        preamble_text.contains(&name_a),
        "preamble should contain creature's name '{name_a}'"
    );
    assert!(
        ephemeral.contains(&name_b),
        "prompt should contain target's name '{name_b}'"
    );
}

#[test]
fn social_chat_prompt_contains_opinion_when_present() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    sim.upsert_opinion(elf_a, OpinionKind::Friendliness, elf_b, 20);

    let (_preambles, ephemeral, _schema) = prompt::build_social_chat_prompt(&sim, elf_a, elf_b);

    assert!(
        ephemeral.contains("friend") || ephemeral.contains("Friend"),
        "prompt should mention friendship: {ephemeral}"
    );
}

#[test]
fn social_chat_prompt_contains_schema() {
    let (sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let (_preambles, _prompt, schema) = prompt::build_social_chat_prompt(&sim, elf_a, elf_b);

    assert!(schema.contains("greet_warmly"));
    assert!(schema.contains("share_gossip"));
}

#[test]
fn social_chat_rules_preamble_contains_key_instructions() {
    let preamble = prompt::social_chat_rules_preamble();
    assert!(preamble.contains("JSON"));
    assert!(preamble.contains("choice"));
    assert!(preamble.contains("say"));
}

// ---------------------------------------------------------------------------
// Multi-turn conversation tests
// ---------------------------------------------------------------------------

#[test]
fn inbox_message_triggers_reply_on_idle_activation() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Insert an unprocessed inbox message for elf_b from elf_a.
    let msg_id = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_id,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Hello friend!".into(),
            choice: "greet_warmly".into(),
            tick_created: sim.tick,
            processed: false,
        })
        .unwrap();

    // Elf B is idle. Trigger inbox check directly.
    let emitted = sim.try_process_inbox(elf_b);
    assert!(emitted, "should emit LLM request for inbox message");

    // Elf B should now have a pending LLM request.
    assert_eq!(sim.pending_llm_requests.len(), 1);
    let pending = sim.pending_llm_requests.values().next().unwrap();
    assert_eq!(pending.creature_id, elf_b);

    // Elf B should be in a Conversing task.
    let cb = sim.db.creatures.get(&elf_b).unwrap();
    assert!(cb.current_task.is_some());
    let task = sim.db.tasks.get(&cb.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, TaskKindTag::Conversing);

    // The inbox message should be marked as processed.
    let msg = sim.db.creature_messages.get(&msg_id).unwrap();
    assert!(msg.processed, "inbox message should be marked processed");
}

#[test]
fn inbox_message_waits_when_creature_is_busy() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Put elf_b in a Survival-level task.
    let pos_b = sim.db.creatures.get(&elf_b).unwrap().position.min;
    let sleep_task = Task {
        id: TaskId::new(&mut sim.rng),
        kind: TaskKind::Sleep {
            bed_pos: None,
            location: crate::task::SleepLocation::Ground,
        },
        state: TaskState::InProgress,
        location: pos_b,
        progress: 0,
        total_cost: 10000,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: Some(elf_b),
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    let sleep_id = sleep_task.id;
    sim.insert_task(sim.home_zone_id(), sleep_task);
    sim.claim_task(elf_b, sleep_id);

    // Insert inbox message.
    let msg_id = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_id,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Hey!".into(),
            choice: "greet_warmly".into(),
            tick_created: sim.tick,
            processed: false,
        })
        .unwrap();

    let emitted = sim.try_process_inbox(elf_b);
    assert!(!emitted, "should not emit when creature is busy");
    assert!(sim.pending_llm_requests.is_empty());

    // Message should still be unprocessed.
    let msg = sim.db.creature_messages.get(&msg_id).unwrap();
    assert!(!msg.processed);
}

#[test]
fn multi_turn_conversation_flows() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Step 1: elf_a initiates casual social → LLM request emitted.
    sim.try_casual_social(elf_a);
    assert_eq!(sim.pending_llm_requests.len(), 1);
    let req_id_a = *sim.pending_llm_requests.keys().next().unwrap();

    // Step 2: LLM responds with a warm greeting → message stored for elf_b.
    let json_a = r#"{"choice": "greet_warmly", "say": "Hello there!"}"#;
    let cmd_a = make_llm_result_cmd(&sim, req_id_a, json_a);
    let mut events = Vec::new();
    sim.apply_command(&cmd_a, &mut events);

    let msgs_b: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert_eq!(msgs_b.len(), 1, "elf_b should have 1 inbox message");
    assert!(!msgs_b[0].processed);

    // Step 3: elf_b processes inbox → emits reply LLM request.
    // First complete elf_b's conversing task so they're idle.
    if let Some(task_id) = sim.db.creatures.get(&elf_b).and_then(|c| c.current_task) {
        sim.complete_task(task_id);
    }
    let emitted = sim.try_process_inbox(elf_b);
    assert!(emitted, "elf_b should emit a reply request");
    assert_eq!(sim.pending_llm_requests.len(), 1);
    let req_id_b = *sim.pending_llm_requests.keys().next().unwrap();

    // Step 4: LLM responds for elf_b → message stored for elf_a.
    let json_b = r#"{"choice": "compliment", "say": "You look well today!"}"#;
    let cmd_b = make_llm_result_cmd(&sim, req_id_b, json_b);
    sim.apply_command(&cmd_b, &mut events);

    let msgs_a: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    assert!(
        msgs_a.iter().any(|m| m.text == "You look well today!"),
        "elf_a should have received elf_b's reply"
    );
}

#[test]
fn cold_response_terminates_conversation() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(&mut sim, elf_a, elf_b, deadline);

    // LLM responds with a cold greeting — should NOT create inbox for target.
    let json = r#"{"choice": "greet_coldly", "say": "Hmph."}"#;
    let cmd = make_llm_result_cmd(&sim, req_id, json);
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    // Message IS stored (for conversation log display), but marked processed
    // so it doesn't trigger a reply.
    let msgs: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert_eq!(msgs.len(), 1, "message should be stored for display");
    assert!(
        msgs[0].processed,
        "terminating messages should be marked processed (no reply)"
    );
}

// ---------------------------------------------------------------------------
// Message GC tests
// ---------------------------------------------------------------------------

#[test]
fn message_gc_removes_old_unprocessed() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    sim.config.llm.message_ttl_ticks = 100;

    // Insert an old unprocessed message.
    let msg_id = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_id,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Old message".into(),
            choice: "greet_warmly".into(),
            tick_created: 0, // Very old.
            processed: false,
        })
        .unwrap();

    // Advance sim tick past TTL.
    sim.step(&[], 200);
    sim.gc_creature_messages(elf_b);

    assert!(
        sim.db.creature_messages.get(&msg_id).is_none(),
        "old unprocessed message should be GC'd"
    );
}

#[test]
fn message_gc_preserves_recent_unprocessed() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    sim.config.llm.message_ttl_ticks = 1000;

    let msg_id = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_id,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Recent message".into(),
            choice: "greet_warmly".into(),
            tick_created: sim.tick,
            processed: false,
        })
        .unwrap();

    sim.gc_creature_messages(elf_b);

    assert!(
        sim.db.creature_messages.get(&msg_id).is_some(),
        "recent unprocessed message should be preserved"
    );
}

#[test]
fn message_gc_caps_total_per_creature() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    sim.config.llm.max_messages_per_creature = 3;
    sim.config.llm.message_ttl_ticks = 1_000_000; // Don't TTL-expire.

    // Insert 5 messages.
    for i in 0..5u64 {
        let msg_id = sim.next_message_id;
        sim.next_message_id += 1;
        sim.db
            .insert_creature_message(CreatureMessage {
                message_id: msg_id,
                recipient_creature_id: elf_b,
                sender_creature_id: elf_a,
                text: format!("Message {i}"),
                choice: "greet_warmly".into(),
                tick_created: sim.tick + i,
                processed: true,
            })
            .unwrap();
    }

    sim.gc_creature_messages(elf_b);

    let remaining: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert_eq!(
        remaining.len(),
        3,
        "should cap at max_messages_per_creature"
    );
    // Should keep the newest 3.
    assert_eq!(remaining[0].text, "Message 2");
    assert_eq!(remaining[1].text, "Message 3");
    assert_eq!(remaining[2].text, "Message 4");
}

// ---------------------------------------------------------------------------
// Social chat emission integration tests
// ---------------------------------------------------------------------------

#[test]
fn casual_social_emits_llm_request_when_eligible() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Both elves are idle and co-located — should emit an LLM request.
    sim.try_casual_social(elf_a);

    // Should have one pending LLM request.
    assert_eq!(sim.pending_llm_requests.len(), 1);
    let pending = sim.pending_llm_requests.values().next().unwrap();
    assert_eq!(pending.creature_id, elf_a);

    // Should have one outbound request.
    assert_eq!(sim.outbound_requests.len(), 1);

    // Both creatures should be in Conversing tasks.
    let ca = sim.db.creatures.get(&elf_a).unwrap();
    let cb = sim.db.creatures.get(&elf_b).unwrap();
    assert!(ca.current_task.is_some(), "elf_a should have a task");
    assert!(cb.current_task.is_some(), "elf_b should have a task");

    let task_a = sim.db.tasks.get(&ca.current_task.unwrap()).unwrap();
    let task_b = sim.db.tasks.get(&cb.current_task.unwrap()).unwrap();
    assert_eq!(task_a.kind_tag, TaskKindTag::Conversing);
    assert_eq!(task_b.kind_tag, TaskKindTag::Conversing);
}

#[test]
fn casual_social_resolves_mechanically_when_busy() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Put elf_b in a Survival-level task (e.g., Sleep) so it's not eligible.
    let pos_b = sim.db.creatures.get(&elf_b).unwrap().position.min;
    let sleep_task = Task {
        id: TaskId::new(&mut sim.rng),
        kind: TaskKind::Sleep {
            bed_pos: None,
            location: crate::task::SleepLocation::Ground,
        },
        state: TaskState::InProgress,
        location: pos_b,
        progress: 0,
        total_cost: 10000,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: Some(elf_b),
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    let sleep_id = sleep_task.id;
    sim.insert_task(sim.home_zone_id(), sleep_task);
    sim.claim_task(elf_b, sleep_id);

    sim.try_casual_social(elf_a);

    // No LLM request — resolves mechanically only.
    assert!(sim.pending_llm_requests.is_empty());
    assert!(sim.outbound_requests.is_empty());

    // But opinions should still be modified (mechanical resolution happened).
    let opinions: Vec<_> = sim
        .db
        .creature_opinions
        .by_creature_id(&elf_a, tabulosity::QueryOpts::ASC);
    assert!(
        !opinions.is_empty(),
        "opinions should be set even without LLM"
    );
}

#[test]
fn casual_social_no_duplicate_request() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // First social interaction — should emit request.
    sim.try_casual_social(elf_a);
    assert_eq!(sim.pending_llm_requests.len(), 1);
    assert_eq!(sim.outbound_requests.len(), 1);

    // Clear outbound (simulating drain) but keep pending.
    sim.outbound_requests.clear();

    // Second social interaction — should NOT emit another request
    // (elf_a already has a pending request).
    sim.try_casual_social(elf_a);
    assert_eq!(sim.pending_llm_requests.len(), 1);
    assert!(sim.outbound_requests.is_empty());
}

#[test]
fn casual_social_conversing_has_correct_expiry() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let expected_expires = sim.tick + sim.config.llm.conversation_timeout_ticks;

    sim.try_casual_social(elf_a);

    let ca = sim.db.creatures.get(&elf_a).unwrap();
    let task_id = ca.current_task.unwrap();
    let conv = sim.db.task_conversing_data.get(&task_id).unwrap();
    assert_eq!(conv.expires_tick, expected_expires);
}

#[test]
fn creature_identity_includes_mood() {
    let (sim, elf_a, _elf_b) = setup_social_chat_sim(fresh_test_seed());
    let identity = prompt::creature_identity(&sim, elf_a);
    let has_mood = [
        "devastated",
        "miserable",
        "unhappy",
        "fine",
        "content",
        "happy",
        "elated",
    ]
    .iter()
    .any(|m| identity.contains(m));
    assert!(has_mood, "identity should contain mood: {identity}");
}

// ---------------------------------------------------------------------------
// Additional coverage (once-over)
// ---------------------------------------------------------------------------

#[test]
fn should_end_conversation_returns_false_while_both_conversing() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let expires = sim.tick + 1000;
    let task_a = assign_conversing_task(&mut sim, elf_a, elf_b, expires);
    let _task_b = assign_conversing_task(&mut sim, elf_b, elf_a, expires);

    assert!(
        !sim.should_end_conversation(elf_a, task_a),
        "should NOT end when both are conversing with each other"
    );
}

#[test]
fn should_end_conversation_when_partner_not_conversing_with_us() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let elf_c = spawn_creature(&mut sim, Species::Elf);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    force_position(&mut sim, elf_c, tree_pos);

    let expires = sim.tick + 1000;
    let task_a = assign_conversing_task(&mut sim, elf_a, elf_b, expires);
    // elf_b is conversing with elf_c, not elf_a
    let _task_b = assign_conversing_task(&mut sim, elf_b, elf_c, expires);

    assert!(
        sim.should_end_conversation(elf_a, task_a),
        "should end when partner is conversing with someone else"
    );
}

#[test]
fn should_end_conversation_missing_extension_data() {
    let (mut sim, elf_a, _elf_b) = setup_social_chat_sim(fresh_test_seed());
    // Create a bogus TaskId with no extension data.
    let bogus_task_id = TaskId::new(&mut sim.rng);
    assert!(
        sim.should_end_conversation(elf_a, bogus_task_id),
        "should end when extension data is missing"
    );
}

#[test]
fn llm_result_insult_terminates_and_stores_message() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(&mut sim, elf_a, elf_b, deadline);

    let json = r#"{"choice": "insult", "say": "You smell terrible."}"#;
    let cmd = make_llm_result_cmd(&sim, req_id, json);
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    let msgs: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert_eq!(msgs.len(), 1);
    assert!(
        msgs[0].processed,
        "insult should be marked processed (terminates)"
    );
    assert_eq!(msgs[0].text, "You smell terrible.");
}

#[test]
fn llm_result_compliment_stores_message_no_bonus() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let deadline = sim.tick + 100;
    let req_id = insert_pending_request(&mut sim, elf_a, elf_b, deadline);

    let json = r#"{"choice": "compliment", "say": "You look great!"}"#;
    let cmd = make_llm_result_cmd(&sim, req_id, json);
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    let msgs: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert_eq!(msgs.len(), 1);
    assert!(
        !msgs[0].processed,
        "compliment should NOT be marked processed"
    );

    // No gossip thought created.
    let thoughts: Vec<_> = sim
        .db
        .thoughts
        .by_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert!(
        !thoughts
            .iter()
            .any(|t| matches!(&t.kind, ThoughtKind::HeardGossip(_))),
        "compliment should not create gossip thought"
    );
}

#[test]
fn inbox_multiple_senders_only_processes_first() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    let elf_c = spawn_creature(&mut sim, Species::Elf);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    force_position(&mut sim, elf_c, tree_pos);

    // elf_a and elf_c both send messages to elf_b.
    for (sender, text) in [(elf_a, "Hi from A"), (elf_c, "Hi from C")] {
        let msg_id = sim.next_message_id;
        sim.next_message_id += 1;
        sim.db
            .insert_creature_message(CreatureMessage {
                message_id: msg_id,
                recipient_creature_id: elf_b,
                sender_creature_id: sender,
                text: text.into(),
                choice: "greet_warmly".into(),
                tick_created: sim.tick,
                processed: false,
            })
            .unwrap();
    }

    // Process inbox — should only reply to first sender (elf_a).
    let emitted = sim.try_process_inbox(elf_b);
    assert!(emitted);

    // elf_a's message should be processed, elf_c's should remain unprocessed.
    let msgs: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    let from_a: Vec<_> = msgs
        .iter()
        .filter(|m| m.sender_creature_id == elf_a)
        .collect();
    let from_c: Vec<_> = msgs
        .iter()
        .filter(|m| m.sender_creature_id == elf_c)
        .collect();
    assert!(from_a[0].processed, "elf_a's message should be processed");
    assert!(
        !from_c[0].processed,
        "elf_c's message should remain unprocessed"
    );
}

#[test]
fn next_message_id_serde_roundtrip() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    sim.next_message_id = 42;

    let json = serde_json::to_string(&sim).expect("serialize");
    let restored: SimState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.next_message_id, 42);
}

#[test]
fn gc_ttl_only_removes_unprocessed() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    sim.config.llm.message_ttl_ticks = 100;

    // Old processed message (history) — should survive TTL.
    let msg_processed = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_processed,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Old processed".into(),
            choice: "greet_warmly".into(),
            tick_created: 0,
            processed: true,
        })
        .unwrap();

    // Old unprocessed message — should be TTL'd.
    let msg_unprocessed = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_unprocessed,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Old unprocessed".into(),
            choice: "greet_warmly".into(),
            tick_created: 0,
            processed: false,
        })
        .unwrap();

    sim.step(&[], 200);
    sim.gc_creature_messages(elf_b);

    assert!(
        sim.db.creature_messages.get(&msg_processed).is_some(),
        "old processed message should survive TTL"
    );
    assert!(
        sim.db.creature_messages.get(&msg_unprocessed).is_none(),
        "old unprocessed message should be TTL'd"
    );
}

#[test]
fn pending_request_survives_conversation_end() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Trigger social chat — emits request, both enter Conversing.
    sim.try_casual_social(elf_a);
    assert_eq!(sim.pending_llm_requests.len(), 1);
    let req_id = *sim.pending_llm_requests.keys().next().unwrap();

    // Complete both Conversing tasks (simulating expiry).
    if let Some(tid) = sim.db.creatures.get(&elf_a).and_then(|c| c.current_task) {
        sim.complete_task(tid);
    }
    if let Some(tid) = sim.db.creatures.get(&elf_b).and_then(|c| c.current_task) {
        sim.complete_task(tid);
    }

    // Pending request should still exist.
    assert!(sim.pending_llm_requests.contains_key(&req_id));

    // Deliver LLM result — should still store message even though
    // neither creature is in a Conversing task anymore.
    let json = r#"{"choice": "greet_warmly", "say": "Hello!"}"#;
    let cmd = make_llm_result_cmd(&sim, req_id, json);
    let mut events = Vec::new();
    sim.apply_command(&cmd, &mut events);

    let msgs: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert_eq!(
        msgs.len(),
        1,
        "message should be stored even after task ended"
    );
}

#[test]
fn heard_gossip_thought_serde_roundtrip() {
    let thought = ThoughtKind::HeardGossip("Aelindra".into());
    let json = serde_json::to_string(&thought).unwrap();
    let restored: ThoughtKind = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, thought);
}

#[test]
fn inbox_reply_to_dead_sender_skips() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Insert inbox message from elf_a to elf_b.
    let msg_id = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_id,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Hello!".into(),
            choice: "greet_warmly".into(),
            tick_created: sim.tick,
            processed: false,
        })
        .unwrap();

    // Kill the sender.
    let mut c = sim.db.creatures.get(&elf_a).unwrap();
    c.vital_status = VitalStatus::Dead;
    sim.db.update_creature(c).unwrap();

    // Elf B's inbox processing should skip the dead sender.
    let emitted = sim.try_process_inbox(elf_b);
    assert!(!emitted, "should not emit request for dead sender");
    assert!(sim.pending_llm_requests.is_empty());

    // Message should be marked processed (cleaned up, not left dangling).
    let msg = sim.db.creature_messages.get(&msg_id).unwrap();
    assert!(
        msg.processed,
        "dead sender's message should be marked processed"
    );
}

#[test]
fn gc_cap_does_not_evict_unprocessed_inbox() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());
    sim.config.llm.max_messages_per_creature = 2;
    sim.config.llm.message_ttl_ticks = 1_000_000;

    // Insert 3 processed (history) messages and 1 unprocessed (inbox).
    for i in 0..3u64 {
        let msg_id = sim.next_message_id;
        sim.next_message_id += 1;
        sim.db
            .insert_creature_message(CreatureMessage {
                message_id: msg_id,
                recipient_creature_id: elf_b,
                sender_creature_id: elf_a,
                text: format!("History {i}"),
                choice: "greet_warmly".into(),
                tick_created: sim.tick + i,
                processed: true,
            })
            .unwrap();
    }
    let inbox_msg_id = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: inbox_msg_id,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Unread inbox".into(),
            choice: "greet_warmly".into(),
            tick_created: sim.tick + 10,
            processed: false,
        })
        .unwrap();

    sim.gc_creature_messages(elf_b);

    // The unprocessed inbox message must survive cap eviction.
    assert!(
        sim.db.creature_messages.get(&inbox_msg_id).is_some(),
        "unprocessed inbox message must not be evicted by cap"
    );
}

#[test]
fn social_chat_emit_skips_dead_target() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Kill the target.
    let mut c = sim.db.creatures.get(&elf_b).unwrap();
    c.vital_status = VitalStatus::Dead;
    sim.db.update_creature(c).unwrap();

    sim.try_casual_social(elf_a);

    // No LLM request — dead target is ineligible.
    assert!(sim.pending_llm_requests.is_empty());
    assert!(sim.outbound_requests.is_empty());
}

// ---------------------------------------------------------------------------
// Additional coverage: deadline, serde, assign_conversing, inbox prompt
// ---------------------------------------------------------------------------

#[test]
fn llm_result_past_deadline_is_discarded() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Insert a pending request with a deadline that has already passed.
    let past_deadline = sim.tick.saturating_sub(1);
    let req_id = insert_pending_request(&mut sim, elf_a, elf_b, past_deadline);

    let cmd = make_llm_result_cmd(
        &sim,
        req_id,
        r#"{"choice": "greet_warmly", "say": "Hello!"}"#,
    );
    sim.step(&[cmd], sim.tick + 1);

    // Message should NOT have been stored — deadline was past.
    let msgs: Vec<_> = sim
        .db
        .creature_messages
        .by_recipient_creature_id(&elf_b, tabulosity::QueryOpts::ASC);
    assert!(
        msgs.is_empty(),
        "expired request should not store a message"
    );
}

#[test]
fn assign_conversing_unassigns_existing_autonomous_task() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Spawn a third elf.
    let elf_c = spawn_creature(&mut sim, Species::Elf);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    force_position(&mut sim, elf_c, tree_pos);
    set_trait(&mut sim, elf_c, TraitKind::Charisma, 200);

    // Give elf_a a Conversing task with elf_b.
    let expires = sim.tick + 1000;
    let old_task = assign_conversing_task(&mut sim, elf_a, elf_b, expires);
    assert_eq!(
        sim.db.creatures.get(&elf_a).unwrap().current_task,
        Some(old_task)
    );

    // Insert an inbox message from elf_c to elf_a. Processing the inbox will
    // call assign_conversing internally, which should unassign the old task.
    let msg_id = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_id,
            recipient_creature_id: elf_a,
            sender_creature_id: elf_c,
            text: "Hello there".into(),
            choice: "greet_warmly".into(),
            tick_created: sim.tick,
            processed: false,
        })
        .unwrap();

    let emitted = sim.try_process_inbox(elf_a);
    assert!(emitted, "should process inbox and emit LLM request");

    // Elf_a should now have a different Conversing task (with elf_c).
    let new_task = sim.db.creatures.get(&elf_a).unwrap().current_task;
    assert!(new_task.is_some());
    assert_ne!(
        new_task,
        Some(old_task),
        "old task should have been replaced"
    );
}

#[test]
fn inbox_reply_prompt_contains_sender_message_text() {
    let (mut sim, elf_a, elf_b) = setup_social_chat_sim(fresh_test_seed());

    // Insert an unprocessed inbox message for elf_b from elf_a.
    let msg_id = sim.next_message_id;
    sim.next_message_id += 1;
    sim.db
        .insert_creature_message(CreatureMessage {
            message_id: msg_id,
            recipient_creature_id: elf_b,
            sender_creature_id: elf_a,
            text: "Beautiful weather today".into(),
            choice: "compliment".into(),
            tick_created: sim.tick,
            processed: false,
        })
        .unwrap();

    let emitted = sim.try_process_inbox(elf_b);
    assert!(emitted, "should emit LLM request for inbox message");

    // The outbound request's prompt should contain the message text.
    let request = &sim.outbound_requests[0];
    match request {
        crate::llm::OutboundRequest::LlmInference { prompt, .. } => {
            assert!(
                prompt.contains("Beautiful weather today"),
                "prompt should include sender's message text, got: {prompt}"
            );
        }
    }
}

#[test]
fn task_kind_conversing_serde_roundtrip() {
    let (sim, elf_a, _elf_b) = setup_social_chat_sim(fresh_test_seed());
    let kind = TaskKind::Conversing {
        with: elf_a,
        expires_tick: 42000,
    };
    let json = serde_json::to_string(&kind).unwrap();
    let restored: TaskKind = serde_json::from_str(&json).unwrap();
    // TaskKind doesn't derive PartialEq, so compare via re-serialization.
    let json2 = serde_json::to_string(&restored).unwrap();
    assert_eq!(json, json2);
    // Also verify the JSON contains the expected variant.
    assert!(
        json.contains("Conversing"),
        "JSON should contain variant name"
    );
    assert!(json.contains("42000"), "JSON should contain expires_tick");
    drop(sim);
}

#[test]
fn llm_config_serde_defaults() {
    // Deserialize an empty object — all fields should get defaults.
    let config: crate::config::LlmConfig = serde_json::from_str("{}").unwrap();
    assert!(
        config.deadline_ticks > 0,
        "deadline_ticks should default to non-zero"
    );
    assert!(
        config.max_tokens > 0,
        "max_tokens should default to non-zero"
    );
    assert!(
        config.max_messages_per_creature > 0,
        "max_messages_per_creature should default to non-zero"
    );

    // Roundtrip.
    let json = serde_json::to_string(&config).unwrap();
    let restored: crate::config::LlmConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(config.deadline_ticks, restored.deadline_ticks);
    assert_eq!(config.max_tokens, restored.max_tokens);
}
