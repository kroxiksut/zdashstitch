import type { NewOfflinePage } from './client';
export function widgetOrder(widgets: NewOfflinePage['widgets']): string[] {
  return widgets.map((widget, index) => ({ widget, index })).sort((a, b) => {
    const left = a.widget.layout; const right = b.widget.layout;
    if (left?.mode === 'fixed' && right?.mode === 'fixed') return left.y - right.y || left.x - right.x || a.index - b.index;
    return Number(right?.mode === 'fixed') - Number(left?.mode === 'fixed') || a.index - b.index;
  }).map(({ widget }) => widget.widget_id).filter((id): id is string => id !== undefined);
}
export function selectWidget(current: readonly string[], id: string, order: readonly string[], anchor: string | null, toggle: boolean, range: boolean): { ids: string[]; anchor: string } {
  if (range && anchor && order.includes(anchor)) {
    const a = order.indexOf(anchor); const b = order.indexOf(id);
    return { ids: order.slice(Math.min(a, b), Math.max(a, b) + 1), anchor };
  }
  if (toggle) return { ids: current.includes(id) ? current.filter((value) => value !== id) : [...current, id], anchor: id };
  return { ids: [id], anchor: id };
}
