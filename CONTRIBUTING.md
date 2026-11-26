# Contributing to MCLE

Target: a new service at L1 in under an hour, following only this file.

## Layout

| Path | What lives there |
|---|---|
| `crates/mcle-sdk` | The plugin API. The only crate plugins must depend on (plus engines they use). |
| `crates/mcle-core`, `mcle-gateway`, `mcle-iam`, `mcle-identity`, `mcle-persist`, `mcle-runtime` | Cloud-agnostic core. |
| `engines/*` | Shared semantics (`crud` today). |
| `clouds/<cloud>` | One `CloudAdapter` per cloud: detection, errors, LRO rendering, CRUD shaper. |
| `services/<cloud>/<svc>` | Service facades: `service.toml`, generated `gen/`, `src/`, `tests/`. |
| `specs/` | Vendored API specs and IAM catalogs (see `specs/specs.lock`). |
| `xtask/` | Codegen, bundle, status, lints, scaffolding. |
| `tests/sdk` | Official-SDK tests (pytest). |

## Add a service

```sh
cargo xtask new-service --cloud gcp --service pubsub --tier T0
```

This creates `services/gcp/pubsub/` (manifest, facade, smoke test), a stub spec, and regenerates
`gen/`, the plugin bundle and STATUS.md. Then:

1. Replace the stub spec with the upstream spec (Smithy JSON AST for AWS, OpenAPI for
   Azure/OCI/IBM, Discovery for GCP) and add it to `specs/specs.lock` (licence check first).
2. Declare CRUD resources in `service.toml` so lifecycle ops get generic L1:
   ```toml
   [crud.resources.topic]
   parent_template = "projects/{projectsId}"
   collection = "topics"
   id_param = "topicsId"
   ```
   Hint keys per cloud are listed on each shaper in `clouds/<cloud>/src`.
3. Override ops where inference is wrong:
   ```toml
   [ops."projects.topics.publish"]
   plane = "data"
   level = "L2"
   custom = true          # requires facade = "custom"
   emits = ["MessagePublished"]
   iam_action = "pubsub.topics.publish"
   ```
4. `cargo xtask codegen && cargo xtask bundle && cargo xtask status`
5. Add tests with markers backing every claimed level:
   - Rust: `// verifies(L1): CreateTopic, GetTopic` in `services/<cloud>/<svc>/tests/*.rs`
   - SDK: `# verifies(L2): gcp/pubsub: projects.topics.publish` in `tests/sdk/*.py`
6. `cargo xtask ci` must pass.

## Extension points

Implement a trait from `mcle-sdk` in a new crate; never edit core crates:

| Trait | Use |
|---|---|
| `ServiceFacade` | a cloud service |
| `CloudAdapter` | a whole cloud (detection, errors, LRO, CRUD shaping) |
| `CrudShaper` | native shapes for generic L1 CRUD of a protocol family |
| `Engine` | shared semantics used by many facades |
| `WireServer` | a non-HTTP protocol listener |
| `IdentityProvider` | a credential/token type |
| `AuthzEvaluator` | a cloud's policy language |
| `LroRenderer` | a cloud's LRO shape |
| `TriggerSource` / `TriggerSink` | event sources/targets |
| `FunctionRuntime` / `ExecutionBackend` | function and container execution |
| `PersistenceBackend` | state store |
| `PipelineLayer` | a request pipeline layer |

A new cloud: add `clouds/<name>` exposing `pub fn adapter(host: &Host) -> Arc<dyn CloudAdapter>`,
then `cargo xtask bundle`.

## Rules

- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo xtask ci` must pass.
- No `unwrap()`/`expect()` outside tests and startup. `#![forbid(unsafe_code)]`.
- Never hand-edit generated files (`gen/`, `STATUS.md`, `.github/CODEOWNERS`, `crates/mcle-bundle`).
- Comments only for non-obvious logic or parameters.
- Changes to `mcle-sdk`, core types or engine traits need an RFC (`rfcs/0000-template.md`).
- Conventional commits (`feat(services/gcp/pubsub): ...`), DCO sign-off (`git commit -s`).

Labels: `good-first-op`, `tier:T0`, `cloud:<x>`, `level:L1→L2`. STATUS.md is the backlog.
