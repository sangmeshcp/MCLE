#![forbid(unsafe_code)]

use mcle_sdk::host::StatePartition;
use mcle_sdk::{PersistenceBackend, StateStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const FORMAT: &str = "mcle-state/v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: String,
    pub mcle_version: String,
    pub partitions: Vec<StatePartition>,
}

#[derive(Debug)]
pub enum PersistError {
    Format(String),
    Json(serde_json::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersistError::Format(s) => write!(f, "unsupported snapshot format `{s}`"),
            PersistError::Json(e) => write!(f, "snapshot json: {e}"),
            PersistError::Io(e) => write!(f, "snapshot io: {e}"),
        }
    }
}

impl std::error::Error for PersistError {}

pub fn export(store: &dyn StateStore) -> Snapshot {
    Snapshot {
        format: FORMAT.into(),
        mcle_version: env!("CARGO_PKG_VERSION").into(),
        partitions: store.export(),
    }
}

pub fn parse(v: Value) -> Result<Snapshot, PersistError> {
    let fmt = v
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if fmt != FORMAT {
        return Err(PersistError::Format(fmt));
    }
    serde_json::from_value(v).map_err(PersistError::Json)
}

pub fn import(store: &dyn StateStore, snap: Snapshot) {
    store.import(snap.partitions);
}

#[derive(Clone, Debug)]
pub struct FileBackend {
    pub path: PathBuf,
}

impl PersistenceBackend for FileBackend {
    fn name(&self) -> &'static str {
        "file"
    }

    fn save(&self, snapshot: &Value) -> Result<(), String> {
        let tmp = self.path.with_extension("tmp");
        let data = serde_json::to_vec_pretty(snapshot).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    fn load(&self) -> Result<Option<Value>, String> {
        match std::fs::read(&self.path) {
            Ok(b) => serde_json::from_slice(&b)
                .map(Some)
                .map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use mcle_core::store::MemoryStateStore;
    use mcle_sdk::SandboxId;
    use serde_json::json;

    #[test]
    fn round_trip_through_file() {
        let s = MemoryStateStore::default();
        s.put("crud", &SandboxId::default(), "k", json!({"a": 1}));
        let snap = export(&s);
        let dir = std::env::temp_dir().join(format!("mcle-persist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fb = FileBackend {
            path: dir.join("state.json"),
        };
        fb.save(&serde_json::to_value(&snap).unwrap()).unwrap();
        let loaded = parse(fb.load().unwrap().unwrap()).unwrap();
        let s2 = MemoryStateStore::default();
        import(&s2, loaded);
        assert_eq!(
            s2.get("crud", &SandboxId::default(), "k"),
            Some(json!({"a": 1}))
        );
        assert!(parse(json!({"format": "v0"})).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
