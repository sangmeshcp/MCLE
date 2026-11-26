#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::conformance::{crud, CrudDriver};
use mcle_testkit::{TestResponse, TestServer};
use serde_json::{json, Value};

struct Secrets(&'static str);

#[async_trait::async_trait]
impl CrudDriver for Secrets {
    async fn create(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json(
            "POST",
            &format!("/v1/{}/secrets?secretId={n}", self.0),
            &[],
            &json!({}),
        )
        .await
    }
    async fn read(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json(
            "GET",
            &format!("/v1/{}/secrets/{n}", self.0),
            &[],
            &Value::Null,
        )
        .await
    }
    async fn update(&self, s: &TestServer, n: &str) -> TestResponse {
        let p = format!("/v1/{}/secrets/{n}?updateMask=labels", self.0);
        s.json("PATCH", &p, &[], &json!({"labels": {"k": "v"}}))
            .await
    }
    async fn delete(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json(
            "DELETE",
            &format!("/v1/{}/secrets/{n}", self.0),
            &[],
            &Value::Null,
        )
        .await
    }
    async fn list(&self, s: &TestServer) -> TestResponse {
        s.json("GET", &format!("/v1/{}/secrets", self.0), &[], &Value::Null)
            .await
    }
    fn listed(&self, b: &Value) -> Vec<String> {
        b["secrets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect()
    }
}

#[tokio::test]
async fn crud_conformance() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_gcp::adapter(host);
        (vec![mcle_svc_gcp_secretmanager::facade(host, &*a)], vec![a])
    })
    .await;
    crud(&s, &Secrets("projects/c")).await;
    crud(&s, &Secrets("projects/c/locations/europe-west1")).await;
}
