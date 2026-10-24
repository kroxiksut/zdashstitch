from pathlib import Path
import json,sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from validate import assert_valid, schema

def test_local_bindings():
    obj={
      'format':'zdashstitch.local-bindings/v1','storage_scope':'local_only',
      'project_id':'db-pilot','connection_id':'corp_zabbix','deployment_id':'pilot',
      'roles':{'pg_nodes':[{'slot_id':'pg_a','hostid':'123'},{'slot_id':'pg_b','hostid':'456'}]},
      'metric_overrides':[]
    }
    assert_valid(obj,schema('local-bindings-v1.schema.json'))
