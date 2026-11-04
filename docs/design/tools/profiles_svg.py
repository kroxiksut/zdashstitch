"""Review mockups for P1-06C/D (style profiles and widget blueprints):
14-01 «Библиотека → Профили оформления», 14-02 «Библиотека → Заготовки виджетов»,
14-03 «Импорт файлов» with profile/blueprint files and an identifier conflict,
14-04 «Сохранить оформление как профиль» (editor dialog), 14-05 «Создать заготовку» (editor dialog).
P1-06E/F: 14-06 and 14-07 «Профиль оформления» editor (svggraph section; item section with an invalid field),
14-08 «Добавить виджет → Из заготовок» parameters step in a local draft, 14-09 the same step in a server copy.
P1-06G: 14-10 «Применить профиль» (scope, compatibility, before → after), 14-11 «Обновить из профиля» (modes,
value provenance), 14-12 inspector with «из профиля» field states and the outdated-version banner.

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/profiles_svg.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402
from publication_svg import (  # noqa: E402
    AMBER, BLUE, GREEN, NEUTRAL, RED, box, button, checkbox, chip, head, icon, radio, select, t,
)
from import_library_svg import (  # noqa: E402
    X0, kv, page_title, pills, row_line, search, table_head, wrap, write,
)
from editor_svg import (  # noqa: E402
    BORDER, CLICKHOUSE, canvas, chipw, editor_base, outline, page_tabs, title as editor_title,
)

OUT = "docs/design/assets/14-profiles/"
OFFLINE = "offline"
LIB_ICONS = ["search", "info-circle", "circle-check", "alert-triangle", "circle-x", "plus", "upload",
             "download", "copy", "palette", "chevron-down", "refresh"]
EDITOR_ICONS = ["plus", "arrow-back-up", "arrow-forward-up", "zoom-in", "zoom-out", "check", "chevron-down",
                "chevron-right", "search", "x", "ban", "info-circle", "alert-triangle", "circle-check", "copy",
                "grip-vertical", "pencil", "palette", "circle-x"]
PALETTE = ["#3156a3", "#8b5cf6", "#1f6b43"]  # series palette of the sample profile (all >= 3:1 on white)
CRUMB = "Черновики  /  Базы данных: сводная  /  Структура  /  ClickHouse"
DIRTY = ("Изменена", "#fff4e5", "#8a4b00", 100)
LOCAL = ("Локальная", "#ecfdf3", "#1f6b43", 110)


def write_editor(svg, name, crumb=CRUMB, chips=(DIRTY, LOCAL), mode=OFFLINE):
    svg = appbar.apply(svg, active="drafts", mode=mode, depth_prefix="../../../../",
                       breadcrumb=crumb, chips=list(chips))
    os.makedirs(OUT, exist_ok=True)
    with open(OUT + name, "w", encoding="utf-8", newline="") as f:
        f.write(svg)
    print("written", OUT + name)


def hline(x1, x2, y, color="#e4e7ec"):
    return f'  <line x1="{x1}" y1="{y}" x2="{x2}" y2="{y}" stroke="{color}"/>\n'


def swatches(x, y, colors, size=16, gap=6):
    s = ""
    for c in colors:
        s += f'  <rect x="{x}" y="{y}" width="{size}" height="{size}" rx="3" fill="{c}"/>\n'
        x += size + gap
    return s


def library_shell(section, subtitle):
    """Title, sub-navigation and the empty table/detail frames of «Библиотека» (as in 04-01)."""
    s = "  <!-- Title -->\n" + page_title("Библиотека", subtitle)
    s += button(1134, 100, 206, "Создать профиль", icon_name="plus") + button(1356, 100, 196, "Импорт файлов", icon_name="upload")
    s += box(X0, 176, 240, 732)
    nav = [("Снимки", "6"), ("Страницы", "2"), ("Каталоги метрик", "6"), ("Профили оформления", "6"), ("Заготовки виджетов", "4")]
    y = 196
    for name, n in nav:
        sel = name == section
        if sel:
            s += box(60, y, 216, 40, fill="#eef4ff", stroke="#eef4ff", rx=8)
        s += t(74, y + 26, name, "small", "#1d3f86" if sel else None, weight="700" if sel else None)
        s += t(262, y + 26, n, "small muted", anchor="end")
        y += 46
    s += icon("info-circle", 64, 448, "#667085", 18)
    s += wrap(90, 462, ["Профили и заготовки", "экспортируются отдельными", "файлами и в пакет панели", "не входят."], "xs muted", step=18)
    s += box(304, 176, 776, 732)
    s += box(1104, 176, 448, 732)
    return s


def mini_bars(x, y, w, h, colors):
    """Synthetic stacked-column preview: three series, labelled «Синтетика»."""
    s = box(x, y, w, h, fill="#ffffff", stroke="#e4e7ec", rx=8)
    s += chip(x + w - 96, y + 8, 86, "Синтетика", AMBER)
    base = y + h - 14
    gx = x + 16
    heights = [(18, 12, 8), (24, 14, 10), (30, 16, 12), (34, 20, 12), (40, 22, 14), (44, 26, 16), (50, 28, 18)]
    for hs in heights:
        yy = base
        for hh, c in zip(hs, colors):
            s += f'  <rect x="{gx}" y="{yy - hh}" width="22" height="{hh}" fill="{c}"/>\n'
            yy -= hh
        gx += 30
    s += f'  <line x1="{x + 12}" y1="{base}" x2="{x + w - 12}" y2="{base}" stroke="#7a8d9b"/>\n'
    s += t(x + 236, y + 40, "легенда: 2 строки", "xs muted")
    s += t(x + 236, y + 58, "ось Y от 0", "xs muted")
    s += t(x + 236, y + 76, "накопление", "xs muted")
    return s


# ================================================================== 14-01 library: style profiles (P1-06C)
def library_profiles():
    s = head("библиотека: профили оформления", "Светлый макет раздела «Библиотека → Профили оформления»: список профилей с типами "
             "виджетов, версией, использованием и происхождением, фильтры по типу и происхождению, поиск и сортировка; карточка "
             "выбранного профиля с совместимостью, использованием, предпросмотром на синтетике и действиями; два встроенных "
             "эталонных профиля; строки с состояниями «не проверено» и «ошибка в поле».", LIB_ICONS)
    s += library_shell("Профили оформления", "Снимки, страницы, каталоги метрик, профили оформления и заготовки виджетов на этом компьютере. Черновики — в разделе «Черновики».")
    for p in pills(328, 196, [("Все 6", 78, True), ("График 5", 98, False), ("Значение элемента данных 1", 230, False)]):
        s += p
    s += search(752, 193, 304, "Поиск по имени, описанию, тегу")
    for p in pills(328, 240, [("Встроенные и мои", 160, True), ("Мои", 64, False), ("Встроенные", 110, False), ("Из файла", 92, False)]):
        s += p
    s += select(816, 238, 240, "Сортировка: по имени", h=36)
    W = 728
    s += table_head(328, 292, W, [(342, "ИМЯ"), (548, "ТИПЫ ВИДЖЕТОВ"), (800, "ВЕРСИЯ"), (860, "ИСПОЛЬЗУЮТ"), (960, "ПРОИСХОЖДЕНИЕ")])
    rows = [
        ("Рост БД — столбцы", "График", "3", "12 · 2 черн.", "мой", None, True),
        ("Рост БД — столбцы", "График", "1", "—", "встроенный", None, False),
        ("Нагрузка CPU — линия", "График", "1", "—", "встроенный", None, False),
        ("Значения: крупно", "Значение элемента данных", "2", "9 · 2 черн.", "мой", None, False),
        ("Тёмные графики", "График", "5", "—", "из файла", ("Не проверено", NEUTRAL), False),
        ("Пилот: столбцы", "График", "2", "1 · 1 черн.", "из файла", ("Ошибка в поле", RED), False),
    ]
    y = 332
    for name, types, ver, used, origin, state, sel in rows:
        if sel:
            s += box(328, y, W, 50, fill="#eef4ff", stroke="#3156a3", rx=8, sw=2)
        else:
            s += row_line(328, y + 50, W)
        s += t(342, y + 31, name, "small", weight="650")
        s += t(548, y + 31, types, "small")
        s += t(800, y + 31, ver, "small")
        s += t(860, y + 31, used, "small", "#667085" if used == "—" else None)
        if state:
            s += chip(944, y + 12, 108 if state[0].startswith("Не") else 112, state[0], state[1])
        else:
            s += t(960, y + 31, origin, "small", "#667085" if origin != "мой" else None)
        y += 56
    s += t(328, y + 30, "«Используют» — виджеты · черновики. Использование, происхождение и время — локальные сведения;", "xs muted")
    s += t(328, y + 48, "в файл профиля они не попадают. «Не проверено» — файл для другого адаптера; «Ошибка в поле» — применить нельзя.", "xs muted")
    s += t(328, y + 66, "Встроенные — два эталона приложения, только чтение; «Рост БД — столбцы» (мой) — изменённая копия встроенного.", "xs muted")
    s += t(328, y + 96, "Пусто: «Профилей оформления пока нет. Создайте профиль из виджета в редакторе или импортируйте файл.»", "xs muted")
    # detail card
    s += icon("palette", 1128, 196, "#1d3f86", 20) + t(1156, 212, "Рост БД — столбцы", "h3")
    s += chip(1128, 226, 160, "Профиль оформления", BLUE) + chip(1296, 226, 50, "Мой", NEUTRAL)
    s += kv(1128, 284, "Типы виджетов", "График (svggraph)", 1252)
    s += kv(1128, 310, "Версия", "3 · Zabbix 7.4.0+ (вычислено)", 1252)
    s += kv(1128, 336, "Состав", "8 полей · палитра 3 цвета", 1252)
    s += kv(1128, 362, "Создан", "здесь, 24.10.2026", 1252)
    s += kv(1128, 388, "Изменён", "26.10.2026, 10:12", 1252)
    s += kv(1128, 414, "Используют", "12 виджетов в 2 черновиках", 1252)
    s += t(1252, 434, "из них на прежней версии — 4", "xs", "#8a4b00")
    s += hline(1128, 1528, 448)
    s += t(1128, 474, "ПРЕДПРОСМОТР · СИНТЕТИКА", "label muted")
    s += mini_bars(1128, 486, 400, 110, PALETTE)
    s += button(1128, 612, 400, "Изменить профиль", "primary")
    s += button(1128, 666, 194, "Дублировать", icon_name="copy") + button(1334, 666, 194, "Переименовать")
    s += button(1128, 720, 400, "Экспорт в файл…", icon_name="download")
    s += button(1128, 774, 400, "Удалить", "danger")
    s += icon("info-circle", 1128, 832, "#667085", 18)
    s += wrap(1154, 846, ["Черновики от профиля не зависят: значения", "останутся, метки «из профиля» будут сняты.",
                          "Встроенный профиль не меняется: «Дублировать»."], "xs muted", step=18)
    write(s + "</svg>\n", "14-profiles/14-01_P1-06C_library-profiles.svg", "library", mode=OFFLINE)


# ================================================================== 14-02 library: widget blueprints (P1-06C)
def library_blueprints():
    s = head("библиотека: заготовки виджетов", "Светлый макет раздела «Библиотека → Заготовки виджетов»: список заготовок с типом "
             "виджета, параметрами, размером по умолчанию, версией и происхождением; карточка выбранной заготовки с "
             "параметрами-ролями и метриками, тем, что не хранится, и действиями; заготовка для другого адаптера — «не проверено».",
             LIB_ICONS)
    s += library_shell("Заготовки виджетов", "Снимки, страницы, каталоги метрик, профили оформления и заготовки виджетов на этом компьютере. Черновики — в разделе «Черновики».")
    for p in pills(328, 196, [("Все 4", 78, True), ("График 2", 98, False), ("Значение элемента данных 2", 230, False)]):
        s += p
    s += search(752, 193, 304, "Поиск по имени, описанию, тегу")
    for p in pills(328, 240, [("Любое происхождение", 180, True), ("Мои", 64, False), ("Из файла", 92, False)]):
        s += p
    s += select(816, 238, 240, "Сортировка: по имени", h=36)
    W = 728
    s += table_head(328, 292, W, [(342, "ИМЯ"), (536, "ТИП"), (724, "ПАРАМЕТРЫ"), (862, "РАЗМЕР"), (920, "ВЕРСИЯ"), (972, "ПРОИСХОЖД.")])
    rows = [
        ("Нагрузка QPS — линия", "График", "1 роль · 2 метрики", "36 × 5", "1", "моя", None, True),
        ("Подключения PostgreSQL", "График", "1 роль · 1 метрика", "24 × 6", "2", "моя", None, False),
        ("Отставание репликации", "Значение элемента данных", "1 роль · 1 метрика", "12 × 3", "1", "из файла", None, False),
        ("Счётчик соединений", "Значение элемента данных", "1 роль · 1 метрика", "12 × 3", "1", "из файла", ("Не проверено", NEUTRAL), False),
    ]
    y = 332
    for name, typ, params, size, ver, origin, state, sel in rows:
        if sel:
            s += box(328, y, W, 50, fill="#eef4ff", stroke="#3156a3", rx=8, sw=2)
        else:
            s += row_line(328, y + 50, W)
        s += t(342, y + 31, name, "small", weight="650")
        s += t(536, y + 31, typ, "small")
        s += t(724, y + 31, params, "small")
        s += t(862, y + 31, size, "small")
        s += t(920, y + 31, ver, "small")
        if state:
            s += chip(944, y + 12, 108, state[0], state[1])
        else:
            s += t(972, y + 31, origin, "small", "#667085" if origin != "моя" else None)
        y += 56
    s += icon("info-circle", 328, y + 22, "#667085", 18)
    s += wrap(354, y + 36, ["Заготовки создаются из виджета в редакторе страницы («Создать заготовку…»); из библиотеки их нельзя создать с нуля.",
                            "Вставка — в редакторе: «Добавить виджет» → «Из заготовок». Источники при вставке не подбираются автоматически."], "xs muted", step=18)
    s += t(328, y + 92, "Пусто: «Заготовок виджетов пока нет. Создайте заготовку из виджета в редакторе или импортируйте файл.»", "xs muted")
    # detail
    s += t(1128, 212, "Нагрузка QPS — линия", "h3")
    s += chip(1128, 226, 150, "Заготовка виджета", BLUE) + chip(1286, 226, 50, "Моя", NEUTRAL)
    s += kv(1128, 284, "Тип виджета", "График (svggraph)", 1262)
    s += kv(1128, 310, "Размер", "36 × 5 ячеек по умолчанию", 1262)
    s += kv(1128, 336, "Версия", "1 · Zabbix 7.4.0+ (вычислено)", 1262)
    s += kv(1128, 362, "Создана", "из виджета «Queries per second»,", 1262)
    s += t(1262, 382, "26.10.2026, 11:40", "small", weight="600")
    s += kv(1128, 410, "Вставлена", "3 раза в 2 черновиках", 1262)
    s += hline(1128, 1528, 432)
    s += t(1128, 458, "ПАРАМЕТРЫ", "label muted")
    params = [("ch_hosts", "роль узлов сети · несколько узлов", "«Узлы с ClickHouse»"),
              ("qps", "метрика · числовой (с плавающей точкой)", "«Запросов в секунду»"),
              ("ips", "метрика · числовой (с плавающей точкой)", "«Вставок в секунду»")]
    y = 470
    for name, kind, desc in params:
        s += box(1128, y, 400, 48, fill="#f8fafc", stroke="#e4e7ec", rx=8)
        s += t(1140, y + 20, name, "mono") + t(1230, y + 20, kind, "xs")
        s += t(1140, y + 38, desc, "xs muted")
        y += 54
    s += t(1128, y + 20, "НЕ ХРАНИТ", "label muted")
    s += t(1128, y + 42, "положение, reference, узлы сети, элементы данных,", "xs muted")
    s += t(1128, y + 60, "маски и замещения, неизвестные поля", "xs muted")
    s += button(1128, 720, 194, "Дублировать", icon_name="copy") + button(1334, 720, 194, "Переименовать")
    s += button(1128, 774, 400, "Экспорт в файл…", icon_name="download")
    s += button(1128, 828, 400, "Удалить", "danger")
    s += t(1128, 892, "Вставленные виджеты от заготовки не зависят.", "xs muted")
    write(s + "</svg>\n", "14-profiles/14-02_P1-06C_library-blueprints.svg", "library", mode=OFFLINE)


# ================================================================== 14-03 import of profile/blueprint files (P1-06C, P1-03A)
def import_conflict():
    s = head("импорт файлов: профили и заготовки", "Светлый макет «Импорт файлов» с файлами профилей оформления и заготовок виджетов: "
             "распознанный вид, что станет, конфликт идентификатора с явным выбором «Заменить (новая версия)» или «Сохранить как "
             "копию», файл для другого адаптера, файл более новой версии формата и заготовка с маской узлов — отклонены.",
             ["circle-check", "alert-triangle", "circle-x", "chevron-down", "info-circle", "x", "upload"])
    s += "  <!-- Title -->\n" + t(X0, 92, "Библиотека  /  Импорт файлов", "xs muted")
    s += page_title("Импорт файлов", "Файлы панелей и страниц, пакеты ZIP, экспорты шаблонов Zabbix 7.4, профили оформления и заготовки виджетов.")
    s += box(X0, 176, 1012, 72, fill="#f8fafc", stroke="#7a8d9b", rx=14)
    s += f'  <rect x="{X0}" y="176" width="1012" height="72" rx="14" fill="none" stroke="#7a8d9b" stroke-dasharray="6 6"/>\n'
    s += icon("upload", X0 + 24, 202, "#475467", 22) + t(X0 + 60, 218, "Перетащите файлы сюда или выберите их", "body", None, None, "650")
    s += button(X0 + 560, 193, 200, "Выбрать файлы…", h=38) + button(X0 + 772, 193, 220, "Загрузить через браузер", h=38)
    s += box(X0, 264, 1012, 644)
    s += table_head(64, 280, 980, [(78, "ФАЙЛ"), (350, "РАСПОЗНАН КАК"), (540, "СОДЕРЖИМОЕ"), (800, "СТАНЕТ")])
    y = 320
    # row 1: conflict
    s += row_line(64, y + 150, 980)
    s += icon("alert-triangle", 72, y + 16, "#8a4b00", 20)
    s += t(100, y + 31, "db-growth.style-profile.yaml", "mono")
    s += t(350, y + 31, "Профиль оформления", "small", weight="600")
    s += t(540, y + 31, "«Рост БД — столбцы» · версия 4", "small")
    s += t(800, y + 31, "профиль в библиотеке", "small", weight="600")
    s += icon("x", 1016, y + 18, "#667085", 18, "Убрать файл из списка")
    s += t(100, y + 60, "Профиль с таким же идентификатором уже есть в библиотеке: версия 3, изменён 26.10.2026, используется 12 виджетами.", "xs", "#8a4b00")
    s += radio(112, y + 90, True) + t(132, y + 96, "Заменить (новая версия)", "small", weight="650")
    s += t(350, y + 96, "идентификатор сохраняется; версия станет 5 (больше обеих + 1); у виджетов «из профиля» — «обновлён до версии 5»", "xs muted")
    s += radio(112, y + 122, False) + t(132, y + 128, "Сохранить как копию", "small", weight="650")
    s += t(350, y + 128, "новый идентификатор, имя «Рост БД — столбцы (импорт)»; существующий профиль не меняется", "xs muted")
    y += 156
    # row 2: blueprint ok
    s += row_line(64, y + 76, 980)
    s += icon("circle-check", 72, y + 16, "#1f6b43", 20)
    s += t(100, y + 31, "cpu-load.widget-blueprint.yaml", "mono")
    s += t(350, y + 31, "Заготовка виджета", "small", weight="600")
    s += t(540, y + 31, "«Загрузка и нагрузка CPU»", "small")
    s += t(800, y + 31, "заготовка в библиотеке", "small", weight="600")
    s += icon("x", 1016, y + 18, "#667085", 18, "Убрать файл из списка")
    s += t(100, y + 58, "График · 3 параметра: роль узлов сети, 2 метрики · Zabbix 7.4.0+", "xs muted")
    y += 82
    # row 3: other adapter
    s += row_line(64, y + 76, 980)
    s += icon("alert-triangle", 72, y + 16, "#8a4b00", 20)
    s += t(100, y + 31, "values-big.style-profile.yaml", "mono")
    s += t(350, y + 31, "Профиль оформления", "small", weight="600")
    s += t(540, y + 31, "«Значения: крупно» · адаптер 8.0", "small")
    s += t(800, y + 31, "профиль в библиотеке", "small", weight="600")
    s += t(100, y + 58, "Создан для адаптера Zabbix 8.0, которого в этой версии приложения нет: сохранится, но применить его будет нельзя.", "xs", "#8a4b00")
    s += chip(936, y + 42, 108, "Не проверено", NEUTRAL)
    y += 82
    # row 4: newer format
    s += row_line(64, y + 76, 980)
    s += icon("circle-x", 72, y + 16, "#a32935", 20)
    s += t(100, y + 31, "profile-next.style-profile.yaml", "mono")
    s += t(350, y + 31, "Не распознан", "small", weight="600")
    s += t(540, y + 31, "zdashstitch.style-profile/v2", "mono")
    s += t(800, y + 31, "не импортируется", "small", "#a32935", weight="600")
    s += t(100, y + 58, "Файл создан более новой версией ZDashstitch (формат v2): обновите приложение. Файл не изменён.", "xs", "#a32935")
    y += 82
    # row 5: blueprint with host mask
    s += row_line(64, y + 76, 980)
    s += icon("circle-x", 72, y + 16, "#a32935", 20)
    s += t(100, y + 31, "haproxy.widget-blueprint.yaml", "mono")
    s += t(350, y + 31, "Заготовка виджета", "small", weight="600")
    s += t(540, y + 31, "«HAProxy» · График", "small")
    s += t(800, y + 31, "не импортируется", "small", "#a32935", weight="600")
    s += t(100, y + 58, "Данные противоречивы (blueprint_host_pattern): заготовка не может содержать имена или маски узлов сети.", "xs", "#a32935")
    y += 82
    s += t(64, y + 26, "Профили и заготовки — отдельные файлы: в пакет панели они не входят и из пакета не читаются.", "xs muted")
    s += t(64, y + 46, "Повторный импорт того же файла без конфликта идентификатора создаёт новый объект; существующие не перезаписываются.", "xs muted")
    # summary
    s += box(1080, 176, 472, 732)
    s += t(1104, 210, "ИТОГ ПЕРЕД ИМПОРТОМ", "label muted")
    for i, (k, v) in enumerate([("Профили", "2 (1 заменит версию 3 → 5)"), ("Заготовки", "1"), ("Не импортируется", "2 файла"), ("Нужен выбор", "0")]):
        s += kv(1104, 248 + i * 30, k, v, 1260)
    s += hline(1104, 1528, 370)
    s += icon("info-circle", 1104, 388, "#667085", 18)
    s += wrap(1130, 402, ["Файлы — недоверенные данные. Они проверяются",
                          "в памяти; скрипты, внешние ссылки и сетевые",
                          "ресурсы не выполняются. Каждый файл",
                          "сохраняется целиком или не сохраняется совсем."], "small muted")
    s += box(1104, 510, 424, 94, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
    s += icon("info-circle", 1118, 524, "#1d3f86", 18)
    s += wrap(1144, 538, ["Замена сохраняет идентификатор: черновики",
                          "не меняются, у виджетов «из профиля» появится",
                          "подпись «обновлён до версии 5»."], "small", "#344054")
    s += button(1104, 836, 140, "Отмена") + button(1256, 836, 272, "Импортировать", "primary")
    write(s + "</svg>\n", "14-profiles/14-03_P1-06C_import-conflict.svg", "library", mode=OFFLINE)


# ================================================================== 14-04 extract profile (P1-06D)
def extract_profile():
    s = head("редактор: сохранить оформление как профиль", "Светлый макет окна «Сохранить оформление как профиль» поверх редактора: "
             "поля виджета «График» по классам «в профиль» (включены), «по выбору» (выключены по умолчанию) и «никогда» "
             "(только перечислены), палитра рядов, имя и описание, новый профиль или раздел существующего, итог и пометка "
             "исходного виджета «из профиля»; виджет не меняется.", EDITOR_ICONS)
    s += editor_base()
    s += '  <!-- Dialog -->\n  <rect x="0" y="72" width="1600" height="928" fill="#17213a" fill-opacity=".45"/>\n'
    s += box(240, 100, 1120, 840, stroke="#c9d3ea")
    s += icon("palette", 272, 128, "#1d3f86", 24) + t(306, 146, "Сохранить оформление как профиль", "title")
    s += icon("x", 1320, 126, "#475467", 22, "Закрыть")
    s += t(272, 176, "Из виджета «Queries per second» (График) на странице «ClickHouse». Значения копируются в профиль; виджет не меняется.", "small muted")
    # left: field classes
    LX = 272
    s += icon("circle-check", LX, 198, "#1f6b43", 18) + t(LX + 26, 212, "В ПРОФИЛЬ · 7", "label muted")
    s += t(LX + 160, 212, "включены по умолчанию", "xs muted")
    fields = [("Вид ряда", "столбцы", "ds.N.type"), ("Накопление", "да", "ds.N.stacked"), ("Прозрачность", "3", "ds.N.transparency"),
              ("Левая ось", "показывать", "lefty"), ("Легенда", "2 строки · статистика", "legend, legend_lines"),
              ("Показывать проблемы", "да", "show_problems")]
    y = 226
    for name, val, code in fields:
        s += checkbox(LX, y + 6, True) + t(LX + 30, y + 21, name, "small") + t(LX + 230, y + 21, val, "small", weight="600")
        s += t(LX + 560, y + 21, code, "mono", anchor="end")
        y += 32
    s += checkbox(LX, y + 6, True) + t(LX + 30, y + 21, "Палитра рядов", "small") + swatches(LX + 230, y + 6, PALETTE)
    s += t(LX + 310, y + 21, "3 цвета по порядку наборов", "xs muted") + t(LX + 560, y + 21, "ds.N.color", "mono", anchor="end")
    y += 32
    s += t(LX + 30, y + 16, "Заливка не включена: не применяется при текущем значении «Вид ряда» (столбцы).", "xs muted")
    y += 34
    s += icon("info-circle", LX, y - 2, "#8a4b00", 18) + t(LX + 26, y + 12, "ПО ВЫБОРУ · 3", "label muted")
    s += t(LX + 160, y + 12, "выключены по умолчанию; применяются с предупреждением", "xs muted")
    y += 26
    optional = [("Минимум левой оси", "0", "lefty_min", "зависит от области значений"),
                ("Единицы левой оси", "статически «%»", "lefty_units, lefty_static_units", "зависит от единиц метрики"),
                ("Показ заголовка", "да", "view_mode", "атрибут виджета")]
    for name, val, code, why in optional:
        s += checkbox(LX, y + 6, False) + t(LX + 30, y + 21, name, "small") + t(LX + 200, y + 21, val, "small", weight="600")
        s += t(LX + 340, y + 21, why, "xs muted")
        y += 32
    s += t(LX + 30, y + 10, "lefty_min · lefty_units, lefty_static_units · view_mode", "mono")
    y += 20
    y += 10
    s += icon("ban", LX, y - 2, "#667085", 18) + t(LX + 26, y + 12, "НИКОГДА · 11", "label muted")
    y += 26
    s += wrap(LX, y + 14, ["Источники (наборы 1–2: роль «clickhouse», метрики «Queries per second», «Inserts per second»),",
                           "период (последний час), агрегация, подписи наборов, замещения, связь периода с «Replication lag»,",
                           "неизвестные поля (2), положение 36, 0 · 36 × 5."], "xs muted", step=18)
    y += 76
    s += checkbox(LX, y, True) + t(LX + 30, y + 15, "Включать значения, равные умолчанию Zabbix (профиль будет возвращать их)", "small")
    # right: name and target
    RX = 880
    s += box(RX, 198, 448, 640, fill="#f8fafc", stroke="#e4e7ec", rx=12)
    s += t(RX + 24, 228, "Имя профиля", "xs muted")
    s += box(RX + 24, 236, 400, 38, stroke=BORDER, rx=8) + t(RX + 36, 261, "Рост БД — столбцы", "body")
    s += icon("alert-triangle", RX + 24, 282, "#8a4b00", 16)
    s += t(RX + 46, 294, "Другой профиль называется так же. Имя — не идентификатор,", "xs", "#8a4b00")
    s += t(RX + 46, 310, "но другое имя поможет не перепутать.", "xs", "#8a4b00")
    s += t(RX + 24, 338, "Описание", "xs muted")
    s += box(RX + 24, 346, 400, 56, stroke=BORDER, rx=8) + t(RX + 36, 370, "Столбцы с накоплением для графиков роста", "small")
    s += t(RX + 36, 390, "размера баз данных", "small")
    s += t(RX + 24, 428, "КУДА", "label muted")
    s += radio(RX + 34, 450, True) + t(RX + 54, 456, "Новый профиль", "small", weight="650")
    s += radio(RX + 34, 480, False) + t(RX + 54, 486, "Раздел «График» в существующий профиль:", "small")
    s += select(RX + 54, 498, 370, "Выберите профиль", h=34).replace('class="body">Выберите', 'class="body" fill="#667085">Выберите')
    s += t(RX + 54, 552, "Раздел «График» выбранного профиля будет заменён (новая версия).", "xs muted")
    s += hline(RX + 24, RX + 424, 570)
    s += t(RX + 24, 596, "ИТОГ", "label muted")
    s += wrap(RX + 24, 620, ["В профиль войдут 7 полей и палитра из 3 цветов.", "3 поля «по выбору» не включены; 11 полей",
                             "не входят в профиль никогда."], "small")
    s += icon("info-circle", RX + 24, 690, "#667085", 18)
    s += wrap(RX + 50, 704, ["Виджет будет помечен «из профиля «Рост БД — столбцы»»;",
                             "его значения не меняются (служебная отметка, не шаг",
                             "отмены). Применить профиль к другим — отдельное действие."], "xs muted", step=18)
    s += t(RX + 24, 776, "Источники, период, фильтры и расположение", "xs muted")
    s += t(RX + 24, 794, "в профиль не входят никогда.", "xs muted")
    s += button(956, 880, 140, "Отмена") + button(1108, 880, 220, "Сохранить в библиотеку", "primary")
    write_editor(s + "</svg>\n", "14-04_P1-06D_extract-profile.svg")


# ================================================================== 14-05 create blueprint (P1-06D)
def create_blueprint():
    s = head("редактор: создать заготовку", "Светлый макет окна «Создать заготовку» поверх редактора: источники виджета «График» "
             "заменяются параметрами (роль узлов сети, метрики каталога), препятствия для сохранения — замещения по маске, "
             "связь периода с другим виджетом и неизвестные поля — требуют явного решения; имя, размер по умолчанию, что "
             "хранится и не хранится; предупреждение сканера значений; сохранение недоступно до решения.", EDITOR_ICONS)
    s += editor_base()
    s += '  <!-- Dialog -->\n  <rect x="0" y="72" width="1600" height="928" fill="#17213a" fill-opacity=".45"/>\n'
    s += box(240, 100, 1120, 840, stroke="#c9d3ea")
    s += t(272, 146, "Создать заготовку из «Queries per second»", "title")
    s += icon("x", 1320, 126, "#475467", 22, "Закрыть")
    s += t(272, 176, "График · 36 × 5 ячеек станут размером по умолчанию. Виджет в черновике не меняется.", "small muted")
    LX = 272
    s += t(LX, 212, "ИСТОЧНИКИ → ПАРАМЕТРЫ", "label muted")
    s += t(LX + 220, 212, "узлы сети и элементы данных становятся параметрами", "xs muted")
    s += table_head(LX, 224, 592, [(LX + 26, "ПОЛЕ"), (LX + 130, "СЕЙЧАС В ВИДЖЕТЕ"), (LX + 340, "ПАРАМЕТР")])
    rows = [
        ("ds.0.hosts.0", "роль «clickhouse» · все узлы", "ch_hosts", "роль · несколько узлов"),
        ("ds.0.items.0", "метрика «Queries per second»", "qps", "метрика · числовой · —"),
        ("ds.1.hosts.0", "роль «clickhouse» · все узлы", "ch_hosts", "тот же параметр"),
        ("ds.1.items.0", "метрика «Inserts per second»", "ips", "метрика · числовой · —"),
    ]
    y = 262
    for field, now, param, kind in rows:
        s += row_line(LX, y + 40, 592)
        s += icon("circle-check", LX, y + 10, "#1f6b43", 18)
        s += t(LX + 26, y + 25, field, "mono") + t(LX + 130, y + 25, now, "small")
        s += box(LX + 340, y + 6, 92, 28, stroke=BORDER, rx=6) + t(LX + 348, y + 25, param, "mono")
        s += t(LX + 440, y + 25, kind, "xs muted")
        y += 44
    s += t(LX, y + 18, "Имя параметра — латиница, цифры, «_» и «-», с буквы. Описание и число узлов берутся из роли,", "xs muted")
    s += t(LX, y + 36, "тип значения и единицы — из метрики каталога; «—» — у метрики нет единиц.", "xs muted")
    y += 60
    s += icon("circle-x", LX, y - 2, "#a32935", 18) + t(LX + 26, y + 12, "ПРЕПЯТСТВИЯ · 3 (БЕЗ РЕШЕНИЯ 2)", "label muted")
    s += t(LX + 290, y + 12, "каждому — явное решение; молча не отбрасывается", "xs muted")
    y += 24
    blockers = [
        ("or.0.hosts.0 = «ch-a-*»", "Замещения по маске узлов не хранятся в заготовке.", "Сохранить без замещений (or.*)", "Отмена"),
        ("time_period._reference → «Replication lag»", "Связь с другим виджетом: у заготовки нет соседей.", "Убрать связь: период — умолчание Zabbix", "Отмена"),
        ("Неизвестные поля · 2 (ds.0.vendor_hint, vendor_layout)", "Заготовка создаёт новый виджет; неизвестные поля в нём запрещены.", "Сохранить без неизвестных полей", "Отмена"),
    ]
    for i, (what, why, fix, _) in enumerate(blockers):
        s += box(LX, y, 592, 78, fill="#fef3f2", stroke="#f2b8b5", rx=10)
        s += t(LX + 14, y + 22, what, "mono") if i != 2 else t(LX + 14, y + 22, what, "small", weight="650")
        s += t(LX + 14, y + 42, why, "xs", "#a32935")
        s += radio(LX + 24, y + 62, i == 0) + t(LX + 44, y + 67, fix, "small", weight="650")
        s += radio(LX + 420, y + 62, False) + t(LX + 440, y + 67, "Решение не выбрано", "small", "#667085")
        y += 86
    # right
    RX = 880
    s += box(RX, 198, 448, 560, fill="#f8fafc", stroke="#e4e7ec", rx=12)
    s += t(RX + 24, 228, "Имя заготовки", "xs muted")
    s += box(RX + 24, 236, 400, 38, stroke=BORDER, rx=8) + t(RX + 36, 261, "Нагрузка QPS — линия", "body")
    s += t(RX + 24, 300, "Описание", "xs muted")
    s += box(RX + 24, 308, 400, 38, stroke=BORDER, rx=8) + t(RX + 36, 333, "Запросы и вставки в секунду по узлам роли", "small")
    s += t(RX + 24, 376, "ХРАНИТСЯ", "label muted")
    s += wrap(RX + 24, 400, ["тип, имя по умолчанию, размер 36 × 5, все настройки", "отображения, период (свой), агрегация, фильтры",
                             "проблем, цвета рядов"], "small")
    s += t(RX + 24, 480, "НЕ ХРАНИТСЯ", "label muted")
    s += wrap(RX + 24, 504, ["положение, reference и связи, узлы сети и элементы", "данных (вместо них — параметры), маски и замещения,",
                             "неизвестные поля, ID сервера и подключение"], "small")
    s += box(RX + 24, 584, 400, 80, fill="#fff8ed", stroke="#f3d3a6", rx=10)
    s += icon("alert-triangle", RX + 38, 598, "#8a4b00", 18)
    s += wrap(RX + 64, 612, ["Подпись набора 1 «ch-a-01 QPS» похожа на имя", "узла сети: проверьте текст. Сохранение", "не блокируется."], "small", "#5c3a00", step=18)
    s += icon("info-circle", RX + 24, 686, "#667085", 18)
    s += wrap(RX + 50, 700, ["При вставке источники не подбираются автоматически:", "роль и метрику выбирает пользователь при вставке."], "xs muted", step=18)
    s += box(RX, 776, 448, 62, fill="#fef3f2", stroke="#f2b8b5", rx=10)
    s += icon("circle-x", RX + 14, 790, "#a32935", 18)
    s += wrap(RX + 40, 804, ["Сохранить нельзя: 2 препятствия без решения.", "Виджет в черновике не меняется."], "small", "#a32935", step=18)
    s += button(956, 880, 140, "Отмена") + button(1108, 880, 220, "Сохранить в библиотеку", "disabled")
    write_editor(s + "</svg>\n", "14-05_P1-06D_create-blueprint.svg")


# ================================================================== P1-06E/F helpers
EDIT_ICONS = LIB_ICONS + ["ban", "x", "check", "pencil", "template"]
INSERT_ICONS = EDITOR_ICONS + ["template"]
UNSAVED = ("Не сохранено", "#fff4e5", "#8a4b00", 124)
COPY = ("Копия с сервера", "#eef4ff", "#1d3f86", 140)
LAB_RO = "lab_read_only"
CRUMB_COPY = "Черновики  /  Базы данных: Prod  /  Структура  /  ClickHouse"


def profile_editor_shell(name, subtitle, sections, selected, pname, pdesc, ptags, save_kind="primary"):
    """Breadcrumb, title, Save/Cancel, the sections column with the profile name block, and the two empty frames."""
    s = "  <!-- Title -->\n" + t(X0, 92, "Библиотека  /  Профили оформления  /  " + name, "xs muted")
    s += page_title("Профиль «" + name + "»", subtitle)
    s += chip(1104, 101, UNSAVED[3], UNSAVED[0], AMBER)
    s += button(1244, 96, 128, "Отмена") + button(1384, 96, 168, "Сохранить", save_kind)
    s += box(X0, 176, 240, 732)
    s += t(64, 206, "РАЗДЕЛЫ ПРОФИЛЯ", "label muted")
    y = 220
    for label, code in sections:
        sel = label == selected
        if sel:
            s += box(60, y, 216, 44, fill="#eef4ff", stroke="#eef4ff", rx=8)
        s += t(74, y + 20, label, "small", "#1d3f86" if sel else None, weight="700" if sel else "600")
        s += t(74, y + 36, code, "mono")
        y += 50
    if len(sections) < 2:
        s += icon("plus", 64, y + 6, "#3156a3", 18) + t(88, y + 20, "Добавить раздел…", "small", "#3156a3", weight="650")
        s += t(64, y + 42, "Значение элемента данных (item)", "xs muted")
        y += 60
    else:
        s += t(64, y + 18, "Типы с изменением полей: оба", "xs muted")
        s += t(64, y + 36, "раздела уже есть (реестр 7.4).", "xs muted")
        y += 54
    s += hline(64, 272, y + 6)
    y += 30
    s += t(64, y, "ПРОФИЛЬ", "label muted")
    s += t(64, y + 26, "Имя", "xs muted") + box(64, y + 32, 208, 36, stroke=BORDER, rx=8) + t(76, y + 56, pname, "small")
    s += t(64, y + 92, "Описание", "xs muted") + box(64, y + 98, 208, 56, stroke=BORDER, rx=8)
    for i, line in enumerate(pdesc):
        s += t(76, y + 120 + i * 18, line, "xs")
    s += t(64, y + 178, "Теги", "xs muted") + box(64, y + 184, 208, 36, stroke=BORDER, rx=8) + t(76, y + 208, ptags, "small")
    s += t(64, y + 244, "Имя — не идентификатор; совпадение", "xs muted")
    s += t(64, y + 262, "с другим именем — предупреждение.", "xs muted")
    s += box(304, 176, 776, 732)
    s += box(1104, 176, 448, 732)
    return s


def group_head(x, y, label):
    return t(x, y + 18, label, "label muted")


def field_row(y, included, label, code, control="", note=None, cls=None, disabled=False, error=None):
    """One profile field: include checkbox, label, control text, class chip or note, API name (monospace, right)."""
    X = 328
    if disabled:
        # dependent field excluded by the controlling value: no checkbox at all, the reason is text (P1-06E §7)
        s = icon("ban", X + 1, y + 6, "#98a2b3", 18)
    else:
        s = checkbox(X, y + 5, included)
    s += t(X + 30, y + 20, label, "small", "#98a2b3" if disabled else None)
    if disabled:
        s += t(X + 250, y + 20, note or control, "xs muted")
    elif error:
        s += box(X + 250, y, 90, 30, stroke="#a32935", rx=6, sw=2) + t(X + 260, y + 20, control, "small")
        s += icon("alert-triangle", X + 348, y + 6, "#a32935", 18) + t(X + 372, y + 20, error, "xs", "#a32935", weight="650")
    else:
        s += t(X + 250, y + 20, control, "small", weight="600")
        nx = X + 384
        if cls:
            s += chip(nx, y + 3, chipw(cls), cls, AMBER)
            nx += chipw(cls) + 8
        if note:
            s += t(nx, y + 20, note, "xs muted")
    s += t(1056, y + 20, code, "mono", anchor="end")
    return s


def preview_frame(x, y, w, h, caption):
    s = box(x, y, w, h, fill="#ffffff", stroke="#d0d5dd", rx=8)
    s += t(x + 12, y + 20, caption, "xs", weight="700")
    s += chip(x + w - 96, y + 8, 86, "Синтетика", AMBER)
    return s


def preview_columns(x, y, w, h, colors):
    """Synthetic stacked columns for the svggraph section: three series in bytes, Y axis from 0, legend in two lines."""
    s = preview_frame(x, y, w, h, "Пример виджета «График»")
    px0, py0, py1 = x + 56, y + 40, y + h - 58
    for i, lab in enumerate(["0", "1 ГБ", "2 ГБ"]):
        yy = py1 - i * (py1 - py0) / 2
        s += f'  <line x1="{px0}" y1="{yy:.1f}" x2="{x + w - 14}" y2="{yy:.1f}" stroke="#eaecf0"/>\n'
        s += t(px0 - 8, f"{yy + 4:.1f}", lab, "xs muted", anchor="end")
    s += f'  <line x1="{px0}" y1="{py1}" x2="{x + w - 14}" y2="{py1}" stroke="#7a8d9b"/>\n'
    heights = [(26, 14, 8), (30, 16, 10), (36, 20, 12), (40, 24, 14), (46, 26, 16), (52, 30, 18), (58, 34, 20), (62, 38, 22)]
    gx = px0 + 10
    for hs in heights:
        yy = py1
        for hh, c in zip(hs, colors):
            s += f'  <rect x="{gx}" y="{yy - hh}" width="26" height="{hh}" fill="{c}" fill-opacity=".85"/>\n'
            yy -= hh
        gx += 40
    for i, hh in enumerate(["13:00", "13:30", "14:00"]):
        s += t(f"{px0 + i * (w - 70) / 2:.1f}", py1 + 16, hh, "xs muted", anchor="start" if i == 0 else ("end" if i == 2 else "middle"))
    ly = y + h - 24
    for i, (c, lab) in enumerate(zip(colors, ["Пример 1 · узел 1", "Пример 2 · узел 1", "Пример 3 · узел 1"])):
        lx = x + 14 + (i % 2) * 190
        yy = ly + (i // 2) * 16
        s += f'  <rect x="{lx}" y="{yy - 9}" width="12" height="4" rx="2" fill="{c}"/>\n' + t(lx + 20, yy, lab, "xs")
    return s


def preview_item(x, y, w, h, value_color):
    s = preview_frame(x, y, w, h, "Пример виджета «Значение элемента данных»")
    s += t(x + w / 2, y + 60, "Пример: загрузка CPU", "small muted", anchor="middle")
    s += f'  <text x="{x + w / 2 - 22}" y="{y + 132}" text-anchor="middle" font-size="64" font-weight="800" fill="{value_color}">87,3</text>\n'
    s += f'  <text x="{x + w / 2 + 84}" y="{y + 132}" text-anchor="middle" font-size="35" fill="{value_color}">%</text>\n'
    s += t(x + w / 2, y + h - 18, "порог 80 → цвет значения", "xs muted", anchor="middle")
    return s


def param_card(y, h, name, kind, desc, state_icon, state_color, state_chip=None, chip_dy=12):
    LX = 272
    s = box(LX, y, 592, h, fill="#ffffff", stroke="#d0d5dd", rx=10)
    s += icon(state_icon, LX + 14, y + 12, state_color, 20)
    s += t(LX + 44, y + 25, name, "mono") + t(LX + 44 + 9 * len(name) + 12, y + 25, kind, "small")
    s += t(LX + 44, y + 45, desc, "xs muted")
    if state_chip:
        s += chip(LX + 592 - chipw(state_chip[0]) - 14, y + chip_dy, chipw(state_chip[0]), state_chip[0], state_chip[1])
    return s


def insert_right(name, after_lines, blocker=None, ins_kind="primary"):
    RX = 880
    s = box(RX, 198, 448, 640, fill="#f8fafc", stroke="#e4e7ec", rx=12)
    s += t(RX + 24, 228, "Имя виджета", "xs muted")
    s += box(RX + 24, 236, 400, 38, stroke=BORDER, rx=8) + t(RX + 36, 261, name, "body")
    s += t(RX + 24, 306, "РАЗМЕР", "label muted")
    s += box(RX + 24, 318, 72, 34, stroke=BORDER, rx=8) + t(RX + 36, 341, "24", "body")
    s += t(RX + 104, 341, "×", "body")
    s += box(RX + 120, 318, 72, 34, stroke=BORDER, rx=8) + t(RX + 132, 341, "6", "body")
    s += t(RX + 204, 341, "ячеек · по умолчанию из заготовки", "xs muted")
    s += t(RX + 24, 386, "РАЗМЕСТИТЬ", "label muted")
    s += radio(RX + 34, 410, True) + t(RX + 54, 416, "В первое свободное место: 0, 8", "small")
    s += radio(RX + 34, 442, False) + t(RX + 54, 448, "Указать на холсте", "small")
    s += t(RX + 24, 480, "Существующие виджеты не сдвигаются.", "xs muted")
    s += t(RX + 24, 520, "ПОСЛЕ ВСТАВКИ", "label muted")
    s += wrap(RX + 24, 544, after_lines, "xs", "#344054", step=18)
    s += icon("info-circle", RX + 24, 700, "#667085", 18)
    s += wrap(RX + 50, 714, ["Вставленный виджет от заготовки не зависит;", "узлы и метрики не подбираются автоматически."], "xs muted", step=18)
    if blocker:
        s += box(RX, 776, 448, 62, fill="#fef3f2", stroke="#f2b8b5", rx=10)
        s += icon("circle-x", RX + 14, 790, "#a32935", 18)
        s += wrap(RX + 40, 804, blocker, "small", "#a32935", step=18)
    s += button(808, 880, 128, "Назад") + button(948, 880, 140, "Отмена") + button(1100, 880, 228, "Вставить", ins_kind)
    return s


# ================================================================== 14-06 profile editor: svggraph section (P1-06E)
def profile_editor_svggraph():
    s = head("библиотека: профиль оформления, раздел «График»", "Светлый макет редактора профиля оформления «Рост БД — столбцы», "
             "раздел «График»: поля по разделам инспектора с флажком «в профиле», значения, классы «по выбору» с причиной, "
             "зависимые поля недоступны с причиной, палитра рядов с предупреждением о контрасте, предпросмотр на синтетике, "
             "итог, число использующих виджетов и версия при сохранении; состояние «Не сохранено».", EDIT_ICONS)
    s += profile_editor_shell("Рост БД — столбцы", "Версия 3 → 4 при сохранении · Zabbix 7.4.0+ (вычислено) · Мой · изменён 26.10.2026, 10:12",
                              [("График", "svggraph")], "График", "Рост БД — столбцы",
                              ["Столбцы с накоплением для", "графиков роста размера баз"], "базы, рост")
    s += icon("palette", 328, 194, "#1d3f86", 20) + t(356, 210, "График · svggraph", "h3")
    s += t(1056, 210, "в профиле: 7 полей · палитра 4 цвета", "xs muted", anchor="end")
    s += t(328, 234, "Значения применяются ко всем наборам данных виджета одинаково. Поле без флажка у виджета не меняется.", "xs muted")
    y = 246
    s += group_head(328, y, "ОТОБРАЖЕНИЕ · РЯДЫ"); y += 26
    s += field_row(y, True, "Вид ряда", "ds.N.type", "столбцы"); y += 30
    s += field_row(y, True, "Накопление", "ds.N.stacked", "да"); y += 30
    s += field_row(y, True, "Прозрачность", "ds.N.transparency", "3"); y += 30
    s += field_row(y, False, "Толщина линии", "ds.N.width", "—", note="не применяется при виде ряда «столбцы»", disabled=True); y += 30
    s += field_row(y, False, "Заливка", "ds.N.fill", "—", note="не применяется при виде ряда «столбцы»", disabled=True); y += 30
    s += field_row(y, False, "Обработка пропусков", "ds.N.missingdatafunc", "умолчание Zabbix", cls="по выбору"); y += 30
    s += checkbox(328, y + 5, True) + t(358, y + 20, "Палитра рядов", "small") + swatches(578, y + 5, PALETTE + ["#ffd166"])
    s += icon("plus", 672, y + 6, "#3156a3", 18) + t(694, y + 20, "Добавить цвет", "xs", "#3156a3", weight="650")
    s += t(1056, y + 20, "ds.N.color · ds.N.color.M", "mono", anchor="end"); y += 30
    s += icon("alert-triangle", 358, y + 3, "#8a4b00", 18)
    s += t(382, y + 17, "Цвет 4 (FFD166) — контраст 1,4:1 к белому: ряд будет плохо виден на светлой теме Zabbix. Не блокирует.", "xs", "#8a4b00"); y += 28
    s += group_head(328, y, "ОТОБРАЖЕНИЕ · ЛЕГЕНДА"); y += 26
    s += field_row(y, True, "Показывать легенду", "legend", "да"); y += 30
    s += field_row(y, True, "Строк легенды", "legend_lines", "3"); y += 30
    s += group_head(328, y, "ОСИ И ПОРОГИ"); y += 26
    s += field_row(y, True, "Левая ось", "lefty", "показывать"); y += 30
    s += field_row(y, True, "Минимум левой оси", "lefty_min", "0", cls="по выбору", note="область значений"); y += 30
    s += field_row(y, False, "Максимум левой оси", "lefty_max", "—", cls="по выбору", note="область значений"); y += 30
    s += field_row(y, False, "Единицы левой оси", "lefty_units", "авто", cls="по выбору", note="единицы метрики"); y += 30
    s += group_head(328, y, "ДОПОЛНИТЕЛЬНО"); y += 26
    s += field_row(y, False, "Показывать проблемы", "show_problems", "умолчание Zabbix", note="было в версии 3 → у виджетов «изменено»"); y += 30
    s += group_head(328, y, "ОСНОВНОЕ"); y += 26
    s += field_row(y, False, "Показ заголовка", "view_mode", "—", cls="по выбору", note="атрибут виджета"); y += 34
    s += icon("ban", 328, y - 2, "#667085", 18)
    s += t(354, y + 12, "Никогда не входят в профиль: источники, период, агрегация, подписи, замещения, связи, неизвестные поля, положение.", "xs muted")
    # right: preview and summary
    s += t(1128, 206, "ПРЕДПРОСМОТР · СИНТЕТИКА", "label muted")
    s += select(1128, 218, 400, "Пример: 3 ряда · байты (B) · 1 час", h=34)
    s += preview_columns(1128, 264, 400, 236, PALETTE)
    s += t(1128, 522, "Фон — светлая тема Zabbix. Тема приложения на профиль не влияет;", "xs muted")
    s += t(1128, 540, "синтетика — не данные Zabbix и никуда не сохраняется.", "xs muted")
    s += hline(1128, 1528, 558)
    s += t(1128, 586, "ИТОГ", "label muted")
    s += wrap(1128, 612, ["В профиле: 7 полей и палитра 4 цвета; 1 поле «по выбору»", "(минимум левой оси) применяется с предупреждением;",
                          "7 полей не включены и у виджетов не меняются."], "small", step=20)
    s += box(1128, 682, 400, 60, fill="#fff8ed", stroke="#f3d3a6", rx=10)
    s += icon("alert-triangle", 1142, 696, "#8a4b00", 18)
    s += wrap(1168, 710, ["1 цвет палитры ниже 3:1 к белому —", "сохранить можно."], "small", "#5c3a00", step=18)
    s += box(1128, 756, 400, 94, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
    s += icon("info-circle", 1142, 770, "#1d3f86", 18)
    s += wrap(1168, 784, ["Используется: 12 виджетов в 2 черновиках.", "После сохранения у них появится «обновлён до", "версии 4»; значения не изменятся без", "«Обновить из профиля»."], "small", "#344054", step=18)
    s += t(1128, 886, "Ctrl+S — сохранить · Esc — закрыть (спросит о несохранённых изменениях)", "xs muted")
    write(s + "</svg>\n", "14-profiles/14-06_P1-06E_profile-editor-svggraph.svg", "library", mode=OFFLINE)


# ================================================================== 14-07 profile editor: item section with an invalid value (P1-06E)
def profile_editor_item():
    s = head("библиотека: профиль оформления, раздел «Значение элемента данных»", "Светлый макет редактора профиля оформления "
             "«Значения: крупно», раздел «Значение элемента данных»: поля по разделам инспектора, недопустимое значение поля с "
             "ошибкой рядом, зависимые поля «время» недоступны, пороги «по выбору» с единицами, предпросмотр значения на синтетике "
             "с цветом порога, сохранение недоступно до исправления.", EDIT_ICONS)
    s += profile_editor_shell("Значения: крупно", "Версия 2 → 3 при сохранении · Zabbix 7.4.0+ (вычислено) · Мой · изменён 25.10.2026, 17:40",
                              [("График", "svggraph"), ("Значение элемента данных", "item")], "Значение элемента данных",
                              "Значения: крупно", ["Крупное значение, описание", "сверху, пороги для процентов"], "значения", save_kind="disabled")
    s += icon("palette", 328, 194, "#1d3f86", 20) + t(356, 210, "Значение элемента данных · item", "h3")
    s += t(1056, 210, "в профиле: 8 полей · пороги 2 · 1 ошибка", "xs", "#a32935", anchor="end", weight="650")
    s += t(328, 234, "Профиль с пустым «Что показывать» недопустим: это обязательное поле Zabbix.", "xs muted")
    y = 246
    s += group_head(328, y, "ОТОБРАЖЕНИЕ · ЧТО ПОКАЗЫВАТЬ"); y += 26
    s += field_row(y, True, "Что показывать", "show.N", "описание, значение"); y += 30
    s += group_head(328, y, "ОТОБРАЖЕНИЕ · ЗНАЧЕНИЕ"); y += 26
    s += field_row(y, True, "Размер значения, %", "value_size", "145", error="Допустимо 1–100"); y += 30
    s += field_row(y, True, "Жирное значение", "value_bold", "да"); y += 30
    s += field_row(y, False, "Цвет значения", "value_color", "умолчание Zabbix"); y += 30
    s += group_head(328, y, "ОТОБРАЖЕНИЕ · ОПИСАНИЕ И ЕДИНИЦЫ"); y += 26
    s += field_row(y, True, "Размер описания, %", "desc_size", "15"); y += 30
    s += field_row(y, True, "Положение описания", "desc_v_pos, desc_h_pos", "сверху · по центру"); y += 30
    s += field_row(y, True, "Показывать единицы", "units_show", "да"); y += 30
    s += field_row(y, True, "Размер единиц, %", "units_size", "35"); y += 30
    s += field_row(y, False, "Текст единиц", "units", "—", cls="по выбору", note="заменяет единицы элемента"); y += 30
    s += group_head(328, y, "ОТОБРАЖЕНИЕ · ВРЕМЯ И ИНДИКАТОР"); y += 26
    s += field_row(y, False, "Размер времени, %", "time_size", "—", note="не применяется: «время» не входит в «Что показывать»", disabled=True); y += 30
    s += field_row(y, False, "Цвета индикатора изменения", "up_color, down_color", "—", note="не применяется: «индикатор» не в «Что показывать»", disabled=True); y += 30
    s += group_head(328, y, "ОСИ И ПОРОГИ"); y += 26
    s += checkbox(328, y + 5, True) + t(358, y + 20, "Пороги · 2", "small") + chip(578, y + 3, chipw("по выбору"), "по выбору", AMBER)
    s += t(678, y + 20, "для «%»; иные единицы — предупреждение", "xs muted")
    s += t(1056, y + 20, "thresholds.N.*", "mono", anchor="end"); y += 30
    for thr, col in [("80", "#e8a33d"), ("95", "#a32935")]:
        s += box(358, y + 2, 72, 26, stroke=BORDER, rx=6) + t(368, y + 20, thr, "small")
        s += t(440, y + 20, "→", "small") + swatches(462, y + 7, [col], size=16) + t(486, y + 20, col[1:].upper(), "mono")
        s += icon("x", 560, y + 6, "#667085", 18, "Убрать порог")
        y += 30
    s += icon("plus", 358, y + 2, "#3156a3", 18) + t(382, y + 16, "Добавить порог", "xs", "#3156a3", weight="650"); y += 30
    s += icon("ban", 328, y + 2, "#667085", 18)
    s += t(354, y + 16, "Никогда: элемент данных, текст описания, агрегация, период, данные спарклайна, положение.", "xs muted")
    # right
    s += t(1128, 206, "ПРЕДПРОСМОТР · СИНТЕТИКА", "label muted")
    s += select(1128, 218, 400, "Пример: число с плавающей точкой · %", h=34)
    s += preview_item(1128, 264, 400, 200, "#8a4b00")
    s += t(1128, 486, "Размер значения показан как 100 % (максимум), пока", "xs muted")
    s += t(1128, 504, "в поле недопустимое значение.", "xs muted")
    s += hline(1128, 1528, 524)
    s += t(1128, 552, "ИТОГ", "label muted")
    s += wrap(1128, 578, ["В профиле: 8 полей, пороги 2 (по выбору); 2 поля", "не включены; 2 недоступны при текущем «Что показывать»."], "small", step=20)
    s += box(1128, 630, 400, 76, fill="#fef3f2", stroke="#f2b8b5", rx=10)
    s += icon("circle-x", 1142, 644, "#a32935", 18)
    s += wrap(1168, 658, ["Сохранить нельзя: 1 поле с недопустимым", "значением — «Размер значения». Профиль", "в библиотеке не изменён."], "small", "#a32935", step=18)
    s += box(1128, 722, 400, 76, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
    s += icon("info-circle", 1142, 736, "#1d3f86", 18)
    s += wrap(1168, 750, ["Используется: 9 виджетов в 2 черновиках.", "Пороги применяются только к значениям", "с единицами «%»; иначе — предупреждение."], "small", "#344054", step=18)
    s += t(1128, 886, "Ctrl+S — сохранить · Esc — закрыть (спросит о несохранённых изменениях)", "xs muted")
    write(s + "</svg>\n", "14-profiles/14-07_P1-06E_profile-editor-item.svg", "library", mode=OFFLINE)


# ================================================================== 14-08 insert blueprint: parameters in a local draft (P1-06F)
def insert_blueprint_local():
    s = head("редактор: вставка заготовки, параметры", "Светлый макет шага «Параметры» при вставке заготовки «Загрузка и нагрузка CPU» "
             "в локальную панель: параметр-роль выбирается из ролей черновика или создаётся новая, параметры-метрики выбираются "
             "из каталогов черновика по типу значения и единицам со статусами «Точно», ничего не подбирается автоматически, "
             "«Оставить «нужна привязка»» допустимо; имя, размер, размещение; вставка недоступна, пока параметр без решения.", INSERT_ICONS)
    s += editor_base()
    s += '  <!-- Dialog -->\n  <rect x="0" y="72" width="1600" height="928" fill="#17213a" fill-opacity=".45"/>\n'
    s += box(240, 100, 1120, 840, stroke="#c9d3ea")
    s += icon("template", 272, 128, "#1d3f86", 24) + t(306, 146, "Добавить виджет на страницу «ClickHouse»", "title")
    s += icon("x", 1320, 126, "#475467", 22, "Закрыть")
    s += t(272, 176, "Из заготовок · «Загрузка и нагрузка CPU» · График · шаг 2 из 2: параметры. Виджет появится после «Вставить».", "small muted")
    LX = 272
    s += t(LX, 212, "ПАРАМЕТРЫ · 3", "label muted") + t(LX + 140, 212, "источники не подбираются автоматически", "xs muted")
    y = 224
    s += param_card(y, 100, "cpu_hosts", "роль узлов сети · несколько узлов", "«Узлы для графика CPU»", "circle-check", "#1f6b43")
    s += select(LX + 44, y + 56, 330, "Роль «clickhouse» · несколько узлов", h=34)
    s += icon("plus", LX + 392, y + 64, "#3156a3", 18) + t(LX + 414, y + 78, "Создать роль…", "small", "#3156a3", weight="650")
    y += 108
    s += param_card(y, 164, "cpu_util", "метрика · с плавающей точкой · %", "«Загрузка CPU» · подходящих метрик: 2 · каталог «Linux by Zabbix agent»", "circle-check", "#1f6b43")
    s += radio(LX + 54, y + 74, True) + t(LX + 74, y + 79, "CPU utilization", "small", weight="650") + t(LX + 200, y + 79, "system.cpu.util · %", "mono")
    s += chip(LX + 520, y + 62, 58, "Точно", GREEN)
    s += radio(LX + 54, y + 104, False) + t(LX + 74, y + 109, "CPU iowait time", "small", weight="650") + t(LX + 200, y + 109, "system.cpu.util[,iowait] · %", "mono")
    s += chip(LX + 520, y + 92, 58, "Точно", GREEN)
    s += radio(LX + 54, y + 134, False) + t(LX + 74, y + 139, "Оставить «нужна привязка»", "small")
    s += t(LX + 300, y + 139, "метрику можно выбрать позже в инспекторе", "xs muted")
    y += 172
    s += param_card(y, 164, "load_avg", "метрика · с плавающей точкой · без единиц", "«Средняя нагрузка» · подходящих метрик: 3", "alert-triangle", "#8a4b00",
                    ("Решение не выбрано", AMBER), chip_dy=32)
    s += radio(LX + 54, y + 74, False) + t(LX + 74, y + 79, "Load average (1m avg)", "small", weight="650") + t(LX + 240, y + 79, "system.cpu.load[all,avg1] · —", "mono")
    s += chip(LX + 520, y + 62, 58, "Точно", GREEN)
    s += radio(LX + 54, y + 104, False) + t(LX + 74, y + 109, "Load average (5m avg)", "small", weight="650") + t(LX + 240, y + 109, "system.cpu.load[all,avg5] · —", "mono")
    s += chip(LX + 520, y + 92, 58, "Точно", GREEN)
    s += radio(LX + 54, y + 134, False) + t(LX + 74, y + 139, "Оставить «нужна привязка»", "small")
    s += t(LX + 300, y + 139, "ещё 1 — с другими единицами · Показать все", "xs muted")
    y += 180
    s += icon("info-circle", LX, y - 2, "#667085", 18)
    s += wrap(LX + 26, y + 12, ["«Точно» — тип значения и единицы совпадают с параметром; «С отличиями» — единицы отличаются.",
                                "Похожее имя — не доказательство: проверьте ключ и единицы. Метрики — только из каталогов",
                                "этого черновика, роли — только из этого черновика."], "xs muted", step=18)
    s += insert_right("Загрузка и нагрузка CPU",
                      ["reference выдаст ZDashstitch; один шаг истории", "«вставка заготовки»: Ctrl+Z отменит и виджет,", "и созданную роль.",
                       "«Нужна привязка» — находка «Проверок»: сохранить", "и экспортировать можно, публикация — после привязки.", "Откроется инспектор вставленного виджета."],
                      blocker=["Вставить нельзя: 1 параметр без решения", "(load_avg). Страница не изменена."], ins_kind="disabled")
    write_editor(s + "</svg>\n", "14-08_P1-06F_insert-blueprint-local.svg")


# ================================================================== 14-09 insert blueprint: parameters in a server copy (P1-06F)
def insert_blueprint_copy():
    s = head("редактор: вставка заготовки в копию с сервера", "Светлый макет шага «Параметры» при вставке заготовки «Загрузка и нагрузка CPU» "
             "в копию панели с сервера: ролей нет, параметр-роль получает реальные узлы сети подключения через выбор узлов, "
             "для параметра-метрики сначала выбирается метрика каталога из библиотеки, затем она разрешается в элемент данных "
             "на каждом узле со статусами; узел без элемента остаётся «нужна привязка» по явному решению; ID сервера не "
             "показываются; вставка доступна.", INSERT_ICONS)
    s += editor_base()
    s += '  <!-- Dialog -->\n  <rect x="0" y="72" width="1600" height="928" fill="#17213a" fill-opacity=".45"/>\n'
    s += box(240, 100, 1120, 840, stroke="#c9d3ea")
    s += icon("template", 272, 128, "#1d3f86", 24) + t(306, 146, "Добавить виджет на страницу «ClickHouse»", "title")
    s += icon("x", 1320, 126, "#475467", 22, "Закрыть")
    s += t(272, 176, "Из заготовок · «Загрузка и нагрузка CPU» · График · шаг 2 из 2: параметры. Копия с сервера «Базы данных: Prod».", "small muted")
    LX = 272
    s += box(LX, 196, 592, 74, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
    s += icon("info-circle", LX + 14, 210, "#1d3f86", 18)
    s += wrap(LX + 40, 224, ["Копия с сервера: ролей нет. Узлы сети и элементы данных берутся из подключения",
                             "«Лаборатория 7.4» (только чтение), метрики — из каталогов библиотеки; ID сервера",
                             "не показываются. Без подключения параметры остались бы «нужна привязка»."], "xs", "#344054", step=18)
    s += t(LX, 300, "ПАРАМЕТРЫ · 3", "label muted") + t(LX + 140, 300, "узлы, метрики и элементы не подбираются автоматически", "xs muted")
    y = 312
    s += param_card(y, 100, "cpu_hosts", "роль узлов сети · несколько узлов", "«Узлы для графика CPU» → узлы сети подключения", "circle-check", "#1f6b43")
    s += box(LX + 44, y + 56, 330, 34, fill="#f8fafc", stroke=BORDER, rx=8) + t(LX + 56, y + 78, "db-lab-01, db-lab-02 · 2 узла", "small")
    s += button(LX + 392, y + 52, 186, "Выбрать узлы…", h=38)
    y += 108
    s += param_card(y, 128, "cpu_util", "метрика · с плавающей точкой · %", "«Загрузка CPU» → метрика каталога → элемент данных на каждом узле", "circle-check", "#1f6b43",
                    ("2 из 2 узлов", GREEN))
    s += t(LX + 44, y + 79, "Метрика каталога:", "xs muted") + t(LX + 150, y + 79, "CPU utilization", "small", weight="650")
    s += t(LX + 276, y + 79, "system.cpu.util · %", "mono") + t(LX + 440, y + 79, "Linux by Zabbix agent", "xs muted")
    s += t(LX + 44, y + 105, "db-lab-01 — Точно · db-lab-02 — Точно", "xs")
    s += t(LX + 300, y + 105, "Другая метрика каталога…", "xs", "#3156a3", weight="650") + t(LX + 470, y + 105, "Другой элемент…", "xs", "#3156a3", weight="650")
    y += 136
    s += param_card(y, 196, "load_avg", "метрика · с плавающей точкой · без единиц", "«Средняя нагрузка» → метрика каталога → элемент данных на каждом узле", "alert-triangle", "#8a4b00",
                    ("1 из 2 узлов", AMBER))
    s += t(LX + 44, y + 79, "Метрика каталога:", "xs muted") + t(LX + 150, y + 79, "Load average (1m avg)", "small", weight="650")
    s += t(LX + 316, y + 79, "system.cpu.load[all,avg1] · —", "mono")
    s += t(LX + 44, y + 105, "db-lab-01 — Точно · db-lab-02 — Не найдено", "xs", "#8a4b00", weight="650")
    s += t(LX + 380, y + 105, "Другая метрика каталога…", "xs", "#3156a3", weight="650")
    s += radio(LX + 54, y + 134, True) + t(LX + 74, y + 139, "Оставить «нужна привязка» для db-lab-02", "small")
    s += t(LX + 380, y + 139, "публикация — после привязки", "xs muted")
    s += radio(LX + 54, y + 164, False) + t(LX + 74, y + 169, "Выбрать другой элемент данных на db-lab-02…", "small")
    s += t(LX + 420, y + 169, "таблица соответствий", "xs muted")
    y += 212
    s += icon("info-circle", LX, y - 2, "#667085", 18)
    s += wrap(LX + 26, y + 12, ["Порядок узлов = порядок рядов. Заготовка ключей не хранит: метрика выбирается из каталога",
                                "библиотеки, элемент ищется по её ключу, затем сверяются тип значения и единицы; «Не найдено»",
                                "и «Неоднозначно» требуют решения — другой узел, метрика или элемент не подставляется."], "xs muted", step=18)
    s += insert_right("Загрузка и нагрузка CPU",
                      ["операция копии «добавить виджет» — виден", "в «Мои изменения» публикации; при публикации", "создаётся заново, ID сервера до записи нет.",
                       "«Нужна привязка» у db-lab-02 — находка;", "публикация — после решения в «Привязках».", "Ctrl+Z отменяет вставку одним шагом."])
    write_editor(s + "</svg>\n", "14-09_P1-06F_insert-blueprint-copy.svg", crumb=CRUMB_COPY, chips=(DIRTY, COPY), mode=LAB_RO)


# ================================================================== P1-06G helpers
APPLY_ICONS = EDITOR_ICONS + ["refresh"]
ORIGIN_PROFILE = ("из профиля", BLUE)
ORIGIN_OVERRIDDEN = ("изменено после профиля", AMBER)
ORIGIN_CHANGED = ("изменено", AMBER)
ORIGIN_DEFAULT = ("умолчание Zabbix", NEUTRAL)


def origin_chip(x, y, origin):
    text, tone = origin
    return chip(x, y, chipw(text), text, tone)


def dialog_frame(title_text, subtitle, icon_name="palette"):
    s = editor_base()
    s += '  <!-- Dialog -->\n  <rect x="0" y="72" width="1600" height="928" fill="#17213a" fill-opacity=".45"/>\n'
    s += box(240, 100, 1120, 840, stroke="#c9d3ea")
    s += icon(icon_name, 272, 128, "#1d3f86", 24) + t(306, 146, title_text, "title")
    s += icon("x", 1320, 126, "#475467", 22, "Закрыть")
    s += t(272, 176, subtitle, "small muted")
    return s


def radio_row(x, y, on, label, hint=None, hint_x=None, bold=True):
    s = radio(x, y, on) + t(x + 20, y + 6, label, "small", weight="650" if bold else None)
    if hint:
        s += t(hint_x or (x + 320), y + 6, hint, "xs muted")
    return s


def apply_right(summary_lines, boxes, notes, buttons):
    """Right column of the apply/update dialogs: summary, warning/info boxes, notes and the action buttons."""
    RX = 880
    s = box(RX, 198, 448, 640, fill="#f8fafc", stroke="#e4e7ec", rx=12)
    s += t(RX + 24, 228, "ИТОГ", "label muted")
    s += wrap(RX + 24, 254, summary_lines, "small", step=20)
    y = 254 + len(summary_lines) * 20 + 10
    for kind, lines in boxes:
        h = 24 + len(lines) * 18
        if kind == "warn":
            s += box(RX + 24, y, 400, h, fill="#fff8ed", stroke="#f3d3a6", rx=10)
            s += icon("alert-triangle", RX + 38, y + 14, "#8a4b00", 18)
            s += wrap(RX + 64, y + 28, lines, "small", "#5c3a00", step=18)
        else:
            s += box(RX + 24, y, 400, h, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
            s += icon("info-circle", RX + 38, y + 14, "#1d3f86", 18)
            s += wrap(RX + 64, y + 28, lines, "small", "#344054", step=18)
        y += h + 10
    y += 6
    for lines in notes:
        s += wrap(RX + 24, y + 12, lines, "xs muted", step=18)
        y += len(lines) * 18 + 10
    return s + buttons


# ================================================================== 14-10 apply profile to a page (P1-06G)
def apply_profile_bulk():
    s = head("редактор: применить профиль", "Светлый макет окна «Применить профиль» поверх редактора: выбор профиля, область "
             "(виджет, страница, вся панель), режим для уже изменённых полей, таблица виджетов страницы с уровнем совместимости "
             "и числом применимых полей, раскрытое «было → станет» одного виджета с происхождением значений, итог, "
             "предупреждения о палитре и поле «по выбору», один шаг отмены; до «Применить» черновик не меняется.", APPLY_ICONS)
    s += dialog_frame("Применить профиль", "Значения копируются в виджеты; черновик от профиля не зависит. До «Применить» ничего не меняется.")
    LX = 272
    s += t(LX, 206, "ПРОФИЛЬ", "label muted")
    s += select(LX, 216, 592, "«Рост БД — столбцы» · версия 4 · График", h=36)
    s += t(LX, 280, "ПРИМЕНИТЬ К", "label muted")
    s += radio_row(LX + 10, 294, False, "виджету «Queries per second»")
    s += radio_row(LX + 10, 318, True, "всем совместимым виджетам страницы «ClickHouse»")
    s += radio_row(LX + 10, 342, False, "всем совместимым виджетам панели (3 страницы)")
    s += t(LX, 384, "ПОЛЯ, УЖЕ ИЗМЕНЁННЫЕ В ВИДЖЕТАХ", "label muted")
    s += radio_row(LX + 10, 398, True, "Заменить все настройки", "в том числе ваши правки", LX + 300)
    s += radio_row(LX + 10, 422, False, "Сохранить мои правки", "изменённые вами поля остаются", LX + 300)
    s += radio_row(LX + 10, 446, False, "Добавить только новые настройки", "только поля, которых не было в прежней версии", LX + 300)
    # widgets table: name + level + fields; second line — type and reason
    s += table_head(LX, 476, 592, [(LX + 10, "ВИДЖЕТ"), (LX + 330, "СОВМЕСТИМОСТЬ"), (LX + 486, "ПОЛЯ")])
    rows = [
        ("Queries per second", "График", ("Можно применить", GREEN), "7 из 7", None, True),
        ("Connections", "График", ("Частично", AMBER), "1 из 7", "старый вид файла: применится только цвет рядов", False),
        ("Replication lag", "Значение элемента данных", ("Нельзя применить", NEUTRAL), "пропущен", "в профиле нет настроек для этого типа", False),
        ("Disk usage", "Радиальный датчик", ("Нельзя применить", NEUTRAL), "пропущен", "ZDashstitch не изменяет поля виджетов этого типа", False),
    ]
    y = 512
    for name, typ, (lvl, tone), fields, why, sel in rows:
        if sel:
            s += box(LX, y, 592, 40, fill="#eef4ff", stroke="#3156a3", rx=8, sw=2)
            s += icon("chevron-down", LX + 568, y + 11, "#1d3f86", 18)
        else:
            s += row_line(LX, y + 40, 592)
        s += t(LX + 10, y + 18, name, "small", weight="650") + t(LX + 10 + 8 * len(name) + 10, y + 18, "· " + typ, "xs muted")
        if why:
            s += t(LX + 10, y + 34, why, "xs muted")
        s += chip(LX + 330, y + 7, chipw(lvl), lvl, tone)
        s += t(LX + 486, y + 25, fields, "small", "#667085" if fields == "пропущен" else None, weight=None if fields == "пропущен" else "600")
        y += 40
    y += 8
    # before → after
    s += t(LX, y + 14, "БЫЛО → СТАНЕТ: «QUERIES PER SECOND»", "label muted")
    y += 22
    s += table_head(LX, y, 592, [(LX + 10, "ПОЛЕ"), (LX + 170, "БЫЛО"), (LX + 380, "СТАНЕТ"), (LX + 470, "ИТОГ")])
    y += 36
    diff = [
        ("Вид ряда", "линия", ORIGIN_CHANGED, "столбцы", "изменится"),
        ("Накопление", "—", ORIGIN_DEFAULT, "да", "изменится"),
        ("Прозрачность", "5", ORIGIN_CHANGED, "3", "изменится"),
        ("Строк легенды", "2", ORIGIN_CHANGED, "3", "изменится"),
        ("Левая ось", "да", ORIGIN_CHANGED, "да", "уже совпадает"),
        ("Минимум левой оси", "—", ORIGIN_DEFAULT, "0", "изменится · по выбору"),
        ("Палитра рядов", "2 цвета", ORIGIN_CHANGED, "4 цвета", "изменится"),
    ]
    for field, before, origin, after, outcome in diff:
        s += row_line(LX, y + 26, 592)
        s += t(LX + 10, y + 18, field, "small")
        s += t(LX + 170, y + 18, before, "small", weight="600")
        s += origin_chip(LX + 230, y + 1, origin) if origin is not ORIGIN_DEFAULT else t(LX + 232, y + 18, "умолчание Zabbix", "xs muted")
        s += t(LX + 380, y + 18, after, "small", weight="600")
        s += t(LX + 470, y + 18, outcome, "xs", "#667085" if outcome.startswith("уже") else "#1f6b43", weight="650")
        y += 26
    buttons = button(956, 880, 140, "Отмена") + button(1108, 880, 220, "Применить к 2 виджетам", "primary")
    s += apply_right(["Применится к 1 виджету, частично к 1,", "пропущено: 2.", "Изменится полей: 7 в 2 виджетах; уже совпадают: 1."],
                     [("warn", ["В палитре 4 цвета; рядов больше у 1 виджета", "(Connections): цвета повторяются."]),
                      ("warn", ["«Минимум левой оси» — поле «по выбору»:", "применяется с предупреждением."]),
                      ("info", ["Один шаг отмены: Ctrl+Z вернёт все виджеты", "сразу. Источники, период, фильтры", "и расположение не меняются."])],
                     [["Значения применяются ко всем наборам данных", "виджета одинаково."],
                      ["Это окно и есть подтверждение. При выключенной", "настройке «Спрашивать перед массовыми действиями»",
                       "действия с известным профилем и областью", "применяются сразу, с «Отменить» в результате."],
                      ["Метки «из профиля» — локальные сведения черновика;", "в файл и пакет они не попадают."]],
                     buttons)
    write_editor(s + "</svg>\n", "14-10_P1-06G_apply-profile-bulk.svg")


# ================================================================== 14-11 update from profile (P1-06G)
def update_from_profile():
    s = head("редактор: обновить из профиля", "Светлый макет окна «Обновить из профиля» поверх редактора: виджеты на версии 3, "
             "в библиотеке версия 4; область обновления, фильтр «только на прежней версии», режимы «Сохранить мои правки» "
             "(по умолчанию), «Заменить все настройки», «Добавить только новые настройки»; таблица «было → станет» с происхождением "
             "каждого значения, поле, убранное из профиля, итог; один шаг отмены.", APPLY_ICONS)
    s += dialog_frame("Обновить из профиля «Рост БД — столбцы»",
                      "Виджеты на версии 3; в библиотеке — версия 4. На прежней версии: 4 из 12 виджетов из этого профиля.", "refresh")
    LX = 272
    s += t(LX, 212, "ОБНОВИТЬ", "label muted")
    s += radio_row(LX + 10, 226, True, "виджет «Queries per second»")
    s += radio_row(LX + 10, 252, False, "все виджеты из этого профиля на странице «ClickHouse» (2)")
    s += radio_row(LX + 10, 278, False, "все виджеты из этого профиля в панели (4)")
    s += checkbox(LX + 6, 304, True) + t(LX + 36, 319, "Только виджеты на прежней версии профиля", "small")
    s += t(LX, 362, "ПОЛЯ, УЖЕ ИЗМЕНЁННЫЕ В ВИДЖЕТАХ", "label muted")
    s += radio_row(LX + 10, 376, True, "Сохранить мои правки", "по умолчанию · изменённые вами поля остаются", LX + 300)
    s += radio_row(LX + 10, 402, False, "Заменить все настройки", "в том числе ваши правки", LX + 300)
    s += radio_row(LX + 10, 428, False, "Добавить только новые настройки", "только поля, которых не было в версии 3", LX + 300)
    y = 462
    s += t(LX, y + 14, "БЫЛО → СТАНЕТ: «QUERIES PER SECOND»", "label muted")
    y += 22
    s += table_head(LX, y, 592, [(LX + 10, "ПОЛЕ"), (LX + 170, "БЫЛО"), (LX + 330, "СТАНЕТ · ИТОГ")])
    y += 36
    diff = [
        ("Вид ряда", "столбцы", ORIGIN_PROFILE, "столбцы", "уже совпадает", "#667085"),
        ("Накопление", "да", ORIGIN_PROFILE, "да", "уже совпадает", "#667085"),
        ("Прозрачность", "5", ORIGIN_OVERRIDDEN, "3", "останется: ваша правка", "#8a4b00"),
        ("Строк легенды", "2", ORIGIN_PROFILE, "3", "изменится", "#1f6b43"),
        ("Минимум левой оси", "0", ORIGIN_PROFILE, "0", "уже совпадает", "#667085"),
        ("Показывать проблемы", "да", ORIGIN_PROFILE, "—", "убрано из профиля → останется, «изменено»", "#8a4b00"),
        ("Палитра рядов", "3 цвета", ORIGIN_PROFILE, "4 цвета", "изменится", "#1f6b43"),
    ]
    for field, before, origin, after, outcome, color in diff:
        s += row_line(LX, y + 44, 592)
        s += t(LX + 10, y + 20, field, "small")
        s += t(LX + 170, y + 20, before, "small", weight="600")
        s += origin_chip(LX + 10, y + 26, origin)
        s += t(LX + 330, y + 20, after, "small", weight="600")
        s += t(LX + 330, y + 38, outcome, "xs", color, weight="650")
        y += 44
    s += t(LX, y + 20, "Новых полей в версии 4 нет: режим «Добавить только новые настройки» здесь ничего не изменил бы.", "xs muted")
    s += t(LX, y + 38, "«Вернуть значение профиля» у поля берёт значение из записи применения (версия 3), а не из текущей версии.", "xs muted")
    buttons = button(956, 880, 140, "Отмена") + button(1108, 880, 220, "Обновить", "primary")
    s += apply_right(["Изменится полей: 2; останется ваших", "правок: 1; убрано из профиля: 1;", "уже совпадают: 4."],
                     [("warn", ["Поля, убранные из профиля, сохраняют", "значения и становятся «изменено»:", "показывать проблемы."]),
                      ("info", ["После обновления запись виджета — версия 4;", "поля «изменено после профиля» сохраняют", "свою метку и значение."]),
                      ("info", ["Один шаг отмены: Ctrl+Z вернёт виджет", "и запись применения."])],
                     [["Профиль с недопустимыми значениями или для другого", "адаптера обновлять нельзя — сначала исправьте его",
                       "в библиотеке. Удалённый профиль переводит метки", "в «изменено»."]],
                     buttons)
    write_editor(s + "</svg>\n", "14-11_P1-06G_update-from-profile.svg")


# ================================================================== 14-12 inspector: field origin states (P1-06G)
def inspector_origin():
    s = head("редактор: инспектор, значения из профиля", "Светлый макет инспектора виджета «График», раздел «Отображение»: "
             "баннер «профиль обновлён до версии 4» с «Обновить из профиля…» над разделами, состояния полей «умолчание "
             "Zabbix», «изменено», «из профиля», «изменено после профиля» с действиями «Вернуть умолчание», «Вернуть значение "
             "профиля», «Отвязать от профиля»; сводка «из профиля» и действия виджета — в разделе «Основное» и в меню «Ещё».", APPLY_ICONS)
    s += outline(CLICKHOUSE, "Queries per second")
    s += editor_title("Параметры «Queries per second»")
    s += page_tabs("ClickHouse")
    s += box(292, 186, 520, 704, rx=12)
    s += t(312, 214, "СТРАНИЦА «ClickHouse»", "label muted")
    s += canvas(CLICKHOUSE, 316, 246, 6.6, 18.5, 18, selected="Queries per second", ruler=False, compact=True)
    s += t(312, 600, "Изменения инспектора видны на холсте сразу.", "xs muted")
    s += t(312, 640, "ЕЩЁ (МЕНЮ ВИДЖЕТА)", "label muted")
    for i, item in enumerate(["Применить профиль…", "Обновить из профиля…", "Вернуть все значения профиля", "Отвязать виджет от профиля",
                              "Сохранить оформление как профиль…", "Создать заготовку…"]):
        s += t(312, 664 + i * 24, item, "small", "#1d3f86", weight="650")
    s += t(312, 824, "Пункты профилей видны у типов с изменением полей; «Обновить»,", "xs muted")
    s += t(312, 842, "«Вернуть», «Отвязать» — только при записи применения, без подтверждения.", "xs muted")
    # inspector
    s += box(832, 140, 748, 750, rx=12)
    s += t(856, 172, "Queries per second", "h3")
    s += chip(856, 184, chipw("График · svggraph"), "График · svggraph", BLUE) + chip(1032, 184, chipw("Изменение поддерживается"), "Изменение поддерживается", GREEN)
    s += button(1424, 150, 132, "Готово", h=34)
    secs = [("Основное", False), ("Данные", False), ("Отображение", True), ("Оси и пороги", False), ("Период", False),
            ("Дополнительно", False), ("Неизвестные поля · 2", False)]
    y = 236
    for name, on in secs:
        if on:
            s += box(856, y, 176, 36, fill="#eef4ff", stroke="#eef4ff", rx=8)
        s += t(870, y + 23, name, "small", "#1d3f86" if on else None, weight="700" if on else None)
        y += 42
    s += t(856, y + 14, "Разделы — порядок табуляции.", "xs muted")
    X = 1052
    # outdated banner
    s += box(X, 236, 504, 96, fill="#fff8ed", stroke="#f3d3a6", rx=10)
    s += icon("alert-triangle", X + 14, 250, "#8a4b00", 18)
    s += t(X + 40, 262, "Профиль «Рост БД — столбцы» обновлён до версии 4.", "small", "#5c3a00", weight="650")
    s += t(X + 40, 280, "У виджета — версия 3. Значения не изменятся без обновления. Виден в любом разделе.", "xs", "#5c3a00")
    s += button(X + 40, 290, 240, "Обновить из профиля…", h=32, icon_name="refresh")
    rows = [
        ("ВИД РЯДОВ · ВСЕ НАБОРЫ", None, None),
        ("Вид ряда: столбцы", ORIGIN_PROFILE, ["Вернуть умолчание", "Отвязать от профиля"]),
        ("Накопление: да", ORIGIN_PROFILE, ["Вернуть умолчание", "Отвязать от профиля"]),
        ("Прозрачность: 5", ORIGIN_OVERRIDDEN, ["Вернуть значение профиля (3)", "Вернуть умолчание"]),
        ("Толщина линии: —", ORIGIN_DEFAULT, []),
        ("ЛЕГЕНДА", None, None),
        ("Показывать легенду: да", ORIGIN_PROFILE, ["Вернуть умолчание", "Отвязать от профиля"]),
        ("Строк легенды: 2", ORIGIN_PROFILE, ["Вернуть умолчание", "Отвязать от профиля"]),
        ("Колонок легенды: 3", ORIGIN_CHANGED, ["Вернуть умолчание"]),
    ]
    y = 346
    for label, origin, acts in rows:
        if origin is None:
            s += t(X, y + 14, label, "label muted")
            y += 22
            continue
        s += t(X, y + 18, label, "small")
        s += origin_chip(X + 180, y + 1, origin)
        if len(acts) > 1:
            # two actions do not fit after the chip: they go under the row
            ax = X + 180
            for a in acts:
                s += t(ax, y + 40, a, "xs", "#3156a3", weight="650")
                ax += round(6.6 * len(a)) + 14
            y += 48
        else:
            ax = X + 180 + chipw(origin[0]) + 12
            for a in acts:
                s += t(ax, y + 18, a, "xs", "#3156a3", weight="650")
            y += 28
    s += t(X, y + 14, "ПАЛИТРА РЯДОВ", "label muted")
    y += 22
    s += swatches(X, y + 2, PALETTE) + origin_chip(X + 180, y + 1, ORIGIN_PROFILE)
    s += t(X + 180, y + 40, "Вернуть умолчание", "xs", "#3156a3", weight="650")
    s += t(X + 180 + round(6.6 * 17) + 14, y + 40, "Отвязать от профиля", "xs", "#3156a3", weight="650")
    y += 56
    s += hline(X, X + 504, y)
    s += wrap(X, y + 22, ["Подсказка у плашки: «из профиля «Рост БД — столбцы», версия 3»; у «изменено",
                          "после профиля» — «в профиле: 3». «Вернуть…» — обычные правки поля (шаг истории);",
                          "«Отвязать» снимает метку без подтверждения, значение остаётся (шаг истории).",
                          "Сводка «Из профиля…: полей 8, изменено после — 1», «Вернуть все значения профиля»",
                          "и «Отвязать виджет от профиля» — в разделе «Основное» и в меню «Ещё»."], "xs muted", step=18)
    write_editor(s + "</svg>\n", "14-12_P1-06G_inspector-origin.svg")



if __name__ == "__main__":
    library_profiles()
    library_blueprints()
    import_conflict()
    extract_profile()
    create_blueprint()
    profile_editor_svggraph()
    profile_editor_item()
    insert_blueprint_local()
    insert_blueprint_copy()
    apply_profile_bulk()
    update_from_profile()
    inspector_origin()
