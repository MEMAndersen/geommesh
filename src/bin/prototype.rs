//! Phase 1 prototype: exercise Gmsh FFI end-to-end.
//!
//! Creates a box and a sphere via OCC, subtracts the sphere from the box
//! (boolean cut), runs occ.fragment, names the resulting entities as physical
//! groups, generates a mesh, and reads back nodes, elements and physical
//! groups. Prints results to stdout.

use geommesh::ffi::*;
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== geommesh Phase 1 prototype ===\n");

    // --- 1. Initialize Gmsh ---
    unsafe {
        let mut ierr: c_int = 0;
        gmshInitialize(0, ptr::null_mut(), 1, 0, &mut ierr);
        check(ierr)?;
        println!("Gmsh initialized (API v4.15.2)");
    }

    // Ensure cleanup on exit.
    let result = run_prototype();

    unsafe {
        let mut ierr: c_int = 0;
        gmshFinalize(&mut ierr);
        check(ierr)?;
        println!("\nGmsh finalized.");
    }

    result
}

fn run_prototype() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        // --- 2. Clear any existing model ---
        let mut ierr: c_int = 0;
        gmshClear(&mut ierr);
        check(ierr)?;

        // --- 3. Create a box (cube): [0,1]^3 ---
        println!("Creating box [0,1]^3 ...");
        let box_tag = gmshModelOccAddBox(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 100, &mut ierr);
        check(ierr)?;
        println!("  box tag = {}", box_tag);

        // --- 4. Create a sphere at (0.5, 0.5, 0.5), radius 0.2 ---
        println!("Creating sphere center=(0.5,0.5,0.5) radius=0.2 ...");
        let sphere_tag = gmshModelOccAddSphere(0.5, 0.5, 0.5, 0.2, 200, -std::f64::consts::PI / 2.0, std::f64::consts::PI / 2.0, 2.0 * std::f64::consts::PI, &mut ierr);
        check(ierr)?;
        println!("  sphere tag = {}", sphere_tag);

        // --- 5. Synchronize OCC with the Gmsh model ---
        gmshModelOccSynchronize(&mut ierr);
        check(ierr)?;
        println!("OCC synchronized.");

        // --- 6. Boolean cut: box - sphere ---
        println!("Boolean cut: box - sphere ...");
        let object: Vec<DimTag> = vec![DimTag { dim: 3, tag: box_tag }];
        let tools: Vec<DimTag> = vec![DimTag { dim: 3, tag: sphere_tag }];
        let (obj_flat, obj_n) = dimtags_to_ffi(&object);
        let (tool_flat, tool_n) = dimtags_to_ffi(&tools);

        let mut out_dimtags: *mut c_int = ptr::null_mut();
        let mut out_dimtags_n: usize = 0;
        let mut out_map: *mut *mut c_int = ptr::null_mut();
        let mut out_map_n: *mut *mut usize = ptr::null_mut();
        let mut out_map_nn: usize = 0;

        gmshModelOccCut(
            obj_flat.as_ptr(),
            obj_n,
            tool_flat.as_ptr(),
            tool_n,
            &mut out_dimtags,
            &mut out_dimtags_n,
            &mut out_map,
            out_map_n,
            &mut out_map_nn,
            0, // tag: let Gmsh assign
            1, // removeObject
            1, // removeTool
            &mut ierr,
        );
        println!("  cut ierr = {}", ierr);
        check(ierr)?;

        // Synchronize OCC boolean result back to the Gmsh model.
        gmshModelOccSynchronize(&mut ierr);
        println!("  sync after cut ierr = {}", ierr);
        check(ierr)?;

        // Query all 3D entities in the model after the cut.
        let mut all_entities: *mut c_int = ptr::null_mut();
        let mut all_entities_n: usize = 0;
        gmshModelGetEntities(
            &mut all_entities,
            &mut all_entities_n,
            3, // dim: 3D only
            &mut ierr,
        );
        check(ierr)?;
        let num_entities = all_entities_n / 2;
        println!("  getEntities ierr = {}, n_ints = {}, n_entities = {}", ierr, all_entities_n, num_entities);
        let mut all_dimtags: Vec<DimTag> = Vec::new();
        for i in 0..num_entities {
            let dim = *all_entities.add(i * 2);
            let tag = *all_entities.add(i * 2 + 1);
            all_dimtags.push(DimTag { dim, tag });
        }
        println!("  all 3D entities after cut: {:?}", all_dimtags);
        gmshFree(all_entities as *mut c_void);

        let mut result_dimtags: Vec<DimTag> = Vec::new();
        let num_cut_entities = out_dimtags_n / 2;
        for i in 0..num_cut_entities {
            let dim = *out_dimtags.add(i * 2);
            let tag = *out_dimtags.add(i * 2 + 1);
            result_dimtags.push(DimTag { dim, tag });
        }
        println!(
            "  cut outDimTags: {} entity(ies): {:?}",
            num_cut_entities,
            result_dimtags
        );

        // Record pre-fragment labels
        let fragment_input = if !result_dimtags.is_empty() {
            result_dimtags.clone()
        } else {
            all_dimtags.clone()
        };
        let pre_fragment_labels: Vec<(String, Vec<DimTag>)> =
            vec![("box_minus_sphere".to_string(), fragment_input.clone())];

        // Free the cut output arrays
        gmshFree(out_dimtags as *mut c_void);
        if !out_map.is_null() {
            for i in 0..num_cut_entities {
                let map_ptr = *out_map.add(i);
                if !map_ptr.is_null() {
                    gmshFree(map_ptr as *mut c_void);
                }
                let map_n_ptr = *out_map_n.add(i);
                if !map_n_ptr.is_null() {
                    gmshFree(map_n_ptr as *mut c_void);
                }
            }
            gmshFree(out_map_n as *mut c_void);
            gmshFree(out_map as *mut c_void);
        }

        // --- 7. occ.fragment on the result entities ---
        println!("Running occ.fragment on result entities ...");
        let (frag_obj_flat, frag_obj_n) = dimtags_to_ffi(&fragment_input);
        // Empty tools: fragment self-interfaces
        let empty: Vec<DimTag> = vec![];
        let (frag_tool_flat, frag_tool_n) = dimtags_to_ffi(&empty);

        let mut frag_out_dimtags: *mut c_int = ptr::null_mut();
        let mut frag_out_dimtags_n: usize = 0;
        let mut frag_out_map: *mut *mut c_int = ptr::null_mut();
        let mut frag_out_map_n: *mut *mut usize = ptr::null_mut();
        let mut frag_out_map_nn: usize = 0;

        gmshModelOccFragment(
            frag_obj_flat.as_ptr(),
            frag_obj_n,
            frag_tool_flat.as_ptr(),
            frag_tool_n,
            &mut frag_out_dimtags,
            &mut frag_out_dimtags_n,
            &mut frag_out_map,
            frag_out_map_n,
            &mut frag_out_map_nn,
            0,
            0, // removeObject
            0, // removeTool
            &mut ierr,
        );
        check(ierr)?;

        // Synchronize OCC fragment result back to the Gmsh model.
        gmshModelOccSynchronize(&mut ierr);
        check(ierr)?;

        let mut fragment_dimtags: Vec<DimTag> = Vec::new();
        let num_frag_entities = frag_out_dimtags_n / 2;
        for i in 0..num_frag_entities {
            let dim = *frag_out_dimtags.add(i * 2);
            let tag = *frag_out_dimtags.add(i * 2 + 1);
            fragment_dimtags.push(DimTag { dim, tag });
        }
        println!(
            "  fragment produced {} entity(ies): {:?}",
            num_frag_entities,
            fragment_dimtags
        );

        // Free fragment output arrays
        gmshFree(frag_out_dimtags as *mut c_void);
        if !frag_out_map.is_null() {
            for i in 0..num_frag_entities {
                let map_ptr = *frag_out_map.add(i);
                if !map_ptr.is_null() {
                    gmshFree(map_ptr as *mut c_void);
                }
                let map_n_ptr = *frag_out_map_n.add(i);
                if !map_n_ptr.is_null() {
                    gmshFree(map_n_ptr as *mut c_void);
                }
            }
            gmshFree(frag_out_map_n as *mut c_void);
            gmshFree(frag_out_map as *mut c_void);
        }

        // --- 8. Map pre-fragment labels to post-fragment entities ---
        // For this simple prototype, all fragment result entities inherit the
        // label "box_minus_sphere". In a more complex scenario we'd use the
        // outDimTagsMap correspondence to track which pre-fragment entities
        // map to which post-fragment entities.
        println!("\nMapping labels to post-fragment entities ...");
        for (_label, _pre_entities) in &pre_fragment_labels {
            let label = "box_minus_sphere";
            // Collect 3D entities from the fragment result for this label
            let vol_entities: Vec<c_int> = fragment_dimtags
                .iter()
                .filter(|dt| dt.dim == 3)
                .map(|dt| dt.tag)
                .collect();

            if !vol_entities.is_empty() {
                let name = std::ffi::CString::new(label)?;
                let phys_tag = gmshModelAddPhysicalGroup(
                    3, // dim
                    vol_entities.as_ptr(),
                    vol_entities.len(),
                    -1, // auto-assign tag
                    name.as_ptr(),
                    &mut ierr,
                );
                check(ierr)?;
                println!(
                    "  physical group '{}' (tag={}) -> volume entities {:?}",
                    label, phys_tag, vol_entities
                );
            }
        }

        // --- 9. Generate mesh ---
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

        // Print first few nodes
        for i in 0..node_tags_n.min(5) {
            println!(
                "    node {} = ({:.6}, {:.6}, {:.6})",
                *node_tags.add(i),
                *coords.add(i * 3),
                *coords.add(i * 3 + 1),
                *coords.add(i * 3 + 2)
            );
        }

        // --- 11. Read back elements ---
        let mut elem_types: *mut c_int = ptr::null_mut();
        let mut elem_types_n: usize = 0;
        let mut elem_tags: *mut *mut usize = ptr::null_mut();
        let mut elem_tags_n: *mut usize = ptr::null_mut();
        let mut elem_tags_nn: usize = 0;
        let mut node_tags_per_type: *mut *mut usize = ptr::null_mut();
        let mut node_tags_n_per_type: *mut usize = ptr::null_mut();
        let mut node_tags_nn: usize = 0;

        gmshModelMeshGetElements(
            &mut elem_types,
            &mut elem_types_n,
            &mut elem_tags,
            &mut elem_tags_n,
            &mut elem_tags_nn,
            &mut node_tags_per_type,
            &mut node_tags_n_per_type,
            &mut node_tags_nn,
            -1, // dim: all
            -1, // tag: all
            &mut ierr,
        );
        check(ierr)?;

        let mut total_elements: usize = 0;
        for i in 0..elem_types_n {
            let etype = *elem_types.add(i);
            let n_elems = *elem_tags_n.add(i);
            total_elements += n_elems;
            println!(
                "  element type {}: {} elements",
                etype, n_elems
            );
        }
        println!("  total elements: {}", total_elements);

        // --- 12. Read back physical groups ---
        let mut phys_dimtags: *mut c_int = ptr::null_mut();
        let mut phys_dimtags_n: usize = 0;
        gmshModelGetPhysicalGroups(
            &mut phys_dimtags,
            &mut phys_dimtags_n,
            -1, // all dimensions
            &mut ierr,
        );
        check(ierr)?;
        println!("\nPhysical groups: {}", phys_dimtags_n / 2);
        for i in 0..(phys_dimtags_n / 2) {
            let dim = *phys_dimtags.add(i * 2);
            let tag = *phys_dimtags.add(i * 2 + 1);

            let mut name_ptr: *mut c_char = ptr::null_mut();
            gmshModelGetPhysicalName(dim, tag, &mut name_ptr, &mut ierr);
            check(ierr)?;

            let name = if !name_ptr.is_null() {
                let len = (0..).find(|&k| *name_ptr.add(k) == 0).unwrap_or(0);
                let bytes = std::slice::from_raw_parts(name_ptr as *const u8, len);
                let s = String::from_utf8_lossy(bytes).to_string();
                gmshFree(name_ptr as *mut c_void);
                s
            } else {
                String::new()
            };

            // Get entities in this physical group
            let mut ent_dimtags: *mut c_int = ptr::null_mut();
            let mut ent_dimtags_n: usize = 0;
            gmshModelGetEntitiesForPhysicalGroup(
                dim,
                tag,
                &mut ent_dimtags,
                &mut ent_dimtags_n,
                &mut ierr,
            );
            check(ierr)?;

            let mut entities: Vec<(i32, i32)> = Vec::new();
            let num_ent_entities = ent_dimtags_n / 2;
            for j in 0..num_ent_entities {
                let e_dim = *ent_dimtags.add(j * 2);
                let e_tag = *ent_dimtags.add(j * 2 + 1);
                entities.push((e_dim, e_tag));
            }
            gmshFree(ent_dimtags as *mut c_void);

            println!(
                "  group '{}' (dim={}, tag={}) -> entities {:?}",
                name, dim, tag, entities
            );
        }
        gmshFree(phys_dimtags as *mut c_void);

        // --- 13. Free allocated arrays ---
        free_size_array(node_tags);
        free_double_array(coords);
        free_size_ptr_array(elem_tags, elem_types_n);
        free_size_ptr_array(node_tags_per_type, elem_types_n);
        if !elem_tags_n.is_null() {
            gmshFree(elem_tags_n as *mut c_void);
        }
        if !node_tags_n_per_type.is_null() {
            gmshFree(node_tags_n_per_type as *mut c_void);
        }
        free_int_array(elem_types);

        println!("\n=== Phase 1 prototype complete ===");
        Ok(())
    }
}

