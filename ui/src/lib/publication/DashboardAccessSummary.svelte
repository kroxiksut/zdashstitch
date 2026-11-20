<script lang="ts">
  import type { I18n } from '../i18n';
  import type { AccessChoices, AccessState } from './client';

  let { i18n, access, choices, label }: {
    i18n: I18n;
    access: AccessState;
    choices: AccessChoices | null;
    label?: string;
  } = $props();
</script>

{#if label}<h4>{label}</h4>{/if}
<p>{i18n.t(access.private ? 'publication.access.private' : 'publication.access.public')}</p>
{#if choices?.can_choose_owner}
  <p>{i18n.t('publication.access.owner')}: {choices.users.find((user) => user.id === access.owner)?.name ?? access.owner}</p>
{/if}
{#each ['users', 'groups'] as kind}
  {@const type = kind === 'users' ? 'users' : 'groups'}
  {#if access[type].length}
    <p>{i18n.t(type === 'users' ? 'publication.access.users' : 'publication.access.groups')}</p>
    <ul>
      {#each access[type] as entry (entry.id)}
        <li>{choices?.[type].find((choice) => choice.id === entry.id)?.name ?? entry.id}
          · {i18n.t(entry.permission === 'read_only' ? 'publication.access.read_only' : 'publication.access.read_write')}</li>
      {/each}
    </ul>
  {/if}
{/each}
