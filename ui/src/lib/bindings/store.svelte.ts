// State of «Привязки» (ReplaceHost wizard, P1-05/P2-03E) and the ReplaceHost preview of
// «Публикация» (P1-08B/C, P2-03D) for one draft. Kept in the shell so that moving between the
// two sections keeps the loaded preview and the wizard choices. Server data of another draft is
// never shown: opening a different draft starts from an empty state.

import type { LocalizedText } from '../components';
import { failureOf, type CommandFailure } from '../connections/client';
import { MemoryFailure, type ReplaceHostClient } from './client';
import {
  keyId,
  type FieldKey,
  type OperationRecord,
  type ReplaceHostInventory,
  type ReplaceHostPreview,
  type ReplaceScope,
  type RowDecision,
  type TargetHostCandidate,
  type TargetHostSearch
} from './model';

export const WIZARD_STEPS = ['scope', 'dependencies', 'new_host', 'mapping', 'summary'] as const;
export type WizardStep = (typeof WIZARD_STEPS)[number];
export type ScopeKind = ReplaceScope['kind'];
export type Busy = 'calculate' | 'decide' | 'confirm' | 'undo' | 'reset' | 'check';

export function bindingsFailure(error: unknown): CommandFailure {
  if (error instanceof MemoryFailure) return { code: error.code, params: error.params, status: 0 };
  return failureOf(error);
}

export class ReplaceHostStore {
  localId = $state<string | null>(null);
  /** `none`: the draft has no calculated replacement yet. */
  status = $state<'idle' | 'loading' | 'none' | 'ready' | 'failed'>('idle');
  preview = $state<ReplaceHostPreview | null>(null);
  loadFailure = $state<CommandFailure | null>(null);

  inventory = $state<ReplaceHostInventory | null>(null);
  inventoryLoading = $state(false);
  inventoryFailure = $state<CommandFailure | null>(null);

  step = $state<WizardStep>('scope');
  sourceHostid = $state<string | null>(null);
  scopeKind = $state<ScopeKind>('dashboard');
  scopePages = $state<string[]>([]);
  scopeWidgets = $state<string[]>([]);

  searchInput = $state('');
  searchResult = $state<TargetHostSearch | null>(null);
  searchLoading = $state(false);
  searchFailure = $state<CommandFailure | null>(null);
  target = $state<TargetHostCandidate | null>(null);

  busy = $state<Busy | null>(null);
  commandFailure = $state<CommandFailure | null>(null);
  /** Result of the last command, announced politely. */
  announcement = $state<LocalizedText | null>(null);
  history = $state<ReadonlyArray<OperationRecord> | null>(null);
  /** `keyId` of the row open in the 09-03 inspector. */
  selectedRow = $state<string | null>(null);

  #request = 0;
  #inventoryAbort: AbortController | null = null;
  #searchAbort: AbortController | null = null;

  constructor(readonly client: ReplaceHostClient) {}

  /** Switch to a draft and load its stored replacement; the same draft keeps its state. */
  open(localId: string): void {
    if (this.localId === localId && this.status !== 'idle') return;
    this.#inventoryAbort?.abort();
    this.#searchAbort?.abort();
    this.#request += 1;
    this.localId = localId;
    this.status = 'idle';
    this.preview = null;
    this.loadFailure = null;
    this.inventory = null;
    this.inventoryFailure = null;
    this.inventoryLoading = false;
    this.resetChoices();
    this.busy = null;
    this.commandFailure = null;
    this.announcement = null;
    this.history = null;
    this.selectedRow = null;
    void this.load();
  }

  resetChoices(): void {
    this.step = 'scope';
    this.sourceHostid = null;
    this.scopeKind = 'dashboard';
    this.scopePages = [];
    this.scopeWidgets = [];
    this.searchInput = '';
    this.searchResult = null;
    this.searchFailure = null;
    this.searchLoading = false;
    this.target = null;
  }

  async load(): Promise<void> {
    const localId = this.localId;
    if (localId === null) return;
    const request = ++this.#request;
    this.status = 'loading';
    this.loadFailure = null;
    try {
      const preview = await this.client.preview(localId);
      if (request !== this.#request) return;
      this.#show(preview);
      if (this.step === 'scope') this.step = 'mapping';
    } catch (error) {
      if (request !== this.#request) return;
      const failure = bindingsFailure(error);
      if (failure.code === 'replace_host_not_calculated') {
        this.preview = null;
        this.status = 'none';
      } else {
        this.preview = null;
        this.loadFailure = failure;
        this.status = 'failed';
      }
    }
  }

  #show(preview: ReplaceHostPreview | null): void {
    this.preview = preview;
    this.status = preview ? 'ready' : 'none';
    this.history = null;
    if (preview && this.selectedRow !== null && !preview.rows.some((row) => keyId(row.key) === this.selectedRow)) {
      this.selectedRow = null;
    }
  }

  async loadInventory(): Promise<void> {
    const localId = this.localId;
    if (localId === null) return;
    this.#inventoryAbort?.abort();
    const abort = new AbortController();
    this.#inventoryAbort = abort;
    this.inventoryLoading = true;
    this.inventoryFailure = null;
    try {
      const inventory = await this.client.inventory(localId, abort.signal);
      if (abort.signal.aborted || localId !== this.localId) return;
      this.inventory = inventory;
      const first = inventory.hosts[0];
      if (this.sourceHostid === null && first) this.sourceHostid = first.hostid;
      if (this.sourceHostid !== null && !inventory.hosts.some((host) => host.hostid === this.sourceHostid)) {
        this.sourceHostid = first?.hostid ?? null;
      }
    } catch (error) {
      if (abort.signal.aborted || localId !== this.localId) return;
      const failure = bindingsFailure(error);
      if (failure.code === 'aborted') return;
      this.inventory = null;
      this.inventoryFailure = failure;
    } finally {
      if (this.#inventoryAbort === abort) {
        this.inventoryLoading = false;
        this.#inventoryAbort = null;
      }
    }
  }

  get sourceHost() {
    return this.inventory?.hosts.find((host) => host.hostid === this.sourceHostid) ?? null;
  }

  /** The scope as the backend takes it; `null` while a partial scope has no selection. */
  get scope(): ReplaceScope | null {
    switch (this.scopeKind) {
      case 'dashboard':
        return { kind: 'dashboard' };
      case 'pages':
        return this.scopePages.length > 0 ? { kind: 'pages', dashboard_pageids: [...this.scopePages].sort() } : null;
      case 'widgets':
        return this.scopeWidgets.length > 0 ? { kind: 'widgets', widgetids: [...this.scopeWidgets].sort() } : null;
    }
  }

  setSource(hostid: string): void {
    if (this.sourceHostid === hostid) return;
    this.sourceHostid = hostid;
    // The new host and its template coverage depend on the replaced host.
    this.target = null;
    this.searchResult = null;
    this.searchFailure = null;
  }

  async searchTargets(): Promise<void> {
    const localId = this.localId;
    const source = this.sourceHostid;
    if (localId === null || source === null) return;
    this.#searchAbort?.abort();
    const abort = new AbortController();
    this.#searchAbort = abort;
    this.searchLoading = true;
    this.searchFailure = null;
    try {
      const result = await this.client.searchHosts(localId, source, this.searchInput, abort.signal);
      if (abort.signal.aborted || localId !== this.localId) return;
      this.searchResult = result;
    } catch (error) {
      if (abort.signal.aborted || localId !== this.localId) return;
      const failure = bindingsFailure(error);
      if (failure.code === 'aborted') return;
      this.searchResult = null;
      this.searchFailure = failure;
    } finally {
      if (this.#searchAbort === abort) {
        this.searchLoading = false;
        this.#searchAbort = null;
      }
    }
  }

  chooseTarget(hostid: string): void {
    this.target = this.searchResult?.hosts.find((host) => host.hostid === hostid) ?? null;
  }

  async #command(busy: Busy, run: (localId: string) => Promise<ReplaceHostPreview | null>, done: LocalizedText): Promise<boolean> {
    const localId = this.localId;
    if (localId === null || this.busy !== null) return false;
    this.busy = busy;
    this.commandFailure = null;
    this.announcement = null;
    try {
      const preview = await run(localId);
      if (localId !== this.localId) return false;
      this.#show(preview);
      this.announcement = done;
      return true;
    } catch (error) {
      if (localId !== this.localId) return false;
      this.commandFailure = bindingsFailure(error);
      return false;
    } finally {
      if (localId === this.localId) this.busy = null;
    }
  }

  /** «Далее: соответствия»: the read-only calculation, stored as an operation of the draft. */
  async calculate(): Promise<boolean> {
    const scope = this.scope;
    const source = this.sourceHostid;
    const target = this.target;
    if (scope === null || source === null || target === null) return false;
    const ok = await this.#command(
      'calculate',
      (localId) => this.client.calculate(localId, { source_hostid: source, target_hostid: target.hostid, scope }),
      { id: 'bindings.calculated.done' }
    );
    if (ok) this.step = 'mapping';
    return ok;
  }

  decide(key: FieldKey, decision: RowDecision): Promise<boolean> {
    return this.#command('decide', (localId) => this.client.decide(localId, key, decision), { id: 'bindings.decision.done' });
  }

  undo(seq: number): Promise<boolean> {
    return this.#command(
      'undo',
      async (localId) => (await this.client.removeOperation(localId, seq)).preview,
      { id: 'bindings.decision.undone' }
    );
  }

  confirmKept(): Promise<boolean> {
    const digest = this.preview?.kept_on_source.digest;
    if (digest === undefined) return Promise.resolve(false);
    return this.#command('confirm', (localId) => this.client.confirmKept(localId, digest), { id: 'publication.kept.confirmed_done' });
  }

  /** «Сбросить все привязки»: removes the calculation with its decisions. */
  async reset(): Promise<boolean> {
    const seq = this.preview?.operation.seq;
    if (seq === undefined) return false;
    const ok = await this.#command(
      'reset',
      async (localId) => (await this.client.removeOperation(localId, seq)).preview,
      { id: 'bindings.reset.done' }
    );
    if (ok) {
      this.resetChoices();
      this.selectedRow = null;
    }
    return ok;
  }

  /** «Проверить все»: a fresh read; nothing is written and nothing is chosen automatically. */
  revalidate(): Promise<boolean> {
    return this.#command('check', (localId) => this.client.revalidate(localId), { id: 'bindings.checked.done' });
  }

  async loadHistory(): Promise<void> {
    const localId = this.localId;
    if (localId === null) return;
    try {
      const history = await this.client.history(localId);
      if (localId === this.localId) this.history = history;
    } catch (error) {
      if (localId === this.localId) this.commandFailure = bindingsFailure(error);
    }
  }
}
