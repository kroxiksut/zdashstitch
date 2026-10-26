"""Review storyboard 00-02: where the Stezhik mascot may appear and how it moves (P1-10).

Uses only the existing in-house artwork from assets/brand/mascot; no new images are drawn.

Usage (from the repository root):
    .venv/Scripts/python.exe docs/design/tools/motion_svg.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from publication_svg import STYLE, box, chip, icon, symbol, t  # noqa: E402

OUT = "docs/design/assets/00-foundations/00-02_P1-10_mascot-states.svg"
MASCOT = "../../../../assets/brand/mascot/"
GREEN = ("#ecfdf3", "#1f6b43")
NEUTRAL = ("#f2f4f7", "#344054")
BLUE = ("#eef4ff", "#1d3f86")


def lines(x, y, items, cls="small", fill=None, step=20):
    return "".join(t(x, y + i * step, v, cls, fill) for i, v in enumerate(items))


def header():
    icons = ["circle-check", "ban", "info-circle", "alert-triangle"]
    syms = "".join(symbol(n) for n in icons)
    return ('<svg xmlns="http://www.w3.org/2000/svg" width="100%" viewBox="0 0 1600 1000" fill="#17213a" role="img" aria-labelledby="title desc">\n'
            '  <title id="title">ZDashstitch — Стежик: состояния и движение</title>\n'
            '  <desc id="desc">Раскадровка P1-10: существующие изображения Стежика в пяти разрешённых состояниях с поводом '
            'появления, экраном, движением и временем; токены времени; список мест, где персонаж запрещён; поведение при '
            'уменьшенной анимации и высокой контрастности.</desc>\n  <defs>\n'
            f'    <!-- Tabler Icons outline v3.49.0 (MIT), see assets/icons/THIRD_PARTY.md -->\n{syms}{STYLE}  </defs>\n'
            '  <rect width="1600" height="1000" fill="#f5f7fb"/>\n')


CARDS = [
    ("stezhik-wave.png", "Приветствие", "«Начало» при первом запуске;", "пустые «Черновики»",
     "01-01, 02-01", "вход 320 мс; покачивание", "2,4 с × 3, затем статично", "decorative"),
    ("stezhik-chart.png", "Долгая безопасная операция", "копирование, импорт, экспорт", "шаблона: этап дольше 2 с",
     "03-03, 02-02, 03-02", "вход 320 мс; покачивание", "до 3 циклов, уход 160 мс", "рядом с этапом"),
    ("stezhik-compare.png", "Привязки ещё не заданы", "«Привязки»: пустое состояние", "до первого соответствия",
     "«Привязки», пусто", "вход 320 мс", "дальше статично", "decorative"),
    ("stezhik-inspect.png", "Находок нет", "«Проверки»: после текста", "«Находок нет»",
     "P1-04F «Проверки»", "вход 320 мс", "без повторов", "рядом с итогом"),
    ("stezhik-success.png", "Проверено", "только после итога", "«Проверено повторным чтением»",
     "11-07, сверка 11-08", "однократный подъём", "480 мс, не повторяется", "рядом с итогом"),
]


def cards():
    s = t(48, 128, "РАЗРЕШЕНО: ПЯТЬ СОСТОЯНИЙ, ТОЛЬКО ГОТОВЫЕ ИЗОБРАЖЕНИЯ", "label muted")
    x = 48
    w = 288
    for img, name, l1, l2, screens, m1, m2, kind in CARDS:
        s += box(x, 144, w, 470, rx=14)
        s += box(x + 16, 160, w - 32, 196, fill="#eef2f8", stroke="#e4e7ec", rx=10)
        s += (f'  <image href="{MASCOT}{img}" x="{x + w / 2 - 92:.0f}" y="166" width="184" height="184" '
              f'preserveAspectRatio="xMidYMid meet" aria-hidden="true"/>\n')
        s += t(x + 20, 386, name, "h3")
        s += lines(x + 20, 412, [l1, l2], "small")
        s += t(x + 20, 466, "Экран", "xs muted") + t(x + 96, 466, screens, "xs", weight="650")
        s += t(x + 20, 492, "Движение", "xs muted") + t(x + 96, 492, m1, "xs", weight="650")
        s += t(x + 96, 510, m2, "xs", weight="650")
        tone = BLUE if kind == "decorative" else NEUTRAL
        label = "декоративно, скрыт от диктора" if kind == "decorative" else "рядом с текстом, смысл — в тексте"
        s += chip(x + 20, 532, w - 40, label, tone)
        s += t(x + 20, 590, img, "mono")
        x += w + 16
    return s


def timing():
    s = box(48, 632, 760, 320, rx=14)
    s += t(72, 666, "ТОКЕНЫ ВРЕМЕНИ", "label muted")
    rows = [("--mascot-enter", "320 мс", "cubic-bezier(.2, .8, .2, 1)", "появление: прозрачность 0→1, сдвиг вверх 8 px"),
            ("--mascot-idle", "2400 мс × 3", "ease-in-out", "покачивание ±4 px по вертикали, затем статично"),
            ("--mascot-success", "480 мс × 1", "cubic-bezier(.3, 1.4, .5, 1)", "подъём 6 px и возврат, один раз"),
            ("--mascot-exit", "160 мс", "ease-in", "исчезновение при завершении этапа или уходе"),
            ("--mascot-delay", "2000 мс", "—", "долгая операция: персонаж не раньше кнопки «Отмена»")]
    y = 696
    for tok, dur, ease, what in rows:
        s += t(72, y, tok, "mono") + t(250, y, dur, "small", weight="650") + t(372, y, ease, "mono")
        s += t(72, y + 20, what, "xs muted")
        y += 46
    s += t(72, 932, "Только transform и opacity; анимация не показывает ход операции и не ждёт её окончания.", "xs muted")
    return s


def forbidden():
    s = box(828, 632, 724, 228, fill="#fef3f2", stroke="#f2b8b5", rx=14)
    s += icon("ban", 848, 650, "#a32935", 22)
    s += t(880, 666, "ЗАПРЕЩЕНО — ПЕРСОНАЖА НЕТ", "label", "#a32935")
    left = ["ошибки, сбой сохранения, отказ импорта", "конфликт (11-05), неизвестный итог (11-08)",
            "подтверждение записи (11-06), «Разрешить", "запись панелей» (05-04), удаление"]
    right = ["этапы записи и повторного чтения", "редактор, сравнение, таблицы соответствий",
             "проверки с блокировками, нет связи", "диалоги входа, ввод паролей и токенов"]
    s += lines(852, 698, ["•  " + v for v in left], "small", "#5c1f1f", 26)
    s += lines(1196, 698, ["•  " + v for v in right], "small", "#5c1f1f", 26)
    s += t(852, 840, "В этих местах нужна сосредоточенность: эмоция персонажа не должна подталкивать к действию.", "xs", "#5c1f1f")
    s += box(828, 876, 724, 76, fill="#ffffff", stroke="#d0d5dd", rx=14)
    s += icon("info-circle", 848, 892, "#475467", 20)
    s += t(878, 906, "Уменьшенная анимация — статичный кадр, без движения. Высокая контрастность — персонажа нет.", "small")
    s += t(878, 930, "«Показывать Стежика» в «Настройки → Внешний вид»; без него экраны полностью понятны.", "small")
    return s


def main():
    s = header()
    s += t(48, 64, "Стежик: где появляется и как движется", "title")
    s += t(48, 94, "P1-10, раскадровка. Используются только готовые изображения assets/brand/mascot; новых изображений нет. "
                   "Статус всегда передаёт текст интерфейса.", "small muted")
    s += cards() + timing() + forbidden()
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="") as f:
        f.write(s + "</svg>\n")
    print("written", OUT)


if __name__ == "__main__":
    main()
