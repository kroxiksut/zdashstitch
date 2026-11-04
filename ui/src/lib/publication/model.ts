// Data of the publication screens (P1-08D–G, P1-01G §4/§7, P2-04E/F). Everything here comes from
// the local server as codes, counts, fingerprints and server/user data; the browser never sees a
// plan, a payload or Zabbix JSON-RPC. Times are epoch milliseconds (parsed from RFC 3339, O-035).

import type { CompatibilityReport, ReplaceHostPreview } from '../bindings/model';

/** P1-08E: the action is always chosen explicitly; there is no default. */
export type PublicationAction = 'update_source' | 'save_as_copy';

export type FreshnessState = 'current' | 'conflict' | 'source_missing' | 'stale';

export interface SourceCheck {
  readonly state: FreshnessState;
  /** Epoch milliseconds; only for `current`. */
  readonly checked_at: number;
  /** Technical details. */
  readonly base_fingerprint: string;
  readonly remote_fingerprint: string | null;
  /** `publication.plan.reason.*` code when Remote differs from Base. */
  readonly invalidated: string | null;
}

export type RemoteChangeKind =
  | 'dashboard_properties'
  | 'access'
  | 'page_added'
  | 'page_removed'
  | 'page_renamed'
  | 'page_changed'
  | 'widget_added'
  | 'widget_removed'
  | 'widget_changed'
  | 'widget_moved';

/** One row of «На сервере: база → Zabbix сейчас». Names are server data. */
export interface RemoteChange {
  readonly kind: RemoteChangeKind;
  readonly page_name: string | null;
  readonly page_number: number | null;
  readonly widget_name: string | null;
  readonly widget_type: string | null;
  readonly widgetid: string | null;
  /** The widget is also changed by this draft. */
  readonly overlaps: boolean;
}

export interface RemoteDiff {
  readonly missing: boolean;
  readonly changes: ReadonlyArray<RemoteChange>;
  readonly truncated: number;
  readonly overlapping_widgets: number;
}

export interface CodeWithParams {
  readonly code: string;
  readonly params: Readonly<Record<string, string>>;
}

/** `publication.recheck`: fresh read of the source and of the replacement. */
export interface CheckResult {
  readonly source: SourceCheck;
  readonly preview: ReplaceHostPreview;
  readonly remote: RemoteDiff | null;
  readonly mode: 'read_only' | 'read_write';
  /** An unfinished operation or a damaged journal file blocks this dashboard. */
  readonly journal_block: CodeWithParams | null;
}

export type PlanRequest =
  | {
      readonly action: 'update_source';
      readonly revision: number;
      readonly base_fingerprint: string;
      readonly local_fingerprint: string;
      readonly kept_digest: string | null;
    }
  | {
      readonly action: 'save_as_copy';
      readonly revision: number;
      readonly base_fingerprint: string;
      readonly local_fingerprint: string;
      /** User data: the new dashboard name. */
      readonly name: string;
    };

export interface FieldKindCount {
  readonly contract: string;
  readonly count: number;
}

/** What the confirmation dialog shows (P1-08F); never a payload. */
export type PlanPreview =
  | {
      readonly action: 'update_source';
      readonly plan_id: string;
      readonly write_requests: number;
      readonly created_at: number;
      readonly dashboardid: string;
      readonly replacement_count: number;
      readonly changed_pages: ReadonlyArray<number>;
      readonly contracts: ReadonlyArray<string>;
      readonly replacement_kinds: ReadonlyArray<FieldKindCount>;
      readonly revision: number | null;
      readonly kept_digest: string | null;
      readonly source_fingerprint: string;
      readonly expected_fingerprint: string;
      readonly report: CompatibilityReport;
    }
  | {
      readonly action: 'save_as_copy';
      readonly plan_id: string;
      readonly write_requests: number;
      readonly created_at: number;
      readonly target_name: string;
      readonly private: boolean;
      readonly page_count: number;
      readonly widget_count: number;
      readonly unknown_widgets: number;
      readonly contracts: ReadonlyArray<string>;
      /** `not_visible`: free among visible dashboards — a hint, never a proof. */
      readonly name_check: string;
      readonly revision: number;
      readonly local_fingerprint: string;
      readonly content_fingerprint: string;
      readonly report: CompatibilityReport;
    };

export interface PlanResult {
  /** Opaque handle: the only reference to the plan the UI holds. */
  readonly handle: string;
  readonly plan: PlanPreview;
}

export interface ConfirmRequest {
  readonly handle: string;
  readonly plan_id: string;
  readonly confirmed: true;
  readonly kept_digest?: string | null;
  readonly name?: string;
}

export type JournalState =
  | 'confirmed'
  | 'submitting'
  | 'verified'
  | 'conflict'
  | 'failed_before_send'
  | 'failed_after_response'
  | 'unknown_outcome'
  | 'reconciled_verified'
  | 'reconciled_failed'
  | 'manual_check_required'
  | 'closed_manually';

export type JournalProgress = 'running' | 'needs_reconciliation' | 'abandoned' | 'finished';

export interface JournalTarget {
  readonly kind: 'existing_dashboard' | 'new_dashboard';
  /** Zabbix ID: used only for «Открыть в Zabbix». */
  readonly dashboardid: string | null;
  /** Server or user data. */
  readonly name: string | null;
}

export interface JournalEntry {
  readonly operation_id: string;
  readonly kind: 'update' | 'create';
  readonly intent: string;
  readonly connection_local_id: string;
  readonly server_version: string;
  readonly target: JournalTarget;
  readonly source: { readonly local_id: string; readonly revision: number } | null;
  readonly state: JournalState;
  readonly progress: JournalProgress;
  readonly sends: number;
  readonly transitions: ReadonlyArray<{ readonly state: JournalState; readonly at: number }>;
  readonly plan_created_at: number;
  readonly confirmed_at: number;
  readonly submitted_at: number | null;
  readonly evidence: string | null;
  readonly created_dashboardid: string | null;
  readonly reported_dashboardid: string | null;
  readonly closed_by: string | null;
  readonly source_fingerprint: string;
  readonly expected_fingerprint: string;
  readonly observed_fingerprint: string | null;
}

export interface DamagedJournal {
  readonly operation_id: string;
  readonly code: string;
  readonly connection_local_id: string | null;
  readonly target: JournalTarget | null;
}

export interface JournalList {
  readonly unfinished: ReadonlyArray<JournalEntry>;
  readonly damaged: ReadonlyArray<DamagedJournal>;
}

/** Result of a write or of a reconciliation (P1-08G). */
export interface Outcome {
  readonly operation_id: string;
  readonly kind: 'update' | 'create';
  readonly state: JournalState;
  readonly sent: boolean;
  readonly invalidated: string | null;
  readonly journal_error: CodeWithParams | null;
  readonly evidence: string | null;
  readonly created_dashboardid: string | null;
  /** User data: the new dashboard name of a copy. */
  readonly target_name: string | null;
  readonly base_advanced: { readonly revision: number; readonly base_fingerprint: string; readonly read_at: number } | null;
  readonly base_advance_error: CodeWithParams | null;
  readonly record_error: CodeWithParams | null;
  readonly entry: JournalEntry | null;
}

export interface RefetchResult {
  readonly local_id: string;
  readonly kind: 'dashboard' | 'page';
  /** User data. */
  readonly name: string;
  readonly replayed: boolean;
  readonly replay_code: string | null;
}

/** A refused command: code, safe parameters and, for blockers, the report. */
export interface PublicationFailure extends CodeWithParams {
  readonly status: number;
  readonly report?: CompatibilityReport;
}

/** States after which the target is free again and a new plan may be built. */
export const FINAL_STATES: ReadonlySet<JournalState> = new Set([
  'verified',
  'conflict',
  'failed_before_send',
  'reconciled_verified',
  'reconciled_failed',
  'closed_manually'
]);

/** States that need «Проверить результат» before any new write to the target. */
export const NEEDS_RECONCILIATION: ReadonlySet<JournalState> = new Set([
  'submitting',
  'unknown_outcome',
  'failed_after_response',
  'manual_check_required'
]);

/** «Открыть в Zabbix»: the dashboard page of the profile's frontend; `null` without a usable address. */
export function zabbixDashboardUrl(endpoint: string | null | undefined, dashboardid: string | null): string | null {
  if (!endpoint || dashboardid === null || !/^[1-9][0-9]{0,19}$/.test(dashboardid)) return null;
  let url: URL;
  try {
    url = new URL(endpoint);
  } catch {
    return null;
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return null;
  url.pathname = url.pathname.replace(/api_jsonrpc\.php$/, '').replace(/\/?$/, '/') + 'zabbix.php';
  url.search = `?action=dashboard.view&dashboardid=${dashboardid}`;
  url.hash = '';
  url.username = '';
  url.password = '';
  return url.toString();
}
