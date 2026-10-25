# Contributing to ZDashstitch

Thank you for considering a contribution. The project is currently in the **specification / pre-implementation stage**. Issue reports, reproducible test cases, corrections to portable schemas and future dashboard recipes are useful.

Please read [`AGENTS.md`](AGENTS.md), the [documentation map](docs/README.md), the [roadmap](ROADMAP.md) and the [architecture decisions](docs/adr/) before making implementation changes. Changes to a public schema or security contract need an explicit design decision, not an incidental code cleanup.

## What to submit

- A clear problem statement with target Zabbix version and dashboard/widget type.
- **Synthetic or explicitly authorized, sanitized fixtures only**; never real tokens, IPs, DNS names, SQL, macros, host mappings or private monitoring configurations.
- A before/after description, affected schemas/ADRs, tests and compatibility impact.
- For widget presets and catalog contributions (post-MVP), document the upstream template dependency, licensing and supported target versions.

## Translation contributions

Runtime translations are welcome as a separate kind of contribution. Submit translation-only pull requests that change only data-only locale files under `lang/<locale>/`; do not mix Rust, TypeScript, Svelte, build-script or public-contract changes into a translation pull request. If code support is required for a language, propose it separately.

English is the source catalog and Russian is a complete MVP translation. A new language may initially be accepted as experimental with English fallback and lighter linguistic review. Structural and safety checks remain mandatory for every locale: keys and placeholders must match the English source, catalogs must contain no executable code or HTML, warnings must keep their meaning, and provenance/licensing must be clear.

Use the established terminology of the target Zabbix localization where it exists. Keep Zabbix/API identifiers, item keys, macros, field names and terms left untranslated by Zabbix unchanged. Do not copy complete upstream translation catalogs without compatible licensing and provenance. See the [UI, localization and theme contract](docs/architecture/ui_i18n_theme_contract_v1_ru.md).

Application logs and comments in working configuration files are English only. Localized comments may be contributed as separate example configurations such as `config.example.en.*` and `config.example.ru.*`; keys and behavior must remain identical.

## Quality checks for current specification

```bash
python -m pip install -r requirements-reference.txt
python tools/build_schemas.py
python tools/validate.py examples/db-pilot.yaml
python tools/validate.py --package examples/db-pilot-package.zip
python -m pytest -q
```

These are **reference checks only**. Working against a live Zabbix server requires separate contractual testing and permission. For vulnerability reports, see [`SECURITY.md`](SECURITY.md).

## Contribution licensing

Unless agreed otherwise before inclusion, a contribution is offered under the repository's existing split: source code and code-like material under **MIT OR Apache-2.0**, and documentation prose, original diagrams and original visuals under **CC-BY-4.0**. Contributors retain their copyright. Do not submit third-party material under incompatible terms or without its provenance and required notices. Interface, status and tray icons must be selected from the Tabler Icons outline set rather than drawn ad hoc; record the upstream icon name and keep [`assets/icons/THIRD_PARTY.md`](assets/icons/THIRD_PARTY.md) current. The project logo and Stezhik artwork are separate in-house assets. See [`LICENSING.md`](LICENSING.md).
