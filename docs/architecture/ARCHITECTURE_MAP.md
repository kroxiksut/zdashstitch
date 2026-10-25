# Архитектурная карта ZDashstitch

**Статус:** проектируемая архитектура, а не готовые Rust-модули. Нормативные детали — ADR-001—013, схемы и контракт адаптера.

```text
Imported template files ─→ versioned export parsers ─→ MetricCatalog ┐
Live Zabbix API ─→ v7_4 adapter ─→ Dashboard/Page Snapshot ─→ Workcopy ├─→ typed Dashboard/Page project ↔ Svelte UI
                                                                  │    │
                                                                  └────┴─→ application intents / resolver / preflight
                                                                                         ↓
Live Zabbix API ← closed publisher ← VerifiedPlan ← explicit source + target + operation scope
                          ↑
           LocalBindings, RawSnapshot, Workcopy, DeploymentState (never exported as DATA-ONLY)
```

**Пакеты (цель):** `core` (Dashboard/Page/Widget, типизированные проекты, каталог, роли и геометрия); `template_import` (импорт форматов без сети); `zabbix` (transport, auth, версионные API, поля/ссылки виджетов); `application` (snapshots/workcopies, операции, source/target, сопоставление, проверки, журнал, безопасная публикация); `apps/local_server` (Axum loopback); `ui` (Svelte/TypeScript, никаких прямых обращений к Zabbix).

**Контракт записи:** только подтверждённый `VerifiedPlan`, создаваемый доверенным планировщиком. UI получает opaque ID плана. Повторное чтение целевого объекта, сверка идентичности/версии, прав/зависимостей; unknown outcome после отправки требует reconciliation, не слепого повтора. Отсутствие атомарного CAS в API означает остаточный риск параллельного изменения.

**Портативность:** утверждённая предметная модель — типизированный проект одного Dashboard либо одной Page. Существующий `project/v1` остаётся читаемым legacy multi-dashboard container до O-026; он отделён от `local-bindings/v1`, `deployment-state/v1` и локальных snapshots/workcopies. Исходная версия экспорта и фактическая версия Zabbix в целевом контуре — разные атрибуты.

**Граница продукта:** только глобальные панели Zabbix 7.4 в MVP; создание виджетов `svggraph` и `item`. Другие действия лишь после доказательства для конкретного codec и версии. Неизвестные виджеты сохраняются, если операция над страницей это безопасно допускает, иначе отклоняется.

**Граница пользовательских сообщений:** Rust/Axum передают стабильные message IDs и параметры, Svelte/TypeScript локализуют их из общих data-only каталогов `lang/<locale>/`; готовая пользовательская проза не пересекает application/UI boundary. Темы строятся на общих семантических tokens, а не на цветах отдельных компонентов.

Подробнее: [предметная модель](domain_model_v1_ru.md), [форматы](format_contract_v1_ru.md), [контракт адаптера](adapter_contract_v1_ru.md), [UI, локализация и темы](ui_i18n_theme_contract_v1_ru.md), [решение о версиях](../adr/ADR-011.md), [безопасность и пакеты](../adr/ADR-012.md), [лицензирование](../adr/ADR-013.md).
