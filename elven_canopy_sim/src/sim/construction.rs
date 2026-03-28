// Construction system — build/carve designation, materialization, and furnishing.
//
// Handles the full lifecycle of player-designated construction: validation,
// blueprint creation, voxel materialization (one voxel per build action),
// structure completion, furnishing, and nav graph updates. Also includes
// raycasting for structure identification and home assignment.
//
// See also: `blueprint.rs` (blueprint data model), `building.rs` (building
// templates), `structural.rs` (integrity solver).
use super::*;
use crate::blueprint::{Blueprint, BlueprintState};
use crate::building;
use crate::db::ActionKind;
use crate::event::{ScheduledEventKind, SimEvent, SimEventKind};
use crate::inventory;
use crate::nav::{self};
use crate::structural;
use crate::task;
use std::collections::BTreeMap;

impl SimState {
    /// Validate and create a blueprint from a `DesignateBuild` command.
    ///
    /// **Blueprint-aware:** Uses `blueprint_overlay()` to treat designated
    /// (not yet built) blueprints as their target voxel types for overlap,
    /// adjacency, and structural checks.
    ///
    /// Validation (silent no-op on failure, consistent with other commands):
    /// - Voxels must be non-empty.
    /// - All voxels must be in-bounds.
    /// - No voxel may belong to an existing designated blueprint (F-no-bp-overlap).
    /// - All voxels must be Air (or overlap-compatible, considering overlay).
    /// - At least one voxel must have a solid face neighbor (considering overlay).
    pub(crate) fn designate_build(
        &mut self,
        build_type: BuildType,
        voxels: &[VoxelCoord],
        priority: Priority,
        events: &mut Vec<SimEvent>,
    ) {
        self.last_build_message = None;

        if voxels.is_empty() {
            self.last_build_message = Some("No voxels to build.".to_string());
            return;
        }
        for &coord in voxels {
            if !self.world.in_bounds(coord) {
                self.last_build_message = Some("Build position is out of bounds.".to_string());
                return;
            }
        }

        // Build overlay from existing designated blueprints so we treat
        // planned builds as already present for overlap, adjacency, and
        // structural checks.
        let overlay = self.blueprint_overlay();

        // F-no-bp-overlap: reject if any proposed voxel belongs to an
        // existing designated blueprint. A voxel can only belong to one
        // blueprint at a time. Exception: struts can overlap with platform
        // and bridge blueprints (they pass through flat structures).
        let strut_overlap_ok = build_type == BuildType::Strut;
        if voxels.iter().any(|v| {
            overlay.voxels.get(v).is_some_and(|&vt| {
                if strut_overlap_ok {
                    // Struts only conflict with non-flat blueprint types.
                    !matches!(vt, VoxelType::GrownPlatform)
                } else {
                    true
                }
            })
        }) {
            self.last_build_message =
                Some("Overlaps an existing blueprint designation.".to_string());
            return;
        }

        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(&self.world, coord) };

        // Branch validation: struts use custom replacement rules, overlap-enabled
        // types classify voxels, and the default requires all Air.
        let build_voxels: Vec<VoxelCoord>;
        let original_voxels: Vec<(VoxelCoord, VoxelType)>;

        if build_type == BuildType::Strut {
            // Strut-specific validation: Bresenham list check + replacement rules.
            if voxels.len() < 2 {
                self.last_build_message = Some("Strut must have at least 2 voxels.".to_string());
                return;
            }
            let endpoint_a = voxels[0];
            let endpoint_b = voxels[voxels.len() - 1];
            let recomputed = endpoint_a.line_to(endpoint_b);
            if recomputed.len() != voxels.len()
                || !recomputed.iter().zip(voxels.iter()).all(|(a, b)| a == b)
            {
                self.last_build_message =
                    Some("Strut voxel list does not match Bresenham line.".to_string());
                return;
            }

            // Replacement validation: check each voxel is a replaceable type.
            let mut ov = Vec::new();
            for &coord in voxels {
                let eff = effective_type(coord);
                match eff {
                    VoxelType::Air => {}
                    VoxelType::Leaf
                    | VoxelType::Fruit
                    | VoxelType::Dirt
                    | VoxelType::Trunk
                    | VoxelType::Branch
                    | VoxelType::Root => {
                        ov.push((coord, self.world.get(coord)));
                    }
                    VoxelType::Strut | VoxelType::GrownPlatform => {
                        // Struts can pass through platforms and existing
                        // struts. Record the original type for restoration
                        // on cancel.
                        ov.push((coord, self.world.get(coord)));
                    }
                    VoxelType::GrownWall
                    | VoxelType::BuildingInterior
                    | VoxelType::WoodLadder
                    | VoxelType::RopeLadder => {
                        self.last_build_message = Some(
                            "Strut cannot pass through buildings, walls, or ladders.".to_string(),
                        );
                        return;
                    }
                }
            }
            build_voxels = voxels.to_vec();
            original_voxels = ov;
        } else if build_type.allows_tree_overlap() {
            let mut bv = Vec::new();
            let mut ov = Vec::new();
            for &coord in voxels {
                match effective_type(coord).classify_for_overlap() {
                    OverlapClassification::Exterior => {
                        bv.push(coord);
                    }
                    OverlapClassification::Convertible => {
                        ov.push((coord, self.world.get(coord)));
                        bv.push(coord);
                    }
                    OverlapClassification::AlreadyWood => {
                        // Skip — already wood, no blueprint voxel needed.
                    }
                    OverlapClassification::Blocked => {
                        self.last_build_message = Some("Build position is not empty.".to_string());
                        return;
                    }
                }
            }
            if bv.is_empty() {
                self.last_build_message =
                    Some("Nothing to build — all voxels are already wood.".to_string());
                return;
            }
            build_voxels = bv;
            original_voxels = ov;
        } else {
            for &coord in voxels {
                if effective_type(coord) != VoxelType::Air {
                    self.last_build_message = Some("Build position is not empty.".to_string());
                    return;
                }
            }
            build_voxels = voxels.to_vec();
            original_voxels = Vec::new();
        }

        // Adjacency check: at least one build voxel (for struts, at least one
        // endpoint) must be face-adjacent to existing solid structure or overlay.
        let adjacency_voxels = if build_type == BuildType::Strut {
            // For struts, only check the two endpoints (not interior voxels).
            vec![build_voxels[0], build_voxels[build_voxels.len() - 1]]
        } else {
            build_voxels.clone()
        };
        let any_adjacent = adjacency_voxels.iter().any(|&coord| {
            self.world.has_solid_face_neighbor(coord)
                || FaceDirection::ALL.iter().any(|&dir| {
                    let (dx, dy, dz) = dir.to_offset();
                    let neighbor = VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
                    overlay
                        .voxels
                        .get(&neighbor)
                        .is_some_and(|vt| vt.is_solid())
                })
        });
        if !any_adjacent {
            self.last_build_message =
                Some("Must build adjacent to an existing structure.".to_string());
            return;
        }

        // Structural validation: fast BFS + weight-flow check (no full solver).
        let struts: Vec<_> = self.db.struts.iter_all().cloned().collect();
        let validation = structural::validate_blueprint_fast(
            &self.world,
            &self.face_data,
            &build_voxels,
            build_type.to_voxel_type(),
            &BTreeMap::new(),
            &self.config,
            &overlay,
            &struts,
        );
        if matches!(validation.tier, structural::ValidationTier::Blocked) {
            self.last_build_message = Some(validation.message);
            return;
        }
        let stress_warning = matches!(validation.tier, structural::ValidationTier::Warning);
        if stress_warning {
            self.last_build_message = Some(validation.message);
        }

        let project_id = ProjectId::new(&mut self.rng);

        // Create a Build task at the nearest nav node to the blueprint.
        if self.nav_graph.find_nearest_node(build_voxels[0]).is_none() {
            return;
        }
        let task_id = TaskId::new(&mut self.rng);
        let num_voxels = build_voxels.len() as u64;
        let task_location = build_voxels[0];

        // Insert blueprint before task — task_blueprint_ref has FK to blueprints.
        // Blueprint initially has task_id: None; updated after task insertion.
        let composition_id = Some(self.create_composition(build_voxels.len()));

        // For struts, capture endpoints before build_voxels is moved.
        let strut_endpoints = if build_type == BuildType::Strut {
            Some((build_voxels[0], build_voxels[build_voxels.len() - 1]))
        } else {
            None
        };

        let bp = Blueprint {
            id: project_id,
            build_type,
            voxels: build_voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: None,
            composition_id,
            face_layout: None,
            stress_warning,
            original_voxels,
        };
        self.db.insert_blueprint(bp).unwrap();

        // For struts, create a Strut row (blueprint must exist for FK).
        if let Some((endpoint_a, endpoint_b)) = strut_endpoints {
            self.db
                .insert_strut_auto(|id| crate::db::Strut {
                    id,
                    endpoint_a,
                    endpoint_b,
                    blueprint_id: Some(project_id),
                    structure_id: None,
                })
                .unwrap();
        }

        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            progress: 0,
            total_cost: num_voxels as i64,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: self.player_civ_id,
        };
        self.insert_task(build_task);

        // Update blueprint with the task_id now that the task exists.
        if let Some(mut bp) = self.db.blueprints.get(&project_id) {
            bp.task_id = Some(task_id);
            let _ = self.db.update_blueprint(bp);
        }
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Validate and create a blueprint for a building with paper-thin walls.
    ///
    /// **Blueprint-aware:** Uses `blueprint_overlay()` to treat designated
    /// (not yet built) blueprints as their target voxel types for foundation
    /// solidity, interior clearance, and structural checks.
    ///
    /// Validation (silent no-op on failure):
    /// - width and depth must be >= 3 (minimum building size)
    /// - height must be >= 1
    /// - All foundation voxels (anchor.y level) must be solid (considering overlay)
    /// - All interior voxels (above foundation) must be Air (considering overlay)
    /// - All interior voxels must be in-bounds
    /// - No interior voxel may belong to an existing designated blueprint (F-no-bp-overlap)
    pub(crate) fn designate_building(
        &mut self,
        anchor: VoxelCoord,
        width: i32,
        depth: i32,
        height: i32,
        priority: Priority,
        events: &mut Vec<SimEvent>,
    ) {
        self.last_build_message = None;

        if width < 3 || depth < 3 || height < 1 {
            self.last_build_message = Some("Building too small (min 3x3x1).".to_string());
            return;
        }

        let overlay = self.blueprint_overlay();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(&self.world, coord) };

        // F-no-bp-overlap: reject if any interior voxel belongs to an
        // existing designated blueprint. Checked early (before foundation/
        // interior validation) so the overlap message takes priority.
        for y in anchor.y + 1..anchor.y + 1 + height {
            for x in anchor.x..anchor.x + width {
                for z in anchor.z..anchor.z + depth {
                    let coord = VoxelCoord::new(x, y, z);
                    if overlay.voxels.contains_key(&coord) {
                        self.last_build_message =
                            Some("Overlaps an existing blueprint designation.".to_string());
                        return;
                    }
                }
            }
        }

        // Validate foundation (all must be solid, considering blueprint overlay).
        for x in anchor.x..anchor.x + width {
            for z in anchor.z..anchor.z + depth {
                let coord = VoxelCoord::new(x, anchor.y, z);
                if !self.world.in_bounds(coord) || !effective_type(coord).is_solid() {
                    self.last_build_message =
                        Some("Foundation must be on solid ground.".to_string());
                    return;
                }
            }
        }

        // Validate interior (all must be Air, considering blueprint overlay).
        for y in anchor.y + 1..anchor.y + 1 + height {
            for x in anchor.x..anchor.x + width {
                for z in anchor.z..anchor.z + depth {
                    let coord = VoxelCoord::new(x, y, z);
                    if !self.world.in_bounds(coord) || effective_type(coord) != VoxelType::Air {
                        self.last_build_message =
                            Some("Building interior must be clear.".to_string());
                        return;
                    }
                }
            }
        }

        // Compute face layout.
        let face_layout =
            crate::building::compute_building_face_layout(anchor, width, depth, height);
        let voxels: Vec<VoxelCoord> = face_layout.keys().copied().collect();

        // Structural validation: fast BFS + weight-flow check (no full solver).
        let struts: Vec<_> = self.db.struts.iter_all().cloned().collect();
        let validation = structural::validate_blueprint_fast(
            &self.world,
            &self.face_data,
            &voxels,
            VoxelType::BuildingInterior,
            &face_layout,
            &self.config,
            &overlay,
            &struts,
        );
        if matches!(validation.tier, structural::ValidationTier::Blocked) {
            self.last_build_message = Some(validation.message);
            return;
        }
        let stress_warning = matches!(validation.tier, structural::ValidationTier::Warning);
        if stress_warning {
            self.last_build_message = Some(validation.message);
        }

        let project_id = ProjectId::new(&mut self.rng);

        // Create a Build task at the nearest nav node.
        if self.nav_graph.find_nearest_node(voxels[0]).is_none() {
            return;
        }
        let task_id = TaskId::new(&mut self.rng);
        let num_voxels = voxels.len() as u64;
        let task_location = voxels[0];

        // Insert blueprint before task — task_blueprint_ref has FK to blueprints.
        let composition_id = Some(self.create_composition(voxels.len()));
        let bp = Blueprint {
            id: project_id,
            build_type: BuildType::Building,
            voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: None,
            composition_id,
            face_layout: Some(face_layout.into_iter().collect()),
            stress_warning,
            original_voxels: Vec::new(),
        };
        self.db.insert_blueprint(bp).unwrap();

        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            progress: 0,
            total_cost: num_voxels as i64,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: self.player_civ_id,
        };
        self.insert_task(build_task);

        // Update blueprint with the task_id now that the task exists.
        if let Some(mut bp) = self.db.blueprints.get(&project_id) {
            bp.task_id = Some(task_id);
            let _ = self.db.update_blueprint(bp);
        }
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Validate and create a blueprint for a ladder (wood or rope).
    ///
    /// **Blueprint-aware:** Uses `blueprint_overlay()` to treat designated
    /// (not yet built) blueprints as their target voxel types for overlap,
    /// anchoring, and structural checks.
    ///
    /// Validation:
    /// - height >= 1
    /// - orientation must be horizontal (PosX/NegX/PosZ/NegZ)
    /// - No column voxel may belong to an existing designated blueprint (F-no-bp-overlap)
    /// - All column voxels must be Air or Convertible (considering overlay)
    /// - Wood: at least one voxel's ladder face is adjacent to solid (considering overlay)
    /// - Rope: topmost voxel's ladder face is adjacent to solid (considering overlay)
    pub(crate) fn designate_ladder(
        &mut self,
        anchor: VoxelCoord,
        height: i32,
        orientation: FaceDirection,
        kind: LadderKind,
        priority: Priority,
        events: &mut Vec<SimEvent>,
    ) {
        self.last_build_message = None;

        if height < 1 {
            self.last_build_message = Some("Ladder height must be at least 1.".to_string());
            return;
        }

        // Orientation must be horizontal (ody == 0 after this guard).
        let (odx, _ody, odz) = orientation.to_offset();
        if _ody != 0 {
            self.last_build_message = Some("Ladder orientation must be horizontal.".to_string());
            return;
        }

        // Build overlay from existing designated blueprints.
        let overlay = self.blueprint_overlay();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(&self.world, coord) };

        // F-no-bp-overlap: reject if any ladder voxel belongs to an
        // existing designated blueprint.
        for dy in 0..height {
            let coord = VoxelCoord::new(anchor.x, anchor.y + dy, anchor.z);
            if overlay.voxels.contains_key(&coord) {
                self.last_build_message =
                    Some("Overlaps an existing blueprint designation.".to_string());
                return;
            }
        }

        // Classify column voxels using overlap rules (ladders allow tree overlap).
        let build_type = match kind {
            LadderKind::Wood => BuildType::WoodLadder,
            LadderKind::Rope => BuildType::RopeLadder,
        };
        let mut build_voxels = Vec::new();
        let mut original_voxels = Vec::new();
        for dy in 0..height {
            let coord = VoxelCoord::new(anchor.x, anchor.y + dy, anchor.z);
            if !self.world.in_bounds(coord) {
                self.last_build_message = Some("Ladder extends out of bounds.".to_string());
                return;
            }
            match effective_type(coord).classify_for_overlap() {
                OverlapClassification::Exterior => {
                    build_voxels.push(coord);
                }
                OverlapClassification::Convertible => {
                    original_voxels.push((coord, self.world.get(coord)));
                    build_voxels.push(coord);
                }
                OverlapClassification::AlreadyWood => {
                    // Skip — already wood, no blueprint voxel needed.
                }
                OverlapClassification::Blocked => {
                    self.last_build_message =
                        Some("Ladder position is blocked by existing construction.".to_string());
                    return;
                }
            }
        }
        if build_voxels.is_empty() {
            self.last_build_message =
                Some("Nothing to build — all voxels are already wood.".to_string());
            return;
        }

        // Anchoring validation (considers blueprint overlay).
        match kind {
            LadderKind::Wood => {
                // At least one voxel's ladder face must be adjacent to solid.
                let any_anchored = build_voxels.iter().any(|&coord| {
                    let neighbor = VoxelCoord::new(coord.x + odx, coord.y, coord.z + odz);
                    effective_type(neighbor).is_solid()
                });
                if !any_anchored {
                    self.last_build_message =
                        Some("Wood ladder must be adjacent to a solid surface.".to_string());
                    return;
                }
            }
            LadderKind::Rope => {
                // Topmost voxel's ladder face must be adjacent to solid.
                let top = VoxelCoord::new(anchor.x + odx, anchor.y + height - 1, anchor.z + odz);
                if !effective_type(top).is_solid() {
                    self.last_build_message =
                        Some("Rope ladder must hang from a solid surface at the top.".to_string());
                    return;
                }
            }
        }

        let project_id = ProjectId::new(&mut self.rng);

        // Create a Build task at the nearest nav node to the bottom of the ladder.
        if self.nav_graph.find_nearest_node(build_voxels[0]).is_none() {
            return;
        }
        let task_id = TaskId::new(&mut self.rng);
        let num_voxels = build_voxels.len() as u64;
        let task_location = build_voxels[0];

        // Store the orientation in the blueprint's face_layout field.
        let face_layout: Vec<(VoxelCoord, FaceData)> = build_voxels
            .iter()
            .map(|&coord| (coord, ladder_face_data(orientation)))
            .collect();

        // Insert blueprint before task — task_blueprint_ref has FK to blueprints.
        let composition_id = Some(self.create_composition(build_voxels.len()));
        let bp = Blueprint {
            id: project_id,
            build_type,
            voxels: build_voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: None,
            composition_id,
            face_layout: Some(face_layout.into_iter().collect()),
            stress_warning: false,
            original_voxels,
        };
        self.db.insert_blueprint(bp).unwrap();

        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            progress: 0,
            total_cost: num_voxels as i64,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: self.player_civ_id,
        };
        self.insert_task(build_task);

        // Update blueprint with the task_id now that the task exists.
        if let Some(mut bp) = self.db.blueprints.get(&project_id) {
            bp.task_id = Some(task_id);
            let _ = self.db.update_blueprint(bp);
        }
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Validate and create a blueprint for carving (removing) solid voxels.
    ///
    /// **Blueprint-aware:** Uses `blueprint_overlay()` to treat designated
    /// (not yet built) blueprints as their target voxel types for carvability
    /// checks and structural validation. A voxel that is Air in the real world
    /// but solid in the overlay (pending build) is considered carvable; a voxel
    /// that is solid but overlaid as Air (pending carve) is not.
    ///
    /// Filters the input to only carvable voxels (solid and above the bedrock
    /// layer at y=0, considering overlay). Air, bedrock, and voxels belonging
    /// to existing blueprints (F-no-bp-overlap) are silently skipped. Records
    /// original voxel types for cancel restoration.
    pub(crate) fn designate_carve(
        &mut self,
        voxels: &[VoxelCoord],
        priority: Priority,
        events: &mut Vec<SimEvent>,
    ) {
        self.last_build_message = None;

        if voxels.is_empty() {
            self.last_build_message = Some("No voxels to carve.".to_string());
            return;
        }
        for &coord in voxels {
            if !self.world.in_bounds(coord) {
                self.last_build_message = Some("Carve position is out of bounds.".to_string());
                return;
            }
        }

        let overlay = self.blueprint_overlay();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(&self.world, coord) };

        // Filter to only carvable voxels: solid, not at the bedrock layer
        // (y=0), and not already claimed by an existing blueprint
        // (F-no-bp-overlap).
        let mut carve_voxels = Vec::new();
        let mut original_voxels = Vec::new();
        for &coord in voxels {
            if overlay.voxels.contains_key(&coord) {
                continue;
            }
            let vt = effective_type(coord);
            if vt.is_solid() && coord.y > 0 {
                carve_voxels.push(coord);
                original_voxels.push((coord, self.world.get(coord)));
            }
        }

        if carve_voxels.is_empty() {
            self.last_build_message = Some("Nothing to carve.".to_string());
            return;
        }
        let struts: Vec<_> = self.db.struts.iter_all().cloned().collect();
        let validation = structural::validate_carve_fast(
            &self.world,
            &self.face_data,
            &carve_voxels,
            &self.config,
            &overlay,
            &struts,
        );
        if matches!(validation.tier, structural::ValidationTier::Blocked) {
            self.last_build_message = Some(validation.message);
            return;
        }
        let stress_warning = matches!(validation.tier, structural::ValidationTier::Warning);
        if stress_warning {
            self.last_build_message = Some(validation.message);
        }

        let project_id = ProjectId::new(&mut self.rng);

        // Create a Build task at the nearest nav node to the carve site.
        // Use the nav node's position as the task location so that
        // find_available_task's expanding-box search resolves instantly
        // instead of scanning the entire world from underground dirt.
        let nav_node = match self.nav_graph.find_nearest_node(carve_voxels[0]) {
            Some(n) => n,
            None => return,
        };
        let task_id = TaskId::new(&mut self.rng);
        let num_voxels = carve_voxels.len() as u64;
        let task_location = self.nav_graph.node(nav_node).position;

        // Insert blueprint before task — task_blueprint_ref has FK to blueprints.
        let bp = Blueprint {
            id: project_id,
            build_type: BuildType::Carve,
            voxels: carve_voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: None,
            composition_id: None,
            face_layout: None,
            stress_warning,
            original_voxels,
        };
        self.db.insert_blueprint(bp).unwrap();

        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            progress: 0,
            total_cost: num_voxels as i64,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: self.player_civ_id,
        };
        self.insert_task(build_task);

        // Update blueprint with the task_id now that the task exists.
        if let Some(mut bp) = self.db.blueprints.get(&project_id) {
            bp.task_id = Some(task_id);
            let _ = self.db.update_blueprint(bp);
        }
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Cancel a blueprint by ProjectId. Removes the associated Build task,
    /// unassigns any workers, reverts materialized voxels to Air, and rebuilds
    /// the nav graph. Emits `BuildCancelled` if found.
    /// Silent no-op if the ProjectId doesn't exist (idempotent for multiplayer).
    pub(crate) fn cancel_build(&mut self, project_id: ProjectId, events: &mut Vec<SimEvent>) {
        let bp = match self.db.blueprints.get(&project_id) {
            Some(bp) => bp,
            None => return,
        };

        // Clear FK references before removing entities. Order matters:
        // - blueprint.task_id → tasks (restrict)
        // - creature.current_task → tasks (restrict)
        // - structure.project_id → blueprints (restrict)
        // - task_blueprint_ref.project_id → blueprints (restrict)
        // All restrict FKs must be nullified/removed before the target can be deleted.

        // 1. Nullify blueprint.task_id so the blueprint no longer blocks task removal.
        if let Some(mut bp_mut) = self.db.blueprints.get(&project_id) {
            bp_mut.task_id = None;
            let _ = self.db.update_blueprint(bp_mut);
        }

        // 2. Unassign creatures from the task and remove the task.
        if let Some(task_id) = bp.task_id {
            for mut creature in self
                .db
                .creatures
                .by_current_task(&Some(task_id), tabulosity::QueryOpts::ASC)
            {
                creature.current_task = None;
                creature.path = None;
                let _ = self.db.update_creature(creature);
            }
            // Task removal cascades to task_blueprint_ref, task_structure_ref, etc.
            let _ = self.db.remove_task(&task_id);
        }

        // 3. Remove structures (structure.project_id → blueprints is restrict).
        for &coord in &bp.voxels {
            self.structure_voxels.remove(&coord);
        }
        let structure_ids_to_remove: Vec<StructureId> = self
            .db
            .structures
            .iter_all()
            .filter(|s| s.project_id == project_id)
            .map(|s| s.id)
            .collect();
        for sid in structure_ids_to_remove {
            let _ = self.db.remove_structure(&sid);
        }

        // 4. Remove blueprint (cascades to struts via blueprint_id FK).
        let _ = self.db.remove_blueprint(&project_id);

        let bp_voxels: Vec<VoxelCoord> = bp.voxels.clone();
        let original_map: BTreeMap<VoxelCoord, VoxelType> =
            bp.original_voxels.iter().copied().collect();
        let is_building = bp.build_type == BuildType::Building;
        let is_carve = bp.build_type == BuildType::Carve;
        let mut any_reverted = false;

        if is_carve {
            // Carve cancel: restore carved voxels to their original types.
            for &coord in &bp_voxels {
                if self.world.get(coord) == VoxelType::Air
                    && let Some(&original) = original_map.get(&coord)
                {
                    self.set_voxel(coord, original);
                    any_reverted = true;
                }
            }
            self.carved_voxels.retain(|c| !bp_voxels.contains(c));
        } else {
            // Build cancel: revert materialized voxels to Air (or original for
            // overlap builds with convertible Leaf/Fruit).
            for &coord in &bp_voxels {
                if self.world.get(coord) != VoxelType::Air {
                    let revert_to = original_map.get(&coord).copied().unwrap_or(VoxelType::Air);
                    self.set_voxel(coord, revert_to);
                    any_reverted = true;
                }
            }
            // Remove from placed_voxels.
            self.placed_voxels
                .retain(|(coord, _)| !bp_voxels.contains(coord));
        }

        // For buildings and ladders, also remove face_data entries.
        let is_ladder = matches!(bp.build_type, BuildType::WoodLadder | BuildType::RopeLadder);
        if is_building || is_ladder {
            for &coord in &bp_voxels {
                self.face_data.remove(&coord);
            }
            self.face_data_list
                .retain(|(coord, _)| !bp_voxels.contains(coord));
        }
        // For ladders, also remove ladder_orientations entries.
        if is_ladder {
            for &coord in &bp_voxels {
                self.ladder_orientations.remove(&coord);
            }
            self.ladder_orientations_list
                .retain(|(coord, _)| !bp_voxels.contains(coord));
        }

        // Rebuild nav graph if geometry changed.
        if any_reverted {
            self.nav_graph = nav::build_nav_graph(&self.world, &self.face_data);
            self.resnap_creature_nodes();
            // Reverted voxels may have been supporting ground piles.
            self.apply_pile_gravity();
        }

        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BuildCancelled { project_id },
        });
    }

    /// Create a task at the nearest nav node to the given position.
    pub(crate) fn create_task(
        &mut self,
        kind: task::TaskKind,
        position: VoxelCoord,
        required_species: Option<Species>,
    ) {
        if self.nav_graph.find_nearest_node(position).is_none() {
            return;
        }
        let task_id = TaskId::new(&mut self.rng);
        let new_task = task::Task {
            id: task_id,
            kind,
            state: task::TaskState::Available,
            location: position,
            progress: 0,
            total_cost: 0,
            required_species,
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: self.player_civ_id,
        };
        self.insert_task(new_task);
    }

    /// Start a Build action: set action state, mark music as started on
    /// the first action, and schedule the completion activation.
    pub(crate) fn start_build_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        project_id: ProjectId,
    ) {
        let is_carve = self
            .db
            .blueprints
            .get(&project_id)
            .is_some_and(|bp| bp.build_type.is_carve());

        let base_duration = if is_carve {
            self.config.carve_work_ticks_per_voxel
        } else {
            self.config.build_work_ticks_per_voxel
        };
        let duration = self.skill_modified_duration(
            creature_id,
            base_duration,
            crate::types::TraitKind::Charisma,
            crate::types::TraitKind::Singing,
        );

        // Mark composition as build_started on the first Build action.
        let progress = self.db.tasks.get(&task_id).map(|t| t.progress).unwrap_or(0);
        if progress == 0
            && let Some(bp) = self.db.blueprints.get(&project_id)
            && let Some(comp_id) = bp.composition_id
            && let Some(mut comp) = self.db.music_compositions.get(&comp_id)
        {
            comp.build_started = true;
            let _ = self.db.update_music_composition(comp);
        }

        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.action_kind = ActionKind::Build;
            c.next_available_tick = Some(self.tick + duration);
            let _ = self.db.update_creature(c);
        }

        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Build action: materialize one voxel (or carve),
    /// increment progress, and check for task completion. Returns true if
    /// the task was completed.
    pub(crate) fn resolve_build_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let project_id = match self.task_project_id(task_id) {
            Some(p) => p,
            None => return false,
        };

        let build_type = self.db.blueprints.get(&project_id).map(|bp| bp.build_type);

        // Drain mana. On failure, this is a wasted action (no progress).
        // try_drain_mana handles wasted_action_count and abandon threshold.
        let cost = self.mana_cost_per_action(build_type);
        if cost > 0 && !self.try_drain_mana(creature_id, cost) {
            // Wasted action — creature may have been unassigned by abandon.
            return self
                .db
                .creatures
                .get(&creature_id)
                .and_then(|c| c.current_task)
                .is_none();
        }

        let is_carve = build_type.is_some_and(|bt| bt.is_carve());

        // Materialize one voxel.
        if is_carve {
            self.materialize_next_carve_voxel(project_id);
        } else {
            self.materialize_next_build_voxel(project_id);
        }

        // Increment progress by 1 (one voxel).
        if let Some(mut t) = self.db.tasks.get(&task_id) {
            t.progress += 1;
            let _ = self.db.update_task(t);
        }

        // Skill advancement: construction is woodsinging (Singing + Channeling
        // primary, Woodcraft secondary).
        self.try_advance_skill(creature_id, crate::types::TraitKind::Singing, 1000);
        self.try_advance_skill(creature_id, crate::types::TraitKind::Channeling, 1000);
        self.try_advance_skill(creature_id, crate::types::TraitKind::Woodcraft, 500);

        // Check if the build is complete.
        let task = match self.db.tasks.get(&task_id) {
            Some(t) => t,
            None => return true,
        };
        if task.progress >= task.total_cost {
            self.complete_build(project_id, task_id);
            return true;
        }
        false
    }

    /// Compute the creature-scale (i64) mana cost for one work action of the
    /// given build type. Platform uses its specific config cost; all others
    /// (Furnish, Carve, Wall, etc.) use `default_mana_cost_per_mille`.
    ///
    /// Config costs are in per-mille of creature mp_max (20 = 2%).
    /// Conversion: `mp_max / 1000 × cost` — pure integer math, no floats.
    ///
    /// NOTE: currently hardcodes Elf's mp_max. If a future magical species
    /// has a different mp_max, this will need a creature_id parameter.
    pub(crate) fn mana_cost_per_action(&self, build_type: Option<BuildType>) -> i64 {
        let cost_per_mille = match build_type {
            Some(BuildType::Platform) => self.config.platform_mana_cost_per_mille,
            _ => self.config.default_mana_cost_per_mille,
        };
        let elf_mp_max = self.species_table[&Species::Elf].mp_max;
        elf_mp_max / 1000 * cost_per_mille as i64
    }

    /// Creature-scale mana cost for one Grow-verb crafting action.
    /// Same per-mille conversion as `mana_cost_per_action`.
    pub(crate) fn mana_cost_for_grow_action(&self) -> i64 {
        let cost_per_mille = self.config.grow_mana_cost_per_mille;
        let elf_mp_max = self.species_table[&Species::Elf].mp_max;
        elf_mp_max / 1000 * cost_per_mille as i64
    }

    /// Try to drain mana from a creature for a work action. Returns true if
    /// the creature had enough mana (action proceeds). Returns false if
    /// insufficient (wasted action — creature still spends time but no progress).
    ///
    /// On wasted action: increments `wasted_action_count`. If the count reaches
    /// `mana_abandon_threshold`, the creature abandons the task (interrupt_task).
    /// On success: resets `wasted_action_count` to 0.
    pub(crate) fn try_drain_mana(&mut self, creature_id: CreatureId, cost: i64) -> bool {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return false,
        };

        if creature.mp_max == 0 {
            // Nonmagical creature — no mana cost applies (shouldn't be here,
            // but handle gracefully).
            return true;
        }

        if creature.mp >= cost {
            // Enough mana: drain and reset wasted counter.
            let mut creature = creature;
            creature.mp -= cost;
            creature.wasted_action_count = 0;
            let _ = self.db.update_creature(creature);
            true
        } else {
            // Insufficient mana: wasted action. Record position for VFX.
            self.mana_wasted_positions.push(creature.position);
            let threshold = self.config.mana_abandon_threshold;
            let mut creature = creature;
            creature.wasted_action_count += 1;
            let count = creature.wasted_action_count;
            let _ = self.db.update_creature(creature);
            // Check if we've hit the abandon threshold.
            if count >= threshold {
                // Abandon: interrupt_task reverts it to Available.
                if let Some(task_id) = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| c.current_task)
                {
                    self.interrupt_task(creature_id, task_id);
                }
            }
            false
        }
    }

    /// Pick the next blueprint voxel to materialize and place it.
    ///
    /// Selection criteria:
    /// 1. Must not already be the target type (not yet placed).
    /// 2. Must have at least one face-adjacent solid neighbor (adjacency
    ///    invariant — connects to existing geometry).
    /// 3. Prefer voxels NOT occupied by any creature.
    /// 4. If all eligible are occupied, pick randomly using the sim PRNG.
    pub(crate) fn materialize_next_build_voxel(&mut self, project_id: ProjectId) {
        let bp = match self.db.blueprints.get(&project_id) {
            Some(bp) => bp,
            None => return,
        };
        let build_type = bp.build_type;
        let voxel_type = build_type.to_voxel_type();
        let is_building = build_type == BuildType::Building;
        let is_ladder = matches!(build_type, BuildType::WoodLadder | BuildType::RopeLadder);
        let is_strut = build_type == BuildType::Strut;
        let allows_overlap = build_type.allows_tree_overlap();

        // Find unplaced voxels that are adjacent to existing geometry.
        // For buildings, adjacency accepts BuildingInterior face neighbors in
        // addition to solid neighbors (building interior voxels grow from the
        // foundation and from each other).
        // For ladders, adjacency accepts same-type ladder face neighbors
        // (ladder voxels grow from bottom to top or from an anchored voxel).
        // For overlap-enabled types, a voxel is "unplaced" if it hasn't been
        // converted to the target type yet (it may be Air, Leaf, or Fruit).
        // For struts, voxels may be Air or any replaceable natural type
        // (Trunk, Dirt, etc.) — struts replace natural materials during
        // construction.
        let eligible: Vec<VoxelCoord> = bp
            .voxels
            .iter()
            .copied()
            .filter(|&coord| {
                let current = self.world.get(coord);
                if allows_overlap {
                    // Already materialized to target type → skip.
                    if current == voxel_type {
                        return false;
                    }
                    // Must be Air or Convertible (Leaf/Fruit).
                    if current != VoxelType::Air
                        && !matches!(
                            current.classify_for_overlap(),
                            OverlapClassification::Convertible
                        )
                    {
                        return false;
                    }
                } else if is_strut {
                    // Struts replace natural materials. Already Strut → skip.
                    if current == VoxelType::Strut {
                        return false;
                    }
                } else if current != VoxelType::Air {
                    return false;
                }
                if self.world.has_solid_face_neighbor(coord) {
                    return true;
                }
                // For buildings, also accept BuildingInterior face neighbors.
                if is_building {
                    return self
                        .world
                        .has_face_neighbor_of_type(coord, VoxelType::BuildingInterior);
                }
                // For ladders, also accept same-type ladder face neighbors.
                if is_ladder {
                    return self.world.has_face_neighbor_of_type(coord, voxel_type);
                }
                false
            })
            .collect();

        if eligible.is_empty() {
            return;
        }

        // Collect creature positions for occupancy check.
        let creature_positions: Vec<VoxelCoord> = self
            .db
            .creatures
            .iter_all()
            .filter(|c| c.vital_status == VitalStatus::Alive)
            .map(|c| c.position)
            .collect();

        // Prefer unoccupied voxels.
        let unoccupied: Vec<VoxelCoord> = eligible
            .iter()
            .copied()
            .filter(|coord| !creature_positions.contains(coord))
            .collect();

        let chosen = if !unoccupied.is_empty() {
            let idx = self.rng.range_u64(0, unoccupied.len() as u64) as usize;
            unoccupied[idx]
        } else {
            let idx = self.rng.range_u64(0, eligible.len() as u64) as usize;
            eligible[idx]
        };

        // Place the voxel.
        self.set_voxel(chosen, voxel_type);
        self.placed_voxels.push((chosen, voxel_type));

        // For buildings and ladders, copy face data from the blueprint into sim state.
        if is_building || is_ladder {
            if let Some(bp) = self.db.blueprints.get(&project_id)
                && let Some(layout) = bp.face_layout_map()
                && let Some(fd) = layout.get(&chosen)
            {
                self.face_data.insert(chosen, fd.clone());
                self.face_data_list.push((chosen, fd.clone()));
            }
            // For ladders, also store the orientation.
            if is_ladder
                && let Some(bp) = self.db.blueprints.get(&project_id)
                && let Some(layout) = bp.face_layout_map()
                && let Some(fd) = layout.get(&chosen)
            {
                // Derive orientation: the horizontal Wall face whose opposite is Open.
                for dir in [
                    FaceDirection::PosX,
                    FaceDirection::NegX,
                    FaceDirection::PosZ,
                    FaceDirection::NegZ,
                ] {
                    if fd.get(dir) == FaceType::Wall && fd.get(dir.opposite()) == FaceType::Open {
                        self.ladder_orientations.insert(chosen, dir);
                        self.ladder_orientations_list.push((chosen, dir));
                        break;
                    }
                }
            }
            let removed = self.nav_graph.update_after_building_voxel_set(
                &self.world,
                &self.face_data,
                chosen,
            );
            let large_removed = nav::update_large_after_voxel_solidified(
                &mut self.large_nav_graph,
                &self.world,
                chosen,
            );
            let mut all_removed = removed;
            all_removed.extend(large_removed);
            self.resnap_removed_nodes(&all_removed);
        } else {
            // Incrementally update nav graph (touches only ~7 affected positions
            // instead of scanning the entire world) and resnap displaced creatures.
            let removed =
                self.nav_graph
                    .update_after_voxel_solidified(&self.world, &self.face_data, chosen);
            let large_removed = nav::update_large_after_voxel_solidified(
                &mut self.large_nav_graph,
                &self.world,
                chosen,
            );
            let mut all_removed = removed;
            all_removed.extend(large_removed);
            self.resnap_removed_nodes(&all_removed);
        }
    }

    /// Pick the next blueprint voxel to carve (set to Air).
    ///
    /// Selection: find voxels that are still solid, pick one randomly using
    /// the sim PRNG (no adjacency constraint for carving).
    pub(crate) fn materialize_next_carve_voxel(&mut self, project_id: ProjectId) {
        let bp = match self.db.blueprints.get(&project_id) {
            Some(bp) => bp,
            None => return,
        };

        // Find blueprint voxels that are still solid (not yet carved).
        let still_solid: Vec<VoxelCoord> = bp
            .voxels
            .iter()
            .copied()
            .filter(|&coord| self.world.get(coord).is_solid())
            .collect();

        if still_solid.is_empty() {
            return;
        }

        let idx = self.rng.range_u64(0, still_solid.len() as u64) as usize;
        let chosen = still_solid[idx];

        // Set to Air.
        self.set_voxel(chosen, VoxelType::Air);
        self.carved_voxels.push(chosen);

        // Nav update: the algorithm is state-based and works for both
        // solidifying and clearing voxels.
        let removed =
            self.nav_graph
                .update_after_voxel_solidified(&self.world, &self.face_data, chosen);
        let large_removed = nav::update_large_after_voxel_solidified(
            &mut self.large_nav_graph,
            &self.world,
            chosen,
        );
        let mut all_removed = removed;
        all_removed.extend(large_removed);
        self.resnap_removed_nodes(&all_removed);

        // A carved voxel may have been supporting a ground pile above it.
        self.apply_pile_gravity();
    }

    /// Mark a blueprint as Complete, register the completed structure, and
    /// complete its associated task.
    pub(crate) fn complete_build(&mut self, project_id: ProjectId, task_id: TaskId) {
        if let Some(mut bp) = self.db.blueprints.get(&project_id) {
            bp.state = BlueprintState::Complete;
            let _ = self.db.update_blueprint(bp);
        }

        // Register a CompletedStructure if the blueprint exists.
        if let Some(bp) = self.db.blueprints.get(&project_id) {
            let structure_id = StructureId(self.next_structure_id);
            self.next_structure_id += 1;
            // Populate structure_voxels ownership map.
            for &coord in &bp.voxels {
                self.structure_voxels.insert(coord, structure_id);
            }
            let inv_id = self.create_inventory(crate::db::InventoryOwnerKind::Structure);
            let structure =
                crate::db::CompletedStructure::from_blueprint(structure_id, &bp, self.tick, inv_id);
            self.db.insert_structure(structure).unwrap();

            // For struts: update the Strut row with the completed structure_id
            // and clear the blueprint_id (build is done).
            if bp.build_type == BuildType::Strut {
                let strut_to_update: Option<crate::db::Strut> = self
                    .db
                    .struts
                    .iter_all()
                    .find(|s| s.blueprint_id == Some(project_id))
                    .cloned();
                if let Some(mut strut) = strut_to_update {
                    strut.blueprint_id = None;
                    strut.structure_id = Some(structure_id);
                    let _ = self.db.update_strut(strut);
                }
            }
        }

        self.complete_task(task_id);
    }

    /// Start furnishing a completed building. Validates the structure is a
    /// building with no existing furnishing, computes furniture positions,
    /// sets the furnishing type, auto-renames if no custom name, and creates
    /// a Furnish task for an elf to work on.
    pub(crate) fn furnish_structure(
        &mut self,
        structure_id: StructureId,
        furnishing_type: FurnishingType,
        greenhouse_species: Option<FruitSpeciesId>,
    ) {
        // Validate: structure exists, is a Building, and has no furnishing yet.
        let structure = match self.db.structures.get(&structure_id) {
            Some(s) => s,
            None => return,
        };
        if structure.build_type != BuildType::Building {
            return;
        }
        if structure.furnishing.is_some() {
            return;
        }

        // DanceHall: no furniture, no task — just set the furnishing and return.
        if furnishing_type == FurnishingType::DanceHall {
            let mut structure = self.db.structures.get(&structure_id).unwrap();
            structure.furnishing = Some(FurnishingType::DanceHall);
            let _ = self.db.update_structure(structure);
            return;
        }

        // Greenhouse-specific validation: species must exist and be cultivable.
        if furnishing_type == FurnishingType::Greenhouse {
            let species_id = match greenhouse_species {
                Some(id) => id,
                None => return, // Greenhouse requires a species.
            };
            let species = match self.db.fruit_species.get(&species_id) {
                Some(s) => s,
                None => return, // Species must exist.
            };
            if !species.greenhouse_cultivable {
                return; // Species must be cultivable.
            }
        }

        // Compute furniture positions based on furnishing type.
        let planned_furniture =
            structure.compute_furniture_positions(furnishing_type, &mut self.rng);
        if planned_furniture.is_empty() {
            return;
        }
        let planned_count = planned_furniture.len();

        // Insert planned furniture rows.
        for coord in &planned_furniture {
            let _ = self.db.insert_furniture_auto(|id| crate::db::Furniture {
                id,
                structure_id,
                coord: *coord,
                placed: false,
            });
        }

        // Set furnishing type on the structure.
        let mut structure = self.db.structures.get(&structure_id).unwrap();
        structure.furnishing = Some(furnishing_type);

        // Set default logistics and cooking config based on furnishing type.
        let inv_id = structure.inventory_id;
        let default_wants = match furnishing_type {
            FurnishingType::Storehouse => {
                structure.logistics_priority = Some(self.config.storehouse_default_priority);
                vec![
                    building::LogisticsWant {
                        item_kind: inventory::ItemKind::Fruit,
                        material_filter: inventory::MaterialFilter::Any,
                        target_quantity: self.config.storehouse_default_fruit_want,
                    },
                    building::LogisticsWant {
                        item_kind: inventory::ItemKind::Bread,
                        material_filter: inventory::MaterialFilter::Any,
                        target_quantity: self.config.storehouse_default_bread_want,
                    },
                ]
            }
            FurnishingType::Kitchen => {
                structure.logistics_priority = Some(self.config.kitchen_default_priority);
                structure.crafting_enabled = true;
                // No explicit wants — auto-logistics handles fruit input.
                Vec::new()
            }
            FurnishingType::Workshop => {
                structure.logistics_priority = Some(self.config.workshop_default_priority);
                structure.crafting_enabled = true;
                // No explicit wants — auto-logistics handles recipe inputs.
                Vec::new()
            }
            FurnishingType::DiningHall => {
                structure.logistics_priority = Some(self.config.dining_hall_default_priority);
                Vec::new()
            }
            FurnishingType::Greenhouse => {
                structure.logistics_priority = Some(self.config.greenhouse_default_priority);
                structure.greenhouse_species = greenhouse_species;
                structure.greenhouse_enabled = true;
                structure.greenhouse_last_production_tick = self.tick;
                Vec::new()
            }
            _ => Vec::new(),
        };

        // Find a nav node inside the building to use as the task location.
        let interior_pos = structure.floor_interior_positions();
        let task_pos = interior_pos.first().copied().unwrap_or(structure.anchor);
        let _ = self.db.update_structure(structure);
        self.set_inv_wants(inv_id, &default_wants);

        if self.nav_graph.find_nearest_node(task_pos).is_none() {
            return;
        }

        // Create the Furnish task. total_cost = number of furniture items.
        let total_cost = planned_count as i64;
        let task_id = TaskId::new(&mut self.rng);
        let new_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Furnish { structure_id },
            state: task::TaskState::Available,
            location: task_pos,
            progress: 0,
            total_cost,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: self.player_civ_id,
        };
        self.insert_task(new_task);
    }

    /// Assign a creature to a home structure, or unassign if `structure_id`
    /// is `None`. Validates: creature is an Elf, target is a Home-furnished
    /// building. Evicts a previous occupant if the target already has one.
    pub(crate) fn assign_home(
        &mut self,
        creature_id: CreatureId,
        structure_id: Option<StructureId>,
    ) {
        // Validate creature exists and is an Elf.
        match self.db.creatures.get(&creature_id) {
            Some(c) if c.species == Species::Elf => {}
            _ => return,
        };

        // Nothing to clear on old home — creature.assigned_home is the
        // single source of truth for home assignment.

        let target_id = match structure_id {
            Some(id) => id,
            None => {
                // Unassign only.
                if let Some(mut c) = self.db.creatures.get(&creature_id) {
                    c.assigned_home = None;
                    let _ = self.db.update_creature(c);
                }
                return;
            }
        };

        // Validate target structure exists and is a Home.
        match self.db.structures.get(&target_id) {
            Some(s) if s.furnishing == Some(FurnishingType::Home) => {}
            _ => return,
        };

        // Evict previous occupant if there is one.
        let prev_occupants = self
            .db
            .creatures
            .by_assigned_home(&Some(target_id), tabulosity::QueryOpts::ASC);
        for prev_elf in prev_occupants {
            if prev_elf.id != creature_id {
                let mut prev = prev_elf;
                prev.assigned_home = None;
                let _ = self.db.update_creature(prev);
            }
        }

        // Set creature's assigned_home.
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.assigned_home = Some(target_id);
            let _ = self.db.update_creature(c);
        }
    }

    /// Start a Furnish action: set action kind and schedule next activation.
    /// Base duration is `furnish_work_ticks_per_item`, modified by DEX+Woodcraft.
    pub(crate) fn start_furnish_action(&mut self, creature_id: CreatureId) {
        let duration = self.skill_modified_duration(
            creature_id,
            self.config.furnish_work_ticks_per_item,
            crate::types::TraitKind::Dexterity,
            crate::types::TraitKind::Woodcraft,
        );
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.action_kind = ActionKind::Furnish;
            c.next_available_tick = Some(self.tick + duration);
            let _ = self.db.update_creature(c);
        }
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Furnish action: place one furniture item, increment
    /// progress, check for completion. Returns true if task completed.
    pub(crate) fn resolve_furnish_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };

        // Drain mana (furnishing uses default_mana_cost_per_mille).
        let cost = self.mana_cost_per_action(None);
        if cost > 0 && !self.try_drain_mana(creature_id, cost) {
            return self
                .db
                .creatures
                .get(&creature_id)
                .and_then(|c| c.current_task)
                .is_none();
        }

        let structure_id =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::FurnishTarget) {
                Some(s) => s,
                None => return false,
            };

        // Place the next unplaced furniture item.
        if let Some(furn) = self
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|f| !f.placed)
        {
            let mut furn = furn;
            furn.placed = true;
            let _ = self.db.update_furniture(furn);
        }

        // Increment progress by 1 (one item).
        if let Some(mut t) = self.db.tasks.get(&task_id) {
            t.progress += 1;
            let _ = self.db.update_task(t);
        }

        // Check if furnishing is complete.
        let task = match self.db.tasks.get(&task_id) {
            Some(t) => t,
            None => return true,
        };
        if task.progress >= task.total_cost {
            self.complete_task(task_id);
            return true;
        }
        false
    }

    /// After a nav graph rebuild, re-resolve every creature's position
    /// by finding the nearest node to its current position. Clears stored paths
    /// since NavNodeIds change when the graph is rebuilt.
    pub(crate) fn resnap_creature_nodes(&mut self) {
        let creature_info: Vec<(CreatureId, Species, VoxelCoord)> = self
            .db
            .creatures
            .iter_all()
            .filter(|c| c.vital_status == VitalStatus::Alive)
            .map(|c| (c.id, c.species, c.position))
            .collect();
        for (cid, species, old_pos) in creature_info {
            let graph = self.graph_for_species(species);
            let new_node = graph.find_nearest_node(old_pos);
            let new_pos = new_node.map(|nid| graph.node(nid).position);
            if let Some(mut creature) = self.db.creatures.get(&cid) {
                creature.path = None;
                if let Some(p) = new_pos {
                    creature.position = p;
                }
                let _ = self.db.update_creature(creature);
            }
            if let Some(p) = new_pos {
                self.update_creature_spatial_index(cid, species, old_pos, p);
            }
        }
    }

    /// Resnap only creatures whose position's nav node was among the removed IDs.
    /// Used after incremental nav graph updates where most creatures are
    /// unaffected — much cheaper than resnapping all creatures.
    pub(crate) fn resnap_removed_nodes(&mut self, removed: &[NavNodeId]) {
        if removed.is_empty() {
            return;
        }
        // Collect candidate creatures first, then filter by nav node membership.
        // We can't call graph_for_species inside the iter_all closure because
        // it borrows self, so we collect first and filter after.
        let candidates: Vec<(CreatureId, Species, VoxelCoord)> = self
            .db
            .creatures
            .iter_all()
            .filter(|c| c.vital_status == VitalStatus::Alive)
            .map(|c| (c.id, c.species, c.position))
            .collect();
        let to_resnap: Vec<(CreatureId, Species, VoxelCoord)> = candidates
            .into_iter()
            .filter(|&(_, species, pos)| {
                self.graph_for_species(species)
                    .node_at(pos)
                    .is_none_or(|nid| removed.contains(&nid))
            })
            .collect();
        for (cid, species, old_pos) in to_resnap {
            let graph = self.graph_for_species(species);
            let new_node = graph.find_nearest_node(old_pos);
            let new_pos = new_node.map(|nid| graph.node(nid).position);
            if let Some(mut creature) = self.db.creatures.get(&cid) {
                creature.path = None;
                if let Some(p) = new_pos {
                    creature.position = p;
                }
                let _ = self.db.update_creature(creature);
            }
            if let Some(p) = new_pos {
                self.update_creature_spatial_index(cid, species, old_pos, p);
            }
        }
    }

    /// DDA voxel raycast returning `(StructureId, VoxelCoord)` of the first
    /// structure voxel hit. Like `raycast_structure()` but also returns the
    /// hit coordinate, needed by the roof-click-select feature to decide
    /// whether the hit voxel is a building roof.
    ///
    /// When `skip_roofs` is true, roof voxels (building ceilings) are ignored
    /// and the ray continues through them. This supports the roof-hide feature
    /// where hidden roofs should not intercept clicks.
    ///
    /// When `y_cutoff` is `Some(y)`, solid voxels at or above `y` are treated
    /// as air for ray traversal (the ray passes through them). This supports
    /// the height-cutoff view mode where hidden upper voxels should not block
    /// clicks on visible surfaces below.
    pub fn raycast_structure_with_hit(
        &self,
        from: [f32; 3],
        dir: [f32; 3],
        max_steps: u32,
        skip_roofs: bool,
        y_cutoff: Option<i32>,
    ) -> Option<(StructureId, VoxelCoord)> {
        let mut voxel = [
            from[0].floor() as i32,
            from[1].floor() as i32,
            from[2].floor() as i32,
        ];

        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];

        for axis in 0..3 {
            if dir[axis] > 0.0 {
                step[axis] = 1;
                t_delta[axis] = 1.0 / dir[axis];
                t_max[axis] = ((voxel[axis] as f32 + 1.0) - from[axis]) / dir[axis];
            } else if dir[axis] < 0.0 {
                step[axis] = -1;
                t_delta[axis] = 1.0 / (-dir[axis]);
                t_max[axis] = (from[axis] - voxel[axis] as f32) / (-dir[axis]);
            }
        }

        for _ in 0..max_steps {
            let coord = VoxelCoord::new(voxel[0], voxel[1], voxel[2]);

            // When height cutoff is active, voxels at or above the cutoff
            // are invisible — skip both structure hits and solid occlusion.
            let above_cutoff = y_cutoff.is_some_and(|cutoff| coord.y >= cutoff);

            if !above_cutoff {
                if let Some(&sid) = self.structure_voxels.get(&coord) {
                    // Skip roof voxels when roofs are hidden.
                    let is_roof = skip_roofs
                        && self
                            .db
                            .structures
                            .get(&sid)
                            .is_some_and(|s| s.is_roof_voxel(coord));
                    if !is_roof {
                        return Some((sid, coord));
                    }
                }

                let vt = self.world.get(coord);
                if vt.is_solid() {
                    return None;
                }
            }

            let min_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };

            voxel[min_axis] += step[min_axis];
            t_max[min_axis] += t_delta[min_axis];
        }

        None
    }

    /// DDA voxel raycast that returns the `StructureId` of the first structure
    /// voxel hit along the ray. Uses the same Amanatides & Woo algorithm as
    /// `VoxelWorld::raycast_hits_solid`, but checks `structure_voxels` at each
    /// step:
    /// - If the voxel is in `structure_voxels`, return that `StructureId`.
    /// - If the voxel is solid but NOT a structure voxel (e.g., tree trunk),
    ///   stop (return `None` — geometry occludes).
    /// - If the voxel is air (and not a structure voxel), continue.
    ///
    /// This correctly handles non-solid structure types (ladders, building
    /// interiors) since they're in `structure_voxels` even though
    /// `is_solid()` returns false.
    ///
    /// When `y_cutoff` is `Some(y)`, voxels at or above `y` are treated as
    /// air for ray traversal.
    pub fn raycast_structure(
        &self,
        from: [f32; 3],
        dir: [f32; 3],
        max_steps: u32,
        y_cutoff: Option<i32>,
    ) -> Option<StructureId> {
        let mut voxel = [
            from[0].floor() as i32,
            from[1].floor() as i32,
            from[2].floor() as i32,
        ];

        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];

        for axis in 0..3 {
            if dir[axis] > 0.0 {
                step[axis] = 1;
                t_delta[axis] = 1.0 / dir[axis];
                t_max[axis] = ((voxel[axis] as f32 + 1.0) - from[axis]) / dir[axis];
            } else if dir[axis] < 0.0 {
                step[axis] = -1;
                t_delta[axis] = 1.0 / (-dir[axis]);
                t_max[axis] = (from[axis] - voxel[axis] as f32) / (-dir[axis]);
            }
        }

        for _ in 0..max_steps {
            let coord = VoxelCoord::new(voxel[0], voxel[1], voxel[2]);

            let above_cutoff = y_cutoff.is_some_and(|cutoff| coord.y >= cutoff);

            if !above_cutoff {
                // Check structure ownership first.
                if let Some(&sid) = self.structure_voxels.get(&coord) {
                    return Some(sid);
                }

                // Non-structure solid voxels occlude — stop.
                let vt = self.world.get(coord);
                if vt.is_solid() {
                    return None;
                }
            }

            // Advance along the axis with the smallest t_max.
            let min_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };

            voxel[min_axis] += step[min_axis];
            t_max[min_axis] += t_delta[min_axis];
        }

        None
    }

    /// DDA voxel raycast that returns the first solid voxel hit and the face
    /// the ray entered through. Uses the same Amanatides & Woo algorithm as
    /// `raycast_structure()`, but tracks `last_axis` (the axis most recently
    /// stepped) to compute the entry face on hit.
    ///
    /// If `overlay` is `Some`, designated (not yet built) blueprints are
    /// treated as their target voxel types — a designated platform reads as
    /// solid and can be "hit" by the ray. Pass `None` to raycast against the
    /// actual world only.
    ///
    /// Face encoding matches `FaceDirection` ordinals:
    ///   0=PosX, 1=NegX, 2=PosY, 3=NegY, 4=PosZ, 5=NegZ
    /// The face returned is the face of the solid voxel that the ray entered
    /// through. A ray stepping -Y (downward) enters through the PosY face
    /// (2); a ray stepping +X enters through the NegX face (1); etc.
    ///
    /// When `y_cutoff` is `Some(y)`, solid voxels at or above `y` are treated
    /// as air for ray traversal.
    pub fn raycast_solid(
        &self,
        from: [f32; 3],
        dir: [f32; 3],
        max_steps: u32,
        overlay: Option<&structural::BlueprintOverlay>,
        y_cutoff: Option<i32>,
    ) -> Option<(VoxelCoord, u8)> {
        let mut voxel = [
            from[0].floor() as i32,
            from[1].floor() as i32,
            from[2].floor() as i32,
        ];

        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];

        for axis in 0..3 {
            if dir[axis] > 0.0 {
                step[axis] = 1;
                t_delta[axis] = 1.0 / dir[axis];
                t_max[axis] = ((voxel[axis] as f32 + 1.0) - from[axis]) / dir[axis];
            } else if dir[axis] < 0.0 {
                step[axis] = -1;
                t_delta[axis] = 1.0 / (-dir[axis]);
                t_max[axis] = (from[axis] - voxel[axis] as f32) / (-dir[axis]);
            }
        }

        // Track which axis was last stepped to compute the entry face.
        let mut last_axis: usize = 0;
        let mut first_step = true;

        for _ in 0..max_steps {
            let coord = VoxelCoord::new(voxel[0], voxel[1], voxel[2]);

            let above_cutoff = y_cutoff.is_some_and(|cutoff| coord.y >= cutoff);
            let vt = match overlay {
                Some(ov) => ov.effective_type(&self.world, coord),
                None => self.world.get(coord),
            };
            if !first_step && !above_cutoff && vt.is_solid() {
                // Compute the face: the ray entered through the face opposite
                // to the step direction on last_axis.
                let face = match (last_axis, step[last_axis] > 0) {
                    (0, true) => 1,  // stepped +X → entered through NegX face
                    (0, false) => 0, // stepped -X → entered through PosX face
                    (1, true) => 3,  // stepped +Y → entered through NegY face
                    (1, false) => 2, // stepped -Y → entered through PosY face
                    (2, true) => 5,  // stepped +Z → entered through NegZ face
                    (2, false) => 4, // stepped -Z → entered through PosZ face
                    _ => unreachable!(),
                };
                return Some((coord, face));
            }

            first_step = false;

            // Advance along the axis with the smallest t_max.
            last_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };

            voxel[last_axis] += step[last_axis];
            t_max[last_axis] += t_delta[last_axis];
        }

        None
    }

    /// For a ladder column at `(x, y..y+height, z)`, count how many voxels
    /// in the column have a solid neighbor in each of the 4 cardinal
    /// directions. Return the orientation (as FaceDirection ordinal) with
    /// the highest count. Tie-break: first in iteration order (East,
    /// South, West, North).
    pub fn auto_ladder_orientation(&self, x: i32, y: i32, z: i32, height: i32) -> u8 {
        // Cardinal directions: East(+X)=0, South(+Z)=4, West(-X)=1, North(-Z)=5
        let orientations: [(i32, i32, u8); 4] = [
            (1, 0, 0),  // East (+X) → face PosX
            (0, 1, 4),  // South (+Z) → face PosZ
            (-1, 0, 1), // West (-X) → face NegX
            (0, -1, 5), // North (-Z) → face NegZ
        ];

        let mut best_face: u8 = 0;
        let mut best_count: i32 = -1;

        for &(dx, dz, face) in &orientations {
            let mut count = 0i32;
            for dy in 0..height {
                let neighbor = VoxelCoord::new(x + dx, y + dy, z + dz);
                if self.world.get(neighbor).is_solid() {
                    count += 1;
                }
            }
            if count > best_count {
                best_count = count;
                best_face = face;
            }
        }

        best_face
    }
}
