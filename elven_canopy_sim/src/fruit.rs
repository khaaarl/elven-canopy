// Procedural fruit species: types, generation, and naming.
//
// Each game world generates 20-40+ unique fruit species during worldgen from
// composable parts (flesh, rind, seed, fiber, sap, resin) and properties
// (starchy, sweet, fibrous, luminescent, etc.). Processing paths emerge from
// part properties — recipes match on properties, not fruit IDs.
//
// The generation algorithm works in two phases:
//   1. **Coverage phase** — generate fruits biased toward filling gameplay-
//      critical coverage gaps (starchy for bread, fibrous for cord, pigments
//      for dye, etc.).
//   2. **Bonus phase** — once all coverage minimums are met, generate
//      additional random fruits for variety.
//
// Names are generated via the lang crate's `NameTag::Botanical` morphemes,
// selecting 1-2 roots based on the fruit's most notable properties. Each
// fruit gets a Vaelith name (primary) and an English gloss (secondary).
//
// See also: `worldgen.rs` for the generator entry point, `config.rs` for
// `FruitConfig`, `db.rs` for the `FruitSpecies` tabulosity table,
// `docs/drafts/fruit_variety.md` for the full design document.
//
// **Critical constraint: determinism.** All generation uses the worldgen PRNG.
// No HashMap, no float (all integer math), no system entropy.

use std::collections::BTreeSet;

use elven_canopy_prng::GameRng;
use serde::{Deserialize, Serialize};

use crate::config::FruitConfig;

// ---------------------------------------------------------------------------
// Part types and properties
// ---------------------------------------------------------------------------

/// A physically separable component type of a fruit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PartType {
    Flesh,
    Rind,
    Seed,
    Fiber,
    Sap,
    Resin,
}

impl PartType {
    /// All part types in definition order.
    pub const ALL: [PartType; 6] = [
        PartType::Flesh,
        PartType::Rind,
        PartType::Seed,
        PartType::Fiber,
        PartType::Sap,
        PartType::Resin,
    ];
}

/// A property flag on a fruit part. Determines processing paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PartProperty {
    // Food properties
    Starchy,
    Sweet,
    Oily,
    Bland,
    Bitter,
    // Material properties
    FibrousCoarse,
    FibrousFine,
    Tough,
    // Chemical properties
    Fermentable,
    Aromatic,
    Luminescent,
    Psychoactive,
    Medicinal,
    ManaResonant,
    Stimulant,
    Adhesive,
}

impl PartProperty {
    /// All property variants in definition order.
    pub const ALL: [PartProperty; 16] = [
        PartProperty::Starchy,
        PartProperty::Sweet,
        PartProperty::Oily,
        PartProperty::Bland,
        PartProperty::Bitter,
        PartProperty::FibrousCoarse,
        PartProperty::FibrousFine,
        PartProperty::Tough,
        PartProperty::Fermentable,
        PartProperty::Aromatic,
        PartProperty::Luminescent,
        PartProperty::Psychoactive,
        PartProperty::Medicinal,
        PartProperty::ManaResonant,
        PartProperty::Stimulant,
        PartProperty::Adhesive,
    ];

    /// The coverage category this property satisfies, if any.
    /// A property may satisfy zero or one category for coverage tracking.
    pub fn coverage_category(self) -> Option<CoverageCategory> {
        match self {
            PartProperty::Starchy => Some(CoverageCategory::Starchy),
            PartProperty::Sweet => Some(CoverageCategory::Sweet),
            PartProperty::FibrousCoarse => Some(CoverageCategory::FibrousCoarse),
            PartProperty::FibrousFine => Some(CoverageCategory::FibrousFine),
            PartProperty::Fermentable => Some(CoverageCategory::Fermentable),
            PartProperty::Medicinal => Some(CoverageCategory::Medicinal),
            PartProperty::Aromatic => Some(CoverageCategory::Aromatic),
            PartProperty::Luminescent => Some(CoverageCategory::Luminescent),
            PartProperty::Psychoactive => Some(CoverageCategory::Psychoactive),
            PartProperty::Stimulant => Some(CoverageCategory::Stimulant),
            PartProperty::ManaResonant => Some(CoverageCategory::ManaResonant),
            _ => None,
        }
    }
}

/// Dye colors available from pigmented fruit parts. Primary colors appear
/// directly on fruit; secondary colors are only produced by mixing at a dye
/// workshop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DyeColor {
    // Primaries (appear on fruit parts)
    Red,
    Yellow,
    Blue,
    // Shade/tint modifiers (appear on fruit parts)
    Black,
    White,
    // Secondaries (mixing only, never on fruit parts)
    Orange,
    Green,
    Violet,
}

impl DyeColor {
    /// Colors that can appear directly on fruit parts during worldgen.
    pub const FRUIT_COLORS: [DyeColor; 5] = [
        DyeColor::Red,
        DyeColor::Yellow,
        DyeColor::Blue,
        DyeColor::Black,
        DyeColor::White,
    ];

    /// The coverage category this pigment satisfies.
    pub fn coverage_category(self) -> CoverageCategory {
        match self {
            DyeColor::Red => CoverageCategory::PigmentRed,
            DyeColor::Yellow => CoverageCategory::PigmentYellow,
            DyeColor::Blue => CoverageCategory::PigmentBlue,
            DyeColor::Black => CoverageCategory::PigmentBlack,
            DyeColor::White => CoverageCategory::PigmentWhite,
            // Secondaries don't appear on fruit, but map for completeness.
            DyeColor::Orange | DyeColor::Green | DyeColor::Violet => CoverageCategory::Sweet,
        }
    }
}

// ---------------------------------------------------------------------------
// Fruit part
// ---------------------------------------------------------------------------

/// A physically separable component of a fruit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FruitPart {
    /// What kind of part this is.
    pub part_type: PartType,
    /// Properties that determine processing paths.
    pub properties: BTreeSet<PartProperty>,
    /// If pigmented, what dye color this part yields.
    pub pigment: Option<DyeColor>,
    /// Percentage of the fruit's mass this part represents (1-100).
    /// All parts of a fruit must sum to exactly 100.
    pub yield_percent: u8,
}

// ---------------------------------------------------------------------------
// Appearance and shape
// ---------------------------------------------------------------------------

/// An RGB color stored as integers (no floats).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FruitColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Visual shape hint for sprite generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FruitShape {
    Round,
    Oblong,
    Clustered,
    Pod,
    Nut,
    Gourd,
}

impl FruitShape {
    /// Noun used when displaying this fruit shape in item names.
    /// Round and Oblong use "Fruit"; others use their shape name.
    pub fn item_noun(self) -> &'static str {
        match self {
            FruitShape::Round | FruitShape::Oblong => "Fruit",
            FruitShape::Clustered => "Cluster",
            FruitShape::Pod => "Pod",
            FruitShape::Nut => "Nut",
            FruitShape::Gourd => "Gourd",
        }
    }

    pub const ALL: [FruitShape; 6] = [
        FruitShape::Round,
        FruitShape::Oblong,
        FruitShape::Clustered,
        FruitShape::Pod,
        FruitShape::Nut,
        FruitShape::Gourd,
    ];
}

/// Visual appearance parameters for rendering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FruitAppearance {
    /// Base exterior color.
    pub exterior_color: FruitColor,
    /// Shape hint for sprite generation.
    pub shape: FruitShape,
    /// Size relative to standard (100 = normal, 50 = half, 200 = double).
    pub size_percent: u16,
    /// Whether the fruit visibly glows.
    pub glows: bool,
}

/// Where a fruit species grows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GrowthHabitat {
    /// Grows on tree branches (Leaf voxels). Most common.
    Branch,
    /// Grows on trunk surface.
    Trunk,
    /// Grows on forest floor bushes (requires foraging).
    GroundBush,
}

impl GrowthHabitat {
    pub const ALL: [GrowthHabitat; 3] = [
        GrowthHabitat::Branch,
        GrowthHabitat::Trunk,
        GrowthHabitat::GroundBush,
    ];
}

/// How common a fruit species is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
}

// ---------------------------------------------------------------------------
// Fruit species (the main type)
// ---------------------------------------------------------------------------

/// A procedurally generated fruit species. Immutable worldgen data.
///
/// Stored as a tabulosity table in `SimDb` — one row per species per world.
#[derive(tabulosity::Table, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FruitSpecies {
    /// Unique ID for this fruit species in this world.
    #[primary_key]
    pub id: FruitSpeciesId,
    /// Generated Vaelith name.
    pub vaelith_name: String,
    /// English gloss (e.g., "glow-berry").
    pub english_gloss: String,
    /// The separable parts of this fruit.
    pub parts: Vec<FruitPart>,
    /// Where this fruit grows.
    pub habitat: GrowthHabitat,
    /// How common this fruit is.
    pub rarity: Rarity,
    /// Whether this fruit can be grown in a greenhouse.
    pub greenhouse_cultivable: bool,
    /// Visual appearance hints for rendering.
    pub appearance: FruitAppearance,
}

impl FruitSpecies {
    /// Collect all properties across all parts of this fruit.
    pub fn all_properties(&self) -> BTreeSet<PartProperty> {
        let mut props = BTreeSet::new();
        for part in &self.parts {
            props.extend(&part.properties);
        }
        props
    }

    /// Collect all pigments across all parts.
    pub fn all_pigments(&self) -> Vec<DyeColor> {
        self.parts.iter().filter_map(|p| p.pigment).collect()
    }

    /// Check if any part has the given property.
    pub fn has_property(&self, prop: PartProperty) -> bool {
        self.parts.iter().any(|p| p.properties.contains(&prop))
    }

    /// Check if any part is pigmented.
    pub fn has_pigment(&self) -> bool {
        self.parts.iter().any(|p| p.pigment.is_some())
    }
}

// FruitSpeciesId is defined in `types.rs` (with `Bounded` derive for tabulosity).
pub use crate::types::FruitSpeciesId;

// ---------------------------------------------------------------------------
// Coverage tracking
// ---------------------------------------------------------------------------

/// Categories tracked during worldgen to ensure the world has enough fruit
/// diversity for all gameplay-critical chains.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CoverageCategory {
    Starchy,
    Sweet,
    FibrousCoarse,
    FibrousFine,
    PigmentRed,
    PigmentYellow,
    PigmentBlue,
    PigmentBlack,
    PigmentWhite,
    Fermentable,
    Medicinal,
    Aromatic,
    Luminescent,
    Psychoactive,
    Stimulant,
    ManaResonant,
}

impl CoverageCategory {
    pub const ALL: [CoverageCategory; 16] = [
        CoverageCategory::Starchy,
        CoverageCategory::Sweet,
        CoverageCategory::FibrousCoarse,
        CoverageCategory::FibrousFine,
        CoverageCategory::PigmentRed,
        CoverageCategory::PigmentYellow,
        CoverageCategory::PigmentBlue,
        CoverageCategory::PigmentBlack,
        CoverageCategory::PigmentWhite,
        CoverageCategory::Fermentable,
        CoverageCategory::Medicinal,
        CoverageCategory::Aromatic,
        CoverageCategory::Luminescent,
        CoverageCategory::Psychoactive,
        CoverageCategory::Stimulant,
        CoverageCategory::ManaResonant,
    ];
}

// ---------------------------------------------------------------------------
// Exclusion rules
// ---------------------------------------------------------------------------

/// Within-part property exclusion groups. Properties in the same group cannot
/// coexist on a single part. Across different parts of the same fruit, any
/// combination is valid.
pub fn exclusion_groups() -> Vec<Vec<PartProperty>> {
    vec![
        // Structural tissue types are mutually exclusive.
        vec![
            PartProperty::Starchy,
            PartProperty::FibrousCoarse,
            PartProperty::FibrousFine,
        ],
        // Contradictory flavors.
        vec![PartProperty::Sweet, PartProperty::Bitter],
        // Opposing neurological effects.
        vec![PartProperty::Psychoactive, PartProperty::Stimulant],
        // Flavor/texture categories — at most one per part.
        vec![
            PartProperty::Starchy,
            PartProperty::Sweet,
            PartProperty::Oily,
            PartProperty::Bland,
            PartProperty::Bitter,
        ],
    ]
}

/// Check if adding `candidate` to a part that already has `existing` properties
/// would violate any exclusion rule. Also checks the luminescent/pigment
/// exclusion (a part cannot be both luminescent and pigmented).
pub fn violates_exclusion(
    existing: &BTreeSet<PartProperty>,
    candidate: PartProperty,
    has_pigment: bool,
) -> bool {
    // Luminescent + pigment exclusion.
    if candidate == PartProperty::Luminescent && has_pigment {
        return true;
    }

    for group in exclusion_groups() {
        if group.contains(&candidate) {
            for prop in existing {
                if group.contains(prop) && *prop != candidate {
                    return true;
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

/// Coverage state tracker used during fruit generation.
struct CoverageTracker {
    counts: std::collections::BTreeMap<CoverageCategory, u16>,
    minimums: std::collections::BTreeMap<CoverageCategory, u16>,
}

impl CoverageTracker {
    fn new(config: &FruitConfig) -> Self {
        let mut minimums = std::collections::BTreeMap::new();
        for &cat in CoverageCategory::ALL.iter() {
            let min = config.coverage_minimum(cat);
            if min > 0 {
                minimums.insert(cat, min);
            }
        }
        CoverageTracker {
            counts: std::collections::BTreeMap::new(),
            minimums,
        }
    }

    fn record_fruit(&mut self, fruit: &FruitSpecies) {
        for part in &fruit.parts {
            for prop in &part.properties {
                if let Some(cat) = prop.coverage_category() {
                    *self.counts.entry(cat).or_insert(0) += 1;
                }
            }
            if let Some(color) = part.pigment {
                let cat = color.coverage_category();
                *self.counts.entry(cat).or_insert(0) += 1;
            }
        }
    }

    fn unfilled_categories(&self) -> Vec<CoverageCategory> {
        let mut gaps = Vec::new();
        for (&cat, &min) in &self.minimums {
            let count = self.counts.get(&cat).copied().unwrap_or(0);
            if count < min {
                gaps.push(cat);
            }
        }
        gaps
    }

    fn is_satisfied(&self) -> bool {
        self.unfilled_categories().is_empty()
    }
}

/// Generate all fruit species for a world. Called during worldgen.
///
/// Returns a vector of `FruitSpecies`, each with a unique ID (0, 1, 2, ...).
/// Guarantees coverage minimums are met before generating bonus fruits.
pub fn generate_fruit_species(
    rng: &mut GameRng,
    config: &FruitConfig,
    lexicon: &elven_canopy_lang::Lexicon,
) -> Vec<FruitSpecies> {
    let total_count = config.min_species_per_world
        + (rng.next_u64()
            % (config.max_species_per_world - config.min_species_per_world + 1) as u64)
            as u16;

    let mut tracker = CoverageTracker::new(config);
    let mut fruits = Vec::new();
    let mut used_names: BTreeSet<String> = BTreeSet::new();

    for i in 0..total_count {
        let id = FruitSpeciesId(i);
        let gaps = tracker.unfilled_categories();

        // Generate parts, biased toward filling gaps if any remain.
        let parts = generate_parts(rng, config, &gaps);

        // Assign habitat and rarity.
        let habitat = pick_habitat(rng);
        let rarity = pick_rarity(rng, config);

        // Derive appearance from parts.
        let appearance = derive_appearance(&parts, rng);

        // Generate name from properties.
        let (vaelith_name, english_gloss) =
            generate_fruit_name(rng, lexicon, &parts, &mut used_names);

        let fruit = FruitSpecies {
            id,
            vaelith_name,
            english_gloss,
            parts,
            habitat,
            rarity,
            greenhouse_cultivable: true, // All fruits cultivable initially.
            appearance,
        };

        tracker.record_fruit(&fruit);
        fruits.push(fruit);
    }

    // Safety: if coverage still not met (shouldn't happen with enough fruits),
    // log but don't panic — the game can still run with reduced variety.
    debug_assert!(
        tracker.is_satisfied(),
        "Fruit coverage not satisfied after generating {} fruits. Gaps: {:?}",
        fruits.len(),
        tracker.unfilled_categories()
    );

    fruits
}

/// Generate the parts for a single fruit species.
fn generate_parts(
    rng: &mut GameRng,
    config: &FruitConfig,
    coverage_gaps: &[CoverageCategory],
) -> Vec<FruitPart> {
    // 1-4 parts, biased toward 2-3.
    let max_parts = config.max_parts_per_fruit.min(4) as usize;
    let part_count = match rng.next_u64() % 100 {
        0..15 => 1,
        15..55 => 2,
        55..85 => 3,
        _ => max_parts,
    };

    // Pick part types (no duplicates).
    let mut available_types: Vec<PartType> = PartType::ALL.to_vec();
    let mut chosen_types = Vec::new();
    for _ in 0..part_count {
        if available_types.is_empty() {
            break;
        }
        let idx = rng.next_u64() as usize % available_types.len();
        chosen_types.push(available_types.remove(idx));
    }

    // Allocate yield percentages (must sum to 100).
    let yields = allocate_yields(rng, chosen_types.len());

    // Generate properties for each part. The first part gets biased toward
    // filling coverage gaps; remaining parts get random properties.
    let mut parts = Vec::new();
    for (i, (&pt, &yld)) in chosen_types.iter().zip(yields.iter()).enumerate() {
        let (properties, pigment) =
            generate_part_properties(rng, pt, if i == 0 { coverage_gaps } else { &[] });
        parts.push(FruitPart {
            part_type: pt,
            properties,
            pigment,
            yield_percent: yld,
        });
    }

    parts
}

/// Allocate yield percentages across N parts, summing to exactly 100.
fn allocate_yields(rng: &mut GameRng, count: usize) -> Vec<u8> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![100];
    }

    // Generate random breakpoints in [1, 99], sort, then take differences.
    let mut breakpoints: Vec<u8> = Vec::new();
    for _ in 0..count - 1 {
        // Range 5..95 to avoid tiny yields.
        let bp = 5 + (rng.next_u64() % 91) as u8;
        breakpoints.push(bp);
    }
    breakpoints.sort();

    // Ensure no two breakpoints are identical (shift duplicates).
    for i in 1..breakpoints.len() {
        if breakpoints[i] <= breakpoints[i - 1] {
            breakpoints[i] = breakpoints[i - 1] + 1;
        }
    }

    // Compute yields from breakpoints.
    let mut yields = Vec::with_capacity(count);
    yields.push(breakpoints[0]);
    for i in 1..breakpoints.len() {
        yields.push(breakpoints[i] - breakpoints[i - 1]);
    }
    yields.push(100 - breakpoints[breakpoints.len() - 1]);

    // Clamp any zero or negative yields to at least 1, redistributing.
    for y in yields.iter_mut() {
        if *y == 0 {
            *y = 1;
        }
    }

    // Adjust to sum to exactly 100.
    let sum: u16 = yields.iter().map(|&y| y as u16).sum();
    if sum != 100 {
        let diff = sum as i16 - 100;
        // Find the largest yield and adjust it.
        let max_idx = yields
            .iter()
            .enumerate()
            .max_by_key(|&(_, &y)| y)
            .unwrap()
            .0;
        yields[max_idx] = (yields[max_idx] as i16 - diff).max(1) as u8;
    }

    yields
}

/// Generate properties and optional pigment for a single fruit part.
/// If `bias_categories` is non-empty, tries to include a property that
/// fills one of those coverage gaps.
fn generate_part_properties(
    rng: &mut GameRng,
    part_type: PartType,
    bias_categories: &[CoverageCategory],
) -> (BTreeSet<PartProperty>, Option<DyeColor>) {
    let mut properties = BTreeSet::new();
    let mut pigment: Option<DyeColor> = None;

    // Typical property affinities per part type.
    let type_affinities: &[PartProperty] = match part_type {
        PartType::Flesh => &[
            PartProperty::Starchy,
            PartProperty::Sweet,
            PartProperty::Oily,
            PartProperty::Bland,
            PartProperty::Bitter,
        ],
        PartType::Rind => &[
            PartProperty::Aromatic,
            PartProperty::Tough,
            PartProperty::Bitter,
        ],
        PartType::Seed => &[
            PartProperty::Oily,
            PartProperty::Bitter,
            PartProperty::ManaResonant,
        ],
        PartType::Fiber => &[PartProperty::FibrousCoarse, PartProperty::FibrousFine],
        PartType::Sap => &[
            PartProperty::Sweet,
            PartProperty::Fermentable,
            PartProperty::Luminescent,
            PartProperty::Psychoactive,
            PartProperty::Medicinal,
        ],
        PartType::Resin => &[
            PartProperty::Aromatic,
            PartProperty::Adhesive,
            PartProperty::ManaResonant,
        ],
    };

    // Try to fill a coverage gap first.
    if !bias_categories.is_empty() {
        let gap_idx = rng.next_u64() as usize % bias_categories.len();
        let target = bias_categories[gap_idx];

        match target {
            // Pigment coverage — add pigment to this part.
            CoverageCategory::PigmentRed => {
                pigment = Some(DyeColor::Red);
            }
            CoverageCategory::PigmentYellow => {
                pigment = Some(DyeColor::Yellow);
            }
            CoverageCategory::PigmentBlue => {
                pigment = Some(DyeColor::Blue);
            }
            CoverageCategory::PigmentBlack => {
                pigment = Some(DyeColor::Black);
            }
            CoverageCategory::PigmentWhite => {
                pigment = Some(DyeColor::White);
            }
            // Property coverage — try to add the corresponding property.
            _ => {
                if let Some(prop) = category_to_property(target)
                    && !violates_exclusion(&properties, prop, pigment.is_some())
                {
                    properties.insert(prop);
                }
            }
        }
    }

    // Add 1-2 properties from type affinities.
    let prop_count = 1 + (rng.next_u64() % 2) as usize;
    for _ in 0..prop_count {
        if type_affinities.is_empty() {
            break;
        }
        let idx = rng.next_u64() as usize % type_affinities.len();
        let candidate = type_affinities[idx];
        if !violates_exclusion(&properties, candidate, pigment.is_some()) {
            properties.insert(candidate);
        }
    }

    // If no properties were added (unlikely but possible), add Bland as fallback.
    if properties.is_empty() {
        properties.insert(PartProperty::Bland);
    }

    // Small chance of pigment on Rind or Flesh if not already pigmented.
    if pigment.is_none()
        && matches!(part_type, PartType::Rind | PartType::Flesh | PartType::Sap)
        && rng.next_u64() % 100 < 20
        && !properties.contains(&PartProperty::Luminescent)
    {
        let color_idx = rng.next_u64() as usize % DyeColor::FRUIT_COLORS.len();
        pigment = Some(DyeColor::FRUIT_COLORS[color_idx]);
    }

    (properties, pigment)
}

/// Map a coverage category back to a property for biasing.
fn category_to_property(cat: CoverageCategory) -> Option<PartProperty> {
    match cat {
        CoverageCategory::Starchy => Some(PartProperty::Starchy),
        CoverageCategory::Sweet => Some(PartProperty::Sweet),
        CoverageCategory::FibrousCoarse => Some(PartProperty::FibrousCoarse),
        CoverageCategory::FibrousFine => Some(PartProperty::FibrousFine),
        CoverageCategory::Fermentable => Some(PartProperty::Fermentable),
        CoverageCategory::Medicinal => Some(PartProperty::Medicinal),
        CoverageCategory::Aromatic => Some(PartProperty::Aromatic),
        CoverageCategory::Luminescent => Some(PartProperty::Luminescent),
        CoverageCategory::Psychoactive => Some(PartProperty::Psychoactive),
        CoverageCategory::Stimulant => Some(PartProperty::Stimulant),
        CoverageCategory::ManaResonant => Some(PartProperty::ManaResonant),
        // Pigment categories are handled separately.
        CoverageCategory::PigmentRed
        | CoverageCategory::PigmentYellow
        | CoverageCategory::PigmentBlue
        | CoverageCategory::PigmentBlack
        | CoverageCategory::PigmentWhite => None,
    }
}

fn pick_habitat(rng: &mut GameRng) -> GrowthHabitat {
    // Branch 60%, Trunk 25%, GroundBush 15%.
    match rng.next_u64() % 100 {
        0..60 => GrowthHabitat::Branch,
        60..85 => GrowthHabitat::Trunk,
        _ => GrowthHabitat::GroundBush,
    }
}

fn pick_rarity(rng: &mut GameRng, config: &FruitConfig) -> Rarity {
    let [w_common, w_uncommon, w_rare] = config.rarity_weights;
    let total = w_common as u64 + w_uncommon as u64 + w_rare as u64;
    if total == 0 {
        return Rarity::Common;
    }
    let roll = rng.next_u64() % total;
    if roll < w_common as u64 {
        Rarity::Common
    } else if roll < (w_common + w_uncommon) as u64 {
        Rarity::Uncommon
    } else {
        Rarity::Rare
    }
}

/// Derive visual appearance from the fruit's parts and properties.
fn derive_appearance(parts: &[FruitPart], rng: &mut GameRng) -> FruitAppearance {
    // Exterior color: use dominant pigment, or heuristic from properties.
    let exterior_color = if let Some(pigment) = parts.iter().find_map(|p| p.pigment) {
        dye_to_rgb(pigment)
    } else {
        // Heuristic based on dominant property.
        let all_props: BTreeSet<PartProperty> = parts
            .iter()
            .flat_map(|p| p.properties.iter().copied())
            .collect();
        if all_props.contains(&PartProperty::Luminescent) {
            FruitColor {
                r: 200,
                g: 255,
                b: 220,
            } // pale green-white
        } else if all_props.contains(&PartProperty::Starchy) {
            FruitColor {
                r: 200,
                g: 170,
                b: 120,
            } // tan
        } else if all_props.contains(&PartProperty::Sweet) {
            FruitColor {
                r: 240,
                g: 200,
                b: 80,
            } // warm yellow
        } else {
            // Neutral brownish-green.
            FruitColor {
                r: 140,
                g: 160,
                b: 100,
            }
        }
    };

    // Shape from part composition.
    let has_fiber = parts.iter().any(|p| {
        p.properties.contains(&PartProperty::FibrousCoarse)
            || p.properties.contains(&PartProperty::FibrousFine)
    });
    let has_tough_rind = parts
        .iter()
        .any(|p| p.part_type == PartType::Rind && p.properties.contains(&PartProperty::Tough));
    let has_big_flesh = parts
        .iter()
        .any(|p| p.part_type == PartType::Flesh && p.yield_percent >= 50);

    let shape = if has_fiber {
        FruitShape::Pod
    } else if has_tough_rind {
        FruitShape::Nut
    } else if parts.len() >= 3 && has_big_flesh {
        FruitShape::Gourd
    } else if parts.len() == 1 {
        // Small single-part fruits.
        match rng.next_u64() % 3 {
            0 => FruitShape::Nut,
            1 => FruitShape::Round,
            _ => FruitShape::Clustered,
        }
    } else {
        match rng.next_u64() % 3 {
            0 => FruitShape::Round,
            1 => FruitShape::Oblong,
            _ => FruitShape::Clustered,
        }
    };

    // Size from part count and distribution.
    let size_percent = match parts.len() {
        1 => 50 + (rng.next_u64() % 40) as u16,  // 50-89
        2 => 80 + (rng.next_u64() % 50) as u16,  // 80-129
        3 => 100 + (rng.next_u64() % 60) as u16, // 100-159
        _ => 120 + (rng.next_u64() % 80) as u16, // 120-199
    };

    let glows = parts
        .iter()
        .any(|p| p.properties.contains(&PartProperty::Luminescent));

    FruitAppearance {
        exterior_color,
        shape,
        size_percent,
        glows,
    }
}

fn dye_to_rgb(color: DyeColor) -> FruitColor {
    match color {
        DyeColor::Red => FruitColor {
            r: 200,
            g: 50,
            b: 50,
        },
        DyeColor::Yellow => FruitColor {
            r: 230,
            g: 210,
            b: 60,
        },
        DyeColor::Blue => FruitColor {
            r: 60,
            g: 80,
            b: 200,
        },
        DyeColor::Black => FruitColor {
            r: 40,
            g: 30,
            b: 40,
        },
        DyeColor::White => FruitColor {
            r: 240,
            g: 240,
            b: 235,
        },
        DyeColor::Orange => FruitColor {
            r: 230,
            g: 140,
            b: 40,
        },
        DyeColor::Green => FruitColor {
            r: 60,
            g: 180,
            b: 70,
        },
        DyeColor::Violet => FruitColor {
            r: 140,
            g: 50,
            b: 180,
        },
    }
}

// ---------------------------------------------------------------------------
// Naming
// ---------------------------------------------------------------------------

/// Property-to-gloss mapping for selecting botanical morphemes.
/// Each property maps to one or more English glosses that the name generator
/// searches for in the lexicon's Botanical-tagged entries.
fn property_glosses(prop: PartProperty) -> &'static [&'static str] {
    match prop {
        PartProperty::Starchy => &["grain"],
        PartProperty::Sweet => &["sweet", "honey", "nectar"],
        PartProperty::Oily => &["rich"],
        PartProperty::Bland => &["bland"],
        PartProperty::Bitter => &["bitter"],
        PartProperty::FibrousCoarse => &["rough", "cord"],
        PartProperty::FibrousFine => &["fiber", "thread"],
        PartProperty::Tough => &["hard"],
        PartProperty::Fermentable => &["ripe"],
        PartProperty::Aromatic => &["fragrant", "incense"],
        PartProperty::Luminescent => &["glow", "light"],
        PartProperty::Psychoactive => &["dream"],
        PartProperty::Medicinal => &["heal"],
        PartProperty::ManaResonant => &["mana", "essence"],
        PartProperty::Stimulant => &["spice", "sharp"],
        PartProperty::Adhesive => &["sticky", "resin"],
    }
}

/// Gloss values for pigment colors.
fn pigment_glosses(color: DyeColor) -> &'static [&'static str] {
    match color {
        DyeColor::Red => &["red"],
        DyeColor::Yellow => &["yellow", "golden"],
        DyeColor::Blue => &["blue"],
        DyeColor::Black => &["black", "dark"],
        DyeColor::White => &["white", "pale"],
        DyeColor::Orange => &["orange"],
        DyeColor::Green => &["green"],
        DyeColor::Violet => &["violet"],
    }
}

/// Generate a Vaelith name and English gloss for a fruit species.
///
/// Strategy: pick 1-2 morphemes from the Botanical pool. The first morpheme
/// describes the most notable property; the second describes the shape/form.
/// Uses rejection sampling to guarantee uniqueness within the world.
fn generate_fruit_name(
    rng: &mut GameRng,
    lexicon: &elven_canopy_lang::Lexicon,
    parts: &[FruitPart],
    used_names: &mut BTreeSet<String>,
) -> (String, String) {
    let botanical_pool = lexicon.by_name_tag(elven_canopy_lang::NameTag::Botanical);
    if botanical_pool.is_empty() {
        // Fallback if no botanical entries (shouldn't happen with proper lexicon).
        let name = format!("vela-{}", used_names.len());
        used_names.insert(name.clone());
        return (name, "unknown-fruit".to_string());
    }

    // Collect the "most notable" property — prefer rare/interesting ones.
    let notable_prop = pick_notable_property(parts, rng);
    let notable_pigment = parts.iter().find_map(|p| p.pigment);

    // Build candidate gloss lists for the descriptor morpheme.
    let descriptor_glosses: Vec<&str> = if let Some(prop) = notable_prop {
        property_glosses(prop).to_vec()
    } else if let Some(color) = notable_pigment {
        pigment_glosses(color).to_vec()
    } else {
        vec!["wild", "fruit"]
    };

    // Shape/form morpheme glosses.
    let form_glosses: Vec<&str> = {
        // Determine shape from parts (same logic as appearance derivation).
        let has_fiber = parts.iter().any(|p| {
            p.properties.contains(&PartProperty::FibrousCoarse)
                || p.properties.contains(&PartProperty::FibrousFine)
        });
        let has_tough_rind = parts
            .iter()
            .any(|p| p.part_type == PartType::Rind && p.properties.contains(&PartProperty::Tough));
        if has_fiber {
            vec!["pod", "husk"]
        } else if has_tough_rind {
            vec!["nut", "seed"]
        } else if parts.len() >= 3 {
            vec!["gourd", "fruit"]
        } else {
            vec!["berry", "fruit", "cluster"]
        }
    };

    // Try to find matching lexicon entries.
    let descriptor_entry = find_entry_by_glosses(&botanical_pool, &descriptor_glosses, rng);
    let form_entry = find_entry_by_glosses(&botanical_pool, &form_glosses, rng);

    // Rejection-sample for uniqueness (max 50 attempts, then fall through).
    for _attempt in 0..50 {
        let (name, gloss) = compose_name(rng, descriptor_entry, form_entry, &botanical_pool);

        if !used_names.contains(&name) {
            used_names.insert(name.clone());
            return (name, gloss);
        }
    }

    // Exhausted retries — append a disambiguator.
    let (base_name, base_gloss) = compose_name(rng, descriptor_entry, form_entry, &botanical_pool);
    let name = format!("{}{}", base_name, used_names.len());
    used_names.insert(name.clone());
    (name, base_gloss)
}

/// Find a lexicon entry whose gloss matches one of the candidates.
fn find_entry_by_glosses<'a>(
    pool: &[&'a elven_canopy_lang::LexEntry],
    glosses: &[&str],
    rng: &mut GameRng,
) -> Option<&'a elven_canopy_lang::LexEntry> {
    let matches: Vec<&&elven_canopy_lang::LexEntry> = pool
        .iter()
        .filter(|e| glosses.contains(&e.gloss.as_str()))
        .collect();
    if matches.is_empty() {
        return None;
    }
    let idx = rng.next_u64() as usize % matches.len();
    Some(matches[idx])
}

/// Compose a Vaelith name from descriptor and form morphemes.
fn compose_name(
    rng: &mut GameRng,
    descriptor: Option<&elven_canopy_lang::LexEntry>,
    form: Option<&elven_canopy_lang::LexEntry>,
    fallback_pool: &[&elven_canopy_lang::LexEntry],
) -> (String, String) {
    match (descriptor, form) {
        (Some(d), Some(f)) if d.root != f.root => {
            let name = capitalize(&format!("{}{}", d.root, f.root));
            let gloss = format!("{}-{}", d.gloss, f.gloss);
            (name, gloss)
        }
        (Some(d), _) => {
            // Single morpheme or same entry — pick a random second.
            if fallback_pool.len() >= 2 {
                let idx = rng.next_u64() as usize % fallback_pool.len();
                let f = fallback_pool[idx];
                if f.root != d.root {
                    let name = capitalize(&format!("{}{}", d.root, f.root));
                    let gloss = format!("{}-{}", d.gloss, f.gloss);
                    return (name, gloss);
                }
            }
            (capitalize(&d.root), d.gloss.clone())
        }
        (None, Some(f)) => (capitalize(&f.root), f.gloss.clone()),
        (None, None) => {
            // Fallback: random from pool.
            if !fallback_pool.is_empty() {
                let idx = rng.next_u64() as usize % fallback_pool.len();
                let entry = fallback_pool[idx];
                (capitalize(&entry.root), entry.gloss.clone())
            } else {
                ("Vela".to_string(), "fruit".to_string())
            }
        }
    }
}

/// Pick the most "notable" property from a fruit's parts.
/// Prefers rare/interesting properties over common food ones.
fn pick_notable_property(parts: &[FruitPart], rng: &mut GameRng) -> Option<PartProperty> {
    // Priority tiers (higher = more notable for naming).
    let priority = |p: &PartProperty| -> u8 {
        match p {
            PartProperty::ManaResonant => 10,
            PartProperty::Luminescent => 9,
            PartProperty::Psychoactive => 8,
            PartProperty::Medicinal => 7,
            PartProperty::Stimulant => 6,
            PartProperty::Aromatic => 5,
            PartProperty::Fermentable => 4,
            PartProperty::FibrousFine => 3,
            PartProperty::FibrousCoarse => 3,
            PartProperty::Starchy => 2,
            PartProperty::Sweet => 2,
            PartProperty::Oily => 1,
            PartProperty::Bitter => 1,
            PartProperty::Bland => 0,
            PartProperty::Tough => 0,
            PartProperty::Adhesive => 4,
        }
    };

    let mut all_props: Vec<PartProperty> = parts
        .iter()
        .flat_map(|p| p.properties.iter().copied())
        .collect();
    all_props.sort_by_key(|p| std::cmp::Reverse(priority(p)));
    all_props.dedup();

    if all_props.is_empty() {
        return None;
    }

    // Pick from the top tier (all props with the same highest priority).
    let top_priority = priority(&all_props[0]);
    let top_tier: Vec<_> = all_props
        .iter()
        .filter(|p| priority(p) == top_priority)
        .collect();
    let idx = rng.next_u64() as usize % top_tier.len();
    Some(*top_tier[idx])
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

    fn test_config() -> FruitConfig {
        FruitConfig::default()
    }

    // --- Exclusion rules ---

    #[test]
    fn sweet_and_bitter_excluded_on_same_part() {
        let mut props = BTreeSet::new();
        props.insert(PartProperty::Sweet);
        assert!(violates_exclusion(&props, PartProperty::Bitter, false));
    }

    #[test]
    fn starchy_and_fibrous_coarse_excluded_on_same_part() {
        let mut props = BTreeSet::new();
        props.insert(PartProperty::Starchy);
        assert!(violates_exclusion(
            &props,
            PartProperty::FibrousCoarse,
            false
        ));
    }

    #[test]
    fn psychoactive_and_stimulant_excluded_on_same_part() {
        let mut props = BTreeSet::new();
        props.insert(PartProperty::Psychoactive);
        assert!(violates_exclusion(&props, PartProperty::Stimulant, false));
    }

    #[test]
    fn luminescent_excluded_with_pigment() {
        let props = BTreeSet::new();
        assert!(violates_exclusion(&props, PartProperty::Luminescent, true));
    }

    #[test]
    fn non_conflicting_properties_allowed() {
        let mut props = BTreeSet::new();
        props.insert(PartProperty::Aromatic);
        assert!(!violates_exclusion(
            &props,
            PartProperty::ManaResonant,
            false
        ));
    }

    #[test]
    fn flavor_categories_mutually_exclusive() {
        let mut props = BTreeSet::new();
        props.insert(PartProperty::Starchy);
        assert!(violates_exclusion(&props, PartProperty::Sweet, false));
        assert!(violates_exclusion(&props, PartProperty::Oily, false));
        assert!(violates_exclusion(&props, PartProperty::Bland, false));
        assert!(violates_exclusion(&props, PartProperty::Bitter, false));
    }

    // --- Yield allocation ---

    #[test]
    fn yields_sum_to_100() {
        let mut rng = GameRng::new(42);
        for _ in 0..100 {
            for count in 1..=4 {
                let yields = allocate_yields(&mut rng, count);
                let sum: u16 = yields.iter().map(|&y| y as u16).sum();
                assert_eq!(sum, 100, "Yields {:?} don't sum to 100", yields);
                assert_eq!(yields.len(), count);
                for &y in &yields {
                    assert!(y >= 1, "Yield should be at least 1, got {}", y);
                }
            }
        }
    }

    #[test]
    fn single_part_yield_is_100() {
        let mut rng = GameRng::new(0);
        let yields = allocate_yields(&mut rng, 1);
        assert_eq!(yields, vec![100]);
    }

    #[test]
    fn empty_parts_yield_empty() {
        let mut rng = GameRng::new(0);
        let yields = allocate_yields(&mut rng, 0);
        assert!(yields.is_empty());
    }

    // --- Full generation ---

    #[test]
    fn generation_is_deterministic() {
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();
        let mut rng1 = GameRng::new(42);
        let mut rng2 = GameRng::new(42);

        let fruits1 = generate_fruit_species(&mut rng1, &config, &lexicon);
        let fruits2 = generate_fruit_species(&mut rng2, &config, &lexicon);

        assert_eq!(fruits1.len(), fruits2.len());
        for (f1, f2) in fruits1.iter().zip(fruits2.iter()) {
            assert_eq!(f1.id, f2.id);
            assert_eq!(f1.vaelith_name, f2.vaelith_name);
            assert_eq!(f1.english_gloss, f2.english_gloss);
            assert_eq!(f1.parts, f2.parts);
            assert_eq!(f1.habitat, f2.habitat);
            assert_eq!(f1.rarity, f2.rarity);
            assert_eq!(f1.appearance, f2.appearance);
        }
    }

    #[test]
    fn different_seeds_produce_different_fruits() {
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();
        let mut rng1 = GameRng::new(1);
        let mut rng2 = GameRng::new(2);

        let fruits1 = generate_fruit_species(&mut rng1, &config, &lexicon);
        let fruits2 = generate_fruit_species(&mut rng2, &config, &lexicon);

        // Names should differ.
        let names1: Vec<_> = fruits1.iter().map(|f| &f.vaelith_name).collect();
        let names2: Vec<_> = fruits2.iter().map(|f| &f.vaelith_name).collect();
        assert_ne!(names1, names2);
    }

    #[test]
    fn generation_respects_species_count_range() {
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();

        for seed in 0..20 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_fruit_species(&mut rng, &config, &lexicon);
            assert!(
                fruits.len() >= config.min_species_per_world as usize,
                "Seed {}: got {} fruits, min {}",
                seed,
                fruits.len(),
                config.min_species_per_world
            );
            assert!(
                fruits.len() <= config.max_species_per_world as usize,
                "Seed {}: got {} fruits, max {}",
                seed,
                fruits.len(),
                config.max_species_per_world
            );
        }
    }

    #[test]
    fn all_fruit_names_unique() {
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();

        for seed in 0..10 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_fruit_species(&mut rng, &config, &lexicon);
            let mut names = BTreeSet::new();
            for f in &fruits {
                assert!(
                    names.insert(&f.vaelith_name),
                    "Seed {}: duplicate name '{}'",
                    seed,
                    f.vaelith_name
                );
            }
        }
    }

    #[test]
    fn all_parts_sum_to_100() {
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();
        let mut rng = GameRng::new(42);
        let fruits = generate_fruit_species(&mut rng, &config, &lexicon);

        for fruit in &fruits {
            let sum: u16 = fruit.parts.iter().map(|p| p.yield_percent as u16).sum();
            assert_eq!(
                sum, 100,
                "Fruit '{}' parts sum to {}, not 100",
                fruit.vaelith_name, sum
            );
        }
    }

    #[test]
    fn no_within_part_exclusion_violations() {
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();
        let mut rng = GameRng::new(42);
        let fruits = generate_fruit_species(&mut rng, &config, &lexicon);

        for fruit in &fruits {
            for part in &fruit.parts {
                // Check luminescent + pigment.
                if part.pigment.is_some() {
                    assert!(
                        !part.properties.contains(&PartProperty::Luminescent),
                        "Fruit '{}': part {:?} has both Luminescent and pigment",
                        fruit.vaelith_name,
                        part.part_type
                    );
                }
                // Check all exclusion groups.
                for group in exclusion_groups() {
                    let in_group: Vec<_> = part
                        .properties
                        .iter()
                        .filter(|p| group.contains(p))
                        .collect();
                    assert!(
                        in_group.len() <= 1,
                        "Fruit '{}': part {:?} has multiple props from exclusion group: {:?}",
                        fruit.vaelith_name,
                        part.part_type,
                        in_group
                    );
                }
            }
        }
    }

    #[test]
    fn coverage_satisfied_across_seeds() {
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();

        for seed in 0..10 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_fruit_species(&mut rng, &config, &lexicon);

            let mut tracker = CoverageTracker::new(&config);
            for fruit in &fruits {
                tracker.record_fruit(fruit);
            }
            let gaps = tracker.unfilled_categories();
            assert!(
                gaps.is_empty(),
                "Seed {}: coverage gaps remain: {:?}",
                seed,
                gaps
            );
        }
    }

    #[test]
    fn sequential_ids() {
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();
        let mut rng = GameRng::new(42);
        let fruits = generate_fruit_species(&mut rng, &config, &lexicon);

        for (i, fruit) in fruits.iter().enumerate() {
            assert_eq!(fruit.id, FruitSpeciesId(i as u16));
        }
    }

    #[test]
    fn serde_roundtrip_fruit_species() {
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();
        let mut rng = GameRng::new(42);
        let fruits = generate_fruit_species(&mut rng, &config, &lexicon);

        for fruit in &fruits {
            let json = serde_json::to_string(fruit).unwrap();
            let parsed: FruitSpecies = serde_json::from_str(&json).unwrap();
            assert_eq!(*fruit, parsed);
        }
    }

    #[test]
    fn serde_roundtrip_coverage_category() {
        for &cat in CoverageCategory::ALL.iter() {
            let json = serde_json::to_string(&cat).unwrap();
            let parsed: CoverageCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(cat, parsed);
        }
    }

    #[test]
    fn serde_roundtrip_part_type() {
        for &pt in PartType::ALL.iter() {
            let json = serde_json::to_string(&pt).unwrap();
            let parsed: PartType = serde_json::from_str(&json).unwrap();
            assert_eq!(pt, parsed);
        }
    }

    #[test]
    fn serde_roundtrip_part_property() {
        for &pp in PartProperty::ALL.iter() {
            let json = serde_json::to_string(&pp).unwrap();
            let parsed: PartProperty = serde_json::from_str(&json).unwrap();
            assert_eq!(pp, parsed);
        }
    }

    #[test]
    fn serde_roundtrip_dye_color() {
        for &color in &[
            DyeColor::Red,
            DyeColor::Yellow,
            DyeColor::Blue,
            DyeColor::Black,
            DyeColor::White,
            DyeColor::Orange,
            DyeColor::Green,
            DyeColor::Violet,
        ] {
            let json = serde_json::to_string(&color).unwrap();
            let parsed: DyeColor = serde_json::from_str(&json).unwrap();
            assert_eq!(color, parsed);
        }
    }

    #[test]
    fn fruit_species_helper_methods() {
        let fruit = FruitSpecies {
            id: FruitSpeciesId(0),
            vaelith_name: "Test".to_string(),
            english_gloss: "test".to_string(),
            parts: vec![
                FruitPart {
                    part_type: PartType::Flesh,
                    properties: [PartProperty::Starchy].into_iter().collect(),
                    pigment: None,
                    yield_percent: 70,
                },
                FruitPart {
                    part_type: PartType::Rind,
                    properties: [PartProperty::Aromatic].into_iter().collect(),
                    pigment: Some(DyeColor::Red),
                    yield_percent: 30,
                },
            ],
            habitat: GrowthHabitat::Branch,
            rarity: Rarity::Common,
            greenhouse_cultivable: true,
            appearance: FruitAppearance {
                exterior_color: FruitColor {
                    r: 200,
                    g: 50,
                    b: 50,
                },
                shape: FruitShape::Round,
                size_percent: 100,
                glows: false,
            },
        };

        assert!(fruit.has_property(PartProperty::Starchy));
        assert!(fruit.has_property(PartProperty::Aromatic));
        assert!(!fruit.has_property(PartProperty::Sweet));
        assert!(fruit.has_pigment());
        assert_eq!(fruit.all_pigments(), vec![DyeColor::Red]);
        assert_eq!(fruit.all_properties().len(), 2);
    }
}
