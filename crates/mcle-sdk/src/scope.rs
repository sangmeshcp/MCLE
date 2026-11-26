use crate::cloud::CloudId;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Scope {
    Aws {
        account: String,
        region: String,
    },
    Azure {
        tenant: Uuid,
        subscription: Uuid,
        resource_group: Option<String>,
        location: Option<String>,
    },
    Gcp {
        project: String,
        location: String,
    },
    Oci {
        tenancy: String,
        compartment: String,
        region: String,
    },
    Ibm {
        account: String,
        resource_group: String,
        region: String,
    },
    Ext {
        cloud: CloudId,
        parts: Vec<String>,
    },
}

impl Scope {
    pub fn cloud(&self) -> CloudId {
        match self {
            Scope::Aws { .. } => CloudId::AWS,
            Scope::Azure { .. } => CloudId::AZURE,
            Scope::Gcp { .. } => CloudId::GCP,
            Scope::Oci { .. } => CloudId::OCI,
            Scope::Ibm { .. } => CloudId::IBM,
            Scope::Ext { cloud, .. } => cloud.clone(),
        }
    }

    pub fn key(&self) -> String {
        self.to_string()
    }

    pub fn ancestors(&self) -> Vec<Scope> {
        match self {
            Scope::Azure {
                tenant,
                subscription,
                resource_group: Some(_),
                ..
            } => vec![Scope::Azure {
                tenant: *tenant,
                subscription: *subscription,
                resource_group: None,
                location: None,
            }],
            _ => Vec::new(),
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::Aws { account, region } => write!(f, "aws/{account}/{region}"),
            Scope::Azure {
                subscription,
                resource_group,
                ..
            } => match resource_group {
                Some(rg) => write!(f, "azure/{subscription}/{}", rg.to_ascii_lowercase()),
                None => write!(f, "azure/{subscription}"),
            },
            Scope::Gcp { project, location } => write!(f, "gcp/{project}/{location}"),
            Scope::Oci {
                compartment,
                region,
                ..
            } => write!(f, "oci/{compartment}/{region}"),
            Scope::Ibm {
                account, region, ..
            } => write!(f, "ibm/{account}/{region}"),
            Scope::Ext { cloud, parts } => write!(f, "{cloud}/{}", parts.join("/")),
        }
    }
}
