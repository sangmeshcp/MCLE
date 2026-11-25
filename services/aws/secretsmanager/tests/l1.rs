// verifies(L1): CreateSecret, DescribeSecret, ListSecrets, UpdateSecret, DeleteSecret
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::TestServer;
use serde_json::json;

async fn server() -> TestServer {
    TestServer::start(|host| {
        let a = mcle_cloud_aws::adapter(host);
        (
            vec![mcle_svc_aws_secretsmanager::facade(host, &*a)],
            vec![a],
        )
    })
    .await
}

async fn call(s: &TestServer, op: &str, body: serde_json::Value) -> mcle_testkit::TestResponse {
    let target = format!("secretsmanager.{op}");
    s.request(
        "POST",
        "/",
        &[
            ("x-amz-target", target.as_str()),
            ("content-type", "application/x-amz-json-1.1"),
        ],
        body.to_string(),
    )
    .await
}

#[tokio::test]
async fn lifecycle() {
    let s = server().await;
    let r = call(
        &s,
        "CreateSecret",
        json!({"Name": "a", "SecretString": "x"}),
    )
    .await;
    assert_eq!(r.status, 200);
    let arn = r.json()["ARN"].as_str().unwrap().to_string();
    assert_eq!(r.header("content-type"), Some("application/x-amz-json-1.1"));

    let d = call(&s, "DescribeSecret", json!({"SecretId": arn}))
        .await
        .json();
    assert_eq!(d["Name"], "a");
    assert!(d.get("SecretString").is_none());
    assert!(d["CreatedDate"].is_number());

    call(
        &s,
        "UpdateSecret",
        json!({"SecretId": "a", "Description": "d"}),
    )
    .await;
    let l = call(&s, "ListSecrets", json!({})).await.json();
    assert_eq!(l["SecretList"][0]["Description"], "d");

    let dup = call(&s, "CreateSecret", json!({"Name": "a"})).await;
    assert_eq!(dup.status, 409);
    assert_eq!(dup.json()["__type"], "ResourceExistsException");
    assert_eq!(
        dup.header("x-amzn-errortype"),
        Some("ResourceExistsException")
    );

    assert_eq!(
        call(&s, "DeleteSecret", json!({"SecretId": "a"}))
            .await
            .status,
        200
    );
    let nf = call(&s, "DescribeSecret", json!({"SecretId": "a"})).await;
    assert_eq!(nf.status, 404);
    assert_eq!(nf.json()["__type"], "ResourceNotFoundException");
}

#[tokio::test]
async fn missing_required_field_is_validation_error() {
    let s = server().await;
    let r = call(&s, "CreateSecret", json!({})).await;
    assert_eq!(r.status, 400);
    assert_eq!(r.json()["__type"], "ValidationException");
}
