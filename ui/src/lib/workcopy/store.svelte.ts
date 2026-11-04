// State of «Панели в Zabbix» and the copy check for one connection (P1-03B/C/D): query, the
// loaded list, the selected dashboard with its preview and the running import. Kept in the shell so
// that the copy check and the way back keep the search, filter and selection. Server data is
// never cached across connections: a different connection starts from an empty state.

import { failureOf, type CommandFailure } from '../connections/client';
import type {
  AccessFilter,
  ImportOutcome,
  ImportPreview,
  ImportScope,
  ServerClient,
  ServerDashboardList,
  ServerDashboardRow
} from './client';
import type { DraftsClient } from '../shell/drafts';
import { SERVER_PAGE_SIZE, copyDraftName, type CopyNaming } from './server';

export type ImportKind = ImportScope['scope'];

export class ServerDashboardsStore {
  connectionLocalId = $state<string | null>(null);
  /** Text of the search field; applied on submit. */
  searchInput = $state('');
  search = $state('');
  access = $state<AccessFilter>('all');
  /** 1-based table page. */
  page = $state(1);
  list = $state<ServerDashboardList | null>(null);
  listFailure = $state<CommandFailure | null>(null);
  loading = $state(false);
  selectedId = $state<string | null>(null);
  /** The selected row as last listed; kept while the list reloads. */
  selectedRow = $state<ServerDashboardRow | null>(null);
  preview = $state<ImportPreview | null>(null);
  previewFailure = $state<CommandFailure | null>(null);
  previewLoading = $state(false);
  importing = $state<ImportKind | null>(null);
  importFailure = $state<CommandFailure | null>(null);

  /** Context generation of the connection (P2-01): a new sign-in or server starts afresh. */
  #generation: number | null = null;
  #listRequest = 0;
  #previewRequest = 0;
  #listAbort: AbortController | null = null;
  #previewAbort: AbortController | null = null;

  /** `drafts` lists local drafts to name another copy of the same source (P2-08A). */
  constructor(
    readonly client: ServerClient,
    readonly drafts: Pick<DraftsClient, 'list'> | null = null
  ) {}

  /**
   * Switch to a connection. A different connection or a new context generation (reconnect,
   * changed server) clears every server datum read before, so no old list is shown as current.
   */
  open(connectionLocalId: string, generation: number | null = null): void {
    if (this.connectionLocalId === connectionLocalId && this.#generation === generation) return;
    this.#listAbort?.abort();
    this.#previewAbort?.abort();
    this.#listRequest += 1;
    this.#previewRequest += 1;
    this.connectionLocalId = connectionLocalId;
    this.#generation = generation;
    this.searchInput = '';
    this.search = '';
    this.access = 'all';
    this.page = 1;
    this.list = null;
    this.listFailure = null;
    this.clearSelection();
  }

  /** The connection ended: forget its server data. */
  close(): void {
    this.#listAbort?.abort();
    this.#previewAbort?.abort();
    this.#listRequest += 1;
    this.#previewRequest += 1;
    this.connectionLocalId = null;
    this.#generation = null;
    this.list = null;
    this.listFailure = null;
    this.loading = false;
    this.clearSelection();
  }

  clearSelection(): void {
    this.#previewAbort?.abort();
    this.selectedId = null;
    this.selectedRow = null;
    this.preview = null;
    this.previewFailure = null;
    this.previewLoading = false;
    this.importFailure = null;
  }

  async load(): Promise<void> {
    const connectionLocalId = this.connectionLocalId;
    if (connectionLocalId === null) return;
    const request = ++this.#listRequest;
    this.#listAbort?.abort();
    const abort = new AbortController();
    this.#listAbort = abort;
    this.loading = true;
    this.listFailure = null;
    try {
      const list = await this.client.list(
        connectionLocalId,
        { search: this.search, access: this.access, offset: (this.page - 1) * SERVER_PAGE_SIZE, limit: SERVER_PAGE_SIZE },
        abort.signal
      );
      if (request !== this.#listRequest) return;
      this.list = list;
      const selected = list.rows.find((row) => row.dashboardid === this.selectedId);
      if (selected) this.selectedRow = selected;
    } catch (error) {
      if (request !== this.#listRequest) return;
      const failure = failureOf(error);
      if (failure.code === 'aborted') return;
      // The last list is not shown as current after a failed read (P1-03B).
      this.list = null;
      this.listFailure = failure;
    } finally {
      if (request === this.#listRequest) this.loading = false;
    }
  }

  applySearch(): Promise<void> {
    this.search = this.searchInput.trim();
    this.page = 1;
    return this.load();
  }

  setAccess(access: AccessFilter): Promise<void> {
    this.access = access;
    this.page = 1;
    return this.load();
  }

  setPage(page: number): Promise<void> {
    this.page = page;
    return this.load();
  }

  /** Select a row and read its composition (one `dashboard.get`, nothing stored). */
  async select(row: ServerDashboardRow): Promise<void> {
    if (this.selectedId === row.dashboardid && (this.preview || this.previewLoading)) {
      this.selectedRow = row;
      return;
    }
    this.clearSelection();
    this.selectedId = row.dashboardid;
    this.selectedRow = row;
    await this.loadPreview();
  }

  async loadPreview(): Promise<void> {
    const connectionLocalId = this.connectionLocalId;
    const dashboardid = this.selectedId;
    if (connectionLocalId === null || dashboardid === null) return;
    const request = ++this.#previewRequest;
    this.#previewAbort?.abort();
    const abort = new AbortController();
    this.#previewAbort = abort;
    this.previewLoading = true;
    this.previewFailure = null;
    try {
      const preview = await this.client.preview(connectionLocalId, dashboardid, abort.signal);
      if (request === this.#previewRequest) this.preview = preview;
    } catch (error) {
      if (request !== this.#previewRequest) return;
      const failure = failureOf(error);
      if (failure.code !== 'aborted') this.previewFailure = failure;
    } finally {
      if (request === this.#previewRequest) this.previewLoading = false;
    }
  }

  /**
   * «Скопировать с сервера» / «Взять страницу» (P1-03D): one consistent import. There is no
   * cancel: the server finishes or rejects the whole copy, so a cancelled request could still
   * create it. Returns the outcome, or `null` with `importFailure` set. With `naming`, another
   * copy of the same source is named «<name> (copy N)» (P2-08A).
   */
  async importCopy(scope: ImportScope, naming?: CopyNaming): Promise<ImportOutcome | null> {
    const connectionLocalId = this.connectionLocalId;
    const dashboardid = this.selectedId;
    if (connectionLocalId === null || dashboardid === null || this.importing !== null) return null;
    this.importing = scope.scope;
    this.importFailure = null;
    try {
      const name = naming ? await this.#copyName(scope, naming) : undefined;
      const outcome = await this.client.importDashboard(connectionLocalId, dashboardid, scope, name);
      // The list and the preview now miss the new copy; read them again when shown.
      this.list = null;
      this.preview = null;
      return outcome;
    } catch (error) {
      this.importFailure = failureOf(error);
      return null;
    } finally {
      this.importing = null;
    }
  }

  /**
   * The name of a new copy, or `undefined` for the source name. Copies of the selected dashboard
   * come from the server read (`existing_copies`); a page copy counts only for the same page.
   * Without a readable draft list the backend default name is kept.
   */
  async #copyName(scope: ImportScope, naming: CopyNaming): Promise<string | undefined> {
    if (this.drafts === null) return undefined;
    let listed;
    try {
      listed = await this.drafts.list();
    } catch {
      return undefined;
    }
    if (listed.status !== 'ready') return undefined;
    const copies = new Set([...(this.preview?.existing_copies ?? []), ...(this.selectedRow?.existing_copies ?? [])].map((copy) => copy.local_id));
    const hasCopy = listed.drafts.some(
      (draft) =>
        copies.has(draft.local_id) &&
        draft.kind === scope.scope &&
        (scope.scope === 'dashboard' || draft.source?.page_number === naming.pageNumber)
    );
    return copyDraftName(naming, hasCopy, listed.drafts.map((draft) => draft.name));
  }
}
