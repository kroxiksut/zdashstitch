import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { createI18n } from '../i18n';
import { CSRF_HEADER } from '../session';
import OfflinePageBuilder from './OfflinePageBuilder.svelte';
import { createCatalogClient, type NewOfflinePage, type OfflinePage, type PageRole } from './client';
import { pageRequestFromSaved, savedCatalogChoices, saveKnownPage } from './offline-edit';
import { createHttpWorkcopyClients } from '../workcopy/client';
import DraftsScreen from '../shell/DraftsScreen.svelte';
import { singleSource, validNewRole, validSource } from './offline-roles';
import { compositionFindings, graphRowCount } from './editor-composition';

const roles: Record<string, PageRole> = {
  primary: { cardinality: 'one', slots: ['primary-01'] },
  workers: { cardinality: 'many', slots: ['worker-b', 'worker-a'] }
};

const seriesColor = '#' + (0x102030).toString(16);
const savedPage: OfflinePage = {
  local_id: 'pg-synthetic', created_at_ms: 1, updated_at_ms: 2, revision: 3,
  catalog_versions: [{ catalog_id: 'synthetic', version: 2 }],
  widget_evaluation: { profile: '7.4', unrecognized: [] },
  bundle: {
    project: {
      kind: 'page', catalogs: [{ id: 'synthetic', file: 'metrics/synthetic.yaml' }], roles,
      root: { id: 'stable-page', name: 'Saved synthetic page', widgets: [
        { id: 'stable-graph', type: 'graph', title: 'Graph', expansion: 'series_per_host', series: [{ metric: { catalog: 'synthetic', metric: 'rate' }, hosts: { role: 'workers', selection: 'slot', slot_id: 'worker-a' }, color: seriesColor }], layout: { mode: 'fixed', x: 5, y: 7, w: 30, h: 9 } },
        { id: 'stable-item', type: 'item_value', title: 'Repeated', expansion: 'widget_per_host', metric: { catalog: 'synthetic', metric: 'rate' }, hosts: { role: 'workers', selection: 'all' }, layout: { mode: 'auto', w: 12, h: 5 } }
      ] }
    },
    catalogs: { synthetic: { catalog_id: 'synthetic', source: { template_name: 'Pinned catalog', export_format_version: '7.4' }, metrics: [{ id: 'rate', name: 'Rate', value_type: 'numeric_float', units: '', origin: 'template_item' }] } }
  },
  composition: { slots_declared: true, instance_count: 3, source_count: 3, truncated: false, instances: [] }
};

describe('offline page source choices', () => {
  it('takes a library page through the protected local API', async () => {
    const calls: Array<{ path: string; method: string; csrf?: string }> = [];
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input);
      calls.push({ path, method: init?.method ?? 'GET', csrf: new Headers(init?.headers).get(CSRF_HEADER) ?? undefined });
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      return Response.json(savedPage);
    };
    const taken = await createCatalogClient(fetcher).takeLibraryPage('lp-synthetic');
    expect(taken.local_id).toBe('pg-synthetic');
    expect(calls.some((call) => call.path === '/api/library/pages/lp-synthetic/take' && call.method === 'POST' && call.csrf === 'test-csrf')).toBe(true);
  });
  it('keeps many slots as rows of one graph and updates counts and diagnostics when slots change', () => {
    const graph = { kind: 'graph' as const, widget_id: 'cpu-memory', title: 'CPU and memory', metrics: [
      { catalog_id: 'synthetic', metric_id: 'cpu', hosts: { role: 'workers', selection: 'all' as const } },
      { catalog_id: 'synthetic', metric_id: 'memory', hosts: { role: 'workers', selection: 'all' as const } }
    ] };
    const page = { name: 'Metrics', catalogs: [{ catalog_id: 'synthetic', version: 1 }], roles: structuredClone(roles), widgets: [graph] };
    const metrics = { synthetic: [
      { id: 'cpu', name: 'CPU iowait', value_type: 'numeric_float', units: '%', origin: 'item' },
      { id: 'memory', name: 'Memory', value_type: 'numeric_unsigned', units: 'B', origin: 'item' }
    ] };
    expect(graphRowCount(graph.metrics, page.roles)).toBe(4);
    expect(page.widgets).toHaveLength(1);
    expect(compositionFindings(page, metrics).map((finding) => finding.code)).toContain('graph_mixed_units');
    page.roles.workers.slots?.push('worker-c');
    expect(graphRowCount(graph.metrics, page.roles)).toBe(6);
  });
  it('allows singleton and explicit slots but never silently picks the first of many', () => {
    expect(singleSource({ role: 'primary', selection: 'one' }, roles)).toBe(true);
    expect(singleSource({ role: 'primary', selection: 'all' }, roles)).toBe(true);
    expect(validSource({ role: 'workers', selection: 'all' }, roles)).toBe(true);
    expect(singleSource({ role: 'workers', selection: 'all' }, roles)).toBe(false);
    expect(validSource({ role: 'workers', selection: 'one' }, roles)).toBe(false);
    expect(singleSource({ role: 'workers', selection: 'slot', slot_id: 'worker-a' }, roles)).toBe(true);
    expect(singleSource({ role: 'workers', selection: 'slot', slot_id: '' }, roles)).toBe(false);
    expect(singleSource({ role: 'workers', selection: 'slot', slot_id: 'primary-01' }, roles)).toBe(false);
    expect(validSource({ role: 'workers', selection: 'all', slot_id: 'worker-a' }, roles)).toBe(false);
    expect(validSource({ role: 'missing', selection: 'all' }, roles)).toBe(false);
  });

  it('requires globally unique ordered slots and validates cardinality', () => {
    expect(validNewRole('storage', { cardinality: 'many', slots: ['storage-b', 'storage-a'] }, roles)).toBe(true);
    expect(validNewRole('primary', { cardinality: 'one', slots: ['new-slot'] }, roles)).toBe(false);
    for (const slots of [[], ['same-slot', 'same-slot'], ['worker-a'], ['bad slot']]) {
      expect(validNewRole('storage', { cardinality: 'many', slots }, roles)).toBe(false);
    }
    expect(validNewRole('storage', { cardinality: 'one', slots: ['first-slot', 'second-slot'] }, roles)).toBe(false);
    expect(validNewRole('Bad ID', { cardinality: 'one', slots: ['new-slot'] }, roles)).toBe(false);
  });

  it('renders accessible role controls in both complete catalogs', () => {
    for (const locale of ['en', 'ru'] as const) {
      const i18n = createI18n(locale);
      const { body } = render(OfflinePageBuilder, { props: { i18n } });
      expect(body).toContain('for="offline-role-id"');
      expect(body).toContain('id="offline-role-slots"');
      expect(body).toContain('value="one"');
      expect(body).toContain('value="many"');
      expect(body).toContain('for="offline-evaluation-profile"');
      expect(body).toContain('value="8.0" selected');
      expect(body).toContain('value="7.4"');
      expect(body).toContain(i18n.t('offline_builder.roles'));
      expect(body).not.toContain('offline_builder.');
    }
  });

  it('sends explicit role order and per-series sources through the protected local API', async () => {
    const page: NewOfflinePage = {
      name: 'Synthetic page', catalogs: [{ catalog_id: 'synthetic', version: 2 }], roles,
      evaluation_profile: '8.0',
      widgets: [
        { kind: 'graph', title: 'Graph', metrics: [{ catalog_id: 'synthetic', metric_id: 'rate', hosts: { role: 'workers', selection: 'all' } }] },
        { kind: 'item_value', title: 'Repeated values', metric: { catalog_id: 'synthetic', metric_id: 'rate' }, hosts: { role: 'workers', selection: 'all' }, expansion: 'widget_per_host' }
      ]
    };
    const calls: string[] = [];
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input);
      calls.push(path);
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      expect(path).toBe('/api/library/offline-pages');
      expect(init?.method).toBe('POST');
      expect(new Headers(init?.headers).get(CSRF_HEADER)).toBe('test-csrf');
      expect(new Headers(init?.headers).get('Content-Type')).toBe('application/json');
      expect(JSON.parse(String(init?.body))).toEqual(page);
      return Response.json({ local_id: 'pg-synthetic' }, { status: 201 });
    };
    expect((await createCatalogClient(fetcher).createPage(page)).local_id).toBe('pg-synthetic');
    expect(calls).toEqual(['/api/session', '/api/library/offline-pages']);
  });

  it('reconstructs editable fields from the saved bundle without losing widget identities or series colors', () => {
    const editable = pageRequestFromSaved(savedPage);
    expect(editable.name).toBe('Saved synthetic page');
    expect(editable.catalogs).toEqual([{ catalog_id: 'synthetic', version: 2 }]);
    expect(editable.widgets.map((widget) => widget.widget_id)).toEqual(['stable-graph', 'stable-item']);
    expect(editable.widgets[0]).toMatchObject({ kind: 'graph', metrics: [{ hosts: { role: 'workers', selection: 'slot', slot_id: 'worker-a' }, color: seriesColor }] });
    editable.roles.workers.slots?.reverse();
    expect(savedPage.bundle.project.roles.workers.slots).toEqual(['worker-b', 'worker-a']);
    expect(savedCatalogChoices(savedPage)[0]).toMatchObject({ name: 'Pinned catalog', version: 2, metrics: 1 });
  });

  it('keeps one fixed graph and its geometry when reopened roles grow from two to three hosts', () => {
    const page = structuredClone(savedPage);
    page.bundle.project.root.widgets = [{
      id: 'cpu-iowait', type: 'graph', title: 'CPU iowait', expansion: 'series_per_host',
      series: [{ metric: { catalog: 'synthetic', metric: 'rate' }, hosts: { role: 'workers', selection: 'all' } }],
      layout: { mode: 'fixed', x: 5, y: 7, w: 30, h: 9 }
    }];
    const editable = pageRequestFromSaved(page);
    expect(editable.widgets).toHaveLength(1);
    expect(graphRowCount(editable.widgets[0].kind === 'graph' ? editable.widgets[0].metrics : [], editable.roles)).toBe(2);
    editable.roles.workers.slots?.push('worker-c');
    expect(editable.widgets[0]).toMatchObject({ widget_id: 'cpu-iowait', layout: { mode: 'fixed', x: 5, y: 7, w: 30, h: 9 }, metrics: [{ hosts: { role: 'workers', selection: 'all' } }] });
    expect(graphRowCount(editable.widgets[0].kind === 'graph' ? editable.widgets[0].metrics : [], editable.roles)).toBe(3);
    expect(page.bundle.project.roles.workers.slots).toEqual(['worker-b', 'worker-a']);
  });

  it('keeps local page summaries and load results distinct from Workcopy details', async () => {
    const summary = { storage_kind: 'offline_page', local_id: savedPage.local_id, kind: 'page', origin: 'local', name: savedPage.bundle.project.root.name, local_state: 'saved', updated_at: '2026-11-07T04:00:00Z', counts: { pages: 1, widgets: 2 } };
    const fetcher: typeof fetch = async (input) => String(input) === '/api/drafts'
      ? Response.json({ drafts: [summary], damaged: [] })
      : Response.json({ storage_kind: 'offline_page', page: savedPage });
    const client = createHttpWorkcopyClients(fetcher).drafts;
    const list = await client.list();
    expect(list.status).toBe('ready');
    if (list.status !== 'ready') throw new Error('ready list');
    expect(list.drafts[0]).toMatchObject({ storage_kind: 'offline_page', origin: 'local', provenance: { kind: 'created_here' } });
    expect(list.drafts[0].source).toBeUndefined();
    expect(await client.load(savedPage.local_id)).toEqual({ status: 'offline_page', page: savedPage });
    const { body } = render(DraftsScreen, { props: { i18n: createI18n('en'), drafts: list } });
    expect(body).toContain(`/draft/${savedPage.local_id}/overview`);
    expect(body).toContain('Saved synthetic page');
    expect(body).not.toContain('Saved local pages');
  });

  it('updates the same page with its loaded revision and surfaces conflicts without retrying', async () => {
    const calls: string[] = [];
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input); calls.push(path);
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      expect(path).toBe('/api/library/offline-pages/pg-synthetic');
      expect(new Headers(init?.headers).get(CSRF_HEADER)).toBe('test-csrf');
      expect(JSON.parse(String(init?.body))).toEqual({ expected_revision: 3, page: pageRequestFromSaved(savedPage) });
      return Response.json({ code: 'offline_page_conflict', params: {} }, { status: 409 });
    };
    await expect(createCatalogClient(fetcher).updatePage(savedPage.local_id, savedPage.revision, pageRequestFromSaved(savedPage))).rejects.toMatchObject({ response: { code: 'offline_page_conflict' } });
    expect(calls).toEqual(['/api/session', '/api/library/offline-pages/pg-synthetic']);
  });

  it('retains opaque fields on the server instead of round-tripping them through editable fields', () => {
    const page = structuredClone(savedPage);
    page.bundle.project.root.widgets.push({ id: 'opaque', type: 'zabbix', name: 'Opaque widget', widget_type: 'vendor.status', registry_state: 'unknown', fields: [{ vendor: 'preserve' }], layout: { mode: 'fixed', x: 40, y: 20, w: 12, h: 5 } });
    const request = pageRequestFromSaved(page);
    expect(request.widgets.at(-1)).toEqual({ kind: 'preserved', widget_id: 'opaque', layout: { mode: 'fixed', x: 40, y: 20, w: 12, h: 5 } });
    expect(JSON.stringify(request)).not.toContain('vendor.status');
    expect(page.bundle.project.root.widgets.at(-1)).toHaveProperty('fields');
  });

  it('shows persisted registry findings and rejects a JSON metric in either logical widget', () => {
    const page = structuredClone(savedPage);
    page.widget_evaluation = {
      profile: '8.0',
      unrecognized: [{ widget_id: 'opaque', widget_type: 'svggraph', unknown_type: false, unknown_field_count: 1 }]
    };
    page.bundle.project.root.widgets.push({
      id: 'opaque', type: 'zabbix', name: 'Preserved graph', widget_type: 'svggraph',
      registry_state: 'known', fields: [{ name: 'vendor.hint' }], layout: { mode: 'auto', w: 12, h: 5 }
    });
    page.bundle.catalogs.synthetic.metrics[0].value_type = 'json';
    const findings = compositionFindings(pageRequestFromSaved(page), { synthetic: page.bundle.catalogs.synthetic.metrics }, page.widget_evaluation);
    expect(findings).toContainEqual(expect.objectContaining({
      code: 'unrecognized_widget_fields', widget_ids: ['opaque'], blocks: [],
      message: { id: 'offline_editor.check.unrecognized_fields', params: { count: 1 } }
    }));
    expect(findings.filter(({ code }) => code === 'metric_type').map(({ widget_ids }) => widget_ids)).toEqual([['stable-graph'], ['stable-item']]);
    const i18n = createI18n('ru');
    const { body } = render(OfflinePageBuilder, { props: { i18n, initialPage: page } });
    expect(body).toContain(i18n.t('offline_builder.profile_current', { version: '8.0' }));
  });

  it('reconciles a lost save response by one read, with no second write', async () => {
    const content = pageRequestFromSaved(savedPage); content.name = 'Recovered save';
    const latest = structuredClone(savedPage); latest.revision++; latest.bundle.project.root.name = content.name;
    const calls: string[] = [];
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input); calls.push(`${init?.method ?? 'GET'} ${path}`);
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      if (init?.method === 'POST') throw new TypeError('response lost after commit');
      return Response.json(latest);
    };
    expect(await saveKnownPage(createCatalogClient(fetcher), savedPage, content)).toEqual(latest);
    expect(calls.filter((call) => call.startsWith('POST'))).toHaveLength(1);
    expect(calls.at(-1)).toBe('GET /api/library/offline-pages/pg-synthetic');
  });

  it('turns a different read-back into a conflict instead of blindly retrying', async () => {
    const content = pageRequestFromSaved(savedPage); content.name = 'Our edit';
    const latest = structuredClone(savedPage); latest.revision++; latest.bundle.project.root.name = 'Other tab';
    let writes = 0;
    const client = { updatePage: async () => { writes++; throw new TypeError('lost'); }, page: async () => latest };
    await expect(saveKnownPage(client, savedPage, content)).rejects.toMatchObject({ response: { code: 'offline_page_conflict' } });
    expect(writes).toBe(1);
  });
  it('keeps edit tokens in memory and sends them only in protected request headers', async () => {
    const token = 'synthetic-editor-token'; const calls: string[] = [];
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input); calls.push(path);
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      expect(new Headers(init?.headers).get(CSRF_HEADER)).toBe('test-csrf');
      expect(path).not.toContain(token); expect(String(init?.body)).not.toContain(token);
      if (path.endsWith('/edit-session')) { expect(JSON.parse(String(init?.body))).toEqual({ take_over: false }); return Response.json({ state: 'editing', token }); }
      expect(new Headers(init?.headers).get('X-ZDashstitch-Editor')).toBe(token);
      if (path.endsWith('/release')) expect(init?.keepalive).toBe(true);
      return Response.json(savedPage);
    };
    const client = createCatalogClient(fetcher);
    expect(await client.acquirePage(savedPage.local_id)).toBe('editing');
    await client.updatePage(savedPage.local_id, savedPage.revision, pageRequestFromSaved(savedPage));
    await client.renewPage(savedPage.local_id); await client.releasePage(savedPage.local_id);
    expect(calls).toHaveLength(5);
  });
});
