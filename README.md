# MCLE

Multi-cloud local emulator: one fast binary that emulates AWS, Azure, GCP, OCI and IBM Cloud
APIs locally so official SDKs, CLIs and Terraform providers work with endpoint configuration only.

Status: **P0 — Foundation**. See [STATUS.md](STATUS.md) for what works per service and
[DESIGN.md](DESIGN.md) for the plan.

## Quick start

```sh
cargo build --release -p mcle-cli
./target/release/mcle start                 # listens on :4700, all clouds
eval "$(./target/release/mcle env)"         # SDK endpoint/credential exports

# or wrap a test command: start → inject env → run → report → stop
./target/release/mcle run --strict --artifacts-dir out -- pytest tests/
```

| Cloud | Point the SDK at |
|---|---|
| AWS | `AWS_ENDPOINT_URL=http://localhost:4700` |
| Azure | `base_url=http://localhost:4700` (ARM) |
| GCP | `api_endpoint=http://localhost:4700` (REST transport) |
| OCI | `service_endpoint=http://localhost:4700` |
| IBM | service URL and IAM URL `http://localhost:4700` |

Useful flags: `--port 0 --port-file p.json`, `--iam off|audit|enforce`, `--seed N`,
`--clock fixed:<rfc3339>`, `--lro-delay-ms N`, `--faults faults.json`, `--state-import state.json`.
Internal API: `/_mcle/{health,ready,info,env,reset,report,state/export,state/import,sandboxes,faults,lro,iam/explain}`.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md).

Licence: Apache-2.0.
