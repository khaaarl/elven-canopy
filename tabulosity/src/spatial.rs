//! Spatial indexing support for tabulosity tables.
//!
//! Provides the `SpatialKey` trait (user-facing) and `SpatialIndex` wrapper
//! (doc-hidden, used by generated code) backed by the `rstar` crate's R*-tree.
//!
//! Users implement `SpatialKey` on their bounding-box types to enable spatial
//! indexes via `#[indexed(spatial)]` or `#[index(kind = "spatial", ...)]`.
//! The `rstar` dependency is an implementation detail — user code interacts
//! only with `SpatialKey`, `SpatialPoint`, and `[i32; N]` arrays.
//!
//! `MaybeSpatialKey` provides uniform dispatch for both `T: SpatialKey` and
//! `Option<T: SpatialKey>` fields, routing `None` values to a side container
//! instead of the R-tree.

use rstar::{AABB, RTree, RTreeObject};
use std::fmt;

// =============================================================================
// Public traits
// =============================================================================

/// Marker trait for point types usable as `SpatialKey::Point`.
///
/// Only implemented for `[i32; 2]` (2D) and `[i32; 3]` (3D).
/// Users choose the dimensionality by setting `type Point = [i32; 3]`
/// (or `[i32; 2]`) in their `SpatialKey` impl.
pub trait SpatialPoint:
    rstar::Point<Scalar = i32> + Clone + Copy + fmt::Debug + PartialEq + 'static
{
}

impl SpatialPoint for [i32; 2] {}
impl SpatialPoint for [i32; 3] {}

/// Types that can be used as spatial index keys.
///
/// Represents an axis-aligned bounding box (AABB) with `i32` coordinates.
/// Point entries should return the same value from both `spatial_min` and
/// `spatial_max`.
///
/// # Example
///
/// ```ignore
/// impl SpatialKey for VoxelBox {
///     type Point = [i32; 3];
///     fn spatial_min(&self) -> [i32; 3] { [self.min_x, self.min_y, self.min_z] }
///     fn spatial_max(&self) -> [i32; 3] { [self.max_x, self.max_y, self.max_z] }
/// }
/// ```
pub trait SpatialKey: Clone + 'static {
    /// The point type — `[i32; 2]` for 2D or `[i32; 3]` for 3D.
    type Point: SpatialPoint;

    /// Lower corner of the bounding box (minimum coordinate per axis).
    fn spatial_min(&self) -> Self::Point;

    /// Upper corner of the bounding box (maximum coordinate per axis).
    fn spatial_max(&self) -> Self::Point;
}

/// Uniform dispatch for spatial key fields that may be `Option<T>`.
///
/// Generated code calls `MaybeSpatialKey::as_spatial(&row.field)` to decide
/// whether to insert into the R-tree (`Some`) or the none-set (`None`).
pub trait MaybeSpatialKey: Clone + 'static {
    /// The underlying spatial key type (the `T` inside `Option<T>`, or `T` itself).
    type Key: SpatialKey;

    /// Returns `Some(&key)` if the value has spatial data, `None` otherwise.
    fn as_spatial(&self) -> Option<&Self::Key>;
}

impl<T: SpatialKey> MaybeSpatialKey for T {
    type Key = T;
    fn as_spatial(&self) -> Option<&T> {
        Some(self)
    }
}

impl<T: SpatialKey> MaybeSpatialKey for Option<T> {
    type Key = T;
    fn as_spatial(&self) -> Option<&T> {
        self.as_ref()
    }
}

// =============================================================================
// Internal R-tree entry
// =============================================================================

/// An entry in the R-tree pairing a bounding box with a primary key.
///
/// `PartialEq` compares by PK only — rstar uses envelope-based search to
/// narrow candidates, then PK equality to identify the exact entry.
#[doc(hidden)]
pub struct RTreeEntry<PK, P>
where
    PK: Clone + Ord,
    P: SpatialPoint,
{
    aabb: AABB<P>,
    pk: PK,
}

impl<PK: Clone + Ord, P: SpatialPoint> RTreeObject for RTreeEntry<PK, P> {
    type Envelope = AABB<P>;
    fn envelope(&self) -> Self::Envelope {
        self.aabb
    }
}

impl<PK: Clone + Ord, P: SpatialPoint> PartialEq for RTreeEntry<PK, P> {
    fn eq(&self, other: &Self) -> bool {
        self.pk == other.pk
    }
}

// =============================================================================
// SpatialIndex — wrapper around RTree used by generated table code
// =============================================================================

/// R-tree wrapper providing spatial indexing for tabulosity tables.
///
/// All query results are sorted by PK for deterministic iteration order.
/// This type is `#[doc(hidden)]` — user code interacts with generated
/// `intersecting_*` methods on the table companion struct, not with this
/// type directly.
#[doc(hidden)]
pub struct SpatialIndex<PK, P>
where
    PK: Clone + Ord,
    P: SpatialPoint,
{
    tree: RTree<RTreeEntry<PK, P>>,
}

impl<PK: Clone + Ord, P: SpatialPoint> SpatialIndex<PK, P> {
    pub fn new() -> Self {
        Self { tree: RTree::new() }
    }

    /// Insert a spatial key + PK pair into the R-tree.
    pub fn insert(&mut self, key: &impl SpatialKey<Point = P>, pk: PK) {
        let aabb = AABB::from_corners(key.spatial_min(), key.spatial_max());
        self.tree.insert(RTreeEntry { aabb, pk });
    }

    /// Remove a spatial key + PK pair from the R-tree.
    /// The key must match the value that was inserted (same AABB).
    pub fn remove(&mut self, key: &impl SpatialKey<Point = P>, pk: &PK) {
        let aabb = AABB::from_corners(key.spatial_min(), key.spatial_max());
        self.tree.remove(&RTreeEntry {
            aabb,
            pk: pk.clone(),
        });
    }

    /// Returns all PKs whose bounding boxes intersect the given envelope,
    /// sorted by PK for determinism.
    pub fn intersecting(&self, envelope: &impl SpatialKey<Point = P>) -> Vec<PK> {
        let aabb = AABB::from_corners(envelope.spatial_min(), envelope.spatial_max());
        let mut pks: Vec<PK> = self
            .tree
            .locate_in_envelope_intersecting(&aabb)
            .map(|e| e.pk.clone())
            .collect();
        pks.sort();
        pks
    }

    /// Returns the count of entries whose bounding boxes intersect the given
    /// envelope. More efficient than `intersecting().len()` — avoids allocation
    /// and sorting.
    pub fn count_intersecting(&self, envelope: &impl SpatialKey<Point = P>) -> usize {
        let aabb = AABB::from_corners(envelope.spatial_min(), envelope.spatial_max());
        self.tree.locate_in_envelope_intersecting(&aabb).count()
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.tree = RTree::new();
    }

    /// Number of entries in the R-tree.
    pub fn len(&self) -> usize {
        self.tree.size()
    }

    /// Returns true if the R-tree is empty.
    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }
}

impl<PK: Clone + Ord, P: SpatialPoint> Default for SpatialIndex<PK, P> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple 3D bounding box for testing.
    #[derive(Clone, Debug, PartialEq)]
    struct Box3d {
        min: [i32; 3],
        max: [i32; 3],
    }

    impl Box3d {
        fn new(min: [i32; 3], max: [i32; 3]) -> Self {
            Self { min, max }
        }

        fn point(x: i32, y: i32, z: i32) -> Self {
            Self {
                min: [x, y, z],
                max: [x, y, z],
            }
        }
    }

    impl SpatialKey for Box3d {
        type Point = [i32; 3];
        fn spatial_min(&self) -> [i32; 3] {
            self.min
        }
        fn spatial_max(&self) -> [i32; 3] {
            self.max
        }
    }

    /// A simple 2D bounding box for testing.
    #[derive(Clone, Debug, PartialEq)]
    struct Box2d {
        min: [i32; 2],
        max: [i32; 2],
    }

    impl Box2d {
        fn new(min: [i32; 2], max: [i32; 2]) -> Self {
            Self { min, max }
        }
    }

    impl SpatialKey for Box2d {
        type Point = [i32; 2];
        fn spatial_min(&self) -> [i32; 2] {
            self.min
        }
        fn spatial_max(&self) -> [i32; 2] {
            self.max
        }
    }

    #[test]
    fn spatial_index_empty() {
        let idx: SpatialIndex<u64, [i32; 3]> = SpatialIndex::new();
        assert_eq!(idx.len(), 0);
        let results = idx.intersecting(&Box3d::new([0, 0, 0], [10, 10, 10]));
        assert!(results.is_empty());
    }

    #[test]
    fn spatial_index_insert_and_query() {
        let mut idx: SpatialIndex<u64, [i32; 3]> = SpatialIndex::new();
        idx.insert(&Box3d::new([0, 0, 0], [5, 5, 5]), 1);
        idx.insert(&Box3d::new([3, 3, 3], [8, 8, 8]), 2);
        idx.insert(&Box3d::new([10, 10, 10], [15, 15, 15]), 3);

        // Query that hits first two boxes
        let results = idx.intersecting(&Box3d::new([4, 4, 4], [6, 6, 6]));
        assert_eq!(results, vec![1, 2]);

        // Query that hits only the third box
        let results = idx.intersecting(&Box3d::new([12, 12, 12], [13, 13, 13]));
        assert_eq!(results, vec![3]);

        // Query that hits nothing
        let results = idx.intersecting(&Box3d::new([20, 20, 20], [25, 25, 25]));
        assert!(results.is_empty());
    }

    #[test]
    fn spatial_index_remove() {
        let mut idx: SpatialIndex<u64, [i32; 3]> = SpatialIndex::new();
        let box1 = Box3d::new([0, 0, 0], [5, 5, 5]);
        let box2 = Box3d::new([3, 3, 3], [8, 8, 8]);
        idx.insert(&box1, 1);
        idx.insert(&box2, 2);
        assert_eq!(idx.len(), 2);

        idx.remove(&box1, &1);
        assert_eq!(idx.len(), 1);

        let results = idx.intersecting(&Box3d::new([0, 0, 0], [10, 10, 10]));
        assert_eq!(results, vec![2]);
    }

    #[test]
    fn spatial_index_clear() {
        let mut idx: SpatialIndex<u64, [i32; 3]> = SpatialIndex::new();
        idx.insert(&Box3d::new([0, 0, 0], [5, 5, 5]), 1);
        idx.insert(&Box3d::new([3, 3, 3], [8, 8, 8]), 2);
        assert_eq!(idx.len(), 2);

        idx.clear();
        assert_eq!(idx.len(), 0);
        assert!(
            idx.intersecting(&Box3d::new([0, 0, 0], [10, 10, 10]))
                .is_empty()
        );
    }

    #[test]
    fn spatial_index_deterministic_ordering() {
        let mut idx: SpatialIndex<u64, [i32; 3]> = SpatialIndex::new();
        // Insert in reverse PK order
        for pk in (1..=20).rev() {
            idx.insert(&Box3d::new([0, 0, 0], [10, 10, 10]), pk);
        }
        let results = idx.intersecting(&Box3d::new([0, 0, 0], [10, 10, 10]));
        let expected: Vec<u64> = (1..=20).collect();
        assert_eq!(results, expected);
    }

    #[test]
    fn spatial_index_point_entries() {
        let mut idx: SpatialIndex<u64, [i32; 3]> = SpatialIndex::new();
        idx.insert(&Box3d::point(5, 5, 5), 1);
        idx.insert(&Box3d::point(15, 15, 15), 2);

        // Point at (5,5,5) intersects a query box containing it
        let results = idx.intersecting(&Box3d::new([4, 4, 4], [6, 6, 6]));
        assert_eq!(results, vec![1]);

        // Point query (zero-volume box) at exact location
        let results = idx.intersecting(&Box3d::point(5, 5, 5));
        assert_eq!(results, vec![1]);
    }

    #[test]
    fn spatial_index_2d() {
        let mut idx: SpatialIndex<u32, [i32; 2]> = SpatialIndex::new();
        idx.insert(&Box2d::new([0, 0], [5, 5]), 1);
        idx.insert(&Box2d::new([3, 3], [8, 8]), 2);
        idx.insert(&Box2d::new([10, 10], [15, 15]), 3);

        let results = idx.intersecting(&Box2d::new([4, 4], [6, 6]));
        assert_eq!(results, vec![1, 2]);
    }

    #[test]
    fn spatial_index_touching_edges_intersect() {
        let mut idx: SpatialIndex<u64, [i32; 3]> = SpatialIndex::new();
        idx.insert(&Box3d::new([0, 0, 0], [5, 5, 5]), 1);

        // Query box shares an edge at x=5 — should intersect
        let results = idx.intersecting(&Box3d::new([5, 0, 0], [10, 5, 5]));
        assert_eq!(results, vec![1]);
    }

    #[test]
    fn spatial_index_remove_nonexistent_is_noop() {
        let mut idx: SpatialIndex<u64, [i32; 3]> = SpatialIndex::new();
        idx.insert(&Box3d::new([0, 0, 0], [5, 5, 5]), 1);
        assert_eq!(idx.len(), 1);

        // Remove a PK that was never inserted — should be a no-op.
        idx.remove(&Box3d::new([0, 0, 0], [5, 5, 5]), &999);
        assert_eq!(idx.len(), 1);

        // Remove with wrong AABB — should also be a no-op.
        idx.remove(&Box3d::new([50, 50, 50], [60, 60, 60]), &1);
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn spatial_index_count_intersecting() {
        let mut idx: SpatialIndex<u64, [i32; 3]> = SpatialIndex::new();
        idx.insert(&Box3d::new([0, 0, 0], [5, 5, 5]), 1);
        idx.insert(&Box3d::new([3, 3, 3], [8, 8, 8]), 2);
        idx.insert(&Box3d::new([10, 10, 10], [15, 15, 15]), 3);

        assert_eq!(idx.count_intersecting(&Box3d::new([4, 4, 4], [6, 6, 6])), 2);
        assert_eq!(
            idx.count_intersecting(&Box3d::new([20, 20, 20], [25, 25, 25])),
            0
        );
    }

    #[test]
    fn maybe_spatial_key_some() {
        let key = Box3d::new([0, 0, 0], [5, 5, 5]);
        assert!(MaybeSpatialKey::as_spatial(&key).is_some());
    }

    #[test]
    fn maybe_spatial_key_option_some() {
        let key: Option<Box3d> = Some(Box3d::new([0, 0, 0], [5, 5, 5]));
        assert!(MaybeSpatialKey::as_spatial(&key).is_some());
    }

    #[test]
    fn maybe_spatial_key_option_none() {
        let key: Option<Box3d> = None;
        assert!(MaybeSpatialKey::as_spatial(&key).is_none());
    }
}
