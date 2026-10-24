#!/usr/bin/env python3
"""Development-time JSON Schema and cross-file checks for ZDashstitch v1.

This is NOT the production Rust import security boundary, API compatibility proof,
verified publish-plan constructor or an external-data disclosure approval.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import stat
import sys
import zipfile

import yaml
from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parent.parent
MAX_FILE = 2 * 1024 * 1024
MAX_TOTAL = 32 * 1024 * 1024
MAX_DEPTH = 40
MAX_ARCHIVE_ENTRIES = 33  # project.yaml and up to 32 catalogs
MAX_COMPRESSION_RATIO = 300
ARCHIVE_PROJECT = 'project.yaml'
CATALOG_PATH = re.compile(r'^metrics/[a-z][a-z0-9_-]{1,63}\.ya?ml$')


class StrictLoader(yaml.SafeLoader):
    def compose_node(self, parent, index):
        if self.check_event(yaml.AliasEvent):
            raise ValueError('YAML aliases are prohibited')
        return super().compose_node(parent, index)


def construct_mapping(loader, node, deep=False):
    out = {}
    for k, v in node.value:
        key = loader.construct_object(k, deep=deep)
        if not isinstance(key, str) or key in out:
            raise ValueError('Duplicate or non-string YAML map key')
        out[key] = loader.construct_object(v, deep=deep)
    return out


StrictLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, construct_mapping)


def depth(obj):
    if isinstance(obj, dict):
        return 1 + max([depth(v) for v in obj.values()] or [0])
    if isinstance(obj, list):
        return 1 + max([depth(v) for v in obj] or [0])
    return 0


def load_bytes(data: bytes):
    if len(data) > MAX_FILE:
        raise ValueError('File too large')
    try:
        parsed = yaml.load(data.decode('utf-8'), Loader=StrictLoader)
    except (UnicodeDecodeError, yaml.YAMLError, RecursionError) as exc:
        raise ValueError(f'Invalid UTF-8/YAML: {exc}') from exc
    if depth(parsed) > MAX_DEPTH:
        raise ValueError('Nesting too deep')
    return parsed


def read_regular_nofollow(path: Path) -> bytes:
    """Best-effort dev validator; uses O_NOFOLLOW + dirfd traversal on POSIX.

    Production Rust must perform equivalent OS-appropriate no-follow opening,
    not merely check a path before normal open (TOCTOU).
    """
    path = Path(path)
    if not path.is_absolute():
        path = path.absolute()
    if sys.platform != 'win32' and hasattr(os, 'O_NOFOLLOW'):
        parts = path.parts
        root_fd = os.open(parts[0], os.O_RDONLY | os.O_DIRECTORY)
        try:
            fd = root_fd
            for part in parts[1:-1]:
                next_fd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                if fd != root_fd:
                    os.close(fd)
                fd = next_fd
            file_fd = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | getattr(os, 'O_NONBLOCK', 0), dir_fd=fd)
            try:
                info = os.fstat(file_fd)
                if not stat.S_ISREG(info.st_mode) or info.st_size > MAX_FILE:
                    raise ValueError('Unsafe type or oversized file')
                raw = os.read(file_fd, MAX_FILE + 1)
                if len(raw) > MAX_FILE:
                    raise ValueError('File too large')
                return raw
            finally:
                os.close(file_fd)
        finally:
            if fd != root_fd:
                os.close(fd)
            os.close(root_fd)
    # Windows developer fallback; the Rust production importer must open without
    # following reparse points (this precheck is not a TOCTOU guarantee).
    if any(part.is_symlink() for part in [path, *path.parents]):
        raise ValueError('Symlinks/reparse-point path components forbidden')
    if not path.is_file() or path.stat().st_size > MAX_FILE:
        raise ValueError('Unsafe type or oversized file')
    return path.read_bytes()


def load(path):
    return load_bytes(read_regular_nofollow(Path(path)))


def schema(filename):
    return json.loads((ROOT / 'schemas' / filename).read_text('utf8'))


def assert_valid(data, sc):
    problems = list(Draft202012Validator(sc, format_checker=FormatChecker()).iter_errors(data))
    if problems:
        raise ValueError('Schema: ' + ' | '.join(x.message for x in problems[:5]))


def catalog_normalized_source(entry):
    """Compatibility reader: v0.5 legacy zabbix_version is input-only."""
    src = entry['source']
    kind = src['kind']
    out = {'kind': kind}
    if kind == 'template_export':
        out['export_format_version'] = src.get('export_format_version', src.get('zabbix_version'))
    elif kind == 'zabbix_api':
        out['source_server_version'] = src.get('source_server_version', src.get('zabbix_version'))
    if kind == 'template_export' and 'source_server_version' in src:
        out['source_server_version'] = src['source_server_version']
    for key in ('template_name', 'template_uuid'):
        if key in src:
            out[key] = src[key]
    return out


def validate_catalog(entry):
    assert_valid(entry, schema('metrics-v1.schema.json'))
    ids = [m['id'] for m in entry['metrics']]
    if len(ids) != len(set(ids)):
        raise ValueError('Duplicate metric id')
    return {m['id']: m for m in entry['metrics']}


def validate_content(project, catalogs):
    assert_valid(project, schema('project-v1.schema.json'))
    declared = [c['id'] for c in project['catalogs']]
    if len(declared) != len(set(declared)):
        raise ValueError('Duplicate catalog ID')
    if set(catalogs) != set(declared):
        raise ValueError('Missing or extra catalog')
    catalog_data = {}
    for spec in project['catalogs']:
        entry = catalogs[spec['id']]
        if entry['catalog_id'] != spec['id']:
            raise ValueError('Catalog ID mismatch')
        catalog_data[spec['id']] = validate_catalog(entry)
    dashids = set()
    for dash in project['dashboards']:
        if dash['id'] in dashids:
            raise ValueError('Duplicate dashboard id')
        dashids.add(dash['id'])
        pageids = set()
        for page in dash['pages']:
            if page['id'] in pageids:
                raise ValueError('Duplicate page id')
            pageids.add(page['id'])
            widgetids, rectangles = set(), []
            for w in page['widgets']:
                if w['id'] in widgetids:
                    raise ValueError('Duplicate widget id')
                widgetids.add(w['id'])
                references = [(s['metric'], s['hosts']) for s in w['series']] if w['type'] == 'graph' else [(w['metric'], w['hosts'])]
                for metric, hosts in references:
                    role = hosts['role']
                    if role not in project['roles']:
                        raise ValueError('Unknown role')
                    cardinality = project['roles'][role]['cardinality']
                    selection = hosts['selection']
                    if selection == 'one' and cardinality != 'one':
                        raise ValueError('one selector requires one cardinality; use a named slot for many')
                    if selection == 'slot':
                        declared_slots = project['roles'][role].get('slots', [])
                        if hosts['slot_id'] not in declared_slots:
                            raise ValueError('Unknown role slot')
                    if metric['catalog'] not in catalog_data or metric['metric'] not in catalog_data[metric['catalog']]:
                        raise ValueError('Unknown metric')
                    m = catalog_data[metric['catalog']][metric['metric']]
                    if w['type'] == 'graph' and not m['value_type'].startswith('numeric_'):
                        raise ValueError('Nonnumeric graph series')
                    if w['type'] == 'item_value' and m['value_type'] == 'binary':
                        raise ValueError('Binary item_value unsupported by MVP')
                if w['type'] == 'item_value':
                    if w['expansion'] == 'single' and w['hosts']['selection'] == 'all':
                        # Singleton cardinality is statically known, otherwise runtime check required.
                        if project['roles'][w['hosts']['role']]['cardinality'] != 'one':
                            raise ValueError('single value needs exactly one host selector')
                    if w['expansion'] == 'widget_per_host' and w['hosts']['selection'] != 'all':
                        raise ValueError('repeat needs all selection')
                if w['layout']['mode'] == 'fixed':
                    L = w['layout']
                    if L['x'] + L['w'] > 72 or L['y'] + L['h'] > 64:
                        raise ValueError('Fixed widget outside 72x64')
                    box = (L['x'], L['y'], L['x'] + L['w'], L['y'] + L['h'])
                    for old in rectangles:
                        if box[0] < old[2] and old[0] < box[2] and box[1] < old[3] and old[1] < box[3]:
                            raise ValueError('Overlapping fixed widgets')
                    rectangles.append(box)
    return project


def validate_project(path):
    path = Path(path)
    project = load(path)
    assert_valid(project, schema('project-v1.schema.json'))
    catalog_data = {}
    for spec in project['catalogs']:
        rel = spec['file']
        if not CATALOG_PATH.fullmatch(rel):
            raise ValueError('Unsafe catalog path')
        try:
            catalog_data[spec['id']] = load(path.parent / rel)
        except (OSError, ValueError) as exc:
            raise ValueError('Missing/unsafe catalog file ' + rel) from exc
    return validate_content(project, catalog_data)


def validate_local_bindings(data, project=None, catalogs=None):
    assert_valid(data, schema('local-bindings-v1.schema.json'))
    if project is not None and data['project_id'] != project['project_id']:
        raise ValueError('Bindings/project identity mismatch')
    slots = set()
    for role, entries in data['roles'].items():
        if project is not None:
            if role not in project['roles']:
                raise ValueError('Unknown bindings role')
            if project['roles'][role]['cardinality'] == 'one' and len(entries) != 1:
                raise ValueError('Singleton role needs one host binding')
        local = set()
        for e in entries:
            slot = e['slot_id']
            if slot in local or slot in slots:
                raise ValueError('Duplicate slot_id')
            local.add(slot)
            slots.add(slot)
        if project is not None:
            declared = set(project['roles'][role].get('slots', []))
            if declared and not declared.issubset(local):
                raise ValueError('Missing required role slot')
    seen = set()
    for override in data.get('metric_overrides', []):
        key = (override['catalog'], override['metric'], override['slot_id'])
        if key in seen:
            raise ValueError('Duplicate metric override')
        seen.add(key)
        if override['slot_id'] not in slots:
            raise ValueError('Unknown override slot')
        if catalogs is not None and (override['catalog'] not in catalogs or override['metric'] not in catalogs[override['catalog']]):
            raise ValueError('Unknown override metric')
        if 'role' in override and override['slot_id'] not in {e['slot_id'] for e in data['roles'].get(override['role'], [])}:
            raise ValueError('Override role/slot mismatch')
    return data


def validate_deployment_state(data, project=None, bindings=None):
    assert_valid(data, schema('deployment-state-v1.schema.json'))
    if project is not None and data['project_id'] != project['project_id']:
        raise ValueError('Deployment/project identity mismatch')
    if bindings is not None:
        for k in ('project_id', 'connection_id', 'deployment_id'):
            if data[k] != bindings[k]:
                raise ValueError('Deployment/bindings identity mismatch')
    project_dashes = {d['id']: d for d in project['dashboards']} if project else {}
    deployed_dashes, zabbix_dashes = set(), set()
    for dash in data['dashboards']:
        if dash['logical_id'] in deployed_dashes or dash['dashboardid'] in zabbix_dashes:
            raise ValueError('Duplicate deployment dashboard identity')
        deployed_dashes.add(dash['logical_id'])
        zabbix_dashes.add(dash['dashboardid'])
        if project and dash['logical_id'] not in project_dashes:
            raise ValueError('Unknown deployment dashboard')
        pages = {p['id']: p for p in project_dashes[dash['logical_id']]['pages']} if project else {}
        deployed_pages, zabbix_pages, references = set(), set(), set()
        for page in dash['pages']:
            if page['logical_id'] in deployed_pages or page['dashboard_pageid'] in zabbix_pages:
                raise ValueError('Duplicate deployment page identity')
            deployed_pages.add(page['logical_id'])
            zabbix_pages.add(page['dashboard_pageid'])
            if project and page['logical_id'] not in pages:
                raise ValueError('Unknown deployment page')
            allowed_widgets = {w['id']: w for w in pages[page['logical_id']]['widgets']} if project else {}
            identities, zabbix_widgets = set(), set()
            for w in page['widgets']:
                key = (w['logical_id'], w.get('slot_id'))
                if key in identities or w['widgetid'] in zabbix_widgets:
                    raise ValueError('Duplicate deployed widget identity')
                identities.add(key)
                zabbix_widgets.add(w['widgetid'])
                if 'reference' in w:
                    if w['reference'] in references:
                        raise ValueError('Duplicate widget reference')
                    references.add(w['reference'])
                if project:
                    if w['logical_id'] not in allowed_widgets:
                        raise ValueError('Unknown deployment widget')
                    if w.get('slot_id') and allowed_widgets[w['logical_id']]['expansion'] != 'widget_per_host':
                        raise ValueError('Unexpected slot on widget identity')
                if bindings is not None and 'slot_id' in w:
                    known = {e['slot_id'] for role in bindings['roles'].values() for e in role}
                    if w['slot_id'] not in known:
                        raise ValueError('Deployment references unknown slot')
    return data


def validate_package(path):
    path = Path(path)
    # Read package without extraction. No executable files or arbitrary resources.
    if not zipfile.is_zipfile(path):
        raise ValueError('Not a ZIP file')
    with zipfile.ZipFile(path, 'r') as archive:
        infos = archive.infolist()
        if len(infos) > MAX_ARCHIVE_ENTRIES:
            raise ValueError('Too many archive entries')
        names, raw = set(), {}
        total = 0
        for info in infos:
            name = info.filename
            if name in names:
                raise ValueError('Duplicate archive entry')
            names.add(name)
            # Forbid alternate separators, traversal, dirs, symlinks, encrypted entries.
            if name != ARCHIVE_PROJECT and not CATALOG_PATH.fullmatch(name):
                raise ValueError('Unrecognized/unsafe archive entry')
            mode = (info.external_attr >> 16) & 0o170000
            if info.is_dir() or mode not in (0, stat.S_IFREG) or info.flag_bits & 1:
                raise ValueError('Non-regular/encrypted archive entry')
            if info.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
                raise ValueError('Unsupported archive compression')
            if info.file_size > MAX_FILE:
                raise ValueError('Oversized archive entry')
            total += info.file_size
            if total > MAX_TOTAL:
                raise ValueError('Archive exceeds total size limit')
            if info.file_size and info.file_size / max(info.compress_size, 1) > MAX_COMPRESSION_RATIO:
                raise ValueError('Suspicious compression ratio')
            with archive.open(info, 'r') as file:
                content = file.read(MAX_FILE + 1)
            if len(content) != info.file_size:
                raise ValueError('Inconsistent archive entry size')
            raw[name] = content
        if ARCHIVE_PROJECT not in raw:
            raise ValueError('Package has no project.yaml')
        project = load_bytes(raw[ARCHIVE_PROJECT])
        assert_valid(project, schema('project-v1.schema.json'))
        needed = {c['file'] for c in project['catalogs']} | {ARCHIVE_PROJECT}
        if names != needed:
            raise ValueError('Missing or unexpected catalog entries')
        catalogs = {c['id']: load_bytes(raw[c['file']]) for c in project['catalogs']}
        return validate_content(project, catalogs)


if __name__ == '__main__':
    argv = sys.argv[1:]
    try:
        if argv and argv[0] == '--package':
            if len(argv) != 2:
                raise ValueError('Usage: validate.py --package FILE.zip')
            validate_package(argv[1])
            print('PASS package', argv[1])
        else:
            p = Path(argv[0]) if argv else ROOT / 'examples/db-pilot.yaml'
            validate_project(p)
            print('PASS project', p)
    except (OSError, ValueError, zipfile.BadZipFile, yaml.YAMLError) as exc:
        print('FAIL', exc, file=sys.stderr)
        raise SystemExit(1)
