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
| `common.state.verified` | Verified by reading from Zabbix | Проверено повторным чтением из Zabbix | — |
| `common.state.unknown_outcome` | Result not yet known | Результат пока неизвестен | — |
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
| `library.title` | Local library | Локальная библиотека | — |
| `library.section.snapshots` | Dashboard and page snapshots | Снимки панелей и страниц | — |
| `library.section.pages` | Pages | Страницы | — |
| `library.section.catalogs` | Metric catalogs | Каталоги метрик | — |
| `library.section.blueprints` | Widget blueprints | Заготовки виджетов | — |
| `library.empty` | No local objects yet | Локальных объектов пока нет | — |
| `dashboards.title` | Global dashboards | Глобальные панели | — |
| `dashboards.action.import_snapshot` | Import as working copy | Импортировать как рабочую копию | — |
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
| `editor.canvas.label` | Dashboard layout, 72 by 64 cells | Макет панели, 72 на 64 ячейки | — |
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
| `bindings.fitness.read_only` | Understood but read-only | Распознано, но доступно только для чтения | — |
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
| `clone.notice.private` | The new dashboard will be private. The original will not be changed. | Новая панель будет личной. Исходная панель не изменится. | — |
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
| `settings.appearance.title` | Appearance | Оформление | — |
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
| read-only / read-write | только чтение / чтение-запись | Официальные термины Zabbix |
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
