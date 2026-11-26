use mcle_sdk::util::glob_match;
use mcle_sdk::CloudError;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaultError {
    pub code: String,
    #[serde(default = "default_status")]
    pub status: u16,
    #[serde(default)]
    pub message: String,
}

fn default_status() -> u16 {
    500
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FaultRule {
    #[serde(default)]
    pub cloud: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub op: Option<String>,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub principal: Option<String>,
    #[serde(default)]
    pub error: Option<FaultError>,
    #[serde(default)]
    pub latency_ms: u64,
    #[serde(default)]
    pub times: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaultAction {
    pub error: Option<CloudError>,
    pub latency_ms: u64,
    pub label: String,
}

#[derive(Clone, Copy, Debug)]
pub struct FaultKey<'a> {
    pub cloud: &'a str,
    pub service: &'a str,
    pub op: &'a str,
    pub resource: &'a str,
    pub principal: &'a str,
}

#[derive(Debug, Default)]
pub struct FaultSet {
    rules: RwLock<Vec<FaultRule>>,
}

fn m(p: &Option<String>, v: &str) -> bool {
    p.as_ref().is_none_or(|p| glob_match(p, v))
}

impl FaultSet {
    pub fn set(&self, rules: Vec<FaultRule>) {
        if let Ok(mut g) = self.rules.write() {
            *g = rules;
        }
    }
    pub fn add(&self, rules: Vec<FaultRule>) {
        if let Ok(mut g) = self.rules.write() {
            g.extend(rules);
        }
    }
    pub fn list(&self) -> Vec<FaultRule> {
        self.rules.read().map(|g| g.clone()).unwrap_or_default()
    }
    pub fn check(&self, k: FaultKey<'_>) -> Option<FaultAction> {
        let mut g = self.rules.write().ok()?;
        let idx = g.iter().position(|r| {
            r.times != Some(0)
                && m(&r.cloud, k.cloud)
                && m(&r.service, k.service)
                && m(&r.op, k.op)
                && m(&r.resource, k.resource)
                && m(&r.principal, k.principal)
        })?;
        let r = &mut g[idx];
        if let Some(t) = r.times.as_mut() {
            *t -= 1;
        }
        Some(FaultAction {
            error: r.error.as_ref().map(|e| {
                let kind = match e.status {
                    429 => mcle_sdk::error::ErrorKind::Throttled,
                    403 => mcle_sdk::error::ErrorKind::AccessDenied,
                    404 => mcle_sdk::error::ErrorKind::NotFound,
                    400 => mcle_sdk::error::ErrorKind::InvalidArgument,
                    409 => mcle_sdk::error::ErrorKind::Conflict,
                    _ => mcle_sdk::error::ErrorKind::Internal,
                };
                let msg = if e.message.is_empty() {
                    format!("MCLE injected fault: {}", e.code)
                } else {
                    e.message.clone()
                };
                CloudError::new(kind, msg)
                    .with_code(e.code.clone())
                    .with_status(e.status)
            }),
            latency_ms: r.latency_ms,
            label: format!(
                "{}/{}/{}",
                r.cloud.as_deref().unwrap_or("*"),
                r.service.as_deref().unwrap_or("*"),
                r.op.as_deref().unwrap_or("*")
            ),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn match_and_consume() {
        let f = FaultSet::default();
        f.set(vec![FaultRule {
            cloud: Some("aws".into()),
            op: Some("Get*".into()),
            error: Some(FaultError {
                code: "ThrottlingException".into(),
                status: 429,
                message: String::new(),
            }),
            times: Some(1),
            ..Default::default()
        }]);
        let k = FaultKey {
            cloud: "aws",
            service: "s",
            op: "GetSecretValue",
            resource: "",
            principal: "",
        };
        let a = f.check(k).unwrap();
        assert_eq!(a.error.unwrap().http_status, 429);
        assert!(f.check(k).is_none());
        let k2 = FaultKey { op: "PutX", ..k };
        assert!(f.check(k2).is_none());
    }
}
