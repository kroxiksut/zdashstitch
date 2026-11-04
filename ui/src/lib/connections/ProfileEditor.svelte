<script lang="ts">
  import { onDestroy, tick, untrack } from 'svelte';
  import type { I18n } from '../i18n';
  import {
    Banner,
    Breadcrumb,
    Button,
    Checkbox,
    Dialog,
    EmptyState,
    Icon,
    RadioGroup,
    TextField,
    resolveText,
    type DisplayText,
    type LocalizedText
  } from '../components';
  import { connectionPath } from '../shell/router';
  import { failureOf, type AddressPreview, type AuthMethod, type ConnectionProfile } from './client';
  import { codeText } from './messages';
  import {
    ZABBIX_DEFAULT_ACCOUNT,
    duplicateName,
    emptyForm,
    fieldOfCode,
    firstInvalidField,
    formFromProfile,
    inputFromForm,
    serverChanged,
    validateForm,
    type FormErrors,
    type FormField,
    type ProfileForm
  } from './state';
  import type { ConnectionsStore } from './store.svelte';

  // Profile editor (P1-02B/C/F, mockup 05-02; secrets in the form by O-037). The password or token
  // is typed in its own masked field, kept apart from the profile form model, and leaves the
  // browser once (D-09), with the profile save: the local process puts it into the system
  // credential store when the user chose so, otherwise keeps it only in its memory. The field is
  // cleared when the secret is sent. The address is normalized by the local server, exactly as
  // `ZabbixEndpoint` does, and the API address is shown before saving.
  let { i18n, store, localId, navigate }: {
    i18n: I18n;
    store: ConnectionsStore;
    localId: string | null;
    navigate: (path: string) => void;
  } = $props();

  const original = $derived(localId === null ? null : store.profile(localId));
  const missing = $derived(localId !== null && store.list !== null && original === null);

  // The form starts from the saved profile when it is already loaded (also in server rendering).
  const initial = untrack(() => original);
  let form = $state<ProfileForm>(initial ? formFromProfile(initial) : emptyForm());
  let loadedFor = $state<string | null>(initial?.local_id ?? null);
  let preview = $state<AddressPreview | null>(null);
  let addressError = $state<LocalizedText | undefined>(undefined);
  let errors = $state<FormErrors>({});
  /** The typed password or token; never part of `form`, never persisted. */
  let secret = $state('');
  /** The stored secret was forgotten in this editor. */
  let forgotten = $state(false);
  let busy = $state<'save' | 'check' | 'duplicate' | 'delete' | 'forget' | 'ca' | null>(null);
  let status = $state<DisplayText | null>(null);
  let confirmDelete = $state(false);
  let nameField = $state<HTMLInputElement>();
  let addressField = $state<HTMLInputElement>();
  let usernameField = $state<HTMLInputElement>();
  let secretField = $state<HTMLInputElement>();
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  let previewController: AbortController | null = null;

  const changedServer = $derived(serverChanged(original, preview?.base_url ?? null));
  /** The saved profile keeps a secret of the selected method in the system credential store. */
  const storedSecret = $derived(
    !forgotten && original !== null && original.secret_storage === 'os_store' && original.auth_method === form.method
  );
  /** The local process keeps a secret of the selected method in memory and storage stays off. */
  const memorySecret = $derived(
    !forgotten &&
      original !== null &&
      original.secret_in_memory &&
      original.auth_method === form.method &&
      !(form.method === 'password' ? form.storePassword : form.storeToken)
  );
  const secretHint: LocalizedText | undefined = $derived(
    storedSecret
      ? { id: form.method === 'password' ? 'connection.form.password_stored' : 'connection.form.token_stored' }
      : memorySecret
        ? { id: 'connection.form.secret_in_memory' }
        : undefined
  );
  const plainHttp = $derived(preview?.plain_http ?? original?.plain_http ?? false);
  const titleText: LocalizedText = $derived(
    original ? { id: 'connection.editor.title_edit', params: { profile: original.name } } : { id: 'connection.editor.title_new' }
  );

  $effect(() => {
    const profile = original;
    if (profile && untrack(() => loadedFor) !== profile.local_id) {
      untrack(() => {
        form = formFromProfile(profile);
        loadedFor = profile.local_id;
      });
    }
  });

  $effect(() => {
    const address = form.address.trim();
    untrack(() => schedulePreview(address));
  });

  function schedulePreview(address: string): void {
    clearTimeout(previewTimer);
    previewController?.abort();
    if (address === '') {
      preview = null;
      addressError = undefined;
      return;
    }
    previewTimer = setTimeout(() => {
      const controller = new AbortController();
      previewController = controller;
      store.client
        .previewAddress(address, controller.signal)
        .then((result) => {
          preview = result;
          addressError = undefined;
        })
        .catch((error: unknown) => {
          const failure = failureOf(error);
          if (failure.code === 'aborted') return;
          preview = null;
          addressError = failure.code === 'invalid_address' ? codeText(failure.code, failure.params) : undefined;
        });
    }, 300);
  }

  onDestroy(() => {
    clearTimeout(previewTimer);
    previewController?.abort();
    secret = '';
  });

  const methodOptions = [
    { value: 'api_token', label: { id: 'connection.form.api_token' } },
    { value: 'password', label: { id: 'connection.form.auth_password' } }
  ] as const;
  const trustOptions = [
    { value: 'system', label: { id: 'connection.form.trust_system' } },
    { value: 'custom_ca', label: { id: 'connection.form.trust_custom_ca' }, description: { id: 'connection.form.custom_ca_hint' } }
  ] as const;

  let method = $state<string>(untrack(() => form.method));
  let trust = $state<string>(untrack(() => form.trust));
  $effect(() => {
    // Keep the radio groups and the typed form in step (RadioGroup binds plain strings).
    const formMethod = form.method;
    const formTrust = form.trust;
    untrack(() => {
      method = formMethod;
      trust = formTrust;
    });
  });
  $effect(() => {
    const next = method === 'password' ? 'password' : 'api_token';
    untrack(() => {
      if (form.method !== next) {
        form.method = next as AuthMethod;
        // A password never becomes a token or the other way round.
        secret = '';
      }
    });
  });
  $effect(() => {
    const next = trust === 'custom_ca' ? 'custom_ca' : 'system';
    untrack(() => {
      if (form.trust !== next) form.trust = next;
    });
  });

  async function focusField(field: FormField | 'form' | null): Promise<void> {
    await tick();
    const target: Record<FormField, HTMLElement | null | undefined> = {
      name: nameField,
      address: addressField,
      username: usernameField,
      confirm_http: document.getElementById('connection-confirm-http'),
      trust: document.getElementById('connection-trust-file') ?? document.getElementById('connection-trust-system')
    };
    if (field && field !== 'form') target[field]?.focus();
  }

  async function save(next: 'list' | 'check'): Promise<void> {
    const local = validateForm(form, plainHttp);
    if (addressError) local.address = addressError;
    errors = local;
    if (Object.keys(local).length > 0) {
      status = { id: 'connection.form.has_errors' };
      await focusField(firstInvalidField(local));
      return;
    }
    busy = next === 'check' ? 'check' : 'save';
    status = null;
    const withProfile = secret === '' ? undefined : secret;
    // D-09: the field is cleared as soon as the secret is sent.
    secret = '';
    try {
      const input = inputFromForm(form);
      let saved: ConnectionProfile;
      let contextChanged = false;
      if (original) {
        const result = await store.client.update(original.local_id, input, withProfile);
        saved = result.profile;
        contextChanged = result.context_changed;
      } else {
        saved = await store.client.create(input, withProfile);
      }
      if (withProfile !== undefined) forgotten = false;
      await store.refresh();
      store.announce({
        text: contextChanged
          ? { id: 'connection.state.context_changed', params: { profile: saved.name } }
          : { id: 'connection.saved', params: { profile: saved.name } },
        tone: contextChanged ? 'warning' : 'success'
      });
      if (next === 'check') {
        store.autostart = saved.local_id;
        navigate(connectionPath(saved.local_id, 'check'));
      } else {
        navigate('/connections');
      }
    } catch (error) {
      const failure = failureOf(error);
      const field = fieldOfCode(failure.code);
      errors = { [field]: codeText(failure.code, failure.params) };
      status = field === 'form' ? codeText(failure.code, failure.params) : { id: 'connection.form.has_errors' };
      await focusField(field);
    } finally {
      busy = null;
    }
  }

  async function duplicate(): Promise<void> {
    if (!original) return;
    busy = 'duplicate';
    try {
      const name = duplicateName(original.name, (profile) => i18n.t('connection.duplicate.name', { profile }));
      const copy = await store.client.duplicate(original.local_id, name);
      await store.refresh();
      store.announce({ text: { id: 'connection.saved', params: { profile: copy.name } }, tone: 'success' });
      navigate(connectionPath(copy.local_id, 'edit'));
    } catch (error) {
      const failure = failureOf(error);
      status = codeText(failure.code, failure.params);
    } finally {
      busy = null;
    }
  }

  async function remove(): Promise<void> {
    if (!original) return;
    busy = 'delete';
    try {
      const outcome = await store.client.remove(original.local_id);
      store.forget(original.local_id);
      await store.refresh();
      store.announce({
        text: { id: 'connection.deleted', params: { profile: original.name } },
        detail: outcome.local_credential ? { id: 'connection.delete.credential_left' } : undefined,
        tone: 'info'
      });
      confirmDelete = false;
      navigate('/connections');
    } catch (error) {
      const failure = failureOf(error);
      confirmDelete = false;
      status = codeText(failure.code, failure.params);
    } finally {
      busy = null;
    }
  }

  async function forgetSecret(): Promise<void> {
    if (!original) return;
    busy = 'forget';
    try {
      await store.client.forgetSecret(original.local_id);
      forgotten = true;
      status = original.auth_method === 'password' ? { id: 'connection.forget_password.done' } : { id: 'connection.forget_token.not_revoked' };
    } catch (error) {
      const failure = failureOf(error);
      status = codeText(failure.code, failure.params);
    } finally {
      busy = null;
    }
  }

  /** «Fill in the default account» (O-037): fills the form only; nothing is saved or sent. */
  function useDefaultAccount(): void {
    form.username = ZABBIX_DEFAULT_ACCOUNT.username;
    secret = ZABBIX_DEFAULT_ACCOUNT.password;
    void tick().then(() => secretField?.focus());
  }

  async function chooseCa(event: Event): Promise<void> {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    busy = 'ca';
    errors = { ...errors, trust: undefined };
    try {
      const imported = await store.client.importTrustedCa(file);
      form.caRef = imported.ca_ref;
      form.caFingerprints = imported.certificate_fingerprints;
    } catch (error) {
      const failure = failureOf(error);
      errors = { ...errors, trust: codeText(failure.code, failure.params) };
    } finally {
      busy = null;
      input.value = '';
    }
  }
</script>

<main id="main-content" tabindex="-1">
  <div class="screen connection-editor">
    {#if missing}
      <EmptyState {i18n} level={1} heading={{ id: 'connection.list.title' }} reason={{ id: 'errors.profile.not_found' }}>
        {#snippet action()}<a class="button-link" href="/connections">{i18n.t('connection.action.back_to_list')}</a>{/snippet}
      </EmptyState>
    {:else}
      <Breadcrumb
        {i18n}
        label={{ id: 'shell.a11y.breadcrumbs' }}
        items={original
          ? [
              { label: { id: 'connection.list.title' }, href: '/connections' },
              { label: { value: original.name, kind: 'user' } },
              { label: { id: 'connection.action.edit' } }
            ]
          : [{ label: { id: 'connection.list.title' }, href: '/connections' }, { label: { id: 'connection.editor.title_new' } }]}
      />
      <h1>{resolveText(i18n, titleText)}</h1>

      <div class="status-line" role="status" aria-live="polite">
        {#if status}<p>{resolveText(i18n, status)}</p>{/if}
      </div>

      <div class="editor-layout">
        <form class="card editor-form" novalidate onsubmit={(event) => { event.preventDefault(); void save('list'); }}>
          <section class="form-section" aria-labelledby="connection-section-profile">
            <h2 id="connection-section-profile" class="eyebrow">{i18n.t('connection.editor.section.profile')}</h2>
            <TextField {i18n} id="connection-name" label={{ id: 'connection.form.profile_name' }} bind:value={form.name} bind:element={nameField} required error={errors.name} />
            <TextField
              {i18n}
              id="connection-address"
              type="url"
              technical
              autocomplete="off"
              label={{ id: 'connection.form.endpoint' }}
              hint={{ id: 'connection.form.endpoint_hint' }}
              bind:value={form.address}
              bind:element={addressField}
              required
              error={errors.address ?? addressError}
            />
            <p class="hint api-url" aria-live="polite">
              {#if preview}
                <span class="technical">{i18n.t('connection.form.api_url', { url: preview.api_url })}</span>
              {:else}
                {i18n.t('connection.form.api_url_pending')}
              {/if}
            </p>
            {#if changedServer}
              <Banner {i18n} heading={{ id: 'connection.form.server_changed.title' }} body={{ id: 'connection.form.server_changed.body' }} tone="warning" />
            {/if}
            {#if plainHttp}
              <p class="note warning"><Icon name="alert-triangle" /><span>{i18n.t('connection.warning.http')}</span></p>
            {/if}
          </section>

          <section class="form-section" aria-labelledby="connection-section-sign-in">
            <h2 id="connection-section-sign-in" class="eyebrow">{i18n.t('connection.editor.section.sign_in')}</h2>
            <RadioGroup {i18n} name="connection-method" legend={{ id: 'connection.form.auth_method' }} options={methodOptions} bind:value={method} />
            {#if form.method === 'password'}
              <TextField {i18n} id="connection-username" label={{ id: 'connection.form.username' }} bind:value={form.username} bind:element={usernameField} autocomplete="off" required error={errors.username} />
              <TextField
                {i18n}
                id="connection-password"
                type="password"
                label={{ id: 'connection.form.password' }}
                hint={secretHint}
                bind:value={secret}
                bind:element={secretField}
                autocomplete="off"
              />
              <div class="actions">
                <Button {i18n} label={{ id: 'connection.action.use_default_account' }} disabled={busy !== null} onclick={useDefaultAccount} />
              </div>
              <p class="hint">{i18n.t('connection.form.default_account_hint')}</p>
              <Checkbox
                {i18n}
                id="connection-store-password"
                label={{ id: 'connection.form.store_password' }}
                description={{ id: 'connection.form.store_password_hint' }}
                bind:checked={form.storePassword}
              />
              {#if form.storePassword && plainHttp}
                <Checkbox {i18n} id="connection-confirm-http" label={{ id: 'connection.form.http_confirm_password' }} bind:checked={form.confirmPlainHttp} />
                {#if errors.confirm_http}<p class="field-error">{resolveText(i18n, errors.confirm_http)}</p>{/if}
              {/if}
              {#if !form.storePassword}<p class="hint">{i18n.t('connection.form.password_not_stored')}</p>{/if}
              <p class="hint">{i18n.t('connection.form.mfa_hint')}</p>
              {#if storedSecret}
                <div class="actions">
                  <Button {i18n} label={{ id: 'connection.action.forget_password' }} busy={busy === 'forget'} disabled={busy !== null} onclick={() => void forgetSecret()} />
                </div>
              {/if}
            {:else}
              <TextField
                {i18n}
                id="connection-token"
                type="password"
                technical
                label={{ id: 'connection.form.api_token' }}
                hint={secretHint}
                bind:value={secret}
                bind:element={secretField}
                autocomplete="off"
              />
              <Checkbox {i18n} id="connection-store-token" label={{ id: 'connection.form.store_token' }} bind:checked={form.storeToken} />
              {#if form.storeToken && plainHttp}
                <Checkbox {i18n} id="connection-confirm-http" label={{ id: 'connection.form.http_confirm' }} bind:checked={form.confirmPlainHttp} />
                {#if errors.confirm_http}<p class="field-error">{resolveText(i18n, errors.confirm_http)}</p>{/if}
              {/if}
              {#if !form.storeToken}<p class="hint">{i18n.t('connection.form.token_at_connect')}</p>{/if}
              {#if storedSecret}
                <div class="actions">
                  <Button {i18n} label={{ id: 'connection.action.forget_token' }} busy={busy === 'forget'} disabled={busy !== null} onclick={() => void forgetSecret()} />
                </div>
                <p class="hint">{i18n.t('connection.forget_token.not_revoked')}</p>
              {/if}
            {/if}
          </section>

          <section class="form-section" aria-labelledby="connection-section-trust">
            <h2 id="connection-section-trust" class="eyebrow">{i18n.t('connection.editor.section.trust')}</h2>
            <RadioGroup {i18n} name="connection-trust" legend={{ id: 'connection.form.custom_ca' }} options={trustOptions} bind:value={trust} />
            {#if form.trust === 'custom_ca'}
              <div class="field">
                <label for="connection-trust-file">{i18n.t('connection.form.custom_ca_choose')}</label>
                <input
                  id="connection-trust-file"
                  type="file"
                  accept=".pem,.crt,.cer"
                  disabled={busy !== null}
                  aria-invalid={errors.trust ? 'true' : undefined}
                  aria-describedby={errors.trust ? 'connection-trust-error' : undefined}
                  onchange={(event) => void chooseCa(event)}
                />
                {#if busy === 'ca'}<p class="hint" role="status">{i18n.t('connection.form.custom_ca_loading')}</p>{/if}
                {#if errors.trust}<p id="connection-trust-error" class="field-error">{resolveText(i18n, errors.trust)}</p>{/if}
              </div>
              {#if form.caRef && form.caFingerprints.length > 0}
                <p>{i18n.t('connection.form.custom_ca_loaded', { count: form.caFingerprints.length })}</p>
                <ul class="fingerprints">
                  {#each form.caFingerprints as fingerprint (fingerprint)}
                    <li class="technical">{i18n.t('connection.form.custom_ca_fingerprint', { fingerprint })}</li>
                  {/each}
                </ul>
              {/if}
            {/if}
            <p class="hint">{i18n.t('connection.form.tls_no_bypass')}</p>
          </section>

          <section class="form-section" aria-labelledby="connection-section-mode">
            <h2 id="connection-section-mode" class="eyebrow">{i18n.t('connection.editor.section.mode')}</h2>
            <p>{i18n.t('connection.form.mode_read_only')}</p>
          </section>
        </form>

        <aside class="editor-side">
          <section class="card" aria-labelledby="connection-rules-title">
            <h2 id="connection-rules-title" class="eyebrow">{i18n.t('connection.form.rules.title')}</h2>
            <ul class="rules">
              <li>{i18n.t('connection.form.rules.scheme')}</li>
              <li>{i18n.t('connection.form.rules.port_path')}</li>
              <li>{i18n.t('connection.form.rules.rejected')}</li>
              <li>{i18n.t('connection.form.rules.http')}</li>
            </ul>
            <p class="hint">{i18n.t('connection.form.rename_keeps_server')}</p>
          </section>
          <div class="actions editor-actions">
            <Button {i18n} label={{ id: 'connection.action.test' }} busy={busy === 'check'} disabled={busy !== null} onclick={() => void save('check')} />
            <Button {i18n} variant="primary" label={{ id: 'connection.action.save' }} busy={busy === 'save'} disabled={busy !== null} onclick={() => void save('list')} />
          </div>
          <div class="actions editor-actions">
            <a class="button-link" href="/connections">{i18n.t('connection.action.cancel')}</a>
            {#if original}
              <Button {i18n} label={{ id: 'common.action.duplicate' }} busy={busy === 'duplicate'} disabled={busy !== null} onclick={() => void duplicate()} />
              <Button {i18n} variant="danger" label={{ id: 'common.action.delete' }} disabled={busy !== null} onclick={() => (confirmDelete = true)} />
            {/if}
          </div>
        </aside>
      </div>
    {/if}
  </div>
</main>

{#if confirmDelete && original}
  <Dialog
    {i18n}
    id="connection-delete"
    open
    title={{ id: 'connection.delete.title', params: { profile: original.name } }}
    description={{ id: 'connection.delete.body' }}
    onclose={() => (confirmDelete = false)}
  >
    {#if original.state === 'connected'}<p class="hint">{i18n.t('connection.delete.disconnect_first')}</p>{/if}
    {#snippet actions()}
      <Button {i18n} label={{ id: 'connection.action.cancel' }} onclick={() => (confirmDelete = false)} />
      <Button {i18n} variant="danger" label={{ id: 'common.action.delete' }} busy={busy === 'delete'} onclick={() => void remove()} />
    {/snippet}
  </Dialog>
{/if}

<style>
  .connection-editor {
    max-width: 84rem;
  }

  .editor-layout {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(16rem, 1fr);
    align-items: start;
    gap: var(--space-6);
  }

  .editor-form {
    gap: var(--space-6);
  }

  .form-section {
    display: grid;
    gap: var(--space-3);
  }

  .form-section + .form-section {
    padding-block-start: var(--space-5);
    border-block-start: 1px solid var(--border-subtle);
  }

  .editor-side {
    display: grid;
    gap: var(--space-4);
  }

  .rules {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding-inline-start: var(--space-5);
  }

  .fingerprints {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .api-url {
    overflow-wrap: anywhere;
  }

  .note {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid;
    border-radius: var(--radius-panel);
  }

  .editor-actions {
    justify-content: flex-end;
  }

  @media (max-width: 64rem) {
    .editor-layout {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
