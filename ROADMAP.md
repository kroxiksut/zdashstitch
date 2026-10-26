# ZDashstitch roadmap

[Русская версия](ROADMAP.ru.md) · [MVP in README](README.md#planned-mvp-010) · [Documentation map](docs/README.md)

**Status:** product direction after MVP, not a release promise or evidence of implemented features. The MVP itself is defined in the README, ADRs and architecture contracts. No post-MVP item may start merely because it appears here.

## Baseline: MVP 0.1.0

The baseline is a local Rust/Axum/Svelte GUI for global dashboards on **Zabbix 7.4 only**. It must prove offline creation of a typed dashboard project or page project, portable YAML/data-only ZIP, Create/Append/Clone/ReplaceHost operations, explicit preview and a closed verified publisher. The existing multi-dashboard `project/v1` remains a legacy container until P0-08 implements the format decided in O-026. See the [README](README.md#planned-mvp-010) for scope, exclusions and quality gates.

## How priorities are chosen

There are no fixed version numbers or dates for the directions below. Order is chosen from demonstrated administrator time savings, user demand, security evidence and available test environments. Every proposal needs dependencies, risks and measurable acceptance criteria. Supporting one Zabbix branch never implies support for another.

| Horizon | Direction | Trigger | Required foundation |
|---|---|---|---|
| A — operational scale | F-01 Multi-dashboard operations | Repeated maintenance across several dashboards or zones | Verified MVP deployment state and journal |
| A — compatibility | F-02 More Zabbix versions | Concrete demand and an available test environment | Stable adapter contract and independent capability tests |
| B — reuse | F-03 Community recipes | Stable portable format and contribution demand | Package validator, licensing and provenance rules |
| B — richer dashboards | F-05 Visual presets and Custom Widgets | Native widgets no longer meet real use cases | Versioned codecs and separate server-module security model |
| B — broader coverage | F-07 More dashboard objects | Real fixtures for additional widgets/template dashboards/LLD | P0-like tests per object and version |
| B — authoring help | F-08 Live-data preview | The MVP synthetic preview is not enough to check a dashboard | Limits on Zabbix load and rules for sensitive values |
| C — assisted creation | F-04 AI-oriented exchange | Approved data policy and stable redaction workflow | Explicit export boundary and human confirmation |
| C — collaboration | F-06 Organizational server mode | Sustained demand from multiple administrators | New network auth, TLS, roles, audit and storage contracts |

## F-01 — multi-dashboard operations and repeatable maintenance

- Apply an explicitly selected typed project/recipe to several dashboards or environments without turning that project into a multi-dashboard workspace.
- Compare desired and actual state, detect manual changes and resolve conflicts.
- Support grouped metric/host/layout changes without claiming atomicity across Zabbix API calls.
- Extend history and recovery on top of verified `DeploymentState`, identity mapping and the MVP operation journal.

**Acceptance signal:** recurring measured time savings while maintaining multiple real dashboards, with unchanged content demonstrably preserved.

## F-02 — additional Zabbix generations

- Candidate order from current needs: 7.0 LTS, 6.0 LTS, 8.0, then a separate feasibility assessment for 5.4/5.0.
- Each generation gets its own API adapter, export parser profile, capability matrix, fixtures and contract tests.
- Reintroduce macOS CI, packaging and real-Mac acceptance during this multi-version compatibility stage; macOS is not built or supported by the MVP.
- `core`, portable projects and the UI remain version-neutral; unsupported behavior fails closed.
- A widget compatibility matrix from the official Zabbix documentation: widget type and its fields → the versions where they exist and change. It refines the compatibility label of a dashboard or page (O-029). In the MVP the label is computed for 7.4 only, but the version-range model and the widget capability registry are built in from the start.

**Acceptance signal:** the same portable project produces a verified result or an explicit compatibility refusal, never silent feature loss.

## F-03 — recipe library and community contributions

- Data-only dashboard/widget recipes for built-in and custom templates, delivered through reviewed contributions.
- Explicit catalog dependencies, authorship, licenses, screenshots, supported versions and automated import checks.
- Offline import remains first-class even if an optional online catalog is introduced.
- Native-widget presets and executable Zabbix Custom Widgets remain different artifact classes.

## F-04 — controlled AI-assisted exchange

- A dedicated “prepare for external sharing” flow with allowlisted fields, explicit review and replacement of potentially confidential names/keys.
- Stable schemas and examples for file-based generation by external or local models.
- A later constrained local `validate`/`preview` API may be considered; publishing still requires a human and a verified plan.

**Limit:** automatic de-identification is never guaranteed, and models receive no corporate credentials or local bindings.

## F-05 — richer visuals and Custom Widgets

- Faster styling presets, layout tooling and themes within supported native Zabbix widgets.
- Separately installed Zabbix UI Custom Widgets for use cases such as heatmaps or advanced indicators.
- PHP/JS/CSS server modules require their own delivery, compatibility and security lifecycle and never enter data-only packages.

The goal is to improve native Zabbix dashboards, not to replace Grafana/Prometheus as a whole.

## F-06 — optional organizational server mode

- Shared projects, versions, approvals, access control and audit for multiple administrators.
- Reusing Rust/Axum code does not make the MVP loopback security model suitable for a network service.
- This direction requires a new threat model, TLS, accounts, roles, remote-request protections and a storage contract.

It remains optional and must not weaken local/offline operation.

## F-07 — broader dashboard object coverage

- Template dashboards (`templatedashboard.*`), more built-in widgets, richer LLD handling, dynamic host selection and complex widget references.
- Every new object/operation/version combination requires real fixtures and P0-like read/write contract tests.

## F-08 — live-data preview

- When connected to Zabbix, show real item values in the preview (`history.get`/`trends.get`) only on an explicit user action.
- Limit server load: request count, period and number of points, cancellation and an in-memory cache only.
- Real values may be sensitive: they are never stored in drafts, files, exports, logs or diagnostics.
- The MVP synthetic preview (O-028) stays for offline work and is always labelled.

**Acceptance signal:** the live preview causes no noticeable load on the test Zabbix and leaves no values on disk.

## F-09 — trusted transfer with real bindings

- Move a draft between your own trusted machines together with its host bindings, without re-binding by roles (O-011; the MVP uses the portable package and re-binding).
- A separate versioned encrypted container (`zdashstitch.private-transfer/v1`): password-derived key, integrity check, manifest with fingerprints; secrets are never transferred.
- Requires its own decision, a crypto library selection (version, licence, security) and an ADR before implementation.

**Acceptance signal:** the container opens only with the password, corruption or tampering is detected before anything is applied, and the portable DATA-ONLY ZIP is unchanged.

## Roadmap governance

- Evidence and completed gates belong in `IMPLEMENTATION_LOG.md`; unresolved choices belong in `OPEN_DECISIONS.md`.
- Material changes to public formats, security boundaries, supported versions or MVP scope require owner approval and an ADR where appropriate.
- Portable data and executable code remain separate in every future direction.

