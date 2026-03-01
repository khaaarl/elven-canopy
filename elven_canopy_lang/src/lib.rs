// Shared Vaelith constructed language crate.
//
// Provides the Vaelith conlang as a programmatic resource for both
// `elven_canopy_sim` (elf name generation) and `elven_canopy_music`
// (lyric/phrase generation). No Godot dependencies.
//
// Architecture:
// - `types.rs`: Core types — `Tone`, `VowelClass`, `Syllable`, `LexEntry`, `Word`
// - `phonotactics.rs`: Suffix tables (aspect, case) with vowel harmony variants
// - `names.rs`: Deterministic name generator from lexicon entries
// - `lib.rs` (this file): `Lexicon` struct — loads and queries the JSON vocabulary
//
// The lexicon is loaded from `data/vaelith_lexicon.json` via `Lexicon::from_json()`,
// following the same pattern as `GameConfig` in the sim crate (JSON string in,
// typed struct out). The `default_lexicon()` convenience function uses
// `include_str!` to embed the default lexicon at compile time.
//
// Determinism constraint: this crate is used by `elven_canopy_sim` and must
// not introduce any non-deterministic behavior. All RNG goes through
// `elven_canopy_prng::GameRng`.

pub mod names;
pub mod phonotactics;
pub mod types;

// Re-export key types at crate root for convenience.
pub use types::{LexEntry, NameTag, PartOfSpeech, Syllable, SyllableDef, Tone, VowelClass, Word};

use types::LexEntry as LexEntryType;

/// The top-level JSON structure for the lexicon file.
#[derive(Debug, serde::Deserialize)]
struct LexiconFile {
    words: Vec<LexEntryType>,
}

/// A loaded Vaelith lexicon with query methods.
///
/// Constructed from JSON via `from_json()`. Preserves entry order from the
/// JSON file for deterministic iteration (important for same-seed output
/// in the music crate's brightness-biased word selection).
#[derive(Debug, Clone)]
pub struct Lexicon {
    entries: Vec<LexEntryType>,
}

impl Lexicon {
    /// Parse a lexicon from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let file: LexiconFile = serde_json::from_str(json)?;
        Ok(Lexicon {
            entries: file.words,
        })
    }

    /// All entries in the lexicon, in file order.
    pub fn all(&self) -> &[LexEntry] {
        &self.entries
    }

    /// Filter entries by part of speech.
    pub fn by_pos(&self, pos: PartOfSpeech) -> Vec<&LexEntry> {
        self.entries.iter().filter(|e| e.pos == pos).collect()
    }

    /// Filter entries that have a specific name tag.
    pub fn by_name_tag(&self, tag: NameTag) -> Vec<&LexEntry> {
        self.entries
            .iter()
            .filter(|e| e.name_tags.contains(&tag))
            .collect()
    }
}

/// Load the default lexicon embedded at compile time.
///
/// Uses `include_str!` to embed `data/vaelith_lexicon.json`. Panics if
/// the embedded JSON is malformed (should never happen in a released build).
pub fn default_lexicon() -> Lexicon {
    let json = include_str!("../../data/vaelith_lexicon.json");
    Lexicon::from_json(json).expect("embedded vaelith_lexicon.json is malformed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexicon_from_json() {
        let json = r#"{"words": [
            {
                "root": "thír",
                "gloss": "star",
                "pos": "noun",
                "syllables": [{"text": "thír", "tone": "rising"}],
                "vowel_class": "front",
                "stressed_first": true,
                "name_tags": ["given", "surname"]
            },
            {
                "root": "rase",
                "gloss": "sing",
                "pos": "verb",
                "syllables": [{"text": "ra", "tone": "level"}, {"text": "se", "tone": "level"}],
                "vowel_class": "front",
                "stressed_first": true,
                "name_tags": ["given"]
            }
        ]}"#;

        let lexicon = Lexicon::from_json(json).unwrap();
        assert_eq!(lexicon.all().len(), 2);
    }

    #[test]
    fn test_lexicon_by_pos() {
        let json = r#"{"words": [
            {
                "root": "thír",
                "gloss": "star",
                "pos": "noun",
                "syllables": [{"text": "thír", "tone": "rising"}],
                "vowel_class": "front"
            },
            {
                "root": "rase",
                "gloss": "sing",
                "pos": "verb",
                "syllables": [{"text": "ra", "tone": "level"}, {"text": "se", "tone": "level"}],
                "vowel_class": "front"
            },
            {
                "root": "dai",
                "gloss": "truly",
                "pos": "particle",
                "syllables": [{"text": "dai", "tone": "level"}],
                "vowel_class": "front"
            }
        ]}"#;

        let lexicon = Lexicon::from_json(json).unwrap();
        assert_eq!(lexicon.by_pos(PartOfSpeech::Noun).len(), 1);
        assert_eq!(lexicon.by_pos(PartOfSpeech::Verb).len(), 1);
        assert_eq!(lexicon.by_pos(PartOfSpeech::Particle).len(), 1);
        assert_eq!(lexicon.by_pos(PartOfSpeech::Adjective).len(), 0);
    }

    #[test]
    fn test_lexicon_by_name_tag() {
        let json = r#"{"words": [
            {
                "root": "thír",
                "gloss": "star",
                "pos": "noun",
                "syllables": [{"text": "thír", "tone": "rising"}],
                "vowel_class": "front",
                "name_tags": ["given", "surname"]
            },
            {
                "root": "mòru",
                "gloss": "forest",
                "pos": "noun",
                "syllables": [{"text": "mò", "tone": "falling"}, {"text": "ru", "tone": "level"}],
                "vowel_class": "back",
                "name_tags": ["surname"]
            },
            {
                "root": "dai",
                "gloss": "truly",
                "pos": "particle",
                "syllables": [{"text": "dai", "tone": "level"}],
                "vowel_class": "front"
            }
        ]}"#;

        let lexicon = Lexicon::from_json(json).unwrap();
        assert_eq!(lexicon.by_name_tag(NameTag::Given).len(), 1);
        assert_eq!(lexicon.by_name_tag(NameTag::Surname).len(), 2);
    }

    #[test]
    fn test_default_lexicon_loads() {
        let lexicon = default_lexicon();
        // Should have all the words from the JSON file
        assert!(
            lexicon.all().len() >= 40,
            "Expected >= 40 words, got {}",
            lexicon.all().len()
        );
    }

    #[test]
    fn test_default_lexicon_has_all_pos() {
        let lexicon = default_lexicon();
        assert!(
            !lexicon.by_pos(PartOfSpeech::Noun).is_empty(),
            "Should have nouns"
        );
        assert!(
            !lexicon.by_pos(PartOfSpeech::Verb).is_empty(),
            "Should have verbs"
        );
        assert!(
            !lexicon.by_pos(PartOfSpeech::Adjective).is_empty(),
            "Should have adjectives"
        );
        assert!(
            !lexicon.by_pos(PartOfSpeech::Particle).is_empty(),
            "Should have particles"
        );
    }

    #[test]
    fn test_default_lexicon_has_name_tags() {
        let lexicon = default_lexicon();
        assert!(
            !lexicon.by_name_tag(NameTag::Given).is_empty(),
            "Should have given name entries"
        );
        assert!(
            !lexicon.by_name_tag(NameTag::Surname).is_empty(),
            "Should have surname entries"
        );
    }

    #[test]
    fn test_lexicon_preserves_order() {
        let json = r#"{"words": [
            {
                "root": "alpha",
                "gloss": "first",
                "pos": "noun",
                "syllables": [{"text": "al", "tone": "level"}, {"text": "pha", "tone": "level"}],
                "vowel_class": "front"
            },
            {
                "root": "beta",
                "gloss": "second",
                "pos": "noun",
                "syllables": [{"text": "be", "tone": "level"}, {"text": "ta", "tone": "level"}],
                "vowel_class": "front"
            }
        ]}"#;

        let lexicon = Lexicon::from_json(json).unwrap();
        assert_eq!(lexicon.all()[0].root, "alpha");
        assert_eq!(lexicon.all()[1].root, "beta");
    }
}
