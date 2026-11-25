use crate::manifest::{root, ServiceEntry};
use std::collections::BTreeMap;
use std::path::Path;

fn files(dir: &Path, ext: &str, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

fn level_rank(l: &str) -> u8 {
    l.trim_start_matches('L').parse().unwrap_or(0)
}

fn parse_line(line: &str) -> Option<(String, &str)> {
    let i = line.find("verifies(")?;
    let rest = &line[i + "verifies(".len()..];
    let close = rest.find(')')?;
    let level = rest[..close].trim().to_string();
    let after = rest[close + 1..].trim_start().strip_prefix(':')?.trim();
    Some((level, after))
}

pub fn verified(entry: &ServiceEntry) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut put = |op: &str, lvl: &str| {
        let op = op.trim().to_string();
        if op.is_empty() {
            return;
        }
        let e = out.entry(op).or_insert_with(|| lvl.to_string());
        if level_rank(lvl) > level_rank(e) {
            *e = lvl.to_string();
        }
    };
    let mut rs = Vec::new();
    files(&entry.dir.join("tests"), "rs", &mut rs);
    for f in rs {
        for line in std::fs::read_to_string(&f).unwrap_or_default().lines() {
            if let Some((lvl, ops)) = parse_line(line) {
                ops.split(',').for_each(|o| put(o, &lvl));
            }
        }
    }
    let key = format!("{}/{}:", entry.toml.service.cloud, entry.toml.service.name);
    let mut py = Vec::new();
    files(&root().join("tests").join("sdk"), "py", &mut py);
    for f in py {
        for line in std::fs::read_to_string(&f).unwrap_or_default().lines() {
            if let Some((lvl, rest)) = parse_line(line) {
                if let Some(ops) = rest.strip_prefix(&key) {
                    ops.split(',').for_each(|o| put(o, &lvl));
                }
            }
        }
    }
    out
}

pub fn satisfies(have: Option<&String>, need: &str) -> bool {
    have.is_some_and(|h| level_rank(h) >= level_rank(need))
}
