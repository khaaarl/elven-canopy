// Draft generation: filling the grid's free cells with Markov-guided pitches.
//
// After structure.rs has placed motif entries (structural cells), this module
// fills the remaining empty cells with musically plausible pitches. It uses
// the melodic Markov model for each voice and the harmonic model to weight
// proposals by how well they fit with other sounding voices.
//
// The draft doesn't need to be perfect — it's refined by SA (sa.rs). But a
// good draft dramatically reduces the work SA needs to do.

use crate::grid::{Grid, Voice, interval};
use crate::markov::MarkovModels;
use rand::Rng;

/// Fill all rest cells in the grid with Markov-sampled pitches.
///
/// Processes each voice left-to-right, using the melodic model conditioned
/// on recent intervals. Harmonic model weights proposals by consonance with
/// other voices.
pub fn fill_draft(
    grid: &mut Grid,
    models: &MarkovModels,
    structural_cells: &[(usize, usize)],
    rng: &mut impl Rng,
) {
    let structural_set: std::collections::HashSet<(usize, usize)> =
        structural_cells.iter().copied().collect();

    for voice in Voice::ALL {
        fill_voice(grid, voice, models, &structural_set, rng);
    }
}

/// Fill free cells in a single voice.
fn fill_voice(
    grid: &mut Grid,
    voice: Voice,
    models: &MarkovModels,
    structural: &std::collections::HashSet<(usize, usize)>,
    rng: &mut impl Rng,
) {
    let (range_low, range_high) = voice.range();
    let mut recent_intervals: Vec<i8> = Vec::new();
    let mut last_pitch: Option<u8> = None;

    for beat in 0..grid.num_beats {
        // Skip structural cells — they're already placed
        if structural.contains(&(voice.index(), beat)) {
            // Update tracking
            let cell = grid.cell(voice, beat);
            if cell.attack && !cell.is_rest {
                if let Some(prev) = last_pitch {
                    let iv = cell.pitch as i8 - prev as i8;
                    recent_intervals.push(iv);
                    if recent_intervals.len() > 4 {
                        recent_intervals.remove(0);
                    }
                }
                last_pitch = Some(cell.pitch);
            }
            continue;
        }

        // Skip non-rest cells that are continuations
        let cell = grid.cell(voice, beat);
        if !cell.is_rest && !cell.attack {
            continue;
        }

        // Decide: should this beat be a new note, a rest, or continuation?
        // Simple heuristic: alternate between 2-beat notes and some variety
        let beat_in_bar = beat % 8; // position within a 4/4 bar

        // Prefer notes on strong beats, allow rests at phrase boundaries
        let should_rest = if last_pitch.is_none() && beat < 4 {
            // Some voices enter later
            rng.random_bool(0.3)
        } else {
            // Occasional rests for breathing
            rng.random_bool(0.05)
        };

        if should_rest {
            continue; // Leave as rest
        }

        // Sample a pitch
        let pitch = if let Some(prev) = last_pitch {
            // Use Markov model to get next interval
            let rng_val: f64 = rng.random();
            let proposed_interval = models.melodic.sample(&recent_intervals, rng_val);
            let proposed_pitch = (prev as i16 + proposed_interval as i16)
                .clamp(range_low as i16, range_high as i16) as u8;

            // Check harmonic compatibility with other sounding voices
            let mut best_pitch = proposed_pitch;
            let mut best_score = harmonic_score(grid, voice, beat, proposed_pitch, models);

            // Try a few alternatives and pick the best
            for _ in 0..3 {
                let alt_rng: f64 = rng.random();
                let alt_interval = models.melodic.sample(&recent_intervals, alt_rng);
                let alt_pitch = (prev as i16 + alt_interval as i16)
                    .clamp(range_low as i16, range_high as i16) as u8;
                let alt_score = harmonic_score(grid, voice, beat, alt_pitch, models);
                if alt_score > best_score {
                    best_score = alt_score;
                    best_pitch = alt_pitch;
                }
            }

            best_pitch
        } else {
            // First note: start in the middle of the range
            let mid = (range_low + range_high) / 2;
            // Adjust to be on a "nice" scale degree (C major / A minor for now)
            snap_to_mode(mid)
        };

        grid.set_note(voice, beat, pitch);

        // Hold note for appropriate duration based on beat position
        let hold_beats = if beat_in_bar == 0 { 3 } // dotted quarter on downbeat
            else if beat_in_bar % 2 == 0 { 1 }     // quarter on other strong beats
            else { 0 };                              // eighth note on weak beats

        for hold in 1..=hold_beats {
            let next = beat + hold;
            if next < grid.num_beats && !structural.contains(&(voice.index(), next)) {
                let next_cell = grid.cell(voice, next);
                if next_cell.is_rest {
                    grid.extend_note(voice, next);
                }
            }
        }

        // Update context
        if let Some(prev) = last_pitch {
            let iv = pitch as i8 - prev as i8;
            recent_intervals.push(iv);
            if recent_intervals.len() > 4 {
                recent_intervals.remove(0);
            }
        }
        last_pitch = Some(pitch);
    }
}

/// Score how well a proposed pitch fits harmonically with other voices at this beat.
fn harmonic_score(
    grid: &Grid,
    voice: Voice,
    beat: usize,
    proposed_pitch: u8,
    _models: &MarkovModels,
) -> f64 {
    let mut score = 0.0;

    for other_voice in Voice::ALL {
        if other_voice == voice {
            continue;
        }

        if let Some(other_pitch) = grid.sounding_pitch(other_voice, beat) {
            let iv = interval::semitones(other_pitch, proposed_pitch);

            // Consonance bonus
            if interval::is_consonant(iv) {
                score += 2.0;
            } else {
                score -= 1.0;
            }

            // Perfect consonance on strong beats
            let is_strong = beat % 4 == 0;
            if is_strong && interval::is_perfect_consonance(iv) {
                score += 1.0;
            }

            // Voice spacing: penalize if too close or too far from adjacent voices
            let abs_iv = iv.unsigned_abs();
            if abs_iv > 24 {
                score -= 1.0; // More than 2 octaves apart
            }
        }
    }

    score
}

/// Snap a pitch to the nearest note in a white-key (C major) scale.
fn snap_to_mode(pitch: u8) -> u8 {
    let pc = pitch % 12;
    let octave = pitch - pc;
    // White key pitch classes: 0,2,4,5,7,9,11
    let snapped = match pc {
        0 => 0,
        1 => 0,
        2 => 2,
        3 => 2,
        4 => 4,
        5 => 5,
        6 => 5,
        7 => 7,
        8 => 7,
        9 => 9,
        10 => 9,
        11 => 11,
        _ => pc,
    };
    octave + snapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markov::{MarkovModels, MotifLibrary};
    use crate::structure::{generate_structure, apply_structure};

    #[test]
    fn test_fill_draft_produces_notes() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mut rng = rand::rng();

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mut rng);

        // Grid should have some non-rest cells
        let mut note_count = 0;
        for voice in Voice::ALL {
            for beat in 0..grid.num_beats {
                if !grid.cell(voice, beat).is_rest {
                    note_count += 1;
                }
            }
        }
        assert!(note_count > 20, "Draft should produce substantial notes, got {}", note_count);
    }

    #[test]
    fn test_snap_to_mode() {
        assert_eq!(snap_to_mode(60), 60); // C4 -> C4
        assert_eq!(snap_to_mode(61), 60); // C#4 -> C4
        assert_eq!(snap_to_mode(62), 62); // D4 -> D4
        assert_eq!(snap_to_mode(66), 65); // F#4 -> F4
    }
}
