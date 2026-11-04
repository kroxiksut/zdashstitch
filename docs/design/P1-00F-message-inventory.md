# P1-00F — локализация, терминология и message-ID map

**Версия:** 1.0, 2026-10-24. **Статус:** принято как полный inventory для P1 reference pack. Это не runtime-каталог: JSON, typed bindings и проверки placeholders реализуются отдельно в P2-00.

## 1. Контракт ключей и параметров

- Полный ключ: `<namespace>.<group>.<meaning>`, lower `snake_case`, без screen number и visual position.
- English — source copy. Russian — обязательная полная пара с теми же named placeholders.
- Параметры типизированы как `text`, `integer`, `date_time`, `duration`, `technical`, `user_data`, `server_data`, `message_id`. HTML и готовые фразы параметрами не передаются.
- Числа, даты, время и plural форматирует locale. Предложения не собираются конкатенацией.
- User/server data не переводятся и всегда получают локализованный context. API methods/fields, IDs, keys, macros and versions остаются technical values.
- Backend возвращает `code`, safe parameters, severity and diagnostic ID. Он не возвращает готовую пользовательскую фразу.

Notation в таблицах: `{name:type}`. `plural(count; one | other)` означает locale-aware plural branches, а не строковую склейку.

## 2. Core inventory

### `common`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `common.action.continue` | Continue | Продолжить | — |
| `common.action.cancel` | Cancel | Отмена | — |
| `common.action.close` | Close | Закрыть | — |
| `common.action.back` | Back | Назад | — |
| `common.action.retry` | Try again | Повторить | — |
| `common.action.refresh` | Refresh | Обновить | — |
| `common.action.copy` | Copy | Копировать | — |
| `common.action.open_details` | Show technical details | Показать технические сведения | — |
| `common.action.hide_details` | Hide technical details | Скрыть технические сведения | — |
| `common.action.choose_file` | Choose file… | Выбрать файл… | — |
| `common.action.choose_folder` | Choose folder… | Выбрать папку… | — |
| `common.action.save_as` | Save as… | Сохранить как… | — |
| `common.state.loading` | Loading… | Загрузка… | — |
| `common.state.offline` | Offline | Автономно | — |
| `common.state.read_only` | Read-only | Только чтение | — |
| `common.state.read_write` | Read-write | Чтение-запись | — |
| `common.state.partial` | Partially available | Доступно частично | — |
| `common.state.unsupported` | Unsupported | Не поддерживается | — |
| `common.state.edit_unsupported` | Editing not supported | Изменение не поддерживается | — |
| `common.state.verified` | Verified by reading back from Zabbix | Проверено повторным чтением | — |
| `common.state.unknown_outcome` | Result unknown | Результат неизвестен | — |
| `common.count.items` | `{count, plural, one {# item} other {# items}}` | `{count, plural, one {# элемент} few {# элемента} many {# элементов} other {# элемента}}` | `count:integer` |
| `common.a11y.skip_to_content` | Skip to main content | Перейти к основному содержимому | — |
| `common.a11y.close_dialog` | Close dialog | Закрыть диалог | — |
| `common.a11y.copy_value` | Copy {label} | Копировать: {label} | `label:message_id` |
| `common.a11y.sort_column` | Sort by {column} | Сортировать по столбцу «{column}» | `column:message_id` |
| `common.a11y.resize_separator` | Resize {panel} | Изменить размер области «{panel}» | `panel:message_id` |

### `startup` and `session`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `startup.title` | ZDashstitch | ZDashstitch | — |
| `startup.subtitle` | Create and maintain Zabbix dashboards locally | Локальное создание и сопровождение панелей Zabbix | — |
| `startup.question` | What do you want to do with Zabbix dashboards? | Что вы хотите сделать с панелями Zabbix? | — |
| `startup.server.title` | Dashboards in Zabbix | Панели в Zabbix | — |
| `startup.server.help` | Replace a host, clone a dashboard or take a page. | Заменить узел сети, клонировать панель или взять страницу. | — |
| `startup.action.choose_dashboard` | Choose dashboard | Выбрать панель | — |
| `startup.action.change_connection` | Change connection | Сменить подключение | — |
| `startup.local.title` | Local drafts | Локальные черновики | — |
| `startup.local.help` | Prepare a dashboard or a page and publish it later. | Подготовьте панель или страницу и опубликуйте позже. | — |
| `startup.action.new_dashboard` | Create dashboard | Создать панель | — |
| `startup.action.new_page` | Create page | Создать страницу | — |
| `startup.local.dashboard_help` | Several pages and their widgets. | Несколько страниц и их виджеты. | — |
| `startup.local.page_help` | One page and its widgets. | Одна страница и её виджеты. | — |
| `startup.action.assemble_dashboard` | Build from pages | Собрать из страниц | — |
| `startup.action.take_page` | Take a page | Взять страницу | — |
| `startup.local.assemble_help` | Combine pages from other dashboards and files. | Объедините страницы других панелей и файлов. | — |
| `startup.local.take_page_help` | Take one page without the whole dashboard. | Возьмите одну страницу без всей панели. | — |
| `startup.recent.title` | Recent | Недавнее | — |
| `startup.recent.empty` | No local drafts yet | Локальных черновиков пока нет | — |
| `startup.action.all_drafts` | All local drafts | Все локальные черновики | — |
| `startup.action.import_files` | Import files | Импортировать файлы | — |
| `startup.action.connect` | Connect to Zabbix | Подключиться к Zabbix | — |
| `startup.recovery.found` | Unfinished operation found | Найдена незавершённая операция | — |
| `session.bootstrap.invalid` | Open ZDashstitch from its launcher | Откройте ZDashstitch через ярлык запуска | — |
| `session.expired.title` | Local session ended | Локальный сеанс завершён | — |
| `session.expired.body` | Reopen the application from its launcher. Saved local drafts remain on disk; unsaved browser input is not restored. | Снова откройте приложение через ярлык. Сохранённые локальные черновики остались на диске; несохранённый ввод в браузере не восстанавливается. | — |
| `session.tab.additional` | This tab uses the current local session | Эта вкладка использует текущий локальный сеанс | — |

### `connection`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `connection.list.title` | Connections | Подключения | — |
| `connection.form.endpoint` | Zabbix URL | Адрес Zabbix | — |
| `connection.form.auth_method` | Authentication method | Способ аутентификации | — |
| `connection.form.api_token` | API token | API-токен | — |
| `connection.form.username` | User name | Имя пользователя | — |
| `connection.form.password` | Password | Пароль | — |
| `connection.form.custom_ca` | Trusted CA certificate | Доверенный сертификат УЦ | — |
| `connection.action.test` | Test connection | Проверить подключение | — |
| `connection.action.connect_read_only` | Connect in read-only mode | Подключиться в режиме только чтения | — |
| `connection.action.enable_writes` | Enable dashboard writes | Разрешить запись панелей | — |
| `connection.progress.stage` | Checking {stage}… | Проверка: {stage}… | `stage:message_id` |
| `connection.result.version` | Zabbix {version} | Zabbix {version} | `version:technical` |
| `connection.mode.read_only_help` | Server data can be read; Zabbix objects will not be changed. | Данные сервера можно читать; объекты Zabbix изменяться не будут. | — |
| `connection.mode.read_write_help` | Only verified global-dashboard operations can be written after preview and confirmation. | После предпросмотра и подтверждения можно записывать только проверенные операции с глобальными панелями. | — |
| `connection.auth.mfa_token_required` | Password sign-in cannot be used for this MFA account. Use an API token allowed by your administrator. | Для этой учётной записи с MFA нельзя войти по паролю. Используйте API-токен, разрешённый администратором. | — |

### `import`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `import.title` | Import | Импорт | — |
| `import.source.files` | Template export files | Файлы экспорта шаблонов | — |
| `import.source.dashboard` | Global dashboard snapshot | Снимок глобальной панели | — |
| `import.drop.prompt` | Drop YAML, JSON, XML or a ZDashstitch ZIP here | Перетащите сюда YAML, JSON, XML или ZIP ZDashstitch | — |
| `import.progress.reading` | Reading {file_name}… | Чтение файла «{file_name}»… | `file_name:user_data` |
| `import.progress.objects` | Processed {completed} of {total} objects | Обработано объектов: {completed} из {total} | `completed:integer`, `total:integer` |
| `import.result.complete` | Import completed | Импорт завершён | — |
| `import.result.partial` | Import completed with unsupported or unresolved objects | Импорт завершён с неподдерживаемыми или неразрешёнными объектами | — |
| `import.result.template_dashboards` | {count, plural, one {# template dashboard was not imported} other {# template dashboards were not imported}} | {count, plural, one {# панель шаблона не импортирована} few {# панели шаблонов не импортированы} many {# панелей шаблонов не импортировано} other {# панели шаблона не импортировано}} | `count:integer` |
| `import.security.untrusted` | Imported files are untrusted data. Scripts, external resources and network references are not executed. | Импортируемые файлы считаются недоверенными данными. Скрипты, внешние ресурсы и сетевые ссылки не выполняются. | — |

### `library` and `dashboards`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `library.title` | Library | Библиотека | — |
| `library.section.snapshots` | Dashboard and page snapshots | Снимки панелей и страниц | — |
| `library.section.pages` | Pages | Страницы | — |
| `library.section.catalogs` | Metric catalogs | Каталоги метрик | — |
| `library.section.blueprints` | Widget blueprints | Заготовки виджетов | — |
| `library.empty` | No local objects yet | Локальных объектов пока нет | — |
| `dashboards.title` | Global dashboards | Глобальные панели | — |
| `dashboards.action.import_snapshot` | Copy from server | Скопировать с сервера | — |
| `dashboards.visibility.private` | Private | Личная | — |
| `dashboards.permission.not_visible_or_denied` | No dashboards found, or your account cannot view them | Панели не найдены либо учётная запись не может их просматривать | — |
| `dashboards.summary.pages_widgets` | {pages, plural, one {# page} other {# pages}}, {widgets, plural, one {# widget} other {# widgets}} | {pages, plural, one {# страница} few {# страницы} many {# страниц} other {# страницы}}, {widgets, plural, one {# виджет} few {# виджета} many {# виджетов} other {# виджета}} | `pages:integer`, `widgets:integer` |

### `object` and `intent`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `object.kind.dashboard` | Dashboard | Панель | — |
| `object.kind.page` | Page | Страница | — |
| `object.origin.local` | Local | Локальная | — |
| `object.origin.server_copy` | Copy from server | Копия с сервера | — |
| `intent.create_dashboard` | Create dashboard | Создать панель | — |
| `intent.save_as_copy` | Save as copy | Сохранить копию | — |
| `intent.append_page` | Add page to dashboard | Добавить страницу в панель | — |
| `intent.append_widgets` | Add widgets to page | Добавить виджеты на страницу | — |
| `intent.clone_dashboard` | Clone dashboard | Клонировать панель | — |
| `intent.replace_host` | Replace host | Заменить узел сети | — |
| `intent.update_dashboard` | Update dashboard | Обновить панель | — |
| `intent.assemble_dashboard` | Build dashboard from pages | Собрать панель из страниц | — |
| `intent.take_page` | Take page | Взять страницу | — |
| `assemble.title` | Build dashboard from pages | Сборка панели из страниц | — |
| `assemble.action.add_page` | Add page from… | Добавить страницу из… | — |
| `assemble.source.server` | Dashboard in Zabbix | Панель в Zabbix | — |
| `assemble.source.draft` | Local page | Локальная страница | — |
| `assemble.source.file` | Page file | Файл страницы | — |
| `assemble.reference.renamed` | {count, plural, one {# widget link was renamed to stay unique} other {# widget links were renamed to stay unique}} | {count, plural, one {# связь виджетов переименована для уникальности} few {# связи виджетов переименованы для уникальности} many {# связей виджетов переименовано для уникальности} other {# связи виджетов переименовано для уникальности}} | `count:integer` |
| `assemble.page.template_rejected` | Pages of template dashboards cannot be added: their widgets refer to template items. | Страницы панелей шаблонов добавить нельзя: их виджеты ссылаются на элементы данных шаблона. | — |

Внутренние имена intent (`CreateDashboard`, `AppendPage` и т. д.) в пользовательском тексте не показываются; они допустимы только в технических сведениях.

### `editor`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `editor.title` | Dashboard editor | Редактор панели | — |
| `editor.canvas.label` | Dashboard layout, 72 columns by 64 rows | Макет панели, 72 колонки × 64 строки | — |
| `editor.action.add_page` | Add page | Добавить страницу | — |
| `editor.action.add_widget` | Add widget | Добавить виджет | — |
| `editor.action.move` | Move | Переместить | — |
| `editor.action.resize` | Resize | Изменить размер | — |
| `editor.action.undo` | Undo {action} | Отменить: {action} | `action:message_id` |
| `editor.action.redo` | Redo {action} | Повторить: {action} | `action:message_id` |
| `editor.inspector.title` | Inspector | Инспектор | — |
| `editor.geometry.position` | Position: column {x}, row {y} | Позиция: столбец {x}, строка {y} | `x:integer`, `y:integer` |
| `editor.geometry.size` | Size: {width} by {height} cells | Размер: {width} на {height} ячеек | `width:integer`, `height:integer` |
| `editor.validation.collision` | This widget overlaps another widget | Этот виджет перекрывает другой виджет | — |
| `editor.validation.out_of_bounds` | This widget is outside the 72 by 64 grid | Этот виджет выходит за пределы сетки 72 на 64 | — |
| `editor.preview.no_live_data` | Offline preview shows layout and settings, not live monitoring values. | Автономный предпросмотр показывает компоновку и настройки, но не реальные значения мониторинга. | — |

### `bindings` and `replace-host`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `bindings.title` | Data bindings | Привязки данных | — |
| `bindings.target_area` | Target area | Целевая область | — |
| `bindings.source_evidence` | Matching evidence | Основания сопоставления | — |
| `bindings.match.exact` | Exact match | Точное соответствие | — |
| `bindings.match.compatible` | Compatible with differences | Совместимо с отличиями | — |
| `bindings.match.ambiguous` | Multiple candidates | Несколько кандидатов | — |
| `bindings.match.missing` | No matching item | Соответствующий элемент данных не найден | — |
| `bindings.match.unresolved_lld` | LLD instance is not available on the target | Экземпляр LLD недоступен на целевом узле сети | — |
| `bindings.match.denied` | Not found or not visible with current permissions | Не найдено или не видно с текущими правами | — |
| `bindings.fitness.editable` | Editable | Можно изменить | — |
| `bindings.fitness.read_only` | Recognised, editing not supported | Распознано, изменение не поддерживается | — |
| `bindings.fitness.unsupported` | Unsupported dependency | Неподдерживаемая зависимость | — |
| `bindings.evidence.key` | Item key: {key} | Ключ элемента данных: {key} | `key:technical` |
| `bindings.evidence.units` | Units: {units} | Единицы измерения: {units} | `units:server_data` |
| `replace-host.title` | Replace host references | Заменить ссылки на узел сети | — |
| `replace-host.source` | Host to replace | Заменяемый узел сети | — |
| `replace-host.target` | Replacement host | Новый узел сети | — |
| `replace-host.result.summary` | Matched {matched} of {total} references | Сопоставлено ссылок: {matched} из {total} | `matched:integer`, `total:integer` |
| `replace-host.blocked.unresolved` | Resolve or exclude every unsupported reference before preview | Перед предпросмотром разрешите или исключите каждую неподдерживаемую ссылку | — |

### `clone` and `profiles`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `clone.title` | Clone dashboard | Клонировать панель | — |
| `clone.source` | Source dashboard | Исходная панель | — |
| `clone.new_name` | New dashboard name | Имя новой панели | — |
| `clone.target_area` | Target binding area | Целевая область привязок | — |
| `clone.notice.private` | By default the copy gets the sharing of the original; you can change it in Publication. The original will not be changed. | По умолчанию копия получает доступ исходной панели; его можно изменить в «Публикации». Исходная панель не изменится. | — |
| `clone.cardinality.change` | Change host slots from {before} to {after} | Изменить число слотов узлов сети: {before} → {after} | `before:integer`, `after:integer` |
| `profiles.title` | Style profiles | Профили оформления | — |
| `profiles.blueprints.title` | Widget blueprints | Заготовки виджетов | — |
| `profiles.action.extract` | Create profile from selection | Создать профиль из выбранного | — |
| `profiles.action.apply` | Apply profile | Применить профиль | — |
| `profiles.compatibility.partial` | Some settings cannot be applied to this widget type | Часть настроек нельзя применить к этому типу виджета | — |

### `export`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `export.title` | Export | Экспорт | — |
| `export.mode.portable` | Portable data-only package | Переносимый пакет только с данными | — |
| `export.mode.private_transfer` | Private transfer | Доверенный перенос | — |
| `export.mode.private_transfer_blocked` | Private transfer is not available until its security format is approved. | Доверенный перенос недоступен до утверждения защищённого формата. | — |
| `export.scan.title` | Review potentially sensitive values | Проверьте потенциально чувствительные значения | — |
| `export.scan.not_anonymous` | A portable package is not guaranteed to be anonymized. Follow your organization’s sharing policy. | Переносимый пакет не гарантирует обезличивание. Соблюдайте правила организации при передаче. | — |
| `export.summary.files` | Package contains {count, plural, one {# file} other {# files}} | Пакет содержит {count, plural, one {# файл} few {# файла} many {# файлов} other {# файла}} | `count:integer` |
| `export.action.save_portable` | Save portable package | Сохранить переносимый пакет | — |
| `export.result.saved` | Package saved as {file_name} | Пакет сохранён: {file_name} | `file_name:user_data` |

### `publication`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `publication.title` | Preview and publish | Предпросмотр и публикация | — |
| `publication.context.target` | Target dashboard | Целевая панель | — |
| `publication.context.operation` | Operation | Операция | — |
| `publication.diff.base` | Base | База | — |
| `publication.diff.local` | Local changes | Локальные изменения | — |
| `publication.diff.remote` | Current Zabbix state | Текущее состояние Zabbix | — |
| `publication.preflight.title` | Preflight checks | Предварительные проверки | — |
| `publication.preflight.blocked` | Publication is blocked | Публикация заблокирована | — |
| `publication.confirm.title` | Confirm dashboard write | Подтвердите запись панели | — |
| `publication.confirm.body` | Write {change_count} verified changes to {dashboard_name}? | Записать {change_count} проверенных изменений в панель «{dashboard_name}»? | `change_count:integer`, `dashboard_name:server_data` |
| `publication.action.publish` | Publish verified changes | Опубликовать проверенные изменения | — |
| `publication.progress.submitting` | Sending one verified dashboard request… | Отправка одного проверенного запроса панели… | — |
| `publication.progress.read_back` | Reading the dashboard back from Zabbix… | Повторное чтение панели из Zabbix… | — |
| `publication.result.verified` | Publication verified | Публикация проверена | — |
| `publication.result.visually_unverified` | Structure and data sources were verified. Check the dashboard visually in Zabbix. | Структура и источники данных проверены. Проверьте отображение панели в Zabbix. | — |
| `publication.conflict.title` | Dashboard changed after preview | Панель изменилась после предпросмотра | — |
| `publication.conflict.body` | The old plan cannot be published. Refresh the source or save a copy. | Старый план нельзя опубликовать. Обновите исходные данные или сохраните копию. | — |
| `publication.unknown.title` | Publication result is unknown | Результат публикации неизвестен | — |
| `publication.unknown.body` | Do not publish again. Read the dashboard to determine what happened. | Не повторяйте публикацию. Прочитайте панель, чтобы определить результат. | — |
| `publication.action.reconcile` | Check the result | Проверить результат | — |

### `settings` and `diagnostics`

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `settings.title` | Settings | Настройки | — |
| `settings.appearance.title` | Appearance | Внешний вид | — |
| `settings.theme.system` | Use system theme | Использовать системную тему | — |
| `settings.theme.light` | Light | Светлая | — |
| `settings.theme.dark` | Dark | Тёмная | — |
| `settings.language.title` | Language | Язык | — |
| `settings.storage.title` | Local storage | Локальное хранилище | — |
| `settings.editor.title` | Editor | Редактор | — |
| `settings.result.reloaded` | Settings reloaded | Настройки перечитаны | — |
| `diagnostics.title` | Diagnostics | Диагностика | — |
| `diagnostics.field.id` | Diagnostic ID | Идентификатор диагностики | — |
| `diagnostics.field.stage` | Stage | Этап | — |
| `diagnostics.field.code` | Error code | Код ошибки | — |
| `diagnostics.field.http_status` | HTTP status | Статус HTTP | — |
| `diagnostics.field.api_method` | API method | Метод API | — |
| `diagnostics.action.copy_safe` | Copy safe technical details | Копировать безопасные технические сведения | — |
| `diagnostics.notice.sanitized` | Secrets, raw bodies and full local paths are excluded. | Секреты, полные тела ответов и полные локальные пути исключены. | — |

## 3. Error-code map

| Backend/application code | Primary message ID | Safe parameters |
|---|---|---|
| `dns_unresolvable` | `errors.connection.dns_unresolvable` | `host_label?` only if already user-visible |
| `connection_refused` | `errors.connection.refused` | `port?` |
| `host_unreachable_or_timeout` | `errors.connection.unreachable_or_timeout` | `timeout?` |
| `wrong_base_path`, `redirect`, `not_jsonrpc` | `errors.connection.api_not_found` | `http_status?`, `stage` |
| `tls_untrusted` | `errors.tls.untrusted` | `certificate_subject?` sanitized |
| `tls_certificate_time_invalid` | `errors.tls.time_invalid` | no expired/not-yet-valid claim |
| `tls_name_mismatch` | `errors.tls.name_mismatch` | expected and presented names only in technical details, where the transport distinguishes them (P1-IN-01); otherwise `tls_untrusted` |
| `auth_failed` | `errors.auth.credentials_or_directory` | `auth_method` |
| `token_not_authorized` | `errors.auth.token_invalid_or_disabled` | — |
| `token_expired` | `errors.auth.token_expired` | — |
| `mfa_session_terminated` | `errors.auth.mfa_token_required` | — |
| `system_access_denied` | `errors.permission.system_access` | — |
| `api_method_denied` | `errors.permission.api_method` | `method:technical` |
| `unsupported_version` | `errors.compatibility.version` | `version:technical` |
| `archive_*`, `compression_ratio`, `entry_size`, `total_size`, `file_size`, `file_type`, `relative_path`, `duplicate_archive_entry` | `errors.import.policy` | `code`, counts/limits only |
| `yaml_depth`, `yaml_alias`, future XML DTD/entity/network codes | `errors.import.unsafe_structure` | `code` |
| `format_yaml`, `format_json`, `format_xml` | `errors.import.parse` | `format`, safe line/column if available |
| `duplicate_*`, `unknown_*`, `missing_role_slot`, `layout_bounds`, `layout_overlap`, `binary_item_value` | `errors.import.validation` | `code`, safe object path/count |
| `conflict` | `errors.publication.conflict` | `dashboard_name:user_data` |
| `unknown_outcome` | `errors.publication.unknown_outcome` | `operation_id:technical` |

English/Russian source copy for mapped errors:

| ID | English source | Русский |
|---|---|---|
| `errors.connection.dns_unresolvable` | The server name could not be resolved. Check the Zabbix address. | Не удалось определить адрес сервера. Проверьте адрес Zabbix. |
| `errors.connection.refused` | The server refused the connection. Check the address, port and service. | Сервер отклонил подключение. Проверьте адрес, порт и работу службы. |
| `errors.connection.unreachable_or_timeout` | The server is unreachable or did not respond in time. | Сервер недоступен или не ответил вовремя. |
| `errors.connection.api_not_found` | No Zabbix API was found at this address. Check the base path. | По этому адресу API Zabbix не найден. Проверьте базовый путь. |
| `errors.tls.untrusted` | The server certificate is not trusted. Add an approved CA certificate; TLS verification cannot be disabled. | Сертификат сервера не является доверенным. Добавьте разрешённый сертификат УЦ; проверку TLS отключить нельзя. |
| `errors.tls.name_mismatch` | The certificate was issued for a different name. Check the address or the certificate; TLS verification cannot be disabled. | Сертификат выдан для другого имени. Проверьте адрес или сертификат; отключить проверку TLS нельзя. |
| `errors.tls.time_invalid` | The server certificate is not valid for the current time. | Сертификат сервера недействителен по сроку. |
| `errors.auth.credentials_or_directory` | Sign-in failed. Check the credentials. For LDAP accounts, the directory may also be unavailable. | Не удалось войти. Проверьте учётные данные. Для учётной записи LDAP также может быть недоступен каталог. |
| `errors.auth.token_invalid_or_disabled` | The API token is invalid or disabled. | API-токен неверен или отключён. |
| `errors.auth.token_expired` | The API token has expired. | Срок действия API-токена истёк. |
| `errors.auth.mfa_token_required` | This MFA account cannot use password API sign-in. Use an API token allowed by your administrator. | Для этой учётной записи с MFA недоступен вход в API по паролю. Используйте API-токен, разрешённый администратором. |
| `errors.permission.system_access` | This account has no access to Zabbix. | У этой учётной записи нет доступа к Zabbix. |
| `errors.permission.api_method` | This account cannot call {method}. | Эта учётная запись не может вызвать метод {method}. |
| `errors.compatibility.version` | Zabbix {version} is not supported for this write operation. You can continue offline or read-only. | Zabbix {version} не поддерживается для этой операции записи. Можно продолжить автономно или в режиме только чтения. |
| `errors.import.policy` | The file was rejected by import safety policy ({code}). | Файл отклонён правилами безопасности импорта ({code}). |
| `errors.import.unsafe_structure` | The file contains a structure that is not allowed for safe import ({code}). | Файл содержит структуру, запрещённую для безопасного импорта ({code}). |
| `errors.import.parse` | The {format} file could not be parsed. | Не удалось разобрать файл {format}. |
| `errors.import.validation` | The imported data is inconsistent ({code}). | Импортируемые данные противоречивы ({code}). |
| `errors.publication.conflict` | The dashboard changed after preview. Build a new preview before writing. | Панель изменилась после предпросмотра. Перед записью создайте новый предпросмотр. |
| `errors.publication.unknown_outcome` | The write result is unknown. Do not retry it; check the dashboard first. | Результат записи неизвестен. Не повторяйте её; сначала проверьте панель. |

## 4. Glossary

| English source term | Russian term | Rule |
|---|---|---|
| dashboard | панель | Для global/template dashboard; тип уточняется контекстом |
| global dashboard | глобальная панель | Не «дашборд» в основной UI-копии |
| template dashboard | панель шаблона | В MVP unsupported for editing/publishing |
| widget | виджет | Официальный термин Zabbix |
| dashboard page | страница | Официальный термин Zabbix («Добавить страницу») |
| local draft | локальный черновик | Черновик называется своей сущностью: «панель» или «страница» |
| copy from server | копия с сервера | Происхождение рабочей копии снимка |
| project | — | В интерфейсе не используется (O-025); `project/v1` — технический формат, имя не показывается |
| read-only / read-write | только чтение / чтение-запись | Официальные термины Zabbix; только для режима подключения |
| editing not supported | изменение не поддерживается | Объект, который ZDashstitch сохраняет без изменений; не «только чтение» (P1-00E) |
| panel (UI region) | область, боковая колонка, инспектор | Не «панель»: это слово закреплено за dashboard |
| host | узел сети | Официальный термин Zabbix; server hostname остаётся без перевода |
| item | элемент данных | API field/method `item`, `itemid`, `item.get` не переводятся |
| trigger | триггер | — |
| template | шаблон | — |
| host group | группа узлов сети | — |
| item key | ключ элемента данных | Сам `key_` не переводится |
| binding | привязка | Реальная связь logical slot → host/item |
| slot | слот | Логический `slot_id` не переводится |
| working copy | рабочая копия | Не server dashboard до публикации |
| snapshot | снимок | Не обещает восстановление |
| preview | предпросмотр | Не результат записи |
| read-back | повторное чтение | Проверка после записи |
| conflict | конфликт | Base/Local/Remote diverged |
| unknown outcome | неизвестный результат | Не «ошибка публикации» и не «успех» |
| Save as copy | сохранить копию | UI action; новая private dashboard |

Always untranslated: `Zabbix`, `ZDashstitch`, `YAML`, `JSON`, `XML`, `ZIP`, `API`, `URL`, `TLS`, `CA`, `MFA`, `LDAP`, `LLD`, `HTTP`, `JSON-RPC`, method/field/type IDs, macros and item keys.

## 5. Long-text stress set

Reference sheets test without ellipsis-only access:

- Endpoint: `https://monitoring.example.invalid:8443/department/platform/zabbix/api_jsonrpc.php`.
- Dashboard name: `Regional database platform — PostgreSQL, ClickHouse, Keeper, HAProxy and operating-system diagnostics`.
- Russian name: `Региональная платформа баз данных — PostgreSQL, ClickHouse, Keeper, HAProxy и диагностика операционных систем`.
- Key: `vfs.file.regexp[/var/lib/example/application.log,"timeout|connection refused",,,skip]`.
- Macro: `{$ZDASHSTITCH_REFERENCE_WITH_A_VERY_LONG_NAME}`.
- Blocking RU copy: `Панель изменилась после предпросмотра; старый план нельзя опубликовать. Обновите исходные данные или сохраните отдельную личную копию.`
- Plural counts: `0, 1, 2, 5, 21, 101` objects/pages/widgets in both locales.

At 200% zoom and 320 CSS px equivalent width, labels wrap; critical buttons use complete verbs; full technical value remains copyable.

## 6. Acceptance checks

- [x] Required namespaces `common/startup/session/connection/import/library/dashboards/editor/bindings/clone/profiles/export/replace-host/publication/settings/diagnostics/errors` are covered.
- [x] All reference-pack components, states and P1 flows have source/RU IDs; accessible labels and technical details are included.
- [x] IN-01/03/05 codes map to message IDs without exposing raw prose or merging distinct safe states incorrectly.
- [x] Placeholders, plural/date/number policy, glossary and long-text set are defined.
- [ ] P2-00 generates runtime catalogs/types and mechanically proves EN/RU key and placeholder parity.
- [ ] Feature handoffs may add IDs, but may not rename these stable IDs or introduce displayed literals without versioning this inventory.

## 7. Сводный реестр message IDs (P1-11E, 25.10.2026; дополнен в проходе G1b)

Тексты EN/RU определены в спецификациях-источниках; здесь только реестр, чтобы ID не дублировались и P2-00 переносил их в каталоги `lang/` из одного списка. Вне этого файла определено 409 ID в 14 документах; вместе с 219 ID этого файла — 628 уникальных ID, дублей нет (проверка P1-11E). Строки таблиц команд API в спецификациях (`publication.preview`, `settings.get` и т. п.) — не сообщения и в реестр не входят. Найденные несоответствия и рекомендации по объединению — [`P1-11D-E-visual-l10n.md`](P1-11D-E-visual-l10n.md).

| Источник | Число | ID |
|---|---|---|
| [`P1-01A-entry-points.md`](P1-01A-entry-points.md) | 3 | `startup.dialog.waiting`, `startup.dialog.browser_fallback`, `startup.draft.missing` |
| [`P1-01B-information-architecture.md`](P1-01B-information-architecture.md) | 29 | `drafts.title`, `server.dashboards.title`, `draft.context.saved`, `draft.context.dirty`, `draft.context.offline`, `draft.context.source`, `draft.context.target`, `draft.nav.overview`, `draft.nav.structure`, `draft.nav.bindings`, `draft.nav.checks`, `draft.nav.publication`, `draft.structure.grid_help`, `draft.structure.dashboard_properties`, `draft.structure.page_period_default`, `draft.structure.widget_geometry`, `draft.structure.action.add_page`, `draft.structure.action.paste_page`, `draft.structure.action.move_widget`, `draft.structure.action.edit_widget`, `draft.structure.move_widget.no_space`, `draft.preview.title`, `draft.preview.screen_width`, `draft.preview.rows_shown`, `draft.preview.synthetic.toggle`, `draft.preview.synthetic.badge`, `draft.preview.synthetic.notice`, `draft.compat.label`, `draft.close.dirty.title` |
| [`P1-01C-session-states.md`](P1-01C-session-states.md) | 19 | `connection.state.connecting`, `connection.state.read_only_profile`, `connection.state.read_write_profile`, `connection.state.auth_required`, `connection.state.unreachable`, `connection.state.context_changed`, `draft.context.save_failed`, `draft.context.open_elsewhere`, `draft.action.edit_here`, `draft.conflict.revision`, `draft.freshness.not_checked`, `draft.freshness.current`, `draft.freshness.stale`, `draft.freshness.conflict`, `draft.freshness.source_missing`, `publication.state.idle`, `publication.state.submitting`, `publication.state.failed_before_send`, `publication.state.reconciled_failed` |
| [`P1-01D-offline-flow.md`](P1-01D-offline-flow.md) | 5 | `draft.export.needs_catalog`, `draft.metrics.no_catalog`, `draft.bindings.connect_to_resolve`, `publication.requirement.no_connection`, `drafts.action.export_file` |
| [`P1-01E-readonly-flow.md`](P1-01E-readonly-flow.md) | 2 | `draft.action.check_source`, `drafts.server_copy_export_hint` |
| [`P1-01F-readwrite-flow.md`](P1-01F-readwrite-flow.md) | 5 | `connection.action.set_read_only`, `publication.requirement.read_write`, `publication.requirement.connection`, `publication.operation.not_verified`, `publication.access.not_verified` |
| [`P1-01G-recovery.md`](P1-01G-recovery.md) | 18 | `session.action.end`, `session.end.not_confirmed`, `startup.recovery.close_unverified`, `startup.recovery.close_unverified.confirm`, `startup.recovery.closed_manually`, `session.end.confirm.title`, `session.end.confirm.body`, `session.end.unavailable_writing`, `session.ended.title`, `startup.recovery.body`, `startup.recovery.connect_first`, `startup.recovery.action.open_draft`, `publication.plan.reason.draft_changed`, `publication.plan.reason.target_changed`, `publication.plan.reason.connection_changed`, `publication.plan.reason.bindings_changed`, `publication.plan.reason.source_changed`, `publication.plan.reason.app_restarted` |
| [`P1-01H-flow-pack.md`](P1-01H-flow-pack.md) | 2 | `common.state.coming_later`, `editor.unavailable_yet` |
| [`P1-02-connections.md`](P1-02-connections.md) | 32 | `connection.list.subtitle`, `connection.list.empty`, `connection.action.new`, `connection.action.continue_offline`, `connection.state.connected_mode`, `connection.state.not_connected`, `connection.state.last_check_failed`, `connection.warning.http`, `connection.form.profile_name`, `connection.form.api_url`, `connection.form.server_changed.title`, `connection.form.server_changed.body`, `connection.form.password_not_stored`, `connection.form.store_token`, `connection.form.trust_system`, `connection.form.trust_custom_ca`, `connection.form.tls_no_bypass`, `connection.test.title`, `connection.test.step.endpoint`, `connection.test.step.api`, `connection.test.step.auth`, `connection.test.step.identity`, `connection.test.step.capabilities`, `connection.test.not_checked`, `connection.test.report.include_address`, `connection.test.report.copy`, `connection.enable_writes.title`, `connection.enable_writes.never`, `connection.enable_writes.duration`, `connection.enable_writes.no_extra_rights`, `connection.action.forget_token`, `connection.forget_token.not_revoked` |
| [`P1-03-import-library.md`](P1-03-import-library.md) | 67 | `drafts.subtitle`, `drafts.filter.any_origin`, `drafts.filter.server_copies`, `drafts.created_from_copy`, `drafts.action.make_portable`, `drafts.delete.safe`, `import.files.title`, `import.files.choose`, `import.files.upload`, `import.files.detected_as`, `import.files.becomes`, `import.files.kind.page`, `import.files.kind.package`, `import.files.kind.templates`, `import.files.kind.legacy`, `import.files.kind.unknown`, `import.files.legacy_choose`, `import.files.reimport_new`, `import.files.atomic`, `import.result.title`, `import.result.status.imported`, `import.result.status.partial`, `import.result.status.rejected`, `import.result.status.skipped`, `import.result.report_scope`, `server.tab.dashboards`, `server.tab.templates`, `server.subtitle`, `server.owner.you`, `server.owner.other`, `server.column.editable`, `server.column.mine`, `server.editable_count`, `server.visibility_note`, `server.action.copy`, `server.action.open_copy`, `server.actions_start_with_copy`, `server.templates.selected`, `server.templates.becomes`, `server.templates.new_version`, `server.templates.empty_export`, `server.templates.action.import`, `copy.title`, `copy.saved.snapshot`, `copy.saved.draft`, `copy.saved.access`, `copy.not_saved`, `copy.read_twice`, `copy.existing`, `library.section.profiles`, `library.catalog.source.export_file`, `library.catalog.versions`, `library.catalog.used_by`, `library.catalog.refresh`, `library.delete.in_use`, `workcopy.refetch.action`, `workcopy.refetch.summary`, `workcopy.refetch.no_silent_merge`, `workcopy.replay.applied`, `workcopy.replay.conflict`, `workcopy.replay.target_missing`, `workcopy.replay.keep_server`, `workcopy.replay.apply_mine`, `workcopy.replay.drop_edit`, `workcopy.base.accept`, `workcopy.base.revert`, `errors.import.inconsistent` |
| [`P1-05-bindings.md`](P1-05-bindings.md) | 52 | `bindings.step.scope`, `bindings.step.dependencies`, `bindings.step.new_host`, `bindings.step.targets`, `bindings.step.mapping`, `bindings.step.series`, `bindings.step.summary`, `bindings.scope.whole`, `bindings.scope.partial_warning`, `bindings.draft_only`, `bindings.ref.by_id`, `bindings.ref.host_name_pattern`, `bindings.ref.host_id`, `bindings.outcome.changes`, `bindings.outcome.kept`, `bindings.kept.title`, `bindings.kept.not_blocking`, `bindings.kept.confirm`, `bindings.kept.source_data`, `bindings.targets.order_hint`, `bindings.targets.new_slot`, `bindings.targets.instead_of`, `bindings.targets.templates_coverage`, `bindings.targets.filter_no_autoadd`, `bindings.targets.same_connection`, `bindings.mapping.accept_exact`, `bindings.mapping.candidates`, `bindings.mapping.keep_pattern`, `bindings.mapping.choose_lld_instance`, `bindings.mapping.leave_unresolved`, `bindings.mapping.similar_name_not_proof`, `bindings.clone.pattern.new`, `bindings.clone.pattern.as_is`, `bindings.clone.pattern.undecided`, `bindings.clone.keep_source`, `bindings.series.new_row`, `bindings.series.add_host`, `bindings.series.single_host_blocked`, `bindings.series.open_editor`, `bindings.state.checked`, `bindings.state.rechecked`, `bindings.state.stale`, `bindings.state.invalid`, `bindings.state.kept_source`, `bindings.action.check_all`, `bindings.action.choose_other`, `bindings.action.remove_slot`, `bindings.action.reset_all`, `bindings.local_only`, `clone.summary.title`, `clone.summary.whole_structure`, `clone.action.to_publication` |
| [`P1-08-publication.md`](P1-08-publication.md) | 84 | `publication.step.target`, `publication.step.checks`, `publication.step.changes`, `publication.step.confirm`, `publication.step.write`, `publication.context.source`, `publication.context.scope`, `publication.context.no_base`, `publication.expected.title`, `publication.target.create`, `publication.target.update_source`, `publication.target.choose_explicitly`, `publication.target.update_visible_to_all`, `publication.target.clone_no_update`, `publication.target.name`, `publication.target.name_free_visible`, `publication.target.name_server_check`, `publication.untouched.title`, `publication.access.title`, `publication.access.owner`, `publication.access.owner_super_admin_only`, `publication.access.private`, `publication.access.shared`, `publication.access.copy_source`, `publication.access.copy_partial`, `publication.access.restore_source`, `publication.access.add_recipient`, `publication.access.recipients_visible_only`, `publication.access.view_only`, `publication.access.partial_list.title`, `publication.access.partial_list.body`, `publication.access.public_all_users`, `publication.access.list_view_only`, `publication.access.self_write_removed`, `publication.check.group.connection`, `publication.check.group.source`, `publication.check.group.widgets`, `publication.check.group.bindings`, `publication.check.group.preservation`, `publication.check.fixed_in`, `publication.check.widget_kept_on_source`, `publication.check.confirm_kept`, `publication.check.action.confirm_kept`, `publication.check.summary.ready`, `publication.check.summary.plan_not_built`, `publication.check.recheck`, `publication.diff.tab.local`, `publication.diff.tab.remote`, `publication.diff.no_changes`, `publication.diff.filter.side_effects`, `publication.diff.filter.kept`, `publication.diff.change.field`, `publication.diff.change.kept`, `publication.diff.page_summary`, `publication.diff.unknown_fields_kept`, `publication.diff.matching_by_id`, `publication.conflict.overlap`, `publication.conflict.no_author`, `publication.conflict.action.reimport`, `publication.conflict.overwrite_unavailable`, `publication.confirm.api_action`, `publication.confirm.plan_once`, `publication.confirm.reread`, `publication.confirm.no_cancel_after_send`, `publication.action.confirm_write`, `publication.plan.invalidated`, `publication.stage.pre_read`, `publication.stage.write`, `publication.stage.read_back`, `publication.stage.structure`, `publication.stage.sources`, `publication.stage.display`, `publication.stage.display_not_checked`, `publication.stage.waiting`, `publication.result.base_advanced`, `publication.reconcile.found_one`, `publication.reconcile.found_none`, `publication.reconcile.ambiguous`, `publication.reconcile.manual`, `publication.reconcile.write_blocked`, `publication.action.open_in_zabbix`, `publication.action.open_journal`, `errors.publication.name_taken_maybe_hidden`, `errors.publication.owner_requires_super_admin` |
| [`P1-09B-09C-settings.md`](P1-09B-09C-settings.md) | 51 | `settings.general.title`, `settings.general.subtitle`, `settings.language.system`, `settings.language.current`, `settings.language.after_restart`, `settings.motion.title`, `settings.motion.reduce`, `settings.motion.system_state`, `settings.contrast.system_only`, `settings.formats.title`, `settings.theme.not_widgets`, `settings.data_not_translated`, `settings.file.title`, `settings.file.hint`, `settings.file.open`, `settings.file.open_folder`, `settings.file.copy_path`, `settings.file.reload`, `settings.file.format`, `settings.file.last_valid`, `settings.file.changed_outside`, `settings.file.invalid.title`, `settings.file.invalid.body`, `settings.file.invalid.location`, `settings.file.invalid.content_not_logged`, `settings.file.replace_with_valid`, `settings.file.invalid_kept`, `settings.file.session_only`, `settings.file.changed_externally.title`, `settings.file.changed_externally.reload`, `settings.file.changed_externally.keep_mine`, `settings.backups.title`, `settings.backups.latest_valid`, `settings.backups.restore`, `settings.transfer.title`, `settings.transfer.export`, `settings.transfer.import`, `settings.transfer.preview_changes`, `settings.transfer.storage_root_local`, `settings.reset.section`, `settings.reset.all`, `settings.reset.preview`, `settings.reset.no_data_removed`, `settings.locale.fallback`, `settings.locale.untranslated_count`, `errors.settings.invalid_value`, `errors.settings.unknown_key`, `errors.settings.path_not_absolute`, `errors.settings.storage_root_network`, `errors.settings.yaml_syntax`, `errors.settings.unreadable` |
| [`P1-12-about.md`](P1-12-about.md) | 41 | `about.title`, `about.subtitle`, `about.tagline`, `about.field.version`, `about.field.build`, `about.field.build_unknown`, `about.field.zabbix`, `about.field.zabbix_value`, `about.field.platform`, `about.license.title`, `about.license.code`, `about.license.code_choice`, `about.license.docs`, `about.license.text_mit`, `about.license.text_apache`, `about.license.text`, `about.license.zabbix_note`, `about.data.title`, `about.data.storage`, `about.data.settings_file`, `about.data.journal`, `about.data.copy_path`, `about.data.paths_local_only`, `about.report.copy`, `about.report.scope`, `about.copied`, `about.third_party.title`, `about.third_party.subtitle`, `about.third_party.filter.all`, `about.third_party.filter.icons`, `about.third_party.filter.rust`, `about.third_party.filter.ui`, `about.third_party.search`, `about.third_party.column.component`, `about.third_party.column.version`, `about.third_party.column.license`, `about.third_party.more`, `about.third_party.missing`, `about.docs.title`, `about.docs.embedded`, `about.docs.website` |
| [`P1-04A-E-editor.md`](P1-04A-E-editor.md) | 68 | `editor.page_title`, `editor.action.done`, `editor.zoom.in`, `editor.zoom.out`, `editor.zoom.level`, `editor.zoom.fit_width`, `editor.outline.title`, `editor.outline.order`, `editor.selection.summary`, `editor.selection.announce`, `editor.rows_used`, `editor.keyboard.hint`, `editor.page.empty`, `editor.page.unnamed`, `editor.page.duplicate_name`, `editor.page.last_cannot_delete`, `editor.page.delete.confirm`, `editor.page.all_pages`, `editor.capability.editable`, `editor.widget.needs_setup`, `editor.widget.needs_binding`, `editor.chooser.title`, `editor.chooser.search`, `editor.chooser.group.editable`, `editor.chooser.group.defaults`, `editor.chooser.group.defaults_hint`, `editor.chooser.group.copy_only`, `editor.chooser.group.copy_only_hint`, `editor.chooser.group.blueprints`, `editor.chooser.default_size`, `editor.chooser.place.first_free`, `editor.chooser.place.on_canvas`, `editor.chooser.no_shift`, `editor.chooser.no_space`, `editor.inspector.section.position`, `editor.inspector.section.basic`, `editor.inspector.section.data`, `editor.inspector.section.display`, `editor.inspector.section.axes`, `editor.inspector.section.period`, `editor.inspector.section.advanced`, `editor.inspector.section.unknown`, `editor.inspector.cells_hint`, `editor.inspector.cells_range`, `editor.inspector.hosts_in_bindings`, `editor.inspector.all_params`, `editor.inspector.local_only`, `editor.field.default_zabbix`, `editor.field.changed`, `editor.field.reset_default`, `editor.field.unknown_readonly`, `editor.field.invalid_interval`, `editor.dataset.add`, `editor.align.title`, `editor.distribute.title`, `editor.distribute.not_integer`, `editor.paste.title`, `editor.paste.source`, `editor.paste.place_taken`, `editor.paste.change_place`, `editor.paste.role_missing`, `editor.paste.add_role`, `editor.paste.keep_unbound`, `editor.paste.kept_as_is`, `editor.paste.format_unsupported`, `editor.paste.reference_renamed`, `editor.paste.undo_hint`, `editor.paste.action` |
| [`P1-04F-G-validation-history.md`](P1-04F-G-validation-history.md) | 59 | `checks.subtitle`, `checks.summary.save_always`, `checks.summary.export_ready`, `checks.summary.export_blocked`, `checks.summary.publication_ready`, `checks.summary.publication_blocked`, `checks.empty`, `checks.progress`, `checks.group.structure`, `checks.group.widgets`, `checks.group.sources`, `checks.group.references`, `checks.group.compatibility`, `checks.group.passed`, `checks.action.go_to`, `checks.action.fix_in_editor_later`, `checks.last_preflight.title`, `checks.last_preflight.none`, `checks.last_preflight.at`, `checks.object_path`, `checks.header_badge`, `validation.page_rows_overflow`, `validation.dashboard_no_pages`, `validation.page_empty`, `validation.name_empty`, `validation.name_too_long`, `validation.count_limit`, `validation.widget_incomplete`, `validation.widget_field_invalid`, `validation.unknown_metric`, `validation.catalog_missing`, `validation.source_binding_required`, `validation.reference_broken`, `validation.reference_duplicate`, `validation.reference_outside_page`, `validation.widget_type_unknown`, `validation.widget_field_unknown`, `validation.capability_not_verified`, `validation.graph_classic_unsupported`, `draft.context.saving`, `draft.autosave.retrying`, `draft.recovery.maybe_lost`, `draft.corrupted`, `history.reopened_hint`, `history.boundary`, `history.boundary.new_base`, `history.boundary.published`, `history.limit`, `editor.op.move`, `editor.op.resize`, `editor.op.align`, `editor.op.add_widget`, `editor.op.remove_widget`, `editor.op.duplicate`, `editor.op.paste`, `editor.op.edit_field`, `editor.op.page`, `editor.op.dashboard_properties`, `editor.op.bindings` |
| [`P1-04I-synthetic-preview.md`](P1-04I-synthetic-preview.md) | 15 | `draft.preview.synthetic.no_data`, `draft.preview.synthetic.no_data.reason.no_metric`, `draft.preview.synthetic.no_data.reason.no_value_type`, `draft.preview.synthetic.no_data.reason.pattern_no_match`, `draft.preview.synthetic.no_data.reason.unsupported_type`, `draft.preview.synthetic.a11y`, `draft.preview.synthetic.sample_text`, `draft.preview.synthetic.threshold`, `draft.preview.synthetic.basis.title`, `draft.preview.synthetic.basis.value_type_units`, `draft.preview.synthetic.basis.value_map`, `draft.preview.synthetic.basis.template_default`, `draft.preview.card.no_data`, `draft.preview.period`, `draft.preview.legend.title` |
| [`P1-07-export.md`](P1-07-export.md) | 74 | `export.step.goal`, `export.step.review`, `export.step.save`, `export.goal.title`, `export.goal.internal`, `export.goal.internal.hint`, `export.goal.external`, `export.goal.external.hint`, `export.goal.private`, `export.goal.private.hint`, `export.contents.included`, `export.contents.never`, `export.contents.never.secrets`, `export.contents.never.bindings`, `export.contents.never.local_state`, `export.contents.never.sources`, `export.risk.title`, `export.risk.body`, `export.compat.computed`, `export.compat.hint`, `export.scan.local_only`, `export.scan.confidence.exact`, `export.scan.confidence.possible`, `export.scan.confidence.unchecked`, `export.scan.class.host`, `export.scan.class.host_mask`, `export.scan.class.url`, `export.scan.class.environment`, `export.scan.class.key_part`, `export.scan.class.unknown_field`, `export.scan.no_full_values`, `export.replace.title`, `export.replace.role`, `export.replace.role.hint`, `export.replace.alias`, `export.replace.keep`, `export.replace.remove_widget`, `export.replace.key_keep_only`, `export.replace.before_after`, `export.replace.apply_all`, `export.replace.rescan`, `export.replace.draft_unchanged`, `export.ack`, `export.stage.scan`, `export.stage.transform`, `export.stage.validate`, `export.stage.package`, `export.stage.verify`, `export.result.mode_depersonalised`, `export.result.accepted_warnings`, `export.result.not_sent`, `export.action.open_folder`, `export.action.copy_report`, `export.error.destination_unavailable`, `export.error.destination_exists`, `export.error.disk_full`, `export.error.server_id_found`, `convert.title`, `convert.subtitle`, `convert.root.dashboard`, `convert.root.page`, `convert.new_name`, `convert.provenance`, `convert.report.title`, `convert.report.hosts`, `convert.report.items`, `convert.report.lld`, `convert.report.no_catalog`, `convert.report.classic_graphs`, `convert.report.masks`, `convert.report.unknown`, `convert.report.not_transferred`, `convert.report.nothing_silent`, `convert.action.create` |
| [`P1-09D-H-settings.md`](P1-09D-H-settings.md) | 87 | `settings.workspace.title`, `settings.workspace.subtitle`, `settings.storage.root`, `settings.storage.change`, `settings.storage.part.drafts`, `settings.storage.part.workcopies`, `settings.storage.part.snapshots`, `settings.storage.part.journal`, `settings.storage.part.library`, `settings.storage.used_free`, `settings.storage.low_space`, `settings.storage.state.available`, `settings.storage.state.unavailable`, `settings.storage.state.read_only`, `settings.storage.move.title`, `settings.storage.move.checks`, `settings.storage.move.option_move`, `settings.storage.move.option_move_hint`, `settings.storage.move.option_empty`, `settings.storage.move.option_existing`, `settings.storage.move.previous_kept`, `settings.storage.move.save_all`, `settings.storage.move.progress`, `settings.storage.move.verified`, `settings.storage.move.action`, `settings.storage.previous.delete`, `settings.storage.cleanup`, `settings.storage.cleanup.never`, `settings.storage.unavailable.banner`, `settings.storage.unavailable.choose_other`, `settings.autosave.title`, `settings.autosave.max_interval`, `settings.autosave.off_warning`, `settings.editor.default_zoom`, `settings.editor.zoom.fit_width`, `settings.editor.zoom.actual`, `settings.editor.show_grid`, `settings.editor.show_rulers`, `settings.confirm.title`, `settings.confirm.delete_widget`, `settings.confirm.delete_page`, `settings.confirm.bulk_apply`, `settings.confirm.locked`, `settings.connections.title`, `settings.connections.edit_in_profile`, `settings.credentials.os_store`, `settings.credentials.memory_only`, `settings.credentials.password_not_saved`, `settings.credentials.store_unavailable`, `settings.credentials.store_locked`, `settings.credentials.entry_missing`, `settings.trust.title`, `settings.trust.expiring`, `settings.trust.used_by`, `settings.trust.remove`, `settings.trust.remove.consequence`, `settings.security.title`, `settings.security.session`, `settings.security.session_hint`, `settings.export.default_directory`, `settings.export.ask_each_time`, `settings.export.on_existing`, `settings.export.on_existing.ask`, `settings.export.on_existing.new_name`, `settings.export.never_silent_overwrite`, `settings.export.scan_always`, `settings.privacy.title`, `settings.privacy.no_telemetry`, `settings.diagnostics.level`, `settings.diagnostics.level.normal`, `settings.diagnostics.level.detailed`, `settings.diagnostics.retention`, `settings.diagnostics.bundle.title`, `settings.diagnostics.bundle.create`, `settings.diagnostics.bundle.includes`, `settings.diagnostics.bundle.excludes`, `settings.diagnostics.bundle.include_addresses`, `settings.diagnostics.bundle.local_only`, `settings.diagnostics.bundle.save`, `errors.storage.linked`, `errors.storage.not_writable`, `errors.storage.nested`, `errors.storage.not_empty`, `errors.storage.no_space`, `errors.storage.blocked_journal`, `errors.storage.move_verify_failed`, `errors.storage.unavailable` |
| [`P1-10-motion.md`](P1-10-motion.md) | 2 | `settings.appearance.mascot`, `settings.appearance.mascot.hint` |
| P1-00F §8 «Общие подписи» (P1-11H) | 7 | `common.action.next_step`, `common.action.save_draft`, `common.filter.with_count`, `common.filter.all`, `publication.action.open_confirm`, `publication.access.permission.read`, `publication.access.permission.read_write` |

## 8. Общие подписи интерфейса (P1-11H, 25.10.2026)

Подписи, которые встречаются на макетах G1a без собственного ID; `nav.*` — пункты глобальной шапки (P1-11 G1b, G-02: нужны для P2-08). Заголовки колонок таблиц и разделов экранов получают ID в пространстве имён своего экрана (`server.column.*`, `bindings.column.*`, `publication.column.*`, `import.column.*`, `drafts.column.*`, `library.column.*`, `settings.section.*`) при реализации экрана; текст берётся из принятого макета, правило — P1-11E.

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `nav.start` | Start | Начало | — |
| `nav.drafts` | Drafts | Черновики | — |
| `nav.server` | Dashboards in Zabbix | Панели в Zabbix | — |
| `nav.library` | Library | Библиотека | — |
| `nav.connections` | Connections | Подключения | — |
| `nav.settings` | Settings | Настройки | — |
| `settings.search.placeholder` | Search settings | Поиск настроек | — |
| `settings.nav.no_secrets` | Passwords and tokens are not stored or shown here. | Пароли и токены здесь не хранятся и не показываются. | — |
| `common.action.next_step` | Next: {step} | Далее: {step} | `step:message_id` |
| `common.action.save_draft` | Save draft | Сохранить черновик | — |
| `common.filter.with_count` | {label} {count} | {label} {count} | `label:message_id`, `count:integer` |
| `common.filter.all` | All | Все | — |
| `publication.action.open_confirm` | Confirm write… | Подтвердить запись… | — |
| `publication.access.permission.read` | Read-only | Только чтение | — |
| `publication.access.permission.read_write` | Read-write | Чтение-запись | — |

`publication.access.permission.*` — официальные термины прав доступа к панели в Zabbix; рядом всегда имя получателя, поэтому они не смешиваются с режимом подключения (P1-08I).

## 9. Подписи оболочки P2-08 (25.10.2026)

Тексты, которых не хватило при реализации P2-08 (оболочка, «Начало», «Черновики», «Обзор», «Структура» для просмотра, «Проверки»). Русский текст — из принятых макетов 01-01, 02-01, 07-01, 08-01; английский — исходный. Восемь ID, созданных при реализации P2-08 по правилу §8 (`drafts.column.name/kind/origin/source/state`, `drafts.filter.dashboards/pages/local`), подтверждены с английским текстом «Name / Type / Origin / Source / State», «Dashboards / Pages / Local». Состояние в списке «Черновиков» показывается короткими подписями §9.2 (`drafts.state.*`) и `draft.freshness.not_checked`; длинные `draft.freshness.*` — в сведениях и «Обзоре».

### 9.1 «Начало» (01-01)

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `startup.kicker` | Getting started | Начало работы | — |
| `startup.intro` | Local drafts do not need a connection. Changes on the server are made only through a verified plan. | Локальные черновики не требуют подключения. Изменения на сервере — только через проверенный план. | — |
| `startup.local.badge` | No connection needed | Без подключения | — |
| `startup.import.help` | Dashboards, pages, template exports; several files at once | Панели, страницы, экспорт шаблонов; можно несколько файлов сразу | — |

### 9.2 Происхождение и состояние черновика («Начало», «Черновики»)

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `draft.provenance.assembled` | {count, plural, one {Built from # page} other {Built from # pages}}{sources, select, other {: {sources}}} | {count, plural, one {собрана из # страницы} other {собрана из # страниц}}{sources, select, other {: {sources}}} | `count:integer`, `sources?:text` |
| `draft.provenance.taken_from` | Taken from dashboard “{dashboard}” | взята из панели «{dashboard}» | `dashboard:text` |
| `draft.provenance.page_of` | “{dashboard}”, page {page} | «{dashboard}», стр. {page} | `dashboard:text`, `page:integer` |
| `draft.provenance.imported_file` | Imported from a file | импортирована из файла | — |
| `draft.provenance.page_file` | Page file | файл страницы | — |
| `draft.provenance.legacy_file` | Old-format file | файл старого формата | — |
| `draft.provenance.created_here` | Created here | создана здесь | — |
| `draft.provenance.snapshot` | {connection} · {time} | {connection} · {time} | `connection:text`, `time:datetime` |
| `draft.state.modified_locally` | Changed locally | изменена локально | — |
| `draft.state.never_published` | Not published | не публиковалась | — |
| `drafts.state.saved` | Saved | Сохранена | — |
| `drafts.state.modified` | Changed | Изменена | — |
| `drafts.state.changed_on_server` | Changed on server | Изменена на сервере | — |
| `drafts.state.current` | Checked | Проверена | — |
| `drafts.state.stale` | Out of date | Устарела | — |
| `drafts.state.source_missing` | Source missing | Источник не найден | — |

Короткие подписи свежести (дополнение 26.10.2026): `drafts.state.current/stale/source_missing` — для колонки «Состояние» таблицы «Черновиков» по образцу `drafts.state.changed_on_server` (для `conflict`) и `draft.freshness.not_checked`; время проверки в короткую подпись не входит (оно в `draft.freshness.current` в сведениях и «Обзоре»). Термины «Проверена», «Устарела» и «Источник не найден» согласованы с длинными `draft.freshness.*`.

### 9.3 «Черновики» (02-01): поиск, создание, сведения

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `drafts.search.placeholder` | Search by name | Поиск по имени | — |
| `drafts.action.create` | Create | Создать | — |
| `drafts.sort_hint` | Sorted by last change | Сортировка — по времени изменения | — |
| `drafts.details.source` | Source | Источник | — |
| `drafts.details.connection` | Connection | Подключение | — |
| `drafts.details.base` | Base | База | — |
| `drafts.details.base_snapshot` | Snapshot {time} | снимок {time} | `time:datetime` |
| `drafts.details.freshness` | Freshness | Свежесть | — |
| `drafts.details.changed_on_server_at` | Changed on server ({time}) | изменена на сервере ({time}) | `time:datetime` |
| `drafts.details.local` | Locally | Локально | — |
| `drafts.details.created_item` | “{name}” — created {time} | «{name}» — создана {time} | `name:text`, `time:datetime` |
| `common.action.open` | Open | Открыть | — |
| `common.action.duplicate` | Duplicate | Дублировать | — |
| `common.action.rename` | Rename | Переименовать | — |
| `common.action.delete` | Delete | Удалить | — |

### 9.4 Страницы, свойства панели, длительности

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `object.page.untitled` | Untitled | Без названия | — |
| `object.page.numbered` | {number}. {name} | {number}. {name} | `number:integer`, `name:text` |
| `object.page.widget_count` | {count, plural, one {# widget} other {# widgets}} | {count, plural, one {# виджет} few {# виджета} many {# виджетов} other {# виджета}} | `count:integer` |
| `draft.structure.page_summary` | {widgets} · {period} | {widgets} · {period} | `widgets:message_id`, `period:message_id` |
| `draft.structure.dashboard_properties_summary` | Default page display period: {period} · slideshow {slideshow} | Период страницы по умолчанию: {period} · слайд-шоу {slideshow} | `period:message_id`, `slideshow:message_id` |
| `draft.structure.slideshow.on` | on | вкл. | — |
| `draft.structure.slideshow.off` | off | выкл. | — |
| `common.duration.seconds` | {count, plural, one {# second} other {# seconds}} | {count, plural, one {# секунда} few {# секунды} many {# секунд} other {# секунды}} | `count:integer` |
| `common.duration.minutes` | {count, plural, one {# minute} other {# minutes}} | {count, plural, one {# минута} few {# минуты} many {# минут} other {# минуты}} | `count:integer` |
| `common.duration.hours` | {count, plural, one {# hour} other {# hours}} | {count, plural, one {# час} few {# часа} many {# часов} other {# часа}} | `count:integer` |

### 9.5 «Обзор» (07-01)

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `draft.overview.origin` | Origin | Происхождение | — |
| `draft.overview.snapshot_from` | snapshot of {date} | снимок от {date} | `date:date` |
| `draft.overview.grid_rows_used` | {rows, plural, one {72-column grid, # row used} other {72-column grid, # rows used}} | {rows, plural, one {Сетка 72 колонки, занята # строка} few {Сетка 72 колонки, занято # строки} many {Сетка 72 колонки, занято # строк} other {Сетка 72 колонки, занято # строки}} | `rows:integer` |
| `draft.overview.local_changes_note` | Local changes do not change the dashboard on the server. | Локальные изменения не меняют панель на сервере. | — |
| `draft.overview.source` | Source | Источник | — |
| `draft.overview.snapshot` | Snapshot | Снимок | — |
| `draft.overview.needs_connection` | needs a connection | нужно подключение | — |
| `draft.overview.target` | Target | Назначение | — |
| `draft.overview.target.none` | No target dashboard selected | Целевая панель не выбрана | — |
| `draft.overview.target.independent` | Source and target are set independently. | Источник и назначение задаются независимо. | — |
| `draft.overview.composition.page` | Page contents | Состав страницы | — |
| `draft.overview.composition.dashboard` | Dashboard contents | Состав панели | — |
| `draft.overview.composition.help` | Sizes are in Zabbix grid cells. Order and moves are in “Structure”. | Размеры — в клетках сетки Zabbix. Порядок и перенос — в разделе «Структура». | — |
| `draft.overview.publish_unavailable` | Unavailable: no target selected, no checks run. | Недоступно: не выбрана цель, нет проверки. | — |
| `draft.action.save_local` | Save locally | Сохранить локально | — |
| `draft.overview.widget_size` | {type} · {width} × {height} | {type} · {width} × {height} | `type:message_id`, `width:integer`, `height:integer` |
| `draft.overview.widget_host` | host: {host} | узел сети: {host} | `host:text` |
| `draft.overview.edit_unsupported_note` | “Editing not supported” means ZDashstitch does not change this type yet; the widget is kept unchanged and shown in the preview. | «Изменение не поддерживается» — ZDashstitch пока не изменяет этот тип; виджет сохраняется без изменений и показывается в предпросмотре. | — |
| `draft.overview.selection_note` | Selected widgets define the scope, for example “Add widgets to page”. | Выбранные виджеты задают область действия, например «Добавить виджеты на страницу». | — |
| `draft.overview.next_steps` | Next steps | Следующие шаги | — |
| `draft.overview.step.numbered` | {number}. {step} | {number}. {step} | `number:integer`, `step:message_id` |
| `draft.overview.step.choose_target` | Choose a target dashboard | Выберите целевую панель | — |
| `draft.overview.step.choose_target.help` | Needed for “Add page to dashboard”. | Нужна для «Добавить страницу в панель». | — |
| `draft.overview.step.check_bindings` | Check bindings | Проверьте привязки | — |
| `draft.overview.step.check_bindings.help` | Hosts, items and permissions. | Узлы сети, элементы данных и права. | — |
| `draft.overview.step.review_plan` | Review the plan | Посмотрите план | — |
| `draft.overview.step.review_plan.help` | Writing only by a verified plan. | Запись только по проверенному плану. | — |

Примечания (26.10.2026, найдено при P2-08):

- `draft.overview.publish_unavailable` — причина недоступности шага публикации в «Обзоре» (макет 07-01). По P1-01H §6 до P2-04 такие входы показывают «Появится позже» (`common.state.coming_later`), поэтому в MVP-оболочке сейчас **не показывается**; ID зарезервирован: когда после P2-04 шаг публикации в «Обзоре» станет включаемым, ему нужен текст причины. Если причина одна, показывается не этот объединённый текст, а точная причина (`publication.requirement.no_connection`, `publication.requirement.read_write` и т. п.); объединённый текст — для случая, когда не выполнены оба условия, как на макете.
- `draft.action.save_local` — подпись кнопки «Сохранить локально» в «Обзоре» (макет 07-01) и в диалоге закрытия (D7 в P1-01D). Не путать с `common.action.save_draft` («Сохранить черновик», редактор и «Проверки», P1-04F): для одного и того же действия в каждом месте сохраняется подпись своего макета. **Не подключена**: хранилища черновиков ещё нет, подключается в P2-05D/P4.

### 9.6 Уровни находок («Проверки», P1-04F §2.1)

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `checks.level.warning` | Warning | Предупреждение | — |
| `checks.level.blocks_export` | Blocks export | Блокирует экспорт | — |
| `checks.level.blocks_publication` | Blocks publication | Блокирует публикацию | — |
| `checks.level.blocks_both` | Blocks export and publication | Блокирует экспорт и публикацию | — |

### 9.7 Ориентиры для вспомогательных технологий

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `shell.a11y.main_nav` | Main navigation | Основная навигация | — |
| `shell.a11y.breadcrumbs` | Breadcrumbs | Навигационная цепочка | — |
| `draft.a11y.sections` | Draft sections | Разделы черновика | — |

### 9.8 Типы виджетов Zabbix 7.4

| ID | English source | Русский | Parameters |
|---|---|---|---|
| `widget.type.actionlog` | Action log | Журнал действий | — |
| `widget.type.clock` | Clock | Часы | — |
| `widget.type.discovery` | Discovery status | Состояние обнаружения | — |
| `widget.type.favgraphs` | Favorite graphs | Избранные графики | — |
| `widget.type.favmaps` | Favorite maps | Избранные карты | — |
| `widget.type.gauge` | Gauge | Радиальный датчик | — |
| `widget.type.geomap` | Geomap | Геокарта | — |
| `widget.type.graph` | Graph (classic) | График (классический) | — |
| `widget.type.graphprototype` | Graph prototype | Прототип графиков | — |
| `widget.type.honeycomb` | Honeycomb | Сотовый график | — |
| `widget.type.hostavail` | Host availability | Доступность узлов сети | — |
| `widget.type.hostcard` | Host card | Описание хоста | — |
| `widget.type.hostnavigator` | Host navigator | Навигатор по узлам сети | — |
| `widget.type.item` | Item value | Значение элемента данных | — |
| `widget.type.itemcard` | Item card | Карточка элемента данных | — |
| `widget.type.itemhistory` | Item history | История элементов данных | — |
| `widget.type.itemnavigator` | Item navigator | Навигатор по элементам данных | — |
| `widget.type.map` | Map | Карта сети | — |
| `widget.type.navtree` | Map navigation tree | Дерево навигации карт сетей | — |
| `widget.type.piechart` | Pie chart | Круговая диаграмма | — |
| `widget.type.problemhosts` | Problem hosts | Узлы сети с проблемами | — |
| `widget.type.problems` | Problems | Проблемы | — |
| `widget.type.problemsbysv` | Problems by severity | Проблемы по важности | — |
| `widget.type.slareport` | SLA report | Отчет SLA | — |
| `widget.type.svggraph` | Graph | График | — |
| `widget.type.systeminfo` | System information | Информация о системе | — |
| `widget.type.tophosts` | Top hosts | Топ узлов сети | — |
| `widget.type.topitems` | Top items | Часто используемые элементы | — |
| `widget.type.toptriggers` | Top triggers | Топ триггеров | — |
| `widget.type.trigover` | Trigger overview | Обзор триггеров | — |
| `widget.type.url` | URL | URL | — |
| `widget.type.web` | Web monitoring | Веб-мониторинг | — |

Английские названия типов — поле `name` в `manifest.json` каждого виджета лабораторного Zabbix 7.4.16 (сверено 25.10.2026, 32 типа); русские — официальная локализация (`frontend.mo`, `backend_inputs_ru.md`, таблицы A и B). Единственное отступление: у `topitems` в официальном переводе опечатка («эелемнты»), в интерфейсе — «Часто используемые элементы». Тип, которого нет в списке (неизвестный или новее 7.4), показывается техническим именем `type` моноширинным шрифтом с подписью «Изменение не поддерживается»; выдуманное название не подставляется.

Решения по вопросам P2-08:

- **`/server` без подключения.** Пункт шапки «Панели в Zabbix» ведёт на `/server`: при активном подключении — переход на `/server/:connection_local_id/dashboards`, без него — пустое состояние `connection.state.not_connected` с действием `startup.action.connect` (→ `/connections`). Маршрут внесён в P1-01B §3.
- **`draft.preview.screen_width`.** Параметр `width` — технический (`1920 px`, без разделителя разрядов): это фиксированная ширина экрана из набора Zabbix, а не измеряемая величина.
- **Значки шапки.** Значки Tabler outline подключаются при сборке как локальные SVG (файл из `assets/icons/tabler/outline/` встраивается сборкой Vite, без загрузки по сети и без внешних ссылок, совместимо с CSP); каждый использованный значок — строка в `assets/icons/THIRD_PARTY.md`. Это задача реализации P2-08, не изменение дизайна.
