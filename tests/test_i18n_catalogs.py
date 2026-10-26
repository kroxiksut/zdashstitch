from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("build_i18n", ROOT / "tools" / "build_i18n.py")
assert SPEC is not None and SPEC.loader is not None
BUILD_I18N = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD_I18N)


def test_supported_catalogs_have_identical_keys_and_placeholders() -> None:
    catalogs = {locale: BUILD_I18N.load_catalog(locale) for locale in BUILD_I18N.LOCALES}
    BUILD_I18N.validate_parity(catalogs)


def test_markup_is_rejected_from_data_only_messages(tmp_path: Path) -> None:
    with pytest.raises(BUILD_I18N.CatalogError, match="data-only"):
        BUILD_I18N.validate_entry(
            tmp_path / "catalog.json",
            "test.message",
            {"message": "<strong>unsafe</strong>", "placeholders": {}},
        )


def test_undeclared_placeholder_is_rejected(tmp_path: Path) -> None:
    with pytest.raises(BUILD_I18N.CatalogError, match="placeholder mismatch"):
        BUILD_I18N.validate_entry(
            tmp_path / "catalog.json",
            "test.message",
            {"message": "Hello {name}", "placeholders": {}},
        )


def test_duplicate_json_key_is_rejected(tmp_path: Path) -> None:
    path = tmp_path / "catalog.json"
    path.write_text('{"duplicate": 1, "duplicate": 2}', encoding="utf-8")
    with pytest.raises(BUILD_I18N.CatalogError, match="duplicate JSON key"):
        BUILD_I18N.load_json(path)


def test_plural_requires_other_and_declares_branch_placeholders(tmp_path: Path) -> None:
    with pytest.raises(BUILD_I18N.CatalogError, match="other branch"):
        BUILD_I18N.validate_entry(
            tmp_path / "catalog.json",
            "test.message",
            {
                "message": "{count, plural, one {# item}}",
                "placeholders": {"count": "number"},
            },
        )
    with pytest.raises(BUILD_I18N.CatalogError, match="placeholder mismatch"):
        BUILD_I18N.validate_entry(
            tmp_path / "catalog.json",
            "test.message",
            {
                "message": "{count, plural, one {{label}} other {# items}}",
                "placeholders": {"count": "number"},
            },
        )


def test_plural_subset_accepts_english_and_russian_categories(tmp_path: Path) -> None:
    for message in [
        "{count, plural, one {# item} other {# items}}",
        "{count, plural, one {# элемент} few {# элемента} many {# элементов} other {# элемента}}",
    ]:
        BUILD_I18N.validate_entry(
            tmp_path / "catalog.json",
            "test.message",
            {"message": message, "placeholders": {"count": "number"}},
        )


def test_plural_categories_follow_the_locale(tmp_path: Path) -> None:
    path = tmp_path / "catalog.json"
    with pytest.raises(BUILD_I18N.CatalogError, match="require categories"):
        BUILD_I18N.validate_entry(
            path,
            "test.message",
            {"message": "{count, plural, one {# файл} other {# файлов}}", "placeholders": {"count": "number"}},
            "ru",
        )
    with pytest.raises(BUILD_I18N.CatalogError, match="not used by en"):
        BUILD_I18N.validate_entry(
            path,
            "test.message",
            {"message": "{count, plural, one {# file} few {# files} other {# files}}", "placeholders": {"count": "number"}},
            "en",
        )
    BUILD_I18N.validate_entry(
        path,
        "test.message",
        {"message": "{count, plural, =0 {no files} one {# file} other {# files}}", "placeholders": {"count": "number"}},
        "en",
    )


def test_plural_argument_must_be_a_number(tmp_path: Path) -> None:
    with pytest.raises(BUILD_I18N.CatalogError, match="must be declared number"):
        BUILD_I18N.validate_entry(
            tmp_path / "catalog.json",
            "test.message",
            {"message": "{count, plural, one {# item} other {# items}}", "placeholders": {"count": "text"}},
            "en",
        )


def test_select_requires_other_and_a_text_argument(tmp_path: Path) -> None:
    path = tmp_path / "catalog.json"
    BUILD_I18N.validate_entry(
        path,
        "test.message",
        {
            "message": "{count, plural, one {# page} other {# pages}}{sources, select, other {: {sources}}}",
            "placeholders": {"count": "number", "sources": "text_optional"},
        },
        "en",
    )
    with pytest.raises(BUILD_I18N.CatalogError, match="other branch"):
        BUILD_I18N.validate_entry(
            path,
            "test.message",
            {"message": "{kind, select, page {Page}}", "placeholders": {"kind": "text"}},
        )
    with pytest.raises(BUILD_I18N.CatalogError, match="must be declared text"):
        BUILD_I18N.validate_entry(
            path,
            "test.message",
            {"message": "{kind, select, other {x}}", "placeholders": {"kind": "number"}},
        )
    with pytest.raises(BUILD_I18N.CatalogError, match="placeholder mismatch"):
        BUILD_I18N.validate_entry(
            path,
            "test.message",
            {"message": "{kind, select, other {{name}}}", "placeholders": {"kind": "text"}},
        )


def test_unknown_placeholder_type_is_rejected(tmp_path: Path) -> None:
    with pytest.raises(BUILD_I18N.CatalogError, match="unsupported placeholder type"):
        BUILD_I18N.validate_entry(
            tmp_path / "catalog.json",
            "test.message",
            {"message": "{when}", "placeholders": {"when": "date_time"}},
        )
