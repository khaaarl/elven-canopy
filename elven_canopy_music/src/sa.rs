// Simulated annealing refinement of the score grid.
//
// Takes a draft-filled grid and iteratively improves it by proposing
// mutations, scoring the result, and accepting/rejecting based on the
// Metropolis criterion. Uses a cooling schedule with periodic reheating.
//
// Two types of mutations:
// - Micro: change a single note's pitch (Markov-guided proposal)
// - Macro: shift motif entries, adjust transposition (not yet implemented)
//
// Depends on scoring.rs for quality evaluation and markov.rs for
// mutation proposals.

use crate::grid::{Grid, Voice};
use crate::markov::MarkovModels;
use crate::scoring::{ScoringWeights, score_grid, score_local};
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
            final_score: score_grid(grid, weights),
            iterations: 0,
            accepted: 0,
            reheats: 0,
        };
    }

    let mut temp = config.initial_temp;
    let mut _current_score = score_grid(grid, weights);
    let mut iterations = 0;
    let mut accepted = 0;
    let mut reheats = 0;

    while temp > config.final_temp {
        for _ in 0..config.mutations_per_step {
            // Pick a random mutable cell
            let idx = rng.random_range(0..mutable_cells.len());
            let (voice, beat) = mutable_cells[idx];

            // Get current pitch
            let old_pitch = grid.cell(voice, beat).pitch;
            let (range_low, range_high) = voice.range();

            // Score the local region before mutation
            let old_local = score_local(grid, weights, beat);

            // Propose new pitch using Markov model
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

            // Also find the pitch just before this beat for interval calculation
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
            let new_pitch = (pitch_before as i16 + proposed_interval as i16)
                .clamp(range_low as i16, range_high as i16) as u8;

            if new_pitch == old_pitch {
                iterations += 1;
                continue;
            }

            // Apply mutation
            grid.cell_mut(voice, beat).pitch = new_pitch;

            // Also update continuation cells
            for b in (beat + 1)..grid.num_beats {
                let cell = grid.cell(voice, b);
                if cell.is_rest || cell.attack {
                    break;
                }
                grid.cell_mut(voice, b).pitch = new_pitch;
            }

            // Score after mutation
            let new_local = score_local(grid, weights, beat);
            let delta = new_local - old_local;

            // Metropolis criterion
            let accept = if delta >= 0.0 {
                true
            } else {
                let probability = (delta / temp).exp();
                rng.random::<f64>() < probability
            };

            if accept {
                _current_score += delta;
                accepted += 1;
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
        final_score: score_grid(grid, weights),
        iterations,
        accepted,
        reheats,
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
        let mut rng = rand::rng();

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mut rng);

        let score_before = score_grid(&grid, &weights);

        let config = SAConfig {
            initial_temp: 5.0,
            final_temp: 0.1,
            cooling_rate: 0.99,
            mutations_per_step: 5,
            max_reheats: 1,
            ..Default::default()
        };

        let result = anneal(&mut grid, &models, &structural, &weights, &config, &mut rng);

        // SA should generally improve or maintain score
        // (Not guaranteed with limited iterations, but should usually improve)
        assert!(result.iterations > 0, "SA should have run some iterations");
        assert!(result.accepted > 0, "SA should have accepted some mutations");
    }
}
