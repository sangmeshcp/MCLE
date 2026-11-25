use mcle_sdk::io::RequestHead;
use mcle_sdk::traits::DetectionRule;
use mcle_sdk::CloudAdapter;
use std::sync::Arc;

// Precedence: x-mcle-cloud header, then host rules, auth header, other headers,
// path prefix, and finally the fallback adapter.
pub fn detect(
    adapters: &[Arc<dyn CloudAdapter>],
    head: &RequestHead,
) -> Option<Arc<dyn CloudAdapter>> {
    if let Some(c) = head.header("x-mcle-cloud") {
        return adapters
            .iter()
            .find(|a| a.cloud().as_str().eq_ignore_ascii_case(c))
            .cloned();
    }
    let host = head.host.to_ascii_lowercase();
    let auth = head.header("authorization").unwrap_or("");
    type Pred<'a> = Box<dyn Fn(&DetectionRule) -> bool + 'a>;
    let passes: [Pred<'_>; 4] = [
        Box::new(|r| match r {
            DetectionRule::HostSuffix(s) => host.ends_with(s),
            DetectionRule::HostContains(s) => host.contains(s),
            _ => false,
        }),
        Box::new(|r| matches!(r, DetectionRule::AuthPrefix(p) if auth.starts_with(p))),
        Box::new(|r| matches!(r, DetectionRule::Header(h) if head.header(h).is_some())),
        Box::new(|r| matches!(r, DetectionRule::PathPrefix(p) if head.path.starts_with(p))),
    ];
    for pass in &passes {
        if let Some(a) = adapters
            .iter()
            .find(|a| a.detection_rules().iter().any(pass))
        {
            return Some(a.clone());
        }
    }
    adapters.iter().find(|a| a.is_fallback()).cloned()
}
