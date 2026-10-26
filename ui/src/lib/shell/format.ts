import type { I18n } from '../i18n';
import { resolveText } from '../components';
import type { DraftSource, DraftSummary, DraftWidget } from './drafts';
import { freshnessLabel, LOCAL_STATE_LABEL, widgetTypeLabel } from './labels';
import { periodUnit } from './preview';

export const SHORT_DATE_TIME: Intl.DateTimeFormatOptions = { dateStyle: 'short', timeStyle: 'short' };

// Values below are data lists (names, times), not sentences: each localized part comes from a message ID.
const SEPARATOR = ' · ';

/** Source dashboard, optional page and snapshot time; server data is shown as is. */
export function sourceText(i18n: I18n, source: DraftSource): string {
  const object = source.page_name ? `${source.dashboard_name} / ${source.page_name}` : source.dashboard_name;
  return [object, i18n.date(source.snapshot_at, SHORT_DATE_TIME)].join(SEPARATOR);
}

/** Local state and, for server copies, snapshot freshness. */
export function stateText(i18n: I18n, draft: Pick<DraftSummary, 'local_state' | 'freshness'>): string {
  const parts = [resolveText(i18n, LOCAL_STATE_LABEL[draft.local_state])];
  if (draft.freshness) parts.push(resolveText(i18n, freshnessLabel(draft.freshness)));
  return parts.join(SEPARATOR);
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

/** Page name; an empty user name is shown as «Untitled». */
export function pageName(i18n: I18n, name: string): string {
  return name.trim() === '' ? i18n.t('object.page.untitled') : name;
}

/** Display name of a page or widget; an empty user name falls back to a neutral mark. */
export function displayName(name: string, fallback = '—'): string {
  return name.trim() === '' ? fallback : name;
}
