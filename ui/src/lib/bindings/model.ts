// Data model of «Привязки» for ReplaceHost (P1-05, P2-03E) and the preview «база → черновик»
// (P1-08B/C, P2-03D). Mirrors the local server routes `/api/drafts/:id/replace-host/*`: codes,
// safe parameters and server data only, never prose. Times are epoch milliseconds parsed at the
// client boundary from RFC 3339 UTC strings (O-035). Zabbix IDs are used in API bodies only, never
// in interface routes or as visible text (technical details aside).
//
// The model is written for ReplaceHost (one source host → one target host). Clone (P3) extends it
// with several targets and slots; the row, decision and finding shapes stay the same.

import type { LocalizedText } from '../components';

export type MatchStatus = 'exact' | 'compatible' | 'ambiguous' | 'missing' | 'unresolved_lld' | 'denied';
export type Applicability = 'editable' | 'read_only' | 'unsupported';
export type ChangeState = 'changed' | 'untouched' | 'unresolved' | 'kept_on_source';
export type RowCheck = 'checked' | 'stale' | 'invalid';
export type DecisionState = 'none' | 'applied' | 'invalid';
export type DecisionKind = 'choose_candidate' | 'choose_lld_instance' | 'keep_on_source';

export const MATCH_STATUSES: ReadonlyArray<MatchStatus> = ['exact', 'compatible', 'ambiguous', 'missing', 'unresolved_lld', 'denied'];
export const CHANGE_STATES: ReadonlyArray<ChangeState> = ['changed', 'untouched', 'unresolved', 'kept_on_source'];

export type FieldValue =
  | { readonly kind: 'item'; readonly itemid: string }
  | { readonly kind: 'graph'; readonly graphid: string }
  | { readonly kind: 'host'; readonly hostid: string }
  | { readonly kind: 'host_pattern'; readonly pattern: string }
  | { readonly kind: 'item_prototype'; readonly itemid: string }
  | { readonly kind: 'graph_prototype'; readonly graphid: string };

/** Address of one widget field in Base: the key of rows and decisions. */
export interface FieldKey {
  readonly page_index: number;
  readonly widget_index: number;
  /** Zabbix field name, e.g. `itemid.0`, `ds.0.hosts.0`; a technical value. */
  readonly field: string;
}

/** Item evidence (P1-05D): key, value type, units, template origin. Server data. */
export interface ItemFacts {
  readonly itemid: string;
  readonly key: string;
  readonly name: string;
  /** 0 float, 1 character, 2 log, 3 unsigned, 4 text, 5 binary. */
  readonly value_type: number;
  readonly units: string;
  readonly template_itemid: string | null;
  readonly prototype_key: string | null;
}

export interface PatternCount {
  readonly pattern: string;
  readonly source_matches: number;
  readonly target_matches: number;
}

export type RowDecision =
  | { readonly kind: 'choose_candidate'; readonly itemid: string }
  | { readonly kind: 'choose_lld_instance'; readonly itemid: string }
  | { readonly kind: 'keep_on_source' };

/** One reference of the replaced host in scope (09-03 row). */
export interface BindingRow {
  readonly key: FieldKey;
  readonly dashboard_pageid: string | null;
  readonly widgetid: string | null;
  readonly widget_type: string;
  readonly source: FieldValue;
  /** Effective target; only for `changed`. */
  readonly target: FieldValue | null;
  /** `null` for unverified references. */
  readonly match: MatchStatus | null;
  readonly applicability: Applicability | null;
  readonly reasons: ReadonlyArray<string>;
  readonly differences: ReadonlyArray<string>;
  readonly source_item: ItemFacts | null;
  readonly target_item: ItemFacts | null;
  readonly candidates: ReadonlyArray<ItemFacts>;
  readonly pattern_coverage: ReadonlyArray<PatternCount>;
  readonly state: ChangeState;
  readonly kept_reason: string | null;
  readonly decision: RowDecision | null;
  /** Operation `seq` of the decision, for undo. */
  readonly decision_seq: number | null;
  readonly decision_state: DecisionState;
  readonly allowed_decisions: ReadonlyArray<DecisionKind>;
  readonly unverified: boolean;
  readonly check: RowCheck;
  readonly check_reason: string | null;
}

export interface FieldCounts {
  readonly changed: number;
  readonly unresolved: number;
  readonly kept_on_source: number;
}

export interface StateCounts {
  readonly total: number;
  readonly changed: number;
  readonly untouched: number;
  readonly unresolved: number;
  readonly kept_on_source: number;
}

export interface SeriesDiff {
  readonly dataset_index: number;
  readonly state: ChangeState;
  readonly fields: FieldCounts;
  readonly field_names: ReadonlyArray<string>;
  /** Base values (server data). */
  readonly host_patterns: ReadonlyArray<string>;
  readonly item_patterns: ReadonlyArray<string>;
  readonly itemids: ReadonlyArray<string>;
}

export interface WidgetDiff {
  readonly widget_index: number;
  readonly widgetid: string | null;
  readonly widget_type: string;
  /** Server data; may be empty. */
  readonly name: string;
  readonly in_scope: boolean;
  readonly state: ChangeState;
  readonly fields: FieldCounts;
  /** Field names of this widget's rows. */
  readonly row_fields: ReadonlyArray<string>;
  readonly series: ReadonlyArray<SeriesDiff>;
}

export interface PageDiff {
  readonly page_index: number;
  readonly dashboard_pageid: string | null;
  /** Server data; may be empty. */
  readonly name: string;
  readonly state: ChangeState;
  readonly widget_counts: StateCounts;
  readonly widgets: ReadonlyArray<WidgetDiff>;
}

export interface StructuralDiff {
  readonly pages: ReadonlyArray<PageDiff>;
  readonly summary: {
    readonly pages: StateCounts;
    readonly widgets: StateCounts;
    readonly series: StateCounts;
    readonly fields: FieldCounts;
  };
}

/** One entry of «останутся на старом узле» (O-003). */
export interface KeptEntry {
  readonly page_index: number;
  readonly widget_index: number;
  readonly widgetid: string | null;
  readonly field: string;
  readonly widget_type: string;
  readonly source: FieldValue;
  readonly reason: string | null;
  readonly by_decision: boolean;
}

export interface KeptList {
  readonly entries: ReadonlyArray<KeptEntry>;
  /** Pass back to confirm exactly this list. */
  readonly digest: string;
  readonly confirmed: boolean;
  readonly confirmation_seq: number | null;
}

export interface FieldChange {
  readonly key: FieldKey;
  readonly widgetid: string | null;
  readonly widget_type: string;
  readonly source: FieldValue;
  readonly target: FieldValue;
}

export type FindingSeverity = 'blocks_publication' | 'informational';
export type FindingGroup = 'connection' | 'source' | 'bindings' | 'widgets' | 'plan';
export const FINDING_GROUPS: ReadonlyArray<FindingGroup> = ['connection', 'source', 'bindings', 'widgets', 'plan'];

export interface Finding {
  readonly code: string;
  readonly severity: FindingSeverity;
  readonly group: FindingGroup;
  readonly subject: {
    readonly page_index: number;
    readonly widget_index: number;
    readonly widgetid: string | null;
    readonly field: string | null;
  } | null;
  readonly reason: string | null;
  readonly params: Readonly<Record<string, string>>;
}

export interface CapabilityCheck {
  readonly name: string;
  readonly state: 'supported' | 'experimental' | 'unsupported';
  readonly blocking: boolean;
}

export interface CompatibilityReport {
  readonly allows_publication: boolean;
  readonly checks: ReadonlyArray<CapabilityCheck>;
  readonly findings: ReadonlyArray<Finding>;
}

export type ReplaceScope =
  | { readonly kind: 'dashboard' }
  | { readonly kind: 'pages'; readonly dashboard_pageids: ReadonlyArray<string> }
  | { readonly kind: 'widgets'; readonly widgetids: ReadonlyArray<string> };

export interface HostRef {
  readonly hostid: string;
  /** Server data: visible name. */
  readonly name: string;
}

export interface ReplaceHostOperation {
  readonly seq: number;
  readonly recorded_at: number;
  readonly source_host: HostRef;
  /** `null`: the new host was missing or hidden at calculation time. */
  readonly target_host: HostRef | null;
  readonly scope: ReplaceScope;
  readonly calculated_at: number;
  readonly server_version: string;
}

/** Everything the screens need about the ReplaceHost operation of one draft. */
export interface ReplaceHostPreview {
  readonly local_id: string;
  readonly revision: number;
  readonly kind: 'dashboard' | 'page';
  readonly operation: ReplaceHostOperation;
  readonly rows: ReadonlyArray<BindingRow>;
  readonly diff: StructuralDiff;
  readonly kept_on_source: KeptList;
  readonly changes: ReadonlyArray<FieldChange>;
  /** Technical details. */
  readonly base_fingerprint: string;
  readonly local_fingerprint: string;
  /** Epoch milliseconds of the last «Проверить все»; `null` when not rechecked in this answer. */
  readonly checked_at: number | null;
  readonly report: CompatibilityReport;
}

export type ReferenceKind = 'item' | 'graph' | 'host' | 'host_pattern' | 'host_mask';
export type ExpectedOutcome = 'changes' | 'kept_on_source';

export interface InventoryWidget {
  readonly widget_index: number;
  readonly widgetid: string | null;
  readonly name: string;
  readonly type: string;
}

export interface InventoryPage {
  readonly page_index: number;
  readonly dashboard_pageid: string | null;
  readonly name: string;
  readonly widgets: ReadonlyArray<InventoryWidget>;
}

export interface HostReference {
  readonly page_index: number;
  readonly widget_index: number;
  readonly field: string;
  readonly widget_type: string;
  readonly kind: ReferenceKind;
  readonly outcome: ExpectedOutcome;
  readonly reason: string | null;
}

export interface InventoryHost {
  readonly hostid: string;
  readonly name: string;
  readonly host: string;
  readonly widgets: number;
  readonly references: ReadonlyArray<HostReference>;
}

export interface UnattributedReference {
  readonly page_index: number;
  readonly widget_index: number;
  readonly field: string;
  readonly widget_type: string;
  readonly kind: string;
}

/** 09-01: the draft scope and the hosts it references. */
export interface ReplaceHostInventory {
  readonly local_id: string;
  readonly kind: 'dashboard' | 'page';
  readonly pages: ReadonlyArray<InventoryPage>;
  readonly hosts: ReadonlyArray<InventoryHost>;
  readonly unattributed: ReadonlyArray<UnattributedReference>;
}

export interface TargetHostCandidate {
  readonly hostid: string;
  readonly name: string;
  readonly host: string;
  readonly groups: ReadonlyArray<string>;
  readonly templates: ReadonlyArray<string>;
  readonly source_templates_found: number;
}

export interface TargetHostSearch {
  readonly hosts: ReadonlyArray<TargetHostCandidate>;
  readonly truncated: boolean;
  /** `null` when the replaced host is not visible. */
  readonly source_templates: ReadonlyArray<string> | null;
  readonly empty_reason: 'not_visible_or_denied' | null;
}

export interface CalculateRequest {
  readonly source_hostid: string;
  readonly target_hostid: string;
  readonly scope: ReplaceScope;
}

export interface OperationRecord {
  readonly seq: number;
  readonly op: string;
  readonly op_version: number;
  readonly recorded_at: number;
  readonly replace_seq: number | null;
}

export interface RemoveResult {
  readonly removed: ReadonlyArray<number>;
  /** `null` when no operation is left. */
  readonly preview: ReplaceHostPreview | null;
}

/** One step of a step bar (bindings wizard P1-05, publication path P1-08A). */
export interface StepItem {
  readonly id: string;
  readonly label: LocalizedText;
  /** `error`: the step found a conflict or a blocker (P1-08A: red icon, mockup 11-05). */
  readonly state: 'done' | 'current' | 'todo' | 'unavailable' | 'error';
}

// ---------------------------------------------------------------- helpers

export function sameKey(left: FieldKey, right: FieldKey): boolean {
  return left.page_index === right.page_index && left.widget_index === right.widget_index && left.field === right.field;
}

export function keyId(key: FieldKey): string {
  return `${key.page_index}:${key.widget_index}:${key.field}`;
}

export function rowsOfWidget(preview: ReplaceHostPreview, pageIndex: number, widgetIndex: number): BindingRow[] {
  return preview.rows.filter((row) => row.key.page_index === pageIndex && row.key.widget_index === widgetIndex);
}

/** References of `host` inside and outside the chosen scope (P1-05A partial-scope warning). */
export function referencesInScope(
  host: InventoryHost,
  pages: ReadonlyArray<InventoryPage>,
  scope: ReplaceScope
): { inside: HostReference[]; outside: number } {
  const inScope = (reference: HostReference): boolean => {
    if (scope.kind === 'dashboard') return true;
    const page = pages.find((candidate) => candidate.page_index === reference.page_index);
    if (scope.kind === 'pages') return page?.dashboard_pageid != null && scope.dashboard_pageids.includes(page.dashboard_pageid);
    const widget = page?.widgets.find((candidate) => candidate.widget_index === reference.widget_index);
    return widget?.widgetid != null && scope.widgetids.includes(widget.widgetid);
  };
  const inside = host.references.filter(inScope);
  return { inside, outside: host.references.length - inside.length };
}

/** Rows by match status for the 09-03 filters; unverified rows count as `denied`-like «не найдено». */
export function matchCounts(rows: ReadonlyArray<BindingRow>): Record<MatchStatus | 'all' | 'kept_on_source', number> {
  const counts = { all: rows.length, exact: 0, compatible: 0, ambiguous: 0, missing: 0, unresolved_lld: 0, denied: 0, kept_on_source: 0 };
  for (const row of rows) {
    if (row.state === 'kept_on_source') counts.kept_on_source += 1;
    else if (row.match) counts[row.match] += 1;
    else counts.denied += 1;
  }
  return counts;
}

export function blockingFindings(report: CompatibilityReport): Finding[] {
  return report.findings.filter((finding) => finding.severity === 'blocks_publication');
}

export function informationalFindings(report: CompatibilityReport): Finding[] {
  return report.findings.filter((finding) => finding.severity === 'informational');
}
