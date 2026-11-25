use crate::context::RequestContext;
use crate::error::CloudError;
use crate::io::{OpInput, OpOutput};
use crate::meta::CrudVerb;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug)]
pub struct CrudResource {
    pub type_name: &'static str,
    pub hints: &'static [(&'static str, &'static str)],
}

impl CrudResource {
    pub fn hint(&self, key: &str) -> Option<&'static str> {
        self.hints.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrudTarget {
    pub id: String,
    pub name: String,
    pub parent: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrudRecord {
    pub id: String,
    pub name: String,
    pub resource_type: String,
    pub parent: String,
    pub body: Value,
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated: OffsetDateTime,
    pub generation: u64,
}

#[derive(Debug)]
pub enum CrudResult<'a> {
    One {
        record: &'a CrudRecord,
        created: bool,
    },
    Many(&'a [CrudRecord]),
    Deleted(&'a CrudRecord),
}

pub trait CrudShaper: Send + Sync {
    fn target(
        &self,
        ctx: &RequestContext,
        res: &CrudResource,
        verb: CrudVerb,
        input: &OpInput,
    ) -> Result<CrudTarget, CloudError>;

    fn create_body(
        &self,
        ctx: &RequestContext,
        res: &CrudResource,
        target: &CrudTarget,
        input: &OpInput,
    ) -> Result<Value, CloudError>;

    fn update_body(
        &self,
        _ctx: &RequestContext,
        _res: &CrudResource,
        existing: &CrudRecord,
        input: &OpInput,
    ) -> Result<Value, CloudError> {
        let mut body = existing.body.clone();
        if let (Some(dst), Some(src)) = (body.as_object_mut(), input.body.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        Ok(body)
    }

    fn render(
        &self,
        ctx: &RequestContext,
        res: &CrudResource,
        verb: CrudVerb,
        result: CrudResult<'_>,
    ) -> Result<OpOutput, CloudError>;

    fn not_found(&self, res: &CrudResource, target: &CrudTarget) -> CloudError {
        CloudError::not_found(format!("{} {} not found", res.type_name, target.name))
    }

    fn already_exists(&self, res: &CrudResource, target: &CrudTarget) -> CloudError {
        CloudError::already_exists(format!("{} {} already exists", res.type_name, target.name))
    }
}
