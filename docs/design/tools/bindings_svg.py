"""Review mockups 09-01..09-07: «Привязки» (P1-05: ReplaceHost, Clone, cardinality, bindings).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/bindings_svg.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402
from publication_svg import (  # noqa: E402
    AMBER, BLUE, GREEN, NEUTRAL, RED, box, button, checkbox, chip, head, icon, radio, select, t,
)

OUT = "docs/design/assets/09-bindings/"
LAB_RO = ("Только чтение · Лаборатория 7.4", "#eef4ff", "#3156a3", 270)
SAVED = ("Сохранена локально", "#ecfdf3", "#1f6b43", 170)
DIRTY = ("Изменена", "#fff4e5", "#8a4b00", 100)
NAV = ["Обзор", "Структура", "Привязки", "Проверки", "Публикация"]


def sidebar(origin, selected="Привязки"):
    s = '  <rect x="0" y="72" width="260" height="928" fill="#ffffff" stroke="#e4e7ec"/>\n'
    s += t(28, 116, "ПАНЕЛЬ", "label muted")
    y = 164
    for name in NAV:
        if name == selected:
            s += box(16, y - 28, 228, 46, fill="#eef4ff", stroke="#eef4ff", rx=10)
            s += t(36, y + 2, name, "nav", "#1d3f86", weight="700")
        else:
            s += t(36, y, name, "nav")
        y += 48
    s += '  <line x1="20" y1="392" x2="240" y2="392" stroke="#e4e7ec"/>\n'
    s += t(28, 428, "ПРОИСХОЖДЕНИЕ", "label muted")
    y = 458
    for i, line in enumerate(origin):
        s += t(28, y, line, "body" if i == 0 else "small", None if i == 0 else "#667085", weight="650" if i == 0 else None)
        y += 26 if i == 0 else 24
    return s


ORIGIN = ["Копия с сервера", "«Базы данных: Prod»", "Лаборатория 7.4", "снимок 25.10.2026, 14:02", "3 страницы · 19 виджетов"]
CRUMB = "Черновики  /  Базы данных: Prod  /  Привязки"
STEPS_RH = ["Область", "Зависимости", "Новый узел", "Соответствия", "Итог"]
STEPS_CL = ["Область", "Зависимости", "Целевые узлы", "Соответствия", "Серии", "Итог"]


def steps(names, current, x=292, y=146):
    s = ""
    for i, name in enumerate(names, start=1):
        w = 42 + 9 * len(name)
        if i == current:
            fill, stroke, color, weight = "#eef4ff", "#3156a3", "#1d3f86", "700"
        elif i < current:
            fill, stroke, color, weight = "#ffffff", "#d0d5dd", "#1f6b43", "600"
        else:
            fill, stroke, color, weight = "#ffffff", "#e4e7ec", "#667085", "500"
        s += f'  <rect x="{x}" y="{y}" width="{w}" height="34" rx="17" fill="{fill}" stroke="{stroke}"/>\n'
        if i < current:
            s += icon("circle-check", x + 10, y + 8, "#1f6b43", 18)
        else:
            s += t(x + 19, y + 22, str(i), "small", color, "middle", "700")
        s += t(x + 34, y + 22, name, "small", color, weight=weight)
        x += w + 8
    return s


def bar(left, right, y=941):
    s = '  <rect x="260" y="924" width="1340" height="76" fill="#ffffff" stroke="#e4e7ec"/>\n'
    x = 292
    for text, kind, w in left:
        s += button(x, y, w, text, kind)
        x += w + 12
    x = 1544
    for text, kind, w in reversed(right):
        x -= w
        s += button(x, y, w, text, kind)
        x -= 12
    return s


def write(svg, name, chips=(SAVED,), mode=LAB_RO):
    svg = appbar.apply(svg, active="drafts", mode=mode, depth_prefix="../../../../",
                       breadcrumb=CRUMB, chips=list(chips))
    os.makedirs(OUT, exist_ok=True)
    with open(OUT + name, "w", encoding="utf-8", newline="") as f:
        f.write(svg)
    print("written", OUT + name)


def title(text):
    return "  <!-- Title -->\n" + t(292, 124, text, "title")


def hline(x1, x2, y):
    return f'  <line x1="{x1}" y1="{y}" x2="{x2}" y2="{y}" stroke="#eef0f4"/>\n'


def thead(x, y, w, cols):
    s = box(x, y, w, 32, fill="#f8fafc", stroke="#eef0f4", rx=6)
    for cx, label in cols:
        s += t(cx, y + 21, label, "xs muted", weight="700")
    return s


def lines(x, y, items, cls="small muted", fill=None, step=22):
    return "".join(t(x, y + i * step, v, cls, fill) for i, v in enumerate(items))


# ================================================================== 09-01 scope + inventory (ReplaceHost)
def scope_inventory():
    s = head("привязки: область и зависимости", "Светлый макет начала мастера замены узла сети: выбор намерения и области "
             "(вся панель по умолчанию), заменяемый узел сети и инвентаризация его ссылок по страницам и виджетам с "
             "видом ссылки и тем, что с ней будет: изменится, останется на старом узле или не поддерживается.",
             ["circle-check", "ban", "alert-triangle", "info-circle", "chevron-down", "chevron-right"])
    s += sidebar(ORIGIN) + title("Заменить узел сети") + steps(STEPS_RH, 2)
    # left: intent + scope
    s += box(292, 196, 420, 712)
    s += t(316, 230, "НАМЕРЕНИЕ", "label muted")
    s += box(316, 244, 372, 54, fill="#eef4ff", stroke="#3156a3", rx=10, sw=2) + radio(338, 271, True)
    s += t(358, 276, "Заменить узел сети", "body", weight="700")
    s += box(316, 306, 372, 54, stroke="#e4e7ec", rx=10) + radio(338, 333, False)
    s += t(358, 338, "Клонировать панель", "body", weight="600")
    s += t(316, 396, "ОБЛАСТЬ", "label muted")
    s += radio(328, 420, True) + t(346, 426, "Вся панель: 3 страницы, 19 виджетов", "small", weight="600")
    s += radio(328, 452, False) + t(346, 458, "Выбранные страницы", "small")
    s += radio(328, 484, False) + t(346, 490, "Выбранные виджеты", "small")
    s += t(316, 530, "ЗАМЕНЯЕМЫЙ УЗЕЛ СЕТИ", "label muted")
    s += select(316, 544, 372, "pg-a-01")
    s += t(316, 606, "Найден в панели: pg-a-01 (24 ссылки), pg-a-02 (8),", "xs muted")
    s += t(316, 622, "ch-a-01 (5), ch-a-02 (5)", "xs muted")
    s += box(316, 650, 372, 108, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
    s += icon("info-circle", 330, 664, "#1d3f86", 18)
    s += lines(356, 678, ["Меняется только черновик. Исходная", "панель на сервере не изменится, пока", "вы не опубликуете проверенный план."],
               "small", "#344054")
    # right: inventory
    s += box(732, 196, 812, 712)
    s += t(756, 230, "ССЫЛКИ НА pg-a-01", "label muted")
    s += chip(1036, 212, 148, "Изменятся: 23", GREEN) + chip(1192, 212, 196, "Останется на старом: 1", AMBER) + chip(1396, 212, 124, "Не найдено: 0", NEUTRAL)
    s += thead(756, 250, 764, [(770, "ВИДЖЕТ"), (990, "ТИП"), (1140, "КАК ССЫЛАЕТСЯ"), (1360, "ЧТО БУДЕТ")])
    rows = [
        ("group", "chevron-down", "Страница 1. PostgreSQL", "4 виджета · 9 ссылок"),
        ("ok", "Connections", "График", "шаблон узла: имя", "изменится"),
        ("ok", "TPS", "График", "шаблон узла: имя", "изменится"),
        ("ok", "Cache hit ratio", "Значение элемента", "элемент данных (ID)", "изменится"),
        ("ok", "Locks", "Значение элемента", "элемент данных (ID)", "изменится"),
        ("group", "chevron-down", "Страница 2. ClickHouse", "4 виджета · 8 ссылок"),
        ("ok", "Queries per second", "График", "шаблон узла: имя", "изменится"),
        ("ok", "Replication lag", "Значение элемента", "элемент данных (ID)", "изменится"),
        ("ok", "Inserts per second", "График", "шаблон узла: имя", "изменится"),
        ("warn", "Top queries", "Топ узлов сети", "узел сети (ID)", "останется на pg-a-01"),
        ("group", "chevron-right", "Страница 3. Keeper и HAProxy", "4 виджета · 7 ссылок"),
    ]
    y = 288
    for r in rows:
        if r[0] == "group":
            s += box(756, y, 764, 34, fill="#f5f7fb", stroke="#f5f7fb", rx=6)
            s += icon(r[1], 764, y + 8, "#475467", 18) + t(790, y + 22, r[2], "small", weight="700")
            s += t(1508, y + 22, r[3], "small muted", anchor="end")
            y += 40
            continue
        kind, name, typ, how, what = r
        ic, color = ("circle-check", "#1f6b43") if kind == "ok" else ("ban", "#8a4b00")
        s += hline(756, 1520, y + 38)
        s += icon(ic, 764, y + 10, color, 18)
        s += t(790, y + 24, name, "small", weight="600") + t(990, y + 24, typ, "small muted")
        s += t(1140, y + 24, how, "small") + t(1360, y + 24, what, "small", color if kind != "ok" else None, weight="650" if kind != "ok" else None)
        y += 42
    s += box(756, y + 12, 764, 104, fill="#fffaf2", stroke="#f3d3a6", rx=10)
    s += icon("alert-triangle", 770, y + 26, "#8a4b00", 18)
    s += t(796, y + 40, "Останется на pg-a-01: 1 виджет", "small", "#8a4b00", weight="700")
    s += lines(796, y + 64, ["Изменение «Топ узлов сети» не поддерживается: виджет продолжит показывать pg-a-01.",
                             "Это не блокирует замену, но перед публикацией список нужно подтвердить."], "small", "#5c3a00")
    s += t(756, y + 148, "Шаблон узла сети по имени меняется, только если совпадает с видимым именем pg-a-01 без «*»; маски не меняются.", "xs muted")
    s += bar([("Назад", "secondary", 120)], [("Сохранить черновик", "secondary", 200), ("Далее: новый узел", "primary", 210)])
    write(s + "</svg>\n", "09-01_P1-05B_scope-inventory.svg")


# ================================================================== 09-02 target picker (Clone, ordered)
def target_picker():
    s = head("привязки: целевые узлы сети", "Светлый макет выбора целевых узлов сети при клонировании: роль источника, поиск "
             "по группам, тегам и имени без смысла окружений, множественный выбор с явным порядком, видимость "
             "шаблонов на узле и оговорка о правах; фильтр не добавляет новые узлы сам.",
             ["search", "chevron-down", "check", "arrow-up", "arrow-down", "x", "info-circle", "circle-check", "alert-triangle"])
    s += sidebar(ORIGIN) + title("Клонировать панель") + steps(STEPS_CL, 3)
    s += box(292, 196, 1252, 60, rx=12)
    s += t(316, 222, "Роль источника", "xs muted") + t(316, 244, "PostgreSQL · 2 узла: pg-a-01, pg-a-02", "body", weight="700")
    s += t(760, 222, "Другие роли", "xs muted") + t(760, 244, "ClickHouse — 2 узла · Zabbix server — 1 узел", "small")
    s += t(1520, 236, "Роль — набор узлов источника, а не тип окружения.", "xs muted", anchor="end")
    # host search
    s += box(292, 272, 760, 636)
    s += select(316, 292, 230, "Группа: Pilot/DB", h=38)
    s += select(556, 292, 190, "Тег: role=pgsql", h=38)
    s += box(756, 292, 272, 38, stroke="#7a8d9b", rx=8) + icon("search", 768, 301, "#667085", 20) + t(798, 316, "pg-b", "small")
    s += thead(316, 346, 712, [(352, "УЗЕЛ СЕТИ"), (520, "ГРУППЫ"), (700, "ШАБЛОНЫ ИСТОЧНИКА НА УЗЛЕ")])
    hosts = [(True, "pg-b-01", "Pilot/DB", "2 из 2", True), (True, "pg-b-02", "Pilot/DB", "2 из 2", True),
             (True, "pg-b-03", "Pilot/DB", "2 из 2", True), (False, "pg-b-04-test", "Pilot/DB, Test", "1 из 2", False),
             (False, "pg-backup", "Pilot/DB", "0 из 2", False)]
    y = 384
    for on, name, groups, tpl, full in hosts:
        if on:
            s += box(316, y, 712, 46, fill="#f5f8ff", stroke="#f5f8ff", rx=6)
        s += hline(316, 1028, y + 46)
        s += checkbox(328, y + 13, on) + t(358, y + 29, name, "mono") + t(520, y + 29, groups, "small muted")
        s += icon("circle-check" if full else "alert-triangle", 700, y + 14, "#1f6b43" if full else "#8a4b00", 16)
        s += t(724, y + 29, tpl + (" (PostgreSQL by Zabbix agent 2, Linux)" if full else " — часть метрик не найдётся"), "small", None if full else "#8a4b00")
        y += 50
    s += icon("info-circle", 316, y + 18, "#667085", 18)
    s += lines(342, y + 32, ["Показаны узлы, видимые учётной записи. Нужного узла нет — его нет или не хватает прав.",
                             "Фильтр только помогает найти узлы: в соответствиях хранится выбранный список,",
                             "и новые узлы группы сами не добавляются."], "xs muted", step=18)
    # ordered selection
    s += box(1072, 272, 472, 636)
    s += t(1096, 306, "ЦЕЛЕВЫЕ УЗЛЫ PostgreSQL · ПОРЯДОК", "label muted")
    s += t(1096, 330, "Порядок задаёт слоты: 1-й источник → 1-я цель.", "xs muted")
    sel = [("1", "pg-b-01", "вместо pg-a-01"), ("2", "pg-b-02", "вместо pg-a-02"), ("3", "pg-b-03", "новый слот")]
    y = 346
    for n, name, note in sel:
        s += box(1096, y, 424, 52, fill="#ffffff", stroke="#d0d5dd", rx=8)
        s += t(1112, y + 32, n, "body", "#667085", weight="700") + t(1140, y + 32, name, "mono")
        s += t(1240, y + 32, note, "small", "#8a4b00" if note == "новый слот" else "#667085")
        s += icon("arrow-up", 1414, y + 16, "#475467", 18, "Выше") + icon("arrow-down", 1442, y + 16, "#475467", 18, "Ниже")
        s += icon("x", 1478, y + 16, "#667085", 18, "Убрать")
        y += 60
    s += box(1096, y + 6, 424, 92, fill="#fff8ed", stroke="#f3d3a6", rx=10)
    s += icon("alert-triangle", 1110, y + 20, "#8a4b00", 18)
    s += lines(1136, y + 34, ["Целей больше, чем источников (2 → 3).", "Серии графиков получат третий ряд —",
                              "это настраивается на шаге «Серии»."], "small", "#5c3a00")
    s += t(1096, y + 136, "Подключение: Лаборатория 7.4 — то же, что у", "xs muted")
    s += t(1096, y + 154, "исходной панели. Клон в другое подключение невозможен.", "xs muted")
    s += bar([("Назад", "secondary", 120)], [("Сохранить черновик", "secondary", 200), ("Далее: соответствия", "primary", 220)])
    write(s + "</svg>\n", "09-02_P1-05C_target-picker.svg", chips=(DIRTY,))


# ================================================================== 09-03 mapping table (ReplaceHost)
def mapping_table():
    s = head("привязки: таблица соответствий", "Светлый макет таблицы соответствий при замене pg-a-01 на pg-b-01: строки "
             "исходный элемент данных → целевой со статусом совпадения и затронутыми виджетами, фильтры и массовое "
             "принятие точных, боковая панель сравнения кандидатов для неоднозначной строки с основаниями.",
             ["circle-check", "alert-triangle", "circle-x", "circle-dashed", "search", "info-circle"])
    s += sidebar(ORIGIN) + title("Заменить узел сети: pg-a-01 → pg-b-01") + steps(STEPS_RH, 4)
    s += box(292, 196, 860, 712)
    x = 316
    for label, w, on in [("Все 23", 84, True), ("Точно 19", 98, False), ("С отличиями 2", 134, False),
                         ("Неоднозначно 1", 140, False), ("Не найдено 1", 124, False)]:
        if on:
            s += box(x, 212, w, 32, fill="#243746", stroke="#243746", rx=16) + t(x + w / 2, 233, label, "small", "#ffffff", "middle", "650")
        else:
            s += box(x, 212, w, 32, stroke="#c9d3ea", rx=16) + t(x + w / 2, 233, label, "small", None, "middle", "600")
        x += w + 8
    s += button(944, 208, 184, "Принять точные (19)", h=38)
    s += thead(316, 258, 812, [(352, "ИСТОЧНИК (pg-a-01)"), (612, "ЦЕЛЬ (pg-b-01)"), (850, "СОВПАДЕНИЕ"), (1010, "ВИДЖЕТЫ")])
    rows = [
        ("circle-check", "#1f6b43", "pgsql.connections[total]", "pgsql.connections[total]", "Точно", "3", False),
        ("circle-check", "#1f6b43", "pgsql.transactions.commit", "pgsql.transactions.commit", "Точно", "2", False),
        ("circle-check", "#1f6b43", "pgsql.cache.hit", "pgsql.cache.hit", "Точно", "1", False),
        ("alert-triangle", "#8a4b00", "net.if.in[eth0]", "net.if.in[eth0]", "С отличиями", "1", False),
        ("alert-triangle", "#8a4b00", "CPU*  (шаблон имени)", "11 кандидатов", "Неоднозначно", "1", True),
        ("circle-x", "#a32935", "pgsql.db.size[proddb]", "—", "Не найдено (LLD)", "1", False),
    ]
    y = 296
    for ic, color, src, dst, st, wn, sel in rows:
        if sel:
            s += box(316, y, 812, 46, fill="#eef4ff", stroke="#3156a3", rx=8, sw=2)
        else:
            s += hline(316, 1128, y + 46)
        s += icon(ic, 326, y + 14, color, 18)
        s += t(352, y + 29, src, "mono") + t(612, y + 29, dst, "mono" if dst[0] not in "—1" else "small")
        s += t(850, y + 29, st, "small", color if ic != "circle-check" else None, weight="650")
        s += t(1010, y + 29, wn, "small")
        y += 50
    s += t(316, y + 16, "и ещё 17 строк: 16 точных, 1 с отличиями", "xs muted")
    s += box(316, y + 34, 812, 92, fill="#fff5f5", stroke="#f2b8b5", rx=10)
    s += icon("circle-x", 330, y + 48, "#a32935", 18)
    s += t(356, y + 62, "pgsql.db.size[proddb]: элемент найден LLD, на pg-b-01 такого экземпляра нет", "small", "#a32935", weight="650")
    s += t(356, y + 86, "Есть экземпляры того же прототипа: pgsql.db.size[pilotdb], pgsql.db.size[postgres]", "small")
    s += t(356, y + 108, "Выбрать экземпляр вручную · Оставить неразрешённым (блокирует публикацию)", "small", "#3156a3", weight="650")
    s += t(316, y + 152, "Похожее имя не считается доказательством: точным считается только тот же ключ, тип значения и единицы.", "xs muted")
    # side panel candidates
    s += box(1172, 196, 372, 712)
    s += t(1196, 230, "КАНДИДАТЫ", "label muted")
    s += t(1196, 256, "CPU* на pg-b-01 · график «CPU»", "small", weight="650")
    s += t(1196, 278, "шаблон имени даёт 11 элементов данных", "xs muted")
    cands = [("CPU utilization", "system.cpu.util", "%"), ("CPU idle time", "system.cpu.util[,idle]", "%"),
             ("CPU iowait time", "system.cpu.util[,iowait]", "%"), ("CPU load avg1", "system.cpu.load[all,avg1]", "")]
    y = 298
    for i, (n, k, u) in enumerate(cands):
        s += box(1196, y, 324, 58, fill="#ffffff", stroke="#3156a3" if i == 0 else "#e4e7ec", rx=8, sw=2 if i == 0 else None)
        s += radio(1214, y + 20, i == 0) + t(1232, y + 25, n, "small", weight="650")
        s += t(1232, y + 46, k + (f" · {u}" if u else ""), "mono")
        y += 64
    s += t(1196, y + 14, "и ещё 7", "xs muted")
    s += t(1196, y + 50, "ОСНОВАНИЯ", "label muted")
    s += lines(1196, y + 76, ["Источник: шаблон «CPU*» совпал с 11 элементами", "на pg-a-01; на графике показан 1 ряд.",
                              "Совпадает ключ, тип и единицы у 1 кандидата."], "small")
    s += button(1196, y + 152, 160, "Выбрать") + button(1366, y + 152, 154, "Оставить шаблон")
    s += t(1196, y + 220, "«Оставить шаблон» — ряд будет искать «CPU*»", "xs muted")
    s += t(1196, y + 238, "на pg-b-01 так же, как сейчас на pg-a-01.", "xs muted")
    s += bar([("Назад", "secondary", 120)], [("Сохранить черновик", "secondary", 200), ("Далее: итог", "primary", 160)])
    write(s + "</svg>\n", "09-03_P1-05D_mapping-table.svg", chips=(DIRTY,))


# ================================================================== 09-04 clone decisions: masks, keep_source, kept list
def clone_decisions():
    s = head("привязки: решения для клона", "Светлый макет решений при клонировании: маски узлов сети и шаблоны элементов "
             "данных в графиках — новый шаблон с предпросмотром совпадений или «оставить как есть» с предупреждением, "
             "общий узел «оставить исходный», обязательное подтверждение списка виджетов, которые останутся на "
             "исходных узлах.",
             ["circle-check", "alert-triangle", "info-circle", "check", "circle-dashed"])
    s += sidebar(ORIGIN) + title("Клонировать панель: решения") + steps(STEPS_CL, 4)
    s += box(292, 196, 1252, 712)
    s += t(316, 230, "МАСКИ И ШАБЛОНЫ В ГРАФИКАХ · НУЖНО РЕШЕНИЕ ДЛЯ КАЖДОЙ СТРОКИ", "label muted")
    s += thead(316, 246, 1204, [(340, "ГДЕ"), (560, "СЕЙЧАС"), (760, "РЕШЕНИЕ"), (1120, "СОВПАДЕНИЯ НА ЦЕЛИ")])
    rows = [
        ("«Connections», ряд 1", "pg-a-*", "new", "pg-b-*", "3 узла: pg-b-01, pg-b-02, pg-b-03", "ok"),
        ("«Disk I/O», ряд 2", "*-a-0*", "keep", None, "останется: данные исходной области", "warn"),
        ("«Queries», элементы", "ClickHouse: *", "new", "ClickHouse: *", "12 элементов на ch-b-01, ch-b-02", "ok"),
        ("«Load», ряд 1", "db-*", "none", None, "решение не выбрано", "todo"),
    ]
    y = 284
    for where, now, kind, new, matches, st in rows:
        h = 60
        s += hline(316, 1520, y + h)
        s += t(340, y + 34, where, "small", weight="600") + t(560, y + 34, now, "mono")
        if kind == "new":
            s += radio(772, y + 20, True) + t(790, y + 25, "Новый шаблон", "small")
            s += box(790, y + 32, 200, 26, stroke="#3156a3", rx=6) + t(800, y + 50, new, "mono")
            s += radio(1012, y + 20, False) + t(1030, y + 25, "Как есть", "small muted")
        elif kind == "keep":
            s += radio(772, y + 20, False) + t(790, y + 25, "Новый шаблон", "small muted")
            s += radio(1012, y + 20, True) + t(1030, y + 25, "Как есть", "small")
        else:
            s += radio(772, y + 20, False) + t(790, y + 25, "Новый шаблон", "small muted")
            s += radio(1012, y + 20, False) + t(1030, y + 25, "Как есть", "small muted")
        ic, color = {"ok": ("circle-check", "#1f6b43"), "warn": ("alert-triangle", "#8a4b00"), "todo": ("circle-dashed", "#a32935")}[st]
        s += icon(ic, 1120, y + 12, color, 18)
        s += t(1144, y + 26, matches.split(":")[0] + (":" if ":" in matches else ""), "small", color if st != "ok" else None, weight="650")
        if ":" in matches:
            s += t(1144, y + 46, matches.split(":", 1)[1].strip(), "xs muted")
        y += h + 4
    s += t(316, y + 18, "Предпросмотр совпадений — по правилам Zabbix 7.4: видимое имя, без учёта регистра, единственный символ шаблона «*».", "xs muted")
    # keep_source
    y += 44
    s += t(316, y, "ОБЩИЕ УЗЛЫ СЕТИ", "label muted")
    s += box(316, y + 14, 590, 74, fill="#f8fafc", stroke="#eef0f4", rx=10)
    s += t(336, y + 42, "Zabbix server", "mono") + t(480, y + 42, "используется в 2 виджетах", "small muted")
    s += radio(346, y + 66, True) + t(364, y + 71, "Оставить исходный", "small", weight="650")
    s += radio(540, y + 66, False) + t(558, y + 71, "Заменить на другой узел", "small")
    s += t(316, y + 112, "«Оставить исходный» — осознанный выбор, без предупреждения.", "xs muted")
    # kept_on_source confirmation
    s += box(930, y + 14, 590, 196, fill="#fffaf2", stroke="#f3d3a6", rx=10)
    s += icon("alert-triangle", 946, y + 28, "#8a4b00", 18)
    s += t(972, y + 42, "Останутся на исходных узлах: 4 виджета", "small", "#8a4b00", weight="700")
    s += lines(972, y + 66, ["Top queries, Slow queries (Топ узлов сети) — pg-a-01, pg-a-02",
                             "Problems (Проблемы) — фильтр по pg-a-*",
                             "Disk I/O (График), ряд 2 — «как есть»"], "small", "#344054")
    s += lines(972, y + 138, ["Копия покажет в них данные исходной области."], "small", "#5c3a00")
    s += checkbox(972, y + 160, False) + t(1002, y + 176, "Подтверждаю этот список", "small", weight="650")
    s += bar([("Назад", "secondary", 120)], [("Сохранить черновик", "secondary", 200), ("Далее: серии", "disabled", 170)])
    s += t(1520, 890, "Осталось: 1 решение по шаблону и подтверждение списка", "small", "#a32935", anchor="end", weight="650")
    write(s + "</svg>\n", "09-04_P1-05D_clone-decisions.svg", chips=(DIRTY,))


# ================================================================== 09-05 cardinality 2 -> 3
def cardinality():
    s = head("привязки: серии 2 → 3", "Светлый макет редактора кардинальности: график с двумя рядами PostgreSQL получает третий "
             "ряд для нового целевого узла с той же метрикой и настройками, подпись и цвет по правилам профиля видны до "
             "принятия; виджет «один узел на виджет» при росте числа целей — блокировка со ссылкой на редактор.",
             ["circle-check", "plus", "circle-x", "arrow-up", "arrow-down", "x", "info-circle", "alert-triangle"])
    s += sidebar(ORIGIN) + title("Клонировать панель: серии") + steps(STEPS_CL, 5)
    s += box(292, 196, 820, 712)
    s += t(316, 230, "«Connections» · График · страница 1", "h3")
    s += t(316, 254, "Метрика pgsql.connections[total]; набор данных — список узлов сети", "small muted")
    s += t(316, 292, "БЫЛО (ИСТОЧНИК)", "label muted") + t(716, 292, "СТАНЕТ (КОПИЯ)", "label muted")
    before = [("#3b82c4", "pg-a-01"), ("#e0803a", "pg-a-02")]
    after = [("#3b82c4", "pg-b-01", "слот 1"), ("#e0803a", "pg-b-02", "слот 2"), ("#8b5cf6", "pg-b-03", "новый ряд")]
    y = 308
    for color, name in before:
        s += box(316, y, 360, 44, stroke="#e4e7ec", rx=8)
        s += f'  <rect x="330" y="{y + 15}" width="14" height="14" rx="3" fill="{color}"/>\n' + t(354, y + 28, name, "mono")
        s += t(660, y + 28, "pgsql.connections[total]", "xs muted", anchor="end")
        y += 52
    y = 308
    for color, name, note in after:
        new = note == "новый ряд"
        s += box(716, y, 372, 44, fill="#f3faf5" if new else "#ffffff", stroke="#7cc495" if new else "#e4e7ec", rx=8, sw=2 if new else None)
        s += f'  <rect x="730" y="{y + 15}" width="14" height="14" rx="3" fill="{color}"/>\n' + t(754, y + 28, name, "mono")
        s += t(848, y + 28, note, "small", "#1f6b43" if new else "#667085", weight="650" if new else None)
        s += icon("arrow-up", 1004, y + 13, "#475467", 18, "Выше") + icon("arrow-down", 1030, y + 13, "#475467", 18, "Ниже")
        s += icon("x", 1060, y + 13, "#667085", 18, "Убрать ряд")
        y += 52
    s += icon("plus", 716, y + 6, "#3156a3", 18) + t(740, y + 20, "Добавить узел", "small", "#3156a3", weight="650")
    s += box(316, 494, 772, 136, fill="#f8fafc", stroke="#eef0f4", rx=10)
    s += t(336, 522, "Новый ряд pg-b-03", "small", weight="700")
    s += lines(336, 548, ["Метрика, тип отображения, ось, функция агрегации — как у ряда 1.",
                          "Подпись: «{HOST.NAME}: Connections» — как у остальных рядов.",
                          "Цвет: следующий из палитры профиля (#8b5cf6), не совпадает с соседними."], "small")
    s += t(316, 664, "ПРЕДПРОСМОТР ЛЕГЕНДЫ · синтетика", "label muted")
    for i, (color, name, _) in enumerate(after):
        s += f'  <rect x="{316 + i * 200}" y="680" width="14" height="14" rx="3" fill="{color}"/>\n' + t(338 + i * 200, 692, f"{name}: Connections", "small")
    s += t(316, 732, "Значения в предпросмотре — синтетические; данные Zabbix появятся после публикации.", "xs muted")
    s += t(316, 772, "ЛИМИТЫ", "label muted")
    s += t(316, 798, "Рядов в наборе: 3 из 50 · длина подписи и цвета проверены", "small")
    s += icon("circle-check", 316, 812, "#1f6b43", 16) + t(340, 825, "Дублей узлов нет · метрика есть на всех трёх узлах", "small")
    # other widgets
    s += box(1132, 196, 412, 712)
    s += t(1156, 230, "ДРУГИЕ ВИДЖЕТЫ PostgreSQL", "label muted")
    other = [("circle-check", "#1f6b43", "TPS · График", "2 → 3 ряда, как выше"),
             ("circle-check", "#1f6b43", "Cache hit ratio · График", "2 → 3 ряда"),
             ("circle-x", "#a32935", "Locks · Значение элемента", "один узел на виджет"),
             ("circle-x", "#a32935", "Replication lag · Значение", "один узел на виджет")]
    y = 248
    for ic, color, name, note in other:
        s += icon(ic, 1156, y + 6, color, 18) + t(1182, y + 20, name, "small", weight="600") + t(1182, y + 40, note, "xs muted")
        y += 56
    s += box(1156, y + 6, 364, 164, fill="#fff5f5", stroke="#f2b8b5", rx=10)
    s += icon("circle-x", 1170, y + 20, "#a32935", 18)
    s += t(1196, y + 34, "Для pg-b-03 нет своих виджетов", "small", "#a32935", weight="700")
    s += lines(1196, y + 58, ["«Locks» и «Replication lag» показывают", "по одному узлу. Для третьего узла их",
                              "нужно добавить в редакторе (копией", "с новым узлом) — мастер не создаёт", "виджеты и не меняет раскладку."], "small", "#344054")
    s += button(1156, y + 184, 364, "Открыть редактор страницы")
    s += t(1156, y + 252, "Или уберите pg-b-03 из целей.", "xs muted")
    s += bar([("Назад", "secondary", 120)], [("Сохранить черновик", "secondary", 200), ("Далее: итог", "primary", 160)])
    write(s + "</svg>\n", "09-05_P1-05F_cardinality.svg", chips=(DIRTY,))


# ================================================================== 09-06 transformation summary (Clone)
def summary():
    s = head("привязки: итог преобразования", "Светлый макет итога клонирования перед публикацией: новое имя, то же подключение, "
             "оригинал не меняется, сводка изменённых, неизменных и оставшихся на исходных узлах ссылок, нерешённое, "
             "переход к публикации без записи на этом шаге.",
             ["circle-check", "alert-triangle", "circle-x", "info-circle", "minus"])
    s += sidebar(ORIGIN) + title("Клонировать панель: итог") + steps(STEPS_CL, 6)
    s += box(292, 196, 1252, 100, rx=14)
    s += t(316, 228, "НОВАЯ ПАНЕЛЬ", "label muted")
    s += box(316, 240, 360, 40, stroke="#3156a3", rx=8, sw=2) + t(330, 266, "Базы данных: Pilot", "body")
    s += t(700, 252, "Подключение: Лаборатория 7.4 (то же)", "small")
    s += t(700, 274, "Исходная «Базы данных: Prod» не изменяется.", "small muted")
    s += t(1520, 252, "Копируется вся структура:", "small", anchor="end")
    s += t(1520, 274, "3 страницы, 19 виджетов, связи виджетов", "small muted", anchor="end")
    s += box(292, 312, 820, 596)
    s += t(316, 346, "ЧТО ПРОИЗОЙДЁТ С ССЫЛКАМИ", "label muted")
    blocks = [
        ("circle-check", "#1f6b43", "Изменятся: 40 ссылок в 14 виджетах", ["pg-a-01 → pg-b-01, pg-a-02 → pg-b-02, ch-a-01 → ch-b-01, ch-a-02 → ch-b-02",
                                                                         "3 графика получат третий ряд pg-b-03", "2 маски: pg-a-* → pg-b-*, ClickHouse: * без изменений имени"]),
        ("minus", "#98a2b3", "Не меняются: 2 ссылки (выбрано «оставить исходный»)", ["Zabbix server в «Problems» и «Server health»"]),
        ("alert-triangle", "#8a4b00", "Останутся на исходных узлах: 4 виджета · подтверждено", ["Top queries, Slow queries, Problems (фильтр), Disk I/O (ряд 2)"]),
        ("circle-x", "#a32935", "Не решено: 0", ["Блокировок нет: можно переходить к публикации."]),
    ]
    y = 364
    for ic, color, head_, ls in blocks:
        s += icon(ic, 316, y + 4, color, 20) + t(346, y + 20, head_, "body", color if ic in ("alert-triangle", "circle-x") else None, weight="700")
        s += lines(346, y + 46, ls, "small muted")
        y += 34 + 22 * len(ls) + 22
    s += t(316, y + 6, "Сравнение по виджетам и полям — в «Публикации», шаг «Изменения».", "xs muted")
    s += box(1132, 312, 412, 596)
    s += t(1156, 346, "СОХРАНЕНО В ЧЕРНОВИКЕ", "label muted")
    s += lines(1156, 374, ["Соответствия узлов и элементов данных,", "решения по шаблонам и порядок слотов —",
                           "как операции копии, только на этом", "компьютере."], "small")
    s += icon("info-circle", 1156, 476, "#667085", 18)
    s += lines(1182, 490, ["Идентификаторы Zabbix и подключение", "в файлы экспорта не попадают.",
                           "При смене подключения, прав или снимка", "соответствия проверяются заново."], "xs muted", step=18)
    s += t(1156, 594, "ДАЛЬШЕ", "label muted")
    s += lines(1156, 620, ["«Публикация»: владелец и доступ,", "проверки, сравнение и подтверждение.", "На этом шаге сервер не меняется."], "small")
    s += bar([("Назад", "secondary", 120)], [("Сохранить черновик", "secondary", 200), ("К публикации", "primary", 180)])
    write(s + "</svg>\n", "09-06_P1-05E_clone-summary.svg", chips=(DIRTY,))


# ================================================================== 09-07 bindings status
def bindings_status():
    s = head("привязки: состояние и повторная проверка", "Светлый макет раздела «Привязки» после смены контекста: "
             "состояние каждой привязки (проверена, устарела, недействительна) с причиной, повторная проверка, "
             "повторный выбор и сброс, история изменений; секреты и идентификаторы не уходят в файлы.",
             ["circle-check", "alert-triangle", "circle-x", "refresh", "info-circle", "chevron-right"])
    s += sidebar(ORIGIN) + title("Привязки")
    s += box(292, 146, 1252, 74, fill="#fff8ed", stroke="#f3d3a6", rx=14)
    s += icon("alert-triangle", 316, 166, "#8a4b00", 24)
    s += t(352, 178, "Изменились права учётной записи: привязки проверяются заново", "h3", "#8a4b00")
    s += t(352, 204, "Готово 3 из 6. Проверенные строки повторно подтверждены, остальные ждут проверки. План публикации недействителен.", "small", "#5c3a00")
    s += box(292, 236, 860, 672)
    s += t(316, 270, "ПРИВЯЗКИ ЧЕРНОВИКА", "label muted")
    s += button(948, 254, 180, "Проверить все", icon_name="refresh", h=36)
    s += thead(316, 300, 812, [(352, "ИСТОЧНИК → ЦЕЛЬ"), (640, "СОСТОЯНИЕ"), (900, "ПРИЧИНА")])
    rows = [("circle-check", "#1f6b43", "pg-a-01 → pg-b-01", "Проверена заново", "14:52 · 23 ссылки"),
            ("circle-check", "#1f6b43", "pg-a-02 → pg-b-02", "Проверена заново", "14:52 · 7 ссылок"),
            ("alert-triangle", "#8a4b00", "ch-a-01 → ch-b-01", "Устарела", "ждёт проверки"),
            ("alert-triangle", "#8a4b00", "ch-a-02 → ch-b-02", "Устарела", "ждёт проверки"),
            ("circle-x", "#a32935", "(новый слот) → pg-b-03", "Недействительна", "узел не найден или нет доступа"),
            ("circle-check", "#1f6b43", "Zabbix server", "Оставлен исходный", "решение пользователя")]
    y = 338
    for ic, color, pair, st, why in rows:
        s += hline(316, 1128, y + 46)
        s += icon(ic, 326, y + 14, color, 18) + t(352, y + 29, pair, "mono")
        s += t(640, y + 29, st, "small", color if ic != "circle-check" else None, weight="650") + t(900, y + 29, why, "small muted")
        y += 50
    s += box(316, y + 16, 812, 100, fill="#fff5f5", stroke="#f2b8b5", rx=10)
    s += icon("circle-x", 330, y + 30, "#a32935", 18)
    s += t(356, y + 44, "pg-b-03 больше не виден учётной записи", "small", "#a32935", weight="700")
    s += t(356, y + 68, "Zabbix не различает «нет узла» и «нет прав». Автоматически другой узел не выбирается.", "small")
    s += button(356, y + 78, 200, "Выбрать другой", h=32) + button(568, y + 78, 200, "Убрать слот", h=32)
    s += t(316, y + 148, "При изменении прав проверяются все строки; при новом снимке или каталоге — только затронутые.", "xs muted")
    s += box(1172, 236, 372, 672)
    s += t(1196, 270, "ПРАВИЛА", "label muted")
    rules = [("Подключение или сервер другой", "все привязки — недействительны"),
             ("Обновлена версия Zabbix", "все — устарели, проверить"),
             ("Изменились права", "все — устарели, проверить"),
             ("Новый снимок (скопировано заново)", "затронутые строки — проверить"),
             ("Новая версия каталога метрик", "строки этого каталога — проверить")]
    y = 296
    for k, v in rules:
        s += t(1196, y, k, "small", weight="650") + t(1196, y + 20, v, "xs muted")
        y += 48
    s += f'  <line x1="1196" y1="{y}" x2="1520" y2="{y}" stroke="#e4e7ec"/>\n'
    s += icon("chevron-right", 1192, y + 16, "#475467", 18) + t(1216, y + 30, "История привязок", "small", weight="650")
    s += t(1216, y + 50, "кто и когда выбрал каждое соответствие", "xs muted")
    s += icon("info-circle", 1196, y + 80, "#667085", 18)
    s += lines(1222, y + 94, ["Привязки хранятся только на этом", "компьютере, вместе с черновиком.",
                              "В файлы экспорта не попадают ни", "идентификаторы Zabbix, ни подключение."], "xs muted", step=18)
    s += button(1196, 836, 324, "Сбросить все привязки", "danger")
    write(s + "</svg>\n", "09-07_P1-05G_bindings-status.svg", chips=(SAVED, ("Привязки устарели", "#fff4e5", "#8a4b00", 170)))


if __name__ == "__main__":
    scope_inventory()
    target_picker()
    mapping_table()
    clone_decisions()
    cardinality()
    summary()
    bindings_status()
