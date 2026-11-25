// verifies(L1): Subscriptions_List, Subscriptions_Get, Providers_List, Providers_Get, Providers_Register, ResourceGroups_List, ResourceGroups_CreateOrUpdate, ResourceGroups_Get, ResourceGroups_CheckExistence, ResourceGroups_Update, ResourceGroups_Delete
// verifies(L1): Resources_ListByResourceGroup, Resources_List, Resources_CreateOrUpdate, Resources_Get, Resources_CheckExistence, Resources_Update, Resources_Delete, Resources_ListBySubscriptionAndType
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_core::CoreOptions;
use mcle_gateway::GatewayConfig;
use mcle_testkit::TestServer;
use serde_json::{json, Value};

const SUB: &str = "/subscriptions/00000000-0000-0000-0000-000000000000";
const V: &str = "?api-version=2021-04-01";

async fn server(lro_delay_ms: u64) -> TestServer {
    TestServer::start_with(
        CoreOptions {
            seed: 1,
            clock: "fixed:2026-01-01T00:00:00Z".into(),
            lro_delay_ms,
            ..Default::default()
        },
        GatewayConfig::default(),
        |host| {
            let a = mcle_cloud_azure::adapter(host);
            (vec![mcle_svc_azure_arm::facade(host, &*a)], vec![a])
        },
    )
    .await
}

#[tokio::test]
async fn subscriptions_and_providers() {
    let s = server(0).await;
    assert_eq!(
        s.json("GET", &format!("/subscriptions{V}"), &[], &Value::Null)
            .await
            .json()["value"][0]["state"],
        "Enabled"
    );
    assert_eq!(
        s.json("GET", &format!("{SUB}{V}"), &[], &Value::Null)
            .await
            .status,
        200
    );
    assert!(!s
        .json("GET", &format!("{SUB}/providers{V}"), &[], &Value::Null)
        .await
        .json()["value"]
        .as_array()
        .unwrap()
        .is_empty());
    let p = s
        .json(
            "POST",
            &format!("{SUB}/providers/Microsoft.Foo/register{V}"),
            &[],
            &Value::Null,
        )
        .await
        .json();
    assert_eq!(p["registrationState"], "Registered");
    assert_eq!(
        s.json(
            "GET",
            &format!("{SUB}/providers/Microsoft.Foo{V}"),
            &[],
            &Value::Null
        )
        .await
        .json()["namespace"],
        "Microsoft.Foo"
    );
}

#[tokio::test]
async fn api_version_is_required() {
    let s = server(0).await;
    let r = s.json("GET", "/subscriptions", &[], &Value::Null).await;
    assert_eq!(
        (r.status, r.json()["error"]["code"].clone()),
        (400, json!("MissingApiVersionParameter"))
    );
}

#[tokio::test]
async fn groups_and_generic_resources() {
    let s = server(0).await;
    let rg = format!("{SUB}/resourcegroups/RG1");
    assert_eq!(
        s.json(
            "PUT",
            &format!("{rg}{V}"),
            &[],
            &json!({"location": "westus"})
        )
        .await
        .status,
        201
    );
    assert_eq!(
        s.json(
            "PUT",
            &format!("{rg}{V}"),
            &[],
            &json!({"location": "westus"})
        )
        .await
        .status,
        200
    );
    assert_eq!(
        s.json(
            "HEAD",
            &format!("{SUB}/resourceGroups/rg1{V}"),
            &[],
            &Value::Null
        )
        .await
        .status,
        204
    );
    let up = s
        .json(
            "PATCH",
            &format!("{rg}{V}"),
            &[],
            &json!({"tags": {"t": "1"}}),
        )
        .await
        .json();
    assert_eq!(up["tags"], json!({"t": "1"}));
    assert_eq!(
        s.json(
            "GET",
            &format!("{SUB}/resourcegroups{V}"),
            &[],
            &Value::Null
        )
        .await
        .json()["value"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let res = format!("{rg}/providers/Microsoft.Storage/storageAccounts/sa1");
    let missing_rg = s
        .json(
            "PUT",
            &format!("{SUB}/resourcegroups/nope/providers/Microsoft.Storage/storageAccounts/x{V}"),
            &[],
            &json!({}),
        )
        .await;
    assert_eq!(missing_rg.json()["error"]["code"], "ResourceGroupNotFound");
    let c = s
        .json(
            "PUT",
            &format!("{res}{V}"),
            &[],
            &json!({"location": "westus", "properties": {"a": 1}}),
        )
        .await;
    assert_eq!(c.status, 201);
    assert_eq!(c.json()["properties"]["provisioningState"], "Succeeded");
    assert_eq!(
        s.json(
            "PATCH",
            &format!("{res}{V}"),
            &[],
            &json!({"properties": {"b": 2}})
        )
        .await
        .json()["properties"]["a"],
        1
    );
    assert_eq!(
        s.json("HEAD", &format!("{res}{V}"), &[], &Value::Null)
            .await
            .status,
        204
    );
    assert_eq!(
        s.json("GET", &format!("{res}{V}"), &[], &Value::Null)
            .await
            .json()["type"],
        "Microsoft.Storage/storageAccounts"
    );
    let by_type = s
        .json(
            "GET",
            &format!("{rg}/providers/Microsoft.Storage/storageAccounts{V}"),
            &[],
            &Value::Null,
        )
        .await
        .json();
    assert_eq!(by_type["value"].as_array().unwrap().len(), 1);
    let sub_type = s
        .json(
            "GET",
            &format!("{SUB}/providers/Microsoft.Storage/storageAccounts{V}"),
            &[],
            &Value::Null,
        )
        .await
        .json();
    assert_eq!(sub_type["value"].as_array().unwrap().len(), 1);
    assert_eq!(
        s.json("GET", &format!("{SUB}/resources{V}"), &[], &Value::Null)
            .await
            .json()["value"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.json("GET", &format!("{rg}/resources{V}"), &[], &Value::Null)
            .await
            .json()["value"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    assert_eq!(
        s.json("DELETE", &format!("{res}{V}"), &[], &Value::Null)
            .await
            .status,
        200
    );
    assert_eq!(
        s.json("DELETE", &format!("{res}{V}"), &[], &Value::Null)
            .await
            .status,
        204
    );
    s.json(
        "PUT",
        &format!("{res}{V}"),
        &[],
        &json!({"location": "westus"}),
    )
    .await;
    assert_eq!(
        s.json("DELETE", &format!("{rg}{V}"), &[], &Value::Null)
            .await
            .status,
        200
    );
    assert_eq!(
        s.json("GET", &format!("{SUB}/resources{V}"), &[], &Value::Null)
            .await
            .json()["value"],
        json!([])
    );
}

#[tokio::test]
async fn long_running_create_polls_to_success() {
    let s = server(1000).await;
    s.json(
        "PUT",
        &format!("{SUB}/resourcegroups/g{V}"),
        &[],
        &json!({"location": "westus"}),
    )
    .await;
    let res = format!("{SUB}/resourcegroups/g/providers/Microsoft.Web/sites/app");
    let c = s
        .json(
            "PUT",
            &format!("{res}{V}"),
            &[],
            &json!({"location": "westus"}),
        )
        .await;
    assert_eq!(c.json()["properties"]["provisioningState"], "Creating");
    let poll = c
        .header("azure-asyncoperation")
        .unwrap()
        .split_once("/_mcle")
        .unwrap()
        .1
        .to_string();
    let path = format!("/_mcle{poll}");
    assert_eq!(
        s.request("GET", &path, &[], "").await.json()["status"],
        "InProgress"
    );
    s.json("POST", "/_mcle/clock/advance", &[], &json!({"ms": 1000}))
        .await;
    assert_eq!(
        s.request("GET", &path, &[], "").await.json()["status"],
        "Succeeded"
    );
    assert_eq!(
        s.json("GET", &format!("{res}{V}"), &[], &Value::Null)
            .await
            .json()["properties"]["provisioningState"],
        "Succeeded"
    );
}
