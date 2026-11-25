use crate::ir::{IrService, Matcher, Verb};
use crate::manifest::{root, ServiceEntry};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ResolvedOp {
    pub name: String,
    pub matcher: Matcher,
    pub iam_action: String,
    pub resource: String,
    pub level: String,
    pub plane: String,
    pub lro: String,
    pub emits: Vec<String>,
    pub idempotency: String,
    pub crud: Option<(String, Verb)>,
    pub custom: bool,
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub protocol: String,
    pub signing_name: String,
    pub ops: Vec<ResolvedOp>,
    pub catalog: Option<Catalog>,
    pub unknown_overrides: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub actions: Vec<String>,
    pub operations: BTreeMap<String, String>,
}

pub fn load_spec(entry: &ServiceEntry) -> Result<IrService> {
    let api = &entry.toml.service.api;
    let path = root().join(&api.spec);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading spec {}", path.display()))?;
    let doc: Value =
        serde_json::from_str(&text).with_context(|| format!("parsing spec {}", path.display()))?;
    crate::spec::load(&api.format, &doc, entry.toml.service.protocol.as_deref())
}

fn load_catalog(entry: &ServiceEntry) -> Result<Option<Catalog>> {
    let Some(p) = &entry.toml.service.api.iam_catalog else {
        return Ok(None);
    };
    let path = root().join(p);
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    )?;
    let actions = doc
        .get("actions")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let operations = doc
        .get("operations")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(Catalog {
        actions,
        operations,
    }))
}

fn resource_expr(spec: &str) -> Result<String> {
    let lit = |s: &str| format!("{s:?}");
    Ok(match spec.split_once(':') {
        None if spec == "scope" => "ResourceExtractor::Scope".into(),
        Some(("path", p)) => format!("ResourceExtractor::PathParam({})", lit(p)),
        Some(("body", p)) => format!("ResourceExtractor::BodyField({})", lit(p)),
        Some(("query", p)) => format!("ResourceExtractor::QueryParam({})", lit(p)),
        Some(("template", t)) => format!("ResourceExtractor::Template({})", lit(t)),
        _ => bail!("bad resource extractor `{spec}` (scope|path:x|body:x|query:x|template:...)"),
    })
}

pub fn resolve(entry: &ServiceEntry) -> Result<Resolved> {
    let ir = load_spec(entry)?;
    let t = &entry.toml;
    let catalog = load_catalog(entry)?;
    let default_level = t.defaults.level.clone().unwrap_or_else(|| "L1".into());
    let default_plane = t.defaults.plane.clone().unwrap_or_else(|| "control".into());
    let mut ops = Vec::new();
    for o in &ir.ops {
        let ov = t.ops.get(&o.name).cloned().unwrap_or_default();
        let crud = match ov.crud.as_deref() {
            Some("none") => None,
            Some(s) => {
                let (ty, v) = s
                    .split_once(':')
                    .with_context(|| format!("{}: crud must be `type:verb`", o.name))?;
                Some((
                    ty.to_string(),
                    Verb::parse(v).with_context(|| format!("{}: bad verb `{v}`", o.name))?,
                ))
            }
            None => o
                .crud_guess
                .clone()
                .filter(|(ty, _)| t.crud.resources.contains_key(ty)),
        };
        if let Some((ty, _)) = &crud {
            if !t.crud.resources.contains_key(ty) {
                bail!(
                    "{}: op {} binds undeclared CRUD resource `{ty}`",
                    entry.dir.display(),
                    o.name
                );
            }
        }
        let level = ov.level.clone().unwrap_or_else(|| {
            if crud.is_some() || ov.custom {
                default_level.clone()
            } else {
                "L0".into()
            }
        });
        let iam_action = ov
            .iam_action
            .clone()
            .or_else(|| {
                catalog
                    .as_ref()
                    .and_then(|c| c.operations.get(&o.name).cloned())
            })
            .unwrap_or_else(|| o.iam_action.clone());
        let resource = match &ov.resource {
            Some(r) => resource_expr(r)?,
            None => {
                let hints = crud.as_ref().and_then(|(ty, _)| t.crud.resources.get(ty));
                let body_field = hints.and_then(|h| match crud.as_ref().map(|c| &c.1) {
                    Some(Verb::Create) => h.get("name_field"),
                    Some(Verb::List) => None,
                    _ => h.get("id_field").or(h.get("name_field")),
                });
                match (&o.matcher, body_field, &o.last_path_param) {
                    (Matcher::Target(_) | Matcher::QueryAction(_), Some(f), _) => {
                        format!("ResourceExtractor::BodyField({f:?})")
                    }
                    (_, _, Some(p))
                        if !matches!(
                            crud.as_ref().map(|c| &c.1),
                            Some(Verb::List) | Some(Verb::Create)
                        ) =>
                    {
                        format!("ResourceExtractor::PathParam({p:?})")
                    }
                    _ => "ResourceExtractor::Scope".into(),
                }
            }
        };
        ops.push(ResolvedOp {
            name: o.name.clone(),
            matcher: o.matcher.clone(),
            iam_action,
            resource,
            level,
            plane: ov.plane.clone().unwrap_or_else(|| default_plane.clone()),
            lro: ov.lro.clone().unwrap_or_else(|| o.lro.clone()),
            emits: ov.emits.clone(),
            idempotency: o.idempotency.clone(),
            crud,
            custom: ov.custom,
        });
    }
    let unknown_overrides = t
        .ops
        .keys()
        .filter(|k| !ir.ops.iter().any(|o| &o.name == *k))
        .cloned()
        .collect();
    Ok(Resolved {
        protocol: ir.protocol,
        signing_name: ir.signing_name,
        ops,
        catalog,
        unknown_overrides,
    })
}
