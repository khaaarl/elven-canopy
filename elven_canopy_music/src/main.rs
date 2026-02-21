// Elven Canopy Music Generator — CLI entry point.
//
// Generates a Palestrina-style four-voice choral piece and writes it to MIDI.
// The pipeline: structure planning → draft generation → SA refinement → MIDI output.
//
// Usage:
//   cargo run -p elven_canopy_music -- [output.mid] [--sections N] [--sa-iterations N]
//     [--seed N] [--mode MODE] [--tempo BPM]
//
// Modes: dorian, phrygian, lydian, mixolydian, aeolian, ionian

use elven_canopy_music::draft::{fill_draft, generate_final_cadence};
use elven_canopy_music::grid::Grid;
use elven_canopy_music::markov::{MarkovModels, MotifLibrary};
use elven_canopy_music::midi::write_midi;
use elven_canopy_music::mode::{Mode, ModeInstance};
use elven_canopy_music::sa::{SAConfig, anneal};
use elven_canopy_music::scoring::{ScoringWeights, score_grid};
use elven_canopy_music::structure::{generate_structure, apply_structure};
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse arguments
    let output_path = args.get(1)
        .filter(|s| !s.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or("output.mid");
    let num_sections = parse_flag(&args, "--sections").unwrap_or(3);
    let sa_iters = parse_flag(&args, "--sa-iterations").unwrap_or(10000);
    let seed: Option<u64> = parse_flag(&args, "--seed");
    let tempo: u16 = parse_flag(&args, "--tempo").unwrap_or(72);
    let mode_name: String = parse_flag(&args, "--mode").unwrap_or_else(|| "dorian".to_string());

    // Parse mode
    let mode = parse_mode(&mode_name);

    println!("=== Elven Canopy Music Generator ===");
    println!("Output: {}", output_path);
    println!("Mode: {:?} (final = {})", mode.mode, pitch_name(mode.final_pc));
    println!("Tempo: {} BPM", tempo);
    println!("Sections: {}", num_sections);
    println!("SA iterations target: ~{}", sa_iters);
    if let Some(s) = seed {
        println!("Seed: {}", s);
    }
    println!();

    // Initialize RNG
    let mut rng = if let Some(s) = seed {
        StdRng::seed_from_u64(s)
    } else {
        StdRng::from_os_rng()
    };

    // Load models
    println!("[1/5] Loading models...");
    let models = MarkovModels::default_models();
    let motif_library = MotifLibrary::default_library();
    let weights = ScoringWeights::default();

    let models = if Path::new("data/markov_models.json").exists() {
        println!("  Found trained Markov models, loading...");
        match MarkovModels::load(Path::new("data/markov_models.json")) {
            Ok(m) => { println!("  Loaded successfully."); m }
            Err(e) => { println!("  Failed to load: {}. Using defaults.", e); models }
        }
    } else {
        println!("  Using default models.");
        models
    };

    let motif_library = if Path::new("data/motif_library.json").exists() {
        println!("  Found motif library, loading...");
        match MotifLibrary::load(Path::new("data/motif_library.json")) {
            Ok(l) => { println!("  Loaded {} motifs.", l.motifs.len()); l }
            Err(e) => { println!("  Failed to load: {}. Using defaults.", e); motif_library }
        }
    } else {
        println!("  Using default motif library ({} built-in motifs).", motif_library.motifs.len());
        motif_library
    };

    // Generate structure
    println!("[2/5] Planning structure ({} sections)...", num_sections);
    let plan = generate_structure(&motif_library, num_sections, &mut rng);
    println!("  Total beats: {} ({:.1} bars of 4/4)",
        plan.total_beats, plan.total_beats as f64 / 8.0);
    for (i, point) in plan.imitation_points.iter().enumerate() {
        println!("  Section {}: {} voice entries, motif of {} intervals",
            i + 1, point.entries.len(), point.motif.intervals.len());
    }

    // Create grid and apply structure
    println!("[3/5] Generating draft...");
    let mut grid = Grid::new(plan.total_beats);
    grid.tempo_bpm = tempo;
    let structural = apply_structure(&mut grid, &plan);
    println!("  {} structural cells placed.", structural.len());

    fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

    // Generate a proper final cadence
    let mut structural = structural;
    generate_final_cadence(&mut grid, &mode, &mut structural);
    println!("  {} total structural cells (including final cadence).", structural.len());

    let draft_score = score_grid(&grid, &weights, &mode);
    println!("  Draft score: {:.1}", draft_score);

    // SA refinement
    println!("[4/5] Refining with simulated annealing...");
    let config = SAConfig {
        cooling_rate: 1.0 - (1.0 / sa_iters as f64),
        ..Default::default()
    };
    let result = anneal(&mut grid, &models, &structural, &weights, &mode, &config, &mut rng);
    println!("  Iterations: {}", result.iterations);
    println!("  Accepted: {} ({:.1}%)",
        result.accepted,
        if result.iterations > 0 { result.accepted as f64 / result.iterations as f64 * 100.0 } else { 0.0 });
    println!("  Reheats: {}", result.reheats);
    println!("  Score: {:.1} -> {:.1} (delta {:+.1})",
        draft_score, result.final_score, result.final_score - draft_score);

    // Write MIDI
    println!("[5/5] Writing MIDI to {}...", output_path);
    match write_midi(&grid, Path::new(output_path)) {
        Ok(()) => {
            let duration_seconds = grid.num_beats as f64 / (grid.tempo_bpm as f64 / 60.0 * 2.0);
            println!("  Done! Duration: {:.0}s ({:.1} bars)",
                duration_seconds, grid.num_beats as f64 / 8.0);
        }
        Err(e) => {
            eprintln!("  Error writing MIDI: {}", e);
            std::process::exit(1);
        }
    }

    println!();
    println!("Play with: timidity {} (or any MIDI player)", output_path);
}

fn parse_mode(name: &str) -> ModeInstance {
    match name.to_lowercase().as_str() {
        "dorian" => ModeInstance::new(Mode::Dorian, 2),         // D Dorian
        "phrygian" => ModeInstance::new(Mode::Phrygian, 4),     // E Phrygian
        "lydian" => ModeInstance::new(Mode::Lydian, 5),         // F Lydian
        "mixolydian" => ModeInstance::new(Mode::Mixolydian, 7), // G Mixolydian
        "aeolian" => ModeInstance::new(Mode::Aeolian, 9),       // A Aeolian
        "ionian" => ModeInstance::new(Mode::Ionian, 0),         // C Ionian
        _ => {
            eprintln!("Unknown mode '{}'. Using Dorian.", name);
            ModeInstance::d_dorian()
        }
    }
}

fn pitch_name(pc: u8) -> &'static str {
    match pc % 12 {
        0 => "C", 1 => "C#", 2 => "D", 3 => "Eb",
        4 => "E", 5 => "F", 6 => "F#", 7 => "G",
        8 => "Ab", 9 => "A", 10 => "Bb", 11 => "B",
        _ => "?"
    }
}

fn parse_flag<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    args.iter().position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}
