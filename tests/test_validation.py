from pathlib import Path
import copy, sys
import pytest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from validate import load, validate_project, assert_valid, schema

ROOT=Path(__file__).resolve().parents[1]
EXAMPLE=ROOT/'examples/db-pilot.yaml'

def test_main_example():
    assert validate_project(EXAMPLE)['format']=='zdashstitch.project/v1'

def test_catalog_redacted():
    obj=load(ROOT/'examples/metrics/redacted_example.yaml')
    assert_valid(obj,schema('metrics-v1.schema.json'))

def test_redacted_rejects_raw_key():
    obj=load(ROOT/'examples/metrics/redacted_example.yaml')
    obj['metrics'][0]['selector']={'kind':'key','value':'internal.secret'}
    with pytest.raises(ValueError):assert_valid(obj,schema('metrics-v1.schema.json'))

def test_unknown_field_rejected():
    obj=load(EXAMPLE)
    obj['dashboards'][0]['pages'][0]['widgets'][0]['free_css']='*{display:none}'
    with pytest.raises(ValueError):assert_valid(obj,schema('project-v1.schema.json'))

def test_path_escape_rejected():
    obj=load(EXAMPLE)
    obj['catalogs'][0]['file']='../private/bindings.yaml'
    with pytest.raises(ValueError):assert_valid(obj,schema('project-v1.schema.json'))

def test_duplicate_yaml_key_rejected(tmp_path):
    p=tmp_path/'bad.yaml';p.write_text('format: a\nformat: b\n',encoding='utf8')
    with pytest.raises(ValueError):load(p)

def test_yaml_alias_rejected(tmp_path):
    p=tmp_path/'bad.yaml';p.write_text('a: &x [1]\nb: *x\n',encoding='utf8')
    with pytest.raises(ValueError):load(p)

def test_unknown_metric_semantic_rejected(tmp_path):
    import shutil
    shutil.copytree(ROOT/'examples/metrics',tmp_path/'metrics')
    p=tmp_path/'db-pilot.yaml'
    p.write_text(EXAMPLE.read_text('utf8').replace('metric: queries','metric: totally_missing'),encoding='utf8')
    with pytest.raises(ValueError,match='Unknown metric'): validate_project(p)

def test_wrong_one_role_semantic_rejected(tmp_path):
    import shutil
    shutil.copytree(ROOT/'examples/metrics',tmp_path/'metrics')
    p=tmp_path/'db-pilot.yaml'
    p.write_text(EXAMPLE.read_text('utf8').replace('selection: all','selection: one',1),encoding='utf8')
    with pytest.raises(ValueError,match='one selector'): validate_project(p)
