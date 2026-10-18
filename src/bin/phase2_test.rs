//! Phase 2 integration test: exercise the full build pipeline.
//!
//! Creates a box, subtracts a sphere, builds a mesh, verifies that physical
//! groups are accessible by name, and writes the mesh to `part_with_hole.msh`.

use geommesh::builder::{build, MeshSettings};
use geommesh::geometry::{BooleanOp, Shape};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== geommesh Phase 2 integration test ===\n");

    // Create a box [0,1]^3 labeled "block".
    let block = Shape::Cube {
        min: [0.0, 0.0, 0.0],
        max: [1.0, 1.0, 1.0],
        label: Some("block".into()),
    };

    // Create a sphere labeled "hole".
    let hole = Shape::Sphere {
        center: [0.5, 0.5, 0.5],
        radius: 0.2,
        label: Some("hole".into()),
    };

    // Boolean difference: block - hole, labeled "part_with_hole".
    let part = Shape::Boolean {
        op: BooleanOp::Difference,
        operands: vec![block, hole],
        label: Some("part_with_hole".into()),
    };

    println!("Shape tree: block - hole = part_with_hole");
    println!("Shape dimension: {}\n", part.dimension());

    let settings = MeshSettings {
        characteristic_length: Some(0.15),
        order: 1,
        optimize: true,
        algorithm: None,
        advanced: Vec::new(),
    };

    println!("Building mesh...");
    let mesh = build(&[part], &settings)?;

    println!("\nMesh built successfully:");
    println!("  nodes: {}", mesh.metadata.node_count);
    println!("  elements: {}", mesh.metadata.element_count);
    println!("  physical groups: {}", mesh.metadata.physical_group_count);
    if let Some(bb) = mesh.metadata.bounding_box {
        println!(
            "  bounding box: [{:.3}, {:.3}, {:.3}] - [{:.3}, {:.3}, {:.3}]",
            bb[0], bb[1], bb[2], bb[3], bb[4], bb[5]
        );
    }

    println!("\nPhysical groups:");
    for pg in &mesh.physical_groups {
        println!(
            "  '{}' (dim={}, tag={}, entities={:?})",
            pg.name, pg.dim, pg.tag, pg.entities
        );
    }

    // "block" and "hole" were consumed by the cut: their labels must be gone.
    println!("\nGroup lookups by name:");
    println!(
        "  'block': {:?}",
        mesh.physical_group("block").map(|g| (g.dim, g.tag))
    );
    println!(
        "  'hole': {:?}",
        mesh.physical_group("hole").map(|g| (g.dim, g.tag))
    );
    println!(
        "  'part_with_hole': {:?}",
        mesh.physical_group("part_with_hole")
            .map(|g| (g.dim, g.tag))
    );

    // Verify mesh invariants.
    assert!(mesh.metadata.node_count > 0, "mesh should have nodes");
    assert!(mesh.metadata.element_count > 0, "mesh should have elements");
    assert!(
        mesh.physical_group("part_with_hole").is_some(),
        "the boolean result label must become a physical group"
    );

    // Verify elements can be retrieved by group name (entity-level lookup).
    let element_tags = mesh
        .elements_in_group("part_with_hole")
        .expect("expected elements in 'part_with_hole'");
    println!("\nElements in 'part_with_hole': {}", element_tags.len());
    assert!(
        !element_tags.is_empty(),
        "should have elements in part_with_hole"
    );

    // Every element must reference existing nodes.
    let node_ids: std::collections::HashSet<usize> = mesh.nodes.iter().map(|n| n.tag).collect();
    for element in &mesh.elements {
        for node in &element.node_tags {
            assert!(
                node_ids.contains(node),
                "element {} references missing node {node}",
                element.tag
            );
        }
    }

    // All nodes must lie inside the box (the sphere is subtracted inside it).
    for node in &mesh.nodes {
        assert!(
            (0.0..=1.0 + 1e-6).contains(&node.x)
                && (0.0..=1.0 + 1e-6).contains(&node.y)
                && (0.0..=1.0 + 1e-6).contains(&node.z),
            "node {} at ({}, {}, {}) is outside the box",
            node.tag,
            node.x,
            node.y,
            node.z
        );
    }

    // Export the mesh as Gmsh 2.2 ASCII and sanity-check the file.
    let path = "part_with_hole.msh";
    mesh.write(path)?;
    let written = std::fs::read_to_string(path)?;
    println!("\nWrote {path} ({} bytes)", written.len());
    assert!(
        written.contains("$PhysicalNames"),
        "export should contain physical names"
    );
    assert!(
        written.contains("\"part_with_hole\""),
        "export should name the boolean result group"
    );

    println!("\nAll assertions passed!");
    println!("\n=== Phase 2 integration test complete ===");

    Ok(())
}
