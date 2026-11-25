use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

async fn send(method: &str, url: &str, body: Vec<u8>) -> Result<(u16, String)> {
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build_http();
    let req = http::Request::builder()
        .method(method)
        .uri(url)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))?;
    let resp = client
        .request(req)
        .await
        .with_context(|| format!("{method} {url}"))?;
    let status = resp.status().as_u16();
    let body = resp.into_body().collect().await?.to_bytes();
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

pub async fn get(url: &str) -> Result<(u16, String)> {
    send("GET", url, Vec::new()).await
}

pub async fn post(url: &str, body: Vec<u8>) -> Result<(u16, String)> {
    send("POST", url, body).await
}
