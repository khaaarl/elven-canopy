# Hierarchical Nav Oracle

**Status:** Speculative draft

## Problem

With 100+ creatures, per-creature A* on a large nav graph becomes the dominant
performance cost — the Dwarf Fortress problem. Paths are expensive to compute and
frequently invalidated by world changes.

## Core idea

Partition the nav graph into a tree of groups based on **connectivity**, not spatial
proximity. Leaves are walkable/climbable voxels. Connected leaves cluster into groups
(bounded by max intra-group diameter). Groups sharing member-level edges connect at
the next level. Recurse to the root.

This is a **direction oracle**: given positions A and B, find their lowest common
ancestor (LCA) in the group tree — O(log n) — then route toward the boundary of the
target's ancestor group. Per-step cost is O(log n + b) where b is the boundary node
count of the current group. No full path computed or cached; creatures steer
reactively. Local A* within a group is trivial given bounded size; the exact query
strategy (pure oracle vs. local A* vs. hybrid) is a tuning decision.

## Why connectivity-based

Elven Canopy's topology is spatially irregular — platforms, bridges, stairs, trunk
surfaces. Spatial partitioning (grid/quadtree) cuts across natural boundaries.
Connectivity grouping surfaces the real structure: platforms as groups, bridges and
stairs as inter-group edges. Choke points emerge automatically.

On flat terrain, groups have wide shared boundaries (b is large), but pathfinding
there is trivial anyway. The hierarchy earns its keep in complex vertical topology.

## Connectivity oracle

Beyond pathfinding: "can A reach B?" is O(log n) — check whether they share a root.
"Which regions disconnected when this bridge was destroyed?" falls out of group-split
detection.

## Movement modalities

Each movement type / speed ratio combination (walk vs climb) requires a separate
hierarchy. Flyers have a fundamentally different (denser) graph.

## Update cost

**Adding a node/edge:** joins an existing group or merges two. Bounded by group size
plus O(log n) propagation up the tree.

**Removing a node/edge:** may split a group if it was a bridge within it, cascading
upward. Uncommon (construction > destruction). Per-group spanning trees detect splits.

With bounded group diameter, blast radius is bounded. Amortized cost is low for a
mostly-static world.

## Determinism and persistence

Unlike the current nav graph (derived state, rebuilt on load), this hierarchy is
**sim state** — partitioning decisions affect creature behavior and must be identical
across players. This means:

- Deterministic initial construction (seeded PRNG for tie-breaking)
- All mutations through SimCommand (canonically ordered)
- Full serialization in saves (incremental updates produce different partitions than
  building from scratch, so cannot be re-derived)

## Open questions

- Partitioning heuristic: greedy BFS from seed nodes likely sufficient given natural
  cluster structure. Quality may degrade over many updates; periodic rebalancing.
- Group size cap: smaller groups mean cheaper local routing but taller trees.
- Path optimality: the oracle routes on graph topology, not spatial distance, so
  U-shaped paths are handled correctly. Suboptimality may arise from coarse
  boundary-node selection within a group, but not from directional misrouting.
