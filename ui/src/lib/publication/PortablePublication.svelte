<script lang="ts">
  import { Button, Dialog, resolveText } from '../components';
  import type { ActiveConnection, ConnectionProfile } from '../connections/client';
  import type { I18n } from '../i18n';
  import type { OfflineDashboard, OfflinePage, PortablePageWidget } from '../library/client';
  import { createHttpWorkcopyClients, type ImportPreview, type ServerDashboardRow } from '../workcopy/client';
  import { createHttpPublicationClient, publicationFailure, type AccessChoices, type AccessSelection, type AccessState, type PortablePublicationClient, type PortableUpdateIntent } from './client';
  import DashboardAccessEditor from './DashboardAccessEditor.svelte';
  import DashboardAccessSummary from './DashboardAccessSummary.svelte';
  import { publicationCodeText, REMOTE_CHANGE_LABEL, stateLabel } from './messages';
  import { zabbixDashboardUrl, type Outcome, type PublicationFailure } from './model';

  let { i18n, draft, enabled, active, profile }: {
    i18n: I18n;
    draft: OfflineDashboard | OfflinePage;
    enabled: boolean;
    active: ActiveConnection | null;
    profile: ConnectionProfile | null;
  } = $props();

  const client = createHttpPublicationClient();
  const maxPageWidgets = 256;
  const { server } = createHttpWorkcopyClients();
  type Links = Awaited<ReturnType<PortablePublicationClient['deployments']>>;
  type Plan = Awaited<ReturnType<PortablePublicationClient['planPortable'] | PortablePublicationClient['planPortableUpdate']>>;
  let links = $state<Links | null>(null);
  let plan = $state<Plan | null>(null);
  let acknowledgeDeletions = $state(false);
  let acknowledgeRepeatAppend = $state(false);
  let acknowledgePublicAccess = $state(false);
  let accessChoices = $state<AccessChoices | null>(null);
  let createAccess = $state<AccessSelection>({});
  let updateAccess = $state<AccessSelection>({});
  let accessTarget = $state<{ deploymentId: string; current: AccessState } | null>(null);
  const removesContent = $derived(plan?.plan.action !== 'create_dashboard' && plan?.plan.removes_content === true);
  const repeatsAppend = $derived(plan?.plan.action !== 'create_dashboard' && plan?.plan.repeats_append === true);
  const publicAccess = $derived(plan?.plan.action === 'create_dashboard' ? !plan.plan.private :
    plan?.plan.access_change?.before.private === true && plan.plan.access_change.after.private === false);
  const confirmationPending = $derived((removesContent && !acknowledgeDeletions) ||
    (repeatsAppend && !acknowledgeRepeatAppend) || (publicAccess && !acknowledgePublicAccess));
  let outcome = $state<Outcome | null>(null);
  let failure = $state<PublicationFailure | null>(null);
  let busy = $state(false);
  let name = $state('');
  let createAnother = $state(false);
  let choosing = $state(false);
  let targets = $state<ReadonlyArray<ServerDashboardRow>>([]);
  let dashboardid = $state('');
  let destination = $state<ImportPreview | null>(null);
  let pageid = $state('');
  let afterPage = $state('');
  let statuses = $state<Record<string, Awaited<ReturnType<PortablePublicationClient['checkDeployment']>>>>({});
  let sourcePageId = $state('');
  let placements = $state<Record<string, { x: number | undefined; y: number | undefined }>>({});
  const context = $derived(`${draft.local_id}:${draft.revision}:${active?.local_id ?? ''}:${active?.context_generation ?? ''}:${active?.mode ?? ''}`);
  const ready = $derived(enabled && active?.mode === 'read_write' && profile?.last_check?.server_version === '8.0.0');
  const existing = $derived(links?.deployments.some((link) => link.connection_id === active?.local_id) ?? false);
  const recoverable = $derived(links?.recovery.filter((entry) => entry.connection_local_id === active?.local_id) ?? []);
  const localPage = $derived(draft.local_id.startsWith('pg-'));
  const project = $derived(draft.bundle.project);
  const sourcePages = $derived(project.kind === 'page' ? [project.root] : project.root.pages);
  const sourcePage = $derived(sourcePages.find((page) => page.id === sourcePageId) ?? (sourcePages.length === 1 ? sourcePages[0] : undefined));
  const graphWidgets = $derived(sourcePage?.widgets.filter((widget) =>
    widget.type === 'graph' || (widget.type === 'zabbix' && widget.widget_type === 'svggraph' && widget.registry_state === 'known')
  ) ?? []);
  const selected = $derived(graphWidgets.filter((widget) => placements[widget.id] !== undefined));
  const targetPage = $derived(destination?.pages.find((page) => page.dashboard_pageid === pageid));

  $effect(() => {
    const key = context;
    const id = draft.local_id;
    name = draft.bundle.project.root.name;
    createAnother = false;
    links = null;
    plan = null;
    acknowledgeDeletions = false;
    acknowledgeRepeatAppend = false;
    acknowledgePublicAccess = false;
    accessChoices = null;
    createAccess = {};
    updateAccess = {};
    accessTarget = null;
    outcome = null;
    failure = null;
    choosing = false;
    targets = [];
    dashboardid = '';
    destination = null;
    pageid = '';
    afterPage = '';
    statuses = {};
    sourcePageId = '';
    placements = {};
    void refresh(id, key);
    if (ready) void refreshAccessChoices(key);
  });

  async function refreshAccessChoices(key: string) {
    try {
      const result = await client.accessChoices();
      if (key === context) accessChoices = result;
    } catch {
      if (key === context) accessChoices = null;
    }
  }

  async function editAccess(deploymentId: string) {
    if (!ready || busy || !accessChoices) return;
    const key = context;
    busy = true;
    failure = null;
    accessTarget = null;
    try {
      const current = await client.deploymentAccess(draft.local_id, deploymentId);
      if (key === context) {
        updateAccess = {};
        accessTarget = {deploymentId, current};
      }
    } catch (error) {
      if (key === context) failure = publicationFailure(error);
    } finally {
      busy = false;
    }
  }

  function hasAccessSelection(selection: AccessSelection) {
    return Object.values(selection).some((value) => value !== undefined);
  }

  async function refresh(id = draft.local_id, key = context) {
    try {
      const result = await client.deployments(id);
      if (key === context) links = result;
    } catch (error) {
      if (key === context) failure = publicationFailure(error);
    }
  }

  async function chooseTargets() {
    if (!active || !ready || busy) return;
    const key = context;
    busy = true;
    try {
      const listed = await server.list(active.local_id, {search: '', access: 'mine', offset: 0, limit: 50});
      if (context === key) {
        targets = listed.rows.filter((row) => row.owner === 'me' && row.access === 'private');
        choosing = true;
      }
    } catch (error) {
      if (context === key) failure = publicationFailure(error);
    } finally {
      busy = false;
    }
  }

  async function selectDashboard() {
    destination = null;
    pageid = '';
    afterPage = '';
    placements = {};
    if (!active || !dashboardid) return;
    const key = context;
    const target = dashboardid;
    busy = true;
    try {
      const preview = await server.preview(active.local_id, target);
      if (key === context && target === dashboardid) destination = preview;
    } catch (error) {
      if (key === context) failure = publicationFailure(error);
    } finally {
      busy = false;
    }
  }

  function chooseWidget(widget: PortablePageWidget, checked: boolean) {
    if (!checked) {
      delete placements[widget.id];
      return;
    }
    const page = targetPage;
    if (!page) return;
    const occupied = page.widgets.map(({ x, y, width, height }) => ({ x, y, width, height }));
    for (const other of selected) {
      const location = placements[other.id];
      if (location.x !== undefined && location.y !== undefined) {
        occupied.push({ x: location.x, y: location.y, width: other.layout.w, height: other.layout.h });
      }
    }
    const { w, h } = widget.layout;
    for (let y = 0; y <= 64 - h; y += 1) {
      for (let x = 0; x <= 72 - w; x += 1) {
        if (occupied.every((cell) => x >= cell.x + cell.width || x + w <= cell.x ||
          y >= cell.y + cell.height || y + h <= cell.y)) {
          placements[widget.id] = { x, y };
          return;
        }
      }
    }
    placements[widget.id] = { x: undefined, y: undefined };
  }

  function prepareSelected() {
    if (!sourcePage) return;
    const widgets = [];
    for (const widget of selected) {
      const { x, y } = placements[widget.id];
      if (x === undefined || y === undefined) return;
      widgets.push({ widget_id: widget.id, x, y });
    }
    void prepareUpdate({ kind: 'append_widgets', dashboardid, pageid, source_page_id: sourcePage.id, widgets });
  }

  async function checkDeployment(deploymentId: string) {
    if (!active || busy) return;
    const key = context;
    busy = true;
    failure = null;
    try {
      const result = await client.checkDeployment(draft.local_id, deploymentId);
      if (key === context) statuses[deploymentId] = result;
    } catch (error) {
      if (key === context) failure = publicationFailure(error);
    } finally {
      busy = false;
    }
  }

  async function prepareUpdate(intent: PortableUpdateIntent) {
    if (!ready || !active || busy || plan) return;
    const id = draft.local_id;
    const key = context;
    busy = true;
    failure = null;
    outcome = null;
    try {
      const result = await client.planPortableUpdate(id, {connection_id: active.local_id, revision: draft.revision, intent});
      if (key === context && result.plan.write_requests === 1) plan = result;
      else await client.discard(id, result.handle);
    } catch (error) {
      if (key === context) failure = publicationFailure(error);
    } finally {
      busy = false;
    }
  }

  async function prepare() {
    if (!ready || !active || busy || plan || !name.trim() || !links || (existing && !createAnother)) return;
    const id = draft.local_id;
    const key = context;
    busy = true;
    failure = null;
    outcome = null;
    try {
      const result = await client.planPortable(id, {
        connection_id: active.local_id, revision: draft.revision, name: name.trim(), create_another: createAnother,
        ...(hasAccessSelection(createAccess) ? {access: createAccess} : {})
      });
      if (key === context && result.plan.write_requests === 1) plan = result;
      else await client.discard(id, result.handle);
    } catch (error) {
      if (key === context) failure = publicationFailure(error);
    } finally {
      busy = false;
    }
  }

  function discard() {
    const current = plan;
    plan = null;
    acknowledgeDeletions = false;
    acknowledgeRepeatAppend = false;
    acknowledgePublicAccess = false;
    if (current) void client.discard(draft.local_id, current.handle).catch(() => {});
  }

  async function confirm() {
    const current = plan;
    if (!current || !ready || busy || confirmationPending) return;
    const acknowledged = removesContent && acknowledgeDeletions;
    const repeatAcknowledged = repeatsAppend && acknowledgeRepeatAppend;
    const publicAcknowledged = publicAccess && acknowledgePublicAccess;
    const id = draft.local_id;
    const key = context;
    plan = null;
    acknowledgeDeletions = false;
    acknowledgeRepeatAppend = false;
    acknowledgePublicAccess = false;
    busy = true;
    try {
      const result = await client.confirm(id, {
        handle: current.handle, plan_id: current.plan.plan_id, confirmed: true,
        ...(current.plan.action === 'create_dashboard' ? {name: current.plan.target_name} : {}),
        ...(acknowledged ? {acknowledge_deletions: true} : {}),
        ...(repeatAcknowledged ? {acknowledge_repeat_append: true} : {}),
        ...(publicAcknowledged ? {acknowledge_public_access: true} : {})
      });
      if (key === context) {
        statuses = {};
        outcome = result;
        accessTarget = null;
      }
    } catch (error) {
      if (key === context) failure = publicationFailure(error);
    } finally {
      await refresh(id, key);
      busy = false;
    }
  }

  async function reconcile(operationId: string) {
    if (busy) return;
    const id = draft.local_id;
    const key = context;
    busy = true;
    failure = null;
    try {
      const result = await client.reconcile(operationId);
      if (key === context) outcome = result;
    } catch (error) {
      if (key === context) failure = publicationFailure(error);
    } finally {
      await refresh(id, key);
      busy = false;
    }
  }
</script>

<section aria-labelledby="portable-publication-title">
  <h3 id="portable-publication-title">{i18n.t('publication.title')}</h3>
  {#if links?.deployments.length}
    <h4>{i18n.t('publication.portable.created')}</h4>
    <ul>
      {#each links.deployments as link (link.deployment_id)}
        {@const url = profile && link.connection_id === profile.local_id ? zabbixDashboardUrl(profile.endpoint_display, link.dashboard.dashboardid) : null}
        {@const checked = statuses[link.deployment_id]}
        {@const status = checked?.state}
        <li>
          {#if url}<a href={url} target="_blank" rel="noreferrer">{link.dashboard_name}</a>{:else}{link.dashboard_name}{/if}
          {#if active}
            <Button {i18n} label={{id:'publication.portable.check_link'}} disabled={busy} onclick={() => void checkDeployment(link.deployment_id)} />
          {/if}
          {#if status}
            <span role="status">{i18n.t(status === 'current' ? 'publication.portable.current' : status === 'changed' ? 'publication.portable.changed'
              : status === 'other_server' ? 'publication.portable.other_server' : 'publication.portable.missing')}</span>
          {/if}
          {#if checked?.remote?.changes.length}
            <ul>
              {#each checked.remote.changes as change}
                <li>
                  {resolveText(i18n, REMOTE_CHANGE_LABEL[change.kind])}
                  {#if change.page_name}<span>{change.page_name}</span>{/if}
                  {#if change.widget_name}<span>{change.widget_name}</span>{/if}
                </li>
              {/each}
            </ul>
          {/if}
          {#if ready && link.connection_id === active?.local_id && (!status || status === 'current')}
            <Button {i18n} label={{id:'publication.portable.update_created'}} disabled={busy || recoverable.length > 0}
              onclick={() => void prepareUpdate({kind:'update_created', deployment_id:link.deployment_id})} />
            {#if accessChoices && link.source_revision === draft.revision}
              <Button {i18n} label={{id:'publication.access.edit'}} disabled={busy || recoverable.length > 0}
                onclick={() => void editAccess(link.deployment_id)} />
            {/if}
          {/if}
          {#if accessChoices && accessTarget?.deploymentId === link.deployment_id}
            <DashboardAccessEditor {i18n} choices={accessChoices} current={accessTarget.current}
              bind:selection={updateAccess} disabled={busy} />
            <Button {i18n} label={{id:'publication.action.confirm_dialog'}} disabled={busy || !hasAccessSelection(updateAccess)}
              onclick={() => void prepareUpdate({kind:'change_access', deployment_id:link.deployment_id, access:updateAccess})} />
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#each recoverable as entry (entry.operation_id)}
    <p>{entry.target.name} · {resolveText(i18n, stateLabel(entry.state))}</p>
    <Button {i18n} label={{id:'publication.action.reconcile'}} disabled={busy || entry.progress === 'running'} onclick={() => void reconcile(entry.operation_id)} />
  {/each}
  {#if !ready}
    <p>{i18n.t('publication.portable.require_connection')}</p>
  {:else}
    <p>{profile?.name}</p>
    {#if !localPage}
    <label>
      {i18n.t('publication.target.name')}
      <input type="text" bind:value={name} maxlength="255" disabled={busy} />
    </label>
    {#if existing || failure?.code === 'portable_already_deployed'}
      <label><input type="checkbox" bind:checked={createAnother} disabled={busy} />{i18n.t('publication.portable.create_another')}</label>
    {/if}
    {#if accessChoices}
      <DashboardAccessEditor {i18n} choices={accessChoices} bind:selection={createAccess} disabled={busy} />
    {/if}
    <Button {i18n} label={{id:'publication.action.confirm_dialog'}} disabled={busy || !links || !name.trim() || (existing && !createAnother) || recoverable.length > 0} onclick={() => void prepare()} />
    {/if}
    <Button {i18n} label={{id:'publication.portable.choose_target'}} disabled={busy || recoverable.length > 0} onclick={() => void chooseTargets()} />
    {#if choosing}
      <label>{i18n.t('publication.portable.choose_target')}
        <select bind:value={dashboardid} onchange={() => void selectDashboard()} disabled={busy}>
          <option value="">{i18n.t('publication.portable.select')}</option>
          {#each targets as target (target.dashboardid)}<option value={target.dashboardid}>{target.name}</option>{/each}
        </select>
      </label>
      {#if destination}
        {#if sourcePages.length > 1}
          <label>{i18n.t('publication.portable.source_page')}
            <select bind:value={sourcePageId} onchange={() => { placements = {}; }} disabled={busy}>
              <option value="">{i18n.t('publication.portable.select')}</option>
              {#each sourcePages as page (page.id)}<option value={page.id}>{page.name}</option>{/each}
            </select>
          </label>
        {/if}
        {#if sourcePage}
          <label>{i18n.t('publication.portable.page_position')}
            <select bind:value={afterPage} disabled={busy}>
              <option value="">{i18n.t('publication.portable.position_end')}</option>
              {#each destination.pages as page (page.dashboard_pageid)}<option value={page.dashboard_pageid}>
                {i18n.t('publication.portable.position_after', {name: page.name})}
              </option>{/each}
            </select>
          </label>
          <Button {i18n} label={{id:'publication.portable.append_page'}} disabled={busy}
            onclick={() => void prepareUpdate({kind:'append_page', dashboardid, source_page_id:sourcePage.id, ...(afterPage ? {after_pageid: afterPage} : {})})} />
        {/if}
        <label>{i18n.t('publication.portable.target_page')}
          <select bind:value={pageid} onchange={() => { placements = {}; }} disabled={busy}>
            <option value="">{i18n.t('publication.portable.select')}</option>
            {#each destination.pages as page (page.dashboard_pageid)}<option value={page.dashboard_pageid}>{page.name}</option>{/each}
          </select>
        </label>
        {#if targetPage && sourcePage}
          <fieldset>
            <legend>{i18n.t('publication.portable.source_widgets')}</legend>
            {#each graphWidgets as widget (widget.id)}
              <label><input type="checkbox" checked={placements[widget.id] !== undefined}
                disabled={busy || (selected.length >= maxPageWidgets && !placements[widget.id])}
                onchange={(event) => chooseWidget(widget,event.currentTarget.checked)} />
                {widget.type === 'zabbix' ? widget.name : widget.title}</label>
              {#if placements[widget.id]}
                <label>{i18n.t('publication.portable.location_x')}<input type="number" min="0" max={72-widget.layout.w} bind:value={placements[widget.id].x} disabled={busy} /></label>
                <label>{i18n.t('publication.portable.location_y')}<input type="number" min="0" max={64-widget.layout.h} bind:value={placements[widget.id].y} disabled={busy} /></label>
              {/if}
            {/each}
          </fieldset>
          <div class="placement-preview" aria-hidden="true">
            {#each targetPage.widgets as widget (widget.id)}
              <div class="occupied" style:grid-column={`${widget.x + 1} / span ${widget.width}`}
                style:grid-row={`${widget.y + 1} / span ${widget.height}`}>
                {widget.name}
              </div>
            {/each}
            {#each selected as widget (widget.id)}
              {@const location = placements[widget.id]}
              {#if location.x !== undefined && location.y !== undefined && location.x >= 0 && location.y >= 0 &&
                   location.x + widget.layout.w <= 72 && location.y + widget.layout.h <= 64}
                <div class="addition" style:grid-column={`${location.x + 1} / span ${widget.layout.w}`}
                  style:grid-row={`${location.y + 1} / span ${widget.layout.h}`}>
                  {widget.type === 'zabbix' ? widget.name : widget.title}
                </div>
              {/if}
            {/each}
          </div>
          <Button {i18n} label={{id:'publication.portable.append_selected'}} disabled={busy || selected.length === 0 ||
            selected.some((widget) => !Number.isInteger(placements[widget.id].x) || !Number.isInteger(placements[widget.id].y))}
            onclick={prepareSelected} />
        {/if}
      {/if}
    {/if}
  {/if}
  {#if failure}<p role="alert">{resolveText(i18n, publicationCodeText(i18n, failure))}</p>{/if}
  {#if outcome}
    <p role="status">{resolveText(i18n, stateLabel(outcome.state))}</p>
    {#if outcome.record_error}<p role="alert">{i18n.t('publication.portable.link_pending')}</p>{/if}
  {/if}
</section>

{#if plan}
  <Dialog {i18n} id="portable-publication-confirm" open title={{id:'publication.confirm.title'}}
    description={{id:plan.plan.action === 'create_dashboard' ? 'publication.confirm.question_create' : 'publication.portable.question_update',params:{name:plan.plan.target_name}}}
    onclose={discard}>
    {#if plan.plan.action === 'create_dashboard'}
      <p>{i18n.t('publication.portable.summary', { profile: profile?.name ?? '', pages: plan.plan.page_count, widgets: plan.plan.widget_count })}</p>
      {#if plan.plan.access}
        <DashboardAccessSummary {i18n} access={plan.plan.access} choices={accessChoices} label={i18n.t('publication.access.title')} />
      {:else}
        <p>{i18n.t('publication.access.private')}</p>
      {/if}
    {:else}
      <p>{i18n.t('publication.portable.preview_source', {name: plan.plan.source_name})}</p>
      {#if plan.plan.access_change}
        <h4>{i18n.t('publication.access.title')}</h4>
        {#if accessChoices?.can_choose_owner === false}
          <p>{i18n.t('publication.access.visible_part')}</p>
        {/if}
        <DashboardAccessSummary {i18n} access={plan.plan.access_change.before} choices={accessChoices} label={i18n.t('publication.access.before')} />
        <DashboardAccessSummary {i18n} access={plan.plan.access_change.after} choices={accessChoices} label={i18n.t('publication.access.after')} />
      {/if}
      <ul>
        {#each plan.plan.changes as change}
          <li>
            {#if change.kind === 'add_page'}
              {i18n.t('publication.portable.diff_page', {name:change.name, number:change.number})}
            {:else if change.kind === 'add_widget'}
              {i18n.t('publication.portable.diff_widget', change)}
            {:else if change.kind === 'remove_page'}
              {i18n.t('publication.portable.diff_remove_page', change)}
            {:else if change.kind === 'remove_widget'}
              {i18n.t('publication.portable.diff_remove_widget', change)}
            {:else if change.kind === 'move_page'}
              {i18n.t('publication.portable.diff_move_page', change)}
            {:else if change.kind === 'geometry'}
              {i18n.t('publication.portable.diff_geometry', { page: change.page, widget: change.widget,
                old_x: change.before[0], old_y: change.before[1], old_w: change.before[2], old_h: change.before[3],
                new_x: change.after[0], new_y: change.after[1], new_w: change.after[2], new_h: change.after[3] })}
            {:else if change.kind === 'name'}
              {i18n.t(change.object === 'dashboard' ? 'publication.portable.diff_dashboard_name'
                : change.object === 'page' ? 'publication.portable.diff_page_name' : 'publication.portable.diff_widget_name', change)}
            {:else if change.before === null}
              {i18n.t('publication.portable.diff_add_field', change)}
            {:else}
              {i18n.t('publication.portable.diff_field', {page:change.page, widget:change.widget, name:change.name, before:change.before, after:change.after})}
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if removesContent}
      <label><input type="checkbox" bind:checked={acknowledgeDeletions} /> {i18n.t('publication.portable.acknowledge_deletions')}</label>
    {/if}
    {#if repeatsAppend}
      <p>{i18n.t('publication.portable.equivalent_page_exists')}</p>
      <label><input type="checkbox" bind:checked={acknowledgeRepeatAppend} /> {i18n.t('publication.portable.append_again')}</label>
    {/if}
    {#if publicAccess}
      <label><input type="checkbox" bind:checked={acknowledgePublicAccess} /> {i18n.t('publication.access.public_warning')}</label>
    {/if}
    <Button {i18n} label={{id:'common.action.cancel'}} onclick={discard} />
    <Button {i18n} label={{id:'publication.action.confirm_write'}} disabled={busy || !ready || confirmationPending} onclick={() => void confirm()} />
  </Dialog>
{/if}

<style>
  .placement-preview {
    display: grid;
    grid-template-columns: repeat(72, minmax(0, 1fr));
    grid-template-rows: repeat(64, minmax(0, 1fr));
    aspect-ratio: 72 / 64;
    max-width: 32rem;
    border: 1px solid var(--grid-bounds);
    background: var(--surface-canvas);
  }
  .occupied, .addition {
    min-width: 0;
    min-height: 0;
    padding-inline: .2rem;
    overflow: hidden;
    font-size: .75rem;
    background: var(--surface-raised);
  }
  .occupied { border: 1px solid var(--grid-bounds); }
  .addition { border: 1px dashed var(--focus-ring); background: var(--surface-selected); z-index: 1; }
</style>
