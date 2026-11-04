import { describe, expect, it, vi } from 'vitest';

import { CSRF_HEADER, SESSION_END_ENDPOINT, SESSION_ENDPOINT, endLocalSession } from './session';

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

describe('end local session (P2-SEC-01E)', () => {
  it('sends the confirmation with the in-memory CSRF token to the same-origin endpoint', async () => {
    const fetcher = vi.fn(async (input: RequestInfo | URL) =>
      input === SESSION_ENDPOINT ? json({ csrf_token: 'token-1' }) : new Response(null, { status: 204 })
    );

    await endLocalSession(fetcher);

    expect(fetcher).toHaveBeenLastCalledWith(SESSION_END_ENDPOINT, {
      body: JSON.stringify({ confirmed: true }),
      credentials: 'same-origin',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json', [CSRF_HEADER]: 'token-1' },
      method: 'POST'
    });
    expect(SESSION_END_ENDPOINT.startsWith('/api/')).toBe(true);
  });

  it('reports a running write as a data-only code', async () => {
    const fetcher = vi.fn(async (input: RequestInfo | URL) =>
      input === SESSION_ENDPOINT
        ? json({ csrf_token: 'token-1' })
        : json({ code: 'session.end.unavailable_writing', params: {} }, 409)
    );

    await expect(endLocalSession(fetcher)).rejects.toMatchObject({
      status: 409,
      response: { code: 'session.end.unavailable_writing', params: {} }
    });
  });

  it('does not post without a valid session', async () => {
    const fetcher = vi.fn(async () => json({ code: 'http.security.session_invalid', params: {} }, 401));

    await expect(endLocalSession(fetcher)).rejects.toMatchObject({ status: 401 });
    expect(fetcher).toHaveBeenCalledTimes(1);
  });
});
