// verifies(L2): get_token
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcle_testkit::TestServer;

#[tokio::test]
async fn api_key_exchange_returns_decodable_jwt() {
    let s = TestServer::start(|host| {
        let a = mcle_cloud_ibm::adapter(host);
        (vec![mcle_svc_ibm_iamidentity::facade(host, &*a)], vec![a])
    })
    .await;
    let r = s
        .request(
            "POST",
            "/identity/token",
            &[("content-type", "application/x-www-form-urlencoded")],
            "grant_type=urn%3Aibm%3Aparams%3Aoauth%3Agrant-type%3Aapikey&apikey=k1",
        )
        .await;
    assert_eq!(r.status, 200);
    let tok = r.json()["access_token"].as_str().unwrap().to_string();
    let claims = mcle_identity::jwt_claims(&tok).unwrap();
    assert_eq!(claims["iam_id"], "iam-ServiceId-k1");
    assert!(claims["exp"].as_i64().unwrap() > claims["iat"].as_i64().unwrap());

    let bad = s
        .request(
            "POST",
            "/identity/token",
            &[("content-type", "application/x-www-form-urlencoded")],
            "grant_type=password",
        )
        .await;
    assert_eq!(bad.status, 400);
}
