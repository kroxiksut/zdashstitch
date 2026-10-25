# P1-00A — аудит визуальной основы и ограничений

**Версия:** 1.0, 2026-10-24. **Статус:** принято для P1 handoff; это design input, а не снимок реализованного приложения и не разрешение начинать пользовательские экраны до G1a/G1b.

## 1. Область аудита

Проверены текущий Svelte-каркас в `ui/`, loopback shell в `apps/local_server/`, опубликованные материалы `assets/`, брендовые заметки, продуктовые сценарии UC-01—UC-10, P1-00—P1-10 и контракты UI/i18n/theme, файлов и Zabbix-адаптера. Исторический `docs/history/` не использовался.

## 2. Что можно переиспользовать

| Источник | Переиспользуемое решение | Граница переиспользования |
|---|---|---|
| `assets/brand/*` | Узнаваемая палитра: глубокий navy/teal, яркий orange, холодные светлые поверхности; образ Стежика; мотив соединения и преобразования карточек | Только visual direction. Нарисованные панели, подписи, пиктограммы, сетки и позиции не являются UI-контрактом |
| Tabler Icons outline | Единый источник interface/status/tray icons для Windows и Linux | Third-party MIT; не относится к логотипу/Стежику, требует upstream name и notice `assets/icons/THIRD_PARTY.md`; фактический пакет выбирается в P2-00 |
| `ui/src/App.svelte` | Честные состояния `loading → health/error`, семантические `main`, `section`, `dl`, `role=alert`, `aria-live` | Компоненты, навигация и сценарии редактора отсутствуют; hardcoded copy подлежит замене message IDs в P2-00 |
| `ui/src/styles.css` | System-font fallback и компактный single-surface smoke layout | Цветовые литералы, один тёмный режим и декоративный градиент не являются системой tokens/themes |
| `ui/src/lib/api.ts` | UI обращается только к same-origin `/api/health`, а не к Zabbix | Тип ответа минимален; он не задаёт контракты экранов подключения, импорта или публикации |
| `apps/local_server` | Loopback, случайный порт и отсутствие mutating routes в scaffold | Session bootstrap, Origin/Host, CSRF, DNS-rebinding и CSP ещё не реализованы; UI нельзя проектировать так, будто они уже есть |
| `docs/architecture/ui_i18n_theme_contract_v1_ru.md` | Единая граница message IDs, `en` source + полный `ru`, темы `system/light/dark`, semantic tokens | Runtime-каталоги, typed bindings и CSS реализуются позже в P2-00 |

## 3. Текущий инвентарь UI

### Поверхности и данные

- Одна центрированная карточка состояния локального приложения.
- Статические название продукта, описание scaffold и eyebrow.
- Четыре read-only значения health: backend, стадия, целевая ветка, технически разрешённая запись.
- Нет shell приложения, маршрутов, toolbar/sidebar, таблиц, canvas, inspector, dialog/drawer, file picker, connection form, preview/diff или publish flow.

### Контролы и состояния

- Пользовательских интерактивных контролов нет.
- Реализованы только `loading`, `success/read-only status` и `error` health-check.
- Нет empty/partial/denied/unsupported/offline/stale/conflict/unknown-outcome, focus/hover/pressed/disabled/busy или recovery flows.
- Нет сохранённого выбора языка/темы, keyboard model, reduced-motion и high-contrast поведения.

### Текст и локализация

- Пользовательские строки сейчас захардкожены в `App.svelte` и `index.html`; это допустимый долг scaffold, но прямой gap к принятому контракту.
- Русский и английский смешаны (`Backend`, `Zabbix dashboard builder`, русские подписи); locale не выбирается.
- Сырые `status` и `product_stage` выводятся напрямую, хотя будущий UI обязан отображать их через message IDs.
- Технические идентификаторы, которые нельзя переводить: `ZDashstitch`, `Zabbix`, API-методы (`dashboard.get`, `dashboard.update`), типы виджетов (`item`, `svggraph`, `graph`), поля/API keys (`hostid`, `itemid`, `graphid`, `dashboardid`, `widgetid`, `key_`), item keys, macros, версии и diagnostic IDs.

## 4. Обязательные поверхности MVP

| Поверхность | Windows/Linux | Браузерная граница | Критический риск |
|---|---|---|---|
| Launcher/bootstrap локальной сессии | Открытие системного браузера, повторный запуск, один экземпляр | Новый session secret после рестарта; прямой URL без bootstrap не даёт сессию | Нельзя считать loopback доверенным только по адресу |
| App shell и локальные черновики | Min viewport, zoom/text scaling, системные шрифты | Несохранённый ввод вкладки теряется при закрытии | Смешение локального dirty-state и серверной публикации |
| Connections | Token/login, TLS/CA diagnostics, `read_only`/`read_write` | Секрет кратковременно проходит через JS, но не storage/URL/log | Нельзя показывать raw server error как основное сообщение |
| Import/library | File/folder selection, drag/drop, progress | Браузер не раскрывает обычный filesystem path; решение зависит от P1-IN-09 | Traversal/symlink/reparse, объёмы и partial result |
| Dashboard browser | Таблицы, поиск, owner/visibility, supported state | Только безопасные DTO/message params | Объёмы и виртуализация зависят от P1-IN-02/06/08 |
| Editor | Page tabs, palette/library, 72×64 canvas, outline, inspector, diagnostics | Canvas остаётся целочисленной моделью Zabbix, не CSS-pixel моделью | App layout grid нельзя смешивать с dashboard grid |
| Mapping/Clone | Dependency inventory, target picker, mapping table, cardinality | Unknown/raw данные остаются видимыми, но не становятся editable | Похожее имя не является доказательством match |
| Preview/diff/publication | Base/Local/Remote, blockers, confirmation, progress, read-back | UI получает opaque plan ID, а не payload | Stale/conflict/unknown outcome нельзя маскировать успехом |
| Settings | Locale, `system/light/dark`, storage root, CA | Настройки не ослабляют security confirmations | Platform path disclosure и invalidation context |

## 5. Две независимые сетки

- **App layout grid** размещает shell, панели, gutters и responsive breakpoints в CSS pixels/rem. Она может перестраиваться при узком viewport и zoom.
- **Dashboard grid 72×64** представляет координаты и размеры Zabbix как целые model units. Zoom/pan меняют только отображение; они не округляют и не переписывают `x/y/width/height` скрыто.
- Inspector и status overlays не занимают dashboard cells. Изменение ширины боковой панели не изменяет модель виджетов.
- Перетаскивание использует явный snap к model units; любое недопустимое размещение остаётся диагностикой, а не автоматически создаёт страницу или меняет размер.

## 6. Gap list

| Приоритет | Gap | Зависимость / адресат |
|---|---|---|
| Blocker | Нет session/bootstrap/Origin/Host/CSRF/DNS-rebinding/CSP | P2-SEC-01; до него нет mutating HTTP routes |
| Blocker | Нет message catalogs и typed IDs; scaffold hardcoded | P1-00F → P2-00 |
| Blocker | Нет модели app/session/publish состояний | P1-01C/G, P1-IN-07 |
| Blocker | Нет подтверждённых объёмов таблиц и длин имён | P1-IN-02/06/08; блокирует приёмку density/virtualization P1-00B |
| High | Нет component/state/accessibility matrix | P1-00D/E/G |
| High | Нет file/folder UX решения | P1-IN-09 → O-decision до соответствующих экранов |
| High | Нет светлой темы и high-contrast mapping | P1-00C → P2-00 |
| High | Icon source не был закреплён в scaffold | Решено O-024: только Tabler outline; inventory/package integration → P2-00 |
| High | Нет отображения capability/permission/match как независимых осей | P1-IN-03/04 и P1-05/P1-08 |
| Medium | Mascot scenes не сопоставлены с разрешёнными empty/help/success contexts | P1-00H; декоративность не должна скрывать diagnostics |

## 7. Риски визуального направления

- Orange glow хорошо работает как брендовый accent, но не может одновременно означать focus, warning и destructive action.
- Светлые glass-like карточки из concept art теряют границы в high-contrast и не должны задавать production elevation.
- Крупный Стежик полезен для welcome/empty/help, но мешает плотным mapping/diff/editor поверхностям; там используется знак или малый contextual asset.
- Нарисованные графики могут быть приняты за live telemetry. Offline/editor screens обязаны явно обозначать preview/placeholder и не имитировать реальные значения.
- Текущий dark-only scaffold нельзя использовать как основание для token mapping. После решения владельца 24.10.2026 новые feature-screen SVG выполняются только в light и по одному экрану на файл; dark/system/high-contrast остаются обязательными для token contract и runtime QA, но не требуют отдельных SVG-макетов.

## 8. Выход P1-00A

Reuse ограничен брендом, семантической HTML-заготовкой health-state и безопасной same-origin границей. Для продуктового UI требуются новая design-system спецификация, message inventory, state model и security-aware flows. Новых функций этим аудитом не добавлено; готовность редактора или публикации не заявляется.
