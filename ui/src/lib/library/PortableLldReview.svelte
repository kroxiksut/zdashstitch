<script lang="ts">
  import type { I18n } from '../i18n';
  import { CSRF_HEADER, loadCsrfToken } from '../session';
  import type { OfflinePage, OfflineDashboard } from './client';
  let { i18n, draft, enabled }: { i18n: I18n; draft: OfflinePage | OfflineDashboard; enabled: boolean } = $props();
  type Entry = {
    page_index: number; widget_index: number; field_index: number;
    page_name: string; widget_name: string; field_name: string | null;
    role: string; slot: string | null; metric_name: string | null; source_key: string | null;
  };
  type Host = { hostid: string; name: string };
  type Candidate = { itemid: string; name: string; key: string };
  type Review = { candidates: Candidate[]; fingerprint: string; host?: Host };
  let entries = $state<Entry[]>([]);
  let selected = $state(0); let search = $state(''); let hosts = $state<Host[]>([]);
  let hostid = $state(''); let itemid = $state(''); let review = $state<Review | null>(null);
  let truncated = $state(false); let confirmed = $state(false); let failed = $state(false); let busy = $state(false);
  let listFailed = $state(false);
  $effect(() => {
    const {local_id, revision} = draft;
    const abort = new AbortController();
    entries = []; selected = 0; hosts = []; hostid = ''; clear(); listFailed = false; failed = false; truncated = false;
    if (enabled) void loadEntries(local_id, revision, abort.signal);
    return () => abort.abort();
  });
  async function loadEntries(id: string, revision: number, signal: AbortSignal) {
    try {
      const response = await fetch(`/api/drafts/${encodeURIComponent(id)}/portable-sources?${new URLSearchParams({revision: String(revision)})}`, {signal});
      if (!response.ok) throw new Error();
      const result = await response.json() as Entry[];
      if (!signal.aborted) entries = result;
    } catch { if (!signal.aborted) listFailed = true; }
  }
  function clear() { review = null; itemid = ''; confirmed = false; }
  const path = (suffix: string) => `/api/drafts/${encodeURIComponent(draft.local_id)}/${suffix}`;
  function request() {
    const entry = entries[selected]; if (!entry) throw new Error();
    const {page_index, widget_index, field_index} = entry;
    return {revision: draft.revision, page_index, widget_index, field_index, hostid};
  }
  const isCurrent = (localId: string, revision: number) => enabled && localId === draft.local_id && revision === draft.revision;
  async function find() {
    const { local_id, revision } = draft; busy = true; failed = false; clear();
    try { const response = await fetch(`${path('portable-pattern-hosts')}?${new URLSearchParams({search: search.trim()})}`); if (!response.ok) throw new Error();
      const result = await response.json() as {hosts: typeof hosts; truncated: boolean}; if (!isCurrent(local_id, revision)) return;
      hosts = result.hosts; truncated = result.truncated; hostid = '';
    } catch { failed = true; } finally { busy = false; }
  }
  async function send(confirm: boolean) {
    const { local_id, revision } = draft; busy = true; failed = false;
    try {
      const token = await loadCsrfToken(fetch);
      if (!isCurrent(local_id, revision)) return;
      const body = confirm ? {review: request(), itemid: review?.host ? null : itemid, fingerprint: review?.fingerprint} : request();
      const response = await fetch(path(confirm ? 'portable-lld-confirm' : 'portable-lld-review'), {method:'POST', headers:{'Content-Type':'application/json',[CSRF_HEADER]:token},body:JSON.stringify(body)});
      if (!response.ok) throw new Error(); const result = await response.json() as Review; if (!isCurrent(local_id, revision)) return;
      review = result; confirmed = confirm;
    } catch { clear(); failed = true; } finally { busy = false; }
  }
  async function reopen() {
    const entry = entries[selected]; if (!entry) return;
    const { local_id, revision } = draft; busy = true; failed = false; clear();
    try {
      const token = await loadCsrfToken(fetch);
      if (!isCurrent(local_id, revision)) return;
      const {page_index, widget_index, field_index} = entry;
      const response = await fetch(path('portable-lld-reopen'), {method:'POST', headers:{'Content-Type':'application/json',[CSRF_HEADER]:token}, body:JSON.stringify({revision, page_index, widget_index, field_index})});
      if (!response.ok) throw new Error();
      const result = await response.json() as {review: Review; hostid: string; itemid: string | null};
      if (!isCurrent(local_id, revision)) return;
      if (!hosts.some(host => host.hostid === result.hostid)) hosts = [...hosts, {hostid: result.hostid, name: result.review.host?.name ?? i18n.t('portable_lld.saved_host')}];
      hostid = result.hostid; review = result.review; itemid = result.itemid ?? ''; confirmed = true;
    } catch { failed = true; } finally { busy = false; }
  }
</script>
{#if entries.length}
  <section aria-label={i18n.t('portable_lld.title')}>
    <h3>{i18n.t('portable_lld.title')}</h3><p>{i18n.t('portable_lld.help')}</p>
    <label>{i18n.t('portable_lld.field')} <select bind:value={selected} disabled={busy || !enabled} onchange={clear}>{#each entries as entry, index}<option value={index}>{[entry.page_name, entry.widget_name, entry.field_name, entry.role, entry.slot, entry.metric_name, entry.source_key].filter(Boolean).join(' / ')}</option>{/each}</select></label>
    <button type="button" disabled={busy || !enabled} onclick={() => void reopen()}>{i18n.t('portable_lld.reopen')}</button>
    <label>{i18n.t('portable_review.search')} <input bind:value={search} maxlength="128" disabled={busy || !enabled} /></label>
    <button type="button" disabled={busy || !enabled} onclick={() => void find()}>{i18n.t('portable_review.find')}</button>
    {#if truncated}<p>{i18n.t('portable_review.truncated')}</p>{/if}
    <label>{i18n.t('portable_review.choose')} <select bind:value={hostid} disabled={busy || !enabled} onchange={clear}><option value="">{i18n.t('portable_review.choose')}</option>{#each hosts as host}<option value={host.hostid}>{host.name}</option>{/each}</select></label>
    <button type="button" disabled={busy || !enabled || !hostid} onclick={() => void send(false)}>{i18n.t('portable_lld.find')}</button>
    {#if review}
      {#if review.host}
        <p>{review.host.name}</p>
      {:else}
        <label>{i18n.t('portable_lld.candidate')} <select bind:value={itemid} disabled={busy || !enabled || confirmed}><option value="">{i18n.t('portable_lld.choose')}</option>{#each review.candidates as candidate}<option value={candidate.itemid}>{candidate.name}: {candidate.key}</option>{/each}</select></label>
        {#if review.candidates.length === 0}<p>{i18n.t('portable_lld.empty')}</p>{/if}
      {/if}
      <button type="button" disabled={busy || !enabled || (!review.host && !itemid) || confirmed} onclick={() => void send(true)}>{i18n.t('portable_lld.confirm')}</button>
      {#if confirmed}<p role="status">{i18n.t('portable_lld.confirmed')}</p>{/if}
    {/if}
    {#if failed}<p role="alert">{i18n.t('portable_lld.failed')}</p>{/if}
  </section>
{/if}
{#if listFailed}<p role="alert">{i18n.t('portable_lld.failed')}</p>{/if}
