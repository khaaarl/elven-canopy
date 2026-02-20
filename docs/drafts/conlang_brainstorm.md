# Elvish Conlang Brainstorming Notes

Working notes for the constructed Elvish language described in the design doc (S20). Not a spec -- a capture of brainstorming to resume from later.

## Design Constraints

- The phonology must produce a **finite, recordable set of vocal units** for the audio pipeline (sampled vocal syllables, see design doc S21 Phase 2). Target: manageable in an afternoon of recording per voice type at 3 reference pitches.
- The grammar must be **flexible enough for the poetry generator** (simulated annealing) to rearrange words freely while preserving meaning.
- The language should feel like a **real language** that players can optionally learn over time, but understanding it is never required.
- Start with a small vocabulary and grow. Dictionary is a data file, not hardcoded.

## Phonology

### Vowels

**Pure vowels (~5):** a, e, i, o, u

**Diphthongs (~6-8):** ai, ei, au, ou, ia, oi, ue, possibly ae. Diphthongs add lyrical, gliding quality. Think Welsh place names or Finnish -- the diphthongs are what make it sing.

### Consonants (~10-12)

- **Liquids:** l, r (flowing, melodic)
- **Nasals:** m, n (warm, resonant)
- **Fricatives:** s, sh, h (airy, gentle)
- **Stops:** t, k (soft, unaspirated -- occasional crispness)
- **Semivowels:** y, w
- **Undecided flavor consonants:** th (voiceless, Sindarin-ish edge)? v or f? These are the biggest levers on aesthetic feel.

Harsh sounds (hard g, harsh ch, z, voiced stops b/d) are absent or rare.

### Syllable Structure

More flexible than strict CV. Allowed types:

- **V:** standalone vowel -- a, i, o
- **D:** standalone diphthong -- ai, au, ei
- **CV:** consonant + vowel -- ta, ri, mo, se
- **CD:** consonant + diphthong -- kai, lei, thou, nau
- **CVC/CDC:** syllable-final coda limited to nasals (n, m) or liquids (l, r) -- tan, rin, kaur, shel

Estimated syllable inventory: **~150-200 syllables**. At 3 reference pitches = ~450-600 short recordings per voice type. A long afternoon, not a weekend.

Open question: restrict which consonants can precede diphthongs (e.g., only liquids and nasals) to cut the count? Or keep it open?

### Stress

Penultimate syllable (like Japanese, Italian, Finnish). Natural for singing.

### Word Formation

Words are 2-4 syllables. Compound words formed by joining roots.

## Grammar

### Free Word Order via Case Marking

The key unlock for poetry: if nouns carry **case suffixes**, word order is free. The suffix tells you who did what to whom, so the poetry generator can rearrange for meter and rhyme without changing meaning.

Example (placeholder suffixes):
- *Tanuel kaisen moritha.* -- "The elf sings a song." (SOV)
- *Moritha tanuel kaisen.* -- same meaning, object-first emphasis (OSV)
- *Kaisen moritha tanuel.* -- same meaning, verb-first for drama (VOS)

Where `-el` = subject (doer), `-a` = object (receiver), verb unmarked.

Default/neutral word order is **SOV** (subject-object-verb), but any order is grammatical.

### Cases (~4-6)

- **Nominative** -- subject/doer
- **Accusative** -- direct object
- **Genitive** -- possession, "of"
- **Dative** -- indirect object, "to/for"
- **Locative** -- place, "in/at/on" (important for tree-dwellers)
- **Ablative** (maybe) -- source/origin, "from" (pairs with locative)

Each case is a 1-2 syllable suffix. Fits naturally into agglutinative model.

### Other Grammar Features

- **Agglutinative suffixes** for tense, mood, and case. Suffixes stack predictably, and the meter system can account for added syllables.
- **Particles** for questions, emphasis, and poetic flourish.
- **No articles** -- streamlines generation, sounds more poetic.
- Grammar should be somewhat flexible/forgiving -- kinder to the poetry generator, and feels natural for a language used primarily in song.

## Vowel Harmony (Proposed)

Vowels split into two groups. All suffixes match the group of the root word's last vowel. Simple to implement, but makes the language sound internally consistent.

**Light vowels:** e, i (and diphthongs containing them: ei, ai, ia)
**Deep vowels:** a, o, u (and diphthongs: au, ou, oi)

Example: locative suffix might be *-lie* on light-vowel words, *-lai* on deep-vowel words. *Mirithel-lie* vs *Tanusel-lai*.

Bonus: lean into this during dictionary construction. Light vowels for sky/spirit/song concepts; deep vowels for earth/root/stone. Phonology itself carries emotional weight.

Decision: not yet final. Could add later without breaking earlier work.

## Morphology

- **Compounding:** root + root to form new words. *tanu* (tree) + *mori* (song) = *tanumori* (treesong). Simple, very moddable.
- **Agglutination:** suffixes stack. Gives long, rolling words that sound good in choral music.
- **Vowel harmony** (see above): suffixes shift vowels to match root. Adds perceived depth cheaply.
- **Reduplication:** ruled out. Doesn't sound right for the aesthetic.

## Name Structure

Elves have a **five-part name**:

| Component | Example | Function |
|-----------|---------|----------|
| Given | *Kaisen* | Chosen at birth. Always meaningful (from dictionary roots) |
| Nickname | *Patches* | Player-chosen, any language. Informal/affectionate. Optional |
| Family | *Morithel* | Lineage name, inherited. Compound of two roots |
| Home tree | *au-Tanushel* | Prepositional prefix *au-* ("of/dwelling-in") + tree's name. **Changes if they move** |
| Epithet | *Selkaimori* | Earned. Describes a deed or trait. Grows/changes over a lifetime |

Display in different contexts:
- **Tooltip:** Kaisen "Patches" Morithel
- **Full formal:** Kaisen Morithel au-Tanushel Selkaimori
- **Casual/logs:** Kaisen, or "Patches"
- **Poetic/songs:** given + epithet, or family + home tree

The home-tree-in-the-name is thematically resonant -- the tree (the player) is literally part of their identity. Displacement = name change = emotional beat.

### Open Questions on Names

- Should the tree itself have an Elvish name given by the elves? (The player is the tree spirit -- do the elves name you?)
- Do family names pass matrilineally, patrilineally, or by home tree? Home-tree inheritance makes "family" = "everyone in your tree" -- more of a clan/house system.
- When an elf earns a new epithet, does it replace the old one or accumulate? Accumulation is more DF-ish.

## Semantic Domains for the Dictionary

Core clusters for semantic tags on words:

- **Tree/growth:** root, branch, leaf, bark, sap, bud, bloom, canopy, ring (growth ring)
- **Light/sky:** sun, moon, star, dawn, dusk, shadow, rain, wind
- **Song/spirit/magic:** sing, chant, dream, mana, spirit, soul, harmony, echo
- **Community:** home, hearth, friend, elder, child, feast, gather
- **Craft:** weave, carve, build, shape, grow (transitive)
- **Emotion:** joy, sorrow, longing, pride, peace, fury, hope
- **Warfare/death:** fight, defend, fall, blade, shield, wound, death, ending, honor
- **Trade/outsiders:** exchange, travel, stranger, gift, bargain
- **Magic** (future): may fold into song/spirit domain

## Sample Roots (Vibes Only)

Not committed -- just testing aesthetic feel. Say them out loud.

| Root | Meaning | Notes |
|------|---------|-------|
| *tanu* | tree, wood | deep vowels, grounded |
| *kaise* | leaf, foliage | light vowels, airy |
| *mori* | song, melody | |
| *shela* | branch, arm | |
| *auren* | dawn, awakening | diphthong gives it sweep |
| *thiel* | spirit, breath | th adds a Sindarin-ish edge |
| *nauso* | root, foundation | diphthong + nasal = weighty |
| *leira* | starlight, silver | diphthong, liquid -- very "elven" |
| *kouth* | death, ending | deep + diphthong, heavier feel |
| *serim* | to weave, to craft | |
| *haloum* | home, shelter | long, warm |

## Wood Golem Naming (Deferred)

Revisit once dictionary exists. Direction: should feel **unnerving/creepy**, inspired by Warhammer 40K Eldar wraith-constructs (wraithlord, wraithseer, wraithguard). Not cute animated puppets -- something unsettling about dead wood given motion.

## Next Steps (When Resuming)

1. Nail the consonant inventory -- especially th, v/f, and how prominent sh should be.
2. Decide on vowel harmony (yes/no/later).
3. Sketch 5-6 case/grammar suffixes and test on sample sentences.
4. Generate 10-15 sample elf names using the five-part structure.
5. Build initial ~50-80 root words covering core semantic domains.
6. Generate 2-3 sample poem fragments to test the aesthetic end-to-end.
