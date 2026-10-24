# ZDashstitch documentation

[Русская версия](README.ru.md) · [Project README](../README.md) · [Roadmap](../ROADMAP.md)

This directory contains product requirements, accepted decisions, architecture contracts and traceability. The project is still at the specification/pre-implementation stage; older revisions remain in Git history or an optional ignored local archive.

## Start here

| Need | Read |
|---|---|
| Understand the product and MVP | [`../README.md`](../README.md), [`product/PROJECT_GOALS.md`](product/PROJECT_GOALS.md), [`product/USE_CASES.md`](product/USE_CASES.md) |
| Understand future directions | [`../ROADMAP.md`](../ROADMAP.md) |
| Implement safely | [`../AGENTS.md`](../AGENTS.md), [`DOC_GOVERNANCE.md`](DOC_GOVERNANCE.md), ADRs and architecture contracts below |
| Trace MVP goals to implementation gates | [`MVP_TRACEABILITY.md`](MVP_TRACEABILITY.md) |
| Understand current unresolved gaps | [`traceability_v06.md`](traceability_v06.md), [`../OPEN_DECISIONS.md`](../OPEN_DECISIONS.md) |
| Validate portable formats | [`../schemas/`](../schemas/), [`architecture/format_contract_v1_ru.md`](architecture/format_contract_v1_ru.md), [`../tests/`](../tests/) |

The maintainer's current execution queue is local and intentionally ignored as `TASKS_RU.md`; public documents define product boundaries and evidence, not live checkbox state.

## Current normative documents

### Product

- [`product/PROJECT_GOALS.md`](product/PROJECT_GOALS.md) — problem, users, success criteria and boundaries.
- [`product/USE_CASES.md`](product/USE_CASES.md) — end-to-end acceptance scenarios.
- [`product/BRANDING_NOTES.md`](product/BRANDING_NOTES.md) — intended visual identity and its non-functional status.

### Architecture

- [`architecture/ARCHITECTURE_MAP.md`](architecture/ARCHITECTURE_MAP.md) — component boundaries and data flow.
- [`architecture/format_contract_v1_ru.md`](architecture/format_contract_v1_ru.md) — portable/local data contracts.
- [`architecture/adapter_contract_v1_ru.md`](architecture/adapter_contract_v1_ru.md) — versioned Zabbix adapter and safe publishing boundary.

### Decisions

- [`adr/ADR-001-005.md`](adr/ADR-001-005.md) — product shape, local GUI and first architecture decisions.
- [`adr/ADR-006-010.md`](adr/ADR-006-010.md) — safety, operations and laboratory contracts.
- [`adr/ADR-011.md`](adr/ADR-011.md) — multi-version adapter boundary.
- [`adr/ADR-012.md`](adr/ADR-012.md) — v0.6 format, package and `VerifiedPlan` corrections.
- [`adr/ADR-013.md`](adr/ADR-013.md) — code/documentation licensing split.

### Governance and evidence

- [`DOC_GOVERNANCE.md`](DOC_GOVERNANCE.md) — document authority and status rules.
- [`MVP_TRACEABILITY.md`](MVP_TRACEABILITY.md) — R0 mapping from product use cases to P0–P5 tasks, G0–G5 gates and acceptance evidence.
- [`traceability_v06.md`](traceability_v06.md) — unresolved findings and coverage matrix.
- [`../IMPLEMENTATION_LOG.md`](../IMPLEMENTATION_LOG.md) — factual completed work and evidence.
- [`../OPEN_DECISIONS.md`](../OPEN_DECISIONS.md) — decisions requiring owner or laboratory confirmation.

## Language status

The public README, roadmap and this documentation map are bilingual. Most detailed product, ADR and architecture documents currently exist in Russian only; their filenames are kept stable until an approved English translation exists. JSON Schemas, synthetic data and source code are language-neutral where practical.

## Authority when documents disagree

1. Verified behavior of an explicitly permitted test Zabbix environment.
2. Generated schemas/invariants and accepted ADRs, with later explicit ADR corrections taking precedence.
3. Architecture contracts.
4. MVP boundaries in the root README.
5. Post-MVP direction in the root roadmap.
6. Historical material.

Unresolved normative conflicts must be recorded in [`../OPEN_DECISIONS.md`](../OPEN_DECISIONS.md); do not silently choose the easiest document.
