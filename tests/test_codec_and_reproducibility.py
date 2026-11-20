from pathlib import Path
import copy, subprocess, sys, re
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
from wire_codec_reference import graph_color_to_zabbix


def test_c2_color_normalization():
    assert graph_color_to_zabbix('#aabbcc')=='AABBCC'


def test_c2_invalid_colors():
    import pytest
    for c in ['aabbcc','#zzzzzz','#ABCDE',None]:
        with pytest.raises(ValueError): graph_color_to_zabbix(c)


def test_typed_deployment_records_the_dashboard_root_and_only_confirmed_results():
    import pytest
    from validate import validate_deployment_state

    state = {
        'format': 'zdashstitch.deployment-state/v2',
        'storage_scope': 'local_only',
        'project_id': 'fixture',
        'root_id': 'dashboard',
        'source_revision': 1,
        'connection_id': 'fixture-connection',
        'deployment_id': 'fixture-deployment',
        'adapter_id': 'zabbix-8.0',
        'target_server_version': '8.0.0',
        'server_identity_fingerprint': 'a' * 64,
        'status': 'verified',
        'last_verified_at': '2026-11-20T05:00:00Z',
        'last_operation_id': 'zds-fixture',
        'dashboard_name': 'Synthetic panel',
        'dashboard': {
            'logical_id': 'dashboard', 'dashboardid': '901', 'fingerprint': 'b' * 64,
            'pages': [{
                'logical_id': 'page', 'dashboard_pageid': '902',
                'widgets': [{'logical_id': 'graph', 'widgetid': '903'}],
            }],
        },
    }
    project = {'project_id': 'fixture', 'kind': 'dashboard', 'root': {
        'id': 'dashboard', 'pages': [{'id': 'page', 'widgets': [{'id': 'graph'}]}],
    }}
    assert validate_deployment_state(state, project) == state
    with pytest.raises(ValueError):
        validate_deployment_state(dict(state, root_id='another'), project)
    with pytest.raises(ValueError):
        validate_deployment_state(dict(state, status='unknown_outcome'), project)
    duplicated = copy.deepcopy(state)
    duplicated['dashboard']['pages'][0]['widgets'].append({'logical_id': 'other-graph', 'widgetid': '903'})
    with pytest.raises(ValueError):
        validate_deployment_state(duplicated, project)


def test_b9_all_schemas_generated_without_drift():
    paths=[ROOT/'schemas'/f for f in ['project-v1.schema.json','project-v2.schema.json','metrics-v1.schema.json','metrics-v2.schema.json',
                                     'local-bindings-v1.schema.json','deployment-state-v1.schema.json',
                                     'deployment-state-v2.schema.json']]
    before=[x.read_bytes() for x in paths]
    result=subprocess.run([sys.executable,str(ROOT/'tools/build_schemas.py')],capture_output=True,text=True)
    assert result.returncode==0, result.stderr
    after=[x.read_bytes() for x in paths]
    assert before==after
