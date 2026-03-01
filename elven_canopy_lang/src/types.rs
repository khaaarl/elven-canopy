// Core Vaelith language types: tones, vowel classes, syllables, and lexical entries.
//
// These types are shared by the music crate (phrase generation, tonal contour
// scoring) and the sim crate (name generation). They were originally defined
// in `elven_canopy_music/src/vaelith.rs` and extracted here so both crates
// can depend on a single source of truth.
//
// The type hierarchy is:
// - `Tone` — pitch contour for a syllable (level, rising, falling, dipping, peaking)
// - `VowelClass` — front/back vowel harmony class
// - `Syllable` — a rendered syllable with text, tone, and stress
// - `SyllableDef` — a syllable as stored in JSON (text + tone, no stress)
// - `PartOfSpeech` — noun, verb, adjective, particle
// - `NameTag` — which name positions a word fits (given, surname)
// - `LexEntry` — a JSON-loadable lexical entry with owned Strings
// - `Word` — a complete word (root + optional suffixes) with syllable breakdown
//
// Determinism constraint: these types are used by `elven_canopy_sim` and must
// not introduce any non-deterministic behavior (no HashMap, no system RNG).

use serde::{Deserialize, Serialize};

/// Pitch contour tone for a syllable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// Held steady — no pitch change within the syllable.
    Level,
    /// Pitch ascends from first to last note.
    Rising,
    /// Pitch descends from first to last note.
    Falling,
    /// Down-up valley shape (descend then ascend).
    Dipping,
    /// Up-down hill shape (ascend then descend).
    Peaking,
}

impl Tone {
    /// Minimum number of notes needed to realize this tone's contour.
    pub fn min_notes(self) -> usize {
        match self {
            Tone::Level => 1,
            Tone::Rising | Tone::Falling => 2,
            Tone::Dipping | Tone::Peaking => 3,
        }
    }
}

/// Vowel class for suffix harmony.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VowelClass {
    /// Front vowels (e, i) — bright, silvery.
    Front,
    /// Back vowels (o, u) — deep, warm.
    Back,
}

/// A single syllable in a generated phrase or word.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Syllable {
    /// The text of this syllable (romanized).
    pub text: String,
    /// The tone contour for this syllable.
    pub tone: Tone,
    /// Whether this syllable is stressed (first syllable of root).
    pub stressed: bool,
}

/// A syllable definition as stored in the JSON lexicon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyllableDef {
    /// The text of this syllable (romanized).
    pub text: String,
    /// The tone contour for this syllable.
    pub tone: Tone,
}

/// Part of speech for a lexical entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartOfSpeech {
    Noun,
    Verb,
    Adjective,
    Particle,
}

/// Which name positions a word can fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NameTag {
    Given,
    Surname,
}

/// A JSON-loadable lexical entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexEntry {
    /// The root text (with tone diacritics).
    pub root: String,
    /// English translation / meaning.
    pub gloss: String,
    /// Part of speech.
    pub pos: PartOfSpeech,
    /// Syllable definitions (text + tone per syllable).
    pub syllables: Vec<SyllableDef>,
    /// Vowel harmony class.
    pub vowel_class: VowelClass,
    /// Whether stress falls on the first syllable.
    #[serde(default = "default_true")]
    pub stressed_first: bool,
    /// Which name positions this word fits.
    #[serde(default)]
    pub name_tags: Vec<NameTag>,
}

fn default_true() -> bool {
    true
}

impl LexEntry {
    /// Convert this lexical entry into a `Word` with stress applied.
    pub fn to_word(&self) -> Word {
        let syllables: Vec<Syllable> = self
            .syllables
            .iter()
            .enumerate()
            .map(|(i, def)| Syllable {
                text: def.text.clone(),
                tone: def.tone,
                stressed: self.stressed_first && i == 0,
            })
            .collect();

        Word {
            text: self.root.clone(),
            syllables,
        }
    }

    /// Add a suffix with the appropriate vowel harmony variant.
    pub fn with_suffix(&self, front: &str, back: &str, tone: Tone) -> Word {
        let mut word = self.to_word();
        let suffix = match self.vowel_class {
            VowelClass::Front => front,
            VowelClass::Back => back,
        };
        word.text.push_str(suffix);
        word.syllables.push(Syllable {
            text: suffix.to_string(),
            tone,
            stressed: false,
        });
        word
    }
}

/// A complete word (root + optional suffixes) with its syllable breakdown.
#[derive(Debug, Clone)]
pub struct Word {
    /// The full text of the word.
    pub text: String,
    /// Syllable breakdown with tones and stress.
    pub syllables: Vec<Syllable>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tone_min_notes() {
        assert_eq!(Tone::Level.min_notes(), 1);
        assert_eq!(Tone::Rising.min_notes(), 2);
        assert_eq!(Tone::Falling.min_notes(), 2);
        assert_eq!(Tone::Dipping.min_notes(), 3);
        assert_eq!(Tone::Peaking.min_notes(), 3);
    }

    #[test]
    fn test_tone_serde_roundtrip() {
        let json = serde_json::to_string(&Tone::Rising).unwrap();
        assert_eq!(json, "\"rising\"");
        let parsed: Tone = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Tone::Rising);
    }

    #[test]
    fn test_vowel_class_serde() {
        let json = serde_json::to_string(&VowelClass::Front).unwrap();
        assert_eq!(json, "\"front\"");
        let parsed: VowelClass = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, VowelClass::Front);
    }

    #[test]
    fn test_pos_serde() {
        let json = serde_json::to_string(&PartOfSpeech::Noun).unwrap();
        assert_eq!(json, "\"noun\"");
        let parsed: PartOfSpeech = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, PartOfSpeech::Noun);
    }

    #[test]
    fn test_name_tag_serde() {
        let json = serde_json::to_string(&NameTag::Given).unwrap();
        assert_eq!(json, "\"given\"");
        let parsed: NameTag = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, NameTag::Given);
    }

    #[test]
    fn test_lex_entry_to_word() {
        let entry = LexEntry {
            root: "aleth".to_string(),
            gloss: "tree".to_string(),
            pos: PartOfSpeech::Noun,
            syllables: vec![
                SyllableDef {
                    text: "a".to_string(),
                    tone: Tone::Level,
                },
                SyllableDef {
                    text: "leth".to_string(),
                    tone: Tone::Level,
                },
            ],
            vowel_class: VowelClass::Front,
            stressed_first: true,
            name_tags: vec![NameTag::Given, NameTag::Surname],
        };

        let word = entry.to_word();
        assert_eq!(word.text, "aleth");
        assert_eq!(word.syllables.len(), 2);
        assert!(word.syllables[0].stressed);
        assert!(!word.syllables[1].stressed);
        assert_eq!(word.syllables[0].tone, Tone::Level);
    }

    #[test]
    fn test_lex_entry_with_suffix_front() {
        let entry = LexEntry {
            root: "thír".to_string(),
            gloss: "star".to_string(),
            pos: PartOfSpeech::Noun,
            syllables: vec![SyllableDef {
                text: "thír".to_string(),
                tone: Tone::Rising,
            }],
            vowel_class: VowelClass::Front,
            stressed_first: true,
            name_tags: vec![],
        };

        let word = entry.with_suffix("-ne", "-no", Tone::Level);
        assert_eq!(word.text, "thír-ne");
        assert_eq!(word.syllables.len(), 2);
        assert_eq!(word.syllables[1].text, "-ne");
        assert!(!word.syllables[1].stressed);
    }

    #[test]
    fn test_lex_entry_with_suffix_back() {
        let entry = LexEntry {
            root: "mòru".to_string(),
            gloss: "forest".to_string(),
            pos: PartOfSpeech::Noun,
            syllables: vec![
                SyllableDef {
                    text: "mò".to_string(),
                    tone: Tone::Falling,
                },
                SyllableDef {
                    text: "ru".to_string(),
                    tone: Tone::Level,
                },
            ],
            vowel_class: VowelClass::Back,
            stressed_first: true,
            name_tags: vec![],
        };

        let word = entry.with_suffix("-ne", "-no", Tone::Level);
        assert_eq!(word.text, "mòru-no");
        assert_eq!(word.syllables[2].text, "-no");
    }

    #[test]
    fn test_lex_entry_deserialize() {
        let json = r#"{
            "root": "thír",
            "gloss": "star",
            "pos": "noun",
            "syllables": [{"text": "thír", "tone": "rising"}],
            "vowel_class": "front",
            "stressed_first": true,
            "name_tags": ["given", "surname"]
        }"#;

        let entry: LexEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.root, "thír");
        assert_eq!(entry.gloss, "star");
        assert_eq!(entry.pos, PartOfSpeech::Noun);
        assert_eq!(entry.syllables.len(), 1);
        assert_eq!(entry.syllables[0].tone, Tone::Rising);
        assert_eq!(entry.vowel_class, VowelClass::Front);
        assert!(entry.stressed_first);
        assert_eq!(entry.name_tags, vec![NameTag::Given, NameTag::Surname]);
    }

    #[test]
    fn test_lex_entry_defaults() {
        // stressed_first defaults to true, name_tags defaults to empty
        let json = r#"{
            "root": "dai",
            "gloss": "truly",
            "pos": "particle",
            "syllables": [{"text": "dai", "tone": "level"}],
            "vowel_class": "front"
        }"#;

        let entry: LexEntry = serde_json::from_str(json).unwrap();
        assert!(entry.stressed_first);
        assert!(entry.name_tags.is_empty());
    }
}
