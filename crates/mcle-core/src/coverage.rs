use mcle_sdk::{FidelityLevel, Plane};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoverageEntry {
    pub request_id: String,
    pub sandbox: String,
    pub cloud: String,
    pub service: String,
    pub op: String,
    pub level: FidelityLevel,
    pub plane: Plane,
    pub outcome: String,
    pub status: u16,
    pub iam: String,
    pub fault: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OpSummary {
    pub cloud: String,
    pub service: String,
    pub op: String,
    pub level: Option<FidelityLevel>,
    pub calls: u64,
    pub errors: u64,
    pub iam_denials: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strict {
    L1,
    L2,
}

impl Strict {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "" | "L2" => Some(Strict::L2),
            "L1" => Some(Strict::L1),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct CoverageRecorder {
    entries: Mutex<Vec<CoverageEntry>>,
}

impl CoverageRecorder {
    pub fn record(&self, e: CoverageEntry) {
        if let Ok(mut g) = self.entries.lock() {
            g.push(e);
        }
    }

    pub fn entries(&self, sandbox: Option<&str>) -> Vec<CoverageEntry> {
        let g = match self.entries.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        g.iter()
            .filter(|e| sandbox.is_none_or(|s| e.sandbox == s))
            .cloned()
            .collect()
    }

    pub fn reset(&self, sandbox: Option<&str>) {
        if let Ok(mut g) = self.entries.lock() {
            match sandbox {
                Some(s) => g.retain(|e| e.sandbox != s),
                None => g.clear(),
            }
        }
    }

    pub fn violations(&self, strict: Strict, sandbox: Option<&str>) -> Vec<String> {
        violations(&self.entries(sandbox), strict)
    }
}

pub fn violations(entries: &[CoverageEntry], strict: Strict) -> Vec<String> {
    let mut out = BTreeMap::new();
    for e in entries {
        let bad = e.level == FidelityLevel::L0
            || (strict == Strict::L2 && e.level == FidelityLevel::L1 && e.plane == Plane::Data)
            || e.iam == "deny";
        if bad {
            let why = if e.iam == "deny" {
                "IAM denied".to_string()
            } else {
                format!("fidelity {}", e.level)
            };
            out.insert(format!("{}/{}/{}: {why}", e.cloud, e.service, e.op), ());
        }
    }
    out.into_keys().collect()
}

pub fn summarize(entries: &[CoverageEntry]) -> Vec<OpSummary> {
    let mut m: BTreeMap<(String, String, String), OpSummary> = BTreeMap::new();
    for e in entries {
        let s = m
            .entry((e.cloud.clone(), e.service.clone(), e.op.clone()))
            .or_insert_with(|| OpSummary {
                cloud: e.cloud.clone(),
                service: e.service.clone(),
                op: e.op.clone(),
                level: Some(e.level),
                ..Default::default()
            });
        s.calls += 1;
        if e.outcome != "ok" {
            s.errors += 1;
        }
        if e.iam == "deny" || e.iam == "would-deny" {
            s.iam_denials += 1;
        }
    }
    m.into_values().collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn render(entries: &[CoverageEntry], format: &str, strict: Option<Strict>) -> String {
    let rows = summarize(entries);
    let viol = strict.map(|s| violations(entries, s)).unwrap_or_default();
    match format {
        "json" => serde_json::json!({
            "calls": entries.len(),
            "ops": rows,
            "violations": viol,
            "entries": entries,
        })
        .to_string(),
        "junit" => {
            let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            let failures = rows
                .iter()
                .filter(|r| {
                    viol.iter()
                        .any(|v| v.starts_with(&format!("{}/{}/{}:", r.cloud, r.service, r.op)))
                })
                .count();
            x.push_str(&format!(
                "<testsuite name=\"mcle-coverage\" tests=\"{}\" failures=\"{failures}\">\n",
                rows.len()
            ));
            for r in &rows {
                let key = format!("{}/{}/{}", r.cloud, r.service, r.op);
                x.push_str(&format!(
                    "  <testcase classname=\"{}.{}\" name=\"{}\">",
                    xml_escape(&r.cloud),
                    xml_escape(&r.service),
                    xml_escape(&r.op)
                ));
                if let Some(v) = viol.iter().find(|v| v.starts_with(&format!("{key}:"))) {
                    x.push_str(&format!("<failure message=\"{}\"/>", xml_escape(v)));
                }
                x.push_str("</testcase>\n");
            }
            x.push_str("</testsuite>\n");
            x
        }
        _ => {
            let mut s = String::from("# MCLE coverage report\n\n");
            s.push_str(&format!(
                "{} calls, {} distinct ops\n\n",
                entries.len(),
                rows.len()
            ));
            s.push_str("| cloud | service | op | level | calls | errors | IAM denials |\n");
            s.push_str("|---|---|---|---|---|---|---|\n");
            for r in &rows {
                s.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} | {} |\n",
                    r.cloud,
                    r.service,
                    r.op,
                    r.level.map(|l| l.to_string()).unwrap_or_default(),
                    r.calls,
                    r.errors,
                    r.iam_denials
                ));
            }
            if !viol.is_empty() {
                s.push_str("\n## Strict-mode violations\n\n");
                for v in &viol {
                    s.push_str(&format!("- {v}\n"));
                }
            }
            s
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn e(op: &str, level: FidelityLevel, plane: Plane) -> CoverageEntry {
        CoverageEntry {
            request_id: "r".into(),
            sandbox: "default".into(),
            cloud: "aws".into(),
            service: "s3".into(),
            op: op.into(),
            level,
            plane,
            outcome: "ok".into(),
            status: 200,
            iam: "allow".into(),
            fault: None,
        }
    }

    #[test]
    fn strict_levels() {
        let es = vec![
            e("CreateBucket", FidelityLevel::L1, Plane::Control),
            e("PutObject", FidelityLevel::L1, Plane::Data),
            e("SelectObjectContent", FidelityLevel::L0, Plane::Data),
        ];
        assert_eq!(violations(&es, Strict::L1).len(), 1);
        assert_eq!(violations(&es, Strict::L2).len(), 2);
        let md = render(&es, "md", Some(Strict::L2));
        assert!(md.contains("PutObject"));
        let junit = render(&es, "junit", Some(Strict::L2));
        assert!(junit.contains("failures=\"2\""));
        let json: serde_json::Value = serde_json::from_str(&render(&es, "json", None)).unwrap();
        assert_eq!(json["calls"], 3);
    }
}
