// `*` matches any run (including empty), `?` exactly one char.
pub fn glob_match(pattern: &str, value: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let v: Vec<char> = value.chars().collect();
    let (mut pi, mut vi) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while vi < v.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == v[vi]) {
            pi += 1;
            vi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = vi;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            vi = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn form_decode(s: &str) -> String {
    percent_decode(&s.replace('+', " "))
}

pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (form_decode(k), form_decode(v)),
            None => (form_decode(kv), String::new()),
        })
        .collect()
}

pub fn snake_case(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev_lower =
                i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit());
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            let prev_upper = i > 0 && chars[i - 1].is_ascii_uppercase();
            if i > 0 && (prev_lower || (prev_upper && next_lower)) {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if *c == '-' || *c == '.' || *c == ' ' {
            out.push('_');
        } else {
            out.push(*c);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn globbing() {
        assert!(glob_match("*", ""));
        assert!(glob_match("s3:*", "s3:GetObject"));
        assert!(glob_match("s3:Get*", "s3:GetObject"));
        assert!(!glob_match("s3:Put*", "s3:GetObject"));
        assert!(glob_match("a?c", "abc"));
        assert!(glob_match("*b*d", "abcd"));
        assert!(!glob_match("*b*e", "abcd"));
    }

    #[test]
    fn decoding() {
        assert_eq!(percent_decode("a%2Fb%3a"), "a/b:");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(form_decode("a+b%20c"), "a b c");
        assert_eq!(percent_encode("a/b c"), "a%2Fb%20c");
        assert_eq!(
            parse_query("a=1&b=x%20y&c"),
            vec![
                ("a".into(), "1".into()),
                ("b".into(), "x y".into()),
                ("c".into(), String::new())
            ]
        );
    }

    #[test]
    fn snake() {
        assert_eq!(snake_case("CreateSecret"), "create_secret");
        assert_eq!(snake_case("GetHTTPEndpoint"), "get_http_endpoint");
        assert_eq!(
            snake_case("ResourceGroups_CreateOrUpdate"),
            "resource_groups_create_or_update"
        );
    }
}
