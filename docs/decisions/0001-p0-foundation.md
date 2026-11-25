# 0001 — P0 foundation: verified items and deviations

Date: 2026-11-25 · Status: accepted

## Verified with official SDKs (tests/sdk)

| Item | Result |
|---|---|
| boto3 `secretsmanager` with `endpoint_url` + any access key | Works (awsJson1_1, `__type` errors, `x-amzn-requestid`). |
| AWS sandbox selection via access key | Works: `POST /_mcle/sandboxes` returns an `MCLESBX…` access key that isolates state. |
| `google-cloud-secret-manager` with `transport="rest"` + `api_endpoint` | Works. Default gRPC transport is not served yet (P1, tonic). |
| google-api-core maps HTTP 409 to `Conflict` | Confirmed; `ALREADY_EXISTS` body status is still returned. |
| `oci` SDK with a real RSA signer + `service_endpoint` | Works; signature parsed (`keyId`), not verified. Config requires a well-formed 16-byte fingerprint. |
| `ibm-platform-services` + `IAMAuthenticator(url=…)` | Works; the core SDK accepts an unsigned (`alg: none`) JWT from `/identity/token`. |
| `azure-mgmt-resource` (26.x: `azure.mgmt.resource.resources`) | Works over plain HTTP only with a custom `authentication_policy`; `BearerTokenCredentialPolicy` refuses non-TLS URLs. TLS (P1) removes this workaround. |
| ARM empty `parentResourcePath` | SDK sends `ns//type/name`; MCLE normalises ids to `ns/type/name`. |

## Deviations from DESIGN.md

- **Pipeline**: implemented as an ordered list of `PipelineLayer`s around dispatch rather than
  `tower` layers. Same extension contract; can move to `tower` without SDK changes.
- **TLS, gRPC**: not in P0. Gateway is HTTP/1.1 + h2c only.
- **Specs**: hand-trimmed excerpts matching upstream names and paths (`specs/specs.lock`,
  `status = excerpt`). Vendoring pinned upstream files is pending a licence check per source.
- **IAM**: framework, modes, explain log and catalogs are in place; evaluators are allow-all
  until WS-IAM. `enforce` therefore never denies yet.
- **Persistence**: JSON snapshot files (`mcle-state/v1`); `redb` decision still open (§22.7).
- **Azure ARM IAM actions** for generic resources are templates (`{resourceType}/write`),
  to be expanded by the IAM layer when evaluators land.

## Still [VERIFY]

- IBM Resource Controller error envelope and delete status code.
- OCI/IBM OpenAPI upstream locations and licences.
- `AWS_ENDPOINT_URL_<SVC>`, Azure authority override, GCP `CLOUDSDK_API_ENDPOINT_OVERRIDES_*`
  env vars for CLIs (only SDK constructor overrides were tested).
