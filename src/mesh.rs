//! Mesh data model: nodes, elements, physical groups, and metadata.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::os::raw::{c_double, c_int, c_void};
use std::path::Path;

use crate::ffi::{self, DimTag, FfiError};

/// A mesh node: unique tag + (x, y, z) coordinates.
#[derive(Debug, Clone)]
pub struct Node {
    pub tag: usize,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Gmsh element type codes (subset of the full Gmsh element type registry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    Line,
    Tri,
    Quad,
    Tetra,
    Hexa,
    Prism,
    Pyramid,
    Line3,
    Tri6,
    Quad9,
    Tetra10,
    Hexa27,
    Prism18,
    Pyramid14,
    Vertex,
    Quad8,
    Hexa20,
    Prism15,
    Pyramid13,
    Tri9,
    Tri10,
    /// Any element type not listed above, keeping its Gmsh type code.
    Unknown(i32),
}

impl ElementType {
    /// The Gmsh element type code.
    pub fn code(self) -> i32 {
        match self {
            ElementType::Line => 1,
            ElementType::Tri => 2,
            ElementType::Quad => 3,
            ElementType::Tetra => 4,
            ElementType::Hexa => 5,
            ElementType::Prism => 6,
            ElementType::Pyramid => 7,
            ElementType::Line3 => 8,
            ElementType::Tri6 => 9,
            ElementType::Quad9 => 10,
            ElementType::Tetra10 => 11,
            ElementType::Hexa27 => 12,
            ElementType::Prism18 => 13,
            ElementType::Pyramid14 => 14,
            ElementType::Vertex => 15,
            ElementType::Quad8 => 16,
            ElementType::Hexa20 => 17,
            ElementType::Prism15 => 18,
            ElementType::Pyramid13 => 19,
            ElementType::Tri9 => 20,
            ElementType::Tri10 => 21,
            ElementType::Unknown(code) => code,
        }
    }

    /// Number of nodes per element, if the type is known.
    pub fn num_nodes(self) -> Option<usize> {
        Some(match self {
            ElementType::Line => 2,
            ElementType::Tri => 3,
            ElementType::Quad => 4,
            ElementType::Tetra => 4,
            ElementType::Hexa => 8,
            ElementType::Prism => 6,
            ElementType::Pyramid => 5,
            ElementType::Line3 => 3,
            ElementType::Tri6 => 6,
            ElementType::Quad9 => 9,
            ElementType::Tetra10 => 10,
            ElementType::Hexa27 => 27,
            ElementType::Prism18 => 18,
            ElementType::Pyramid14 => 14,
            ElementType::Vertex => 1,
            ElementType::Quad8 => 8,
            ElementType::Hexa20 => 20,
            ElementType::Prism15 => 15,
            ElementType::Pyramid13 => 13,
            ElementType::Tri9 => 9,
            ElementType::Tri10 => 10,
            ElementType::Unknown(_) => return None,
        })
    }
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
            8 => ElementType::Line3,
            9 => ElementType::Tri6,
            10 => ElementType::Quad9,
            11 => ElementType::Tetra10,
            12 => ElementType::Hexa27,
            13 => ElementType::Prism18,
            14 => ElementType::Pyramid14,
            15 => ElementType::Vertex,
            16 => ElementType::Quad8,
            17 => ElementType::Hexa20,
            18 => ElementType::Prism15,
            19 => ElementType::Pyramid13,
            20 => ElementType::Tri9,
            21 => ElementType::Tri10,
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
    /// The model entities that belong to this group, as `(dim, entity_tag)`.
    pub entities: Vec<(i32, i32)>,
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
    ///
    /// A label that spans several dimensions produces one physical group per
    /// dimension; this returns the first (lowest dimension) match. Use
    /// [`Mesh::physical_groups_named`] to get all of them.
    pub fn physical_group(&self, name: &str) -> Option<&PhysicalGroup> {
        self.physical_groups.iter().find(|g| g.name == name)
    }

    /// All physical groups with the given name.
    pub fn physical_groups_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a PhysicalGroup> {
        self.physical_groups.iter().filter(move |g| g.name == name)
    }

    /// All element tags belonging to a physical group by name.
    ///
    /// Follows the Gmsh `.msh` model: a physical group contains entity
    /// (dim, tag) pairs; elements are classified on entities. This method
    /// finds the group's entities, then returns all elements classified on
    /// any of those entities.
    pub fn elements_in_group(&self, name: &str) -> Option<Vec<usize>> {
        let mut group_entities: Vec<(i32, i32)> = Vec::new();
        for group in self.physical_groups_named(name) {
            group_entities.extend_from_slice(&group.entities);
        }
        if group_entities.is_empty() {
            return None;
        }

        let element_tags: Vec<usize> = self
            .elements
            .iter()
            .filter(|e| group_entities.contains(&(e.dim, e.entity_tag)))
            .map(|e| e.tag)
            .collect();

        if element_tags.is_empty() {
            None
        } else {
            Some(element_tags)
        }
    }

    /// All nodes (as flat [x, y, z, ...] array).
    pub fn node_coordinates(&self) -> Vec<f64> {
        self.nodes
            .iter()
            .flat_map(|n| vec![n.x, n.y, n.z])
            .collect()
    }

    /// All element node connectivity (per element).
    pub fn element_connectivity(&self) -> Vec<Vec<usize>> {
        self.elements.iter().map(|e| e.node_tags.clone()).collect()
    }

    /// Write the mesh to disk. The format is chosen from the file extension;
    /// currently only `.msh` (Gmsh 2.2 ASCII) is supported.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<(), MeshError> {
        let path = path.as_ref();
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("msh") | None => self.write_msh(path),
            Some(other) => Err(MeshError::Io(format!(
                "unsupported mesh format '.{other}' (supported: .msh)"
            ))),
        }
    }

    /// Write the mesh as a Gmsh 2.2 ASCII `.msh` file.
    pub fn write_msh(&self, path: impl AsRef<Path>) -> Result<(), MeshError> {
        let file = File::create(path.as_ref()).map_err(|e| {
            MeshError::Io(format!("cannot create {}: {e}", path.as_ref().display()))
        })?;
        let mut w = BufWriter::new(file);
        let io = |e: std::io::Error| MeshError::Io(e.to_string());

        writeln!(w, "$MeshFormat").map_err(io)?;
        writeln!(w, "2.2 0 8").map_err(io)?;
        writeln!(w, "$EndMeshFormat").map_err(io)?;

        if !self.physical_groups.is_empty() {
            writeln!(w, "$PhysicalNames").map_err(io)?;
            writeln!(w, "{}", self.physical_groups.len()).map_err(io)?;
            for g in &self.physical_groups {
                writeln!(w, "{} {} \"{}\"", g.dim, g.tag, g.name.replace('"', "\\\""))
                    .map_err(io)?;
            }
            writeln!(w, "$EndPhysicalNames").map_err(io)?;
        }

        writeln!(w, "$Nodes").map_err(io)?;
        writeln!(w, "{}", self.nodes.len()).map_err(io)?;
        for n in &self.nodes {
            writeln!(w, "{} {} {} {}", n.tag, n.x, n.y, n.z).map_err(io)?;
        }
        writeln!(w, "$EndNodes").map_err(io)?;

        // Element (dim, entity tag) -> physical group tag.
        let mut physical_of: HashMap<(i32, i32), i32> = HashMap::new();
        for g in &self.physical_groups {
            for &entity in &g.entities {
                physical_of.entry(entity).or_insert(g.tag);
            }
        }

        writeln!(w, "$Elements").map_err(io)?;
        writeln!(w, "{}", self.elements.len()).map_err(io)?;
        for e in &self.elements {
            let physical = physical_of
                .get(&(e.dim, e.entity_tag))
                .copied()
                .unwrap_or(0);
            write!(
                w,
                "{} {} 2 {} {}",
                e.tag,
                e.element_type.code(),
                physical,
                e.entity_tag
            )
            .map_err(io)?;
            for node in &e.node_tags {
                write!(w, " {node}").map_err(io)?;
            }
            writeln!(w).map_err(io)?;
        }
        writeln!(w, "$EndElements").map_err(io)?;

        Ok(())
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
            MeshError::Ffi(e) => write!(f, "{e}"),
            MeshError::Io(msg) => write!(f, "I/O error: {msg}"),
            MeshError::Gmsh(msg) => write!(f, "Gmsh error: {msg}"),
        }
    }
}

impl std::error::Error for MeshError {}

impl From<FfiError> for MeshError {
    fn from(e: FfiError) -> Self {
        MeshError::Ffi(e)
    }
}

impl From<std::io::Error> for MeshError {
    fn from(e: std::io::Error) -> Self {
        MeshError::Io(e.to_string())
    }
}

impl From<std::ffi::NulError> for MeshError {
    fn from(e: std::ffi::NulError) -> Self {
        MeshError::Io(format!("string contains a null byte: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Reading data back from Gmsh
// ---------------------------------------------------------------------------

/// All entities currently in the Gmsh model (all dimensions).
pub fn get_entities() -> Result<Vec<DimTag>, MeshError> {
    let mut dimtags: *mut c_int = std::ptr::null_mut();
    let mut dimtags_n: usize = 0;
    let mut ierr: c_int = 0;
    unsafe { ffi::gmshModelGetEntities(&mut dimtags, &mut dimtags_n, -1, &mut ierr) };
    let result = if ierr == 0 {
        Ok(unsafe { ffi::read_dimtags(dimtags, dimtags_n) })
    } else {
        Err(MeshError::Ffi(FfiError(ierr)))
    };
    unsafe { ffi::free(dimtags as *mut c_void) };
    result
}

/// Read all mesh nodes from the Gmsh model.
pub fn read_nodes() -> Result<Vec<Node>, MeshError> {
    let mut node_tags: *mut usize = std::ptr::null_mut();
    let mut node_tags_n: usize = 0;
    let mut coords: *mut c_double = std::ptr::null_mut();
    let mut coords_n: usize = 0;
    let mut parametric: *mut c_double = std::ptr::null_mut();
    let mut parametric_n: usize = 0;
    let mut ierr: c_int = 0;
    unsafe {
        ffi::gmshModelMeshGetNodes(
            &mut node_tags,
            &mut node_tags_n,
            &mut coords,
            &mut coords_n,
            &mut parametric,
            &mut parametric_n,
            -1,
            -1,
            0,
            0,
            &mut ierr,
        );
    }

    let result = if ierr == 0 {
        let mut nodes = Vec::with_capacity(node_tags_n);
        for i in 0..node_tags_n {
            nodes.push(Node {
                tag: unsafe { *node_tags.add(i) },
                x: unsafe { *coords.add(i * 3) },
                y: unsafe { *coords.add(i * 3 + 1) },
                z: unsafe { *coords.add(i * 3 + 2) },
            });
        }
        Ok(nodes)
    } else {
        Err(MeshError::Ffi(FfiError(ierr)))
    };

    unsafe {
        ffi::free(node_tags as *mut c_void);
        ffi::free(coords as *mut c_void);
        ffi::free(parametric as *mut c_void);
    }
    result
}

/// Read the elements meshed on one model entity.
pub fn read_elements_for_entity(dim: i32, entity_tag: i32) -> Result<Vec<Element>, MeshError> {
    let mut element_types: *mut c_int = std::ptr::null_mut();
    let mut element_types_n: usize = 0;
    let mut element_tags: *mut *mut usize = std::ptr::null_mut();
    let mut element_tags_n: *mut usize = std::ptr::null_mut();
    let mut element_tags_nn: usize = 0;
    let mut node_tags: *mut *mut usize = std::ptr::null_mut();
    let mut node_tags_n: *mut usize = std::ptr::null_mut();
    let mut node_tags_nn: usize = 0;
    let mut ierr: c_int = 0;
    unsafe {
        ffi::gmshModelMeshGetElements(
            &mut element_types,
            &mut element_types_n,
            &mut element_tags,
            &mut element_tags_n,
            &mut element_tags_nn,
            &mut node_tags,
            &mut node_tags_n,
            &mut node_tags_nn,
            dim,
            entity_tag,
            &mut ierr,
        );
    }
    if ierr != 0 {
        // Release whatever Gmsh may have allocated, then report.
        unsafe {
            ffi::free(element_types as *mut c_void);
            ffi::free(element_tags as *mut c_void);
            ffi::free(element_tags_n as *mut c_void);
            ffi::free(node_tags as *mut c_void);
            ffi::free(node_tags_n as *mut c_void);
        }
        return Err(MeshError::Ffi(FfiError(ierr)));
    }

    // `element_types_n` is the number of element-type blocks; one block per
    // element type present on the entity.
    let num_blocks = element_types_n.min(element_tags_nn).min(node_tags_nn);
    let mut elements = Vec::new();

    unsafe {
        for block in 0..num_blocks {
            let element_type = ElementType::from(*element_types.add(block));
            let num_elements = *element_tags_n.add(block);
            let tags = *element_tags.add(block);
            let nodes = *node_tags.add(block);
            let nodes_per_element = element_type.num_nodes().unwrap_or_else(|| {
                // Fall back to the total node count divided by the number of
                // elements when the element type is unknown to us.
                if num_elements > 0 {
                    (*node_tags_n.add(block)) / num_elements
                } else {
                    0
                }
            });

            for i in 0..num_elements {
                let mut connectivity = Vec::with_capacity(nodes_per_element);
                for j in 0..nodes_per_element {
                    connectivity.push(*nodes.add(i * nodes_per_element + j));
                }
                elements.push(Element {
                    tag: *tags.add(i),
                    element_type,
                    node_tags: connectivity,
                    dim,
                    entity_tag,
                });
            }

            ffi::free(tags as *mut c_void);
            ffi::free(nodes as *mut c_void);
        }
        ffi::free(element_types as *mut c_void);
        ffi::free(element_tags as *mut c_void);
        ffi::free(element_tags_n as *mut c_void);
        ffi::free(node_tags as *mut c_void);
        ffi::free(node_tags_n as *mut c_void);
    }

    Ok(elements)
}

/// Read all elements of all given model entities.
pub fn read_elements(entities: &[DimTag]) -> Result<Vec<Element>, MeshError> {
    let mut elements = Vec::new();
    for &(dim, tag) in entities {
        elements.extend(read_elements_for_entity(dim, tag)?);
    }
    Ok(elements)
}

/// Read all physical groups from the Gmsh model.
///
/// Note that Gmsh may not report the entities of a physical group once the
/// mesh has been generated; `build()` tracks those entities itself.
pub fn read_physical_groups() -> Result<Vec<PhysicalGroup>, MeshError> {
    let mut dimtags: *mut c_int = std::ptr::null_mut();
    let mut dimtags_n: usize = 0;
    let mut ierr: c_int = 0;
    unsafe { ffi::gmshModelGetPhysicalGroups(&mut dimtags, &mut dimtags_n, -1, &mut ierr) };
    if ierr != 0 {
        unsafe { ffi::free(dimtags as *mut c_void) };
        return Err(MeshError::Ffi(FfiError(ierr)));
    }

    let group_dimtags = unsafe { ffi::read_dimtags(dimtags, dimtags_n) };
    unsafe { ffi::free(dimtags as *mut c_void) };

    let mut groups = Vec::with_capacity(group_dimtags.len());
    for (dim, tag) in group_dimtags {
        // Name
        let mut name_ptr: *mut std::os::raw::c_char = std::ptr::null_mut();
        let mut ierr: c_int = 0;
        unsafe { ffi::gmshModelGetPhysicalName(dim, tag, &mut name_ptr, &mut ierr) };
        if ierr != 0 {
            unsafe { ffi::free(name_ptr as *mut c_void) };
            return Err(MeshError::Ffi(FfiError(ierr)));
        }
        let name = unsafe { ffi::read_owned_string(name_ptr) };

        // Entities (plain tags within `dim`)
        let mut tags: *mut c_int = std::ptr::null_mut();
        let mut tags_n: usize = 0;
        unsafe {
            ffi::gmshModelGetEntitiesForPhysicalGroup(dim, tag, &mut tags, &mut tags_n, &mut ierr)
        };
        if ierr != 0 {
            unsafe { ffi::free(tags as *mut c_void) };
            return Err(MeshError::Ffi(FfiError(ierr)));
        }
        let entities: Vec<(i32, i32)> =
            unsafe { (0..tags_n).map(|i| (dim, *tags.add(i))).collect() };
        unsafe { ffi::free(tags as *mut c_void) };

        groups.push(PhysicalGroup {
            name,
            dim,
            tag,
            entities,
        });
    }

    Ok(groups)
}

/// Compute the bounding box of a node set.
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
        assert_eq!(ElementType::from(15), ElementType::Vertex);
        assert_eq!(ElementType::from(99), ElementType::Unknown(99));
    }

    #[test]
    fn element_type_num_nodes() {
        assert_eq!(ElementType::Tetra.num_nodes(), Some(4));
        assert_eq!(ElementType::Tri6.num_nodes(), Some(6));
        assert_eq!(ElementType::Unknown(99).num_nodes(), None);
    }

    #[test]
    fn bounding_box() {
        let nodes = vec![
            Node {
                tag: 1,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            Node {
                tag: 2,
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            Node {
                tag: 3,
                x: -1.0,
                y: 0.5,
                z: 2.0,
            },
        ];
        let bb = compute_bounding_box(&nodes).unwrap();
        assert!((bb[0] - -1.0).abs() < 1e-10);
        assert!((bb[1] - 0.0).abs() < 1e-10);
        assert!((bb[2] - 0.0).abs() < 1e-10);
        assert!((bb[3] - 1.0).abs() < 1e-10);
        assert!((bb[4] - 2.0).abs() < 1e-10);
        assert!((bb[5] - 3.0).abs() < 1e-10);
    }

    #[test]
    fn write_msh_roundtrip_shape() {
        let mesh = Mesh {
            nodes: vec![
                Node {
                    tag: 1,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                Node {
                    tag: 2,
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                Node {
                    tag: 3,
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
            ],
            elements: vec![Element {
                tag: 1,
                element_type: ElementType::Tri,
                node_tags: vec![1, 2, 3],
                dim: 2,
                entity_tag: 5,
            }],
            physical_groups: vec![PhysicalGroup {
                name: "surface".into(),
                dim: 2,
                tag: 7,
                entities: vec![(2, 5)],
            }],
            metadata: MeshMetadata {
                gmsh_version: "4.15.2".into(),
                node_count: 3,
                element_count: 1,
                physical_group_count: 1,
                bounding_box: None,
            },
        };

        let dir = std::env::temp_dir();
        let path = dir.join("geommesh_writer_test.msh");
        mesh.write(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(text.contains("$MeshFormat"));
        assert!(text.contains("2.2 0 8"));
        assert!(text.contains("$PhysicalNames"));
        assert!(text.contains("2 7 \"surface\""));
        assert!(text.contains("$Nodes"));
        assert!(text.contains("1 0 0 0"));
        assert!(text.contains("1 2 2 7 5 1 2 3"));
        assert!(text.trim_end().ends_with("$EndElements"));
    }
}
