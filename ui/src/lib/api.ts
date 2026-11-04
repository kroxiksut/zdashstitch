export const HEALTH_ENDPOINT = '/api/health' as const;

export interface HealthResponse {
  status: string;
  product_stage: string;
  target_zabbix_family: string;
  zabbix_writes_enabled: boolean;
}

export interface ApiErrorResponse {
  code: string;
  params: Record<string, string>;
}

export class ApiFailure extends Error {
  constructor(
    readonly status: number,
    readonly response: ApiErrorResponse | null
  ) {
    super('local API request failed');
    this.name = 'ApiFailure';
  }
}

/** Data-only failure of a non-2xx response: the backend sends a code and parameters, never prose. */
export async function apiFailure(response: Response): Promise<ApiFailure> {
  const contentType = response.headers.get('Content-Type') ?? '';
  const error = contentType.startsWith('application/json')
    ? ((await response.json()) as ApiErrorResponse)
    : null;
  return new ApiFailure(response.status, error);
}

export async function loadHealth(fetcher: typeof fetch = fetch): Promise<HealthResponse> {
  const response = await fetcher(HEALTH_ENDPOINT, {
    credentials: 'same-origin',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    method: 'GET'
  });
  if (!response.ok) {
    throw await apiFailure(response);
  }
  return (await response.json()) as HealthResponse;
}
