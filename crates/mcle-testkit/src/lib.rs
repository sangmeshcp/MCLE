#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use mcle_core::CoreOptions;
use mcle_gateway::{Gateway, GatewayConfig, Plugins, RunningGateway};
use mcle_sdk::{CloudAdapter, Host, ServiceFacade};
use serde_json::Value;
use std::sync::Arc;

pub mod conformance;

pub type Build = Vec<Arc<dyn ServiceFacade>>;

pub struct TestServer {
    pub base: String,
    pub running: RunningGateway,
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>,
}

#[derive(Debug)]
pub struct TestResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

impl TestResponse {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

impl TestServer {
    /// `build` receives the host and returns the services and adapters to register.
    pub async fn start<F>(build: F) -> Self
    where
        F: FnOnce(&Host) -> (Build, Vec<Arc<dyn CloudAdapter>>),
    {
        Self::start_with(
            CoreOptions {
                seed: 1,
                ..Default::default()
            },
            GatewayConfig::default(),
            build,
        )
        .await
    }

    pub async fn start_with<F>(mut opts: CoreOptions, cfg: GatewayConfig, build: F) -> Self
    where
        F: FnOnce(&Host) -> (Build, Vec<Arc<dyn CloudAdapter>>),
    {
        opts.config.public_base_url = "http://127.0.0.1:0".into();
        let host = mcle_core::build_host(&opts).expect("host");
        let (services, adapters) = build(&host);
        let gw = Gateway::new(
            host,
            Plugins {
                adapters,
                services,
                ..Default::default()
            },
            cfg,
        );
        let running = gw.serve(([127, 0, 0, 1], 0).into()).await.expect("bind");
        TestServer {
            base: format!("http://127.0.0.1:{}", running.addr.port()),
            running,
            client: Client::builder(TokioExecutor::new()).build_http(),
        }
    }

    pub fn gateway(&self) -> &Arc<Gateway> {
        &self.running.gateway
    }

    pub async fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: impl Into<Bytes>,
    ) -> TestResponse {
        let mut b = http::Request::builder()
            .method(method)
            .uri(format!("{}{}", self.base, path));
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let resp = self
            .client
            .request(b.body(Full::new(body.into())).unwrap())
            .await
            .expect("request");
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        TestResponse {
            status,
            headers,
            body,
        }
    }

    pub async fn json(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &Value,
    ) -> TestResponse {
        let mut h = headers.to_vec();
        h.push(("content-type", "application/json"));
        let data = if body.is_null() {
            Bytes::new()
        } else {
            Bytes::from(body.to_string())
        };
        self.request(method, path, &h, data).await
    }

    pub async fn info(&self) -> Value {
        self.request("GET", "/_mcle/info", &[], Bytes::new())
            .await
            .json()
    }

    pub async fn report(&self) -> Value {
        self.request("GET", "/_mcle/report?format=json", &[], Bytes::new())
            .await
            .json()
    }

    pub async fn stop(self) {
        self.running.shutdown().await;
    }
}
