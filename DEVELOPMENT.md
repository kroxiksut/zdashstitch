# Local development

ZDashstitch is still a scaffold. The commands below start only the local UI
shell and its read-only health endpoint; they do not connect to Zabbix.

## Prerequisites

- Rust 1.94.1 with `rustfmt` and `clippy` (pinned by `rust-toolchain.toml`)
- [cargo-nextest](https://nexte.st), the Rust test runner used locally and in
  CI: `cargo install cargo-nextest --locked`
- Node.js 24 and npm 11
- Python 3.11+ only for the temporary reference checks

Rust output on the owner Windows host is redirected by the ignored
`.cargo/config.toml` to `C:\temp\zdashstitch\target`.

## Production-like local smoke run

```powershell
npm --prefix ui ci
npm --prefix ui run build
cargo run -p zdashstitch-local-server
```

The server binds only to `127.0.0.1` and chooses an ephemeral port. Open the
printed URL manually. Set `ZDASHSTITCH_UI_DIR` only when testing another local
UI build directory.

## Frontend development

Run the backend on the development port in one terminal:

```powershell
$env:ZDASHSTITCH_PORT='4173'
cargo run -p zdashstitch-local-server
```

Run Vite in another terminal:

```powershell
npm --prefix ui run dev
```

Vite binds to loopback and proxies only `/api` to the local Rust process.

## Checks

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo test --workspace --doc
npm --prefix ui run check
npm --prefix ui test
npm --prefix ui run build
.\.venv\Scripts\python.exe tools\check_architecture_boundaries.py
```

nextest runs each test in a process of its own, ends a hung test (see
`.config/nextest.toml`) and does not run doctests, hence the separate
`cargo test --doc`. `cargo test --workspace` still runs the same suite when
nextest is not installed. Run one test with
`cargo nextest run -p <crate> <test-name-substring>`.

The existing Python suite remains a reference specification, not an
application runtime dependency.

## Opt-in read-only Zabbix 7.4 laboratory acceptance

The ignored integration test performs only `apiinfo.version`, authentication,
`template.get`, `host.get`, `item.get`, `dashboard.get`, optional `graph.get`,
and `user.logout`. Run it only against an explicitly authorized laboratory VM.
The acknowledgement is deliberate, and credentials remain environment-only:

```powershell
$env:ZDASHSTITCH_LAB_ACK='read-only-authorized-7.4'
$env:ZDASHSTITCH_LAB_API_URL='https://lab.example/api_jsonrpc.php'
$env:ZDASHSTITCH_LAB_API_TOKEN='<temporary test token>'
cargo nextest run -p zdashstitch-zabbix --test lab_read_only --run-ignored only --no-capture
```

For password authentication, omit the token and set
`ZDASHSTITCH_LAB_USERNAME` plus `ZDASHSTITCH_LAB_PASSWORD`. Zabbix accounts
using MFA cannot authenticate through `user.login`; use an authorized API
token instead. The test prints only method status, object counts and
content-free dashboard aggregates (widget types, numeric field types, page and
widget distributions, name lengths), never object bodies, names or credentials.

`lab_error_taxonomy` (design input P1-IN-01) classifies connection and
authentication failures. It calls only `apiinfo.version` and one read method
with a fixed dummy token, so no real credential is sent. It needs
`ZDASHSTITCH_LAB_ACK`, `ZDASHSTITCH_TAXONOMY_LAB_BASE` (front-end base URL
without `api_jsonrpc.php`) and, for TLS cases, synthetic HTTPS fixtures on
ports 8441-8445 with `ZDASHSTITCH_TAXONOMY_TLS_HOST` and
`ZDASHSTITCH_TAXONOMY_TLS_CA`. Results are summarized in
`docs/design/backend_inputs_ru.md`.

The laboratory tests (`lab_*` test binaries) share one VM and one contract
dashboard; `.config/nextest.toml` places them in a test group that runs one
at a time, so selecting several of them never produces concurrent writes.
