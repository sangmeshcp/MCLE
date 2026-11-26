use crate::authz::{AuthzRequest, Decision, PolicySet};
use crate::cloud::CloudId;
use crate::context::{Principal, RequestContext};
use crate::crud::CrudShaper;
use crate::error::CloudError;
use crate::event::CloudEvent;
use crate::io::{OpInput, OpOutput, RequestHead};
use crate::lro::LroOperation;
use crate::manifest::ServiceManifest;
use crate::meta::OpMeta;
use crate::route::{OpId, RouteTable};
use crate::scope::Scope;
use crate::SandboxId;
use bytes::Bytes;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

pub use crate::route::{RouteHit, RouteRequest};

#[async_trait::async_trait]
pub trait ServiceFacade: Send + Sync + 'static {
    fn manifest(&self) -> &'static ServiceManifest;
    fn routes(&self) -> &'static RouteTable;
    fn ops(&self) -> &'static [OpMeta];
    fn op_meta(&self, op: OpId) -> &'static OpMeta {
        &self.ops()[op.0 as usize]
    }
    async fn dispatch(
        &self,
        ctx: &RequestContext,
        op: OpId,
        input: OpInput,
    ) -> Result<OpOutput, CloudError>;
}

#[derive(Clone, Copy, Debug)]
pub enum DetectionRule {
    HostSuffix(&'static str),
    HostContains(&'static str),
    AuthPrefix(&'static str),
    PathPrefix(&'static str),
    Header(&'static str),
}

pub trait CloudAdapter: Send + Sync + 'static {
    fn cloud(&self) -> CloudId;
    fn detection_rules(&self) -> &'static [DetectionRule];
    fn is_fallback(&self) -> bool {
        false
    }
    fn route_request<'a>(&self, head: &'a RequestHead, body: &'a Value) -> RouteRequest<'a> {
        let _ = body;
        RouteRequest {
            method: &head.method,
            path: &head.path,
            query_keys: Vec::new(),
            target: head.header("x-amz-target"),
            action: None,
        }
    }
    fn parse_body(&self, head: &RequestHead, raw: &Bytes) -> Result<Value, CloudError> {
        let _ = head;
        if raw.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(raw).or(Ok(Value::Null))
    }
    fn principal(&self, head: &RequestHead) -> Principal;
    fn scope(&self, head: &RequestHead, principal: &Principal, input: &OpInput) -> Scope;
    fn request_id_header(&self) -> &'static str;
    fn render_output(&self, ctx: &RequestContext, protocol: &str, out: OpOutput) -> OpOutput {
        let _ = (ctx, protocol);
        out
    }
    fn render_error(&self, ctx: &RequestContext, protocol: &str, err: &CloudError) -> OpOutput;
    fn lro_renderer(&self) -> Option<Arc<dyn LroRenderer>> {
        None
    }
    fn crud_shaper(&self, protocol: &str) -> Option<Arc<dyn CrudShaper>> {
        let _ = protocol;
        None
    }
    fn env(&self, base_url: &str) -> Vec<(String, String)>;
}

pub trait Engine: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn reset(&self, sandbox: Option<&SandboxId>) {
        let _ = sandbox;
    }
}

#[async_trait::async_trait]
pub trait WireServer: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn default_port(&self) -> u16;
    async fn serve(&self, listener: std::net::TcpListener) -> Result<(), String>;
}

pub trait IdentityProvider: Send + Sync + 'static {
    fn cloud(&self) -> CloudId;
    fn principal(&self, head: &RequestHead) -> Option<Principal>;
}

pub trait AuthzEvaluator: Send + Sync + 'static {
    fn cloud(&self) -> CloudId;
    fn evaluate(&self, req: &AuthzRequest, policies: &PolicySet) -> Decision;
}

pub trait LroRenderer: Send + Sync + 'static {
    fn render(&self, op: &LroOperation, base_url: &str) -> OpOutput;
}

pub trait TriggerSource: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn selects(&self, event: &CloudEvent) -> bool;
}

#[async_trait::async_trait]
pub trait TriggerSink: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn invoke(&self, events: &[CloudEvent]) -> Result<(), String>;
}

#[derive(Clone, Debug, Default)]
pub struct FunctionSpec {
    pub name: String,
    pub runtime: String,
    pub handler: String,
    pub code: String,
    pub env: BTreeMap<String, String>,
    pub timeout_ms: u64,
}

#[async_trait::async_trait]
pub trait FunctionRuntime: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn available(&self) -> bool;
    async fn invoke(&self, f: &FunctionSpec, payload: Bytes) -> Result<Bytes, CloudError>;
}

#[derive(Clone, Debug, Default)]
pub struct ContainerSpec {
    pub image: String,
    pub command: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub labels: BTreeMap<String, String>,
}

#[async_trait::async_trait]
pub trait ExecutionBackend: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn available(&self) -> bool;
    async fn run(&self, spec: ContainerSpec) -> Result<String, String>;
    async fn stop(&self, id: &str) -> Result<(), String>;
}

pub trait PersistenceBackend: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn save(&self, snapshot: &Value) -> Result<(), String>;
    fn load(&self) -> Result<Option<Value>, String>;
}

#[async_trait::async_trait]
pub trait PipelineLayer: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    async fn before(
        &self,
        ctx: &RequestContext,
        meta: &OpMeta,
        input: &OpInput,
    ) -> Result<(), CloudError>;
    async fn after(
        &self,
        ctx: &RequestContext,
        meta: &OpMeta,
        result: &Result<OpOutput, CloudError>,
    ) {
        let _ = (ctx, meta, result);
    }
}
