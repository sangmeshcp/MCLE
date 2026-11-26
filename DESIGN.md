# MCLE — Multi-Cloud Local Emulator (Rust)

> **Status:** Design plan v0.3
> v0.2: CI, build-system, Terraform testing · **v0.3: day-1 service breadth, IAM enforcement, streaming, functions, open extension model**
> **Owner:** Sang
> **Language:** Rust (stable, edition 2021+)

---

## 0. How to use this document

1. Read this whole file before writing code. §2 (principles) and §6 (extension model) are binding.
2. `AGENTS.md` (agent instructions; `CLAUDE.md` imports it) is generated from §21; `CONTRIBUTING.md` from §6.
3. Follow the roadmap (§19). Phase P0 must finish — and the plugin SDK must be stable — before service work fans out.
4. Anything marked **`[VERIFY]`** is an assumption about a vendor SDK/CLI/API/spec that must be confirmed against current docs or a real test before building on it. Record the outcome in `docs/decisions/`.
5. Keep `STATUS.md` generated (never hand-written) from service manifests + test results (§6.4).

---

## 1. Goal

A single fast binary that emulates **AWS, Azure, GCP, OCI, and IBM Cloud** locally — control planes, data planes, IAM, events, and compute — so that **official SDKs, CLIs, and Terraform providers work unchanged** (or with endpoint configuration only).

### 1.1 Use cases
- **CI-first:** hermetic integration tests on GitHub Actions, Buildkite, CircleCI, GitLab CI, Jenkins, Tekton (§15).
- **Build-system native:** first-class Bazel rules; Pants, Buck2, Gradle, Nx, Make via a universal wrapper (§16).
- **Terraform / IaC change testing:** plan → apply → idempotency → destroy → leak check per PR, real remote-state backends (§17).
- **Testing new application services** against everything a service typically touches — identity, IAM, secrets, storage, queues, streams, functions, databases, caches, registries, logs — with fault injection (§12, §15.5).
- Local dev and coding-agent sandboxes with zero real-cloud blast radius.

### 1.2 What "day-1 support" means
Breadth across five clouds is only achievable with a **fidelity ladder** (§12.1):
- **Every resource type in every allowlisted spec** works at **L1 (CRUD)** — generated automatically (§14.3). Terraform `plan/apply/destroy` succeeds for the long tail on day 1.
- **The Tier-0 catalog** (§12.2) — what a typical service touches — works at **L2 (functional)** on all five clouds, with **IAM enforcement** available.
- The **1.0 release gate** (§19.3) is defined in these terms. Nothing ships labelled "supported" without a declared fidelity level backed by tests.

### 1.3 Non-goals (1.0)
- Production use or production-grade durability.
- Billing, quotas, service-limit emulation (except where SDK retry logic depends on it — via fault injection).
- Web console UI.
- Real AI model hosting (AI APIs proxy to a local OpenAI-compatible server or a deterministic stub).
- Data-plane parity for analytics warehouses (BigQuery/Athena/Synapse) — L1 only in 1.0.

### 1.4 Design targets
| Metric | Target |
|---|---|
| Cold start | < 150 ms to ready (no sidecars) |
| `mcle run -- true` | < 300 ms total |
| Idle RSS | < 80 MB with all clouds enabled |
| Parallelism | 16+ instances per CI runner (`--port 0`), no clashes |
| Distribution | Single static binary + multi-arch OCI image |
| Ports | One HTTP port for all clouds (default `4700`) + optional wire-protocol ports (§7.4) |
| Runtime deps for core | None: no Docker, no egress, no telemetry, no account/token |

---

## 2. Design principles (the constitution)

Changing any of these requires an RFC (§6.8). Every PR is reviewed against them.

1. **Wire parity over internal elegance.** If an official SDK/CLI/Terraform provider behaves differently against MCLE than against the real cloud, that is an MCLE bug.
2. **Core is cloud-agnostic.** `mcle-core`, `mcle-sdk`, engines, and runtime crates never depend on a cloud or service crate. Enforced by an `xtask check-deps` lint in CI.
3. **Engines are shared; facades are thin.** Semantics (object storage, queues, logs, secrets, KMS, IAM evaluation, LRO, functions) are implemented once. A cloud service is a facade that maps API shapes onto an engine.
4. **Extension is additive.** Adding a service, a wire protocol, an engine, or even a sixth cloud must never require editing core crates — only adding crates and manifests (§6).
5. **Everything is declared.** Each operation declares its IAM action, resource-ID extraction, emitted events, LRO behavior, and fidelity level in metadata. Undeclared = CI failure.
6. **Generated first, handwritten second.** Shapes, routes, traits, CRUD (L1) come from vendor specs. Humans write semantics.
7. **Honest fidelity.** Every operation reports its level. `--strict` turns any gap into a loud failure, never a silent success.
8. **Hermetic by default.** Core paths need no Docker, network egress, TTY, fixed port, or wall clock. Sidecars and containers are opt-in capabilities with mock fallbacks.
9. **Deterministic.** Seeded IDs, injectable clock, reproducible state snapshots.
10. **Contributor time-to-first-op < 1 day.** Scaffolding, conformance kits, and docs are first-class deliverables, not afterthoughts.

---

## 3. Prior art and lessons

| Project | Borrow | Avoid |
|---|---|---|
| LocalStack (ASF) | Spec-driven parsing/serialization; generated skeletons with `NotImplemented`; handler chain; snapshot parity tests; Lambda runtime-API approach | Python startup cost; AWS-only state model; license gating |
| moto | Breadth via simple models; per-service contributions | In-process only; no wire parity |
| Floci | Free/no-auth, fast native image, multi-cloud family | Separate binaries/ports per cloud |
| Azurite, GCP emulators, Cosmos emulator | High-fidelity single-service data planes | Use as sidecar backends, not architecture |
| Redpanda / Kafka-protocol servers | Kafka wire compatibility | JVM/heavy runtime in core path |

---

## 4. Architecture overview

```
 Clients: official SDKs / CLIs / Terraform / Kafka & AMQP clients / container tools
        │ HTTP(S) + gRPC                        │ wire protocols (§7.4)
        ▼                                       ▼
 ┌──────────────────────────────┐   ┌───────────────────────────────────┐
 │ Edge gateway (hyper + tonic) │   │ Protocol servers: Kafka · AMQP 1.0│
 │ TLS · ctx · cloud detection  │   │ OCI distribution (registry) · DNS │
 └──────────────┬───────────────┘   └──────────────┬────────────────────┘
                ▼                                  │
 ┌──────────────────────────────────────────┐      │
 │ Cloud adapters (one per cloud, plugin)   │      │
 │ parse/serialize · errors · LRO renderer  │      │
 └──────────────┬───────────────────────────┘      │
                ▼                                  │
 ┌──────────────────────────────────────────┐      │
 │ Request pipeline (tower layers)          │      │
 │ identity → IAM authz → faults → dispatch │◄─────┘
 └──────────────┬───────────────────────────┘
                ▼
 ┌──────────────────────────────────────────────────────────────┐
 │ Service facades (plugins): generated trait + handwritten impl │
 │ or generic L1 CRUD provider · or external/out-of-process      │
 └──────────────┬───────────────────────────────────────────────┘
                ▼
 ┌──────────────────────────────┐  ┌──────────────────────────────┐
 │ Engines (shared semantics)   │  │ Runtime                       │
 │ object · queue · pubsub · log│  │ functions (container/process) │
 │ kv/doc · secrets · kms · cfg │  │ triggers · k8s (k3s)          │
 │ registry · telemetry · dns   │  │ sidecars (postgres, valkey…)  │
 │ iam-eval · mail · workflow   │  └──────────────────────────────┘
 └──────────────┬───────────────┘
                ▼
 ┌──────────────────────────────────────────────────────────────┐
 │ Core: Scope · ResourceId · Store · LRO · Clock · Event bus ·  │
 │ Persistence · Sandboxes · Coverage recorder                   │
 └──────────────────────────────────────────────────────────────┘
```

Request lifecycle (HTTP):
1. Gateway builds `RequestContext` (request id, sandbox, raw head).
2. Cloud detector (§7.2) picks a cloud adapter → resolves `(service, operation)` via generated route tables.
3. Identity layer extracts the `Principal` (§9).
4. Generated `OpMeta` gives the IAM action + resource-ID extractor → IAM layer evaluates (`off|audit|enforce`).
5. Fault layer may inject an error/latency (§15.5).
6. Dispatch to the facade (handwritten, generic L1, or external plugin).
7. Facade calls engines; engines publish events to the bus; triggers fan out (§13).
8. Adapter serializes native output/error; coverage recorder logs `(op, level, outcome)`.

---

## 5. Repository layout (Cargo workspace)

```
mcle/
├── Cargo.toml                    # workspace
├── AGENTS.md                     # agent instructions, from §21
├── CLAUDE.md                     # `@AGENTS.md` import for Claude Code
├── CONTRIBUTING.md               # from §6
├── DESIGN.md                     # this file
├── STATUS.md                     # GENERATED coverage matrix
├── rfcs/                         # RFCs for core/SDK changes (§6.8)
├── specs/                        # vendored upstream specs + IAM action catalogs, pinned
│   ├── specs.lock                # source URL + git SHA + license
│   └── {aws,azure,gcp,oci,ibm}/{api,iam}/
├── xtask/                        # codegen, new-service, check-deps, status, parity-record, tf-test
├── crates/
│   ├── mcle-sdk/                 # ★ PUBLIC, semver-stable plugin API (traits, OpMeta, ctx, errors)
│   ├── mcle-core/                # Scope, ResourceId, Store, LRO, Clock, event bus, sandboxes
│   ├── mcle-persist/             # snapshot/restore
│   ├── mcle-gateway/             # hyper/tonic server, TLS, cloud detection, tower pipeline
│   ├── mcle-iam/                 # authz engine trait + per-cloud evaluators
│   ├── mcle-identity/            # credential parsing, fake issuers, metadata servers, OIDC federation
│   ├── mcle-runtime/             # functions (container + process), triggers, k3s, sidecars
│   ├── mcle-testkit/             # conformance suites, SDK harness, snapshot helpers
│   ├── mcle-bundle/              # GENERATED registry of enabled plugins (cargo features)
│   └── mcle-cli/                 # `mcle` binary
├── engines/                      # one crate per engine
│   ├── object/  queue/  pubsub/  log/  kvdoc/  secrets/  kms/  config/
│   ├── registry/  telemetry/  dns/  mail/  workflow/  crud/   # crud = generic L1 engine
├── wire/                         # non-HTTP protocol servers
│   ├── kafka/  amqp/  oci-distribution/  dns/
├── clouds/                       # one adapter crate per cloud
│   ├── aws/  azure/  gcp/  oci/  ibm/
├── services/                     # ★ where most contributions land
│   ├── aws/{s3,sqs,sns,iam,sts,kms,secretsmanager,ssm,lambda,kinesis,dynamodb,...}/
│   │   └── <svc>/{service.toml, gen/, src/, tests/}
│   ├── azure/{arm,storage,keyvault,servicebus,eventhubs,functions,...}/
│   ├── gcp/{storage,pubsub,iam,secretmanager,kms,functions,run,...}/
│   ├── oci/{identity,objectstorage,vault,streaming,functions,...}/
│   └── ibm/{iam,resourcecontroller,cos,secretsmanager,eventstreams,codeengine,...}/
├── plugins-external/             # example out-of-process plugin (Python + Go) (§6.6)
├── integrations/                 # CI + build-tool integrations (§15.8, §16)
│   ├── github-action/ buildkite-plugin/ circleci-orb/ gitlab/ jenkins/ tekton/ helm/
│   ├── bazel/ gradle/ testcontainers/{go,java,python,node,rust,dotnet}/
├── examples/
│   ├── reference-app/{aws,azure,gcp,oci,ibm}/   # golden-path apps used as release gate (§19.3)
│   └── ci/ terraform/ app-service/
├── coverage/terraform/           # GENERATED resource-type → op → level per provider
└── tests/{sdk,terraform,parity,e2e}/
```

---

## 6. Extension model

### 6.1 The plugin SDK (`mcle-sdk`)
The **only** crate plugins depend on (plus engines they use). Semver-stable from 1.0; pre-1.0 changes need an RFC once P0 closes. It exports:
- `RequestContext`, `Principal`, `Scope`, `ResourceId`, `CloudError`, `OpMeta`, `FidelityLevel`.
- Engine client handles (`ObjectEngine`, `QueueEngine`, …) as traits.
- All extension-point traits (§6.2).

### 6.2 Extension points
| Trait | Implement it to… | Example |
|---|---|---|
| `ServiceFacade` | add/extend a cloud service | `services/gcp/cloudtasks` |
| `CloudAdapter` | add a whole cloud (protocols, errors, LRO rendering, detection rules) | DigitalOcean, Alibaba, Cloudflare, Hetzner |
| `Engine` | add shared semantics reused by many facades | `engines/vector` for AI search services |
| `WireServer` | add a non-HTTP protocol listener | Redis RESP, MQTT, Postgres proxy |
| `IdentityProvider` | parse a new credential/token type | new federation flow |
| `AuthzEvaluator` | add a cloud's policy language | OCI policy grammar |
| `LroRenderer` | render the shared LRO model in a cloud's shape | — |
| `TriggerSource` / `TriggerSink` | add event sources/targets | DynamoDB Streams → function |
| `FunctionRuntime` | add a function packaging/runtime | WASI runtime |
| `ExecutionBackend` | add a container/k8s backend | Podman, containerd, remote k8s |
| `PersistenceBackend` | add a state store | S3-backed shared snapshots |
| `PipelineLayer` | add a tower layer to the request pipeline | request recording, chaos |

```rust
#[async_trait::async_trait]
pub trait ServiceFacade: Send + Sync + 'static {
    fn manifest(&self) -> &'static ServiceManifest;          // from service.toml
    fn routes(&self) -> &'static RouteTable;                  // generated
    fn op_meta(&self, op: OpId) -> &'static OpMeta;           // generated + overrides
    async fn dispatch(&self, ctx: &RequestContext, op: OpId, input: Input)
        -> Result<Output, CloudError>;                        // generated → typed trait
}

pub struct OpMeta {
    pub name: &'static str,
    pub iam_action: IamAction,              // e.g. "s3:PutObject", "storage.objects.create"
    pub resource: ResourceExtractor,        // how to build the ResourceId from input
    pub level: FidelityLevel,               // L0..L3, verified by CI (§12.1)
    pub lro: LroBehavior,                   // None | Create | Update | Delete | Action
    pub emits: &'static [EventKind],        // e.g. ObjectCreated
    pub idempotency: Idempotency,
}
```

### 6.3 Registration (compile-time, explicit)
- Each plugin crate has a manifest. `xtask bundle` generates `mcle-bundle/src/registry.rs` listing every enabled plugin behind a Cargo feature (`aws-s3`, `gcp-pubsub`, `cloud-aws`, `all`).
- No link-time magic (`inventory`/`linkme`) — explicit, reviewable, works with static musl and Bazel.
- Custom builds: `cargo build -p mcle-cli --no-default-features --features cloud-aws,azure-keyvault`.

### 6.4 Service manifest (`service.toml`)
```toml
[service]
cloud = "gcp"
name = "secretmanager"
display = "Secret Manager"
api = { source = "googleapis", path = "google/cloud/secretmanager/v1", version = "v1" }
tier = "T0"                      # T0 | T1 | T2 (§12.2)
engines = ["secrets", "kms"]
owners = ["@mcle/gcp"]           # CODEOWNERS generated from this

[defaults]
level = "L1"                     # ops not listed below get generic CRUD where possible

[ops.AccessSecretVersion]
level = "L2"
emits = []
[ops.AddSecretVersion]
level = "L2"
emits = ["SecretVersionAdded"]
```
`STATUS.md`, `coverage/terraform/*`, CODEOWNERS, and docs pages are **generated** from manifests + test results. CI fails if a manifest claims L2 for an op with no passing L2 test.

### 6.5 Scaffolding
```
cargo xtask new-service --cloud gcp --service cloudtasks [--tier T1]
```
Creates: `codegen.toml` entry → generated crate → facade skeleton (all ops L1 via generic CRUD or `NotImplemented`) → manifest → SDK test template (pytest with official SDK) → snapshot test template → docs stub. Target: L1 service compiling and passing generic conformance in < 1 hour.

### 6.6 Out-of-process plugins (non-Rust contributors, private services)
- A plugin registers via config: `plugins: [{ name, cloud, service, endpoint: "unix:///tmp/p.sock" | "http://…", manifest: path }]`.
- MCLE still does parsing, identity, IAM, faults, LRO, coverage; it forwards **typed JSON** `{op, input, ctx}` over HTTP/gRPC and serializes the reply. Plugins can call back into engines via a local engine API.
- Use cases: internal/proprietary services, prototyping in Python/Go/TS, company-specific mocks of internal APIs on a fake cloud endpoint.
- Reference plugins in `plugins-external/` (Python, Go).
- **Later (post-1.0, RFC):** WASM component plugins via `wasmtime` for sandboxed, hermetic, Bazel-friendly extensions.

### 6.7 Conformance kits (`mcle-testkit`)
- **Engine conformance:** one suite per engine run automatically against *every* facade bound to it (e.g. the object suite runs against S3, Blob, GCS, OCI OS, COS).
- **Adapter conformance:** error envelopes, request-id headers, LRO rendering, pagination, idempotency tokens.
- **Metadata lint:** every op has IAM action + resource extractor + level; every `emits` has a matching event schema.
- **SDK harness:** start MCLE on a random port, inject env, run the official SDK test file.

### 6.8 Governance
- Licence: Apache-2.0 (decided, docs/decisions/0002-licence.md) + DCO sign-off.
- `rfcs/` for any change to principles (§2), `mcle-sdk`, core types, or engine traits. Lightweight template: motivation, design, alternatives, migration.
- CODEOWNERS per cloud and per engine; core changes need a core maintainer.
- Labels: `good-first-op`, `tier:T0`, `cloud:<x>`, `level:L1→L2`. `STATUS.md` doubles as the contribution backlog.

---

## 7. Gateway and protocol servers

### 7.1 Stack
- `tokio`, `hyper` 1.x (+ `hyper-util`), `tower` pipeline, `tonic` on the same listener for gRPC (`content-type: application/grpc`).
- TLS: `rustls` + `rcgen` local CA, SNI-based leaf certs; `mcle ca export`.
- `tracing` spans with cloud/service/op/sandbox.

### 7.2 Cloud detection (first match wins)
1. `x-mcle-cloud` header (tests/debug).
2. Host suffix patterns (`*.amazonaws.com.<local>`, `management.azure.<local>`, `<acct>.blob.core.azure.<local>`, `<svc>.googleapis.<local>`, `<svc>.<region>.oci.<local>`, `<svc>.cloud.ibm.<local>`).
3. Auth header shape: `AWS4-HMAC-SHA256` → AWS; `Signature version="1",keyId="ocid1…"` → OCI; `Bearer <JWT>` → by our issuer `iss`.
4. Path prefix: `/subscriptions/` → Azure ARM; `/v2/resource_instances` → IBM RC; `/storage/v1/` → GCS; `/n/{ns}/b/` → OCI OS; `/identity/token` → IBM IAM.
5. Fallback: AWS.
Detection rules are contributed by each `CloudAdapter` (principle 4).

### 7.3 DNS
- Default local domain `localhost` `[VERIFY wildcard resolution per OS/container]`.
- Built-in DNS server (`wire/dns`, `hickory-server`) serves `*.<local>` **and** zones created via Route 53 / Azure DNS / Cloud DNS / OCI DNS / IBM DNS Services — so apps can resolve names they created.
- Env-var endpoint overrides remain the preferred redirection (§10).

### 7.4 Wire-protocol servers (enabled per need, each a `WireServer` plugin)
| Server | Default port | Serves | Notes |
|---|---|---|---|
| Kafka | 9092 | MSK, Azure Event Hubs Kafka endpoint, GCP Managed Kafka, OCI Streaming, IBM Event Streams | Native subset over the `log` engine: ApiVersions, Metadata, Produce, Fetch, ListOffsets, consumer groups (Join/Sync/Heartbeat/OffsetCommit/Fetch), CreateTopics/DeleteTopics, SASL PLAIN/SCRAM/OAUTHBEARER (accept-all, principal extracted for IAM). Use `kafka-protocol` crate `[VERIFY]`. Redpanda sidecar fallback for full fidelity. |
| AMQP 1.0 | 5672 / 5671 | Azure Service Bus, Event Hubs (AMQP) | Azure SDKs default to AMQP. `fe2o3-amqp` crate `[VERIFY]`. CBS token auth accepted. |
| OCI distribution | 4700 (path `/v2/`) | ECR, ACR, Artifact Registry, OCIR, ICR | Native registry on `registry` engine + per-cloud token endpoints. |
| DNS | 5353 (opt-in 53) | All DNS services | See §7.3. |

Port map is printed by `mcle env` and exposed at `/_mcle/info`.

### 7.5 Internal endpoints
`/_mcle/{health,ready,info,reset,state/export,state/import,lro,ca.pem,sandboxes,faults,report,mailbox,iam/explain}`.

---

## 8. Shared core (`mcle-core`)

### 8.1 Scope
```rust
pub enum Scope {
    Aws   { account: String, region: String },
    Azure { tenant: Uuid, subscription: Uuid, resource_group: Option<String>, location: Option<String> },
    Gcp   { project: String, location: String },
    Oci   { tenancy: String, compartment: String, region: String },
    Ibm   { account: String, resource_group: String, region: String },
    Ext   { cloud: CloudId, parts: SmallVec<[String; 4]> },   // for plugin clouds (principle 4)
}
```
Hierarchy-aware: Azure mgmt-group/sub/RG, GCP org/folder/project, OCI compartment tree, IBM account/RG, AWS org/OU/account (for IAM inheritance).

### 8.2 Resource identifiers
`ResourceId::{Arn, ArmId, GcpName, Ocid, Crn, Ext}` — lossless parse/format, `proptest` round-trips, wildcard matching for IAM.

### 8.3 LRO engine
One state machine (`Pending → Running → Succeeded|Failed|Cancelled`), cloud renderers:
- Azure: `201/202` + `Azure-AsyncOperation` + `Location` + `Retry-After`; `provisioningState`.
- GCP: `google.longrunning.Operation`.
- OCI: `opc-work-request-id` + `lifecycleState`.
- IBM: `state` / `lifecycle_state`.
- AWS: status fields (`creating → available`).
`MCLE_LRO_DELAY_MS` (default `0`), per-service overrides; manual clock for tests.

### 8.4 Store
`Store<T>` keyed by `(SandboxId, Scope, ResourceId)`; typed, `Serialize + Deserialize`. Modes: `ephemeral` | `persistent` | `import/export`. Blobs content-addressed on disk.

### 8.5 Event bus
Typed `CloudEvent { id, source: ResourceId, kind, time, data, sandbox }` over an internal bus with **at-least-once** delivery to subscribers, retries, and DLQ semantics shared by all trigger types (§13.3).

### 8.6 Errors
`CloudError { code, message, http_status, retryable, details }`, rendered natively per adapter (AWS XML/JSON `__type`, Azure `{"error":…}`, GCP `{"error":…}`/gRPC `Status`, OCI `{"code","message"}`, IBM `{"errors":[…],"trace"}` `[VERIFY]`), plus native request-id headers.

---

## 9. Identity and IAM (day-1)

### 9.1 Identity (`mcle-identity`)
**Accept** any well-formed credential, **parse** the principal, never verify against real cloud. Optional signature verification (`MCLE_VERIFY_SIGNATURES=1`).

| Cloud | Fake endpoints | Notes |
|---|---|---|
| AWS | STS (`GetCallerIdentity`, `AssumeRole`, `AssumeRoleWithWebIdentity`, `GetSessionToken`), IMDSv2 | `AWS_EC2_METADATA_SERVICE_ENDPOINT` `[VERIFY]` |
| Azure | Entra OIDC discovery + `/oauth2/v2.0/token` (client credentials, federated assertion), IMDS, App-Service MSI (`IDENTITY_ENDPOINT`/`IDENTITY_HEADER`) | `DefaultAzureCredential` must succeed `[VERIFY authority override]` |
| GCP | `oauth2.googleapis.com/token`, metadata server, STS `sts.googleapis.com/v1/token` (WIF), `iamcredentials` `generateAccessToken` (impersonation) | `GCE_METADATA_HOST` `[VERIFY]` |
| OCI | HTTP-signature parsing; instance/resource principal endpoints; session tokens | `[VERIFY]` principal flows |
| IBM | IAM `/identity/token` (API key, trusted profile, CR token) | `[VERIFY claim names]` |

**CI federation (day-1):** accept external OIDC JWTs (GitHub Actions, Buildkite, CircleCI, GitLab) for `AssumeRoleWithWebIdentity`, Azure federated credentials, GCP WIF, IBM trusted profiles — optionally verified offline against a supplied JWKS. Lets teams test their real CI auth wiring.

### 9.2 IAM control planes (L2 on day 1)
AWS IAM (users, groups, roles, policies, instance profiles, OIDC providers) · Entra apps/service principals/managed identities + Azure RBAC (role definitions, assignments) · GCP IAM (service accounts, keys, roles, allow policies on any resource) · OCI Identity (users, groups, dynamic groups, policies, compartments) · IBM IAM (service IDs, access groups, policies, trusted profiles).

### 9.3 Authorization engine (`mcle-iam`)
```rust
pub trait AuthzEvaluator: Send + Sync {
    fn cloud(&self) -> CloudId;
    fn evaluate(&self, req: &AuthzRequest, policies: &PolicySet) -> Decision; // Allow | Deny{reason, matched}
}
```
- Modes: `MCLE_IAM=off|audit|enforce` (default **`audit`**: never blocks, records would-be denials in the report).
- `enforce` is how teams test **least-privilege**: run the new service with the role/identity Terraform created; a missing permission fails exactly as in the cloud, with the native `AccessDenied`/`AuthorizationFailed`/`PERMISSION_DENIED` error.
- `mcle iam explain --request-id <id>` and `/_mcle/iam/explain` show matched statements/roles.
- Day-1 policy coverage:

| Cloud | Day-1 | Later |
|---|---|---|
| AWS | identity + resource policies, explicit deny, wildcards, `NotAction`/`NotResource`, common condition operators + keys (`aws:SourceArn`, `aws:PrincipalArn`, `aws:SourceAccount`, tags) | permission boundaries, SCPs, session policies |
| Azure | built-in + custom role definitions, assignments with scope inheritance, `actions/notActions/dataActions` | deny assignments, ABAC conditions |
| GCP | allow policies on org/folder/project/resource with inheritance, predefined + custom roles | IAM conditions, deny policies |
| OCI | policy statement grammar (`Allow group G to <verb> <resource-type> in compartment C [where …]`), verb → permission mapping | dynamic-group matching rules, tag conditions |
| IBM | access policies (subject, roles, resource attributes), access groups, service IDs | context-based restrictions |

- **Action catalogs** are vendored data (`specs/<cloud>/iam/`): AWS service authorization reference, Azure provider operations + built-in roles, GCP permissions + predefined roles, OCI verb/permission tables, IBM service actions/roles `[VERIFY sources + licenses]`. Every `OpMeta.iam_action` must exist in the catalog (lint).

---

## 10. Client redirection

Preference: **env vars → CLI wrapper → custom-cloud config → DNS**.

| Cloud | SDKs | CLI | Terraform |
|---|---|---|---|
| AWS | `AWS_ENDPOINT_URL` (+ `AWS_ENDPOINT_URL_<SVC>`) `[VERIFY]` | `mcle aws …` | provider `endpoints {}` |
| Azure | custom cloud / authority host + ARM overrides `[VERIFY per SDK]` | `az cloud register` custom cloud; `mcle az …` | `azurerm` metadata host / environment `[VERIFY]` |
| GCP | `*_EMULATOR_HOST`, `api_endpoint`, `CLOUDSDK_API_ENDPOINT_OVERRIDES_<SVC>` `[VERIFY]` | `mcle gcloud …` | `*_custom_endpoint` `[VERIFY]` |
| OCI | `service_endpoint` | `mcle oci …` | endpoint override `[VERIFY]` |
| IBM | `<SERVICE>_URL`, `<SERVICE>_AUTH_TYPE`, `<SERVICE>_AUTH_URL` `[VERIFY]` | `mcle ibmcloud …` `[VERIFY]` | endpoint overrides `[VERIFY]` |
| Kafka / AMQP | bootstrap `localhost:9092` / connection string from `mcle env` | — | — |
| Registries | `docker login` via each cloud's token flow (`aws ecr get-login-password`, `az acr login`, …) | — | — |

`mcle env <cloud>|--all` prints exports (incl. wire-protocol endpoints and port map).

---

## 11. Engines

Each engine is a crate with a trait in `mcle-sdk`, one in-process implementation, an optional sidecar implementation, and a conformance suite.

| Engine | Semantics | Sidecar option |
|---|---|---|
| `object` | buckets, objects, ranges, conditionals, multipart/block/resumable/compose, versioning, lifecycle (time-compressed), presigned/SAS/PAR URLs, notifications | — |
| `queue` | visibility timeout, delays, FIFO + dedup, DLQ/max-receive, sessions, scheduled messages, peek-lock | — |
| `pubsub` | topics, subscriptions, filters, push (HTTP) + pull, ack deadlines, ordering keys, retry/DLQ | GCP Pub/Sub emulator |
| `log` | partitioned append-only log: offsets, retention, consumer groups, shard iterators/checkpoints — backs Kinesis + all Kafka-style services | Redpanda |
| `kvdoc` | key-value + document: conditional writes, TTL, secondary indexes (subset), change streams, transactions (subset), query language subsets per facade | Cosmos emulator, Spanner/Bigtable/Firestore emulators |
| `secrets` | versions, stages/labels, rotation hooks (invoke function), replication flags | — |
| `kms` | real crypto (`ring`/RustCrypto): symmetric encrypt/decrypt, data keys, sign/verify, key versions/rotation, envelope encryption | — |
| `config` | parameters/app config, hierarchical keys, labels, feature flags | — |
| `registry` | OCI distribution spec: blobs, manifests, tags, referrers; image scanning stub | — |
| `telemetry` | logs, metrics, traces ingestion + basic query; alarms on metrics; **OTLP export** to local Grafana/Jaeger | — |
| `dns` | zones, records, health-check stubs, served by `wire/dns` | — |
| `mail` | captured outbound email/SMS/push; `/_mcle/mailbox` inspection API | — |
| `workflow` | state-machine executor (ASL subset for Step Functions; GCP Workflows syntax subset) | — |
| `crud` | generic L1 resource store driven by spec metadata (§14.3) | — |
| relational / cache / search | provisioning + connection info; data plane is a **real engine container** | Postgres, MySQL, SQL Server, Oracle Free `[VERIFY]`, Db2, Valkey, MongoDB, OpenSearch, CouchDB |

**No-Docker fallback for relational/cache/search:** control plane works; data plane connects to a user-supplied instance (`MCLE_EXTERNAL_POSTGRES_URL`, `…_REDIS_URL`, …) — fits CI systems that already provide service containers.

---

## 12. Service catalog and fidelity

### 12.1 Fidelity ladder
| Level | Meaning | Verified by |
|---|---|---|
| L0 | Not implemented — native "not implemented" error | — |
| L1 | **CRUD**: create/read/update/delete/list with correct shapes, IDs, LRO, tags; no behavior | generic conformance + Terraform apply/destroy |
| L2 | **Functional**: data plane works through an engine; SDK happy paths + common errors | SDK tests + engine conformance |
| L3 | **Behavioral parity**: IAM enforce, events/triggers, edge-case errors, parity-recorded snapshots | parity replay suite |

### 12.2 Tiers
- **T0** — what typical services touch. **L2 on all five clouds at 1.0**; IAM enforce supported.
- **T1** — common but secondary. **L2 on ≥ 3 clouds, L1 elsewhere at 1.0**.
- **T2** — long tail. **L1 at 1.0** (generated), deeper by contribution.

### 12.3 Catalog (service names `[VERIFY current product names]`)
| Category | Tier | AWS | Azure | GCP | OCI | IBM | Engine |
|---|---|---|---|---|---|---|---|
| Identity & IAM | T0 | IAM, STS, IAM Identity Center (L1), Organizations (L1) | Entra ID (apps/SPs/MI), RBAC | IAM, Service Accounts, Resource Manager, WIF | Identity (domains, policies, compartments) | IAM, Access Groups, Resource Manager | iam |
| Secrets | T0 | Secrets Manager | Key Vault secrets | Secret Manager | Vault secrets | Secrets Manager | secrets |
| Keys & crypto | T0 | KMS | Key Vault keys | Cloud KMS | Vault keys | Key Protect | kms |
| Certificates | T1 | ACM | Key Vault certificates | Certificate Manager | Certificates | Secrets Manager certs | secrets+kms |
| Config & flags | T0 | SSM Parameter Store, AppConfig | App Configuration | Parameter Manager `[VERIFY]` | — (Vault) | App Configuration | config |
| Object storage | T0 | S3 | Blob Storage, Data Lake Gen2 (L1) | Cloud Storage | Object Storage | COS | object |
| Queues | T0 | SQS | Service Bus queues, Storage Queues | Cloud Tasks | Queue | — | queue |
| Pub/sub & events | T0 | SNS, EventBridge (+ Scheduler) | Service Bus topics, Event Grid | Pub/Sub, Eventarc, Cloud Scheduler | Notifications, Events | Event Notifications | pubsub |
| Streaming | T0 | Kinesis Data Streams, MSK, Firehose (L1→sink to object) | Event Hubs (AMQP + Kafka) | Pub/Sub, Managed Kafka | Streaming (Kafka-compatible) | Event Streams (Kafka) | log + wire/kafka |
| Functions | T0 | Lambda (+ layers, URLs, ESM) | Functions | Cloud Functions, Cloud Run functions | Functions | Code Engine functions | runtime |
| Containers (run) | T1 | ECS/Fargate tasks, App Runner (L1) | Container Apps, Container Instances | Cloud Run services/jobs | Container Instances | Code Engine apps/jobs | runtime |
| Kubernetes | T1 | EKS | AKS | GKE | OKE | IKS, Red Hat OpenShift on IBM Cloud (MicroShift `[VERIFY]`) | runtime (k3s) |
| Container registry | T0 | ECR | ACR | Artifact Registry | OCIR | Container Registry | registry |
| Key-value / document | T0 | DynamoDB (+ Streams) | Cosmos DB (NoSQL API subset), Table | Firestore, Datastore | NoSQL | Cloudant | kvdoc |
| Relational | T0 (control) / sidecar (data) | RDS, Aurora | SQL DB, PostgreSQL/MySQL Flexible | Cloud SQL, Spanner (emulator), AlloyDB (L1) | MySQL HeatWave, Autonomous DB `[VERIFY]` | Databases for PostgreSQL/MySQL, Db2 | sidecar |
| Cache | T0 (control) / sidecar (data) | ElastiCache, MemoryDB | Cache for Redis / Managed Redis | Memorystore | Cache | Databases for Redis | sidecar (Valkey) |
| Search | T1 | OpenSearch | AI Search (L1) | Vertex AI Search (L1) | Search with OpenSearch | Databases for Elasticsearch | sidecar |
| Observability | T0 | CloudWatch Logs/Metrics/Alarms, X-Ray (ingest) | Monitor, Log Analytics, App Insights (ingest) | Cloud Logging, Monitoring, Trace | Logging, Monitoring | Cloud Logs, Monitoring | telemetry |
| DNS | T0 | Route 53 | DNS, Private DNS | Cloud DNS | DNS | DNS Services | dns |
| Networking (control) | T1 | VPC, subnets, SGs, ELB/ALB/NLB, NAT, endpoints | VNet, NSG, LB, App Gateway, Private Endpoints | VPC, firewall, LB, PSC | VCN, security lists/NSGs, LB | VPC, security groups, LB | crud (+ ALB HTTP routing to functions/containers L2) |
| API front doors | T1 | API Gateway (REST/HTTP), Lambda URLs | API Management (L1→L2 routing) | API Gateway | API Gateway | — | runtime routing |
| Workflows | T1 | Step Functions | Durable Functions (via Functions), Logic Apps (L1) | Workflows | — | — | workflow |
| Email / notifications | T1 | SES, SNS SMS/push | Communication Services Email/SMS | — | Email Delivery | Event Notifications (email/SMS) | mail |
| AI / LLM APIs | T1 | Bedrock | Azure OpenAI / AI Foundry | Vertex AI | Generative AI | watsonx.ai | AI gateway → local OpenAI-compatible server (vLLM/Ollama) or deterministic stub |
| Analytics | T2 | Athena, Glue, Redshift | Synapse, Data Factory | BigQuery (sidecar emulator), Dataflow | Data Flow | Data Engine | crud |
| Everything else in allowlisted specs | T2 | — | — | — | — | — | crud |

### 12.4 Cross-service behaviors required for T0
- Object events → queue / topic / function / stream on every cloud (S3 notifications, Event Grid blob events, GCS notifications/Eventarc, OCI Events, COS notifications).
- Queue/stream → function via trigger mappings with batching, retries, DLQ.
- Secrets rotation → function invocation.
- Function logs/metrics → telemetry engine; function identity → IAM evaluation on downstream calls.
- KMS-encrypted secrets/objects/queues decrypt only with permission under `enforce`.
- Registry images → runnable by functions/containers/k8s.

---

## 13. Functions and compute (`mcle-runtime`)

### 13.1 Runtime modes (`MCLE_FUNCTIONS=auto|container|process|mock`)
- **container** (default when Docker/Podman available): official/base images per cloud.
  - AWS Lambda: MCLE implements the **Lambda Runtime API** and sets `AWS_LAMBDA_RUNTIME_API`, so AWS base images and custom runtimes work unchanged.
  - Azure Functions: Functions host container; triggers/bindings read from the function app (`host.json`, `function.json` / worker indexing) `[VERIFY]`.
  - GCP: Functions Framework / buildpack-built containers; Cloud Run = any HTTP container `[VERIFY]`.
  - OCI Functions: Fn FDK containers (Fn-compatible invoke protocol) `[VERIFY]`.
  - IBM Code Engine: any container (apps, jobs, functions) `[VERIFY]`.
- **process** (no Docker — CI and Bazel friendly): runs handlers with host runtimes (`python`, `node`, `java`, native/`provided` binaries, `dotnet`) via a small per-language shim speaking the same runtime protocol. Hermetic enough for unit-of-deploy tests.
- **mock**: control plane only; invocations return configured canned responses.
- Packaging accepted: zip, container image (from our registry or local Docker), source dir (process mode).

### 13.2 Execution details
- Warm pools with configurable concurrency; cold-start simulation optional.
- Env vars, layers/extensions (Lambda), timeouts, memory (advisory), reserved concurrency (throttling errors).
- Function identity: each invocation gets credentials for its role/MI/SA via the identity layer → downstream calls are IAM-evaluated.
- Logs streamed to `telemetry` and to `mcle logs fn <name>`.

### 13.3 Triggers (`TriggerSource` → `TriggerSink`)
```rust
pub struct Trigger { source: EventSelector, target: Invocable, filter: Option<Filter>,
                     batch: BatchPolicy, retry: RetryPolicy, dlq: Option<ResourceId> }
```
Covers: Lambda event source mappings (SQS, Kinesis, DynamoDB Streams, Kafka/MSK), EventBridge rules, SNS subscriptions, S3 notifications, API Gateway/URLs; Event Grid subscriptions, Functions bindings (Blob, Queue, Service Bus, Event Hubs, Timer, HTTP); Pub/Sub push, Eventarc, GCS triggers, Cloud Scheduler; OCI Events rules, Connector Hub `[VERIFY]`; Code Engine subscriptions (COS, cron, Kafka) `[VERIFY]`.
**HTTP push to user services** (SNS HTTP, Pub/Sub push, Event Grid webhook with validation handshake) is first-class — key for testing new services.

### 13.4 Kubernetes and containers
- k3s cluster per EKS/AKS/GKE/OKE/IKS cluster (one container each); real kubeconfig returned by the cloud's credential API.
- Node groups/pools are L1 (reflected as k3s node labels).
- Container services (ECS tasks, Container Apps, Cloud Run, Container Instances, Code Engine apps) run as containers on the backend with ingress via the gateway.

---

## 14. Code generation

### 14.1 Inputs
| Cloud | Source | Format |
|---|---|---|
| AWS | `aws/api-models-aws` | Smithy JSON AST |
| Azure | `Azure/azure-rest-api-specs` (stable) | OpenAPI 2.0 (from TypeSpec) |
| GCP | Discovery docs + `googleapis/googleapis` | JSON Discovery + `.proto` |
| OCI | OCI OpenAPI specs `[VERIFY location]` | OpenAPI 3 |
| IBM | IBM OpenAPI specs `[VERIFY location]` | OpenAPI 3 |
Pinned in `specs/specs.lock` with licence `[VERIFY each permits vendoring]`.

### 14.2 Outputs (per service, into `services/<cloud>/<svc>/gen/`)
Shapes (serde), `OpId` enum, route table, typed facade trait with `NotImplemented` defaults, error enum, and **`OpMeta` defaults** (IAM action from catalog match, resource extractor from path params/ARN templates, LRO behavior from `x-ms-long-running-operation` / `google.longrunning` / OCI work-request markers).

### 14.3 Generic L1 CRUD (breadth on day 1)
The `crud` engine + codegen infer resource lifecycles and serve them without handwritten code:
- **Azure:** ARM PUT/GET/PATCH/DELETE/LIST on `/providers/{ns}/{type}/{name}` — uniform, ~fully automatic.
- **GCP:** resource-oriented AIP standard methods (Create/Get/List/Update/Delete) + `google.longrunning`.
- **OCI / IBM:** REST collection/item patterns + lifecycle-state fields.
- **AWS:** Smithy `@resource` lifecycle bindings where present `[VERIFY coverage in AWS models]`; otherwise heuristics (`Create*/Describe*/Get*/List*/Delete*/Update*`) with manual hints in the manifest.
Read-after-write returns stored input + server-generated fields (IDs, ARNs, timestamps, defaults from spec). Unknown outputs fall back to spec defaults; mismatches are fixed with manifest overrides, not core changes.

### 14.4 Rules
- Generated code checked in; `cargo xtask codegen --check` in CI.
- Never hand-edit `gen/`. Overrides go in `service.toml` or the facade.
- Protocol (de)serializers are handwritten per protocol family in `clouds/<cloud>` (AWS: restXml, restJson1, awsJson1_0/1_1, awsQuery, ec2Query; Azure ARM/data-plane REST; GCP JSON REST + gRPC; OCI/IBM REST).
- Compile-time budget: features per service; CI builds `all` nightly, per-PR builds only changed services + core.

---

## 15. CI integration

### 15.1 CI contract (non-negotiable)
- No accounts, tokens, egress, telemetry; air-gapped capable.
- No Docker for core; container features degrade to `process`/`mock` modes automatically.
- `--port 0 --port-file <path>`; wire-protocol ports also support `0` and are written to the port file (JSON).
- `/_mcle/health` vs `/_mcle/ready`; `mcle wait --timeout`; `mcle healthcheck` for distroless `HEALTHCHECK`.
- `MCLE_PUBLIC_HOST` used in every generated URL (presigned, queue URLs, `Location`, `Azure-AsyncOperation`, registry realms, Kafka advertised listeners).
- Determinism: `MCLE_SEED`, `MCLE_CLOCK=real|fixed:<rfc3339>`.
- JSON logs, request log, artifacts dir (`MCLE_ARTIFACTS_DIR`, `--artifacts always|on-failure`).
- Exit codes: `0` ok · `2` config · `3` bind · `4` readiness timeout · `10` strict violation · child code in wrapper mode.
- Clean SIGTERM within 2 s; sidecars/containers labelled and reaped on exit (and on next start if orphaned).

### 15.2 Wrapper mode — the universal primitive
```
mcle run [--clouds …] [--seed-file …] [--state-import …] [--faults …]
         [--iam off|audit|enforce] [--strict[=L1|L2]] [--functions auto|container|process|mock]
         [--artifacts-dir DIR] -- <command> [args…]
```
Start → ready → inject env → exec child → forward signals → artifacts → stop → child's exit code (or `10`).

### 15.3 Sandboxes
`POST /_mcle/sandboxes {ttl}` → `{id, env}`; isolation by credential per cloud; per-sandbox reset/export/report; TTL expiry. Store key includes `SandboxId`.

### 15.4 Coverage report & strict mode
- Records `(cloud, service, op, level, outcome, iam_decision)`.
- `mcle report --format md|json|junit`.
- `--strict` (= `--strict=L2`): fail if any call hit L0 **or** an L1-only op was called for data-plane behavior. `--strict=L1`: fail only on L0.
- IAM `audit` denials listed in the report as warnings; `enforce` makes them failures.

### 15.5 Fault injection
`faults.yaml` / `/_mcle/faults`: match `(cloud, service, op, resource glob, principal)` → inject native throttling/errors, latency, LRO failure, truncated pagination, connection reset, Kafka broker unavailability, function timeout/throttle.

### 15.6 Seeding & cached state
`mcle.seed.yaml` (declarative resources incl. IAM, functions from local dirs, topics, secrets), `--init-dir`, `mcle state export/import` (versioned format) for CI caching keyed by infra hash.

### 15.7 Distribution
Static musl binaries (linux x86_64/arm64), macOS universal, Windows (decision §22); distroless multi-arch image; SHA256SUMS, cosign, SLSA provenance; Homebrew; `cargo binstall`. Docs always pin versions.

### 15.8 Platform integrations
| Platform | Deliverable |
|---|---|
| GitHub Actions | `setup-mcle` action (download/cache, start, wait, `$GITHUB_ENV`, post-step artifacts + `$GITHUB_STEP_SUMMARY`) + `services:` container pattern |
| Buildkite | plugin (`environment` / `pre-exit` hooks, artifact upload, annotations) |
| CircleCI | orb (`install`, `start`, `wait`, `run`) + secondary-image pattern |
| GitLab CI | template (`services:` alias + `MCLE_PUBLIC_HOST`) |
| Jenkins | shared-library `withMcle {}` |
| Tekton / OpenShift Pipelines | Task + sidecar |
| Kubernetes | Helm chart (shared instance + sandboxes) |
| Testcontainers | Go, Java, Python, Node, Rust, .NET modules |
Each has an e2e workflow in its own CI. Marketplace mirrors as required `[VERIFY publishing rules]`.

---

## 16. Build-system integration

### 16.1 Bazel — `rules_mcle` (Bzlmod, BCR `[VERIFY process]`)
- Prebuilt binaries per exec platform with sha256; no Rust toolchain for consumers.
- `mcle_test(test, clouds, seed, state, faults, iam, strict, functions = "process")` — one instance per test/shard.
- `mcle_state(seed | tf_srcs)` — cacheable state snapshot action.
- `mcle_function(srcs, runtime, handler)` — packages a function for process/container mode as a Bazel output.
- `mcle_terraform_test(srcs, lock_file, terraform_toolchain)` — §17.3 pipeline.
- Hermetic: loopback only, `$TEST_TMPDIR`, outputs to `$TEST_UNDECLARED_OUTPUTS_DIR`, JUnit to `$XML_OUTPUT_FILE`, shard env honored, fixed seed/clock defaults. `[VERIFY]` loopback under `block-network`.
- RBE-friendly: static binary, `process` functions, no Docker.

### 16.2 Other tools
Gradle plugin + JUnit 5 extension; Maven/Testcontainers; Pants, Buck2, Nx/Turborepo, Make/Just via `mcle run` recipes in `examples/`.

---

## 17. Terraform and IaC testing

### 17.1 `mcle tf` / `mcle tofu`
Temporary `_mcle_override.tf` with endpoints + skip flags for `aws`, `azurerm`, `azuread`, `google`, `oci`, `ibm` (+ backend override); everything else passes through. `[VERIFY attribute names per provider version]`.

### 17.2 Remote state backends
`s3` (DynamoDB lock + native lockfile; also IBM COS), `azurerm` (blob lease), `gcs` (generation lock), `oci` `[VERIFY]` — locking tested with concurrent applies.

### 17.3 `mcle tf check`
`init → validate → plan → apply → plan (must be empty) → [terraform test] → destroy → mcle assert empty`. Outputs plan JSON, resource-type coverage, IAM audit findings, JUnit. Flags: `--keep`, `--export-state`, `--iam enforce`, `--strict`.

### 17.4 Frameworks
`terraform test` / `tofu test`; Terratest helpers; module-upgrade tests (apply vN → snapshot → plan vN+1 → assert expected diff).

### 17.5 Resource-type coverage
`coverage/terraform/<provider>.json` (generated): resource type → ops → level. `--strict` reports e.g. `azurerm_key_vault → Microsoft.KeyVault/vaults PUT: L1 (CRUD only)`. Stretch: provider acceptance-test subsets (`TF_ACC=1`) as parity suite.

### 17.6 Other IaC
1.0: OpenTofu, Pulumi, Crossplane, ARM/Bicep deployments (generic ARM deployment engine), CloudFormation (template engine over L1/L2 facades) as T1. Later: CDK synth flows beyond CFN, Deployment Manager.

### 17.7 Testing new application services (golden path)
1. `mcle tf apply` the service's infra (IAM, secrets, queues, streams, functions, DB, registry image).
2. `mcle state export` → CI cache.
3. `mcle run --state-import … --iam enforce --faults … --strict -- <integration tests>`.
4. Report shows every API touched, its fidelity, IAM decisions, and injected faults.

---

## 18. Testing strategy

| Layer | Tooling | Proves |
|---|---|---|
| Unit | `cargo nextest`, `proptest` | IDs, LRO, engines, IAM evaluators, serializers |
| Engine conformance | `mcle-testkit` | every facade on an engine meets shared semantics |
| Protocol snapshots | `insta` + redactions | response/error byte shapes |
| SDK integration | pytest + official SDKs (boto3, azure-*, google-cloud-*, oci, ibm-*); plus Go/Java/Node SDK smoke suites | unchanged clients work |
| Wire protocols | official Kafka clients (librdkafka, Java), Azure AMQP SDKs, `docker`/`crane` for registry | non-HTTP parity |
| IAM | policy fixtures per cloud: allow/deny matrices, parity-recorded denials | authz correctness |
| Terraform | `mcle tf check` per module per cloud | control plane + LRO |
| Parity | record against real accounts (opt-in) → replay in CI | fidelity vs. cloud |
| Reference apps | `examples/reference-app/<cloud>` end-to-end | the golden path works |

Rules: every L2 op has ≥ 1 SDK test + snapshot; every manifest claim is test-backed; tests hermetic (random ports, temp dirs, no egress).

---

## 19. Roadmap to 1.0

Organised by **workstreams** so contributors can work in parallel once P0 lands.

### 19.1 Phases
**P0 — Foundation (blocking; core team only)**
- Workspace, CI (fmt, clippy `-D warnings`, nextest, cargo-deny, `check-deps`), release pipeline skeleton.
- `mcle-sdk` v0.1 with all extension-point traits (§6.2); `mcle-core` (Scope, ResourceId, Store, LRO, Clock, event bus, sandboxes); gateway + tower pipeline; persistence.
- Codegen for all five spec formats → shapes, routes, traits, `OpMeta`; IAM action catalogs vendored.
- `crud` engine + generic L1 for one service per cloud.
- CI contract + `mcle run`; `mcle-testkit`; `xtask new-service`; `STATUS.md` generation; CONTRIBUTING + RFC template.
- **Exit:** a contributor can scaffold a new service and reach L1 in < 1 hour following CONTRIBUTING only; `mcle run -- true` < 300 ms; SDK v0.1 tagged, further changes need RFC.

**P1 — Breadth (parallel)**
- Generic L1 for all allowlisted services on all five clouds (§14.3).
- Identity layer for all clouds incl. metadata servers and CI OIDC federation.
- IAM control planes (L2) + evaluators in `audit` mode.
- **Exit:** Terraform apply/destroy succeeds at L1 for the top-50 resource types per provider (list in `coverage/terraform/top50.toml`).

**P2 — Tier-0 depth (parallel workstreams by category, not by cloud)**
| Workstream | Scope |
|---|---|
| WS-IAM | `enforce` mode, all evaluators to day-1 coverage (§9.3), `iam explain` |
| WS-Secrets | secrets, kms, config, certificates engines + facades |
| WS-Object | object engine + 5 facades + notifications |
| WS-Messaging | queue + pubsub engines + facades, HTTP push |
| WS-Streaming | log engine, Kinesis, `wire/kafka`, `wire/amqp`, Event Hubs/Service Bus, Event Streams, OCI Streaming |
| WS-Functions | runtime (container/process/mock), Lambda Runtime API, per-cloud function hosts, triggers |
| WS-Registry | `wire/oci-distribution`, registry facades, token flows |
| WS-Data | kvdoc engine + facades; relational/cache sidecars + no-Docker external mode |
| WS-Telemetry | logs/metrics/traces ingest, alarms, OTLP export |
| WS-DNS-Net | dns engine/server, networking L1, ALB/API GW routing to functions/containers |
- **Exit:** every T0 row in §12.3 at L2 on all five clouds; cross-service behaviors (§12.4) pass.

**P3 — Integrations (can start after P0, parallel with P1/P2)**
- GitHub Action + Bazel MVP first; then Buildkite, CircleCI, GitLab; then Jenkins, Tekton, Helm, Gradle, Testcontainers.
- `mcle tf` + backends + `tf check`; fault injection; seeding/state cache; sandboxes.

**P4 — 1.0 hardening**
- T1 rows to target levels; parity suites; reference apps; docs site with generated coverage tables; performance + memory budgets; security review of identity/token code; signed releases.

### 19.2 Suggested ordering inside P2
WS-IAM, WS-Secrets, WS-Object, WS-Messaging first (most services depend on them) → WS-Functions + WS-Streaming → WS-Registry, WS-Data, WS-Telemetry, WS-DNS-Net.

### 19.3 1.0 release gate
1. All T0 rows L2 on all five clouds (exceptions documented in `STATUS.md` with an issue link).
2. All T1 rows at target (§12.2).
3. Generic L1 for every allowlisted spec resource; top-50 Terraform resource types per provider pass `mcle tf check`.
4. IAM `enforce` passes allow/deny matrices for all T0 services.
5. `examples/reference-app/<cloud>` passes under `--iam enforce --strict` on GitHub Actions, Buildkite, CircleCI, and `bazel test`.
6. Design targets (§1.4) met in CI benchmarks.
7. `mcle-sdk` 1.0 published; external-plugin example passes conformance.

**Reference app (per cloud):** an HTTP service + worker that reads config & a secret, writes an object, gets an event, enqueues a message, a function consumes it, produces to a Kafka-compatible stream, a consumer writes to KV and a relational DB, everything logs/metrics to telemetry, images pulled from the cloud registry, all provisioned by Terraform with least-privilege IAM.

---

## 20. Engineering conventions

- **Preferred crates:** `tokio`, `hyper` 1.x, `hyper-util`, `tower`, `tonic`, `prost`, `rustls`, `rcgen`, `serde`, `serde_json`, `quick-xml`, `dashmap`, `uuid`, `time`, `jsonwebtoken`, RustCrypto (`sha2`, `hmac`, `aes-gcm`, `rsa`, `p256`, `ed25519-dalek`) or `ring`, `base64`, `bollard`, `redb`, `zstd`, `hickory-server`, `kafka-protocol` `[VERIFY]`, `fe2o3-amqp` `[VERIFY]`, `tracing`, `clap`, `thiserror`, `anyhow` (bins only), `async-trait`, `proptest`, `insta`.
- Ask before adding a dependency not on this list.
- `#![forbid(unsafe_code)]` everywhere unless justified in a decision record.
- No `unwrap()`/`expect()` outside tests and startup.
- Comments only where logic is non-obvious or parameters need explaining; `cargo doc` warning-free.
- Conventional commits (`feat(services/gcp/pubsub): …`); decision records in `docs/decisions/`.

---

## 21. Agent instructions

See `AGENTS.md` at the repo root (`CLAUDE.md` imports it).

---

## 22. Open decisions (for Sang)

1. ~~**Name + licence**~~ — **Decided:** name MCLE, licence Apache-2.0 (docs/decisions/0002-licence.md).
2. **Default ports** — HTTP `4700`, Kafka `9092`, AMQP `5672`; or offset Kafka/AMQP to avoid clashing with real local brokers?
3. **Kafka/AMQP** — native protocol servers in core (proposed) vs. Redpanda/broker sidecars only.
4. **Relational/cache data planes** — sidecar containers + external-URL mode (proposed) vs. embedded engines (e.g. PGlite-style) for no-Docker CI.
5. **Default IAM mode** — `audit` (proposed) vs. `enforce`.
6. **Plugin ABI beyond Rust** — out-of-process HTTP/gRPC in 1.0 (proposed); WASM components post-1.0?
7. **Persistence backend** — `redb` vs. serde snapshot files.
8. **Release scope** — single 1.0 for all five clouds vs. per-cloud 1.0s (e.g. AWS + Azure + GCP first, OCI + IBM as 1.1).
9. **Repo/org names** for marketplace integrations and BCR module.
10. **Windows runners** — best effort or supported in 1.0?
11. **Governance** — foundation-neutral from day 1 (e.g. CNCF sandbox path) or company/personal repo first?
