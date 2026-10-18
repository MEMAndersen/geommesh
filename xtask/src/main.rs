//! Development tasks for the geommesh repository.
//!
//! Run through the workspace alias: `cargo xtask <task>`.
//!
//! `fetch-gmsh` obtains the official Gmsh SDK — published by the Gmsh author
//! on PyPI as the `gmsh` package — through pip (or uv) and installs it into
//! `.sandbox/`, where the integration tests and example binaries pick it up
//! automatically (see `.cargo/config.toml`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The Gmsh version this repository is developed and tested against.
const GMSH_VERSION: &str = "4.15.2";

fn main() {
    let mut args = std::env::args().skip(1);
    let task = args.next();
    let rest: Vec<String> = args.collect();

    let result = match task.as_deref() {
        Some("fetch-gmsh") => fetch_gmsh(&rest),
        Some("help" | "--help" | "-h") | None => {
            print_usage();
            return;
        }
        Some(other) => Err(format!("unknown task '{other}' (try `cargo xtask help`)")),
    };

    if let Err(message) = result {
        eprintln!("xtask: error: {message}");
        std::process::exit(1);
    }
}

fn print_usage() {
    print!(
        "\
geommesh development tasks

usage: cargo xtask <task> [options]

tasks:
  fetch-gmsh [--force]
      Install the official Gmsh {GMSH_VERSION} SDK into .sandbox/, where
      integration tests and example binaries pick it up. Downloads the
      PyPI package `gmsh=={GMSH_VERSION}` (published by the Gmsh author)
      through pip or uv; pip/uv verify the index-provided hashes.
  help
      Show this text.
"
    );
}

/// The repository root (the parent of this `xtask` crate).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the repository")
        .to_path_buf()
}

/// `cargo xtask fetch-gmsh [--force]`
///
/// Install `gmsh=={GMSH_VERSION}` from PyPI into `.sandbox/gmsh-{version}`.
fn fetch_gmsh(args: &[String]) -> Result<(), String> {
    let mut force = false;
    for arg in args {
        match arg.as_str() {
            "--force" => force = true,
            other => return Err(format!("unrecognized option '{other}' for fetch-gmsh")),
        }
    }

    let destination = repo_root()
        .join(".sandbox")
        .join(format!("gmsh-{GMSH_VERSION}"));

    if !force {
        if let Some(found) = find_library(&destination, 4) {
            println!(
                "Gmsh {GMSH_VERSION} is already installed: {}",
                found.display()
            );
            println!("(rerun with --force to reinstall)");
            return Ok(());
        }
    }

    if destination.exists() {
        std::fs::remove_dir_all(&destination)
            .map_err(|e| format!("could not remove {}: {e}", destination.display()))?;
    }
    std::fs::create_dir_all(&destination)
        .map_err(|e| format!("could not create {}: {e}", destination.display()))?;

    let installer = find_installer()?;
    println!(
        "Installing gmsh=={GMSH_VERSION} into {} (via {})",
        destination.display(),
        installer.describe()
    );
    let status = installer
        .install_command(&destination)
        .status()
        .map_err(|e| format!("could not run {}: {e}", installer.describe()))?;
    if !status.success() {
        return Err(format!("the package installer failed ({status})"));
    }

    match find_library(&destination, 4) {
        Some(found) => {
            println!();
            println!("Gmsh {GMSH_VERSION} installed: {}", found.display());
            println!("Tests and example binaries pick it up automatically from .sandbox/.");
            Ok(())
        }
        None => Err(format!(
            "the installer reported success, but no Gmsh shared library was found under {}",
            destination.display()
        )),
    }
}

/// How to install a Python package.
struct Installer {
    program: String,
    /// Leading arguments, e.g. `-3` for the `py` launcher.
    prefix: Vec<String>,
    kind: InstallerKind,
}

enum InstallerKind {
    /// `python -m pip install …`
    Pip,
    /// `uv pip install …`
    Uv,
}

impl Installer {
    fn pip(program: impl Into<String>, prefix: Vec<String>) -> Self {
        Installer {
            program: program.into(),
            prefix,
            kind: InstallerKind::Pip,
        }
    }

    fn uv() -> Self {
        Installer {
            program: "uv".into(),
            prefix: Vec::new(),
            kind: InstallerKind::Uv,
        }
    }

    fn describe(&self) -> String {
        let mut parts = vec![self.program.clone()];
        parts.extend(self.prefix.iter().cloned());
        parts.join(" ")
    }

    /// Can this tool actually run? (`py -3` exists even when its registered
    /// interpreter is gone, so probing matters.)
    fn is_usable(&self) -> bool {
        let probe: &[&str] = match self.kind {
            InstallerKind::Pip => &["-c", "import sys, pip; print(sys.version.split()[0])"],
            InstallerKind::Uv => &["--version"],
        };
        let mut command = Command::new(&self.program);
        command
            .args(&self.prefix)
            .args(probe)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn install_command(&self, destination: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.prefix);
        let subcommand: &[&str] = match self.kind {
            InstallerKind::Pip => &["-m", "pip", "install", "--no-deps", "--upgrade", "--target"],
            InstallerKind::Uv => &["pip", "install", "--no-deps", "--target"],
        };
        command
            .args(subcommand)
            .arg(destination)
            .arg(format!("gmsh=={GMSH_VERSION}"));
        command
    }
}

/// Find something that can install from PyPI, preferring plain pip.
fn find_installer() -> Result<Installer, String> {
    let mut candidates: Vec<Installer> = Vec::new();

    if let Some(explicit) = std::env::var_os("PYTHON") {
        candidates.push(Installer::pip(
            explicit.to_string_lossy().into_owned(),
            vec![],
        ));
    }
    if cfg!(windows) {
        candidates.push(Installer::pip("py", vec!["-3".into()]));
    }
    candidates.push(Installer::pip("python3", vec![]));
    candidates.push(Installer::pip("python", vec![]));
    candidates.push(Installer::uv());

    for candidate in candidates {
        if candidate.is_usable() {
            return Ok(candidate);
        }
    }

    Err(
        "no way to install Python packages found: tried %PYTHON%, `py -3`, `python3`, \
         `python` (each with pip) and `uv`. Install one of them, or fetch the Gmsh SDK \
         manually — see README.md"
            .into(),
    )
}

/// Scan for a Gmsh shared library, mirroring the runtime loader's matching
/// rules (kept local so xtask stays dependency-free).
fn find_library(root: &Path, max_depth: usize) -> Option<PathBuf> {
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
            } else if file_type.is_file()
                && entry.file_name().to_str().is_some_and(is_library_file_name)
            {
                found.push(entry.path());
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
