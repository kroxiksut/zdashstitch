"""Review mockups 13-01..13-04: «Экспорт» (P1-07: export, de-personalisation, workcopy conversion).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/export_svg.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bindings_svg import steps  # noqa: E402
from import_library_svg import X0, page_title, pills, row_line, table_head, wrap, write  # noqa: E402
from publication_svg import (  # noqa: E402
    AMBER, BLUE, GREEN, NEUTRAL, RED, box, button, checkbox, chip, head, icon, radio, select, t,
)

STEPS = ["Цель и состав", "Проверка значений", "Сохранение"]
OUT = "13-export/"


def crumb(text):
    return "  <!-- Title -->\n" + t(X0, 92, text, "xs muted")


def bottom_bar(left, right, y=852):
    s = ""
    x = X0
    for text, kind, w in left:
        s += button(x, y, w, text, kind)
        x += w + 12
    x = 1552
    for text, kind, w in reversed(right):
        x -= w
        s += button(x, y, w, text, kind)
        x -= 12
    return s


# ================================================================== 13-01 goal, mode, contents
def goal_and_contents():
    s = head("экспорт: цель и состав", "Светлый макет первого шага экспорта черновика панели: цель передачи (себе или коллегам, "
             "наружу с обезличиванием, доверенный перенос с привязками недоступен до O-011), состав пакета, что "
             "исключено всегда, заявленная совместимость и оставшийся риск.",
             ["circle-check", "lock", "alert-triangle", "info-circle", "chevron-down", "ban"])
    s += crumb("Черновики  /  Базы данных: сводная  /  Экспорт")
    s += page_title("Экспорт «Базы данных: сводная»", "Панель · Локальная · 3 страницы, 19 виджетов. Черновик при экспорте не меняется.")
    s += steps(STEPS, 1, x=X0, y=172)
    # goals
    s += box(X0, 222, 740, 610)
    s += t(72, 256, "ДЛЯ ЧЕГО ЭКСПОРТ", "label muted")
    goals = [
        (False, "Себе или коллегам в организации", "Обычный переносимый пакет. Имена узлов сети, ключи и тексты",
         "остаются как есть — передавайте по правилам организации.", None),
        (True, "Наружу: в Git, на форум, другой команде", "Обезличенный пакет: найденные имена и адреса заменяются",
         "на псевдонимы или привязки к ролям; вы проверяете каждую замену.", None),
        (False, "На свою доверенную машину вместе с привязками", "Доверенный перенос: формат защиты ещё не утверждён (O-011).",
         "Сейчас — переносимый пакет и повторная привязка на другой машине.", "lock"),
    ]
    y = 272
    for sel, name, l1, l2, locked in goals:
        disabled = locked is not None
        s += box(72, y, 692, 104, fill="#eef4ff" if sel else ("#f8fafc" if disabled else "#ffffff"),
                 stroke="#3156a3" if sel else "#e4e7ec", rx=12, sw=2 if sel else None)
        s += radio(98, y + 30, sel, disabled=disabled)
        s += t(120, y + 36, name, "body", "#98a2b3" if disabled else None, weight="700")
        if locked:
            s += icon("lock", 732, y + 18, "#98a2b3", 18, "Недоступно до решения O-011")
        s += t(120, y + 62, l1, "small muted") + t(120, y + 84, l2, "small muted")
        y += 116
    s += t(72, y + 22, "ЗАЯВЛЕННАЯ СОВМЕСТИМОСТЬ", "label muted")
    s += select(72, y + 36, 300, "Zabbix 7.4.0+ (вычислено)")
    s += t(392, y + 62, "Вычислено по типам виджетов; можно сузить вручную.", "small muted")
    # contents
    s += box(808, 222, 744, 610)
    s += t(832, 256, "ЧТО ВОЙДЁТ", "label muted")
    inc = [("project.yaml", "панель: 3 страницы, 19 виджетов, роли «PostgreSQL», «ClickHouse», «Linux»"),
           ("metrics/postgresql-agent2.yaml", "каталог «PostgreSQL by Zabbix agent 2»"),
           ("metrics/clickhouse-http.yaml", "каталог «ClickHouse by HTTP»"),
           ("metrics/linux-agent.yaml", "каталог «Linux by Zabbix agent»")]
    y = 272
    for f, d in inc:
        s += icon("circle-check", 832, y + 4, "#1f6b43", 18) + t(860, y + 18, f, "mono") + t(860, y + 40, d, "xs muted")
        y += 54
    s += t(832, y + 16, "НЕ ВХОДИТ НИКОГДА", "label muted")
    exc = ["пароли, токены и данные входа", "привязки к узлам сети этого компьютера и ID Zabbix",
           "владелец и доступ панелей, снимки, журнал операций", "исходные файлы экспорта шаблонов Zabbix"]
    y += 30
    for e in exc:
        s += icon("ban", 832, y + 2, "#98a2b3", 16) + t(858, y + 15, e, "small")
        y += 28
    s += box(832, y + 14, 696, 108, fill="#fff8ed", stroke="#f3d3a6", rx=10)
    s += icon("alert-triangle", 846, y + 28, "#8a4b00", 18)
    s += t(872, y + 42, "Остаётся риск", "small", "#8a4b00", weight="700")
    s += wrap(872, y + 66, ["Имена виджетов, страниц, шаблоны узлов в графиках, ключи элементов", "данных и макросы могут выдать вашу инфраструктуру. На следующем шаге",
                            "они будут найдены и показаны; гарантии обезличивания нет."], "small", "#5c3a00")
    s += bottom_bar([("Отмена", "secondary", 120)], [("Далее: проверка значений", "primary", 260)])
    write(s + "</svg>\n", OUT + "13-01_P1-07A_export-goal.svg", "drafts", mode="offline")


# ================================================================== 13-02 sensitive values and replacements
def sensitive_review():
    s = head("экспорт: проверка значений", "Светлый макет проверки чувствительных значений перед обезличенным экспортом: найденные "
             "значения по классам с местом и уверенностью, замена выбранного значения на привязку к роли или псевдоним "
             "с предпросмотром «было — станет» во всех местах, повторная проверка и подтверждение остаточного риска.",
             ["circle-check", "alert-triangle", "circle-dashed", "circle-x", "info-circle", "check", "refresh"])
    s += crumb("Черновики  /  Базы данных: сводная  /  Экспорт")
    s += page_title("Экспорт «Базы данных: сводная»", "Обезличенный пакет. Значения проверяются на этом компьютере и никуда не отправляются.")
    s += steps(STEPS, 2, x=X0, y=172)
    s += box(X0, 222, 900, 610)
    for p in pills(72, 238, [("Все 7", 78, True), ("Точно 3", 92, False), ("Возможно 3", 116, False), ("Не проверено 1", 140, False)]):
        s += p
    s += table_head(72, 284, 852, [(104, "ЗНАЧЕНИЕ"), (300, "ВИД"), (470, "ГДЕ"), (720, "РЕШЕНИЕ")])
    rows = [
        ("circle-check", "#1f6b43", "pg-a-01", "имя узла сети · точно", "«Connections», ряд 1 · ещё 2", "роль «PostgreSQL», слот 1", True),
        ("circle-check", "#1f6b43", "ch-a-01", "имя узла сети · точно", "«Queries per second» · ещё 3", "роль «ClickHouse», слот 1", False),
        ("alert-triangle", "#8a4b00", "grafana.corp…", "адрес (URL) · точно", "«Ссылка на Grafana» · URL", "убрать виджет", False),
        ("alert-triangle", "#8a4b00", "pg-a-*", "маска узлов · возможно", "«Disk I/O», ряд 2", "псевдоним «db-*»", False),
        ("alert-triangle", "#8a4b00", "Prod", "окружение · возможно", "имя «Prod: replication lag»", "оставить", False),
        ("alert-triangle", "#8a4b00", "proddb", "база в ключе · возможно", "pgsql.db.size[proddb]", "оставить (ключ)", False),
        ("circle-dashed", "#98a2b3", "ds.0.vendor_hint", "неизвестное поле", "«Queries per second»", "показано, не изменяется", False),
    ]
    y = 324
    for ic, color, val, kind, where, dec, sel in rows:
        if sel:
            s += box(72, y, 852, 50, fill="#eef4ff", stroke="#3156a3", rx=8, sw=2)
        else:
            s += row_line(72, y + 50, 852)
        s += icon(ic, 82, y + 16, color, 18) + t(104, y + 31, val, "mono") + t(300, y + 31, kind, "small")
        s += t(470, y + 31, where, "small muted") + t(720, y + 31, dec, "small", weight="600")
        y += 56
    s += t(72, y + 22, "Значения в отчёте и журнале не сохраняются целиком; найденное не отправляется в сеть.", "xs muted")
    # replacement panel
    s += box(968, 222, 584, 610)
    s += t(992, 256, "ЗАМЕНА «pg-a-01»", "label muted")
    s += radio(1002, 282, True) + t(1020, 288, "Привязка к роли", "body", weight="650")
    s += t(1020, 310, "ряд будет искать узлы роли — лучший вариант для узла сети", "xs muted")
    s += select(1020, 322, 508, "Роль «PostgreSQL», слот 1")
    s += radio(1002, 390, False) + t(1020, 396, "Псевдоним", "body")
    s += box(1144, 376, 384, 36, stroke="#7a8d9b", rx=8) + t(1156, 400, "host-1", "small muted")
    s += radio(1002, 434, False) + t(1020, 440, "Оставить как есть", "body")
    s += t(992, 484, "БЫЛО — СТАНЕТ · 3 МЕСТА", "label muted")
    places = [("«Connections» · ds.0.hosts.0", "pg-a-01", "роль PostgreSQL · слот 1"),
              ("«TPS» · ds.0.hosts.0", "pg-a-01", "роль PostgreSQL · слот 1"),
              ("«Cache hit ratio» · ds.1.hosts.0", "pg-a-01", "роль PostgreSQL · слот 1")]
    y = 500
    for where, before, after in places:
        s += box(992, y, 536, 62, fill="#f8fafc", stroke="#eef0f4", rx=8)
        s += t(1006, y + 22, where, "xs muted")
        s += t(1006, y + 46, before, "mono") + t(1110, y + 46, "→", "small muted") + t(1136, y + 46, after, "small", weight="600")
        y += 68
    s += button(992, y + 6, 260, "Применить ко всем 3", "primary", h=38) + button(1264, y + 6, 264, "Проверить пакет снова", h=38, icon_name="refresh")
    s += t(992, y + 72, "Черновик не меняется: замены действуют только на пакет.", "xs muted")
    # acknowledgement bar
    s += checkbox(X0, 860, False)
    s += t(X0 + 30, 876, "Понимаю: пакет не гарантированно обезличен; передаю по правилам организации.", "small", weight="650")
    s += t(X0 + 30, 896, "Решение принято по всем 7 значениям; 2 возможных оставлены как есть сознательно.", "xs muted")
    s += button(1184, 852, 120, "Назад") + button(1316, 852, 236, "Далее: сохранение", "disabled")
    write(s + "</svg>\n", OUT + "13-02_P1-07C_sensitive-review.svg", "drafts", mode="offline")


# ================================================================== 13-03 progress and result
def export_result():
    s = head("экспорт: результат", "Светлый макет результата экспорта: этапы проверки, преобразования, сборки и сверки пройдены, "
             "имя и путь файла, контрольная сумма, режим и принятые предупреждения, сводка без значений; пакет "
             "никуда не отправлялся.",
             ["circle-check", "alert-triangle", "copy", "info-circle"])
    s += crumb("Черновики  /  Базы данных: сводная  /  Экспорт")
    s += page_title("Экспорт «Базы данных: сводная»", "Обезличенный пакет сохранён.")
    s += steps(STEPS, 3, x=X0, y=172)
    s += box(X0, 222, 740, 610)
    s += t(72, 256, "ЭТАПЫ", "label muted")
    stages = [("Проверка значений", "7 найдено, 4 заменено, 2 оставлено, 1 неизвестное поле"),
              ("Преобразование", "замены применены к копии; черновик не изменён"),
              ("Проверка формата", "project/v2 и 3 каталога метрик — без ошибок"),
              ("Сборка пакета", "4 файла, сжатие и пути по правилам пакета"),
              ("Сверка", "пакет прочитан заново и совпал с проверенным содержимым")]
    y = 272
    for name, line in stages:
        s += box(72, y, 692, 60, stroke="#eef0f4", rx=10)
        s += icon("circle-check", 86, y + 18, "#1f6b43", 22) + t(122, y + 26, name, "body", weight="700") + t(122, y + 48, line, "small muted")
        y += 68
    s += t(72, y + 20, "Отмена была доступна до записи файла; временный файл удаляется при любой ошибке.", "xs muted")
    s += box(808, 222, 744, 610)
    s += box(832, 246, 696, 76, fill="#ecfdf3", stroke="#a6dcbc", rx=12)
    s += icon("circle-check", 848, 266, "#1f6b43", 24)
    s += t(884, 280, "Сохранено: bazy-dannyh-svodnaya.zip", "h3", "#1f6b43")
    s += t(884, 304, "Обезличенный пакет · 4 файла · 38 КБ", "small", "#14432a")
    rows = [("Папка", "D:\\Обмен\\ZDashstitch"), ("SHA-256", "4f1c…9a07"), ("Совместимость", "Zabbix 7.4.0+"),
            ("Режим", "обезличенный, 4 замены")]
    y = 352
    for k, v in rows:
        s += t(832, y, k, "small muted") + t(990, y, v, "mono" if k in ("Папка", "SHA-256") else "small", weight=None if k in ("Папка", "SHA-256") else "600")
        y += 30
    s += box(832, y + 4, 696, 86, fill="#fff8ed", stroke="#f3d3a6", rx=10)
    s += icon("alert-triangle", 846, y + 18, "#8a4b00", 18)
    s += t(872, y + 32, "Принятые предупреждения: 2", "small", "#8a4b00", weight="700")
    s += wrap(872, y + 56, ["«Prod» в имени виджета и «proddb» в ключе элемента данных оставлены."], "small", "#5c3a00")
    y += 112
    s += t(832, y + 14, "Пакет не отправлялся в сеть и не открывался автоматически.", "small muted")
    s += t(832, y + 38, "Отчёт содержит состав, режим, контрольные суммы и счётчики — без значений.", "small muted")
    s += button(832, 760, 210, "Открыть папку") + button(1054, 760, 230, "Копировать отчёт", icon_name="copy")
    s += bottom_bar([], [("Ещё экспорт", "secondary", 160), ("К черновику", "primary", 180)])
    write(s + "</svg>\n", OUT + "13-03_P1-07G_export-result.svg", "drafts", mode="offline")


# ================================================================== 13-04 workcopy -> portable draft
def conversion():
    s = head("сделать копию с сервера переносимой", "Светлый макет преобразования копии с сервера в переносимый черновик: "
             "выбор корня (вся панель или одна страница), имя нового черновика, отчёт о том, что становится ролями, "
             "метриками каталогов, «нужна привязка» или переносится как есть, и что не переносится; копия не меняется.",
             ["circle-check", "alert-triangle", "circle-dashed", "ban", "info-circle", "chevron-down"])
    s += crumb("Черновики  /  Базы данных: Prod  /  Сделать переносимой")
    s += page_title("Сделать «Базы данных: Prod» переносимой", "Создаётся новый локальный черновик; копия с сервера и её привязки остаются как есть.")
    s += box(X0, 176, 520, 656)
    s += t(72, 210, "ЧТО ПРЕОБРАЗОВАТЬ", "label muted")
    s += box(72, 226, 472, 58, fill="#eef4ff", stroke="#3156a3", rx=10, sw=2) + radio(96, 255, True)
    s += t(116, 261, "Всю панель: 3 страницы, 19 виджетов", "body", weight="650")
    s += box(72, 292, 472, 58, stroke="#e4e7ec", rx=10) + radio(96, 321, False) + t(116, 327, "Одну страницу", "body")
    s += t(72, 384, "ИМЯ НОВОГО ЧЕРНОВИКА", "label muted")
    s += box(72, 398, 472, 40, stroke="#3156a3", rx=8, sw=2) + t(86, 424, "Базы данных: шаблон", "body")
    s += t(72, 476, "ПРОИСХОЖДЕНИЕ", "label muted")
    s += wrap(72, 502, ["Новый черновик помнит только, что он сделан из копии", "с сервера — без адреса сервера, ID и имени", "подключения. Цель публикации не задаётся."], "small")
    s += box(72, 586, 472, 112, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
    s += icon("info-circle", 86, 600, "#1d3f86", 18)
    s += wrap(112, 614, ["Экспортировать новый черновик можно сразу;", "перед публикацией узлы сети снова подбираются", "в «Привязках» по ролям.", "Метрики без каталога — после импорта шаблона."], "small", "#344054")
    # report
    s += box(588, 176, 964, 656)
    s += t(612, 210, "ОТЧЁТ ПРЕОБРАЗОВАНИЯ", "label muted")
    s += table_head(612, 226, 916, [(648, "ЧТО"), (880, "СТАНЕТ"), (1240, "СКОЛЬКО")])
    rows = [
        ("circle-check", "#1f6b43", "Узлы сети", "роли «PostgreSQL» (2 слота), «ClickHouse» (2),", "«Zabbix server» (1)", "5 узлов"),
        ("circle-check", "#1f6b43", "Элементы данных", "метрики каталогов: по ключу 18, по имени 20", None, "38 из 42 ссылок"),
        ("alert-triangle", "#8a4b00", "Элементы LLD", "метрика-прототип; экземпляр — при привязке", None, "3"),
        ("alert-triangle", "#8a4b00", "Без каталога", "«нужна привязка»: импортируйте шаблон", "«Zabbix server health»", "1"),
        ("alert-triangle", "#8a4b00", "Классические графики", "«нужна привязка» к графику по имени", None, "2"),
        ("circle-dashed", "#98a2b3", "Маски узлов", "строки как есть; проверка при экспорте", None, "2"),
        ("circle-dashed", "#98a2b3", "Неизвестные поля и типы", "как есть, с пометкой, без изменения (O-034)", None, "4 поля"),
        ("circle-check", "#1f6b43", "Связи виджетов", "сохраняются", None, "6"),
        ("ban", "#98a2b3", "Владелец и доступ, снимки", "не переносятся: остаются у копии", None, "—"),
    ]
    y = 266
    for ic, color, what, l1, l2, n in rows:
        h = 44 if not l2 else 62
        s += row_line(612, y + h, 916)
        s += icon(ic, 622, y + 13, color, 18) + t(648, y + 28, what, "small", weight="650")
        s += t(880, y + 28, l1, "small")
        if l2:
            s += t(880, y + 48, l2, "small")
        s += t(1240, y + 28, n, "small", weight="600")
        y += h + 4
    s += t(612, y + 22, "Ничего не теряется молча: всё, что не стало ролью или метрикой, помечено и видно в «Проверках».", "xs muted")
    s += bottom_bar([("Отмена", "secondary", 120)], [("Создать переносимый черновик", "primary", 300)])
    write(s + "</svg>\n", OUT + "13-04_P1-07I_workcopy-convert.svg", "drafts", mode="offline")


if __name__ == "__main__":
    goal_and_contents()
    sensitive_review()
    export_result()
    conversion()
