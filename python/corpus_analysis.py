"""
Corpus analysis pipeline for Palestrina-style music generation.

Parses Palestrina masses and Renaissance polyphony from music21's built-in
corpus, extracts interval-based Markov models and a motif library, and
exports them as JSON for the Rust generator to load.

This is the offline training phase (§5 of the design doc). The output feeds
into elven_canopy_music's MarkovModels and MotifLibrary structs.

Usage:
    python corpus_analysis.py [--output-dir DIR] [--max-pieces N]

Output files:
    markov_models.json  — melodic + harmonic Markov transition tables
    motif_library.json  — ranked interval n-gram motifs
"""

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

import music21
import numpy as np


def load_palestrina_corpus(max_pieces=None):
    """Load Palestrina pieces from music21's built-in corpus.

    music21 includes a substantial collection of Palestrina masses.
    Returns a list of parsed Score objects.
    """
    print("Loading Palestrina corpus from music21...")

    # Search for Palestrina works in the corpus
    corpus_paths = music21.corpus.getComposer("palestrina")

    if not corpus_paths:
        print("  No Palestrina pieces found in corpus. Trying alternative search...")
        # Try broader search
        corpus_paths = [p for p in music21.corpus.getCorePaths()
                        if "palestrina" in str(p).lower()]

    if not corpus_paths:
        print("  WARNING: No Palestrina pieces found. Using Bach chorales as fallback.")
        corpus_paths = music21.corpus.getComposer("bach")
        if max_pieces:
            corpus_paths = corpus_paths[:max_pieces]

    if max_pieces:
        corpus_paths = corpus_paths[:max_pieces]

    print(f"  Found {len(corpus_paths)} pieces to analyze.")

    scores = []
    for i, path in enumerate(corpus_paths):
        try:
            score = music21.corpus.parse(path)
            scores.append(score)
            if (i + 1) % 10 == 0 or i + 1 == len(corpus_paths):
                print(f"  Parsed {i + 1}/{len(corpus_paths)}...")
        except Exception as e:
            print(f"  Skipping {path}: {e}")

    print(f"  Successfully loaded {len(scores)} pieces.")
    return scores


def extract_voice_intervals(score):
    """Extract interval sequences from each voice part in a score.

    Returns a list of (intervals, metric_positions) tuples, one per voice.
    Intervals are in semitones. Metric positions are beat strengths (0-3).
    """
    voices = []

    parts = score.parts
    for part in parts:
        intervals = []
        metric_positions = []
        prev_midi = None

        for note in part.recurse().notes:
            if note.isChord:
                # Use the highest note of chords
                midi_num = max(p.midi for p in note.pitches)
            else:
                midi_num = note.pitch.midi

            if prev_midi is not None:
                interval = midi_num - prev_midi
                intervals.append(interval)

                # Metric position: beat strength
                try:
                    beat_strength = note.beatStrength
                    if beat_strength >= 1.0:
                        metric_pos = 0  # downbeat
                    elif beat_strength >= 0.5:
                        metric_pos = 1  # strong beat
                    elif beat_strength >= 0.25:
                        metric_pos = 2  # weak beat
                    else:
                        metric_pos = 3  # very weak
                except Exception:
                    metric_pos = 2

                metric_positions.append(metric_pos)

            prev_midi = midi_num

        if intervals:
            voices.append((intervals, metric_positions))

    return voices


def extract_voice_pair_intervals(score):
    """Extract harmonic interval sequences between voice pairs.

    For each pair of parts, extracts the interval between simultaneous notes
    at each beat position.
    """
    parts = list(score.parts)
    pairs = []

    for i in range(len(parts)):
        for j in range(i + 1, len(parts)):
            pair_intervals = extract_pair_intervals(parts[i], parts[j])
            if pair_intervals:
                pairs.append(pair_intervals)

    return pairs


def extract_pair_intervals(part_a, part_b):
    """Extract the sequence of harmonic intervals between two parts."""
    intervals = []

    # Flatten both parts to note sequences with offsets
    notes_a = {}
    for note in part_a.recurse().notes:
        offset = float(note.offset + note.activeSite.offset)
        midi = note.pitch.midi if not note.isChord else max(p.midi for p in note.pitches)
        notes_a[round(offset * 2) / 2] = midi  # quantize to eighth notes

    notes_b = {}
    for note in part_b.recurse().notes:
        offset = float(note.offset + note.activeSite.offset)
        midi = note.pitch.midi if not note.isChord else max(p.midi for p in note.pitches)
        notes_b[round(offset * 2) / 2] = midi

    # Find common time positions
    common_times = sorted(set(notes_a.keys()) & set(notes_b.keys()))
    for t in common_times:
        interval = notes_a[t] - notes_b[t]
        intervals.append(interval)

    return intervals


def build_melodic_model(all_voice_intervals, max_order=3):
    """Build an interval-based Markov model with Katz backoff.

    Takes all voice interval sequences from the corpus and builds
    transition tables at orders 0-3.
    """
    print("Building melodic Markov model...")

    # Count transitions at each order
    counts = {order: defaultdict(lambda: defaultdict(float))
              for order in range(max_order + 1)}

    for intervals, _metric_positions in all_voice_intervals:
        # Clamp intervals to [-24, 24] range
        intervals = [max(-24, min(24, iv)) for iv in intervals]

        for i in range(len(intervals)):
            # Order 0: just count interval frequencies
            counts[0][""][intervals[i]] += 1.0

            # Higher orders: context -> next interval
            for order in range(1, max_order + 1):
                if i >= order:
                    context = tuple(intervals[i - order:i])
                    context_key = ",".join(str(x) for x in context)
                    counts[order][context_key][intervals[i]] += 1.0

    # Convert to plain dicts for JSON serialization
    model = {
        "order0": dict(counts[0][""]),
        "order1": {k: dict(v) for k, v in counts[1].items() if len(v) >= 2},
        "order2": {k: dict(v) for k, v in counts[2].items() if len(v) >= 2},
        "order3": {k: dict(v) for k, v in counts[3].items() if len(v) >= 2},
    }

    # Stats
    total_transitions = sum(counts[0][""].values())
    print(f"  Total melodic transitions: {int(total_transitions)}")
    print(f"  Order-1 contexts: {len(model['order1'])}")
    print(f"  Order-2 contexts: {len(model['order2'])}")
    print(f"  Order-3 contexts: {len(model['order3'])}")

    # Top intervals
    sorted_intervals = sorted(model["order0"].items(), key=lambda x: -x[1])
    print("  Top 10 intervals:")
    for iv, count in sorted_intervals[:10]:
        pct = count / total_transitions * 100
        print(f"    {iv:+3d} semitones: {pct:5.1f}%")

    return model


def build_harmonic_model(all_pair_intervals):
    """Build a harmonic Markov model for voice pair intervals.

    First-order: P(interval_t | interval_{t-1}).
    """
    print("Building harmonic Markov model...")

    transitions = defaultdict(lambda: defaultdict(float))
    unigram = defaultdict(float)

    for pair_intervals in all_pair_intervals:
        # Clamp to [-36, 36]
        pair_intervals = [max(-36, min(36, iv)) for iv in pair_intervals]

        for iv in pair_intervals:
            unigram[iv] += 1.0

        for i in range(1, len(pair_intervals)):
            prev_key = str(pair_intervals[i - 1])
            transitions[prev_key][pair_intervals[i]] += 1.0

    model = {
        "transitions": {k: dict(v) for k, v in transitions.items() if len(v) >= 2},
        "unigram": dict(unigram),
    }

    total = sum(unigram.values())
    print(f"  Total harmonic observations: {int(total)}")
    print(f"  Transition contexts: {len(model['transitions'])}")

    # Top intervals
    sorted_intervals = sorted(unigram.items(), key=lambda x: -x[1])
    print("  Top 10 harmonic intervals:")
    for iv, count in sorted_intervals[:10]:
        pct = count / total * 100
        ic = abs(iv) % 12
        name = {0: "P1/P8", 1: "m2", 2: "M2", 3: "m3", 4: "M3",
                5: "P4", 6: "TT", 7: "P5", 8: "m6", 9: "M6",
                10: "m7", 11: "M7"}.get(ic, "?")
        print(f"    {iv:+3d} ({name}): {pct:5.1f}%")

    return model


def extract_motifs(all_voice_intervals, min_length=4, max_length=10,
                   min_frequency=3, min_pieces=2):
    """Extract interval n-gram motifs that appear across multiple pieces.

    Groups intervals by piece, then finds n-grams that appear in
    multiple pieces (indicating stylistic vocabulary rather than
    piece-specific themes).
    """
    print("Extracting motifs...")

    # Group intervals by piece index
    # all_voice_intervals is flat, so we need to track piece boundaries
    # For simplicity, just find frequent n-grams across all voices
    ngram_counts = defaultdict(int)
    ngram_piece_sets = defaultdict(set)

    piece_idx = 0
    for voice_idx, (intervals, _) in enumerate(all_voice_intervals):
        # Clamp intervals
        intervals = [max(-24, min(24, iv)) for iv in intervals]

        for length in range(min_length, max_length + 1):
            for i in range(len(intervals) - length + 1):
                ngram = tuple(intervals[i:i + length])
                key = ",".join(str(x) for x in ngram)
                ngram_counts[key] += 1
                ngram_piece_sets[key].add(voice_idx // 4)  # approximate piece grouping

    # Filter and rank
    motifs = []
    for key, count in ngram_counts.items():
        if count < min_frequency:
            continue
        if len(ngram_piece_sets[key]) < min_pieces:
            continue

        intervals = [int(x) for x in key.split(",")]
        motifs.append({
            "intervals": intervals,
            "frequency": count,
            "typical_entry_offset": 8,  # default 1 bar
            "typical_transposition": 7,  # default at the 5th
        })

    # Sort by frequency
    motifs.sort(key=lambda m: -m["frequency"])

    # Keep top 50
    motifs = motifs[:50]

    print(f"  Found {len(motifs)} motifs meeting criteria.")
    if motifs:
        print("  Top 5 motifs:")
        for m in motifs[:5]:
            intervals_str = " ".join(f"{iv:+d}" for iv in m["intervals"])
            print(f"    [{intervals_str}] (freq: {m['frequency']})")

    return motifs


def main():
    parser = argparse.ArgumentParser(description="Corpus analysis for Palestrina music generation")
    parser.add_argument("--output-dir", type=str, default="data",
                        help="Output directory for JSON files")
    parser.add_argument("--max-pieces", type=int, default=None,
                        help="Maximum number of pieces to analyze (None = all)")
    args = parser.parse_args()

    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    # Load corpus
    scores = load_palestrina_corpus(max_pieces=args.max_pieces)

    if not scores:
        print("ERROR: No scores loaded. Cannot proceed.")
        sys.exit(1)

    # Extract intervals from all voices
    print("\nExtracting intervals from all voices...")
    all_melodic = []
    all_harmonic = []

    for i, score in enumerate(scores):
        voice_intervals = extract_voice_intervals(score)
        all_melodic.extend(voice_intervals)

        pair_intervals = extract_voice_pair_intervals(score)
        all_harmonic.extend(pair_intervals)

        if (i + 1) % 10 == 0:
            print(f"  Processed {i + 1}/{len(scores)} scores...")

    print(f"  Total voice parts extracted: {len(all_melodic)}")
    print(f"  Total voice pairs extracted: {len(all_harmonic)}")

    # Build models
    print()
    melodic_model = build_melodic_model(all_melodic)

    print()
    harmonic_model = build_harmonic_model(all_harmonic)

    # Combine into MarkovModels format (matching Rust struct)
    markov_models = {
        "melodic": melodic_model,
        "harmonic": harmonic_model,
    }

    # Export Markov models
    markov_path = output_dir / "markov_models.json"
    with open(markov_path, "w") as f:
        json.dump(markov_models, f, indent=2)
    print(f"\nWrote Markov models to {markov_path}")

    # Extract and export motifs
    print()
    motifs = extract_motifs(all_melodic)
    motif_library = {"motifs": motifs}

    motif_path = output_dir / "motif_library.json"
    with open(motif_path, "w") as f:
        json.dump(motif_library, f, indent=2)
    print(f"Wrote motif library to {motif_path}")

    print("\nDone! Files ready for the Rust generator to load.")


if __name__ == "__main__":
    main()
