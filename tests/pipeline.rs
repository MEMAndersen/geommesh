//! End-to-end integration tests: every test drives a real Gmsh session
//! through `geommesh::build()`.
//!
//! Gmsh is a process-wide singleton; `build()` serializes access internally,
//! so these tests may run in parallel.

use std::collections::HashMap;
use std::path::PathBuf;

use geommesh::builder::GmshContext;
use geommesh::ffi;
use geommesh::mesh::{self, ElementType};
use geommesh::{build, BooleanOp, MeshSettings, Shape};

fn cube(min: [f64; 3], max: [f64; 3], label: &str) -> Shape {
    Shape::Cube {
        min,
        max,
        label: Some(label.into()),
    }
}

fn sphere(center: [f64; 3], radius: f64, label: &str) -> Shape {
    Shape::Sphere {
        center,
        radius,
        label: Some(label.into()),
    }
}

fn settings() -> MeshSettings {
    MeshSettings {
        characteristic_length: Some(0.25),
        ..MeshSettings::default()
    }
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

/// Total measure of the elements of a physical group: volume for
/// tetrahedra, area for triangles.
fn group_measure(mesh: &geommesh::Mesh, group: &str) -> f64 {
    let node_by_tag: HashMap<usize, &geommesh::Node> =
        mesh.nodes.iter().map(|n| (n.tag, n)).collect();
    let tags = mesh.elements_in_group(group).unwrap_or_default();

    let mut measure = 0.0;
    for element in mesh.elements.iter() {
        if !tags.contains(&element.tag) {
            continue;
        }
        let p: Vec<[f64; 3]> = element
            .node_tags
            .iter()
            .map(|t| {
                let n = node_by_tag[t];
                [n.x, n.y, n.z]
            })
            .collect();
        let a = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let b = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let cross = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let norm = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        match element.element_type {
            ElementType::Tri => measure += 0.5 * norm,
            ElementType::Tetra => {
                let c = [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]];
                measure += (c[0] * cross[0] + c[1] * cross[1] + c[2] * cross[2]).abs() / 6.0;
            }
            _ => {}
        }
    }
    measure
}

#[test]
fn cube_minus_sphere() {
    let part = Shape::Boolean {
        op: BooleanOp::Difference,
        operands: vec![
            cube([0.0; 3], [1.0; 3], "block"),
            sphere([0.5, 0.5, 0.5], 0.2, "hole"),
        ],
        label: Some("part_with_hole".into()),
    };

    let mesh = build(&[part], &settings()).expect("build failed");

    assert!(mesh.metadata.node_count > 0);
    assert!(mesh.metadata.element_count > 0);

    // The result label becomes a 3D physical group; consumed operands vanish.
    let group = mesh
        .physical_group("part_with_hole")
        .expect("missing 'part_with_hole' group");
    assert_eq!(group.dim, 3);
    assert!(mesh.physical_group("block").is_none());
    assert!(mesh.physical_group("hole").is_none());

    // The group's elements are the volume elements of the box minus the sphere.
    let elements = mesh
        .elements_in_group("part_with_hole")
        .expect("no elements in 'part_with_hole'");
    assert!(!elements.is_empty());

    let volume = group_measure(&mesh, "part_with_hole");
    let expected = 1.0 - 4.0 / 3.0 * std::f64::consts::PI * 0.2_f64.powi(3);
    assert!(
        (volume - expected).abs() < 0.02,
        "volume {volume} should be close to {expected}"
    );

    // Elements only reference nodes that exist.
    let node_tags: std::collections::HashSet<usize> = mesh.nodes.iter().map(|n| n.tag).collect();
    for element in &mesh.elements {
        for node in &element.node_tags {
            assert!(node_tags.contains(node), "dangling node tag {node}");
        }
    }
}

#[test]
fn two_overlapping_cubes_are_fragmented() {
    // Two overlapping labeled cubes: fragmenting must split them into three
    // conformal pieces, and both labels must keep their full volume.
    let a = cube([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], "a");
    let b = cube([0.5, 0.0, 0.0], [1.5, 1.0, 1.0], "b");

    let mesh = build(&[a, b], &settings()).expect("build failed");

    let group_a = mesh.physical_group("a").expect("missing group 'a'");
    let group_b = mesh.physical_group("b").expect("missing group 'b'");
    assert_eq!(group_a.dim, 3);
    assert_eq!(group_b.dim, 3);

    // Both groups are non-trivial and the mesh contains the three fragments.
    assert!(mesh.elements_in_group("a").is_some_and(|e| !e.is_empty()));
    assert!(mesh.elements_in_group("b").is_some_and(|e| !e.is_empty()));

    let volume_a = group_measure(&mesh, "a");
    let volume_b = group_measure(&mesh, "b");
    assert!((volume_a - 1.0).abs() < 0.02, "volume of 'a' is {volume_a}");
    assert!((volume_b - 1.0).abs() < 0.02, "volume of 'b' is {volume_b}");

    // The bounding box covers both cubes.
    let bb = mesh.metadata.bounding_box.expect("missing bounding box");
    assert!((bb[0] - 0.0).abs() < 1e-9);
    assert!((bb[3] - 1.5).abs() < 1e-9);
}

#[test]
fn union_of_cube_and_sphere() {
    let shape = Shape::Boolean {
        op: BooleanOp::Union,
        operands: vec![
            cube([0.0; 3], [1.0; 3], "block"),
            sphere([1.0, 0.5, 0.5], 0.3, "blob"),
        ],
        label: Some("merged".into()),
    };

    let mesh = build(&[shape], &settings()).expect("build failed");
    let group = mesh.physical_group("merged").expect("missing group");
    assert_eq!(group.dim, 3);

    // The union is bigger than the cube but smaller than cube + sphere.
    let volume = group_measure(&mesh, "merged");
    let sphere_volume = 4.0 / 3.0 * std::f64::consts::PI * 0.3_f64.powi(3);
    assert!(volume > 1.0, "union volume {volume} should exceed the cube");
    assert!(
        volume < 1.0 + sphere_volume,
        "union volume {volume} too large"
    );
}

#[test]
fn nested_boolean_and_label_uniqueness() {
    // (a - b) + c, with a label on every node.
    let a = cube([0.0, 0.0, 0.0], [2.0, 2.0, 2.0], "outer");
    let b = sphere([1.0, 1.0, 1.0], 0.75, "cavity");
    let c = sphere([2.0, 1.0, 1.0], 0.5, "bump");

    let cut = Shape::Boolean {
        op: BooleanOp::Difference,
        operands: vec![a, b],
        label: Some("shell".into()),
    };
    let union = Shape::Boolean {
        op: BooleanOp::Union,
        operands: vec![cut, c],
        label: Some("assembly".into()),
    };

    let mesh = build(&[union], &settings()).expect("build failed");

    let group = mesh.physical_group("assembly").expect("missing group");
    assert_eq!(group.dim, 3);
    // (2^3 - 4/3*pi*0.75^3) + half of the 0.5 sphere sitting on the +x face.
    // The meshed sphere is a polyhedron, so allow a generous tolerance.
    let volume = group_measure(&mesh, "assembly");
    let expected = 8.0 - 4.0 / 3.0 * std::f64::consts::PI * 0.75_f64.powi(3)
        + 0.5 * 4.0 / 3.0 * std::f64::consts::PI * 0.5_f64.powi(3);
    assert!(
        (volume - expected).abs() < 0.35,
        "nested boolean volume {volume} should be close to {expected}"
    );
    assert!(mesh.physical_group("shell").is_none());
    assert!(mesh.physical_group("outer").is_none());
}

#[test]
fn line_labeled_inside_a_cube() {
    // Mixed dimensions: fragment() embeds the 1D line inside the 3D volume.
    let body = cube([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], "body");
    let axis = Shape::Line {
        start: [0.0, 0.0, 0.0],
        end: [1.0, 1.0, 1.0],
        label: Some("axis".into()),
    };

    let mesh = build(&[body, axis], &settings()).expect("build failed");

    let body_group = mesh.physical_group("body").expect("missing 'body'");
    assert_eq!(body_group.dim, 3);
    let axis_group = mesh.physical_group("axis").expect("missing 'axis'");
    assert_eq!(axis_group.dim, 1);

    // The axis is meshed with 1D elements, inside the volume mesh.
    let axis_elements = mesh
        .elements_in_group("axis")
        .expect("no 1D elements for 'axis'");
    assert!(!axis_elements.is_empty());
    let line_count = mesh
        .elements
        .iter()
        .filter(|e| e.element_type == ElementType::Line)
        .count();
    assert!(line_count >= axis_elements.len());

    // The volume is unaffected by the embedded line.
    let volume = group_measure(&mesh, "body");
    assert!((volume - 1.0).abs() < 0.02, "volume is {volume}");
}

#[test]
fn labeled_surface_splits_into_multiple_entities() {
    // A circle inside a disk splits the disk in two: both pieces keep the
    // "disk" label, and the "rim" label covers the circular curve.
    let disk = Shape::Disk {
        center: [0.0, 0.0, 0.0],
        radius: 2.0,
        label: Some("disk".into()),
    };
    let rim = Shape::Circle {
        center: [0.0, 0.0, 0.0],
        radius: 1.0,
        label: Some("rim".into()),
    };

    let mesh = build(&[disk, rim], &settings()).expect("build failed");

    let disk_group = mesh.physical_group("disk").expect("missing 'disk'");
    assert_eq!(disk_group.dim, 2);
    assert_eq!(
        disk_group.entities.len(),
        2,
        "the disk should be split into an annulus and a core"
    );

    let rim_group = mesh.physical_group("rim").expect("missing 'rim'");
    assert_eq!(rim_group.dim, 1);

    // The two surface pieces together have the area of the original disk.
    let area = group_measure(&mesh, "disk");
    let expected = std::f64::consts::PI * 4.0;
    assert!(
        (area - expected).abs() < 0.1,
        "area {area} should be ~{expected}"
    );
}

#[test]
fn exported_msh_reopens_in_gmsh() {
    let shape = cube([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], "block");
    let mesh = build(&[shape], &settings()).expect("build failed");

    let path = temp_path("geommesh_roundtrip.msh");
    mesh.write(&path).expect("write failed");

    // Re-open the file in Gmsh and compare the mesh sizes.
    let _guard = ffi::lock_gmsh();
    let _ctx = GmshContext::new().expect("Gmsh init failed");
    let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap();
    let mut ierr: std::os::raw::c_int = 0;
    unsafe { ffi::gmshOpen(c_path.as_ptr(), &mut ierr) };
    assert_eq!(ierr, 0, "Gmsh could not open {}", path.display());

    let reopened_nodes = mesh::read_nodes().expect("reading nodes from reopened file");
    let reopened_entities = mesh::get_entities().expect("reading entities");
    let reopened_elements = mesh::read_elements(&reopened_entities).expect("reading elements");
    let reopened_groups = mesh::read_physical_groups().expect("reading physical groups");

    assert_eq!(reopened_nodes.len(), mesh.nodes.len());
    assert_eq!(reopened_elements.len(), mesh.elements.len());
    let block = reopened_groups
        .iter()
        .find(|g| g.name == "block")
        .expect("'block' group missing after reopening");
    assert_eq!(block.dim, 3);
    assert!(!block.entities.is_empty());

    let _ = std::fs::remove_file(&path);
}
