# ZDashstitch

![Status: in development](https://img.shields.io/badge/status-in%20development-0d6efd)
[![Specification checks](https://github.com/kroxiksut/zdashstitch/actions/workflows/spec-tests.yml/badge.svg)](https://github.com/kroxiksut/zdashstitch/actions/workflows/spec-tests.yml)
[![Application checks](https://github.com/kroxiksut/zdashstitch/actions/workflows/build.yml/badge.svg)](https://github.com/kroxiksut/zdashstitch/actions/workflows/build.yml)
![Rust 1.94.1](https://img.shields.io/badge/Rust-1.94.1-000000?logo=rust&logoColor=white)
![Axum 0.8.9](https://img.shields.io/badge/Axum-0.8.9-6f42c1)
![Svelte 5.57.2](https://img.shields.io/badge/Svelte-5.57.2-FF3E00?logo=svelte&logoColor=white)
![Zabbix 8.0: planned target](https://img.shields.io/badge/Zabbix%208.0-planned%20target-D40000)
![Zabbix 7.4.16: laboratory contracts](https://img.shields.io/badge/Zabbix%207.4.16-lab%20contracts-6b7280)
![Target platforms](https://img.shields.io/badge/target-Windows%2010%2B%20%7C%20Linux-0078D4)
[![Code license](https://img.shields.io/badge/code-MIT%20OR%20Apache--2.0-blue)](LICENSING.md)
[![Documentation license](https://img.shields.io/badge/docs-CC%20BY%204.0-lightgrey)](LICENSE-DOCS)

**Build, reuse and safely maintain Zabbix dashboards — without repeating the same manual work.**

![ZDashstitch visual direction](assets/brand/banners/readme-en.png)

> The banner and other images in `assets/` describe the intended visual direction. They are not screenshots of an implemented application.

[Русский](README.ru.md) · [Roadmap](ROADMAP.md) · [Documentation](docs/README.md) · [Architecture](docs/architecture/ARCHITECTURE_MAP.md) · [Contributing](CONTRIBUTING.md)

> **Project status (17 November 2026):** `project/v2`, offline editing and parts of the protected local HTTP publication flow have been implemented; the complete user-facing workflow and acceptance are unfinished. Narrow transport and write contracts were tested on laboratory Zabbix 7.4.16. **Zabbix 8.0 is the target, not yet an implemented or accepted adapter.**

## Why ZDashstitch?

Maintaining dashboards in Zabbix can become repetitive: a PostgreSQL dashboard is recreated for the pilot environment; a ClickHouse page combines metrics from ClickHouse, Keeper, HAProxy and Linux templates; a retired host must be replaced across many widgets; a whole page needs to be copied without breaking existing content.

ZDashstitch aims to solve these cases with a **local graphical dashboard builder** and explicit, reviewable operations. It is not a monitoring backend, a replacement for Zabbix, or a clone of Grafana.

## Planned MVP 0.1.0

The first release prioritizes **global dashboards on Zabbix 8.0**. Existing 7.4 work is retained without claiming complete support; 8.0 support is planned, not yet implemented or accepted.

### User outcomes

1. Replace a retired host across supported widgets in one selected dashboard; unresolved references block publication.
2. Within one Zabbix connection, clone a global dashboard from Prod to Pilot host groups, explicitly bind new hosts/metrics and leave the source dashboard unchanged.
3. Build a DB Pilot dashboard with PostgreSQL, ClickHouse, OS and Backup pages; the ClickHouse page may combine ClickHouse, Keeper, HAProxy and Linux catalogs.
4. Import supported template exports offline, create a portable YAML/data-only ZIP project, then bind real hosts and create or extend a dashboard inside the target environment.

### Required capabilities

| Area | MVP result |
|---|---|
| Offline import | Normalize supported YAML/JSON/XML template exports into metric catalogs with explicit diagnostics. The 7.4 profile retains its existing checks. The 8.0 profile has been tested against synthetic files based on the official format. |
| Typed offline drafts | A dashboard project or page project opens on another computer without Zabbix or internet access; `project/v2` stores one `dashboard|page` root, while multi-dashboard `project/v1` remains readable and requires explicit root selection for migration |
| Authentication | API token or username/password where server policy permits `user.login`; no MFA bypasses. Credentials are stored in the OS credential store only if the user chooses to save them. |
| Dashboard operations | CreateDashboard, SaveAsCopy, UpdateDashboard, AppendPage, AppendWidgets, CloneDashboard and ReplaceHost for global dashboards (full list in ADR-006) |
| Initial widgets | Create modern Graph (`svggraph`) and Item value (`item`); other widget changes require separate proof |
| Style profiles and widget blueprints | Base style profiles and single-widget blueprints for `svggraph` and `item` (O-012, option B): a profile copies field values into a widget; a blueprint is one widget without position whose sources are “host role” and “metric” parameters, bound explicitly; both are exported as separate library files, never inside a draft package |
| Mixed sources | Multiple templates on one page, multiple hosts in a graph and per-host item cards |
| Safe publishing | Read → resolve → preflight → preview → confirm → re-read → write → verify; unknown outcomes are reconciled, never blindly retried |
| Local GUI | Rust + Axum on loopback with a Svelte/TypeScript UI in a native window on the system web engine (WebView2 / WebKitGTK 4.1), browser fallback; no end-user CLI/TUI or required company server |
| Languages and themes | All user-visible Rust/TypeScript/Svelte messages come from data-only catalogs; English source plus complete Russian translation; system/light/dark/high-contrast themes use semantic tokens |

### Quality gates

| Gate | Evidence required |
|---|---|
| G0 | Rust foundation plus read/write contracts proven on an explicitly permitted test Zabbix 8.0 and a closed `VerifiedPlan` publisher; historical 7.4 evidence remains version-specific |
| G1 | Staged: G1a — approved layouts, states, localizable copy, and implementation handoff for connections, import, ReplaceHost, Clone, and publishing; G1b — for the remaining MVP flows |
| G2 | ReplaceHost preserves unrelated widget data and safely refuses unsupported references |
| G3 | CloneDashboard produces a verified copy without changing the source |
| G4 | Offline project creation and transfer work without Zabbix or internet access |
| G5 | A dashboard project creates a dashboard and a page project appends to an explicitly selected dashboard without damaging existing content |
| G6 | End-to-end Windows/Linux GUI acceptance, documented support matrix and known limitations |

Passing a schema test does not satisfy any live Zabbix gate.

### Not in the MVP

Adapters for Zabbix 7.0/6.0/5.x, template dashboards, automatic bidirectional synchronization, a community catalog, built-in AI/MCP, arbitrary CSS/JS/PHP, server-side Custom Widgets, a shared multi-user service and an end-user CLI/TUI are excluded. See the [post-MVP roadmap](ROADMAP.md).

**Target platforms:** Windows 10+ and tested Linux desktops (WebKitGTK 4.1 for the program window, otherwise a browser). macOS builds and support are deferred until the post-MVP multi-version Zabbix work. The implementation is Rust + Axum on local loopback with a Svelte/TypeScript UI, shown in a native window via the system web engine with a browser fallback (O-036).

## Offline design and data boundaries

```text
Zabbix template exports / normalized catalogs
                  ↓
         Portable dashboard project
                  ↓                 (may be created using AI externally)
      project.yaml + metrics/*.yaml
                  ↓  transfer via approved channel
         Local binding to real hosts
                  ↓
    validation → preview → confirmation
                  ↓
           Zabbix API adapter
```

**Portable is not synonymous with anonymized.** Template names, item keys and free text may reveal internal details. Real host IDs, connection credentials, raw dashboard snapshots and deployment state are stored separately and must not be included in portable packages. Whether external AI may receive any project data is subject to the organization's policy.

Zabbix 8.0 is the priority target; existing 7.4 implementation and evidence are retained. Shared logic should be reusable across versions, while each adapter and operation **must** be tested independently. Unknown or unverified write capabilities fail closed.

## Implementation direction

- Rust 1.94.1 / edition 2024 workspace: `core`, `template_import`, `zabbix`, `application`, `apps/local_server`, `apps/desktop` (launcher `zdashstitch`) and `apps/desktop_window` (program window).
- Axum serves the UI and local API on loopback; session, Origin/Host, CSRF and DNS-rebinding protections guard local routes. Publication remains restricted to independently verified operations.
- Svelte/TypeScript provides the GUI and never talks to Zabbix directly.

The original P0-01 shell is only the starting point; later work adds parts of an offline editor and publication flow, neither yet accepted end to end. Build and local-run commands are documented in [`DEVELOPMENT.md`](DEVELOPMENT.md). Future product directions are consolidated in the bilingual [roadmap](ROADMAP.md). Concrete user stories remain in [project goals](docs/product/PROJECT_GOALS.md) and [acceptance use cases](docs/product/USE_CASES.md).

## Why Python still exists

Python is a **temporary executable specification** for development. It deterministically generates the JSON Schemas, validates portable YAML/data-only ZIP and cross-file invariants, and supplies positive and negative fixtures for parity with Rust.

P0-02 and P0-03 now provide equivalent Rust models, semantic validators and a safe directory/data-only ZIP layer. Python remains developer-only as the single schema generator and an independent regression oracle; it is no longer needed in the normal product workflow and is never shipped with the GUI.

## Current repository contents

| Location | Purpose |
|---|---|
| [`docs/README.md`](docs/README.md) / [`docs/README.ru.md`](docs/README.ru.md) | Documentation map, authority and language status |
| [`docs/MVP_TRACEABILITY.md`](docs/MVP_TRACEABILITY.md) | MVP use cases mapped to P0–P6 tasks, G0–G6 gates and acceptance evidence |
| [`ROADMAP.md`](ROADMAP.md) / [`ROADMAP.ru.md`](ROADMAP.ru.md) | Consolidated post-MVP directions |
| [`AGENTS.md`](AGENTS.md) | Mandatory start point and safety rules for coding agents |
| [`docs/adr/`](docs/adr/) | Accepted architecture and licensing decisions 001–014 |
| [`docs/architecture/`](docs/architecture/) | Adapter and portable-format contracts |
| [`schemas/`](schemas/) | Generated JSON Schemas |
| [`examples/`](examples/) | Synthetic dashboard, metric catalogs, local-state examples |
| [`fixtures/formats/v1/`](fixtures/formats/v1/) | Positive and negative fixtures shared by Python and Rust |
| [`tools/` and `tests/`](tools/) | Python reference validators and tests; not the product runtime |
| [`assets/`](assets/) | Target visual direction and Stezhik concept art; not product screenshots |
| [`LICENSING.md`](LICENSING.md) | Software/documentation license split and boundaries |

### Validate the specification

For documentation contributors (Python 3.11+; dependencies are pinned in `requirements-reference.txt`):

```bash
python -m pip install -r requirements-reference.txt
python tools/build_schemas.py
python tools/validate.py examples/db-pilot.yaml
python tools/validate.py --package examples/db-pilot-package.zip
python -m pytest -q
```

These checks establish **schema/reference validation only**. They do not demonstrate a functional GUI or safe updates against a live Zabbix server.

## Accessibility

The interface is designed for keyboard-only use, 200 % and 400 % zoom, light, dark and high-contrast themes and the operating system's forced-colors mode; keyboard use and zoom were checked manually on Windows. Screen reader support is built into the markup (landmarks, accessible names, live regions for errors and progress), but it **has not been tested with a screen reader**. NVDA on Windows and Orca on Linux are the suggested references. Reports and pull requests from screen reader users are very welcome.

## Contributing and license

Contributions are welcome, including translation-only pull requests, synthetic fixtures, documentation, future dashboard recipes and verified compatibility tests. New experimental languages may use English fallback; code changes must be submitted separately from translations. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md); never commit corporate hosts, addresses, tokens or raw infrastructure exports.

Original source code and code-like material are dual-licensed under [MIT](LICENSE-MIT) **OR** [Apache-2.0](LICENSE-APACHE). Documentation prose and original project visuals are licensed under [CC-BY-4.0](LICENSE-DOCS). See [LICENSING.md](LICENSING.md) for the exact boundaries. Zabbix is a separate project with its own licenses and trademarks; ZDashstitch is an independent tool using documented APIs.
