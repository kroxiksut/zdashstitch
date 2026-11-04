import { DATE_TIME_FORMAT, type I18n } from '../i18n';
import { resolveText } from '../components';
import type { CreatedDashboard, DraftDetail, DraftSource, DraftSummary, DraftWidget, LocalProvenance } from './drafts';
import { searchKey } from './drafts';
import {
  freshnessLabel,
  LIST_STATE_LABEL,
  listFreshnessLabel,
  LOCAL_STATE_LABEL,
  widgetTypeLabel
} from './labels';
import { periodUnit } from './preview';

/** The single date-and-time format of the interface (`datetime` placeholders). */
export const SHORT_DATE_TIME: Intl.DateTimeFormatOptions = DATE_TIME_FORMAT;

// Values below are data lists (names, times), not sentences: each localized part comes from a message ID.
const SEPARATOR = ' · ';

/** Source dashboard, optional page and snapshot time; server data is shown as is. */
export function sourceText(i18n: I18n, source: DraftSource): string {
  const object = source.page_name ? `${source.dashboard_name} / ${source.page_name}` : source.dashboard_name;
  return [object, i18n.date(source.snapshot_at, SHORT_DATE_TIME)].join(SEPARATOR);
}

/** Local state and, for server copies, snapshot freshness: short forms of a «Drafts» row. */
export function stateText(i18n: I18n, draft: Pick<DraftSummary, 'local_state' | 'freshness'>): string {
  const parts = [resolveText(i18n, LIST_STATE_LABEL[draft.local_state])];
  if (draft.freshness) parts.push(resolveText(i18n, listFreshnessLabel(draft.freshness)));
  return parts.join(SEPARATOR);
}

/**
 * «Built from N pages» or «Built from N pages: sources». An optional part of a phrase is a
 * separate message ID, not an empty branch (O-035).
 */
export function assembledText(i18n: I18n, count: number, sources: ReadonlyArray<string>): string {
  return sources.length > 0
    ? i18n.t('draft.provenance.assembled_with_sources', { count, sources: sources.join(', ') })
    : i18n.t('draft.provenance.assembled', { count });
}

/** Where a local draft came from; `withSources` adds the source names of an assembled dashboard. */
export function localProvenanceText(i18n: I18n, provenance: LocalProvenance, withSources = false): string {
  switch (provenance.kind) {
    case 'created_here':
      return i18n.t('draft.provenance.created_here');
    case 'imported_file':
      return i18n.t('draft.provenance.imported_file');
    case 'page_file':
      return i18n.t('draft.provenance.page_file');
    case 'legacy_file':
      return i18n.t('draft.provenance.legacy_file');
    case 'assembled':
      return assembledText(i18n, provenance.page_count, withSources ? provenance.sources : []);
  }
}

/** Snapshot of a server copy: «connection · time», or the source object when the profile is unknown. */
function snapshotText(i18n: I18n, source: DraftSource): string {
  return source.connection_name
    ? i18n.t('draft.provenance.snapshot', { connection: source.connection_name, time: source.snapshot_at })
    : sourceText(i18n, source);
}

type OriginDraft = Pick<DraftSummary, 'kind' | 'origin' | 'source' | 'provenance'>;

/** «Source» cell of «Drafts» (mockup 02-01); `—` when the storage does not know the origin. */
export function listSourceText(i18n: I18n, draft: OriginDraft): string {
  if (draft.origin === 'local') return draft.provenance ? localProvenanceText(i18n, draft.provenance) : '—';
  const source = draft.source;
  if (!source) return '—';
  if (draft.kind === 'page' && source.page_number !== undefined) {
    return i18n.t('draft.provenance.page_of', { dashboard: source.dashboard_name, page: source.page_number });
  }
  return snapshotText(i18n, source);
}

/** Origin line of a «Recent» entry (mockup 01-01): origin and the notes about local state. */
export function recentOriginText(
  i18n: I18n,
  draft: OriginDraft & Pick<DraftSummary, 'local_state' | 'last_published_at'>
): string {
  const parts: string[] = [];
  if (draft.origin === 'local') {
    if (draft.provenance) parts.push(localProvenanceText(i18n, draft.provenance, true));
  } else if (draft.source) {
    parts.push(
      draft.kind === 'page'
        ? i18n.t('draft.provenance.taken_from', { dashboard: draft.source.dashboard_name })
        : snapshotText(i18n, draft.source)
    );
  }
  if (draft.local_state === 'dirty') parts.push(i18n.t('draft.state.modified_locally'));
  if (draft.origin === 'local' && draft.last_published_at === undefined) parts.push(i18n.t('draft.state.never_published'));
  return parts.join(SEPARATOR);
}

/** Second line of the workspace «Origin» block (mockups 07-01, 08-01). */
export function originSummaryText(i18n: I18n, draft: OriginDraft): string | null {
  if (draft.origin === 'server_copy') {
    return draft.source ? i18n.t('draft.overview.snapshot_from', { date: draft.source.snapshot_at }) : null;
  }
  return draft.provenance ? localProvenanceText(i18n, draft.provenance) : null;
}

export interface DetailRow {
  readonly term: string;
  readonly value: string;
}

/** Rows of the «Drafts» details column (mockup 02-01, P1-03F). Absent data is left out. */
export function detailRows(i18n: I18n, draft: DraftSummary): DetailRow[] {
  const rows: DetailRow[] = [];
  const source = draft.source;
  if (draft.origin === 'local') {
    if (draft.provenance) {
      rows.push({ term: i18n.t('drafts.details.source'), value: localProvenanceText(i18n, draft.provenance, true) });
    }
  } else if (source) {
    const object =
      draft.kind === 'page' && source.page_number !== undefined
        ? i18n.t('draft.provenance.page_of', { dashboard: source.dashboard_name, page: source.page_number })
        : source.page_name
          ? `${source.dashboard_name} / ${source.page_name}`
          : source.dashboard_name;
    rows.push({ term: i18n.t('drafts.details.source'), value: object });
    if (source.connection_name) {
      const connection = [source.connection_name];
      if (source.server_version) connection.push(i18n.t('connection.result.version', { version: source.server_version }));
      rows.push({ term: i18n.t('drafts.details.connection'), value: connection.join(SEPARATOR) });
    }
    rows.push({ term: i18n.t('drafts.details.base'), value: i18n.t('drafts.details.base_snapshot', { time: source.snapshot_at }) });
  }
  if (draft.freshness) {
    const freshness = draft.freshness;
    rows.push({
      term: i18n.t('drafts.details.freshness'),
      value:
        freshness.state === 'conflict' && freshness.changed_at !== undefined
          ? i18n.t('drafts.details.changed_on_server_at', { time: freshness.changed_at })
          : resolveText(i18n, freshnessLabel(freshness))
    });
  }
  const local = [resolveText(i18n, LOCAL_STATE_LABEL[draft.local_state])];
  if (draft.counts) {
    local.push(i18n.t('dashboards.summary.pages_widgets', { pages: draft.counts.pages, widgets: draft.counts.widgets }));
  }
  rows.push({ term: i18n.t('drafts.details.local'), value: local.join(SEPARATOR) });
  return rows;
}

/** One «Created from this copy» entry: the name and time, then the read-back state. */
export function createdDashboardLines(i18n: I18n, created: CreatedDashboard): [string, string] {
  const state = i18n.t(created.state === 'verified' ? 'common.state.verified' : 'drafts.details.created_not_found');
  return [
    i18n.t('drafts.details.created_item', { name: created.name, time: created.created_at }),
    created.connection_name ? [state, created.connection_name].join(SEPARATOR) : state
  ];
}

/** Display name of a widget type; unknown types keep the technical identifier. */
export function widgetTypeName(i18n: I18n, type: string): string {
  const label = widgetTypeLabel(type);
  return label ? resolveText(i18n, label) : type;
}

export function widgetGeometry(i18n: I18n, widget: DraftWidget): string {
  return i18n.t('draft.structure.widget_geometry', {
    type: widgetTypeName(i18n, widget.type),
    x: widget.x,
    y: widget.y,
    width: widget.width,
    height: widget.height
  });
}

/** Widget type and size for «Overview» (mockup 07-01); the position lives in «Structure». */
export function widgetSize(i18n: I18n, widget: DraftWidget): string {
  return i18n.t('draft.overview.widget_size', {
    type: widgetTypeName(i18n, widget.type),
    width: widget.width,
    height: widget.height
  });
}

/** Page display period: the dashboard default or a locale-formatted duration. */
export function periodText(i18n: I18n, seconds: number | null): string {
  if (seconds === null) return i18n.t('draft.structure.page_period_default');
  return durationText(i18n, seconds);
}

/** Duration in the largest whole unit (Zabbix page periods: 10 s … 1 h). */
export function durationText(i18n: I18n, seconds: number): string {
  const { value, unit } = periodUnit(seconds);
  if (unit === 'hour') return i18n.t('common.duration.hours', { count: value });
  if (unit === 'minute') return i18n.t('common.duration.minutes', { count: value });
  return i18n.t('common.duration.seconds', { count: value });
}

/** «Default page display period: 30 seconds · slideshow on» of the dashboard node (mockup 08-01). */
export function dashboardPropertiesText(i18n: I18n, properties: NonNullable<DraftDetail['properties']>): string {
  return i18n.t('draft.structure.dashboard_properties_summary', {
    period: durationText(i18n, properties.display_period),
    slideshow: i18n.t(properties.auto_start ? 'draft.structure.slideshow.on' : 'draft.structure.slideshow.off')
  });
}

/**
 * Page name; an empty user name is shown as «Page {number}», as Zabbix does (1-based position).
 * Without a known position an empty name falls back to the neutral mark.
 */
export function pageName(i18n: I18n, name: string, number?: number): string {
  if (name.trim() !== '') return name;
  return number === undefined ? displayName(name) : i18n.t('object.page.unnamed', { number });
}

/** «2. Databases»; an unnamed page is shown only as «Page 2», without repeating the number. */
export function numberedPageName(i18n: I18n, name: string, number: number): string {
  return name.trim() === ''
    ? i18n.t('object.page.unnamed', { number })
    : i18n.t('object.page.numbered', { number, name });
}

/** Display name of a page or widget; an empty user name falls back to a neutral mark. */
export function displayName(name: string, fallback = '—'): string {
  return name.trim() === '' ? fallback : name;
}

/**
 * «(connection)» after the name of a draft whose name another draft shares (P2-08A); empty for a
 * unique name, a local draft or a connection profile that is no longer known.
 */
export function sourceConnectionSuffix(
  i18n: I18n,
  draft: Pick<DraftSummary, 'name' | 'origin' | 'source'>,
  duplicates: ReadonlySet<string>
): string {
  const connection = draft.source?.connection_name;
  if (draft.origin !== 'server_copy' || !connection || !duplicates.has(searchKey(draft.name, i18n.locale))) return '';
  return i18n.t('drafts.name.source_connection', { connection });
}
