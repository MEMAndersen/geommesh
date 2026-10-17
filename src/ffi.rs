//! Raw FFI bindings to the Gmsh C API (gmshc.h, v4.15.2).
//!
//! These are unsafe, low-level bindings. A safe Rust wrapper should be built
//! on top of this module.

pub use std::os::raw::{c_char, c_double, c_int, c_void};

/// Opaque type representing the Gmsh API context (the kernel is a global
/// singleton, so there is no handle to pass around).
pub type GmshContext = ();

extern "C" {
    // --- Memory management ---
    pub fn gmshFree(p: *mut c_void);
    pub fn gmshMalloc(n: usize) -> *mut c_void;

    // --- Initialization / finalization ---
    pub fn gmshInitialize(
        argc: c_int,
        argv: *mut *mut c_char,
        readConfigFiles: c_int,
        run: c_int,
        ierr: *mut c_int,
    );
    pub fn gmshIsInitialized(ierr: *mut c_int) -> c_int;
    pub fn gmshFinalize(ierr: *mut c_int);

    // --- Model lifecycle ---
    pub fn gmshClear(ierr: *mut c_int);
    pub fn gmshModelGetDimension(ierr: *mut c_int) -> c_int;

    // --- Options ---
    pub fn gmshOptionSetNumber(
        name: *const c_char,
        value: c_double,
        ierr: *mut c_int,
    );
    pub fn gmshOptionSetString(
        name: *const c_char,
        value: *const c_char,
        ierr: *mut c_int,
    );

    // --- Entity queries ---
    pub fn gmshModelGetEntities(
        dimTags: *mut *mut c_int,
        dimTags_n: *mut usize,
        dim: c_int,
        ierr: *mut c_int,
    );
    pub fn gmshModelGetPhysicalGroups(
        dimTags: *mut *mut c_int,
        dimTags_n: *mut usize,
        dim: c_int,
        ierr: *mut c_int,
    );
    pub fn gmshModelGetEntitiesForPhysicalGroup(
        dim: c_int,
        tag: c_int,
        dimTags: *mut *mut c_int,
        dimTags_n: *mut usize,
        ierr: *mut c_int,
    );
    pub fn gmshModelGetPhysicalGroupsEntities(
        dimTags: *mut *mut c_int,
        dimTags_n: *mut usize,
        entities: *mut *mut c_int,
        entities_n: *mut usize,
        entities_nn: *mut usize,
        dim: c_int,
        ierr: *mut c_int,
    );
    pub fn gmshModelGetPhysicalName(
        dim: c_int,
        tag: c_int,
        name: *mut *mut c_char,
        ierr: *mut c_int,
    );

    // --- Entity names ---
    /// Set the name of the entity of dimension `dim' and tag `tag'.
    pub fn gmshModelSetEntityName(
        dim: c_int,
        tag: c_int,
        name: *const c_char,
        ierr: *mut c_int,
    );
    /// Get the name of the entity of dimension `dim' and tag `tag'.
    pub fn gmshModelGetEntityName(
        dim: c_int,
        tag: c_int,
        name: *mut *mut c_char,
        ierr: *mut c_int,
    );

    // --- Physical groups ---
    /// Add a physical group of dimension `dim`, grouping entities with tags
    /// `tags`. Returns the physical group tag. If `name` is non-empty, sets
    /// the group name.
    pub fn gmshModelAddPhysicalGroup(
        dim: c_int,
        tags: *const c_int,
        tags_n: usize,
        tag: c_int,
        name: *const c_char,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelSetPhysicalName(
        dim: c_int,
        tag: c_int,
        name: *const c_char,
        ierr: *mut c_int,
    );

    // --- OCC geometry creation ---
    pub fn gmshModelOccAddPoint(
        x: c_double,
        y: c_double,
        z: c_double,
        meshSize: c_double,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelOccAddLine(
        startTag: c_int,
        endTag: c_int,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelOccAddCircle(
        xc: c_double,
        yc: c_double,
        zc: c_double,
        radius: c_double,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelOccAddCircleArc(
        startTag: c_int,
        centerTag: c_int,
        endTag: c_int,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelOccAddRectangle(
        x: c_double,
        y: c_double,
        z: c_double,
        dx: c_double,
        dy: c_double,
        tag: c_int,
        radius: c_double,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelOccAddDisk(
        xc: c_double,
        yc: c_double,
        zc: c_double,
        radius: c_double,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelOccAddPlaneSurface(
        wireTags: *const c_int,
        wireTags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelOccAddSurfaceLoop(
        surfaceTags: *const c_int,
        surfaceTags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelOccAddCurveLoop(
        curveTags: *const c_int,
        curveTags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;
    pub fn gmshModelOccAddVolume(
        shellTags: *const c_int,
        shellTags_n: usize,
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
        ierr: *mut c_int,
    ) -> c_int;

    // --- OCC boolean operations ---
    pub fn gmshModelOccFuse(
        objectDimTags: *const c_int,
        objectDimTags_n: usize,
        toolDimTags: *const c_int,
        toolDimTags_n: usize,
        outDimTags: *mut *mut c_int,
        outDimTags_n: *mut usize,
        outDimTagsMap: *mut *mut *mut c_int,
        outDimTagsMap_n: *mut *mut usize,
        outDimTagsMap_nn: *mut usize,
        tag: c_int,
        removeObject: c_int,
        removeTool: c_int,
        ierr: *mut c_int,
    );
    pub fn gmshModelOccCut(
        objectDimTags: *const c_int,
        objectDimTags_n: usize,
        toolDimTags: *const c_int,
        toolDimTags_n: usize,
        outDimTags: *mut *mut c_int,
        outDimTags_n: *mut usize,
        outDimTagsMap: *mut *mut *mut c_int,
        outDimTagsMap_n: *mut *mut usize,
        outDimTagsMap_nn: *mut usize,
        tag: c_int,
        removeObject: c_int,
        removeTool: c_int,
        ierr: *mut c_int,
    );
    pub fn gmshModelOccIntersect(
        objectDimTags: *const c_int,
        objectDimTags_n: usize,
        toolDimTags: *const c_int,
        toolDimTags_n: usize,
        outDimTags: *mut *mut c_int,
        outDimTags_n: *mut usize,
        outDimTagsMap: *mut *mut *mut c_int,
        outDimTagsMap_n: *mut *mut usize,
        outDimTagsMap_nn: *mut usize,
        tag: c_int,
        removeObject: c_int,
        removeTool: c_int,
        ierr: *mut c_int,
    );

    // --- OCC fragment ---
    pub fn gmshModelOccFragment(
        objectDimTags: *const c_int,
        objectDimTags_n: usize,
        toolDimTags: *const c_int,
        toolDimTags_n: usize,
        outDimTags: *mut *mut c_int,
        outDimTags_n: *mut usize,
        outDimTagsMap: *mut *mut *mut c_int,
        outDimTagsMap_n: *mut *mut usize,
        outDimTagsMap_nn: *mut usize,
        tag: c_int,
        removeObject: c_int,
        removeTool: c_int,
        ierr: *mut c_int,
    );

    // --- OCC synchronization ---
    pub fn gmshModelOccSynchronize(ierr: *mut c_int);

    // --- Mesh ---
    pub fn gmshModelMeshGenerate(dim: c_int, ierr: *mut c_int);
    pub fn gmshModelMeshSetOrder(order: c_int, ierr: *mut c_int);

    /// Get all nodes of the mesh (optionally restricted to an entity).
    pub fn gmshModelMeshGetNodes(
        nodeTags: *mut *mut usize,
        nodeTags_n: *mut usize,
        coord: *mut *mut c_double,
        coord_n: *mut usize,
        parametricCoord: *mut *mut c_double,
        parametricCoord_n: *mut usize,
        dim: c_int,
        tag: c_int,
        includeBoundary: c_int,
        returnParametricCoord: c_int,
        ierr: *mut c_int,
    );

    /// Get elements of the mesh (optionally restricted to an entity).
    pub fn gmshModelMeshGetElements(
        elementTypes: *mut *mut c_int,
        elementTypes_n: *mut usize,
        elementTags: *mut *mut *mut usize,
        elementTags_n: *mut *mut usize,
        elementTags_nn: *mut usize,
        nodeTags: *mut *mut *mut usize,
        nodeTags_n: *mut *mut usize,
        nodeTags_nn: *mut usize,
        dim: c_int,
        tag: c_int,
        ierr: *mut c_int,
    );
    pub fn gmshModelMeshGetElement(
        elementTag: usize,
        elementType: *mut c_int,
        nodeTags: *mut *mut usize,
        nodeTags_n: *mut usize,
        dim: *mut c_int,
        tag: *mut c_int,
        ierr: *mut c_int,
    );
}

/// Check the Gmsh error code and return a Result.
#[inline]
pub fn check(ierr: c_int) -> Result<(), FfiError> {
    if ierr != 0 {
        Err(FfiError(ierr))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfiError(pub c_int);

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Gmsh FFI error code {}", self.0)
    }
}

impl std::error::Error for FfiError {}

/// Convert a Rust &str to a CString for FFI calls.
pub struct CStrHolder {
    _inner: std::ffi::CString,
}

impl CStrHolder {
    pub fn new(s: &str) -> Self {
        Self {
            _inner: std::ffi::CString::new(s).expect("string contained null byte"),
        }
    }
    pub fn as_ptr(&self) -> *const c_char {
        self._inner.as_ptr()
    }
}

/// A (dim, tag) pair representing a Gmsh model entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimTag {
    pub dim: c_int,
    pub tag: c_int,
}

impl DimTag {
    pub fn as_ffi_pair(self) -> (c_int, c_int) {
        (self.dim, self.tag)
    }
}

/// Helper to build a flat C array of (dim, tag) pairs from a slice.
pub fn dimtags_to_ffi(tags: &[DimTag]) -> (Vec<c_int>, usize) {
    let flat: Vec<c_int> = tags.iter().flat_map(|t| [t.dim, t.tag]).collect();
    let n = flat.len() / 2;
    (flat, n)
}

/// Free a C-allocated array of ints.
pub unsafe fn free_int_array(p: *mut c_int) {
    if !p.is_null() {
        gmshFree(p as *mut c_void);
    }
}

/// Free a C-allocated array of usize.
pub unsafe fn free_size_array(p: *mut usize) {
    if !p.is_null() {
        gmshFree(p as *mut c_void);
    }
}

/// Free a C-allocated array of doubles.
pub unsafe fn free_double_array(p: *mut c_double) {
    if !p.is_null() {
        gmshFree(p as *mut c_void);
    }
}

/// Free a C-allocated array of char* (strings).
pub unsafe fn free_char_ptr_array(p: *mut *mut c_char, n: usize) {
    if p.is_null() {
        return;
    }
    for i in 0..n {
        let s = *p.add(i);
        if !s.is_null() {
            gmshFree(s as *mut c_void);
        }
    }
    gmshFree(p as *mut c_void);
}

/// Free a C-allocated array of int* (for outDimTagsMap).
pub unsafe fn free_int_ptr_array(p: *mut *mut c_int, n: usize) {
    if p.is_null() {
        return;
    }
    for i in 0..n {
        let inner = *p.add(i);
        if !inner.is_null() {
            gmshFree(inner as *mut c_void);
        }
    }
    gmshFree(p as *mut c_void);
}

/// Free a C-allocated array of usize* (for outDimTagsMap_n).
pub unsafe fn free_size_ptr_array(p: *mut *mut usize, n: usize) {
    if p.is_null() {
        return;
    }
    for i in 0..n {
        let inner = *p.add(i);
        if !inner.is_null() {
            gmshFree(inner as *mut c_void);
        }
    }
    gmshFree(p as *mut c_void);
}

/// Free a C-allocated array of usize** (for elementTags/nodeTags).
pub unsafe fn free_size_ptr_ptr_array(p: *mut *mut *mut usize, n: usize) {
    if p.is_null() {
        return;
    }
    for i in 0..n {
        let inner = *p.add(i);
        if !inner.is_null() {
            gmshFree(inner as *mut c_void);
        }
    }
    gmshFree(p as *mut c_void);
}
