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
      headers: { Accept: 'application/json' },
      method: 'GET'
    });
    expect(HEALTH_ENDPOINT.startsWith('/api/')).toBe(true);
    expect(health.zabbix_writes_enabled).toBe(false);
  });
});
