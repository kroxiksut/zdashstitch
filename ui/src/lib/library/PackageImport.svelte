<script lang="ts">
  import type { I18n } from '../i18n';
  import { createCatalogClient, type PackagePreview, type PackageImportResult, type PackageSelection } from './client';
  let { i18n, onchanged }: { i18n: I18n; onchanged?: () => void } = $props();
  const client = createCatalogClient();
  let files = $state<File[]>([]);
  let preview = $state<PackagePreview[]>([]);
  let id = $state<string | null>(null);
  let selected = $state<Record<string, string>>({});
  let results = $state<PackageImportResult[]>([]);
  let busy = $state(false);
  let failed = $state(false);
  const ready = $derived(preview.length > 0 && preview.every((item) => item.outcome.status === 'ready' && (item.outcome.version !== 'v1' || !!selected[item.file])));
  async function inspect(): Promise<void> {
    if (!files.length || busy) return;
    busy = true; failed = false; preview = []; id = null; selected = {}; results = [];
    try { const response = await client.inspectPackages(files); id = response.id; preview = response.files; }
    catch { failed = true; }
    finally { busy = false; }
  }
  async function save(): Promise<void> {
    if (!id || !ready || busy) return;
    busy = true; failed = false;
    try {
      const selections: PackageSelection[] = preview.map((item) => ({ file: item.file, ...(item.outcome.version === 'v1' ? JSON.parse(selected[item.file]) as { dashboard_id: string; page_id: string | null } : { dashboard_id: null, page_id: null }) }));
      results = await client.importPackages(id, selections); id = null; preview = [];
      onchanged?.();
    } catch { failed = true; id = null; }
    finally { busy = false; }
  }
</script>

<section class="card" aria-labelledby="package-import-title">
  <h2 id="package-import-title">{i18n.t('package_import.title')}</h2>
  <p>{i18n.t('package_import.help')}</p>
  <p class="muted">{i18n.t('package_import.limits')}</p>
  <p class="muted">{i18n.t('package_import.hash_hint')}</p>
  <label for="package-import-files">{i18n.t('package_import.title')}</label>
  <input id="package-import-files" type="file" accept=".zip,.yaml,.yml,application/zip,application/yaml,text/yaml" multiple disabled={busy} onchange={(event) => { files = Array.from(event.currentTarget.files ?? []); preview = []; id = null; results = []; }} />
  <button type="button" disabled={busy || files.length === 0} onclick={() => void inspect()}>{i18n.t('package_import.preview')}</button>
  {#each preview as item (item.file)}
    <div>
      <strong>{item.file}</strong>
      <p>{i18n.t('package_import.size', { bytes: item.size_bytes })}</p>
      <p>{i18n.t('package_import.sha256')}: <code>{item.sha256}</code></p>
      {#if item.entry_hashes}<details><summary>{i18n.t('package_import.file_hashes')}</summary><ul>{#each Object.entries(item.entry_hashes) as [name, hash] (name)}<li><bdi>{name}</bdi>: <code>{hash}</code></li>{/each}</ul></details>{/if}
      {#if item.outcome.status === 'invalid'}<p role="alert">{i18n.t('package_import.invalid')}</p>
      {:else if item.outcome.version === 'v1'}
        <label for={`package-select-${item.file}`}>{i18n.t('package_import.choose')}</label>
        <select id={`package-select-${item.file}`} bind:value={selected[item.file]}>
          <option value="">{i18n.t('package_import.choose')}</option>
          {#each item.outcome.dashboards ?? [] as dashboard}
            <option value={JSON.stringify({ dashboard_id: dashboard.id, page_id: null })}>{i18n.t('package_import.dashboard')}: {dashboard.name}</option>
            {#each dashboard.pages as page}
              <option value={JSON.stringify({ dashboard_id: dashboard.id, page_id: page.id })}>{i18n.t('package_import.page')}: {dashboard.name} / {page.name}</option>
            {/each}
          {/each}
        </select>
      {:else}<p>{item.outcome.kind === 'page' ? i18n.t('package_import.page') : i18n.t('package_import.dashboard')}: {item.outcome.name}</p>
        <p>{i18n.t('package_import.summary', { pages: item.outcome.pages ?? 0, widgets: item.outcome.widgets ?? 0, catalogs: item.outcome.catalogs ?? 0 })}</p>
        {#if item.outcome.declared_compatibility}
          {#if item.outcome.declared_compatibility.maximum_exclusive}
            <p>{i18n.t('package_import.compatibility_range', { minimum: item.outcome.declared_compatibility.minimum, maximum: item.outcome.declared_compatibility.maximum_exclusive })}</p>
          {:else}<p>{i18n.t('package_import.compatibility_min', { version: item.outcome.declared_compatibility.minimum })}</p>{/if}
        {/if}
        {#if item.outcome.registry_compatibility}
          <p>{i18n.t(item.outcome.registry_compatibility.knowledge === 'known' ? 'package_import.registry_known' : item.outcome.registry_compatibility.knowledge === 'unknown' ? 'package_import.registry_unknown' : 'package_import.registry_incompatible')}</p>
          {#if item.outcome.registry_compatibility.calculated}
            {#if item.outcome.registry_compatibility.calculated.maximum_exclusive}
              <p>{i18n.t('package_import.registry_range', { minimum: item.outcome.registry_compatibility.calculated.minimum, maximum: item.outcome.registry_compatibility.calculated.maximum_exclusive })}</p>
            {:else}<p>{i18n.t('package_import.registry_min', { version: item.outcome.registry_compatibility.calculated.minimum })}</p>{/if}
          {/if}
          {#if item.outcome.registry_compatibility.declared_outside_registry}<p role="alert">{i18n.t('package_import.registry_mismatch')}</p>{/if}
        {/if}
      {/if}
    </div>
  {/each}
  {#if id}<button type="button" disabled={busy || !ready} onclick={() => void save()}>{i18n.t('package_import.import')}</button>{/if}
  {#each results as result (result.file)}<p role="status">{result.file}: {i18n.t(result.outcome.status === 'saved' ? 'package_import.saved' : 'package_import.failed')}</p>{/each}
  {#if failed}<p role="alert">{i18n.t('package_import.failed')}</p>{/if}
</section>
