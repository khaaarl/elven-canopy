// Elven Canopy Music Generator
//
// A Palestrina-style polyphonic music generator that produces four-voice
// choral music with procedurally generated Vaelith elvish lyrics. The system
// uses a hybrid approach: Markov models trained on Palestrina's corpus provide
// stylistic guidance, while simulated annealing with rule-based scoring
// ensures counterpoint correctness and tonal contour compliance.
//
// Architecture:
// - grid.rs: Core score representation (SATB voices on an eighth-note grid)
// - mode.rs: Church mode definitions (dorian through ionian), pitch mapping + snapping
// - markov.rs: Loaded Markov transition tables for melodic/harmonic guidance
// - structure.rs: High-level form planning (motifs, imitation, dai/thol responses)
// - draft.rs: Initial grid filling using Markov-guided sampling + final cadence
// - scoring.rs: Multi-layer scoring (counterpoint + harmonic + modal + texture
//   + tension curve + interval distribution + tonal contour)
// - sa.rs: Simulated annealing with pitch/duration/text-swap mutations and
//   adaptive cooling
// - midi.rs: MIDI file output from completed grids
// - lilypond.rs: LilyPond sheet music output (.ly files for engraving)
// - vaelith.rs: Vaelith tonal conlang grammar engine (vocabulary + phrase gen)
// - text_mapping.rs: Syllable-to-grid mapping and tonal contour tracking
//
// The generator is deterministic given a seed, supporting reproducible output.

pub mod draft;
pub mod grid;
pub mod lilypond;
pub mod markov;
pub mod midi;
pub mod mode;
pub mod sa;
pub mod scoring;
pub mod structure;
pub mod text_mapping;
pub mod vaelith;
