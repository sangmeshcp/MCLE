use mcle_sdk::{
    Clock, CloudError, CloudId, IdGen, LroBehavior, LroManager, LroOperation, LroState, SandboxId,
    StateStore, Store,
};
use serde_json::Value;
use std::sync::Arc;

pub struct Lros {
    store: Store<LroOperation>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    delay_ms: u64,
}

impl Lros {
    pub fn new(
        state: Arc<dyn StateStore>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGen>,
        delay_ms: u64,
    ) -> Self {
        Lros {
            store: Store::new(state, "_lro"),
            clock,
            ids,
            delay_ms,
        }
    }

    // State is derived lazily from the clock: Pending until created, Running
    // until ready_at, then Succeeded. Terminal states are never changed.
    fn advance(&self, mut op: LroOperation) -> LroOperation {
        if !op.state.is_terminal() {
            let now = self.clock.now();
            op.state = if now >= op.ready_at {
                LroState::Succeeded
            } else if now > op.created {
                LroState::Running
            } else {
                LroState::Pending
            };
        }
        op
    }
}

impl LroManager for Lros {
    fn start(
        &self,
        sandbox: &SandboxId,
        cloud: CloudId,
        behavior: LroBehavior,
        resource: Option<String>,
        result: Value,
    ) -> LroOperation {
        let now = self.clock.now();
        let op = LroOperation {
            id: self.ids.uuid().to_string(),
            cloud,
            sandbox: sandbox.clone(),
            behavior,
            resource,
            state: if self.delay_ms == 0 {
                LroState::Succeeded
            } else {
                LroState::Pending
            },
            created: now,
            ready_at: now + time::Duration::milliseconds(self.delay_ms as i64),
            result,
            error: None,
        };
        let _ = self.store.put(sandbox, &op.id, &op);
        op
    }

    fn get(&self, sandbox: &SandboxId, id: &str) -> Option<LroOperation> {
        let op = self.advance(self.store.get(sandbox, id)?);
        let _ = self.store.put(sandbox, id, &op);
        Some(op)
    }

    fn list(&self, sandbox: &SandboxId) -> Vec<LroOperation> {
        self.store
            .list(sandbox, "")
            .into_iter()
            .map(|(_, op)| self.advance(op))
            .collect()
    }

    fn cancel(&self, sandbox: &SandboxId, id: &str) -> Option<LroOperation> {
        let mut op = self.get(sandbox, id)?;
        if !op.state.is_terminal() {
            op.state = LroState::Cancelled;
            let _ = self.store.put(sandbox, id, &op);
        }
        Some(op)
    }

    fn fail(&self, sandbox: &SandboxId, id: &str, error: CloudError) -> Option<LroOperation> {
        let mut op = self.store.get(sandbox, id)?;
        op.state = LroState::Failed;
        op.error = Some(error);
        let _ = self.store.put(sandbox, id, &op);
        Some(op)
    }

    fn delay_ms(&self) -> u64 {
        self.delay_ms
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use crate::ids::SeededIds;
    use crate::store::MemoryStateStore;

    #[test]
    fn completes_after_delay_by_clock() {
        let clock = Arc::new(ManualClock::new(time::OffsetDateTime::UNIX_EPOCH));
        let l = Lros::new(
            Arc::new(MemoryStateStore::default()),
            clock.clone(),
            Arc::new(SeededIds::new(1)),
            1000,
        );
        let sb = SandboxId::default();
        let op = l.start(&sb, CloudId::AZURE, LroBehavior::Create, None, Value::Null);
        assert_eq!(l.get(&sb, &op.id).unwrap().state, LroState::Pending);
        clock.advance(time::Duration::milliseconds(500));
        assert_eq!(l.get(&sb, &op.id).unwrap().state, LroState::Running);
        clock.advance(time::Duration::milliseconds(500));
        assert_eq!(l.get(&sb, &op.id).unwrap().state, LroState::Succeeded);
        let op2 = l.start(&sb, CloudId::GCP, LroBehavior::Delete, None, Value::Null);
        assert_eq!(l.cancel(&sb, &op2.id).unwrap().state, LroState::Cancelled);
        assert_eq!(l.list(&sb).len(), 2);
    }

    #[test]
    fn zero_delay_is_immediate() {
        let l = Lros::new(
            Arc::new(MemoryStateStore::default()),
            Arc::new(crate::clock::SystemClock),
            Arc::new(SeededIds::new(1)),
            0,
        );
        let op = l.start(
            &SandboxId::default(),
            CloudId::OCI,
            LroBehavior::Create,
            None,
            Value::Null,
        );
        assert_eq!(op.state, LroState::Succeeded);
    }
}
