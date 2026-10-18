//! geommesh — geometry construction and meshing via Gmsh.
//!
//! This crate provides a Rust API for defining 0–3D geometry from primitives,
//! combining shapes with boolean operations, and generating meshes through
//! Gmsh's C API (FFI).
//!
//! ```no_run
//! use geommesh::{BooleanOp, MeshSettings, Shape, build};
//!
//! let block = Shape::Cube {
//!     min: [0.0, 0.0, 0.0],
//!     max: [1.0, 1.0, 1.0],
//!     label: Some("block".into()),
//! };
//! let hole = Shape::Sphere {
//!     center: [0.5, 0.5, 0.5],
//!     radius: 0.2,
//!     label: Some("hole".into()),
//! };
//! let part = Shape::Boolean {
//!     op: BooleanOp::Difference,
//!     operands: vec![block, hole],
//!     label: Some("part_with_hole".into()),
//! };
//!
//! let mesh = build(&[part], &MeshSettings::default())?;
//! assert!(mesh.physical_group("part_with_hole").is_some());
//! mesh.write("part.msh")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Current phase: Phase 2 — Rust core library (shape model, builder and mesh
//! data model). Python bindings (Phase 3) are not implemented yet.

pub mod builder;
pub mod ffi;
pub mod geometry;
pub mod mesh;

pub use builder::{build, build_default, MeshSettings};
pub use ffi::DimTag;
pub use geometry::{BooleanOp, Realizable, RealizeCtx, RealizeError, Shape};
pub use mesh::{Element, ElementType, Mesh, MeshError, MeshMetadata, Node, PhysicalGroup};

/// Common imports for users of the library.
pub mod prelude {
    pub use crate::builder::{build, build_default, MeshSettings};
    pub use crate::geometry::{BooleanOp, Shape};
    pub use crate::mesh::{Element, ElementType, Mesh, MeshError, Node, PhysicalGroup};
}
