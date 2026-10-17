//! Phase 2 integration test: exercise the full build pipeline.
//! Creates a box, subtracts a sphere, builds a mesh, and verifies
//! physical groups are accessible by name.

use geommesh::builder::{build, MeshSettings};
use geommesh::geometry::{BooleanOp, Shape};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== geommesh Phase 2 integration test ===\n");

    // Create a box [0,1]^3 labeled "block"
    let block = Shape::Cube {
        min: [0.0, 0.0, 0.0],
        max: [1.0, 1.0, 1.0],
        label: Some("block".into()),
    };

    // Create a sphere labeled "hole"
    let hole = Shape::Sphere {
        center: [0.5, 0.5, 0.5],
        radius: 0.2,
        label: Some("hole".into()),
    };

    // Boolean difference: block - hole, labeled "part_with_hole"
    let part = Shape::Boolean {
        op: BooleanOp::Difference,
        operands: vec![block, hole],
        label: Some("part_with_hole".into()),
    };

    println!("Shape tree: block - hole = part_with_hole");
    println!("Shape dimension: {}", part.dimension());

    // Build the mesh
    let settings = MeshSettings {
        characteristic_length: Some(0.15),
        order: 1,
        optimize: true,
        algorithm: None,
        advanced: Vec::new(),
    };

    let mesh = build(&part, &settings)?;

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

    // Verify physical groups exist
    println!("\nPhysical groups:");
    for pg in &mesh.physical_groups {
        println!(
            "  '{}' (dim={}, tag={}, entities={:?})",
            pg.name, pg.dim, pg.tag, pg.entities
        );
    }

    // Query by name
    let block_group = mesh.physical_group("block");
    let hole_group = mesh.physical_group("hole");
    let part_group = mesh.physical_group("part_with_hole");

    println!("\nGroup lookups by name:");
    println!("  'block': {:?}", block_group.map(|g| (g.dim, g.tag)));
    println!("  'hole': {:?}", hole_group.map(|g| (g.dim, g.tag)));
    println!(
        "  'part_with_hole': {:?}",
        part_group.map(|g| (g.dim, g.tag))
    );

    // Verify elements can be retrieved by group
    if let Some(elements) = mesh.elements_in_group("part_with_hole") {
        println!(
            "\nElements in 'part_with_hole': {}",
            elements.len()
        );
    }

    // Verify mesh invariants
    assert!(mesh.metadata.node_count > 0, "mesh should have nodes");
    assert!(mesh.metadata.element_count > 0, "mesh should have elements");
    assert!(
        mesh.metadata.physical_group_count > 0,
        "mesh should have physical groups"
    );

    // Verify node coordinates are reasonable
    for node in &mesh.nodes {
        assert!(
            node.x >= 0.0 && node.x <= 1.0 + 1e-6,
            "node {} x out of range",
            node.tag
        );
        assert!(
            node.y >= 0.0 && node.y <= 1.0 + 1e-6,
            "node {} y out of range",
            node.tag
        );
        assert!(
            node.z >= 0.0 && node.z <= 1.0 + 1e-6,
            "node {} z out of range",
            node.tag
        );
    }

    println!("\nAll assertions passed!");
    println!("\n=== Phase 2 integration test complete ===");

    Ok(())
}
