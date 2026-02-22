// Vaelith grammar engine: generates elvish text phrases with tone maps.
//
// Vaelith is a tonal agglutinative conlang with vowel harmony and free
// word order (SOV default). Every morpheme has a fixed, lexically-specified
// tone. The tone system uses five contour tones (level, rising, falling,
// dipping, peaking) that constrain pitch movement within syllables.
//
// This module provides:
// - Vocabulary lexicon (roots, suffixes, particles with tones)
// - Phrase generator producing grammatically valid text with tone maps
// - Multiple candidate phrases for SA text-swap optimization
//
// The output (VaelithPhrase) is consumed by the music pipeline to assign
// syllables to grid cells with tonal contour constraints.
//
// See docs/drafts/vaelith_v4.md for the full language specification.

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Pitch contour tone for a syllable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
pub enum VowelClass {
    Front,  // e, i — bright, silvery
    Back,   // o, u — deep, warm
}

/// A single syllable in a generated phrase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Syllable {
    /// The text of this syllable (romanized).
    pub text: String,
    /// The tone contour for this syllable.
    pub tone: Tone,
    /// Whether this syllable is stressed (first syllable of root).
    pub stressed: bool,
}

/// A generated Vaelith phrase with its tone map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaelithPhrase {
    /// The full text of the phrase.
    pub text: String,
    /// Syllable breakdown with tones.
    pub syllables: Vec<Syllable>,
    /// Minimum total notes needed to sing this phrase.
    pub min_notes: usize,
    /// Semantic description (for logging/debugging).
    pub meaning: String,
}

impl VaelithPhrase {
    fn from_words(words: &[Word], meaning: &str) -> Self {
        let mut syllables = Vec::new();
        let mut text_parts = Vec::new();

        for word in words {
            text_parts.push(word.text.clone());
            for syl in &word.syllables {
                syllables.push(syl.clone());
            }
        }

        let min_notes: usize = syllables.iter().map(|s| s.tone.min_notes()).sum();
        let text = text_parts.join(" ");

        VaelithPhrase {
            text,
            syllables,
            min_notes,
            meaning: meaning.to_string(),
        }
    }
}

/// A complete word (root + suffixes) with its syllable breakdown.
#[derive(Debug, Clone)]
struct Word {
    text: String,
    syllables: Vec<Syllable>,
}

/// A lexical entry (root word or particle).
#[derive(Debug, Clone)]
struct LexEntry {
    /// The root text.
    text: &'static str,
    /// Syllable texts for the root.
    syllables: &'static [(&'static str, Tone)],
    /// Whether this is stressed on the first syllable.
    stressed_first: bool,
    /// Vowel class for suffix harmony.
    vowel_class: VowelClass,
    /// Part of speech.
    pos: PartOfSpeech,
    /// English meaning (for debugging).
    meaning: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartOfSpeech {
    Noun,
    Verb,
    Adjective,
    Particle,
    Pronoun,
}

impl LexEntry {
    fn to_word(&self) -> Word {
        let syllables: Vec<Syllable> = self.syllables.iter().enumerate().map(|(i, &(text, tone))| {
            Syllable {
                text: text.to_string(),
                tone,
                stressed: self.stressed_first && i == 0,
            }
        }).collect();

        Word {
            text: self.text.to_string(),
            syllables,
        }
    }

    /// Add a suffix with the appropriate vowel harmony variant.
    fn with_suffix(&self, front: &'static str, back: &'static str, tone: Tone) -> Word {
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

// ── Vocabulary ──

const NOUNS: &[LexEntry] = &[
    LexEntry { text: "thír", syllables: &[("thír", Tone::Rising)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "star" },
    LexEntry { text: "aleth", syllables: &[("a", Tone::Level), ("leth", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "tree" },
    LexEntry { text: "lena", syllables: &[("le", Tone::Level), ("na", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "leaf" },
    LexEntry { text: "wena", syllables: &[("we", Tone::Level), ("na", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "water" },
    LexEntry { text: "léshi", syllables: &[("lé", Tone::Rising), ("shi", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "light" },
    LexEntry { text: "mòru", syllables: &[("mò", Tone::Falling), ("ru", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Back,
        pos: PartOfSpeech::Noun, meaning: "forest" },
    LexEntry { text: "kael", syllables: &[("kael", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "song" },
    LexEntry { text: "ana", syllables: &[("a", Tone::Level), ("na", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "spirit" },
    LexEntry { text: "resha", syllables: &[("re", Tone::Level), ("sha", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "moon" },
    LexEntry { text: "fola", syllables: &[("fo", Tone::Level), ("la", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Back,
        pos: PartOfSpeech::Noun, meaning: "love" },
    LexEntry { text: "sena", syllables: &[("se", Tone::Level), ("na", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "memory" },
    LexEntry { text: "eirith", syllables: &[("ei", Tone::Level), ("rith", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "eternity" },
    LexEntry { text: "něma", syllables: &[("ně", Tone::Dipping), ("ma", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "dream" },
    LexEntry { text: "hâli", syllables: &[("hâ", Tone::Peaking), ("li", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "sun" },
    LexEntry { text: "áiren", syllables: &[("ái", Tone::Rising), ("ren", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "canopy" },
    LexEntry { text: "nàshi", syllables: &[("nà", Tone::Falling), ("shi", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "stream" },
    LexEntry { text: "washi", syllables: &[("wa", Tone::Level), ("shi", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "wisdom" },
    LexEntry { text: "thâne", syllables: &[("thâ", Tone::Peaking), ("ne", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Noun, meaning: "fire" },
];

const VERBS: &[LexEntry] = &[
    LexEntry { text: "rase", syllables: &[("ra", Tone::Level), ("se", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "sing" },
    LexEntry { text: "shine", syllables: &[("shi", Tone::Level), ("ne", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "glow" },
    LexEntry { text: "kóse", syllables: &[("kó", Tone::Rising), ("se", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Back,
        pos: PartOfSpeech::Verb, meaning: "grow" },
    LexEntry { text: "fáre", syllables: &[("fá", Tone::Rising), ("re", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "fly" },
    LexEntry { text: "lethe", syllables: &[("le", Tone::Level), ("the", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "flow" },
    LexEntry { text: "ashe", syllables: &[("a", Tone::Level), ("she", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "breathe" },
    LexEntry { text: "mire", syllables: &[("mi", Tone::Level), ("re", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "see" },
    LexEntry { text: "fole", syllables: &[("fo", Tone::Level), ("le", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Back,
        pos: PartOfSpeech::Verb, meaning: "love" },
    LexEntry { text: "sethe", syllables: &[("se", Tone::Level), ("the", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "remember" },
    LexEntry { text: "wethe", syllables: &[("we", Tone::Level), ("the", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "weave" },
    LexEntry { text: "niwe", syllables: &[("ni", Tone::Level), ("we", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "dwell" },
    LexEntry { text: "thale", syllables: &[("tha", Tone::Level), ("le", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Verb, meaning: "name" },
];

const ADJECTIVES: &[LexEntry] = &[
    LexEntry { text: "ráva", syllables: &[("rá", Tone::Rising), ("va", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Adjective, meaning: "bright" },
    LexEntry { text: "ála", syllables: &[("á", Tone::Rising), ("la", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Adjective, meaning: "great" },
    LexEntry { text: "mèsha", syllables: &[("mè", Tone::Falling), ("sha", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Adjective, meaning: "deep" },
    LexEntry { text: "néla", syllables: &[("né", Tone::Rising), ("la", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Adjective, meaning: "new" },
    LexEntry { text: "rǐma", syllables: &[("rǐ", Tone::Dipping), ("ma", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Adjective, meaning: "hidden" },
];

const PARTICLES: &[LexEntry] = &[
    LexEntry { text: "dai", syllables: &[("dai", Tone::Level)],
        stressed_first: false, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Particle, meaning: "truly" },
    LexEntry { text: "na", syllables: &[("na", Tone::Level)],
        stressed_first: false, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Particle, meaning: "and" },
    LexEntry { text: "e", syllables: &[("e", Tone::Level)],
        stressed_first: false, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Particle, meaning: "that" },
    LexEntry { text: "késhi", syllables: &[("ké", Tone::Rising), ("shi", Tone::Level)],
        stressed_first: false, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Particle, meaning: "forever" },
    LexEntry { text: "há", syllables: &[("há", Tone::Rising)],
        stressed_first: false, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Particle, meaning: "behold" },
    LexEntry { text: "lóshi", syllables: &[("ló", Tone::Rising), ("shi", Tone::Level)],
        stressed_first: false, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Particle, meaning: "now" },
];

const PRONOUNS: &[LexEntry] = &[
    LexEntry { text: "náire", syllables: &[("nái", Tone::Rising), ("re", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Pronoun, meaning: "we (inclusive)" },
    LexEntry { text: "le", syllables: &[("le", Tone::Level)],
        stressed_first: true, vowel_class: VowelClass::Front,
        pos: PartOfSpeech::Pronoun, meaning: "it/they" },
];

/// Verb aspect suffixes: (front_suffix, back_suffix, tone).
const ASPECT_SUFFIXES: &[(&str, &str, Tone, &str)] = &[
    ("-thir", "-thur", Tone::Level, "eternal"),
    ("-ren",  "-ran",  Tone::Level, "ongoing"),
    ("-shi",  "-shu",  Tone::Level, "completed"),
    ("-tha",  "-tha",  Tone::Level, "habitual"),
];

/// Case suffixes: (front_suffix, back_suffix, tone).
const CASE_SUFFIXES: &[(&str, &str, Tone, &str)] = &[
    ("-ne", "-no", Tone::Level, "accusative"),
    ("-li", "-lu", Tone::Level, "genitive"),
    ("-se", "-so", Tone::Level, "dative"),
    ("-mi", "-mu", Tone::Level, "locative"),
];

// ── Phrase generation ──

/// Template for a type of phrase.
#[derive(Debug, Clone, Copy)]
enum PhraseTemplate {
    /// "Subject Verb" — simple intransitive (e.g., "Stars shine")
    SubjectVerb,
    /// "Subject Object Verb" — transitive (e.g., "Light weaves song")
    SubjectObjectVerb,
    /// "Behold, Subject Verb" — exclamatory (e.g., "Behold, the tree grows")
    Exclamation,
    /// "Adjective Noun Verb" — descriptive (e.g., "Bright star shines")
    DescriptiveVerb,
    /// "Subject Object-case Verb-aspect" — full sentence
    FullSentence,
    /// "Noun genitive Noun" — possessive (e.g., "Song of the forest")
    PossessiveNoun,
    /// "Verb-eternal, dai" — liturgical affirmation
    LiturgicalAffirmation,
}

/// Generate a set of candidate phrases for use in music generation.
///
/// `brightness` (0.0–1.0) biases vocabulary toward front-class (bright, silvery)
/// or back-class (dark, warm) vowels. 0.5 is neutral. This controls timbral color
/// through vowel harmony — front vowels (e, i) have brighter formants, back
/// vowels (o, u) are warmer.
///
/// Returns multiple phrases for each section, allowing SA to swap between
/// candidates during refinement.
pub fn generate_phrases(
    num_sections: usize,
    rng: &mut impl Rng,
) -> Vec<Vec<VaelithPhrase>> {
    generate_phrases_with_brightness(num_sections, 0.5, rng)
}

/// Generate phrases with brightness bias (0.0 = dark, 1.0 = bright).
pub fn generate_phrases_with_brightness(
    num_sections: usize,
    brightness: f64,
    rng: &mut impl Rng,
) -> Vec<Vec<VaelithPhrase>> {
    let mut sections = Vec::new();

    for _ in 0..num_sections {
        let mut candidates = Vec::new();

        // Generate 3-4 candidate phrases per section with different templates
        let templates = [
            PhraseTemplate::SubjectVerb,
            PhraseTemplate::DescriptiveVerb,
            PhraseTemplate::SubjectObjectVerb,
            PhraseTemplate::FullSentence,
        ];

        for &template in &templates {
            if let Some(phrase) = generate_phrase(template, brightness, rng) {
                candidates.push(phrase);
            }
        }

        // Add a liturgical variant
        if let Some(phrase) = generate_phrase(PhraseTemplate::LiturgicalAffirmation, brightness, rng) {
            candidates.push(phrase);
        }

        sections.push(candidates);
    }

    sections
}

/// Pick a lexical entry with brightness bias.
/// Higher brightness favors front-class vowels, lower favors back-class.
fn pick_biased<'a>(entries: &'a [LexEntry], brightness: f64, rng: &mut impl Rng) -> &'a LexEntry {
    if entries.is_empty() {
        panic!("Empty lexicon");
    }

    // Calculate weights: front-class words get more weight when brightness > 0.5
    let weights: Vec<f64> = entries.iter().map(|e| {
        match e.vowel_class {
            VowelClass::Front => 0.3 + brightness * 0.7,       // 0.3–1.0
            VowelClass::Back => 0.3 + (1.0 - brightness) * 0.7, // 1.0–0.3
        }
    }).collect();

    let total: f64 = weights.iter().sum();
    let r: f64 = rng.random::<f64>() * total;
    let mut cum = 0.0;
    for (i, &w) in weights.iter().enumerate() {
        cum += w;
        if cum > r {
            return &entries[i];
        }
    }
    &entries[0]
}

/// Generate a single phrase from a template.
fn generate_phrase(template: PhraseTemplate, brightness: f64, rng: &mut impl Rng) -> Option<VaelithPhrase> {
    match template {
        PhraseTemplate::SubjectVerb => {
            let noun = pick_biased(NOUNS, brightness, rng);
            let verb = pick_biased(VERBS, brightness, rng);
            let aspect_idx = rng.random_range(0..ASPECT_SUFFIXES.len());
            let (front, back, tone, _aspect_name) = ASPECT_SUFFIXES[aspect_idx];

            let subject = noun.to_word();
            let verb_word = verb.with_suffix(front, back, tone);

            let meaning = format!("{} {}", noun.meaning, verb.meaning);
            Some(VaelithPhrase::from_words(&[subject, verb_word], &meaning))
        }

        PhraseTemplate::SubjectObjectVerb => {
            let subj = pick_biased(NOUNS, brightness, rng);
            let obj = pick_biased(NOUNS, brightness, rng);
            let verb = pick_biased(VERBS, brightness, rng);
            let aspect_idx = rng.random_range(0..ASPECT_SUFFIXES.len());
            let (af, ab, at, _) = ASPECT_SUFFIXES[aspect_idx];

            let subject = subj.to_word();
            let object = obj.with_suffix("-ne", "-no", Tone::Level); // accusative
            let verb_word = verb.with_suffix(af, ab, at);

            let meaning = format!("{} {} {}", subj.meaning, verb.meaning, obj.meaning);
            Some(VaelithPhrase::from_words(&[subject, object, verb_word], &meaning))
        }

        PhraseTemplate::Exclamation => {
            let ha = PARTICLES.iter().find(|p| p.text == "há").unwrap();
            let noun = pick_biased(NOUNS, brightness, rng);
            let verb = pick_biased(VERBS, brightness, rng);
            let aspect_idx = rng.random_range(0..ASPECT_SUFFIXES.len());
            let (af, ab, at, _) = ASPECT_SUFFIXES[aspect_idx];

            let excl = ha.to_word();
            let subject = noun.to_word();
            let verb_word = verb.with_suffix(af, ab, at);

            let meaning = format!("behold, {} {}", noun.meaning, verb.meaning);
            Some(VaelithPhrase::from_words(&[excl, subject, verb_word], &meaning))
        }

        PhraseTemplate::DescriptiveVerb => {
            let adj = pick_biased(ADJECTIVES, brightness, rng);
            let noun = pick_biased(NOUNS, brightness, rng);
            let verb = pick_biased(VERBS, brightness, rng);
            let aspect_idx = rng.random_range(0..ASPECT_SUFFIXES.len());
            let (af, ab, at, _) = ASPECT_SUFFIXES[aspect_idx];

            let adj_word = adj.to_word();
            let noun_word = noun.to_word();
            let verb_word = verb.with_suffix(af, ab, at);

            let meaning = format!("{} {} {}", adj.meaning, noun.meaning, verb.meaning);
            Some(VaelithPhrase::from_words(&[adj_word, noun_word, verb_word], &meaning))
        }

        PhraseTemplate::FullSentence => {
            let subj = pick_biased(NOUNS, brightness, rng);
            let obj = pick_biased(NOUNS, brightness, rng);
            let verb = pick_biased(VERBS, brightness, rng);
            let adj = pick_biased(ADJECTIVES, brightness, rng);
            let aspect_idx = rng.random_range(0..ASPECT_SUFFIXES.len());
            let case_idx = rng.random_range(0..CASE_SUFFIXES.len());
            let (af, ab, at, _) = ASPECT_SUFFIXES[aspect_idx];
            let (cf, cb, ct, _case) = CASE_SUFFIXES[case_idx];

            let adj_word = adj.to_word();
            let subj_word = subj.to_word();
            let obj_word = obj.with_suffix(cf, cb, ct);
            let verb_word = verb.with_suffix(af, ab, at);

            let meaning = format!("{} {} {} {}", adj.meaning, subj.meaning,
                                  verb.meaning, obj.meaning);
            Some(VaelithPhrase::from_words(
                &[adj_word, subj_word, obj_word, verb_word], &meaning))
        }

        PhraseTemplate::PossessiveNoun => {
            let n1 = pick_biased(NOUNS, brightness, rng);
            let n2 = pick_biased(NOUNS, brightness, rng);

            let noun1 = n1.to_word();
            let noun2_gen = n2.with_suffix("-li", "-lu", Tone::Level); // genitive

            let meaning = format!("{} of {}", n1.meaning, n2.meaning);
            Some(VaelithPhrase::from_words(&[noun1, noun2_gen], &meaning))
        }

        PhraseTemplate::LiturgicalAffirmation => {
            let verb = pick_biased(VERBS, brightness, rng);
            let dai = PARTICLES.iter().find(|p| p.text == "dai").unwrap();

            // Use eternal aspect for liturgical
            let verb_word = verb.with_suffix("-thir", "-thur", Tone::Level);
            let dai_word = dai.to_word();

            let meaning = format!("{} eternally, truly", verb.meaning);
            Some(VaelithPhrase::from_words(&[verb_word, dai_word], &meaning))
        }
    }
}

/// Generate a single random phrase (convenience function).
pub fn generate_single_phrase(rng: &mut impl Rng) -> VaelithPhrase {
    let templates = [
        PhraseTemplate::SubjectVerb,
        PhraseTemplate::DescriptiveVerb,
        PhraseTemplate::SubjectObjectVerb,
        PhraseTemplate::Exclamation,
        PhraseTemplate::LiturgicalAffirmation,
        PhraseTemplate::PossessiveNoun,
    ];
    let template = templates[rng.random_range(0..templates.len())];
    generate_phrase(template, 0.5, rng).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_phrase() {
        let mut rng = rand::rng();
        let phrase = generate_single_phrase(&mut rng);

        assert!(!phrase.text.is_empty(), "Phrase should have text");
        assert!(!phrase.syllables.is_empty(), "Phrase should have syllables");
        assert!(phrase.min_notes >= phrase.syllables.len(),
            "Min notes ({}) should be >= syllable count ({})",
            phrase.min_notes, phrase.syllables.len());
    }

    #[test]
    fn test_tone_distribution() {
        let mut rng = rand::rng();
        let mut level_count = 0;
        let mut total_count = 0;

        for _ in 0..100 {
            let phrase = generate_single_phrase(&mut rng);
            for syl in &phrase.syllables {
                total_count += 1;
                if syl.tone == Tone::Level {
                    level_count += 1;
                }
            }
        }

        // Vaelith should have >50% level tones (roots ~40% + suffixes ~95%)
        let pct = level_count as f64 / total_count as f64;
        assert!(pct > 0.40,
            "Level tones should dominate ({:.0}% found, expected >40%)", pct * 100.0);
    }

    #[test]
    fn test_generate_sections() {
        let mut rng = rand::rng();
        let sections = generate_phrases(3, &mut rng);

        assert_eq!(sections.len(), 3);
        for section in &sections {
            assert!(section.len() >= 2,
                "Each section should have multiple candidate phrases");
        }
    }

    #[test]
    fn test_min_notes_correct() {
        // Rising tone needs 2 notes, level needs 1
        let phrase = VaelithPhrase {
            text: "test".to_string(),
            syllables: vec![
                Syllable { text: "thír".to_string(), tone: Tone::Rising, stressed: true },
                Syllable { text: "ne".to_string(), tone: Tone::Level, stressed: false },
            ],
            min_notes: 3, // 2 + 1
            meaning: "test".to_string(),
        };
        assert_eq!(phrase.min_notes, 3);
    }
}
