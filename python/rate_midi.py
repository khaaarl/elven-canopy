"""
MIDI pairwise rating interface for preference model training.

Plays two MIDI files and asks the user which they prefer. Collects
pairwise preferences for training a learned aesthetic model (§12 of
the design doc).

Usage:
    python rate_midi.py [--midi-dir DIR] [--output FILE] [--pairs N]

The script:
1. Scans a directory for .mid files
2. Selects random pairs
3. Plays each using the system MIDI player
4. Records the user's preference (1, 2, or 0 for tie)
5. Saves results to a JSON file for later model training

Requires a MIDI player on the system (timidity, fluidsynth, or similar).
"""

import argparse
import json
import os
import random
import subprocess
import sys
from datetime import datetime
from pathlib import Path


def find_midi_player():
    """Find an available MIDI player on the system."""
    players = [
        ("timidity", ["-Os"]),           # TiMidity++ with ALSA output
        ("fluidsynth", ["-a", "alsa"]),  # FluidSynth
        ("aplaymidi", []),               # ALSA raw MIDI
    ]

    for cmd, args in players:
        try:
            result = subprocess.run(
                ["which", cmd], capture_output=True, text=True
            )
            if result.returncode == 0:
                return cmd, args
        except FileNotFoundError:
            continue

    return None, None


def play_midi(filepath, player_cmd, player_args, timeout=60):
    """Play a MIDI file using the system MIDI player."""
    try:
        cmd = [player_cmd] + player_args + [str(filepath)]
        proc = subprocess.Popen(
            cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )
        return proc
    except Exception as e:
        print(f"  Error playing {filepath}: {e}")
        return None


def stop_playback(proc):
    """Stop a playing MIDI process."""
    if proc and proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            proc.kill()


def print_midi_info(filepath):
    """Print basic info about a MIDI file."""
    try:
        import mido
        mid = mido.MidiFile(str(filepath))
        duration = mid.length
        num_tracks = len(mid.tracks)
        note_count = sum(
            1 for track in mid.tracks
            for msg in track
            if msg.type == "note_on" and msg.velocity > 0
        )
        print(f"  Duration: {duration:.1f}s, Tracks: {num_tracks}, Notes: {note_count}")
    except Exception:
        print(f"  (Could not read MIDI info)")


def run_rating_session(midi_dir, output_file, num_pairs):
    """Run an interactive pairwise rating session."""
    # Find MIDI files
    midi_files = sorted(Path(midi_dir).glob("*.mid"))
    if len(midi_files) < 2:
        print(f"Error: Need at least 2 MIDI files in {midi_dir}, found {len(midi_files)}.")
        print("Generate some first with: cargo run -p elven_canopy_music -- output.mid")
        sys.exit(1)

    print(f"Found {len(midi_files)} MIDI files in {midi_dir}")

    # Find player
    player_cmd, player_args = find_midi_player()
    if not player_cmd:
        print("Warning: No MIDI player found (tried timidity, fluidsynth, aplaymidi).")
        print("Install one with: sudo apt install timidity")
        print("Continuing in metadata-only mode (no playback).\n")

    # Load existing ratings if any
    ratings = []
    if Path(output_file).exists():
        with open(output_file) as f:
            data = json.load(f)
            ratings = data.get("ratings", [])
        print(f"Loaded {len(ratings)} existing ratings from {output_file}")

    print(f"\nStarting pairwise rating session ({num_pairs} pairs)")
    print("Controls:")
    print("  1 = prefer file A")
    print("  2 = prefer file B")
    print("  0 = tie / no preference")
    print("  r = replay current pair")
    print("  q = quit and save")
    print()

    completed = 0
    for pair_idx in range(num_pairs):
        # Select a random pair
        file_a, file_b = random.sample(midi_files, 2)

        print(f"--- Pair {pair_idx + 1}/{num_pairs} ---")
        print(f"  A: {file_a.name}")
        print_midi_info(file_a)
        print(f"  B: {file_b.name}")
        print_midi_info(file_b)

        while True:
            choice = input("\nPlay and rate [1/2/0/r/q]: ").strip().lower()

            if choice == "q":
                print("Saving and quitting...")
                save_ratings(output_file, ratings)
                print(f"Saved {len(ratings)} total ratings to {output_file}")
                return

            if choice == "r" or choice == "":
                # Play A
                if player_cmd:
                    print("  Playing A...")
                    proc = play_midi(file_a, player_cmd, player_args)
                    input("  Press Enter to stop and play B...")
                    stop_playback(proc)

                    print("  Playing B...")
                    proc = play_midi(file_b, player_cmd, player_args)
                    input("  Press Enter to stop...")
                    stop_playback(proc)
                else:
                    print("  (No MIDI player available - rate based on filenames)")
                continue

            if choice in ("1", "2", "0"):
                rating = {
                    "file_a": str(file_a.name),
                    "file_b": str(file_b.name),
                    "preference": int(choice),  # 1=A, 2=B, 0=tie
                    "timestamp": datetime.now().isoformat(),
                }
                ratings.append(rating)
                completed += 1

                pref_str = {1: "A preferred", 2: "B preferred", 0: "Tie"}[int(choice)]
                print(f"  Recorded: {pref_str}")
                break
            else:
                print("  Invalid input. Enter 1, 2, 0, r, or q.")

    save_ratings(output_file, ratings)
    print(f"\nSession complete! {completed} new ratings, {len(ratings)} total.")
    print(f"Saved to {output_file}")


def save_ratings(output_file, ratings):
    """Save ratings to a JSON file."""
    Path(output_file).parent.mkdir(parents=True, exist_ok=True)
    data = {
        "ratings": ratings,
        "total": len(ratings),
        "last_updated": datetime.now().isoformat(),
    }
    with open(output_file, "w") as f:
        json.dump(data, f, indent=2)


def generate_batch(output_dir, count=10, sections=3):
    """Generate a batch of MIDI files for rating."""
    print(f"Generating {count} MIDI files for rating...")
    Path(output_dir).mkdir(parents=True, exist_ok=True)

    for i in range(count):
        seed = random.randint(0, 2**32)
        output_path = Path(output_dir) / f"piece_{i:03d}_s{seed}.mid"
        cmd = [
            "cargo", "run", "-p", "elven_canopy_music", "--",
            str(output_path),
            "--seed", str(seed),
            "--sections", str(sections),
        ]
        print(f"  [{i+1}/{count}] Generating with seed {seed}...")
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            print(f"    Error: {result.stderr[:200]}")

    midi_count = len(list(Path(output_dir).glob("*.mid")))
    print(f"Done! {midi_count} MIDI files in {output_dir}")


def main():
    parser = argparse.ArgumentParser(
        description="MIDI pairwise rating interface for preference model training"
    )
    parser.add_argument("--midi-dir", type=str, default=".tmp/rating_batch",
                        help="Directory containing MIDI files to rate")
    parser.add_argument("--output", type=str, default="data/ratings.json",
                        help="Output file for ratings")
    parser.add_argument("--pairs", type=int, default=20,
                        help="Number of pairs to rate per session")
    parser.add_argument("--generate", type=int, default=0,
                        help="Generate N MIDI files before rating")
    parser.add_argument("--sections", type=int, default=3,
                        help="Sections per generated piece")
    args = parser.parse_args()

    if args.generate > 0:
        generate_batch(args.midi_dir, count=args.generate, sections=args.sections)
        print()

    run_rating_session(args.midi_dir, args.output, args.pairs)


if __name__ == "__main__":
    main()
