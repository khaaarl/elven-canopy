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
        if cell.is_rest {
            None
        } else {
            Some(cell.pitch)
        }
    }

    /// Get all sounding pitches at a beat (vertical slice).
    pub fn vertical_slice(&self, beat: usize) -> [Option<u8>; 4] {
        Voice::ALL.map(|v| self.sounding_pitch(v, beat))
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
        assert!(interval::is_perfect_consonance(7));  // P5
        assert!(interval::is_perfect_consonance(0));   // unison
        assert!(interval::is_perfect_consonance(12));  // octave
        assert!(!interval::is_perfect_consonance(4));  // M3

        assert!(interval::is_consonant(7));  // P5
        assert!(interval::is_consonant(3));  // m3
        assert!(interval::is_consonant(4));  // M3
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
