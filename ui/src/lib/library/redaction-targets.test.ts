import { describe, expect, it } from 'vitest';
import type { PortablePageWidget } from './client';
import { redactableHostMaskIndices, redactableTextIndices, redactableItemPatternIndices } from './redaction-targets';

describe('redaction host-mask choices', () => {
  it('allows supported dynamic masks without exposing their values', () => {
    const widget: PortablePageWidget = { id: 'w', type: 'zabbix', widget_type: 'svggraph', name: 'Graph', registry_state: 'known', layout: { mode: 'auto', w: 18, h: 4 }, fields: [
      { name: 'ds.0.hosts.0', field_type: 1, registry_state: 'known', value: { kind: 'dynamic_pattern', value: 'db-*', role: 'hosts' } },
      { name: 'ds.0.items.0', field_type: 1, registry_state: 'known', value: { kind: 'dynamic_pattern', value: 'CPU*', role: 'hosts' } }
    ] };
    expect(redactableHostMaskIndices(widget)).toEqual([0]);
    expect(redactableItemPatternIndices(widget)).toEqual([1]);
  });
  it('offers only known string host masks on known svggraph widgets', () => {
    const widget: PortablePageWidget = { id: 'w', type: 'zabbix', widget_type: 'svggraph', name: 'Graph', registry_state: 'known', layout: { mode: 'auto', w: 18, h: 4 }, fields: [
      { name: 'ds.0.hosts.0', field_type: 1, registry_state: 'known', value: { kind: 'string', value: 'db-*' } },
      { name: 'ds.0.items.0', field_type: 1, registry_state: 'known', value: { kind: 'string', value: 'key' } },
      { name: 'ds.0.hosts.1', field_type: 1, registry_state: 'unknown', value: { kind: 'string', value: 'db-*' } },
      { name: 'ds.0.hosts.2', field_type: 3, registry_state: 'known', value: { kind: 'host' } },
      { name: 'reference', field_type: 1, registry_state: 'known', value: { kind: 'string', value: 'ABC' } },
      { name: 'or.0.hosts.0', field_type: 1, registry_state: 'known', value: { kind: 'string', value: 'db-*' } },
      { name: 'problemhosts.0', field_type: 1, registry_state: 'known', value: { kind: 'string', value: 'db-*' } }
    ] };
    expect(redactableHostMaskIndices(widget)).toEqual([0, 5, 6]);
    expect(redactableHostMaskIndices({ ...widget, registry_state: 'unknown' })).toEqual([]);
    expect(redactableHostMaskIndices({ ...widget, widget_type: 'clock' })).toEqual([]);
  });
});

it('allows labels while protecting metric keys, references and unknown fields', () => {
  const widget: PortablePageWidget = { id: 'w', type: 'zabbix', widget_type: 'svggraph', name: 'Graph', registry_state: 'known', layout: { mode: 'auto', w: 18, h: 4 }, fields: [
    { name: 'ds.0.data_set_label', field_type: 1, registry_state: 'known', value: { kind: 'string', value: 'label' } },
    { name: 'ds.0.items.0', field_type: 1, registry_state: 'known', value: { kind: 'string', value: 'key' } },
    { name: 'reference', field_type: 1, registry_state: 'known', value: { kind: 'string', value: 'ref' } },
    { name: 'ds.1.data_set_label', field_type: 1, registry_state: 'unknown', value: { kind: 'string', value: 'label' } }
  ] };
  expect(redactableTextIndices(widget)).toEqual([0]);
  expect(redactableItemPatternIndices(widget)).toEqual([1]);
  expect(redactableItemPatternIndices({ ...widget, registry_state: 'unknown' })).toEqual([]);
});
