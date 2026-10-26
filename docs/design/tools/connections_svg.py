"""Review mockups 05-01..05-04: «Подключения» (P1-02).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/connections_svg.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402

OUT = "docs/design/assets/05-connections/"
SYM = ('viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" '
       'stroke-linecap="round" stroke-linejoin="round"')
ICONS = {
    "circle-check": '<path d="M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0"/><path d="M9 12l2 2l4 -4"/>',
    "circle-x": '<path d="M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0"/><path d="M10 10l4 4m0 -4l-4 4"/>',
    "circle-dashed": ('<path d="M8.56 3.69a9 9 0 0 0 -2.92 1.95"/><path d="M3.69 8.56a9 9 0 0 0 -.69 3.44"/>'
                      '<path d="M3.69 15.44a9 9 0 0 0 1.95 2.92"/><path d="M8.56 20.31a9 9 0 0 0 3.44 .69"/>'
                      '<path d="M15.44 20.31a9 9 0 0 0 2.92 -1.95"/><path d="M20.31 15.44a9 9 0 0 0 .69 -3.44"/>'
                      '<path d="M20.31 8.56a9 9 0 0 0 -1.95 -2.92"/><path d="M15.44 3.69a9 9 0 0 0 -3.44 -.69"/>'),
    "plus": '<path d="M12 5l0 14"/><path d="M5 12l14 0"/>',
    "alert-triangle": ('<path d="M12 9v4"/><path d="M10.363 3.591l-8.106 13.534a1.914 1.914 0 0 0 1.636 2.871h16.214'
                       'a1.914 1.914 0 0 0 1.636 -2.87l-8.106 -13.536a1.914 1.914 0 0 0 -3.274 0"/><path d="M12 16h.01"/>'),
}
STYLE = '''    <style>
      text{font-family:system-ui,-apple-system,"Segoe UI",sans-serif}
      .muted{fill:#667085}.white{fill:#ffffff}.small{font-size:14px}.xs{font-size:12px}.label{font-size:13px;font-weight:700;letter-spacing:.08em}
      .title{font-size:24px;font-weight:750}.h3{font-size:17px;font-weight:700}.body{font-size:16px}.button{font-size:15px;font-weight:700}
      .badge{font-size:13px;font-weight:650}.mono{font-family:ui-monospace,"Cascadia Mono",Consolas,monospace;font-size:14px;fill:#344054}
    </style>
'''
LAB_RO = ("Только чтение · Лаборатория 7.4", "#eef4ff", "#3156a3", 270)


def head(title, desc, icons):
    syms = "".join(f'    <symbol id="ti-{n}" {SYM}>{ICONS[n]}</symbol>\n' for n in icons)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1000" viewBox="0 0 1600 1000" fill="#17213a" role="img" aria-labelledby="title desc">\n'
            f'  <title id="title">ZDashstitch — {title}</title>\n  <desc id="desc">{desc}</desc>\n  <defs>\n'
            f'    <!-- Tabler Icons outline v3.49.0 (MIT), see assets/icons/THIRD_PARTY.md -->\n{syms}{STYLE}  </defs>\n'
            f'  <rect width="1600" height="1000" fill="#f5f7fb"/>\n\n  <!-- App bar -->\n  <!-- Content -->\n')


def chip(x, y, w, text, fill, color):
    return (f'  <rect x="{x}" y="{y}" width="{w}" height="28" rx="14" fill="{fill}"/>\n'
            f'  <text x="{x + w / 2:.0f}" y="{y + 19}" class="badge" text-anchor="middle" fill="{color}">{text}</text>\n')


def button(x, y, w, text, primary=False, h=42, color=None):
    if primary:
        return (f'  <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="10" fill="#3156a3"/>\n'
                f'  <text x="{x + w / 2:.0f}" y="{y + h / 2 + 5:.0f}" class="button white" text-anchor="middle">{text}</text>\n')
    fill = f' fill="{color}"' if color else ""
    return (f'  <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="10" fill="#ffffff" stroke="#c9d3ea"/>\n'
            f'  <text x="{x + w / 2:.0f}" y="{y + h / 2 + 5:.0f}" class="button" text-anchor="middle"{fill}>{text}</text>\n')


def finish(svg, path, mode):
    svg = appbar.apply(svg, active="connections", mode=mode, depth_prefix="../../../../")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(svg)
    print("written", path)


# ------------------------------------------------------------------ 05-01 list
def list_body():
    s = '''  <text x="128" y="124" class="title">Подключения</text>
  <text x="128" y="154" class="small muted">Профили серверов Zabbix. Локальные черновики работают без подключения.</text>
  <rect x="1240" y="98" width="232" height="42" rx="10" fill="#3156a3"/>
  <use href="#ti-plus" x="1258" y="109" width="20" height="20" color="#ffffff"/>
  <text x="1286" y="125" class="button white">Новое подключение</text>
'''
    cards = [
        dict(name="Лаборатория 7.4", active=True,
             chip=("Подключено · только чтение", "#ecfdf3", "#1f6b43", 220),
             url="https://zabbix-lab.example.invalid/zabbix/",
             meta="Zabbix 7.4.16 · проверено сегодня, 14:05 · API-токен в системном хранилище",
             buttons=[("Изменить", False, 120), ("Отключиться", False, 150)]),
        dict(name="Prod", active=False,
             chip=("Не подключено", "#f2f4f7", "#344054", 140),
             url="https://monitoring.example.invalid:8443/",
             meta="Zabbix 7.4.16 · проверено 24.10.2026 · имя пользователя и пароль, пароль не сохраняется",
             buttons=[("Изменить", False, 120), ("Подключиться", True, 160)]),
        dict(name="Pilot", active=False,
             chip=("Последняя проверка: ошибка", "#fff4e5", "#8a4b00", 230),
             url="http://zabbix-pilot.example.invalid/zabbix/",
             meta="Сервер недоступен или не ответил вовремя · 23.10.2026",
             warn="HTTP без шифрования: данные входа передаются открытым текстом",
             buttons=[("Изменить", False, 120), ("Проверить", False, 130)]),
    ]
    y = 184
    for c in cards:
        stroke = 'stroke="#3156a3" stroke-width="2"' if c["active"] else 'stroke="#d0d5dd"'
        h = 150 if c.get("warn") else 128
        s += f'  <rect x="128" y="{y}" width="1344" height="{h}" rx="16" fill="#ffffff" {stroke}/>\n'
        s += f'  <text x="156" y="{y + 40}" class="h3">{c["name"]}</text>\n'
        cx = 156 + 12 * len(c["name"]) + 24
        s += chip(cx, y + 20, c["chip"][3], *c["chip"][:3])
        s += f'  <text x="156" y="{y + 74}" class="mono">{c["url"]}</text>\n'
        s += f'  <text x="156" y="{y + 104}" class="small muted">{c["meta"]}</text>\n'
        if c.get("warn"):
            s += f'  <use href="#ti-alert-triangle" x="156" y="{y + 116}" width="18" height="18" color="#8a4b00"/>\n'
            s += f'  <text x="182" y="{y + 130}" class="small" fill="#8a4b00">{c["warn"]}</text>\n'
        bx = 1448
        for text, primary, w in reversed(c["buttons"]):
            bx -= w
            s += button(bx, y + 43, w, text, primary)
            bx -= 12
        y += h + 16
    s += f'''
  <text x="128" y="{y + 36}" class="body" font-weight="650" fill="#3156a3">Продолжить без подключения</text>
  <text x="128" y="{y + 64}" class="small muted">Пароли и токены не показываются на этом экране и не попадают в файлы, журналы и отчёты.</text>
'''
    return s


svg = head("подключения", "Светлый макет списка подключений: активный профиль в режиме только чтения, сохранённый профиль без подключения, профиль с ошибкой последней проверки и предупреждением о HTTP; создание профиля и продолжение без подключения.",
           ["plus", "alert-triangle"]) + list_body() + "</svg>\n"
finish(svg, OUT + "05-01_P1-02A_connections-list.svg", LAB_RO)

# ------------------------------------------------------------------ 05-02 editor
s = head("изменение подключения", "Светлый макет редактора профиля подключения: имя, полный адрес с портом и базовым путём, вычисленный адрес API, предупреждение о смене сервера, вход по имени пользователя и паролю без сохранения пароля, доверие к сертификату и режим после подключения.",
         ["alert-triangle"])
s += '''  <text x="128" y="92" font-size="13" fill="#667085">Подключения  /  Prod  /  Изменить</text>
  <text x="128" y="124" class="title">Изменить подключение «Prod»</text>

  <rect x="128" y="148" width="860" height="772" rx="16" fill="#ffffff" stroke="#d0d5dd"/>
  <text x="156" y="184" class="label muted">ПРОФИЛЬ</text>
  <text x="156" y="214" class="small muted">Имя профиля</text>
  <rect x="156" y="224" width="400" height="40" rx="8" fill="#ffffff" stroke="#7a8d9b"/>
  <text x="170" y="250" class="body">Prod</text>
  <text x="156" y="292" class="small muted">Адрес Zabbix — как в адресной строке браузера, с портом и путём</text>
  <rect x="156" y="302" width="804" height="40" rx="8" fill="#ffffff" stroke="#3156a3" stroke-width="2"/>
  <text x="170" y="328" class="mono">https://monitoring-new.example.invalid:8443/zabbix</text>
  <text x="156" y="364" class="xs muted">Адрес API: https://monitoring-new.example.invalid:8443/zabbix/api_jsonrpc.php</text>
  <rect x="156" y="378" width="804" height="64" rx="10" fill="#fff8ed" stroke="#f3d3a6"/>
  <use href="#ti-alert-triangle" x="172" y="392" width="18" height="18" color="#8a4b00"/>
  <text x="198" y="406" class="small" font-weight="700" fill="#8a4b00">Это другой сервер</text>
  <text x="198" y="428" class="small" fill="#5c3a00">После сохранения привязки и планы публикации этого подключения нужно будет проверить заново.</text>

  <line x1="156" y1="466" x2="960" y2="466" stroke="#e4e7ec"/>
  <text x="156" y="498" class="label muted">ВХОД</text>
  <rect x="156" y="514" width="390" height="36" rx="8" fill="#ffffff" stroke="#8f9da8"/>
  <text x="226" y="537" class="button" text-anchor="middle">API-токен</text>
  <rect x="296" y="514" width="250" height="36" rx="8" fill="#243746"/>
  <text x="421" y="537" class="button white" text-anchor="middle">Имя пользователя и пароль</text>
  <text x="156" y="580" class="small muted">Имя пользователя</text>
  <rect x="156" y="590" width="390" height="40" rx="8" fill="#ffffff" stroke="#7a8d9b"/>
  <text x="170" y="616" class="body">dash-editor</text>
  <text x="570" y="580" class="small muted">Пароль</text>
  <rect x="570" y="590" width="390" height="40" rx="8" fill="#ffffff" stroke="#7a8d9b"/>
  <text x="584" y="616" class="body muted">вводится при подключении</text>
  <text x="156" y="656" class="small muted">Пароль не сохраняется: его спрашивают при каждом подключении. Для учётной записи с MFA вход по паролю</text>
  <text x="156" y="676" class="small muted">через API недоступен — используйте API-токен, выданный администратором.</text>

  <line x1="156" y1="700" x2="960" y2="700" stroke="#e4e7ec"/>
  <text x="156" y="732" class="label muted">ДОВЕРИЕ К СЕРТИФИКАТУ</text>
  <circle cx="168" cy="758" r="9" fill="#ffffff" stroke="#3156a3" stroke-width="2"/>
  <circle cx="168" cy="758" r="4.5" fill="#3156a3"/>
  <text x="188" y="764" class="body">Системное хранилище сертификатов</text>
  <circle cx="168" cy="794" r="9" fill="#ffffff" stroke="#7a8d9b" stroke-width="2"/>
  <text x="188" y="800" class="body">Добавить доверенный сертификат УЦ</text>
  <text x="508" y="800" class="small muted">файл PEM, только для этого подключения</text>
  <text x="156" y="836" class="small muted">Отключить проверку сертификата нельзя.</text>
  <text x="156" y="876" class="label muted">РЕЖИМ ПОСЛЕ ПОДКЛЮЧЕНИЯ</text>
  <text x="156" y="900" class="small">Только чтение. Запись панелей включается отдельно и до отключения.</text>

  <!-- side panel -->
  <rect x="1012" y="148" width="460" height="530" rx="16" fill="#ffffff" stroke="#d0d5dd"/>
  <text x="1036" y="184" class="label muted">ПРИМЕРЫ АДРЕСА</text>
  <text x="1036" y="214" class="small" font-weight="700" fill="#1f6b43">Подходят</text>
  <text x="1036" y="240" class="mono">https://zabbix.example.invalid/</text>
  <text x="1036" y="264" class="mono">https://zabbix.example.invalid:8443/zabbix</text>
  <text x="1036" y="288" class="mono">http://192.0.2.10/zabbix</text>
  <text x="1036" y="312" class="mono">https://[2001:db8::10]:8443/</text>
  <text x="1036" y="352" class="small" font-weight="700" fill="#a32935">Не подходят</text>
  <text x="1036" y="378" class="mono">zabbix.example.invalid</text>
  <text x="1300" y="378" class="xs muted">нет схемы</text>
  <text x="1036" y="402" class="mono">https://user@host/zabbix</text>
  <text x="1300" y="402" class="xs muted">имя в адресе</text>
  <text x="1036" y="426" class="mono">https://host/zabbix#main</text>
  <text x="1300" y="426" class="xs muted">фрагмент</text>
  <text x="1036" y="450" class="mono">ftp://host/zabbix</text>
  <text x="1300" y="450" class="xs muted">схема</text>
  <line x1="1036" y1="474" x2="1448" y2="474" stroke="#e4e7ec"/>
  <text x="1036" y="504" class="small muted">Для http данные входа идут открытым текстом;</text>
  <text x="1036" y="526" class="small muted">сохранить такой профиль можно только после</text>
  <text x="1036" y="548" class="small muted">явного подтверждения.</text>
  <text x="1036" y="588" class="small muted">Переименование профиля не меняет сервер.</text>
  <text x="1036" y="610" class="small muted">Смена адреса, порта или пути — меняет.</text>
'''
s += button(1012, 702, 220, "Проверить подключение")
s += button(1252, 702, 220, "Сохранить", primary=True)
s += button(1012, 760, 140, "Отмена")
s += button(1164, 760, 150, "Дублировать")
s += button(1326, 760, 146, "Удалить", color="#a32935")
s += "</svg>\n"
finish(s, OUT + "05-02_P1-02B_profile-editor.svg", LAB_RO)

# ------------------------------------------------------------------ 05-03 test
s = head("проверка подключения", "Светлый макет пошаговой проверки подключения: адрес и TLS, API Zabbix и версия пройдены, вход не удался, следующие шаги не проверялись; исправление, повтор и копирование санитизированного отчёта.",
         ["circle-check", "circle-x", "circle-dashed"])
s += '''  <text x="128" y="92" font-size="13" fill="#667085">Подключения  /  Prod  /  Проверка</text>
  <text x="128" y="124" class="title">Проверка подключения «Prod»</text>
  <text x="128" y="154" class="small muted">Шаги выполняются по порядку; следующий шаг не начинается, пока не пройден предыдущий.</text>
  <rect x="128" y="176" width="900" height="620" rx="16" fill="#ffffff" stroke="#d0d5dd"/>
'''
steps = [
    ("circle-check", "#1f6b43", "1. Адрес и TLS", "Соединение установлено, сертификат проверен по системному хранилищу.", None),
    ("circle-check", "#1f6b43", "2. API Zabbix", "Найден API Zabbix 7.4.16 по адресу …/zabbix/api_jsonrpc.php.", None),
    ("circle-x", "#a32935", "3. Вход", "Не удалось войти. Проверьте учётные данные. Для учётных записей LDAP", "каталог тоже может быть недоступен."),
    ("circle-dashed", "#98a2b3", "4. Идентичность сервера", "Не проверялось: нужен успешный вход.", None),
    ("circle-dashed", "#98a2b3", "5. Права и возможности", "Не проверялось: нужен успешный вход.", None),
]
y = 204
for icon, color, title, line1, line2 in steps:
    fail = icon == "circle-x"
    h = 150 if fail else 92
    bg = "#fff5f5" if fail else "#ffffff"
    st = "#f2b8b5" if fail else "#eaecf0"
    s += f'  <rect x="152" y="{y}" width="852" height="{h - 12}" rx="12" fill="{bg}" stroke="{st}"/>\n'
    s += f'  <use href="#ti-{icon}" x="172" y="{y + 18}" width="24" height="24" color="{color}"/>\n'
    s += f'  <text x="210" y="{y + 36}" class="h3"{" fill=\"#a32935\"" if fail else ""}>{title}</text>\n'
    s += f'  <text x="210" y="{y + 62}" class="small{"" if fail else " muted"}">{line1}</text>\n'
    if line2:
        s += f'  <text x="210" y="{y + 82}" class="small">{line2}</text>\n'
    if fail:
        s += button(210, y + 92, 230, "Изменить данные входа", primary=True, h=38)
        s += button(452, y + 92, 130, "Повторить", h=38)
    y += h
s += '''
  <rect x="1052" y="176" width="420" height="376" rx="16" fill="#ffffff" stroke="#d0d5dd"/>
  <text x="1076" y="212" class="label muted">ТЕХНИЧЕСКИЕ СВЕДЕНИЯ</text>
  <text x="1076" y="248" class="small muted">Код</text>
  <text x="1170" y="248" class="mono">auth_failed</text>
  <text x="1076" y="276" class="small muted">Шаг</text>
  <text x="1170" y="276" class="mono">authentication</text>
  <text x="1076" y="304" class="small muted">Способ</text>
  <text x="1170" y="304" class="small">имя пользователя и пароль</text>
  <text x="1076" y="332" class="small muted">Время</text>
  <text x="1170" y="332" class="small">25.10.2026, 14:12</text>
  <rect x="1076" y="360" width="20" height="20" rx="4" fill="#ffffff" stroke="#7a8d9b" stroke-width="2"/>
  <text x="1106" y="376" class="small">Включить адрес сервера в отчёт</text>
'''
s += button(1076, 400, 372, "Копировать отчёт")
s += '''  <text x="1076" y="476" class="xs muted">В отчёте нет пароля, токена, ответов сервера</text>
  <text x="1076" y="494" class="xs muted">и путей к файлам; адрес — только по галке.</text>
  <text x="1076" y="530" class="xs muted">Повтор входа: Zabbix блокирует вход на 30 с после 5 ошибок.</text>
</svg>
'''
finish(s, OUT + "05-03_P1-02D_connection-test.svg", "offline")

# ------------------------------------------------------------------ 05-04 enable writes
s = head("разрешить запись панелей", "Светлый макет подтверждения перехода подключения в режим чтения-записи поверх списка подключений: что станет доступно, что не изменяется никогда, когда режим сбрасывается.",
         ["plus", "alert-triangle"]) + list_body()
s = s.replace("</svg>\n", "")
s += '''
  <!-- Dialog -->
  <rect x="0" y="72" width="1600" height="928" fill="#17213a" fill-opacity=".45"/>
  <rect x="460" y="200" width="680" height="560" rx="16" fill="#ffffff" stroke="#c9d3ea"/>
  <text x="496" y="250" class="title">Разрешить запись панелей?</text>
  <text x="496" y="284" class="small muted">Подключение «Лаборатория 7.4» · Zabbix 7.4.16 · учётная запись подключения</text>
  <text x="496" y="330" class="label muted">СТАНЕТ ДОСТУПНО</text>
  <text x="496" y="360" class="body">Создание панели, сохранение копии, клонирование, добавление</text>
  <text x="496" y="384" class="body">страниц и виджетов, замена узла сети в панели.</text>
  <text x="496" y="412" class="small muted">Каждая запись — только после предпросмотра, проверки и подтверждения плана.</text>
  <text x="496" y="458" class="label muted">НЕ ИЗМЕНЯЕТСЯ НИКОГДА</text>
  <text x="496" y="488" class="body">Узлы сети, шаблоны, элементы данных, триггеры и пользователи.</text>
  <text x="496" y="534" class="label muted">СРОК ДЕЙСТВИЯ</text>
  <text x="496" y="564" class="body">До отключения или перезапуска приложения, затем снова «Только чтение».</text>
  <text x="496" y="588" class="small muted">Вернуть «Только чтение» можно в любой момент кнопкой в шапке.</text>
  <rect x="496" y="618" width="608" height="56" rx="10" fill="#fff8ed" stroke="#f3d3a6"/>
  <use href="#ti-alert-triangle" x="512" y="630" width="18" height="18" color="#8a4b00"/>
  <text x="540" y="645" class="small" fill="#5c3a00">Права учётной записи в Zabbix не расширяются: недоступное ей</text>
  <text x="540" y="665" class="small" fill="#5c3a00">останется недоступным и в ZDashstitch.</text>
'''
s += button(756, 694, 120, "Отмена")
s += button(888, 694, 216, "Разрешить запись панелей", primary=True)
s += "</svg>\n"
finish(s, OUT + "05-04_P1-02E_enable-writes.svg", LAB_RO)
