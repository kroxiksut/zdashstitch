from pathlib import Path
import json
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))

from package import build_package  # noqa: E402
from validate import (  # noqa: E402
    assert_valid,
    load,
    migrate_legacy_project,
    ProjectValidationError,
    schema,
    validate_package,
    validate_project,
)


@pytest.mark.parametrize(('filename', 'kind'), [('dashboard-v2.yaml', 'dashboard'), ('page-v2.yaml', 'page')])
def test_typed_project_round_trip_through_data_only_zip(tmp_path, filename, kind):
    source = ROOT / 'examples' / filename
    project = validate_project(source)
    assert project['kind'] == kind
    package = build_package(source, tmp_path / f'{kind}.zip')
    reopened = validate_package(package)
    assert reopened == project
    assert reopened['root'] == project['root']


@pytest.mark.parametrize('filename', ['clock-page-v2.yaml', 'global-view-v2.yaml'])
def test_generic_zabbix_widgets_and_empty_bindings_round_trip(tmp_path, filename):
    source = ROOT / 'examples' / filename
    project = validate_project(source)
    assert project['catalogs'] == []
    assert project['roles'] == {}
    package = build_package(source, tmp_path / f'{source.stem}.zip')
    assert validate_package(package) == project


def test_unknown_widget_and_field_type_are_preserved_with_markers(tmp_path):
    source = ROOT / 'fixtures/formats/v2/positive/unknown-widget.yaml'
    project = validate_project(source)
    widget = project['root']['widgets'][0]
    assert widget['registry_state'] == 'unknown'
    assert widget['fields'][1]['field_type'] == 42
    assert widget['fields'][1]['registry_state'] == 'unknown'
    assert validate_package(build_package(source, tmp_path / 'unknown.zip')) == project


def test_page_source_provenance_is_not_target_intent():
    page = validate_project(ROOT / 'examples/page-v2.yaml')
    assert page['provenance']['source_dashboard_id'] == 'db-pilot'
    assert not any(key.startswith('target') for key in page)


def test_explicit_legacy_migration_selects_one_dashboard_or_page():
    legacy = load(ROOT / 'examples/db-pilot.yaml')
    dashboard = migrate_legacy_project(legacy, 'dashboard', 'db-pilot')
    assert dashboard['kind'] == 'dashboard'
    assert dashboard['root']['id'] == 'db-pilot'
    page = migrate_legacy_project(legacy, 'page', 'db-pilot', 'clickhouse')
    assert page['kind'] == 'page'
    assert page['root']['id'] == 'clickhouse'
    assert page['provenance']['source_dashboard_id'] == 'db-pilot'
    assert 'target_dashboard_id' not in page


def test_legacy_migration_never_guesses_a_missing_root():
    legacy = load(ROOT / 'examples/db-pilot.yaml')
    with pytest.raises(ValueError, match='legacy_root_not_found'):
        migrate_legacy_project(legacy, 'dashboard', 'missing-dashboard')
    with pytest.raises(ValueError, match='legacy_selection'):
        migrate_legacy_project(legacy, 'page', 'db-pilot')


@pytest.mark.parametrize('filename', ['unknown-kind.yaml', 'target-intent.yaml'])
def test_unknown_kind_and_target_intent_are_rejected(filename):
    document = load(ROOT / 'fixtures/formats/v2/negative' / filename)
    with pytest.raises(ValueError, match='Schema'):
        assert_valid(document, schema('project-v2.schema.json'))


def test_shared_v2_negative_cases_have_stable_codes(tmp_path):
    fixture_root = ROOT / 'fixtures/formats/v2'
    cases = json.loads((fixture_root / 'negative-cases.json').read_text('utf8'))
    observed = {}
    for case in cases:
        with pytest.raises(ProjectValidationError) as failure:
            if case['type'] == 'document':
                validate_project(fixture_root / case['file'])
            else:
                package = build_package(ROOT / case['source'], tmp_path / f"{case['name']}.zip")
                expected = (
                    'zdashstitch.project/v1'
                    if case['reader'] == 'v1'
                    else 'zdashstitch.project/v2'
                )
                validate_package(package, expected_format=expected)
        observed[case['name']] = failure.value.code
    assert observed == {case['name']: case['code'] for case in cases}
