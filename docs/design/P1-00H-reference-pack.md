# P1-00H — reference pack и handoff в P2-00

**Версия:** 1.0, 2026-10-24. **Статус:** принято как design-system handoff P1-00. Реализация компонентов, CSS и runtime-каталогов относится к P2-00; этот документ не является снимком работающего приложения.

## 1. Нормативный состав pack

| Part | Source | Что берёт P2-00 |
|---|---|---|
| Audit | [P1-00A](P1-00A-visual-audit.md) | Reuse/gaps, surfaces, offline/CSP/platform limits |
| Foundations | [P1-00B](P1-00B-foundations.md) | Type/spacing/control sizes, app/grid separation, density |
| Tokens/themes | [P1-00C](P1-00C-tokens-and-themes.md) | Semantic aliases, light/dark/system/forced-colors behavior |
| Components | [P1-00D](P1-00D-component-matrix.md) | Anatomy, variants, keyboard models and shared primitives |
| States/feedback | [P1-00E](P1-00E-states-feedback.md) | Component/screen states, transitions, timing and confirmations |
| Messages | [P1-00F](P1-00F-message-inventory.md) | Stable IDs, EN source, RU copy/glossary, parameters and stress set |
| Accessibility/platform | [P1-00G](P1-00G-accessibility-platform.md) | Focus, names, targets, reflow, contrast, reduced motion and test matrix |
| Backend evidence | [P1-IN](backend_inputs_ru.md) | Error/status codes, density proxy, latency, file-dialog constraints |
| Visual sheets | [P1-00H SVG](assets/P1-00H-reference-pack.svg) | Light/dark component sheets and dense table/inspector/diagnostics reference |

При конфликте действуют schema/ADR/architecture contracts по `docs/DOC_GOVERNANCE.md`. Pack не меняет публичный формат, security boundary, supported version или MVP scope.

## 2. Implementation order for P2-00

1. Data-only EN source and complete RU catalogs; generated/checked typed keys and placeholder parity.
2. Theme preference `system | light | dark`, semantic tokens and forced-colors mapping without component color literals.
3. Native primitives: text/link/button/form/status/disclosure/dialog/progress.
4. Composite primitives: tabs, combobox/listbox, tree, data table/grid only where required, split pane and inspector.
5. Shared feedback: banner, field/summary errors, toast/history link, skeleton/progress, technical details.
6. App shell and dense reference implementation; then feature screens consume only shared contracts.

Each layer is testable without Zabbix writes. No UI layer calls raw Zabbix JSON-RPC.

## 3. Component sheets

Версионируемый визуальный источник: [`assets/P1-00H-reference-pack.svg`](assets/P1-00H-reference-pack.svg). SVG использует системные шрифты и не загружает внешние ресурсы; это макет, а не снимок работающего приложения. Этот уже принятый foundation pack сохраняет парные light/dark component sheets. Для новых feature-screen макетов после решения владельца 24.10.2026 действует другое review-правило: только светлая тема и один экран на SVG; runtime-поддержка `system/light/dark` не отменяется.

### Light sheet

Reference surface uses `surface.canvas` → `surface.default` panels, `text.primary/secondary`, `border.default`, neutral primary action and semantic status aliases. Required rows:

| Row | Components/states | Copy/data example |
|---|---|---|
| Actions | primary, secondary, quiet, danger; default/hover/focus/disabled/busy | `publication.action.publish`, `common.action.cancel` |
| Fields | text, URL, password, select, combobox, checkbox/radio/switch; default/focus/error/read-only | Synthetic endpoint; no token value in visual/reference |
| Navigation | link, tabs, breadcrumb, tree row selected/focused | Project → dashboard → page |
| Feedback | info/warning/danger/success banners, badges, progress, skeleton, empty | Offline, partial, conflict, verified |
| Overlay/detail | dialog, non-modal inspector, tooltip, technical-details disclosure | Safe diagnostics only |
| Dense data | table headers/rows, changed/unsupported/unknown-outcome markers, pagination | 20-row page group; Base/Local/Remote |

### Dark sheet

The same DOM/anatomy/messages are rendered through dark semantic mappings. Acceptance is not “colors inverted”: text, boundaries, focus, status pairs and overlay retain contrast; diff meaning keeps labels/icons; chart series colors never substitute UI statuses. No separate dark component API exists.

### Forced-colors sheet

Authored gradients/shadows are removed; native/system colors convey surface, text, link, disabled, button and focus. Icons and text retain status meaning. `forced-color-adjust:none` is forbidden for ordinary controls.

## 4. Dense reference: dashboard change review

This reference intentionally combines the broadest shared surface without claiming a finished P1-08 screen.

```text
┌ App bar: project / saved state / connection / read-only-or-write mode ┐
├ Navigation 240 ┬ Main: Preview and publish ─────────────┬ Inspector 360 ┤
│ Dashboards     │ target + operation + current freshness │ Selection     │
│ Pages          │ [blocking conflict/unknown banner]      │ Base          │
│ Bindings       │ filters / page groups / counts          │ Local         │
│ Checks         │ ┌ structural diff table ──────────────┐ │ Remote        │
│ Publication    │ │ marker · object · Base · Local ... │ │ Evidence      │
│                │ │ 20–60 visible rows in stress view   │ │ Technical     │
│                │ └─────────────────────────────────────┘ │ details       │
├────────────────┴─────────────────────────────────────────┴───────────────┤
│ Persistent action bar: Cancel · Save as copy · Publish verified changes │
└───────────────────────────────────────────────────────────────────────────┘
```

At 1024 px inspector becomes a drawer and navigation collapses; at 320 CSS px equivalent the regions are sequential. The structural table alone may scroll in two dimensions. Primary action remains outside it. The reference uses 9 page groups × 20 rows for saturated review and the 20 × 60 stress fixture only for virtualization testing.

## 5. Reference contracts

| Element | Data inputs | Events | Message IDs | Acceptance checks |
|---|---|---|---|---|
| App status bar | project name, saved/dirty, connection state, access mode | open project/connection context | `common.state.*`, feature labels | Axes remain independent; server data unmodified |
| Navigation/tree | route nodes, object IDs/names, current node, counts | navigate, expand/collapse, select | `library.*`, `dashboards.*`, `editor.*` | Native route order; selected ≠ focus; long names available |
| Data table | stable row ID, columns, sort/page, row state | sort, page, select, open details | namespace column/status IDs | Header association; no color-only diff; stress virtualization preserves focus |
| Inspector/drawer | selected object, editable/read-only fields, evidence | edit, copy, close, resize | `editor.inspector.title`, `bindings.evidence.*`, `diagnostics.*` | Non-modal no trap; narrow layout becomes labelled drawer |
| Status banner | code, severity, safe params, diagnostic ID | primary recovery, details | `errors.*`, `publication.*`, `common.action.open_details` | One primary blocking message; no raw OS/API prose |
| Progress | stage, completed/total, cancellable | cancel | `common.state.loading`, feature progress IDs | Delay thresholds from E; static/reduced-motion understandable |
| Confirmation dialog | operation, exact target, change count, risks | confirm once, cancel | `publication.confirm.*`, concrete action ID | Focus trap/restore; modified preview invalidates confirmation |
| Technical details | allowlisted code/stage/status/version/method | disclose, copy safe details | `diagnostics.*` | No secrets, raw bodies or full paths |
| File-dialog launcher | availability, intent, display name, opaque handle | launch, cancel, fallback | `common.action.choose_*`, error/state IDs | One dialog; timeout/cancel/fallback; conditional on O-023 |
| Theme/language controls | enum preference/locale | change preference | `settings.theme.*`, `settings.language.title` | No loss of focus/form/selection; `system` follows OS |

## 6. EN/RU long-text reference

The dense reference is rendered with both locale sets from P1-00F. Mandatory cases:

- 128-character dashboard/page name and long endpoint/key/macro;
- compound conflict sentence and safe TLS/LDAP guidance;
- plural counts `0/1/2/5/21/101`;
- 200% zoom and system fonts on Windows/Linux;
- labels wrap to two lines; critical action labels are not ellipsis-only; technical values remain copyable.

EN and RU use the same structure and actions. Layout does not reserve fixed pixel width based on English. Translation may not weaken “do not retry”, “private”, “read-only”, “not anonymized” or any write blocker.

## 7. State sheet

Every interactive row is checked in `default/hover/pressed/focus/selected/disabled/read_only/busy/error/warning/success`. Every reference screen is checked in `loading/empty/partial/denied/unsupported/offline/stale/conflict/unknown_outcome` as applicable.

Minimum paired examples:

- `disabled`: write unavailable because access mode is read-only; separate action explains how to request `read_write`.
- `read_only`: unknown widget remains inspectable and copyable.
- `partial`: imported template dashboards listed as unsupported.
- `conflict`: Base/Local/Remote shown; old plan cannot publish.
- `unknown_outcome`: publish action absent; reconciliation is primary.
- `success`: read-back verified and visually-unverified note remains.

## 8. Cross-feature rules for P1-01—P1-10

- Features may compose documented components but may not introduce local palette, hardcoded displayed prose or a new state synonymous with an existing state.
- Features may not draw or import another icon style. Interface/status/tray slots name an icon from Tabler outline; the accessible message ID remains independent. Only the in-house logo/Stezhik may come from `assets/brand/`.
- New backend codes require mapping to a stable message ID and safe parameter allowlist before a mockup can be accepted.
- Feature-specific patterns must identify shared primitives and document only their composition/domain guards.
- Offline preview never imitates real metric values. `read_only` never exposes a hidden server-write action. Unknown/read-only objects remain lossless where contracts require it.
- All publish paths end in read-back verified, known failure/partial failure, or unknown outcome; none infer success from HTTP completion.

## 9. Open questions and explicit limitations

| Item | Status | Effect on pack |
|---|---|---|
| O-003 real global-dashboard aggregates | Decided 2026-10-24; optional calibration | ReplaceHost edits `item`, `svggraph`, classic `graph`; other widgets are listed for confirmation. Real aggregates may recalibrate density thresholds only |
| O-023 native file dialog | Decided 2026-10-24 | Native dialog launcher is the primary path; upload/download fallback is always represented; manual checks in P2-07E |
| O-011 private transfer container | Open until P5-06 | Shown only as unavailable; no invented fields/actions |
| O-012 StyleProfile/WidgetBlueprint scope | Open until P4-05 | Generic library/anatomy exists; exact editor/schema not invented |
| O-014 automatic merge | Open/post-basic conflict | Pack specifies block + re-import/Save as copy; no auto-merge |
| Browser/AT/distro versions | P1-11/P6 | Rules are acceptance criteria, not claims of tested runtime support |

## 10. P2-00 acceptance handoff

- [ ] No displayed literal exists outside catalogs; EN/RU key and placeholder parity is mechanically checked.
- [ ] Components use semantic tokens only and render light/dark/system/forced-colors.
- [ ] Component and screen state sheets pass keyboard, focus, reduced-motion, 200% zoom and 320 CSS px reflow checks.
- [ ] Dense reference renders saturated and stress fixtures without losing full labels, focus or screen-reader structure.
- [ ] Technical details and support output contain only allowlisted safe fields.
- [ ] Theme/locale switches preserve route, selection, focus, form and unsaved local state.
- [ ] No runtime asset, request or font depends on an external network.
- [ ] P1-11 records actual OS/browser/AT observations; this pack alone does not close runtime accessibility.

## 11. P1-00 acceptance

- [x] Foundations, tokens, components, states, messages and accessibility rules form one versioned handoff.
- [x] Component sheets cover light/dark/forced-colors with identical semantics.
- [x] EN/RU long-text and one dense table/inspector/diagnostics reference are specified.
- [x] Data inputs, events, message IDs and acceptance checks are listed.
- [x] Open questions are explicit and do not silently expand MVP.
- [x] No Svelte component, runtime catalog, production CSS or new robot image was created by P1-00.
