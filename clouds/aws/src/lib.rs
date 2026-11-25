#![forbid(unsafe_code)]

mod shaper;
pub mod template;

use bytes::Bytes;
use mcle_identity::parse_sigv4;
use mcle_sdk::error::ErrorKind;
use mcle_sdk::io::RequestHead;
use mcle_sdk::traits::{DetectionRule, RouteRequest};
use mcle_sdk::util::parse_query;
use mcle_sdk::{
    Body, CloudAdapter, CloudError, CloudId, CrudShaper, Host, OpInput, OpOutput, Principal,
    RequestContext, Scope,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub use shaper::AwsJsonShaper;

pub struct AwsAdapter {
    host: Host,
}

pub fn adapter(host: &Host) -> Arc<dyn CloudAdapter> {
    Arc::new(AwsAdapter { host: host.clone() })
}

static RULES: &[DetectionRule] = &[
    DetectionRule::HostSuffix(".amazonaws.com"),
    DetectionRule::HostContains(".amazonaws."),
    DetectionRule::AuthPrefix("AWS4-HMAC-SHA256"),
    DetectionRule::Header("x-amz-target"),
];

pub fn default_code(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::InvalidArgument => "ValidationException",
        ErrorKind::NotFound => "ResourceNotFoundException",
        ErrorKind::AlreadyExists => "ResourceExistsException",
        ErrorKind::Conflict => "ConflictException",
        ErrorKind::Unauthenticated => "UnrecognizedClientException",
        ErrorKind::AccessDenied => "AccessDeniedException",
        ErrorKind::Throttled => "ThrottlingException",
        ErrorKind::NotImplemented => "NotImplementedException",
        ErrorKind::Internal => "InternalFailure",
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl CloudAdapter for AwsAdapter {
    fn cloud(&self) -> CloudId {
        CloudId::AWS
    }

    fn detection_rules(&self) -> &'static [DetectionRule] {
        RULES
    }

    fn is_fallback(&self) -> bool {
        true
    }

    fn parse_body(&self, head: &RequestHead, raw: &Bytes) -> Result<Value, CloudError> {
        if raw.is_empty() {
            return Ok(Value::Null);
        }
        let ct = head.header("content-type").unwrap_or("");
        if ct.starts_with("application/x-www-form-urlencoded") {
            let s = String::from_utf8_lossy(raw);
            return Ok(Value::Object(
                parse_query(&s)
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect(),
            ));
        }
        if ct.contains("json") || head.header("x-amz-target").is_some() {
            return serde_json::from_slice(raw).map_err(|e| {
                CloudError::invalid(format!("malformed JSON body: {e}"))
                    .with_code("SerializationException")
            });
        }
        Ok(Value::Null)
    }

    fn route_request<'a>(&self, head: &'a RequestHead, body: &'a Value) -> RouteRequest<'a> {
        RouteRequest {
            method: &head.method,
            path: &head.path,
            query_keys: Vec::new(),
            target: head.header("x-amz-target"),
            action: body.get("Action").and_then(Value::as_str),
        }
    }

    fn principal(&self, head: &RequestHead) -> Principal {
        let account = self.host.config().aws_account;
        match head.header("authorization").and_then(parse_sigv4) {
            Some(s) => Principal {
                cloud: Some(CloudId::AWS),
                id: format!("arn:aws:iam::{account}:root"),
                kind: "user".into(),
                access_key: Some(s.access_key),
                account: Some(account),
                region: Some(s.region),
                claims: json!({"service": s.service, "date": s.date}),
            },
            None => Principal {
                cloud: Some(CloudId::AWS),
                account: Some(account),
                ..Principal::anonymous()
            },
        }
    }

    fn scope(&self, _head: &RequestHead, p: &Principal, _input: &OpInput) -> Scope {
        let cfg = self.host.config();
        Scope::Aws {
            account: p.account.clone().unwrap_or(cfg.aws_account),
            region: p.region.clone().unwrap_or(cfg.default_region),
        }
    }

    fn request_id_header(&self) -> &'static str {
        "x-amzn-requestid"
    }

    fn render_output(&self, _ctx: &RequestContext, protocol: &str, mut out: OpOutput) -> OpOutput {
        if let Some(ct) = json_content_type(protocol) {
            if !out
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                out.headers.push(("content-type".into(), ct.into()));
            }
        }
        out
    }

    fn render_error(&self, ctx: &RequestContext, protocol: &str, err: &CloudError) -> OpOutput {
        let code = if err.code.is_empty() {
            default_code(err.kind)
        } else {
            err.code.as_str()
        };
        match protocol {
            "awsQuery" | "restXml" | "ec2Query" => {
                let ty = if err.http_status < 500 {
                    "Sender"
                } else {
                    "Receiver"
                };
                let xml = if protocol == "restXml" {
                    format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>{}</Code><Message>{}</Message><RequestId>{}</RequestId></Error>",
                        xml_escape(code), xml_escape(&err.message), ctx.request_id
                    )
                } else {
                    format!(
                        "<ErrorResponse><Error><Type>{ty}</Type><Code>{}</Code><Message>{}</Message></Error><RequestId>{}</RequestId></ErrorResponse>",
                        xml_escape(code), xml_escape(&err.message), ctx.request_id
                    )
                };
                OpOutput {
                    status: err.http_status,
                    headers: vec![],
                    body: Body::Text {
                        content_type: "text/xml".into(),
                        data: xml,
                    },
                }
            }
            _ => OpOutput {
                status: err.http_status,
                headers: vec![
                    ("x-amzn-errortype".into(), code.to_string()),
                    (
                        "content-type".into(),
                        json_content_type(protocol)
                            .unwrap_or("application/x-amz-json-1.1")
                            .into(),
                    ),
                ],
                body: Body::Json(
                    json!({"__type": code, "message": err.message, "Message": err.message}),
                ),
            },
        }
    }

    fn crud_shaper(&self, protocol: &str) -> Option<Arc<dyn CrudShaper>> {
        match protocol {
            "awsJson1_0" | "awsJson1_1" => Some(Arc::new(AwsJsonShaper::new(&self.host))),
            _ => None,
        }
    }

    fn env(&self, base_url: &str) -> Vec<(String, String)> {
        let cfg = self.host.config();
        vec![
            ("AWS_ENDPOINT_URL".into(), base_url.into()),
            ("AWS_ACCESS_KEY_ID".into(), "test".into()),
            ("AWS_SECRET_ACCESS_KEY".into(), "test".into()),
            ("AWS_DEFAULT_REGION".into(), cfg.default_region.clone()),
            ("AWS_REGION".into(), cfg.default_region),
        ]
    }
}

fn json_content_type(protocol: &str) -> Option<&'static str> {
    match protocol {
        "awsJson1_0" => Some("application/x-amz-json-1.0"),
        "awsJson1_1" => Some("application/x-amz-json-1.1"),
        "restJson1" => Some("application/json"),
        _ => None,
    }
}
