import type { NewOfflinePage, PageLayout } from './client';
type Widget = NewOfflinePage['widgets'][number];
export type FixedLayout = Extract<PageLayout, { mode: 'fixed' }>;
export type LayoutChange = { index: number; layout: FixedLayout };
export function validGroup(widgets: readonly Widget[], changes: readonly LayoutChange[]): boolean {
  const indices = new Set(changes.map((change) => change.index));
  if (changes.length === 0 || indices.size !== changes.length) return false;
  const untouched = widgets.map((widget, index) => indices.has(index) ? { ...widget, layout: undefined } : widget);
  return changes.every((change, at) => change.index >= 0 && change.index < widgets.length && !repeatLayout(widgets[change.index])
    && validBox(change.layout, untouched, change.index) && !changes.some((other, position) => at !== position && overlaps(change.layout, other.layout)));
}
export function moveSelection(widgets: readonly Widget[], ids: readonly string[], dx: number, dy: number): LayoutChange[] | null {
  const changes: LayoutChange[] = [];
  for (const id of ids) {
    const index = widgets.findIndex((widget) => widget.widget_id === id); const layout = widgets[index]?.layout;
    if (layout?.mode !== 'fixed') return null;
    changes.push({ index, layout: { ...layout, x: layout.x + dx, y: layout.y + dy } });
  }
  return validGroup(widgets, changes) ? changes : null;
}
export function pointerBox(original: FixedLayout, dx: number, dy: number, cellWidth: number, cellHeight: number, resize: boolean): FixedLayout {
  const columns = Math.round(dx / cellWidth); const rows = Math.round(dy / cellHeight);
  return resize ? { ...original, w: original.w + columns, h: original.h + rows }
    : { ...original, x: original.x + columns, y: original.y + rows };
}
export function overlaps(a: FixedLayout, b: FixedLayout): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}
export function validBox(box: FixedLayout, widgets: readonly Widget[], index: number): boolean {
  return [box.x, box.y, box.w, box.h].every(Number.isInteger) && box.x >= 0 && box.y >= 0
    && box.w >= 1 && box.h >= 1 && box.x + box.w <= 72 && box.y + box.h <= 64
    && !widgets.some((widget, at) => at !== index && widget.layout?.mode === 'fixed' && overlaps(box, widget.layout));
}
export function repeatLayout(widget: Widget): boolean { return widget.kind === 'item_value' && widget.expansion === 'widget_per_host'; }
export function firstFree(widgets: readonly Widget[], index: number): FixedLayout | null {
  const widget = widgets[index];
  if (repeatLayout(widget)) return null;
  const { w, h } = widget.layout ?? { w: widget.kind === 'graph' ? 36 : 12, h: 5 };
  for (let y = 0; y <= 64 - h; y++) for (let x = 0; x <= 72 - w; x++) {
    const box = { mode: 'fixed' as const, x, y, w, h };
    if (validBox(box, widgets, index)) return box;
  }
  return null;
}
