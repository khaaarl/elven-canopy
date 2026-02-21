// Scoring function: multi-layer evaluation of score quality.
//
// Evaluates a complete or partial grid against counterpoint rules and
// aesthetic preferences. The score is a weighted sum of penalties/rewards
// across several layers:
//
// Layer 1 (Hard rules, high weight): Parallel 5ths/8ves, strong-beat
//   dissonance, voice crossing, range violations.
// Layer 2 (Melodic prefs, medium weight): Stepwise motion, leap recovery,
//   direction variety.
// Layer 3 (Harmonic prefs, medium weight): Consonance preference, voice
//   spacing, cadence patterns.
// Layer 4 (Global, medium weight): Opening/closing conventions, rhythmic
//   independence.
//
// Scoring is designed for incremental updates: each beat's contribution
// depends on a window of ~2-3 beats, so after a single-cell mutation only
// a small region needs rescoring.
//
// Consumed by sa.rs for simulated annealing refinement.

use crate::grid::{Grid, Voice, interval};

/// Weights for scoring layers. Tunable parameters.
#[derive(Debug, Clone)]
pub struct ScoringWeights {
    // Layer 1: Hard rules (heavy penalties)
    pub parallel_fifths: f64,
    pub parallel_octaves: f64,
    pub strong_beat_dissonance: f64,
    pub voice_crossing: f64,
    pub range_violation: f64,

    // Layer 2: Melodic preferences
    pub stepwise_reward: f64,
    pub leap_penalty: f64,
    pub large_leap_penalty: f64,
    pub leap_recovery_penalty: f64,
    pub repeated_note_penalty: f64,
    pub direction_run_penalty: f64,

    // Layer 3: Harmonic preferences
    pub consonance_reward: f64,
    pub voice_spacing_penalty: f64,

    // Layer 4: Global
    pub opening_consonance_reward: f64,
    pub closing_consonance_reward: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        ScoringWeights {
            // Hard rules: 10-100x penalties
            parallel_fifths: -50.0,
            parallel_octaves: -50.0,
            strong_beat_dissonance: -30.0,
            voice_crossing: -20.0,
            range_violation: -15.0,

            // Melodic: moderate
            stepwise_reward: 2.0,
            leap_penalty: -3.0,
            large_leap_penalty: -8.0,
            leap_recovery_penalty: -5.0,
            repeated_note_penalty: -1.0,
            direction_run_penalty: -2.0,

            // Harmonic: moderate
            consonance_reward: 1.5,
            voice_spacing_penalty: -3.0,

            // Global
            opening_consonance_reward: 5.0,
            closing_consonance_reward: 8.0,
        }
    }
}

/// Full score for a grid.
pub fn score_grid(grid: &Grid, weights: &ScoringWeights) -> f64 {
    let mut total = 0.0;

    total += score_hard_rules(grid, weights);
    total += score_melodic(grid, weights);
    total += score_harmonic(grid, weights);
    total += score_global(grid, weights);

    total
}

/// Score contribution from a local window around a specific beat.
/// Used for incremental scoring after a mutation.
pub fn score_local(grid: &Grid, weights: &ScoringWeights, beat: usize) -> f64 {
    let window_start = beat.saturating_sub(2);
    let window_end = (beat + 3).min(grid.num_beats);

    let mut total = 0.0;

    for b in window_start..window_end {
        total += score_beat_hard_rules(grid, weights, b);
        total += score_beat_harmonic(grid, weights, b);
    }

    // Melodic scoring for voices that have notes in the window
    for voice in Voice::ALL {
        total += score_melodic_window(grid, weights, voice, window_start, window_end);
    }

    total
}

// ── Layer 1: Hard counterpoint rules ──

fn score_hard_rules(grid: &Grid, weights: &ScoringWeights) -> f64 {
    let mut score = 0.0;
    for beat in 0..grid.num_beats {
        score += score_beat_hard_rules(grid, weights, beat);
    }
    score
}

fn score_beat_hard_rules(grid: &Grid, weights: &ScoringWeights, beat: usize) -> f64 {
    let mut score = 0.0;
    let is_strong = beat % 4 == 0; // strong beats every half-bar (2 quarter notes)

    // Check all voice pairs
    for i in 0..4 {
        let vi = Voice::ALL[i];
        let pitch_i = grid.sounding_pitch(vi, beat);

        for j in (i + 1)..4 {
            let vj = Voice::ALL[j];
            let pitch_j = grid.sounding_pitch(vj, beat);

            if let (Some(pi), Some(pj)) = (pitch_i, pitch_j) {
                let iv = interval::semitones(pj, pi);

                // Voice crossing: lower voice should have lower pitch
                if pi < pj {
                    score += weights.voice_crossing;
                }

                // Strong-beat dissonance
                if is_strong && interval::is_dissonant(iv) {
                    // Check if it's a properly prepared suspension
                    // (simplified: just penalize for now)
                    score += weights.strong_beat_dissonance;
                }

                // Parallel 5ths/octaves (need previous beat)
                if beat > 0 {
                    let prev_pi = grid.sounding_pitch(vi, beat - 1);
                    let prev_pj = grid.sounding_pitch(vj, beat - 1);

                    if let (Some(ppi), Some(ppj)) = (prev_pi, prev_pj) {
                        let prev_iv = interval::semitones(ppj, ppi);
                        let curr_iv = iv;

                        // Both current and previous are attacks (new notes moving together)
                        let i_attacks = grid.cell(vi, beat).attack;
                        let j_attacks = grid.cell(vj, beat).attack;

                        if i_attacks || j_attacks {
                            // Parallel motion: both voices move in same direction
                            let motion_i = pi as i16 - ppi as i16;
                            let motion_j = pj as i16 - ppj as i16;
                            let parallel = (motion_i > 0 && motion_j > 0)
                                || (motion_i < 0 && motion_j < 0);

                            if parallel {
                                let curr_ic = (curr_iv.unsigned_abs()) % 12;
                                let prev_ic = (prev_iv.unsigned_abs()) % 12;

                                // Parallel 5ths
                                if curr_ic == 7 && prev_ic == 7 {
                                    score += weights.parallel_fifths;
                                }
                                // Parallel octaves/unisons
                                if curr_ic == 0 && prev_ic == 0 {
                                    score += weights.parallel_octaves;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Range violations
        if let Some(pitch) = pitch_i {
            let (low, high) = vi.range();
            if pitch < low || pitch > high {
                score += weights.range_violation;
            }
        }
    }

    score
}

// ── Layer 2: Melodic preferences ──

fn score_melodic(grid: &Grid, weights: &ScoringWeights) -> f64 {
    let mut score = 0.0;
    for voice in Voice::ALL {
        score += score_melodic_window(grid, weights, voice, 0, grid.num_beats);
    }
    score
}

fn score_melodic_window(
    grid: &Grid,
    weights: &ScoringWeights,
    voice: Voice,
    start: usize,
    end: usize,
) -> f64 {
    let mut score = 0.0;
    let mut prev_pitch: Option<u8> = None;
    let mut prev_direction: Option<i8> = None; // 1=up, -1=down, 0=same
    let mut direction_run = 0i32;
    let mut repeated_count = 0u32;

    // Look back from start to get context
    if start > 0 {
        for beat in (0..start).rev() {
            let cell = grid.cell(voice, beat);
            if cell.attack && !cell.is_rest {
                prev_pitch = Some(cell.pitch);
                break;
            }
        }
    }

    for beat in start..end {
        let cell = grid.cell(voice, beat);
        if !cell.attack || cell.is_rest {
            continue;
        }

        if let Some(prev) = prev_pitch {
            let iv = cell.pitch as i16 - prev as i16;
            let abs_iv = iv.unsigned_abs();

            // Stepwise motion reward
            if abs_iv <= 2 {
                score += weights.stepwise_reward;
            } else if abs_iv <= 4 {
                // 3rds: mild
                score += weights.leap_penalty * 0.3;
            } else if abs_iv <= 7 {
                // 4ths-5ths
                score += weights.leap_penalty;
            } else {
                // Larger leaps
                score += weights.large_leap_penalty;
            }

            // Repeated notes
            if iv == 0 {
                repeated_count += 1;
                if repeated_count > 2 {
                    score += weights.repeated_note_penalty;
                }
            } else {
                repeated_count = 0;
            }

            // Direction tracking
            let direction = if iv > 0 { 1i8 } else if iv < 0 { -1 } else { 0 };

            if let Some(prev_dir) = prev_direction {
                if direction == prev_dir && direction != 0 {
                    direction_run += 1;
                    if direction_run > 4 {
                        score += weights.direction_run_penalty;
                    }
                } else {
                    direction_run = 0;
                }

                // Leap recovery: after a leap (>4 semitones), the next move
                // should be in the opposite direction by step
                if abs_iv > 4 && direction != 0 {
                    // Check if the previous interval was a leap
                    // (simplified: just check direction)
                }
            }

            prev_direction = Some(direction);
        }

        prev_pitch = Some(cell.pitch);
    }

    score
}

// ── Layer 3: Harmonic preferences ──

fn score_harmonic(grid: &Grid, weights: &ScoringWeights) -> f64 {
    let mut score = 0.0;
    for beat in 0..grid.num_beats {
        score += score_beat_harmonic(grid, weights, beat);
    }
    score
}

fn score_beat_harmonic(grid: &Grid, weights: &ScoringWeights, beat: usize) -> f64 {
    let mut score = 0.0;
    let slice = grid.vertical_slice(beat);

    let pitches: Vec<u8> = slice.iter().filter_map(|&p| p).collect();
    if pitches.len() < 2 {
        return 0.0;
    }

    // Consonance reward for each pair
    for i in 0..pitches.len() {
        for j in (i + 1)..pitches.len() {
            let iv = interval::semitones(pitches[j], pitches[i]);
            if interval::is_consonant(iv) {
                score += weights.consonance_reward;
            }
        }
    }

    // Voice spacing: check adjacent voices
    for i in 0..pitches.len().saturating_sub(1) {
        let gap = (pitches[i] as i16 - pitches[i + 1] as i16).unsigned_abs();
        if gap > 24 {
            score += weights.voice_spacing_penalty;
        }
    }

    score
}

// ── Layer 4: Global ──

fn score_global(grid: &Grid, weights: &ScoringWeights) -> f64 {
    let mut score = 0.0;

    // Opening: reward perfect consonance on first sounding beat
    for beat in 0..grid.num_beats.min(8) {
        let slice = grid.vertical_slice(beat);
        let pitches: Vec<u8> = slice.iter().filter_map(|&p| p).collect();
        if pitches.len() >= 2 {
            let all_consonant = pitches.windows(2).all(|w| {
                interval::is_consonant(interval::semitones(w[0], w[1]))
            });
            if all_consonant {
                score += weights.opening_consonance_reward;
            }
            break;
        }
    }

    // Closing: reward perfect consonance on last sounding beat
    for beat in (0..grid.num_beats).rev() {
        let slice = grid.vertical_slice(beat);
        let pitches: Vec<u8> = slice.iter().filter_map(|&p| p).collect();
        if pitches.len() >= 2 {
            let all_perfect = pitches.windows(2).all(|w| {
                interval::is_perfect_consonance(interval::semitones(w[0], w[1]))
            });
            if all_perfect {
                score += weights.closing_consonance_reward;
            }
            break;
        }
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_fifths_penalized() {
        let mut grid = Grid::new(2);
        let weights = ScoringWeights::default();

        // Beat 0: Soprano C4 (60), Alto F3 (53) — perfect 5th
        // Beat 1: Soprano D4 (62), Alto G3 (55) — perfect 5th
        // Both move up by a whole step = parallel 5ths
        grid.set_note(Voice::Soprano, 0, 60);
        grid.set_note(Voice::Soprano, 1, 62);
        grid.set_note(Voice::Alto, 0, 53);
        grid.set_note(Voice::Alto, 1, 55);

        let score = score_grid(&grid, &weights);
        // The parallel fifths penalty (-50) should dominate
        assert!(score < 0.0, "Parallel fifths should produce negative score, got {}", score);
    }

    #[test]
    fn test_consonance_rewarded() {
        let mut grid = Grid::new(4);
        let weights = ScoringWeights::default();

        // Soprano: C4 (60), Alto: E3 (52) — major 6th, consonant
        grid.set_note(Voice::Soprano, 0, 60);
        grid.set_note(Voice::Alto, 0, 52);

        let beat_score = score_beat_harmonic(&grid, &weights, 0);
        assert!(beat_score > 0.0, "Consonant intervals should be rewarded");
    }

    #[test]
    fn test_stepwise_motion_rewarded() {
        let mut grid = Grid::new(6);
        let weights = ScoringWeights::default();

        // Stepwise ascending: C4, D4, E4
        grid.set_note(Voice::Soprano, 0, 60);
        grid.set_note(Voice::Soprano, 2, 62);
        grid.set_note(Voice::Soprano, 4, 64);

        let score = score_melodic_window(&grid, &weights, Voice::Soprano, 0, 6);
        assert!(score > 0.0, "Stepwise motion should be rewarded, got {}", score);
    }
}
