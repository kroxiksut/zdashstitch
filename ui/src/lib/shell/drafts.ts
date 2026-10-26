// Typed client boundary for local drafts (P1-03H «Данные строки черновика»).
// The drafts/workcopy HTTP API does not exist yet: the production client reports
// `unavailable`, and tests use the in-memory client. Nothing here invents data.

import type { LocalizedText } from '../components';

export type DraftKind = 'dashboard' | 'page';
export type DraftOrigin = 'local' | 'server_copy';
export type DraftLocalState = 'saved' | 'dirty';
export type DraftFreshnessState = 'not_checked' | 'current' | 'stale' | 'conflict' | 'source_missing';

export interface DraftSource {
  /** Server data: the source dashboard name, shown as data, never translated. */
  readonly dashboard_name: string;
  readonly page_name?: string;
  /** Snapshot time, epoch milliseconds. */
  readonly snapshot_at: number;
}

export interface DraftFreshness {
  readonly state: DraftFreshnessState;
  /** Epoch milliseconds of the last check; only for `current`. */
  readonly checked_at?: number;
}

export interface DraftSummary {
  readonly local_id: string;
  readonly kind: DraftKind;
  readonly origin: DraftOrigin;
  /** User data. */
  readonly name: string;
  readonly source?: DraftSource;
  /** Only server copies have freshness. */
  readonly freshness?: DraftFreshness;
  readonly local_state: DraftLocalState;
  /** Epoch milliseconds. */
  readonly updated_at: number;
}

export interface DraftWidget {
  readonly id: string;
  /** User data; may be empty. */
  readonly name: string;
  /** Zabbix widget type identifier, a technical value. */
  readonly type: string;
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  /** `false` for types without a codec: «Editing not supported». */
  readonly editable: boolean;
}

export interface DraftPage {
  readonly id: string;
  /** User data; may be empty. */
  readonly name: string;
  /** Seconds; `null` means the dashboard default. */
  readonly display_period: number | null;
  readonly widgets: ReadonlyArray<DraftWidget>;
}

export type CheckGroup = 'structure' | 'widgets' | 'sources' | 'references' | 'compatibility';
export const CHECK_GROUPS: ReadonlyArray<CheckGroup> = ['structure', 'widgets', 'sources', 'references', 'compatibility'];

export interface DraftFinding {
  readonly id: string;
  readonly group: CheckGroup;
  readonly level: 'blocking' | 'warning';
  readonly blocks: ReadonlyArray<'export' | 'publication'>;
  readonly message: LocalizedText;
  /** User data: page and widget names for the object path. */
  readonly page?: string;
  readonly widget?: string;
}

export interface DraftChecks {
  readonly checked_objects: number;
  readonly passed: Readonly<Record<CheckGroup, number>>;
  readonly findings: ReadonlyArray<DraftFinding>;
}

export interface DraftDetail extends DraftSummary {
  readonly pages: ReadonlyArray<DraftPage>;
  /** Server data: the chosen target dashboard name, only when chosen. */
  readonly target?: { readonly dashboard_name: string };
  /** Computed compatibility (P0-09/O-029), only when computed. */
  readonly compatibility?: { readonly min_version: string };
  /** Result of the local check, only when it has run. */
  readonly checks?: DraftChecks;
}

export type DraftListResult =
  | { readonly status: 'unavailable' }
  | { readonly status: 'ready'; readonly drafts: ReadonlyArray<DraftSummary> };

export type DraftLoadResult =
  | { readonly status: 'unavailable' }
  | { readonly status: 'missing' }
  | { readonly status: 'ready'; readonly draft: DraftDetail };

export interface DraftsClient {
  list(): Promise<DraftListResult>;
  load(localId: string): Promise<DraftLoadResult>;
}

/** Production client until the drafts API exists: no data, an explicit `unavailable` state. */
export const unavailableDraftsClient: DraftsClient = {
  async list() {
    return { status: 'unavailable' };
  },
  async load() {
    return { status: 'unavailable' };
  }
};

function summaryOf(draft: DraftDetail): DraftSummary {
  const { pages: _pages, target: _target, compatibility: _compatibility, checks: _checks, ...summary } = draft;
  return summary;
}

/** In-memory client for tests and component development; never wired into the production build. */
export function createMemoryDraftsClient(drafts: ReadonlyArray<DraftDetail>): DraftsClient {
  const byId = new Map(drafts.map((draft) => [draft.local_id, draft]));
  return {
    async list() {
      return { status: 'ready', drafts: drafts.map(summaryOf) };
    },
    async load(localId) {
      const draft = byId.get(localId);
      return draft ? { status: 'ready', draft } : { status: 'missing' };
    }
  };
}

export type KindFilter = 'all' | DraftKind;
export type OriginFilter = 'any' | DraftOrigin;

/** Newest first, as on «Drafts» (P1-03F) and «Recent» (P1-01A). */
export function sortByUpdated<T extends Pick<DraftSummary, 'updated_at' | 'local_id'>>(drafts: ReadonlyArray<T>): T[] {
  return [...drafts].sort((left, right) => right.updated_at - left.updated_at || left.local_id.localeCompare(right.local_id));
}

export function filterDrafts<T extends Pick<DraftSummary, 'kind' | 'origin'>>(
  drafts: ReadonlyArray<T>,
  kind: KindFilter,
  origin: OriginFilter
): T[] {
  return drafts.filter((draft) => (kind === 'all' || draft.kind === kind) && (origin === 'any' || draft.origin === origin));
}

export function kindCounts(drafts: ReadonlyArray<Pick<DraftSummary, 'kind'>>): Record<KindFilter, number> {
  return {
    all: drafts.length,
    dashboard: drafts.filter((draft) => draft.kind === 'dashboard').length,
    page: drafts.filter((draft) => draft.kind === 'page').length
  };
}

export const RECENT_LIMIT = 5;

export function recentDrafts<T extends Pick<DraftSummary, 'updated_at' | 'local_id'>>(drafts: ReadonlyArray<T>): T[] {
  return sortByUpdated(drafts).slice(0, RECENT_LIMIT);
}

export function widgetCount(pages: ReadonlyArray<DraftPage>): number {
  return pages.reduce((total, page) => total + page.widgets.length, 0);
}

/** ISO timestamp for `<time datetime>`, or undefined for an invalid value. */
export function isoTime(epochMs: number): string | undefined {
  return Number.isFinite(epochMs) ? new Date(epochMs).toISOString() : undefined;
}
