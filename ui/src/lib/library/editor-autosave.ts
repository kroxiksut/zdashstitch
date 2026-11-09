/** One serial writer, trailing idle save and a maximum wait under continuous edits. */
export class EditorAutosave {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private firstDirty: number | null = null;
  private pending: Promise<boolean> | null = null;
  private failures = 0;
  private disposed = false;
  private blocked = false;
  enabled = true;
  private maxIntervalMs = 30_000;
  constructor(private readonly options: {
    dirty: () => boolean;
    save: () => Promise<void>;
    retryable: (error: unknown) => boolean;
    failed: (error: unknown, retrySeconds: number | null) => void;
  }) {}
  changed(): void {
    if (this.disposed || !this.enabled) return;
    if (!this.options.dirty()) { this.clear(); this.firstDirty = null; this.failures = 0; return; }
    this.blocked = false;
    this.firstDirty ??= Date.now();
    if (this.pending || this.failures > 0) return;
    this.schedule(Math.max(0, Math.min(2_000, this.firstDirty + this.maxIntervalMs - Date.now())));
  }
  setMaxInterval(seconds: 30 | 60 | 120): void {
    this.maxIntervalMs = seconds * 1_000;
    if (this.enabled && this.options.dirty()) this.changed();
  }
  setEnabled(enabled: boolean): void {
    this.enabled = enabled; this.clear();
    if (enabled) { this.failures = 0; this.changed(); }
  }
  flush(): Promise<boolean> {
    if (this.pending) return this.pending;
    this.blocked = false;
    this.clear();
    if (this.disposed || !this.options.dirty()) { this.failures = 0; this.firstDirty = null; return Promise.resolve(true); }
    this.pending = this.options.save().then(() => {
      this.failures = 0;
      if (!this.options.dirty()) this.firstDirty = null;
      return true;
    }).catch((error: unknown) => {
      const delay = this.enabled && this.options.retryable(error)
        ? [5_000, 15_000, 60_000][Math.min(this.failures++, 2)] : null;
      this.options.failed(error, delay === null ? null : delay / 1_000);
      this.blocked = delay === null;
      if (delay !== null && this.enabled && !this.disposed) this.schedule(delay);
      return false;
    }).finally(() => {
      this.pending = null;
      if (!this.blocked && this.failures === 0 && this.options.dirty()) this.changed();
    });
    return this.pending;
  }
  dispose(): void { this.disposed = true; this.clear(); }
  private clear(): void { if (this.timer !== null) clearTimeout(this.timer); this.timer = null; }
  private schedule(delay: number): void {
    this.clear(); this.timer = setTimeout(() => { this.timer = null; void this.flush(); }, delay);
  }
}
