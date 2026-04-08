// Social chat choice types for LLM-generated creature dialogue (F-llm-social-chat).
//
// When a casual social interaction is delegated to the LLM, the model picks
// from a curated `SocialChatChoice` list and generates one sentence of dialogue.
// The choice drives mechanical effects (opinion bonuses, activity invitations,
// gossip thoughts); the dialogue is stored for player display.
//
// The `SocialChatChoice` enum is the single source of truth for valid choices.
// `json_schema_fragment()` generates the JSON schema enum array from the same
// variants, ensuring the prompt schema and the Rust handler stay in sync.
//
// See also: `llm.rs` (outbox types), `config.rs::LlmConfig` (tuning),
// `sim/social.rs` (casual social trigger), `docs/drafts/llm-creatures.md`
// (design rationale, choice-to-operation mapping table).

use serde::{Deserialize, Serialize};

/// The set of social actions the LLM can pick from during a casual chat.
/// Each variant maps to a concrete sim operation (see `docs/drafts/llm-creatures.md`,
/// "Choice-to-operation mapping" table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocialChatChoice {
    /// Warm greeting. No bonus effect beyond baseline mechanical resolution.
    GreetWarmly,
    /// Cold greeting. No bonus effect beyond baseline mechanical resolution.
    GreetColdly,
    /// Ignore the other creature. Dialogue text may be empty or an inner thought.
    Ignore,
    /// Invite the other creature to a dance. Sets `wants_to_organize_dance`
    /// flag on the initiator, picked up on their next idle activation.
    InviteToDance,
    /// Share gossip. Creates a thought on the target creature.
    ShareGossip,
    /// Compliment the other creature. No bonus effect beyond baseline.
    Compliment,
    /// Insult the other creature. No bonus effect beyond baseline.
    Insult,
    /// Ask the other creature for a favor. No bonus effect beyond baseline
    /// (favor mechanics are deferred to future features).
    AskFavor,
}

impl SocialChatChoice {
    /// All valid choice values, in the order they appear in the enum.
    pub const ALL: &[SocialChatChoice] = &[
        Self::GreetWarmly,
        Self::GreetColdly,
        Self::Ignore,
        Self::InviteToDance,
        Self::ShareGossip,
        Self::Compliment,
        Self::Insult,
        Self::AskFavor,
    ];

    /// The snake_case string for this choice (matches serde serialization).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GreetWarmly => "greet_warmly",
            Self::GreetColdly => "greet_coldly",
            Self::Ignore => "ignore",
            Self::InviteToDance => "invite_to_dance",
            Self::ShareGossip => "share_gossip",
            Self::Compliment => "compliment",
            Self::Insult => "insult",
            Self::AskFavor => "ask_favor",
        }
    }

    /// Whether this choice should terminate the conversation (no inbox reply
    /// created for the target). Cold/ignore responses end the exchange.
    pub fn terminates_conversation(self) -> bool {
        matches!(self, Self::GreetColdly | Self::Ignore | Self::Insult)
    }

    /// Generate the JSON schema fragment listing all valid choice values.
    /// Used in prompt construction to constrain the LLM's output.
    pub fn json_schema_fragment() -> String {
        let values: Vec<String> = Self::ALL
            .iter()
            .map(|c| format!("\"{}\"", c.as_str()))
            .collect();
        format!("[{}]", values.join(", "))
    }
}

/// The JSON response structure expected from the LLM for a social chat.
/// The model produces JSON matching this shape; the sim validates it post-hoc.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SocialChatResponse {
    /// Which social action the creature chose.
    pub choice: SocialChatChoice,
    /// One sentence of dialogue the creature says (may be empty for `ignore`).
    #[serde(default)]
    pub say: String,
}

/// Build the response schema description string included in LLM prompts.
/// Describes the expected JSON shape so the model knows what to produce.
pub fn response_schema_description() -> String {
    format!(
        r#"{{"choice": one of {}, "say": "one sentence of dialogue"}}"#,
        SocialChatChoice::json_schema_fragment()
    )
}

/// Attempt to parse an LLM response string into a `SocialChatResponse`.
/// Strips markdown code fences if present, extracts the first balanced
/// `{{...}}` from the raw text, then parses JSON.
/// Returns `None` on any failure (no JSON found, parse error, unknown choice).
pub fn parse_social_chat_response(raw: &str) -> Option<SocialChatResponse> {
    let stripped = strip_code_fences(raw);
    let json_str = extract_first_json_object(&stripped)?;
    serde_json::from_str(json_str).ok()
}

/// Strip markdown code fences (`` ```json ... ``` ``) from LLM output.
/// Small models frequently wrap JSON in code fences even when told not to.
fn strip_code_fences(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

/// Extract the first balanced `{...}` substring from raw text.
/// The LLM may produce preamble text or trailing content around the JSON.
fn extract_first_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let mut depth = 0i32;
    for (i, ch) in raw[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&raw[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn social_chat_choice_serde_roundtrip() {
        for choice in SocialChatChoice::ALL {
            let json = serde_json::to_string(choice).unwrap();
            let restored: SocialChatChoice = serde_json::from_str(&json).unwrap();
            assert_eq!(*choice, restored, "roundtrip failed for {choice:?}");
        }
    }

    #[test]
    fn social_chat_response_parse_valid() {
        let json = r#"{"choice": "greet_warmly", "say": "Hello friend!"}"#;
        let resp: SocialChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choice, SocialChatChoice::GreetWarmly);
        assert_eq!(resp.say, "Hello friend!");
    }

    #[test]
    fn social_chat_response_parse_missing_say() {
        let json = r#"{"choice": "ignore"}"#;
        let resp: SocialChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choice, SocialChatChoice::Ignore);
        assert_eq!(resp.say, "");
    }

    #[test]
    fn social_chat_response_parse_unknown_choice_fails() {
        let json = r#"{"choice": "unknown_thing", "say": "hi"}"#;
        let result = serde_json::from_str::<SocialChatResponse>(json);
        assert!(result.is_err());
    }

    #[test]
    fn schema_fragment_contains_all_choices() {
        let schema = SocialChatChoice::json_schema_fragment();
        for choice in SocialChatChoice::ALL {
            assert!(
                schema.contains(choice.as_str()),
                "schema missing choice: {}",
                choice.as_str()
            );
        }
    }

    #[test]
    fn extract_json_from_clean_input() {
        let raw = r#"{"choice": "greet_warmly", "say": "hi"}"#;
        let extracted = extract_first_json_object(raw).unwrap();
        assert_eq!(extracted, raw);
    }

    #[test]
    fn extract_json_with_preamble() {
        let raw = r#"Sure, here's my response: {"choice": "ignore", "say": ""} done"#;
        let extracted = extract_first_json_object(raw).unwrap();
        assert_eq!(extracted, r#"{"choice": "ignore", "say": ""}"#);
    }

    #[test]
    fn extract_json_nested_braces() {
        let raw = r#"{"choice": "share_gossip", "say": "did you hear about {the thing}?"}"#;
        let extracted = extract_first_json_object(raw).unwrap();
        assert_eq!(extracted, raw);
    }

    #[test]
    fn extract_json_no_object_returns_none() {
        assert!(extract_first_json_object("no json here").is_none());
    }

    #[test]
    fn extract_json_unbalanced_returns_none() {
        assert!(extract_first_json_object("{unbalanced").is_none());
    }

    #[test]
    fn parse_social_chat_response_with_preamble() {
        let raw = r#"I'll greet them warmly. {"choice": "greet_warmly", "say": "Hello!"}"#;
        let resp = parse_social_chat_response(raw).unwrap();
        assert_eq!(resp.choice, SocialChatChoice::GreetWarmly);
        assert_eq!(resp.say, "Hello!");
    }

    #[test]
    fn parse_social_chat_response_garbage_returns_none() {
        assert!(parse_social_chat_response("not json at all").is_none());
    }

    #[test]
    fn parse_social_chat_response_with_code_fences() {
        let raw = "```json\n{\"choice\": \"greet_warmly\", \"say\": \"Hello!\"}\n```";
        let resp = parse_social_chat_response(raw).unwrap();
        assert_eq!(resp.choice, SocialChatChoice::GreetWarmly);
        assert_eq!(resp.say, "Hello!");
    }

    #[test]
    fn strip_code_fences_removes_fences() {
        let input = "```json\n{\"a\": 1}\n```\n";
        let stripped = strip_code_fences(input);
        assert_eq!(stripped, "{\"a\": 1}\n");
    }

    #[test]
    fn response_schema_description_is_nonempty() {
        let schema = response_schema_description();
        assert!(schema.contains("choice"));
        assert!(schema.contains("say"));
    }

    #[test]
    fn terminates_conversation_variants() {
        assert!(!SocialChatChoice::GreetWarmly.terminates_conversation());
        assert!(SocialChatChoice::GreetColdly.terminates_conversation());
        assert!(SocialChatChoice::Ignore.terminates_conversation());
        assert!(!SocialChatChoice::InviteToDance.terminates_conversation());
        assert!(!SocialChatChoice::ShareGossip.terminates_conversation());
        assert!(!SocialChatChoice::Compliment.terminates_conversation());
        assert!(SocialChatChoice::Insult.terminates_conversation());
        assert!(!SocialChatChoice::AskFavor.terminates_conversation());
    }
}
