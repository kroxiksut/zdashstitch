import { describe, expect, it } from 'vitest';
import { CSRF_HEADER } from '../session';
import { createCatalogClient } from './client';

describe('template library client', () => {
  it('binds each save-as request to the chosen kind, format and revision', async () => {
    const calls: string[] = [];
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input);
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      expect(new Headers(init?.headers).get(CSRF_HEADER)).toBe('test-csrf');
      const body = JSON.parse(String(init?.body));
      expect(body.expected_revision).toBe(7);
      expect(body.job_id).toBe('job');
      calls.push(`${path}:${String(body.yaml)}`);
      return Response.json({ cancelled: true });
    };
    const client = createCatalogClient(fetcher);
    for (const kind of ['page', 'dashboard'] as const) for (const yaml of [false, true]) await client.saveExport(kind, 'draft', 7, yaml, 'job');
    expect(calls).toEqual(['/api/library/offline-pages/draft/save-export:false', '/api/library/offline-pages/draft/save-export:true', '/api/library/offline-dashboards/draft/save-export:false', '/api/library/offline-dashboards/draft/save-export:true']);
  });
  it('binds copy export to the reviewed revision, changes and digest', async () => {
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input);
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      expect(new Headers(init?.headers).get(CSRF_HEADER)).toBe('test-csrf');
      const body = JSON.parse(String(init?.body));
      expect(body.expected_revision).toBe(4);
      expect(body.changes).toEqual([{ target: { kind: 'page_name', page_index: 0 }, replacement: 'page-1', apply_to_matching: true }]);
      if (path.endsWith('/redaction-preview')) return Response.json({ revision: 4, sha256: 'a'.repeat(64), changes: [], remaining: { findings: [], truncated: false, unknown_objects: 0, credential_candidates: 0 } });
      expect(path).toBe('/api/library/offline-pages/page-id/redaction-export');
      expect(body.preview_sha256).toBe('a'.repeat(64));
      expect(body.acknowledged).toBe(true);
      return Response.json({ id: 'download', display_name: 'copy.zip', sha256: 'a'.repeat(64), size_bytes: 1, entry_hashes: {} });
    };
    const client = createCatalogClient(fetcher);
    const changes = [{ target: { kind: 'page_name' as const, page_index: 0 }, replacement: 'page-1', apply_to_matching: true }];
    const preview = await client.previewRedaction('page', 'page-id', 4, changes);
    expect((await client.exportRedaction('page', 'page-id', 4, changes, preview.sha256)).id).toBe('download');
  });
  it('previews a package and submits an explicit legacy page selection', async () => {
    const calls: string[] = [];
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input); calls.push(path);
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      expect(new Headers(init?.headers).get(CSRF_HEADER)).toBe('test-csrf');
      if (path === '/api/upload-sets') return Response.json({ id: 'set' });
      if (path.startsWith('/api/uploads/')) return Response.json({});
      if (path.endsWith('/inspect')) return Response.json([{ file: 'legacy.zip', outcome: { status: 'ready', version: 'v1', dashboards: [] } }]);
      if (path.endsWith('/run')) {
        expect(JSON.parse(String(init?.body))).toEqual({ selections: [{ file: 'legacy.zip', dashboard_id: 'db', page_id: 'page' }] });
        return Response.json([{ file: 'legacy.zip', outcome: { status: 'saved', kind: 'page', local_id: 'pg' } }]);
      }
      throw new Error('unexpected path');
    };
    const client = createCatalogClient(fetcher);
    const preview = await client.inspectPackages([new File(['data'], 'legacy.zip')]);
    expect(preview.files[0]?.outcome.version).toBe('v1');
    const result = await client.importPackages(preview.id, [{ file: 'legacy.zip', dashboard_id: 'db', page_id: 'page' }]);
    expect(result[0]?.outcome.kind).toBe('page');
    expect(calls).toContain('/api/library/package-import/set/inspect');
  });
  it('prepares a page export through the protected page route', async () => {
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input);
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      expect(path).toBe('/api/library/offline-pages/page%20id/export');
      expect(init?.method).toBe('POST');
      expect(new Headers(init?.headers).get(CSRF_HEADER)).toBe('test-csrf');
      return Response.json({ id: 'download-id', display_name: 'page.zip' });
    };
    expect(await createCatalogClient(fetcher).preparePageExport('page id')).toEqual({ id: 'download-id', display_name: 'page.zip' });
  });
  it('routes an embedded page editor through its dashboard and keeps the lease token in headers', async () => {
    const calls: string[] = [];
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input); calls.push(path);
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      if (path.endsWith('/edit-session')) return Response.json({ state: 'editing', token: 'editor-token' });
      if (init?.method === 'POST') expect(new Headers(init.headers).get('X-ZDashstitch-Editor')).toBe('editor-token');
      return Response.json({ revision: 2 });
    };
    const client = createCatalogClient(fetcher, 'db-local');
    const pagePath = '/api/library/offline-dashboards/db-local/pages/pg-local';
    await client.page('pg-local');
    expect(await client.acquirePage('pg-local')).toBe('editing');
    await client.updatePage('pg-local', 1, { name: 'Local page', catalogs: [], roles: {}, widgets: [] });
    expect(calls).toEqual([pagePath, '/api/session', `${pagePath}/edit-session`, pagePath]);
  });

  it('previews with protected upload handles and saves only on confirmation', async () => {
    const calls: string[] = [];
    const fetcher: typeof fetch = async (input, init) => {
      const path = String(input);
      calls.push(path);
      const headers = new Headers(init?.headers);
      if (init?.method === 'POST') expect(headers.get(CSRF_HEADER)).toBe('test-csrf');
      if (path === '/api/session') return Response.json({ csrf_token: 'test-csrf' });
      if (path === '/api/upload-sets') return Response.json({ id: 'set-id' }, { status: 201 });
      if (path === '/api/uploads/set-id/files/Lab%20export.yaml') {
        expect(init?.body).toBeInstanceOf(File);
        expect(headers.get('Content-Type')).toBe('application/octet-stream');
        return Response.json({}, { status: 201 });
      }
      if (path.endsWith('/inspect')) return Response.json([{ file: 'Lab export.yaml', status: 'ready', catalogs: [] }]);
      if (path.endsWith('/run') || path.endsWith('/catalogs')) return Response.json([]);
      throw new Error(`unexpected path: ${path}`);
    };
    const client = createCatalogClient(fetcher);
    const selection = await client.inspect([new File(['fixture'], 'Lab export.yaml')]);
    expect(selection.id).toBe('set-id');
    expect(selection.files[0]?.status).toBe('ready');
    expect(calls).toEqual([
      '/api/session', '/api/upload-sets', '/api/uploads/set-id/files/Lab%20export.yaml',
      '/api/library/template-import/set-id/inspect'
    ]);
    await client.run(selection.id ?? '');
    await client.list();
    expect(calls.slice(-2)).toEqual(['/api/library/template-import/set-id/run', '/api/library/catalogs']);
  });
});
