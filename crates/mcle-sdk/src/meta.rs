use crate::event::EventKind;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FidelityLevel {
    L0,
    L1,
    L2,
    L3,
}

impl FidelityLevel {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "L0" => Some(Self::L0),
            "L1" => Some(Self::L1),
            "L2" => Some(Self::L2),
            "L3" => Some(Self::L3),
            _ => None,
        }
    }
}

impl fmt::Display for FidelityLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IamAction(pub &'static str);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceExtractor {
    Scope,
    PathParam(&'static str),
    BodyField(&'static str),
    QueryParam(&'static str),
    Template(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LroBehavior {
    None,
    Create,
    Update,
    Delete,
    Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idempotency {
    None,
    Natural,
    Token(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Plane {
    Control,
    Data,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrudVerb {
    Create,
    Read,
    Update,
    Delete,
    List,
    Upsert,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrudBinding {
    pub resource: &'static str,
    pub verb: CrudVerb,
}

#[derive(Clone, Copy, Debug)]
pub struct OpMeta {
    pub name: &'static str,
    pub iam_action: IamAction,
    pub resource: ResourceExtractor,
    pub level: FidelityLevel,
    pub lro: LroBehavior,
    pub emits: &'static [EventKind],
    pub idempotency: Idempotency,
    pub plane: Plane,
    pub crud: Option<CrudBinding>,
}
