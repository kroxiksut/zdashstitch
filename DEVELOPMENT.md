# Local development

ZDashstitch is still a scaffold. The commands below start only the local UI
shell and its read-only health endpoint; they do not connect to Zabbix.

## Prerequisites

- Rust 1.94.1 with `rustfmt` and `clippy` (pinned by `rust-toolchain.toml`)
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
cargo test --workspace
npm --prefix ui run check
npm --prefix ui test
npm --prefix ui run build
.\.venv\Scripts\python.exe tools\check_architecture_boundaries.py
```

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
cargo test -p zdashstitch-zabbix --test lab_read_only -- --ignored --nocapture
```

For password authentication, omit the token and set
`ZDASHSTITCH_LAB_USERNAME` plus `ZDASHSTITCH_LAB_PASSWORD`. Zabbix accounts
using MFA cannot authenticate through `user.login`; use an authorized API
token instead. The test prints only method status and object counts, never
object bodies or credentials.
