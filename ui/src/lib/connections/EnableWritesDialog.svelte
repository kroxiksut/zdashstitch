<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Dialog, Icon } from '../components';
  import type { ConnectionProfile } from './client';

  // «Allow dashboard writes?» (P1-02E, mockup 05-04). Writes are allowed only by this explicit
  // confirmation and only until disconnect or restart; Zabbix account permissions are unchanged.
  let { i18n, profile, busy = false, onconfirm, oncancel }: {
    i18n: I18n;
    profile: ConnectionProfile;
    busy?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();
</script>

<Dialog
  {i18n}
  id="connection-enable-writes"
  open
  title={{ id: 'connection.enable_writes.title' }}
  description={{
    id: 'connection.enable_writes.context',
    params: { profile: profile.name, version: profile.last_check?.server_version ?? '' }
  }}
  onclose={oncancel}
>
  <div class="enable-writes">
    <section aria-labelledby="enable-writes-available">
      <h3 id="enable-writes-available" class="eyebrow">{i18n.t('connection.enable_writes.available_title')}</h3>
      <p>{i18n.t('connection.enable_writes.available')}</p>
      <p class="hint">{i18n.t('connection.enable_writes.plan_only')}</p>
    </section>
    <section aria-labelledby="enable-writes-never">
      <h3 id="enable-writes-never" class="eyebrow">{i18n.t('connection.enable_writes.never_title')}</h3>
      <p>{i18n.t('connection.enable_writes.never')}</p>
    </section>
    <section aria-labelledby="enable-writes-duration">
      <h3 id="enable-writes-duration" class="eyebrow">{i18n.t('connection.enable_writes.duration_title')}</h3>
      <p>{i18n.t('connection.enable_writes.duration')}</p>
      <p class="hint">{i18n.t('connection.enable_writes.return_hint')}</p>
    </section>
    <p class="note warning"><Icon name="alert-triangle" /><span>{i18n.t('connection.enable_writes.no_extra_rights')}</span></p>
    <div class="actions">
      <Button {i18n} label={{ id: 'connection.action.cancel' }} onclick={oncancel} />
      <Button {i18n} variant="primary" label={{ id: 'connection.action.enable_writes' }} {busy} onclick={onconfirm} />
    </div>
  </div>
</Dialog>

<style>
  .enable-writes {
    display: grid;
    gap: var(--space-4);
    margin-block-start: var(--space-4);
  }

  .enable-writes section {
    display: grid;
    gap: var(--space-1);
  }

  .note {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid;
    border-radius: var(--radius-panel);
  }

  .actions {
    justify-content: flex-end;
  }
</style>
