# 0003 — Vendored upstream specs

Date: 2026-11-26 · Status: accepted

## Vendored (pinned, unmodified; see specs/specs.lock)

| Spec | Upstream | Licence |
|---|---|---|
| AWS Secrets Manager (Smithy JSON AST) | aws/api-models-aws @ d504436b | Apache-2.0 |
| Azure Microsoft.Resources `resources.json` 2021-04-01, `subscriptions.json` 2021-01-01 | Azure/azure-rest-api-specs @ a4cb6169 | MIT |
| GCP Secret Manager v1 Discovery | googleapis/google-api-go-client @ 1be59361 | BSD-3-Clause |

Licence texts are in `specs/LICENSES/`. All three are compatible with redistribution in an
Apache-2.0 project.

## Codegen changes needed by the real specs

- `service.api.extra_specs`: Azure splits subscriptions into a second file; files are merged and
  the first definition of an op name wins (`Operations_List` appears in both).
- `[ops.X] path`: Azure models generic resources as
  `{resourceProviderNamespace}/{parentResourcePath}/{resourceType}/{resourceName}` with
  unencoded multi-segment values; the ARM facade overrides these five routes to a greedy
  `{resourcePath+}`.
- `[extra_ops.X]`: routes absent from any upstream spec but required for parity
  (subscription-level list by resource type, normally defined in each provider's own spec).
- IAM catalog membership is checked only for ops above L0, since catalogs are still excerpts.

## Effect

Ops tracked: 56 → 127. GCP location-scoped secrets (`projects/*/locations/*/secrets`) are now L1.
All newly visible ops without bindings report L0 (`NotImplemented`) as before.

## Not vendored

- OCI and IBM: no public spec repository found on GitHub, and docs.oracle.com / cloud.ibm.com
  are blocked by the build environment's network policy. Excerpts remain until a source is
  confirmed.
- IAM action catalogs remain excerpts; the AWS Service Authorization Reference, Azure provider
  operations and GCP permissions reference are published as docs pages, not versioned data.
