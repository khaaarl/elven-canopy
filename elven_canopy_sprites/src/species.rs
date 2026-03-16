// Creature species sprite generation — dispatcher and per-species modules.
//
// Each of the 10 species has a `*_params_from_seed` function that maps an
// integer seed to deterministic visual parameters (colors, accessories, etc.)
// via Knuth multiplicative hashing, and a `create_*` function that draws
// the sprite pixel-by-pixel into a `PixelBuffer`.
//
// The `SpriteParams` enum wraps per-species params, and the top-level
// `species_params_from_seed` / `create_species_sprite` functions dispatch
// by `Species` variant.
//
// See also: `drawing.rs` for the PixelBuffer, `color.rs` for Color.

mod boar;
mod capybara;
mod deer;
mod elephant;
mod elf;
mod goblin;
mod monkey;
mod orc;
mod squirrel;
mod troll;

use elven_canopy_sim::types::Species;

use crate::drawing::PixelBuffer;

/// Knuth multiplicative hash constant (2654435761), used to spread bits
/// from integer seeds before modular indexing into palette arrays.
/// Matches the GDScript `absi(seed * 2654435761)` pattern.
pub(crate) fn knuth_hash(seed: i64) -> u64 {
    (seed.wrapping_mul(2_654_435_761)).unsigned_abs()
}

/// Per-species sprite parameters, produced by `species_params_from_seed`.
#[derive(Clone, Debug)]
pub enum SpriteParams {
    Elf(elf::ElfParams),
    Capybara(capybara::CapybaraParams),
    Boar(boar::BoarParams),
    Deer(deer::DeerParams),
    Monkey(monkey::MonkeyParams),
    Squirrel(squirrel::SquirrelParams),
    Elephant(elephant::ElephantParams),
    Goblin(goblin::GoblinParams),
    Orc(orc::OrcParams),
    Troll(troll::TrollParams),
}

/// Build a deterministic params from a species and integer seed.
pub fn species_params_from_seed(species: Species, seed: i64) -> SpriteParams {
    match species {
        Species::Elf => SpriteParams::Elf(elf::params_from_seed(seed)),
        Species::Capybara => SpriteParams::Capybara(capybara::params_from_seed(seed)),
        Species::Boar => SpriteParams::Boar(boar::params_from_seed(seed)),
        Species::Deer => SpriteParams::Deer(deer::params_from_seed(seed)),
        Species::Monkey => SpriteParams::Monkey(monkey::params_from_seed(seed)),
        Species::Squirrel => SpriteParams::Squirrel(squirrel::params_from_seed(seed)),
        Species::Elephant => SpriteParams::Elephant(elephant::params_from_seed(seed)),
        Species::Goblin => SpriteParams::Goblin(goblin::params_from_seed(seed)),
        Species::Orc => SpriteParams::Orc(orc::params_from_seed(seed)),
        Species::Troll => SpriteParams::Troll(troll::params_from_seed(seed)),
    }
}

/// Create a sprite for any species. Returns a PixelBuffer of the appropriate
/// dimensions for the species.
pub fn create_species_sprite(params: &SpriteParams) -> PixelBuffer {
    match params {
        SpriteParams::Elf(p) => elf::create_sprite(p),
        SpriteParams::Capybara(p) => capybara::create_sprite(p),
        SpriteParams::Boar(p) => boar::create_sprite(p),
        SpriteParams::Deer(p) => deer::create_sprite(p),
        SpriteParams::Monkey(p) => monkey::create_sprite(p),
        SpriteParams::Squirrel(p) => squirrel::create_sprite(p),
        SpriteParams::Elephant(p) => elephant::create_sprite(p),
        SpriteParams::Goblin(p) => goblin::create_sprite(p),
        SpriteParams::Orc(p) => orc::create_sprite(p),
        SpriteParams::Troll(p) => troll::create_sprite(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knuth_hash_deterministic() {
        assert_eq!(knuth_hash(42), knuth_hash(42));
        assert_ne!(knuth_hash(1), knuth_hash(2));
    }

    const ALL_SPECIES: [Species; 10] = [
        Species::Elf,
        Species::Capybara,
        Species::Boar,
        Species::Deer,
        Species::Monkey,
        Species::Squirrel,
        Species::Elephant,
        Species::Goblin,
        Species::Orc,
        Species::Troll,
    ];

    #[test]
    fn all_species_produce_nonempty_sprites() {
        for species in ALL_SPECIES {
            let params = species_params_from_seed(species, 12345);
            let buf = create_species_sprite(&params);
            assert!(buf.width() > 0);
            assert!(buf.height() > 0);
            // At least some pixels should be non-transparent.
            let data = buf.data();
            let has_opaque = data.chunks(4).any(|px| px[3] > 0);
            assert!(has_opaque, "{species:?} sprite is completely transparent");
        }
    }

    #[test]
    fn params_from_seed_deterministic() {
        for species in ALL_SPECIES {
            let p1 = species_params_from_seed(species, 999);
            let p2 = species_params_from_seed(species, 999);
            let b1 = create_species_sprite(&p1);
            let b2 = create_species_sprite(&p2);
            assert_eq!(b1.data(), b2.data(), "{species:?} not deterministic");
        }
    }

    #[test]
    fn different_seeds_produce_different_sprites() {
        // At least some species should produce different sprites for different seeds.
        let mut any_different = false;
        for species in ALL_SPECIES {
            let b1 = create_species_sprite(&species_params_from_seed(species, 1));
            let b2 = create_species_sprite(&species_params_from_seed(species, 2));
            if b1.data() != b2.data() {
                any_different = true;
                break;
            }
        }
        assert!(
            any_different,
            "No species produced different sprites for different seeds"
        );
    }

    #[test]
    fn species_dimensions_match_expected() {
        let cases = [
            (Species::Elf, 48, 48),
            (Species::Capybara, 40, 32),
            (Species::Boar, 44, 36),
            (Species::Deer, 44, 44),
            (Species::Monkey, 40, 44),
            (Species::Squirrel, 32, 32),
            (Species::Elephant, 96, 80),
            (Species::Goblin, 36, 40),
            (Species::Orc, 48, 48),
            (Species::Troll, 96, 80),
        ];
        for (species, w, h) in cases {
            let buf = create_species_sprite(&species_params_from_seed(species, 1));
            assert_eq!(
                (buf.width(), buf.height()),
                (w, h),
                "{species:?} dimensions wrong"
            );
        }
    }
}
