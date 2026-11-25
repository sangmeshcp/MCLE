#[derive(Debug, Clone, PartialEq)]
pub enum Matcher {
    Rest {
        method: String,
        path: String,
        query: Vec<String>,
    },
    Target(String),
    QueryAction(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verb {
    Create,
    Read,
    Update,
    Delete,
    List,
    Upsert,
}

impl Verb {
    pub fn parse(s: &str) -> Option<Verb> {
        Some(match s.to_ascii_lowercase().as_str() {
            "create" => Verb::Create,
            "read" | "get" => Verb::Read,
            "update" | "patch" => Verb::Update,
            "delete" => Verb::Delete,
            "list" => Verb::List,
            "upsert" | "put" => Verb::Upsert,
            _ => return None,
        })
    }
    pub fn rust(&self) -> &'static str {
        match self {
            Verb::Create => "Create",
            Verb::Read => "Read",
            Verb::Update => "Update",
            Verb::Delete => "Delete",
            Verb::List => "List",
            Verb::Upsert => "Upsert",
        }
    }
}

#[derive(Debug, Clone)]
pub struct IrOp {
    pub name: String,
    pub matcher: Matcher,
    pub iam_action: String,
    pub crud_guess: Option<(String, Verb)>,
    pub lro: String,
    pub last_path_param: Option<String>,
    pub idempotency: String,
}

#[derive(Debug, Clone)]
pub struct IrService {
    pub protocol: String,
    pub signing_name: String,
    pub ops: Vec<IrOp>,
}

pub fn guess_crud(op_id: &str) -> Option<(String, Verb)> {
    let (group, action) = match op_id.split_once('_') {
        Some((g, a)) if g.chars().next().is_some_and(|c| c.is_ascii_uppercase()) => (Some(g), a),
        _ => (None, op_id),
    };
    let snaked = snake(action);
    let mut words: Vec<&str> = snaked.split('_').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return None;
    }
    let (verb, consumed) = match (
        words.first().copied(),
        words.get(1).copied(),
        words.get(2).copied(),
    ) {
        (Some("create"), Some("or"), Some("update")) => (Verb::Upsert, 3),
        (Some("create"), _, _) => (Verb::Create, 1),
        (Some("get"), _, _) | (Some("describe"), _, _) | (Some("read"), _, _) => (Verb::Read, 1),
        (Some("update"), _, _) | (Some("modify"), _, _) | (Some("patch"), _, _) => {
            (Verb::Update, 1)
        }
        (Some("delete"), _, _) | (Some("remove"), _, _) => (Verb::Delete, 1),
        (Some("list"), _, _) => (Verb::List, 1),
        (Some("put"), _, _) => (Verb::Upsert, 1),
        _ => return None,
    };
    words.drain(..consumed);
    let base = match group {
        Some(g) if words.is_empty() || words == ["by", "id"] => snake(g),
        _ => words.join("_"),
    };
    let base = base
        .trim_end_matches("_by_resource_group")
        .trim_end_matches("_by_subscription")
        .to_string();
    if base.is_empty() {
        return None;
    }
    Some((singular(&base), verb))
}

pub fn singular(s: &str) -> String {
    if let Some(stem) = s.strip_suffix("ies") {
        format!("{stem}y")
    } else if s.ends_with("sses") || s.ends_with("uses") {
        s[..s.len() - 2].to_string()
    } else if s.ends_with('s') && !s.ends_with("ss") {
        s[..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

pub fn snake(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev_lower =
                i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit());
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            let prev_upper = i > 0 && chars[i - 1].is_ascii_uppercase();
            if i > 0 && (prev_lower || (prev_upper && next_lower)) && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if matches!(c, '-' | '.' | ' ') {
            out.push('_');
        } else {
            out.push(*c);
        }
    }
    out
}

pub fn last_path_param(path: &str) -> Option<String> {
    path.split('/').rev().find_map(|seg| {
        let s = seg.find('{')?;
        let e = seg.find('}')?;
        let name = &seg[s + 1..e];
        (!name.ends_with('+')).then(|| name.to_string())
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn crud_guesses() {
        assert_eq!(
            guess_crud("CreateSecret"),
            Some(("secret".into(), Verb::Create))
        );
        assert_eq!(
            guess_crud("ListSecrets"),
            Some(("secret".into(), Verb::List))
        );
        assert_eq!(
            guess_crud("DescribeSecret"),
            Some(("secret".into(), Verb::Read))
        );
        assert_eq!(
            guess_crud("createBucket"),
            Some(("bucket".into(), Verb::Create))
        );
        assert_eq!(
            guess_crud("list_resource_instances"),
            Some(("resource_instance".into(), Verb::List))
        );
        assert_eq!(
            guess_crud("ResourceGroups_CreateOrUpdate"),
            Some(("resource_group".into(), Verb::Upsert))
        );
        assert_eq!(
            guess_crud("ResourceGroups_List"),
            Some(("resource_group".into(), Verb::List))
        );
        assert_eq!(guess_crud("TagResource"), None);
        assert_eq!(
            last_path_param("/v1/projects/{p}/secrets/{s}:addVersion"),
            Some("s".into())
        );
        assert_eq!(last_path_param("/{Bucket}/{Key+}"), Some("Bucket".into()));
    }
}
