#![forbid(unsafe_code)]

use bytes::Bytes;
use mcle_sdk::traits::FunctionSpec;
use mcle_sdk::{CloudError, FunctionRuntime};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionsMode {
    Auto,
    Container,
    Process,
    Mock,
}

impl FunctionsMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "container" => Some(Self::Container),
            "process" => Some(Self::Process),
            "mock" => Some(Self::Mock),
            _ => None,
        }
    }

    pub fn resolve(self, container_available: bool) -> Self {
        match self {
            Self::Auto if container_available => Self::Container,
            Self::Auto => Self::Process,
            m => m,
        }
    }
}

#[derive(Debug, Default)]
pub struct MockRuntime {
    canned: RwLock<HashMap<String, Bytes>>,
}

impl MockRuntime {
    pub fn set_response(&self, function: &str, body: Bytes) {
        if let Ok(mut g) = self.canned.write() {
            g.insert(function.to_string(), body);
        }
    }
}

#[async_trait::async_trait]
impl FunctionRuntime for MockRuntime {
    fn name(&self) -> &'static str {
        "mock"
    }
    fn available(&self) -> bool {
        true
    }
    async fn invoke(&self, f: &FunctionSpec, payload: Bytes) -> Result<Bytes, CloudError> {
        Ok(self
            .canned
            .read()
            .ok()
            .and_then(|g| g.get(&f.name).cloned())
            .unwrap_or(payload))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn resolve_modes() {
        assert_eq!(FunctionsMode::Auto.resolve(false), FunctionsMode::Process);
        assert_eq!(FunctionsMode::Auto.resolve(true), FunctionsMode::Container);
        assert_eq!(FunctionsMode::Mock.resolve(true), FunctionsMode::Mock);
    }

    #[tokio::test]
    async fn mock_echoes_or_cans() {
        let rt = MockRuntime::default();
        let f = FunctionSpec {
            name: "f".into(),
            ..Default::default()
        };
        assert_eq!(
            rt.invoke(&f, Bytes::from_static(b"hi")).await.unwrap(),
            Bytes::from_static(b"hi")
        );
        rt.set_response("f", Bytes::from_static(b"canned"));
        assert_eq!(
            rt.invoke(&f, Bytes::new()).await.unwrap(),
            Bytes::from_static(b"canned")
        );
    }
}
