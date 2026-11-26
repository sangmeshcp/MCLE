use crate::cloud::CloudId;
use crate::util::glob_match;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ResourceId {
    Arn {
        partition: String,
        service: String,
        region: String,
        account: String,
        resource: String,
    },
    ArmId(String),
    GcpName {
        service: Option<String>,
        path: String,
    },
    Ocid {
        resource_type: String,
        realm: String,
        region: String,
        future: Option<String>,
        unique: String,
    },
    Crn {
        version: String,
        cname: String,
        ctype: String,
        service_name: String,
        location: String,
        scope: String,
        service_instance: String,
        resource_type: String,
        resource: String,
    },
    Ext {
        cloud: CloudId,
        raw: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid resource id `{input}`: {reason}")]
pub struct ParseResourceIdError {
    pub input: String,
    pub reason: &'static str,
}

impl ResourceId {
    pub fn parse(s: &str) -> Result<Self, ParseResourceIdError> {
        let err = |reason| ParseResourceIdError {
            input: s.to_string(),
            reason,
        };
        if let Some(rest) = s.strip_prefix("arn:") {
            let p: Vec<&str> = rest.splitn(5, ':').collect();
            if p.len() != 5 || p[0].is_empty() || p[1].is_empty() {
                return Err(err("ARN needs 6 `:`-separated parts"));
            }
            return Ok(ResourceId::Arn {
                partition: p[0].into(),
                service: p[1].into(),
                region: p[2].into(),
                account: p[3].into(),
                resource: p[4].into(),
            });
        }
        if let Some(rest) = s.strip_prefix("crn:") {
            let p: Vec<&str> = rest.splitn(9, ':').collect();
            if p.len() != 9 {
                return Err(err("CRN needs 10 `:`-separated parts"));
            }
            return Ok(ResourceId::Crn {
                version: p[0].into(),
                cname: p[1].into(),
                ctype: p[2].into(),
                service_name: p[3].into(),
                location: p[4].into(),
                scope: p[5].into(),
                service_instance: p[6].into(),
                resource_type: p[7].into(),
                resource: p[8].into(),
            });
        }
        if let Some(rest) = s.strip_prefix("ocid1.") {
            let p: Vec<&str> = rest.split('.').collect();
            return match p.len() {
                4 => Ok(ResourceId::Ocid {
                    resource_type: p[0].into(),
                    realm: p[1].into(),
                    region: p[2].into(),
                    future: None,
                    unique: p[3].into(),
                }),
                5 => Ok(ResourceId::Ocid {
                    resource_type: p[0].into(),
                    realm: p[1].into(),
                    region: p[2].into(),
                    future: Some(p[3].into()),
                    unique: p[4].into(),
                }),
                _ => Err(err("OCID needs 5 or 6 `.`-separated parts")),
            };
        }
        if s.starts_with('/') && !s.starts_with("//") {
            if s.len() < 2 {
                return Err(err("empty ARM id"));
            }
            return Ok(ResourceId::ArmId(s.to_string()));
        }
        if let Some(rest) = s.strip_prefix("//") {
            let (svc, path) = rest
                .split_once('/')
                .ok_or_else(|| err("full name needs a path"))?;
            if svc.is_empty() || path.is_empty() {
                return Err(err("full name needs service and path"));
            }
            return Ok(ResourceId::GcpName {
                service: Some(svc.into()),
                path: path.into(),
            });
        }
        if ["projects/", "organizations/", "folders/", "locations/"]
            .iter()
            .any(|p| s.starts_with(p))
        {
            return Ok(ResourceId::GcpName {
                service: None,
                path: s.into(),
            });
        }
        Err(err("unrecognised identifier format"))
    }

    pub fn cloud(&self) -> CloudId {
        match self {
            ResourceId::Arn { .. } => CloudId::AWS,
            ResourceId::ArmId(_) => CloudId::AZURE,
            ResourceId::GcpName { .. } => CloudId::GCP,
            ResourceId::Ocid { .. } => CloudId::OCI,
            ResourceId::Crn { .. } => CloudId::IBM,
            ResourceId::Ext { cloud, .. } => cloud.clone(),
        }
    }

    pub fn matches(&self, pattern: &str) -> bool {
        let me = self.to_string();
        match self {
            ResourceId::ArmId(_) => {
                glob_match(&pattern.to_ascii_lowercase(), &me.to_ascii_lowercase())
            }
            _ => glob_match(pattern, &me),
        }
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceId::Arn {
                partition,
                service,
                region,
                account,
                resource,
            } => write!(f, "arn:{partition}:{service}:{region}:{account}:{resource}"),
            ResourceId::ArmId(s) => f.write_str(s),
            ResourceId::GcpName { service, path } => match service {
                Some(svc) => write!(f, "//{svc}/{path}"),
                None => f.write_str(path),
            },
            ResourceId::Ocid {
                resource_type,
                realm,
                region,
                future,
                unique,
            } => match future {
                Some(fu) => write!(f, "ocid1.{resource_type}.{realm}.{region}.{fu}.{unique}"),
                None => write!(f, "ocid1.{resource_type}.{realm}.{region}.{unique}"),
            },
            ResourceId::Crn {
                version,
                cname,
                ctype,
                service_name,
                location,
                scope,
                service_instance,
                resource_type,
                resource,
            } => write!(
                f,
                "crn:{version}:{cname}:{ctype}:{service_name}:{location}:{scope}:{service_instance}:{resource_type}:{resource}"
            ),
            ResourceId::Ext { raw, .. } => f.write_str(raw),
        }
    }
}

impl FromStr for ResourceId {
    type Err = ParseResourceIdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for ResourceId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ResourceId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ResourceId::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn known_examples_round_trip() {
        for s in [
            "arn:aws:secretsmanager:us-east-1:000000000000:secret:db-AbCdEf",
            "arn:aws:s3:::my-bucket/key/with:colon",
            "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg/providers/Microsoft.KeyVault/vaults/kv",
            "projects/p/secrets/s",
            "//secretmanager.googleapis.com/projects/p/secrets/s",
            "ocid1.bucket.oc1.iad.aaaaaaaa",
            "ocid1.tenancy.oc1..aaaaaaaa",
            "ocid1.instance.oc1.phx.abc.uniq",
            "crn:v1:bluemix:public:cloud-object-storage:global:a/acc:guid::",
        ] {
            let id = ResourceId::parse(s).unwrap();
            assert_eq!(id.to_string(), s);
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(ResourceId::parse("hello").is_err());
        assert!(ResourceId::parse("arn:aws").is_err());
        assert!(ResourceId::parse("ocid1.x").is_err());
    }

    #[test]
    fn wildcard_matching() {
        let id = ResourceId::parse("arn:aws:s3:::bucket/a/b").unwrap();
        assert!(id.matches("arn:aws:s3:::bucket/*"));
        assert!(!id.matches("arn:aws:s3:::other/*"));
        let arm = ResourceId::parse("/subscriptions/S/resourceGroups/RG").unwrap();
        assert!(arm.matches("/subscriptions/s/resourcegroups/*"));
    }

    fn seg() -> impl Strategy<Value = String> {
        "[a-z0-9-]{1,12}"
    }

    proptest! {
        #[test]
        fn arn_round_trip(p in seg(), s in seg(), r in "[a-z0-9-]{0,10}", a in "[0-9]{0,12}", res in "[a-zA-Z0-9:/_-]{1,30}") {
            let txt = format!("arn:{p}:{s}:{r}:{a}:{res}");
            let id = ResourceId::parse(&txt).unwrap();
            prop_assert_eq!(id.to_string(), txt);
        }

        #[test]
        fn crn_round_trip(parts in proptest::collection::vec("[a-z0-9/-]{0,8}", 9)) {
            let txt = format!("crn:{}", parts.join(":"));
            let id = ResourceId::parse(&txt).unwrap();
            prop_assert_eq!(id.to_string(), txt);
        }

        #[test]
        fn ocid_round_trip(t in seg(), realm in seg(), region in "[a-z0-9-]{0,6}", u in "[a-z0-9]{1,20}") {
            let txt = format!("ocid1.{t}.{realm}.{region}.{u}");
            let id = ResourceId::parse(&txt).unwrap();
            prop_assert_eq!(id.to_string(), txt);
        }

        #[test]
        fn arm_round_trip(segs in proptest::collection::vec("[A-Za-z0-9.-]{1,10}", 1..8)) {
            let txt = format!("/subscriptions/{}", segs.join("/"));
            let id = ResourceId::parse(&txt).unwrap();
            prop_assert_eq!(id.to_string(), txt);
        }

        #[test]
        fn gcp_round_trip(segs in proptest::collection::vec("[a-z0-9-]{1,10}", 1..6), full in any::<bool>()) {
            let path = format!("projects/{}", segs.join("/"));
            let txt = if full { format!("//svc.googleapis.com/{path}") } else { path };
            let id = ResourceId::parse(&txt).unwrap();
            prop_assert_eq!(id.to_string(), txt);
        }
    }
}
