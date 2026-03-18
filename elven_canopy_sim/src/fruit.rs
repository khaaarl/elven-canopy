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
// Naming uses a temperature-weighted affinity scoring algorithm (see
// `assign_fruit_names`). A static affinity table maps each botanical root
// to trait dimensions (property, pigment, shape, habitat) with u8 weights.
// For each fruit, roots are sampled with probability proportional to
// score^temperature, producing names that reflect the fruit's most notable
// characteristics. Fruits that don't score well against any roots fall back
// to world-naming via `names.rs` with genitive case.
//
// See also: `worldgen.rs` for the generator entry point, `config.rs` for
// `FruitConfig`, `db.rs` for the `FruitSpecies` tabulosity table,
// `docs/drafts/fruit_variety.md` for the full design document.
//
// **Critical constraint: determinism.** All generation uses the worldgen PRNG.
// No iterated HashMap (use LookupMap for point queries), no float (all integer math), no system entropy.

use std::collections::{BTreeMap, BTreeSet};

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

    /// The `ItemKind` produced when this part type is extracted from a fruit.
    pub fn extracted_item_kind(self) -> crate::inventory::ItemKind {
        use crate::inventory::ItemKind;
        match self {
            PartType::Flesh => ItemKind::Pulp,
            PartType::Rind => ItemKind::Husk,
            PartType::Seed => ItemKind::Seed,
            PartType::Fiber => ItemKind::FruitFiber,
            PartType::Sap => ItemKind::FruitSap,
            PartType::Resin => ItemKind::FruitResin,
        }
    }
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

/// Properties that drive crafting recipe generation. Each of these must appear
/// on at most one part per fruit to avoid ambiguous recipes (e.g., two different
/// "Species Flour" recipes from different components of the same fruit).
pub const RECIPE_RELEVANT_PROPERTIES: [PartProperty; 3] = [
    PartProperty::Starchy,
    PartProperty::FibrousCoarse,
    PartProperty::FibrousFine,
];

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

    /// Human-readable display name for this dye color.
    pub fn display_name(self) -> &'static str {
        match self {
            DyeColor::Red => "Red",
            DyeColor::Yellow => "Yellow",
            DyeColor::Blue => "Blue",
            DyeColor::Black => "Black",
            DyeColor::White => "White",
            DyeColor::Orange => "Orange",
            DyeColor::Green => "Green",
            DyeColor::Violet => "Violet",
        }
    }

    /// Convert this dye color to an `ItemColor` for use on dyed item stacks.
    pub fn to_item_color(self) -> crate::inventory::ItemColor {
        let rgb = dye_to_rgb(self);
        crate::inventory::ItemColor::new(rgb.r, rgb.g, rgb.b)
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
    /// How many units of this component a single fruit yields when processed.
    /// Each part's units are independent (they do not sum to a fixed total).
    /// The fruit's overall "size" is simply the sum of all parts' units.
    /// Typical range: 10-100.
    pub component_units: u16,
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

    /// Total units across all parts — a proxy for the fruit's overall size.
    pub fn total_units(&self) -> u32 {
        self.parts.iter().map(|p| p.component_units as u32).sum()
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
/// Names are left empty — call `assign_fruit_names` as a post-generation pass.
pub fn generate_fruit_species(rng: &mut GameRng, config: &FruitConfig) -> Vec<FruitSpecies> {
    let total_count = config.min_species_per_world
        + (rng.next_u64()
            % (config.max_species_per_world - config.min_species_per_world + 1) as u64)
            as u16;

    let mut tracker = CoverageTracker::new(config);
    let mut fruits = Vec::new();

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

        let fruit = FruitSpecies {
            id,
            vaelith_name: String::new(),
            english_gloss: String::new(),
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

    // Allocate component units per part (independent values, 10-100 each).
    let units = allocate_component_units(rng, chosen_types.len());

    // Generate properties for each part. The first part gets biased toward
    // filling coverage gaps; remaining parts get random properties.
    let mut parts = Vec::new();
    for (i, (&pt, &u)) in chosen_types.iter().zip(units.iter()).enumerate() {
        let (properties, pigment) =
            generate_part_properties(rng, pt, if i == 0 { coverage_gaps } else { &[] });
        parts.push(FruitPart {
            part_type: pt,
            properties,
            pigment,
            component_units: u,
        });
    }

    // Dedup recipe-relevant properties across parts: each may appear on at most
    // one part per fruit. The first part to carry a property keeps it; later
    // parts have it stripped. This prevents ambiguous crafting recipes.
    let mut seen_recipe_props = BTreeSet::new();
    for part in &mut parts {
        part.properties.retain(|prop| {
            if RECIPE_RELEVANT_PROPERTIES.contains(prop) {
                seen_recipe_props.insert(*prop)
                // insert returns true if newly added (first occurrence) → keep it
            } else {
                true
            }
        });
        // If stripping left this part propertyless, add Bland as fallback.
        if part.properties.is_empty() {
            part.properties.insert(PartProperty::Bland);
        }
    }

    parts
}

/// Allocate independent component unit values for N parts.
/// Each part gets a value in [10, 100] — they do not sum to a fixed total.
fn allocate_component_units(rng: &mut GameRng, count: usize) -> Vec<u16> {
    (0..count)
        .map(|_| 10 + (rng.next_u64() % 91) as u16)
        .collect()
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
        .any(|p| p.part_type == PartType::Flesh && p.component_units >= 50);

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
// Affinity-based naming
// ---------------------------------------------------------------------------

/// Habitat/character affinities for roots like "deep," "wild," "ancient."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HabitatTrait {
    LowTrunk,
    Wild,
}

/// Trait dimensions for root-fruit affinity scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AffinityTrait {
    Property(PartProperty),
    Pigment(DyeColor),
    Shape(FruitShape),
    Habitat(HabitatTrait),
}

/// Affinity between a botanical root (by gloss) and fruit traits.
struct RootAffinity {
    gloss: &'static str,
    affinities: &'static [(AffinityTrait, u8)],
}

/// A botanical root joined with its lexicon entry and affinity data.
struct JoinedRoot<'a> {
    entry: &'a elven_canopy_lang::LexEntry,
    affinities: &'static [(AffinityTrait, u8)],
    is_shape_root: bool,
}

/// Static affinity table mapping all 48 botanical glosses to their trait
/// affinities. Weights are on a 1-8 scale.
static ROOT_AFFINITIES: &[RootAffinity] = &[
    // Shape roots
    RootAffinity {
        gloss: "berry",
        affinities: &[
            (AffinityTrait::Shape(FruitShape::Clustered), 6),
            (AffinityTrait::Shape(FruitShape::Round), 3),
        ],
    },
    RootAffinity {
        gloss: "pod",
        affinities: &[(AffinityTrait::Shape(FruitShape::Pod), 8)],
    },
    RootAffinity {
        gloss: "nut",
        affinities: &[(AffinityTrait::Shape(FruitShape::Nut), 8)],
    },
    RootAffinity {
        gloss: "gourd",
        affinities: &[(AffinityTrait::Shape(FruitShape::Gourd), 8)],
    },
    RootAffinity {
        gloss: "cluster",
        affinities: &[(AffinityTrait::Shape(FruitShape::Clustered), 8)],
    },
    RootAffinity {
        gloss: "husk",
        affinities: &[
            (AffinityTrait::Shape(FruitShape::Pod), 5),
            (AffinityTrait::Shape(FruitShape::Nut), 3),
        ],
    },
    RootAffinity {
        gloss: "blossom",
        affinities: &[
            (AffinityTrait::Shape(FruitShape::Round), 4),
            (AffinityTrait::Property(PartProperty::Aromatic), 3),
        ],
    },
    RootAffinity {
        gloss: "seed",
        affinities: &[
            (AffinityTrait::Shape(FruitShape::Nut), 4),
            (AffinityTrait::Shape(FruitShape::Pod), 3),
        ],
    },
    // Pigment roots
    RootAffinity {
        gloss: "red",
        affinities: &[(AffinityTrait::Pigment(DyeColor::Red), 8)],
    },
    RootAffinity {
        gloss: "orange",
        affinities: &[
            (AffinityTrait::Pigment(DyeColor::Red), 4),
            (AffinityTrait::Pigment(DyeColor::Yellow), 4),
        ],
    },
    RootAffinity {
        gloss: "yellow",
        affinities: &[(AffinityTrait::Pigment(DyeColor::Yellow), 8)],
    },
    RootAffinity {
        gloss: "golden",
        affinities: &[
            (AffinityTrait::Pigment(DyeColor::Yellow), 6),
            (AffinityTrait::Property(PartProperty::ManaResonant), 2),
        ],
    },
    RootAffinity {
        gloss: "green",
        affinities: &[
            (AffinityTrait::Pigment(DyeColor::Red), 2),
            (AffinityTrait::Pigment(DyeColor::Yellow), 2),
            (AffinityTrait::Pigment(DyeColor::Blue), 2),
        ],
    },
    RootAffinity {
        gloss: "blue",
        affinities: &[(AffinityTrait::Pigment(DyeColor::Blue), 8)],
    },
    RootAffinity {
        gloss: "violet",
        affinities: &[
            (AffinityTrait::Pigment(DyeColor::Red), 3),
            (AffinityTrait::Pigment(DyeColor::Blue), 5),
        ],
    },
    RootAffinity {
        gloss: "black",
        affinities: &[(AffinityTrait::Pigment(DyeColor::Black), 8)],
    },
    RootAffinity {
        gloss: "white",
        affinities: &[(AffinityTrait::Pigment(DyeColor::White), 8)],
    },
    RootAffinity {
        gloss: "pale",
        affinities: &[
            (AffinityTrait::Pigment(DyeColor::White), 5),
            (AffinityTrait::Property(PartProperty::Bland), 2),
        ],
    },
    RootAffinity {
        gloss: "dark",
        affinities: &[
            (AffinityTrait::Pigment(DyeColor::Black), 6),
            (AffinityTrait::Habitat(HabitatTrait::LowTrunk), 2),
        ],
    },
    // Property roots
    RootAffinity {
        gloss: "dream",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Psychoactive), 8),
            (AffinityTrait::Property(PartProperty::ManaResonant), 2),
        ],
    },
    RootAffinity {
        gloss: "fruit",
        affinities: &[
            (AffinityTrait::Shape(FruitShape::Round), 2),
            (AffinityTrait::Shape(FruitShape::Oblong), 2),
        ],
    },
    RootAffinity {
        gloss: "nectar",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Sweet), 6),
            (AffinityTrait::Property(PartProperty::Aromatic), 3),
        ],
    },
    RootAffinity {
        gloss: "sap",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Adhesive), 4),
            (AffinityTrait::Property(PartProperty::Medicinal), 3),
        ],
    },
    RootAffinity {
        gloss: "pulp",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Sweet), 3),
            (AffinityTrait::Property(PartProperty::Starchy), 3),
        ],
    },
    RootAffinity {
        gloss: "fiber",
        affinities: &[
            (AffinityTrait::Property(PartProperty::FibrousFine), 8),
            (AffinityTrait::Property(PartProperty::FibrousCoarse), 4),
        ],
    },
    RootAffinity {
        gloss: "resin",
        affinities: &[(AffinityTrait::Property(PartProperty::Adhesive), 8)],
    },
    RootAffinity {
        gloss: "mana",
        affinities: &[(AffinityTrait::Property(PartProperty::ManaResonant), 8)],
    },
    RootAffinity {
        gloss: "sweet",
        affinities: &[(AffinityTrait::Property(PartProperty::Sweet), 8)],
    },
    RootAffinity {
        gloss: "bitter",
        affinities: &[(AffinityTrait::Property(PartProperty::Bitter), 8)],
    },
    RootAffinity {
        gloss: "bland",
        affinities: &[(AffinityTrait::Property(PartProperty::Bland), 8)],
    },
    RootAffinity {
        gloss: "sharp",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Stimulant), 6),
            (AffinityTrait::Property(PartProperty::Bitter), 3),
        ],
    },
    RootAffinity {
        gloss: "rich",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Oily), 7),
            (AffinityTrait::Property(PartProperty::Sweet), 2),
        ],
    },
    RootAffinity {
        gloss: "smooth",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Oily), 3),
            (AffinityTrait::Property(PartProperty::FibrousFine), 2),
        ],
    },
    RootAffinity {
        gloss: "rough",
        affinities: &[
            (AffinityTrait::Property(PartProperty::FibrousCoarse), 7),
            (AffinityTrait::Property(PartProperty::Tough), 3),
        ],
    },
    RootAffinity {
        gloss: "spiky",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Tough), 5),
            (AffinityTrait::Shape(FruitShape::Clustered), 2),
        ],
    },
    RootAffinity {
        gloss: "soft",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Sweet), 2),
            (AffinityTrait::Property(PartProperty::Bland), 3),
        ],
    },
    RootAffinity {
        gloss: "hard",
        affinities: &[(AffinityTrait::Property(PartProperty::Tough), 8)],
    },
    RootAffinity {
        gloss: "dry",
        affinities: &[
            (AffinityTrait::Property(PartProperty::FibrousCoarse), 3),
            (AffinityTrait::Property(PartProperty::Bland), 2),
        ],
    },
    RootAffinity {
        gloss: "fragrant",
        affinities: &[(AffinityTrait::Property(PartProperty::Aromatic), 8)],
    },
    RootAffinity {
        gloss: "sticky",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Adhesive), 7),
            (AffinityTrait::Property(PartProperty::Fermentable), 2),
        ],
    },
    RootAffinity {
        gloss: "ripe",
        affinities: &[(AffinityTrait::Property(PartProperty::Fermentable), 8)],
    },
    RootAffinity {
        gloss: "wild",
        affinities: &[(AffinityTrait::Habitat(HabitatTrait::Wild), 8)],
    },
    RootAffinity {
        gloss: "grain",
        affinities: &[(AffinityTrait::Property(PartProperty::Starchy), 8)],
    },
    RootAffinity {
        gloss: "honey",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Sweet), 7),
            (AffinityTrait::Property(PartProperty::Aromatic), 2),
        ],
    },
    RootAffinity {
        gloss: "spice",
        affinities: &[(AffinityTrait::Property(PartProperty::Stimulant), 8)],
    },
    RootAffinity {
        gloss: "heal",
        affinities: &[(AffinityTrait::Property(PartProperty::Medicinal), 8)],
    },
    RootAffinity {
        gloss: "glow",
        affinities: &[(AffinityTrait::Property(PartProperty::Luminescent), 8)],
    },
    RootAffinity {
        gloss: "rind",
        affinities: &[
            (AffinityTrait::Property(PartProperty::Tough), 3),
            (AffinityTrait::Shape(FruitShape::Gourd), 2),
        ],
    },
];

/// Check if a root is a shape root (highest-weight affinity is Shape).
fn is_shape_root(affinities: &[(AffinityTrait, u8)]) -> bool {
    affinities
        .iter()
        .max_by_key(|(_, w)| w)
        .map(|(t, _)| matches!(t, AffinityTrait::Shape(_)))
        .unwrap_or(false)
}

/// Compute trait intensity for a fruit. Properties/pigments use component_units sums.
/// Shapes and habitats use binary 0/1.
fn fruit_trait_intensity(fruit: &FruitSpecies, trait_: AffinityTrait) -> u32 {
    match trait_ {
        AffinityTrait::Property(prop) => fruit
            .parts
            .iter()
            .filter(|p| p.properties.contains(&prop))
            .map(|p| p.component_units as u32)
            .sum(),
        AffinityTrait::Pigment(color) => fruit
            .parts
            .iter()
            .filter(|p| p.pigment == Some(color))
            .map(|p| p.component_units as u32)
            .sum(),
        AffinityTrait::Shape(shape) => {
            if fruit.appearance.shape == shape {
                1
            } else {
                0
            }
        }
        AffinityTrait::Habitat(_) => {
            // Habitat traits could map to GrowthHabitat in the future.
            // For now, all habitats return 0 — habitat roots are only
            // reachable via world-naming.
            0
        }
    }
}

/// Score a (fruit, root) pair: sum of affinity_weight * intensity.
fn score_fruit_root(fruit: &FruitSpecies, affinities: &[(AffinityTrait, u8)]) -> u32 {
    affinities
        .iter()
        .map(|&(trait_, weight)| weight as u32 * fruit_trait_intensity(fruit, trait_))
        .sum()
}

/// Assign names to all fruit species using temperature-weighted affinity scoring.
///
/// Must be called after all fruit species are generated (post-generation pass).
/// Uses the botanical pool from the lexicon and the static affinity table.
///
/// **Algorithm overview:**
/// 1. Join botanical roots with affinities, precompute all (fruit, root) scores.
/// 2. Iteratively assign roots to fruits over 10 passes with temperature-scaled
///    weighted sampling. A fruit is name-ready when it has >= 1 property root
///    and >= 1 shape root, or >= 2 property roots, with a unique root combination.
/// 3. Compose Vaelith names from assigned roots: property root(s) first, shape
///    root last.
/// 4. Fruits that fail affinity naming fall back to world-naming (genitive case
///    name from the Given pool + a shape noun).
/// 5. Uniqueness enforcement: string-level collisions demote the lower-scoring
///    fruit to world-naming.
pub fn assign_fruit_names(
    fruits: &mut [FruitSpecies],
    rng: &mut GameRng,
    config: &FruitConfig,
    lexicon: &elven_canopy_lang::Lexicon,
) {
    if fruits.is_empty() {
        return;
    }

    let botanical_pool = lexicon.by_name_tag(elven_canopy_lang::NameTag::Botanical);
    if botanical_pool.is_empty() {
        // Fallback if no botanical entries.
        for (i, fruit) in fruits.iter_mut().enumerate() {
            fruit.vaelith_name = format!("Vela{}", i);
            fruit.english_gloss = "fruit".to_string();
        }
        return;
    }

    // Build gloss -> RootAffinity lookup.
    let affinity_map: BTreeMap<&str, &RootAffinity> =
        ROOT_AFFINITIES.iter().map(|ra| (ra.gloss, ra)).collect();

    // Phase 1: Join botanical roots with affinities.
    let joined_roots: Vec<JoinedRoot> = botanical_pool
        .iter()
        .map(|entry| {
            let affinities = affinity_map
                .get(entry.gloss.as_str())
                .map(|ra| ra.affinities)
                .unwrap_or(&[]);
            JoinedRoot {
                entry,
                affinities,
                is_shape_root: is_shape_root(affinities),
            }
        })
        .collect();

    let fruit_count = fruits.len();
    let root_count = joined_roots.len();

    // Precompute scores: scores[fruit_idx][root_idx].
    let scores: Vec<Vec<u32>> = fruits
        .iter()
        .map(|fruit| {
            joined_roots
                .iter()
                .map(|root| score_fruit_root(fruit, root.affinities))
                .collect()
        })
        .collect();

    // Phase 2: Iterative root assignment (10 passes, fruit-first).
    let mut assigned_roots: Vec<Vec<usize>> = vec![Vec::new(); fruit_count];
    let mut name_ready: Vec<bool> = vec![false; fruit_count];
    // Track unique root combinations (sorted sets) to prevent duplicates.
    let mut used_combos: BTreeSet<Vec<usize>> = BTreeSet::new();

    for _pass in 0..10 {
        for fi in 0..fruit_count {
            if name_ready[fi] {
                continue;
            }

            // Compute temperature-scaled weights for each root.
            let mut weights: Vec<u64> = Vec::with_capacity(root_count);
            let mut total_weight: u64 = 0;
            for ri in 0..root_count {
                let score = scores[fi][ri] as u64;
                let temp = if joined_roots[ri].is_shape_root {
                    config.naming_temperature
                } else {
                    config.naming_temperature * 2
                };
                let weight = score.saturating_pow(temp);
                weights.push(weight);
                total_weight = total_weight.saturating_add(weight);
            }

            if total_weight == 0 {
                continue;
            }

            // Weighted sample using cumulative sums.
            let roll = rng.range_u64(0, total_weight);
            let mut cumulative = 0u64;
            let mut chosen_ri = 0;
            for (ri, &w) in weights.iter().enumerate() {
                cumulative = cumulative.saturating_add(w);
                if roll < cumulative {
                    chosen_ri = ri;
                    break;
                }
            }

            assigned_roots[fi].push(chosen_ri);
        }

        // Check name-readiness after each pass.
        for fi in 0..fruit_count {
            if name_ready[fi] {
                continue;
            }
            let roots = &assigned_roots[fi];
            let property_count = roots
                .iter()
                .filter(|&&ri| !joined_roots[ri].is_shape_root)
                .count();
            let shape_count = roots
                .iter()
                .filter(|&&ri| joined_roots[ri].is_shape_root)
                .count();

            let structurally_ready =
                (property_count >= 1 && shape_count >= 1) || (property_count >= 2);

            if structurally_ready {
                let mut sorted_combo: Vec<usize> = roots.clone();
                sorted_combo.sort();
                sorted_combo.dedup();
                if !used_combos.contains(&sorted_combo) {
                    used_combos.insert(sorted_combo);
                    name_ready[fi] = true;
                }
            }
        }
    }

    // Phase 3: Name composition.
    let given_pool = lexicon.by_name_tag(elven_canopy_lang::NameTag::Given);

    // Track display names for uniqueness enforcement.
    let mut display_names: BTreeMap<String, usize> = BTreeMap::new();
    // Track which fruits are world-named.
    let mut world_named: Vec<bool> = vec![false; fruit_count];

    for fi in 0..fruit_count {
        if name_ready[fi] {
            // Affinity-named: property root(s) first, shape root last.
            let roots = &assigned_roots[fi];
            let mut property_indices: Vec<usize> = roots
                .iter()
                .copied()
                .filter(|&ri| !joined_roots[ri].is_shape_root)
                .collect();
            let mut shape_indices: Vec<usize> = roots
                .iter()
                .copied()
                .filter(|&ri| joined_roots[ri].is_shape_root)
                .collect();

            // Dedup each group (keep first occurrence order).
            dedup_preserve_order(&mut property_indices);
            dedup_preserve_order(&mut shape_indices);

            // Compose: property roots first, then one shape root.
            let mut name_parts: Vec<usize> = Vec::new();
            for &ri in &property_indices {
                name_parts.push(ri);
            }
            if let Some(&sri) = shape_indices.first() {
                name_parts.push(sri);
            }

            // If we ended up with only shape roots (no property roots), use them as-is.
            if name_parts.is_empty() {
                for &ri in &shape_indices {
                    name_parts.push(ri);
                }
            }

            // Build Vaelith name (concatenated roots, capitalized).
            let vaelith: String = name_parts
                .iter()
                .map(|&ri| joined_roots[ri].entry.root.as_str())
                .collect();
            let vaelith_name = capitalize(&vaelith);

            // Build English gloss: "gloss1-gloss2" format.
            let english_gloss: String = name_parts
                .iter()
                .map(|&ri| joined_roots[ri].entry.gloss.as_str())
                .collect::<Vec<_>>()
                .join("-");

            // Build display name: "{Vaelith} {ShapeNoun}"
            let shape_noun = if let Some(&sri) = shape_indices.first() {
                shape_noun_from_gloss(joined_roots[sri].entry.gloss.as_str())
            } else {
                fruits[fi].appearance.shape.item_noun()
            };
            let display_name = format!("{} {}", vaelith_name, shape_noun);

            fruits[fi].vaelith_name = display_name.clone();
            fruits[fi].english_gloss = english_gloss;

            if let Some(&prev_fi) = display_names.get(&display_name) {
                // Collision — demote the lower-scoring fruit to world-naming.
                let score_a: u32 = assigned_roots[prev_fi]
                    .iter()
                    .map(|&ri| scores[prev_fi][ri])
                    .sum();
                let score_b: u32 = assigned_roots[fi].iter().map(|&ri| scores[fi][ri]).sum();
                if score_b < score_a {
                    // Demote current fruit.
                    world_named[fi] = true;
                } else {
                    // Demote previous fruit.
                    world_named[prev_fi] = true;
                    display_names.insert(display_name, fi);
                }
            } else {
                display_names.insert(display_name, fi);
            }
        } else {
            world_named[fi] = true;
        }
    }

    // Phase 3b: World-naming for fruits that failed affinity naming or were demoted.
    let mut used_world_names: BTreeSet<String> = BTreeSet::new();
    // Collect existing display names for collision checking.
    for fi in 0..fruit_count {
        if !world_named[fi] {
            used_world_names.insert(fruits[fi].vaelith_name.clone());
        }
    }

    for fi in 0..fruit_count {
        if !world_named[fi] {
            continue;
        }

        // Pick a shape root from the botanical pool for the noun.
        let shape_root_indices: Vec<usize> = (0..root_count)
            .filter(|&ri| joined_roots[ri].is_shape_root)
            .collect();
        let shape_noun = if !shape_root_indices.is_empty() {
            let sri = shape_root_indices[rng.range_usize(0, shape_root_indices.len())];
            shape_noun_from_gloss(joined_roots[sri].entry.gloss.as_str())
        } else {
            fruits[fi].appearance.shape.item_noun()
        };

        // Generate a world-name using the Given pool with genitive case.
        for _attempt in 0..50 {
            let (name_text, _meaning, vowel_class) =
                elven_canopy_lang::names::generate_name_part(&given_pool, rng);

            // Apply genitive case suffix: -li (front) / -lu (back).
            let genitive_suffix = match vowel_class {
                elven_canopy_lang::VowelClass::Front => "li",
                elven_canopy_lang::VowelClass::Back => "lu",
            };
            let genitive_name = format!("{}{}", name_text, genitive_suffix);
            let display_name = format!("{} {}", genitive_name, shape_noun);

            if !used_world_names.contains(&display_name) {
                used_world_names.insert(display_name.clone());
                fruits[fi].vaelith_name = display_name;
                fruits[fi].english_gloss = format!("world-{}", shape_noun.to_lowercase());
                break;
            }
        }

        // If all 50 attempts failed, use index-based fallback.
        if fruits[fi].vaelith_name.is_empty() {
            let fallback = format!("Vela{} {}", fi, shape_noun);
            fruits[fi].vaelith_name = fallback.clone();
            fruits[fi].english_gloss = "fruit".to_string();
            used_world_names.insert(fallback);
        }
    }
}

/// Map a botanical gloss to a display noun for fruit names.
fn shape_noun_from_gloss(gloss: &str) -> &'static str {
    match gloss {
        "berry" => "Berry",
        "pod" => "Pod",
        "nut" => "Nut",
        "gourd" => "Gourd",
        "cluster" => "Cluster",
        "husk" => "Husk",
        "blossom" => "Blossom",
        "seed" => "Seed",
        "fruit" => "Fruit",
        _ => "Fruit",
    }
}

/// Deduplicate a vector while preserving first-occurrence order.
fn dedup_preserve_order(v: &mut Vec<usize>) {
    let mut seen = BTreeSet::new();
    v.retain(|x| seen.insert(*x));
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

    /// Helper: generate fruit species and assign names (the two-step API).
    fn generate_and_name(rng: &mut GameRng, config: &FruitConfig) -> Vec<FruitSpecies> {
        let lexicon = elven_canopy_lang::default_lexicon();
        let mut fruits = generate_fruit_species(rng, config);
        assign_fruit_names(&mut fruits, rng, config, &lexicon);
        fruits
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

    // --- Component unit allocation ---

    #[test]
    fn component_units_in_range() {
        let mut rng = GameRng::new(42);
        for _ in 0..100 {
            for count in 1..=4 {
                let units = allocate_component_units(&mut rng, count);
                assert_eq!(units.len(), count);
                for &u in &units {
                    assert!(
                        (10..=100).contains(&u),
                        "Component units should be in [10, 100], got {}",
                        u
                    );
                }
            }
        }
    }

    #[test]
    fn empty_parts_yield_empty() {
        let mut rng = GameRng::new(0);
        let units = allocate_component_units(&mut rng, 0);
        assert!(units.is_empty());
    }

    // --- Full generation ---

    #[test]
    fn generation_is_deterministic() {
        let config = test_config();
        let mut rng1 = GameRng::new(42);
        let mut rng2 = GameRng::new(42);

        let fruits1 = generate_and_name(&mut rng1, &config);
        let fruits2 = generate_and_name(&mut rng2, &config);

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
        let mut rng1 = GameRng::new(1);
        let mut rng2 = GameRng::new(2);

        let fruits1 = generate_and_name(&mut rng1, &config);
        let fruits2 = generate_and_name(&mut rng2, &config);

        // Names should differ.
        let names1: Vec<_> = fruits1.iter().map(|f| &f.vaelith_name).collect();
        let names2: Vec<_> = fruits2.iter().map(|f| &f.vaelith_name).collect();
        assert_ne!(names1, names2);
    }

    #[test]
    fn generation_respects_species_count_range() {
        let config = test_config();

        for seed in 0..20 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_fruit_species(&mut rng, &config);
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

        for seed in 0..10 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_and_name(&mut rng, &config);
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
    fn all_component_units_in_range() {
        let config = test_config();
        let mut rng = GameRng::new(42);
        let fruits = generate_and_name(&mut rng, &config);

        for fruit in &fruits {
            for part in &fruit.parts {
                assert!(
                    (10..=100).contains(&part.component_units),
                    "Fruit '{}' part {:?} has component_units {}, expected [10, 100]",
                    fruit.vaelith_name,
                    part.part_type,
                    part.component_units,
                );
            }
            assert!(
                fruit.total_units() > 0,
                "Fruit '{}' has zero total units",
                fruit.vaelith_name,
            );
        }
    }

    #[test]
    fn no_within_part_exclusion_violations() {
        let config = test_config();
        let mut rng = GameRng::new(42);
        let fruits = generate_and_name(&mut rng, &config);

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

    /// Recipe-relevant properties (Starchy, FibrousCoarse, FibrousFine) must
    /// appear on at most one part per fruit. If two parts carried the same
    /// recipe-relevant property, we'd generate ambiguous crafting recipes
    /// (e.g., two different "Species Flour" recipes from different components).
    #[test]
    fn no_cross_part_recipe_property_repeats() {
        let config = test_config();

        for seed in 0..50 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_fruit_species(&mut rng, &config);

            for fruit in &fruits {
                for &prop in &RECIPE_RELEVANT_PROPERTIES {
                    let parts_with: Vec<_> = fruit
                        .parts
                        .iter()
                        .filter(|p| p.properties.contains(&prop))
                        .map(|p| p.part_type)
                        .collect();
                    assert!(
                        parts_with.len() <= 1,
                        "Seed {}: fruit {:?} has {:?} on multiple parts: {:?}",
                        seed,
                        fruit.id,
                        prop,
                        parts_with,
                    );
                }
            }
        }
    }

    #[test]
    fn coverage_satisfied_across_seeds() {
        let config = test_config();

        for seed in 0..10 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_fruit_species(&mut rng, &config);

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
        let mut rng = GameRng::new(42);
        let fruits = generate_fruit_species(&mut rng, &config);

        for (i, fruit) in fruits.iter().enumerate() {
            assert_eq!(fruit.id, FruitSpeciesId(i as u16));
        }
    }

    #[test]
    fn serde_roundtrip_fruit_species() {
        let config = test_config();
        let mut rng = GameRng::new(42);
        let fruits = generate_and_name(&mut rng, &config);

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
                    component_units: 70,
                },
                FruitPart {
                    part_type: PartType::Rind,
                    properties: [PartProperty::Aromatic].into_iter().collect(),
                    pigment: Some(DyeColor::Red),
                    component_units: 30,
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

    // --- Affinity naming tests ---

    #[test]
    fn zero_collisions_across_100_seeds() {
        let config = test_config();
        for seed in 0..100 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_and_name(&mut rng, &config);
            let mut names = BTreeSet::new();
            for f in &fruits {
                assert!(
                    !f.vaelith_name.is_empty(),
                    "Seed {}: fruit {:?} has empty name",
                    seed,
                    f.id
                );
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
    fn naming_determinism() {
        let config = test_config();
        for seed in [0, 42, 999] {
            let mut rng1 = GameRng::new(seed);
            let mut rng2 = GameRng::new(seed);
            let fruits1 = generate_and_name(&mut rng1, &config);
            let fruits2 = generate_and_name(&mut rng2, &config);
            assert_eq!(fruits1.len(), fruits2.len());
            for (f1, f2) in fruits1.iter().zip(fruits2.iter()) {
                assert_eq!(f1.vaelith_name, f2.vaelith_name);
                assert_eq!(f1.english_gloss, f2.english_gloss);
            }
        }
    }

    #[test]
    fn naming_variety_across_seeds() {
        let config = test_config();
        let mut all_names = BTreeSet::new();
        for seed in 0..20 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_and_name(&mut rng, &config);
            for f in &fruits {
                all_names.insert(f.vaelith_name.clone());
            }
        }
        // 20 seeds * ~20-40 fruits each = 400-800 fruits; should have >100 unique names.
        assert!(
            all_names.len() > 100,
            "Expected >100 unique names across 20 seeds, got {}",
            all_names.len()
        );
    }

    #[test]
    fn world_name_genitive_case_correctness() {
        // Try multiple seeds to ensure we find at least one world-named fruit,
        // then verify all world-named fruits use correct genitive suffixes.
        let config = test_config();
        let mut total_world_named = 0;

        for seed in 0..20 {
            let mut rng = GameRng::new(seed);
            let fruits = generate_and_name(&mut rng, &config);

            for f in &fruits {
                if f.english_gloss.starts_with("world-") {
                    total_world_named += 1;
                    // The Vaelith name should have "Name Noun" format.
                    let parts: Vec<&str> = f.vaelith_name.splitn(2, ' ').collect();
                    assert_eq!(
                        parts.len(),
                        2,
                        "World-named fruit should have 'Name Noun' format, got '{}'",
                        f.vaelith_name
                    );
                    let name_part = parts[0];
                    assert!(
                        name_part.ends_with("li") || name_part.ends_with("lu"),
                        "World-named fruit name '{}' should end with genitive suffix (li/lu)",
                        name_part
                    );
                }
            }
        }

        assert!(
            total_world_named > 0,
            "Expected at least one world-named fruit across 20 seeds"
        );
    }

    #[test]
    fn property_intensity_scoring() {
        // A fruit with 70 units of Starchy flesh should score high for "grain" root.
        let fruit = FruitSpecies {
            id: FruitSpeciesId(0),
            vaelith_name: String::new(),
            english_gloss: String::new(),
            parts: vec![FruitPart {
                part_type: PartType::Flesh,
                properties: [PartProperty::Starchy].into_iter().collect(),
                pigment: None,
                component_units: 70,
            }],
            habitat: GrowthHabitat::Branch,
            rarity: Rarity::Common,
            greenhouse_cultivable: true,
            appearance: FruitAppearance {
                exterior_color: FruitColor {
                    r: 200,
                    g: 170,
                    b: 120,
                },
                shape: FruitShape::Round,
                size_percent: 100,
                glows: false,
            },
        };

        // "grain" has Starchy affinity weight 8.
        let grain_affinities: &[(AffinityTrait, u8)] =
            &[(AffinityTrait::Property(PartProperty::Starchy), 8)];
        let score = score_fruit_root(&fruit, grain_affinities);
        // 8 * 70 = 560
        assert_eq!(score, 560);

        // "glow" has Luminescent affinity weight 8; this fruit has no Luminescent.
        let glow_affinities: &[(AffinityTrait, u8)] =
            &[(AffinityTrait::Property(PartProperty::Luminescent), 8)];
        let score = score_fruit_root(&fruit, glow_affinities);
        assert_eq!(score, 0);
    }

    #[test]
    fn bland_fruit_fallthrough_to_world_naming() {
        // A fruit with only Bland property should struggle to get affinity-named,
        // because "bland" root has low weights and the fruit has low intensity.
        let config = test_config();
        let lexicon = elven_canopy_lang::default_lexicon();

        let mut fruits = vec![FruitSpecies {
            id: FruitSpeciesId(0),
            vaelith_name: String::new(),
            english_gloss: String::new(),
            parts: vec![FruitPart {
                part_type: PartType::Flesh,
                properties: [PartProperty::Bland].into_iter().collect(),
                pigment: None,
                component_units: 100,
            }],
            habitat: GrowthHabitat::Branch,
            rarity: Rarity::Common,
            greenhouse_cultivable: true,
            appearance: FruitAppearance {
                exterior_color: FruitColor {
                    r: 140,
                    g: 160,
                    b: 100,
                },
                shape: FruitShape::Round,
                size_percent: 80,
                glows: false,
            },
        }];

        let mut rng = GameRng::new(42);
        assign_fruit_names(&mut fruits, &mut rng, &config, &lexicon);

        assert!(
            !fruits[0].vaelith_name.is_empty(),
            "Bland fruit should still get a name"
        );
    }

    #[test]
    fn shape_root_sharing() {
        // Multiple fruits with the same shape can share shape roots if their
        // property roots differ. Generate many fruits and check that shape root
        // reuse happens.
        let config = test_config();
        let mut rng = GameRng::new(42);
        let fruits = generate_and_name(&mut rng, &config);

        // Extract the last word (shape noun) from each affinity-named fruit.
        let shape_nouns: Vec<&str> = fruits
            .iter()
            .filter(|f| !f.english_gloss.starts_with("world-"))
            .filter_map(|f| f.vaelith_name.split(' ').next_back())
            .collect();

        if shape_nouns.len() > 5 {
            // Count occurrences of each shape noun.
            let mut noun_counts = BTreeMap::new();
            for noun in &shape_nouns {
                *noun_counts.entry(*noun).or_insert(0u32) += 1;
            }
            // At least one shape noun should appear more than once.
            let max_count = noun_counts.values().max().unwrap_or(&0);
            assert!(
                *max_count > 1,
                "Expected at least one shape noun reused across fruits, got counts: {:?}",
                noun_counts
            );
        }
    }

    #[test]
    fn is_shape_root_classification() {
        // Berry: Shape(Clustered) 6, Shape(Round) 3 — highest is Shape, so shape root.
        assert!(is_shape_root(&[
            (AffinityTrait::Shape(FruitShape::Clustered), 6),
            (AffinityTrait::Shape(FruitShape::Round), 3),
        ]));

        // Dream: Property(Psychoactive) 8, Property(ManaResonant) 2 — not shape root.
        assert!(!is_shape_root(&[
            (AffinityTrait::Property(PartProperty::Psychoactive), 8),
            (AffinityTrait::Property(PartProperty::ManaResonant), 2),
        ]));

        // Empty affinities — not a shape root.
        assert!(!is_shape_root(&[]));

        // Blossom: Shape(Round) 4, Property(Aromatic) 3 — highest is Shape, so shape root.
        assert!(is_shape_root(&[
            (AffinityTrait::Shape(FruitShape::Round), 4),
            (AffinityTrait::Property(PartProperty::Aromatic), 3),
        ]));
    }
}
