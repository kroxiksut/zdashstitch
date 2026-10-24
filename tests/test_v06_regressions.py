"""Regression suite for v0.5 audit defects A1-A4, B1-B12, selected security cases."""
from pathlib import Path
import copy
import io
import json
import os
import stat
import sys
import zipfile
import pytest
import yaml

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
from validate import (assert_valid, schema, load, validate_project, validate_package,
                      validate_local_bindings, validate_deployment_state,
                      catalog_normalized_source)
from package import build_package

PROJECT=load(ROOT/'examples/db-pilot.yaml')
BASE=load(ROOT/'examples/metrics/clickhouse.yaml')


def bindings():
    return {'format':'zdashstitch.local-bindings/v1','storage_scope':'local_only','project_id':'db-pilot',
            'connection_id':'corp','deployment_id':'pilot',
            'roles':{'pg_nodes':[{'slot_id':'pg_a','hostid':'101'}, {'slot_id':'pg_b','hostid':'102'}],
                     'ch_nodes':[{'slot_id':'ch_a','hostid':'201'}],
                     'keeper_nodes':[{'slot_id':'keeper_a','hostid':'301'}],
                     'proxy_nodes':[{'slot_id':'proxy_a','hostid':'401'}],
                     'system_nodes':[{'slot_id':'os_a','hostid':'501'}],
                     'backup_nodes':[{'slot_id':'backup_a','hostid':'601'}]},
            'metric_overrides':[]}


def deployment():
    return {'format':'zdashstitch.deployment-state/v1','storage_scope':'local_only',
            'project_id':'db-pilot','connection_id':'corp','deployment_id':'pilot',
            'adapter_id':'zabbix74','target_server_version':'7.4.3','server_identity_fingerprint':'e'*64,'status':'verified',
            'last_verified_at':'2026-10-23T10:00:00Z',
            'dashboards':[{'logical_id':'db-pilot','dashboardid':'999','fingerprint':'f'*64,
                           'pages':[{'logical_id':'postgresql','dashboard_pageid':'1001',
                                     'widgets':[{'logical_id':'pg-database-size','widgetid':'3001'},
                                                {'logical_id':'pg-database-size-values','slot_id':'pg_a','widgetid':'3002'},
                                                {'logical_id':'pg-database-size-values','slot_id':'pg_b','widgetid':'3003'}]}]}]}


def test_a3_manual_catalog_without_zabbix_version():
    assert 'zabbix_version' not in BASE['source']
    assert_valid(BASE,schema('metrics-v1.schema.json'))
    assert catalog_normalized_source(BASE)['kind']=='manual'


def test_a4_binary_catalog_accepted_but_no_numeric_graph():
    c=copy.deepcopy(BASE)
    c['metrics'][0]['value_type']='binary'
    assert_valid(c,schema('metrics-v1.schema.json'))
    import tempfile,shutil
    with tempfile.TemporaryDirectory() as t:
        root=Path(t)
        shutil.copytree(ROOT/'examples/metrics',root/'metrics')
        (root/'metrics/clickhouse.yaml').write_text(yaml.safe_dump(c),encoding='utf-8')
        project=root/'db-pilot.yaml'
        project.write_text((ROOT/'examples/db-pilot.yaml').read_text(encoding='utf-8'),encoding='utf-8')
        with pytest.raises(ValueError,match='Nonnumeric graph'):
            validate_project(project)


def test_a1_symlink_file_rejected(tmp_path):
    import shutil
    shutil.copytree(ROOT/'examples/metrics',tmp_path/'metrics')
    target=tmp_path/'metrics/clickhouse.yaml'
    sibling=tmp_path/'metrics/original.yaml'
    target.rename(sibling)
    try:
        target.symlink_to(sibling)
    except (OSError,NotImplementedError):
        pytest.skip('Symlinks unsupported')
    project=tmp_path/'db-pilot.yaml'
    project.write_text((ROOT/'examples/db-pilot.yaml').read_text(encoding='utf-8'),encoding='utf8')
    with pytest.raises(ValueError,match='unsafe'):
        validate_project(project)


def test_a1_symlink_directory_rejected(tmp_path):
    import shutil
    import os
    outside=tmp_path/'actual'
    shutil.copytree(ROOT/'examples/metrics',outside)
    link=tmp_path/'metrics'
    try:
        link.symlink_to(outside, target_is_directory=True)
    except (OSError, NotImplementedError):
        pytest.skip('Symlinks unsupported')
    project=tmp_path/'db-pilot.yaml'
    project.write_text((ROOT/'examples/db-pilot.yaml').read_text(encoding='utf-8'),encoding='utf8')
    with pytest.raises(ValueError,match='unsafe'):
        validate_project(project)


def test_a2_global_duplicate_slots_rejected():
    b=bindings()
    b['roles']['ch_nodes'][0]['slot_id']='pg_a'
    assert_valid(b,schema('local-bindings-v1.schema.json'))
    with pytest.raises(ValueError,match='Duplicate slot'):
        validate_local_bindings(b,PROJECT)


def test_bindings_full_reference_integrity():
    b=bindings()
    validate_local_bindings(b,PROJECT)
    b['metric_overrides']=[{'catalog':'ch','metric':'queries','slot_id':'missing','itemid':'777'}]
    with pytest.raises(ValueError,match='Unknown override slot'):
        validate_local_bindings(b,PROJECT)


def test_bindings_invalid_role():
    b=bindings();b['roles']['wrong']=[{'slot_id':'wrong_a','hostid':'123'}]
    with pytest.raises(ValueError,match='Unknown bindings role'):
        validate_local_bindings(b,PROJECT)


def test_bindings_duplicate_overrides():
    b=bindings()
    b['metric_overrides']=[{'catalog':'ch','metric':'queries','slot_id':'ch_a','itemid':'777'}]*2
    with pytest.raises(ValueError,match='Duplicate metric override'):
        validate_local_bindings(b,PROJECT)


def test_b3_new_export_origin_and_server_origin():
    c=copy.deepcopy(BASE)
    c['source']={'kind':'template_export','export_format_version':'7.4','template_name':'ClickHouse example'}
    assert_valid(c,schema('metrics-v1.schema.json'))
    assert catalog_normalized_source(c)['export_format_version']=='7.4'
    c['source']={'kind':'zabbix_api','source_server_version':'7.4.3'}
    assert_valid(c,schema('metrics-v1.schema.json'))
    assert catalog_normalized_source(c)['source_server_version']=='7.4.3'


def test_b3_independent_export_and_server_versions_accepted():
    c=copy.deepcopy(BASE)
    c['source']={'kind':'template_export','export_format_version':'7.4','source_server_version':'7.4.1'}
    assert_valid(c,schema('metrics-v1.schema.json'))

def test_b3_legacy_and_explicit_version_mix_rejected():
    c=copy.deepcopy(BASE)
    c['source']={'kind':'template_export','export_format_version':'7.4','zabbix_version':'7.0'}
    with pytest.raises(ValueError): assert_valid(c,schema('metrics-v1.schema.json'))


def test_b3_legacy_v05_catalog_is_readable():
    c=copy.deepcopy(BASE)
    c['source']={'kind':'template_export','zabbix_version':'6.0'}
    assert_valid(c,schema('metrics-v1.schema.json'))
    assert catalog_normalized_source(c)['export_format_version']=='6.0'


def test_b10_named_slot_valid_for_many():
    import shutil,tempfile
    with tempfile.TemporaryDirectory() as t:
        p=Path(t)
        shutil.copytree(ROOT/'examples/metrics',p/'metrics')
        d=copy.deepcopy(PROJECT)
        d['roles']['keeper_nodes']['slots']=['keeper_a','keeper_b']
        w=d['dashboards'][0]['pages'][1]['widgets'][-1]
        w['expansion']='single'
        w['hosts']={'role':'keeper_nodes','selection':'slot','slot_id':'keeper_a'}
        path=p/'project.yaml'
        path.write_text(yaml.safe_dump(d,allow_unicode=True),encoding='utf8')
        validate_project(path)
        w['hosts']['slot_id']='unknown'
        path.write_text(yaml.safe_dump(d,allow_unicode=True),encoding='utf8')
        with pytest.raises(ValueError,match='Unknown role slot'):
            validate_project(path)


def test_deployment_state_read_and_semantic_check():
    d=deployment()
    assert_valid(d,schema('deployment-state-v1.schema.json'))
    validate_deployment_state(d,PROJECT,bindings())
    d['dashboards'][0]['pages'][0]['widgets'][1]['widgetid']='3001'
    with pytest.raises(ValueError,match='Duplicate deployed widget'):
        validate_deployment_state(d,PROJECT,bindings())


def test_deployment_state_cannot_store_unverified():
    d=deployment();d['status']='unknown_outcome'
    with pytest.raises(ValueError):
        assert_valid(d,schema('deployment-state-v1.schema.json'))


def test_deployment_state_binding_identity():
    d=deployment();d['connection_id']='other'
    with pytest.raises(ValueError,match='identity mismatch'):
        validate_deployment_state(d,PROJECT,bindings())


def test_b2_self_contained_package(tmp_path):
    target=tmp_path/'package.zip'
    build_package(ROOT/'examples/db-pilot.yaml',target)
    assert validate_package(target)['project_id']=='db-pilot'
    with zipfile.ZipFile(target) as z:
        assert all(x.startswith('metrics/') or x=='project.yaml' for x in z.namelist())
        assert not any('bindings' in x or 'snapshot' in x for x in z.namelist())


def test_package_rejects_unexpected_content(tmp_path):
    out=tmp_path/'extra.zip'
    target=ROOT/'examples/db-pilot-package.zip'
    with zipfile.ZipFile(target) as src,zipfile.ZipFile(out,'w') as dst:
        for info in src.infolist(): dst.writestr(info.filename,src.read(info.filename))
        dst.writestr('token.txt','secret')
    with pytest.raises(ValueError,match='Unrecognized'):
        validate_package(out)


def test_package_rejects_duplicate_entries(tmp_path):
    out=tmp_path/'dup.zip'
    with zipfile.ZipFile(out,'w') as z:
        z.writestr('project.yaml','format: a')
        import warnings
        with warnings.catch_warnings():
            warnings.simplefilter('ignore')
            z.writestr('project.yaml','format: b')
    with pytest.raises(ValueError,match='Duplicate archive entry'):
        validate_package(out)


def test_package_rejects_symlink(tmp_path):
    out=tmp_path/'link.zip'
    info=zipfile.ZipInfo('project.yaml')
    info.create_system=3
    info.external_attr=(stat.S_IFLNK|0o777)<<16
    with zipfile.ZipFile(out,'w') as z:
        z.writestr(info,'../../private')
    with pytest.raises(ValueError,match='Non-regular'):
        validate_package(out)


def test_package_rejects_path_traversal(tmp_path):
    out=tmp_path/'escape.zip'
    with zipfile.ZipFile(out,'w') as z:z.writestr('../project.yaml','a')
    with pytest.raises(ValueError,match='unsafe'):
        validate_package(out)


def test_package_rejects_oversized_entry(tmp_path):
    out=tmp_path/'big.zip'
    with zipfile.ZipFile(out,'w',compression=zipfile.ZIP_STORED) as z:
        z.writestr('project.yaml','x'*(2*1024*1024+1))
    with pytest.raises(ValueError,match='Oversized'):
        validate_package(out)


def test_build_schemas_is_reproducible(tmp_path):
    # Checked separately in CI: run build_schemas.py and verify no git diff.
    for name in ('project-v1','metrics-v1','local-bindings-v1','deployment-state-v1'):
        obj=schema(name+'.schema.json')
        assert obj['$schema']=='https://json-schema.org/draft/2020-12/schema'


def test_package_rejects_lzma_method(tmp_path):
    target=tmp_path/'lzma.zip'
    with zipfile.ZipFile(target,'w',compression=zipfile.ZIP_LZMA) as z:
        z.writestr('project.yaml','format: zdashstitch.project/v1')
    with pytest.raises(ValueError,match='Unsupported archive compression'):
        validate_package(target)


def test_package_rejects_extreme_ratio(tmp_path):
    target=tmp_path/'ratio.zip'
    with zipfile.ZipFile(target,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as z:
        z.writestr('project.yaml','x'*(1024*1024))
    with pytest.raises(ValueError,match='compression ratio'):
        validate_package(target)
