# `elven_canopy_lang` Crate Design

> **Temporary draft.** When implementing F-lang-crate, delete this file and move
> its content into the appropriate permanent locations: the JSON lexicon file
> should have a companion JSON schema or be self-documenting, and the Rust code
> should have thorough module docstrings explaining the data model and design
> decisions. This draft exists only to capture the design before implementation.

## Purpose

A pure-Rust crate providing the Vaelith constructed language as a programmatic
resource. Shared by `elven_canopy_sim` (name generation) and
`elven_canopy_music` (lyrics/phrase generation). No Godot dependencies.

## Dependency Graph

```
elven_canopy_prng       (pure Rust, zero deps except serde)
  └── elven_canopy_lang (depends on prng, serde, serde_json)
        ├── elven_canopy_sim   (depends on lang, prng)
        └── elven_canopy_music (depends on lang, prng)
```

The lang crate depends on `elven_canopy_prng` so that language operations
(name generation, phrase selection) can accept `&mut GameRng` directly,
preserving the sim's determinism guarantee.

## Lexicon Data File

**Location:** `data/vaelith_lexicon.json`

A data-driven vocabulary file loaded at startup. Both the sim and music crates
read from the same file, ensuring vocabulary consistency.

### Schema

```json
{
  "words": [
    {
      "root": "thír",
      "gloss": "star",
      "pos": "noun",
      "tones": ["rising"],
      "vowel_class": "front",
      "name_tags": ["given", "surname"]
    },
    {
      "root": "rase",
      "gloss": "sing",
      "pos": "verb",
      "tones": ["level", "level"],
      "vowel_class": "front",
      "name_tags": ["given"]
    }
  ]
}
```

### Field Reference

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `root` | string | yes | The Vaelith word in its base form, with tone diacritics |
| `gloss` | string | yes | English translation / meaning |
| `pos` | string | yes | Part of speech: `"noun"`, `"verb"`, `"adjective"`, `"particle"` |
| `tones` | string[] | yes | Tone per syllable: `"level"`, `"rising"`, `"falling"`, `"dipping"`, `"peaking"` |
| `vowel_class` | string | yes | Vowel harmony class: `"front"` or `"back"` |
| `name_tags` | string[] | no | Which name positions this word fits: `"given"`, `"surname"`. Omit or empty = not used in names. |

### Design Notes

- **`name_tags`** is deliberately simple. A word tagged `["given"]` tends to
  appear as a given name or part of one; `["surname"]` for surnames;
  `["given", "surname"]` for either. No gender, no thematic domains — those
  can be added later when the vocabulary is larger and patterns emerge.
- **`tones`** are per-syllable, matching the music crate's existing `Tone` enum.
  The syllable count of a word is implicit in the length of its `tones` array.
- **`vowel_class`** determines suffix vowel harmony (front suffixes vs back
  suffixes), following the rules in `docs/drafts/vaelith_v4.md`.

## Crate Structure

```
elven_canopy_lang/
├── Cargo.toml
└── src/
    ├── lib.rs          # Crate root, module declarations, lexicon loading
    ├── types.rs        # Tone, VowelClass, Syllable, LexEntry, PartOfSpeech
    ├── phonotactics.rs # Syllable structure rules, vowel harmony
    └── names.rs        # Name generation from lexicon + phonotactic rules
```

### What Moves from the Music Crate

These types and logic from `elven_canopy_music/src/vaelith.rs` move into the
lang crate:

- **Types:** `Tone`, `VowelClass`, `Syllable`, `Word`, `LexEntry` (adapted to
  load from JSON rather than being hardcoded)
- **Phonotactic rules:** vowel harmony application, suffix selection
- **Lexicon data:** the hardcoded `NOUNS`, `VERBS`, `ADJECTIVES`, `PARTICLES`
  arrays become the JSON data file

These stay in the music crate (they're music-specific):

- `VaelithPhrase` and phrase generation templates
- Brightness-biased word selection
- Phrase candidate generation for SA text swaps

### What's New

- **`names.rs`**: Name generator that composes given names and surnames from
  lexicon entries tagged with `name_tags`. Names are compounds of 1-2 roots
  joined following vowel harmony rules (e.g., *Thíraleth* = "star-tree",
  *Kaelána* = "song-spirit"). The generator takes `&mut GameRng` for
  deterministic output.
- **JSON lexicon loader**: Deserializes `data/vaelith_lexicon.json` into the
  in-memory lexicon. The sim passes the JSON as a string (same pattern as
  `GameConfig`).

## Name Generation Design

Names are composed of meaningful Vaelith roots, loosely inspired by Dwarf
Fortress compound names but without rigid structure.

**Given names:** 1-2 roots from words tagged `"given"`, concatenated with
vowel harmony. Single-root names are common; two-root compounds less so.

**Surnames:** 1-2 roots from words tagged `"surname"`, same composition rules.

**Full name format:** `Given Surname` (e.g., *Thíraleth Kaelshíne* —
"star-tree song-glow").

The generator should produce names that:
- Are pronounceable according to Vaelith phonotactics (CV syllable structure)
- Sound consistent with the conlang's aesthetic
- Carry semantic meaning (translatable to English glosses)
- Are deterministic given the same PRNG state

## Initial Vocabulary

The initial lexicon should include roughly 30-50 words, migrated from the
music crate's hardcoded entries plus additional nature/culture terms suitable
for names. Every word that currently exists in `vaelith.rs` should be in the
JSON file; `name_tags` are added where appropriate.

Not every word makes a good name element — particles like "and" or "truly"
should have empty `name_tags`. Nature words (star, tree, leaf, forest), action
words (sing, grow, glow), and quality words (bright, great, hidden) are
natural name elements.

## Determinism Constraint

Since `elven_canopy_sim` depends on this crate, all operations that consume
PRNG state must be deterministic. The crate takes `&mut GameRng` from
`elven_canopy_prng`, never `rand` or system randomness.
