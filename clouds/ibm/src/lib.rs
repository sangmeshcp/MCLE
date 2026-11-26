#![forbid(unsafe_code)]

use bytes::Bytes;
use mcle_identity::{bearer, jwt_claims};
use mcle_sdk::crud::CrudResult;
use mcle_sdk::error::ErrorKind;
use mcle_sdk::io::RequestHead;
use mcle_sdk::traits::DetectionRule;
use mcle_sdk::{
    CloudAdapter, CloudError, CloudId, CrudRecord, CrudResource, CrudShaper, CrudTarget, CrudVerb,
    Host, OpInput, OpOutput, Principal, RequestContext, Scope,
};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;

pub const DEFAULT_ACCOUNT: &str = "mcleaccount0000000000000000000";
pub const DEFAULT_RESOURCE_GROUP: &str = "mcledefaultresourcegroup00000";
pub const DEFAULT_REGION: &str = "us-south";

pub struct IbmAdapter {
    host: Host,
}

pub fn adapter(host: &Host) -> Arc<dyn CloudAdapter> {
    Arc::new(IbmAdapter { host: host.clone() })
}

static RULES: &[DetectionRule] = &[
    DetectionRule::HostContains("cloud.ibm"),
    DetectionRule::HostContains("bluemix"),
    DetectionRule::PathPrefix("/v2/resource_instances"),
    DetectionRule::PathPrefix("/v2/resource_keys"),
    DetectionRule::PathPrefix("/identity/token"),
];

pub fn default_code(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::InvalidArgument => "bad_request",
        ErrorKind::NotFound => "not_found",
        ErrorKind::AlreadyExists | ErrorKind::Conflict => "conflict",
        ErrorKind::Unauthenticated => "not_authenticated",
        ErrorKind::AccessDenied => "forbidden",
        ErrorKind::Throttled => "too_many_requests",
        ErrorKind::NotImplemented => "not_implemented",
        ErrorKind::Internal => "internal_error",
    }
}

pub fn ts(t: time::OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

impl CloudAdapter for IbmAdapter {
    fn cloud(&self) -> CloudId {
        CloudId::IBM
    }

    fn detection_rules(&self) -> &'static [DetectionRule] {
        RULES
    }

    fn parse_body(&self, head: &RequestHead, raw: &Bytes) -> Result<Value, CloudError> {
        if raw.is_empty() {
            return Ok(Value::Null);
        }
        if head
            .header("content-type")
            .unwrap_or("")
            .starts_with("application/x-www-form-urlencoded")
        {
            let s = String::from_utf8_lossy(raw);
            return Ok(Value::Object(
                mcle_sdk::util::parse_query(&s)
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect(),
            ));
        }
        serde_json::from_slice(raw).map_err(|e| CloudError::invalid(format!("Invalid JSON: {e}")))
    }

    fn principal(&self, head: &RequestHead) -> Principal {
        match head
            .header("authorization")
            .and_then(bearer)
            .and_then(jwt_claims)
        {
            Some(c) => Principal {
                cloud: Some(CloudId::IBM),
                id: c
                    .get("iam_id")
                    .or(c.get("sub"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                kind: c
                    .get("sub_type")
                    .and_then(Value::as_str)
                    .unwrap_or("user")
                    .to_string(),
                access_key: None,
                account: c
                    .pointer("/account/bss")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                region: None,
                claims: c,
            },
            None => Principal {
                cloud: Some(CloudId::IBM),
                ..Principal::anonymous()
            },
        }
    }

    fn scope(&self, _head: &RequestHead, p: &Principal, input: &OpInput) -> Scope {
        Scope::Ibm {
            account: p.account.clone().unwrap_or_else(|| DEFAULT_ACCOUNT.into()),
            resource_group: input
                .body_str("resource_group")
                .or(input.query("resource_group_id"))
                .unwrap_or(DEFAULT_RESOURCE_GROUP)
                .to_string(),
            region: input
                .body_str("target")
                .unwrap_or(DEFAULT_REGION)
                .to_string(),
        }
    }

    fn request_id_header(&self) -> &'static str {
        "x-request-id"
    }

    fn render_error(&self, ctx: &RequestContext, _protocol: &str, err: &CloudError) -> OpOutput {
        let code = if err.code.is_empty() {
            default_code(err.kind)
        } else {
            err.code.as_str()
        };
        OpOutput::json_status(
            err.http_status,
            json!({
                "errors": [{"code": code, "message": err.message, "more_info": "https://github.com/sangmeshcp/MCLE/blob/main/STATUS.md"}],
                "trace": ctx.request_id,
                "status_code": err.http_status,
                "message": err.message,
            }),
        )
    }

    fn crud_shaper(&self, protocol: &str) -> Option<Arc<dyn CrudShaper>> {
        (protocol == "ibmRest").then(|| {
            Arc::new(IbmShaper {
                host: self.host.clone(),
            }) as Arc<dyn CrudShaper>
        })
    }

    fn env(&self, base_url: &str) -> Vec<(String, String)> {
        vec![
            ("IBMCLOUD_API_KEY".into(), "mcle".into()),
            ("IAM_URL".into(), base_url.into()),
            ("RESOURCE_CONTROLLER_URL".into(), base_url.into()),
            ("RESOURCE_CONTROLLER_AUTH_TYPE".into(), "iam".into()),
            ("RESOURCE_CONTROLLER_APIKEY".into(), "mcle".into()),
            ("RESOURCE_CONTROLLER_AUTH_URL".into(), base_url.into()),
        ]
    }
}

pub struct IbmShaper {
    host: Host,
}

fn guid_of(id: &str) -> String {
    if id.starts_with("crn:") {
        id.split(':').nth(7).unwrap_or(id).to_string()
    } else {
        id.to_string()
    }
}

impl IbmShaper {
    fn view(&self, rec: &CrudRecord) -> Value {
        let mut b = rec.body.clone();
        if let Some(o) = b.as_object_mut() {
            o.insert("created_at".into(), Value::String(ts(rec.created)));
            o.insert("updated_at".into(), Value::String(ts(rec.updated)));
        }
        b
    }
}

impl CrudShaper for IbmShaper {
    fn target(
        &self,
        ctx: &RequestContext,
        res: &CrudResource,
        verb: CrudVerb,
        input: &OpInput,
    ) -> Result<CrudTarget, CloudError> {
        let account = match &ctx.scope {
            Scope::Ibm { account, .. } => account.clone(),
            _ => DEFAULT_ACCOUNT.into(),
        };
        let parent = format!("{account}/");
        match verb {
            CrudVerb::List => Ok(CrudTarget {
                id: String::new(),
                name: String::new(),
                parent,
            }),
            CrudVerb::Create => {
                let name = input
                    .body_str(res.hint("name_field").unwrap_or("name"))
                    .ok_or_else(|| CloudError::invalid("name is required"))?
                    .to_string();
                let guid = self.host.ids.uuid().to_string();
                Ok(CrudTarget {
                    id: guid,
                    name,
                    parent,
                })
            }
            _ => {
                let raw = input
                    .param(res.hint("id_param").unwrap_or("id"))
                    .ok_or_else(|| CloudError::invalid("id is required"))?;
                Ok(CrudTarget {
                    id: guid_of(raw),
                    name: raw.to_string(),
                    parent,
                })
            }
        }
    }

    fn create_body(
        &self,
        ctx: &RequestContext,
        res: &CrudResource,
        t: &CrudTarget,
        input: &OpInput,
    ) -> Result<Value, CloudError> {
        let (account, rg, region) = match &ctx.scope {
            Scope::Ibm {
                account,
                resource_group,
                region,
            } => (account.clone(), resource_group.clone(), region.clone()),
            _ => (
                DEFAULT_ACCOUNT.into(),
                DEFAULT_RESOURCE_GROUP.into(),
                DEFAULT_REGION.into(),
            ),
        };
        let svc = res.hint("crn_service").unwrap_or("mcle");
        let crn = format!(
            "crn:v1:bluemix:public:{svc}:{region}:a/{account}:{}:{}:",
            t.id,
            res.hint("crn_type").unwrap_or("")
        );
        let mut body: Map<String, Value> = input.body.as_object().cloned().unwrap_or_default();
        let prefix = res.hint("url_prefix").unwrap_or("/v2/resources");
        for (k, v) in [
            ("id", Value::String(crn.clone())),
            ("guid", Value::String(t.id.clone())),
            ("crn", Value::String(crn.clone())),
            ("url", Value::String(format!("{prefix}/{}", t.id))),
            ("name", Value::String(t.name.clone())),
            ("account_id", Value::String(account.clone())),
            ("resource_group_id", Value::String(rg.clone())),
            ("region_id", Value::String(region.clone())),
            ("state", Value::String("active".into())),
            (
                "type",
                Value::String(res.hint("type_value").unwrap_or("service_instance").into()),
            ),
            ("created_by", Value::String(ctx.principal.id.clone())),
            ("locked", Value::Bool(false)),
            (
                "last_operation",
                json!({"type": "create", "state": "succeeded", "async": false}),
            ),
        ] {
            body.insert(k.into(), v);
        }
        body.remove("resource_group");
        body.remove("target");
        Ok(Value::Object(body))
    }

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
                if !matches!(k.as_str(), "id" | "guid" | "crn" | "url") {
                    dst.insert(k.clone(), v.clone());
                }
            }
            dst.insert(
                "last_operation".into(),
                json!({"type": "update", "state": "succeeded", "async": false}),
            );
        }
        Ok(body)
    }

    fn render(
        &self,
        _ctx: &RequestContext,
        res: &CrudResource,
        verb: CrudVerb,
        result: CrudResult<'_>,
    ) -> Result<OpOutput, CloudError> {
        Ok(match result {
            CrudResult::One { record, created } => OpOutput::json_status(
                if created && verb == CrudVerb::Create {
                    201
                } else {
                    200
                },
                self.view(record),
            ),
            CrudResult::Many(recs) => {
                let items: Vec<Value> = recs.iter().map(|r| self.view(r)).collect();
                OpOutput::json(
                    json!({"rows_count": items.len(), "next_url": Value::Null, res.hint("list_field").unwrap_or("resources"): items}),
                )
            }
            CrudResult::Deleted(_) => OpOutput::empty(204),
        })
    }

    fn not_found(&self, res: &CrudResource, t: &CrudTarget) -> CloudError {
        CloudError::not_found(format!(
            "The {} with ID {} was not found.",
            res.type_name, t.name
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn crn_guid() {
        assert_eq!(
            guid_of("crn:v1:bluemix:public:svc:us-south:a/acc:1234::"),
            "1234"
        );
        assert_eq!(guid_of("1234"), "1234");
    }
}
