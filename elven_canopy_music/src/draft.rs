// Draft generation: filling the grid's free cells with Markov-guided pitches.
//
// After structure.rs has placed motif entries (structural cells), this module
// fills the remaining empty cells with musically plausible pitches. It uses:
// - Melodic Markov model for interval-based pitch proposals
// - Harmonic model (unigram + transitions) for voice-pair compatibility
// - Mode instance for scale fitness and pitch snapping
//
// Also provides generate_final_cadence() which places a V→I cadential
// formula in the last beats, marking those cells as structural.
//
// Voice entries are staggered: each voice waits until its first structural
// cell before filling, preserving the imitative texture from structure.rs.
//
// The draft doesn't need to be perfect — it's refined by SA (sa.rs). But a
// good draft dramatically reduces the work SA needs to do.

use crate::grid::{Grid, Voice, interval};
use crate::markov::MarkovModels;
use crate::mode::ModeInstance;
use rand::Rng;

/// Fill all rest cells in the grid with Markov-sampled pitches.
///
/// Processes each voice left-to-right, using the melodic model conditioned
/// on recent intervals. Harmonic model weights proposals by consonance with
/// other voices. All pitches are snapped to the given mode.
pub fn fill_draft(
    grid: &mut Grid,
    models: &MarkovModels,
    structural_cells: &[(usize, usize)],
    mode: &ModeInstance,
    rng: &mut impl Rng,
) {
    let structural_set: std::collections::HashSet<(usize, usize)> =
        structural_cells.iter().copied().collect();

    // Find section start beats for each voice (where structural motif entries begin)
    // These help us insert breathing room between sections.
    let mut section_starts: Vec<Vec<usize>> = vec![Vec::new(); 4];
    for &(vi, beat) in structural_cells {
        // An attack on a structural cell that has a rest before it marks a section start
        let cell = grid.cell(Voice::ALL[vi], beat);
        if cell.attack
            && !cell.is_rest
            && (beat == 0
                || grid.cell(Voice::ALL[vi], beat - 1).is_rest
                || !structural_set.contains(&(vi, beat - 1)))
        {
            section_starts[vi].push(beat);
        }
    }
    for starts in &mut section_starts {
        starts.sort();
        starts.dedup();
    }

    for voice in Voice::ALL {
        fill_voice(
            grid,
            voice,
            models,
            &structural_set,
            &section_starts[voice.index()],
            mode,
            rng,
        );
    }
}

/// Fill free cells in a single voice.
fn fill_voice(
    grid: &mut Grid,
    voice: Voice,
    models: &MarkovModels,
    structural: &std::collections::HashSet<(usize, usize)>,
    section_starts: &[usize],
    mode: &ModeInstance,
    rng: &mut impl Rng,
) {
    let (range_low, range_high) = voice.range();
    let mut recent_intervals: Vec<i8> = Vec::new();
    let mut last_pitch: Option<u8> = None;
    let mut beats_since_rest: usize = 0;

    // Find the first structural cell for this voice — don't fill before it
    let first_structural_beat = (0..grid.num_beats)
        .find(|&b| structural.contains(&(voice.index(), b)))
        .unwrap_or(0);

    for beat in 0..grid.num_beats {
        // Skip structural cells — they're already placed
        if structural.contains(&(voice.index(), beat)) {
            let cell = grid.cell(voice, beat);
            if cell.attack && !cell.is_rest {
                if let Some(prev) = last_pitch {
                    let iv = cell.pitch as i8 - prev as i8;
                    recent_intervals.push(iv);
                    if recent_intervals.len() > 4 {
                        recent_intervals.remove(0);
                    }
                }
                last_pitch = Some(cell.pitch);
                beats_since_rest += 1;
            }
            continue;
        }

        // Skip non-rest cells that are continuations
        let cell = grid.cell(voice, beat);
        if !cell.is_rest && !cell.attack {
            beats_since_rest += 1;
            continue;
        }

        let beat_in_bar = beat % 8; // position within a 4/4 bar

        // Check if a new structural section starts soon (within 2 beats)
        let near_section_start = section_starts.iter().any(|&s| s > beat && s <= beat + 2);

        // Breathing: insert rests for phrase structure
        let should_rest = if last_pitch.is_none() {
            // Don't enter before the voice's first structural entry
            beat < first_structural_beat
        } else if near_section_start {
            // Rest just before a new structural section for clean transitions
            true
        } else if beats_since_rest > 24 && beat_in_bar == 0 {
            // After ~3 bars, take a breath on a downbeat
            rng.random_bool(0.4)
        } else {
            rng.random_bool(0.03)
        };

        if should_rest {
            beats_since_rest = 0;
            continue;
        }

        // Sample a pitch
        let pitch = if let Some(prev) = last_pitch {
            let rng_val: f64 = rng.random();
            let proposed_interval = models.melodic.sample(&recent_intervals, rng_val);
            let raw_pitch = (prev as i16 + proposed_interval as i16)
                .clamp(range_low as i16, range_high as i16) as u8;
            let proposed_pitch = mode.snap_to_mode(raw_pitch);

            // Try several alternatives, pick best
            let mut best_pitch = proposed_pitch;
            let mut best_score = pitch_score(grid, voice, beat, proposed_pitch, models, mode);

            for _ in 0..4 {
                let alt_rng: f64 = rng.random();
                let alt_interval = models.melodic.sample(&recent_intervals, alt_rng);
                let raw = (prev as i16 + alt_interval as i16)
                    .clamp(range_low as i16, range_high as i16) as u8;
                let alt_pitch = mode.snap_to_mode(raw);
                let alt_score = pitch_score(grid, voice, beat, alt_pitch, models, mode);
                if alt_score > best_score {
                    best_score = alt_score;
                    best_pitch = alt_pitch;
                }
            }

            best_pitch
        } else {
            // First note: start on a structurally important mode degree
            let mid = (range_low + range_high) / 2;
            // Prefer the final or 5th
            let candidates = mode.pitches_in_range(mid.saturating_sub(3), mid + 3);
            if candidates.is_empty() {
                mode.snap_to_mode(mid)
            } else {
                // Weight toward final and 5th
                let weights: Vec<f64> = candidates.iter().map(|&p| mode.pitch_fitness(p)).collect();
                let total: f64 = weights.iter().sum();
                let r: f64 = rng.random::<f64>() * total;
                let mut cum = 0.0;
                let mut chosen = candidates[0];
                for (i, &w) in weights.iter().enumerate() {
                    cum += w;
                    if cum > r {
                        chosen = candidates[i];
                        break;
                    }
                }
                chosen
            }
        };

        grid.set_note(voice, beat, pitch);

        // Variable note duration based on metric position and style
        let hold_beats = match beat_in_bar {
            0 => rng.random_range(2..=5),     // downbeat: half to dotted half
            4 => rng.random_range(1..=3),     // beat 3: quarter to dotted quarter
            2 | 6 => rng.random_range(1..=2), // weak beats: eighth to quarter
            _ => {
                if rng.random_bool(0.6) {
                    1
                } else {
                    0
                }
            } // off-beats
        };

        for hold in 1..=hold_beats {
            let next = beat + hold;
            if next < grid.num_beats && !structural.contains(&(voice.index(), next)) {
                let next_cell = grid.cell(voice, next);
                if next_cell.is_rest {
                    grid.extend_note(voice, next);
                }
            }
        }

        // Update context
        if let Some(prev) = last_pitch {
            let iv = pitch as i8 - prev as i8;
            recent_intervals.push(iv);
            if recent_intervals.len() > 4 {
                recent_intervals.remove(0);
            }
        }
        last_pitch = Some(pitch);
        beats_since_rest += 1;
    }
}

/// Combined score for a proposed pitch at a given position.
/// Considers harmonic compatibility with other voices, modal fitness,
/// and trained harmonic model preferences.
fn pitch_score(
    grid: &Grid,
    voice: Voice,
    beat: usize,
    proposed_pitch: u8,
    models: &MarkovModels,
    mode: &ModeInstance,
) -> f64 {
    let mut score = 0.0;

    // Modal fitness
    score += mode.pitch_fitness(proposed_pitch) * 2.0;

    // Harmonic compatibility with other voices
    for other_voice in Voice::ALL {
        if other_voice == voice {
            continue;
        }

        if let Some(other_pitch) = grid.sounding_pitch(other_voice, beat) {
            let iv = interval::semitones(other_pitch, proposed_pitch);

            // Basic consonance/dissonance
            if interval::is_consonant(iv) {
                score += 2.0;
            } else {
                score -= 1.5;
            }

            let is_strong = beat.is_multiple_of(4);
            if is_strong && interval::is_perfect_consonance(iv) {
                score += 1.0;
            }

            // Use trained harmonic model for finer preference
            let iv_clamped = iv.clamp(-24, 24) as i8;
            if let Some(&weight) = models.harmonic.unigram.get(&iv_clamped) {
                let total: f64 = models.harmonic.unigram.values().sum();
                if total > 0.0 {
                    // Normalize to [0, 1] and scale
                    score += (weight / total) * 3.0;
                }
            }

            // Also check harmonic transitions from previous beat
            if beat > 0 {
                let prev_other = grid.sounding_pitch(other_voice, beat - 1);
                let prev_self = grid.sounding_pitch(voice, beat - 1);
                if let (Some(po), Some(ps)) = (prev_other, prev_self) {
                    let prev_iv = interval::semitones(po, ps);
                    let prev_iv_clamped = prev_iv.clamp(-24, 24) as i8;
                    let key = prev_iv_clamped.to_string();
                    if let Some(table) = models.harmonic.transitions.get(&key)
                        && let Some(&weight) = table.get(&iv_clamped)
                    {
                        let total: f64 = table.values().sum();
                        if total > 0.0 {
                            score += (weight / total) * 2.0;
                        }
                    }
                }
            }

            let abs_iv = iv.unsigned_abs();
            if abs_iv > 24 {
                score -= 1.0;
            }

            // Penalize parallel 5ths/octaves with previous beat
            if beat > 0 {
                let prev_other = grid.sounding_pitch(other_voice, beat - 1);
                let prev_self = grid.sounding_pitch(voice, beat - 1);
                if let (Some(po), Some(ps)) = (prev_other, prev_self) {
                    let prev_iv = interval::semitones(po, ps);
                    let curr_ic = (iv.unsigned_abs()) % 12;
                    let prev_ic = (prev_iv.unsigned_abs()) % 12;

                    let motion_self = proposed_pitch as i16 - ps as i16;
                    let motion_other = other_pitch as i16 - po as i16;
                    let same_direction = (motion_self > 0 && motion_other > 0)
                        || (motion_self < 0 && motion_other < 0);

                    if same_direction {
                        // Parallel 5ths or octaves — heavy penalty
                        if (curr_ic == 7 && prev_ic == 7) || (curr_ic == 0 && prev_ic == 0) {
                            score -= 10.0;
                        }
                        // Hidden (direct) 5ths/octaves — lighter penalty
                        if (curr_ic == 7 && prev_ic != 7) || (curr_ic == 0 && prev_ic != 0) {
                            score -= 3.0;
                        }
                    }
                }
            }
        }
    }

    // Voice crossing: penalize if this pitch crosses another voice
    for other_voice in Voice::ALL {
        if other_voice == voice {
            continue;
        }
        if let Some(other_pitch) = grid.sounding_pitch(other_voice, beat) {
            // Higher-numbered voice should have lower pitch
            if voice.index() < other_voice.index() && proposed_pitch < other_pitch {
                score -= 5.0;
            }
            if voice.index() > other_voice.index() && proposed_pitch > other_pitch {
                score -= 5.0;
            }
        }
    }

    score
}

/// Generate a proper final cadence in the last beats of the piece.
///
/// Places a cadential formula that converges all voices to a perfect
/// consonance on the mode's final. This is called after fill_draft
/// and marks the cadence cells as structural so SA doesn't disturb them.
///
/// The cadence occupies the last 6 eighth-note beats:
///   - Beat -6: penultimate harmony (dominant-function chord)
///   - Beat -4: suspension/resolution (soprano step, bass 5th→final)
///   - Beat -2: final chord (all voices on final/5th/octave)
///   - Beats -1,0: held final
pub fn generate_final_cadence(
    grid: &mut Grid,
    mode: &ModeInstance,
    structural_cells: &mut Vec<(usize, usize)>,
) {
    let n = grid.num_beats;
    if n < 8 {
        return; // Too short for a cadence
    }

    // Determine target pitches for the final chord
    let final_pc = mode.final_pc;

    // Find the final/5th pitches in each voice's range
    let final_pitches: [u8; 4] = Voice::ALL.map(|v| {
        let (low, high) = v.range();
        let mid = (low + high) / 2;
        // Find the nearest pitch with the final's pitch class
        nearest_pc_in_range(final_pc, mid, low, high)
    });

    // For the penultimate beat, use the 5th above the final (dominant function)
    // This gives a V→I cadential motion
    let fifth_pc = (final_pc + 7) % 12;

    // Bass: 5th → final (the strongest cadential bass motion)
    let bass_range = Voice::Bass.range();
    let bass_fifth = nearest_pc_in_range(fifth_pc, final_pitches[3], bass_range.0, bass_range.1);
    let bass_final = final_pitches[3];

    // Soprano: step above → final (resolving down)
    let sop_final = final_pitches[0];
    // One scale degree above the final
    let sop_penult = mode.snap_to_mode(sop_final + 2);

    // Alto: stays on or near the 5th/3rd of final chord
    let alto_range = Voice::Alto.range();
    let third_pc = (final_pc + mode.mode.intervals()[2]) % 12;
    let alto_final = nearest_pc_in_range(fifth_pc, final_pitches[1], alto_range.0, alto_range.1);
    let alto_penult = nearest_pc_in_range(third_pc, alto_final, alto_range.0, alto_range.1);

    // Tenor: complementary motion
    let tenor_range = Voice::Tenor.range();
    let tenor_final = nearest_pc_in_range(final_pc, final_pitches[2], tenor_range.0, tenor_range.1);
    let tenor_penult = nearest_pc_in_range(fifth_pc, tenor_final, tenor_range.0, tenor_range.1);

    // Place the cadence
    let cadence_start = n.saturating_sub(6);

    // Penultimate chord (beat -6 to -4)
    let penult_beat = cadence_start;
    place_cadence_note(
        grid,
        Voice::Soprano,
        penult_beat,
        sop_penult,
        2,
        structural_cells,
    );
    place_cadence_note(
        grid,
        Voice::Alto,
        penult_beat,
        alto_penult,
        2,
        structural_cells,
    );
    place_cadence_note(
        grid,
        Voice::Tenor,
        penult_beat,
        tenor_penult,
        2,
        structural_cells,
    );
    place_cadence_note(
        grid,
        Voice::Bass,
        penult_beat,
        bass_fifth,
        2,
        structural_cells,
    );

    // Resolution to final chord (beat -4 to end)
    let final_beat = cadence_start + 2;
    let hold = n - final_beat - 1;
    place_cadence_note(
        grid,
        Voice::Soprano,
        final_beat,
        sop_final,
        hold,
        structural_cells,
    );
    place_cadence_note(
        grid,
        Voice::Alto,
        final_beat,
        alto_final,
        hold,
        structural_cells,
    );
    place_cadence_note(
        grid,
        Voice::Tenor,
        final_beat,
        tenor_final,
        hold,
        structural_cells,
    );
    place_cadence_note(
        grid,
        Voice::Bass,
        final_beat,
        bass_final,
        hold,
        structural_cells,
    );
}

/// Place a note with hold duration and mark all cells as structural.
fn place_cadence_note(
    grid: &mut Grid,
    voice: Voice,
    beat: usize,
    pitch: u8,
    hold_beats: usize,
    structural: &mut Vec<(usize, usize)>,
) {
    if beat >= grid.num_beats {
        return;
    }
    grid.set_note(voice, beat, pitch);
    structural.push((voice.index(), beat));

    for h in 1..=hold_beats {
        if beat + h < grid.num_beats {
            grid.extend_note(voice, beat + h);
            structural.push((voice.index(), beat + h));
        }
    }
}

/// Find the nearest MIDI pitch with a given pitch class within a range.
fn nearest_pc_in_range(target_pc: u8, near: u8, low: u8, high: u8) -> u8 {
    let mut best = near;
    let mut best_dist = 128u8;

    for p in low..=high {
        if p % 12 == target_pc {
            let dist = p.abs_diff(near);
            if dist < best_dist {
                best = p;
                best_dist = dist;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markov::{MarkovModels, MotifLibrary};
    use crate::structure::{apply_structure, generate_structure};

    #[test]
    fn test_final_cadence_ends_on_mode_final() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mode = ModeInstance::d_dorian();
        let mut rng = rand::rng();

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let mut structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);
        generate_final_cadence(&mut grid, &mode, &mut structural);

        // The last sounding beat should have all voices on the mode final or its 5th
        let last_beat = grid.num_beats - 1;
        for voice in Voice::ALL {
            if let Some(pitch) = grid.sounding_pitch(voice, last_beat) {
                let pc = pitch % 12;
                let final_pc = mode.final_pc;
                let fifth_pc = (final_pc + 7) % 12;
                assert!(
                    pc == final_pc || pc == fifth_pc,
                    "Voice {:?} on final beat has pitch {} (pc {}), expected final {} or 5th {}",
                    voice,
                    pitch,
                    pc,
                    final_pc,
                    fifth_pc
                );
            }
        }
    }

    #[test]
    fn test_fill_draft_produces_notes() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mode = ModeInstance::d_dorian();
        let mut rng = rand::rng();

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        let mut note_count = 0;
        for voice in Voice::ALL {
            for beat in 0..grid.num_beats {
                if !grid.cell(voice, beat).is_rest {
                    note_count += 1;
                }
            }
        }
        assert!(
            note_count > 20,
            "Draft should produce substantial notes, got {}",
            note_count
        );
    }

    #[test]
    fn test_draft_respects_mode() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mode = ModeInstance::d_dorian();
        let mut rng = rand::rng();

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        // Count how many non-structural attack notes are in mode
        let structural_set: std::collections::HashSet<(usize, usize)> =
            structural.iter().copied().collect();

        let mut in_mode = 0;
        let mut total = 0;
        for voice in Voice::ALL {
            for beat in 0..grid.num_beats {
                if structural_set.contains(&(voice.index(), beat)) {
                    continue;
                }
                let cell = grid.cell(voice, beat);
                if cell.attack && !cell.is_rest {
                    total += 1;
                    if mode.is_in_mode(cell.pitch) {
                        in_mode += 1;
                    }
                }
            }
        }
        let pct = if total > 0 {
            in_mode as f64 / total as f64
        } else {
            0.0
        };
        assert!(
            pct > 0.85,
            "At least 85% of draft notes should be in mode, got {:.0}%",
            pct * 100.0
        );
    }
}
