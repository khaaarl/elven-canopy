// Phase 1 waveform synthesizer: renders a Grid to mono PCM audio.
//
// Converts the SATB score grid into audible audio by generating simple
// triangle waves for each voice and mixing them together. This is the
// "mediocre MIDI quality" renderer described in the design doc §21 —
// harmonies are clearly audible but the sound is electronic.
//
// The output is a Vec<f32> of mono samples at 44100 Hz, normalized to
// [-1.0, 1.0]. Each voice gets equal volume (0.25 amplitude) so the
// mix stays within range. A short attack/release envelope (5ms each)
// prevents audible clicks at note boundaries.
//
// This module has no external dependencies beyond the Grid type from
// grid.rs. It is used by the game runtime (via gdext) to produce PCM
// buffers that Godot's AudioStreamGenerator can play.

use crate::grid::{Grid, Voice};

/// Standard audio sample rate.
pub const SAMPLE_RATE: u32 = 44_100;

/// Envelope fade duration in seconds (prevents clicks at note boundaries).
const ENVELOPE_FADE_SECS: f32 = 0.005;

/// Per-voice amplitude. Four voices at 0.25 each sum to at most 1.0.
const VOICE_AMPLITUDE: f32 = 0.25;

/// Convert a MIDI pitch number to frequency in Hz.
/// Uses equal temperament: f = 440 * 2^((pitch - 69) / 12).
fn midi_to_freq(pitch: u8) -> f32 {
    440.0 * 2.0_f32.powf((pitch as f32 - 69.0) / 12.0)
}

/// Generate a triangle wave sample at the given phase (0.0 to 1.0).
fn triangle_wave(phase: f32) -> f32 {
    // Triangle: rises from -1 to 1 over first half, falls from 1 to -1 over second half
    if phase < 0.5 {
        4.0 * phase - 1.0
    } else {
        3.0 - 4.0 * phase
    }
}

/// Render a Grid to mono PCM samples at 44100 Hz.
///
/// Returns a Vec<f32> with samples in [-1.0, 1.0]. The length depends on
/// the grid's beat count and tempo.
pub fn render_grid_to_pcm(grid: &Grid) -> Vec<f32> {
    if grid.num_beats == 0 {
        return Vec::new();
    }

    let eighth_note_secs = 60.0 / grid.tempo_bpm as f32 / 2.0;
    let total_secs = grid.num_beats as f32 * eighth_note_secs;
    let total_samples = (total_secs * SAMPLE_RATE as f32).ceil() as usize;
    let samples_per_beat = (eighth_note_secs * SAMPLE_RATE as f32) as usize;
    let envelope_samples = (ENVELOPE_FADE_SECS * SAMPLE_RATE as f32) as usize;

    let mut output = vec![0.0_f32; total_samples];

    for voice in Voice::ALL {
        render_voice(grid, voice, samples_per_beat, envelope_samples, &mut output);
    }

    output
}

/// Render a single voice into the output buffer (additive mixing).
fn render_voice(
    grid: &Grid,
    voice: Voice,
    samples_per_beat: usize,
    envelope_samples: usize,
    output: &mut [f32],
) {
    let total_samples = output.len();
    let cells = &grid.voices[voice.index()];

    // Walk through beats and identify note spans (attack to next attack/rest/end).
    let mut beat = 0;
    while beat < grid.num_beats {
        let cell = &cells[beat];

        if cell.is_rest {
            beat += 1;
            continue;
        }

        if !cell.attack {
            // Continuation without a preceding attack in this walk — skip.
            beat += 1;
            continue;
        }

        // Found an attack. Scan forward to find note duration.
        let pitch = cell.pitch;
        let freq = midi_to_freq(pitch);
        let start_beat = beat;
        beat += 1;
        while beat < grid.num_beats {
            let next = &cells[beat];
            if next.is_rest || next.attack {
                break;
            }
            beat += 1;
        }
        let dur_beats = beat - start_beat;

        // Render this note span.
        let sample_start = start_beat * samples_per_beat;
        let sample_end = (sample_start + dur_beats * samples_per_beat).min(total_samples);
        let note_samples = sample_end - sample_start;

        let period = SAMPLE_RATE as f32 / freq;
        for i in 0..note_samples {
            let sample_idx = sample_start + i;
            if sample_idx >= total_samples {
                break;
            }

            // Phase accumulation for the triangle wave.
            let phase = (i as f32 / period).fract();
            let raw = triangle_wave(phase);

            // Apply attack/release envelope (skip if note is too short).
            let env = if note_samples <= 2 * envelope_samples {
                1.0
            } else if i < envelope_samples {
                i as f32 / envelope_samples as f32
            } else if i >= note_samples - envelope_samples {
                (note_samples - 1 - i) as f32 / envelope_samples as f32
            } else {
                1.0
            };

            output[sample_idx] += raw * VOICE_AMPLITUDE * env;
        }
    }
}

/// Compute the duration of the rendered audio in seconds.
pub fn grid_duration_secs(grid: &Grid) -> f32 {
    if grid.num_beats == 0 {
        return 0.0;
    }
    let eighth_note_secs = 60.0 / grid.tempo_bpm as f32 / 2.0;
    grid.num_beats as f32 * eighth_note_secs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Grid;

    #[test]
    fn test_midi_to_freq() {
        // A4 = 440 Hz
        let freq = midi_to_freq(69);
        assert!((freq - 440.0).abs() < 0.01);

        // A3 = 220 Hz
        let freq = midi_to_freq(57);
        assert!((freq - 220.0).abs() < 0.01);

        // C4 = ~261.63 Hz
        let freq = midi_to_freq(60);
        assert!((freq - 261.63).abs() < 0.1);
    }

    #[test]
    fn test_triangle_wave_extremes() {
        // At phase 0.0: should be -1.0
        assert!((triangle_wave(0.0) - (-1.0)).abs() < 1e-6);
        // At phase 0.25: should be 0.0
        assert!((triangle_wave(0.25) - 0.0).abs() < 1e-6);
        // At phase 0.5: should be 1.0
        assert!((triangle_wave(0.5) - 1.0).abs() < 1e-6);
        // At phase 0.75: should be 0.0
        assert!((triangle_wave(0.75) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_empty_grid_produces_empty_pcm() {
        let grid = Grid::new(0);
        let pcm = render_grid_to_pcm(&grid);
        assert!(pcm.is_empty());
    }

    #[test]
    fn test_silent_grid_produces_silence() {
        let grid = Grid::new(8); // All rests
        let pcm = render_grid_to_pcm(&grid);
        assert!(!pcm.is_empty());
        for sample in &pcm {
            assert!(*sample == 0.0, "Expected silence but got {sample}");
        }
    }

    #[test]
    fn test_single_note_produces_audio() {
        let mut grid = Grid::new(8);
        grid.tempo_bpm = 120; // eighth note = 0.25s
        grid.set_note(Voice::Soprano, 0, 69); // A4
        for beat in 1..8 {
            grid.extend_note(Voice::Soprano, beat);
        }

        let pcm = render_grid_to_pcm(&grid);
        assert!(!pcm.is_empty());

        // Should have non-zero samples (the note is sounding)
        let max_abs = pcm.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(max_abs > 0.1, "Expected audible output, max was {max_abs}");
        assert!(
            max_abs <= VOICE_AMPLITUDE + 0.001,
            "Single voice should not exceed VOICE_AMPLITUDE, got {max_abs}"
        );
    }

    #[test]
    fn test_four_voices_stay_in_range() {
        let mut grid = Grid::new(16);
        grid.tempo_bpm = 120;

        // Set up a full SATB chord (C major)
        let pitches = [72_u8, 67, 64, 60]; // C5, G4, E4, C4
        for (voice, &pitch) in Voice::ALL.iter().zip(pitches.iter()) {
            grid.set_note(*voice, 0, pitch);
            for beat in 1..16 {
                grid.extend_note(*voice, beat);
            }
        }

        let pcm = render_grid_to_pcm(&grid);
        let max_abs = pcm.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);

        // Four voices at 0.25 each can sum to at most 1.0
        assert!(
            max_abs <= 1.05,
            "Mix should stay near [-1, 1], got max {max_abs}"
        );
    }

    #[test]
    fn test_rest_gaps_produce_silence() {
        let mut grid = Grid::new(8);
        grid.tempo_bpm = 120;

        // Note on beats 0-3, rest on beats 4-7
        grid.set_note(Voice::Soprano, 0, 69);
        for beat in 1..4 {
            grid.extend_note(Voice::Soprano, beat);
        }
        // Beats 4-7 are rests (default)

        let pcm = render_grid_to_pcm(&grid);
        let samples_per_beat = (SAMPLE_RATE as f32 * 0.25) as usize; // 0.25s per eighth at 120bpm

        // Check that the second half is near-silent (allowing for envelope tail)
        let silence_start = 5 * samples_per_beat; // well into the rest region
        if silence_start < pcm.len() {
            let max_in_rest = pcm[silence_start..]
                .iter()
                .map(|s| s.abs())
                .fold(0.0_f32, f32::max);
            assert!(
                max_in_rest < 0.001,
                "Rest region should be silent, got {max_in_rest}"
            );
        }
    }

    #[test]
    fn test_grid_duration_secs() {
        let mut grid = Grid::new(16);
        grid.tempo_bpm = 120;
        // 16 eighth notes at 120 BPM = 16 * 0.25s = 4.0s
        let dur = grid_duration_secs(&grid);
        assert!((dur - 4.0).abs() < 0.01, "Expected 4.0s, got {dur}");

        grid.tempo_bpm = 60;
        // 16 eighth notes at 60 BPM = 16 * 0.5s = 8.0s
        let dur = grid_duration_secs(&grid);
        assert!((dur - 8.0).abs() < 0.01, "Expected 8.0s, got {dur}");
    }

    #[test]
    fn test_envelope_prevents_clicks() {
        let mut grid = Grid::new(4);
        grid.tempo_bpm = 120;
        grid.set_note(Voice::Soprano, 0, 69);
        for beat in 1..4 {
            grid.extend_note(Voice::Soprano, beat);
        }

        let pcm = render_grid_to_pcm(&grid);

        // First sample should be near zero (envelope attack)
        assert!(
            pcm[0].abs() < 0.01,
            "First sample should be near zero due to envelope, got {}",
            pcm[0]
        );

        // Last non-zero sample region should taper toward zero
        let last = pcm.len() - 1;
        assert!(
            pcm[last].abs() < 0.01,
            "Last sample should be near zero due to envelope, got {}",
            pcm[last]
        );
    }
}
