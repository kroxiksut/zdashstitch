// Synthetic publication data for manual checks of the DEMO build only (P2-04E/F,
// `npm run build:demo`). Every name and identifier below is invented. This module is imported
// only behind `import.meta.env.MODE === 'demo'` in `main.ts`; `scripts/check-dist.mjs` fails the
// normal build if the `demo-synthetic-` IDs leak into it. Nothing here reaches a server.
//
// - `demo-synthetic-01`: the dashboard changed on the server — the conflict screen (11-05) with
//   «Сохранить копию»; a copy ends with an unknown result (11-08) and its reconciliation finds
//   nothing, which is a manual check (OPEN_DECISIONS 28.10.2026 evening, item 1).
// - `demo-synthetic-02`: current and publishable — update ends verified (11-07), a copy ends with
//   «имя занято, возможно, недоступной вам панелью» (item 3).
// - The journal holds one unfinished create and one damaged file («Убрать в карантин», item 4).

import { createMemoryPublicationClient, type PublicationClient } from '../lib/publication/client';
import type { CheckResult, JournalEntry } from '../lib/publication/model';
import { DEMO_REPLACE_HOST } from './replace-host';

const at = (day: number, hour: number, minute = 0): number => Date.UTC(2026, 9, day, hour, minute);

const { preview, queuesPreview } = DEMO_REPLACE_HOST;

const conflict: CheckResult = {
  source: { state: 'conflict', checked_at: Number.NaN, base_fingerprint: 'demo-synthetic-base', remote_fingerprint: 'demo-synthetic-remote', invalidated: 'source_changed' },
  preview,
  remote: {
    missing: false,
    changes: [
      { kind: 'widget_changed', page_name: 'Обзор', page_number: 1, widget_name: 'Подключения к базе', widget_type: 'svggraph', widgetid: 'demo-synthetic-w1', overlaps: true },
      { kind: 'widget_added', page_name: 'Репликация', page_number: 2, widget_name: 'Новый график', widget_type: 'svggraph', widgetid: 'demo-synthetic-w10', overlaps: false },
      { kind: 'page_renamed', page_name: 'Карта и ссылки', page_number: 3, widget_name: null, widget_type: null, widgetid: null, overlaps: false }
    ],
    truncated: 0,
    overlapping_widgets: 1
  },
  mode: 'read_write',
  journal_block: null
};

const publishable: CheckResult = {
  source: { state: 'current', checked_at: Number.NaN, base_fingerprint: 'demo-synthetic-base', remote_fingerprint: 'demo-synthetic-base', invalidated: null },
  preview: {
    ...queuesPreview,
    changes: [
      {
        key: { page_index: 1, widget_index: 0, field: 'ds.0.hosts.0' },
        widgetid: 'demo-synthetic-70011',
        widget_type: 'svggraph',
        source: { kind: 'host_pattern', pattern: 'Demo host C' },
        target: { kind: 'host_pattern', pattern: 'Demo host E' }
      }
    ],
    checked_at: Number.NaN,
    report: { allows_publication: true, checks: [], findings: [] }
  },
  remote: null,
  mode: 'read_write',
  journal_block: null
};

const unfinished: JournalEntry = {
  operation_id: 'zds-create-demo-synthetic-0001',
  kind: 'create',
  intent: 'save_as_copy',
  connection_local_id: 'demo-synthetic-connection',
  server_version: '7.4.16',
  target: { kind: 'new_dashboard', dashboardid: null, name: 'Демо: базы данных — Пилот (копия 2)' },
  source: { local_id: 'demo-synthetic-01', revision: 4 },
  state: 'unknown_outcome',
  progress: 'needs_reconciliation',
  sends: 1,
  transitions: [
    { state: 'confirmed', at: at(27, 14, 40) },
    { state: 'submitting', at: at(27, 14, 40) },
    { state: 'unknown_outcome', at: at(27, 14, 41) }
  ],
  plan_created_at: at(27, 14, 39),
  confirmed_at: at(27, 14, 40),
  submitted_at: at(27, 14, 40),
  evidence: 'reconcile_read_failed',
  created_dashboardid: null,
  reported_dashboardid: null,
  closed_by: null,
  source_fingerprint: 'demo-synthetic-local',
  expected_fingerprint: 'demo-synthetic-content',
  observed_fingerprint: null
};

export function createDemoPublicationClient(): PublicationClient {
  return createMemoryPublicationClient({
    drafts: {
      'demo-synthetic-01': { check: conflict, outcome: { save_as_copy: 'unknown_outcome', update_source: 'conflict' } },
      'demo-synthetic-02': { check: publishable, outcome: { update_source: 'verified', save_as_copy: 'reconciled_failed' }, evidence: 'dashboard_name_taken' }
    },
    journal: {
      unfinished: [unfinished],
      damaged: [{ operation_id: 'zds-demo-synthetic-damaged-0001', code: 'journal_corrupt', connection_local_id: null, target: null }]
    }
  });
}
