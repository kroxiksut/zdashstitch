import { describe, expect, it } from 'vitest';
import { CSRF_HEADER } from '../session';
import { createCatalogClient } from './client';

describe('template library client', () => {
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
