"""Shared app bar for review mockups: global navigation with Settings."""
import re

SETTINGS_SYMBOL = (
    '    <symbol id="ti-settings" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" '
    'stroke-linecap="round" stroke-linejoin="round"><path d="M10.325 4.317c.426 -1.756 2.924 -1.756 3.35 0a1.724 '
    '1.724 0 0 0 2.573 1.066c1.543 -.94 3.31 .826 2.37 2.37a1.724 1.724 0 0 0 1.065 2.572c1.756 .426 1.756 2.924 0 '
    '3.35a1.724 1.724 0 0 0 -1.066 2.573c.94 1.543 -.826 3.31 -2.37 2.37a1.724 1.724 0 0 0 -2.572 1.065c-.426 1.756 '
    '-2.924 1.756 -3.35 0a1.724 1.724 0 0 0 -2.573 -1.066c-1.543 .94 -3.31 -.826 -2.37 -2.37a1.724 1.724 0 0 0 '
    '-1.065 -2.572c-1.756 -.426 -1.756 -2.924 0 -3.35a1.724 1.724 0 0 0 1.066 -2.573c-.94 -1.543 .826 -3.31 2.37 '
    '-2.37c1 .608 2.296 .07 2.572 -1.065"/><path d="M9 12a3 3 0 1 0 6 0a3 3 0 0 0 -6 0"/></symbol>\n'
)

TABS = [  # key, label, x, approximate width at 15px
    ("start", "Начало", 232, 54),
    ("drafts", "Черновики", 318, 80),
    ("server", "Панели в Zabbix", 430, 120),
    ("library", "Библиотека", 582, 88),
    ("connections", "Подключения", 702, 100),
]

MODES = {
    "offline": ("Автономно", "#eef4ff", "#3156a3", 120),
    "read_only": ("Только чтение", "#eef4ff", "#3156a3", 150),
    "lab_read_only": ("Только чтение · Лаборатория 7.4", "#eef4ff", "#3156a3", 260),
}


def app_bar(active, mode, brand_href, settings_active=False):
    out = ['  <!-- App bar: global navigation (P1-01B §1) -->',
           '  <rect width="1600" height="72" fill="#ffffff" stroke="#e4e7ec"/>',
           f'  <image href="{brand_href}" x="32" y="18" width="36" height="36" preserveAspectRatio="xMidYMid meet"/>',
           '  <text x="80" y="45" font-size="20" font-weight="750">ZDashstitch</text>']
    for key, label, x, w in TABS:
        if key == active:
            out.append(f'  <text x="{x}" y="44" font-size="15" font-weight="700" fill="#1d3f86">{label}</text>')
            out.append(f'  <rect x="{x}" y="67" width="{w}" height="4" rx="2" fill="#3156a3"/>')
        else:
            out.append(f'  <text x="{x}" y="44" font-size="15" font-weight="500" fill="#475467">{label}</text>')
    # mode is a MODES key or a custom (text, fill, color, width) tuple
    text, fill, color, w = MODES[mode] if isinstance(mode, str) else mode
    x = 1408 - w
    out.append(f'  <rect x="{x}" y="19" width="{w}" height="34" rx="17" fill="{fill}"/>')
    out.append(f'  <text x="{x + w / 2:.0f}" y="41" font-size="14" text-anchor="middle" fill="{color}">{text}</text>')
    if settings_active:
        out.append('  <rect x="1424" y="18" width="144" height="36" rx="8" fill="#eef4ff" stroke="#3156a3"/>')
        out.append('  <use href="#ti-settings" x="1438" y="27" width="18" height="18" color="#1d3f86"/>')
        out.append('  <text x="1464" y="41" font-size="15" font-weight="700" fill="#1d3f86">Настройки</text>')
    else:
        out.append('  <rect x="1424" y="18" width="144" height="36" rx="8" fill="#ffffff" stroke="#c9d3ea"/>')
        out.append('  <use href="#ti-settings" x="1438" y="27" width="18" height="18" color="#475467"/>')
        out.append('  <text x="1464" y="41" font-size="15" font-weight="600" fill="#17213a">Настройки</text>')
    return "\n".join(out) + "\n"


def draft_context(breadcrumb, chips):
    """Breadcrumb above the title and draft status chips on the right of the title row."""
    out = [f'  <text x="292" y="92" font-size="13" fill="#667085">{breadcrumb}</text>']
    x_end = 1544
    for text, fill, color, w in reversed(chips):
        x = x_end - w
        out.append(f'  <rect x="{x}" y="100" width="{w}" height="28" rx="14" fill="{fill}"/>')
        out.append(f'  <text x="{x + w / 2:.0f}" y="119" font-size="13" font-weight="650" text-anchor="middle" fill="{color}">{text}</text>')
        x_end = x - 10
    return "\n".join(out) + "\n"


DIRTY = ("Изменена", "#fff4e5", "#8a4b00", 100)
UNCHECKED = ("Не проверена", "#f2f4f7", "#344054", 130)


def apply(svg, *, active, mode, depth_prefix, breadcrumb=None, chips=(), settings_active=False):
    # scale to the window: keep viewBox, drop fixed pixel size
    svg = re.sub(r'^<svg xmlns="http://www.w3.org/2000/svg" width="\d+" height="\d+"',
                 '<svg xmlns="http://www.w3.org/2000/svg" width="100%"', svg, count=1)
    brand = depth_prefix + "assets/brand/icons/stezhik-head-1024.png"
    svg = re.sub(r'href="(\.\./)+assets/brand/', 'href="' + depth_prefix + 'assets/brand/', svg)
    if 'id="ti-settings"' not in svg:
        svg = svg.replace("    <style>", SETTINGS_SYMBOL + "    <style>", 1)
    start = svg.index("  <!-- App bar")
    end = svg.index("  <!--", start + 10)
    svg = svg[:start] + app_bar(active, mode, brand, settings_active) + "\n" + svg[end:]
    if breadcrumb:
        marker = "  <!-- Title -->\n"
        assert marker in svg
        svg = svg.replace(marker, marker + draft_context(breadcrumb, chips), 1)
    return svg
