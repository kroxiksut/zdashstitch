<script lang="ts">
  import type { I18n } from '../i18n';
  import { Banner, Button, Icon, Skeleton, TextField, resolveText } from '../components';
  import { replaceHostCodeText } from './messages';
  import type { ReplaceHostStore } from './store.svelte';

  // «Новый узел» of ReplaceHost (P1-05C, mockup 09-02 with single choice): search among the hosts
  // visible to the draft's connection, one radio per host with groups and the coverage of the
  // replaced host's templates. No order and no role: those are Clone (P3).
  let { i18n, store }: { i18n: I18n; store: ReplaceHostStore } = $props();

  const source = $derived(store.sourceHost);
  const result = $derived(store.searchResult);
  const total = $derived(result?.source_templates?.length ?? null);

  function submit(event: SubmitEvent): void {
    event.preventDefault();
    void store.searchTargets();
  }
</script>

<section class="card new-host" aria-labelledby="new-host-title">
  <h3 id="new-host-title">{i18n.t('bindings.new_host.legend', { host: source?.name ?? '—' })}</h3>
  <form class="search-row" onsubmit={submit} role="search">
    <TextField {i18n} id="new-host-search" label={{ id: 'bindings.new_host.search' }} bind:value={store.searchInput} />
    <Button {i18n} type="submit" label={{ id: 'bindings.new_host.search_action' }} busy={store.searchLoading}>
      {#snippet leadingIcon()}<Icon name="search" />{/snippet}
    </Button>
  </form>
  <p class="muted">{i18n.t('bindings.new_host.same_connection')}</p>

  <div aria-live="polite" aria-busy={store.searchLoading}>
    {#if store.searchLoading}
      <Skeleton {i18n} label={{ id: 'bindings.progress.search' }} lines={3} />
    {:else if store.searchFailure}
      <Banner {i18n} heading={{ id: 'bindings.command_failed' }} body={replaceHostCodeText(store.searchFailure.code, store.searchFailure.params)} tone="danger" />
    {:else if result}
      {#if result.hosts.length === 0}
        <p>{i18n.t('bindings.new_host.empty')}</p>
      {:else}
        <fieldset class="choice-group host-choices">
          <legend class="visually-hidden">{i18n.t('bindings.new_host.legend', { host: source?.name ?? '—' })}</legend>
          {#each result.hosts as host (host.hostid)}
            <label class="choice host-choice">
              <input type="radio" name="bindings-target" value={host.hostid} checked={store.target?.hostid === host.hostid} onchange={() => store.chooseTarget(host.hostid)} />
              <span class="host-name">{host.name}</span>
              <span class="description">
                {#if host.groups.length > 0}<span>{i18n.t('bindings.new_host.groups', { groups: host.groups.join(', ') })}</span>{/if}
                {#if total !== null && total > 0}
                  {#if host.source_templates_found < total}
                    <span class="marker"><Icon name="alert-triangle" size={16} />{i18n.t('bindings.new_host.partial', { found: host.source_templates_found, total })}</span>
                  {:else}
                    <span>{i18n.t('bindings.targets.templates_coverage', { found: host.source_templates_found, total })}</span>
                  {/if}
                {/if}
              </span>
            </label>
          {/each}
        </fieldset>
        {#if result.truncated}<p class="muted">{i18n.t('bindings.new_host.truncated', { count: result.hosts.length })}</p>{/if}
      {/if}
      {#if total === 0 && source}<p class="muted">{i18n.t('bindings.new_host.no_templates', { host: source.name })}</p>{/if}
    {/if}
  </div>
  <p class="note"><Icon name="info-circle" />{i18n.t('bindings.new_host.visible_only')}</p>
  {#if store.target}<p role="status">{resolveText(i18n, { id: 'bindings.new_host.chosen', params: { host: store.target.name } })}</p>{/if}
</section>
