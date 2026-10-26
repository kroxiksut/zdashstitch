"""Review mockups 12-01..12-04: page editor on the 72-column canvas (P1-04A-E).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/editor_svg.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402
from publication_svg import (  # noqa: E402
    AMBER, BLUE, GREEN, NEUTRAL, RED, box, button, checkbox, chip, head, icon, radio, select, t,
)

OUT = "docs/design/assets/12-editor/"
OFFLINE = "offline"
DIRTY = ("Изменена", "#fff4e5", "#8a4b00", 100)
LOCAL = ("Локальная", "#ecfdf3", "#1f6b43", 110)
CRUMB = "Черновики  /  Базы данных: сводная  /  Структура  /  ClickHouse"
BORDER = "#7a8d9b"  # control border, >= 3:1 (P1-11F)
ICONS = ["plus", "arrow-back-up", "arrow-forward-up", "zoom-in", "zoom-out", "check", "chevron-down",
         "chevron-right", "search", "x", "ban", "info-circle", "alert-triangle", "circle-check", "copy",
         "grip-vertical", "pencil"]

# ClickHouse page of «Базы данных: сводная», same as 08-01
CLICKHOUSE = [
    ("Connections", "График", 0, 0, 36, 5, "edit"),
    ("Queries per second", "График", 36, 0, 36, 5, "edit"),
    ("Replication lag", "Значение элемента данных", 0, 5, 24, 3, "edit"),
    ("Disk usage", "Радиальный датчик", 24, 5, 24, 3, "kept"),
    ("Top queries", "Топ узлов сети", 48, 5, 24, 3, "kept"),
]
KEEPER = [
    ("Keeper sessions", "График", 0, 0, 24, 4, "edit"),
    ("Keeper latency", "График", 24, 0, 24, 4, "edit"),
    ("ZNodes", "Значение элемента данных", 48, 0, 24, 4, "edit"),
    ("HAProxy backends", "График", 0, 4, 36, 5, "edit"),
    ("HAProxy errors", "График", 36, 4, 36, 5, "edit"),
    ("Время", "Часы", 0, 9, 12, 1, "fixed"),
]


def chipw(text):
    return round(22 + 8.3 * len(text))


def hline(x1, x2, y, color="#eef0f4"):
    return f'  <line x1="{x1}" y1="{y}" x2="{x2}" y2="{y}" stroke="{color}"/>\n'


def vline(x, y1, y2, color="#eef0f4"):
    return f'  <line x1="{x}" y1="{y1}" x2="{x}" y2="{y2}" stroke="{color}"/>\n'


def write(svg, name, chips=(DIRTY, LOCAL), crumb=CRUMB):
    svg = appbar.apply(svg, active="drafts", mode=OFFLINE, depth_prefix="../../../../",
                       breadcrumb=crumb, chips=list(chips))
    os.makedirs(OUT, exist_ok=True)
    with open(OUT + name, "w", encoding="utf-8", newline="") as f:
        f.write(svg)
    print("written", OUT + name)


def outline(widgets, selected, title="ВИДЖЕТЫ СТРАНИЦЫ"):
    s = '  <rect x="0" y="72" width="260" height="928" fill="#ffffff" stroke="#e4e7ec"/>\n'
    s += t(24, 112, title, "label muted")
    s += t(24, 134, "сверху вниз, слева направо", "xs muted")
    y = 150
    for name, typ, x, yy, w, h, cap in widgets:
        sel = name == selected
        if sel:
            s += box(12, y, 236, 54, fill="#eef4ff", stroke="#3156a3", rx=8, sw=2)
        s += t(28, y + 22, name, "small", "#1d3f86" if sel else None, weight="700" if sel else "600")
        s += t(28, y + 42, f"{typ} · {x}, {yy} · {w} × {h}", "xs muted")
        if cap == "kept":
            s += icon("ban", 222, y + 9, "#8a4b00", 16, "Изменение не поддерживается")
        y += 60
    return s


def page_tabs(active, y=140, x=292):
    s = ""
    for name, w in [("PostgreSQL", 132), ("ClickHouse", 128), ("Keeper и HAProxy", 176)]:
        on = name == active
        s += box(x, y, w, 36, fill="#ffffff" if on else "#f5f7fb", stroke="#3156a3" if on else "#d0d5dd", rx=8, sw=2 if on else None)
        s += t(x + w / 2, y + 23, name, "small", "#1d3f86" if on else "#475467", "middle", "700" if on else "600")
        x += w + 8
    s += box(x, y, 36, 36, stroke=BORDER, rx=8) + icon("plus", x + 8, y + 8, "#475467", 20, "Добавить страницу")
    return s


def toolbar(y=186, synthetic=False):
    s = button(292, y, 196, "Добавить виджет", "primary", h=36, icon_name="plus")
    x = 500
    for ic, tip in [("arrow-back-up", "Отменить: переместить виджет"), ("arrow-forward-up", "Повторить")]:
        s += box(x, y, 36, 36, stroke=BORDER, rx=8) + icon(ic, x + 8, y + 8, "#475467", 20, tip)
        x += 44
    x += 12
    s += box(x, y, 36, 36, stroke=BORDER, rx=8) + icon("zoom-out", x + 8, y + 8, "#475467", 20, "Уменьшить")
    s += box(x + 44, y, 72, 36, stroke=BORDER, rx=8) + t(x + 80, y + 23, "100 %", "small", None, "middle", "600")
    s += box(x + 124, y, 36, 36, stroke=BORDER, rx=8) + icon("zoom-in", x + 132, y + 8, "#475467", 20, "Увеличить")
    s += button(x + 168, y, 150, "По ширине", h=36)
    cx = x + 336
    s += checkbox(cx, y + 8, synthetic) + t(cx + 30, y + 24, "Показать синтетические данные", "small")
    return s


def canvas(widgets, x0, y0, cw, rh, rows, selected=None, ghosts=(), ruler=True, compact=False):
    """Zabbix grid: 72 columns, integer cells; row height ~2.8 column widths."""
    s = "  <!-- Canvas: 72 columns, integer cells -->\n"
    W = 72 * cw
    H = rows * rh
    s += f'  <rect x="{x0}" y="{y0}" width="{W}" height="{H}" fill="#ffffff" stroke="#c9d3ea"/>\n'
    for c in range(1, 72):
        color = "#d6dde8" if c % 6 == 0 else "#f0f2f5"
        s += vline(f"{x0 + c * cw:.1f}", y0, y0 + H, color)
    for r in range(1, rows):
        s += hline(x0, f"{x0 + W:.1f}", f"{y0 + r * rh:.1f}", "#f0f2f5")
    if ruler:
        for c in range(0, 73, 6):
            s += t(f"{x0 + c * cw:.1f}", y0 - 6, str(c), "xs muted", anchor="middle")
        for r in range(0, rows, 2):
            s += t(x0 - 8, f"{y0 + r * rh + 12:.1f}", str(r), "xs muted", anchor="end")
    for name, typ, x, y, w, h, cap in widgets:
        px, py, pw, ph = x0 + x * cw + 2, y0 + y * rh + 2, w * cw - 4, h * rh - 4
        sel = name == selected
        fill = "#ffffff" if cap != "kept" else "#f8fafc"
        s += f'  <rect x="{px:.1f}" y="{py:.1f}" width="{pw:.1f}" height="{ph:.1f}" rx="6" fill="{fill}" stroke="{"#3156a3" if sel else "#b8c2d0"}" stroke-width="{2 if sel else 1}"/>\n'
        if ph >= 30:
            s += t(f"{px + 8:.1f}", f"{py + 18:.1f}", name, "xs" if compact else "small", weight="700")
        if ph >= 50 and not compact:
            s += t(f"{px + 8:.1f}", f"{py + 36:.1f}", typ, "xs muted")
        if cap == "kept" and ph >= 70 and not compact:
            s += chip(round(px + 8), round(py + ph - 34), 196, "Изменение не поддерживается", NEUTRAL)
        if sel:
            for hx, hy in [(px, py), (px + pw / 2, py), (px + pw, py), (px + pw, py + ph / 2), (px + pw, py + ph),
                           (px + pw / 2, py + ph), (px, py + ph), (px, py + ph / 2)]:
                s += f'  <rect x="{hx - 5:.1f}" y="{hy - 5:.1f}" width="10" height="10" rx="2" fill="#ffffff" stroke="#3156a3" stroke-width="2"/>\n'
    for name, x, y, w, h in ghosts:
        px, py, pw, ph = x0 + x * cw + 2, y0 + y * rh + 2, w * cw - 4, h * rh - 4
        s += f'  <rect x="{px:.1f}" y="{py:.1f}" width="{pw:.1f}" height="{ph:.1f}" rx="6" fill="#eef4ff" fill-opacity=".7" stroke="#3156a3" stroke-dasharray="6 4" stroke-width="2"/>\n'
        s += t(f"{px + 8:.1f}", f"{py + 18:.1f}", name, "xs", "#1d3f86", weight="700")
        s += t(f"{px + 8:.1f}", f"{py + 34:.1f}", f"вставка · {x}, {y}", "xs", "#1d3f86")
    return s


def title(text):
    return "  <!-- Title -->\n" + t(292, 124, text, "title")


def editor_base(selected="Queries per second", synthetic=False):
    s = outline(CLICKHOUSE, selected)
    s += title("Редактор страницы «ClickHouse»")
    s += page_tabs("ClickHouse")
    s += toolbar(synthetic=synthetic)
    # canvas frame
    s += button(1210, 98, 100, "Готово", h=34)
    s += box(292, 234, 888, 656, rx=12)
    s += canvas(CLICKHOUSE, 330, 266, 11.5, 32, 18, selected=selected)
    s += hline(304, 1168, 850, "#e4e7ec")
    s += t(316, 874, "Выбрано: Queries per second · столбец 36, строка 0 · 36 × 5 ячеек", "small", weight="600")
    s += t(1168, 874, "Занято строк: 8 из 64", "small muted", anchor="end")
    s += t(292, 914, "Стрелки — сдвиг на 1 ячейку · Shift+стрелки — на 6 · Ctrl+стрелки — размер · Delete — удалить (Ctrl+Z вернёт)", "xs muted")
    return s


# ================================================================== 12-01 editor shell + canvas
def editor_canvas():
    s = head("редактор страницы", "Светлый макет редактора страницы: вкладки страниц, холст сетки Zabbix 72 колонки с линейками, "
             "выбранный виджет с маркерами размера, список виджетов в порядке чтения, инспектор с положением и размером "
             "в ячейках, масштаб, синтетика выключена, локальный статус «Изменена».", ICONS)
    s += editor_base()
    # inspector
    s += box(1200, 140, 380, 750, rx=12)
    s += t(1222, 172, "ИНСПЕКТОР", "label muted")
    s += t(1222, 202, "Queries per second", "h3")
    s += chip(1222, 214, chipw("График · svggraph"), "График · svggraph", BLUE)
    s += chip(1222, 246, chipw("Изменение поддерживается"), "Изменение поддерживается", GREEN)
    s += t(1222, 300, "ПОЛОЖЕНИЕ И РАЗМЕР · в ячейках сетки", "label muted")
    for i, (lab, val) in enumerate([("X (столбец)", "36"), ("Y (строка)", "0"), ("Ширина", "36"), ("Высота", "5")]):
        cx = 1222 + (i % 2) * 172
        cy = 312 + (i // 2) * 66
        s += t(cx, cy + 14, lab, "xs muted")
        s += box(cx, cy + 20, 160, 36, stroke=BORDER, rx=8) + t(cx + 12, cy + 44, val, "body")
    s += t(1222, 462, "Целые ячейки: 0–71 по горизонтали, 0–63 по вертикали.", "xs muted")
    s += t(1222, 498, "ОСНОВНОЕ", "label muted")
    s += t(1222, 518, "Имя", "xs muted") + box(1222, 524, 334, 36, stroke=BORDER, rx=8) + t(1234, 548, "Queries per second", "body")
    s += t(1222, 594, "ДАННЫЕ", "label muted")
    for i, (n, d) in enumerate([("Набор 1", "узлы: роль «ClickHouse», все узлы"), ("", "элементы: «Queries per second»")]):
        s += t(1222, 618 + i * 22, (n + " · " if n else "          ") + d, "small")
    s += t(1222, 668, "Узлы сети подбираются в разделе «Привязки».", "xs muted")
    s += button(1222, 690, 334, "Все параметры виджета")
    s += f'  <line x1="1222" y1="752" x2="1556" y2="752" stroke="#e4e7ec"/>\n'
    s += button(1222, 768, 160, "Дублировать", h=36, icon_name="copy") + button(1394, 768, 162, "Удалить", "danger", h=36)
    s += t(1222, 836, "Изменения хранятся в черновике; сервер не меняется.", "xs muted")
    write(s + "</svg>\n", "12-01_P1-04A_editor-canvas.svg")


# ================================================================== 12-02 widget chooser
def widget_chooser():
    s = head("редактор: добавить виджет", "Светлый макет выбора виджета поверх редактора: типы, которые можно добавить и "
             "настроить, типы с настройками по умолчанию без изменения, распознаваемые типы, которые добавляются только "
             "копированием, заготовки позже; размещение в первом свободном месте.", ICONS)
    s += editor_base()
    s += '  <!-- Dialog -->\n  <rect x="0" y="72" width="1600" height="928" fill="#17213a" fill-opacity=".45"/>\n'
    s += box(300, 120, 1000, 800, stroke="#c9d3ea")
    s += t(332, 166, "Добавить виджет на страницу «ClickHouse»", "title")
    s += icon("x", 1260, 146, "#475467", 22, "Закрыть")
    s += box(332, 186, 560, 40, stroke=BORDER, rx=8) + icon("search", 344, 196, "#667085", 20) + t(374, 212, "Поиск по типу виджета", "small muted")
    groups = [
        ("МОЖНО ДОБАВИТЬ И НАСТРОИТЬ", "circle-check", "#1f6b43",
         [("График", "svggraph", True), ("Значение элемента данных", "item", False)], None),
        ("МОЖНО ДОБАВИТЬ, НАСТРОЙКИ ПО УМОЛЧАНИЮ", "info-circle", "#1d3f86",
         [("Часы", "clock", False), ("Проблемы", "problems", False)],
         "Поля этих типов ZDashstitch пока не изменяет: виджет создаётся с умолчаниями Zabbix."),
    ]
    y = 250
    for gname, ic, color, items, note in groups:
        s += icon(ic, 332, y - 14, color, 18) + t(358, y, gname, "label muted")
        y += 12
        for name, code, sel in items:
            s += box(332, y, 560, 48, fill="#eef4ff" if sel else "#ffffff", stroke="#3156a3" if sel else "#e4e7ec", rx=8, sw=2 if sel else None)
            s += radio(354, y + 24, sel) + t(376, y + 30, name, "body", weight="650") + t(876, y + 30, code, "mono", anchor="end")
            y += 56
        if note:
            s += t(332, y + 4, note, "xs muted")
            y += 22
        y += 22
    s += icon("ban", 332, y - 14, "#8a4b00", 18) + t(358, y, "ТОЛЬКО КОПИРОВАНИЕМ · 26 ТИПОВ", "label muted")
    s += t(332, y + 24, "Распознаются и сохраняются как есть; добавить можно, скопировав", "small muted")
    s += t(332, y + 46, "виджет с другой страницы или панели. Например: Радиальный датчик,", "small muted")
    s += t(332, y + 68, "Топ узлов сети, Карта сети, Сотовый график.", "small muted")
    s += icon("chevron-right", 328, y + 84, "#475467", 18) + t(352, y + 98, "Показать все 26", "small", "#3156a3", weight="650")
    y += 138
    s += t(358, y, "ИЗ ЗАГОТОВОК", "label muted") + chip(470, y - 18, 140, "Появится позже", NEUTRAL)
    s += t(332, y + 24, "Заготовки виджетов появятся вместе с профилями оформления.", "small muted")
    # right: preview of choice
    s += box(916, 186, 352, 640, fill="#f8fafc", stroke="#e4e7ec", rx=12)
    s += t(940, 220, "График", "h3") + t(940, 244, "svggraph · Zabbix 7.4.0+", "xs muted")
    s += t(940, 282, "Ряды по шаблонам узлов и элементов", "small")
    s += t(940, 304, "данных; несколько наборов данных.", "small")
    s += t(940, 346, "РАЗМЕР ПО УМОЛЧАНИЮ", "label muted") + t(940, 372, "12 × 5 ячеек (как в Zabbix)", "small")
    s += t(940, 414, "РАЗМЕСТИТЬ", "label muted")
    s += radio(952, 438, True) + t(972, 444, "В первое свободное место: 0, 8", "small")
    s += radio(952, 472, False) + t(972, 478, "Указать на холсте", "small")
    s += t(940, 520, "Существующие виджеты не сдвигаются.", "xs muted")
    s += t(940, 562, "ПОСЛЕ ДОБАВЛЕНИЯ", "label muted")
    s += t(940, 588, "Откроется инспектор: имя и наборы", "small")
    s += t(940, 610, "данных. Узлы сети — в «Привязках».", "small")
    s += button(940, 762, 128, "Отмена") + button(1080, 762, 164, "Добавить", "primary")
    write(s + "</svg>\n", "12-02_P1-04C_widget-chooser.svg")


# ================================================================== 12-03 inspector svggraph
def inspector():
    s = head("редактор: параметры графика", "Светлый макет полного инспектора виджета «График»: разделы основное, данные, "
             "отображение, оси и пороги, период, дополнительно и неизвестные поля только для чтения; проверка рядом с "
             "полем, значение по умолчанию Zabbix против изменённого, привязка узлов через роль.", ICONS)
    s += outline(CLICKHOUSE, "Queries per second")
    s += title("Параметры «Queries per second»")
    s += page_tabs("ClickHouse")
    # mini canvas
    s += box(292, 186, 520, 704, rx=12)
    s += t(312, 214, "СТРАНИЦА «ClickHouse»", "label muted")
    s += canvas(CLICKHOUSE, 316, 246, 6.6, 18.5, 18, selected="Queries per second", ruler=False, compact=True)
    s += t(312, 600, "Изменения инспектора видны на холсте сразу.", "xs muted")
    # inspector
    s += box(832, 140, 748, 750, rx=12)
    s += t(856, 172, "Queries per second", "h3")
    s += chip(856, 184, chipw("График · svggraph"), "График · svggraph", BLUE) + chip(1032, 184, chipw("Изменение поддерживается"), "Изменение поддерживается", GREEN)
    s += button(1424, 150, 132, "Готово", h=34)
    secs = [("Основное", False), ("Данные", True), ("Отображение", False), ("Оси и пороги", False), ("Период", False),
            ("Дополнительно", False), ("Неизвестные поля · 2", False)]
    y = 236
    for name, on in secs:
        if on:
            s += box(856, y, 176, 36, fill="#eef4ff", stroke="#eef4ff", rx=8)
        s += t(870, y + 23, name, "small", "#1d3f86" if on else None, weight="700" if on else None)
        y += 42
    s += t(856, y + 14, "Разделы — порядок табуляции.", "xs muted")
    # Data section content
    X = 1052
    s += t(X, 252, "НАБОРЫ ДАННЫХ", "label muted")
    s += box(X, 266, 504, 236, fill="#ffffff", stroke="#d0d5dd", rx=10)
    s += icon("grip-vertical", X + 10, 280, "#98a2b3", 18, "Перетащить набор")
    s += t(X + 36, 294, "Набор 1 · шаблоны", "small", weight="700")
    s += f'  <rect x="{X + 448}" y="282" width="16" height="16" rx="3" fill="#3b82c4"/>\n'
    s += icon("x", X + 474, 280, "#667085", 18, "Удалить набор")
    s += t(X + 16, 324, "Узлы сети", "xs muted")
    s += box(X + 16, 330, 472, 36, fill="#f8fafc", stroke=BORDER, rx=8) + t(X + 28, 354, "роль «ClickHouse» · все узлы роли", "small")
    s += t(X + 476, 354, "в «Привязках»", "xs", "#3156a3", "end", "650")
    s += t(X + 16, 388, "Элементы данных (шаблон имени)", "xs muted")
    s += box(X + 16, 394, 472, 36, stroke=BORDER, rx=8) + t(X + 28, 418, "Queries per second", "body")
    s += t(X + 16, 452, "Интервал агрегации", "xs muted")
    s += box(X + 16, 458, 160, 32, stroke="#a32935", rx=8, sw=2) + t(X + 28, 480, "1 часов", "small")
    s += icon("alert-triangle", X + 186, 464, "#a32935", 18)
    s += t(X + 210, 479, "Неверный формат: например 1h или 30m", "xs", "#a32935", weight="650")
    s += box(X, 514, 504, 54, fill="#ffffff", stroke="#d0d5dd", rx=10)
    s += t(X + 36, 546, "Набор 2 · шаблоны · узлы роли «ClickHouse» · «Inserts per second»", "small")
    s += icon("chevron-right", X + 10, 532, "#475467", 18)
    s += icon("plus", X, 586, "#3156a3", 18) + t(X + 24, 600, "Добавить набор данных", "small", "#3156a3", weight="650")
    s += t(X, 640, "ВИД РЯДОВ НАБОРА 1", "label muted")
    s += t(X, 664, "Тип: линия", "small") + chip(X + 120, 648, 170, "умолчание Zabbix", NEUTRAL)
    s += t(X, 696, "Ширина линии: 2", "small") + chip(X + 120, 680, 96, "изменено", AMBER)
    s += t(X + 226, 696, "Вернуть умолчание", "xs", "#3156a3", weight="650")
    s += t(X, 742, "НЕИЗВЕСТНЫЕ ПОЛЯ · ТОЛЬКО ПРОСМОТР", "label muted")
    s += box(X, 754, 504, 64, fill="#f8fafc", stroke="#e4e7ec", rx=8)
    s += t(X + 12, 778, "ds.0.vendor_hint", "mono") + t(X + 220, 778, "тип 1", "xs muted") + t(X + 300, 778, "\"compact\"", "mono")
    s += t(X + 12, 802, "legend_lines_mode", "mono") + t(X + 220, 802, "тип 0", "xs muted") + t(X + 300, 802, "1", "mono")
    s += t(X, 842, "Неизвестные поля передаются как есть и не изменяются.", "xs muted")
    write(s + "</svg>\n", "12-03_P1-04D_inspector-svggraph.svg")


# ================================================================== 12-04 paste resolution
def paste_resolution():
    s = head("редактор: вставка виджетов", "Светлый макет вставки трёх виджетов со страницы другой панели на страницу "
             "«Keeper и HAProxy»: исходные места заняты — предложены свободные, связи виджетов сохранены, роль узлов "
             "сети отсутствует в черновике, тип без изменения копируется как есть; холст показывает места вставки.", ICONS)
    s += outline(KEEPER, None)
    s += title("Редактор страницы «Keeper и HAProxy»")
    s += page_tabs("Keeper и HAProxy")
    s += box(292, 186, 520, 704, rx=12)
    s += t(312, 214, "МЕСТА ВСТАВКИ", "label muted")
    ghosts = [("Queries per second", 0, 10, 36, 5), ("Replication lag", 36, 10, 24, 3), ("Top queries", 36, 13, 24, 3)]
    s += canvas(KEEPER, 316, 246, 6.6, 18.5, 18, ghosts=ghosts, ruler=False, compact=True)
    s += t(312, 600, "Пунктир — новые виджеты. Существующие", "xs muted")
    s += t(312, 618, "не сдвигаются.", "xs muted")
    # dialog
    s += box(832, 140, 748, 750, stroke="#c9d3ea")
    s += t(856, 182, "Вставить 3 виджета", "title")
    s += t(856, 210, "Из «Базы данных: Prod» → страница «ClickHouse». Источник не меняется.", "small muted")
    rows = [
        ("circle-check", "#1f6b43", "Queries per second", "График", "место 36, 0 занято → 0, 10", ["Связь «период» с Replication lag вставляется вместе"], None),
        ("alert-triangle", "#8a4b00", "Replication lag", "Значение элемента данных", "место 0, 5 занято → 36, 10",
         ["Роль узлов «ClickHouse» в этом черновике нет"], "role"),
        ("ban", "#8a4b00", "Top queries", "Топ узлов сети", "место 48, 5 занято → 36, 13",
         ["Копируется как есть: изменение не поддерживается"], None),
    ]
    y = 236
    for ic, color, name, typ, place, notes, kind in rows:
        h = 90 + (56 if kind == "role" else 0)
        s += box(856, y, 700, h, fill="#ffffff", stroke="#e4e7ec", rx=10)
        s += icon(ic, 872, y + 14, color, 20)
        s += t(902, y + 30, name, "body", weight="700") + t(1540, y + 30, typ, "small muted", anchor="end")
        s += t(902, y + 56, place, "small") + t(1540, y + 56, "Изменить место", "small", "#3156a3", "end", "650")
        for i, n in enumerate(notes):
            s += t(902, y + 78 + i * 20, n, "xs muted" if kind != "role" else "xs", "#8a4b00" if kind == "role" else None)
        if kind == "role":
            s += radio(914, y + 108, True) + t(934, y + 114, "Добавить роль «ClickHouse» (узлы — в «Привязках»)", "small")
            s += radio(914, y + 132, False) + t(934, y + 138, "Оставить ссылку «нужна привязка»", "small")
        y += h + 10
    s += t(856, y + 18, "ССЫЛКИ МЕЖДУ ВИДЖЕТАМИ", "label muted")
    s += t(856, y + 42, "Значение reference «QPS01» на этой странице уже занято — переименуется", "small")
    s += t(856, y + 64, "в «QPS02» у источника и у всех, кто на него ссылается.", "small")
    s += t(856, y + 104, "Отменить вставку можно одним Ctrl+Z.", "xs muted")
    s += button(1268, 830, 120, "Отмена") + button(1400, 830, 156, "Вставить 3", "primary")
    write(s + "</svg>\n", "12-04_P1-04E_paste-resolution.svg",
          crumb="Черновики  /  Базы данных: сводная  /  Структура  /  Keeper и HAProxy")


if __name__ == "__main__":
    editor_canvas()
    widget_chooser()
    inspector()
    paste_resolution()
