"""Review mockup 06-04: «Настройки → О программе и лицензии» (P1-12).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/about_svg.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402
from publication_svg import box, button, chip, head, icon, t  # noqa: E402

OUT = "docs/design/assets/06-settings/06-04_P1-12_about.svg"
BLUE = ("#eef4ff", "#1d3f86")
NEUTRAL = ("#f2f4f7", "#344054")
NAV = ["Общие", "Внешний вид", "Рабочие каталоги", "Редактор", "Подключения и сертификаты",
       "Безопасность и экспорт", "Диагностика", "О программе и лицензии"]
SELECTED = "О программе и лицензии"


def settings_nav():
    s = '  <rect x="0" y="72" width="260" height="928" fill="#ffffff" stroke="#e4e7ec"/>\n'
    s += t(28, 116, "НАСТРОЙКИ", "label muted")
    y = 136
    for item in NAV:
        if item == SELECTED:
            s += box(16, y, 228, 42, fill="#eef4ff", stroke="#eef4ff", rx=10)
            s += t(32, y + 27, item, "nav" if len(item) <= 18 else "body", "#1d3f86", weight="700")
        else:
            s += t(32, y + 27, item, "nav" if len(item) <= 18 else "small")
        y += 46
    s += '  <line x1="20" y1="520" x2="240" y2="520" stroke="#e4e7ec"/>\n'
    s += t(28, 552, "Пароли и токены здесь", "small muted") + t(28, 574, "не хранятся и не показываются.", "small muted")
    return s


def logo(x, y, size):
    href = "../../../../assets/brand/icons/stezhik-head-1024.png"
    return f'  <image href="{href}" x="{x}" y="{y}" width="{size}" height="{size}" preserveAspectRatio="xMidYMid meet"/>\n'


def build():
    s = head("настройки: о программе и лицензии",
             "Светлый макет раздела «Настройки → О программе и лицензии»: имя и логотип, версия и сборка, "
             "поддерживаемые версии Zabbix, лицензии ZDashstitch, перечень сторонних компонентов с лицензиями "
             "без загрузки из сети, где лежат данные на этом компьютере и копирование сведений для отчёта без секретов.",
             ["search", "copy", "chevron-right", "chevron-down", "external-link", "info-circle"])
    s = s.replace("  <!-- Workspace -->\n", "  <!-- Settings navigation -->\n")
    s += settings_nav()
    s += t(292, 120, "О программе и лицензии", "title")
    s += t(292, 150, "Сведения о сборке и лицензиях. Всё показывается без обращения к сети.", "small muted")

    # product card
    s += box(292, 176, 610, 236)
    s += logo(316, 200, 72)
    s += t(408, 228, "ZDashstitch", "title")
    s += t(408, 254, "Локальный редактор глобальных панелей Zabbix", "small muted")
    s += t(408, 290, "Версия", "small muted") + t(520, 290, "0.1.0", "mono")
    s += t(408, 316, "Сборка", "small muted") + t(520, 316, "a1b2c3d4e5f6 · 25.10.2026", "mono")
    s += t(408, 342, "Zabbix", "small muted") + t(520, 342, "7.4 (адаптер 7.4, проверено на 7.4.16)", "small", weight="600")
    s += t(408, 368, "Платформа", "small muted") + t(520, 368, "Windows 10 · x86_64", "small", weight="600")
    s += t(316, 398, "© 2026 Fyodor Malkov", "xs muted")

    # own licences
    s += box(292, 428, 610, 230)
    s += t(316, 462, "ЛИЦЕНЗИИ ZDASHSTITCH", "label muted")
    s += t(316, 492, "Программа и исходный код", "small muted")
    s += t(316, 516, "MIT OR Apache-2.0 — на выбор получателя", "body", weight="650")
    s += t(316, 540, "Текст MIT", "small", "#3156a3", weight="650") + t(410, 540, "Текст Apache-2.0", "small", "#3156a3", weight="650")
    s += t(316, 574, "Документация и оригинальная графика", "small muted")
    s += t(316, 598, "CC BY 4.0", "body", weight="650") + t(410, 598, "Текст лицензии", "small", "#3156a3", weight="650")
    s += t(316, 632, "Zabbix — отдельный продукт; его лицензии и товарные знаки на ZDashstitch не переходят.", "xs muted")

    # local data
    s += box(292, 674, 610, 234)
    s += t(316, 708, "ДАННЫЕ НА ЭТОМ КОМПЬЮТЕРЕ", "label muted")
    rows = [("Хранилище", "%LOCALAPPDATA%\\ZDashstitch"),
            ("Файл настроек", "…\\ZDashstitch\\settings.yaml"),
            ("Журнал операций", "%LOCALAPPDATA%\\ZDashstitch\\journal")]
    y = 740
    for k, v in rows:
        s += t(316, y, k, "small muted") + t(470, y, v, "mono")
        s += icon("copy", 860, y - 15, "#475467", 18, "Копировать путь")
        y += 30
    s += t(316, y + 6, "Полные пути видны только здесь и не попадают в отчёты.", "xs muted")
    s += button(316, y + 22, 300, "Копировать сведения для отчёта", icon_name="copy", h=40)
    s += t(632, y + 47, "без адресов серверов, имён и секретов", "xs muted")

    # third-party
    s += box(926, 176, 618, 732)
    s += t(950, 210, "СТОРОННИЕ КОМПОНЕНТЫ", "label muted")
    s += t(950, 234, "Перечень собран при сборке программы; тексты лицензий встроены в неё.", "xs muted")
    x = 950
    for label, w, on in [("Все 268", 92, True), ("Значки 1", 98, False), ("Rust 243", 98, False), ("Интерфейс 24", 128, False)]:
        if on:
            s += box(x, 250, w, 32, fill="#243746", stroke="#243746", rx=16) + t(x + w / 2, 271, label, "small", "#ffffff", "middle", "650")
        else:
            s += box(x, 250, w, 32, stroke="#c9d3ea", rx=16) + t(x + w / 2, 271, label, "small", None, "middle", "600")
        x += w + 8
    s += box(950, 294, 570, 38, stroke="#7a8d9b", rx=8) + icon("search", 962, 303, "#667085", 20)
    s += t(992, 318, "Поиск по имени или лицензии", "small muted")
    s += box(950, 344, 570, 32, fill="#f8fafc", stroke="#eef0f4", rx=6)
    for cx, label in [(986, "КОМПОНЕНТ"), (1196, "ВЕРСИЯ"), (1306, "ЛИЦЕНЗИЯ")]:
        s += t(cx, 365, label, "xs muted", weight="700")
    comps = [
        ("chevron-down", "Tabler Icons", "3.49.0", "MIT", "значки интерфейса · 24 значка"),
        ("chevron-right", "axum", "0.8.9", "MIT", None),
        ("chevron-right", "tokio", "1.48.0", "MIT", None),
        ("chevron-right", "serde", "1.0.229", "MIT OR Apache-2.0", None),
        ("chevron-right", "reqwest", "0.13.1", "MIT OR Apache-2.0", None),
        ("chevron-right", "rustls", "0.23.35", "Apache-2.0 OR ISC OR MIT", None),
        ("chevron-right", "svelte", "5.43.2", "MIT", None),
    ]
    y = 382
    for ic, name, ver, lic, note in comps:
        h = 92 if note else 40
        s += f'  <line x1="950" y1="{y + h}" x2="1520" y2="{y + h}" stroke="#eef0f4"/>\n'
        s += icon(ic, 958, y + 11, "#475467", 18, "Свернуть" if ic == "chevron-down" else "Показать текст лицензии")
        s += t(986, y + 25, name, "small", weight="650") + t(1196, y + 25, ver, "mono") + t(1306, y + 25, lic, "small")
        if note:
            s += t(986, y + 48, note, "xs muted")
            s += box(986, y + 56, 520, 30, fill="#f8fafc", stroke="#eef0f4", rx=6)
            s += t(998, y + 76, "MIT License · Copyright (c) 2020-2026 Paweł Kuna …", "mono")
        y += h
    s += t(950, y + 22, "и ещё 261 компонент", "xs muted")
    s += f'  <line x1="950" y1="{y + 40}" x2="1520" y2="{y + 40}" stroke="#e4e7ec"/>\n'
    s += t(950, y + 66, "ДОКУМЕНТАЦИЯ", "label muted")
    s += t(950, y + 92, "Руководство и лицензии — встроенные копии, открываются без сети.", "small")
    s += icon("external-link", 950, y + 106, "#3156a3", 18)
    s += t(976, y + 120, "Страница ZDashstitch на GitHub — откроется в браузере, если есть сеть", "small", "#3156a3", weight="650")

    svg = appbar.apply(s + "</svg>\n", active="none", mode="offline", depth_prefix="../../../../", settings_active=True)
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="") as f:
        f.write(svg)
    print("written", OUT)


if __name__ == "__main__":
    build()
