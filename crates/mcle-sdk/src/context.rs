use crate::cloud::CloudId;
use crate::scope::Scope;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SandboxId(pub String);

impl SandboxId {
    pub fn default_id() -> Self {
        SandboxId("default".into())
    }
}

impl Default for SandboxId {
    fn default() -> Self {
        Self::default_id()
    }
}

impl fmt::Display for SandboxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Principal {
    pub cloud: Option<CloudId>,
    pub id: String,
    pub kind: String,
    pub access_key: Option<String>,
    pub account: Option<String>,
    pub region: Option<String>,
    #[serde(default)]
    pub claims: Value,
}

impl Principal {
    pub fn anonymous() -> Self {
        Principal {
            kind: "anonymous".into(),
            id: "anonymous".into(),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub request_id: String,
    pub sandbox: SandboxId,
    pub cloud: CloudId,
    pub service: String,
    pub op: String,
    pub principal: Principal,
    pub scope: Scope,
    pub public_base_url: String,
}
