//! geommesh — geometry construction and meshing via Gmsh.
//!
//! This crate provides a Rust API for defining 0–3D geometry from primitives,
//! combining shapes with boolean operations, and generating meshes through
//! Gmsh's C API (FFI).
//!
//! Current phase: Phase 2 — Rust core library with shape model, builder, and
//! mesh data model.

pub mod builder;
pub mod ffi;
pub mod geometry;
pub mod mesh;

pub use builder::{build, build_default, MeshSettings};
pub use geometry::{BooleanOp, Shape};
pub use mesh::{Element, Mesh, MeshMetadata, Node, PhysicalGroup};
