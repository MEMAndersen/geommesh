//! Build script for geommesh.
//! Links against the vendored Gmsh SDK (gmsh-4.15.dll + gmsh.dll.lib)
//! and copies the DLL next to the output binary so it can be found at runtime.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Locate the Gmsh SDK. It is expected in .sandbox relative to the crate root.
    let crate_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let sdk_dir = crate_root.join(".sandbox").join("gmsh-4.15.2-Windows64-sdk");

    let lib_dir = sdk_dir.join("lib");
    let bin_dir = sdk_dir.join("lib"); // DLL is in lib/, not bin/
    let dll_name = "gmsh-4.15.dll";
    let lib_name = "gmsh.dll.lib";

    // Tell the Rust linker where to find the import library.
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib={}", lib_name.trim_end_matches(".lib"));

    // Tell cargo to re-run if the SDK changes.
    println!("cargo:rerun-if-changed={}", lib_dir.join(lib_name).display());
    println!("cargo:rerun-if-changed={}", bin_dir.join(dll_name).display());

    // Copy the DLL next to the target binary so the executable can find it.
    // We do this in a build script so both `cargo build` and `cargo run` work.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let _ = fs::create_dir_all(&out_dir);

    let src_dll = bin_dir.join(dll_name);
    let dst_dll = out_dir.join(dll_name);

    if src_dll.exists() {
        if let Err(e) = fs::copy(&src_dll, &dst_dll) {
            eprintln!("warning: failed to copy {}: {}", src_dll.display(), e);
        } else {
            println!("copied {} -> {}", src_dll.display(), dst_dll.display());
        }
    } else {
        eprintln!(
            "warning: Gmsh DLL not found at {}; the built binary will not run unless the DLL is on PATH",
            src_dll.display()
        );
    }
}
