# Контракт адаптера Zabbix v1 — требования для кодового агента

**Основание:** ADR-011. **Статус:** эскиз публичного контракта Rust для разработки P0, не готовый исполняемый код. Подлежит уточнению на реальном API 7.4.

**Поправка O-025/O-026:** `PortableProject` ниже — legacy `project/v1` container. Будущий core contract обязан принимать typed Dashboard/Page project либо Dashboard/Page Workcopy и передавать в application отдельные source, target и operation scope; adapter по-прежнему не решает, какой объект является целью.

## 1. Разделение ответственности

- Core хранит typed Dashboard/Page project, legacy `PortableProject` migration boundary, `MetricCatalog`, `HostRole`, `LogicalWidget`, `Layout`. Не знает `dashboardid`, `itemid`, `field.name`, HTTP и ветку Zabbix.
- Application хранит намерение пользователя (`CreateDashboard`, `AppendPage`, `AppendWidgets`, `CloneDashboard`, `ReplaceHost`), выбор целей, `ResolutionOutcome`, `PublishPlan` и журнал. Отдельно вычисляет семантические требования операции.
- Zabbix transport выполняет JSON-RPC, TLS и ошибки связи. Transport не интерпретирует содержимое страниц и не решает, что можно редактировать.
- Версионный адаптер знает сырые DTO своей ветки, извлекает зависимости, сообщает возможности и строит запрос для единой операции публикации.
- Importer экспортов шаблонов работает с версиями **входных файлов** независимо от целевого API; возвращает `MetricCatalog` с `source.export_format_version` (файл) или `source.source_server_version` (API), либо без версии для ручного каталога.

## 2. Объекты между слоями (псевдоконтракты Rust)

```rust
// Псевдокод: имена и сигнатуры примерные, не готовый compilable Rust. См. ADR-012 о закрытом конструкторе VerifiedPlan.
struct DetectedZabbixVersion {
    original: String,    // "7.4.3", "8.0.0alpha1"
    major: u32,
    minor: u32,
    patch: Option<u32>,
    prerelease: Option<String>,
}

enum CapabilityState {
    Verified,      // запись допускается лишь после проверенных сценариев
    Experimental,  // изучение/лаборатория
    Unsupported,
    Unknown,
    NotApplicable,
}

enum DashboardOperation {
    Read,
    CreateGlobal,
    AppendPage,
    AppendWidgets,
    Clone,
    ReplaceHost,
    // TemplateDashboard — вне MVP
}

struct CapabilityCheck {
    operation: DashboardOperation,
    widget_kind: Option<String>,
    state: CapabilityState,
    reason: String,
    evidence_test_ids: Vec<String>,
}

struct LayoutConstraints {
    columns: u16,
    rows: u16,
    // Другие ограничения заполняются по проверенной версии.
}

// Internal only — NEVER output in portable YAML.
struct VersionedDashboardSnapshot {
    connection_id: String,
    server_version: DetectedZabbixVersion,
    adapter_id: String,
    raw_api_document: serde_json::Value,
    fingerprint: String,
}

struct CompatibilityReport {
    server_version: DetectedZabbixVersion,
    adapter_id: Option<String>,
    checks: Vec<CapabilityCheck>,
    blockers: Vec<String>,
}
```

Интерфейсы адаптеров разделяются по чтению и построению допустимого запроса. Публикация вызывается **только** после формирования `VerifiedPlan` с версией, fingerprint, набором ID и основаниями проверок. `VersionedDashboardSnapshot` никогда не копируется в `dashboard.update` целиком.

## 3. Матрица поддержки по состоянию на старт P0

| Адаптер | Чтение | Запись | Импорт файлов | Состояние |
|---|---|---|---|---|
| Zabbix 7.4 | Реализовать и испытать | Только после P0 | YAML/JSON/XML 7.4 в MVP | Целевой |
| Zabbix 7.0 LTS | Позднее | Позднее | Позднее | Не реализован |
| Zabbix 6.0 LTS | Позднее | Позднее | Позднее | Не реализован |
| Zabbix 8.0 | После стабильного релиза и проверок | Позднее | Позднее | Документация на 23.10.2026 помечена devel |
| Zabbix 5.4 / 5.0 | Позднее | Позднее | Позднее | 5.0 требует иной модели страниц |

**Не смешивать:** `MetricCatalog` из 6.0 может пройти проверку схемы v1 уже сейчас, но программа **не обещает** самостоятельно разобрать сырой файл экспорта 6.0 до появления соответствующего экспортного парсера. Аналогично наличие адаптера чтения не означает поддержку записи.

## 4. Контракт выбора адаптера

1. Вызываем `apiinfo.version` без авторизации и определяем версию API.
2. Ищем адаптер с явно зарегистрированной совместимой веткой и проверенными возможностями.
3. Аутентифицируемся способом, разрешённым этим адаптером (пользователь/токен; MFA учитывать).
4. Для выбранного действия формируем `CompatibilityReport` по **явным source/target, root kind, operation scope и содержимому целевого Dashboard/Page**, а не только по номеру версии.
5. Если хотя бы один необходимый виджет/тип зависимости или операция не имеет статуса `Verified`, запись блокируется. Устаревшие типы не переводить молча.
6. Если между предпросмотром и отправкой обнаружилась смена версии или конфигурации, исходный план недействителен.

## 5. Как добавить адаптер 6.0 без изменения UI и core

1. Добавить `zabbix/adapters/v6_0/` с модулями dashboard, widget codecs, references и правилами auth.
2. Зарегистрировать его в `AdapterRegistry` с независимой матрицей возможностей.
3. Добавить fixtures реальных панелей 6.0 и испытания каждой поддерживаемой операции.
4. При необходимости написать `template_import/zabbix_exports/v6_0` для файлов экспорта, **не меняя** `MetricCatalog`.
5. Проверить один и тот же `DB Pilot` на 7.4 и 6.0; если какой-либо тип/макет не воспроизводится, выводить корректный блокирующий отчёт, не удалять данные.
6. Подтвердить, что во всех остальных пакетах не появилось условий вида `if version == 6.0`.

## 6. Автоматические проверки границ

В CI предусмотреть тесты/проверки импорта пакетов, запрещающие `core -> zabbix`, `ui -> raw Zabbix API`; проверять отсутствие ID в переносимом YAML и наличие адаптера/capability gate для каждой операции записи. Жёсткая проверка по строковым совпадениям версий может давать ложные срабатывания: анализировать границы зависимостей и контракты, а не запрещать упоминание версии в документации/интерфейсе диагностики.

## Дополнение v0.6: допуск и форматы

1. `CompatibilityPolicy` хранит семейство версии, диапазон исправлений, подтверждённые кодеки и ID контрактных тестов. Допуск к записи: **версия ∩ конкретная операция ∩ виджеты/поля/ссылки ∩ права ∩ актуальность данных**. Любое `unknown/experimental` в критической зависимости запрещает запись.
2. В `application::publishing` конструктор `VerifiedPlan` не экспортируется как общедоступный `new()` и не десериализуется из HTTP/файла. UI получает только непрозрачный идентификатор плана; подтверждение вызывает повторную проверку его связи с подключением, объектом, снимком, версией и входными привязками.
3. Для неизвестного patch не используем ни безусловный допуск, ни список разрешённых чисел. Нужен доказанный диапазон и runtime-проверка. Чужие major/minor не допускают записи.
4. `PortableProject` и `MetricCatalog` не содержат Zabbix ID; `DeploymentState` только локальный, только подтверждённый результат. Старый `source.zabbix_version` — входной переходный псевдоним.
5. Идентичность подключения включает неизменяемый `connection_id`, fingerprint адреса/контекста сервера и проверку перед публикацией; смена цели инвалидирует план и deployment bindings.
