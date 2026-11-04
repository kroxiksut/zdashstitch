# P1-00D — инвентарь и анатомия компонентов

**Версия:** 1.1, 2026-10-26 (DESIGN-FIX-01: без слова «project», m-02); 1.0 — 2026-10-24. **Статус:** принято для P1 handoff. Компоненты описывают общий UI-язык; feature-specific composition остаётся в P1-01—P1-10. Runtime-компоненты создаются в P2-00.

## 1. Общие правила

1. Native HTML semantics имеют приоритет; ARIA не заменяет корректный element/label.
2. Один общий компонент обслуживает одинаковую anatomy и keyboard model. Feature pattern может компоновать общие элементы, но не вводит локальные цвета, focus или собственный `busy`.
3. Любой visible text и accessible name задаётся message ID. User/server data передаются параметрами и не переводятся.
4. `disabled` означает действие невозможно и недоступно focus; `read-only` остаётся focusable/copyable и объясняет причину. Эти состояния не взаимозаменяемы.
5. Tooltip содержит только дополнительную краткую подсказку; критическая причина всегда видима рядом либо доступна через technical-details block.
6. File/directory selection проектируется как launcher нативного диалога с browser upload/download fallback. Встроенного path browser в MVP нет. До O-023 это условный pattern, а не утверждённая dependency.
7. Interface/status/tray icons не рисуются внутри feature или общего компонента. Используется только Tabler Icons outline; component contract хранит upstream icon name и отдельный локализуемый accessible label. Логотип ZDashstitch/Стежик — собственный брендовый asset, не часть icon set.

## 2. Component matrix

| Компонент | Anatomy и variants | Допустимый контент | Keyboard model | Общая граница / feature pattern |
|---|---|---|---|---|
| Button | label, optional leading icon, busy indicator; primary/secondary/quiet/danger | Короткое действие; без status-only текста | `Enter`/`Space`; busy остаётся focusable, повторная активация блокируется | Общий; publish/replace confirmation — feature pattern |
| Icon button | icon, accessible label, tooltip; standard/quiet | Одно однозначное действие | Как button; target 36×36, dense 32×32 | Общий; не создавать icon-only для редких/опасных действий |
| Link | label, optional external indicator | Навигация или open-in-Zabbix; не command | `Enter`; стандартный browser context | Общий |
| Text/password/URL input | label, field, hint, status/error, optional reveal/copy | Одна строка; URL не нормализуется без preview | Native text editing; `Tab`; reveal — отдельная button | Общий; endpoint/auth group — feature pattern |
| Select | label, native select, hint/error | До ~12 стабильных options | Native select | Общий |
| Combobox | label, input, popup list, status/count, clear | Большие host/item/template sets; server values unmodified | APG combobox/listbox; arrows/Enter/Escape | Общий; resolver evidence row — feature pattern |
| Checkbox | input, label, optional description | Независимый boolean | `Space` | Общий |
| Radio group | legend, radio options, descriptions | Взаимоисключающий небольшой набор | APG radio: arrows, `Space` | Общий; access mode/export mode — feature pattern |
| Switch | checkbox semantics, visible on/off label | Немедленная обратимая setting; не submit/permission | `Space` | Общий; не использовать для read/write elevation |
| Tabs | tablist, tab, tabpanel, optional count | Peer views одного context; не route hierarchy | APG tabs; arrows/Home/End, activation documented | Общий; dashboard pages — feature pattern с overflow |
| Tree/list | labelled container, rows, disclosure, selection/status | Dashboard → page → widget outline of a draft, library and page lists; server names | APG tree/listbox; arrows, Home/End, typeahead if stable | Общий; canvas outline — feature pattern |
| Table/data-grid | caption/label, headers, rows, sort/filter/pagination, selection | Сравнимые records; technical columns; expandable details | Native table for read-only; APG grid only if cell interaction needed | Общий; diff/resolver mapping — feature patterns |
| Badge/status | icon/shape, localized label, optional count | Краткий state, не действие | Не focusable без action | Общий; exact/read-only/conflict mappings documented in feature |
| Tooltip | trigger relation, plain text popup | Дополнительная подсказка, полный truncated label | Hover/focus/touch; `Escape` dismiss | Общий |
| Banner/alert | icon, heading, body, actions/details | Route/section-wide actionable state | DOM order; blocking alert announced once | Общий; security/conflict compositions feature-specific |
| Toast | concise status, optional safe action | Неблокирующий ephemeral result; не единственное доказательство | Не получает focus автоматически; pause/dismiss available | Общий; durable result duplicated in history/diagnostics |
| Dialog | title, description, body, actions, close; modal | Короткое blocking decision/confirmation | APG modal; focus trap, `Escape` if cancellable, restore opener | Общий; irreversible confirmation feature pattern |
| Drawer/inspector | labelled header, sections, close/resize, actions | Contextual details/editing | Non-modal: no trap; predictable DOM order | Общий shell; widget/diagnostic inspector feature-specific |
| Progress | label, stage/count, bar/spinner, cancel where safe | Determinate count or indeterminate stage | Status semantics; cancel is button | Общий; import/publish sequence feature-specific |
| Skeleton | inert shapes + one loading label | Initial layout only after delayed threshold | Removed from a11y tree; one status | Общий; never for write outcome |
| Empty state | heading, reason, next action, optional licensed existing art | Truly empty filtered/unfiltered state | Normal focus order | Общий; content/actions feature-specific |
| Pagination | current/total, previous/next, page selector when useful | Stable result set | Native buttons/links; current page announced | Общий; virtualization may replace DOM density, not navigation semantics |
| Breadcrumb | nav label, ordered links, current item | Route/object hierarchy | Native links | Общий; never duplicates dashboard page tabs |
| Split pane | primary/detail regions, separator, collapse controls | Table+inspector, outline+canvas | Separator keyboard resize; panels usable sequentially | Общий layout primitive |
| Technical details | summary, sanitized key/value list, diagnostic ID, copy, disclosure | Codes, stages, HTTP status, safe parameters; no secrets/raw bodies | Disclosure button, selectable/copyable values | Общий safe block; allowed fields supplied by application contract |
| Drop zone | labelled upload input + browse action; visual drag state | Local import files only, validated after selection | Native file input and button are primary paths | Conditional common pattern; not directory/path browser |
| File-dialog launcher | label, selected display name/opaque handle, launch/cancel/fallback | File/folder/save intent; never exposes full path to browser | Button opens native dialog; focus/state retained; one dialog only | Conditional feature pattern pending O-023 |

## 3. Tables, trees and virtualization

- P1-IN-02 proxy distribution fits native table/list composition through the saturated case. Virtualization is required only for the P1-00B stress set.
- A virtualized table preserves header associations, stable row identity, selection and focus when rows leave/re-enter the viewport. Screen reader mode may use paged segments instead of pretending the complete set is in DOM.
- Tree hierarchy represents real parent/child structure only. Filtering does not silently reparent matches; hidden ancestors are disclosed as context.
- Read-only structural diff is a semantic table. Editable resolver mapping is a grid only if cell-by-cell keyboard interaction is actually implemented; otherwise each row contains ordinary controls.

## 4. Safe technical-details allowlist

Primary user message uses `errors.*` and safe parameters. Collapsed technical details may show:

- stable diagnostic ID, operation stage (`dns`, `connect`, `tls`, `http`, `jsonrpc`, `auth`, `permissions`, `read_back`);
- normalized error code, HTTP status, JSON-RPC numeric code and API method when safe;
- application/server version, adapter/capability ID and timestamp;
- sanitized object counts and opaque local operation ID.

Never show token/password/session/CSRF/bootstrap values, raw request/response bodies, Authorization headers, full local paths, unrestricted endpoint query/fragment, user/server free text copied into logs, or OS-localized error prose as the primary explanation.

## 5. File selection (O-023 decided)

The reference pattern contains one `Choose…`/`Save as…` launcher, state `dialog_open`, cancel and browser fallback. It must support `dialog_unavailable`, `dialog_busy`, `dialog_timeout`, `user_cancelled` and successful opaque selection. The browser never receives a full local path. O-023 was decided on 2026-10-24: the native dialog is the primary path and browser upload/download is the always-available fallback (ADR-012 D-12). Acceptance of the native implementation waits for manual Windows/Linux dialog checks in P2-07E.

## 6. Acceptance checks

- [x] Every required P1-00D component has anatomy, content, keyboard model and ownership boundary.
- [x] Common controls cover file fallback, dense data, inspector, diagnostics and safe confirmations without unique one-off controls.
- [x] P1-IN-01 safe details, P1-IN-02 density and P1-IN-09 conditional file selection are reflected.
- [ ] Runtime semantics, browser behavior and virtualization performance are verified in P2-00/P1-11.
- [x] Native file-dialog pattern is the primary path after O-023 (2026-10-24); manual dialog checks are part of P2-07E.
