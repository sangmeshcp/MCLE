use crate::ir::{singular, IrOp, IrService, Matcher, Verb};
use anyhow::{anyhow, Result};
use serde_json::Value;

const SCOPE_RESOURCES: &[&str] = &[
    "projects",
    "locations",
    "organizations",
    "folders",
    "billingAccounts",
];

fn walk(res: &Value, chain: &mut Vec<String>, out: &mut Vec<IrOp>, service: &str) {
    if let Some(methods) = res.get("methods").and_then(Value::as_object) {
        for (mname, m) in methods {
            let path = m
                .get("flatPath")
                .or_else(|| m.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let method = m.get("httpMethod").and_then(Value::as_str).unwrap_or("GET");
            let id = m
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(mname)
                .to_string();
            let leaf = chain
                .iter()
                .rev()
                .find(|c| !SCOPE_RESOURCES.contains(&c.as_str()))
                .cloned();
            let verb = Verb::parse(mname).filter(|v| *v != Verb::Upsert);
            let perm_verb = match mname.as_str() {
                "patch" => "update",
                other => other,
            };
            let iam = match &leaf {
                Some(l) => format!("{service}.{l}.{perm_verb}"),
                None => format!("{service}.{perm_verb}"),
            };
            let returns_op =
                m.pointer("/response/$ref").and_then(Value::as_str) == Some("Operation");
            let lro = if returns_op {
                match verb {
                    Some(Verb::Create) => "create",
                    Some(Verb::Update) => "update",
                    Some(Verb::Delete) => "delete",
                    _ => "action",
                }
            } else {
                "none"
            };
            let full = format!("/{}", path.trim_start_matches('/'));
            out.push(IrOp {
                name: id
                    .strip_prefix(&format!("{service}."))
                    .unwrap_or(&id)
                    .to_string(),
                matcher: Matcher::Rest {
                    method: method.into(),
                    path: full.clone(),
                    query: vec![],
                },
                iam_action: iam,
                crud_guess: match (&leaf, verb) {
                    (Some(l), Some(v)) => Some((singular(l), v)),
                    _ => None,
                },
                lro: lro.into(),
                last_path_param: crate::ir::last_path_param(&full),
                idempotency: if matches!(method, "GET" | "DELETE" | "PUT") {
                    "natural".into()
                } else {
                    "none".into()
                },
            });
        }
    }
    if let Some(children) = res.get("resources").and_then(Value::as_object) {
        for (k, v) in children {
            chain.push(k.clone());
            walk(v, chain, out, service);
            chain.pop();
        }
    }
}

pub fn parse(doc: &Value) -> Result<IrService> {
    let service = doc
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("discovery: missing `name`"))?
        .to_string();
    let mut ops = Vec::new();
    walk(doc, &mut Vec::new(), &mut ops, &service);
    ops.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(IrService {
        protocol: "gcpRest".into(),
        signing_name: service,
        ops,
    })
}
