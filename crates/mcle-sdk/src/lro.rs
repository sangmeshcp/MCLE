use crate::cloud::CloudId;
use crate::context::SandboxId;
use crate::error::CloudError;
use crate::meta::LroBehavior;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LroState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl LroState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            LroState::Succeeded | LroState::Failed | LroState::Cancelled
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LroOperation {
    pub id: String,
    pub cloud: CloudId,
    pub sandbox: SandboxId,
    pub behavior: LroBehavior,
    pub resource: Option<String>,
    pub state: LroState,
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub ready_at: OffsetDateTime,
    pub result: Value,
    pub error: Option<CloudError>,
}
