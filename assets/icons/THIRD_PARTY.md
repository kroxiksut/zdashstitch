# Third-party interface icons

This directory records the provenance and licence of icon artwork used by ZDashstitch. It does not cover the in-house ZDashstitch logo or Stezhik artwork under `assets/brand/`.

## Tabler Icons (all platforms)

- **Author:** Tabler / Paweł Kuna and contributors
- **Licence:** MIT
- **Home page:** <https://tabler.io/icons>
- **Source:** <https://github.com/tabler/tabler-icons>
- **Style used by ZDashstitch:** outline
- **Scope:** interface, status and tray icons on every supported platform

ZDashstitch contributors must not invent, redraw or silently substitute interface, status or tray icons. Select an existing icon from the Tabler outline set, record its upstream name, and preserve the accessible label/message ID separately from the artwork. The application's own logo is original project artwork and is not part of Tabler Icons.

Agents may download the Tabler outline SVGs required by their current task directly from the official `tabler/tabler-icons` upstream without additional approval. They must use an exact release tag or commit rather than an unpinned `latest` URL, vendor only the required files, preserve upstream filenames/names, validate that the SVGs contain no scripts or external references, and keep runtime use local/offline and CSP-safe. This authorization does not cover another icon library or unrelated third-party assets.

When actual Tabler SVGs or a Tabler package are added to a distributable artifact, record the pinned upstream package/release or commit, source URLs and included icon names here or in a generated inventory next to this notice. This notice and the MIT text below must travel with the corresponding source and binary distributions and must be exposed by the in-app Licenses window.

### Vendored inventory

| Field | Value |
|---|---|
| Release | `v3.49.0` |
| Commit | `bbed884d15354b5cebf2493371f20dc2d5e83eaf` |
| Source URL pattern | `https://raw.githubusercontent.com/tabler/tabler-icons/bbed884d15354b5cebf2493371f20dc2d5e83eaf/icons/outline/<name>.svg` |
| Local path | `assets/icons/tabler/outline/<name>.svg` (upstream files, unchanged) |
| Validation | no `<script>`, event handlers, `href`/`xlink:href` or `url(...)` references |

| Icon (upstream name) | Used in |
|---|---|
| `grip-vertical` | review mockups P1-01I, P1-01B structure: drag handle of a page or widget row |
| `arrow-up` | review mockups P1-01I, P1-01B structure: move page up |
| `arrow-down` | review mockups P1-01I, P1-01B structure: move page down |
| `x` | review mockup P1-01I: remove page from the dashboard |
| `pencil` | review mockup P1-00H: changed row marker |
| `alert-triangle` | review mockup P1-00H: row needs a decision |
| `ban` | review mockup P1-00H: unsupported object |
| `chevron-down` | review mockup P1-01B structure: expanded tree node, select |
| `chevron-right` | review mockup P1-01B structure: collapsed tree node |
| `check` | review mockups P1-01B structure, P1-09A settings: checked checkbox |
| `settings` | all review mockups: «Настройки» button in the app bar |
| `search` | review mockup P1-09A settings: settings search field |
| `circle-check` | review mockup P1-02D: passed check step |
| `circle-x` | review mockup P1-02D: failed check step |
| `circle-dashed` | review mockup P1-02D: step not checked |
| `plus` | review mockup P1-02A: «Новое подключение» |
| `alert-triangle` | also P1-02A/B/E: HTTP and server-change warnings |
| `refresh` | review mockups P1-08C/D/G: «Проверить снова», «Проверить результат» |
| `lock` | review mockup P1-08I: private dashboard |
| `users` | review mockups P1-08I: shared dashboard, sharing recipients |
| `external-link` | review mockups P1-08G: «Открыть в Zabbix» |
| `info-circle` | review mockups P1-08: informational notes |
| `minus` | review mockup P1-08B: unchanged row marker |
| `arrow-back-up` | review mockups P1-04: «Отменить» |
| `arrow-forward-up` | review mockups P1-04: «Повторить» |
| `zoom-in` | review mockups P1-04: «Увеличить» |
| `zoom-out` | review mockups P1-04: «Уменьшить» |
| `folder` | review mockups P1-09D/G: storage root, choose folder |
| `logout` | review mockup P1-09G: «Завершить сеанс» |
| `download` | review mockup P1-09G: «Сохранить пакет…» |
| `copy` | review mockup P1-12: «Копировать путь», «Копировать сведения для отчёта» |
| `arrows-move` | P1-08B diff model: moved page or widget marker (specified, not drawn yet) |
| `upload` | review mockups P1-06C: «Импорт файлов» in the library (profiles and blueprints) |
| `palette` | review mockups P1-06C/D: style profile marker, «Сохранить оформление как профиль» |
| `template` | review mockups P1-06F: widget blueprint marker in «Добавить виджет → Из заготовок» |

Review mockups in `docs/design/assets/` inline the upstream path data as `<symbol>` elements so the SVG stays self-contained; the path data is not modified.

#### Runtime use in the application UI

The UI bundles the unchanged `<path d>` data of these vendored files in `ui/src/lib/components/icons.ts` and renders it through `Icon.svelte` as decorative inline SVG (no raw HTML, no external references); `icons.test.ts` checks the data against the vendored files. The meaning is always carried by adjacent text.

| Icon (upstream name) | Used in |
|---|---|
| `settings` | app header: «Настройки» link |
| `plus` | «Подключения»: «Новое подключение»; «Черновики»: «Создать» |
| `search` | «Черновики»: «Поиск по имени» field |
| `circle-check` | connection check: passed step; «Черновики» details: created dashboard verified by reading back; bindings: passed wizard step, exact match, applied decision |
| `circle-x` | connection check: failed step; bindings and publication preview: not found, unresolved, blocking finding |
| `circle-dashed` | connection check: step not checked; bindings: unverified reference, out-of-date binding |
| `info-circle` | connection check: informational step; «Черновики» details: safe deletion note |
| `alert-triangle` | connection profile, list and «Разрешить запись»: HTTP and write warnings; «Черновики» details: created dashboard not found at the last check; bindings: match with differences, ambiguous match, partial scope; publication preview: warning finding |
| `copy` | bundled for the connection check report copy action (mockup 05-03); not rendered yet |
| `pencil` | «Привязки» and «Публикация» (ReplaceHost): changed reference, widget or field marker |
| `minus` | «Публикация» (ReplaceHost) changes: unchanged widget marker; bindings summary |
| `ban` | «Привязки» and «Публикация» (ReplaceHost): reference that stays on the old host |
| `refresh` | «Привязки»: «Проверить все»; «Публикация»: «Проверить снова» |

### MIT License

Copyright (c) 2020-2026 Paweł Kuna

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
