//! Raw FFI bindings to the Gmsh C API (`gmshc.h`, Gmsh 4.15.2).
//!
//! The Gmsh shared library is loaded at **runtime** (through `libloading`),
//! not linked at build time: `cargo build` works on machines without Gmsh,
//! and the same binary can pick up whichever Gmsh library it finds when it
//! runs.
//!
//! # Finding the library
//!
//! [`ensure_loaded`] — or the first use of any function in this module, or
//! [`crate::build`], which does it for you — searches, in order:
//!
//! 1. `GEOMESH_GMSH_LIBRARY` — full path to the shared library file. This is
//!    authoritative: if it is set and unusable, nothing else is tried.
//! 2. `GEOMESH_GMSH_DIR` — a directory searched a few levels deep for a
//!    library file. Also authoritative.
//! 3. The directory containing the running executable.
//! 4. Typical install locations (conda prefixes, `Program Files`, `/opt`,
//!    `/usr/local/lib`, `/usr/lib`).
//! 5. Every directory on `PATH`.
//!
//! The file that was loaded is available as [`loaded_library`]. When nothing
//! is found, [`GmshLibraryError::NotFound`] lists every location that was
//! tried.
//!
//! # Version policy
//!
//! Every signature below was checked against the Gmsh 4.15.2 header. The
//! library is developed and tested with **Gmsh 4.15.2**; other 4.x versions
//! are not verified. The C API has no version query (only compile-time
//! macros), so a mismatched library is detected only indirectly, as missing
//! symbols.
//!
//! # Conventions
//!
//! Two conventions of the C API are easy to get wrong and are called out
//! here:
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

use std::path::{Path, PathBuf};

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
// Loading the shared library
// ---------------------------------------------------------------------------

/// Error raised while locating, loading, or binding to the Gmsh shared
/// library.
///
/// `Debug` prints the same human-readable text as `Display`, on purpose:
/// this error is usually seen for the first time through `main() ->
/// Result<_, _>`, whose default handler prints the `Debug` form — and that
/// form should tell the reader what to install or configure, not just show
/// struct fields.
pub enum GmshLibraryError {
    /// No usable Gmsh shared library was found.
    NotFound {
        /// Human-readable list of the locations that were tried.
        tried: Vec<String>,
    },
    /// A candidate file exists but could not be loaded.
    LoadFailed {
        /// The file that failed to load.
        path: PathBuf,
        /// The error reported by the operating system's loader.
        message: String,
    },
    /// The file loaded, but it does not export the full Gmsh 4.15 C API.
    MissingSymbols {
        /// The file that was loaded.
        path: PathBuf,
        /// The names that could not be resolved.
        missing: Vec<&'static str>,
    },
}

impl std::fmt::Debug for GmshLibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

impl std::fmt::Display for GmshLibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GmshLibraryError::NotFound { tried } => {
                writeln!(f, "the Gmsh shared library could not be found; tried:")?;
                for entry in tried {
                    writeln!(f, "  - {entry}")?;
                }
                write!(
                    f,
                    "Gmsh 4.15.2 is tested (other 4.x versions may work). Install Gmsh \
                     (e.g. `pip install gmsh==4.15.2`, or your package manager) and either \
                     put its library next to your executable or point GEOMESH_GMSH_LIBRARY \
                     at the library file (GEOMESH_GMSH_DIR at a directory containing it)."
                )
            }
            GmshLibraryError::LoadFailed { path, message } => write!(
                f,
                "found a Gmsh shared library at {} but failed to load it: {message}",
                path.display()
            ),
            GmshLibraryError::MissingSymbols { path, missing } => write!(
                f,
                "the shared library at {} does not look like Gmsh 4.15.2: {} Gmsh C API \
                 symbol(s) missing (first: {})",
                path.display(),
                missing.len(),
                missing.first().copied().unwrap_or("?")
            ),
        }
    }
}

impl std::error::Error for GmshLibraryError {}

/// The file the Gmsh library was loaded from.
static LOADED_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Make sure the Gmsh shared library is loaded and its API is resolved.
///
/// The first successful call wins; calling this again is cheap and returns
/// `Ok(())`. The raw functions in this module panic with the same message if
/// they are called before the library could be loaded, so call this first
/// (or [`crate::build`], which does it for you) if you want to handle a
/// missing library gracefully.
///
/// # Errors
///
/// Returns [`GmshLibraryError`] when no Gmsh shared library can be found or
/// loaded; see the type for the search order and the error details.
pub fn ensure_loaded() -> Result<(), GmshLibraryError> {
    if API.get().is_some() {
        return Ok(());
    }
    let api = load_api()?;
    let _ = API.set(api);
    Ok(())
}

/// Like [`ensure_loaded`], but load the library from the given file instead
/// of searching.
///
/// If a library has already been loaded, this is a no-op and returns
/// `Ok(())`.
///
/// # Errors
///
/// Returns [`GmshLibraryError`] if the file cannot be loaded or does not
/// export the expected API.
pub fn ensure_loaded_from(path: impl AsRef<Path>) -> Result<(), GmshLibraryError> {
    if API.get().is_some() {
        return Ok(());
    }
    let path = path.as_ref().to_path_buf();
    let library = open_exact(&path)?;
    let library: &'static libloading::Library = Box::leak(Box::new(library));
    let api = load_api_from(path, library)?;
    let _ = API.set(api);
    Ok(())
}

/// The file the Gmsh shared library was loaded from, if it has been loaded.
pub fn loaded_library() -> Option<&'static Path> {
    LOADED_PATH.get().map(PathBuf::as_path)
}

/// Locate a Gmsh shared library and load it.
///
/// The search order is described in the [module documentation](self).
fn open_library() -> Result<(PathBuf, libloading::Library), GmshLibraryError> {
    if let Some(explicit) = std::env::var_os("GEOMESH_GMSH_LIBRARY") {
        let path = PathBuf::from(explicit);
        let library = open_exact(&path)?;
        return Ok((path, library));
    }

    if let Some(dir) = std::env::var_os("GEOMESH_GMSH_DIR") {
        let dir = PathBuf::from(dir);
        let Some(library_path) = scan_dir_for_library(&dir, 4) else {
            return Err(GmshLibraryError::NotFound {
                tried: vec![format!("{} (no Gmsh shared library found)", dir.display())],
            });
        };
        let library = open_exact(&library_path)?;
        return Ok((library_path, library));
    }

    let mut candidates: Vec<(PathBuf, usize)> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push((dir.to_path_buf(), 1));
        }
    }
    candidates.extend(typical_install_dirs());

    let mut tried: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (dir, depth) in candidates {
        if !seen.insert(normalize_for_dedup(&dir)) {
            continue;
        }
        match scan_dir_for_library(&dir, depth) {
            Some(library_path) => match open_exact(&library_path) {
                Ok(library) => return Ok((library_path, library)),
                Err(error) => tried.push(format!("{} ({error})", library_path.display())),
            },
            None => tried.push(format!("{} (no Gmsh shared library found)", dir.display())),
        }
    }

    Err(GmshLibraryError::NotFound { tried })
}

/// Load exactly one file, with a precise error if that fails.
fn open_exact(path: &Path) -> Result<libloading::Library, GmshLibraryError> {
    if !path.is_file() {
        return Err(GmshLibraryError::LoadFailed {
            path: path.to_path_buf(),
            message: "file does not exist".into(),
        });
    }
    // SAFETY: loading a shared library runs its initializers, which is
    // exactly what we intend here.
    unsafe { libloading::Library::new(path) }.map_err(|error| GmshLibraryError::LoadFailed {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

/// Walk `root` (at most `max_depth` levels below it) and return the first
/// file whose name looks like a Gmsh shared library, if any.
///
/// Results are sorted by path so that for several candidates the choice is
/// deterministic (e.g. a versioned `gmsh-4.15.dll` comes before `gmsh.dll`).
fn scan_dir_for_library(root: &Path, max_depth: usize) -> Option<PathBuf> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut found: Vec<PathBuf> = Vec::new();

    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if depth < max_depth {
                    stack.push((entry.path(), depth + 1));
                }
            } else if file_type.is_file() {
                let looks_right = entry.file_name().to_str().is_some_and(is_library_file_name);
                if looks_right {
                    found.push(entry.path());
                }
            }
        }
    }

    found.sort();
    found.into_iter().next()
}

/// Does this file name look like a Gmsh shared library? (Windows)
#[cfg(windows)]
fn is_library_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.starts_with("gmsh") || lower.starts_with("libgmsh")) && lower.ends_with(".dll")
}

/// Does this file name look like a Gmsh shared library? (Unix)
#[cfg(unix)]
fn is_library_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("libgmsh") && lower.contains(".so")
}

/// How deep to search a Python-style prefix (a Python installation, a conda
/// environment or a virtual environment).
///
/// `pip install gmsh` puts the wheel's `lib/` directory below
/// `site-packages`, so these roots need more than one level; Unix keeps the
/// `pythonX.Y` layer in between, hence the extra level there.
#[cfg(windows)]
const PYTHON_PREFIX_DEPTH: usize = 3;
#[cfg(unix)]
const PYTHON_PREFIX_DEPTH: usize = 4;

/// Well-known directories that may contain a Gmsh shared library, in the
/// order they should be tried, paired with the scan depth to use.
fn typical_install_dirs() -> Vec<(PathBuf, usize)> {
    let mut dirs: Vec<(PathBuf, usize)> = Vec::new();

    // Conda environments: the classic layout, the Windows `Library` prefix
    // layout, and `pip install` inside the environment.
    if let Some(conda) = std::env::var_os("CONDA_PREFIX") {
        let base = PathBuf::from(conda);
        dirs.push((base.join("lib"), PYTHON_PREFIX_DEPTH));
        dirs.push((base.join("bin"), 1));
        #[cfg(windows)]
        {
            dirs.push((base.join("Lib"), PYTHON_PREFIX_DEPTH));
            dirs.push((base.join("Library").join("bin"), 1));
        }
    }

    // Python virtual environments.
    if let Some(venv) = std::env::var_os("VIRTUAL_ENV") {
        dirs.push((PathBuf::from(venv), PYTHON_PREFIX_DEPTH));
    }

    // A Gmsh SDK or installer under Program Files, Chocolatey packages, and
    // WinGet packages (which WinGet keeps in the user profile).
    #[cfg(windows)]
    {
        let mut bases: Vec<(PathBuf, usize)> = Vec::new();
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(program_files) = std::env::var_os(var) {
                bases.push((PathBuf::from(program_files), 2));
            }
        }
        if let Some(program_data) = std::env::var_os("ProgramData") {
            bases.push((
                PathBuf::from(program_data).join("chocolatey").join("lib"),
                2,
            ));
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            bases.push((
                PathBuf::from(local_app_data)
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Packages"),
                3,
            ));
        }
        for (base, depth) in bases {
            if let Ok(entries) = std::fs::read_dir(&base) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                    if name.contains("gmsh") {
                        dirs.push((entry.path(), depth));
                    }
                }
            }
        }
    }

    // System library directories (distributions ship Gmsh in several of
    // these) and `/opt` installs.
    #[cfg(unix)]
    {
        for lib_dir in ["/usr/local/lib", "/usr/lib", "/usr/lib64"] {
            dirs.push((PathBuf::from(lib_dir), 1));
        }
        for scan_root in ["/opt", "/usr/local"] {
            if let Ok(entries) = std::fs::read_dir(scan_root) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                    if name.contains("gmsh") {
                        dirs.push((entry.path(), 2));
                    }
                }
            }
        }
    }

    // Anything the user put on PATH; Python-style roots are searched deeper
    // because a plain `pip install` buries the library under
    // `site-packages`.
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let is_scripts = dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("Scripts"));
            if is_scripts {
                // A `Scripts` directory is the wrong side of a Python
                // prefix; search the prefix itself instead.
                if let Some(prefix) = dir.parent() {
                    dirs.push((prefix.to_path_buf(), PYTHON_PREFIX_DEPTH));
                }
            } else if is_python_prefix(&dir) {
                dirs.push((dir, PYTHON_PREFIX_DEPTH));
            } else {
                dirs.push((dir, 1));
            }
        }
    }

    dirs
}

/// Heuristic: does this PATH entry look like a Python prefix (a Python
/// installation, a virtual environment, …)? The wheel's `lib/` lives below
/// `site-packages` there, so it is worth searching a little deeper.
fn is_python_prefix(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    name.starts_with("python")
        || name == "site-packages"
        || name.ends_with(".venv")
        || name == "venv"
}

/// Key used to de-duplicate candidate directories (Windows paths are
/// case-insensitive and may carry trailing separators).
fn normalize_for_dedup(dir: &Path) -> String {
    let normalized: PathBuf = dir.components().collect();
    let mut key = normalized.to_string_lossy().into_owned();
    if cfg!(windows) {
        key.make_ascii_lowercase();
    }
    key
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

/// Declare the Gmsh C API: one line per function, written once.
///
/// For every listed function this generates
///
/// * a field in a function-pointer table that is filled in by resolving the
///   symbol in the loaded shared library, and
/// * a `pub unsafe fn` wrapper with the same name and signature, so call
///   sites look exactly like ordinary FFI calls.
///
/// On load, *all* symbols are resolved up front and every missing name is
/// collected into one [`GmshLibraryError::MissingSymbols`] error.
macro_rules! gmsh_api {
    ($(
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)?;
    )*) => {
        /// Function pointers into the loaded Gmsh shared library.
        ///
        /// The `non_snake_case` allowance must sit on the struct because a
        /// field-level attribute has no effect on names that come from a
        /// macro invocation.
        #[allow(non_snake_case)]
        struct GmshApi {
            $(
                $name: libloading::Symbol<'static, unsafe extern "C" fn($($ty),*) $(-> $ret)?>,
            )*
        }

        // SAFETY: the table holds plain function pointers into a library that
        // is deliberately never unloaded, and it is immutable after being
        // initialized once.
        unsafe impl Send for GmshApi {}
        unsafe impl Sync for GmshApi {}

        static API: std::sync::OnceLock<GmshApi> = std::sync::OnceLock::new();

        /// Resolve every symbol; collect all missing names for one clear
        /// error.
        fn load_api_from(
            path: std::path::PathBuf,
            library: &'static libloading::Library,
        ) -> Result<GmshApi, GmshLibraryError> {
            let mut missing: Vec<&'static str> = Vec::new();
            $(
                let found = unsafe {
                    library.get::<unsafe extern "C" fn($($ty),*) $(-> $ret)?>(
                        concat!(stringify!($name), "\0").as_bytes(),
                    )
                };
                if found.is_err() {
                    missing.push(stringify!($name));
                }
            )*
            if !missing.is_empty() {
                return Err(GmshLibraryError::MissingSymbols { path, missing });
            }

            let _ = LOADED_PATH.set(path);
            Ok(GmshApi {
                $(
                    $name: unsafe {
                        library.get::<unsafe extern "C" fn($($ty),*) $(-> $ret)?>(
                            concat!(stringify!($name), "\0").as_bytes(),
                        )
                    }
                    .expect("symbol presence was checked above"),
                )*
            })
        }

        /// Locate, load and resolve the whole Gmsh API.
        fn load_api() -> Result<GmshApi, GmshLibraryError> {
            let (path, library) = open_library()?;
            let library: &'static libloading::Library = Box::leak(Box::new(library));
            load_api_from(path, library)
        }

        /// The resolved API, loading it on first use.
        ///
        /// Panics with a descriptive message if the library cannot be
        /// loaded; call [`ensure_loaded`] first to handle that gracefully.
        fn api() -> &'static GmshApi {
            API.get_or_init(|| {
                load_api()
                    .unwrap_or_else(|e| panic!("geommesh could not load the Gmsh shared library\n\n{e}"))
            })
        }

        $(
            $(#[$meta])*
            #[allow(non_snake_case)]
            pub unsafe fn $name($($arg: $ty),*) $(-> $ret)? {
                (api().$name)($($arg),*)
            }
        )*
    };
}

gmsh_api! {
    // --- Top-level ---

    fn gmshInitialize(
        argc: c_int,
        argv: *mut *mut c_char,
        read_config_files: c_int,
        run: c_int,
        ierr: *mut c_int,
    );

    fn gmshIsInitialized(ierr: *mut c_int) -> c_int;
    fn gmshFinalize(ierr: *mut c_int);
    fn gmshClear(ierr: *mut c_int);
    fn gmshWrite(file_name: *const c_char, ierr: *mut c_int);

    /// Open a file (`.msh`, `.geo`, …). A file with model data creates a new
    /// model and makes it current.
    fn gmshOpen(file_name: *const c_char, ierr: *mut c_int);

    /// Human-readable description of the last error logged by Gmsh.
    fn gmshLoggerGetLastError(error: *mut *mut c_char, ierr: *mut c_int);

    // --- Options ---

    fn gmshOptionSetNumber(name: *const c_char, value: c_double, ierr: *mut c_int);
    fn gmshOptionSetString(name: *const c_char, value: *const c_char, ierr: *mut c_int);

    // --- Model ---

    fn gmshModelAdd(name: *const c_char, ierr: *mut c_int);
    fn gmshModelGetEntities(
        dim_tags: *mut *mut c_int,
        dim_tags_n: *mut usize,
        dim: c_int,
        ierr: *mut c_int,
    );
    fn gmshModelSetEntityName(dim: c_int, tag: c_int, name: *const c_char, ierr: *mut c_int);
    fn gmshModelGetEntityName(dim: c_int, tag: c_int, name: *mut *mut c_char, ierr: *mut c_int);
    fn gmshModelGetEntityType(
        dim: c_int,
        tag: c_int,
        entity_type: *mut *mut c_char,
        ierr: *mut c_int,
    );

    // --- Physical groups ---

    fn gmshModelAddPhysicalGroup(
        dim: c_int,
        tags: *const c_int,
        tags_n: usize,
        tag: c_int,
        name: *const c_char,
        ierr: *mut c_int,
    ) -> c_int;
    fn gmshModelSetPhysicalName(dim: c_int, tag: c_int, name: *const c_char, ierr: *mut c_int);
    fn gmshModelGetPhysicalName(
        dim: c_int,
        tag: c_int,
        name: *mut *mut c_char,
        ierr: *mut c_int,
    );
    fn gmshModelGetPhysicalGroups(
        dim_tags: *mut *mut c_int,
        dim_tags_n: *mut usize,
        dim: c_int,
        ierr: *mut c_int,
    );
    /// Returns the *entity tags* (within `dim`) contained in the physical
    /// group, not `(dim, tag)` pairs.
    fn gmshModelGetEntitiesForPhysicalGroup(
        dim: c_int,
        tag: c_int,
        tags: *mut *mut c_int,
        tags_n: *mut usize,
        ierr: *mut c_int,
    );

    // --- OCC geometry creation ---

    fn gmshModelOccAddPoint(
        x: c_double,
        y: c_double,
        z: c_double,
        mesh_size: c_double,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddLine(
        start_tag: c_int,
        end_tag: c_int,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddCircleArc(
        start_tag: c_int,
        middle_tag: c_int,
        end_tag: c_int,
        tag: c_int,
        center: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddCircle(
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

    fn gmshModelOccAddEllipse(
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

    fn gmshModelOccAddRectangle(
        x: c_double,
        y: c_double,
        z: c_double,
        dx: c_double,
        dy: c_double,
        tag: c_int,
        rounded_radius: c_double,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddDisk(
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

    fn gmshModelOccAddWire(
        curve_tags: *const c_int,
        curve_tags_n: usize,
        tag: c_int,
        check_closed: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddCurveLoop(
        curve_tags: *const c_int,
        curve_tags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddPlaneSurface(
        wire_tags: *const c_int,
        wire_tags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddSurfaceLoop(
        surface_tags: *const c_int,
        surface_tags_n: usize,
        tag: c_int,
        sewing: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddVolume(
        shell_tags: *const c_int,
        shell_tags_n: usize,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddSphere(
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

    fn gmshModelOccAddBox(
        x: c_double,
        y: c_double,
        z: c_double,
        dx: c_double,
        dy: c_double,
        dz: c_double,
        tag: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelOccAddCylinder(
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

    fn gmshModelOccAddCone(
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

    fn gmshModelOccAddWedge(
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

    fn gmshModelOccAddTorus(
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

    fn gmshModelOccFuse(
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

    fn gmshModelOccCut(
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

    fn gmshModelOccIntersect(
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

    fn gmshModelOccFragment(
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
    fn gmshModelOccSynchronize(ierr: *mut c_int);

    // --- Mesh ---

    fn gmshModelMeshGenerate(dim: c_int, ierr: *mut c_int);
    fn gmshModelMeshSetOrder(order: c_int, ierr: *mut c_int);
    fn gmshModelMeshSetSize(
        dim_tags: *const c_int,
        dim_tags_n: usize,
        size: c_double,
        ierr: *mut c_int,
    );

    fn gmshModelMeshGetNodes(
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

    fn gmshModelMeshGetElements(
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

    fn gmshModelMeshGetElement(
        element_tag: usize,
        element_type: *mut c_int,
        node_tags: *mut *mut usize,
        node_tags_n: *mut usize,
        dim: *mut c_int,
        tag: *mut c_int,
        ierr: *mut c_int,
    );

    fn gmshModelMeshGetElementType(
        family_name: *const c_char,
        order: c_int,
        serendip: c_int,
        ierr: *mut c_int,
    ) -> c_int;

    fn gmshModelMeshGetElementProperties(
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
    fn gmshFree(p: *mut c_void);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn library_file_names_are_recognized() {
        assert!(is_library_file_name("gmsh-4.15.dll"));
        assert!(is_library_file_name("gmsh.dll"));
        assert!(is_library_file_name("libgmsh-4.15.dll"));
        assert!(!is_library_file_name("gmsh.dll.lib"));
        assert!(!is_library_file_name("gmsh.exe"));
        assert!(!is_library_file_name("libgmsh.so.4.15"));
    }

    #[cfg(unix)]
    #[test]
    fn library_file_names_are_recognized() {
        assert!(is_library_file_name("libgmsh.so"));
        assert!(is_library_file_name("libgmsh.so.4.15"));
        assert!(is_library_file_name("libgmsh.so.4.15.2"));
        assert!(!is_library_file_name("gmsh-4.15.dll"));
        assert!(!is_library_file_name("libgmsh.h"));
    }

    #[test]
    fn python_prefix_heuristic() {
        assert!(is_python_prefix(Path::new("/opt/python313")));
        assert!(is_python_prefix(Path::new("/home/x/projects/.venv")));
        assert!(!is_python_prefix(Path::new("/usr/lib")));
        assert!(!is_python_prefix(Path::new("/home/x/.cargo/bin")));
    }

    #[test]
    fn directory_scan_finds_nested_library_and_respects_depth() {
        #[cfg(windows)]
        const LIB_NAME: &str = "gmsh-4.15.dll";
        #[cfg(unix)]
        const LIB_NAME: &str = "libgmsh.so.4.15";

        let root = std::env::temp_dir().join(format!("geommesh_scan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sdk").join("lib")).expect("create fixture");
        std::fs::write(root.join("sdk").join("lib").join(LIB_NAME), b"").expect("write fixture");
        std::fs::write(root.join("notes.txt"), b"").expect("write fixture");

        // The library sits two levels below the root: found with depth >= 2.
        let found = scan_dir_for_library(&root, 4).expect("library should be found");
        assert_eq!(found, root.join("sdk").join("lib").join(LIB_NAME));

        // With a smaller depth budget the same walk must come up empty.
        assert!(scan_dir_for_library(&root, 1).is_none());

        std::fs::remove_dir_all(&root).expect("clean up fixture");
    }
}
