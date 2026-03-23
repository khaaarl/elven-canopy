// Simulated annealing refinement of the score grid.
//
// Takes a draft-filled grid and iteratively improves it by proposing
// mutations, scoring the result, and accepting/rejecting based on the
// Metropolis criterion. Uses a cooling schedule with periodic reheating
// and optional adaptive cooling. All arithmetic uses `Score` (Fixed64)
// for cross-platform determinism — no floating-point operations.
//
// Three types of mutations:
// - Pitch mutation (~75%): change a note's pitch using Markov-guided proposal,
//   snapped to the active mode. Includes tonal contour scoring when text
//   mapping is active.
// - Duration mutation (~20%): extend a note into an adjacent rest, or shorten
//   by converting the last beat to a rest. Respects structural cells.
// - Text-swap macro mutation (~5%, text mode only): replace a section's phrase
//   with an alternative candidate, changing the tonal constraints.
//
// Adaptive cooling (AdaptiveCooler): tracks acceptance rate over sliding
// windows and adjusts cooling rate to maintain a productive 15-40% acceptance
// zone. Slows cooling when acceptance drops too low, speeds up when too high.
//
// Structural cells (motif entries from structure.rs and final cadence from
// draft.rs) are excluded from mutation — they're the compositional anchors.
//
// Depends on scoring.rs for quality evaluation, markov.rs for mutation
// proposals, and text_mapping.rs + vaelith.rs for text-aware optimization.

use crate::grid::{Grid, Voice};
use crate::markov::MarkovModels;
use crate::mode::{ModeInstance, Score};
use crate::scoring::{
    ScoringWeights, score_grid, score_local, score_tonal_contour, score_tonal_contour_local,
};
use crate::structure::StructurePlan;
use crate::text_mapping::{TextMapping, swap_section_phrase};
use crate::vaelith::VaelithPhrase;
use elven_canopy_prng::GameRng;

/// SA configuration parameters.
/// Temperature and cooling use `Score` (Fixed64) for determinism.
#[derive(Debug, Clone)]
pub struct SAConfig {
    /// Initial temperature.
    pub initial_temp: Score,
    /// Final temperature (stop condition).
    pub final_temp: Score,
    /// Cooling rate per step (multiplicative), as a fraction < 1.
    /// Stored as Score where 1.0 = Score::ONE.
    pub cooling_rate: Score,
    /// Number of mutations to try per temperature step.
    pub mutations_per_step: usize,
    /// Temperature at which to reheat.
    pub reheat_temp: Score,
    /// Temperature to reheat to.
    pub reheat_target: Score,
    /// Number of reheats allowed.
    pub max_reheats: usize,
    /// Enable adaptive cooling (adjust cooling rate based on acceptance ratio).
    pub adaptive: bool,
}

impl Default for SAConfig {
    fn default() -> Self {
        SAConfig {
            initial_temp: Score::from_int(10),
            final_temp: Score::from_ratio(1, 100), // 0.01
            cooling_rate: Score::from_ratio(9995, 10000), // 0.9995
            mutations_per_step: 1,
            reheat_temp: Score::from_ratio(1, 10), // 0.1
            reheat_target: Score::from_int(3),
            max_reheats: 3,
            adaptive: true,
        }
    }
}

/// Adaptive cooling state: tracks acceptance rate over a sliding window
/// and adjusts the cooling rate accordingly.
struct AdaptiveCooler {
    base_rate: Score,
    window_accepted: u32,
    window_total: u32,
    window_size: u32,
}

impl AdaptiveCooler {
    fn new(base_rate: Score) -> Self {
        AdaptiveCooler {
            base_rate,
            window_accepted: 0,
            window_total: 0,
            window_size: 200,
        }
    }

    fn record(&mut self, accepted: bool) {
        self.window_total += 1;
        if accepted {
            self.window_accepted += 1;
        }
    }

    /// Return the current effective cooling rate, adapting based on acceptance.
    /// Target acceptance rate: 15-40%. If too low, slow cooling (rate closer
    /// to 1.0). If too high, speed up cooling (rate closer to base).
    fn effective_rate(&mut self) -> Score {
        if self.window_total < self.window_size {
            return self.base_rate;
        }

        // accept_ratio as percentage (0-100)
        let accept_pct = self.window_accepted * 100 / self.window_total;

        // Reset window for next measurement
        self.window_accepted = 0;
        self.window_total = 0;

        // Compute adjustment toward Score::ONE (slower cooling)
        let gap = Score::ONE - self.base_rate;

        if accept_pct < 5 {
            // Almost nothing accepted — slow cooling dramatically
            // base_rate + gap * 0.5
            self.base_rate + gap.div_int(2)
        } else if accept_pct < 15 {
            // Below target — slow cooling a bit
            // base_rate + gap * 0.2
            self.base_rate + gap.mul_int(2).div_int(10)
        } else if accept_pct > 50 {
            // Too many accepted — speed up cooling
            // base_rate * 0.95
            self.base_rate.mul_int(95).div_int(100)
        } else {
            // In the sweet spot — use base rate
            self.base_rate
        }
    }
}

/// Result of an SA run.
#[derive(Debug)]
pub struct SAResult {
    pub final_score: Score,
    pub iterations: usize,
    pub accepted: usize,
    pub reheats: usize,
}

/// Run simulated annealing on a grid.
///
/// Modifies the grid in-place. Returns statistics about the run.
pub fn anneal(
    grid: &mut Grid,
    models: &MarkovModels,
    structural_cells: &[(usize, usize)],
    weights: &ScoringWeights,
    mode: &ModeInstance,
    config: &SAConfig,
    rng: &mut GameRng,
) -> SAResult {
    let structural_set: std::collections::HashSet<(usize, usize)> =
        structural_cells.iter().copied().collect();

    // Collect mutable cells (non-structural, non-rest attack cells)
    let num_beats = grid.num_beats;
    let mut mutable_cells: Vec<(Voice, usize)> = Vec::new();
    for &voice in grid.active_voices() {
        for beat in 0..num_beats {
            if structural_set.contains(&(voice.index(), beat)) {
                continue;
            }
            let cell = grid.cell(voice, beat);
            if !cell.is_rest && cell.attack {
                mutable_cells.push((voice, beat));
            }
        }
    }

    if mutable_cells.is_empty() {
        return SAResult {
            final_score: score_grid(grid, weights, mode),
            iterations: 0,
            accepted: 0,
            reheats: 0,
        };
    }

    let mut temp = config.initial_temp;
    let mut _current_score = score_grid(grid, weights, mode);
    let mut iterations = 0;
    let mut accepted = 0;
    let mut reheats = 0;

    while temp > config.final_temp {
        for _ in 0..config.mutations_per_step {
            // 20% duration mutations, 80% pitch mutations
            let roll = rng.range_usize(0, 5);
            let do_duration_mutation = roll == 0; // 1 in 5 = 20%

            if do_duration_mutation {
                // Duration mutation: extend or shorten a note
                let idx = rng.range_usize(0, mutable_cells.len());
                let (voice, beat) = mutable_cells[idx];
                let delta = try_duration_mutation(
                    grid,
                    weights,
                    mode,
                    voice,
                    beat,
                    &structural_set,
                    temp,
                    rng,
                );
                if let Some(d) = delta {
                    _current_score += d;
                    accepted += 1;
                }
            } else {
                // Pitch mutation
                let idx = rng.range_usize(0, mutable_cells.len());
                let (voice, beat) = mutable_cells[idx];
                let delta = try_pitch_mutation(grid, models, weights, mode, voice, beat, temp, rng);
                if let Some(d) = delta {
                    _current_score += d;
                    accepted += 1;
                }
            }

            iterations += 1;
        }

        // Cool: temp = temp * cooling_rate
        temp = temp.mul_fixed(config.cooling_rate);

        // Reheat if needed
        if temp < config.reheat_temp && reheats < config.max_reheats {
            temp = config.reheat_target;
            reheats += 1;
        }
    }

    SAResult {
        final_score: score_grid(grid, weights, mode),
        iterations,
        accepted,
        reheats,
    }
}

/// Run text-aware simulated annealing on a grid.
///
/// Extends the base SA with tonal contour scoring and text-swap macro mutations.
/// The text mapping is modified in-place when phrase swaps are accepted.
#[allow(clippy::too_many_arguments)]
pub fn anneal_with_text(
    grid: &mut Grid,
    models: &MarkovModels,
    structural_cells: &[(usize, usize)],
    weights: &ScoringWeights,
    mode: &ModeInstance,
    config: &SAConfig,
    plan: &StructurePlan,
    mapping: &mut TextMapping,
    phrase_candidates: &[Vec<VaelithPhrase>],
    rng: &mut GameRng,
) -> SAResult {
    let structural_set: std::collections::HashSet<(usize, usize)> =
        structural_cells.iter().copied().collect();

    let num_beats = grid.num_beats;
    let mut mutable_cells: Vec<(Voice, usize)> = Vec::new();
    for &voice in grid.active_voices() {
        for beat in 0..num_beats {
            if structural_set.contains(&(voice.index(), beat)) {
                continue;
            }
            let cell = grid.cell(voice, beat);
            if !cell.is_rest && cell.attack {
                mutable_cells.push((voice, beat));
            }
        }
    }

    if mutable_cells.is_empty() {
        let base = score_grid(grid, weights, mode);
        let text = score_tonal_contour(grid, mapping, weights);
        return SAResult {
            final_score: base + text,
            iterations: 0,
            accepted: 0,
            reheats: 0,
        };
    }

    let mut temp = config.initial_temp;
    let base_score = score_grid(grid, weights, mode);
    let text_score = score_tonal_contour(grid, mapping, weights);
    let mut _current_score = base_score + text_score;
    let mut iterations = 0;
    let mut accepted = 0;
    let mut reheats = 0;
    let mut cooler = AdaptiveCooler::new(config.cooling_rate);

    let num_sections = plan.imitation_points.len();

    while temp > config.final_temp {
        for _ in 0..config.mutations_per_step {
            // Use integer roll: 0-19 range
            let roll = rng.range_usize(0, 20);
            let mut step_accepted = false;

            if roll == 0 && num_sections > 0 && !phrase_candidates.is_empty() {
                // Text-swap macro mutation (~5%)
                let delta = try_text_swap_mutation(
                    grid,
                    weights,
                    mode,
                    plan,
                    mapping,
                    phrase_candidates,
                    temp,
                    rng,
                );
                if let Some(d) = delta {
                    _current_score += d;
                    accepted += 1;
                    step_accepted = true;
                }
            } else if roll < 4 {
                // Duration mutation (~15-20%)
                let idx = rng.range_usize(0, mutable_cells.len());
                let (voice, beat) = mutable_cells[idx];
                let delta = try_duration_mutation(
                    grid,
                    weights,
                    mode,
                    voice,
                    beat,
                    &structural_set,
                    temp,
                    rng,
                );
                if let Some(d) = delta {
                    _current_score += d;
                    accepted += 1;
                    step_accepted = true;
                }
            } else {
                // Pitch mutation with tonal contour awareness (~75-80%)
                let idx = rng.range_usize(0, mutable_cells.len());
                let (voice, beat) = mutable_cells[idx];
                let delta = try_pitch_mutation_with_text(
                    grid, models, weights, mode, mapping, voice, beat, temp, rng,
                );
                if let Some(d) = delta {
                    _current_score += d;
                    accepted += 1;
                    step_accepted = true;
                }
            }

            if config.adaptive {
                cooler.record(step_accepted);
            }
            iterations += 1;
        }

        let rate = if config.adaptive {
            cooler.effective_rate()
        } else {
            config.cooling_rate
        };
        temp = temp.mul_fixed(rate);

        if temp < config.reheat_temp && reheats < config.max_reheats {
            temp = config.reheat_target;
            reheats += 1;
        }
    }

    let final_base = score_grid(grid, weights, mode);
    let final_text = score_tonal_contour(grid, mapping, weights);
    SAResult {
        final_score: final_base + final_text,
        iterations,
        accepted,
        reheats,
    }
}

/// Try a pitch mutation that also considers tonal contour constraints.
#[allow(clippy::too_many_arguments)]
fn try_pitch_mutation_with_text(
    grid: &mut Grid,
    models: &MarkovModels,
    weights: &ScoringWeights,
    mode: &ModeInstance,
    mapping: &TextMapping,
    voice: Voice,
    beat: usize,
    temp: Score,
    rng: &mut GameRng,
) -> Option<Score> {
    let old_pitch = grid.cell(voice, beat).pitch;
    let (range_low, range_high) = voice.range();

    let old_local = score_local(grid, weights, mode, beat)
        + score_tonal_contour_local(grid, mapping, weights, beat);

    // Build Markov context from preceding notes
    let mut context = Vec::new();
    let mut prev_pitch = None;
    for b in (0..beat).rev() {
        let cell = grid.cell(voice, b);
        if cell.attack && !cell.is_rest {
            if let Some(pp) = prev_pitch {
                let iv = cell.pitch as i8 - pp as i8;
                context.insert(0, iv);
                if context.len() >= 3 {
                    break;
                }
            }
            prev_pitch = Some(cell.pitch);
        }
    }

    let pitch_before = {
        let mut p = None;
        for b in (0..beat).rev() {
            let cell = grid.cell(voice, b);
            if !cell.is_rest {
                p = Some(cell.pitch);
                break;
            }
        }
        p.unwrap_or(old_pitch)
    };

    let rng_val: u64 = rng.next_u64();
    let proposed_interval = models.melodic.sample(&context, rng_val);
    let raw_pitch = (pitch_before as i16 + proposed_interval as i16)
        .clamp(range_low as i16, range_high as i16) as u8;
    let new_pitch = mode.snap_to_mode(raw_pitch);

    if new_pitch == old_pitch {
        return None;
    }

    // Apply
    grid.cell_mut(voice, beat).pitch = new_pitch;
    for b in (beat + 1)..grid.num_beats {
        let cell = grid.cell(voice, b);
        if cell.is_rest || cell.attack {
            break;
        }
        grid.cell_mut(voice, b).pitch = new_pitch;
    }

    let new_local = score_local(grid, weights, mode, beat)
        + score_tonal_contour_local(grid, mapping, weights, beat);
    let delta = new_local - old_local;

    if metropolis_accept(delta, temp, rng) {
        Some(delta)
    } else {
        // Revert
        grid.cell_mut(voice, beat).pitch = old_pitch;
        for b in (beat + 1)..grid.num_beats {
            let cell = grid.cell(voice, b);
            if cell.is_rest || cell.attack {
                break;
            }
            grid.cell_mut(voice, b).pitch = old_pitch;
        }
        None
    }
}

/// Try swapping a section's text phrase with a different candidate.
/// Scores before and after the swap, accepting via Metropolis criterion.
#[allow(clippy::too_many_arguments)]
fn try_text_swap_mutation(
    grid: &mut Grid,
    weights: &ScoringWeights,
    _mode: &ModeInstance,
    plan: &StructurePlan,
    mapping: &mut TextMapping,
    phrase_candidates: &[Vec<VaelithPhrase>],
    temp: Score,
    rng: &mut GameRng,
) -> Option<Score> {
    let num_sections = plan.imitation_points.len().min(phrase_candidates.len());
    if num_sections == 0 {
        return None;
    }

    let section_idx = rng.range_usize(0, num_sections);
    let candidates = &phrase_candidates[section_idx];
    if candidates.len() <= 1 {
        return None;
    }

    // Pick a different phrase
    let current_text = if section_idx < mapping.section_phrases.len() {
        &mapping.section_phrases[section_idx].text
    } else {
        return None;
    };

    let alternatives: Vec<&VaelithPhrase> = candidates
        .iter()
        .filter(|p| p.text != *current_text)
        .collect();

    if alternatives.is_empty() {
        return None;
    }

    let new_phrase = alternatives[rng.range_usize(0, alternatives.len())].clone();

    // Score before
    let old_score = score_tonal_contour(grid, mapping, weights);

    // Save old phrase for potential revert
    let old_phrase = mapping.section_phrases[section_idx].clone();
    let old_spans: Vec<_> = mapping.spans.clone();

    // Apply swap
    swap_section_phrase(grid, mapping, plan, section_idx, &new_phrase);

    // Score after
    let new_score = score_tonal_contour(grid, mapping, weights);
    let delta = new_score - old_score;

    if metropolis_accept(delta, temp, rng) {
        Some(delta)
    } else {
        // Revert: restore old mapping
        swap_section_phrase(grid, mapping, plan, section_idx, &old_phrase);
        // Restore the exact old spans (the IDs may have changed)
        mapping.spans = old_spans;
        None
    }
}

/// Try a pitch mutation at (voice, beat). Returns Some(delta) if accepted, None if rejected.
#[allow(clippy::too_many_arguments)]
fn try_pitch_mutation(
    grid: &mut Grid,
    models: &MarkovModels,
    weights: &ScoringWeights,
    mode: &ModeInstance,
    voice: Voice,
    beat: usize,
    temp: Score,
    rng: &mut GameRng,
) -> Option<Score> {
    let old_pitch = grid.cell(voice, beat).pitch;
    let (range_low, range_high) = voice.range();

    let old_local = score_local(grid, weights, mode, beat);

    // Build Markov context from preceding notes
    let mut context = Vec::new();
    let mut prev_pitch = None;
    for b in (0..beat).rev() {
        let cell = grid.cell(voice, b);
        if cell.attack && !cell.is_rest {
            if let Some(pp) = prev_pitch {
                let iv = cell.pitch as i8 - pp as i8;
                context.insert(0, iv);
                if context.len() >= 3 {
                    break;
                }
            }
            prev_pitch = Some(cell.pitch);
        }
    }

    let pitch_before = {
        let mut p = None;
        for b in (0..beat).rev() {
            let cell = grid.cell(voice, b);
            if !cell.is_rest {
                p = Some(cell.pitch);
                break;
            }
        }
        p.unwrap_or(old_pitch)
    };

    let rng_val: u64 = rng.next_u64();
    let proposed_interval = models.melodic.sample(&context, rng_val);
    let raw_pitch = (pitch_before as i16 + proposed_interval as i16)
        .clamp(range_low as i16, range_high as i16) as u8;
    let new_pitch = mode.snap_to_mode(raw_pitch);

    if new_pitch == old_pitch {
        return None;
    }

    // Apply
    grid.cell_mut(voice, beat).pitch = new_pitch;
    for b in (beat + 1)..grid.num_beats {
        let cell = grid.cell(voice, b);
        if cell.is_rest || cell.attack {
            break;
        }
        grid.cell_mut(voice, b).pitch = new_pitch;
    }

    let new_local = score_local(grid, weights, mode, beat);
    let delta = new_local - old_local;

    if metropolis_accept(delta, temp, rng) {
        Some(delta)
    } else {
        // Revert
        grid.cell_mut(voice, beat).pitch = old_pitch;
        for b in (beat + 1)..grid.num_beats {
            let cell = grid.cell(voice, b);
            if cell.is_rest || cell.attack {
                break;
            }
            grid.cell_mut(voice, b).pitch = old_pitch;
        }
        None
    }
}

/// Try a duration mutation at (voice, beat). Extends or shortens the note.
/// Returns Some(delta) if accepted, None if rejected.
#[allow(clippy::too_many_arguments)]
fn try_duration_mutation(
    grid: &mut Grid,
    weights: &ScoringWeights,
    mode: &ModeInstance,
    voice: Voice,
    beat: usize,
    structural: &std::collections::HashSet<(usize, usize)>,
    temp: Score,
    rng: &mut GameRng,
) -> Option<Score> {
    let cell = grid.cell(voice, beat);
    if cell.is_rest || !cell.attack {
        return None;
    }
    let pitch = cell.pitch;

    // Find the end of this note (how many continuation cells follow)
    let mut note_end = beat;
    for b in (beat + 1)..grid.num_beats {
        let c = grid.cell(voice, b);
        if c.is_rest || c.attack {
            break;
        }
        note_end = b;
    }
    let current_dur = note_end - beat + 1;

    // Decide: extend (+1) or shorten (-1)
    let extend = rng.range_usize(0, 2) == 0;

    if extend {
        // Try to extend by 1 beat
        let target = note_end + 1;
        if target >= grid.num_beats {
            return None;
        }
        if structural.contains(&(voice.index(), target)) {
            return None;
        }
        let target_cell = grid.cell(voice, target);
        if !target_cell.is_rest {
            return None; // Can't extend into another note
        }

        // Score before
        let old_local =
            score_local(grid, weights, mode, beat) + score_local(grid, weights, mode, target);

        // Apply extension
        grid.cell_mut(voice, target).pitch = pitch;
        grid.cell_mut(voice, target).is_rest = false;
        grid.cell_mut(voice, target).attack = false;

        let new_local =
            score_local(grid, weights, mode, beat) + score_local(grid, weights, mode, target);
        let delta = new_local - old_local;

        if metropolis_accept(delta, temp, rng) {
            Some(delta)
        } else {
            // Revert
            grid.cell_mut(voice, target).pitch = 0;
            grid.cell_mut(voice, target).is_rest = true;
            grid.cell_mut(voice, target).attack = false;
            None
        }
    } else {
        // Try to shorten by 1 beat (remove the last continuation)
        if current_dur <= 1 {
            return None; // Can't shorten a single-beat note
        }
        if structural.contains(&(voice.index(), note_end)) {
            return None;
        }

        let old_local =
            score_local(grid, weights, mode, beat) + score_local(grid, weights, mode, note_end);

        // Remove the last beat of the note (make it a rest)
        grid.cell_mut(voice, note_end).pitch = 0;
        grid.cell_mut(voice, note_end).is_rest = true;
        grid.cell_mut(voice, note_end).attack = false;

        let new_local =
            score_local(grid, weights, mode, beat) + score_local(grid, weights, mode, note_end);
        let delta = new_local - old_local;

        if metropolis_accept(delta, temp, rng) {
            Some(delta)
        } else {
            // Revert
            grid.cell_mut(voice, note_end).pitch = pitch;
            grid.cell_mut(voice, note_end).is_rest = false;
            grid.cell_mut(voice, note_end).attack = false;
            None
        }
    }
}

/// Metropolis acceptance criterion using integer arithmetic.
///
/// For delta >= 0: always accept (improvement).
/// For delta < 0: accept with probability exp(delta / temp).
///
/// Uses a precomputed lookup table for deterministic exp approximation.
/// The table maps integer x values (where x = delta * 1024 / temp, clamped
/// to [-10240, 0]) to acceptance thresholds in [0, u64::MAX].
fn metropolis_accept(delta: Score, temp: Score, rng: &mut GameRng) -> bool {
    if delta.raw() >= 0 {
        return true;
    }
    if temp.raw() <= 0 {
        return false;
    }

    // Compute x = delta / temp, scaled by 1024 for table lookup.
    // Using i128 to avoid overflow: (delta.raw() * 1024) / temp.raw()
    let x_scaled = (delta.raw() as i128 * 1024) / temp.raw() as i128;

    // Clamp to table range [-10240, 0]
    let x_clamped = x_scaled.clamp(-10240, 0) as i64;

    // Lookup: exp(x_clamped / 1024) * u64::MAX
    let threshold = exp_threshold(x_clamped);

    rng.next_u64() < threshold
}

/// Precomputed exp(x/1024) * u64::MAX for x in [-10240, 0].
///
/// Uses a piecewise linear approximation between anchor points computed
/// at compile time. For x < -10240, exp ≈ 0 (always reject). For x = 0,
/// exp = 1.0 (always accept, handled before this is called).
///
/// The anchor points are spaced 1024 apart (corresponding to integer
/// exp arguments -10, -9, ..., -1, 0), with linear interpolation between.
fn exp_threshold(x_scaled: i64) -> u64 {
    // Anchor points: exp(-k) * u64::MAX for k = 0, 1, ..., 10
    // Computed as: (e^(-k) * 2^64) rounded to nearest u64.
    const ANCHORS: [u64; 11] = [
        u64::MAX,            // exp(0) = 1.0
        6786177901268085487, // exp(-1) ≈ 0.3679
        2496495334008789231, // exp(-2) ≈ 0.1353
        918327078262498632,  // exp(-3) ≈ 0.0498
        337794023071187612,  // exp(-4) ≈ 0.0183
        124266580753498688,  // exp(-5) ≈ 0.00674
        45716512014715680,   // exp(-6) ≈ 0.00248
        16820555515377696,   // exp(-7) ≈ 0.000912
        6188154691388704,    // exp(-8) ≈ 0.000335
        2276172972498720,    // exp(-9) ≈ 0.000123
        837677599463360,     // exp(-10) ≈ 0.0000454
    ];

    if x_scaled >= 0 {
        return u64::MAX;
    }
    if x_scaled <= -10240 {
        return 0; // exp(-10) is already tiny; below that, reject
    }

    // Find which segment we're in: segment k corresponds to x in [-(k+1)*1024, -k*1024]
    let abs_x = (-x_scaled) as u64;
    let segment = (abs_x / 1024) as usize; // 0..=9
    let frac = abs_x % 1024; // 0..1023

    let high = ANCHORS[segment]; // exp(-segment)
    let low = ANCHORS[segment + 1]; // exp(-(segment+1))

    // Linear interpolation: high - (high - low) * frac / 1024
    // Using u128 to avoid overflow in the multiplication
    let range = high - low;
    high - ((range as u128 * frac as u128) / 1024) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::fill_draft;
    use crate::markov::{MarkovModels, MotifLibrary};
    use crate::structure::{apply_structure, generate_structure};

    #[test]
    fn test_sa_improves_score() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let weights = ScoringWeights::default();
        let mode = ModeInstance::d_dorian();
        let mut rng = GameRng::new(42);

        let plan = generate_structure(&library, 2, None, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        let _score_before = score_grid(&grid, &weights, &mode);

        let config = SAConfig {
            initial_temp: Score::from_int(5),
            final_temp: Score::from_ratio(1, 10),     // 0.1
            cooling_rate: Score::from_ratio(99, 100), // 0.99
            mutations_per_step: 5,
            max_reheats: 1,
            ..Default::default()
        };

        let result = anneal(
            &mut grid,
            &models,
            &structural,
            &weights,
            &mode,
            &config,
            &mut rng,
        );

        assert!(result.iterations > 0, "SA should have run some iterations");
        assert!(
            result.accepted > 0,
            "SA should have accepted some mutations"
        );
    }

    #[test]
    fn test_sa_with_text() {
        use crate::text_mapping::apply_text_mapping;
        use crate::vaelith::generate_phrases;
        use elven_canopy_lang::default_lexicon;

        let lexicon = default_lexicon();
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let weights = ScoringWeights::default();
        let mode = ModeInstance::d_dorian();
        let mut rng = GameRng::new(42);

        let plan = generate_structure(&library, 2, None, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        let phrases = generate_phrases(&lexicon, 2, &mut rng);
        let mut mapping = apply_text_mapping(&mut grid, &plan, &phrases);

        let config = SAConfig {
            initial_temp: Score::from_int(5),
            final_temp: Score::from_ratio(1, 10),
            cooling_rate: Score::from_ratio(99, 100),
            mutations_per_step: 5,
            max_reheats: 1,
            ..Default::default()
        };

        let result = anneal_with_text(
            &mut grid,
            &models,
            &structural,
            &weights,
            &mode,
            &config,
            &plan,
            &mut mapping,
            &phrases,
            &mut rng,
        );

        assert!(result.iterations > 0, "Text-aware SA should have run");
        assert!(result.accepted > 0, "Text-aware SA should accept mutations");
        assert!(!mapping.spans.is_empty(), "Mapping should still have spans");
    }

    #[test]
    fn test_metropolis_always_accepts_improvement() {
        let mut rng = GameRng::new(42);
        let delta = Score::from_int(5); // positive = improvement
        let temp = Score::from_int(1);
        assert!(metropolis_accept(delta, temp, &mut rng));
    }

    #[test]
    fn test_metropolis_rejects_at_zero_temp() {
        let mut rng = GameRng::new(42);
        let delta = Score::from_int(-5); // negative = worsening
        let temp = Score::ZERO;
        assert!(!metropolis_accept(delta, temp, &mut rng));
    }

    #[test]
    fn test_exp_threshold_boundary_values() {
        // exp(0) = 1.0 → u64::MAX
        assert_eq!(exp_threshold(0), u64::MAX);
        // exp(-10240) → very close to 0
        assert_eq!(exp_threshold(-10240), 0);
        // exp(-1024) should be near exp(-1) ≈ 0.3679 * u64::MAX
        let t = exp_threshold(-1024);
        // Should be roughly 0.3679 * u64::MAX ≈ 6.786e18
        assert!(t > 6_000_000_000_000_000_000);
        assert!(t < 7_500_000_000_000_000_000);
    }

    #[test]
    fn test_exp_threshold_monotonic() {
        // exp(x) is monotonically increasing, so exp_threshold should be too.
        let mut prev = 0u64;
        for x in -10239..=0i64 {
            let t = exp_threshold(x);
            assert!(
                t >= prev,
                "exp_threshold not monotonic: exp_threshold({}) = {} < exp_threshold({}) = {}",
                x,
                t,
                x - 1,
                prev
            );
            prev = t;
        }
    }

    #[test]
    fn test_exp_threshold_anchor_ratios() {
        // Adjacent anchors should have ratio ≈ e ≈ 2.718.
        // Check ANCHORS[k] / ANCHORS[k+1] ≈ e for k = 0..9.
        // Use integer ratio: ANCHORS[k] * 1000 / ANCHORS[k+1] should be ~2718.
        // Allow 1% tolerance (2691-2745).
        for k in 0..10 {
            let high = exp_threshold(-(k as i64) * 1024);
            let low = exp_threshold(-((k + 1) as i64) * 1024);
            if low == 0 {
                continue;
            }
            let ratio_millis = (high as u128 * 1000 / low as u128) as u64;
            assert!(
                (2690..=2750).contains(&ratio_millis),
                "Anchor ratio at k={k}: {ratio_millis}/1000 (expected ~2.718)"
            );
        }
    }

    #[test]
    fn test_adaptive_cooler_low_acceptance() {
        let base = Score::from_ratio(9995, 10000); // 0.9995
        let mut cooler = AdaptiveCooler::new(base);
        // Feed 200 rejections
        for _ in 0..200 {
            cooler.record(false);
        }
        let rate = cooler.effective_rate();
        // With 0% acceptance (< 5%), rate should be base + (1 - base) * 0.5
        // which is closer to 1.0 than base
        assert!(
            rate > base,
            "Low acceptance should slow cooling: rate {} should > base {}",
            rate,
            base
        );
    }

    #[test]
    fn test_adaptive_cooler_high_acceptance() {
        let base = Score::from_ratio(9995, 10000);
        let mut cooler = AdaptiveCooler::new(base);
        // Feed 200 acceptances
        for _ in 0..200 {
            cooler.record(true);
        }
        let rate = cooler.effective_rate();
        // With 100% acceptance (> 50%), rate should be base * 0.95
        assert!(
            rate < base,
            "High acceptance should speed cooling: rate {} should < base {}",
            rate,
            base
        );
    }

    #[test]
    fn test_adaptive_cooler_window_reset() {
        let base = Score::from_ratio(9995, 10000);
        let mut cooler = AdaptiveCooler::new(base);
        // Feed 200 rejections to trigger adaptation
        for _ in 0..200 {
            cooler.record(false);
        }
        let _first = cooler.effective_rate();
        // Window should have reset. Within new window, return base unchanged.
        let second = cooler.effective_rate();
        assert_eq!(second, base, "Within new window, should return base rate");
    }

    #[test]
    fn test_metropolis_large_negative_delta() {
        let mut rng = GameRng::new(42);
        let delta = Score::from_int(-10000);
        let temp = Score::ONE;
        // x_scaled = -10000 * 2^30 * 1024 / 2^30 = -10240000, clamped to -10240
        // exp_threshold(-10240) = 0, so should always reject
        assert!(!metropolis_accept(delta, temp, &mut rng));
    }
}
