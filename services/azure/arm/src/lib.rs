#![forbid(unsafe_code)]

#[path = "../gen/mod.rs"]
pub mod gen;

use mcle_cloud_azure::{lro_headers, DEFAULT_SUBSCRIPTION, DEFAULT_TENANT};
use mcle_engine_crud::CrudEngine;
use mcle_sdk::{
    CloudAdapter, CloudError, CloudId, CrudRecord, CrudTarget, Host, LroBehavior, LroState, OpId,
    OpInput, OpMeta, OpOutput, RequestContext, RouteTable, ServiceFacade, ServiceManifest,
};
use serde_json::{json, Map, Value};
use std::sync::Arc;

const RG: &str = "resourcegroup";
const RES: &str = "resource";
const META: &str = "_mcle";

static PROVIDERS: &[&str] = &[
    "Microsoft.Authorization",
    "Microsoft.Compute",
    "Microsoft.ContainerRegistry",
    "Microsoft.DocumentDB",
    "Microsoft.EventHub",
    "Microsoft.Insights",
    "Microsoft.KeyVault",
    "Microsoft.ManagedIdentity",
    "Microsoft.Network",
    "Microsoft.OperationalInsights",
    "Microsoft.Resources",
    "Microsoft.ServiceBus",
    "Microsoft.Storage",
    "Microsoft.Web",
];

pub struct Arm {
    engine: CrudEngine,
    host: Host,
}

pub fn facade(host: &Host, _adapter: &dyn CloudAdapter) -> Arc<dyn ServiceFacade> {
    Arc::new(Arm {
        engine: CrudEngine::new(host),
        host: host.clone(),
    })
}

fn param<'a>(input: &'a OpInput, k: &str) -> Result<&'a str, CloudError> {
    input
        .param(k)
        .ok_or_else(|| CloudError::invalid(format!("missing `{k}`")))
}

fn rg_id(sub: &str, rg: &str) -> String {
    format!("/subscriptions/{sub}/resourceGroups/{rg}")
}

// A provider path `ns/type/name[/type/name...]` addresses an item when it has an
// odd number of segments, and a collection when even.
struct ProviderPath {
    namespace: String,
    types: Vec<String>,
    names: Vec<String>,
}

impl ProviderPath {
    fn parse(path: &str) -> Result<Self, CloudError> {
        let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if segs.len() < 2 {
            return Err(CloudError::invalid(
                "resource path must be `{namespace}/{type}[/{name}...]`",
            )
            .with_code("InvalidResourceType"));
        }
        let mut types = Vec::new();
        let mut names = Vec::new();
        for (i, s) in segs[1..].iter().enumerate() {
            if i % 2 == 0 {
                types.push(s.to_string());
            } else {
                names.push(s.to_string());
            }
        }
        Ok(ProviderPath {
            namespace: segs[0].to_string(),
            types,
            names,
        })
    }
    fn is_item(&self) -> bool {
        self.types.len() == self.names.len()
    }
    fn type_name(&self) -> String {
        format!("{}/{}", self.namespace, self.types.join("/"))
    }
    fn name(&self) -> String {
        self.names.join("/")
    }
}

impl Arm {
    fn require_api_version(input: &OpInput) -> Result<(), CloudError> {
        if input.query("api-version").is_none() {
            return Err(CloudError::invalid(
                "The api-version query parameter (?api-version=) is required for all requests.",
            )
            .with_code("MissingApiVersionParameter"));
        }
        Ok(())
    }

    fn lro_state(&self, ctx: &RequestContext, body: &Value) -> Option<LroState> {
        let id = body.pointer(&format!("/{META}/lro"))?.as_str()?;
        self.host.lro.get(&ctx.sandbox, id).map(|o| o.state)
    }

    fn view(&self, ctx: &RequestContext, rec: &CrudRecord) -> Value {
        let mut b = rec.body.clone();
        let state = match self.lro_state(ctx, &rec.body) {
            Some(LroState::Succeeded) | None => "Succeeded",
            Some(LroState::Failed) => "Failed",
            Some(LroState::Cancelled) => "Canceled",
            Some(_) => {
                if rec.generation == 1 {
                    "Creating"
                } else {
                    "Updating"
                }
            }
        };
        if let Some(o) = b.as_object_mut() {
            o.remove(META);
            let props = o.entry("properties").or_insert_with(|| json!({}));
            if let Some(p) = props.as_object_mut() {
                p.insert("provisioningState".into(), Value::String(state.into()));
            }
        }
        b
    }

    fn list(
        &self,
        ctx: &RequestContext,
        ty: &str,
        parent: &str,
        filter_type: Option<&str>,
    ) -> OpOutput {
        let value: Vec<Value> = self
            .engine
            .list(&ctx.sandbox, ty, parent)
            .iter()
            .filter(|r| {
                filter_type.is_none_or(|t| {
                    r.body
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|x| x.eq_ignore_ascii_case(t))
                })
            })
            .map(|r| self.view(ctx, r))
            .collect();
        OpOutput::json(json!({ "value": value }))
    }

    // Stores `body` at `id` (create or replace) and returns the ARM PUT response,
    // starting an LRO when a completion delay is configured.
    fn put(
        &self,
        ctx: &RequestContext,
        ty: &str,
        target: CrudTarget,
        mut body: Map<String, Value>,
    ) -> Result<OpOutput, CloudError> {
        let sb = &ctx.sandbox;
        let lro = (self.host.lro.delay_ms() > 0).then(|| {
            self.host.lro.start(
                sb,
                CloudId::AZURE,
                LroBehavior::Create,
                Some(target.id.clone()),
                Value::Null,
            )
        });
        if let Some(op) = &lro {
            body.insert(META.into(), json!({"lro": op.id}));
        }
        let (rec, created) = match self.engine.get(sb, ty, &target.id) {
            Some(existing) => {
                let mut merged = body;
                if let Some(old) = existing.body.get("properties").and_then(Value::as_object) {
                    let props = merged.entry("properties").or_insert_with(|| json!({}));
                    if let Some(p) = props.as_object_mut() {
                        for (k, v) in old {
                            p.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                    }
                }
                (
                    self.engine.replace(sb, existing, Value::Object(merged))?,
                    false,
                )
            }
            None => match self.engine.create(sb, ty, &target, Value::Object(body))? {
                Some(r) => (r, true),
                None => return Err(CloudError::conflict("concurrent create")),
            },
        };
        let mut out = OpOutput::json_status(if created { 201 } else { 200 }, self.view(ctx, &rec));
        if let Some(op) = lro {
            out.headers.extend(lro_headers(&op, &ctx.public_base_url));
        }
        Ok(out)
    }

    fn patch(
        &self,
        ctx: &RequestContext,
        ty: &str,
        key: &str,
        input: &OpInput,
        not_found: CloudError,
    ) -> Result<OpOutput, CloudError> {
        let existing = self.engine.get(&ctx.sandbox, ty, key).ok_or(not_found)?;
        let mut body = existing.body.clone();
        if let (Some(dst), Some(src)) = (body.as_object_mut(), input.body.as_object()) {
            for (k, v) in src {
                match (k.as_str(), dst.get_mut(k)) {
                    ("properties", Some(Value::Object(p))) => {
                        if let Some(np) = v.as_object() {
                            p.extend(np.clone());
                        }
                    }
                    _ => {
                        dst.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        let rec = self.engine.replace(&ctx.sandbox, existing, body)?;
        Ok(OpOutput::json(self.view(ctx, &rec)))
    }

    fn delete_response(&self, ctx: &RequestContext, resource: String) -> OpOutput {
        if self.host.lro.delay_ms() == 0 {
            return OpOutput::empty(200);
        }
        let op = self.host.lro.start(
            &ctx.sandbox,
            CloudId::AZURE,
            LroBehavior::Delete,
            Some(resource),
            Value::Null,
        );
        let mut out = OpOutput::empty(202);
        out.headers.extend(lro_headers(&op, &ctx.public_base_url));
        out
    }

    fn rg_not_found(rg: &str) -> CloudError {
        CloudError::not_found(format!("Resource group '{rg}' could not be found."))
            .with_code("ResourceGroupNotFound")
    }

    fn resource_not_found(pp: &ProviderPath, rg: &str) -> CloudError {
        CloudError::not_found(format!(
            "The Resource '{}/{}' under resource group '{rg}' was not found.",
            pp.type_name(),
            pp.name()
        ))
        .with_code("ResourceNotFound")
    }

    fn subscription(sub: &str) -> Value {
        json!({
            "id": format!("/subscriptions/{sub}"),
            "subscriptionId": sub,
            "tenantId": DEFAULT_TENANT,
            "displayName": "MCLE",
            "state": "Enabled",
            "subscriptionPolicies": {"locationPlacementId": "Public_2014-09-01", "quotaId": "PayAsYouGo_2014-09-01", "spendingLimit": "Off"},
            "authorizationSource": "RoleBased",
        })
    }

    fn provider(sub: &str, ns: &str) -> Value {
        json!({
            "id": format!("/subscriptions/{sub}/providers/{ns}"),
            "namespace": ns,
            "registrationState": "Registered",
            "registrationPolicy": "RegistrationRequired",
            "resourceTypes": [],
        })
    }

    fn generic(
        &self,
        ctx: &RequestContext,
        op: OpId,
        input: &OpInput,
    ) -> Result<OpOutput, CloudError> {
        use gen::op::*;
        let sub = param(input, "subscriptionId")?;
        let rg = param(input, "resourceGroupName")?;
        let pp = ProviderPath::parse(param(input, "resourcePath")?)?;
        let rg_key = rg_id(sub, rg).to_ascii_lowercase();
        if self.engine.get(&ctx.sandbox, RG, &rg_key).is_none() && op != RESOURCES_DELETE {
            return Err(Self::rg_not_found(rg));
        }
        let raw_path = param(input, "resourcePath")?;
        let clean: Vec<&str> = raw_path.split('/').filter(|s| !s.is_empty()).collect();
        let display_id = format!("{}/providers/{}", rg_id(sub, rg), clean.join("/"));
        let key = display_id.to_ascii_lowercase();
        let parent = format!("{rg_key}/");
        if !pp.is_item() {
            if op != RESOURCES_GET {
                return Err(CloudError::invalid("collection paths only support GET")
                    .with_code("InvalidRequestUri"));
            }
            // Collection ids are `<parent>/providers/ns/type`; items are `<that>/<name>`.
            let ty = pp.type_name();
            let value: Vec<Value> = self
                .engine
                .list_all_with_id_prefix(&ctx.sandbox, &format!("{key}/"))
                .iter()
                .filter(|r| {
                    r.resource_type == RES
                        && r.body
                            .get("type")
                            .and_then(Value::as_str)
                            .is_some_and(|t| t.eq_ignore_ascii_case(&ty))
                })
                .map(|r| self.view(ctx, r))
                .collect();
            return Ok(OpOutput::json(json!({ "value": value })));
        }
        let target = CrudTarget {
            id: key.clone(),
            name: pp.name(),
            parent,
        };
        match op {
            RESOURCES_CREATE_OR_UPDATE => {
                let mut body = input.body.as_object().cloned().unwrap_or_default();
                body.insert("id".into(), Value::String(display_id));
                body.insert(
                    "name".into(),
                    Value::String(pp.names.last().cloned().unwrap_or_default()),
                );
                body.insert("type".into(), Value::String(pp.type_name()));
                body.entry("tags").or_insert_with(|| json!({}));
                self.put(ctx, RES, target, body)
            }
            RESOURCES_GET => {
                let rec = self
                    .engine
                    .get(&ctx.sandbox, RES, &key)
                    .ok_or_else(|| Self::resource_not_found(&pp, rg))?;
                Ok(OpOutput::json(self.view(ctx, &rec)))
            }
            RESOURCES_CHECK_EXISTENCE => Ok(OpOutput::empty(
                if self.engine.get(&ctx.sandbox, RES, &key).is_some() {
                    204
                } else {
                    404
                },
            )),
            RESOURCES_UPDATE => {
                self.patch(ctx, RES, &key, input, Self::resource_not_found(&pp, rg))
            }
            RESOURCES_DELETE => {
                if self.engine.delete(&ctx.sandbox, RES, &key).is_none() {
                    return Ok(OpOutput::empty(204));
                }
                for child in self
                    .engine
                    .list_all_with_id_prefix(&ctx.sandbox, &format!("{key}/"))
                {
                    self.engine.delete(&ctx.sandbox, RES, &child.id);
                }
                Ok(self.delete_response(ctx, display_id))
            }
            _ => Err(CloudError::not_implemented(&ctx.service, &ctx.op)),
        }
    }
}

#[async_trait::async_trait]
impl ServiceFacade for Arm {
    fn manifest(&self) -> &'static ServiceManifest {
        &gen::MANIFEST
    }
    fn routes(&self) -> &'static RouteTable {
        &gen::ROUTES
    }
    fn ops(&self) -> &'static [OpMeta] {
        &gen::OPS
    }

    async fn dispatch(
        &self,
        ctx: &RequestContext,
        op: OpId,
        input: OpInput,
    ) -> Result<OpOutput, CloudError> {
        use gen::op::*;
        Self::require_api_version(&input)?;
        let sb = &ctx.sandbox;
        match op {
            SUBSCRIPTIONS_LIST => Ok(OpOutput::json(
                json!({"value": [Self::subscription(DEFAULT_SUBSCRIPTION)]}),
            )),
            SUBSCRIPTIONS_GET => Ok(OpOutput::json(Self::subscription(param(
                &input,
                "subscriptionId",
            )?))),
            PROVIDERS_LIST => {
                let sub = param(&input, "subscriptionId")?;
                Ok(OpOutput::json(
                    json!({"value": PROVIDERS.iter().map(|ns| Self::provider(sub, ns)).collect::<Vec<_>>()}),
                ))
            }
            PROVIDERS_GET | PROVIDERS_REGISTER => Ok(OpOutput::json(Self::provider(
                param(&input, "subscriptionId")?,
                param(&input, "resourceProviderNamespace")?,
            ))),
            RESOURCE_GROUPS_LIST => {
                let sub = param(&input, "subscriptionId")?;
                Ok(self.list(
                    ctx,
                    RG,
                    &format!("/subscriptions/{sub}/").to_ascii_lowercase(),
                    None,
                ))
            }
            RESOURCE_GROUPS_CREATE_OR_UPDATE
            | RESOURCE_GROUPS_GET
            | RESOURCE_GROUPS_CHECK_EXISTENCE
            | RESOURCE_GROUPS_UPDATE
            | RESOURCE_GROUPS_DELETE => {
                let sub = param(&input, "subscriptionId")?;
                let rg = param(&input, "resourceGroupName")?;
                let display = rg_id(sub, rg);
                let key = display.to_ascii_lowercase();
                match op {
                    RESOURCE_GROUPS_CREATE_OR_UPDATE => {
                        let location = input.body_str("location").ok_or_else(|| {
                            CloudError::invalid("The 'location' property is required.")
                                .with_code("LocationRequired")
                        })?;
                        let mut body = Map::new();
                        body.insert("id".into(), Value::String(display.clone()));
                        body.insert("name".into(), Value::String(rg.into()));
                        body.insert(
                            "type".into(),
                            Value::String("Microsoft.Resources/resourceGroups".into()),
                        );
                        body.insert("location".into(), Value::String(location.into()));
                        body.insert(
                            "tags".into(),
                            input.body.get("tags").cloned().unwrap_or(json!({})),
                        );
                        if let Some(m) = input.body.get("managedBy") {
                            body.insert("managedBy".into(), m.clone());
                        }
                        let target = CrudTarget {
                            id: key,
                            name: rg.into(),
                            parent: format!("/subscriptions/{sub}/").to_ascii_lowercase(),
                        };
                        let mut out = self.put(ctx, RG, target, body)?;
                        out.headers
                            .retain(|(k, _)| k != "azure-asyncoperation" && k != "location");
                        Ok(out)
                    }
                    RESOURCE_GROUPS_GET => {
                        let rec = self
                            .engine
                            .get(sb, RG, &key)
                            .ok_or_else(|| Self::rg_not_found(rg))?;
                        Ok(OpOutput::json(self.view(ctx, &rec)))
                    }
                    RESOURCE_GROUPS_CHECK_EXISTENCE => Ok(OpOutput::empty(
                        if self.engine.get(sb, RG, &key).is_some() {
                            204
                        } else {
                            404
                        },
                    )),
                    RESOURCE_GROUPS_UPDATE => {
                        self.patch(ctx, RG, &key, &input, Self::rg_not_found(rg))
                    }
                    _ => {
                        self.engine
                            .delete(sb, RG, &key)
                            .ok_or_else(|| Self::rg_not_found(rg))?;
                        for r in self.engine.list(sb, RES, &format!("{key}/")) {
                            self.engine.delete(sb, RES, &r.id);
                        }
                        Ok(self.delete_response(ctx, display))
                    }
                }
            }
            RESOURCES_LIST_BY_RESOURCE_GROUP => {
                let sub = param(&input, "subscriptionId")?;
                let rg = param(&input, "resourceGroupName")?;
                let key = rg_id(sub, rg).to_ascii_lowercase();
                if self.engine.get(sb, RG, &key).is_none() {
                    return Err(Self::rg_not_found(rg));
                }
                Ok(self.list(ctx, RES, &format!("{key}/"), None))
            }
            RESOURCES_LIST => {
                let sub = param(&input, "subscriptionId")?;
                Ok(self.list(
                    ctx,
                    RES,
                    &format!("/subscriptions/{sub}/").to_ascii_lowercase(),
                    None,
                ))
            }
            RESOURCES_LIST_BY_SUBSCRIPTION_AND_TYPE => {
                let sub = param(&input, "subscriptionId")?;
                let pp = ProviderPath::parse(param(&input, "resourcePath")?)?;
                if pp.is_item() {
                    return Err(CloudError::invalid(
                        "subscription-level resources are not supported",
                    )
                    .with_code("InvalidResourceType"));
                }
                Ok(self.list(
                    ctx,
                    RES,
                    &format!("/subscriptions/{sub}/").to_ascii_lowercase(),
                    Some(&pp.type_name()),
                ))
            }
            _ => self.generic(ctx, op, &input),
        }
    }
}
