//! Phase 1 prototype: exercise the raw Gmsh FFI end-to-end.
//!
//! Creates a box and a sphere via OCC, subtracts the sphere from the box
//! (boolean cut), runs occ.fragment, names the resulting entities as physical
//! groups, generates a mesh, and reads back nodes, elements and physical
//! groups. Prints results to stdout.
//!
//! This binary deliberately talks to `geommesh::ffi` directly (no builder) so
//! that it can serve as a low-level smoke test of the bindings.

use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use geommesh::ffi::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== geommesh Phase 1 prototype ===\n");

    let mut ierr: c_int = 0;
    unsafe { gmshInitialize(0, ptr::null_mut(), 1, 0, &mut ierr) };
    check(ierr)?;
    println!("Gmsh initialized (API v4.15.2)");

    let result = run_prototype();

    unsafe { gmshFinalize(&mut ierr) };
    println!("\nGmsh finalized.");

    result
}

fn run_prototype() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        // --- 1. Fresh model ---
        let mut ierr: c_int = 0;
        gmshClear(&mut ierr);
        check(ierr)?;
        let model_name = CString::new("prototype")?;
        gmshModelAdd(model_name.as_ptr(), &mut ierr);
        check(ierr)?;

        // --- 2. Create a box (cube): [0,1]^3 ---
        println!("Creating box [0,1]^3 ...");
        let box_tag = gmshModelOccAddBox(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 100, &mut ierr);
        check(ierr)?;
        println!("  box tag = {box_tag}");

        // --- 3. Create a sphere at (0.5, 0.5, 0.5), radius 0.2 ---
        println!("Creating sphere center=(0.5,0.5,0.5) radius=0.2 ...");
        let sphere_tag = gmshModelOccAddSphere(
            0.5,
            0.5,
            0.5,
            0.2,
            200,
            -std::f64::consts::PI / 2.0,
            std::f64::consts::PI / 2.0,
            2.0 * std::f64::consts::PI,
            &mut ierr,
        );
        check(ierr)?;
        println!("  sphere tag = {sphere_tag}");

        // --- 4. Synchronize OCC with the Gmsh model ---
        gmshModelOccSynchronize(&mut ierr);
        check(ierr)?;
        println!("OCC synchronized.");

        // Give the box a name: does it survive the boolean? (used for debugging)
        let box_name = CString::new("box")?;
        gmshModelSetEntityName(3, box_tag, box_name.as_ptr(), &mut ierr);
        check(ierr)?;

        // --- 5. Boolean cut: box - sphere ---
        println!("Boolean cut: box - sphere ...");
        let objects: Vec<DimTag> = vec![(3, box_tag)];
        let tools: Vec<DimTag> = vec![(3, sphere_tag)];
        let objects_flat = dimtags_to_flat(&objects);
        let tools_flat = dimtags_to_flat(&tools);

        let mut out_dim_tags: *mut c_int = ptr::null_mut();
        let mut out_dim_tags_n: usize = 0;
        let mut out_map: *mut *mut c_int = ptr::null_mut();
        let mut out_map_n: *mut usize = ptr::null_mut();
        let mut out_map_nn: usize = 0;

        gmshModelOccCut(
            objects_flat.as_ptr(),
            objects_flat.len(),
            tools_flat.as_ptr(),
            tools_flat.len(),
            &mut out_dim_tags,
            &mut out_dim_tags_n,
            &mut out_map,
            &mut out_map_n,
            &mut out_map_nn,
            -1, // tag: let Gmsh assign
            1,  // removeObject
            1,  // removeTool
            &mut ierr,
        );
        check(ierr)?;

        let cut_results = read_dimtags(out_dim_tags, out_dim_tags_n);
        println!(
            "  cut -> {} entity(ies): {:?} (map entries: {})",
            cut_results.len(),
            cut_results,
            out_map_nn
        );

        // Free the arrays Gmsh allocated for the cut.
        if !out_map.is_null() {
            for i in 0..out_map_nn {
                free(*out_map.add(i) as *mut c_void);
            }
        }
        free(out_map as *mut c_void);
        free(out_map_n as *mut c_void);
        free(out_dim_tags as *mut c_void);

        gmshModelOccSynchronize(&mut ierr);
        check(ierr)?;

        // --- 6. Query all 3D entities, and print their names ---
        let mut all_entities: *mut c_int = ptr::null_mut();
        let mut all_entities_n: usize = 0;
        gmshModelGetEntities(&mut all_entities, &mut all_entities_n, 3, &mut ierr);
        check(ierr)?;
        let all_dimtags = read_dimtags(all_entities, all_entities_n);
        free(all_entities as *mut c_void);
        println!("  3D entities after cut: {all_dimtags:?}");

        for &(dim, tag) in &all_dimtags {
            let mut name_ptr: *mut c_char = ptr::null_mut();
            gmshModelGetEntityName(dim, tag, &mut name_ptr, &mut ierr);
            check(ierr)?;
            let name = read_owned_string(name_ptr);
            println!("    ({dim},{tag}) name = {name:?}");
        }

        // --- 7. occ.fragment on the result entities ---
        let fragment_input = if cut_results.is_empty() {
            all_dimtags.clone()
        } else {
            cut_results.clone()
        };
        println!("Running occ.fragment on {fragment_input:?} ...");

        let frag_objects_flat = dimtags_to_flat(&fragment_input);
        let empty: Vec<DimTag> = Vec::new();
        let frag_tools_flat = dimtags_to_flat(&empty);

        let mut frag_out: *mut c_int = ptr::null_mut();
        let mut frag_out_n: usize = 0;
        let mut frag_map: *mut *mut c_int = ptr::null_mut();
        let mut frag_map_n: *mut usize = ptr::null_mut();
        let mut frag_map_nn: usize = 0;

        gmshModelOccFragment(
            frag_objects_flat.as_ptr(),
            frag_objects_flat.len(),
            frag_tools_flat.as_ptr(),
            frag_tools_flat.len(),
            &mut frag_out,
            &mut frag_out_n,
            &mut frag_map,
            &mut frag_map_n,
            &mut frag_map_nn,
            -1,
            1, // removeObject
            1, // removeTool
            &mut ierr,
        );
        check(ierr)?;

        let fragment_dimtags = read_dimtags(frag_out, frag_out_n);
        println!(
            "  fragment produced {} entity(ies): {:?}",
            fragment_dimtags.len(),
            fragment_dimtags
        );
        for i in 0..frag_map_nn {
            let mapped = read_dimtags(*frag_map.add(i), *frag_map_n.add(i));
            if i < fragment_input.len() {
                println!("    input {:?} -> {mapped:?}", fragment_input[i]);
            }
        }

        if !frag_map.is_null() {
            for i in 0..frag_map_nn {
                free(*frag_map.add(i) as *mut c_void);
            }
        }
        free(frag_map as *mut c_void);
        free(frag_map_n as *mut c_void);
        free(frag_out as *mut c_void);

        gmshModelOccSynchronize(&mut ierr);
        check(ierr)?;

        // --- 8. Create a physical group for the resulting volumes ---
        // Gmsh refuses to fragment "too few arguments" (fewer than two
        // entities), so fall back to the fragment input in that case.
        let grouped_entities = if fragment_dimtags.is_empty() {
            fragment_input.clone()
        } else {
            fragment_dimtags.clone()
        };
        let volume_tags: Vec<c_int> = grouped_entities
            .iter()
            .filter(|(dim, _)| *dim == 3)
            .map(|(_, tag)| *tag)
            .collect();
        if volume_tags.is_empty() {
            println!("  no 3D fragment entities; skipping physical group");
        } else {
            let name = CString::new("box_minus_sphere")?;
            let phys_tag = gmshModelAddPhysicalGroup(
                3,
                volume_tags.as_ptr(),
                volume_tags.len(),
                -1,
                name.as_ptr(),
                &mut ierr,
            );
            check(ierr)?;
            println!("  physical group 'box_minus_sphere' (tag={phys_tag}) -> {volume_tags:?}");
        }

        // --- 9. Generate the mesh ---
        println!("\nGenerating 3D mesh ...");
        gmshModelMeshGenerate(3, &mut ierr);
        check(ierr)?;
        println!("Mesh generated.");

        // --- 10. Read back nodes ---
        let mut node_tags: *mut usize = ptr::null_mut();
        let mut node_tags_n: usize = 0;
        let mut coords: *mut c_double = ptr::null_mut();
        let mut coords_n: usize = 0;
        gmshModelMeshGetNodes(
            &mut node_tags,
            &mut node_tags_n,
            &mut coords,
            &mut coords_n,
            ptr::null_mut(),
            ptr::null_mut(),
            -1, // dim: all
            -1, // tag: all
            0,  // includeBoundary
            0,  // returnParametricCoord
            &mut ierr,
        );
        check(ierr)?;
        println!(
            "  nodes: {} tags, {} coordinates",
            node_tags_n,
            coords_n / 3
        );
        for i in 0..node_tags_n.min(3) {
            println!(
                "    node {} = ({:.6}, {:.6}, {:.6})",
                *node_tags.add(i),
                *coords.add(i * 3),
                *coords.add(i * 3 + 1),
                *coords.add(i * 3 + 2)
            );
        }

        // --- 11. Read back elements ---
        let mut element_types: *mut c_int = ptr::null_mut();
        let mut element_types_n: usize = 0;
        let mut element_tags: *mut *mut usize = ptr::null_mut();
        let mut element_tags_n: *mut usize = ptr::null_mut();
        let mut element_tags_nn: usize = 0;
        let mut elem_nodes: *mut *mut usize = ptr::null_mut();
        let mut elem_nodes_n: *mut usize = ptr::null_mut();
        let mut elem_nodes_nn: usize = 0;

        gmshModelMeshGetElements(
            &mut element_types,
            &mut element_types_n,
            &mut element_tags,
            &mut element_tags_n,
            &mut element_tags_nn,
            &mut elem_nodes,
            &mut elem_nodes_n,
            &mut elem_nodes_nn,
            -1, // dim: all
            -1, // tag: all
            &mut ierr,
        );
        check(ierr)?;

        let mut total_elements: usize = 0;
        for block in 0..element_tags_nn.min(elem_nodes_nn) {
            let element_type = *element_types.add(block);
            let count = *element_tags_n.add(block);
            total_elements += count;
            println!("  element type {element_type}: {count} element(s)");
        }
        println!("  total elements: {total_elements}");

        // --- 12. Read back physical groups ---
        let mut phys_dimtags: *mut c_int = ptr::null_mut();
        let mut phys_dimtags_n: usize = 0;
        gmshModelGetPhysicalGroups(&mut phys_dimtags, &mut phys_dimtags_n, -1, &mut ierr);
        check(ierr)?;
        println!("\nPhysical groups: {}", phys_dimtags_n / 2);
        for i in 0..(phys_dimtags_n / 2) {
            let dim = *phys_dimtags.add(i * 2);
            let tag = *phys_dimtags.add(i * 2 + 1);

            let mut name_ptr: *mut c_char = ptr::null_mut();
            gmshModelGetPhysicalName(dim, tag, &mut name_ptr, &mut ierr);
            check(ierr)?;
            let name = read_owned_string(name_ptr);

            let mut entity_tags: *mut c_int = ptr::null_mut();
            let mut entity_tags_n: usize = 0;
            gmshModelGetEntitiesForPhysicalGroup(
                dim,
                tag,
                &mut entity_tags,
                &mut entity_tags_n,
                &mut ierr,
            );
            check(ierr)?;
            let entities: Vec<i32> = (0..entity_tags_n).map(|j| *entity_tags.add(j)).collect();
            free(entity_tags as *mut c_void);

            println!("  group '{name}' (dim={dim}, tag={tag}) -> entities {entities:?}");
        }
        free(phys_dimtags as *mut c_void);

        // --- 13. Free the node/element arrays ---
        free(node_tags as *mut c_void);
        free(coords as *mut c_void);
        for i in 0..element_tags_nn {
            free(*element_tags.add(i) as *mut c_void);
        }
        for i in 0..elem_nodes_nn {
            free(*elem_nodes.add(i) as *mut c_void);
        }
        free(element_tags as *mut c_void);
        free(element_tags_n as *mut c_void);
        free(elem_nodes as *mut c_void);
        free(elem_nodes_n as *mut c_void);
        free(element_types as *mut c_void);

        println!("\n=== Phase 1 prototype complete ===");
        Ok(())
    }
}
