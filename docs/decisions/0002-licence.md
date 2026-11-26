# 0002 — Licence

Date: 2026-11-26 · Status: accepted

MCLE is licensed under **Apache-2.0** (`LICENSE`, `license = "Apache-2.0"` in every crate via
`[workspace.package]`). Contributions are accepted under the same licence with DCO sign-off
(`git commit -s`).

Vendored specs and IAM catalogs keep their upstream licences; each must be compatible with
Apache-2.0 redistribution and recorded in `specs/specs.lock` before it is vendored.
