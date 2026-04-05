// Navigation constants and edge types shared by pathfinding and walkability.
//
// This module was formerly the full navigation graph implementation. After the
// migration to voxel-direct A* (F-remove-navgraph), the graph structures were
// removed and only the shared types remain: `EdgeType` for edge classification,
// distance constants, and `scaled_distance()` for integer-only Euclidean
// distance computation.
//
// See also: `pathfinding.rs` for A* search, `walkability.rs` for walkable
// position queries.

/// The type of connection between two nav nodes.
/// Serializable because it appears in surface-type derivation and edge
/// classification for pathfinding cost computation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EdgeType {
    /// Walking on the ground (dirt terrain) around the trunk base.
    #[serde(alias = "ForestFloor")]
    Ground,
    /// Climbing up/down the raw trunk surface.
    TrunkClimb,
    /// Walking along a branch.
    BranchWalk,
    /// Circumferential movement around the trunk at one y-level.
    TrunkCircumference,
    /// Connecting ground-level nodes to trunk surface nodes.
    GroundToTrunk,
    /// Climbing a wood ladder.
    WoodLadderClimb,
    /// Climbing a rope ladder.
    RopeLadderClimb,
}

/// Movement mode that determines which edges a creature can traverse and
/// at what cost relative to its base `move_ticks_per_voxel`. All creatures
/// in the same category use identical edge-cost ratios, enabling cached
/// paths to be shared across species.
///
/// Stored on both `SpeciesData` (default for the species) and the `Creature`
/// DB row (per-creature, initially copied from species at spawn). Pathfinding
/// reads the creature row, not species, to support future modality changes.
///
/// Cost multipliers are applied to the creature's stat-modified base tpv:
/// - 1x = base speed (the `move_ticks_per_voxel` after AGI scaling)
/// - 2x = double the move cost (half the speed)
/// - blocked = edge is impassable for this category
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum MovementCategory {
    /// Ground edges only. No ladders, no climbing.
    WalkOnly = 0,
    /// Ground at 1x + ladders at 2x. No climbing.
    WalkOrLadder = 1,
    /// Ground at 1x + ladders at 1x + climbing (incl. TrunkCircumference) at 2x.
    Climber = 2,
    /// 3D flight at base speed. Uses flight A*, not ground A*.
    Flyer = 3,
}

impl MovementCategory {
    /// Compute the ticks-per-voxel for traversing an edge of the given type.
    /// Returns `None` if this category cannot traverse the edge.
    pub fn tpv_for_edge_type(self, edge_type: EdgeType, base_tpv: u64) -> Option<u64> {
        match self {
            MovementCategory::WalkOnly => match edge_type {
                EdgeType::Ground | EdgeType::BranchWalk => Some(base_tpv),
                // WalkOnly cannot climb, use ladders, or traverse trunk surfaces.
                _ => None,
            },
            MovementCategory::WalkOrLadder => match edge_type {
                EdgeType::Ground | EdgeType::BranchWalk => Some(base_tpv),
                EdgeType::WoodLadderClimb | EdgeType::RopeLadderClimb => Some(base_tpv * 2),
                _ => None,
            },
            MovementCategory::Climber => match edge_type {
                EdgeType::Ground | EdgeType::BranchWalk => Some(base_tpv),
                EdgeType::WoodLadderClimb | EdgeType::RopeLadderClimb => Some(base_tpv),
                EdgeType::TrunkClimb | EdgeType::GroundToTrunk | EdgeType::TrunkCircumference => {
                    Some(base_tpv * 2)
                }
            },
            MovementCategory::Flyer => {
                // Flyers use flight A*, not ground A*. This method shouldn't
                // be called for flyers, but return None for safety.
                None
            }
        }
    }

    /// Whether this category can climb (used for walkability checks —
    /// non-climbers require solid ground directly below).
    pub fn can_climb(self) -> bool {
        matches!(self, MovementCategory::Climber)
    }

    /// Whether this category uses flight A* instead of ground A*.
    pub fn is_flyer(self) -> bool {
        matches!(self, MovementCategory::Flyer)
    }
}

/// Scale factor for integer edge distances. Euclidean voxel distances are
/// multiplied by this value so that irrational lengths (sqrt(2), sqrt(3))
/// are represented without floats. A power of two for cheap multiply/divide.
pub const DIST_SCALE: u32 = 1024;

/// Compute a scaled integer Euclidean distance from coordinate deltas.
/// Returns `floor(sqrt(dx² + dy² + dz²) * DIST_SCALE)`, computed with
/// integer square root for determinism (no floats).
pub fn scaled_distance(dx: i32, dy: i32, dz: i32) -> u32 {
    let sq = (dx as i64 * dx as i64 + dy as i64 * dy as i64 + dz as i64 * dz as i64) as u64;
    let scaled_sq = sq * (DIST_SCALE as u64 * DIST_SCALE as u64);
    scaled_sq.isqrt() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = 500;

    // -- WalkOnly ---------------------------------------------------------

    #[test]
    fn walk_only_ground_is_base_speed() {
        assert_eq!(
            MovementCategory::WalkOnly.tpv_for_edge_type(EdgeType::Ground, BASE),
            Some(BASE)
        );
    }

    #[test]
    fn walk_only_branch_is_base_speed() {
        assert_eq!(
            MovementCategory::WalkOnly.tpv_for_edge_type(EdgeType::BranchWalk, BASE),
            Some(BASE)
        );
    }

    #[test]
    fn walk_only_cannot_climb_trunk() {
        assert_eq!(
            MovementCategory::WalkOnly.tpv_for_edge_type(EdgeType::TrunkClimb, BASE),
            None
        );
    }

    #[test]
    fn walk_only_cannot_use_ladders() {
        assert_eq!(
            MovementCategory::WalkOnly.tpv_for_edge_type(EdgeType::WoodLadderClimb, BASE),
            None
        );
        assert_eq!(
            MovementCategory::WalkOnly.tpv_for_edge_type(EdgeType::RopeLadderClimb, BASE),
            None
        );
    }

    #[test]
    fn walk_only_cannot_trunk_circumference() {
        assert_eq!(
            MovementCategory::WalkOnly.tpv_for_edge_type(EdgeType::TrunkCircumference, BASE),
            None
        );
    }

    #[test]
    fn walk_only_cannot_ground_to_trunk() {
        assert_eq!(
            MovementCategory::WalkOnly.tpv_for_edge_type(EdgeType::GroundToTrunk, BASE),
            None
        );
    }

    // -- WalkOrLadder -----------------------------------------------------

    #[test]
    fn walk_or_ladder_ground_is_base_speed() {
        assert_eq!(
            MovementCategory::WalkOrLadder.tpv_for_edge_type(EdgeType::Ground, BASE),
            Some(BASE)
        );
    }

    #[test]
    fn walk_or_ladder_ladders_are_double_cost() {
        assert_eq!(
            MovementCategory::WalkOrLadder.tpv_for_edge_type(EdgeType::WoodLadderClimb, BASE),
            Some(BASE * 2)
        );
        assert_eq!(
            MovementCategory::WalkOrLadder.tpv_for_edge_type(EdgeType::RopeLadderClimb, BASE),
            Some(BASE * 2)
        );
    }

    #[test]
    fn walk_or_ladder_cannot_climb() {
        assert_eq!(
            MovementCategory::WalkOrLadder.tpv_for_edge_type(EdgeType::TrunkClimb, BASE),
            None
        );
        assert_eq!(
            MovementCategory::WalkOrLadder.tpv_for_edge_type(EdgeType::GroundToTrunk, BASE),
            None
        );
        assert_eq!(
            MovementCategory::WalkOrLadder.tpv_for_edge_type(EdgeType::TrunkCircumference, BASE),
            None
        );
    }

    // -- Climber ----------------------------------------------------------

    #[test]
    fn climber_ground_is_base_speed() {
        assert_eq!(
            MovementCategory::Climber.tpv_for_edge_type(EdgeType::Ground, BASE),
            Some(BASE)
        );
    }

    #[test]
    fn climber_ladders_are_base_speed() {
        assert_eq!(
            MovementCategory::Climber.tpv_for_edge_type(EdgeType::WoodLadderClimb, BASE),
            Some(BASE)
        );
        assert_eq!(
            MovementCategory::Climber.tpv_for_edge_type(EdgeType::RopeLadderClimb, BASE),
            Some(BASE)
        );
    }

    #[test]
    fn climber_climb_is_double_cost() {
        assert_eq!(
            MovementCategory::Climber.tpv_for_edge_type(EdgeType::TrunkClimb, BASE),
            Some(BASE * 2)
        );
        assert_eq!(
            MovementCategory::Climber.tpv_for_edge_type(EdgeType::GroundToTrunk, BASE),
            Some(BASE * 2)
        );
    }

    #[test]
    fn climber_trunk_circumference_is_climb_cost() {
        // B-trunk-circ-speed: circumferential movement should cost like climbing,
        // not like walking.
        assert_eq!(
            MovementCategory::Climber.tpv_for_edge_type(EdgeType::TrunkCircumference, BASE),
            Some(BASE * 2)
        );
    }

    #[test]
    fn climber_branch_walk_is_base_speed() {
        assert_eq!(
            MovementCategory::Climber.tpv_for_edge_type(EdgeType::BranchWalk, BASE),
            Some(BASE)
        );
    }

    // -- Flyer ------------------------------------------------------------

    #[test]
    fn flyer_returns_none_for_ground_edges() {
        // Flyers use flight A*, not ground edges.
        assert_eq!(
            MovementCategory::Flyer.tpv_for_edge_type(EdgeType::Ground, BASE),
            None
        );
    }

    // -- Helper methods ---------------------------------------------------

    #[test]
    fn can_climb_only_for_climber() {
        assert!(!MovementCategory::WalkOnly.can_climb());
        assert!(!MovementCategory::WalkOrLadder.can_climb());
        assert!(MovementCategory::Climber.can_climb());
        assert!(!MovementCategory::Flyer.can_climb());
    }

    #[test]
    fn is_flyer_only_for_flyer() {
        assert!(!MovementCategory::WalkOnly.is_flyer());
        assert!(!MovementCategory::WalkOrLadder.is_flyer());
        assert!(!MovementCategory::Climber.is_flyer());
        assert!(MovementCategory::Flyer.is_flyer());
    }

    // -- Serde roundtrip --------------------------------------------------

    #[test]
    fn movement_category_serde_roundtrip() {
        for cat in [
            MovementCategory::WalkOnly,
            MovementCategory::WalkOrLadder,
            MovementCategory::Climber,
            MovementCategory::Flyer,
        ] {
            let json = serde_json::to_string(&cat).unwrap();
            let back: MovementCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(cat, back);
        }
    }

    #[test]
    fn walk_or_ladder_branch_walk_is_base_speed() {
        assert_eq!(
            MovementCategory::WalkOrLadder.tpv_for_edge_type(EdgeType::BranchWalk, BASE),
            Some(BASE)
        );
    }

    #[test]
    fn movement_category_repr_values() {
        assert_eq!(MovementCategory::WalkOnly as u8, 0);
        assert_eq!(MovementCategory::WalkOrLadder as u8, 1);
        assert_eq!(MovementCategory::Climber as u8, 2);
        assert_eq!(MovementCategory::Flyer as u8, 3);
    }
}
