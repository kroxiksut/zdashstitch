#!/usr/bin/env python3
"""Fail closed on unsafe UI patterns prohibited by the localization and CSP contracts."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "ui" / "src"
SOURCE_SUFFIXES = {".svelte", ".css", ".ts", ".html", ".js", ".mjs"}
COLOR_LITERAL = re.compile(r"#[0-9a-fA-F]{3,8}\b|\brgba?\(|\bhsla?\(")
NAMED_COLOR = re.compile(
    r"(?:\bcolor|\bbackground(?:-color)?|\bborder(?:-(?:top|right|bottom|left))?(?:-color)?)"
    r"\s*:[^;{}]*\b(?:red|blue|green|black|white|gray|grey|yellow|orange|purple|pink|brown|"
    r"cyan|magenta|navy|teal|lime|maroon|olive|aqua|fuchsia|silver)\b",
    re.IGNORECASE,
)
SCRIPT_BLOCK = re.compile(r"<script\b.*?</script>", re.DOTALL | re.IGNORECASE)
STYLE_BLOCK = re.compile(r"<style\b.*?</style>", re.DOTALL | re.IGNORECASE)
# Attributes and component props that carry user-visible or accessible text. Literal values
# (quoted, or a string/template literal inside a Svelte expression) must come from message IDs.
TEXT_ATTRIBUTES = r"(?:aria-label|aria-description|title|placeholder|alt|label|heading|description)"
LITERAL_ACCESSIBLE_ATTRIBUTE = re.compile(
    r"(?<![\w-])" + TEXT_ATTRIBUTES + r"\s*=\s*(?:([\"'])(?P<value>.*?)\1"
    r"|\{\s*([\"'`])(?P<expression>.*?)\3\s*\})",
    re.DOTALL | re.IGNORECASE,
)
TEMPLATE_SUBSTITUTION = re.compile(r"\$\{[^}]*\}")
POSITIVE_TABINDEX = re.compile(
    r"\btabindex\s*=\s*(?:[\"']\s*[1-9][0-9]*\s*[\"']|\{\s*[1-9][0-9]*\s*\}|[1-9][0-9]*)",
    re.IGNORECASE,
)
RAW_HTML = re.compile(
    r"\{@html\b|bind:innerHTML\b|\.innerHTML\b|\.outerHTML\b|\binsertAdjacentHTML\b|"
    r"\bdocument\.write\s*\(|"
    r"\[\s*[\"'`](?:inner|outer)HTML[\"'`]\s*\]|"
    r"\bReflect\s*\.\s*set\s*\([^)]*[\"'`](?:inner|outer)HTML[\"'`]|"
    r"\bDOMParser\b|\bcreateContextualFragment\b|\bsetHTMLUnsafe\b|\bparseHTMLUnsafe\b|"
    r"\bsrcdoc\s*=|[\"'`]\s*javascript\s*:",
    re.IGNORECASE,
)
REMOTE_RESOURCE = re.compile(r"https?://|(?<!:)//[A-Za-z0-9.-]+|url\(\s*['\"]?(?:https?:)?//", re.IGNORECASE)


def markup_text_nodes(markup: str) -> list[str]:
    nodes: list[str] = []
    position = 0
    while position < len(markup):
        if markup[position] != "<":
            closing = markup.find("<", position)
            if closing == -1:
                closing = len(markup)
            nodes.append(markup[position:closing])
            position = closing
            continue
        position += 1
        quote: str | None = None
        brace_depth = 0
        while position < len(markup):
            character = markup[position]
            if quote is not None:
                if character == quote and markup[position - 1] != "\\":
                    quote = None
            elif character in {'"', "'"}:
                quote = character
            elif character == "{":
                brace_depth += 1
            elif character == "}":
                brace_depth = max(0, brace_depth - 1)
            elif character == ">" and brace_depth == 0:
                position += 1
                break
            position += 1
    return nodes


def strip_svelte_expressions(text: str) -> str:
    output: list[str] = []
    position = 0
    while position < len(text):
        if text[position] != "{":
            output.append(text[position])
            position += 1
            continue
        depth = 1
        position += 1
        quote: str | None = None
        while position < len(text) and depth > 0:
            character = text[position]
            if quote is not None:
                if character == quote and text[position - 1] != "\\":
                    quote = None
            elif character in {'"', "'"}:
                quote = character
            elif character == "{":
                depth += 1
            elif character == "}":
                depth -= 1
            position += 1
    return "".join(output)


def check_text(path: Path, text: str, root: Path = ROOT) -> list[str]:
    relative = path.relative_to(root) if path.is_relative_to(root) else path
    failures: list[str] = []
    if RAW_HTML.search(text):
        failures.append(f"{relative}: raw HTML APIs are forbidden")
    if POSITIVE_TABINDEX.search(text):
        failures.append(f"{relative}: positive tabindex is forbidden")
    if path.name != "tokens.css" and (COLOR_LITERAL.search(text) or NAMED_COLOR.search(text)):
        failures.append(f"{relative}: color literals belong only in tokens.css")
    if REMOTE_RESOURCE.search(text):
        failures.append(f"{relative}: runtime external resources are forbidden")
    for match in LITERAL_ACCESSIBLE_ATTRIBUTE.finditer(text):
        literal = match.group("value")
        if literal is None:
            literal = TEMPLATE_SUBSTITUTION.sub("", match.group("expression") or "")
        value = " ".join(literal.split())
        if re.search(r"[A-Za-zА-Яа-яЁё]", value):
            failures.append(
                f"{relative}: accessible text attributes must come from a message ID: {value!r}"
            )
    if path.suffix == ".svelte":
        markup = STYLE_BLOCK.sub("", SCRIPT_BLOCK.sub("", text))
        for node in markup_text_nodes(markup):
            visible = " ".join(strip_svelte_expressions(node).split())
            if visible and re.search(r"[A-Za-zА-Яа-яЁё]", visible):
                failures.append(
                    f"{relative}: visible text must come from a message ID: {visible!r}"
                )
    return failures


def collect_failures(source: Path = SOURCE, root: Path = ROOT) -> list[str]:
    failures: list[str] = []
    for path in source.rglob("*"):
        if path.suffix not in SOURCE_SUFFIXES:
            continue
        failures.extend(check_text(path, path.read_text(encoding="utf-8"), root))
    return failures


def main() -> int:
    failures = collect_failures()
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    print(
        "UI contracts OK: no raw HTML APIs, hardcoded accessible text, positive tabindex, "
        "local colors, or external resources."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
