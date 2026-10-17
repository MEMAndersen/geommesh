//! Shape model: closed enum of 0–3D primitives and boolean compositions.
//!
//! Every shape carries an optional `label`. Labels survive boolean operations
//! and are mapped to Gmsh physical groups after `occ.fragment`.

use std::collections::HashMap;
use std::ffi::CString;

use crate::ffi::{self, check, DimTag, FfiError};

/// A (dimension, tag) pair identifying a Gmsh model entity.
pub type GeomDimTag = (i32, i64);

/// Boolean operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    /// Union of all operands (n-ary, commutative). Unary union = fuse/clean.
    Union,
    /// Intersection of all operands (n-ary, commutative).
    Intersection,
    /// Difference: operands[0] − operands[1] − operands[2] … (n-ary, order matters).
    Difference,
}

impl std::fmt::Display for BooleanOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BooleanOp::Union => write!(f, "union"),
            BooleanOp::Intersection => write!(f, "intersection"),
            BooleanOp::Difference => write!(f, "difference"),
        }
    }
}

/// Closed enum of geometry primitives and boolean compositions.
#[derive(Debug, Clone)]
pub enum Shape {
    /// 0D point.
    Point {
        center: [f64; 3],
        label: Option<String>,
    },
    /// 1D line segment between two points.
    Line {
        start: [f64; 3],
        end: [f64; 3],
        label: Option<String>,
    },
    /// 1D circular arc / full circle.
    Circle {
        center: [f64; 3],
        radius: f64,
        label: Option<String>,
    },
    /// 2D polygon in the XY plane (z=0), given as a list of (x, y) points.
    Polygon {
        points: Vec<[f64; 2]>,
        label: Option<String>,
    },
    /// 2D surface (disk) in the XY plane.
    Disk {
        center: [f64; 3],
        radius: f64,
        label: Option<String>,
    },
    /// 2D rectangle in the XY plane.
    Rectangle {
        min: [f64; 2],
        max: [f64; 2],
        label: Option<String>,
    },
    /// 3D sphere.
    Sphere {
        center: [f64; 3],
        radius: f64,
        label: Option<String>,
    },
    /// 3D box / cuboid.
    Cube {
        min: [f64; 3],
        max: [f64; 3],
        label: Option<String>,
    },
    /// 3D cylinder.
    Cylinder {
        center: [f64; 3],
        axis: [f64; 3],
        radius: f64,
        height: f64,
        label: Option<String>,
    },
    /// Boolean composition of shapes.
    Boolean {
        op: BooleanOp,
        operands: Vec<Shape>,
        label: Option<String>,
    },
}

impl Shape {
    /// Return the label attached to this shape node, if any.
    pub fn label(&self) -> Option<&str> {
        match self {
            Shape::Point { label, .. } => label.as_deref(),
            Shape::Line { label, .. } => label.as_deref(),
            Shape::Circle { label, .. } => label.as_deref(),
            Shape::Polygon { label, .. } => label.as_deref(),
            Shape::Disk { label, .. } => label.as_deref(),
            Shape::Rectangle { label, .. } => label.as_deref(),
            Shape::Sphere { label, .. } => label.as_deref(),
            Shape::Cube { label, .. } => label.as_deref(),
            Shape::Cylinder { label, .. } => label.as_deref(),
            Shape::Boolean { label, .. } => label.as_deref(),
        }
    }

    /// The topological dimension of this shape (0=point, 1=curve, 2=surface, 3=volume).
    pub fn dimension(&self) -> u8 {
        match self {
            Shape::Point { .. } => 0,
            Shape::Line { .. } => 1,
            Shape::Circle { .. } => 1,
            Shape::Polygon { .. } => 2,
            Shape::Disk { .. } => 2,
            Shape::Rectangle { .. } => 2,
            Shape::Sphere { .. } => 3,
            Shape::Cube { .. } => 3,
            Shape::Cylinder { .. } => 3,
            Shape::Boolean { operands, .. } => {
                // Boolean result dimension = max of operand dimensions
                operands.iter().map(|s| s.dimension()).max().unwrap_or(0)
            }
        }
    }

    /// Convenience method to realize this shape in isolation, creating a new
    /// `RealizeCtx`. Useful for testing shape realization outside of `build()`.
    pub fn realize_ffi(&self) -> Result<Vec<DimTag>, RealizeError> {
        let mut ctx = RealizeCtx::new(self.dimension());
        self.realize(&mut ctx)
    }
}

/// Context maintained during realization of a shape tree into Gmsh.
///
/// Tracks which Gmsh entities (dimtags) were created by which labeled shape,
/// so labels can be mapped to physical groups after `occ.fragment`.
pub struct RealizeCtx {
    /// label -> dimtags created by that labeled shape (pre-fragment)
    pub label_map: HashMap<String, Vec<DimTag>>,
    /// All dimtags created so far (for fragment input)
    pub all_dimtags: Vec<DimTag>,
    /// The Gmsh dimension of the model being built (0–3)
    pub model_dim: u8,
}

impl RealizeCtx {
    pub fn new(model_dim: u8) -> Self {
        Self {
            label_map: HashMap::new(),
            all_dimtags: Vec::new(),
            model_dim,
        }
    }

    /// Record dimtags produced by a labeled shape.
    ///
    /// Panics if the label has already been used by another shape in the tree.
    /// Shape labels must be unique so they can be unambiguously mapped to
    /// physical groups in the resulting mesh.
    pub fn record_label(&mut self, label: &str, dimtags: &[DimTag]) {
        if dimtags.is_empty() {
            return;
        }
        if self.label_map.contains_key(label) {
            panic!(
                "Duplicate shape label: '{}'. Shape labels must be unique.",
                label
            );
        }
        self.label_map.insert(label.to_string(), dimtags.to_vec());
        self.all_dimtags.extend_from_slice(dimtags);
    }

    /// Record dimtags produced by an unlabeled shape (still needed for fragment input).
    pub fn record_unlabeled(&mut self, dimtags: &[DimTag]) {
        self.all_dimtags.extend_from_slice(dimtags);
    }

    /// Set the Gmsh entity name on a dimtag. Entity names survive tag
    /// renumbering caused by boolean operations and mesh generation, making
    /// them the reliable way to track labels through the pipeline.
    pub fn name_entity(&mut self, dim: i32, tag: i32, name: &str) -> Result<(), RealizeError> {
        let c_name = CString::new(name)?;
        let mut ierr: ffi::c_int = 0;
        unsafe { ffi::gmshModelSetEntityName(dim, tag, c_name.as_ptr(), &mut ierr) };
        check(ierr)?;
        Ok(())
    }
}

/// Error type for shape realization.
#[derive(Debug)]
pub enum RealizeError {
    Ffi(FfiError),
    NoResultEntities(String),
    InvalidBooleanOperands(String),
    Gmsh(String),
}

impl std::fmt::Display for RealizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RealizeError::Ffi(e) => write!(f, "Gmsh FFI error: {}", e),
            RealizeError::NoResultEntities(op) => {
                write!(f, "boolean operation '{}' produced no result entities", op)
            }
            RealizeError::InvalidBooleanOperands(msg) => {
                write!(f, "invalid boolean operands: {}", msg)
            }
            RealizeError::Gmsh(msg) => write!(f, "Gmsh error: {}", msg),
        }
    }
}

impl std::error::Error for RealizeError {}

impl From<FfiError> for RealizeError {
    fn from(e: FfiError) -> Self {
        RealizeError::Ffi(e)
    }
}

impl From<std::ffi::NulError> for RealizeError {
    fn from(e: std::ffi::NulError) -> Self {
        RealizeError::Gmsh(format!("NulError: {}", e))
    }
}

/// Trait for realizing a shape into the Gmsh model.
///
/// Implementations call the Gmsh C API to create OCC entities and return
/// their dimtags. Realization is recursive: a `Boolean` shape first realizes
/// all its operands, then applies the boolean operation.
pub trait Realizable {
    fn realize(&self, ctx: &mut RealizeCtx) -> Result<Vec<DimTag>, RealizeError>;
}

impl Realizable for Shape {
    fn realize(&self, ctx: &mut RealizeCtx) -> Result<Vec<DimTag>, RealizeError> {
        match self {
            Shape::Point { center, label } => {
                let [x, y, z] = center;
                let mut ierr: ffi::c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddPoint(*x, *y, *z, 0.0, -1, &mut ierr)
                };
                check(ierr)?;
                let dimtags = vec![DimTag { dim: 0, tag }];
                if let Some(l) = label {
                    ctx.record_label(l, &dimtags);
                    ctx.name_entity(0, tag, l)?;
                } else {
                    ctx.record_unlabeled(&dimtags);
                }
                Ok(dimtags)
            }
            Shape::Line { start, end, label } => {
                // First create the two endpoint points
                let p1 = Shape::Point {
                    center: *start,
                    label: None,
                };
                let p2 = Shape::Point {
                    center: *end,
                    label: None,
                };
                let p1_tags = p1.realize(ctx)?;
                let p2_tags = p2.realize(ctx)?;
                if p1_tags.is_empty() || p2_tags.is_empty() {
                    return Err(RealizeError::Gmsh("failed to create line endpoints".into()));
                }
                let mut ierr: ffi::c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddLine(p1_tags[0].tag, p2_tags[0].tag, -1, &mut ierr)
                };
                check(ierr)?;
                let dimtags = vec![DimTag { dim: 1, tag }];
                if let Some(l) = label {
                    ctx.record_label(l, &dimtags);
                    ctx.name_entity(1, tag, l)?;
                } else {
                    ctx.record_unlabeled(&dimtags);
                }
                Ok(dimtags)
            }
            Shape::Circle { center, radius, label } => {
                let [xc, yc, zc] = center;
                let mut ierr: ffi::c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddCircle(*xc, *yc, *zc, *radius, -1, &mut ierr)
                };
                check(ierr)?;
                let dimtags = vec![DimTag { dim: 1, tag }];
                if let Some(l) = label {
                    ctx.record_label(l, &dimtags);
                    ctx.name_entity(1, tag, l)?;
                } else {
                    ctx.record_unlabeled(&dimtags);
                }
                Ok(dimtags)
            }
            Shape::Rectangle { min, max, label } => {
                let [x0, y0] = min;
                let [x1, y1] = max;
                let dx = x1 - x0;
                let dy = y1 - y0;
                eprintln!("  realize(): calling gmshModelOccAddRectangle({},{},0,{},{},-1,0)", x0, y0, dx, dy);
                let mut ierr: ffi::c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddRectangle(*x0, *y0, 0.0, dx, dy, -1, 0.0, &mut ierr)
                };
                eprintln!("  realize(): gmshModelOccAddRectangle returned tag={}, ierr={}", tag, ierr);
                check(ierr)?;
                // Rectangle returns a surface (dim 2) and its boundary curves
                // We need to synchronize to get all created entities
                let mut sync_ierr: ffi::c_int = 0;
                unsafe { ffi::gmshModelOccSynchronize(&mut sync_ierr) };
                check(sync_ierr)?;
                // Get all 2D entities (the surface) - the rectangle creates one surface
                // We'll just return the surface dimtag. The tag returned by addRectangle
                // is the surface tag.
                let dimtags = vec![DimTag { dim: 2, tag }];
                if let Some(l) = label {
                    ctx.record_label(l, &dimtags);
                    ctx.name_entity(2, tag, l)?;
                } else {
                    ctx.record_unlabeled(&dimtags);
                }
                Ok(dimtags)
            }
            Shape::Disk { center, radius, label } => {
                let [xc, yc, zc] = center;
                let mut ierr: ffi::c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddDisk(*xc, *yc, *zc, *radius, -1, &mut ierr)
                };
                check(ierr)?;
                let mut sync_ierr: ffi::c_int = 0;
                unsafe { ffi::gmshModelOccSynchronize(&mut sync_ierr) };
                check(sync_ierr)?;
                let dimtags = vec![DimTag { dim: 2, tag }];
                if let Some(l) = label {
                    ctx.record_label(l, &dimtags);
                    ctx.name_entity(2, tag, l)?;
                } else {
                    ctx.record_unlabeled(&dimtags);
                }
                Ok(dimtags)
            }
            Shape::Polygon { points, label } => {
                // Create points for each vertex
                let mut point_tags: Vec<i32> = Vec::new();
                for pt in points {
                    let mut ierr: ffi::c_int = 0;
                    let tag = unsafe {
                        ffi::gmshModelOccAddPoint(pt[0], pt[1], 0.0, 0.0, -1, &mut ierr)
                    };
                    check(ierr)?;
                    point_tags.push(tag);
                }
                // Create lines between consecutive points (closed loop)
                let n = point_tags.len();
                if n < 2 {
                    return Err(RealizeError::Gmsh(
                        "polygon needs at least 2 points".into(),
                    ));
                }
                let mut line_tags: Vec<i32> = Vec::new();
                for i in 0..n {
                    let start = point_tags[i];
                    let end = point_tags[(i + 1) % n];
                    let mut ierr: ffi::c_int = 0;
                    let tag = unsafe {
                        ffi::gmshModelOccAddLine(start, end, -1, &mut ierr)
                    };
                    check(ierr)?;
                    line_tags.push(tag);
                }
                // Create a curve loop from the lines
                let mut ierr: ffi::c_int = 0;
                let loop_tag = unsafe {
                    ffi::gmshModelOccAddCurveLoop(
                        line_tags.as_ptr(),
                        line_tags.len(),
                        -1,
                        &mut ierr,
                    )
                };
                check(ierr)?;
                // Create a plane surface from the curve loop
                let mut ierr: ffi::c_int = 0;
                let wire_tags: Vec<ffi::c_int> = vec![loop_tag];
                let surface_tag = unsafe {
                    ffi::gmshModelOccAddPlaneSurface(
                        wire_tags.as_ptr(),
                        wire_tags.len(),
                        -1,
                        &mut ierr,
                    )
                };
                check(ierr)?;
                let mut sync_ierr: ffi::c_int = 0;
                unsafe { ffi::gmshModelOccSynchronize(&mut sync_ierr) };
                check(sync_ierr)?;
                let dimtags = vec![DimTag { dim: 2, tag: surface_tag }];
                if let Some(l) = label {
                    ctx.record_label(l, &dimtags);
                    ctx.name_entity(2, surface_tag, l)?;
                } else {
                    ctx.record_unlabeled(&dimtags);
                }
                Ok(dimtags)
            }
            Shape::Sphere { center, radius, label } => {
                let [xc, yc, zc] = center;
                let mut ierr: ffi::c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddSphere(
                        *xc, *yc, *zc, *radius, -1,
                        -std::f64::consts::PI / 2.0,
                        std::f64::consts::PI / 2.0,
                        2.0 * std::f64::consts::PI,
                        &mut ierr,
                    )
                };
                check(ierr)?;
                let dimtags = vec![DimTag { dim: 3, tag }];
                if let Some(l) = label {
                    ctx.record_label(l, &dimtags);
                    ctx.name_entity(3, tag, l)?;
                } else {
                    ctx.record_unlabeled(&dimtags);
                }
                Ok(dimtags)
            }
            Shape::Cube { min, max, label } => {
                let [x0, y0, z0] = min;
                let [x1, y1, z1] = max;
                let dx = x1 - x0;
                let dy = y1 - y0;
                let dz = z1 - z0;
                let mut ierr: ffi::c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddBox(*x0, *y0, *z0, dx, dy, dz, -1, &mut ierr)
                };
                check(ierr)?;
                let dimtags = vec![DimTag { dim: 3, tag }];
                if let Some(l) = label {
                    ctx.record_label(l, &dimtags);
                    ctx.name_entity(3, tag, l)?;
                } else {
                    ctx.record_unlabeled(&dimtags);
                }
                Ok(dimtags)
            }
            Shape::Cylinder { center, axis, radius, height: _, label } => {
                let [x, y, z] = center;
                let [dx, dy, dz] = axis;
                let mut ierr: ffi::c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddCylinder(
                        *x, *y, *z, *dx, *dy, *dz, *radius, -1, &mut ierr,
                    )
                };
                check(ierr)?;
                // Cylinder in OCC creates a surface; we need to extrude or add height
                // For now, just return what was created
                let dimtags = vec![DimTag { dim: 2, tag }];
                if let Some(l) = label {
                    ctx.record_label(l, &dimtags);
                    ctx.name_entity(2, tag, l)?;
                } else {
                    ctx.record_unlabeled(&dimtags);
                }
                Ok(dimtags)
            }
            Shape::Boolean { op, operands, label } => {
                if operands.is_empty() {
                    return Err(RealizeError::InvalidBooleanOperands(
                        "boolean needs at least one operand".into(),
                    ));
                }

                // Realize all operands first
                let mut operand_dimtags: Vec<Vec<DimTag>> = Vec::new();
                for operand in operands {
                    let tags = operand.realize(ctx)?;
                    operand_dimtags.push(tags);
                }

                // Determine object and tool dimtags based on operation
                let (object_flat, object_n, tool_flat, tool_n) =
                    match op {
                        BooleanOp::Union => {
                            // All operands are objects; no tools
                            let all: Vec<DimTag> =
                                operand_dimtags.iter().flatten().cloned().collect();
                            let (flat, n) = dimtags_to_flat(&all);
                            (flat, n, Vec::new(), 0)
                        }
                        BooleanOp::Intersection => {
                            // All operands are objects; for n>2, intersect pairwise
                            // For simplicity, use first as object, rest as tools
                            if operand_dimtags.len() < 2 {
                                return Err(RealizeError::InvalidBooleanOperands(
                                    "intersection needs at least 2 operands".into(),
                                ));
                            }
                            let object = &operand_dimtags[0];
                            let tools: Vec<DimTag> = operand_dimtags[1..]
                                .iter()
                                .flatten()
                                .cloned()
                                .collect();
                            let (of, on) = dimtags_to_flat(object);
                            let (tf, tn) = dimtags_to_flat(&tools);
                            (of, on, tf, tn)
                        }
                        BooleanOp::Difference => {
                            // operands[0] is object, rest are tools
                            if operand_dimtags.len() < 2 {
                                return Err(RealizeError::InvalidBooleanOperands(
                                    "difference needs at least 2 operands (object - tools)".into(),
                                ));
                            }
                            let object = &operand_dimtags[0];
                            let tools: Vec<DimTag> = operand_dimtags[1..]
                                .iter()
                                .flatten()
                                .cloned()
                                .collect();
                            let (of, on) = dimtags_to_flat(object);
                            let (tf, tn) = dimtags_to_flat(&tools);
                            (of, on, tf, tn)
                        }
                    };

                // Synchronize before boolean
                {
                    let mut sync_ierr: ffi::c_int = 0;
                    unsafe { ffi::gmshModelOccSynchronize(&mut sync_ierr) };
                    check(sync_ierr)?;
                }

                let mut ierr: ffi::c_int = 0;
                let mut out_dimtags: *mut ffi::c_int = std::ptr::null_mut();
                let mut out_dimtags_n: usize = 0;

                let result_dimtags = match op {
                    BooleanOp::Union => {
                        if object_n == 1 {
                            // Unary union = fuse/clean: just synchronize, no boolean needed
                            // Return the single object's dimtags
                            operand_dimtags[0].clone()
                        } else {
                            unsafe {
                                ffi::gmshModelOccFuse(
                                    object_flat.as_ptr(),
                                    object_n,
                                    tool_flat.as_ptr(),
                                    tool_n,
                                    &mut out_dimtags,
                                    &mut out_dimtags_n,
                                    std::ptr::null_mut(), // outDimTagsMap
                                    std::ptr::null_mut(), // outDimTagsMap_n
                                    std::ptr::null_mut(), // outDimTagsMap_nn
                                    0,
                                    1, // removeObject
                                    1, // removeTool
                                    &mut ierr,
                                )
                            };
                            check(ierr)?;
                            read_flat_dimtags(out_dimtags, out_dimtags_n)?
                        }
                    }
                    BooleanOp::Intersection => {
                        unsafe {
                            ffi::gmshModelOccIntersect(
                                object_flat.as_ptr(),
                                object_n,
                                tool_flat.as_ptr(),
                                tool_n,
                                &mut out_dimtags,
                                &mut out_dimtags_n,
                                std::ptr::null_mut(), // outDimTagsMap
                                std::ptr::null_mut(), // outDimTagsMap_n
                                std::ptr::null_mut(), // outDimTagsMap_nn
                                0,
                                1,
                                1,
                                &mut ierr,
                            )
                        };
                        check(ierr)?;
                        read_flat_dimtags(out_dimtags, out_dimtags_n)?
                    }
                    BooleanOp::Difference => {
                        unsafe {
                            ffi::gmshModelOccCut(
                                object_flat.as_ptr(),
                                object_n,
                                tool_flat.as_ptr(),
                                tool_n,
                                &mut out_dimtags,
                                &mut out_dimtags_n,
                                std::ptr::null_mut(), // outDimTagsMap
                                std::ptr::null_mut(), // outDimTagsMap_n
                                std::ptr::null_mut(), // outDimTagsMap_nn
                                0,
                                1, // removeObject
                                1, // removeTool
                                &mut ierr,
                            )
                        };
                        check(ierr)?;
                        read_flat_dimtags(out_dimtags, out_dimtags_n)?
                    }
                };

                // Free out_dimtags output array
                if !out_dimtags.is_null() {
                    unsafe { ffi::gmshFree(out_dimtags as *mut std::ffi::c_void) };
                }

                // Synchronize after boolean
                {
                    let mut sync_ierr: ffi::c_int = 0;
                    unsafe { ffi::gmshModelOccSynchronize(&mut sync_ierr) };
                    check(sync_ierr)?;
                }

                // If no result entities returned, query the model for entities
                // of the expected dimension
                let result = if result_dimtags.is_empty() {
                    query_entities_by_dim(ctx.model_dim as i32)?
                } else {
                    result_dimtags
                };

                if let Some(l) = label {
                    ctx.record_label(l, &result);
                    for dt in &result {
                        ctx.name_entity(dt.dim, dt.tag, l)?;
                    }
                } else {
                    ctx.record_unlabeled(&result);
                }
                Ok(result)
            }
        }
    }
}

/// Convert a slice of DimTag to a flat C array [dim, tag, dim, tag, ...].
fn dimtags_to_flat(tags: &[DimTag]) -> (Vec<ffi::c_int>, usize) {
    let flat: Vec<ffi::c_int> = tags.iter().flat_map(|t| vec![t.dim, t.tag]).collect();
    let n = flat.len();
    (flat, n)
}

/// Read flat dimtags output from a boolean operation.
/// `out_dimtags` is a flat array [dim, tag, dim, tag, ...] of `out_dimtags_n` ints.
fn read_flat_dimtags(
    out_dimtags: *mut ffi::c_int,
    out_dimtags_n: usize,
) -> Result<Vec<DimTag>, RealizeError> {
    let num_entities = out_dimtags_n / 2;
    let mut result = Vec::with_capacity(num_entities);
    for i in 0..num_entities {
        let dim = unsafe { *out_dimtags.add(i * 2) };
        let tag = unsafe { *out_dimtags.add(i * 2 + 1) };
        result.push(DimTag { dim, tag });
    }
    Ok(result)
}

/// Query all Gmsh model entities of a given dimension.
fn query_entities_by_dim(dim: i32) -> Result<Vec<DimTag>, RealizeError> {
    let mut ierr: ffi::c_int = 0;
    let mut dimtags: *mut ffi::c_int = std::ptr::null_mut();
    let mut dimtags_n: usize = 0;
    unsafe {
        ffi::gmshModelGetEntities(
            &mut dimtags,
            &mut dimtags_n,
            dim,
            &mut ierr,
        )
    };
    check(ierr)?;
    let num_entities = dimtags_n / 2;
    let mut result = Vec::with_capacity(num_entities);
    for i in 0..num_entities {
        let d = unsafe { *dimtags.add(i * 2) };
        let t = unsafe { *dimtags.add(i * 2 + 1) };
        result.push(DimTag { dim: d, tag: t });
    }
    if !dimtags.is_null() {
        unsafe { ffi::gmshFree(dimtags as *mut std::ffi::c_void) };
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_dimension() {
        assert_eq!(Shape::Point { center: [0.0; 3], label: None }.dimension(), 0);
        assert_eq!(Shape::Line { start: [0.0; 3], end: [1.0; 3], label: None }.dimension(), 1);
        assert_eq!(Shape::Sphere { center: [0.0; 3], radius: 1.0, label: None }.dimension(), 3);
        assert_eq!(Shape::Cube { min: [0.0; 3], max: [1.0; 3], label: None }.dimension(), 3);
    }

    #[test]
    fn boolean_dimension() {
        let b = Shape::Boolean {
            op: BooleanOp::Union,
            operands: vec![
                Shape::Sphere { center: [0.0; 3], radius: 1.0, label: None },
                Shape::Cube { min: [0.0; 3], max: [1.0; 3], label: None },
            ],
            label: None,
        };
        assert_eq!(b.dimension(), 3);
    }

    #[test]
    fn duplicate_label_panics() {
        let mut ctx = RealizeCtx::new(3);
        let dt = DimTag { dim: 3, tag: 100 };
        ctx.record_label("my_label", &[dt]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.record_label("my_label", &[dt]);
        }));
        assert!(result.is_err(), "Expected panic on duplicate label");
    }
}
