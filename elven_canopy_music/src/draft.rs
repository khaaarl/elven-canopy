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
use crate::mode::{ModeInstance, Score};
use elven_canopy_prng::GameRng;

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
    rng: &mut GameRng,
) {
    let structural_set: std::collections::HashSet<(usize, usize)> =
        structural_cells.iter().copied().collect();

    // Find section start beats for each voice (where structural motif entries begin)
    // These help us insert breathing room between sections.
    let mut section_starts: Vec<Vec<usize>> = vec![Vec::new(); 4];
    for &(vi, beat) in structural_cells {
        if vi >= 4 || grid.voices[vi].is_empty() {
            continue;
        }
        // An attack on a structural cell that has a rest before it marks a section start
        let voice = Voice::ALL[vi];
        let cell = grid.cell(voice, beat);
        if cell.attack
            && !cell.is_rest
            && (beat == 0
                || grid.cell(voice, beat - 1).is_rest
                || !structural_set.contains(&(vi, beat - 1)))
        {
            section_starts[vi].push(beat);
        }
    }
    for starts in &mut section_starts {
        starts.sort();
        starts.dedup();
    }

    let active = grid.active_voices().to_vec();
    for &voice in &active {
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
    rng: &mut GameRng,
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
            // After ~3 bars, take a breath on a downbeat (~40%)
            rng.range_usize(0, 5) < 2
        } else {
            // Rare random rest (~3%)
            rng.range_usize(0, 100) < 3
        };

        if should_rest {
            beats_since_rest = 0;
            continue;
        }

        // Sample a pitch
        let pitch = if let Some(prev) = last_pitch {
            let rng_val: u64 = rng.next_u64();
            let proposed_interval = models.melodic.sample(&recent_intervals, rng_val);
            let raw_pitch = (prev as i16 + proposed_interval as i16)
                .clamp(range_low as i16, range_high as i16) as u8;
            let proposed_pitch = mode.snap_to_mode(raw_pitch);

            // Try several alternatives, pick best
            let mut best_pitch = proposed_pitch;
            let mut best_score = pitch_score(grid, voice, beat, proposed_pitch, models, mode);

            for _ in 0..4 {
                let alt_rng: u64 = rng.next_u64();
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
                // Weight toward final and 5th (using Score for determinism)
                let weights: Vec<i64> = candidates
                    .iter()
                    .map(|&p| mode.pitch_fitness(p).raw())
                    .collect();
                let total: u64 = weights.iter().map(|&w| w.max(0) as u64).sum();
                if total == 0 {
                    candidates[0]
                } else {
                    let r: u64 = rng.next_u64() % total;
                    let mut cum: u64 = 0;
                    let mut chosen = candidates[0];
                    for (i, &w) in weights.iter().enumerate() {
                        cum += w.max(0) as u64;
                        if cum > r {
                            chosen = candidates[i];
                            break;
                        }
                    }
                    chosen
                }
            }
        };

        grid.set_note(voice, beat, pitch);

        // Variable note duration based on metric position and style
        let hold_beats = match beat_in_bar {
            0 => rng.range_usize_inclusive(2, 5), // downbeat: half to dotted half
            4 => rng.range_usize_inclusive(1, 3), // beat 3: quarter to dotted quarter
            2 | 6 => rng.range_usize_inclusive(1, 2), // weak beats: eighth to quarter
            _ => {
                if rng.range_usize(0, 5) < 3 {
                    1 // ~60%
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
) -> Score {
    let mut score = Score::ZERO;

    // Modal fitness
    score += mode.pitch_fitness(proposed_pitch).mul_int(2);

    // Harmonic compatibility with other active voices
    for &other_voice in grid.active_voices() {
        if other_voice == voice {
            continue;
        }

        if let Some(other_pitch) = grid.sounding_pitch(other_voice, beat) {
            let iv = interval::semitones(other_pitch, proposed_pitch);

            // Basic consonance/dissonance
            if interval::is_consonant(iv) {
                score += Score::from_int(2);
            } else {
                score -= Score::from_ratio(3, 2); // 1.5
            }

            let is_strong = beat.is_multiple_of(4);
            if is_strong && interval::is_perfect_consonance(iv) {
                score += Score::ONE;
            }

            // Use trained harmonic model for finer preference
            let iv_clamped = iv.clamp(-24, 24) as i8;
            if let Some(&weight) = models.harmonic.unigram.get(&iv_clamped) {
                let total: u64 = models.harmonic.unigram.values().map(|&w| w as u64).sum();
                if total > 0 {
                    // Normalize and scale by 3: weight * 3 / total
                    score += Score::from_ratio(weight as i64 * 3, total as i64);
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
                        let total: u64 = table.values().map(|&w| w as u64).sum();
                        if total > 0 {
                            score += Score::from_ratio(weight as i64 * 2, total as i64);
                        }
                    }
                }
            }

            let abs_iv = iv.unsigned_abs();
            if abs_iv > 24 {
                score -= Score::ONE;
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
                            score -= Score::from_int(10);
                        }
                        // Hidden (direct) 5ths/octaves — lighter penalty
                        if (curr_ic == 7 && prev_ic != 7) || (curr_ic == 0 && prev_ic != 0) {
                            score -= Score::from_int(3);
                        }
                    }
                }
            }
        }
    }

    // Voice crossing: penalize if this pitch crosses another active voice
    for &other_voice in grid.active_voices() {
        if other_voice == voice {
            continue;
        }
        if let Some(other_pitch) = grid.sounding_pitch(other_voice, beat) {
            // Higher-numbered voice should have lower pitch
            if voice.index() < other_voice.index() && proposed_pitch < other_pitch {
                score -= Score::from_int(5);
            }
            if voice.index() > other_voice.index() && proposed_pitch > other_pitch {
                score -= Score::from_int(5);
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
    let fifth_pc = (final_pc + 7) % 12;
    let third_pc = (final_pc + mode.mode.intervals()[2]) % 12;

    let cadence_start = n.saturating_sub(6);
    let penult_beat = cadence_start;
    let final_beat = cadence_start + 2;
    let hold = n - final_beat - 1;

    // Place cadence for each active voice with voice-appropriate pitches
    let active = grid.active_voices().to_vec();
    for voice in &active {
        let (low, high) = voice.range();
        let mid = (low + high) / 2;
        let voice_final = nearest_pc_in_range(final_pc, mid, low, high);

        let (penult_pitch, final_pitch) = match voice {
            Voice::Soprano => {
                let penult = mode.snap_to_mode(voice_final + 2);
                (penult, voice_final)
            }
            Voice::Alto => {
                let alto_final = nearest_pc_in_range(fifth_pc, voice_final, low, high);
                let alto_penult = nearest_pc_in_range(third_pc, alto_final, low, high);
                (alto_penult, alto_final)
            }
            Voice::Tenor => {
                let tenor_final = nearest_pc_in_range(final_pc, voice_final, low, high);
                let tenor_penult = nearest_pc_in_range(fifth_pc, tenor_final, low, high);
                (tenor_penult, tenor_final)
            }
            Voice::Bass => {
                let bass_fifth = nearest_pc_in_range(fifth_pc, voice_final, low, high);
                (bass_fifth, voice_final)
            }
        };

        place_cadence_note(grid, *voice, penult_beat, penult_pitch, 2, structural_cells);
        place_cadence_note(
            grid,
            *voice,
            final_beat,
            final_pitch,
            hold,
            structural_cells,
        );
    }
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
        let mut rng = GameRng::new(42);

        let plan = generate_structure(&library, 2, None, &mut rng);
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
        let mut rng = GameRng::new(42);

        let plan = generate_structure(&library, 2, None, &mut rng);
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
        let mut rng = GameRng::new(42);

        let plan = generate_structure(&library, 2, None, &mut rng);
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
        let pct = if total > 0 { in_mode * 100 / total } else { 0 };
        assert!(
            pct > 85,
            "At least 85% of draft notes should be in mode, got {pct}%",
        );
    }

    #[test]
    fn test_final_cadence_solo_soprano() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mode = ModeInstance::d_dorian();
        let mut rng = GameRng::new(42);

        let plan = crate::structure::generate_structure_for_voices(
            &library,
            2,
            None,
            &[crate::grid::Voice::Soprano],
            &mut rng,
        );
        let mut grid =
            crate::grid::Grid::new_with_voices(plan.total_beats, &[crate::grid::Voice::Soprano]);
        let mut structural = crate::structure::apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);
        generate_final_cadence(&mut grid, &mode, &mut structural);

        // Last beat should have soprano sounding on the mode final or 5th
        let last_beat = grid.num_beats - 1;
        if let Some(pitch) = grid.sounding_pitch(crate::grid::Voice::Soprano, last_beat) {
            let pc = pitch % 12;
            let final_pc = mode.final_pc;
            let fifth_pc = (final_pc + 7) % 12;
            assert!(
                pc == final_pc || pc == fifth_pc,
                "Solo cadence: soprano pitch {} (pc {}) should be final {} or 5th {}",
                pitch,
                pc,
                final_pc,
                fifth_pc
            );
        }
    }

    #[test]
    fn test_fill_draft_duet_only_active_voices() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mode = ModeInstance::d_dorian();
        let mut rng = GameRng::new(42);

        let plan = crate::structure::generate_structure_for_voices(
            &library,
            2,
            None,
            &[crate::grid::Voice::Soprano, crate::grid::Voice::Alto],
            &mut rng,
        );
        let mut grid = crate::grid::Grid::new_with_voices(
            plan.total_beats,
            &[crate::grid::Voice::Soprano, crate::grid::Voice::Alto],
        );
        let structural = crate::structure::apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        // Inactive voices should have empty rows
        assert!(grid.voices[crate::grid::Voice::Tenor.index()].is_empty());
        assert!(grid.voices[crate::grid::Voice::Bass.index()].is_empty());

        // Active voices should have notes
        let mut note_count = 0;
        for &voice in grid.active_voices() {
            for beat in 0..grid.num_beats {
                if !grid.cell(voice, beat).is_rest {
                    note_count += 1;
                }
            }
        }
        assert!(
            note_count > 10,
            "Duet draft should have notes, got {note_count}"
        );
    }
}
