# Instructions for coding agents working on ZDashstitch

**READ THIS BEFORE WRITING CODE.** This repository is a design/specification starter, **not** a working Zabbix dashboard editor. Do not claim otherwise.

## 1. Product purpose

ZDashstitch must save administrators time creating, cloning and maintaining **global Zabbix dashboards**. First real use: Mamonsu/PostgreSQL and ClickHouse in Prod/Pilot on Zabbix 7.4, including multiple custom templates per page, offline project creation and replacing a host across many widgets. The target is a free, local, GUI-only tool; code is licensed `MIT OR Apache-2.0`, documentation and original visuals are `CC-BY-4.0`; no end-user TUI/CLI and no server deployment in MVP.

## 2. Required reading order

1. [`README.ru.md`](README.ru.md), [`docs/README.ru.md`](docs/README.ru.md), [`docs/product/PROJECT_GOALS.md`](docs/product/PROJECT_GOALS.md), and [`docs/product/USE_CASES.md`](docs/product/USE_CASES.md).
2. [`ROADMAP.ru.md`](ROADMAP.ru.md) for post-MVP awareness and the local ignored `TASKS_RU.md`, when present, for current P0–P6 execution state.
3. [`docs/adr/ADR-001-005.md`](docs/adr/ADR-001-005.md), [`ADR-006-010`](docs/adr/ADR-006-010.md), [`ADR-011`](docs/adr/ADR-011.md), [`ADR-012`](docs/adr/ADR-012.md), [`ADR-013`](docs/adr/ADR-013.md).
4. [`docs/architecture/domain_model_v1_ru.md`](docs/architecture/domain_model_v1_ru.md), [`adapter_contract_v1_ru.md`](docs/architecture/adapter_contract_v1_ru.md), [`format_contract_v1_ru.md`](docs/architecture/format_contract_v1_ru.md), and [`ui_i18n_theme_contract_v1_ru.md`](docs/architecture/ui_i18n_theme_contract_v1_ru.md).
5. `schemas/`, `examples/`, `tools/`, `tests/`, [`docs/MVP_TRACEABILITY.md`](docs/MVP_TRACEABILITY.md), [`docs/traceability_v06.md`](docs/traceability_v06.md).
6. [`ROADMAP.md`](ROADMAP.md) / [`ROADMAP.ru.md`](ROADMAP.ru.md) for architectural awareness, **not for immediate implementation**.

The ignored local `docs/history/` directory may exist in some working copies. It is archival and never normative. Where documents conflict, use `docs/DOC_GOVERNANCE.md`; **stop and report unresolved normative conflicts**, do not silently choose whichever is easier to implement.

## 3. Safety and data

- No requests to corporate/production Zabbix without explicit access and scope. **Never write** to productive dashboards as an experiment. P0 write contracts require explicitly permitted test dashboard and account.
- On the explicitly authorized laboratory Zabbix VM, agents may create test dashboards, users, API tokens and new or cloned templates without asking again. Preserve useful test data. Existing templates are read-only: if a change is needed, clone the template first and modify only the clone.
- Never include real IPs, hostnames, passwords, API tokens, raw corporate dashboards, enterprise CA material or secrets in commits, fixtures, logs or screenshots.
- A portable YAML/ZIP is **not guaranteed anonymized**; external sharing still needs human/policy approval.
- A successful Python schema test **does not** prove Zabbix API write safety. Lab contract tests and capability gates must be proven independently.
- Treat imported YAML/ZIP as untrusted data; no arbitrary code execution/network resource retrieval. Do not disable TLS verification.
- `VerifiedPlan` must have a private constructor, be non-deserializable from HTTP and bind server identity/version, source snapshot, permissions and validated capabilities. No direct UI → Zabbix writes.

## 4. Architecture boundaries

`core` independent of Zabbix IDs/version and transport. `template_import` independently parses export formats. `zabbix` handles version-specific API and widget codecs. `application` owns intents, matching, plan and publish. `ui` has no Zabbix JSON-RPC logic. First adapter v7_4; other adapters **planned**. Rust/Axum/Svelte as documented.

The UI domain hierarchy is `Dashboard → Page → Widget`. A local draft is typed as one dashboard or one page (engineering name until O-026: Dashboard/Page project); it is not a Zabbix entity and not a generic wrapper for every workcopy/library object. The UI never says “project”: drafts are labelled by entity (dashboard/page) and origin (local/copy from server), and Russian UI terms follow the official Zabbix localization (панель → страница → виджет, узел сети, элемент данных). Source and target identities are independent. Follow `docs/architecture/domain_model_v1_ru.md`; do not change the legacy multi-dashboard `project/v1` schema or claim typed-project round-trip until O-026 is resolved.

All user-visible text from Rust/Axum, TypeScript and Svelte uses shared localization message IDs and parameters; no user-facing prose is hardcoded in components or returned as a ready sentence by application APIs. Canonical data-only catalogs live under `lang/<locale>/`: English is the source language and Russian is a complete MVP translation. Application logs, diagnostic identifiers and comments in working configuration files are English only. Follow official Zabbix localization terminology where it is established, while preserving untranslated API/field/item-key/macro identifiers. UI styling uses semantic tokens with `system`, light and dark themes; follow the UI/i18n/theme contract. Review mockups are produced in Russian only, in the light theme only, and contain exactly one screen per SVG file; Russian UI terms follow the official Zabbix localization, and English source strings and their length are checked in the message inventory (P1-00F) and P2-00 tests, not in mockups; this does not reduce the runtime requirement to support `system`, light and dark themes. Do not invent interface, status or tray icons: use only the Tabler Icons outline set on all platforms, record upstream icon names, and preserve the MIT notice in `assets/icons/THIRD_PARTY.md` and the in-app Licenses window. Agents are explicitly authorized to download the Tabler outline SVGs needed for the current task from the official `tabler/tabler-icons` upstream without asking again. Download only required icons from an exact release tag or commit, never from an unpinned `latest` URL; keep upstream filenames/names, record the tag or commit, source URLs and included icon names in `assets/icons/THIRD_PARTY.md` or its generated inventory, validate that vendored SVGs contain no scripts or external references, and keep runtime use local/offline and CSP-safe. This authorization does not extend to another icon library or unrelated third-party assets. The in-house application logo and Stezhik artwork are not Tabler icons.

## 5. How to work

- Use the completed **R0-01** matrix in `docs/MVP_TRACEABILITY.md`, then start **P0** from the local `TASKS_RU.md`; follow dependencies and the G0–G6 gates in the README. Parallelize only independent non-write work.
- Before each task: state touched files, expected behavior, checks and risks. After: report actual changed files, commands/tests, real outcomes and limitations.
- Update local task checkboxes **only after acceptance**. Keep `IMPLEMENTATION_LOG.md`. Use `OPEN_DECISIONS.md` to request human approval for changes in schema, security boundaries, public contracts, supported versions or MVP scope.
- Maintain the root bilingual roadmap for post-MVP direction; never quietly change long-term priorities, commit dates, or claim features completed because a document exists.
- Do not implement post-MVP features while doing P0–P6 absent explicit approval. No invented success on unavailable test servers.
- Ensure schema generation is reproducible, and Rust tests eventually cover all positive/negative reference fixtures. Python is the pre-implementation reference, not a runtime dependency.
- Keep translation-only contributions separate from code changes. New experimental locales may receive lighter linguistic review, but never weaker structural, placeholder, security or licensing validation.
- On the owner Windows host, all Rust build output goes under `C:\temp\zdashstitch\` via the ignored `.cargo/config.toml` or an explicit `CARGO_TARGET_DIR`. Remove task-specific scratch/probe output after verification; never build into the checkout or delete the whole shared temp root blindly.

## 6. Local Python reference environment

- Use the repository-local virtual environment at `.venv` for the Python reference validators and tests. It is intentionally ignored by Git.
- On Windows, run tools with `.\.venv\Scripts\python.exe` (activation is optional). To activate in PowerShell: `.\.venv\Scripts\Activate.ps1`.
- The environment contains the current reference-test dependencies: `PyYAML`, `jsonschema`, and `pytest`.
- Recreate it with `python -m venv .venv`, then install the pinned reference dependencies with `.\.venv\Scripts\python.exe -m pip install -r requirements-reference.txt`.
- On Windows, set `$env:PYTHONUTF8='1'` before the reference suite. In sandboxed runs, use a unique temporary path inside the ignored environment to avoid stale ACLs: `$pytestTemp = ".venv/pytest-$PID"; .\.venv\Scripts\python.exe -m pytest -q -p no:cacheprovider --basetemp $pytestTemp`.
- This environment is only for specification/reference checks; it is not a runtime dependency of the planned Rust application.

## 7. Task report format

```text
Task ID:
Status: done | partial | blocked
Changed files:
Behavior actually implemented:
Tests run (commands and actual outcomes):
Lab details (sanitized, exact supported Zabbix version):
Known limitations / unsupported objects:
Decisions requiring approval:
Next unblocked task:
```
