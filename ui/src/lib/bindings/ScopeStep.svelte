<script lang="ts">
  import type { I18n } from '../i18n';
  import { Icon, StatusBadge } from '../components';
  import { displayName, numberedPageName, widgetTypeName } from '../shell/format';
  import { REFERENCE_LABEL, reasonText } from './messages';
  import { referencesInScope, type HostReference, type InventoryPage } from './model';
  import type { ReplaceHostStore } from './store.svelte';
  import { resolveText } from '../components';

  // «Область» and «Зависимости» of ReplaceHost (P1-05A/B, mockup 09-01): intent, scope, the host to
  // replace among those the draft references, and its references by page with «что будет».
  let { i18n, store, withDependencies }: {
    i18n: I18n;
    store: ReplaceHostStore;
    /** Step «Зависимости»: show the reference table next to the choices. */
    withDependencies: boolean;
  } = $props();

  const inventory = $derived(store.inventory);
  const pages = $derived(inventory?.pages ?? []);
  const isPage = $derived(inventory?.kind === 'page');
  const widgetTotal = $derived(pages.reduce((total, page) => total + page.widgets.length, 0));
  const host = $derived(store.sourceHost);
  const scope = $derived(store.scope);
  const split = $derived(host && scope ? referencesInScope(host, pages, scope) : null);
  const inside = $derived(split?.inside ?? []);
  const changes = $derived(inside.filter((reference) => reference.outcome === 'changes').length);
  const kept = $derived(inside.filter((reference) => reference.outcome === 'kept_on_source'));
  const unverified = $derived(inventory?.unattributed.length ?? 0);

  function widgetName(page: InventoryPage | undefined, index: number): string {
    const widget = page?.widgets.find((candidate) => candidate.widget_index === index);
    return displayName(widget?.name ?? '', widgetTypeName(i18n, widget?.type ?? ''));
  }

  function pageTitle(page: InventoryPage): string {
    const position = pages.indexOf(page) + 1;
    return numberedPageName(i18n, page.name, position);
  }

  function referencesOf(page: InventoryPage): HostReference[] {
    return inside.filter((reference) => reference.page_index === page.page_index);
  }

  function toggle(list: string[], id: string, checked: boolean): string[] {
    return checked ? [...new Set([...list, id])] : list.filter((value) => value !== id);
  }
</script>

<div class="bindings-columns" class:single={!withDependencies}>
  <div class="bindings-choices">
    <fieldset class="card choice-group">
      <legend class="eyebrow">{i18n.t('bindings.intent.title')}</legend>
      <label class="choice">
        <input type="radio" name="bindings-intent" value="replace_host" checked />
        <span>{i18n.t('intent.replace_host')}</span>
      </label>
      <label class="choice">
        <input type="radio" name="bindings-intent" value="clone" disabled aria-describedby="bindings-intent-clone-later" />
        <span>{i18n.t('bindings.intent.clone')}</span>
        <span id="bindings-intent-clone-later" class="description">{i18n.t('common.state.coming_later')}</span>
      </label>
    </fieldset>

    <fieldset class="card choice-group">
      <legend class="eyebrow">{i18n.t('bindings.step.scope')}</legend>
      <label class="choice">
        <input type="radio" name="bindings-scope" value="dashboard" checked={store.scopeKind === 'dashboard'} onchange={() => (store.scopeKind = 'dashboard')} />
        <span>
          {#if isPage}
            {i18n.t('bindings.scope.whole_page', { widgets: widgetTotal })}
          {:else}
            {i18n.t('bindings.scope.whole', { pages: pages.length, widgets: widgetTotal })}
          {/if}
        </span>
      </label>
      {#if !isPage}
        <label class="choice">
          <input type="radio" name="bindings-scope" value="pages" checked={store.scopeKind === 'pages'} onchange={() => (store.scopeKind = 'pages')} />
          <span>{i18n.t('bindings.scope.pages')}</span>
        </label>
      {/if}
      <label class="choice">
        <input type="radio" name="bindings-scope" value="widgets" checked={store.scopeKind === 'widgets'} onchange={() => (store.scopeKind = 'widgets')} />
        <span>{i18n.t('bindings.scope.widgets')}</span>
      </label>

      {#if store.scopeKind === 'pages'}
        <fieldset class="nested-choices">
          <legend>{i18n.t('bindings.scope.choose_pages')}</legend>
          {#each pages as page (page.page_index)}
            {#if page.dashboard_pageid}
              {@const id = page.dashboard_pageid}
              <label class="choice">
                <input
                  type="checkbox"
                  checked={store.scopePages.includes(id)}
                  onchange={(event) => (store.scopePages = toggle(store.scopePages, id, event.currentTarget.checked))}
                />
                <span>{pageTitle(page)}</span>
              </label>
            {/if}
          {/each}
        </fieldset>
      {:else if store.scopeKind === 'widgets'}
        <fieldset class="nested-choices">
          <legend>{i18n.t('bindings.scope.choose_widgets')}</legend>
          {#each pages as page (page.page_index)}
            <p class="muted">{pageTitle(page)}</p>
            {#each page.widgets as widget (widget.widget_index)}
              {#if widget.widgetid}
                {@const id = widget.widgetid}
                <label class="choice">
                  <input
                    type="checkbox"
                    checked={store.scopeWidgets.includes(id)}
                    onchange={(event) => (store.scopeWidgets = toggle(store.scopeWidgets, id, event.currentTarget.checked))}
                  />
                  <span>{displayName(widget.name, widgetTypeName(i18n, widget.type))}</span>
                </label>
              {/if}
            {/each}
          {/each}
        </fieldset>
      {/if}
      {#if scope === null}<p class="field-error" role="alert">{i18n.t('bindings.scope.empty_selection')}</p>{/if}
    </fieldset>

    <fieldset class="card choice-group">
      <legend class="eyebrow">{i18n.t('replace_host.source')}</legend>
      {#if inventory && inventory.hosts.length === 0}
        <p>{i18n.t('bindings.source.none')}</p>
      {/if}
      {#each inventory?.hosts ?? [] as candidate (candidate.hostid)}
        <label class="choice">
          <input type="radio" name="bindings-source" value={candidate.hostid} checked={store.sourceHostid === candidate.hostid} onchange={() => store.setSource(candidate.hostid)} />
          <span class="host-name">{candidate.name}</span>
          <span class="description">{i18n.t('bindings.source.option_detail', { references: candidate.references.length, widgets: candidate.widgets })}</span>
        </label>
      {/each}
      {#if unverified > 0}<p class="muted">{i18n.t('bindings.source.unattributed', { count: unverified })}</p>{/if}
    </fieldset>

    <p class="note"><Icon name="info-circle" />{i18n.t('bindings.draft_only')}</p>
  </div>

  {#if withDependencies}
    <section class="card bindings-deps" aria-labelledby="bindings-deps-title">
      {#if host}
        <div class="section-head">
          <h3 id="bindings-deps-title">{i18n.t('bindings.deps.title', { host: host.name })}</h3>
          <ul class="readiness">
            <li><StatusBadge {i18n} label={{ id: 'bindings.deps.count.changes', params: { count: changes } }} tone="info" /></li>
            <li><StatusBadge {i18n} label={{ id: 'bindings.deps.count.kept', params: { count: kept.length } }} tone="warning" /></li>
            <li><StatusBadge {i18n} label={{ id: 'bindings.deps.count.unverified', params: { count: unverified } }} tone={unverified > 0 ? 'danger' : 'success'} /></li>
          </ul>
        </div>
        {#if split && split.outside > 0}
          <p class="note warning-note"><Icon name="alert-triangle" />{i18n.t('bindings.scope.partial_warning', { host: host.name, count: split.outside })}</p>
        {/if}
        {#each pages as page (page.page_index)}
          {@const references = referencesOf(page)}
          {#if references.length > 0}
            <details class="deps-page" open>
              <summary>
                <span class="deps-page-title">{pageTitle(page)}</span>
                <span class="muted">{i18n.t('bindings.deps.page_summary', { widgets: new Set(references.map((reference) => reference.widget_index)).size, references: references.length })}</span>
              </summary>
              <div class="table-scroll">
                <table>
                  <caption class="visually-hidden">{pageTitle(page)}</caption>
                  <thead>
                    <tr>
                      <th scope="col">{i18n.t('bindings.deps.column.widget')}</th>
                      <th scope="col">{i18n.t('bindings.deps.column.type')}</th>
                      <th scope="col">{i18n.t('bindings.deps.column.reference')}</th>
                      <th scope="col">{i18n.t('bindings.deps.column.outcome')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each references as reference (`${reference.widget_index}:${reference.field}`)}
                      <tr>
                        <td>{widgetName(page, reference.widget_index)}</td>
                        <td>{widgetTypeName(i18n, reference.widget_type)}</td>
                        <td>{resolveText(i18n, REFERENCE_LABEL[reference.kind])}</td>
                        <td>
                          {#if reference.outcome === 'changes'}
                            <span class="marker"><Icon name="pencil" size={18} />{i18n.t('bindings.outcome.changes')}</span>
                          {:else}
                            <span class="marker"><Icon name="ban" size={18} />{i18n.t('bindings.outcome.kept', { host: host.name })}</span>
                            {#if reference.reason}<span class="muted reason">{resolveText(i18n, reasonText(reference.reason))}</span>{/if}
                          {/if}
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </details>
          {/if}
        {/each}
        {#if kept.length > 0}
          <div class="kept-box" role="note">
            <p class="kept-title"><Icon name="ban" />{i18n.t('bindings.kept.title', { count: new Set(kept.map((reference) => `${reference.page_index}:${reference.widget_index}`)).size, host: host.name })}</p>
            <p>{i18n.t('bindings.kept.not_blocking')}</p>
          </div>
        {/if}
        <p class="muted">{i18n.t('bindings.deps.pattern_rule', { host: host.name })}</p>
      {:else}
        <h3 id="bindings-deps-title">{i18n.t('bindings.step.dependencies')}</h3>
        <p class="muted">{i18n.t('bindings.deps.choose_host')}</p>
      {/if}
    </section>
  {/if}
</div>
