import { describe, expect, it, vi } from 'vitest';

import { HEALTH_ENDPOINT, loadHealth } from './api';

describe('local API boundary', () => {
  it('uses only the same-origin health endpoint', async () => {
    const fetcher = vi.fn(async () =>
      new Response(
        JSON.stringify({
          status: 'ok',
          product_stage: 'scaffold',
          target_zabbix_family: '7.4',
          zabbix_writes_enabled: false
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } }
      )
    );

    const health = await loadHealth(fetcher);

    expect(fetcher).toHaveBeenCalledWith(HEALTH_ENDPOINT, {
      credentials: 'same-origin',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      method: 'GET'
    });
    expect(HEALTH_ENDPOINT.startsWith('/api/')).toBe(true);
    expect(health.zabbix_writes_enabled).toBe(false);
  });

  it('keeps backend error details data-only', async () => {
    const fetcher = vi.fn(async () =>
      new Response(JSON.stringify({ code: 'http.security.session_invalid', params: {} }), {
        status: 401,
        headers: { 'Content-Type': 'application/json' }
      })
    );

    await expect(loadHealth(fetcher)).rejects.toMatchObject({
      status: 401,
      response: { code: 'http.security.session_invalid', params: {} }
    });
  });
});
