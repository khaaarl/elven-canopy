// Chibi elf sprite generation (48x48).
//
// Generates a deterministic chibi-style elf with hair color/style, eye color,
// and skin tone. The base sprite is bare-skinned — all clothing and armor
// visuals come from equipment overlays drawn by `elf_equipment.rs`.
//
// `create_base_sprite` produces the unclothed elf. Equipment compositing is
// handled by `apply_equipment_overlays`, which layers slot-ordered overlays
// onto a base sprite. The species-agnostic entry point is
// `create_sprite_with_equipment` in `species.rs`. The old `create_sprite`
// function calls `create_base_sprite` for backward compatibility (e.g.,
// elfcyclopedia).
//
// See also: `elf_equipment.rs` for equipment overlay drawing, `color.rs`
// for the ItemColor→Color conversion, `species.rs` for the dispatcher.

use super::elf_equipment;
use crate::color::Color;
use crate::drawing::PixelBuffer;
use elven_canopy_sim::inventory::{EquipSlot, ItemKind, WearCategory};
use elven_canopy_sim::types::TraitKind;

/// Base hue palette for elf hair. Arranged in hue-wheel order so that
/// adjacent categories can blend to produce intermediate colors (future
/// hue blending feature). The VSH pigmentation axes (Value/Saturation)
/// shift these base hues — e.g., low saturation + high value = silver/ash,
/// low saturation + medium value = brown, very low value = near-black.
const HAIR_COLORS: [Color; 7] = [
    Color::rgb(0.95, 0.80, 0.25), // gold — warm golden blonde
    Color::rgb(0.85, 0.40, 0.20), // copper — auburn/orange-red
    Color::rgb(0.90, 0.45, 0.55), // rose — vivid pink
    Color::rgb(0.65, 0.35, 0.85), // violet — rich purple
    Color::rgb(0.30, 0.50, 0.90), // blue — cool blue
    Color::rgb(0.25, 0.70, 0.65), // teal — blue-green
    Color::rgb(0.30, 0.70, 0.30), // green — emerald/forest
];

/// Base hue palette for elf eyes. Hue-wheel order (warm→cool arc) for
/// adjacent-category blending. Grey/dark eyes emerge from VSH axes.
const EYE_COLORS: [Color; 6] = [
    Color::rgb(0.85, 0.65, 0.20), // amber — warm golden
    Color::rgb(0.25, 0.72, 0.35), // green — vivid emerald
    Color::rgb(0.20, 0.65, 0.70), // teal — blue-green
    Color::rgb(0.30, 0.45, 0.90), // blue — cool blue
    Color::rgb(0.60, 0.30, 0.80), // violet — purple
    Color::rgb(0.80, 0.40, 0.55), // rose — pinkish fantasy
];

const SKIN_TONES: [Color; 4] = [
    Color::rgb(0.93, 0.80, 0.65), // fair
    Color::rgb(0.85, 0.70, 0.55), // light
    Color::rgb(0.72, 0.55, 0.40), // medium
    Color::rgb(0.55, 0.38, 0.25), // dark
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HairStyle {
    StraightBangs,
    SideSwept,
    Wild,
}

const HAIR_STYLES: [HairStyle; 3] = [
    HairStyle::StraightBangs,
    HairStyle::SideSwept,
    HairStyle::Wild,
];

#[derive(Clone, Debug, PartialEq)]
pub struct ElfParams {
    pub hair_color: Color,
    pub eye_color: Color,
    pub skin_tone: Color,
    pub hair_style: HairStyle,
}

/// Per-slot equipment drawing info: what kind of item, what color, and wear state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EquipSlotDrawInfo {
    pub kind: ItemKind,
    pub color: Color,
    pub wear: WearCategory,
}

/// Build `ElfParams` from trait indices (as stored in the `creature_traits` table).
/// Out-of-range indices wrap via modulo to guarantee valid palette access.
/// VSH pigmentation axes modify the base palette color when present.
/// Adjacent hue blending is applied when blend target/weight traits are set.
pub fn params_from_traits(traits: &super::TraitMap) -> ElfParams {
    let hair_idx = super::trait_idx(traits, TraitKind::HairColor, 0) % HAIR_COLORS.len();
    let eye_idx = super::trait_idx(traits, TraitKind::EyeColor, 0) % EYE_COLORS.len();
    let skin_idx = super::trait_idx(traits, TraitKind::SkinTone, 0) % SKIN_TONES.len();
    let style_idx = super::trait_idx(traits, TraitKind::HairStyle, 0) % HAIR_STYLES.len();

    let hair_value = super::trait_i64(traits, TraitKind::HairValue, 0);
    let hair_sat = super::trait_i64(traits, TraitKind::HairSaturation, 0);
    let hair_blend_target = super::trait_i64(traits, TraitKind::HairBlendTarget, -1);
    let hair_blend_weight = super::trait_i64(traits, TraitKind::HairBlendWeight, 0);
    let eye_value = super::trait_i64(traits, TraitKind::EyeValue, 0);
    let eye_sat = super::trait_i64(traits, TraitKind::EyeSaturation, 0);
    let eye_blend_target = super::trait_i64(traits, TraitKind::EyeBlendTarget, -1);
    let eye_blend_weight = super::trait_i64(traits, TraitKind::EyeBlendWeight, 0);
    let melanin = super::trait_i64(traits, TraitKind::SkinMelanin, 0);
    let ruddiness = super::trait_i64(traits, TraitKind::SkinRuddiness, 0);

    ElfParams {
        hair_color: super::resolve_hue(
            &HAIR_COLORS,
            hair_idx,
            hair_blend_target,
            hair_blend_weight,
        )
        .apply_value(hair_value)
        .apply_saturation(hair_sat),
        eye_color: super::resolve_hue(&EYE_COLORS, eye_idx, eye_blend_target, eye_blend_weight)
            .apply_value(eye_value)
            .apply_saturation(eye_sat),
        skin_tone: apply_skin_vsh(SKIN_TONES[skin_idx], melanin, ruddiness),
        hair_style: HAIR_STYLES[style_idx],
    }
}

/// Apply melanin (darker/lighter) and ruddiness (more/less rosy) to a base
/// skin tone. Melanin maps to value (negative=lighter, positive=darker with
/// inverted sign since higher melanin = darker skin). Ruddiness adds a warm
/// reddish tint by boosting the red channel.
fn apply_skin_vsh(base: Color, melanin: i64, ruddiness: i64) -> Color {
    // Melanin: positive trait value = darker skin.
    let with_melanin = base.apply_value(-melanin);
    // Ruddiness: boost red channel proportionally.
    let ruddiness_shift = (ruddiness as f32 * 0.0008).clamp(-0.15, 0.15);
    Color {
        r: {
            let v = with_melanin.r as f32 / 255.0 + ruddiness_shift;
            (v.clamp(0.0, 1.0) * 255.0) as u8
        },
        ..with_melanin
    }
}

/// Create a bare-skin elf sprite (no clothing or equipment).
/// Hair, eyes, skin tone, and ears provide visual variety.
pub fn create_base_sprite(p: &ElfParams) -> PixelBuffer {
    let (w, h) = (48i32, 48i32);
    let mut img = PixelBuffer::new(w as u32, h as u32);

    let skin = p.skin_tone;
    let hair = p.hair_color;
    let eyes = p.eye_color;
    let style = p.hair_style;

    let skin_dark = skin.darken(0.12);
    let hair_dark = hair.darken(0.15);
    let outline = Color::rgb(0.15, 0.12, 0.10);
    let white = Color::rgb(1.0, 1.0, 1.0);
    let black = Color::rgb(0.08, 0.06, 0.06);
    let mouth = Color::rgb(0.75, 0.40, 0.40);

    let cx = w / 2; // 24

    // 1. Hair back layer
    match style {
        HairStyle::StraightBangs => {
            img.draw_ellipse(cx, 16, 13, 14, hair_dark);
        }
        HairStyle::SideSwept => {
            img.draw_ellipse(cx + 2, 16, 14, 13, hair_dark);
        }
        HairStyle::Wild => {
            img.draw_ellipse(cx, 15, 14, 15, hair_dark);
            img.draw_circle(cx - 10, 8, 4, hair_dark);
            img.draw_circle(cx + 10, 8, 4, hair_dark);
            img.draw_circle(cx - 7, 3, 3, hair_dark);
            img.draw_circle(cx + 7, 3, 3, hair_dark);
        }
    }

    // 2. Head
    let head_cy = 14;
    let head_r = 11;
    img.draw_circle(cx, head_cy, head_r + 1, outline);
    img.draw_circle(cx, head_cy, head_r, skin);
    // Cheek blush
    let blush = Color::from_f32(0.90, 0.60, 0.55, 0.45);
    img.draw_ellipse(cx - 7, 18, 3, 2, blush);
    img.draw_ellipse(cx + 7, 18, 3, 2, blush);

    // 3. Pointed elf ears
    for i in 0..5 {
        img.set_px(cx - head_r - 1 - i, head_cy - 2 - i, outline);
        img.set_px(cx - head_r - i, head_cy - 1 - i, skin);
        img.set_px(cx - head_r - i, head_cy - i, skin);
        img.set_px(cx + head_r + 1 + i, head_cy - 2 - i, outline);
        img.set_px(cx + head_r + i, head_cy - 1 - i, skin);
        img.set_px(cx + head_r + i, head_cy - i, skin);
    }

    // 4. Big anime eyes
    let eye_y = head_cy - 1;
    let left_eye_x = cx - 6;
    let right_eye_x = cx + 2;

    for ex in 0..5 {
        for ey in 0..5 {
            if ey == 0 || ey == 4 || ex == 0 || ex == 4 {
                img.set_px(left_eye_x + ex, eye_y + ey, outline);
                img.set_px(right_eye_x + ex, eye_y + ey, outline);
            } else {
                img.set_px(left_eye_x + ex, eye_y + ey, eyes);
                img.set_px(right_eye_x + ex, eye_y + ey, eyes);
            }
        }
    }

    // Pupils
    for px in 0..2 {
        for py in 0..2 {
            img.set_px(left_eye_x + 2 + px, eye_y + 2 + py, black);
            img.set_px(right_eye_x + 2 + px, eye_y + 2 + py, black);
        }
    }

    // White highlights
    img.set_px(left_eye_x + 1, eye_y + 1, white);
    img.set_px(right_eye_x + 1, eye_y + 1, white);

    // 5. Tiny mouth
    img.draw_hline(cx - 1, cx + 1, head_cy + 6, mouth);

    // 6. Hair front layer (bangs)
    match style {
        HairStyle::StraightBangs => {
            img.draw_rect(cx - 10, 3, 20, 7, hair);
            img.draw_ellipse(cx, 5, 11, 5, hair);
            let mut i = -9;
            while i < 10 {
                img.set_px(cx + i, 10, hair);
                img.set_px(cx + i + 1, 11, hair);
                i += 3;
            }
        }
        HairStyle::SideSwept => {
            img.draw_ellipse(cx + 1, 5, 11, 5, hair);
            for i in 0..10 {
                img.draw_hline(cx - 10 + i, cx + 10, 4 + i / 3, hair);
                if 4 + i / 3 > 8 {
                    break;
                }
            }
            img.draw_rect(cx - 10, 3, 22, 6, hair);
            img.draw_circle(cx + 11, 8, 3, hair);
        }
        HairStyle::Wild => {
            img.draw_ellipse(cx, 5, 12, 5, hair);
            let mut spike = -8;
            while spike <= 8 {
                img.draw_vline(cx + spike, 0, 6, hair);
                img.set_px(cx + spike - 1, 1, hair);
                img.set_px(cx + spike + 1, 1, hair);
                spike += 4;
            }
        }
    }

    // 7. Bare-skin body
    let body_top = 25;
    let body_bot = 36;
    for y in body_top..=body_bot {
        let hw = if y < body_top + 3 { 8 } else { 7 };
        img.draw_hline(cx - hw, cx + hw, y, skin);
    }

    // 8. Stubby arms + hands
    img.draw_rect(cx - 10, body_top + 2, 3, 7, skin);
    img.draw_rect(cx - 11, body_top + 2, 1, 7, outline);
    img.draw_rect(cx - 10, body_top + 9, 3, 2, skin);
    img.draw_rect(cx + 8, body_top + 2, 3, 7, skin);
    img.draw_rect(cx + 11, body_top + 2, 1, 7, outline);
    img.draw_rect(cx + 8, body_top + 9, 3, 2, skin);

    // 9. Short legs + bare feet
    let leg_top = body_bot + 1;
    let leg_bot = 42;
    let foot_top = 43;

    img.draw_rect(cx - 5, leg_top, 4, leg_bot - leg_top, skin_dark);
    img.draw_rect(cx + 2, leg_top, 4, leg_bot - leg_top, skin_dark);
    img.draw_rect(cx - 6, foot_top, 6, 5, skin_dark);
    img.draw_rect(cx + 1, foot_top, 6, 5, skin_dark);

    img
}

/// Create a bare-skin elf sprite (backward-compatible entry point).
pub fn create_sprite(p: &ElfParams) -> PixelBuffer {
    create_base_sprite(p)
}

/// Draw order for equipment slots — determines z-layering index.
/// Lower values are drawn first (behind higher values).
fn slot_draw_order(slot: EquipSlot) -> u8 {
    match slot {
        EquipSlot::Legs => 0,
        EquipSlot::Feet => 1,
        EquipSlot::Torso => 2,
        EquipSlot::Hands => 3,
        EquipSlot::Head => 4,
    }
}

/// Apply equipment overlays onto an existing sprite buffer. Equipment is
/// drawn in a fixed z-order (legs → feet → torso → hands → head).
/// Species-agnostic entry point — called from `create_sprite_with_equipment`
/// for any species that has overlay art (currently only elves).
pub fn apply_equipment_overlays(
    buf: &mut PixelBuffer,
    equipment: &[Option<EquipSlotDrawInfo>; EquipSlot::COUNT],
) {
    let mut slots: Vec<(EquipSlot, &EquipSlotDrawInfo)> = ALL_SLOTS
        .iter()
        .copied()
        .filter_map(|slot| equipment[slot as usize].as_ref().map(|draw| (slot, draw)))
        .collect();
    slots.sort_by_key(|(slot, _)| slot_draw_order(*slot));

    for (_slot, draw) in slots {
        elf_equipment::draw_equipment(buf, draw.kind, draw.color);
    }
}

/// All equip slots in declaration order, for iteration.
const ALL_SLOTS: [EquipSlot; EquipSlot::COUNT] = [
    EquipSlot::Head,
    EquipSlot::Torso,
    EquipSlot::Legs,
    EquipSlot::Feet,
    EquipSlot::Hands,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::species::SpriteParams;

    fn with_slot(
        equipment: &[Option<EquipSlotDrawInfo>; EquipSlot::COUNT],
        slot: EquipSlot,
        kind: ItemKind,
        color: Color,
    ) -> [Option<EquipSlotDrawInfo>; EquipSlot::COUNT] {
        let mut new = *equipment;
        new[slot as usize] = Some(EquipSlotDrawInfo {
            kind,
            color,
            wear: WearCategory::Good,
        });
        new
    }

    fn default_elf_params() -> ElfParams {
        let traits = std::collections::BTreeMap::new();
        params_from_traits(&traits)
    }

    fn make_sprite(
        params: &ElfParams,
        equipment: &[Option<EquipSlotDrawInfo>; EquipSlot::COUNT],
    ) -> PixelBuffer {
        super::super::create_sprite_with_equipment(&SpriteParams::Elf(params.clone()), equipment)
    }

    #[test]
    fn base_sprite_dimensions() {
        let buf = create_base_sprite(&default_elf_params());
        assert_eq!(buf.width(), 48);
        assert_eq!(buf.height(), 48);
    }

    #[test]
    fn base_sprite_has_opaque_pixels() {
        let buf = create_base_sprite(&default_elf_params());
        let has_opaque = buf.data().chunks(4).any(|px| px[3] > 0);
        assert!(has_opaque);
    }

    #[test]
    fn create_sprite_backward_compat() {
        let p = default_elf_params();
        let buf = create_sprite(&p);
        assert_eq!(buf.width(), 48);
        assert_eq!(buf.height(), 48);
        assert_eq!(buf.data(), create_base_sprite(&p).data());
    }

    #[test]
    fn creature_sprite_deterministic() {
        let params = default_elf_params();
        let no_equip = [None; EquipSlot::COUNT];
        let equipment = with_slot(
            &with_slot(
                &no_equip,
                EquipSlot::Head,
                ItemKind::Helmet,
                Color::rgb(0.5, 0.5, 0.5),
            ),
            EquipSlot::Torso,
            ItemKind::Breastplate,
            Color::rgb(0.6, 0.3, 0.2),
        );
        let b1 = make_sprite(&params, &equipment);
        let b2 = make_sprite(&params, &equipment);
        assert_eq!(b1.data(), b2.data());
    }

    #[test]
    fn creature_sprite_differs_from_bare() {
        let params = default_elf_params();
        let no_equip = [None; EquipSlot::COUNT];
        let bare = make_sprite(&params, &no_equip);
        let equipped = with_slot(
            &no_equip,
            EquipSlot::Torso,
            ItemKind::Tunic,
            Color::rgb(0.8, 0.2, 0.2),
        );
        let equipped_buf = make_sprite(&params, &equipped);
        assert_ne!(bare.data(), equipped_buf.data());
    }

    #[test]
    fn different_equipment_colors_produce_different_sprites() {
        let params = default_elf_params();
        let no_equip = [None; EquipSlot::COUNT];
        let red = with_slot(
            &no_equip,
            EquipSlot::Torso,
            ItemKind::Tunic,
            Color::rgb(0.9, 0.1, 0.1),
        );
        let blue = with_slot(
            &no_equip,
            EquipSlot::Torso,
            ItemKind::Tunic,
            Color::rgb(0.1, 0.1, 0.9),
        );
        assert_ne!(
            make_sprite(&params, &red).data(),
            make_sprite(&params, &blue).data()
        );
    }

    #[test]
    fn fully_equipped_creature_sprite() {
        let params = default_elf_params();
        let equipment: [Option<EquipSlotDrawInfo>; EquipSlot::COUNT] = [
            Some(EquipSlotDrawInfo {
                kind: ItemKind::Helmet,
                color: Color::rgb(0.5, 0.5, 0.5),
                wear: WearCategory::Good,
            }),
            Some(EquipSlotDrawInfo {
                kind: ItemKind::Breastplate,
                color: Color::rgb(0.6, 0.3, 0.2),
                wear: WearCategory::Worn,
            }),
            Some(EquipSlotDrawInfo {
                kind: ItemKind::Greaves,
                color: Color::rgb(0.4, 0.4, 0.4),
                wear: WearCategory::Good,
            }),
            Some(EquipSlotDrawInfo {
                kind: ItemKind::Boots,
                color: Color::rgb(0.3, 0.2, 0.1),
                wear: WearCategory::Good,
            }),
            Some(EquipSlotDrawInfo {
                kind: ItemKind::Gauntlets,
                color: Color::rgb(0.5, 0.5, 0.5),
                wear: WearCategory::Damaged,
            }),
        ];
        let buf = make_sprite(&params, &equipment);
        assert_eq!(buf.width(), 48);
        assert_eq!(buf.height(), 48);
        let no_equip = [None; EquipSlot::COUNT];
        let bare = make_sprite(&params, &no_equip);
        assert_ne!(buf.data(), bare.data());
    }

    #[test]
    fn wear_category_in_equip_slot_draw_info() {
        let no_equip = [None; EquipSlot::COUNT];
        let good = with_slot(
            &no_equip,
            EquipSlot::Torso,
            ItemKind::Tunic,
            Color::rgb(0.5, 0.5, 0.5),
        );
        let mut damaged = good;
        damaged[EquipSlot::Torso as usize].as_mut().unwrap().wear = WearCategory::Damaged;
        assert_ne!(good, damaged);
    }
}
