#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::conformance::{crud, CrudDriver};
use mcle_testkit::{TestResponse, TestServer};
use serde_json::{json, Value};

const COMP: &str = "ocid1.compartment.oc1..conf";

struct Buckets;

#[async_trait::async_trait]
impl CrudDriver for Buckets {
    async fn create(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json(
            "POST",
            "/n/mcle/b/",
            &[],
            &json!({"name": n, "compartmentId": COMP}),
        )
        .await
    }
    async fn read(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json("GET", &format!("/n/mcle/b/{n}/"), &[], &Value::Null)
            .await
    }
    async fn update(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json(
            "POST",
            &format!("/n/mcle/b/{n}/"),
            &[],
            &json!({"freeformTags": {"k": "v"}}),
        )
        .await
    }
    async fn delete(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json("DELETE", &format!("/n/mcle/b/{n}/"), &[], &Value::Null)
            .await
    }
    async fn list(&self, s: &TestServer) -> TestResponse {
        s.json(
            "GET",
            &format!("/n/mcle/b/?compartmentId={COMP}"),
            &[],
            &Value::Null,
        )
        .await
    }
    fn listed(&self, b: &Value) -> Vec<String> {
        b.as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect()
    }
}

#[tokio::test]
async fn crud_conformance() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_oci::adapter(host);
        (vec![mcle_svc_oci_objectstorage::facade(host, &*a)], vec![a])
    })
    .await;
    crud(&s, &Buckets).await;
}
