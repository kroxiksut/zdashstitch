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


def test_b9_all_schemas_generated_without_drift():
    paths=[ROOT/'schemas'/f for f in ['project-v1.schema.json','project-v2.schema.json','metrics-v1.schema.json',
                                     'local-bindings-v1.schema.json','deployment-state-v1.schema.json']]
    before=[x.read_bytes() for x in paths]
    result=subprocess.run([sys.executable,str(ROOT/'tools/build_schemas.py')],capture_output=True,text=True)
    assert result.returncode==0, result.stderr
    after=[x.read_bytes() for x in paths]
    assert before==after
