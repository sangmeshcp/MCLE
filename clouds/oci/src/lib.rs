#![forbid(unsafe_code)]

use bytes::Bytes;
use mcle_identity::parse_oci_signature;
use mcle_sdk::crud::CrudResult;
use mcle_sdk::error::ErrorKind;
use mcle_sdk::io::RequestHead;
use mcle_sdk::traits::DetectionRule;
use mcle_sdk::{
    CloudAdapter, CloudError, CloudId, CrudRecord, CrudResource, CrudShaper, CrudTarget, CrudVerb,
    Host, LroOperation, LroRenderer, LroState, OpInput, OpOutput, Principal, RequestContext, Scope,
};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;

pub const DEFAULT_TENANCY: &str = "ocid1.tenancy.oc1..mcle";
pub const DEFAULT_REGION: &str = "us-ashburn-1";
pub const DEFAULT_NAMESPACE: &str = "mcle";

pub struct OciAdapter {
    host: Host,
}

pub fn adapter(host: &Host) -> Arc<dyn CloudAdapter> {
    Arc::new(OciAdapter { host: host.clone() })
}

static RULES: &[DetectionRule] = &[
    DetectionRule::HostContains(".oci."),
    DetectionRule::HostContains("oraclecloud"),
    DetectionRule::AuthPrefix("Signature "),
    DetectionRule::Header("opc-client-info"),
    DetectionRule::PathPrefix("/n/"),
    DetectionRule::PathPrefix("/20160918/"),
];

pub fn default_code(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::InvalidArgument => "InvalidParameter",
        ErrorKind::NotFound => "NotAuthorizedOrNotFound",
        ErrorKind::AlreadyExists | ErrorKind::Conflict => "Conflict",
        ErrorKind::Unauthenticated => "NotAuthenticated",
        ErrorKind::AccessDenied => "NotAuthorizedOrNotFound",
        ErrorKind::Throttled => "TooManyRequests",
        ErrorKind::NotImplemented => "NotImplemented",
        ErrorKind::Internal => "InternalServerError",
    }
}

pub fn ts(t: time::OffsetDateTime) -> String {
    let t = t
        .replace_nanosecond(t.millisecond() as u32 * 1_000_000)
        .unwrap_or(t);
    t.format(&Rfc3339).unwrap_or_default()
}

impl CloudAdapter for OciAdapter {
    fn cloud(&self) -> CloudId {
        CloudId::OCI
    }

    fn detection_rules(&self) -> &'static [DetectionRule] {
        RULES
    }

    fn parse_body(&self, _head: &RequestHead, raw: &Bytes) -> Result<Value, CloudError> {
        if raw.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(raw).map_err(|e| {
            CloudError::invalid(format!("Malformed JSON: {e}")).with_code("InvalidParameter")
        })
    }

    fn principal(&self, head: &RequestHead) -> Principal {
        match head.header("authorization").and_then(parse_oci_signature) {
            Some(k) => Principal {
                cloud: Some(CloudId::OCI),
                id: if k.user.is_empty() {
                    k.tenancy.clone()
                } else {
                    k.user.clone()
                },
                kind: "user".into(),
                access_key: Some(k.fingerprint),
                account: Some(k.tenancy),
                region: None,
                claims: Value::Null,
            },
            None => Principal {
                cloud: Some(CloudId::OCI),
                ..Principal::anonymous()
            },
        }
    }

    fn scope(&self, head: &RequestHead, p: &Principal, input: &OpInput) -> Scope {
        let region = head
            .host
            .split('.')
            .find(|s| s.contains('-') && s.chars().last().is_some_and(|c| c.is_ascii_digit()))
            .unwrap_or(DEFAULT_REGION)
            .to_string();
        let tenancy = p.account.clone().unwrap_or_else(|| DEFAULT_TENANCY.into());
        let compartment = input
            .query("compartmentId")
            .map(str::to_string)
            .or_else(|| input.body_str("compartmentId").map(str::to_string))
            .unwrap_or_else(|| tenancy.clone());
        Scope::Oci {
            tenancy,
            compartment,
            region,
        }
    }

    fn request_id_header(&self) -> &'static str {
        "opc-request-id"
    }

    fn render_error(&self, _ctx: &RequestContext, _protocol: &str, err: &CloudError) -> OpOutput {
        let code = if err.code.is_empty() {
            default_code(err.kind)
        } else {
            err.code.as_str()
        };
        OpOutput::json_status(
            err.http_status,
            json!({"code": code, "message": err.message}),
        )
    }

    fn lro_renderer(&self) -> Option<Arc<dyn LroRenderer>> {
        Some(Arc::new(OciLro))
    }

    fn crud_shaper(&self, protocol: &str) -> Option<Arc<dyn CrudShaper>> {
        (protocol == "ociRest").then(|| {
            Arc::new(OciShaper {
                host: self.host.clone(),
            }) as Arc<dyn CrudShaper>
        })
    }

    fn env(&self, base_url: &str) -> Vec<(String, String)> {
        vec![
            ("OCI_SERVICE_ENDPOINT".into(), base_url.into()),
            ("OCI_TENANCY".into(), DEFAULT_TENANCY.into()),
            ("OCI_REGION".into(), DEFAULT_REGION.into()),
            ("OCI_NAMESPACE".into(), DEFAULT_NAMESPACE.into()),
        ]
    }
}

pub struct OciLro;

impl LroRenderer for OciLro {
    fn render(&self, op: &LroOperation, _base: &str) -> OpOutput {
        let status = match op.state {
            LroState::Pending => "ACCEPTED",
            LroState::Running => "IN_PROGRESS",
            LroState::Succeeded => "SUCCEEDED",
            LroState::Failed => "FAILED",
            LroState::Cancelled => "CANCELED",
        };
        OpOutput::json(json!({
            "id": op.id,
            "status": status,
            "percentComplete": if op.state.is_terminal() { 100.0 } else { 0.0 },
            "timeAccepted": ts(op.created),
            "resources": op.resource.iter().map(|r| json!({"identifier": r})).collect::<Vec<_>>(),
        }))
        .with_header("opc-work-request-id", op.id.clone())
    }
}

pub struct OciShaper {
    host: Host,
}

impl OciShaper {
    fn view(&self, rec: &CrudRecord, res: &CrudResource) -> Value {
        let mut b = rec.body.clone();
        if let (Some(o), Some(f)) = (b.as_object_mut(), res.hint("time_field")) {
            o.insert(f.into(), Value::String(ts(rec.created)));
        }
        if let Some(o) = b.as_object_mut() {
            o.insert("etag".into(), Value::String(etag(rec)));
        }
        b
    }
}

fn etag(rec: &CrudRecord) -> String {
    format!("{:x}-{}", rec.created.unix_timestamp(), rec.generation)
}

impl CrudShaper for OciShaper {
    fn target(
        &self,
        _ctx: &RequestContext,
        res: &CrudResource,
        verb: CrudVerb,
        input: &OpInput,
    ) -> Result<CrudTarget, CloudError> {
        let parent = res
            .hint("parent_param")
            .and_then(|p| input.param(p))
            .unwrap_or("")
            .to_string();
        let compartment = input
            .query("compartmentId")
            .or_else(|| input.body_str("compartmentId"))
            .unwrap_or("");
        let name = match verb {
            CrudVerb::List => {
                if compartment.is_empty() {
                    return Err(CloudError::invalid("compartmentId is required")
                        .with_code("MissingParameter"));
                }
                return Ok(CrudTarget {
                    id: String::new(),
                    name: String::new(),
                    parent: format!("{parent}/{compartment}/"),
                });
            }
            CrudVerb::Create => input
                .body_str(res.hint("name_field").unwrap_or("name"))
                .ok_or_else(|| {
                    CloudError::invalid("name is required").with_code("MissingParameter")
                })?
                .to_string(),
            _ => input
                .param(res.hint("name_param").unwrap_or("name"))
                .ok_or_else(|| CloudError::invalid("name is required"))?
                .to_string(),
        };
        Ok(CrudTarget {
            id: format!("{parent}/{name}"),
            name,
            parent: format!("{parent}/{compartment}/"),
        })
    }

    fn create_body(
        &self,
        ctx: &RequestContext,
        res: &CrudResource,
        t: &CrudTarget,
        input: &OpInput,
    ) -> Result<Value, CloudError> {
        let mut body: Map<String, Value> = res
            .hint("defaults")
            .and_then(|d| serde_json::from_str::<Value>(d).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        if let Some(o) = input.body.as_object() {
            for (k, v) in o {
                body.insert(k.clone(), v.clone());
            }
        }
        if let Some(f) = res.hint("parent_field") {
            let p = t.id.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            body.insert(f.into(), Value::String(p.into()));
        }
        if let Some(f) = res.hint("created_by_field") {
            body.insert(f.into(), Value::String(ctx.principal.id.clone()));
        }
        if let Some(f) = res.hint("lifecycle_field") {
            body.insert(f.into(), Value::String("ACTIVE".into()));
        }
        let region = match &ctx.scope {
            Scope::Oci { region, .. } => region.clone(),
            _ => DEFAULT_REGION.into(),
        };
        let ty = res.hint("ocid_type").unwrap_or(res.type_name);
        body.insert(
            "id".into(),
            Value::String(format!("ocid1.{ty}.oc1.{region}.{}", self.host.ids.hex(60))),
        );
        for k in ["freeformTags", "definedTags"] {
            body.entry(k).or_insert_with(|| json!({}));
        }
        Ok(Value::Object(body))
    }

    fn render(
        &self,
        _ctx: &RequestContext,
        res: &CrudResource,
        _verb: CrudVerb,
        result: CrudResult<'_>,
    ) -> Result<OpOutput, CloudError> {
        Ok(match result {
            CrudResult::One { record, .. } => {
                OpOutput::json(self.view(record, res)).with_header("etag", etag(record))
            }
            CrudResult::Many(recs) => OpOutput::json(Value::Array(
                recs.iter().map(|r| self.view(r, res)).collect(),
            )),
            CrudResult::Deleted(_) => OpOutput::empty(204),
        })
    }

    fn not_found(&self, res: &CrudResource, t: &CrudTarget) -> CloudError {
        let code = res
            .hint("not_found_code")
            .unwrap_or("NotAuthorizedOrNotFound");
        CloudError::not_found(format!(
            "Either the {} named '{}' does not exist or you are not authorized to access it",
            res.type_name, t.name
        ))
        .with_code(code)
    }

    fn already_exists(&self, res: &CrudResource, t: &CrudTarget) -> CloudError {
        let code = res.hint("exists_code").unwrap_or("Conflict");
        CloudError::already_exists(format!(
            "Either the {} '{}' already exists or you are not authorized to create it",
            res.type_name, t.name
        ))
        .with_code(code)
    }
}
