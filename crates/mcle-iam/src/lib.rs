#![forbid(unsafe_code)]

use mcle_sdk::{AuthzEvaluator, AuthzRequest, CloudId, Decision, IamMode, PolicySet};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug)]
pub struct AllowAll(pub CloudId);

impl AuthzEvaluator for AllowAll {
    fn cloud(&self) -> CloudId {
        self.0.clone()
    }
    fn evaluate(&self, _req: &AuthzRequest, _p: &PolicySet) -> Decision {
        Decision::Allow {
            matched: vec!["mcle:no-evaluator".into()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub decision: Decision,
    pub blocked: bool,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Explain {
    pub request: AuthzRequest,
    pub outcome: Outcome,
}

pub trait PolicySource: Send + Sync {
    fn policies(&self, req: &AuthzRequest) -> PolicySet;
}

pub struct IamEngine {
    mode: RwLock<IamMode>,
    evaluators: RwLock<HashMap<CloudId, Arc<dyn AuthzEvaluator>>>,
    sources: RwLock<Vec<Arc<dyn PolicySource>>>,
    explain: Mutex<(VecDeque<String>, BTreeMap<String, Explain>)>,
    explain_cap: usize,
}

impl IamEngine {
    pub fn new(mode: IamMode) -> Self {
        IamEngine {
            mode: RwLock::new(mode),
            evaluators: RwLock::new(HashMap::new()),
            sources: RwLock::new(Vec::new()),
            explain: Mutex::new((VecDeque::new(), BTreeMap::new())),
            explain_cap: 10_000,
        }
    }

    pub fn mode(&self) -> IamMode {
        self.mode.read().map(|m| *m).unwrap_or_default()
    }

    pub fn set_mode(&self, m: IamMode) {
        if let Ok(mut g) = self.mode.write() {
            *g = m;
        }
    }

    pub fn register(&self, e: Arc<dyn AuthzEvaluator>) {
        if let Ok(mut g) = self.evaluators.write() {
            g.insert(e.cloud(), e);
        }
    }

    pub fn add_source(&self, s: Arc<dyn PolicySource>) {
        if let Ok(mut g) = self.sources.write() {
            g.push(s);
        }
    }

    pub fn authorize(&self, request_id: &str, req: AuthzRequest) -> Outcome {
        let mode = self.mode();
        let outcome = if mode == IamMode::Off {
            Outcome {
                decision: Decision::Allow { matched: vec![] },
                blocked: false,
                label: "off".into(),
            }
        } else {
            let ev = self
                .evaluators
                .read()
                .ok()
                .and_then(|g| g.get(&req.cloud).cloned());
            let mut policies = PolicySet::default();
            if let Ok(srcs) = self.sources.read() {
                for s in srcs.iter() {
                    policies.policies.extend(s.policies(&req).policies);
                }
            }
            let decision = match ev {
                Some(e) => e.evaluate(&req, &policies),
                None => AllowAll(req.cloud.clone()).evaluate(&req, &policies),
            };
            let allow = decision.is_allow();
            Outcome {
                decision,
                blocked: !allow && mode == IamMode::Enforce,
                label: match (allow, mode) {
                    (true, _) => "allow".into(),
                    (false, IamMode::Enforce) => "deny".into(),
                    (false, _) => "would-deny".into(),
                },
            }
        };
        if let Ok(mut g) = self.explain.lock() {
            let (order, map) = &mut *g;
            order.push_back(request_id.to_string());
            map.insert(
                request_id.to_string(),
                Explain {
                    request: req,
                    outcome: outcome.clone(),
                },
            );
            while order.len() > self.explain_cap {
                if let Some(old) = order.pop_front() {
                    map.remove(&old);
                }
            }
        }
        outcome
    }

    pub fn explain(&self, request_id: &str) -> Option<Explain> {
        self.explain.lock().ok()?.1.get(request_id).cloned()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use mcle_sdk::{Principal, Scope};

    struct DenyAll;
    impl AuthzEvaluator for DenyAll {
        fn cloud(&self) -> CloudId {
            CloudId::AWS
        }
        fn evaluate(&self, _r: &AuthzRequest, _p: &PolicySet) -> Decision {
            Decision::Deny {
                reason: "implicit".into(),
                matched: vec![],
            }
        }
    }

    fn req() -> AuthzRequest {
        AuthzRequest {
            cloud: CloudId::AWS,
            principal: Principal::anonymous(),
            action: "s3:GetObject".into(),
            resource: None,
            scope: Scope::Aws {
                account: "1".into(),
                region: "r".into(),
            },
            context: BTreeMap::new(),
        }
    }

    #[test]
    fn modes() {
        let e = IamEngine::new(IamMode::Audit);
        e.register(Arc::new(DenyAll));
        let o = e.authorize("r1", req());
        assert!(!o.blocked);
        assert_eq!(o.label, "would-deny");
        e.set_mode(IamMode::Enforce);
        assert!(e.authorize("r2", req()).blocked);
        e.set_mode(IamMode::Off);
        assert_eq!(e.authorize("r3", req()).label, "off");
        assert_eq!(e.explain("r2").unwrap().outcome.label, "deny");
    }
}
