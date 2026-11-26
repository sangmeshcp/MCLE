#![forbid(unsafe_code)]

#[path = "../gen/mod.rs"]
pub mod gen;

use mcle_engine_crud::{GenericCrudFacade, NoShaper};
use mcle_sdk::{
    CloudAdapter, CloudError, Host, OpId, OpInput, OpMeta, OpOutput, RequestContext, RouteTable,
    ServiceFacade, ServiceManifest,
};
use serde_json::Value;
use std::sync::Arc;

pub struct ObjectStorage {
    generic: GenericCrudFacade,
}

pub fn facade(host: &Host, adapter: &dyn CloudAdapter) -> Arc<dyn ServiceFacade> {
    let shaper = adapter
        .crud_shaper(gen::MANIFEST.protocol)
        .unwrap_or_else(|| Arc::new(NoShaper));
    Arc::new(ObjectStorage {
        generic: GenericCrudFacade::new(
            host,
            &gen::MANIFEST,
            &gen::ROUTES,
            &gen::OPS,
            &gen::CRUD_RESOURCES,
            shaper,
        ),
    })
}

#[async_trait::async_trait]
impl ServiceFacade for ObjectStorage {
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
        if op == gen::op::GET_NAMESPACE {
            return Ok(OpOutput::json(Value::String(
                mcle_cloud_oci::DEFAULT_NAMESPACE.into(),
            )));
        }
        self.generic.dispatch(ctx, op, input).await
    }
}
