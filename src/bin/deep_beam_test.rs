//! Simple test: single rectangle
use geommesh::builder::{build, MeshSettings};
use geommesh::geometry::Shape;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Single rectangle test ===\n");

    let rect = Shape::Rectangle {
        min: [0.0, 0.0],
        max: [5.0, 4.0],
        label: Some("rect".into()),
    };

    let left_edge = Shape::Line {
        start: [0.0, 0.0, 0.0],
        end: [0.0, 4.0, 0.0],
        label: Some("left_edge".into()),
    };

    let right_edge = Shape::Line {
        start: [5.0, 0.0, 0.0],
        end: [5.0, 4.0, 0.0],
        label: Some("right_edge".into()),
    };

    let center_point = Shape::Point {
        center: [2.5, 4.0, 0.0],
        label: Some("center_point".into()),
    };

    let shapes = vec![rect, left_edge, right_edge, center_point];

    println!("Shape tree:");
    println!("  rect = Rectangle [(0,0) -> (5,4)]\n");

    let settings = MeshSettings {
        characteristic_length: Some(0.5),
        order: 1,
        optimize: true,
        algorithm: None,
        advanced: vec![],
    };

    println!("Step 1: Building mesh...");
    let mesh = build(&shapes, &settings)?;

    println!(
        "  Mesh: {} nodes, {} elements, {} physical groups",
        mesh.metadata.node_count, mesh.metadata.element_count, mesh.metadata.physical_group_count
    );

    let group = mesh
        .physical_group("rect")
        .expect("rect group should exist");
    println!(
        "  'rect': dim={}, tag={}, entities={:?}",
        group.dim, group.tag, group.entities
    );

    println!("\n=== Test Complete ===");
    Ok(())
}
