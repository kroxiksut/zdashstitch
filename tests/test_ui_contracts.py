from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "check_ui_contracts", ROOT / "tools" / "check_ui_contracts.py"
)
assert SPEC is not None and SPEC.loader is not None
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


@pytest.mark.parametrize(
    ("name", "source", "expected"),
    [
        ("unsafe.html", '<img alt="Dashboard preview">', "accessible text"),
        ("unsafe.js", "node.innerHTML = value", "raw HTML"),
        ("unsafe.mjs", "node.outerHTML = value", "raw HTML"),
        ("unsafe.ts", "node.insertAdjacentHTML('beforeend', value)", "raw HTML"),
        ("unsafe.ts", "document.write(value)", "raw HTML"),
        ("unsafe.svelte", "<div bind:innerHTML={value}></div>", "raw HTML"),
        ("unsafe.svelte", '<button tabindex="2"></button>', "positive tabindex"),
        ("unsafe.svelte", "<button tabindex={3}></button>", "positive tabindex"),
        ("unsafe.css", ".x { color: hsl(0 0% 0%); }", "color literals"),
        ("unsafe.css", ".x { color: red; }", "color literals"),
        ("unsafe.css", ".x { background: url(//cdn.invalid/a.png); }", "external resources"),
        ("unsafe.js", "const endpoint = '//host.invalid/file';", "external resources"),
        ("unsafe.svelte", "<p>Hello {name}</p>", "visible text"),
        ("unsafe.svelte", '<input placeholder="Search dashboards">', "accessible text"),
        ("unsafe.ts", "node['innerHTML'] = value", "raw HTML"),
        ("unsafe.ts", 'node["outerHTML"] = value', "raw HTML"),
        ("unsafe.ts", "Reflect.set(node, 'innerHTML', value)", "raw HTML"),
        ("unsafe.ts", "new DOMParser().parseFromString(value, 'text/html')", "raw HTML"),
        ("unsafe.ts", "range.createContextualFragment(value)", "raw HTML"),
        ("unsafe.ts", "node.setHTMLUnsafe(value)", "raw HTML"),
        ("unsafe.svelte", "<iframe srcdoc={value}></iframe>", "raw HTML"),
        ("unsafe.svelte", '<a href="javascript:alert(1)">x</a>', "raw HTML"),
        ("unsafe.ts", "link.href = `javascript:${code}`", "raw HTML"),
        ("unsafe.svelte", "<button aria-label={'Close dialog'}></button>", "accessible text"),
        ("unsafe.svelte", "<span title={`Узел ${name}`}></span>", "accessible text"),
        ("unsafe.svelte", '<Button label="Сохранить" />', "accessible text"),
        ("unsafe.svelte", '<Panel heading="Connections" />', "accessible text"),
        ("unsafe.svelte", "<EmptyState description={'Nothing here'} />", "accessible text"),
        ("unsafe.svelte", '<div aria-description="Grid of widgets"></div>', "accessible text"),
    ],
)
def test_unsafe_patterns_are_rejected(
    tmp_path: Path, name: str, source: str, expected: str
) -> None:
    path = tmp_path / name
    path.write_text(source, encoding="utf-8")
    failures = CHECKER.check_text(path, source, tmp_path)
    assert any(expected in failure for failure in failures), failures


def test_safe_message_bindings_and_local_resources_are_allowed(tmp_path: Path) -> None:
    path = tmp_path / "safe.svelte"
    source = (
        '<script>const icon = "/assets/icon.svg";</script>'
        '<button aria-label={i18n.t("common.action.close")} tabindex="0">'
        "{i18n.t('common.action.close')}"
        "</button>"
    )
    assert CHECKER.check_text(path, source, tmp_path) == []


def test_message_bound_text_props_and_identifier_only_templates_are_allowed(tmp_path: Path) -> None:
    path = tmp_path / "safe-props.svelte"
    source = (
        "<Button label={i18n.t('common.action.save')} />"
        "<Panel heading={title} description={i18n.t('common.state.loading')} />"
        "<span title={`${count}`}></span>"
        '<a data-label="x" href="/local">{i18n.t("common.action.open")}</a>'
    )
    assert CHECKER.check_text(path, source, tmp_path) == []
