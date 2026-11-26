use crate::cloud::CloudId;
use crate::context::Principal;
use crate::scope::Scope;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IamMode {
    Off,
    #[default]
    Audit,
    Enforce,
}

impl IamMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "audit" => Some(Self::Audit),
            "enforce" => Some(Self::Enforce),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthzRequest {
    pub cloud: CloudId,
    pub principal: Principal,
    pub action: String,
    pub resource: Option<String>,
    pub scope: Scope,
    pub context: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PolicySet {
    pub policies: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
pub enum Decision {
    Allow {
        matched: Vec<String>,
    },
    Deny {
        reason: String,
        matched: Vec<String>,
    },
}

impl Decision {
    pub fn is_allow(&self) -> bool {
        matches!(self, Decision::Allow { .. })
    }
}
