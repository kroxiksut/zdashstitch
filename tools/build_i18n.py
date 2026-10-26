#!/usr/bin/env python3
"""Validate data-only localization catalogs and generate typed Rust/TypeScript IDs."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
LANG_ROOT = ROOT / "lang"
RUST_OUTPUT = ROOT / "crates" / "application" / "src" / "generated_message_ids.rs"
TS_OUTPUT = ROOT / "ui" / "src" / "lib" / "i18n" / "generated.ts"
LOCALES = ("en", "ru")
PLACEHOLDER_NAME_RE = re.compile(r"[a-z][a-z0-9_]*")
PLURAL_CATEGORY_RE = re.compile(r"(?:zero|one|two|few|many|other|=[0-9]+)")
SELECT_KEY_RE = re.compile(r"[a-z][a-z0-9_]*")
MESSAGE_ID_RE = re.compile(r"^[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+$")
# CLDR cardinal categories that every plural message must provide, per locale.
# Exact matches (=N) are always allowed in addition.
REQUIRED_PLURAL_CATEGORIES = {
    "en": frozenset({"one", "other"}),
    "ru": frozenset({"one", "few", "many", "other"}),
}
PLACEHOLDER_TYPES = {
    "technical",
    "technical_optional",
    "text",
    "text_optional",
    "number",
    "date",
    "datetime",
}
# `select` chooses a branch by a string value. An absent optional argument renders nothing,
# which is how a message spells an optional suffix such as `{sources, select, other {: {sources}}}`.
SELECT_ARGUMENT_TYPES = {"technical", "technical_optional", "text", "text_optional"}


class CatalogError(ValueError):
    """A catalog violates the localization contract."""


def reject_duplicate_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise CatalogError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_pairs)
    except CatalogError as error:
        raise CatalogError(f"{path}: {error}") from error


def matching_brace(text: str, opening: int) -> int:
    depth = 0
    for index in range(opening, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return index
    raise CatalogError("unbalanced message braces")


def plural_branches(source: str, locale: str | None = None) -> dict[str, str]:
    branches: dict[str, str] = {}
    position = 0
    while position < len(source):
        while position < len(source) and source[position].isspace():
            position += 1
        if position == len(source):
            break
        category = PLURAL_CATEGORY_RE.match(source, position)
        if category is None:
            raise CatalogError("invalid plural category")
        key = category.group(0)
        if key in branches:
            raise CatalogError(f"duplicate plural category {key}")
        position = category.end()
        while position < len(source) and source[position].isspace():
            position += 1
        if position >= len(source) or source[position] != "{":
            raise CatalogError(f"plural category {key} has no branch")
        closing = matching_brace(source, position)
        branches[key] = source[position + 1 : closing]
        position = closing + 1
    if "other" not in branches:
        raise CatalogError("plural messages require an other branch")
    if locale is not None:
        required = REQUIRED_PLURAL_CATEGORIES[locale]
        categories = {key for key in branches if not key.startswith("=")}
        missing = sorted(required - categories)
        unexpected = sorted(categories - required)
        if missing:
            raise CatalogError(f"plural messages for {locale} require categories {missing}")
        if unexpected:
            raise CatalogError(f"plural categories {unexpected} are not used by {locale}")
    return branches


def select_branches(source: str) -> dict[str, str]:
    branches: dict[str, str] = {}
    position = 0
    while position < len(source):
        while position < len(source) and source[position].isspace():
            position += 1
        if position == len(source):
            break
        key_match = SELECT_KEY_RE.match(source, position)
        if key_match is None:
            raise CatalogError("invalid select key")
        key = key_match.group(0)
        if key in branches:
            raise CatalogError(f"duplicate select key {key}")
        position = key_match.end()
        while position < len(source) and source[position].isspace():
            position += 1
        if position >= len(source) or source[position] != "{":
            raise CatalogError(f"select key {key} has no branch")
        closing = matching_brace(source, position)
        branches[key] = source[position + 1 : closing]
        position = closing + 1
    if "other" not in branches:
        raise CatalogError("select messages require an other branch")
    return branches


def message_placeholders(
    message: str,
    locale: str | None = None,
    plural_arguments: set[str] | None = None,
    select_arguments: set[str] | None = None,
) -> set[str]:
    used: set[str] = set()
    position = 0
    while position < len(message):
        if message[position] == "}":
            raise CatalogError("unbalanced message braces")
        if message[position] != "{":
            position += 1
            continue
        closing = matching_brace(message, position)
        expression = message[position + 1 : closing].strip()
        plural = re.fullmatch(r"([a-z][a-z0-9_]*)\s*,\s*plural\s*,(.*)", expression, re.DOTALL)
        if plural is not None:
            name, options = plural.groups()
            used.add(name)
            if plural_arguments is not None:
                plural_arguments.add(name)
            for branch in plural_branches(options, locale).values():
                used.update(message_placeholders(branch, locale, plural_arguments, select_arguments))
            position = closing + 1
            continue
        select = re.fullmatch(r"([a-z][a-z0-9_]*)\s*,\s*select\s*,(.*)", expression, re.DOTALL)
        if select is not None:
            name, options = select.groups()
            used.add(name)
            if select_arguments is not None:
                select_arguments.add(name)
            for branch in select_branches(options).values():
                used.update(message_placeholders(branch, locale, plural_arguments, select_arguments))
        elif PLACEHOLDER_NAME_RE.fullmatch(expression):
            used.add(expression)
        else:
            raise CatalogError(f"unsupported message expression {{{expression}}}")
        position = closing + 1
    return used


def load_catalog(locale: str) -> dict[str, dict[str, Any]]:
    locale_dir = LANG_ROOT / locale
    manifest = load_json(locale_dir / "manifest.json")
    if manifest.get("locale") != locale:
        raise CatalogError(f"{locale}: manifest locale does not match directory")
    if manifest.get("source_locale") != "en" or manifest.get("fallback_locale") != "en":
        raise CatalogError(f"{locale}: source and fallback locale must be en")

    catalog: dict[str, dict[str, Any]] = {}
    for namespace in manifest.get("namespaces", []):
        path = locale_dir / f"{namespace}.json"
        document = load_json(path)
        if not isinstance(document, dict):
            raise CatalogError(f"{path}: namespace must be a JSON object")
        for message_id, entry in document.items():
            if message_id in catalog:
                raise CatalogError(f"{locale}: duplicate message ID {message_id}")
            validate_entry(path, message_id, entry, locale)
            catalog[message_id] = entry
    return catalog


def validate_entry(path: Path, message_id: str, entry: Any, locale: str | None = None) -> None:
    if not MESSAGE_ID_RE.fullmatch(message_id):
        raise CatalogError(f"{path}: invalid message ID {message_id!r}")
    if not isinstance(entry, dict) or set(entry) != {"message", "placeholders"}:
        raise CatalogError(f"{path}: {message_id} must contain only message and placeholders")
    message = entry["message"]
    placeholders = entry["placeholders"]
    if not isinstance(message, str) or not message.strip():
        raise CatalogError(f"{path}: {message_id} has an empty message")
    if "<" in message or ">" in message:
        raise CatalogError(f"{path}: {message_id} must be data-only text, not markup")
    if not isinstance(placeholders, dict):
        raise CatalogError(f"{path}: {message_id} placeholders must be an object")
    if any(value not in PLACEHOLDER_TYPES for value in placeholders.values()):
        raise CatalogError(f"{path}: {message_id} uses an unsupported placeholder type")
    plural_arguments: set[str] = set()
    select_arguments: set[str] = set()
    try:
        used = message_placeholders(message, locale, plural_arguments, select_arguments)
    except CatalogError as error:
        raise CatalogError(f"{path}: {message_id}: {error}") from error
    for name in sorted(plural_arguments):
        if placeholders.get(name) != "number":
            raise CatalogError(f"{path}: {message_id}: plural argument {name} must be declared number")
    for name in sorted(select_arguments):
        if placeholders.get(name) not in SELECT_ARGUMENT_TYPES:
            raise CatalogError(f"{path}: {message_id}: select argument {name} must be declared text")
    declared = set(placeholders)
    if used != declared:
        raise CatalogError(
            f"{path}: {message_id} placeholder mismatch: used={sorted(used)}, "
            f"declared={sorted(declared)}"
        )


def validate_parity(catalogs: dict[str, dict[str, dict[str, Any]]]) -> None:
    source = catalogs["en"]
    for locale, catalog in catalogs.items():
        if set(catalog) != set(source):
            missing = sorted(set(source) - set(catalog))
            extra = sorted(set(catalog) - set(source))
            raise CatalogError(f"{locale}: key mismatch; missing={missing}, extra={extra}")
        for message_id, source_entry in source.items():
            if catalog[message_id]["placeholders"] != source_entry["placeholders"]:
                raise CatalogError(f"{locale}: placeholder mismatch for {message_id}")


def rust_identifier(message_id: str) -> str:
    return re.sub(r"[^A-Z0-9]+", "_", message_id.upper()).strip("_")


def generate_rust(source: dict[str, dict[str, Any]]) -> str:
    constants = "\n".join(
        f'    pub const {rust_identifier(message_id)}: Self = Self("{message_id}");'
        for message_id in sorted(source)
    )
    all_values = "\n".join(
        f"        Self::{rust_identifier(message_id)}," for message_id in sorted(source)
    )
    return f'''// @generated by tools/build_i18n.py; do not edit by hand.
// SPDX-License-Identifier: MIT OR Apache-2.0

/// A message identifier proven to exist in both supported catalogs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MessageId(&'static str);

#[rustfmt::skip]
impl MessageId {{
{constants}

    /// Every message identifier in stable lexical order.
    pub const ALL: &'static [Self] = &[
{all_values}
    ];

    /// Return the wire/catalog representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {{
        self.0
    }}
}}

impl std::fmt::Display for MessageId {{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        formatter.write_str(self.0)
    }}
}}
'''


def ts_type(type_name: str) -> str:
    return {
        "technical": "string",
        "technical_optional": "string | undefined",
        "text": "string",
        "text_optional": "string | undefined",
        "number": "number",
        "date": "Date | number",
        "datetime": "Date | number",
    }[type_name]


def generate_typescript(catalogs: dict[str, dict[str, dict[str, Any]]]) -> str:
    source = catalogs["en"]
    params = []
    for message_id in sorted(source):
        fields = []
        for name, type_name in source[message_id]["placeholders"].items():
            optional = "?" if type_name.endswith("_optional") else ""
            fields.append(f"    {json.dumps(name)}{optional}: {ts_type(type_name)};")
        body = "\n".join(fields)
        params.append(f"  {json.dumps(message_id)}: {{\n{body}\n  }};")

    messages = {
        locale: {message_id: entry["message"] for message_id, entry in sorted(catalog.items())}
        for locale, catalog in catalogs.items()
    }
    placeholder_types = {
        message_id: entry["placeholders"] for message_id, entry in sorted(source.items())
    }
    serialized = json.dumps(messages, ensure_ascii=False, indent=2)
    serialized_types = json.dumps(placeholder_types, ensure_ascii=False, indent=2)
    return f'''// @generated by tools/build_i18n.py; do not edit by hand.

export const catalogs = {serialized} as const;

export const placeholderTypes = {serialized_types} as const;

export type Locale = keyof typeof catalogs;
export type MessageId = keyof (typeof catalogs)['en'];

export interface MessageParamsById {{
{chr(10).join(params)}
}}
'''


def write_or_check(path: Path, expected: str, check: bool) -> bool:
    actual = path.read_text(encoding="utf-8") if path.exists() else None
    if check:
        if actual != expected:
            print(f"generated file is stale: {path.relative_to(ROOT)}", file=sys.stderr)
            return False
        return True
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(expected, encoding="utf-8", newline="\n")
    return True


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if generated files are stale")
    args = parser.parse_args()
    try:
        catalogs = {locale: load_catalog(locale) for locale in LOCALES}
        validate_parity(catalogs)
        ok = write_or_check(RUST_OUTPUT, generate_rust(catalogs["en"]), args.check)
        ok &= write_or_check(TS_OUTPUT, generate_typescript(catalogs), args.check)
    except (CatalogError, OSError, json.JSONDecodeError) as error:
        print(f"i18n catalog error: {error}", file=sys.stderr)
        return 1
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
