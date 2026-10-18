//! Raw FFI bindings to the Gmsh C API (`gmshc.h`, Gmsh 4.15.2).
//!
//! Every signature below was checked against
//! `.sandbox/gmsh-4.15.2-Windows64-sdk/include/gmshc.h`. Two conventions of
//! the C API are easy to get wrong and are called out here:
//!
//! * "Vectors of pairs" (`dimTags`) are passed as flat `[dim, tag, dim, ...]`
//!   int arrays, and their `*_n` companion is the number of **ints** (i.e.
//!   twice the number of entities).
//! * Output arrays are allocated by Gmsh with `gmshMalloc`; they must be
//!   released with [`gmshFree`]. Every output parameter must point at valid
//!   storage (passing null is not a "don't care").
//!
//! Functions that the header declares as returning `void` are declared here
//! without a return value; the others return either a new entity tag or an
//! exit code.

pub use std::os::raw::{c_char, c_double, c_int, c_void};

/// A `(dimension, tag)` pair identifying a Gmsh model entity.
///
/// Gmsh uses `int` for both members, so this matches the C ABI exactly.
pub type DimTag = (i32, i32);

/// Error code reported through Gmsh's `int *ierr` out-parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfiError(pub c_int);

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Gmsh API error (ierr = {})", self.0)
    }
}

impl std::error::Error for FfiError {}

/// Convert a Gmsh `ierr` value into a `Result`.
#[inline]
pub fn check(ierr: c_int) -> Result<(), FfiError> {
    if ierr != 0 {
        Err(FfiError(ierr))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper functions (safe wrappers around the raw calls below)
// ---------------------------------------------------------------------------

/// Flatten `[(dim, tag), ...]` into the C API layout `[dim, tag, ...]`.
pub fn dimtags_to_flat(dimtags: &[DimTag]) -> Vec<c_int> {
    let mut flat = Vec::with_capacity(dimtags.len() * 2);
    for &(dim, tag) in dimtags {
        flat.push(dim);
        flat.push(tag);
    }
    flat
}

/// Read `n` ints from a flat Gmsh output array as `(dim, tag)` pairs.
///
/// `n` is the number of ints, not the number of entities. The caller keeps
/// ownership of `ptr` and remains responsible for freeing it.
///
/// # Safety
/// `ptr` must point to at least `n` readable `c_int` values.
pub unsafe fn read_dimtags(ptr: *const c_int, n: usize) -> Vec<DimTag> {
    let mut out = Vec::with_capacity(n / 2);
    for i in 0..n / 2 {
        out.push((*ptr.add(i * 2), *ptr.add(i * 2 + 1)));
    }
    out
}

/// Take ownership of a NUL-terminated string allocated by Gmsh, copy it into
/// a `String` and free it.
///
/// # Safety
/// `ptr` must either be null or point to a NUL-terminated string allocated by
/// Gmsh (i.e. one that may be released with `gmshFree`).
pub unsafe fn read_owned_string(ptr: *mut c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let s = std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned();
    free(ptr as *mut c_void);
    s
}

/// Free a pointer allocated by Gmsh. No-op for null.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by Gmsh.
pub unsafe fn free(ptr: *mut c_void) {
    if !ptr.is_null() {
        gmshFree(ptr);
    }
}

/// Serialize access to the global Gmsh kernel.
///
/// Gmsh keeps process-wide state, so the whole crate funnels through this
/// lock: [`crate::builder::build`] takes it for the duration of a build. Take
/// it yourself when calling the raw functions in this module from multiple
/// threads. The lock is not reentrant.
pub fn lock_gmsh() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Return the message Gmsh logged for the last error, if any.
///
/// Must only be called while Gmsh is initialized.
pub fn last_error_message() -> String {
    let mut ierr: c_int = 0;
    let mut msg: *mut c_char = std::ptr::null_mut();
    unsafe { gmshLoggerGetLastError(&mut msg, &mut ierr) };
    if ierr != 0 {
        return String::new();
    }
    unsafe { read_owned_string(msg) }
}

// ---------------------------------------------------------------------------
// Raw bindings
// ---------------------------------------------------------------------------

extern "C" {
    // --- Top-level ---

    pub fn gmshInitialize(
        argc: c_int,
        argv: *mut *mut c_char,
        read_config_files: c_int,
        run: c_int,
        ierr: *mut c_int,
    );

    pub fn gmshIsInitialized(ierr: *mut c_int) -> c_int;
    pub fn gmshFinalize(ierr: *mut c_int);
    pub fn gmshClear(ierr: *mut c_int);
    pub fn gmshWrite(file_name: *const c_char, ierr: *mut c_int);

    /// Open a file (`.msh`, `.geo`, …). A file with model data creates a new
    /// model and makes it current.
    pub fn gmshOpen(file_name: *const c_char, ierr: *mut c_int);

    /// Human-readable description of the last error logged by Gmsh.
    pub fn gmshLoggerGetLastError(error: *mut *mut c_char, ierr: *mut c_int);

    // --- Options ---

    pub fn gmshOptionSetNumber(name: *const c_char, value: c_double, ierr: *mut c_int);
    pub fn gmshOptionSetString(name: *const c_char, value: *const c_char, ierr: *mut c_int);

    // --- Model ---

    pub fn gmshModelAdd(name: *const c_char, ierr: *mut c_int);
    pub fn gmshModelGetEntities(
        dim_tags: *mut *mut c_int,
        dim_tags_n: *mut usize,
        dim: c_int,
        ierr: *mut c_int,
    );
    pub fn gmshModelSetEntityName(dim: c_int, tag: c_int, name: *const c_char, ierr: *mut c_int);
    pub fn gmshModelGetEntityName(dim: c_int, tag: c_int, name: *mut *mut c_char, ierr: *mut c_int);
    pub fn gmshModelGetEntityType(
        dim: c_int,
        tag: c_int,
        entity_type: *mut *mut c_char,
        ierr: *mut c_int,
    );

    // --- Physical groups ---

    pub fn gmshModelAddPhysicalGroup(
        dim: c_int,
        tags: *const c_int,
        tags_n: usize,
        tag: c_int,
        name: *const c_char,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelSetPhysicalName(dim: c_int, tag: c_int, name: *const c_char, ierr: *mut c_int);
    pub fn gmshModelGetPhysicalName(
        dim: c_int,
        tag: c_int,
        name: *mut *mut c_char,
        ierr: *mut c_int,
    );
    pub fn gmshModelGetPhysicalGroups(
        dim_tags: *mut *mut c_int,
        dim_tags_n: *mut usize,
        dim: c_int,
        ierr: *mut c_int,
    );
    /// Returns the *entity tags* (within `dim`) contained in the physical
    /// group, not `(dim, tag)` pairs.
    pub fn gmshModelGetEntitiesForPhysicalGroup(
        dim: c_int,
        tag: c_int,
        tags: *mut *mut c_int,
        tags_n: *mut usize,
        ierr: *mut c_int,
    );

    // --- OCC geometry creation ---

    pub fn gmshModelOccAddPoint(
        x: c_double,
        y: c_double,
        z: c_double,
        mesh_size: c_double,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddLine(
        start_tag: c_int,
        end_tag: c_int,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddCircleArc(
        start_tag: c_int,
        middle_tag: c_int,
        end_tag: c_int,
        tag: c_int,
        center: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddCircle(
        x: c_double,
        y: c_double,
        z: c_double,
        r: c_double,
        tag: c_int,
        angle1: c_double,
        angle2: c_double,
        z_axis: *const c_double,
        z_axis_n: usize,
        x_axis: *const c_double,
        x_axis_n: usize,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddEllipse(
        x: c_double,
        y: c_double,
        z: c_double,
        r1: c_double,
        r2: c_double,
        tag: c_int,
        angle1: c_double,
        angle2: c_double,
        z_axis: *const c_double,
        z_axis_n: usize,
        x_axis: *const c_double,
        x_axis_n: usize,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddRectangle(
        x: c_double,
        y: c_double,
        z: c_double,
        dx: c_double,
        dy: c_double,
        tag: c_int,
        rounded_radius: c_double,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddDisk(
        xc: c_double,
        yc: c_double,
        zc: c_double,
        rx: c_double,
        ry: c_double,
        tag: c_int,
        z_axis: *const c_double,
        z_axis_n: usize,
        x_axis: *const c_double,
        x_axis_n: usize,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddWire(
        curve_tags: *const c_int,
        curve_tags_n: usize,
        tag: c_int,
        check_closed: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddCurveLoop(
        curve_tags: *const c_int,
        curve_tags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddPlaneSurface(
        wire_tags: *const c_int,
        wire_tags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddSurfaceLoop(
        surface_tags: *const c_int,
        surface_tags_n: usize,
        tag: c_int,
        sewing: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddVolume(
        shell_tags: *const c_int,
        shell_tags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddSphere(
        xc: c_double,
        yc: c_double,
        zc: c_double,
        radius: c_double,
        tag: c_int,
        angle1: c_double,
        angle2: c_double,
        angle3: c_double,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddBox(
        x: c_double,
        y: c_double,
        z: c_double,
        dx: c_double,
        dy: c_double,
        dz: c_double,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddCylinder(
        x: c_double,
        y: c_double,
        z: c_double,
        dx: c_double,
        dy: c_double,
        dz: c_double,
        r: c_double,
        tag: c_int,
        angle: c_double,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddCone(
        x: c_double,
        y: c_double,
        z: c_double,
        dx: c_double,
        dy: c_double,
        dz: c_double,
        r1: c_double,
        r2: c_double,
        tag: c_int,
        angle: c_double,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddWedge(
        x: c_double,
        y: c_double,
        z: c_double,
        dx: c_double,
        dy: c_double,
        dz: c_double,
        tag: c_int,
        ltx: c_double,
        z_axis: *const c_double,
        z_axis_n: usize,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelOccAddTorus(
        x: c_double,
        y: c_double,
        z: c_double,
        r1: c_double,
        r2: c_double,
        tag: c_int,
        angle: c_double,
        z_axis: *const c_double,
        z_axis_n: usize,
        ierr: *mut c_int,
    ) -> c_int;

    // --- OCC boolean operations ---
    //
    // All of these return `void`; results come back through the out-params.
    // `out_dim_tags_map` has one entry per *input* entity (objects first, then
    // tools); `out_dim_tags_map_n[i]` is the number of ints in
    // `out_dim_tags_map[i]`.

    pub fn gmshModelOccFuse(
        object_dim_tags: *const c_int,
        object_dim_tags_n: usize,
        tool_dim_tags: *const c_int,
        tool_dim_tags_n: usize,
        out_dim_tags: *mut *mut c_int,
        out_dim_tags_n: *mut usize,
        out_dim_tags_map: *mut *mut *mut c_int,
        out_dim_tags_map_n: *mut *mut usize,
        out_dim_tags_map_nn: *mut usize,
        tag: c_int,
        remove_object: c_int,
        remove_tool: c_int,
        ierr: *mut c_int,
    );

    pub fn gmshModelOccCut(
        object_dim_tags: *const c_int,
        object_dim_tags_n: usize,
        tool_dim_tags: *const c_int,
        tool_dim_tags_n: usize,
        out_dim_tags: *mut *mut c_int,
        out_dim_tags_n: *mut usize,
        out_dim_tags_map: *mut *mut *mut c_int,
        out_dim_tags_map_n: *mut *mut usize,
        out_dim_tags_map_nn: *mut usize,
        tag: c_int,
        remove_object: c_int,
        remove_tool: c_int,
        ierr: *mut c_int,
    );

    pub fn gmshModelOccIntersect(
        object_dim_tags: *const c_int,
        object_dim_tags_n: usize,
        tool_dim_tags: *const c_int,
        tool_dim_tags_n: usize,
        out_dim_tags: *mut *mut c_int,
        out_dim_tags_n: *mut usize,
        out_dim_tags_map: *mut *mut *mut c_int,
        out_dim_tags_map_n: *mut *mut usize,
        out_dim_tags_map_nn: *mut usize,
        tag: c_int,
        remove_object: c_int,
        remove_tool: c_int,
        ierr: *mut c_int,
    );

    pub fn gmshModelOccFragment(
        object_dim_tags: *const c_int,
        object_dim_tags_n: usize,
        tool_dim_tags: *const c_int,
        tool_dim_tags_n: usize,
        out_dim_tags: *mut *mut c_int,
        out_dim_tags_n: *mut usize,
        out_dim_tags_map: *mut *mut *mut c_int,
        out_dim_tags_map_n: *mut *mut usize,
        out_dim_tags_map_nn: *mut usize,
        tag: c_int,
        remove_object: c_int,
        remove_tool: c_int,
        ierr: *mut c_int,
    );

    // --- OCC synchronization ---

    /// Bind pending OCC shapes into the Gmsh model (and give them tags).
    pub fn gmshModelOccSynchronize(ierr: *mut c_int);

    // --- Mesh ---

    pub fn gmshModelMeshGenerate(dim: c_int, ierr: *mut c_int);
    pub fn gmshModelMeshSetOrder(order: c_int, ierr: *mut c_int);
    pub fn gmshModelMeshSetSize(
        dim_tags: *const c_int,
        dim_tags_n: usize,
        size: c_double,
        ierr: *mut c_int,
    );

    pub fn gmshModelMeshGetNodes(
        node_tags: *mut *mut usize,
        node_tags_n: *mut usize,
        coord: *mut *mut c_double,
        coord_n: *mut usize,
        parametric_coord: *mut *mut c_double,
        parametric_coord_n: *mut usize,
        dim: c_int,
        tag: c_int,
        include_boundary: c_int,
        return_parametric_coord: c_int,
        ierr: *mut c_int,
    );

    pub fn gmshModelMeshGetElements(
        element_types: *mut *mut c_int,
        element_types_n: *mut usize,
        element_tags: *mut *mut *mut usize,
        element_tags_n: *mut *mut usize,
        element_tags_nn: *mut usize,
        node_tags: *mut *mut *mut usize,
        node_tags_n: *mut *mut usize,
        node_tags_nn: *mut usize,
        dim: c_int,
        tag: c_int,
        ierr: *mut c_int,
    );

    pub fn gmshModelMeshGetElement(
        element_tag: usize,
        element_type: *mut c_int,
        node_tags: *mut *mut usize,
        node_tags_n: *mut usize,
        dim: *mut c_int,
        tag: *mut c_int,
        ierr: *mut c_int,
    );

    pub fn gmshModelMeshGetElementType(
        family_name: *const c_char,
        order: c_int,
        serendip: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    pub fn gmshModelMeshGetElementProperties(
        element_type: c_int,
        element_name: *mut *mut c_char,
        dim: *mut c_int,
        order: *mut c_int,
        num_nodes: *mut c_int,
        local_node_coord: *mut *mut c_double,
        local_node_coord_n: *mut usize,
        num_primary_nodes: *mut c_int,
        ierr: *mut c_int,
    );

    // --- Memory management ---

    /// Release memory allocated by Gmsh for output arrays.
    pub fn gmshFree(p: *mut c_void);
}
