// verifies(L1): projects.secrets.create, projects.secrets.get, projects.secrets.list, projects.secrets.patch, projects.secrets.delete
// verifies(L1): projects.locations.secrets.create, projects.locations.secrets.get, projects.locations.secrets.list, projects.locations.secrets.patch, projects.locations.secrets.delete
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

#[tokio::test]
async fn regional_secrets_are_separate_from_global() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_gcp::adapter(host);
        (vec![mcle_svc_gcp_secretmanager::facade(host, &*a)], vec![a])
    })
    .await;
    let base = "/v1/projects/p/locations/us-east1/secrets";
    let r = s
        .json("POST", &format!("{base}?secretId=r1"), &[], &json!({}))
        .await;
    assert_eq!(r.json()["name"], "projects/p/locations/us-east1/secrets/r1");
    assert_eq!(
        s.json("GET", &format!("{base}/r1"), &[], &Value::Null)
            .await
            .status,
        200
    );
    let p = s
        .json(
            "PATCH",
            &format!("{base}/r1?updateMask=labels"),
            &[],
            &json!({"labels": {"k": "v"}}),
        )
        .await;
    assert_eq!(p.json()["labels"], json!({"k": "v"}));
    assert_eq!(
        s.json("GET", base, &[], &Value::Null).await.json()["totalSize"],
        1
    );
    assert_eq!(
        s.json("GET", "/v1/projects/p/secrets", &[], &Value::Null)
            .await
            .json()["totalSize"],
        0
    );
    assert_eq!(
        s.json("DELETE", &format!("{base}/r1"), &[], &Value::Null)
            .await
            .status,
        200
    );
    assert_eq!(
        s.json("GET", &format!("{base}/r1"), &[], &Value::Null)
            .await
            .status,
        404
    );
}

#[tokio::test]
async fn responses_decode_into_generated_shapes() {
    use mcle_svc_gcp_secretmanager::gen::shapes::{ListSecretsResponse, Secret};
    let s = TestServer::start(|host| {
        let a = mcle_cloud_gcp::adapter(host);
        (vec![mcle_svc_gcp_secretmanager::facade(host, &*a)], vec![a])
    })
    .await;
    let body = serde_json::to_value(Secret {
        labels: Some([("a".to_string(), "b".to_string())].into()),
        ..Default::default()
    })
    .unwrap();
    let r = s
        .json("POST", "/v1/projects/t/secrets?secretId=s", &[], &body)
        .await;
    let sec: Secret = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(sec.name.as_deref(), Some("projects/t/secrets/s"));
    let l: ListSecretsResponse = serde_json::from_slice(
        &s.json("GET", "/v1/projects/t/secrets", &[], &Value::Null)
            .await
            .body,
    )
    .unwrap();
    assert_eq!(l.total_size, Some(1));
    assert_eq!(l.secrets.unwrap()[0].labels.as_ref().unwrap()["a"], "b");
}
