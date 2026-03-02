// Building face layout computation and completed structure registry types.
//
// This file has two concerns:
//
// ## Face layout computation
//
// Computes the per-face `FaceData` for each interior voxel of a building.
// A building is an axis-aligned box of `BuildingInterior` voxels sitting on
// a solid foundation (one layer below `anchor.y`). Walls don't consume voxel
// space — they exist as face restrictions on the interior voxels.
//
// The layout rules are:
// - Bottom layer: -Y = Floor on all voxels.
// - Top layer: +Y = Ceiling on all voxels.
// - Exterior side faces: Window (maximizes visibility).
// - One ground-level door: auto-placed at the center of the +Z edge.
// - Interior-facing sides: Open (no restriction).
//
// The `anchor` is the minimum corner of the interior volume at the foundation
// level (y = foundation top). Interior voxels span:
//   x: anchor.x .. anchor.x + width
//   y: anchor.y + 1 .. anchor.y + 1 + height
//   z: anchor.z .. anchor.z + depth
//
// ## Completed structure registry
//
// `CompletedStructure` records a completed build's metadata — type, bounding
// box, completion tick, optional user-editable name, and optional furnishing
// state. Created by `SimState::complete_build()` via `from_blueprint()` and
// stored in `SimState::structures`. Buildings can be furnished (e.g. as
// dormitories) via `SimAction::FurnishStructure`, which triggers incremental
// furniture placement tracked in `planned_furniture` / `furniture_positions`.
// The structure
// list panel in the UI queries these to show a browsable list of all
// completed constructions with zoom-to-location. `display_name()` returns
// the custom name if set, or a furnishing-derived name like "Dormitory #7",
// or a build-type default like "Platform #12".
//
// See also: `types.rs` for `FaceDirection`, `FaceType`, `FaceData`,
// `VoxelCoord`, `StructureId`. `sim.rs` for the `DesignateBuilding` command
// that calls face layout, and `complete_build()` that creates structures.
// `nav.rs` for how face data affects pathfinding. `blueprint.rs` for the
// blueprint data model that `from_blueprint()` consumes.
//
// **Critical constraint: determinism.** Uses `BTreeMap` for output ordering.

use crate::blueprint::Blueprint;
use crate::prng::GameRng;
use crate::types::{
    BuildType, CreatureId, FaceData, FaceDirection, FaceType, FurnishingType, ProjectId,
    StructureId, VoxelCoord,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Completed structure
// ---------------------------------------------------------------------------

/// A completed construction registered in the sim's structure list.
///
/// Created from a `Blueprint` when its build task finishes. Stores the
/// bounding box (anchor + dimensions) for the UI's zoom-to-location feature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedStructure {
    pub id: StructureId,
    pub project_id: ProjectId,
    pub build_type: BuildType,
    /// Min corner of the voxel bounding box.
    pub anchor: VoxelCoord,
    pub width: i32,
    pub depth: i32,
    pub height: i32,
    pub completed_tick: u64,
    /// User-editable name. `None` means use the auto-generated default
    /// (e.g. "Platform #12"). Saved with `#[serde(default)]` so old saves
    /// without this field deserialize correctly.
    #[serde(default)]
    pub name: Option<String>,
    /// The furnishing type applied to this building, if any.
    #[serde(default)]
    pub furnishing: Option<FurnishingType>,
    /// The elf assigned to live in this home, if any. Only meaningful when
    /// `furnishing == Some(Home)`.
    #[serde(default)]
    pub assigned_elf: Option<CreatureId>,
    /// Voxel positions of placed furniture (grows incrementally during furnishing).
    #[serde(default, alias = "bed_positions")]
    pub furniture_positions: Vec<VoxelCoord>,
    /// All planned furniture positions (computed when furnishing starts). The
    /// elf works through these one at a time; as each is placed, it moves from
    /// `planned_furniture` to `furniture_positions`.
    #[serde(default, alias = "planned_beds")]
    pub planned_furniture: Vec<VoxelCoord>,
}

impl CompletedStructure {
    /// Create a `CompletedStructure` from a completed blueprint.
    ///
    /// Computes the axis-aligned bounding box from the blueprint's voxel list.
    /// `id` and `completed_tick` are provided by the caller (`SimState`).
    pub fn from_blueprint(id: StructureId, blueprint: &Blueprint, completed_tick: u64) -> Self {
        let (anchor, width, depth, height) = Self::compute_bounding_box(&blueprint.voxels);
        Self {
            id,
            project_id: blueprint.id,
            build_type: blueprint.build_type,
            anchor,
            width,
            depth,
            height,
            completed_tick,
            name: None,
            furnishing: None,
            assigned_elf: None,
            furniture_positions: Vec::new(),
            planned_furniture: Vec::new(),
        }
    }

    /// Return the display name for this structure.
    ///
    /// If the player has set a custom name, returns that. Otherwise returns
    /// a default like "Platform #12" derived from `build_type` and `id`.
    pub fn display_name(&self) -> String {
        if let Some(ref custom) = self.name {
            return custom.clone();
        }
        // Furnished buildings use the furnishing type as the display name.
        if let Some(furnishing) = &self.furnishing {
            return format!("{} #{}", furnishing.display_str(), self.id.0);
        }
        let type_str = match self.build_type {
            BuildType::Platform => "Platform",
            BuildType::Bridge => "Bridge",
            BuildType::Stairs => "Stairs",
            BuildType::Wall => "Wall",
            BuildType::Enclosure => "Enclosure",
            BuildType::Building => "Building",
            BuildType::WoodLadder => "Wood Ladder",
            BuildType::RopeLadder => "Rope Ladder",
            BuildType::Carve => "Carve",
        };
        format!("{} #{}", type_str, self.id.0)
    }

    /// Compute the ground-floor interior voxel positions.
    ///
    /// These are the voxels at y = anchor.y, spanning
    /// x in [anchor.x .. anchor.x + width) and
    /// z in [anchor.z .. anchor.z + depth). The anchor of a CompletedStructure
    /// is the bounding-box minimum of the blueprint voxels, which for buildings
    /// are the BuildingInterior voxels (one level above the foundation).
    /// Only meaningful for Building structures.
    pub fn floor_interior_positions(&self) -> Vec<VoxelCoord> {
        let y = self.anchor.y;
        let mut positions = Vec::new();
        for z in self.anchor.z..self.anchor.z + self.depth {
            for x in self.anchor.x..self.anchor.x + self.width {
                positions.push(VoxelCoord::new(x, y, z));
            }
        }
        positions
    }

    /// Choose furniture positions for a given furnishing type.
    ///
    /// Picks positions from the ground-floor interior, skipping the door
    /// position (center of +Z edge at ground level) and positions adjacent
    /// to the door (to keep the doorway clear). Density varies by type:
    /// - Home: exactly 1
    /// - Dormitory, ConcertHall: ~1 per 2 tiles
    /// - Kitchen, Storehouse: ~1 per 3 tiles
    /// - DiningHall, Workshop: ~1 per 4 tiles
    pub fn compute_furniture_positions(
        &self,
        furnishing_type: FurnishingType,
        rng: &mut GameRng,
    ) -> Vec<VoxelCoord> {
        let floor = self.floor_interior_positions();
        if floor.is_empty() {
            return Vec::new();
        }

        // Door is at center of +Z edge, ground level (same Y as anchor).
        let door_x = self.anchor.x + self.width / 2;
        let door_y = self.anchor.y;
        let door_z = self.anchor.z + self.depth - 1;
        let door_pos = VoxelCoord::new(door_x, door_y, door_z);

        // Filter out door position and its immediate horizontal neighbors.
        let eligible: Vec<VoxelCoord> = floor
            .into_iter()
            .filter(|pos| {
                if *pos == door_pos {
                    return false;
                }
                // Skip positions adjacent to door (manhattan distance 1 on xz plane).
                let dx = (pos.x - door_pos.x).abs();
                let dz = (pos.z - door_pos.z).abs();
                dx + dz > 1
            })
            .collect();

        if eligible.is_empty() {
            return Vec::new();
        }

        // Home: exactly 1 item.
        if furnishing_type == FurnishingType::Home {
            let idx = rng.next_u64() as usize % eligible.len();
            return vec![eligible[idx]];
        }

        // Density divisor: how many floor tiles per furniture item.
        let divisor = match furnishing_type {
            FurnishingType::Dormitory | FurnishingType::ConcertHall => 2,
            FurnishingType::Kitchen | FurnishingType::Storehouse => 3,
            FurnishingType::DiningHall | FurnishingType::Workshop => 4,
            FurnishingType::Home => unreachable!(),
        };

        let total_floor = (self.width * self.depth) as usize;
        let target = (total_floor / divisor).max(1).min(eligible.len());

        // Shuffle eligible positions using the PRNG, then take the first `target`.
        let mut shuffled = eligible;
        for i in (1..shuffled.len()).rev() {
            let j = rng.next_u64() as usize % (i + 1);
            shuffled.swap(i, j);
        }
        shuffled.truncate(target);

        // Sort for deterministic ordering (BTreeMap-friendly).
        shuffled.sort();
        shuffled
    }

    /// Compute the axis-aligned bounding box of a set of voxel coordinates.
    /// Returns (min_corner, width, depth, height) where width/depth/height
    /// are at least 1.
    fn compute_bounding_box(voxels: &[VoxelCoord]) -> (VoxelCoord, i32, i32, i32) {
        if voxels.is_empty() {
            return (VoxelCoord::new(0, 0, 0), 0, 0, 0);
        }
        let mut min_x = voxels[0].x;
        let mut max_x = voxels[0].x;
        let mut min_y = voxels[0].y;
        let mut max_y = voxels[0].y;
        let mut min_z = voxels[0].z;
        let mut max_z = voxels[0].z;
        for v in &voxels[1..] {
            min_x = min_x.min(v.x);
            max_x = max_x.max(v.x);
            min_y = min_y.min(v.y);
            max_y = max_y.max(v.y);
            min_z = min_z.min(v.z);
            max_z = max_z.max(v.z);
        }
        let anchor = VoxelCoord::new(min_x, min_y, min_z);
        let width = max_x - min_x + 1;
        let depth = max_z - min_z + 1;
        let height = max_y - min_y + 1;
        (anchor, width, depth, height)
    }
}

// ---------------------------------------------------------------------------
// Face layout computation
// ---------------------------------------------------------------------------

/// Compute the face layout for a building.
///
/// `anchor` is the minimum corner of the building footprint at foundation
/// level. Interior voxels occupy the volume above the foundation:
///   x in [anchor.x, anchor.x + width)
///   y in [anchor.y + 1, anchor.y + 1 + height)
///   z in [anchor.z, anchor.z + depth)
///
/// Returns a map from interior voxel coordinate to its `FaceData`.
pub fn compute_building_face_layout(
    anchor: VoxelCoord,
    width: i32,
    depth: i32,
    height: i32,
) -> BTreeMap<VoxelCoord, FaceData> {
    let mut layout = BTreeMap::new();

    let x_min = anchor.x;
    let x_max = anchor.x + width; // exclusive
    let y_min = anchor.y + 1; // interior starts one above foundation
    let y_max = anchor.y + 1 + height; // exclusive
    let z_min = anchor.z;
    let z_max = anchor.z + depth; // exclusive

    for y in y_min..y_max {
        for z in z_min..z_max {
            for x in x_min..x_max {
                let coord = VoxelCoord::new(x, y, z);
                let mut fd = FaceData::default();

                // Bottom layer gets Floor on -Y.
                if y == y_min {
                    fd.set(FaceDirection::NegY, FaceType::Floor);
                }

                // Top layer gets Ceiling on +Y.
                if y == y_max - 1 {
                    fd.set(FaceDirection::PosY, FaceType::Ceiling);
                }

                // Exterior side faces: Window (or Door for the door position).
                if x == x_min {
                    fd.set(FaceDirection::NegX, FaceType::Window);
                }
                if x == x_max - 1 {
                    fd.set(FaceDirection::PosX, FaceType::Window);
                }
                if z == z_min {
                    fd.set(FaceDirection::NegZ, FaceType::Window);
                }
                if z == z_max - 1 {
                    fd.set(FaceDirection::PosZ, FaceType::Window);
                }

                layout.insert(coord, fd);
            }
        }
    }

    // Place door at ground level, center of +Z edge.
    let door_x = x_min + width / 2;
    let door_y = y_min;
    let door_coord = VoxelCoord::new(door_x, door_y, z_max - 1);
    if let Some(fd) = layout.get_mut(&door_coord) {
        fd.set(FaceDirection::PosZ, FaceType::Door);
    }

    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_3x3x1_layout() {
        let anchor = VoxelCoord::new(0, 0, 0);
        let layout = compute_building_face_layout(anchor, 3, 3, 1);

        // 3x3x1 = 9 interior voxels (at y=1).
        assert_eq!(layout.len(), 9);

        // All voxels should be at y=1.
        for coord in layout.keys() {
            assert_eq!(coord.y, 1);
        }

        // Corner voxel (0,1,0): should have NegX=Window, NegZ=Window,
        // NegY=Floor, PosY=Ceiling (single floor building).
        let corner = layout.get(&VoxelCoord::new(0, 1, 0)).unwrap();
        assert_eq!(corner.get(FaceDirection::NegX), FaceType::Window);
        assert_eq!(corner.get(FaceDirection::NegZ), FaceType::Window);
        assert_eq!(corner.get(FaceDirection::NegY), FaceType::Floor);
        assert_eq!(corner.get(FaceDirection::PosY), FaceType::Ceiling);
        // Interior-facing sides should be Open.
        assert_eq!(corner.get(FaceDirection::PosX), FaceType::Open);
        assert_eq!(corner.get(FaceDirection::PosZ), FaceType::Open);

        // Center voxel (1,1,1): all sides interior, so only Floor+Ceiling.
        let center = layout.get(&VoxelCoord::new(1, 1, 1)).unwrap();
        assert_eq!(center.get(FaceDirection::NegY), FaceType::Floor);
        assert_eq!(center.get(FaceDirection::PosY), FaceType::Ceiling);
        assert_eq!(center.get(FaceDirection::PosX), FaceType::Open);
        assert_eq!(center.get(FaceDirection::NegX), FaceType::Open);
        assert_eq!(center.get(FaceDirection::PosZ), FaceType::Open);
        assert_eq!(center.get(FaceDirection::NegZ), FaceType::Open);
    }

    #[test]
    fn has_exactly_one_door() {
        let anchor = VoxelCoord::new(0, 0, 0);
        let layout = compute_building_face_layout(anchor, 3, 3, 1);

        let door_count: usize = layout
            .values()
            .flat_map(|fd| FaceDirection::ALL.iter().map(move |&dir| fd.get(dir)))
            .filter(|&ft| ft == FaceType::Door)
            .count();

        assert_eq!(door_count, 1);

        // Door should be at center of +Z edge at ground level.
        let door_voxel = layout.get(&VoxelCoord::new(1, 1, 2)).unwrap();
        assert_eq!(door_voxel.get(FaceDirection::PosZ), FaceType::Door);
    }

    #[test]
    fn interior_faces_are_open() {
        let anchor = VoxelCoord::new(0, 0, 0);
        let layout = compute_building_face_layout(anchor, 5, 5, 1);

        // A voxel fully interior horizontally (2,1,2) should have Open on
        // all 4 horizontal sides.
        let interior = layout.get(&VoxelCoord::new(2, 1, 2)).unwrap();
        assert_eq!(interior.get(FaceDirection::PosX), FaceType::Open);
        assert_eq!(interior.get(FaceDirection::NegX), FaceType::Open);
        assert_eq!(interior.get(FaceDirection::PosZ), FaceType::Open);
        assert_eq!(interior.get(FaceDirection::NegZ), FaceType::Open);
    }

    #[test]
    fn taller_building_ceiling_only_on_top() {
        let anchor = VoxelCoord::new(0, 0, 0);
        let layout = compute_building_face_layout(anchor, 3, 3, 3);

        // 3x3x3 = 27 interior voxels (y=1,2,3).
        assert_eq!(layout.len(), 27);

        // Bottom layer (y=1): Floor on NegY, no Ceiling on PosY.
        let bottom = layout.get(&VoxelCoord::new(1, 1, 1)).unwrap();
        assert_eq!(bottom.get(FaceDirection::NegY), FaceType::Floor);
        assert_eq!(bottom.get(FaceDirection::PosY), FaceType::Open);

        // Middle layer (y=2): no Floor, no Ceiling.
        let middle = layout.get(&VoxelCoord::new(1, 2, 1)).unwrap();
        assert_eq!(middle.get(FaceDirection::NegY), FaceType::Open);
        assert_eq!(middle.get(FaceDirection::PosY), FaceType::Open);

        // Top layer (y=3): no Floor on NegY, Ceiling on PosY.
        let top = layout.get(&VoxelCoord::new(1, 3, 1)).unwrap();
        assert_eq!(top.get(FaceDirection::NegY), FaceType::Open);
        assert_eq!(top.get(FaceDirection::PosY), FaceType::Ceiling);
    }

    #[test]
    fn non_origin_anchor() {
        let anchor = VoxelCoord::new(10, 5, 20);
        let layout = compute_building_face_layout(anchor, 3, 3, 1);

        // Interior at y=6 (one above foundation at y=5).
        assert_eq!(layout.len(), 9);
        assert!(layout.contains_key(&VoxelCoord::new(10, 6, 20)));
        assert!(layout.contains_key(&VoxelCoord::new(12, 6, 22)));
        assert!(!layout.contains_key(&VoxelCoord::new(10, 5, 20))); // foundation level
    }

    // --- CompletedStructure tests ---

    #[test]
    fn completed_structure_from_blueprint() {
        use crate::blueprint::Blueprint;
        use crate::blueprint::BlueprintState;
        use crate::prng::GameRng;
        use crate::types::{BuildType, Priority, ProjectId, StructureId};

        let mut rng = GameRng::new(42);
        let project_id = ProjectId::new(&mut rng);
        let bp = Blueprint {
            id: project_id,
            build_type: BuildType::Platform,
            voxels: vec![
                VoxelCoord::new(5, 3, 10),
                VoxelCoord::new(6, 3, 10),
                VoxelCoord::new(7, 3, 10),
                VoxelCoord::new(5, 3, 11),
            ],
            priority: Priority::Normal,
            state: BlueprintState::Complete,
            task_id: None,
            face_layout: None,
            stress_warning: false,
            original_voxels: Vec::new(),
        };

        let structure = CompletedStructure::from_blueprint(StructureId(0), &bp, 5000);

        assert_eq!(structure.id, StructureId(0));
        assert_eq!(structure.project_id, project_id);
        assert_eq!(structure.build_type, BuildType::Platform);
        assert_eq!(structure.anchor, VoxelCoord::new(5, 3, 10));
        assert_eq!(structure.width, 3); // x: 5..7 inclusive
        assert_eq!(structure.depth, 2); // z: 10..11 inclusive
        assert_eq!(structure.height, 1); // y: 3..3 inclusive
        assert_eq!(structure.completed_tick, 5000);
        assert_eq!(structure.name, None);
    }

    #[test]
    fn completed_structure_serialization_roundtrip() {
        use crate::prng::GameRng;
        use crate::types::{BuildType, ProjectId, StructureId};

        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(42),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Bridge,
            anchor: VoxelCoord::new(10, 5, 20),
            width: 4,
            depth: 1,
            height: 1,
            completed_tick: 10000,
            name: None,
            furnishing: None,
            assigned_elf: None,
            furniture_positions: Vec::new(),
            planned_furniture: Vec::new(),
        };

        let json = serde_json::to_string(&structure).unwrap();
        let restored: CompletedStructure = serde_json::from_str(&json).unwrap();

        assert_eq!(structure, restored);
    }

    #[test]
    fn display_name_default_when_no_custom_name() {
        use crate::prng::GameRng;
        use crate::types::{BuildType, ProjectId, StructureId};

        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(12),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Platform,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 1,
            depth: 1,
            height: 1,
            completed_tick: 100,
            name: None,
            furnishing: None,
            assigned_elf: None,
            furniture_positions: Vec::new(),
            planned_furniture: Vec::new(),
        };
        assert_eq!(structure.display_name(), "Platform #12");
    }

    #[test]
    fn display_name_returns_custom_name() {
        use crate::prng::GameRng;
        use crate::types::{BuildType, ProjectId, StructureId};

        let mut rng = GameRng::new(42);
        let mut structure = CompletedStructure {
            id: StructureId(5),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Bridge,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 1,
            depth: 1,
            height: 1,
            completed_tick: 100,
            name: Some("Starlight Bridge".to_string()),
            furnishing: None,
            assigned_elf: None,
            furniture_positions: Vec::new(),
            planned_furniture: Vec::new(),
        };
        assert_eq!(structure.display_name(), "Starlight Bridge");

        // Clearing name reverts to default.
        structure.name = None;
        assert_eq!(structure.display_name(), "Bridge #5");
    }

    #[test]
    fn display_name_all_build_types() {
        use crate::prng::GameRng;
        use crate::types::{BuildType, ProjectId, StructureId};

        let mut rng = GameRng::new(42);
        let types_and_names = [
            (BuildType::Platform, "Platform #0"),
            (BuildType::Bridge, "Bridge #0"),
            (BuildType::Stairs, "Stairs #0"),
            (BuildType::Wall, "Wall #0"),
            (BuildType::Enclosure, "Enclosure #0"),
            (BuildType::Building, "Building #0"),
            (BuildType::WoodLadder, "Wood Ladder #0"),
            (BuildType::RopeLadder, "Rope Ladder #0"),
            (BuildType::Carve, "Carve #0"),
        ];
        for (build_type, expected) in types_and_names {
            let structure = CompletedStructure {
                id: StructureId(0),
                project_id: ProjectId::new(&mut rng),
                build_type,
                anchor: VoxelCoord::new(0, 0, 0),
                width: 1,
                depth: 1,
                height: 1,
                completed_tick: 0,
                name: None,
                furnishing: None,
                assigned_elf: None,
                furniture_positions: Vec::new(),
                planned_furniture: Vec::new(),
            };
            assert_eq!(structure.display_name(), expected);
        }
    }

    #[test]
    fn serialization_without_name_field_deserializes_as_none() {
        use crate::prng::GameRng;
        use crate::types::{BuildType, ProjectId, StructureId};

        // Serialize a structure, strip the "name" field, then deserialize.
        // This simulates loading an old save that predates the name field.
        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(0),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Platform,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 1,
            depth: 1,
            height: 1,
            completed_tick: 100,
            name: Some("Custom".to_string()),
            furnishing: None,
            assigned_elf: None,
            furniture_positions: Vec::new(),
            planned_furniture: Vec::new(),
        };
        let mut value: serde_json::Value = serde_json::to_value(&structure).unwrap();
        value.as_object_mut().unwrap().remove("name");
        let restored: CompletedStructure = serde_json::from_value(value).unwrap();
        assert_eq!(restored.name, None);
        assert_eq!(restored.display_name(), "Platform #0");
    }
}
