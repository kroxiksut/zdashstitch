// Codes of the publication screens (P1-08D–G, P1-01G §4/§7) → message IDs. The backend never sends
// prose; unknown codes fall back to a message that shows the code, never to an empty text.

import type { LocalizedText } from '../components';
import type { I18n } from '../i18n';
import { planInvalidText, replaceHostCodeText } from '../bindings/messages';
import type { JournalState, PublicationFailure, RemoteChangeKind } from './model';

type Tone = 'success' | 'info' | 'warning' | 'danger';

export const STATE_LABEL: Readonly<Record<JournalState, LocalizedText>> = {
  confirmed: { id: 'publication.state.confirmed' },
  submitting: { id: 'publication.state.submitting' },
  verified: { id: 'publication.state.verified' },
  conflict: { id: 'publication.state.conflict' },
  failed_before_send: { id: 'publication.state.failed_before_send' },
  failed_after_response: { id: 'publication.state.failed_after_response' },
  unknown_outcome: { id: 'publication.state.unknown_outcome' },
  reconciled_verified: { id: 'publication.state.reconciled_verified' },
  reconciled_failed: { id: 'publication.state.reconciled_failed' },
  manual_check_required: { id: 'publication.state.manual_check_required' },
  closed_manually: { id: 'publication.state.closed_manually' }
};

export const STATE_TONE: Readonly<Record<JournalState, Tone>> = {
  confirmed: 'info',
  submitting: 'warning',
  verified: 'success',
  conflict: 'danger',
  failed_before_send: 'danger',
  failed_after_response: 'danger',
  unknown_outcome: 'warning',
  reconciled_verified: 'success',
  reconciled_failed: 'danger',
  manual_check_required: 'warning',
  closed_manually: 'info'
};

export const JOURNAL_STATES: ReadonlyArray<JournalState> = Object.keys(STATE_LABEL) as JournalState[];

export function stateLabel(state: string): LocalizedText {
  return state in STATE_LABEL ? STATE_LABEL[state as JournalState] : { id: 'publication.evidence.unknown', params: { code: state } };
}

/**
 * The result body of a final or pending state. `evidence` decides the special cases accepted on
 * 28.10.2026 (evening): `dashboard_name_taken` after send is the final «не записано» with «имя
 * занято, возможно, недоступной вам панелью» (item 3); zero candidates of a create reconciliation
 * is a manual check, never «Сверено: не записано» (item 1).
 */
export function resultBody(i18n: I18n, state: JournalState, kind: 'update' | 'create', evidence: string | null, invalidated: string | null): LocalizedText {
  switch (state) {
    case 'verified':
      return { id: 'publication.result.verified_body' };
    case 'reconciled_verified':
      return { id: 'publication.result.reconciled_verified_body' };
    case 'reconciled_failed':
      return evidence === 'dashboard_name_taken' ? { id: 'errors.publication.name_taken_maybe_hidden' } : { id: 'publication.result.reconciled_failed_body' };
    case 'unknown_outcome':
    case 'submitting':
      return { id: 'publication.result.unknown_body' };
    case 'failed_after_response':
      return { id: 'publication.result.failed_after_response_body' };
    case 'manual_check_required':
      return kind === 'create' ? { id: 'publication.result.manual_body' } : { id: 'publication.result.manual_update_body' };
    case 'closed_manually':
      return { id: 'publication.result.closed_manually_body' };
    case 'conflict':
      return { id: 'publication.conflict.body' };
    case 'failed_before_send':
    case 'confirmed':
      if (evidence === 'dashboard_name_taken') return { id: 'errors.publication.name_taken_maybe_hidden' };
      return {
        id: 'publication.result.failed_before_send_body',
        params: { reason: resolveInvalidated(i18n, invalidated ?? evidence) }
      };
  }
}

function resolveInvalidated(i18n: I18n, code: string | null): string {
  if (code === null) return i18n.t('publication.plan.reason.unknown', { code: '—' });
  const text = planInvalidText(i18n, code);
  return 'params' in text && text.params && 'reason' in text.params ? String(text.params.reason) : code;
}

const EVIDENCE = {
  read_back_matches: 'publication.evidence.read_back_matches',
  reconcile_confirmed: 'publication.evidence.reconcile_confirmed',
  dashboard_name_taken: 'publication.evidence.dashboard_name_taken',
  reconcile_not_found: 'publication.evidence.reconcile_not_found',
  reconcile_ambiguous: 'publication.evidence.reconcile_ambiguous',
  reconcile_structure_mismatch: 'publication.evidence.reconcile_structure_mismatch',
  expected_found: 'publication.evidence.expected_found',
  source_found: 'publication.evidence.source_found',
  other_found: 'publication.evidence.other_found',
  not_visible: 'publication.evidence.not_visible'
} as const;

export const EVIDENCE_CODES: ReadonlyArray<string> = Object.keys(EVIDENCE);

export function evidenceText(code: string): LocalizedText {
  if (code in EVIDENCE) return { id: EVIDENCE[code as keyof typeof EVIDENCE] };
  return { id: 'publication.evidence.unknown', params: { code } };
}

export const REMOTE_CHANGE_LABEL: Readonly<Record<RemoteChangeKind, LocalizedText>> = {
  dashboard_properties: { id: 'publication.conflict.change.dashboard_properties' },
  access: { id: 'publication.conflict.change.access' },
  page_added: { id: 'publication.conflict.change.page_added' },
  page_removed: { id: 'publication.conflict.change.page_removed' },
  page_renamed: { id: 'publication.conflict.change.page_renamed' },
  page_changed: { id: 'publication.conflict.change.page_changed' },
  widget_added: { id: 'publication.conflict.change.widget_added' },
  widget_removed: { id: 'publication.conflict.change.widget_removed' },
  widget_changed: { id: 'publication.conflict.change.widget_changed' },
  widget_moved: { id: 'publication.conflict.change.widget_moved' }
};

export function intentText(intent: string): LocalizedText {
  switch (intent) {
    case 'replace_host':
      return { id: 'intent.replace_host' };
    case 'save_as_copy':
      return { id: 'intent.save_as_copy' };
    case 'replace_item_fields':
      return { id: 'intent.replace_item_fields' };
    default:
      return { id: 'intent.unknown', params: { code: intent } };
  }
}

const ERRORS = {
  write_mode_required: 'publication.finding.write_mode_required',
  plan_not_found: 'errors.publication.plan_not_found',
  plan_mismatch: 'errors.publication.plan_mismatch',
  publication_blocked: 'errors.publication.blocked',
  local_mismatch: 'errors.publication.local_mismatch',
  reconcile_server_changed: 'errors.publication.reconcile_server_changed',
  reconcile_partial_visibility: 'errors.publication.reconcile_partial_visibility',
  reconcile_not_needed: 'errors.publication.reconcile_not_needed',
  operation_running: 'errors.publication.operation_running',
  operation_not_found: 'errors.publication.operation_not_found',
  journal_not_damaged: 'errors.publication.journal_not_damaged',
  quarantine_exists: 'errors.publication.quarantine_exists',
  journal_write_failed: 'errors.publication.journal_write_failed',
  dashboard_name_taken: 'errors.publication.name_taken_maybe_hidden'
} as const;

/** Every code the publication routes add to the draft and connection codes. */
export const PUBLICATION_ERROR_CODES: ReadonlyArray<string> = [...Object.keys(ERRORS), 'plan_invalidated', 'target_blocked', 'journal_damaged'];

/** User-facing text of a refused publication command. */
export function publicationCodeText(i18n: I18n, failure: Pick<PublicationFailure, 'code' | 'params'>): LocalizedText {
  const { code, params } = failure;
  if (code in ERRORS) return { id: ERRORS[code as keyof typeof ERRORS] } as LocalizedText;
  switch (code) {
    case 'plan_invalidated':
      return planInvalidText(i18n, params.reason ?? 'unknown');
    case 'target_blocked':
      return { id: 'errors.publication.target_blocked', params: { operation_id: params.operation_id ?? '—' } };
    case 'journal_damaged':
      return { id: 'errors.publication.journal_damaged', params: { operation_id: params.operation_id ?? '—' } };
    case 'write_contract_unverified':
    case 'changed_page_widget_unproven':
    case 'create_link_outside_page':
    case 'publication_authority_offline':
      return { id: 'errors.publication.code', params: { code } };
    default: {
      const text = replaceHostCodeText(code, params);
      return text.id === 'errors.connection.unknown' ? { id: 'errors.publication.code', params: { code } } : text;
    }
  }
}
