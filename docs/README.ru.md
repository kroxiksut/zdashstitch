# Документация ZDashstitch

[English](README.md) · [README проекта](../README.ru.md) · [Дорожная карта](../ROADMAP.ru.md)

В этом каталоге находятся цели продукта, принятые решения, архитектурные контракты и трассировка. Проект остаётся на стадии спецификации/подготовки к реализации; прежние редакции доступны в истории Git или необязательном игнорируемом локальном архиве.

## С чего начать

| Задача | Что читать |
|---|---|
| Понять продукт и MVP | [`../README.ru.md`](../README.ru.md), [`product/PROJECT_GOALS.md`](product/PROJECT_GOALS.md), [`product/USE_CASES.md`](product/USE_CASES.md) |
| Понять развитие после MVP | [`../ROADMAP.ru.md`](../ROADMAP.ru.md) |
| Реализовывать безопасно | [`../AGENTS.md`](../AGENTS.md), [`DOC_GOVERNANCE.md`](DOC_GOVERNANCE.md), ADR и архитектурные контракты ниже |
| Понять Dashboard/Page/Widget, workcopy и типы проекта | [`architecture/domain_model_v1_ru.md`](architecture/domain_model_v1_ru.md), O-025/O-026 в [`../OPEN_DECISIONS.md`](../OPEN_DECISIONS.md) |
| Реализовывать или переводить UI | [`architecture/ui_i18n_theme_contract_v1_ru.md`](architecture/ui_i18n_theme_contract_v1_ru.md), [`../CONTRIBUTING.md`](../CONTRIBUTING.md) |
| Проследить цели MVP до задач и шлюзов | [`MVP_TRACEABILITY.md`](MVP_TRACEABILITY.md) |
| Увидеть нерешённые пробелы | [`traceability_v06.md`](traceability_v06.md), [`../OPEN_DECISIONS.md`](../OPEN_DECISIONS.md) |
| Проверить переносимые форматы | [`../schemas/`](../schemas/), [`architecture/format_contract_v1_ru.md`](architecture/format_contract_v1_ru.md), [`../tests/`](../tests/) |

Текущая очередь владельца ведётся локально в игнорируемом `TASKS_RU.md`; публичные документы фиксируют границы и доказательства, а не живые чекбоксы выполнения.

## Актуальные нормативные документы

### Продукт

- [`product/PROJECT_GOALS.md`](product/PROJECT_GOALS.md) — проблема, пользователи, критерии успеха и границы.
- [`product/USE_CASES.md`](product/USE_CASES.md) — сквозные сценарии приёмки.
- [`product/BRANDING_NOTES.md`](product/BRANDING_NOTES.md) — целевой визуальный образ и его нефункциональный статус.

### Архитектура

- [`architecture/ARCHITECTURE_MAP.md`](architecture/ARCHITECTURE_MAP.md) — границы компонентов и поток данных.
- [`architecture/domain_model_v1_ru.md`](architecture/domain_model_v1_ru.md) — Dashboard/Page/Widget, snapshot/workcopy, проекты Dashboard/Page и конфликт legacy `project/v1`.
- [`architecture/format_contract_v1_ru.md`](architecture/format_contract_v1_ru.md) — контракты переносимых и локальных данных.
- [`architecture/adapter_contract_v1_ru.md`](architecture/adapter_contract_v1_ru.md) — версионный адаптер Zabbix и безопасная граница публикации.
- [`architecture/ui_i18n_theme_contract_v1_ru.md`](architecture/ui_i18n_theme_contract_v1_ru.md) — граница пользовательских сообщений, вклад переводчиков и правила светлой/тёмной тем.

### Дизайн (рабочие материалы P1)

- [`design/P1-00A-visual-audit.md`](design/P1-00A-visual-audit.md) — аудит текущего scaffold, бренда, поверхностей, пробелов и рисков.
- [`design/P1-00B-foundations.md`](design/P1-00B-foundations.md) — typography, spacing, density и разделение app layout от сетки Zabbix 72×64; статус partial до P1-IN-02.
- [`design/P1-00C-tokens-and-themes.md`](design/P1-00C-tokens-and-themes.md) — semantic tokens и mapping `light`/`dark`/`system`/high-contrast для design review.
- [`design/P1-00G-accessibility-platform.md`](design/P1-00G-accessibility-platform.md) — focus/keyboard/landmarks/errors/targets/reflow/motion/forced-colors и Windows/Linux acceptance contract.
- [`design/backend_inputs_ru.md`](design/backend_inputs_ru.md) — входные данные backend для дизайна P1-IN-01—09: ошибки подключения, виджеты и поля, статусы resolver, матрица записи 7.4.16, экспорты шаблонов, задержки, локальная сессия, права, выбор файлов.

### Решения

- [`adr/ADR-001-005.md`](adr/ADR-001-005.md) — форма продукта, локальный GUI и первые архитектурные решения.
- [`adr/ADR-006-010.md`](adr/ADR-006-010.md) — безопасность, операции и лабораторные контракты.
- [`adr/ADR-011.md`](adr/ADR-011.md) — граница многоверсионных адаптеров.
- [`adr/ADR-012.md`](adr/ADR-012.md) — исправления v0.6 для форматов, пакетов и `VerifiedPlan`.
- [`adr/ADR-013.md`](adr/ADR-013.md) — лицензирование кода и документации.

### Управление и доказательства

- [`DOC_GOVERNANCE.md`](DOC_GOVERNANCE.md) — приоритет документов и правила статусов.
- [`MVP_TRACEABILITY.md`](MVP_TRACEABILITY.md) — результат R0 с перенумерацией от 24.10.2026: связь сценариев продукта с P0–P6, G0–G6 и доказательствами приёмки.
- [`traceability_v06.md`](traceability_v06.md) — нерешённые замечания и матрица покрытия.
- [`../IMPLEMENTATION_LOG.md`](../IMPLEMENTATION_LOG.md) — фактически завершённая работа и доказательства.
- [`../OPEN_DECISIONS.md`](../OPEN_DECISIONS.md) — решения, требующие владельца или лабораторного подтверждения.

## Языки

README, дорожная карта и эта карта документации существуют на русском и английском. Большинство детальных продуктовых, ADR и архитектурных документов пока только на русском; их имена сохраняются до появления утверждённого английского перевода. Runtime-локализация UI регулируется отдельно: английский является исходным каталогом, русский обязателен и полон для MVP, дополнительные языки можно добавлять независимо. JSON Schema, синтетические данные и исходный код по возможности нейтральны к языку.

## Приоритет при расхождении

1. Проверенное поведение явно разрешённого тестового Zabbix.
2. Генерируемые схемы/инварианты и принятые ADR; более поздние явные уточнения имеют приоритет.
3. Архитектурные контракты.
4. Границы MVP в корневом README.
5. Направления после MVP в корневой дорожной карте.
6. Исторические материалы.

Неразрешимый нормативный конфликт записывается в [`../OPEN_DECISIONS.md`](../OPEN_DECISIONS.md); молча выбирать удобный документ запрещено.
