# Palestrina-Style Polyphonic Music Generator v2 — Design Document

## 1. Project Goal

Build a program (primarily Rust, with Python for ML training and corpus analysis) that generates original choral music in the style of Renaissance polyphony (Palestrina), with procedurally generated lyrics in the **Vaelith** tonal elvish conlang. For use in the Elven Canopy game.

The system should produce music that is:
- **Rule-compliant** — follows counterpoint conventions
- **Aesthetically pleasing** — with a learned preference model tuned to the user's taste
- **Linguistically valid** — lyrics in grammatically correct Vaelith with tonal constraints realized in the melody
- **Synthesizable** — output feeds a concatenative vocal synthesis engine

---

## 2. Target Style: Key Properties

Palestrina-style polyphony:

- **4 independent voices** (SATB), each a real melodic line
- **Smooth, stepwise motion** — mostly seconds, occasional thirds/fourths, rare larger leaps recovered by contrary stepwise motion
- **Rhythmic independence** between voices — staggered entries, overlapping phrases
- **Points of imitation** — the primary structural device; a motif stated in one voice, echoed in others at time offsets, typically transposed to the 5th or octave
- **Strict consonance on strong beats** — dissonance only via properly prepared and resolved suspensions
- **Modal** (Dorian, Phrygian, Mixolydian, etc.), not tonal major/minor
- **A cappella** — unaccompanied voices
- **Text-driven phrase structure** — each text phrase becomes a point of imitation; heavy repetition through imitative entries means a few sentences fill a 3–5 minute motet

---

## 3. High-Level Architecture

A **multi-stage pipeline**: corpus analysis → text generation → structure generation → draft generation → refinement → vocal synthesis.

### Why Hybrid

- Pure SA from random state: enormous search space, most states musically meaningless — too slow
- Pure constraint satisfaction: "legal but boring" — no aesthetic quality
- Pure Markov generation: meanders without global structure — "pleasant mush"
- The hybrid lets each method handle what it's good at

### Pipeline Overview

```
┌──────────────────────────────────────────────────────────────────┐
│ OFFLINE / TRAINING PHASE (Python)                                │
│                                                                  │
│  Palestrina Corpus ──► Markov Model Training                     │
│         │            ──► Motif Library Extraction                 │
│         │            ──► Preference Model Training                │
│         ▼                        │                               │
│    music21 parsing         ONNX / weights export                 │
│                                                                  │
│  Vaelith Language Data ──► Grammar engine tables                  │
│                          ──► Vocabulary + tone data               │
│                          ──► Suffix/morpheme tone tables          │
└──────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────┐
│ GENERATION PHASE (Rust)                                          │
│                                                                  │
│  1. Text Generation                                              │
│     Vaelith grammar engine → candidate phrases with tone maps    │
│                        │                                         │
│  2. Structure Generation                                         │
│     Select/generate motifs, plan imitative entries,              │
│     assign text phrases to sections, plan call-and-response      │
│                        │                                         │
│  3. Draft Generation                                             │
│     Fill grid from structure plan + Markov sampling              │
│     + tonal contour constraints from text                        │
│                        │                                         │
│  4. SA Refinement                                                │
│     Markov-guided mutations (notes AND word choices),            │
│     rule-based + tonal + learned scoring                         │
│                        │                                         │
│  5. Output                                                       │
│     Grid → MIDI (for evaluation)                                 │
│     Grid + text → vocal synthesis engine                         │
└──────────────────────────────────────────────────────────────────┘
```

---

## 4. Representation: The Grid

The score is a 2D grid:

- **Rows** = voices (4 rows for SATB)
- **Columns** = beats at eighth-note granularity
- **Cell values** = pitch (MIDI number) or silence, **plus**:
  - **Attack flag** — new attack vs. continuation of a held note (critical for dissonance rules — a held note can form a suspension, a new attack must be consonant on strong beats)
  - **Syllable onset flag** — marks cells where a new syllable of text begins
  - **Syllable ID** — which syllable from the text is being sung at this cell (needed for tonal constraint lookup and synthesis)

### Why a Grid

A grid is easier to reason about for SA mutations, scoring, and the preference model. It's a fixed-size tensor-like structure. MIDI's variable-length events are harder to index into and mutate. Conversion to/from MIDI for listening is straightforward.

### Syllable-to-Grid Mapping

Each syllable occupies a contiguous span of cells in a voice row. The span's length is:
- **Minimum:** determined by the syllable's tone (level ≥ 1 cell, rising/falling ≥ 2, dipping/peaking ≥ 3)
- **Maximum:** unconstrained (melismatic extension is fine; extra notes beyond the tone minimum are free)

The tone constraint applies to the **pitch contour within the syllable's span** (see §11).

---

## 5. Corpus Analysis (Python, Offline)

### 5.1 Data Source

Primary: **music21's built-in corpus** — Palestrina masses and Renaissance polyphony already parsed into Python objects. Supplemented by:

- MusicXML files from CPDL (cpdl.org)
- Kern-format files from Humdrum datasets
- MIDI files (lower quality, last resort)

music21 handles all parsing — key signatures, voice extraction, duration calculation, transposition.

### 5.2 Markov Model

A **factored model** combining:

#### Per-Voice Melodic Model
- **Interval-based** (not absolute pitch) — transposition-invariant melodic tendencies
- **2nd or 3rd order with Katz backoff** — conditioned on previous 2–3 intervals, falling back to lower orders when data is sparse
- **Conditioned on metric position** (strong/weak beat)

Interval-based encoding reduces state space: ~25 possible intervals (±octave) vs. 30+ absolute pitches, making 3rd-order models feasible from 30–50 pieces.

#### Per-Voice-Pair Harmonic Model
- 1st or 2nd order — P(interval between voices A,B at beat N | interval at beat N-1)
- Captures voice-leading conventions between specific pairs

#### Why Factored
Joint 4-voice model = ~20⁴ ≈ 160,000 states per beat. 2nd-order = ~25 billion entries — impossibly sparse. Factoring into melodic + harmonic models is an approximation but a practical one.

#### Additional Conditioning Variables
- **Metric position** (strong/weak beat)
- **Distance from voice's recent high/low point** (contour tendency)
- **Position in phrase** (near cadence vs. mid-phrase)

### 5.3 Motif Library Extraction

**Method: Frequency-based interval n-gram mining.**

1. Convert all voices in all pieces to interval sequences
2. Extract all n-grams of length 4–10
3. Count frequency across the corpus, filtering for n-grams appearing in **multiple pieces** (captures stylistic vocabulary, not piece-specific themes)
4. Store as a ranked library:
   - Interval sequence
   - Typical starting pitch relative to mode
   - Typical entry offsets between voices (e.g., 2nd voice enters 4–6 beats later)
   - Typical transposition intervals for imitation (5th, octave, unison)

**Why frequency-based n-grams:** Simple, directly yields a ranked library, output is interpretable. More sophisticated methods (suffix trees, approximate matching) can be added later.

**Motif boundary heuristic:** Motifs often start after a rest and on a strong beat.

---

## 6. Text Generation: The Vaelith Grammar Engine (Rust)

The grammar engine generates candidate text phrases for the music system. It operates as a constrained generator: given semantic parameters (topic, register, mood), it produces grammatically valid Vaelith phrases along with their **complete tone maps** (the tone of every syllable, fully determined by the lexicon).

### 6.1 Input: Semantic Parameters

- **Theme/topic:** from a set of poem templates or procedural themes (invocation, lament, praise, etc.)
- **Register:** sacred, narrative, communal
- **Target syllable count range:** 3–8 per phrase (one phrase = one point of imitation)
- **Vowel class preference:** front (bright) or back (dark) — controls timbral color

### 6.2 Output: Phrase + Tone Map

For each generated phrase, the engine produces:

```
Phrase: "Thír-ri shine-thir dai"
Syllables: [thír, ri, shi, ne, thir, dai]
Tones: [rising, level, level, level, level, level]
Min notes: [2, 1, 1, 1, 1, 1]
Stressed: [true, false, false, false, false, false]
```

Every morpheme in Vaelith has a lexically specified, fixed tone. There are no relaxation rules — the tone map is always deterministic from the text. This makes constraint checking trivial: look up the syllable, get its tone, enforce it.

### 6.3 Flexibility Points for Joint Optimization

The grammar engine can generate **multiple candidate phrases** expressing similar meanings, differing in:

1. **Synonyms** — different words with different tone patterns
2. **Word order** — SOV, OSV, VSO, OVS all grammatical (case marking disambiguates). Different orders place different tones at different metric positions
3. **Case frames** — dative (-se/-so) vs. locative (-mi/-mu) for similar meanings → different suffix sounds
4. **Aspect choice** — eternal (-thir/-thur) vs. habitual (-tha) vs. imperfective (-ren/-ran) when multiple aspects are semantically valid → different suffix tone/sound
5. **Active vs. middle voice** — middle (en-/an-) drops the agent, saving a word and case suffix
6. **Ablaut exploitation** — perfective stems introduce diphthongs for denser, more archaic sound
7. **Inclusive vs. exclusive "we"** — náire (communal, inviting) vs. náli (solemn, exclusive)
8. **Poetic elision** — drop case suffixes, evidentials, pronouns when recoverable → 1–2 fewer syllables

The SA optimizer can swap between these candidates during refinement (§10.1, macro mutations).

### 6.4 Stress and Metric Placement

**Vaelith stress rule: First syllable of root, always. Suffixes never stressed.**

The music generator should place stressed syllables on strong beats. Since stress is always root-initial and suffix-invariant, the generator always knows where to place metric emphasis, regardless of how many suffixes are stacked.

---

## 7. Structure Generation (Rust)

Before filling in individual notes, the system plans high-level structure:

### 7.1 Text-to-Music Assignment

Each clause or short phrase (3–8 syllables) from the Vaelith text becomes one **point of imitation** — heard once per voice in staggered entries. A three-stanza hymn with ~4 lines per stanza yields ~12 points of imitation, filling a 3–5 minute motet.

### 7.2 Call-and-Response Structure

Vaelith liturgical music uses two responsorial markers:

**Dai** (level tone, "truly / so it is") — short homophonic affirmation. All voices attack together on a consonance. Set between points of imitation as rhythmic punctuation.

**Thol** (level tone, "eternal / so be it") — sustained polyphonic chord. Voices enter one at a time, building a resonant cluster over 4–8 beats. Set at major section boundaries as structural breathing.

The structure generator places *dai* and *thol* markers at appropriate positions, creating a large-scale form of alternating polyphony and homophony.

### 7.3 Motif Planning

1. **Select motifs** — draw from extracted library (~50%) or generate novel ones via Markov model (~50%). Library motifs may receive slight random mutations.
2. **Plan imitative entries** — for each motif:
   - Which voices participate
   - Time offset for each voice's entry (from corpus statistics)
   - Transposition interval (typically 5th or octave)
3. **Plan section sequence** — how many points of imitation, approximate duration, transitions

This creates a partially-filled grid: motif cells are fixed/constrained, connective tissue cells are free.

### Why Structure First

Without explicit structure, all downstream methods produce output that is locally correct but globally aimless. Points of imitation give Palestrina's music its sense of intentionality.

---

## 8. Draft Generation (Rust)

Fill the free cells using the Markov model, now also subject to tonal constraints:

1. For each voice, iterate through beats sequentially
2. At each free cell, sample a pitch from the **melodic Markov model** (conditioned on previous intervals)
3. Weight by the **harmonic Markov model** (conditioned on other voices at that beat)
4. **Filter by tonal constraint:** if this cell is within a syllable span that requires a specific pitch contour (rising, falling, etc.), reject proposals that violate it
5. Accept the most probable valid pitch

The draft should be "mostly reasonable" — stylistically plausible, generally consonant, tonal constraints mostly satisfied, but with rule violations and no aesthetic polish.

---

## 9. Scoring Function

The scoring function evaluates the quality of a complete (or partially filled) grid. It is a weighted sum of penalty/reward terms, structured in layers.

### Layer 1: Hard Counterpoint Rules (High Weight)

Pass/fail rules. Heavy penalties ensure they dominate.

| Rule | Description |
|------|-------------|
| Parallel 5ths/octaves | Two voices in parallel motion to a perfect 5th or octave — the cardinal sin |
| Direct/hidden 5ths/octaves | Both voices moving in same direction into a perfect consonance |
| Strong-beat dissonance | Dissonant intervals on strong beats, unless properly prepared as suspensions |
| Suspension violations | Must be prepared (held from consonance) and resolved (step down) |
| Voice crossing | A lower voice going above a higher one |
| Range violations | Any voice exceeding ~octave + third total range |

### Layer 2: Soft Melodic Preferences (Medium Weight)

| Preference | Description |
|------------|-------------|
| Stepwise motion | Reward 2nds; mild penalty for 3rds/4ths; heavy penalty for larger leaps |
| Leap recovery | After a leap, penalize failure to step back in opposite direction |
| Repeated notes | Mild penalty for too many consecutive repeated pitches |
| Direction variety | Penalize runs of >4–5 notes in one direction |
| Contour shape | Reward arch-shaped melodic contours |
| Climax uniqueness | Voice's highest note should occur once or very few times, at a structurally significant moment |

### Layer 3: Harmonic Preferences (Medium Weight)

| Preference | Description |
|------------|-------------|
| Consonance on strong beats | Reward 3rds, 6ths, 5ths, octaves |
| Voice spacing | Penalize gaps >octave between adjacent voices (bass excepted) |
| Interval variety | Penalize overuse of any single interval between voice pairs |
| Cadence placement | Reward proper cadential formulas at phrase boundaries |

### Layer 4: Global/Structural (Medium Weight)

| Preference | Description |
|------------|-------------|
| Opening/closing | Start and end on perfect consonance; final cadence by contrary stepwise motion |
| Rhythmic independence | Penalize all voices moving in same rhythm for extended passages |

### Layer 5: Tonal Contour Constraints (High Weight)

This layer enforces the Vaelith tone system. Each syllable has a fixed tone specifying the required pitch contour within its span:

| Tone | Constraint on pitch(es) within the syllable span | Min notes |
|------|--------------------------------------------------|-----------|
| Level | Held steady — **no pitch change** within the syllable | 1 |
| Rising | Pitch must ascend from first note to last note in the span | 2 |
| Falling | Pitch must descend from first note to last note | 2 |
| Dipping | Pitch must descend then ascend (valley shape) | 3 |
| Peaking | Pitch must ascend then descend (hill shape) | 3 |

**Key property:** Tones constrain what happens *inside* a syllable. The transition *between* syllables (end of one to start of next) is governed by counterpoint rules (Layers 1–4), not by the tone system. This separation keeps the two constraint systems from fighting each other.

**Constraint density (from Vaelith v4 analysis):** ~60–70% of syllables in a typical phrase are level-toned (no internal pitch constraint). Of the remaining 30–40% with directional tones, each constrains only the notes within that syllable — not what happens before or after. **Approximately 20–30% of inter-syllable transitions carry a strong directional interaction** (e.g., a rising syllable's high endpoint leading into a falling syllable's high start). This is a manageable constraint density for the counterpoint optimizer.

**Cadence alignment:** Vaelith's SOV word order means most phrases end with a verb. Most verbs have level-toned roots, and verb suffixes are mostly level. The phrase-final falling sandhi tendency adds gentle descent. Result: phrase endings naturally align with Western cadence patterns.

### Layer 6: Ensemble Texture (Low Weight)

| Preference | Description |
|------------|-------------|
| Fricative density | Penalize all voices hitting dense fricatives (sh, s, f) simultaneously — causes muddy hissing |
| Open vowel alignment | Reward moments where multiple voices sustain open vowels (a, o, e) — resonant bloom points |

These are soft preferences that improve the *sonic* quality of the overlapping text, not just the musical quality.

### Layer 7: Learned Aesthetic Preferences (Variable Weight)

See Section 12 below.

### Scoring Implementation Notes

- **Locality:** Each beat's score depends on a window of ~2–3 beats. Allows efficient incremental rescoring after a mutation.
- **Normalization:** Ensure degenerate solutions (all unisons, all silence) don't score well.
- **Weight tuning:** Hard rules at 10–100x the weight of soft preferences. Tonal constraints at high weight (similar to hard rules — they're part of the language's identity). Adjust empirically.

---

## 10. SA Refinement (Rust)

Simulated annealing refines the draft using the composite scoring function and Markov-guided mutations.

### 10.1 Mutation Operators

**Micro mutations (applied to free/non-structural cells):**
- **Change a single note's pitch** — proposed pitch sampled from the Markov model (Metropolis-Hastings style), not uniform random. Stylistically informed proposals dramatically improve acceptance rate.
- **Extend or shorten a note's duration** — shift the attack/continuation boundary within a syllable span.

**Macro mutations (applied to structural elements, at lower rate):**
- **Mutate a motif** — change one interval within a motif template; propagated to all linked instances with appropriate transposition.
- **Shift a motif entry's time offset** by ±1–2 beats.
- **Change a motif entry's transposition interval.**
- **Swap a text phrase** — replace a Vaelith phrase with an alternative candidate (different synonym, word order, case frame, aspect, voice) generated by the grammar engine (§6.3). This changes the tone map for that phrase, which changes the tonal constraints on the associated grid cells, which may require micro mutations to satisfy. This is the key mechanism for **joint text-music optimization.**

### 10.2 The Repetition Problem

Random pointwise mutations destroy repetitive structure. Solution: **hierarchical representation.**

- Motif instances across voices are **linked** — mutating the motif template propagates to all instances (with transposition)
- SA operates on two levels: macro mutations reshape structure, micro mutations polish connective tissue
- Non-structural cells are the primary target for micro mutations

### 10.3 SA Schedule

Standard cooling schedule with restarts. Temperature starts high (accepting most mutations) and decreases (accepting only improvements). Periodic reheating prevents getting stuck in local optima. The schedule should be tuned empirically — convergence depends heavily on piece length and scoring function weights.

---

## 11. Tonal Contour Constraints — Detailed Specification

This section specifies exactly how Vaelith's tone system interacts with the music grid.

### 11.1 Tone Definitions (from Vaelith v4)

Every syllable in Vaelith has a lexically fixed tone. Five contour tones exist:

| Tone | Diacritic | Pitch Movement Within the Syllable | Min Notes |
|------|-----------|-------------------------------------|-----------|
| Level | (none) | Held steady — no pitch change | 1 |
| Rising | acute (á) | Ascends: first note lower than last | 2 |
| Falling | grave (à) | Descends: first note higher than last | 2 |
| Dipping | caron (ǎ) | Descends then ascends (valley) | 3 |
| Peaking | circumflex (â) | Ascends then descends (hill) | 3 |

**Why contour, not register:** "High" and "low" pitches are meaningless across voices at different pitch levels. A "high tone" in the soprano and "high tone" in the bass are completely different pitches. But "rising" in both means "go up" — preserved under transposition. Contour tones and polyphonic imitation are naturally compatible.

### 11.2 Tone Distribution

| Tone | Frequency in Root Syllables | Frequency in Suffixes/Particles |
|------|---------------------------|-------------------------------|
| Level | ~40% | ~85% |
| Rising | ~20% | ~10% (vocative, inceptive, imperative, question particle) |
| Falling | ~20–25% | ~3% (cessative) |
| Dipping | ~7% | ~0% |
| Peaking | ~7% | ~2% (intuited evidential) |

The heavy prevalence of level tones in suffixes/particles means **grammatical machinery is almost entirely musically unconstrained.** The meaningful, root syllables carry the tonal constraints.

### 11.3 Tone Realization in the Grid

When a syllable occupies cells [i, i+1, ..., i+k] in a voice row:

| Tone | Constraint on pitches at those cells |
|------|--------------------------------------|
| Level | pitch[i] = pitch[i+1] = ... = pitch[i+k] (all same) |
| Rising | pitch[i] < pitch[i+k]; intermediate notes monotonically non-decreasing (preferred) or at least net ascending |
| Falling | pitch[i] > pitch[i+k]; intermediate notes monotonically non-increasing (preferred) or at least net descending |
| Dipping | pitch[i] > some pitch[j] < pitch[i+k] where i < j < i+k; a valley |
| Peaking | pitch[i] < some pitch[j] > pitch[i+k] where i < j < i+k; a hill |

For level tone, "all same" means the held pitch doesn't bend. For multi-note melismas on level syllables, the pitch is literally held — this creates syllabic (one-note-per-syllable) texture, which is musically fine and common.

### 11.4 Between Syllables

The transition from the **last note of syllable N** to the **first note of syllable N+1** is governed by counterpoint rules, not tone rules. This is the key design decision that prevents the tone system and the counterpoint system from conflicting: each controls a different domain (within-syllable vs. between-syllable).

### 11.5 At Phrase Boundaries

No constraints span a rest or long held note. Phrase boundaries are "free" — the melody resets. The imitative structure (entries separated by rests) naturally creates windows of total freedom.

### 11.6 In Imitation

When a motif is transposed to a different voice, the **text phrase travels with it.** The same syllables, same tones, same contour constraints — just at a different absolute pitch level. Since contour tones are defined by direction (not absolute pitch), they are automatically satisfied by the transposition. This is the deepest reason contour tones were chosen: they survive imitative transposition unchanged.

### 11.7 Tones Drive Musical Texture

Non-level tones require multiple notes per syllable. This means **the tone pattern of the text directly controls rhythmic density:**

- Level-heavy passages (everyday vocabulary, suffixes) → syllabic, quick, light
- Rising/falling passages (content words, sacred roots) → moderately melismatic
- Dipping/peaking passages (archaic/sacred vocabulary) → ornately melismatic

Since dipping and peaking tones cluster in sacred and archaic vocabulary, **sacred text automatically generates more ornate singing.** The register distinction is built into the phonology.

### 11.8 Tone Sandhi (Simplification Rules)

These are natural simplification patterns, used sparingly by the optimizer as escape valves:

1. **Adjacent identical contours merge.** Two rising syllables may realize as one longer rise.
2. **Phrase-final falling tendency.** The last syllable of a phrase tends toward a gentle fall, aligning with cadence patterns.
3. **Emphatic override.** At a musical climax, any tone may be overridden. Rare — the optimizer's emergency valve.
4. **Dipping/peaking reduction.** In fast passages, dipping → just the low point (effectively falling), peaking → just the high point (effectively rising).

---

## 12. Learned Preference Model

### 12.1 Purpose

The rule-based scoring (Layers 1–6) produces "correct" music. The learned model adds a subjective "beauty" signal — what a specific listener finds pleasing beyond mere correctness.

### 12.2 Training Data Collection (Python)

**Pairwise comparison, not absolute ratings.** Listen to pairs of generated fragments (8–16 bars, ~15–30 seconds MIDI) and click which you prefer. Pairwise preferences are more reliable and consistent than absolute ratings.

**Estimated data requirement:**
- With engineered features: ~150–300 rated fragments in pairs
- Budget ~3–5 hours of focused listening across multiple sessions
- Use active learning: select pairs where the model is most uncertain

**Iterative collection:** Generate 50 pieces → rate → train preliminary model → use in SA → generate better pieces → rate → retrain. Each cycle improves both model and piece quality.

### 12.3 Input Features (Hand-Engineered)

Features computed per beat or per short window. Hand-engineering dramatically reduces data requirements — the model doesn't have to discover what intervals are, just which combinations humans prefer.

- Interval between each voice pair
- Consonance/dissonance classification of each vertical sonority
- Melodic interval (step/leap/direction) in each voice
- Whether parallel 5ths/octaves occur
- Local tension estimate (composite of dissonance, distance from modal center, rhythmic density)
- Entropy/predictability of recent melodic motion
- Repetition/imitation detection scores
- Tension curve shape (longer windows)
- **Tonal constraint satisfaction rate** (what % of tonal constraints are met — a meta-feature)
- **Vowel class distribution** (proportion of front vs. back vowels in current passage — timbral feature)

### 12.4 Model Architecture

**Start simple: logistic regression with Bradley-Terry ranking loss** on engineered features. May be sufficient. Trivially portable to Rust (dot product + sigmoid — export weights as JSON, ~10 lines of Rust inference).

**If more capacity needed: small 1D CNN.** Beats as timesteps, features as channels. A few conv layers → global pooling → scalar output. Captures temporal patterns (phrase shape, tension arcs) that flat logistic regression would miss.

**Export:** PyTorch → ONNX (`torch.onnx.export`) → Rust via `ort` crate (ONNX Runtime) or `tract` (pure Rust ONNX inference).

**Regularization:** Strong dropout and weight decay. Hold out 20%. Keep model small (thousands of parameters).

### 12.5 Additional Aesthetic Heuristics

- **Corpus log-likelihood** under Markov model. But don't optimize for max likelihood (produces blandness). Score for **optimal entropy range** — moderate information content.
- **Tension curve shape** — reward arcs of tension/release, build to climax around 2/3 through, strongest resolution at end. Parameterizable.
- **Melodic contour variety** — penalize predictable sawtooth patterns.
- **Interval distribution** — score against empirical distribution from preferred pieces.

### 12.6 User-Tunable Parameters

| Parameter | What It Controls |
|-----------|-----------------|
| Serenity ↔ Drama | Preferred tension level / tension curve shape |
| Conservative ↔ Chromatic | Harmonic adventurousness / dissonance tolerance |
| Simple ↔ Ornate | Melodic complexity / note density |
| Bright ↔ Dark | Bias toward front-class (e, i) vs. back-class (o, u) vocabulary — timbral color |
| Style reference corpus | Which pieces the Markov model is trained on |

---

## 13. Vowel Harmony and Timbral Control

Vaelith has vowel harmony: suffixes change their vowels to match the root's vowel class (front or back). This creates runs of similar-sounding vowels.

**Acoustic consequence:** Front vowels (e, i, ei, ai, ia) have a brighter, more silvery vocal timbre. Back vowels (o, u, au, oi) are darker and warmer. This is a formant difference preserved under pitch-shifting.

**For the optimizer:** The grammar engine can bias vocabulary selection toward front-class words (bright stanzas about starlight) or back-class words (dark stanzas about deep roots). This gives a subtle but real dimension of expressive timbral control. The user-tunable Bright ↔ Dark parameter controls this bias.

---

## 14. Vocal Synthesis

### 14.1 Approach: Concatenative Synthesis with Pitch Envelopes

Record a small set of syllable snippets. At playback, concatenate snippets according to the generated lyrics, applying pitch envelopes to realize both the melody and the tonal contours.

### 14.2 Recording Inventory (~132 snippets)

| Category | Count | Examples |
|----------|-------|---------|
| CV syllables (15 consonants × 5 vowels) — unstressed (tap r) | 75 | ra, thi, ke, so, fu... |
| CV syllables with trilled r (stressed variant) | 5 | ra, re, ri, ro, ru (trill) |
| V-only (word-initial) | 5 | a, e, i, o, u |
| "dh" syllables | 5 | dha, dhe, dhi, dho, dhu |
| Diphthong syllables (common onsets × 5 diphthongs) | ~30 | thai, rai, kau, lei, shoi, kia... |
| Closed-syllable codas (archaic/loan words) | ~12 | -el, -ir, -ith, -eim, -an, -ren, -eth... |
| **Total** | **~132** | |

At ~0.4 seconds per snippet: under 55 seconds of raw audio. Perhaps 20 minutes of studio recording time.

### 14.3 Pitch Envelopes for Tone Realization

Tones are realized by applying a **pitch envelope** (a curve, not a flat shift) to each snippet:

| Tone | Envelope Shape |
|------|---------------|
| Level | Flat — constant pitch throughout |
| Rising | Ascending ramp — starts lower, ends higher |
| Falling | Descending ramp — starts higher, ends lower |
| Dipping | V-shaped — descends to low point, then ascends |
| Peaking | Inverted-V — ascends to high point, then descends |

This is slightly more complex than flat pitch-shifting but well within standard vocal synthesis capabilities.

### 14.4 The Dual R

Two /r/ recordings: tap [ɾ] for unstressed syllables, trill [r] for stressed. Selection is automatic from the stress rule (always first syllable of root). The trill can be looped/extended for emphatic passages.

### 14.5 Transitions

- **CV → CV:** Vowel-to-consonant — clean crossfade (overlap-add). This is the most common transition.
- **Diphthong syllables:** Recorded as complete units — internal vowel glide is captured.
- **Closed → CV:** Consonant-to-consonant at the boundary — slightly harder. Pre-record common pairs (el-th, ir-sh) for smooth joins. Only affects archaic/loan vocabulary (~15%).

### 14.6 Synthesis Pipeline

```
Grid + Text → for each voice:
  → sequence of (syllable_id, pitch_sequence, duration) tuples
  → look up recorded snippet for each syllable
  → apply pitch envelope matching the pitch_sequence
  → crossfade adjacent snippets
  → output audio stream
→ mix four voices
→ final output
```

---

## 15. Rust/Python Boundary

### Training Phase (Python)
- Corpus parsing via music21
- Markov model parameter estimation
- Motif library extraction
- Preference model training (PyTorch / scikit-learn)

### Export to Rust
- **Markov model:** serialize transition probability tables (JSON, MessagePack, or custom binary)
- **Motif library:** list of interval sequences + metadata (JSON)
- **Vaelith language data:** vocabulary with tones, morpheme tables, grammar rules — all as structured data (JSON or similar)
- **Preference model:**
  - If logistic regression: weight vector as JSON → trivial Rust inference
  - If neural net: ONNX format → `ort` or `tract` crate

### Generation Phase (Rust)
- Text generation (Vaelith grammar engine)
- Structure generation, draft generation, SA refinement
- Scoring (all layers)
- MIDI output for evaluation
- Vocal synthesis

### Rating Interface (Python)
- Script that plays two MIDI files and collects pairwise preference
- Can be minimal: subprocess call to MIDI player, press 1 or 2
- Feeds into preference model retraining

---

## 16. Implementation Order

### Phase 1: Corpus Analysis & Markov Model
1. Set up music21, load Palestrina corpus, convert pieces to grid format
2. Build interval-based Markov model (per-voice melodic, per-voice-pair harmonic)
3. Validate: generate single melodies from Markov model — do they sound Palestrina-ish?

### Phase 2: Motif Extraction
4. Extract interval n-grams, build ranked motif library
5. Inspect results — do top motifs look like real Palestrina subjects?

### Phase 3: Basic Generation (Rust, Instrumental)
6. Implement grid representation with attack/continuation flags
7. Implement structure generation (motif selection, entry planning)
8. Implement draft generation (Markov-guided grid filling)
9. Implement MIDI output
10. Listen to drafts — recognizably "in the style"?

### Phase 4: Rule-Based Scoring & SA
11. Implement rule-based scoring (Layers 1–4)
12. Implement SA with Markov-guided mutations and hierarchical macro/micro operators
13. Validate: does SA improve drafts? Hard rule violations eliminated?

### Phase 5: Vaelith Text Integration
14. Implement Vaelith grammar engine in Rust (vocabulary, morphology, tone maps)
15. Add syllable-to-grid mapping (syllable onset flags, tone constraint lookup)
16. Add tonal contour constraint layer (Layer 5) to scoring
17. Add text-swap macro mutations
18. Implement call-and-response structure (dai/thol)
19. Validate: does the combined system produce text-music that respects both counterpoint and tonal constraints?

### Phase 6: Learned Preference Model
20. Generate a batch of pieces, build rating interface
21. Collect pairwise preferences
22. Train preference model in Python, export to ONNX/weights
23. Integrate into SA scoring (Layer 7)
24. Iterate: generate → rate → retrain → generate better

### Phase 7: Vocal Synthesis
25. Record ~132 syllable snippets
26. Implement concatenative synthesis with pitch envelopes
27. Implement crossfade transitions
28. Integrate with generation pipeline

### Phase 8: Polish & Tuning
29. Add user-facing parameter knobs (§12.6)
30. Tune SA temperature schedule, scoring weights, mutation rates
31. Add ensemble texture scoring (Layer 6)
32. Experiment with timbral control via vowel class biasing (§13)

---

## 17. Key Dependencies

### Python
- `music21` — corpus parsing and analysis
- `numpy` — feature computation
- `torch` or `scikit-learn` — preference model training
- `skl2onnx` or `torch.onnx` — model export
- `mido` or `pretty_midi` — MIDI I/O for rating interface

### Rust
- `ort` or `tract` — ONNX inference (if neural net preference model)
- `serde` / `serde_json` — deserializing Markov tables, motif library, language data, config
- `midly` or `nodi` — MIDI output
- `rand` — RNG for SA and sampling
- Audio library (e.g., `hound` for WAV output, `rodio` for playback) — vocal synthesis
- Pitch-shifting library or custom DSP — applying pitch envelopes to recorded snippets

---

## 18. Key References

- **Fux, *Gradus ad Parnassum* (1725)** — the counterpoint rules
- **Jeppesen, *The Style of Palestrina and the Dissonance* (1946)** — definitive analytical study
- **Tymoczko, *A Geometry of Music* (2011)** — mathematical voice-leading framework
- **Cope, *Computer Models of Musical Creativity* (2005)** — constraint + optimization approaches
- **Komosinski & Szachewicz, "Automatic species counterpoint composition by means of the dominance relation" (2015)** — evolutionary methods for counterpoint
- **Meredith, *Computational Music Analysis* (2016)** — geometric pattern discovery (motif extraction)
- **music21 documentation** — https://web.mit.edu/music21/

---

## 19. Open Questions

- **Rhythm modeling.** The current grid treats each cell as one eighth-note beat. Real Palestrina has varied note durations. Partially addressed by attack/continuation flags, but explicit rhythmic variety (e.g., dotted rhythms, syncopation) could be added as an additional mutation dimension.
- **Mode selection.** The system should operate in specific modes (Dorian, Mixolydian, etc.). The Markov model partially handles this if trained on mode-specific subsets. Explicit mode enforcement could be added as a scoring term.
- **More than 4 voices.** Architecture supports it, but SA convergence and scoring complexity grow. Start with 4.
- **Real-time interaction.** A user could fix some cells and have the system fill in the rest — interactive composition assistance.
- **Vaelith vocabulary expansion.** Current vocabulary is sufficient for prototyping. As game backstory develops, vocabulary will expand. The grammar engine is designed to accommodate growth without architectural changes.
- **Mythology-driven text.** Currently the text generator works from abstract semantic parameters. Eventually it could draw from a structured mythology database, generating text that tells specific stories or references specific lore.
- **Concatenative synthesis quality.** Pitch-shifting recorded snippets introduces formant artifacts (vowels don't sound quite right at extreme transpositions). For a stylized, slightly artificial elvish-choral sound this may be acceptable or even desirable. If not, formant-preserving pitch shifting (PSOLA or similar) can be investigated.
