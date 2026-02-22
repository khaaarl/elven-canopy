// Elven Canopy Music Generator
//
// A Palestrina-style polyphonic music generator that produces four-voice
// choral music. The system uses a hybrid approach: Markov models trained on
// Palestrina's corpus provide stylistic guidance, while simulated annealing
// with rule-based scoring ensures counterpoint correctness.
//
// Architecture:
// - grid.rs: Core score representation (SATB voices on an eighth-note grid)
// - markov.rs: Loaded Markov transition tables for melodic/harmonic guidance
// - structure.rs: High-level form planning (motif placement, imitative entries)
// - draft.rs: Initial grid filling using Markov-guided sampling
// - scoring.rs: Multi-layer scoring function (counterpoint rules + preferences)
// - sa.rs: Simulated annealing refinement loop
// - midi.rs: MIDI file output from completed grids
//
// The generator is deterministic given a seed, supporting reproducible output.

pub mod grid;
pub mod mode;
pub mod markov;
pub mod structure;
pub mod draft;
pub mod scoring;
pub mod sa;
pub mod midi;
pub mod vaelith;
