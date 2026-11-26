#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_core::CoreOptions;
use mcle_gateway::GatewayConfig;
use mcle_sdk::IamMode;
use mcle_testkit::TestServer;
use serde_json::{json, Value};
use std::process::Command;
use std::time::Instant;

const SM_CREATE: (&str, &str) = ("x-amz-target", "secretsmanager.CreateSecret");
const JSON11: (&str, &str) = ("content-type", "application/x-amz-json-1.1");

async fn all(iam: IamMode) -> TestServer {
    TestServer::start_with(
        CoreOptions {
            seed: 3,
            ..Default::default()
        },
        GatewayConfig {
            iam,
            ..Default::default()
        },
        |host| {
            let p = mcle_bundle::plugins(host, &[]);
            (p.services, p.adapters)
        },
    )
    .await
}

async fn secret_count(s: &TestServer, sandbox: Option<&str>) -> usize {
    let mut h = vec![("x-amz-target", "secretsmanager.ListSecrets"), JSON11];
    if let Some(x) = sandbox {
        h.push(("x-mcle-sandbox", x));
    }
    s.request("POST", "/", &h, "{}").await.json()["SecretList"]
        .as_array()
        .unwrap()
        .len()
}

#[tokio::test]
async fn internal_endpoints_and_info() {
    let s = all(IamMode::Audit).await;
    assert_eq!(s.request("GET", "/_mcle/health", &[], "").await.status, 200);
    assert_eq!(s.request("GET", "/_mcle/ready", &[], "").await.status, 200);
    let info = s.info().await;
    assert_eq!(info["clouds"].as_array().unwrap().len(), 5);
    assert_eq!(info["iam"], "audit");
    let env = s
        .request("GET", "/_mcle/env?format=sh", &[], "")
        .await
        .text();
    assert!(env.contains("export AWS_ENDPOINT_URL=http://127.0.0.1:"));
    assert_eq!(s.request("GET", "/_mcle/nope", &[], "").await.status, 404);
}

#[tokio::test]
async fn cloud_detection() {
    let s = all(IamMode::Off).await;
    let r = s
        .json("GET", "/v2/resource_instances", &[], &Value::Null)
        .await;
    assert!(r.json().get("resources").is_some());
    assert_eq!(
        s.json("GET", "/n/", &[], &Value::Null).await.json(),
        json!("mcle")
    );
    let r = s
        .request("GET", "/whatever", &[("x-mcle-cloud", "gcp")], "")
        .await;
    assert_eq!(r.json()["error"]["status"], "UNIMPLEMENTED");
    let r = s
        .request("POST", "/", &[("x-amz-target", "Nope.Op"), JSON11], "{}")
        .await;
    assert_eq!(
        (r.status, r.json()["__type"].clone()),
        (501, json!("NotImplementedException"))
    );
}

#[tokio::test]
async fn coverage_report_and_strict_violations() {
    let s = all(IamMode::Audit).await;
    s.request("POST", "/", &[SM_CREATE, JSON11], r#"{"Name":"a"}"#)
        .await;
    s.request(
        "POST",
        "/",
        &[("x-amz-target", "secretsmanager.GetSecretValue"), JSON11],
        r#"{"SecretId":"a"}"#,
    )
    .await;
    assert_eq!(s.report().await["calls"], 2);
    let strict = s
        .request("GET", "/_mcle/report?format=json&strict=L1", &[], "")
        .await
        .json();
    assert_eq!(
        strict["violations"],
        json!(["aws/secretsmanager/GetSecretValue: fidelity L0"])
    );
    let junit = s
        .request("GET", "/_mcle/report?format=junit&strict=L2", &[], "")
        .await
        .text();
    assert!(junit.contains("failures=\"1\""));
}

#[tokio::test]
async fn fault_injection() {
    let s = all(IamMode::Off).await;
    let rule = json!({"cloud": "aws", "op": "CreateSecret", "error": {"code": "ThrottlingException", "status": 400}, "times": 1});
    assert_eq!(
        s.json("POST", "/_mcle/faults", &[], &rule).await.status,
        200
    );
    let r = s
        .request("POST", "/", &[SM_CREATE, JSON11], r#"{"Name":"a"}"#)
        .await;
    assert_eq!(
        (r.status, r.json()["__type"].clone()),
        (400, json!("ThrottlingException"))
    );
    assert_eq!(
        s.request("POST", "/", &[SM_CREATE, JSON11], r#"{"Name":"a"}"#)
            .await
            .status,
        200
    );
    assert_eq!(
        s.report().await["entries"][0]["fault"],
        "aws/*/CreateSecret"
    );
}

#[tokio::test]
async fn iam_explain_records_decisions() {
    let s = all(IamMode::Audit).await;
    let r = s
        .request("POST", "/", &[SM_CREATE, JSON11], r#"{"Name":"a"}"#)
        .await;
    let rid = r.header("x-amzn-requestid").unwrap().to_string();
    let ex = s
        .request(
            "GET",
            &format!("/_mcle/iam/explain?request_id={rid}"),
            &[],
            "",
        )
        .await
        .json();
    assert_eq!(ex["request"]["action"], "secretsmanager:CreateSecret");
    assert_eq!(ex["request"]["resource"], "a");
    assert_eq!(ex["outcome"]["label"], "allow");
}

#[tokio::test]
async fn sandboxes_and_state_round_trip() {
    let s = all(IamMode::Off).await;
    let sb = s
        .json("POST", "/_mcle/sandboxes", &[], &json!({"id": "t1"}))
        .await
        .json();
    assert_eq!(sb["env"]["AWS_ACCESS_KEY_ID"], sb["access_key"]);
    s.request(
        "POST",
        "/",
        &[SM_CREATE, JSON11, ("x-mcle-sandbox", "t1")],
        r#"{"Name":"in-sb"}"#,
    )
    .await;
    assert_eq!(secret_count(&s, Some("t1")).await, 1);
    assert_eq!(secret_count(&s, None).await, 0);

    let snap = s
        .request("GET", "/_mcle/state/export", &[], "")
        .await
        .json();
    assert_eq!(snap["format"], "mcle-state/v1");
    s.request("POST", "/_mcle/reset", &[], "").await;
    assert_eq!(secret_count(&s, Some("t1")).await, 0);
    assert_eq!(
        s.json("POST", "/_mcle/state/import", &[], &snap)
            .await
            .status,
        200
    );
    assert_eq!(secret_count(&s, Some("t1")).await, 1);
    assert_eq!(
        s.request("DELETE", "/_mcle/sandboxes/t1", &[], "")
            .await
            .status,
        200
    );
    assert_eq!(secret_count(&s, Some("t1")).await, 0);
}

fn mcle() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcle"))
}

#[test]
fn run_wrapper_exit_codes_and_env() {
    let ok = mcle()
        .args([
            "run",
            "--",
            "sh",
            "-c",
            "test -n \"$AWS_ENDPOINT_URL\" && test -n \"$MCLE_ENDPOINT\"",
        ])
        .status()
        .unwrap();
    assert_eq!(ok.code(), Some(0));
    assert_eq!(
        mcle()
            .args(["run", "--", "sh", "-c", "exit 7"])
            .status()
            .unwrap()
            .code(),
        Some(7)
    );
    assert_eq!(
        mcle()
            .args(["run", "--iam=bogus", "--", "true"])
            .status()
            .unwrap()
            .code(),
        Some(2)
    );
}

#[test]
fn run_strict_fails_on_l0_call() {
    if Command::new("curl").arg("--version").output().is_err() {
        return;
    }
    let script = r#"curl -s -o /dev/null -X POST "$MCLE_ENDPOINT/" -H 'x-amz-target: secretsmanager.GetSecretValue' -H 'content-type: application/x-amz-json-1.1' -d '{"SecretId":"x"}'"#;
    let dir = std::env::temp_dir().join(format!("mcle-artifacts-{}", std::process::id()));
    let st = mcle()
        .args(["run", "--strict=L1", "--artifacts-dir"])
        .arg(&dir)
        .args(["--", "sh", "-c", script])
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(10));
    assert!(std::fs::read_to_string(dir.join("report.md"))
        .unwrap()
        .contains("GetSecretValue"));
    assert!(dir.join("state.json").exists() && dir.join("report.xml").exists());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn run_true_is_fast() {
    let t = Instant::now();
    assert!(mcle()
        .args(["run", "--", "true"])
        .status()
        .unwrap()
        .success());
    let limit = if cfg!(debug_assertions) { 2000 } else { 300 };
    assert!(
        t.elapsed().as_millis() < limit,
        "mcle run -- true took {:?}",
        t.elapsed()
    );
}

#[test]
fn env_prints_exports() {
    let out = mcle()
        .args(["env", "--cloud", "aws", "--url", "http://h:1"])
        .output()
        .unwrap();
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("export AWS_ENDPOINT_URL=http://h:1"));
    assert!(!s.contains("AZURE_"));
}

#[tokio::test]
async fn bundle_conformance() {
    let s = all(IamMode::Off).await;
    mcle_testkit::conformance::adapter_errors(&s).await;
    let host = mcle_core::build_host(&CoreOptions::default()).unwrap();
    mcle_testkit::conformance::metadata(&mcle_bundle::plugins(&host, &[]).services);
}
