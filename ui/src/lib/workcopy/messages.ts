// Machine codes of the draft and server-dashboard routes → message IDs (P1-03H, P1-00F §3). The
// backend never sends prose. Zabbix failures keep the connection taxonomy and are mapped by the
// connection messages; unknown codes fall back to `errors.connection.unknown` with the code.

import type { LocalizedText } from '../components';
import { codeText } from '../connections/messages';

type Params = Readonly<Record<string, string>>;

function count(params: Params, name: string): number {
  const value = Number(params[name]);
  return Number.isFinite(value) ? value : Number.NaN;
}

const SIMPLE = {
  not_visible_or_denied: 'errors.workcopy.not_visible_or_denied',
  import_inconsistent: 'errors.import.inconsistent',
  import_page_not_found: 'errors.workcopy.page_not_found',
  // Malformed opaque IDs or limits come only from a broken client, never from user input.
  connection_id_invalid: 'errors.connection.request_invalid',
  dashboard_id_invalid: 'errors.connection.request_invalid',
  page_id_invalid: 'errors.connection.request_invalid',
  workcopy_id_invalid: 'errors.connection.request_invalid',
  list_limit_invalid: 'errors.connection.request_invalid',
  workcopy_not_found: 'startup.draft.missing',
  workcopy_corrupt: 'errors.workcopy.corrupt',
  workcopy_too_large: 'errors.workcopy.too_large',
  workcopy_unsupported_operation: 'errors.workcopy.unsupported_operation',
  snapshot_missing: 'errors.snapshot.missing',
  snapshot_corrupt: 'errors.snapshot.corrupt',
  snapshot_too_large: 'errors.snapshot.too_large',
  snapshot_unsupported_adapter: 'errors.snapshot.unsupported_adapter',
  snapshot_shape_invalid: 'errors.snapshot.shape_invalid',
  storage_unavailable: 'errors.workcopy.storage_unavailable',
  storage_linked_path: 'errors.storage.linked',
  storage_file_too_large: 'errors.workcopy.storage_file_too_large',
  storage_limit_reached: 'errors.workcopy.storage_limit_reached',
  storage_write_failed: 'errors.workcopy.storage_write_failed',
  settings_unavailable: 'errors.workcopy.settings_unavailable'
} as const;

/**
 * Strict YAML/JSON reader codes of `core::formats::strict` → the import messages of P1-00F §3.
 * Aliases and excessive nesting are unsafe structures; duplicate or non-string keys are
 * inconsistent data; anything else is a parse failure of the named format.
 */
const PARSE_CODES = {
  yaml_alias: 'unsafe_structure',
  yaml_depth: 'unsafe_structure',
  json_depth: 'unsafe_structure',
  yaml_duplicate_key: 'validation',
  yaml_key: 'validation',
  json_duplicate_key: 'validation',
  json_key: 'validation',
  yaml_invalid: 'parse_yaml',
  yaml_data: 'parse_yaml',
  json_invalid: 'parse_json',
  json_data: 'parse_json'
} as const;

/** Every strict YAML/JSON reader code with its own import message. */
export const PARSE_ERROR_CODES: ReadonlyArray<string> = Object.keys(PARSE_CODES);

function parseCodeText(code: keyof typeof PARSE_CODES): LocalizedText {
  switch (PARSE_CODES[code]) {
    case 'unsafe_structure':
      return { id: 'errors.import.unsafe_structure', params: { code } };
    case 'validation':
      return { id: 'errors.import.validation', params: { code } };
    case 'parse_yaml':
      return { id: 'errors.import.parse', params: { format: 'YAML' } };
    case 'parse_json':
      return { id: 'errors.import.parse', params: { format: 'JSON' } };
  }
}

/** Every code the workcopy routes add to the connection codes (P2-05A–D). */
export const WORKCOPY_CODES: ReadonlyArray<string> = [...Object.keys(SIMPLE), 'search_invalid', 'draft_name_invalid'];

/** User-facing text of a workcopy, import or storage failure. */
export function workcopyCodeText(code: string, params: Params = {}): LocalizedText {
  if (code in SIMPLE) return { id: SIMPLE[code as keyof typeof SIMPLE] } as LocalizedText;
  if (code in PARSE_CODES) return parseCodeText(code as keyof typeof PARSE_CODES);
  switch (code) {
    case 'search_invalid':
      return { id: 'errors.workcopy.search_invalid', params: { max: count(params, 'max') } };
    case 'draft_name_invalid':
      return { id: 'errors.drafts.name_invalid', params: { max: count(params, 'max') } };
    default:
      return codeText(code, params);
  }
}

/** Codes after which the next step is signing in again, not retrying (P1-03B «Нужен вход»). */
const SIGN_IN_CODES: ReadonlySet<string> = new Set([
  'session_expired',
  'token_expired',
  'token_not_authorized',
  'auth_failed',
  'mfa_session_terminated'
]);

export type RecoveryAction = 'sign_in' | 'connect' | 'retry';

/** What the user can do after a failed server read (P1-03B states, P1-01E §5). */
export function recoveryAction(code: string): RecoveryAction {
  if (SIGN_IN_CODES.has(code)) return 'sign_in';
  if (code === 'not_connected' || code === 'connections_unavailable') return 'connect';
  return 'retry';
}
