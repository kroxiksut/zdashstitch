# Local development

ZDashstitch is still a scaffold. The commands below start only the local UI
shell and its read-only health endpoint; they do not connect to Zabbix.

## Prerequisites

- Rust 1.94.1 with `rustfmt` and `clippy` (pinned by `rust-toolchain.toml`)
- [cargo-nextest](https://nexte.st), the Rust test runner used locally and in
  CI: `cargo install cargo-nextest --locked`
- Node.js 24 and npm 11
- Python 3.11+ only for the temporary reference checks
- Linux only: GTK 3 and WebKitGTK 4.1 development packages for the program
  window (see «Program window» below)

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

Only one instance runs per operating-system user. Running the same command
again while the server runs (for example after «End session» in Settings)
binds no second server: the new process asks the running one over a local
channel (Windows named pipe, Linux Unix socket; no network) for a fresh
one-time `Bootstrap URL`, prints it and exits. While a session is active the
URL opens another tab of it; after «End session» it is issued only once the
secrets of the ended session are erased, and it opens a new session. The
lock and endpoint files live in `%LOCALAPPDATA%\ZDashstitch\runtime` or
`$XDG_RUNTIME_DIR/zdashstitch`; set `ZDASHSTITCH_INSTANCE_DIR` (absolute) to
run a separate development instance next to the usual one. On Windows that
directory must lie inside `%LOCALAPPDATA%` (no junctions or links on the way),
on Linux it must be owned by you with mode `0700`; anything else, for example a
shared `C:\temp` folder, is refused with `instance.directory.insecure`.

## Program window (O-036)

The launcher `zdashstitch` (`apps/desktop`) starts the same loopback server
and shows the UI in the program window `zdashstitch-window`
(`apps/desktop_window`, a separate process on the system web engine: WebView2
on Windows, WebKitGTK 4.1 on Linux). Both executables must sit in the same
directory; `cargo run` builds only the launcher, so build the window first:

```powershell
npm --prefix ui run build
cargo build -p zdashstitch-window
cargo run -p zdashstitch-desktop
```

The window opens the one-time bootstrap URL like a browser does (it receives
it over stdin, never on the command line) and shows only
`http://127.0.0.1:<port>`; clicked `https://` links elsewhere open in the
system browser, anything else is blocked, `window.open` creates no window and
permission requests are denied. Developer tools (F12) and the browser
accelerator keys exist only in debug builds. On Windows the engine profile
lives in `%LOCALAPPDATA%\ZDashstitch\webview2` (in-private: no cookies or
storage are kept). Closing the window ends the local session like «End
session» and stops the server; while a write to Zabbix runs the window stays
open and says why. A repeated launch while the program runs opens a further
window of the same session; closing that one only closes it.

Browser mode: `cargo run -p zdashstitch-desktop -- --browser` or
`ZDASHSTITCH_BROWSER=1` opens the system browser instead (the server then
runs until Ctrl+C). If the window cannot start, the launcher shows a native
message with the install hint and the choice «I will install and restart» /
«Continue in the browser»; without a message box (no `zenity` on Linux) it
prints the text and opens the browser. `zdashstitch-local-server` stays the
browser-only server that only prints the URL.

Linux build dependencies (Debian, Ubuntu, Astra Linux 1.8):
`libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev`.
Runtime engine: RED OS 8 `sudo dnf install webkit2gtk4.1` (repository
`updates`), Debian and Astra Linux 1.8 `sudo apt install libwebkit2gtk-4.1-0`.
Astra Linux 1.7 has only WebKitGTK 4.0, so it uses the browser.

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

Through the Vite dev server only reads work: the browser sends the `Origin` of
the Vite port, so mutating requests get 403 `http.security.invalid_origin` by
design. Manual checks and any mutating flow use the production-like run (see
«Manual accessibility check»).

## Manual accessibility check

Keyboard use and 200 %/400 % zoom are checked manually on Windows. Screen
readers are not part of the manual check (owner decision O-039, 28.10.2026):
the markup supports them, but no screen reader pass has been done. If you test
one, NVDA on Windows and Orca on Linux are the references; please report
findings or send a pull request.

The opt-in DEMO build fills «Start», «Drafts» and one draft's sections
(«Overview», «Structure», «Bindings», «Publication») with synthetic data
(dashboards and pages, local and server copies, conflict and stale states,
long Russian and English names, several pages, an unknown widget type) and
shows a «Demo data» marker on every screen. `npm run build` never contains
this data: `ui/scripts/check-dist.mjs` fails the build if it leaks.

```powershell
npm --prefix ui ci
npm --prefix ui run build:demo
$env:ZDASHSTITCH_UI_DIR = (Resolve-Path ui/dist-demo).Path
cargo run -p zdashstitch-local-server
```

Open the printed `Bootstrap URL` in the browser within two minutes; it works
once. Open a draft by its name; its sections are in the left column. Check
keyboard order and focus, 200 % and 400 % zoom and every theme. Stop the
server with Ctrl+C, then run `Remove-Item Env:ZDASHSTITCH_UI_DIR`.

Connection, server dashboard and copy screens need a laboratory Zabbix: build
normally (`npm --prefix ui run build`), run `cargo run -p
zdashstitch-local-server` without `ZDASHSTITCH_UI_DIR`, and add a laboratory
Zabbix profile under «Connections».

## Optional local XERJ search

XERJ is an optional developer tool, not an application dependency. The pinned
local pilot uses XERJ `1.0.0-rc.100`, a loopback-only authenticated node, the
offline lexical embedder, and disabled feedback. The index may contain private
planning text and must not be published or committed.

Install the verified `xerj.exe` at
`%LOCALAPPDATA%\Programs\xerj\xerj.exe`, then use the helper:

```powershell
.\tools\xerj-local.ps1 start
.\tools\xerj-local.ps1 update
.\tools\xerj-local.ps1 search 'Точка продолжения P2-04G' -Prefix zdash-planning
.\tools\xerj-local.ps1 def VerifiedPlan
.\tools\xerj-local.ps1 status
.\tools\xerj-local.ps1 stop
```

`zdash-all` contains code and documentation. `zdash-planning` contains fresh
local copies of `TASKS_RU.md`, `OPEN_DECISIONS.md`, and
`IMPLEMENTATION_LOG.md`. `.xerjignore` explicitly makes those ignored source
files visible to the whole-repository index without making Git track them.
Never replace these narrow rules with `--no-ignore`: ignored trees may contain
credentials, laboratory state, logs, virtual environments, and build output.

The node stores its authenticated local index under `%USERPROFILE%\xerj-data`.
The helper never prints `admin.key`; MCP clients read the same local key through
XERJ's loopback discovery. Start the node before opening an MCP-backed agent
session, and restart that client after changing its MCP configuration.

## Checks

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo test --workspace --doc
cargo deny check advisories licenses
npm --prefix ui run check
npm --prefix ui test
npm --prefix ui run build
.\.venv\Scripts\python.exe tools\build_licenses.py --check  # regenerate without --check after changing dependencies
.\.venv\Scripts\python.exe tools\check_architecture_boundaries.py
```

nextest runs each test in a process of its own, ends a hung test (see
`.config/nextest.toml`) and does not run doctests, hence the separate
`cargo test --doc`. `cargo test --workspace` still runs the same suite when
nextest is not installed. Run one test with
`cargo nextest run -p <crate> <test-name-substring>`.

`cargo deny check advisories licenses` enforces the dependency licence
allow-list (`[licenses] allow` in `deny.toml`, permissive licences only) and
the recorded advisory exceptions. `tools/build_licenses.py` reads the same list
for npm and Rust packages, so a package outside it fails both checks. An
exception for a single package needs an owner decision recorded with its reason.

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

`lab_write_contracts_p2` (LAB-02: several pages, `svggraph` items and host
names, classic `graph`, `dashboard.create`) writes only to dashboards it
creates under the `ZDashstitch Contract Lab —` prefix and keeps them as
evidence; it also maintains one non-Super-admin user and its group. Contract
updates go through the adapter's closed publication path, so a pass proves
the production read-back on the canonical fingerprint. It needs its own
acknowledgement in addition to the endpoint and password variables:

```powershell
$env:ZDASHSTITCH_LAB_P2_WRITE_ACK='write-authorized-7.4-p2-contract-dashboards'
cargo nextest run -p zdashstitch-zabbix --test lab_write_contracts_p2 --run-ignored only --no-capture
```
