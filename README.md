# geommesh

Geometry construction and meshing for Rust, built on [Gmsh](https://gmsh.info)'s
C API.

Describe 0–3D geometry from primitives (points, lines, polylines, circles,
polygons, disks, rectangles, spheres, boxes, …), combine it with boolean
operations, label parts of the model, and generate a tetrahedral mesh you can
read back in Rust and write to `.msh`.

> **Status:** Phase 2 — pre-release. The Rust API is usable today; Python
> bindings are planned for Phase 3. Developed and tested against Gmsh
> **4.15.2**.

## Requirements

Building the crate needs only Rust: the Gmsh shared library is loaded **at
runtime**, so there is no build-time dependency and nothing to link or copy.

Running a program that builds a mesh needs the Gmsh shared library (4.15.2 is
tested; other 4.x versions may work, but are not verified).

### Getting the library

- **Official Python wheel** (published on PyPI by the Gmsh author) — the
  simplest cross-platform route:

  ```sh
  pip install gmsh==4.15.2
  ```

- **Conda:** `conda install -c conda-forge gmsh`.
- **Linux packages:** many distributions ship a `gmsh`/`libgmsh` package.
- **Manual SDK:** download the `-sdk` archive for your platform from
  <https://gmsh.info/#Download> and unzip it wherever you like (the shared
  library lives in its `lib/` directory).

### Pointing geommesh at the library

The library is searched in this order:

1. `GEOMESH_GMSH_LIBRARY` — full path to the shared library file;
2. `GEOMESH_GMSH_DIR` — a directory that is searched a few levels deep for a
   library file;
3. the directory containing the running executable;
4. typical install locations (Python/conda environments, `Program Files`,
   WinGet packages, `/opt`, `/usr/local/lib`, `/usr/lib`) and then every
   directory on `PATH`.

The environment variables are authoritative: if one is set but unusable,
nothing else is tried and you get a precise error (`No such file`, "not a
Gmsh library", …) listing what was searched. When auto-detection is good
enough, no configuration is needed at all.

```sh
# Windows (cmd)
set GEOMESH_GMSH_LIBRARY=C:\path\to\gmsh-4.15.dll

# Linux / macOS
export GEOMESH_GMSH_DIR=/path/to/gmsh-sdk
```

To see which file was loaded, call `geommesh::ffi::loaded_library()`.

## Quick start

```rust,no_run
use geommesh::{BooleanOp, MeshSettings, Shape, build};

let block = Shape::Cube {
    min: [0.0, 0.0, 0.0],
    max: [1.0, 1.0, 1.0],
    label: Some("block".into()),
};
let hole = Shape::Sphere {
    center: [0.5, 0.5, 0.5],
    radius: 0.2,
    label: Some("hole".into()),
};
let part = Shape::Boolean {
    op: BooleanOp::Difference,
    operands: vec![block, hole],
    label: Some("part_with_hole".into()),
};

let mesh = build(&[part], &MeshSettings::default())?;
assert!(mesh.physical_group("part_with_hole").is_some());
mesh.write("part.msh")?;
```

Labeled shapes become physical groups; a label disappears when its shape is
consumed by a boolean operation. `build()` returns a `Mesh` with nodes,
elements, physical groups and metadata (also available without Gmsh from
`Mesh::read` / `Mesh::write` for meshes in Gmsh's 2.2 ASCII format).

## Development (this repository)

```sh
cargo xtask fetch-gmsh   # install the official SDK into .sandbox/ (via pip or uv)
cargo test               # unit + integration tests (build a real mesh)
cargo run --bin prototype      # raw-FFI smoke test
cargo run --bin phase2_test    # build pipeline assertions
cargo run --bin deep_beam_test # mixed-dimensional geometry test
```

`.cargo/config.toml` points cargo-run processes (tests, example binaries) at
`.sandbox/` when you have not configured a Gmsh yourself; an explicit
`GEOMESH_GMSH_LIBRARY` or `GEOMESH_GMSH_DIR` always wins. You can also unzip
any Gmsh SDK into `.sandbox/` manually — the search goes a few levels deep.

`cargo doc --no-deps` builds the API documentation; the `ffi` module documents
the raw bindings and the loader contract.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
