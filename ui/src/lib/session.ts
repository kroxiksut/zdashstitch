import { apiFailure } from './api';

export const SESSION_ENDPOINT = '/api/session' as const;
export const SESSION_END_ENDPOINT = '/api/session/end' as const;
/** CSRF header checked by the local server for every mutating `/api` request (P2-SEC-01). */
export const CSRF_HEADER = 'X-ZDashstitch-CSRF' as const;

interface SessionResponse {
  csrf_token: string;
}

/** CSRF token of the current local session. It lives only in memory, never in browser storage. */
export async function loadCsrfToken(fetcher: typeof fetch = fetch): Promise<string> {
  const response = await fetcher(SESSION_ENDPOINT, {
    credentials: 'same-origin',
    headers: { Accept: 'application/json' },
    method: 'GET'
  });
  if (!response.ok) {
    throw await apiFailure(response);
  }
  return ((await response.json()) as SessionResponse).csrf_token;
}

/**
 * `session.end` (P2-SEC-01E, O-032, P1-01G §5). Call only after the user confirmed the
 * `session.end.confirm.*` dialog. Afterwards every tab gets `http.security.session_invalid`
 * and shows `session.expired.*`; this tab may show `session.ended.title`.
 * Rejects with `ApiFailure` 409 `session.end.unavailable_writing` while a write stage runs.
 */
export async function endLocalSession(fetcher: typeof fetch = fetch): Promise<void> {
  const csrf = await loadCsrfToken(fetcher);
  const response = await fetcher(SESSION_END_ENDPOINT, {
    body: JSON.stringify({ confirmed: true }),
    credentials: 'same-origin',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json', [CSRF_HEADER]: csrf },
    method: 'POST'
  });
  if (!response.ok) {
    throw await apiFailure(response);
  }
}
