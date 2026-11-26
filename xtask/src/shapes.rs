//! Typed shapes and API trait generated from spec definitions.

use crate::manifest::{root, ServiceEntry};
use crate::resolve::Resolved;
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
enum Ty {
    Str,
    Bool,
    Int,
    Float,
    Any,
    List(Box<Ty>),
    Map(Box<Ty>),
    Named(String),
}

#[derive(Debug, Clone)]
struct Field {
    wire: String,
    ty: Ty,
}

#[derive(Debug, Clone)]
enum Shape {
    Struct(Vec<Field>),
    Alias(Ty),
}

#[derive(Default)]
struct Set {
    shapes: BTreeMap<String, Shape>,
    // op name -> (input type, output type)
    ops: BTreeMap<String, (Ty, Ty)>,
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "try", "typeof", "unsized", "virtual", "yield", "gen",
];

fn type_name(raw: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            if up {
                out.extend(c.to_uppercase());
                up = false;
            } else {
                out.push(c);
            }
        } else {
            up = true;
        }
    }
    if out.is_empty() || out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 'T');
    }
    match out.as_str() {
        "Option" | "Result" | "Vec" | "String" | "Box" | "Value" | "Request" | "Api" | "Self" => {
            format!("{out}Shape")
        }
        _ => out,
    }
}

fn field_name(wire: &str, used: &mut BTreeSet<String>) -> String {
    let s = crate::ir::snake(wire);
    let mut s: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    s = s.trim_matches('_').to_string();
    if s.is_empty() || s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s = format!("f_{s}");
    }
    let mut cand = s.clone();
    let mut n = 2;
    while used.contains(&cand) {
        cand = format!("{s}_{n}");
        n += 1;
    }
    used.insert(cand.clone());
    if KEYWORDS.contains(&cand.as_str()) {
        format!("r#{cand}")
    } else {
        cand
    }
}

// ---------- Smithy ----------

fn smithy_ty(shapes: &Map<String, Value>, target: &str) -> Ty {
    if let Some(p) = target.strip_prefix("smithy.api#") {
        return match p {
            "String" => Ty::Str,
            "Boolean" | "PrimitiveBoolean" => Ty::Bool,
            "Integer" | "Long" | "Short" | "Byte" | "PrimitiveInteger" | "PrimitiveLong"
            | "BigInteger" => Ty::Int,
            "Float" | "Double" | "BigDecimal" | "PrimitiveFloat" | "PrimitiveDouble" => Ty::Float,
            "Blob" => Ty::Str,
            _ => Ty::Any,
        };
    }
    let Some(s) = shapes.get(target) else {
        return Ty::Any;
    };
    let short = target.rsplit('#').next().unwrap_or(target);
    match s.get("type").and_then(Value::as_str).unwrap_or("") {
        "structure" => Ty::Named(type_name(short)),
        "string" | "enum" | "blob" => Ty::Str,
        "boolean" => Ty::Bool,
        "integer" | "long" | "short" | "byte" | "intEnum" | "bigInteger" => Ty::Int,
        "float" | "double" | "bigDecimal" => Ty::Float,
        "list" | "set" => Ty::List(Box::new(
            s.pointer("/member/target")
                .and_then(Value::as_str)
                .map(|t| smithy_ty(shapes, t))
                .unwrap_or(Ty::Any),
        )),
        "map" => Ty::Map(Box::new(
            s.pointer("/value/target")
                .and_then(Value::as_str)
                .map(|t| smithy_ty(shapes, t))
                .unwrap_or(Ty::Any),
        )),
        _ => Ty::Any,
    }
}

fn smithy(doc: &Value, set: &mut Set) {
    let Some(shapes) = doc.get("shapes").and_then(Value::as_object) else {
        return;
    };
    for (id, s) in shapes {
        if s.get("type").and_then(Value::as_str) != Some("structure") {
            continue;
        }
        let short = id.rsplit('#').next().unwrap_or(id);
        let fields = s
            .get("members")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| Field {
                        wire: v
                            .pointer("/traits/smithy.api#jsonName")
                            .and_then(Value::as_str)
                            .unwrap_or(k)
                            .to_string(),
                        ty: v
                            .get("target")
                            .and_then(Value::as_str)
                            .map(|t| smithy_ty(shapes, t))
                            .unwrap_or(Ty::Any),
                    })
                    .collect()
            })
            .unwrap_or_default();
        set.shapes.insert(type_name(short), Shape::Struct(fields));
    }
    for (id, s) in shapes {
        if s.get("type").and_then(Value::as_str) != Some("operation") {
            continue;
        }
        let name = id.rsplit('#').next().unwrap_or(id).to_string();
        let io = |k: &str| {
            s.pointer(&format!("/{k}/target"))
                .and_then(Value::as_str)
                .map(|t| smithy_ty(shapes, t))
                .unwrap_or(Ty::Any)
        };
        set.ops.insert(name, (io("input"), io("output")));
    }
}

// ---------- JSON Schema (OpenAPI definitions / Discovery schemas) ----------

fn ref_name(r: &str) -> String {
    type_name(r.rsplit('/').next().unwrap_or(r))
}

fn schema_ty(s: &Value) -> Ty {
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        return Ty::Named(ref_name(r));
    }
    match s.get("type").and_then(Value::as_str) {
        Some("string") => Ty::Str,
        Some("boolean") => Ty::Bool,
        Some("integer") => Ty::Int,
        Some("number") => Ty::Float,
        Some("array") => Ty::List(Box::new(s.get("items").map(schema_ty).unwrap_or(Ty::Any))),
        Some("object") if s.get("properties").is_none() => match s.get("additionalProperties") {
            Some(ap) if ap.is_object() => Ty::Map(Box::new(schema_ty(ap))),
            _ => Ty::Any,
        },
        _ => Ty::Any,
    }
}

// Flattens `allOf` references (ARM models inherit `Resource` this way).
fn properties(all: &Map<String, Value>, s: &Value, depth: u8, out: &mut Vec<Field>) {
    if depth > 8 {
        return;
    }
    for part in s
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match part.get("$ref").and_then(Value::as_str) {
            Some(r) => {
                if let Some(base) = all.get(r.rsplit('/').next().unwrap_or(r)) {
                    properties(all, base, depth + 1, out);
                }
            }
            None => properties(all, part, depth + 1, out),
        }
    }
    for (k, v) in s
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if !out.iter().any(|f| &f.wire == k) {
            out.push(Field {
                wire: k.clone(),
                ty: schema_ty(v),
            });
        }
    }
}

fn schemas(all: &Map<String, Value>, set: &mut Set) {
    for (k, s) in all {
        let shape = if s.get("properties").is_some() || s.get("allOf").is_some() {
            let mut f = Vec::new();
            properties(all, s, 0, &mut f);
            Shape::Struct(f)
        } else {
            Shape::Alias(schema_ty(s))
        };
        set.shapes.insert(type_name(k), shape);
    }
}

fn openapi(doc: &Value, set: &mut Set) {
    let defs = doc
        .get("definitions")
        .or_else(|| doc.pointer("/components/schemas"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    schemas(&defs, set);
    for item in doc
        .get("paths")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|p| p.values())
    {
        for op in item.as_object().into_iter().flat_map(|o| o.values()) {
            let Some(id) = op.get("operationId").and_then(Value::as_str) else {
                continue;
            };
            let input = op
                .get("parameters")
                .and_then(Value::as_array)
                .and_then(|ps| {
                    ps.iter()
                        .find(|p| p.get("in").and_then(Value::as_str) == Some("body"))
                })
                .and_then(|p| p.get("schema"))
                .or_else(|| op.pointer("/requestBody/content/application~1json/schema"))
                .map(schema_ty)
                .unwrap_or(Ty::Any);
            let output = ["200", "201"]
                .iter()
                .find_map(|c| {
                    let r = op.pointer(&format!("/responses/{c}"))?;
                    r.get("schema")
                        .or_else(|| r.pointer("/content/application~1json/schema"))
                })
                .map(schema_ty)
                .unwrap_or(Ty::Any);
            set.ops.entry(id.to_string()).or_insert((input, output));
        }
    }
}

fn discovery(doc: &Value, set: &mut Set) {
    let service = doc.get("name").and_then(Value::as_str).unwrap_or("");
    if let Some(s) = doc.get("schemas").and_then(Value::as_object) {
        schemas(s, set);
    }
    fn walk(r: &Value, service: &str, set: &mut Set) {
        for m in r
            .get("methods")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|m| m.values())
        {
            let id = m.get("id").and_then(Value::as_str).unwrap_or("");
            let name = id
                .strip_prefix(&format!("{service}."))
                .unwrap_or(id)
                .to_string();
            let io = |k: &str| {
                m.pointer(&format!("/{k}/$ref"))
                    .and_then(Value::as_str)
                    .map(|r| Ty::Named(ref_name(r)))
                    .unwrap_or(Ty::Any)
            };
            set.ops.insert(name, (io("request"), io("response")));
        }
        for c in r
            .get("resources")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|c| c.values())
        {
            walk(c, service, set);
        }
    }
    walk(doc, service, set);
}

// ---------- rendering ----------

fn rust_ty(t: &Ty, known: &BTreeMap<String, Shape>) -> String {
    match t {
        Ty::Str => "String".into(),
        Ty::Bool => "bool".into(),
        Ty::Int => "i64".into(),
        Ty::Float => "f64".into(),
        Ty::Any => "serde_json::Value".into(),
        Ty::List(i) => format!("Vec<{}>", rust_ty(i, known)),
        Ty::Map(i) => format!("std::collections::BTreeMap<String, {}>", rust_ty(i, known)),
        Ty::Named(n) if known.contains_key(n) => n.clone(),
        Ty::Named(_) => "serde_json::Value".into(),
    }
}

fn load(entry: &ServiceEntry) -> Result<Set> {
    let api = &entry.toml.service.api;
    let mut set = Set::default();
    for rel in std::iter::once(&api.spec).chain(api.extra_specs.iter()) {
        let p = root().join(rel);
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&p)?)
            .with_context(|| format!("parsing {}", p.display()))?;
        match api.format.as_str() {
            "smithy" => smithy(&doc, &mut set),
            "discovery" => discovery(&doc, &mut set),
            _ => openapi(&doc, &mut set),
        }
    }
    Ok(set)
}

/// Emits `gen/shapes.rs` and `gen/api.rs` contents.
pub fn render(entry: &ServiceEntry, r: &Resolved) -> Result<(String, String)> {
    let set = load(entry)?;
    let mut sh = String::from("// @generated by `cargo xtask codegen`. DO NOT EDIT.\n#![allow(clippy::all, non_camel_case_types, dead_code, unused_imports)]\nuse serde::{Deserialize, Serialize};\n\n");
    for (name, shape) in &set.shapes {
        match shape {
            Shape::Alias(t) => writeln!(sh, "pub type {name} = {};\n", rust_ty(t, &set.shapes))?,
            Shape::Struct(fields) => {
                writeln!(sh, "#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]\n#[serde(default)]\npub struct {name} {{")?;
                let mut used = BTreeSet::new();
                for f in fields {
                    let ty = rust_ty(&f.ty, &set.shapes);
                    // Boxed so self-referential shapes have a finite size.
                    let ty = if matches!(f.ty, Ty::Named(_)) && ty != "serde_json::Value" {
                        format!("Box<{ty}>")
                    } else {
                        ty
                    };
                    writeln!(
                        sh,
                        "    #[serde(rename = {:?}, skip_serializing_if = \"Option::is_none\")]\n    pub {}: Option<{ty}>,",
                        f.wire,
                        field_name(&f.wire, &mut used)
                    )?;
                }
                writeln!(sh, "}}\n")?;
            }
        }
    }

    let mut api = String::from("// @generated by `cargo xtask codegen`. DO NOT EDIT.\n#![allow(clippy::all, unused_variables, unused_imports)]\nuse super::shapes::*;\nuse mcle_sdk::{CloudError, OpId, OpInput, OpOutput, RequestContext};\n\n");
    api.push_str("/// Decoded body plus the raw request (path/query params, headers).\n#[derive(Clone, Debug)]\npub struct Request<T> {\n    pub body: T,\n    pub raw: OpInput,\n}\n\n");
    api.push_str("#[async_trait::async_trait]\npub trait Api: Send + Sync {\n");
    let mut sigs = Vec::new();
    for op in &r.ops {
        let (i, o) = set.ops.get(&op.name).cloned().unwrap_or((Ty::Any, Ty::Any));
        let (i, o) = (rust_ty(&i, &set.shapes), rust_ty(&o, &set.shapes));
        let f = crate::gen::const_name(&op.name).to_ascii_lowercase();
        let f = if KEYWORDS.contains(&f.as_str()) {
            format!("r#{f}")
        } else {
            f
        };
        writeln!(
            api,
            "    async fn {f}(&self, ctx: &RequestContext, req: Request<{i}>) -> Result<{o}, CloudError> {{\n        Err(CloudError::not_implemented(&ctx.service, &ctx.op))\n    }}"
        )?;
        sigs.push((crate::gen::const_name(&op.name), f, i));
    }
    api.push_str("}\n\n");
    api.push_str("/// Decodes the JSON body into the op's input type, calls `api`, and encodes the output.\npub async fn dispatch<A: Api + ?Sized>(api: &A, ctx: &RequestContext, op: OpId, input: OpInput) -> Result<OpOutput, CloudError> {\n    fn decode<T: serde::de::DeserializeOwned + Default>(v: &serde_json::Value) -> Result<T, CloudError> {\n        if v.is_null() {\n            return Ok(T::default());\n        }\n        serde_json::from_value(v.clone()).map_err(|e| CloudError::invalid(format!(\"malformed request body: {e}\")))\n    }\n    fn encode<T: serde::Serialize>(v: T) -> Result<OpOutput, CloudError> {\n        serde_json::to_value(v).map(OpOutput::json).map_err(|e| CloudError::internal(e.to_string()))\n    }\n    match op {\n");
    for (c, f, i) in &sigs {
        writeln!(api, "        super::op::{c} => {{\n            let body: {i} = decode(&input.body)?;\n            encode(api.{f}(ctx, Request {{ body, raw: input }}).await?)\n        }}")?;
    }
    api.push_str("        _ => Err(CloudError::internal(\"unknown op id\")),\n    }\n}\n");
    Ok((crate::gen::rustfmt(&sh), crate::gen::rustfmt(&api)))
}
