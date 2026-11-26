#![forbid(unsafe_code)]

#[path = "../gen/mod.rs"]
pub mod gen;

use mcle_engine_crud::{GenericCrudFacade, NoShaper};
use mcle_sdk::{CloudAdapter, Host, ServiceFacade};
use std::sync::Arc;

pub fn facade(host: &Host, adapter: &dyn CloudAdapter) -> Arc<dyn ServiceFacade> {
    let shaper = adapter
        .crud_shaper(gen::MANIFEST.protocol)
        .unwrap_or_else(|| Arc::new(NoShaper));
    Arc::new(GenericCrudFacade::new(
        host,
        &gen::MANIFEST,
        &gen::ROUTES,
        &gen::OPS,
        &gen::CRUD_RESOURCES,
        shaper,
    ))
}
