//! Builder: MeshSettings, GmshContext lifecycle, and the top-level `build()`
//! function that realizes a shape tree, runs `occ.fragment`, creates physical
//! groups, generates a mesh, and reads it back into a Rust `Mesh`.

use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::ffi::{self, check, DimTag, FfiError};
use crate::geometry::{self, Realizable, RealizeCtx, Shape};
use crate::mesh::{self, Mesh, MeshError};

/// Meshing settings exposed to users.
#[derive(Debug, Clone)]
pub struct MeshSettings {
    /// Target characteristic element size (hint; Gmsh may deviate).
    pub characteristic_length: Option<f64>,
    /// Mesh element order: 1 = linear, 2 = quadratic.
    pub order: u8,
    /// Run mesh optimization after generation.
    pub optimize: bool,
    /// Mesh algorithm (Gmsh option name).
    pub algorithm: Option<String>,
    /// Advanced options passed through to Gmsh as "Category.Option" = value.
    pub advanced: Vec<(String, f64)>,
}

impl Default for MeshSettings {
    fn default() -> Self {
        Self {
            characteristic_length: None,
            order: 1,
            optimize: true,
            algorithm: None,
            advanced: Vec::new(),
        }
    }
}

/// Wraps a Gmsh session: initialize on creation, finalize on drop.
///
/// Gmsh's kernel is a global singleton, so only one `GmshContext` should be
/// active at a time.
pub struct GmshContext {
    initialized: bool,
}

impl GmshContext {
    /// Initialize the Gmsh API.
    pub fn new() -> Result<Self, MeshError> {
        let mut ierr: c_int = 0;
        unsafe {
            ffi::gmshInitialize(0, std::ptr::null_mut(), 1, 0, &mut ierr);
        }
        check(ierr)?;
        Ok(Self { initialized: true })
    }

    /// Clear all models and start fresh.
    pub fn clear(&mut self) -> Result<(), MeshError> {
        let mut ierr: c_int = 0;
        unsafe { ffi::gmshClear(&mut ierr) };
        check(ierr)?;
        Ok(())
    }

    /// Set a numerical Gmsh option.
    pub fn set_option_number(&self, name: &str, value: f64) -> Result<(), MeshError> {
        let c_name = CString::new(name)?;
        let mut ierr: c_int = 0;
        unsafe {
            ffi::gmshOptionSetNumber(c_name.as_ptr(), value, &mut ierr);
        }
        check(ierr)?;
        Ok(())
    }

    /// Set a string Gmsh option.
    pub fn set_option_string(&self, name: &str, value: &str) -> Result<(), MeshError> {
        let c_name = CString::new(name)?;
        let c_value = CString::new(value)?;
        let mut ierr: c_int = 0;
        unsafe {
            ffi::gmshOptionSetString(c_name.as_ptr(), c_value.as_ptr(), &mut ierr);
        }
        check(ierr)?;
        Ok(())
    }
}

impl Drop for GmshContext {
    fn drop(&mut self) {
        if self.initialized {
            let mut ierr: c_int = 0;
            unsafe { ffi::gmshFinalize(&mut ierr) };
            // Ignore errors during drop
            let _ = check(ierr);
        }
    }
}

/// Error type for the builder.
#[derive(Debug)]
pub enum BuildError {
    Ffi(FfiError),
    Mesh(MeshError),
    Geometry(geometry::RealizeError),
    Io(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Ffi(e) => write!(f, "Gmsh FFI error: {}", e),
            BuildError::Mesh(e) => write!(f, "Mesh error: {}", e),
            BuildError::Geometry(e) => write!(f, "Geometry error: {}", e),
            BuildError::Io(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<FfiError> for BuildError {
    fn from(e: FfiError) -> Self {
        BuildError::Ffi(e)
    }
}

impl From<MeshError> for BuildError {
    fn from(e: MeshError) -> Self {
        BuildError::Mesh(e)
    }
}

impl From<geometry::RealizeError> for BuildError {
    fn from(e: geometry::RealizeError) -> Self {
        BuildError::Geometry(e)
    }
}

impl From<std::ffi::NulError> for BuildError {
    fn from(e: std::ffi::NulError) -> Self {
        BuildError::Io(format!("NulError: {}", e))
    }
}

/// Top-level build function: realize shape tree → fragment → physical groups
/// → mesh → read back into Rust `Mesh`.
///
/// Entity names are set during realization (in geometry.rs) and survive
/// through boolean operations, fragment, and mesh generation. After mesh
/// generation, entity tags may be renumbered but names are preserved, so we
/// query entities by name to build the final label → post-mesh-entity mapping.
pub fn build(shapes: &[Shape], settings: &MeshSettings) -> Result<Mesh, BuildError> {
    let mut ctx = GmshContext::new()?;
    ctx.clear()?;

    // Apply meshing settings
    if let Some(h) = settings.characteristic_length {
        ctx.set_option_number("Mesh.CharacteristicLengthMin", h)?;
        ctx.set_option_number("Mesh.CharacteristicLengthMax", h)?;
    }
    ctx.set_option_number("Mesh.ElementOrder", settings.order as f64)?;
    if settings.optimize {
        ctx.set_option_number("Mesh.Optimize", 1.0)?;
    }
    if let Some(algo) = &settings.algorithm {
        ctx.set_option_string("Mesh.Algorithm", algo)?;
    }
    for (name, value) in &settings.advanced {
        ctx.set_option_number(name, *value)?;
    }

    // Determine the model dimension from the shape tree
    let model_dim = shapes
        .iter()
        .map(|s| s.dimension())
        .max()
        .unwrap_or(0);
    eprintln!("  build(): model_dim = {}", model_dim);

    // Realize each shape into Gmsh OCC entities.
    // Entity names are set during realization (in geometry.rs) and survive
    // through boolean operations, fragment, and mesh generation.
    let mut realize_ctx = RealizeCtx::new(model_dim);
    for shape in shapes {
        eprintln!("  build(): realizing shape with dim={}", shape.dimension());
        shape.realize(&mut realize_ctx)?;
        eprintln!("    realized, dimtags={:?}", realize_ctx.all_dimtags);
    }

    // Synchronize OCC with the Gmsh model
    {
        let mut ierr: c_int = 0;
        unsafe { ffi::gmshModelOccSynchronize(&mut ierr) };
        check(ierr)?;
        eprintln!("    synchronized");
    }

    // Run occ.fragment to create shared topology between all entities.
    // Fragment may renumber entity tags, but entity names (set during
    // realization) survive renumbering.
    let object_flat: Vec<ffi::c_int> = realize_ctx
        .all_dimtags
        .iter()
        .flat_map(|t| vec![t.dim, t.tag])
        .collect();
    let object_n = object_flat.len();

    if object_n > 0 {
        let mut ierr: c_int = 0;
        let mut out_dimtags: *mut ffi::c_int = ptr::null_mut();
        let mut out_dimtags_n: usize = 0;

        unsafe {
            ffi::gmshModelOccFragment(
                object_flat.as_ptr(),
                object_n / 2,
                ptr::null(),
                0,
                &mut out_dimtags,
                &mut out_dimtags_n,
                std::ptr::null_mut(), // outDimTagsMap
                std::ptr::null_mut(), // outDimTagsMap_n
                std::ptr::null_mut(), // outDimTagsMap_nn
                0,
                0, // removeObject
                0, // removeTool
                &mut ierr,
            )
        };
        check(ierr)?;

        if !out_dimtags.is_null() {
            unsafe { ffi::gmshFree(out_dimtags as *mut c_void) };
        }

        // Synchronize fragment results back to the Gmsh model
        {
            let mut ierr: c_int = 0;
            unsafe { ffi::gmshModelOccSynchronize(&mut ierr) };
            check(ierr)?;
        }
    }

    // Generate the mesh
    {
        let mut ierr: c_int = 0;
        unsafe { ffi::gmshModelMeshGenerate(model_dim as i32, &mut ierr) };
        check(ierr)?;
    }

    // After mesh generation, entity tags may have been renumbered but names
    // (set during realization) are preserved. Query all entities, read their
    // names, and build the final label → post-mesh-entity mapping.
    let mut name_to_dimtags: HashMap<String, Vec<DimTag>> = HashMap::new();
    for dim in 0..=model_dim as i32 {
        let mut dimtags: *mut c_int = ptr::null_mut();
        let mut dimtags_n: usize = 0;
        let mut ierr: c_int = 0;
        unsafe {
            ffi::gmshModelGetEntities(
                &mut dimtags,
                &mut dimtags_n,
                dim,
                &mut ierr,
            )
        };
        check(ierr)?;

        let num_entities = dimtags_n / 2;
        for i in 0..num_entities {
            let e_dim = unsafe { *dimtags.add(i * 2) };
            let e_tag = unsafe { *dimtags.add(i * 2 + 1) };

            // Get entity name
            let mut name_ptr: *mut c_char = ptr::null_mut();
            unsafe {
                ffi::gmshModelGetEntityName(e_dim, e_tag, &mut name_ptr, &mut ierr);
            }
            check(ierr)?;

            if !name_ptr.is_null() {
                let len = (0..).find(|&k| unsafe { *name_ptr.add(k) == 0 }).unwrap_or(0);
                let bytes = unsafe { std::slice::from_raw_parts(name_ptr as *const u8, len) };
                let name = String::from_utf8_lossy(bytes).to_string();
                unsafe { ffi::gmshFree(name_ptr as *mut c_void) };

                name_to_dimtags.entry(name).or_default().push(DimTag { dim: e_dim, tag: e_tag });
            }
        }

        if !dimtags.is_null() {
            unsafe { ffi::gmshFree(dimtags as *mut c_void) };
        }
    }

    // Create physical groups using post-mesh entity tags (which are final).
    for (name, dimtags) in &name_to_dimtags {
        let mut by_dim: HashMap<i32, Vec<i32>> = HashMap::new();
        for dt in dimtags {
            by_dim.entry(dt.dim).or_default().push(dt.tag);
        }

        for (dim, tags) in &by_dim {
            if tags.is_empty() {
                continue;
            }
            let c_name = CString::new(name.as_str())?;
            let mut ierr: c_int = 0;
            unsafe {
                ffi::gmshModelAddPhysicalGroup(
                    *dim,
                    tags.as_ptr(),
                    tags.len(),
                    -1, // auto-assign tag
                    c_name.as_ptr(),
                    &mut ierr,
                )
            };
            check(ierr)?;
        }
    }

    // Read back nodes
    let mut ierr: c_int = 0;
    let nodes = mesh::read_nodes(&mut ierr)?;
    check(ierr)?;

    // Read back elements
    let mut ierr: c_int = 0;
    let elements = mesh::read_elements(&mut ierr)?;
    check(ierr)?;

    // Build physical groups from name_to_dimtags (entity dimtags preserved
    // through mesh generation via entity names). Following the Gmsh .msh
    // data model: physical groups store entity (dim, tag) pairs, and
    // elements are found by looking up elements classified on those entities.
    let mut physical_groups: Vec<mesh::PhysicalGroup> = Vec::new();
    for (name, dimtags) in &name_to_dimtags {
        let mut by_dim: HashMap<i32, Vec<i32>> = HashMap::new();
        for dt in dimtags {
            by_dim.entry(dt.dim).or_default().push(dt.tag);
        }

        for (dim, tags) in &by_dim {
            if tags.is_empty() {
                continue;
            }
            physical_groups.push(mesh::PhysicalGroup {
                name: name.clone(),
                dim: *dim,
                tag: tags[0], // first tag as group tag
                entities: tags.iter().map(|&t| (*dim, t)).collect(),
            });
        }
    }

    // Build metadata
    let bounding_box = mesh::compute_bounding_box(&nodes);
    let metadata = mesh::MeshMetadata {
        gmsh_version: "4.15.2".to_string(),
        node_count: nodes.len(),
        element_count: elements.len(),
        physical_group_count: physical_groups.len(),
        bounding_box,
    };

    Ok(Mesh {
        nodes,
        elements,
        physical_groups,
        metadata,
    })
}

/// Convenience: build with default settings.
pub fn build_default(shape: &Shape) -> Result<Mesh, BuildError> {
    build(&[shape.clone()], &MeshSettings::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_settings_default() {
        let s = MeshSettings::default();
        assert_eq!(s.order, 1);
        assert!(s.optimize);
        assert!(s.characteristic_length.is_none());
    }
}
