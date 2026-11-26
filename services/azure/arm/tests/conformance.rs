#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::conformance::{crud, CrudDriver};
use mcle_testkit::{TestResponse, TestServer};
use serde_json::{json, Value};

const RG: &str = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/conf";
const V: &str = "?api-version=2023-01-01";

struct Generic;

fn item(n: &str) -> String {
    format!("{RG}/providers/Microsoft.Storage/storageAccounts/{n}{V}")
}

#[async_trait::async_trait]
impl CrudDriver for Generic {
    async fn setup(&self, s: &TestServer) {
        s.json(
            "PUT",
            &format!("{RG}{V}"),
            &[],
            &json!({"location": "westus"}),
        )
        .await;
    }
    async fn create(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json(
            "PUT",
            &item(n),
            &[],
            &json!({"location": "westus", "properties": {}}),
        )
        .await
    }
    async fn read(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json("GET", &item(n), &[], &Value::Null).await
    }
    async fn update(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json("PATCH", &item(n), &[], &json!({"tags": {"k": "v"}}))
            .await
    }
    async fn delete(&self, s: &TestServer, n: &str) -> TestResponse {
        s.json("DELETE", &item(n), &[], &Value::Null).await
    }
    async fn list(&self, s: &TestServer) -> TestResponse {
        s.json(
            "GET",
            &format!("{RG}/providers/Microsoft.Storage/storageAccounts{V}"),
            &[],
            &Value::Null,
        )
        .await
    }
    fn listed(&self, b: &Value) -> Vec<String> {
        b["value"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect()
    }
    fn redelete_status(&self) -> u16 {
        204
    }
    fn conflict_on_duplicate(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn crud_conformance() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_azure::adapter(host);
        (vec![mcle_svc_azure_arm::facade(host, &*a)], vec![a])
    })
    .await;
    crud(&s, &Generic).await;
}
