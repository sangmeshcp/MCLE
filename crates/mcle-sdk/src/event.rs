use crate::context::SandboxId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EventKind(pub &'static str);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudEvent {
    pub id: String,
    pub source: String,
    pub kind: String,
    #[serde(with = "time::serde::rfc3339")]
    pub time: OffsetDateTime,
    pub data: Value,
    pub sandbox: SandboxId,
}
