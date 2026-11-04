from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("build_licenses", ROOT / "tools" / "build_licenses.py")
assert SPEC is not None and SPEC.loader is not None
GEN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GEN)

NOTICE = (
    "| Release | `v3.49.0` |\n| Commit | `" + "a" * 40 + "` |\n\n### MIT License\n\n"
    "Copyright (c) 2020-2026 Someone\n\nPermission is hereby granted, free of charge.\n"
)


ALLOWED = GEN.load_allowed_licenses()


def package(identifier: str, name: str, license_expression: str | None, source: str | None = GEN.CRATES_IO) -> dict:
    return {"id": identifier, "name": name, "version": "1.0.0", "license": license_expression, "source": source}


def metadata() -> dict:
    # app -> normal: zeta, alpha; dev: devonly; build: buildonly; zeta -> normal: leaf
    deps = {
        "app": [("zeta", [None]), ("alpha", [None]), ("devonly", ["dev"]), ("buildonly", ["build"])],
        "zeta": [("leaf", [None])],
        "alpha": [],
        "leaf": [],
        "devonly": [],
        "buildonly": [],
    }
    return {
        "workspace_members": ["app"],
        "packages": [
            package("app", "app", "MIT OR Apache-2.0", None),
            package("zeta", "zeta", "MIT"),
            package("alpha", "alpha", "Apache-2.0"),
            package("leaf", "leaf", "ISC"),
            package("devonly", "devonly", "GPL-3.0"),
            package("buildonly", "buildonly", "GPL-3.0"),
        ],
        "resolve": {
            "nodes": [
                {"id": key, "deps": [{"pkg": pkg, "dep_kinds": [{"kind": kind} for kind in kinds]} for pkg, kinds in value]}
                for key, value in deps.items()
            ]
        },
    }


LOCK = {
    "packages": {
        "": {},
        "node_modules/svelte": {"version": "5.0.0", "dev": True, "license": "MIT", "dependencies": {"clsx": "^2"}},
        "node_modules/clsx": {"version": "2.0.0", "dev": True, "license": "MIT"},
        "node_modules/vite": {"version": "8.0.0", "dev": True, "license": "MIT"},
        "node_modules/a/node_modules/nested": {"version": "1.0.0", "license": "MIT"},
    }
}


def document(meta: dict | None = None, lock: dict | None = None) -> dict:
    return GEN.build_document(
        meta or metadata(), lock or LOCK, {"dependencies": {}}, NOTICE, ["settings", "copy"], "# Provenance",
        {"mit": "MIT text", "apache": "Apache text", "docs": "CC text"}, ALLOWED,
    )


def test_dev_and_build_only_dependencies_are_excluded() -> None:
    names = [(c["kind"], c["name"]) for c in document()["components"]]
    assert ("rust", "devonly") not in names and ("rust", "buildonly") not in names
    assert ("rust", "app") not in names, "workspace members are the product itself"
    assert ("npm", "vite") not in names
    assert {("rust", "alpha"), ("rust", "zeta"), ("rust", "leaf"), ("npm", "svelte"), ("npm", "clsx")} <= set(names)


def test_output_is_sorted_and_deterministic() -> None:
    first = GEN.render(document())
    meta = metadata()
    meta["packages"].reverse()
    meta["resolve"]["nodes"].reverse()
    assert GEN.render(document(meta)) == first
    names = [c["name"] for c in json.loads(first)["components"] if c["kind"] == "rust"]
    assert names == sorted(names)


def test_missing_rust_license_fails() -> None:
    meta = metadata()
    meta["packages"][3]["license"] = None
    with pytest.raises(GEN.LicenseDataError, match="leaf"):
        document(meta)


def test_unsupported_rust_source_fails() -> None:
    meta = metadata()
    meta["packages"][2]["source"] = "git+https://example.invalid/x"
    with pytest.raises(GEN.LicenseDataError, match="unsupported source"):
        document(meta)


def test_missing_npm_license_fails() -> None:
    lock = copy.deepcopy(LOCK)
    del lock["packages"]["node_modules/clsx"]["license"]
    with pytest.raises(GEN.LicenseDataError, match="clsx"):
        document(lock=lock)


def test_tabler_notice_must_contain_mit_text() -> None:
    with pytest.raises(GEN.LicenseDataError):
        GEN.tabler_entry("| Release | `v1` |\n", ["x"])
    entry = GEN.tabler_entry(NOTICE, ["b", "a"])
    assert entry["icon_names"] == ["a", "b"] and entry["license_text"].startswith("Copyright (c)")


def test_committed_inventory_is_in_sync_and_complete() -> None:
    committed = json.loads(GEN.OUTPUT.read_text(encoding="utf-8"))
    assert committed["schema_version"] == GEN.SCHEMA_VERSION
    assert committed["components"], "inventory must not be empty"
    assert all(c["license"] for c in committed["components"])
    expected_mit = (ROOT / "assets/icons/THIRD_PARTY.md").read_text(encoding="utf-8").replace("\r\n", "\n")
    assert committed["icons"]["license_text"].strip() in expected_mit
    for key, name in GEN.PRODUCT_LICENSES.items():
        assert committed["product_licenses"][key] == (ROOT / name).read_text(encoding="utf-8").replace("\r\n", "\n")


@pytest.mark.parametrize(
    "expression",
    [
        "MIT",
        "MIT OR Apache-2.0",
        "GPL-3.0-only OR MIT",
        "(GPL-3.0-only AND MIT) OR ISC",
        "Apache-2.0 WITH LLVM-exception",
        "Apache-2.0 WITH LLVM-exception OR GPL-2.0-only",
        "MIT AND (BSD-3-Clause OR LGPL-2.1-only)",
        "Apache-2.0/MIT",
        "Unlicense/MIT",
        "MIT OR Apache-2.0 OR Zlib",
        "Unicode-3.0",
        "MIT-0",
    ],
)
def test_allowed_expressions(expression: str) -> None:
    assert GEN.spdx_allowed(expression, ALLOWED)


@pytest.mark.parametrize(
    "expression",
    [
        "GPL-3.0-only",
        "LGPL-2.1-or-later",
        "MPL-2.0",
        "MIT AND GPL-3.0-only",
        "MIT AND (ISC AND MPL-2.0)",
        "GPL-2.0-only WITH Classpath-exception-2.0",
        "MIT WITH LLVM-exception",
        "GPL-3.0/MPL-2.0",
    ],
)
def test_rejected_expressions(expression: str) -> None:
    assert not GEN.spdx_allowed(expression, ALLOWED)


@pytest.mark.parametrize("expression", ["", "   ", "MIT OR", "(MIT", "MIT)", "AND MIT", "MIT WITH", "SEE LICENSE IN LICENSE.txt"])
def test_malformed_expressions_fail(expression: str) -> None:
    with pytest.raises(GEN.LicenseDataError):
        GEN.spdx_allowed(expression, ALLOWED)


def test_disallowed_rust_license_fails_naming_the_crate() -> None:
    meta = metadata()
    meta["packages"][3]["license"] = "GPL-3.0-only"
    with pytest.raises(GEN.LicenseDataError, match="leaf"):
        document(meta)


def test_disallowed_npm_license_fails_naming_the_package() -> None:
    lock = copy.deepcopy(LOCK)
    lock["packages"]["node_modules/clsx"]["license"] = "GPL-3.0-only AND MIT"
    with pytest.raises(GEN.LicenseDataError, match="clsx"):
        document(lock=lock)


def test_allow_list_is_read_from_deny_toml() -> None:
    assert {"MIT", "Apache-2.0 WITH LLVM-exception", "CDLA-Permissive-2.0"} <= ALLOWED
    assert "GPL-3.0-only" not in ALLOWED


def test_license_exception_allows_only_the_named_crate_version() -> None:
    meta = metadata()
    meta["packages"][3]["license"] = "MPL-2.0"
    exact = {("leaf", "1.0.0"): frozenset({"MPL-2.0"})}
    names = [c["name"] for c in GEN.rust_components(meta, ALLOWED, exact)]
    assert "leaf" in names
    for other in ({("leaf", "2.0.0"): frozenset({"MPL-2.0"})}, {("zeta", None): frozenset({"MPL-2.0"})}, {}):
        with pytest.raises(GEN.LicenseDataError, match="leaf"):
            GEN.rust_components(meta, ALLOWED, other)


def test_exceptions_and_targets_are_read_from_deny_toml() -> None:
    assert GEN.load_license_exceptions() == {("option-ext", "0.2.0"): frozenset({"MPL-2.0"})}
    assert GEN.load_graph_targets() == ("x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu")


def test_per_target_metadata_is_merged() -> None:
    windows = metadata()
    linux = copy.deepcopy(windows)
    linux["packages"].append(package("linuxonly", "linuxonly", "MIT"))
    linux["resolve"]["nodes"].append({"id": "linuxonly", "deps": []})
    linux["resolve"]["nodes"][0]["deps"].append({"pkg": "linuxonly", "dep_kinds": [{"kind": None}]})
    app_deps = windows["resolve"]["nodes"][0]["deps"]
    windows["resolve"]["nodes"][0]["deps"] = [dep for dep in app_deps if dep["pkg"] != "alpha"]
    merged = GEN.merge_metadata([windows, linux])
    names = sorted(c["name"] for c in GEN.rust_components(merged, ALLOWED))
    assert names == ["alpha", "leaf", "linuxonly", "zeta"]
    assert merged["workspace_members"] == ["app"]
