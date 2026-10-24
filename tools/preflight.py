"""Reference-only preflight for expansion and grid placement, not a Zabbix adapter.

The Rust adapter owns the VERIFIED target constraints. These defaults are synthetic
reference constraints for the project-v1 logical grid, not proof of Zabbix limits.
"""
from dataclasses import dataclass


@dataclass(frozen=True)
class Limits:
    columns: int = 72
    rows: int = 64
    max_series: int = 32
    max_widgets: int = 256


def _hosts(selector, bindings, project):
    role = selector['role']
    slots = [e['slot_id'] for e in bindings['roles'][role]]
    if not slots:
        raise ValueError('unresolved_role')
    if len(slots) != len(set(slots)):
        raise ValueError('duplicate_slot_id')
    selection = selector['selection']
    if selection == 'all':
        return slots
    if selection == 'one':
        if project['roles'][role]['cardinality'] != 'one' or len(slots) != 1:
            raise ValueError('not_single_host')
        return slots
    if selection == 'slot':
        wanted = selector['slot_id']
        if wanted not in slots:
            raise ValueError('unresolved_slot')
        return [wanted]
    raise ValueError('unknown_selector')


def _place(boxes, w, h, lim: Limits):
    if w > lim.columns or h > lim.rows:
        raise ValueError('no_layout_space')
    for y in range(lim.rows - h + 1):
        for x in range(lim.columns - w + 1):
            proposed = (x, y, x + w, y + h)
            if all(not (x < old[2] and old[0] < x+w and y < old[3] and old[1] < y+h) for old in boxes):
                boxes.append(proposed)
                return proposed
    raise ValueError('no_layout_space')


def preflight_expansion(project, bindings, limits: Limits | None = None):
    """Raises named blocking issues; returns deterministic intended widget placements.

    Does not validate real item IDs, permissions, adapter widget features or actual
    Zabbix API payload. These MUST be resolved separately before a VerifiedPlan.
    """
    lim = limits or Limits()
    result = []
    for dashboard in project['dashboards']:
        for page in dashboard['pages']:
            fixed, instances = [], []
            for widget in page['widgets']:
                L = widget['layout']
                if L['mode'] == 'fixed':
                    x,y,w,h = L['x'],L['y'],L['w'],L['h']
                    if x+w > lim.columns or y+h > lim.rows:
                        raise ValueError('no_layout_space')
                    fixed.append((x,y,x+w,y+h))
            boxes = fixed.copy()
            for widget in page['widgets']:
                if widget['type'] == 'graph':
                    count = sum(len(_hosts(s['hosts'],bindings,project)) for s in widget['series'])
                    if count > lim.max_series:
                        raise ValueError('too_many_items')
                    repeat_slots = [None]
                else:
                    slots = _hosts(widget['hosts'],bindings,project)
                    repeat_slots = slots if widget['expansion']=='widget_per_host' else [None]
                L = widget['layout']
                if L['mode']=='fixed' and len(repeat_slots)>1:
                    raise ValueError('fixed_repeat_ambiguous')
                for slot_id in repeat_slots:
                    if len(instances)>=lim.max_widgets:
                        raise ValueError('too_many_widgets')
                    if L['mode']=='auto':
                        box=_place(boxes,L['w'],L['h'],lim)
                    else:
                        box=(L['x'],L['y'],L['x']+L['w'],L['y']+L['h'])
                    instances.append({'logical_id':widget['id'],'slot_id':slot_id,
                                      'rect':box})
            result.append({'dashboard':dashboard['id'],'page':page['id'],'instances':instances})
    return result
