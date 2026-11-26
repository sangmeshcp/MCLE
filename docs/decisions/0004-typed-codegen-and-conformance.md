# 0004 — Typed codegen and conformance kit

Date: 2026-11-26 · Status: accepted

## Typed codegen

`cargo xtask codegen` now also writes, per service:

- `gen/shapes.rs` — serde structs from Smithy structures, OpenAPI `definitions` /
  `components.schemas` (with `allOf` flattened, as ARM models inherit `Resource`) and Discovery
  `schemas`. All fields are `Option` and unknown fields are ignored, so shapes tolerate spec
  drift. Struct-typed fields are boxed to allow recursive shapes. Enums, unions, timestamps and
  documents map to `String` / `serde_json::Value`.
- `gen/api.rs` — a typed `Api` trait with one method per op (default `NotImplemented`), a
  `Request<T>` wrapper (decoded body + raw request) and `dispatch` that decodes the JSON body,
  calls the method and encodes the result.

`Request<T>` lives in generated code rather than `mcle-sdk`, so the SDK public API is unchanged.

Limits: REST inputs are typed by request body only (path/query stay on `Request::raw`); output
is always a 200 JSON body, so ops needing other statuses or headers stay hand-written; XML
protocols (restXml, awsQuery) are not decoded yet.

## Conformance kit (`mcle-testkit::conformance`)

- `crud` — lifecycle invariants for any CRUD resource via a per-service `CrudDriver`
  (knobs: upsert/duplicate-name semantics, re-delete status). Runs against all five services.
- `adapter_errors` — every adapter returns 501 with its request-id header for unrouted calls.
- `metadata` — every op has an IAM action and a route; CRUD-bound ops are at least L1.
