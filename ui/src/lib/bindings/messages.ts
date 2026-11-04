// Machine codes of ReplaceHost (P1-05H, P1-08H, P1-IN-03) → message IDs. The backend never sends
// prose: statuses, reasons, findings and errors are codes with safe parameters. Unknown codes fall
// back to a message that shows the code, never to an empty text.

import type { LocalizedText } from '../components';
import type { I18n } from '../i18n';
import { workcopyCodeText } from '../workcopy/messages';
import type { IconName } from '../components';
import type { Applicability, ChangeState, DecisionKind, Finding, FindingGroup, MatchStatus, ReferenceKind, RowCheck } from './model';

type Params = Readonly<Record<string, string>>;

export type Tone = 'success' | 'info' | 'warning' | 'danger';

/** Short labels of 09-03 (P1-05D «как показано»); never colour only: always with an icon. */
export const MATCH_SHORT: Readonly<Record<MatchStatus, LocalizedText>> = {
  exact: { id: 'bindings.match_short.exact' },
  compatible: { id: 'bindings.match_short.compatible' },
  ambiguous: { id: 'bindings.match_short.ambiguous' },
  missing: { id: 'bindings.match_short.missing' },
  unresolved_lld: { id: 'bindings.match_short.unresolved_lld' },
  denied: { id: 'bindings.match_short.denied' }
};

export const MATCH_LONG: Readonly<Record<MatchStatus, LocalizedText>> = {
  exact: { id: 'bindings.match.exact' },
  compatible: { id: 'bindings.match.compatible' },
  ambiguous: { id: 'bindings.match.ambiguous' },
  missing: { id: 'bindings.match.missing' },
  unresolved_lld: { id: 'bindings.match.unresolved_lld' },
  denied: { id: 'bindings.match.denied' }
};

export const MATCH_ICON: Readonly<Record<MatchStatus, IconName>> = {
  exact: 'circle-check',
  compatible: 'alert-triangle',
  ambiguous: 'alert-triangle',
  missing: 'circle-x',
  unresolved_lld: 'circle-x',
  denied: 'circle-x'
};

export const MATCH_TONE: Readonly<Record<MatchStatus, Tone>> = {
  exact: 'success',
  compatible: 'warning',
  ambiguous: 'warning',
  missing: 'danger',
  unresolved_lld: 'danger',
  denied: 'danger'
};

/** Diff markers of P1-08B: Tabler icon + word. */
export const CHANGE_LABEL: Readonly<Record<ChangeState, LocalizedText>> = {
  changed: { id: 'bindings.change.changed' },
  untouched: { id: 'bindings.change.untouched' },
  unresolved: { id: 'bindings.change.unresolved' },
  kept_on_source: { id: 'bindings.change.kept_on_source' }
};

export const CHANGE_ICON: Readonly<Record<ChangeState, IconName>> = {
  changed: 'pencil',
  untouched: 'minus',
  unresolved: 'circle-x',
  kept_on_source: 'ban'
};

export const CHANGE_TONE: Readonly<Record<ChangeState, Tone>> = {
  changed: 'info',
  untouched: 'info',
  unresolved: 'danger',
  kept_on_source: 'warning'
};

export const FITNESS_LABEL: Readonly<Record<Applicability, LocalizedText>> = {
  editable: { id: 'bindings.fitness.editable' },
  read_only: { id: 'bindings.fitness.read_only' },
  unsupported: { id: 'bindings.fitness.unsupported' }
};

export const CHECK_LABEL: Readonly<Record<RowCheck, LocalizedText>> = {
  checked: { id: 'bindings.state.checked' },
  stale: { id: 'bindings.state.stale' },
  invalid: { id: 'bindings.state.invalid' }
};

export const REFERENCE_LABEL: Readonly<Record<ReferenceKind, LocalizedText>> = {
  item: { id: 'bindings.ref.by_id' },
  graph: { id: 'bindings.ref.graph_id' },
  host: { id: 'bindings.ref.host_id' },
  host_pattern: { id: 'bindings.ref.host_name_pattern' },
  host_mask: { id: 'bindings.ref.host_mask' }
};

/** How a source field value refers (FieldValue kind or unattributed kind). */
export function sourceKindText(kind: string): LocalizedText {
  switch (kind) {
    case 'item':
      return { id: 'bindings.ref.by_id' };
    case 'graph':
      return { id: 'bindings.ref.graph_id' };
    case 'host':
      return { id: 'bindings.ref.host_id' };
    case 'host_pattern':
      return { id: 'bindings.ref.host_name_pattern' };
    default:
      return { id: 'bindings.ref.prototype' };
  }
}

const REASONS = {
  not_found_or_no_access: 'bindings.reason.not_found_or_no_access',
  key_not_found: 'bindings.reason.key_not_found',
  lld_instance_missing: 'bindings.reason.lld_instance_missing',
  several_target_candidates: 'bindings.reason.several_target_candidates',
  value_type_incompatible: 'bindings.reason.value_type_incompatible',
  value_type_differs: 'bindings.reason.value_type_differs',
  units_differ: 'bindings.reason.units_differ',
  origin_differs: 'bindings.reason.origin_differs',
  pattern_coverage_differs: 'bindings.reason.pattern_coverage_differs',
  item_patterns_without_target_items: 'bindings.reason.item_patterns_without_target_items',
  target_name_is_mask: 'bindings.reason.target_name_is_mask',
  host_mask: 'bindings.reason.host_mask',
  host_wildcard_all: 'bindings.reason.host_wildcard_all',
  host_field_not_supported: 'bindings.reason.host_field_not_supported',
  pattern_field_not_supported: 'bindings.reason.pattern_field_not_supported',
  classic_graph_contract_pending: 'bindings.reason.classic_graph_contract_pending',
  graph_name_not_found: 'bindings.reason.graph_name_not_found',
  widget_not_editable: 'bindings.reason.widget_not_editable',
  field_not_replaceable: 'bindings.reason.field_not_replaceable',
  unknown_widget: 'bindings.reason.unknown_widget',
  unknown_field: 'bindings.reason.unknown_field',
  prototype_owner_unknown: 'bindings.reason.prototype_owner_unknown'
} as const;

/** Every resolver reason code (P2-02, 22 codes). */
export const REASON_CODES: ReadonlyArray<string> = Object.keys(REASONS);

export function reasonText(code: string): LocalizedText {
  if (code in REASONS) return { id: REASONS[code as keyof typeof REASONS] };
  return { id: 'bindings.reason.unknown', params: { code } };
}

const CHECK_REASONS = {
  server_version_changed: 'bindings.check_reason.server_version_changed',
  resolution_changed: 'bindings.check_reason.resolution_changed',
  reference_gone: 'bindings.check_reason.reference_gone',
  reference_added: 'bindings.check_reason.reference_added',
  target_not_visible: 'bindings.check_reason.target_not_visible',
  candidate_not_visible: 'bindings.check_reason.candidate_not_visible'
} as const;

export const CHECK_REASON_CODES: ReadonlyArray<string> = Object.keys(CHECK_REASONS);

export function checkReasonText(code: string): LocalizedText {
  if (code in CHECK_REASONS) return { id: CHECK_REASONS[code as keyof typeof CHECK_REASONS] };
  return { id: 'bindings.reason.unknown', params: { code } };
}

const DIFFERENCES = {
  value_type: 'bindings.difference.value_type',
  units: 'bindings.difference.units',
  origin: 'bindings.difference.origin',
  pattern_coverage: 'bindings.difference.pattern_coverage'
} as const;

/** «единицы измерения, происхождение из шаблона» — a localized list of difference codes. */
export function differencesText(i18n: I18n, codes: ReadonlyArray<string>): string {
  const words = codes.map((code) => (code in DIFFERENCES ? i18n.t(DIFFERENCES[code as keyof typeof DIFFERENCES]) : code));
  return new Intl.ListFormat(i18n.locale, { style: 'long', type: 'conjunction' }).format(words);
}

const VALUE_TYPES = ['bindings.value_type.float', 'bindings.value_type.character', 'bindings.value_type.log', 'bindings.value_type.unsigned', 'bindings.value_type.text', 'bindings.value_type.binary'] as const;

export function valueTypeText(i18n: I18n, valueType: number): string {
  const id = VALUE_TYPES[valueType];
  return id ? i18n.t(id) : String(valueType);
}

export const DECISION_LABEL: Readonly<Record<DecisionKind, LocalizedText>> = {
  choose_candidate: { id: 'bindings.decision.choose' },
  choose_lld_instance: { id: 'bindings.mapping.choose_lld_instance' },
  keep_on_source: { id: 'bindings.decision.keep_on_source' }
};

export const GROUP_LABEL: Readonly<Record<FindingGroup, LocalizedText>> = {
  connection: { id: 'publication.check.group.connection' },
  source: { id: 'publication.check.group.source' },
  bindings: { id: 'publication.check.group.bindings' },
  widgets: { id: 'publication.check.group.widgets' },
  plan: { id: 'publication.check.group.plan' }
};

/** Where a finding is fixed (P1-08C «исправляется: …»). */
export const GROUP_FIX_SECTION: Readonly<Record<FindingGroup, LocalizedText>> = {
  connection: { id: 'nav.connections' },
  source: { id: 'draft.nav.publication' },
  bindings: { id: 'draft.nav.bindings' },
  widgets: { id: 'draft.nav.bindings' },
  plan: { id: 'draft.nav.publication' }
};

const CONTRACTS = {
  update_one_page_with_full_fields: 'publication.contract.update_one_page_with_full_fields',
  preserve_untouched_pages: 'publication.contract.preserve_untouched_pages',
  preserve_widget_references: 'publication.contract.preserve_widget_references',
  reconcile_unknown_outcome: 'publication.contract.reconcile_unknown_outcome',
  item_widget_itemid: 'publication.contract.item_widget_itemid',
  svggraph_dataset_itemids: 'publication.contract.svggraph_dataset_itemids',
  svggraph_dataset_hosts: 'publication.contract.svggraph_dataset_hosts',
  update_several_pages: 'publication.contract.update_several_pages'
} as const;

export const FINDING_CODES: ReadonlyArray<string> = [
  'no_changes',
  'capability_not_verified',
  'connection_required',
  'write_mode_required',
  'server_version_mismatch',
  'write_contract_unverified',
  'target_host_not_visible',
  'binding_unresolved',
  'source_unverified',
  'decision_invalid',
  'binding_stale',
  'binding_invalid',
  'match_differences',
  'kept_on_source',
  'kept_on_source_unconfirmed',
  'kept_on_source_confirmation_stale',
  'references_outside_scope',
  'scope_link_crossing',
  'dashboard_not_editable'
];

/** Text of a publication finding. `host` is the replaced host (server data). */
export function findingText(i18n: I18n, finding: Finding, host: string): LocalizedText {
  const params = finding.params;
  switch (finding.code) {
    case 'no_changes':
      return { id: 'publication.finding.no_changes' };
    case 'capability_not_verified':
      return { id: 'publication.finding.capability_not_verified', params: { capability: params.capability ?? '' } };
    case 'connection_required':
      return { id: 'publication.finding.connection_required' };
    case 'write_mode_required':
      return { id: 'publication.finding.write_mode_required' };
    case 'server_version_mismatch':
      return { id: 'publication.finding.server_version_mismatch', params: { base_version: params.base_version ?? '' } };
    case 'write_contract_unverified': {
      const contract = params.contract ?? '';
      const text =
        contract === 'resend_widget_type'
          ? i18n.t('publication.contract.resend_widget_type', { widget_type: params.widget_type ?? '' })
          : contract in CONTRACTS
            ? i18n.t(CONTRACTS[contract as keyof typeof CONTRACTS])
            : contract;
      return { id: 'publication.finding.write_contract_unverified', params: { contract: text } };
    }
    case 'target_host_not_visible':
      return { id: 'publication.finding.target_host_not_visible' };
    case 'binding_unresolved':
      return { id: 'publication.finding.binding_unresolved' };
    case 'source_unverified':
      return { id: 'publication.finding.source_unverified' };
    case 'decision_invalid':
      return { id: 'publication.finding.decision_invalid' };
    case 'binding_stale':
      return { id: 'publication.finding.binding_stale' };
    case 'binding_invalid':
      return { id: 'publication.finding.binding_invalid' };
    case 'match_differences':
      return {
        id: 'publication.finding.match_differences',
        params: { differences: differencesText(i18n, (params.differences ?? '').split(',').filter((code) => code !== '')) }
      };
    case 'kept_on_source':
      return { id: 'publication.finding.kept_on_source', params: { host } };
    case 'kept_on_source_unconfirmed':
      return { id: 'publication.check.confirm_kept' };
    case 'kept_on_source_confirmation_stale':
      return { id: 'publication.finding.kept_on_source_confirmation_stale' };
    case 'references_outside_scope':
      return { id: 'bindings.scope.partial_warning', params: { host, count: Number(params.count ?? Number.NaN) } };
    case 'scope_link_crossing':
      return params.side === 'consumer_outside'
        ? { id: 'publication.finding.scope_link_crossing.consumer_outside' }
        : { id: 'publication.finding.scope_link_crossing.provider_outside' };
    case 'dashboard_not_editable':
      return { id: 'publication.finding.dashboard_not_editable' };
    default:
      return { id: 'publication.finding.unknown', params: { code: finding.code } };
  }
}

const PLAN_REASONS = {
  draft_changed: 'publication.plan.reason.draft_changed',
  target_changed: 'publication.plan.reason.target_changed',
  connection_changed: 'publication.plan.reason.connection_changed',
  bindings_changed: 'publication.plan.reason.bindings_changed',
  source_changed: 'publication.plan.reason.source_changed',
  app_restarted: 'publication.plan.reason.app_restarted',
  session_ended: 'publication.plan.reason.session_ended'
} as const;

/** Every reason a verified plan stops being valid (`PlanInvalidReason`, P1-01G §6). */
export const PLAN_REASON_CODES: ReadonlyArray<string> = Object.keys(PLAN_REASONS);

/** «The plan is no longer valid: {reason}. Build a new preview.» for a plan invalidation code. */
export function planInvalidText(i18n: I18n, code: string): LocalizedText {
  const reason =
    code in PLAN_REASONS
      ? i18n.t(PLAN_REASONS[code as keyof typeof PLAN_REASONS])
      : i18n.t('publication.plan.reason.unknown', { code });
  return { id: 'publication.plan.invalidated', params: { reason } };
}

const ERRORS = {
  replace_host_not_calculated: 'errors.replace_host.not_calculated',
  replace_host_same_host: 'errors.replace_host.same_host',
  replace_host_scope_invalid: 'errors.replace_host.scope_invalid',
  replace_host_scope_outside_draft: 'errors.replace_host.scope_outside_draft',
  replace_host_pending: 'errors.replace_host.pending',
  binding_not_found: 'errors.replace_host.binding_not_found',
  decision_not_applicable: 'errors.replace_host.decision_not_applicable',
  candidate_not_visible: 'errors.replace_host.candidate_not_visible',
  candidate_value_type_incompatible: 'errors.replace_host.candidate_value_type_incompatible',
  kept_on_source_empty: 'errors.replace_host.kept_empty',
  kept_on_source_list_changed: 'errors.replace_host.kept_changed',
  operation_not_found: 'errors.replace_host.operation_not_found',
  workcopy_changed_concurrently: 'errors.replace_host.changed_concurrently',
  source_host_not_found_or_no_access: 'errors.replace_host.source_not_visible',
  resolver_host_objects_limit: 'errors.replace_host.host_objects_limit',
  local_materialization_failed: 'errors.replace_host.internal',
  resolver_facts_mismatch: 'errors.replace_host.internal',
  draft_other_connection: 'errors.replace_host.other_connection',
  // Malformed IDs come only from a broken client, never from user input.
  resolver_hostid_invalid: 'errors.connection.request_invalid'
} as const;

/** Every code the ReplaceHost routes add to the workcopy and connection codes. */
export const REPLACE_HOST_ERROR_CODES: ReadonlyArray<string> = Object.keys(ERRORS);

/** User-facing text of a failed ReplaceHost command. */
export function replaceHostCodeText(code: string, params: Params = {}): LocalizedText {
  if (code in ERRORS) return { id: ERRORS[code as keyof typeof ERRORS] } as LocalizedText;
  return workcopyCodeText(code, params);
}

export const HISTORY_OP = {
  replace_host: 'bindings.history.op.replace_host',
  replace_host_decision: 'bindings.history.op.replace_host_decision',
  replace_host_confirm_kept: 'bindings.history.op.replace_host_confirm_kept'
} as const;

export function historyOpText(op: string): LocalizedText {
  if (op in HISTORY_OP) return { id: HISTORY_OP[op as keyof typeof HISTORY_OP] };
  return { id: 'bindings.history.op.unknown', params: { op } };
}
