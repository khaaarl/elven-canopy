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

/// Common entry orders for imitation points. Variety creates interest.
const ENTRY_ORDERS: &[[Voice; 4]] = &[
    [Voice::Soprano, Voice::Alto, Voice::Tenor, Voice::Bass],     // top-down
    [Voice::Tenor, Voice::Bass, Voice::Soprano, Voice::Alto],     // bottom-up (pair)
    [Voice::Alto, Voice::Soprano, Voice::Tenor, Voice::Bass],     // inner voice leads
    [Voice::Bass, Voice::Tenor, Voice::Alto, Voice::Soprano],     // bass-up
    [Voice::Soprano, Voice::Tenor, Voice::Alto, Voice::Bass],     // crossed pairs
];

/// Common transposition schemes for imitation entries.
/// (soprano, alto, tenor, bass) in semitones from reference.
const TRANSPOSITION_SCHEMES: &[[i8; 4]] = &[
    [0, -5, -12, -17],    // standard: unison, 4th down, octave, octave+4th
    [0, -7, -12, -19],    // 5th answer: unison, 5th down, octave, octave+5th
    [0, -5, -12, -12],    // tenor/bass same octave
    [0, -7, -12, -17],    // mixed: 5th down alto, octave tenor, octave+4th bass
    [0, 0, -12, -12],     // paired: sop=alto, tenor=bass (antiphonal)
];

/// Generate a structure plan for a piece.
///
/// Creates a sequence of imitation points using motifs from the library,
/// planning voice entries at staggered offsets. Varies entry order,
/// transposition, and number of participating voices for variety.
pub fn generate_structure(
    motif_library: &MotifLibrary,
    num_sections: usize,
    rng: &mut impl Rng,
) -> StructurePlan {
    let mut imitation_points = Vec::new();
    let mut current_beat: usize = 0;

    // Bias motif selection toward more frequent (more idiomatic) motifs
    let total_freq: u64 = motif_library.motifs.iter().map(|m| m.frequency as u64).sum();

    for section_idx in 0..num_sections {
        // Pick a motif weighted by frequency (more common = more idiomatic)
        let motif = pick_weighted_motif(motif_library, total_freq, rng);

        // Choose reference pitch — vary between sections for pitch interest
        let reference_pitch = match section_idx % 3 {
            0 => rng.random_range(62u8..68),   // lower soprano range
            1 => rng.random_range(67u8..74),   // higher
            _ => rng.random_range(64u8..70),   // middle
        };

        // Vary entry offset slightly
        let base_offset = motif.typical_entry_offset as usize;
        let entry_offset = base_offset + rng.random_range(0..3); // 8-10 beats typical

        // Pick entry order and transposition scheme
        let voice_order = ENTRY_ORDERS[rng.random_range(0..ENTRY_ORDERS.len())];
        let transpositions = TRANSPOSITION_SCHEMES[rng.random_range(0..TRANSPOSITION_SCHEMES.len())];

        // Decide how many voices participate (2-4)
        let min_voices = if section_idx == 0 || section_idx == num_sections - 1 {
            3 // First and last sections should be fuller
        } else {
            2
        };
        let num_voices = rng.random_range(min_voices..=4);

        let mut entries = Vec::new();
        let mut voices_added = 0;

        for (i, &voice) in voice_order.iter().enumerate() {
            if voices_added >= num_voices {
                break;
            }

            // Skip voices randomly (except the lead voice)
            if i > 0 && voices_added >= min_voices && rng.random_bool(0.25) {
                continue;
            }

            let transposition = transpositions[voice.index()];

            entries.push(VoiceEntry {
                voice,
                start_beat: current_beat + i * entry_offset,
                transposition,
            });
            voices_added += 1;
        }

        imitation_points.push(ImitationPoint {
            motif: motif.clone(),
            reference_pitch,
            entries,
        });

        // Advance to next section
        let section_length = (voices_added * entry_offset)
            + motif.intervals.len() * 2
            + rng.random_range(4..12);
        current_beat += section_length;

        // Add a rest gap between sections (2-6 beats of breathing room)
        let gap = rng.random_range(2..7);
        current_beat += gap;
    }

    let total_beats = current_beat + 8; // Add beats for final cadence

    StructurePlan {
        total_beats,
        imitation_points,
    }
}

/// Pick a motif weighted by its corpus frequency (more common = more likely).
fn pick_weighted_motif<'a>(
    library: &'a MotifLibrary,
    total_freq: u64,
    rng: &mut impl Rng,
) -> &'a Motif {
    let target = rng.random_range(0..total_freq);
    let mut cumulative = 0u64;
    for motif in &library.motifs {
        cumulative += motif.frequency as u64;
        if cumulative > target {
            return motif;
        }
    }
    // Fallback (shouldn't happen)
    &library.motifs[0]
}

/// Common rhythmic patterns for motifs (durations in eighth-note beats).
/// These give Palestrina-style variety: mix of quarters, halves, dotted quarters.
const RHYTHM_PATTERNS: &[&[usize]] = &[
    &[2, 2, 2, 2, 2, 2, 2, 2, 2, 2],          // all quarters (simple)
    &[4, 2, 2, 4, 2, 2, 4, 2, 2, 4],          // half, quarter, quarter (stately)
    &[2, 2, 4, 2, 2, 4, 2, 2, 4, 2],          // quarter, quarter, half
    &[3, 1, 2, 2, 3, 1, 2, 2, 3, 1],          // dotted quarter + eighth
    &[2, 4, 2, 4, 2, 4, 2, 4, 2, 4],          // alternating quarter and half
    &[4, 4, 2, 2, 4, 4, 2, 2, 4, 4],          // two halves then two quarters
];

/// Write the motif entries from a structure plan onto the grid.
/// Returns a set of (voice, beat) pairs that are "structural" (shouldn't be
/// freely mutated by micro-mutations in SA).
pub fn apply_structure(grid: &mut Grid, plan: &StructurePlan) -> Vec<(usize, usize)> {
    let mut structural_cells = Vec::new();

    for (point_idx, point) in plan.imitation_points.iter().enumerate() {
        // Pick a rhythm pattern for this imitation point
        // All entries of the same point use the same rhythm (imitation!)
        let rhythm = RHYTHM_PATTERNS[point_idx % RHYTHM_PATTERNS.len()];

        for entry in &point.entries {
            let mut pitch = (point.reference_pitch as i16 + entry.transposition as i16) as u8;
            let voice = entry.voice;

            let (low, high) = voice.range();
            while pitch < low {
                pitch += 12;
            }
            while pitch > high {
                pitch -= 12;
            }

            let mut beat = entry.start_beat;
            if beat >= grid.num_beats {
                continue;
            }

            // First note of motif
            grid.set_note(voice, beat, pitch);
            structural_cells.push((voice.index(), beat));

            // Hold first note for its rhythmic duration
            let first_dur = rhythm[0];
            for hold in 1..first_dur {
                if beat + hold < grid.num_beats {
                    grid.extend_note(voice, beat + hold);
                    structural_cells.push((voice.index(), beat + hold));
                }
            }

            // Each subsequent interval
            for (iv_idx, &interval) in point.motif.intervals.iter().enumerate() {
                let dur = rhythm[(iv_idx + 1) % rhythm.len()];

                beat += rhythm[iv_idx % rhythm.len()];
                if beat >= grid.num_beats {
                    break;
                }

                let new_pitch = (pitch as i16 + interval as i16).clamp(low as i16, high as i16) as u8;
                pitch = new_pitch;
                grid.set_note(voice, beat, pitch);
                structural_cells.push((voice.index(), beat));

                // Hold for this note's rhythmic duration
                for hold in 1..dur {
                    if beat + hold < grid.num_beats {
                        grid.extend_note(voice, beat + hold);
                        structural_cells.push((voice.index(), beat + hold));
                    }
                }
            }

            // Hold final note for an extra beat (phrase ending)
            let final_dur = rhythm[(point.motif.intervals.len()) % rhythm.len()];
            for hold in 1..=final_dur {
                if beat + hold < grid.num_beats {
                    grid.extend_note(voice, beat + hold);
                    structural_cells.push((voice.index(), beat + hold));
                }
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
