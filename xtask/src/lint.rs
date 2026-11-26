use crate::manifest::load_all;
use crate::markers::{satisfies, verified};
use crate::resolve::resolve;
use anyhow::{bail, Result};

pub fn run() -> Result<()> {
    let mut errors = Vec::new();
    let mut n = 0;
    for e in load_all()? {
        let r = resolve(&e)?;
        let v = verified(&e);
        let id = format!("{}/{}", e.toml.service.cloud, e.toml.service.name);
        for k in &r.unknown_overrides {
            errors.push(format!(
                "{id}: [ops.{k}] does not match any operation in the spec"
            ));
        }
        for op in &r.ops {
            n += 1;
            if op.iam_action.trim().is_empty() {
                errors.push(format!("{id}/{}: missing iam_action", op.name));
            }
            if let (Some(cat), false) = (&r.catalog, op.level == "L0") {
                if !cat.actions.is_empty()
                    && !op.iam_action.contains('{')
                    && !cat.actions.iter().any(|a| a == &op.iam_action)
                {
                    errors.push(format!(
                        "{id}/{}: iam_action `{}` not in IAM catalog",
                        op.name, op.iam_action
                    ));
                }
            }
            if !["L0", "L1", "L2", "L3"].contains(&op.level.as_str()) {
                errors.push(format!("{id}/{}: bad level `{}`", op.name, op.level));
            }
            if op.level != "L0" && op.crud.is_none() && !op.custom {
                errors.push(format!(
                    "{id}/{}: claims {} but has no CRUD binding and is not `custom`",
                    op.name, op.level
                ));
            }
            if op.custom && e.toml.service.facade != "custom" {
                errors.push(format!(
                    "{id}/{}: `custom = true` requires `facade = \"custom\"`",
                    op.name
                ));
            }
            if op.level != "L0" && !satisfies(v.get(&op.name), &op.level) {
                errors.push(format!(
                    "{id}/{}: claims {} without a `verifies({})` test marker",
                    op.name, op.level, op.level
                ));
            }
            if op.emits.iter().any(|x| x.trim().is_empty()) {
                errors.push(format!("{id}/{}: empty event kind in `emits`", op.name));
            }
        }
    }
    if !errors.is_empty() {
        bail!("manifest lint failed:\n  {}", errors.join("\n  "));
    }
    println!("lint-manifests: ok ({n} ops)");
    Ok(())
}
