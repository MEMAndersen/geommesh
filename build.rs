//! Build script for geommesh.
//!
//! Links against the vendored Gmsh SDK (gmsh-4.15.dll + gmsh.dll.lib) and
//! copies the DLL next to the build artifacts so that `cargo run` and
//! `cargo test` can find it at runtime.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // Locate the Gmsh SDK. It is expected in .sandbox relative to the crate root.
    let crate_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let sdk_dir = crate_root
        .join(".sandbox")
        .join("gmsh-4.15.2-Windows64-sdk");

    let lib_dir = sdk_dir.join("lib"); // the DLL ships in lib/, next to the import library
    let dll_name = "gmsh-4.15.dll";
    let lib_name = "gmsh.dll.lib";

    // Tell the Rust linker where to find the import library.
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib={}", lib_name.trim_end_matches(".lib"));

    // Tell cargo to re-run if the SDK changes.
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join(lib_name).display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join(dll_name).display()
    );

    let src_dll = lib_dir.join(dll_name);
    if !src_dll.exists() {
        eprintln!(
            "warning: Gmsh DLL not found at {}; the built binaries will not run unless \
             the DLL is on PATH",
            src_dll.display()
        );
        return;
    }

    // Copy the DLL where the linker/loader will look for it:
    // * OUT_DIR — keeps a copy alongside this build's artifacts
    // * <target>/<profile> — next to `cargo run` binaries
    // * <target>/<profile>/deps — next to test executables
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut destinations: Vec<PathBuf> = vec![out_dir.clone()];
    if let Some(profile_dir) = out_dir.ancestors().nth(3) {
        destinations.push(profile_dir.to_path_buf());
        let deps = profile_dir.join("deps");
        if deps.is_dir() {
            destinations.push(deps);
        }
    }

    for dir in destinations {
        copy(&src_dll, &dir.join(dll_name));
    }
}

fn copy(src: &Path, dst: &Path) {
    if let Err(e) = fs::copy(src, dst) {
        eprintln!(
            "warning: failed to copy {} to {}: {}",
            src.display(),
            dst.display(),
            e
        );
    }
}
