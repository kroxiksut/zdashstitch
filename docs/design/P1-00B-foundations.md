# P1-00B — visual foundations

**Версия:** 1.0, 2026-10-24. **Статус:** принято для P1 handoff на расчётных размерах P1-IN-02. Реальные глобальные панели по O-003 могут уточнить калибровку, но не архитектуру компонентов. Конкретная реализация относится к P2-00.

## 1. Принципы

1. Плотные административные данные важнее декоративной площади, но основной interactive target не меньше 32 px, а предпочтительный обычный control — 36 px.
2. Системные шрифты используются без CDN и внешних ресурсов. Layout выдерживает различия Segoe UI на Windows и распространённых sans-serif на Linux.
3. Увеличение текста до 200% не скрывает действие и не обрезает критическое сообщение; таблица может получить горизонтальный scroll, но не фиксированную высоту строки с clip.
4. Технические IDs/keys используют monospace и безопасный wrap/copy; user/server names используют обычный sans-serif и никогда не нормализуются визуально в идентификаторы.
5. App layout и dashboard grid 72×64 независимы.

## 2. Typography roles

| Role | Size / line-height | Weight | Назначение |
|---|---:|---:|---|
| `display` | 32 / 40 px | 700 | Welcome/empty start; не для рабочих таблиц |
| `page-title` | 24 / 32 px | 700 | Один H1 на route/workspace |
| `section-title` | 18 / 26 px | 650 | Панель или смысловая секция |
| `subheading` | 15 / 22 px | 650 | Card/table group/inspector section |
| `body` | 14 / 21 px | 400 | Основной текст и labels |
| `body-strong` | 14 / 21 px | 600 | Primary values, не status только весом |
| `compact` | 13 / 18 px | 400 | Плотные таблицы, metadata |
| `caption` | 12 / 17 px | 400 | Secondary metadata; не критические инструкции |
| `code` | 13 / 19 px | 400 | IDs, keys, macros, versions, diagnostics |

Font stack sans: `system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`. Font stack mono: `ui-monospace, "Cascadia Mono", "Segoe UI Mono", monospace`. `Inter` не требуется: текущий CSS его не поставляет локально, поэтому фактический runtime и так падает на system UI.

## 3. Spacing, radii, borders, elevation

- Base spacing unit: 4 px; scale: `0, 4, 8, 12, 16, 20, 24, 32, 40, 48, 64`.
- Control inline gap: 8 px; form row gap: 12 px; section gap: 24 px; page padding: 24 px desktop, 16 px compact viewport.
- Radii: 4 px technical cell/inline code, 6 px control, 8 px card/panel, 12 px dialog/drawer. Pill radius only for status/tag content.
- Default border: 1 px semantic border. Selected/focus treatment не изменяет layout; focus ring минимум 2 px + offset.
- Elevation levels: `none`, `raised` (popover/sticky), `overlay` (dialog). Состояние не передаётся одной тенью.

## 4. Controls and density

| Context | Height | Horizontal padding | Notes |
|---|---:|---:|---|
| Standard input/button | 36 px min | 12 px | Default forms/toolbars |
| Compact table control | 32 px min | 8 px | Только плотные data surfaces |
| Prominent action | 40 px min | 16 px | Primary onboarding/confirmation, не каждая toolbar action |
| Icon button | 36×36 px | — | Accessible name + tooltip; 32×32 only in dense grid with equivalent keyboard path |
| Table row | 36 px min | 8–12 px cells | Расширяется при wrap/error/secondary line |
| Tree/list row | 32 px min | 8 px | Indentation кратно 16 px, не цветом одним |

Расчётная калибровка P1-IN-02:

| Набор | Панели/страницы/виджеты | Назначение |
|---|---|---|
| Обычный | 1–3 страницы, 3–10 виджетов на странице | Основные component sheets и рабочие экраны |
| Насыщенный | До 9 страниц, до 20 виджетов на странице | Diff, inspector и diagnostics reference |
| Stress | 20 страниц, 60 виджетов на странице, имена до 128 Unicode scalar values | Проверка wrap, поиска, pagination и virtualization |

Основание — 306 панелей и 440 страниц официальных template dashboards Zabbix 7.4.16 как proxy: медиана 3, p90 9, p99 19, максимум 20 виджетов на страницу; по полям виджета медиана 3, p90 11, p99 21, максимум 33. Это не выдаётся за статистику реальных глобальных панелей.

Правила плотности:

- До насыщенного набора обычные tables/lists рендерятся полностью с pagination только там, где она является частью серверного чтения или пользовательской навигации.
- Virtualization включается для stress-набора, когда один viewport представляет более 200 однородных строк или оценка полного DOM превышает 500 строк. Порог — design default для P2-00, а не API-лимит; реализация обязана сохранить screen-reader list/table semantics, focus и выбранную строку.
- Структурный diff группируется по страницам и виджетам; collapsed группы не создают интерактивные строки в accessibility tree.
- Inspector показывает не более одной раскрытой секции с длинными raw-safe details одновременно; исходное значение всегда доступно copy action.

## 5. Application layout

- Минимальный проектируемый viewport: 1024×720 CSS px при 100%; основной desktop reference: 1440×900. Ни один из них не становится обещанием минимального размера окна до проверки P1-00G/P1-11.
- Global shell: top bar 48–56 px; primary navigation 224–256 px в expanded режиме; workspace заполняет остаток.
- Inspector: 320–400 px resizable, drawer/overlay ниже compact breakpoint; outline/palette не обязаны быть открыты одновременно.
- Dense diff/mapping: main table + resizable detail inspector. Горизонтальный scroll остаётся внутри data region, primary actions остаются видимыми.
- Canvas: workspace использует доступную площадь, page tabs/toolbar находятся вне 72×64 model grid.

## 6. Dashboard grid 72×64

- Координаты и размеры хранятся целыми model units; CSS scale вычисляется из viewport и zoom.
- `zoom-to-fit` показывает все 72×64 units без изменения модели. Pan/zoom не являются undoable domain commands.
- Move/resize становятся command только после валидного commit. Preview может показать collision/out-of-bounds, но не исправляет их автоматически.
- Grid line density адаптируется визуально к zoom; snapping остаётся по unit независимо от видимости каждой линии.
- Keyboard move/resize increment и accessible announcements определяются P1-04/P1-00G.

## 7. Long text and technical values

- Labels могут занимать две строки; buttons не получают ellipsis для критического действия.
- User/server values показываются полностью в details/tooltip или expandable cell; ellipsis не является единственным способом доступа.
- IDs/keys/macros используют `overflow-wrap:anywhere` только в detail blocks; в таблицах — middle-aware presentation с доступным полным copy value.
- EN/RU stress cases должны включать длинный endpoint/base path, dashboard/page names, `key_` с параметрами, макросы, compound diagnostics и plural counts.

## 8. Content-density examples

### Обычный экран

- 56 px app bar, 240 px navigation, один title/action row, одна таблица из 8–12 строк.
- Primary table columns: name, type/status, source, last check/action. Вторичные IDs уходят в inspector, но не скрываются навсегда.
- Один persistent status banner максимум; дополнительные сообщения находятся у поля/строки или в diagnostics.

### Inspector

- 360 px reference width, секции по 12–16 px internal gap.
- Label/value располагаются вертикально, если русская label либо server value не помещается в одну строку.
- Technical values используют `code`, wrap/copy; destructive и publish actions не размещаются в inspector без feature-level confirmation pattern.

### Насыщенный diff

- 9 page groups × 20 widget rows допустимы без virtualization при collapsed-by-default page groups; одновременно раскрытая область ориентирована на 20–60 строк.
- Колонки: change marker, object path, Base, Local, Remote/evidence, result. На 1024 px Base/Local/Remote становятся stacked cell; action и blocking reason остаются видимыми.
- Counts и filters не заменяют полный список изменений. `unknown_outcome`, unsupported и conflict не объединяются в общий красный статус.

## 9. Приёмка и остающееся уточнение

- [x] Агрегаты P1-IN-02 и времена P1-IN-06 учтены; proxy и лабораторные числа не названы продуктивными.
- [x] Определены обычный, насыщенный и stress-наборы, pagination/virtualization и плотность inspector/diff.
- [x] Foundations согласованы с targets/reflow/high-contrast contract P1-00G.
- [ ] O-003: повторно откалибровать thresholds на разрешённых агрегатах реальных глобальных панелей; изменение требует новой версии этого документа только при изменении архитектуры, а не каждого числа.
- [ ] Фактические Windows/Linux fonts, zoom, reflow и high contrast проверяются на реализации в P2-00/P1-11; документ не заявляет runtime-соответствие.
