import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import { resolveText } from '../components';
import { catalogs, createI18n, type MessageId } from '../i18n';
import { CSRF_HEADER, SESSION_ENDPOINT } from '../session';
import DraftsScreen from '../shell/DraftsScreen.svelte';
import { createMemoryDraftsClient, withConnectionNames, type DraftDetail } from '../shell/drafts';
import { parseRoute, serverCopyPath, serverPreviewPath, topSection } from '../shell/router';
import {
  createHttpWorkcopyClients,
  type ImportOutcome,
  type ImportPreview,
  type ServerClient,
  type ServerDashboardList,
  type ServerDashboardRow
} from './client';
import CopyScreen from './CopyScreen.svelte';
import { PARSE_ERROR_CODES, WORKCOPY_CODES, recoveryAction, workcopyCodeText } from './messages';
import {
  accessFilterOptions,
  copyDraftName,
  copyNaming,
  mineText,
  pageCount,
  previewStructure,
  searchProblem,
  typeBreakdown
} from './server';
import ServerPreviewScreen from './ServerPreviewScreen.svelte';
import ServerDashboardsScreen from './ServerDashboardsScreen.svelte';
import ServerGate from './ServerGate.svelte';
import { ServerDashboardsStore } from './store.svelte';

// Synthetic fixtures only: neutral names, no real infrastructure.
const en = createI18n('en');
const ru = createI18n('ru');
const CONNECTION = 'conn-0123456789abcdef0123456789abcdef';
const COPY_ID = 'wc-0123456789abcdef0123456789abcdef';

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

const wireSummary = {
  local_id: COPY_ID,
  kind: 'page',
  origin: 'server_copy',
  name: 'Queues',
  source: {
    dashboard_name: 'Synthetic DB',
    page_name: 'Queues',
    page_number: 2,
    connection_local_id: CONNECTION,
    server_version: '7.4.16',
    snapshot_at: '2026-10-26T07:33:20.123Z'
  },
  freshness: { state: 'current', checked_at: '2026-10-26T07:33:20Z' },
  local_state: 'saved',
  updated_at: '2026-10-26T08:00:00Z',
  counts: { pages: 1, widgets: 1 },
  created_dashboards: []
};

const wireDetail = {
  ...wireSummary,
  kind: 'dashboard',
  source: { ...wireSummary.source, page_name: null, page_number: null },
  pages: [
    {
      id: '5010',
      name: 'Overview',
      display_period: null,
      widgets: [{ id: '50100', name: 'Value', type: 'item', x: 0, y: 0, width: 12, height: 3, editable: true }]
    }
  ],
  properties: { display_period: 30, auto_start: true }
};

const row: ServerDashboardRow = {
  dashboardid: '501',
  name: 'Synthetic DB',
  owner: 'other',
  access: 'shared',
  page_count: 2,
  widget_count: 3,
  editable_widget_count: 2,
  existing_copies: [{ local_id: COPY_ID, kind: 'dashboard', snapshot_at: Date.UTC(2026, 9, 25, 14, 2) }]
};

const list: ServerDashboardList = {
  rows: [row, { ...row, dashboardid: '502', name: 'Mine only', owner: 'me', access: 'private', existing_copies: [] }],
  total: 2,
  counts: { all: 2, mine: 1, shared: 1 },
  offset: 0,
  limit: 50,
  truncated: true,
  empty_reason: null,
  existing_copies_known: true
};

const preview: ImportPreview = {
  dashboardid: '501',
  name: 'Synthetic DB',
  owner: 'other',
  access: 'shared',
  pages: [
    {
      dashboard_pageid: '5010',
      name: 'Overview',
      display_period: null,
      widget_count: 2,
      editable_widget_count: 2,
      widgets: [
        { id: '50100', name: 'Load', type: 'svggraph', x: 0, y: 0, width: 36, height: 5, editable: true },
        { id: '50101', name: '', type: 'svggraph', x: 36, y: 0, width: 36, height: 5, editable: true }
      ]
    },
    {
      dashboard_pageid: '5011',
      name: '',
      display_period: 120,
      widget_count: 1,
      editable_widget_count: 0,
      widgets: [{ id: '50110', name: 'Vendor', type: 'vendor.heatmap', x: 0, y: 0, width: 72, height: 8, editable: false }]
    }
  ],
  properties: { display_period: 30, auto_start: false },
  dependencies: { hosts: 1, host_patterns: 2, items: 18, item_patterns: 24, graphs: 2, widget_references: 6, unknown: 0 },
  unknown_widget_types: ['vendor.heatmap'],
  unknown_field_count: 4,
  existing_copies: []
};

const outcome: ImportOutcome = {
  local_id: COPY_ID,
  kind: 'dashboard',
  name: 'Synthetic DB',
  attempts: 1,
  snapshot_fingerprint: 'f'.repeat(64),
  snapshot_reused: false,
  item_metadata: 'complete'
};

interface Call {
  readonly path: string;
  readonly method: string;
  readonly csrf: string | null;
  readonly body: string | null;
}

function fakeServer(answer: (path: string, method: string) => Response | Promise<Response>) {
  const calls: Call[] = [];
  const fetcher: typeof fetch = async (input, init) => {
    const path = String(input);
    const headers = (init?.headers ?? {}) as Record<string, string>;
    const method = init?.method ?? 'GET';
    calls.push({ path, method, csrf: headers[CSRF_HEADER] ?? null, body: typeof init?.body === 'string' ? init.body : null });
    if (path === SESSION_ENDPOINT) return json({ csrf_token: 'csrf-token' });
    return answer(path, method);
  };
  return { calls, fetcher };
}

function fakeServerClient(overrides: Partial<ServerClient> = {}): ServerClient & { imports: unknown[] } {
  const imports: unknown[] = [];
  return {
    imports,
    list: async () => list,
    preview: async () => preview,
    importDashboard: async (_connection, _dashboard, scope, name) => {
      imports.push(name === undefined ? scope : { ...scope, name });
      return name === undefined ? outcome : { ...outcome, name };
    },
    ...overrides
  };
}

describe('workcopy HTTP client', () => {
  it('maps drafts with RFC 3339 times and keeps Zabbix IDs out of the model', async () => {
    const server = fakeServer((path) => {
      if (path === '/api/drafts') return json({ drafts: [wireSummary], damaged: [{ local_id: 'wc-x', code: 'workcopy_corrupt' }] });
      if (path === `/api/drafts/${COPY_ID}`) return json(wireDetail);
      return json({ code: 'workcopy_not_found', params: {} }, 404);
    });
    const { drafts } = createHttpWorkcopyClients(server.fetcher);
    const listed = await drafts.list();
    expect(listed.status).toBe('ready');
    if (listed.status !== 'ready') return;
    const [summary] = listed.drafts;
    expect(summary.updated_at).toBe(Date.UTC(2026, 9, 26, 8));
    expect(summary.source?.snapshot_at).toBe(Date.UTC(2026, 9, 26, 7, 33, 20, 123));
    expect(summary.source?.page_number).toBe(2);
    expect(summary.source?.connection_local_id).toBe(CONNECTION);
    expect(summary.freshness).toEqual({ state: 'current', checked_at: Date.UTC(2026, 9, 26, 7, 33, 20) });
    expect(listed.damaged).toEqual([{ local_id: 'wc-x', code: 'workcopy_corrupt' }]);

    const loaded = await drafts.load(COPY_ID);
    expect(loaded.status).toBe('ready');
    if (loaded.status !== 'ready') return;
    expect(loaded.draft.source).not.toHaveProperty('page_name');
    expect(loaded.draft.properties).toEqual({ display_period: 30, auto_start: true });
    expect(loaded.draft.pages[0].widgets[0].type).toBe('item');
    expect(await drafts.load('wc-gone')).toEqual({ status: 'missing' });
    expect(server.calls.every((call) => call.path.startsWith('/api/'))).toBe(true);
  });

  it('renames with the CSRF token and reports failures by code', async () => {
    const server = fakeServer((path, method) => {
      if (method === 'POST' && path.endsWith('/rename')) {
        return path.includes(COPY_ID) ? json({ ...wireSummary, name: 'Renamed' }) : json({ code: 'draft_name_invalid', params: { max: '255' } }, 422);
      }
      return json({ code: 'storage_unavailable', params: {} }, 500);
    });
    const { drafts } = createHttpWorkcopyClients(server.fetcher);
    const renamed = await drafts.rename(COPY_ID, 'Renamed');
    expect(renamed.status === 'ready' && renamed.draft.name).toBe('Renamed');
    const rename = server.calls.find((call) => call.path.endsWith('/rename'));
    expect(rename?.csrf).toBe('csrf-token');
    expect(rename?.body).toBe(JSON.stringify({ name: 'Renamed' }));
    expect(await drafts.rename('wc-other', '\u0007')).toEqual({
      status: 'failed',
      failure: { code: 'draft_name_invalid', params: { max: '255' }, status: 422 }
    });
    expect(await drafts.list()).toEqual({ status: 'failed', failure: { code: 'storage_unavailable', params: {}, status: 500 } });
  });

  it('treats a missing backend as unavailable, not as an empty list', async () => {
    const { drafts } = createHttpWorkcopyClients(async () => {
      throw new TypeError('network');
    });
    expect(await drafts.list()).toEqual({ status: 'unavailable' });
    expect(await drafts.load(COPY_ID)).toEqual({ status: 'unavailable' });
  });

  it('builds server queries, parses copy times and posts the import scope', async () => {
    const server = fakeServer((path, method) => {
      if (method === 'POST') return json(outcome, 201);
      if (path.includes('/dashboards/501')) return json({ ...preview, existing_copies: [{ local_id: COPY_ID, kind: 'page', snapshot_at: '2026-10-25T14:02:00Z' }] });
      return json({ ...list, rows: [{ ...row, existing_copies: [{ local_id: COPY_ID, kind: 'dashboard', snapshot_at: '2026-10-25T14:02:00Z' }] }] });
    });
    const { server: client } = createHttpWorkcopyClients(server.fetcher);
    const listed = await client.list(CONNECTION, { search: '  db ', access: 'shared', offset: 50, limit: 50 });
    expect(server.calls[0].path).toBe(`/api/server/${CONNECTION}/dashboards?search=db&access=shared&offset=50&limit=50`);
    expect(listed.rows[0].existing_copies[0].snapshot_at).toBe(Date.UTC(2026, 9, 25, 14, 2));
    const previewed = await client.preview(CONNECTION, '501');
    expect(previewed.existing_copies[0].kind).toBe('page');
    expect(await client.importDashboard(CONNECTION, '501', { scope: 'page', dashboard_pageid: '5011' })).toEqual(outcome);
    const post = server.calls.find((call) => call.method === 'POST' && call.path.endsWith('/import'));
    expect(post?.path).toBe(`/api/server/${CONNECTION}/dashboards/501/import`);
    expect(post?.csrf).toBe('csrf-token');
    expect(post?.body).toBe(JSON.stringify({ scope: 'page', dashboard_pageid: '5011' }));
    // P2-08A: a chosen name travels in the import body.
    await client.importDashboard(CONNECTION, '501', { scope: 'dashboard' }, 'Synthetic DB (copy 2)');
    const named = server.calls.filter((call) => call.method === 'POST' && call.path.endsWith('/import')).at(-1);
    expect(named?.body).toBe(JSON.stringify({ scope: 'dashboard', name: 'Synthetic DB (copy 2)' }));
  });
});

describe('workcopy messages', () => {
  it('maps every backend code to its own message ID', () => {
    const backendCodes = [
      'connection_id_invalid', 'dashboard_id_invalid', 'draft_name_invalid', 'import_inconsistent', 'import_page_not_found',
      'list_limit_invalid', 'not_visible_or_denied', 'page_id_invalid', 'random_source_unavailable', 'search_invalid',
      'snapshot_corrupt', 'snapshot_missing', 'snapshot_shape_invalid', 'snapshot_too_large', 'snapshot_unsupported_adapter',
      'storage_file_too_large', 'storage_limit_reached', 'storage_linked_path', 'storage_unavailable', 'storage_write_failed',
      'workcopy_corrupt', 'workcopy_id_invalid', 'workcopy_not_found', 'workcopy_too_large', 'workcopy_unsupported_operation',
      'settings_unavailable', 'not_connected', 'connections_unavailable', 'request_invalid', 'session_expired',
      'api_method_denied', 'host_unreachable_or_timeout'
    ];
    for (const code of backendCodes) {
      const text = workcopyCodeText(code, { max: '255', method: 'item.get' });
      expect(text.id, code).not.toBe('errors.connection.unknown');
      expect(Object.prototype.hasOwnProperty.call(catalogs.en, text.id)).toBe(true);
      expect(Object.prototype.hasOwnProperty.call(catalogs.ru, text.id)).toBe(true);
    }
    expect(WORKCOPY_CODES).toContain('import_inconsistent');
    expect(en.t('errors.import.inconsistent')).toBe('The dashboard changed while it was being read. No copy was created; try again.');
    expect(ru.t('errors.import.inconsistent')).toBe('Панель изменилась во время чтения. Копия не создана; повторите.');
    expect(workcopyCodeText('vendor_future_code')).toEqual({ id: 'errors.connection.unknown', params: { code: 'vendor_future_code' } });
  });

  it('maps strict YAML and JSON reader codes to import messages', () => {
    for (const code of PARSE_ERROR_CODES) {
      const text = workcopyCodeText(code);
      expect(text.id, code).toMatch(/^errors\.import\./);
      expect(resolveText(en, text), code).not.toContain('{');
      expect(resolveText(ru, text), code).not.toContain('{');
    }
    expect(resolveText(ru, workcopyCodeText('yaml_alias'))).toBe('Файл содержит структуру, запрещённую для безопасного импорта (yaml_alias).');
    expect(resolveText(en, workcopyCodeText('json_invalid'))).toBe('The JSON file could not be parsed.');
    expect(workcopyCodeText('yaml_duplicate_key')).toEqual({ id: 'errors.import.validation', params: { code: 'yaml_duplicate_key' } });
  });

  it('offers sign-in, connect or retry after a failed read', () => {
    expect(recoveryAction('session_expired')).toBe('sign_in');
    expect(recoveryAction('not_connected')).toBe('connect');
    expect(recoveryAction('host_unreachable_or_timeout')).toBe('retry');
  });
});

describe('server screen logic', () => {
  it('formats owner, copies, filters and the widget breakdown', () => {
    expect(mineText(ru, row.existing_copies, true)).toMatch(/^копия · 25\.10/);
    expect(mineText(ru, [], true)).toBe('—');
    expect(mineText(en, row.existing_copies, false)).toBe('unknown');
    const options = accessFilterOptions(ru, list);
    expect(options.map((option) => ru.t(option.label.id as MessageId & 'common.filter.with_count', option.label.params as never))).toEqual([
      'Все 2',
      'Мои 1',
      'Общие 1'
    ]);
    expect(accessFilterOptions(en, null)[1].label).toEqual({ id: 'server.filter.mine' });
    const breakdown = typeBreakdown(ru, preview.pages);
    expect(breakdown.editable).toBe(2);
    expect(breakdown.other).toBe(1);
    expect(breakdown.editableTypes).toEqual([{ label: 'График', count: 2 }]);
    expect(breakdown.otherTypes).toEqual([{ label: 'vendor.heatmap', count: 1 }]);
    expect(pageCount({ total: 101, limit: 50 })).toBe(3);
    expect(pageCount({ total: 0, limit: 50 })).toBe(1);
    expect(searchProblem('x'.repeat(256))).toBe('too_long');
    expect(searchProblem('a\u0007')).toBe('control');
    expect(searchProblem('  Базы  ')).toBeNull();
  });

  it('routes the copy check without a Zabbix ID in the address', () => {
    expect(serverCopyPath(CONNECTION)).toBe(`/server/${CONNECTION}/copy`);
    const parsed = parseRoute(`/server/${CONNECTION}/copy`);
    expect(parsed.route).toEqual({ name: 'server_copy', connectionLocalId: CONNECTION });
    expect(topSection(parsed.route)).toBe('server');
    expect(parseRoute(`/server/${CONNECTION}/copy/501`).canonical).toBe('/server');
  });

  it('routes the read-only preview without a Zabbix ID in the address (P2-05E)', () => {
    expect(serverPreviewPath(CONNECTION)).toBe(`/server/${CONNECTION}/preview`);
    const parsed = parseRoute(`/server/${CONNECTION}/preview`);
    expect(parsed.route).toEqual({ name: 'server_preview', connectionLocalId: CONNECTION });
    expect(parsed.canonical).toBeUndefined();
    expect(topSection(parsed.route)).toBe('server');
    expect(parseRoute(`/server/${CONNECTION}/preview/501`).canonical).toBe('/server');
    expect(parseRoute(`/server/${CONNECTION}/dashboards/501`).canonical).toBe('/server');
  });

  it('names another copy of the same source «… (copy N)» with the next free number (P2-08A)', () => {
    const naming = copyNaming(en, 'Synthetic DB');
    expect(copyDraftName(naming, false, ['Synthetic DB'])).toBeUndefined();
    expect(copyDraftName(naming, true, ['Synthetic DB'])).toBe('Synthetic DB (copy 2)');
    expect(copyDraftName(naming, true, ['Synthetic DB', 'Synthetic DB (copy 2)', 'Synthetic DB (copy 4)'])).toBe('Synthetic DB (copy 3)');
    expect(copyDraftName(copyNaming(ru, 'Базы'), true, ['Базы', 'Базы (копия 2)'])).toBe('Базы (копия 3)');
    // The source name is shortened so that the name stays within the local limit.
    const long = copyDraftName(copyNaming(en, 'x'.repeat(255)), true, [], 255);
    expect(long).toBe(`${'x'.repeat(246)} (copy 2)`);
    expect([...(long ?? '')]).toHaveLength(255);
  });

  it('maps the preview to the «Structure» view of a dashboard (P2-05E)', () => {
    const structure = previewStructure(preview);
    expect(structure.kind).toBe('dashboard');
    expect(structure.properties).toEqual({ display_period: 30, auto_start: false });
    expect(structure.pages.map((page) => [page.id, page.display_period, page.widgets.length])).toEqual([
      ['5010', null, 2],
      ['5011', 120, 1]
    ]);
    expect(structure.pages[0].widgets[1]).toEqual({ id: '50101', name: '', type: 'svggraph', x: 36, y: 0, width: 36, height: 5, editable: true });
  });

  it('names connections of drafts from the profile list', () => {
    const draft: DraftDetail = { ...(wireDetail as unknown as DraftDetail), source: { dashboard_name: 'X', connection_local_id: CONNECTION, snapshot_at: 0 } };
    expect(withConnectionNames(draft, (id) => (id === CONNECTION ? 'Lab' : undefined)).source?.connection_name).toBe('Lab');
    expect(withConnectionNames(draft, () => undefined).source?.connection_name).toBeUndefined();
  });
});

describe('server dashboards store', () => {
  it('lists, selects with a preview and imports one copy at a time', async () => {
    const client = fakeServerClient();
    const store = new ServerDashboardsStore(client);
    store.open(CONNECTION);
    await store.load();
    expect(store.list?.rows).toHaveLength(2);
    await store.select(row);
    expect(store.preview?.pages).toHaveLength(2);
    const [first, second] = await Promise.all([store.importCopy({ scope: 'dashboard' }), store.importCopy({ scope: 'dashboard' })]);
    expect(first).toEqual(outcome);
    expect(second).toBeNull();
    expect(client.imports).toEqual([{ scope: 'dashboard' }]);
    // The list now misses the new copy and is read again when shown.
    expect(store.list).toBeNull();
  });

  it('does not show the last list as current after a failed read', async () => {
    let fail = false;
    const client = fakeServerClient({
      list: async () => {
        if (fail) throw new (await import('../api')).ApiFailure(502, { code: 'session_expired', params: {} });
        return list;
      }
    });
    const store = new ServerDashboardsStore(client);
    store.open(CONNECTION);
    await store.load();
    fail = true;
    await store.load();
    expect(store.list).toBeNull();
    expect(store.listFailure?.code).toBe('session_expired');
    store.open('conn-ffffffffffffffffffffffffffffffff');
    expect(store.listFailure).toBeNull();
    expect(store.selectedId).toBeNull();
    await store.load().catch(() => undefined);
    fail = false;
    store.open(CONNECTION, 1);
    await store.load();
    expect(store.list).not.toBeNull();
    // A reconnect (new context generation) never shows the previous list as current.
    store.open(CONNECTION, 2);
    expect(store.list).toBeNull();
    store.close();
    expect(store.connectionLocalId).toBeNull();
  });

  it('names a new copy of a dashboard that already has a local copy (P2-08A)', async () => {
    const client = fakeServerClient();
    const existing: DraftDetail = {
      ...(wireDetail as unknown as DraftDetail),
      local_id: COPY_ID,
      kind: 'dashboard',
      name: 'Synthetic DB',
      source: { dashboard_name: 'Synthetic DB', connection_local_id: CONNECTION, snapshot_at: 0 }
    };
    const other: DraftDetail = { ...existing, local_id: 'wc-other', name: 'Synthetic DB (copy 2)' };
    const store = new ServerDashboardsStore(client, createMemoryDraftsClient([existing, other]));
    store.open(CONNECTION);
    await store.select(row);
    const named = await store.importCopy({ scope: 'dashboard' }, copyNaming(en, 'Synthetic DB'));
    expect(named?.name).toBe('Synthetic DB (copy 3)');
    // A page of this dashboard has no page copy yet: the source name is kept.
    await store.importCopy({ scope: 'page', dashboard_pageid: '5010' }, copyNaming(en, 'Overview', 1));
    expect(client.imports).toEqual([
      { scope: 'dashboard', name: 'Synthetic DB (copy 3)' },
      { scope: 'page', dashboard_pageid: '5010' }
    ]);
    // Without a draft list the backend default name is kept.
    const plain = new ServerDashboardsStore(fakeServerClient());
    plain.open(CONNECTION);
    await plain.select(row);
    expect((await plain.importCopy({ scope: 'dashboard' }, copyNaming(en, 'Synthetic DB')))?.name).toBe('Synthetic DB');
  });

  it('keeps nothing when the import fails', async () => {
    const client = fakeServerClient({
      importDashboard: async () => {
        throw new (await import('../api')).ApiFailure(409, { code: 'import_inconsistent', params: { attempts: '2' } });
      }
    });
    const store = new ServerDashboardsStore(client);
    store.open(CONNECTION);
    await store.select(row);
    expect(await store.importCopy({ scope: 'dashboard' })).toBeNull();
    expect(store.importFailure?.code).toBe('import_inconsistent');
    expect(store.importing).toBeNull();
  });
});

function loadedStore(withPreview: boolean): ServerDashboardsStore {
  const store = new ServerDashboardsStore(fakeServerClient());
  store.open(CONNECTION);
  store.list = list;
  store.selectedId = row.dashboardid;
  store.selectedRow = row;
  if (withPreview) store.preview = preview;
  return store;
}

describe('server screens', () => {
  it('renders the dashboard table, filters and the details of the selected dashboard (03-01)', () => {
    const { body } = render(ServerDashboardsScreen, {
      props: {
        i18n: ru,
        store: loadedStore(true),
        profileName: 'Лаборатория',
        serverVersion: '7.4.16',
        modeLabel: { id: 'connection.mode.read_only' },
        onimported: () => undefined
      }
    });
    for (const text of [
      'Панели в Zabbix',
      'Лаборатория · Zabbix 7.4.16 · только чтение. Сервер не изменяется на этом экране.',
      'Панели',
      'Шаблоны',
      'Все 2',
      'Мои 1',
      'Общие 1',
      'Поиск по имени панели',
      'Владелец',
      'Доступ',
      'Страниц',
      'Виджетов',
      'Изменение',
      'У меня',
      'другой пользователь',
      'вы',
      'Общая',
      'Личная',
      '2 из 3',
      'Владелец — другой пользователь · Общая · 2 страницы, 3 виджета',
      'Изменение поддерживается: 2',
      'Изменение не поддерживается: 1',
      'Скопировать с сервера',
      'Взять страницу',
      'Клонировать',
      'Заменить узел сети',
      'Появится позже',
      'Открыть копию',
      'Каждое действие начинается с копии',
      'Zabbix эти случаи не различает',
      'Прочитаны только первые 2 панели.'
    ]) {
      expect(body, text).toContain(text);
    }
    expect(body).toContain('role="tablist"');
    expect(body).toContain('role="tabpanel"');
    expect(body).toMatch(/<button id="server-row-501"[^>]*aria-pressed="true"/);
    expect(body).toContain(`href="/draft/${COPY_ID}/overview"`);
    expect(body).toContain(`href="/server/${CONNECTION}/copy"`);
    expect(body).toContain('type="radio"');
    // Zabbix IDs never become visible text.
    expect(body).not.toMatch(/>\s*501\s*</);
  });

  it('says why the list is empty and offers sign-in after a lost session', () => {
    const empty = loadedStore(false);
    empty.list = { ...list, rows: [], total: 0, counts: { all: 0, mine: 0, shared: 0 }, truncated: false, empty_reason: 'not_visible_or_denied' };
    const emptyBody = render(ServerDashboardsScreen, {
      props: { i18n: ru, store: empty, profileName: 'Лаборатория', serverVersion: null, modeLabel: { id: 'connection.mode.read_only' }, onimported: () => undefined }
    }).body;
    expect(emptyBody).toContain('Панели не найдены либо учётная запись не может их просматривать');
    expect(emptyBody).not.toContain('<table');

    const failed = loadedStore(false);
    failed.list = null;
    failed.listFailure = { code: 'session_expired', params: {}, status: 502 };
    const failedBody = render(ServerDashboardsScreen, {
      props: { i18n: en, store: failed, profileName: 'Lab', serverVersion: null, modeLabel: { id: 'connection.mode.read_only' }, onimported: () => undefined }
    }).body;
    expect(failedBody).toContain('Could not load the dashboard list');
    expect(failedBody).toContain('The Zabbix session has expired. Sign in again.');
    expect(failedBody).toContain('Sign in');
    expect(failedBody).not.toContain('<table');
  });

  it('shows the copy check with composition, dependencies and what is saved (03-03)', () => {
    const store = loadedStore(true);
    store.selectedRow = { ...row, existing_copies: [] };
    const { body } = render(CopyScreen, {
      props: { i18n: ru, store, connectionLocalId: CONNECTION, onimported: () => undefined }
    });
    for (const text of [
      'Скопировать «Synthetic DB» с сервера',
      'Вся панель: 2 страницы, 3 виджета. Одну страницу берёт действие «Взять страницу».',
      'Состав',
      '1. Overview',
      '2 виджета: изменение поддерживается — 2, нет — 0',
      'копируется как есть',
      'Зависимости',
      '3 ссылки: 1 по ID, 2 по шаблону имени',
      '42 ссылки: 18 по ID, 24 по шаблону имени',
      '6 — сохраняются вместе с виджетами',
      '4 — сохраняются как есть, не меняются',
      'Что сохранится на этом компьютере',
      'Не сохраняется',
      'Панель читается дважды.',
      'Отмена',
      'Скопировать с сервера',
      'Копирование'
    ]) {
      expect(body, text).toContain(text);
    }
    expect(body).not.toContain('Открыть мою копию');
  });

  it('makes «Open my copy» the primary action when a copy exists', () => {
    const store = loadedStore(true);
    store.preview = { ...preview, existing_copies: row.existing_copies };
    const { body } = render(CopyScreen, { props: { i18n: ru, store, connectionLocalId: CONNECTION, onimported: () => undefined } });
    expect(body).toContain('У вас уже есть копия этой панели от');
    expect(body).toMatch(new RegExp(`class="button-link primary" href="/draft/${COPY_ID}/overview">Открыть мою копию`));
    expect(body).toContain('Новая копия');
  });

  it('opens the read-only preview from the dashboard details (P2-05E)', () => {
    const { body } = render(ServerDashboardsScreen, {
      props: {
        i18n: en,
        store: loadedStore(true),
        profileName: 'Lab',
        serverVersion: '7.4.16',
        modeLabel: { id: 'connection.mode.read_only' },
        onimported: () => undefined
      }
    });
    expect(body).toMatch(new RegExp(`<a class="button-link" href="/server/${CONNECTION}/preview"[^>]*>Preview</a>`));
  });

  it('previews a server dashboard read only in the «Structure» view (P2-05E)', () => {
    const store = loadedStore(true);
    store.preview = { ...preview, existing_copies: row.existing_copies };
    const { body } = render(ServerPreviewScreen, { props: { i18n: ru, store, connectionLocalId: CONNECTION, onimported: () => undefined } });
    for (const text of [
      'Панели в Zabbix',
      'Предпросмотр',
      'Synthetic DB',
      'Только просмотр',
      'на этом компьютере ничего не сохраняется',
      'Структура',
      '1. Overview',
      'Load',
      'Предпросмотр страницы',
      'Скопировать с сервера',
      'Открыть копию',
      'Страница, которую взять',
      'Взять страницу'
    ]) {
      expect(body, text).toContain(text);
    }
    expect(body).toContain(`href="/server/${CONNECTION}/copy"`);
    expect(body).toContain(`href="/draft/${COPY_ID}/overview"`);
    // Read only: no structure or editor actions, not even unavailable ones.
    expect(body).not.toContain('Добавить страницу');
    expect(body).not.toContain('structure-page-actions');
    const loading = loadedStore(false);
    expect(render(ServerPreviewScreen, { props: { i18n: en, store: loading, connectionLocalId: CONNECTION, onimported: () => undefined } }).body).toContain(
      'Loading…'
    );
  });

  it('shows the offline and other-connection states with a way to connect', () => {
    const offline = render(ServerGate, { props: { i18n: ru, state: 'offline' } }).body;
    expect(offline).toContain('Подключитесь к Zabbix, чтобы увидеть панели');
    expect(offline).toContain('href="/connections"');
    const other = render(ServerGate, { props: { i18n: en, state: 'other_connection' } }).body;
    expect(other).toContain('This connection is not active.');
  });
});

describe('drafts screen with storage data', () => {
  it('reports damaged drafts and offers renaming', async () => {
    const client = createMemoryDraftsClient([]);
    const { body } = render(DraftsScreen, {
      props: {
        i18n: ru,
        drafts: { status: 'ready', drafts: [], damaged: [{ local_id: 'wc-x', code: 'workcopy_corrupt' }] },
        onrename: client.rename
      }
    });
    expect(body).toContain('1 черновик не удалось прочитать; файлы не изменялись.');
  });

  it('renames in memory with the backend rules', async () => {
    const draft = { ...(wireDetail as unknown as DraftDetail), local_id: COPY_ID, updated_at: 0 };
    const client = createMemoryDraftsClient([draft]);
    const renamed = await client.rename(COPY_ID, '  New name ');
    expect(renamed.status === 'ready' && renamed.draft.name).toBe('New name');
    expect(await client.rename(COPY_ID, '   ')).toEqual({ status: 'failed', failure: { code: 'draft_name_invalid', params: { max: '255' } } });
    const listed = await client.list();
    expect(listed.status === 'ready' && listed.drafts[0].name).toBe('New name');
  });
});
