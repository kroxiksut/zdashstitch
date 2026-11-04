import type { LocalizedText } from '../components';
import type { MessageId } from '../i18n';
import type { DraftFreshness, DraftKind, DraftLocalState, DraftOrigin, CheckGroup } from './drafts';
import type { DraftSection, SettingsSection, TopSection } from './router';

export type Tone = 'success' | 'info' | 'warning' | 'danger';

export const KIND_LABEL: Readonly<Record<DraftKind, LocalizedText>> = {
  dashboard: { id: 'object.kind.dashboard' },
  page: { id: 'object.kind.page' }
};

export const ORIGIN_LABEL: Readonly<Record<DraftOrigin, LocalizedText>> = {
  local: { id: 'object.origin.local' },
  server_copy: { id: 'object.origin.server_copy' }
};

export const LOCAL_STATE_LABEL: Readonly<Record<DraftLocalState, LocalizedText>> = {
  saved: { id: 'draft.context.saved' },
  dirty: { id: 'draft.context.dirty' }
};

/** Short local state of a «Drafts» row (P1-00F §9.2); the long form is for details and «Overview». */
export const LIST_STATE_LABEL: Readonly<Record<DraftLocalState, LocalizedText>> = {
  saved: { id: 'drafts.state.saved' },
  dirty: { id: 'drafts.state.modified' }
};

export const LOCAL_STATE_TONE: Readonly<Record<DraftLocalState, Tone>> = { saved: 'info', dirty: 'warning' };

export function freshnessLabel(freshness: DraftFreshness): LocalizedText {
  switch (freshness.state) {
    case 'current':
      return { id: 'draft.freshness.current', params: { time: freshness.checked_at ?? Number.NaN } };
    case 'stale':
      return { id: 'draft.freshness.stale' };
    case 'conflict':
      return { id: 'draft.freshness.conflict' };
    case 'source_missing':
      return { id: 'draft.freshness.source_missing' };
    case 'not_checked':
      return { id: 'draft.freshness.not_checked' };
  }
}

const SHORT_FRESHNESS_ID = {
  current: 'drafts.state.current',
  stale: 'drafts.state.stale',
  conflict: 'drafts.state.changed_on_server',
  source_missing: 'drafts.state.source_missing'
} as const;

/** Freshness in a «Drafts» row: short forms (P1-00F §9.2); «not checked» has no separate short form. */
export function listFreshnessLabel(freshness: DraftFreshness): LocalizedText {
  return freshness.state === 'not_checked' ? freshnessLabel(freshness) : { id: SHORT_FRESHNESS_ID[freshness.state] };
}

export function freshnessTone(freshness: DraftFreshness): Tone {
  if (freshness.state === 'current') return 'success';
  if (freshness.state === 'not_checked') return 'info';
  return 'warning';
}

export const DRAFT_SECTION_LABEL: Readonly<Record<DraftSection, LocalizedText>> = {
  overview: { id: 'draft.nav.overview' },
  structure: { id: 'draft.nav.structure' },
  bindings: { id: 'draft.nav.bindings' },
  checks: { id: 'draft.nav.checks' },
  publication: { id: 'draft.nav.publication' }
};

export const CHECK_GROUP_LABEL: Readonly<Record<CheckGroup, LocalizedText>> = {
  structure: { id: 'checks.group.structure' },
  widgets: { id: 'checks.group.widgets' },
  sources: { id: 'checks.group.sources' },
  references: { id: 'checks.group.references' },
  compatibility: { id: 'checks.group.compatibility' }
};

export interface SettingsEntry {
  readonly section: SettingsSection;
  readonly label: LocalizedText;
  /** G1b sections stay visible but unavailable with «Will be available later» (P1-01H §6). */
  readonly available: boolean;
}

// Order and names follow the accepted settings navigation (mockup 06-01, P1-09).
export const SETTINGS_ENTRIES: ReadonlyArray<SettingsEntry> = [
  { section: 'general', label: { id: 'settings.general.title' }, available: true },
  { section: 'appearance', label: { id: 'settings.appearance.title' }, available: true },
  { section: 'workspace', label: { id: 'settings.workspace.title' }, available: false },
  { section: 'editor', label: { id: 'settings.editor.title' }, available: false },
  { section: 'connections', label: { id: 'settings.connections.title' }, available: false },
  // TODO(O-032): End session — the button lives in «Security and export» (P1-12) and calls
  // `endLocalSession` from `../session` after the `session.end.confirm.*` dialog (P2-SEC-01E).
  { section: 'security', label: { id: 'settings.security.title' }, available: false },
  { section: 'diagnostics', label: { id: 'diagnostics.title' }, available: false },
  { section: 'about', label: { id: 'about.title' }, available: true }
];

export interface TopNavEntry {
  readonly section: Exclude<TopSection, 'settings'>;
  readonly label: LocalizedText;
  readonly href: string;
}

export const TOP_NAV: ReadonlyArray<TopNavEntry> = [
  { section: 'start', label: { id: 'nav.start' }, href: '/' },
  { section: 'drafts', label: { id: 'nav.drafts' }, href: '/drafts' },
  { section: 'server', label: { id: 'nav.server' }, href: '/server' },
  { section: 'library', label: { id: 'nav.library' }, href: '/library' },
  { section: 'connections', label: { id: 'nav.connections' }, href: '/connections' }
];

export type ConnectionMode = 'offline' | 'read_only' | 'read_write';

export const MODE_LABEL: Readonly<Record<ConnectionMode, LocalizedText>> = {
  offline: { id: 'common.state.offline' },
  read_only: { id: 'common.state.read_only' },
  read_write: { id: 'common.state.read_write' }
};

type WidgetTypeMessage<I> = I extends `widget.type.${infer Type}` ? Type : never;

// Zabbix 7.4 widget types with an official display name (P1-00F §9.8). Each one has a
// `widget.type.<id>` message; `satisfies` keeps the list and the catalog in step.
const WIDGET_TYPES = [
  'actionlog', 'clock', 'discovery', 'favgraphs', 'favmaps', 'gauge', 'geomap', 'graph', 'graphprototype',
  'honeycomb', 'hostavail', 'hostcard', 'hostnavigator', 'item', 'itemcard', 'itemhistory', 'itemnavigator',
  'map', 'navtree', 'piechart', 'problemhosts', 'problems', 'problemsbysv', 'slareport', 'svggraph',
  'systeminfo', 'tophosts', 'topitems', 'toptriggers', 'trigover', 'url', 'web'
] as const satisfies ReadonlyArray<WidgetTypeMessage<MessageId>>;
type KnownWidgetType = (typeof WIDGET_TYPES)[number];

/**
 * Localized widget type name, or `null` for an unknown type (decision O-035): application APIs
 * send the technical type id, and the UI shows that id itself when it has no `widget.type.<id>`.
 */
export function widgetTypeLabel(type: string): LocalizedText | null {
  return (WIDGET_TYPES as readonly string[]).includes(type)
    ? ({ id: `widget.type.${type as KnownWidgetType}` } as LocalizedText)
    : null;
}

/** Finding level as text (P1-04F §2.1); colour is never the only carrier. */
export function findingLevelLabel(blocks: ReadonlyArray<'export' | 'publication'>): LocalizedText {
  const exportBlocked = blocks.includes('export');
  const publicationBlocked = blocks.includes('publication');
  if (exportBlocked && publicationBlocked) return { id: 'checks.level.blocks_both' };
  if (exportBlocked) return { id: 'checks.level.blocks_export' };
  if (publicationBlocked) return { id: 'checks.level.blocks_publication' };
  return { id: 'checks.level.warning' };
}

export interface OverviewStep {
  readonly label: LocalizedText;
  readonly help: LocalizedText;
  /** Workspace section that continues the step; absent while its screen is not built. */
  readonly section?: DraftSection;
}

/**
 * «Next steps» of «Overview» (mockup 07-01). A page needs a target dashboard for «Add page to
 * dashboard»; a dashboard is created or published back, so it starts with bindings.
 */
export function overviewSteps(kind: DraftKind): ReadonlyArray<OverviewStep> {
  const bindings: OverviewStep = {
    label: { id: 'draft.overview.step.check_bindings' },
    help: { id: 'draft.overview.step.check_bindings.help' },
    section: 'bindings'
  };
  const plan: OverviewStep = {
    label: { id: 'draft.overview.step.review_plan' },
    help: { id: 'draft.overview.step.review_plan.help' },
    section: 'publication'
  };
  if (kind === 'dashboard') return [bindings, plan];
  return [{ label: { id: 'draft.overview.step.choose_target' }, help: { id: 'draft.overview.step.choose_target.help' } }, bindings, plan];
}
