import type { CatalogMetric, NewOfflinePage, PageRole } from './client';
import { canonicalEditorState } from './offline-edit';
import { singleSource, validSource } from './offline-roles';
import { validGroup, type FixedLayout } from './editor-geometry';

type Widget = NewOfflinePage['widgets'][number];
type SupportedWidget = Exclude<Widget, { kind: 'preserved' }>;
export type ClipboardCode = 'empty' | 'opaque' | 'reference' | 'catalog' | 'role' | 'metric' | 'geometry' | 'limit' | 'source_changed' | 'cross_page_cut' | 'ids';
export interface ClipboardIssue { code: ClipboardCode; value?: string }
export interface ClipboardContext { sourceId: string; page: NewOfflinePage; metrics: Record<string, CatalogMetric[]> }
export interface ClipboardContent { sourceId: string; cut: boolean; widgets: SupportedWidget[]; roles: Record<string, PageRole>; catalogs: NewOfflinePage['catalogs']; evidence: Record<string, CatalogMetric> }
const refs = (widget: SupportedWidget) => widget.kind === 'graph' ? widget.metrics : [{ ...widget.metric, hosts: widget.hosts }];
const extra = (value: object, keys: readonly string[]) => Object.keys(value).some((key) => !keys.includes(key));
function unsupportedFields(widget: SupportedWidget): boolean {
  if (extra(widget, widget.kind === 'graph' ? ['kind', 'widget_id', 'layout', 'title', 'metrics'] : ['kind', 'widget_id', 'layout', 'title', 'metric', 'hosts', 'expansion'])) return true;
  if (widget.layout && extra(widget.layout, widget.layout.mode === 'fixed' ? ['mode', 'x', 'y', 'w', 'h'] : ['mode', 'w', 'h'])) return true;
  if (widget.kind === 'item_value' && extra(widget.metric, ['catalog_id', 'metric_id'])) return true;
  return refs(widget).some((source) => extra(source, widget.kind === 'graph' ? ['catalog_id', 'metric_id', 'hosts', 'color'] : ['catalog_id', 'metric_id', 'hosts']) || extra(source.hosts, ['role', 'selection', 'slot_id']));
}

/** Per application window, memory-only. No system clipboard or browser storage. */
export class PageClipboard {
  private content: ClipboardContent | null = null;
  read(): ClipboardContent | null { return this.content ? structuredClone(this.content) : null; }
  clear(): void { this.content = null; }
  copy(context: ClipboardContext, ids: readonly string[], cut: boolean): ClipboardIssue[] {
    if (!ids.length) return [{ code: 'empty' }];
    const widgets = context.page.widgets.filter((widget) => widget.widget_id && ids.includes(widget.widget_id));
    if (widgets.length !== new Set(ids).size) return [{ code: 'source_changed' }];
    if (widgets.some((widget) => widget.kind === 'preserved')) return [{ code: 'opaque' }];
    const supported = widgets as SupportedWidget[];
    if (supported.some(unsupportedFields)) return [{ code: 'reference' }];
    const roles: Record<string, PageRole> = {}; const catalogs: NewOfflinePage['catalogs'] = []; const evidence: Record<string, CatalogMetric> = {};
    for (const widget of supported) for (const metric of refs(widget)) {
      const role = context.page.roles[metric.hosts.role]; if (!role) return [{ code: 'role', value: metric.hosts.role }];
      roles[metric.hosts.role] = role;
      const pin = context.page.catalogs.find((pin) => pin.catalog_id === metric.catalog_id);
      if (!pin) return [{ code: 'catalog', value: metric.catalog_id }];
      if (!catalogs.some((item) => item.catalog_id === pin.catalog_id)) catalogs.push(pin);
      const source = context.metrics[metric.catalog_id]?.find((item) => item.id === metric.metric_id);
      if (!source) return [{ code: 'metric', value: metric.metric_id }];
      evidence[`${metric.catalog_id}/${metric.metric_id}`] = source;
    }
    this.content = structuredClone({ sourceId: context.sourceId, cut, widgets: supported, roles, catalogs, evidence });
    return [];
  }
  consumeCut(): void { if (this.content?.cut) this.content = null; }
}

function sameRole(left: PageRole | undefined, right: PageRole): boolean {
  return !!left && canonicalEditorState({ cardinality: left.cardinality, slots: left.slots }) === canonicalEditorState({ cardinality: right.cardinality, slots: right.slots });
}

export function inspectPaste(content: ClipboardContent | null, target: ClipboardContext, placement: 'original' | 'first_free'): { issues: ClipboardIssue[]; widgets: SupportedWidget[]; removeIds: string[] } {
  const failed = (issues: ClipboardIssue[]) => ({ issues, widgets: [], removeIds: [] });
  if (!content) return failed([{ code: 'empty' }]);
  if (content.cut && content.sourceId !== target.sourceId) return failed([{ code: 'cross_page_cut' }]);
  const removeIds = content.cut ? content.widgets.map((widget) => widget.widget_id ?? '') : [];
  if (content.cut && content.widgets.some((source) => canonicalEditorState(target.page.widgets.find((widget) => widget.widget_id === source.widget_id)) !== canonicalEditorState(source))) return failed([{ code: 'source_changed' }]);
  const issues: ClipboardIssue[] = [];
  for (const pin of content.catalogs) if (!target.page.catalogs.some((item) => item.catalog_id === pin.catalog_id && item.version === pin.version)) issues.push({ code: 'catalog', value: pin.catalog_id });
  for (const [id, role] of Object.entries(content.roles)) if (!sameRole(target.page.roles[id], role)) issues.push({ code: 'role', value: id });
  for (const widget of content.widgets) for (const metric of refs(widget)) {
    const candidate = target.metrics[metric.catalog_id]?.find((item) => item.id === metric.metric_id);
    if (!candidate || canonicalEditorState(candidate) !== canonicalEditorState(content.evidence[`${metric.catalog_id}/${metric.metric_id}`])
      || (widget.kind === 'graph' && !['numeric_float', 'numeric_unsigned'].includes(candidate.value_type)) || candidate.value_type === 'binary') issues.push({ code: 'metric', value: metric.metric_id });
    if (!validSource(metric.hosts, target.page.roles) || (widget.kind === 'item_value' && (widget.expansion === 'single' ? !singleSource(metric.hosts, target.page.roles) : metric.hosts.selection !== 'all'))) issues.push({ code: 'role', value: metric.hosts.role });
  }
  const existing = target.page.widgets.filter((widget) => !removeIds.includes(widget.widget_id ?? ''));
  if (existing.length + content.widgets.length > 256) issues.push({ code: 'limit' });
  if (issues.length) return failed(issues);
  const widgets = structuredClone(content.widgets);
  const fixed = widgets.map((widget, index) => ({ widget, index })).filter((entry): entry is { widget: SupportedWidget & { layout: FixedLayout }; index: number } => entry.widget.layout?.mode === 'fixed');
  if (fixed.length) {
    const minX = Math.min(...fixed.map(({ widget }) => widget.layout.x)); const minY = Math.min(...fixed.map(({ widget }) => widget.layout.y));
    const width = Math.max(...fixed.map(({ widget }) => widget.layout.x + widget.layout.w)) - minX;
    const height = Math.max(...fixed.map(({ widget }) => widget.layout.y + widget.layout.h)) - minY;
    const validAt = (x: number, y: number): boolean => {
      const changes = fixed.map(({ widget, index }) => ({ index: existing.length + index, layout: { ...widget.layout, x: widget.layout.x - minX + x, y: widget.layout.y - minY + y } }));
      if (!validGroup([...existing, ...widgets], changes)) return false;
      for (const change of changes) widgets[change.index - existing.length].layout = change.layout;
      return true;
    };
    if (placement === 'original') { if (!validAt(minX, minY)) return failed([{ code: 'geometry' }]); }
    else {
      let placed = false;
      for (let y = 0; !placed && y <= 64 - height; y++) for (let x = 0; !placed && x <= 72 - width; x++) placed = validAt(x, y);
      if (!placed) return failed([{ code: 'geometry' }]);
    }
  }
  return { issues: [], widgets, removeIds };
}

export function applyPaste(content: ClipboardContent | null, target: ClipboardContext, placement: 'original' | 'first_free', newId: () => string): { page: NewOfflinePage | null; ids: string[]; issues: ClipboardIssue[] } {
  const plan = inspectPaste(content, target, placement);
  if (plan.issues.length || !content) return { page: null, ids: [], issues: plan.issues };
  const base = target.page.widgets.filter((widget) => !plan.removeIds.includes(widget.widget_id ?? ''));
  const existingIds = new Set(base.map((widget) => widget.widget_id)); const ids: string[] = [];
  const pasted = plan.widgets.map((widget) => ({ ...widget, widget_id: content.cut ? widget.widget_id : newId() }));
  for (const widget of pasted) { if (!widget.widget_id || existingIds.has(widget.widget_id)) return { page: null, ids: [], issues: [{ code: 'ids' }] }; existingIds.add(widget.widget_id); ids.push(widget.widget_id); }
  return { page: { ...target.page, widgets: [...base, ...pasted] }, ids, issues: [] };
}
