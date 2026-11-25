#![forbid(unsafe_code)]

use bytes::Bytes;
use mcle_identity::{bearer, jwt_claims};
use mcle_sdk::crud::CrudResult;
use mcle_sdk::error::ErrorKind;
use mcle_sdk::io::RequestHead;
use mcle_sdk::traits::DetectionRule;
use mcle_sdk::{
    Body, CloudAdapter, CloudError, CloudId, CrudRecord, CrudResource, CrudShaper, CrudTarget,
    CrudVerb, Host, LroOperation, LroRenderer, LroState, OpInput, OpOutput, Principal,
    RequestContext, Scope,
};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;

pub const DEFAULT_PROJECT: &str = "mcle-project";

pub struct GcpAdapter {
    host: Host,
}

pub fn adapter(host: &Host) -> Arc<dyn CloudAdapter> {
    Arc::new(GcpAdapter { host: host.clone() })
}

static RULES: &[DetectionRule] = &[
    DetectionRule::HostContains("googleapis"),
    DetectionRule::Header("x-goog-api-client"),
    DetectionRule::PathPrefix("/v1/projects/"),
    DetectionRule::PathPrefix("/v1beta1/projects/"),
    DetectionRule::PathPrefix("/v2/projects/"),
    DetectionRule::PathPrefix("/storage/v1/"),
    DetectionRule::PathPrefix("/v1/operations"),
];

pub fn canonical(kind: ErrorKind) -> (u16, &'static str) {
    match kind {
        ErrorKind::InvalidArgument => (400, "INVALID_ARGUMENT"),
        ErrorKind::NotFound => (404, "NOT_FOUND"),
        ErrorKind::AlreadyExists => (409, "ALREADY_EXISTS"),
        ErrorKind::Conflict => (409, "ABORTED"),
        ErrorKind::Unauthenticated => (401, "UNAUTHENTICATED"),
        ErrorKind::AccessDenied => (403, "PERMISSION_DENIED"),
        ErrorKind::Throttled => (429, "RESOURCE_EXHAUSTED"),
        ErrorKind::NotImplemented => (501, "UNIMPLEMENTED"),
        ErrorKind::Internal => (500, "INTERNAL"),
    }
}

pub fn ts(t: time::OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn project_from_path(path: &str) -> Option<String> {
    let mut it = path.split('/').filter(|s| !s.is_empty());
    while let Some(s) = it.next() {
        if s == "projects" {
            return it.next().map(str::to_string);
        }
    }
    None
}

impl CloudAdapter for GcpAdapter {
    fn cloud(&self) -> CloudId {
        CloudId::GCP
    }

    fn detection_rules(&self) -> &'static [DetectionRule] {
        RULES
    }

    fn parse_body(&self, _head: &RequestHead, raw: &Bytes) -> Result<Value, CloudError> {
        if raw.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(raw)
            .map_err(|e| CloudError::invalid(format!("Invalid JSON payload received. {e}")))
    }

    fn principal(&self, head: &RequestHead) -> Principal {
        let tok = head.header("authorization").and_then(bearer);
        let claims = tok.and_then(jwt_claims);
        let id = claims
            .as_ref()
            .and_then(|c| c.get("email").or(c.get("sub")).and_then(Value::as_str))
            .map(|e| format!("serviceAccount:{e}"))
            .unwrap_or_else(|| {
                if tok.is_some() {
                    format!("serviceAccount:default@{DEFAULT_PROJECT}.iam.gserviceaccount.com")
                } else {
                    "allUsers".into()
                }
            });
        Principal {
            cloud: Some(CloudId::GCP),
            kind: if tok.is_some() {
                "serviceAccount".into()
            } else {
                "anonymous".into()
            },
            id,
            claims: claims.unwrap_or(Value::Null),
            ..Default::default()
        }
    }

    fn scope(&self, head: &RequestHead, _p: &Principal, input: &OpInput) -> Scope {
        let project = project_from_path(&head.path)
            .or_else(|| input.query("project").map(str::to_string))
            .unwrap_or_else(|| DEFAULT_PROJECT.into());
        let location = {
            let segs: Vec<&str> = head.path.split('/').collect();
            segs.windows(2)
                .find(|w| w[0] == "locations")
                .map(|w| w[1].to_string())
                .unwrap_or_else(|| "global".into())
        };
        Scope::Gcp { project, location }
    }

    fn request_id_header(&self) -> &'static str {
        "x-mcle-request-id"
    }

    fn render_output(&self, _ctx: &RequestContext, _protocol: &str, out: OpOutput) -> OpOutput {
        if matches!(out.body, Body::Json(_)) {
            out.with_header("content-type", "application/json; charset=UTF-8")
        } else {
            out
        }
    }

    fn render_error(&self, _ctx: &RequestContext, _protocol: &str, err: &CloudError) -> OpOutput {
        let (_, status) = canonical(err.kind);
        let status = if err.code.is_empty() {
            status
        } else {
            err.code.as_str()
        };
        OpOutput::json_status(
            err.http_status,
            json!({"error": {"code": err.http_status, "message": err.message, "status": status}}),
        )
        .with_header("content-type", "application/json; charset=UTF-8")
    }

    fn lro_renderer(&self) -> Option<Arc<dyn LroRenderer>> {
        Some(Arc::new(GcpLro))
    }

    fn crud_shaper(&self, protocol: &str) -> Option<Arc<dyn CrudShaper>> {
        (protocol == "gcpRest").then(|| {
            Arc::new(AipShaper {
                host: self.host.clone(),
            }) as Arc<dyn CrudShaper>
        })
    }

    fn env(&self, base_url: &str) -> Vec<(String, String)> {
        let hostport = base_url.split("://").nth(1).unwrap_or(base_url).to_string();
        vec![
            ("GOOGLE_CLOUD_PROJECT".into(), DEFAULT_PROJECT.into()),
            ("CLOUDSDK_CORE_PROJECT".into(), DEFAULT_PROJECT.into()),
            ("STORAGE_EMULATOR_HOST".into(), base_url.into()),
            ("PUBSUB_EMULATOR_HOST".into(), hostport),
            (
                "CLOUDSDK_API_ENDPOINT_OVERRIDES_SECRETMANAGER".into(),
                format!("{base_url}/"),
            ),
        ]
    }
}

pub struct GcpLro;

pub fn operation_doc(op: &LroOperation) -> Value {
    let mut v = json!({
        "name": format!("operations/{}", op.id),
        "done": op.state.is_terminal(),
        "metadata": {"@type": "type.googleapis.com/google.protobuf.Empty", "createTime": ts(op.created)},
    });
    match (op.state, &op.error) {
        (LroState::Failed, Some(e)) | (LroState::Cancelled, Some(e)) => {
            let (_, s) = canonical(e.kind);
            v["error"] = json!({"code": e.http_status, "message": e.message, "status": s});
        }
        (LroState::Cancelled, None) => {
            v["error"] = json!({"code": 1, "message": "cancelled", "status": "CANCELLED"});
        }
        (LroState::Succeeded, _) => v["response"] = op.result.clone(),
        _ => {}
    }
    v
}

impl LroRenderer for GcpLro {
    fn render(&self, op: &LroOperation, _base_url: &str) -> OpOutput {
        OpOutput::json(operation_doc(op))
    }
}

pub struct AipShaper {
    host: Host,
}

fn fill(t: &str, input: &OpInput) -> Result<String, CloudError> {
    let mut out = String::new();
    let mut rest = t;
    while let Some(o) = rest.find('{') {
        out.push_str(&rest[..o]);
        let c = rest[o..]
            .find('}')
            .map(|c| c + o)
            .ok_or_else(|| CloudError::internal("bad template"))?;
        let k = &rest[o + 1..c];
        out.push_str(
            &input
                .lookup(k)
                .ok_or_else(|| CloudError::invalid(format!("missing `{k}`")))?,
        );
        rest = &rest[c + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

impl AipShaper {
    fn view(&self, rec: &CrudRecord, res: &CrudResource) -> Value {
        let mut b = rec.body.clone();
        if let Some(o) = b.as_object_mut() {
            o.insert("name".into(), Value::String(rec.id.clone()));
            if let Some(f) = res.hint("create_time") {
                o.insert(f.into(), Value::String(ts(rec.created)));
            }
            if res.hint("etag") == Some("true") {
                o.insert(
                    "etag".into(),
                    Value::String(format!("\"{}\"", rec.generation)),
                );
            }
        }
        b
    }
}

impl CrudShaper for AipShaper {
    fn target(
        &self,
        _ctx: &RequestContext,
        res: &CrudResource,
        verb: CrudVerb,
        input: &OpInput,
    ) -> Result<CrudTarget, CloudError> {
        let parent = fill(res.hint("parent_template").unwrap_or(""), input)?;
        let coll = res.hint("collection").unwrap_or(res.type_name);
        let id = match verb {
            CrudVerb::List => {
                return Ok(CrudTarget {
                    id: String::new(),
                    name: String::new(),
                    parent: format!("{parent}/{coll}/"),
                })
            }
            CrudVerb::Create => {
                let q = res.hint("id_query").unwrap_or("id");
                input
                    .query(q)
                    .map(str::to_string)
                    .ok_or_else(|| CloudError::invalid(format!("`{q}` is required")))?
            }
            _ => {
                let p = res.hint("id_param").unwrap_or("id");
                input
                    .param(p)
                    .map(str::to_string)
                    .ok_or_else(|| CloudError::invalid(format!("`{p}` is required")))?
            }
        };
        Ok(CrudTarget {
            id: format!("{parent}/{coll}/{id}"),
            name: id,
            parent: format!("{parent}/{coll}/"),
        })
    }

    fn create_body(
        &self,
        _ctx: &RequestContext,
        _res: &CrudResource,
        _t: &CrudTarget,
        input: &OpInput,
    ) -> Result<Value, CloudError> {
        Ok(match &input.body {
            Value::Object(o) => {
                let mut o = o.clone();
                o.remove("name");
                Value::Object(o)
            }
            _ => Value::Object(Map::new()),
        })
    }

    fn update_body(
        &self,
        _ctx: &RequestContext,
        _res: &CrudResource,
        existing: &CrudRecord,
        input: &OpInput,
    ) -> Result<Value, CloudError> {
        let mut body = existing.body.clone();
        let mask: Option<Vec<String>> = input
            .query("updateMask")
            .map(|m| m.split(',').map(|s| s.trim().to_string()).collect());
        if let (Some(dst), Some(src)) = (body.as_object_mut(), input.body.as_object()) {
            match mask {
                Some(fields) => {
                    for f in fields {
                        let camel = to_camel(&f);
                        match src.get(&camel).or_else(|| src.get(&f)) {
                            Some(v) => {
                                dst.insert(camel, v.clone());
                            }
                            None => {
                                dst.remove(&camel);
                            }
                        }
                    }
                }
                None => {
                    for (k, v) in src {
                        if k != "name" {
                            dst.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
        }
        Ok(body)
    }

    fn render(
        &self,
        _ctx: &RequestContext,
        res: &CrudResource,
        _verb: CrudVerb,
        result: CrudResult<'_>,
    ) -> Result<OpOutput, CloudError> {
        let _ = &self.host;
        Ok(match result {
            CrudResult::One { record, .. } => OpOutput::json(self.view(record, res)),
            CrudResult::Many(recs) => {
                let items: Vec<Value> = recs.iter().map(|r| self.view(r, res)).collect();
                let n = items.len();
                let mut doc = json!({ res.hint("list_field").unwrap_or("items"): items });
                if res.hint("total_size") == Some("true") {
                    doc["totalSize"] = json!(n);
                }
                OpOutput::json(doc)
            }
            CrudResult::Deleted(_) => OpOutput::json(json!({})),
        })
    }

    fn not_found(&self, res: &CrudResource, t: &CrudTarget) -> CloudError {
        CloudError::not_found(format!(
            "{} [{}] not found.",
            capitalize(res.type_name),
            t.id
        ))
    }

    fn already_exists(&self, res: &CrudResource, t: &CrudTarget) -> CloudError {
        CloudError::already_exists(format!(
            "{} [{}] already exists.",
            capitalize(res.type_name),
            t.id
        ))
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

fn to_camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for ch in s.chars() {
        if ch == '_' {
            up = true;
        } else if up {
            out.extend(ch.to_uppercase());
            up = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(
            project_from_path("/v1/projects/p1/secrets/x"),
            Some("p1".into())
        );
        assert_eq!(to_camel("expire_time"), "expireTime");
        assert_eq!(canonical(ErrorKind::NotFound), (404, "NOT_FOUND"));
    }
}
