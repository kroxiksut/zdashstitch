// Screen logic of «Панели в Zabbix» and the copy check (P1-03B/C) without DOM: labels, the
// «On this computer» cell, the widget-type breakdown and the existing-copy rule. Pure for tests.

import { resolveText, type LocalizedText } from '../components';
import type { I18n } from '../i18n';
import { MAX_DRAFT_NAME_CHARS, hasControlCharacter, type DraftDetail } from '../shell/drafts';
import { widgetTypeName } from '../shell/format';
import type {
  AccessFilter,
  DashboardAccess,
  DashboardOwner,
  ExistingCopy,
  ImportPreview,
  PreviewPage,
  PreviewWidget,
  ServerDashboardList
} from './client';

/** Rows per page of the dashboard table. */
export const SERVER_PAGE_SIZE = 50;

/** Longest search (`MAX_DASHBOARD_SEARCH_CHARS` of the backend), in Unicode code points. */
export const MAX_SEARCH_CHARS = 255;

export const OWNER_LABEL: Readonly<Record<DashboardOwner, LocalizedText>> = {
  me: { id: 'server.owner.you' },
  other: { id: 'server.owner.other' }
};

export const ACCESS_LABEL: Readonly<Record<DashboardAccess, LocalizedText>> = {
  private: { id: 'dashboards.visibility.private' },
  shared: { id: 'dashboards.visibility.shared' }
};

export const ACCESS_FILTERS: ReadonlyArray<AccessFilter> = ['all', 'mine', 'shared'];

export function isAccessFilter(value: string): value is AccessFilter {
  return (ACCESS_FILTERS as ReadonlyArray<string>).includes(value);
}

/** «Все 5 / Мои 1 / Общие 4» (mockup 03-01); counts stay hidden until a list is loaded. */
export function accessFilterOptions(
  i18n: I18n,
  list: Pick<ServerDashboardList, 'counts'> | null
): ReadonlyArray<{ value: AccessFilter; label: LocalizedText }> {
  const labels: Record<AccessFilter, LocalizedText> = {
    all: { id: 'common.filter.all' },
    mine: { id: 'server.filter.mine' },
    shared: { id: 'server.filter.shared' }
  };
  return ACCESS_FILTERS.map((value) => ({
    value,
    label: list
      ? { id: 'common.filter.with_count', params: { label: resolveText(i18n, labels[value]), count: list.counts[value] } }
      : labels[value]
  }));
}

/** Newest local copy, or `undefined` (copies arrive newest first; sorted again defensively). */
export function newestCopy(copies: ReadonlyArray<ExistingCopy>): ExistingCopy | undefined {
  return [...copies].sort((left, right) => right.snapshot_at - left.snapshot_at)[0];
}

/** «У меня» cell: «копия · 25.10», «—», or «unknown» when the storage could not be read. */
export function mineText(i18n: I18n, copies: ReadonlyArray<ExistingCopy>, known: boolean): string {
  if (!known) return i18n.t('server.mine.unknown');
  const copy = newestCopy(copies);
  return copy ? i18n.t('server.mine.copy', { date: copy.snapshot_at }) : '—';
}

export interface TypeCount {
  /** Display name, or the technical ID of an unknown type. */
  readonly label: string;
  readonly count: number;
}

export interface TypeBreakdown {
  readonly editable: number;
  readonly other: number;
  readonly editableTypes: ReadonlyArray<TypeCount>;
  readonly otherTypes: ReadonlyArray<TypeCount>;
}

function countTypes(i18n: I18n, widgets: ReadonlyArray<PreviewWidget>): TypeCount[] {
  const counts = new Map<string, number>();
  for (const widget of widgets) counts.set(widget.type, (counts.get(widget.type) ?? 0) + 1);
  return [...counts.entries()]
    .map(([type, count]) => ({ label: widgetTypeName(i18n, type), count }))
    .sort((left, right) => right.count - left.count || left.label.localeCompare(right.label, i18n.locale));
}

/** «Изменение поддерживается: 14 — График 9, …» (mockup 03-01 details). */
export function typeBreakdown(i18n: I18n, pages: ReadonlyArray<PreviewPage>): TypeBreakdown {
  const widgets = pages.flatMap((page) => page.widgets);
  const editable = widgets.filter((widget) => widget.editable);
  const other = widgets.filter((widget) => !widget.editable);
  return {
    editable: editable.length,
    other: other.length,
    editableTypes: countTypes(i18n, editable),
    otherTypes: countTypes(i18n, other)
  };
}

export function typeList(i18n: I18n, types: ReadonlyArray<TypeCount>): string {
  return types.map((type) => i18n.t('server.details.type_count', { type: type.label, count: type.count })).join(', ');
}

/** Number of table pages, at least 1. */
export function pageCount(list: Pick<ServerDashboardList, 'total' | 'limit'>): number {
  return Math.max(1, Math.ceil(list.total / Math.max(1, list.limit)));
}

/** Search text as the backend accepts it: trimmed, at most 255 code points, no control characters. */
export function searchProblem(value: string): 'too_long' | 'control' | null {
  const trimmed = value.trim();
  if ([...trimmed].length > MAX_SEARCH_CHARS) return 'too_long';
  return hasControlCharacter(trimmed) ? 'control' : null;
}

/**
 * How a new copy is named (P2-08A): `base` is the source dashboard or page name (server data),
 * `numbered` renders the localized «<name> (copy N)». `pageNumber` is the 1-based source page of
 * «Take page».
 */
export interface CopyNaming {
  readonly base: string;
  readonly numbered: (name: string, number: number) => string;
  readonly pageNumber?: number;
}

/**
 * Name of a new copy (P2-08A). The first copy of a source keeps the source name (`undefined`: the
 * backend default). When this source already has a local copy, the copy is «<name> (copy N)» with
 * the smallest N ≥ 2 not used by any local draft; the source name is shortened so that the result
 * fits the local name limit.
 */
export function copyDraftName(
  naming: Pick<CopyNaming, 'base' | 'numbered'>,
  hasCopy: boolean,
  takenNames: ReadonlyArray<string>,
  max: number = MAX_DRAFT_NAME_CHARS
): string | undefined {
  if (!hasCopy) return undefined;
  const taken = new Set(takenNames.map((name) => name.trim()));
  const base = naming.base.trim();
  // At most `taken.size + 1` candidates can be taken, so the loop always ends.
  for (let number = 2; number <= taken.size + 2; number += 1) {
    let candidate = naming.numbered(base, number).trim();
    const overflow = [...candidate].length - max;
    if (overflow > 0) {
      const points = [...base];
      candidate = naming.numbered(points.slice(0, Math.max(0, points.length - overflow)).join('').trimEnd(), number).trim();
    }
    if (!taken.has(candidate)) return candidate;
  }
  return undefined;
}

/**
 * The read-only server preview (P2-05E) in the shape of the «Structure» view: the same tree and
 * grid preview as a dashboard draft. Nothing is stored; Zabbix IDs stay keys only.
 */
export function previewStructure(
  preview: Pick<ImportPreview, 'name' | 'pages' | 'properties'>
): Pick<DraftDetail, 'kind' | 'name' | 'pages' | 'properties'> {
  return {
    kind: 'dashboard',
    name: preview.name,
    pages: preview.pages.map((page) => ({
      id: page.dashboard_pageid,
      name: page.name,
      display_period: page.display_period,
      widgets: page.widgets.map((widget) => ({ ...widget }))
    })),
    properties: { ...preview.properties }
  };
}

/** Localized naming of a new copy (P2-08A): «<name> (copy N)» through `drafts.copy_name`. */
export function copyNaming(i18n: I18n, base: string, pageNumber?: number): CopyNaming {
  const numbered = (name: string, number: number): string => i18n.t('drafts.copy_name', { name, number });
  return pageNumber === undefined ? { base, numbered } : { base, numbered, pageNumber };
}
