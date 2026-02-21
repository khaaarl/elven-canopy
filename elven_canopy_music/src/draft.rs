// Draft generation: filling the grid's free cells with Markov-guided pitches.
//
// After structure.rs has placed motif entries (structural cells), this module
// fills the remaining empty cells with musically plausible pitches. It uses
// the melodic Markov model for each voice and the harmonic model to weight
// proposals by how well they fit with other sounding voices.
//
// Now mode-aware: pitches are snapped to the active mode and scored for
// modal fitness. Uses mode.rs for scale membership checks.
//
// The draft doesn't need to be perfect — it's refined by SA (sa.rs). But a
// good draft dramatically reduces the work SA needs to do.

use crate::grid::{Grid, Voice, interval};
use crate::markov::MarkovModels;
use crate::mode::ModeInstance;
use rand::Rng;

/// Fill all rest cells in the grid with Markov-sampled pitches.
///
/// Processes each voice left-to-right, using the melodic model conditioned
/// on recent intervals. Harmonic model weights proposals by consonance with
/// other voices. All pitches are snapped to the given mode.
pub fn fill_draft(
    grid: &mut Grid,
    models: &MarkovModels,
    structural_cells: &[(usize, usize)],
    mode: &ModeInstance,
    rng: &mut impl Rng,
) {
    let structural_set: std::collections::HashSet<(usize, usize)> =
        structural_cells.iter().copied().collect();

    for voice in Voice::ALL {
        fill_voice(grid, voice, models, &structural_set, mode, rng);
    }
}

/// Fill free cells in a single voice.
fn fill_voice(
    grid: &mut Grid,
    voice: Voice,
    models: &MarkovModels,
    structural: &std::collections::HashSet<(usize, usize)>,
    mode: &ModeInstance,
    rng: &mut impl Rng,
) {
    let (range_low, range_high) = voice.range();
    let mut recent_intervals: Vec<i8> = Vec::new();
    let mut last_pitch: Option<u8> = None;
    let mut beats_since_rest: usize = 0;

    for beat in 0..grid.num_beats {
        // Skip structural cells — they're already placed
        if structural.contains(&(voice.index(), beat)) {
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
                beats_since_rest += 1;
            }
            continue;
        }

        // Skip non-rest cells that are continuations
        let cell = grid.cell(voice, beat);
        if !cell.is_rest && !cell.attack {
            beats_since_rest += 1;
            continue;
        }

        let beat_in_bar = beat % 8; // position within a 4/4 bar

        // Breathing: insert rests for phrase structure
        let should_rest = if last_pitch.is_none() {
            // Stagger voice entries: each voice enters a few beats apart
            let voice_delay = voice.index() * 4 + rng.random_range(0..4);
            beat < voice_delay
        } else if beats_since_rest > 24 && beat_in_bar == 0 {
            // After ~3 bars, take a breath on a downbeat
            rng.random_bool(0.4)
        } else {
            rng.random_bool(0.03)
        };

        if should_rest {
            beats_since_rest = 0;
            continue;
        }

        // Sample a pitch
        let pitch = if let Some(prev) = last_pitch {
            let rng_val: f64 = rng.random();
            let proposed_interval = models.melodic.sample(&recent_intervals, rng_val);
            let raw_pitch = (prev as i16 + proposed_interval as i16)
                .clamp(range_low as i16, range_high as i16) as u8;
            let proposed_pitch = mode.snap_to_mode(raw_pitch);

            // Try several alternatives, pick best
            let mut best_pitch = proposed_pitch;
            let mut best_score = pitch_score(grid, voice, beat, proposed_pitch, mode);

            for _ in 0..4 {
                let alt_rng: f64 = rng.random();
                let alt_interval = models.melodic.sample(&recent_intervals, alt_rng);
                let raw = (prev as i16 + alt_interval as i16)
                    .clamp(range_low as i16, range_high as i16) as u8;
                let alt_pitch = mode.snap_to_mode(raw);
                let alt_score = pitch_score(grid, voice, beat, alt_pitch, mode);
                if alt_score > best_score {
                    best_score = alt_score;
                    best_pitch = alt_pitch;
                }
            }

            best_pitch
        } else {
            // First note: start on a structurally important mode degree
            let mid = (range_low + range_high) / 2;
            // Prefer the final or 5th
            let candidates = mode.pitches_in_range(mid.saturating_sub(3), mid + 3);
            if candidates.is_empty() {
                mode.snap_to_mode(mid)
            } else {
                // Weight toward final and 5th
                let weights: Vec<f64> = candidates.iter()
                    .map(|&p| mode.pitch_fitness(p))
                    .collect();
                let total: f64 = weights.iter().sum();
                let r: f64 = rng.random::<f64>() * total;
                let mut cum = 0.0;
                let mut chosen = candidates[0];
                for (i, &w) in weights.iter().enumerate() {
                    cum += w;
                    if cum > r {
                        chosen = candidates[i];
                        break;
                    }
                }
                chosen
            }
        };

        grid.set_note(voice, beat, pitch);

        // Variable note duration based on metric position and style
        let hold_beats = match beat_in_bar {
            0 => rng.random_range(2..=5),       // downbeat: half to dotted half
            4 => rng.random_range(1..=3),       // beat 3: quarter to dotted quarter
            2 | 6 => rng.random_range(1..=2),   // weak beats: eighth to quarter
            _ => if rng.random_bool(0.6) { 1 } else { 0 },  // off-beats
        };

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
        beats_since_rest += 1;
    }
}

/// Combined score for a proposed pitch at a given position.
/// Considers harmonic compatibility with other voices AND modal fitness.
fn pitch_score(
    grid: &Grid,
    voice: Voice,
    beat: usize,
    proposed_pitch: u8,
    mode: &ModeInstance,
) -> f64 {
    let mut score = 0.0;

    // Modal fitness
    score += mode.pitch_fitness(proposed_pitch) * 2.0;

    // Harmonic compatibility with other voices
    for other_voice in Voice::ALL {
        if other_voice == voice {
            continue;
        }

        if let Some(other_pitch) = grid.sounding_pitch(other_voice, beat) {
            let iv = interval::semitones(other_pitch, proposed_pitch);

            if interval::is_consonant(iv) {
                score += 2.0;
            } else {
                score -= 1.5;
            }

            let is_strong = beat % 4 == 0;
            if is_strong && interval::is_perfect_consonance(iv) {
                score += 1.0;
            }

            let abs_iv = iv.unsigned_abs();
            if abs_iv > 24 {
                score -= 1.0;
            }
        }
    }

    score
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
        let mode = ModeInstance::d_dorian();
        let mut rng = rand::rng();

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

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
    fn test_draft_respects_mode() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mode = ModeInstance::d_dorian();
        let mut rng = rand::rng();

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        // Count how many non-structural attack notes are in mode
        let structural_set: std::collections::HashSet<(usize, usize)> =
            structural.iter().copied().collect();

        let mut in_mode = 0;
        let mut total = 0;
        for voice in Voice::ALL {
            for beat in 0..grid.num_beats {
                if structural_set.contains(&(voice.index(), beat)) {
                    continue;
                }
                let cell = grid.cell(voice, beat);
                if cell.attack && !cell.is_rest {
                    total += 1;
                    if mode.is_in_mode(cell.pitch) {
                        in_mode += 1;
                    }
                }
            }
        }
        let pct = if total > 0 { in_mode as f64 / total as f64 } else { 0.0 };
        assert!(pct > 0.85, "At least 85% of draft notes should be in mode, got {:.0}%", pct * 100.0);
    }
}
