// Vaelith phonotactic rules: suffix constants and vowel harmony application.
//
// Vaelith uses vowel harmony — suffixes have front and back variants chosen
// based on the root word's vowel class. This module provides the suffix
// tables (aspect and case) that were originally hardcoded in
// `elven_canopy_music/src/vaelith.rs`.
//
// Used by the music crate for phrase generation (verb inflection, noun case
// marking) and potentially by the sim crate for future linguistic features.

use crate::types::Tone;

/// A suffix with front/back vowel harmony variants.
#[derive(Debug, Clone)]
pub struct HarmonySuffix {
    /// Front vowel variant (for Front vowel class roots).
    pub front: &'static str,
    /// Back vowel variant (for Back vowel class roots).
    pub back: &'static str,
    /// Tone of the suffix syllable.
    pub tone: Tone,
    /// Descriptive label (e.g., "eternal", "accusative").
    pub label: &'static str,
}

/// Verb aspect suffixes (4 variants).
pub const ASPECT_SUFFIXES: &[HarmonySuffix] = &[
    HarmonySuffix {
        front: "-thir",
        back: "-thur",
        tone: Tone::Level,
        label: "eternal",
    },
    HarmonySuffix {
        front: "-ren",
        back: "-ran",
        tone: Tone::Level,
        label: "ongoing",
    },
    HarmonySuffix {
        front: "-shi",
        back: "-shu",
        tone: Tone::Level,
        label: "completed",
    },
    HarmonySuffix {
        front: "-tha",
        back: "-tha",
        tone: Tone::Level,
        label: "habitual",
    },
];

/// Case suffixes for nouns (4 variants).
pub const CASE_SUFFIXES: &[HarmonySuffix] = &[
    HarmonySuffix {
        front: "-ne",
        back: "-no",
        tone: Tone::Level,
        label: "accusative",
    },
    HarmonySuffix {
        front: "-li",
        back: "-lu",
        tone: Tone::Level,
        label: "genitive",
    },
    HarmonySuffix {
        front: "-se",
        back: "-so",
        tone: Tone::Level,
        label: "dative",
    },
    HarmonySuffix {
        front: "-mi",
        back: "-mu",
        tone: Tone::Level,
        label: "locative",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aspect_suffix_count() {
        assert_eq!(ASPECT_SUFFIXES.len(), 4);
    }

    #[test]
    fn test_case_suffix_count() {
        assert_eq!(CASE_SUFFIXES.len(), 4);
    }

    #[test]
    fn test_aspect_suffixes_content() {
        assert_eq!(ASPECT_SUFFIXES[0].front, "-thir");
        assert_eq!(ASPECT_SUFFIXES[0].back, "-thur");
        assert_eq!(ASPECT_SUFFIXES[0].label, "eternal");
    }

    #[test]
    fn test_case_suffixes_content() {
        assert_eq!(CASE_SUFFIXES[0].front, "-ne");
        assert_eq!(CASE_SUFFIXES[0].back, "-no");
        assert_eq!(CASE_SUFFIXES[0].label, "accusative");
    }

    #[test]
    fn test_all_suffixes_are_level_tone() {
        for s in ASPECT_SUFFIXES {
            assert_eq!(s.tone, Tone::Level, "Aspect suffix '{}' should be level", s.label);
        }
        for s in CASE_SUFFIXES {
            assert_eq!(s.tone, Tone::Level, "Case suffix '{}' should be level", s.label);
        }
    }
}
