<script lang="ts">
  import type { I18n, MessageId } from '../i18n';
  import { createCatalogClient } from './client';
  let { i18n, messageId, disabled, run, onDone, onError }: { i18n: I18n; messageId: MessageId; disabled: boolean; run: (id: string) => Promise<{ cancelled: boolean }>; onDone: () => void; onError: () => void } = $props();
  const client = createCatalogClient();
  let active = $state(false);
  let phase = $state<'created' | 'choosing' | 'preparing' | 'scanning' | 'transforming' | 'validating' | 'packaging' | 'verifying' | 'writing' | 'committing' | 'done' | 'failed' | 'cancelled'>('created');
  let cancellable = $state(false);
  let jobId = $state('');
  let cancelPending = $state(false);
  let tooLate = $state(false);
  let started = $state(false);
  const phaseLabels = { created: 'export_progress.created', choosing: 'export_progress.choosing', preparing: 'export_progress.preparing', scanning: 'export_progress.scanning', transforming: 'export_progress.transforming', validating: 'export_progress.validating', packaging: 'export_progress.packaging', verifying: 'export_progress.verifying', writing: 'export_progress.writing', committing: 'export_progress.committing', done: 'export_progress.done', failed: 'export_progress.failed', cancelled: 'export_progress.cancelled' } as const satisfies Record<typeof phase, MessageId>;
  async function poll(id: string): Promise<void> {
    while (active && jobId === id) {
      try { const status = await client.exportJobStatus(id); if (jobId !== id || !active) return; phase = status.phase; cancellable = status.cancellable; }
      catch { cancellable = false; }
      await new Promise((resolve) => setTimeout(resolve, 300));
    }
  }
  async function start(): Promise<void> {
    if (active || disabled) return;
    active = true; started = true; tooLate = false; phase = 'created'; cancellable = false;
    try {
      const job = await client.createExportJob(); jobId = job.id;
      void poll(job.id);
      const result = await run(job.id);
      if (result.cancelled) { try { await client.cancelExportJob(job.id); } catch { /* The job may already have expired. */ } }
      phase = result.cancelled ? 'cancelled' : 'done';
      if (!result.cancelled) onDone();
    } catch { phase = 'failed'; onError(); }
    finally { active = false; jobId = ''; cancellable = false; }
  }
  async function cancel(): Promise<void> {
    if (!jobId || !cancellable || cancelPending) return;
    cancelPending = true;
    try { if (!(await client.cancelExportJob(jobId)).cancelled) tooLate = true; }
    catch { tooLate = true; }
    finally { cancelPending = false; }
  }
</script>

<span>
  <button type="button" disabled={disabled || active} onclick={() => void start()}>{i18n.t(messageId)}</button>
  {#if active}<span role="status">{i18n.t(phaseLabels[phase])}</span> <button type="button" disabled={!cancellable || cancelPending} onclick={() => void cancel()}>{i18n.t('export_progress.cancel')}</button>{/if}
  {#if started && !active && phase === 'cancelled'}<span role="status">{i18n.t('export_progress.cancelled')}</span>{/if}
  {#if tooLate}<span role="status">{i18n.t('export_progress.too_late')}</span>{/if}
</span>
