// MIDI output from score grids.
//
// Converts a Grid into a Standard MIDI File (SMF) for playback and evaluation.
// Each voice maps to a separate MIDI track. The grid's eighth-note beats
// map to MIDI ticks based on the tempo.
//
// Uses the `midly` crate for MIDI writing. Output is SMF Format 1 (multi-track).

use crate::grid::{Grid, Voice};
use midly::{
    Format, Header, MidiMessage, Smf, Timing, Track, TrackEvent, TrackEventKind,
    num::{u4, u7, u15, u24, u28},
};
use std::path::Path;

/// Ticks per quarter note in MIDI output.
const TICKS_PER_QUARTER: u16 = 480;

/// Ticks per eighth note (half a quarter note).
const TICKS_PER_EIGHTH: u32 = TICKS_PER_QUARTER as u32 / 2;

/// Convert a Grid to MIDI and write to a file.
pub fn write_midi(grid: &Grid, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let smf = grid_to_smf(grid);
    let mut buf = Vec::new();
    smf.write(&mut buf)?;
    std::fs::write(path, &buf)?;
    Ok(())
}

/// Convert a Grid to an in-memory SMF.
fn grid_to_smf(grid: &Grid) -> Smf<'static> {
    let mut smf = Smf::new(Header::new(
        Format::Parallel,
        Timing::Metrical(u15::new(TICKS_PER_QUARTER)),
    ));

    // Track 0: tempo track
    let mut tempo_track: Track<'static> = Vec::new();
    let tempo_microseconds = 60_000_000 / grid.tempo_bpm as u32;
    tempo_track.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(midly::MetaMessage::Tempo(u24::new(tempo_microseconds))),
    });
    tempo_track.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
    });
    smf.tracks.push(tempo_track);

    // One track per voice
    let voice_names = ["Soprano", "Alto", "Tenor", "Bass"];
    let channels = [u4::new(0), u4::new(1), u4::new(2), u4::new(3)];

    for (vi, voice) in Voice::ALL.iter().enumerate() {
        let mut track: Track<'static> = Vec::new();

        // Track name
        track.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(midly::MetaMessage::TrackName(
                voice_names[vi].as_bytes(),
            )),
        });

        // Set to choir aahs (program 52) for choral sound
        track.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Midi {
                channel: channels[vi],
                message: MidiMessage::ProgramChange {
                    program: u7::new(52),
                },
            },
        });

        let voice_row = &grid.voices[voice.index()];
        let mut current_tick: u32 = 0;
        let mut last_event_tick: u32 = 0;
        let mut note_on: Option<u8> = None;

        for beat in 0..grid.num_beats {
            let cell = &voice_row[beat];
            let beat_tick = beat as u32 * TICKS_PER_EIGHTH;

            if cell.is_rest {
                // End any sounding note
                if let Some(pitch) = note_on.take() {
                    let delta = beat_tick - last_event_tick;
                    track.push(TrackEvent {
                        delta: u28::new(delta),
                        kind: TrackEventKind::Midi {
                            channel: channels[vi],
                            message: MidiMessage::NoteOff {
                                key: u7::new(pitch),
                                vel: u7::new(0),
                            },
                        },
                    });
                    last_event_tick = beat_tick;
                }
            } else if cell.attack {
                // End previous note if any
                if let Some(pitch) = note_on.take() {
                    let delta = beat_tick - last_event_tick;
                    track.push(TrackEvent {
                        delta: u28::new(delta),
                        kind: TrackEventKind::Midi {
                            channel: channels[vi],
                            message: MidiMessage::NoteOff {
                                key: u7::new(pitch),
                                vel: u7::new(0),
                            },
                        },
                    });
                    last_event_tick = beat_tick;
                }
                // Start new note
                let delta = beat_tick - last_event_tick;
                track.push(TrackEvent {
                    delta: u28::new(delta),
                    kind: TrackEventKind::Midi {
                        channel: channels[vi],
                        message: MidiMessage::NoteOn {
                            key: u7::new(cell.pitch),
                            vel: u7::new(80),
                        },
                    },
                });
                last_event_tick = beat_tick;
                note_on = Some(cell.pitch);
            }
            // If not attack and not rest, it's a continuation — do nothing

            current_tick = beat_tick + TICKS_PER_EIGHTH;
        }

        // End final note
        if let Some(pitch) = note_on.take() {
            let delta = current_tick - last_event_tick;
            track.push(TrackEvent {
                delta: u28::new(delta),
                kind: TrackEventKind::Midi {
                    channel: channels[vi],
                    message: MidiMessage::NoteOff {
                        key: u7::new(pitch),
                        vel: u7::new(0),
                    },
                },
            });
            let _ = current_tick;
        }

        track.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
        });

        smf.tracks.push(track);
    }

    smf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_to_smf_basic() {
        let mut grid = Grid::new(8);
        // Quarter note C4 in soprano (beats 0-1)
        grid.set_note(Voice::Soprano, 0, 60);
        grid.extend_note(Voice::Soprano, 1);
        // Eighth note E4 (beat 2)
        grid.set_note(Voice::Soprano, 2, 64);

        let smf = grid_to_smf(&grid);
        // 1 tempo track + 4 voice tracks
        assert_eq!(smf.tracks.len(), 5);
    }
}
