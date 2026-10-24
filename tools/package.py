#!/usr/bin/env python3
"""Create self-contained DATA-ONLY ZIP from validated project and catalogs.
This is for development fixtures; production exports need classification approval.
"""
import os
from pathlib import Path
import sys
import zipfile
from validate import validate_project, validate_package, read_regular_nofollow


def build_package(project_file: Path, destination: Path):
    project_file, destination = Path(project_file), Path(destination)
    project = validate_project(project_file)
    files = [('project.yaml', project_file)] + [(c['file'], project_file.parent / c['file']) for c in project['catalogs']]
    dest = destination.absolute()
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists() and dest.is_symlink():
        raise ValueError('Refusing symlink output')
    tmp = dest.with_name(dest.name + '.tmp')
    try:
        with zipfile.ZipFile(tmp, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as out:
            for name, path in sorted(files):
                entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                entry.compress_type = zipfile.ZIP_DEFLATED
                entry.create_system = 3
                entry.external_attr = 0o100644 << 16
                out.writestr(entry, read_regular_nofollow(path))
        validate_package(tmp)
        os.replace(tmp, dest)
    finally:
        tmp.unlink(missing_ok=True)
    return dest


if __name__ == '__main__':
    if len(sys.argv) != 3:
        raise SystemExit('Usage: python tools/package.py examples/db-pilot.yaml out.zip')
    try:
        print('PASS', build_package(Path(sys.argv[1]), Path(sys.argv[2])))
    except (ValueError, OSError, zipfile.BadZipFile) as exc:
        raise SystemExit(f'FAIL: {exc}')
