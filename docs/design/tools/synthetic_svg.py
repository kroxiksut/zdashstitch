"""Review mockup 12-06: page preview with synthetic data (P1-04I, O-028).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/synthetic_svg.py
"""
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402
from bindings_svg import sidebar  # noqa: E402
from publication_svg import AMBER, NEUTRAL, box, checkbox, chip, head, icon, select, t  # noqa: E402

OUT = "docs/design/assets/12-editor/12-06_P1-04I_synthetic-preview.svg"
# Series palette: distinct from status colors (success/warning/danger) and from the action blue.
SERIES = ["#3b82c4", "#8b5cf6"]
X0, Y0, W = 316, 240, 1204.0
COL = W / 72
ROW = 70 * W / 1920
ROWS = 10


def badge(bx, by, bw):
    return (f'  <rect x="{bx + bw - 80:.1f}" y="{by + 6:.1f}" width="72" height="18" rx="9" fill="#fff4e5"/>\n'
            f'  <text x="{bx + bw - 44:.1f}" y="{by + 19:.1f}" font-size="11" text-anchor="middle" fill="#8a4b00">Синтетика</text>\n')


def wbox(x, y, w, h, name, kind):
    bx, by = X0 + x * COL + 3, Y0 + y * ROW + 3
    bw, bh = w * COL - 6, h * ROW - 6
    s = f'  <rect x="{bx:.1f}" y="{by:.1f}" width="{bw:.1f}" height="{bh:.1f}" rx="6" fill="#ffffff" stroke="#d0d5dd"/>\n'
    s += t(f"{bx + 10:.1f}", f"{by + 19:.1f}", name, "small", weight="700")
    s += t(f"{bx + 10 + 7.6 * len(name) + 10:.1f}", f"{by + 19:.1f}", kind, "xs muted")
    return s, bx, by, bw, bh


def series(px0, px1, py0, py1, seed, base, amp, n=60):
    pts = []
    for i in range(n + 1):
        u = i / n
        v = base + amp * (0.55 * math.sin(u * 6.3 + seed) + 0.3 * math.sin(u * 17 + seed * 2) + 0.15 * math.cos(u * 41 + seed))
        v = min(max(v, 0.04), 0.96)
        pts.append(f"{px0 + u * (px1 - px0):.1f},{py1 - v * (py1 - py0):.1f}")
    return " ".join(pts)


def axes(px0, px1, py0, py1, ticks, units=None):
    s = f'  <line x1="{px0:.1f}" y1="{py1:.1f}" x2="{px1:.1f}" y2="{py1:.1f}" stroke="#d0d5dd"/>\n'
    for i, label in enumerate(ticks):
        yy = py1 - i * (py1 - py0) / (len(ticks) - 1)
        if i:
            s += f'  <line x1="{px0:.1f}" y1="{yy:.1f}" x2="{px1:.1f}" y2="{yy:.1f}" stroke="#f2f4f7"/>\n'
        s += t(f"{px0 - 8:.1f}", f"{yy + 4:.1f}", label, "xs muted", anchor="end")
    if units:
        s += t(f"{px0 - 8:.1f}", f"{py0 - 12:.1f}", units, "xs muted", anchor="end")
    hours = ["13:00", "13:15", "13:30", "13:45", "14:00"]
    for i, hh in enumerate(hours):
        xx = px0 + i * (px1 - px0) / 4
        anchor = "start" if i == 0 else ("end" if i == len(hours) - 1 else "middle")
        s += t(f"{xx:.1f}", f"{py1 + 17:.1f}", hh, "xs muted", anchor=anchor)
    return s


def legend_row(x, y, color, text, no_data=False):
    if no_data:
        return (icon("info-circle", x, y - 11, "#667085", 14) + t(x + 20, y, text, "xs", "#475467"))
    return (f'  <rect x="{x}" y="{y - 9}" width="12" height="4" rx="2" fill="{color}"/>\n' + t(x + 20, y, text, "xs"))


def build():
    s = head("предпросмотр с синтетическими данными",
             "Светлый макет предпросмотра страницы «ClickHouse» черновика «Базы данных: сводная» с включённой синтетикой: "
             "графики с синтетическими рядами и меткой «Синтетика», линия порога триггера, ряд «нет данных для синтетики», "
             "значение элемента данных, карточки без данных для типов без синтетики, пояснение, основания по виджетам и обозначения.",
             ["check", "chevron-down", "info-circle", "ban"])
    s += sidebar(["Локальная", "собрана из 3 страниц", "19 виджетов"], selected="Структура")
    s += "  <!-- Title -->\n" + t(292, 124, "Базы данных: сводная", "title")
    s += t(292, 154, "Предпросмотр страницы «ClickHouse» · 5 виджетов · сетка Zabbix 72 колонки", "small muted")
    # preview card and controls
    s += box(292, 172, 1252, 534)
    s += checkbox(316, 190, True) + t(346, 206, "Показать синтетические данные", "body")
    s += select(880, 184, 270, "Период: последний 1 час", h=34)
    s += select(1166, 184, 354, "Ширина экрана: 1920 px", h=34)
    grid_h = ROWS * ROW
    s += f'  <rect x="{X0}" y="{Y0}" width="{W:.0f}" height="{grid_h:.1f}" fill="#f8fafc" stroke="#d0d5dd"/>\n'
    for c in range(6, 72, 6):
        s += f'  <line x1="{X0 + c * COL:.1f}" y1="{Y0}" x2="{X0 + c * COL:.1f}" y2="{Y0 + grid_h:.1f}" stroke="#eaecf0"/>\n'
    for r in range(1, ROWS):
        s += f'  <line x1="{X0}" y1="{Y0 + r * ROW:.1f}" x2="{X0 + W:.0f}" y2="{Y0 + r * ROW:.1f}" stroke="#eaecf0"/>\n'

    # Connections: two synthetic series, unsigned without units
    w, bx, by, bw, bh = wbox(0, 0, 36, 5, "Connections", "График")
    s += w + badge(bx, by, bw)
    px0, px1, py0, py1 = bx + 56, bx + bw - 14, by + 44, by + bh - 54
    s += axes(px0, px1, py0, py1, ["0", "60", "120"])
    s += f'  <polyline points="{series(px0, px1, py0, py1, 0.4, 0.55, 0.22)}" fill="none" stroke="{SERIES[0]}" stroke-width="1.8"/>\n'
    s += f'  <polyline points="{series(px0, px1, py0, py1, 2.7, 0.25, 0.12)}" fill="none" stroke="{SERIES[1]}" stroke-width="1.8"/>\n'
    s += legend_row(bx + 14, by + bh - 12, SERIES[0], "TCP connections · ClickHouse, слот 1")
    s += legend_row(bx + 300, by + bh - 12, SERIES[1], "HTTP connections · ClickHouse, слот 1")

    # Queries per second: one series + threshold, second dataset without data
    w, bx, by, bw, bh = wbox(36, 0, 36, 5, "Queries per second", "График")
    s += w + badge(bx, by, bw)
    px0, px1, py0, py1 = bx + 56, bx + bw - 14, by + 44, by + bh - 54
    s += axes(px0, px1, py0, py1, ["0", "250", "500"], "rps")
    thr = py1 - 0.8 * (py1 - py0)
    s += f'  <line x1="{px0:.1f}" y1="{thr:.1f}" x2="{px1:.1f}" y2="{thr:.1f}" stroke="#a32935" stroke-dasharray="6 4" stroke-width="1.4"/>\n'
    s += t(f"{px1 - 4:.1f}", f"{thr - 5:.1f}", "Порог триггера: 400 rps", "xs", "#a32935", "end")
    s += f'  <polyline points="{series(px0, px1, py0, py1, 1.3, 0.48, 0.2)}" fill="none" stroke="{SERIES[0]}" stroke-width="1.8"/>\n'
    s += legend_row(bx + 14, by + bh - 12, SERIES[0], "Queries per second · ClickHouse, слот 1")
    s += legend_row(bx + 300, by + bh - 12, None, "Inserts* — нет данных для синтетики", no_data=True)

    # Replication lag: item value
    w, bx, by, bw, bh = wbox(0, 5, 24, 3, "Replication lag", "Значение")
    s += w + badge(bx, by, bw)
    s += t(f"{bx + bw / 2:.1f}", f"{by + bh / 2 + 18:.1f}", "0,8 с", "title", None, "middle")
    s += t(f"{bx + bw / 2:.1f}", f"{by + bh - 10:.1f}", "ClickHouse, слот 1", "xs muted", anchor="middle")

    # Disk usage, Top queries: known types without synthetic -> cards
    for (x, name, kind, line) in [(24, "Disk usage", "Радиальный датчик", "мин. 0 · макс. 100 · %"),
                                  (48, "Top queries", "Топ узлов сети", "столбцов: 3 · узлов: 10")]:
        w, bx, by, bw, bh = wbox(x, 5, 24, 3, name, kind)
        s += w
        s += t(f"{bx + 10:.1f}", f"{by + 52:.1f}", line, "small")
        s += t(f"{bx + 10:.1f}", f"{by + 78:.1f}", "Показано без данных: тип и основные настройки", "xs muted")

    s += t(316, Y0 + grid_h + 18, "Показано строк: 10 из 64. Часы и другие типы без синтетики данных не показывают.", "xs muted")
    # notice
    s += box(292, 716, 1252, 46, fill="#fff8ed", stroke="#f3d3a6", rx=10)
    s += icon("info-circle", 308, 730, "#8a4b00", 18)
    s += t(334, 744, "Синтетические данные — не значения из Zabbix. В файлы, экспорт и публикацию не попадают.", "small", "#5c3a00", weight="650")
    # basis table
    s += box(292, 774, 820, 134, rx=14)
    s += t(316, 800, "НА ЧЁМ ПОСТРОЕНА СИНТЕТИКА", "label muted")
    rows = [("Connections", "целое без знака, без единиц · шаг 1 мин"),
            ("Queries per second", "дробное, rps · порог 400 — значение макроса шаблона по умолчанию"),
            ("Inserts*", "шаблон элементов не совпал ни с одной метрикой каталога"),
            ("Replication lag", "дробное, s · задержка: малое основание, редкие пики")]
    y = 824
    for name, basis in rows:
        s += t(316, y, name, "small", weight="650") + t(480, y, basis, "small muted")
        y += 22
    # legend
    s += box(1132, 774, 412, 134, rx=14)
    s += t(1156, 800, "ОБОЗНАЧЕНИЯ", "label muted")
    s += chip(1156, 810, 82, "Синтетика", AMBER) + t(1250, 828, "синтетические значения", "xs muted")
    s += f'  <line x1="1156" y1="850" x2="1236" y2="850" stroke="#a32935" stroke-dasharray="6 4" stroke-width="1.4"/>\n'
    s += t(1250, 854, "порог триггера из шаблона", "xs muted")
    s += icon("info-circle", 1156, 866, "#667085", 14) + t(1176, 878, "нет данных: ряд не придумывается", "xs muted")
    s += icon("ban", 1156, 886, "#8a4b00", 14) + t(1176, 898, "тип неизвестен: каркас «Изменение не поддерживается»", "xs muted")

    svg = appbar.apply(s + "</svg>\n", active="drafts", mode="offline", depth_prefix="../../../../",
                       breadcrumb="Черновики  /  Базы данных: сводная  /  Структура  /  Предпросмотр",
                       chips=(appbar.DIRTY, appbar.UNCHECKED))
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="") as f:
        f.write(svg)
    print("written", OUT)


if __name__ == "__main__":
    build()
