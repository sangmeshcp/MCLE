#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::conformance::{crud, CrudDriver};
use mcle_testkit::{TestResponse, TestServer};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

// Instances are addressed by generated GUID; remember the GUID minted for each name.
#[derive(Default)]
struct Instances(Mutex<HashMap<String, String>>);

impl Instances {
    fn guid(&self, n: &str) -> String {
        self.0
            .lock()
            .unwrap()
            .get(n)
            .cloned()
            .unwrap_or_else(|| n.to_string())
    }
}

#[async_trait::async_trait]
impl CrudDriver for Instances {
    async fn create(&self, s: &TestServer, n: &str) -> TestResponse {
        let r = s.json("POST", "/v2/resource_instances", &[], &json!({"name": n, "target": "us-south", "resource_group": "g", "resource_plan_id": "p"})).await;
        if let Some(g) = r.json()["guid"].as_str() {
            self.0.lock().unwrap().insert(n.to_string(), g.to_string());
        }
        r
    }
    async fn read(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json(
            "GET",
            &format!("/v2/resource_instances/{}", self.guid(n)),
            &[],
            &Value::Null,
        )
        .await
    }
    async fn update(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json(
            "PATCH",
            &format!("/v2/resource_instances/{}", self.guid(n)),
            &[],
            &json!({"parameters": {"k": "v"}}),
        )
        .await
    }
    async fn delete(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json(
            "DELETE",
            &format!("/v2/resource_instances/{}", self.guid(n)),
            &[],
            &Value::Null,
        )
        .await
    }
    async fn list(&self, s: &TestServer) -> TestResponse {
        s.json("GET", "/v2/resource_instances", &[], &Value::Null)
            .await
    }
    fn listed(&self, b: &Value) -> Vec<String> {
        b["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect()
    }
    fn conflict_on_duplicate(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn crud_conformance() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_ibm::adapter(host);
        (
            vec![mcle_svc_ibm_resourcecontroller::facade(host, &*a)],
            vec![a],
        )
    })
    .await;
    crud(&s, &Instances::default()).await;
}
