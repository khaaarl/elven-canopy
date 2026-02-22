# Elven Canopy Music Generator

A Palestrina-style four-voice polyphonic music generator with procedurally generated Vaelith elvish lyrics. Produces MIDI files suitable for playback, evaluation, and eventually in-game use.

## Quick Start

```bash
# Generate a single piece (random seed)
cargo run -p elven_canopy_music

# Generate with a specific seed (reproducible)
cargo run -p elven_canopy_music -- output.mid --seed 42

# Generate with more control
cargo run -p elven_canopy_music -- my_piece.mid \
  --seed 42 --sections 4 --mode phrygian \
  --brightness 0.8 --tempo 66 --sa-iterations 15000 -v
```

Output is a Standard MIDI File (Format 1, 4 voice tracks + tempo track). Play with any MIDI player:

```bash
timidity output.mid          # Linux
fluidsynth output.mid        # Cross-platform
open output.mid              # macOS (opens in GarageBand/QuickTime)
```

## CLI Reference

### Single piece mode (default)

```
cargo run -p elven_canopy_music -- [OUTPUT_PATH] [OPTIONS]
```

| Flag | Default | Description |
|------|---------|-------------|
| `OUTPUT_PATH` | `output.mid` | Path for the generated MIDI file |
| `--seed N` | random | RNG seed for reproducible output |
| `--sections N` | 3 | Number of imitative sections (more = longer piece) |
| `--mode MODE` | dorian | Church mode (see below) |
| `--tempo N` | 72 | Tempo in BPM |
| `--brightness F` | 0.5 | Vaelith vowel brightness, 0.0-1.0 |
| `--sa-iterations N` | 10000 | SA refinement effort (higher = better but slower) |
| `-v` / `--verbose` | off | Show grid summary, score breakdown, contour stats |

### Batch mode

Generate multiple pieces with sequential seeds for comparison or preference rating:

```bash
cargo run -p elven_canopy_music -- --batch 20 --seed 1 --output-dir .tmp/batch
```

| Flag | Default | Description |
|------|---------|-------------|
| `--batch N` | 10 | Number of pieces to generate |
| `--seed N` | 1 | Starting seed (increments per piece) |
| `--output-dir DIR` | `.tmp/batch` | Output directory |

Prints a score comparison table. Use with the rating tool:

```bash
python python/rate_midi.py --dir .tmp/batch
```

### Mode scan

Generate the same piece in all 6 church modes for side-by-side comparison:

```bash
cargo run -p elven_canopy_music -- --mode-scan --seed 42
```

Produces 6 MIDI files in `.tmp/mode_scan/` (one per mode), with a table comparing scores.

## Parameters Guide

### Modes

Six Renaissance church modes, each with a distinct character:

| Mode | Final | Character |
|------|-------|-----------|
| `dorian` | D | Minor-ish, versatile, Palestrina's most common |
| `phrygian` | E | Dark, exotic half-step at bottom |
| `lydian` | F | Bright, raised 4th creates tension |
| `mixolydian` | G | Major-ish, lowered 7th gives gentleness |
| `aeolian` | A | Natural minor, melancholic |
| `ionian` | C | Major scale, bright and stable |

### Brightness

Controls the vowel palette of the generated Vaelith lyrics:

- **0.0** (dark): Favors back vowels (o, u) -- "moru", "kose", "fole"
- **0.5** (neutral): Balanced mix
- **1.0** (bright): Favors front vowels (e, i) -- "airen", "lethe", "wethe"

This affects the *timbral color* of the text, not the pitches. When vocal synthesis is added, this will directly affect the perceived warmth/brightness of the choral sound.

### SA iterations

Controls how long the simulated annealing refinement runs. The actual iteration count is typically 8-17x the target because of adaptive cooling and reheating.

| Target | Actual iters | Quality | Time (debug) |
|--------|-------------|---------|--------------|
| 3000 | ~25K | Quick draft | ~1s |
| 10000 | ~85K | Good | ~3s |
| 30000 | ~250K | High quality | ~10s |
| 100000 | ~800K | Diminishing returns | ~30s |

For batch generation/evaluation, 10000 is a good balance. For a "final" piece, 30000+.

### Sections

Each section introduces a new melodic motif treated imitatively (voices enter in sequence with the same melody). More sections = longer piece with more thematic material.

| Sections | Approx. duration | Character |
|----------|------------------|-----------|
| 2 | 40-50s | Short, focused |
| 3 | 60-75s | Standard length |
| 4 | 80-100s | Extended, with call-and-response |
| 5+ | 100s+ | Long form |

## Verbose Output

With `-v`, the generator shows:

**Grid summary** -- ASCII representation of all four voices with pitch names, showing the rhythmic structure and note placement.

**Score breakdown** -- Per-layer contributions to the total quality score:

```
Score breakdown:
  Hard rules:        -330.0    <- parallel 5ths/8ves, dissonance, voice crossing
  Melodic:            335.5    <- stepwise motion, leap recovery, direction
  Harmonic:          1137.5    <- consonance, voice spacing, interval variety
  Global:               2.0    <- cadences, opening/closing, rhythmic independence
  Modal:               65.0    <- mode compliance, degree fitness
  Texture:              2.0    <- voice density variety
  Tension curve:       17.5    <- arc shape toward climax
  Interval dist:     -105.7    <- deviation from Palestrina interval norms
  Entropy:             24.0    <- melodic information content balance
  Tonal contour:      -73.0    <- Vaelith tone/pitch direction match
  ─────────────────────────
  Total:             1064.8
```

**Tonal contour stats** -- How well the music respects the Vaelith tonal constraints (syllable tone shapes mapped to pitch direction).

## Architecture Overview

The generation pipeline has 5 stages:

```
[1] Load models     Markov tables + motif library (from data/ or built-in)
        |
[2] Plan structure  Choose motifs, voice entry timing, dai/thol markers
        |
[3] Generate draft  Place motif entries, fill gaps with Markov sampling
        |                    + generate Vaelith text, map syllables to grid
        |
[4] SA refinement   ~85K iterations of pitch/duration/text-swap mutations
        |                    adaptive cooling, periodic reheating
        |
[5] Write MIDI      4 voice tracks + tempo + embedded lyrics
```

### Source files (elven_canopy_music/src/)

| File | Lines | Role |
|------|-------|------|
| `grid.rs` | 355 | Core SATB score grid (eighth-note resolution) |
| `mode.rs` | 196 | Church mode scales, pitch snapping, fitness |
| `markov.rs` | 342 | Melodic/harmonic Markov models, motif library |
| `structure.rs` | 510 | Form planning, imitation points, dai/thol |
| `draft.rs` | 548 | Initial note placement with parallel-motion avoidance |
| `scoring.rs` | 1512 | 10-layer quality scoring (see breakdown above) |
| `sa.rs` | 779 | Simulated annealing with adaptive cooling |
| `vaelith.rs` | 615 | Vaelith conlang grammar engine |
| `text_mapping.rs` | 487 | Syllable-to-grid mapping, tone constraints |
| `midi.rs` | 222 | MIDI output with embedded lyrics |
| `main.rs` | 444 | CLI: single, batch, and mode-scan modes |
| `lib.rs` | 33 | Module declarations |

### Trained data (data/)

| File | Source |
|------|--------|
| `markov_models.json` | Trained from Palestrina corpus via `python/corpus_analysis.py` |
| `motif_library.json` | Extracted melodic motifs from corpus analysis |

These are loaded automatically if present. Without them, built-in default models are used (functional but less stylistically accurate).

## Python Tools

### Corpus analysis (`python/corpus_analysis.py`)

Trains Markov models from MIDI files of Palestrina's music. Run this if you want to retrain from a different corpus or add more training data.

```bash
cd python
pip install -r requirements.txt   # music21, numpy
python corpus_analysis.py          # processes corpus, writes to data/
```

### MIDI rating (`python/rate_midi.py`)

Pairwise comparison interface for rating generated pieces. This is the first step toward training a learned preference model (Phase 6).

```bash
python python/rate_midi.py --dir .tmp/batch
```

Plays pairs of MIDI files and asks you to choose which sounds better. Results are saved for future model training.

## Running Tests

```bash
cargo test -p elven_canopy_music
```

37 unit tests covering all modules: grid operations, Markov sampling, mode logic, structure planning, draft generation, scoring layers, SA convergence, text mapping, MIDI output, and Vaelith grammar.

## Next Steps

The design doc (`docs/drafts/palestrina_generator_v2.md`) lays out a full 8-phase plan. Phases 1-5 and most of Phase 8 are complete. Here's what remains:

### Ready to do now

**Weight tuning.** The scoring breakdown (verbose mode) shows exactly where points are lost. The biggest levers:

- `hard_rules` penalties are still significant (~-300). These are parallel 5ths/8ves and dissonance violations that SA hasn't fully resolved. You could increase `sa-iterations` for better convergence, or adjust the weight magnitudes in `ScoringWeights::default()` to prioritize what matters to your ear.
- `interval_dist` penalty (~-100) means the generated melodies still leap more than Palestrina would. Increasing this weight would push toward more stepwise motion but might reduce melodic interest.
- Experiment with these by editing `scoring.rs` and running batch comparisons.

**Vocabulary expansion.** The Vaelith lexicon in `vaelith.rs` has ~50 words across nouns, verbs, adjectives, and suffixes. Adding more words (especially for game-specific concepts) is straightforward -- just add entries to the `NOUNS`, `VERBS`, etc. arrays.

**Preference model data collection.** Use batch mode to generate 50+ pieces, then rate them with `python/rate_midi.py`. After ~150-300 pairwise comparisons, you'll have enough data to train a simple preference model (logistic regression on the engineered features from the scoring breakdown).

### Phase 6: Learned Preference Model

The scoring system produces "correct" counterpoint. A learned model adds subjective "beauty" -- what you personally find pleasing. The infrastructure is already in place:

1. Generate batches: `--batch 50 --seed 1`
2. Rate with pairwise comparisons: `python/rate_midi.py --dir .tmp/batch`
3. Train a model on the ratings (logistic regression with Bradley-Terry loss on the score breakdown features)
4. Export weights as JSON, load in Rust as an additional scoring layer

The design doc recommends ~3-5 hours of focused listening across multiple sessions, with active learning to select the most informative pairs.

### Phase 7: Vocal Synthesis

Replace MIDI "choir aahs" with actual Vaelith vocal synthesis. This requires:

1. **Record ~132 syllable snippets** (~55 seconds of audio total, ~20 minutes of recording). The inventory covers 15 consonants x 5 vowels, plus diphthongs and closed-syllable codas.
2. **Build a concatenative synthesizer** that sequences snippets according to the generated text, applying pitch envelopes for tonal realization.
3. **Mix four voice streams** with appropriate stereo positioning.

The grid already tracks syllable onsets and MIDI lyrics, so the data needed by the synthesizer is available. The design doc has the full recording inventory in Section 14.2.

### Game integration

The generator is a standalone Rust crate with no Godot dependencies. To integrate with the game:

- Call the generation pipeline from `elven_canopy_gdext` (the GDExtension bridge)
- Use game state to choose parameters: mode based on biome/mood, brightness based on time of day, sections based on scene importance
- Pre-generate a library of pieces at different parameter combinations, or generate on a background thread during gameplay
- The deterministic seeding means you can regenerate the same piece from just a seed number + parameters (no need to store MIDI files)

### Open questions from the design doc

- **Rhythm modeling**: The grid uses eighth-note resolution with attack/continuation flags. More varied rhythms (dotted rhythms, syncopation) could be added as additional SA mutation types.
- **More than 4 voices**: The architecture supports it, but SA convergence gets harder. Start with 4.
- **Real-time interaction**: A user could fix some cells and have the system fill the rest -- interactive composition assistance.
- **Mythology-driven text**: The grammar engine could draw from a structured mythology database to generate text that tells specific stories.
