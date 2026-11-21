# SPDX-License-Identifier: MIT OR Apache-2.0

import json
from pathlib import Path

import pytest

from tools.validate import load, validate_reusable

ROOT = Path(__file__).resolve().parents[1]
CASES = json.loads((ROOT / 'fixtures/formats/profiles/negative-cases.json').read_text('utf-8'))
EXAMPLES = {'profile': 'style-profile', 'blueprint': 'widget-blueprint'}


def sample(kind):
    return load(ROOT / f'examples/{EXAMPLES[kind]}.yaml')


@pytest.mark.parametrize('kind', EXAMPLES)
def test_reusable_examples(kind):
    data = sample(kind)
    assert validate_reusable(data) is data


def test_item_background_reset_is_limited_to_the_proven_field_and_adapter():
    profile = sample('profile')
    profile['adapter'] = '8.0'
    style = profile['widget_types']
    style['item']['widget_fields']['bg_color'] = ''
    assert validate_reusable(profile) is profile
    style['item']['widget_fields']['value_color'] = ''
    with pytest.raises(ValueError):
        validate_reusable(profile)
    blueprint = sample('blueprint')
    blueprint['adapter'] = '8.0'
    blueprint['widget_type'] = 'item'
    blueprint['parameters'] = {'roles': [], 'metrics': []}
    blueprint['fields'] = [
        {'name': 'itemid.0', 'field_type': 4, 'value': {'kind': 'unbound', 'object': 'item'}},
        {'name': 'bg_color', 'field_type': 1, 'value': {'kind': 'string', 'value': ''}},
    ]
    assert validate_reusable(blueprint) is blueprint
    blueprint['adapter'] = '7.4'
    with pytest.raises(ValueError):
        validate_reusable(blueprint)


@pytest.mark.parametrize('case', CASES, ids=[case[1] for case in CASES])
def test_reusable_negative_cases(case):
    kind, _, path, *replacement = case
    data = sample(kind)
    node = data
    for key in path[:-1]:
        node = node[key]
    key = path[-1]
    if not replacement:
        del node[key]
    elif isinstance(node, list) and key == len(node):
        node.append(replacement[0])
    else:
        node[key] = replacement[0]
    with pytest.raises(ValueError):
        validate_reusable(data)
