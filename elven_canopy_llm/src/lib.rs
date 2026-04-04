//! LLM inference wrapper for Elven Canopy.
//!
//! Wraps `llama-cpp-2` to provide model loading, grammar-constrained inference,
//! and KV cache management for creature decision-making. Always compiled — every
//! build includes LLM support. GPU backends (`cuda`, `vulkan`, `rocm`) are
//! additive feature flags.
//!
//! This crate is consumed by `elven_canopy_gdext` (which hosts the inference
//! thread) and has no dependency on `elven_canopy_sim` — communication between
//! the sim and inference happens via the relay protocol's opaque payloads.

mod engine;

pub use engine::*;
