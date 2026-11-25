pub mod discovery;
pub mod openapi;
pub mod smithy;

use crate::ir::IrService;
use anyhow::{bail, Result};
use serde_json::Value;

pub fn load(format: &str, doc: &Value, protocol_override: Option<&str>) -> Result<IrService> {
    let mut svc = match format {
        "smithy" => smithy::parse(doc)?,
        "openapi" => openapi::parse(doc)?,
        "discovery" => discovery::parse(doc)?,
        other => bail!("unknown spec format `{other}` (expected smithy|openapi|discovery)"),
    };
    if let Some(p) = protocol_override {
        svc.protocol = p.to_string();
    }
    Ok(svc)
}
