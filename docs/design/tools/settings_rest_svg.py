"""Review mockups 06-03, 06-05, 06-06: settings «Рабочие каталоги», «Безопасность и экспорт», «Диагностика» (P1-09D/G).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/settings_rest_svg.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402
from publication_svg import AMBER, GREEN, NEUTRAL, box, button, checkbox, chip, head, icon, radio, select, t  # noqa: E402

OUT = "docs/design/assets/06-settings/"
NAV = ["Общие", "Внешний вид", "Рабочие каталоги", "Редактор", "Подключения и сертификаты",
       "Безопасность и экспорт", "Диагностика", "О программе и лицензии"]


def nav(selected):
    s = '  <rect x="0" y="72" width="260" height="928" fill="#ffffff" stroke="#e4e7ec"/>\n'
    s += t(28, 116, "НАСТРОЙКИ", "label muted")
    y = 136
    for name in NAV:
        cls = "body" if len(name) <= 22 else "small"
        if name == selected:
            s += box(16, y, 228, 42, fill="#eef4ff", stroke="#eef4ff", rx=10)
            s += t(32, y + 27, name, cls, "#1d3f86", weight="700")
        else:
            s += t(32, y + 27, name, cls)
        y += 46
    s += '  <line x1="20" y1="520" x2="240" y2="520" stroke="#e4e7ec"/>\n'
    s += t(28, 552, "Пароли и токены здесь", "small muted") + t(28, 574, "не хранятся и не показываются.", "small muted")
    return s


def lines(x, y, items, cls="small muted", fill=None, step=22):
    return "".join(t(x, y + i * step, v, cls, fill) for i, v in enumerate(items))


def header(title, subtitle):
    s = "  <!-- Title -->\n" + t(292, 120, title, "title") + t(292, 150, subtitle, "small muted")
    s += box(1224, 96, 320, 40, stroke="#c9d3ea", rx=8) + icon("search", 1238, 107, "#667085", 18) + t(1266, 121, "Поиск настроек", "small muted")
    return s


def write(svg, name, mode="offline"):
    svg = appbar.apply(svg, active=None, mode=mode, depth_prefix="../../../../", settings_active=True)
    os.makedirs(OUT, exist_ok=True)
    with open(OUT + name, "w", encoding="utf-8", newline="") as f:
        f.write(svg)
    print("written", OUT + name)


def toggle(x, y, on):
    fill = "#3156a3" if on else "#ffffff"
    stroke = "#3156a3" if on else "#7a8d9b"
    knob = x + 26 if on else x + 10
    kfill = "#ffffff" if on else "#7a8d9b"
    return (f'  <rect x="{x}" y="{y}" width="40" height="22" rx="11" fill="{fill}" stroke="{stroke}" stroke-width="2"/>\n'
            f'  <circle cx="{knob}" cy="{y + 11}" r="7" fill="{kfill}"/>\n')


# ================================================================== 06-03 storage + move dialog
def storage_move():
    s = head("настройки: рабочие каталоги, перенос хранилища",
             "Светлый макет раздела «Рабочие каталоги» с открытым диалогом переноса корня хранилища: текущий путь, "
             "состав и размеры, свободное место, автосохранение; проверки нового расположения, предупреждение о "
             "несохранённом черновике, выбор между переносом данных и новым пустым хранилищем, прежняя копия не удаляется.",
             ["folder", "copy", "circle-check", "alert-triangle", "info-circle", "search", "chevron-down"])
    s += nav("Рабочие каталоги")
    s += header("Рабочие каталоги", "Где хранятся черновики, копии, снимки, журнал и библиотека.")
    # storage card
    s += box(292, 166, 820, 520)
    s += t(316, 200, "КОРЕНЬ ХРАНИЛИЩА", "label muted") + chip(1000, 182, 88, "Доступно", GREEN)
    s += icon("folder", 316, 214, "#475467", 20) + t(346, 230, "C:\\Users\\admin\\AppData\\Local\\ZDashstitch", "mono")
    s += button(316, 248, 160, "Открыть папку", h=36) + button(486, 248, 180, "Копировать путь", h=36, icon_name="copy")
    s += button(676, 248, 230, "Изменить расположение…", "primary", h=36)
    s += t(316, 320, "СОСТАВ", "label muted")
    parts = [("Черновики", "7", "3,1 МБ"), ("Рабочие копии", "3", "1,2 МБ"), ("Снимки", "9", "4,8 МБ"),
             ("Журнал операций", "14 записей", "0,1 МБ"), ("Библиотека", "6 каталогов, 2 страницы", "2,4 МБ")]
    y = 336
    for name, cnt, size in parts:
        s += f'  <line x1="316" y1="{y + 34}" x2="1088" y2="{y + 34}" stroke="#eef0f4"/>\n'
        s += t(316, y + 24, name, "small", weight="600") + t(560, y + 24, cnt, "small muted") + t(1088, y + 24, size, "small", anchor="end")
        y += 36
    s += t(316, y + 30, "Занято 11,6 МБ · свободно на диске 118 ГБ", "small")
    s += button(316, y + 50, 260, "Очистить неиспользуемое…", h=36)
    s += t(316, y + 112, "Черновики, копии, библиотека и незавершённые операции не удаляются никогда.", "xs muted")
    # autosave card
    s += box(1132, 166, 412, 520)
    s += t(1156, 200, "АВТОСОХРАНЕНИЕ", "label muted")
    s += toggle(1156, 216, True) + t(1208, 233, "Автосохранение", "body", weight="650")
    s += t(1156, 278, "Сохранять не реже чем раз в", "small muted")
    s += select(1156, 288, 200, "30 секунд", h=38)
    s += lines(1156, 356, ["После 2 секунд без правки черновик", "сохраняется сам. Публикацией это не является."], "small")
    s += lines(1156, 420, ["При выключении несохранённые изменения", "пропадут при сбое приложения или компьютера."], "xs muted", step=18)
    # dialog
    s += '  <!-- Dialog -->\n  <rect x="0" y="72" width="1600" height="928" fill="#17213a" fill-opacity=".45"/>\n'
    s += box(420, 120, 760, 820, stroke="#c9d3ea")
    s += t(456, 168, "Перенос хранилища", "title")
    s += t(456, 200, "Новое расположение — выбрано в системном диалоге:", "small muted")
    s += box(456, 212, 688, 40, fill="#f8fafc", stroke="#d0d5dd", rx=8) + icon("folder", 468, 222, "#475467", 20) + t(498, 238, "D:\\ZDashstitch-data", "mono")
    s += t(456, 290, "ПРОВЕРКА НОВОГО РАСПОЛОЖЕНИЯ", "label muted")
    checks = [("circle-check", "#1f6b43", "Локальный диск, не сетевая папка", None),
              ("circle-check", "#1f6b43", "Запись разрешена; символьных ссылок в пути нет", None),
              ("circle-check", "#1f6b43", "Папка пуста", None),
              ("circle-check", "#1f6b43", "Места хватает: нужно 13 МБ, свободно 412 ГБ", None),
              ("circle-check", "#1f6b43", "Незавершённых операций публикации нет", None),
              ("alert-triangle", "#8a4b00", "Не сохранён 1 черновик: «Базы данных: сводная»", "Сохранить все")]
    y = 306
    for ic, color, text, act in checks:
        s += icon(ic, 456, y, color, 18) + t(484, y + 14, text, "small", "#8a4b00" if act else None, weight="650" if act else None)
        if act:
            s += button(984, y - 10, 160, act, h=34)
        y += 34
    s += t(456, y + 22, "ЧТО СДЕЛАТЬ", "label muted")
    y += 38
    s += box(456, y, 688, 82, fill="#eef4ff", stroke="#3156a3", rx=10, sw=2) + radio(478, y + 26, True)
    s += t(498, y + 31, "Перенести данные сюда", "body", weight="700")
    s += lines(498, y + 55, ["Данные копируются, копия проверяется, затем приложение переключается на неё."], "small muted")
    y += 94
    s += box(456, y, 688, 70, stroke="#e4e7ec", rx=10) + radio(478, y + 26, False)
    s += t(498, y + 31, "Начать новое пустое хранилище", "body", weight="650")
    s += t(498, y + 53, "Прежние черновики останутся в прежнем месте.", "small muted")
    y += 88
    s += box(456, y, 688, 66, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
    s += icon("info-circle", 470, y + 14, "#1d3f86", 18)
    s += lines(496, y + 28, ["Прежняя копия остаётся на месте; удалить её можно отдельно после проверки.",
                             "Во время переноса правка черновиков и публикация недоступны."], "small", "#344054")
    s += t(1144, 888, "Сначала сохраните черновики", "xs muted", anchor="end")
    s += button(880, 834, 120, "Отмена") + button(1012, 834, 132, "Перенести", "disabled")
    write(s + "</svg>\n", "06-03_P1-09D_storage-move.svg")


# ================================================================== 06-05 security and export
def security_export():
    s = head("настройки: безопасность и экспорт",
             "Светлый макет раздела «Безопасность и экспорт»: «Завершить сессию», политика экспорта (папка, "
             "существующий файл, вид пакета, доверенный перенос недоступен, проверка чувствительных значений всегда), "
             "подтверждения, которые нельзя отключить, и приватность без телеметрии.",
             ["logout", "lock", "folder", "info-circle", "search", "circle-check"])
    s += nav("Безопасность и экспорт")
    s += header("Безопасность и экспорт", "Локальный сеанс, правила экспорта, подтверждения и приватность.")
    # session
    s += box(292, 166, 820, 132)
    s += t(316, 200, "ЛОКАЛЬНЫЙ СЕАНС", "label muted")
    s += lines(316, 228, ["Завершает сеанс во всех вкладках и стирает секреты Zabbix из памяти.",
                          "Сохранённые черновики остаются на диске."], "small")
    s += button(868, 212, 220, "Завершить сессию", "danger", icon_name="logout")
    s += t(868, 280, "Недоступно во время записи в Zabbix", "xs muted")
    # export
    s += box(292, 314, 820, 486)
    s += t(316, 348, "ЭКСПОРТ", "label muted")
    s += t(316, 380, "Папка для экспорта", "small muted")
    s += radio(328, 404, True) + t(346, 409, "Спрашивать каждый раз", "small", weight="650")
    s += t(560, 409, "диалог откроется в последней папке этого сеанса", "xs muted")
    s += radio(328, 436, False) + t(346, 441, "Всегда эта папка:", "small")
    s += box(480, 422, 420, 36, fill="#f2f4f7", stroke="#e4e7ec", rx=8) + t(494, 445, "не выбрана", "small", "#98a2b3")
    s += button(912, 422, 150, "Выбрать…", h=36, icon_name="folder")
    s += t(316, 494, "Если файл уже есть", "small muted")
    s += radio(328, 518, True) + t(346, 523, "Спросить", "small", weight="650")
    s += radio(500, 518, False) + t(518, 523, "Сохранить под новым именем", "small")
    s += t(316, 552, "Существующий файл никогда не перезаписывается молча.", "xs muted")
    s += t(316, 594, "Вид пакета", "small muted") + t(316, 618, "Переносимый пакет только с данными", "body", weight="650")
    s += t(316, 656, "Доверенный перенос", "small muted")
    s += chip(316, 666, 330, "Недоступен до утверждения формата (O-011)", NEUTRAL)
    s += box(316, 712, 772, 56, fill="#f8fafc", stroke="#eaecf0", rx=8)
    s += icon("lock", 330, 730, "#475467", 18)
    s += t(358, 736, "Чувствительные значения всегда проверяются перед экспортом", "small", weight="650")
    s += t(358, 756, "Нельзя отключить: защищает данные или сервер", "xs muted")
    # confirmations
    s += box(1132, 166, 412, 486)
    s += t(1156, 200, "НЕЛЬЗЯ ОТКЛЮЧИТЬ", "label muted")
    locked = ["Разрешить запись панелей", "Подтвердить запись в Zabbix", "Закрыть без сверки", "Проверка перед экспортом",
              "Перенос и удаление хранилища", "Сброс настроек", "Завершить сессию"]
    y = 222
    for item in locked:
        s += icon("lock", 1156, y, "#475467", 18) + t(1184, y + 14, item, "small")
        y += 34
    s += lines(1156, y + 20, ["Остальные подтверждения (удаление", "страницы, массовые действия) —", "в разделе «Редактор»."], "xs muted", step=18)
    # privacy
    s += box(1132, 668, 412, 132)
    s += t(1156, 702, "ПРИВАТНОСТЬ", "label muted")
    s += icon("circle-check", 1156, 718, "#1f6b43", 18)
    s += lines(1184, 732, ["Телеметрии и отправки данных в сеть нет.", "Приложение обращается только к серверам",
                           "Zabbix из ваших профилей подключений."], "small", step=20)
    write(s + "</svg>\n", "06-05_P1-09G_security-export.svg")


# ================================================================== 06-06 diagnostics + support package
def diagnostics():
    s = head("настройки: диагностика",
             "Светлый макет раздела «Диагностика»: подробность и срок хранения журнала приложения; пакет для поддержки "
             "с предпросмотром состава — что входит, что не входит никогда, адреса серверов только по флажку; пакет "
             "сохраняется локально и никуда не отправляется.",
             ["download", "info-circle", "circle-check", "circle-x", "search", "chevron-down"])
    s += nav("Диагностика")
    s += header("Диагностика", "Журнал приложения и пакет для поддержки. Секретов и полных путей в них нет.")
    s += box(292, 166, 1252, 150)
    s += t(316, 200, "ЖУРНАЛ ПРИЛОЖЕНИЯ", "label muted")
    s += t(316, 230, "Подробность", "small muted")
    s += radio(328, 254, True) + t(346, 259, "Обычная: коды и этапы", "small", weight="650")
    s += radio(600, 254, False) + t(618, 259, "Подробная: также время этапов и счётчики", "small")
    s += t(316, 298, "Хранить", "small muted") + select(390, 278, 150, "7 дней", h=34)
    s += t(560, 300, "Сейчас: 1,8 МБ, 7 файлов. Секреты, тела ответов сервера, содержимое файлов и полные пути не пишутся.", "xs muted")
    # bundle
    s += box(292, 332, 1252, 576)
    s += t(316, 366, "ПАКЕТ ДЛЯ ПОДДЕРЖКИ", "label muted")
    s += t(316, 392, "Создаётся только по вашему действию, сохраняется на этом компьютере и никуда не отправляется.", "small")
    s += t(316, 432, "ВХОДИТ", "label muted")
    inc = ["версия, сборка, ОС и платформа", "проверенная версия Zabbix и адаптер",
           "настройки без пути хранилища (&lt;storage_root&gt;)", "коды ошибок и этапы за 7 дней",
           "состояния операций публикации без имён и ID", "число объектов и размеры хранилища"]
    y = 448
    for item in inc:
        s += icon("circle-check", 316, y, "#1f6b43", 16) + t(340, y + 13, item, "small")
        y += 28
    s += t(760, 432, "НЕ ВХОДИТ НИКОГДА", "label muted")
    exc = ["пароли, токены, сертификаты, cookie сессии", "имена панелей, страниц, узлов сети, пользователей",
           "содержимое черновиков, снимков и файлов", "тела запросов и ответов Zabbix", "полные локальные пути"]
    y = 448
    for item in exc:
        s += icon("circle-x", 760, y, "#a32935", 16) + t(784, y + 13, item, "small")
        y += 28
    s += checkbox(316, 636, False) + t(346, 652, "Включить адреса серверов", "small", weight="650")
    s += t(346, 674, "по умолчанию выключено", "xs muted")
    s += t(316, 716, "ФАЙЛЫ ПАКЕТА · zdashstitch-support-2026-10-25.zip", "label muted")
    files = [("manifest.json", "состав и версия пакета"), ("environment.json", "версия, ОС, Zabbix, адаптер"),
             ("settings.yaml", "настройки, путь заменён"), ("app-log.jsonl", "коды и этапы"),
             ("operations.jsonl", "состояния операций"), ("storage-counts.json", "число объектов и размеры")]
    y = 732
    for i, (fn, desc) in enumerate(files):
        x = 316 if i < 3 else 760
        yy = y + (i % 3) * 28
        s += t(x, yy + 14, fn, "mono") + t(x + 190, yy + 14, desc, "small muted")
    s += button(1244, 846, 276, "Сохранить пакет…", "primary", icon_name="download")
    s += t(316, 880, "Перед сохранением можно открыть любой файл пакета и посмотреть его содержимое.", "xs muted")
    write(s + "</svg>\n", "06-06_P1-09G_diagnostics.svg")


if __name__ == "__main__":
    storage_move()
    security_export()
    diagnostics()
