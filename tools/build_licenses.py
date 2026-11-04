#!/usr/bin/env python3
"""Generate the embedded third-party licence inventory for the «About and licenses» screen (P1-12).

Offline and deterministic. Inputs: `cargo metadata --locked` (normal dependencies of the workspace
members on the shipped targets, `[graph] targets` of deny.toml), `ui/package-lock.json` (packages bundled into `ui/dist`), the Tabler notice in
`assets/icons/THIRD_PARTY.md`, `assets/brand/PROVENANCE.md` and the product licence files. A package
without licence data fails the run (owner decision: the build fails on missing licence data).
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "ui" / "src" / "lib" / "about" / "licenses.json"
LOCK = ROOT / "ui" / "package-lock.json"
PACKAGE_JSON = ROOT / "ui" / "package.json"
THIRD_PARTY = ROOT / "assets" / "icons" / "THIRD_PARTY.md"
ICON_DIR = ROOT / "assets" / "icons" / "tabler" / "outline"
PROVENANCE = ROOT / "assets" / "brand" / "PROVENANCE.md"
PRODUCT_LICENSES = {"mit": "LICENSE-MIT", "apache": "LICENSE-APACHE", "docs": "LICENSE-DOCS"}
# `svelte` is a devDependency of the build, but its runtime is bundled into `ui/dist`, so the
# production closure starts from these roots in addition to `dependencies` of `ui/package.json`.
NPM_BUNDLED_ROOTS = ("svelte",)
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"
SCHEMA_VERSION = 1
DENY_TOML = ROOT / "deny.toml"


class LicenseDataError(ValueError):
    """Licence data is missing or malformed; the build must fail."""


def load_allowed_licenses(path: Path = DENY_TOML) -> frozenset[str]:
    """The single allow-list: `[licenses] allow` of deny.toml (shared with cargo-deny)."""
    allow = tomllib.loads(path.read_text(encoding="utf-8")).get("licenses", {}).get("allow")
    if not isinstance(allow, list) or not allow or not all(isinstance(item, str) for item in allow):
        raise LicenseDataError(f"{path.name}: [licenses] allow is missing or malformed")
    return frozenset(allow)


def load_graph_targets(path: Path = DENY_TOML) -> tuple[str, ...]:
    """Shipped target triples (`[graph] targets` of deny.toml, shared with cargo-deny); empty = all."""
    targets = tomllib.loads(path.read_text(encoding="utf-8")).get("graph", {}).get("targets", [])
    if not isinstance(targets, list) or not all(isinstance(item, str) and item for item in targets):
        raise LicenseDataError(f"{path.name}: [graph] targets is malformed")
    return tuple(targets)


LicenseExceptions = dict[tuple[str, str | None], frozenset[str]]


def load_license_exceptions(path: Path = DENY_TOML) -> LicenseExceptions:
    """Per-crate licence exceptions (`[licenses] exceptions` of deny.toml): `crate` is `name` or
    `name@version`; `allow` lists the extra licences for that crate only."""
    entries = tomllib.loads(path.read_text(encoding="utf-8")).get("licenses", {}).get("exceptions", [])
    if not isinstance(entries, list):
        raise LicenseDataError(f"{path.name}: [licenses] exceptions is malformed")
    exceptions: LicenseExceptions = {}
    for entry in entries:
        spec = entry.get("crate") if isinstance(entry, dict) else None
        allow = entry.get("allow") if isinstance(entry, dict) else None
        if (
            not isinstance(spec, str)
            or not spec
            or not isinstance(allow, list)
            or not allow
            or not all(isinstance(item, str) for item in allow)
        ):
            raise LicenseDataError(f"{path.name}: [licenses] exceptions entry is malformed")
        name, _, version = spec.partition("@")
        exceptions[(name, version or None)] = frozenset(allow)
    return exceptions


_SPDX_TOKEN = re.compile(r"\s*(\(|\)|[^\s()]+)")


def _spdx_tokens(expression: str) -> list[str]:
    tokens: list[str] = []
    position = 0
    while position < len(expression.rstrip()):
        match = _SPDX_TOKEN.match(expression, position)
        if match is None:
            break
        token = match.group(1)
        position = match.end()
        # Old slash form `A/B` means `A OR B`.
        parts = re.split(r"(/)", token) if "/" in token and token not in ("(", ")") else [token]
        for part in parts:
            if part == "/":
                tokens.append("OR")
            elif part:
                tokens.append(part)
    return tokens


def spdx_allowed(expression: str, allowed: frozenset[str]) -> bool:
    """True if the SPDX expression is satisfied by the allow-list.

    OR needs one fully allowed alternative, AND needs all, `X WITH Y` is matched as the compound
    identifier `X WITH Y` (it must be listed verbatim). Malformed input raises LicenseDataError.
    """
    tokens = _spdx_tokens(expression)
    pos = 0

    def peek() -> str | None:
        return tokens[pos] if pos < len(tokens) else None

    def parse_or() -> bool:
        nonlocal pos
        result = parse_and()
        while peek() is not None and peek().upper() == "OR":  # type: ignore[union-attr]
            pos += 1
            right = parse_and()
            result = result or right
        return result

    def parse_and() -> bool:
        nonlocal pos
        result = parse_atom()
        while peek() is not None and peek().upper() == "AND":  # type: ignore[union-attr]
            pos += 1
            right = parse_atom()
            result = result and right
        return result

    def parse_atom() -> bool:
        nonlocal pos
        token = peek()
        if token is None:
            raise LicenseDataError(f"malformed licence expression {expression!r}")
        pos += 1
        if token == "(":
            value = parse_or()
            if peek() != ")":
                raise LicenseDataError(f"malformed licence expression {expression!r}: missing ')'")
            pos += 1
            return value
        if token == ")" or token.upper() in ("OR", "AND", "WITH"):
            raise LicenseDataError(f"malformed licence expression {expression!r}")
        name = token.removesuffix("+") if token.removesuffix("+") in allowed else token
        if peek() is not None and peek().upper() == "WITH":  # type: ignore[union-attr]
            pos += 1
            exception = peek()
            if exception is None or exception in ("(", ")"):
                raise LicenseDataError(f"malformed licence expression {expression!r}")
            pos += 1
            return f"{name} WITH {exception}" in allowed
        return name in allowed

    if not tokens:
        raise LicenseDataError(f"empty licence expression {expression!r}")
    value = parse_or()
    if pos != len(tokens):
        raise LicenseDataError(f"malformed licence expression {expression!r}")
    return value


def check_allowed(kind: str, name: str, version: str, expression: str, allowed: frozenset[str]) -> None:
    if not spdx_allowed(expression, allowed):
        raise LicenseDataError(
            f"{kind} package {name} {version}: licence {expression!r} is not covered by the allow-list in deny.toml"
        )


def normalize_text(text: str) -> str:
    return text.replace("\r\n", "\n").replace("\r", "\n")


def read_text(path: Path) -> str:
    return normalize_text(path.read_text(encoding="utf-8"))


def rust_components(
    metadata: dict[str, Any], allowed: frozenset[str], exceptions: LicenseExceptions | None = None
) -> list[dict[str, str]]:
    """Packages reachable from workspace members through normal (non-dev, non-build) edges."""
    exceptions = exceptions or {}
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    members = set(metadata["workspace_members"])
    seen: set[str] = set()
    pending = list(members)
    while pending:
        current = pending.pop()
        for dep in nodes[current]["deps"]:
            kinds = dep.get("dep_kinds") or [{"kind": None}]
            if any(kind.get("kind") is None for kind in kinds) and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                pending.append(dep["pkg"])
    result: list[dict[str, str]] = []
    for identifier in seen - members:
        package = packages[identifier]
        name, version = package["name"], package["version"]
        license_expression = (package.get("license") or "").strip()
        if not license_expression:
            raise LicenseDataError(f"crate {name} {version} has no licence expression")
        extra = exceptions.get((name, version), frozenset()) | exceptions.get((name, None), frozenset())
        check_allowed("crate", name, version, license_expression, allowed | extra)
        source = package.get("source")
        if source != CRATES_IO:
            raise LicenseDataError(f"crate {name} {version}: unsupported source {source!r}")
        result.append(
            {"kind": "rust", "name": name, "version": version, "license": license_expression, "source": "crates.io"}
        )
    return result


def npm_name(path: str) -> str:
    return path.rsplit("node_modules/", 1)[1]


def npm_components(lock: dict[str, Any], package_json: dict[str, Any], allowed: frozenset[str]) -> list[dict[str, str]]:
    """Packages in the production closure of `ui/package.json` plus the bundled runtime roots."""
    entries: dict[str, dict[str, Any]] = {
        npm_name(path): entry for path, entry in lock["packages"].items() if path.startswith("node_modules/")
    }
    pending = list(package_json.get("dependencies", {})) + list(NPM_BUNDLED_ROOTS)
    seen: set[str] = set()
    while pending:
        name = pending.pop()
        if name in seen:
            continue
        if name not in entries:
            raise LicenseDataError(f"npm package {name} is not in package-lock.json")
        seen.add(name)
        pending.extend(entries[name].get("dependencies", {}))
    result: list[dict[str, str]] = []
    for name in seen:
        entry = entries[name]
        license_expression = entry.get("license")
        if not isinstance(license_expression, str) or not license_expression.strip():
            raise LicenseDataError(f"npm package {name} {entry.get('version')} has no licence in package-lock.json")
        check_allowed("npm", name, entry["version"], license_expression, allowed)
        result.append(
            {
                "kind": "npm",
                "name": name,
                "version": entry["version"],
                "license": license_expression.strip(),
                "source": "npm",
            }
        )
    return result


def tabler_entry(notice: str, icon_names: list[str]) -> dict[str, Any]:
    release = re.search(r"\| Release \| `([^`]+)` \|", notice)
    commit = re.search(r"\| Commit \| `([0-9a-f]{40})` \|", notice)
    marker = "### MIT License\n"
    if release is None or commit is None or marker not in notice:
        raise LicenseDataError("assets/icons/THIRD_PARTY.md: Tabler release, commit or MIT text is missing")
    mit_text = notice.split(marker, 1)[1].strip("\n")
    if "Permission is hereby granted" not in mit_text or "Copyright (c)" not in mit_text:
        raise LicenseDataError("assets/icons/THIRD_PARTY.md: the Tabler MIT text is incomplete")
    if not icon_names:
        raise LicenseDataError("no vendored Tabler icons found")
    return {
        "name": "Tabler Icons",
        "version": release.group(1).lstrip("v"),
        "commit": commit.group(1),
        "license": "MIT",
        "license_text": mit_text + "\n",
        "icon_names": sorted(icon_names),
    }


def sorted_components(items: list[dict[str, str]]) -> list[dict[str, str]]:
    return sorted(items, key=lambda item: (item["kind"], item["name"].lower(), item["name"], item["version"]))


def build_document(
    metadata: dict[str, Any],
    lock: dict[str, Any],
    package_json: dict[str, Any],
    notice: str,
    icon_names: list[str],
    provenance: str,
    product_texts: dict[str, str],
    allowed: frozenset[str],
    exceptions: LicenseExceptions | None = None,
) -> dict[str, Any]:
    for key, text in product_texts.items():
        if not text.strip():
            raise LicenseDataError(f"product licence text {key!r} is empty")
    if not provenance.strip():
        raise LicenseDataError("assets/brand/PROVENANCE.md is empty")
    return {
        "schema_version": SCHEMA_VERSION,
        "product_licenses": {key: product_texts[key] for key in sorted(product_texts)},
        "brand_provenance": provenance,
        "icons": tabler_entry(notice, icon_names),
        "components": sorted_components(
            rust_components(metadata, allowed, exceptions) + npm_components(lock, package_json, allowed)
        ),
    }


def render(document: dict[str, Any]) -> str:
    return json.dumps(document, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def cargo_metadata(target: str | None = None) -> dict[str, Any]:
    command = ["cargo", "metadata", "--format-version", "1", "--locked"]
    if target is not None:
        command += ["--filter-platform", target]
    completed = subprocess.run(command, cwd=ROOT, check=True, capture_output=True, encoding="utf-8")
    return json.loads(completed.stdout)


def merge_metadata(documents: list[dict[str, Any]]) -> dict[str, Any]:
    """Union of per-target `cargo metadata` documents: packages by id, dependency edges per node."""
    packages: dict[str, dict[str, Any]] = {}
    edges: dict[str, dict[str, dict[str, Any]]] = {}
    members: list[str] = []
    for document in documents:
        for package in document["packages"]:
            packages.setdefault(package["id"], package)
        for node in document["resolve"]["nodes"]:
            node_edges = edges.setdefault(node["id"], {})
            for dep in node["deps"]:
                known = node_edges.setdefault(dep["pkg"], {"pkg": dep["pkg"], "dep_kinds": []})
                for kind in dep.get("dep_kinds") or [{"kind": None}]:
                    if kind not in known["dep_kinds"]:
                        known["dep_kinds"].append(kind)
        members.extend(member for member in document["workspace_members"] if member not in members)
    return {
        "workspace_members": members,
        "packages": [packages[key] for key in sorted(packages)],
        "resolve": {
            "nodes": [{"id": key, "deps": [edges[key][pkg] for pkg in sorted(edges[key])]} for key in sorted(edges)]
        },
    }


def shipped_metadata() -> dict[str, Any]:
    targets = load_graph_targets()
    if not targets:
        return cargo_metadata()
    return merge_metadata([cargo_metadata(target) for target in targets])


def generate() -> str:
    document = build_document(
        shipped_metadata(),
        json.loads(LOCK.read_text(encoding="utf-8")),
        json.loads(PACKAGE_JSON.read_text(encoding="utf-8")),
        read_text(THIRD_PARTY),
        [path.stem for path in ICON_DIR.glob("*.svg")],
        read_text(PROVENANCE),
        {key: read_text(ROOT / name) for key, name in PRODUCT_LICENSES.items()},
        load_allowed_licenses(),
        load_license_exceptions(),
    )
    return render(document)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if the generated file is stale")
    args = parser.parse_args()
    try:
        text = generate()
    except (LicenseDataError, OSError, KeyError, tomllib.TOMLDecodeError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"licence inventory error: {error}", file=sys.stderr)
        return 1
    current = OUTPUT.read_text(encoding="utf-8") if OUTPUT.exists() else None
    if args.check:
        if current != text:
            print(f"{OUTPUT.relative_to(ROOT)} is stale: run python tools/build_licenses.py", file=sys.stderr)
            return 1
        return 0
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    with OUTPUT.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
