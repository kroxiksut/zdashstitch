"""Schema-level backward compatibility; these are NOT API adapter tests."""
from pathlib import Path
import copy, sys
import pytest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from validate import load, assert_valid, schema, validate_project

ROOT=Path(__file__).resolve().parents[1]
METRICS_SCHEMA=schema('metrics-v1.schema.json')
PROJECT_SCHEMA=schema('project-v1.schema.json')
BASE=load(ROOT/'examples/metrics/clickhouse.yaml')

@pytest.mark.parametrize('origin_version', ['5.0', '5.4', '6.0', '7.0', '7.4', '7.4.3', '8.0', '8.0.0alpha1'])
def test_catalog_source_version_is_metadata(origin_version):
    catalog=copy.deepcopy(BASE)
    catalog['source']['zabbix_version']=origin_version
    assert_valid(catalog,METRICS_SCHEMA)

@pytest.mark.parametrize('malformed_version', ['7', '', 'v7.4', '7.x', '../private', '7.4;drop', '0.0'])
def test_malformed_catalog_source_version_is_rejected(malformed_version):
    catalog=copy.deepcopy(BASE)
    catalog['source']['zabbix_version']=malformed_version
    with pytest.raises(ValueError): assert_valid(catalog,METRICS_SCHEMA)

def test_portable_project_v1_has_no_target_api_version():
    project=load(ROOT/'examples/db-pilot.yaml')
    assert 'zabbix_version' not in project
    assert_valid(project,PROJECT_SCHEMA)
    assert validate_project(ROOT/'examples/db-pilot.yaml')['format']=='zdashstitch.project/v1'

def test_db_pilot_covers_declared_reference_pages_and_clickhouse_sources():
    project=load(ROOT/'examples/db-pilot.yaml')
    pages={page['id']: page for page in project['dashboards'][0]['pages']}
    assert set(pages)=={'postgresql','clickhouse','operating-system','backup'}
    clickhouse_catalogs={
        series['metric']['catalog']
        for widget in pages['clickhouse']['widgets'] if widget['type']=='graph'
        for series in widget['series']
    } | {
        widget['metric']['catalog']
        for widget in pages['clickhouse']['widgets'] if widget['type']=='item_value'
    }
    assert {'ch','keeper','haproxy','linux'} <= clickhouse_catalogs

def test_future_catalog_metadata_does_not_require_rewriting_project(tmp_path):
    import shutil
    shutil.copytree(ROOT/'examples/metrics',tmp_path/'metrics')
    project=tmp_path/'db-pilot.yaml'
    project.write_text((ROOT/'examples/db-pilot.yaml').read_text('utf8'),encoding='utf8')
    import yaml
    ch=tmp_path/'metrics/clickhouse.yaml'
    data=load(ch)
    data['source']['zabbix_version']='6.0'
    ch.write_text(yaml.safe_dump(data,allow_unicode=True,sort_keys=False),encoding='utf8')
    assert validate_project(project)['format']=='zdashstitch.project/v1'
