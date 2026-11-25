use crate::cloud::CloudId;
use crate::context::SandboxId;
use crate::error::CloudError;
use crate::event::CloudEvent;
use crate::lro::LroOperation;
use crate::meta::LroBehavior;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
    fn advance(&self, _by: time::Duration) {}
}

pub trait IdGen: Send + Sync {
    fn next_u64(&self) -> u64;

    fn uuid(&self) -> Uuid {
        let hi = self.next_u64().to_be_bytes();
        let lo = self.next_u64().to_be_bytes();
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&hi);
        b[8..].copy_from_slice(&lo);
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        Uuid::from_bytes(b)
    }

    fn hex(&self, n: usize) -> String {
        self.pick_alphabet(n, b"0123456789abcdef")
    }

    fn alnum(&self, n: usize) -> String {
        self.pick_alphabet(
            n,
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
        )
    }

    fn pick_alphabet(&self, n: usize, alphabet: &[u8]) -> String {
        (0..n)
            .map(|_| alphabet[(self.next_u64() % alphabet.len() as u64) as usize] as char)
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StatePartition {
    pub ns: String,
    pub sandbox: SandboxId,
    pub entries: BTreeMap<String, Value>,
}

pub trait StateStore: Send + Sync {
    fn get(&self, ns: &str, sandbox: &SandboxId, key: &str) -> Option<Value>;
    fn put(&self, ns: &str, sandbox: &SandboxId, key: &str, value: Value) -> Option<Value>;
    fn insert_new(&self, ns: &str, sandbox: &SandboxId, key: &str, value: Value) -> bool;
    fn delete(&self, ns: &str, sandbox: &SandboxId, key: &str) -> Option<Value>;
    fn list(&self, ns: &str, sandbox: &SandboxId, prefix: &str) -> Vec<(String, Value)>;
    fn reset(&self, sandbox: Option<&SandboxId>);
    fn export(&self) -> Vec<StatePartition>;
    fn import(&self, parts: Vec<StatePartition>);
}

// Typed view over one namespace of a StateStore.
pub struct Store<T> {
    inner: Arc<dyn StateStore>,
    ns: &'static str,
    _t: PhantomData<fn() -> T>,
}

impl<T> Clone for Store<T> {
    fn clone(&self) -> Self {
        Store {
            inner: self.inner.clone(),
            ns: self.ns,
            _t: PhantomData,
        }
    }
}

impl<T: Serialize + DeserializeOwned> Store<T> {
    pub fn new(inner: Arc<dyn StateStore>, ns: &'static str) -> Self {
        Store {
            inner,
            ns,
            _t: PhantomData,
        }
    }
    pub fn get(&self, sb: &SandboxId, key: &str) -> Option<T> {
        self.inner
            .get(self.ns, sb, key)
            .and_then(|v| serde_json::from_value(v).ok())
    }
    pub fn put(&self, sb: &SandboxId, key: &str, v: &T) -> Result<(), CloudError> {
        let v = serde_json::to_value(v).map_err(|e| CloudError::internal(e.to_string()))?;
        self.inner.put(self.ns, sb, key, v);
        Ok(())
    }
    pub fn insert_new(&self, sb: &SandboxId, key: &str, v: &T) -> Result<bool, CloudError> {
        let v = serde_json::to_value(v).map_err(|e| CloudError::internal(e.to_string()))?;
        Ok(self.inner.insert_new(self.ns, sb, key, v))
    }
    pub fn delete(&self, sb: &SandboxId, key: &str) -> Option<T> {
        self.inner
            .delete(self.ns, sb, key)
            .and_then(|v| serde_json::from_value(v).ok())
    }
    pub fn list(&self, sb: &SandboxId, prefix: &str) -> Vec<(String, T)> {
        self.inner
            .list(self.ns, sb, prefix)
            .into_iter()
            .filter_map(|(k, v)| serde_json::from_value(v).ok().map(|t| (k, t)))
            .collect()
    }
}

#[async_trait::async_trait]
pub trait EventSubscriber: Send + Sync {
    async fn deliver(&self, event: &CloudEvent) -> Result<(), String>;
}

#[derive(Clone, Debug, Default)]
pub struct EventFilter {
    pub kind: Option<String>,
    pub source_glob: Option<String>,
}

pub trait EventBus: Send + Sync {
    fn publish(&self, event: CloudEvent);
    fn subscribe(&self, filter: EventFilter, sub: Arc<dyn EventSubscriber>) -> u64;
    fn unsubscribe(&self, id: u64);
    fn dead_letters(&self) -> Vec<CloudEvent>;
}

pub trait LroManager: Send + Sync {
    fn start(
        &self,
        sandbox: &SandboxId,
        cloud: CloudId,
        behavior: LroBehavior,
        resource: Option<String>,
        result: Value,
    ) -> LroOperation;
    fn get(&self, sandbox: &SandboxId, id: &str) -> Option<LroOperation>;
    fn list(&self, sandbox: &SandboxId) -> Vec<LroOperation>;
    fn cancel(&self, sandbox: &SandboxId, id: &str) -> Option<LroOperation>;
    fn fail(&self, sandbox: &SandboxId, id: &str, error: CloudError) -> Option<LroOperation>;
    fn delay_ms(&self) -> u64;
}

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub public_base_url: String,
    pub aws_account: String,
    pub default_region: String,
}

impl Default for HostConfig {
    fn default() -> Self {
        HostConfig {
            public_base_url: "http://localhost:4700".into(),
            aws_account: "000000000000".into(),
            default_region: "us-east-1".into(),
        }
    }
}

#[derive(Clone)]
pub struct Host {
    pub clock: Arc<dyn Clock>,
    pub ids: Arc<dyn IdGen>,
    pub state: Arc<dyn StateStore>,
    pub bus: Arc<dyn EventBus>,
    pub lro: Arc<dyn LroManager>,
    pub config: Arc<std::sync::RwLock<HostConfig>>,
}

impl Host {
    pub fn config(&self) -> HostConfig {
        match self.config.read() {
            Ok(c) => c.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }
}
