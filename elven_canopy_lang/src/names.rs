// Vaelith name generator: creates elvish names from lexicon entries.
//
// Names are compounds of 1-2 roots drawn from the lexicon, filtered by
// `NameTag` (given vs surname). The generator takes `&mut GameRng` for
// deterministic output, matching the sim's determinism constraint.
//
// Used by `elven_canopy_sim` to name creatures at spawn time. Each name
// carries both the Vaelith text and an English meaning gloss (e.g.,
// "star-tree" for "Thíraleth").
//
// Depends on `types.rs` for `LexEntry`/`NameTag` and `lib.rs` for `Lexicon`.

use crate::types::NameTag;
use crate::Lexicon;
use elven_canopy_prng::GameRng;

/// A generated Vaelith name with given name, surname, and meanings.
#[derive(Debug, Clone)]
pub struct VaelithName {
    /// Full name: "Given Surname".
    pub full_name: String,
    /// Given (first) name.
    pub given: String,
    /// Surname (family/clan name).
    pub surname: String,
    /// English meaning of the given name (e.g., "star-tree").
    pub given_meaning: String,
    /// English meaning of the surname (e.g., "song-glow").
    pub surname_meaning: String,
}

/// Generate a Vaelith name from the lexicon.
///
/// Algorithm for each name part (given/surname):
/// 1. Pick root count: ~70% two roots, ~30% one root
/// 2. Pick root(s) uniformly from the pool filtered by the appropriate NameTag
/// 3. Concatenate root texts, capitalize first letter
/// 4. Meaning is "gloss1-gloss2" or just "gloss1"
pub fn generate_name(lexicon: &Lexicon, rng: &mut GameRng) -> VaelithName {
    let given_pool = lexicon.by_name_tag(NameTag::Given);
    let surname_pool = lexicon.by_name_tag(NameTag::Surname);

    let given = generate_name_part(&given_pool, rng);
    let surname = generate_name_part(&surname_pool, rng);

    VaelithName {
        full_name: format!("{} {}", given.0, surname.0),
        given: given.0,
        surname: surname.0,
        given_meaning: given.1,
        surname_meaning: surname.1,
    }
}

/// Generate one name part (given or surname) from a pool of entries.
/// Returns (name_text, meaning).
fn generate_name_part(pool: &[&crate::types::LexEntry], rng: &mut GameRng) -> (String, String) {
    if pool.is_empty() {
        return ("Unnamed".to_string(), "unknown".to_string());
    }

    // ~70% two roots, ~30% one root
    let two_roots = rng.next_f64() < 0.7;

    if two_roots && pool.len() >= 2 {
        let idx1 = rng.range_usize(0, pool.len());
        let mut idx2 = rng.range_usize(0, pool.len());
        // Retry if duplicate (up to 3 attempts)
        for _ in 0..3 {
            if idx2 != idx1 {
                break;
            }
            idx2 = rng.range_usize(0, pool.len());
        }

        let entry1 = pool[idx1];
        let entry2 = pool[idx2];

        let combined = format!("{}{}", entry1.root, entry2.root);
        let name = capitalize(&combined);
        let meaning = format!("{}-{}", entry1.gloss, entry2.gloss);
        (name, meaning)
    } else {
        let idx = rng.range_usize(0, pool.len());
        let entry = pool[idx];
        let name = capitalize(&entry.root);
        let meaning = entry.gloss.clone();
        (name, meaning)
    }
}

/// Capitalize the first character of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            format!("{}{}", upper, chars.as_str())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("aleth"), "Aleth");
        assert_eq!(capitalize("thír"), "Thír");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("A"), "A");
    }

    #[test]
    fn test_generate_name_deterministic() {
        let json = include_str!("../../data/vaelith_lexicon.json");
        let lexicon = Lexicon::from_json(json).unwrap();
        let mut rng1 = GameRng::new(42);
        let mut rng2 = GameRng::new(42);

        let name1 = generate_name(&lexicon, &mut rng1);
        let name2 = generate_name(&lexicon, &mut rng2);

        assert_eq!(name1.full_name, name2.full_name);
        assert_eq!(name1.given_meaning, name2.given_meaning);
        assert_eq!(name1.surname_meaning, name2.surname_meaning);
    }

    #[test]
    fn test_generate_name_nonempty() {
        let json = include_str!("../../data/vaelith_lexicon.json");
        let lexicon = Lexicon::from_json(json).unwrap();
        let mut rng = GameRng::new(123);

        let name = generate_name(&lexicon, &mut rng);

        assert!(!name.full_name.is_empty());
        assert!(!name.given.is_empty());
        assert!(!name.surname.is_empty());
        assert!(!name.given_meaning.is_empty());
        assert!(!name.surname_meaning.is_empty());
        assert!(name.full_name.contains(' '));
    }

    #[test]
    fn test_generate_name_capitalized() {
        let json = include_str!("../../data/vaelith_lexicon.json");
        let lexicon = Lexicon::from_json(json).unwrap();
        let mut rng = GameRng::new(99);

        for seed in 0..20 {
            let mut rng = GameRng::new(seed);
            let name = generate_name(&lexicon, &mut rng);
            assert!(
                name.given.starts_with(|c: char| c.is_uppercase()),
                "Given name '{}' should start with uppercase",
                name.given
            );
            assert!(
                name.surname.starts_with(|c: char| c.is_uppercase()),
                "Surname '{}' should start with uppercase",
                name.surname
            );
        }
        // Use the rng we declared
        let _ = generate_name(&lexicon, &mut rng);
    }

    #[test]
    fn test_generate_name_variety() {
        let json = include_str!("../../data/vaelith_lexicon.json");
        let lexicon = Lexicon::from_json(json).unwrap();

        let mut names = std::collections::BTreeSet::new();
        for seed in 0..50 {
            let mut rng = GameRng::new(seed);
            let name = generate_name(&lexicon, &mut rng);
            names.insert(name.full_name);
        }

        // With 50 seeds we should get a good variety of unique names
        assert!(
            names.len() > 20,
            "Expected >20 unique names from 50 seeds, got {}",
            names.len()
        );
    }
}
