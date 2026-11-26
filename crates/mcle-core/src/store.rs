use mcle_sdk::host::StatePartition;
use mcle_sdk::{SandboxId, StateStore};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;

type Key = (String, SandboxId);

#[derive(Debug, Default)]
pub struct MemoryStateStore {
    parts: RwLock<HashMap<Key, BTreeMap<String, Value>>>,
}

impl MemoryStateStore {
    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<Key, BTreeMap<String, Value>>> {
        match self.parts.read() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<Key, BTreeMap<String, Value>>> {
        match self.parts.write() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }
}

impl StateStore for MemoryStateStore {
    fn get(&self, ns: &str, sb: &SandboxId, key: &str) -> Option<Value> {
        self.read()
            .get(&(ns.to_string(), sb.clone()))
            .and_then(|m| m.get(key).cloned())
    }

    fn put(&self, ns: &str, sb: &SandboxId, key: &str, value: Value) -> Option<Value> {
        self.write()
            .entry((ns.to_string(), sb.clone()))
            .or_default()
            .insert(key.to_string(), value)
    }

    fn insert_new(&self, ns: &str, sb: &SandboxId, key: &str, value: Value) -> bool {
        let mut g = self.write();
        let m = g.entry((ns.to_string(), sb.clone())).or_default();
        if m.contains_key(key) {
            false
        } else {
            m.insert(key.to_string(), value);
            true
        }
    }

    fn delete(&self, ns: &str, sb: &SandboxId, key: &str) -> Option<Value> {
        self.write()
            .get_mut(&(ns.to_string(), sb.clone()))
            .and_then(|m| m.remove(key))
    }

    fn list(&self, ns: &str, sb: &SandboxId, prefix: &str) -> Vec<(String, Value)> {
        self.read()
            .get(&(ns.to_string(), sb.clone()))
            .map(|m| {
                m.range(prefix.to_string()..)
                    .take_while(|(k, _)| k.starts_with(prefix))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn reset(&self, sandbox: Option<&SandboxId>) {
        let mut g = self.write();
        match sandbox {
            Some(sb) => g.retain(|(_, s), _| s != sb),
            None => g.clear(),
        }
    }

    fn export(&self) -> Vec<StatePartition> {
        let mut out: Vec<StatePartition> = self
            .read()
            .iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|((ns, sb), m)| StatePartition {
                ns: ns.clone(),
                sandbox: sb.clone(),
                entries: m.clone(),
            })
            .collect();
        out.sort_by(|a, b| (&a.ns, &a.sandbox).cmp(&(&b.ns, &b.sandbox)));
        out
    }

    fn import(&self, parts: Vec<StatePartition>) {
        let mut g = self.write();
        g.clear();
        for p in parts {
            g.insert((p.ns, p.sandbox), p.entries);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn crud_prefix_and_isolation() {
        let s = MemoryStateStore::default();
        let a = SandboxId("a".into());
        let b = SandboxId("b".into());
        assert!(s.insert_new("ns", &a, "x/1", json!(1)));
        assert!(!s.insert_new("ns", &a, "x/1", json!(2)));
        s.put("ns", &a, "x/2", json!(2));
        s.put("ns", &a, "y/1", json!(3));
        s.put("ns", &b, "x/1", json!(9));
        let l = s.list("ns", &a, "x/");
        assert_eq!(l.len(), 2);
        assert_eq!(s.get("ns", &b, "x/1"), Some(json!(9)));
        let snap = s.export();
        s.reset(Some(&a));
        assert!(s.get("ns", &a, "x/1").is_none());
        assert!(s.get("ns", &b, "x/1").is_some());
        s.import(snap);
        assert_eq!(s.get("ns", &a, "x/1"), Some(json!(1)));
    }
}
