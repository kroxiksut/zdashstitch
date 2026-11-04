<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button } from '../components';
  import { recoveryAction } from './messages';

  // Next step after a failed server read (P1-03B «Нужен вход / нет связи», P1-01E §5): sign in
  // again or connect in «Connections», otherwise retry the same read.
  let { i18n, code, onretry }: { i18n: I18n; code: string; onretry: () => void } = $props();

  const action = $derived(recoveryAction(code));
</script>

{#if action === 'sign_in'}
  <a class="button-link primary" href="/connections">{i18n.t('connection.action.sign_in')}</a>
{:else if action === 'connect'}
  <a class="button-link primary" href="/connections">{i18n.t('startup.action.connect')}</a>
{:else}
  <Button {i18n} label={{ id: 'connection.action.retry' }} onclick={onretry} />
{/if}
