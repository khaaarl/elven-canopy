// High-level generation API for runtime use.
//
// Provides `generate_piece()` which runs the full composition pipeline
// (structure planning → draft → SA refinement) and returns a Grid ready
// for synthesis. This is the function the game runtime calls on a background
// thread when a new construction project is designated.
//
// The pipeline is deterministic given a seed — same seed + params produces
// an identical Grid every time.

use crate::draft::{fill_draft, generate_final_cadence};
use crate::grid::Grid;
use crate::markov::{MarkovModels, MotifLibrary};
use crate::mode::{Mode, ModeInstance};
use crate::sa::{SAConfig, anneal_with_text};
use crate::scoring::ScoringWeights;
use crate::structure::{apply_responses, apply_structure, generate_structure};
use crate::text_mapping::apply_text_mapping;
use crate::vaelith::generate_phrases_with_brightness;
use elven_canopy_lang::default_lexicon;
use elven_canopy_prng::GameRng;

/// Mode + standard final pitch class pairs, indexed by mode_index (0–5).
const ALL_MODES: [(Mode, u8); 6] = [
    (Mode::Dorian, 2),     // D Dorian
    (Mode::Phrygian, 4),   // E Phrygian
    (Mode::Lydian, 5),     // F Lydian
    (Mode::Mixolydian, 7), // G Mixolydian
    (Mode::Aeolian, 9),    // A Aeolian
    (Mode::Ionian, 0),     // C Ionian
];

/// Parameters for runtime music generation.
pub struct GenerateParams {
    pub seed: u64,
    pub sections: usize,
    pub mode_index: usize,
    pub brightness: f32,
    pub sa_iterations: usize,
    pub tempo_bpm: u16,
    /// If set, caps structure generation at this many eighth-note beats.
    /// The final cadence (8 beats) is always appended, so the actual grid
    /// may be up to `max_beats + 8` beats long.
    pub max_beats: Option<usize>,
}

/// Run the full composition pipeline and return the resulting Grid.
///
/// This is CPU-intensive (hundreds of milliseconds to seconds depending on
/// `sa_iterations`) and should be called on a background thread.
pub fn generate_piece(params: &GenerateParams) -> Grid {
    let mut rng = GameRng::new(params.seed);

    let lexicon = default_lexicon();
    let models = MarkovModels::default_models();
    let motif_library = MotifLibrary::default_library();
    let weights = ScoringWeights::default();

    let (mode, final_pc) = ALL_MODES[params.mode_index % ALL_MODES.len()];
    let mode_inst = ModeInstance::new(mode, final_pc);

    let plan = generate_structure(&motif_library, params.sections, params.max_beats, &mut rng);

    let mut grid = Grid::new(plan.total_beats);
    grid.tempo_bpm = params.tempo_bpm;
    let mut structural = apply_structure(&mut grid, &plan);
    apply_responses(&mut grid, &plan, &mode_inst, &mut structural);
    fill_draft(&mut grid, &models, &structural, &mode_inst, &mut rng);
    generate_final_cadence(&mut grid, &mode_inst, &mut structural);

    let phrase_candidates = generate_phrases_with_brightness(
        &lexicon,
        params.sections,
        params.brightness as f64,
        &mut rng,
    );
    let mut mapping = apply_text_mapping(&mut grid, &plan, &phrase_candidates);

    let sa_config = SAConfig {
        cooling_rate: 1.0 - (1.0 / params.sa_iterations as f64),
        ..Default::default()
    };
    anneal_with_text(
        &mut grid,
        &models,
        &structural,
        &weights,
        &mode_inst,
        &sa_config,
        &plan,
        &mut mapping,
        &phrase_candidates,
        &mut rng,
    );

    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_piece_produces_nonempty_grid() {
        let params = GenerateParams {
            seed: 42,
            sections: 2,
            mode_index: 0,
            brightness: 0.5,
            sa_iterations: 500, // Low for test speed
            tempo_bpm: 72,
            max_beats: None,
        };
        let grid = generate_piece(&params);
        assert!(grid.num_beats > 0);
        // Should have at least some non-rest cells
        let stats = grid.stats();
        assert!(stats.total_attacks > 0);
    }

    #[test]
    fn generate_piece_is_deterministic() {
        let params = GenerateParams {
            seed: 123,
            sections: 2,
            mode_index: 1,
            brightness: 0.3,
            sa_iterations: 500,
            tempo_bpm: 80,
            max_beats: None,
        };
        let grid1 = generate_piece(&params);
        let grid2 = generate_piece(&params);
        // Same seed → identical grids
        assert_eq!(grid1.num_beats, grid2.num_beats);
        for v in 0..4 {
            for b in 0..grid1.num_beats {
                assert_eq!(
                    grid1.voices[v][b], grid2.voices[v][b],
                    "Mismatch at voice {v}, beat {b}"
                );
            }
        }
    }

    #[test]
    fn max_beats_caps_composition_length() {
        let uncapped = GenerateParams {
            seed: 77,
            sections: 3,
            mode_index: 0,
            brightness: 0.5,
            sa_iterations: 500,
            tempo_bpm: 80,
            max_beats: None,
        };
        let capped = GenerateParams {
            seed: 77,
            sections: 3,
            mode_index: 0,
            brightness: 0.5,
            sa_iterations: 500,
            tempo_bpm: 80,
            max_beats: Some(20),
        };
        let grid_full = generate_piece(&uncapped);
        let grid_short = generate_piece(&capped);
        // The capped grid should be substantially shorter.
        assert!(
            grid_short.num_beats < grid_full.num_beats,
            "Capped grid ({} beats) should be shorter than uncapped ({} beats)",
            grid_short.num_beats,
            grid_full.num_beats,
        );
        // Should still have musical content.
        let stats = grid_short.stats();
        assert!(stats.total_attacks > 0);
    }
}
