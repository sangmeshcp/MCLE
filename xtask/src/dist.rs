//! `cargo xtask dist`: release archives and checksums.

use crate::manifest::root;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

fn version() -> Result<String> {
    let text = std::fs::read_to_string(root().join("Cargo.toml"))?;
    let doc: toml::Value = toml::from_str(&text)?;
    doc.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .context("workspace.package.version missing")
}

// Directory name the Dockerfile reads for a target (matches buildx TARGETOS-TARGETARCH).
fn docker_platform(target: &str) -> Option<&'static str> {
    match target {
        "x86_64-unknown-linux-musl" => Some("linux-amd64"),
        "aarch64-unknown-linux-musl" => Some("linux-arm64"),
        _ => None,
    }
}

fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("{cmd:?} failed: {status}");
    }
    Ok(())
}

/// Packages `mcle` for `target` into `out`.
/// `binary`: an already-built binary (e.g. a lipo'd macOS universal build); built with cargo if `None`.
pub fn package(target: &str, out: &Path, binary: Option<PathBuf>) -> Result<PathBuf> {
    let bin = match binary {
        Some(b) => b,
        None => {
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            run(Command::new(cargo)
                .args([
                    "build",
                    "--release",
                    "--locked",
                    "-p",
                    "mcle-cli",
                    "--target",
                    target,
                ])
                .current_dir(root()))?;
            root()
                .join("target")
                .join(target)
                .join("release")
                .join("mcle")
        }
    };
    if !bin.exists() {
        bail!("binary not found: {}", bin.display());
    }
    let name = format!("mcle-{}-{target}", version()?);
    let stage = out.join("stage").join(&name);
    std::fs::create_dir_all(&stage)?;
    std::fs::copy(&bin, stage.join("mcle"))?;
    for f in ["LICENSE", "README.md"] {
        std::fs::copy(root().join(f), stage.join(f))?;
    }
    let archive = out.join(format!("{name}.tar.gz"));
    run(Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(out.join("stage"))
        .arg(&name))?;
    std::fs::remove_dir_all(out.join("stage"))?;
    if let Some(p) = docker_platform(target) {
        let dir = out.join("bin").join(p);
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(&bin, dir.join("mcle"))?;
    }
    println!("dist: wrote {}", archive.display());
    Ok(archive)
}

/// Writes `SHA256SUMS` for every archive in `dir` (sorted, `sha256sum -c` compatible).
pub fn checksums(dir: &Path) -> Result<()> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".tar.gz"))
        .collect();
    files.sort();
    if files.is_empty() {
        bail!("no archives in {}", dir.display());
    }
    let mut out = String::new();
    for f in &files {
        let digest = Sha256::digest(std::fs::read(f)?);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push_str(&format!("{hex}  {name}\n"));
    }
    std::fs::write(dir.join("SHA256SUMS"), out)?;
    println!("dist: wrote {}", dir.join("SHA256SUMS").display());
    Ok(())
}
