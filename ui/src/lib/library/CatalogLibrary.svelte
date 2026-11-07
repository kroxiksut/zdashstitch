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

  async function inspect(native = false): Promise<void> {
    if (busy || (!native && files.length === 0)) return;
    busy = true;
    preview = null;
    savedCount = null;
    importError = false;
    try {
      if (native) {
        const selection = await client.choose();
        if (selection.files.length > 0) {
          files = [];
          input.value = '';
          preview = selection;
        }
      } else {
        preview = await client.inspect(files);
      }
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

<div class="screen">
  <h1>{i18n.t('library.title')}</h1>
  <section aria-labelledby="catalog-import-title" id="template-import">
    <h2 id="catalog-import-title">{i18n.t('startup.action.import_files')}</h2>
    <p id="catalog-import-help">{i18n.t('template_import.help')}</p>
    <div class="actions">
      <Button {i18n} label={{ id: 'template_import.choose_native' }} disabled={busy} onclick={() => void inspect(true)} />
      <div>
        <label for="template-import-file">{i18n.t('template_import.choose')}</label>
        <input id="template-import-file" type="file" multiple accept=".yaml,.yml,.json,.xml"
          aria-describedby="catalog-import-help" disabled={busy} bind:this={input}
          onchange={(event) => select(event.currentTarget.files)}
          ondragover={(event) => event.preventDefault()}
          ondrop={(event) => { event.preventDefault(); select(event.dataTransfer?.files ?? null); }} />
      </div>
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
  <section aria-labelledby="catalog-list-title">
    <h2 id="catalog-list-title">{i18n.t('template_import.catalogs')}</h2>
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
  section { margin-block: var(--space-5); }
  .actions { display: flex; flex-wrap: wrap; align-items: end; gap: var(--space-3); }
  label { display: block; margin-block-end: var(--space-1); }
  input { max-inline-size: 100%; }
  li { overflow-wrap: anywhere; }
</style>
