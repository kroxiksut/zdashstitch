// Synthetic drafts for manual accessibility checks of the DEMO build only (`npm run build:demo`).
// Every name, profile and host below is invented; there is no real infrastructure here. This module
// is imported only behind `import.meta.env.MODE === 'demo'` in `main.ts`, so the normal build drops
// it; `scripts/check-dist.mjs` fails the normal build if the `demo-synthetic-` IDs ever leak into it.

import { createMemoryDraftsClient, type DraftDetail, type DraftsClient } from '../lib/shell/drafts';

const at = (day: number, hour: number, minute = 0): number => Date.UTC(2026, 9, day, hour, minute);

const databases: DraftDetail = {
  local_id: 'demo-synthetic-01',
  kind: 'dashboard',
  origin: 'server_copy',
  name: 'Демо: базы данных — Пилот',
  source: {
    dashboard_name: 'Демо: базы данных',
    connection_name: 'Демо-лаборатория',
    server_version: '7.4.16',
    snapshot_at: at(24, 9, 15)
  },
  freshness: { state: 'conflict', changed_at: at(25, 11, 31) },
  local_state: 'dirty',
  updated_at: at(26, 8, 40),
  counts: { pages: 3, widgets: 9 },
  created_dashboards: [
    { name: 'Демо: базы данных (копия)', created_at: at(20, 15, 20), connection_name: 'Демо-лаборатория', state: 'verified' },
    { name: 'Демо: старая копия', created_at: at(12, 9, 0), connection_name: 'Демо-лаборатория', state: 'not_found' }
  ],
  properties: { display_period: 30, auto_start: true },
  target: { dashboard_name: 'Демо: базы данных' },
  compatibility: { min_version: '7.4.0' },
  pages: [
    {
      id: 'p1',
      name: 'Обзор',
      display_period: null,
      widgets: [
        { id: 'w1', name: 'Подключения к базе', type: 'svggraph', x: 0, y: 0, width: 36, height: 5, editable: true, host_name: 'Демо-узел А' },
        { id: 'w2', name: 'Проблемы', type: 'problems', x: 36, y: 0, width: 36, height: 5, editable: true },
        { id: 'w3', name: '', type: 'clock', x: 0, y: 5, width: 12, height: 3, editable: true },
        { id: 'w4', name: 'Самые активные запросы', type: 'tophosts', x: 12, y: 5, width: 60, height: 6, editable: false }
      ]
    },
    {
      id: 'p2',
      name: 'Репликация',
      display_period: 120,
      widgets: [
        { id: 'w5', name: 'Отставание реплики', type: 'svggraph', x: 0, y: 0, width: 72, height: 6, editable: true, host_name: 'Демо-узел Б' },
        { id: 'w6', name: 'Синтетический виджет стороннего модуля', type: 'demo_vendor.unknown', x: 0, y: 6, width: 24, height: 4, editable: false },
        { id: 'w7', name: 'Значение', type: 'item', x: 24, y: 6, width: 12, height: 3, editable: true }
      ]
    },
    {
      id: 'p3',
      name: '',
      display_period: 3600,
      widgets: [
        { id: 'w8', name: 'Карта', type: 'geomap', x: 0, y: 0, width: 48, height: 8, editable: false },
        { id: 'w9', name: 'Ссылка', type: 'url', x: 48, y: 0, width: 24, height: 2, editable: true }
      ]
    }
  ],
  checks: {
    checked_objects: 12,
    passed: { structure: 3, widgets: 6, sources: 2, references: 0, compatibility: 0 },
    findings: [
      {
        id: 'f1',
        group: 'compatibility',
        level: 'blocking',
        blocks: ['export', 'publication'],
        message: { id: 'compatibility.unknown_widget_type', params: { widget_type: 'demo_vendor.unknown' } },
        page: 'Репликация',
        widget: 'Синтетический виджет стороннего модуля'
      },
      {
        id: 'f2',
        group: 'compatibility',
        level: 'warning',
        blocks: ['publication'],
        message: { id: 'compatibility.unknown_widget_type', params: { widget_type: 'tophosts' } },
        page: 'Обзор',
        widget: 'Самые активные запросы'
      }
    ]
  }
};

const queues: DraftDetail = {
  local_id: 'demo-synthetic-02',
  kind: 'page',
  origin: 'server_copy',
  name: 'Demo: queue overview',
  source: {
    dashboard_name: 'Demo: message broker',
    page_name: 'Queues',
    page_number: 2,
    connection_name: 'Demo lab',
    server_version: '7.4.16',
    snapshot_at: at(23, 16, 5)
  },
  freshness: { state: 'stale' },
  local_state: 'saved',
  updated_at: at(26, 7, 55),
  counts: { pages: 1, widgets: 3 },
  pages: [
    {
      id: 'p1',
      name: 'Queues',
      display_period: 60,
      widgets: [
        { id: 'w1', name: 'Throughput', type: 'svggraph', x: 0, y: 0, width: 36, height: 5, editable: true },
        { id: 'w2', name: 'Top queues', type: 'topitems', x: 36, y: 0, width: 36, height: 5, editable: true },
        { id: 'w3', name: 'Consumers', type: 'honeycomb', x: 0, y: 5, width: 72, height: 4, editable: false, host_name: 'Demo host C' }
      ]
    }
  ],
  compatibility: { min_version: '7.4.0' },
  checks: {
    checked_objects: 4,
    passed: { structure: 1, widgets: 3, sources: 0, references: 0, compatibility: 0 },
    findings: []
  }
};

const assembled: DraftDetail = {
  local_id: 'demo-synthetic-03',
  kind: 'dashboard',
  origin: 'local',
  name: 'Демонстрационная сводная панель для проверки переноса очень длинных названий черновиков в списке, карточке и заголовке рабочей области',
  local_state: 'saved',
  updated_at: at(25, 18, 10),
  provenance: {
    kind: 'assembled',
    page_count: 4,
    sources: ['Демо: базы данных', 'Demo: message broker', 'файл коллеги (демо)']
  },
  counts: { pages: 4, widgets: 5 },
  properties: { display_period: 60, auto_start: false },
  pages: [
    {
      id: 'p1',
      name: 'Страница с очень длинным названием, которое должно переноситься и не ломать структуру',
      display_period: 10,
      widgets: [
        { id: 'w1', name: 'График с длинным названием для проверки переноса строк при увеличении до 400 %', type: 'graph', x: 0, y: 0, width: 72, height: 4, editable: true }
      ]
    },
    {
      id: 'p2',
      name: 'Pilot',
      display_period: null,
      widgets: [
        { id: 'w2', name: 'Hosts', type: 'hostavail', x: 0, y: 0, width: 24, height: 3, editable: true },
        { id: 'w3', name: 'Problems by severity', type: 'problemsbysv', x: 24, y: 0, width: 48, height: 3, editable: true }
      ]
    },
    { id: 'p3', name: 'Пустая страница', display_period: null, widgets: [] },
    {
      id: 'p4',
      name: 'Main',
      display_period: 300,
      widgets: [
        { id: 'w4', name: 'Map', type: 'map', x: 0, y: 0, width: 36, height: 8, editable: false },
        { id: 'w5', name: 'Navigation', type: 'navtree', x: 36, y: 0, width: 36, height: 8, editable: false }
      ]
    }
  ]
};

const imported: DraftDetail = {
  local_id: 'demo-synthetic-04',
  kind: 'page',
  origin: 'local',
  name: 'Demo: imported page with a deliberately long English name to check wrapping at high zoom levels',
  local_state: 'dirty',
  updated_at: at(25, 12, 0),
  provenance: { kind: 'imported_file' },
  counts: { pages: 1, widgets: 0 },
  pages: [{ id: 'p1', name: 'Imported', display_period: null, widgets: [] }]
};

const published: DraftDetail = {
  local_id: 'demo-synthetic-05',
  kind: 'dashboard',
  origin: 'server_copy',
  name: 'Demo: web checks',
  source: {
    dashboard_name: 'Demo: web checks',
    connection_name: 'Demo lab',
    server_version: '7.4.16',
    snapshot_at: at(22, 10, 0)
  },
  freshness: { state: 'current', checked_at: at(26, 9, 0) },
  local_state: 'saved',
  updated_at: at(24, 14, 30),
  last_published_at: at(24, 14, 35),
  counts: { pages: 2, widgets: 2 },
  properties: { display_period: 30, auto_start: false },
  pages: [
    {
      id: 'p1',
      name: 'Scenarios',
      display_period: null,
      widgets: [{ id: 'w1', name: 'Web monitoring', type: 'web', x: 0, y: 0, width: 36, height: 4, editable: true }]
    },
    {
      id: 'p2',
      name: 'Availability',
      display_period: null,
      widgets: [{ id: 'w2', name: 'SLA', type: 'slareport', x: 0, y: 0, width: 72, height: 6, editable: true }]
    }
  ]
};

const created: DraftDetail = {
  local_id: 'demo-synthetic-06',
  kind: 'dashboard',
  origin: 'local',
  name: 'Демо: новая панель',
  local_state: 'dirty',
  updated_at: at(21, 9, 45),
  provenance: { kind: 'created_here' },
  counts: { pages: 2, widgets: 0 },
  properties: { display_period: 30, auto_start: true },
  pages: [
    { id: 'p1', name: '', display_period: null, widgets: [] },
    { id: 'p2', name: '', display_period: 30, widgets: [] }
  ]
};

const missingSource: DraftDetail = {
  local_id: 'demo-synthetic-07',
  kind: 'page',
  origin: 'server_copy',
  name: 'Демо: удалённая на сервере страница',
  source: { dashboard_name: 'Демо: архив', page_name: 'Старое', page_number: 5, snapshot_at: at(10, 8, 0) },
  freshness: { state: 'source_missing' },
  local_state: 'saved',
  updated_at: at(19, 17, 0),
  pages: [
    {
      id: 'p1',
      name: 'Старое',
      display_period: null,
      widgets: [{ id: 'w1', name: 'Журнал действий', type: 'actionlog', x: 0, y: 0, width: 72, height: 5, editable: true }]
    }
  ]
};

const legacy: DraftDetail = {
  local_id: 'demo-synthetic-08',
  kind: 'dashboard',
  origin: 'local',
  name: 'Demo: legacy file',
  local_state: 'saved',
  updated_at: at(15, 11, 0),
  provenance: { kind: 'legacy_file' },
  pages: [
    {
      id: 'p1',
      name: 'Legacy',
      display_period: null,
      widgets: [{ id: 'w1', name: 'System information', type: 'systeminfo', x: 0, y: 0, width: 36, height: 5, editable: true }]
    }
  ]
};

const pageFile: DraftDetail = {
  local_id: 'demo-synthetic-09',
  kind: 'page',
  origin: 'local',
  name: 'Демо: страница из файла',
  local_state: 'saved',
  updated_at: at(14, 10, 0),
  provenance: { kind: 'page_file' },
  pages: [
    {
      id: 'p1',
      name: 'Из файла',
      display_period: null,
      widgets: [{ id: 'w1', name: 'Неподдерживаемый тип', type: 'demo_vendor.another', x: 0, y: 0, width: 24, height: 4, editable: false }]
    }
  ]
};

/** All synthetic demo drafts; `local_id`s share the `demo-synthetic-` prefix checked in `dist`. */
export const DEMO_DRAFTS: ReadonlyArray<DraftDetail> = [
  databases,
  queues,
  assembled,
  imported,
  published,
  created,
  missingSource,
  legacy,
  pageFile
];

export function createDemoDraftsClient(): DraftsClient {
  return createMemoryDraftsClient(DEMO_DRAFTS);
}
