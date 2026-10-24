from pathlib import Path
import copy, sys
import pytest
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
from validate import load,validate_local_bindings
from preflight import preflight_expansion,Limits
from test_v06_regressions import bindings


PROJECT=load(ROOT/'examples/db-pilot.yaml')

def test_concrete_expansion_and_identity():
    result=preflight_expansion(PROJECT,bindings())
    pg=result[0]['instances']
    assert len(pg)==3
    assert ('pg-database-size-values','pg_a') in [(x['logical_id'],x['slot_id']) for x in pg]
    assert ('pg-database-size-values','pg_b') in [(x['logical_id'],x['slot_id']) for x in pg]


def test_excess_graph_series_fails_before_publication():
    b=bindings()
    b['roles']['ch_nodes']=[{'slot_id':f'ch_{i}','hostid':str(1000+i)} for i in range(40)]
    validate_local_bindings(b,PROJECT)
    with pytest.raises(ValueError,match='too_many_items'):
        preflight_expansion(PROJECT,b)


def test_repeat_widget_capacity_fails():
    b=bindings()
    b['roles']['keeper_nodes']=[{'slot_id':f'keeper_{i}','hostid':str(1000+i)} for i in range(40)]
    with pytest.raises(ValueError,match='too_many_widgets'):
        preflight_expansion(PROJECT,b,Limits(max_widgets=10))


def test_layout_space_fails_before_publication():
    b=bindings()
    b['roles']['keeper_nodes']=[{'slot_id':f'keeper_{i}','hostid':str(1000+i)} for i in range(40)]
    with pytest.raises(ValueError,match='no_layout_space'):
        preflight_expansion(PROJECT,b,Limits(columns=12,rows=10,max_widgets=100))


def test_failed_missing_slot():
    pr=copy.deepcopy(PROJECT)
    pr['roles']['keeper_nodes']['slots']=['keeper_a','keeper_b']
    w=pr['dashboards'][0]['pages'][1]['widgets'][-1]
    w['expansion']='single'
    w['hosts']={'role':'keeper_nodes','selection':'slot','slot_id':'keeper_b'}
    with pytest.raises(ValueError,match='unresolved_slot'):
        preflight_expansion(pr,bindings())
