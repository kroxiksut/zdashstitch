<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiFailure } from '../api';
  import { Button } from '../components';
  import type { I18n } from '../i18n';
  import { createCatalogClient, type CatalogPreview, type CatalogSummary } from './client';

  let { i18n }: { i18n: I18n } = $props();
  const client = createCatalogClient();
  let catalogs = $state<CatalogSummary[] | null>(null);
  let listError = $state(false);
  let files = $state<File[]>([]);
  let preview = $state<CatalogPreview | null>(null);
  let busy = $state(false);
  let importError = $state(false);
  let savedCount = $state<number | null>(null);
  let input: HTMLInputElement;

  async function reload(): Promise<void> {
    listError = false;
    try {
      catalogs = await client.list();
    } catch {
      listError = true;
    }
  }

  onMount(() => { void reload(); });

  function select(selection: FileList | null): void {
    if (busy || !selection) return;
    files = Array.from(selection);
    preview = null;
    importError = false;
    savedCount = null;
  }

  async function inspect(): Promise<void> {
    if (busy || files.length === 0) return;
    busy = true;
    preview = null;
    savedCount = null;
    importError = false;
    try {
      preview = await client.inspect(files);
    } catch {
      importError = true;
    } finally {
      busy = false;
    }
  }

  async function save(): Promise<void> {
    if (busy || !preview?.id || !preview.files.some((file) => file.status === 'ready')) return;
    busy = true;
    importError = false;
    try {
      const saved = await client.run(preview.id);
      savedCount = saved.length;
      preview = null;
      files = [];
      input.value = '';
      await reload();
    } catch (error) {
      importError = true;
      preview = null;
      // If the local server handled the request, its upload set is already spent.
      if (!(error instanceof ApiFailure)) await reload();
    } finally {
      busy = false;
    }
  }
</script>

<div class="screen library-screen">
  <header class="screen-head">
    <h1>{i18n.t('library.title')}</h1>
  </header>
  <section class="card import-card" aria-labelledby="catalog-import-title" id="template-import">
    <h2 id="catalog-import-title">{i18n.t('startup.action.import_files')}</h2>
    <p id="catalog-import-help">{i18n.t('template_import.help')}</p>
    <p class="muted">{i18n.t('template_import.source_steps')}</p>
    <p class="muted">{i18n.t('template_import.server_future')}</p>
    <div class="actions">
      <label class="button-link browser-select" for="template-import-file"
        ondragover={(event) => event.preventDefault()}
        ondrop={(event) => { event.preventDefault(); select(event.dataTransfer?.files ?? null); }}
      >{i18n.t('template_import.choose')}</label>
      <input class="file-input" id="template-import-file" type="file" multiple accept=".yaml,.yml,.json,.xml"
        aria-describedby="catalog-import-help" disabled={busy} bind:this={input}
        onchange={(event) => select(event.currentTarget.files)} />
      {#if files.length > 0}<span class="muted">{i18n.t('template_import.selected', { count: files.length })}</span>{/if}
      <Button {i18n} label={{ id: 'template_import.inspect' }} disabled={busy || files.length === 0}
        onclick={() => void inspect()} />
    </div>
    <div aria-live="polite">
      {#if busy}<p>{i18n.t('common.state.loading')}</p>{/if}
      {#if importError}<p role="alert">{i18n.t('template_import.action_failed')}</p>{/if}
      {#if savedCount !== null}<p>{i18n.t('template_import.saved', { count: savedCount })}</p>{/if}
      {#if preview}
        <ul class="preview">
          {#each preview.files as file}
            <li>
              <bdi>{file.file}</bdi>
              {#if file.status === 'ready'}
                <p>{i18n.t('template_import.becomes', { count: file.catalogs.length })}</p>
                <ul>
                  {#each file.catalogs as catalog}
                    <li>
                      <bdi>{catalog.name}</bdi> {i18n.t('template_import.preview_counts', {
                        metrics: catalog.metrics, rules: catalog.discovery_rules, hints: catalog.metrics_with_hints
                      })}
                      {#if catalog.version > 1}<p>{i18n.t('template_import.new_version')}</p>{/if}
                      {#if catalog.diagnostics.includes('source_partial') || catalog.omitted_template_dashboards > 0}
                        <p>{i18n.t('template_import.partial')}</p>
                      {/if}
                      {#if catalog.diagnostics.includes('missing_parent_template')}
                        <p>{i18n.t('template_import.missing_parent')}</p>
                      {/if}
                      {#if catalog.diagnostics.includes('unresolved_macros')}
                        <p>{i18n.t('template_import.unresolved_macros')}</p>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {:else}
                <p>{i18n.t('template_import.rejected', { code: file.code })}
                  {#if file.line !== null && file.column !== null}
                    {i18n.t('template_import.position', { line: file.line, column: file.column })}
                  {/if}
                </p>
              {/if}
            </li>
          {/each}
        </ul>
        {#if preview.id && preview.files.some((file) => file.status === 'ready')}
          <Button {i18n} variant="primary" label={{ id: 'template_import.save' }} disabled={busy}
            onclick={() => void save()} />
        {/if}
      {/if}
    </div>
  </section>
  <section class="card catalog-card" aria-labelledby="catalog-list-title">
    <h2 id="catalog-list-title">{i18n.t('template_import.catalogs')}</h2>
    <p class="muted">{i18n.t('template_import.catalog_use_later')}</p>
    {#if catalogs && catalogs.length > 0}<p><a href="/drafts">{i18n.t('offline_builder.go_to_drafts')}</a></p>{/if}
    {#if listError}
      <p role="alert">{i18n.t('template_import.list_failed')}</p>
    {:else if catalogs === null}
      <p aria-live="polite">{i18n.t('common.state.loading')}</p>
    {:else if catalogs.length === 0}
      <p>{i18n.t('template_import.empty')}</p>
    {:else}
      <ul class="catalogs">
        {#each catalogs as catalog (`${catalog.catalog_id}/${catalog.version}`)}
          <li>
            <bdi>{catalog.name}</bdi>
            <p>{i18n.t('template_import.version', { version: catalog.version, export_version: catalog.export_format_version ?? '—' })}</p>
            <p>{i18n.t('template_import.preview_counts', {
              metrics: catalog.metrics, rules: catalog.discovery_rules, hints: catalog.metrics_with_hints
            })}</p>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .library-screen { display: grid; gap: var(--space-6); }
  .card { align-content: start; }
  .card h2, .card p { margin: 0; }
  .import-card { max-inline-size: 52rem; }
  .catalog-card { max-inline-size: 52rem; }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-3); margin-block: var(--space-3); }
  .browser-select { cursor: pointer; }
  .file-input { position: absolute; inline-size: 1px; block-size: 1px; padding: 0; border: 0; clip-path: inset(50%); overflow: hidden; }
  .browser-select:has(+ .file-input:focus-visible) { outline: 2px solid currentColor; outline-offset: 2px; }
  .preview, .catalogs { margin-block: var(--space-3); }
  li { overflow-wrap: anywhere; }
</style>
