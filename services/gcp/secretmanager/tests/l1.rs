// verifies(L1): projects.secrets.create, projects.secrets.get, projects.secrets.list, projects.secrets.patch, projects.secrets.delete
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::TestServer;
use serde_json::{json, Value};

#[tokio::test]
async fn lifecycle() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_gcp::adapter(host);
        (vec![mcle_svc_gcp_secretmanager::facade(host, &*a)], vec![a])
    })
    .await;
    let r = s
        .json(
            "POST",
            "/v1/projects/p/secrets?secretId=s1",
            &[],
            &json!({"labels": {"a": "b"}}),
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["name"], "projects/p/secrets/s1");
    assert!(r.json()["createTime"].is_string());

    let p = s
        .json(
            "PATCH",
            "/v1/projects/p/secrets/s1?updateMask=labels",
            &[],
            &json!({"labels": {"x": "y"}}),
        )
        .await
        .json();
    assert_eq!(p["labels"], json!({"x": "y"}));

    let l = s
        .json("GET", "/v1/projects/p/secrets", &[], &Value::Null)
        .await
        .json();
    assert_eq!(l["secrets"].as_array().unwrap().len(), 1);
    assert_eq!(l["totalSize"], 1);

    let dup = s
        .json(
            "POST",
            "/v1/projects/p/secrets?secretId=s1",
            &[],
            &json!({}),
        )
        .await;
    assert_eq!(
        (dup.status, dup.json()["error"]["status"].clone()),
        (409, json!("ALREADY_EXISTS"))
    );

    assert_eq!(
        s.json("DELETE", "/v1/projects/p/secrets/s1", &[], &Value::Null)
            .await
            .json(),
        json!({})
    );
    let nf = s
        .json("GET", "/v1/projects/p/secrets/s1", &[], &Value::Null)
        .await;
    assert_eq!(
        (nf.status, nf.json()["error"]["status"].clone()),
        (404, json!("NOT_FOUND"))
    );

    let l0 = s
        .json(
            "POST",
            "/v1/projects/p/secrets/s1:addVersion",
            &[],
            &json!({}),
        )
        .await;
    assert_eq!(l0.json()["error"]["status"], "UNIMPLEMENTED");
}
