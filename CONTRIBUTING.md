# Contributing to ZDashstitch

Thank you for considering a contribution. The project is currently in the **specification / pre-implementation stage**. Issue reports, reproducible test cases, corrections to portable schemas and future dashboard recipes are useful.

Please read [`AGENTS.md`](AGENTS.md), the [documentation map](docs/README.md), the [roadmap](ROADMAP.md) and the [architecture decisions](docs/adr/) before making implementation changes. Changes to a public schema or security contract need an explicit design decision, not an incidental code cleanup.

## What to submit

- A clear problem statement with target Zabbix version and dashboard/widget type.
- **Synthetic or explicitly authorized, sanitized fixtures only**; never real tokens, IPs, DNS names, SQL, macros, host mappings or private monitoring configurations.
- A before/after description, affected schemas/ADRs, tests and compatibility impact.
- For widget presets and catalog contributions (post-MVP), document the upstream template dependency, licensing and supported target versions.

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

Unless agreed otherwise before inclusion, a contribution is offered under the repository's existing split: source code and code-like material under **MIT OR Apache-2.0**, and documentation prose, original diagrams and original visuals under **CC-BY-4.0**. Contributors retain their copyright. Do not submit third-party material under incompatible terms or without its provenance and required notices. See [`LICENSING.md`](LICENSING.md).
