<script lang="ts">
  import { onMount } from 'svelte';
  import type { I18n } from '../i18n';
  import { Banner, Button, Icon } from '../components';
  import { loadAbout, shortBuildCommit, type AboutInfo } from '../about';
  import logoUrl from '../../../../assets/brand/icons/stezhik-head-1024.png';
  import {
    LICENSES,
    copyText,
    countByFilter,
    describeClient,
    filterRows,
    reportText,
    thirdPartyRows,
    type ComponentFilter
  } from './model';

  // «About and licenses» (P1-12, mockup 06-04). Everything is local: build data comes from the
  // local server, license texts are embedded at build time (tools/build_licenses.py). Texts are
  // rendered as plain preformatted text, never as HTML.
  let { i18n, fetcher = fetch }: { i18n: I18n; fetcher?: typeof fetch } = $props();

  const COLLAPSED_ROWS = 12;
  const rows = thirdPartyRows(LICENSES);
  const counts = countByFilter(rows);
  const FILTERS = [
    { value: 'all', id: 'about.third_party.filter.all' },
    { value: 'icons', id: 'about.third_party.filter.icons' },
    { value: 'rust', id: 'about.third_party.filter.rust' },
    { value: 'npm', id: 'about.third_party.filter.ui' }
  ] as const;

  let info = $state<AboutInfo | null>(null);
  let failed = $state(false);
  let filter = $state<ComponentFilter>('all');
  let query = $state('');
  let showAll = $state(false);
  let copied = $state(false);
  let copyFailed = $state(false);

  const client = describeClient(globalThis.navigator ?? {});
  const visible = $derived(filterRows(rows, filter, query));
  const expanded = $derived(showAll || query.trim() !== '' || visible.length <= COLLAPSED_ROWS);
  const shown = $derived(expanded ? visible : visible.slice(0, COLLAPSED_ROWS));
  const report = $derived(info ? reportText(info, client, i18n.locale) : '');

  onMount(() => {
    loadAbout(fetcher).then(
      (value) => {
        info = value;
      },
      () => {
        failed = true;
      }
    );
  });

  async function copyReport(): Promise<void> {
    copied = false;
    copyFailed = false;
    const result = await copyText(report, globalThis.navigator?.clipboard);
    copied = result === 'copied';
    copyFailed = result === 'unavailable';
  }

  function filterLabel(value: ComponentFilter): string {
    const entry = FILTERS.find((item) => item.value === value) ?? FILTERS[0];
    return i18n.t(entry.id, { count: counts[value] });
  }
</script>

{#snippet licenseBlock(label: string, text: string)}
  <!-- A scrollable text region must be reachable with the keyboard (WCAG 2.1.1). -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <pre class="license-text" tabindex="0" aria-label={label}>{text}</pre>
{/snippet}

<div class="about">
  <header class="screen-head">
    <h1>{i18n.t('about.title')}</h1>
    <p class="hint">{i18n.t('about.subtitle')}</p>
  </header>

  {#if failed}
    <Banner {i18n} tone="warning" heading={{ id: 'about.title' }} body={{ id: 'about.error.unavailable' }} />
  {/if}

  <section class="card" aria-labelledby="about-program">
    <div class="about-program">
      <img class="about-logo" src={logoUrl} alt="" width="64" height="64" />
      <div>
        <h2 id="about-program">{i18n.t('common.product_name')}</h2>
        <p>{i18n.t('about.tagline')}</p>
      </div>
    </div>
    <dl class="facts">
      <dt>{i18n.t('about.field.version')}</dt>
      <dd class="technical">{info ? info.version : '…'}</dd>
      <dt>{i18n.t('about.field.build')}</dt>
      <dd class="technical">{info ? (shortBuildCommit(info) ?? i18n.t('about.field.build_unknown')) : '…'}</dd>
      <dt>{i18n.t('about.field.platform')}</dt>
      <dd class="technical">{client.platform || i18n.t('about.field.build_unknown')}</dd>
    </dl>
    <p class="hint">{i18n.t('about.copyright')}</p>
  </section>

  <section class="card" aria-labelledby="about-licenses">
    <h2 id="about-licenses">{i18n.t('about.license.title')}</h2>
    <dl class="facts">
      <dt>{i18n.t('about.license.code')}</dt>
      <dd>{i18n.t('about.license.code_choice')}</dd>
      <dt>{i18n.t('about.license.docs')}</dt>
      <dd>{i18n.t('about.license.docs_value')}</dd>
    </dl>
    <p class="hint">{i18n.t('about.license.zabbix_note')}</p>
    <p class="hint">{i18n.t('about.license.text_original')}</p>
    <details>
      <summary>{i18n.t('about.license.text_mit')}</summary>
      {@render licenseBlock(i18n.t('about.license.text_mit'), LICENSES.product_licenses.mit)}
    </details>
    <details>
      <summary>{i18n.t('about.license.text_apache')}</summary>
      {@render licenseBlock(i18n.t('about.license.text_apache'), LICENSES.product_licenses.apache)}
    </details>
    <details>
      <summary>{i18n.t('about.license.text')} · {i18n.t('about.license.docs_value')}</summary>
      {@render licenseBlock(i18n.t('about.license.text'), LICENSES.product_licenses.docs)}
    </details>
    <details>
      <summary>{i18n.t('about.license.brand')}</summary>
      {@render licenseBlock(i18n.t('about.license.brand'), LICENSES.brand_provenance)}
    </details>
  </section>

  <section class="card" aria-labelledby="about-report">
    <h2 id="about-report">{i18n.t('about.report.copy')}</h2>
    <p class="hint">{i18n.t('about.report.scope')}</p>
    <div class="actions">
      <Button {i18n} label={{ id: 'about.report.copy' }} disabled={info === null} onclick={() => void copyReport()}>
        {#snippet leadingIcon()}<Icon name="copy" size={16} />{/snippet}
      </Button>
    </div>
    <p class="hint" role="status">{copied ? i18n.t('about.copied') : ''}</p>
    {#if copyFailed}
      <p class="hint">{i18n.t('about.report.copy_failed')}</p>
      <textarea class="technical report-preview" readonly rows="5" aria-label={i18n.t('about.report.preview')} value={report}></textarea>
    {/if}
  </section>

  <section class="card" aria-labelledby="about-third-party">
    <h2 id="about-third-party">{i18n.t('about.third_party.title')}</h2>
    <p class="hint">{i18n.t('about.third_party.subtitle')}</p>
    {#if rows.length <= 1}
      <p role="alert">{i18n.t('about.third_party.missing')}</p>
    {/if}
    <details>
      <summary>{i18n.t('about.license.tabler')}</summary>
      <p class="hint">{i18n.t('about.third_party.icons_summary', { count: LICENSES.icons.icon_names.length })}</p>
      {@render licenseBlock(i18n.t('about.license.tabler'), LICENSES.icons.license_text)}
    </details>
    <div class="filters" role="group" aria-label={i18n.t('about.third_party.filters')}>
      {#each FILTERS as item (item.value)}
        <button type="button" class="chip" aria-pressed={filter === item.value} onclick={() => { filter = item.value; showAll = false; }}>
          {filterLabel(item.value)}
        </button>
      {/each}
    </div>
    <div class="search-field">
      <Icon name="search" size={16} />
      <input
        type="search"
        aria-label={i18n.t('about.third_party.search')}
        placeholder={i18n.t('about.third_party.search')}
        bind:value={query}
      />
    </div>
    {#if visible.length === 0}
      <p>{i18n.t('about.third_party.empty')}</p>
    {:else}
      <div class="table-wrap">
        <table class="components">
          <caption class="visually-hidden">{i18n.t('about.third_party.title')}</caption>
          <thead>
            <tr>
              <th scope="col">{i18n.t('about.third_party.column.component')}</th>
              <th scope="col">{i18n.t('about.third_party.column.version')}</th>
              <th scope="col">{i18n.t('about.third_party.column.license')}</th>
            </tr>
          </thead>
          <tbody>
            {#each shown as row (row.kind + ':' + row.name + ':' + row.version)}
              <tr>
                <th scope="row" class="technical">{row.name}</th>
                <td class="technical">{row.version}</td>
                <td class="technical">{row.license}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if !expanded}
        <p class="hint">{i18n.t('about.third_party.more', { count: visible.length - shown.length })}</p>
        <div class="actions">
          <Button {i18n} variant="quiet" label={{ id: 'about.third_party.show_all', params: { count: visible.length } }} onclick={() => { showAll = true; }} />
        </div>
      {/if}
    {/if}
  </section>
</div>
