#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::conformance::{crud, CrudDriver};
use mcle_testkit::{TestResponse, TestServer};
use serde_json::{json, Value};

struct Secrets;

async fn call(s: &TestServer, op: &str, body: Value) -> TestResponse {
    let target = format!("secretsmanager.{op}");
    let h = [
        ("x-amz-target", target.as_str()),
        ("content-type", "application/x-amz-json-1.1"),
    ];
    s.request("POST", "/", &h, body.to_string()).await
}

#[async_trait::async_trait]
impl CrudDriver for Secrets {
    async fn create(&self, s: &TestServer, n: &str) -> TestResponse {
        call(s, "CreateSecret", json!({"Name": n})).await
    }
    async fn read(&self, s: &TestServer, n: &str) -> TestResponse {
        call(s, "DescribeSecret", json!({"SecretId": n})).await
    }
    async fn update(&self, s: &TestServer, n: &str) -> TestResponse {
        call(
            s,
            "UpdateSecret",
            json!({"SecretId": n, "Description": "u"}),
        )
        .await
    }
    async fn delete(&self, s: &TestServer, n: &str) -> TestResponse {
        call(s, "DeleteSecret", json!({"SecretId": n})).await
    }
    async fn list(&self, s: &TestServer) -> TestResponse {
        call(s, "ListSecrets", json!({})).await
    }
    fn listed(&self, b: &Value) -> Vec<String> {
        b["SecretList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["Name"].as_str().unwrap().to_string())
            .collect()
    }
}

#[tokio::test]
async fn crud_conformance() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_aws::adapter(host);
        (
            vec![mcle_svc_aws_secretsmanager::facade(host, &*a)],
            vec![a],
        )
    })
    .await;
    crud(&s, &Secrets).await;
}
