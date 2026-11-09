import { render } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { EditorHistory } from './editor-history';
import { EditorAutosave } from './editor-autosave';
import { firstFree, pointerBox, validBox } from './editor-geometry';
import { PageEditingSession, type EditorAccess } from './editor-session';
import { ApiFailure } from '../api';
import EditorCanvas from './EditorCanvas.svelte';
import { createI18n } from '../i18n';
import type { NewOfflinePage } from './client';

afterEach(() => vi.useRealTimers());

describe('local draft history', () => {
  it('restores complete objects with stable IDs and clears redo after a new command', () => {
    const history = new EditorHistory({ widgets: [{ id: 'stable', source: 'first' }] });
    history.commit({ widgets: [] }, 'delete');
    expect(history.undo()).toEqual({ widgets: [{ id: 'stable', source: 'first' }] });
    expect(history.redoAction).toBe('delete');
    history.commit({ widgets: [{ id: 'stable', source: 'second' }] }, 'source');
    expect(history.redo()).toBeNull();
    expect(history.undo()).toEqual({ widgets: [{ id: 'stable', source: 'first' }] });
  });
  it('keeps 200 steps, reports truncation and starts empty after reopen', () => {
    const history = new EditorHistory(0);
    for (let value = 1; value <= 205; value++) history.commit(value, 'step');
    expect(history.truncated).toBe(true);
    let oldest = 205;
    for (let count = 0; count < 200; count++) oldest = history.undo() ?? -1;
    expect(oldest).toBe(5); expect(history.undo()).toBeNull();
    const reopened = new EditorHistory(205);
    expect(reopened.undoAction).toBeNull(); expect(reopened.redoAction).toBeNull();
  });
  it('does not add steps for save/zoom or mutate caller snapshots', () => {
    const original = { x: 1 }; const history = new EditorHistory(original);
    original.x = 9;
    expect(history.commit({ x: 1 }, 'save')).toBe(false);
    history.commit({ x: 2 }, 'move');
    const undone = history.undo(); if (undone) undone.x = 20;
    expect(history.redo()).toEqual({ x: 2 });
  });
});

function autosaveFixture(save?: () => Promise<void>, retryable = true) {
  let dirty = true;
  const saved = vi.fn(async () => { if (save) await save(); dirty = false; });
  const failed = vi.fn();
  const controller = new EditorAutosave({ dirty: () => dirty, save: saved, retryable: () => retryable, failed });
  return { controller, saved, failed, setDirty: (value: boolean) => { dirty = value; } };
}

describe('serial autosave deadlines', () => {
  it('saves after 2 seconds idle', async () => {
    vi.useFakeTimers(); const { controller, saved } = autosaveFixture();
    controller.changed(); await vi.advanceTimersByTimeAsync(1_999); expect(saved).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1); expect(saved).toHaveBeenCalledTimes(1); controller.dispose();
  });
  it('saves by 30 seconds under continuous editing', async () => {
    vi.useFakeTimers(); const { controller, saved } = autosaveFixture();
    controller.changed();
    for (let second = 1; second < 30; second++) { await vi.advanceTimersByTimeAsync(1_000); controller.changed(); }
    expect(saved).not.toHaveBeenCalled(); await vi.advanceTimersByTimeAsync(1_000);
    expect(saved).toHaveBeenCalledTimes(1); controller.dispose();
  });
  it('uses the configured 60-second ceiling and keeps manual save when disabled', async () => {
    vi.useFakeTimers(); const { controller, saved, setDirty } = autosaveFixture();
    controller.setMaxInterval(60); controller.changed();
    for (let second = 1; second < 60; second++) { await vi.advanceTimersByTimeAsync(1_000); controller.changed(); }
    expect(saved).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1_000); expect(saved).toHaveBeenCalledTimes(1);
    setDirty(true); controller.setEnabled(false); controller.changed();
    await vi.advanceTimersByTimeAsync(120_000); expect(saved).toHaveBeenCalledTimes(1);
    await controller.flush(); expect(saved).toHaveBeenCalledTimes(2); controller.dispose();
  });
  it('retries write failures after 5, 15 and 60 seconds, without a tight loop', async () => {
    vi.useFakeTimers(); const { controller, saved, failed } = autosaveFixture(async () => { throw new Error('disk'); });
    controller.changed(); await vi.advanceTimersByTimeAsync(2_000); expect(saved).toHaveBeenCalledTimes(1);
    expect(failed.mock.calls[0]?.[1]).toBe(5);
    await vi.advanceTimersByTimeAsync(5_000); expect(saved).toHaveBeenCalledTimes(2);
    expect(failed.mock.calls[1]?.[1]).toBe(15);
    await vi.advanceTimersByTimeAsync(15_000); expect(saved).toHaveBeenCalledTimes(3);
    expect(failed.mock.calls[2]?.[1]).toBe(60);
    await vi.advanceTimersByTimeAsync(59_999); expect(saved).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(1); expect(saved).toHaveBeenCalledTimes(4); controller.dispose();
  });
  it('stops automatic retries after a conflict and leaves explicit retry available', async () => {
    vi.useFakeTimers(); const { controller, saved } = autosaveFixture(async () => { throw new Error('conflict'); }, false);
    controller.changed(); await vi.advanceTimersByTimeAsync(2_000); await vi.advanceTimersByTimeAsync(120_000);
    expect(saved).toHaveBeenCalledTimes(1); await controller.flush(); expect(saved).toHaveBeenCalledTimes(2); controller.dispose();
  });
  it('shares an in-flight save and cancels timers on close or session disable', async () => {
    vi.useFakeTimers(); let finish = (): void => {};
    const { controller, saved, setDirty } = autosaveFixture(() => new Promise<void>((resolve) => { finish = resolve; }));
    const first = controller.flush(); const second = controller.flush(); expect(first).toBe(second); expect(saved).toHaveBeenCalledTimes(1);
    finish(); await first; setDirty(true); controller.setEnabled(false); controller.changed();
    await vi.advanceTimersByTimeAsync(40_000); expect(saved).toHaveBeenCalledTimes(1);
    controller.setEnabled(true); controller.dispose(); await vi.advanceTimersByTimeAsync(40_000); expect(saved).toHaveBeenCalledTimes(1);
  });
  it('resumes after undo returns to the saved checkpoint during retry backoff', async () => {
    vi.useFakeTimers(); let fail = true;
    const fixture = autosaveFixture(async () => { if (fail) throw new Error('disk'); });
    fixture.controller.changed(); await vi.advanceTimersByTimeAsync(2_000);
    fixture.setDirty(false); fixture.controller.changed(); await vi.advanceTimersByTimeAsync(10_000);
    fail = false; fixture.setDirty(true); fixture.controller.changed(); await vi.advanceTimersByTimeAsync(2_000);
    expect(fixture.saved).toHaveBeenCalledTimes(2); fixture.controller.dispose();
  });
});

describe('explicit grid edits', () => {
  const widgets: NewOfflinePage['widgets'] = [
    { kind: 'graph', widget_id: 'fixed-widget', title: 'Fixed', metrics: [], layout: { mode: 'fixed', x: 0, y: 0, w: 36, h: 5 } },
    { kind: 'graph', widget_id: 'auto-widget', title: 'Automatic', metrics: [], layout: { mode: 'auto', w: 36, h: 5 } }
  ];
  it('places only the chosen widget in the first free box without shifting neighbours', () => {
    expect(firstFree(widgets, 1)).toEqual({ mode: 'fixed', x: 36, y: 0, w: 36, h: 5 });
    expect(widgets[1].layout?.mode).toBe('auto'); expect(widgets[0].layout).toMatchObject({ x: 0, y: 0 });
  });
  it('rejects overlap, fractions and bounds and never assigns one fixed box to a repeat', () => {
    for (const box of [{ x: 35, y: 0, w: 36, h: 5 }, { x: .5, y: 5, w: 12, h: 5 }, { x: 71, y: 63, w: 2, h: 2 }]) {
      expect(validBox({ mode: 'fixed', ...box }, widgets, 1)).toBe(false);
    }
    expect(firstFree([{ kind: 'item_value', title: 'Repeat', metric: { catalog_id: 'catalog', metric_id: 'metric' }, hosts: { role: 'workers', selection: 'all' }, expansion: 'widget_per_host', layout: { mode: 'auto', w: 12, h: 5 } }], 0)).toBeNull();
  });
  it('computes pointer previews in grid cells at different zoom levels without mutating the model', () => {
    const original = { mode: 'fixed' as const, x: 4, y: 7, w: 12, h: 5 };
    const full = pointerBox(original, 24, 67.2, 12, 33.6, false);
    expect(full).toEqual({ ...original, x: 6, y: 9 });
    expect(pointerBox(original, 12, 33.6, 6, 16.8, false)).toEqual(full);
    expect(pointerBox(original, 24, 33.6, 12, 33.6, true)).toEqual({ ...original, w: 14, h: 6 });
    const outside = pointerBox(original, -120, 0, 12, 33.6, false);
    expect(validBox(outside, widgets, 1)).toBe(false);
    expect(original).toEqual({ mode: 'fixed', x: 4, y: 7, w: 12, h: 5 });
  });
  it('renders actual geometry and named keyboard controls in both languages', () => {
    for (const locale of ['en', 'ru'] as const) {
      const i18n = createI18n(locale);
      const { body } = render(EditorCanvas, { props: { i18n, widgets, titles: ['Fixed', 'Automatic'], onchange: () => {}, onremove: () => {}, onduplicate: () => {} } });
      expect(body).toContain(i18n.t('offline_editor.widget_box', { widget: 'Fixed', x: 0, y: 0, w: 36, h: 5 }));
      expect(body).toContain('width: 50%'); expect(body).toContain(i18n.t('offline_editor.place'));
      expect(body).not.toContain('offline_editor.');
    }
  });
});

describe('page ownership lifecycle', () => {
  it('keeps a second tab read-only, requires explicit takeover, renews and releases on close', async () => {
    vi.useFakeTimers(); const states: EditorAccess[] = [];
    const client = { acquirePage: vi.fn(async (_id: string, takeOver?: boolean) => takeOver ? 'editing' as const : 'read_only' as const), renewPage: vi.fn(async () => ({})), releasePage: vi.fn(async () => ({})) };
    const session = new PageEditingSession(client, (state) => states.push(state));
    await session.open('page'); expect(states.at(-1)).toBe('read_only');
    await vi.advanceTimersByTimeAsync(40_000); expect(client.renewPage).not.toHaveBeenCalled();
    await session.open('page', true); expect(client.acquirePage).toHaveBeenLastCalledWith('page', true);
    await vi.advanceTimersByTimeAsync(20_000); expect(client.renewPage).toHaveBeenCalledTimes(1);
    session.dispose(); await vi.advanceTimersByTimeAsync(40_000); expect(client.renewPage).toHaveBeenCalledTimes(1); expect(client.releasePage).toHaveBeenCalledWith('page');
  });
  it('stops renewal when ownership is lost and never reacquires automatically', async () => {
    vi.useFakeTimers(); const states: EditorAccess[] = [];
    const client = { acquirePage: vi.fn(async () => 'editing' as const), renewPage: vi.fn(async () => { throw new ApiFailure(409, { code: 'offline_page_edit_lost', params: {} }); }), releasePage: vi.fn(async () => ({})) };
    const session = new PageEditingSession(client, (state) => states.push(state));
    await session.open('page'); await vi.advanceTimersByTimeAsync(20_000); expect(states.at(-1)).toBe('read_only');
    await vi.advanceTimersByTimeAsync(120_000); expect(client.renewPage).toHaveBeenCalledTimes(1); expect(client.acquirePage).toHaveBeenCalledTimes(1); session.dispose();
  });
  it('ignores a stale heartbeat reply after an explicit new acquisition', async () => {
    vi.useFakeTimers(); const states: EditorAccess[] = []; let rejectOld = (_error: unknown): void => {};
    const client = { acquirePage: vi.fn(async () => 'editing' as const), renewPage: vi.fn(() => new Promise((_resolve, reject) => { rejectOld = reject; })), releasePage: vi.fn(async () => ({})) };
    const session = new PageEditingSession(client, (state) => states.push(state));
    await session.open('page'); const old = session.check(); await session.open('page', true);
    rejectOld(new ApiFailure(409, { code: 'offline_page_edit_lost', params: {} })); await old;
    expect(states.at(-1)).toBe('editing'); session.dispose();
  });
});
