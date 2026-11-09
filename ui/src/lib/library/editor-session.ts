import { ApiFailure } from '../api';
export type EditorAccess = 'claiming' | 'editing' | 'read_only' | 'unavailable';

/** Ownership is process-local and renewed; tokens remain in the client closure. */
export class PageEditingSession {
  private id: string | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private disposed = false;
  private generation = 0;
  constructor(private readonly client: {
    acquirePage: (id: string, takeOver?: boolean) => Promise<'editing' | 'read_only'>;
    renewPage: (id: string) => Promise<unknown>;
    releasePage: (id: string) => Promise<unknown>;
  }, private readonly changed: (state: EditorAccess) => void) {}
  async open(id: string, takeOver = false): Promise<void> {
    this.stop(); const generation = ++this.generation; this.id = id; this.changed('claiming');
    try {
      const result = await this.client.acquirePage(id, takeOver);
      if (generation !== this.generation) return;
      if (this.disposed) { if (result === 'editing') await this.client.releasePage(id); return; }
      this.changed(result);
      if (result === 'editing') this.timer = setInterval(() => { void this.check(); }, 20_000);
    } catch { if (!this.disposed) this.changed('unavailable'); }
  }
  async check(): Promise<void> {
    if (!this.id || this.disposed) return;
    const generation = this.generation;
    try { await this.client.renewPage(this.id); if (!this.disposed && generation === this.generation) this.changed('editing'); }
    catch (error) {
      if (this.disposed || generation !== this.generation) return;
      if (error instanceof ApiFailure && error.response?.code === 'offline_page_edit_lost') { this.stop(); this.changed('read_only'); }
      else this.changed('unavailable');
    }
  }
  lost(): void { this.stop(); if (!this.disposed) this.changed('read_only'); }
  dispose(): void {
    this.disposed = true; this.stop();
    if (this.id) void this.client.releasePage(this.id).catch(() => {});
  }
  private stop(): void { if (this.timer !== null) clearInterval(this.timer); this.timer = null; }
}
