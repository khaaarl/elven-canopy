// Creature species sprite generation — dispatcher and per-species modules.
//
// Each species has `params_from_seed` (backward-compatible, seed → visual
// parameters via Knuth hashing) and `params_from_traits` (preferred path,
// reads palette indices from a `TraitMap` populated by the sim's
// `creature_traits` table). The `create_*` functions draw sprites
// pixel-by-pixel into a `PixelBuffer`.
//
// `TraitMap` is a `BTreeMap<TraitKind, TraitValue>` — the sprite crate's
// read-only view of a creature's biological traits. `trait_idx` extracts
// an integer trait as a `usize`, falling back to a default on missing or
// wrong-type values.
//
// See also: `drawing.rs` for the PixelBuffer, `color.rs` for Color.

mod boar;
mod capybara;
mod deer;
mod elephant;
pub mod elf;
mod elf_equipment;
mod goblin;
mod hornet;
mod monkey;
mod orc;
mod squirrel;
mod troll;
mod wyvern;

use elven_canopy_sim::types::{Species, TraitKind, TraitValue};
use std::collections::BTreeMap;

use crate::drawing::PixelBuffer;

/// Read-only view of a creature's biological traits, populated from the
/// `creature_traits` table in the sim.
pub type TraitMap = BTreeMap<TraitKind, TraitValue>;

/// Extract an integer trait as a `usize`, returning `default` if the trait
/// is missing or holds a non-integer value.
pub(crate) fn trait_idx(traits: &TraitMap, kind: TraitKind, default: usize) -> usize {
    traits
        .get(&kind)
        .and_then(|v| match v {
            TraitValue::Int(i) => Some(*i as usize),
            TraitValue::Text(_) => None,
        })
        .unwrap_or(default)
}

/// Extract a signed integer trait, returning `default` if absent.
/// Used for VSH pigmentation axes (value, saturation, melanin, etc.)
/// which are centered on 0.
pub(crate) fn trait_i64(traits: &TraitMap, kind: TraitKind, default: i64) -> i64 {
    traits
        .get(&kind)
        .and_then(|v| match v {
            TraitValue::Int(i) => Some(*i),
            TraitValue::Text(_) => None,
        })
        .unwrap_or(default)
}

/// Knuth multiplicative hash constant (2654435761), used to spread bits
/// from integer seeds before modular indexing into palette arrays.
/// Matches the GDScript `absi(seed * 2654435761)` pattern.
pub(crate) fn knuth_hash(seed: i64) -> u64 {
    (seed.wrapping_mul(2_654_435_761)).unsigned_abs()
}

/// Resolve a hue color from a palette with optional blending. If `blend_target`
/// is a valid palette index and `blend_weight > 0`, blends between primary
/// and secondary hue colors before applying VSH adjustments.
pub(crate) fn resolve_hue(
    palette: &[crate::color::Color],
    primary_idx: usize,
    blend_target: i64,
    blend_weight: i64,
) -> crate::color::Color {
    let base = palette[primary_idx % palette.len()];
    if blend_target >= 0 && blend_weight > 0 {
        let secondary = palette[blend_target as usize % palette.len()];
        base.blend(secondary, blend_weight.clamp(0, 255) as u8)
    } else {
        base
    }
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
    Hornet(hornet::HornetParams),
    Wyvern(wyvern::WyvernParams),
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
        Species::Hornet => SpriteParams::Hornet(hornet::params_from_seed(seed)),
        Species::Wyvern => SpriteParams::Wyvern(wyvern::params_from_seed(seed)),
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
        SpriteParams::Hornet(p) => hornet::create_sprite(p),
        SpriteParams::Wyvern(p) => wyvern::create_sprite(p),
    }
}

/// Build species params from biological trait data (the preferred path).
/// Falls back to default palette indices for missing traits.
pub fn species_params_from_traits(species: Species, traits: &TraitMap) -> SpriteParams {
    match species {
        Species::Elf => SpriteParams::Elf(elf::params_from_traits(traits)),
        Species::Capybara => SpriteParams::Capybara(capybara::params_from_traits(traits)),
        Species::Boar => SpriteParams::Boar(boar::params_from_traits(traits)),
        Species::Deer => SpriteParams::Deer(deer::params_from_traits(traits)),
        Species::Monkey => SpriteParams::Monkey(monkey::params_from_traits(traits)),
        Species::Squirrel => SpriteParams::Squirrel(squirrel::params_from_traits(traits)),
        Species::Elephant => SpriteParams::Elephant(elephant::params_from_traits(traits)),
        Species::Goblin => SpriteParams::Goblin(goblin::params_from_traits(traits)),
        Species::Orc => SpriteParams::Orc(orc::params_from_traits(traits)),
        Species::Troll => SpriteParams::Troll(troll::params_from_traits(traits)),
        Species::Hornet => SpriteParams::Hornet(hornet::params_from_traits(traits)),
        Species::Wyvern => SpriteParams::Wyvern(wyvern::params_from_traits(traits)),
    }
}

/// Create a fully composited elf sprite from a `CreatureDrawInfo`.
/// Delegates to `elf::create_creature_sprite`.
pub fn create_creature_sprite(info: &elf::CreatureDrawInfo) -> PixelBuffer {
    elf::create_creature_sprite(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    #[test]
    fn resolve_hue_no_blend() {
        let palette = [Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 1.0, 0.0)];
        let result = resolve_hue(&palette, 0, -1, 0);
        assert_eq!(result, palette[0]);
    }

    #[test]
    fn resolve_hue_with_blend() {
        let palette = [
            Color::from_u8(200, 0, 0, 255),
            Color::from_u8(0, 200, 0, 255),
        ];
        let result = resolve_hue(&palette, 0, 1, 128);
        // Should be roughly midpoint between red and green.
        assert!(result.r > 80 && result.r < 120, "blended r={}", result.r);
        assert!(result.g > 80 && result.g < 120, "blended g={}", result.g);
    }

    #[test]
    fn resolve_hue_zero_weight_no_blend() {
        let palette = [Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 1.0, 0.0)];
        let result = resolve_hue(&palette, 0, 1, 0);
        assert_eq!(result, palette[0], "weight 0 should not blend");
    }

    #[test]
    fn resolve_hue_wraps_index() {
        let palette = [Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 1.0, 0.0)];
        let result = resolve_hue(&palette, 5, -1, 0); // 5 % 2 = 1
        assert_eq!(result, palette[1]);
    }

    #[test]
    fn knuth_hash_deterministic() {
        assert_eq!(knuth_hash(42), knuth_hash(42));
        assert_ne!(knuth_hash(1), knuth_hash(2));
    }

    const ALL_SPECIES: [Species; 12] = [
        Species::Elf,
        Species::Capybara,
        Species::Boar,
        Species::Deer,
        Species::Monkey,
        Species::Squirrel,
        Species::Elephant,
        Species::Goblin,
        Species::Orc,
        Species::Hornet,
        Species::Troll,
        Species::Wyvern,
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
    fn all_species_produce_nonempty_sprites_from_traits() {
        use elven_canopy_sim::types::{TraitKind, TraitValue};

        for species in ALL_SPECIES {
            let mut traits = TraitMap::new();
            // Insert a few generic traits — species will ignore irrelevant ones.
            traits.insert(TraitKind::HairColor, TraitValue::Int(2));
            traits.insert(TraitKind::EyeColor, TraitValue::Int(1));
            traits.insert(TraitKind::SkinTone, TraitValue::Int(0));
            traits.insert(TraitKind::HairStyle, TraitValue::Int(1));
            traits.insert(TraitKind::BodyColor, TraitValue::Int(1));
            traits.insert(TraitKind::SkinColor, TraitValue::Int(1));
            traits.insert(TraitKind::FurColor, TraitValue::Int(1));
            traits.insert(TraitKind::Accessory, TraitValue::Int(0));
            traits.insert(TraitKind::TuskSize, TraitValue::Int(0));
            traits.insert(TraitKind::AntlerStyle, TraitValue::Int(0));
            traits.insert(TraitKind::SpotPattern, TraitValue::Int(0));
            traits.insert(TraitKind::TuskType, TraitValue::Int(0));
            traits.insert(TraitKind::EarStyle, TraitValue::Int(0));
            traits.insert(TraitKind::FaceMarking, TraitValue::Int(0));
            traits.insert(TraitKind::WarPaint, TraitValue::Int(0));
            traits.insert(TraitKind::TailType, TraitValue::Int(0));
            traits.insert(TraitKind::HornStyle, TraitValue::Int(0));

            let params = species_params_from_traits(species, &traits);
            let buf = create_species_sprite(&params);
            assert!(buf.width() > 0);
            assert!(buf.height() > 0);
            let data = buf.data();
            let has_opaque = data.chunks(4).any(|px| px[3] > 0);
            assert!(
                has_opaque,
                "{species:?} trait-based sprite is completely transparent"
            );
        }
    }

    #[test]
    fn empty_trait_map_produces_valid_sprites() {
        // params_from_traits with empty map should fall back to defaults.
        for species in ALL_SPECIES {
            let traits = TraitMap::new();
            let params = species_params_from_traits(species, &traits);
            let buf = create_species_sprite(&params);
            assert!(buf.width() > 0, "{species:?} empty traits failed");
        }
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
            (Species::Hornet, 36, 32),
            (Species::Wyvern, 96, 80),
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
