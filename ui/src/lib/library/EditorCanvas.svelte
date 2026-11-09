<script lang="ts">
  import type { I18n } from '../i18n';
  import type { NewOfflinePage } from './client';
  import { firstFree, pointerBox, repeatLayout, validBox, validGroup, moveSelection, type FixedLayout, type LayoutChange } from './editor-geometry';
  import { selectWidget, widgetOrder } from './editor-selection';
  import type { EditorFinding } from './editor-composition';
  import { resolveText } from '../components';
  let { i18n, widgets, titles, disabled = false, onchange, onremove, onduplicate, selectedIds = [], onselection = () => {}, ongroupmove, ongroupremove, findings = [] }: {
    i18n: I18n; widgets: NewOfflinePage['widgets']; titles: string[]; disabled?: boolean;
    onchange: (index: number, layout: FixedLayout, action: 'move' | 'resize') => void;
    onremove: (index: number) => void; onduplicate: (index: number) => void;
    selectedIds?: string[]; onselection?: (ids: string[]) => void;
    ongroupmove?: (changes: LayoutChange[]) => void; ongroupremove?: (ids: string[]) => void;
    findings?: readonly EditorFinding[];
  } = $props();
  let viewport: HTMLDivElement;
  let anchor = $state<string | null>(null);
  const selected = $derived(selectedIds.at(-1) ?? null);
  const selectedIndex = $derived(widgets.findIndex((widget) => widget.widget_id === selected));
  let zoom = $state(1);
  let x = $state(0); let y = $state(0); let w = $state(12); let h = $state(5);
  let rejected = $state(false);
  let grid: HTMLDivElement;
  let gesture = $state<{ pointer: number; index: number; widgetId: string | undefined; ids: string[]; originals: LayoutChange[]; startX: number; startY: number; cellWidth: number; cellHeight: number; resize: boolean; original: FixedLayout; preview: FixedLayout } | null>(null);
  const gestureChanges = $derived(gesture ? gesture.resize ? [{ index: gesture.index, layout: gesture.preview }] : gesture.originals.map((change) => ({ index: change.index, layout: { ...change.layout, x: change.layout.x + gesture!.preview.x - gesture!.original.x, y: change.layout.y + gesture!.preview.y - gesture!.original.y } })) : []);
  let capture: HTMLElement | null = null;
  function cancelGesture(): void {
    const pointer = gesture?.pointer; gesture = null;
    if (pointer !== undefined && capture?.hasPointerCapture(pointer)) capture.releasePointerCapture(pointer);
    capture = null;
  }
  function startPointer(event: PointerEvent, index: number, resize: boolean): void {
    if (gesture || event.button !== 0 || widgets[index].layout?.mode !== 'fixed' || repeatLayout(widgets[index])) return;
    if (disabled) { select(index, event); return; }
    event.preventDefault();
    const id = widgets[index].widget_id;
    if (!id) return;
    if (event.ctrlKey || event.metaKey || event.shiftKey) { select(index, event); return; }
    const ids = resize || !selectedIds.includes(id) ? select(index) : [...selectedIds];
    const originals = moveSelection(widgets, ids, 0, 0);
    if (!originals) { rejected = true; return; }
    const box = widgets[index].layout as FixedLayout; const rect = grid.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;
    capture = event.currentTarget as HTMLElement; capture.focus({ preventScroll: true }); capture.setPointerCapture(event.pointerId);
    gesture = { pointer: event.pointerId, index, widgetId: id, ids, originals, startX: event.clientX, startY: event.clientY, cellWidth: rect.width / 72, cellHeight: rect.height / 64, resize, original: { ...box }, preview: { ...box } };
  }
  function movePointer(event: PointerEvent): void {
    if (!gesture || gesture.pointer !== event.pointerId) return;
    gesture.preview = pointerBox(gesture.original, event.clientX - gesture.startX, event.clientY - gesture.startY, gesture.cellWidth, gesture.cellHeight, gesture.resize);
  }
  function finishPointer(event: PointerEvent): void {
    if (!gesture || gesture.pointer !== event.pointerId) return;
    const current = widgets[gesture.index];
    if (!current || current.widget_id !== gesture.widgetId || gesture.originals.some((change) => !widgets[change.index] || !gesture!.ids.includes(widgets[change.index].widget_id ?? '') || JSON.stringify(widgets[change.index].layout) !== JSON.stringify(change.layout))) { cancelGesture(); return; }
    movePointer(event); const finished = gesture; const changes = gestureChanges; cancelGesture();
    if (JSON.stringify(finished.preview) !== JSON.stringify(finished.original)) {
      if (finished.resize) apply(finished.index, finished.preview, 'resize'); else applyGroup(changes);
    }
  }
  $effect(() => {
    const box = selectedIndex >= 0 ? widgets[selectedIndex].layout : null;
    if (box?.mode === 'fixed') { x = box.x; y = box.y; w = box.w; h = box.h; }
  });
  function select(index: number, modifiers?: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean }, toggle = false): string[] {
    const id = widgets[index].widget_id; if (!id) return [];
    const next = selectWidget(selectedIds, id, widgetOrder(widgets), anchor, toggle || !!modifiers?.ctrlKey || !!modifiers?.metaKey, !!modifiers?.shiftKey);
    anchor = next.anchor; onselection(next.ids);
    const layout = widgets[index].layout;
    if (layout) { w = layout.w; h = layout.h; if (layout.mode === 'fixed') { x = layout.x; y = layout.y; } }
    rejected = false;
    return next.ids;
  }
  function applyGroup(changes: LayoutChange[]): void {
    if (disabled) return;
    rejected = !validGroup(widgets, changes) || (changes.length > 1 && !ongroupmove);
    if (rejected) return;
    if (changes.length === 1) onchange(changes[0].index, changes[0].layout, 'move'); else ongroupmove?.(changes);
  }
  function selectionKey(event: KeyboardEvent): boolean {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') { event.preventDefault(); event.stopPropagation(); onselection(widgetOrder(widgets)); return true; }
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onselection([]); return true; }
    return false;
  }
  function apply(index: number, box: FixedLayout, action: 'move' | 'resize'): void {
    if (disabled) return;
    rejected = !validBox(box, widgets, index) || repeatLayout(widgets[index]);
    if (rejected) return;
    onchange(index, box, action); x = box.x; y = box.y; w = box.w; h = box.h;
  }
  function key(event: KeyboardEvent, index: number, inList = false): void {
    if (gesture) { if (event.key === 'Escape') { event.preventDefault(); cancelGesture(); } return; }
    if (selectionKey(event)) return;
    if (inList && event.key === ' ') { event.preventDefault(); select(index, undefined, true); return; }
    if (disabled) return;
    if (event.key === 'Delete') { event.preventDefault(); if (selectedIds.length > 1 && ongroupremove) ongroupremove(selectedIds); else onremove(index); return; }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'd') { event.preventDefault(); onduplicate(index); return; }
    const delta = ({ ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] } as Record<string, number[]>)[event.key];
    const layout = widgets[index].layout;
    if (!delta || layout?.mode !== 'fixed') return;
    event.preventDefault();
    const ids = selectedIds.includes(widgets[index].widget_id ?? '') ? selectedIds : select(index);
    const step = event.shiftKey ? 6 : 1;
    if (!event.ctrlKey) {
      const changes = moveSelection(widgets, ids, delta[0] * step, delta[1] * step);
      if (!changes) { rejected = true; return; } applyGroup(changes); return;
    }
    if (ids.length > 1) { rejected = true; return; }
    apply(index, event.ctrlKey ? { ...layout, w: layout.w + delta[0] * step, h: layout.h + delta[1] * step }
      : { ...layout, x: layout.x + delta[0] * step, y: layout.y + delta[1] * step }, event.ctrlKey ? 'resize' : 'move');
  }
  function place(index: number): void {
    const box = firstFree(widgets, index);
    if (!box) { rejected = true; return; }
    select(index); apply(index, box, 'move');
  }
</script>

<section aria-labelledby="editor-canvas-title" class="editor-canvas">
  <h3 id="editor-canvas-title">{i18n.t('offline_editor.canvas')}</h3>
  <p id="editor-canvas-help">{i18n.t('offline_editor.canvas_help')}</p>
  <div class="actions">
    <button type="button" disabled={gesture !== null} onclick={() => zoom = Math.max(.25, zoom / 1.25)}>{i18n.t('offline_editor.zoom_out')}</button>
    <button type="button" disabled={gesture !== null} onclick={() => zoom = 1}>{i18n.t('offline_editor.zoom_reset')}</button>
    <button type="button" disabled={gesture !== null} onclick={() => zoom = Math.min(3, zoom * 1.25)}>{i18n.t('offline_editor.zoom_in')}</button>
    <button type="button" disabled={gesture !== null} onclick={() => zoom = Math.max(.25, (viewport?.clientWidth ?? 864) / 864)}>{i18n.t('offline_editor.zoom_fit')}</button>
  </div>
  {#if rejected}<p role="alert">{i18n.t('offline_editor.geometry_rejected')}</p>{/if}
  <!-- The scroll viewport needs its own keyboard stop for panning. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Focusable scroll region supports canvas selection shortcuts.) -->
  <div class="canvas-scroll" bind:this={viewport} role="region" aria-label={i18n.t('offline_editor.canvas')} tabindex="0" onkeydown={(event) => { if (event.target === viewport) selectionKey(event); }}>
    <div class="canvas-grid" bind:this={grid} style:width={`${864 * zoom}px`} style:height={`${64 * 33.6 * zoom}px`}>
      {#each widgets as widget, index (widget.widget_id)}
        {#if widget.layout?.mode === 'fixed'}
          {@const box = widget.layout}
          {@const widgetFindings = findings.filter((finding) => finding.widget_ids.includes(widget.widget_id ?? ''))}
          <div class="canvas-widget-shell"
            style:left={`${box.x / 72 * 100}%`} style:top={`${box.y / 64 * 100}%`} style:width={`${box.w / 72 * 100}%`} style:height={`${box.h / 64 * 100}%`}
          >
          <button type="button" class="canvas-widget" class:selected={selectedIds.includes(widget.widget_id ?? '')}
            aria-pressed={selectedIds.includes(widget.widget_id ?? '')} aria-describedby={`editor-canvas-help canvas-findings-${widget.widget_id}`}
            aria-label={i18n.t('offline_editor.widget_box', { widget: titles[index], x: box.x, y: box.y, w: box.w, h: box.h })}
            onclick={(event) => { if (event.detail === 0) select(index, event); }} onkeydown={(event) => key(event, index)}
            onpointerdown={(event) => startPointer(event, index, false)} onpointermove={movePointer} onpointerup={finishPointer} onpointercancel={cancelGesture} onlostpointercapture={cancelGesture}><bdi>{titles[index]}</bdi></button>
          <button type="button" class="resize-handle" disabled={disabled} aria-label={i18n.t('offline_editor.resize_widget', { widget: titles[index] })}
            onclick={() => select(index)}
            onkeydown={(event) => key(event, index)} onpointerdown={(event) => startPointer(event, index, true)} onpointermove={movePointer} onpointerup={finishPointer} onpointercancel={cancelGesture} onlostpointercapture={cancelGesture}>{i18n.t('offline_editor.resize')}</button>
          <span id={`canvas-findings-${widget.widget_id}`} class="visually-hidden">{#each widgetFindings as finding}{resolveText(i18n, finding.message)} {/each}</span>
          {#if widgetFindings.length}<span class="canvas-finding-badge">{i18n.t('offline_editor.finding_count', { count: widgetFindings.length })}</span>{/if}
          </div>
        {/if}
      {/each}
      {#if gesture}
        {#each gestureChanges as change (change.index)}
        {@const box = change.layout}
        <div class="gesture-preview" class:invalid={!validGroup(widgets, gestureChanges)} aria-hidden="true"
          style:left={`${box.x / 72 * 100}%`} style:top={`${box.y / 64 * 100}%`} style:width={`${box.w / 72 * 100}%`} style:height={`${box.h / 64 * 100}%`}></div>
        {/each}
      {/if}
    </div>
  </div>
  {#if gesture}<p role="status">{i18n.t('offline_editor.widget_box', { widget: titles[gesture.index], x: gesture.preview.x, y: gesture.preview.y, w: gesture.preview.w, h: gesture.preview.h })}</p>{/if}
  <ul class="geometry-list">
    {#each widgets as widget, index (widget.widget_id)}<li>
      <button type="button" id={`editor-list-widget-${widget.widget_id}`} aria-pressed={selectedIds.includes(widget.widget_id ?? '')} onclick={(event) => select(index, event)} onkeydown={(event) => key(event, index, true)}><bdi>{titles[index]}</bdi></button>
      {#if widget.layout?.mode === 'fixed'}<span>{i18n.t('offline_editor.box', { x: widget.layout.x, y: widget.layout.y, w: widget.layout.w, h: widget.layout.h })}</span>
      {:else if repeatLayout(widget)}<span>{i18n.t('offline_editor.repeat_auto')}</span>
      {:else}<button type="button" disabled={disabled} onclick={() => place(index)}>{i18n.t('offline_editor.place')}</button>{/if}
    </li>{/each}
  </ul>
  <p role="status">{i18n.t('offline_editor.selected_count', { count: selectedIds.length })}</p>
  {#if selectedIds.length === 1 && selectedIndex >= 0 && widgets[selectedIndex].layout?.mode === 'fixed'}
    <form onsubmit={(event) => { event.preventDefault(); apply(selectedIndex, { mode: 'fixed', x, y, w, h }, 'resize'); }}>
      <fieldset disabled={disabled}>
        <legend>{i18n.t('offline_editor.geometry_for', { widget: titles[selectedIndex] })}</legend>
        <label for="editor-box-x">{i18n.t('offline_editor.x')}</label><input id="editor-box-x" type="number" min="0" max="71" step="1" bind:value={x} required />
        <label for="editor-box-y">{i18n.t('offline_editor.y')}</label><input id="editor-box-y" type="number" min="0" max="63" step="1" bind:value={y} required />
        <label for="editor-box-w">{i18n.t('offline_editor.w')}</label><input id="editor-box-w" type="number" min="1" max="72" step="1" bind:value={w} required />
        <label for="editor-box-h">{i18n.t('offline_editor.h')}</label><input id="editor-box-h" type="number" min="1" max="64" step="1" bind:value={h} required />
        <button type="submit">{i18n.t('offline_editor.apply_geometry')}</button>
      </fieldset>
    </form>
  {/if}
</section>

<style>
  .editor-canvas { min-width: 0; }
  .canvas-scroll { max-height: 60vh; overflow: auto; border: 1px solid var(--grid-bounds); margin-block: .5rem; }
  .canvas-grid { position: relative; background-color: var(--surface-canvas); background-image: linear-gradient(to right, var(--grid-line) 1px, transparent 1px), linear-gradient(to bottom, var(--grid-line) 1px, transparent 1px); background-size: calc(100% / 72) calc(100% / 64); }
  .canvas-widget-shell { position: absolute; }
  .canvas-widget { position: absolute; inset: 0; width: 100%; height: 100%; overflow: hidden; min-height: 0; border: 1px solid var(--grid-bounds); background: var(--surface-base); color: var(--text-primary); border-radius: 0; padding: .25rem; touch-action: none; cursor: move; }
  .resize-handle { position: absolute; right: 0; bottom: 0; min-width: 32px; min-height: 32px; touch-action: none; cursor: nwse-resize; }
  .gesture-preview { position: absolute; border: 3px dashed var(--focus-ring); background: var(--surface-selected); opacity: .7; pointer-events: none; }
  .gesture-preview.invalid { border-style: double; border-color: var(--border-strong); }
  .canvas-widget.selected { outline: 3px solid var(--focus-ring); outline-offset: -3px; }
  .canvas-finding-badge { position: absolute; top: 0; right: 0; background: var(--surface-raised); color: var(--text-primary); pointer-events: none; font-size: .75rem; }
  .geometry-list li { display: flex; flex-wrap: wrap; align-items: center; gap: .5rem; margin-block: .3rem; }
  .geometry-list button, .actions button { min-height: 32px; }
</style>
