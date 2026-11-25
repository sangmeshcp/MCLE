use crate::manifest::root;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::process::Command;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Class {
    Sdk,
    Core,
    Engine,
    Cloud,
    Service,
    Other,
}

fn classify(name: &str, manifest: &str) -> Class {
    let rel = manifest.replace('\\', "/");
    if name == "mcle-sdk" {
        Class::Sdk
    } else if rel.contains("/engines/") {
        Class::Engine
    } else if rel.contains("/clouds/") {
        Class::Cloud
    } else if rel.contains("/services/") {
        Class::Service
    } else if [
        "mcle-core",
        "mcle-persist",
        "mcle-iam",
        "mcle-identity",
        "mcle-runtime",
        "mcle-gateway",
        "mcle-testkit",
    ]
    .contains(&name)
    {
        Class::Core
    } else {
        Class::Other
    }
}

pub fn run() -> Result<()> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root())
        .output()
        .context("running cargo metadata")?;
    if !out.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let meta: Value = serde_json::from_slice(&out.stdout)?;
    let pkgs = meta["packages"].as_array().cloned().unwrap_or_default();
    let classes: BTreeMap<String, Class> = pkgs
        .iter()
        .map(|p| {
            let n = p["name"].as_str().unwrap_or("").to_string();
            let m = p["manifest_path"].as_str().unwrap_or("");
            (n.clone(), classify(&n, m))
        })
        .collect();
    let mut errors = Vec::new();
    for p in &pkgs {
        let name = p["name"].as_str().unwrap_or("");
        let me = classes.get(name).copied().unwrap_or(Class::Other);
        for d in p["dependencies"].as_array().into_iter().flatten() {
            let dn = d["name"].as_str().unwrap_or("");
            let Some(dc) = classes.get(dn).copied() else {
                continue;
            };
            let kind = d["kind"].as_str().unwrap_or("normal");
            let bad = match me {
                Class::Sdk => true,
                Class::Core | Class::Engine => matches!(dc, Class::Cloud | Class::Service),
                Class::Cloud => matches!(dc, Class::Service | Class::Cloud) && dn != name,
                Class::Service => dc == Class::Service,
                Class::Other => false,
            };
            if bad {
                errors.push(format!(
                    "{name} ({me:?}) must not depend on {dn} ({dc:?}) [{kind}]"
                ));
            }
        }
    }
    if !errors.is_empty() {
        bail!("dependency rule violations:\n  {}", errors.join("\n  "));
    }
    println!("check-deps: ok ({} workspace crates)", pkgs.len());
    Ok(())
}
