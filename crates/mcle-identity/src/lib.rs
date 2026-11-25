#![forbid(unsafe_code)]

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigV4 {
    pub access_key: String,
    pub date: String,
    pub region: String,
    pub service: String,
}

pub fn parse_sigv4(auth: &str) -> Option<SigV4> {
    let rest = auth.strip_prefix("AWS4-HMAC-SHA256")?.trim_start();
    let cred = rest
        .split(',')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("Credential="))?;
    let mut it = cred.split('/');
    Some(SigV4 {
        access_key: it.next()?.to_string(),
        date: it.next()?.to_string(),
        region: it.next()?.to_string(),
        service: it.next()?.to_string(),
    })
}

pub fn parse_sigv4_query(credential: &str) -> Option<SigV4> {
    parse_sigv4(&format!("AWS4-HMAC-SHA256 Credential={credential}"))
}

pub fn jwt_claims(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .or_else(|_| STANDARD.decode(payload))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn bearer(auth: &str) -> Option<&str> {
    let (scheme, tok) = auth.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| tok.trim())
}

pub fn issue_unsigned_jwt(claims: &Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
    format!("{header}.{payload}.")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OciKeyId {
    pub tenancy: String,
    pub user: String,
    pub fingerprint: String,
}

pub fn parse_oci_signature(auth: &str) -> Option<OciKeyId> {
    let rest = auth.strip_prefix("Signature")?.trim_start();
    let key_id = rest.split(',').map(str::trim).find_map(|p| {
        p.strip_prefix("keyId=")
            .map(|v| v.trim_matches('"').to_string())
    })?;
    let mut it = key_id.splitn(3, '/');
    Some(OciKeyId {
        tenancy: it.next()?.to_string(),
        user: it.next().unwrap_or("").to_string(),
        fingerprint: it.next().unwrap_or("").to_string(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sigv4() {
        let s = parse_sigv4("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, SignedHeaders=host, Signature=abc").unwrap();
        assert_eq!(s.access_key, "AKIDEXAMPLE");
        assert_eq!(s.region, "us-east-1");
        assert_eq!(s.service, "iam");
        assert!(parse_sigv4("Bearer x").is_none());
    }

    #[test]
    fn jwt_round_trip() {
        let t = issue_unsigned_jwt(&json!({"oid": "o1", "tid": "t1"}));
        let c = jwt_claims(&t).unwrap();
        assert_eq!(c["oid"], "o1");
        assert_eq!(bearer(&format!("Bearer {t}")), Some(t.as_str()));
        assert!(jwt_claims("not-a-jwt").is_none());
    }

    #[test]
    fn oci() {
        let k = parse_oci_signature(r#"Signature version="1",keyId="ocid1.tenancy.oc1..aaa/ocid1.user.oc1..bbb/aa:bb",algorithm="rsa-sha256",headers="date",signature="x""#).unwrap();
        assert_eq!(k.tenancy, "ocid1.tenancy.oc1..aaa");
        assert_eq!(k.user, "ocid1.user.oc1..bbb");
        assert_eq!(k.fingerprint, "aa:bb");
    }
}
