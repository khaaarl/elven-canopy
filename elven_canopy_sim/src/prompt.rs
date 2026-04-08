// Prompt construction for LLM creature decisions (F-llm-social-chat).
//
// Builds the preamble sections and ephemeral prompt for social chat requests.
// The prompt budget is tight (~600 tokens total): ~200 tokens base preamble,
// ~100 tokens creature identity, ~100 tokens situational context, ~50 tokens
// response schema, ~50 tokens output budget.
//
// Preamble sections are split into `WellKnown` (cacheable) and `Literal`
// (varies per creature/interaction) to enable future KV cache reuse.
//
// Tests live in `sim/tests/social_chat_tests.rs` where they have access to
// test helpers (flat_world_sim, spawn_creature, etc.).
//
// See also: `social_chat.rs` (response schema), `llm.rs` (outbox types),
// `config.rs::LlmConfig` (tuning), `docs/drafts/llm-creatures.md`.

use crate::llm::PreambleSection;
use crate::sim::SimState;
use crate::social_chat;
use crate::types::{CreatureId, FriendshipCategory, MoodTier, OpinionKind};

/// Build the full prompt components for a social chat request.
/// Returns (preambles, ephemeral_prompt, response_schema).
pub fn build_social_chat_prompt(
    sim: &SimState,
    creature_id: CreatureId,
    target_id: CreatureId,
) -> (Vec<PreambleSection>, String, String) {
    let preambles = vec![
        PreambleSection::WellKnown("social_chat_rules".into()),
        PreambleSection::Literal(creature_identity(sim, creature_id)),
    ];

    let ephemeral = situational_context(sim, creature_id, target_id);
    let schema = social_chat::response_schema_description();

    (preambles, ephemeral, schema)
}

/// The well-known base preamble text for social chat. This is fixed for the
/// lifetime of a game session and can be KV-cached by the inference engine.
/// The inference layer maps the key "social_chat_rules" to this text.
pub fn social_chat_rules_preamble() -> String {
    // NOTE: The response schema is appended separately by the bridge layer
    // (sim_bridge.rs) after the ephemeral prompt. Do not duplicate it here.
    "You are an elf in a fantasy forest village built in the branches of an enormous tree. \
     You live alongside other elves, sharing food, shelter, and community.\n\
     \n\
     Rules:\n\
     - Stay in character. Speak naturally as your character would.\n\
     - Keep \"say\" to one short sentence.\n\
     - Pick the choice that fits your personality and relationship.\n\
     - Output ONLY the JSON object, nothing else."
        .into()
}

/// Build a creature identity string for the preamble. Includes name, species,
/// path, key stats, and current mood.
pub(crate) fn creature_identity(sim: &SimState, creature_id: CreatureId) -> String {
    let creature = match sim.db.creatures.get(&creature_id) {
        Some(c) => c,
        None => return "You are an unknown creature.".into(),
    };

    let name = &creature.name;
    let species = creature.species;

    // Path (elves only).
    let path_str = sim
        .db
        .path_assignments
        .get(&creature_id)
        .map(|pa| format!(" ({} path)", pa.path_id.display_name()))
        .unwrap_or_default();

    // Key stats summary.
    let cha = sim.trait_int(creature_id, crate::types::TraitKind::Charisma, 0);
    let int = sim.trait_int(creature_id, crate::types::TraitKind::Intelligence, 0);

    // Mood.
    let (_, mood_tier) = sim.mood_for_creature(creature_id);
    let mood_str = mood_tier_description(mood_tier);

    format!(
        "You are {name}, a {species:?}{path_str}. \
         CHA {cha}, INT {int}. You feel {mood_str}."
    )
}

/// Build the ephemeral situational context for a social chat. Includes the
/// target's name, the relationship between the two creatures, and a couple
/// of the creature's recent thoughts.
fn situational_context(sim: &SimState, creature_id: CreatureId, target_id: CreatureId) -> String {
    let target_name = sim
        .db
        .creatures
        .get(&target_id)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "someone".into());

    // Relationship.
    let relationship = describe_relationship(sim, creature_id, target_id);

    // Recent thoughts (last 2).
    let thoughts: Vec<_> = sim
        .db
        .thoughts
        .by_creature_id(&creature_id, crate::tabulosity::QueryOpts::ASC);
    let recent: Vec<String> = thoughts
        .iter()
        .rev()
        .take(2)
        .map(|t| t.kind.description())
        .collect();
    let thoughts_str = if recent.is_empty() {
        String::new()
    } else {
        format!(" Recent thoughts: {}.", recent.join("; "))
    };

    format!("You encounter {target_name}. {relationship}{thoughts_str}")
}

/// Describe the relationship between two creatures based on opinion data.
fn describe_relationship(sim: &SimState, creature_id: CreatureId, target_id: CreatureId) -> String {
    let friendliness = sim
        .db
        .creature_opinions
        .get(&(creature_id, OpinionKind::Friendliness, target_id))
        .map(|o| o.intensity)
        .unwrap_or(0);

    let category = sim.friendship_category(friendliness);
    match category {
        FriendshipCategory::Friend => format!("They are your friend (opinion: {friendliness})."),
        FriendshipCategory::Acquaintance => {
            format!("You know them (opinion: {friendliness}).")
        }
        FriendshipCategory::Neutral => "You don't know them well.".into(),
        FriendshipCategory::Disliked => {
            format!("You dislike them (opinion: {friendliness}).")
        }
        FriendshipCategory::Enemy => format!("They are your enemy (opinion: {friendliness})."),
    }
}

/// Convert a MoodTier to a short natural-language descriptor.
fn mood_tier_description(tier: MoodTier) -> &'static str {
    match tier {
        MoodTier::Devastated => "devastated",
        MoodTier::Miserable => "miserable",
        MoodTier::Unhappy => "unhappy",
        MoodTier::Neutral => "fine",
        MoodTier::Content => "content",
        MoodTier::Happy => "happy",
        MoodTier::Elated => "elated",
    }
}
