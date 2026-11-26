// verifies(L1): GetNamespace, CreateBucket, GetBucket, ListBuckets, UpdateBucket, DeleteBucket
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::TestServer;
use serde_json::{json, Value};

#[tokio::test]
async fn lifecycle() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_oci::adapter(host);
        (vec![mcle_svc_oci_objectstorage::facade(host, &*a)], vec![a])
    })
    .await;
    assert_eq!(
        s.json("GET", "/n/", &[], &Value::Null).await.json(),
        json!("mcle")
    );
    let c = s
        .json(
            "POST",
            "/n/mcle/b/",
            &[],
            &json!({"name": "b", "compartmentId": "ocid1.compartment.oc1..c"}),
        )
        .await;
    assert_eq!(c.status, 200);
    assert!(c.header("etag").is_some());
    assert_eq!(c.json()["namespace"], "mcle");

    assert_eq!(
        s.json("GET", "/n/mcle/b/b/", &[], &Value::Null)
            .await
            .json()["name"],
        "b"
    );
    let l = s
        .json(
            "GET",
            "/n/mcle/b/?compartmentId=ocid1.compartment.oc1..c",
            &[],
            &Value::Null,
        )
        .await
        .json();
    assert_eq!(l.as_array().unwrap().len(), 1);
    let u = s
        .json(
            "POST",
            "/n/mcle/b/b/",
            &[],
            &json!({"freeformTags": {"k": "v"}}),
        )
        .await
        .json();
    assert_eq!(u["freeformTags"], json!({"k": "v"}));

    assert_eq!(
        s.json("DELETE", "/n/mcle/b/b/", &[], &Value::Null)
            .await
            .status,
        204
    );
    let nf = s.json("GET", "/n/mcle/b/b/", &[], &Value::Null).await;
    assert_eq!(
        (nf.status, nf.json()["code"].clone()),
        (404, json!("BucketNotFound"))
    );
}
