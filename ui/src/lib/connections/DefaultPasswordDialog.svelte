<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Checkbox, Dialog } from '../components';
  import type { ConnectionsStore } from './store.svelte';
  import type { ConnectionProfile } from './client';

  // One-time warning after a sign-in with the default Zabbix account (O-037). The local server
  // compares the submitted credentials and returns only a flag; «Do not show again for this
  // connection» stores a non-secret flag in the profile.
  let { i18n, store, profile }: { i18n: I18n; store: ConnectionsStore; profile: ConnectionProfile } = $props();

  let hide = $state(false);
  let closed = false;

  function close(): void {
    if (closed) return;
    closed = true;
    void store.dismissDefaultPasswordWarning(hide);
  }
</script>

<Dialog
  {i18n}
  id="connection-default-password"
  open
  title={{ id: 'connection.default_password.title' }}
  description={{ id: 'connection.default_password.body', params: { profile: profile.name } }}
  onclose={close}
>
  <Checkbox {i18n} id="connection-default-password-hide" label={{ id: 'connection.default_password.dont_show' }} bind:checked={hide} />
  {#snippet actions()}
    <Button {i18n} variant="primary" label={{ id: 'connection.default_password.acknowledge' }} onclick={close} />
  {/snippet}
</Dialog>
