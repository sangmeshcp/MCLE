use mcle_sdk::SandboxId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::RwLock;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sandbox {
    pub id: SandboxId,
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires: Option<OffsetDateTime>,
    pub access_key: String,
}

#[derive(Debug, Default)]
pub struct SandboxRegistry {
    inner: RwLock<BTreeMap<String, Sandbox>>,
}

pub const ACCESS_KEY_PREFIX: &str = "MCLESBX";

impl SandboxRegistry {
    pub fn create(&self, id: String, now: OffsetDateTime, ttl_secs: Option<u64>) -> Sandbox {
        let sb = Sandbox {
            access_key: format!("{ACCESS_KEY_PREFIX}{}", id.to_ascii_uppercase()),
            id: SandboxId(id.clone()),
            created: now,
            expires: ttl_secs.map(|s| now + time::Duration::seconds(s as i64)),
        };
        if let Ok(mut g) = self.inner.write() {
            g.insert(id, sb.clone());
        }
        sb
    }

    pub fn get(&self, id: &str) -> Option<Sandbox> {
        self.inner.read().ok()?.get(id).cloned()
    }

    pub fn by_access_key(&self, key: &str) -> Option<SandboxId> {
        if !key.starts_with(ACCESS_KEY_PREFIX) {
            return None;
        }
        self.inner
            .read()
            .ok()?
            .values()
            .find(|s| s.access_key == key)
            .map(|s| s.id.clone())
    }

    pub fn remove(&self, id: &str) -> Option<Sandbox> {
        self.inner.write().ok()?.remove(id)
    }

    pub fn list(&self) -> Vec<Sandbox> {
        self.inner
            .read()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn expire(&self, now: OffsetDateTime) -> Vec<SandboxId> {
        let Ok(mut g) = self.inner.write() else {
            return Vec::new();
        };
        let dead: Vec<String> = g
            .values()
            .filter(|s| s.expires.is_some_and(|e| e <= now))
            .map(|s| s.id.0.clone())
            .collect();
        dead.iter()
            .filter_map(|id| g.remove(id))
            .map(|s| s.id)
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle() {
        let r = SandboxRegistry::default();
        let t = OffsetDateTime::UNIX_EPOCH;
        let s = r.create("abc".into(), t, Some(10));
        assert_eq!(
            r.by_access_key(&s.access_key),
            Some(SandboxId("abc".into()))
        );
        assert!(r.by_access_key("AKIAEXAMPLE").is_none());
        assert!(r.expire(t + time::Duration::seconds(5)).is_empty());
        assert_eq!(r.expire(t + time::Duration::seconds(10)).len(), 1);
        assert!(r.get("abc").is_none());
    }
}
