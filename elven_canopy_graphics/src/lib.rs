// elven_canopy_graphics — chunk mesh generation and related graphics code.
//
// This crate contains all voxel mesh generation, smoothing, decimation, and
// texture generation code. It is a pure Rust library with no Godot
// dependencies. The companion `elven_canopy_gdext` crate converts the output
// types into Godot `ArrayMesh` objects for rendering.
//
// Depends on `elven_canopy_sim` for core voxel types (`VoxelCoord`,
// `VoxelType`, `VoxelWorld`).
//
// Module overview:
// - `mesh_gen.rs`:         Chunk-based voxel mesh generation with smooth surface rendering.
// - `smooth_mesh.rs`:      Smooth mesh pipeline: subdivision, anchoring, chamfer, smoothing.
// - `mesh_decimation.rs`:  QEM edge-collapse decimation + coplanar retri + collinear collapse.
// - `texture_gen.rs`:      Procedural face texture generation (kept for reference, not active).
// - `chunk_neighborhood.rs`: Self-contained voxel snapshot for off-thread mesh generation.
//
// See `elven_canopy_sim` for the simulation logic, `elven_canopy_sprites` for
// procedural creature/fruit sprite generation.

pub mod chunk_neighborhood;
pub mod mesh_decimation;
pub mod mesh_gen;
pub mod smooth_mesh;
pub mod texture_gen;
