//! Builder: `MeshSettings`, the `GmshContext` lifecycle, and the top-level
//! `build()` function that realizes a shape tree, fragments it, creates
//! physical groups, generates a mesh and reads it back into a Rust [`Mesh`].

use std::collections::BTreeMap;
use std::ffi::CString;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ffi::{self, check, DimTag, FfiError};
use crate::geometry::{self, Realizable, RealizeCtx, RealizeError, Shape};
use crate::mesh::{self, Mesh, MeshError, MeshMetadata, PhysicalGroup};

/// Meshing settings exposed to users.
///
/// These map onto Gmsh options; anything else can be set through
/// [`MeshSettings::advanced`].
#[derive(Debug, Clone)]
pub struct MeshSettings {
    /// Target characteristic element size. Sets both
    /// `Mesh.CharacteristicLengthMin` and `Mesh.CharacteristicLengthMax`;
    /// Gmsh may still deviate from it.
    pub characteristic_length: Option<f64>,
    /// Mesh element order: 1 = linear, 2 = quadratic.
    pub order: u8,
    /// Run the Gmsh mesh optimizer after generation (on by default).
    pub optimize: bool,
    /// Gmsh 2D meshing algorithm code (`Mesh.Algorithm`), e.g. 5 = Delaunay,
    /// 6 = Frontal-Delaunay. See the Gmsh reference manual. The 3D algorithm
    /// can be set through `advanced` (`("Mesh.Algorithm3D", 10.0)`).
    pub algorithm: Option<i32>,
    /// Extra Gmsh options, set verbatim as number options before meshing,
    /// e.g. `("Mesh.Algorithm3D", 10.0)`.
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

/// Wraps a Gmsh session: initialize on creation, finalize on drop (only if
/// this context performed the initialization).
///
/// Gmsh's kernel is a global singleton, so only one `GmshContext` should be
/// active at a time.
pub struct GmshContext {
    /// True when this context initialized Gmsh and must finalize it.
    owned: bool,
}

impl GmshContext {
    /// Initialize the Gmsh API, unless it is already initialized.
    pub fn new() -> Result<Self, MeshError> {
        let mut ierr: c_int = 0;
        let initialized = unsafe { ffi::gmshIsInitialized(&mut ierr) };
        check(ierr)?;

        let owned = initialized == 0;
        if owned {
            unsafe { ffi::gmshInitialize(0, std::ptr::null_mut(), 1, 0, &mut ierr) };
            check(ierr)?;
        }
        Ok(Self { owned })
    }

    /// Create a fresh, empty model and make it the current one.
    ///
    /// This does not touch existing models (a new one is added).
    pub fn create_model(&mut self) -> Result<(), MeshError> {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = CString::new(format!("geommesh_{n}"))?;
        let mut ierr: c_int = 0;
        unsafe { ffi::gmshModelAdd(name.as_ptr(), &mut ierr) };
        check(ierr)?;
        Ok(())
    }

    /// Set a numerical Gmsh option.
    pub fn set_option_number(&self, name: &str, value: f64) -> Result<(), MeshError> {
        let c_name = CString::new(name)?;
        let mut ierr: c_int = 0;
        unsafe { ffi::gmshOptionSetNumber(c_name.as_ptr(), value, &mut ierr) };
        check(ierr)?;
        Ok(())
    }

    /// Set a string Gmsh option.
    pub fn set_option_string(&self, name: &str, value: &str) -> Result<(), MeshError> {
        let c_name = CString::new(name)?;
        let c_value = CString::new(value)?;
        let mut ierr: c_int = 0;
        unsafe { ffi::gmshOptionSetString(c_name.as_ptr(), c_value.as_ptr(), &mut ierr) };
        check(ierr)?;
        Ok(())
    }

    /// Human-readable description of the last error Gmsh logged.
    pub fn last_error(&self) -> String {
        ffi::last_error_message()
    }
}

impl Drop for GmshContext {
    fn drop(&mut self) {
        if self.owned {
            let mut ierr: c_int = 0;
            unsafe { ffi::gmshFinalize(&mut ierr) };
            // Errors during teardown are intentionally ignored.
        }
    }
}

/// Error type for the builder.
#[derive(Debug)]
pub enum BuildError {
    /// A Gmsh C API call failed.
    Ffi(FfiError),
    /// Reading back the generated mesh failed.
    Mesh(MeshError),
    /// A shape could not be realized (see [`RealizeError`]).
    Geometry(RealizeError),
    /// The arguments to `build` were invalid (empty slice, no entities, …).
    InvalidArgument(String),
    /// A string conversion or file operation failed.
    Io(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Ffi(e) => write!(f, "{e}"),
            BuildError::Mesh(e) => write!(f, "{e}"),
            BuildError::Geometry(e) => write!(f, "{e}"),
            BuildError::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
            BuildError::Io(msg) => write!(f, "I/O error: {msg}"),
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

impl From<RealizeError> for BuildError {
    fn from(e: RealizeError) -> Self {
        BuildError::Geometry(e)
    }
}

impl From<std::ffi::NulError> for BuildError {
    fn from(e: std::ffi::NulError) -> Self {
        BuildError::Io(format!("string contains a null byte: {e}"))
    }
}

/// Top-level build function: realize the shape tree, fragment it, create
/// physical groups, generate a mesh and read it back.
///
/// * Every label becomes one or more Gmsh physical groups (one per
///   dimension), containing the entities that the labeled shape produced.
/// * All live entities are fragmented once before meshing, so the resulting
///   mesh has conformal interfaces.
/// * Labels on entities consumed by a boolean operation disappear (their
///   entities no longer exist); labels on boolean results survive.
///
/// Gmsh is a process-wide singleton: calls to `build` are serialized
/// internally, so this function can safely be called from several threads
/// (concurrent builds queue up).
///
/// # Errors
///
/// Returns [`BuildError`] if `shapes` is empty or produces no entities, if a
/// shape cannot be realized (bad dimensions, invalid polygon, …), if a Gmsh
/// call fails, or if the mesh cannot be generated.
///
/// # Examples
///
/// ```no_run
/// use geommesh::{build, MeshSettings, Shape};
///
/// let cube = Shape::Cube {
///     min: [0.0, 0.0, 0.0],
///     max: [1.0, 1.0, 1.0],
///     label: Some("cube".into()),
/// };
/// let mesh = build(&[cube], &MeshSettings::default())?;
/// assert!(mesh.physical_group("cube").is_some());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn build(shapes: &[Shape], settings: &MeshSettings) -> Result<Mesh, BuildError> {
    if shapes.is_empty() {
        return Err(BuildError::InvalidArgument(
            "`shapes` must contain at least one shape".into(),
        ));
    }

    // Gmsh is a process-wide singleton: serialize all builds.
    let _gmsh_guard = ffi::lock_gmsh();

    let mut gmsh = GmshContext::new()?;
    gmsh.create_model()?;

    // Apply meshing settings.
    if let Some(h) = settings.characteristic_length {
        gmsh.set_option_number("Mesh.CharacteristicLengthMin", h)?;
        gmsh.set_option_number("Mesh.CharacteristicLengthMax", h)?;
    }
    gmsh.set_option_number("Mesh.ElementOrder", settings.order as f64)?;
    if settings.optimize {
        gmsh.set_option_number("Mesh.Optimize", 1.0)?;
    }
    if let Some(algorithm) = settings.algorithm {
        gmsh.set_option_number("Mesh.Algorithm", algorithm as f64)?;
    }
    for (name, value) in &settings.advanced {
        gmsh.set_option_number(name, *value)?;
    }

    // Realize every shape into OCC entities.
    let model_dim = shapes.iter().map(|s| s.dimension()).max().unwrap_or(0);
    let mut ctx = RealizeCtx::new(model_dim);
    for shape in shapes {
        shape.realize(&mut ctx)?;
    }
    geometry::occ_sync()?;

    let live: Vec<DimTag> = ctx.live_dimtags().to_vec();
    if live.is_empty() {
        return Err(BuildError::InvalidArgument(
            "the given shapes produced no entities".into(),
        ));
    }

    // Fragment all live entities once so that everything is conformal.
    // `fragment_map[i]` lists the entities that input entity `i` turned into.
    let fragment_map: Vec<Vec<DimTag>> = if live.len() == 1 {
        vec![live.clone()]
    } else {
        let (_results, map) = geometry::occ_fragment(&live, &[], true, true)?;
        map
    };
    // Gmsh fragments live in the OCC kernel until synchronized; the model
    // must know the resulting entities before physical groups can name them.
    geometry::occ_sync()?;

    // Map shape labels to their post-fragment entities.
    let mut groups: Vec<PhysicalGroup> = Vec::new();
    for (label, entities) in ctx.labels() {
        let mut post: Vec<DimTag> = Vec::new();
        for entity in entities {
            if let Some(index) = live.iter().position(|e| e == entity) {
                if let Some(mapped) = fragment_map.get(index) {
                    post.extend_from_slice(mapped);
                }
            }
        }
        post.sort_unstable();
        post.dedup();
        if post.is_empty() {
            continue;
        }

        let mut by_dim: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        for (dim, tag) in post {
            by_dim.entry(dim).or_default().push(tag);
        }
        for (dim, tags) in by_dim {
            let tag = geometry::add_physical_group(dim, &tags, label)?;
            groups.push(PhysicalGroup {
                name: label.clone(),
                dim,
                tag,
                entities: tags.into_iter().map(|t| (dim, t)).collect(),
            });
        }
    }
    groups.sort_by(|a, b| (a.dim, a.name.as_str(), a.tag).cmp(&(b.dim, b.name.as_str(), b.tag)));

    // Generate the mesh. Gmsh's `generate(0)` is a no-op, so always ask for
    // at least dimension 1 (this matters for point-only models).
    let mut ierr: c_int = 0;
    unsafe { ffi::gmshModelMeshGenerate(model_dim.max(1), &mut ierr) };
    check(ierr)?;

    // Read the mesh back.
    let entities = mesh::get_entities()?;
    let nodes = mesh::read_nodes()?;
    let elements = mesh::read_elements(&entities)?;

    let bounding_box = mesh::compute_bounding_box(&nodes);
    let metadata = MeshMetadata {
        gmsh_version: "4.15.2".to_string(),
        node_count: nodes.len(),
        element_count: elements.len(),
        physical_group_count: groups.len(),
        bounding_box,
    };

    Ok(Mesh {
        nodes,
        elements,
        physical_groups: groups,
        metadata,
    })
}

/// Convenience wrapper: build a single shape with [`MeshSettings::default`].
pub fn build_default(shape: &Shape) -> Result<Mesh, BuildError> {
    build(std::slice::from_ref(shape), &MeshSettings::default())
}
