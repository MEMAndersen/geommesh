//! Mixed-dimension test: a rectangle with its boundary edges and a point.
use geommesh::builder::{build, MeshSettings};
use geommesh::geometry::Shape;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Mixed-dimension rectangle test ===\n");

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
    println!("  rect        = Rectangle [(0,0) -> (5,4)]");
    println!("  left_edge   = Line [(0,0) -> (0,4)]");
    println!("  right_edge  = Line [(5,0) -> (5,4)]");
    println!("  center_point= Point (2.5, 4)");

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

    println!("\nStep 2: Verifying every shape survived into the mesh...");

    // Every labeled shape must come back as a physical group of the right
    // dimension, with mesh elements assigned to it.
    let expected: [(&str, i32); 4] = [
        ("rect", 2),
        ("left_edge", 1),
        ("right_edge", 1),
        ("center_point", 0),
    ];

    let mut missing = Vec::new();
    for (name, dim) in expected {
        match mesh.physical_group(name) {
            None => {
                missing.push(name);
                println!("  '{name}': MISSING");
            }
            Some(group) if group.dim != dim => {
                missing.push(name);
                println!(
                    "  '{name}': WRONG DIMENSION dim={} (expected {dim})",
                    group.dim
                );
            }
            Some(group) => {
                let elements = mesh.elements_in_group(name).unwrap_or_default();
                if elements.is_empty() {
                    missing.push(name);
                    println!("  '{name}': dim={}, but no elements", group.dim);
                } else {
                    println!(
                        "  '{name}': dim={}, entities={:?}, {} elements",
                        group.dim,
                        group.entities,
                        elements.len()
                    );
                }
            }
        }
    }

    if !missing.is_empty() {
        return Err(format!("shapes missing from the mesh: {}", missing.join(", ")).into());
    }

    println!(
        "\n=== Test Complete: all {} shapes survived ===",
        expected.len()
    );
    Ok(())
}
