// Scoring function: multi-layer evaluation of score quality.
//
// Evaluates a complete or partial grid against counterpoint rules and
// aesthetic preferences. The score is a weighted sum of penalties/rewards
// across several layers. All arithmetic uses `Score` (Fixed64 with 2^30
// fractional bits) for cross-platform determinism — no floating-point
// operations in any scoring path.
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
use crate::mode::{ModeInstance, Score};
use crate::text_mapping::TextMapping;
use crate::vaelith::Tone;

/// Weights for scoring layers. Tunable parameters.
/// All weights are `Score` (Fixed64) for deterministic arithmetic.
#[derive(Debug, Clone)]
pub struct ScoringWeights {
    // Layer 1: Hard rules (heavy penalties)
    pub parallel_fifths: Score,
    pub parallel_octaves: Score,
    pub strong_beat_dissonance: Score,
    pub voice_crossing: Score,
    pub range_violation: Score,

    // Layer 2: Melodic preferences
    pub stepwise_reward: Score,
    pub leap_penalty: Score,
    pub large_leap_penalty: Score,
    pub leap_recovery_penalty: Score,
    pub repeated_note_penalty: Score,
    pub direction_run_penalty: Score,

    // Layer 3: Harmonic preferences
    pub consonance_reward: Score,
    pub voice_spacing_penalty: Score,

    // Layer 4: Global
    pub opening_consonance_reward: Score,
    pub closing_consonance_reward: Score,

    // Layer 5: Modal compliance
    pub out_of_mode_penalty: Score,
    pub mode_degree_reward: Score,

    // Suspension reward (a properly prepared suspension is idiomatic, reward it)
    pub suspension_reward: Score,

    // Rhythmic independence
    pub homorhythm_penalty: Score,

    // Hidden 5ths/octaves (lighter than parallel)
    pub hidden_fifths: Score,
    pub hidden_octaves: Score,

    // Contour: reward arch shapes, penalize repeated climax
    pub climax_repeat_penalty: Score,
    pub arch_contour_reward: Score,

    // Interval variety: penalize monotonous voice-pair intervals
    pub interval_monotony_penalty: Score,

    // Layer 6: Tonal contour constraints (Vaelith text)
    pub tonal_contour_violation: Score,
    pub tonal_contour_reward: Score,

    // Stressed syllable metric placement
    pub stressed_on_strong_beat: Score,
    pub stressed_on_weak_beat: Score,

    // Ensemble texture
    pub texture_variety_reward: Score,
    pub thin_texture_penalty: Score,

    // Tension curve
    pub tension_curve_reward: Score,

    // Interval distribution: penalty for deviating from Palestrina norms
    pub interval_distribution_penalty: Score,

    // Melodic entropy: reward moderate information content, penalize extremes
    pub entropy_reward: Score,
    pub entropy_penalty: Score,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        ScoringWeights {
            // Hard rules: 10-100x penalties
            parallel_fifths: Score::from_int(-50),
            parallel_octaves: Score::from_int(-50),
            strong_beat_dissonance: Score::from_int(-30),
            voice_crossing: Score::from_int(-20),
            range_violation: Score::from_int(-15),

            // Melodic: moderate
            stepwise_reward: Score::from_int(2),
            leap_penalty: Score::from_int(-3),
            large_leap_penalty: Score::from_int(-8),
            leap_recovery_penalty: Score::from_int(-5),
            repeated_note_penalty: Score::from_int(-1),
            direction_run_penalty: Score::from_int(-2),

            // Harmonic: moderate
            consonance_reward: Score::from_ratio(3, 2), // 1.5
            voice_spacing_penalty: Score::from_int(-3),

            // Global
            opening_consonance_reward: Score::from_int(5),
            closing_consonance_reward: Score::from_int(8),

            // Modal compliance
            out_of_mode_penalty: Score::from_int(-8),
            mode_degree_reward: Score::ONE,

            // Suspensions
            suspension_reward: Score::from_int(5),

            // Rhythmic independence
            homorhythm_penalty: Score::from_int(-4),

            // Hidden 5ths/octaves (lighter than parallel motion)
            hidden_fifths: Score::from_int(-15),
            hidden_octaves: Score::from_int(-15),

            // Contour
            climax_repeat_penalty: Score::from_int(-3),
            arch_contour_reward: Score::from_int(5),

            // Interval variety
            interval_monotony_penalty: Score::from_int(-2),

            // Tonal contour (high weight — Vaelith tone system)
            tonal_contour_violation: Score::from_int(-20),
            tonal_contour_reward: Score::from_int(3),

            // Stressed syllable metric placement
            stressed_on_strong_beat: Score::from_int(4),
            stressed_on_weak_beat: Score::from_int(-3),

            // Ensemble texture: reward varied voice count, penalize too thin
            texture_variety_reward: Score::from_int(8),
            thin_texture_penalty: Score::from_int(-2),

            // Tension curve: reward proper arc shape
            tension_curve_reward: Score::from_int(15),

            // Interval distribution
            interval_distribution_penalty: Score::from_ratio(-1, 2), // -0.5

            // Melodic entropy (information content)
            entropy_reward: Score::from_int(6),
            entropy_penalty: Score::from_int(-4),
        }
    }
}

/// Full score for a grid.
pub fn score_grid(grid: &Grid, weights: &ScoringWeights, mode: &ModeInstance) -> Score {
    let mut total = Score::ZERO;

    total += score_hard_rules(grid, weights);
    total += score_melodic(grid, weights);
    total += score_harmonic(grid, weights);
    total += score_global(grid, weights);
    total += score_modal(grid, weights, mode);
    total += score_texture(grid, weights);
    total += score_tension_curve(grid, weights);
    total += score_interval_distribution(grid, weights);
    total += score_entropy(grid, weights);

    total
}

/// Per-layer scoring breakdown for diagnostic output.
pub struct ScoreBreakdown {
    pub hard_rules: Score,
    pub melodic: Score,
    pub harmonic: Score,
    pub global: Score,
    pub modal: Score,
    pub texture: Score,
    pub tension_curve: Score,
    pub interval_dist: Score,
    pub entropy: Score,
    pub tonal_contour: Score,
    pub total: Score,
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
    let entropy = score_entropy(grid, weights);
    let tonal_contour = score_tonal_contour(grid, mapping, weights);

    let total = hard_rules
        + melodic
        + harmonic
        + global
        + modal
        + texture
        + tension_curve
        + interval_dist
        + entropy
        + tonal_contour;

    ScoreBreakdown {
        hard_rules,
        melodic,
        harmonic,
        global,
        modal,
        texture,
        tension_curve,
        interval_dist,
        entropy,
        tonal_contour,
        total,
    }
}

/// Score contribution from a local window around a specific beat.
/// Used for incremental scoring after a mutation.
pub fn score_local(
    grid: &Grid,
    weights: &ScoringWeights,
    mode: &ModeInstance,
    beat: usize,
) -> Score {
    let window_start = beat.saturating_sub(2);
    let window_end = (beat + 3).min(grid.num_beats);

    let mut total = Score::ZERO;

    for b in window_start..window_end {
        total += score_beat_hard_rules(grid, weights, b);
        total += score_beat_harmonic(grid, weights, b);
        total += score_beat_modal(grid, weights, mode, b);
    }

    // Melodic scoring for voices that have notes in the window
    for &voice in grid.active_voices() {
        total += score_melodic_window(grid, weights, voice, window_start, window_end);
    }

    total
}

// ── Layer 1: Hard counterpoint rules ──

fn score_hard_rules(grid: &Grid, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;
    for beat in 0..grid.num_beats {
        score += score_beat_hard_rules(grid, weights, beat);
    }
    score
}

fn score_beat_hard_rules(grid: &Grid, weights: &ScoringWeights, beat: usize) -> Score {
    let mut score = Score::ZERO;
    let is_strong = beat.is_multiple_of(4); // strong beats every half-bar (2 quarter notes)

    // Check all active voice pairs
    let active = grid.active_voices();
    for (idx_i, &vi) in active.iter().enumerate() {
        let pitch_i = grid.sounding_pitch(vi, beat);

        for &vj in &active[(idx_i + 1)..] {
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
                            let same_direction =
                                (motion_i > 0 && motion_j > 0) || (motion_i < 0 && motion_j < 0);

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
        if (1..=2).contains(&resolution) {
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

fn score_melodic(grid: &Grid, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;
    for &voice in grid.active_voices() {
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
) -> Score {
    let mut score = Score::ZERO;
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
                // 3rds: mild — leap_penalty * 3 / 10
                score += weights.leap_penalty.mul_int(3).div_int(10);
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
            let direction = if iv > 0 {
                1i8
            } else if iv < 0 {
                -1
            } else {
                0
            };

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
                if prev_interval_abs > 4 {
                    let recovered = direction != 0 && direction != prev_dir && abs_iv <= 2;
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

fn score_harmonic(grid: &Grid, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;
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
fn score_interval_variety(grid: &Grid, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;

    let active = grid.active_voices();
    for (idx_i, &vi) in active.iter().enumerate() {
        for &vj in &active[(idx_i + 1)..] {
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

fn score_beat_harmonic(grid: &Grid, weights: &ScoringWeights, beat: usize) -> Score {
    let mut score = Score::ZERO;
    let slice = grid.vertical_slice(beat);

    let pitches: Vec<u8> = slice.iter().filter_map(|&(_, p)| p).collect();
    if pitches.len() < 2 {
        return Score::ZERO;
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

fn score_global(grid: &Grid, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;

    // Opening: reward perfect consonance on first sounding beat
    for beat in 0..grid.num_beats.min(8) {
        let slice = grid.vertical_slice(beat);
        let pitches: Vec<u8> = slice.iter().filter_map(|&(_, p)| p).collect();
        if pitches.len() >= 2 {
            let all_consonant = pitches
                .windows(2)
                .all(|w| interval::is_consonant(interval::semitones(w[0], w[1])));
            if all_consonant {
                score += weights.opening_consonance_reward;
            }
            break;
        }
    }

    // Closing: reward perfect consonance on last sounding beat
    for beat in (0..grid.num_beats).rev() {
        let slice = grid.vertical_slice(beat);
        let pitches: Vec<u8> = slice.iter().filter_map(|&(_, p)| p).collect();
        if pitches.len() >= 2 {
            let all_perfect = pitches
                .windows(2)
                .all(|w| interval::is_perfect_consonance(interval::semitones(w[0], w[1])));
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
fn score_cadences(grid: &Grid, _weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;

    // Find beats where a rest follows sounding notes (phrase endings)
    for beat in 2..grid.num_beats.saturating_sub(1) {
        // Check if this beat is near a phrase boundary:
        // at least 2 voices have a rest within 1-2 beats after this point
        let mut voices_resting_soon = 0;
        for &voice in grid.active_voices() {
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
        // Use the highest and lowest active voices for cadential scoring.
        let active = grid.active_voices();
        let top_voice = active[0]; // highest voice (earliest in SATB order)
        let bottom_voice = active[active.len() - 1]; // lowest voice
        if top_voice == bottom_voice {
            continue; // Solo — no cadential voice pair to evaluate
        }

        let sop_now = grid.sounding_pitch(top_voice, beat);
        let sop_prev = if beat > 0 {
            grid.sounding_pitch(top_voice, beat - 1)
        } else {
            None
        };
        let bass_now = grid.sounding_pitch(bottom_voice, beat);
        let bass_prev = if beat > 0 {
            grid.sounding_pitch(bottom_voice, beat - 1)
        } else {
            None
        };

        if let (Some(sn), Some(sp), Some(bn), Some(bp)) = (sop_now, sop_prev, bass_now, bass_prev) {
            let sop_motion = sn as i16 - sp as i16;
            let bass_motion = bn as i16 - bp as i16;

            // Contrary motion between soprano and bass
            if (sop_motion > 0 && bass_motion < 0) || (sop_motion < 0 && bass_motion > 0) {
                score += Score::from_int(3);
            }

            // Soprano moving by step (1-2 semitones)
            if sop_motion.unsigned_abs() <= 2 && sop_motion != 0 {
                score += Score::from_int(2);
            }

            // Bass moving by 4th or 5th (5 or 7 semitones)
            let bass_abs = bass_motion.unsigned_abs();
            if bass_abs == 5 || bass_abs == 7 {
                score += Score::from_int(4);
            }

            // Final beat of cadence lands on perfect consonance
            let iv = interval::semitones(bn, sn);
            if interval::is_perfect_consonance(iv) {
                score += Score::from_int(3);
            }
        }
    }

    score
}

/// Score melodic contour for each voice.
/// Rewards arch-shaped contours (rise to climax, then descent) and penalizes
/// the highest note occurring too many times (climax should be special).
fn score_contour(grid: &Grid, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;

    for &voice in grid.active_voices() {
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
            score += weights
                .climax_repeat_penalty
                .mul_int(climax_count as i64 - 3);
        }

        // Arch contour: the climax should be roughly in the middle 30-70% of the piece.
        let first_climax_beat = notes
            .iter()
            .find(|&&(_, p)| p == max_pitch)
            .map(|&(b, _)| b)
            .unwrap();
        let total_span = notes.last().unwrap().0 - notes.first().unwrap().0;
        if total_span > 0 {
            // Use integer arithmetic: climax_pos_pct = (offset * 100) / total_span
            let offset = first_climax_beat - notes.first().unwrap().0;
            let climax_pct = offset * 100 / total_span;
            // Ideal: climax between 30% and 70% through the phrase
            if climax_pct > 30 && climax_pct < 70 {
                score += weights.arch_contour_reward;
            }
        }
    }

    score
}

/// Score rhythmic independence between voices.
/// Penalizes when 3+ voices attack on the same beat for many consecutive beats.
fn score_rhythmic_independence(grid: &Grid, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;
    let mut consecutive_homorhythm = 0;

    for beat in 0..grid.num_beats {
        let attacks: usize = grid
            .active_voices()
            .iter()
            .filter(|&&v| {
                let c = grid.cell(v, beat);
                c.attack && !c.is_rest
            })
            .count();

        // NOTE: This threshold is hardcoded at 3, which means duets (max 2
        // voices) never trigger homorhythm penalties.
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

fn score_modal(grid: &Grid, weights: &ScoringWeights, mode: &ModeInstance) -> Score {
    let mut score = Score::ZERO;
    for beat in 0..grid.num_beats {
        score += score_beat_modal(grid, weights, mode, beat);
    }
    score
}

fn score_beat_modal(
    grid: &Grid,
    weights: &ScoringWeights,
    mode: &ModeInstance,
    beat: usize,
) -> Score {
    let mut score = Score::ZERO;

    for &voice in grid.active_voices() {
        if let Some(pitch) = grid.sounding_pitch(voice, beat) {
            if mode.is_in_mode(pitch) {
                // Reward structurally important degrees more
                score += mode
                    .pitch_fitness(pitch)
                    .mul_fixed(weights.mode_degree_reward);
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
fn score_texture(grid: &Grid, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;
    let mut density_counts = [0u32; 5]; // index = number of active voices (0-4)
    let mut consecutive_thin = 0u32;

    for beat in 0..grid.num_beats {
        let active = grid
            .active_voices()
            .iter()
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
        score += weights.texture_variety_reward.div_int(2);
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
///
/// Uses integer arithmetic throughout. Tension values are scaled by 1000
/// for precision without floating-point.
fn score_tension_curve(grid: &Grid, weights: &ScoringWeights) -> Score {
    if grid.num_beats < 16 {
        return Score::ZERO;
    }

    // Divide the piece into 8 segments
    let seg_len = grid.num_beats / 8;
    if seg_len == 0 {
        return Score::ZERO;
    }

    // Tension per segment, scaled by 1000 for integer precision.
    let mut segment_tension: Vec<i64> = Vec::new();

    for seg in 0..8 {
        let start = seg * seg_len;
        let end = if seg == 7 {
            grid.num_beats
        } else {
            (seg + 1) * seg_len
        };

        let mut total_pitch = 0u32;
        let mut pitch_count = 0u32;
        let mut dissonance_count = 0u32;
        let mut pair_count = 0u32;

        for beat in start..end {
            let slice = grid.vertical_slice(beat);
            let pitches: Vec<u8> = slice.iter().filter_map(|&(_, p)| p).collect();

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
            segment_tension.push(0);
            continue;
        }

        // Tension = normalized pitch height + dissonance ratio (both scaled by 1000)
        // pitch_tension = (avg_pitch - 48) * 1000 / 36
        let avg_pitch_1000 = total_pitch as i64 * 1000 / pitch_count as i64;
        let pitch_tension = (avg_pitch_1000 - 48_000) * 1000 / 36_000;
        let diss_ratio_1000 = if pair_count > 0 {
            dissonance_count as i64 * 1000 / pair_count as i64
        } else {
            0
        };

        segment_tension.push(pitch_tension + diss_ratio_1000 * 2);
    }

    // Find the peak segment
    let peak_seg = segment_tension
        .iter()
        .enumerate()
        .max_by_key(|(_, v)| *v)
        .map(|(i, _)| i)
        .unwrap_or(0);

    // Ideal peak: segments 4-5 (55-75% through the piece)
    // peak_pos_pct = peak_seg * 100 / 7
    let peak_pct = peak_seg * 100 / 7;
    let mut score = Score::ZERO;

    if (40..=80).contains(&peak_pct) {
        score += weights.tension_curve_reward;
    } else if (30..=90).contains(&peak_pct) {
        score += weights.tension_curve_reward.div_int(2);
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

    // arc_quality as percentage (0-100): (rising/total + falling/total) * 50
    let arc_quality_pct = rising_before * 50 / total_before + falling_after * 50 / total_after;

    // Reward good arc shape (50 = random, 100 = perfect arc)
    // Original: if arc_quality > 0.6 → score += reward * 0.5 * (arc_quality - 0.5)
    // Translated: if arc_quality_pct > 60 → score += reward * (arc_quality_pct - 50) / 200
    if arc_quality_pct > 60 {
        score += weights
            .tension_curve_reward
            .mul_int(arc_quality_pct as i64 - 50)
            .div_int(200);
    }

    score
}

// ── Interval distribution scoring ──

/// Score how well melodic intervals match Palestrina's empirical distribution.
///
/// Uses integer percentage arithmetic (0-100 scale) for determinism.
fn score_interval_distribution(grid: &Grid, weights: &ScoringWeights) -> Score {
    // Target distribution percentages (out of 100)
    const TARGET_STEP_PCT: i64 = 55; // unison + 2nds (0-2 semitones)
    const TARGET_THIRD_PCT: i64 = 22; // 3rds (3-4 semitones)
    const TARGET_FOURTH_FIFTH_PCT: i64 = 15; // 4ths-5ths (5-7 semitones)

    let mut step_count = 0i64;
    let mut third_count = 0i64;
    let mut fourth_fifth_count = 0i64;
    let mut total = 0i64;

    for &voice in grid.active_voices() {
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
        return Score::ZERO; // Not enough data for meaningful comparison
    }

    // Compute deviations in percentage points (0-100 scale)
    let step_pct = step_count * 100 / total;
    let third_pct = third_count * 100 / total;
    let fourth_pct = fourth_fifth_count * 100 / total;

    let step_dev = (step_pct - TARGET_STEP_PCT).abs();
    let third_dev = (third_pct - TARGET_THIRD_PCT).abs();
    let fourth_dev = (fourth_pct - TARGET_FOURTH_FIFTH_PCT).abs();

    // Weight step motion deviation more heavily (it's most important)
    // Total deviation in percentage points
    let total_deviation = step_dev * 2 + third_dev + fourth_dev;

    // Only penalize if deviation is substantial (> 15 percentage points)
    if total_deviation > 15 {
        // penalty * (deviation - 15) * total / 100
        weights
            .interval_distribution_penalty
            .mul_int((total_deviation - 15) * total)
            .div_int(100)
    } else {
        Score::ZERO
    }
}

// ── Melodic entropy scoring ──

/// Score melodic entropy (information content) per voice.
///
/// Uses integer approximation of Shannon entropy via a lookup table for
/// -p*log2(p) scaled by 1000. The target entropy range is 2.0-3.5 bits,
/// mapped to 2000-3500 in our millibits scale.
fn score_entropy(grid: &Grid, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;

    for &voice in grid.active_voices() {
        // Count interval occurrences
        let mut interval_counts: std::collections::BTreeMap<i8, u32> =
            std::collections::BTreeMap::new();
        let mut prev_pitch: Option<u8> = None;
        let mut total = 0u32;

        for beat in 0..grid.num_beats {
            let cell = grid.cell(voice, beat);
            if cell.attack && !cell.is_rest {
                if let Some(prev) = prev_pitch {
                    let iv = (cell.pitch as i8).wrapping_sub(prev as i8);
                    *interval_counts.entry(iv).or_insert(0) += 1;
                    total += 1;
                }
                prev_pitch = Some(cell.pitch);
            }
        }

        if total < 10 {
            continue; // Not enough data
        }

        // Compute Shannon entropy in millibits: H = -Σ p(x) * log2(p(x)) * 1000
        // = Σ count(x) * log2(total/count(x)) * 1000 / total
        // = (log2(total) * 1000 * total - Σ count(x) * log2(count(x)) * 1000) / total
        //
        // Using integer log2 approximation: ilog2_millis(n) ≈ log2(n) * 1000
        let total_log = ilog2_millis(total);
        let mut sum_c_log_c: i64 = 0;
        for &c in interval_counts.values() {
            sum_c_log_c += c as i64 * ilog2_millis(c);
        }
        let entropy_millibits = total_log * total as i64 - sum_c_log_c;
        // Normalize: divide by total to get per-symbol entropy in millibits
        let entropy_mb = entropy_millibits / total as i64;

        // Target range: 2000-3500 millibits (= 2.0-3.5 bits)
        if (2000..=3500).contains(&entropy_mb) {
            score += weights.entropy_reward;
        } else if !(1500..=4500).contains(&entropy_mb) {
            // Way outside the sweet spot
            score += weights.entropy_penalty;
        }
        // Between 1500-2000 or 3500-4500: no bonus, no penalty
    }

    score
}

/// Integer approximation of log2(n) * 1000 (millibits).
/// Uses the integer part from bit position plus a small linear interpolation
/// for the fractional part. Accurate to within ~3% for n >= 1.
fn ilog2_millis(n: u32) -> i64 {
    if n <= 1 {
        return 0;
    }
    // Integer part: floor(log2(n))
    let int_part = 31 - n.leading_zeros(); // = floor(log2(n))
    // Fractional approximation: (n - 2^int_part) * 1000 / 2^int_part
    let power = 1u32 << int_part;
    let frac_millis = (n - power) as i64 * 1000 / power as i64;
    int_part as i64 * 1000 + frac_millis
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
pub fn score_tonal_contour(grid: &Grid, mapping: &TextMapping, weights: &ScoringWeights) -> Score {
    let mut score = Score::ZERO;
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
) -> Score {
    let mut score = Score::ZERO;
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
pub fn tonal_contour_stats(grid: &Grid, mapping: &TextMapping) -> TonalContourStats {
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
        if contour_score > Score::ZERO {
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
) -> Score {
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
        return Score::ZERO;
    }

    // Check if the span has enough notes for the tone's minimum
    if pitches.len() < span.tone.min_notes() {
        // Not enough notes to realize the contour — mild penalty
        return weights.tonal_contour_violation.div_int(2);
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
                let min_middle = pitches[1..pitches.len() - 1]
                    .iter()
                    .min()
                    .copied()
                    .unwrap_or(first);
                if min_middle < first && min_middle < last {
                    weights.tonal_contour_reward
                } else {
                    weights.tonal_contour_violation
                }
            } else {
                // Only 2 notes but need 3 for dipping
                weights.tonal_contour_violation.div_int(2)
            }
        }
        Tone::Peaking => {
            // Hill: some pitch in the middle is higher than both first and last
            if pitches.len() >= 3 {
                let max_middle = pitches[1..pitches.len() - 1]
                    .iter()
                    .max()
                    .copied()
                    .unwrap_or(first);
                if max_middle > first && max_middle > last {
                    weights.tonal_contour_reward
                } else {
                    weights.tonal_contour_violation
                }
            } else {
                weights.tonal_contour_violation.div_int(2)
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
        assert!(
            score < Score::ZERO,
            "Parallel fifths should produce negative score, got {}",
            score
        );
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

        // Test the is_prepared_suspension function directly.
        let is_sus = is_prepared_suspension(&grid, Voice::Soprano, Voice::Alto, 1);
        assert!(
            is_sus,
            "Should detect prepared suspension (Alto held from consonance, resolves down)"
        );
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
        assert!(
            score < Score::ZERO,
            "Unprepared dissonance on strong beat should penalize, got {}",
            score
        );
    }

    #[test]
    fn test_consonance_rewarded() {
        let mut grid = Grid::new(4);
        let weights = ScoringWeights::default();

        // Soprano: C4 (60), Alto: E3 (52) — major 6th, consonant
        grid.set_note(Voice::Soprano, 0, 60);
        grid.set_note(Voice::Alto, 0, 52);

        let beat_score = score_beat_harmonic(&grid, &weights, 0);
        assert!(
            beat_score > Score::ZERO,
            "Consonant intervals should be rewarded"
        );
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

        assert!(
            with_recovery > no_recovery,
            "Leap with recovery ({}) should score better than without ({})",
            with_recovery,
            no_recovery
        );
    }

    #[test]
    fn test_tonal_contour_level_rewarded() {
        use crate::text_mapping::{SyllableSpan, TextMapping};

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
        assert!(
            score > Score::ZERO,
            "Level tone with held pitch should be rewarded, got {}",
            score
        );
    }

    #[test]
    fn test_tonal_contour_rising_penalized_when_falling() {
        use crate::text_mapping::{SyllableSpan, TextMapping};

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
        assert!(
            score < Score::ZERO,
            "Rising tone with falling pitch should be penalized, got {}",
            score
        );
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
        assert!(
            score > Score::ZERO,
            "Stepwise motion should be rewarded, got {}",
            score
        );
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
        assert!(
            score > Score::ZERO,
            "Varied texture (2/3/4 voices) should be rewarded, got {}",
            score
        );
    }

    #[test]
    fn test_interval_distribution_stepwise_good() {
        let mut grid = Grid::new(40);
        let weights = ScoringWeights::default();

        // Mostly stepwise soprano line (Palestrina-like)
        let pitches = [
            60, 62, 64, 62, 60, 62, 64, 65, 64, 62, 60, 62, 64, 62, 60, 62, 64, 65, 67, 65,
        ];
        for (i, &p) in pitches.iter().enumerate() {
            grid.set_note(Voice::Soprano, i * 2, p);
        }

        let score = score_interval_distribution(&grid, &weights);
        // Mostly steps = close to target, should have low or no penalty
        assert!(
            score >= Score::from_int(-5),
            "Stepwise-dominated line should have mild distribution penalty, got {}",
            score
        );
    }

    #[test]
    fn test_ilog2_millis_basic() {
        assert_eq!(ilog2_millis(1), 0);
        assert_eq!(ilog2_millis(2), 1000);
        assert_eq!(ilog2_millis(4), 2000);
        assert_eq!(ilog2_millis(8), 3000);
        // log2(3) ≈ 1.585, our approx: 1000 + 500 = 1500
        let l3 = ilog2_millis(3);
        assert!(l3 >= 1400 && l3 <= 1600, "log2(3) approx: {l3}");
    }

    #[test]
    fn test_ilog2_millis_accuracy() {
        // Verify accuracy within 5% for representative values.
        // Reference: log2(n) * 1000 (computed here as f64 for comparison only).
        let cases: &[(u32, i64)] = &[
            (10, 3321),   // log2(10) = 3.321928
            (100, 6643),  // log2(100) = 6.643856
            (1000, 9965), // log2(1000) = 9.96578
        ];
        for &(n, expected_millis) in cases {
            let actual = ilog2_millis(n);
            let error_pct = ((actual - expected_millis).abs() * 100) / expected_millis.max(1);
            assert!(
                error_pct <= 5,
                "ilog2_millis({n}) = {actual}, expected ~{expected_millis} (error {error_pct}%)"
            );
        }
    }

    #[test]
    fn test_ilog2_millis_zero() {
        assert_eq!(ilog2_millis(0), 0);
    }
}
