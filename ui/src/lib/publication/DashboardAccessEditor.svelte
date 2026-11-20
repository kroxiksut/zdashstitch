<script lang="ts">
  import { Button } from '../components';
  import type { I18n } from '../i18n';
  import type { AccessChoices, AccessSelection, AccessState, ShareRecipient } from './client';

  let { i18n, choices, current = null, disabled, selection = $bindable({}) }: {
    i18n: I18n;
    choices: AccessChoices;
    current?: AccessState | null;
    disabled: boolean;
    selection: AccessSelection;
  } = $props();

  const privateAccess = $derived(selection.private ?? current?.private ?? true);
  const owner = $derived(selection.owner ?? current?.owner ?? '');
  const canEditRecipients = $derived(!current || choices.can_choose_owner);

  function setEntries(kind: 'users' | 'groups', entries: ShareRecipient[]) {
    const previous = current?.[kind] ?? [];
    const unchanged = previous.length === entries.length
      && previous.every((entry) => entries.some((changed) => changed.id === entry.id && changed.permission === entry.permission));
    selection = { ...selection, [kind]: unchanged ? undefined : entries };
  }
</script>

<fieldset {disabled}>
  <legend>{i18n.t('publication.access.title')}</legend>
  {#if choices.can_choose_owner}
    <label>
      {i18n.t('publication.access.owner')}
      <select value={owner} onchange={(event) => {
        const chosen = event.currentTarget.value;
        selection = { ...selection, owner: chosen && chosen !== current?.owner ? chosen : undefined };
      }}>
        {#if !current}<option value="">{i18n.t('publication.access.current_account')}</option>{/if}
        {#if current && !choices.users.some((user) => user.id === current.owner)}
          <option value={current.owner}>{current.owner}</option>
        {/if}
        {#each choices.users as user (user.id)}<option value={user.id}>{user.name}</option>{/each}
      </select>
    </label>
  {/if}
  <label>
    {i18n.t('publication.access.type')}
    <select value={privateAccess ? 'private' : 'public'} onchange={(event) => {
      const chosen = event.currentTarget.value === 'private';
      selection = { ...selection, private: chosen !== (current?.private ?? true) ? chosen : undefined };
    }}>
      <option value="private">{i18n.t('publication.access.private')}</option>
      <option value="public">{i18n.t('publication.access.public')}</option>
    </select>
  </label>
  {#if !privateAccess}<p>{i18n.t('publication.access.public_warning')}</p>{/if}
  {#if !canEditRecipients}<p>{i18n.t('publication.access.visible_part')}</p>{/if}
  {#if choices.truncated}<p>{i18n.t('publication.access.choices_truncated')}</p>{/if}

  {#each ['users', 'groups'] as kind}
    {@const type = kind === 'users' ? 'users' : 'groups'}
    {@const entries = selection[type] ?? current?.[type] ?? []}
    <fieldset>
      <legend>{i18n.t(type === 'users' ? 'publication.access.users' : 'publication.access.groups')}</legend>
      {#each entries as entry (entry.id)}
        {@const label = choices[type].find((choice) => choice.id === entry.id)?.name ?? entry.id}
        {#if canEditRecipients}
          <label>
            {label}
            <select value={entry.permission} onchange={(event) => {
              const permission = event.currentTarget.value === 'read_write' ? 'read_write' : 'read_only';
              setEntries(type, entries.map((existing) => existing.id === entry.id ? { ...existing, permission } : existing));
            }}>
              <option value="read_only">{i18n.t('publication.access.read_only')}</option>
              <option value="read_write">{i18n.t('publication.access.read_write')}</option>
            </select>
          </label>
          <Button {i18n} label={{id:'publication.access.remove', params:{name:label}}}
            onclick={() => setEntries(type, entries.filter((item) => item.id !== entry.id))} />
        {:else}
          <p>{label} · {i18n.t(entry.permission === 'read_only' ? 'publication.access.read_only' : 'publication.access.read_write')}</p>
        {/if}
      {/each}
      {#if canEditRecipients}
        <label>
          {i18n.t(type === 'users' ? 'publication.access.add_user' : 'publication.access.add_group')}
          <select value="" onchange={(event) => {
            const id = event.currentTarget.value;
            if (id) setEntries(type, [...entries, {id, permission:'read_only'}]);
            event.currentTarget.value = '';
          }}>
            <option value="">{i18n.t('publication.portable.select')}</option>
            {#each choices[type].filter((choice) => !entries.some((entry) => entry.id === choice.id)) as choice (choice.id)}
              <option value={choice.id}>{choice.name}</option>
            {/each}
          </select>
        </label>
      {/if}
    </fieldset>
  {/each}
</fieldset>
