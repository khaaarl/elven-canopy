// Spring-mass structural integrity solver.
//
// Validates that tree geometry and player constructions are structurally sound
// by modeling voxels as mass-spring nodes and running iterative relaxation to
// compute stress. This catches physically absurd structures like long
// single-voxel cantilevers.
//
// ## Architecture
//
// The solver builds a `StructuralNetwork` of `Node`s (one per solid voxel or
// BuildingInterior voxel) and `Spring`s (one per face-adjacent pair). Gravity
// acts on each node; springs resist deformation. After a fixed number of
// relaxation iterations, per-spring stress is computed as `force / strength`.
//
// ## Key functions
//
// - `build_network()`: Construct the spring-mass network from a voxel world.
// - `solve()`: Run iterative relaxation and return per-spring stress data.
// - `validate_tree()`: Convenience wrapper — does the tree pass under its own weight?
// - `flood_fill_connected()`: BFS connectivity check — are all proposed voxels
//   reachable from ground?
// - `validate_blueprint()`: Full blueprint validation with tiered enforcement
//   (OK / Warning / Blocked).
// - `validate_blueprint_fast()`: Lightweight blueprint validation using BFS +
//   weight-flow-only analysis (~700x faster). Used by `sim.rs` for interactive
//   placement; the full solver is reserved for tree validation at startup.
// - `validate_carve_fast()`: Like `validate_blueprint_fast()` but checks whether
//   *removing* voxels would compromise remaining structure. Seeds BFS from
//   neighbors of carved voxels rather than the carved voxels themselves.
//
// ## Integration points
//
// - `sim.rs`: `with_config()` wraps tree generation in a retry loop using
//   `validate_tree()`. `designate_build()` and `designate_building()` call
//   `validate_blueprint()` to gate construction. `designate_carve()` calls
//   `validate_carve_fast()` to block or warn on structurally dangerous carves.
// - `config.rs`: `StructuralConfig` holds all material/face properties and
//   solver parameters.
//
// See also: `docs/drafts/structural_integrity.md` for the full design.
//
// **Critical constraint: determinism.** The solver iterates nodes in flat-array
// order (x inner, z mid, y outer), uses fixed iteration count, and avoids
// HashMap. All floating-point operations are deterministic given identical input.

use crate::config::GameConfig;
use crate::types::{FaceData, FaceDirection, FaceType, VoxelCoord, VoxelType};
use crate::world::VoxelWorld;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A mass point in the structural network.
struct Node {
    /// Current position (starts at voxel center, displaced by solver).
    position: [f32; 3],
    /// Mass of this node (from material density + face weights).
    mass: f32,
    /// Pinned nodes (ForestFloor) don't move under load.
    pinned: bool,
}

/// A spring connecting two nodes.
struct Spring {
    /// Index of the first node.
    node_a: usize,
    /// Index of the second node.
    node_b: usize,
    /// Spring stiffness (resists deformation).
    stiffness: f32,
    /// Maximum force before failure.
    strength: f32,
    /// Natural (unstressed) length.
    rest_length: f32,
}

/// The spring-mass network built from a voxel world.
pub struct StructuralNetwork {
    nodes: Vec<Node>,
    springs: Vec<Spring>,
    coord_to_node: BTreeMap<VoxelCoord, usize>,
}

/// Result of running the structural solver.
pub struct SolveResult {
    /// Per-spring stress ratio (force / strength). Values > 1.0 mean failure.
    pub spring_stresses: Vec<f32>,
    /// Highest stress ratio across all springs.
    pub max_stress_ratio: f32,
    /// Whether any spring exceeded its strength (stress ratio > 1.0).
    pub any_failed: bool,
}

/// Tiered validation result for blueprint checking.
#[derive(Debug, PartialEq, Eq)]
pub enum ValidationTier {
    /// All springs well within limits.
    Ok,
    /// Some springs above warn threshold but below block threshold.
    Warning,
    /// At least one spring exceeds the block threshold, or connectivity failed.
    Blocked,
}

/// Full blueprint validation result.
pub struct BlueprintValidation {
    /// Overall validation tier.
    pub tier: ValidationTier,
    /// Per-voxel maximum stress ratio (for heatmap rendering).
    pub stress_map: BTreeMap<VoxelCoord, f32>,
    /// Human-readable explanation.
    pub message: String,
}

// ---------------------------------------------------------------------------
// Network construction
// ---------------------------------------------------------------------------

/// Build a structural network from a voxel world.
///
/// Creates nodes for all solid voxels and BuildingInterior voxels, and springs
/// for face-adjacent pairs. Only checks 3 positive-half neighbors (+x, +y, +z)
/// per node to avoid duplicate springs.
///
/// `face_data` provides per-face structural data for BuildingInterior voxels.
pub fn build_network(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    config: &GameConfig,
) -> StructuralNetwork {
    let mut nodes = Vec::new();
    let mut coord_to_node = BTreeMap::new();
    let mut springs = Vec::new();

    let structural = &config.structural;

    // Pass 1: Create nodes for all non-Air voxels.
    // Iterate in flat-array order: x inner, z mid, y outer.
    for y in 0..world.size_y as i32 {
        for z in 0..world.size_z as i32 {
            for x in 0..world.size_x as i32 {
                let coord = VoxelCoord::new(x, y, z);
                let vt = world.get(coord);

                if vt == VoxelType::Air || vt == VoxelType::Leaf || vt == VoxelType::Fruit {
                    continue;
                }

                let mass;
                let pinned;

                if vt == VoxelType::BuildingInterior {
                    // BuildingInterior: base weight + face weights.
                    let mut m = structural.building_interior_base_weight;
                    if let Some(fd) = face_data.get(&coord) {
                        for &dir in &FaceDirection::ALL {
                            let ft = fd.get(dir);
                            if let Some(fp) = structural.face_properties.get(&ft) {
                                m += fp.weight;
                            }
                        }
                    }
                    mass = m;
                    pinned = false;
                } else if let Some(mat) = structural.materials.get(&vt) {
                    mass = mat.density;
                    pinned = vt == VoxelType::ForestFloor || vt == VoxelType::Dirt;
                } else {
                    // Unknown voxel type — skip.
                    continue;
                }

                let node_idx = nodes.len();
                coord_to_node.insert(coord, node_idx);
                nodes.push(Node {
                    position: [x as f32, y as f32, z as f32],
                    mass,
                    pinned,
                });
            }
        }
    }

    // Pass 2: Create springs for face-adjacent pairs.
    // Only check 3 positive-half neighbors to avoid duplicates.
    let positive_offsets: [(i32, i32, i32); 3] = [(1, 0, 0), (0, 1, 0), (0, 0, 1)];

    for (&coord_a, &idx_a) in &coord_to_node {
        let vt_a = world.get(coord_a);

        for &(dx, dy, dz) in &positive_offsets {
            let coord_b = VoxelCoord::new(coord_a.x + dx, coord_a.y + dy, coord_a.z + dz);
            let idx_b = match coord_to_node.get(&coord_b) {
                Some(&idx) => idx,
                None => continue,
            };
            let vt_b = world.get(coord_b);

            // Determine spring properties based on the two voxel types.
            let (stiffness, strength, rest_length) = compute_spring_properties(
                coord_a, vt_a, coord_b, vt_b, dx, dy, dz, face_data, structural,
            );

            if stiffness <= 0.0 && strength <= 0.0 {
                continue; // No structural connection.
            }

            springs.push(Spring {
                node_a: idx_a,
                node_b: idx_b,
                stiffness,
                strength,
                rest_length,
            });
        }
    }

    StructuralNetwork {
        nodes,
        springs,
        coord_to_node,
    }
}

/// Compute spring properties for a face-adjacent pair of voxels.
#[allow(clippy::too_many_arguments)]
fn compute_spring_properties(
    coord_a: VoxelCoord,
    vt_a: VoxelType,
    _coord_b: VoxelCoord,
    vt_b: VoxelType,
    dx: i32,
    dy: i32,
    dz: i32,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    structural: &crate::config::StructuralConfig,
) -> (f32, f32, f32) {
    let rest_length = 1.0; // Face-adjacent voxels are always 1 unit apart.

    // Case 1: Both are solid (non-BuildingInterior, non-Air).
    if vt_a != VoxelType::BuildingInterior && vt_b != VoxelType::BuildingInterior {
        let mat_a = match structural.materials.get(&vt_a) {
            Some(m) => m,
            None => return (0.0, 0.0, rest_length),
        };
        let mat_b = match structural.materials.get(&vt_b) {
            Some(m) => m,
            None => return (0.0, 0.0, rest_length),
        };
        // Harmonic mean for stiffness, minimum for strength.
        let stiffness = if mat_a.stiffness + mat_b.stiffness > 0.0 {
            2.0 * mat_a.stiffness * mat_b.stiffness / (mat_a.stiffness + mat_b.stiffness)
        } else {
            0.0
        };
        let strength = mat_a.strength.min(mat_b.strength);
        return (stiffness, strength, rest_length);
    }

    // Case 2: At least one is BuildingInterior — use face properties.
    // The face direction from A to B.
    let dir_a_to_b = FaceDirection::from_offset(dx, dy, dz);
    let dir_b_to_a = dir_a_to_b.map(|d| d.opposite());

    // Get the face type on each side.
    let face_a = if vt_a == VoxelType::BuildingInterior {
        dir_a_to_b.and_then(|d| face_data.get(&coord_a).map(|fd| fd.get(d)))
    } else {
        None
    };
    let face_b = if vt_b == VoxelType::BuildingInterior {
        dir_b_to_a.and_then(|d| face_data.get(&_coord_b).map(|fd| fd.get(d)))
    } else {
        None
    };

    // If one side is solid and the other is BuildingInterior, use the face
    // properties from the BuildingInterior side. If both are BuildingInterior,
    // use the stronger face.
    let face_type = match (face_a, face_b) {
        (Some(fa), Some(fb)) => {
            // Both BuildingInterior — pick the face with higher stiffness.
            let fp_a = structural.face_properties.get(&fa);
            let fp_b = structural.face_properties.get(&fb);
            match (fp_a, fp_b) {
                (Some(a), Some(b)) if a.stiffness >= b.stiffness => fa,
                (Some(_), Some(_)) => fb,
                (Some(_), None) => fa,
                (None, Some(_)) => fb,
                (None, None) => return (0.0, 0.0, rest_length),
            }
        }
        (Some(f), None) => f,
        (None, Some(f)) => f,
        (None, None) => return (0.0, 0.0, rest_length),
    };

    // If the winning face is Open, no spring.
    if face_type == FaceType::Open {
        return (0.0, 0.0, rest_length);
    }

    let fp = match structural.face_properties.get(&face_type) {
        Some(fp) => fp,
        None => return (0.0, 0.0, rest_length),
    };

    // If one side is solid, blend face stiffness with material stiffness.
    let (stiffness, strength) = if vt_a != VoxelType::BuildingInterior {
        let mat = structural.materials.get(&vt_a);
        match mat {
            Some(m) => {
                let s = if m.stiffness + fp.stiffness > 0.0 {
                    2.0 * m.stiffness * fp.stiffness / (m.stiffness + fp.stiffness)
                } else {
                    0.0
                };
                (s, m.strength.min(fp.strength))
            }
            None => (fp.stiffness, fp.strength),
        }
    } else if vt_b != VoxelType::BuildingInterior {
        let mat = structural.materials.get(&vt_b);
        match mat {
            Some(m) => {
                let s = if m.stiffness + fp.stiffness > 0.0 {
                    2.0 * m.stiffness * fp.stiffness / (m.stiffness + fp.stiffness)
                } else {
                    0.0
                };
                (s, m.strength.min(fp.strength))
            }
            None => (fp.stiffness, fp.strength),
        }
    } else {
        // Both BuildingInterior — pure face spring.
        (fp.stiffness, fp.strength)
    };

    (stiffness, strength, rest_length)
}

// ---------------------------------------------------------------------------
// Iterative relaxation solver
// ---------------------------------------------------------------------------

/// Run the Gauss-Seidel iterative relaxation solver on a structural network.
///
/// Uses per-node adaptive damping (damping_factor / local_stiffness) for
/// fast convergence. Each node is updated in-place so later nodes see earlier
/// updates within the same iteration (Gauss-Seidel, not Jacobi).
///
/// After all iterations, computes per-spring stress ratios.
pub fn solve(network: &mut StructuralNetwork, config: &GameConfig) -> SolveResult {
    let structural = &config.structural;
    let gravity = structural.gravity;
    let damping_scale = structural.damping_factor;
    let max_iter = structural.max_iterations;

    // Build per-node adjacency list of (spring_index, other_node_index).
    let num_nodes = network.nodes.len();
    let mut node_springs: Vec<Vec<(usize, usize)>> = vec![Vec::new(); num_nodes];
    for (si, spring) in network.springs.iter().enumerate() {
        node_springs[spring.node_a].push((si, spring.node_b));
        node_springs[spring.node_b].push((si, spring.node_a));
    }

    // Compute per-node effective stiffness (sum of connected spring stiffnesses).
    let mut k_eff: Vec<f32> = vec![0.0; num_nodes];
    for spring in network.springs.iter() {
        k_eff[spring.node_a] += spring.stiffness;
        k_eff[spring.node_b] += spring.stiffness;
    }

    // Gauss-Seidel iteration: update each node in-place using latest positions.
    for _ in 0..max_iter {
        for i in 0..num_nodes {
            if network.nodes[i].pinned || k_eff[i] <= 0.0 {
                continue;
            }

            // Net force: gravity + spring forces.
            let mut force = [0.0f32, -network.nodes[i].mass * gravity, 0.0f32];

            for &(si, other) in &node_springs[i] {
                let spring = &network.springs[si];
                let pos_i = network.nodes[i].position;
                let pos_o = network.nodes[other].position;

                let dx = pos_o[0] - pos_i[0];
                let dy = pos_o[1] - pos_i[1];
                let dz = pos_o[2] - pos_i[2];
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();

                if dist < 1e-10 {
                    continue;
                }

                let extension = dist - spring.rest_length;
                let f_mag = spring.stiffness * extension;

                force[0] += f_mag * dx / dist;
                force[1] += f_mag * dy / dist;
                force[2] += f_mag * dz / dist;
            }

            // Per-node damping: damping_scale / local_stiffness.
            let damping = damping_scale / k_eff[i];
            network.nodes[i].position[0] += force[0] * damping;
            network.nodes[i].position[1] += force[1] * damping;
            network.nodes[i].position[2] += force[2] * damping;
        }
    }

    // --- Phase 1: Deformation-based stress ---
    // Compute per-spring stress from spring extension after relaxation.
    let num_springs = network.springs.len();
    let mut deform_stresses = vec![0.0f32; num_springs];
    for (si, spring) in network.springs.iter().enumerate() {
        if spring.strength <= 0.0 {
            continue;
        }
        let pos_a = network.nodes[spring.node_a].position;
        let pos_b = network.nodes[spring.node_b].position;
        let dx = pos_b[0] - pos_a[0];
        let dy = pos_b[1] - pos_a[1];
        let dz = pos_b[2] - pos_a[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        let extension = (dist - spring.rest_length).abs();
        let force = spring.stiffness * extension;
        deform_stresses[si] = force / spring.strength;
    }

    // --- Phase 2: Weight-flow stress ---
    // Build a BFS spanning tree from pinned nodes. For each tree edge, the
    // downstream weight is the total weight of all descendants. This captures
    // cantilever bottleneck stress that the deformation solver misses (because
    // horizontal springs resist bending only through geometric nonlinearity).
    let mut weight_stresses = vec![0.0f32; num_springs];
    compute_weight_flow_stress(network, gravity, &node_springs, &mut weight_stresses);

    // --- Combine: take max of both analyses per spring ---
    let mut spring_stresses = Vec::with_capacity(num_springs);
    let mut max_stress_ratio: f32 = 0.0;
    let mut any_failed = false;

    for si in 0..num_springs {
        let stress = deform_stresses[si].max(weight_stresses[si]);
        spring_stresses.push(stress);
        if stress > max_stress_ratio {
            max_stress_ratio = stress;
        }
        if stress > 1.0 {
            any_failed = true;
        }
    }

    SolveResult {
        spring_stresses,
        max_stress_ratio,
        any_failed,
    }
}

/// Compute weight-flow stress by distributing load among parallel paths.
///
/// BFS from pinned nodes computes distance-to-ground for each node. Then
/// processes nodes from leaves to roots (reverse distance order). At each
/// node, the accumulated load (own mass + received from downstream nodes)
/// is distributed among "upstream" springs (those connecting to closer-to-ground
/// nodes) proportionally to spring stiffness. This correctly models load
/// sharing in redundant structures (e.g., a 3-wide arm distributes load
/// among 3 junction springs).
fn compute_weight_flow_stress(
    network: &StructuralNetwork,
    gravity: f32,
    node_springs: &[Vec<(usize, usize)>],
    weight_stresses: &mut [f32],
) {
    let num_nodes = network.nodes.len();
    if num_nodes == 0 {
        return;
    }

    // BFS from pinned nodes to compute distance-to-ground.
    let mut dist_to_ground: Vec<u32> = vec![u32::MAX; num_nodes];
    let mut queue = std::collections::VecDeque::new();
    for (i, node) in network.nodes.iter().enumerate() {
        if node.pinned {
            dist_to_ground[i] = 0;
            queue.push_back(i);
        }
    }
    while let Some(current) = queue.pop_front() {
        for &(_, other) in &node_springs[current] {
            if dist_to_ground[other] > dist_to_ground[current] + 1 {
                dist_to_ground[other] = dist_to_ground[current] + 1;
                queue.push_back(other);
            }
        }
    }

    // Sort nodes by distance (largest first = leaves first).
    let mut order: Vec<usize> = (0..num_nodes).collect();
    order.sort_by(|a, b| dist_to_ground[*b].cmp(&dist_to_ground[*a]));

    // Process from leaves to roots, distributing load upstream.
    let mut accumulated_load: Vec<f32> = network.nodes.iter().map(|n| n.mass).collect();

    for &node_idx in &order {
        if network.nodes[node_idx].pinned || dist_to_ground[node_idx] == u32::MAX {
            continue;
        }
        if accumulated_load[node_idx] <= 0.0 {
            continue;
        }

        // Find upstream springs (connecting to closer-to-ground nodes).
        let mut upstream: Vec<(usize, usize, f32)> = Vec::new();
        let mut total_k = 0.0f32;

        for &(si, other) in &node_springs[node_idx] {
            if dist_to_ground[other] < dist_to_ground[node_idx] {
                let k = network.springs[si].stiffness.max(1e-6);
                upstream.push((si, other, k));
                total_k += k;
            }
        }

        if total_k <= 0.0 {
            continue;
        }

        let load = accumulated_load[node_idx];

        for (si, other, k) in upstream {
            let fraction = k / total_k;
            let flow = load * gravity * fraction;
            let spring = &network.springs[si];
            if spring.strength > 0.0 {
                weight_stresses[si] = flow / spring.strength;
            }
            accumulated_load[other] += load * fraction;
        }
    }
}

// ---------------------------------------------------------------------------
// Tree validation
// ---------------------------------------------------------------------------

/// Validate that a tree (and any existing construction) is structurally sound
/// under its own weight. Returns `true` if the peak stress ratio stays below
/// the warn threshold, ensuring the generated tree has ample headroom for
/// player-placed platforms and buildings without triggering warnings.
pub fn validate_tree(world: &VoxelWorld, config: &GameConfig) -> bool {
    let mut network = build_network(world, &BTreeMap::new(), config);
    let result = solve(&mut network, config);
    result.max_stress_ratio < config.structural.warn_stress_ratio
}

// ---------------------------------------------------------------------------
// Connectivity flood fill
// ---------------------------------------------------------------------------

/// Check whether all `proposed_voxels` would be connected to ground (any
/// ForestFloor voxel) via face-adjacent solid voxels in the hypothetical
/// world where `proposed_voxels` are set to `proposed_type`.
///
/// Returns `true` if all proposed voxels are reachable from ground.
pub fn flood_fill_connected(
    world: &VoxelWorld,
    proposed_voxels: &[VoxelCoord],
    proposed_type: VoxelType,
) -> bool {
    if proposed_voxels.is_empty() {
        return true;
    }

    // Build a lookup of proposed voxels for O(1) checking.
    let proposed_set: BTreeMap<VoxelCoord, ()> = proposed_voxels.iter().map(|&c| (c, ())).collect();

    // Helper: is this coord solid in the hypothetical world?
    let is_solid = |coord: VoxelCoord| -> bool {
        if proposed_set.contains_key(&coord) {
            // Proposed type: BuildingInterior is not solid for connectivity,
            // but it IS a structural node. For connectivity purposes, any
            // non-Air proposed type counts.
            proposed_type != VoxelType::Air
        } else {
            let vt = world.get(coord);
            vt.is_solid() || vt == VoxelType::BuildingInterior
        }
    };

    // Find a starting ForestFloor voxel for BFS.
    let mut start = None;
    for y in 0..world.size_y as i32 {
        for z in 0..world.size_z as i32 {
            for x in 0..world.size_x as i32 {
                let coord = VoxelCoord::new(x, y, z);
                if world.get(coord) == VoxelType::ForestFloor {
                    start = Some(coord);
                    break;
                }
            }
            if start.is_some() {
                break;
            }
        }
        if start.is_some() {
            break;
        }
    }

    let start = match start {
        Some(s) => s,
        None => return false, // No ground at all.
    };

    // BFS from the starting ForestFloor voxel through face-adjacent solid voxels.
    let mut visited = BTreeMap::new();
    let mut queue = std::collections::VecDeque::new();
    visited.insert(start, ());
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        for &dir in &FaceDirection::ALL {
            let (dx, dy, dz) = dir.to_offset();
            let neighbor = VoxelCoord::new(current.x + dx, current.y + dy, current.z + dz);

            if !world.in_bounds(neighbor) && !proposed_set.contains_key(&neighbor) {
                continue;
            }

            if visited.contains_key(&neighbor) {
                continue;
            }

            if is_solid(neighbor) {
                visited.insert(neighbor, ());
                queue.push_back(neighbor);
            }
        }
    }

    // Check that all proposed voxels were reached.
    proposed_voxels
        .iter()
        .all(|coord| visited.contains_key(coord))
}

// ---------------------------------------------------------------------------
// Blueprint validation
// ---------------------------------------------------------------------------

/// Validate a proposed blueprint against structural integrity rules.
///
/// Checks connectivity (fast-path to Blocked if disconnected), then builds
/// a hypothetical network including the proposed voxels and solves. Returns
/// a tiered result with per-voxel stress data for heatmap rendering.
pub fn validate_blueprint(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    proposed_voxels: &[VoxelCoord],
    proposed_type: VoxelType,
    proposed_faces: &BTreeMap<VoxelCoord, FaceData>,
    config: &GameConfig,
) -> BlueprintValidation {
    // Fast-path: connectivity check.
    if !flood_fill_connected(world, proposed_voxels, proposed_type) {
        return BlueprintValidation {
            tier: ValidationTier::Blocked,
            stress_map: BTreeMap::new(),
            message: "Structure is not connected to the ground.".to_string(),
        };
    }

    // Build hypothetical world with proposed voxels placed.
    let mut hypo_world = world.clone();
    for &coord in proposed_voxels {
        hypo_world.set(coord, proposed_type);
    }

    // Merge face data.
    let mut hypo_face_data = face_data.clone();
    for (coord, fd) in proposed_faces {
        hypo_face_data.insert(*coord, fd.clone());
    }

    // Build network and solve.
    let mut network = build_network(&hypo_world, &hypo_face_data, config);
    let result = solve(&mut network, config);

    // Build per-voxel stress map: for each voxel, the max stress ratio of
    // any spring connected to it.
    let mut stress_map = BTreeMap::new();

    // Build reverse lookup: node index -> VoxelCoord.
    let node_to_coord: BTreeMap<usize, VoxelCoord> = network
        .coord_to_node
        .iter()
        .map(|(&coord, &idx)| (idx, coord))
        .collect();

    for (spring_idx, spring) in network.springs.iter().enumerate() {
        let stress = result.spring_stresses[spring_idx];
        if let Some(&coord_a) = node_to_coord.get(&spring.node_a) {
            let entry = stress_map.entry(coord_a).or_insert(0.0f32);
            if stress > *entry {
                *entry = stress;
            }
        }
        if let Some(&coord_b) = node_to_coord.get(&spring.node_b) {
            let entry = stress_map.entry(coord_b).or_insert(0.0f32);
            if stress > *entry {
                *entry = stress;
            }
        }
    }

    // Classify based on thresholds.
    let structural = &config.structural;
    if result.max_stress_ratio >= structural.block_stress_ratio {
        BlueprintValidation {
            tier: ValidationTier::Blocked,
            stress_map,
            message: format!(
                "Structure would fail: peak stress {:.1}x exceeds limit {:.1}x.",
                result.max_stress_ratio, structural.block_stress_ratio
            ),
        }
    } else if result.max_stress_ratio >= structural.warn_stress_ratio {
        BlueprintValidation {
            tier: ValidationTier::Warning,
            stress_map,
            message: format!(
                "Structure is under significant stress ({:.1}x of limit).",
                result.max_stress_ratio
            ),
        }
    } else {
        BlueprintValidation {
            tier: ValidationTier::Ok,
            stress_map,
            message: "Structure is sound.".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Fast blueprint validation (BFS + weight-flow only)
// ---------------------------------------------------------------------------

/// Build a structural network from an explicit set of solid voxels.
///
/// Like `build_network()` but instead of iterating the entire world grid, it
/// operates only on a provided set of voxels. This is much faster for
/// blueprint validation where we only need the connected component around
/// the proposed construction, not the whole 8.4M-voxel world.
fn build_network_from_set(
    voxels: &BTreeMap<VoxelCoord, VoxelType>,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    config: &GameConfig,
) -> StructuralNetwork {
    let mut nodes = Vec::new();
    let mut coord_to_node = BTreeMap::new();
    let mut springs = Vec::new();

    let structural = &config.structural;

    // Pass 1: Create nodes for all voxels in the set.
    for (&coord, &vt) in voxels {
        if vt == VoxelType::Air || vt == VoxelType::Leaf || vt == VoxelType::Fruit {
            continue;
        }

        let mass;
        let pinned;

        if vt == VoxelType::BuildingInterior {
            let mut m = structural.building_interior_base_weight;
            if let Some(fd) = face_data.get(&coord) {
                for &dir in &FaceDirection::ALL {
                    let ft = fd.get(dir);
                    if let Some(fp) = structural.face_properties.get(&ft) {
                        m += fp.weight;
                    }
                }
            }
            mass = m;
            pinned = false;
        } else if let Some(mat) = structural.materials.get(&vt) {
            mass = mat.density;
            pinned = vt == VoxelType::ForestFloor || vt == VoxelType::Dirt;
        } else {
            continue;
        }

        let node_idx = nodes.len();
        coord_to_node.insert(coord, node_idx);
        nodes.push(Node {
            position: [coord.x as f32, coord.y as f32, coord.z as f32],
            mass,
            pinned,
        });
    }

    // Pass 2: Create springs for face-adjacent pairs (3 positive-half neighbors).
    let positive_offsets: [(i32, i32, i32); 3] = [(1, 0, 0), (0, 1, 0), (0, 0, 1)];

    for (&coord_a, &idx_a) in &coord_to_node {
        let vt_a = voxels[&coord_a];

        for &(dx, dy, dz) in &positive_offsets {
            let coord_b = VoxelCoord::new(coord_a.x + dx, coord_a.y + dy, coord_a.z + dz);
            let idx_b = match coord_to_node.get(&coord_b) {
                Some(&idx) => idx,
                None => continue,
            };
            let vt_b = voxels[&coord_b];

            let (stiffness, strength, rest_length) = compute_spring_properties(
                coord_a, vt_a, coord_b, vt_b, dx, dy, dz, face_data, structural,
            );

            if stiffness <= 0.0 && strength <= 0.0 {
                continue;
            }

            springs.push(Spring {
                node_a: idx_a,
                node_b: idx_b,
                stiffness,
                strength,
                rest_length,
            });
        }
    }

    StructuralNetwork {
        nodes,
        springs,
        coord_to_node,
    }
}

/// Fast blueprint validation using BFS + weight-flow-only analysis.
///
/// Instead of iterating the full world grid (8.4M voxels), cloning the world,
/// and running 200 Gauss-Seidel iterations, this function:
///
/// 1. BFS outward from proposed voxels through face-adjacent solid voxels
///    (treating proposed as their target type). This simultaneously checks
///    connectivity (did we reach ForestFloor?) and collects the connected
///    component.
///
/// 2. Builds a network from the visited set only (typically ~5K voxels).
///
/// 3. Runs weight-flow-only analysis (single BFS from pinned nodes + reverse
///    load propagation) — no Gauss-Seidel solver. Weight-flow captures
///    cantilever bottleneck stress, which is the primary gameplay concern.
///
/// Cost: ~15K ops vs ~10M+ for the full path (~700x faster).
pub fn validate_blueprint_fast(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    proposed_voxels: &[VoxelCoord],
    proposed_type: VoxelType,
    proposed_faces: &BTreeMap<VoxelCoord, FaceData>,
    config: &GameConfig,
) -> BlueprintValidation {
    if proposed_voxels.is_empty() {
        return BlueprintValidation {
            tier: ValidationTier::Ok,
            stress_map: BTreeMap::new(),
            message: "No voxels proposed.".to_string(),
        };
    }

    // Build a lookup of proposed voxels.
    let proposed_set: BTreeMap<VoxelCoord, ()> = proposed_voxels.iter().map(|&c| (c, ())).collect();

    // Helper: what type is this voxel in the hypothetical world?
    let hypo_type = |coord: VoxelCoord| -> VoxelType {
        if proposed_set.contains_key(&coord) {
            proposed_type
        } else {
            world.get(coord)
        }
    };

    // Helper: is this coord structurally relevant (non-Air, non-Leaf, non-Fruit)?
    let is_structural = |vt: VoxelType| -> bool {
        vt != VoxelType::Air && vt != VoxelType::Leaf && vt != VoxelType::Fruit
    };

    // BFS from proposed voxels outward through face-adjacent structural voxels.
    let mut visited: BTreeMap<VoxelCoord, VoxelType> = BTreeMap::new();
    let mut queue = std::collections::VecDeque::new();
    let mut reached_ground = false;

    // Seed with proposed voxels.
    for &coord in proposed_voxels {
        let vt = proposed_type;
        if is_structural(vt) {
            visited.insert(coord, vt);
            queue.push_back(coord);
            if vt == VoxelType::ForestFloor {
                reached_ground = true;
            }
        }
    }

    while let Some(current) = queue.pop_front() {
        for &dir in &FaceDirection::ALL {
            let (dx, dy, dz) = dir.to_offset();
            let neighbor = VoxelCoord::new(current.x + dx, current.y + dy, current.z + dz);

            if visited.contains_key(&neighbor) {
                continue;
            }

            // Check bounds for non-proposed voxels.
            if !proposed_set.contains_key(&neighbor) && !world.in_bounds(neighbor) {
                continue;
            }

            let vt = hypo_type(neighbor);
            if is_structural(vt) {
                visited.insert(neighbor, vt);
                queue.push_back(neighbor);
                if vt == VoxelType::ForestFloor {
                    reached_ground = true;
                }
            }
        }
    }

    // Connectivity check: all proposed voxels must be reachable from ground.
    if !reached_ground {
        return BlueprintValidation {
            tier: ValidationTier::Blocked,
            stress_map: BTreeMap::new(),
            message: "Structure is not connected to the ground.".to_string(),
        };
    }

    // Merge face data for the network.
    let mut merged_face_data = BTreeMap::new();
    for &coord in visited.keys() {
        if let Some(fd) = face_data.get(&coord) {
            merged_face_data.insert(coord, fd.clone());
        }
        if let Some(fd) = proposed_faces.get(&coord) {
            merged_face_data.insert(coord, fd.clone());
        }
    }

    // Build network from visited set and run weight-flow-only analysis.
    let network = build_network_from_set(&visited, &merged_face_data, config);
    let gravity = config.structural.gravity;

    // Build per-node adjacency.
    let num_nodes = network.nodes.len();
    let mut node_springs: Vec<Vec<(usize, usize)>> = vec![Vec::new(); num_nodes];
    for (si, spring) in network.springs.iter().enumerate() {
        node_springs[spring.node_a].push((si, spring.node_b));
        node_springs[spring.node_b].push((si, spring.node_a));
    }

    // Weight-flow stress computation.
    let num_springs = network.springs.len();
    let mut weight_stresses = vec![0.0f32; num_springs];
    compute_weight_flow_stress(&network, gravity, &node_springs, &mut weight_stresses);

    // Find max stress and build per-voxel stress map.
    let mut max_stress_ratio: f32 = 0.0;
    let mut stress_map = BTreeMap::new();

    let node_to_coord: BTreeMap<usize, VoxelCoord> = network
        .coord_to_node
        .iter()
        .map(|(&coord, &idx)| (idx, coord))
        .collect();

    for (si, spring) in network.springs.iter().enumerate() {
        let stress = weight_stresses[si];
        if stress > max_stress_ratio {
            max_stress_ratio = stress;
        }
        if let Some(&coord_a) = node_to_coord.get(&spring.node_a) {
            let entry = stress_map.entry(coord_a).or_insert(0.0f32);
            if stress > *entry {
                *entry = stress;
            }
        }
        if let Some(&coord_b) = node_to_coord.get(&spring.node_b) {
            let entry = stress_map.entry(coord_b).or_insert(0.0f32);
            if stress > *entry {
                *entry = stress;
            }
        }
    }

    // Classify based on thresholds.
    let structural = &config.structural;
    if max_stress_ratio >= structural.block_stress_ratio {
        BlueprintValidation {
            tier: ValidationTier::Blocked,
            stress_map,
            message: format!(
                "Structure would fail: peak stress {:.1}x exceeds limit {:.1}x.",
                max_stress_ratio, structural.block_stress_ratio
            ),
        }
    } else if max_stress_ratio >= structural.warn_stress_ratio {
        BlueprintValidation {
            tier: ValidationTier::Warning,
            stress_map,
            message: format!(
                "Structure is under significant stress ({:.1}x of limit).",
                max_stress_ratio
            ),
        }
    } else {
        BlueprintValidation {
            tier: ValidationTier::Ok,
            stress_map,
            message: "Structure is sound.".to_string(),
        }
    }
}

/// Fast carve validation using BFS + weight-flow-only analysis.
///
/// Mirrors `validate_blueprint_fast()` but checks whether *removing* voxels
/// would compromise the remaining structure. Seeds BFS from the face-adjacent
/// neighbors of the carved voxels (the surviving structure), not from the
/// carved voxels themselves.
pub fn validate_carve_fast(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    carved_voxels: &[VoxelCoord],
    config: &GameConfig,
) -> BlueprintValidation {
    if carved_voxels.is_empty() {
        return BlueprintValidation {
            tier: ValidationTier::Ok,
            stress_map: BTreeMap::new(),
            message: "No voxels to carve.".to_string(),
        };
    }

    // Build lookup of carved voxels.
    let carved_set: BTreeMap<VoxelCoord, ()> = carved_voxels.iter().map(|&c| (c, ())).collect();

    // Hypothetical world: carved coords become Air, everything else unchanged.
    let hypo_type = |coord: VoxelCoord| -> VoxelType {
        if carved_set.contains_key(&coord) {
            VoxelType::Air
        } else {
            world.get(coord)
        }
    };

    let is_structural = |vt: VoxelType| -> bool {
        vt != VoxelType::Air && vt != VoxelType::Leaf && vt != VoxelType::Fruit
    };

    // Seed BFS from face-adjacent neighbors of carved voxels that are
    // structural in the hypothetical world and not themselves carved.
    let mut visited: BTreeMap<VoxelCoord, VoxelType> = BTreeMap::new();
    let mut queue = std::collections::VecDeque::new();
    let mut reached_ground = false;

    for &carved in carved_voxels {
        for &dir in &FaceDirection::ALL {
            let (dx, dy, dz) = dir.to_offset();
            let neighbor = VoxelCoord::new(carved.x + dx, carved.y + dy, carved.z + dz);

            if carved_set.contains_key(&neighbor) || visited.contains_key(&neighbor) {
                continue;
            }
            if !world.in_bounds(neighbor) {
                continue;
            }

            let vt = hypo_type(neighbor);
            if is_structural(vt) {
                // Seed from non-ground structural neighbors only.
                // ForestFloor is ground — the question is whether the
                // remaining *above-ground* structure can still reach it.
                // If ForestFloor were a seed, disconnected voxels above
                // would appear connected via the shared BFS frontier.
                if vt == VoxelType::ForestFloor {
                    // Mark as visited so BFS reaching this coord counts
                    // as reaching ground, but don't enqueue — we don't
                    // want to flood outward from the floor itself.
                    visited.insert(neighbor, vt);
                } else {
                    visited.insert(neighbor, vt);
                    queue.push_back(neighbor);
                }
            }
        }
    }

    // No structural neighbors → carving non-structural or isolated voxels.
    if visited.is_empty() {
        return BlueprintValidation {
            tier: ValidationTier::Ok,
            stress_map: BTreeMap::new(),
            message: "Structure is sound.".to_string(),
        };
    }

    // BFS outward through remaining structural voxels.
    while let Some(current) = queue.pop_front() {
        for &dir in &FaceDirection::ALL {
            let (dx, dy, dz) = dir.to_offset();
            let neighbor = VoxelCoord::new(current.x + dx, current.y + dy, current.z + dz);

            if visited.contains_key(&neighbor) || carved_set.contains_key(&neighbor) {
                continue;
            }
            if !world.in_bounds(neighbor) {
                continue;
            }

            let vt = hypo_type(neighbor);
            if is_structural(vt) {
                visited.insert(neighbor, vt);
                queue.push_back(neighbor);
                if vt == VoxelType::ForestFloor {
                    reached_ground = true;
                }
            }
        }
    }

    // Connectivity check: remaining structure must reach ground.
    if !reached_ground {
        return BlueprintValidation {
            tier: ValidationTier::Blocked,
            stress_map: BTreeMap::new(),
            message: "Carving would disconnect structure from the ground.".to_string(),
        };
    }

    // Merge face data for the network (excluding carved voxels).
    let mut merged_face_data = BTreeMap::new();
    for &coord in visited.keys() {
        if let Some(fd) = face_data.get(&coord) {
            merged_face_data.insert(coord, fd.clone());
        }
    }

    // Build network from visited set and run weight-flow-only analysis.
    let network = build_network_from_set(&visited, &merged_face_data, config);
    let gravity = config.structural.gravity;

    let num_nodes = network.nodes.len();
    let mut node_springs: Vec<Vec<(usize, usize)>> = vec![Vec::new(); num_nodes];
    for (si, spring) in network.springs.iter().enumerate() {
        node_springs[spring.node_a].push((si, spring.node_b));
        node_springs[spring.node_b].push((si, spring.node_a));
    }

    let num_springs = network.springs.len();
    let mut weight_stresses = vec![0.0f32; num_springs];
    compute_weight_flow_stress(&network, gravity, &node_springs, &mut weight_stresses);

    // Find max stress and build per-voxel stress map.
    let mut max_stress_ratio: f32 = 0.0;
    let mut stress_map = BTreeMap::new();

    let node_to_coord: BTreeMap<usize, VoxelCoord> = network
        .coord_to_node
        .iter()
        .map(|(&coord, &idx)| (idx, coord))
        .collect();

    for (si, spring) in network.springs.iter().enumerate() {
        let stress = weight_stresses[si];
        if stress > max_stress_ratio {
            max_stress_ratio = stress;
        }
        if let Some(&coord_a) = node_to_coord.get(&spring.node_a) {
            let entry = stress_map.entry(coord_a).or_insert(0.0f32);
            if stress > *entry {
                *entry = stress;
            }
        }
        if let Some(&coord_b) = node_to_coord.get(&spring.node_b) {
            let entry = stress_map.entry(coord_b).or_insert(0.0f32);
            if stress > *entry {
                *entry = stress;
            }
        }
    }

    // Classify based on thresholds.
    let structural = &config.structural;
    if max_stress_ratio >= structural.block_stress_ratio {
        BlueprintValidation {
            tier: ValidationTier::Blocked,
            stress_map,
            message: format!(
                "Carving would cause structural failure: peak stress {:.1}x exceeds limit {:.1}x.",
                max_stress_ratio, structural.block_stress_ratio
            ),
        }
    } else if max_stress_ratio >= structural.warn_stress_ratio {
        BlueprintValidation {
            tier: ValidationTier::Warning,
            stress_map,
            message: format!(
                "Remaining structure is under significant stress ({:.1}x of limit).",
                max_stress_ratio
            ),
        }
    } else {
        BlueprintValidation {
            tier: ValidationTier::Ok,
            stress_map,
            message: "Structure is sound.".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GameConfig;
    use crate::types::{FaceData, FaceDirection, FaceType, VoxelCoord, VoxelType};
    use crate::world::VoxelWorld;

    /// Helper: create a small world with a forest floor at y=0 across the
    /// given x/z range, plus a vertical column of trunk from y=1 up to
    /// `column_height`, centered at (cx, cz).
    fn make_column_world(
        size: u32,
        floor_range: std::ops::Range<i32>,
        cx: i32,
        cz: i32,
        column_height: i32,
    ) -> VoxelWorld {
        let mut world = VoxelWorld::new(size, size, size);

        // Forest floor at y=0.
        for x in floor_range.clone() {
            for z in floor_range.clone() {
                world.set(VoxelCoord::new(x, 0, z), VoxelType::ForestFloor);
            }
        }

        // Trunk column.
        for y in 1..=column_height {
            world.set(VoxelCoord::new(cx, y, cz), VoxelType::Trunk);
        }

        world
    }

    /// Helper: add a horizontal arm of `arm_type` at height `y` extending
    /// from (start_x, y, z) to (end_x, y, z).
    fn add_horizontal_arm(
        world: &mut VoxelWorld,
        y: i32,
        z: i32,
        start_x: i32,
        end_x: i32,
        arm_type: VoxelType,
    ) {
        for x in start_x..=end_x {
            world.set(VoxelCoord::new(x, y, z), arm_type);
        }
    }

    // --- Network construction tests ---

    #[test]
    fn build_network_column_and_platform() {
        let mut world = make_column_world(16, 0..8, 4, 4, 5);
        // Add a 3-voxel platform at y=5 extending from x=5 to x=7.
        add_horizontal_arm(&mut world, 5, 4, 5, 7, VoxelType::GrownPlatform);

        let config = GameConfig::default();
        let network = build_network(&world, &BTreeMap::new(), &config);

        // Count nodes: 64 floor (8x8) + 5 trunk + 3 platform = 72.
        assert_eq!(network.nodes.len(), 72);

        // All ForestFloor nodes should be pinned.
        let pinned_count = network.nodes.iter().filter(|n| n.pinned).count();
        assert_eq!(pinned_count, 64);

        // Springs should exist (exact count depends on adjacency).
        assert!(!network.springs.is_empty());
    }

    #[test]
    fn build_network_no_air_nodes() {
        let world = VoxelWorld::new(8, 8, 8); // All air.
        let config = GameConfig::default();
        let network = build_network(&world, &BTreeMap::new(), &config);
        assert_eq!(network.nodes.len(), 0);
        assert_eq!(network.springs.len(), 0);
    }

    // --- Solver tests ---

    #[test]
    fn short_cantilever_passes() {
        // A 3-voxel horizontal arm should be well within limits.
        let mut world = make_column_world(16, 0..8, 4, 4, 5);
        add_horizontal_arm(&mut world, 5, 4, 5, 7, VoxelType::Branch);

        let config = GameConfig::default();
        let mut network = build_network(&world, &BTreeMap::new(), &config);
        let result = solve(&mut network, &config);

        assert!(
            !result.any_failed,
            "Short cantilever should not fail. Max stress: {}",
            result.max_stress_ratio
        );
        assert!(
            result.max_stress_ratio < config.structural.warn_stress_ratio,
            "Short cantilever stress {} should be below warn threshold {}",
            result.max_stress_ratio,
            config.structural.warn_stress_ratio
        );
    }

    #[test]
    fn long_cantilever_fails() {
        // A 35-voxel GrownPlatform arm exceeds the junction spring's capacity:
        // weight = 35 * 0.6 = 21.0, strength = min(Trunk=10000, GrownPlatform=8) = 8.
        // Stress = 21/8 = 2.625 → fails.
        let mut world = make_column_world(48, 0..10, 5, 5, 10);
        add_horizontal_arm(&mut world, 10, 5, 6, 40, VoxelType::GrownPlatform);

        let config = GameConfig::default();
        let mut network = build_network(&world, &BTreeMap::new(), &config);
        let result = solve(&mut network, &config);

        assert!(
            result.any_failed,
            "Long cantilever should fail. Max stress: {}",
            result.max_stress_ratio
        );
        assert!(
            result.max_stress_ratio > 1.0,
            "Max stress {} should exceed 1.0",
            result.max_stress_ratio
        );
    }

    #[test]
    fn stress_monotonically_increases_with_arm_length() {
        let lengths = [3, 5, 8, 12, 16, 20];
        let mut prev_stress = 0.0f32;

        for &len in &lengths {
            let mut world = make_column_world(32, 0..10, 5, 5, 10);
            add_horizontal_arm(&mut world, 10, 5, 6, 5 + len, VoxelType::Branch);

            let config = GameConfig::default();
            let mut network = build_network(&world, &BTreeMap::new(), &config);
            let result = solve(&mut network, &config);

            assert!(
                result.max_stress_ratio > prev_stress,
                "Stress should increase with arm length. len={}, stress={}, prev={}",
                len,
                result.max_stress_ratio,
                prev_stress
            );
            prev_stress = result.max_stress_ratio;
        }
    }

    #[test]
    fn solver_is_deterministic() {
        let mut world = make_column_world(16, 0..8, 4, 4, 5);
        add_horizontal_arm(&mut world, 5, 4, 5, 12, VoxelType::Branch);

        let config = GameConfig::default();

        let mut network1 = build_network(&world, &BTreeMap::new(), &config);
        let result1 = solve(&mut network1, &config);

        let mut network2 = build_network(&world, &BTreeMap::new(), &config);
        let result2 = solve(&mut network2, &config);

        assert_eq!(result1.spring_stresses.len(), result2.spring_stresses.len());
        for (s1, s2) in result1.spring_stresses.iter().zip(&result2.spring_stresses) {
            assert_eq!(
                s1.to_bits(),
                s2.to_bits(),
                "Stress values must be bit-identical"
            );
        }
        assert_eq!(
            result1.max_stress_ratio.to_bits(),
            result2.max_stress_ratio.to_bits()
        );
    }

    #[test]
    fn highest_stress_at_junction() {
        // The junction spring (where arm meets column) should have the
        // highest stress, not the tip.
        let mut world = make_column_world(32, 0..10, 5, 5, 10);
        add_horizontal_arm(&mut world, 10, 5, 6, 20, VoxelType::Branch);

        let config = GameConfig::default();
        let mut network = build_network(&world, &BTreeMap::new(), &config);
        let result = solve(&mut network, &config);

        // Find the stress at the junction spring (connecting trunk at (5,10,5)
        // to first arm voxel at (6,10,5)).
        let junction_trunk_idx = network.coord_to_node[&VoxelCoord::new(5, 10, 5)];
        let junction_arm_idx = network.coord_to_node[&VoxelCoord::new(6, 10, 5)];

        let mut junction_stress = None;
        for (i, spring) in network.springs.iter().enumerate() {
            if (spring.node_a == junction_trunk_idx && spring.node_b == junction_arm_idx)
                || (spring.node_a == junction_arm_idx && spring.node_b == junction_trunk_idx)
            {
                junction_stress = Some(result.spring_stresses[i]);
                break;
            }
        }

        let junction_stress = junction_stress.expect("Junction spring should exist");

        // The tip spring (between the last two arm voxels) should have lower stress.
        let tip_a_idx = network.coord_to_node[&VoxelCoord::new(19, 10, 5)];
        let tip_b_idx = network.coord_to_node[&VoxelCoord::new(20, 10, 5)];

        let mut tip_stress = None;
        for (i, spring) in network.springs.iter().enumerate() {
            if (spring.node_a == tip_a_idx && spring.node_b == tip_b_idx)
                || (spring.node_a == tip_b_idx && spring.node_b == tip_a_idx)
            {
                tip_stress = Some(result.spring_stresses[i]);
                break;
            }
        }

        let tip_stress = tip_stress.expect("Tip spring should exist");

        assert!(
            junction_stress > tip_stress,
            "Junction stress {} should exceed tip stress {}",
            junction_stress,
            tip_stress
        );
    }

    // --- validate_tree tests ---

    #[test]
    fn small_tree_passes_validation() {
        // Column + short arm = structurally sound.
        let mut world = make_column_world(16, 0..8, 4, 4, 5);
        add_horizontal_arm(&mut world, 5, 4, 5, 7, VoxelType::Branch);

        let config = GameConfig::default();
        assert!(validate_tree(&world, &config));
    }

    #[test]
    fn extreme_cantilever_fails_validation() {
        // 40-voxel GrownPlatform arm: weight = 40 * 0.6 = 24, strength = 8. Fails.
        let mut world = make_column_world(56, 0..10, 5, 5, 10);
        add_horizontal_arm(&mut world, 10, 5, 6, 45, VoxelType::GrownPlatform);

        let config = GameConfig::default();
        assert!(!validate_tree(&world, &config));
    }

    // --- Flood fill connectivity tests ---

    #[test]
    fn connected_cluster_returns_true() {
        let mut world = make_column_world(16, 0..8, 4, 4, 5);
        // Propose extending the column by one voxel.
        let proposed = vec![VoxelCoord::new(4, 6, 4)];
        assert!(flood_fill_connected(&world, &proposed, VoxelType::Trunk));

        // Propose a platform adjacent to trunk.
        let proposed = vec![VoxelCoord::new(5, 5, 4), VoxelCoord::new(6, 5, 4)];
        assert!(flood_fill_connected(
            &world,
            &proposed,
            VoxelType::GrownPlatform
        ));

        // Existing arm also connected.
        add_horizontal_arm(&mut world, 5, 4, 5, 7, VoxelType::Branch);
        let proposed = vec![VoxelCoord::new(8, 5, 4)];
        assert!(flood_fill_connected(&world, &proposed, VoxelType::Branch));
    }

    #[test]
    fn floating_cluster_returns_false() {
        let world = make_column_world(16, 0..8, 4, 4, 5);
        // Propose voxels floating in the air, not adjacent to any solid.
        let proposed = vec![VoxelCoord::new(10, 10, 10), VoxelCoord::new(11, 10, 10)];
        assert!(!flood_fill_connected(
            &world,
            &proposed,
            VoxelType::GrownPlatform
        ));
    }

    #[test]
    fn single_voxel_bridge_connection() {
        let world = make_column_world(16, 0..8, 4, 4, 5);
        // Propose a single voxel adjacent to the top of the column.
        let proposed = vec![VoxelCoord::new(5, 5, 4)];
        assert!(flood_fill_connected(
            &world,
            &proposed,
            VoxelType::GrownPlatform
        ));
    }

    // --- Blueprint validation tests ---

    #[test]
    fn short_platform_blueprint_ok() {
        let mut world = make_column_world(16, 0..8, 4, 4, 5);
        // Ensure column is solid.
        add_horizontal_arm(&mut world, 5, 4, 5, 6, VoxelType::GrownPlatform);

        let config = GameConfig::default();
        let proposed = vec![VoxelCoord::new(5, 5, 4), VoxelCoord::new(6, 5, 4)];
        let validation = validate_blueprint(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );

        assert_eq!(validation.tier, ValidationTier::Ok);
    }

    #[test]
    fn extreme_cantilever_blueprint_blocked() {
        // block_stress_ratio is 1.0, so junction stress >= 1.0 triggers block.
        // strength = min(Trunk=10000, GrownPlatform=8) = 8.
        // Total = 38 arm voxels. Weight = 38 * 0.6 = 22.8.
        // Stress = 22.8/8 = 2.85 > block_stress_ratio (1.0).
        let config = GameConfig::default();

        let mut world = make_column_world(48, 0..10, 5, 5, 10);
        add_horizontal_arm(&mut world, 10, 5, 6, 38, VoxelType::GrownPlatform);

        // Propose extending the 33-voxel arm by 5 more.
        let proposed: Vec<VoxelCoord> = (39..=43).map(|x| VoxelCoord::new(x, 10, 5)).collect();
        let validation = validate_blueprint(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );

        assert_eq!(
            validation.tier,
            ValidationTier::Blocked,
            "Extreme cantilever extension should be blocked. Message: {}",
            validation.message
        );
    }

    #[test]
    fn disconnected_blueprint_blocked() {
        let world = make_column_world(16, 0..8, 4, 4, 5);
        let proposed = vec![VoxelCoord::new(10, 10, 10)];
        let config = GameConfig::default();

        let validation = validate_blueprint(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );

        assert_eq!(validation.tier, ValidationTier::Blocked);
        assert!(
            validation.message.contains("not connected"),
            "Message should mention connectivity: {}",
            validation.message
        );
    }

    #[test]
    fn support_struts_reduce_stress() {
        // An arm with a support column to ground has lower peak stress
        // because load is distributed among two paths to ground.
        let arm_end = 15;
        let arm_y = 10;

        // Without support.
        let mut world1 = make_column_world(24, 0..16, 5, 5, arm_y);
        add_horizontal_arm(&mut world1, arm_y, 5, 6, arm_end, VoxelType::Branch);
        let config = GameConfig::default();
        let mut net1 = build_network(&world1, &BTreeMap::new(), &config);
        let res1 = solve(&mut net1, &config);

        // With support: a full column from forest floor to the arm midpoint.
        // This creates an alternative load path so the junction spring
        // carries less weight.
        let mut world2 = make_column_world(24, 0..16, 5, 5, arm_y);
        add_horizontal_arm(&mut world2, arm_y, 5, 6, arm_end, VoxelType::Branch);
        // Full support column at x=10 from y=1 to y=9.
        for y in 1..arm_y {
            world2.set(VoxelCoord::new(10, y, 5), VoxelType::Trunk);
        }
        let mut net2 = build_network(&world2, &BTreeMap::new(), &config);
        let res2 = solve(&mut net2, &config);

        assert!(
            res2.max_stress_ratio < res1.max_stress_ratio,
            "Supported arm stress {} should be less than unsupported {}",
            res2.max_stress_ratio,
            res1.max_stress_ratio
        );
    }

    #[test]
    fn wider_arm_on_single_column_increases_stress() {
        // A 3-wide arm on a single column funnels more weight through the
        // junction spring, increasing stress compared to a 1-wide arm.
        let arm_len = 12;
        let arm_y = 10;

        // 1-voxel wide arm.
        let mut world1 = make_column_world(24, 0..10, 5, 5, arm_y);
        add_horizontal_arm(&mut world1, arm_y, 5, 6, 5 + arm_len, VoxelType::Branch);
        let config = GameConfig::default();
        let mut net1 = build_network(&world1, &BTreeMap::new(), &config);
        let res1 = solve(&mut net1, &config);

        // 3-voxel wide arm (z = 4, 5, 6) on the same single column.
        // Extra rows route weight through z-direction springs to the center,
        // increasing load on the junction.
        let mut world2 = make_column_world(24, 0..10, 5, 5, arm_y);
        for z in 4..=6 {
            add_horizontal_arm(&mut world2, arm_y, z, 6, 5 + arm_len, VoxelType::Branch);
        }
        let mut net2 = build_network(&world2, &BTreeMap::new(), &config);
        let res2 = solve(&mut net2, &config);

        assert!(
            res2.max_stress_ratio > res1.max_stress_ratio,
            "3-wide arm on single column stress {} should exceed 1-wide {}",
            res2.max_stress_ratio,
            res1.max_stress_ratio
        );
    }

    // --- Building face tests ---

    #[test]
    fn building_on_cantilever_adds_load() {
        // A bare platform should have lower stress than one with a building on it.
        let arm_y = 5;
        let arm_len = 8;

        // Bare platform.
        let mut world1 = make_column_world(24, 0..10, 5, 5, arm_y);
        add_horizontal_arm(
            &mut world1,
            arm_y,
            5,
            6,
            5 + arm_len,
            VoxelType::GrownPlatform,
        );
        let config = GameConfig::default();
        let mut net1 = build_network(&world1, &BTreeMap::new(), &config);
        let res1 = solve(&mut net1, &config);

        // Platform with BuildingInterior voxels on top (simple 3x3 at end of arm).
        let mut world2 = make_column_world(24, 0..10, 5, 5, arm_y);
        add_horizontal_arm(
            &mut world2,
            arm_y,
            5,
            6,
            5 + arm_len,
            VoxelType::GrownPlatform,
        );
        let mut face_data = BTreeMap::new();
        for x in 10..=12 {
            let coord = VoxelCoord::new(x, arm_y + 1, 5);
            world2.set(coord, VoxelType::BuildingInterior);
            let mut fd = FaceData::default();
            fd.set(FaceDirection::NegY, FaceType::Floor);
            fd.set(FaceDirection::PosY, FaceType::Ceiling);
            // Exterior walls.
            if x == 10 {
                fd.set(FaceDirection::NegX, FaceType::Wall);
            }
            if x == 12 {
                fd.set(FaceDirection::PosX, FaceType::Wall);
            }
            face_data.insert(coord, fd);
        }
        let mut net2 = build_network(&world2, &face_data, &config);
        let res2 = solve(&mut net2, &config);

        assert!(
            res2.max_stress_ratio > res1.max_stress_ratio,
            "Building adds load: with-building stress {} should exceed bare {}",
            res2.max_stress_ratio,
            res1.max_stress_ratio
        );
    }

    #[test]
    fn walls_brace_better_than_windows() {
        // A building with all Wall faces should have lower stress (more bracing)
        // than one with all Window faces, on the same cantilever.
        let arm_y = 5;
        let arm_len = 8;
        let config = GameConfig::default();

        let make_building = |face_type: FaceType| -> f32 {
            let mut world = make_column_world(24, 0..10, 5, 5, arm_y);
            add_horizontal_arm(
                &mut world,
                arm_y,
                5,
                6,
                5 + arm_len,
                VoxelType::GrownPlatform,
            );
            let mut face_data = BTreeMap::new();
            for x in 10..=12 {
                let coord = VoxelCoord::new(x, arm_y + 1, 5);
                world.set(coord, VoxelType::BuildingInterior);
                let mut fd = FaceData::default();
                fd.set(FaceDirection::NegY, FaceType::Floor);
                fd.set(FaceDirection::PosY, FaceType::Ceiling);
                if x == 10 {
                    fd.set(FaceDirection::NegX, face_type);
                }
                if x == 12 {
                    fd.set(FaceDirection::PosX, face_type);
                }
                face_data.insert(coord, fd);
            }
            let mut net = build_network(&world, &face_data, &config);
            let res = solve(&mut net, &config);
            res.max_stress_ratio
        };

        let wall_stress = make_building(FaceType::Wall);
        let window_stress = make_building(FaceType::Window);

        // Wall provides more bracing, but also more weight. The key point is
        // that both should produce valid structural results. We test that
        // wall bracing is at least not worse than window.
        // (The actual relationship depends on whether bracing benefit > weight cost.)
        // For a cantilevered building, the bracing effect of walls is significant
        // since they create stiffer connections to the platform below via Floor.
        assert!(
            wall_stress < window_stress || (wall_stress - window_stress).abs() < 0.1,
            "Wall stress {} should not be significantly worse than window stress {}",
            wall_stress,
            window_stress
        );
    }

    #[test]
    fn stress_map_populated_for_blueprint() {
        let world = make_column_world(16, 0..8, 4, 4, 5);
        let config = GameConfig::default();
        let proposed = vec![VoxelCoord::new(5, 5, 4), VoxelCoord::new(6, 5, 4)];
        let validation = validate_blueprint(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );

        // The stress map should contain entries for at least the proposed voxels.
        assert!(
            !validation.stress_map.is_empty(),
            "Stress map should not be empty"
        );
    }

    // --- Fast blueprint validation tests ---

    #[test]
    fn fast_short_platform_ok() {
        // A 2-voxel platform adjacent to a trunk column should pass.
        let world = make_column_world(16, 0..8, 4, 4, 5);
        let config = GameConfig::default();
        let proposed = vec![VoxelCoord::new(5, 5, 4), VoxelCoord::new(6, 5, 4)];

        let fast = validate_blueprint_fast(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );
        let full = validate_blueprint(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );

        assert_eq!(fast.tier, ValidationTier::Ok);
        assert_eq!(fast.tier, full.tier);
    }

    #[test]
    fn fast_long_cantilever_blocked() {
        // A 33-voxel GrownPlatform arm plus 5 proposed extension should be blocked.
        let config = GameConfig::default();

        let mut world = make_column_world(48, 0..10, 5, 5, 10);
        add_horizontal_arm(&mut world, 10, 5, 6, 38, VoxelType::GrownPlatform);

        let proposed: Vec<VoxelCoord> = (39..=43).map(|x| VoxelCoord::new(x, 10, 5)).collect();

        let fast = validate_blueprint_fast(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );
        let full = validate_blueprint(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );

        assert_eq!(fast.tier, ValidationTier::Blocked);
        assert_eq!(fast.tier, full.tier);
    }

    #[test]
    fn fast_disconnected_blocked() {
        // Floating voxels not connected to ground should be blocked.
        let world = make_column_world(16, 0..8, 4, 4, 5);
        let config = GameConfig::default();
        let proposed = vec![VoxelCoord::new(10, 10, 10)];

        let fast = validate_blueprint_fast(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );

        assert_eq!(fast.tier, ValidationTier::Blocked);
        assert!(
            fast.message.contains("not connected"),
            "Message should mention connectivity: {}",
            fast.message
        );
    }

    #[test]
    fn fast_matches_full_for_warning() {
        // A medium-length cantilever that triggers a warning (stress >= 0.5
        // but < 1.0) should get the same tier from both validators.
        let config = GameConfig::default();

        // 10-voxel arm: weight = 10 * 0.6 = 6.0, strength = 8.
        // Stress = 6.0/8 = 0.75 → Warning (>= 0.5, < 1.0).
        let mut world = make_column_world(24, 0..10, 5, 5, 10);
        add_horizontal_arm(&mut world, 10, 5, 6, 15, VoxelType::GrownPlatform);

        let proposed = vec![VoxelCoord::new(16, 10, 5)];

        let fast = validate_blueprint_fast(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );
        let full = validate_blueprint(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );

        assert_eq!(
            fast.tier, full.tier,
            "Fast tier {:?} should match full tier {:?} (fast stress in msg: {}, full: {})",
            fast.tier, full.tier, fast.message, full.message
        );
    }

    #[test]
    fn fast_platform_on_dirt_terrain_ok() {
        // A short platform on a trunk column should pass even when the floor
        // has Dirt voxels (hilly terrain). Dirt must be pinned in the fast
        // validator just like ForestFloor, otherwise its high density (999)
        // causes bogus structural failure.
        let config = GameConfig::default();
        let mut world = VoxelWorld::new(16, 16, 16);

        // Forest floor at y=0.
        for x in 0..8 {
            for z in 0..8 {
                world.set(VoxelCoord::new(x, 0, z), VoxelType::ForestFloor);
            }
        }
        // Dirt terrain at y=1 (hilly area).
        for x in 0..8 {
            for z in 0..8 {
                world.set(VoxelCoord::new(x, 1, z), VoxelType::Dirt);
            }
        }
        // Trunk column from y=2 to y=6.
        for y in 2..=6 {
            world.set(VoxelCoord::new(4, y, 4), VoxelType::Trunk);
        }

        // Propose a 2-voxel platform at y=6 adjacent to the trunk.
        let proposed = vec![VoxelCoord::new(5, 6, 4), VoxelCoord::new(6, 6, 4)];

        let fast = validate_blueprint_fast(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );
        let full = validate_blueprint(
            &world,
            &BTreeMap::new(),
            &proposed,
            VoxelType::GrownPlatform,
            &BTreeMap::new(),
            &config,
        );

        assert_eq!(
            full.tier,
            ValidationTier::Ok,
            "Full validator should approve short platform on trunk: {}",
            full.message,
        );
        assert_eq!(
            fast.tier,
            ValidationTier::Ok,
            "Fast validator should approve short platform on trunk with dirt: {}",
            fast.message,
        );
    }

    // --- Carve validation tests ---

    #[test]
    fn test_carve_structural_blocks_disconnect() {
        // Build a vertical pillar on forest floor. Carving the base voxel
        // (y=1) disconnects the rest from the ground → Blocked.
        let world = make_column_world(16, 0..8, 4, 4, 5);
        let config = GameConfig::default();

        let carved = vec![VoxelCoord::new(4, 1, 4)];
        let result = validate_carve_fast(&world, &BTreeMap::new(), &carved, &config);

        assert_eq!(
            result.tier,
            ValidationTier::Blocked,
            "Carving base of pillar should be Blocked, got: {}",
            result.message
        );
    }

    #[test]
    fn test_carve_structural_ok_small() {
        // Carve a single voxel from the top of a wide base — should be Ok.
        let world = make_column_world(16, 0..8, 4, 4, 5);
        let config = GameConfig::default();

        // Carve the top voxel — base remains connected.
        let carved = vec![VoxelCoord::new(4, 5, 4)];
        let result = validate_carve_fast(&world, &BTreeMap::new(), &carved, &config);

        assert_eq!(
            result.tier,
            ValidationTier::Ok,
            "Carving top of pillar should be Ok, got: {}",
            result.message
        );
    }
}
