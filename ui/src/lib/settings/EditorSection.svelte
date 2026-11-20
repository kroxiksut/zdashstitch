<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiFailure } from '../api';
  import { resolveText } from '../components';
  import type { I18n } from '../i18n';
  import { createEditorSettingsClient, type EditorPreferences, type EditorSnapshot } from './editor';
  import { settingsIssueText } from './file-issue';

  let { i18n }: { i18n: I18n } = $props();
  const client = createEditorSettingsClient();
  let snapshot = $state<EditorSnapshot | null>(null);
  let busy = $state(false);
  let error = $state<'unavailable' | 'changed' | 'not_saved' | null>(null);
  let saved = $state(false);
  let requestNo = 0;

  async function load(): Promise<void> {
    if (busy) return;
    const request = ++requestNo;
    try {
      const loaded = await client.load();
      if (request === requestNo) { snapshot = loaded; error = null; saved = false; }
    } catch { if (request === requestNo) error = 'unavailable'; }
  }
  onMount(() => {
    void load();
    const focused = (): void => { if (!busy) void load(); };
    window.addEventListener('focus', focused);
    return () => { requestNo++; window.removeEventListener('focus', focused); };
  });

  async function change(patch: Partial<EditorPreferences>): Promise<void> {
    if (!snapshot || busy || snapshot.file_state === 'unreadable') return;
    const previous = snapshot;
    ++requestNo;
    busy = true; saved = false; error = null;
    try { snapshot = await client.save({ ...previous.values, ...patch }, previous.fingerprint); saved = !snapshot.session_only; }
    catch (failure) {
      error = failure instanceof ApiFailure && failure.response?.code === 'settings_changed' ? 'changed' : 'not_saved';
      snapshot = previous;
    } finally { busy = false; }
  }
</script>

<section class="workspace" aria-labelledby="editor-settings-title" aria-busy={busy}>
  <header class="screen-head"><h1 id="editor-settings-title">{i18n.t('settings.editor.title')}</h1></header>
  {#if snapshot?.file_state === 'invalid'}
    <p role="alert">{i18n.t('settings.file.invalid.title')}</p>
    {#if snapshot.issue}<p>{resolveText(i18n, settingsIssueText(snapshot.issue))}</p>{/if}
  {/if}
  {#if snapshot?.file_state === 'unreadable'}<p role="alert">{i18n.t('settings.editor.error.unavailable')}</p>{/if}
  {#if snapshot?.session_only}<p role="status">{i18n.t('settings.editor.session_only')}</p>{/if}
  {#if error}<p role="alert">{i18n.t(error === 'changed' ? 'settings.editor.error.changed' : error === 'unavailable' ? 'settings.editor.error.unavailable' : 'settings.editor.error.not_saved')}</p>{/if}
  {#if error === 'changed' || error === 'unavailable' || snapshot?.file_state === 'invalid' || snapshot?.file_state === 'unreadable'}
    <button type="button" disabled={busy} onclick={() => void load()}>{i18n.t('settings.editor.reload')}</button>
  {/if}
  {#if snapshot}
    <section class="card" aria-labelledby="editor-autosave-title">
      <h2 id="editor-autosave-title">{i18n.t('settings.autosave.title')}</h2>
      <label for="editor-autosave">{i18n.t('settings.editor.autosave_mode')}</label>
      <select id="editor-autosave" value={snapshot.values.autosave} disabled={busy || snapshot.file_state === 'unreadable'} onchange={(event) => void change({ autosave: event.currentTarget.value as 'on' | 'off' })}>
        <option value="on">{i18n.t('settings.editor.on')}</option>
        <option value="off">{i18n.t('settings.editor.off')}</option>
      </select>
      {#if snapshot.values.autosave === 'off'}<p role="status">{i18n.t('settings.autosave.off_warning')}</p>{/if}
      <label for="editor-autosave-interval">{i18n.t('settings.editor.max_interval')}</label>
      <select id="editor-autosave-interval" value={snapshot.values.autosave_max_interval_s} disabled={busy || snapshot.file_state === 'unreadable'} onchange={(event) => void change({ autosave_max_interval_s: Number(event.currentTarget.value) as 30 | 60 | 120 })}>
        {#each [30, 60, 120] as seconds}<option value={seconds}>{i18n.t('settings.autosave.max_interval', { seconds })}</option>{/each}
      </select>
    </section>
    <section class="card" aria-labelledby="editor-canvas-settings-title">
      <h2 id="editor-canvas-settings-title">{i18n.t('offline_editor.canvas')}</h2>
      <label for="editor-default-zoom">{i18n.t('settings.editor.default_zoom')}</label>
      <select id="editor-default-zoom" value={snapshot.values.default_zoom} disabled={busy || snapshot.file_state === 'unreadable'}
        onchange={(event) => void change({ default_zoom: event.currentTarget.value as EditorPreferences['default_zoom'] })}>
        <option value="fit_width">{i18n.t('settings.editor.zoom.fit_width')}</option>
        <option value="actual_size">{i18n.t('settings.editor.zoom.actual')}</option>
      </select>
      <label>
        <input type="checkbox" checked={snapshot.values.show_grid} disabled={busy || snapshot.file_state === 'unreadable'}
          onchange={(event) => void change({ show_grid: event.currentTarget.checked })} />
        {i18n.t('settings.editor.show_grid')}
      </label>
    </section>
    {#if saved}<p role="status">{i18n.t('settings.appearance.saved')}</p>{/if}
  {/if}
</section>
