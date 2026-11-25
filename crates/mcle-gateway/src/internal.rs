use crate::pipeline::to_response;
use crate::server::Gateway;
use bytes::Bytes;
use http::Response;
use http_body_util::Full;
use mcle_core::coverage::{render, Strict};
use mcle_core::faults::FaultRule;
use mcle_sdk::io::RequestHead;
use mcle_sdk::util::parse_query;
use mcle_sdk::{Body, CloudId, IamMode, OpOutput, SandboxId};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub(crate) fn plain(status: u16, msg: &str) -> Response<Full<Bytes>> {
    to_response(
        OpOutput {
            status,
            headers: vec![],
            body: Body::Text {
                content_type: "text/plain; charset=utf-8".into(),
                data: format!("{msg}\n"),
            },
        },
        "",
        "",
    )
}

fn json_resp(status: u16, v: Value) -> Response<Full<Bytes>> {
    to_response(OpOutput::json_status(status, v), "", "")
}

pub fn info(gw: &Gateway) -> Value {
    let services: Vec<Value> = gw
        .plugins
        .services
        .iter()
        .map(|s| {
            let m = s.manifest();
            let mut levels = std::collections::BTreeMap::<String, u32>::new();
            for op in s.ops() {
                *levels.entry(op.level.to_string()).or_default() += 1;
            }
            json!({
                "cloud": m.cloud.as_str(),
                "service": m.name,
                "display": m.display,
                "tier": format!("{:?}", m.tier),
                "ops": s.ops().len(),
                "levels": levels,
            })
        })
        .collect();
    let ports = gw.ports.read().map(|p| p.clone()).unwrap_or_default();
    json!({
        "name": "mcle",
        "version": env!("CARGO_PKG_VERSION"),
        "sdk_version": mcle_sdk::SDK_VERSION,
        "public_base_url": gw.host.config().public_base_url,
        "ports": ports,
        "clouds": gw.plugins.adapters.iter().map(|a| a.cloud().to_string()).collect::<Vec<_>>(),
        "services": services,
        "iam": gw.iam.mode(),
        "seed": gw.config.seed,
        "lro_delay_ms": gw.host.lro.delay_ms(),
    })
}

pub fn env(gw: &Gateway, cloud: Option<&str>) -> Vec<(String, String)> {
    let base = gw.host.config().public_base_url;
    let mut out = vec![("MCLE_ENDPOINT".to_string(), base.clone())];
    for a in &gw.plugins.adapters {
        if cloud.is_none_or(|c| c == "all" || a.cloud().as_str() == c) {
            out.extend(a.env(&base));
        }
    }
    out
}

pub(crate) async fn handle(
    gw: &Arc<Gateway>,
    head: RequestHead,
    raw: Bytes,
) -> Response<Full<Bytes>> {
    let q = parse_query(&head.query);
    let qv = |k: &str| q.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.as_str());
    let sandbox = head
        .header("x-mcle-sandbox")
        .or(qv("sandbox"))
        .map(|s| SandboxId(s.to_string()));
    let body: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let path = head.path.trim_end_matches('/');
    let segs: Vec<&str> = path
        .trim_start_matches("/_mcle")
        .trim_start_matches('/')
        .split('/')
        .collect();
    let m = head.method.as_str();
    match (m, segs.as_slice()) {
        ("GET", ["health"]) => json_resp(200, json!({"status": "ok"})),
        ("GET", ["ready"]) => {
            if gw.ready.load(Ordering::SeqCst) {
                json_resp(200, json!({"ready": true}))
            } else {
                json_resp(503, json!({"ready": false}))
            }
        }
        ("GET", ["info"]) => json_resp(200, info(gw)),
        ("GET", ["env"]) => {
            let fmt = qv("format").unwrap_or("json");
            let vars = env(gw, qv("cloud"));
            if fmt == "sh" {
                let s: String = vars
                    .iter()
                    .map(|(k, v)| format!("export {k}={v}\n"))
                    .collect();
                plain(200, s.trim_end())
            } else {
                json_resp(
                    200,
                    Value::Object(
                        vars.into_iter()
                            .map(|(k, v)| (k, Value::String(v)))
                            .collect(),
                    ),
                )
            }
        }
        ("POST", ["reset"]) => {
            gw.host.state.reset(sandbox.as_ref());
            gw.coverage.reset(sandbox.as_ref().map(|s| s.0.as_str()));
            json_resp(
                200,
                json!({"reset": sandbox.map(|s| s.0).unwrap_or_else(|| "all".into())}),
            )
        }
        ("GET", ["state", "export"]) => {
            match serde_json::to_value(mcle_persist::export(&*gw.host.state)) {
                Ok(v) => json_resp(200, v),
                Err(e) => plain(500, &e.to_string()),
            }
        }
        ("POST", ["state", "import"]) => match mcle_persist::parse(body) {
            Ok(snap) => {
                let n = snap.partitions.len();
                mcle_persist::import(&*gw.host.state, snap);
                json_resp(200, json!({"imported_partitions": n}))
            }
            Err(e) => plain(400, &e.to_string()),
        },
        ("GET", ["lro", id]) => {
            let sb = sandbox.unwrap_or_default();
            let Some(op) = gw.host.lro.get(&sb, id) else {
                return plain(404, "no such operation");
            };
            let cloud = qv("cloud")
                .map(CloudId::from_name)
                .unwrap_or(op.cloud.clone());
            let renderer = gw
                .plugins
                .adapters
                .iter()
                .find(|a| a.cloud() == cloud)
                .and_then(|a| a.lro_renderer());
            match renderer {
                Some(r) => to_response(r.render(&op, &gw.host.config().public_base_url), "", ""),
                None => json_resp(200, serde_json::to_value(&op).unwrap_or(Value::Null)),
            }
        }
        ("GET", ["lro"]) => {
            let sb = sandbox.unwrap_or_default();
            json_resp(
                200,
                serde_json::to_value(gw.host.lro.list(&sb)).unwrap_or(Value::Null),
            )
        }
        ("POST", ["sandboxes"]) => {
            let ttl = body.get("ttl").and_then(Value::as_u64);
            let id = body
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| gw.host.ids.hex(12));
            let sb = gw.sandboxes.create(id, gw.host.clock.now(), ttl);
            let mut env: serde_json::Map<String, Value> = env(gw, None)
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect();
            env.insert(
                "AWS_ACCESS_KEY_ID".into(),
                Value::String(sb.access_key.clone()),
            );
            env.insert("MCLE_SANDBOX".into(), Value::String(sb.id.0.clone()));
            json_resp(
                201,
                json!({"id": sb.id, "access_key": sb.access_key, "expires": sb.expires.map(|e| e.unix_timestamp()), "env": env}),
            )
        }
        ("GET", ["sandboxes"]) => json_resp(
            200,
            serde_json::to_value(gw.sandboxes.list()).unwrap_or(Value::Null),
        ),
        ("DELETE", ["sandboxes", id]) => match gw.sandboxes.remove(id) {
            Some(sb) => {
                gw.host.state.reset(Some(&sb.id));
                gw.coverage.reset(Some(&sb.id.0));
                json_resp(200, json!({"deleted": sb.id}))
            }
            None => plain(404, "no such sandbox"),
        },
        ("GET", ["faults"]) => json_resp(
            200,
            serde_json::to_value(gw.faults.list()).unwrap_or(Value::Null),
        ),
        ("POST", ["faults"]) | ("PUT", ["faults"]) => {
            let rules: Result<Vec<FaultRule>, _> = match &body {
                Value::Array(_) => serde_json::from_value(body.clone()),
                Value::Object(o) if o.contains_key("faults") => {
                    serde_json::from_value(o["faults"].clone())
                }
                _ => serde_json::from_value(Value::Array(vec![body.clone()])),
            };
            match rules {
                Ok(r) => {
                    let n = r.len();
                    if m == "PUT" {
                        gw.faults.set(r);
                    } else {
                        gw.faults.add(r);
                    }
                    json_resp(200, json!({"rules": n}))
                }
                Err(e) => plain(400, &format!("bad fault rules: {e}")),
            }
        }
        ("DELETE", ["faults"]) => {
            gw.faults.set(vec![]);
            json_resp(200, json!({"rules": 0}))
        }
        ("GET", ["report"]) => {
            let fmt = qv("format").unwrap_or("md");
            let strict = qv("strict").and_then(Strict::parse).or(gw.config.strict);
            let entries = gw.coverage.entries(sandbox.as_ref().map(|s| s.0.as_str()));
            let text = render(&entries, fmt, strict);
            let ct = match fmt {
                "json" => "application/json",
                "junit" => "application/xml",
                _ => "text/markdown; charset=utf-8",
            };
            to_response(
                OpOutput {
                    status: 200,
                    headers: vec![],
                    body: Body::Text {
                        content_type: ct.into(),
                        data: text,
                    },
                },
                "",
                "",
            )
        }
        ("GET", ["iam", "explain"]) => match qv("request_id").and_then(|r| gw.iam.explain(r)) {
            Some(e) => json_resp(200, serde_json::to_value(e).unwrap_or(Value::Null)),
            None => plain(404, "no IAM decision recorded for that request id"),
        },
        ("POST", ["iam", "mode"]) => match body
            .get("mode")
            .and_then(Value::as_str)
            .and_then(IamMode::parse)
        {
            Some(mode) => {
                gw.iam.set_mode(mode);
                json_resp(200, json!({"iam": mode}))
            }
            None => plain(400, "expected {\"mode\": \"off|audit|enforce\"}"),
        },
        ("POST", ["clock", "advance"]) => {
            let ms = body.get("ms").and_then(Value::as_i64).unwrap_or(0);
            gw.host.clock.advance(time::Duration::milliseconds(ms));
            json_resp(200, json!({"now": gw.host.clock.now().unix_timestamp()}))
        }
        ("GET", ["dlq"]) => json_resp(
            200,
            serde_json::to_value(gw.host.bus.dead_letters()).unwrap_or(Value::Null),
        ),
        _ => plain(404, &format!("unknown internal endpoint {m} {}", head.path)),
    }
}
