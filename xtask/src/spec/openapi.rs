use crate::ir::{guess_crud, last_path_param, IrOp, IrService, Matcher};
use anyhow::{anyhow, Result};
use serde_json::Value;

const METHODS: &[&str] = &["get", "put", "post", "patch", "delete", "head"];

fn azure_action(path: &str, method: &str) -> Option<String> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let verb = match method {
        "put" | "patch" => "write",
        "get" | "head" => "read",
        "delete" => "delete",
        _ => "action",
    };
    if let Some(i) = segs
        .iter()
        .rposition(|s| s.eq_ignore_ascii_case("providers"))
    {
        let ns = segs.get(i + 1)?;
        if ns.starts_with('{') {
            return None;
        }
        let mut parts = vec![ns.to_string()];
        let mut j = i + 2;
        while j < segs.len() {
            if !segs[j].starts_with('{') {
                parts.push(segs[j].to_string());
            }
            j += 2;
        }
        if verb == "action" {
            return Some(format!("{}/action", parts.join("/")));
        }
        return Some(format!("{}/{verb}", parts.join("/")));
    }
    if segs
        .iter()
        .any(|s| s.eq_ignore_ascii_case("resourcegroups"))
    {
        return Some(format!(
            "Microsoft.Resources/subscriptions/resourceGroups/{verb}"
        ));
    }
    if segs
        .first()
        .is_some_and(|s| s.eq_ignore_ascii_case("subscriptions"))
    {
        return Some(format!("Microsoft.Resources/subscriptions/{verb}"));
    }
    None
}

pub fn parse(doc: &Value) -> Result<IrService> {
    let paths = doc
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("openapi: missing `paths`"))?;
    let base = doc
        .get("basePath")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    let is_arm = doc.get("swagger").is_some()
        && doc.get("host").and_then(Value::as_str) == Some("management.azure.com");
    let mut ops = Vec::new();
    for (path, item) in paths {
        for m in METHODS {
            let Some(op) = item.get(*m) else { continue };
            let Some(id) = op.get("operationId").and_then(Value::as_str) else {
                continue;
            };
            let full = format!("{base}{path}");
            let lro = if op
                .get("x-ms-long-running-operation")
                .and_then(Value::as_bool)
                == Some(true)
            {
                match *m {
                    "put" => "create",
                    "patch" => "update",
                    "delete" => "delete",
                    _ => "action",
                }
            } else {
                "none"
            };
            let iam = op
                .get("x-mcle-iam-action")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| if is_arm { azure_action(&full, m) } else { None })
                .unwrap_or_default();
            ops.push(IrOp {
                name: id.to_string(),
                matcher: Matcher::Rest {
                    method: m.to_ascii_uppercase(),
                    path: full.clone(),
                    query: vec![],
                },
                iam_action: iam,
                crud_guess: guess_crud(id),
                lro: lro.into(),
                last_path_param: last_path_param(&full),
                idempotency: if matches!(*m, "get" | "put" | "delete" | "head") {
                    "natural".into()
                } else {
                    "none".into()
                },
            });
        }
    }
    ops.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(IrService {
        protocol: if is_arm { "arm".into() } else { "rest".into() },
        signing_name: String::new(),
        ops,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn arm_actions() {
        assert_eq!(
            azure_action(
                "/subscriptions/{s}/resourceGroups/{rg}/providers/Microsoft.KeyVault/vaults/{v}",
                "put"
            ),
            Some("Microsoft.KeyVault/vaults/write".into())
        );
        assert_eq!(
            azure_action("/subscriptions/{s}/resourcegroups/{rg}", "delete"),
            Some("Microsoft.Resources/subscriptions/resourceGroups/delete".into())
        );
        assert_eq!(
            azure_action("/subscriptions/{s}/providers/{ns}/register", "post"),
            None
        );
    }
}
