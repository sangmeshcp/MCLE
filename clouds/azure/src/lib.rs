#![forbid(unsafe_code)]

use bytes::Bytes;
use mcle_identity::{bearer, jwt_claims};
use mcle_sdk::error::ErrorKind;
use mcle_sdk::io::RequestHead;
use mcle_sdk::traits::DetectionRule;
use mcle_sdk::{
    Body, CloudAdapter, CloudError, CloudId, Host, LroOperation, LroRenderer, LroState, OpInput,
    OpOutput, Principal, RequestContext, Scope,
};
use serde_json::{json, Value};
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

pub const DEFAULT_TENANT: &str = "00000000-0000-0000-0000-000000000001";
pub const DEFAULT_SUBSCRIPTION: &str = "00000000-0000-0000-0000-000000000000";

pub struct AzureAdapter {
    _host: Host,
}

pub fn adapter(host: &Host) -> Arc<dyn CloudAdapter> {
    Arc::new(AzureAdapter {
        _host: host.clone(),
    })
}

static RULES: &[DetectionRule] = &[
    DetectionRule::HostContains("management.azure"),
    DetectionRule::HostContains(".azure."),
    DetectionRule::HostContains(".windows.net"),
    DetectionRule::Header("x-ms-client-request-id"),
    DetectionRule::PathPrefix("/subscriptions"),
    DetectionRule::PathPrefix("/providers/Microsoft."),
    DetectionRule::PathPrefix("/tenants"),
];

pub fn default_code(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::InvalidArgument => "InvalidRequestContent",
        ErrorKind::NotFound => "ResourceNotFound",
        ErrorKind::AlreadyExists | ErrorKind::Conflict => "Conflict",
        ErrorKind::Unauthenticated => "AuthenticationFailed",
        ErrorKind::AccessDenied => "AuthorizationFailed",
        ErrorKind::Throttled => "TooManyRequests",
        ErrorKind::NotImplemented => "NotImplemented",
        ErrorKind::Internal => "InternalServerError",
    }
}

pub fn path_scope(path: &str) -> (Option<String>, Option<String>) {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut sub = None;
    let mut rg = None;
    for w in segs.windows(2) {
        if w[0].eq_ignore_ascii_case("subscriptions") && sub.is_none() {
            sub = Some(w[1].to_string());
        }
        if w[0].eq_ignore_ascii_case("resourcegroups") && rg.is_none() {
            rg = Some(w[1].to_string());
        }
    }
    (sub, rg)
}

impl CloudAdapter for AzureAdapter {
    fn cloud(&self) -> CloudId {
        CloudId::AZURE
    }

    fn detection_rules(&self) -> &'static [DetectionRule] {
        RULES
    }

    fn parse_body(&self, _head: &RequestHead, raw: &Bytes) -> Result<Value, CloudError> {
        if raw.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(raw).map_err(|e| {
            CloudError::invalid(format!(
                "The request content was invalid and could not be deserialized: {e}"
            ))
            .with_code("InvalidRequestContent")
        })
    }

    fn principal(&self, head: &RequestHead) -> Principal {
        let claims = head
            .header("authorization")
            .and_then(bearer)
            .and_then(jwt_claims);
        match claims {
            Some(c) => {
                let id = ["oid", "appid", "azp", "sub"]
                    .iter()
                    .find_map(|k| c.get(*k).and_then(Value::as_str))
                    .unwrap_or("unknown")
                    .to_string();
                Principal {
                    cloud: Some(CloudId::AZURE),
                    id,
                    kind: if c.get("appid").is_some() {
                        "servicePrincipal".into()
                    } else {
                        "user".into()
                    },
                    access_key: None,
                    account: c.get("tid").and_then(Value::as_str).map(str::to_string),
                    region: None,
                    claims: c,
                }
            }
            None => Principal {
                cloud: Some(CloudId::AZURE),
                ..Principal::anonymous()
            },
        }
    }

    fn scope(&self, head: &RequestHead, p: &Principal, _input: &OpInput) -> Scope {
        let (sub, rg) = path_scope(&head.path);
        let parse = |s: Option<&str>, d: &str| {
            s.and_then(|s| Uuid::parse_str(s).ok())
                .unwrap_or_else(|| Uuid::parse_str(d).unwrap_or_default())
        };
        Scope::Azure {
            tenant: parse(p.account.as_deref(), DEFAULT_TENANT),
            subscription: parse(sub.as_deref(), DEFAULT_SUBSCRIPTION),
            resource_group: rg,
            location: None,
        }
    }

    fn request_id_header(&self) -> &'static str {
        "x-ms-request-id"
    }

    fn render_output(&self, ctx: &RequestContext, _protocol: &str, out: OpOutput) -> OpOutput {
        out.with_header("x-ms-correlation-request-id", ctx.request_id.clone())
    }

    fn render_error(&self, ctx: &RequestContext, _protocol: &str, err: &CloudError) -> OpOutput {
        let code = if err.code.is_empty() {
            default_code(err.kind)
        } else {
            err.code.as_str()
        };
        OpOutput {
            status: err.http_status,
            headers: vec![
                ("x-ms-error-code".into(), code.to_string()),
                ("x-ms-correlation-request-id".into(), ctx.request_id.clone()),
            ],
            body: Body::Json(json!({"error": {"code": code, "message": err.message}})),
        }
    }

    fn lro_renderer(&self) -> Option<Arc<dyn LroRenderer>> {
        Some(Arc::new(AzureLro))
    }

    fn env(&self, base_url: &str) -> Vec<(String, String)> {
        vec![
            ("AZURE_RESOURCE_MANAGER_ENDPOINT".into(), base_url.into()),
            ("AZURE_AUTHORITY_HOST".into(), base_url.into()),
            ("AZURE_SUBSCRIPTION_ID".into(), DEFAULT_SUBSCRIPTION.into()),
            ("AZURE_TENANT_ID".into(), DEFAULT_TENANT.into()),
            (
                "AZURE_CLIENT_ID".into(),
                "00000000-0000-0000-0000-0000000000c1".into(),
            ),
            ("AZURE_CLIENT_SECRET".into(), "mcle".into()),
        ]
    }
}

pub struct AzureLro;

pub fn status_word(s: LroState) -> &'static str {
    match s {
        LroState::Pending | LroState::Running => "InProgress",
        LroState::Succeeded => "Succeeded",
        LroState::Failed => "Failed",
        LroState::Cancelled => "Canceled",
    }
}

pub fn lro_headers(op: &LroOperation, base_url: &str) -> Vec<(String, String)> {
    vec![
        (
            "azure-asyncoperation".into(),
            format!(
                "{base_url}/_mcle/lro/{}?cloud=azure&sandbox={}",
                op.id, op.sandbox
            ),
        ),
        (
            "location".into(),
            format!(
                "{base_url}/_mcle/lro/{}?cloud=azure&kind=location&sandbox={}",
                op.id, op.sandbox
            ),
        ),
        ("retry-after".into(), "0".into()),
    ]
}

impl LroRenderer for AzureLro {
    fn render(&self, op: &LroOperation, _base_url: &str) -> OpOutput {
        let fmt = |t: time::OffsetDateTime| t.format(&Rfc3339).unwrap_or_default();
        let mut body = json!({
            "id": op.id,
            "name": op.id,
            "status": status_word(op.state),
            "startTime": fmt(op.created),
        });
        if op.state.is_terminal() {
            body["endTime"] = Value::String(fmt(op.ready_at));
        }
        if let Some(e) = &op.error {
            body["error"] = json!({"code": if e.code.is_empty() { default_code(e.kind) } else { &e.code }, "message": e.message});
        }
        OpOutput::json(body).with_header("retry-after", "0")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn scope_from_path() {
        let (s, rg) = path_scope("/subscriptions/abc/resourcegroups/RG1/providers/X/y/z");
        assert_eq!(s.as_deref(), Some("abc"));
        assert_eq!(rg.as_deref(), Some("RG1"));
    }

    #[test]
    fn principal_from_jwt() {
        let host = mcle_core::build_host(&Default::default()).unwrap();
        let a = AzureAdapter { _host: host };
        let tok = mcle_identity::issue_unsigned_jwt(&json!({"oid": "o-1", "tid": DEFAULT_TENANT}));
        let head = RequestHead {
            headers: vec![("authorization".into(), format!("Bearer {tok}"))],
            ..Default::default()
        };
        let p = a.principal(&head);
        assert_eq!(p.id, "o-1");
        assert_eq!(p.account.as_deref(), Some(DEFAULT_TENANT));
    }
}
