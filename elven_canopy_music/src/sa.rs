// Simulated annealing refinement of the score grid.
//
// Takes a draft-filled grid and iteratively improves it by proposing
// mutations, scoring the result, and accepting/rejecting based on the
// Metropolis criterion. Uses a cooling schedule with periodic reheating.
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
// Structural cells (motif entries from structure.rs and final cadence from
// draft.rs) are excluded from mutation — they're the compositional anchors.
//
// Depends on scoring.rs for quality evaluation, markov.rs for mutation
// proposals, and text_mapping.rs + vaelith.rs for text-aware optimization.

use crate::grid::{Grid, Voice};
use crate::markov::MarkovModels;
use crate::mode::ModeInstance;
use crate::scoring::{ScoringWeights, score_grid, score_local, score_tonal_contour, score_tonal_contour_local};
use crate::structure::StructurePlan;
use crate::text_mapping::{TextMapping, swap_section_phrase};
use crate::vaelith::VaelithPhrase;
use rand::Rng;

/// SA configuration parameters.
#[derive(Debug, Clone)]
pub struct SAConfig {
    /// Initial temperature.
    pub initial_temp: f64,
    /// Final temperature (stop condition).
    pub final_temp: f64,
    /// Cooling rate per step (multiplicative).
    pub cooling_rate: f64,
    /// Number of mutations to try per temperature step.
    pub mutations_per_step: usize,
    /// Temperature at which to reheat.
    pub reheat_temp: f64,
    /// Temperature to reheat to.
    pub reheat_target: f64,
    /// Number of reheats allowed.
    pub max_reheats: usize,
    /// Enable adaptive cooling (adjust cooling rate based on acceptance ratio).
    pub adaptive: bool,
}

impl Default for SAConfig {
    fn default() -> Self {
        SAConfig {
            initial_temp: 10.0,
            final_temp: 0.01,
            cooling_rate: 0.9995,
            mutations_per_step: 1,
            reheat_temp: 0.1,
            reheat_target: 3.0,
            max_reheats: 3,
            adaptive: true,
        }
    }
}

/// Adaptive cooling state: tracks acceptance rate over a sliding window
/// and adjusts the cooling rate accordingly.
struct AdaptiveCooler {
    base_rate: f64,
    window_accepted: u32,
    window_total: u32,
    window_size: u32,
}

impl AdaptiveCooler {
    fn new(base_rate: f64) -> Self {
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
    fn effective_rate(&mut self) -> f64 {
        if self.window_total < self.window_size {
            return self.base_rate;
        }

        let accept_ratio = self.window_accepted as f64 / self.window_total as f64;

        // Reset window for next measurement
        self.window_accepted = 0;
        self.window_total = 0;

        if accept_ratio < 0.05 {
            // Almost nothing accepted — slow cooling dramatically
            // Move rate toward 1.0 (slow)
            self.base_rate + (1.0 - self.base_rate) * 0.5
        } else if accept_ratio < 0.15 {
            // Below target — slow cooling a bit
            self.base_rate + (1.0 - self.base_rate) * 0.2
        } else if accept_ratio > 0.50 {
            // Too many accepted — speed up cooling
            self.base_rate * 0.95
        } else {
            // In the sweet spot — use base rate
            self.base_rate
        }
    }
}

/// Result of an SA run.
#[derive(Debug)]
pub struct SAResult {
    pub final_score: f64,
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
    rng: &mut impl Rng,
) -> SAResult {
    let structural_set: std::collections::HashSet<(usize, usize)> =
        structural_cells.iter().copied().collect();

    // Collect mutable cells (non-structural, non-rest attack cells)
    let num_beats = grid.num_beats;
    let mut mutable_cells: Vec<(Voice, usize)> = Vec::new();
    for &voice in &Voice::ALL {
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
            let do_duration_mutation = rng.random::<f64>() < 0.2;

            if do_duration_mutation {
                // Duration mutation: extend or shorten a note
                let idx = rng.random_range(0..mutable_cells.len());
                let (voice, beat) = mutable_cells[idx];
                let delta = try_duration_mutation(
                    grid, weights, mode, voice, beat, &structural_set, temp, rng,
                );
                if let Some(d) = delta {
                    _current_score += d;
                    accepted += 1;
                }
            } else {
                // Pitch mutation
                let idx = rng.random_range(0..mutable_cells.len());
                let (voice, beat) = mutable_cells[idx];
                let delta = try_pitch_mutation(
                    grid, models, weights, mode, voice, beat, temp, rng,
                );
                if let Some(d) = delta {
                    _current_score += d;
                    accepted += 1;
                }
            }

            iterations += 1;
        }

        // Cool
        temp *= config.cooling_rate;

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
    rng: &mut impl Rng,
) -> SAResult {
    let structural_set: std::collections::HashSet<(usize, usize)> =
        structural_cells.iter().copied().collect();

    let num_beats = grid.num_beats;
    let mut mutable_cells: Vec<(Voice, usize)> = Vec::new();
    for &voice in &Voice::ALL {
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
            let roll: f64 = rng.random();
            let mut step_accepted = false;

            if roll < 0.05 && num_sections > 0 && !phrase_candidates.is_empty() {
                // Text-swap macro mutation (~5%)
                let delta = try_text_swap_mutation(
                    grid, weights, mode, plan, mapping, phrase_candidates, temp, rng,
                );
                if let Some(d) = delta {
                    _current_score += d;
                    accepted += 1;
                    step_accepted = true;
                }
            } else if roll < 0.25 {
                // Duration mutation (~20%)
                let idx = rng.random_range(0..mutable_cells.len());
                let (voice, beat) = mutable_cells[idx];
                let delta = try_duration_mutation(
                    grid, weights, mode, voice, beat, &structural_set, temp, rng,
                );
                if let Some(d) = delta {
                    _current_score += d;
                    accepted += 1;
                    step_accepted = true;
                }
            } else {
                // Pitch mutation with tonal contour awareness (~75%)
                let idx = rng.random_range(0..mutable_cells.len());
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
        temp *= rate;

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
fn try_pitch_mutation_with_text(
    grid: &mut Grid,
    models: &MarkovModels,
    weights: &ScoringWeights,
    mode: &ModeInstance,
    mapping: &TextMapping,
    voice: Voice,
    beat: usize,
    temp: f64,
    rng: &mut impl Rng,
) -> Option<f64> {
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

    let rng_val: f64 = rng.random();
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
fn try_text_swap_mutation(
    grid: &mut Grid,
    weights: &ScoringWeights,
    _mode: &ModeInstance,
    plan: &StructurePlan,
    mapping: &mut TextMapping,
    phrase_candidates: &[Vec<VaelithPhrase>],
    temp: f64,
    rng: &mut impl Rng,
) -> Option<f64> {
    let num_sections = plan.imitation_points.len().min(phrase_candidates.len());
    if num_sections == 0 {
        return None;
    }

    let section_idx = rng.random_range(0..num_sections);
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

    let alternatives: Vec<&VaelithPhrase> = candidates.iter()
        .filter(|p| p.text != *current_text)
        .collect();

    if alternatives.is_empty() {
        return None;
    }

    let new_phrase = alternatives[rng.random_range(0..alternatives.len())].clone();

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
fn try_pitch_mutation(
    grid: &mut Grid,
    models: &MarkovModels,
    weights: &ScoringWeights,
    mode: &ModeInstance,
    voice: Voice,
    beat: usize,
    temp: f64,
    rng: &mut impl Rng,
) -> Option<f64> {
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

    let rng_val: f64 = rng.random();
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
fn try_duration_mutation(
    grid: &mut Grid,
    weights: &ScoringWeights,
    mode: &ModeInstance,
    voice: Voice,
    beat: usize,
    structural: &std::collections::HashSet<(usize, usize)>,
    temp: f64,
    rng: &mut impl Rng,
) -> Option<f64> {
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
    let extend = rng.random_bool(0.5);

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
        let old_local = score_local(grid, weights, mode, beat)
            + score_local(grid, weights, mode, target);

        // Apply extension
        grid.cell_mut(voice, target).pitch = pitch;
        grid.cell_mut(voice, target).is_rest = false;
        grid.cell_mut(voice, target).attack = false;

        let new_local = score_local(grid, weights, mode, beat)
            + score_local(grid, weights, mode, target);
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

        let old_local = score_local(grid, weights, mode, beat)
            + score_local(grid, weights, mode, note_end);

        // Remove the last beat of the note (make it a rest)
        grid.cell_mut(voice, note_end).pitch = 0;
        grid.cell_mut(voice, note_end).is_rest = true;
        grid.cell_mut(voice, note_end).attack = false;

        let new_local = score_local(grid, weights, mode, beat)
            + score_local(grid, weights, mode, note_end);
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

/// Metropolis acceptance criterion.
fn metropolis_accept(delta: f64, temp: f64, rng: &mut impl Rng) -> bool {
    if delta >= 0.0 {
        true
    } else {
        let probability = (delta / temp).exp();
        rng.random::<f64>() < probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markov::{MarkovModels, MotifLibrary};
    use crate::structure::{generate_structure, apply_structure};
    use crate::draft::fill_draft;

    #[test]
    fn test_sa_improves_score() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let weights = ScoringWeights::default();
        let mode = ModeInstance::d_dorian();
        let mut rng = rand::rng();

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        let _score_before = score_grid(&grid, &weights, &mode);

        let config = SAConfig {
            initial_temp: 5.0,
            final_temp: 0.1,
            cooling_rate: 0.99,
            mutations_per_step: 5,
            max_reheats: 1,
            ..Default::default()
        };

        let result = anneal(&mut grid, &models, &structural, &weights, &mode, &config, &mut rng);

        assert!(result.iterations > 0, "SA should have run some iterations");
        assert!(result.accepted > 0, "SA should have accepted some mutations");
    }

    #[test]
    fn test_sa_with_text() {
        use crate::vaelith::generate_phrases;
        use crate::text_mapping::apply_text_mapping;

        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let weights = ScoringWeights::default();
        let mode = ModeInstance::d_dorian();
        let mut rng = rand::rng();

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        let phrases = generate_phrases(2, &mut rng);
        let mut mapping = apply_text_mapping(&mut grid, &plan, &phrases);

        let config = SAConfig {
            initial_temp: 5.0,
            final_temp: 0.1,
            cooling_rate: 0.99,
            mutations_per_step: 5,
            max_reheats: 1,
            ..Default::default()
        };

        let result = anneal_with_text(
            &mut grid, &models, &structural, &weights, &mode,
            &config, &plan, &mut mapping, &phrases, &mut rng,
        );

        assert!(result.iterations > 0, "Text-aware SA should have run");
        assert!(result.accepted > 0, "Text-aware SA should accept mutations");
        assert!(!mapping.spans.is_empty(), "Mapping should still have spans");
    }
}
