use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CloudId(Cow<'static, str>);

impl CloudId {
    pub const AWS: CloudId = CloudId(Cow::Borrowed("aws"));
    pub const AZURE: CloudId = CloudId(Cow::Borrowed("azure"));
    pub const GCP: CloudId = CloudId(Cow::Borrowed("gcp"));
    pub const OCI: CloudId = CloudId(Cow::Borrowed("oci"));
    pub const IBM: CloudId = CloudId(Cow::Borrowed("ibm"));

    pub const fn ext(name: &'static str) -> Self {
        CloudId(Cow::Borrowed(name))
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "aws" => Self::AWS,
            "azure" => Self::AZURE,
            "gcp" => Self::GCP,
            "oci" => Self::OCI,
            "ibm" => Self::IBM,
            other => CloudId(Cow::Owned(other.to_ascii_lowercase())),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CloudId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
