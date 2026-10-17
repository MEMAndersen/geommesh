//! Mesh data model: nodes, elements, physical groups, and metadata.

use std::collections::HashMap;

use crate::ffi::{self, check, FfiError};

/// A mesh node: unique tag + (x, y, z) coordinates.
#[derive(Debug, Clone)]
pub struct Node {
    pub tag: usize,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Gmsh element type codes (subset of the full Gmsh element type registry).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    Line = 1,
    Tri = 2,
    Quad = 3,
    Tetra = 4,
    Hexa = 5,
    Prism = 6,
    Pyramid = 7,
    Unknown(i32),
}

impl From<i32> for ElementType {
    fn from(v: i32) -> Self {
        match v {
            1 => ElementType::Line,
            2 => ElementType::Tri,
            3 => ElementType::Quad,
            4 => ElementType::Tetra,
            5 => ElementType::Hexa,
            6 => ElementType::Prism,
            7 => ElementType::Pyramid,
            v => ElementType::Unknown(v),
        }
    }
}

/// A mesh element: type, node tags, and the Gmsh entity it belongs to.
#[derive(Debug, Clone)]
pub struct Element {
    pub tag: usize,
    pub element_type: ElementType,
    pub node_tags: Vec<usize>,
    pub dim: i32,
    pub entity_tag: i32,
}

/// A physical group: name, dimension, Gmsh tag, and associated entities.
#[derive(Debug, Clone)]
pub struct PhysicalGroup {
    pub name: String,
    pub dim: i32,
    pub tag: i32,
    pub entities: Vec<(i32, i32)>, // (dim, tag) pairs
}

/// Mesh metadata.
#[derive(Debug, Clone)]
pub struct MeshMetadata {
    pub gmsh_version: String,
    pub node_count: usize,
    pub element_count: usize,
    pub physical_group_count: usize,
    pub bounding_box: Option<[f64; 6]>, // [xmin, ymin, zmin, xmax, ymax, zmax]
}

/// The mesh returned by `build()`.
///
/// Follows the Gmsh `.msh` data model:
/// - Elements are classified on model entities (dim, tag pairs)
/// - Physical groups contain entity (dim, tag) pairs
/// - To find elements in a physical group, look up the group's entities,
///   then find all elements classified on those entities.
#[derive(Debug, Clone)]
pub struct Mesh {
    pub nodes: Vec<Node>,
    pub elements: Vec<Element>,
    pub physical_groups: Vec<PhysicalGroup>,
    pub metadata: MeshMetadata,
}

impl Mesh {
    /// Look up a physical group by name.
    pub fn physical_group(&self, name: &str) -> Option<&PhysicalGroup> {
        self.physical_groups.iter().find(|g| g.name == name)
    }

    /// All element tags belonging to a physical group by name.
    ///
    /// Follows the Gmsh `.msh` model: a physical group contains entity
    /// (dim, tag) pairs; elements are classified on entities. This method
    /// finds the group's entities, then returns all elements classified on
    /// any of those entities.
    pub fn elements_in_group(&self, name: &str) -> Option<Vec<usize>> {
        let group = self.physical_group(name)?;

        // Build a map from (dim, tag) -> element tags
        let mut entity_to_elements: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for element in &self.elements {
            entity_to_elements
                .entry((element.dim, element.entity_tag))
                .or_default()
                .push(element.tag);
        }

        // Collect all element tags for entities in this physical group
        let mut element_tags: Vec<usize> = Vec::new();
        for &(dim, tag) in &group.entities {
            if let Some(tags) = entity_to_elements.get(&(dim, tag)) {
                element_tags.extend_from_slice(tags);
            }
        }

        if element_tags.is_empty() {
            None
        } else {
            Some(element_tags)
        }
    }

    /// All nodes (as flat [x, y, z, ...] array).
    pub fn node_coordinates(&self) -> Vec<f64> {
        self.nodes.iter().flat_map(|n| vec![n.x, n.y, n.z]).collect()
    }

    /// All element node connectivity (per element).
    pub fn element_connectivity(&self) -> Vec<Vec<usize>> {
        self.elements.iter().map(|e| e.node_tags.clone()).collect()
    }
}

/// Error type for mesh operations.
#[derive(Debug)]
pub enum MeshError {
    Ffi(FfiError),
    Io(String),
    Gmsh(String),
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::Ffi(e) => write!(f, "Gmsh FFI error: {}", e),
            MeshError::Io(msg) => write!(f, "I/O error: {}", msg),
            MeshError::Gmsh(msg) => write!(f, "Gmsh error: {}", msg),
        }
    }
}

impl std::error::Error for MeshError {}

impl From<FfiError> for MeshError {
    fn from(e: FfiError) -> Self {
        MeshError::Ffi(e)
    }
}

impl From<std::ffi::NulError> for MeshError {
    fn from(e: std::ffi::NulError) -> Self {
        MeshError::Io(format!("NulError: {}", e))
    }
}

/// Read all nodes from the Gmsh mesh.
pub fn read_nodes(ierr: &mut ffi::c_int) -> Result<Vec<Node>, MeshError> {
    let mut node_tags: *mut usize = std::ptr::null_mut();
    let mut node_tags_n: usize = 0;
    let mut coords: *mut ffi::c_double = std::ptr::null_mut();
    let mut coords_n: usize = 0;
    unsafe {
        ffi::gmshModelMeshGetNodes(
            &mut node_tags,
            &mut node_tags_n,
            &mut coords,
            &mut coords_n,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            -1,
            -1,
            0,
            0,
            ierr,
        )
    };
    check(*ierr)?;

    let mut nodes = Vec::with_capacity(node_tags_n);
    for i in 0..node_tags_n {
        nodes.push(Node {
            tag: unsafe { *node_tags.add(i) },
            x: unsafe { *coords.add(i * 3) },
            y: unsafe { *coords.add(i * 3 + 1) },
            z: unsafe { *coords.add(i * 3 + 2) },
        });
    }

    if !node_tags.is_null() {
        unsafe { ffi::gmshFree(node_tags as *mut std::ffi::c_void) };
    }
    if !coords.is_null() {
        unsafe { ffi::gmshFree(coords as *mut std::ffi::c_void) };
    }

    Ok(nodes)
}

/// Read all elements from the Gmsh mesh.
pub fn read_elements(ierr: &mut ffi::c_int) -> Result<Vec<Element>, MeshError> {
    let mut elem_types: *mut ffi::c_int = std::ptr::null_mut();
    let mut elem_types_n: usize = 0;
    let mut elem_tags: *mut *mut usize = std::ptr::null_mut();
    let mut elem_tags_n: *mut usize = std::ptr::null_mut();
    let mut elem_tags_nn: usize = 0;
    let mut node_tags_per_type: *mut *mut usize = std::ptr::null_mut();
    let mut node_tags_n_per_type: *mut usize = std::ptr::null_mut();
    let mut node_tags_nn: usize = 0;
    unsafe {
        ffi::gmshModelMeshGetElements(
            &mut elem_types,
            &mut elem_types_n,
            &mut elem_tags,
            &mut elem_tags_n,
            &mut elem_tags_nn,
            &mut node_tags_per_type,
            &mut node_tags_n_per_type,
            &mut node_tags_nn,
            -1,
            -1,
            ierr,
        )
    };
    check(*ierr)?;

    let mut elements = Vec::new();
    for i in 0..elem_types_n {
        let mut etype = ElementType::from(unsafe { *elem_types.add(i) });
        let n_elems = unsafe { *elem_tags_n.add(i) } as usize;
        let elem_tag_array = unsafe { *elem_tags.add(i) };
        let node_tag_array = unsafe { *node_tags_per_type.add(i) };

        for j in 0..n_elems {
            let elem_tag = unsafe { *elem_tag_array.add(j) };
            let mut e_dim: ffi::c_int = 0;
            let mut e_tag: ffi::c_int = 0;
            let mut e_node_tags: *mut usize = std::ptr::null_mut();
            let mut e_node_tags_n: usize = 0;
            unsafe {
                ffi::gmshModelMeshGetElement(
                    elem_tag,
                    &mut etype as *const _ as *mut ffi::c_int,
                    &mut e_node_tags,
                    &mut e_node_tags_n,
                    &mut e_dim,
                    &mut e_tag,
                    ierr,
                )
            };
            if *ierr != 0 {
                // Skip elements we can't read
                continue;
            }

            let mut node_vec = Vec::with_capacity(e_node_tags_n);
            for k in 0..e_node_tags_n {
                node_vec.push(unsafe { *e_node_tags.add(k) });
            }
            if !e_node_tags.is_null() {
                unsafe { ffi::gmshFree(e_node_tags as *mut std::ffi::c_void) };
            }

            elements.push(Element {
                tag: elem_tag,
                element_type: etype,
                node_tags: node_vec,
                dim: e_dim,
                entity_tag: e_tag,
            });
        }

        // Free per-type arrays
        if !elem_tag_array.is_null() {
            unsafe { ffi::gmshFree(elem_tag_array as *mut std::ffi::c_void) };
        }
        if !node_tag_array.is_null() {
            unsafe { ffi::gmshFree(node_tag_array as *mut std::ffi::c_void) };
        }
    }

    if !elem_types.is_null() {
        unsafe { ffi::gmshFree(elem_types as *mut std::ffi::c_void) };
    }
    if !elem_tags.is_null() {
        unsafe { ffi::gmshFree(elem_tags as *mut std::ffi::c_void) };
    }
    if !elem_tags_n.is_null() {
        unsafe { ffi::gmshFree(elem_tags_n as *mut std::ffi::c_void) };
    }
    if !node_tags_per_type.is_null() {
        unsafe { ffi::gmshFree(node_tags_per_type as *mut std::ffi::c_void) };
    }
    if !node_tags_n_per_type.is_null() {
        unsafe { ffi::gmshFree(node_tags_n_per_type as *mut std::ffi::c_void) };
    }

    Ok(elements)
}

/// Read all physical groups from the Gmsh model.
pub fn read_physical_groups(ierr: &mut ffi::c_int) -> Result<Vec<PhysicalGroup>, MeshError> {
    let mut dimtags: *mut ffi::c_int = std::ptr::null_mut();
    let mut dimtags_n: usize = 0;
    unsafe {
        ffi::gmshModelGetPhysicalGroups(
            &mut dimtags,
            &mut dimtags_n,
            -1,
            ierr,
        )
    };
    check(*ierr)?;

    let num_groups = dimtags_n / 2;
    let mut groups = Vec::with_capacity(num_groups);

    for i in 0..num_groups {
        let dim = unsafe { *dimtags.add(i * 2) };
        let tag = unsafe { *dimtags.add(i * 2 + 1) };

        // Get name
        let mut name_ptr: *mut ffi::c_char = std::ptr::null_mut();
        unsafe {
            ffi::gmshModelGetPhysicalName(dim, tag, &mut name_ptr, ierr)
        };
        check(*ierr)?;

        let name = if !name_ptr.is_null() {
            let len = (0..).find(|&k| unsafe { *name_ptr.add(k) == 0 }).unwrap_or(0);
            let bytes = unsafe { std::slice::from_raw_parts(name_ptr as *const u8, len) };
            let s = String::from_utf8_lossy(bytes).to_string();
            unsafe { ffi::gmshFree(name_ptr as *mut std::ffi::c_void) };
            s
        } else {
            format!("group_{}", tag)
        };

        // Get entities in this group
        let mut ent_dimtags: *mut ffi::c_int = std::ptr::null_mut();
        let mut ent_dimtags_n: usize = 0;
        unsafe {
            ffi::gmshModelGetEntitiesForPhysicalGroup(
                dim,
                tag,
                &mut ent_dimtags,
                &mut ent_dimtags_n,
                ierr,
            )
        };
        check(*ierr)?;

        let num_entities = ent_dimtags_n / 2;
        let mut entities = Vec::with_capacity(num_entities);
        for j in 0..num_entities {
            let e_dim = unsafe { *ent_dimtags.add(j * 2) };
            let e_tag = unsafe { *ent_dimtags.add(j * 2 + 1) };
            entities.push((e_dim, e_tag));
        }
        if !ent_dimtags.is_null() {
            unsafe { ffi::gmshFree(ent_dimtags as *mut std::ffi::c_void) };
        }

        groups.push(PhysicalGroup {
            name,
            dim,
            tag,
            entities,
        });
    }

    if !dimtags.is_null() {
        unsafe { ffi::gmshFree(dimtags as *mut std::ffi::c_void) };
    }

    Ok(groups)
}

/// Compute bounding box from nodes.
pub fn compute_bounding_box(nodes: &[Node]) -> Option<[f64; 6]> {
    if nodes.is_empty() {
        return None;
    }
    let mut xmin = nodes[0].x;
    let mut ymin = nodes[0].y;
    let mut zmin = nodes[0].z;
    let mut xmax = nodes[0].x;
    let mut ymax = nodes[0].y;
    let mut zmax = nodes[0].z;
    for n in &nodes[1..] {
        xmin = xmin.min(n.x);
        ymin = ymin.min(n.y);
        zmin = zmin.min(n.z);
        xmax = xmax.max(n.x);
        ymax = ymax.max(n.y);
        zmax = zmax.max(n.z);
    }
    Some([xmin, ymin, zmin, xmax, ymax, zmax])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_type_from_i32() {
        assert_eq!(ElementType::from(1), ElementType::Line);
        assert_eq!(ElementType::from(4), ElementType::Tetra);
        assert_eq!(ElementType::from(99), ElementType::Unknown(99));
    }

    #[test]
    fn bounding_box() {
        let nodes = vec![
            Node { tag: 1, x: 0.0, y: 0.0, z: 0.0 },
            Node { tag: 2, x: 1.0, y: 2.0, z: 3.0 },
            Node { tag: 3, x: -1.0, y: 0.5, z: 2.0 },
        ];
        let bb = compute_bounding_box(&nodes).unwrap();
        assert!((bb[0] - -1.0).abs() < 1e-10);
        assert!((bb[1] - 0.0).abs() < 1e-10);
        assert!((bb[2] - 0.0).abs() < 1e-10);
        assert!((bb[3] - 1.0).abs() < 1e-10);
        assert!((bb[4] - 2.0).abs() < 1e-10);
        assert!((bb[5] - 3.0).abs() < 1e-10);
    }
}
