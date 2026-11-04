// State of «Публикация» steps 1, 4 and 5 (P1-08E/F/G) for one draft, and of the journal recovery
// banner (P1-01G §4/§7). The plan itself never reaches the browser: only its opaque handle and the
// preview the dialog shows. Opening another draft starts from an empty state.

import type { LocalizedText } from '../components';
import type { ReplaceHostPreview } from '../bindings/model';
import { publicationFailure, type PublicationClient } from './client';
import type {
  CheckResult,
  JournalEntry,
  JournalList,
  Outcome,
  PlanRequest,
  PlanResult,
  PublicationAction,
  PublicationFailure,
  RefetchResult
} from './model';

export const PUBLICATION_STEPS = ['target', 'checks', 'changes', 'confirm', 'write'] as const;
export type PublicationStep = (typeof PUBLICATION_STEPS)[number];

/** P1-08G: after 10 s without an answer the stage says «Ожидание ответа сервера…». */
export const WAITING_AFTER_MS = 10_000;

export class PublicationStore {
  localId = $state<string | null>(null);
  step = $state<PublicationStep>('target');
  /** No default (P1-08E): `null` until the user chooses. */
  action = $state<PublicationAction | null>(null);
  /** User data: the name of the new dashboard. */
  copyName = $state('');

  check = $state<CheckResult | null>(null);
  /** Epoch milliseconds of the answer to the last check. */
  checkedAt = $state<number | null>(null);
  checking = $state(false);
  checkFailure = $state<PublicationFailure | null>(null);

  plan = $state<PlanResult | null>(null);
  planning = $state(false);
  planFailure = $state<PublicationFailure | null>(null);

  writing = $state(false);
  waitingLong = $state(false);
  outcome = $state<Outcome | null>(null);
  /** Refusal of a confirm or reconcile, before or without an outcome. */
  writeFailure = $state<PublicationFailure | null>(null);
  reconciling = $state(false);

  refetching = $state(false);
  refetched = $state<RefetchResult | null>(null);
  refetchFailure = $state<PublicationFailure | null>(null);

  announcement = $state<LocalizedText | null>(null);

  #request = 0;
  #timer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    readonly client: PublicationClient,
    /** Called after anything that may change the journal (write, reconcile). */
    readonly onjournal: () => void = () => {}
  ) {}

  open(localId: string): void {
    if (this.localId === localId) return;
    this.#request += 1;
    this.#stopTimer();
    this.localId = localId;
    this.step = 'target';
    this.action = null;
    this.copyName = '';
    this.check = null;
    this.checkedAt = null;
    this.checking = false;
    this.checkFailure = null;
    this.plan = null;
    this.planning = false;
    this.planFailure = null;
    this.writing = false;
    this.waitingLong = false;
    this.outcome = null;
    this.writeFailure = null;
    this.reconciling = false;
    this.refetching = false;
    this.refetched = null;
    this.refetchFailure = null;
    this.announcement = null;
  }

  /** The step bar and «Назад»: a step before the write; the outcome stays visible on step 5. */
  goto(step: PublicationStep): void {
    if (this.writing) return;
    this.step = step;
  }

  /** P1-08F: changing the action invalidates the plan. */
  setAction(action: PublicationAction): void {
    if (this.action === action) return;
    this.action = action;
    void this.discardPlan();
  }

  /** `publication.recheck`: read-only; any mode. */
  async recheck(): Promise<void> {
    const localId = this.localId;
    if (localId === null || this.checking) return;
    const request = this.#request;
    this.checking = true;
    this.checkFailure = null;
    try {
      const result = await this.client.check(localId);
      if (request !== this.#request) return;
      this.check = result;
      this.checkedAt = Date.now();
      if (result.source.state !== 'current') void this.discardPlan();
      this.announcement = result.source.state === 'current' ? { id: 'bindings.checked.done' } : { id: 'publication.state.conflict' };
    } catch (error) {
      if (request !== this.#request) return;
      this.checkFailure = publicationFailure(error);
    } finally {
      if (request === this.#request) this.checking = false;
    }
  }

  /** What the plan is bound to: the freshest preview the user saw. */
  planRequest(preview: ReplaceHostPreview): PlanRequest | null {
    const common = { revision: preview.revision, base_fingerprint: preview.base_fingerprint, local_fingerprint: preview.local_fingerprint };
    if (this.action === 'update_source') {
      const kept = preview.kept_on_source;
      return { action: 'update_source', ...common, kept_digest: kept.entries.length > 0 ? kept.digest : null };
    }
    if (this.action === 'save_as_copy') {
      const name = this.copyName.trim();
      return name === '' ? null : { action: 'save_as_copy', ...common, name };
    }
    return null;
  }

  /** «Подтвердить запись…»: the server re-reads and verifies everything, then keeps the plan. */
  async buildPlan(preview: ReplaceHostPreview): Promise<boolean> {
    const localId = this.localId;
    const body = this.planRequest(preview);
    if (localId === null || body === null || this.planning) return false;
    const request = this.#request;
    this.planning = true;
    this.planFailure = null;
    try {
      const plan = await this.client.plan(localId, body);
      if (request !== this.#request) return false;
      this.plan = plan;
      return true;
    } catch (error) {
      if (request !== this.#request) return false;
      this.planFailure = publicationFailure(error);
      return false;
    } finally {
      if (request === this.#request) this.planning = false;
    }
  }

  /** «Отмена»: the plan is dropped on the server too. */
  async discardPlan(): Promise<void> {
    const plan = this.plan;
    const localId = this.localId;
    this.plan = null;
    if (plan === null || localId === null) return;
    try {
      await this.client.discard(localId, plan.handle);
    } catch {
      // The plan is one-shot and ends with the session anyway.
    }
  }

  /** «Записать в Zabbix»: exactly what the dialog showed; the plan is consumed. */
  async confirm(): Promise<void> {
    const localId = this.localId;
    const plan = this.plan;
    if (localId === null || plan === null || this.writing) return;
    const request = this.#request;
    this.plan = null;
    this.writing = true;
    this.waitingLong = false;
    this.outcome = null;
    this.writeFailure = null;
    this.step = 'write';
    this.#timer = setTimeout(() => {
      if (request === this.#request) this.waitingLong = true;
    }, WAITING_AFTER_MS);
    const body =
      plan.plan.action === 'update_source'
        ? { handle: plan.handle, plan_id: plan.plan.plan_id, confirmed: true as const, kept_digest: plan.plan.kept_digest }
        : { handle: plan.handle, plan_id: plan.plan.plan_id, confirmed: true as const, name: plan.plan.target_name };
    try {
      const outcome = await this.client.confirm(localId, body);
      if (request !== this.#request) return;
      this.outcome = outcome;
      this.announcement = { id: 'publication.state.verified' };
    } catch (error) {
      if (request !== this.#request) return;
      this.writeFailure = publicationFailure(error);
    } finally {
      if (request === this.#request) {
        this.#stopTimer();
        this.writing = false;
        this.waitingLong = false;
        this.check = null;
      }
      this.onjournal();
    }
  }

  /** «Проверить результат»: reads only, for the outcome on step 5. */
  async reconcile(): Promise<void> {
    const operationId = this.outcome?.operation_id;
    if (operationId === undefined || this.reconciling) return;
    const request = this.#request;
    this.reconciling = true;
    this.writeFailure = null;
    try {
      const outcome = await this.client.reconcile(operationId);
      if (request !== this.#request) return;
      this.outcome = outcome;
    } catch (error) {
      if (request !== this.#request) return;
      this.writeFailure = publicationFailure(error);
    } finally {
      if (request === this.#request) this.reconciling = false;
      this.onjournal();
    }
  }

  /** «Скопировать заново» (P2-04E): a new copy; this draft is not changed. */
  async refetch(name: string | null): Promise<RefetchResult | null> {
    const localId = this.localId;
    if (localId === null || this.refetching) return null;
    const request = this.#request;
    this.refetching = true;
    this.refetchFailure = null;
    try {
      const result = await this.client.refetch(localId, name);
      if (request !== this.#request) return null;
      this.refetched = result;
      return result;
    } catch (error) {
      if (request !== this.#request) return null;
      this.refetchFailure = publicationFailure(error);
      return null;
    } finally {
      if (request === this.#request) this.refetching = false;
    }
  }

  /** Back to «Проверки» with a new preview after an outcome or an invalidated plan. */
  restart(): void {
    if (this.writing) return;
    this.outcome = null;
    this.writeFailure = null;
    this.planFailure = null;
    this.step = 'checks';
    void this.recheck();
  }

  #stopTimer(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }
}

/** The journal of this process for the recovery banner (P1-01G §4): unfinished and damaged. */
export class RecoveryStore {
  list = $state<JournalList | null>(null);
  loadFailure = $state<PublicationFailure | null>(null);
  /** Operation ID of the running command. */
  busy = $state<string | null>(null);
  commandFailure = $state<{ readonly operationId: string; readonly failure: PublicationFailure } | null>(null);
  announcement = $state<LocalizedText | null>(null);
  /** «Журнал операции»: the entry shown in the dialog. */
  shown = $state<JournalEntry | null>(null);
  /** The last «Проверить результат» of the banner, announced with its state. */
  reconciled = $state<Outcome | null>(null);
  /** The last entry closed without checking. */
  lastClosed = $state<JournalEntry | null>(null);

  constructor(readonly client: PublicationClient) {}

  get count(): number {
    return (this.list?.unfinished.length ?? 0) + (this.list?.damaged.length ?? 0);
  }

  /** Entries of one draft (its banner in the workspace) or all of them. */
  forDraft(localId: string): JournalList {
    const list = this.list ?? { unfinished: [], damaged: [] };
    return {
      unfinished: list.unfinished.filter((entry) => entry.source?.local_id === localId),
      // A damaged file whose target is unknown blocks every publication, this draft's too.
      damaged: list.damaged.filter((damaged) => damaged.target === null)
    };
  }

  async load(): Promise<void> {
    try {
      this.list = await this.client.journal();
      this.loadFailure = null;
    } catch (error) {
      this.loadFailure = publicationFailure(error);
    }
  }

  async #run(operationId: string, command: () => Promise<LocalizedText | null>): Promise<boolean> {
    if (this.busy !== null) return false;
    this.busy = operationId;
    this.commandFailure = null;
    this.announcement = null;
    try {
      this.announcement = await command();
      await this.load();
      return true;
    } catch (error) {
      this.commandFailure = { operationId, failure: publicationFailure(error) };
      return false;
    } finally {
      this.busy = null;
    }
  }

  reconcile(operationId: string): Promise<boolean> {
    return this.#run(operationId, async () => {
      const outcome = await this.client.reconcile(operationId);
      if (this.shown?.operation_id === operationId && outcome.entry) this.shown = outcome.entry;
      this.reconciled = outcome;
      return null;
    });
  }

  close(operationId: string): Promise<boolean> {
    return this.#run(operationId, async () => {
      const entry = await this.client.close(operationId);
      if (this.shown?.operation_id === operationId) this.shown = entry;
      this.lastClosed = entry;
      return { id: 'startup.recovery.closed' };
    });
  }

  quarantine(operationId: string): Promise<boolean> {
    return this.#run(operationId, async () => {
      await this.client.quarantine(operationId);
      return { id: 'startup.recovery.quarantined' };
    });
  }

  async show(operationId: string): Promise<void> {
    try {
      this.shown = await this.client.entry(operationId);
    } catch (error) {
      this.commandFailure = { operationId, failure: publicationFailure(error) };
    }
  }
}
