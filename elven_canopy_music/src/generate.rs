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
use crate::grid::{Grid, Voice};
use crate::markov::{MarkovModels, MotifLibrary};
use crate::mode::Score;
use crate::mode::{Mode, ModeInstance};
use crate::sa::{SAConfig, anneal_with_text};
use crate::scoring::ScoringWeights;
use crate::structure::{apply_responses, apply_structure, generate_structure_for_voices};
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
    /// Which SATB voices to include. Defaults to all four if empty or None.
    /// Pass e.g. `vec![Voice::Soprano]` for a solo, or
    /// `vec![Voice::Soprano, Voice::Alto]` for a duet.
    pub voices: Vec<Voice>,
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

    let active_voices = if params.voices.is_empty() {
        Voice::ALL.to_vec()
    } else {
        params.voices.clone()
    };

    let plan = generate_structure_for_voices(
        &motif_library,
        params.sections,
        params.max_beats,
        &active_voices,
        &mut rng,
    );

    let mut grid = Grid::new_with_voices(plan.total_beats, &active_voices);
    grid.tempo_bpm = params.tempo_bpm;
    let mut structural = apply_structure(&mut grid, &plan);
    apply_responses(&mut grid, &plan, &mode_inst, &mut structural);
    fill_draft(&mut grid, &models, &structural, &mode_inst, &mut rng);
    generate_final_cadence(&mut grid, &mode_inst, &mut structural);

    // Convert f32 brightness (0.0–1.0) to integer (0–1000) at the API boundary.
    let brightness_int = (params.brightness.clamp(0.0, 1.0) * 1000.0) as u32;
    let phrase_candidates =
        generate_phrases_with_brightness(&lexicon, params.sections, brightness_int, &mut rng);
    let mut mapping = apply_text_mapping(&mut grid, &plan, &phrase_candidates);

    // cooling_rate = 1 - 1/sa_iterations, as fixed-point.
    // from_ratio(sa_iterations - 1, sa_iterations)
    let sa_iters = params.sa_iterations.max(2) as i64;
    let sa_config = SAConfig {
        cooling_rate: Score::from_ratio(sa_iters - 1, sa_iters),
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
            voices: Vec::new(),
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
            voices: Vec::new(),
        };
        let grid1 = generate_piece(&params);
        let grid2 = generate_piece(&params);
        // Same seed → identical grids
        assert_eq!(grid1.num_beats, grid2.num_beats);
        for &voice in grid1.active_voices() {
            let v = voice.index();
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
            voices: Vec::new(),
        };
        let capped = GenerateParams {
            seed: 77,
            sections: 3,
            mode_index: 0,
            brightness: 0.5,
            sa_iterations: 500,
            tempo_bpm: 80,
            max_beats: Some(20),
            voices: Vec::new(),
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

    #[test]
    fn generate_piece_solo_soprano() {
        let params = GenerateParams {
            seed: 42,
            sections: 2,
            mode_index: 0,
            brightness: 0.5,
            sa_iterations: 500,
            tempo_bpm: 72,
            max_beats: None,
            voices: vec![Voice::Soprano],
        };
        let grid = generate_piece(&params);
        assert_eq!(grid.active_voices(), &[Voice::Soprano]);
        assert!(grid.num_beats > 0);
        let stats = grid.stats();
        assert!(stats.total_attacks > 0, "Solo should have notes");
        // Other voice rows should be empty
        assert!(grid.voices[Voice::Alto.index()].is_empty());
        assert!(grid.voices[Voice::Tenor.index()].is_empty());
        assert!(grid.voices[Voice::Bass.index()].is_empty());
    }

    #[test]
    fn generate_piece_duet() {
        let params = GenerateParams {
            seed: 42,
            sections: 2,
            mode_index: 0,
            brightness: 0.5,
            sa_iterations: 500,
            tempo_bpm: 72,
            max_beats: None,
            voices: vec![Voice::Soprano, Voice::Alto],
        };
        let grid = generate_piece(&params);
        assert_eq!(grid.active_voices(), &[Voice::Soprano, Voice::Alto]);
        let stats = grid.stats();
        assert!(stats.total_attacks > 0);
        // Inactive voices should be empty
        assert!(grid.voices[Voice::Tenor.index()].is_empty());
        assert!(grid.voices[Voice::Bass.index()].is_empty());
    }

    #[test]
    fn generate_piece_solo_is_deterministic() {
        let params = GenerateParams {
            seed: 99,
            sections: 2,
            mode_index: 0,
            brightness: 0.5,
            sa_iterations: 500,
            tempo_bpm: 72,
            max_beats: None,
            voices: vec![Voice::Soprano],
        };
        let grid1 = generate_piece(&params);
        let grid2 = generate_piece(&params);
        assert_eq!(grid1.num_beats, grid2.num_beats);
        for b in 0..grid1.num_beats {
            assert_eq!(
                grid1.voices[0][b], grid2.voices[0][b],
                "Mismatch at beat {b}"
            );
        }
    }

    /// Smoke test: run generate_piece for every non-empty subset of SATB.
    /// Ensures no panics for any voice combination.
    #[test]
    fn generate_piece_all_voice_combinations() {
        let all = [Voice::Soprano, Voice::Alto, Voice::Tenor, Voice::Bass];
        // Iterate all 15 non-empty subsets (bitmask 1..=15)
        for mask in 1u8..=15 {
            let voices: Vec<Voice> = all
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, v)| *v)
                .collect();

            let params = GenerateParams {
                seed: 42,
                sections: 2,
                mode_index: 0,
                brightness: 0.5,
                sa_iterations: 200,
                tempo_bpm: 72,
                max_beats: Some(30),
                voices: voices.clone(),
            };
            let grid = generate_piece(&params);
            assert!(grid.num_beats > 0, "mask {mask}: no beats");
            let stats = grid.stats();
            assert!(stats.total_attacks > 0, "mask {mask}: no notes");

            // Inactive voices must have empty rows
            for &v in &all {
                if !voices.contains(&v) {
                    assert!(
                        grid.voices[v.index()].is_empty(),
                        "mask {mask}: voice {:?} should be inactive",
                        v
                    );
                }
            }
        }
    }

    #[test]
    fn generate_piece_brightness_boundaries() {
        // Verify the f32→u32 boundary conversion works at extremes.
        for brightness in [0.0f32, 1.0, 0.5] {
            let params = GenerateParams {
                seed: 42,
                sections: 2,
                mode_index: 0,
                brightness,
                sa_iterations: 200,
                tempo_bpm: 72,
                max_beats: Some(20),
                voices: Vec::new(),
            };
            let grid = generate_piece(&params);
            assert!(grid.num_beats > 0);
            assert!(grid.stats().total_attacks > 0);
        }
    }
}
