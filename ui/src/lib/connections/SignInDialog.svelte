<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import type { I18n } from '../i18n';
  import { Button, Dialog, TextField, type DisplayText, type LocalizedText } from '../components';
  import { codeText } from './messages';
  import { SIGN_IN_WAITING_AFTER_MS, lockoutSeconds } from './state';
  import type { ConnectionsStore, SignInRequest } from './store.svelte';

  // Sign-in on connect or check (P1-02C1): one masked field, focus in it, cleared right after
  // submission; the secret goes to the local server once and is never kept here.
  let { i18n, store, request }: { i18n: I18n; store: ConnectionsStore; request: SignInRequest } = $props();

  const profile = $derived(request.profile);
  const isPassword = $derived(profile.auth_method === 'password');
  let secret = $state('');
  let field = $state<HTMLInputElement>();
  let busy = $state(false);
  let waiting = $state(false);
  let error = $state<DisplayText | undefined>(undefined);
  let lockedUntil = $state(0);
  let now = $state(Date.now());
  let waitTimer: ReturnType<typeof setTimeout> | undefined;
  let clock: ReturnType<typeof setInterval> | undefined;

  const remaining = $derived(Math.max(0, Math.ceil((lockedUntil - now) / 1000)));
  const fieldLabel: LocalizedText = $derived(isPassword ? { id: 'connection.form.password' } : { id: 'connection.form.api_token' });

  $effect(() => {
    void tick().then(() => field?.focus());
  });

  function startClock(seconds: number): void {
    lockedUntil = Date.now() + seconds * 1000;
    now = Date.now();
    clearInterval(clock);
    clock = setInterval(() => {
      now = Date.now();
      if (now >= lockedUntil) clearInterval(clock);
    }, 1000);
  }

  function errorFor(code: string, params: Readonly<Record<string, string>>): DisplayText {
    if (code === 'mfa_session_terminated') return { id: 'connection.auth.mfa_token_required' };
    if (code === 'secret_required' || code === 'credential_missing') {
      return isPassword ? { id: 'connection.sign_in.password_required' } : { id: 'connection.sign_in.token_required' };
    }
    return codeText(code, params);
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (busy || remaining > 0) return;
    if (secret === '') {
      error = isPassword ? { id: 'connection.sign_in.password_required' } : { id: 'connection.sign_in.token_required' };
      field?.focus();
      return;
    }
    const typed = secret;
    secret = '';
    error = undefined;
    busy = true;
    waiting = false;
    waitTimer = setTimeout(() => (waiting = true), SIGN_IN_WAITING_AFTER_MS);
    const outcome = await store.submitSecret(request, typed);
    clearTimeout(waitTimer);
    busy = false;
    waiting = false;
    if (outcome.kind === 'rejected') {
      const failed = outcome.report.steps.find((step) => step.status === 'failed');
      error = errorFor(failed?.code ?? 'auth_failed', failed?.params ?? {});
      const seconds = lockoutSeconds(outcome.report);
      if (seconds > 0) startClock(seconds);
      await tick();
      field?.focus();
    } else if (outcome.kind === 'failed') {
      error = errorFor(outcome.failure.code, outcome.failure.params);
      await tick();
      field?.focus();
    }
  }

  onDestroy(() => {
    clearTimeout(waitTimer);
    clearInterval(clock);
    secret = '';
  });
</script>

<Dialog
  {i18n}
  id="connection-sign-in"
  open
  title={{ id: 'connection.sign_in.title', params: { profile: profile.name } }}
  description={{ id: 'connection.sign_in.description', params: { address: profile.endpoint_display } }}
  onclose={() => store.closeSignIn()}
>
  <form class="sign-in" novalidate onsubmit={submit}>
    {#if isPassword}
      <p>{i18n.t('connection.sign_in.username', { username: profile.username ?? '' })}</p>
      <p class="hint">{i18n.t('connection.sign_in.username_hint')}</p>
      <p class="hint">
        {profile.secret_storage === 'os_store' ? i18n.t('connection.sign_in.password_will_be_stored') : i18n.t('connection.sign_in.password_memory_only')}
      </p>
    {:else}
      <p class="hint">
        {profile.secret_storage === 'os_store' ? i18n.t('connection.sign_in.token_will_be_stored') : i18n.t('connection.sign_in.token_memory_only')}
      </p>
    {/if}
    <TextField
      {i18n}
      id="connection-sign-in-secret"
      type="password"
      label={fieldLabel}
      bind:value={secret}
      bind:element={field}
      autocomplete="off"
      required
      disabled={busy}
      {error}
    />
    {#if isPassword}<p class="hint">{i18n.t('connection.test.lockout_hint')}</p>{/if}
    <div class="sign-in-status" role="status" aria-live="polite">
      {#if remaining > 0}
        <p>{i18n.t('connection.sign_in.locked', { seconds: remaining })}</p>
      {:else if busy}
        <p>{i18n.t('connection.sign_in.in_progress')}</p>
        {#if waiting}<p class="hint">{i18n.t('connection.sign_in.waiting')}</p>{/if}
      {/if}
    </div>
    <div class="actions">
      <Button {i18n} type="submit" variant="primary" label={{ id: 'connection.action.sign_in' }} {busy} disabled={remaining > 0} />
      <Button {i18n} label={{ id: 'connection.action.cancel' }} onclick={() => store.closeSignIn()} />
    </div>
  </form>
</Dialog>

<style>
  .sign-in {
    display: grid;
    gap: var(--space-3);
    margin-block-start: var(--space-3);
  }
</style>
