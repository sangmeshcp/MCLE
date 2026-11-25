#![forbid(unsafe_code)]

pub mod bus;
pub mod clock;
pub mod coverage;
pub mod faults;
pub mod ids;
pub mod lro;
pub mod sandbox;
pub mod store;

use mcle_sdk::host::HostConfig;
use mcle_sdk::Host;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug)]
pub struct CoreOptions {
    pub seed: u64,
    pub clock: String,
    pub lro_delay_ms: u64,
    pub config: HostConfig,
}

impl Default for CoreOptions {
    fn default() -> Self {
        CoreOptions {
            seed: 0,
            clock: "real".into(),
            lro_delay_ms: 0,
            config: HostConfig::default(),
        }
    }
}

pub fn build_host(opts: &CoreOptions) -> Result<Host, String> {
    let clock = clock::from_spec(&opts.clock)?;
    let ids: Arc<dyn mcle_sdk::IdGen> = Arc::new(ids::SeededIds::new(opts.seed));
    let state: Arc<dyn mcle_sdk::StateStore> = Arc::new(store::MemoryStateStore::default());
    let bus: Arc<dyn mcle_sdk::EventBus> = Arc::new(bus::InMemoryBus::new(3));
    let lro: Arc<dyn mcle_sdk::LroManager> = Arc::new(lro::Lros::new(
        state.clone(),
        clock.clone(),
        ids.clone(),
        opts.lro_delay_ms,
    ));
    Ok(Host {
        clock,
        ids,
        state,
        bus,
        lro,
        config: Arc::new(RwLock::new(opts.config.clone())),
    })
}
