// The score grid: the central representation for music generation.
//
// The score is a 2D grid where rows are voices (SATB) and columns are beats
// at eighth-note granularity. Each cell holds a pitch (MIDI number) or rest,
// plus flags for note attacks, syllable onsets, and syllable IDs.
//
// This representation makes it easy to:
// - Evaluate counterpoint rules at any beat (vertical slice)
// - Apply SA mutations (change a cell, rescore locally)
// - Convert to/from MIDI for playback
//
// The grid is the "source of truth" throughout generation. MIDI is derived
// from it, never the other way around.

use serde::{Deserialize, Serialize};

/// Voice index in SATB order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Voice {
    Soprano = 0,
    Alto = 1,
    Tenor = 2,
    Bass = 3,
}

impl Voice {
    pub const ALL: [Voice; 4] = [Voice::Soprano, Voice::Alto, Voice::Tenor, Voice::Bass];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Standard MIDI pitch ranges for each voice (approximate).
    /// These are soft constraints — the scoring function penalizes violations.
    pub fn range(self) -> (u8, u8) {
        match self {
            Voice::Soprano => (60, 79), // C4–G5
            Voice::Alto => (53, 72),    // F3–C5
            Voice::Tenor => (48, 67),   // C3–G4
            Voice::Bass => (40, 60),    // E2–C4
        }
    }
}

/// A single cell in the score grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// MIDI pitch number (0-127), or 0 if rest.
    pub pitch: u8,
    /// True if this cell is a rest (silence).
    pub is_rest: bool,
    /// True if this is a new note attack (vs. continuation of a held note).
    /// A rest cell always has attack = false.
    pub attack: bool,
    /// True if a new syllable of text begins at this cell.
    pub syllable_onset: bool,
    /// Which syllable from the text is being sung (index into phrase syllables).
    /// Only meaningful when not a rest.
    pub syllable_id: u16,
}

impl Cell {
    pub fn rest() -> Self {
        Cell {
            pitch: 0,
            is_rest: true,
            attack: false,
            syllable_onset: false,
            syllable_id: 0,
        }
    }

    pub fn note(pitch: u8, attack: bool) -> Self {
        Cell {
            pitch,
            is_rest: false,
            attack,
            syllable_onset: false,
            syllable_id: 0,
        }
    }
}

/// The complete score grid.
///
/// Indexed as `voices[voice_index][beat_index]`.
/// Beat granularity is eighth notes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grid {
    /// Number of eighth-note beats in the piece.
    pub num_beats: usize,
    /// Tempo in BPM (quarter notes per minute). Default 60.
    pub tempo_bpm: u16,
    /// The four voice rows. Each is a Vec<Cell> of length num_beats.
    pub voices: [Vec<Cell>; 4],
}

impl Grid {
    /// Create a new empty grid (all rests) with the given number of eighth-note beats.
    pub fn new(num_beats: usize) -> Self {
        let make_voice = || vec![Cell::rest(); num_beats];
        Grid {
            num_beats,
            tempo_bpm: 60,
            voices: [make_voice(), make_voice(), make_voice(), make_voice()],
        }
    }

    /// Get the cell at (voice, beat).
    pub fn cell(&self, voice: Voice, beat: usize) -> &Cell {
        &self.voices[voice.index()][beat]
    }

    /// Get a mutable reference to the cell at (voice, beat).
    pub fn cell_mut(&mut self, voice: Voice, beat: usize) -> &mut Cell {
        &mut self.voices[voice.index()][beat]
    }

    /// Set a note at (voice, beat). Marks it as an attack.
    pub fn set_note(&mut self, voice: Voice, beat: usize, pitch: u8) {
        let cell = self.cell_mut(voice, beat);
        cell.pitch = pitch;
        cell.is_rest = false;
        cell.attack = true;
    }

    /// Extend the note at the given beat to the next beat (continuation, not attack).
    /// If the current beat is a rest, does nothing.
    pub fn extend_note(&mut self, voice: Voice, beat: usize) {
        if beat == 0 {
            return;
        }
        let prev = self.voices[voice.index()][beat - 1];
        if prev.is_rest {
            return;
        }
        let cell = self.cell_mut(voice, beat);
        cell.pitch = prev.pitch;
        cell.is_rest = false;
        cell.attack = false;
        cell.syllable_id = prev.syllable_id;
    }

    /// Get the sounding pitch at a given beat for a voice.
    /// Returns None if the voice is resting.
    pub fn sounding_pitch(&self, voice: Voice, beat: usize) -> Option<u8> {
        let cell = self.cell(voice, beat);
        if cell.is_rest { None } else { Some(cell.pitch) }
    }

    /// Get all sounding pitches at a beat (vertical slice).
    pub fn vertical_slice(&self, beat: usize) -> [Option<u8>; 4] {
        Voice::ALL.map(|v| self.sounding_pitch(v, beat))
    }
}

impl Grid {
    /// Print a compact text summary of the grid for debugging.
    /// Shows each voice as a row with note names and durations.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        let bar_beats = 8; // 4/4 time in eighth notes

        for voice in Voice::ALL {
            out.push_str(&format!("{:>8}: ", format!("{:?}", voice)));
            let mut beat = 0;
            while beat < self.num_beats {
                // Bar line
                if beat > 0 && beat % bar_beats == 0 {
                    out.push('|');
                }

                let cell = self.cell(voice, beat);
                if cell.is_rest {
                    out.push('.');
                    beat += 1;
                } else if cell.attack {
                    let name = pitch_name(cell.pitch);
                    out.push_str(name);
                    // Count continuation beats
                    let mut dur = 1;
                    while beat + dur < self.num_beats {
                        let next = self.cell(voice, beat + dur);
                        if next.is_rest || next.attack {
                            break;
                        }
                        dur += 1;
                    }
                    // Show holds as dashes
                    for _ in 1..dur {
                        out.push('-');
                    }
                    beat += dur;
                } else {
                    // Continuation without attack (shouldn't happen at start of display)
                    out.push('-');
                    beat += 1;
                }
            }
            out.push('\n');
        }
        out
    }

    /// Count note statistics for the grid.
    pub fn stats(&self) -> GridStats {
        let mut total_attacks = 0;
        let mut total_sounding = 0;
        let mut rests = 0;

        for voice in Voice::ALL {
            for beat in 0..self.num_beats {
                let cell = self.cell(voice, beat);
                if cell.is_rest {
                    rests += 1;
                } else {
                    total_sounding += 1;
                    if cell.attack {
                        total_attacks += 1;
                    }
                }
            }
        }

        GridStats {
            total_beats: self.num_beats,
            total_attacks,
            total_sounding,
            rests,
        }
    }
}

/// Statistics about a grid's contents.
#[derive(Debug)]
pub struct GridStats {
    pub total_beats: usize,
    pub total_attacks: usize,
    pub total_sounding: usize,
    pub rests: usize,
}

/// Convert a MIDI pitch to a compact note name (e.g., "C4", "F#3").
pub fn pitch_name(pitch: u8) -> &'static str {
    const NAMES: &[&str] = &[
        "C0", "C#0", "D0", "Eb0", "E0", "F0", "F#0", "G0", "Ab0", "A0", "Bb0", "B0", "C1", "C#1",
        "D1", "Eb1", "E1", "F1", "F#1", "G1", "Ab1", "A1", "Bb1", "B1", "C2", "C#2", "D2", "Eb2",
        "E2", "F2", "F#2", "G2", "Ab2", "A2", "Bb2", "B2", "C3", "C#3", "D3", "Eb3", "E3", "F3",
        "F#3", "G3", "Ab3", "A3", "Bb3", "B3", "C4", "C#4", "D4", "Eb4", "E4", "F4", "F#4", "G4",
        "Ab4", "A4", "Bb4", "B4", "C5", "C#5", "D5", "Eb5", "E5", "F5", "F#5", "G5", "Ab5", "A5",
        "Bb5", "B5", "C6", "C#6", "D6", "Eb6", "E6", "F6", "F#6", "G6", "Ab6", "A6", "Bb6", "B6",
        "C7", "C#7", "D7", "Eb7", "E7", "F7", "F#7", "G7", "Ab7", "A7", "Bb7", "B7", "C8", "C#8",
        "D8", "Eb8", "E8", "F8", "F#8", "G8", "Ab8", "A8", "Bb8", "B8",
    ];
    if (pitch as usize) < NAMES.len() {
        NAMES[pitch as usize]
    } else {
        "??"
    }
}

/// Musical interval helpers.
pub mod interval {
    /// Compute the interval in semitones between two MIDI pitches.
    /// Positive means pitch_b is higher.
    pub fn semitones(pitch_a: u8, pitch_b: u8) -> i16 {
        pitch_b as i16 - pitch_a as i16
    }

    /// Interval class (0-11) — the interval mod 12, ignoring octave and direction.
    pub fn interval_class(pitch_a: u8, pitch_b: u8) -> u8 {
        let diff = (pitch_b as i16 - pitch_a as i16).unsigned_abs() as u8;
        let ic = diff % 12;
        if ic > 6 { 12 - ic } else { ic }
    }

    /// Check if an interval (in semitones, mod 12) is a perfect consonance.
    /// Perfect consonances: unison (0), perfect fifth (7), octave (0/12).
    pub fn is_perfect_consonance(semitones: i16) -> bool {
        let ic = (semitones.unsigned_abs() as u8) % 12;
        matches!(ic, 0 | 7)
    }

    /// Check if an interval is consonant (for strong-beat rules).
    /// Consonances: unison, m3, M3, P4 (sometimes), P5, m6, M6, octave.
    /// P4 is treated as consonant when it's not the lowest interval.
    pub fn is_consonant(semitones: i16) -> bool {
        let ic = (semitones.unsigned_abs() as u8) % 12;
        matches!(ic, 0 | 3 | 4 | 5 | 7 | 8 | 9)
    }

    /// Check if an interval is a dissonance (seconds, sevenths, tritone).
    pub fn is_dissonant(semitones: i16) -> bool {
        !is_consonant(semitones)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_creation() {
        let grid = Grid::new(32);
        assert_eq!(grid.num_beats, 32);
        for voice in Voice::ALL {
            assert_eq!(grid.voices[voice.index()].len(), 32);
            assert!(grid.cell(voice, 0).is_rest);
        }
    }

    #[test]
    fn test_set_and_extend_note() {
        let mut grid = Grid::new(8);
        grid.set_note(Voice::Soprano, 0, 60); // C4
        grid.extend_note(Voice::Soprano, 1);
        grid.extend_note(Voice::Soprano, 2);

        assert_eq!(grid.sounding_pitch(Voice::Soprano, 0), Some(60));
        assert!(grid.cell(Voice::Soprano, 0).attack);
        assert_eq!(grid.sounding_pitch(Voice::Soprano, 1), Some(60));
        assert!(!grid.cell(Voice::Soprano, 1).attack);
        assert_eq!(grid.sounding_pitch(Voice::Soprano, 2), Some(60));
    }

    #[test]
    fn test_interval_helpers() {
        assert!(interval::is_perfect_consonance(7)); // P5
        assert!(interval::is_perfect_consonance(0)); // unison
        assert!(interval::is_perfect_consonance(12)); // octave
        assert!(!interval::is_perfect_consonance(4)); // M3

        assert!(interval::is_consonant(7)); // P5
        assert!(interval::is_consonant(3)); // m3
        assert!(interval::is_consonant(4)); // M3
        assert!(!interval::is_consonant(1)); // m2
        assert!(!interval::is_consonant(6)); // tritone
    }

    #[test]
    fn test_vertical_slice() {
        let mut grid = Grid::new(4);
        grid.set_note(Voice::Soprano, 0, 72);
        grid.set_note(Voice::Alto, 0, 67);
        grid.set_note(Voice::Bass, 0, 48);

        let slice = grid.vertical_slice(0);
        assert_eq!(slice, [Some(72), Some(67), None, Some(48)]);
    }
}
