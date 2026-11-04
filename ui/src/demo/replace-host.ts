// Synthetic ReplaceHost data for manual accessibility checks of the DEMO build only (P2-03D/E,
// `npm run build:demo`). Every host, item and key below is invented. This module is imported only
// behind `import.meta.env.MODE === 'demo'` in `main.ts`; `scripts/check-dist.mjs` fails the
// normal build if the `demo-synthetic-` IDs leak into it.
//
// `demo-synthetic-01` already has a calculated replacement (mapping, summary, preview with
// blockers and a list to confirm); `demo-synthetic-02` starts the wizard from «Область».

import { createMemoryReplaceHostClient, type ReplaceHostClient } from '../lib/bindings/client';
import type {
  BindingRow,
  Finding,
  ItemFacts,
  ReplaceHostInventory,
  ReplaceHostPreview,
  TargetHostSearch,
  WidgetDiff
} from '../lib/bindings/model';

const at = (day: number, hour: number, minute = 0): number => Date.UTC(2026, 9, day, hour, minute);

function item(itemid: string, key: string, name: string, units = '', valueType = 3, extra: Partial<ItemFacts> = {}): ItemFacts {
  return { itemid, key, name, value_type: valueType, units, template_itemid: '9001', prototype_key: null, ...extra };
}

function row(partial: Partial<BindingRow> & Pick<BindingRow, 'key' | 'widget_type' | 'source' | 'state'>): BindingRow {
  return {
    dashboard_pageid: null,
    widgetid: null,
    target: null,
    match: 'exact',
    applicability: 'editable',
    reasons: [],
    differences: [],
    source_item: null,
    target_item: null,
    candidates: [],
    pattern_coverage: [],
    kept_reason: null,
    decision: null,
    decision_seq: null,
    decision_state: 'none',
    allowed_decisions: ['keep_on_source'],
    unverified: false,
    check: 'checked',
    check_reason: null,
    ...partial
  };
}

const SOURCE = { hostid: '10101', name: 'Демо-узел А' };
const TARGET = { hostid: '10202', name: 'Демо-узел Б' };

const rows: BindingRow[] = [
  row({
    key: { page_index: 0, widget_index: 0, field: 'ds.0.hosts.0' },
    widget_type: 'svggraph',
    source: { kind: 'host_pattern', pattern: SOURCE.name },
    target: { kind: 'host_pattern', pattern: TARGET.name },
    state: 'changed',
    pattern_coverage: [{ pattern: 'Connections*', source_matches: 3, target_matches: 3 }]
  }),
  row({
    key: { page_index: 0, widget_index: 1, field: 'hostids.0' },
    widget_type: 'problems',
    source: { kind: 'host', hostid: SOURCE.hostid },
    target: null,
    match: 'exact',
    applicability: 'read_only',
    reasons: ['host_field_not_supported'],
    state: 'kept_on_source',
    kept_reason: 'host_field_not_supported',
    allowed_decisions: []
  }),
  row({
    key: { page_index: 0, widget_index: 3, field: 'hostids.0' },
    widget_type: 'tophosts',
    source: { kind: 'host', hostid: SOURCE.hostid },
    match: 'exact',
    applicability: 'read_only',
    reasons: ['widget_not_editable'],
    state: 'kept_on_source',
    kept_reason: 'widget_not_editable',
    allowed_decisions: []
  }),
  row({
    key: { page_index: 1, widget_index: 0, field: 'ds.0.itemids.0' },
    widget_type: 'svggraph',
    source: { kind: 'item', itemid: '50001' },
    match: 'ambiguous',
    reasons: ['several_target_candidates'],
    state: 'unresolved',
    source_item: item('50001', 'demo.replication.lag', 'Отставание реплики', 's', 0),
    candidates: [
      item('60001', 'demo.replication.lag', 'Отставание реплики (основной поток)', 's', 0),
      item('60002', 'demo.replication.lag', 'Отставание реплики (резервный поток)', 's', 0, { template_itemid: null })
    ],
    allowed_decisions: ['choose_candidate', 'keep_on_source']
  }),
  row({
    key: { page_index: 1, widget_index: 0, field: 'ds.0.itemids.1' },
    widget_type: 'svggraph',
    source: { kind: 'item', itemid: '50002' },
    match: 'unresolved_lld',
    reasons: ['lld_instance_missing'],
    state: 'unresolved',
    source_item: item('50002', 'demo.db.size[pilotdb]', 'Размер базы pilotdb', 'B', 3, { template_itemid: null, prototype_key: 'demo.db.size[{#DBNAME}]' }),
    candidates: [
      item('60011', 'demo.db.size[demo_a]', 'Размер базы demo_a', 'B', 3, { template_itemid: null, prototype_key: 'demo.db.size[{#DBNAME}]' }),
      item('60012', 'demo.db.size[demo_b]', 'Размер базы demo_b', 'B', 3, { template_itemid: null, prototype_key: 'demo.db.size[{#DBNAME}]' })
    ],
    allowed_decisions: ['choose_lld_instance', 'keep_on_source']
  }),
  row({
    key: { page_index: 1, widget_index: 2, field: 'itemid.0' },
    widget_type: 'item',
    source: { kind: 'item', itemid: '50003' },
    target: { kind: 'item', itemid: '60003' },
    match: 'compatible',
    reasons: ['units_differ'],
    differences: ['units'],
    state: 'changed',
    source_item: item('50003', 'demo.net.in[eth0]', 'Входящий трафик', 'B'),
    target_item: item('60003', 'demo.net.in[eth0]', 'Входящий трафик', 'Bps')
  }),
  row({
    key: { page_index: 1, widget_index: 2, field: 'itemid.1' },
    widget_type: 'item',
    source: { kind: 'item', itemid: '50004' },
    match: 'missing',
    reasons: ['key_not_found'],
    state: 'unresolved',
    source_item: item('50004', 'demo.locks.count', 'Блокировки')
  })
];

function widget(partial: Partial<WidgetDiff> & Pick<WidgetDiff, 'widget_index' | 'widget_type' | 'name' | 'state'>): WidgetDiff {
  return { widgetid: null, in_scope: true, fields: { changed: 0, unresolved: 0, kept_on_source: 0 }, row_fields: [], series: [], ...partial };
}

const counts = (total: number, changed: number, untouched: number, unresolved: number, kept: number) => ({
  total,
  changed,
  untouched,
  unresolved,
  kept_on_source: kept
});

const blocking = (code: string, group: Finding['group'], extra: Partial<Finding> = {}): Finding => ({
  code,
  severity: 'blocks_publication',
  group,
  subject: null,
  reason: null,
  params: {},
  ...extra
});

const subject = (page: number, widgetIndex: number, field: string): Finding['subject'] => ({
  page_index: page,
  widget_index: widgetIndex,
  widgetid: null,
  field
});

const preview: ReplaceHostPreview = {
  local_id: 'demo-synthetic-01',
  revision: 4,
  kind: 'dashboard',
  operation: {
    seq: 1,
    recorded_at: at(26, 8, 30),
    source_host: SOURCE,
    target_host: TARGET,
    scope: { kind: 'dashboard' },
    calculated_at: at(26, 8, 30),
    server_version: '7.4.16'
  },
  rows,
  diff: {
    pages: [
      {
        page_index: 0,
        dashboard_pageid: null,
        name: 'Обзор',
        state: 'changed',
        widget_counts: counts(4, 1, 1, 0, 2),
        widgets: [
          widget({
            widget_index: 0,
            widget_type: 'svggraph',
            name: 'Подключения к базе',
            state: 'changed',
            fields: { changed: 1, unresolved: 0, kept_on_source: 0 },
            row_fields: ['ds.0.hosts.0'],
            series: [
              {
                dataset_index: 0,
                state: 'changed',
                fields: { changed: 1, unresolved: 0, kept_on_source: 0 },
                field_names: ['ds.0.hosts.0'],
                host_patterns: [SOURCE.name],
                item_patterns: ['Connections*'],
                itemids: []
              }
            ]
          }),
          widget({ widget_index: 1, widget_type: 'problems', name: 'Проблемы', state: 'kept_on_source', fields: { changed: 0, unresolved: 0, kept_on_source: 1 }, row_fields: ['hostids.0'] }),
          widget({ widget_index: 2, widget_type: 'clock', name: '', state: 'untouched' }),
          widget({ widget_index: 3, widget_type: 'tophosts', name: 'Самые активные запросы', state: 'kept_on_source', fields: { changed: 0, unresolved: 0, kept_on_source: 1 }, row_fields: ['hostids.0'] })
        ]
      },
      {
        page_index: 1,
        dashboard_pageid: null,
        name: 'Репликация',
        state: 'unresolved',
        widget_counts: counts(3, 0, 1, 2, 0),
        widgets: [
          widget({
            widget_index: 0,
            widget_type: 'svggraph',
            name: 'Отставание реплики',
            state: 'unresolved',
            fields: { changed: 0, unresolved: 2, kept_on_source: 0 },
            row_fields: ['ds.0.itemids.0', 'ds.0.itemids.1'],
            series: [
              {
                dataset_index: 0,
                state: 'unresolved',
                fields: { changed: 0, unresolved: 2, kept_on_source: 0 },
                field_names: ['ds.0.itemids.0', 'ds.0.itemids.1'],
                host_patterns: [],
                item_patterns: [],
                itemids: ['50001', '50002']
              }
            ]
          }),
          widget({ widget_index: 1, widget_type: 'demo_vendor.unknown', name: 'Синтетический виджет стороннего модуля', state: 'untouched' }),
          widget({ widget_index: 2, widget_type: 'item', name: 'Значение', state: 'unresolved', fields: { changed: 1, unresolved: 1, kept_on_source: 0 }, row_fields: ['itemid.0', 'itemid.1'] })
        ]
      },
      {
        page_index: 2,
        dashboard_pageid: null,
        name: '',
        state: 'untouched',
        widget_counts: counts(2, 0, 2, 0, 0),
        widgets: [
          widget({ widget_index: 0, widget_type: 'geomap', name: 'Карта', state: 'untouched' }),
          widget({ widget_index: 1, widget_type: 'url', name: 'Ссылка', state: 'untouched' })
        ]
      }
    ],
    summary: {
      pages: counts(3, 1, 1, 1, 0),
      widgets: counts(9, 1, 4, 2, 2),
      series: counts(2, 1, 0, 1, 0),
      fields: { changed: 2, unresolved: 3, kept_on_source: 2 }
    }
  },
  kept_on_source: {
    entries: [
      { page_index: 0, widget_index: 1, widgetid: null, field: 'hostids.0', widget_type: 'problems', source: { kind: 'host', hostid: SOURCE.hostid }, reason: 'host_field_not_supported', by_decision: false },
      { page_index: 0, widget_index: 3, widgetid: null, field: 'hostids.0', widget_type: 'tophosts', source: { kind: 'host', hostid: SOURCE.hostid }, reason: 'widget_not_editable', by_decision: false }
    ],
    digest: 'demo-synthetic-digest',
    confirmed: false,
    confirmation_seq: null
  },
  changes: [],
  base_fingerprint: 'demo-synthetic-base',
  local_fingerprint: 'demo-synthetic-local',
  checked_at: null,
  report: {
    allows_publication: false,
    checks: [],
    findings: [
      blocking('connection_required', 'connection'),
      blocking('write_contract_unverified', 'connection', { params: { contract: 'svggraph_dataset_hosts' } }),
      blocking('write_contract_unverified', 'connection', { params: { contract: 'update_several_pages' } }),
      blocking('binding_unresolved', 'bindings', { subject: subject(1, 0, 'ds.0.itemids.0'), reason: 'several_target_candidates', params: { match: 'ambiguous' } }),
      blocking('binding_unresolved', 'bindings', { subject: subject(1, 0, 'ds.0.itemids.1'), reason: 'lld_instance_missing', params: { match: 'unresolved_lld' } }),
      blocking('binding_unresolved', 'bindings', { subject: subject(1, 2, 'itemid.1'), reason: 'key_not_found', params: { match: 'missing' } }),
      { code: 'match_differences', severity: 'informational', group: 'bindings', subject: subject(1, 2, 'itemid.0'), reason: null, params: { differences: 'units' } },
      blocking('kept_on_source_unconfirmed', 'widgets', { params: { count: '2', digest: 'demo-synthetic-digest' } })
    ]
  }
};

const inventory: ReplaceHostInventory = {
  local_id: 'demo-synthetic-02',
  kind: 'page',
  pages: [
    {
      page_index: 1,
      dashboard_pageid: '7001',
      name: 'Queues',
      widgets: [
        { widget_index: 0, widgetid: '70011', name: 'Throughput', type: 'svggraph' },
        { widget_index: 1, widgetid: '70012', name: 'Top queues', type: 'topitems' },
        { widget_index: 2, widgetid: '70013', name: 'Consumers', type: 'honeycomb' }
      ]
    }
  ],
  hosts: [
    {
      hostid: '20101',
      name: 'Demo host C',
      host: 'demo-host-c',
      widgets: 3,
      references: [
        { page_index: 1, widget_index: 0, field: 'ds.0.hosts.0', widget_type: 'svggraph', kind: 'host_pattern', outcome: 'changes', reason: null },
        { page_index: 1, widget_index: 0, field: 'ds.1.itemids.0', widget_type: 'svggraph', kind: 'item', outcome: 'changes', reason: null },
        { page_index: 1, widget_index: 1, field: 'hostids.0', widget_type: 'topitems', kind: 'host', outcome: 'kept_on_source', reason: 'host_field_not_supported' },
        { page_index: 1, widget_index: 2, field: 'hostids.0', widget_type: 'honeycomb', kind: 'host', outcome: 'kept_on_source', reason: 'widget_not_editable' }
      ]
    },
    {
      hostid: '20102',
      name: 'Demo host D',
      host: 'demo-host-d',
      widgets: 1,
      references: [{ page_index: 1, widget_index: 0, field: 'ds.2.hosts.0', widget_type: 'svggraph', kind: 'host_mask', outcome: 'kept_on_source', reason: 'host_mask' }]
    }
  ],
  unattributed: [{ page_index: 1, widget_index: 0, field: 'ds.3.itemids.0', widget_type: 'svggraph', kind: 'item' }]
};

const search: TargetHostSearch = {
  hosts: [
    { hostid: '20201', name: 'Demo host E', host: 'demo-host-e', groups: ['Demo/Queues'], templates: ['Demo broker by agent', 'Demo Linux'], source_templates_found: 2 },
    { hostid: '20202', name: 'Demo host F (test)', host: 'demo-host-f', groups: ['Demo/Queues', 'Demo/Test'], templates: ['Demo Linux'], source_templates_found: 1 },
    { hostid: '20203', name: 'Demo backup host', host: 'demo-backup', groups: ['Demo/Backup'], templates: [], source_templates_found: 0 }
  ],
  truncated: false,
  source_templates: ['Demo broker by agent', 'Demo Linux'],
  empty_reason: null
};

/** The queue page preview after «Далее: соответствия»: a small, publishable-looking result. */
const queuesPreview: ReplaceHostPreview = {
  ...preview,
  local_id: 'demo-synthetic-02',
  kind: 'page',
  operation: { ...preview.operation, source_host: { hostid: '20101', name: 'Demo host C' }, target_host: { hostid: '20201', name: 'Demo host E' } },
  rows: [
    row({
      key: { page_index: 1, widget_index: 0, field: 'ds.0.hosts.0' },
      widget_type: 'svggraph',
      source: { kind: 'host_pattern', pattern: 'Demo host C' },
      target: { kind: 'host_pattern', pattern: 'Demo host E' },
      state: 'changed'
    })
  ],
  diff: {
    pages: [
      {
        page_index: 1,
        dashboard_pageid: null,
        name: 'Queues',
        state: 'changed',
        widget_counts: counts(3, 1, 2, 0, 0),
        widgets: [
          widget({ widget_index: 0, widget_type: 'svggraph', name: 'Throughput', state: 'changed', fields: { changed: 1, unresolved: 0, kept_on_source: 0 }, row_fields: ['ds.0.hosts.0'] }),
          widget({ widget_index: 1, widget_type: 'topitems', name: 'Top queues', state: 'untouched' }),
          widget({ widget_index: 2, widget_type: 'honeycomb', name: 'Consumers', state: 'untouched' })
        ]
      }
    ],
    summary: { pages: counts(1, 1, 0, 0, 0), widgets: counts(3, 1, 2, 0, 0), series: counts(1, 1, 0, 0, 0), fields: { changed: 1, unresolved: 0, kept_on_source: 0 } }
  },
  kept_on_source: { entries: [], digest: 'demo-synthetic-empty', confirmed: false, confirmation_seq: null },
  report: { allows_publication: false, checks: [], findings: [blocking('connection_required', 'connection')] }
};

export const DEMO_REPLACE_HOST = { preview, inventory, search, queuesPreview } as const;

export function createDemoReplaceHostClient(): ReplaceHostClient {
  return createMemoryReplaceHostClient({
    'demo-synthetic-01': { preview, calculated: true, inventory: { ...inventory, local_id: 'demo-synthetic-01' }, search },
    'demo-synthetic-02': { preview: queuesPreview, inventory, search }
  });
}
