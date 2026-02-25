// Markov model for melodic and harmonic guidance.
//
// Loaded from JSON files exported by the Python corpus analysis pipeline.
// Two model types:
// - MelodicModel: per-voice, interval-based, conditioned on previous intervals
//   and metric position. Uses Katz backoff from 3rd to 2nd to 1st order.
// - HarmonicModel: per-voice-pair, conditioned on the interval between two
//   voices at the previous beat.
//
// These models guide draft generation (§8) and SA mutation proposals (§10.1).
// They are "soft" guides — the scoring function (scoring.rs) is the actual
// quality measure.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// An interval-based Markov state for melodic models.
/// Context is a sequence of recent intervals (semitones).
/// Transition probabilities from a melodic context to next intervals.
/// Key: next interval (semitones). Value: probability (unnormalized count).
type TransitionTable = BTreeMap<i8, f64>;

/// Per-voice melodic Markov model with Katz backoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MelodicModel {
    /// Order-3 transitions: (context of 3 intervals) -> next interval probabilities
    pub order3: BTreeMap<String, TransitionTable>,
    /// Order-2 transitions
    pub order2: BTreeMap<String, TransitionTable>,
    /// Order-1 transitions
    pub order1: BTreeMap<String, TransitionTable>,
    /// Order-0 (unigram): overall interval distribution
    pub order0: TransitionTable,
}

impl MelodicModel {
    /// Create a minimal default model with common Palestrina intervals.
    pub fn default_model() -> Self {
        let mut order0 = TransitionTable::new();
        // Stepwise motion dominates (~60%), then thirds (~20%), then rest
        order0.insert(0, 5.0);   // repeated note
        order0.insert(1, 15.0);  // minor 2nd up
        order0.insert(-1, 15.0); // minor 2nd down
        order0.insert(2, 15.0);  // major 2nd up
        order0.insert(-2, 15.0); // major 2nd down
        order0.insert(3, 5.0);   // minor 3rd up
        order0.insert(-3, 5.0);  // minor 3rd down
        order0.insert(4, 5.0);   // major 3rd up
        order0.insert(-4, 5.0);  // major 3rd down
        order0.insert(5, 3.0);   // perfect 4th up
        order0.insert(-5, 3.0);  // perfect 4th down
        order0.insert(7, 2.0);   // perfect 5th up
        order0.insert(-7, 2.0);  // perfect 5th down

        MelodicModel {
            order3: BTreeMap::new(),
            order2: BTreeMap::new(),
            order1: BTreeMap::new(),
            order0,
        }
    }

    /// Sample a next interval given context, using Katz backoff.
    /// Returns (interval, probability).
    pub fn sample(&self, context: &[i8], rng_val: f64) -> i8 {
        // Try order-3 first
        if context.len() >= 3 {
            let key = context_key(&context[context.len()-3..]);
            if let Some(table) = self.order3.get(&key) {
                if let Some(interval) = sample_from_table(table, rng_val) {
                    return interval;
                }
            }
        }

        // Backoff to order-2
        if context.len() >= 2 {
            let key = context_key(&context[context.len()-2..]);
            if let Some(table) = self.order2.get(&key) {
                if let Some(interval) = sample_from_table(table, rng_val) {
                    return interval;
                }
            }
        }

        // Backoff to order-1
        if context.len() >= 1 {
            let key = context_key(&context[context.len()-1..]);
            if let Some(table) = self.order1.get(&key) {
                if let Some(interval) = sample_from_table(table, rng_val) {
                    return interval;
                }
            }
        }

        // Fallback to order-0
        sample_from_table(&self.order0, rng_val).unwrap_or(0)
    }

    /// Get the probability of a next interval given context.
    pub fn probability(&self, context: &[i8], next_interval: i8) -> f64 {
        // Try highest order first, back off
        if context.len() >= 3 {
            let key = context_key(&context[context.len()-3..]);
            if let Some(table) = self.order3.get(&key) {
                if let Some(&p) = table.get(&next_interval) {
                    let total: f64 = table.values().sum();
                    if total > 0.0 {
                        return p / total;
                    }
                }
            }
        }

        if context.len() >= 2 {
            let key = context_key(&context[context.len()-2..]);
            if let Some(table) = self.order2.get(&key) {
                if let Some(&p) = table.get(&next_interval) {
                    let total: f64 = table.values().sum();
                    if total > 0.0 {
                        return p / total;
                    }
                }
            }
        }

        if context.len() >= 1 {
            let key = context_key(&context[context.len()-1..]);
            if let Some(table) = self.order1.get(&key) {
                if let Some(&p) = table.get(&next_interval) {
                    let total: f64 = table.values().sum();
                    if total > 0.0 {
                        return p / total;
                    }
                }
            }
        }

        // Order-0
        let total: f64 = self.order0.values().sum();
        let p = self.order0.get(&next_interval).copied().unwrap_or(0.01);
        p / total
    }
}

/// Per-voice-pair harmonic model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarmonicModel {
    /// Transition: previous interval between voices -> next interval probabilities
    pub transitions: BTreeMap<String, TransitionTable>,
    /// Unigram: overall interval distribution between this voice pair
    pub unigram: TransitionTable,
}

impl HarmonicModel {
    /// Create a default harmonic model favoring consonances.
    pub fn default_model() -> Self {
        let mut unigram = TransitionTable::new();
        // Favor consonant intervals (mod 12)
        for interval in -24i8..=24 {
            let ic = (interval.unsigned_abs()) % 12;
            let weight = match ic {
                0 => 8.0,  // unison/octave
                3 => 10.0, // minor 3rd
                4 => 10.0, // major 3rd
                5 => 6.0,  // perfect 4th
                7 => 12.0, // perfect 5th
                8 => 8.0,  // minor 6th
                9 => 8.0,  // major 6th
                _ => 1.0,  // dissonances
            };
            unigram.insert(interval, weight);
        }

        HarmonicModel {
            transitions: BTreeMap::new(),
            unigram,
        }
    }
}

/// The complete set of Markov models loaded from corpus analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkovModels {
    /// One melodic model per voice (shared if not enough data per voice)
    pub melodic: MelodicModel,
    /// Harmonic models for voice pairs (6 pairs for SATB)
    pub harmonic: HarmonicModel,
}

impl MarkovModels {
    /// Load from a JSON file.
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let data = std::fs::read_to_string(path)?;
        let models: MarkovModels = serde_json::from_str(&data)?;
        Ok(models)
    }

    /// Create default models (for use before corpus analysis is complete).
    pub fn default_models() -> Self {
        MarkovModels {
            melodic: MelodicModel::default_model(),
            harmonic: HarmonicModel::default_model(),
        }
    }
}

/// Motif: a short melodic pattern extracted from the corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Motif {
    /// Interval sequence (semitones between consecutive notes).
    pub intervals: Vec<i8>,
    /// How often this pattern appeared in the corpus.
    pub frequency: u32,
    /// Typical entry offset between voices (in eighth-note beats).
    pub typical_entry_offset: u8,
    /// Typical transposition for imitation (semitones, e.g. 7 for 5th).
    pub typical_transposition: i8,
}

/// Library of motifs extracted from the corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotifLibrary {
    pub motifs: Vec<Motif>,
}

impl MotifLibrary {
    /// Load from JSON.
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let data = std::fs::read_to_string(path)?;
        let lib: MotifLibrary = serde_json::from_str(&data)?;
        Ok(lib)
    }

    /// Create a small default motif library with common Palestrina patterns.
    pub fn default_library() -> Self {
        MotifLibrary {
            motifs: vec![
                // Ascending stepwise motif (very common in Palestrina)
                Motif {
                    intervals: vec![2, 2, 1, 2],  // whole, whole, half, whole steps
                    frequency: 50,
                    typical_entry_offset: 8,       // 1 bar offset
                    typical_transposition: 7,      // at the 5th
                },
                // Descending stepwise
                Motif {
                    intervals: vec![-2, -2, -1, -2],
                    frequency: 45,
                    typical_entry_offset: 8,
                    typical_transposition: 7,
                },
                // Ascending with leap recovery
                Motif {
                    intervals: vec![2, 2, 5, -2, -1],  // step up, leap 4th, step back
                    frequency: 30,
                    typical_entry_offset: 6,
                    typical_transposition: 7,
                },
                // Arch shape (up then down)
                Motif {
                    intervals: vec![2, 2, 1, -2, -2],
                    frequency: 35,
                    typical_entry_offset: 8,
                    typical_transposition: 12,     // at the octave
                },
                // Opening 5th leap with stepwise descent
                Motif {
                    intervals: vec![7, -2, -2, -1, -2],
                    frequency: 25,
                    typical_entry_offset: 8,
                    typical_transposition: 7,
                },
                // Gentle undulation
                Motif {
                    intervals: vec![2, -1, 2, -1, 2],
                    frequency: 20,
                    typical_entry_offset: 6,
                    typical_transposition: 7,
                },
            ],
        }
    }
}

/// Encode a context (slice of intervals) as a string key for BTreeMap lookup.
fn context_key(context: &[i8]) -> String {
    context.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
}

/// Sample an interval from a transition table using a random value in [0, 1).
fn sample_from_table(table: &TransitionTable, rng_val: f64) -> Option<i8> {
    if table.is_empty() {
        return None;
    }
    let total: f64 = table.values().sum();
    if total <= 0.0 {
        return None;
    }

    let target = rng_val * total;
    let mut cumulative = 0.0;
    for (&interval, &weight) in table {
        cumulative += weight;
        if cumulative > target {
            return Some(interval);
        }
    }
    // Return last key as fallback
    table.keys().next_back().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_melodic_model() {
        let model = MelodicModel::default_model();
        // Should produce a reasonable interval
        let interval = model.sample(&[], 0.5);
        assert!((-12..=12).contains(&interval));
    }

    #[test]
    fn test_sample_from_table() {
        let mut table = TransitionTable::new();
        table.insert(2, 1.0);
        table.insert(-2, 1.0);

        // With rng_val=0.0, should get first entry
        let result = sample_from_table(&table, 0.0);
        assert!(result.is_some());
    }

    #[test]
    fn test_context_key() {
        assert_eq!(context_key(&[2, -1, 3]), "2,-1,3");
        assert_eq!(context_key(&[]), "");
    }
}
