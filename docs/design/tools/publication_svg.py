"""Review mockups 11-01..11-08: «Публикация» (P1-08).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/publication_svg.py
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402

OUT = "docs/design/assets/11-publication/"
ICON_DIR = "assets/icons/tabler/outline/"
SYM = ('viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" '
       'stroke-linecap="round" stroke-linejoin="round"')
STYLE = '''    <style>
      text{font-family:system-ui,-apple-system,"Segoe UI",sans-serif}
      .muted{fill:#667085}.white{fill:#ffffff}.small{font-size:14px}.xs{font-size:12px}.label{font-size:13px;font-weight:700;letter-spacing:.08em}
      .title{font-size:24px;font-weight:750}.h3{font-size:17px;font-weight:700}.body{font-size:16px}.button{font-size:15px;font-weight:700}
      .nav{font-size:17px}.badge{font-size:13px;font-weight:650}.row{font-size:15px;font-weight:650}
      .mono{font-family:ui-monospace,"Cascadia Mono",Consolas,monospace;font-size:13px;fill:#344054}
    </style>
'''
RW = ("Чтение-запись · Лаборатория 7.4", "#fff4e5", "#8a4b00", 280)
GREEN = ("#ecfdf3", "#1f6b43")
AMBER = ("#fff4e5", "#8a4b00")
RED = ("#fef3f2", "#a32935")
NEUTRAL = ("#f2f4f7", "#344054")
BLUE = ("#eef4ff", "#1d3f86")


def symbol(name):
    """Inline the vendored upstream path data unchanged."""
    raw = open(ICON_DIR + name + ".svg", encoding="utf-8").read()
    paths = "".join(p.replace(" />", "/>") for p in re.findall(r"<path[^>]*/>", raw))
    return f'    <symbol id="ti-{name}" {SYM}>{paths}</symbol>\n'


def head(title, desc, icons):
    syms = "".join(symbol(n) for n in icons)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1000" viewBox="0 0 1600 1000" fill="#17213a" role="img" aria-labelledby="title desc">\n'
            f'  <title id="title">ZDashstitch — {title}</title>\n  <desc id="desc">{desc}</desc>\n  <defs>\n'
            f'    <!-- Tabler Icons outline v3.49.0 (MIT), see assets/icons/THIRD_PARTY.md -->\n{syms}{STYLE}  </defs>\n'
            f'  <rect width="1600" height="1000" fill="#f5f7fb"/>\n\n  <!-- App bar -->\n  <!-- Workspace -->\n')


def t(x, y, s, cls="small", fill=None, anchor=None, weight=None):
    extra = ""
    if fill:
        extra += f' fill="{fill}"'
    if anchor:
        extra += f' text-anchor="{anchor}"'
    if weight:
        extra += f' font-weight="{weight}"'
    return f'  <text x="{x}" y="{y}" class="{cls}"{extra}>{s}</text>\n'


def box(x, y, w, h, fill="#ffffff", stroke="#d0d5dd", rx=16, sw=None):
    extra = f' stroke-width="{sw}"' if sw else ""
    return f'  <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{fill}" stroke="{stroke}"{extra}/>\n'


def icon(name, x, y, color, size=20, tip=None):
    if tip:
        return f'  <use href="#ti-{name}" x="{x}" y="{y}" width="{size}" height="{size}" color="{color}"><title>{tip}</title></use>\n'
    return f'  <use href="#ti-{name}" x="{x}" y="{y}" width="{size}" height="{size}" color="{color}"/>\n'


def chip(x, y, w, text, tone):
    fill, color = tone
    return (f'  <rect x="{x}" y="{y}" width="{w}" height="26" rx="13" fill="{fill}"/>\n'
            f'  <text x="{x + w / 2:.0f}" y="{y + 18}" class="badge" text-anchor="middle" fill="{color}">{text}</text>\n')


def button(x, y, w, text, kind="secondary", h=42, icon_name=None):
    if kind == "primary":
        bg, stroke, cls, fill = "#3156a3", "#3156a3", "button white", None
    elif kind == "disabled":
        bg, stroke, cls, fill = "#f2f4f7", "#e4e7ec", "button", "#98a2b3"
    elif kind == "danger":
        bg, stroke, cls, fill = "#ffffff", "#c9d3ea", "button", "#a32935"
    else:
        bg, stroke, cls, fill = "#ffffff", "#c9d3ea", "button", None
    s = f'  <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="10" fill="{bg}" stroke="{stroke}"/>\n'
    cx = x + w / 2
    if icon_name:
        color = "#ffffff" if kind == "primary" else (fill or "#17213a")
        s += icon(icon_name, x + 16, y + h / 2 - 9, color, 18)
        cx += 12
    s += t(f"{cx:.0f}", f"{y + h / 2 + 5:.0f}", text, cls, fill, "middle")
    return s


def radio(x, y, on, disabled=False):
    stroke = "#3156a3" if on else ("#d0d5dd" if disabled else "#7a8d9b")
    s = f'  <circle cx="{x}" cy="{y}" r="9" fill="#ffffff" stroke="{stroke}" stroke-width="2"/>\n'
    if on:
        s += f'  <circle cx="{x}" cy="{y}" r="4.5" fill="#3156a3"/>\n'
    return s


def checkbox(x, y, on):
    if on:
        return (f'  <rect x="{x}" y="{y}" width="20" height="20" rx="4" fill="#3156a3"/>\n'
                + icon("check", x + 3, y + 3, "#ffffff", 14))
    return f'  <rect x="{x}" y="{y}" width="20" height="20" rx="4" fill="#ffffff" stroke="#7a8d9b" stroke-width="2"/>\n'


def select(x, y, w, value, h=40):
    return (box(x, y, w, h, stroke="#7a8d9b", rx=8) + t(x + 14, y + 26, value, "body")
            + icon("chevron-down", x + w - 30, y + 11, "#475467", 18))


def sidebar(origin_lines):
    s = '''  <rect x="0" y="72" width="260" height="928" fill="#ffffff" stroke="#e4e7ec"/>
  <text x="28" y="116" class="label muted">ПАНЕЛЬ</text>
  <text x="36" y="164" class="nav">Обзор</text>
  <text x="36" y="212" class="nav">Структура</text>
  <text x="36" y="260" class="nav">Привязки</text>
  <text x="36" y="308" class="nav">Проверки</text>
  <rect x="16" y="328" width="228" height="46" rx="10" fill="#eef4ff"/>
  <text x="36" y="358" class="nav" font-weight="700" fill="#1d3f86">Публикация</text>
  <line x1="20" y1="392" x2="240" y2="392" stroke="#e4e7ec"/>
  <text x="28" y="428" class="label muted">ПРОИСХОЖДЕНИЕ</text>
'''
    y = 458
    for i, line in enumerate(origin_lines):
        s += t(28, y, line, "body" if i == 0 else "small", None if i == 0 else "#667085", weight="650" if i == 0 else None)
        y += 26 if i == 0 else 24
    return s


STEPS = ["Назначение", "Проверки", "Изменения", "Подтверждение", "Запись и проверка"]


def steps_bar(current, done=(), error=None):
    """Linear publication steps under the title (P1-08A)."""
    s = "  <!-- Publication steps -->\n"
    x = 292
    widths = [150, 136, 142, 176, 206]
    for i, (name, w) in enumerate(zip(STEPS, widths), start=1):
        if i == current:
            fill, stroke, color, weight = "#eef4ff", "#3156a3", "#1d3f86", "700"
        elif i == error:
            fill, stroke, color, weight = "#fef3f2", "#f2b8b5", "#a32935", "650"
        elif i in done:
            fill, stroke, color, weight = "#ffffff", "#d0d5dd", "#1f6b43", "600"
        else:
            fill, stroke, color, weight = "#ffffff", "#e4e7ec", "#667085", "500"
        s += f'  <rect x="{x}" y="146" width="{w}" height="36" rx="18" fill="{fill}" stroke="{stroke}"/>\n'
        if i in done and i != current:
            s += icon("circle-check", x + 12, 155, "#1f6b43", 18)
        elif i == error:
            s += icon("circle-x", x + 12, 155, "#a32935", 18)
        else:
            s += f'  <text x="{x + 21}" y="169" class="small" text-anchor="middle" fill="{color}" font-weight="700">{i}</text>\n'
        s += t(x + 38, 169, name, "small", color, weight=weight)
        x += w + 10
    return s


def context_card(cells, result, y=196, h=112):
    """Operation context: intent, source/base, connection, scope + expected result."""
    s = "  <!-- Operation context (P1-08A) -->\n" + box(292, y, 1252, h)
    x = 316
    for label, value, sub, w in cells:
        s += t(x, y + 28, label, "label muted")
        s += t(x, y + 52, value, "body", weight="650")
        if sub:
            s += t(x, y + 71, sub, "xs muted")
        x += w
    s += f'  <line x1="316" y1="{y + h - 32}" x2="1520" y2="{y + h - 32}" stroke="#eef0f4"/>\n'
    s += t(316, y + h - 10, result, "small")
    return s


def action_bar(left, right):
    s = "  <!-- Persistent action bar -->\n"
    s += '  <rect x="260" y="924" width="1340" height="76" fill="#ffffff" stroke="#e4e7ec"/>\n'
    x = 292
    for text, kind, w, ic in left:
        s += button(x, 941, w, text, kind, icon_name=ic)
        x += w + 12
    x = 1544
    for text, kind, w, ic in reversed(right):
        x -= w
        s += button(x, 941, w, text, kind, icon_name=ic)
        x -= 12
    return s


def finish(svg, name, breadcrumb, chips, mode=RW):
    svg = appbar.apply(svg, active="drafts", mode=mode, depth_prefix="../../../../",
                       breadcrumb=breadcrumb, chips=chips)
    path = OUT + name
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(svg)
    print("written", path)


def title(text):
    return "  <!-- Title -->\n" + t(292, 124, text, "title")


SAVED = ("Сохранена локально", "#ecfdf3", "#1f6b43", 170)
ORIGIN_PROD = ["Копия с сервера", "«Базы данных: Prod»", "Лаборатория 7.4", "снимок 25.10.2026, 14:02", "3 страницы · 19 виджетов"]
CRUMB_PROD = "Черновики  /  Базы данных: Prod  /  Публикация"
CTX_REPLACE = [
    ("ОПЕРАЦИЯ", "Заменить узел сети", "pg-a-01 → pg-b-01", 250),
    ("ИСТОЧНИК (БАЗА)", "«Базы данных: Prod»", "снимок 25.10.2026, 14:02", 290),
    ("ЦЕЛЬ", "Та же панель на сервере", "выбрана на шаге «Назначение»", 290),
    ("ПОДКЛЮЧЕНИЕ", "Лаборатория 7.4", "Zabbix 7.4.16 · чтение-запись", 230),
    ("ОБЛАСТЬ", "12 из 19 виджетов", "3 страницы", 180),
]


# ------------------------------------------------------------------ 11-01 target + access (Super admin, Clone)
def screen_target_superadmin():
    s = head("публикация: назначение и доступ, Super admin",
             "Светлый макет шага «Назначение» при клонировании панели учётной записью Super admin: контекст операции, "
             "единственное допустимое действие «Создать новую панель», имя новой панели, выбор владельца, общий доступ "
             "с копией доступа исходной панели и списком получателей.",
             ["circle-check", "chevron-down", "check", "plus", "info-circle", "lock", "users", "x"])
    s += sidebar(ORIGIN_PROD)
    s += title("Предпросмотр и публикация")
    s += steps_bar(1)
    s += context_card([
        ("ОПЕРАЦИЯ", "Клонировать панель", "узлы сети группы Prod → Pilot", 250),
        ("ИСТОЧНИК (БАЗА)", "«Базы данных: Prod»", "снимок 25.10.2026, 14:02", 290),
        ("ЦЕЛЬ", "Новая панель", "в том же подключении", 290),
        ("ПОДКЛЮЧЕНИЕ", "Лаборатория 7.4", "Zabbix 7.4.16 · чтение-запись", 230),
        ("ОБЛАСТЬ", "Вся панель", "3 страницы · 19 виджетов", 180),
    ], "Ожидаемый результат: новая панель «Базы данных: Pilot» с узлами сети группы Pilot. Исходная панель не изменяется.")
    # left: action and name
    s += box(292, 324, 760, 584)
    s += t(316, 358, "ДЕЙСТВИЕ", "label muted")
    s += box(316, 374, 712, 76, fill="#eef4ff", stroke="#3156a3", rx=12, sw=2)
    s += radio(342, 400, True)
    s += t(364, 406, "Создать новую панель", "body", weight="700")
    s += t(364, 430, "Новая глобальная панель в этом подключении. Исходная панель не изменяется.", "small muted")
    s += box(316, 462, 712, 76, fill="#ffffff", stroke="#e4e7ec", rx=12)
    s += radio(342, 488, False, disabled=True)
    s += t(364, 494, "Обновить существующую панель", "body", "#98a2b3", weight="700")
    s += t(364, 518, "Недоступно для клонирования: исходная панель остаётся как есть.", "small muted")
    s += t(316, 580, "ИМЯ НОВОЙ ПАНЕЛИ", "label muted")
    s += box(316, 594, 440, 40, stroke="#3156a3", rx=8, sw=2) + t(330, 620, "Базы данных: Pilot", "body")
    s += icon("circle-check", 770, 604, "#1f6b43", 20) + t(798, 619, "Среди видимых панелей свободно", "small", "#1f6b43")
    s += t(316, 662, "Имя панели уникально на всём сервере, включая панели, которые вам не видны.", "small muted")
    s += t(316, 684, "Окончательно это проверяет сервер при записи.", "small muted")
    s += t(316, 732, "ПОДКЛЮЧЕНИЕ", "label muted")
    s += t(316, 760, "Лаборатория 7.4 — то же, что у исходной панели", "body")
    s += t(316, 784, "Клонирование выполняется внутри одного подключения.", "small muted")
    s += t(316, 832, "НЕ ИЗМЕНЯЕТСЯ", "label muted")
    s += t(316, 860, "Исходная панель «Базы данных: Prod», её владелец и доступ.", "body")
    s += t(316, 884, "Узлы сети, шаблоны и элементы данных не изменяются никогда.", "small muted")
    # right: owner and access
    s += box(1072, 324, 472, 584)
    s += t(1096, 358, "ВЛАДЕЛЕЦ И ДОСТУП", "label muted")
    s += t(1096, 390, "Владелец", "small muted")
    s += select(1096, 400, 424, "dash-admin (вы)")
    s += t(1096, 462, "Выбор владельца доступен только Super admin.", "xs muted")
    # private/shared segmented
    s += t(1096, 498, "Тип доступа", "small muted")
    s += box(1096, 508, 212, 38, fill="#ffffff", stroke="#8f9da8", rx=8)
    s += icon("lock", 1128, 518, "#475467", 18) + t(1154, 533, "Личная", "button")
    s += box(1308, 508, 212, 38, fill="#243746", stroke="#243746", rx=8)
    s += icon("users", 1340, 518, "#ffffff", 18) + t(1366, 533, "Общая", "button white")
    s += checkbox(1096, 566, True) + t(1126, 582, "Скопировать доступ исходной панели", "body")
    s += t(1126, 604, "Super admin видит весь список доступа исходной панели.", "xs muted")
    # recipients
    rows = [("users", "Группа «DB admins»", "Чтение-запись"),
            ("users", "Группа «NOC»", "Только чтение"),
            ("users", "Пользователь «oncall»", "Только чтение")]
    y = 624
    for ic, name, perm in rows:
        s += box(1096, y, 424, 44, fill="#ffffff", stroke="#eaecf0", rx=8)
        s += icon(ic, 1108, y + 12, "#475467", 18)
        s += t(1134, y + 27, name, "small", weight="600")
        s += box(1328, y + 6, 150, 32, stroke="#c9d3ea", rx=6) + t(1340, y + 27, perm, "small")
        s += icon("chevron-down", 1454, y + 14, "#475467", 14)
        s += icon("x", 1490, y + 13, "#667085", 18, "Убрать получателя")
        y += 52
    s += icon("plus", 1096, y + 6, "#3156a3", 18) + t(1122, y + 20, "Добавить пользователя или группу", "small", "#3156a3", weight="650")
    s += icon("info-circle", 1096, y + 42, "#667085", 18)
    s += t(1122, y + 56, "Получатели — только из видимых учётной записи", "xs muted")
    s += t(1122, y + 72, "подключения (user.get, usergroup.get).", "xs muted")
    s += action_bar([("Назад к привязкам", "secondary", 190, None)],
                    [("Сохранить черновик", "secondary", 200, None), ("Далее: проверки", "primary", 190, None)])
    finish(s + "</svg>\n", "11-01_P1-08E_target-access-superadmin.svg", CRUMB_PROD, [SAVED])


# ------------------------------------------------------------------ 11-02 target + access (Admin, update existing)
def screen_target_admin():
    s = head("публикация: назначение и доступ, Admin",
             "Светлый макет шага «Назначение» при замене узла сети учётной записью Admin: явный выбор между копией и "
             "обновлением исходной панели (обновление не выбрано по умолчанию), список того, что не меняется; у существующей "
             "панели можно сменить только тип доступа, список получателей — видимая часть только для просмотра.",
             ["alert-triangle", "info-circle", "users", "lock"])
    s += sidebar(ORIGIN_PROD)
    s += title("Предпросмотр и публикация")
    s += steps_bar(1)
    s += context_card(CTX_REPLACE, "Ожидаемый результат: в 12 виджетах узел сети pg-a-01 заменяется на pg-b-01. Остальное содержимое панели не меняется.")
    s += box(292, 324, 760, 584)
    s += t(316, 358, "ДЕЙСТВИЕ", "label muted")
    s += t(316, 382, "Выберите явно: варианта по умолчанию нет.", "small muted")
    s += box(316, 398, 712, 76, fill="#ffffff", stroke="#e4e7ec", rx=12)
    s += radio(342, 424, False)
    s += t(364, 430, "Сохранить копию", "body", weight="700")
    s += t(364, 454, "Новая панель с заменой узла сети. Исходная панель не изменяется.", "small muted")
    s += box(316, 486, 712, 128, fill="#eef4ff", stroke="#3156a3", rx=12, sw=2)
    s += radio(342, 512, True)
    s += t(364, 518, "Обновить исходную панель «Базы данных: Prod»", "body", weight="700")
    s += t(364, 542, "Изменяет существующую панель на сервере одним запросом после подтверждения.", "small muted")
    s += box(364, 556, 640, 44, fill="#fff8ed", stroke="#f3d3a6", rx=8)
    s += icon("alert-triangle", 376, 568, "#8a4b00", 18)
    s += t(402, 583, "Панель увидят изменённой все, у кого есть к ней доступ.", "small", "#5c3a00")
    s += t(316, 656, "НЕ ИЗМЕНЯЕТСЯ", "label muted")
    lines = ["7 виджетов без ссылок на pg-a-01 — передаются как есть, с прежними идентификаторами",
             "3 страницы, их порядок и периоды; имя и свойства панели",
             "Владелец и доступ — не передаются в запросе, поэтому сохраняются",
             "Узлы сети, шаблоны, элементы данных и триггеры — не изменяются никогда"]
    y = 686
    for line in lines:
        s += t(316, y, "•  " + line, "small")
        y += 28
    s += t(316, 820, "Сменить цель на другую панель нельзя: замена узла сети записывается", "small muted")
    s += t(316, 842, "в исходную панель или в её копию.", "small muted")
    # right: access type editable, recipients view only (O-031 item 6)
    s += box(1072, 324, 472, 584)
    s += t(1096, 358, "ВЛАДЕЛЕЦ И ДОСТУП", "label muted")
    s += t(1096, 394, "Тип доступа", "small muted")
    s += box(1096, 404, 212, 38, fill="#ffffff", stroke="#8f9da8", rx=8)
    s += icon("lock", 1128, 414, "#475467", 18) + t(1154, 429, "Личная", "button")
    s += box(1308, 404, 212, 38, fill="#243746", stroke="#243746", rx=8)
    s += icon("users", 1340, 414, "#ffffff", 18) + t(1366, 429, "Общая", "button white")
    s += t(1096, 464, "Меняется только тип: список получателей не передаётся", "xs muted")
    s += t(1096, 480, "и сохраняется.", "xs muted")
    s += t(1096, 520, "Получатели · видимая вам часть", "small muted")
    s += chip(1376, 502, 144, "Только просмотр", NEUTRAL)
    y = 534
    for name, perm in [("Группа «DB admins»", "Чтение-запись"), ("Пользователь «dash-editor» (вы)", "Чтение-запись")]:
        s += box(1096, y, 424, 44, fill="#f8fafc", stroke="#eaecf0", rx=8)
        s += icon("users", 1108, y + 12, "#475467", 18)
        s += t(1134, y + 27, name, "small", weight="600")
        s += t(1504, y + 27, perm, "small muted", anchor="end")
        y += 52
    s += box(1096, y + 8, 424, 194, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
    s += icon("info-circle", 1110, y + 22, "#1d3f86", 18)
    s += t(1136, y + 36, "Список может быть неполным", "small", "#1d3f86", weight="700")
    for i, line in enumerate(["Без прав Super admin видны не все получатели,",
                              "а сохранение списка заменяет его целиком",
                              "и удаляет невидимых — и в ZDashstitch,",
                              "и в веб-интерфейсе Zabbix. Список может",
                              "изменить только Super admin.",
                              "Тип «Личная/Общая» менять можно."]):
        s += t(1136, y + 60 + i * 22, line, "small", "#344054")
    s += t(1096, y + 234, "Владелец не показывается и не передаётся.", "xs muted")
    s += action_bar([("Назад к привязкам", "secondary", 190, None)],
                    [("Сохранить черновик", "secondary", 200, None), ("Далее: проверки", "primary", 190, None)])
    finish(s + "</svg>\n", "11-02_P1-08I_target-access-admin.svg", CRUMB_PROD, [SAVED])


# ------------------------------------------------------------------ 11-03 preflight
def screen_preflight():
    s = head("публикация: предварительные проверки",
             "Светлый макет шага «Проверки»: проверки сгруппированы по шагу, где их исправляют; одна блокировка "
             "(виджет с неподдерживаемым изменением затронут заменой) и одно предупреждение; итог справа, план записи "
             "не создаётся, пока есть блокировки.",
             ["circle-check", "circle-x", "alert-triangle", "circle-dashed", "refresh"])
    s += sidebar(ORIGIN_PROD)
    s += title("Предпросмотр и публикация")
    s += steps_bar(2, done=(1,))
    s += context_card(CTX_REPLACE, "Цель: обновить исходную панель «Базы данных: Prod». Проверки выполняются чтением, на сервере ничего не меняется.")
    s += box(292, 324, 880, 584)
    s += t(316, 358, "ПРЕДВАРИТЕЛЬНЫЕ ПРОВЕРКИ", "label muted")
    s += t(1148, 358, "проверено в 14:21", "small muted", anchor="end")
    ok = ("circle-check", "#1f6b43")
    groups = [
        ("Подключение и права", "Подключения", [
            (ok, "Zabbix 7.4.16, адаптер 7.4, «Чтение-запись»; учётная запись может изменять панель", None, None)]),
        ("Источник", "Публикация", [
            (ok, "Панель на сервере совпадает с базой (снимок 14:02)", None, None)]),
        ("Виджеты и поля", "Привязки", [
            (ok, "11 виджетов: изменение подтверждено для Zabbix 7.4.16", None, None),
            (("circle-x", "#a32935"), "«Top queries» (Топ узлов сети) останется на pg-a-01: изменение типа не поддерживается.",
             "Подтвердите список виджетов, которые останутся на старом узле", [("Подтвердить список (1)", 220), ("Открыть привязки", 170)])]),
        ("Привязки", "Привязки", [
            (ok, "21 ссылка на элементы данных сопоставлена точно", None, None),
            (("alert-triangle", "#8a4b00"), "2 ссылки: ключ совпадает, единицы отличаются (B и Bps)", None, [("Посмотреть", 130)])]),
        ("Сохранение и геометрия", "Структура", [
            (ok, "8 виджетов и 3 страницы без изменений, с прежними ID; сетка без выходов и перекрытий", None, None)]),
    ]
    y = 384
    for gname, owner, items in groups:
        s += t(316, y, gname, "small", weight="700")
        s += t(1148, y, "исправляется: " + owner, "xs muted", anchor="end")
        y += 8
        for (ic, color), line1, line2, actions in items:
            h = 34
            if line2:
                h += 22
            if actions:
                h += 42
            bg, st = ("#fff5f5", "#f2b8b5") if ic == "circle-x" else (("#fffaf2", "#f3d3a6") if ic == "alert-triangle" else ("#ffffff", "#eef0f4"))
            s += box(316, y, 832, h, fill=bg, stroke=st, rx=8)
            s += icon(ic, 328, y + 9, color, 18)
            s += t(356, y + 22, line1, "small", "#a32935" if ic == "circle-x" else None, weight="600" if ic != "circle-check" else None)
            yy = y + 22
            if line2:
                yy += 22
                s += t(356, yy, line2, "small", "#a32935", weight="600")
            if actions:
                x = 356
                for text, w in actions:
                    s += button(x, yy + 10, w, text, h=32)
                    x += w + 10
            y += h + 5
        y += 14
    # summary
    s += box(1192, 324, 352, 584)
    s += t(1216, 358, "ИТОГ", "label muted")
    s += box(1216, 374, 304, 64, fill="#fef3f2", stroke="#f2b8b5", rx=10)
    s += icon("circle-x", 1230, 394, "#a32935", 22)
    s += t(1262, 412, "Публикация заблокирована", "body", "#a32935", weight="700")
    counts = [("circle-check", "#1f6b43", "Пройдено", "9"), ("alert-triangle", "#8a4b00", "Предупреждения", "1"),
              ("circle-x", "#a32935", "Блокируют", "1"), ("circle-dashed", "#98a2b3", "Не проверялось", "0")]
    y = 470
    for ic, color, label, n in counts:
        s += icon(ic, 1216, y - 15, color, 20) + t(1246, y, label, "body") + t(1520, y, n, "body", anchor="end", weight="700")
        y += 36
    s += f'  <line x1="1216" y1="{y - 6}" x2="1520" y2="{y - 6}" stroke="#e4e7ec"/>\n'
    for i, line in enumerate(["План записи не создаётся, пока есть", "блокировки. Предупреждения нужно",
                              "просмотреть; они не блокируют."]):
        s += t(1216, y + 24 + i * 22, line, "small muted")
    s += button(1216, y + 102, 304, "Проверить снова", icon_name="refresh")
    s += t(1216, y + 172, "Проверки повторяются автоматически", "xs muted")
    s += t(1216, y + 190, "при открытии этого шага и перед записью.", "xs muted")
    s += action_bar([("Назад: назначение", "secondary", 190, None)],
                    [("Далее: изменения", "primary", 190, None)])
    finish(s + "</svg>\n", "11-03_P1-08C_preflight.svg", CRUMB_PROD, [SAVED])


# ------------------------------------------------------------------ 11-04 diff
def screen_diff():
    s = head("публикация: изменения",
             "Светлый макет шага «Изменения»: отдельные сравнения «База → черновик» и «База → Zabbix сейчас», фильтры "
             "по виду изменения, поиск, таблица по страницам с маркерами и подписями (не только цветом), инспектор "
             "выбранного виджета с изменёнными полями, источниками данных и техническими сведениями.",
             ["pencil", "minus", "plus", "ban", "search", "chevron-down", "chevron-right", "circle-check", "info-circle"])
    s += sidebar(ORIGIN_PROD)
    s += title("Предпросмотр и публикация")
    s += steps_bar(3, done=(1, 2))
    # comparison tabs
    s += box(292, 196, 1252, 60, rx=12)
    s += box(304, 206, 330, 40, fill="#eef4ff", stroke="#3156a3", rx=8)
    s += t(469, 231, "Мои изменения: база → черновик", "small", "#1d3f86", "middle", "700")
    s += box(644, 206, 420, 40, fill="#ffffff", stroke="#e4e7ec", rx=8)
    s += t(800, 231, "На сервере: база → Zabbix сейчас", "small", None, "middle", "600")
    s += chip(930, 213, 124, "без изменений", GREEN)
    s += t(1520, 231, "Проверено в 14:21 · снимок 14:02", "small muted", anchor="end")
    # filters
    s += box(292, 268, 840, 640)
    fx = 316
    for text, w, on in [("Все 19", 86, True), ("Изменены 11", 122, False), ("Остаются 1", 112, False),
                        ("Без изменений 7", 150, False), ("Побочные 0", 110, False), ("Доступ 0", 92, False)]:
        if on:
            s += box(fx, 284, w, 32, fill="#243746", stroke="#243746", rx=16) + t(fx + w / 2, 305, text, "small", "#ffffff", "middle", "650")
        else:
            s += box(fx, 284, w, 32, stroke="#c9d3ea", rx=16) + t(fx + w / 2, 305, text, "small", None, "middle", "600")
        fx += w + 8
    s += box(316, 328, 792, 38, stroke="#7a8d9b", rx=8) + icon("search", 328, 337, "#667085", 20)
    s += t(358, 352, "Поиск по имени виджета, полю или значению", "small muted")
    # header
    s += box(316, 378, 792, 34, fill="#f8fafc", stroke="#eef0f4", rx=6)
    for x, h in [(330, "ОБЪЕКТ"), (590, "ИЗМЕНЕНИЕ"), (770, "БЫЛО (БАЗА)"), (940, "СТАНЕТ")]:
        s += t(x, 400, h, "xs muted", weight="700")
    rows = [
        ("group", "chevron-down", "Страница 1. PostgreSQL", "4 из 8 виджетов изменены"),
        ("pencil", "Connections · График", "Изменено поле", "pg-a-01", "pg-b-01", False),
        ("pencil", "TPS · График", "Изменено поле", "pg-a-01", "pg-b-01", False),
        ("minus", "Locks · Значение", "Без изменений", "", "", False),
        ("group", "chevron-down", "Страница 2. ClickHouse", "3 из 5 виджетов изменены"),
        ("pencil", "Queries per second · График", "Изменены 2 поля", "pg-a-01", "pg-b-01", True),
        ("pencil", "Replication lag · Значение", "Изменено поле", "pg-a-01", "pg-b-01", False),
        ("ban", "Top queries · Топ узлов сети", "Останется на старом", "pg-a-01", "pg-a-01", False),
        ("group", "chevron-right", "Страница 3. Keeper и HAProxy", "4 из 6 виджетов изменены"),
        ("group", None, "Свойства панели", "без изменений"),
        ("group", None, "Владелец и доступ", "не передаются в запросе"),
    ]
    y = 418
    for r in rows:
        if r[0] == "group":
            _, chev, name, note = r
            s += box(316, y, 792, 34, fill="#f5f7fb", stroke="#f5f7fb", rx=6)
            if chev:
                s += icon(chev, 324, y + 8, "#475467", 18)
            s += t(350, y + 23, name, "small", weight="700")
            s += t(1096, y + 23, note, "small muted", anchor="end")
            y += 40
            continue
        ic, name, change, before, after, sel = r
        if sel:
            s += box(316, y, 792, 40, fill="#eef4ff", stroke="#3156a3", rx=6, sw=2)
        else:
            s += f'  <line x1="316" y1="{y + 40}" x2="1108" y2="{y + 40}" stroke="#eef0f4"/>\n'
        color = {"pencil": "#3156a3", "minus": "#98a2b3", "ban": "#8a4b00"}[ic]
        s += icon(ic, 330, y + 11, color, 18)
        s += t(356, y + 25, name, "small", weight="600")
        s += t(590, y + 25, change, "small", "#8a4b00" if ic == "ban" else None)
        if before:
            s += t(770, y + 25, before, "mono")
            s += t(940, y + 25, after, "mono")
        y += 44
    s += t(316, 896, "Сопоставление виджетов — по идентификаторам Zabbix из базы: при записи они передаются и сохраняются.", "xs muted")
    # inspector
    s += box(1152, 268, 392, 640)
    s += t(1176, 302, "ВЫБРАНО", "label muted")
    s += t(1176, 330, "Queries per second", "h3")
    s += t(1176, 352, "График (svggraph) · страница 2 · 36, 0 · 36 × 5", "xs muted")
    s += t(1176, 386, "ИЗМЕНЁННЫЕ ПОЛЯ", "label muted")
    y = 400
    for field, before, after in [("ds.0.hosts.0", "pg-a-01", "pg-b-01"), ("ds.1.hosts.0", "pg-a-01", "pg-b-01")]:
        s += box(1176, y, 344, 64, fill="#f8fafc", stroke="#eef0f4", rx=8)
        s += t(1190, y + 22, field, "mono")
        s += t(1190, y + 48, before, "mono") + t(1290, y + 48, "→", "small muted") + t(1316, y + 48, after, "mono")
        y += 72
    s += t(1176, y + 22, "ИСТОЧНИКИ ДАННЫХ", "label muted")
    s += icon("circle-check", 1176, y + 36, "#1f6b43", 18)
    s += t(1202, y + 50, "На pg-b-01 найдены 2 из 2 элементов", "small")
    s += t(1202, y + 70, "данных по имени шаблона (точно)", "small")
    s += t(1176, y + 108, "НЕ МЕНЯЕТСЯ", "label muted")
    s += t(1176, y + 134, "Положение, размер, период, остальные 14 полей.", "small")
    s += t(1176, y + 156, "Неизвестные поля (2) передаются как есть.", "small")
    s += f'  <line x1="1176" y1="{y + 178}" x2="1520" y2="{y + 178}" stroke="#e4e7ec"/>\n'
    s += icon("chevron-right", 1172, y + 192, "#475467", 18)
    s += t(1196, y + 206, "Технические сведения", "small", weight="650")
    s += t(1196, y + 228, "widgetid, pageid, тип и коды полей", "xs muted")
    s += action_bar([("Назад: проверки", "secondary", 180, None)],
                    [("Сохранить черновик", "secondary", 200, None), ("Далее: подтверждение", "primary", 230, None)])
    finish(s + "</svg>\n", "11-04_P1-08B_changes.svg", CRUMB_PROD, [SAVED])


# ------------------------------------------------------------------ 11-05 conflict
def screen_conflict():
    s = head("публикация: конфликт",
             "Светлый макет конфликта: панель на сервере изменилась после копирования, старый план недействителен, "
             "показаны изменения на сервере и пересечение с локальными изменениями, безопасные выходы — сохранить копию "
             "или скопировать панель заново; обновления поверх и автоматического слияния нет.",
             ["alert-triangle", "plus", "pencil", "circle-x", "circle-check", "info-circle", "refresh"])
    s += sidebar(["Копия с сервера", "«Базы данных: Prod»", "Лаборатория 7.4", "снимок 25.10.2026, 14:02", "Изменена на сервере"])
    s += title("Предпросмотр и публикация")
    s += steps_bar(2, done=(1,), error=2)
    # banner
    s += box(292, 196, 1252, 112, fill="#fef3f2", stroke="#f2b8b5", rx=14)
    s += icon("alert-triangle", 316, 216, "#a32935", 24)
    s += t(352, 234, "Панель «Базы данных: Prod» изменилась на сервере после копирования", "h3", "#a32935")
    s += t(352, 262, "Старый план нельзя опубликовать, обновление этой панели остановлено. Ваш черновик не изменён и сохранён.", "small", "#5c1f1f")
    s += t(352, 286, "Изменения обнаружены при проверке в 14:31: сервер не совпадает со снимком 14:02.", "small", "#5c1f1f")
    # remote changes
    s += box(292, 324, 840, 584)
    s += box(316, 340, 400, 40, fill="#eef4ff", stroke="#3156a3", rx=8)
    s += t(516, 365, "На сервере: база → Zabbix сейчас", "small", "#1d3f86", "middle", "700")
    s += box(726, 340, 300, 40, fill="#ffffff", stroke="#e4e7ec", rx=8)
    s += t(876, 365, "Мои изменения: база → черновик", "small", None, "middle", "600")
    s += box(316, 396, 792, 34, fill="#f8fafc", stroke="#eef0f4", rx=6)
    for x, h in [(330, "ОБЪЕКТ"), (620, "ИЗМЕНЕНИЕ НА СЕРВЕРЕ"), (900, "МОИ ИЗМЕНЕНИЯ")]:
        s += t(x, 418, h, "xs muted", weight="700")
    rows = [
        ("plus", "#1f6b43", "Merges · График", "Страница 2: добавлен", "—", False),
        ("pencil", "#3156a3", "Queries per second · График", "Изменён период отображения", "Заменён узел сети", True),
        ("pencil", "#3156a3", "Страница 3. Keeper и HAProxy", "Переименована", "—", False),
    ]
    y = 436
    for ic, color, name, remote, local, overlap in rows:
        if overlap:
            s += box(316, y, 792, 44, fill="#fffaf2", stroke="#f3d3a6", rx=6)
        else:
            s += f'  <line x1="316" y1="{y + 44}" x2="1108" y2="{y + 44}" stroke="#eef0f4"/>\n'
        s += icon(ic, 330, y + 13, color, 18)
        s += t(356, y + 27, name, "small", weight="600")
        s += t(620, y + 27, remote, "small")
        s += t(900, y + 27, local, "small", "#8a4b00" if overlap else "#667085", weight="650" if overlap else None)
        y += 50
    s += icon("alert-triangle", 316, y + 12, "#8a4b00", 18)
    s += t(342, y + 26, "Пересекается с вашими изменениями: 1 виджет (Queries per second).", "small", "#8a4b00", weight="650")
    s += t(316, y + 64, "Кто и когда изменил панель, Zabbix API не сообщает: известно только, что панель", "small muted")
    s += t(316, y + 86, "отличается от снимка. Сравнение без автоматического слияния.", "small muted")
    # options
    s += box(1152, 324, 392, 584)
    s += t(1176, 358, "ЧТО ДАЛЬШЕ", "label muted")
    opts = [
        ("Сохранить копию", "Ваша версия станет новой панелью.", "Изменённая на сервере панель не трогается.", "primary"),
        ("Скопировать панель заново", "Новый снимок с сервера. Ваши изменения", "нужно повторить; этот черновик остаётся.", "secondary"),
    ]
    y = 378
    for name, l1, l2, kind in opts:
        s += button(1176, y, 344, name, kind)
        s += t(1176, y + 66, l1, "small muted")
        s += t(1176, y + 88, l2, "small muted")
        y += 124
    s += box(1176, y, 344, 108, fill="#f8fafc", stroke="#eaecf0", rx=10)
    s += icon("circle-x", 1190, y + 14, "#98a2b3", 18)
    s += t(1216, y + 28, "Обновить поверх — недоступно", "small", "#475467", weight="700")
    s += t(1216, y + 52, "Запись затёрла бы чужие изменения.", "small muted")
    s += t(1216, y + 74, "Автоматического слияния нет.", "small muted")
    s += t(1176, y + 146, "Проверить снова: если сервер вернули", "xs muted")
    s += t(1176, y + 164, "к снимку, конфликт исчезнет.", "xs muted")
    s += button(1176, y + 180, 344, "Проверить снова", icon_name="refresh")
    s += action_bar([("Назад: назначение", "secondary", 190, None)],
                    [("Далее: изменения", "disabled", 190, None)])
    finish(s + "</svg>\n", "11-05_P1-08D_conflict.svg", CRUMB_PROD,
           [("Изменена на сервере", "#fef3f2", "#a32935", 190), SAVED])


# ------------------------------------------------------------------ 11-06 confirmation
def screen_confirm():
    s = head("публикация: подтверждение",
             "Светлый макет диалога подтверждения записи поверх шага «Подтверждение»: точная панель и подключение, "
             "действие API, число изменений, что не меняется, принятое предупреждение, одноразовый план, повторное "
             "чтение перед отправкой и условия, при которых план становится недействительным.",
             ["alert-triangle", "info-circle", "circle-check"])
    s += sidebar(ORIGIN_PROD)
    s += title("Предпросмотр и публикация")
    s += steps_bar(4, done=(1, 2, 3))
    s += context_card(CTX_REPLACE[:4] + [("ОБЛАСТЬ", "11 из 19 виджетов", "1 остаётся на pg-a-01", 180)],
                      "Цель: обновить исходную панель «Базы данных: Prod».")
    s += box(292, 324, 1252, 584)
    s += action_bar([("Назад: изменения", "secondary", 190, None)],
                    [("Подтвердить запись…", "primary", 220, None)])
    # dialog
    s += '  <!-- Dialog -->\n  <rect x="0" y="72" width="1600" height="928" fill="#17213a" fill-opacity=".45"/>\n'
    s += box(420, 150, 760, 760, stroke="#c9d3ea")
    s += t(456, 200, "Подтвердите запись панели", "title")
    s += t(456, 234, "Записать 14 проверенных изменений в панель «Базы данных: Prod»?", "body")
    rows = [("Подключение", "Лаборатория 7.4 · Zabbix 7.4.16 · чтение-запись"),
            ("Панель", "«Базы данных: Prod» — существующая, обновляется"),
            ("Действие", "один запрос dashboard.update"),
            ("Изменяется", "11 виджетов, 14 полей: узел сети pg-a-01 → pg-b-01"),
            ("Не изменяется", "8 виджетов, 3 страницы, свойства, владелец и доступ"),
            ("Предупреждение", "2 ссылки с другими единицами — просмотрено"),
            ("Остаётся на pg-a-01", "«Top queries» — список подтверждён")]
    y = 272
    for k, v in rows:
        s += f'  <line x1="456" y1="{y}" x2="1144" y2="{y}" stroke="#eef0f4"/>\n'
        s += t(456, y + 30, k, "small muted")
        if k == "Действие":
            s += t(620, y + 30, "один запрос", "small") + t(712, y + 30, "dashboard.update", "mono")
        else:
            s += t(620, y + 30, v, "small", weight="600")
        y += 44
    s += f'  <line x1="456" y1="{y}" x2="1144" y2="{y}" stroke="#eef0f4"/>\n'
    s += t(456, y + 30, "План", "small muted") + t(620, y + 30, "zds-3f9a41c2d07e-81b0c5aa94e1-14", "mono")
    s += t(620, y + 52, "действует один раз; создан в 14:24", "xs muted")
    y += 80
    s += box(456, y, 688, 120, fill="#f5f8ff", stroke="#c9d3ea", rx=10)
    s += icon("info-circle", 472, y + 14, "#1d3f86", 18)
    s += t(498, y + 28, "Перед отправкой панель будет прочитана ещё раз.", "small", "#1d3f86", weight="700")
    s += t(498, y + 52, "Если она изменилась после проверки в 14:21, запись не выполнится.", "small", "#344054")
    s += t(498, y + 76, "План станет недействительным при изменении черновика, цели,", "small", "#344054")
    s += t(498, y + 98, "подключения, режима или прав — тогда нужен новый предпросмотр.", "small", "#344054")
    y += 140
    s += t(456, y + 10, "После отправки отменить запись нельзя; результат проверяется повторным чтением.", "small muted")
    s += button(812, 840, 120, "Отмена")
    s += button(944, 840, 200, "Записать в Zabbix", "primary")
    finish(s + "</svg>\n", "11-06_P1-08F_confirm.svg", CRUMB_PROD, [SAVED])


# ------------------------------------------------------------------ 11-07 verified result
def progress_rows(s, rows, y0):
    y = y0
    for ic, color, name, line1, line2 in rows:
        h = 76 if line2 else 56
        s += box(316, y, 832, h, fill="#ffffff", stroke="#eef0f4", rx=10)
        s += icon(ic, 332, y + 16, color, 22)
        s += t(368, y + 32, name, "body", weight="700")
        s += t(640, y + 32, line1, "small")
        if line2:
            s += t(640, y + 56, line2, "small muted")
        y += h + 8
    return s, y


def screen_verified():
    s = head("публикация: результат проверен",
             "Светлый макет результата публикации: этапы повторного чтения перед записью, единственной отправки, "
             "повторного чтения, структурной проверки и проверки источников данных пройдены; отображение не проверяется "
             "автоматически; база черновика обновлена снимком после записи.",
             ["circle-check", "circle-dashed", "external-link", "info-circle"])
    s += sidebar(["Копия с сервера", "«Базы данных: Prod»", "Лаборатория 7.4", "снимок 25.10.2026, 14:33", "база обновлена после записи"])
    s += title("Предпросмотр и публикация")
    s += steps_bar(5, done=(1, 2, 3, 4))
    s += box(292, 196, 1252, 96, fill="#ecfdf3", stroke="#a6dcbc", rx=14)
    s += icon("circle-check", 316, 216, "#1f6b43", 26)
    s += t(354, 236, "Проверено повторным чтением", "h3", "#1f6b43")
    s += t(354, 264, "Структура и источники данных проверены. Проверьте отображение панели в Zabbix.", "small", "#14432a")
    s += box(292, 308, 880, 600)
    s += t(316, 342, "ЭТАПЫ", "label muted")
    ok = "#1f6b43"
    rows = [
        ("circle-check", ok, "Чтение перед записью", "Панель совпадает с базой · 14:33:01", None),
        ("circle-check", ok, "Запись", "Один запрос dashboard.update, ответ получен · 0,1 с", None),
        ("circle-check", ok, "Повторное чтение", "Панель прочитана · 14:33:02", None),
        ("circle-check", ok, "Структура", "11 виджетов изменены как в плане; 8 виджетов и 3 страницы",
         "не изменились; идентификаторы сохранены; неизвестные поля на месте"),
        ("circle-check", ok, "Источники данных", "23 из 23 ссылок указывают на элементы данных pg-b-01", None),
        ("circle-dashed", "#98a2b3", "Отображение", "Не проверяется автоматически: откройте панель в Zabbix", None),
    ]
    s, y = progress_rows(s, rows, 358)
    s += box(1192, 308, 352, 600)
    s += t(1216, 342, "ОПЕРАЦИЯ", "label muted")
    kv = [("Состояние", "Проверено"), ("Начата", "25.10.2026, 14:33:01"), ("Завершена", "14:33:02"),
          ("Повторных записей", "0")]
    y = 376
    for k, v in kv:
        s += t(1216, y, k, "small muted") + t(1520, y, v, "small", anchor="end", weight="600")
        y += 30
    s += t(1216, y + 4, "План", "small muted")
    s += t(1216, y + 28, "zds-3f9a41c2d07e-81b0c5aa94e1-14", "mono")
    s += f'  <line x1="1216" y1="{y + 50}" x2="1520" y2="{y + 50}" stroke="#e4e7ec"/>\n'
    s += t(1216, y + 82, "Что изменилось в черновике", "small", weight="700")
    for i, line in enumerate(["Снимок после записи стал новой базой:", "следующий предпросмотр сравнивает", "с ним. Локальных изменений нет."]):
        s += t(1216, y + 106 + i * 22, line, "small muted")
    s += button(1216, y + 190, 304, "Журнал операции")
    s += action_bar([("Вернуться к черновику", "secondary", 230, None)],
                    [("Открыть в Zabbix", "primary", 210, "external-link")])
    finish(s + "</svg>\n", "11-07_P1-08G_verified.svg", CRUMB_PROD,
           [("Опубликована · проверено", "#ecfdf3", "#1f6b43", 220)])


# ------------------------------------------------------------------ 11-08 unknown outcome
def screen_unknown():
    s = head("публикация: результат неизвестен",
             "Светлый макет неизвестного результата при создании копии: запрос отправлен, ответ не получен, повторная "
             "запись запрещена; доступна только проверка результата чтением по точному имени и отпечатку с тремя "
             "возможными исходами; запись журнала пережила бы перезапуск.",
             ["circle-check", "alert-triangle", "circle-dashed", "circle-x", "refresh", "info-circle", "external-link"])
    s += sidebar(["Копия с сервера", "«Базы данных: Prod»", "Лаборатория 7.4", "снимок 25.10.2026, 14:02", "3 страницы · 19 виджетов"])
    s += title("Предпросмотр и публикация")
    s += steps_bar(5, done=(1, 2, 3, 4))
    s += box(292, 196, 1252, 96, fill="#fef3f2", stroke="#f2b8b5", rx=14)
    s += icon("alert-triangle", 316, 216, "#a32935", 26)
    s += t(354, 236, "Результат публикации неизвестен", "h3", "#a32935")
    s += t(354, 264, "Не повторяйте публикацию. Прочитайте панель, чтобы определить результат.", "small", "#5c1f1f")
    s += box(292, 308, 880, 600)
    s += t(316, 342, "ЭТАПЫ · СОХРАНИТЬ КОПИЮ «Базы данных: Pilot»", "label muted")
    rows = [
        ("circle-check", "#1f6b43", "Проверка имени", "Среди видимых панелей имя свободно · 14:40:11", None),
        ("alert-triangle", "#a32935", "Запись", "Запрос dashboard.create отправлен, ответ не получен:",
         "соединение прервано через 30 с. Панель могла быть создана."),
        ("circle-dashed", "#98a2b3", "Повторное чтение", "Не выполнено", None),
    ]
    s, y = progress_rows(s, rows, 358)
    s += t(316, y + 26, "ЧТО ПОКАЖЕТ ПРОВЕРКА РЕЗУЛЬТАТА", "label muted")
    y += 40
    s += box(316, y, 832, 34, fill="#f8fafc", stroke="#eef0f4", rx=6)
    s += t(330, y + 22, "НАЙДЕНО ПО ИМЕНИ И ОТПЕЧАТКУ", "xs muted", weight="700")
    s += t(640, y + 22, "ИТОГ", "xs muted", weight="700")
    s += t(900, y + 22, "ДАЛЬШЕ", "xs muted", weight="700")
    y += 34
    outcomes = [("Одна панель, структура как в плане", "Сверено: записано", "можно продолжать"),
                ("Ни одной", "Сверено: не записано", "новый предпросмотр"),
                ("Несколько или структура другая", "Нужна проверка вручную", "открыть в Zabbix")]
    for found, result, nxt in outcomes:
        s += t(330, y + 26, found, "small")
        s += t(640, y + 26, result, "small", weight="650")
        s += t(900, y + 26, nxt, "small muted")
        s += f'  <line x1="316" y1="{y + 40}" x2="1148" y2="{y + 40}" stroke="#eef0f4"/>\n'
        y += 40
    s += t(316, y + 28, "Новая запись в эту цель заблокирована, пока результат не сверен.", "small", "#a32935", weight="650")
    # right
    s += box(1192, 308, 352, 600)
    s += t(1216, 342, "ПРОВЕРКА РЕЗУЛЬТАТА", "label muted")
    for i, line in enumerate(["Только чтение: панель ищется по", "точному имени и отпечатку структуры.", "Повторной отправки нет."]):
        s += t(1216, 374 + i * 22, line, "small")
    s += button(1216, 450, 304, "Проверить результат", "primary", icon_name="refresh")
    s += f'  <line x1="1216" y1="516" x2="1520" y2="516" stroke="#e4e7ec"/>\n'
    s += t(1216, 548, "ЖУРНАЛ", "label muted")
    kv = [("Состояние", "Результат неизвестен"), ("Отправлено", "14:40:12"), ("Повторных записей", "0")]
    y = 580
    for k, v in kv:
        s += t(1216, y, k, "small muted") + t(1520, y, v, "small", anchor="end", weight="600")
        y += 30
    s += icon("info-circle", 1216, y + 4, "#667085", 18)
    for i, line in enumerate(["Запись журнала сделана на диск до", "отправки. После перезапуска операция", "покажется до любых новых записей."]):
        s += t(1242, y + 18 + i * 22, line, "small muted")
    s += button(1216, y + 96, 304, "Открыть журнал операции")
    s += action_bar([("Вернуться к черновику", "secondary", 230, None)],
                    [("Открыть «Панели в Zabbix»", "secondary", 270, "external-link")])
    finish(s + "</svg>\n", "11-08_P1-08G_unknown-outcome.svg", CRUMB_PROD,
           [("Результат неизвестен", "#fef3f2", "#a32935", 190), SAVED])


if __name__ == "__main__":
    screen_target_superadmin()
    screen_target_admin()
    screen_preflight()
    screen_diff()
    screen_conflict()
    screen_confirm()
    screen_verified()
    screen_unknown()
