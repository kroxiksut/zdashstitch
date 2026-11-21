# SPDX-License-Identifier: MIT OR Apache-2.0
"""Standalone profile/blueprint schema generation from the closed field inventory."""

import json
import re

I32 = {'type': 'integer', 'minimum': -(2**31), 'maximum': 2**31 - 1}
U32 = {'type': 'integer', 'minimum': 1, 'maximum': 2**32 - 1}
SLUG = {'type': 'string', 'pattern': r'^[a-z][a-z0-9_-]{1,63}(?![\s\S])'}
PARAM = {'type': 'string', 'pattern': r'^\$[a-z][a-z0-9_-]{1,63}(?![\s\S])'}
COLOR = {'type': 'string', 'pattern': r'^[0-9A-F]{6}(?![\s\S])'}
UUID = {'type': 'string', 'pattern': r'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}(?![\s\S])'}
VERSION = {'type': 'string', 'pattern': r'^[1-9][0-9]*\.[0-9]+(?:\.[0-9]+)?(?:[A-Za-z][A-Za-z0-9._-]*)?(?![\s\S])'}


def obj(properties, required=(), **kwargs):
    return {'type': 'object', 'properties': properties, 'additionalProperties': False,
            **({'required': list(required)} if required else {}), **kwargs}


def string(limit, minimum=0):
    return {'type': 'string', 'minLength': minimum, 'maxLength': limit}


def arr(items, maximum, minimum=0, **kwargs):
    return {'type': 'array', 'items': items, 'minItems': minimum, 'maxItems': maximum, **kwargs}


def metadata(kind, name_limit):
    return {'format': {'const': f'zdashstitch.{kind}/v1'},
            'name': {**string(name_limit, 1), 'pattern': r'^[^\u0000-\u001F\u007F-\u009F]*\S[^\u0000-\u001F\u007F-\u009F]*(?![\s\S])'},
            'description': string(2000),
            'tags': arr(string(40, 1), 20, uniqueItems=True),
            'version': U32,
            'adapter': {'type': 'string', 'pattern': r'^[0-9]+\.[0-9]+(?![\s\S])'},
            'declared_compatibility': obj({'minimum': VERSION, 'maximum_exclusive': VERSION}, ['minimum'])}


def rule_inventory(fields, widget_type):
    keys = fields[widget_type]
    data = {key: value for kind, names in keys.items() for key in names
            for value in (I32 if kind == 'integer' else
                          string(255 if key == 'units' else 2048) if kind == 'string' else COLOR,) }
    return data


def reset_adapters(fields, widget_type, name):
    return [adapter for adapter, keys in fields['empty_color_resets'].items()
            if f'{widget_type}.{name}' in keys]


def style_schema(fields):
    graph = rule_inventory(fields, 'svggraph')
    item = {**rule_inventory(fields, 'item'), 'show': arr({'enum': [1, 2, 3, 4, 5]}, 5, 1, uniqueItems=True)}
    reset_rules = []
    for key, rule in tuple(item.items()):
        adapters = reset_adapters(fields, 'item', key)
        if adapters:
            item[key] = {'anyOf': [rule, {'const': ''}]}
            reset_rules.append({
                'if': {'properties': {'adapter': {'not': {'enum': adapters}}}},
                'then': {'properties': {'widget_types': {'properties': {'item': {'properties': {
                    'widget_fields': {'properties': {key: rule}},
                }}}}}},
            })
    attributes = obj({'view_mode': {'enum': [0, 1]}}, ['view_mode'])
    plot = obj({
        'widget_fields': obj(graph),
        'dataset_fields': obj(dict.fromkeys(fields['dataset_fields'], I32)),
        'series_palette': arr(COLOR, 64, 1),
        'color_palette': {**I32, 'minimum': 0},
        'attributes': attributes,
    }, anyOf=[
        {'required': ['widget_fields'], 'properties': {'widget_fields': {'minProperties': 1}}},
        {'required': ['dataset_fields'], 'properties': {'dataset_fields': {'minProperties': 1}}},
        *({'required': [name]} for name in ['series_palette', 'color_palette', 'attributes']),
    ], **{'not': {'required': ['series_palette', 'color_palette']}})
    thresholds = arr(obj({
        'threshold': {**string(32, 1), 'pattern': r'^[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?(?![\s\S])'},
        'color': COLOR,
    }, ['threshold', 'color']), 64)
    indicator = obj({'widget_fields': obj(item, ['show']), 'thresholds': thresholds,
                     'thresholds_units': string(32), 'attributes': attributes}, ['widget_fields'],
                    dependentSchemas={'thresholds_units': {
                        'required': ['thresholds'], 'properties': {'thresholds': {'minItems': 1}}}})
    return obj({**metadata('style-profile', 120), 'profile_id': UUID,
                'widget_types': obj({'item': indicator, 'svggraph': plot}, minProperties=1)},
               ['format', 'profile_id', 'name', 'version', 'adapter', 'widget_types'], allOf=reset_rules)


def reference(kind, *, direct):
    properties = {'kind': {'const': kind}, 'role': PARAM,
                  'selection': {'enum': ['one', 'slot'] if direct else ['all', 'one', 'slot']},
                  'slot_id': SLUG}
    if kind == 'metric':
        properties['metric'] = PARAM
    return obj(properties, [name for name in properties if name != 'slot_id'], allOf=[{
        'if': {'properties': {'selection': {'const': 'slot'}}},
        'then': {'required': ['slot_id']}, 'else': {'not': {'required': ['slot_id']}},
    }])


def field_defs(fields, widget_type, *, allow_reset=False):
    inventory = {}
    for group in (fields[widget_type], fields['blueprint_extra'][widget_type]):
        for kind, names in group.items():
            inventory.setdefault(kind, []).extend(names)
    if widget_type == 'svggraph':
        inventory['integer'].extend(f'ds.#.{name}' for name in fields['dataset_fields'])
    variants = []
    if allow_reset:
        reset_names = [name for name in inventory.get('color', [])
                       if reset_adapters(fields, widget_type, name)]
        if reset_names:
            variants.append(obj({'name': {'enum': reset_names}, 'field_type': {'const': 1},
                                 'value': obj({'kind': {'const': 'string'}, 'value': {'const': ''}},
                                              ['kind', 'value'])}, ['name', 'field_type', 'value']))
    for kind, names in inventory.items():
        if kind in ('integer', 'string', 'color'):
            field_type = 0 if kind == 'integer' else 1
            value = obj({'kind': {'const': 'integer' if field_type == 0 else 'string'},
                         'value': {'integer': I32, 'string': string(2048), 'color': COLOR}[kind]},
                        ['kind', 'value'])
        else:
            field_type = {'host': 3, 'metric': 4}.get(kind, 1)
            ref_kind = kind.removeprefix('pattern_')
            value = {'oneOf': [reference(ref_kind, direct=field_type != 1),
                               obj({'kind': {'const': 'unbound'},
                                    'object': {'const': 'host' if ref_kind == 'host' else 'item'}},
                                   ['kind', 'object'])]}
        escaped = '|'.join(re.escape(name).replace(r'\#', '(?:0|[1-9][0-9]*)') for name in names)
        variants.append(obj({'name': {'type': 'string', 'maxLength': 128,
                                      'pattern': f'^(?:{escaped})(?![\\s\\S])'},
                             'field_type': {'const': field_type}, 'value': value},
                            ['name', 'field_type', 'value']))
    return {'oneOf': variants}


def blueprint_schema(fields):
    role = obj({'param': SLUG, 'cardinality': {'enum': ['one', 'many']},
                'slots': arr(SLUG, 256, 1, uniqueItems=True), 'description': string(500)},
               ['param', 'cardinality'], allOf=[{
                   'if': {'required': ['slots']}, 'then': {'properties': {'cardinality': {'const': 'many'}}},
               }])
    metric = obj({'param': SLUG, 'value_type': {'enum': ['numeric_float', 'numeric_unsigned', 'character', 'log', 'text']},
                  'units': string(255), 'description': string(500)}, ['param'])
    result = obj({**metadata('widget-blueprint', 255), 'blueprint_id': UUID,
                  'widget_type': {'enum': ['item', 'svggraph']},
                  'default_size': obj({'width': {'type': 'integer', 'minimum': 1, 'maximum': 72},
                                       'height': {'type': 'integer', 'minimum': 1, 'maximum': 64}},
                                      ['width', 'height']),
                  'attributes': obj({'view_mode': {'enum': [0, 1]}, 'rf_rate': I32}),
                  'parameters': obj({'roles': arr(role, 64), 'metrics': arr(metric, 1024)}, ['roles', 'metrics']),
                  'fields': {'type': 'array', 'maxItems': 1024, 'minItems': 1}},
                 ['format', 'blueprint_id', 'name', 'version', 'adapter', 'widget_type', 'default_size', 'parameters', 'fields'])
    conditions = []
    for widget_type in ('item', 'svggraph'):
        adapters = [adapter for adapter, names in fields['empty_color_resets'].items()
                    if any(name.startswith(f'{widget_type}.') for name in names)]
        simple = {'properties': {'fields': {'items': field_defs(fields, widget_type)}}}
        allowed = simple if not adapters else {
            'if': {'properties': {'adapter': {'enum': adapters}}},
            'then': {'properties': {'fields': {'items': field_defs(fields, widget_type, allow_reset=True)}}},
            'else': simple,
        }
        conditions.append({'if': {'properties': {'widget_type': {'const': widget_type}}}, 'then': allowed})
    result['allOf'] = conditions
    return result


def reusable_schemas(root):
    fields = json.loads((root / 'schemas/profile-fields.json').read_text(encoding='utf-8'))
    specs = [('style-profile.schema.json', 'ZDashstitch style profile v1', style_schema(fields)),
             ('widget-blueprint.schema.json', 'ZDashstitch widget blueprint v1', blueprint_schema(fields))]
    return [(filename, {'$schema': 'https://json-schema.org/draft/2020-12/schema',
                        '$id': f'https://zdashstitch.example/schemas/{filename}',
                        'title': title, **descriptor})
            for filename, title, descriptor in specs]
