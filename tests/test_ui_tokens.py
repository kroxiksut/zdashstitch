from __future__ import annotations

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TOKENS = (ROOT / "ui" / "src" / "tokens.css").read_text(encoding="utf-8")


def _rgb(value: str) -> tuple[float, float, float]:
    return tuple(int(value[index : index + 2], 16) / 255 for index in (1, 3, 5))  # type: ignore[return-value]


def _luminance(value: str) -> float:
    channels = [channel / 12.92 if channel <= 0.04045 else ((channel + 0.055) / 1.055) ** 2.4 for channel in _rgb(value)]
    return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]


def _contrast(foreground: str, background: str) -> float:
    light, dark = sorted((_luminance(foreground), _luminance(background)), reverse=True)
    return (light + 0.05) / (dark + 0.05)


def _block(selector: str) -> dict[str, str]:
    match = re.search(re.escape(selector) + r"\s*\{(?P<body>.*?)\n\}", TOKENS, re.DOTALL)
    assert match is not None
    return dict(re.findall(r"--([a-z0-9-]+):\s*(#[0-9a-f]{6});", match.group("body")))


def test_light_and_dark_core_text_and_action_pairs_meet_wcag_aa() -> None:
    light = _block(":root,\n:root[data-theme='light']")
    dark = _block(":root[data-theme='dark']")
    for theme in (light, dark):
        assert _contrast(theme["text-primary"], theme["surface-base"]) >= 4.5
        assert _contrast(theme["text-secondary"], theme["surface-base"]) >= 4.5
        assert _contrast(theme["text-link"], theme["surface-base"]) >= 4.5
        assert _contrast(theme["action-primary-text"], theme["action-primary-bg"]) >= 4.5
        for status in ("success", "info", "warning", "danger"):
            assert _contrast(theme[f"status-{status}-text"], theme[f"status-{status}-surface"]) >= 4.5


HIGH_CONTRAST = ":root[data-theme='high_contrast']"
# Every text token on every surface it is drawn on; text >= 7:1 (owner decision O-038).
HIGH_CONTRAST_TEXT_PAIRS = (
    *((text, surface) for text in ("text-primary", "text-secondary", "text-muted", "text-link", "text-code")
      for surface in ("surface-canvas", "surface-base", "surface-raised", "surface-sunken", "surface-overlay",
                      "surface-selected")),
    ("text-disabled", "surface-base"),
    ("text-disabled", "surface-disabled"),
    ("text-primary", "selection-fill"),
    ("text-inverse", "action-primary-bg"),
    *(("action-primary-text", f"action-primary-{state}") for state in ("bg", "hover", "pressed")),
    *(("action-secondary-text", f"action-secondary-{state}") for state in ("bg", "hover", "pressed")),
    *(("action-ghost-text", f"action-ghost-{state}") for state in ("hover", "pressed")),
    ("action-ghost-text", "surface-base"),
    *(("action-destructive-text", f"action-destructive-{state}") for state in ("bg", "hover", "pressed")),
    *((f"status-{status}-text", f"status-{status}-surface") for status in ("success", "info", "warning", "danger")),
    *((f"status-{status}-text", "surface-base") for status in ("success", "info", "warning", "danger")),
    *((f"diff-{kind}", "surface-base") for kind in ("added", "changed", "removed", "unknown")),
)
# Borders, focus, icons and grid lines (non-text UI): >= 4.5:1 against the surfaces they separate.
HIGH_CONTRAST_UI_PAIRS = (
    *((border, surface) for border in ("border-subtle", "border-default", "border-strong", "border-selected",
                                       "focus-ring", "grid-line", "grid-major", "grid-bounds", "selection-border")
      for surface in ("surface-canvas", "surface-base", "surface-sunken", "surface-selected")),
    ("border-disabled", "surface-base"),
    ("border-disabled", "surface-disabled"),
    ("focus-ring", "focus-offset"),
    *((f"status-{status}-{part}", f"status-{status}-surface")
      for status in ("success", "info", "warning", "danger") for part in ("border", "icon")),
    *((f"status-{status}-border", "surface-base") for status in ("success", "info", "warning", "danger")),
)


def test_high_contrast_theme_meets_owner_thresholds() -> None:
    theme = _block(HIGH_CONTRAST)
    light = _block(":root,\n:root[data-theme='light']")
    color_tokens = {name for name in light if not name.startswith("theme-preview-")}
    # Every hex colour token of the light theme is redefined.
    assert color_tokens - set(theme) == set()
    failures = []
    for foreground, background in HIGH_CONTRAST_TEXT_PAIRS:
        ratio = _contrast(theme[foreground], theme[background])
        if ratio < 7:
            failures.append(f"text {foreground} / {background}: {ratio:.2f}")
    for foreground, background in HIGH_CONTRAST_UI_PAIRS:
        ratio = _contrast(theme[foreground], theme[background])
        if ratio < 4.5:
            failures.append(f"ui {foreground} / {background}: {ratio:.2f}")
    assert failures == []


def test_high_contrast_theme_is_light_with_a_thicker_focus_ring_and_yields_to_forced_colors() -> None:
    match = re.search(re.escape(HIGH_CONTRAST) + r"\s*\{(?P<body>.*?)\n\}", TOKENS, re.DOTALL)
    assert match is not None
    body = match.group("body")
    assert "color-scheme: light;" in body
    assert re.search(r"--focus-width:\s*3px;", body)
    assert "--shadow-raised: none;" in body and "--shadow-overlay: none;" in body
    # The forced-colors block comes after every theme and outranks `:root[data-theme=…]`.
    forced = TOKENS.index("@media (forced-colors: active)")
    assert forced > TOKENS.index(HIGH_CONTRAST)
    assert ":root[data-theme] {" in TOKENS[forced:]


def test_explicit_reduced_motion_overrides_the_system_preference() -> None:
    assert ":root[data-motion='reduce']" in TOKENS
    assert ":root:not([data-motion='no-preference'])" in TOKENS


def test_system_forced_colors_and_reduced_motion_are_css_driven() -> None:
    assert "prefers-color-scheme: dark" in TOKENS
    assert ":root[data-theme='system']" in TOKENS
    assert "forced-colors: active" in TOKENS
    assert "prefers-reduced-motion: reduce" in TOKENS
