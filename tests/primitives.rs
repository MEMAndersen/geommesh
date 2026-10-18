//! Per-primitive smoke tests: every `Shape` variant must be drawable in Gmsh
//! on its own (its `realize` call) and must survive a full `build()`.
//!
//! Gmsh is a process-wide singleton; `build()` serializes access internally
//! and the raw-session test takes the same lock (`ffi::lock_gmsh()`), so these
//! tests may run in parallel.

use std::collections::HashMap;

use geommesh::builder::GmshContext;
use geommesh::ffi;
use geommesh::geometry::occ_sync;
use geommesh::mesh::{self, ElementType};
use geommesh::{build, BooleanOp, MeshSettings, Realizable, RealizeCtx, Shape};

/// Set (or replace) the label on any shape variant.
fn with_label(mut shape: Shape, name: &str) -> Shape {
    let label = Some(name.to_string());
    match &mut shape {
        Shape::Point { label: l, .. }
        | Shape::Line { label: l, .. }
        | Shape::Polyline { label: l, .. }
        | Shape::Circle { label: l, .. }
        | Shape::Polygon { label: l, .. }
        | Shape::Disk { label: l, .. }
        | Shape::Rectangle { label: l, .. }
        | Shape::Sphere { label: l, .. }
        | Shape::Cube { label: l, .. }
        | Shape::Cylinder { label: l, .. }
        | Shape::Boolean { label: l, .. } => *l = label,
    }
    shape
}

/// Every primitive variant, labeled with its case name so `build()` creates a
/// physical group with that name.
fn primitives() -> Vec<(&'static str, Shape)> {
    let cases: Vec<(&'static str, Shape)> = vec![
        (
            "point",
            Shape::Point {
                center: [0.0, 0.0, 0.0],
                label: None,
            },
        ),
        (
            "line",
            Shape::Line {
                start: [0.0, 0.0, 0.0],
                end: [1.0, 0.0, 0.0],
                label: None,
            },
        ),
        (
            "polyline",
            Shape::Polyline {
                // Open, bent, out of the XY plane: two 3D segments.
                points: vec![[0.0, 0.0, 0.0], [0.5, 0.0, 0.5], [0.5, 1.0, 0.5]],
                label: None,
            },
        ),
        (
            "circle",
            Shape::Circle {
                center: [0.0, 0.0, 0.0],
                radius: 1.0,
                label: None,
            },
        ),
        (
            "polygon",
            Shape::Polygon {
                // A square in the vertical XZ plane: exercises 3D points.
                points: vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [1.0, 0.0, 1.0],
                    [0.0, 0.0, 1.0],
                ],
                label: None,
            },
        ),
        (
            "disk",
            Shape::Disk {
                center: [0.0, 0.0, 0.0],
                radius: 1.0,
                label: None,
            },
        ),
        (
            "rectangle",
            Shape::Rectangle {
                min: [0.0, 0.0],
                max: [1.0, 1.0],
                label: None,
            },
        ),
        (
            "sphere",
            Shape::Sphere {
                center: [0.0, 0.0, 0.0],
                radius: 1.0,
                label: None,
            },
        ),
        (
            "cube",
            Shape::Cube {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 1.0],
                label: None,
            },
        ),
        (
            "cylinder",
            Shape::Cylinder {
                center: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                radius: 0.5,
                height: 2.0,
                label: None,
            },
        ),
    ];
    cases
        .into_iter()
        .map(|(name, shape)| (name, with_label(shape, name)))
        .collect()
}

fn settings() -> MeshSettings {
    MeshSettings {
        characteristic_length: Some(0.25),
        ..MeshSettings::default()
    }
}

/// Each primitive's `realize` call must create exactly one entity of the
/// expected dimension, and it must show up in the synchronized model.
#[test]
fn every_primitive_realizes_standalone() {
    let _guard = ffi::lock_gmsh();
    let mut gmsh = GmshContext::new().expect("Gmsh init failed");

    for (name, shape) in primitives() {
        // A fresh model per primitive keeps the cases independent.
        gmsh.create_model()
            .unwrap_or_else(|e| panic!("create_model failed for {name}: {e}"));

        let mut ctx = RealizeCtx::new(shape.dimension());
        let dimtags = shape
            .realize(&mut ctx)
            .unwrap_or_else(|e| panic!("realize failed for {name}: {e}"));

        // Every primitive realizes to one entity, except a polyline, which is
        // made of one edge per segment.
        let expected = match &shape {
            Shape::Polyline { points, .. } => points.len() - 1,
            _ => 1,
        };
        assert_eq!(
            dimtags.len(),
            expected,
            "{name} should realize to {expected} OCC entities, got {dimtags:?}"
        );
        for &(dim, tag) in &dimtags {
            assert_eq!(dim, shape.dimension(), "wrong dimension for {name}");
            assert!(tag > 0, "invalid tag {tag} for {name}");
        }

        occ_sync().unwrap_or_else(|e| panic!("occ_sync failed for {name}: {e}"));
        let entities = mesh::get_entities().unwrap_or_else(|e| panic!("get_entities failed: {e}"));
        for &(dim, tag) in &dimtags {
            assert!(
                entities.contains(&(dim, tag)),
                "{name} ({dim}, {tag}) missing from the model: {entities:?}"
            );
        }
    }
}

/// Each primitive must also survive the full `build()` pipeline: realize,
/// fragment, physical group, mesh generation, read-back.
#[test]
fn every_primitive_builds_a_mesh() {
    for (name, shape) in primitives() {
        let dim = shape.dimension();
        let mesh =
            build(&[shape], &settings()).unwrap_or_else(|e| panic!("build failed for {name}: {e}"));

        assert!(!mesh.nodes.is_empty(), "{name} produced no nodes");

        let group = mesh
            .physical_group(name)
            .unwrap_or_else(|| panic!("missing physical group '{name}'"));
        assert_eq!(group.dim, dim, "wrong group dimension for {name}");

        match dim {
            // A meshed point is exactly one node and one vertex element.
            0 => {
                assert_eq!(mesh.nodes.len(), 1, "{name} should mesh to one node");
                let elements = mesh
                    .elements_in_group(name)
                    .unwrap_or_else(|| panic!("no elements in group '{name}'"));
                assert_eq!(elements.len(), 1, "{name} should mesh to one element");
                let element = mesh
                    .elements
                    .iter()
                    .find(|e| elements.contains(&e.tag))
                    .unwrap_or_else(|| panic!("element of '{name}' not found"));
                assert_eq!(element.element_type, ElementType::Vertex);
            }
            _ => {
                let elements = mesh
                    .elements_in_group(name)
                    .unwrap_or_else(|| panic!("no elements in group '{name}'"));
                assert!(!elements.is_empty(), "{name} produced no elements");
            }
        }
    }
}

/// Total volume of the tetrahedral elements of a physical group.
fn tetra_volume(mesh: &geommesh::Mesh, group: &str) -> f64 {
    let by_tag: HashMap<usize, &geommesh::Node> = mesh.nodes.iter().map(|n| (n.tag, n)).collect();
    let tags = mesh.elements_in_group(group).unwrap_or_default();

    let mut volume = 0.0;
    for e in &mesh.elements {
        if e.element_type != ElementType::Tetra || !tags.contains(&e.tag) {
            continue;
        }
        let p: Vec<[f64; 3]> = e
            .node_tags
            .iter()
            .map(|t| {
                let n = by_tag[t];
                [n.x, n.y, n.z]
            })
            .collect();
        let a = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let b = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let c = [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]];
        let cross = [
            b[1] * c[2] - b[2] * c[1],
            b[2] * c[0] - b[0] * c[2],
            b[0] * c[1] - b[1] * c[0],
        ];
        volume += (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]).abs() / 6.0;
    }
    volume
}

/// All three boolean operations must produce a mesh; the measured volume
/// doubles as a correctness check (two unit cubes overlapping in a 0.5³ box).
#[test]
fn every_boolean_op_builds_a_mesh() {
    let cases = [
        (BooleanOp::Union, "union", 1.875),
        (BooleanOp::Difference, "difference", 0.875),
        (BooleanOp::Intersection, "intersection", 0.125),
    ];

    for (op, name, expected) in cases {
        let a = with_label(
            Shape::Cube {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 1.0],
                label: None,
            },
            "a",
        );
        let b = with_label(
            Shape::Cube {
                min: [0.5, 0.5, 0.5],
                max: [1.5, 1.5, 1.5],
                label: None,
            },
            "b",
        );
        let result = Shape::Boolean {
            op,
            operands: vec![a, b],
            label: Some(name.into()),
        };

        let mesh = build(&[result], &settings())
            .unwrap_or_else(|e| panic!("build failed for {name}: {e}"));

        let group = mesh
            .physical_group(name)
            .unwrap_or_else(|| panic!("missing physical group '{name}'"));
        assert_eq!(group.dim, 3, "wrong group dimension for {name}");

        let volume = tetra_volume(&mesh, name);
        assert!(
            (volume - expected).abs() < 0.01,
            "{name} volume {volume} should be {expected}"
        );

        // Operands are consumed by the boolean: their labels are gone.
        assert!(
            mesh.physical_group("a").is_none(),
            "'a' should be consumed by {name}"
        );
        assert!(
            mesh.physical_group("b").is_none(),
            "'b' should be consumed by {name}"
        );
    }
}
