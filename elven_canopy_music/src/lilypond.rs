// LilyPond sheet music output from score grids.
//
// Converts a Grid into a LilyPond (.ly) text file that can be engraved into
// PDF/SVG sheet music using the LilyPond typesetter. Each of the four SATB
// voices gets its own staff in a ChoirStaff, with Vaelith lyrics attached
// via \addlyrics blocks.
//
// The approach mirrors midi.rs: walk each voice's grid cells, collect note/rest
// events with durations, then serialize. The extra complexity here is duration
// decomposition — LilyPond requires durations expressed as power-of-two note
// values (with optional dots), and notes crossing barlines must be split into
// tied segments.
//
// Uses absolute pitches (not \relative) for simplicity and correctness.

use crate::grid::{Grid, Voice};
use crate::mode::{Mode, ModeInstance};
use crate::text_mapping::TextMapping;
use std::fmt::Write;
use std::path::Path;

/// Pitch class names in LilyPond notation (indexed by pitch class 0-11).
/// Uses flats for enharmonic spellings (Palestrina convention).
const LY_PITCH_NAMES: [&str; 12] = [
    "c", "cis", "d", "ees", "e", "f", "fis", "g", "aes", "a", "bes", "b",
];

/// Convert a MIDI pitch number to a LilyPond absolute pitch string.
///
/// LilyPond's `c` with no octave marks = MIDI 48 (C3).
/// Each `'` raises one octave, each `,` lowers one octave.
pub fn midi_to_ly_note(midi_pitch: u8) -> String {
    let pc = (midi_pitch % 12) as usize;
    let octave = (midi_pitch / 12) as i8 - 4; // 0 at octave 4 (MIDI 48-59)
    let name = LY_PITCH_NAMES[pc];
    let mut result = name.to_string();
    if octave > 0 {
        for _ in 0..octave {
            result.push('\'');
        }
    } else if octave < 0 {
        for _ in 0..(-octave) {
            result.push(',');
        }
    }
    result
}

/// A valid LilyPond duration: note value (in eighth-note beats) and its
/// text representation.
const DURATION_TABLE: [(usize, &str); 6] = [
    (8, "1"),   // whole note = 8 eighth-note beats
    (6, "2."),  // dotted half
    (4, "2"),   // half note
    (3, "4."),  // dotted quarter
    (2, "4"),   // quarter note
    (1, "8"),   // eighth note
];

/// Decompose a duration (in eighth-note beats) into a sequence of LilyPond
/// duration strings, largest first. Multiple parts are connected with ties.
///
/// For example: 5 beats = "2" + "8" (half note tied to eighth note).
pub fn decompose_duration(mut beats: usize) -> Vec<&'static str> {
    let mut parts = Vec::new();
    if beats == 0 {
        return parts;
    }
    for &(value, name) in &DURATION_TABLE {
        while beats >= value {
            parts.push(name);
            beats -= value;
        }
    }
    parts
}

/// Split a duration at barlines (every 8 eighth-note beats in 4/4 time).
///
/// Returns a list of fragment durations. A note starting at `start_beat`
/// with total `duration` beats that crosses a barline is split into pieces
/// that each fit within a single bar.
pub fn split_at_barlines(start_beat: usize, duration: usize) -> Vec<usize> {
    let bar_length = 8; // 4/4 time in eighth notes
    let mut fragments = Vec::new();
    let mut remaining = duration;
    let mut pos = start_beat;

    while remaining > 0 {
        let bar_end = ((pos / bar_length) + 1) * bar_length;
        let space_in_bar = bar_end - pos;
        let frag = remaining.min(space_in_bar);
        fragments.push(frag);
        remaining -= frag;
        pos += frag;
    }
    fragments
}

/// Map a ModeInstance to a LilyPond \key command string.
///
/// Aeolian maps to \minor and Ionian to \major (LilyPond convention).
/// Other modes use their modal name.
pub fn mode_to_ly_key(mode: &ModeInstance) -> String {
    let pitch = LY_PITCH_NAMES[mode.final_pc as usize % 12];
    let mode_name = match mode.mode {
        Mode::Dorian => "dorian",
        Mode::Phrygian => "phrygian",
        Mode::Lydian => "lydian",
        Mode::Mixolydian => "mixolydian",
        Mode::Aeolian => "minor",
        Mode::Ionian => "major",
    };
    format!("\\key {} \\{}", pitch, mode_name)
}

/// An event in a voice: either a note or a rest with a duration.
#[derive(Debug, Clone)]
enum VoiceEvent {
    Note { pitch: u8, duration: usize, start_beat: usize },
    Rest { duration: usize, start_beat: usize },
}

/// Walk a voice's grid cells and collect events (notes and rests with durations).
fn collect_voice_events(grid: &Grid, voice: Voice) -> Vec<VoiceEvent> {
    let mut events = Vec::new();
    let row = &grid.voices[voice.index()];
    let mut beat = 0;

    while beat < grid.num_beats {
        let cell = &row[beat];
        if cell.is_rest {
            // Count consecutive rest beats
            let start = beat;
            let mut dur = 1;
            while beat + dur < grid.num_beats && row[beat + dur].is_rest {
                dur += 1;
            }
            events.push(VoiceEvent::Rest { duration: dur, start_beat: start });
            beat += dur;
        } else if cell.attack {
            // Count continuation beats
            let start = beat;
            let pitch = cell.pitch;
            let mut dur = 1;
            while beat + dur < grid.num_beats {
                let next = &row[beat + dur];
                if next.is_rest || next.attack {
                    break;
                }
                dur += 1;
            }
            events.push(VoiceEvent::Note { pitch, duration: dur, start_beat: start });
            beat += dur;
        } else {
            // Orphan continuation (shouldn't happen in well-formed grids)
            beat += 1;
        }
    }
    events
}

/// Render a single voice's music as a LilyPond music expression.
fn render_voice_music(grid: &Grid, voice: Voice) -> String {
    let events = collect_voice_events(grid, voice);
    let mut out = String::new();
    let mut beats_in_bar = 0;

    for event in &events {
        match event {
            VoiceEvent::Note { pitch, duration, start_beat } => {
                let fragments = split_at_barlines(*start_beat, *duration);
                for (i, frag) in fragments.iter().enumerate() {
                    let parts = decompose_duration(*frag);
                    for (j, dur_str) in parts.iter().enumerate() {
                        if !out.is_empty() {
                            out.push(' ');
                        }
                        let _ = write!(out, "{}{}", midi_to_ly_note(*pitch), dur_str);
                        // Tie if there are more parts in this fragment or more fragments
                        let more_parts = j + 1 < parts.len();
                        let more_fragments = i + 1 < fragments.len();
                        if more_parts || more_fragments {
                            out.push('~');
                        }
                    }
                    beats_in_bar = (*start_beat + fragments[..=i].iter().sum::<usize>()) % 8;
                }
            }
            VoiceEvent::Rest { duration, start_beat } => {
                let fragments = split_at_barlines(*start_beat, *duration);
                for (i, frag) in fragments.iter().enumerate() {
                    let parts = decompose_duration(*frag);
                    for dur_str in &parts {
                        if !out.is_empty() {
                            out.push(' ');
                        }
                        let _ = write!(out, "r{}", dur_str);
                    }
                    beats_in_bar = (*start_beat + fragments[..=i].iter().sum::<usize>()) % 8;
                }
            }
        }
    }

    let _ = beats_in_bar; // used only for tracking position
    out
}

/// Render lyrics for a voice from the text mapping.
///
/// For each note event, checks if a syllable span starts at that beat for
/// this voice. If yes, emits the syllable text. If no (melismatic extension),
/// emits `_`. Rests are automatically skipped by LilyPond.
fn render_voice_lyrics(grid: &Grid, voice: Voice, mapping: &TextMapping) -> String {
    let events = collect_voice_events(grid, voice);
    let mut out = String::new();
    let mut has_any_lyrics = false;

    for event in &events {
        if let VoiceEvent::Note { start_beat, duration, .. } = event {
            // Check for syllable at this beat
            let syllable = mapping.spans.iter()
                .find(|s| s.voice == voice && s.start_beat == *start_beat);

            // For notes that cross barlines, we get tied notes — each tied
            // segment after the first needs a `_` skip in lyrics
            let fragments = split_at_barlines(*start_beat, *duration);
            let total_ly_notes: usize = fragments.iter()
                .map(|f| decompose_duration(*f).len())
                .sum();

            if let Some(span) = syllable {
                if !out.is_empty() {
                    out.push(' ');
                }
                // Escape special LilyPond characters in text
                let text = span.text.replace('\"', "\\\"");
                out.push_str(&text);
                has_any_lyrics = true;
                // Skip remaining tied sub-notes
                for _ in 1..total_ly_notes {
                    out.push_str(" _");
                }
            } else {
                // Melismatic: underscore for each sub-note
                for k in 0..total_ly_notes {
                    if !out.is_empty() || k > 0 {
                        out.push(' ');
                    }
                    out.push('_');
                }
            }
        }
        // Rests don't need lyrics entries — LilyPond skips them automatically
    }

    if has_any_lyrics { out } else { String::new() }
}

/// Generate a complete LilyPond file from a Grid, ModeInstance, and TextMapping.
pub fn grid_to_lilypond(
    grid: &Grid,
    mode: &ModeInstance,
    mapping: &TextMapping,
    title: Option<&str>,
) -> String {
    let mut ly = String::new();

    // Header
    ly.push_str("\\version \"2.24.0\"\n\n");

    // Title block
    let title_text = title.unwrap_or("Elven Canopy");
    let mode_subtitle = format!("{:?} on {}", mode.mode,
        LY_PITCH_NAMES[mode.final_pc as usize % 12].to_uppercase());
    let _ = write!(ly, "\\header {{\n  title = \"{}\"\n  subtitle = \"{}\"\n}}\n\n",
        title_text, mode_subtitle);

    // Global block (key, time, tempo)
    let key_str = mode_to_ly_key(mode);
    let _ = write!(ly, "global = {{\n  {} \\time 4/4 \\tempo 4 = {}\n}}\n\n",
        key_str, grid.tempo_bpm);

    // Voice music variables
    let voice_names = ["soprano", "alto", "tenor", "bass"];
    for (vi, voice) in Voice::ALL.iter().enumerate() {
        let music = render_voice_music(grid, *voice);
        let _ = write!(ly, "{} = \\absolute {{\n  \\global\n  {}\n}}\n\n",
            voice_names[vi], music);
    }

    // Score block with ChoirStaff
    ly.push_str("\\score {\n  \\new ChoirStaff <<\n");

    let clefs = ["treble", "treble", "\"treble_8\"", "bass"];
    let display_names = ["Soprano", "Alto", "Tenor", "Bass"];

    for (vi, _voice) in Voice::ALL.iter().enumerate() {
        let _ = write!(ly,
            "    \\new Staff = \"{}\" \\with {{ instrumentName = \"{}\" }} {{\n      \\clef {}\n      \\{}\n    }}\n",
            display_names[vi], display_names[vi], clefs[vi], voice_names[vi]);

        // Add lyrics if there are any for this voice
        let lyrics = render_voice_lyrics(grid, Voice::ALL[vi], mapping);
        if !lyrics.is_empty() {
            let _ = write!(ly, "    \\addlyrics {{ {} }}\n", lyrics);
        }
    }

    ly.push_str("  >>\n");
    ly.push_str("  \\layout { }\n");
    ly.push_str("  \\midi { }\n");
    ly.push_str("}\n");

    ly
}

/// Write a LilyPond file from a Grid.
pub fn write_lilypond(
    grid: &Grid,
    mode: &ModeInstance,
    mapping: &TextMapping,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let ly = grid_to_lilypond(grid, mode, mapping, None);
    std::fs::write(path, ly)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_to_ly_note_middle_c() {
        // MIDI 60 = C4 = c' in LilyPond (one octave above reference)
        assert_eq!(midi_to_ly_note(60), "c'");
    }

    #[test]
    fn test_midi_to_ly_note_reference_octave() {
        // MIDI 48 = C3 = c (no marks, reference octave)
        assert_eq!(midi_to_ly_note(48), "c");
        assert_eq!(midi_to_ly_note(50), "d");
        assert_eq!(midi_to_ly_note(55), "g");
    }

    #[test]
    fn test_midi_to_ly_note_low_octaves() {
        // MIDI 36 = C2 = c,
        assert_eq!(midi_to_ly_note(36), "c,");
        // MIDI 24 = C1 = c,,
        assert_eq!(midi_to_ly_note(24), "c,,");
    }

    #[test]
    fn test_midi_to_ly_note_high_octaves() {
        // MIDI 72 = C5 = c''
        assert_eq!(midi_to_ly_note(72), "c''");
        // MIDI 84 = C6 = c'''
        assert_eq!(midi_to_ly_note(84), "c'''");
    }

    #[test]
    fn test_midi_to_ly_note_accidentals() {
        assert_eq!(midi_to_ly_note(61), "cis'");   // C#4
        assert_eq!(midi_to_ly_note(63), "ees'");    // Eb4
        assert_eq!(midi_to_ly_note(66), "fis'");    // F#4
        assert_eq!(midi_to_ly_note(68), "aes'");    // Ab4
        assert_eq!(midi_to_ly_note(70), "bes'");    // Bb4
    }

    #[test]
    fn test_decompose_duration_single_values() {
        assert_eq!(decompose_duration(8), vec!["1"]);    // whole note
        assert_eq!(decompose_duration(6), vec!["2."]);   // dotted half
        assert_eq!(decompose_duration(4), vec!["2"]);    // half note
        assert_eq!(decompose_duration(3), vec!["4."]);   // dotted quarter
        assert_eq!(decompose_duration(2), vec!["4"]);    // quarter note
        assert_eq!(decompose_duration(1), vec!["8"]);    // eighth note
    }

    #[test]
    fn test_decompose_duration_compound() {
        // 5 = 4 + 1 = half + eighth
        assert_eq!(decompose_duration(5), vec!["2", "8"]);
        // 7 = 6 + 1 = dotted half + eighth
        assert_eq!(decompose_duration(7), vec!["2.", "8"]);
    }

    #[test]
    fn test_decompose_duration_zero() {
        assert_eq!(decompose_duration(0), Vec::<&str>::new());
    }

    #[test]
    fn test_split_at_barlines_within_bar() {
        // Note starting at beat 0, duration 4 — fits in first bar
        assert_eq!(split_at_barlines(0, 4), vec![4]);
        // Note starting at beat 2, duration 3
        assert_eq!(split_at_barlines(2, 3), vec![3]);
    }

    #[test]
    fn test_split_at_barlines_crossing() {
        // Note starting at beat 6, duration 4 — crosses barline at 8
        assert_eq!(split_at_barlines(6, 4), vec![2, 2]);
        // Note starting at beat 4, duration 8 — crosses barline at 8
        assert_eq!(split_at_barlines(4, 8), vec![4, 4]);
    }

    #[test]
    fn test_split_at_barlines_spanning_multiple() {
        // 12 beats starting at beat 2 — spans beats 2..14, crossing barlines at 8 and 16
        // bar 0: beats 2-7 = 6 beats
        // bar 1: beats 8-13 = 6 beats
        assert_eq!(split_at_barlines(2, 12), vec![6, 6]);
        // 20 beats starting at beat 4 — spans beats 4..24, crossing barlines at 8, 16, 24
        // bar 0: beats 4-7 = 4
        // bar 1: beats 8-15 = 8
        // bar 2: beats 16-23 = 8
        assert_eq!(split_at_barlines(4, 20), vec![4, 8, 8]);
    }

    #[test]
    fn test_split_at_barlines_exact_bar() {
        // Exactly one bar starting at barline
        assert_eq!(split_at_barlines(0, 8), vec![8]);
        assert_eq!(split_at_barlines(8, 8), vec![8]);
    }

    #[test]
    fn test_mode_to_ly_key_dorian() {
        let mode = ModeInstance::new(Mode::Dorian, 2);
        assert_eq!(mode_to_ly_key(&mode), "\\key d \\dorian");
    }

    #[test]
    fn test_mode_to_ly_key_aeolian() {
        let mode = ModeInstance::new(Mode::Aeolian, 9);
        assert_eq!(mode_to_ly_key(&mode), "\\key a \\minor");
    }

    #[test]
    fn test_mode_to_ly_key_ionian() {
        let mode = ModeInstance::new(Mode::Ionian, 0);
        assert_eq!(mode_to_ly_key(&mode), "\\key c \\major");
    }

    #[test]
    fn test_mode_to_ly_key_phrygian() {
        let mode = ModeInstance::new(Mode::Phrygian, 4);
        assert_eq!(mode_to_ly_key(&mode), "\\key e \\phrygian");
    }

    #[test]
    fn test_mode_to_ly_key_lydian() {
        let mode = ModeInstance::new(Mode::Lydian, 5);
        assert_eq!(mode_to_ly_key(&mode), "\\key f \\lydian");
    }

    #[test]
    fn test_mode_to_ly_key_mixolydian() {
        let mode = ModeInstance::new(Mode::Mixolydian, 7);
        assert_eq!(mode_to_ly_key(&mode), "\\key g \\mixolydian");
    }

    #[test]
    fn test_render_voice_simple_notes() {
        let mut grid = Grid::new(8); // one bar
        // Quarter note C4 (beats 0-1)
        grid.set_note(Voice::Soprano, 0, 60);
        grid.extend_note(Voice::Soprano, 1);
        // Quarter note E4 (beats 2-3)
        grid.set_note(Voice::Soprano, 2, 64);
        grid.extend_note(Voice::Soprano, 3);
        // Half rest (beats 4-7)

        let music = render_voice_music(&grid, Voice::Soprano);
        assert!(music.contains("c'4"), "Expected c'4, got: {}", music);
        assert!(music.contains("e'4"), "Expected e'4, got: {}", music);
        assert!(music.contains("r2"), "Expected r2, got: {}", music);
    }

    #[test]
    fn test_render_voice_tied_across_barline() {
        let mut grid = Grid::new(16); // two bars
        // Note starting at beat 6, lasting 4 beats (crosses barline)
        grid.set_note(Voice::Soprano, 6, 60);
        grid.extend_note(Voice::Soprano, 7);
        grid.extend_note(Voice::Soprano, 8);
        grid.extend_note(Voice::Soprano, 9);
        // Fill rest before
        // Beats 0-5 are rests, beats 10-15 are rests

        let music = render_voice_music(&grid, Voice::Soprano);
        assert!(music.contains('~'), "Expected tie in: {}", music);
    }

    #[test]
    fn test_grid_to_lilypond_structure() {
        let mut grid = Grid::new(16);
        grid.tempo_bpm = 72;
        // Put a note in each voice
        grid.set_note(Voice::Soprano, 0, 67);
        grid.extend_note(Voice::Soprano, 1);
        grid.set_note(Voice::Alto, 0, 60);
        grid.extend_note(Voice::Alto, 1);
        grid.set_note(Voice::Tenor, 0, 55);
        grid.extend_note(Voice::Tenor, 1);
        grid.set_note(Voice::Bass, 0, 48);
        grid.extend_note(Voice::Bass, 1);

        let mode = ModeInstance::new(Mode::Dorian, 2);
        let mapping = TextMapping { section_phrases: vec![], spans: vec![] };

        let ly = grid_to_lilypond(&grid, &mode, &mapping, Some("Test Piece"));

        assert!(ly.contains("\\version"), "Missing version: {}", ly);
        assert!(ly.contains("Test Piece"), "Missing title");
        assert!(ly.contains("\\key d \\dorian"), "Missing key");
        assert!(ly.contains("\\time 4/4"), "Missing time sig");
        assert!(ly.contains("\\tempo 4 = 72"), "Missing tempo");
        assert!(ly.contains("ChoirStaff"), "Missing ChoirStaff");
        assert!(ly.contains("soprano"), "Missing soprano variable");
        assert!(ly.contains("alto"), "Missing alto variable");
        assert!(ly.contains("tenor"), "Missing tenor variable");
        assert!(ly.contains("bass"), "Missing bass variable");
        assert!(ly.contains("\\clef bass"), "Missing bass clef");
        assert!(ly.contains("\\layout"), "Missing layout block");
    }

    #[test]
    fn test_integration_with_generated_grid() {
        use crate::draft::{fill_draft, generate_final_cadence};
        use crate::markov::{MarkovModels, MotifLibrary};
        use crate::structure::{generate_structure, apply_structure, apply_responses};
        use crate::vaelith::generate_phrases_with_brightness;
        use crate::text_mapping::apply_text_mapping;
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        let mut rng = StdRng::seed_from_u64(42);
        let models = MarkovModels::default_models();
        let motif_library = MotifLibrary::default_library();
        let mode = ModeInstance::new(Mode::Dorian, 2);

        let plan = generate_structure(&motif_library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        grid.tempo_bpm = 72;
        let mut structural = apply_structure(&mut grid, &plan);
        apply_responses(&mut grid, &plan, &mode, &mut structural);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);
        generate_final_cadence(&mut grid, &mode, &mut structural);

        let phrases = generate_phrases_with_brightness(2, 0.5, &mut rng);
        let mapping = apply_text_mapping(&mut grid, &plan, &phrases);

        let ly = grid_to_lilypond(&grid, &mode, &mapping, None);

        // Structural checks
        assert!(ly.contains("\\version"));
        assert!(ly.contains("ChoirStaff"));
        assert!(ly.contains("\\addlyrics"), "Expected lyrics in output");

        // Should have real note content, not just rests
        assert!(ly.contains("c'") || ly.contains("d'") || ly.contains("e'"),
            "Expected some notes in the output");

        // Length sanity — a 2-section piece should produce substantial output
        assert!(ly.len() > 500, "Output too short ({} bytes)", ly.len());
    }
}
