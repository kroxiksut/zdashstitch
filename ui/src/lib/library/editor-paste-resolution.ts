import type { CatalogMetric, PageRole, PageHostSelection } from './client';
import { PageClipboard, applyPaste, inspectPaste, type ClipboardContent, type ClipboardContext, type ClipboardIssue } from './editor-clipboard';
import { canonicalEditorState } from './offline-edit';
import { validSource } from './offline-roles';
import { acceptsMetricType } from './editor-composition';

export interface PasteResolution {
  catalogs: Record<string, { catalog_id: string; version: number }>;
  metrics: Record<string, { metric_id: string; evidence: CatalogMetric }>;
  roles: Record<string, { role: string; evidence: PageRole; slots: Record<string, string> }>;
}
export const emptyResolution = (): PasteResolution => ({ catalogs: {}, metrics: {}, roles: {} });
export const metricKey = (catalog: string, metric: string): string => `${catalog}/${metric}`;
const same = (a: unknown, b: unknown) => canonicalEditorState(a) === canonicalEditorState(b);
export function compatibleMetric(source: CatalogMetric, candidate: CatalogMetric): boolean {
  return acceptsMetricType('item_value', source.value_type) && source.value_type === candidate.value_type && source.units === candidate.units;
}
export function compatibleRole(source: PageRole, target: PageRole): boolean {
  return source.cardinality === target.cardinality && !!source.slots?.length && source.slots.length === target.slots?.length;
}
export function metricRequirements(content: ClipboardContent | null): Array<{ key: string; catalog: string; metric: CatalogMetric }> {
  if (!content) return [];
  return content.catalogs.flatMap((pin) => Object.entries(content.evidence)
    .filter(([key, metric]) => key === metricKey(pin.catalog_id, metric.id))
    .map(([key, metric]) => ({ key, catalog: pin.catalog_id, metric })));
}

/** Exact identities need no semantic choice. Different identities remain blank. */
export function initialResolution(content: ClipboardContent | null, target: ClipboardContext): PasteResolution {
  const result = emptyResolution();
  if (!content) return result;
  for (const pin of content.catalogs) {
    const found = target.page.catalogs.find((candidate) => same(pin, candidate));
    if (found) result.catalogs[pin.catalog_id] = { ...found };
  }
  for (const { key, catalog, metric } of metricRequirements(content)) {
    const found = target.metrics[catalog]?.find((candidate) => same(metric, candidate));
    if (found && result.catalogs[catalog]) result.metrics[key] = { metric_id: found.id, evidence: structuredClone(found) };
  }
  for (const [id, role] of Object.entries(content.roles)) {
    const candidate = target.page.roles[id];
    if (candidate && compatibleRole(role, candidate) && same(role, candidate)) result.roles[id] = { role: id, evidence: structuredClone(candidate), slots: Object.fromEntries((role.slots ?? []).map((slot) => [slot, slot])) };
  }
  return result;
}

/** Recheck every chosen snapshot against the current pinned target before rewriting a clone. */
export function resolveClipboard(content: ClipboardContent | null, target: ClipboardContext, resolution: PasteResolution): { content: ClipboardContent | null; issues: ClipboardIssue[] } {
  if (!content) return { content: null, issues: [{ code: 'empty' }] };
  if (content.cut) return { content, issues: [] }; // Cut is checked by the existing same-page transaction.
  const sourceMetrics = Object.fromEntries(content.catalogs.map((pin) => [pin.catalog_id, metricRequirements(content).filter((entry) => entry.catalog === pin.catalog_id).map((entry) => entry.metric)]));
  const source = new PageClipboard();
  const invalid = source.copy({ sourceId: content.sourceId, page: { name: '', widgets: content.widgets, catalogs: content.catalogs, roles: content.roles }, metrics: sourceMetrics }, content.widgets.map((widget) => widget.widget_id ?? ''), false);
  if (invalid.length) return { content: null, issues: invalid };
  const issues: ClipboardIssue[] = [];
  for (const pin of content.catalogs) {
    const chosen = resolution.catalogs[pin.catalog_id];
    if (!chosen || !target.page.catalogs.some((candidate) => same(chosen, candidate))) issues.push({ code: 'catalog', value: pin.catalog_id });
  }
  for (const { key, catalog, metric } of metricRequirements(content)) {
    const chosen = resolution.metrics[key]; const pin = resolution.catalogs[catalog];
    const candidate = pin && target.metrics[pin.catalog_id]?.find((candidate) => candidate.id === chosen?.metric_id);
    if (!chosen || !candidate || !same(candidate, chosen.evidence) || !compatibleMetric(metric, candidate)) issues.push({ code: 'metric', value: metric.id });
  }
  for (const [id, role] of Object.entries(content.roles)) {
    const chosen = resolution.roles[id]; const candidate = chosen && target.page.roles[chosen.role];
    // A bijection in declared order preserves all/slot expansion without silent collapse or reorder.
    if (!chosen || !candidate || !same(candidate, chosen.evidence) || !compatibleRole(role, candidate)
      || !same((role.slots ?? []).map((slot) => chosen.slots[slot]), candidate.slots)) issues.push({ code: 'role', value: id });
  }
  if (issues.length) return { content: null, issues };
  const resolved = structuredClone(content); resolved.roles = {}; resolved.catalogs = []; resolved.evidence = {};
  for (const pin of content.catalogs) {
    const chosen = resolution.catalogs[pin.catalog_id];
    if (!resolved.catalogs.some((pin) => pin.catalog_id === chosen.catalog_id)) resolved.catalogs.push({ ...chosen });
  }
  for (const id of Object.keys(content.roles)) {
    const chosen = resolution.roles[id]; resolved.roles[chosen.role] = structuredClone(chosen.evidence);
  }
  const hosts = (source: PageHostSelection): PageHostSelection => {
    if (!validSource(source, content.roles)) { issues.push({ code: 'role', value: source.role }); return source; }
    const chosen = resolution.roles[source.role];
    return { ...source, role: chosen.role, ...(source.selection === 'slot' ? { slot_id: chosen.slots[source.slot_id ?? ''] } : {}) };
  };
  for (const widget of resolved.widgets) {
    if (widget.kind === 'graph') widget.metrics = widget.metrics.map((metric) => {
      const chosen = resolution.metrics[metricKey(metric.catalog_id, metric.metric_id)]; const pin = resolution.catalogs[metric.catalog_id];
      resolved.evidence[metricKey(pin.catalog_id, chosen.metric_id)] = structuredClone(chosen.evidence);
      return { ...metric, catalog_id: pin.catalog_id, metric_id: chosen.metric_id, hosts: hosts(metric.hosts) };
    });
    else {
      const chosen = resolution.metrics[metricKey(widget.metric.catalog_id, widget.metric.metric_id)]; const pin = resolution.catalogs[widget.metric.catalog_id];
      resolved.evidence[metricKey(pin.catalog_id, chosen.metric_id)] = structuredClone(chosen.evidence);
      widget.metric = { catalog_id: pin.catalog_id, metric_id: chosen.metric_id }; widget.hosts = hosts(widget.hosts);
    }
  }
  return issues.length ? { content: null, issues } : { content: resolved, issues: [] };
}

export function inspectResolvedPaste(content: ClipboardContent | null, target: ClipboardContext, placement: 'original' | 'first_free', resolution: PasteResolution) {
  const resolved = resolveClipboard(content, target, resolution);
  return resolved.issues.length ? { issues: resolved.issues, widgets: [], removeIds: [] } : inspectPaste(resolved.content, target, placement);
}
export function applyResolvedPaste(content: ClipboardContent | null, target: ClipboardContext, placement: 'original' | 'first_free', newId: () => string, resolution: PasteResolution) {
  const resolved = resolveClipboard(content, target, resolution);
  return resolved.issues.length ? { issues: resolved.issues, page: null, ids: [] } : applyPaste(resolved.content, target, placement, newId);
}
