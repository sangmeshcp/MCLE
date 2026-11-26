use crate::template;
use mcle_sdk::crud::CrudResult;
use mcle_sdk::{
    CloudError, CrudRecord, CrudResource, CrudShaper, CrudTarget, CrudVerb, Host, OpInput,
    OpOutput, RequestContext, Scope,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub struct AwsJsonShaper {
    host: Host,
}

impl AwsJsonShaper {
    pub fn new(host: &Host) -> Self {
        AwsJsonShaper { host: host.clone() }
    }
}

fn scope_parts(ctx: &RequestContext) -> (String, String) {
    match &ctx.scope {
        Scope::Aws { account, region } => (account.clone(), region.clone()),
        _ => ("000000000000".into(), "us-east-1".into()),
    }
}

fn epoch(t: time::OffsetDateTime) -> Value {
    json!(t.unix_timestamp_nanos() as f64 / 1e9)
}

impl AwsJsonShaper {
    fn arn_of(rec: &CrudRecord, res: &CrudResource) -> String {
        let f = res.hint("arn_output").unwrap_or("Arn");
        rec.body
            .get(f)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    fn view(&self, rec: &CrudRecord, res: &CrudResource) -> Value {
        let mut out = rec.body.clone();
        if let Some(o) = out.as_object_mut() {
            if let Some(f) = res.hint("created_field") {
                o.insert(f.into(), epoch(rec.created));
            }
            if let Some(f) = res.hint("updated_field") {
                o.insert(f.into(), epoch(rec.updated));
            }
        }
        out
    }

    fn ident(&self, rec: &CrudRecord, res: &CrudResource) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert(
            res.hint("arn_output").unwrap_or("Arn").into(),
            Value::String(Self::arn_of(rec, res)),
        );
        m.insert(
            res.hint("name_output").unwrap_or("Name").into(),
            Value::String(rec.name.clone()),
        );
        m
    }
}

impl CrudShaper for AwsJsonShaper {
    fn target(
        &self,
        ctx: &RequestContext,
        res: &CrudResource,
        verb: CrudVerb,
        input: &OpInput,
    ) -> Result<CrudTarget, CloudError> {
        let (account, region) = scope_parts(ctx);
        let parent = format!("{account}/{region}");
        if verb == CrudVerb::List {
            return Ok(CrudTarget {
                id: String::new(),
                name: String::new(),
                parent,
            });
        }
        let field = if verb == CrudVerb::Create {
            res.hint("name_field").unwrap_or("Name")
        } else {
            res.hint("id_field")
                .or(res.hint("name_field"))
                .unwrap_or("Name")
        };
        let raw = input
            .body_str(field)
            .ok_or_else(|| CloudError::invalid(format!("1 validation error detected: Value null at '{field}' failed to satisfy constraint: Member must not be null")))?;
        let name = if raw.starts_with("arn:") {
            let t = res.hint("arn_template").unwrap_or("");
            template::matches(t, raw)
                .and_then(|m| m.get("name").cloned())
                .ok_or_else(|| {
                    CloudError::not_found(format!("{} {raw} not found", res.type_name))
                })?
        } else {
            raw.to_string()
        };
        Ok(CrudTarget {
            id: format!("{parent}/{name}"),
            name,
            parent,
        })
    }

    fn create_body(
        &self,
        ctx: &RequestContext,
        res: &CrudResource,
        target: &CrudTarget,
        input: &OpInput,
    ) -> Result<Value, CloudError> {
        let (account, region) = scope_parts(ctx);
        let mut vars = BTreeMap::new();
        vars.insert("partition", "aws".to_string());
        vars.insert("region", region);
        vars.insert("account", account);
        vars.insert("name", target.name.clone());
        let ids = self.host.ids.clone();
        let arn = template::expand(
            res.hint("arn_template")
                .unwrap_or("arn:{partition}:unknown:{region}:{account}:{name}"),
            &vars,
            &|n| ids.alnum(n),
        );
        let redact: Vec<&str> = res
            .hint("redact")
            .map(|r| r.split(',').map(str::trim).collect())
            .unwrap_or_default();
        let mut body = Map::new();
        if let Some(o) = input.body.as_object() {
            for (k, v) in o {
                if !redact.contains(&k.as_str()) {
                    body.insert(k.clone(), v.clone());
                }
            }
        }
        body.insert(
            res.hint("arn_output").unwrap_or("Arn").into(),
            Value::String(arn),
        );
        body.insert(
            res.hint("name_output").unwrap_or("Name").into(),
            Value::String(target.name.clone()),
        );
        Ok(Value::Object(body))
    }

    fn update_body(
        &self,
        _ctx: &RequestContext,
        res: &CrudResource,
        existing: &CrudRecord,
        input: &OpInput,
    ) -> Result<Value, CloudError> {
        let redact: Vec<&str> = res
            .hint("redact")
            .map(|r| r.split(',').map(str::trim).collect())
            .unwrap_or_default();
        let id_field = res.hint("id_field").unwrap_or("");
        let mut body = existing.body.clone();
        if let (Some(dst), Some(src)) = (body.as_object_mut(), input.body.as_object()) {
            for (k, v) in src {
                if !redact.contains(&k.as_str()) && k != id_field {
                    dst.insert(k.clone(), v.clone());
                }
            }
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
            CrudResult::One { record, .. } => match verb {
                CrudVerb::Read => OpOutput::json(self.view(record, res)),
                _ => OpOutput::json(Value::Object(self.ident(record, res))),
            },
            CrudResult::Many(recs) => {
                let list: Vec<Value> = recs.iter().map(|r| self.view(r, res)).collect();
                OpOutput::json(json!({ res.hint("list_field").unwrap_or("Items"): list }))
            }
            CrudResult::Deleted(rec) => {
                let mut m = self.ident(rec, res);
                if let Some(f) = res.hint("deleted_field") {
                    m.insert(f.into(), epoch(self.host.clock.now()));
                }
                OpOutput::json(Value::Object(m))
            }
        })
    }

    fn not_found(&self, res: &CrudResource, target: &CrudTarget) -> CloudError {
        CloudError::not_found(format!(
            "{} can't find the specified {}: {}",
            res.hint("display").unwrap_or("MCLE"),
            res.type_name,
            target.name
        ))
        .with_code(
            res.hint("not_found_code")
                .unwrap_or("ResourceNotFoundException"),
        )
    }

    fn already_exists(&self, res: &CrudResource, target: &CrudTarget) -> CloudError {
        CloudError::already_exists(format!(
            "The operation failed because the {} {} already exists.",
            res.type_name, target.name
        ))
        .with_code(res.hint("exists_code").unwrap_or("ResourceExistsException"))
    }
}
