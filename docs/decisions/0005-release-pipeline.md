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

- Tag `v<version>` → build + publish a release; image also tagged `latest`.
- Tag `v<version>-rc.N` → build + publish a GitHub pre-release; image tagged only with the rc
  version, never `latest`. Used to dry-run the full publish path.
- Any other tag fails before publishing (`<version>` = `workspace.package.version`).
- `workflow_dispatch` → build only, or publish when `publish: true`.

After publishing, `verify` and `verify-macos` re-download the release and check it as a user
would: `sha256sum -c`, `cosign verify-blob` / `cosign verify` pinned to this repo's
`release.yml` identity, `gh attestation verify` for archives and image, static linking,
`--version` on both image platforms, a readiness probe, and `lipo -archs` + startup for macOS.

## Windows

Decided (§22.10): no Windows binaries or runners until there is a real request in a GitHub
issue.

## Verified locally (2026-11-26)

- `xtask dist` for x86_64 and aarch64 musl: static binaries, archives, `sha256sum -c` OK.
- aarch64 binary under `qemu-aarch64-static`: starts, serves AWS `CreateSecret`, healthcheck OK.
- macOS targets compile (`cargo check` for x86_64/aarch64-apple-darwin; linking needs a Mac).
- Image: built for linux/amd64 and linux/arm64; amd64 run: 13.4 MB, uid 65532, Docker health
  status `healthy`, `docker stop` exits 0 in ~0.2 s.
- Full official-SDK suite passes against the static release binary.
- Tag validation script: accepts `v0.1.0`, `v0.1.0-rc.1`; rejects other versions/suffixes.

## Not yet
- Homebrew tap and crates.io publishing (crates are `publish = false`; `cargo binstall` reads
  GitHub Releases via `[package.metadata.binstall]`).
- The distroless base is referenced by tag, not digest; pin it once a digest can be resolved
  from CI.
- The publish/verify jobs (cosign, attestations, GHCR push, macOS link) run only on GitHub;
  first exercised by a `v0.1.0-rc.1` tag.
