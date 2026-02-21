// Structure generation: high-level form planning for a piece.
//
// Before filling in individual notes, the system plans the overall form:
// which motifs to use, when each voice enters, how long each section lasts.
// This creates a partially-filled grid with motif cells constrained and
// connective tissue cells free for the draft generator.
//
// Depends on markov.rs for motif library access. The output is consumed by
// draft.rs to fill free cells.

use crate::grid::{Grid, Voice};
use crate::markov::{Motif, MotifLibrary};
use rand::Rng;

/// A planned entry for one voice in a point of imitation.
#[derive(Debug, Clone)]
pub struct VoiceEntry {
    /// Which voice sings this entry.
    pub voice: Voice,
    /// Beat offset where this entry starts.
    pub start_beat: usize,
    /// Transposition from the motif's reference pitch (semitones).
    pub transposition: i8,
}

/// A planned point of imitation: one motif stated across multiple voices.
#[derive(Debug, Clone)]
pub struct ImitationPoint {
    /// Index into the motif library (or inline motif).
    pub motif: Motif,
    /// Starting pitch for the first (reference) entry.
    pub reference_pitch: u8,
    /// Voice entries with their start times and transpositions.
    pub entries: Vec<VoiceEntry>,
}

/// High-level structure plan for a piece.
#[derive(Debug, Clone)]
pub struct StructurePlan {
    /// Total length in eighth-note beats.
    pub total_beats: usize,
    /// Ordered sequence of imitation points.
    pub imitation_points: Vec<ImitationPoint>,
}

/// Generate a structure plan for a piece.
///
/// Creates a sequence of imitation points using motifs from the library,
/// planning voice entries at staggered offsets.
pub fn generate_structure(
    motif_library: &MotifLibrary,
    num_sections: usize,
    rng: &mut impl Rng,
) -> StructurePlan {
    let mut imitation_points = Vec::new();
    let mut current_beat: usize = 0;

    for _section_idx in 0..num_sections {
        // Pick a motif
        let motif_idx = rng.random_range(0..motif_library.motifs.len());
        let motif = &motif_library.motifs[motif_idx];

        // Choose a reference starting pitch appropriate for soprano range
        let reference_pitch = rng.random_range(62u8..72);

        // Plan entries for all 4 voices with staggered timing
        let entry_offset = motif.typical_entry_offset as usize;
        let mut entries = Vec::new();

        // Entry order varies — common patterns: S-A-T-B or T-B-S-A
        let voice_order = if rng.random_bool(0.5) {
            [Voice::Soprano, Voice::Alto, Voice::Tenor, Voice::Bass]
        } else {
            [Voice::Tenor, Voice::Bass, Voice::Soprano, Voice::Alto]
        };

        for (i, &voice) in voice_order.iter().enumerate() {
            // Not all voices need to participate in every point
            if i > 0 && rng.random_bool(0.15) {
                continue; // 15% chance to skip a voice
            }

            let transposition = match voice {
                Voice::Soprano => 0,
                Voice::Alto => -5, // down a 4th (or up a 5th from an octave lower)
                Voice::Tenor => -12, // down an octave
                Voice::Bass => -17,  // down an octave + 4th
            };

            entries.push(VoiceEntry {
                voice,
                start_beat: current_beat + i * entry_offset,
                transposition,
            });
        }

        imitation_points.push(ImitationPoint {
            motif: motif.clone(),
            reference_pitch,
            entries,
        });

        // Advance to next section with some breathing room
        let section_length = (voice_order.len() * entry_offset)
            + motif.intervals.len() * 2
            + rng.random_range(4..12);
        current_beat += section_length;
    }

    let total_beats = current_beat + 8; // Add a few beats for final cadence

    StructurePlan {
        total_beats,
        imitation_points,
    }
}

/// Write the motif entries from a structure plan onto the grid.
/// Returns a set of (voice, beat) pairs that are "structural" (shouldn't be
/// freely mutated by micro-mutations in SA).
pub fn apply_structure(grid: &mut Grid, plan: &StructurePlan) -> Vec<(usize, usize)> {
    let mut structural_cells = Vec::new();

    for point in &plan.imitation_points {
        for entry in &point.entries {
            let mut pitch = (point.reference_pitch as i16 + entry.transposition as i16) as u8;
            let voice = entry.voice;

            // Clamp to voice range
            let (low, high) = voice.range();
            while pitch < low {
                pitch += 12;
            }
            while pitch > high {
                pitch -= 12;
            }

            // Write the motif onto the grid
            let mut beat = entry.start_beat;
            if beat >= grid.num_beats {
                continue;
            }

            // First note of motif
            grid.set_note(voice, beat, pitch);
            structural_cells.push((voice.index(), beat));

            // Each subsequent interval
            for &interval in &point.motif.intervals {
                // Each motif note gets 2 eighth-note beats (quarter note)
                if beat + 1 < grid.num_beats {
                    grid.extend_note(voice, beat + 1);
                    structural_cells.push((voice.index(), beat + 1));
                }
                beat += 2;
                if beat >= grid.num_beats {
                    break;
                }

                let new_pitch = (pitch as i16 + interval as i16).clamp(low as i16, high as i16) as u8;
                pitch = new_pitch;
                grid.set_note(voice, beat, pitch);
                structural_cells.push((voice.index(), beat));
            }

            // Hold final note for 2 beats
            if beat + 1 < grid.num_beats {
                grid.extend_note(voice, beat + 1);
                structural_cells.push((voice.index(), beat + 1));
            }
        }
    }

    structural_cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_structure() {
        let library = MotifLibrary::default_library();
        let mut rng = rand::rng();
        let plan = generate_structure(&library, 3, &mut rng);

        assert_eq!(plan.imitation_points.len(), 3);
        assert!(plan.total_beats > 0);

        for point in &plan.imitation_points {
            assert!(!point.entries.is_empty());
        }
    }

    #[test]
    fn test_apply_structure() {
        let library = MotifLibrary::default_library();
        let mut rng = rand::rng();
        let plan = generate_structure(&library, 2, &mut rng);

        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);

        // Should have placed some notes
        assert!(!structural.is_empty());

        // Check that structural cells have actual notes
        for &(vi, beat) in &structural {
            assert!(!grid.voices[vi][beat].is_rest,
                "Structural cell at voice {} beat {} should not be rest", vi, beat);
        }
    }
}
