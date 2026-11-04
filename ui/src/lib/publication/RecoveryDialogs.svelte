<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Dialog, TechnicalDetails, resolveText } from '../components';
  import { evidenceText, intentText, STATE_LABEL, stateLabel } from './messages';
  import type { DamagedJournal, JournalEntry } from './model';
  import type { RecoveryStore } from './store.svelte';

  // Dialogs of the journal (P1-01G §7, P1-08H, item 4 of 28.10.2026 evening): «Закрыть без
  // сверки» and «Убрать в карантин» only after an explicit confirmation that names the operation,
  // its target and time; «Журнал операции» shows the audit fields of one entry (no payload, no
  // secrets, no server answers exist in the journal). Focus starts on «Отмена».
  let { i18n, store, closing, quarantining, targetText, sourceText, profileText, onclosed, journal = true }: {
    i18n: I18n;
    store: RecoveryStore;
    closing: JournalEntry | null;
    quarantining: DamagedJournal | null;
    targetText: (entry: JournalEntry) => string;
    sourceText: (entry: JournalEntry) => string;
    profileText: (entry: JournalEntry) => string;
    onclosed: () => void;
    /** Renders «Журнал операции»; only one instance per screen does. */
    journal?: boolean;
  } = $props();

  const shown = $derived(store.shown);
</script>

{#if closing}
  <Dialog
    {i18n}
    id="recovery-close"
    open
    title={{ id: 'startup.recovery.close_unverified.title' }}
    description={{
      id: 'startup.recovery.close_unverified.confirm',
      params: { operation_id: closing.operation_id, target: targetText(closing), time: closing.submitted_at ?? closing.confirmed_at }
    }}
    onclose={onclosed}
  >
    {#snippet actions()}
      <Button {i18n} label={{ id: 'common.action.cancel' }} onclick={onclosed} />
      <Button
        {i18n}
        variant="danger"
        label={{ id: 'startup.recovery.close_unverified' }}
        busy={store.busy === closing?.operation_id}
        disabled={store.busy !== null}
        onclick={async () => {
          const id = closing?.operation_id;
          if (id !== undefined) await store.close(id);
          onclosed();
        }}
      />
    {/snippet}
  </Dialog>
{/if}

{#if quarantining}
  <Dialog
    {i18n}
    id="recovery-quarantine"
    open
    title={{ id: 'startup.recovery.quarantine.title' }}
    description={{ id: 'startup.recovery.quarantine.confirm', params: { operation_id: quarantining.operation_id } }}
    onclose={onclosed}
  >
    {#snippet actions()}
      <Button {i18n} label={{ id: 'common.action.cancel' }} onclick={onclosed} />
      <Button
        {i18n}
        variant="danger"
        label={{ id: 'startup.recovery.quarantine' }}
        busy={store.busy === quarantining?.operation_id}
        disabled={store.busy !== null}
        onclick={async () => {
          const id = quarantining?.operation_id;
          if (id !== undefined) await store.quarantine(id);
          onclosed();
        }}
      />
    {/snippet}
  </Dialog>
{/if}

{#if journal && shown}
  <Dialog
    {i18n}
    id="journal-entry"
    open
    title={{ id: 'publication.action.open_journal' }}
    description={{ id: 'publication.journal.description' }}
    onclose={() => (store.shown = null)}
  >
    <dl class="journal-facts">
      <div><dt>{i18n.t('publication.journal.field.operation')}</dt><dd>{resolveText(i18n, intentText(shown.intent))}</dd></div>
      <div><dt>{i18n.t('publication.journal.field.state')}</dt><dd>{resolveText(i18n, STATE_LABEL[shown.state] ?? stateLabel(shown.state))}</dd></div>
      <div><dt>{i18n.t('publication.journal.field.connection')}</dt><dd>{profileText(shown)}</dd></div>
      <div><dt>{i18n.t('publication.journal.field.server_version')}</dt><dd class="technical">{shown.server_version}</dd></div>
      <div><dt>{i18n.t('publication.journal.field.target')}</dt><dd>{targetText(shown)}</dd></div>
      <div><dt>{i18n.t('publication.journal.field.source')}</dt><dd>{sourceText(shown)}</dd></div>
      <div><dt>{i18n.t('publication.journal.field.sends')}</dt><dd>{i18n.number(shown.sends)}</dd></div>
      {#if shown.evidence}<div><dt>{i18n.t('publication.journal.field.evidence')}</dt><dd>{resolveText(i18n, evidenceText(shown.evidence))}</dd></div>{/if}
      {#if shown.closed_by}<div><dt>{i18n.t('publication.journal.field.closed_by')}</dt><dd>{shown.closed_by}</dd></div>{/if}
      <div>
        <dt>{i18n.t('publication.journal.field.transitions')}</dt>
        <dd>
          <ol class="plain-list">
            {#each shown.transitions as transition, index (index)}
              <li>{i18n.t('publication.journal.transition', { state: resolveText(i18n, STATE_LABEL[transition.state] ?? stateLabel(transition.state)), time: transition.at })}</li>
            {/each}
          </ol>
        </dd>
      </div>
    </dl>
    <TechnicalDetails
      {i18n}
      summary={{ id: 'publication.tech.title' }}
      values={[
        { label: { id: 'publication.journal.field.operation_id' }, value: { value: shown.operation_id, kind: 'technical' } },
        { label: { id: 'publication.tech.base' }, value: { value: shown.source_fingerprint, kind: 'technical' } },
        { label: { id: 'publication.tech.local' }, value: { value: shown.expected_fingerprint, kind: 'technical' } }
      ]}
    />
    {#snippet actions()}
      <Button {i18n} label={{ id: 'publication.journal.close' }} onclick={() => (store.shown = null)} />
    {/snippet}
  </Dialog>
{/if}
