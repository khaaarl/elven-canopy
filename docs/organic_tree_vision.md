# Organic Tree Generation — Vision Document

## Overview

The tree generation system uses **energy-based recursive segment growth** to produce natural-looking trees. The core insight: **the trunk is just the first branch**. A single unified algorithm grows all segments (trunk, branches, sub-branches, roots) by allocating an energy budget that determines length, radius, and splitting behavior. Different tree species — oaks, conifers, willows, fantasy mega-trees — emerge from tuning the same parameter set.

## V1: Core Recursive Growth (This Branch)

### Energy Model

Every segment starts with an energy budget. As it grows step-by-step:
- **Radius** tapers proportionally to remaining energy: `radius = sqrt(energy * energy_to_radius)`
- **Energy per step** is consumed at a fixed rate
- When energy is exhausted, the segment terminates
- At each step, the segment may **split**, dividing energy between a continuation and one or more child branches

### Segment Growth

Each segment is a `SegmentJob` processed from a FIFO work queue (breadth-first by generation for determinism):
- **Direction**: initial direction + gravitropism + random deflection with coherence
- **Gravitropism**: pulls upward for branches (`gravitropism > 0`) or downward for roots (`root_gravitropism > 0`)
- **Random deflection**: small angular perturbations with coherence (successive steps tend toward the same deflection direction, creating smooth curves rather than zigzags)
- **Splitting**: probabilistic, controlled by `split_chance_base` and `min_progress_for_split`

### Voxel Classification

- **Generation 0, non-root** = `VoxelType::Trunk`
- **Generation 1+, non-root** = `VoxelType::Branch`
- **Root segments** = `VoxelType::Root`
- **Terminal positions** = `VoxelType::Leaf` (blobs at branch endpoints)

Placement priority: Trunk > Branch > Root > Leaf (higher-priority types are never overwritten).

### Roots

Roots use the same growth algorithm, seeded at the trunk base directed outward and downward:
- `root_energy_fraction` of total energy goes to roots
- `root_initial_count` root segments share root energy equally
- `root_gravitropism` pulls roots downward
- `root_surface_tendency` keeps roots near y=0 (surface roots for v1)
- Root voxels are classified as `VoxelType::Root`

### Leaves

After all segments are grown, leaf blobs are placed at terminal positions of non-root segments:
- `leaf_shape`: Sphere or Cloud (cloud = vertically compressed ellipsoid)
- `leaf_density`: probability of placing each voxel within the blob radius
- `leaf_size`: radius of leaf blobs
- `canopy_density`: overall scaling factor

### Tree Profiles (Presets)

Named constructors produce different tree archetypes by tuning the same parameters:

- **`fantasy_mega()`**: The default — a towering mega-tree with thick trunk, wide spread, many splits. High initial energy, moderate gravitropism, generous root system.
- **`oak()`**: Broad spreading crown, thick trunk, moderate height. High split chance, wide split angles, strong horizontal tendency.
- **`conifer()`**: Tall and narrow, strong central leader. Low split chance, narrow split angles, strong upward gravitropism.
- **`willow()`**: Drooping branches, negative gravitropism on higher generations. High split count, branches curve downward.

### Config Structure

```
TreeProfile
  +-- GrowthParams      (initial_energy, energy_to_radius, min_radius, growth_step_length, energy_per_step)
  +-- SplitParams       (split_chance_base, split_count, split_energy_ratio, split_angle, split_angle_variance, min_progress_for_split)
  +-- CurvatureParams   (gravitropism, random_deflection, deflection_coherence)
  +-- RootParams        (root_energy_fraction, root_initial_count, root_gravitropism, root_initial_angle, root_surface_tendency)
  +-- LeafParams        (leaf_shape, leaf_density, leaf_size, canopy_density)
  +-- TrunkParams       (base_flare, initial_direction)
```

### Navigation

Root voxels integrate into the existing nav graph:
- Walking on roots: `EdgeType::BranchWalk`
- Root-to-ground transitions: `EdgeType::ForestFloor`
- Root-to-trunk transitions: `EdgeType::TrunkClimb`

No new `EdgeType` variants needed for v1.

## Deferred Features (Future Branches)

### Aerial Roots
Roots that grow from branches downward to the ground, creating natural columns. Would use the same segment growth algorithm with a modified gravitropism that targets the ground plane.

### Hollow Trunks
Large trees develop hollow interiors over time. Interior spaces could serve as sheltered areas for elves. Implementation: after trunk generation, carve out interior voxels above a certain trunk radius threshold.

### Fused Trees
Multiple trees growing close together can fuse at contact points, creating shared canopy structures. Implementation: detect voxel overlap between trees and merge their nav graphs.

### Bark Color Parameters
Per-species bark coloration driven by config. The renderer would read color values from the tree profile rather than using hardcoded browns.

### Dynamic Growth
Trees that grow over time in response to mana investment. Segments extend gradually, new splits appear, the canopy fills out. Implementation: store the `SegmentJob` queue as part of tree state and process a few jobs per heartbeat.

### Candelabra Splitting
A specialized split pattern where the trunk terminates and multiple equal-energy branches emerge from the crown, creating the flat-topped candelabra shape seen in some tropical trees. Implementation: a `split_style` enum on `SplitParams`.

### Wind Response
Subtle sway animation driven by a noise function. Branch tips deflect more than trunk. Implementation: a vertex shader in Godot that displaces based on world position and time.
