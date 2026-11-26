use crate::server::Gateway;
use bytes::Bytes;
use http::request::Parts;
use http::{HeaderValue, Response, StatusCode};
use http_body_util::Full;
use mcle_core::coverage::CoverageEntry;
use mcle_core::faults::FaultKey;
use mcle_core::sandbox::ACCESS_KEY_PREFIX;
use mcle_sdk::io::RequestHead;
use mcle_sdk::util::parse_query;
use mcle_sdk::{
    AuthzRequest, Body, CloudAdapter, CloudError, FidelityLevel, OpInput, OpMeta, OpOutput, Plane,
    RequestContext, ResourceExtractor, SandboxId, Scope, ServiceFacade,
};
use std::sync::Arc;

pub(crate) fn head_from_parts(parts: &Parts) -> RequestHead {
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_ascii_lowercase(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let host = parts
        .uri
        .host()
        .map(str::to_string)
        .or_else(|| {
            headers
                .iter()
                .find(|(k, _)| k == "host")
                .map(|(_, v)| v.clone())
        })
        .unwrap_or_default();
    let host = strip_port(&host);
    RequestHead {
        method: parts.method.as_str().to_ascii_uppercase(),
        path: parts.uri.path().to_string(),
        query: parts.uri.query().unwrap_or("").to_string(),
        host,
        headers,
    }
}

fn strip_port(host: &str) -> String {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("").to_string();
    }
    match host.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => h.to_string(),
        _ => host.to_string(),
    }
}

pub(crate) fn to_response(
    out: OpOutput,
    request_id_header: &str,
    request_id: &str,
) -> Response<Full<Bytes>> {
    let mut b = Response::builder()
        .status(StatusCode::from_u16(out.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR));
    let has_ct = out
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
    for (k, v) in &out.headers {
        if let Ok(v) = HeaderValue::from_str(v) {
            b = b.header(k.as_str(), v);
        }
    }
    if !request_id_header.is_empty()
        && !out
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case(request_id_header))
    {
        b = b.header(request_id_header, request_id);
    }
    let body = match out.body {
        Body::Empty => Bytes::new(),
        Body::Json(v) => {
            if !has_ct {
                b = b.header("content-type", "application/json");
            }
            Bytes::from(v.to_string())
        }
        Body::Text { content_type, data } => {
            if !has_ct {
                b = b.header("content-type", content_type);
            }
            Bytes::from(data)
        }
        Body::Bytes { content_type, data } => {
            if !has_ct {
                b = b.header("content-type", content_type);
            }
            data
        }
    };
    b.body(Full::new(body))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

fn resource_of(meta: &OpMeta, input: &OpInput, scope: &Scope) -> Option<String> {
    match meta.resource {
        ResourceExtractor::Scope => Some(scope.to_string()),
        ResourceExtractor::PathParam(p) => input.param(p).map(str::to_string),
        ResourceExtractor::QueryParam(p) => input.query(p).map(str::to_string),
        ResourceExtractor::BodyField(p) => input.body_str(p).map(str::to_string),
        ResourceExtractor::Template(t) => {
            let mut out = String::new();
            let mut rest = t;
            while let Some(open) = rest.find('{') {
                out.push_str(&rest[..open]);
                let close = rest[open..].find('}')? + open;
                let key = &rest[open + 1..close];
                let val = match (key, scope) {
                    ("region", Scope::Aws { region, .. }) => Some(region.clone()),
                    ("account", Scope::Aws { account, .. }) => Some(account.clone()),
                    ("project", Scope::Gcp { project, .. }) => Some(project.clone()),
                    ("subscription", Scope::Azure { subscription, .. }) => {
                        Some(subscription.to_string())
                    }
                    _ => input.lookup(key),
                };
                out.push_str(&val.unwrap_or_else(|| "*".into()));
                rest = &rest[close + 1..];
            }
            out.push_str(rest);
            Some(out)
        }
    }
}

fn resolve_sandbox(gw: &Gateway, head: &RequestHead, access_key: Option<&str>) -> SandboxId {
    if let Some(h) = head.header("x-mcle-sandbox") {
        return SandboxId(h.to_string());
    }
    if let Some(k) = access_key.filter(|k| k.starts_with(ACCESS_KEY_PREFIX)) {
        if let Some(sb) = gw.sandboxes.by_access_key(k) {
            return sb;
        }
    }
    SandboxId::default()
}

pub(crate) async fn handle(
    gw: &Arc<Gateway>,
    head: RequestHead,
    raw: Bytes,
) -> Response<Full<Bytes>> {
    let request_id = gw.host.ids.uuid().to_string();
    let Some(adapter) = crate::detect::detect(&gw.plugins.adapters, &head) else {
        return crate::internal::plain(404, "MCLE: no enabled cloud adapter matches this request");
    };
    let principal = adapter.principal(&head);
    let sandbox = resolve_sandbox(gw, &head, principal.access_key.as_deref());
    let base_ctx = |service: &str, op: &str, scope: Scope| RequestContext {
        request_id: request_id.clone(),
        sandbox: sandbox.clone(),
        cloud: adapter.cloud(),
        service: service.to_string(),
        op: op.to_string(),
        principal: principal.clone(),
        scope,
        public_base_url: gw.host.config().public_base_url,
    };
    let body = match adapter.parse_body(&head, &raw) {
        Ok(b) => b,
        Err(e) => {
            let ctx = base_ctx("", "", fallback_scope(&*adapter));
            return to_response(
                adapter.render_error(&ctx, "", &e),
                adapter.request_id_header(),
                &request_id,
            );
        }
    };
    let query = parse_query(&head.query);
    let (best, route_label) = {
        let mut rreq = adapter.route_request(&head, &body);
        rreq.query_keys
            .extend(query.iter().map(|(k, _)| k.as_str()));
        let mut best: Option<(Arc<dyn ServiceFacade>, mcle_sdk::route::RouteHit)> = None;
        for svc in gw
            .plugins
            .services
            .iter()
            .filter(|s| s.manifest().cloud == adapter.cloud())
        {
            if let Some(hit) = svc.routes().find(&rreq) {
                if best.as_ref().is_none_or(|(_, b)| hit.score > b.score) {
                    best = Some((svc.clone(), hit));
                }
            }
        }
        (best, rreq.target.or(rreq.action).map(str::to_string))
    };
    let input_base = OpInput {
        method: head.method.clone(),
        path: head.path.clone(),
        query,
        path_params: Vec::new(),
        headers: head.headers.clone(),
        body,
        raw,
    };
    let Some((svc, hit)) = best else {
        let scope = adapter.scope(&head, &principal, &input_base);
        let op_name = route_label.clone().unwrap_or_else(|| "<unknown>".into());
        let ctx = base_ctx("<unknown>", &op_name, scope);
        let err = CloudError::new(
            mcle_sdk::error::ErrorKind::NotImplemented,
            format!(
                "MCLE: no service in this build handles {} {} (target={:?}); fidelity L0",
                head.method, head.path, route_label
            ),
        );
        gw.coverage.record(CoverageEntry {
            request_id: request_id.clone(),
            sandbox: sandbox.0.clone(),
            cloud: adapter.cloud().to_string(),
            service: "<unknown>".into(),
            op: op_name,
            level: FidelityLevel::L0,
            plane: Plane::Data,
            outcome: "NotImplemented".into(),
            status: err.http_status,
            iam: "n/a".into(),
            fault: None,
        });
        return to_response(
            adapter.render_error(&ctx, "", &err),
            adapter.request_id_header(),
            &request_id,
        );
    };
    let meta = svc.op_meta(hit.op);
    let manifest = svc.manifest();
    let protocol = manifest.protocol;
    let input = OpInput {
        path_params: hit.params,
        ..input_base
    };
    let scope = adapter.scope(&head, &principal, &input);
    let ctx = base_ctx(manifest.name, meta.name, scope.clone());
    let resource = resource_of(meta, &input, &scope);
    tracing::debug!(cloud = %ctx.cloud, service = manifest.name, op = meta.name, sandbox = %sandbox, "dispatch");

    let skip_iam = meta.iam_action.0.is_empty() || meta.iam_action.0.starts_with("none:");
    let outcome = if skip_iam {
        mcle_iam::Outcome {
            decision: mcle_sdk::Decision::Allow { matched: vec![] },
            blocked: false,
            label: "n/a".into(),
        }
    } else {
        gw.iam.authorize(
            &request_id,
            AuthzRequest {
                cloud: adapter.cloud(),
                principal: principal.clone(),
                action: meta.iam_action.0.to_string(),
                resource: resource.clone(),
                scope,
                context: Default::default(),
            },
        )
    };
    let mut fault_label = None;
    let result: Result<OpOutput, CloudError> = if outcome.blocked {
        Err(CloudError::access_denied(format!(
            "User: {} is not authorized to perform: {} on resource: {}",
            principal.id,
            meta.iam_action.0,
            resource.as_deref().unwrap_or("*")
        )))
    } else {
        let fault = gw.faults.check(FaultKey {
            cloud: adapter.cloud().as_str(),
            service: manifest.name,
            op: meta.name,
            resource: resource.as_deref().unwrap_or(""),
            principal: &principal.id,
        });
        let mut injected = None;
        if let Some(f) = fault {
            fault_label = Some(f.label.clone());
            if f.latency_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(f.latency_ms)).await;
            }
            injected = f.error;
        }
        match injected {
            Some(e) => Err(e),
            None => run_layers_and_dispatch(gw, &ctx, meta, &*svc, hit.op, input).await,
        }
    };
    let (status, outcome_label) = match &result {
        Ok(o) => (o.status, "ok".to_string()),
        Err(e) => (
            e.http_status,
            if e.code.is_empty() {
                format!("{:?}", e.kind)
            } else {
                e.code.clone()
            },
        ),
    };
    gw.coverage.record(CoverageEntry {
        request_id: request_id.clone(),
        sandbox: sandbox.0.clone(),
        cloud: adapter.cloud().to_string(),
        service: manifest.name.to_string(),
        op: meta.name.to_string(),
        level: meta.level,
        plane: meta.plane,
        outcome: outcome_label,
        status,
        iam: outcome.label,
        fault: fault_label,
    });
    let out = match result {
        Ok(o) => adapter.render_output(&ctx, protocol, o),
        Err(e) => adapter.render_error(&ctx, protocol, &e),
    };
    to_response(out, adapter.request_id_header(), &request_id)
}

async fn run_layers_and_dispatch(
    gw: &Gateway,
    ctx: &RequestContext,
    meta: &'static OpMeta,
    svc: &dyn ServiceFacade,
    op: mcle_sdk::OpId,
    input: OpInput,
) -> Result<OpOutput, CloudError> {
    for l in &gw.plugins.layers {
        l.before(ctx, meta, &input).await?;
    }
    let r = svc.dispatch(ctx, op, input).await;
    for l in &gw.plugins.layers {
        l.after(ctx, meta, &r).await;
    }
    r
}

pub(crate) fn fallback_scope(adapter: &dyn CloudAdapter) -> Scope {
    Scope::Ext {
        cloud: adapter.cloud(),
        parts: vec![],
    }
}
