// Elven Canopy Music Generator — CLI entry point.
//
// Generates a Palestrina-style four-voice choral piece and writes it to MIDI.
// The pipeline: structure planning → draft generation → SA refinement → MIDI output.
//
// Usage:
//   cargo run -p elven_canopy_music -- [output.mid] [--sections N] [--sa-iterations N]
//     [--seed N] [--mode MODE] [--tempo BPM] [-v|--verbose]
//
//   Batch mode:
//   cargo run -p elven_canopy_music -- --batch N [--output-dir DIR] [other flags]
//
// Modes: dorian, phrygian, lydian, mixolydian, aeolian, ionian

use elven_canopy_music::draft::{fill_draft, generate_final_cadence};
use elven_canopy_music::grid::Grid;
use elven_canopy_music::markov::{MarkovModels, MotifLibrary};
use elven_canopy_music::midi::write_midi;
use elven_canopy_music::mode::{Mode, ModeInstance};
use elven_canopy_music::sa::{SAConfig, anneal_with_text};
use elven_canopy_music::scoring::{ScoringWeights, score_grid, score_tonal_contour};
use elven_canopy_music::structure::{generate_structure, apply_structure, apply_responses};
use elven_canopy_music::text_mapping::apply_text_mapping;
use elven_canopy_music::vaelith::generate_phrases;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Check for batch mode
    if args.iter().any(|a| a == "--batch") {
        run_batch(&args);
        return;
    }

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
    let mut structural = apply_structure(&mut grid, &plan);
    println!("  {} structural cells placed.", structural.len());
    apply_responses(&mut grid, &plan, &mode, &mut structural);
    if !plan.response_points.is_empty() {
        println!("  {} response markers (dai/thol) applied.", plan.response_points.len());
    }

    fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

    // Generate a proper final cadence
    generate_final_cadence(&mut grid, &mode, &mut structural);
    println!("  {} total structural cells (including cadence+responses).", structural.len());

    // Generate Vaelith text and apply text mapping
    println!("  Generating Vaelith text...");
    let phrase_candidates = generate_phrases(num_sections, &mut rng);
    let mut mapping = apply_text_mapping(&mut grid, &plan, &phrase_candidates);
    println!("  {} syllable spans mapped across {} section phrases.",
        mapping.spans.len(), mapping.section_phrases.len());
    for (i, phrase) in mapping.section_phrases.iter().enumerate() {
        println!("    Section {}: \"{}\" ({})", i + 1, phrase.text, phrase.meaning);
    }

    let draft_score = score_grid(&grid, &weights, &mode);
    let draft_text_score = score_tonal_contour(&grid, &mapping, &weights);
    println!("  Draft score: {:.1} (counterpoint) + {:.1} (tonal) = {:.1}",
        draft_score, draft_text_score, draft_score + draft_text_score);

    // SA refinement with text awareness
    println!("[4/5] Refining with text-aware simulated annealing...");
    let config = SAConfig {
        cooling_rate: 1.0 - (1.0 / sa_iters as f64),
        ..Default::default()
    };
    let result = anneal_with_text(
        &mut grid, &models, &structural, &weights, &mode,
        &config, &plan, &mut mapping, &phrase_candidates, &mut rng,
    );
    println!("  Iterations: {}", result.iterations);
    println!("  Accepted: {} ({:.1}%)",
        result.accepted,
        if result.iterations > 0 { result.accepted as f64 / result.iterations as f64 * 100.0 } else { 0.0 });
    println!("  Reheats: {}", result.reheats);
    let total_draft = draft_score + draft_text_score;
    println!("  Score: {:.1} -> {:.1} (delta {:+.1})",
        total_draft, result.final_score, result.final_score - total_draft);
    println!("  Final text phrases:");
    for (i, phrase) in mapping.section_phrases.iter().enumerate() {
        println!("    Section {}: \"{}\" ({})", i + 1, phrase.text, phrase.meaning);
    }

    // Show grid summary if verbose
    if args.iter().any(|a| a == "--verbose" || a == "-v") {
        println!();
        println!("Grid summary:");
        print!("{}", grid.summary());
        let stats = grid.stats();
        println!("  {} attacks, {} sounding beats, {} rests",
            stats.total_attacks, stats.total_sounding, stats.rests);
        println!();
    }

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

/// Generate a batch of pieces with sequential seeds for comparison/rating.
fn run_batch(args: &[String]) {
    let count: usize = parse_flag(args, "--batch").unwrap_or(10);
    let num_sections: usize = parse_flag(args, "--sections").unwrap_or(3);
    let sa_iters: usize = parse_flag(args, "--sa-iterations").unwrap_or(10000);
    let tempo: u16 = parse_flag(args, "--tempo").unwrap_or(72);
    let mode_name: String = parse_flag(args, "--mode").unwrap_or_else(|| "dorian".to_string());
    let base_seed: u64 = parse_flag(args, "--seed").unwrap_or(1);
    let output_dir: String = parse_flag(args, "--output-dir").unwrap_or_else(|| ".tmp/batch".to_string());

    let mode = parse_mode(&mode_name);
    let weights = ScoringWeights::default();

    println!("=== Batch Generation: {} pieces ===", count);
    println!("Mode: {:?}, Tempo: {}, Sections: {}", mode.mode, tempo, num_sections);
    println!("Output dir: {}", output_dir);
    println!();

    // Create output directory
    std::fs::create_dir_all(&output_dir).expect("Failed to create output directory");

    // Load models once
    let models = if Path::new("data/markov_models.json").exists() {
        MarkovModels::load(Path::new("data/markov_models.json"))
            .unwrap_or_else(|_| MarkovModels::default_models())
    } else {
        MarkovModels::default_models()
    };

    let motif_library = if Path::new("data/motif_library.json").exists() {
        MotifLibrary::load(Path::new("data/motif_library.json"))
            .unwrap_or_else(|_| MotifLibrary::default_library())
    } else {
        MotifLibrary::default_library()
    };

    let config = SAConfig {
        cooling_rate: 1.0 - (1.0 / sa_iters as f64),
        ..Default::default()
    };

    println!("{:>5} {:>10} {:>10} {:>10} {:>8}", "Seed", "Draft", "Final", "Delta", "Accept%");
    println!("{}", "-".repeat(50));

    for i in 0..count {
        let seed = base_seed + i as u64;
        let mut rng = StdRng::seed_from_u64(seed);

        let plan = generate_structure(&motif_library, num_sections, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        grid.tempo_bpm = tempo;
        let mut structural = apply_structure(&mut grid, &plan);
        apply_responses(&mut grid, &plan, &mode, &mut structural);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);
        generate_final_cadence(&mut grid, &mode, &mut structural);

        let phrase_candidates = generate_phrases(num_sections, &mut rng);
        let mut mapping = apply_text_mapping(&mut grid, &plan, &phrase_candidates);

        let draft_score = score_grid(&grid, &weights, &mode)
            + score_tonal_contour(&grid, &mapping, &weights);
        let result = anneal_with_text(
            &mut grid, &models, &structural, &weights, &mode,
            &config, &plan, &mut mapping, &phrase_candidates, &mut rng,
        );

        let accept_pct = if result.iterations > 0 {
            result.accepted as f64 / result.iterations as f64 * 100.0
        } else { 0.0 };

        let output_path = format!("{}/piece_{:04}.mid", output_dir, seed);
        write_midi(&grid, Path::new(&output_path)).expect("Failed to write MIDI");

        println!("{:>5} {:>10.1} {:>10.1} {:>+10.1} {:>7.1}%",
            seed, draft_score, result.final_score,
            result.final_score - draft_score, accept_pct);
    }

    println!();
    println!("Generated {} pieces in {}/", count, output_dir);
    println!("Rate them with: python python/rate_midi.py --dir {}", output_dir);
}
