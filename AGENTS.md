# AGENTS.md — MCLE

Instructions for any coding agent (and human) working in this repo.

Multi-cloud local emulator in Rust (AWS, Azure, GCP, OCI, IBM). Read DESIGN.md first; §2 principles and §6 extension model are binding.

## Golden rules
- Wire parity beats internal elegance. Official SDKs/CLIs/Terraform must work unchanged.
- Core is cloud-agnostic. Never add a cloud/service dependency to mcle-core, mcle-sdk, engines, or runtime (`cargo xtask check-deps`).
- New service/cloud/protocol = new crate + manifest. Never edit core to add one; if you think you must, write an RFC in rfcs/ and stop.
- Semantics live in engines; facades only map shapes. Don't duplicate engine logic per cloud.
- Every op declares iam_action, resource extractor, level, lro, emits. Undeclared fails CI.
- Never hand-edit generated code (`gen/`, STATUS.md, .github/CODEOWNERS, mcle-bundle). Change specs/manifests and run xtask.
- Every L1+ claim needs a `verifies(Lx)` test marker; every L2 claim an SDK test; every L3 claim a parity fixture.
- CI contract (DESIGN.md §15.1) is non-negotiable: no Docker/egress/TTY/fixed port in core paths; respect the startup budget.
- Comments: only where logic is non-obvious or a parameter needs explaining. No design narration in code.
- [VERIFY] items: confirm before relying; record in docs/decisions/.

## Commands
- Build: `cargo build --workspace`
- Lint: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo xtask ci`
- Test: `cargo nextest run --workspace` (or `cargo test --workspace`)
- SDK tests: `cargo build -p mcle-cli && pytest tests/sdk -k <cloud>` (deps: `tests/sdk/requirements.txt`)
- Codegen: `cargo xtask codegen [--check]`
- Bundle: `cargo xtask bundle [--check]`
- New service: `cargo xtask new-service --cloud <c> --service <s>`
- Status: `cargo xtask status`
- Run: `cargo run -p mcle-cli -- start`

## Current phase
See STATUS.md header. Do not start P1/P2 work until P0 exit criteria pass.

## Ask before
- Adding dependencies outside DESIGN.md §20.
- Any change to mcle-sdk public API, Scope/ResourceId/LRO/OpMeta, or engine traits (RFC).
- Vendoring a new spec or IAM catalog source (licence check).
