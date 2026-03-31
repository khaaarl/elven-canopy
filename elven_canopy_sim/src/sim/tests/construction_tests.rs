//! Tests for the construction system: platform designation/building, buildings,
//! carving, ladders, struts, blueprints (overlay, overlap rejection),
//! furnishing, structure voxels, and raycasting.
//! Corresponds to `sim/construction.rs`.

use super::*;

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Find a Leaf voxel that is face-adjacent to a Trunk, Branch, or Root
/// voxel (not just any solid -- must be adjacent to structural wood so the
/// structural validator can reach the ground).
fn find_leaf_adjacent_to_wood(sim: &SimState) -> VoxelCoord {
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    for &leaf_coord in &tree.leaf_voxels {
        for &(dx, dy, dz) in &[
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            let neighbor = VoxelCoord::new(leaf_coord.x + dx, leaf_coord.y + dy, leaf_coord.z + dz);
            let vt = sim.world.get(neighbor);
            if matches!(vt, VoxelType::Trunk | VoxelType::Branch | VoxelType::Root) {
                return leaf_coord;
            }
        }
    }
    panic!("No leaf voxel adjacent to wood found");
}

/// Helper: designate a single-voxel platform and run the sim until the
/// build task is complete. Returns the sim after completion.
fn designate_and_complete_build(mut sim: SimState) -> SimState {
    // Disable food/rest so the elf focuses on building.
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Spawn an elf near the build site.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: air_coord,
        },
    };
    sim.step(&[cmd], 1);

    // Designate a 1-voxel platform.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 2);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Run the sim forward until the blueprint is Complete.
    // The elf will claim the task, walk to the site, and do build work.
    // build_work_ticks_per_voxel * 1 voxel = total_cost ticks of work.
    // Cap at 1 million ticks to avoid infinite loops in tests.
    let max_tick = sim.tick + 1_000_000;
    while sim.tick < max_tick {
        sim.step(&[], sim.tick + 100);
        let all_complete = sim
            .db
            .blueprints
            .iter_all()
            .all(|bp| bp.state == BlueprintState::Complete);
        if all_complete {
            break;
        }
    }
    assert!(
        sim.db
            .blueprints
            .iter_all()
            .all(|bp| bp.state == BlueprintState::Complete),
        "Build did not complete within tick limit"
    );
    sim
}

/// Helper: designate and complete a 3x3x1 building, returning the sim with
/// a completed building structure whose interior voxels are registered in
/// `structure_voxels`.
fn designate_and_complete_building(mut sim: SimState) -> SimState {
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
    let anchor = find_building_site(&sim);

    // Spawn an elf near the build site.
    let air_above = VoxelCoord::new(anchor.x, anchor.y + 1, anchor.z);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: air_above,
        },
    };
    sim.step(&[cmd], 1);

    // Designate a 3x3x1 building.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuilding {
            anchor,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 2);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Run until complete.
    let max_tick = sim.tick + 1_000_000;
    while sim.tick < max_tick {
        sim.step(&[], sim.tick + 100);
        if sim
            .db
            .blueprints
            .iter_all()
            .all(|bp| bp.state == BlueprintState::Complete)
        {
            break;
        }
    }
    assert!(
        sim.db
            .blueprints
            .iter_all()
            .all(|bp| bp.state == BlueprintState::Complete),
        "Building did not complete within tick limit"
    );
    sim
}

/// Helper: find a solid voxel that is safe to carve (won't disconnect the
/// structure). Picks the highest trunk voxel so removing it doesn't sever
/// the tree's connection to ground.
fn find_carvable_voxel(sim: &SimState) -> VoxelCoord {
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    // Pick the lowest trunk voxel above the floor so the elf can reach it
    // quickly from ground level (the highest voxel may be unreachable
    // within the test's tick budget).
    tree.trunk_voxels
        .iter()
        .copied()
        .filter(|v| v.y > sim.config.floor_y + 1)
        .min_by_key(|v| v.y)
        .expect("No trunk voxel above floor")
}

/// Find an air voxel adjacent to a trunk voxel on a horizontal face,
/// ensuring at least `height` air voxels above it. Returns (anchor, orientation)
/// where orientation is the face of the anchor pointing toward the trunk.
fn find_ladder_column(sim: &SimState, height: i32) -> (VoxelCoord, FaceDirection) {
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    for &trunk_coord in &tree.trunk_voxels {
        for &(dx, dz) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let base = VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y, trunk_coord.z + dz);
            if !sim.world.in_bounds(base) {
                continue;
            }
            // The orientation points from air toward trunk (i.e., face
            // direction on the ladder voxel that faces the wall).
            let orientation = FaceDirection::from_offset(-dx, 0, -dz).unwrap();
            let all_air = (0..height).all(|dy| {
                let coord = VoxelCoord::new(base.x, base.y + dy, base.z);
                sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air
            });
            if all_air {
                return (base, orientation);
            }
        }
    }
    panic!("No suitable ladder column found");
}

/// Find two air voxels adjacent to trunk that form a valid 2-voxel strut
/// line (face-adjacent pair, at least one endpoint adjacent to trunk).
fn find_strut_endpoints(sim: &SimState) -> (VoxelCoord, VoxelCoord) {
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    for &trunk_coord in &tree.trunk_voxels {
        // Try +X direction: two air voxels extending from trunk.
        let a = VoxelCoord::new(trunk_coord.x + 1, trunk_coord.y, trunk_coord.z);
        let b = VoxelCoord::new(trunk_coord.x + 2, trunk_coord.y, trunk_coord.z);
        if sim.world.in_bounds(a)
            && sim.world.in_bounds(b)
            && sim.world.get(a) == VoxelType::Air
            && sim.world.get(b) == VoxelType::Air
        {
            return (a, b);
        }
    }
    panic!("No suitable strut endpoints found");
}
// =======================================================================
// DesignateBuild tests
// =======================================================================

#[test]
fn designate_build_creates_build_task() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    // Blueprint should exist and have a linked task.
    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert!(
        bp.task_id.is_some(),
        "Blueprint should have a linked task_id"
    );

    // Task should exist.
    let task_id = bp.task_id.unwrap();
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert!(task.kind_tag == TaskKindTag::Build);
    assert_eq!(task.state, TaskState::Available);
    assert_eq!(task.total_cost, 1);
    assert_eq!(task.required_species, Some(Species::Elf));
}

#[test]
fn designate_build_rejects_out_of_bounds() {
    let mut sim = test_sim(legacy_test_seed());
    let oob = VoxelCoord::new(-1, 0, 0);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![oob],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(sim.db.blueprints.is_empty());
}

#[test]
fn designate_build_rejects_non_air() {
    let mut sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let trunk_coord = tree.trunk_voxels[0];

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![trunk_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(sim.db.blueprints.is_empty());
}

#[test]
fn designate_build_rejects_no_adjacency() {
    let mut sim = test_sim(legacy_test_seed());
    // Pick a coord far from any solid geometry.
    let isolated = VoxelCoord::new(0, 50, 0);
    assert_eq!(sim.world.get(isolated), VoxelType::Air);
    assert!(!sim.world.has_solid_face_neighbor(isolated));

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![isolated],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(sim.db.blueprints.is_empty());
}

#[test]
fn designate_build_rejects_empty_voxels() {
    let mut sim = test_sim(legacy_test_seed());

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(sim.db.blueprints.is_empty());
}

// =======================================================================
// CancelBuild tests
// =======================================================================

#[test]
fn cancel_build_removes_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // First designate.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);
    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Now cancel.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::CancelBuild { project_id },
    };
    let result = sim.step(&[cmd2], 2);

    assert!(sim.db.blueprints.is_empty());
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::BuildCancelled { .. }))
    );
}

#[test]
fn cancel_build_nonexistent_is_noop() {
    let mut sim = test_sim(legacy_test_seed());
    let fake_id = ProjectId::new(&mut GameRng::new(999));

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::CancelBuild {
            project_id: fake_id,
        },
    };
    let result = sim.step(&[cmd], 1);

    assert!(sim.db.blueprints.is_empty());
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::BuildCancelled { .. }))
    );
}

#[test]
fn cancel_build_removes_associated_task() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Designate a build.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);

    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();
    let task_id = sim.db.blueprints.get(&project_id).unwrap().task_id.unwrap();
    assert!(sim.db.tasks.contains(&task_id));

    // Cancel.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::CancelBuild { project_id },
    };
    sim.step(&[cmd2], 2);

    assert!(sim.db.blueprints.is_empty());
    assert!(!sim.db.tasks.contains(&task_id));
}

#[test]
fn cancel_build_unassigns_elf() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Spawn elf.
    let elf_id = spawn_elf(&mut sim);

    // Designate a build.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Tick enough for the elf to claim the task, but not complete it.
    // The elf claims on its next idle activation after the build is
    // designated. Elf walk speed is 500 tpv, so one wander step takes
    // ~500 ticks. We need enough ticks for at least one idle activation
    // to occur after the build designation, but not enough for the elf to
    // finish the build (1000 work ticks). 800 ticks is enough for one
    // full activation cycle.
    sim.step(&[], sim.tick + 800);

    let task_id = sim.db.blueprints.get(&project_id).unwrap().task_id.unwrap();
    // Wait for the elf to claim the build task. The elf claims on its next
    // idle activation, which depends on when its wander step finishes.
    // Tick in small increments to avoid overshooting past task completion.
    let mut claimed = false;
    for _ in 0..20 {
        sim.step(&[], sim.tick + 100);
        if sim
            .db
            .creatures
            .get(&elf_id)
            .is_some_and(|c| c.current_task == Some(task_id))
        {
            claimed = true;
            break;
        }
    }
    assert!(
        claimed,
        "Elf should have claimed the build task within 2000 ticks"
    );

    // Cancel the build.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::CancelBuild { project_id },
    };
    sim.step(&[cmd2], sim.tick + 2);

    // Elf should be unassigned.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_none(),
        "Elf should have no task after cancel"
    );
}

#[test]
fn cancel_build_reverts_partial_voxels() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Designate a build.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Simulate partial construction by manually placing a voxel.
    sim.placed_voxels
        .push((air_coord, VoxelType::GrownPlatform));
    sim.world.set(air_coord, VoxelType::GrownPlatform);

    // Cancel the build.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::CancelBuild { project_id },
    };
    sim.step(&[cmd2], 2);

    // Voxel should be reverted to Air.
    assert_eq!(sim.world.get(air_coord), VoxelType::Air);
    assert!(
        sim.placed_voxels.is_empty(),
        "placed_voxels should be cleared"
    );
}

// =======================================================================
// Blueprint serialization and determinism
// =======================================================================

#[test]
fn sim_serialization_with_blueprints() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    assert_eq!(restored.db.blueprints.len(), 1);
    let bp = restored.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.voxels, vec![air_coord]);
    assert_eq!(bp.state, BlueprintState::Designated);
}

#[test]
fn blueprint_determinism() {
    let seed = legacy_test_seed();
    let mut sim_a = test_sim(seed);
    let mut sim_b = test_sim(seed);

    let air_a = find_air_adjacent_to_trunk(&sim_a);
    let air_b = find_air_adjacent_to_trunk(&sim_b);
    assert_eq!(air_a, air_b);

    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_a],
            priority: Priority::Normal,
        },
    };
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_b],
            priority: Priority::Normal,
        },
    };
    sim_a.step(&[cmd_a], 1);
    sim_b.step(&[cmd_b], 1);

    let id_a = *sim_a.db.blueprints.iter_keys().next().unwrap();
    let id_b = *sim_b.db.blueprints.iter_keys().next().unwrap();
    assert_eq!(id_a, id_b);
    assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
}

// =======================================================================
// Build work + incremental materialization tests
// =======================================================================

#[test]
fn build_task_completes_and_all_voxels_placed() {
    let mut sim = build_test_sim();
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Spawn elf.
    let elf_id = spawn_elf(&mut sim);

    // Designate a 1-voxel platform.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();
    let task_id = sim.db.blueprints.get(&project_id).unwrap().task_id.unwrap();

    // Tick until completion (elf needs to pathfind + do work). The exact
    // timing depends on PRNG state (spawn position, wander direction), so
    // we step generously.
    sim.step(&[], sim.tick + 400_000);

    // Blueprint should be Complete.
    let bp = &sim.db.blueprints.get(&project_id).unwrap();
    assert_eq!(
        bp.state,
        BlueprintState::Complete,
        "Blueprint should be Complete"
    );

    // Voxel should be solid.
    assert_eq!(
        sim.world.get(air_coord),
        VoxelType::GrownPlatform,
        "Build voxel should be GrownPlatform"
    );

    // Task should be Complete.
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(task.state, TaskState::Complete);

    // Elf should be freed (no current task).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_none(),
        "Elf should be free after build completion"
    );

    // placed_voxels should contain the coord.
    assert!(
        sim.placed_voxels
            .contains(&(air_coord, VoxelType::GrownPlatform))
    );
}

#[test]
fn build_task_materializes_voxels_incrementally() {
    let mut config = test_config();
    // Slow build: 50000 ticks per voxel (elf walk_tpv is 500, so the elf
    // needs to arrive first, then do 50000 ticks of work per voxel).
    config.build_work_ticks_per_voxel = 50000;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    let strip = find_air_strip_adjacent_to_trunk(&sim, 3);

    // Spawn elf.
    spawn_elf(&mut sim);

    // Designate a 3-voxel platform.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: strip.clone(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Tick enough for the elf to arrive and do partial work (enough for
    // 1 voxel but not all 3). Elf walk speed is 500 tpv, so a few
    // thousand ticks should let it arrive. Then 50000 more for 1 voxel.
    sim.step(&[], sim.tick + 200_000);

    // At least 1 voxel should be placed, but not all 3.
    let placed_count = strip
        .iter()
        .filter(|c| sim.world.get(**c) != VoxelType::Air)
        .count();

    // With 200k ticks and 50k per voxel, we'd expect 1-3 placed.
    // The exact count depends on pathfinding time, but at least 1.
    assert!(
        placed_count >= 1,
        "Expected at least 1 voxel placed, got {placed_count}"
    );

    // Blueprint should still be Designated (not all voxels done).
    if placed_count < 3 {
        let bp = &sim.db.blueprints.get(&project_id).unwrap();
        assert_eq!(bp.state, BlueprintState::Designated);
    }
}

#[test]
fn build_voxels_maintain_adjacency() {
    let mut sim = build_test_sim();

    let strip = find_air_strip_adjacent_to_trunk(&sim, 3);

    // Spawn elf.
    spawn_elf(&mut sim);

    // Designate a 3-voxel strip.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: strip.clone(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Tick to completion.
    sim.step(&[], sim.tick + 200_000);

    // All 3 voxels should be solid.
    for coord in &strip {
        assert_eq!(
            sim.world.get(*coord),
            VoxelType::GrownPlatform,
            "Voxel at {coord} should be GrownPlatform"
        );
    }

    // Verify each placed voxel is adjacent to at least one solid neighbor
    // that existed BEFORE it was placed (the trunk or a previously-placed
    // voxel). Since we can't replay the order, we verify the weaker
    // property: each voxel has at least one solid face neighbor now.
    for coord in &strip {
        assert!(
            sim.world.has_solid_face_neighbor(*coord),
            "Placed voxel at {coord} should have a solid face neighbor"
        );
    }
}

#[test]
fn build_displaces_creature_on_occupied_voxel() {
    let mut sim = build_test_sim();
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Spawn an elf, then manually place it at the blueprint voxel.
    let elf_id = spawn_elf(&mut sim);

    // Find the nav node at air_coord (if one exists).
    let node_at_build = sim.nav_graph.find_nearest_node(air_coord);
    if let Some(node_id) = node_at_build {
        let node_pos = sim.nav_graph.node(node_id).position;
        if node_pos == air_coord {
            // Move the elf there.
            let mut elf = sim.db.creatures.get(&elf_id).unwrap().clone();
            elf.position = air_coord;
            sim.db.update_creature(elf).unwrap();
        }
    }

    // Spawn a SECOND elf to do the building (the first one is standing
    // on the build site, so we need another builder).
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Designate the build at the occupied voxel.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Tick to completion.
    sim.step(&[], sim.tick + 200_000);

    // The voxel should be solid.
    assert_eq!(sim.world.get(air_coord), VoxelType::GrownPlatform);

    // The first elf should have been displaced — its position should not
    // be at air_coord (which is now solid).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_ne!(
        elf.position, air_coord,
        "Elf should have been displaced from the now-solid voxel"
    );
    // It should still have a valid nav node.
    assert!(sim.nav_graph.node_at(elf.position).is_some());
}

#[test]
fn save_load_preserves_partially_built_platform() {
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 50000;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    let strip = find_air_strip_adjacent_to_trunk(&sim, 3);

    spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: strip.clone(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Tick for partial construction.
    sim.step(&[], sim.tick + 200_000);

    let placed_before = sim.placed_voxels.len();

    // Save and load.
    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    // placed_voxels should be preserved.
    assert_eq!(restored.placed_voxels.len(), placed_before);

    // The world should contain the placed voxels.
    for &(coord, vt) in &restored.placed_voxels {
        assert_eq!(
            restored.world.get(coord),
            vt,
            "Restored world should contain placed voxel at {coord}"
        );
    }
}

// =======================================================================
// DesignateBuilding tests
// =======================================================================

#[test]
fn designate_building_creates_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuilding {
            anchor,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.build_type, BuildType::Building);
    assert_eq!(bp.voxels.len(), 9); // 3x3x1
    assert!(bp.face_layout.is_some());
    assert_eq!(bp.face_layout.as_ref().unwrap().len(), 9);
}

#[test]
fn designate_building_creates_task() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuilding {
            anchor,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.tasks.len(), 1);
    let task = sim.db.tasks.iter_all().next().unwrap();
    assert_eq!(task.state, TaskState::Available);
    assert_eq!(task.kind_tag, TaskKindTag::Build, "Expected Build task");
    let project_id = sim.task_project_id(task.id).unwrap();
    assert!(sim.db.blueprints.contains(&project_id));
}

#[test]
fn designate_building_rejects_small_width() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuilding {
            anchor,
            width: 2, // too small
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert!(sim.db.blueprints.is_empty());
}

#[test]
fn designate_building_rejects_non_solid_foundation() {
    let mut sim = test_sim(legacy_test_seed());
    // Place anchor at a position where foundation is Air.
    // y=10 should have Air below.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuilding {
            anchor: VoxelCoord::new(1, 10, 1),
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert!(sim.db.blueprints.is_empty());
}

#[test]
fn building_materialization_sets_building_interior() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuilding {
            anchor,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Manually materialize one voxel.
    sim.materialize_next_build_voxel(project_id);

    // At least one voxel should now be BuildingInterior.
    let has_building = sim
        .placed_voxels
        .iter()
        .any(|(_, vt)| *vt == VoxelType::BuildingInterior);
    assert!(has_building, "Should have placed a BuildingInterior voxel");

    // The placed voxel should have face_data.
    let placed_coord = sim.placed_voxels[0].0;
    assert!(
        sim.face_data.contains_key(&placed_coord),
        "Placed building voxel should have face_data",
    );
}

#[test]
fn building_materialization_creates_nav_node() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuilding {
            anchor,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    sim.materialize_next_build_voxel(project_id);

    let placed_coord = sim.placed_voxels[0].0;
    assert!(
        sim.nav_graph.has_node_at(placed_coord),
        "BuildingInterior voxel should be a nav node",
    );
}

#[test]
fn cancel_building_removes_face_data() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuilding {
            anchor,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Materialize some voxels.
    sim.materialize_next_build_voxel(project_id);
    sim.materialize_next_build_voxel(project_id);
    assert!(!sim.face_data.is_empty(), "Should have face_data");
    assert!(!sim.placed_voxels.is_empty(), "Should have placed voxels");

    // Cancel the build.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::CancelBuild { project_id },
    };
    sim.step(&[cmd2], 2);

    assert!(sim.face_data.is_empty(), "face_data should be cleared");
    assert!(
        sim.placed_voxels.is_empty(),
        "placed_voxels should be cleared",
    );
    assert!(sim.db.blueprints.is_empty(), "blueprint should be removed");

    // Verify voxels reverted to Air.
    for x in anchor.x..anchor.x + 3 {
        for z in anchor.z..anchor.z + 3 {
            assert_eq!(
                sim.world.get(VoxelCoord::new(x, anchor.y + 1, z)),
                VoxelType::Air,
            );
        }
    }
}

#[test]
fn save_load_preserves_building() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuilding {
            anchor,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Materialize some voxels.
    sim.materialize_next_build_voxel(project_id);
    sim.materialize_next_build_voxel(project_id);
    sim.materialize_next_build_voxel(project_id);

    let original_face_data_len = sim.face_data.len();
    let original_placed_len = sim.placed_voxels.len();
    assert!(original_face_data_len > 0);
    assert!(original_placed_len > 0);

    // Save and reload.
    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    // Check face_data preserved.
    assert_eq!(restored.face_data.len(), original_face_data_len);
    for (coord, fd) in &sim.face_data {
        let restored_fd = restored.face_data.get(coord).unwrap();
        assert_eq!(fd, restored_fd);
    }

    // Check placed voxels preserved in rebuilt world.
    for &(coord, vt) in &sim.placed_voxels {
        assert_eq!(restored.world.get(coord), vt);
    }

    // Check nav graph has nodes at building voxels.
    for &(coord, vt) in &sim.placed_voxels {
        if vt == VoxelType::BuildingInterior {
            assert!(
                restored.nav_graph.has_node_at(coord),
                "Restored nav graph should have node at {coord}",
            );
        }
    }
}

#[test]
fn designate_building_rejects_non_air_interior() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);

    // Place a solid voxel in the interior area.
    let interior = VoxelCoord::new(anchor.x + 1, anchor.y + 1, anchor.z + 1);
    sim.world.set(interior, VoxelType::Trunk);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuilding {
            anchor,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert!(sim.db.blueprints.is_empty());
}

// =======================================================================
// CompletedStructure integration tests
// =======================================================================

#[test]
fn completed_structure_registered_on_build_complete() {
    let sim = designate_and_complete_build(test_sim(legacy_test_seed()));

    assert_eq!(sim.db.structures.len(), 1);
    let structure = sim.db.structures.iter_all().next().unwrap();
    assert_eq!(structure.id, StructureId(0));
    assert_eq!(structure.build_type, BuildType::Platform);
    assert_eq!(structure.width, 1);
    assert_eq!(structure.depth, 1);
    assert_eq!(structure.height, 1);
    assert!(structure.completed_tick > 0);
}

#[test]
fn completed_structure_sequential_ids() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: air_coord,
        },
    };
    sim.step(&[cmd], 1);

    // Designate first build.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 2);

    // Run until first build completes.
    let max_tick = sim.tick + 1_000_000;
    while sim.tick < max_tick {
        sim.step(&[], sim.tick + 100);
        let all_complete = sim
            .db
            .blueprints
            .iter_all()
            .all(|bp| bp.state == BlueprintState::Complete);
        if all_complete {
            break;
        }
    }
    assert_eq!(sim.db.structures.len(), 1);
    assert_eq!(
        sim.db.structures.iter_all().next().unwrap().id,
        StructureId(0)
    );

    // Find another air coord for the second build.
    let mut second_air = None;
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    for &trunk_coord in &tree.trunk_voxels {
        for (dx, dy, dz) in [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 0, 1),
            (0, 0, -1),
            (0, 1, 0),
            (0, -1, 0),
        ] {
            let neighbor =
                VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y + dy, trunk_coord.z + dz);
            if sim.world.in_bounds(neighbor)
                && sim.world.get(neighbor) == VoxelType::Air
                && neighbor != air_coord
            {
                second_air = Some(neighbor);
                break;
            }
        }
        if second_air.is_some() {
            break;
        }
    }
    let second_coord = second_air.expect("Need a second air coord");

    // Designate second build.
    let tick = sim.tick + 1;
    let cmd = SimCommand {
        player_name: String::new(),
        tick,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![second_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], tick);

    // Run until second build completes.
    let max_tick = sim.tick + 1_000_000;
    while sim.tick < max_tick {
        sim.step(&[], sim.tick + 100);
        let all_complete = sim
            .db
            .blueprints
            .iter_all()
            .all(|bp| bp.state == BlueprintState::Complete);
        if all_complete {
            break;
        }
    }
    assert_eq!(sim.db.structures.len(), 2);

    // IDs should be 0 and 1.
    let ids: Vec<StructureId> = sim.db.structures.iter_keys().copied().collect();
    assert!(ids.contains(&StructureId(0)));
    assert!(ids.contains(&StructureId(1)));
}

#[test]
fn cancel_completed_structure_removes_entry() {
    let mut sim = designate_and_complete_build(test_sim(legacy_test_seed()));
    assert_eq!(sim.db.structures.len(), 1);

    // Get the project_id of the completed structure.
    let project_id = sim.db.structures.iter_all().next().unwrap().project_id;

    // Cancel the build (should remove from structures too).
    let tick = sim.tick + 1;
    let cmd = SimCommand {
        player_name: String::new(),
        tick,
        action: SimAction::CancelBuild { project_id },
    };
    sim.step(&[cmd], tick);

    assert!(
        sim.db.structures.is_empty(),
        "Cancelling a completed build should remove it from structures"
    );
}

// =======================================================================
// Structure voxels and raycast tests
// =======================================================================

#[test]
fn structure_voxels_populated_on_complete_build() {
    let sim = designate_and_complete_build(test_sim(legacy_test_seed()));

    // The completed build should populate structure_voxels.
    assert!(!sim.structure_voxels.is_empty());
    let structure = sim.db.structures.iter_all().next().unwrap();
    let bp = sim
        .db
        .blueprints
        .iter_all()
        .find(|bp| bp.state == BlueprintState::Complete)
        .unwrap();
    for &coord in &bp.voxels {
        assert_eq!(
            sim.structure_voxels.get(&coord),
            Some(&structure.id),
            "Voxel {coord} should map to structure {}",
            structure.id
        );
    }
}

#[test]
fn structure_voxels_cleared_on_cancel_build() {
    let mut sim = designate_and_complete_build(test_sim(legacy_test_seed()));
    assert!(!sim.structure_voxels.is_empty());

    let project_id = sim.db.structures.iter_all().next().unwrap().project_id;

    let tick = sim.tick + 1;
    let cmd = SimCommand {
        player_name: String::new(),
        tick,
        action: SimAction::CancelBuild { project_id },
    };
    sim.step(&[cmd], tick);

    assert!(
        sim.structure_voxels.is_empty(),
        "Cancelling a completed build should clear structure_voxels"
    );
}

#[test]
fn structure_voxels_rebuilt_on_rebuild_transient_state() {
    let sim = designate_and_complete_build(test_sim(legacy_test_seed()));
    let voxels_before = sim.structure_voxels.clone();
    assert!(!voxels_before.is_empty());

    // Round-trip through JSON (which drops transient fields).
    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    assert_eq!(
        restored.structure_voxels, voxels_before,
        "structure_voxels should be identical after save/load"
    );
}

#[test]
fn raycast_structure_finds_structure_voxel() {
    let sim = designate_and_complete_build(test_sim(legacy_test_seed()));
    let structure = sim.db.structures.iter_all().next().unwrap();
    let bp = sim
        .db
        .blueprints
        .iter_all()
        .find(|bp| bp.state == BlueprintState::Complete)
        .unwrap();
    let voxel = bp.voxels[0];

    // Cast a ray from far away on the X axis toward the structure voxel.
    // Horizontal rays avoid tree canopy interference (leaf_size=5 can
    // block vertical rays from above). The world edge at x=0 is far
    // enough from the tree center (x=32) to guarantee a clear path.
    let from = [0.5, voxel.y as f32 + 0.5, voxel.z as f32 + 0.5];
    let dir = [1.0, 0.0, 0.0];
    let result = sim.raycast_structure(from, dir, 100, None);

    assert_eq!(
        result,
        Some(structure.id),
        "Raycast should find the structure at {voxel}"
    );
}

#[test]
fn raycast_structure_stops_at_trunk() {
    let sim = designate_and_complete_build(test_sim(legacy_test_seed()));
    let bp = sim
        .db
        .blueprints
        .iter_all()
        .find(|bp| bp.state == BlueprintState::Complete)
        .unwrap();
    let voxel = bp.voxels[0];

    // Place a trunk voxel between the ray origin and the structure.
    let mut sim = sim;
    let blocker = VoxelCoord::new(voxel.x, voxel.y + 5, voxel.z);
    sim.world.set(blocker, VoxelType::Trunk);

    let from = [
        voxel.x as f32 + 0.5,
        voxel.y as f32 + 10.0,
        voxel.z as f32 + 0.5,
    ];
    let dir = [0.0, -1.0, 0.0];
    let result = sim.raycast_structure(from, dir, 100, None);

    assert_eq!(
        result, None,
        "Raycast should stop at the trunk and not find the structure"
    );
}

#[test]
fn raycast_structure_returns_none_for_empty_ray() {
    let sim = test_sim(legacy_test_seed());
    // Cast a ray into empty space.
    let from = [32.5, 50.0, 32.5];
    let dir = [0.0, 1.0, 0.0];
    let result = sim.raycast_structure(from, dir, 100, None);
    assert_eq!(result, None);
}

// =======================================================================
// Roof-click-select: is_roof_voxel + raycast_structure_with_hit
// =======================================================================

#[test]
fn is_roof_voxel_true_for_top_layer() {
    let sim = designate_and_complete_building(test_sim(legacy_test_seed()));
    let structure = sim.db.structures.iter_all().next().unwrap();

    // For a 3x3x1 building, all interior voxels are at the same Y level,
    // which is also the roof (top layer = only layer).
    let roof_y = structure.anchor.y + structure.height - 1;
    let coord = VoxelCoord::new(structure.anchor.x, roof_y, structure.anchor.z);
    assert!(
        structure.is_roof_voxel(coord),
        "Top-layer voxel should be a roof voxel"
    );
}

#[test]
fn is_roof_voxel_false_for_non_building() {
    let sim = designate_and_complete_build(test_sim(legacy_test_seed()));
    let structure = sim.db.structures.iter_all().next().unwrap();
    assert_eq!(structure.build_type, BuildType::Platform);

    // Platforms have no roof concept.
    let coord = VoxelCoord::new(structure.anchor.x, structure.anchor.y, structure.anchor.z);
    assert!(
        !structure.is_roof_voxel(coord),
        "Platform voxels should never be roof voxels"
    );
}

#[test]
fn is_roof_voxel_false_outside_bounds() {
    let sim = designate_and_complete_building(test_sim(legacy_test_seed()));
    let structure = sim.db.structures.iter_all().next().unwrap();

    let roof_y = structure.anchor.y + structure.height - 1;
    // One step outside in X.
    let outside = VoxelCoord::new(
        structure.anchor.x + structure.width,
        roof_y,
        structure.anchor.z,
    );
    assert!(
        !structure.is_roof_voxel(outside),
        "Voxel outside building bounds should not be a roof voxel"
    );
}

#[test]
fn raycast_structure_with_hit_returns_coord() {
    let sim = designate_and_complete_building(test_sim(legacy_test_seed()));
    let structure = sim.db.structures.iter_all().next().unwrap();
    let bp = sim
        .db
        .blueprints
        .iter_all()
        .find(|bp| bp.state == BlueprintState::Complete)
        .unwrap();
    let voxel = bp.voxels[0];

    // Cast a ray from above straight down.
    let from = [
        voxel.x as f32 + 0.5,
        voxel.y as f32 + 10.0,
        voxel.z as f32 + 0.5,
    ];
    let dir = [0.0, -1.0, 0.0];
    let result = sim.raycast_structure_with_hit(from, dir, 100, false, None);

    assert!(result.is_some(), "Should hit the building");
    let (sid, hit_coord) = result.unwrap();
    assert_eq!(sid, structure.id);
    assert_eq!(hit_coord, voxel);
}

#[test]
fn raycast_roof_voxel_is_identified_as_roof() {
    let sim = designate_and_complete_building(test_sim(legacy_test_seed()));
    let structure = sim.db.structures.iter_all().next().unwrap();

    // For a 3x3x1 building, every interior voxel is the top layer (roof).
    // Cast a ray from above into the center of the building.
    let center_x = structure.anchor.x as f32 + structure.width as f32 / 2.0;
    let roof_y = structure.anchor.y + structure.height - 1;
    let center_z = structure.anchor.z as f32 + structure.depth as f32 / 2.0;

    let from = [center_x, roof_y as f32 + 10.0, center_z];
    let dir = [0.0, -1.0, 0.0];
    let result = sim.raycast_structure_with_hit(from, dir, 100, false, None);

    assert!(result.is_some());
    let (sid, hit_coord) = result.unwrap();
    assert_eq!(sid, structure.id);
    assert!(
        structure.is_roof_voxel(hit_coord),
        "Hit voxel at {:?} should be a roof voxel (roof_y={roof_y})",
        hit_coord
    );
}

#[test]
fn raycast_skip_roofs_passes_through_roof_voxels() {
    let sim = designate_and_complete_building(test_sim(legacy_test_seed()));
    let structure = sim.db.structures.iter_all().next().unwrap();

    let center_x = structure.anchor.x as f32 + structure.width as f32 / 2.0;
    let roof_y = structure.anchor.y + structure.height - 1;
    let center_z = structure.anchor.z as f32 + structure.depth as f32 / 2.0;

    let from = [center_x, roof_y as f32 + 10.0, center_z];
    let dir = [0.0, -1.0, 0.0];

    // Without skip_roofs: hits the roof.
    let result = sim.raycast_structure_with_hit(from, dir, 100, false, None);
    assert!(result.is_some());
    let (_, hit_coord) = result.unwrap();
    assert!(structure.is_roof_voxel(hit_coord));

    // With skip_roofs: passes through the roof. May hit a non-roof structure
    // voxel below, or nothing if the building is only 1 voxel tall.
    let result_skip = sim.raycast_structure_with_hit(from, dir, 100, true, None);
    if let Some((_, coord)) = result_skip {
        assert!(
            !structure.is_roof_voxel(coord),
            "skip_roofs should not return a roof voxel"
        );
    }
}

#[test]
fn raycast_structure_with_hit_skips_above_y_cutoff() {
    let sim = designate_and_complete_building(test_sim(legacy_test_seed()));
    let structure = sim.db.structures.iter_all().next().unwrap();
    let bp = sim
        .db
        .blueprints
        .iter_all()
        .find(|bp| bp.state == BlueprintState::Complete)
        .unwrap();
    let voxel = bp.voxels[0];

    // Cast a ray from above straight down — would normally hit.
    let from = [
        voxel.x as f32 + 0.5,
        voxel.y as f32 + 10.0,
        voxel.z as f32 + 0.5,
    ];
    let dir = [0.0, -1.0, 0.0];

    // Sanity: without y_cutoff, we do hit the structure.
    let result_no_cutoff = sim.raycast_structure_with_hit(from, dir, 100, false, None);
    assert!(result_no_cutoff.is_some(), "Should hit without y_cutoff");
    assert_eq!(result_no_cutoff.unwrap().0, structure.id);

    // With y_cutoff below the structure's Y, the voxel is treated as air.
    let result = sim.raycast_structure_with_hit(from, dir, 100, false, Some(voxel.y));
    assert_eq!(
        result, None,
        "Structure at y={} should be skipped with y_cutoff={}",
        voxel.y, voxel.y,
    );
}

// =======================================================================
// RenameStructure tests
// =======================================================================

#[test]
fn rename_structure_sets_custom_name() {
    let mut sim = designate_and_complete_build(test_sim(legacy_test_seed()));
    assert_eq!(sim.db.structures.len(), 1);
    let sid = *sim.db.structures.iter_keys().next().unwrap();
    assert_eq!(
        sim.db.structures.get(&sid).unwrap().display_name(),
        "Platform #0"
    );

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::RenameStructure {
            structure_id: sid,
            name: Some("Great Hall".to_string()),
        },
    };
    sim.step(&[cmd], sim.tick + 1);
    assert_eq!(
        sim.db.structures.get(&sid).unwrap().display_name(),
        "Great Hall"
    );
}

#[test]
fn rename_structure_to_none_resets_to_default() {
    let mut sim = designate_and_complete_build(test_sim(legacy_test_seed()));
    let sid = *sim.db.structures.iter_keys().next().unwrap();

    // Set a custom name.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::RenameStructure {
            structure_id: sid,
            name: Some("Great Hall".to_string()),
        },
    };
    sim.step(&[cmd], sim.tick + 1);
    assert_eq!(
        sim.db.structures.get(&sid).unwrap().display_name(),
        "Great Hall"
    );

    // Reset to default.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::RenameStructure {
            structure_id: sid,
            name: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);
    assert_eq!(
        sim.db.structures.get(&sid).unwrap().display_name(),
        "Platform #0"
    );
}

#[test]
fn rename_nonexistent_structure_is_noop() {
    let mut sim = test_sim(legacy_test_seed());
    let tick_before = sim.tick;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::RenameStructure {
            structure_id: StructureId(999),
            name: Some("Ghost".to_string()),
        },
    };
    // Should not panic.
    sim.step(&[cmd], sim.tick + 1);
    assert!(sim.db.structures.is_empty());
    assert!(sim.tick > tick_before);
}

// =======================================================================
// Overlap (leaf/trunk) build tests
// =======================================================================

#[test]
fn overlap_platform_at_leaf_creates_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    let leaf_coord = find_leaf_adjacent_to_wood(&sim);
    assert_eq!(sim.world.get(leaf_coord), VoxelType::Leaf);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![leaf_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1, "Blueprint should be created");
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.voxels, vec![leaf_coord]);
    assert_eq!(bp.original_voxels.len(), 1);
    assert_eq!(bp.original_voxels[0], (leaf_coord, VoxelType::Leaf));
}

#[test]
fn overlap_all_trunk_rejects_nothing_to_build() {
    let mut sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let trunk_coord = tree.trunk_voxels[0];

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![trunk_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(sim.db.blueprints.is_empty(), "All-trunk should be rejected");
    assert_eq!(
        sim.last_build_message.as_deref(),
        Some("Nothing to build — all voxels are already wood.")
    );
}

#[test]
fn overlap_mixed_air_trunk_only_builds_air() {
    let mut sim = test_sim(legacy_test_seed());
    // Find a trunk voxel with an air neighbor.
    let air_coord = find_air_adjacent_to_trunk(&sim);
    // Find which trunk voxel is adjacent.
    let mut trunk_coord = None;
    for &(dx, dy, dz) in &[
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ] {
        let neighbor = VoxelCoord::new(air_coord.x + dx, air_coord.y + dy, air_coord.z + dz);
        if sim.world.in_bounds(neighbor)
            && matches!(
                sim.world.get(neighbor),
                VoxelType::Trunk | VoxelType::Branch | VoxelType::Root
            )
        {
            trunk_coord = Some(neighbor);
            break;
        }
    }
    let trunk_coord = trunk_coord.expect("Should find adjacent trunk");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord, trunk_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    // Only the air voxel should be in the blueprint.
    assert_eq!(bp.voxels, vec![air_coord]);
    assert!(bp.original_voxels.is_empty());
}

#[test]
fn overlap_blocked_voxel_rejects() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // First build a platform at the air coord.
    sim.world.set(air_coord, VoxelType::GrownPlatform);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(
        sim.db.blueprints.is_empty(),
        "Blocked voxel should reject build"
    );
}

#[test]
fn overlap_leaf_materializes_to_grown_platform() {
    let mut sim = build_test_sim();
    let leaf_coord = find_leaf_adjacent_to_wood(&sim);
    assert_eq!(sim.world.get(leaf_coord), VoxelType::Leaf);

    // Spawn elf.
    spawn_elf(&mut sim);

    // Designate platform at the leaf voxel.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![leaf_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Tick until completion.
    sim.step(&[], sim.tick + 200_000);

    // Leaf should have been converted to GrownPlatform.
    assert_eq!(
        sim.world.get(leaf_coord),
        VoxelType::GrownPlatform,
        "Leaf voxel should be converted to GrownPlatform"
    );

    // Blueprint should be Complete.
    let bp = &sim.db.blueprints.get(&project_id).unwrap();
    assert_eq!(bp.state, BlueprintState::Complete);
}

#[test]
fn overlap_cancel_reverts_to_original_type() {
    let mut sim = test_sim(legacy_test_seed());
    let leaf_coord = find_leaf_adjacent_to_wood(&sim);
    assert_eq!(sim.world.get(leaf_coord), VoxelType::Leaf);

    // Designate platform at the leaf voxel.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![leaf_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Simulate partial construction by manually placing the voxel.
    sim.placed_voxels
        .push((leaf_coord, VoxelType::GrownPlatform));
    sim.world.set(leaf_coord, VoxelType::GrownPlatform);

    // Cancel the build.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::CancelBuild { project_id },
    };
    sim.step(&[cmd2], 2);

    // Voxel should revert to Leaf, not Air.
    assert_eq!(
        sim.world.get(leaf_coord),
        VoxelType::Leaf,
        "Cancelled overlap build should revert to original Leaf, not Air"
    );
}

#[test]
fn overlap_save_load_preserves_original_voxels() {
    let mut sim = test_sim(legacy_test_seed());
    let leaf_coord = find_leaf_adjacent_to_wood(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![leaf_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    assert_eq!(restored.db.blueprints.len(), 1);
    let bp = restored.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.original_voxels.len(), 1);
    assert_eq!(bp.original_voxels[0], (leaf_coord, VoxelType::Leaf));
}

#[test]
fn overlap_determinism() {
    let seed = legacy_test_seed();
    let mut sim_a = test_sim(seed);
    let mut sim_b = test_sim(seed);

    let leaf_a = find_leaf_adjacent_to_wood(&sim_a);
    let leaf_b = find_leaf_adjacent_to_wood(&sim_b);
    assert_eq!(leaf_a, leaf_b);

    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![leaf_a],
            priority: Priority::Normal,
        },
    };
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![leaf_b],
            priority: Priority::Normal,
        },
    };
    sim_a.step(&[cmd_a], 1);
    sim_b.step(&[cmd_b], 1);

    let id_a = *sim_a.db.blueprints.iter_keys().next().unwrap();
    let id_b = *sim_b.db.blueprints.iter_keys().next().unwrap();
    assert_eq!(id_a, id_b);

    let bp_a = sim_a.db.blueprints.get(&id_a).unwrap();
    let bp_b = sim_b.db.blueprints.get(&id_b).unwrap();
    assert_eq!(bp_a.voxels, bp_b.voxels);
    assert_eq!(bp_a.original_voxels, bp_b.original_voxels);
    assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
}

#[test]
fn overlap_wall_at_leaf_rejects() {
    let mut sim = test_sim(legacy_test_seed());
    let leaf_coord = find_leaf_adjacent_to_wood(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Wall,
            voxels: vec![leaf_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(
        sim.db.blueprints.is_empty(),
        "Wall does not allow overlap, should reject leaf"
    );
}

// =======================================================================
// Carve tests
// =======================================================================

#[test]
fn test_designate_carve_filters_air() {
    let mut sim = test_sim(legacy_test_seed());
    let solid = find_carvable_voxel(&sim);
    // Pick an air voxel (high up, guaranteed empty).
    let air = VoxelCoord::new(5, 50, 5);
    assert_eq!(sim.world.get(air), VoxelType::Air);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![solid, air],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    // A blueprint should exist with only the solid voxel.
    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.build_type, BuildType::Carve);
    assert_eq!(bp.voxels.len(), 1);
    assert_eq!(bp.voxels[0], solid);
}

#[test]
fn test_carve_execution_removes_voxels() {
    let mut config = test_config();
    // Set carve ticks very low so the test completes quickly.
    config.carve_work_ticks_per_voxel = 1;
    // Disable needs so the elf focuses on carving.
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    // Pick the lowest trunk voxel (reachable from floor) instead of the
    // highest one, which may be unreachable with some tree shapes.
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let solid = tree
        .trunk_voxels
        .iter()
        .copied()
        .filter(|v| v.y > 0)
        .min_by_key(|v| v.y)
        .expect("No trunk voxel above floor");
    assert!(sim.world.get(solid).is_solid());

    // Spawn an elf at tree base and designate carve.
    let _elf_id = spawn_elf(&mut sim);
    let carve_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateCarve {
            voxels: vec![solid],
            priority: Priority::Normal,
        },
    };
    sim.step(&[carve_cmd], sim.tick + 2);

    // Ensure carve blueprint was created (not blocked by structural check).
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Blueprint should exist; last_build_message: {:?}",
        sim.last_build_message
    );

    // Run the sim long enough for the elf to reach and carve.
    sim.step(&[], 500_000);

    // The solid voxel should now be Air.
    assert_eq!(
        sim.world.get(solid),
        VoxelType::Air,
        "Carved voxel should be Air"
    );
    assert!(
        sim.carved_voxels.contains(&solid),
        "carved_voxels should track the removal"
    );
}

#[test]
fn test_carve_task_location_uses_nav_node() {
    // The carve task location should be at the nearest nav node's position,
    // not at the raw carve voxel coordinate. Underground dirt voxels have no
    // nearby nav node, so using the voxel directly causes a full-world
    // expanding-box search in find_available_task (~415ms per creature).
    //
    // Use terrain_max_height > 0 so dirt extends above y=0 (y=0 is bedrock,
    // not carvable). The test_config sets floor_y=0 and terrain_max_height=0,
    // giving no carvable dirt.
    let mut config = test_config();
    config.terrain_max_height = 3;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let tree_pos = tree.position;

    // Find a carvable dirt voxel near the tree (y > 0 so it's above bedrock).
    let dirt_coord = (-20i32..=20)
        .flat_map(|dx| (-20i32..=20).map(move |dz| (dx, dz)))
        .flat_map(|(dx, dz)| (1i32..=5).rev().map(move |y| (dx, y, dz)))
        .map(|(dx, y, dz)| VoxelCoord::new(tree_pos.x + dx, y, tree_pos.z + dz))
        .find(|&c| sim.world.in_bounds(c) && sim.world.get(c) == VoxelType::Dirt && c.y > 0)
        .expect("Should find carvable dirt voxel near tree");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![dirt_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    // Find the carve task.
    let task = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == crate::db::TaskKindTag::Build)
        .expect("Carve task should exist");

    // Task location should be at a nav node, not the underground dirt voxel.
    assert!(
        sim.nav_graph.node_at(task.location).is_some(),
        "Task location {:?} should be a valid nav node",
        task.location,
    );
}

#[test]
fn test_designate_carve_accepts_dirt_voxels() {
    // Regression test for B-carve-dirt: pure dirt voxels above bedrock
    // must be accepted by designate_carve (and by validate_carve_preview
    // in gdext, which mirrors this filter).
    let mut config = test_config();
    config.terrain_max_height = 3;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let tree_pos = tree.position;

    // Find a dirt voxel above bedrock (y > 0).
    let dirt_coord = (-20i32..=20)
        .flat_map(|dx| (-20i32..=20).map(move |dz| (dx, dz)))
        .flat_map(|(dx, dz)| (1i32..=5).rev().map(move |y| (dx, y, dz)))
        .map(|(dx, y, dz)| VoxelCoord::new(tree_pos.x + dx, y, tree_pos.z + dz))
        .find(|&c| sim.world.in_bounds(c) && sim.world.get(c) == VoxelType::Dirt && c.y > 0)
        .expect("Should find carvable dirt voxel near tree");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![dirt_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    // A blueprint should be created — dirt above bedrock is carvable.
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Dirt voxel at {:?} should be carvable; last_build_message: {:?}",
        dirt_coord,
        sim.last_build_message,
    );
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.build_type, BuildType::Carve);
    assert_eq!(bp.voxels, vec![dirt_coord]);
}

#[test]
fn test_carve_skips_bedrock_layer() {
    let mut sim = test_sim(legacy_test_seed());
    let (ws_x, _, ws_z) = sim.config.world_size;
    let center_x = ws_x as i32 / 2;
    let center_z = ws_z as i32 / 2;
    let bedrock = VoxelCoord::new(center_x, 0, center_z);
    assert!(sim.world.get(bedrock).is_solid());

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![bedrock],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    // No blueprint should be created — y=0 bedrock is not carvable.
    assert!(
        sim.db.blueprints.is_empty(),
        "Bedrock layer (y=0) should not be carvable"
    );
    assert_eq!(sim.last_build_message.as_deref(), Some("Nothing to carve."));
}

#[test]
fn test_cancel_carve_restores_originals() {
    let mut config = test_config();
    config.carve_work_ticks_per_voxel = 1;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    // Find two adjacent trunk voxels for carving.
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let v1 = tree.trunk_voxels.iter().copied().find(|v| v.y > 0).unwrap();
    let original_type = sim.world.get(v1);
    assert!(original_type.is_solid());

    // Spawn elf and designate carve.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z + 3);
    let spawn_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: elf_pos,
        },
    };
    let carve_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateCarve {
            voxels: vec![v1],
            priority: Priority::Normal,
        },
    };
    sim.step(&[spawn_cmd, carve_cmd], 2);

    // Run long enough for the carve to complete.
    sim.step(&[], 500_000);
    assert_eq!(sim.world.get(v1), VoxelType::Air);

    // Now cancel the build.
    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();
    let cancel_cmd = SimCommand {
        player_name: String::new(),
        tick: 500_001,
        action: SimAction::CancelBuild { project_id },
    };
    sim.step(&[cancel_cmd], 500_001);

    // The voxel should be restored to its original type.
    assert_eq!(
        sim.world.get(v1),
        original_type,
        "Cancelled carve should restore original voxel type"
    );
    assert!(
        !sim.carved_voxels.contains(&v1),
        "carved_voxels should be cleaned up"
    );
}

#[test]
fn test_carve_nav_graph_update() {
    let mut config = test_config();
    config.carve_work_ticks_per_voxel = 1;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    // Find a solid voxel that is part of the tree.
    let solid = find_carvable_voxel(&sim);
    // Before carving, the voxel is solid — it should not be a nav node itself.
    assert!(
        sim.world.get(solid).is_solid(),
        "Precondition: voxel is solid"
    );

    // Spawn elf near the tree base.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let spawn_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    let carve_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateCarve {
            voxels: vec![solid],
            priority: Priority::Normal,
        },
    };
    sim.step(&[spawn_cmd, carve_cmd], 2);

    // Run sim to complete the carve.
    sim.step(&[], 500_000);

    // After carving, the voxel is Air. If it has a solid face neighbor,
    // it should now be a nav node.
    assert_eq!(sim.world.get(solid), VoxelType::Air);
    if sim.world.has_solid_face_neighbor(solid) {
        let node = sim.nav_graph.find_nearest_node(solid);
        assert!(
            node.is_some(),
            "Carved voxel with solid neighbor should be a nav node"
        );
    }
}

#[test]
fn test_carve_save_load_roundtrip() {
    let mut config = test_config();
    config.carve_work_ticks_per_voxel = 1;
    // Disable needs so the elf focuses on carving.
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    // Pick a low trunk voxel (reachable from floor) instead of the highest
    // one, which may be unreachable with some tree shapes.
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let solid = tree
        .trunk_voxels
        .iter()
        .copied()
        .filter(|v| v.y > 0)
        .min_by_key(|v| v.y)
        .expect("No trunk voxel above floor");
    let original_type = sim.world.get(solid);

    // Spawn elf at tree base and designate carve.
    let _elf_id = spawn_elf(&mut sim);
    let carve_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateCarve {
            voxels: vec![solid],
            priority: Priority::Normal,
        },
    };
    sim.step(&[carve_cmd], sim.tick + 2);

    // Complete the carve.
    sim.step(&[], sim.tick + 500_000);
    assert_eq!(sim.world.get(solid), VoxelType::Air);
    assert!(sim.carved_voxels.contains(&solid));

    // Save and load.
    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    // Verify carved voxels survived.
    assert!(restored.carved_voxels.contains(&solid));
    assert_eq!(
        restored.world.get(solid),
        VoxelType::Air,
        "Carved voxel should be Air after reload"
    );

    // Verify the original type is in the blueprint's original_voxels.
    let bp = restored.db.blueprints.iter_all().next().unwrap();
    let orig = bp.original_voxels.iter().find(|(c, _)| *c == solid);
    assert!(orig.is_some());
    assert_eq!(orig.unwrap().1, original_type);
}

#[test]
fn carve_uses_separate_duration_from_build() {
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 5000;
    config.carve_work_ticks_per_voxel = 500_000; // 100x slower than build.
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    // Find a non-forest-floor solid voxel to carve (e.g., a trunk voxel
    // that isn't on the ground).
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let carve_coord = tree
        .trunk_voxels
        .iter()
        .find(|c| c.y > 1)
        .copied()
        .expect("Should have trunk voxels above y=1");

    let _elf_id = spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateCarve {
            voxels: vec![carve_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Verify the carve task exists.
    let task = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == TaskKindTag::Build);
    assert!(task.is_some(), "Carve task should exist");
    let task_id = task.unwrap().id;

    // Run enough for the elf to arrive and start working, but less than
    // carve_work_ticks_per_voxel. A build (5000 ticks) would be done by now,
    // but a carve (500_000 ticks) should still be in progress.
    sim.step(&[], sim.tick + 100_000);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::InProgress,
        "Carve should still be in progress (500k ticks) — a build (5k) would be done by now"
    );
}

// =======================================================================
// Ladder tests
// =======================================================================

#[test]
fn designate_wood_ladder_creates_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    let (anchor, orientation) = find_ladder_column(&sim, 3);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor,
            height: 3,
            orientation,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    let result = sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.build_type, BuildType::WoodLadder);
    assert_eq!(bp.voxels.len(), 3);
    assert_eq!(bp.state, BlueprintState::Designated);
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::BlueprintDesignated { .. }))
    );
}

#[test]
fn designate_rope_ladder_creates_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    // Rope ladders need top voxel adjacent to solid. Find a trunk voxel
    // with air below and on the side.
    let (anchor, orientation) = find_ladder_column(&sim, 1);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor,
            height: 1,
            orientation,
            kind: LadderKind::Rope,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.build_type, BuildType::RopeLadder);
}

#[test]
fn designate_ladder_rejects_vertical_orientation() {
    let mut sim = test_sim(legacy_test_seed());
    let (anchor, _) = find_ladder_column(&sim, 1);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor,
            height: 1,
            orientation: FaceDirection::PosY,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(sim.db.blueprints.is_empty());
    assert_eq!(
        sim.last_build_message.as_deref(),
        Some("Ladder orientation must be horizontal.")
    );
}

#[test]
fn designate_ladder_rejects_zero_height() {
    let mut sim = test_sim(legacy_test_seed());
    let (anchor, orientation) = find_ladder_column(&sim, 1);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor,
            height: 0,
            orientation,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(sim.db.blueprints.is_empty());
    assert_eq!(
        sim.last_build_message.as_deref(),
        Some("Ladder height must be at least 1.")
    );
}

#[test]
fn designate_wood_ladder_rejects_no_anchor() {
    let mut sim = test_sim(legacy_test_seed());
    // Place ladder in open air with no adjacent solid.
    let anchor = VoxelCoord::new(1, 10, 1);
    // Confirm it's air with no solid neighbor.
    assert_eq!(sim.world.get(anchor), VoxelType::Air);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor,
            height: 1,
            orientation: FaceDirection::PosX,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert!(sim.db.blueprints.is_empty());
}

#[test]
fn designate_ladder_creates_build_task() {
    let mut sim = test_sim(legacy_test_seed());
    let (anchor, orientation) = find_ladder_column(&sim, 2);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor,
            height: 2,
            orientation,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert!(bp.task_id.is_some());
    let task = sim.db.tasks.get(&bp.task_id.unwrap()).unwrap();
    assert!(task.kind_tag == TaskKindTag::Build);
    assert_eq!(task.required_species, Some(Species::Elf));
    assert_eq!(task.total_cost, 2);
}

#[test]
fn ladder_face_data_blocks_correctly() {
    let fd = ladder_face_data(FaceDirection::PosX);
    // Only the ladder face (PosX) should be Wall.
    assert_eq!(fd.get(FaceDirection::PosX), FaceType::Wall);
    // All other faces should be Open.
    assert_eq!(fd.get(FaceDirection::NegX), FaceType::Open);
    assert_eq!(fd.get(FaceDirection::PosZ), FaceType::Open);
    assert_eq!(fd.get(FaceDirection::NegZ), FaceType::Open);
    assert_eq!(fd.get(FaceDirection::PosY), FaceType::Open);
    assert_eq!(fd.get(FaceDirection::NegY), FaceType::Open);
}

#[test]
fn ladder_voxel_type_not_solid() {
    assert!(!VoxelType::WoodLadder.is_solid());
    assert!(!VoxelType::RopeLadder.is_solid());
}

#[test]
fn ladder_voxel_type_is_ladder() {
    assert!(VoxelType::WoodLadder.is_ladder());
    assert!(VoxelType::RopeLadder.is_ladder());
    assert!(!VoxelType::Air.is_ladder());
    assert!(!VoxelType::Trunk.is_ladder());
}

#[test]
fn ladder_build_type_allows_tree_overlap() {
    assert!(BuildType::WoodLadder.allows_tree_overlap());
    assert!(BuildType::RopeLadder.allows_tree_overlap());
}

#[test]
fn cancel_ladder_removes_blueprint_and_data() {
    let mut sim = test_sim(legacy_test_seed());
    let (anchor, orientation) = find_ladder_column(&sim, 2);

    // Designate a wood ladder.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor,
            height: 2,
            orientation,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();
    let cancel_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::CancelBuild { project_id },
    };
    let result = sim.step(&[cancel_cmd], 2);

    assert!(sim.db.blueprints.is_empty());
    assert!(sim.db.tasks.is_empty());
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::BuildCancelled { .. }))
    );
}

#[test]
fn ladder_save_load_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let (anchor, orientation) = find_ladder_column(&sim, 2);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor,
            height: 2,
            orientation,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();
    assert_eq!(restored.db.blueprints.len(), 1);
    let bp = restored.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.build_type, BuildType::WoodLadder);
}

#[test]
fn designate_rope_ladder_rejects_no_anchor() {
    let mut sim = test_sim(legacy_test_seed());
    // Find a column of 3 air voxels next to trunk, then try placing a
    // rope ladder of height 3. The top voxel's ladder face must be
    // adjacent to solid — pick an anchor where that's not the case.
    let (anchor, orientation) = find_ladder_column(&sim, 3);
    // The anchor faces toward trunk, so height=1 passes (top is adjacent).
    // But we want to fail: place the anchor 2 voxels further out from the
    // trunk so the top voxel's neighbor is air.
    let (odx, _, odz) = orientation.to_offset();
    let far_anchor = VoxelCoord::new(anchor.x - odx * 2, anchor.y, anchor.z - odz * 2);
    // Make sure the far anchor column is air (best effort — skip if not).
    let all_air = (0..3).all(|dy| {
        let coord = VoxelCoord::new(far_anchor.x, far_anchor.y + dy, far_anchor.z);
        sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air
    });
    if !all_air {
        // Can't construct the test scenario — skip gracefully.
        return;
    }

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor: far_anchor,
            height: 3,
            orientation,
            kind: LadderKind::Rope,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 0);
    assert!(sim.last_build_message.is_some());
}

#[test]
fn designate_rope_ladder_multiheight() {
    let mut sim = test_sim(legacy_test_seed());
    // Find a column of 3 air voxels next to trunk. Rope ladder needs
    // top voxel adjacent to solid — but the anchor is at the bottom.
    // With height=3, the top (anchor.y+2) must have its ladder face
    // neighbor be solid.
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let mut found = None;
    for &trunk_coord in &tree.trunk_voxels {
        for &(dx, dz) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let orientation = FaceDirection::from_offset(-dx, 0, -dz).unwrap();
            // We need a column of 3 air voxels starting below the trunk,
            // where the topmost voxel (base.y+2) is at trunk_coord.y.
            let base = VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y - 2, trunk_coord.z + dz);
            let all_air = (0..3).all(|dy| {
                let coord = VoxelCoord::new(base.x, base.y + dy, base.z);
                sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air
            });
            // Top voxel's ladder-face neighbor must be solid (the trunk).
            let (odx, _, odz) = orientation.to_offset();
            let top_neighbor = VoxelCoord::new(base.x + odx, base.y + 2, base.z + odz);
            if all_air && sim.world.get(top_neighbor).is_solid() {
                found = Some((base, orientation));
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let (anchor, orientation) = found.expect("No suitable multi-height rope column found");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor,
            height: 3,
            orientation,
            kind: LadderKind::Rope,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.build_type, BuildType::RopeLadder);
    assert_eq!(bp.voxels.len(), 3);
}

#[test]
fn ladder_classify_for_overlap_blocked() {
    assert_eq!(
        VoxelType::WoodLadder.classify_for_overlap(),
        OverlapClassification::Blocked
    );
    assert_eq!(
        VoxelType::RopeLadder.classify_for_overlap(),
        OverlapClassification::Blocked
    );
}

#[test]
fn ladder_build_type_to_voxel_type() {
    assert_eq!(BuildType::WoodLadder.to_voxel_type(), VoxelType::WoodLadder);
    assert_eq!(BuildType::RopeLadder.to_voxel_type(), VoxelType::RopeLadder);
}

#[test]
fn designate_ladder_determinism() {
    let seed = legacy_test_seed();
    let (anchor_a, orientation_a) = {
        let sim = test_sim(seed);
        find_ladder_column(&sim, 3)
    };
    let (anchor_b, orientation_b) = {
        let sim = test_sim(seed);
        find_ladder_column(&sim, 3)
    };
    assert_eq!(anchor_a, anchor_b);
    assert_eq!(orientation_a, orientation_b);

    let mut sim_a = test_sim(legacy_test_seed());
    let mut sim_b = test_sim(legacy_test_seed());

    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor: anchor_a,
            height: 3,
            orientation: orientation_a,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateLadder {
            anchor: anchor_b,
            height: 3,
            orientation: orientation_b,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    sim_a.step(&[cmd_a], 1);
    sim_b.step(&[cmd_b], 1);

    assert_eq!(sim_a.db.blueprints.len(), sim_b.db.blueprints.len());
}

// =======================================================================
// Blueprint overlap rejection (F-no-bp-overlap)
// =======================================================================

#[test]
fn build_rejects_overlap_with_existing_build_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // First designation succeeds.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Second designation on the same voxel should be rejected.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Second blueprint should not be created"
    );
    assert!(
        sim.last_build_message
            .as_ref()
            .unwrap()
            .contains("existing blueprint"),
        "Should mention existing blueprint overlap: {:?}",
        sim.last_build_message
    );
}

#[test]
fn build_rejects_overlap_with_carve_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    let solid = find_carvable_voxel(&sim);

    // Designate a carve on the solid voxel.
    let carve_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![solid],
            priority: Priority::Normal,
        },
    };
    sim.step(&[carve_cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Attempt to build on the same voxel (carve shows as Air in overlay).
    // This should be rejected due to blueprint overlap, not silently allowed.
    let build_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Wall,
            voxels: vec![solid],
            priority: Priority::Normal,
        },
    };
    sim.step(&[build_cmd], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Build should not overlap carve blueprint"
    );
    assert!(
        sim.last_build_message
            .as_ref()
            .unwrap()
            .contains("existing blueprint"),
    );
}

#[test]
fn carve_filters_out_build_blueprint_voxels() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Designate a platform build.
    let build_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[build_cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Attempt to carve the same voxel — filtered out as blueprint-claimed,
    // so nothing to carve.
    let carve_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateCarve {
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[carve_cmd], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Carve should not overlap build blueprint"
    );
}

#[test]
fn carve_filters_out_carve_blueprint_voxels() {
    let mut sim = test_sim(legacy_test_seed());
    let solid = find_carvable_voxel(&sim);

    // First carve designation.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![solid],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Second carve on the same voxel — filtered out.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateCarve {
            voxels: vec![solid],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Double carve should be filtered"
    );
}

#[test]
fn building_rejects_overlap_with_existing_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    let site = find_building_site(&sim);

    // Designate a platform on one of the building's wall voxels (y+1).
    // Building walls are at the perimeter of (site.x..site.x+3, site.y+1, site.z..site.z+3).
    let wall_coord = VoxelCoord::new(site.x, site.y + 1, site.z);
    assert_eq!(sim.world.get(wall_coord), VoxelType::Air);

    // First: place a platform at the wall position.
    // Need adjacency — wall_coord is above solid ground, so has a solid
    // face neighbor below.
    let platform_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![wall_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[platform_cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Now try to designate a building that overlaps.
    let building_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuilding {
            anchor: site,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[building_cmd], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Building should be rejected due to overlap; msg: {:?}",
        sim.last_build_message
    );
    assert!(
        sim.last_build_message
            .as_ref()
            .unwrap()
            .contains("existing blueprint"),
    );
}

#[test]
fn ladder_rejects_overlap_with_existing_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    let (anchor, orientation) = find_ladder_column(&sim, 2);

    // Designate a platform at the ladder's anchor voxel.
    let platform_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![anchor],
            priority: Priority::Normal,
        },
    };
    sim.step(&[platform_cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Now try to designate a ladder overlapping that voxel.
    let ladder_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateLadder {
            anchor,
            height: 2,
            orientation,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    sim.step(&[ladder_cmd], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Ladder should be rejected due to overlap; msg: {:?}",
        sim.last_build_message
    );
    assert!(
        sim.last_build_message
            .as_ref()
            .unwrap()
            .contains("existing blueprint"),
    );
}

#[test]
fn carve_partial_overlap_filters_claimed_voxels() {
    // When carving a region where some voxels are claimed by an existing
    // blueprint and some are not, only unclaimed voxels appear in the
    // new carve blueprint.
    let mut sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();

    // Find two carvable (solid, above bedrock) voxels.
    let carvable: Vec<VoxelCoord> = tree
        .trunk_voxels
        .iter()
        .copied()
        .filter(|v| {
            let vt = sim.world.get(*v);
            vt.is_solid() && v.y > 0
        })
        .take(2)
        .collect();
    assert!(carvable.len() >= 2, "Need two carvable voxels");

    // Designate a carve on the first voxel.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![carvable[0]],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Now carve both voxels — the first should be filtered out.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateCarve {
            voxels: carvable.clone(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        2,
        "Second carve should succeed with the unclaimed voxel"
    );
    let second_bp = sim
        .db
        .blueprints
        .iter_all()
        .find(|bp| bp.voxels.len() == 1 && bp.voxels[0] == carvable[1])
        .expect("Second blueprint should contain only the unclaimed voxel");
    assert_eq!(second_bp.voxels, vec![carvable[1]]);
}

#[test]
fn build_partial_overlap_rejected() {
    // If a multi-voxel designation has even one voxel overlapping an
    // existing blueprint, the entire designation is rejected.
    let mut sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();

    // Find two adjacent air voxels next to trunk.
    let mut air_pair: Option<(VoxelCoord, VoxelCoord)> = None;
    'outer: for &trunk_coord in &tree.trunk_voxels {
        for &(dx, dz) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let a = VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y, trunk_coord.z + dz);
            let b = VoxelCoord::new(a.x + dx, a.y, a.z + dz);
            if sim.world.in_bounds(a)
                && sim.world.in_bounds(b)
                && sim.world.get(a) == VoxelType::Air
                && sim.world.get(b) == VoxelType::Air
            {
                air_pair = Some((a, b));
                break 'outer;
            }
        }
    }
    let (a, b) = air_pair.expect("Need two adjacent air voxels near trunk");

    // Designate first voxel.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![a],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Try to designate both voxels (a overlaps, b is new).
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![a, b],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Partial overlap should reject entire designation"
    );
}

#[test]
fn non_overlapping_builds_both_succeed() {
    let mut sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();

    // Find two separate air voxels next to trunk.
    let mut air_voxels = Vec::new();
    for &trunk_coord in &tree.trunk_voxels {
        for &(dx, dz) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let neighbor = VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y, trunk_coord.z + dz);
            if sim.world.in_bounds(neighbor) && sim.world.get(neighbor) == VoxelType::Air {
                air_voxels.push(neighbor);
                if air_voxels.len() >= 2 {
                    break;
                }
            }
        }
        if air_voxels.len() >= 2 {
            break;
        }
    }
    assert!(air_voxels.len() >= 2, "Need two air voxels near trunk");
    let (a, b) = (air_voxels[0], air_voxels[1]);
    assert_ne!(a, b);

    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![a],
            priority: Priority::Normal,
        },
    };
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![b],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1, cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        2,
        "Non-overlapping builds should both succeed"
    );
}

#[test]
fn completed_blueprint_does_not_block_carve() {
    // Completed blueprints should not trigger the overlap check — only
    // Designated blueprints are in the overlay.
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 1;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Spawn an elf and designate a platform.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z + 3);
    let spawn_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: elf_pos,
        },
    };
    let build_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[spawn_cmd, build_cmd], 2);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Let the build complete.
    sim.step(&[], 500_000);
    assert_eq!(
        sim.world.get(air_coord),
        VoxelType::GrownPlatform,
        "Platform should be materialized"
    );

    // Now carve the completed platform — should succeed since the
    // blueprint is Complete, not Designated.
    let carve_cmd = SimCommand {
        player_name: String::new(),
        tick: 500_001,
        action: SimAction::DesignateCarve {
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[carve_cmd], 500_001);
    // Should have 2 blueprints now (the completed build + the new carve).
    assert_eq!(
        sim.db.blueprints.len(),
        2,
        "Carve over completed build should succeed; msg: {:?}",
        sim.last_build_message
    );
}

// =======================================================================
// Furnishing tests
// =======================================================================

#[test]
fn compute_furniture_positions_3x3_dormitory() {
    let mut rng = GameRng::new(42);
    let structure = CompletedStructure {
        id: StructureId(0),
        project_id: ProjectId::new(&mut rng),
        build_type: BuildType::Building,
        anchor: VoxelCoord::new(0, 0, 0),
        width: 3,
        depth: 3,
        height: 1,
        completed_tick: 0,
        name: None,
        furnishing: None,
        inventory_id: InventoryId(0),
        logistics_priority: None,
        crafting_enabled: false,
        greenhouse_species: None,
        greenhouse_enabled: false,
        greenhouse_last_production_tick: 0,
        last_dance_completed_tick: 0,
        last_dinner_party_completed_tick: 0,
    };

    let items = structure.compute_furniture_positions(FurnishingType::Dormitory, &mut rng);

    // 3x3 = 9 floor tiles, target ~4 items. Door at (1,0,2) and its
    // neighbors are excluded, so fewer eligible positions.
    assert!(!items.is_empty());
    // All items should be at y=0 (anchor.y, the interior level).
    for item in &items {
        assert_eq!(item.y, 0);
    }
    // No item should be at the door position.
    let door = VoxelCoord::new(1, 0, 2);
    assert!(!items.contains(&door));
}

#[test]
fn compute_furniture_positions_5x5_dormitory() {
    let mut rng = GameRng::new(42);
    let structure = CompletedStructure {
        id: StructureId(0),
        project_id: ProjectId::new(&mut rng),
        build_type: BuildType::Building,
        anchor: VoxelCoord::new(0, 0, 0),
        width: 5,
        depth: 5,
        height: 1,
        completed_tick: 0,
        name: None,
        furnishing: None,
        inventory_id: InventoryId(0),
        logistics_priority: None,
        crafting_enabled: false,
        greenhouse_species: None,
        greenhouse_enabled: false,
        greenhouse_last_production_tick: 0,
        last_dance_completed_tick: 0,
        last_dinner_party_completed_tick: 0,
    };

    let items = structure.compute_furniture_positions(FurnishingType::Dormitory, &mut rng);

    // 5x5 = 25 floor tiles, target ~12 items (1 per 2 tiles).
    assert!(
        items.len() >= 8,
        "Expected at least 8 items for 5x5 dormitory, got {}",
        items.len()
    );
    assert!(
        items.len() <= 12,
        "Expected at most 12 items for 5x5 dormitory, got {}",
        items.len()
    );
}

#[test]
fn display_name_dormitory_when_furnished() {
    let mut rng = GameRng::new(42);
    let structure = CompletedStructure {
        id: StructureId(7),
        project_id: ProjectId::new(&mut rng),
        build_type: BuildType::Building,
        anchor: VoxelCoord::new(0, 0, 0),
        width: 3,
        depth: 3,
        height: 1,
        completed_tick: 0,
        name: None,
        furnishing: Some(FurnishingType::Dormitory),
        inventory_id: InventoryId(0),
        logistics_priority: None,
        crafting_enabled: false,
        greenhouse_species: None,
        greenhouse_enabled: false,
        greenhouse_last_production_tick: 0,
        last_dance_completed_tick: 0,
        last_dinner_party_completed_tick: 0,
    };

    assert_eq!(structure.display_name(), "Dormitory #7");
}

#[test]
fn display_name_custom_overrides_dormitory() {
    let mut rng = GameRng::new(42);
    let structure = CompletedStructure {
        id: StructureId(7),
        project_id: ProjectId::new(&mut rng),
        build_type: BuildType::Building,
        anchor: VoxelCoord::new(0, 0, 0),
        width: 3,
        depth: 3,
        height: 1,
        completed_tick: 0,
        name: Some("Starlight Hall".to_string()),
        furnishing: Some(FurnishingType::Dormitory),
        inventory_id: InventoryId(0),
        logistics_priority: None,
        crafting_enabled: false,
        greenhouse_species: None,
        greenhouse_enabled: false,
        greenhouse_last_production_tick: 0,
        last_dance_completed_tick: 0,
        last_dinner_party_completed_tick: 0,
    };

    assert_eq!(structure.display_name(), "Starlight Hall");
}

#[test]
fn furnish_structure_creates_task() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Dormitory,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Should have created a Furnish task.
    let furnish_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Furnish)
        .collect();
    assert_eq!(furnish_tasks.len(), 1);
    let task = furnish_tasks[0];
    assert_eq!(task.state, TaskState::Available);
    assert_eq!(task.required_species, Some(Species::Elf));

    // Structure should have furnishing set and planned furniture computed.
    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert_eq!(structure.furnishing, Some(FurnishingType::Dormitory));
    let planned = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|f| !f.placed)
        .count();
    let placed = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|f| f.placed)
        .count();
    assert!(planned > 0);
    assert_eq!(placed, 0);

    // Total cost should be planned count (number of items = number of actions).
    let expected_cost = planned as i64;
    assert_eq!(task.total_cost, expected_cost);
}

#[test]
fn furnish_structure_display_name_changes() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Before furnishing: "Building #N"
    assert_eq!(
        sim.db.structures.get(&structure_id).unwrap().display_name(),
        format!("Building #{}", structure_id.0)
    );

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Dormitory,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // After furnishing: "Dormitory #N"
    assert_eq!(
        sim.db.structures.get(&structure_id).unwrap().display_name(),
        format!("Dormitory #{}", structure_id.0)
    );
}

#[test]
fn furnish_preserves_custom_name() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Give it a custom name first.
    let rename_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::RenameStructure {
            structure_id,
            name: Some("Starlight Hall".to_string()),
        },
    };
    sim.step(&[rename_cmd], sim.tick + 1);
    assert_eq!(
        sim.db.structures.get(&structure_id).unwrap().display_name(),
        "Starlight Hall"
    );

    // Furnish as dormitory — custom name should be preserved.
    let furnish_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Dormitory,
            greenhouse_species: None,
        },
    };
    sim.step(&[furnish_cmd], sim.tick + 1);

    assert_eq!(
        sim.db.structures.get(&structure_id).unwrap().furnishing,
        Some(FurnishingType::Dormitory)
    );
    assert_eq!(
        sim.db.structures.get(&structure_id).unwrap().display_name(),
        "Starlight Hall"
    );
}

#[test]
fn furnish_rejects_non_building() {
    let mut sim = test_sim(legacy_test_seed());

    // Insert a platform structure (not a Building).
    let id = StructureId(sim.next_structure_id);
    sim.next_structure_id += 1;
    let mut rng = GameRng::new(99);
    let project_id = ProjectId::new(&mut rng);
    insert_stub_blueprint(&mut sim, project_id);
    let structure = CompletedStructure {
        id,
        project_id,
        build_type: BuildType::Platform,
        anchor: VoxelCoord::new(10, 5, 10),
        width: 3,
        depth: 3,
        height: 1,
        completed_tick: 0,
        name: None,
        furnishing: None,
        inventory_id: sim.create_inventory(crate::db::InventoryOwnerKind::Structure),
        logistics_priority: None,
        crafting_enabled: false,
        greenhouse_species: None,
        greenhouse_enabled: false,
        greenhouse_last_production_tick: 0,
        last_dance_completed_tick: 0,
        last_dinner_party_completed_tick: 0,
    };
    sim.db.insert_structure(structure).unwrap();

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id: id,
            furnishing_type: FurnishingType::Dormitory,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Should NOT have created a task or set furnishing.
    assert!(
        sim.db
            .tasks
            .iter_all()
            .all(|t| t.kind_tag != TaskKindTag::Furnish)
    );
    assert_eq!(sim.db.structures.get(&id).unwrap().furnishing, None);
}

#[test]
fn furnish_rejects_already_furnished() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Furnish once.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Dormitory,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);
    assert_eq!(
        sim.db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Furnish)
            .count(),
        1
    );

    // Try to furnish again.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Dormitory,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd2], sim.tick + 1);

    // Should still have exactly one Furnish task (second was rejected).
    assert_eq!(
        sim.db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Furnish)
            .count(),
        1
    );
}

#[test]
fn do_furnish_work_places_items_incrementally() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Furnish the building.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Dormitory,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let planned_count = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|f| !f.placed)
        .count();
    assert!(planned_count > 0);

    // Spawn an elf near the building so it can claim the task.
    let spawn_pos = VoxelCoord::new(anchor.x, anchor.y + 1, anchor.z + 3);
    let spawn_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: spawn_pos,
        },
    };
    sim.step(&[spawn_cmd], sim.tick + 1);

    // Run the sim long enough for the elf to walk there and place at
    // least one item. furnish_work_ticks_per_item = 2000, so after ~3000
    // ticks (walk + first item), we should see progress.
    let ticks_per_item = sim.config.furnish_work_ticks_per_item;
    let advance_ticks = ticks_per_item * 3 + 5000; // generous for walking
    sim.step(&[], sim.tick + advance_ticks);

    let placed_count = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|f| f.placed)
        .count();
    assert!(
        placed_count > 0,
        "Expected at least one item placed after {} ticks, got 0. planned={}",
        advance_ticks,
        planned_count
    );
}

#[test]
fn furnish_completes_all_items() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Furnish the building.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Dormitory,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let all_furniture = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    let total_planned = all_furniture.len();
    assert!(total_planned > 0);

    // Spawn an elf near the building.
    let spawn_pos = VoxelCoord::new(anchor.x, anchor.y + 1, anchor.z + 3);
    let spawn_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: spawn_pos,
        },
    };
    sim.step(&[spawn_cmd], sim.tick + 1);

    // Run long enough for all items to be placed.
    let ticks_per_item = sim.config.furnish_work_ticks_per_item;
    let advance_ticks = ticks_per_item * (total_planned as u64 + 2) + 10000;
    sim.step(&[], sim.tick + advance_ticks);

    let placed_count = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|f| f.placed)
        .count();
    let unplaced_count = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|f| !f.placed)
        .count();
    assert_eq!(
        placed_count, total_planned,
        "Expected all {} items placed, got {}",
        total_planned, placed_count
    );
    assert_eq!(
        unplaced_count, 0,
        "Expected no unplaced furniture after completion"
    );

    // The Furnish task should be complete.
    let furnish_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Furnish)
        .collect();
    assert!(
        furnish_tasks.is_empty() || furnish_tasks.iter().all(|t| t.state == TaskState::Complete),
        "Furnish task should be Complete"
    );
}

#[test]
fn furnish_serialization_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Furnish the building.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Dormitory,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Serialize and restore.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let orig = sim.db.structures.get(&structure_id).unwrap();
    let rest = restored.db.structures.get(&structure_id).unwrap();
    assert_eq!(orig.furnishing, rest.furnishing);
    // Check furniture rows survived serialization.
    let orig_furn: Vec<_> = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|f| (f.coord, f.placed))
        .collect();
    let rest_furn: Vec<_> = restored
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .map(|f| (f.coord, f.placed))
        .collect();
    assert_eq!(orig_furn, rest_furn);
}

#[test]
fn compute_furniture_positions_home_single_item() {
    let mut rng = GameRng::new(42);
    let structure = CompletedStructure {
        id: StructureId(0),
        project_id: ProjectId::new(&mut rng),
        build_type: BuildType::Building,
        anchor: VoxelCoord::new(0, 0, 0),
        width: 5,
        depth: 5,
        height: 1,
        completed_tick: 0,
        name: None,
        furnishing: None,
        inventory_id: InventoryId(0),
        logistics_priority: None,
        crafting_enabled: false,
        greenhouse_species: None,
        greenhouse_enabled: false,
        greenhouse_last_production_tick: 0,
        last_dance_completed_tick: 0,
        last_dinner_party_completed_tick: 0,
    };

    let items = structure.compute_furniture_positions(FurnishingType::Home, &mut rng);

    // Home always produces exactly 1 item regardless of building size.
    assert_eq!(items.len(), 1, "Home should produce exactly 1 item");
    assert_eq!(items[0].y, 0);
}

#[test]
fn compute_furniture_positions_dining_hall_density() {
    let mut rng = GameRng::new(42);
    let structure = CompletedStructure {
        id: StructureId(0),
        project_id: ProjectId::new(&mut rng),
        build_type: BuildType::Building,
        anchor: VoxelCoord::new(0, 0, 0),
        width: 5,
        depth: 5,
        height: 1,
        completed_tick: 0,
        name: None,
        furnishing: None,
        inventory_id: InventoryId(0),
        logistics_priority: None,
        crafting_enabled: false,
        greenhouse_species: None,
        greenhouse_enabled: false,
        greenhouse_last_production_tick: 0,
        last_dance_completed_tick: 0,
        last_dinner_party_completed_tick: 0,
    };

    let items = structure.compute_furniture_positions(FurnishingType::DiningHall, &mut rng);

    // 5x5 = 25 tiles, 1 per 4 = ~6 tables. Should be fewer than dormitory.
    assert!(
        items.len() >= 3,
        "Expected at least 3 tables for 5x5 dining hall, got {}",
        items.len()
    );
    assert!(
        items.len() <= 6,
        "Expected at most 6 tables for 5x5 dining hall, got {}",
        items.len()
    );
}

#[test]
fn display_name_all_furnishing_types() {
    let mut rng = GameRng::new(42);
    let types_and_names = [
        (FurnishingType::ConcertHall, "Concert Hall #0"),
        (FurnishingType::DanceHall, "Dance Hall #0"),
        (FurnishingType::DiningHall, "Dining Hall #0"),
        (FurnishingType::Dormitory, "Dormitory #0"),
        (FurnishingType::Home, "Home #0"),
        (FurnishingType::Kitchen, "Kitchen #0"),
        (FurnishingType::Storehouse, "Storehouse #0"),
        (FurnishingType::Workshop, "Workshop #0"),
    ];
    for (furnishing_type, expected) in types_and_names {
        let structure = CompletedStructure {
            id: StructureId(0),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Building,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 3,
            depth: 3,
            height: 1,
            completed_tick: 0,
            name: None,
            furnishing: Some(furnishing_type),
            inventory_id: InventoryId(0),
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        };
        assert_eq!(
            structure.display_name(),
            expected,
            "display_name() for {:?}",
            furnishing_type
        );
    }
}

#[test]
fn furnish_structure_workshop() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Workshop,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert_eq!(structure.furnishing, Some(FurnishingType::Workshop));
    let planned_furn = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|f| !f.placed)
        .count();
    assert!(planned_furn > 0);
    assert_eq!(
        structure.display_name(),
        format!("Workshop #{}", structure_id.0)
    );
}

// =======================================================================
// Dance hall tests
// =======================================================================

#[test]
fn furnish_dance_hall_no_furniture_no_task() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::DanceHall,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Should have furnishing set.
    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert_eq!(structure.furnishing, Some(FurnishingType::DanceHall));

    // Should have zero furniture rows.
    let furniture_count = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .len();
    assert_eq!(furniture_count, 0, "Dance halls should have no furniture");

    // Should have created no Furnish task.
    let furnish_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Furnish)
        .collect();
    assert!(
        furnish_tasks.is_empty(),
        "Dance halls should not create a Furnish task"
    );

    // Display name should use "Dance Hall".
    assert_eq!(
        structure.display_name(),
        format!("Dance Hall #{}", structure_id.0)
    );
}

#[test]
fn dance_hall_display_str() {
    assert_eq!(FurnishingType::DanceHall.display_str(), "Dance Hall");
}

// =======================================================================
// Greenhouse tests
// =======================================================================

// =======================================================================
// Raycast solid tests
// =======================================================================

#[test]
fn raycast_solid_finds_solid_voxel() {
    let mut sim = test_sim(legacy_test_seed());
    // Place a known solid voxel and cast a ray at it.
    let target = VoxelCoord::new(5, 5, 5);
    sim.world.set(target, VoxelType::Trunk);
    let from = [5.5, 10.0, 5.5];
    let dir = [0.0, -1.0, 0.0];
    let result = sim.raycast_solid(from, dir, 100, None, None);
    assert!(result.is_some(), "Should hit the trunk voxel");
    let (coord, face) = result.unwrap();
    assert_eq!(coord, target);
    assert_eq!(face, 2, "Should enter through PosY face (from above)");
}

#[test]
fn raycast_solid_returns_correct_face() {
    let mut sim = test_sim(legacy_test_seed());
    // Place a solid voxel in a clear area far from the tree.
    let target = VoxelCoord::new(5, 10, 5);
    sim.world.set(target, VoxelType::Trunk);

    // Ray from above -> enters through PosY face (index 2).
    let from_above = [5.5, 15.0, 5.5];
    let dir_down = [0.0, -1.0, 0.0];
    let (coord, face) = sim
        .raycast_solid(from_above, dir_down, 100, None, None)
        .unwrap();
    assert_eq!(coord, target);
    assert_eq!(face, 2, "Ray from above should enter through PosY face");

    // Ray from +X side -> enters through PosX face (index 0).
    let from_east = [10.5, 10.5, 5.5];
    let dir_west = [-1.0, 0.0, 0.0];
    let (coord, face) = sim
        .raycast_solid(from_east, dir_west, 100, None, None)
        .unwrap();
    assert_eq!(coord, target);
    assert_eq!(face, 0, "Ray from +X should enter through PosX face");

    // Ray from +Z side -> enters through PosZ face (index 4).
    let from_south = [5.5, 10.5, 10.5];
    let dir_north = [0.0, 0.0, -1.0];
    let (coord, face) = sim
        .raycast_solid(from_south, dir_north, 100, None, None)
        .unwrap();
    assert_eq!(coord, target);
    assert_eq!(face, 4, "Ray from +Z should enter through PosZ face");
}

#[test]
fn raycast_solid_returns_none_for_empty_ray() {
    let sim = test_sim(legacy_test_seed());
    // Cast a ray straight up from the top of the world — should hit nothing.
    let from = [5.5, 50.0, 5.5];
    let dir = [0.0, 1.0, 0.0];
    let result = sim.raycast_solid(from, dir, 100, None, None);
    assert_eq!(result, None);
}

#[test]
fn raycast_solid_negative_face_directions() {
    let mut sim = test_sim(legacy_test_seed());
    let target = VoxelCoord::new(5, 10, 5);
    sim.world.set(target, VoxelType::Trunk);

    // Ray from -X side -> enters through NegX face (index 1).
    let from_west = [0.5, 10.5, 5.5];
    let dir_east = [1.0, 0.0, 0.0];
    let (coord, face) = sim
        .raycast_solid(from_west, dir_east, 100, None, None)
        .unwrap();
    assert_eq!(coord, target);
    assert_eq!(face, 1, "Ray from -X should enter through NegX face");

    // Ray from -Z side -> enters through NegZ face (index 5).
    let from_north = [5.5, 10.5, 0.5];
    let dir_south = [0.0, 0.0, 1.0];
    let (coord, face) = sim
        .raycast_solid(from_north, dir_south, 100, None, None)
        .unwrap();
    assert_eq!(coord, target);
    assert_eq!(face, 5, "Ray from -Z should enter through NegZ face");
}

#[test]
fn raycast_solid_skips_starting_voxel() {
    let mut sim = test_sim(legacy_test_seed());
    // Place two solid voxels adjacent vertically.
    sim.world.set(VoxelCoord::new(5, 10, 5), VoxelType::Trunk);
    sim.world.set(VoxelCoord::new(5, 11, 5), VoxelType::Trunk);
    // Start inside the upper voxel, cast downward — should skip
    // the starting voxel and hit the lower one.
    let from = [5.5, 11.5, 5.5];
    let dir = [0.0, -1.0, 0.0];
    let result = sim.raycast_solid(from, dir, 100, None, None);
    assert!(result.is_some());
    let (coord, _face) = result.unwrap();
    assert_eq!(
        coord,
        VoxelCoord::new(5, 10, 5),
        "Should skip starting voxel"
    );
}

#[test]
fn raycast_solid_hits_blueprint_with_overlay() {
    let mut sim = test_sim(legacy_test_seed());
    // Find an air voxel adjacent to trunk (valid for platform placement).
    let target = find_air_adjacent_to_trunk(&sim);

    // Without overlay, ray passes through (it's air).
    let from = [
        target.x as f32 + 0.5,
        target.y as f32 + 5.0,
        target.z as f32 + 0.5,
    ];
    let dir = [0.0, -1.0, 0.0];
    let result_no_overlay = sim.raycast_solid(from, dir, 20, None, None);
    // Ray might hit something else (e.g., floor below), but not the target.
    assert!(
        result_no_overlay.is_none() || result_no_overlay.unwrap().0 != target,
        "Without overlay, ray should not hit the air voxel as solid"
    );

    // Designate a platform blueprint at the target.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![target],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // With overlay, ray hits the blueprint voxel.
    let overlay = sim.blueprint_overlay();
    let result_with_overlay = sim.raycast_solid(from, dir, 20, Some(&overlay), None);
    assert!(
        result_with_overlay.is_some(),
        "With overlay, ray should hit the blueprint voxel"
    );
    let (coord, face) = result_with_overlay.unwrap();
    assert_eq!(coord, target);
    assert_eq!(face, 2, "Should enter through PosY face (from above)");
}

#[test]
fn raycast_solid_skips_above_y_cutoff() {
    let mut sim = test_sim(legacy_test_seed());
    let target = VoxelCoord::new(5, 10, 5);
    sim.world.set(target, VoxelType::Trunk);

    // Sanity: without y_cutoff, we hit the voxel.
    let from = [5.5, 15.0, 5.5];
    let dir = [0.0, -1.0, 0.0];
    let result = sim.raycast_solid(from, dir, 100, None, None);
    assert!(result.is_some(), "Should hit without y_cutoff");
    assert_eq!(result.unwrap().0, target);

    // With y_cutoff at the voxel's Y, the trunk is treated as air.
    // The ray passes through it and hits the terrain at y=0 instead.
    let result = sim.raycast_solid(from, dir, 100, None, Some(10));
    assert!(
        result.is_none() || result.unwrap().0.y < 10,
        "Trunk at y=10 should be skipped with y_cutoff=10, got {result:?}"
    );
}

#[test]
fn raycast_solid_hits_below_y_cutoff() {
    let mut sim = test_sim(legacy_test_seed());
    // Place two solid voxels stacked vertically.
    let upper = VoxelCoord::new(5, 11, 5);
    let lower = VoxelCoord::new(5, 10, 5);
    sim.world.set(upper, VoxelType::Trunk);
    sim.world.set(lower, VoxelType::Trunk);

    let from = [5.5, 15.0, 5.5];
    let dir = [0.0, -1.0, 0.0];

    // y_cutoff=11 skips the upper voxel but hits the lower one.
    let result = sim.raycast_solid(from, dir, 100, None, Some(11));
    assert!(result.is_some(), "Should hit the lower voxel");
    let (coord, face) = result.unwrap();
    assert_eq!(
        coord, lower,
        "Should skip upper (y=11) and hit lower (y=10)"
    );
    assert_eq!(face, 2, "Should enter through PosY face (from above)");
}

// =======================================================================
// auto_ladder_orientation tests
// =======================================================================

#[test]
fn auto_ladder_orientation_faces_trunk() {
    let mut sim = test_sim(legacy_test_seed());
    // Place a trunk column at (5, 10..14, 5) and test ladder at (6, 10, 5).
    // Use an elevated position far from the real tree to avoid interference.
    for y in 10..14 {
        sim.world.set(VoxelCoord::new(5, y, 5), VoxelType::Trunk);
    }
    // Clear all neighbors around the ladder column to ensure only the
    // trunk at x=5 is adjacent.
    for y in 10..14 {
        sim.world.set(VoxelCoord::new(6, y, 5), VoxelType::Air);
        sim.world.set(VoxelCoord::new(7, y, 5), VoxelType::Air);
        sim.world.set(VoxelCoord::new(6, y, 4), VoxelType::Air);
        sim.world.set(VoxelCoord::new(6, y, 6), VoxelType::Air);
    }

    let face = sim.auto_ladder_orientation(6, 10, 5, 4);
    // Trunk is to the west (-X) of the ladder, so the ladder should face
    // NegX (face 1).
    assert_eq!(face, 1, "Ladder should face the trunk (NegX)");
}

#[test]
fn auto_ladder_orientation_tie_breaks_to_first() {
    let mut sim = test_sim(legacy_test_seed());
    // Place solid voxels on both +X and -X sides of the ladder column,
    // creating a tie. The code iterates [PosX, PosZ, NegX, NegZ], so
    // PosX (face 0) should win.
    for y in 10..14 {
        sim.world.set(VoxelCoord::new(4, y, 5), VoxelType::Trunk); // -X
        sim.world.set(VoxelCoord::new(6, y, 5), VoxelType::Trunk); // +X
        // Clear other neighbors.
        sim.world.set(VoxelCoord::new(5, y, 4), VoxelType::Air);
        sim.world.set(VoxelCoord::new(5, y, 6), VoxelType::Air);
    }
    let face = sim.auto_ladder_orientation(5, 10, 5, 4);
    assert_eq!(
        face, 0,
        "Tie should break to PosX (first in iteration order)"
    );
}

#[test]
fn auto_ladder_orientation_no_neighbors_defaults_east() {
    let mut sim = test_sim(legacy_test_seed());
    // Clear all neighbors around the ladder column — no solid voxels.
    for y in 10..14 {
        sim.world.set(VoxelCoord::new(5, y, 5), VoxelType::Air);
        sim.world.set(VoxelCoord::new(4, y, 5), VoxelType::Air);
        sim.world.set(VoxelCoord::new(6, y, 5), VoxelType::Air);
        sim.world.set(VoxelCoord::new(5, y, 4), VoxelType::Air);
        sim.world.set(VoxelCoord::new(5, y, 6), VoxelType::Air);
    }
    let face = sim.auto_ladder_orientation(5, 10, 5, 4);
    // All counts are 0, so first direction (PosX, face 0) wins.
    assert_eq!(face, 0, "No neighbors should default to PosX (East)");
}

// =======================================================================
// CompletedStructure serde backward compat
// =======================================================================

#[test]
fn completed_structure_serde_backward_compat_crafting() {
    // Old JSON without crafting_enabled field should deserialize with default.
    let mut rng = GameRng::new(42);
    let structure = CompletedStructure {
        id: StructureId(1),
        project_id: ProjectId::new(&mut rng),
        build_type: BuildType::Building,
        anchor: VoxelCoord::new(0, 0, 0),
        width: 5,
        depth: 5,
        height: 3,
        completed_tick: 100,
        name: None,
        furnishing: None,
        inventory_id: InventoryId(0),
        logistics_priority: None,
        crafting_enabled: false,
        greenhouse_species: None,
        greenhouse_enabled: false,
        greenhouse_last_production_tick: 0,
        last_dance_completed_tick: 0,
        last_dinner_party_completed_tick: 0,
    };
    let json = serde_json::to_string(&structure).unwrap();
    // Remove crafting_enabled to simulate old save.
    let json_old = json.replace(r#","crafting_enabled":false"#, "");
    let restored: CompletedStructure = serde_json::from_str(&json_old).unwrap();
    assert!(!restored.crafting_enabled);
}

// =======================================================================
// Workshop furnishing defaults
// =======================================================================

#[test]
fn furnish_workshop_sets_defaults() {
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Workshop,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert!(
        structure.crafting_enabled,
        "Workshop should have crafting_enabled"
    );
    assert_eq!(
        structure.logistics_priority,
        Some(sim.config.workshop_default_priority),
        "Workshop should have default priority"
    );

    // Auto-add on furnish was removed (F-recipe-params). Workshops start
    // with no active recipes; the player adds them manually.
    let active_recipes = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    assert_eq!(
        active_recipes.len(),
        0,
        "Workshop should have no active recipes after furnishing"
    );
}

// =======================================================================
// Blueprint overlay tests (B-preview-blueprints)
// =======================================================================

#[test]
fn blueprint_overlay_includes_designated_blueprints() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Designate a platform build.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    let overlay = sim.blueprint_overlay();
    assert_eq!(
        overlay.voxels.get(&air_coord),
        Some(&VoxelType::GrownPlatform),
        "Designated platform blueprint should appear in overlay as GrownPlatform"
    );
}

#[test]
fn blueprint_overlay_excludes_complete_blueprints() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Designate and then manually mark as complete.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    // Manually flip the blueprint to Complete.
    let mut bp = sim.db.blueprints.iter_all().next().unwrap().clone();
    bp.state = BlueprintState::Complete;
    sim.db.update_blueprint(bp).unwrap();

    let overlay = sim.blueprint_overlay();
    assert!(
        overlay.voxels.is_empty(),
        "Complete blueprints should not appear in overlay"
    );
}

#[test]
fn blueprint_overlay_maps_carve_to_air() {
    let mut sim = test_sim(legacy_test_seed());

    // Find a solid carvable voxel from the tree's trunk.
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let carve_coord = *tree
        .trunk_voxels
        .iter()
        .find(|&&c| {
            let vt = sim.world.get(c);
            vt.is_solid() && c.y > 0
        })
        .expect("Need a carvable trunk voxel");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![carve_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    let overlay = sim.blueprint_overlay();
    assert_eq!(
        overlay.voxels.get(&carve_coord),
        Some(&VoxelType::Air),
        "Carve blueprint should appear in overlay as Air"
    );
}

#[test]
fn second_platform_blocked_by_existing_blueprint() {
    // Designating a platform on the same voxel as an existing blueprint
    // should fail because the overlay makes the voxel appear as
    // GrownPlatform (Blocked for overlap).
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // First designation succeeds.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Second designation on the same voxel should be rejected.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    // Still only one blueprint — second was rejected.
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Second platform on same voxel should be rejected"
    );
    assert!(
        sim.last_build_message.is_some(),
        "Should have a rejection message"
    );
}

#[test]
fn adjacent_platform_sees_blueprint_support() {
    // Place platforms in a chain extending from the trunk. Designate the
    // first N-1 as a single blueprint, then designate the last one
    // separately — it's only adjacent to the blueprint, not to any solid
    // in the real world, so it exercises the overlay adjacency check.
    let mut sim = test_sim(legacy_test_seed());

    // Search across trunk voxels and all 4 horizontal directions for a
    // strip of air that eventually leaves all solid face neighbors behind.
    let directions: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let mut best: Option<(Vec<VoxelCoord>, usize)> = None; // (strip, split_index)
    'outer: for &trunk_coord in &tree.trunk_voxels {
        for &(dx, dz) in &directions {
            let mut strip = Vec::new();
            for i in 1..=20_i32 {
                let c = VoxelCoord::new(
                    trunk_coord.x + dx * i,
                    trunk_coord.y,
                    trunk_coord.z + dz * i,
                );
                if !sim.world.in_bounds(c) || sim.world.get(c) != VoxelType::Air {
                    break;
                }
                strip.push(c);
            }
            if strip.len() < 2 {
                continue;
            }
            if let Some(split) = strip
                .iter()
                .position(|&c| !sim.world.has_solid_face_neighbor(c))
                && split > 0
            {
                best = Some((strip, split));
                break 'outer;
            }
        }
    }
    let (strip, split) = best.expect(
        "Need a trunk voxel with an air strip that transitions from solid-neighbor to open",
    );

    let first_batch = &strip[..split];
    let extension = strip[split];

    // Designate the first batch (adjacent to trunk).
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: first_batch.to_vec(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Designate the extension. Without the overlay it would fail
    // adjacency; with the overlay the blueprint batch acts as solid.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![extension],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        2,
        "Platform adjacent to blueprint should be accepted via overlay support"
    );
}

#[test]
fn overlapping_carve_designations_rejected() {
    // A second carve on the same voxels should be rejected because
    // the overlay maps them to Air (nothing to carve).
    let mut sim = test_sim(legacy_test_seed());

    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    let carve_coord = *tree
        .trunk_voxels
        .iter()
        .find(|&&c| {
            let vt = sim.world.get(c);
            vt.is_solid() && c.y > 0
        })
        .expect("Need a carvable trunk voxel");

    // First carve succeeds.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![carve_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Second carve on same voxel rejected — overlay shows Air.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateCarve {
            voxels: vec![carve_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Second carve on same voxel should be rejected"
    );
    assert!(
        sim.last_build_message.is_some(),
        "Should have a rejection message"
    );
}

#[test]
fn building_foundation_on_designated_platform() {
    // A building placed on a designated platform (not yet built) should
    // see the platform voxels as solid via the overlay.
    let mut sim = test_sim(legacy_test_seed());

    // Find a 3x3 air area adjacent to the trunk at some Y level.
    // Use the building site finder logic but at y=1 where terrain
    // provides the foundation, then place a platform to serve as a
    // higher foundation.
    let site = find_building_site(&sim);
    // site is at y=0 (terrain). Interior starts at y=1.
    // Designate a 3x3 platform at y=1.
    let mut platform_voxels = Vec::new();
    for dx in 0..3 {
        for dz in 0..3 {
            platform_voxels.push(VoxelCoord::new(site.x + dx, 1, site.z + dz));
        }
    }

    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: platform_voxels,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Verify the overlay makes the platform voxels solid.
    let overlay = sim.blueprint_overlay();
    let platform_coord = VoxelCoord::new(site.x, 1, site.z);
    assert_eq!(
        overlay.voxels.get(&platform_coord),
        Some(&VoxelType::GrownPlatform)
    );

    // Now designate a building with foundation at y=1 (the platform).
    // Interior at y=2. Clear any non-air voxels at y=2 so the test
    // always exercises the building-on-blueprint path.
    let building_anchor = VoxelCoord::new(site.x, 1, site.z);
    for dx in 0..3 {
        for dz in 0..3 {
            let coord = VoxelCoord::new(site.x + dx, 2, site.z + dz);
            if sim.world.get(coord) != VoxelType::Air {
                sim.world.set(coord, VoxelType::Air);
            }
        }
    }

    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuilding {
            anchor: building_anchor,
            width: 3,
            depth: 3,
            height: 1,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        2,
        "Building on designated platform should be accepted: {:?}",
        sim.last_build_message
    );
}

#[test]
fn ladder_anchored_to_designated_platform() {
    // A wood ladder placed next to a designated platform should see the
    // platform as solid for anchoring via the overlay.
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Designate a platform.
    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd1], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Find a voxel adjacent to the platform in any horizontal direction
    // that is Air and has no solid face neighbors in the real world, so
    // the ladder can only anchor via the blueprint overlay. Clear any
    // solid neighbors at the ladder voxel if needed to isolate it.
    let directions: [(i32, i32, FaceDirection); 4] = [
        (-1, 0, FaceDirection::PosX),
        (1, 0, FaceDirection::NegX),
        (0, -1, FaceDirection::PosZ),
        (0, 1, FaceDirection::NegZ),
    ];
    let (ladder_coord, orientation) = directions
        .iter()
        .map(|&(dx, dz, dir)| {
            (
                VoxelCoord::new(air_coord.x + dx, air_coord.y, air_coord.z + dz),
                dir,
            )
        })
        .find(|&(coord, _)| sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air)
        .expect("Need an air voxel adjacent to the platform for ladder placement");

    // Clear any solid face neighbors so anchoring can only succeed via overlay.
    for &dir in &FaceDirection::ALL {
        let (dx, dy, dz) = dir.to_offset();
        let neighbor = VoxelCoord::new(
            ladder_coord.x + dx,
            ladder_coord.y + dy,
            ladder_coord.z + dz,
        );
        if neighbor != air_coord && sim.world.get(neighbor).is_solid() {
            sim.world.set(neighbor, VoxelType::Air);
        }
    }
    assert!(
        !sim.world.has_solid_face_neighbor(ladder_coord),
        "Ladder voxel should have no solid face neighbors in the real world"
    );

    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateLadder {
            anchor: ladder_coord,
            height: 1,
            orientation,
            kind: LadderKind::Wood,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        2,
        "Wood ladder anchored to designated platform should be accepted: {:?}",
        sim.last_build_message
    );
}

// =======================================================================
// Strut tests
// =======================================================================

#[test]
fn strut_designate_creates_blueprint_and_strut_row() {
    let mut sim = test_sim(legacy_test_seed());
    let (a, b) = find_strut_endpoints(&sim);
    let line = a.line_to(b);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: line.clone(),
            priority: Priority::Normal,
        },
    };
    let result = sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.build_type, BuildType::Strut);
    assert_eq!(bp.voxels, line);
    assert_eq!(bp.state, BlueprintState::Designated);

    // Strut row created with correct endpoints.
    assert_eq!(sim.db.struts.len(), 1);
    let strut = sim.db.struts.iter_all().next().unwrap();
    assert_eq!(strut.endpoint_a, a);
    assert_eq!(strut.endpoint_b, b);
    assert_eq!(strut.blueprint_id, Some(bp.id));
    assert!(strut.structure_id.is_none());

    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::BlueprintDesignated { .. }))
    );
}

#[test]
fn strut_rejects_single_voxel() {
    let mut sim = test_sim(legacy_test_seed());
    let air = find_air_adjacent_to_trunk(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: vec![air],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 0);
    assert!(
        sim.last_build_message
            .as_ref()
            .unwrap()
            .contains("at least 2")
    );
}

#[test]
fn strut_rejects_invalid_bresenham_list() {
    let mut sim = test_sim(legacy_test_seed());
    let (a, b) = find_strut_endpoints(&sim);

    // Submit a voxel list that doesn't match the line.
    let bogus = vec![a, VoxelCoord::new(a.x + 5, a.y + 5, a.z + 5), b];
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: bogus,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 0);
    assert!(
        sim.last_build_message
            .as_ref()
            .unwrap()
            .contains("Bresenham")
    );
}

#[test]
fn strut_rejects_player_built_structure() {
    let mut sim = test_sim(legacy_test_seed());
    let air = find_air_adjacent_to_trunk(&sim);

    // Place a GrownPlatform at `air`.
    sim.world.set(air, VoxelType::GrownPlatform);

    // Try to designate a strut through the platform.
    let b = VoxelCoord::new(air.x + 1, air.y, air.z);
    if sim.world.in_bounds(b) && sim.world.get(b) == VoxelType::Air {
        let line = air.line_to(b);
        let cmd = SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Strut,
                voxels: line,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        assert_eq!(sim.db.blueprints.len(), 0);
        assert!(
            sim.last_build_message
                .as_ref()
                .unwrap()
                .contains("player-built")
        );
    }
}

#[test]
fn strut_replaces_trunk_records_originals() {
    let mut sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    // Find a trunk voxel with air on both sides (+X).
    let mut found = None;
    for &trunk_coord in &tree.trunk_voxels {
        let before = VoxelCoord::new(trunk_coord.x - 1, trunk_coord.y, trunk_coord.z);
        let after = VoxelCoord::new(trunk_coord.x + 1, trunk_coord.y, trunk_coord.z);
        if sim.world.in_bounds(before)
            && sim.world.in_bounds(after)
            && sim.world.get(before) == VoxelType::Air
            && sim.world.get(after) == VoxelType::Air
        {
            found = Some((before, trunk_coord, after));
            break;
        }
    }
    let (before, trunk, after) = found.expect("Need trunk with air on both sides");

    let line = before.line_to(after);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: line,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    // Original voxels should record the trunk position.
    assert!(
        bp.original_voxels
            .iter()
            .any(|(c, vt)| *c == trunk && *vt == VoxelType::Trunk),
        "Should record trunk in original_voxels"
    );
}

#[test]
fn strut_rejects_no_adjacency() {
    let mut sim = test_sim(legacy_test_seed());
    // Place strut entirely in open air, far from any structure.
    let a = VoxelCoord::new(60, 60, 60);
    let b = VoxelCoord::new(61, 60, 60);
    if sim.world.in_bounds(a)
        && sim.world.in_bounds(b)
        && sim.world.get(a) == VoxelType::Air
        && sim.world.get(b) == VoxelType::Air
    {
        let line = a.line_to(b);
        let cmd = SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Strut,
                voxels: line,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        assert_eq!(sim.db.blueprints.len(), 0);
        assert!(
            sim.last_build_message
                .as_ref()
                .unwrap()
                .contains("adjacent")
        );
    }
}

#[test]
fn strut_blueprint_overlap_rejected() {
    let mut sim = test_sim(legacy_test_seed());
    let (a, b) = find_strut_endpoints(&sim);
    let line = a.line_to(b);

    // Designate first strut.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: line.clone(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Try to designate overlapping strut.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: line,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd2], 2);
    assert_eq!(
        sim.db.blueprints.len(),
        1,
        "Second strut should be rejected"
    );
    assert!(
        sim.last_build_message
            .as_ref()
            .unwrap()
            .contains("Overlaps")
    );
}

#[test]
fn strut_cancel_deletes_strut_row() {
    let mut sim = test_sim(legacy_test_seed());
    let (a, b) = find_strut_endpoints(&sim);
    let line = a.line_to(b);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: line,
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.struts.len(), 1);

    let bp_id = sim.db.blueprints.iter_all().next().unwrap().id;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::CancelBuild { project_id: bp_id },
    };
    sim.step(&[cmd], 2);
    assert_eq!(
        sim.db.struts.len(),
        0,
        "Strut row should be cascade-deleted on cancel"
    );
}

#[test]
fn strut_serde_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let (a, b) = find_strut_endpoints(&sim);
    let line = a.line_to(b);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: line.clone(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    // Serde roundtrip the sim state.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.db.struts.len(), 1);
    let strut = restored.db.struts.iter_all().next().unwrap();
    assert_eq!(strut.endpoint_a, a);
    assert_eq!(strut.endpoint_b, b);

    assert_eq!(restored.db.blueprints.len(), 1);
    let bp = restored.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.build_type, BuildType::Strut);
}

#[test]
fn strut_replaces_replaceable_types() {
    let mut sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();

    // Find a trunk voxel at y > 1 to place air->trunk->air line.
    let trunk_coord = tree
        .trunk_voxels
        .iter()
        .find(|c| {
            let a = VoxelCoord::new(c.x - 1, c.y, c.z);
            let b = VoxelCoord::new(c.x + 1, c.y, c.z);
            sim.world.in_bounds(a)
                && sim.world.in_bounds(b)
                && sim.world.get(a) == VoxelType::Air
                && sim.world.get(b) == VoxelType::Air
        })
        .copied()
        .expect("Need trunk with air on both sides");

    let a = VoxelCoord::new(trunk_coord.x - 1, trunk_coord.y, trunk_coord.z);
    let b = VoxelCoord::new(trunk_coord.x + 1, trunk_coord.y, trunk_coord.z);

    let mut tick = 1u64;
    // Set various replaceable types and verify strut accepts them.
    for replaceable in [
        VoxelType::Leaf,
        VoxelType::Fruit,
        VoxelType::Dirt,
        VoxelType::Root,
        VoxelType::Branch,
    ] {
        sim.world.set(trunk_coord, replaceable);
        let line = a.line_to(b);
        let cmd = SimCommand {
            player_name: String::new(),
            tick,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Strut,
                voxels: line,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], tick);
        tick += 1;
        assert!(
            !sim.db.blueprints.is_empty(),
            "Strut should accept replacing {:?}, msg: {:?}",
            replaceable,
            sim.last_build_message
        );
        // Cancel so we can try the next type.
        let bp_id = sim.db.blueprints.iter_all().next().unwrap().id;
        let cmd = SimCommand {
            player_name: String::new(),
            tick,
            action: SimAction::CancelBuild { project_id: bp_id },
        };
        sim.step(&[cmd], tick);
        tick += 1;
        // Restore the original type for the next iteration.
        sim.world.set(trunk_coord, VoxelType::Trunk);
    }
}

#[test]
fn strut_build_type_to_voxel_type() {
    assert_eq!(BuildType::Strut.to_voxel_type(), VoxelType::Strut);
}

#[test]
fn strut_completed_structure_display_name() {
    let inv_id = InventoryId(0);
    let bp = Blueprint {
        id: ProjectId(crate::types::SimUuid::new_v4(
            &mut crate::prng::GameRng::new(1),
        )),
        build_type: BuildType::Strut,
        voxels: vec![VoxelCoord::new(0, 0, 0), VoxelCoord::new(1, 0, 0)],
        priority: Priority::Normal,
        state: BlueprintState::Complete,
        task_id: None,
        composition_id: None,
        face_layout: None,
        stress_warning: false,
        original_voxels: vec![],
    };
    let structure = CompletedStructure::from_blueprint(StructureId(42), &bp, 100, inv_id);
    assert_eq!(structure.display_name(), "Strut #42");
}

#[test]
fn strut_cancel_restores_original_voxels() {
    // Designating a strut through trunk records the trunk in
    // original_voxels. Manually materializing the strut voxel and
    // cancelling should restore the original trunk type.
    let mut sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();

    // Find a trunk voxel with air on both sides.
    let trunk_coord = tree
        .trunk_voxels
        .iter()
        .find(|c| {
            let a = VoxelCoord::new(c.x - 1, c.y, c.z);
            let b = VoxelCoord::new(c.x + 1, c.y, c.z);
            sim.world.in_bounds(a)
                && sim.world.in_bounds(b)
                && sim.world.get(a) == VoxelType::Air
                && sim.world.get(b) == VoxelType::Air
        })
        .copied()
        .expect("Need trunk with air on both sides");

    let a = VoxelCoord::new(trunk_coord.x - 1, trunk_coord.y, trunk_coord.z);
    let b = VoxelCoord::new(trunk_coord.x + 1, trunk_coord.y, trunk_coord.z);
    let line = a.line_to(b);

    // Designate strut.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: line.clone(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Verify original_voxels tracked the trunk voxel.
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert!(
        bp.original_voxels
            .iter()
            .any(|(c, vt)| *c == trunk_coord && *vt == VoxelType::Trunk),
        "original_voxels should include the trunk coord"
    );

    // Simulate materialization: set the trunk voxel to Strut and add
    // to placed_voxels (as the build task would).
    sim.world.set(trunk_coord, VoxelType::Strut);
    sim.placed_voxels.push((trunk_coord, VoxelType::Strut));
    assert_eq!(sim.world.get(trunk_coord), VoxelType::Strut);

    // Cancel the build.
    let bp_id = sim.db.blueprints.iter_all().next().unwrap().id;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::CancelBuild { project_id: bp_id },
    };
    sim.step(&[cmd], 2);

    // After cancel, the trunk voxel should be restored.
    assert_eq!(
        sim.world.get(trunk_coord),
        VoxelType::Trunk,
        "Cancel should restore trunk voxel from original_voxels"
    );

    // placed_voxels should no longer contain the strut entry.
    assert!(
        !sim.placed_voxels.iter().any(|(c, _)| *c == trunk_coord),
        "placed_voxels should not contain cancelled strut voxel"
    );
}

#[test]
fn strut_materialization_creates_voxels() {
    // Complete a strut build and verify all line voxels become Strut.
    let mut sim = test_sim(legacy_test_seed());
    let (a, b) = find_strut_endpoints(&sim);
    let line = a.line_to(b);

    // Spawn an elf near the build site.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: a,
        },
    };
    sim.step(&[cmd], 1);

    // Designate the strut.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: line.clone(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 2);
    assert_eq!(sim.db.blueprints.len(), 1);

    // Run forward until complete.
    let max_tick = sim.tick + 1_000_000;
    while sim.tick < max_tick {
        sim.step(&[], sim.tick + 100);
        let all_complete = sim
            .db
            .blueprints
            .iter_all()
            .all(|bp| bp.state == BlueprintState::Complete);
        if all_complete {
            break;
        }
    }
    assert!(
        sim.db
            .blueprints
            .iter_all()
            .all(|bp| bp.state == BlueprintState::Complete),
        "Strut build did not complete"
    );

    // All line voxels should be Strut.
    for &coord in &line {
        assert_eq!(
            sim.world.get(coord),
            VoxelType::Strut,
            "Voxel at {:?} should be Strut after completion",
            coord
        );
    }

    // A CompletedStructure should exist.
    assert!(
        !sim.db.structures.is_empty(),
        "Should have a completed structure"
    );

    // The Strut row should have structure_id set.
    let strut = sim.db.struts.iter_all().next().unwrap();
    assert!(
        strut.structure_id.is_some(),
        "Strut row should have structure_id after completion"
    );
}

#[test]
fn strut_save_load_roundtrip_with_completed_strut() {
    // Complete a strut, save, load, and verify the strut row + voxels
    // survive the roundtrip.
    let mut sim = test_sim(legacy_test_seed());
    let (a, b) = find_strut_endpoints(&sim);
    let line = a.line_to(b);

    // Spawn elf and designate.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: a,
        },
    };
    sim.step(&[cmd], 1);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Strut,
            voxels: line.clone(),
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 2);

    // Complete the build.
    let max_tick = sim.tick + 1_000_000;
    while sim.tick < max_tick {
        sim.step(&[], sim.tick + 100);
        if sim
            .db
            .blueprints
            .iter_all()
            .all(|bp| bp.state == BlueprintState::Complete)
        {
            break;
        }
    }

    // Save/load roundtrip.
    let json = serde_json::to_string(&sim).unwrap();
    let mut restored: SimState = serde_json::from_str(&json).unwrap();
    restored.rebuild_transient_state();

    // Verify strut row survived.
    assert_eq!(restored.db.struts.len(), 1);
    let strut = restored.db.struts.iter_all().next().unwrap();
    assert_eq!(strut.endpoint_a, a);
    assert_eq!(strut.endpoint_b, b);
    assert!(strut.structure_id.is_some());

    // Verify voxels survived.
    for &coord in &line {
        assert_eq!(
            restored.world.get(coord),
            VoxelType::Strut,
            "Strut voxel at {:?} should survive save/load",
            coord
        );
    }
}

// =======================================================================
// Dining hall logistics
// =======================================================================

// =======================================================================
// cancel_build edge case
// =======================================================================

#[test]
fn cancel_build_with_no_task_cleans_up() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Insert a blueprint directly with task_id: None.
    let project_id = ProjectId::new(&mut sim.rng);
    sim.db
        .insert_blueprint(crate::db::Blueprint {
            id: project_id,
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: crate::types::Priority::Normal,
            state: crate::blueprint::BlueprintState::Designated,
            task_id: None,
            composition_id: None,
            face_layout: None,
            stress_warning: false,
            original_voxels: Vec::new(),
        })
        .unwrap();

    // Place the voxel as GrownPlatform so cancel_build has something to revert.
    sim.world.set(air_coord, VoxelType::GrownPlatform);
    sim.structure_voxels.insert(air_coord, StructureId(999_999));

    assert!(sim.db.blueprints.contains(&project_id));

    // Cancel should not panic despite task_id being None.
    let mut events = Vec::new();
    sim.cancel_build(project_id, &mut events);

    assert!(
        !sim.db.blueprints.contains(&project_id),
        "Blueprint should be removed after cancel"
    );
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn designate_build_creates_blueprint() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    let result = sim.step(&[cmd], 1);

    assert_eq!(sim.db.blueprints.len(), 1);
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.voxels, vec![air_coord]);
    assert_eq!(bp.state, BlueprintState::Designated);
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::BlueprintDesignated { .. }))
    );
}

#[test]
fn designate_build_creates_composition() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    // Blueprint should have a composition FK.
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    assert!(
        bp.composition_id.is_some(),
        "Build blueprint should have a composition"
    );

    // The composition should exist in the DB with Pending status.
    let comp_id = bp.composition_id.unwrap();
    let comp = sim.db.music_compositions.get(&comp_id).unwrap();
    assert_eq!(comp.status, crate::db::CompositionStatus::Pending);
    assert!(!comp.build_started);
    assert!(comp.seed != 0, "Composition should have a non-trivial seed");
    assert!(comp.sections >= 1 && comp.sections <= 4);
    assert!(comp.mode_index <= 5);
    assert!(comp.brightness >= 0.2 && comp.brightness <= 0.8);
    // 1 voxel × 1000 ticks/voxel = 1000ms target duration.
    assert_eq!(comp.target_duration_ms, 1000);
}

#[test]
fn composition_persists_across_serde_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    let bp = sim.db.blueprints.iter_all().next().unwrap();
    let comp_id = bp.composition_id.unwrap();
    let comp = sim.db.music_compositions.get(&comp_id).unwrap();
    let orig_seed = comp.seed;
    let orig_sections = comp.sections;
    let orig_mode = comp.mode_index;

    // Serialize and deserialize.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    // Composition should survive roundtrip.
    let comp = restored.db.music_compositions.get(&comp_id).unwrap();
    assert_eq!(comp.seed, orig_seed);
    assert_eq!(comp.sections, orig_sections);
    assert_eq!(comp.mode_index, orig_mode);
    assert_eq!(comp.status, crate::db::CompositionStatus::Pending);

    // Blueprint FK should still point to it.
    let bp = restored.db.blueprints.iter_all().next().unwrap();
    assert_eq!(bp.composition_id, Some(comp_id));
}

#[test]
fn designate_carve_has_no_composition() {
    let mut sim = test_sim(legacy_test_seed());

    // Find a solid trunk voxel to carve.
    let mut carve_coord = None;
    for y in 1..sim.world.size_y as i32 {
        for z in 0..sim.world.size_z as i32 {
            for x in 0..sim.world.size_x as i32 {
                let coord = VoxelCoord::new(x, y, z);
                if sim.world.get(coord) == VoxelType::Trunk {
                    carve_coord = Some(coord);
                    break;
                }
            }
            if carve_coord.is_some() {
                break;
            }
        }
        if carve_coord.is_some() {
            break;
        }
    }
    let carve_coord = carve_coord.expect("Should find a trunk voxel to carve");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DesignateCarve {
            voxels: vec![carve_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], 1);

    if !sim.db.blueprints.is_empty() {
        let bp = sim.db.blueprints.iter_all().next().unwrap();
        assert!(
            bp.composition_id.is_none(),
            "Carve blueprint should not have a composition"
        );
    }
    // No compositions should have been created for carving.
    assert_eq!(
        sim.db.music_compositions.len(),
        0,
        "Carving should not create compositions"
    );
}

#[test]
fn build_work_sets_composition_build_started() {
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 50000;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let air_coord = find_air_adjacent_to_trunk(&sim);

    spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Composition should not be started yet (no work done).
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    let comp_id = bp.composition_id.unwrap();
    assert!(
        !sim.db
            .music_compositions
            .get(&comp_id)
            .unwrap()
            .build_started,
        "Composition should not be started before any work"
    );

    // Run enough ticks for the elf to arrive and do at least one tick of work.
    sim.step(&[], sim.tick + 100_000);

    assert!(
        sim.db
            .music_compositions
            .get(&comp_id)
            .unwrap()
            .build_started,
        "Composition should be started after elf begins building"
    );
}
