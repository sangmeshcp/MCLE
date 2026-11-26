// verifies(L1): create_resource_instance, get_resource_instance, update_resource_instance, list_resource_instances, delete_resource_instance
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::TestServer;
use serde_json::{json, Value};

#[tokio::test]
async fn lifecycle() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_ibm::adapter(host);
        (
            vec![mcle_svc_ibm_resourcecontroller::facade(host, &*a)],
            vec![a],
        )
    })
    .await;
    let c = s.json("POST", "/v2/resource_instances", &[], &json!({"name": "i", "target": "eu-de", "resource_group": "g", "resource_plan_id": "p"})).await;
    assert_eq!(c.status, 201);
    let body = c.json();
    let guid = body["guid"].as_str().unwrap();
    assert_eq!(body["region_id"], "eu-de");
    let crn = body["crn"]
        .as_str()
        .unwrap()
        .replace(':', "%3A")
        .replace('/', "%2F");

    assert_eq!(
        s.json(
            "GET",
            &format!("/v2/resource_instances/{crn}"),
            &[],
            &Value::Null
        )
        .await
        .json()["guid"],
        guid
    );
    let u = s
        .json(
            "PATCH",
            &format!("/v2/resource_instances/{guid}"),
            &[],
            &json!({"name": "j"}),
        )
        .await
        .json();
    assert_eq!(u["name"], "j");
    let l = s
        .json("GET", "/v2/resource_instances", &[], &Value::Null)
        .await
        .json();
    assert_eq!(l["rows_count"], 1);
    assert_eq!(
        s.json(
            "DELETE",
            &format!("/v2/resource_instances/{guid}"),
            &[],
            &Value::Null
        )
        .await
        .status,
        204
    );
    let nf = s
        .json(
            "GET",
            &format!("/v2/resource_instances/{guid}"),
            &[],
            &Value::Null,
        )
        .await;
    assert_eq!(
        (nf.status, nf.json()["errors"][0]["code"].clone()),
        (404, json!("not_found"))
    );
}
