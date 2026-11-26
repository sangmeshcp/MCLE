#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_sdk::{Body, CloudId, OpInput, Principal, RequestContext, SandboxId, Scope};
use mcle_svc_aws_secretsmanager::gen::{api, op, shapes};
use serde_json::json;

struct Describe;

#[async_trait::async_trait]
impl api::Api for Describe {
    async fn describe_secret(
        &self,
        _ctx: &RequestContext,
        req: api::Request<shapes::DescribeSecretRequest>,
    ) -> Result<shapes::DescribeSecretResponse, mcle_sdk::CloudError> {
        Ok(shapes::DescribeSecretResponse {
            name: req.body.secret_id,
            description: Some("typed".into()),
            ..Default::default()
        })
    }
}

fn ctx() -> RequestContext {
    RequestContext {
        request_id: "r".into(),
        sandbox: SandboxId::default(),
        cloud: CloudId::AWS,
        service: "secretsmanager".into(),
        op: "DescribeSecret".into(),
        principal: Principal::anonymous(),
        scope: Scope::Aws {
            account: "0".into(),
            region: "us-east-1".into(),
        },
        public_base_url: String::new(),
    }
}

#[tokio::test]
async fn generated_api_decodes_input_and_encodes_output() {
    let input = OpInput {
        body: json!({"SecretId": "db"}),
        ..Default::default()
    };
    let out = api::dispatch(&Describe, &ctx(), op::DESCRIBE_SECRET, input)
        .await
        .unwrap();
    assert_eq!(
        out.body,
        Body::Json(json!({"Name": "db", "Description": "typed"}))
    );

    let err = api::dispatch(&Describe, &ctx(), op::GET_SECRET_VALUE, OpInput::default())
        .await
        .unwrap_err();
    assert_eq!(err.http_status, 501);

    let bad = OpInput {
        body: json!({"SecretId": 7}),
        ..Default::default()
    };
    assert_eq!(
        api::dispatch(&Describe, &ctx(), op::DESCRIBE_SECRET, bad)
            .await
            .unwrap_err()
            .http_status,
        400
    );
}
