//! Shape model: closed enum of 0–3D primitives and boolean compositions.
//!
//! Every shape carries an optional `label`. Realization creates OCC entities
//! in the Gmsh model and records, for each label, the entities it produced.
//! Labels are turned into Gmsh physical groups by [`crate::builder::build`].

use std::collections::HashSet;
use std::ffi::CString;
use std::os::raw::{c_double, c_int};

use crate::ffi::{self, check, DimTag, FfiError};

/// Boolean operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    /// Union of all operands (n-ary, commutative).
    Union,
    /// Intersection of all operands (n-ary, commutative).
    Intersection,
    /// Difference: operands[0] − operands[1] − operands[2] … (n-ary).
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
    /// 1D full circle in the plane normal to `z`, centered at `center`.
    Circle {
        center: [f64; 3],
        radius: f64,
        label: Option<String>,
    },
    /// 2D polygon in the XY plane (z = 0), given as a list of (x, y) points.
    /// At least three points are required.
    Polygon {
        points: Vec<[f64; 2]>,
        label: Option<String>,
    },
    /// 2D disk in the XY plane.
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
    /// 3D cylinder. `axis` is the direction of the axis (it is normalized
    /// internally) and `height` its length.
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

    /// The topological dimension of the shape.
    pub fn dimension(&self) -> i32 {
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
                operands.iter().map(|s| s.dimension()).max().unwrap_or(0)
            }
        }
    }

    /// Realize this shape in isolation, in a fresh [`RealizeCtx`].
    ///
    /// Note that this touches the global Gmsh model: a session must be
    /// initialized first (see [`crate::builder::GmshContext`]).
    pub fn realize_ffi(&self) -> Result<Vec<DimTag>, RealizeError> {
        let mut ctx = RealizeCtx::new(self.dimension());
        self.realize(&mut ctx)
    }
}

/// Context maintained while realizing a shape tree into Gmsh.
///
/// Tracks which Gmsh entities are currently alive and which entities each
/// label refers to, so labels can be mapped to post-fragment entities by the
/// builder.
#[derive(Debug, Default)]
pub struct RealizeCtx {
    /// label -> entities created by that labeled shape (in insertion order)
    labels: Vec<(String, Vec<DimTag>)>,
    /// Entities that currently exist in the model, in creation order.
    live: Vec<DimTag>,
    /// The Gmsh dimension of the model being built (0–3).
    model_dim: i32,
}

impl RealizeCtx {
    pub fn new(model_dim: i32) -> Self {
        Self {
            labels: Vec::new(),
            live: Vec::new(),
            model_dim,
        }
    }

    /// The topological dimension of the model being built.
    pub fn model_dim(&self) -> i32 {
        self.model_dim
    }

    /// All entities that currently exist in the model (fragment input).
    pub fn live_dimtags(&self) -> &[DimTag] {
        &self.live
    }

    /// All labels and the entities they were attached to.
    pub fn labels(&self) -> &[(String, Vec<DimTag>)] {
        &self.labels
    }

    /// Record dimtags produced by a shape, with or without a label.
    ///
    /// # Panics
    /// Panics if `label` was used before: shape labels must be unique.
    pub fn record(&mut self, label: Option<&str>, dimtags: &[DimTag]) {
        match label {
            Some(label) => self.record_label(label, dimtags),
            None => self.record_unlabeled(dimtags),
        }
    }

    /// Record dimtags produced by a labeled shape.
    ///
    /// # Panics
    /// Panics if `label` was used before: shape labels must be unique.
    pub fn record_label(&mut self, label: &str, dimtags: &[DimTag]) {
        self.track(dimtags);
        if dimtags.is_empty() {
            return;
        }
        if self.labels.iter().any(|(l, _)| l == label) {
            panic!("duplicate shape label '{label}': shape labels must be unique");
        }
        self.labels.push((label.to_string(), dimtags.to_vec()));
    }

    /// Record dimtags without a label (still needed as fragment input).
    pub fn record_unlabeled(&mut self, dimtags: &[DimTag]) {
        self.track(dimtags);
    }

    /// Mark entities as consumed (e.g. by a boolean operation with
    /// `removeObject`/`removeTool`): they leave the live set, and labels that
    /// only referred to them are dropped.
    pub fn consume(&mut self, dimtags: &[DimTag]) {
        if dimtags.is_empty() {
            return;
        }
        let consumed: HashSet<DimTag> = dimtags.iter().copied().collect();
        self.live.retain(|dt| !consumed.contains(dt));
        self.labels.retain_mut(|(_, entities)| {
            entities.retain(|dt| !consumed.contains(dt));
            !entities.is_empty()
        });
    }

    /// Set the Gmsh entity name of an entity. Entity names are metadata; they
    /// show up in the Gmsh GUI and in exported `.msh` files.
    pub fn name_entity(&self, dim: i32, tag: i32, name: &str) -> Result<(), RealizeError> {
        let c_name = CString::new(name)?;
        let mut ierr: c_int = 0;
        unsafe { ffi::gmshModelSetEntityName(dim, tag, c_name.as_ptr(), &mut ierr) };
        check(ierr)?;
        Ok(())
    }

    fn track(&mut self, dimtags: &[DimTag]) {
        for &dt in dimtags {
            if !self.live.contains(&dt) {
                self.live.push(dt);
            }
        }
    }
}

/// Trait for realizing a shape into the Gmsh model.
pub trait Realizable {
    fn realize(&self, ctx: &mut RealizeCtx) -> Result<Vec<DimTag>, RealizeError>;
}

// ---------------------------------------------------------------------------
// OCC helpers
// ---------------------------------------------------------------------------

/// Bind pending OCC shapes into the Gmsh model.
pub fn occ_sync() -> Result<(), RealizeError> {
    let mut ierr: c_int = 0;
    unsafe { ffi::gmshModelOccSynchronize(&mut ierr) };
    check(ierr)?;
    Ok(())
}

/// Boolean operation to run through OCC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OccOp {
    Fuse,
    Cut,
    Intersect,
    Fragment,
}

/// Run an OCC boolean operation and return the resulting entities together
/// with the input → output correspondence (one entry per input entity, in
/// object-then-tool order).
fn occ_boolean(
    op: OccOp,
    objects: &[DimTag],
    tools: &[DimTag],
    remove_object: bool,
    remove_tool: bool,
) -> Result<(Vec<DimTag>, Vec<Vec<DimTag>>), RealizeError> {
    let object_flat = ffi::dimtags_to_flat(objects);
    let tool_flat = ffi::dimtags_to_flat(tools);

    let mut out_dim_tags: *mut c_int = std::ptr::null_mut();
    let mut out_dim_tags_n: usize = 0;
    let mut out_map: *mut *mut c_int = std::ptr::null_mut();
    let mut out_map_n: *mut usize = std::ptr::null_mut();
    let mut out_map_nn: usize = 0;
    let mut ierr: c_int = 0;

    unsafe {
        let common = (
            object_flat.as_ptr(),
            object_flat.len(),
            tool_flat.as_ptr(),
            tool_flat.len(),
        );
        match op {
            OccOp::Fuse => ffi::gmshModelOccFuse(
                common.0,
                common.1,
                common.2,
                common.3,
                &mut out_dim_tags,
                &mut out_dim_tags_n,
                &mut out_map,
                &mut out_map_n,
                &mut out_map_nn,
                -1,
                remove_object as c_int,
                remove_tool as c_int,
                &mut ierr,
            ),
            OccOp::Cut => ffi::gmshModelOccCut(
                common.0,
                common.1,
                common.2,
                common.3,
                &mut out_dim_tags,
                &mut out_dim_tags_n,
                &mut out_map,
                &mut out_map_n,
                &mut out_map_nn,
                -1,
                remove_object as c_int,
                remove_tool as c_int,
                &mut ierr,
            ),
            OccOp::Intersect => ffi::gmshModelOccIntersect(
                common.0,
                common.1,
                common.2,
                common.3,
                &mut out_dim_tags,
                &mut out_dim_tags_n,
                &mut out_map,
                &mut out_map_n,
                &mut out_map_nn,
                -1,
                remove_object as c_int,
                remove_tool as c_int,
                &mut ierr,
            ),
            OccOp::Fragment => ffi::gmshModelOccFragment(
                common.0,
                common.1,
                common.2,
                common.3,
                &mut out_dim_tags,
                &mut out_dim_tags_n,
                &mut out_map,
                &mut out_map_n,
                &mut out_map_nn,
                -1,
                remove_object as c_int,
                remove_tool as c_int,
                &mut ierr,
            ),
        }
    }

    // Copy everything out before releasing the arrays Gmsh allocated.
    let results = unsafe { ffi::read_dimtags(out_dim_tags, out_dim_tags_n) };
    let mut mapping: Vec<Vec<DimTag>> = Vec::with_capacity(out_map_nn);
    unsafe {
        if !out_map.is_null() {
            for i in 0..out_map_nn {
                let n = *out_map_n.add(i);
                mapping.push(ffi::read_dimtags(*out_map.add(i), n));
            }
        }
        if !out_map.is_null() {
            for i in 0..out_map_nn {
                ffi::free(*out_map.add(i) as *mut std::os::raw::c_void);
            }
        }
        ffi::free(out_map as *mut std::os::raw::c_void);
        ffi::free(out_map_n as *mut std::os::raw::c_void);
        ffi::free(out_dim_tags as *mut std::os::raw::c_void);
    }

    check(ierr)?;
    Ok((results, mapping))
}

/// Fragment all given entities (general fuse): split them at their mutual
/// intersections so that the resulting mesh has conformal interfaces.
///
/// When `remove_object` is set, the input entities are replaced by their
/// fragments, and the returned map gives, for each input entity, the
/// resulting entities.
pub fn occ_fragment(
    objects: &[DimTag],
    tools: &[DimTag],
    remove_object: bool,
    remove_tool: bool,
) -> Result<(Vec<DimTag>, Vec<Vec<DimTag>>), RealizeError> {
    occ_boolean(OccOp::Fragment, objects, tools, remove_object, remove_tool)
}

/// Create a physical group for the given entities of a single dimension.
/// Returns the tag Gmsh assigned to the group.
pub fn add_physical_group(dim: i32, tags: &[i32], name: &str) -> Result<i32, RealizeError> {
    assert!(!tags.is_empty(), "physical group must contain entities");
    let c_name = CString::new(name)?;
    let mut ierr: c_int = 0;
    let tag = unsafe {
        ffi::gmshModelAddPhysicalGroup(
            dim,
            tags.as_ptr(),
            tags.len(),
            -1,
            c_name.as_ptr(),
            &mut ierr,
        )
    };
    check(ierr)?;
    Ok(tag)
}

// ---------------------------------------------------------------------------
// Realization
// ---------------------------------------------------------------------------

/// Create an OCC point and return its tag.
fn add_point(x: f64, y: f64, z: f64) -> Result<c_int, RealizeError> {
    let mut ierr: c_int = 0;
    let tag = unsafe { ffi::gmshModelOccAddPoint(x, y, z, 0.0, -1, &mut ierr) };
    check(ierr)?;
    Ok(tag)
}

/// Synchronize, then record and name a freshly created OCC entity.
fn finish_entity(
    ctx: &mut RealizeCtx,
    dim: i32,
    tag: c_int,
    label: &Option<String>,
) -> Result<Vec<DimTag>, RealizeError> {
    occ_sync()?;
    let dimtags = vec![(dim, tag)];
    match label {
        Some(label) => {
            ctx.record_label(label, &dimtags);
            ctx.name_entity(dim, tag, label)?;
        }
        None => ctx.record_unlabeled(&dimtags),
    }
    Ok(dimtags)
}

impl Realizable for Shape {
    fn realize(&self, ctx: &mut RealizeCtx) -> Result<Vec<DimTag>, RealizeError> {
        match self {
            Shape::Point { center, label } => {
                let [x, y, z] = *center;
                let tag = add_point(x, y, z)?;
                finish_entity(ctx, 0, tag, label)
            }

            Shape::Line { start, end, label } => {
                let [x0, y0, z0] = *start;
                let [x1, y1, z1] = *end;
                let p0 = add_point(x0, y0, z0)?;
                let p1 = add_point(x1, y1, z1)?;
                let mut ierr: c_int = 0;
                let tag = unsafe { ffi::gmshModelOccAddLine(p0, p1, -1, &mut ierr) };
                check(ierr)?;
                finish_entity(ctx, 1, tag, label)
            }

            Shape::Circle {
                center,
                radius,
                label,
            } => {
                let [xc, yc, zc] = *center;
                let mut ierr: c_int = 0;
                let z_axis: [c_double; 3] = [0.0, 0.0, 1.0];
                let x_axis: [c_double; 3] = [1.0, 0.0, 0.0];
                let tag = unsafe {
                    ffi::gmshModelOccAddCircle(
                        xc,
                        yc,
                        zc,
                        *radius,
                        -1,
                        0.0,
                        2.0 * std::f64::consts::PI,
                        z_axis.as_ptr(),
                        z_axis.len(),
                        x_axis.as_ptr(),
                        x_axis.len(),
                        &mut ierr,
                    )
                };
                check(ierr)?;
                finish_entity(ctx, 1, tag, label)
            }

            Shape::Polygon { points, label } => {
                if points.len() < 3 {
                    return Err(RealizeError::InvalidShape(format!(
                        "polygon needs at least 3 points, got {}",
                        points.len()
                    )));
                }
                let mut point_tags: Vec<c_int> = Vec::with_capacity(points.len());
                for [x, y] in points {
                    point_tags.push(add_point(*x, *y, 0.0)?);
                }
                let mut line_tags: Vec<c_int> = Vec::with_capacity(point_tags.len());
                for i in 0..point_tags.len() {
                    let start = point_tags[i];
                    let end = point_tags[(i + 1) % point_tags.len()];
                    let mut ierr: c_int = 0;
                    let tag = unsafe { ffi::gmshModelOccAddLine(start, end, -1, &mut ierr) };
                    check(ierr)?;
                    line_tags.push(tag);
                }
                let mut ierr: c_int = 0;
                let loop_tag = unsafe {
                    ffi::gmshModelOccAddCurveLoop(
                        line_tags.as_ptr(),
                        line_tags.len(),
                        -1,
                        &mut ierr,
                    )
                };
                check(ierr)?;
                let wires = [loop_tag];
                let surface_tag = unsafe {
                    ffi::gmshModelOccAddPlaneSurface(wires.as_ptr(), wires.len(), -1, &mut ierr)
                };
                check(ierr)?;
                finish_entity(ctx, 2, surface_tag, label)
            }

            Shape::Disk {
                center,
                radius,
                label,
            } => {
                let [xc, yc, zc] = *center;
                let mut ierr: c_int = 0;
                let z_axis: [c_double; 3] = [0.0, 0.0, 1.0];
                let x_axis: [c_double; 3] = [1.0, 0.0, 0.0];
                let tag = unsafe {
                    ffi::gmshModelOccAddDisk(
                        xc,
                        yc,
                        zc,
                        *radius,
                        *radius,
                        -1,
                        z_axis.as_ptr(),
                        z_axis.len(),
                        x_axis.as_ptr(),
                        x_axis.len(),
                        &mut ierr,
                    )
                };
                check(ierr)?;
                finish_entity(ctx, 2, tag, label)
            }

            Shape::Rectangle { min, max, label } => {
                let [x0, y0] = *min;
                let [x1, y1] = *max;
                let mut ierr: c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddRectangle(x0, y0, 0.0, x1 - x0, y1 - y0, -1, 0.0, &mut ierr)
                };
                check(ierr)?;
                finish_entity(ctx, 2, tag, label)
            }

            Shape::Sphere {
                center,
                radius,
                label,
            } => {
                let [xc, yc, zc] = *center;
                let mut ierr: c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddSphere(
                        xc,
                        yc,
                        zc,
                        *radius,
                        -1,
                        -std::f64::consts::PI / 2.0,
                        std::f64::consts::PI / 2.0,
                        2.0 * std::f64::consts::PI,
                        &mut ierr,
                    )
                };
                check(ierr)?;
                finish_entity(ctx, 3, tag, label)
            }

            Shape::Cube { min, max, label } => {
                let [x0, y0, z0] = *min;
                let [x1, y1, z1] = *max;
                let mut ierr: c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddBox(x0, y0, z0, x1 - x0, y1 - y0, z1 - z0, -1, &mut ierr)
                };
                check(ierr)?;
                finish_entity(ctx, 3, tag, label)
            }

            Shape::Cylinder {
                center,
                axis,
                radius,
                height,
                label,
            } => {
                let [x, y, z] = *center;
                let [ax, ay, az] = *axis;
                let len = (ax * ax + ay * ay + az * az).sqrt();
                if len == 0.0 {
                    return Err(RealizeError::InvalidShape(
                        "cylinder axis must be non-zero".into(),
                    ));
                }
                // Gmsh takes the axis *vector*; its norm is the height.
                let (dx, dy, dz) = (ax / len * height, ay / len * height, az / len * height);
                let mut ierr: c_int = 0;
                let tag = unsafe {
                    ffi::gmshModelOccAddCylinder(
                        x,
                        y,
                        z,
                        dx,
                        dy,
                        dz,
                        *radius,
                        -1,
                        2.0 * std::f64::consts::PI,
                        &mut ierr,
                    )
                };
                check(ierr)?;
                finish_entity(ctx, 3, tag, label)
            }

            Shape::Boolean {
                op,
                operands,
                label,
            } => {
                if operands.is_empty() {
                    return Err(RealizeError::InvalidShape(
                        "boolean needs at least one operand".into(),
                    ));
                }

                // OCC booleans require operands of the same topological dim.
                let first_dim = operands[0].dimension();
                if operands.iter().any(|o| o.dimension() != first_dim) {
                    return Err(RealizeError::InvalidShape(format!(
                        "{op} requires operands of the same dimension"
                    )));
                }

                // Realize the operands first.
                let mut operand_dimtags: Vec<Vec<DimTag>> = Vec::with_capacity(operands.len());
                for operand in operands {
                    operand_dimtags.push(operand.realize(ctx)?);
                }

                // Fold the operands pairwise: (a op b) op c ...
                let mut acc = operand_dimtags[0].clone();
                if acc.is_empty() {
                    return Err(RealizeError::NoResultEntities(format!(
                        "first operand of {op} produced no entities"
                    )));
                }
                for next in &operand_dimtags[1..] {
                    if next.is_empty() {
                        return Err(RealizeError::NoResultEntities(format!(
                            "operand of {op} produced no entities"
                        )));
                    }
                    let occ_op = match op {
                        BooleanOp::Union => OccOp::Fuse,
                        BooleanOp::Intersection => OccOp::Intersect,
                        BooleanOp::Difference => OccOp::Cut,
                    };
                    let (out, _) = occ_boolean(occ_op, &acc, next, true, true)?;
                    occ_sync()?;
                    if out.is_empty() {
                        return Err(RealizeError::NoResultEntities(format!(
                            "{op} produced no entities"
                        )));
                    }
                    // The inputs are gone: update the live set and labels.
                    ctx.consume(&acc);
                    ctx.consume(next);
                    acc = out;
                    ctx.record_unlabeled(&acc);
                }

                // A unary boolean is the identity operation.
                if let Some(label) = label {
                    ctx.record_label(label, &acc);
                    for &(dim, tag) in &acc {
                        ctx.name_entity(dim, tag, label)?;
                    }
                }
                Ok(acc)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Error type for shape realization.
#[derive(Debug)]
pub enum RealizeError {
    Ffi(FfiError),
    NoResultEntities(String),
    InvalidShape(String),
    Gmsh(String),
}

impl std::fmt::Display for RealizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RealizeError::Ffi(e) => write!(f, "{e}"),
            RealizeError::NoResultEntities(msg) => write!(f, "no result entities: {msg}"),
            RealizeError::InvalidShape(msg) => write!(f, "invalid shape: {msg}"),
            RealizeError::Gmsh(msg) => write!(f, "Gmsh error: {msg}"),
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
        RealizeError::Gmsh(format!("string contains a null byte: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(label: Option<&str>) -> Shape {
        Shape::Cube {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 1.0],
            label: label.map(String::from),
        }
    }

    #[test]
    fn dimensions() {
        assert_eq!(cube(None).dimension(), 3);
        assert_eq!(
            Shape::Point {
                center: [0.0; 3],
                label: None
            }
            .dimension(),
            0
        );
        assert_eq!(
            Shape::Circle {
                center: [0.0; 3],
                radius: 1.0,
                label: None
            }
            .dimension(),
            1
        );
        assert_eq!(
            Shape::Cylinder {
                center: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                radius: 1.0,
                height: 2.0,
                label: None,
            }
            .dimension(),
            3
        );
        let boolean = Shape::Boolean {
            op: BooleanOp::Difference,
            operands: vec![
                cube(None),
                Shape::Disk {
                    center: [0.0; 3],
                    radius: 1.0,
                    label: None,
                },
            ],
            label: None,
        };
        assert_eq!(boolean.dimension(), 3);
    }

    #[test]
    fn labels_are_accessible() {
        assert_eq!(cube(Some("a")).label(), Some("a"));
        assert_eq!(cube(None).label(), None);
    }

    #[test]
    fn duplicate_labels_are_rejected() {
        let mut ctx = RealizeCtx::new(3);
        ctx.record_label("a", &[(3, 1)]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.record_label("a", &[(3, 2)]);
        }));
        assert!(result.is_err(), "duplicate labels must panic");
    }

    #[test]
    fn consume_drops_labels() {
        let mut ctx = RealizeCtx::new(3);
        ctx.record_label("a", &[(3, 1)]);
        ctx.record_label("b", &[(3, 2)]);
        ctx.consume(&[(3, 1)]);
        assert_eq!(ctx.labels().len(), 1);
        assert_eq!(ctx.labels()[0].0, "b");
        assert_eq!(ctx.live_dimtags(), &[(3, 2)]);
    }

    #[test]
    fn boolean_requires_same_dimensions() {
        // No Gmsh session is needed to hit the validation error.
        let shape = Shape::Boolean {
            op: BooleanOp::Difference,
            operands: vec![
                cube(None),
                Shape::Point {
                    center: [0.0; 3],
                    label: None,
                },
            ],
            label: None,
        };
        let mut ctx = RealizeCtx::new(3);
        match shape.realize(&mut ctx) {
            Err(RealizeError::InvalidShape(_)) => {}
            other => panic!("expected InvalidShape, got {other:?}"),
        }
    }
}
