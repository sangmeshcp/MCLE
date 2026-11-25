#![forbid(unsafe_code)]

use mcle_sdk::crud::CrudResult;
use mcle_sdk::{
    CloudError, CloudEvent, CrudRecord, CrudResource, CrudShaper, CrudTarget, CrudVerb, Engine,
    Host, OpId, OpInput, OpMeta, OpOutput, RequestContext, RouteTable, SandboxId, ServiceFacade,
    ServiceManifest, Store,
};
use serde_json::Value;
use std::sync::Arc;

pub const NS: &str = "crud";

#[derive(Clone)]
pub struct CrudEngine {
    store: Store<CrudRecord>,
    host: Host,
}

fn key(resource_type: &str, id: &str) -> String {
    format!("{resource_type}|{id}")
}

impl CrudEngine {
    pub fn new(host: &Host) -> Self {
        CrudEngine {
            store: Store::new(host.state.clone(), NS),
            host: host.clone(),
        }
    }

    pub fn get(&self, sb: &SandboxId, ty: &str, id: &str) -> Option<CrudRecord> {
        self.store.get(sb, &key(ty, id))
    }

    pub fn create(
        &self,
        sb: &SandboxId,
        ty: &str,
        t: &CrudTarget,
        body: Value,
    ) -> Result<Option<CrudRecord>, CloudError> {
        let now = self.host.clock.now();
        let rec = CrudRecord {
            id: t.id.clone(),
            name: t.name.clone(),
            resource_type: ty.to_string(),
            parent: t.parent.clone(),
            body,
            created: now,
            updated: now,
            generation: 1,
        };
        Ok(self
            .store
            .insert_new(sb, &key(ty, &t.id), &rec)?
            .then_some(rec))
    }

    pub fn replace(
        &self,
        sb: &SandboxId,
        mut rec: CrudRecord,
        body: Value,
    ) -> Result<CrudRecord, CloudError> {
        rec.body = body;
        rec.updated = self.host.clock.now();
        rec.generation += 1;
        self.store
            .put(sb, &key(&rec.resource_type, &rec.id), &rec)?;
        Ok(rec)
    }

    pub fn delete(&self, sb: &SandboxId, ty: &str, id: &str) -> Option<CrudRecord> {
        self.store.delete(sb, &key(ty, id))
    }

    pub fn list(&self, sb: &SandboxId, ty: &str, parent_prefix: &str) -> Vec<CrudRecord> {
        self.store
            .list(sb, &format!("{ty}|"))
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.parent.starts_with(parent_prefix))
            .collect()
    }

    pub fn list_all_with_id_prefix(&self, sb: &SandboxId, prefix: &str) -> Vec<CrudRecord> {
        self.store
            .list(sb, "")
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.id.starts_with(prefix))
            .collect()
    }
}

impl Engine for CrudEngine {
    fn name(&self) -> &'static str {
        "crud"
    }
}

pub struct NoShaper;

impl CrudShaper for NoShaper {
    fn target(
        &self,
        ctx: &RequestContext,
        _r: &CrudResource,
        _v: CrudVerb,
        _i: &OpInput,
    ) -> Result<CrudTarget, CloudError> {
        Err(CloudError::not_implemented(&ctx.service, &ctx.op))
    }
    fn create_body(
        &self,
        ctx: &RequestContext,
        _r: &CrudResource,
        _t: &CrudTarget,
        _i: &OpInput,
    ) -> Result<Value, CloudError> {
        Err(CloudError::not_implemented(&ctx.service, &ctx.op))
    }
    fn render(
        &self,
        ctx: &RequestContext,
        _r: &CrudResource,
        _v: CrudVerb,
        _res: CrudResult<'_>,
    ) -> Result<OpOutput, CloudError> {
        Err(CloudError::not_implemented(&ctx.service, &ctx.op))
    }
}

pub struct GenericCrudFacade {
    manifest: &'static ServiceManifest,
    routes: &'static RouteTable,
    ops: &'static [OpMeta],
    resources: &'static [CrudResource],
    shaper: Arc<dyn CrudShaper>,
    engine: CrudEngine,
    host: Host,
}

impl GenericCrudFacade {
    pub fn new(
        host: &Host,
        manifest: &'static ServiceManifest,
        routes: &'static RouteTable,
        ops: &'static [OpMeta],
        resources: &'static [CrudResource],
        shaper: Arc<dyn CrudShaper>,
    ) -> Self {
        GenericCrudFacade {
            manifest,
            routes,
            ops,
            resources,
            shaper,
            engine: CrudEngine::new(host),
            host: host.clone(),
        }
    }

    pub fn engine(&self) -> &CrudEngine {
        &self.engine
    }

    pub async fn serve(
        &self,
        ctx: &RequestContext,
        meta: &OpMeta,
        input: OpInput,
    ) -> Result<OpOutput, CloudError> {
        let Some(binding) = meta.crud else {
            return Err(CloudError::not_implemented(self.manifest.name, meta.name));
        };
        let res = self
            .resources
            .iter()
            .find(|r| r.type_name == binding.resource)
            .ok_or_else(|| {
                CloudError::internal(format!("undeclared CRUD resource `{}`", binding.resource))
            })?;
        let sb = &ctx.sandbox;
        let ty = res.type_name;
        let sh = &self.shaper;
        let target = sh.target(ctx, res, binding.verb, &input)?;
        let out = match binding.verb {
            CrudVerb::Create => {
                let body = sh.create_body(ctx, res, &target, &input)?;
                let rec = self
                    .engine
                    .create(sb, ty, &target, body)?
                    .ok_or_else(|| sh.already_exists(res, &target))?;
                self.emit(ctx, meta, &rec);
                sh.render(
                    ctx,
                    res,
                    binding.verb,
                    CrudResult::One {
                        record: &rec,
                        created: true,
                    },
                )
            }
            CrudVerb::Upsert => {
                let (rec, created) = match self.engine.get(sb, ty, &target.id) {
                    Some(existing) => {
                        let body = sh.update_body(ctx, res, &existing, &input)?;
                        (self.engine.replace(sb, existing, body)?, false)
                    }
                    None => {
                        let body = sh.create_body(ctx, res, &target, &input)?;
                        match self.engine.create(sb, ty, &target, body)? {
                            Some(r) => (r, true),
                            None => return Err(CloudError::conflict("concurrent create")),
                        }
                    }
                };
                self.emit(ctx, meta, &rec);
                sh.render(
                    ctx,
                    res,
                    binding.verb,
                    CrudResult::One {
                        record: &rec,
                        created,
                    },
                )
            }
            CrudVerb::Read => {
                let rec = self
                    .engine
                    .get(sb, ty, &target.id)
                    .ok_or_else(|| sh.not_found(res, &target))?;
                sh.render(
                    ctx,
                    res,
                    binding.verb,
                    CrudResult::One {
                        record: &rec,
                        created: false,
                    },
                )
            }
            CrudVerb::Update => {
                let existing = self
                    .engine
                    .get(sb, ty, &target.id)
                    .ok_or_else(|| sh.not_found(res, &target))?;
                let body = sh.update_body(ctx, res, &existing, &input)?;
                let rec = self.engine.replace(sb, existing, body)?;
                self.emit(ctx, meta, &rec);
                sh.render(
                    ctx,
                    res,
                    binding.verb,
                    CrudResult::One {
                        record: &rec,
                        created: false,
                    },
                )
            }
            CrudVerb::Delete => {
                let rec = self
                    .engine
                    .delete(sb, ty, &target.id)
                    .ok_or_else(|| sh.not_found(res, &target))?;
                self.emit(ctx, meta, &rec);
                sh.render(ctx, res, binding.verb, CrudResult::Deleted(&rec))
            }
            CrudVerb::List => {
                let recs = self.engine.list(sb, ty, &target.parent);
                sh.render(ctx, res, binding.verb, CrudResult::Many(&recs))
            }
        }?;
        Ok(out)
    }

    fn emit(&self, ctx: &RequestContext, meta: &OpMeta, rec: &CrudRecord) {
        for k in meta.emits {
            self.host.bus.publish(CloudEvent {
                id: self.host.ids.uuid().to_string(),
                source: rec.id.clone(),
                kind: k.0.to_string(),
                time: self.host.clock.now(),
                data: rec.body.clone(),
                sandbox: ctx.sandbox.clone(),
            });
        }
    }
}

#[async_trait::async_trait]
impl ServiceFacade for GenericCrudFacade {
    fn manifest(&self) -> &'static ServiceManifest {
        self.manifest
    }
    fn routes(&self) -> &'static RouteTable {
        self.routes
    }
    fn ops(&self) -> &'static [OpMeta] {
        self.ops
    }
    async fn dispatch(
        &self,
        ctx: &RequestContext,
        op: OpId,
        input: OpInput,
    ) -> Result<OpOutput, CloudError> {
        let meta = self
            .ops
            .get(op.0 as usize)
            .ok_or_else(|| CloudError::internal("unknown op id"))?;
        self.serve(ctx, meta, input).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use mcle_sdk::*;
    use serde_json::json;

    struct Plain;
    impl CrudShaper for Plain {
        fn target(
            &self,
            _c: &RequestContext,
            _r: &CrudResource,
            verb: CrudVerb,
            i: &OpInput,
        ) -> Result<CrudTarget, CloudError> {
            let name = i.lookup("name").unwrap_or_default();
            if verb != CrudVerb::List && name.is_empty() {
                return Err(CloudError::invalid("name required"));
            }
            Ok(CrudTarget {
                id: format!("p/{name}"),
                name,
                parent: "p".into(),
            })
        }
        fn create_body(
            &self,
            _c: &RequestContext,
            _r: &CrudResource,
            _t: &CrudTarget,
            i: &OpInput,
        ) -> Result<Value, CloudError> {
            Ok(i.body.clone())
        }
        fn render(
            &self,
            _c: &RequestContext,
            _r: &CrudResource,
            _v: CrudVerb,
            r: CrudResult<'_>,
        ) -> Result<OpOutput, CloudError> {
            Ok(match r {
                CrudResult::One { record, created } => {
                    OpOutput::json_status(if created { 201 } else { 200 }, record.body.clone())
                }
                CrudResult::Many(rs) => {
                    OpOutput::json(json!(rs.iter().map(|r| r.name.clone()).collect::<Vec<_>>()))
                }
                CrudResult::Deleted(_) => OpOutput::empty(204),
            })
        }
    }

    const fn op(name: &'static str, verb: CrudVerb) -> OpMeta {
        OpMeta {
            name,
            iam_action: IamAction("x:y"),
            resource: ResourceExtractor::Scope,
            level: FidelityLevel::L1,
            lro: LroBehavior::None,
            emits: &[],
            idempotency: Idempotency::None,
            plane: Plane::Control,
            crud: Some(CrudBinding {
                resource: "thing",
                verb,
            }),
        }
    }

    static OPS: [OpMeta; 6] = [
        op("Create", CrudVerb::Create),
        op("Get", CrudVerb::Read),
        op("Update", CrudVerb::Update),
        op("Delete", CrudVerb::Delete),
        op("List", CrudVerb::List),
        OpMeta {
            crud: None,
            ..op("Fancy", CrudVerb::Read)
        },
    ];
    static ROUTES: RouteTable = RouteTable {
        case_insensitive: false,
        routes: &[],
    };
    static RES: [CrudResource; 1] = [CrudResource {
        type_name: "thing",
        hints: &[],
    }];
    static MANIFEST: ServiceManifest = ServiceManifest {
        cloud: CloudId::ext("test"),
        name: "things",
        display: "Things",
        tier: Tier::T2,
        engines: &["crud"],
        protocol: "plain",
        signing_name: "",
    };

    fn ctx() -> RequestContext {
        RequestContext {
            request_id: "r".into(),
            sandbox: SandboxId::default(),
            cloud: CloudId::ext("test"),
            service: "things".into(),
            op: String::new(),
            principal: Principal::anonymous(),
            scope: Scope::Ext {
                cloud: CloudId::ext("test"),
                parts: vec![],
            },
            public_base_url: String::new(),
        }
    }

    fn input(body: Value) -> OpInput {
        OpInput {
            body,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn generic_lifecycle() {
        let host = mcle_core::build_host(&Default::default()).unwrap();
        let f = GenericCrudFacade::new(&host, &MANIFEST, &ROUTES, &OPS, &RES, Arc::new(Plain));
        let c = ctx();
        let out = f
            .dispatch(&c, OpId(0), input(json!({"name": "a", "v": 1})))
            .await
            .unwrap();
        assert_eq!(out.status, 201);
        let err = f
            .dispatch(&c, OpId(0), input(json!({"name": "a"})))
            .await
            .unwrap_err();
        assert_eq!(err.http_status, 409);
        let out = f
            .dispatch(&c, OpId(2), input(json!({"name": "a", "v": 2})))
            .await
            .unwrap();
        assert_eq!(out.body, Body::Json(json!({"name": "a", "v": 2})));
        f.dispatch(&c, OpId(0), input(json!({"name": "b"})))
            .await
            .unwrap();
        let out = f.dispatch(&c, OpId(4), input(Value::Null)).await.unwrap();
        assert_eq!(out.body, Body::Json(json!(["a", "b"])));
        assert_eq!(
            f.dispatch(&c, OpId(3), input(json!({"name": "a"})))
                .await
                .unwrap()
                .status,
            204
        );
        assert_eq!(
            f.dispatch(&c, OpId(1), input(json!({"name": "a"})))
                .await
                .unwrap_err()
                .http_status,
            404
        );
        assert_eq!(
            f.dispatch(&c, OpId(5), input(json!({"name": "a"})))
                .await
                .unwrap_err()
                .http_status,
            501
        );
    }
}
