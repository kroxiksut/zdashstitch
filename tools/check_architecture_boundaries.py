#!/usr/bin/env python3
"""Mechanical P0-01 checks for the two most important dependency boundaries."""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CORE_MANIFEST = ROOT / "crates" / "core" / "Cargo.toml"
UI_SOURCE = ROOT / "ui" / "src"
FORBIDDEN_UI_MARKERS = (
    "api_jsonrpc.php",
    "dashboard.create",
    "dashboard.update",
    "user.login",
)


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

    print("Architecture boundaries OK: core is transport-free; UI has no raw Zabbix calls.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
