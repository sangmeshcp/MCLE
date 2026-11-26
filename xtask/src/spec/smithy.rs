use crate::ir::{guess_crud, last_path_param, snake, IrOp, IrService, Matcher, Verb};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::BTreeMap;

const PROTOCOLS: &[(&str, &str)] = &[
    ("aws.protocols#awsJson1_0", "awsJson1_0"),
    ("aws.protocols#awsJson1_1", "awsJson1_1"),
    ("aws.protocols#restJson1", "restJson1"),
    ("aws.protocols#restXml", "restXml"),
    ("aws.protocols#awsQuery", "awsQuery"),
    ("aws.protocols#ec2Query", "ec2Query"),
];

fn short(id: &str) -> &str {
    id.rsplit('#').next().unwrap_or(id)
}

pub fn parse(doc: &Value) -> Result<IrService> {
    let shapes = doc
        .get("shapes")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("smithy: missing `shapes`"))?;
    let (svc_id, svc) = shapes
        .iter()
        .find(|(_, s)| s.get("type").and_then(Value::as_str) == Some("service"))
        .ok_or_else(|| anyhow!("smithy: no service shape"))?;
    let traits = svc.get("traits").cloned().unwrap_or(Value::Null);
    let protocol = PROTOCOLS
        .iter()
        .find(|(k, _)| traits.get(*k).is_some())
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| "awsJson1_1".into());
    let arn_ns = traits
        .pointer("/aws.api#service/arnNamespace")
        .and_then(Value::as_str)
        .or_else(|| {
            traits
                .pointer("/aws.auth#sigv4/name")
                .and_then(Value::as_str)
        })
        .unwrap_or("unknown")
        .to_string();
    let signing = traits
        .pointer("/aws.auth#sigv4/name")
        .and_then(Value::as_str)
        .unwrap_or(&arn_ns)
        .to_string();
    let svc_short = short(svc_id).to_string();

    let mut ops: BTreeMap<String, Option<(String, Verb)>> = BTreeMap::new();
    let targets = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.get("target").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    for t in targets(svc.get("operations")) {
        ops.entry(t).or_insert(None);
    }
    let mut stack = targets(svc.get("resources"));
    while let Some(rid) = stack.pop() {
        let Some(r) = shapes.get(&rid) else { continue };
        let rname = snake(short(&rid));
        for (k, verb) in [
            ("create", Verb::Create),
            ("put", Verb::Upsert),
            ("read", Verb::Read),
            ("update", Verb::Update),
            ("delete", Verb::Delete),
            ("list", Verb::List),
        ] {
            if let Some(t) = r.pointer(&format!("/{k}/target")).and_then(Value::as_str) {
                ops.insert(t.to_string(), Some((rname.clone(), verb)));
            }
        }
        for t in targets(r.get("operations"))
            .into_iter()
            .chain(targets(r.get("collectionOperations")))
        {
            ops.entry(t).or_insert(None);
        }
        stack.extend(targets(r.get("resources")));
    }

    let mut out = Vec::new();
    for (oid, lifecycle) in ops {
        let Some(op) = shapes.get(&oid) else { continue };
        let name = short(&oid).to_string();
        let otraits = op.get("traits").cloned().unwrap_or(Value::Null);
        let matcher = match protocol.as_str() {
            "awsJson1_0" | "awsJson1_1" => Matcher::Target(format!("{svc_short}.{name}")),
            "awsQuery" | "ec2Query" => Matcher::QueryAction(name.clone()),
            _ => {
                let method = otraits
                    .pointer("/smithy.api#http/method")
                    .and_then(Value::as_str)
                    .unwrap_or("POST");
                let uri = otraits
                    .pointer("/smithy.api#http/uri")
                    .and_then(Value::as_str)
                    .unwrap_or("/");
                let (path, q) = uri.split_once('?').unwrap_or((uri, ""));
                let query = q
                    .split('&')
                    .filter(|s| !s.is_empty())
                    .map(|kv| kv.split('=').next().unwrap_or(kv).to_string())
                    .collect();
                Matcher::Rest {
                    method: method.into(),
                    path: path.into(),
                    query,
                }
            }
        };
        let input = op
            .pointer("/input/target")
            .and_then(Value::as_str)
            .and_then(|t| shapes.get(t));
        let token = input
            .and_then(|i| i.get("members"))
            .and_then(Value::as_object)
            .and_then(|m| {
                m.iter()
                    .find(|(_, mv)| mv.pointer("/traits/smithy.api#idempotencyToken").is_some())
                    .map(|(k, _)| k.clone())
            });
        let idempotency = match token {
            Some(t) => format!("token:{t}"),
            None if otraits.get("smithy.api#idempotent").is_some()
                || otraits.get("smithy.api#readonly").is_some() =>
            {
                "natural".into()
            }
            None => "none".into(),
        };
        let last = match &matcher {
            Matcher::Rest { path, .. } => last_path_param(path),
            _ => None,
        };
        out.push(IrOp {
            iam_action: format!("{arn_ns}:{name}"),
            crud_guess: lifecycle.or_else(|| guess_crud(&name)),
            lro: "none".into(),
            last_path_param: last,
            idempotency,
            name,
            matcher,
        });
    }
    Ok(IrService {
        protocol,
        signing_name: signing,
        ops: out,
    })
}
