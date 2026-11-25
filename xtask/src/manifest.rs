use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceToml {
    pub service: ServiceSection,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub crud: CrudSection,
    #[serde(default)]
    pub ops: BTreeMap<String, OpOverride>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceSection {
    pub cloud: String,
    pub name: String,
    pub display: String,
    #[serde(default = "default_tier")]
    pub tier: String,
    #[serde(default)]
    pub engines: Vec<String>,
    #[serde(default)]
    pub owners: Vec<String>,
    #[serde(default = "default_facade")]
    pub facade: String,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub case_insensitive_paths: bool,
    pub api: ApiSection,
}

fn default_tier() -> String {
    "T2".into()
}
fn default_facade() -> String {
    "generic".into()
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiSection {
    pub source: String,
    pub format: String,
    pub spec: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub iam_catalog: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Defaults {
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub plane: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct CrudSection {
    #[serde(default)]
    pub resources: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct OpOverride {
    pub level: Option<String>,
    pub iam_action: Option<String>,
    pub resource: Option<String>,
    pub plane: Option<String>,
    pub lro: Option<String>,
    #[serde(default)]
    pub emits: Vec<String>,
    pub crud: Option<String>,
    #[serde(default)]
    pub custom: bool,
}

#[derive(Debug, Clone)]
pub struct ServiceEntry {
    pub dir: PathBuf,
    pub toml: ServiceToml,
}

impl ServiceEntry {
    pub fn crate_name(&self) -> String {
        format!(
            "mcle-svc-{}-{}",
            self.toml.service.cloud, self.toml.service.name
        )
    }
    pub fn feature(&self) -> String {
        format!("{}-{}", self.toml.service.cloud, self.toml.service.name)
    }
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn load_all() -> Result<Vec<ServiceEntry>> {
    let mut out = Vec::new();
    let services = root().join("services");
    if !services.exists() {
        return Ok(out);
    }
    let mut clouds: Vec<_> = std::fs::read_dir(&services)?
        .filter_map(|e| e.ok())
        .collect();
    clouds.sort_by_key(|e| e.file_name());
    for c in clouds {
        if !c.path().is_dir() {
            continue;
        }
        let mut svcs: Vec<_> = std::fs::read_dir(c.path())?
            .filter_map(|e| e.ok())
            .collect();
        svcs.sort_by_key(|e| e.file_name());
        for s in svcs {
            let p = s.path().join("service.toml");
            if p.exists() {
                let text = std::fs::read_to_string(&p)?;
                let toml: ServiceToml =
                    toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
                let expect_cloud = c.file_name().to_string_lossy().to_string();
                if toml.service.cloud != expect_cloud {
                    bail!(
                        "{}: cloud `{}` does not match directory `{expect_cloud}`",
                        p.display(),
                        toml.service.cloud
                    );
                }
                out.push(ServiceEntry {
                    dir: s.path(),
                    toml,
                });
            }
        }
    }
    Ok(out)
}

pub fn clouds() -> Result<Vec<String>> {
    let mut v: Vec<String> = std::fs::read_dir(root().join("clouds"))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("Cargo.toml").exists())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    Ok(v)
}
