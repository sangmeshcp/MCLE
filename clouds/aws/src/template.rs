use std::collections::BTreeMap;

#[derive(Debug, PartialEq)]
enum Tok<'a> {
    Lit(&'a str),
    Var(&'a str),
}

fn tokens(t: &str) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    let mut rest = t;
    while let Some(open) = rest.find('{') {
        if open > 0 {
            out.push(Tok::Lit(&rest[..open]));
        }
        let Some(close) = rest[open..].find('}') else {
            out.push(Tok::Lit(&rest[open..]));
            return out;
        };
        out.push(Tok::Var(&rest[open + 1..open + close]));
        rest = &rest[open + close + 1..];
    }
    if !rest.is_empty() {
        out.push(Tok::Lit(rest));
    }
    out
}

fn rand_len(var: &str) -> Option<usize> {
    var.strip_prefix("rand").and_then(|n| n.parse().ok())
}

pub fn expand(t: &str, vars: &BTreeMap<&str, String>, rand: &dyn Fn(usize) -> String) -> String {
    tokens(t)
        .into_iter()
        .map(|tok| match tok {
            Tok::Lit(l) => l.to_string(),
            Tok::Var(v) => match rand_len(v) {
                Some(n) => rand(n),
                None => vars.get(v).cloned().unwrap_or_default(),
            },
        })
        .collect()
}

// Backtracking match. `{randN}` consumes exactly N alphanumerics; other
// variables consume the longest run without `:` that lets the rest match.
pub fn matches(t: &str, value: &str) -> Option<BTreeMap<String, String>> {
    fn go(toks: &[Tok<'_>], v: &str, acc: &mut BTreeMap<String, String>) -> bool {
        match toks.split_first() {
            None => v.is_empty(),
            Some((Tok::Lit(l), rest)) => v.strip_prefix(l).is_some_and(|r| go(rest, r, acc)),
            Some((Tok::Var(name), rest)) => {
                if let Some(n) = rand_len(name) {
                    return v.len() >= n
                        && v.is_char_boundary(n)
                        && v[..n].chars().all(|c| c.is_ascii_alphanumeric())
                        && go(rest, &v[n..], acc);
                }
                let ends: Vec<usize> = v
                    .char_indices()
                    .map(|(i, _)| i)
                    .skip(1)
                    .chain(std::iter::once(v.len()))
                    .collect();
                for end in ends.into_iter().rev() {
                    let cand = &v[..end];
                    if cand.contains(':') {
                        continue;
                    }
                    if go(rest, &v[end..], acc) {
                        acc.insert((*name).to_string(), cand.to_string());
                        return true;
                    }
                }
                false
            }
        }
    }
    let toks = tokens(t);
    let mut acc = BTreeMap::new();
    go(&toks, value, &mut acc).then_some(acc)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const T: &str = "arn:{partition}:secretsmanager:{region}:{account}:secret:{name}-{rand6}";

    #[test]
    fn expand_and_match() {
        let mut vars = BTreeMap::new();
        vars.insert("partition", "aws".to_string());
        vars.insert("region", "us-east-1".to_string());
        vars.insert("account", "000000000000".to_string());
        vars.insert("name", "prod/db-pass".to_string());
        let arn = expand(T, &vars, &|n| "x".repeat(n));
        assert_eq!(
            arn,
            "arn:aws:secretsmanager:us-east-1:000000000000:secret:prod/db-pass-xxxxxx"
        );
        let m = matches(T, &arn).unwrap();
        assert_eq!(m["name"], "prod/db-pass");
        assert_eq!(m["region"], "us-east-1");
        assert!(matches(T, "arn:aws:s3:::bucket").is_none());
    }
}
