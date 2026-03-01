// Text-to-grid mapping: assigns Vaelith syllables to grid cells.
//
// After structure.rs places motif entries and draft.rs fills free cells,
// this module assigns text syllables to the note attacks. Each syllable
// occupies one or more contiguous grid cells, with its tonal contour
// (level, rising, falling, dipping, peaking) constraining pitch movement
// within that span.
//
// The mapping works per voice entry: for each imitation point, each voice
// entry's attack cells receive syllables from the associated phrase.
// Since imitation reuses the same text, the same phrase is mapped to each
// voice entry of a given section, just at different start times.
//
// The output (TextMapping with SyllableSpans) is consumed by scoring.rs
// for tonal contour constraint evaluation.

use crate::grid::{Grid, Voice};
use crate::structure::StructurePlan;
use crate::vaelith::{Tone, VaelithPhrase};

/// A syllable assigned to a contiguous span of grid cells in one voice.
#[derive(Debug, Clone)]
pub struct SyllableSpan {
    /// Which voice this span belongs to.
    pub voice: Voice,
    /// First beat of this syllable (inclusive).
    pub start_beat: usize,
    /// Last beat of this syllable (inclusive).
    pub end_beat: usize,
    /// The tonal contour required within this span.
    pub tone: Tone,
    /// The syllable text (for display/debugging).
    pub text: String,
    /// Whether this syllable is stressed (should land on a strong beat).
    pub stressed: bool,
    /// Global syllable ID (matches syllable_id in grid cells).
    pub syllable_id: u16,
}

/// Complete text-to-music mapping for a piece.
#[derive(Debug, Clone)]
pub struct TextMapping {
    /// The phrase chosen for each section.
    pub section_phrases: Vec<VaelithPhrase>,
    /// All syllable spans across all voices.
    pub spans: Vec<SyllableSpan>,
}

/// Apply text mapping to a grid, assigning syllables to note attacks.
///
/// For each section in the structure plan, takes the first candidate phrase
/// (index 0) and maps its syllables to the attack cells within each voice
/// entry. Sets syllable_onset and syllable_id on the grid cells.
///
/// Returns a TextMapping containing all syllable span records for use
/// in tonal contour scoring.
pub fn apply_text_mapping(
    grid: &mut Grid,
    plan: &StructurePlan,
    phrase_candidates: &[Vec<VaelithPhrase>],
) -> TextMapping {
    let mut spans = Vec::new();
    let mut section_phrases = Vec::new();
    let mut next_syllable_id: u16 = 1; // 0 = unmapped

    for (section_idx, point) in plan.imitation_points.iter().enumerate() {
        // Pick the first candidate phrase for this section
        let phrase = if section_idx < phrase_candidates.len()
            && !phrase_candidates[section_idx].is_empty()
        {
            phrase_candidates[section_idx][0].clone()
        } else {
            continue;
        };

        section_phrases.push(phrase.clone());

        // Map the phrase to each voice entry in this imitation point
        for entry in &point.entries {
            let voice = entry.voice;
            let entry_start = entry.start_beat;

            // Find all attack cells for this voice starting from entry_start,
            // up to the next section's start or the end of the piece
            let entry_end = find_entry_end(grid, voice, entry_start, plan, section_idx);
            let attacks = collect_attacks(grid, voice, entry_start, entry_end);

            if attacks.is_empty() {
                continue;
            }

            // Assign syllables to attacks
            let syllable_spans =
                assign_syllables_to_attacks(&phrase, &attacks, voice, grid, &mut next_syllable_id);

            // Apply to grid
            for span in &syllable_spans {
                // Mark syllable onset
                let cell = grid.cell_mut(span.voice, span.start_beat);
                cell.syllable_onset = true;
                cell.syllable_id = span.syllable_id;

                // Mark continuation cells with same syllable_id
                for beat in (span.start_beat + 1)..=span.end_beat {
                    if beat < grid.num_beats {
                        let cell = grid.cell_mut(span.voice, beat);
                        cell.syllable_id = span.syllable_id;
                    }
                }
            }

            spans.extend(syllable_spans);
        }
    }

    TextMapping {
        section_phrases,
        spans,
    }
}

/// Find the end beat for a voice entry (exclusive).
/// An entry extends until the next section's first entry for this voice,
/// or the end of the piece.
fn find_entry_end(
    grid: &Grid,
    voice: Voice,
    entry_start: usize,
    plan: &StructurePlan,
    section_idx: usize,
) -> usize {
    // Look at the next section's entries for this voice
    for next_section in &plan.imitation_points[(section_idx + 1)..] {
        for next_entry in &next_section.entries {
            if next_entry.voice == voice && next_entry.start_beat > entry_start {
                return next_entry.start_beat;
            }
        }
    }
    grid.num_beats
}

/// Collect all attack beat positions for a voice within a range.
fn collect_attacks(grid: &Grid, voice: Voice, start: usize, end: usize) -> Vec<usize> {
    let mut attacks = Vec::new();
    let end = end.min(grid.num_beats);
    for beat in start..end {
        let cell = grid.cell(voice, beat);
        if cell.attack && !cell.is_rest {
            attacks.push(beat);
        }
    }
    attacks
}

/// Assign syllables from a phrase to a sequence of attack beats.
///
/// Each syllable gets at least `tone.min_notes()` attacks. If there are
/// more attacks than syllables need, extra attacks are distributed to
/// create melismatic extensions (extra notes on the last syllable or
/// distributed evenly).
fn assign_syllables_to_attacks(
    phrase: &VaelithPhrase,
    attacks: &[usize],
    voice: Voice,
    grid: &Grid,
    next_id: &mut u16,
) -> Vec<SyllableSpan> {
    let mut spans = Vec::new();
    let syllables = &phrase.syllables;

    if syllables.is_empty() || attacks.is_empty() {
        return spans;
    }

    let total_attacks = attacks.len();
    let num_syllables = syllables.len();

    // Calculate minimum attacks needed
    let min_needed: usize = syllables.iter().map(|s| s.tone.min_notes()).sum();

    // If we have fewer attacks than syllables, assign one attack per syllable
    // as many as we can
    if total_attacks <= num_syllables {
        for (i, attack_beat) in attacks.iter().enumerate() {
            if i >= num_syllables {
                break;
            }
            let syl = &syllables[i];
            let end_beat = find_syllable_end(grid, voice, *attack_beat, attacks.get(i + 1));
            let id = *next_id;
            *next_id = next_id.wrapping_add(1);

            spans.push(SyllableSpan {
                voice,
                start_beat: *attack_beat,
                end_beat,
                tone: syl.tone,
                text: syl.text.clone(),
                stressed: syl.stressed,
                syllable_id: id,
            });
        }
        return spans;
    }

    // Distribute attacks to syllables, respecting min_notes per tone
    let mut attack_idx = 0;
    let extra = total_attacks.saturating_sub(min_needed);

    for (syl_idx, syl) in syllables.iter().enumerate() {
        if attack_idx >= total_attacks {
            break;
        }

        let start_attack = attack_idx;
        let min = syl.tone.min_notes();

        // How many attacks this syllable gets
        let alloc = if syl_idx == num_syllables - 1 {
            // Last syllable gets all remaining
            total_attacks - attack_idx
        } else if extra > 0 && syl.stressed {
            // Stressed syllables get extra attacks (melismatic)
            min + (extra / num_syllables)
                .max(1)
                .min(total_attacks - attack_idx - 1)
        } else {
            min.min(total_attacks - attack_idx)
        };

        let end_attack_idx = (start_attack + alloc).min(total_attacks);
        let first_beat = attacks[start_attack];
        let last_beat_attack = attacks[end_attack_idx - 1];

        // The syllable spans from its first attack to just before the next syllable's first attack
        let end_beat =
            find_syllable_end(grid, voice, last_beat_attack, attacks.get(end_attack_idx));

        let id = *next_id;
        *next_id = next_id.wrapping_add(1);

        spans.push(SyllableSpan {
            voice,
            start_beat: first_beat,
            end_beat,
            tone: syl.tone,
            text: syl.text.clone(),
            stressed: syl.stressed,
            syllable_id: id,
        });

        attack_idx = end_attack_idx;
    }

    spans
}

/// Find the end beat of a syllable span: extends from the attack through
/// all continuation cells until the next attack/rest or next syllable start.
fn find_syllable_end(
    grid: &Grid,
    voice: Voice,
    attack_beat: usize,
    next_attack: Option<&usize>,
) -> usize {
    let limit = next_attack.copied().unwrap_or(grid.num_beats);
    let mut end = attack_beat;
    for beat in (attack_beat + 1)..limit {
        if beat >= grid.num_beats {
            break;
        }
        let cell = grid.cell(voice, beat);
        if cell.is_rest || cell.attack {
            break;
        }
        end = beat;
    }
    end
}

/// Swap the phrase for a section, updating the grid's syllable assignments.
///
/// This is the "text-swap macro mutation" for SA: replace a section's phrase
/// with a different candidate, re-map syllables, and let the scoring function
/// evaluate the new tonal constraints.
pub fn swap_section_phrase(
    grid: &mut Grid,
    mapping: &mut TextMapping,
    plan: &StructurePlan,
    section_idx: usize,
    new_phrase: &VaelithPhrase,
) {
    // Clear old syllable data for this section
    let old_spans: Vec<SyllableSpan> = mapping
        .spans
        .iter()
        .filter(|s| {
            // Check if this span belongs to the target section by checking
            // if its start_beat falls within any voice entry of that section
            if section_idx >= plan.imitation_points.len() {
                return false;
            }
            let point = &plan.imitation_points[section_idx];
            point
                .entries
                .iter()
                .any(|e| e.voice == s.voice && s.start_beat >= e.start_beat)
        })
        .cloned()
        .collect();

    // Clear grid cells for old spans
    for span in &old_spans {
        for beat in span.start_beat..=span.end_beat {
            if beat < grid.num_beats {
                let cell = grid.cell_mut(span.voice, beat);
                cell.syllable_onset = false;
                cell.syllable_id = 0;
            }
        }
    }

    // Remove old spans from mapping
    mapping
        .spans
        .retain(|s| !old_spans.iter().any(|o| o.syllable_id == s.syllable_id));

    // Find the next available syllable_id
    let mut next_id = mapping
        .spans
        .iter()
        .map(|s| s.syllable_id)
        .max()
        .unwrap_or(0)
        + 1;

    // Re-map with new phrase
    if section_idx < plan.imitation_points.len() {
        let point = &plan.imitation_points[section_idx];
        for entry in &point.entries {
            let voice = entry.voice;
            let entry_start = entry.start_beat;
            let entry_end = find_entry_end(grid, voice, entry_start, plan, section_idx);
            let attacks = collect_attacks(grid, voice, entry_start, entry_end);

            if attacks.is_empty() {
                continue;
            }

            let new_spans =
                assign_syllables_to_attacks(new_phrase, &attacks, voice, grid, &mut next_id);

            for span in &new_spans {
                let cell = grid.cell_mut(span.voice, span.start_beat);
                cell.syllable_onset = true;
                cell.syllable_id = span.syllable_id;

                for beat in (span.start_beat + 1)..=span.end_beat {
                    if beat < grid.num_beats {
                        let cell = grid.cell_mut(span.voice, beat);
                        cell.syllable_id = span.syllable_id;
                    }
                }
            }

            mapping.spans.extend(new_spans);
        }
    }

    // Update section phrase
    if section_idx < mapping.section_phrases.len() {
        mapping.section_phrases[section_idx] = new_phrase.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::fill_draft;
    use crate::markov::{MarkovModels, MotifLibrary};
    use crate::mode::ModeInstance;
    use crate::structure::{apply_structure, generate_structure};
    use crate::vaelith::generate_phrases;
    use elven_canopy_prng::GameRng;

    #[test]
    fn test_apply_text_mapping() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mode = ModeInstance::d_dorian();
        let mut rng = GameRng::new(42);

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        let phrases = generate_phrases(2, &mut rng);
        let mapping = apply_text_mapping(&mut grid, &plan, &phrases);

        // Should have mapped some syllable spans
        assert!(
            !mapping.spans.is_empty(),
            "Should have at least one syllable span"
        );
        assert_eq!(
            mapping.section_phrases.len(),
            2,
            "Should have 2 section phrases"
        );

        // Every span should have a valid beat range
        for span in &mapping.spans {
            assert!(
                span.start_beat <= span.end_beat,
                "Span start {} should be <= end {}",
                span.start_beat,
                span.end_beat
            );
            assert!(
                span.end_beat < grid.num_beats,
                "Span end {} should be < grid size {}",
                span.end_beat,
                grid.num_beats
            );
        }
    }

    #[test]
    fn test_syllable_onsets_set_on_grid() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mode = ModeInstance::d_dorian();
        let mut rng = GameRng::new(42);

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        let phrases = generate_phrases(2, &mut rng);
        let mapping = apply_text_mapping(&mut grid, &plan, &phrases);

        // Count syllable onsets in the grid
        let mut onset_count = 0;
        for voice in Voice::ALL {
            for beat in 0..grid.num_beats {
                if grid.cell(voice, beat).syllable_onset {
                    onset_count += 1;
                }
            }
        }

        // Should match the number of spans
        assert_eq!(
            onset_count,
            mapping.spans.len(),
            "Grid onset count ({}) should match span count ({})",
            onset_count,
            mapping.spans.len()
        );
    }

    #[test]
    fn test_swap_section_phrase() {
        let models = MarkovModels::default_models();
        let library = MotifLibrary::default_library();
        let mode = ModeInstance::d_dorian();
        let mut rng = GameRng::new(42);

        let plan = generate_structure(&library, 2, &mut rng);
        let mut grid = Grid::new(plan.total_beats);
        let structural = apply_structure(&mut grid, &plan);
        fill_draft(&mut grid, &models, &structural, &mode, &mut rng);

        let phrases = generate_phrases(2, &mut rng);
        let mut mapping = apply_text_mapping(&mut grid, &plan, &phrases);

        // Generate a new phrase to swap in
        let new_phrase = crate::vaelith::generate_single_phrase(&mut rng);
        let _old_span_count = mapping.spans.len();

        swap_section_phrase(&mut grid, &mut mapping, &plan, 0, &new_phrase);

        // Should still have spans (possibly different count)
        assert!(
            !mapping.spans.is_empty(),
            "Should still have spans after swap"
        );
        // The phrase should have been updated
        assert_eq!(mapping.section_phrases[0].text, new_phrase.text);

        // Verify grid consistency: all onset cells should have syllable_id > 0
        for voice in Voice::ALL {
            for beat in 0..grid.num_beats {
                let cell = grid.cell(voice, beat);
                if cell.syllable_onset {
                    assert!(
                        cell.syllable_id > 0,
                        "Onset at {:?} beat {} should have syllable_id > 0",
                        voice,
                        beat
                    );
                }
            }
        }
    }
}
