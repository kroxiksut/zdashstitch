# P1-01 — индекс проверочных SVG-макетов

Статус: принято владельцем 25.10.2026; обновляется при каждом новом макете.

Правило для последующих проверочных макетов: только русский язык, только светлая тема и ровно один экран в одном SVG-файле. Это правило относится к дизайн-артефактам и не отменяет поддержку тем `system`, `light` и `dark` в приложении. Шрифты в макетах — только системный стек (P1-00B). Управляющие пиктограммы — только Tabler Icons outline (O-024), встроенные как `<symbol>` и записанные в [`assets/icons/THIRD_PARTY.md`](../../assets/icons/THIRD_PARTY.md).

Макеты разложены по папкам разделов приложения. Номер папки — порядок разделов в навигации, имя файла начинается с `<папка>-<порядок>`, затем задача P1 и краткое имя, поэтому сортировка по имени совпадает с порядком просмотра. Макеты масштабируются по ширине окна просмотра (у корневого `<svg>` нет фиксированных пикселей, только `viewBox`).

| Папка | Раздел | Статус |
|---|---|---|
| `00-foundations` | основы: компоненты, темы, плотный справочник | есть |
| `01-start` | «Начало» | есть |
| `02-drafts` | «Черновики» — список и импорт файлов | есть, P1-03 |
| `03-server` | «Панели в Zabbix» | есть, P1-03 |
| `04-library` | «Библиотека» | есть, P1-03 (каталоги метрик) |
| `05-connections` | «Подключения» | есть, P1-02 |
| `06-settings` | «Настройки» | есть, начат P1-09A |
| `07-overview` | рабочая область: «Обзор» | есть |
| `08-structure` | рабочая область: «Структура» | есть |
| `09-bindings` | рабочая область: «Привязки» | есть, P1-05 |
| `10-checks` | рабочая область: «Проверки» | план (P1-04F); проверки публикации — в `11-publication` |
| `11-publication` | рабочая область: «Публикация», включая владельца и доступ | есть, P1-08 |
| `12-editor` | редактор страницы на холсте | есть, P1-04 |
| `13-export` | экспорт, обезличивание, преобразование копии | есть, P1-07 |

| Файл | Задача | Экран |
|---|---|---|
| [`00-foundations/00-01_P1-00H_reference-pack.svg`](assets/00-foundations/00-01_P1-00H_reference-pack.svg) | P1-00H | листы компонентов (светлая и тёмная тема), плотный справочник публикации |
| [`00-foundations/00-02_P1-10_mascot-states.svg`](assets/00-foundations/00-02_P1-10_mascot-states.svg) | P1-10 | Стежик: разрешённые состояния, движение, запреты |
| [`01-start/01-01_P1-01A_start.svg`](assets/01-start/01-01_P1-01A_start.svg) | P1-01A | «Начало»: панели в Zabbix, локальные черновики, недавнее |
| [`02-drafts/02-01_P1-03F_drafts-list.svg`](assets/02-drafts/02-01_P1-03F_drafts-list.svg) | P1-03F | «Черновики»: список и сведения о копии |
| [`02-drafts/02-02_P1-03A_import-files.svg`](assets/02-drafts/02-02_P1-03A_import-files.svg) | P1-03A/G | «Импорт файлов»: несколько файлов, что из них станет |
| [`02-drafts/02-03_P1-03D_import-result.svg`](assets/02-drafts/02-03_P1-03D_import-result.svg) | P1-03D/G | итог импорта файлов |
| [`03-server/03-01_P1-03B_server-dashboards.svg`](assets/03-server/03-01_P1-03B_server-dashboards.svg) | P1-03B | «Панели в Zabbix» → «Панели» |
| [`03-server/03-02_P1-03B_server-templates.svg`](assets/03-server/03-02_P1-03B_server-templates.svg) | P1-03B/C | «Панели в Zabbix» → «Шаблоны»: выбор для каталогов метрик |
| [`03-server/03-03_P1-03C_copy-scope.svg`](assets/03-server/03-03_P1-03C_copy-scope.svg) | P1-03C | проверка перед копированием панели |
| [`04-library/04-01_P1-03F_library-catalogs.svg`](assets/04-library/04-01_P1-03F_library-catalogs.svg) | P1-03F/E | «Библиотека» → «Каталоги метрик» |
| [`05-connections/05-01_P1-02A_connections-list.svg`](assets/05-connections/05-01_P1-02A_connections-list.svg) | P1-02A | «Подключения»: список профилей |
| [`05-connections/05-02_P1-02B_profile-editor.svg`](assets/05-connections/05-02_P1-02B_profile-editor.svg) | P1-02B/C/F | редактор профиля: адрес, вход, сертификат |
| [`05-connections/05-03_P1-02D_connection-test.svg`](assets/05-connections/05-03_P1-02D_connection-test.svg) | P1-02D | пошаговая проверка подключения |
| [`05-connections/05-04_P1-02E_enable-writes.svg`](assets/05-connections/05-04_P1-02E_enable-writes.svg) | P1-02E | подтверждение режима «Чтение-запись» |
| [`06-settings/06-01_P1-09A_settings-appearance.svg`](assets/06-settings/06-01_P1-09A_settings-appearance.svg) | P1-09A/C | «Настройки» → «Внешний вид» |
| [`06-settings/06-02_P1-09B_settings-file-error.svg`](assets/06-settings/06-02_P1-09B_settings-file-error.svg) | P1-09B | «Настройки» → «Общие»: ошибочный файл настроек, резервные копии, перенос и сброс |
| [`06-settings/06-03_P1-09D_storage-move.svg`](assets/06-settings/06-03_P1-09D_storage-move.svg) | P1-09D | «Настройки» → «Рабочие каталоги»: хранилище, автосохранение, перенос корня |
| [`06-settings/06-05_P1-09G_security-export.svg`](assets/06-settings/06-05_P1-09G_security-export.svg) | P1-09G | «Настройки» → «Безопасность и экспорт» |
| [`06-settings/06-06_P1-09G_diagnostics.svg`](assets/06-settings/06-06_P1-09G_diagnostics.svg) | P1-09G | «Настройки» → «Диагностика»: журнал и пакет для поддержки |
| [`06-settings/06-04_P1-12_about.svg`](assets/06-settings/06-04_P1-12_about.svg) | P1-12 | «Настройки» → «О программе и лицензии» |
| [`07-overview/07-01_P1-01B_page-copy-overview.svg`](assets/07-overview/07-01_P1-01B_page-copy-overview.svg) | P1-01B | «Обзор» копии страницы с сервера |
| [`07-overview/07-02_P1-03E_reimport-replay.svg`](assets/07-overview/07-02_P1-03E_reimport-replay.svg) | P1-03E | «Обзор»: «Скопировать заново», переигрывание правок |
| [`08-structure/08-01_P1-01B_dashboard-structure.svg`](assets/08-structure/08-01_P1-01B_dashboard-structure.svg) | P1-01B | «Структура» локальной панели: дерево как в Zabbix и предпросмотр страницы |
| [`08-structure/08-02_P1-01I_assemble-dashboard.svg`](assets/08-structure/08-02_P1-01I_assemble-dashboard.svg) | P1-01I | «Структура»: сборка панели из страниц разных источников |
| [`09-bindings/09-01_P1-05B_scope-inventory.svg`](assets/09-bindings/09-01_P1-05B_scope-inventory.svg) | P1-05A/B | замена узла: область и ссылки узла |
| [`09-bindings/09-02_P1-05C_target-picker.svg`](assets/09-bindings/09-02_P1-05C_target-picker.svg) | P1-05C | клон: целевые узлы с порядком |
| [`09-bindings/09-03_P1-05D_mapping-table.svg`](assets/09-bindings/09-03_P1-05D_mapping-table.svg) | P1-05D | замена узла: таблица соответствий |
| [`09-bindings/09-04_P1-05D_clone-decisions.svg`](assets/09-bindings/09-04_P1-05D_clone-decisions.svg) | P1-05D | клон: решения по маскам, общий узел, список «останутся» |
| [`09-bindings/09-05_P1-05F_cardinality.svg`](assets/09-bindings/09-05_P1-05F_cardinality.svg) | P1-05F | клон: серии 2 → 3 |
| [`09-bindings/09-06_P1-05E_clone-summary.svg`](assets/09-bindings/09-06_P1-05E_clone-summary.svg) | P1-05E | клон: итог преобразования |
| [`09-bindings/09-07_P1-05G_bindings-status.svg`](assets/09-bindings/09-07_P1-05G_bindings-status.svg) | P1-05G | состояние привязок и повторная проверка |
| [`11-publication/11-01_P1-08E_target-access-superadmin.svg`](assets/11-publication/11-01_P1-08E_target-access-superadmin.svg) | P1-08A/E/I | «Публикация» → «Назначение»: клонирование, Super admin, владелец и доступ |
| [`11-publication/11-02_P1-08I_target-access-admin.svg`](assets/11-publication/11-02_P1-08I_target-access-admin.svg) | P1-08E/I | «Назначение»: замена узла сети, Admin, доступ только для просмотра |
| [`11-publication/11-03_P1-08C_preflight.svg`](assets/11-publication/11-03_P1-08C_preflight.svg) | P1-08C | «Проверки»: блокировка и предупреждение |
| [`11-publication/11-04_P1-08B_changes.svg`](assets/11-publication/11-04_P1-08B_changes.svg) | P1-08B | «Изменения»: два сравнения, таблица по страницам, инспектор |
| [`11-publication/11-05_P1-08D_conflict.svg`](assets/11-publication/11-05_P1-08D_conflict.svg) | P1-08D | конфликт: панель изменилась на сервере |
| [`11-publication/11-06_P1-08F_confirm.svg`](assets/11-publication/11-06_P1-08F_confirm.svg) | P1-08F | подтверждение одноразового плана |
| [`11-publication/11-07_P1-08G_verified.svg`](assets/11-publication/11-07_P1-08G_verified.svg) | P1-08G | результат: проверено повторным чтением |
| [`11-publication/11-08_P1-08G_unknown-outcome.svg`](assets/11-publication/11-08_P1-08G_unknown-outcome.svg) | P1-08G | результат неизвестен, сверка чтением |
| [`12-editor/12-01_P1-04A_editor-canvas.svg`](assets/12-editor/12-01_P1-04A_editor-canvas.svg) | P1-04A/B/E | редактор страницы: вкладки, холст 72 колонки, список виджетов, инспектор |
| [`12-editor/12-02_P1-04C_widget-chooser.svg`](assets/12-editor/12-02_P1-04C_widget-chooser.svg) | P1-04C | выбор виджета по возможностям |
| [`12-editor/12-03_P1-04D_inspector-svggraph.svg`](assets/12-editor/12-03_P1-04D_inspector-svggraph.svg) | P1-04D | инспектор «График» |
| [`12-editor/12-04_P1-04E_paste-resolution.svg`](assets/12-editor/12-04_P1-04E_paste-resolution.svg) | P1-04E | вставка виджетов с другой панели |
| [`12-editor/12-06_P1-04I_synthetic-preview.svg`](assets/12-editor/12-06_P1-04I_synthetic-preview.svg) | P1-04I | предпросмотр страницы с синтетическими данными |
| [`13-export/13-01_P1-07A_export-goal.svg`](assets/13-export/13-01_P1-07A_export-goal.svg) | P1-07A/B/F | экспорт: цель, состав, исключения, совместимость |
| [`13-export/13-02_P1-07C_sensitive-review.svg`](assets/13-export/13-02_P1-07C_sensitive-review.svg) | P1-07C/D | проверка значений и замены «было — станет» |
| [`13-export/13-03_P1-07G_export-result.svg`](assets/13-export/13-03_P1-07G_export-result.svg) | P1-07G | результат экспорта |
| [`13-export/13-04_P1-07I_workcopy-convert.svg`](assets/13-export/13-04_P1-07I_workcopy-convert.svg) | P1-07I | преобразование копии с сервера в переносимый черновик |

На каждом экране шапка содержит глобальную навигацию, метку режима подключения и кнопку «Настройки» (P1-01B §1). Правила оформления, цвета, иконки и проверка — [`MOCKUP_GUIDE.md`](MOCKUP_GUIDE.md); генераторы — `docs/design/tools/`.

## Зафиксированная предметная модель (O-025, уточнение 24.10.2026)

- Zabbix: `Dashboard → Page → Widget`; в русском интерфейсе — официальные термины Zabbix: **панель → страница → виджет**, а также «узел сети», «элемент данных», «группа узлов сети».
- Слово «проект» в интерфейсе не используется. Локальный черновик называется своей сущностью — «Панель» или «Страница» — и несёт метку происхождения: «Локальная» (создана в ZDashstitch или импортирована из файла) либо «Копия с сервера» (рабочая копия снимка).
- Источник и назначение — разные контексты; страницу можно взять из одной панели и добавить в другую.
- Выбор отдельных виджетов задаёт область действия и не образует третий тип черновика.
- Страница — самостоятельная единица работы (O-027): её можно взять без всей панели, добавить в другую панель, собрать из страниц новую панель, экспортировать и импортировать отдельным файлом. Потоки — [`P1-01I-page-flows.md`](P1-01I-page-flows.md).
- Снимок не показывается как самостоятельный объект работы: он — происхождение копии с сервера.
- Области интерфейса не называются «панелью»: «боковая колонка», «инспектор», «область».

Прежние макеты `P1-01A-entry-project-kind.svg` и `P1-01B-page-project-shell.svg` (модель «проект дашборда / проект страницы») заменены этими файлами.
