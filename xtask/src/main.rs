#![forbid(unsafe_code)]

mod bundle;
mod deps;
mod dist;
mod gen;
mod ir;
mod lint;
mod manifest;
mod markers;
mod resolve;
mod scaffold;
mod shapes;
mod spec;
mod status;

use anyhow::{bail, Result};

const HELP: &str = "cargo xtask <command>

Commands:
  codegen [--check]        generate services/*/*/gen/mod.rs from specs + service.toml
  bundle [--check]         generate crates/mcle-bundle (plugin registry + features)
  status [--check]         generate STATUS.md and .github/CODEOWNERS
  check-deps               enforce core/sdk/engines never depend on clouds/services
  lint-manifests           every op declared; every fidelity claim test-backed
  new-service --cloud C --service S [--tier T0|T1|T2] [--format smithy|openapi|discovery]
  dist --target T [--out DIR] [--binary PATH]
                           build (or take PATH) and package mcle-<version>-<T>.tar.gz
  dist --checksums DIR     write DIR/SHA256SUMS for all archives in DIR
  ci                       all of: codegen --check, bundle --check, status --check, check-deps, lint-manifests
  tf-test --cloud C        (P3) Terraform check pipeline
  parity-record            (P4) record parity fixtures against a real account
";

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    match args.first().map(String::as_str) {
        Some("codegen") => gen::run(check),
        Some("bundle") => bundle::run(check),
        Some("status") => status::run(check),
        Some("check-deps") => deps::run(),
        Some("lint-manifests") => lint::run(),
        Some("new-service") => {
            let (Some(c), Some(s)) = (arg(&args, "--cloud"), arg(&args, "--service")) else {
                bail!("usage: cargo xtask new-service --cloud <c> --service <s> [--tier T1]");
            };
            scaffold::run(
                &c,
                &s,
                &arg(&args, "--tier").unwrap_or_else(|| "T2".into()),
                arg(&args, "--format").as_deref(),
            )
        }
        Some("dist") => {
            if let Some(dir) = arg(&args, "--checksums") {
                return dist::checksums(std::path::Path::new(&dir));
            }
            let Some(target) = arg(&args, "--target") else {
                bail!("usage: cargo xtask dist --target <triple> [--out dist] [--binary path]");
            };
            let out =
                std::path::PathBuf::from(arg(&args, "--out").unwrap_or_else(|| "dist".into()));
            std::fs::create_dir_all(&out)?;
            dist::package(&target, &out, arg(&args, "--binary").map(Into::into)).map(|_| ())
        }
        Some("ci") => {
            gen::run(true)?;
            bundle::run(true)?;
            status::run(true)?;
            deps::run()?;
            lint::run()
        }
        Some("tf-test") | Some("parity-record") => {
            eprintln!("`{}` is not implemented yet", args[0]);
            std::process::exit(2);
        }
        Some("help") | Some("--help") | None => {
            print!("{HELP}");
            Ok(())
        }
        Some(other) => bail!("unknown command `{other}`\n\n{HELP}"),
    }
}
