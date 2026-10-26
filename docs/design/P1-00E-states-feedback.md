# P1-00E — состояния и feedback

**Версия:** 1.0, 2026-10-24. **Статус:** принято для P1 handoff. Это UX/state contract, а не доказательство работающего UI.

## 1. Инварианты

1. Состояние определяется backend-кодом и контекстом, не текстом ОС/Zabbix и не цветом.
2. Ошибка не очищает несекретный ввод. Пароль/token очищается только после безопасной передачи либо явного cancel согласно connection flow.
3. `success` записи показывается только после требуемого read-back. HTTP 200, завершившийся progress или закрывшийся dialog успехом не являются.
4. `unknown_outcome` — отдельное блокирующее состояние: повторная запись запрещена, доступно только reconciliation чтением.
5. `read_only`, `disabled`, `unsupported`, `denied` и `offline` имеют разные причины и действия; общий серый вид без текста запрещён.
   Метка «Только чтение» (`common.state.read_only`) относится только к режиму подключения. Виджет или ссылка, которые ZDashstitch не умеет изменять, получают «Изменение не поддерживается» (`common.state.edit_unsupported`) либо полный fitness-текст «Распознано, но доступно только для чтения» (`bindings.fitness.read_only`); объект при этом сохраняется без потерь.
6. Background refresh не крадёт focus, не закрывает dialog и не перезаписывает dirty input.

## 2. Interactive component state matrix

| State | Видимое поведение | Interaction/ARIA | Message contract |
|---|---|---|---|
| `default` | Базовая surface/border/text | Обычный semantics | Label/help IDs |
| `hover` | Subtle action surface; layout stable | Не единственный путь к action | Без отдельного сообщения |
| `pressed` | Краткая active surface | Native active/pressed | Toggle получает state label при необходимости |
| `focus` | 2 px perimeter + offset, ≥3:1 | `:focus-visible`; не равно selected | Accessible name всегда существует |
| `selected` | Mark/icon + selected surface/label | `aria-selected/current/checked` по pattern | State label при необходимости |
| `disabled` | Сниженный акцент, причина рядом/в группе | Не focusable для native disabled | `common.reason.*` при неочевидной причине |
| `read_only` | Значение normal contrast, edit action отсутствует/locked | Focus/copy разрешены | `common.state.read_only` + reason |
| `busy` | Label сохраняет контекст, indicator/stage | Повторная активация блокируется; cancel отдельно | `common.progress.*` или feature ID |
| `error` | Icon + text + field/row relation | `aria-invalid`, described error; summary after submit | `errors.*` |
| `warning` | Icon + reason + next step | Не блокирует без явного guard | Feature warning ID |
| `success` | Check/icon + verified label/detail | Polite status; не auto-focus | Только подтверждённый result ID |

`busy` не превращает button в disabled без причины: focus остаётся стабильным, чтобы screen reader слышал stage и пользователь мог перейти к cancel. Для idempotency-sensitive write действие после submit становится unavailable до результата/reconciliation.

## 3. Screen state matrix

| State | Blocking | Что сохраняется | Primary action |
|---|---|---|---|
| `loading` | Нет, если уже есть usable stale data | Selection, dirty input, scroll/focus | Cancel только для долгой user-started операции |
| `empty` | Нет | Filters/context | Создать/импортировать/очистить фильтр по причине пустоты |
| `partial` | Иногда для write | Все успешно прочитанные данные + список gaps | Continue offline/read-only либо resolve blockers |
| `denied` | Для требуемого server action | Local project/workcopy | Re-authenticate/request access/change connection; не «нет объектов» |
| `unsupported` | Для затронутой операции | Raw/unknown object losslessly read-only | Inspect details/remove from intent/cancel |
| `offline` | Только server actions | Local editable state | Work locally/reconnect |
| `stale` | Write блокируется | Base + Local + observed Remote metadata | Refresh/re-import/rebuild preview |
| `conflict` | Update блокируется | Base/Local/Remote | Review; re-import or Save as copy, без auto-merge |
| `unknown_outcome` | Новая запись в ту же цель блокируется | Journal/submitting record and plan evidence | Reconcile by read; never blind retry |
| `failed_before_write` | Нет server mutation | Local intent/input | Safe retry after cause fixed |
| `partial_failure` | Да для continuation as success | Per-step results and journal | Inspect/reconcile; new plan for remaining work |
| `verified` | Нет | Read-back result and audit record | Open result/continue |

## 4. Feedback priority

Высший одновременно видимый приоритет:

1. Security/write guard: wrong target, permission/capability failure, stale/conflict, unknown outcome.
2. Field/selection blocker required to continue.
3. Operation progress/cancel.
4. Non-blocking warning/partial information.
5. Verified success and informational hints.

Один route-level banner сообщает главную причину. Более низкие сообщения остаются у полей/строк или в diagnostics; toast не перекрывает blocking banner и не является единственным местом результата.

## 5. Loading thresholds from P1-IN-06

Лабораторные времена не являются SLA. Design defaults:

| Duration after user action | Feedback |
|---:|---|
| `<150 ms` | Не показывать transient indicator, чтобы избежать flicker |
| `150–700 ms` | Local inline busy indicator с неизменной label |
| `>700 ms` | Видимый stage/status; для multi-object operation — count при наличии |
| `>2 s` | Cancel, если transport/parser безопасно поддерживает его; пояснение ongoing stage |
| Long/unknown, включая connection timeout | Persistent progress region + cancel/reconnect guidance |

`user.login` около 0,5 с обычно остаётся inline. Первый `configuration.export` до 3,9 с получает stage и cancel. Connect к неотвечающему адресу длится десятки секунд и всегда имеет явный stage/cancel. Skeleton допустим для delayed initial read списка/структуры; для submit/publish используется progress и journal state, не skeleton.

## 6. Error transitions

```text
idle
  └─ user action → validating
       ├─ validation error → editable_error → validating
       ├─ safe read started → reading → ready | partial | denied | offline
       └─ verified write intent → previewing → confirmed → submitting
              ├─ rejected before send → failed_before_write → previewing
              ├─ response + read-back match → structurally_verified
              ├─ known partial mutation → partial_failure → reconciliation
              └─ send outcome uncertain → unknown_outcome → reconciliation
                                                     ├─ match → verified
                                                     ├─ no change → new plan allowed
                                                     └─ ambiguous → blocked + human review
```

Любое изменение endpoint, connection identity, version, permissions, Base snapshot или bindings инвалидирует preview/plan и возвращает процесс к проверке. UI не восстанавливает `confirmed` после restart.

## 7. Connection/auth error map

| Backend code | User state | Retry/action |
|---|---|---|
| `dns_unresolvable` | endpoint error | Исправить host; затем retry |
| `connection_refused` | endpoint unavailable | Проверить service/port; retry |
| `host_unreachable_or_timeout` | connection timeout/unreachable | Cancel/retry/network check; причины не разделять |
| `wrong_base_path`, `redirect`, `not_jsonrpc` | Zabbix API not found at address | Исправить base URL; не следовать redirect автоматически |
| `tls_untrusted` | TLS trust blocker | Явно добавить разрешённый CA; no bypass |
| `tls_certificate_time_invalid` | TLS time blocker | Исправить certificate/time; expired/not-yet-valid не обещать различать |
| invalid/disabled token | authentication failed | Replace token; не выдавать за permission denied |
| token expired | authentication expired | Re-authenticate |
| password/user/temporary block/LDAP failure | sign-in failed, intentionally merged | Recheck credentials; для LDAP указать возможную недоступность directory |
| system access disabled | access denied | Contact administrator/change account |
| API method denied | permission denied | Read-only/blocked action, показать method safely |
| MFA session terminated | token required for this account | Предложить разрешённый API token |

OS-localized message не отображается как primary copy. Technical details используют только allowlist P1-00D.

## 8. Resolver/import states

- Resolver показывает две независимые оси: match `exact/compatible/ambiguous/missing/unresolved_lld/denied` и fitness `editable/read_only/unsupported`.
- `denied` формулируется «не найдено или нет доступа», если backend не различает причины.
- Import quarantine различает archive/policy/YAML/schema/semantic codes; неизвестный parse detail получает стабильный application code до появления UI.
- Template dashboards из export перечисляются как unsupported/not imported, не исчезают из результата.
- Partial import никогда не становится Base/Local для публикации, пока consistency contract не выполнен.

## 9. Confirmations

- Reversible local delete: обычный confirmation только при потере несохранённого; предпочтителен undo/history.
- Destructive/irreversible or server write: dialog называет target, operation, count, access mode, blockers и consequence. Primary verb конкретен; generic `OK` запрещён.
- Typed confirmation применяется только когда риск оправдывает нагрузку; не используется ритуально.
- Cancel всегда сохраняет local draft и не меняет server.
- Write confirmation одноразовое; повторное открытие после изменения данных требует нового preview.

## 10. Acceptance checks

- [x] Описаны все component и screen states из P1-00E.
- [x] Есть message priority, сохранение ввода, loading thresholds и safe confirmation rules.
- [x] IN-01/06 reflected; success до read-back и blind retry запрещены.
- [x] Conflict, partial failure и unknown outcome имеют разные переходы.
- [ ] Runtime timers, announcements, focus and journal recovery проверяются в P2/P1-11.
