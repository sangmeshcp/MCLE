use mcle_sdk::host::EventFilter;
use mcle_sdk::util::glob_match;
use mcle_sdk::{CloudEvent, EventBus, EventSubscriber};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

struct Sub {
    id: u64,
    filter: EventFilter,
    sub: Arc<dyn EventSubscriber>,
}

pub struct InMemoryBus {
    subs: Mutex<Vec<Arc<Sub>>>,
    dlq: Arc<Mutex<Vec<CloudEvent>>>,
    next: AtomicU64,
    max_attempts: u32,
}

impl InMemoryBus {
    pub fn new(max_attempts: u32) -> Self {
        InMemoryBus {
            subs: Mutex::new(Vec::new()),
            dlq: Arc::new(Mutex::new(Vec::new())),
            next: AtomicU64::new(1),
            max_attempts: max_attempts.max(1),
        }
    }

    fn subs(&self) -> Vec<Arc<Sub>> {
        match self.subs.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }
}

fn wants(f: &EventFilter, e: &CloudEvent) -> bool {
    f.kind.as_ref().is_none_or(|k| k == &e.kind)
        && f.source_glob
            .as_ref()
            .is_none_or(|g| glob_match(g, &e.source))
}

impl EventBus for InMemoryBus {
    fn publish(&self, event: CloudEvent) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::warn!("event published outside a runtime; dropped");
            return;
        };
        for s in self.subs().into_iter().filter(|s| wants(&s.filter, &event)) {
            let ev = event.clone();
            let dlq = self.dlq.clone();
            let max = self.max_attempts;
            handle.spawn(async move {
                for attempt in 1..=max {
                    match s.sub.deliver(&ev).await {
                        Ok(()) => return,
                        Err(e) => {
                            tracing::debug!(sub = s.id, attempt, error = %e, "delivery failed");
                            if attempt < max {
                                tokio::time::sleep(std::time::Duration::from_millis(
                                    10 * u64::from(attempt),
                                ))
                                .await;
                            }
                        }
                    }
                }
                if let Ok(mut d) = dlq.lock() {
                    d.push(ev);
                }
            });
        }
    }

    fn subscribe(&self, filter: EventFilter, sub: Arc<dyn EventSubscriber>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut g) = self.subs.lock() {
            g.push(Arc::new(Sub { id, filter, sub }));
        }
        id
    }

    fn unsubscribe(&self, id: u64) {
        if let Ok(mut g) = self.subs.lock() {
            g.retain(|s| s.id != id);
        }
    }

    fn dead_letters(&self) -> Vec<CloudEvent> {
        match self.dlq.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use mcle_sdk::SandboxId;
    use std::sync::atomic::AtomicU32;

    struct Flaky {
        fail_first: u32,
        calls: AtomicU32,
    }

    #[async_trait::async_trait]
    impl EventSubscriber for Flaky {
        async fn deliver(&self, _e: &CloudEvent) -> Result<(), String> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_first {
                Err("nope".into())
            } else {
                Ok(())
            }
        }
    }

    fn ev(kind: &str) -> CloudEvent {
        CloudEvent {
            id: "1".into(),
            source: "arn:aws:s3:::b".into(),
            kind: kind.into(),
            time: time::OffsetDateTime::UNIX_EPOCH,
            data: serde_json::Value::Null,
            sandbox: SandboxId::default(),
        }
    }

    #[tokio::test]
    async fn retries_then_dlq() {
        let bus = InMemoryBus::new(3);
        let ok = Arc::new(Flaky {
            fail_first: 2,
            calls: AtomicU32::new(0),
        });
        let bad = Arc::new(Flaky {
            fail_first: 99,
            calls: AtomicU32::new(0),
        });
        bus.subscribe(
            EventFilter {
                kind: Some("ObjectCreated".into()),
                source_glob: None,
            },
            ok.clone(),
        );
        bus.subscribe(
            EventFilter {
                kind: None,
                source_glob: Some("arn:aws:s3:::*".into()),
            },
            bad.clone(),
        );
        bus.publish(ev("ObjectCreated"));
        bus.publish(ev("Other"));
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(ok.calls.load(Ordering::SeqCst), 3);
        assert_eq!(bad.calls.load(Ordering::SeqCst), 6);
        assert_eq!(bus.dead_letters().len(), 2);
    }
}
