#![forbid(unsafe_code)]

#[path = "../gen/mod.rs"]
pub mod gen;

use mcle_sdk::{
    CloudAdapter, CloudError, Host, OpId, OpInput, OpMeta, OpOutput, RequestContext, RouteTable,
    ServiceFacade, ServiceManifest,
};
use serde_json::json;
use std::sync::Arc;

pub struct IamIdentity {
    host: Host,
}

pub fn facade(host: &Host, _adapter: &dyn CloudAdapter) -> Arc<dyn ServiceFacade> {
    Arc::new(IamIdentity { host: host.clone() })
}

#[async_trait::async_trait]
impl ServiceFacade for IamIdentity {
    fn manifest(&self) -> &'static ServiceManifest {
        &gen::MANIFEST
    }
    fn routes(&self) -> &'static RouteTable {
        &gen::ROUTES
    }
    fn ops(&self) -> &'static [OpMeta] {
        &gen::OPS
    }
    async fn dispatch(
        &self,
        ctx: &RequestContext,
        op: OpId,
        input: OpInput,
    ) -> Result<OpOutput, CloudError> {
        if op != gen::op::GET_TOKEN {
            return Err(CloudError::not_implemented(&ctx.service, &ctx.op));
        }
        let grant = input.body_str("grant_type").unwrap_or("");
        let subject = match grant {
            "urn:ibm:params:oauth:grant-type:apikey" => {
                input.body_str("apikey").ok_or_else(|| {
                    CloudError::invalid("Provided API key could not be found.")
                        .with_code("BXNIM0415E")
                })?
            }
            "urn:ibm:params:oauth:grant-type:cr-token"
            | "urn:ibm:params:oauth:grant-type:assume" => input
                .body_str("profile_id")
                .or(input.body_str("profile_name"))
                .unwrap_or("profile"),
            _ => {
                return Err(
                    CloudError::invalid(format!("Unsupported grant type `{grant}`."))
                        .with_code("BXNIM0109E"),
                )
            }
        };
        let iat = self.host.clock.now().unix_timestamp();
        let exp = iat + 3600;
        let iam_id = format!(
            "iam-ServiceId-{}",
            subject
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .take(32)
                .collect::<String>()
        );
        let token = mcle_identity::issue_unsigned_jwt(&json!({
            "iam_id": iam_id,
            "sub": iam_id,
            "sub_type": "ServiceId",
            "account": {"bss": mcle_cloud_ibm::DEFAULT_ACCOUNT},
            "iat": iat,
            "exp": exp,
            "iss": format!("{}/identity", ctx.public_base_url),
            "grant_type": grant,
        }));
        Ok(OpOutput::json(json!({
            "access_token": token,
            "refresh_token": "not_supported",
            "token_type": "Bearer",
            "expires_in": 3600,
            "expiration": exp,
            "scope": "ibm openid",
        })))
    }
}
