#![forbid(unsafe_code)]

pub mod authz;
pub mod cloud;
pub mod context;
pub mod crud;
pub mod error;
pub mod event;
pub mod host;
pub mod io;
pub mod lro;
pub mod manifest;
pub mod meta;
pub mod resource_id;
pub mod route;
pub mod scope;
pub mod traits;
pub mod util;

pub use authz::{AuthzRequest, Decision, IamMode, PolicySet};
pub use cloud::CloudId;
pub use context::{Principal, RequestContext, SandboxId};
pub use crud::{CrudRecord, CrudResource, CrudShaper, CrudTarget};
pub use error::CloudError;
pub use event::{CloudEvent, EventKind};
pub use host::{Clock, EventBus, EventSubscriber, Host, IdGen, LroManager, StateStore, Store};
pub use io::{Body, OpInput, OpOutput};
pub use lro::{LroOperation, LroState};
pub use manifest::{ServiceManifest, Tier};
pub use meta::{
    CrudBinding, CrudVerb, FidelityLevel, IamAction, Idempotency, LroBehavior, OpMeta, Plane,
    ResourceExtractor,
};
pub use resource_id::ResourceId;
pub use route::{OpId, Route, RouteMatch, RouteTable};
pub use scope::Scope;
pub use traits::*;

pub const SDK_VERSION: &str = "0.1.0";
