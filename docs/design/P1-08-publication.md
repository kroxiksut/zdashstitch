# P1-08 — предпросмотр, сравнение, конфликты и публикация

Статус: макеты приняты владельцем 25.10.2026; O-031 п. 6 решён в тот же день. Это design specification, не реализованный экран. Реализация — P2-03, P2-04, P2-06, P3-03, P5-02—P5-05; закрытый publisher и `VerifiedPlan` уже есть в P0-07 и этим документом не ослабляются.

Дополнение G1b: [`P1-08-addendum-republish-append.md`](P1-08-addendum-republish-append.md) — повторная публикация черновика, создавшего панель, и выбор цели для «Добавить страницу / виджеты» (P5-02—P5-05).

Входы: P1-IN-03 (статусы сопоставления), P1-IN-04 (матрица записи 7.4.16), P1-IN-06 (задержки), P1-IN-11 (владелец и доступ), O-001, O-002, O-014, O-018, O-019, O-031, P1-01C (оси состояний, guards), P1-02 (режим доступа), P1-00E/F.

Макеты (`docs/design/assets/11-publication/`, только светлая тема, генератор `docs/design/tools/publication_svg.py`):

| Файл | Подзадача | Экран |
|---|---|---|
| `11-01_P1-08E_target-access-superadmin.svg` | P1-08A/E/I | шаг «Назначение» при клонировании, Super admin: владелец, общий доступ, копия доступа |
| `11-02_P1-08I_target-access-admin.svg` | P1-08E/I | шаг «Назначение» при замене узла сети, Admin: явный выбор обновления, тип доступа меняется, список получателей — только просмотр |
| `11-03_P1-08C_preflight.svg` | P1-08C | шаг «Проверки»: одна блокировка, одно предупреждение, итог |
| `11-04_P1-08B_changes.svg` | P1-08B | шаг «Изменения»: два отдельных сравнения, фильтры, таблица по страницам, инспектор |
| `11-05_P1-08D_conflict.svg` | P1-08D | конфликт: панель изменилась на сервере, безопасные выходы («Сохранить копию», «Скопировать заново» с переигрыванием, «Новая копия») |
| `11-06_P1-08F_confirm.svg` | P1-08F | диалог подтверждения одноразового плана |
| `11-07_P1-08G_verified.svg` | P1-08G | результат: проверено повторным чтением |
| `11-08_P1-08G_unknown-outcome.svg` | P1-08G | результат неизвестен при создании копии, сверка чтением |

## P1-08A — оболочка и контекст операции

Раздел «Публикация» рабочей области — линейный путь из пяти шагов: **1 Назначение → 2 Проверки → 3 Изменения → 4 Подтверждение → 5 Запись и проверка**. Шаги видны всегда; пройденный шаг отмечен значком, шаг с ошибкой — красным значком (макет 11-05). Вернуться можно на любой пройденный шаг; переход вперёд — только кнопкой «Далее» внизу.

Под шагами всегда видна карточка контекста:

| Поле | Содержание | Если нет данных |
|---|---|---|
| Операция | намерение (`intent.*`) и краткий параметр (например, `pg-a-01 → pg-b-01`) | — |
| Источник (база) | имя панели или страницы источника и время снимка | «Нет — локальная панель» для черновиков без источника |
| Цель | «Новая панель», «Та же панель на сервере», «Панель «{name}»» | «Не выбрана» до шага 1 |
| Подключение | профиль, версия Zabbix, режим | «Автономно» или «Нет связи» — шаги показывают требования, записи нет |
| Область | число затронутых страниц и виджетов из общего | — |
| Ожидаемый результат | одно предложение по `publication.expected.*` | — |

Различия по намерениям:

| Намерение | Источник | Цель | Сравнение «на сервере» | Ожидаемый результат |
|---|---|---|---|---|
| Создать панель (локальная) | нет | новая панель | нет (показывается «нет базы на сервере») | новая панель с N страницами |
| Сохранить копию | есть | новая панель | есть, для проверки свежести | новая панель; исходная не меняется |
| Клонировать панель | есть | новая панель в том же подключении | есть | новая панель с другими узлами сети; исходная не меняется |
| Добавить страницу / виджеты | есть или нет | существующая панель | есть, по цели | новые страницы или виджеты; остальное не меняется |
| Заменить узел сети | есть | исходная панель или её копия | есть | N виджетов меняют узел сети |
| Обновить панель | есть | исходная панель или созданная из этого черновика панель (цель из сохранённой связи, дополнение P1-08 A) | есть | перечень изменений |

Намерения — по ADR-006 и `domain_model` §3 (полный перечень дополнен 26.10.2026): `CreateDashboard`, `SaveAsCopy`, `CloneDashboard`, `AppendPage`, `AppendWidgets`, `ReplaceHost`, `UpdateDashboard`. Отдельного намерения «обновить созданную» нет: это `UpdateDashboard` с целью из сохранённой связи (решение 2).

Возврат к «Структуре», «Привязкам» и редактору не теряет локальный черновик; любой возврат с правкой делает план недействительным (P1-08F).

## P1-08B — сравнение (diff)

Два **отдельных** сравнения, а не одна двусторонняя таблица:

- «Мои изменения: база → черновик» — что меняет пользователь (намеренные изменения);
- «На сервере: база → Zabbix сейчас» — что изменилось на сервере после снимка. Пусто → плашка «без изменений»; не пусто → конфликт (P1-08D).

Модель строки: объект (свойства панели, страница, виджет, поле, источник данных, ссылка между виджетами, доступ), вид изменения, «было», «станет».

| Вид | Маркер (Tabler) | Подпись |
|---|---|---|
| добавлено | `plus` | «Добавлен» |
| удалено | `minus` (красная подпись) | «Удалён» |
| изменено | `pencil` | «Изменено поле» / «Изменены N полей» |
| перемещено | `arrows-move` | «Перемещён» (страница, порядок, положение) |
| без изменений | `minus` (серый) | «Без изменений» |
| остаётся на старом узле сети (O-003) | `ban` | «Останется на старом» |

Смысл никогда не передаётся только цветом: маркер + подпись.

**Сопоставление виджетов.** По P1-IN-04 `widgetid` и `pageid` сохраняются, если их передавать; виджет без `widgetid` пересоздаётся с новым ID, а не переданная страница удаляется. Поэтому:

- для операций над существующей панелью виджеты и страницы сопоставляются **по идентификаторам из базы**; план всегда передаёт все страницы и все `widgetid`;
- для новой панели (создание, копия, клон) сравнения «на сервере» нет, а «мои изменения» показывают итоговую структуру как «добавлено»;
- любое отличие результата от черновика, не объяснимое локальной операцией, — **побочное изменение**: фильтр «Побочные», и оно же блокирует план в P1-08C.

Повторная отправка неизменённого виджета на изменённой странице — требование контракта записи (`publication.contract.resend_widget_type`: план передаёт все виджеты страницы с их `widgetid`, даже если менялся один), она показывается в инспекторе строки как пояснение, а не как изменение.

Применение профиля оформления (P1-06G) показывается в «Моих изменениях» строкой «Применён профиль «{profile}»: полей — N» (`profiles.apply.changes_entry`) с раскрытием по виджетам и полям «база → черновик»; при публикации это обычные изменения полей. Служебные операции `apply_style_profile` с `service: true` (метка у источника после извлечения) в списке не показываются и ничего не пишут.

Фильтры: «Все», «Изменены», «Остаются» (на старом узле сети), «Без изменений», «Побочные», «Доступ» (с числом). Поиск по имени виджета, коду поля и значению. Таблица сгруппирована по страницам (свёрнутые группы показывают «N из M виджетов изменены»), затем строки «Свойства панели» и «Владелец и доступ». Изменения доступа (P1-08I) — отдельной группой, не смешиваются с виджетами.

Инспектор выбранной строки: тип виджета (с кодом типа), страница и геометрия; изменённые поля (код поля моноширинным, было → станет); результат сопоставления источников (P1-IN-03: `exact/compatible/ambiguous/missing/unresolved_lld/denied`); что не меняется; число неизвестных полей, которые передаются как есть; раскрывающиеся «Технические сведения» (`widgetid`, `pageid`, коды полей).

Большая панель: группы страниц сворачиваются, строки виртуализируются (стресс 20 страниц × 60 виджетов, P1-00H §4); фокус и выбор сохраняются при фильтрации.

## P1-08C — предварительные проверки

Проверки группируются по шагу, где их исправляют («исправляется: Подключения / Привязки / Структура / Публикация»). У каждой: статус, текст по коду, время, при необходимости кнопки исправления.

| Статус | Значок | Влияние |
|---|---|---|
| пройдено | `circle-check` | — |
| предупреждение | `alert-triangle` | показывается в подтверждении как «просмотрено»; не блокирует |
| блокирует | `circle-x` | план не создаётся |
| не проверялось | `circle-dashed` | для обязательной проверки — блокирует; причина показывается |
| устарело | `circle-dashed` + «устарело» | проверка повторяется автоматически |

Состав (все обязательные):

| Группа | Проверка | Блокирует, если |
|---|---|---|
| Подключение и права | подключение, версия, адаптер; режим `read_write`; права учётной записи на цель | не подключено, `read_only`, версия без адаптера, нет права записи панели (`publication.finding.dashboard_not_editable`) |
| Источник | свежесть базы (P1-01C ось E) | `stale` без повторной проверки, `conflict`, `source_missing` для обновления |
| Виджеты и поля | для каждого затронутого виджета и поля — флаги реестра P0-09 `edit`/`create` для точной версии | поле поддержанного типа без лабораторного контракта затронуто операцией. Виджеты неподдержанных для замены типов со ссылкой на старый узел сети не блокируют (O-003, P2-03C): они попадают в список «Останется на старом узле», и блокирует только неподтверждённый список |
| Виджеты при создании из переносимого черновика (P1-11 G1b, M13; уточнено 25.10.2026 после ревью P0-10) | `item`, `svggraph`: обязательные поля есть (`itemid.0`; хотя бы один `ds.N` с узлами и элементами); `clock`, `problems`: пустой набор полей допустим — умолчания применяет сервер (лаборатория 7.4.16); остальные распознанные типы (`gauge`, `tophosts`, …): непустой набор полей из источника, у всех полей известный тип поля Zabbix 0—13, все ссылки восстановлены из привязок, неизвестных полей нет — имена полей в реестре знать не обязательно (лаборатория: `gauge`, `tophosts` создаются из заново привязанных полей) | не хватает обязательных полей (`portable_widget_required_fields_missing`); пустой или урезанный набор полей распознанного типа (`portable_widget_incomplete`); хотя бы одна ссылка «нужна привязка» или поле неизвестного типа — сервер такой виджет **примет** (`gauge` без элемента данных создаётся), поэтому блокирует ZDashstitch |
| Привязки | сопоставление всех затронутых ссылок (P1-IN-03) | `ambiguous`, `missing`, `unresolved_lld`, `denied` |
| Сохранение и геометрия | не затронутое передаётся с ID; сетка 72 колонки без выходов и перекрытий; минимум одна страница; уникальность `reference` (сборка) | побочные изменения, выход за сетку, перекрытие |
| Назначение | имя новой панели свободно среди видимых; цель в том же подключении | имя занято среди видимых панелей |
| Владелец и доступ | P1-08I | правка лишает учётную запись права записи; выбран владелец без прав Super admin |

Итог справа: «Готово к подтверждению» / «Публикация заблокирована», счётчики по статусам, «Проверить снова». Проверки выполняются автоматически при открытии шага и повторяются publisher'ом перед записью. Уникальность имени до отправки не доказывает, что имя свободно (P1-IN-04: невидимая панель с тем же именем) — отказ сервера показывается кодом `name_taken_maybe_hidden`.

## P1-08D — устаревание и конфликт

Свежесть проверяется: при открытии предпросмотра, при переходе к подтверждению и непосредственно перед записью (publisher, P0-07). Состояния — P1-01C ось E.

При `Remote ≠ Base` (макет 11-05):

- план недействителен, шаги 3–5 недоступны, обновление остановлено; черновик не меняется;
- открывается сравнение «На сервере: база → Zabbix сейчас»; строки, пересекающиеся с локальными изменениями, выделены и подписаны;
- Zabbix API не сообщает, кто и когда изменил панель, — так и пишется;
- выходы: «Сохранить копию» (основной); «Скопировать заново» (`workcopy.refetch.action`, одна подпись и один ID с P1-03E; B-01) — новая база читается как новый снимок, локальные операции переигрываются на ней на экране 07-02, конфликты решаются вручную, черновик остаётся на прежней базе до «Принять новую базу»; «Новая копия» (`copy.action.new_copy`, решение В-20) — отдельная рабочая копия с нового снимка без переноса правок, этот черновик не меняется; «Проверить снова»; «Обновить поверх» показан недоступным с причиной. Автоматического слияния в MVP нет (O-014).

Потеря связи или режим `read_only` во время шага: проверки получают статус «не проверялось», запись недоступна, черновик и выбор цели сохраняются; после восстановления проверки повторяются.

## P1-08E — действие и цель

| Действие | Когда предлагается | По умолчанию |
|---|---|---|
| Создать новую панель | локальная панель, клонирование, сборка из страниц | да, если это единственный вариант |
| Сохранить копию | копия с сервера (любое намерение) | нет |
| Добавить страницу / виджеты в панель | намерения AppendPage/AppendWidgets | — (цель выбирается из списка панелей подключения) |
| Обновить исходную панель | копия с сервера при свежем источнике и праве записи | **никогда**: пользователь выбирает явно |

Правила:

- Клонирование создаёт отдельную панель внутри того же подключения; «Обновить существующую панель» для него показан недоступным с причиной.
- Цель всегда принадлежит активному подключению; панель из другого подключения выбрать нельзя. Смена подключения сбрасывает цель.
- Для обновления исходной панели — предупреждение «Панель увидят изменённой все, у кого есть к ней доступ».
- Блок «Не изменяется» перечисляет нетронутые виджеты, страницы, свойства, владельца и доступ, а также объекты Zabbix, которые не меняются никогда.
- Имя новой панели обязательно, проверяется среди видимых панелей; пояснение, что окончательно проверяет сервер.

## P1-08I — владелец и общий доступ (O-031)

| Элемент | Super admin | Admin / User |
|---|---|---|
| Владелец новой панели | выбор из `user.get`, по умолчанию «вы» | поле не показывается; `userid` не отправляется |
| Владелец существующей панели | не меняется, если не меняли явно | не показывается и не передаётся |
| Тип доступа новой панели | «Личная» / «Общая» | то же |
| Получатели | пользователи и группы из видимых (`user.get`, `usergroup.get`), «Только чтение» / «Чтение-запись» | то же, из видимых ему |
| Клон: «Скопировать доступ исходной панели» | включено, полный список | включено, со списком и предупреждением «скопирована только видимая вам часть» |
| Копия с сервера: «Восстановить исходных владельца и доступ» | доступно | только видимая часть, с тем же предупреждением; владелец — нет |
| Тип доступа существующей панели | можно менять | **можно менять**: запрос содержит только `private`, списки сохраняются (O-031 п. 6, проверено) |
| Список получателей существующей панели | можно менять | **только просмотр видимой части** (O-031 п. 6) |

Подписи «Только чтение» и «Чтение-запись» у получателей — официальные термины прав доступа Zabbix и не путаются с режимом подключения: рядом всегда имя получателя.

Списки `users` и `userGroups` передаются, только если меняли сам список; смена одного типа доступа передаёт один `private` — и у Super admin тоже. Перевод в «Общая» показывает «Панель увидят все пользователи Zabbix». Совет «изменить в Zabbix» не даётся: веб-интерфейс Zabbix при сохранении доступа тоже передаёт оба списка целиком (исходники 7.4.16).

Блокируется до отправки: правка, которая лишает учётную запись подключения собственного права записи (P1-IN-11), и любой `userid` от не-Super admin. Обновление без явной правки доступа не передаёт `private`, `userid`, `users`, `userGroups`. Изменения доступа — отдельная группа в сравнении (P1-08B). Запись доступа включается только после лабораторного контракта P5-08; до этого блок показывается, а проверка «Владелец и доступ» даёт «не проверялось: возможность не подтверждена» для любой правки доступа.

## P1-08F — подтверждение и жизненный цикл плана

Кнопка «Подтвердить запись…» на шаге 4 активна только при выполнении P1-01C §7. Диалог (макет 11-06) показывает: вопрос с числом изменений и точным именем панели; подключение, версию и режим; новая или существующая панель; действие API и число запросов (`dashboard.create` / `dashboard.update`, всегда один); что изменяется и что не изменяется; принятые предупреждения; идентификатор плана и время создания; повторное чтение перед отправкой; условия недействительности.

- Подтверждение передаёт серверу приложения только непрозрачный `plan_id`, который пользователь видел; сам `VerifiedPlan` не покидает процесс Rust, не сериализуется в HTTP, файл или журнал.
- План одноразовый: после подтверждения он потребляется, повторное нажатие невозможно (кнопка становится недоступной до результата).
- План становится недействительным при любом из событий P1-01C §6 (правка черновика, смена цели, намерения, учётных данных, режима, контекста подключения, смена привязок, `stale`/`conflict`, перезапуск, завершение сеанса). Тогда диалог закрывается с сообщением `publication.plan.invalidated`, где `{reason}` — один из `publication.plan.reason.*`: `draft_changed`, `target_changed`, `connection_changed`, `bindings_changed`, `source_changed`, `app_restarted`, `session_ended`, `unknown` (с `{code}`), и путь возвращается на шаг 2.
- Отдельного таймера у плана нет: безопасность обеспечивает обязательное повторное чтение перед записью; давность проверки видна в диалоге.
- Фокус в диалоге: на «Отмена»; Esc — отмена; после закрытия фокус возвращается на кнопку подтверждения.

## P1-08G — запись, повторное чтение и неопределённый результат

Этапы на шаге 5 (макеты 11-07, 11-08):

| Этап | Журнал (`JournalState`) | Что видно | Отмена |
|---|---|---|---|
| Чтение перед записью | `Confirmed` | совпадение с базой и время | можно; ничего не отправлено |
| Запись | `Submitting` (запись в журнал на диск до отправки, O-018) | «Отправка одного проверенного запроса…» | **нельзя** |
| Повторное чтение | — | до трёх чтений без повторной записи (P0-07) | нельзя |
| Структура | `Verified` / `FailedAfterResponse` | изменено как в плане; нетронутое и ID на месте; неизвестные поля на месте | — |
| Источники данных | — | ссылки указывают на нужные элементы данных | — |
| Отображение | — | «Не проверяется автоматически: откройте панель в Zabbix» | — |

Итоги:

| Итог | Баннер | Дальше |
|---|---|---|
| `Verified` | «Проверено повторным чтением» + «Проверьте отображение панели в Zabbix» | «Открыть в Zabbix», журнал; снимок после записи становится новой базой черновика |
| `FailedBeforeSend` | «Не отправлено» с кодом | исправить и новый предпросмотр |
| `Conflict` | P1-08D | P1-08D |
| `FailedAfterResponse` | «Ошибка после ответа сервера» | только сверка чтением, затем новый план |
| `UnknownOutcome` | «Результат публикации неизвестен. Не повторяйте…» | только «Проверить результат» (чтение); новая запись в эту цель заблокирована |
| `ReconciledVerified` / `ReconciledFailed` | «Сверено: записано» / «Сверено: не записано» | продолжать / новый предпросмотр |

Баннеры итогов — ID `publication.state.*` (m-12): `verified`, `failed_before_send`, `conflict`, `failed_after_response`, `unknown_outcome`, `reconciled_verified`, `reconciled_failed`; состояния пути до записи — `planned`, `previewed`, `confirmed`. Тексты «Сверено: записано» / «Сверено: не записано» — единые для P1-01C §6, этого документа и макета 11-08 (прежние «Сверено: изменения записаны / не записаны» в P1-01C приводятся к ним).

Сверка для создания (копия, клон, новая панель) ищет панель по точному имени и отпечатку структуры: найдена одна с ожидаемой структурой → «Сверено: записано»; не найдена → «Сверено: не записано»; несколько или структура другая → «Нужна проверка вручную» (открыть в Zabbix), запись остаётся заблокированной. Повторной отправки нет ни в одном исходе. Сверка для обновления сравнивает панель с ожидаемым отпечатком и с базой.

Задержки (P1-IN-06, лаборатория): create ≈ 0,3 с, update ≈ 0,1 с, read-back ≈ 0,07 с — индикатор этапов без процентов; через 10 с без ответа показывается «Ожидание ответа сервера…» без кнопки отмены.

Незавершённая операция (`Submitting` без итога, `UnknownOutcome`) переживает перезапуск и показывается баннером `startup.recovery.found` до любых новых записей (P1-01C §6).

Итог создания (копия, клон, новая панель) дополнительно показывает сравнение «исходная → ожидаемая → созданная» по страницам и виджетам: созданная панель сверяется с ожидаемым результатом плана, а исходная показывается для справки и не меняется (вход P3-03C). Расхождение ожидаемой и созданной — «Ошибка после ответа сервера» со сверкой, без повторной записи.

## P1-08H — handoff для P2-03/P2-04/P2-06, P3-03, P5-02—P5-05

Данные предпросмотра для интерфейса: `intent`, `source` (`kind`, имя, время снимка, отпечаток — только в технических сведениях), `target` (`kind`: `new`/`existing`, имя, подключение), `connection` (`profile`, `server_version`, `mode`), `scope` (страницы/виджеты: затронуто/всего), `freshness` (ось E + `checked_at`), `checks[]` (`code`, `group`, `status`, `params`, `fix_step`), `diff.local[]` и `diff.remote[]` (`object_kind`, `object_ref`, `change`, `before`, `after`, `intended`), `access` (P1-08I), `plan` (`plan_id`, `created_at`) — только при отсутствии блокировок.

| Команда | Предусловия | Успех | Ошибки (коды) |
|---|---|---|---|
| `publication.preview` | черновик загружен | данные выше; план — только без блокировок | `not_connected`, `read_only`, `source_conflict`, коды проверок |
| `publication.set_target` | — | цель сохранена в черновике, план недействителен | `target_other_connection`, `target_not_writable` |
| `publication.set_access` | `read_write`; P5-08 для записи доступа | изменения доступа в предпросмотре | `owner_requires_super_admin`, `self_write_removed`, `recipient_not_visible` |
| `publication.recheck` | подключено | новые проверки и свежесть | — |
| `publication.confirm` | `plan_id` совпадает с показанным; P1-01C §7 | события `publication.stage` по этапам | `plan_invalidated`, `plan_mismatch`, `source_conflict` |
| `publication.reconcile` | журнал `UnknownOutcome`/`FailedAfterResponse` | `ReconciledVerified` / `ReconciledFailed` / `manual_check_required` | `not_connected` |

Live-region: смена статуса проверки и каждого этапа записи объявляется вежливо; блокировка, конфликт и неизвестный результат — ассертивно. Клавиатура: шаги — ссылки в порядке табуляции; таблица сравнения — сетка со стрелками, Enter открывает инспектор; фильтры — группа переключателей. Поля журнала операции для аудита: `operation_id`, `plan_id`, `intent`, профиль подключения, версия сервера, цель (имя и ID), отпечатки базы и ожидаемого результата, состояния с временем, число отправок (всегда 0 или 1). Секретов и полных ответов сервера в журнале нет.

Негативные случаи для тестов P2: `read_only` → ноль записей; `stale`/`conflict` → ноль записей; блокировка → план не создаётся; подменённый или повторный `plan_id` → отказ; правка черновика после предпросмотра → `plan_invalidated`; `UnknownOutcome` → новая запись в ту же цель отклоняется до сверки; не-Super admin → `userid` отсутствует в запросе.

## Новые message IDs

Используются также существующие `publication.*` из P1-00F и P1-01C.

| ID | EN source | RU |
|---|---|---|
| `publication.step.target` | Target | Назначение |
| `publication.step.checks` | Checks | Проверки |
| `publication.step.changes` | Changes | Изменения |
| `publication.step.confirm` | Confirmation | Подтверждение |
| `publication.step.write` | Write and verify | Запись и проверка |
| `publication.context.source` | Source (base) | Источник (база) |
| `publication.context.scope` | Scope | Область |
| `publication.context.no_base` | None — local dashboard | Нет — локальная панель |
| `publication.expected.title` | Expected result: {summary} | Ожидаемый результат: {summary} |
| `publication.target.create` | Create new dashboard | Создать новую панель |
| `publication.target.update_source` | Update the source dashboard {dashboard_name} | Обновить исходную панель «{dashboard_name}» |
| `publication.target.choose_explicitly` | Choose explicitly: there is no default. | Выберите явно: варианта по умолчанию нет. |
| `publication.target.update_visible_to_all` | Everyone with access to this dashboard will see it changed. | Панель увидят изменённой все, у кого есть к ней доступ. |
| `publication.target.clone_no_update` | Not available for cloning: the source dashboard stays as is. | Недоступно для клонирования: исходная панель остаётся как есть. |
| `publication.target.name` | New dashboard name | Имя новой панели |
| `publication.target.name_free_visible` | Free among visible dashboards | Среди видимых панелей свободно |
| `publication.target.name_server_check` | Dashboard names are unique on the whole server, including dashboards you cannot see. The server makes the final check on write. | Имя панели уникально на всём сервере, включая панели, которые вам не видны. Окончательно это проверяет сервер при записи. |
| `publication.untouched.title` | Not changed | Не изменяется |
| `publication.access.title` | Owner and sharing | Владелец и доступ |
| `publication.access.owner` | Owner | Владелец |
| `publication.access.owner_super_admin_only` | Only Super admin can choose the owner. | Выбор владельца доступен только Super admin. |
| `publication.access.private` | Private | Личная |
| `publication.access.shared` | Shared | Общая |
| `publication.access.copy_source` | Copy sharing of the source dashboard | Скопировать доступ исходной панели |
| `publication.access.copy_partial` | Only the part of the sharing list visible to you was copied. | Скопирована только видимая вам часть списка доступа. |
| `publication.access.restore_source` | Restore the original owner and sharing | Восстановить исходных владельца и доступ |
| `publication.access.add_recipient` | Add user or group | Добавить пользователя или группу |
| `publication.access.recipients_visible_only` | Recipients can be chosen only from those visible to the connection account. | Получателей можно выбрать только из видимых учётной записи подключения. |
| `publication.access.view_only` | View only | Только просмотр |
| `publication.access.partial_list.title` | The list may be incomplete | Список может быть неполным |
| `publication.access.partial_list.body` | Without Super admin rights your account does not see all recipients, and saving a recipient list replaces it entirely, removing the hidden ones — in ZDashstitch and in the Zabbix web interface alike. Only Super admin can change the list. You can still switch Private/Shared: the list is not sent and stays intact. | Учётная запись без прав Super admin видит не всех получателей, а сохранение списка заменяет его целиком и удаляет невидимых — и в ZDashstitch, и в веб-интерфейсе Zabbix. Список может изменить только Super admin. Тип «Личная/Общая» менять можно: список при этом не передаётся и сохраняется. |
| `publication.access.public_all_users` | All Zabbix users will be able to view this dashboard. | Панель увидят все пользователи Zabbix. |
| `publication.access.list_view_only` | Recipients: view only | Получатели: только просмотр |
| `publication.access.self_write_removed` | This change would remove your own write access. | Эта правка лишит вас права записи панели. |
| `publication.check.group.connection` | Connection and permissions | Подключение и права |
| `publication.check.group.source` | Source | Источник |
| `publication.check.group.widgets` | Widgets and fields | Виджеты и поля |
| `publication.check.group.bindings` | Bindings | Привязки |
| `publication.check.group.preservation` | Preservation and geometry | Сохранение и геометрия |
| `publication.check.fixed_in` | Fixed in: {section} | Исправляется: {section} |
| `publication.check.widget_kept_on_source` | {widget_name} ({widget_type}) will stay on {host}: changing this widget type is not supported. | «{widget_name}» ({widget_type}) останется на {host}: изменение типа не поддерживается. |
| `publication.check.confirm_kept` | Confirm the list of widgets that stay on the old host | Подтвердите список виджетов, которые останутся на старом узле |
| `publication.check.action.confirm_kept` | Confirm list ({count}) | Подтвердить список ({count}) |
| `publication.check.summary.ready` | Ready to confirm | Готово к подтверждению |
| `publication.check.summary.plan_not_built` | No write plan is built while blockers remain. | План записи не создаётся, пока есть блокировки. |
| `publication.check.recheck` | Check again | Проверить снова |
| `publication.diff.tab.local` | My changes: base → draft | Мои изменения: база → черновик |
| `publication.diff.tab.remote` | On server: base → Zabbix now | На сервере: база → Zabbix сейчас |
| `publication.diff.no_changes` | No changes | Без изменений |
| `publication.diff.filter.side_effects` | Side effects | Побочные |
| `publication.diff.filter.kept` | Stay | Остаются |
| `publication.diff.change.field` | {count, plural, one {# field changed} other {# fields changed}} | {count, plural, one {Изменено # поле} few {Изменены # поля} many {Изменено # полей} other {Изменено # поля}} |
| `publication.diff.change.kept` | Stays on the old host | Останется на старом |
| `publication.diff.page_summary` | {total, plural, one {{changed} of # widget changed} other {{changed} of # widgets changed}} | {total, plural, one {{changed} из # виджета изменено} few {{changed} из # виджетов изменено} many {{changed} из # виджетов изменено} other {{changed} из # виджета изменено}} |
| `publication.diff.unknown_fields_kept` | Unknown fields ({count}) are passed as is. | Неизвестные поля ({count}) передаются как есть. |
| `publication.diff.matching_by_id` | Widgets are matched by Zabbix IDs from the base; they are sent and preserved on write. | Виджеты сопоставляются по идентификаторам Zabbix из базы: при записи они передаются и сохраняются. |
| `publication.conflict.overlap` | {count, plural, one {Overlaps your changes: # widget} other {Overlaps your changes: # widgets}} | {count, plural, one {Пересекается с вашими изменениями: # виджет} few {Пересекается с вашими изменениями: # виджета} many {Пересекается с вашими изменениями: # виджетов} other {Пересекается с вашими изменениями: # виджета}} |
| `publication.conflict.no_author` | The Zabbix API does not report who changed the dashboard or when. | Кто и когда изменил панель, Zabbix API не сообщает. |
| `publication.conflict.refetch_hint` | A new snapshot from the server; your edits are replayed on the new base, conflicts are decided by hand. | Новый снимок с сервера; ваши правки переигрываются на новой базе, конфликты решаете вручную. |
| `publication.conflict.new_copy_hint` | A separate copy from the new snapshot without your edits; this draft stays as is. | Отдельная копия с нового снимка без ваших правок; этот черновик остаётся как есть. |
| `publication.conflict.overwrite_unavailable` | Overwrite is not available: it would discard other changes. There is no automatic merge. | Обновить поверх нельзя: запись затёрла бы чужие изменения. Автоматического слияния нет. |
| `publication.confirm.api_action` | One {method} request | Один запрос {method} |
| `publication.confirm.plan_once` | Valid once; built at {time} | Действует один раз; создан в {time} |
| `publication.confirm.reread` | The dashboard is read again before sending. If it changed after the check at {time}, nothing is written. | Перед отправкой панель будет прочитана ещё раз. Если она изменилась после проверки в {time}, запись не выполнится. |
| `publication.confirm.no_cancel_after_send` | After sending, the write cannot be cancelled; the result is checked by reading back. | После отправки отменить запись нельзя; результат проверяется повторным чтением. |
| `publication.action.confirm_write` | Write to Zabbix | Записать в Zabbix |
| `publication.plan.invalidated` | The plan is no longer valid: {reason}. Build a new preview. | План больше не действует: {reason}. Создайте новый предпросмотр. |
| `publication.plan.reason.draft_changed` | the draft changed after preview | черновик изменён после предпросмотра |
| `publication.plan.reason.target_changed` | the target or action changed | изменились цель или действие |
| `publication.plan.reason.connection_changed` | the connection, mode or credentials changed | изменились подключение, режим или учётные данные |
| `publication.plan.reason.bindings_changed` | bindings changed | изменились привязки |
| `publication.plan.reason.source_changed` | the dashboard on the server changed or must be checked again | панель на сервере изменилась или требует повторной проверки |
| `publication.plan.reason.app_restarted` | the application was restarted | приложение перезапущено |
| `publication.plan.reason.session_ended` | the local session ended | локальный сеанс завершён |
| `publication.plan.reason.unknown` | unknown reason ({code}) | неизвестная причина ({code}) |
| `publication.finding.dashboard_not_editable` | No permission to change the target dashboard | Нет прав на изменение целевой панели |
| `publication.contract.resend_widget_type` | resending an unchanged {widget_type} widget on a changed page | повторная отправка неизменённого виджета {widget_type} на изменённой странице |
| `publication.state.planned` | Plan built | План создан |
| `publication.state.previewed` | Preview built | Предпросмотр создан |
| `publication.state.confirmed` | Confirmed, reading before write | Подтверждено, чтение перед записью |
| `publication.state.verified` | Verified by reading back | Проверено повторным чтением |
| `publication.state.conflict` | Changed on the server | Изменена на сервере |
| `publication.state.failed_after_response` | Error after the server response | Ошибка после ответа сервера |
| `publication.state.unknown_outcome` | Publication result unknown | Результат публикации неизвестен |
| `publication.state.reconciled_verified` | Checked: written | Сверено: записано |
| `publication.stage.pre_read` | Read before write | Чтение перед записью |
| `publication.stage.write` | Write | Запись |
| `publication.stage.read_back` | Read back | Повторное чтение |
| `publication.stage.structure` | Structure | Структура |
| `publication.stage.sources` | Data sources | Источники данных |
| `publication.stage.display` | Display | Отображение |
| `publication.stage.display_not_checked` | Not checked automatically: open the dashboard in Zabbix | Не проверяется автоматически: откройте панель в Zabbix |
| `publication.stage.waiting` | Waiting for the server response… | Ожидание ответа сервера… |
| `publication.result.base_advanced` | The snapshot after writing became the new base of this draft. | Снимок после записи стал новой базой черновика. |
| `publication.reconcile.found_one` | One dashboard, structure as planned | Одна панель, структура как в плане |
| `publication.reconcile.found_none` | None | Ни одной |
| `publication.reconcile.ambiguous` | Several, or a different structure | Несколько или структура другая |
| `publication.reconcile.manual` | Manual check required | Нужна проверка вручную |
| `publication.reconcile.write_blocked` | New writes to this target are blocked until the result is checked. | Новая запись в эту цель заблокирована, пока результат не сверен. |
| `publication.action.open_in_zabbix` | Open in Zabbix | Открыть в Zabbix |
| `publication.action.open_journal` | Operation journal | Журнал операции (единый текст для P1-01G, 11-07 и 11-08; m-20) |
| `errors.publication.name_taken_maybe_hidden` | The name is taken, possibly by a dashboard you cannot see. | Имя занято — возможно, панелью, которая вам не видна. |
| `errors.publication.owner_requires_super_admin` | Only Super admin can set the dashboard owner. | Владельца панели может задать только Super admin. |

Параметры `{dashboard_name}`, `{widget_name}`, `{host}` — данные сервера (не переводятся); `{method}`, `{widget_type}`, `{code}` — технические идентификаторы; `{reason}`, `{section}` — уже локализованные тексты других сообщений, передаются как параметр типа `text` (контракт UI §2.1; M-04); `{time}` — локализованное время. `publication.plan.reason.*`, `publication.finding.dashboard_not_editable` и `publication.contract.resend_widget_type` уже есть в `lang/*/publication.json` (26.10.2026); `publication.state.idle`, `.submitting`, `.failed_before_send`, `.reconciled_failed` определены в P1-01C. Прежние `publication.action.publish`, `publication.result.verified`, `publication.conflict.title` из P1-00F («Опубликовать…», «…после предпросмотра») устарели (m-13): действие — «Записать в Zabbix» (`publication.action.confirm_write`), итог — `publication.state.verified`.

## Проверки приёмки P1-08

- Каждое намерение показывает точный ожидаемый результат и то, что не изменяется.
- `stale`/`conflict` дают ноль записей и безопасный выход без потери черновика; автоматического слияния нет.
- Блокировку нельзя подтвердить: план не создаётся.
- Запись доступна только в `read_write` через актуальный одноразовый план; `VerifiedPlan` не покидает процесс.
- Неизвестный результат не повторяется; доступна только сверка чтением.
- Результат не называется успешным без повторного чтения; отображение явно не проверено.
- Владелец не передаётся от не-Super admin; доступ не передаётся без явной правки; смена только типа доступа передаёт один `private`.

O-031 п. 6 решён 25.10.2026: не-Super admin на существующей панели меняет только тип «Личная/Общая», список получателей — только просмотр (лабораторная проверка — `backend_inputs_ru.md`, P1-IN-11).
