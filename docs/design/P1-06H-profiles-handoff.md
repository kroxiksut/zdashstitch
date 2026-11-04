# P1-06H — профили оформления и заготовки виджетов: сводный handoff для реализации

Версия handoff: **1.1 от 26.10.2026** (эпик P1-06, решение O-012 — вариант Б, пункты 1–5; вопросы Q1–Q19 §13 и решения В-14…В-18 приняты владельцем 26.10.2026 вечером). Это design handoff: он консолидирует принятые в P1-06A–G решения в один документ для задач реализации и **не является** реализованной функцией, схемой или контрактом записи. Схемы двух новых форматов добавляются только по `docs/DOC_GOVERNANCE.md` (§8); лабораторные контракты записи полей `item`/`svggraph` — существующие (P0-06, P0-09), профиль к ним ничего не добавляет.

Изменения:

- **1.1 (26.10.2026, DESIGN-FIX-01 / DF-A)** — по ревью `REVIEW-2026-10-26-coherence.md`: B-04 (метрика заготовки в копии выбирается из каталога библиотеки, `metric_ref` в `resolve_copy`), B-05 (невыбранная метрика — только `source_binding_required`), M-14 (14-03: «3 → 5»), M-15 (`legend_lines_mode` — известное поле; неизвестное в макетах — `vendor_layout`), M-16 (метка у источника — неотменяемая служебная операция), M-17 (единые данные v3/v4 в 14-01/14-06/14-10/14-11/14-12), M-18 (`detach_profile {widget_refs[], fields[]?}`), M-19 (отвязка и «Убрать раздел» без подтверждения), M-20 (`profiles.compatibility.partial` про виджет; `profiles.compatibility.badge.*`), M-21 (два встроенных эталона, без «Zabbix по умолчанию»; заготовка-пример — «Загрузка и нагрузка CPU»), В-18 (сводка только в «Основном»), m-40…m-42 (внутренние расхождения, макеты, ID). §12 пересобран, §14 дополнен п. 10–16, §10 пересчитан (388 ID).
- **1.0 (26.10.2026)** — первая редакция.

Источники (нормативны для деталей, здесь — сводка и точки расхождения): [`P1-06A-B-profiles-model.md`](P1-06A-B-profiles-model.md) (термины, модель, матрица), [`P1-06C-D-library-extraction.md`](P1-06C-D-library-extraction.md) (библиотека, импорт/экспорт, извлечение), [`P1-06E-F-editor-insert.md`](P1-06E-F-editor-insert.md) (редактор профиля, вставка заготовки), [`P1-06G-bulk-apply.md`](P1-06G-bulk-apply.md) (применение, режимы, происхождение, копия с сервера). При расхождении между A–G действует более поздний документ, кроме случаев, перечисленных в §14 с предложенным решением.

## 1. Карта задач реализации

| Задача | Что берёт из эпика | Зависимости |
|---|---|---|
| **P4-05** (основная) | форматы §8, хранилище §7, библиотека §4, извлечение §5.1–§5.2, редактор профиля §5.3, совместимость §3 и применение §5.5–§5.6, происхождение значения §6, команды §9, тесты §11 | P4-04 (разделы библиотеки), P0-10 (общий вид `type: zabbix`, O-012 п. 5), P0-09 (реестр), P4-03 (операции редактора) |
| **P4-04** | разделы «Профили оформления» и «Заготовки виджетов», карточки, фильтры, состояния, «Импорт файлов» для двух форматов (§4) | P1-03 (библиотека, импорт) |
| **P4-03** | действия инспектора «Сохранить оформление как профиль…», «Создать заготовку…», «Применить профиль…», «Обновить из профиля…»; `editor.apply op=apply_profile`, `op=detach_profile`, `op=add_widget {blueprint_id}`; состояния поля §6; меню страницы и панели (§5.5) | P1-04 handoff |
| **P2-05** | операции рабочей копии `apply_style_profile` и `add_widget` с реальными ID; показ в «Мои изменения»; переигрывание (§6.3) | P1-03E, P1-08B |
| **P5-01 / P4-02** | блок «Ссылки без привязки» в «Привязках» копии с сервера; создание роли из параметра заготовки (§5.4) | P1-05 и дополнение |
| **P2-00** | 388 message IDs эпика (§10), изменение текстов `profiles.action.extract`, `profiles.action.apply`, `profiles.compatibility.partial` | P1-00F |
| **P4-06** | ничего нового: `editor.confirm_bulk_apply` уже в P1-09E; уточнение поведения (§5.6) | — |
| Схемы (`tools/build_schemas.py`, `schemas/`, `crates/core`) | `zdashstitch.style-profile/v1`, `zdashstitch.widget-blueprint/v1` с негативными fixtures (§8) | `DOC_GOVERNANCE`, ADR, `format_contract_v1_ru.md` |

Порядок: схемы и `core`-проверки → P4-04 (библиотека, импорт/экспорт) → P4-05 (извлечение, редактор, совместимость, применение) → P4-03 (действия редактора, вставка) → P2-05 / P5-01 (копия с сервера). Экспорт в файл до утверждения схем показывается «Появится позже» (P1-06A §4), остальное работает на внутреннем `/v0`.

## 2. Консолидированная модель

### 2.1 Сущности и инварианты

| Сущность | Идентичность | Переносится | Хранит | Никогда не хранит |
|---|---|---|---|---|
| Профиль оформления `StyleProfile` | `profile_id` (UUID v4), имя — данные | отдельный файл `zdashstitch.style-profile/v1` | разделы по типу (`svggraph`, `item`): поля классов «профиль»/«по выбору», палитра, пороги; имя, описание, теги, `version`, `adapter`, заявленная совместимость | источники, узлы, элементы, роли, метрики, время, агрегацию, фильтры, тексты, связи, геометрию, `rf_rate`, неизвестные поля, ID сервера, подключение, тип виджета как изменяемое |
| Заготовка виджета `WidgetBlueprint` | `blueprint_id` | `zdashstitch.widget-blueprint/v1` | один виджет `svggraph`/`item` в виде `project/v2` без положения, `default_size`, атрибуты, параметры (роли; метрики — только тип значения, единицы, описание), все поля-не-ссылки | положение, `reference`, `*._reference`, `or.*`, маски и имена узлов, реальные `hostid`/`itemid`, ключи элементов, каталоги и селекторы метрик (В-14), неизвестные поля, ID сервера |
| Экземпляр виджета | устойчивый ID виджета черновика / `widgetid` базы копии | в составе черновика | значения полей (после применения — обычные значения) | ссылку на профиль или заготовку |
| Запись применения (локально) | по виджету | **нет** | `{profile_id, profile_version, applied_at, fields{name → value}, overridden[]}` | — |

Инварианты (проверяются тестами §11):

1. `project/v2`, DATA-ONLY ZIP, `local-bindings/v1`, `deployment-state/v1` **не меняются** (O-012 п. 1, O-021).
2. Применение профиля — копирование значений одной операцией; черновик от профиля не зависит; изменение/удаление профиля черновиков не меняет.
3. Профиль меняет только поля матрицы P1-06B §8–§10 классов «профиль» и «по выбору»; никогда — источники, время, агрегацию, фильтры, тексты, связи, геометрию, `rf_rate`, неизвестные поля, тип виджета.
4. Заготовка создаёт виджет только типа с флагом `create` реестра; все ссылки на узлы и элементы — параметры или «нужна привязка»; метрика каталога выбирается при каждой вставке (в копии с сервера — из каталогов библиотеки); **ничего не подбирается автоматически**.
5. Профили и заготовки никогда не содержат ID сервера, имён и масок узлов, учётных данных; экспорт заготовки проходит сканер значений P1-07C.
6. Тема приложения и профиль не связаны (P1-09B).
7. Шаблоны Zabbix не изменяются и не превращаются в заготовки (AGENTS.md §3).

### 2.2 Порядок источников значения

`умолчание Zabbix → профиль (применённые поля) → локальная правка` (P1-06A §3.1). Состояния поля в инспекторе: «умолчание Zabbix», «изменено», «из профиля», «изменено после профиля» (P1-06G §6.1). Значение профиля, равное умолчанию, записывается как снятие поля (кодек, правило `svggraph` P1-IN-10).

## 3. Матрица совместимости (сводно)

### 3.1 Классы полей

| Тип | Класс «профиль» (включается по умолчанию) | Класс «по выбору» (O-012 п. 3; по умолчанию не извлекается, применяется с предупреждением) | «Никогда» |
|---|---|---|---|
| `svggraph` | `ds.N.type`, `ds.N.stacked`, `ds.N.width`, `ds.N.pointsize`, `ds.N.transparency`, `ds.N.fill`, `ds.N.axisy`; палитра (`ds.N.color` / `ds.N.color.M`) или `ds.N.color_palette`; `lefty`, `righty`, `axisx`; `legend`, `legend_statistic`, `legend_aggregation`, `legend_lines_mode`, `legend_lines`, `legend_columns`; `show_problems`, `graph_item_problems`, `simple_triggers`, `working_time`; `percentile_left`, `percentile_left_value`, `percentile_right`, `percentile_right_value` | `ds.N.missingdatafunc`; `lefty_min`, `lefty_max`, `righty_min`, `righty_max`; `lefty_units`, `righty_units`, `lefty_static_units`, `righty_static_units`; атрибут `view_mode` | `ds.N.dataset_type`, `ds.N.hosts.M`, `ds.N.items.M`, `ds.N.itemids.M`, `*._reference`, `or.*`, `ds.N.approximation`, `ds.N.aggregate_*`, `ds.N.timeshift`, `ds.N.data_set_label`, `source`, `time_period.*`, `problemhosts.N`, `severities.N`, `problem_name`, `evaltype`, `tags.N.*`, `override_hostid.*`, `reference`, имя, `rf_rate`, геометрия, неизвестные |
| `item` | `show` (обязательно, ≥ 1 пункт); `desc_h_pos`, `desc_v_pos`, `desc_size`, `desc_bold`, `desc_color`; `value_h_pos`, `value_v_pos`, `value_size`, `value_bold`, `value_color`; `decimal_places`, `decimal_size`; `units_show`, `units_pos`, `units_size`, `units_bold`, `units_color`; `time_h_pos`, `time_v_pos`, `time_size`, `time_bold`, `time_color`; `up_color`, `down_color`, `updown_color`; `bg_color`; `sparkline.width`, `sparkline.fill`, `sparkline.color` | `units`; пороги `thresholds[]` (пары «порог + цвет», все вместе) со справочным `thresholds_units`; атрибут `view_mode` | `itemid.N`, `override_hostid.*`, `description`, `aggregate_function`, `history`, `time_period.*`, `sparkline.history`, `sparkline.time_period.*`, `reference`, имя, `rf_rate`, геометрия, неизвестные |

Имена полей и условия включения зависимых полей сверяются по `WidgetForm.php` 7.4.16 и реестру P0-09 при реализации (пометки «сверить» в P1-06B §8–§9). `legend_lines_mode` — известное поле `svggraph` (реестр `crates/zabbix/src/widget_capabilities.rs` описывает `legend*` как известные целые/строковые поля); в макетах неизвестным полем служит `vendor_layout` (и `ds.0.vendor_hint`), которые ни один шаблон реестра не покрывает (M-15). Зависимости: `fill`/`width` ← `ds.N.type` ∈ {линия, ступени}; `pointsize` ← `type` = точки; `stacked` ← `type` ≠ точки; `legend_*` ← `legend` = 1; `graph_item_problems`/`simple_triggers`/`working_time` ← `show_problems` = 1; `lefty_*` ← `lefty` = 1 (аналогично `righty`); `desc_*`/`value_*`/`time_*`/`up_color`… /`sparkline.*` ← соответствующий пункт в `show`; `*_static_units` ← `*_units` = статически.

### 3.2 Уровни

| Уровень | Условие (P1-06B §7, P1-06G §3.1) | Применяется |
|---|---|---|
| `compatible` | тип с `edit` и в профиле; виджет и поля `known`; все включённые поля применимы | всё |
| `partial` | часть полей неприменима: зависимое при невключённом управляющем поле, поле виджета `unknown`, пороги с другими единицами, старый вид файла (`graph`/`item_value` → только `series.color`) | применимое подмножество; причины видны до подтверждения |
| `incompatible` | типа нет в профиле; у типа нет `edit` (все, кроме `item`, `svggraph` на 7.4); тип `unknown` | ничего; в сводке — «пропущен» с причиной |
| `unknown` | адаптер профиля ≠ адаптер черновика/подключения; файл новее приложения | ничего |

Уровень — свойство пары «профиль → виджет», не типа (M-20): полные подписи `profiles.compatibility.*` («…к этому виджету»), короткие плашки таблицы применения — `profiles.compatibility.badge.*` (P1-06A §7).

Исходы по полю: `apply`, `reset_default`, `unchanged`, `kept_local`, `skipped_dependent`, `skipped_unknown`, `skipped_units`, `skipped_legacy` (P1-06G §3.2). Виджет `partial` с нулём применимых полей в операцию не попадает.

### 3.3 Режимы применения

| Состояние поля | `replace` | `keep_local` | `merge` |
|---|---|---|---|
| умолчание Zabbix | профиль | профиль | профиль |
| изменено (записи нет) | профиль | остаётся | остаётся; **профиль**, если поле новое для профиля при повторном применении |
| из профиля | профиль | профиль | профиль |
| изменено после профиля | профиль | остаётся | остаётся |
| убрано из профиля | остаётся → «изменено» | то же | то же |

Умолчания: первое применение — `replace` (P1-06G, вопрос Q13), «Обновить из профиля» — `keep_local` (O-012 п. 2). Полные таблицы — P1-06G §5.

## 4. Библиотека (сводно)

- Разделы «Профили оформления», «Заготовки виджетов» (компоновка 04-01): фильтры по типу виджета и происхождению (`мой`/`встроенный`/`из файла`), поиск, сортировка, колонки и карточка — P1-06C §2–§3; плашки состояния «Не проверено» (`unknown`), «Ошибка в поле» (`invalid`), «Недоступен» (нечитаемый объект).
- Действия: «Изменить профиль» (→ редактор), «Дублировать» (новый ID, версия 1), «Переименовать», «Экспорт в файл…», «Удалить» (всегда; предупреждение об использовании; отказа `in_use` из P1-03H для профилей и заготовок нет); «Применить» и «Вставить» — только из редактора (P1-06C §4). Встроенных профилей два — эталоны P1-06A §12 «Рост БД — столбцы» и «Нагрузка CPU — линия» (В-17), только чтение; «Zabbix по умолчанию» нет; заготовка в MVP редактируется только именем/описанием (Q6).
- Импорт — через «Импорт файлов» с распознаванием по `format`; совпадение идентификатора → явный выбор «Заменить (новая версия)» / «Сохранить как копию»; другой адаптер → `unknown`; формат новее → отказ; нарушение схемы → отказ с кодом (§8.4); заготовка с масками/`or.*`/`_reference`/неизвестными полями → отказ (P1-06C §5).
- Экспорт — один файл YAML на объект, без локальных сведений; для заготовки — сканер P1-07C; имена по умолчанию `{slug}.style-profile.yaml` / `{slug}.widget-blueprint.yaml` (Q7); правила каталога и «не перезаписывать молча» — P1-09G.
- Версии: каждое сохранение полей профиля — `version + 1`; переименование, описание, теги и дублирование версию **не меняют** (резолюция §14 п. 1); замена при импорте — `max(local, file) + 1`.

## 5. Потоки (команды по шагам)

### 5.1 «Сохранить оформление как профиль» (P1-06D §8, 14-04)

1. `profiles.extract.inspect(widget_refs[])` → по типам: поля по классам с текущими значениями, исключённые зависимые с причиной, палитра, число «никогда», `legacy_view`, `datasets_differ`.
2. Пользователь: флажки (класс «профиль» включён, «по выбору» выключен), набор-источник, «Включать значения, равные умолчанию» (включён, Q8), имя (обязательно), описание, теги, назначение (новый профиль / раздел существующего).
3. `profiles.extract.save(...)` → атомарная запись, `profile_id`, `version` 1 (или `+1` у существующего при замене раздела), запись применения у источника (Q4) — неотменяемая служебная операция без шага истории (В-16: локально — запись в `profile-marks.json`; в копии — `apply_style_profile` без значений с `service: true`, которую «Отмена» пропускает и «Мои изменения» не показывают), тост с «Показать в библиотеке» и «Применить к другим виджетам…» (→ 5.5).

### 5.2 «Создать заготовку» (P1-06D §9, 14-05)

1. `blueprints.extract.inspect(widget_ref)` → источники с параметрами по умолчанию, препятствия с допустимыми решениями, предупреждения сканера, `default_size`, блоки «хранится / не хранится».
2. Пользователь: имена параметров (`^[a-z][a-z0-9_-]{1,63}$`, уникальны), объединение повторных ссылок, решение по каждому препятствию (ничего не отбрасывается молча), имя, описание.
3. `blueprints.extract.save(...)` → `blueprint_id`, версия 1; тост «Показать в библиотеке». Исходный виджет меток не получает.

### 5.3 Редактор профиля (P1-06E, 14-06/14-07)

`profiles.editor.open` → `profiles.form.describe(type)` (поля классов «профиль»/«по выбору» с умолчаниями, зависимостями, проверками, ID подписей) → правки с `profiles.validate(draft)` после каждого ввода → `profiles.save(profile_id?, draft, base_version)` (`version + 1`, пересчёт совместимости, `outdated` у использующих) → `profiles.preview.render(section_draft, sample_id)` для предпросмотра на встроенном примере (синтетика P1-04I, фиксированная светлая поверхность Zabbix). Профиль с ошибками не сохраняется (Q9); `version_conflict` → «Перезагрузить» / «Сохранить как новый». Единственное подтверждение редактора — закрытие с несохранёнными изменениями; «Убрать раздел» подтверждения не требует (обратимо до сохранения, строка «Вернуть раздел» — M-19, P1-00E §9).

### 5.4 Вставка заготовки (P1-06F, 14-08/14-09)

1. «Добавить виджет» → «Из заготовок» (кнопка «Далее: параметры» — `common.action.next_step` с `step` = `blueprints.insert.step.params`) → `blueprints.insert.inspect(blueprint_id, page, draft_kind)` → параметры с кандидатами: роли черновика по кардинальности (или «Создать роль…»), метрики каталогов по типу значения и единицам (`blueprints.insert.metrics.candidates`, статусы `exact`/`compatible`), «Оставить «нужна привязка»».
2. Копия с сервера (В-14): заготовка ключей не хранит, поэтому параметр-метрика сначала получает метрику из каталогов **библиотеки** (`blueprints.insert.metrics.candidates(param, scope: library)`; нет каталогов — `catalog_missing`, «нужна привязка»), параметр-роль — узлы подключения (09-02); затем `blueprints.insert.resolve_copy(param, metric_ref, hosts[])` ищет элемент на каждом узле по ключу выбранной метрики и сверяет тип значения и единицы (09-03 для `missing`/`ambiguous`/`denied`); смена метрики пересчитывает узлы. Ничего не подбирается автоматически.
3. Все параметры решены (без автовыбора) → `editor.apply op=add_widget {blueprint_id, params, new_roles[], place, size, name}` → виджет `type: zabbix` (O-012 п. 5), новый `reference`, один шаг `editor.op.insert_blueprint` (вместе с созданной ролью, Q10); в копии — операция `add_widget` с реальными ID внутри операции (ключ метрики в операции не хранится). «Нужна привязка» → **только** находка `source_binding_required`, блокирующая одну публикацию (O-034 п. 3; сохранение и экспорт возможны); `unknown_metric` не возникает, потому что метрика и каталог не указаны (B-05).

### 5.5 Применение профиля (P1-06G §2–§5, 14-10)

1. Точка входа (инспектор, меню страницы/панели, тост) → `profiles.compatibility(profile_id, scope, mode, force_thresholds)` → уровни, исходы по полям, «было → станет», итоги, предупреждения.
2. Окно: профиль (только `ok`), область (виджет / выбранные / страница / панель), режим, таблица виджетов, переключатель порогов; при `editor.confirm_bulk_apply = false` действия с известным профилем и областью идут без окна с «Отменить» в тосте (P1-06G §2.6, Q15).
3. `editor.apply op=apply_profile {...}` → одна операция, один шаг `editor.op.apply_profile`, записи применения обновлены; в копии — `apply_style_profile`.

### 5.6 «Обновить из профиля» (P1-06G §6.3–§6.4, 14-11)

Баннер устаревания в инспекторе (версия записи < версия библиотеки) → то же окно в режиме обновления: профиль фиксирован, область — виджеты с записью этого профиля (фильтр «только на прежней версии»), режим `keep_local` по умолчанию, строка «убрано из профиля» → `editor.apply op=apply_profile`. Профиль `invalid`/`unknown`/удалён — обновление недоступно с причиной.

### 5.7 Действия у поля и виджета (P1-06G §6.1–§6.2, 14-12)

«Вернуть умолчание» и «Вернуть значение профиля» — `op=edit_field`; «Отвязать от профиля» — `op=detach_profile {widget_refs[], fields[]?}`: с `fields` — одно поле, без — весь виджет (шаг истории `editor.op.detach_profile`, Q16; без подтверждения — M-19, P1-00E §9); «Вернуть все значения профиля» — `op=apply_profile {scope: {widget_refs}, mode: replace, from_record: true}`: значения из записи применения виджета, не из текущей версии профиля; запись не меняется. Сводка «Из профиля…» и действия виджета — только в разделе «Основное» (В-18); баннер устаревания — в любом разделе.

## 6. Поведение при конфликтах и изменениях

| Событие | Локальный черновик | Копия с сервера |
|---|---|---|
| Профиль сохранён новой версией / заменён при импорте | черновики не меняются; `outdated` у виджетов с записью (баннер инспектора, счётчик карточки) | то же (запись — в операции) |
| Профиль удалён | метки → «изменено» при открытии черновика (ленивый пересчёт); значения остаются | то же |
| Профиль переименован | метки показывают новое имя (`profile_id`) | то же; в операции хранится `profile_name_at_apply` только для журнала «Мои изменения» |
| Другой профиль поверх | первое применение нового; метки переходят; поля старого профиля вне нового → «изменено» (Q14) | то же |
| Экспорт/импорт черновика | записи применения отсутствуют у получателя → всё «изменено» | «Сделать переносимой» сохраняет записи у новой локальной панели |
| «Скопировать заново» (P1-03E) | — | `apply_style_profile` переигрывается по полям: конфликт по полю, «цель исчезла» по виджету, «уже совпадает» |
| Публикация | — | поля пишутся существующими кодеками; предварительные проверки P1-08C как для любых правок; ничего о профиле не публикуется |
| Отмена (Ctrl+Z) | один шаг: все виджеты области и записи | один шаг: снятие операции |
| Две вкладки | окно применения — только во вкладке, правящей черновик (P1-01C §3) | то же |

## 7. Хранилище (O-018)

Относительно корня хранилища (P1-09B); запись — временный файл → `fsync` → переименование; ни один файл не содержит ID сервера, узлов, элементов, подключения, секретов.

| Путь | Содержимое | Формат | Переносится |
|---|---|---|---|
| `library/style-profiles/<profile_id>.yaml` | переносимая часть профиля — байт в байт то, что даёт «Экспорт в файл…» | `zdashstitch.style-profile/v1` (до утверждения — `…/v0`, тот же состав полей) | да |
| `library/style-profiles/<profile_id>.meta.json` | локальная карточка: `origin` (`local`/`file`), `created_at`, `updated_at`, `imported_file_name` (только имя файла, без пути), `imported_at` | внутренний, `storage_scope: local_only` | нет |
| `library/widget-blueprints/<blueprint_id>.yaml` | переносимая часть заготовки | `zdashstitch.widget-blueprint/v1` | да |
| `library/widget-blueprints/<blueprint_id>.meta.json` | карточка: `origin`, время, `source_widget_name` (подсказка), `source_page_name` | внутренний | нет |
| `library/library-index.json` | производный индекс всех разделов библиотеки: строки списка §9.1 (включая `usage`/`inserted`); перестраивается при расхождении | внутренний | нет |
| встроенные профили | ресурсы приложения (не в хранилище), фиксированные `profile_id`, `origin: builtin`, только чтение; «Дублировать» копирует в `library/style-profiles/` | как файл профиля | — |
| `drafts/<local_id>/profile-marks.json` | записи применения: `{widget_id → {profile_id, profile_version, applied_at, fields{name → value}, overridden[name]}}` | внутренний, `local_only` | нет (не в `project/v2`, не в пакете) |
| `drafts/<local_id>/blueprint-inserts.json` | журнал вставок: `{blueprint_id → count}` (для счётчика «Вставлена») | внутренний | нет |
| рабочая копия (`workcopy/v0`, Base + операции) | операции `apply_style_profile {profile_id, profile_version, profile_name_at_apply, mode, service?: true, widgets[{widget_ref, fields[{name, before, after, outcome}], view_mode?}]}` (с `service: true` и пустым `fields` — метка у источника после извлечения: «Отмена» её пропускает, «Мои изменения» не показывают, В-16) и `add_widget {blueprint_id, ...}` с реальными ID внутри операции | O-018 | нет |

Счётчики `profiles.used_by` / `blueprints.inserted` считаются лениво по `profile-marks.json`, операциям копий и `blueprint-inserts.json` всех черновиков и кэшируются в индексе; удаление черновика уменьшает их при следующем пересчёте.

## 8. Форматы файлов (точный состав для JSON Schema)

Оба формата — YAML, читаются строгим загрузчиком `core` (без алиасов и дублей ключей, лимиты глубины/размера, как `project/v2`); неизвестные ключи запрещены на всех уровнях (`additionalProperties: false`); файл — недоверенные данные (P1-03G). Значения полей — примитивы Zabbix API: целые для полей типа 0, строки для типа 1. Диапазоны значений, специфичные для версии адаптера (например, `value_size` 1–100), проверяет `application` по описанию формы (`profiles.validate`), а не схема — иначе схема зависела бы от версии Zabbix (§14 п. 3).

### 8.1 `zdashstitch.style-profile/v1`

| Поле | Тип | Обязательно | Ограничения |
|---|---|---|---|
| `format` | string | да | const `zdashstitch.style-profile/v1` |
| `profile_id` | string | да | UUID v4 в нижнем регистре `^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$` |
| `name` | string | да | 1–120 символов, без управляющих символов, не только пробелы |
| `description` | string | нет | ≤ 2000 |
| `tags` | array[string] | нет | ≤ 20 элементов, каждый 1–40, уникальны без учёта регистра |
| `version` | integer | да | ≥ 1 |
| `adapter` | string | да | enum `["7.4"]` в v1 этой версии приложения; иное значение — не ошибка схемы, а состояние `unknown` при импорте (`adapter_unsupported`) — схема допускает `^[0-9]+\.[0-9]+$` |
| `declared_compatibility` | object | нет | `{minimum: "7.4.0", maximum?: "7.4.x"}` по O-029; `maximum ≥ minimum` |
| `widget_types` | object | да | ≥ 1 ключ; ключи enum `svggraph`, `item` |
| `widget_types.svggraph` | object | — | `widget_fields` (object), `dataset_fields` (object), `series_palette` (array[color], ≤ 64), `color_palette` (integer ≥ 0), `attributes` (object); хотя бы одно из `widget_fields`/`dataset_fields`/`series_palette`/`color_palette`/`attributes` непусто; `series_palette` и `color_palette` взаимоисключающие |
| `widget_types.svggraph.widget_fields` | object | — | ключи только из §3.1 «профиль»+«по выбору» уровня виджета; типы: integer для 0-полей, string для 1-полей (`lefty_min`, `lefty_max`, `righty_min`, `righty_max`, `lefty_static_units`, `righty_static_units`, `percentile_*_value`) |
| `widget_types.svggraph.dataset_fields` | object | — | ключи `type`, `stacked`, `width`, `pointsize`, `transparency`, `fill`, `axisy`, `missingdatafunc` — integer |
| `widget_types.item` | object | — | `widget_fields` (object), `thresholds` (array), `thresholds_units` (string, ≤ 32, только вместе с непустым `thresholds` — **Q11**), `attributes` (object); `widget_fields.show` обязателен: array[integer 1–5], ≥ 1, уникальны |
| `widget_types.item.widget_fields` | object | — | ключи из §3.1 для `item`; цвета — color; `units` — string ≤ 255 |
| `widget_types.item.thresholds[]` | object | — | `{threshold: string (число, ≤ 32), color: color}`, оба обязательны; ≤ 64 элементов; `threshold` уникальны |
| `*.attributes` | object | — | `view_mode`: integer 0/1 (только класс «по выбору») |
| color | string | — | `^[0-9A-F]{6}$` (верхний регистр, как хранит Zabbix) |

Пример (как в P1-06A §4, дополнен): см. §8.3.

### 8.2 `zdashstitch.widget-blueprint/v1`

| Поле | Тип | Обязательно | Ограничения |
|---|---|---|---|
| `format` | string | да | const `zdashstitch.widget-blueprint/v1` |
| `blueprint_id` | string | да | UUID v4 (как выше) |
| `name` | string | да | 1–255 (имя виджета Zabbix ≤ 255); при вставке — имя виджета по умолчанию |
| `description`, `tags`, `version`, `adapter`, `declared_compatibility` | — | как у профиля | — |
| `widget_type` | string | да | enum `svggraph`, `item` (типы с `create` + `edit` на 7.4) |
| `default_size` | object | да | `{width: 1–72, height: 1–64}` целые |
| `attributes` | object | нет | `view_mode` 0/1, `rf_rate` integer (допустимые значения Zabbix, проверка `application`) |
| `parameters` | object | да | `roles` (array), `metrics` (array); суммарно ≥ 0; имена `param` уникальны между ролями и метриками, `^[a-z][a-z0-9_-]{1,63}$` |
| `parameters.roles[]` | object | — | `{param, cardinality: one|many, description?: ≤ 500}` |
| `parameters.metrics[]` | object | — | `{param, value_type?: enum numeric_float|numeric_unsigned|character|log|text, units?: ≤ 255, description?}`; ключа элемента, каталога или селектора нет и не будет (В-14): метрика выбирается при вставке |
| `fields[]` | array[object] | да | ≥ 1; каждый `{name, field_type, value}`: `name` — имя поля API `^[a-z][a-z0-9_.]{0,127}$`, уникально; `field_type` — целое `ZBX_WIDGET_FIELD_TYPE_*` (P1-IN-10); `value` — одно из: `{kind: integer, value: int}`, `{kind: string, value: string ≤ 2048}`, `{kind: host, role: "$<param>", selection: all|one}` (только для типов-ссылок на узлы), `{kind: metric, metric: "$<param>"}` (ссылки на элементы), `{kind: unbound, object: host|item}` (без отображаемого имени — в заготовке оно запрещено) |
| запреты в `fields[].name` | — | — | `reference`; любое `*._reference`; `or.*` (`or.N.*`); `problemhosts.N`, `hostids.N`, `groupids.N` литералом (только `{kind: host}` или `unbound`); `ds.N.hosts.M`, `ds.N.items.M`, `ds.N.itemids.M`, `itemid.N`, `override_hostid.N` литералом (только `host`/`metric`/`unbound`) |
| связность | — | — | каждый `$param` в `fields` объявлен; каждый объявленный параметр использован ≥ 1 раз; `item` содержит `itemid.0`; `svggraph` содержит ≥ 1 набор `ds.N` с (`hosts.0` и `items.0`) или `itemids.0` |

### 8.3 Примеры (эталоны для fixtures)

```yaml
format: zdashstitch.style-profile/v1
profile_id: 7a2f3c10-5b7e-4c8a-9d21-0f6e4b1a2c3d
name: "Рост БД — столбцы"
description: "Столбцы с накоплением для графиков роста размера баз данных"
tags: [базы, рост]
version: 3
adapter: "7.4"
declared_compatibility: { minimum: "7.4.0" }
widget_types:
  svggraph:
    widget_fields: { legend: 1, legend_lines: 2, lefty: 1, lefty_min: "0", show_problems: 1 }
    dataset_fields: { type: 3, stacked: 1, transparency: 3 }
    series_palette: ["3156A3", "8B5CF6", "1F6B43"]
  item:
    widget_fields: { show: [1, 2], value_size: 45, value_bold: 1, units_show: 1, units_size: 35, bg_color: "" }
    thresholds: [{ threshold: "80", color: "E8A33D" }, { threshold: "95", color: "A32935" }]
    thresholds_units: "%"
```

```yaml
format: zdashstitch.widget-blueprint/v1
blueprint_id: 91c0d2e4-6a1b-4f3c-8e5d-7b9a0c1d2e3f
name: "Загрузка и нагрузка CPU"     # не совпадает с именем встроенного профиля «Нагрузка CPU — линия» (§14 п. 8)
version: 1
adapter: "7.4"
widget_type: svggraph
default_size: { width: 24, height: 6 }
parameters:
  roles:   [{ param: cpu_hosts, cardinality: many, description: "Узлы для графика CPU" }]
  metrics:
    - { param: cpu_util, value_type: numeric_float, units: "%", description: "Загрузка CPU" }
    - { param: load_avg, value_type: numeric_float, description: "Средняя нагрузка" }
fields:
  - { name: ds.0.dataset_type, field_type: 0, value: { kind: integer, value: 1 } }
  - { name: ds.0.hosts.0,      field_type: 1, value: { kind: host, role: "$cpu_hosts", selection: all } }
  - { name: ds.0.items.0,      field_type: 1, value: { kind: metric, metric: "$cpu_util" } }
  - { name: ds.0.type,         field_type: 0, value: { kind: integer, value: 0 } }
  - { name: ds.0.width,        field_type: 0, value: { kind: integer, value: 2 } }
  - { name: ds.1.dataset_type, field_type: 0, value: { kind: integer, value: 1 } }
  - { name: ds.1.hosts.0,      field_type: 1, value: { kind: host, role: "$cpu_hosts", selection: all } }
  - { name: ds.1.items.0,      field_type: 1, value: { kind: metric, metric: "$load_avg" } }
  - { name: ds.1.axisy,        field_type: 0, value: { kind: integer, value: 1 } }
  - { name: lefty_min,         field_type: 1, value: { kind: string, value: "0" } }
```

Профиль «Рост БД — столбцы» версии 3 выше — ровно те данные, что в макетах 14-01, 14-11 и 14-12 (8 полей, 3 цвета); версия 4 из 14-06/14-10/14-11 отличается `legend_lines: 3`, четвёртым цветом `FFD166` и отсутствием `show_problems` (M-17).

Отрицательные fixtures — по одному на каждый код §8.4 (как `examples/`/`tests/` для `project/v2`); Rust `core` и Python-эталон обязаны давать одинаковый accept/reject (`format_parity.rs`).

### 8.4 Коды отказа `core` (негативные случаи)

| Код | Условие | Формат |
|---|---|---|
| `format_newer` (состояние импорта, не отказ схемы) | `format` того же пространства с версией > 1 | оба |
| `profile_id_invalid` / `blueprint_id_invalid` | не UUID v4 | оба |
| `profile_name_required` / `blueprint_name_required` | пустое или только пробелы | оба |
| `profile_version_invalid` | `version` < 1 или не целое | оба |
| `adapter_unsupported` (состояние `unknown`, не отказ) | `adapter` вне списка приложения | оба |
| `profile_widget_types_empty` | `widget_types` пуст | профиль |
| `profile_widget_type_unsupported` | ключ `widget_types` не `svggraph`/`item` | профиль |
| `profile_section_empty` | раздел без полей, палитры, порогов и атрибутов | профиль |
| `profile_unknown_field` | ключ `widget_fields`/`dataset_fields` вне списка §3.1 (в том числе поля класса «никогда» — например `ds.0.hosts.0`, `aggregate_function`; для них текст `profiles.field.unknown_for_type` уточняется подсказкой «никогда не входит в профиль») | профиль |
| `profile_field_type_mismatch` | строка вместо целого и наоборот | профиль |
| `profile_colour_format` | цвет не `^[0-9A-F]{6}$` | оба |
| `profile_palette_exclusive` | `series_palette` и `color_palette` вместе | профиль |
| `profile_palette_too_long` | > 64 цветов | профиль |
| `profile_show_required` | `item.widget_fields.show` отсутствует или пуст | профиль |
| `profile_threshold_pair` | порог без значения или цвета | профиль |
| `profile_thresholds_units_orphan` | `thresholds_units` без `thresholds` | профиль |
| `blueprint_widget_type_unsupported` | тип не `svggraph`/`item` | заготовка |
| `blueprint_default_size_invalid` | вне 1–72 × 1–64 | заготовка |
| `blueprint_parameter_name_invalid` | имя не по правилу | заготовка |
| `blueprint_parameter_duplicate` | одно имя у двух параметров | заготовка |
| `blueprint_parameter_undeclared` | `$param` в `fields` не объявлен | заготовка |
| `blueprint_parameter_unused` | объявлен, не используется | заготовка |
| `blueprint_host_pattern` | литерал в поле-ссылке на узлы (`ds.N.hosts.M`, `problemhosts.N`, `hostids.N`, `groupids.N`, `override_hostid.N`) | заготовка |
| `blueprint_item_pattern` | литерал в `ds.N.items.M`/`ds.N.itemids.M`/`itemid.N` | заготовка |
| `blueprint_override` | любое `or.*` | заготовка |
| `blueprint_reference` | `reference` или `*._reference` | заготовка |
| `blueprint_unknown_field` | имя поля вне описания формы типа для `adapter` (список имён 7.4.16 фиксируется в схеме как enum по типу — иначе неизвестное поле прошло бы в черновик, что запрещено O-034 п. 5) | заготовка |
| `blueprint_field_type_mismatch` | `field_type` не соответствует имени поля или `value.kind` не допустим для поля | заготовка |
| `blueprint_incomplete` | нет `itemid.0` (`item`) / нет набора с источниками (`svggraph`) | заготовка |
| `blueprint_unbound_display_name` | `{kind: unbound}` с дополнительными ключами | заготовка |
| общие `yaml_*`, `size_*`, `depth_*` | строгий загрузчик `core` | оба |

Тексты — по карте ошибок P1-00F §3 («Данные противоречивы ({code})») и специальным ID `errors.import.blueprint_*` (P1-06C–D).

### 8.5 Чек-лист `DOC_GOVERNANCE` для добавления схем

1. `tools/build_schemas.py`: два новых словаря-схемы; `schemas/style-profile.schema.json`, `schemas/widget-blueprint.schema.json` генерируются, `git diff --exit-code -- schemas/` чист.
2. `tools/validate.py`: распознавание по `format`, семантика §8.1–§8.2 (связность параметров, запреты имён полей, взаимоисключение палитр, `thresholds_units`).
3. `examples/`: два положительных примера §8.3; `tests/`: негативные fixtures по §8.4 с именами `test_<audit-id>_*` и строками в `docs/traceability_v06.md`.
4. `docs/architecture/format_contract_v1_ru.md`: два новых формата в таблице «Переносится», правило «не входят в DATA-ONLY ZIP», список запрещённого содержимого.
5. ADR (новый, после ADR-013): решение O-012 п. 4, границы форматов, отсутствие ссылки на профиль в `project/v2`.
6. Примечание о совместимости: `/v0` внутренние файлы читаются и переписываются в `/v1` при первом сохранении из интерфейса (как настройки P1-09D).
7. Запись в `OPEN_DECISIONS.md` для утверждения владельцем (включая Q11 `thresholds_units`) до слияния схем.
8. Rust `core`: fixtures паритета (`format_parity.rs`), `portable_io.rs` для безопасного чтения.

## 9. Команды приложения и коды ошибок (сводно)

Ответы — коды и параметры, без готового текста (AGENTS.md §4); ID сервера (`hostid`, `itemid`, `widgetid`) в ответах интерфейсу отсутствуют и живут только в операциях рабочей копии (O-018).

### 9.1 Библиотека и импорт/экспорт (P4-04, P4-05)

| Команда | Предусловия | Успех | Ошибки |
|---|---|---|---|
| `library.profiles.list(filter{widget_type, origin}, sort, query)` / `library.blueprints.list(...)` | — | строки: `id`, `kind`, `name`, `widget_types[]`, `version`, `compat_range`, `state` (`ok`/`invalid`/`unknown_adapter`/`builtin`/`unreadable`), `origin`, `usage{widgets, drafts, outdated}` / `inserted{count, drafts}`, `params_summary{roles, metrics}`, `default_size`, `updated_at` | `library_unreadable` по объекту (список не падает) |
| `profiles.get(id)` / `blueprints.get(id)` | объект есть | переносимая часть + карточка + вычисленная совместимость + `state` + ошибки полей для `invalid` | `not_found` |
| `profiles.rename(id, name, description, tags)` / `blueprints.rename(...)` | не встроенный | `updated_at` (версия не меняется, §14 п. 1) | `name_required`, `builtin_readonly`; предупреждение `name_taken` |
| `profiles.duplicate(id)` / `blueprints.duplicate(id)` | состояние ≠ `unknown_adapter` | новый объект, версия 1, `origin: local` | `adapter_unsupported` |
| `profiles.delete(id)` / `blueprints.delete(id)` | не встроенный | удалено; записи применения переводятся лениво | `builtin_readonly` |
| `profiles.export(id, destination)` / `blueprints.export(...)` | форматы утверждены; для заготовки — сканер пройден или подтверждён | файл записан | `format_not_enabled`, `destination_exists`, `destination_unavailable`, `scan_pending` |
| `import.files.inspect(files[])` (расширение P1-03H) | — | по файлу: `kind` (`profile`/`blueprint`), имя, типы, `file_version`, `adapter`, `conflict` (`none`/`same_id {local_version, local_updated_at, used_by}`), `decision_required`, `state_after` (`ok`/`unknown_adapter`) | коды §8.4, `format_newer`, коды P1-03G |
| `import.files.run(files[], decisions{file → replace|copy})` | все решения сделаны | объекты созданы/заменены; `version` = `max(local, file) + 1` при замене; имя «{name} (импорт)» при копии | те же; `decision_required` |

### 9.2 Извлечение и редактор (P4-05, P4-03)

| Команда | Успех | Ошибки |
|---|---|---|
| `profiles.extract.inspect(widget_refs[])` | по типам: источник, поля по классам (`profile`/`optional`/`never`) с текущими значениями, причинами исключения (`dependent {depends_on, value}`, `invalid`), `datasets_differ`, палитра, `legacy_view`, `skipped_unsupported[]` | `capability_not_verified`, `draft_open_elsewhere` |
| `profiles.extract.save(name, description, tags, sections{type → {fields{}, palette | color_palette, thresholds, thresholds_units, attributes}}, include_defaults, target: new | {profile_id})` | `profile_id`, `version`; запись применения у источника (локальный черновик: `profile-marks.json`; копия: `apply_style_profile` без изменений значений) | `name_required`, `field_invalid`, `no_fields`, `storage_unavailable`, `not_found` (цель), `builtin_readonly` |
| `blueprints.extract.inspect(widget_ref)` | источники → параметры по умолчанию, `blockers[{code, fields, fixes[]}]`, `scan_warnings[]`, `default_size`, `kept[]`, `not_kept[]` | `capability_not_verified`, `widget_incomplete` |
| `blueprints.extract.save(name, description, tags, parameters[], merges[], decisions{blocker → fix})` | `blueprint_id` | `blocked {unresolved[]}`, `parameter_name_invalid`, `parameter_name_taken`, `storage_unavailable` |
| `profiles.editor.open(profile_id | new{sections[], from_builtin?})` | профиль, карточка, описания форм, `readonly` | `not_found`, `adapter_unsupported` |
| `profiles.form.describe(widget_type)` | поля классов «профиль»/«по выбору» с умолчаниями, зависимостями, проверками, ID подписей | `capability_not_verified` |
| `profiles.validate(draft)` | ошибки: `field_invalid {field, min, max, format}`, `axis_order`, `threshold_pair`, `show_required`, `palette_exclusive`, `section_empty`, `no_sections`, `name_required`, `unknown_field`; предупреждения: `palette_low_contrast {index, ratio}`, `name_taken` | — |
| `profiles.save(profile_id?, draft, base_version?)` | `profile_id`, `version`, `compat_range`, `updated_at`, `outdated_count` | `field_invalid`, `version_conflict`, `builtin_readonly`, `storage_unavailable`, `deleted_elsewhere` |
| `profiles.preview.render(section_draft, sample_id)` | `SyntheticPreview` (P1-04I), не сериализуется | — |

### 9.3 Вставка заготовки (P4-03, P5-01, P2-05)

| Команда | Успех | Ошибки |
|---|---|---|
| `blueprints.insert.inspect(blueprint_id, page, draft_kind)` | параметры с кандидатами (роли с совместимостью и предложенным именем новой роли; метрики со статусом `exact`/`compatible` и каталогом), `default_size`, первое свободное место, `compat_warning` | `adapter_unsupported`, `capability_not_verified`, `blueprint_unreadable`, `blueprint_not_found`, `draft_open_elsewhere` |
| `blueprints.insert.metrics.candidates(param, scope: draft | library)` | метрики каталогов черновика (локальный) или всех каталогов библиотеки (копия) по типу значения и единицам параметра: `exact`/`compatible`, каталог; ни один не отмечен | `catalog_missing` |
| `blueprints.insert.resolve_copy(param, metric_ref{catalog_id, catalog_version, key}, hosts[])` | строки «узел × элемент» со статусами P1-IN-03 и основаниями: поиск по ключу выбранной метрики каталога, затем тип значения и единицы (В-14) | `not_connected`, `candidate_not_visible`, `catalog_missing` |
| `editor.apply op=add_widget {blueprint_id, params{param → role/slot | metric | hosts+items | unbound}, new_roles[], place, size, name}` | `widget_ref`, созданные роли, шаг `insert_blueprint`, находки (`unbound` → только `source_binding_required`); копия — операция `add_widget` | `params_undecided`, `no_free_space`, `role_name_taken`, `role_name_invalid`, `capability_not_verified`, `catalog_missing`, `storage_unavailable` |

### 9.4 Применение и происхождение (P4-05, P4-03, P2-05)

| Команда | Успех | Ошибки |
|---|---|---|
| `profiles.compatibility(profile_id, scope{widget_refs[] | page | dashboard}, mode, force_thresholds)` | по виджетам: `level`, `reason`, `fields[{name, outcome, before{value, origin}, after}]`; итоги `full/partial/skipped`, `changed/unchanged/kept`; предупреждения `palette_repeats`, `units_unknown`, `legacy_view`, `switch_profile`, `view_mode` | `profile_not_found`, `profile_invalid`, `adapter_unsupported`, `draft_open_elsewhere`, `library_unreadable` |
| `editor.apply op=apply_profile {profile_id, profile_version, mode, scope, force_thresholds, from_record?}` | `applied[]`, `partial[]`, `skipped[]`, шаг `apply_profile`, записи применения (`from_record: true` — «Вернуть все значения профиля»: значения из записи виджета, запись не меняется); копия — `apply_style_profile` | `no_targets`, `nothing_to_apply`, `capability_not_verified`, `storage_unavailable`, `version_conflict` |
| `editor.apply op=detach_profile {widget_refs[], fields[]?}` | без `fields` — записи удалены целиком; с `fields` — поля убраны из записей (метки → «изменено»); шаг `detach_profile`; без подтверждения | — |
| `profiles.origin(widget_ref)` | `{profile_id, profile_name, profile_version, current_version, outdated, deleted, fields[{name, recorded_value, state}]}` или пусто | — |
| `profiles.usage(profile_id)` / `blueprints.usage(blueprint_id)` | `{widgets, drafts, outdated}` / `{count, drafts}` | — |

Соответствие операций: `editor.apply op=apply_profile` ↔ операция копии `apply_style_profile`; `op=add_widget {blueprint_id}` ↔ `add_widget`; `op=detach_profile` ↔ `apply_style_profile` с пустым списком значений — обычная отменяемая операция; метка у источника при извлечении — такая же операция, но с `service: true`, без шага истории и вне «Мои изменения» (В-16). Имена `editor.op.*` для истории: `apply_profile`, `detach_profile`, `insert_blueprint`.

События для интерфейса: `library.changed {kind, id, action}` (список и карточки), `profiles.usage.changed {profile_id}`, `draft.origin.changed {draft_id, widget_refs[]}` (плашки инспектора после применения/отвязки/удаления профиля).

## 10. Message IDs эпика

| Документ | Новых ID | Пространства | Примечание |
|---|---|---|---|
| P1-06A–B | 44 | `profiles.*`, `blueprints.*` | в 1.1 добавлены четыре плашки `profiles.compatibility.badge.*` (M-20); текст `profiles.action.update_from_profile` — с многоточием |
| P1-06C–D | 134 | `library.*`, `profiles.*`, `blueprints.*`, `errors.library.*`, `errors.import.blueprint_*`, `import.files.becomes.*`, `import.files.subtitle` | плюс изменение текста существующего `profiles.action.extract` → «Save style as profile…» / «Сохранить оформление как профиль…»; `import.files.subtitle` — подзаголовок «Импорт файлов» (04-02, 14-03), ID раньше не было |
| P1-06E–F | 132 | `profiles.editor.*`, `blueprints.insert.*`, `bindings.unbound.*`, `editor.op.insert_blueprint` | 1.1: снят `blueprints.insert.next` (дубль `common.action.next_step`, m-42) и `profiles.editor.section.remove.confirm` (M-19); добавлены `blueprints.insert.step.params`, `profiles.editor.section.removed`, `profiles.editor.section.restore`, `blueprints.insert.copy.choose_metric`, `…change_metric`, `…metric_from_catalog`, `…no_catalog` (B-04); `{profile}` в `blueprints.insert.copy_hint` → `{connection}` |
| P1-06G | 78 | `profiles.apply.*`, `profiles.update.*`, `profiles.field.*`, `profiles.widget.*`, `editor.op.apply_profile`, `editor.op.detach_profile`, `common.action.more` | `common.action.more` («Ещё») раньше нигде не был определён (m-42) — вносится в P1-00F §8; RU-формы `few/many` добавлены в пять ID |
| **Итого** | **388** | | используются 5 существующих `profiles.*` из P1-00F (у двух меняется текст, §12) и общие `common.*`, `editor.*`, `library.*`, `bindings.*`, `widget.type.*` |

Правила: ICU-плюрализация (`one/few/many/other` в RU — проверено скриптом по всем таблицам эпика, форм `one/other` без `few/many` не осталось), дублей ID между документами эпика нет, параметры-данные не переводятся, `{widget_type}` — `widget.type.<id>`, заголовки колонок — `library.column.*`/`profiles.apply.column.*` по правилу P1-00F §8. Список строк для языковой проверки — в конце каждого документа (C–D: `profiles.origin.user`/`blueprints.origin.user`, `profiles.extract.summary`; E–F: `profiles.editor.field.dependent_hint`, `profiles.editor.summary.text`, `blueprints.insert.status.*`, `blueprints.insert.after.*`; G: `profiles.apply.summary`, `profiles.apply.outcome.kept_local`, `profiles.apply.mode.merge_hint`, `profiles.update.outdated_count`). В P1-00F §7 добавляются четыре строки с этими числами (§12).

## 11. Проверки приёмки (список тестов)

### 11.1 Схемы и `core` (Python-эталон + Rust-паритет)

1. Положительные примеры §8.3 проходят `validate.py` и `core`.
2. По негативному fixture на каждый код §8.4; Rust и Python отвергают одинаково.
3. Файл профиля с полем класса «никогда» (`ds.0.hosts.0`, `aggregate_function`, `time_period.from`) отвергается (`profile_unknown_field`).
4. Заготовка с `ds.0.hosts.0 = "pg-a-*"`, с `or.0.hosts.0`, с `time_period._reference`, с неизвестным полем, с `{kind: unbound, display_name: …}` — отвергается с соответствующим кодом.
5. Строгий загрузчик: алиасы, дубли ключей, превышение глубины/размера — существующие коды.
6. `format: …/v2` → `format_newer`; `adapter: "8.0"` → принимается, состояние `unknown_adapter`.
7. Экспорт → импорт того же файла даёт байт в байт идентичный `library/*/<id>.yaml` (детерминированная сериализация, сортировка ключей как в `package.py`).

### 11.2 Библиотека, импорт, экспорт (P4-04)

8. Разделы работают без подключения; карточка не содержит ID сервера, узлов, элементов, подключения, имени пользователя ОС.
9. Импорт с тем же `profile_id` без решения → «Импортировать» недоступна; `replace` → `version = max(local, file) + 1` (14-03: локальная 3, файл 4 → 5; В-15), `outdated` у виджетов; `copy` → новый ID, имя «{name} (импорт)».
10. Удаление используемого профиля не блокируется; после удаления `profiles.origin` виджетов → пусто, плашки «изменено».
11. Встроенные профили — ровно два («Рост БД — столбцы», «Нагрузка CPU — линия», В-17): `rename`/`delete`/`save` → `builtin_readonly`; `duplicate` → изменяемая копия; совпадение имени копии со встроенным — только предупреждение `name_taken`.
12. Экспорт при `format_not_enabled` недоступен; экспорт заготовки запускает сканер; существующий файл не перезаписывается молча (P1-09G).
13. Переименование не меняет `version` (§14 п. 1).

### 11.3 Извлечение и редактор (P4-05)

14. `extract.inspect` для 14-04: 7 полей «профиль» включены, 3 «по выбору» выключены, 11 «никогда» перечислены; `fill` исключён с причиной при `ds.0.type = 3`.
15. Виджет в старом виде `graph` → только палитра, `legacy_view`.
16. Извлечение не создаёт шага истории и не меняет значения; создаёт запись применения у источника (Q4); Ctrl+Z сразу после извлечения метку **не снимает** ни в локальном черновике, ни в копии (операция `apply_style_profile` с `service: true` пропускается отменой и отсутствует в «Мои изменения») — В-16.
17. `profiles.validate`: `value_size: 145` → `field_invalid {min: 1, max: 100}`; `lefty_min ≥ lefty_max` → `axis_order`; `show: []` → `show_required`; палитра + `color_palette` → `palette_exclusive`.
18. `profiles.save` увеличивает `version`, пересчитывает `compat_range`, не меняет ни один черновик; `base_version` устаревшая → `version_conflict`.
19. Предпросмотр: детерминирован по `sample_id`, одинаков в светлой и тёмной теме приложения, не сериализуется.

### 11.4 Заготовки (P4-05, P4-03, P5-01, P2-05)

20. `blueprints.extract.inspect` на 14-05: 4 источника → 3 параметра (повторная роль объединена), 3 препятствия (неизвестные поля — `ds.0.vendor_hint`, `vendor_layout`; `legend_lines_mode` известно реестру и препятствием не является, M-15); `save` с одним решением из трёх → `blocked {unresolved: 2}`; с решениями → объект без `or.*`, без `_reference`, без неизвестных полей; имя узла не попадает в имя/описание параметра (рабочая копия).
21. `blueprints.insert.inspect`: кандидаты-метрики только с тем же типом значения; ни один не отмечен; роли `many` для параметра `one` — несовместимы с причиной.
22. `add_widget` с `blueprint_id`: виджет `type: zabbix`, новый уникальный `reference`, `default_size`, роль создана в том же шаге; Ctrl+Z убирает виджет и роль; `unbound` → только `source_binding_required` (нет `unknown_metric`); черновик с такой находкой сохраняется и экспортируется, публикация блокируется (B-05).
23. В копии с сервера: `blueprints.insert.metrics.candidates(scope: library)` без каталогов → `catalog_missing`; `resolve_copy(param, metric_ref, hosts[])` ищет элементы по ключу выбранной метрики (14-09: `cpu_util` → 2 из 2, `load_avg` → 1 из 2, `db-lab-02` — `missing`); реальные `hostid`/`itemid` только в операции; `not_connected` → «нужна привязка»; виджет виден в «Мои изменения» как добавленный.

### 11.5 Применение, режимы, происхождение (P4-05, P4-03, P2-05)

24. `profiles.compatibility` на странице 14-10 (профиль версии 4 — §8.3): `svggraph` с полным набором → `compatible` 7/7; виджет в старом виде → `partial` 1/7; `item` без раздела → `incompatible` `type_not_in_profile`; «Радиальный датчик» → `incompatible` `edit_unsupported`; плашки колонки — `profiles.compatibility.badge.*`.
25. Зависимое поле при невключённом управляющем: профиль `{ds.N.fill: 1}` без `type`, виджет `type = 3` → `skipped_dependent`.
26. Пороги: профиль `thresholds_units: "%"`, метрика в `B` → `skipped_units`; `force_thresholds` → `apply`; единицы неизвестны → `apply` + `units_unknown`.
27. Режимы по таблице §3.3 — по тесту на каждую клетку, включая «убрано из профиля» → «изменено» и `merge` для нового поля при локальном «изменено»; эталон 14-11 (запись версии 3, профиль версии 4, `keep_local`): `legend_lines` 2 → 3 и палитра 3 → 4 — `apply`; `transparency` 5 (изменено после профиля, в записи 3) — `kept_local`; `show_problems` — убрано из профиля → остаётся, «изменено»; `type`, `stacked`, `lefty_min` — `unchanged`.
28. Область «панель» через 3 страницы — одна операция, один шаг истории; Ctrl+Z возвращает все поля и все записи применения.
29. После применения `project/v2` не содержит `profile_id`; экспорт → импорт на чистом хранилище → `profiles.origin` пусто.
30. Инспектор: четыре состояния поля и действия; «Вернуть значение профиля» берёт значение из записи, а не из текущей версии; «Вернуть все значения профиля» — `apply_profile {from_record: true}`, запись и её версия не меняются; «Отвязать от профиля» у поля — `detach_profile {widget_refs, fields: [поле]}`, метка только этого поля → «изменено»; «Отвязать виджет» (`detach_profile` без `fields`) удаляет запись, значения не меняются, `profiles.usage` уменьшается; оба — без подтверждения, Ctrl+Z возвращает метки; сводка и действия виджета есть только в разделе «Основное» (В-18).
31. Устаревание: `save` профиля → `outdated = true` у записи; «Обновить из профиля» в `keep_local` оставляет «изменено после профиля», обновляет запись до новой версии; находок в «Проверках» нет.
32. Копия с сервера: операция `apply_style_profile` в списке; «Мои изменения» показывает поля «база → черновик»; «Скопировать заново» с изменённым на сервере полем → конфликт по полю; публикация пишет поля существующими кодеками (лабораторный контракт `lab_write_contracts` без изменений — проверка, что набор записываемых полей не расширился).
33. `editor.confirm_bulk_apply = false`: тост «Применить к другим виджетам…» применяет без окна, тост результата содержит «Отменить»; «Применить профиль…» всегда открывает окно.
34. Доступность (все экраны): клавиатура, `aria-describedby` причин недоступности, состояния текстом, рамки `#7a8d9b`, цели ≥ 32 px, 200 %, высокая контрастность (решение D3 P1-11F).

## 12. Изменения принятых документов (сводный список; применяет DF-C / DESIGN-FIX-01)

Объединяет предложения P1-06A–B, C–D, E–F и G и правки по ревью `REVIEW-2026-10-26-coherence.md` (B-04, B-05, M-14…M-21, m-40…m-42) с учётом решений владельца 26.10.2026 (вечер); дубли сняты. Точный текст вставок приводится; формат строк — как в целевом документе. Здесь эти документы не правятся.

### P1-03-import-library.md

- **P1-03A, таблица источников импорта** — после строки «Экспорт шаблонов Zabbix 7.4 (YAML/JSON/XML)» добавить две строки:
  - `| Файл профиля оформления \`zdashstitch.style-profile/v1\` | «Импорт файлов» | профиль в библиотеке; совпадение идентификатора — явный выбор «Заменить (новая версия)» / «Сохранить как копию» (\`version = max(local, file) + 1\`) | — |`
  - `| Файл заготовки виджета \`zdashstitch.widget-blueprint/v1\` | «Импорт файлов» | заготовка в библиотеке; маски узлов, замещения, связи и неизвестные поля — отказ с кодом | — |`
- **P1-03A, экран «Импорт файлов» (04-02)** — подзаголовок получает ID `import.files.subtitle` «Dashboard and page files, ZIP packages, Zabbix 7.4 template exports, style profiles and widget blueprints.» / «Файлы панелей и страниц, пакеты ZIP, экспорты шаблонов Zabbix 7.4, профили оформления и заготовки виджетов.» (определён в P1-06C–D); макет 04-02 — тот же текст (генератор `import_library_svg.py`).
- **P1-03F, абзац «Библиотека»** — заменить «…«Профили оформления» и «Заготовки виджетов» (появятся с P1-06)» на «…«Профили оформления» и «Заготовки виджетов» (P1-06C)». Макет 04-01: убрать подпись «появятся позже (P1-06).» (m-10), счётчики разделов «Профили оформления 6», «Заготовки виджетов 4» как в 14-01/14-02.
- **P1-03F, таблица разделов библиотеки** — добавить строки:
  - `| Профили оформления | имя, типы виджетов, версия, используют (виджеты · черновики), происхождение или состояние | «Изменить профиль» (P1-06E), «Дублировать», «Переименовать», «Экспорт в файл…», «Создать профиль» в заголовке «Библиотеки» (окно выбора разделов и «на основе встроенного») | всегда (черновики не зависят; предупреждение об использовании) |`
  - `| Заготовки виджетов | имя, тип, параметры (роли · метрики), размер, версия, происхождение или состояние | «Дублировать», «Переименовать», «Экспорт в файл…»; создаются только из виджета в редакторе, вставляются через «Добавить виджет» → «Из заготовок» (P1-06F) | всегда (вставленные виджеты не зависят) |`
- **P1-03F** — абзац о встроенных объектах: «Встроенных профилей два — эталоны «Рост БД — столбцы» и «Нагрузка CPU — линия» (P1-06A §12), только чтение, «Дублировать» даёт изменяемую копию; встроенных заготовок нет.»
- **P1-03H, handoff, строка `library.*.delete`** — дополнить: «Для `library.profiles.delete` и `library.blueprints.delete` отказа `in_use` нет: удаление всегда, с предупреждением `profiles.delete.in_use` / `blueprints.delete.independent` (черновики от профилей и заготовок не зависят, P1-06C §4).»
- **P1-03E, переигрывание** — добавить: «Операция `apply_style_profile` переигрывается по полям: поле, изменённое на сервере, — «Конфликт» с теми же действиями; виджет, исчезнувший на сервере, — «цель исчезла». Операции с `service: true` (метка у источника после извлечения) переигрываются без конфликтов и в отчёте не показываются.»

### P1-04A-E-editor.md и макеты 12-xx

- **P1-04B, меню страницы** — добавить пункт «Применить профиль к странице…» (открывает окно P1-06G с областью «страница»).
- **P1-04C, таблица групп выбора виджета** — строку `| Из заготовок | P1-06 | «Появится позже» | — | — | — | — |` заменить на `| Из заготовок | заготовки «График» (\`svggraph\`) и «Значение элемента данных» (\`item\`) из библиотеки (P1-06F) | да: шаг «Параметры» (роль из черновика или новая, метрика каталога, «нужна привязка»); кнопка «Далее: параметры» (\`common.action.next_step\`, \`step\` = \`blueprints.insert.step.params\`) | все поля с описанием формы | да | да | да |`; в описании 12-02 убрать «заготовки позже».
- **P1-04D, правила полей** — строку «Значение, отличное от умолчания Zabbix, помечено «изменено» и имеет «Вернуть умолчание»; значение по умолчанию — «умолчание Zabbix». Метка «из профиля» зарезервирована для P1-06.» заменить на: «Состояния поля: «умолчание Zabbix» (поле не записано), «изменено» («Вернуть умолчание»), «из профиля «{profile}»» («Вернуть умолчание», «Отвязать от профиля» — без подтверждения, шаг истории) и «изменено после профиля «{profile}»» («Вернуть значение профиля», «Вернуть умолчание») — P1-06G §6. Только в разделе «Основное» у виджета с записью применения — сводка «Из профиля «{profile}»: полей N, изменено после — M» и кнопка «Обновить из профиля…»; баннер «Профиль обновлён до версии {version}» показывается над содержимым любого раздела, если версия записи ниже версии библиотеки.»
- **P1-04D, меню «Ещё» инспектора** — меню получает ID `common.action.more` («More» / «Ещё», определён в P1-06G); пункты «Сохранить оформление как профиль…» (типы с `edit`, общий вид `type: zabbix`), «Создать заготовку…» (один виджет с `edit` + `create`), «Применить профиль…», «Обновить из профиля…», «Вернуть все значения профиля», «Отвязать виджет от профиля» (последние три — при записи применения).
- **P1-04D, макет 12-03 и `editor_svg.py:309`** — в блоке «Неизвестные поля» заменить `legend_lines_mode` на `vendor_layout`: `legend_lines_mode` — известное поле `svggraph` 7.4 (реестр `widget_capabilities.rs`, шаблон `legend*`), неизвестным оно быть не может (M-15). Строка `ds.0.vendor_hint` остаётся. То же имя используется в 14-05.
- **P1-04D, макет 12-03 и остальные 12-xx (генератор `editor_svg.py`)** — имя роли «ClickHouse» заменить на `clickhouse`: имя роли подчиняется правилу `^[a-z][a-z0-9_-]{1,63}$` (дополнение P1-05 §1); в 14-04/14-05/14-08 уже `clickhouse` (m-41). Имя страницы «ClickHouse» — данные пользователя, не меняется.
- **P1-04E, «Буфер обмена» / вставка в рабочую копию** — дополнение: «Ссылки «нужна привязка» у виджетов, вставленных в копию с сервера из буфера или из заготовки, разрешаются в «Привязках» → «Ссылки без привязки» (P1-06F §11).»
- **Handoff P1-04 (таблица команд)** — строки: `| \`editor.apply\` \`op=add_widget\` с \`blueprint_id\` | заготовка \`ok\`, тип с \`create\`, параметры решены | виджет \`type: zabbix\`, созданные роли, шаг \`insert_blueprint\` | \`params_undecided\`, \`no_free_space\`, \`role_name_taken\`, \`role_name_invalid\`, \`capability_not_verified\` |`; `| \`editor.apply\` \`op=apply_profile {profile_id, profile_version, mode, scope, force_thresholds, from_record?}\` | профиль \`ok\`, область не пуста | значения полей по режиму (при \`from_record\` — из записи применения), записи применения, шаг \`apply_profile\` | \`no_targets\`, \`nothing_to_apply\`, \`capability_not_verified\`, \`version_conflict\` |`; `| \`editor.apply\` \`op=detach_profile {widget_refs[], fields[]?}\` | есть запись применения | запись снята целиком или по полям, значения не меняются; шаг \`detach_profile\` | — |`.

### P1-04F-G-validation-history.md

- **P1-04F §2.2** — к строке `source_binding_required` добавить: «возникает также у виджетов, вставленных из заготовки с «Оставить «нужна привязка»» (локальный черновик и копия с сервера); как и всегда, блокирует только публикацию — сохранение и экспорт возможны». Строка `unknown_metric` **не** меняется: у ссылки без метрики каталог не указан, искать нечего, находка не поднимается (B-05, O-034 п. 3). Добавить абзац: «Применение профиля оформления находок не создаёт; устаревание версии профиля у виджета — не находка (показывается в инспекторе, P1-06G §6.3).»
- **P1-04G §4.2** — в перечень массовых действий «один шаг» добавить «применение профиля к странице или панели, «Обновить из профиля», «Вернуть все значения профиля», отвязка от профиля».
- **P1-04G §4.3 матрица команд** — строки: `| Вставить заготовку | структура | да (с созданием роли в том же шаге) | \`add_widget\` | да | \`layout_*\`, \`reference_*\`, \`widget_*\`, \`source_binding_required\` | да |`; `| Применить профиль, обновить из профиля, вернуть все значения профиля | параметр | да (один шаг на область) | \`apply_style_profile\` | да | \`widget_*\`, \`capability_not_verified\` | да |`; `| Отвязать от профиля (поле или виджет) | параметр | да | \`apply_style_profile\` (без значений) | да (только локальные метаданные) | — | да |`; граничная строка-примечание: «Сохранить оформление как профиль» и «Создать заготовку» шага истории **не** создают; метка «из профиля» у виджета-источника после извлечения — неотменяемая служебная операция (локально — запись `profile-marks.json`; в копии — `apply_style_profile` с `service: true`, которую «Отмена» пропускает, а «Мои изменения» не показывают) — решение В-16.
- **P1-04G §4.1, модель истории копии («Отмена снимает последнюю операцию»)** — уточнить: «операции с признаком `service: true` отменой пропускаются».
- **P1-04G §7 `editor.op.*`** — строки: `| \`editor.op.insert_blueprint\` | inserting a blueprint | вставка заготовки |`, `| \`editor.op.apply_profile\` | applying profile {profile} | применение профиля «{profile}» |`, `| \`editor.op.detach_profile\` | detaching from profile {profile} | отвязка от профиля «{profile}» |`.

### P1-04I-synthetic-preview.md

- **P1-04I-B, входные данные** — дополнение: «Для предпросмотра профиля оформления (P1-06E) источник метрики — встроенный пример виджета с фиктивными метриками (тип значения и единицы примера); зерно — от идентификатора примера и номера ряда, не от профиля. Поверхность — фиксированная светлая тема Zabbix 7.4.»

### P1-05-bindings.md и P1-05-addendum-local-draft-roles.md

- **P1-05F** — фразу «цвет — следующий из палитры профиля, не совпадающий с соседними» заменить на «цвет — следующий из палитры профиля, если виджет помечен «из профиля» и у профиля есть палитра (P1-06A §8); иначе — по детерминированному правилу P3-03B; в обоих случаях не совпадающий с соседними».
- **P1-05 §3.2 и дополнение §2 («Для копии с сервера ролей нет…»)** — добавить: «У копии с сервера в «Привязках» есть блок «Ссылки без привязки» (`bindings.unbound.*`): виджеты, вставленные из буфера (P1-04E) или из заготовок (P1-06F), чьи ссылки на узлы и элементы ещё не выбраны; выбор — те же экраны 09-02/09-03 по виджету; для ссылки на элемент сначала выбирается метрика каталога библиотеки, затем элемент по её ключу (`blueprints.insert.resolve_copy(param, metric_ref, hosts[])`). Это не роли: роли у копии появляются только после «Сделать переносимой».»
- **Дополнение P1-05 §2.1 («Создать роль…»)** — добавить: «Роль может быть создана из параметра заготовки в окне вставки (`bindings.roles.create` из P1-06F §10.1): имя и кардинальность — из параметра, создание входит в шаг истории вставки.»
- **P1-05D, 09-03** — режим «метрика каталога → элемент на узле» используется и при вставке заготовки в копию (P1-06F §11); плашки статусов получают ID `blueprints.insert.status.*` или общее пространство, выбранное при реализации (P1-06E–F, языковая проверка).

### P1-07-export.md и макеты 13-01, 13-04

- **P1-07A** — строку «Профили оформления и заготовки — после P1-06 («Появится позже»)» заменить на «Профили оформления и заготовки виджетов в пакет **не входят никогда**: они экспортируются из «Библиотеки» отдельными файлами (`zdashstitch.style-profile/v1`, `zdashstitch.widget-blueprint/v1`, P1-06C §5); черновик после применения профиля содержит обычные значения полей». В блоке «Не входит никогда» на 13-01 добавить строку «профили оформления и заготовки (экспортируются из библиотеки)», убрать «Появится позже». Генератор `export_svg.py` — соответствующая правка текста (13-01 перерисовывается).
- **P1-07I «Сделать переносимой»** — добавить: «Записи применения профилей (`profile-marks.json`) и журнал вставок заготовок переносятся к новой локальной панели как локальные метаданные (они не входят в `project/v2`); профили и заготовки библиотеки при преобразовании не копируются и не нужны — виджеты содержат обычные значения полей.» Макет 13-04: внутренний номер «(O-034)» из текста интерфейса убрать (m-10).

### P1-08-publication.md

- **P1-08B, «Мои изменения»** — добавить: «Применение профиля показывается строкой «Применён профиль «{profile}»: полей — N» (`profiles.apply.changes_entry`) с раскрытием по виджетам и полям «база → черновик»; при публикации это обычные изменения полей. Служебные операции `apply_style_profile` с `service: true` (метка у источника) в списке не показываются и ничего не пишут.»

### P1-09B-09C-settings.md и P1-09D-H-settings.md

- **P1-09D, «Один корень хранилища»** — в перечень содержимого корня добавить: «библиотека профилей оформления и заготовок виджетов (`library/style-profiles/`, `library/widget-blueprints/`, индекс `library-index.json`) и локальные метаданные черновиков о профилях (`drafts/<local_id>/profile-marks.json`, `drafts/<local_id>/blueprint-inserts.json`)» (P1-06H §7).
- **P1-09E, `editor.confirm_bulk_apply`** — добавить: «Для профилей оформления окно «Применить профиль» показывается всегда, когда нужен выбор профиля или области; при выключенной настройке действия с известным профилем и областью (тост после извлечения, «Обновить из профиля» для одного виджета без правок после профиля) применяются сразу с «Отменить» в результате (P1-06G §2.6).» Ссылки на эту настройку в документах эпика ведут на P1-09E (а не P1-09D).
- **P1-09G, экспорт** — имена файлов по умолчанию для объектов библиотеки: `{slug}.style-profile.yaml`, `{slug}.widget-blueprint.yaml` (Q7); правило «не перезаписывать молча» действует и для них.

### P1-01B-information-architecture.md

- **§3 таблица маршрутов** — добавить `/library/profiles/:profile_id/edit` (редактор профиля, крошки «Библиотека / Профили оформления / {name}») и `/library/profiles/new` (создание: окно выбора разделов, затем редактор); правило возврата — на карточку профиля в `/library/profiles`.
- **«Структура», меню панели** — пункт «Применить профиль ко всей панели…».

### P1-00E-states-feedback.md

- **§9 Confirmations** — исключений для эпика нет: «Отвязать от профиля» (поле/виджет) и «Убрать раздел» профиля идут без подтверждения, подтверждаются только удаление объекта библиотеки (необратимо) и закрытие редактора профиля с несохранёнными изменениями (M-19). Запись для справки, текст §9 не меняется.

### P1-00F-message-inventory.md

- **§2 глоссарий / таблица `profiles.*`** — изменить тексты: `profiles.action.extract`: «Create profile from selection» / «Создать профиль из выбранного» → «Save style as profile…» / «Сохранить оформление как профиль…»; `profiles.action.apply`: «Apply profile» / «Применить профиль» → «Apply profile…» / «Применить профиль…» (пункт меню); `profiles.compatibility.partial`: «Some settings cannot be applied to this widget type» / «Часть настроек нельзя применить к этому типу виджета» → «Some settings cannot be applied to this widget» / «Часть настроек нельзя применить к этому виджету» (уровень — свойство виджета, M-20).
- **§7 сводный реестр** — четыре строки: `| [\`P1-06A-B-profiles-model.md\`](P1-06A-B-profiles-model.md) | 44 | … |`, `| [\`P1-06C-D-library-extraction.md\`](P1-06C-D-library-extraction.md) | 134 | … |`, `| [\`P1-06E-F-editor-insert.md\`](P1-06E-F-editor-insert.md) | 132 | … |`, `| [\`P1-06G-bulk-apply.md\`](P1-06G-bulk-apply.md) | 78 | … |` (перечни ID — из таблиц документов; итого 388).
- **§8 «Общие подписи»** — добавить `common.action.more` | More | Ещё (меню «Ещё» инспектора P1-04D и карточек; определён в P1-06G).
- **§8** — заголовки колонок `library.column.*`, `profiles.apply.column.*` получают ID по общему правилу при реализации.

### P1-01-svg-index.md

- **Таблица папок** — строка `| \`14-profiles\` | «Библиотека» → профили оформления и заготовки виджетов; окна редактора | есть, P1-06 |`.
- **Таблица файлов** — двенадцать строк `14-profiles/14-01_P1-06C_library-profiles.svg` … `14-12_P1-06G_inspector-origin.svg` с задачами и экранами из §15.

### MOCKUP_GUIDE.md

- **Генераторы** — строка: `- \`profiles_svg.py\` — макеты \`14-01\`…\`14-12\` «Профили оформления и заготовки» (P1-06C–G); помощники берёт из \`publication_svg.py\`, \`import_library_svg.py\`, \`editor_svg.py\`.`

### assets/icons/THIRD_PARTY.md

- Иконки `upload`, `palette`, `template` добавлены в шагах C–D и E–F (тот же закреплённый релиз `v3.49.0`); шаг G и правки DF-A новых иконок не добавляли (`ban` для недоступных зависимых полей уже в реестре).

### Не меняются

`project/v2`, DATA-ONLY ZIP (O-021), `local-bindings/v1`, `deployment-state/v1`, реестр P0-09, контракты записи P0-06; `format_contract_v1_ru.md`, `schemas/`, `tools/build_schemas.py` — только по §8.5 после утверждения (Q1 принят; акт — при реализации).

## 13. Открытые вопросы владельцу (сводно, без дублей)

Вопросы P1-06A–B 1, 3, 4, 5, 7 закрыты решением O-012 (пункты 1–5) и не повторяются. Нумерация сквозная; в скобках — источник. **Статус: все вопросы Q1–Q19 приняты владельцем по рекомендациям 26.10.2026 (вечер), `OPEN_DECISIONS.md`;** там же приняты решения ревью В-14 (метрика заготовки — из каталога при вставке), В-15 (`max(local, file) + 1`), В-16 (метка у источника — неотменяемая), В-17 (два встроенных эталона), В-18 (сводка только в «Основном»), учтённые в версии 1.1. Таблица сохранена для прослеживаемости.

| № | Вопрос | Рекомендация |
|---|---|---|
| Q1 (A–B 2, E–F 3, H §8) | Утвердить добавление схем `zdashstitch.style-profile/v1` и `zdashstitch.widget-blueprint/v1` в составе §8.1–§8.4 по чек-листу §8.5 (O-012 п. 4 принял принцип; нужен акт по конкретным спискам полей и кодов) | утвердить; до утверждения — внутренние `/v0` без экспорта |
| Q2 (A–B 6) | `view_mode` в профиле «по выбору»: подтвердить, что запись атрибута покрыта контрактом P0-06 (объект виджета передаётся целиком) | подтвердить; иначе исключить `attributes.view_mode` из профиля и заготовки (поле схемы убирается до утверждения) |
| Q3 (A–B 8, P1-05F) | Палитра нового ряда: «виджет «из профиля» с палитрой → палитра профиля; иначе — P3-03B» | подтвердить (текст правки P1-05F в §12) |
| Q4 (C–D 1, C–D 2) | Метка у источника после извлечения: помечать исходный виджет «из профиля» версии 1; в копии — операцией `apply_style_profile` без изменений значений | да, в обоих видах черновика (виджет участвует в «Обновить из профиля» и счётчике) |
| Q5 (C–D 3, E–F 7) | Встроенные профили: поставлять два эталона P1-06A §12 как встроенные, только чтение, открывать в редакторе для просмотра с «Дублировать для изменения»; встроенных заготовок нет | да |
| Q6 (C–D 4) | Редактирование заготовки в MVP: только имя/описание/теги, дублирование, пересоздание из виджета | да; полный редактор полей — после MVP (ROADMAP F-05) |
| Q7 (C–D 5) | Имена файлов по умолчанию `{slug}.style-profile.yaml` / `{slug}.widget-blueprint.yaml` | да (распознавание всё равно по `format`) |
| Q8 (C–D 6) | Флажок «Включать значения, равные умолчанию Zabbix» при извлечении включён по умолчанию | да (профиль — полный вид; «только изменённое» — явный выбор) |
| Q9 (C–D 7, E–F 1) | Неизвестные поля виджета при создании заготовки — препятствие с явным решением «Сохранить без неизвестных полей»; профиль с ошибками в редакторе не сохраняется (состояние `invalid` — только из импорта) | да для обоих (формулировка A–B §4 «не сохраняются» уточняется как «не сохраняются после явного решения») |
| Q10 (E–F 2, E–F 6) | Вставка заготовки в копию с сервера доступна с привязкой к реальным узлам через экраны P1-05; без подключения — «нужна привязка»; создание роли из окна вставки — в том же шаге истории | да |
| Q11 (E–F 3) | `thresholds_units` — справочное поле раздела `item` в схеме профиля (только вместе с `thresholds`), используется предупреждением `units_differ` и редактором | да; без него пороги применяются без проверки единиц (тогда в §8.4 снимается `profile_thresholds_units_orphan`, а исход `skipped_units` исчезает) |
| Q12 (E–F 4, E–F 5) | Предпросмотр профиля — фиксированный набор из шести примеров на светлой поверхности Zabbix 7.4; тёмная тема Zabbix в предпросмотре — не в MVP | да |
| Q13 (G 1) | Режим первого применения по умолчанию — `replace`; повторного — `keep_local` (O-012 п. 2) | `replace` для первого применения |
| Q14 (G 5) | Смена профиля поверх другого: поля прежнего профиля, которых нет в новом, становятся «изменено» (значения сохраняются) | да |
| Q15 (G 3) | `editor.confirm_bulk_apply = false` пропускает окно только для действий с известным профилем и областью, результат всегда с «Отменить» | да |
| Q16 (G 6) | «Отвязать от профиля» — шаг истории (Ctrl+Z возвращает метки); метка у источника при извлечении шага не создаёт | да (разные действия: отвязка — намеренное изменение состояния виджета, извлечение — побочная метка) |
| Q17 (G 2) | Пороги с другими единицами: пропускать с причиной и явным переключателем «Применить пороги, даже если единицы отличаются» | да |
| Q18 (G 4) | Устаревшие версии: только баннер инспектора и счётчик карточки, без баннера на уровне черновика и без находки | да |
| Q19 (H §14 п. 1) | Версия профиля при переименовании/описании/тегах не меняется (меняется только `updated_at`) | да; C–D §11 (`profiles.rename` → «новая версия») исправить при приёмке |

## 14. Расхождения между P1-06A–F и предложенные решения

| № | Где | Расхождение | Решение в этом handoff |
|---|---|---|---|
| 1 | C–D §4 («переименование … не считается» при росте `version`) ↔ C–D §11 (`profiles.rename` → «новая версия») ↔ E–F §4 («дублирование и переименование версию не меняют») | меняет ли переименование `version` | **не меняет** (E–F §4, C–D §4); `profiles.rename` возвращает `updated_at`; C–D §11 исправляется (Q19) |
| 2 | A–B §4 («неизвестные поля не сохраняются») ↔ C–D §9.3 (препятствие с явным решением) | автоматическое отбрасывание или явное решение | явное решение «Сохранить без неизвестных полей» (C–D), результат тот же — поля не сохраняются (Q9) |
| 3 | C–D §5 («схема нарушена, недопустимое значение → не импортируется») ↔ C–D §6 / E–F §4 (состояние `invalid` возникает при импорте файла) | что отвергается, а что сохраняется как `invalid` | нарушения **схемы** (неизвестный ключ, тип значения, формат цвета, пустые `widget_types`/`show`, пара порога) → отказ с кодом §8.4; нарушения **диапазонов формы адаптера** (`value_size: 145`, `legend_lines: 0`) схема не знает → объект сохраняется в состоянии `invalid` и исправляется в редакторе; `profiles.field.unknown_for_type` на 7.4 практически не возникает (список ключей схемы = список формы 7.4), оставлен для будущих адаптеров |
| 4 | E–F «Итого 137 новых ID» | число включало 10 кодов ошибок таблицы §12 (не message IDs) | фактически было 127; после правок 1.1 (−2 +7) — 132 (§10, пересчёт скриптом); E–F исправлен; в P1-00F §7 вносится 132 |
| 5 | A–B §3.2 / C–D §8.5 (операция копии `apply_style_profile`) ↔ A–B команды (`editor.apply op=apply_profile`) | два имени одной сущности | не противоречие: `editor.op.*`/`op=` — имена шагов и команд редактора, `apply_style_profile` — операция рабочей копии (как `edit_field` ↔ `set_widget_field` в P1-04G); соответствие зафиксировано в §9.4 |
| 6 | A–B §3.2.1 (`merge`: «остаются; поля, которых раньше в профиле не было, получают значение профиля» в одной клетке) | неоднозначно для полей «изменено» без записи | уточнено в P1-06G §5.2: новое для профиля поле получает значение даже при локальном «изменено»; при первом применении `merge` = `keep_local` |
| 7 | A–B §9 («другие единицы — `partial` с предупреждением», то есть пороги не применяются) ↔ E–F `profiles.editor.thresholds.units_hint` («иные единицы — предупреждение», читается как «применяются с предупреждением») | применять ли пороги при разных единицах | не применять (`skipped_units`) с явным переключателем в окне (Q17); текст подсказки E–F остаётся верным («предупреждение» — в окне применения) |
| 8 | A–B §12 называет «Нагрузка CPU — линия» эталонным **профилем**; 14-01 показывает такой профиль, а 14-03/14-08/14-09 показывали **заготовку** с тем же именем | одно имя у двух сущностей в макетах | **снято в 1.1 (M-21):** встроенный профиль остаётся «Нагрузка CPU — линия» (В-17), заготовка-пример переименована в «Загрузка и нагрузка CPU» (A–B §4, H §8.3, 14-03/14-08/14-09); имена — данные пользователя, модель не нарушена |
| 9 | A–B §5 (параметры заготовки: «нужна привязка» допустимо для роли?) ↔ E–F §10.1 («роль обязательна, «нужна привязка» для роли не предлагается» в локальном черновике) | можно ли оставить параметр-роль без решения | E–F: в локальном черновике роль обязательна (ссылка на узлы без роли невозможна в `project/v2`); «нужна привязка» — для параметров-метрик и для копии с сервера без подключения; A–B §5 читать с этим уточнением |
| 10 | G §12 (`detach_profile {widget_refs[]}`, шаг `apply_profile` «с подписью отвязки») ↔ A–B §3.1 и G §6.1 («Отвязать от профиля» у отдельного поля) ↔ H §9.4 | сигнатура отвязки и имя шага | **решено в 1.1 (M-18):** `op=detach_profile {widget_refs[], fields[]?}`, шаг `editor.op.detach_profile`; G §12 приведён к H §9.4 |
| 11 | H §8.2 / C–D §9.2 (в параметре-метрике нет ключа) ↔ E–F §11 («элемент на каждом узле по ключу метрики»), 14-09, `resolve_copy(param, hosts[])` | откуда взять ключ в копии с сервера | **решено (В-14, B-04):** при вставке в копию метрика выбирается из каталогов библиотеки (`blueprints.insert.metrics.candidates(scope: library)`), затем `resolve_copy(param, metric_ref, hosts[])` ищет элементы по её ключу; заготовка остаётся без ключей; 14-09 показывает шаг выбора |
| 12 | H §5.4 (`source_binding_required`/`unknown_metric`), E–F §10.2, правка P1-04F в §12 ↔ O-034 п. 3 и E–F §10.4 («сохранить можно, публикация — после привязки») | какие находки даёт невыбранная метрика | **решено (B-05):** только `source_binding_required` — блокирует одну публикацию; `unknown_metric` не поднимается (каталог не указан); исправлены H §5.4, E–F §10.2, §12 |
| 13 | C–D §7/§8.5 («операция `apply_style_profile` с пустым списком», «шага истории нет») ↔ P1-04G §4.1 (у копии «Отмена снимает последнюю операцию») | снимет ли Ctrl+Z метку у источника | **решено (В-16, M-16):** метка — неотменяемая служебная операция: локально без шага истории; в копии `apply_style_profile` с `service: true`, отмена её пропускает, «Мои изменения» не показывают; отвязка (Q16), напротив, шаг истории |
| 14 | G §11 («Отвязать виджет от профиля» с подтверждением), E–F §3 («Убрать раздел» с подтверждением) ↔ P1-00E §9 | подтверждения обратимых действий | **решено (M-19):** подтверждения сняты; `profiles.editor.section.remove.confirm` заменён строкой `profiles.editor.section.removed` + «Вернуть раздел»; исключений из P1-00E §9 не требуется |
| 15 | 14-06/14-10 ↔ 14-11 ↔ 14-01 (разный состав «версии 4» и «версии 3») | какие данные у эталонных версий профиля | **решено (M-17):** v3 — пример §8.3 (8 полей, 3 цвета); v4 — `legend_lines: 3`, 4 цвета, без `show_problems`; все пять макетов перегенерированы по этим данным; тесты 24 и 27 ссылаются на них |
| 16 | H §3.1 / A–B §8 (`legend_lines_mode` — поле класса «профиль») ↔ 14-05 и принятый 12-03 (`legend_lines_mode` — «неизвестное поле») | известно ли поле реестру | **решено (M-15):** известно (`legend*` в `widget_capabilities.rs`, поле формы 7.4); в 14-05 неизвестное поле заменено на `vendor_layout`; для 12-03 — правка в §12 (DF-C) |

## 15. Индекс макетов эпика

| Файл | Экран |
|---|---|
| `14-01_P1-06C_library-profiles.svg` | библиотека: профили |
| `14-02_P1-06C_library-blueprints.svg` | библиотека: заготовки |
| `14-03_P1-06C_import-conflict.svg` | импорт файлов профилей/заготовок, конфликт идентификатора |
| `14-04_P1-06D_extract-profile.svg` | «Сохранить оформление как профиль» |
| `14-05_P1-06D_create-blueprint.svg` | «Создать заготовку» |
| `14-06_P1-06E_profile-editor-svggraph.svg` | редактор профиля, раздел «График» |
| `14-07_P1-06E_profile-editor-item.svg` | редактор профиля, раздел «Значение элемента данных», ошибка |
| `14-08_P1-06F_insert-blueprint-local.svg` | вставка заготовки, локальный черновик |
| `14-09_P1-06F_insert-blueprint-copy.svg` | вставка заготовки, копия с сервера |
| `14-10_P1-06G_apply-profile-bulk.svg` | «Применить профиль»: область, совместимость, «было → станет» |
| `14-11_P1-06G_update-from-profile.svg` | «Обновить из профиля»: режимы, происхождение значений |
| `14-12_P1-06G_inspector-origin.svg` | инспектор: состояния «из профиля», баннер устаревания |

Все двенадцать — светлая тема, русский язык, один экран на файл, генератор `docs/design/tools/profiles_svg.py` (MOCKUP_GUIDE).
