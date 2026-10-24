# Архитектурная карта ZDashstitch

**Статус:** проектируемая архитектура, а не готовые Rust-модули. Нормативные детали — ADR-001—013, схемы и контракт адаптера.

```text
Imported template files ─→ versioned export parsers ─→ MetricCatalog ┐
                                                               ├──→ PortableProject ↔ Svelte UI
Live Zabbix API ─→ version detector ─→ v7_4 adapter ────────────┘               │
             ↑                                            ↓                     ↓
             └───────── closed publisher ← VerifiedPlan ← application: operations / resolver / preflight
                                     ↑
                      LocalBindings, RawSnapshot, DeploymentState (never exported)
```

**Пакеты (цель):** `core` (логические проекты, каталог, роли и геометрия); `template_import` (импорт форматов без сети); `zabbix` (transport, auth, версионные API, поля/ссылки виджетов); `application` (операции, сопоставление, проверки, журнал, безопасная публикация); `apps/local_server` (Axum loopback); `ui` (Svelte/TypeScript, никаких прямых обращений к Zabbix).

**Контракт записи:** только подтверждённый `VerifiedPlan`, создаваемый доверенным планировщиком. UI получает opaque ID плана. Повторное чтение целевого объекта, сверка идентичности/версии, прав/зависимостей; unknown outcome после отправки требует reconciliation, не слепого повтора. Отсутствие атомарного CAS в API означает остаточный риск параллельного изменения.

**Портативность:** `project/v1` и `metrics/v1` отдельно от `local-bindings/v1` и `deployment-state/v1`; snapshots локальны. Исходная версия экспорта и фактическая версия Zabbix в целевом контуре — разные атрибуты. Новый адаптер не требует переписывать UI и проектную схему.

**Граница продукта:** только глобальные панели Zabbix 7.4 в MVP; создание виджетов `svggraph` и `item`. Другие действия лишь после доказательства для конкретного codec и версии. Неизвестные виджеты сохраняются, если операция над страницей это безопасно допускает, иначе отклоняется.

Подробнее: [форматы](format_contract_v1_ru.md), [контракт адаптера](adapter_contract_v1_ru.md), [решение о версиях](../adr/ADR-011.md), [безопасность и пакеты](../adr/ADR-012.md), [лицензирование](../adr/ADR-013.md).
