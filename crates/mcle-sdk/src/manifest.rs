use crate::cloud::CloudId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    T0,
    T1,
    T2,
}

#[derive(Clone, Debug)]
pub struct ServiceManifest {
    pub cloud: CloudId,
    pub name: &'static str,
    pub display: &'static str,
    pub tier: Tier,
    pub engines: &'static [&'static str],
    pub protocol: &'static str,
    pub signing_name: &'static str,
}
