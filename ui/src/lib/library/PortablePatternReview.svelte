<script lang="ts">
  import { untrack } from 'svelte';
  import type { I18n } from '../i18n';
  import { CSRF_HEADER, loadCsrfToken } from '../session';
  import type { OfflinePage, OfflineDashboard, PortablePageWidget } from './client';

  let { i18n, draft, enabled }: { i18n: I18n; draft: OfflinePage | OfflineDashboard; enabled: boolean } = $props();
  type Pattern = { page: number; widget: number; field: number; name: string; mask: string; role: string };
  type Host = { hostid: string; name: string };
  type VisibleMatch = { hostid: string; host_name: string; itemid: string | null; item_name: string | null; selected: boolean };
  type Review = { fingerprint: string; visible_matches: VisibleMatch[] };
  let patterns = $derived.by(() => {
    const root = draft.bundle.project.root;
    const pages = 'pages' in root ? root.pages : [root];
    const result: Pattern[] = [];
    pages.forEach((page, pi) => page.widgets.forEach((widget: PortablePageWidget, wi) => {
      if (widget.type !== 'zabbix') return;
      widget.fields.forEach((raw, fi) => {
        const field = raw as { name?: unknown; value?: { kind?: unknown; value?: unknown; role?: unknown } };
        if (typeof field.name === 'string' && field.value?.kind === 'dynamic_pattern' && typeof field.value.value === 'string' && typeof field.value.role === 'string')
          result.push({ page: pi, widget: wi, field: fi, name: `${page.name} / ${widget.name} / ${field.name}`, mask: field.value.value, role: field.value.role });
      });
    }));
    return result;
  });
  let selected = $state(0);
  let search = $state('');
  let hosts = $state<Host[]>([]);
  let selections = $state<Record<string, string>>({});
  let truncated = $state(false);
  let review = $state<Review | null>(null);
  let acknowledgeOutside = $state(false);
  let hasOutside = $derived(review?.visible_matches.some(match => !match.selected) ?? false);
  let confirmed = $state(false);
  let checkedDraft = $state(untrack(() => draft.local_id));
  let checkedRevision = $state(untrack(() => draft.revision));
  let busy = $state(false);
  let failed = $state(false);
  let pattern = $derived(patterns[selected]);
  let role = $derived(pattern ? draft.bundle.project.roles[pattern.role] : undefined);
  let slots = $derived(role?.slots?.length ? role.slots : ['target']);
  $effect(() => {
    if (checkedDraft !== draft.local_id || checkedRevision !== draft.revision || !enabled) {
      if (checkedDraft !== draft.local_id || !enabled) {
        hosts = [];
        selections = {};
        truncated = false;
      }
      checkedDraft = draft.local_id;
      checkedRevision = draft.revision;
      clearReview();
    }
  });
  const path = (action: string) => `/api/drafts/${encodeURIComponent(draft.local_id)}/portable-pattern-${action}`;
  function clearReview() { review = null; confirmed = false; acknowledgeOutside = false; }
  async function findHosts() {
    const localId = draft.local_id;
    const revision = draft.revision;
    busy = true; failed = false; clearReview();
    try {
      const response = await fetch(`${path('hosts')}?${new URLSearchParams({ search: search.trim() })}`);
      if (!response.ok) throw new Error();
      const result = await response.json() as { hosts: Host[]; truncated: boolean };
      if (!enabled || localId !== draft.local_id || revision !== draft.revision) return;
      hosts = result.hosts; truncated = result.truncated; selections = {};
    } catch { failed = true; } finally { busy = false; }
  }
  function request() {
    if (!pattern) throw new Error();
    return { revision: draft.revision, page_index: pattern.page, widget_index: pattern.widget, field_index: pattern.field,
      targets: slots.map(slot_id => ({ slot_id, host: { hostid: selections[slot_id], host: '', name: '' } })) };
  }
  async function send(confirm: boolean) {
    if (confirm && (!review || (hasOutside && !acknowledgeOutside))) return;
    const localId = draft.local_id;
    const revision = draft.revision;
    const endpoint = path(confirm ? 'confirm' : 'review');
    if (!confirm) clearReview();
    busy = true; failed = false;
    try {
      const body = confirm ? { review: request(), fingerprint: review?.fingerprint,
        acknowledge_visible_outside: acknowledgeOutside } : request();
      const token = await loadCsrfToken(fetch);
      const response = await fetch(endpoint, { method: 'POST', headers: { 'Content-Type': 'application/json', [CSRF_HEADER]: token }, body: JSON.stringify(body) });
      if (!response.ok) throw new Error();
      const result = await response.json() as Review;
      if (!enabled || localId !== draft.local_id || revision !== draft.revision) return;
      review = result;
      confirmed = confirm;
    } catch { clearReview(); failed = true; } finally { busy = false; }
  }
</script>

{#if patterns.length}
  <section aria-label={i18n.t('portable_review.title')}>
    <h3>{i18n.t('portable_review.title')}</h3>
    <p>{i18n.t('portable_review.help')}</p>
    <label>{i18n.t('portable_review.pattern')}
      <select bind:value={selected} disabled={busy || !enabled} onchange={() => { selections = {}; clearReview(); }}>
        {#each patterns as entry, index}<option value={index}>{entry.name}: {entry.mask}</option>{/each}
      </select>
    </label>
    <label>{i18n.t('portable_review.search')} <input bind:value={search} maxlength="128" disabled={busy || !enabled} /></label>
    <button type="button" disabled={!enabled || busy} onclick={() => void findHosts()}>{i18n.t('portable_review.find')}</button>
    {#if truncated}<p role="status">{i18n.t('portable_review.truncated')}</p>{/if}
    {#each slots as slot}
      <label>{slot}
        <select disabled={busy || !enabled} value={selections[slot] ?? ''} onchange={(event) => { selections = { ...selections, [slot]: event.currentTarget.value }; clearReview(); }}>
          <option value="">{i18n.t('portable_review.choose')}</option>
          {#each hosts as host}<option value={host.hostid}>{host.name}</option>{/each}
        </select>
      </label>
    {/each}
    <button type="button" disabled={!enabled || busy || !slots.every(slot => selections[slot])} onclick={() => void send(false)}>{i18n.t('portable_review.preview')}</button>
    {#if review}
      <p>{i18n.t('portable_review.matches', { count: review.visible_matches.length })}</p>
      <ul>
        {#each review.visible_matches as match (`${match.hostid}:${match.itemid ?? ''}`)}
          <li>{match.host_name}{#if match.item_name} · {match.item_name}{/if}
            {#if !match.selected} · {i18n.t('portable_review.outside')}{/if}
          </li>
        {/each}
      </ul>
      {#if hasOutside}
        <label><input type="checkbox" bind:checked={acknowledgeOutside} disabled={!enabled || busy || confirmed} />
          {i18n.t('portable_review.acknowledge_outside')}
        </label>
      {/if}
      <button type="button" disabled={!enabled || busy || confirmed || (hasOutside && !acknowledgeOutside)} onclick={() => void send(true)}>{i18n.t('portable_review.confirm')}</button>
      {#if confirmed}<p role="status">{i18n.t('portable_review.confirmed')}</p>{/if}
    {/if}
    {#if failed}<p role="alert">{i18n.t('portable_review.failed')}</p>{/if}
  </section>
{/if}
