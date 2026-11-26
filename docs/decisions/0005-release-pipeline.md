# 0005 — Release pipeline

Date: 2026-11-26 · Status: accepted

## Artifacts (DESIGN.md §15.7)

| Artifact | How |
|---|---|
| `mcle-<v>-x86_64-unknown-linux-musl.tar.gz` | static-pie, built on ubuntu with the musl target |
| `mcle-<v>-aarch64-unknown-linux-musl.tar.gz` | cross-linked with `rust-lld` (no C deps, so no cross toolchain) |
| `mcle-<v>-universal-apple-darwin.tar.gz` | x86_64 + aarch64 joined with `lipo` |
| `SHA256SUMS` | `cargo xtask dist --checksums` |
| `*.sigstore.json` | keyless `cosign sign-blob` bundles |
| Build provenance | `actions/attest-build-provenance` (archives and image) |
| `ghcr.io/<owner>/mcle:<v>`, `:latest` | distroless/static nonroot, linux/amd64 + linux/arm64, cosign-signed |

`cargo xtask dist` builds and packages one target; the same command runs locally, in the PR
`package` job and in `release.yml`, so the release path is exercised on every PR (including a
container build and smoke test).

## Triggers

- Tag `v*` → build + publish. The tag must equal `workspace.package.version`.
- `workflow_dispatch` → build only, or publish when `publish: true`.

## Not yet

- Windows binaries (open decision §22.10).
- Homebrew tap and crates.io publishing (crates are `publish = false`; `cargo binstall` reads
  GitHub Releases via `[package.metadata.binstall]`).
- The distroless base is referenced by tag, not digest; pin it once a digest can be resolved
  from CI.
- The publish job (cosign, attestations, GHCR push, macOS build) runs only on a real release
  and has not been exercised yet.
