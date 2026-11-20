"""O-041 portable catalog compatibility; no Zabbix API capability is inferred."""
import copy
from pathlib import Path
import sys

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from package import build_package  # noqa: E402
from validate import assert_valid, load, schema, validate_catalog, validate_content, validate_package  # noqa: E402


MIXED = ROOT / 'fixtures/formats/v2/positive/json-mixed-catalog.yaml'


def test_versions_are_disjoint_and_unknown_types_fail():
    mixed = load(MIXED)
    assert set(validate_catalog(mixed)) == {'memory_used', 'structured_status', 'discovered_document'}
    assert_valid(mixed, schema('metrics-v2.schema.json'))
    with pytest.raises(ValueError):
        assert_valid(mixed, schema('metrics-v1.schema.json'))  # old reader refuses v2
    wrong_version = copy.deepcopy(mixed)
    wrong_version['format'] = 'zdashstitch.metrics/v1'
    with pytest.raises(ValueError):
        validate_catalog(wrong_version)
    unknown = copy.deepcopy(mixed)
    unknown['metrics'][1]['value_type'] = 'future_type'
    with pytest.raises(ValueError):
        validate_catalog(unknown)
    unknown['format'] = 'zdashstitch.metrics/v3'
    with pytest.raises(ValueError, match='Unsupported metric catalog format'):
        validate_catalog(unknown)


def test_v2_without_json_stays_v2_and_v1_remains_compatible():
    mixed = load(MIXED)
    mixed['metrics'] = mixed['metrics'][:1]
    assert validate_catalog(mixed)
    encoded = yaml.safe_dump(mixed)
    assert load_from_yaml(encoded)['format'] == 'zdashstitch.metrics/v2'
    old = load(ROOT / 'examples/metrics/linux.yaml')
    assert validate_catalog(old)
    assert load_from_yaml(yaml.safe_dump(old))['format'] == 'zdashstitch.metrics/v1'


def load_from_yaml(text):
    return yaml.safe_load(text)


@pytest.mark.parametrize('project_file', ['db-pilot.yaml', 'page-v2.yaml'])
def test_mixed_catalog_in_legacy_and_typed_data_only_packages(tmp_path, project_file):
    project = load(ROOT / 'examples' / project_file)
    catalogs = {
        ref['id']: load(ROOT / 'examples' / ref['file'])
        for ref in project['catalogs']
    }
    catalogs['linux'] = load(MIXED)
    validate_content(project, catalogs)
    (tmp_path / 'metrics').mkdir()
    for ref in project['catalogs']:
        (tmp_path / ref['file']).write_text(yaml.safe_dump(catalogs[ref['id']]), encoding='utf8')
    project_path = tmp_path / 'project.yaml'
    project_path.write_text(yaml.safe_dump(project), encoding='utf8')
    reopened = validate_package(build_package(project_path, tmp_path / 'mixed.zip'))
    assert reopened == project
    with __import__('zipfile').ZipFile(tmp_path / 'mixed.zip') as archive:
        restored = yaml.safe_load(archive.read('metrics/linux.yaml'))
    assert restored == catalogs['linux']


def test_json_references_are_blocked_in_graph_item_and_type_four():
    project = load(ROOT / 'examples/page-v2.yaml')
    catalog = load(MIXED)
    project['root']['widgets'][0]['series'][0]['metric']['metric'] = 'structured_status'
    with pytest.raises(ValueError, match='Nonnumeric graph'):
        validate_content(project, {'linux': catalog})
    widget = project['root']['widgets'][0]
    widget.clear()
    widget.update({
        'id': 'json-value', 'type': 'item_value', 'title': 'JSON',
        'expansion': 'widget_per_host',
        'metric': {'catalog': 'linux', 'metric': 'structured_status'},
        'hosts': {'role': 'system_nodes', 'selection': 'all'},
        'layout': {'mode': 'auto', 'w': 12, 'h': 4},
    })
    with pytest.raises(ValueError, match='JSON item_value unsupported'):
        validate_content(project, {'linux': catalog})
    widget.clear()
    widget.update({
        'id': 'json-field', 'type': 'zabbix', 'widget_type': 'item',
        'name': 'JSON', 'registry_state': 'known',
        'layout': {'mode': 'auto', 'w': 12, 'h': 4},
        'fields': [{
            'name': 'itemid.0', 'field_type': 4, 'registry_state': 'known',
            'value': {'kind': 'metric', 'catalog': 'linux', 'metric': 'structured_status',
                      'hosts': {'role': 'system_nodes', 'selection': 'one', 'slot_id': 'node-a'}},
        }],
    })
    # Use a singleton role so the field reaches the value-type gate.
    project['roles']['system_nodes'] = {'cardinality': 'one'}
    del widget['fields'][0]['value']['hosts']['slot_id']
    with pytest.raises(ValueError, match='JSON metrics are unsupported'):
        validate_content(project, {'linux': catalog})
