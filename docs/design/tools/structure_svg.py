"""Review mockup 08-01: «Структура» локальной панели (P1-01B §2a).

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/structure_svg.py
"""
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import appbar  # noqa: E402

out = sys.argv[1] if len(sys.argv) > 1 else "docs/design/assets/08-structure/08-01_P1-01B_dashboard-structure.svg"
S = []
a = S.append
SYM = 'viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"'
a(f'''<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1000" viewBox="0 0 1600 1000" fill="#17213a" role="img" aria-labelledby="title desc">
  <title id="title">ZDashstitch — структура панели</title>
  <desc id="desc">Светлый макет раздела «Структура» локального черновика панели: дерево как в Zabbix (свойства панели, страницы, виджеты с типом, положением и размером в сетке 72 колонки), предпросмотр выбранной страницы в пропорциях сетки Zabbix с включёнными синтетическими данными и меткой совместимости Zabbix 7.4.0+.</desc>
  <defs>
    <!-- Tabler Icons outline v3.49.0 (MIT), see assets/icons/THIRD_PARTY.md -->
    <symbol id="ti-grip-vertical" {SYM}><path d="M8 5a1 1 0 1 0 2 0a1 1 0 1 0 -2 0"/><path d="M8 12a1 1 0 1 0 2 0a1 1 0 1 0 -2 0"/><path d="M8 19a1 1 0 1 0 2 0a1 1 0 1 0 -2 0"/><path d="M14 5a1 1 0 1 0 2 0a1 1 0 1 0 -2 0"/><path d="M14 12a1 1 0 1 0 2 0a1 1 0 1 0 -2 0"/><path d="M14 19a1 1 0 1 0 2 0a1 1 0 1 0 -2 0"/></symbol>
    <symbol id="ti-arrow-up" {SYM}><path d="M12 5l0 14"/><path d="M18 11l-6 -6"/><path d="M6 11l6 -6"/></symbol>
    <symbol id="ti-arrow-down" {SYM}><path d="M12 5l0 14"/><path d="M18 13l-6 6"/><path d="M6 13l6 6"/></symbol>
    <symbol id="ti-chevron-down" {SYM}><path d="M6 9l6 6l6 -6"/></symbol>
    <symbol id="ti-chevron-right" {SYM}><path d="M9 6l6 6l-6 6"/></symbol>
    <symbol id="ti-check" {SYM}><path d="M5 12l5 5l10 -10"/></symbol>
    <style>
      text{{font-family:system-ui,-apple-system,"Segoe UI",sans-serif}}
      .muted{{fill:#667085}}.white{{fill:#ffffff}}.small{{font-size:14px}}.xs{{font-size:12px}}.label{{font-size:13px;font-weight:700;letter-spacing:.08em}}
      .title{{font-size:24px;font-weight:750}}.h3{{font-size:17px;font-weight:700}}.body{{font-size:16px}}.button{{font-size:15px;font-weight:700}}
      .nav{{font-size:17px}}.badge{{font-size:13px;font-weight:650}}.row{{font-size:15px;font-weight:650}}
    </style>
  </defs>
  <rect width="1600" height="1000" fill="#f5f7fb"/>

  <!-- App bar -->
  <rect width="1600" height="72" fill="#ffffff" stroke="#e4e7ec"/>
  <image href="../../../assets/brand/icons/stezhik-head-1024.png" x="32" y="18" width="36" height="36" preserveAspectRatio="xMidYMid meet"/>
  <text x="80" y="45" font-size="20" font-weight="750">ZDashstitch</text>
  <text x="236" y="44" class="body muted">Черновики  /  Базы данных: сводная  /  Структура</text>
  <rect x="1158" y="18" width="110" height="34" rx="17" fill="#fff4e5"/>
  <text x="1213" y="40" class="small" text-anchor="middle" fill="#8a4b00">Изменена</text>
  <rect x="1278" y="18" width="120" height="34" rx="17" fill="#eef4ff"/>
  <text x="1338" y="40" class="small" text-anchor="middle" fill="#3156a3">Автономно</text>
  <rect x="1408" y="18" width="160" height="34" rx="17" fill="#f2f4f7"/>
  <text x="1488" y="40" class="small" text-anchor="middle" fill="#344054">Не проверена</text>

  <!-- Workspace navigation -->
  <rect x="0" y="72" width="260" height="928" fill="#ffffff" stroke="#e4e7ec"/>
  <text x="28" y="116" class="label muted">ПАНЕЛЬ</text>
  <text x="36" y="164" class="nav">Обзор</text>
  <rect x="16" y="184" width="228" height="46" rx="10" fill="#eef4ff"/>
  <text x="36" y="214" class="nav" font-weight="700" fill="#1d3f86">Структура</text>
  <text x="36" y="264" class="nav">Привязки</text>
  <text x="36" y="312" class="nav">Проверки</text>
  <text x="36" y="360" class="nav">Публикация</text>
  <line x1="20" y1="392" x2="240" y2="392" stroke="#e4e7ec"/>
  <text x="28" y="428" class="label muted">ПРОИСХОЖДЕНИЕ</text>
  <text x="28" y="458" class="body" font-weight="650">Локальная</text>
  <text x="28" y="484" class="small muted">собрана из 3 страниц</text>
  <text x="28" y="508" class="small muted">19 виджетов</text>

  <!-- Title -->
  <text x="292" y="120" class="title">Базы данных: сводная</text>
  <rect x="292" y="136" width="84" height="28" rx="14" fill="#eef4ff"/>
  <text x="334" y="155" class="badge" text-anchor="middle" fill="#3156a3">Панель</text>
  <rect x="386" y="136" width="110" height="28" rx="14" fill="#ecfdf3"/>
  <text x="441" y="155" class="badge" text-anchor="middle" fill="#1f6b43">Локальная</text>
  <rect x="506" y="136" width="124" height="28" rx="14" fill="#ffffff" stroke="#7a8d9b"/>
  <text x="568" y="155" class="badge" text-anchor="middle" fill="#344054">Zabbix 7.4.0+</text>
  <text x="644" y="155" class="small muted">Структура как в Zabbix. Внешний вид виджетов настраивается отдельно.</text>

  <!-- Structure tree -->
  <rect x="292" y="184" width="520" height="716" rx="16" fill="#ffffff" stroke="#d0d5dd"/>
  <text x="316" y="216" class="label muted">СТРУКТУРА ПАНЕЛИ</text>
  <text x="316" y="240" class="small muted">Положение «x, y» и размер «ширина × высота» — в клетках сетки.</text>

  <rect x="316" y="256" width="472" height="72" rx="10" fill="#f8fafc" stroke="#e4e7ec"/>
  <use href="#ti-chevron-down" x="324" y="268" width="18" height="18" color="#475467"><title>Свернуть</title></use>
  <text x="350" y="283" class="h3">Базы данных: сводная</text>
  <text x="772" y="283" class="small" font-weight="650" text-anchor="end" fill="#3156a3">Свойства панели</text>
  <text x="350" y="310" class="small muted">Период страницы по умолчанию: 30 секунд · слайд-шоу вкл.</text>
''')


def page_row(y, num, name, meta, expanded=False, selected=False):
    fill = '#eef4ff' if selected else '#ffffff'
    stroke = '#c9d3ea' if selected else '#e4e7ec'
    chev = 'ti-chevron-down' if expanded else 'ti-chevron-right'
    tip = 'Свернуть' if expanded else 'Развернуть'
    a(f'  <rect x="332" y="{y}" width="456" height="44" rx="10" fill="{fill}" stroke="{stroke}"/>')
    a(f'  <use href="#{chev}" x="340" y="{y + 13}" width="18" height="18" color="#475467"><title>{tip}</title></use>')
    a(f'  <use href="#ti-grip-vertical" x="362" y="{y + 12}" width="20" height="20" color="#98a2b3"><title>Перетащить страницу</title></use>')
    a(f'  <text x="390" y="{y + 28}" class="row">{num}. {name}</text>')
    if selected:
        a(f'  <text x="700" y="{y + 27}" class="small muted" text-anchor="end">{meta}</text>')
        a(f'  <use href="#ti-arrow-up" x="716" y="{y + 12}" width="20" height="20" color="#475467"><title>Переместить страницу выше</title></use>')
        a(f'  <use href="#ti-arrow-down" x="748" y="{y + 12}" width="20" height="20" color="#475467"><title>Переместить страницу ниже</title></use>')
    else:
        a(f'  <text x="772" y="{y + 27}" class="small muted" text-anchor="end">{meta}</text>')


def widget_row(y, name, sub, note=None, selected=False):
    if selected:
        a(f'  <rect x="348" y="{y}" width="440" height="46" rx="8" fill="#ffffff" stroke="#3156a3" stroke-width="2"/>')
    else:
        a(f'  <rect x="348" y="{y}" width="440" height="46" rx="8" fill="#ffffff" stroke="#eaecf0"/>')
    a(f'  <use href="#ti-grip-vertical" x="354" y="{y + 14}" width="18" height="18" color="#98a2b3"><title>Перетащить виджет</title></use>')
    a(f'  <text x="380" y="{y + 20}" class="body" font-weight="600">{name}</text>')
    a(f'  <text x="380" y="{y + 38}" class="xs muted">{sub}</text>')
    if note:
        a(f'  <text x="772" y="{y + 20}" class="xs" font-weight="650" text-anchor="end" fill="#475467">{note}</text>')


page_row(340, 1, 'PostgreSQL', '8 виджетов · по умолчанию')
page_row(392, 2, 'ClickHouse', '5 виджетов · 1 минута', expanded=True, selected=True)
widgets = [
    ('Connections', 'График · 0, 0 · 36 × 5', None, False),
    ('Queries per second', 'График · 36, 0 · 36 × 5', None, True),
    ('Replication lag', 'Значение элемента данных · 0, 5 · 24 × 3', None, False),
    ('Disk usage', 'Радиальный датчик · 24, 5 · 24 × 3', 'Изменение не поддерживается', False),
    ('Top queries', 'Топ узлов сети · 48, 5 · 24 × 3', 'Изменение не поддерживается', False),
]
y = 444
for name, sub, note, sel in widgets:
    widget_row(y, name, sub, note, sel)
    y += 50
page_row(700, 3, 'Keeper и HAProxy', '6 виджетов · по умолчанию')
a('''
  <rect x="316" y="764" width="168" height="42" rx="10" fill="#ffffff" stroke="#3156a3" stroke-width="2"/>
  <text x="400" y="790" class="button" text-anchor="middle" fill="#3156a3">Добавить страницу</text>
  <rect x="494" y="764" width="112" height="42" rx="10" fill="#ffffff" stroke="#c9d3ea"/>
  <text x="550" y="790" class="button" text-anchor="middle" fill="#17213a">Копировать</text>
  <rect x="616" y="764" width="168" height="42" rx="10" fill="#ffffff" stroke="#c9d3ea"/>
  <text x="700" y="790" class="button" text-anchor="middle" fill="#17213a">Вставить страницу</text>
  <text x="316" y="846" class="small muted">Здесь: порядок, имена, страницы и перенос виджетов.</text>
  <text x="316" y="868" class="small muted">Положение, размер и поля виджета — в редакторе («Изменить»).</text>
''')

# Preview in Zabbix grid proportions: 72 columns over the full width,
# rows of 70 px at the chosen reference screen width.
X0, W = 860, 660.0
col = W / 72
row = 70 * W / 1920
Y0 = 316
ROWS = 10
H = ROWS * row
a('''  <!-- Page preview -->
  <rect x="836" y="184" width="708" height="716" rx="16" fill="#ffffff" stroke="#d0d5dd"/>
  <text x="860" y="216" class="label muted">ПРЕДПРОСМОТР СТРАНИЦЫ «CLICKHOUSE»</text>
  <rect x="860" y="236" width="20" height="20" rx="4" fill="#3156a3"/>
  <use href="#ti-check" x="862" y="238" width="16" height="16" color="#ffffff"/>
  <text x="890" y="252" class="body">Показать синтетические данные</text>
  <rect x="1296" y="230" width="224" height="34" rx="8" fill="#ffffff" stroke="#c9d3ea"/>
  <text x="1310" y="252" class="small">Ширина экрана: 1920 px</text>
  <use href="#ti-chevron-down" x="1494" y="238" width="18" height="18" color="#475467"/>
  <text x="860" y="296" class="xs muted">Сетка Zabbix: 72 колонки на всю ширину, строка 70 px. Показано строк: 10 из 64.</text>
''')
a(f'  <rect x="{X0}" y="{Y0}" width="{W:.1f}" height="{H:.1f}" fill="#f8fafc" stroke="#d0d5dd"/>')
for c in range(6, 72, 6):
    x = X0 + c * col
    a(f'  <line x1="{x:.1f}" y1="{Y0}" x2="{x:.1f}" y2="{Y0 + H:.1f}" stroke="#eaecf0"/>')
for r in range(1, ROWS):
    yy = Y0 + r * row
    a(f'  <line x1="{X0}" y1="{yy:.1f}" x2="{X0 + W:.1f}" y2="{yy:.1f}" stroke="#eaecf0"/>')


def wbox(x, y, w, h, name, sel=False):
    bx, by = X0 + x * col + 2, Y0 + y * row + 2
    bw, bh = w * col - 4, h * row - 4
    stroke = 'stroke="#3156a3" stroke-width="2"' if sel else 'stroke="#d0d5dd"'
    a(f'  <rect x="{bx:.1f}" y="{by:.1f}" width="{bw:.1f}" height="{bh:.1f}" rx="4" fill="#ffffff" {stroke}/>')
    a(f'  <text x="{bx + 8:.1f}" y="{by + 16:.1f}" class="xs" font-weight="650">{name}</text>')
    return bx, by, bw, bh


def synth_badge(bx, by, bw):
    a(f'  <rect x="{bx + bw - 78:.1f}" y="{by + 5:.1f}" width="72" height="17" rx="8.5" fill="#fff4e5"/>')
    a(f'  <text x="{bx + bw - 42:.1f}" y="{by + 17.5:.1f}" font-size="11" text-anchor="middle" fill="#8a4b00">Синтетика</text>')


def graph(bx, by, bw, bh, seed, color):
    px0, px1 = bx + 30, bx + bw - 10
    py0, py1 = by + 30, by + bh - 18
    a(f'  <line x1="{px0:.1f}" y1="{py1:.1f}" x2="{px1:.1f}" y2="{py1:.1f}" stroke="#d0d5dd"/>')
    a(f'  <line x1="{px0:.1f}" y1="{py0:.1f}" x2="{px0:.1f}" y2="{py1:.1f}" stroke="#d0d5dd"/>')
    pts = []
    n = 40
    for i in range(n + 1):
        t = i / n
        v = 0.55 + 0.25 * math.sin(t * 6.3 + seed) + 0.12 * math.sin(t * 17 + seed * 2) + 0.05 * math.cos(t * 31 + seed)
        v = min(max(v, 0.05), 0.95)
        pts.append(f'{px0 + t * (px1 - px0):.1f},{py1 - v * (py1 - py0):.1f}')
    a(f'  <polyline points="{" ".join(pts)}" fill="none" stroke="{color}" stroke-width="1.6"/>')


bx, by, bw, bh = wbox(0, 0, 36, 5, 'Connections')
graph(bx, by, bw, bh, 0.4, '#3156a3')
synth_badge(bx, by, bw)
bx, by, bw, bh = wbox(36, 0, 36, 5, 'Queries per second', sel=True)
graph(bx, by, bw, bh, 2.1, '#8b5cf6')
synth_badge(bx, by, bw)
bx, by, bw, bh = wbox(0, 5, 24, 3, 'Replication lag')
synth_badge(bx, by, bw)
a(f'  <text x="{bx + bw / 2:.1f}" y="{by + bh - 12:.1f}" font-size="24" font-weight="700" text-anchor="middle">0,8 с</text>')
bx, by, bw, bh = wbox(24, 5, 24, 3, 'Disk usage')
a(f'  <text x="{bx + 8:.1f}" y="{by + 36:.1f}" class="xs muted">Радиальный датчик</text>')
a(f'  <text x="{bx + 8:.1f}" y="{by + 54:.1f}" class="xs muted">мин. 0 · макс. 100 · %</text>')
bx, by, bw, bh = wbox(48, 5, 24, 3, 'Top queries')
a(f'  <text x="{bx + 8:.1f}" y="{by + 36:.1f}" class="xs muted">Топ узлов сети</text>')
a(f'  <text x="{bx + 8:.1f}" y="{by + 54:.1f}" class="xs muted">столбцов: 3 · узлов: 10</text>')

yb = round(Y0 + H + 22)
a(f'''
  <rect x="860" y="{yb}" width="660" height="66" rx="10" fill="#fff8ed" stroke="#f3d3a6"/>
  <text x="878" y="{yb + 26}" class="small" font-weight="700" fill="#8a4b00">Синтетические данные — не значения из Zabbix</text>
  <text x="878" y="{yb + 48}" class="small" fill="#5c3a00">Построены по типу значения и единицам. В файлы, экспорт и публикацию не попадают.</text>

  <text x="860" y="{yb + 108}" class="label muted">СОВМЕСТИМОСТЬ</text>
  <rect x="1010" y="{yb + 90}" width="124" height="28" rx="14" fill="#ffffff" stroke="#7a8d9b"/>
  <text x="1072" y="{yb + 109}" class="badge" text-anchor="middle" fill="#344054">Zabbix 7.4.0+</text>
  <text x="1148" y="{yb + 109}" class="small muted">по типам и полям виджетов; автор не указал</text>

  <line x1="860" y1="{yb + 134}" x2="1520" y2="{yb + 134}" stroke="#e4e7ec"/>
  <text x="860" y="{yb + 164}" class="label muted">ВЫБРАННЫЙ ВИДЖЕТ</text>
  <text x="860" y="{yb + 192}" class="h3">Queries per second</text>
  <text x="1040" y="{yb + 192}" class="small muted">График · положение 36, 0 · размер 36 × 5</text>
  <rect x="860" y="{yb + 210}" width="130" height="42" rx="10" fill="#3156a3"/>
  <text x="925" y="{yb + 236}" class="button white" text-anchor="middle">Изменить</text>
  <rect x="1002" y="{yb + 210}" width="236" height="42" rx="10" fill="#ffffff" stroke="#c9d3ea"/>
  <text x="1120" y="{yb + 236}" class="button" text-anchor="middle">Перенести на страницу…</text>
  <rect x="1250" y="{yb + 210}" width="120" height="42" rx="10" fill="#ffffff" stroke="#c9d3ea"/>
  <text x="1310" y="{yb + 236}" class="button" text-anchor="middle">Удалить</text>
</svg>
''')
svg = appbar.apply('\n'.join(S), active='drafts', mode='offline', depth_prefix='../../../../',
                   breadcrumb='Черновики  /  Базы данных: сводная  /  Структура',
                   chips=(appbar.DIRTY, appbar.UNCHECKED))
with open(out, 'w', encoding='utf-8', newline='') as f:
    f.write(svg)
print('written', out)
