use crate::util::percent_decode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteMatch {
    Rest {
        method: &'static str,
        path: &'static str,
        query: &'static [&'static str],
    },
    Target(&'static str),
    QueryAction(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct Route {
    pub op: OpId,
    pub matcher: RouteMatch,
}

#[derive(Clone, Copy, Debug)]
pub struct RouteTable {
    pub case_insensitive: bool,
    pub routes: &'static [Route],
}

#[derive(Clone, Debug, Default)]
pub struct RouteRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query_keys: Vec<&'a str>,
    pub target: Option<&'a str>,
    pub action: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteHit {
    pub op: OpId,
    pub params: Vec<(String, String)>,
    pub score: u32,
}

impl RouteTable {
    pub fn find(&self, req: &RouteRequest<'_>) -> Option<RouteHit> {
        let mut best: Option<RouteHit> = None;
        for r in self.routes {
            let hit = match r.matcher {
                RouteMatch::Target(t) => (req.target == Some(t)).then(|| RouteHit {
                    op: r.op,
                    params: vec![],
                    score: 10_000,
                }),
                RouteMatch::QueryAction(a) => (req.action == Some(a)).then(|| RouteHit {
                    op: r.op,
                    params: vec![],
                    score: 10_000,
                }),
                RouteMatch::Rest {
                    method,
                    path,
                    query,
                } => {
                    if !method.eq_ignore_ascii_case(req.method)
                        || !query.iter().all(|q| req.query_keys.contains(q))
                    {
                        None
                    } else {
                        match_path(path, req.path, self.case_insensitive).map(|(params, score)| {
                            RouteHit {
                                op: r.op,
                                params,
                                score: score + query.len() as u32 * 50,
                            }
                        })
                    }
                }
            };
            if let Some(h) = hit {
                if best.as_ref().is_none_or(|b| h.score > b.score) {
                    best = Some(h);
                }
            }
        }
        best
    }
}

fn split_path(p: &str) -> Vec<&str> {
    let p = p.strip_prefix('/').unwrap_or(p);
    let p = p.strip_suffix('/').unwrap_or(p);
    if p.is_empty() {
        Vec::new()
    } else {
        p.split('/').collect()
    }
}

// Template segments: `literal`, `{name}`, `{name+}` (greedy rest of path), or
// `prefix{name}suffix` (e.g. `{secret}:addVersion`). Returns decoded params and a
// specificity score; literal characters score higher so exact routes win.
pub fn match_path(
    template: &str,
    path: &str,
    case_insensitive: bool,
) -> Option<(Vec<(String, String)>, u32)> {
    let t = split_path(template);
    let p = split_path(path);
    let mut params = Vec::new();
    let mut score = 0u32;
    let eq = |a: &str, b: &str| {
        if case_insensitive {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let mut i = 0;
    while i < t.len() {
        let seg = t[i];
        if let (Some(open), Some(close)) = (seg.find('{'), seg.find('}')) {
            let prefix = &seg[..open];
            let suffix = &seg[close + 1..];
            let name = &seg[open + 1..close];
            if let Some(name) = name.strip_suffix('+') {
                if i >= p.len() {
                    return None;
                }
                let rest = p[i..].join("/");
                let rest = rest.strip_prefix(prefix)?;
                let rest = rest.strip_suffix(suffix)?;
                if rest.is_empty() {
                    return None;
                }
                params.push((name.to_string(), percent_decode(rest)));
                score += 1;
                return Some((params, score));
            }
            let actual = *p.get(i)?;
            if actual.len() < prefix.len() + suffix.len() {
                return None;
            }
            let ap = actual.get(..prefix.len())?;
            let val = actual.get(prefix.len()..actual.len() - suffix.len())?;
            let asfx = actual.get(actual.len() - suffix.len()..)?;
            if !eq(ap, prefix) || !eq(asfx, suffix) || val.is_empty() {
                return None;
            }
            score += 2 + (prefix.len() + suffix.len()) as u32 * 5;
            params.push((name.to_string(), percent_decode(val)));
        } else {
            if !eq(seg, p.get(i)?) {
                return None;
            }
            score += 10;
        }
        i += 1;
    }
    (t.len() == p.len()).then_some((params, score))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    static ROUTES: RouteTable = RouteTable {
        case_insensitive: false,
        routes: &[
            Route {
                op: OpId(0),
                matcher: RouteMatch::Rest {
                    method: "GET",
                    path: "/v1/projects/{project}/secrets/{secret}",
                    query: &[],
                },
            },
            Route {
                op: OpId(1),
                matcher: RouteMatch::Rest {
                    method: "POST",
                    path: "/v1/projects/{project}/secrets/{secret}:addVersion",
                    query: &[],
                },
            },
            Route {
                op: OpId(2),
                matcher: RouteMatch::Rest {
                    method: "GET",
                    path: "/{Bucket}/{Key+}",
                    query: &[],
                },
            },
            Route {
                op: OpId(3),
                matcher: RouteMatch::Rest {
                    method: "POST",
                    path: "/{Bucket}/{Key+}",
                    query: &["uploads"],
                },
            },
            Route {
                op: OpId(4),
                matcher: RouteMatch::Target("secretsmanager.CreateSecret"),
            },
            Route {
                op: OpId(5),
                matcher: RouteMatch::Rest {
                    method: "POST",
                    path: "/v1/projects/{project}/secrets/{secret}",
                    query: &[],
                },
            },
        ],
    };

    fn req<'a>(m: &'a str, p: &'a str) -> RouteRequest<'a> {
        RouteRequest {
            method: m,
            path: p,
            ..Default::default()
        }
    }

    #[test]
    fn rest_and_verbs() {
        let h = ROUTES
            .find(&req("GET", "/v1/projects/p/secrets/s"))
            .unwrap();
        assert_eq!(h.op, OpId(0));
        assert_eq!(
            h.params,
            vec![
                ("project".into(), "p".into()),
                ("secret".into(), "s".into())
            ]
        );
        let h = ROUTES
            .find(&req("POST", "/v1/projects/p/secrets/s:addVersion"))
            .unwrap();
        assert_eq!(h.op, OpId(1));
        assert_eq!(h.params[1].1, "s");
    }

    #[test]
    fn greedy_and_query() {
        let h = ROUTES.find(&req("GET", "/b/a/b%20c")).unwrap();
        assert_eq!(h.op, OpId(2));
        assert_eq!(h.params[1], ("Key".into(), "a/b c".into()));
        let mut r = req("POST", "/b/k");
        r.query_keys = vec!["uploads"];
        assert_eq!(ROUTES.find(&r).unwrap().op, OpId(3));
        assert!(ROUTES.find(&req("POST", "/b/k")).is_none());
    }

    #[test]
    fn target() {
        let r = RouteRequest {
            method: "POST",
            path: "/",
            target: Some("secretsmanager.CreateSecret"),
            ..Default::default()
        };
        assert_eq!(ROUTES.find(&r).unwrap().op, OpId(4));
    }

    #[test]
    fn trailing_slash_and_case() {
        assert!(match_path("/n/{ns}/b/", "/n/x/b", false).is_some());
        assert!(match_path("/Subscriptions/{s}", "/subscriptions/1", true).is_some());
        assert!(match_path("/Subscriptions/{s}", "/subscriptions/1", false).is_none());
    }
}
