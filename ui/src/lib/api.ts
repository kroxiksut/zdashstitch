export const HEALTH_ENDPOINT = '/api/health' as const;

export interface HealthResponse {
  status: string;
  product_stage: string;
  target_zabbix_family: string;
  zabbix_writes_enabled: boolean;
}

export async function loadHealth(fetcher: typeof fetch = fetch): Promise<HealthResponse> {
  const response = await fetcher(HEALTH_ENDPOINT, {
    headers: { Accept: 'application/json' },
    method: 'GET'
  });
  if (!response.ok) {
    throw new Error(`Local backend returned HTTP ${response.status}`);
  }
  return (await response.json()) as HealthResponse;
}
