#!/usr/bin/env python3
"""Mechanical checks for the most important dependency boundaries (P0-01, O-036)."""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CORE_MANIFEST = ROOT / "crates" / "core" / "Cargo.toml"
UI_SOURCE = ROOT / "ui" / "src"
WORKSPACE_MANIFEST = ROOT / "Cargo.toml"
# O-036: only the program window links a web engine; the launcher and the browser-only server
# must start without WebKitGTK/WebView2 so a missing engine can fall back to the browser.
WEB_ENGINE_CRATES = ("wry", "tao")
WEB_ENGINE_MEMBER = "apps/desktop_window"
FORBIDDEN_UI_MARKERS = (
    "api_jsonrpc.php",
    "dashboard.create",
    "dashboard.update",
    "user.login",
)


def dependency_names(manifest: dict) -> set[str]:
    names = set(manifest.get("dependencies", {}))
    for target in manifest.get("target", {}).values():
        names |= set(target.get("dependencies", {}))
    return names


def web_engine_violations() -> list[str]:
    members = tomllib.loads(WORKSPACE_MANIFEST.read_text(encoding="utf-8"))["workspace"]["members"]
    violations = []
    for member in members:
        if member == WEB_ENGINE_MEMBER:
            continue
        manifest = tomllib.loads((ROOT / member / "Cargo.toml").read_text(encoding="utf-8"))
        linked = sorted(dependency_names(manifest) & set(WEB_ENGINE_CRATES))
        if linked:
            violations.append(f"{member} depends on web-engine crates: " + ", ".join(linked))
    return violations


def main() -> int:
    manifest = tomllib.loads(CORE_MANIFEST.read_text(encoding="utf-8"))
    dependencies = set(manifest.get("dependencies", {}))
    forbidden_core_dependencies = sorted(
        dependency for dependency in dependencies if "zabbix" in dependency.lower()
    )

    violations: list[str] = []
    if forbidden_core_dependencies:
        violations.append(
            "core depends on Zabbix crates: " + ", ".join(forbidden_core_dependencies)
        )

    violations.extend(web_engine_violations())

    for path in sorted(UI_SOURCE.rglob("*")):
        if path.suffix not in {".svelte", ".ts"} or not path.is_file():
            continue
        text = path.read_text(encoding="utf-8")
        for marker in FORBIDDEN_UI_MARKERS:
            if marker in text:
                violations.append(f"{path.relative_to(ROOT)} contains {marker!r}")

    if violations:
        print("Architecture boundary check failed:", file=sys.stderr)
        for violation in violations:
            print(f"- {violation}", file=sys.stderr)
        return 1

    print(
        "Architecture boundaries OK: core is transport-free; UI has no raw Zabbix calls; "
        "only the program window links a web engine."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
