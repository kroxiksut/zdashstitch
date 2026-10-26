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


def test_system_forced_colors_and_reduced_motion_are_css_driven() -> None:
    assert "prefers-color-scheme: dark" in TOKENS
    assert ":root[data-theme='system']" in TOKENS
    assert "forced-colors: active" in TOKENS
    assert "prefers-reduced-motion: reduce" in TOKENS
