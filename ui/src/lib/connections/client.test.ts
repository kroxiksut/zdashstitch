import { describe, expect, it } from 'vitest';

import { CSRF_HEADER, SESSION_ENDPOINT } from '../session';
import { createHttpConnectionsClient, type ProfileInput } from './client';

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

const INPUT: ProfileInput = {
  name: 'Lab',
  address: 'zabbix.example.invalid',
  auth: { method: 'api_token', store_token: false },
  trust: { kind: 'system' },
  confirm_plain_http_token_storage: false
};

interface Call {
  readonly path: string;
  readonly method: string;
  readonly csrf: string | null;
}

/** Fake local server whose `/api/session` fails `sessionFailures` times before answering. */
function fakeServer(sessionFailures: number) {
  const calls: Call[] = [];
  let failures = sessionFailures;
  const fetcher: typeof fetch = async (input, init) => {
    const path = String(input);
    const headers = (init?.headers ?? {}) as Record<string, string>;
    calls.push({ path, method: init?.method ?? 'GET', csrf: headers[CSRF_HEADER] ?? null });
    if (path === SESSION_ENDPOINT) {
      if (failures > 0) {
        failures -= 1;
        return json({ code: 'http.security.session_invalid', params: {} }, 503);
      }
      return json({ csrf_token: 'csrf-token' });
    }
    if (path === '/api/upload-sets') return json({ id: 'set-1' }, 201);
    if (path.startsWith('/api/uploads/')) return json({}, 201);
    if (path === '/api/connections/trusted-ca') return json({ ca_ref: 'sha256-x', certificate_fingerprints: [] }, 201);
    return json({ local_id: 'conn-1' }, 201);
  };
  return { calls, fetcher };
}

describe('connections client CSRF token', () => {
  it('does not cache a failed token load during a PEM import', async () => {
    const server = fakeServer(1);
    const client = createHttpConnectionsClient(server.fetcher);
    const pem = new File(['-----BEGIN CERTIFICATE-----'], 'ca.pem');

    await expect(client.importTrustedCa(pem)).rejects.toBeTruthy();
    await expect(client.create(INPUT)).resolves.toMatchObject({ local_id: 'conn-1' });
    await expect(client.importTrustedCa(pem)).resolves.toMatchObject({ ca_ref: 'sha256-x' });

    expect(server.calls.filter((call) => call.path === SESSION_ENDPOINT)).toHaveLength(2);
    const mutating = server.calls.filter((call) => call.method !== 'GET');
    expect(mutating.length).toBeGreaterThan(0);
    expect(mutating.every((call) => call.csrf === 'csrf-token')).toBe(true);
  });

  it('does not cache a failed token load during a command and reuses a loaded token', async () => {
    const server = fakeServer(1);
    const client = createHttpConnectionsClient(server.fetcher);

    await expect(client.create(INPUT)).rejects.toBeTruthy();
    await expect(client.importTrustedCa(new File(['pem'], 'ca.pem'))).resolves.toMatchObject({
      ca_ref: 'sha256-x'
    });
    await expect(client.create(INPUT)).resolves.toMatchObject({ local_id: 'conn-1' });

    expect(server.calls.filter((call) => call.path === SESSION_ENDPOINT)).toHaveLength(2);
  });
});
