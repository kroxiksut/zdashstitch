"""Review mockup 06-02: «Настройки → Общие» with an invalid settings file (P1-09B).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/settings_yaml_svg.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402
from publication_svg import box, button, head, icon, t  # noqa: E402

OUT = "docs/design/assets/06-settings/"
OFFLINE = "offline"
NAV = ["Общие", "Внешний вид", "Рабочие каталоги", "Редактор", "Подключения и сертификаты",
       "Безопасность и экспорт", "Диагностика", "О программе и лицензии"]


def nav(selected):
    s = '  <rect x="0" y="72" width="260" height="928" fill="#ffffff" stroke="#e4e7ec"/>\n'
    s += t(28, 116, "НАСТРОЙКИ", "label muted")
    y = 136
    for name in NAV:
        if name == selected:
            s += box(16, y, 228, 42, fill="#eef4ff", stroke="#eef4ff", rx=10)
            s += t(32, y + 27, name, "body", "#1d3f86", weight="700")
            s += icon("alert-triangle", 214, y + 12, "#a32935", 18, "В файле настроек есть ошибки")
        else:
            s += t(32, y + 27, name, "body" if len(name) <= 22 else "small")
        y += 46
    s += '  <line x1="20" y1="520" x2="240" y2="520" stroke="#e4e7ec"/>\n'
    s += t(28, 552, "Пароли и токены здесь", "small muted") + t(28, 574, "не хранятся и не показываются.", "small muted")
    return s


def lines(x, y, items, cls="small muted", fill=None, step=22):
    return "".join(t(x, y + i * step, v, cls, fill) for i, v in enumerate(items))


def general_with_error():
    s = head("настройки: общие, ошибка в файле",
             "Светлый макет раздела «Настройки → Общие» при ошибочном файле настроек: файл не применён и не "
             "перезаписывается, работают последние верные настройки; ошибки с номером строки и ключом; действия "
             "открыть, перечитать, восстановить копию или заменить файл верными настройками с сохранением ошибочного "
             "рядом; путь к файлу, резервные копии, экспорт, импорт и сброс.",
             ["alert-triangle", "circle-x", "refresh", "info-circle", "search"])
    s += nav("Общие")
    s += "  <!-- Title -->\n" + t(292, 120, "Общие", "title")
    s += t(292, 150, "Файл настроек, резервные копии, перенос и сброс.", "small muted")
    s += box(1224, 96, 320, 40, stroke="#c9d3ea", rx=8) + icon("search", 1238, 107, "#667085", 18) + t(1266, 121, "Поиск настроек", "small muted")
    # banner
    s += box(292, 166, 1252, 92, fill="#fef3f2", stroke="#f2b8b5", rx=14)
    s += icon("alert-triangle", 316, 186, "#a32935", 24)
    s += t(352, 204, "Файл настроек содержит ошибки и не применён", "h3", "#a32935")
    s += t(352, 230, "Работают последние верные настройки (прочитаны 25.10.2026 в 14:02). Пока файл не исправлен, изменения", "small", "#5c1f1f")
    s += t(352, 250, "в интерфейсе не сохраняются: файл с ошибкой не перезаписывается.", "small", "#5c1f1f")
    # errors
    s += box(292, 274, 820, 404)
    s += t(316, 308, "ОШИБКИ В ФАЙЛЕ · ИЗМЕНЁН ВНЕ ПРИЛОЖЕНИЯ В 14:31", "label muted")
    errs = [("Строка 4, столбец 8", "theme", "значение «blue» не поддерживается; допустимо: system, light, dark"),
            ("Строка 6, столбец 15", "storage_root", "путь должен быть абсолютным: «ZDashstitch\\data»"),
            ("Строка 9, столбец 1", "editor_zoom", "неизвестный ключ: в формате v0 такого параметра нет")]
    y = 324
    for where, key, msg in errs:
        s += box(316, y, 772, 66, fill="#fff5f5", stroke="#f6d0cd", rx=8)
        s += icon("circle-x", 330, y + 12, "#a32935", 18)
        s += t(356, y + 26, where, "small", weight="650") + t(536, y + 26, key, "mono")
        s += t(356, y + 50, msg, "small")
        y += 74
    s += t(316, y + 16, "Текст файла в отчётах и журнале не сохраняется; показаны только строка, ключ и причина.", "xs muted")
    s += button(316, y + 34, 170, "Открыть файл") + button(498, y + 34, 170, "Перечитать", icon_name="refresh")
    s += button(680, y + 34, 408, "Заменить файл верными настройками…")
    s += t(316, y + 104, "Ошибочный файл не удаляется: он сохранится рядом как settings.invalid-2026-10-25-1431.yaml.", "xs muted")
    # file card
    s += box(1132, 274, 412, 244)
    s += t(1156, 308, "ФАЙЛ НАСТРОЕК", "label muted")
    s += t(1156, 336, "C:\\Users\\admin\\AppData\\Local\\", "mono") + t(1156, 356, "ZDashstitch\\settings.yaml", "mono")
    kv = [("Формат", "zdashstitch-settings/v0"), ("Последнее верное", "25.10.2026, 14:02"), ("Изменён", "25.10.2026, 14:31, вне приложения")]
    y = 390
    for k, v in kv:
        s += t(1156, y, k, "small muted") + t(1300, y, v, "small", weight="600")
        y += 26
    s += button(1156, 462, 186, "Открыть папку", h=38) + button(1354, 462, 166, "Копировать путь", h=38)
    # backups
    s += box(1132, 534, 412, 144)
    s += t(1156, 568, "РЕЗЕРВНЫЕ КОПИИ", "label muted")
    for i, (when, note) in enumerate([("25.10.2026, 14:02", "последняя верная"), ("24.10.2026, 18:40", ""), ("23.10.2026, 09:15", "")]):
        yy = 596 + i * 26
        s += t(1156, yy, when, "small") + t(1300, yy, note, "small muted") + t(1520, yy, "Восстановить", "small", "#3156a3", "end", "650")
    # transfer & reset
    s += box(292, 694, 1252, 214)
    s += t(316, 728, "ПЕРЕНОС И СБРОС", "label muted")
    s += t(316, 760, "Экспорт сохраняет последние верные настройки в YAML-файл; секретов в нём нет. Импорт сначала", "small muted")
    s += t(316, 782, "проверяет файл и показывает, какие значения изменятся; заменённый файл сохраняется в резервные копии.", "small muted")
    s += button(316, 804, 200, "Экспорт настроек…") + button(528, 804, 200, "Импорт настроек…")
    s += button(1300, 804, 220, "Сбросить все настройки…", "danger")
    s += icon("info-circle", 316, 866, "#667085", 18)
    s += t(342, 880, "Сброс показывает список параметров, которые вернутся к значениям по умолчанию; корень хранилища не переносится и данные не удаляются.", "xs muted")
    svg = appbar.apply(s + "</svg>\n", active=None, mode=OFFLINE, depth_prefix="../../../../", settings_active=True)
    os.makedirs(OUT, exist_ok=True)
    with open(OUT + "06-02_P1-09B_settings-file-error.svg", "w", encoding="utf-8", newline="") as f:
        f.write(svg)
    print("written", OUT + "06-02_P1-09B_settings-file-error.svg")


if __name__ == "__main__":
    general_with_error()
