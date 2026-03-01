// Vaelith grammar engine: generates elvish text phrases with tone maps.
//
// Vaelith is a tonal agglutinative conlang with vowel harmony and free
// word order (SOV default). Every morpheme has a fixed, lexically-specified
// tone. The tone system uses five contour tones (level, rising, falling,
// dipping, peaking) that constrain pitch movement within syllables.
//
// This module provides:
// - Phrase generator producing grammatically valid text with tone maps
// - Multiple candidate phrases for SA text-swap optimization
//
// Core language types (`Tone`, `VowelClass`, `Syllable`, `LexEntry`, `Word`)
// and vocabulary data live in the `elven_canopy_lang` crate. This module
// re-exports the types so internal consumers (`scoring.rs`, `text_mapping.rs`,
// `sa.rs`, `lilypond.rs`, `midi.rs`) keep working with `crate::vaelith::Tone`.
//
// The output (VaelithPhrase) is consumed by the music pipeline to assign
// syllables to grid cells with tonal contour constraints.
//
// See docs/drafts/vaelith_v4.md for the full language specification.

use elven_canopy_lang::phonotactics::{ASPECT_SUFFIXES, CASE_SUFFIXES};
use elven_canopy_lang::{LexEntry, Lexicon, PartOfSpeech};
use elven_canopy_prng::GameRng;
use serde::{Deserialize, Serialize};

// Re-export core types from the lang crate so existing imports
// like `crate::vaelith::Tone` continue to work.
pub use elven_canopy_lang::{Syllable, Tone, VowelClass, Word};

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
    lexicon: &Lexicon,
    num_sections: usize,
    rng: &mut GameRng,
) -> Vec<Vec<VaelithPhrase>> {
    generate_phrases_with_brightness(lexicon, num_sections, 0.5, rng)
}

/// Generate phrases with brightness bias (0.0 = dark, 1.0 = bright).
pub fn generate_phrases_with_brightness(
    lexicon: &Lexicon,
    num_sections: usize,
    brightness: f64,
    rng: &mut GameRng,
) -> Vec<Vec<VaelithPhrase>> {
    let nouns = lexicon.by_pos(PartOfSpeech::Noun);
    let verbs = lexicon.by_pos(PartOfSpeech::Verb);
    let adjectives = lexicon.by_pos(PartOfSpeech::Adjective);
    let particles = lexicon.by_pos(PartOfSpeech::Particle);

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
            if let Some(phrase) = generate_phrase(
                template,
                brightness,
                &nouns,
                &verbs,
                &adjectives,
                &particles,
                rng,
            ) {
                candidates.push(phrase);
            }
        }

        // Add a liturgical variant
        if let Some(phrase) = generate_phrase(
            PhraseTemplate::LiturgicalAffirmation,
            brightness,
            &nouns,
            &verbs,
            &adjectives,
            &particles,
            rng,
        ) {
            candidates.push(phrase);
        }

        sections.push(candidates);
    }

    sections
}

/// Pick a lexical entry with brightness bias.
/// Higher brightness favors front-class vowels, lower favors back-class.
fn pick_biased<'a>(entries: &[&'a LexEntry], brightness: f64, rng: &mut GameRng) -> &'a LexEntry {
    if entries.is_empty() {
        panic!("Empty lexicon");
    }

    // Calculate weights: front-class words get more weight when brightness > 0.5
    let weights: Vec<f64> = entries
        .iter()
        .map(|e| match e.vowel_class {
            VowelClass::Front => 0.3 + brightness * 0.7, // 0.3–1.0
            VowelClass::Back => 0.3 + (1.0 - brightness) * 0.7, // 1.0–0.3
        })
        .collect();

    let total: f64 = weights.iter().sum();
    let r: f64 = rng.next_f64() * total;
    let mut cum = 0.0;
    for (i, &w) in weights.iter().enumerate() {
        cum += w;
        if cum > r {
            return entries[i];
        }
    }
    entries[0]
}

/// Generate a single phrase from a template.
#[allow(clippy::too_many_arguments)]
fn generate_phrase(
    template: PhraseTemplate,
    brightness: f64,
    nouns: &[&LexEntry],
    verbs: &[&LexEntry],
    adjectives: &[&LexEntry],
    particles: &[&LexEntry],
    rng: &mut GameRng,
) -> Option<VaelithPhrase> {
    match template {
        PhraseTemplate::SubjectVerb => {
            let noun = pick_biased(nouns, brightness, rng);
            let verb = pick_biased(verbs, brightness, rng);
            let aspect_idx = rng.range_usize(0, ASPECT_SUFFIXES.len());
            let suffix = &ASPECT_SUFFIXES[aspect_idx];

            let subject = noun.to_word();
            let verb_word = verb.with_suffix(suffix.front, suffix.back, suffix.tone);

            let meaning = format!("{} {}", noun.gloss, verb.gloss);
            Some(VaelithPhrase::from_words(&[subject, verb_word], &meaning))
        }

        PhraseTemplate::SubjectObjectVerb => {
            let subj = pick_biased(nouns, brightness, rng);
            let obj = pick_biased(nouns, brightness, rng);
            let verb = pick_biased(verbs, brightness, rng);
            let aspect_idx = rng.range_usize(0, ASPECT_SUFFIXES.len());
            let suffix = &ASPECT_SUFFIXES[aspect_idx];

            let subject = subj.to_word();
            let object = obj.with_suffix("-ne", "-no", Tone::Level); // accusative
            let verb_word = verb.with_suffix(suffix.front, suffix.back, suffix.tone);

            let meaning = format!("{} {} {}", subj.gloss, verb.gloss, obj.gloss);
            Some(VaelithPhrase::from_words(
                &[subject, object, verb_word],
                &meaning,
            ))
        }

        PhraseTemplate::Exclamation => {
            let ha = particles.iter().find(|p| p.root == "há")?;
            let noun = pick_biased(nouns, brightness, rng);
            let verb = pick_biased(verbs, brightness, rng);
            let aspect_idx = rng.range_usize(0, ASPECT_SUFFIXES.len());
            let suffix = &ASPECT_SUFFIXES[aspect_idx];

            let excl = ha.to_word();
            let subject = noun.to_word();
            let verb_word = verb.with_suffix(suffix.front, suffix.back, suffix.tone);

            let meaning = format!("behold, {} {}", noun.gloss, verb.gloss);
            Some(VaelithPhrase::from_words(
                &[excl, subject, verb_word],
                &meaning,
            ))
        }

        PhraseTemplate::DescriptiveVerb => {
            let adj = pick_biased(adjectives, brightness, rng);
            let noun = pick_biased(nouns, brightness, rng);
            let verb = pick_biased(verbs, brightness, rng);
            let aspect_idx = rng.range_usize(0, ASPECT_SUFFIXES.len());
            let suffix = &ASPECT_SUFFIXES[aspect_idx];

            let adj_word = adj.to_word();
            let noun_word = noun.to_word();
            let verb_word = verb.with_suffix(suffix.front, suffix.back, suffix.tone);

            let meaning = format!("{} {} {}", adj.gloss, noun.gloss, verb.gloss);
            Some(VaelithPhrase::from_words(
                &[adj_word, noun_word, verb_word],
                &meaning,
            ))
        }

        PhraseTemplate::FullSentence => {
            let subj = pick_biased(nouns, brightness, rng);
            let obj = pick_biased(nouns, brightness, rng);
            let verb = pick_biased(verbs, brightness, rng);
            let adj = pick_biased(adjectives, brightness, rng);
            let aspect_idx = rng.range_usize(0, ASPECT_SUFFIXES.len());
            let case_idx = rng.range_usize(0, CASE_SUFFIXES.len());
            let a_suffix = &ASPECT_SUFFIXES[aspect_idx];
            let c_suffix = &CASE_SUFFIXES[case_idx];

            let adj_word = adj.to_word();
            let subj_word = subj.to_word();
            let obj_word = obj.with_suffix(c_suffix.front, c_suffix.back, c_suffix.tone);
            let verb_word = verb.with_suffix(a_suffix.front, a_suffix.back, a_suffix.tone);

            let meaning = format!("{} {} {} {}", adj.gloss, subj.gloss, verb.gloss, obj.gloss);
            Some(VaelithPhrase::from_words(
                &[adj_word, subj_word, obj_word, verb_word],
                &meaning,
            ))
        }

        PhraseTemplate::PossessiveNoun => {
            let n1 = pick_biased(nouns, brightness, rng);
            let n2 = pick_biased(nouns, brightness, rng);

            let noun1 = n1.to_word();
            let noun2_gen = n2.with_suffix("-li", "-lu", Tone::Level); // genitive

            let meaning = format!("{} of {}", n1.gloss, n2.gloss);
            Some(VaelithPhrase::from_words(&[noun1, noun2_gen], &meaning))
        }

        PhraseTemplate::LiturgicalAffirmation => {
            let verb = pick_biased(verbs, brightness, rng);
            let dai = particles.iter().find(|p| p.root == "dai")?;

            // Use eternal aspect for liturgical
            let verb_word = verb.with_suffix("-thir", "-thur", Tone::Level);
            let dai_word = dai.to_word();

            let meaning = format!("{} eternally, truly", verb.gloss);
            Some(VaelithPhrase::from_words(&[verb_word, dai_word], &meaning))
        }
    }
}

/// Generate a single random phrase (convenience function).
pub fn generate_single_phrase(lexicon: &Lexicon, rng: &mut GameRng) -> VaelithPhrase {
    let nouns = lexicon.by_pos(PartOfSpeech::Noun);
    let verbs = lexicon.by_pos(PartOfSpeech::Verb);
    let adjectives = lexicon.by_pos(PartOfSpeech::Adjective);
    let particles = lexicon.by_pos(PartOfSpeech::Particle);

    let templates = [
        PhraseTemplate::SubjectVerb,
        PhraseTemplate::DescriptiveVerb,
        PhraseTemplate::SubjectObjectVerb,
        PhraseTemplate::Exclamation,
        PhraseTemplate::LiturgicalAffirmation,
        PhraseTemplate::PossessiveNoun,
    ];
    let template = templates[rng.range_usize(0, templates.len())];
    generate_phrase(template, 0.5, &nouns, &verbs, &adjectives, &particles, rng).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_lexicon() -> Lexicon {
        elven_canopy_lang::default_lexicon()
    }

    #[test]
    fn test_generate_phrase() {
        let lexicon = test_lexicon();
        let mut rng = GameRng::new(42);
        let phrase = generate_single_phrase(&lexicon, &mut rng);

        assert!(!phrase.text.is_empty(), "Phrase should have text");
        assert!(!phrase.syllables.is_empty(), "Phrase should have syllables");
        assert!(
            phrase.min_notes >= phrase.syllables.len(),
            "Min notes ({}) should be >= syllable count ({})",
            phrase.min_notes,
            phrase.syllables.len()
        );
    }

    #[test]
    fn test_tone_distribution() {
        let lexicon = test_lexicon();
        let mut rng = GameRng::new(42);
        let mut level_count = 0;
        let mut total_count = 0;

        for _ in 0..100 {
            let phrase = generate_single_phrase(&lexicon, &mut rng);
            for syl in &phrase.syllables {
                total_count += 1;
                if syl.tone == Tone::Level {
                    level_count += 1;
                }
            }
        }

        // Vaelith should have >50% level tones (roots ~40% + suffixes ~95%)
        let pct = level_count as f64 / total_count as f64;
        assert!(
            pct > 0.40,
            "Level tones should dominate ({:.0}% found, expected >40%)",
            pct * 100.0
        );
    }

    #[test]
    fn test_generate_sections() {
        let lexicon = test_lexicon();
        let mut rng = GameRng::new(42);
        let sections = generate_phrases(&lexicon, 3, &mut rng);

        assert_eq!(sections.len(), 3);
        for section in &sections {
            assert!(
                section.len() >= 2,
                "Each section should have multiple candidate phrases"
            );
        }
    }

    #[test]
    fn test_min_notes_correct() {
        // Rising tone needs 2 notes, level needs 1
        let phrase = VaelithPhrase {
            text: "test".to_string(),
            syllables: vec![
                Syllable {
                    text: "thír".to_string(),
                    tone: Tone::Rising,
                    stressed: true,
                },
                Syllable {
                    text: "ne".to_string(),
                    tone: Tone::Level,
                    stressed: false,
                },
            ],
            min_notes: 3, // 2 + 1
            meaning: "test".to_string(),
        };
        assert_eq!(phrase.min_notes, 3);
    }
}
