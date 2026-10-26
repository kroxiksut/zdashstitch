# P1-00C — semantic tokens и темы

**Версия:** 1.0, 2026-10-24. **Статус:** принято для P1 handoff; это semantic map для handoff. Runtime JSON/CSS, concrete component bindings и persistence реализуются в P2-00.

## 1. Правила модели

- Компоненты используют только semantic aliases, не palette names и не hex literals.
- `success`, `info`, `warning`, `danger` имеют отдельные text/surface/border/icon tokens; ни один смысл не передаётся только hue.
- `focus` не совпадает с warning-orange бренда. Focus остаётся отчётливым во всех semantic states.
- Brand accent применяется к identity/primary action умеренно; mascot glow не является системным state color.
- `system` выбирает light/dark через `prefers-color-scheme`; пользовательское `light`/`dark` имеет приоритет. Переключение темы не меняет route, selection, dirty state, form data или publish lifecycle.

## 2. Token taxonomy

| Группа | Tokens |
|---|---|
| Surfaces | `surface.canvas`, `surface.base`, `surface.raised`, `surface.sunken`, `surface.overlay`, `surface.selected`, `surface.disabled` |
| Text | `text.primary`, `text.secondary`, `text.muted`, `text.inverse`, `text.link`, `text.disabled`, `text.code` |
| Borders | `border.subtle`, `border.default`, `border.strong`, `border.selected`, `border.disabled` |
| Actions | `action.primary.bg/text/hover/pressed`, `action.secondary.*`, `action.ghost.*`, `action.destructive.*` |
| Focus | `focus.ring`, `focus.offset` |
| Status | `status.{success,info,warning,danger}.{surface,text,border,icon}` |
| Overlay | `overlay.scrim`, `overlay.backdrop` |
| Shadows | `shadow.raised`, `shadow.overlay` |
| Data/editor | `grid.line`, `grid.major`, `grid.bounds`, `selection.fill`, `selection.border`, `diff.added`, `diff.changed`, `diff.removed`, `diff.unknown` |

`diff.*` всегда сопровождается знаком/текстом и не подменяет status tokens: изменение не обязательно success, удаление не обязательно error.

## 3. Theme mapping (design reference)

Значения ниже предназначены для проверки контраста и visual reference. P2-00 переносит их в канонический token source; компоненты не копируют значения напрямую.

**Палитра обновлена 25.10.2026 (P1-11H, решение D1):** основное действие, ссылки, выделение и информационный статус переведены в синюю гамму принятых проверочных макетов; успех, внимание и ошибка — в цвета макетов. Все пары проверены на WCAG AA тестом `tests/test_ui_tokens.py`; реализация — `ui/src/tokens.css`.

| Semantic token | Light | Dark |
|---|---|---|
| `surface.canvas` | `#F5F7FB` | `#0C1218` |
| `surface.base` | `#FFFFFF` | `#121B24` |
| `surface.raised` | `#FFFFFF` | `#182430` |
| `surface.sunken` | `#EAF0F5` | `#0A1016` |
| `surface.selected` | `#EEF4FF` | `#1B2B4A` |
| `text.primary` | `#17213A` | `#F2F6F8` |
| `text.secondary` | `#475467` | `#BBC8D1` |
| `text.muted` | `#667085` | `#93A6B4` |
| `text.link` | `#3156A3` | `#9DBBF5` |
| `border.default` | `#7A8D9B` | `#647682` |
| `border.strong` | `#526675` | `#91A3AF` |
| `action.primary.bg` | `#3156A3` | `#8FB0F0` |
| `action.primary.text` | `#FFFFFF` | `#0B1630` |
| `action.primary.hover` | `#284A8F` | `#A6C1F4` |
| `action.primary.pressed` | `#1F3D7A` | `#BCD1F7` |
| `focus.ring` | `#6B35D4` | `#C2A7FF` |
| `focus.offset` | `#FFFFFF` | `#0C1218` |
| `status.success.surface` | `#ECFDF3` | `#123526` |
| `status.success.text` | `#1F6B43` | `#8BE0AE` |
| `status.info.surface` | `#EEF4FF` | `#17284A` |
| `status.info.text` | `#1D3F86` | `#A9C3F7` |
| `status.warning.surface` | `#FFF4E5` | `#3B2A0D` |
| `status.warning.text` | `#8A4B00` | `#FFD27A` |
| `status.danger.surface` | `#FEF3F2` | `#401C20` |
| `status.danger.text` | `#A32935` | `#FFADB4` |
| `overlay.scrim` | `rgb(9 20 29 / 48%)` | `rgb(0 0 0 / 68%)` |
| `grid.line` | `#D8E1E8` | `#293844` |
| `grid.major` | `#B7C6D1` | `#425462` |
| `grid.bounds` | `#526675` | `#91A3AF` |

Интерактивные пары hover/pressed/disabled и все status border/icon aliases должны пройти автоматический contrast audit в P2-00; таблица не является заменой browser render checks.

Контрольный расчёт WCAG relative luminance для основных пар на 2026-10-24:

| Pair | Contrast |
|---|---:|
| Light primary / base | 16.00:1 |
| Light secondary / base | 7.64:1 |
| Light link / base | 6.98:1 |
| Light primary-action text / background | 5.98:1 |
| Dark primary / base | 15.99:1 |
| Dark secondary / base | 10.18:1 |
| Dark link / base | 9.22:1 |
| Dark primary-action text / background | 7.38:1 |
| Light/Dark default border / base | 3.44:1 / 3.69:1 |
| Light/Dark warning text / surface | 7.24:1 / 9.69:1 |
| Light/Dark danger text / surface | 7.64:1 / 8.46:1 |

Это проверка только перечисленных foreground/background пар. Она не доказывает contrast non-text controls, focus boundaries, hover/pressed и rendered antialiasing.

## 4. High-contrast mapping

В forced-colors режиме декоративные gradients, background images и shadows отключаются. Основные aliases сопоставляются системным цветам:

- surfaces → `Canvas`; primary text → `CanvasText`; links/actions → `LinkText`/`ButtonText` + `ButtonFace`;
- borders/grid/bounds → `CanvasText`; selected → `Highlight`/`HighlightText`;
- focus ring → `Highlight` с видимым outline;
- disabled → `GrayText` без снижения opacity как единственного отличия;
- semantic status сохраняет icon + label + border; цвет становится вспомогательным.

## 5. Темы и состояние

- Setting хранит только enum `system | light | dark`, не вычисленную тему.
- Изменение OS theme при `system` применяется без перезагрузки и без потери focus/selection/form state.
- До загрузки настройки используется `color-scheme: light dark` и короткий inline bootstrap без внешних ресурсов, чтобы уменьшить theme flash; точный безопасный механизм определяет P2-00/CSP.
- Полные P1 макеты создаются в light; P1-08 получает дополнительный полный dark reference. Остальные dark sheets проверяют component/state coverage, а не дублируют все screens.

## 6. Не кодировать смысл цветом

| Состояние | Помимо цвета обязательно |
|---|---|
| success/verified | Icon + localized label + подтверждённый read-back detail |
| warning/partial | Icon + reason/count + доступное действие |
| danger/blocking | Icon + heading/reason + blocked primary action |
| stale/conflict | Status label + Base/Local/Remote relation + required next step |
| unknown outcome | Отдельный label/icon, запрет повторной записи, reconciliation action |
| editable/read-only/unsupported | Текстовый badge/column; не только green/gray/red |
| diff add/change/remove | `+`, `~`, `−`/labels и structural context |

## 7. Риски и проверки

- Brand orange остаётся accent/illustration и не используется как единственный warning/focus signal.
- Data-series colors принадлежат widget preview/codec model и не берутся из UI status palette.
- Проверить WCAG contrast для текста, controls, focus и adjacent status boundaries; проверить protanopia/deuteranopia/grayscale как дополнительную эвристику.
- Проверить Windows High Contrast, `prefers-contrast`, `prefers-reduced-motion` и browser zoom 200% в P1-00G/P1-11.
- В P1-00H и полном dark reference P1-08 повторно проверить покрытие всех aliases и состояний; это downstream validation принятой taxonomy, а не условие существования runtime implementation.
