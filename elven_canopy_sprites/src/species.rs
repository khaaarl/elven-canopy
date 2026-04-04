// Creature species sprite generation — dispatcher and per-species modules.
//
// Each species has `params_from_traits`, which reads palette indices from a
// `TraitMap` populated by the sim's `creature_traits` table. The `create_*`
// functions draw sprites pixel-by-pixel into a `PixelBuffer`.
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

/// Per-species sprite parameters, produced by `species_params_from_traits`.
#[derive(Clone, Debug, PartialEq)]
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

/// Create a fully composited creature sprite: base species sprite plus
/// equipment overlays for any species that has overlay art. Currently only
/// elves have equipment overlay drawing; other species ignore the equipment
/// array. The entry point is species-agnostic — adding equipment art for a
/// new species is a match arm, not an architectural change.
pub fn create_sprite_with_equipment(
    params: &SpriteParams,
    equipment: &[Option<elf::EquipSlotDrawInfo>; elven_canopy_sim::inventory::EquipSlot::COUNT],
) -> PixelBuffer {
    let mut buf = create_species_sprite(params);
    if let SpriteParams::Elf(_) = params {
        elf::apply_equipment_overlays(&mut buf, equipment);
    }
    buf
}

/// Build species params from biological trait data. Falls back to default
/// palette indices for missing traits.
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
        let traits = TraitMap::new();
        for (species, w, h) in cases {
            let buf = create_species_sprite(&species_params_from_traits(species, &traits));
            assert_eq!(
                (buf.width(), buf.height()),
                (w, h),
                "{species:?} dimensions wrong"
            );
        }
    }

    #[test]
    fn params_from_traits_deterministic() {
        use elven_canopy_sim::types::{TraitKind, TraitValue};

        for species in ALL_SPECIES {
            let mut traits = TraitMap::new();
            traits.insert(TraitKind::BodyColor, TraitValue::Int(2));
            traits.insert(TraitKind::FurColor, TraitValue::Int(1));
            traits.insert(TraitKind::SkinColor, TraitValue::Int(1));
            traits.insert(TraitKind::HairColor, TraitValue::Int(3));
            traits.insert(TraitKind::EyeColor, TraitValue::Int(1));
            traits.insert(TraitKind::SkinTone, TraitValue::Int(2));
            traits.insert(TraitKind::HairStyle, TraitValue::Int(0));
            traits.insert(TraitKind::Accessory, TraitValue::Int(1));
            traits.insert(TraitKind::TuskSize, TraitValue::Int(2));

            let p1 = species_params_from_traits(species, &traits);
            let p2 = species_params_from_traits(species, &traits);
            assert_eq!(
                p1, p2,
                "{species:?}: same traits must produce equal SpriteParams"
            );
        }
    }

    #[test]
    fn different_traits_produce_different_params() {
        use elven_canopy_sim::types::{TraitKind, TraitValue};

        // Every species should produce different params when its primary
        // color trait changes. Each species uses one of HairColor,
        // BodyColor, FurColor, or SkinColor as its main visual axis.
        let species_color_trait: [(Species, TraitKind); 12] = [
            (Species::Elf, TraitKind::HairColor),
            (Species::Capybara, TraitKind::BodyColor),
            (Species::Boar, TraitKind::BodyColor),
            (Species::Deer, TraitKind::BodyColor),
            (Species::Monkey, TraitKind::FurColor),
            (Species::Squirrel, TraitKind::FurColor),
            (Species::Elephant, TraitKind::BodyColor),
            (Species::Goblin, TraitKind::SkinColor),
            (Species::Orc, TraitKind::SkinColor),
            (Species::Troll, TraitKind::SkinColor),
            (Species::Hornet, TraitKind::BodyColor),
            (Species::Wyvern, TraitKind::BodyColor),
        ];
        for (species, color_kind) in species_color_trait {
            let mut traits_a = TraitMap::new();
            let mut traits_b = TraitMap::new();
            traits_a.insert(color_kind, TraitValue::Int(0));
            traits_b.insert(color_kind, TraitValue::Int(2));
            let pa = species_params_from_traits(species, &traits_a);
            let pb = species_params_from_traits(species, &traits_b);
            assert_ne!(
                pa, pb,
                "{species:?} should produce different params for different {color_kind:?} values"
            );
        }
    }

    #[test]
    fn create_sprite_with_equipment_non_elf_ignores_equipment() {
        use elven_canopy_sim::inventory::{EquipSlot, WearCategory};
        use elven_canopy_sim::types::{TraitKind, TraitValue};

        let fake_equip: [Option<elf::EquipSlotDrawInfo>; EquipSlot::COUNT] = {
            let mut arr: [Option<elf::EquipSlotDrawInfo>; EquipSlot::COUNT] =
                std::array::from_fn(|_| None);
            arr[0] = Some(elf::EquipSlotDrawInfo {
                kind: elven_canopy_sim::inventory::ItemKind::Tunic,
                color: crate::Color::from_u8(200, 50, 50, 255),
                wear: WearCategory::Good,
            });
            arr
        };

        let non_elf_species = [
            Species::Capybara,
            Species::Boar,
            Species::Deer,
            Species::Monkey,
            Species::Squirrel,
            Species::Elephant,
            Species::Goblin,
            Species::Orc,
            Species::Troll,
            Species::Hornet,
            Species::Wyvern,
        ];
        for species in non_elf_species {
            let mut traits = TraitMap::new();
            traits.insert(TraitKind::BodyColor, TraitValue::Int(1));
            let params = species_params_from_traits(species, &traits);
            let bare = create_species_sprite(&params);
            let equipped = create_sprite_with_equipment(&params, &fake_equip);
            assert_eq!(
                bare.data(),
                equipped.data(),
                "{species:?}: equipment must not affect non-elf sprites"
            );
        }
    }

    #[test]
    fn create_sprite_with_equipment_elf_applies_overlays() {
        use elven_canopy_sim::inventory::{EquipSlot, WearCategory};

        let no_equip: [Option<elf::EquipSlotDrawInfo>; EquipSlot::COUNT] =
            std::array::from_fn(|_| None);
        let mut with_equip = no_equip.clone();
        with_equip[0] = Some(elf::EquipSlotDrawInfo {
            kind: elven_canopy_sim::inventory::ItemKind::Tunic,
            color: crate::Color::from_u8(200, 50, 50, 255),
            wear: WearCategory::Good,
        });

        let params = species_params_from_traits(Species::Elf, &TraitMap::new());
        let bare = create_sprite_with_equipment(&params, &no_equip);
        let equipped = create_sprite_with_equipment(&params, &with_equip);
        assert_ne!(
            bare.data(),
            equipped.data(),
            "Elf sprite with equipment must differ from bare"
        );
    }
}
