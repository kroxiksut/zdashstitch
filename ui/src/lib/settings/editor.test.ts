import { describe, expect, it, vi } from 'vitest';
import { CSRF_HEADER } from '../session';
import { createEditorSettingsClient, type EditorPreferences } from './editor';

const values: EditorPreferences = {
  autosave: 'on', autosave_max_interval_s: 30, default_zoom: 'fit_width', show_grid: true,
  show_rulers: true, confirm_delete_widget: false, confirm_delete_page: true, confirm_bulk_apply: true
};

describe('editor settings client', () => {
  it('reads the snapshot and sends a full fingerprint-bound update with CSRF', async () => {
    const fetcher = vi.fn(async (url: string, init?: RequestInit) => {
      if (url === '/api/session') return Response.json({ csrf_token: 'csrf-1' });
      if (init?.method === 'PUT') return Response.json({ values: { ...values, autosave: 'off' }, file_state: 'valid', fingerprint: 'next', session_only: false });
      return Response.json({ values, file_state: 'missing', fingerprint: 'missing', session_only: false });
    });
    const client = createEditorSettingsClient(fetcher as typeof fetch);
    const initial = await client.load();
    expect(initial.values.autosave).toBe('on');
    const saved = await client.save({ ...initial.values, autosave: 'off' }, initial.fingerprint);
    expect(saved.values.autosave).toBe('off');
    const call = fetcher.mock.calls.find((entry) => (entry[1] as RequestInit | undefined)?.method === 'PUT');
    const request = call?.[1] as RequestInit;
    expect(JSON.parse(request.body as string)).toEqual({ values: { ...values, autosave: 'off' }, fingerprint: 'missing' });
    expect((request.headers as Record<string, string>)[CSRF_HEADER]).toBe('csrf-1');
  });

  it('rejects an unknown server value', async () => {
    const client = createEditorSettingsClient((async () => Response.json({ values: { ...values, autosave: 'maybe' }, file_state: 'valid', fingerprint: 'x', session_only: false })) as typeof fetch);
    await expect(client.load()).rejects.toThrow('settings_invalid_response');
  });

  it('accepts a session-only value when the file is invalid', async () => {
    const client = createEditorSettingsClient((async () => Response.json({ values: { ...values, autosave: 'off' }, file_state: 'invalid', fingerprint: 'broken', session_only: true })) as typeof fetch);
    const snapshot = await client.load();
    expect(snapshot.values.autosave).toBe('off');
    expect(snapshot.session_only).toBe(true);
  });
});
