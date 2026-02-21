# Vaelith v4: A Tonal Elvish Conlang

*Vaelith /VAI-lith/ — from Old Elvish vael- "true, genuine" + -ith (abstract nominalizer). "True-speaking."*

*The name itself is a fossil: a closed-syllable archaic form that no longer follows Modern Vaelith phonology.*

---

## 1. Design Philosophy

Vaelith is co-designed with a polyphonic music generation system. It has three historical layers:

- **Old Elvish** — archaic roots with closed syllables, diphthongs, and consonant combinations now forbidden in new words. Survive in sacred vocabulary, names, and ritual phrases.
- **Modern Vaelith** — the regular productive system. Strict open-syllable structure, vowel harmony, agglutinative morphology.
- **Dwarven stratum** — loanwords from dwarves, partially assimilated, retaining some foreign features.

---

## 2. Phonology

### 2.1 Consonants (18)

| Letter | Sound | English Example | Notes |
|--------|-------|----------------|-------|
| m | m | **m**an | |
| n | n | **n**et | |
| t | t | **t**op (unaspirated, light) | Lighter than English t |
| k | k | **k**ite (unaspirated, light) | Lighter than English k |
| d | d | **d**og | Only appears word-initially and between vowels |
| f | f | **f**all | |
| v | v | **v**ine | Does not appear at end of words |
| th | as in **th**ink | Voiceless; tongue between teeth | NOT as in "the" — that sound is written **dh** |
| dh | as in **th**e | Voiced; tongue between teeth | In modern words: only between vowels. In old roots: anywhere |
| s | s | **s**un | |
| sh | as in **sh**ip | | |
| h | h | **h**and | |
| r | trilled r | Spanish pe**rr**o (stressed) or Spanish pe**r**o (unstressed) | Stressed: full rolling trill. Unstressed: single tongue-tap. This is the same letter — the pronunciation depends on whether the syllable is stressed. |
| l | l | **l**ight | |
| w | w | **w**ind | Does not appear before "o" or "u" |
| y | y | **y**es | Does not appear before "i" or "e" |

**What's NOT here (and why):** No "b" or "g" (too heavy/percussive). No "z" or "zh" sounds (keeps sibilants clean and soft). No guttural or throat sounds. The overall effect is breathy, flowing, and soft, with the trilled "r" providing dramatic punctuation.

### 2.2 Vowels (5)

| Letter | Sound | English Example |
|--------|-------|----------------|
| a | "ah" | f**a**ther |
| e | "eh" | b**e**d |
| i | "ee" | s**ee** |
| o | "oh" | g**o** |
| u | "oo" | bl**ue** |

All vowels are pure — your mouth doesn't move during them (unlike English "o" which often glides to "oo"). No vowel length distinction; how long a vowel is held depends entirely on the music.

### 2.3 Diphthongs (5)

A diphthong is two vowels blended into one syllable, with your mouth gliding from one position to the other. Vaelith has five:

| Diphthong | Sound | English Example | Direction | Notes |
|-----------|-------|----------------|-----------|-------|
| ai | "ah-ee" | **eye**, r**i**de | Falling (stress on "ah") | The most common diphthong; core sacred vocabulary |
| au | "ah-oo" | c**ow**, h**ou**se | Falling (stress on "ah") | Solemn, deep |
| ei | "eh-ee" | d**ay**, pl**ay** | Falling (stress on "eh") | Common in archaic verb forms |
| oi | "oh-ee" | b**oy**, c**oi**n | Falling (stress on "oh") | Rarest; a handful of ancient roots |
| ia | "ee-ah" | **ia** in "edi**a**" | Rising (stress on "ah") | The only rising diphthong; words about emergence and hope |

Diphthongs appear **only in Old Elvish roots** — never in modern coinages. They concentrate in the most poetically important vocabulary.

### 2.4 Vowel Classes (for Harmony)

Vaelith has **vowel harmony** — suffixes change their vowels to match the root they're attached to, creating runs of similar-sounding vowels within a word. There are two classes:

| Class | Vowels | Diphthongs | Sound Character |
|-------|--------|------------|----------------|
| **Front** | e, i | ei, ai, ia | Bright, clear, sharp |
| **Back** | o, u | au, oi | Deep, warm, solemn |

The vowel "a" is **neutral** — it goes with either class. If a word contains only "a" vowels, it defaults to front harmony.

**How it works:** Each suffix has a front variant and a back variant. You use whichever matches the last non-"a" vowel in the root:

- *thír* (star, front-class) + accusative → *thír-ne* (front suffix)
- *mòru* (forest, back-class) + accusative → *mòru-no* (back suffix)

### 2.5 Syllable Structure

**Modern Vaelith (the regular system for new words):**
- **CV** — one consonant + one vowel: *ra, thi, ke, so* (the vast majority)
- **V** — a vowel alone, only at the start of a word: *a-, e-, i-*
- **CVN** — consonant + vowel + "n" or "m", only in certain grammatical suffixes: *-ren, -shan*

**Old Elvish survivals (archaic words that break the modern rules):**
These are words so old and so important that they've resisted regularization. They have closed syllables (ending in a consonant), which gives them a heavier, denser sound:
- Ending in l, r, n, m, th, or dh: *kael, veir, thain, dheir*
- Rare double-consonant endings: *thairn* "the void"

**Dwarven loanwords:** Retain some foreign syllable patterns: *úrith, kógan, dóren*

**The effect:** Everyday speech flows in smooth open syllables (light, quick, elvish). Sacred and ancient vocabulary is studded with dense closed-syllable words (heavy, Tolkienesque). The contrast between these layers is Vaelith's phonological fingerprint.

### 2.6 Stress

**Rule: Stress always falls on the first syllable of the root word. Adding suffixes never moves the stress.**

| Word | Stress | Why |
|------|--------|-----|
| **ra**se | **RA**-se | Root = rase, stress on first syllable |
| **ra**se-ren | **RA**-se-ren | Suffix -ren added; stress doesn't move |
| **ra**se-ren-vei-me | **RA**-se-ren-vei-me | Three suffixes; stress still on first syllable |
| **thír**-leshi | **THÍR**-le-shi | Compound: primary stress on first word's root |

**Why this matters for music:** Stressed syllables should land on strong beats. Since stress is always predictable (first syllable of root), the music generator always knows where emphasis goes.

---

## 3. Tone System

### 3.1 What Tones Are

Every syllable in Vaelith has a **tone** — a specified pitch movement that happens *within* that syllable when it's spoken or sung. Tones are part of the word's identity, like spelling. Changing a tone can change meaning, just like changing a letter.

### 3.2 The Five Tones

| Tone | Diacritic | Visual Shape | Pitch Movement Within the Syllable | Minimum Notes When Sung | Example |
|------|-----------|-------------|-----------------------------------|-----------------------|---------|
| **Level** | none: a | — flat line | **Held steady.** No pitch change. | 1 | kael (song) |
| **Rising** | acute: á | / going up | **Pitch rises** from lower to higher. | 2 | thír (star) |
| **Falling** | grave: à | \\ going down | **Pitch falls** from higher to lower. | 2 | mòru (forest) |
| **Dipping** | caron: ǎ | ∪ valley/smile | **Pitch falls then rises.** A valley. | 3 | mǐr (tear) |
| **Peaking** | circumflex: â | ∩ hill/frown | **Pitch rises then falls.** A hill. | 3 | hâli (sun) |

**The diacritic shape matches the tone contour:**
- Acute (á) = rising line → pitch rises
- Grave (à) = falling line → pitch falls
- Caron (ǎ) = upward curve (valley/smile shape) → pitch dips down then comes back up
- Circumflex (â) = downward curve (hill/frown shape) → pitch peaks up then comes back down

**The critical rule:** These pitch movements happen *inside* the syllable, not between syllables. A rising-tone syllable must be sung as at least two notes (starting lower, ending higher) — you can hear the rise *within* it. A level-tone syllable is sung as a single held pitch with no bending. This means **you can always tell what tone a syllable has just by listening to it.**

### 3.3 Tones Drive Musical Texture

Because non-level tones require multiple notes per syllable, **the tone pattern of the text directly shapes the rhythmic density of the music:**

- A passage full of **level-tone** syllables → syllabic (one note per syllable), moving quickly through text
- A passage with many **rising/falling** syllables → moderately melismatic (two notes per syllable on toned words)
- A passage with **dipping/peaking** syllables → ornately melismatic (three+ notes per syllable)

Since dipping and peaking tones cluster in sacred and archaic vocabulary, **sacred text automatically generates more ornate, melismatic singing.** The register distinction is built into the phonology — the words themselves determine how elaborate the music becomes.

### 3.4 Tone Frequency Distribution

Not all tones are equally common. The target distribution across root syllables:

| Tone | Frequency | Rationale |
|------|-----------|-----------|
| Level | ~40% | The most common. Provides musical freedom (no pitch constraint). |
| Rising | ~20% | Growth, ascent, aspiration, brightness |
| Falling | ~20–25% | Descent, loss, darkness, depth, solemnity |
| Dipping | ~7% | Transformation, cycles, hidden things. Rare, ornate. |
| Peaking | ~7% | Transient beauty, flashes, crests. Rare, ornate. |

### 3.5 Tones on Diphthongs

Diphthongs are two vowels gliding together. The tone mark goes on the **stressed vowel** of the diphthong:

- **Falling diphthongs** (ai, au, ei, oi) — stress is on the first vowel, so the mark goes there:
  - Level: ai, au, ei, oi (no mark)
  - Rising: ái, áu, éi, ói
  - Falling: ài, àu, èi, òi
  - Dipping: ǎi, ǎu, ěi, ǒi
  - Peaking: âi, âu, êi, ôi

- **Rising diphthong** (ia) — stress is on the second vowel, so the mark goes there:
  - Level: ia (no mark)
  - Rising: iá
  - Falling: ià
  - Dipping: iǎ
  - Peaking: iâ

### 3.6 Tone Sandhi (Simplification Rules)

These are natural simplification patterns that occur in connected speech and singing:

1. **Adjacent identical contours may merge.** Two rising syllables in a row may be realized as one longer rise spanning both. Two falling syllables may merge into one descent. This prevents awkward stutter-stepping.

2. **Phrase-final falling.** The last syllable of a musical phrase tends to add a slight falling quality, aligning with natural cadence patterns. This is a tendency, not an override — the underlying tone is still perceptible within the overall descent.

3. **Emphatic override.** At a musical climax, any tone may be overridden for dramatic effect. This is the optimizer's emergency valve — used rarely, and meaning is preserved by context.

4. **Dipping/peaking reduction.** In fast passages, a dipping tone may simplify to just the low point (effectively falling), and a peaking tone may simplify to just the high point (effectively rising). This lets complex tones work at different tempi.

### 3.7 Tones and Musical Phrases

**Within a phrase:** Each syllable's tone specifies its own internal pitch contour. The transition *between* syllables (end of one to start of the next) is handled by the counterpoint rules, not by the tone system. Tones constrain what happens *inside* a syllable; counterpoint constrains what happens *between* them.

**At phrase boundaries:** No constraints span a rest or long held note. Phrase boundaries are "free" — the melody can reset.

**In long melismas:** If a syllable is sustained across more notes than its tone minimum requires, the extra notes are free — the tone contour happens at the beginning of the melisma, and subsequent notes can go wherever the counterpoint needs.

---

## 4. Historical Phonology (Brief)

This section explains why the irregularities exist — they're not random, they're historical.

**Proto-Elvish** (reconstructed, ~5000 years ago): Rich consonant inventory, free closed syllables, no tones, no vowel harmony.

**Old Elvish** (~3000 years ago): Vowel harmony develops. Final consonants begin eroding except liquids (l, r), nasals (n, m), and dental fricatives (th, dh). **Tones emerge from lost consonants** — a common process in real languages (Vietnamese and Chinese tones evolved this way). A word that once ended in a rising-pitch consonant leaves behind a rising tone on its vowel after the consonant is lost.

**Modern Vaelith** (present): Open-syllable preference is now dominant. New words follow strict CV(N). Old words with closed syllables are preserved as-is — too sacred or too common to regularize. Dwarven contact introduces loanwords.

**This explains:**
- Why sacred words break modern rules (they predate them)
- Why diphthongs only appear in old roots (modern Vaelith stopped creating them)
- Why tones exist (evolved from lost final consonants)
- Why common verbs have ablaut / irregular forms (inherited from Proto-Elvish)

---

## 5. Grammar

### 5.1 Typological Overview

- **Agglutinative** — grammatical relationships expressed by stacking suffixes
- **Vowel harmony** in suffixes (front/back variants)
- **Aspect-prominent** — *how* something happens matters more than *when* (for near-immortal elves)
- **Evidential marking** — *how you know* what you know
- **Head-final default (SOV)** — subject, then object, then verb; but freely reorderable
- **Pro-drop** — pronouns can be omitted when clear from context
- **Middle/passive voice** via prefix
- **Questions** marked by a particle whose position encodes social hierarchy

### 5.2 Nouns

**Case suffixes** with vowel harmony. Each suffix has a **fixed tone** (shown below). Most are level, giving the music maximum freedom on grammatical syllables:

| Case | Front | Back | Tone | Function |
|------|-------|------|------|----------|
| Nominative | -∅ | -∅ | — | Subject |
| Accusative | -ne | -no | Level | Direct object |
| Genitive | -li | -lu | Level | Possession, origin |
| Dative | -se | -so | Level | Recipient, purpose |
| Locative | -mi | -mu | Level | Place, time, "in/at/among" |
| Instrumental | -we | -wo | Level | By means of, through |
| Vocative | -léi | -lái | Rising (on diphthong) | Address, invocation — voice lifts when calling out |

**Number:**

| | Front | Back | Tone |
|---|------|------|------|
| Plural | -ri | -ru | Level |
| Collective | -neth | -noth | Level |

The collective (-neth/-noth) is an Old Elvish form (closed syllable). It means "all of them as a unity" — *thir-neth* "starlight as a phenomenon" vs. *thir-ri* "individual stars."

**Example:** *thír* "star" (front-class, Old Elvish root)

| Form | Vaelith | Tone Sequence |
|------|---------|---------------|
| star | thír | rising |
| star (object) | thír-ne | rising-level |
| star (possession) | thír-li | rising-level |
| O star! | thír-léi | rising-rising(diphthong) |
| stars | thír-ri | rising-level |
| stars (among) | thír-ri-mi | rising-level-level |
| starlight (collective) | thír-neth | rising-level |

**Example with back harmony:** *mòru* "deep forest" (back-class)

| Form | Vaelith | Tone Sequence |
|------|---------|---------------|
| forest (object) | mòru-no | falling-level-level |
| forest (possession) | mòru-lu | falling-level-level |
| O forest! | mòru-lái | falling-level-rising(diphthong) |

### 5.3 Verbs

A verb form is built as: **(voice prefix) + stem + aspect + (evidential) + (mood)**

Each piece has its own fixed tone. Stacking them creates a word with a known tone sequence.

#### Voice: The Middle/Passive Prefix

| Prefix | Harmony | Tone | Meaning |
|--------|---------|------|---------|
| en- | Front | Level | The subject undergoes, receives, or experiences the action |
| an- | Back | Level | Same, with back harmony |

This removes the agent:
- **True passive:** *En-rase-shi kael.* — "The song was sung."
- **Middle/reflexive:** *En-waithe-shi ana.* — "The spirit wove itself."
- **Spontaneous:** *En-shine-ren thír.* — "The star is shining (of its own nature)."

Critical for sacred poetry — things are created, names are spoken, light is woven — by no named agent.

#### Aspect

| Aspect | Front | Back | Tone | Meaning | Notes |
|--------|-------|------|------|---------|-------|
| Imperfective | -ren | -ran | Level | Ongoing, in process | |
| Perfective | -shi | -shu | Level | Completed, done | |
| Inceptive | -kél | -kál | Rising | Beginning, dawning | Rising = starting upward |
| Habitual | -tha | -tha | Level | Regular, recurring | *No harmony — archaic frozen form* |
| Cessative | -mèil | -màil | Falling (on diphthong) | Fading, ceasing | *Diphthong — archaic. The suffix for "fading" itself descends* |
| Eternal | -thir | -thur | Level | Timeless truth | *Closed syllable — archaic. Stillness of eternity* |

#### Evidentiality (Optional — Stacks After Aspect)

| Evidential | Suffix | Tone | Meaning | Usage |
|------------|--------|------|---------|-------|
| Direct | -∅ | — | I witness this now | Default |
| Remembered | -en / -an | Level | I know from personal memory | Very common for long-lived elves |
| Told/Sung | -vei | Level | I know from oral tradition, song | Marks inherited knowledge |
| Intuited | -âith | Peaking (on diphthong) | I know through spiritual perception | *Peaking = a flash of insight that crests and settles. Requires 3+ notes — spiritual knowledge is inherently ornate* |

The intuited evidential is the most musically elaborate suffix in the language. Every time an elf marks knowledge as spiritually perceived, the music becomes more ornate. This is not a coincidence — it's baked into the culture through the phonology.

#### Mood (Optional — Stacks Last)

| Mood | Front | Back | Tone | Use |
|------|-------|------|------|-----|
| Imperative | -ló | -lú | Rising | Command — voice lifts with authority |
| Optative | -me | -mo | Level | Wish, hope, prayer, blessing |
| Conditional | -shan | -shun | Level | "Would" / "if" |

**Full stacking example:** *an-kose-ren-vei-me* — "May it be growing (as the songs foretell)"
- an- (level) + kó (rising) + se (level) + ren (level) + vei (level) + me (level)
- Tone sequence: level-rising-level-level-level-level
- Six syllables, one word. The rising tone on *kó* (the root) is the musical anchor; the rest is free.

#### Ablaut (Irregular Strong Verbs)

The twelve most ancient verbs change their stem vowel to a diphthong in the perfective aspect. **The tone of the original syllable transfers to the diphthong:**

| Verb | Meaning | Stem (Imperf.) | Stem Tone | Stem (Perf.) | Perf. Tone | Example |
|------|---------|----------------|-----------|-------------|------------|---------|
| rase | sing | ras- | Level | rais- | Level | raise-shi |
| lethe | flow | leth- | Level | leith- | Level | leithe-shi |
| shine | shine | shin- | Level | shaun- | Level | shaune-shi |
| fare | fly | far- | Level | foir- | Level | foire-shi |
| ashe | breathe/live | ash- | Level | aush- | Level | aushe-shi |
| mire | see/behold | mir- | Level | meir- | Level | meire-shi |
| wethe | weave | weth- | Level | waith- | Level | waithe-shi |
| thale | name/call | thal- | Level | thail- | Level | thaile-shi |
| kóse | grow | kós- | Rising | káus- | Rising | káuse-shi |
| fáre | fly | fár- | Rising | fóir- | Rising | fóire-shi... |

Wait — I have "fare" listed twice with different tones. Let me fix that. Let me reconsider which verbs are rising vs level.

Actually, let me reconsider: most of the twelve strong verbs should be level (they're the most fundamental, most common verbs — keeping them level gives maximum musical freedom). I'll make only 2-3 of them non-level.

| Verb | Meaning | Stem (Imperf.) | Tone | Stem (Perf.) | Example (Perfective) |
|------|---------|----------------|------|-------------|---------------------|
| rase | sing | ras- | Level | rais- | raise-shi |
| lethe | flow | leth- | Level | leith- | leithe-shi |
| shine | shine | shin- | Level | shaun- | shaune-shi |
| wethe | weave | weth- | Level | waith- | waithe-shi |
| ashe | breathe/live | ash- | Level | aush- | aushe-shi |
| mire | see/behold | mir- | Level | meir- | meire-shi |
| thale | name/call | thal- | Level | thail- | thaile-shi |
| niwe | dwell | niw- | Level | naiv- | naive-shi |
| fole | love | fol- | Level | foil- | foile-shi |
| sethe | remember | seth- | Level | seith- | seithe-shi |
| kóse | grow | kós- | Rising | káus- | káuse-shi |
| fáre | fly | fár- | Rising | fáir- | fáire-shi |

**Ablaut rule:** The tone on the stem syllable stays the same; only the vowel quality changes. This is consistent: tone is a property of that position in the word, independent of vowel quality.

**Musical effect:** The perfective (completed, past) forms introduce diphthongs — making the sound denser and more archaic. The language *sounds different when speaking of the past.*

#### Suppletive Forms (Completely Irregular)

A few verb-noun pairs have unrelated roots, like English "go/went":

| Base | Meaning | Irregular Form | Notes |
|------|---------|---------------|-------|
| ashe | to live/breathe | màured (noun) | "Death" is unrelated to "living" — elves consider death so alien that it has no kinship with life |
| mire | to see | vael (in compounds) | "True-seeing" uses old root *vael-*, not *mir-*. Spiritual and physical sight are distinct |
| ére | to be | nère (negative) | "To not-be" is its own word, not just "not" + "be" |

### 5.4 Pronouns

| Person | Singular | Plural Inclusive | Plural Exclusive |
|--------|----------|-----------------|-----------------|
| 1st | ná (rising) | náire | náli |
| 2nd | thé (rising) | théri | — |
| 3rd | le (level) | leru | — |

**Rising tone on 1st/2nd person** distinguishes them from level-toned particles: *ná* "I" (rising) vs. *na* "and" (level). You can always hear the difference.

**Inclusive vs. exclusive "we":**
- *Náire rase-thir* — "We (all of us, including you) sing eternally" — communal, inviting
- *Náli rase-thir* — "We (our kind, but not you) sing eternally" — exclusive, solemn

Pronouns take standard case suffixes with front-class harmony.

### 5.5 Questions

Questions are marked by the particle **vái** (rising tone — questions ascend). Its **position in the sentence encodes social hierarchy:**

| Position | Register | Social Meaning | Example |
|----------|----------|---------------|---------|
| Sentence-initial | Deferential | Addressing elders, sacred beings | *Vái thír shine-ren?* |
| Pre-verb | Neutral / Equal | Normal polite question among peers | *Thír vái shine-ren?* |
| Sentence-final | Familiar / Downward | Intimate, casual, addressing juniors | *Thír shine-ren vái?* |

**Interrogative words** (question words):

| Word | Tone | Meaning |
|------|------|---------|
| mái | Rising | What? |
| thei | Level | Who? (Old Elvish — diphthong) |
| hàmi | Falling-level | Where? |
| àke | Falling-level | When? |
| théla | Rising-level | Why? |
| séve | Level-level | How? |

Interrogative words sit where the answer would go, with *vái* in its register-appropriate position:

- *Vái thei rase-ren?* — "Who is singing?" (deferential)
- *Thei vái rase-ren?* — "Who is singing?" (neutral)
- *Thei rase-ren vái?* — "Who's singing?" (familiar)

**Rhetorical questions in poetry** may omit *vái* entirely when an interrogative word is present. The question is implicit, unanswered, reverberating:

> *Thei raise-shi ain thairn?*
> "Who sang before the void?" (no *vái* — rhetorical, cosmic)

### 5.6 Subordination

| Particle | Tone | Meaning | Example |
|----------|------|---------|---------|
| ke | Level | when, if | *Ke resha kóse-ren...* "When the moon rises..." |
| e | Level | that, which | *kael e en-rase-shi* "the song that was sung" |
| sha-ke | Level-level | because, since | *Sha-ke thír shine-thir...* "Because stars shine..." |
| nei | Level | although, despite | *Nei thairn mesha ére-ren...* "Although the void is deep..." |
| la-se | Level-level | in order to | *La-se kael en-sethe-me...* "So that the song be remembered..." |
| dha | Level | while, during | *Dha ná rase-ren...* "While I sing..." |

Subordinate clauses typically precede the main clause, but in poetry may follow for dramatic effect:

> *En-rase-thir kael — sha-ke thairn eirith nère-thir.*
> "The song is sung eternally — because the void is not eternal."

### 5.7 Word Order

Default: **SOV** (Subject-Object-Verb), modifiers before heads. Case marking makes all reorderings unambiguous:

| Order | Effect |
|-------|--------|
| SOV | Neutral statement |
| OSV | Topic-fronting: "As for X..." |
| VSO | Dramatic, declamatory — used at climactic moments |
| OVS | Revelation — the agent is revealed last |
| V alone | Sacred / universal — agent and object implied. Pure action |

**For the optimizer:** Any word can be placed at any metric position without changing meaning — only emphasis shifts.

### 5.8 Adjectives and the Copula

Adjectives precede the noun: *aurel thír* "golden-bright star."

**Copula** (the verb "to be"): **ere** (level tone), conjugated for aspect like any verb.

**Negative copula** (suppletive / irregular): **nère** (falling tone). "Is-not" is its own word — punchy and compressed:

- *Thairn eirith nère-thir.* — "The void is not eternal." (One word for "is-not-eternally.")

**Suppletive intensive adjectives** (irregular "comparatives"):

| Base | Meaning | Intensive | Meaning |
|------|---------|-----------|---------|
| ráva | bright | aurel | radiant, golden-bright |
| mèsha | deep | thairn-li | "of the void" (bottomless) |
| ála | great | airendel | exalted beyond measure |

### 5.9 Poetic Elision

In sacred/poetic register, these reductions are licensed under metric pressure:

1. **Case suffix drop.** When grammatical role is obvious from context or word order.
2. **Evidential drop.** When the register is already established by a previous marked verb.
3. **Pronoun drop.** When the subject is obvious from context (very common in Vaelith).
4. **Compound compression.** The linking vowel between elements of a compound may be dropped: *mòru-kael* → *mòr-kael*.

Each saves 1–2 syllables while remaining grammatical.

### 5.10 Numerals

| Number | Vaelith | Tone | Notes |
|--------|---------|------|-------|
| 1 | re | Level | Also the indefinite article "a" |
| 2 | dha | Level | |
| 3 | thel | Level | Closed syllable — old form. Three is sacred |
| 4 | kéra | Rising-level | |
| 5 | hani | Level-level | |
| 6 | sélu | Rising-level | |
| 7 | oith | Level | Diphthong, closed syllable — old form. Seven is mystical |
| 8 | méva | Rising-level | |
| 9 | thúne | Rising-level | |
| 10 | dáiri | Rising-level | Diphthong — old form |
| many | véla | Rising-level | |
| few | fitha | Level-level | |
| first | ain-li | Level-level | Literally "of the before" |
| last | maured-li | Level-level-level | Literally "of the ending" |

Numbers precede the noun: *thel thír* "three stars," *oith aleth* "seven trees."

---

## 6. Liturgical Structure: Call and Response

### Dai — "Truly / So it is"

Short affirmation (level tone on the diphthong — steady, grounded). The chorus sings *dai* (or *dai, dai*) together in unison rhythm — all voices attacking the same note(s) at the same time. This **homophonic** moment punctuates the **polyphonic** (multi-voice, staggered) texture.

> CANTOR: *Thír-neth shine-thir, dai.*
> CHORUS: ***Dai.***

### Thol — "Eternal / So be it"

An Old Elvish word for "eternal" (level tone — still and timeless). Used at major structural boundaries. The chorus sustains *thol* as a long held chord, with voices entering one at a time, building a resonant cluster. It creates meditative space — breathing room.

> CANTOR: *[end of a major section]*
> CHORUS: ***Thol...*** *(sustained, 4–8 beats)*

**Musical function:**
- **Dai** = sharp, rhythmic, punctuating
- **Thol** = sustained, spacious, breathing

---

## 7. Vocabulary

*This section provides vocabulary sufficient for prototyping. Specific words will evolve with backstory. What matters mechanically is tone distribution, vowel class, and syllable type.*

### 7.1 Nature and the Physical World

| Vaelith | Meaning | Vowel Class | Syllable Type | Tone(s) |
|---------|---------|-------------|---------------|---------|
| áiren | high canopy, upper world | Front | Old (closed) | Rising-level |
| mòru | deep forest, home | Back | Modern | Falling-level |
| aleth | tree (sacred, ancient) | Front | Old (closed) | Level-level |
| lena | leaf | Front | Modern | Level-level |
| thuri | root, foundation, ancestor | Front | Modern | Level-level |
| sela | flower, blossom | Front | Modern | Level-level |
| wena | water | Front | Modern | Level-level |
| nàshi | stream, flowing water | Front | Modern | Falling-level |
| kola | stone, earth, ground | Back | Modern | Level-level |
| féri | vine, moss, tendril | Front | Modern | Rising-level |
| hàla | branch, arm of tree | Front | Modern | Falling-level |
| thír | star | Front | Old (closed) | Rising |
| resha | moon | Front | Modern | Level-level |
| hâli | sun | Front | Modern | Peaking-level |
| shela | sky, the open above | Front | Modern | Level-level |
| fàna | cloud, mist | Front | Modern | Falling-level |
| wira | wind | Front | Modern | Level-level |
| thâne | fire, flame | Front | Modern | Peaking-level |
| shîma | shimmer, light on water | Front | Modern | Peaking-level |
| áurem | dawn, the golden hour | Back | Old (closed) | Rising(diph)-level |
| thairn | the void, deep dark | Front | Old (closed) | Level |
| vâith | storm, wrath of sky | Front | Old (closed) | Peaking(diph) |

### 7.2 Light, Shadow, and Perception

| Vaelith | Meaning | Vowel Class | Tone(s) | Notes |
|---------|---------|-------------|---------|-------|
| léshi | light, radiance | Front | Rising-level | Light ascends |
| mura | shadow, darkness | Back | Level-level | Shadow is neutral — restful, natural |
| aurel | golden radiance | Back | Level-level | |
| mir | eye, sight | Front (Old) | Level | Monosyllable |
| mǐr | tear, weeping | Front (Old) | Dipping | Tears: dipping tone → 3+ note melisma → ornate, mournful |
| veir | voice, truth spoken | Front (Old) | Level | Root of the language name |
| rè | ash, remains | Front | Falling | |

### 7.3 Spirit, Mind, and the Sacred

| Vaelith | Meaning | Vowel Class | Tone(s) | Notes |
|---------|---------|-------------|---------|-------|
| ana | spirit, soul | Front | Level-level | Fundamental — no tonal embellishment |
| kael | song, prayer | Front (Old) | Level | Song and prayer are the same word |
| něma | dream (prophetic) | Front | Dipping-level | Dreams dip below the surface and return |
| fola | love, devotion | Back | Level-level | |
| sena | memory | Front | Level-level | |
| washi | wisdom | Front | Level-level | |
| thǒra | magic, craft | Back | Dipping-level | Magic = transformation = dipping |
| mila | peace, fullness | Front | Level-level | |
| vaelin | truth | Front (Old) | Level-level | |
| rìne | sorrow, longing | Front | Falling-level | Sorrow descends |
| eirith | eternity | Front (Old) | Level-level | Eternity is still |
| âitha | prophecy | Front | Peaking(diph)-level | Flash of revelation |
| airendel | exalted beyond measure | Front (Old) | Level-level-level | Already weighty from length |
| kiável | emergence, hope | Front (Old) | Rising(diph)-level | Hope rises |

### 7.4 Dark Vocabulary

| Vaelith | Meaning | Vowel Class | Tone(s) | Notes |
|---------|---------|-------------|---------|-------|
| thairn | the void, unmaking | Front (Old) | Level | Still and empty, like the void itself |
| màured | death, the final silence | Back (Old) | Falling(diph)-level | Death descends |
| sùva | forgetting, loss of memory | Back | Falling-level | The gravest sin |
| hàleth | fear | Front (Old) | Falling-level | |
| dàren | exile, severance from canopy | Front (Old) | Falling-level | |
| nòvith | decay, slow unweaving | Back (Old) | Falling-level | |
| rè | ash, remains | Front | Falling | |
| thàur | betrayal | Front (Old) | Falling | Harsh: closed syllable + falling tone |
| vàire | empty silence (absence of song) | Front | Falling-level | The most terrible word |
| shàdur | ruin, destruction | Back (Dwarven) | Falling-level | |

**Pattern:** Falling tone clusters heavily in the dark vocabulary. Words for loss, descent, ending, and destruction tend to fall. This is not arbitrary — it evolved because these concepts were historically associated with descending pitch patterns. The tone system carries emotional meaning beyond individual words.

### 7.5 Adjectives

| Vaelith | Meaning | Tone(s) | Notes |
|---------|---------|---------|-------|
| ráva | bright | Rising-level | Brightness ascends |
| ála | great, high | Rising-level | Greatness lifts |
| mèsha | deep, profound | Falling-level | Depth descends |
| thana | ancient | Level-level | Antiquity just is |
| néla | new, young | Rising-level | Newness ascends |
| sera | sacred, holy | Level-level | Sacredness is still |
| rǐma | hidden, secret | Dipping-level | Hidden things dip below the surface |

### 7.6 Core Verbs

The twelve **ablaut verbs** (★) plus regular verbs:

| Verb | Meaning | Root Tone | Notes |
|------|---------|-----------|-------|
| ★ rase | sing, pray, create through voice | Level | The most fundamental verb |
| ★ lethe | flow, move fluidly | Level | |
| ★ shine | shine, glow | Level | |
| ★ wethe | weave (song, magic, branches) | Level | |
| ★ ashe | breathe, live, be | Level | |
| ★ mire | see, behold, witness | Level | |
| ★ thale | name, call, summon | Level | |
| ★ niwe | dwell, rest, abide | Level | |
| ★ fole | love, tend, cherish | Level | |
| ★ sethe | remember | Level | |
| ★ kóse | grow, become | Rising | Growth ascends |
| ★ fáre | fly, soar | Rising | Flight ascends |
| methe | listen, attend | Level | |
| kethe | hold, guard, keep | Level | |
| thore | craft, shape | Level | |
| rìni | fall gently (leaves, rain, tears) | Falling | Falling descends |
| lose | dance | Level | |
| hane | give, offer | Level | |
| wake | know, understand | Level | |
| dave | endure, persist | Level | |
| sùve | forget | Falling | Loss descends |
| màure | die, pass beyond | Falling | |
| kíane | emerge, be born | Rising | Contains rising diphthong /ia/. Emergence ascends |
| thaire | cross a boundary | Level | |
| ere | to be (copula) | Level | |
| nère | to not-be (negative copula) | Falling | Negation descends |

**Note:** Most strong verbs are level-toned — these are the most fundamental, most common verbs, and level tone gives the music maximum freedom when they're used.

### 7.7 Function Words

Every particle and function word has a fixed tone:

| Word | Meaning | Tone |
|------|---------|------|
| na | and | Level |
| o | or | Level |
| e | that, which | Level |
| sha | within, deep inside | Level |
| ke | when, if | Level |
| nì | not | Falling — negation descends |
| há | oh, behold | Rising — exclamation lifts |
| á- | O (vocative prefix) | Rising — calling out lifts |
| dai | truly, indeed | Level — steady affirmation |
| ve | from, out of | Level |
| ain | before, prior to | Level |
| vái | question particle | Rising — questions lift |
| sha-ke | because | Level-level |
| nei | although | Level |
| la-se | in order to | Level-level |
| dha | while, during | Level |
| re | one, a (indefinite) | Level |
| lóshi | now | Rising-level |
| fàni | then, long ago | Falling-level — the past descends |
| késhi | always, forever | Rising-level |

### 7.8 Dwarven Loanwords

| Vaelith | Origin | Meaning | Tone(s) | Notes |
|---------|--------|---------|---------|-------|
| úrith | *urist* | Short blade; metaphor: sharp wit | Rising-level | |
| kógan | *kogan* | Sturdy vessel (for things or memories) | Rising-level | Nearly unchanged |
| úshan | *usan* | Fallen in battle (euphemized from "murdered") | Rising-level | |
| dóren | *doren* | Traveling company, band of companions | Rising-level | |
| shàdur | *shadur (?)* | Ruin, desolation | Falling-level | Falling tone added by elves (= loss) |

### 7.9 Key Compounds

| Compound | Meaning | Tone Sequence |
|----------|---------|---------------|
| thír-léshi | starlight | rising-rising-level |
| mòru-kael | forest-song (the sacred choral tradition) | falling-level-level |
| wena-resha | moonlight on water | level-level-level-level |
| aleth-thuri | ancestry, heritage | level-level-level-level |
| ana-thâne | spirit-flame: inspiration | level-level-peaking-level |
| sena-wethe | memory-weave: history-singing | level-level-level-level |
| fola-rìne | love-sorrow: bittersweet love | level-level-falling-level |
| thairn-veir | void-voice: ominous silence that speaks | level-level |
| áurem-kael | dawn-song: morning hymn | rising-level-level |
| kiável-ana | emergence-spirit: newborn soul | rising-level-level-level |

---

## 8. Frozen Archaic Phrases

Ritual formulae in Old Elvish grammar. Every elf knows them; few could parse them grammatically.

---

**"Kael áiren thol."**
*Song [of] canopy eternal.*
Old grammar: no genitive suffix (juxtaposition = possession), adjective *thol* follows noun (modern: adjective precedes). *Thol* "eternal" is extinct in productive use — replaced by suffix *-thir*.
Used: to open sacred ceremonies.

---

**"Dai veir! Dai veir!"**
*Truly, speak! Truly, speak!*
Old grammar: bare stem as imperative (modern uses *-ló*). The repetition is fixed — always twice.
Used: formal call to attention; opens councils.

---

**"Ain thairn, ain kael."**
*Before the void, before song.*
Refers to the moment before creation — when neither nothingness nor song yet existed.
Used: at funerals and the winter solstice.

---

**"Aurel noth-en-dai."**
*Golden-radiance all-beings-truly.*
*Noth* is an extinct Old Elvish collective noun: "all living things." The chain *-en-dai* fuses remembered-evidential with emphatic.
Used: as a blessing, spoken while touching the forehead.

---

**"Mirith vael, thír vael, kael vael."**
*Still [is] truth. Star [is] truth. Song [is] truth.*
Old grammar: copula omitted. A meditation — traditionally chanted on a single note, then repeated higher, then lower.
Used: personal meditation, centering before important decisions.

---

**"Thol kiável, thol màured, thol kael."**
*Eternal [is] emergence, eternal [is] death, eternal [is] song.*
A proverb turned liturgical formula. Originally fatalistic, reinterpreted as cosmic order. The rising diphthong in *kiável* lifts the voice; *màured* brings it down; *kael* settles level.
Used: at births, deaths, and the new year.

---

## 9. Integration with Polyphonic Music Generation

### 9.1 Tone → Music: The Core Mechanism

Each syllable specifies its own internal pitch contour. The music generator must realize this contour within the note(s) assigned to that syllable:

| Tone | What the Generator Must Do | Min. Notes |
|------|---------------------------|-----------|
| Level | Assign one pitch. Hold it steady. | 1 |
| Rising | Assign 2+ notes, ascending. | 2 |
| Falling | Assign 2+ notes, descending. | 2 |
| Dipping | Assign 3+ notes: down, bottom, up. | 3 |
| Peaking | Assign 3+ notes: up, top, down. | 3 |

**Between syllables:** The transition from the end of one syllable to the start of the next is governed by counterpoint rules, not by the tone system. Tones constrain *within*; counterpoint constrains *between*.

### 9.2 Constraint Density Analysis

How much of the music is constrained by tones, and how much is free for the counterpoint optimizer?

**Root syllables of content words:** ~40% level (free), ~60% have a directional tone (constrained within that syllable, but the counterpoint between syllables is still free).

**Suffix syllables:** Almost all level (free). The exceptions are:
- Vocative -léi/-lái (rising)
- Inceptive -kél/-kál (rising)
- Cessative -mèil/-màil (falling)
- Intuited -âith (peaking)
- Imperative -ló/-lú (rising)

In a typical phrase, suffixes outnumber roots. Combined with level-toned function words, approximately **60–70% of all syllables in a given phrase are level-toned** (no internal pitch constraint), giving the counterpoint optimizer wide freedom.

Of the remaining 30–40% that carry directional tones, each constrains only the notes *within* that syllable — it says "these 2–3 notes must go up" or "these must go down," but it doesn't constrain what happens before or after. The space between constrained syllables is entirely free.

### 9.3 Cadence Alignment

SOV word order means most phrases end with a verb. Most verbs have level-toned roots. Verb suffixes are mostly level. The phrase-final falling sandhi tendency adds a gentle descent. Result: phrases naturally end with unconstrained, slightly descending motion — aligning with Western cadence patterns. Grammar and music push in the same direction.

### 9.4 Vowel Harmony and Vocal Color

Front-harmony passages (e, i, ei, ai, ia) have a brighter, more silvery vocal timbre. Back-harmony passages (o, u, au, oi) are darker and warmer. This is a difference in vowel quality (formant frequencies), not pitch, so it's preserved under pitch-shifting.

The optimizer can select vocabulary partly for timbral color: a stanza about starlight favors front-class words (bright), one about deep roots favors back-class (dark).

### 9.5 Joint Optimization of Text and Music

When lyrics are procedurally generated, the optimizer can:

1. **Choose synonyms** based on tone pattern and vowel class
2. **Reorder words** (SOV ↔ OSV ↔ OVS ↔ VSO) to place desired tones at specific beats
3. **Select case frames** for different suffix sounds
4. **Choose aspect** when multiple are valid — different suffixes yield different tone sequences
5. **Exploit ablaut** — perfective stems introduce diphthongs, denser sound
6. **Choose toned vs. level vocabulary** depending on whether the music needs melismatic or syllabic texture
7. **Apply poetic elision** to shorten phrases exceeding their musical space
8. **Select inclusive vs. exclusive "we"** for communal vs. solemn effects
9. **Choose active vs. middle voice** — middle drops the agent, saving words

### 9.6 Ensemble Texture

When four voices sing different text simultaneously:

**Avoid:** All voices hitting dense fricatives (sh, s, f) at the same time — creates muddy hissing. Stagger fricative-heavy syllables.

**Seek:** Moments where multiple voices sustain open vowels (a, o, e) simultaneously — resonant, blended sonority. These are natural "bloom" points.

The optimizer implements this as a soft scoring term: penalize simultaneous fricative density, reward simultaneous open-vowel alignment.

---

## 10. Example Texts

*All tone marks in these poems follow the assignments in §7. Every syllable has a definite, fixed tone.*

### 10.1 "Mòru-Kael" — The Forest Song

*Sacred hymn for four voices with call-and-response.*

**Opening Ritual**

> CANTOR: *Dai veir! Dai veir!*
> CHORUS: ***Dai.***

**Stanza 1 — Invocation**

> Há, aleth-ri, há, áiren airendel —
> théri-li lena lethe-ren,
> théri-li hàla wethe-ren.
> Ashe-thir na kóse-thir, dai.

*O trees, O canopy exalted beyond measure —*
*your leaves are flowing,*
*your branches are weaving.*
*Living and growing eternally, truly.*

> CHORUS: ***Dai, dai.***

Tone analysis of line 1: há (rising), aleth (level-level), ri (level), há (rising), áiren (rising-level), airendel (level-level-level). The two *há* particles lift the voice — exclamatory. The sacred word *áiren* ascends on its first syllable. The long word *airendel* rolls through three level syllables — no pitch constraint, pure musical freedom.

**Stanza 2 — Memory and the Void**

> Thana thuri-mi sena niwe-ren-en.
> Thana kola-mi ana niwe-ren-en.
> Ain thairn — sethe-ló!
> Ain vàire — rase-ló!
> Mé aleth-ri, sethe-ló dai.

*In ancient roots, memory dwells (we remember).*
*In ancient earth, spirit dwells (we remember).*
*Before the void — remember!*
*Before the empty silence — sing!*
*All trees — truly, remember.*

Tone analysis of lines 3–4: Ain (level) thairn (level) — flat, still, void-like. Then sethe (level-level) + -ló (rising) — the imperative suffix *lifts* the command. The frozen phrase *ain thairn* is entirely level (toneless, like the void), contrasting with the rising imperative that follows.

> CHORUS: ***Dai.***
> CHORUS: ***Thol...*** *(sustained chord)*

**Stanza 3 — Light and Shadow**

> Ke mura kóse-ren, thír-léshi en-hane-ren.
> Ke rìne kóse-ren, resha fole-ren.
> Shela na kola, léshi na mura —
> en-waithe-shi-vei, en-waithe-shi-vei, mé kael sha.

*When shadow grows, starlight is given.*
*When sorrow grows, the moon tends us.*
*Sky and earth, light and shadow —*
*woven (the songs tell), woven, into all song.*

Tone analysis of climactic line: en (level) waithe (level-level) shi (level) vei (level) — entirely level, maximally free for the counterpoint. The ablaut diphthong *ai* in *waithe* is level-toned — no pitch constraint, but the diphthong itself gives the syllable acoustic density. The word is repeated: the same text phrase twice means eight entries across four voices in imitative setting.

**Refrain**

> Kael, kael, sera kael,
> ana-ri mila-mi rase-thir.
> Kael, kael, sera kael,
> mòru-mu eirith-mi rase-thir.

*Song, song, sacred song,*
*spirits sing in the peace, eternally.*
*Song, song, sacred song,*
*in the forest, in eternity, singing.*

> CHORUS: ***Thol...***

### 10.2 "Thír-Rìne" — Star-Sorrow

*A lament with evidential shifts: remembered → searching → spiritual → direct knowing.*

> Thír-ri shaune-shi ná-se,
> fàni, ke néla ná ere-shi —
> léshi-we ana-no en-waithe-shi-en,
> fola-we něma-no en-thore-shi-en.

*The stars shone for me,*
*long ago, when I was young —*
*with light, my spirit was woven (I remember),*
*with love, my dreams were crafted (I remember).*

> Lóshi rìne sha mila niwe-ren.
> Sena sha fola niwe-ren.
> Thei raise-shi ain thairn?
> Thei en-thaile-shi ain kael?

*Now sorrow dwells within the peace.*
*Memory dwells within love.*
*Who sang before the void?*
*Who was named before song?*

> Meire-shi-âith ná: màured sha eirith —
> thír-ri shine-thir.
> Nì rìni-ke leru-li léshi.
> Nì rìni-ke.

*I have beheld (through spiritual knowing): death within eternity —*
*the stars shine on.*
*Their light will never fall.*
*Never fall.*

The peaking tone on *-âith* (spiritual intuited evidential) creates a 3-note melisma on the word that reveals the speaker's deepest insight — the music swells at the moment of revelation, then settles. The repeated *nì rìni-ke* at the end: nì (falling) rìni (falling-level) ke (level) — a cascade of descending tones trailing off.

### 10.3 "Wethe-Kael" — The Weaving Chant

*Tight imitative polyphony. Short lines, high repetition.*

> Wethe-ló, wethe-ló,
> lena na sela,
> hàla na féri,
> wethe-ló, wethe-ló.

*Weave, weave,*
*leaf and blossom,*
*branch and vine,*
*weave, weave.*

> Rase-ló, rase-ló,
> kael na ana,
> léshi na fola,
> rase-ló, rase-ló.

*Sing, sing,*
*song and spirit,*
*light and love,*
*sing, sing.*

> Kóse-ló, kóse-ló,
> aleth na thuri,
> shela na kola,
> kóse-ló, kóse-ló.

*Grow, grow,*
*tree and root,*
*sky and earth,*
*grow, grow.*

> ALL VOICES: ***Thol.***

Tone analysis: The imperative verbs end in *-ló* (rising) — each command lifts. The noun pairs in the middle lines (*lena na sela*, *kael na ana*) are almost entirely level — maximally free for the optimizer. The third stanza shifts: *kóse* has a rising root, so *kóse-ló* has two rising syllables — extra lift for "grow!"

### 10.4 "Dàren-Kael" — The Exile's Song

*Narrative, dark, dramatic. Uses dwarven loanwords and all four word orders.*

> Shàdur-mu naive-shi-en ná.
> Aleth-ri rè ere-shi — rè na vàire.
> Úrith-we nì dave-shi ná —
> veir-we dave-shi.

*In the ruin I dwelt (I remember).*
*The trees were ash — ash and empty silence.*
*Not with a blade did I endure —*
*with voice I endured.*

> Thàur-li ana-no kógan-mi kethe-shi ná.
> Kógan thana, kógan dave-ren.
> En-raise-shi ná, thairn-se en-raise-shi —
> na thairn meire-shi-âith: vàire eirith nère-thir.

*Against betrayal, I held my spirit in a vessel.*
*An ancient vessel, a vessel that endures.*
*I was moved to sing — into the void, I was moved to sing —*
*and the void beheld (through spirit): emptiness is not eternal.*

> Lóshi aleth-ri néla kóse-ren.
> Lóshi kael áiren-mi ere-ren.
> Dàren-li ná — sera ná ere-thir dai.
> Rase-ren ná. Rase-thir.

*Now new trees are growing.*
*Now song exists in the canopy.*
*I who was exiled — I am sacred, truly, forever.*
*I am singing. Singing eternally.*

The final line: *Rase-thir.* One word. Level-level. Two syllables of total musical freedom — the optimizer can set this however the counterpoint demands. The meaning is enormous ("singing, forever, as cosmic truth") but the phonology is light and unconstrained, like a breath after a storm.

---

## 11. Vocal Synthesis Notes

### 11.1 Recording Inventory

| Category | Count | Examples |
|----------|-------|---------|
| CV syllables (15C × 5V) — unstressed (tap r) | 75 | ra, thi, ke, so, fu... |
| CV syllables with trilled r (stressed variant) | 5 | ra, re, ri, ro, ru (trill) |
| V-only (word-initial) | 5 | a, e, i, o, u |
| "dh" syllables | 5 | dha, dhe, dhi, dho, dhu |
| Diphthong syllables (common onsets × 5 diphthongs) | ~30 | thai, rai, kau, lei, shoi, kia... |
| Closed-syllable codas (archaic/loan words) | ~12 | -el, -ir, -ith, -eim, -an, -ren, -eth... |
| **Total** | **~132** | |

At ~0.4 seconds per snippet: **under 55 seconds of raw audio.** Perhaps 20 minutes of studio recording time.

### 11.2 Tone Realization in Synthesis

Tones are realized by applying a **pitch envelope** (a curve, not a flat shift) to each recorded snippet:

| Tone | Pitch Envelope |
|------|---------------|
| Level | Flat — constant pitch throughout |
| Rising | Ascending ramp — starts lower, ends higher |
| Falling | Descending ramp — starts higher, ends lower |
| Dipping | V-shaped — descends to a low point, then ascends |
| Peaking | Inverted-V — ascends to a high point, then descends |

For rising/falling tones, the snippet is stretched to accommodate 2 notes. For dipping/peaking, stretched to 3 notes. The pitch envelope determines the shape within that duration.

This is slightly more complex than flat pitch-shifting but well within what vocal synthesis engines (including Vocaloid-style systems) routinely handle.

### 11.3 The Dual R

Two /r/ recordings: tap for unstressed, trill for stressed. Selection is automatic from the stress rule (always known: first syllable of root). The trill can be looped for emphatic passages.

### 11.4 Transitions

- **CV → CV:** Vowel-to-consonant — clean crossfade (overlap-add).
- **Diphthong syllables:** Recorded as complete units — internal glide is captured.
- **Closed → CV:** Consonant-to-consonant — slightly harder. Pre-record common pairs (el-th, ir-sh) for smooth joins. Only affects archaic/loan words.

---

## 12. Quick Grammar Reference

```
NOUN: stem + (number) + (case)
  Number:  -ri/-ru (PL), -neth/-noth (COLL, archaic)
  Case:    -∅ (NOM), -ne/-no (ACC), -li/-lu (GEN), -se/-so (DAT),
           -mi/-mu (LOC), -we/-wo (INST), -léi/-lái (VOC, rising)
  All case suffixes level except vocative (rising).
  Harmony: front (e,i stems) / back (o,u stems) / a defaults to front.

VERB: (voice) + stem + aspect + (evidential) + (mood)
  Voice:   en-/an- (middle/passive, level)
  Aspect:  -ren/-ran (IMPF, level), -shi/-shu (PERF, level),
           -kél/-kál (INCEP, rising), -tha (HAB, level),
           -mèil/-màil (CESS, falling), -thir/-thur (ETER, level)
  Evid:    -∅ (direct), -en/-an (remembered, level),
           -vei (told, level), -âith (intuited, peaking)
  Mood:    -ló/-lú (IMP, rising), -me/-mo (OPT, level),
           -shan/-shun (COND, level)
  12 strong verbs: ablaut (stem vowel → diphthong) in perfective.
  Tone transfers to diphthong unchanged.

COPULA: ere (level). Negative: nère (falling). Suppletive.

PRONOUNS: ná (I, rising), thé (you.SG, rising), le (3.SG, level)
  náire (we.INCL), náli (we.EXCL), théri (you.PL), leru (they)
  Front harmony. Standard case suffixes.

QUESTIONS: vái (rising). Position = register:
  Initial = deferential, pre-verb = neutral, final = familiar.
  Interrogatives: mái (what), thei (who), hàmi (where),
                  àke (when), théla (why), séve (how).

SUBORDINATION: ke (when/if), e (that/which), sha-ke (because),
  nei (although), la-se (in order to), dha (while). All level.

WORD ORDER: SOV default. Free reordering via case marking.
  SOV=neutral, OSV=topic, VSO=dramatic, OVS=revelation.

STRESS: First syllable of root, always. Suffixes never stressed.

TONES: 5 contour tones, realized WITHIN the syllable:
  Level (a) = held steady, 1 note min.
  Rising (á) = pitch ascends, 2 notes min.
  Falling (à) = pitch descends, 2 notes min.
  Dipping (ǎ) = down-up, 3 notes min.
  Peaking (â) = up-down, 3 notes min.
  Every morpheme has a fixed, lexically specified tone.
  No relaxation rules. What you see is what you sing.

CALL-AND-RESPONSE:
  Dai (level) = short homophonic affirmation.
  Thol (level) = sustained structural breathing chord.

POETIC ELISION: Drop case suffix / evidential / pronoun /
  compress compounds. Sacred/poetic register only.
```
