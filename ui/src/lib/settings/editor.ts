import { apiFailure } from '../api';
import { CSRF_HEADER, loadCsrfToken } from '../session';
import type { SettingsFileState } from './appearance.svelte';
import { settingsFileIssue, type SettingsFileIssue } from './file-issue';

export interface EditorPreferences {
  autosave: 'on' | 'off';
  autosave_max_interval_s: 30 | 60 | 120;
  default_zoom: 'fit_width' | 'actual_size';
  show_grid: boolean;
  show_rulers: boolean;
  confirm_delete_widget: boolean;
  confirm_delete_page: boolean;
  confirm_bulk_apply: boolean;
}

export interface EditorSnapshot {
  values: EditorPreferences;
  file_state: SettingsFileState;
  fingerprint: string;
  session_only: boolean;
  issue?: SettingsFileIssue | null;
}

const ENDPOINT = '/api/settings/editor';
const FIELDS = ['show_grid', 'show_rulers', 'confirm_delete_widget', 'confirm_delete_page', 'confirm_bulk_apply'] as const;

function valid(value: unknown): value is EditorSnapshot {
  if (!value || typeof value !== 'object') return false;
  const wire = value as Record<string, unknown>;
  if (!wire.values || typeof wire.values !== 'object') return false;
  const editor = wire.values as Record<string, unknown>;
  return (editor.autosave === 'on' || editor.autosave === 'off') && [30, 60, 120].includes(editor.autosave_max_interval_s as number)
    && (editor.default_zoom === 'fit_width' || editor.default_zoom === 'actual_size')
    && FIELDS.every((field) => typeof editor[field] === 'boolean')
    && ['valid', 'missing', 'invalid', 'unreadable'].includes(wire.file_state as string)
    && typeof wire.fingerprint === 'string' && typeof wire.session_only === 'boolean';
}

export function createEditorSettingsClient(fetcher: typeof fetch = (...args) => fetch(...args)) {
  async function send(init: RequestInit): Promise<EditorSnapshot> {
    const response = await fetcher(ENDPOINT, { credentials: 'same-origin', ...init });
    if (!response.ok) throw await apiFailure(response);
    const wire: unknown = await response.json();
    if (!valid(wire)) throw new Error('settings_invalid_response');
    return wire.issue === undefined ? wire : { ...wire, issue: settingsFileIssue(wire.issue) };
  }
  return {
    load: () => send({ method: 'GET', headers: { Accept: 'application/json' } }),
    async save(values: EditorPreferences, fingerprint: string): Promise<EditorSnapshot> {
      const csrf = await loadCsrfToken(fetcher);
      return send({ method: 'PUT', headers: { Accept: 'application/json', 'Content-Type': 'application/json', [CSRF_HEADER]: csrf },
        body: JSON.stringify({ values, fingerprint }) });
    }
  };
}
