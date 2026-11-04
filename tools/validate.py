#!/usr/bin/env python3
"""Development-time JSON Schema and cross-file checks for ZDashstitch v1.

This is NOT the production Rust import security boundary, API compatibility proof,
verified publish-plan constructor or an external-data disclosure approval.
"""
from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import re
import stat
import sys
import unicodedata
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
WIDGET_TYPE_PATTERN = re.compile(r'^[a-z][a-z0-9._-]{0,127}$')
# Page limit of the project/v2 dashboard root, mirrored by Rust PROJECT_V2_MAX_PAGES.
PROJECT_V2_MAX_PAGES = 50
FIELD_OBJECT_KINDS = {
    2: 'host_group', 3: 'host', 4: 'item', 5: 'item_prototype', 6: 'graph',
    7: 'graph_prototype', 8: 'map', 9: 'service', 10: 'sla', 11: 'user',
    12: 'action', 13: 'media_type',
}
VERSION_PATTERN = re.compile(r'^([1-9][0-9]*)\.([0-9]+)(?:\.([0-9]+))?([A-Za-z][A-Za-z0-9._-]*)?$')


class ProjectValidationError(ValueError):
    def __init__(self, code, message):
        super().__init__(f'{code}: {message}')
        self.code = code


class StrictLoader(yaml.SafeLoader):
    def compose_node(self, parent, index):
        if self.check_event(yaml.AliasEvent):
            raise ProjectValidationError('yaml_alias', 'YAML aliases are prohibited')
        event = self.peek_event()
        if getattr(event, 'anchor', None) is not None:
            raise ProjectValidationError('yaml_alias', 'YAML anchors are prohibited')
        return super().compose_node(parent, index)


def construct_mapping(loader, node, deep=False):
    out = {}
    for k, v in node.value:
        key = loader.construct_object(k, deep=deep)
        if not isinstance(key, str):
            raise ProjectValidationError('yaml_key', 'Non-string YAML map key')
        if key in out:
            raise ProjectValidationError('yaml_duplicate_key', 'Duplicate YAML map key')
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
    """Strict YAML load. Error codes are shared with Rust `formats::strict`."""
    if len(data) > MAX_FILE:
        raise ProjectValidationError('file_size', 'File too large')
    try:
        text = data.decode('utf-8')
    except UnicodeDecodeError as exc:
        raise ProjectValidationError('utf8', f'Invalid UTF-8: {exc}') from exc
    try:
        parsed = yaml.load(text, Loader=StrictLoader)
    except RecursionError as exc:
        raise ProjectValidationError('yaml_depth', 'Nesting too deep') from exc
    except yaml.YAMLError as exc:
        raise ProjectValidationError('yaml_invalid', f'Invalid YAML: {exc}') from exc
    if depth(parsed) > MAX_DEPTH:
        raise ProjectValidationError('yaml_depth', 'Nesting too deep')
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


def project_schema_name(project):
    fmt = project.get('format') if isinstance(project, dict) else None
    if fmt == 'zdashstitch.project/v1':
        return 'project-v1.schema.json'
    if fmt == 'zdashstitch.project/v2':
        return 'project-v2.schema.json'
    raise ProjectValidationError('project_format', f'unsupported project format: {fmt!r}')


def parsed_version(value):
    match = VERSION_PATTERN.fullmatch(value) if isinstance(value, str) else None
    if not match:
        raise ProjectValidationError('invalid_version', f'invalid version: {value!r}')
    major, minor, patch, prerelease = match.groups()
    numeric = (int(major), int(minor), int(patch or 0))
    # A prerelease sorts before the stable release with the same numeric core.
    return numeric + ((0, prerelease) if prerelease else (1, ''))


def raw_project_widgets(project):
    """Yield widget mappings from an unvalidated v2 document; malformed parts are skipped."""
    root = project.get('root')
    if not isinstance(root, dict):
        return
    pages = root.get('pages') if 'pages' in root else [root]
    for page in pages if isinstance(pages, list) else []:
        widgets = page.get('widgets') if isinstance(page, dict) else None
        for widget in widgets if isinstance(widgets, list) else []:
            if isinstance(widget, dict):
                yield widget


def validate_zabbix_field_shape(field):
    """Mirror Rust ZabbixWidgetField::validate before JSON Schema so both report one code."""
    name = field.get('name')
    if isinstance(name, str):
        if not 1 <= len(name) <= 255:
            raise ProjectValidationError('widget_field_name', 'widget field name length must be 1..=255')
        if any(unicodedata.category(char) == 'Cc' for char in name):
            raise ProjectValidationError(
                'widget_field_name', 'widget field name cannot contain control characters'
            )
    field_type = field.get('field_type')
    value = field.get('value')
    if type(field_type) is not int or not isinstance(value, dict):
        return
    if field_type > 13 and field.get('registry_state') != 'unknown':
        raise ProjectValidationError(
            'unknown_field_type_state', 'an unknown field type must be marked unknown'
        )
    kind = value.get('kind')
    if kind == 'unresolved' and 2 <= field_type <= 13:
        if value.get('object_kind') != FIELD_OBJECT_KINDS[field_type]:
            raise ProjectValidationError(
                'widget_field_object_kind', 'unresolved object kind does not match field type'
            )
    elif not (
        (kind == 'integer' and field_type == 0)
        or (kind == 'string' and field_type == 1)
        or (kind == 'host' and field_type in {2, 3})
        or (kind == 'metric' and field_type in {4, 5})
        or (kind == 'unresolved' and field_type > 13 and value.get('object_kind') == 'unknown')
    ):
        raise ProjectValidationError(
            'widget_field_value_kind', 'widget field value kind does not match field type'
        )
    text = value.get('value')
    if kind == 'string' and isinstance(text, str) and len(text) > 16384:
        raise ProjectValidationError(
            'widget_field_value', 'widget field string value length must be 0..=16384'
        )


def validate_zabbix_widget_shape(project):
    """Code-stable limits for generic Zabbix widgets, checked before JSON Schema like Rust."""
    for widget in raw_project_widgets(project):
        if widget.get('type') != 'zabbix':
            continue
        widget_type = widget.get('widget_type')
        if isinstance(widget_type, str) and not WIDGET_TYPE_PATTERN.fullmatch(widget_type):
            raise ProjectValidationError(
                'widget_type', 'widget type must be a lowercase adapter identifier'
            )
        name = widget.get('name')
        if isinstance(name, str) and len(name) > 200:
            raise ProjectValidationError('widget_name', 'widget name length must be 0..=200')
        fields = widget.get('fields')
        if not isinstance(fields, list):
            continue
        if len(fields) > 1024:
            raise ProjectValidationError(
                'widget_field_count', 'a widget may contain at most 1024 fields'
            )
        for field in fields:
            if isinstance(field, dict):
                validate_zabbix_field_shape(field)
        names = [field.get('name') for field in fields if isinstance(field, dict)]
        names = [entry for entry in names if isinstance(entry, str)]
        if len(names) != len(set(names)):
            raise ProjectValidationError('duplicate_widget_field', 'duplicate widget field name')


def validate_project_shape(project, expected_format=None):
    fmt = project.get('format') if isinstance(project, dict) else None
    if expected_format is not None and fmt != expected_format:
        raise ProjectValidationError('project_format', f'expected {expected_format}, got {fmt!r}')
    project_schema_name(project)
    if fmt != 'zdashstitch.project/v2':
        return project
    kind = project.get('kind')
    if kind not in {'dashboard', 'page'}:
        raise ProjectValidationError('unknown_project_kind', f'unknown project kind: {kind!r}')
    provenance = project.get('provenance')
    provenance_kind = provenance.get('kind') if isinstance(provenance, dict) else None
    if provenance_kind not in {'local', 'migrated_v1', 'server_snapshot'}:
        raise ProjectValidationError(
            'unknown_provenance_kind', f'unknown provenance kind: {provenance_kind!r}'
        )
    root = project.get('root')
    root_kind = None
    if isinstance(root, dict):
        if 'pages' in root:
            root_kind = 'dashboard'
        elif 'widgets' in root:
            root_kind = 'page'
    if root_kind != kind:
        raise ProjectValidationError('project_kind_root', 'project kind does not match root object')
    if provenance_kind == 'migrated_v1':
        has_page = 'source_page_id' in provenance
        if kind == 'dashboard' and has_page:
            raise ProjectValidationError(
                'dashboard_source_page',
                'dashboard migration provenance cannot contain source_page_id',
            )
        if kind == 'page' and not has_page:
            raise ProjectValidationError(
                'page_source_missing', 'page migration provenance requires source_page_id'
            )
    compatibility = project.get('declared_compatibility')
    if isinstance(compatibility, dict) and 'minimum' in compatibility:
        minimum = parsed_version(compatibility['minimum'])
        if 'maximum_exclusive' in compatibility:
            maximum = parsed_version(compatibility['maximum_exclusive'])
            if minimum >= maximum:
                raise ProjectValidationError(
                    'invalid_version_range',
                    'maximum_exclusive must be greater than minimum',
                )
    if kind == 'dashboard' and isinstance(root.get('pages'), list):
        if not 1 <= len(root['pages']) <= PROJECT_V2_MAX_PAGES:
            raise ProjectValidationError(
                'page_count',
                f'dashboard pages must contain 1..={PROJECT_V2_MAX_PAGES} entries',
            )
    validate_zabbix_widget_shape(project)
    return project


def project_dashboards(project):
    if project['format'] == 'zdashstitch.project/v1':
        return project['dashboards']
    root = project['root']
    if project['kind'] == 'dashboard':
        return [root]
    return [{'id': 'page-root', 'name': 'Page root', 'kind': 'global', 'pages': [root]}]


def validate_portable_host(hosts, roles):
    role_name = hosts['role']
    if role_name not in roles:
        raise ProjectValidationError('unknown_role', f'unknown role {role_name}')
    role = roles[role_name]
    selection = hosts['selection']
    if selection == 'one' and role['cardinality'] != 'one':
        raise ProjectValidationError(
            'one_requires_singleton',
            'one selector requires one cardinality; use a named slot for many',
        )
    if selection == 'slot' and hosts['slot_id'] not in role.get('slots', []):
        raise ProjectValidationError('unknown_role_slot', 'unknown role slot')


def validate_single_reference(hosts, roles):
    """A generic Zabbix widget field holds one object: `all` needs a one-host role."""
    if hosts['selection'] == 'all' and roles[hosts['role']]['cardinality'] != 'one':
        raise ProjectValidationError(
            'widget_single_reference_multi',
            'a widget field references one object; select one host or a named slot',
        )


def validate_content(project, catalogs):
    validate_project_shape(project)
    assert_valid(project, schema(project_schema_name(project)))
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
    for dash in project_dashboards(project):
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
                if w['type'] == 'zabbix':
                    # Field shape, type/value agreement and duplicates are checked before
                    # JSON Schema in validate_zabbix_widget_shape; only cross-file links remain.
                    for field in w['fields']:
                        value = field['value']
                        kind = value['kind']
                        if kind == 'host':
                            validate_portable_host(value, project['roles'])
                            validate_single_reference(value, project['roles'])
                        elif kind == 'metric':
                            validate_portable_host(value['hosts'], project['roles'])
                            validate_single_reference(value['hosts'], project['roles'])
                            if (
                                value['catalog'] not in catalog_data
                                or value['metric'] not in catalog_data[value['catalog']]
                            ):
                                raise ProjectValidationError('unknown_metric', 'unknown metric')
                            metric = catalog_data[value['catalog']][value['metric']]
                            if field['field_type'] == 4 and metric['value_type'] == 'binary':
                                raise ProjectValidationError(
                                    'binary_item_value',
                                    'binary metrics are unsupported in item fields',
                                )
                    references = []
                else:
                    references = (
                        [(s['metric'], s['hosts']) for s in w['series']]
                        if w['type'] == 'graph'
                        else [(w['metric'], w['hosts'])]
                    )
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


def migrate_legacy_project(project, kind, dashboard_id, page_id=None):
    """Explicit, lossless root selection from legacy multi-dashboard project/v1."""
    assert_valid(project, schema('project-v1.schema.json'))
    dashboard = next((d for d in project['dashboards'] if d['id'] == dashboard_id), None)
    if dashboard is None:
        raise ValueError(f'legacy_root_not_found: dashboard {dashboard_id}')
    if kind == 'dashboard':
        if page_id is not None:
            raise ValueError('legacy_selection: page_id is not valid for dashboard migration')
        root = copy.deepcopy(dashboard)
        source_page_id = None
    elif kind == 'page':
        if page_id is None:
            raise ValueError('legacy_selection: page migration requires page_id')
        root = next((copy.deepcopy(p) for p in dashboard['pages'] if p['id'] == page_id), None)
        if root is None:
            raise ValueError(f'legacy_root_not_found: page {dashboard_id}/{page_id}')
        source_page_id = page_id
    else:
        raise ValueError(f'unknown_project_kind: {kind}')
    provenance = {
        'kind': 'migrated_v1',
        'legacy_format': 'zdashstitch.project/v1',
        'source_project_id': project['project_id'],
        'source_dashboard_id': dashboard_id,
    }
    if source_page_id is not None:
        provenance['source_page_id'] = source_page_id
    migrated = {
        'format': 'zdashstitch.project/v2',
        'project_id': project['project_id'],
        'kind': kind,
        'declared_compatibility': {'minimum': '7.4.0'},
        'provenance': provenance,
        'catalogs': copy.deepcopy(project['catalogs']),
        'roles': copy.deepcopy(project['roles']),
        'root': root,
    }
    assert_valid(migrated, schema('project-v2.schema.json'))
    return migrated


def validate_project(path):
    path = Path(path)
    project = load(path)
    validate_project_shape(project)
    assert_valid(project, schema(project_schema_name(project)))
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


def validate_package(path, expected_format=None):
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
        validate_project_shape(project, expected_format)
        assert_valid(project, schema(project_schema_name(project)))
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
