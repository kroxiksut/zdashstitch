<script lang="ts">
  import type { I18n } from '../i18n';
  import { Icon, resolveText, type LocalizedText } from '../components';
  import type { StepItem } from './model';

  // Step bar of the bindings wizard (P1-05) and of the publication path (P1-08A). Steps are buttons
  // in the tab order; the current one has `aria-current="step"`, a passed one a check icon and the
  // word «пройден», an unavailable one stays visible, disabled, with «пока недоступен».

  let { i18n, label, steps, onselect }: {
    i18n: I18n;
    label: LocalizedText;
    steps: ReadonlyArray<StepItem>;
    onselect: (id: string) => void;
  } = $props();
</script>

<nav class="step-bar" aria-label={resolveText(i18n, label)}>
  <ol>
    {#each steps as step, index (step.id)}
      <li>
        <button
          type="button"
          class="step"
          class:current={step.state === 'current'}
          class:done={step.state === 'done'}
          class:error={step.state === 'error'}
          aria-current={step.state === 'current' ? 'step' : undefined}
          disabled={step.state === 'unavailable'}
          onclick={() => onselect(step.id)}
        >
          {#if step.state === 'done'}<Icon name="circle-check" size={18} />{:else if step.state === 'error'}<Icon name="circle-x" size={18} />{:else}<span class="step-number">{index + 1}</span>{/if}
          <span>{resolveText(i18n, step.label)}</span>
          {#if step.state === 'done'}<span class="visually-hidden">{i18n.t('bindings.step.done')}</span>{/if}
          {#if step.state === 'unavailable'}<span class="visually-hidden">{i18n.t('bindings.step.unavailable')}</span>{/if}
          {#if step.state === 'error'}<span class="visually-hidden">{i18n.t('publication.state.conflict')}</span>{/if}
        </button>
      </li>
    {/each}
  </ol>
</nav>
