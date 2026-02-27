// Modal scale support for Renaissance-style music generation.
//
// Palestrina's music uses the church modes (Dorian, Phrygian, Mixolydian,
// etc.), not modern major/minor tonality. Each mode has a characteristic
// set of scale degrees and melodic tendencies.
//
// This module provides:
// - Mode definitions with their scale degree patterns
// - Pitch-to-mode-degree mapping
// - Scoring terms that reward staying in mode
// - Functions to snap pitches to the nearest in-mode note
//
// Used by draft.rs for initial pitch selection and scoring.rs for
// modal compliance scoring.

use serde::{Deserialize, Serialize};

/// The seven church modes, each defined by their interval pattern from the final.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// D Dorian: D E F G A B C D  (natural minor with raised 6th)
    Dorian,
    /// E Phrygian: E F G A B C D E  (distinctive half-step from 1 to 2)
    Phrygian,
    /// F Lydian: F G A B C D E F  (raised 4th)
    Lydian,
    /// G Mixolydian: G A B C D E F G  (major with lowered 7th)
    Mixolydian,
    /// A Aeolian: A B C D E F G A  (natural minor)
    Aeolian,
    /// C Ionian: C D E F G A B C  (major scale — less common in Palestrina)
    Ionian,
}

impl Mode {
    /// Semitone intervals from the final to each scale degree.
    /// Returns 7 intervals representing degrees 1-7.
    pub fn intervals(self) -> [u8; 7] {
        match self {
            Mode::Dorian => [0, 2, 3, 5, 7, 9, 10],
            Mode::Phrygian => [0, 1, 3, 5, 7, 8, 10],
            Mode::Lydian => [0, 2, 4, 6, 7, 9, 11],
            Mode::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
            Mode::Aeolian => [0, 2, 3, 5, 7, 8, 10],
            Mode::Ionian => [0, 2, 4, 5, 7, 9, 11],
        }
    }

    /// The 12 pitch classes that are "in mode" (as a boolean array indexed by
    /// pitch class 0-11). The final is at pitch class 0.
    pub fn pitch_classes(self) -> [bool; 12] {
        let mut pcs = [false; 12];
        for &interval in &self.intervals() {
            pcs[interval as usize] = true;
        }
        pcs
    }

    /// Get common modes used in Palestrina's music with their typical finals.
    /// Returns (mode, final_pitch_class) pairs.
    pub fn common_modes() -> &'static [(Mode, u8)] {
        &[
            (Mode::Dorian, 2),     // D Dorian (final = D, pc 2)
            (Mode::Phrygian, 4),   // E Phrygian (final = E, pc 4)
            (Mode::Mixolydian, 7), // G Mixolydian (final = G, pc 7)
            (Mode::Aeolian, 9),    // A Aeolian (final = A, pc 9)
            (Mode::Ionian, 0),     // C Ionian (final = C, pc 0)
            (Mode::Dorian, 7),     // G Dorian (transposed, final = G, pc 7)
        ]
    }
}

/// A specific mode instance: a mode type plus its final (tonic) pitch class.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModeInstance {
    pub mode: Mode,
    /// Pitch class of the final (0 = C, 2 = D, 4 = E, etc.)
    pub final_pc: u8,
}

impl ModeInstance {
    pub fn new(mode: Mode, final_pc: u8) -> Self {
        ModeInstance {
            mode,
            final_pc: final_pc % 12,
        }
    }

    /// D Dorian — the most common mode in Palestrina.
    pub fn d_dorian() -> Self {
        ModeInstance::new(Mode::Dorian, 2)
    }

    /// Check if a MIDI pitch is in this mode.
    pub fn is_in_mode(&self, pitch: u8) -> bool {
        let pc = (pitch + 12 - self.final_pc) % 12;
        self.mode.pitch_classes()[pc as usize]
    }

    /// Get all in-mode pitches in a range.
    pub fn pitches_in_range(&self, low: u8, high: u8) -> Vec<u8> {
        (low..=high).filter(|&p| self.is_in_mode(p)).collect()
    }

    /// Snap a pitch to the nearest in-mode pitch.
    pub fn snap_to_mode(&self, pitch: u8) -> u8 {
        if self.is_in_mode(pitch) {
            return pitch;
        }
        // Check adjacent pitches
        for offset in 1u8..=6 {
            if pitch >= offset && self.is_in_mode(pitch - offset) {
                return pitch - offset;
            }
            if pitch + offset <= 127 && self.is_in_mode(pitch + offset) {
                return pitch + offset;
            }
        }
        pitch // shouldn't happen with 7-note modes
    }

    /// Get the scale degree (0-6) of a pitch, or None if not in mode.
    pub fn scale_degree(&self, pitch: u8) -> Option<u8> {
        let pc = (pitch + 12 - self.final_pc) % 12;
        let intervals = self.mode.intervals();
        intervals.iter().position(|&iv| iv == pc).map(|d| d as u8)
    }

    /// Get the pitch of a given scale degree in a given octave.
    pub fn degree_to_pitch(&self, degree: u8, octave: u8) -> u8 {
        let intervals = self.mode.intervals();
        let pc = (self.final_pc + intervals[(degree % 7) as usize]) % 12;
        let extra_octave = degree / 7;
        octave * 12 + pc + extra_octave * 12
    }

    /// Score how well a pitch fits the mode (0.0 = out of mode, 1.0 = in mode,
    /// higher for structurally important degrees like final and 5th).
    pub fn pitch_fitness(&self, pitch: u8) -> f64 {
        if let Some(degree) = self.scale_degree(pitch) {
            match degree {
                0 => 1.5, // Final — most important
                4 => 1.3, // 5th above final — second most important
                2 => 1.1, // 3rd — defines mode quality (major/minor)
                _ => 1.0, // Other in-mode pitches
            }
        } else {
            0.0 // Out of mode
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d_dorian_pitches() {
        let mode = ModeInstance::d_dorian();

        // D4=62, E4=64, F4=65, G4=67, A4=69, B4=71, C5=72
        assert!(mode.is_in_mode(62)); // D
        assert!(mode.is_in_mode(64)); // E
        assert!(mode.is_in_mode(65)); // F
        assert!(mode.is_in_mode(67)); // G
        assert!(mode.is_in_mode(69)); // A
        assert!(mode.is_in_mode(71)); // B
        assert!(mode.is_in_mode(72)); // C

        assert!(!mode.is_in_mode(63)); // Eb (not in D Dorian)
        assert!(!mode.is_in_mode(66)); // F# (not in D Dorian)
    }

    #[test]
    fn test_scale_degree() {
        let mode = ModeInstance::d_dorian();
        assert_eq!(mode.scale_degree(62), Some(0)); // D = degree 0 (final)
        assert_eq!(mode.scale_degree(69), Some(4)); // A = degree 4 (5th)
        assert_eq!(mode.scale_degree(63), None); // Eb = not in mode
    }

    #[test]
    fn test_snap_to_mode() {
        let mode = ModeInstance::d_dorian();
        assert_eq!(mode.snap_to_mode(62), 62); // D stays D
        assert_eq!(mode.snap_to_mode(63), 62); // Eb snaps to D
        assert_eq!(mode.snap_to_mode(66), 65); // F# snaps to F
    }

    #[test]
    fn test_phrygian_half_step() {
        let mode = ModeInstance::new(Mode::Phrygian, 4); // E Phrygian
        // E to F is 1 semitone — the characteristic Phrygian half-step
        assert!(mode.is_in_mode(64)); // E
        assert!(mode.is_in_mode(65)); // F
        assert_eq!(mode.scale_degree(64), Some(0)); // E = final
        assert_eq!(mode.scale_degree(65), Some(1)); // F = 2nd degree
    }
}
