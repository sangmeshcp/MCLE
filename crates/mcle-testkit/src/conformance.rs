use crate::{TestResponse, TestServer};
use mcle_sdk::{FidelityLevel, ServiceFacade};
use serde_json::Value;

/// Drives one CRUD resource type through a service's native wire shapes.
/// `name` is the short resource name; implementations build native ids from it.
#[async_trait::async_trait]
pub trait CrudDriver: Send + Sync {
    async fn create(&self, s: &TestServer, name: &str) -> TestResponse;
    async fn read(&self, s: &TestServer, name: &str) -> TestResponse;
    async fn update(&self, s: &TestServer, name: &str) -> TestResponse;
    async fn delete(&self, s: &TestServer, name: &str) -> TestResponse;
    async fn list(&self, s: &TestServer) -> TestResponse;
    /// Names present in a list response.
    fn listed(&self, body: &Value) -> Vec<String>;
    /// Status a second delete of the same resource must return (404, or 204 for ARM).
    fn redelete_status(&self) -> u16 {
        404
    }
    /// False where the API allows it (ARM PUT is an upsert; IBM allows duplicate names).
    fn conflict_on_duplicate(&self) -> bool {
        true
    }
    async fn setup(&self, _s: &TestServer) {}
}

fn ok(r: &TestResponse, what: &str) {
    assert!(
        (200..300).contains(&r.status),
        "{what}: expected 2xx, got {} {}",
        r.status,
        r.text()
    );
}

/// Lifecycle invariants every L1 resource must satisfy.
pub async fn crud(s: &TestServer, d: &dyn CrudDriver) {
    let (a, b) = ("conf-a", "conf-b");
    d.setup(s).await;
    ok(&d.create(s, a).await, "create");
    if d.conflict_on_duplicate() {
        let dup = d.create(s, a).await;
        assert_eq!(
            dup.status,
            409,
            "duplicate create must conflict: {}",
            dup.text()
        );
    }
    ok(&d.read(s, a).await, "read after create");
    ok(&d.create(s, b).await, "create second");

    let listed = d.listed(&d.list(s).await.json());
    assert!(
        listed.iter().any(|n| n.ends_with(a)) && listed.iter().any(|n| n.ends_with(b)),
        "list missing items: {listed:?}"
    );

    ok(&d.update(s, a).await, "update");
    ok(&d.read(s, a).await, "read after update");

    ok(&d.delete(s, a).await, "delete");
    let gone = d.read(s, a).await;
    assert_eq!(gone.status, 404, "read after delete: {}", gone.text());
    assert_eq!(
        d.delete(s, a).await.status,
        d.redelete_status(),
        "second delete"
    );
    let listed = d.listed(&d.list(s).await.json());
    assert!(
        !listed.iter().any(|n| n.ends_with(a)),
        "deleted item still listed: {listed:?}"
    );
    ok(&d.delete(s, b).await, "cleanup");
}

/// Error-envelope invariants per adapter: a request routed to the cloud that matches no
/// operation returns 501 with the adapter's request-id header and a non-empty body.
pub async fn adapter_errors(s: &TestServer) {
    let info = s.info().await;
    for cloud in info["clouds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let r = s
            .request(
                "GET",
                "/mcle-conformance/no-such-route",
                &[("x-mcle-cloud", cloud)],
                "",
            )
            .await;
        assert_eq!(
            r.status,
            501,
            "{cloud}: unknown route must be L0 (501): {}",
            r.text()
        );
        assert!(!r.body.is_empty(), "{cloud}: empty error body");
        let has_id = [
            "x-amzn-requestid",
            "x-ms-request-id",
            "x-mcle-request-id",
            "opc-request-id",
            "x-request-id",
        ]
        .iter()
        .any(|h| r.header(h).is_some());
        assert!(has_id, "{cloud}: no request-id header in {:?}", r.headers);
    }
}

/// Static metadata invariants for a set of facades.
pub fn metadata(services: &[std::sync::Arc<dyn ServiceFacade>]) {
    for svc in services {
        let m = svc.manifest();
        let ops = svc.ops();
        assert!(!ops.is_empty(), "{}/{}: no ops", m.cloud, m.name);
        for (i, op) in ops.iter().enumerate() {
            let id = format!("{}/{}/{}", m.cloud, m.name, op.name);
            assert!(!op.iam_action.0.is_empty(), "{id}: missing iam_action");
            if let Some(c) = op.crud {
                assert!(op.level >= FidelityLevel::L1, "{id}: CRUD-bound but L0");
                assert!(!c.resource.is_empty(), "{id}: empty CRUD resource");
            }
            assert!(
                svc.routes().routes.iter().any(|r| r.op.0 as usize == i),
                "{id}: no route"
            );
        }
        for r in svc.routes().routes {
            assert!(
                (r.op.0 as usize) < ops.len(),
                "{}/{}: route to unknown op",
                m.cloud,
                m.name
            );
        }
    }
}
