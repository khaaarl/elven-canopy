// Scoring function: multi-layer evaluation of score quality.
//
// Evaluates a complete or partial grid against counterpoint rules and
// aesthetic preferences. The score is a weighted sum of penalties/rewards
// across several layers:
//
// Layer 1 (Hard rules, high weight): Parallel 5ths/8ves, hidden 5ths/8ves,
//   strong-beat dissonance (with suspension exemption), voice crossing,
//   range violations. Properly prepared suspensions are detected and
//   rewarded rather than penalized.
// Layer 2 (Melodic prefs, medium weight): Stepwise motion, leap recovery,
//   direction variety, climax uniqueness, arch contour.
// Layer 3 (Harmonic prefs, medium weight): Consonance preference, voice
//   spacing, interval variety (penalizes monotonous intervals).
// Layer 4 (Global, medium weight): Opening/closing consonance, cadence
//   detection and reward, rhythmic independence (penalizes homorhythm),
//   melodic contour shape.
// Layer 5 (Modal, medium weight): Mode compliance and degree-weighted
//   fitness.
// Ensemble texture: Rewards varied voice density across the piece
//   (mix of 2, 3, and 4 active voices). Penalizes prolonged thin textures.
// Tension curve: Rewards arc-shaped tension profiles that peak around
//   55-75% through the piece, measured via pitch height and dissonance.
// Interval distribution: Penalizes melodic interval profiles that deviate
//   from Palestrina norms (~55% steps, ~22% 3rds, ~15% 4ths/5ths).
// Layer 6 (Tonal contour, high weight): Vaelith tone system enforcement.
//   Each syllable's tone (level/rising/falling/dipping/peaking) constrains
//   the pitch movement within its grid span. Scored via separate
//   score_tonal_contour() with TextMapping from text_mapping.rs.
//   Also scores stressed syllable metric placement (root syllables on
//   strong beats).
//
// Scoring is designed for incremental updates: score_local() evaluates a
// small window around a mutation for efficient SA evaluation. Full
// score_grid() is used for global quality measurement. Tonal contour
// scoring uses score_tonal_contour_local() for per-span evaluation.
//
// Consumed by sa.rs for simulated annealing refinement.

use crate::grid::{Grid, Voice, interval};
use crate::mode::ModeInstance;
use crate::text_mapping::TextMapping;
use crate::vaelith::Tone;

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

    // Layer 5: Modal compliance
    pub out_of_mode_penalty: f64,
    pub mode_degree_reward: f64,

    // Suspension reward (a properly prepared suspension is idiomatic, reward it)
    pub suspension_reward: f64,

    // Rhythmic independence
    pub homorhythm_penalty: f64,

    // Hidden 5ths/octaves (lighter than parallel)
    pub hidden_fifths: f64,
    pub hidden_octaves: f64,

    // Contour: reward arch shapes, penalize repeated climax
    pub climax_repeat_penalty: f64,
    pub arch_contour_reward: f64,

    // Interval variety: penalize monotonous voice-pair intervals
    pub interval_monotony_penalty: f64,

    // Layer 6: Tonal contour constraints (Vaelith text)
    pub tonal_contour_violation: f64,
    pub tonal_contour_reward: f64,

    // Stressed syllable metric placement
    pub stressed_on_strong_beat: f64,
    pub stressed_on_weak_beat: f64,

    // Ensemble texture
    pub texture_variety_reward: f64,
    pub thin_texture_penalty: f64,

    // Tension curve
    pub tension_curve_reward: f64,

    // Interval distribution: penalty for deviating from Palestrina norms
    pub interval_distribution_penalty: f64,
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

            // Modal compliance
            out_of_mode_penalty: -8.0,
            mode_degree_reward: 1.0,

            // Suspensions
            suspension_reward: 5.0,

            // Rhythmic independence
            homorhythm_penalty: -4.0,

            // Hidden 5ths/octaves (lighter than parallel motion)
            hidden_fifths: -15.0,
            hidden_octaves: -15.0,

            // Contour
            climax_repeat_penalty: -3.0,
            arch_contour_reward: 5.0,

            // Interval variety
            interval_monotony_penalty: -2.0,

            // Tonal contour (high weight — Vaelith tone system)
            tonal_contour_violation: -20.0,
            tonal_contour_reward: 3.0,

            // Stressed syllable metric placement
            stressed_on_strong_beat: 4.0,
            stressed_on_weak_beat: -3.0,

            // Ensemble texture: reward varied voice count, penalize too thin
            texture_variety_reward: 8.0,
            thin_texture_penalty: -2.0,

            // Tension curve: reward proper arc shape
            tension_curve_reward: 15.0,

            // Interval distribution
            interval_distribution_penalty: -0.5,
        }
    }
}

/// Full score for a grid.
pub fn score_grid(grid: &Grid, weights: &ScoringWeights, mode: &ModeInstance) -> f64 {
    let mut total = 0.0;

    total += score_hard_rules(grid, weights);
    total += score_melodic(grid, weights);
    total += score_harmonic(grid, weights);
    total += score_global(grid, weights);
    total += score_modal(grid, weights, mode);
    total += score_texture(grid, weights);
    total += score_tension_curve(grid, weights);
    total += score_interval_distribution(grid, weights);

    total
}

/// Per-layer scoring breakdown for diagnostic output.
pub struct ScoreBreakdown {
    pub hard_rules: f64,
    pub melodic: f64,
    pub harmonic: f64,
    pub global: f64,
    pub modal: f64,
    pub texture: f64,
    pub tension_curve: f64,
    pub interval_dist: f64,
    pub tonal_contour: f64,
    pub total: f64,
}

/// Compute a per-layer scoring breakdown for display.
pub fn score_breakdown(
    grid: &Grid,
    weights: &ScoringWeights,
    mode: &ModeInstance,
    mapping: &TextMapping,
) -> ScoreBreakdown {
    let hard_rules = score_hard_rules(grid, weights);
    let melodic = score_melodic(grid, weights);
    let harmonic = score_harmonic(grid, weights);
    let global = score_global(grid, weights);
    let modal = score_modal(grid, weights, mode);
    let texture = score_texture(grid, weights);
    let tension_curve = score_tension_curve(grid, weights);
    let interval_dist = score_interval_distribution(grid, weights);
    let tonal_contour = score_tonal_contour(grid, mapping, weights);

    let total = hard_rules + melodic + harmonic + global + modal
        + texture + tension_curve + interval_dist + tonal_contour;

    ScoreBreakdown {
        hard_rules, melodic, harmonic, global, modal,
        texture, tension_curve, interval_dist, tonal_contour, total,
    }
}

/// Score contribution from a local window around a specific beat.
/// Used for incremental scoring after a mutation.
pub fn score_local(grid: &Grid, weights: &ScoringWeights, mode: &ModeInstance, beat: usize) -> f64 {
    let window_start = beat.saturating_sub(2);
    let window_end = (beat + 3).min(grid.num_beats);

    let mut total = 0.0;

    for b in window_start..window_end {
        total += score_beat_hard_rules(grid, weights, b);
        total += score_beat_harmonic(grid, weights, b);
        total += score_beat_modal(grid, weights, mode, b);
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

                // Strong-beat dissonance — but allow properly prepared suspensions.
                // A suspension is: the dissonant note was held (not attacked) from
                // a consonance on the previous beat, and resolves by step down.
                if is_strong && interval::is_dissonant(iv) {
                    if is_prepared_suspension(grid, vi, vj, beat) {
                        // Reward prepared suspensions — they're idiomatic Palestrina
                        score += weights.suspension_reward;
                    } else {
                        score += weights.strong_beat_dissonance;
                    }
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
                            // Both voices move in same direction
                            let motion_i = pi as i16 - ppi as i16;
                            let motion_j = pj as i16 - ppj as i16;
                            let same_direction = (motion_i > 0 && motion_j > 0)
                                || (motion_i < 0 && motion_j < 0);

                            if same_direction {
                                let curr_ic = (curr_iv.unsigned_abs()) % 12;
                                let prev_ic = (prev_iv.unsigned_abs()) % 12;

                                // Parallel 5ths: 5th → 5th by parallel motion
                                if curr_ic == 7 && prev_ic == 7 {
                                    score += weights.parallel_fifths;
                                }
                                // Parallel octaves/unisons: 8ve → 8ve by parallel motion
                                if curr_ic == 0 && prev_ic == 0 {
                                    score += weights.parallel_octaves;
                                }

                                // Hidden (direct) 5ths/8ves: arriving at a perfect
                                // consonance by similar motion from a non-perfect one.
                                // Lighter penalty than parallel.
                                if curr_ic == 7 && prev_ic != 7 {
                                    score += weights.hidden_fifths;
                                }
                                if curr_ic == 0 && prev_ic != 0 {
                                    score += weights.hidden_octaves;
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

/// Check if a dissonance between two voices at a beat is a properly prepared suspension.
///
/// A prepared suspension has three conditions:
/// 1. Preparation: the suspended note was consonant with the other voice on the previous beat
/// 2. Suspension: the note is HELD (not attacked) into the current beat, forming the dissonance
/// 3. Resolution: the suspended voice resolves by step downward on the next beat
///
/// Either voice can be the suspended one; we check both directions.
fn is_prepared_suspension(grid: &Grid, vi: Voice, vj: Voice, beat: usize) -> bool {
    // Need previous beat for preparation and next beat for resolution
    if beat == 0 || beat + 1 >= grid.num_beats {
        return false;
    }

    // Check if vi is the suspended voice
    if check_suspension_voice(grid, vi, vj, beat) {
        return true;
    }
    // Check if vj is the suspended voice
    if check_suspension_voice(grid, vj, vi, beat) {
        return true;
    }

    false
}

/// Check if `suspended` voice is held into a dissonance against `moving` voice at `beat`.
fn check_suspension_voice(grid: &Grid, suspended: Voice, moving: Voice, beat: usize) -> bool {
    let cell_sus = grid.cell(suspended, beat);
    let cell_mov = grid.cell(moving, beat);

    // The suspended voice must NOT attack on this beat (it's held from before)
    if cell_sus.attack || cell_sus.is_rest {
        return false;
    }
    // The moving voice must have attacked (it moved against the held note)
    if !cell_mov.attack || cell_mov.is_rest {
        return false;
    }

    // Preparation: on the previous beat, both voices were sounding and consonant
    let prev_sus = grid.sounding_pitch(suspended, beat - 1);
    let prev_mov = grid.sounding_pitch(moving, beat - 1);
    if let (Some(ps), Some(pm)) = (prev_sus, prev_mov) {
        let prev_iv = interval::semitones(pm, ps);
        if !interval::is_consonant(prev_iv) {
            return false; // Preparation wasn't consonant
        }
    } else {
        return false; // One voice was resting
    }

    // Resolution: the suspended voice steps down on the next beat
    let next_sus = grid.sounding_pitch(suspended, beat + 1);
    let curr_sus = cell_sus.pitch;
    if let Some(ns) = next_sus {
        let resolution = curr_sus as i16 - ns as i16;
        // Should resolve down by 1-2 semitones
        if resolution >= 1 && resolution <= 2 {
            // The next beat should be an attack (the suspension resolves)
            let next_cell = grid.cell(suspended, beat + 1);
            if next_cell.attack {
                return true;
            }
        }
    }

    false
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
    let mut prev_interval_abs: u16 = 0; // absolute size of previous interval
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
                // should be in the opposite direction by step.
                // If the previous interval was a leap and this interval
                // doesn't move in the opposite direction by step, penalize.
                if prev_interval_abs > 4 {
                    let recovered = direction != 0
                        && direction != prev_dir
                        && abs_iv <= 2;
                    if !recovered {
                        score += weights.leap_recovery_penalty;
                    }
                }
            }

            prev_interval_abs = abs_iv;
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

    // Interval variety: check that voice pairs don't stay at the same interval
    // for too many consecutive beats.
    score += score_interval_variety(grid, weights);

    score
}

/// Penalize monotonous intervals between voice pairs.
/// If the same interval class persists for 8+ beats, it sounds static.
fn score_interval_variety(grid: &Grid, weights: &ScoringWeights) -> f64 {
    let mut score = 0.0;

    for i in 0..4 {
        for j in (i + 1)..4 {
            let vi = Voice::ALL[i];
            let vj = Voice::ALL[j];

            let mut same_count = 0;
            let mut prev_ic: Option<u8> = None;

            for beat in 0..grid.num_beats {
                let pi = grid.sounding_pitch(vi, beat);
                let pj = grid.sounding_pitch(vj, beat);

                if let (Some(a), Some(b)) = (pi, pj) {
                    let ic = interval::interval_class(a, b);
                    if Some(ic) == prev_ic {
                        same_count += 1;
                        if same_count > 8 {
                            score += weights.interval_monotony_penalty;
                        }
                    } else {
                        same_count = 0;
                    }
                    prev_ic = Some(ic);
                } else {
                    same_count = 0;
                    prev_ic = None;
                }
            }
        }
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

// ── Layer 4: Global & Cadence ──

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

    // Cadence detection
    score += score_cadences(grid, weights);

    // Rhythmic independence
    score += score_rhythmic_independence(grid, weights);

    // Melodic contour: arch shapes and climax uniqueness
    score += score_contour(grid, weights);

    score
}

/// Detect phrase boundaries and score cadential motion there.
fn score_cadences(grid: &Grid, _weights: &ScoringWeights) -> f64 {
    let mut score = 0.0;

    // Find beats where a rest follows sounding notes (phrase endings)
    for beat in 2..grid.num_beats.saturating_sub(1) {
        // Check if this beat is near a phrase boundary:
        // at least 2 voices have a rest within 1-2 beats after this point
        let mut voices_resting_soon = 0;
        for voice in Voice::ALL {
            let has_rest_ahead = (1..=2).any(|offset| {
                let b = beat + offset;
                b < grid.num_beats && grid.cell(voice, b).is_rest
            });
            if has_rest_ahead {
                voices_resting_soon += 1;
            }
        }

        if voices_resting_soon < 2 {
            continue;
        }

        // This beat is near a phrase boundary. Check cadential motion.

        // Get soprano and bass pitches at this beat and the previous beat
        let sop_now = grid.sounding_pitch(Voice::Soprano, beat);
        let sop_prev = if beat > 0 { grid.sounding_pitch(Voice::Soprano, beat - 1) } else { None };
        let bass_now = grid.sounding_pitch(Voice::Bass, beat);
        let bass_prev = if beat > 0 { grid.sounding_pitch(Voice::Bass, beat - 1) } else { None };

        if let (Some(sn), Some(sp), Some(bn), Some(bp)) = (sop_now, sop_prev, bass_now, bass_prev) {
            let sop_motion = sn as i16 - sp as i16;
            let bass_motion = bn as i16 - bp as i16;

            // Contrary motion between soprano and bass
            if (sop_motion > 0 && bass_motion < 0) || (sop_motion < 0 && bass_motion > 0) {
                score += 3.0;
            }

            // Soprano moving by step (1-2 semitones)
            if sop_motion.unsigned_abs() <= 2 && sop_motion != 0 {
                score += 2.0;
            }

            // Bass moving by 4th or 5th (5 or 7 semitones)
            let bass_abs = bass_motion.unsigned_abs();
            if bass_abs == 5 || bass_abs == 7 {
                score += 4.0;
            }

            // Final beat of cadence lands on perfect consonance
            let iv = interval::semitones(bn, sn);
            if interval::is_perfect_consonance(iv) {
                score += 3.0;
            }
        }
    }

    score
}

/// Score melodic contour for each voice.
/// Rewards arch-shaped contours (rise to climax, then descent) and penalizes
/// the highest note occurring too many times (climax should be special).
fn score_contour(grid: &Grid, weights: &ScoringWeights) -> f64 {
    let mut score = 0.0;

    for voice in Voice::ALL {
        // Collect all attack pitches with their beat positions
        let mut notes: Vec<(usize, u8)> = Vec::new();
        for beat in 0..grid.num_beats {
            let cell = grid.cell(voice, beat);
            if cell.attack && !cell.is_rest {
                notes.push((beat, cell.pitch));
            }
        }

        if notes.len() < 4 {
            continue;
        }

        // Climax uniqueness: the highest pitch should appear rarely
        let max_pitch = notes.iter().map(|&(_, p)| p).max().unwrap();
        let climax_count = notes.iter().filter(|&&(_, p)| p == max_pitch).count();
        if climax_count > 3 {
            score += weights.climax_repeat_penalty * (climax_count as f64 - 3.0);
        }

        // Arch contour: the climax should be roughly in the middle 40-80% of the piece.
        // Find where the climax occurs as a fraction of the total.
        let first_climax_beat = notes.iter()
            .find(|&&(_, p)| p == max_pitch)
            .map(|&(b, _)| b)
            .unwrap();
        let total_span = notes.last().unwrap().0 - notes.first().unwrap().0;
        if total_span > 0 {
            let climax_pos = (first_climax_beat - notes.first().unwrap().0) as f64
                / total_span as f64;
            // Ideal: climax between 30% and 70% through the phrase
            if climax_pos > 0.3 && climax_pos < 0.7 {
                score += weights.arch_contour_reward;
            }
        }
    }

    score
}

/// Score rhythmic independence between voices.
/// Penalizes when 3+ voices attack on the same beat for many consecutive beats.
fn score_rhythmic_independence(grid: &Grid, weights: &ScoringWeights) -> f64 {
    let mut score = 0.0;
    let mut consecutive_homorhythm = 0;

    for beat in 0..grid.num_beats {
        let attacks: usize = Voice::ALL.iter()
            .filter(|&&v| {
                let c = grid.cell(v, beat);
                c.attack && !c.is_rest
            })
            .count();

        if attacks >= 3 {
            consecutive_homorhythm += 1;
            // Start penalizing after 4 consecutive homorhythmic beats
            if consecutive_homorhythm > 4 {
                score += weights.homorhythm_penalty;
            }
        } else {
            consecutive_homorhythm = 0;
        }
    }

    score
}

// ── Layer 5: Modal compliance ──

fn score_modal(grid: &Grid, weights: &ScoringWeights, mode: &ModeInstance) -> f64 {
    let mut score = 0.0;
    for beat in 0..grid.num_beats {
        score += score_beat_modal(grid, weights, mode, beat);
    }
    score
}

fn score_beat_modal(grid: &Grid, weights: &ScoringWeights, mode: &ModeInstance, beat: usize) -> f64 {
    let mut score = 0.0;

    for voice in Voice::ALL {
        if let Some(pitch) = grid.sounding_pitch(voice, beat) {
            if mode.is_in_mode(pitch) {
                // Reward structurally important degrees more
                score += mode.pitch_fitness(pitch) * weights.mode_degree_reward;
            } else {
                score += weights.out_of_mode_penalty;
            }
        }
    }

    score
}

// ── Ensemble texture scoring ──

/// Score texture variety: reward pieces that vary the number of active voices
/// across the piece. Penalize extremely thin textures (1 voice for too long).
/// Reward variety in voice density (a mix of 2, 3, and 4 voices).
fn score_texture(grid: &Grid, weights: &ScoringWeights) -> f64 {
    let mut score = 0.0;
    let mut density_counts = [0u32; 5]; // index = number of active voices (0-4)
    let mut consecutive_thin = 0u32;

    for beat in 0..grid.num_beats {
        let active = Voice::ALL.iter()
            .filter(|&&v| grid.sounding_pitch(v, beat).is_some())
            .count();
        density_counts[active] += 1;

        if active <= 1 {
            consecutive_thin += 1;
            if consecutive_thin > 6 {
                score += weights.thin_texture_penalty;
            }
        } else {
            consecutive_thin = 0;
        }
    }

    // Reward variety: count how many different densities (2, 3, 4) are used
    let distinct_densities = density_counts[2..=4].iter().filter(|&&c| c > 0).count();
    if distinct_densities >= 3 {
        score += weights.texture_variety_reward;
    } else if distinct_densities >= 2 {
        score += weights.texture_variety_reward * 0.5;
    }

    score
}

// ── Tension curve scoring ──

/// Score the overall tension curve of the piece.
///
/// Palestrina-style pieces typically build tension through the middle and
/// release toward the end. Tension is measured by: average pitch height,
/// dissonance density, and voice density. The ideal curve peaks around
/// 55-75% through the piece.
fn score_tension_curve(grid: &Grid, weights: &ScoringWeights) -> f64 {
    if grid.num_beats < 16 {
        return 0.0;
    }

    // Divide the piece into 8 segments
    let seg_len = grid.num_beats / 8;
    if seg_len == 0 {
        return 0.0;
    }

    let mut segment_tension: Vec<f64> = Vec::new();

    for seg in 0..8 {
        let start = seg * seg_len;
        let end = if seg == 7 { grid.num_beats } else { (seg + 1) * seg_len };

        let mut total_pitch = 0u32;
        let mut pitch_count = 0u32;
        let mut dissonance_count = 0u32;
        let mut pair_count = 0u32;

        for beat in start..end {
            let slice = grid.vertical_slice(beat);
            let pitches: Vec<u8> = slice.iter().filter_map(|&p| p).collect();

            for &p in &pitches {
                total_pitch += p as u32;
                pitch_count += 1;
            }

            // Count dissonant pairs
            for i in 0..pitches.len() {
                for j in (i + 1)..pitches.len() {
                    let iv = interval::semitones(pitches[j], pitches[i]);
                    pair_count += 1;
                    if interval::is_dissonant(iv) {
                        dissonance_count += 1;
                    }
                }
            }
        }

        if pitch_count == 0 {
            segment_tension.push(0.0);
            continue;
        }

        // Tension = normalized pitch height + dissonance ratio
        let avg_pitch = total_pitch as f64 / pitch_count as f64;
        let pitch_tension = (avg_pitch - 48.0) / 36.0; // normalize ~C3-C6 range
        let diss_ratio = if pair_count > 0 {
            dissonance_count as f64 / pair_count as f64
        } else {
            0.0
        };

        segment_tension.push(pitch_tension + diss_ratio * 2.0);
    }

    // Find the peak segment
    let peak_seg = segment_tension.iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(0);

    // Ideal peak: segments 4-5 (55-75% through the piece)
    let peak_pos = peak_seg as f64 / 7.0;
    let mut score = 0.0;

    if peak_pos >= 0.4 && peak_pos <= 0.8 {
        score += weights.tension_curve_reward;
    } else if peak_pos >= 0.3 && peak_pos <= 0.9 {
        score += weights.tension_curve_reward * 0.5;
    }

    // Bonus: tension should generally increase before the peak and decrease after
    let rising_before = (1..=peak_seg)
        .filter(|&i| segment_tension[i] >= segment_tension[i - 1])
        .count();
    let falling_after = (peak_seg + 1..segment_tension.len())
        .filter(|&i| segment_tension[i] <= segment_tension[i - 1])
        .count();

    let total_before = peak_seg.max(1);
    let total_after = (segment_tension.len() - peak_seg - 1).max(1);

    let arc_quality = (rising_before as f64 / total_before as f64
        + falling_after as f64 / total_after as f64) / 2.0;

    // Reward good arc shape (0.5 = random, 1.0 = perfect arc)
    if arc_quality > 0.6 {
        score += weights.tension_curve_reward * 0.5 * (arc_quality - 0.5);
    }

    score
}

// ── Interval distribution scoring ──

/// Score how well melodic intervals match Palestrina's empirical distribution.
///
/// Palestrina's music is dominated by stepwise motion (2nds ~60%), with
/// some 3rds (~20%), occasional 4ths/5ths (~12%), and rare larger leaps (~8%).
/// This function penalizes distributions that deviate significantly.
fn score_interval_distribution(grid: &Grid, weights: &ScoringWeights) -> f64 {
    // Target distribution (proportion of intervals in each category)
    // Based on empirical analysis of Palestrina masses
    const TARGET_STEP: f64 = 0.55;     // unison + 2nds (0-2 semitones)
    const TARGET_THIRD: f64 = 0.22;    // 3rds (3-4 semitones)
    const TARGET_FOURTH_FIFTH: f64 = 0.15; // 4ths-5ths (5-7 semitones)
    // remainder = larger leaps (~8%)

    let mut step_count = 0u32;
    let mut third_count = 0u32;
    let mut fourth_fifth_count = 0u32;
    let mut total = 0u32;

    for voice in Voice::ALL {
        let mut prev_pitch: Option<u8> = None;
        for beat in 0..grid.num_beats {
            let cell = grid.cell(voice, beat);
            if cell.attack && !cell.is_rest {
                if let Some(prev) = prev_pitch {
                    let abs_iv = (cell.pitch as i16 - prev as i16).unsigned_abs();
                    total += 1;
                    match abs_iv {
                        0..=2 => step_count += 1,
                        3..=4 => third_count += 1,
                        5..=7 => fourth_fifth_count += 1,
                        _ => {}
                    }
                }
                prev_pitch = Some(cell.pitch);
            }
        }
    }

    if total < 20 {
        return 0.0; // Not enough data for meaningful comparison
    }

    let total_f = total as f64;
    let step_ratio = step_count as f64 / total_f;
    let third_ratio = third_count as f64 / total_f;
    let fourth_fifth_ratio = fourth_fifth_count as f64 / total_f;

    // Penalize deviation from target proportions
    let step_dev = (step_ratio - TARGET_STEP).abs();
    let third_dev = (third_ratio - TARGET_THIRD).abs();
    let fourth_dev = (fourth_fifth_ratio - TARGET_FOURTH_FIFTH).abs();

    // Weight step motion deviation more heavily (it's most important)
    let total_deviation = step_dev * 2.0 + third_dev + fourth_dev;

    // Only penalize if deviation is substantial (> 0.15)
    if total_deviation > 0.15 {
        weights.interval_distribution_penalty * (total_deviation - 0.15) * total_f
    } else {
        0.0
    }
}

// ── Layer 6: Tonal contour constraints (Vaelith text) ──

/// Score tonal contour compliance for all syllable spans in the mapping.
///
/// Each syllable has a fixed tone (level, rising, falling, dipping, peaking)
/// that constrains pitch movement within its grid span. This function checks
/// each span and rewards/penalizes based on contour match.
///
/// This is separate from score_grid() because it requires the TextMapping.
/// Call it alongside score_grid() in the main scoring pipeline.
pub fn score_tonal_contour(
    grid: &Grid,
    mapping: &TextMapping,
    weights: &ScoringWeights,
) -> f64 {
    let mut score = 0.0;
    for span in &mapping.spans {
        score += score_span_contour(grid, span, weights);

        // Stressed syllable metric placement
        if span.stressed {
            let is_strong_beat = span.start_beat % 4 == 0;
            if is_strong_beat {
                score += weights.stressed_on_strong_beat;
            } else {
                score += weights.stressed_on_weak_beat;
            }
        }
    }
    score
}

/// Score tonal contour locally — only check spans that overlap with the given beat.
/// Used for incremental scoring after a pitch mutation.
pub fn score_tonal_contour_local(
    grid: &Grid,
    mapping: &TextMapping,
    weights: &ScoringWeights,
    beat: usize,
) -> f64 {
    let mut score = 0.0;
    for span in &mapping.spans {
        if beat >= span.start_beat && beat <= span.end_beat {
            score += score_span_contour(grid, span, weights);
        }
    }
    score
}

/// Statistics about tonal contour compliance.
#[derive(Debug)]
pub struct TonalContourStats {
    pub total_spans: usize,
    pub compliant: usize,
    pub violated: usize,
    pub stressed_on_strong: usize,
    pub stressed_on_weak: usize,
    pub total_stressed: usize,
}

/// Compute tonal contour compliance statistics for display.
pub fn tonal_contour_stats(
    grid: &Grid,
    mapping: &TextMapping,
) -> TonalContourStats {
    let mut stats = TonalContourStats {
        total_spans: mapping.spans.len(),
        compliant: 0,
        violated: 0,
        stressed_on_strong: 0,
        stressed_on_weak: 0,
        total_stressed: 0,
    };

    let weights = ScoringWeights::default();

    for span in &mapping.spans {
        let contour_score = score_span_contour(grid, span, &weights);
        if contour_score > 0.0 {
            stats.compliant += 1;
        } else {
            stats.violated += 1;
        }

        if span.stressed {
            stats.total_stressed += 1;
            if span.start_beat % 4 == 0 {
                stats.stressed_on_strong += 1;
            } else {
                stats.stressed_on_weak += 1;
            }
        }
    }

    stats
}

/// Score a single syllable span's tonal contour.
fn score_span_contour(
    grid: &Grid,
    span: &crate::text_mapping::SyllableSpan,
    weights: &ScoringWeights,
) -> f64 {
    // Collect pitches within the span
    let mut pitches: Vec<u8> = Vec::new();
    for beat in span.start_beat..=span.end_beat {
        if beat >= grid.num_beats {
            break;
        }
        let cell = grid.cell(span.voice, beat);
        if !cell.is_rest {
            pitches.push(cell.pitch);
        }
    }

    if pitches.is_empty() {
        return 0.0;
    }

    // Check if the span has enough notes for the tone's minimum
    if pitches.len() < span.tone.min_notes() {
        // Not enough notes to realize the contour — mild penalty
        return weights.tonal_contour_violation * 0.5;
    }

    let first = pitches[0];
    let last = *pitches.last().unwrap();

    match span.tone {
        Tone::Level => {
            // All pitches should be the same
            if pitches.iter().all(|&p| p == first) {
                weights.tonal_contour_reward
            } else {
                weights.tonal_contour_violation
            }
        }
        Tone::Rising => {
            // First pitch < last pitch (net ascending)
            if first < last {
                weights.tonal_contour_reward
            } else {
                weights.tonal_contour_violation
            }
        }
        Tone::Falling => {
            // First pitch > last pitch (net descending)
            if first > last {
                weights.tonal_contour_reward
            } else {
                weights.tonal_contour_violation
            }
        }
        Tone::Dipping => {
            // Valley: some pitch in the middle is lower than both first and last
            if pitches.len() >= 3 {
                let min_middle = pitches[1..pitches.len() - 1].iter().min().copied().unwrap_or(first);
                if min_middle < first && min_middle < last {
                    weights.tonal_contour_reward
                } else {
                    weights.tonal_contour_violation
                }
            } else {
                // Only 2 notes but need 3 for dipping
                weights.tonal_contour_violation * 0.5
            }
        }
        Tone::Peaking => {
            // Hill: some pitch in the middle is higher than both first and last
            if pitches.len() >= 3 {
                let max_middle = pitches[1..pitches.len() - 1].iter().max().copied().unwrap_or(first);
                if max_middle > first && max_middle > last {
                    weights.tonal_contour_reward
                } else {
                    weights.tonal_contour_violation
                }
            } else {
                weights.tonal_contour_violation * 0.5
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_mode() -> ModeInstance {
        ModeInstance::d_dorian()
    }

    #[test]
    fn test_parallel_fifths_penalized() {
        let mut grid = Grid::new(2);
        let weights = ScoringWeights::default();
        let mode = default_mode();

        // Beat 0: Soprano C4 (60), Alto F3 (53) — perfect 5th
        // Beat 1: Soprano D4 (62), Alto G3 (55) — perfect 5th
        // Both move up by a whole step = parallel 5ths
        grid.set_note(Voice::Soprano, 0, 60);
        grid.set_note(Voice::Soprano, 1, 62);
        grid.set_note(Voice::Alto, 0, 53);
        grid.set_note(Voice::Alto, 1, 55);

        let score = score_grid(&grid, &weights, &mode);
        // The parallel fifths penalty (-50) should dominate
        assert!(score < 0.0, "Parallel fifths should produce negative score, got {}", score);
    }

    #[test]
    fn test_suspension_not_penalized() {
        // A properly prepared suspension: Alto holds a C4 from a consonance,
        // Soprano attacks D4 creating a dissonance, then Alto resolves down to B3.
        let mut grid = Grid::new(4);
        let _weights = ScoringWeights::default();
        let _mode = default_mode();

        // Beat 0: Soprano E4 (64), Alto C4 (60) — major 3rd (consonant) — preparation
        grid.set_note(Voice::Soprano, 0, 64);
        grid.set_note(Voice::Alto, 0, 60);

        // Beat 1: Alto holds C4, Soprano attacks D4 (62) — major 2nd (dissonant)
        grid.extend_note(Voice::Alto, 1); // Alto holds (not attacked)
        grid.set_note(Voice::Soprano, 1, 62); // Soprano attacks

        // Beat 2: Alto resolves to B3 (59) — step down — resolution
        grid.set_note(Voice::Alto, 2, 59);
        grid.set_note(Voice::Soprano, 2, 62); // Soprano holds or continues

        // Beat 1 is not a strong beat (strong = beat % 4 == 0), so let's adjust:
        // Use beat 0 for preparation, beat 4... but we only have 4 beats.
        // Instead, test the is_prepared_suspension function directly.
        let is_sus = is_prepared_suspension(&grid, Voice::Soprano, Voice::Alto, 1);
        assert!(is_sus, "Should detect prepared suspension (Alto held from consonance, resolves down)");
    }

    #[test]
    fn test_unprepared_dissonance_penalized() {
        let mut grid = Grid::new(4);
        let weights = ScoringWeights::default();
        let mode = default_mode();

        // Both voices attack into a dissonance on a strong beat = NOT a suspension
        // Beat 0 (strong): Soprano D4 (62), Alto C4 (60) — major 2nd
        grid.set_note(Voice::Soprano, 0, 62);
        grid.set_note(Voice::Alto, 0, 60);

        let score = score_grid(&grid, &weights, &mode);
        // Should include the strong-beat dissonance penalty
        assert!(score < 0.0, "Unprepared dissonance on strong beat should penalize, got {}", score);
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
    fn test_leap_recovery_penalized() {
        let mut grid = Grid::new(6);
        let weights = ScoringWeights::default();

        // Soprano: C4 (60), then leap up to A4 (69) = +9 semitones,
        // then continue upward to B4 (71) = no recovery (same direction)
        grid.set_note(Voice::Soprano, 0, 60);
        grid.set_note(Voice::Soprano, 2, 69);
        grid.set_note(Voice::Soprano, 4, 71);

        let no_recovery = score_melodic_window(&grid, &weights, Voice::Soprano, 0, 6);

        // Now: leap up, then step back down = proper recovery
        let mut grid2 = Grid::new(6);
        grid2.set_note(Voice::Soprano, 0, 60);
        grid2.set_note(Voice::Soprano, 2, 69);
        grid2.set_note(Voice::Soprano, 4, 67); // step down = recovery

        let with_recovery = score_melodic_window(&grid2, &weights, Voice::Soprano, 0, 6);

        assert!(with_recovery > no_recovery,
            "Leap with recovery ({:.1}) should score better than without ({:.1})",
            with_recovery, no_recovery);
    }

    #[test]
    fn test_tonal_contour_level_rewarded() {
        use crate::text_mapping::{TextMapping, SyllableSpan};

        let mut grid = Grid::new(4);
        let weights = ScoringWeights::default();

        // Level tone: all notes should be the same pitch
        grid.set_note(Voice::Soprano, 0, 62);
        grid.extend_note(Voice::Soprano, 1);
        grid.extend_note(Voice::Soprano, 2);

        let mapping = TextMapping {
            section_phrases: vec![],
            spans: vec![SyllableSpan {
                voice: Voice::Soprano,
                start_beat: 0,
                end_beat: 2,
                tone: Tone::Level,
                text: "thir".to_string(),
                stressed: true,
                syllable_id: 1,
            }],
        };

        let score = score_tonal_contour(&grid, &mapping, &weights);
        assert!(score > 0.0, "Level tone with held pitch should be rewarded, got {}", score);
    }

    #[test]
    fn test_tonal_contour_rising_penalized_when_falling() {
        use crate::text_mapping::{TextMapping, SyllableSpan};

        let mut grid = Grid::new(4);
        let weights = ScoringWeights::default();

        // Rising tone but pitch goes down
        grid.set_note(Voice::Soprano, 0, 67);
        grid.set_note(Voice::Soprano, 1, 64);

        let mapping = TextMapping {
            section_phrases: vec![],
            spans: vec![SyllableSpan {
                voice: Voice::Soprano,
                start_beat: 0,
                end_beat: 1,
                tone: Tone::Rising,
                text: "thír".to_string(),
                stressed: true,
                syllable_id: 1,
            }],
        };

        let score = score_tonal_contour(&grid, &mapping, &weights);
        assert!(score < 0.0, "Rising tone with falling pitch should be penalized, got {}", score);
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

    #[test]
    fn test_texture_variety_rewarded() {
        let mut grid = Grid::new(24);
        let weights = ScoringWeights::default();

        // Section with 2 voices (beats 0-7)
        grid.set_note(Voice::Soprano, 0, 67);
        grid.extend_note(Voice::Soprano, 1);
        grid.set_note(Voice::Alto, 0, 62);
        grid.extend_note(Voice::Alto, 1);

        // Section with 3 voices (beats 8-15)
        grid.set_note(Voice::Soprano, 8, 67);
        grid.set_note(Voice::Alto, 8, 62);
        grid.set_note(Voice::Tenor, 8, 55);

        // Section with 4 voices (beats 16-23)
        grid.set_note(Voice::Soprano, 16, 67);
        grid.set_note(Voice::Alto, 16, 62);
        grid.set_note(Voice::Tenor, 16, 55);
        grid.set_note(Voice::Bass, 16, 48);

        let score = score_texture(&grid, &weights);
        assert!(score > 0.0, "Varied texture (2/3/4 voices) should be rewarded, got {}", score);
    }

    #[test]
    fn test_interval_distribution_stepwise_good() {
        let mut grid = Grid::new(40);
        let weights = ScoringWeights::default();

        // Mostly stepwise soprano line (Palestrina-like)
        let pitches = [60, 62, 64, 62, 60, 62, 64, 65, 64, 62,
                       60, 62, 64, 62, 60, 62, 64, 65, 67, 65];
        for (i, &p) in pitches.iter().enumerate() {
            grid.set_note(Voice::Soprano, i * 2, p);
        }

        let score = score_interval_distribution(&grid, &weights);
        // Mostly steps = close to target, should have low or no penalty
        assert!(score >= -5.0, "Stepwise-dominated line should have mild distribution penalty, got {}", score);
    }
}
